use std::{
    ffi::OsStr, fs::File, future::poll_fn, io, os::unix::fs::FileExt, path::{Path, PathBuf}, pin::Pin,
    sync::Arc, task::{ready, Poll},
};

use async_trait::async_trait;
use blocking::Task;
use futures_lite::{AsyncBufRead, AsyncRead, AsyncWrite, FutureExt};
use url::Url;

use crate::io::WritableFile;

use super::{ReadableFile, url::RelativePath};

pub struct FsFile {
    path: PathBuf,
    file: File,
}

impl FsFile {
    pub async fn open(path: PathBuf) -> Result<FsFile, io::Error> {
        blocking::unblock(move || {
            let path = std::path::absolute(path)?;
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

    fn url(&self) -> Option<Url> {
        Url::from_file_path(&self.path).ok()
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

    fn stream(self: Arc<Self>) -> Pin<Box<dyn AsyncBufRead + Send + Sync>> {
        Box::pin(FsFileStream::new(self))

    }

    async fn relative(&self, path: &RelativePath) -> Result<Arc<dyn ReadableFile>, std::io::Error> {
        Ok(Arc::new(Self::open(self.path.with_file_name(path)).await?))
    }
}

pub struct FsFileStream {
    task: Task<std::io::Result<()>>,
    reader: piper::Reader,
}

impl FsFileStream {
    pub fn new(file: Arc<FsFile>) -> Self {
        let (reader, mut writer) = piper::pipe(2 * 1024 * 1024);
        let task = blocking::unblock(move || futures_lite::future::block_on(async {
            let mut offset = 0;
            loop {
                if poll_fn(|cx| writer.poll(cx)).await == false {
                    return Ok(());
                };
                let buf = writer.write_buf(128 * 1024);
                let n = file.file.read_at(buf, offset)?;
                if n == 0 {
                    break;
                } else {
                    offset += n as u64;
                    writer.produced(n);
                }
            }
            Ok(())
        }));
        FsFileStream { reader, task }
    }
}

impl AsyncRead for FsFileStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &mut [u8]) -> std::task::Poll<std::io::Result<usize>> {
        let n = ready!(Pin::new(&mut self.reader).poll_read(cx, buf))?;
        if n == 0 {
            self.task.poll(cx).map_ok(|()| 0)
        } else {
            Poll::Ready(Ok(n))
        }
    }
}

impl AsyncBufRead for FsFileStream {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<io::Result<&[u8]>> {
        let this = Pin::into_inner(self);
        let buf = ready!(Pin::new(&mut this.reader).poll_fill_buf(cx))?;
        if buf.is_empty() {
            if !this.task.is_finished() {
                ready!(this.task.poll(cx))?;
            }
            return Poll::Ready(Ok(&[][..]));
        } else {
            Poll::Ready(Ok(buf))
        }
    }
    
    fn consume(mut self: Pin<&mut Self>, amt: usize) {
        self.reader.consume(amt);
    }
}

pub struct FsWritableFile {
    path: PathBuf,
}

impl FsWritableFile {
    pub fn new(path: &Path) -> Result<Self, io::Error> {
        Ok(Self::new_absolute(std::path::absolute(path)?))
    }

    pub fn new_absolute(path: PathBuf) -> Self {
        assert!(path.is_absolute());
        FsWritableFile { path }
    }
}

#[async_trait]
impl WritableFile for FsWritableFile {
    fn url(&self) -> Option<Url> {
        Url::from_file_path(&self.path).ok()
    }

    fn relative(&self, path: &RelativePath) -> Arc<dyn WritableFile> {
        Arc::new(Self::new_absolute(self.path.with_file_name(path)))
    }

    async fn writer(&self) -> Result<Pin<Box<dyn AsyncWrite + Send>>, io::Error> {
        let path = self.path.clone();
        blocking::unblock(move || {
            let file = File::create(&path)?;
            Ok(Box::pin(blocking::Unblock::new(file)) as Pin<Box<dyn AsyncWrite + Send>>)
        }).await
    }
}
