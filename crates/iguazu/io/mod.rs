//! File I/O abstraction layer
use std::{fmt::{Display, Debug}, io, pin::Pin, sync::Arc};

use ecow::{EcoString, eco_format};
use futures_lite::{AsyncBufRead, AsyncReadExt, AsyncWrite};

mod mem;

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

#[cfg(feature = "srzip")]
pub(crate) mod zip;

pub trait ReadableFile: Send + Sync + 'static {
    /// Description of the file. This will be a URL, a filename, or a `<placeholder>`.
    fn describe(&self) -> EcoString {
        self.url().map(|u| u.as_str().into()).unwrap_or_else(|| self.filename().into())
    }

    /// Get the filename part of the path, or a `<placeholder>` for pseudo-files.
    fn filename(&self) -> &str;

    /// Get the absolute URL of the file
    fn url(&self) -> Option<Url>;

    /// Open a file adjacent to this one
    fn relative(&self, _path: &RelativePath) -> Pin<Box<dyn Future<Output = Result<Arc<dyn ReadableFile>, IoError>> + Send + Sync>> {
        let describe = self.describe();
        Box::pin(async move {
            Err(IoError::new(eco_format!("could not open file by relative path"), Some(describe), Some(io::ErrorKind::Unsupported)))
        })
    }

    /// Get the length of the file
    fn get_len(self: Arc<Self>) -> Pin<Box<dyn Future<Output = Result<u64, IoError>> + Send + Sync>>;

    /// Read a chunk of the file
    fn read_at(self: Arc<Self>, offset: u64, len: usize) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, IoError>> + Send + Sync>>;

    /// Read a chunk at the end of the file, also returning the total size
    ///
    /// Some backends, such as HTTP, can do this in a single request.
    fn read_at_end(self: Arc<Self>, len: usize) -> Pin<Box<dyn Future<Output = Result<(Vec<u8>, u64), IoError>> + Send + Sync>> {
        Box::pin(async move {
            let total_len = self.clone().get_len().await?;
            let offset = total_len.saturating_sub(len as u64);
            let data = self.read_at(offset, len).await?;
            Ok((data, total_len))
        })
    }

    /// Read the entire file into a `Vec<u8>`
    fn read_all(self: Arc<Self>, limit: usize) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, IoError>> + Send + Sync>> {
        Box::pin(async move {
            let mut take = self.clone().stream(0, None).await?.take(limit as u64);
            let mut buf = Vec::new();
            take.read_to_end(&mut buf).await.err_from_file("failed to read from file", &*self)?;

            if take.limit() == 0 {
                return Err(IoError::new(eco_format!("size limit exceeded"), Some(self.describe()), Some(io::ErrorKind::FileTooLarge)));
            }

            Ok(buf)
        })
    }

    /// Create a stream for reading the file
    fn stream(self: Arc<Self>, offset: u64, size: Option<u64>) -> Pin<Box<dyn Future<Output = Result<Pin<Box<dyn AsyncBufRead + Send + Sync>>, IoError>> + Send + Sync>>;
}

pub trait WritableFile: Send + Sync + 'static {
    fn url(&self) -> Option<Url>;

    /// Get a `WritableFile` adjacent to this one
    fn relative(&self, path: &RelativePath) -> Arc<dyn WritableFile>;

    /// Create the file and get an `AsyncWrite`
    fn writer(&self) -> Pin<Box<dyn Future<Output = Result<Pin<Box<dyn AsyncWrite + Send>>, io::Error>> + Send + Sync>>;
}

#[derive(Debug, Clone)]
pub struct IoError {
    /// Error message.
    pub msg: EcoString,

    /// URL, filename, or pseudo-file type in `<>` of the file where the error occurred.
    pub file: Option<EcoString>,

    /// Error kind. This should be pre-formatted into `msg` and is not displayed.
    pub kind: Option<io::ErrorKind>,
}

impl IoError {
    pub fn new(msg: EcoString, file: Option<EcoString>, kind: Option<io::ErrorKind>) -> Self {
        Self { msg, file, kind }
    }
}

impl std::error::Error for IoError {}

impl Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(file) = &self.file {
            write!(f, "{}: {}", file, self.msg)
        } else {
            write!(f, "{}", self.msg)
        }
    }
}

pub trait ResultExt<T> {
    fn err_from_file<F: ReadableFile + ?Sized>(self, msg: &str, file: &F) -> Result<T, IoError>;
}

impl<T, E: Into<io::Error>> ResultExt<T> for Result<T, E> {
    fn err_from_file<F: ReadableFile + ?Sized>(self, msg: &str, file: &F) -> Result<T, IoError> {
        self.map_err(|e| {
            let e = e.into();
            IoError {
                msg: eco_format!("{msg}: {e}"),
                file: Some(file.describe()),
                kind: Some(e.kind()),
            }
        })
    }
}
