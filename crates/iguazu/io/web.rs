use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Poll, ready},
};

use async_trait::async_trait;
use futures_lite::{AsyncBufRead, AsyncRead, FutureExt};
use send_wrapper::SendWrapper;
use url::Url;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    ReadableStream, ReadableStreamDefaultReader, js_sys::Uint8Array, wasm_bindgen::JsCast,
};

use super::{ReadableFile, url::RelativePath};

pub struct WebFile {
    file: SendWrapper<web_sys::File>,
    name: String,
}

impl WebFile {
    /// Wrap a JS file object
    pub fn new(file: web_sys::File) -> WebFile {
        let name = file.name();
        WebFile {
            file: SendWrapper::new(file),
            name,
        }
    }
}

#[async_trait]
impl ReadableFile for WebFile {
    fn filename(&self) -> Option<&str> {
        Some(&self.name)
    }

    fn url(&self) -> Option<Url> {
        None
    }

    async fn get_len(self: Arc<Self>) -> Result<u64, std::io::Error> {
        Ok(self.file.size() as u64)
    }

    async fn read_at(self: Arc<Self>, offset: u64, len: usize) -> Result<Vec<u8>, std::io::Error> {
        let slice_blob = SendWrapper::new(
            self.file
                .slice_with_f64_and_f64(offset as f64, (offset + len as u64) as f64)
                .map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::Other,
                        format!("Failed to slice file: {:?}", e),
                    )
                })?,
        );

        let array_buffer = SendWrapper::new(JsFuture::from(slice_blob.array_buffer()))
            .await
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("Failed to get array buffer: {:?}", e),
                )
            })?;

        let uint8_array = Uint8Array::new(&array_buffer);
        Ok(uint8_array.to_vec())
    }

    fn stream(self: Arc<Self>) -> Pin<Box<dyn AsyncBufRead + Send + Sync>> {
        Box::pin(ReadableStreamReader::new(self.file.stream()))
    }

    async fn relative(
        &self,
        _path: &RelativePath,
    ) -> Result<Arc<dyn ReadableFile>, std::io::Error> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "WebFile.relative not supported",
        ))
    }
}

pub struct ReadableStreamReader {
    reader: SendWrapper<ReadableStreamDefaultReader>,
    buf: Vec<u8>,
    buf_offset: usize,
    future: Option<SendWrapper<JsFuture>>,
}

impl ReadableStreamReader {
    pub fn new(stream: ReadableStream) -> Self {
        let reader = stream
            .get_reader()
            .unchecked_into::<ReadableStreamDefaultReader>();
        ReadableStreamReader {
            reader: SendWrapper::new(reader),
            buf: Vec::new(),
            buf_offset: 0,
            future: None,
        }
    }
}

impl AsyncRead for ReadableStreamReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let available = ready!(self.as_mut().poll_fill_buf(cx))?;
        let n = std::cmp::min(available.len(), buf.len());
        buf[..n].copy_from_slice(&available[..n]);
        self.as_mut().consume(n);
        Poll::Ready(Ok(n))
    }
}

impl AsyncBufRead for ReadableStreamReader {
    fn poll_fill_buf(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<&[u8]>> {
        let this = self.get_mut();

        if this.buf_offset >= this.buf.len() {
            let fut = this
                .future
                .get_or_insert_with(|| SendWrapper::new(JsFuture::from(this.reader.read())));
            let read_result = ready!(fut.poll(cx))
                .map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::Other,
                        format!("Failed to read stream chunk: {:?}", e),
                    )
                })?
                .unchecked_into::<web_sys::ReadableStreamReadResult>();

            this.future = None;
            let chunk = read_result.get_value();
            if !chunk.is_null() {
                let chunk = chunk.dyn_into::<Uint8Array>().unwrap();
                this.buf = chunk.to_vec(); // TODO: reuse allocation
                this.buf_offset = 0;
            } else {
                return Poll::Ready(Ok(&[]));
            }
        }

        let available = &this.buf[this.buf_offset..];
        Poll::Ready(Ok(available))
    }

    fn consume(mut self: Pin<&mut Self>, amt: usize) {
        debug_assert!(self.buf_offset + amt <= self.buf.len());
        self.buf_offset += amt;
    }
}

impl Drop for ReadableStreamReader {
    fn drop(&mut self) {
        drop(self.reader.cancel()); // promise ignored
    }
}
