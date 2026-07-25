use std::{fmt::Debug, io, pin::Pin, sync::{Arc, Mutex}, task::{Context, Poll}};
use futures_lite::AsyncRead;

use itertools::{AllEqualValueError, Itertools};
use once_array::OnceArray;

use crate::{ElementSize, import::ImportError, io::zip::{UnzipReader, ZipEntry}, storage::{Pool, common::{CommonStreamAccess, LoadBlock, LoadBlockRes}}, stream::{BlockDesc, IterState, Stream, StreamAccess, StreamIter, StreamState}, util::weak_map::WeakMap};

pub struct SrZipStream {
    blocks: Vec<ZipEntry>,
    block_size: usize,
    element_size: ElementSize,
    cache: Mutex<WeakMap<u64, OnceArray<u8>>>,
    pool: Arc<Pool>,
}

impl SrZipStream {
    pub(crate) fn new(pool: Arc<Pool>, blocks: Vec<ZipEntry>, element_size: ElementSize) -> Result<Self, ImportError> {
        let Some(block_size) = infer_block_size(blocks.iter().map(|s| s.uncompressed_size() / element_size.bytes() as u64)) else {
            // TODO: could make the iterator work on such files to allow converting them, just can't support random access
            return Err(ImportError::InvalidFile("unsupported block size".to_string()));
        };
        Ok(Self { blocks, block_size, element_size, cache: Mutex::new(WeakMap::new()), pool })
    }
}

impl Debug for SrZipStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SrZip")
    }
}

impl Stream for SrZipStream {
    fn desc(&self) -> BlockDesc {
        BlockDesc { element_size: self.element_size, count: self.block_size }
    }

    fn state(&self) -> StreamState {
        let end = self.blocks.iter().map(|b| b.uncompressed_size() / self.element_size.bytes() as u64).sum();
        StreamState { end, streaming: false }
    }

    fn access(self: Arc<Self>) -> Box<dyn StreamAccess> {
        Box::new(CommonStreamAccess::new(self))
    }

    fn iter(self: Arc<Self>) -> Pin<Box<dyn Future<Output = Result<Box<dyn StreamIter>, io::Error>> + Send + 'static>> {
        Box::pin(SrZipIter::new(self))
    }
}

impl LoadBlock for SrZipStream {
    fn load_block(self: Arc<Self>, block: u64) -> LoadBlockRes {
        let Some(entry) = self.blocks.get(block as usize) else {
            return LoadBlockRes::NotFound;
        };

        let file_name = entry.file().filename().unwrap_or("<unknown>");
        let entry_name = str::from_utf8(entry.name()).unwrap_or("<unknown>");

        if let Some(buf) = self.cache.lock().unwrap().get(&block) {
            log::debug!("Block {block} from {} : {} found in cache", file_name, entry_name);
            return LoadBlockRes::Cached(buf);
        }

        let block_bytes = self.desc().size();
        log::debug!("Loading block {block} from {} : {}", file_name, entry_name);
        LoadBlockRes::Loading(self.pool.executor.clone().spawn(async move {
            let entry = &self.blocks[block as usize];
            let data = entry.read_all(block_bytes).await?;
            let buf = Arc::new(OnceArray::from(data.clone()));
            self.pool.cache.lock().unwrap().insert(buf.clone());
            self.cache.lock().unwrap().insert(block, buf.clone());
            Ok(buf)
        }))
    }
}

struct SrZipIter {
    stream: Arc<SrZipStream>,

    /// Next block number to read (this advances when reads finish, even if not consumed yet)
    block: usize,

    state: SrZipIterState,
    buffer: Vec<u8>,

    /// Next write position in buffer in bytes (buffer[..in_pos] is valid)
    in_pos: usize,

    /// Position within block in elements
    pos: usize,
}

enum SrZipIterState {
    Empty,
    Loading(Pin<Box<dyn Future<Output = Result<UnzipReader, io::Error>> + Send + 'static>>),
    Reading(Pin<Box<UnzipReader>>),
    Error(String),
}

impl SrZipIter {
    fn new(stream: Arc<SrZipStream>) -> Pin<Box<dyn Future<Output = Result<Box<dyn StreamIter>, io::Error>> + Send + 'static>> {
        let block_size = stream.block_size;
        let element_size = stream.element_size;
        Box::pin(async move {
            Ok(Box::new(SrZipIter {
                stream,
                block: 0,
                state: SrZipIterState::Empty,
                buffer: vec![0; block_size * element_size.bytes()],
                in_pos: 0,
                pos: 0,
            }) as Box<dyn StreamIter>)
        })
    }
}

