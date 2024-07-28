use std::{fmt::Debug, fs::File, io, os::unix::fs::FileExt, sync::Arc};

use append_array::AppendArray;

use crate::{schema::{attribute::SampleRate, Attributes, Entity, Field, NestedField}, stream::{Stream, StreamDesc, StreamState}};

pub struct FlatFileStream {
    file: File,
    offset: u64,
    len: u64,
    block_size: usize,
    element_size: usize,
}

impl Debug for FlatFileStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlatFileStream").finish()
    }
}

impl FlatFileStream {
    pub fn new(file: File, block_size: usize, element_size: usize) -> Result<Self, io::Error> {
        let file_len = file.metadata()?.len();
        Ok(Self::range(file, 0, file_len, block_size, element_size))
    }

    pub fn range(file: File, offset: u64, len: u64, block_size: usize, element_size: usize) -> Self {
        FlatFileStream { file, offset, len, block_size, element_size }
    }

    pub fn entity(file: File, encoding: NestedField) -> Result<Entity, io::Error> {
        let element_size = encoding.kind.bit_width().div_ceil(8) as usize;
        let block_size = 1 << 20;
        let data = Arc::new(Self::new(file, block_size, element_size)?);
        Ok(Entity::Data { data, encoding })
    }
}

pub fn binary_file(file: File) -> Result<Entity, io::Error> {
    let encoding = NestedField {
        kind: Field::Bits { bits: 8 },
        attributes: Attributes::default(),
    };
    FlatFileStream::entity(file, encoding)
}

pub fn logic8(file: File, rate: SampleRate) -> Result<Entity, io::Error> {
    let mut attributes = Attributes::default();
    attributes.set(&rate);
    let encoding = NestedField {
        kind: Field::Struct { children: (0..8).map(|i| {
            (i.to_string(), NestedField::new(Field::enum_named(1, &["l", "h"])))
        }).collect()},
        attributes,
    };
    FlatFileStream::entity(file, encoding)
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
            end: self.len / self.element_size as u64,
        }
    }

    fn get_block(&self, block: u64) -> Option<Arc<AppendArray<u8>>> {
        let offset = self.offset + self.block_size as u64 * self.element_size as u64 * block;
        let len = self.block_size * self.element_size;
        let mut buf = vec![0; len];
        let mut pos = 0;

        while pos < len {
            let dest = &mut buf.as_mut_slice()[pos..];
            let bytes_read = self.file.read_at(dest, offset + pos as u64).ok()?;
            if bytes_read == 0 {
                buf.truncate(pos);
                break;
            }
            pos += bytes_read;
        }

        Some(Arc::new(AppendArray::from(buf)))
    }
}
