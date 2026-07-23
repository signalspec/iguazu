/// Common code for the .izs (Iguazu Signal) file format
use std::{io, mem, pin::Pin, sync::Arc};

use async_executor::Executor;
use futures_lite::{AsyncWrite, AsyncWriteExt};
use log::debug;
use serde::{Deserialize, Serialize};

use crate::{ElementSize, export::ExportError, import::ImportError, io::ReadableFile, schema::{Entity, EntityData, EntitySchema, EntityStream}, storage::{Pool, izs::IzsStream}, stream::{ArcStream, Stream}, summary::StoredSummaryMap};
use async_lock::Mutex as AsyncMutex;

pub(crate) const HEADER_MAGIC: [u8; 8] = [ 0x00, 0x21, 0x4a, 0xd9, 0xff, 0x90, 0xba, 0xed ];
pub(crate) const FOOTER_MAGIC: [u8; 8] = [ 0x01, 0x21, 0x4a, 0xd9, 0x01, 0x90, 0xba, 0xed ];

pub(crate) struct Footer {
    pub meta_len: u32,
    pub reserved: u32,
}

impl Footer {
    pub const LEN: usize = 16;

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::LEN {
            return None;
        }

        let data = &data[data.len() - Self::LEN ..];

        if data[8..16] != FOOTER_MAGIC {
            return None;
        }

        let meta_len = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let reserved = u32::from_le_bytes(data[4..8].try_into().unwrap());

        Some(Self { meta_len, reserved })
    }

    fn to_bytes(&self) -> [u8; Self::LEN] {
        let mut bytes = [0u8; Self::LEN];
        bytes[0..4].copy_from_slice(&self.meta_len.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.reserved.to_le_bytes());
        bytes[8..16].copy_from_slice(&FOOTER_MAGIC);
        bytes
    }
}

#[derive(Serialize, Deserialize)]
pub struct FileMeta {
    pub(crate) entity: Entity<StreamMeta, StoredSummaryMap<StreamMeta>>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CompressionMethod {
    None,
    Zstd,
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub(crate) enum IndexCompressionMethod {
    #[default]
    None,
    #[serde(other)]
    Unknown,
}

/// Stream info in `data` of the metadata
#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct StreamMeta {
    #[serde(alias = "element_type")] // Pre-0.1
    pub element: ElementSize,
    #[serde(alias = "block_size")] // Pre-0.1
    pub block: usize,
    pub compress: CompressionMethod,

    #[serde(alias = "root")]
    pub i_offset: u64,
    #[serde(alias = "root_len")]
    pub i_size: u32,
    #[serde(default)]
    pub i_compress: IndexCompressionMethod,

    #[serde(alias = "end_idx")]
    pub end: u64,
}

impl EntityData for StreamMeta {
    type SummaryMap = StoredSummaryMap<StreamMeta>;
}

pub struct IzsFile {
    file: Arc<dyn ReadableFile>,
    tail: Vec<u8>,
    file_size: u64,
}

impl IzsFile {
    pub async fn new(file: Arc<dyn ReadableFile>) -> Result<Self, ImportError> {
        let (tail, file_size) = file.clone().read_at_end(1024 * 1024).await?;
        let s = Self { file, tail, file_size };
        s.footer()?;
        Ok(s)
    }

    pub fn filename(&self) -> Option<&str> {
        self.file.filename()
    }

    pub(crate) async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>, std::io::Error> {
        let tail_offset = self.file_size.saturating_sub(self.tail.len() as u64);
        if let Some(pos) = offset.checked_sub(tail_offset) {
            debug!("Reading {len} bytes at {offset} from tail (pos {pos})");
            let end = (self.tail.len() as u64).min(pos + len as u64) as usize;
            return Ok(self.tail[pos as usize..end].to_vec());
        }

        self.file.clone().read_at(offset, len).await
    }

    fn footer(&self) -> Result<Footer, ImportError> {
        let footer_offset = self.tail.len().checked_sub(Footer::LEN)
            .ok_or_else(|| ImportError::InvalidFile("File too small for footer".to_string()))?;
        Footer::from_bytes(&self.tail[footer_offset..])
            .ok_or_else(|| ImportError::InvalidFile("Invalid footer".to_string()))
    }

    pub async fn load_meta(&self) -> Result<FileMeta, ImportError> {
        let footer = self.footer()?;

        if footer.reserved != 0 {
            return Err(ImportError::InvalidFile("File uses unknown features".to_string()));
        }

        let meta_pos = self.file_size.checked_sub(Footer::LEN as u64 + footer.meta_len as u64)
            .ok_or_else(|| ImportError::InvalidFile("Invalid metadata position".to_string()))?;
        let meta = self.read_at(meta_pos, footer.meta_len as usize).await?;
        let meta = zstd::bulk::decompress(&meta, 16 * 1024 * 1024)
            .map_err(|e| ImportError::InvalidFile(format!("Failed to decompress metadata: {}", e)))?;
        serde_json::from_slice(&meta).map_err(|e| ImportError::InvalidFile(format!("Invalid metadata: {}", e)))
    }

    pub async fn load_schema(&self) -> Result<EntitySchema, ImportError> {
        let meta = self.load_meta().await?;
        Ok(meta.entity.schema())
    }

