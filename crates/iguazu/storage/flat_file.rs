use std::{fmt::Debug, future::Future, pin::Pin, sync::Arc, task::{Context, Poll, ready}};

use ecow::{EcoString, eco_format};
use futures_lite::{AsyncBufRead, AsyncRead, FutureExt};
use once_array::OnceArray;
use url::Url;

use crate::{ElementSize, import::ImportError, io::{IoError, ReadableFile}, schema::{EntitySchema, EntityStream}, stream::{BlockDesc, IterState, Stream, StreamIter, StreamState}};
use crate::storage::{ Pool, block::BlockCache };

use super::BlockState;

#[derive(Clone, Debug)]
pub struct FlatFileOpts {
    pub offset: u64,
    pub count: Option<u64>,
    pub block_size: usize,
}

impl Default for FlatFileOpts {
    fn default() -> Self {
        FlatFileOpts {
            offset: 0,
            count: None,
            block_size: 1 << 20,
        }
    }
}

pub struct FlatFileStream {
    file: Arc<dyn ReadableFile>,
    file_len: u64,
    offset: u64,
    count: Option<u64>,
    block_desc: BlockDesc,
    cache: BlockCache,
    pool: Arc<Pool>,
}

impl Debug for FlatFileStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FlatFileStream")
            .field(&self.file.filename().unwrap_or("<unknown>"))
            .finish()
    }
}

impl FlatFileStream {
    pub async fn new(file: Arc<dyn ReadableFile>, pool: Arc<Pool>, element_size: ElementSize, opts: &FlatFileOpts) -> Result<Self, IoError> {
        let file_len = file.clone().get_len().await?;
        let block_desc = BlockDesc { element_size, count: opts.block_size };
        let cache = BlockCache::new();
        Ok(FlatFileStream { file, file_len, offset: opts.offset, count: opts.count, block_desc, cache, pool })
    }

    pub fn url(&self) -> Option<Url> {
        self.file.url()
    }

    pub fn element_size(&self) -> ElementSize {
        self.block_desc.element_size
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn count(&self) -> Option<u64> {
        self.count
    }

    pub async fn entity(file: Arc<dyn ReadableFile>, pool: Arc<Pool>, element_size: ElementSize, schema: EntitySchema, opts: &FlatFileOpts) -> Result<EntityStream, ImportError> {
        let (_field, _stride) = schema.single_stream()
            .ok_or_else(|| ImportError::SchemaMismatch("FlatFileStream requires a single stream".into()))?;

        let stream = Self::new(file, pool, element_size, opts).await.map_err(ImportError::Io)?;
        Ok(schema.wrap_single(Arc::new(stream)).unwrap())
    }

    fn load_block_uncached(&self, block: u64) -> impl Future<Output = Result<Arc<OnceArray<u8>>, IoError>> + Send + 'static {
        log::debug!("Loading block {block} of {}", self.file.filename().unwrap_or("<unknown>"));
        let offset = block.saturating_mul(self.block_desc.size() as u64);
        let file_offset = self.offset.saturating_add(offset);
        let size = self.count.map(|c| (c * (self.block_desc.element_size.bytes() as u64)).saturating_sub(offset))
            .unwrap_or(u64::MAX)
            .min(self.block_desc.size() as u64) as usize;
        let file = self.file.clone();
        async move {
            let data = file.read_at(file_offset, size).await?;
            Ok(Arc::new(OnceArray::from(data)))
        }
    }
}

impl Stream for FlatFileStream {
    fn desc(&self) -> BlockDesc {
        self.block_desc
    }

    fn state(&self) -> StreamState {
        let end = self.count.unwrap_or(u64::MAX)
            .min(self.file_len.saturating_sub(self.offset) / (self.block_desc.element_size.bytes() as u64));
        StreamState {
            streaming: false,
            end,
        }
    }

    fn get_block(&self, block_num: u64, waker: &std::task::Waker) -> BlockState {
        if let Some(block) = self.cache.get(block_num, waker) {
            block
        } else {
            let task = self.pool.clone().spawn_block_fetch(self.load_block_uncached(block_num));
            task.subscribe(waker);
            self.cache.insert_task(block_num, task);
            BlockState::Pending
        }
    }

