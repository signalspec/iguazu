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

pub trait StreamIter: Send {
    fn element_type(&self) -> ElementSize;

    fn poll_next(&mut self, cx: &mut Context) -> Poll<Result<&[u8], String>>;

    fn consume(&mut self, len: usize);
}

impl dyn StreamIter {
    pub async fn read_to_vec(&mut self, len: usize) -> Result<Vec<u8>, String> {
        let element_type = self.element_type();
        let mut buf = Vec::with_capacity(len * element_type.bytes());
        loop {
            let l = poll_fn(|cx| {
                let block = ready!(self.poll_next(cx))?;
                let l = (block.len() / element_type.bytes()).min(len - buf.len() / element_type.bytes());
                buf.extend_from_slice(&block[.. l * element_type.bytes()]);
                Poll::Ready(Result::<usize, String>::Ok(l))
            }).await?;

            if l == 0 { break; }

            self.consume(l); // TODO: elements
        }
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

#[derive(Debug)]
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