    pub async fn load_entity(self: Arc<Self>, pool: Arc<Pool>) -> Result<EntityStream, ImportError> {
        let meta = self.load_meta().await?;

        meta.entity.try_map_data_async(move |s| {
            let shared = self.clone();
            let pool_inner = pool.clone();
            let s = s.clone();
            pool.executor.spawn(async move {
                let index = shared.read_at(s.i_offset, s.i_size as usize).await?;
                let block_index = BlockIndex::parse(&index);
                let stream = IzsStream::new(shared, pool_inner, &s, block_index);
                Ok(Arc::new(stream) as Arc<dyn Stream>)
            })
        }).await
    }
}

struct WriteState {
    file: Pin<Box<dyn AsyncWrite + Send>>,
    pos: u64,
    block_indexes: Vec<u8>,
    error: bool,
}

impl WriteState {
    fn new(file: Pin<Box<dyn AsyncWrite + Send>>) -> Self {
        Self {
            file,
            pos: 0,
            block_indexes: Vec::new(),
            error: false,
        }
    }

    fn check_error(&self) -> Result<(), io::Error> {
        if self.error {
            Err(io::Error::other("write failed due to previous error"))
        } else {
            Ok(())
        }
    }

    async fn write_block(&mut self, data: &[u8]) -> Result<u64, io::Error> {
        self.check_error()?;
        let old_pos = self.pos;
        match self.file.write_all(data).await {
            Ok(()) => {
                self.pos += data.len() as u64;
                Ok(old_pos)
            }
            Err(e) => {
                self.error = true;
                Err(e)
            }
        }
    }

    /// Append a block index to the buffer to be written at the end of the file.
    ///
    /// These are concatenated and written just before the metadata, such that
    /// they can be read out of the prefetched tail of the file.
    ///
    /// Returns the byte offset within the buffer.
    fn add_block_index(&mut self, block_index: &BlockIndex) -> (u64, u32) {
        let before = self.block_indexes.len() as u64;
        block_index.serialize_into(&mut self.block_indexes);
        let after = self.block_indexes.len() as u64;
        (before, (after - before) as u32)
    }
}

async fn write_stream(write: Arc<AsyncMutex<WriteState>>, stream: ArcStream) -> Result<StreamMeta, ExportError> {
    let desc = stream.desc();
    let element = desc.element_size;
    let block_size = 1024 * 1024;

    let mut block_index = BlockIndex::new();
    let mut pos = 0;

    let mut iter = stream.iter().await.map_err(ExportError::Io)?;
    loop {
        let block = iter.read_to_vec(block_size).await.map_err(ExportError::Source)?;
        let compressed_block = zstd::bulk::compress(&block, 0).unwrap();
        let offset = write.lock().await.write_block(&compressed_block).await?;
        block_index.push(offset, compressed_block.len() as u32);
        pos += (block.len() / element.bytes()) as u64;

        if block.len() < block_size * element.bytes() {
            break;
        }
    }

    let (i_offset, i_size) = write.lock().await.add_block_index(&block_index);

    Ok(StreamMeta {
        element,
        block: block_size,
        compress: CompressionMethod::Zstd,
        i_offset, // This is the offset within the block index buffer. Offset of that buffer within the file is added when it is written out.
        i_size,
        i_compress: IndexCompressionMethod::None,
        end: pos,
    })
}

pub(crate) struct BlockIndex {
    /// Positions of the compressed blocks within the file
    pub offsets: Vec<u64>,

    /// Lengths of the compressed blocks.
    pub lengths: Vec<u32>,
}

impl BlockIndex {
    fn new() -> Self {
        Self {
            offsets: Vec::new(),
            lengths: Vec::new(),
        }
    }

    fn push(&mut self, offset: u64, length: u32) {
        self.offsets.push(offset);
        self.lengths.push(length);
    }

    fn serialize_into(&self, buf: &mut Vec<u8>){
        for &offset in &self.offsets {
            buf.extend_from_slice(&offset.to_le_bytes());
        }
        for &len in &self.lengths {
            buf.extend_from_slice(&len.to_le_bytes());
        }
    }

    pub fn parse(data: &[u8]) -> Self {
        let len = data.len() / 12;
        let offsets = data[..len * 8].as_chunks::<8>().0.iter().map(|&c| u64::from_le_bytes(c)).collect();
        let lengths = data[len * 8..][..len * 4].as_chunks::<4>().0.iter().map(|&c| u32::from_le_bytes(c)).collect();
        Self { offsets, lengths }
    }
}

pub async fn export(ex: Arc<Executor<'static>>, entity: EntityStream, file: Pin<Box<dyn AsyncWrite + Send>>) -> Result<(), ExportError> {
    let write = Arc::new(AsyncMutex::new(WriteState::new(file)));

    // Write header
    write.lock().await.write_block(&HEADER_MAGIC).await?;

    // Write entities
    let w = write.clone();
    let mut meta_entity = entity.try_map_data_async(move |stream| ex.spawn(write_stream(w.clone(), stream.clone()))).await?;

    let mut write = write.lock().await;

    // Write block indexes and update the positions in StreamMeta
    let block_indexes = mem::take(&mut write.block_indexes);
    let block_index_start_pos = write.write_block(&block_indexes).await?;
    meta_entity.each_data_mut(&mut |stream| {
        // Adjust relative offsets within the buffer into absolute offsets within the file.
        stream.i_offset += block_index_start_pos;
    });

    // Write metadata
    let meta = FileMeta {
        entity: meta_entity,
    };
    let meta_data = serde_json::to_vec(&meta).unwrap();
    let meta_data = zstd::bulk::compress(&meta_data, 0).unwrap();
    write.write_block(&meta_data).await?;
    let meta_len = meta_data.len() as u32;

    // Write footer
    let footer = Footer {
        meta_len,
        reserved: 0,
    };
    let footer_bytes = footer.to_bytes();
    write.write_block(&footer_bytes).await?;

    write.file.close().await?;
    Ok(())
}
