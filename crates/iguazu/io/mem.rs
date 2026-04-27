use std::{
    io, pin::Pin, sync::Arc, task::{Context, Poll}
};

use async_trait::async_trait;
use futures_lite::{AsyncBufRead, AsyncRead};

use super::ReadableFile;

#[async_trait]
impl ReadableFile for Vec<u8> {
    fn filename(&self) -> Option<&str> {
        None
    }

    fn url(&self) -> Option<url::Url> {
        None
    }

    async fn get_len(self: Arc<Self>) -> Result<u64, io::Error> {
        Ok(self.len() as u64)
    }

    async fn read_at(self: Arc<Self>, offset: u64, len: usize) -> Result<Vec<u8>, io::Error> {
        let Ok(offset) = usize::try_from(offset) else {
            return Ok(Vec::new());
        };
        let end = offset.saturating_add(len).min(self.len());
        Ok(self.get(offset..end).unwrap_or(&[]).to_vec())
    }

    async fn stream(
        self: Arc<Self>,
        offset: u64,
        size: Option<u64>,
    ) -> Result<Pin<Box<dyn AsyncBufRead + Send + Sync>>, io::Error> {
        let pos = offset.try_into().unwrap_or(usize::MAX);
        let end = pos
            .saturating_add(size.unwrap_or(u64::MAX).try_into().unwrap_or(usize::MAX))
            .min(self.len());
        Ok(Box::pin(Cursor {
            data: self,
            pos,
            end,
        }))
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
