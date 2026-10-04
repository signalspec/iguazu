use std::{
    io, pin::Pin, sync::Arc, task::{Context, Poll}
};

use futures_lite::{AsyncBufRead, AsyncRead};

use crate::io::IoError;
use super::ReadableFile;

impl ReadableFile for Vec<u8> {
    fn filename(&self) -> &str {
        "<memory>"
    }

    fn url(&self) -> Option<url::Url> {
        None
    }

    fn get_len(self: Arc<Self>) -> Pin<Box<dyn Future<Output = Result<u64, IoError>> + Send + Sync>> {
        Box::pin(async move {
            Ok(self.len() as u64)
        })
    }

    fn read_at(self: Arc<Self>, offset: u64, len: usize) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, IoError>> + Send + Sync>> {
        Box::pin(async move {
            let Ok(offset) = usize::try_from(offset) else {
                return Ok(Vec::new());
            };
            let end = offset.saturating_add(len).min(self.len());
            Ok(self.get(offset..end).unwrap_or(&[]).to_vec())
        })
    }

    fn stream(
        self: Arc<Self>,
        offset: u64,
        size: Option<u64>,
    ) -> Pin<Box<dyn Future<Output = Result<Pin<Box<dyn AsyncBufRead + Send + Sync>>, IoError>> + Send + Sync>> {
        Box::pin(async move {
            let pos = offset.try_into().unwrap_or(usize::MAX);
            let end = pos
                .saturating_add(size.unwrap_or(u64::MAX).try_into().unwrap_or(usize::MAX))
                .min(self.len());
            Ok(Box::pin(Cursor {
                data: self,
                pos,
                end,
            }) as Pin<Box<dyn AsyncBufRead + Send + Sync>>)
        })
    }
}

struct Cursor {
    data: Arc<Vec<u8>>,
    pos: usize,
    end: usize,
}

impl AsyncRead for Cursor {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        if self.pos >= self.end {
            return Poll::Ready(Ok(0));
        }
        let n = std::cmp::min(buf.len(), self.end - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Poll::Ready(Ok(n))
    }
}

impl AsyncBufRead for Cursor {
    fn poll_fill_buf(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<&[u8], io::Error>> {
        if self.pos >= self.end {
            return Poll::Ready(Ok(&[]));
        }
        let this = self.get_mut();
        Poll::Ready(Ok(&this.data[this.pos..this.end]))
    }

    fn consume(mut self: Pin<&mut Self>, amt: usize) {
        self.pos = std::cmp::min(self.pos.saturating_add(amt), self.end);
    }
}
