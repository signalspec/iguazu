use std::{
    ffi::OsStr, fs::File, future::poll_fn, io, path::{Path, PathBuf}, pin::Pin,
    sync::{atomic::{AtomicBool, Ordering}, Arc}, task::{ready, Poll},
};

#[cfg(target_family = "unix")]
use std::os::unix::fs::FileExt;

#[cfg(target_family = "windows")]
use std::os::windows::fs::FileExt;

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

    #[cfg(target_family = "unix")]
    fn blocking_read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        self.file.read_at(buf, offset)
    }

    #[cfg(target_family = "windows")]
    fn blocking_read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        self.file.seek_read(buf, offset)
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

    async fn get_len(self: Arc<Self>) -> Result<u64, io::Error> {
        blocking::unblock(move || { Ok(self.file.metadata()?.len()) }).await
    }

    async fn read_at(self: Arc<Self>, offset: u64, len: usize) -> Result<Vec<u8>, io::Error> {
        blocking::unblock(move || {
            let mut buf = vec![0; len];
            let mut pos = 0;

            while pos < len {
                let bytes_read = self.blocking_read_at(&mut buf[pos..], offset + pos as u64)?;

                if bytes_read == 0 {
                    buf.truncate(pos);
                    break;
                }
                pos += bytes_read;
            }

            Ok(buf)
        }).await
    }

    async fn stream(self: Arc<Self>) -> Result<Pin<Box<dyn AsyncBufRead + Send + Sync>>, io::Error> {
        Ok(Box::pin(FsFileStream::new(self)))
    }

    async fn relative(&self, path: &RelativePath) -> Result<Arc<dyn ReadableFile>, io::Error> {
        Ok(Arc::new(Self::open(self.path.with_file_name(path)).await?))
    }
}

pub struct StdinFile {
    used: AtomicBool,
}

impl StdinFile {
    pub fn new() -> Self {
        StdinFile {
            used: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl ReadableFile for StdinFile {
    fn filename(&self) -> Option<&str> {
        Some("<stdin>")
    }

    fn url(&self) -> Option<Url> {
        None
    }

    async fn get_len(self: Arc<Self>) -> Result<u64, std::io::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Cannot get length of stdin",
        ))
    }

    async fn read_at(self: Arc<Self>, _offset: u64, _len: usize) -> Result<Vec<u8>, std::io::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Stdin does not support random access",
        ))
    }

    async fn stream(self: Arc<Self>) -> Result<Pin<Box<dyn AsyncBufRead + Send + Sync>>, io::Error> {
        let previously_used = self.used.swap(true, Ordering::Relaxed);
        if !previously_used {
            Ok(Box::pin(FsFileStream::stdin()))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Stdin can only be read once",
            ))
        }
    }

    async fn relative(&self, _path: &RelativePath) -> Result<Arc<dyn ReadableFile>, std::io::Error> {
        Err(std::io::Error::new(
            io::ErrorKind::Other,
            "Stdin does not support relative path references",
        ))
    }
}

pub struct FsFileStream {
    task: Task<io::Result<()>>,
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
                let n = file.blocking_read_at(buf, offset)?;
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

    pub fn stdin() -> Self {
        let (reader, mut writer) = piper::pipe(2 * 1024 * 1024);
        let task = blocking::unblock(move || futures_lite::future::block_on(async {
            let mut stdin = io::stdin().lock();
            loop {
                match poll_fn(|cx| writer.poll_fill(cx, &mut stdin)).await  {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        }));
        FsFileStream { reader, task }
    }
}

impl AsyncRead for FsFileStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &mut [u8]) -> std::task::Poll<io::Result<usize>> {
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
            Poll::Ready(Ok(&[][..]))
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
