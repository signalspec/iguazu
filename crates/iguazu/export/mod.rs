use std::{pin::Pin, sync::Arc};

use async_executor::Executor;
use thiserror::Error;

use crate::{io::WritableFile, schema::EntityStream};

mod json_virtual;
mod flat_file;
#[cfg(feature = "csv")]
mod csv;
mod izs;

#[derive(Error, Debug)]
pub enum ExportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Unsupported schema: {0}")]
    UnsupportedSchema(String),

    #[error("Failed to read input: {0}")]
    Source(String),

    #[error("{0}")]
    UnsupportedStream(String),

    #[error("{0}")]
    InvalidFile(String),
}


pub struct ExportFormat {
    pub name: &'static str,
    pub description: &'static str,
    pub extension: &'static str,
    pub export: fn (executor: Arc<Executor<'static>>, EntityStream, Box<dyn WritableFile>) -> Pin<Box<dyn Future<Output=Result<(), ExportError>> + Send>>,
}

impl ExportFormat {
    pub fn export(&self, file: Box<dyn WritableFile>, executor: Arc<Executor<'static>>, entity: EntityStream) -> Pin<Box<dyn Future<Output=Result<(), ExportError>> + Send>> {
        (self.export)(executor, entity, file)
    }
}

pub const VIRTUAL: ExportFormat = ExportFormat {
    name: "virtual",
    description: "Iguazu Virtual JSON",
    extension: ".iguazu.json",
    export: json_virtual::export,
};

pub const IZS: ExportFormat = ExportFormat {
    name: "izs",
    description: "Iguazu Pack",
    extension: ".izs",
    export: izs::export,
};

// pub const BIN: ExportFormat = ExportFormat {
//     name: "bin",
//     description: "Raw binary",
//     extension: ".bin",
//     export: flat_file::binary,
// };

// #[cfg(feature = "csv")]
// pub const CSV: ExportFormat = ExportFormat {
//     name: "csv",
//     description: "Comma-separated values",
//     extension: ".csv",
//     export: csv::csv
// };

// #[cfg(feature = "csv")]
// pub const TSV: ExportFormat = ExportFormat {
//     name: "tsv",
//     description: "Tab-separated values",
//     extension: ".tsv",
//     export: csv::tsv
// };

pub const EXPORTERS: &[ExportFormat] = &[
    VIRTUAL,
    IZS,
    // BIN,
    // #[cfg(feature = "csv")] CSV,
    // #[cfg(feature = "csv")] TSV,
];