    fn iter(self: Arc<Self>) -> Box<dyn StreamIter> {
        Box::new(FileStreamIter::from_file(self.file.clone(), self.offset, self.block_desc, self.count))
    }
}

struct FileStreamIter {
    file: EcoString,
    block_desc: BlockDesc,
    reader_future: FutureOutput<>,
    buffer: Box<[u8]>,

    /// Position within block in bytes. Always <= buffer.len().
    write_offset: usize,

    /// Position within block in bytes. Always <= write_offset.
    read_offset: usize,
    end: Option<Result<(), IoError>>,
}

enum FutureOutput {
    Future(Pin<Box<dyn Future<Output = Result<Pin<Box<dyn AsyncBufRead + Send + Sync>>, IoError>> + Send>>),
    Reader(Result<Pin<Box<dyn AsyncRead + Send + Sync>>, IoError>)
}

impl FutureOutput {
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<&mut Result<Pin<Box<dyn AsyncRead + Send + Sync>>, IoError>> {
        if let FutureOutput::Future(f) = self {
            let result = ready!(f.poll(cx));
            *self = FutureOutput::Reader(result.map(|r| r as _));
        }
        if let FutureOutput::Reader(o) = self {
            Poll::Ready(o)
        } else {
            unreachable!()
        }
    }
}

impl FileStreamIter {
    fn from_file(file: Arc<dyn ReadableFile>, offset: u64, block_desc: BlockDesc, count: Option<u64>) -> FileStreamIter {
        let size = count.map(|c| c * (block_desc.element_size.bytes() as u64));
        let reader_future = file.clone().stream(offset, size);
        FileStreamIter::new(file.describe(), FutureOutput::Future(reader_future), block_desc)
    }

    #[cfg(test)]
    fn from_reader(name: EcoString, reader: Pin<Box<dyn AsyncRead + Send + Sync>>, block_desc: BlockDesc) -> FileStreamIter {
        FileStreamIter::new(name, FutureOutput::Reader(Ok(reader)), block_desc)
    }

    fn new(name: EcoString, reader_future: FutureOutput, block_desc: BlockDesc) -> FileStreamIter {
        FileStreamIter {
            file: name,
            block_desc,
            reader_future,
            buffer: vec![0; block_desc.size()].into_boxed_slice(),
            read_offset: 0,
            write_offset: 0,
            end: None,
        }
    }
}

impl StreamIter for FileStreamIter {
    fn desc(&self) -> BlockDesc {
        self.block_desc
    }

    fn poll_next(&mut self, cx: &mut Context) -> IterState<'_> {
        let element_size = self.block_desc.element_size.bytes();

        let file_stream = match self.reader_future.poll(cx) {
            Poll::Pending => return IterState::Partial(&[]),
            Poll::Ready(Ok(file_stream)) => file_stream,
            Poll::Ready(Err(e)) => return IterState::Error(e.clone()),
        };

        while self.write_offset < self.buffer.len() && self.end.is_none() {
            match file_stream.as_mut().poll_read(cx, &mut self.buffer[self.write_offset..]) {
                Poll::Pending => break,
                Poll::Ready(Ok(0)) => {
                    self.end = Some(Ok(()));
                }
                Poll::Ready(Ok(read)) => {
                    self.write_offset += read;
                }
                Poll::Ready(Err(e)) => {
                    self.end = Some(Err(IoError::new(eco_format!("failed to read file: {e}"), Some(self.file.clone()), Some(e.kind()))));
                }
            }
        }

        let available = &self.buffer[self.read_offset..(self.write_offset / element_size * element_size)];

