use std::sync::{Arc, RwLock};
use std::fmt::Debug;

use append_array::{AppendArrayWriter, AppendArray};
use crate::stream::{Stream, StreamDesc, StreamState};

const BLOCK_SIZE: usize = 1<<16;

pub struct MemoryStream {
    element_size: usize,
    chunks: RwLock<Vec<Arc<AppendArray<u8>>>>,
}

impl MemoryStream {
    pub fn new(element_size: usize, data: &[u8]) -> Arc<Self> {
        let mut writer = MemoryStreamWriter::new(element_size);
        writer.extend_from_slice(data);
        writer.stream
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
        let blocks = self.chunks.read().unwrap();
        let last_block = blocks.last().unwrap().len() / self.element_size;
        let end = ((blocks.len() - 1) * BLOCK_SIZE + last_block) as u64;

        StreamState {
            streaming: true,
            end,
        }
    }

    fn get_block(&self, block: u64) -> Option<Arc<AppendArray<u8>>> {
        let chunks = self.chunks.read().unwrap();
        chunks.get(block as usize).cloned()
    }
}

pub struct MemoryStreamWriter {
    stream: Arc<MemoryStream>,
    writer: AppendArrayWriter<u8>,
}

impl MemoryStreamWriter {
    pub fn new(element_size: usize) -> MemoryStreamWriter {
        let writer = AppendArrayWriter::with_capacity(BLOCK_SIZE * element_size);
        let chunks = RwLock::new(vec![writer.reader()]);
        let stream = Arc::new(MemoryStream { chunks, element_size });
        MemoryStreamWriter { stream, writer }
    }

    pub fn stream(&self) -> &Arc<MemoryStream> {
        &self.stream
    }

    pub fn extend_from_slice(&mut self, mut data: &[u8]) {
        loop {
            data = self.writer.extend_from_slice(data);
            if data.is_empty() { break }
            let mut chunks = self.stream.chunks.write().unwrap();
            self.writer = AppendArrayWriter::with_capacity(BLOCK_SIZE * self.stream.element_size);
            chunks.push(self.writer.reader());
        }
    }
}
