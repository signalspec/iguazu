use std::{
    fs::File, future::poll_fn, io, path::{Path, PathBuf}, pin::Pin,
    sync::{atomic::{AtomicBool, Ordering}, Arc}, task::{ready, Poll},
};

#[cfg(target_family = "unix")]
use std::os::unix::fs::FileExt;

#[cfg(target_family = "windows")]
use std::os::windows::fs::FileExt;

use blocking::Task;
use ecow::eco_format;
use futures_lite::{AsyncBufRead, AsyncRead, AsyncWrite, FutureExt};
use url::Url;

use crate::io::{IoError, ResultExt, WritableFile};

use super::{ReadableFile, url::RelativePath};

pub struct FsFile {
    path: PathBuf,
    filename: String,
    file: File,
}

impl FsFile {
    pub async fn open(path: PathBuf) -> Result<FsFile, IoError> {
        blocking::unblock(move || {
            let path = std::path::absolute(&path).map_err(|e| {
                IoError::new(eco_format!("failed to get absolute path for file: {e}"), None, Some(e.kind()))
            })?;

            let filename = path.file_name().map_or("<unnamed>".into(), |f| f.to_string_lossy().to_string());

            let file = File::open(&path)
                .map_err(|e| {
                    let url = Url::from_file_path(&path).ok().map(|u| u.as_str().into());
                    IoError::new(eco_format!("failed to open file: {e}"), url, Some(e.kind()))
                })?;
            Ok(FsFile { path, file, filename })
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

impl ReadableFile for FsFile {
    fn filename(&self) -> &str {
        self.filename.as_str()
    }

    fn url(&self) -> Option<Url> {
        Url::from_file_path(&self.path).ok()
    }

    fn get_len(self: Arc<Self>) -> Pin<Box<dyn Future<Output = Result<u64, IoError>> + Send + Sync>> {
        Box::pin(blocking::unblock(move || { Ok(self.file.metadata().err_from_file("failed to get file length", &*self)?.len()) }))
    }

    fn read_at(self: Arc<Self>, offset: u64, len: usize) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, IoError>> + Send + Sync>> {
        Box::pin(blocking::unblock(move || {
            let mut buf = vec![0; len];
            let mut pos = 0;

            while pos < len {
                let bytes_read = self.blocking_read_at(&mut buf[pos..], offset + pos as u64).err_from_file("failed to read file", &*self)?;

                if bytes_read == 0 {
                    buf.truncate(pos);
                    break;
                }
                pos += bytes_read;
            }

            Ok(buf)
        }))
    }

    fn stream(self: Arc<Self>, offset: u64, len: Option<u64>) -> Pin<Box<dyn Future<Output = Result<Pin<Box<dyn AsyncBufRead + Send + Sync>>, IoError>> + Send + Sync>> {
        Box::pin(async move {
            Ok(Box::pin(FsFileStream::new(self, offset, len)) as Pin<Box<dyn AsyncBufRead + Send + Sync>>)
        })
    }

    fn relative(&self, path: &RelativePath) -> Pin<Box<dyn Future<Output = Result<Arc<dyn ReadableFile>, IoError>> + Send + Sync>> {
        let path = self.path.with_file_name(path);
        Box::pin(async move {
            Ok(Arc::new(Self::open(path).await?) as Arc<dyn ReadableFile>)
        })
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

impl ReadableFile for StdinFile {
    fn filename(&self) -> &str {
        "<stdin>"
    }

    fn url(&self) -> Option<Url> {
        None
    }

    fn get_len(self: Arc<Self>) -> Pin<Box<dyn Future<Output = Result<u64, IoError>> + Send + Sync>> {
        Box::pin(async move {
            Err(IoError::new(
                "cannot get length of stdin".into(),
                Some("<stdin>".into()),
                std::io::ErrorKind::Unsupported.into(),
            ))
        })
    }

    fn read_at(self: Arc<Self>, _offset: u64, _len: usize) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, IoError>> + Send + Sync>> {
        Box::pin(async move {
            Err(IoError::new(
                "cannot seek stdin to offset".into(),
                Some("<stdin>".into()),
                std::io::ErrorKind::Unsupported.into(),
            ))
        })
    }

    fn stream(self: Arc<Self>, offset: u64, size: Option<u64>) -> Pin<Box<dyn Future<Output = Result<Pin<Box<dyn AsyncBufRead + Send + Sync>>, IoError>> + Send + Sync>> {
        Box::pin(async move {
            if offset != 0 || size.is_some() {
                return Err(IoError::new(
                    "cannot seek stdin to offset".into(),
                    Some("<stdin>".into()),
                    Some(std::io::ErrorKind::Unsupported),
                ))
            }

            let previously_used = self.used.swap(true, Ordering::Relaxed);
            if !previously_used {
                Ok(Box::pin(FsFileStream::stdin()) as Pin<Box<dyn AsyncBufRead + Send + Sync>>)
            } else {
                Err(IoError::new(
                    "stdin can only be read once".into(),
                    Some("<stdin>".into()),
                    Some(std::io::ErrorKind::Unsupported),
                ))
            }
        })
    }

    fn relative(&self, _path: &RelativePath) -> Pin<Box<dyn Future<Output = Result<Arc<dyn ReadableFile>, IoError>> + Send + Sync>> {
        Box::pin(async move {
            Err(IoError::new(
                "cannot open relative path".into(),
                Some("<stdin>".into()),
                Some(std::io::ErrorKind::Unsupported),
            ))
        })
    }
}

pub struct FsFileStream {
    task: Task<io::Result<()>>,
    reader: piper::Reader,
}

impl FsFileStream {
    pub fn new(file: Arc<FsFile>, mut offset: u64, mut size: Option<u64>) -> Self {
        let (reader, mut writer) = piper::pipe(2 * 1024 * 1024);
        let task = blocking::unblock(move || futures_lite::future::block_on(async {
            loop {
                if !poll_fn(|cx| writer.poll(cx)).await {
                    return Ok(());
                };
                let buf = writer.write_buf(size.unwrap_or(u64::MAX).min(128 * 1024) as usize);
                let n = file.blocking_read_at(buf, offset)?;
                if n == 0 {
                    break;
                } else {
                    writer.produced(n);
                    offset += n as u64;
                    if let Some(size) = &mut size {
                        *size -= n as u64;
                    }
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

impl WritableFile for FsWritableFile {
    fn url(&self) -> Option<Url> {
        Url::from_file_path(&self.path).ok()
    }

    fn relative(&self, path: &RelativePath) -> Arc<dyn WritableFile> {
        Arc::new(Self::new_absolute(self.path.with_file_name(path)))
    }

    fn writer(&self) -> Pin<Box<dyn Future<Output = Result<Pin<Box<dyn AsyncWrite + Send>>, io::Error>> + Send + Sync>> {
        let path = self.path.clone();
        Box::pin(blocking::unblock(move || {
            let file = File::create(&path)?;
            Ok(Box::pin(blocking::Unblock::new(file)) as Pin<Box<dyn AsyncWrite + Send>>)
        }))
    }
}
