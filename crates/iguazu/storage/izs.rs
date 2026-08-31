use std::io;
use std::pin::Pin;
use std::task::Waker;
use std::sync::Arc;
use std::fmt::Debug;

use ecow::eco_format;
use once_array::OnceArray;

use crate::io::IoError;
use crate::izs::{BlockIndex, CompressionMethod, IzsFile, StreamMeta};
use crate::storage::{ Pool, block::{ BlockCache, BlockState } };
use crate::stream::{ArcStream, BlockDesc, Stream, StreamIter, StreamState};

pub(crate) struct IzsStream {
    shared: Arc<IzsFile>,
    pool: Arc<Pool>,
    id: u64,

    block_desc: BlockDesc,
    compress: CompressionMethod,
    block_index: BlockIndex,

    /// Number of elements
    pos: u64,

    cache: BlockCache,
}

impl IzsStream {
    pub(crate) fn new(shared: Arc<IzsFile>, pool: Arc<Pool>, s: &StreamMeta, block_index: BlockIndex) -> IzsStream {
        IzsStream {
            shared,
            pool,
            id: s.i_offset,
            block_desc: BlockDesc { element_size: s.element, count: s.block },
            compress: s.compress,
            block_index,
            pos: s.end,
            cache: BlockCache::new(),
        }
    }
}

impl Debug for IzsStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IzsStream")
            .field("file", &self.shared.file().filename().unwrap_or("<unknown>"))
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

    fn get_block(&self, block_i: u64, waker: &Waker) -> BlockState {
        let Some(&offset) = self.block_index.offsets.get(block_i as usize) else {
            return BlockState::End
        };

        if let Some(block) = self.cache.get(block_i, waker) {
            log::debug!("Block {block_i} of {}:{:x} found in cache", self.shared.file().filename().unwrap_or("<unknown>"), self.id);
            return block;
        }

        let block_bytes = self.block_desc.size();
        let compression = self.compress;
        let len = self.block_index.lengths[block_i as usize] as usize;
        log::debug!("Loading block {block_i} of {}:{:x} at {} len {}", self.shared.file().filename().unwrap_or("<unknown>"), self.id, offset, len);

        let shared = self.shared.clone();
        let task = self.pool.clone().spawn_block_fetch(async move {
            let data = shared.read_at(offset, len).await?;
            let data = match compression {
                CompressionMethod::None => data,
                CompressionMethod::Zstd => {
                    zstd::bulk::decompress(&data, block_bytes)
                        .map_err(|e| IoError::new(eco_format!("failed to decompress block: {e}"), None, Some(e.kind())))?
                }
                CompressionMethod::Unknown => {
                    return Err(IoError::new(eco_format!("unknown compression method"), None, Some(io::ErrorKind::InvalidData)));
                }
            };

            Ok(Arc::new(OnceArray::from(data)))
        });

        task.subscribe(waker);
        self.cache.insert_task(block_i, task);
        BlockState::Pending
    }

    fn iter(self: Arc<Self>) -> Pin<Box<dyn Future<Output = Result<Box<dyn StreamIter>, IoError>> + Send>> {
        Box::pin(async move {
            Ok(Box::new(crate::storage::common::CommonStreamIter::new(self as ArcStream)) as _)
        })
    }
}
