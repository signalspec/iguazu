mod flat_file;
mod json_virtual;
#[cfg(feature = "csv")]
mod csv;

use std::{pin::Pin, sync::Arc};

use async_executor::Executor;
use thiserror::Error;

use crate::{io::ReadableFile, schema::{EntitySchema, EntityStream}};

#[derive(Error, Debug)]
pub enum ImportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Schema mismatch: {0}")]
    SchemaMismatch(String),

    #[error("File is malformed: {0}")]
    InvalidFile(String),
}

pub trait Importer {
    fn load_schema(&mut self) -> Pin<Box<dyn Future<Output = Result<EntitySchema, ImportError>> + Send + '_>>;

    fn import(self: Box<Self>, schema: Option<EntitySchema>, executor: Arc<Executor<'static>>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send>>;
}

pub struct ImportFormat {
    pub name: &'static str,
    pub description: &'static str,
    pub extensions: &'static [&'static str],
    pub import: fn (Arc<dyn ReadableFile>) -> Box<dyn Importer>,
}

impl ImportFormat {
    pub fn matches_filename(&self, name: &str) -> bool {
        self.extensions.iter().any(|ext| name.ends_with(ext))
    }

    pub fn import(&self, file: Arc<dyn ReadableFile>) -> Box<dyn Importer> {
        (self.import)(file)
    }
}

pub struct ImportFormats<'a>(&'a [ImportFormat]);

impl ImportFormats<'_> {
    pub fn iter(&self) -> std::slice::Iter<ImportFormat> {
        self.0.as_ref().iter()
    }

    pub fn by_name(&self, name: &str) -> Option<&ImportFormat> {
        self.iter().find(|imp| imp.name.eq_ignore_ascii_case(name))
    }

    pub fn first_for_filename(&self, fname: &str) -> Option<&ImportFormat> {
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
    import: json_virtual::importer,
};

pub const BIN: ImportFormat = ImportFormat {
    name: "bin",
    description: "Raw binary",
    extensions: &[".bin"],
    import: flat_file::binary,
};

pub const LOGIC8: ImportFormat = ImportFormat {
    name: "logic8",
    description: "Raw binary (8 bit logic trace)",
    extensions: &[".logic8"],
    import: flat_file::logic8,
};

#[cfg(feature = "csv")]
pub const CSV: ImportFormat = ImportFormat {
    name: "csv",
    description: "Comma-separated values",
    extensions: &[".csv"],
    import: csv::csv
};

#[cfg(feature = "csv")]
pub const TSV: ImportFormat = ImportFormat {
    name: "tsv",
    description: "Tab-separated values",
    extensions: &[".tsv"],
    import: csv::tsv
};

pub const IMPORTERS: ImportFormats<'static> = ImportFormats(&[
    VIRTUAL,
    BIN, LOGIC8,
    #[cfg(feature = "csv")] CSV,
    #[cfg(feature = "csv")] TSV,
]);
