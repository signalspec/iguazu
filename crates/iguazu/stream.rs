use std::{fmt::Debug, sync::Arc, task::{Context, Poll, Waker}};
use crate::{Idx, ElementType};

pub trait Stream: Send + Sync + Debug {
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
