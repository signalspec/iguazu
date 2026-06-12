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

    async fn stream(self: Arc<Self>, start: u64, len: Option<u64>) -> Result<Pin<Box<dyn AsyncBufRead + Send + Sync>>, io::Error> {
        let stream = match (start, len) {
            (0, None) => self.file.stream(),
            (start, None) => self.file.slice_with_f64(start as f64).unwrap().stream(),
            (start, Some(len)) => self.file.slice_with_f64_and_f64(start as f64, (start + len) as f64).unwrap().stream(),
        };
        Ok(Box::pin(ReadableStreamReader::new(stream)))
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

pub struct WebFetchFile {
    url: Url,
}

impl WebFetchFile {
    pub fn new(url: Url) -> WebFetchFile {
        WebFetchFile { url }
    }

    async fn fetch(&self, range_header: Option<&str>) -> Result<web_sys::Response, io::Error> {
        let fetch_promise = {
            let req = web_sys::Request::new_with_str(&self.url.as_str()).unwrap();

            if let Some(range) = range_header {
                req.headers().set("Range", range).unwrap();
            }

            web_sys::window().unwrap().fetch_with_request(&req)
        };

        let response = SendWrapper::new(JsFuture::from(fetch_promise))
            .await
            .map_err(|e| {
                io::Error::other(format!("GET {} failed: {:?}", self.url, e))
            })?
            .unchecked_into::<web_sys::Response>();

        if response.ok() {
            Ok(response)
        } else {
            Err(io::Error::other(format!("GET {} returned: {:?}", self.url, response.status())))
        }
    }

    async fn fetch_range(&self, offset: u64, len: Option<u64>) -> Result<web_sys::Response, io::Error> {
        let range = match (offset, len) {
            (0, None) => { None },
            (start, None) => Some(format!("bytes={}-", start)),
            (start, Some(len)) => Some(format!("bytes={}-{}", start, start + len - 1)),
        };

        self.fetch(range.as_deref()).await
    }
}

#[async_trait]
impl ReadableFile for WebFetchFile {
    fn filename(&self) -> Option<&str> {
        self.url.path_segments().and_then(|segments| segments.last())
    }

    fn url(&self) -> Option<Url> {
        Some(self.url.clone())
    }

    async fn get_len(self: Arc<Self>) -> Result<u64, std::io::Error> {
        let fetch_promise = {
            let init = web_sys::RequestInit::new();
            init.set_method("HEAD");
            let req = web_sys::Request::new_with_str_and_init(&self.url.as_str(), &init).unwrap();
            web_sys::window().unwrap().fetch_with_request(&req)
        };

        let response = SendWrapper::new(JsFuture::from(fetch_promise)).await
            .map_err(|e| io::Error::other(format!("HEAD {} failed: {:?}", self.url, e)))?
            .unchecked_into::<web_sys::Response>();

        if !response.ok() {
            return Err(io::Error::other(format!("HEAD {} returned: {:?}", self.url, response.status())))
        }

        Ok(response.headers().get("Content-Length").ok().flatten().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0))
    }

    async fn read_at(self: Arc<Self>, offset: u64, len: usize) -> Result<Vec<u8>, std::io::Error> {
        let array_buffer_promise = {
            let response = self.fetch_range(offset, Some(len as u64)).await?;
            response.array_buffer().map_err(|e| {
                io::Error::other(format!("Failed to get array buffer promise: {:?}", e))
            })?
        };

        let array_buffer = SendWrapper::new(JsFuture::from(array_buffer_promise))
            .await
            .map_err(|e| io::Error::other(format!("Failed to get array buffer: {:?}", e)))?;

        let uint8_array = Uint8Array::new(&array_buffer);
        Ok(uint8_array.to_vec())
    }

    async fn read_at_end(self: Arc<Self>, size: usize) -> Result<(Vec<u8>, u64), std::io::Error> {
        let (array_buffer_promise, total_size) = {
            let response = self.fetch(Some(&format!("bytes=-{}", size))).await?;
            let content_range = response.headers().get("Content-Range").ok().flatten()
                .ok_or_else(|| io::Error::other(format!("No Content-range in response")))?;

            let total_size = parse_content_range_total(&content_range)
                .ok_or_else(|| io::Error::other(format!("Invalid Content-range header: {}", content_range)))?;

            let p = response.array_buffer().map_err(|e| {
                io::Error::other(format!("Failed to get array buffer promise: {:?}", e))
            })?;

            (p, total_size)
        };

        let array_buffer = SendWrapper::new(JsFuture::from(array_buffer_promise))
            .await
            .map_err(|e| io::Error::other(format!("Failed to get array buffer: {:?}", e)))?;

        let uint8_array = Uint8Array::new(&array_buffer);

        Ok((uint8_array.to_vec(), total_size))
    }

    async fn stream(self: Arc<Self>, start: u64, len: Option<u64>) -> Result<Pin<Box<dyn AsyncBufRead + Send + Sync>>, io::Error> {
        let response = self.fetch_range(start, len).await?;
        let stream = response.body().ok_or_else(|| io::Error::other("Response has no body"))?;
        Ok(Box::pin(ReadableStreamReader::new(stream)))
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
    done: bool,
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
            done: false,
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

        if this.buf_offset >= this.buf.len() && !this.done {
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

            if read_result.get_done().unwrap() {
                this.done = true;
            } else {
                let chunk = read_result.get_value().dyn_into::<Uint8Array>().unwrap();
                let len = chunk.length() as usize;
                this.buf.clear();
                this.buf.reserve(len);
                unsafe {
                    // Safety: the capacity has been set
                    chunk.raw_copy_to_ptr(this.buf.as_mut_ptr());
                    // Safety: len bytes have been initialized
                    this.buf.set_len(len);
                }
                this.buf_offset = 0;
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

fn parse_content_range_total(header: &str) -> Option<u64> {
    // Format: "bytes {offset}-{end}/{total}"
    let content_range = header.strip_prefix("bytes ")?;
    let (_, total) = content_range.split_once('/')?;
    let total = total.parse::<u64>().ok()?;
    Some(total)
}
