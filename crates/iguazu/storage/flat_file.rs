use std::{fmt::Debug, io, sync::Arc};

use append_array::AppendArray;

use crate::{io::ReadableFile, schema::{attribute::SampleRate, Attributes, Entity, Field, NestedField}, stream::{Stream, StreamDesc, StreamState}};

pub struct FlatFileStream<F> {
    file: F,
    offset: u64,
    len: u64,
    block_size: usize,
    element_size: usize,
}

impl<F: ReadableFile> Debug for FlatFileStream<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FlatFileStream")
            .field(&self.file.filename().unwrap_or("<unknown>"))
            .finish()
    }
}

impl<F: ReadableFile + 'static> FlatFileStream<F> {
    pub fn new(file: F, element_size: usize) -> Result<Self, io::Error> {
        let file_len = file.get_len()?;
        let block_size = 1 << 20;
        Ok(Self::range(file, 0, file_len, block_size, element_size))
    }

    pub fn range(file: F, offset: u64, len: u64, block_size: usize, element_size: usize) -> Self {
        FlatFileStream { file, offset, len, block_size, element_size }
    }

    pub fn entity(file: F, encoding: NestedField) -> Result<Entity, io::Error> {
        let element_size = encoding.kind.bit_width().div_ceil(8) as usize;
        let data = Arc::new(Self::new(file, element_size)?);
        Ok(Entity::Data { data, encoding })
    }

    pub fn binary_file(file: F) -> Result<Entity, io::Error> {
        let encoding = NestedField {
            kind: Field::Bits { bits: 8 },
            attributes: Attributes::default(),
        };
        FlatFileStream::entity(file, encoding)
    }

    pub fn logic8(file: F, rate: SampleRate) -> Result<Entity, io::Error> {
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
}

impl<F: ReadableFile> Stream for FlatFileStream<F> {
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
        let buf = self.file.read_at(offset, len).ok()?;
        Some(Arc::new(AppendArray::from(buf)))
    }
}
