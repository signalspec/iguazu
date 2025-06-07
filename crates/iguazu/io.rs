use std::{ffi::OsStr, fs::File, io, ops::Deref, os::unix::fs::FileExt, path::{Path, PathBuf}, sync::Arc};

use async_trait::async_trait;
use serde::{de::{Error as _, Unexpected}, Deserialize, Deserializer, Serialize, Serializer};


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
}

pub struct FsFile {
    path: PathBuf,
    file: File,
}

impl FsFile {
    pub async fn new(path: PathBuf) -> Result<FsFile, io::Error> {
        blocking::unblock(move || {
            let file = File::open(&path)?;
            Ok(FsFile { path, file })
        }).await
    }
}

#[async_trait]
impl ReadableFile for FsFile {
    fn filename(&self) -> Option<&str> {
        self.path.file_name().and_then(OsStr::to_str)
    }

    async fn get_len(self: Arc<Self>) -> Result<u64, std::io::Error> {
        blocking::unblock(move || { Ok(self.file.metadata()?.len()) }).await
    }

    async fn read_at(self: Arc<Self>, offset: u64, len: usize) -> Result<Vec<u8>, std::io::Error> {
        blocking::unblock(move || {
            let mut buf = vec![0; len];
            let mut pos = 0;

            while pos < len {
                let bytes_read = self.file.read_at(&mut buf, offset + pos as u64)?;
                if bytes_read == 0 {
                    buf.truncate(pos);
                    break;
                }
                pos += bytes_read;
            }

            Ok(buf)
        }).await
    }
    
    async fn relative(&self, path: &RelativePath) -> Result<Arc<dyn ReadableFile>, std::io::Error> {
        Ok(Arc::new(Self::new(self.path.with_file_name(path)).await?))
    }
}