impl StreamIter for SrZipIter {
    fn desc(&self) -> BlockDesc {
        self.stream.desc()
    }

    fn poll_next(&mut self, cx: &mut Context) -> IterState<'_> {
        let read_offset = self.pos * self.stream.element_size.bytes();
        loop {
            match self.state {
                SrZipIterState::Empty => {
                    if let Some(entry) = self.stream.blocks.get(self.block) {
                        let file_name = entry.file().filename().unwrap_or("<unknown>");
                        let entry_name = str::from_utf8(entry.name()).unwrap_or("<unknown>");
                        log::debug!("Loading block {} from {} : {}", self.block, file_name, entry_name);
                        self.state = SrZipIterState::Loading(Box::pin(entry.reader()));
                    } else {
                         return IterState::Complete(&self.buffer[read_offset..self.in_pos]);
                    }
                }
                SrZipIterState::Loading(ref mut fut) => {
                    match fut.as_mut().poll(cx) {
                        Poll::Ready(Ok(reader)) => {
                            self.state = SrZipIterState::Reading(Box::pin(reader));
                        }
                        Poll::Ready(Err(e)) => {
                            self.state = SrZipIterState::Error(e.to_string());
                        }
                        Poll::Pending => break,
                    }
                }
                SrZipIterState::Reading(ref mut reader) => {
                    if self.in_pos < self.buffer.len() {
                        match reader.as_mut().poll_read(cx, &mut self.buffer[self.in_pos..]) {
                            Poll::Ready(Ok(0)) => {
                                self.block += 1;
                                self.state = SrZipIterState::Empty;
                            }
                            Poll::Ready(Ok(read)) => {
                                self.in_pos += read;
                            }
                            Poll::Ready(Err(e)) => {
                                self.state = SrZipIterState::Error(e.to_string());
                            }
                            Poll::Pending => break,
                        }
                    } else {
                        return IterState::Complete(&self.buffer[read_offset..self.in_pos]);
                    }
                }
                SrZipIterState::Error(ref err) => {
                    return IterState::Error(err)
                }
            }
        }
        IterState::Partial(&self.buffer[read_offset..self.in_pos])
    }

    fn consume(&mut self, count: usize) {
        debug_assert!(self.pos * self.stream.element_size.bytes() + count <= self.buffer.len());
        self.pos += count;

        if self.pos * self.stream.element_size.bytes() >= self.buffer.len() {
            self.pos = 0;
            self.in_pos = 0;
        }
    }
}

fn infer_block_size(mut sizes: impl DoubleEndedIterator<Item = u64>) -> Option<usize> {
    let last = sizes.next_back()?;
    match sizes.all_equal_value() {
        Ok(s) if s.is_power_of_two() && last <= s => usize::try_from(s).ok(),
        Err(AllEqualValueError(None)) if last <= 16 * 1024 * 1024 => {
            Some(usize::try_from(last).unwrap().next_power_of_two())
        },
        _ => None
    }
}

#[test]
fn test_infer_block_size() {
    assert_eq!(infer_block_size([].into_iter()), None);
    assert_eq!(infer_block_size([1024, 2048].into_iter()), None);
    assert_eq!(infer_block_size([2048, 1024, 500].into_iter()), None);
    assert_eq!(infer_block_size([1024, 1024].into_iter()), Some(1024));
    assert_eq!(infer_block_size([2048, 2048, 300].into_iter()), Some(2048));
    assert_eq!(infer_block_size([4 * 1024 * 1024].into_iter()), Some(4096 * 1024));
    assert_eq!(infer_block_size([4_000_000].into_iter()), Some(4096 * 1024));
    assert_eq!(infer_block_size([4_000_000, 4_000_000].into_iter()), None);
    assert_eq!(infer_block_size([20 * 1024 * 1024].into_iter()), None);
}
