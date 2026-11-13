//! Data import
use std::sync::Arc;

use async_executor::Task;
use thiserror::Error;

use crate::{config::Configurable, io::{IoError, ReadableFile}, schema::{EntitySchema, EntityStream}, storage::Pool, send::{NonWasmSend, BoxFuture}};

#[cfg(feature = "csv")]
mod column_parser;

mod raw;
pub use raw::RawImporter;

mod json_virtual;
pub use json_virtual::VirtualImporter;

#[cfg(feature = "csv")]
mod csv;
#[cfg(feature = "csv")]
pub use csv::CsvImporter;

#[cfg(feature = "izs")]
mod izs;
#[cfg(feature = "izs")]
pub use izs::IzsImporter;

#[cfg(feature = "srzip")]
mod srzip;
#[cfg(feature = "srzip")]
pub use srzip::SrZipImporter;

#[derive(Error, Debug)]
pub enum ImportError {
    #[error("IO error: {0}")]
    Io(#[from] IoError),

    #[error("Schema mismatch: {0}")]
    SchemaMismatch(String),

    #[error("File is malformed: {0}")]
    InvalidFile(String),
}

/// An object that holds the import options and can perform the import.
///
/// This is essentially a builder type for the import format, designed to be used behind `Box<dyn Importer>`.
pub trait Importer: Configurable + NonWasmSend {
    /// Should we prompt for options?
    fn should_show_options(&self) -> bool {
        !self.options().is_empty()
    }

    /// Load or infer the schema from a file.
    fn load_schema(&self) -> BoxFuture<'static, Result<EntitySchema, ImportError>>;

    /// Import a file.
    ///
    /// This returns a future that resolves once the metadata has been read,
    /// providing the [`EntityStream`] and a second future that resolves once
    /// the entire import is complete. Depending on the format, that may be
    /// immediately ready or may require reading and parsing the file.
    fn import(&self, schema: Option<EntitySchema>, pool: Arc<Pool>) -> BoxFuture<'static, Result<(EntityStream, Task<Result<(), ImportError>>), ImportError>>;

    /// The wrapped file
    fn file(&self) -> &Arc<dyn ReadableFile>;
}

/// A description of an import format.
pub struct ImportFormat {
    /// Internal identifier for the format.
    ///
    /// This should uniquely identify the format.
    pub name: &'static str,

    /// A human-readable description of the format.
    pub description: &'static str,

    /// Filename extensions to detect, including the leading `.`.
    pub extensions: &'static [&'static str],

    /// Create an [`Importer`] for a file.
    pub importer: fn (file: Arc<dyn ReadableFile>) -> Box<dyn Importer>,
}

impl ImportFormat {
    pub fn matches_filename(&self, name: &str) -> bool {
        self.extensions.iter().any(|ext| name.ends_with(ext))
    }

    pub fn importer(&self, file: Arc<dyn ReadableFile>) -> Box<dyn Importer> {
        (self.importer)(file)
    }
}

/// A wrapper around `&[ImportFormat]` that provides methods for selecting a format.
pub struct ImportFormats<'a>(&'a [ImportFormat]);

impl<'a> ImportFormats<'a> {
    pub fn iter(&self) -> std::slice::Iter<'a, ImportFormat> {
        self.0.iter()
    }

    pub fn by_name(&self, name: &str) -> Option<&'a ImportFormat> {
        self.iter().find(|imp| imp.name.eq_ignore_ascii_case(name))
    }

    pub fn first_for_filename(&self, fname: &str) -> Option<&'a ImportFormat> {
        self.iter().find(|imp| imp.matches_filename(fname))
    }

    pub fn importer_for_file(&self, file: Arc<dyn ReadableFile>) -> Option<Box<dyn Importer>> {
        self.first_for_filename(file.filename()).map(|imp| imp.importer(file))
    }
}

impl<'a> IntoIterator for &'a ImportFormats<'a> {
    type Item = &'a ImportFormat;
    type IntoIter = std::slice::Iter<'a, ImportFormat>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub const VIRTUAL: ImportFormat = ImportFormat {
    name: "virtual",
    description: "Iguazu Virtual JSON",
    extensions: &[".iguazu.json"],
    importer: |file| Box::new(json_virtual::VirtualImporter::new(file)),
};

#[cfg(feature = "izs")]
pub const IZS: ImportFormat = ImportFormat {
    name: "izs",
    description: "Iguazu Pack",
    extensions: &[".izs"],
    importer: |file| Box::new(izs::IzsImporter::new(file)),
};

pub const RAW: ImportFormat = ImportFormat {
    name: "raw",
    description: "Raw binary",
    extensions: &[".bin", ".logic8",
        ".f32", ".cf32", "cfile",
        ".u8", ".u16", ".u32", ".u64",
        ".s8", ".s16", ".s32", ".s64",
        ".cu8", ".cu16", ".cu32", ".cu64",
        ".cs8", ".cs16", ".cs32", ".cs64"
    ],
    importer: |file| Box::new(raw::RawImporter::for_file(file)),
};

#[cfg(feature = "csv")]
pub const CSV: ImportFormat = ImportFormat {
    name: "csv",
    description: "Comma-separated values",
    extensions: &[".csv"],
    importer: |file| Box::new(csv::CsvImporter::csv(file)),
};

#[cfg(feature = "csv")]
pub const TSV: ImportFormat = ImportFormat {
    name: "tsv",
    description: "Tab-separated values",
    extensions: &[".tsv"],
    importer: |file| Box::new(csv::CsvImporter::tsv(file))
};

#[cfg(feature = "srzip")]
pub const SRZIP: ImportFormat = ImportFormat {
    name: "sigrok",
    description: "Sigrok (srzip v2)",
    extensions: &[".sr"],
    importer: |file| Box::new(SrZipImporter::new(file))
};

pub const IMPORTERS: ImportFormats<'static> = ImportFormats(&[
    VIRTUAL,
    #[cfg(feature = "izs")] IZS,
    RAW,
    #[cfg(feature = "csv")] CSV,
    #[cfg(feature = "csv")] TSV,
    #[cfg(feature = "srzip")] SRZIP,
]);