        if available.is_empty() && let Some(Err(err)) = &self.end {
            IterState::Error(err.clone())
        } else if self.write_offset < self.buffer.len() && self.end.is_none() {
            IterState::Partial(available)
        } else {
            IterState::Complete(available)
        }
    }

    fn consume(&mut self, len: usize) {
        debug_assert!(self.write_offset - self.read_offset >= len);
        self.read_offset += len * self.block_desc.element_size.bytes();
        if self.read_offset >= self.buffer.len() {
            self.read_offset = 0;
            self.write_offset = 0;
        }
    }
}

#[test]
fn test_iter() {
    use std::{ io, task::Waker, assert_matches };
    use futures_lite::{future::block_on, AsyncRead};
    use ecow::EcoString;
    let cx = &mut Context::from_waker(&Waker::noop());

    struct Replay(std::slice::Iter<'static, Poll<Result<&'static [u8], &'static str>>>);

    impl AsyncRead for Replay {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            match self.get_mut().0.next() {
                Some(Poll::Ready(Ok(r))) => {
                    buf[..r.len()].copy_from_slice(r);
                    Poll::Ready(Ok(r.len()))
                }
                Some(Poll::Pending) => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Some(Poll::Ready(Err(e))) => Poll::Ready(Err(std::io::Error::other(*e))),
                None => Poll::Ready(Ok(0),)
            }
        }
    }

    let src = Box::pin(Replay(const { &[
        Poll::Pending,
        Poll::Ready(Ok(&[1u8, 2] as &[u8])),
        Poll::Pending,
        Poll::Ready(Ok(&[3])),
        Poll::Pending,
        Poll::Ready(Ok(&[4])),
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(Ok(&[5, 6, 7, 8])),
        Poll::Ready(Ok(&[9, 10, 11, 12, 13, 14, 15, 16])),
        Poll::Ready(Ok(&[16, 17]))
    ]}.iter()));
    let mut iter = FileStreamIter::from_reader(EcoString::default(), src, BlockDesc { element_size: ElementSize::U16, count: 4 });
    assert_matches!(iter.poll_next(cx), IterState::Partial(&[]));
    assert_matches!(iter.poll_next(cx), IterState::Partial(&[1, 2]));
    assert_matches!(iter.poll_next(cx), IterState::Partial(&[1, 2]));
    assert_matches!(iter.poll_next(cx), IterState::Partial(&[1, 2, 3, 4]));
    assert_matches!(iter.poll_next(cx), IterState::Partial(&[1, 2, 3, 4]));
    iter.consume(1);
    assert_matches!(iter.poll_next(cx), IterState::Complete(&[3, 4, 5, 6, 7, 8]));
    iter.consume(1);
    assert_matches!(iter.poll_next(cx), IterState::Complete(&[5, 6, 7, 8]));
    iter.consume(2);
    assert_matches!(iter.poll_next(cx), IterState::Complete(&[9, 10, 11, 12, 13, 14, 15, 16]));
    iter.consume(4);
    assert_matches!(iter.poll_next(cx), IterState::Complete(&[16, 17]));
    iter.consume(1);
    assert_matches!(iter.poll_next(cx), IterState::Complete(&[]));

    let src = Box::pin(Replay(const { &[
        Poll::Ready(Ok(&[1u8, 2, 3] as &[u8])),
        Poll::Ready(Err("err")),
    ]}.iter()));
    let mut iter = FileStreamIter::from_reader(EcoString::default(), src, BlockDesc { element_size: ElementSize::U16, count: 4 });
    assert_matches!(iter.poll_next(cx), IterState::Complete(&[1, 2]));
    iter.consume(1);
    assert_matches!(iter.poll_next(cx), IterState::Error(_));

    let src = Box::pin(Replay(const { &[
        Poll::Ready(Ok(&[1u8, 2, 3, 4] as &[u8])),
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(Ok(&[5, 6])),
        Poll::Pending,
    ]}.iter()));
    let mut iter = Box::new(FileStreamIter::from_reader(EcoString::default(), src, BlockDesc { element_size: ElementSize::U8, count: 4 })) as Box<dyn StreamIter>;
    assert_matches!(block_on(iter.read_to_vec(10)), Ok(v) if v == [1u8, 2, 3, 4, 5, 6]);
}
