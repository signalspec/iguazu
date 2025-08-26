use std::{path::PathBuf, pin::Pin, sync::Arc};

use async_executor::Executor;
use clap::Args;

use crate::{export::ExportFormat, import::{ImportError, ImportFormats, Importer}, io::{FsFile, FsWritableFile, ReadableFile}, schema::{EntitySchema, EntityStream}};

#[derive(Args, Clone, Debug)]
pub struct ImportOpts {
    pub filename: PathBuf,

    #[clap(short = 'f', long)]
    pub format: Option<String>,

    #[clap(short = 's', long)]
    pub schema: Option<PathBuf>,
}

impl ImportOpts {
    pub async fn importer(&self, importers: ImportFormats<'_>) -> Result<Box<dyn Importer>, String> {
        let format = if let Some(ref fmt) = self.format {
            importers.by_name(fmt).ok_or_else(|| {
                format!("No import format named `{}`", fmt)
            })?
        } else {
            importers.first_for_filename(self.filename.to_str().unwrap()).ok_or_else(|| {
                format!("No import format matched filename `{}`", self.filename.display())
            })?
        };

        let file = FsFile::open(self.filename.clone())
            .await
            .map_err(|e| format!("Failed to open file {}: {}", self.filename.display(), e))?;

        let importer = format.import(Arc::new(file));
        Ok(importer)
    }

    pub async fn schema(&self) -> Result<Option<EntitySchema>, String> {
        if let Some(ref schema_path) = self.schema {
            let schema_file = FsFile::open(schema_path.clone())
                .await
                .map_err(|e| format!("Failed to open schema file {}: {}", schema_path.display(), e))?;

            let data = Arc::new(schema_file).read_all(1024 * 1024 * 16).await
                .map_err(|e| format!("Failed to read schema file {}: {}", schema_path.display(), e))?;
            
            let schema = serde_json::from_slice::<EntitySchema>(&data)
                .map_err(|e| format!("Failed to parse schema file {}: {}", schema_path.display(), e))?;

            Ok(Some(schema))
        } else {
            Ok(None)
        }
    }

    pub async fn import(&self, importers: ImportFormats<'_>, executor: Arc<Executor<'static>>) -> Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), String> {
        let importer = self.importer(importers).await?;
        let schema = self.schema().await?;
        importer.import(schema, executor).await.map_err(|e| format!("Failed to import {}: {}", self.filename.display(), e))
    }
}

#[derive(Args, Clone, Debug)]
pub struct ExportOpts {
    pub out_filename: PathBuf,

    #[clap(short = 'F', long)]
    pub out_format: Option<String>,
}

impl ExportOpts {
    pub fn exporter<'a>(&self, exporters: &'a [ExportFormat]) -> Result<&'a ExportFormat, String> {
        if let Some(ref fmt) = self.out_format {
            exporters.iter().find(|exp| exp.name.eq_ignore_ascii_case(fmt)).ok_or_else(|| {
                format!("No export format named `{}`", fmt)
            })
        } else {
            exporters.iter().find(|exp| self.out_filename.to_str().unwrap().ends_with(exp.extension)).ok_or_else(|| {
                format!("No export format matched filename `{}`", self.out_filename.display())
            })
        }
    }

    pub async fn export(&self, exporters: &[ExportFormat], executor: Arc<Executor<'static>>, entity: EntityStream) -> Result<(), String> {
        let exporter = self.exporter(exporters)?;
        let file = Box::new(FsWritableFile::new(&self.out_filename)
            .map_err(|e| format!("Failed to resolve output file {}: {}", self.out_filename.display(), e))?);
        exporter.export(file, executor, entity).await.map_err(|e| format!("Failed to export {}: {}", self.out_filename.display(), e))
    }
}
