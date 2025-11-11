use std::collections::hash_map;
use std::io;
use std::pin::Pin;
use std::{cell::RefCell, collections::{HashMap, HashSet}, sync::Arc, task::{Context, Poll, Waker}};
use std::fmt::Debug;

use append_array::{AppendArray};
use async_executor::{Executor, Task};
use elsa::FrozenMap;
use futures_lite::FutureExt;

use crate::import::ImportError;
use crate::izs::{self, CompressionMethod, FileMeta, Footer};
use crate::schema::EntityStream;
use crate::stream::{StreamAccess, StreamIter, Stream, StreamDesc, StreamState};
use crate::{io::ReadableFile, ElementType};

pub async fn load_meta(file: Arc<dyn ReadableFile>) -> Result<FileMeta, ImportError> {
    let len = file.clone().get_len().await?;
    let footer = file.clone().read_at(len.saturating_sub(Footer::LEN as u64), Footer::LEN).await?;
    let footer = Footer::from_bytes(&footer).ok_or_else(|| ImportError::InvalidFile("Invalid footer".to_string()))?;

    let meta_pos = len.checked_sub(Footer::LEN as u64 + footer.meta_len as u64)
        .ok_or_else(|| ImportError::InvalidFile("Invalid metadata position".to_string()))?;
    let meta = file.clone().read_at(meta_pos, footer.meta_len as usize).await?;
    let meta = zstd::bulk::decompress(&meta, 16 * 1024 * 1024)
        .map_err(|e| ImportError::InvalidFile(format!("Failed to decompress metadata: {}", e)))?;
    serde_json::from_slice(&meta).map_err(|e| ImportError::InvalidFile(format!("Invalid metadata: {}", e)))
}

pub async fn load(file: Arc<dyn ReadableFile>, executor: Arc<Executor<'static>>) -> Result<EntityStream, ImportError> {
    let meta = load_meta(file.clone()).await?;
    let shared = Arc::new(Shared { file, executor });

    meta.entity.try_map_data_async(move |s| {
        let shared = shared.clone();
        async move {
            let index = shared.read_at(s.root, s.root_len as usize).await?;
            let (block_offsets, block_lengths) = izs::read_block_index(&index);

            let stream = IzsStream {
                id: s.root,
                shared: shared,
                block_size: s.block_size,
                element_type: s.element_type,
                compress: s.compress,
                block_offsets,
                block_lengths,
                pos: s.end_idx,
            };
            Ok(Arc::new(stream) as Arc<dyn Stream>)
        }
    }).await
}

pub struct Shared {
    file: Arc<dyn ReadableFile>,
    executor: Arc<Executor<'static>>,
}

impl Shared {
    async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>, std::io::Error> {
        self.file.clone().read_at(offset, len).await
    }
}

struct IzsStream {
    shared: Arc<Shared>,
    id: u64,

    element_type: ElementType,
    block_size: usize,

    compress: CompressionMethod,

    /// Positions of the compressed blocks within the file
    block_offsets: Vec<u64>,

    /// Lengths of the compressed blocks.
    block_lengths: Vec<u32>,

    /// Number of elements
    pos: u64,
}

enum LoadBlockRes<F> {
    Loading(F),
    NotFound,
}

impl IzsStream {
    fn load_block(&self, block: u64) -> LoadBlockRes<impl Future<Output = Result<Arc<AppendArray<u8>>, io::Error>> + Send + 'static> {
        if let Some(&offset) = self.block_offsets.get(block as usize) {
            let block_bytes = self.block_size * self.element_type.bytes();
            let compression = self.compress;
            let len = self.block_lengths[block as usize] as usize;
            let shared = self.shared.clone();
            log::debug!("Loading block {block} of {}:{:x} at {} len {}", self.shared.file.filename().unwrap_or("<unknown>"), self.id, offset, len);
            LoadBlockRes::Loading(async move {
                let data = shared.read_at(offset, len).await?;
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
                Ok(Arc::new(AppendArray::from(data)))
            })
        } else {
            LoadBlockRes::NotFound
        }
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
    fn desc(&self) -> StreamDesc {
        StreamDesc {
            element_type: self.element_type,
            block_size: self.block_size
        }
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

    fn iter(self: Arc<Self>) -> Box<dyn StreamIter> {
        Box::new(IzsStreamIter {
            stream: self.clone(),
            state: IterState::Empty,
            block: 0,
            pos: 0,
        })
    }
}

struct IzsStreamAccess {
    stream: Arc<IzsStream>,
    blocks: FrozenMap<u64, Arc<AppendArray<u8>>>,
    state: RefCell<IzsStreamAccessState>,
    waker: Waker,
}

struct IzsStreamAccessState {
    used: HashSet<u64>,
    loading: HashMap<u64, Task<Result<Arc<AppendArray<u8>>, io::Error>>>,
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
                match self.stream.load_block(block) {
                    LoadBlockRes::NotFound => {
                        return &[];
                    }
                    LoadBlockRes::Loading(fut) => {
                        entry.insert_entry(self.stream.shared.executor.spawn(fut))
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
    state: IterState,

    /// Block number
    block: u64,

    /// Position within block
    pos: usize,
}

enum IterState {
    Empty,
    Loading(Pin<Box<dyn Future<Output = Result<Arc<AppendArray<u8>>, io::Error>> + Send + 'static>>),
    Loaded(Arc<AppendArray<u8>>),
}

impl StreamIter for IzsStreamIter {
    fn element_type(&self) -> ElementType {
        self.stream.element_type
    }

    fn poll_next(&mut self, cx: &mut Context) -> Poll<Result<&[u8], String>> {
        loop {
            match self.state {
                IterState::Empty => {
                    self.state = match self.stream.load_block(self.block) {
                        LoadBlockRes::NotFound => return Poll::Ready(Ok(&[])),
                        LoadBlockRes::Loading(fut) => IterState::Loading(Box::pin(fut)),
                    };
                }
                IterState::Loading(ref mut fut) => {
                    match fut.as_mut().poll(cx) {
                        Poll::Ready(Ok(buf)) => {
                            self.state = IterState::Loaded(buf);
                            continue;
                        }
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e.to_string())),
                        Poll::Pending => return Poll::Pending,
                    }
                }
                IterState::Loaded(ref buf) => {
                    return Poll::Ready(Ok(&buf[self.pos * self.stream.element_type.bytes()..]));
                }
            }
        }
    }

    fn consume(&mut self, len: usize) {
        debug_assert!(self.pos + len <= self.stream.block_size);
        self.pos += len;

        if self.pos >= self.stream.block_size {
            self.block += 1;
            self.pos = 0;
            self.state = IterState::Empty;
        }
    }
}
