/// Common code for the .izs (Iguazu Signal) file format
use std::{io, pin::Pin, sync::Arc};

use async_executor::Executor;
use futures_lite::{AsyncWrite, AsyncWriteExt};
use serde::{Deserialize, Serialize};

use crate::{export::ExportError, schema::{Entity, EntityStream}, stream::ArcStream, ElementType};
use async_lock::Mutex as AsyncMutex;

pub const HEADER_MAGIC: [u8; 8] = [ 0x00, 0x21, 0x4a, 0xd9, 0xff, 0x90, 0xba, 0xed ];
pub const FOOTER_MAGIC: [u8; 8] = [ 0x01, 0x21, 0x4a, 0xd9, 0x01, 0x90, 0xba, 0xed ];

pub struct Footer {
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

        if &data[8..16] != FOOTER_MAGIC {
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
pub enum CompressionMethod {
    None,
    Zstd,
    #[serde(other)]
    Unknown,
}

/// Stream info in `data` of the metadata
#[derive(Serialize, Deserialize)]
pub struct StreamMeta {
    pub element_type: ElementType,
    pub block_size: usize,
    pub root: u64,
    pub root_len: u32,
    pub end_idx: u64,
    pub compress: CompressionMethod,
}

enum WriteState {
    Writing {
        file: Pin<Box<dyn AsyncWrite + Send>>,
        pos: u64,
    },
    Error,
}

impl WriteState {
    async fn write_block(&mut self, data: &[u8]) -> Result<u64, io::Error> {
        match *self {
            WriteState::Writing { ref mut file, ref mut pos } => {
                let old_pos = *pos;
                match file.write_all(data).await {
                    Ok(()) => {
                        *pos += data.len() as u64;
                        Ok(old_pos)
                    }
                    Err(e) => {
                        *self = WriteState::Error;
                        Err(e)
                    }
                }
            }
            WriteState::Error => {
                Err(io::Error::other("write failed due to previous error"))
            }
        }
    }
}

async fn write_stream(write: Arc<AsyncMutex<WriteState>>, stream: ArcStream) -> Result<StreamMeta, ExportError> {
    let desc = stream.desc();
    let element_type = desc.element_type;
    let block_size = 1024 * 1024;

    let mut block_offsets = Vec::new();
    let mut block_lengths = Vec::new();
    let mut pos = 0;

    let mut iter = stream.iter();
    loop {
        let block = iter.read_to_vec(block_size).await.map_err(ExportError::Source)?;
        let compressed_block = zstd::bulk::compress(&block, 0).unwrap();
        let offset = write.lock().await.write_block(&compressed_block).await?;
        block_offsets.push(offset);
        block_lengths.push(compressed_block.len() as u32);
        pos += (block.len() / element_type.bytes()) as u64;

        if block.len() < block_size * element_type.bytes() {
            break;
        }
    }

    // Write the block index
    let buf = write_block_index(block_offsets, block_lengths);
    let root = write.lock().await.write_block(&buf).await?;
    let root_len = buf.len() as u32;

    Ok(StreamMeta {
        element_type,
        block_size,
        root,
        root_len,
        end_idx: pos,
        compress: CompressionMethod::Zstd,
    })
}

fn write_block_index(block_offsets: Vec<u64>, block_lengths: Vec<u32>) -> Vec<u8> {
    let mut buf = Vec::new();
    for &offset in &block_offsets {
        buf.extend_from_slice(&offset.to_le_bytes());
    }
    for &len in &block_lengths {
        buf.extend_from_slice(&len.to_le_bytes());
    }
    buf
}

pub fn read_block_index(data: &[u8]) -> (Vec<u64>, Vec<u32>) {
    let len = data.len() / 12;
    let block_offsets = data[..len * 8].as_chunks::<8>().0.iter().map(|&c| u64::from_le_bytes(c)).collect();
    let block_lengths = data[len * 8..][..len * 4].as_chunks::<4>().0.iter().map(|&c| u32::from_le_bytes(c)).collect();
    (block_offsets, block_lengths)
}

async fn write_entity(
    ex: Arc<Executor<'static>>,
    write: Arc<AsyncMutex<WriteState>>,
    entity: EntityStream
) -> Result<Entity<StreamMeta>, ExportError> {
    entity.try_map_data_async(move |stream| ex.spawn(write_stream(write.clone(), stream))).await
}

pub async fn export(ex: Arc<Executor<'static>>, entity: EntityStream, file: Pin<Box<dyn AsyncWrite + Send>>) -> Result<(), ExportError> {
    let write = Arc::new(AsyncMutex::new(WriteState::Writing { file, pos: 0 }));

    // Write header
    write.lock().await.write_block(&HEADER_MAGIC).await?;

    // Write entities
    let meta_entity = write_entity(ex.clone(), write.clone(), entity).await?;

    let mut write = write.lock().await;

    // Write metadata
    let meta_data = serde_json::to_vec(&meta_entity).unwrap();
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

    if let WriteState::Writing { ref mut file, .. } = *write {
        file.flush().await?;
    } else {
        unreachable!();
    }

    Ok(())
}
