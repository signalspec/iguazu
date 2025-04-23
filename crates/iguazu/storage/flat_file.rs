use std::{fmt::Debug, io, sync::Arc};

use append_array::AppendArray;
use log::debug;

use crate::{import::ImportError, io::ReadableFile, schema::{Entity, EntitySchema, EntityStream}, stream::{ArcStream, Stream, StreamDesc, StreamState}};

pub struct FlatFileOpts {
    pub offset: u64,
    pub count: Option<u64>,
    pub block_size: usize,
    pub element_size: Option<usize>,
}

impl Default for FlatFileOpts {
    fn default() -> Self {
        FlatFileOpts {
            offset: 0,
            count: None,
            block_size: 1 << 20,
            element_size: None,
        }
    }
}

pub struct FlatFileStream {
    file: Arc<dyn ReadableFile>,
    offset: u64,
    count: u64,
    block_size: usize,
    element_size: usize,
}

impl Debug for FlatFileStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FlatFileStream")
            .field(&self.file.filename().unwrap_or("<unknown>"))
            .finish()
    }
}

impl FlatFileStream {
    pub fn new(file: Arc<dyn ReadableFile>, opts: FlatFileOpts) -> Result<Self, io::Error> {
        let file_len = file.get_len()?;
        
        let offset = opts.offset;
        let element_size = opts.element_size.unwrap_or(1);
        let count = opts.count.unwrap_or(file_len / element_size as u64);
        let block_size = opts.block_size;

        Ok(FlatFileStream { file, offset, count, block_size, element_size })
    }

    pub fn entity(file: Arc<dyn ReadableFile>, schema: EntitySchema, mut opts: FlatFileOpts) -> Result<EntityStream, ImportError> {
        let (kind, _stride) = schema.single_stream()
            .ok_or_else(|| ImportError::SchemaMismatch("FlatFileStream requires a single stream".into()))?;
        opts.element_size.get_or_insert(kind.element_size());
        let stream = Self::new(file, opts).map_err(ImportError::Io)?;
        Ok(schema.wrap_single(Arc::new(stream)).unwrap())
    }
}

impl Stream for FlatFileStream {
    fn desc(&self) -> StreamDesc {
        StreamDesc {
            element_size: self.element_size,
            block_size: self.block_size
        }
    }

    fn state(&self) -> StreamState {
        StreamState {
            streaming: false,
            end: self.count,
        }
    }

    fn get_block(&self, block: u64) -> Option<Arc<AppendArray<u8>>> {
        let offset = self.offset + self.block_size as u64 * self.element_size as u64 * block;
        debug!("Load block of {self:?} at {offset}");
        let len = self.block_size * self.element_size;
        let buf = self.file.read_at(offset, len).ok()?;
        Some(Arc::new(AppendArray::from(buf)))
    }
}
