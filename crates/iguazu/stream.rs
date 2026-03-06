use std::{any::Any, fmt::Debug, future::poll_fn, io, pin::Pin, sync::Arc, task::{Context, Poll, Waker, ready}};
use once_array::OnceArrayWriter;

use crate::{Idx, ElementSize};

pub trait Stream: Send + Sync + Debug + Any {
    fn desc(&self) -> BlockDesc;

    fn state(&self) -> StreamState;

    fn access(self: Arc<Self>) -> Box<dyn StreamAccess>;

    fn iter(self: Arc<Self>) ->  Pin<Box<dyn Future<Output = Result<Box<dyn StreamIter>, io::Error>> + Send>>;
}

pub type ArcStream = Arc<dyn Stream>;

pub trait StreamAccess: Send  {
    fn get_block(&self, block: u64) -> &[u8];

    fn state(&self) -> StreamState;

    fn begin(&mut self, waker: &Waker);

    fn end(&mut self);
}

#[derive(Debug, PartialEq)]
pub enum IterState<'a> {
    /// The data received so far. Polling again may return more contiguous data.
    Partial(&'a [u8]),

    /// The data received in this block. Polling again will not return more data until this amount is consumed.
    ///
    /// If empty, indicates end of stream.
    Complete(&'a [u8]),

    /// An error has occurred.
    Error(&'a str),
}

impl<'a> IterState<'a> {
    pub fn complete_block(self) -> Poll<Result<&'a [u8], &'a str>> {
        match self {
            IterState::Partial(_) => Poll::Pending,
            IterState::Complete(data) => Poll::Ready(Ok(data)),
            IterState::Error(e) => Poll::Ready(Err(e)),
        }
    }

    pub fn at_least(self, needed_bytes: usize) -> Poll<Result<&'a [u8], &'a str>> {
        match self {
            IterState::Partial(data) if data.len() > needed_bytes => Poll::Ready(Ok(data)),
            IterState::Partial(_) => Poll::Pending,
            IterState::Complete(data) => Poll::Ready(Ok(data)),
            IterState::Error(e) => Poll::Ready(Err(e)),
        }
    }
}

pub trait StreamIter: Send {
    fn desc(&self) -> BlockDesc;

    fn poll_next(&mut self, cx: &mut Context) -> IterState<'_>;

    fn consume(&mut self, len: usize);
}

impl dyn StreamIter {
    pub async fn read_to_vec(&mut self, len: usize) -> Result<Vec<u8>, String> {
        let element_size = self.desc().element_size;
        let mut buf = Vec::with_capacity(len * element_size.bytes());
        poll_fn(|cx| -> Poll<Result<(), String>> {
            loop {
                let block = ready!(self.poll_next(cx).complete_block())?;
                let l = (block.len() / element_size.bytes()).min(len - buf.len() / element_size.bytes());
                buf.extend_from_slice(&block[.. l * element_size.bytes()]);

                if l > 0 {
                    self.consume(l);
                } else {
                    return Poll::Ready(Ok(()));
                }
            }
        }).await?;
        Ok(buf)
    }
}

/// Description of the layout of each block in a stream.
#[derive(Clone, Copy, Debug)]
pub struct BlockDesc {
    /// Size of each element.
    pub element_size: ElementSize,

    /// Count of elements per full block.
    pub count: usize,
}

impl BlockDesc {
    /// Size of a full block in bytes.
    pub fn size(&self) -> usize {
        self.element_size.bytes() * self.count
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StreamState {
    pub end: Idx,
    pub streaming: bool,
}

pub trait StreamWriter: Send {
    fn stream(&self) -> ArcStream;

    fn pos(&self) -> Idx;

    /// Get the stream descriptor.
    fn desc(&self) -> BlockDesc;

    /// Access the writable buffer for the current block.
    ///
    /// This can return `Poll::Pending` to apply backpressure, or an error if previous writes
    /// have failed.
    fn poll_buf(&mut self, cx: &mut Context) -> Poll<Result<&mut OnceArrayWriter<u8>, String>>;

    /// Notify readers that data has been written.
    fn commit(&mut self);
}
