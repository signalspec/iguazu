use std::{cell::RefCell, collections::HashMap, fmt::Debug, io, mem, sync::Arc};

use elsa::FrozenMap;
use futures_lite::future::block_on;
use log::{debug, error};

use crate::{import::ImportError, io::ReadableFile, schema::{EntitySchema, EntityStream}, stream::{ElementType, Stream, StreamAccess, StreamDesc, StreamState}};

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
}

impl Debug for FlatFileStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FlatFileStream")
            .field(&self.file.filename().unwrap_or("<unknown>"))
            .finish()
    }
}

impl FlatFileStream {
    pub async fn new(file: Arc<dyn ReadableFile>, opts: FlatFileOpts) -> Result<Self, io::Error> {
        let file_len = file.clone().get_len().await?;
        
        let offset = opts.offset;
        let element_type = opts.element_type;
        let count = opts.count.or(file_len.checked_div(element_type.bytes() as u64)).unwrap_or(0);
        let block_size = opts.block_size;
        let block_size_bytes = block_size.saturating_mul(element_type.bytes() as usize);

        Ok(FlatFileStream { file, offset, count, block_size, block_size_bytes, element_type })
    }

    pub async fn entity(file: Arc<dyn ReadableFile>, schema: EntitySchema, opts: FlatFileOpts) -> Result<EntityStream, ImportError> {
        let _ = schema.single_stream()
            .ok_or_else(|| ImportError::SchemaMismatch("FlatFileStream requires a single stream".into()))?;
        let stream = Self::new(file, opts).await.map_err(ImportError::Io)?;
        Ok(schema.wrap_single(Arc::new(stream)).unwrap())
    }

    fn block_offset(&self, block: u64) -> u64 {
        self.offset.saturating_add((self.block_size_bytes as u64).saturating_mul(block))
    }

    async fn load_block(&self, block: u64) -> Result<Vec<u8>, io::Error> {
        let offset = self.block_offset(block);
        debug!("Loading block of {self:?} at {offset}");
        self.file.clone().read_at(offset, self.block_size_bytes).await.inspect_err(|e| {
            error!("Failed to read block of {:?} at {offset}: {e}", self);
        })
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
        Box::new(FileStreamAccess { stream: self, blocks: FrozenMap::new(), prev_blocks: RefCell::new(HashMap::new()) })
    }
}

struct FileStreamAccess {
    stream: Arc<FlatFileStream>,
    prev_blocks: RefCell<HashMap<u64, Vec<u8>>>,
    blocks: FrozenMap<u64, Vec<u8>>,
}

impl StreamAccess for FileStreamAccess {
    fn get_block(&self, block: u64) -> &[u8] {
        if let Some(buf) = self.blocks.get(&block) {
            return buf;
        }

        let buf = if let Some(buf) = self.prev_blocks.borrow_mut().remove(&block) {
            buf
        } else if let Ok(buf) = block_on(self.stream.load_block(block)) {
            buf
        } else {
            return &[];
        };

        self.blocks.insert(block, buf)
    }

    fn state(&self) -> StreamState {
        self.stream.state()
    }

    fn reset(&mut self) {
        let blocks = mem::take(&mut self.blocks).into_map();
        let mut next_blocks = self.prev_blocks.replace(blocks);
        next_blocks.clear();
        self.blocks = FrozenMap::from(next_blocks);
    }
}
