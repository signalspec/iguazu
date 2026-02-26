use std::collections::hash_map;
use std::io;
use std::pin::Pin;
use std::sync::Mutex;
use std::{cell::RefCell, collections::{HashMap, HashSet}, sync::Arc, task::{Context, Poll, Waker}};
use std::fmt::Debug;

use log::debug;
use once_array::{OnceArray};
use async_executor::Task;
use elsa::FrozenMap;
use futures_lite::FutureExt;

use crate::import::ImportError;
use crate::izs::{self, CompressionMethod, FileMeta, Footer};
use crate::schema::{EntitySchema, EntityStream};
use crate::storage::Pool;
use crate::stream::{BlockDesc, IterState, Stream, StreamAccess, StreamIter, StreamState};
use crate::util::weak_map::WeakMap;
use crate::io::ReadableFile;

pub struct IzsFile {
    file: Arc<dyn ReadableFile>,
    tail: Vec<u8>,
    file_size: u64,
}

impl IzsFile {
    pub async fn new(file: Arc<dyn ReadableFile>) -> Result<Self, ImportError> {
        let (tail, file_size) = file.clone().read_at_end(1024 * 1024).await?;
        let s = Self { file, tail, file_size };
        s.footer()?;
        Ok(s)
    }

    async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>, std::io::Error> {
        let tail_offset = self.file_size.saturating_sub(self.tail.len() as u64);
        if let Some(pos) = offset.checked_sub(tail_offset) {
            debug!("Reading {len} bytes at {offset} from tail (pos {pos})");
            let end = (self.tail.len() as u64).min(pos + len as u64) as usize;
            return Ok(self.tail[pos as usize..end].to_vec());
        }

        self.file.clone().read_at(offset, len).await
    }

    fn footer(&self) -> Result<Footer, ImportError> {
        let footer_offset = self.tail.len().checked_sub(Footer::LEN)
            .ok_or_else(|| ImportError::InvalidFile("File too small for footer".to_string()))?;
        Footer::from_bytes(&self.tail[footer_offset..])
            .ok_or_else(|| ImportError::InvalidFile("Invalid footer".to_string()))
    }

    pub async fn load_meta(&self) -> Result<FileMeta, ImportError> {
        let footer = self.footer()?;
        let meta_pos = self.file_size.checked_sub(Footer::LEN as u64 + footer.meta_len as u64)
            .ok_or_else(|| ImportError::InvalidFile("Invalid metadata position".to_string()))?;
        let meta = self.read_at(meta_pos, footer.meta_len as usize).await?;
        let meta = zstd::bulk::decompress(&meta, 16 * 1024 * 1024)
            .map_err(|e| ImportError::InvalidFile(format!("Failed to decompress metadata: {}", e)))?;
        serde_json::from_slice(&meta).map_err(|e| ImportError::InvalidFile(format!("Invalid metadata: {}", e)))
    }

    pub async fn load_schema(&self) -> Result<EntitySchema, ImportError> {
        let meta = self.load_meta().await?;
        Ok(meta.entity.schema())
    }

    pub async fn load_entity(self: Arc<Self>, pool: Arc<Pool>) -> Result<EntityStream, ImportError> {
        let meta = self.load_meta().await?;

        meta.entity.try_map_data_async(move |s| {
            let shared = self.clone();
            let pool = pool.clone();
            async move {
                let index = shared.read_at(s.root, s.root_len as usize).await?;
                let (block_offsets, block_lengths) = izs::read_block_index(&index);

                let stream = IzsStream {
                    id: s.root,
                    shared,
                    pool,
                    block_desc: BlockDesc { element_size: s.element, count: s.block },
                    compress: s.compress,
                    block_offsets,
                    block_lengths,
                    pos: s.end_idx,
                    cache: Mutex::new(WeakMap::new()),
                };
                Ok(Arc::new(stream) as Arc<dyn Stream>)
            }
        }).await
    }
}

