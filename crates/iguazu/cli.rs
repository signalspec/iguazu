use std::{path::PathBuf, pin::Pin, sync::Arc};

use async_executor::Executor;
use clap::Args;

use crate::{import::{ImportError, ImportFormats, Importer}, io::{FsFile, ReadableFile}, schema::{EntitySchema, EntityStream}};

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

        let file = FsFile::new(self.filename.clone())
            .await
            .map_err(|e| format!("Failed to open file {}: {}", self.filename.display(), e))?;

        let importer = format.import(Arc::new(file));
        Ok(importer)
    }

    pub async fn schema(&self) -> Result<Option<EntitySchema>, String> {
        if let Some(ref schema_path) = self.schema {
            let schema_file = FsFile::new(schema_path.clone())
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