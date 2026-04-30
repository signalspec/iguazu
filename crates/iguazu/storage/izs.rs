use std::io;
use std::pin::Pin;
use std::sync::Mutex;
use std::{sync::Arc, task::{Context, Poll}};
use std::fmt::Debug;

use once_array::OnceArray;

use crate::izs::{BlockIndex, CompressionMethod, IzsFile, StreamMeta};
use crate::storage::{Pool, common::{LoadBlock, LoadBlockRes, CommonStreamAccess}};
use crate::stream::{BlockDesc, IterState, Stream, StreamAccess, StreamIter, StreamState};
use crate::util::weak_map::WeakMap;

pub(crate) struct IzsStream {
    shared: Arc<IzsFile>,
    pool: Arc<Pool>,
    id: u64,

    block_desc: BlockDesc,
    compress: CompressionMethod,
    block_index: BlockIndex,

    /// Number of elements
    pos: u64,

    cache: Mutex<WeakMap<u64, OnceArray<u8>>>,
}

impl IzsStream {
    pub(crate) fn new(shared: Arc<IzsFile>, pool: Arc<Pool>, s: &StreamMeta, block_index: BlockIndex) -> IzsStream {
        IzsStream {
            shared,
            pool,
            id: s.root,
            block_desc: BlockDesc { element_size: s.element, count: s.block },
            compress: s.compress,
            block_index,
            pos: s.end_idx,
            cache: Mutex::new(WeakMap::new()),
        }
    }
}

impl LoadBlock for IzsStream {
    fn load_block(self: Arc<Self>, block: u64) -> LoadBlockRes {
        let Some(&offset) = self.block_index.offsets.get(block as usize) else {
            return LoadBlockRes::NotFound;
        };

        if let Some(buf) = self.cache.lock().unwrap().get(&block) {
            log::debug!("Block {block} of {}:{:x} found in cache", self.shared.filename().unwrap_or("<unknown>"), self.id);
            return LoadBlockRes::Cached(buf);
        }

        let block_bytes = self.block_desc.size();
        let compression = self.compress;
        let len = self.block_index.lengths[block as usize] as usize;
        log::debug!("Loading block {block} of {}:{:x} at {} len {}", self.shared.filename().unwrap_or("<unknown>"), self.id, offset, len);
        LoadBlockRes::Loading(self.pool.executor.clone().spawn(async move {
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
        }))
    }
}

impl Debug for IzsStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IzsStream")
            .field("file", &self.shared.filename().unwrap_or("<unknown>"))
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
        Box::new(CommonStreamAccess::new(self))
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