struct IzsStream {
    shared: Arc<IzsFile>,
    pool: Arc<Pool>,
    id: u64,

    block_desc: BlockDesc,

    compress: CompressionMethod,

    /// Positions of the compressed blocks within the file
    block_offsets: Vec<u64>,

    /// Lengths of the compressed blocks.
    block_lengths: Vec<u32>,

    /// Number of elements
    pos: u64,

    cache: Mutex<WeakMap<u64, OnceArray<u8>>>,
}

enum LoadBlockRes<F> {
    Loading(F),
    Cached(Arc<OnceArray<u8>>),
    NotFound,
}

impl IzsStream {
    fn load_block(self: Arc<Self>, block: u64) -> LoadBlockRes<impl Future<Output = Result<Arc<OnceArray<u8>>, io::Error>> + Send + 'static> {
        let Some(&offset) = self.block_offsets.get(block as usize) else {
            return LoadBlockRes::NotFound;
        };

        if let Some(buf) = self.cache.lock().unwrap().get(&block) {
            log::debug!("Block {block} of {}:{:x} found in cache", self.shared.file.filename().unwrap_or("<unknown>"), self.id);
            return LoadBlockRes::Cached(buf);
        }

        let block_bytes = self.block_desc.size();
        let compression = self.compress;
        let len = self.block_lengths[block as usize] as usize;
        log::debug!("Loading block {block} of {}:{:x} at {} len {}", self.shared.file.filename().unwrap_or("<unknown>"), self.id, offset, len);
        LoadBlockRes::Loading(async move {
            let data = self.shared.read_at(offset, len).await?;
            let data = match compression {
                CompressionMethod::None => data,
                CompressionMethod::Zstd => {
                    zstd::bulk::decompress(&data, block_bytes)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Failed to decompress block: {}", e)))?
                }
                CompressionMethod::Unknown => {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "Unknown compression method"));
                }
            };

            let buf = Arc::new(OnceArray::from(data.clone()));
            self.pool.cache.lock().unwrap().insert(buf.clone());
            self.cache.lock().unwrap().insert(block, buf.clone());
            Ok(buf)
        })
    }
}

impl Debug for IzsStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IzsStream")
            .field("file", &self.shared.file.filename().unwrap_or("<unknown>"))
            .finish()
    }
}

impl Stream for IzsStream {
    fn desc(&self) -> BlockDesc {
        self.block_desc
    }

    fn state(&self) -> StreamState {
        StreamState {
            streaming: false,
            end: self.pos,
        }
    }

    fn access(self: Arc<Self>) -> Box<dyn StreamAccess> {
        Box::new(IzsStreamAccess {
            stream: self.clone(),
            blocks: FrozenMap::new(),
            state: RefCell::new(IzsStreamAccessState {
                used: HashSet::new(),
                loading: HashMap::new(),
                error: None,
            }),
            waker: std::task::Waker::noop().clone(),
        })
    }

    fn iter(self: Arc<Self>) -> Pin<Box<dyn Future<Output = Result<Box<dyn StreamIter>, io::Error>> + Send + 'static>> {
        Box::pin(async move {
            Ok(Box::new(IzsStreamIter {
                stream: self.clone(),
                state: IzsIterState::Empty,
                block: 0,
                pos: 0,
            }) as Box<dyn StreamIter>)
        })
    }
}

struct IzsStreamAccess {
    stream: Arc<IzsStream>,
    blocks: FrozenMap<u64, Arc<OnceArray<u8>>>,
    state: RefCell<IzsStreamAccessState>,
    waker: Waker,
}

struct IzsStreamAccessState {
    used: HashSet<u64>,
    loading: HashMap<u64, Task<Result<Arc<OnceArray<u8>>, io::Error>>>,
    error: Option<io::Error>,
}

