use std::{cell::RefCell, collections::{hash_map, HashMap, HashSet}, fmt::Debug, future::Future, io, pin::Pin, sync::Arc, task::{Context, Poll, Waker}};

use async_executor::{Executor, Task};
use elsa::FrozenMap;
use futures_lite::{AsyncBufRead, FutureExt};

use crate::{ElementType, import::ImportError, io::ReadableFile, schema::{EntitySchema, EntityStream}, stream::{Stream, StreamAccess, StreamDesc, StreamIter, StreamState}};

pub struct FlatFileOpts {
    pub offset: u64,
    pub count: Option<u64>,
    pub block_size: usize,
    pub element_type: ElementType,
}

impl Default for FlatFileOpts {
    fn default() -> Self {
        FlatFileOpts {
            offset: 0,
            count: None,
            block_size: 1 << 20,
            element_type: ElementType::U8,
        }
    }
}

pub struct FlatFileStream {
    file: Arc<dyn ReadableFile>,
    offset: u64,
    count: u64,
    block_size: usize,
    block_size_bytes: usize,
    element_type: ElementType,
    executor: Arc<Executor<'static>>,
}

impl Debug for FlatFileStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FlatFileStream")
            .field(&self.file.filename().unwrap_or("<unknown>"))
            .finish()
    }
}

impl FlatFileStream {
    pub async fn new(file: Arc<dyn ReadableFile>, executor: Arc<Executor<'static>>, opts: FlatFileOpts) -> Result<Self, io::Error> {
        let file_len = file.clone().get_len().await?;
        
        let offset = opts.offset;
        let element_type = opts.element_type;
        let count = opts.count.or(file_len.checked_div(element_type.bytes() as u64)).unwrap_or(0);
        let block_size = opts.block_size;
        let block_size_bytes = block_size.saturating_mul(element_type.bytes() as usize);

        Ok(FlatFileStream { file, offset, count, block_size, block_size_bytes, element_type, executor })
    }

    pub async fn entity(file: Arc<dyn ReadableFile>, executor: Arc<Executor<'static>>, schema: EntitySchema, opts: FlatFileOpts) -> Result<EntityStream, ImportError> {
        let (_field, _stride) = schema.single_stream()
            .ok_or_else(|| ImportError::SchemaMismatch("FlatFileStream requires a single stream".into()))?;
        let stream = Self::new(file, executor, opts).await.map_err(ImportError::Io)?;
        Ok(schema.wrap_single(Arc::new(stream)).unwrap())
    }

    fn block_offset(&self, block: u64) -> u64 {
        self.offset.saturating_add((self.block_size_bytes as u64).saturating_mul(block))
    }

    fn load_block(&self, block: u64) -> impl Future<Output = Result<Vec<u8>, io::Error>> + Send + 'static {
        log::debug!("Loading block {block} of {}", self.file.filename().unwrap_or("<unknown>"));
        let offset = self.block_offset(block);
        self.file.clone().read_at(offset, self.block_size_bytes)
    }
}

impl Stream for FlatFileStream {
    fn desc(&self) -> StreamDesc {
        StreamDesc {
            element_type: self.element_type,
            block_size: self.block_size
        }
    }

    fn state(&self) -> StreamState {
        StreamState {
            streaming: false,
            end: self.count,
        }
    }
    
    fn access(self: Arc<Self>) -> Box<dyn crate::stream::StreamAccess> {
        Box::new(FileStreamAccess {
            stream: self,
            blocks: FrozenMap::new(),
            state: RefCell::new(FileStreamAccessState { 
                used: HashSet::new(),
                loading: HashMap::new(),
                error: None
            }),
            waker: Waker::noop().clone(),
        })
    }
    
    fn iter(self: Arc<Self>) -> Box<dyn crate::stream::StreamIter> {
        Box::new(FileStreamIter {
            stream: self.clone(),
            file_stream: self.file.clone().stream(),
            last: 0,
        })
    }
}

struct FileStreamAccess {
    stream: Arc<FlatFileStream>,
    blocks: FrozenMap<u64, Vec<u8>>,
    state: RefCell<FileStreamAccessState>,
    waker: Waker,
}

struct FileStreamAccessState {
    used: HashSet<u64>,
    loading: HashMap<u64, Task<Result<Vec<u8>, io::Error>>>,
    error: Option<io::Error>,
}

impl StreamAccess for FileStreamAccess {
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
                // Block is not loaded, start loading
                if is_error {
                    return &[];
                }
                entry.insert_entry(self.stream.executor.spawn(self.stream.load_block(block)))
            }
        };

        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(res) = entry.get_mut().poll(&mut cx) {
            drop(entry.remove());
            match res {
                Ok(buf) => {
                    log::debug!("Block {block} of {} finished loading", self.stream.file.filename().unwrap_or("<unknown>"));
                    return self.blocks.insert(block, buf);
                }
                Err(e) => {
                    log::error!("Block {block} of {} failed to load: {}", self.stream.file.filename().unwrap_or("<unknown>"), e);
                    state.error = Some(e);
                }
            }
        }

        log::trace!("Block {block} of {} is still loading", self.stream.file.filename().unwrap_or("<unknown>"));

        return &[];
    }

    fn state(&self) -> StreamState {
        self.stream.state()
    }

    fn begin(&mut self, waker: &Waker) {
        self.waker = waker.clone();
    }

    fn end(&mut self) {
        let mut state = self.state.borrow_mut();
        self.blocks.as_mut().retain(|block, _| state.used.contains(block));
        state.used.clear();
    }
}

struct FileStreamIter {
    stream: Arc<FlatFileStream>,
    file_stream: Pin<Box<dyn AsyncBufRead + Send + Sync>>,
    last: usize,
}

impl StreamIter for FileStreamIter {
    fn poll_next(&mut self, cx: &mut Context) -> Poll<Result<&[u8], String>> {
        self.file_stream.as_mut().consume(self.last);
        self.last = 0;
        match self.file_stream.as_mut().poll_fill_buf(cx) {
            Poll::Ready(Ok(buf)) => {
                self.last = buf.len();
                Poll::Ready(Ok(buf))
            }
            Poll::Ready(Err(e)) => {
                log::error!("Error reading from file {}: {}", self.stream.file.filename().unwrap_or("<unknown>"), e);
                Poll::Ready(Err(e.to_string()))
            }
            Poll::Pending => Poll::Pending
        }
    }
}
