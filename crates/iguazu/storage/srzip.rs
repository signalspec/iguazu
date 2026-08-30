use std::{fmt::Debug, pin::Pin, sync::Arc, task::{Context, Poll, Waker}};
use ecow::eco_format;
use futures_lite::AsyncRead;

use itertools::{AllEqualValueError, Itertools};
use once_array::OnceArray;

use crate::{
    ElementSize, import::ImportError, io::{IoError, zip::{UnzipReader, ZipEntry}}, storage::{BlockState, Pool, block::BlockCache}, stream::{BlockDesc, IterState, Stream, StreamIter, StreamState},
};

pub struct SrZipStream {
    blocks: Vec<ZipEntry>,
    block_size: usize,
    element_size: ElementSize,
    cache: BlockCache,
    pool: Arc<Pool>,
}

impl SrZipStream {
    pub(crate) fn new(pool: Arc<Pool>, blocks: Vec<ZipEntry>, element_size: ElementSize) -> Result<Self, ImportError> {
        let Some(block_size) = infer_block_size(blocks.iter().map(|s| s.uncompressed_size() / element_size.bytes() as u64)) else {
            // TODO: could make the iterator work on such files to allow converting them, just can't support random access
            return Err(ImportError::InvalidFile("unsupported block size".to_string()));
        };
        Ok(Self { blocks, block_size, element_size, cache: BlockCache::new(), pool })
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

    fn get_block(&self, block_i: u64, waker: &Waker) -> BlockState {
        if let Some(block) = self.cache.get(block_i, waker) {
            log::debug!("Block {block_i} found in cache");
            return block;
        }

        let Some(entry) = self.blocks.get(block_i as usize) else {
            return BlockState::End
        };

        let file_name = entry.file().filename().unwrap_or("<unknown>");
        let block_bytes = self.desc().size();
        log::debug!("Loading block {block_i} from {} : {}", file_name, entry.name());
        let entry = entry.clone();
        let task = self.pool.clone().spawn_block_fetch(async move {
            let data = entry.read_all(block_bytes).await?;
            Ok(Arc::new(OnceArray::from(data.clone())))
        });

        task.subscribe(waker);
        self.cache.insert_task(block_i, task);
        BlockState::Pending
    }

    fn iter(self: Arc<Self>) -> Box<dyn StreamIter> {
        Box::new(SrZipIter::new(self))
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
    Loading(Pin<Box<dyn Future<Output = Result<UnzipReader, IoError>> + Send + 'static>>),
    Reading(Pin<Box<UnzipReader>>),
    Error(IoError),
}

impl SrZipIter {
    fn new(stream: Arc<SrZipStream>) -> Self {
        Self {
            block: 0,
            state: SrZipIterState::Empty,
            buffer: vec![0; stream.block_size * stream.element_size.bytes()],
            in_pos: 0,
            pos: 0,
            stream,
        }
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
                        log::debug!("Loading block {} from {} : {}", self.block, file_name, entry.name());
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
                            self.state = SrZipIterState::Error(e);
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
                                self.state = SrZipIterState::Error(IoError::new(eco_format!("failed to read file: {e}"), None, Some(e.kind())));
                            }
                            Poll::Pending => break,
                        }
                    } else {
                        return IterState::Complete(&self.buffer[read_offset..self.in_pos]);
                    }
                }
                SrZipIterState::Error(ref err) => {
                    return IterState::Error(err.clone())
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