impl StreamAccess for IzsStreamAccess {
    fn get_block(&self, block: u64) -> &[u8] {
        let mut state = self.state.borrow_mut();
        state.used.insert(block);

        if let Some(buf) = self.blocks.get(&block) {
            // Block is already loaded
            return buf;
        }

        let is_error = state.error.is_some();
        let mut entry = match state.loading.entry(block) {
            hash_map::Entry::Occupied(entry) => entry,
            hash_map::Entry::Vacant(entry) => {
                if is_error {
                    return &[];
                }

                // Block is not loaded, start loading
                match self.stream.clone().load_block(block) {
                    LoadBlockRes::NotFound => {
                        return &[];
                    }
                    LoadBlockRes::Cached(buf) => {
                        return self.blocks.insert(block, buf)
                    }
                    LoadBlockRes::Loading(fut) => {
                        entry.insert_entry(self.stream.pool.executor.spawn(fut))
                    }
                }
            }
        };

        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(res) = entry.get_mut().poll(&mut cx) {
            drop(entry.remove());
            match res {
                Ok(buf) => {
                    log::debug!("Block {block} of {}:{:x} finished loading", self.stream.shared.file.filename().unwrap_or("<unknown>"), self.stream.id);
                    return self.blocks.insert(block, buf);
                }
                Err(e) => {
                    log::error!("Block {block} of {}:{:x} failed to load: {}", self.stream.shared.file.filename().unwrap_or("<unknown>"), self.stream.id, e);
                    state.error = Some(e);
                }
            }
        }

        return &[];
    }

    fn state(&self) -> StreamState {
        self.stream.state()
    }

    fn begin(&mut self, waker: &Waker) {
        self.waker.clone_from(waker);
    }

    fn end(&mut self) {
        let mut state = self.state.borrow_mut();
        self.blocks.as_mut().retain(|block, _| state.used.contains(block));
        state.used.clear();
    }
}

struct IzsStreamIter {
    stream: Arc<IzsStream>,
    state: IzsIterState,

    /// Block number
    block: u64,

    /// Position within block in elements
    pos: usize,
}

enum IzsIterState {
    Empty,
    Loading(Pin<Box<dyn Future<Output = Result<Arc<OnceArray<u8>>, io::Error>> + Send + 'static>>),
    Loaded(Arc<OnceArray<u8>>),
    Error(String),
}

impl StreamIter for IzsStreamIter {
    fn desc(&self) -> BlockDesc {
        self.stream.desc()
    }

    fn poll_next(&mut self, cx: &mut Context) -> IterState<'_> {
        loop {
            match self.state {
                IzsIterState::Empty => {
                    match self.stream.clone().load_block(self.block) {
                        LoadBlockRes::NotFound => return IterState::Complete(&[]),
                        LoadBlockRes::Cached(buf) => {
                            self.state = IzsIterState::Loaded(buf);
                        }
                        LoadBlockRes::Loading(fut) => {
                            self.state = IzsIterState::Loading(Box::pin(fut));
                        }
                    };
                }
                IzsIterState::Loading(ref mut fut) => {
                    match fut.as_mut().poll(cx) {
                        Poll::Ready(Ok(buf)) => {
                            self.state = IzsIterState::Loaded(buf);
                        }
                        Poll::Ready(Err(e)) => {
                            self.state = IzsIterState::Error(e.to_string());
                        }
                        Poll::Pending => return IterState::Partial(&[]),
                    }
                }
                IzsIterState::Loaded(ref buf) => {
                    return IterState::Complete(&buf[self.pos * self.stream.block_desc.element_size.bytes()..]);
                }
                IzsIterState::Error(ref err) => {
                    return IterState::Error(err)
                }
            }
        }
    }

    fn consume(&mut self, count: usize) {
        debug_assert!(self.pos + count <= self.stream.block_desc.count);
        self.pos += count;

        if self.pos >= self.stream.block_desc.count {
            self.block += 1;
            self.pos = 0;
            self.state = IzsIterState::Empty;
        }
    }
}
