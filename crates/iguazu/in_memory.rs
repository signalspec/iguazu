use std::sync::{Arc, RwLock};
use std::fmt::Debug;

use append_array::{AppendArrayWriter, AppendArray};
use crate::stream::{Stream, StreamState};

const DATA_CHUNK_SIZE: usize = 8192;

pub struct MemoryStream<T> {
    chunks: RwLock<Vec<Arc<AppendArray<T>>>>,
}

impl<T> MemoryStream<T> {
    pub fn new(data: &[T]) -> Arc<Self> {
        let mut writer = MemoryStreamWriter::new();
        writer.extend_from_slice(data);
        writer.stream
    }
}

impl<T> Debug for MemoryStream<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemoryStream")
    }
}

impl <T: 'static> Stream<T> for MemoryStream<T> {
    fn block_size(&self) -> usize { DATA_CHUNK_SIZE }

    fn state(&self) -> StreamState {
        let chunks = self.chunks.read().unwrap();
        let end = ((chunks.len() - 1) * DATA_CHUNK_SIZE + chunks.last().unwrap().len()) as u64;

        StreamState {
            streaming: true,
            end,
        }
    }

    fn get_block(&self, block: u64) -> Option<Arc<AppendArray<T>>> {
        let chunks = self.chunks.read().unwrap();
        chunks.get(block as usize).cloned()
    }
}

pub struct MemoryStreamWriter<T> {
    stream: Arc<MemoryStream<T>>,
    writer: AppendArrayWriter<T>,
}

impl<T> MemoryStreamWriter<T> {
    pub fn new() -> MemoryStreamWriter<T> {
        let writer = AppendArrayWriter::with_capacity(DATA_CHUNK_SIZE);
        let chunks = RwLock::new(vec![writer.reader()]);
        let stream = Arc::new(MemoryStream { chunks });
        MemoryStreamWriter { stream, writer }
    }

    pub fn stream(&self) -> &Arc<MemoryStream<T>> {
        &self.stream
    }

    pub fn push(&mut self, data: T) {
        self.extend_from_slice(&[data])
    }

    pub fn extend_from_slice(&mut self, mut data: &[T]) {
        loop {
            data = self.writer.extend_from_slice(data);
            if data.is_empty() { break }
            let mut chunks = self.stream.chunks.write().unwrap();
            chunks.push(self.writer.reader());
        }
    }
}
