use std::{pin::Pin, sync::Arc};

use thiserror::Error;

use crate::{io::ReadableFile, schema::{EntitySchema, EntityStream}, storage::Pool};

mod column_parser;

mod flat_file;
pub use flat_file::FlatFileImporter;

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

#[derive(Error, Debug)]
pub enum ImportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Schema mismatch: {0}")]
    SchemaMismatch(String),

    #[error("File is malformed: {0}")]
    InvalidFile(String),
}

/// An option for an [`Importer`].
pub struct OptionDescription {
    /// The name of the option passed to `set` and `get`.
    pub name: &'static str,

    /// A human-readable description of the option.
    pub description: &'static str,
}

/// An object that holds the import options and can perform the import.
///
/// This is essentially a builder type for the import format, designed to be used behind `Box<dyn Importer>`.
pub trait Importer: Send {
    /// List available options.
    fn options(&self) -> &'static [ OptionDescription ] {
        &[]
    }

    /// Set an option.
    ///
    /// The option keys and allowed values depend on the importer type.
    fn set(&mut self, option: &str, value: &str) -> Result<(), String> {
        let _ = (option, value);
        Err(format!("Unknown option"))
    }

    /// Get the current value of an option.
    fn get(&self, option: &str) -> Option<String> {
        let _ = option;
        None
    }

    /// Load or infer the schema from a file.
    fn load_schema(&self, file: Arc<dyn ReadableFile>) -> Pin<Box<dyn Future<Output = Result<EntitySchema, ImportError>> + Send + '_>>;

    /// Import a file.
    ///
    /// This returns a future that resolves once the metadata has been read,
    /// providing the [`EntityStream`] and a second future that resolves once
    /// the entire import is complete. Depending on the format, that may be
    /// immediately ready or may require reading and parsing the file.
    fn import(&self, file: Arc<dyn ReadableFile>, schema: Option<EntitySchema>, pool: Arc<Pool>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send + '_>>;
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

    /// Create an [`Importer`].
    pub importer: fn () -> Box<dyn Importer>,
}

impl ImportFormat {
    pub fn matches_filename(&self, name: &str) -> bool {
        self.extensions.iter().any(|ext| name.ends_with(ext))
    }

    pub fn importer(&self) -> Box<dyn Importer> {
        (self.importer)()
    }
}

/// A wrapper around `&[ImportFormat]` that provides methods for selecting a format.
pub struct ImportFormats<'a>(&'a [ImportFormat]);

impl<'a> ImportFormats<'a> {
    pub fn iter(&self) -> std::slice::Iter<'a, ImportFormat> {
        self.0.as_ref().iter()
    }

    pub fn by_name(&self, name: &str) -> Option<&'a ImportFormat> {
        self.iter().find(|imp| imp.name.eq_ignore_ascii_case(name))
    }

    pub fn first_for_filename(&self, fname: &str) -> Option<&'a ImportFormat> {
        self.iter().find(|imp| imp.matches_filename(fname))
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
    importer: || Box::new(json_virtual::VirtualImporter::new()),
};

#[cfg(feature = "izs")]
pub const IZS: ImportFormat = ImportFormat {
    name: "izs",
    description: "Iguazu Pack",
    extensions: &[".izs"],
    importer: || Box::new(izs::IzsImporter::new()),
};

pub const BIN: ImportFormat = ImportFormat {
    name: "bin",
    description: "Raw binary",
    extensions: &[".bin"],
    importer: || Box::new(flat_file::FlatFileImporter::binary()),
};

pub const LOGIC8: ImportFormat = ImportFormat {
    name: "logic8",
    description: "Raw binary (8 bit logic trace)",
    extensions: &[".logic8"],
    importer: || Box::new(flat_file::FlatFileImporter::logic8())
};

#[cfg(feature = "csv")]
pub const CSV: ImportFormat = ImportFormat {
    name: "csv",
    description: "Comma-separated values",
    extensions: &[".csv"],
    importer: || Box::new(csv::CsvImporter::csv()),
};

#[cfg(feature = "csv")]
pub const TSV: ImportFormat = ImportFormat {
    name: "tsv",
    description: "Tab-separated values",
    extensions: &[".tsv"],
    importer: || Box::new(csv::CsvImporter::tsv())
};

pub const IMPORTERS: ImportFormats<'static> = ImportFormats(&[
    VIRTUAL,
    #[cfg(feature = "izs")] IZS,
    BIN, LOGIC8,
    #[cfg(feature = "csv")] CSV,
    #[cfg(feature = "csv")] TSV,
]);
