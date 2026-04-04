use std::{collections::BTreeMap, mem::size_of, pin::Pin, sync::Arc};
use async_compression::futures::bufread::DeflateDecoder;
use futures_lite::{AsyncBufRead, AsyncRead, AsyncReadExt as _};

use crate::io::ReadableFile;

pub struct EndOfCentralDirectoryRecord([u8; 22]);

impl EndOfCentralDirectoryRecord {
    const MAGIC: u32 = 0x06054b50;
    fn magic(&self) -> u32 { u32::from_le_bytes(self.0[0..4].try_into().unwrap()) }
    fn disk_num(&self) -> u16 { u16::from_le_bytes(self.0[4..6].try_into().unwrap()) }
    fn start_cent_dir_disk(&self) -> u16 { u16::from_le_bytes(self.0[6..8].try_into().unwrap()) }
    fn num_of_entries_disk(&self) -> u16 { u16::from_le_bytes(self.0[8..10].try_into().unwrap()) }
    fn num_entries(&self) -> u16 { u16::from_le_bytes(self.0[10..12].try_into().unwrap()) }
    fn size_cent_dir(&self) -> u32 { u32::from_le_bytes(self.0[12..16].try_into().unwrap()) }
    fn cent_dir_offset(&self) -> u32 { u32::from_le_bytes(self.0[16..20].try_into().unwrap()) }
    fn comment_length(&self) -> u16 { u16::from_le_bytes(self.0[20..22].try_into().unwrap()) }
}

pub struct CentralDirectoryFileHeader([u8; 46]);

impl CentralDirectoryFileHeader {
    #![allow(dead_code)]
    const MAGIC: u32 = 0x02014b50;
    fn magic(&self) -> u32 { u32::from_le_bytes(self.0[0..4].try_into().unwrap()) }
    fn v_made_by(&self) -> u16 { u16::from_le_bytes(self.0[4..6].try_into().unwrap()) }
    fn v_needed(&self) -> u16 { u16::from_le_bytes(self.0[6..8].try_into().unwrap()) }
    fn flags(&self) -> u16 { u16::from_le_bytes(self.0[8..10].try_into().unwrap()) }
    fn compression(&self) -> u16 { u16::from_le_bytes(self.0[10..12].try_into().unwrap()) }
    fn mod_time(&self) -> u16 { u16::from_le_bytes(self.0[12..14].try_into().unwrap()) }
    fn mod_date(&self) -> u16 { u16::from_le_bytes(self.0[14..16].try_into().unwrap()) }
    fn crc(&self) -> u32 { u32::from_le_bytes(self.0[16..20].try_into().unwrap()) }
    fn compressed_size(&self) -> u32 { u32::from_le_bytes(self.0[20..24].try_into().unwrap()) }
    fn uncompressed_size(&self) -> u32 { u32::from_le_bytes(self.0[24..28].try_into().unwrap()) }
    fn file_name_length(&self) -> u16 { u16::from_le_bytes(self.0[28..30].try_into().unwrap()) }
    fn extra_field_length(&self) -> u16 { u16::from_le_bytes(self.0[30..32].try_into().unwrap()) }
    fn file_comment_length(&self) -> u16 { u16::from_le_bytes(self.0[32..34].try_into().unwrap()) }
    fn disk_start(&self) -> u16 { u16::from_le_bytes(self.0[34..36].try_into().unwrap()) }
    fn inter_attr(&self) -> u16 { u16::from_le_bytes(self.0[36..38].try_into().unwrap()) }
    fn exter_attr(&self) -> u32 { u32::from_le_bytes(self.0[34..42].try_into().unwrap()) }
    fn lh_offset(&self) -> u32 { u32::from_le_bytes(self.0[42..46].try_into().unwrap()) }
}

pub struct LocalFileHeader([u8; 30]);

impl LocalFileHeader {
    #![allow(dead_code)]
    const MAGIC: u32 = 0x04034b50;
    fn magic(&self) -> u32 { u32::from_le_bytes(self.0[0..4].try_into().unwrap()) }
    fn compression(&self) -> u16 { u16::from_le_bytes(self.0[8..10].try_into().unwrap()) }
    fn crc(&self) -> u32 { u32::from_le_bytes(self.0[14..18].try_into().unwrap()) }
    fn compressed_size(&self) -> u32 { u32::from_le_bytes(self.0[18..22].try_into().unwrap()) }
    fn uncompressed_size(&self) -> u32 { u32::from_le_bytes(self.0[22..26].try_into().unwrap()) }
    fn file_name_length(&self) -> u16 { u16::from_le_bytes(self.0[26..28].try_into().unwrap()) }
    fn extra_field_length(&self) -> u16 { u16::from_le_bytes(self.0[28..30].try_into().unwrap()) }
}

pub struct ZipEntry {
    file: Arc<dyn ReadableFile>,
    name: Box<[u8]>,
    extra_len: usize,
    compression: u16,
    compressed_size: u64,
    uncompressed_size: u64,
    data_offset: u64,
}

pub enum UnzipReader {
    Stored(Pin<Box<dyn AsyncBufRead + Send + Sync + 'static>>),
    Deflate(DeflateDecoder<Pin<Box<dyn AsyncBufRead + Send + Sync + 'static>>>),
}

impl AsyncRead for UnzipReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            UnzipReader::Stored(r) => Pin::new(r).poll_read(cx, buf),
            UnzipReader::Deflate(d) => Pin::new(d).poll_read(cx, buf),
        }
    }
}

