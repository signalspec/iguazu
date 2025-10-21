use std::sync::RwLock;
use std::task::{Context, Poll};
use std::{sync::{Arc, atomic::AtomicBool}, task::Waker};
use std::fmt::Debug;

use append_array::{AppendArrayWriter, AppendArray};
use atomic_waker::AtomicWaker;
use elsa::sync::FrozenVec;
use slab::Slab;
use crate::storage::Storage;
use crate::stream::{ArcStream, StreamWriter};
use crate::Idx;
use crate::{Element, ElementType, stream::{Stream, StreamAccess, StreamDesc, StreamIter, StreamState}};

const BLOCK_SIZE: usize = 1<<16;

pub struct MemoryStream {
    element_type: ElementType,
    blocks: FrozenVec<Arc<AppendArray<u8>>>,
    streaming: AtomicBool,
    wakers: RwLock<Slab<AtomicWaker>>,
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

    fn register(&self, id: usize, waker: &Waker) {
        let wakers = self.wakers.read().unwrap();
        wakers.get(id).unwrap().register(waker);
    }

    fn notify_all(&self) {
        let wakers = self.wakers.read().unwrap();
        for (_, waker) in wakers.iter() {
            waker.wake();
        }
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
        let id = self.wakers.write().unwrap().insert(AtomicWaker::new());
        Box::new(MemoryStreamAccess { stream: self, id })
    }

    fn iter(self: Arc<Self>) -> Box<dyn StreamIter> {
        let id = self.wakers.write().unwrap().insert(AtomicWaker::new());
        Box::new(MemoryStreamIter {
            stream: self,
            block: 0,
            pos: 0,
            id,
        })
    }
}

pub struct MemoryStreamAccess {
    stream: Arc<MemoryStream>,
    id: usize,
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

    fn begin(&mut self, waker: &Waker) {
        // TODO: Only need to wake if this view is interested in the end of the stream
        self.stream.register(self.id, waker);
    }

    fn end(&mut self) {}
}

impl Drop for MemoryStreamAccess {
    fn drop(&mut self) {
        let mut wakers = self.stream.wakers.write().unwrap();
        wakers.remove(self.id);
    }
}

pub struct MemoryStreamIter {
    stream: Arc<MemoryStream>,
    block: usize,
    pos: usize,
    id: usize,
}

impl StreamIter for MemoryStreamIter {
    fn element_type(&self) -> ElementType {
        self.stream.element_type
    }

    fn poll_next(&mut self, cx: &mut Context) -> Poll<Result<&[u8], String>> {
        self.stream.register(self.id, cx.waker());

        let Some(block) = self.stream.blocks.get(self.block) else {
            if self.stream.streaming() {
                return Poll::Pending;
            } else {
                return Poll::Ready(Ok(&[]));
            }
        };

        let Some(data) = block.get(self.pos * self.stream.element_type.bytes()..) else {
            if self.stream.streaming() {
                return Poll::Pending;
            } else {
                return Poll::Ready(Ok(&[]));
            }
        };

        if data.is_empty() && self.stream.streaming() {
            return Poll::Pending;
        }

        Poll::Ready(Ok(data))
    }

    fn consume(&mut self, len: usize) {
        debug_assert!(self.pos + len <= BLOCK_SIZE);
        self.pos += len;

        if self.pos >= BLOCK_SIZE {
            self.block += 1;
            self.pos = 0;
        }
    }
}

impl Drop for MemoryStreamIter {
    fn drop(&mut self) {
        let mut wakers = self.stream.wakers.write().unwrap();
        wakers.remove(self.id);
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
        let stream = Arc::new(MemoryStream { blocks, element_type, streaming: AtomicBool::new(true), wakers: RwLock::new(Slab::new()) });
        MemoryStreamWriter { stream, writer }
    }

    pub fn stream(&self) -> &Arc<MemoryStream> {
        &self.stream
    }

    fn new_block(&mut self) {
        debug_assert!(self.writer.remaining_capacity() == 0, "Cannot extend when there is remaining capacity");
        self.writer = AppendArrayWriter::with_capacity(BLOCK_SIZE * self.stream.element_type.bytes());
        self.stream.blocks.push(self.writer.reader());
    }

    pub fn extend_from_slice(&mut self, mut data: &[u8]) {
        loop {
            data = self.writer.extend_from_slice(data);
            if data.is_empty() { break }
            self.new_block();
        }
        self.stream.notify_all();
    }

    pub fn pos(&self) -> Idx {
        let n_blocks = self.stream.blocks.len();
        ((n_blocks - 1) * BLOCK_SIZE + (self.writer.len() / self.stream.element_type.bytes())) as Idx
    }
}

impl Drop for MemoryStreamWriter {
    fn drop(&mut self) {
        self.stream.streaming.store(false, std::sync::atomic::Ordering::Relaxed);
        self.stream.notify_all();
    }
}

impl StreamWriter for MemoryStreamWriter {
    fn stream(&self) -> ArcStream {
        self.stream.clone()
    }

    fn pos(&self) -> Idx {
        self.pos()
    }

    fn desc(&self) -> StreamDesc {
        self.stream.desc()
    }

    fn poll_buf(&mut self, _cx: &mut Context) -> Poll<Result<&mut AppendArrayWriter<u8>, String>> {
        if self.writer.remaining_capacity() == 0 {
            // If the block is full, create a new block
            self.new_block();
        }

        Poll::Ready(Ok(&mut self.writer))
    }

    fn commit(&mut self) {
        self.stream.notify_all();
    }
}

pub struct MemoryStorage;

impl Storage for MemoryStorage {
    fn create_stream(&self, element_type: ElementType) -> Box<dyn StreamWriter> {
        Box::new(MemoryStreamWriter::new(element_type))
    }
}
