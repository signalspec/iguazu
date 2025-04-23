use std::sync::Arc;
use std::fmt::Debug;

use append_array::{AppendArrayWriter, AppendArray};
use elsa::sync::FrozenVec;
use crate::stream::{ElementSize, Stream, StreamAccess, StreamDesc, StreamState};

const BLOCK_SIZE: usize = 1<<16;

pub struct MemoryStream {
    element_size: ElementSize,
    blocks: FrozenVec<Arc<AppendArray<u8>>>,
}

impl MemoryStream {
    pub fn new(element_size: ElementSize, data: &[u8]) -> Arc<Self> {
        let mut writer = MemoryStreamWriter::new(element_size);
        writer.extend_from_slice(data);
        writer.stream
    }

    pub fn null() -> Arc<Self> {
        Arc::new(MemoryStream {
            element_size: ElementSize::Null,
            blocks: FrozenVec::new(),
        })
    }
}

impl Debug for MemoryStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemoryStream")
    }
}

impl Stream for MemoryStream {
    fn desc(&self) -> StreamDesc {
        StreamDesc {
            element_size: self.element_size,
            block_size: BLOCK_SIZE,
        }
    }

    fn state(&self) -> StreamState {
        let n_blocks = self.blocks.len();
        let last_block = self.blocks.get(n_blocks - 1).unwrap().len() / self.element_size.bytes();
        let end = ((n_blocks - 1) * BLOCK_SIZE + last_block) as u64;

        StreamState {
            streaming: true,
            end,
        }
    }
    
    fn access(self: Arc<Self>) -> Box<dyn StreamAccess> {
        Box::new(MemoryStreamAccess { stream: self })
    }
}

pub struct MemoryStreamAccess {
    stream: Arc<MemoryStream>,
}

impl StreamAccess for MemoryStreamAccess {
    fn get_block(&self, block: u64) -> &[u8] {
        block.try_into().ok()
            .and_then(|block| self.stream.blocks.get(block))
            .map_or(&[] as &[u8], |block| block.as_ref())
    }

    fn state(&self) -> StreamState {
        self.stream.state()
    }

    fn reset(&mut self) {}
}

pub struct MemoryStreamWriter {
    stream: Arc<MemoryStream>,
    writer: AppendArrayWriter<u8>,
}

impl MemoryStreamWriter {
    pub fn new(element_size: ElementSize) -> MemoryStreamWriter {
        let writer = AppendArrayWriter::with_capacity(BLOCK_SIZE * element_size.bytes());
        let blocks = FrozenVec::new();
        blocks.push(writer.reader());
        let stream = Arc::new(MemoryStream { blocks, element_size });
        MemoryStreamWriter { stream, writer }
    }

    pub fn stream(&self) -> &Arc<MemoryStream> {
        &self.stream
    }

    pub fn extend_from_slice(&mut self, mut data: &[u8]) {
        loop {
            data = self.writer.extend_from_slice(data);
            if data.is_empty() { break }
            self.writer = AppendArrayWriter::with_capacity(BLOCK_SIZE * self.stream.element_size.bytes());
            self.stream.blocks.push(self.writer.reader());
        }
    }
}
