use std::{fmt::Debug, future::Future, io, pin::Pin, sync::{Arc, Mutex}, task::{Context, Poll}};

use futures_lite::AsyncRead;
use once_array::OnceArray;
use url::Url;

use crate::{ElementSize, import::ImportError, io::ReadableFile, schema::{EntitySchema, EntityStream}, stream::{BlockDesc, IterState, Stream, StreamAccess, StreamIter, StreamState}, util::weak_map::WeakMap};
use crate::storage::{Pool, common::{LoadBlock, LoadBlockRes, CommonStreamAccess}};

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
    cache: Mutex<WeakMap<u64, OnceArray<u8>>>,
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
    pub async fn new(file: Arc<dyn ReadableFile>, pool: Arc<Pool>, element_size: ElementSize, opts: &FlatFileOpts) -> Result<Self, io::Error> {
        let file_len = file.clone().get_len().await?;
        let block_desc = BlockDesc { element_size, count: opts.block_size };
        let cache = Mutex::new(WeakMap::new());
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

    fn load_block_uncached(&self, block: u64) -> impl Future<Output = Result<Vec<u8>, io::Error>> + Send + 'static {
        log::debug!("Loading block {block} of {}", self.file.filename().unwrap_or("<unknown>"));
        let offset = block.saturating_mul(self.block_desc.size() as u64);
        let file_offset = self.offset.saturating_add(offset);
        let size = self.count.map(|c| (c * (self.block_desc.element_size.bytes() as u64)).saturating_sub(offset))
            .unwrap_or(u64::MAX)
            .min(self.block_desc.size() as u64) as usize;
        self.file.clone().read_at(file_offset, size)
    }
}

impl LoadBlock for FlatFileStream {
    fn load_block(self: Arc<Self>, block: u64) -> LoadBlockRes {
        if let Some(entry) = self.cache.lock().unwrap().get(&block) {
            return LoadBlockRes::Cached(entry);
        }

        LoadBlockRes::Loading(self.pool.executor.clone().spawn(async move {
            let buf = self.load_block_uncached(block).await?;
            let entry = Arc::new(OnceArray::from(buf));
            self.pool.cache.lock().unwrap().insert(entry.clone());
            self.cache.lock().unwrap().insert(block, entry.clone());
            Ok(entry)
        }))
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

    fn access(self: Arc<Self>) -> Box<dyn StreamAccess> {
        Box::new(CommonStreamAccess::new(self))
    }

    fn iter(self: Arc<Self>) -> Pin<Box<dyn Future<Output = Result<Box<dyn StreamIter>, io::Error>> + Send + 'static>> {
        Box::pin(async move {
            let size = self.count.map(|c| c * (self.block_desc.element_size.bytes() as u64));
            let reader = self.file.clone().stream(self.offset, size).await?;
            Ok(Box::new(FileStreamIter::new(self.block_desc, reader)) as Box<dyn StreamIter>)
        })
    }
}

struct FileStreamIter {
    block_desc: BlockDesc,
    file_stream: Pin<Box<dyn AsyncRead + Send + Sync>>,
    buffer: Box<[u8]>,

    /// Position within block in bytes. Always <= buffer.len().
    write_offset: usize,

    /// Position within block in bytes. Always <= write_pos.
    read_offset: usize,
    end: Option<Result<(), String>>,
}

impl FileStreamIter {
    fn new(block_desc: BlockDesc, file_stream: Pin<Box<dyn AsyncRead + Send + Sync>>) -> FileStreamIter {
        FileStreamIter {
            block_desc,
            file_stream,
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
        while self.write_offset < self.buffer.len() && self.end.is_none() {
            match self.file_stream.as_mut().poll_read(cx, &mut self.buffer[self.write_offset..]) {
                Poll::Pending => break,
                Poll::Ready(Ok(0)) => {
                    self.end = Some(Ok(()));
                }
                Poll::Ready(Ok(read)) => {
                    self.write_offset += read;
                }
                Poll::Ready(Err(e)) => {
                    self.end = Some(Err(e.to_string()));
                }
            }
        }

        let available = &self.buffer[self.read_offset..(self.write_offset / element_size * element_size)];

        if available.is_empty() && let Some(Err(err)) = &self.end {
            IterState::Error(err)
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
    use std::task::Waker;
    use futures_lite::future::block_on;
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
    let mut iter = FileStreamIter::new(BlockDesc { element_size: ElementSize::U16, count: 4 }, src);
    assert_eq!(iter.poll_next(cx), IterState::Partial(&[]));
    assert_eq!(iter.poll_next(cx), IterState::Partial(&[1, 2]));
    assert_eq!(iter.poll_next(cx), IterState::Partial(&[1, 2]));
    assert_eq!(iter.poll_next(cx), IterState::Partial(&[1, 2, 3, 4]));
    assert_eq!(iter.poll_next(cx), IterState::Partial(&[1, 2, 3, 4]));
    iter.consume(1);
    assert_eq!(iter.poll_next(cx), IterState::Complete(&[3, 4, 5, 6, 7, 8]));
    iter.consume(1);
    assert_eq!(iter.poll_next(cx), IterState::Complete(&[5, 6, 7, 8]));
    iter.consume(2);
    assert_eq!(iter.poll_next(cx), IterState::Complete(&[9, 10, 11, 12, 13, 14, 15, 16]));
    iter.consume(4);
    assert_eq!(iter.poll_next(cx), IterState::Complete(&[16, 17]));
    iter.consume(1);
    assert_eq!(iter.poll_next(cx), IterState::Complete(&[]));

    let src = Box::pin(Replay(const { &[
        Poll::Ready(Ok(&[1u8, 2, 3] as &[u8])),
        Poll::Ready(Err("err")),
    ]}.iter()));
    let mut iter = FileStreamIter::new(BlockDesc { element_size: ElementSize::U16, count: 4 }, src);
    assert_eq!(iter.poll_next(cx), IterState::Complete(&[1, 2]));
    iter.consume(1);
    assert_eq!(iter.poll_next(cx), IterState::Error("err"));

    let src = Box::pin(Replay(const { &[
        Poll::Ready(Ok(&[1u8, 2, 3, 4] as &[u8])),
        Poll::Pending,
        Poll::Pending,
        Poll::Ready(Ok(&[5, 6])),
        Poll::Pending,
    ]}.iter()));
    let mut iter = Box::new(FileStreamIter::new(BlockDesc { element_size: ElementSize::U8, count: 4 }, src)) as Box<dyn StreamIter>;
    assert_eq!(block_on(iter.read_to_vec(10)), Ok(vec![1u8, 2, 3, 4, 5, 6]));
}
