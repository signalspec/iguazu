use std::{cell::RefCell, collections::{HashMap, HashSet, hash_map}, fmt::Debug, future::Future, io, pin::Pin, sync::{Arc, Mutex}, task::{Context, Poll, Waker}};

use async_executor::Task;
use elsa::FrozenMap;
use futures_lite::{AsyncBufRead, FutureExt};
use once_array::OnceArray;
use url::Url;

use crate::{ElementSize, import::ImportError, io::ReadableFile, schema::{EntitySchema, EntityStream}, storage::Pool, util::weak_map::WeakMap, stream::{Stream, StreamAccess, BlockDesc, StreamIter, StreamState}};

#[derive(Clone)]
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
    offset: u64,
    count: u64,
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

        let offset = opts.offset;
        let count = opts.count.or(file_len.checked_div(element_size.bytes() as u64)).unwrap_or(0);
        let block_desc = BlockDesc { element_size, count: opts.block_size };

        let cache = Mutex::new(WeakMap::new());

        Ok(FlatFileStream { file, offset, count, block_desc, cache, pool })
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

    pub async fn entity(file: Arc<dyn ReadableFile>, pool: Arc<Pool>, schema: EntitySchema, opts: &FlatFileOpts) -> Result<EntityStream, ImportError> {
        let (field, _stride) = schema.single_stream()
            .ok_or_else(|| ImportError::SchemaMismatch("FlatFileStream requires a single stream".into()))?;

        let element_type = ElementSize::from_bits(field.kind.width())
            .ok_or_else(|| ImportError::SchemaMismatch(format!("Field is {} bits wide. Must be <= 64.", field.kind.width())))?;

        let stream = Self::new(file, pool, element_type, &opts).await.map_err(ImportError::Io)?;
        Ok(schema.wrap_single(Arc::new(stream)).unwrap())
    }

    fn block_offset(&self, block: u64) -> u64 {
        self.offset.saturating_add((self.block_desc.size() as u64).saturating_mul(block))
    }

    fn load_block_uncached(&self, block: u64) -> impl Future<Output = Result<Vec<u8>, io::Error>> + Send + 'static {
        log::debug!("Loading block {block} of {}", self.file.filename().unwrap_or("<unknown>"));
        let offset = self.block_offset(block);
        self.file.clone().read_at(offset, self.block_desc.size())
    }

    fn load_block(self: Arc<Self>, block: u64) -> LoadBlockRes<impl Future<Output = Result<Arc<OnceArray<u8>>, io::Error>> + Send> {
        if let Some(entry) = self.cache.lock().unwrap().get(&block) {
            return LoadBlockRes::Cached(entry);
        }

        LoadBlockRes::Loading(async move {
            let buf = self.load_block_uncached(block).await?;
            let entry = Arc::new(OnceArray::from(buf));
            self.pool.cache.lock().unwrap().insert(entry.clone());
            self.cache.lock().unwrap().insert(block, entry.clone());
            Ok(entry)
        })
    }
}

impl Stream for FlatFileStream {
    fn desc(&self) -> BlockDesc {
        self.block_desc
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

    fn iter(self: Arc<Self>) -> Pin<Box<dyn Future<Output = Result<Box<dyn StreamIter>, io::Error>> + Send + 'static>> {
        Box::pin(async move {
            Ok(Box::new(FileStreamIter {
                stream: self.clone(),
                file_stream: self.file.clone().stream().await?,
            }) as Box<dyn StreamIter>)
        })
    }
}

enum LoadBlockRes<F> {
    Cached(Arc<OnceArray<u8>>),
    Loading(F),
}

struct FileStreamAccess {
    stream: Arc<FlatFileStream>,
    blocks: FrozenMap<u64, Arc<OnceArray<u8>>>,
    state: RefCell<FileStreamAccessState>,
    waker: Waker,
}

struct FileStreamAccessState {
    used: HashSet<u64>,
    loading: HashMap<u64, Task<Result<Arc<OnceArray<u8>>, io::Error>>>,
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

                match self.stream.clone().load_block(block) {
                    LoadBlockRes::Cached(buf) => {
                        log::debug!("Block {block} of {} loaded from cache", self.stream.file.filename().unwrap_or("<unknown>"));
                        return self.blocks.insert(block, buf);
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
        self.waker.clone_from(waker);
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
}

impl StreamIter for FileStreamIter {
    fn element_type(&self) -> ElementSize {
        self.stream.block_desc.element_size
    }

    fn poll_next(&mut self, cx: &mut Context) -> Poll<Result<&[u8], String>> {
        match self.file_stream.as_mut().poll_fill_buf(cx) {
            Poll::Ready(Ok(buf)) => {
                Poll::Ready(Ok(buf))
            }
            Poll::Ready(Err(e)) => {
                log::error!("Error reading from file {}: {}", self.stream.file.filename().unwrap_or("<unknown>"), e);
                Poll::Ready(Err(e.to_string()))
            }
            Poll::Pending => Poll::Pending
        }
    }

    fn consume(&mut self, len: usize) {
        self.file_stream.as_mut().consume(len * self.stream.block_desc.element_size.bytes());
    }
}
