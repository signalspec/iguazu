use std::{io, pin::Pin, sync::Arc};

use async_trait::async_trait;
use futures_lite::{AsyncBufRead, AsyncReadExt, AsyncWrite};

#[cfg(all(feature="fs", any(target_family = "unix", target_family = "windows")))]
mod fs;
#[cfg(all(feature="fs", any(target_family = "unix", target_family = "windows")))]
pub use fs::{FsFile, StdinFile, FsWritableFile};

#[cfg(all(feature="web", target_family = "wasm"))]
mod web;
#[cfg(all(feature="web", target_family = "wasm"))]
pub use web::{ WebFile, WebFetchFile };

mod url;
use ::url::Url;
pub use url::{RelativePath, BadRelativePath};

#[async_trait]
pub trait ReadableFile: Send + Sync + 'static {
    /// Get the filename part of the path
    fn filename(&self) -> Option<&str>;

    /// Get the absolute URL of the file
    fn url(&self) -> Option<Url>;

    /// Open a file adjacent to this one
    async fn relative(&self, path: &RelativePath) -> Result<Arc<dyn ReadableFile>, io::Error>;

    /// Get the length of the file
    async fn get_len(self: Arc<Self>) -> Result<u64, io::Error>;

    /// Read a chunk of the file
    async fn read_at(self: Arc<Self>, offset: u64, len: usize) -> Result<Vec<u8>, io::Error>;

    /// Read a chunk at the end of the file, also returning the total size
    ///
    /// Some backends, such as HTTP, can do this in a single request.
    async fn read_at_end(self: Arc<Self>, len: usize) -> Result<(Vec<u8>, u64), io::Error> {
        let total_len = self.clone().get_len().await?;
        let offset = total_len.saturating_sub(len as u64);
        let data = self.read_at(offset, len).await?;
        Ok((data, total_len))
    }

    /// Read the entire file into a `Vec<u8>`
    async fn read_all(self: Arc<Self>, limit: usize) -> Result<Vec<u8>, io::Error> {
        let mut take = self.stream(0, None).await?.take(limit as u64);
        let mut buf = Vec::new();
        take.read_to_end(&mut buf).await?;

        if take.limit() == 0 {
            return Err(io::Error::new(io::ErrorKind::FileTooLarge, "File too large"));
        }

        Ok(buf)
    }

    /// Create a stream for reading the file
    async fn stream(self: Arc<Self>, offset: u64, size: Option<u64>) -> Result<Pin<Box<dyn AsyncBufRead + Send + Sync>>, io::Error>;
}

#[async_trait]
pub trait WritableFile: Send + Sync + 'static {
    fn url(&self) -> Option<Url>;

    /// Get a `WritableFile` adjacent to this one
    fn relative(&self, path: &RelativePath) -> Arc<dyn WritableFile>;

    /// Create the file and get an `AsyncWrite`
    async fn writer(&self) -> Result<Pin<Box<dyn AsyncWrite + Send>>, io::Error>;
}
