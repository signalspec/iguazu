use std::{any::Any, fmt::Debug, sync::Arc, task::{Context, Poll, Waker}};
use append_array::AppendArrayWriter;

use crate::{Idx, ElementType};

pub trait Stream: Send + Sync + Debug + Any {
    fn desc(&self) -> StreamDesc;

    fn state(&self) -> StreamState;

    fn access(self: Arc<Self>) -> Box<dyn StreamAccess>;

    fn iter(self: Arc<Self>) -> Box<dyn StreamIter>;
}

pub type ArcStream = Arc<dyn Stream>;

pub trait StreamAccess: Send  {
    fn get_block(&self, block: u64) -> &[u8];

    fn state(&self) -> StreamState;

    fn begin(&mut self, waker: &Waker);

    fn end(&mut self);
}

pub trait StreamIter: Send {
    fn poll_next(&mut self, cx: &mut Context) -> Poll<Result<&[u8], String>>;

    fn consume(&mut self, len: usize);
}

#[derive(Clone)]
pub struct StreamDesc {
    pub element_type: ElementType,
    pub block_size: usize,
}

#[derive(Debug)]
pub struct StreamState {
    pub end: Idx,
    pub streaming: bool,
}

pub trait StreamWriter: Send {
    fn stream(&self) -> ArcStream;

    fn pos(&self) -> Idx;

    /// Get the stream descriptor.
    fn desc(&self) -> StreamDesc;
    
    /// Access the writable buffer for the current block.
    /// 
    /// This can return `Poll::Pending` to apply backpressure, or an error if previous writes
    /// have failed.
    fn poll_buf(&mut self, cx: &mut Context) -> Poll<Result<&mut AppendArrayWriter<u8>, String>>;

    /// Notify readers that data has been written.
    fn commit(&mut self);
}
