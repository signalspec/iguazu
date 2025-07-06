use std::{ffi::OsStr, io, ops::Deref, path::Path, pin::Pin, sync::Arc};

use async_trait::async_trait;
use futures_lite::{AsyncBufRead, AsyncReadExt};
use serde::{de::{Error as _, Unexpected}, Deserialize, Deserializer, Serialize, Serializer};

#[cfg(all(feature="fs", any(target_family = "unix", target_family = "windows")))]
mod fs;
#[cfg(all(feature="fs", any(target_family = "unix", target_family = "windows")))]
pub use fs::{FsFile, FsFileStream};

/// A platform-independent relative path that does not contain a `..`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RelativePath(String);

impl Deref for RelativePath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<OsStr> for RelativePath {
    fn as_ref(&self) -> &OsStr {
        self.0.as_ref()
    }
}

impl AsRef<Path> for RelativePath {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

pub struct BadRelativePath(pub String);

impl TryFrom<String> for RelativePath {
    type Error = BadRelativePath;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !value.is_empty() && !value.contains('\\') && value.split('/').all(|c| c != "..") {
            Ok(RelativePath(value))
        } else {
            Err(BadRelativePath(value))
        }
    }
}

impl<'a> Deserialize<'a> for RelativePath {
    fn deserialize<D: Deserializer<'a>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?.try_into().map_err(|BadRelativePath(s)| {
            D::Error::invalid_value(Unexpected::Str(&s), &"relative path")
        })
    }
}

impl Serialize for RelativePath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

#[async_trait]
pub trait ReadableFile: Send + Sync + 'static {
    /// Get the filename part of the path
    fn filename(&self) -> Option<&str>;

    /// Open a file adjacent to this one
    async fn relative(&self, path: &RelativePath) -> Result<Arc<dyn ReadableFile>, io::Error>;

    /// Get the length of the file
    async fn get_len(self: Arc<Self>) -> Result<u64, io::Error>;

    /// Read a chunk of the file
    async fn read_at(self: Arc<Self>, offset: u64, len: usize) -> Result<Vec<u8>, io::Error>;

    /// Read the entire file into a `Vec<u8>`
    async fn read_all(self: Arc<Self>, limit: usize) -> Result<Vec<u8>, io::Error> {
        let mut take = self.stream().take(limit as u64);
        let mut buf = Vec::new();
        take.read_to_end(&mut buf).await?;

        if take.limit() == 0 {
            return Err(io::Error::new(io::ErrorKind::FileTooLarge, "File too large"));
        }

        Ok(buf)
    }

    /// Create a stream for reading the file
    fn stream(self: Arc<Self>) -> Pin<Box<dyn AsyncBufRead + Send + Sync>>;

}
