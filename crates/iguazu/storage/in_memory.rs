use std::task::{Context, Poll};
use std::{sync::{Arc, atomic::AtomicBool}, task::Waker};
use std::fmt::Debug;

use append_array::{AppendArrayWriter, AppendArray};
use elsa::sync::FrozenVec;
use crate::{Element, ElementType, stream::{Stream, StreamAccess, StreamDesc, StreamIter, StreamState}};

const BLOCK_SIZE: usize = 1<<16;

pub struct MemoryStream {
    element_type: ElementType,
    blocks: FrozenVec<Arc<AppendArray<u8>>>,
    streaming: AtomicBool,
}

impl MemoryStream {
    pub fn new<T: Element>(data: &[T]) -> Arc<Self> {
        Self::raw(T::ELEMENT_TYPE, bytemuck::cast_slice(data))
    }

    pub fn raw(element_type: ElementType, data: &[u8]) -> Arc<Self> {
        let mut writer = MemoryStreamWriter::new(element_type);
        writer.extend_from_slice(data);
        writer.stream.clone()
    }

    fn streaming(&self) -> bool {
        self.streaming.load(std::sync::atomic::Ordering::Relaxed)
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
            element_type: self.element_type,
            block_size: BLOCK_SIZE,
        }
    }

    fn state(&self) -> StreamState {
        let n_blocks = self.blocks.len();
        let last_block = self.blocks.get(n_blocks - 1).unwrap().len() / self.element_type.bytes();
        let end = ((n_blocks - 1) * BLOCK_SIZE + last_block) as u64;

        StreamState {
            streaming: self.streaming(),
            end,
        }
    }
    
    fn access(self: Arc<Self>) -> Box<dyn StreamAccess> {
        Box::new(MemoryStreamAccess { stream: self })
    }

    fn iter(self: Arc<Self>) -> Box<dyn StreamIter> {
        Box::new(MemoryStreamIter {
            stream: self,
            block: 0,
            pos: 0,
        })
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

    fn begin(&mut self, _waker: &Waker) {}
    fn end(&mut self) {}
}

pub struct MemoryStreamIter {
    stream: Arc<MemoryStream>,
    block: usize,
    pos: usize,
}

impl StreamIter for MemoryStreamIter {
    fn poll_next(&mut self, _cx: &mut Context) -> Poll<Result<&[u8], String>> {
        let Some(block) = self.stream.blocks.get(self.block) else {
            if self.stream.streaming() {
                return Poll::Pending;
            } else {
                return Poll::Ready(Ok(&[]));
            }
        };

        let Some(data) = block.get(self.pos..) else {
            if self.stream.streaming() {
                return Poll::Pending;
            } else {
                return Poll::Ready(Ok(&[]));
            }
        };

        self.pos += data.len();

        if self.pos >= self.stream.element_type.bytes() * BLOCK_SIZE {
            self.block += 1;
            self.pos = 0;
        }

        Poll::Ready(Ok(data))
    }
}

pub struct MemoryStreamWriter {
    stream: Arc<MemoryStream>,
    writer: AppendArrayWriter<u8>,
}

impl MemoryStreamWriter {
    pub fn new(element_type: ElementType) -> MemoryStreamWriter {
        let writer = AppendArrayWriter::with_capacity(BLOCK_SIZE * element_type.bytes());
        let blocks = FrozenVec::new();
        blocks.push(writer.reader());
        let stream = Arc::new(MemoryStream { blocks, element_type, streaming: AtomicBool::new(true) });
        MemoryStreamWriter { stream, writer }
    }

    pub fn stream(&self) -> &Arc<MemoryStream> {
        &self.stream
    }

    pub fn extend_from_slice(&mut self, mut data: &[u8]) {
        loop {
            data = self.writer.extend_from_slice(data);
            if data.is_empty() { break }
            self.writer = AppendArrayWriter::with_capacity(BLOCK_SIZE * self.stream.element_type.bytes());
            self.stream.blocks.push(self.writer.reader());
        }
    }

    pub fn pos(&self) -> u64 {
        let n_blocks = self.stream.blocks.len();
        ((n_blocks - 1) * BLOCK_SIZE + (self.writer.len() / self.stream.element_type.bytes())) as u64
    }
}

impl Drop for MemoryStreamWriter {
    fn drop(&mut self) {
        self.stream.streaming.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}
