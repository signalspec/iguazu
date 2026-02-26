use std::pin::Pin;
use std::sync::RwLock;
use std::task::{Context, Poll};
use std::{sync::{Arc, atomic::AtomicBool}, task::Waker};
use std::fmt::Debug;

use once_array::{OnceArrayWriter, OnceArray};
use atomic_waker::AtomicWaker;
use elsa::sync::FrozenVec;
use slab::Slab;
use crate::storage::Storage;
use crate::stream::{ArcStream, IterState, StreamWriter};
use crate::Idx;
use crate::{Element, ElementSize, stream::{Stream, StreamAccess, BlockDesc, StreamIter, StreamState}};

const BLOCK_SIZE: usize = 1<<16;

pub struct MemoryStream {
    element_type: ElementSize,
    blocks: FrozenVec<Arc<OnceArray<u8>>>,
    streaming: AtomicBool,
    wakers: RwLock<Slab<AtomicWaker>>,
}

impl MemoryStream {
    pub fn new<T: Element>(data: &[T]) -> Arc<Self> {
        Self::raw(T::ELEMENT_SIZE, bytemuck::cast_slice(data))
    }

    pub fn raw(element_type: ElementSize, data: &[u8]) -> Arc<Self> {
        let mut writer = MemoryStreamWriter::new(element_type);
        writer.extend_from_slice(data);
        writer.commit();
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
    fn desc(&self) -> BlockDesc {
        BlockDesc {
            element_size: self.element_type,
            count: BLOCK_SIZE,
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

    fn iter(self: Arc<Self>) -> Pin<Box<dyn Future<Output = Result<Box<dyn StreamIter>, std::io::Error>> + Send + 'static>> {
        Box::pin(async move {
            let id = self.wakers.write().unwrap().insert(AtomicWaker::new());
            Ok(Box::new(MemoryStreamIter {
                stream: self,
                block: 0,
                pos: 0,
                id,
            }) as Box<dyn StreamIter>)
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
    fn desc(&self) -> BlockDesc {
        self.stream.desc()
    }

    fn poll_next(&mut self, cx: &mut Context) -> IterState<'_> {
        let block = self.stream.blocks.get(self.block).map_or(&[][..], |b| b.as_slice());
        let data = &block[self.pos * self.stream.element_type.bytes()..];
        if block.len() < BLOCK_SIZE * self.stream.element_type.bytes() && self.stream.streaming() {
            self.stream.register(self.id, cx.waker());
            IterState::Partial(data)
        } else {
            IterState::Complete(data)
        }
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
    writer: OnceArrayWriter<u8>,
}

impl MemoryStreamWriter {
    pub fn new(element_type: ElementSize) -> MemoryStreamWriter {
        let writer = OnceArrayWriter::with_capacity(BLOCK_SIZE * element_type.bytes());
        let blocks = FrozenVec::new();
        blocks.push(writer.reader().clone());
        let stream = Arc::new(MemoryStream { blocks, element_type, streaming: AtomicBool::new(true), wakers: RwLock::new(Slab::new()) });
        MemoryStreamWriter { stream, writer }
    }

    pub fn stream(&self) -> &Arc<MemoryStream> {
        &self.stream
    }

    fn new_block(&mut self) {
        debug_assert!(self.writer.remaining_capacity() == 0, "Cannot start new block when there is remaining capacity");
        self.commit();
        self.writer = OnceArrayWriter::with_capacity(BLOCK_SIZE * self.stream.element_type.bytes());
        self.stream.blocks.push(self.writer.reader().clone());
    }

    pub fn extend_from_slice(&mut self, mut data: &[u8]) {
        loop {
            data = self.writer.extend_from_slice(data);
            if data.is_empty() { break }
            self.new_block();
        }
    }

    pub fn commit(&mut self) {
        self.writer.commit();
        self.stream.notify_all();
    }

    pub fn pos(&self) -> Idx {
        let n_blocks = self.stream.blocks.len();
        ((n_blocks - 1) * BLOCK_SIZE + (self.writer.as_slice().len() / self.stream.element_type.bytes())) as Idx
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

    fn desc(&self) -> BlockDesc {
        self.stream.desc()
    }

    fn poll_buf(&mut self, _cx: &mut Context) -> Poll<Result<&mut OnceArrayWriter<u8>, String>> {
        if self.writer.remaining_capacity() == 0 {
            // If the block is full, create a new block
            self.new_block();
        }

        Poll::Ready(Ok(&mut self.writer))
    }

    fn commit(&mut self) {
        self.commit();
    }
}

pub struct MemoryStorage;

impl Storage for MemoryStorage {
    fn create_stream(&self, element_type: ElementSize) -> Box<dyn StreamWriter> {
        Box::new(MemoryStreamWriter::new(element_type))
    }
}
