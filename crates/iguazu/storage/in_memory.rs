use std::sync::Mutex;
use std::{sync::Arc, task::{Context, Poll, Waker}};
use std::fmt::Debug;

use once_array::{OnceArrayWriter, OnceArray};
use crate::storage::{BlockState, Storage};
use crate::stream::{ArcStream, StreamIter, StreamWriter};
use crate::Idx;
use crate::util::waker_set::WakerSet;
use crate::{Element, ElementSize, stream::{Stream, BlockDesc, StreamState}};

const BLOCK_SIZE: usize = 1<<16;

struct State {
    streaming: bool,
    blocks: Vec<Arc<OnceArray<u8>>>,
}

pub struct MemoryStream {
    element_type: ElementSize,
    state: Mutex<State>,

    /// Wakers waiting for new data at the end of the stream.
    wakers: WakerSet,
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
        let state = self.state.lock().unwrap();
        let n_blocks = state.blocks.len();
        let last_block = state.blocks.get(n_blocks - 1).unwrap().len() / self.element_type.bytes();
        let end = ((n_blocks - 1) * BLOCK_SIZE + last_block) as u64;

        StreamState {
            streaming: state.streaming,
            end,
        }
    }

    fn get_block(&self, block_i: u64, waker: &Waker) -> BlockState {
        let Ok(block_i) = usize::try_from(block_i) else {
            return BlockState::End;
        };

        let state = self.state.lock().unwrap();
        let block = state.blocks.get(block_i);

        if state.streaming && block_i >= state.blocks.len().saturating_sub(1) {
            self.wakers.insert(waker);
            block.map_or(BlockState::Pending, |block| BlockState::Partial(Arc::clone(block)))
        } else {
            block.map_or(BlockState::End, |block| BlockState::Complete(Arc::clone(block)))
        }
    }

    fn iter(self: Arc<Self>) -> Box<dyn StreamIter> {
        Box::new(crate::storage::common::CommonStreamIter::new(self as ArcStream))
    }
}

pub struct MemoryStreamWriter {
    stream: Arc<MemoryStream>,
    writer: OnceArrayWriter<u8>,
}

impl MemoryStreamWriter {
    pub fn new(element_type: ElementSize) -> MemoryStreamWriter {
        let writer = OnceArrayWriter::with_capacity(BLOCK_SIZE * element_type.bytes());
        let stream = Arc::new(MemoryStream {
            state: Mutex::new(State {
                blocks: vec![writer.reader().clone()],
                streaming: true,
            }),
            element_type,
            wakers: WakerSet::new(),
        });
        MemoryStreamWriter { stream, writer }
    }

    pub fn stream(&self) -> &Arc<MemoryStream> {
        &self.stream
    }

    fn new_block(&mut self) {
        debug_assert!(self.writer.remaining_capacity() == 0, "Cannot start new block when there is remaining capacity");
        self.commit();
        self.writer = OnceArrayWriter::with_capacity(BLOCK_SIZE * self.stream.element_type.bytes());
        self.stream.state.lock().unwrap().blocks.push(self.writer.reader().clone());
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
        self.stream.wakers.wake_all();
    }

    pub fn pos(&self) -> Idx {
        let n_blocks = self.stream.state.lock().unwrap().blocks.len();
        ((n_blocks - 1) * BLOCK_SIZE + (self.writer.as_slice().len() / self.stream.element_type.bytes())) as Idx
    }
}

impl Drop for MemoryStreamWriter {
    fn drop(&mut self) {
        self.stream.state.lock().unwrap().streaming = false;
        self.stream.wakers.wake_all();
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