impl ZipEntry {
    pub fn file(&self) -> &Arc<dyn ReadableFile> { &self.file }
    pub fn name(&self) -> &[u8] { &self.name }
    pub fn uncompressed_size(&self) -> u64 { self.uncompressed_size }

    pub fn reader(&self) -> impl Future<Output = Result<UnzipReader, std::io::Error>> + Send + 'static {
        let compression  = self.compression;
        let name_len = self.name.len();
        let compressed_size = self.compressed_size;
        let uncompressed_size = self.uncompressed_size;
        let extra_len = self.extra_len;
        let file = self.file.clone();
        let data_offset = self.data_offset;

        async move {
            // Came from u16 so can't overflow.
            let lfh_size = size_of::<LocalFileHeader>() + name_len + extra_len;

            let read_size = compressed_size.checked_add(lfh_size as u64)
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid compressed size"))?;

            let mut stream = file.stream(data_offset, Some(read_size)).await?;

            let mut header = Vec::with_capacity(lfh_size);
            (&mut stream).take(lfh_size as u64).read_to_end(&mut header).await?;

            if header.len() != lfh_size {
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "unexpected end of file when reading zip entry"));
            }

            let lfh = LocalFileHeader(header[..size_of::<LocalFileHeader>()].try_into().unwrap());

            if lfh.magic() != LocalFileHeader::MAGIC {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid local file header in zip entry"));
            }

            if lfh.compression() != compression
            || lfh.compressed_size() as u64 != compressed_size
            || lfh.uncompressed_size() as u64 != uncompressed_size
            || lfh.file_name_length() as usize != name_len
            || lfh.extra_field_length() as usize != extra_len {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "local file header does not match central directory entry"));
            }

            Ok(match compression {
                0 => UnzipReader::Stored(stream),
                8 => UnzipReader::Deflate(DeflateDecoder::new(stream)),
                _ => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "unsupported compression method in zip entry"))
            })
        }
    }

    pub async fn read_all(&self, limit: usize) -> Result<Vec<u8>, std::io::Error> {
        if self.uncompressed_size > limit as u64 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "uncompressed size exceeds limit"));
        }

        let reader = self.reader().await?;
        let mut buf = Vec::with_capacity(self.uncompressed_size as usize);
        reader.take(limit as u64).read_to_end(&mut buf).await?;

        Ok(buf)
    }
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ZipError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    MalformedFile(&'static str),
    #[error("{0} is not supported")]
    UnsupportedFeature(&'static str),
    #[error("limit exceeded: {0}")]
    LimitExceeded(&'static str),
}

pub async fn load_zip_file(file: Arc<dyn ReadableFile>) -> Result<BTreeMap<Box<[u8]>, ZipEntry>, ZipError> {
    // File comments not supported, just assume EOCD is at the end.
    let (eocd, _) = file.clone().read_at_end(size_of::<EndOfCentralDirectoryRecord>()).await?;
    let eocd = EndOfCentralDirectoryRecord(eocd.try_into().map_err(|_| ZipError::MalformedFile("file too short"))?);

    if eocd.magic() != EndOfCentralDirectoryRecord::MAGIC {
        return Err(ZipError::MalformedFile("invalid magic number on end of central directory record"));
    }

    if eocd.disk_num() == 0xFFFF {
        return Err(ZipError::UnsupportedFeature("ZIP64 format"));
    }

    if eocd.comment_length() != 0 {
        // though we wouldn't have read this header in that case...
        return Err(ZipError::UnsupportedFeature("file comment"));
    }

    if eocd.disk_num() != 0 || eocd.start_cent_dir_disk() != 0 || eocd.num_of_entries_disk() != eocd.num_entries() {
        return Err(ZipError::UnsupportedFeature("multi-disk ZIP"));
    }

    let num_entries = eocd.num_entries();
    let central_directory_offset = eocd.cent_dir_offset() as u64;
    let central_directory_size = eocd.size_cent_dir() as u64;

    if central_directory_size > 8 * 1024 * 1024 {
        return Err(ZipError::LimitExceeded("central directory too large"));
    }

    let central_directory_data = file.clone().read_at(central_directory_offset, central_directory_size as usize).await?;
    let mut cdir = &central_directory_data[..];

    let mut entries = BTreeMap::new();

    while cdir.len() >= size_of::<CentralDirectoryFileHeader>() && entries.len() < num_entries as usize {
        let header = CentralDirectoryFileHeader(cdir[..size_of::<CentralDirectoryFileHeader>()].try_into().unwrap());

        if header.magic() != CentralDirectoryFileHeader::MAGIC {
            return Err(ZipError::MalformedFile("invalid magic number on central directory file header"));
        }

        let header_len = size_of::<CentralDirectoryFileHeader>();
        let name_len = header.file_name_length() as usize;
        let extra_len = header.extra_field_length() as usize;
        let comment_len = header.file_comment_length() as usize;
        let record_len = header_len + name_len + extra_len + comment_len;

        if cdir.len() < record_len {
            return Err(ZipError::MalformedFile("central directory file header truncated"));
        }

        let name = &cdir[header_len..][..name_len];

        entries.insert(name.into(), ZipEntry {
            file: file.clone(),
            name: name.into(),
            extra_len,
            compression: header.compression(),
            compressed_size: header.compressed_size() as u64,
            uncompressed_size: header.uncompressed_size() as u64,
            data_offset: header.lh_offset() as u64,
        });

        cdir = &cdir[record_len..];
    }

    Ok(entries)
}
