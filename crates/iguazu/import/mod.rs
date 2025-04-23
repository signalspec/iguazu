mod flat_file;
mod json_virtual;

use std::{pin::Pin, sync::Arc};

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
    fn load_schema(&mut self) -> Pin<Box<dyn Future<Output = Result<EntitySchema, ImportError>> + Send + Sync + '_>>;

    fn import(self: Box<Self>, schema: Option<EntitySchema>) -> Pin<Box<dyn Future<Output = Result<EntityStream, ImportError>> + Send + Sync>>;
}

pub struct ImportFormat {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub import: fn (Arc<dyn ReadableFile>) -> Box<dyn Importer>,
}

impl ImportFormat {
    pub fn matches_filename(&self, name: &str) -> bool {
        self.extensions.iter().any(|ext| name.ends_with(ext))
    }

    pub fn import(&self, f: Arc<dyn ReadableFile>) -> Box<dyn Importer> {
        (self.import)(f)
    }
}

pub struct ImportFormats<T>(pub T);

impl<T> ImportFormats<T> where T: AsRef<[ImportFormat]> {
    pub fn iter(&self) -> std::slice::Iter<ImportFormat> {
        self.0.as_ref().iter()
    }

    pub fn by_name(&self, name: &str) -> Option<&ImportFormat> {
        self.iter().find(|imp| imp.name == name)
    }

    pub fn first_for_filename(&self, fname: &str) -> Option<&ImportFormat> {
        self.iter().find(|imp| imp.matches_filename(fname))
    }
}

impl<'a, T> IntoIterator for &'a ImportFormats<T> where T: AsRef<[ImportFormat]> {
    type Item = &'a ImportFormat;
    type IntoIter = std::slice::Iter<'a, ImportFormat>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub const VIRTUAL: ImportFormat = ImportFormat {
    name: "Iguazu Virtual",
    extensions: &[".iguazu.json"],
    import: json_virtual::importer,
};

pub const BIN: ImportFormat = ImportFormat {
    name: "bin",
    extensions: &[".bin"],
    import: flat_file::binary,
};

pub const LOGIC8: ImportFormat = ImportFormat {
    name: "8ch logic trace - raw binary",
    extensions: &[".logic8"],
    import: flat_file::logic8,
};

pub const IMPORTERS: ImportFormats<&'static [ImportFormat]> = ImportFormats(&[
    VIRTUAL, BIN, LOGIC8
]);
