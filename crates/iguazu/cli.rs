use std::{path::PathBuf, pin::Pin, sync::Arc};

use async_executor::Executor;
use clap::Args;

use crate::{export::ExportFormat, import::{ImportError, ImportFormat, ImportFormats, Importer}, io::{FsFile, FsWritableFile, ReadableFile, StdinFile}, schema::{EntitySchema, EntityStream, Path}, storage::Pool};

#[derive(Args, Clone, Debug)]
#[group(requires = "filename")] // Allow `ImportOpts` to be optional (https://github.com/clap-rs/clap/issues/5092#issuecomment-1703980717)
pub struct ImportOpts {
    /// Input filename
    #[arg(required = false)]
    pub filename: PathBuf,

    /// Input format and `:` separated options (if not specified, inferred from filename)
    #[clap(short = 'f', long, value_name = "FORMAT[:OPTION=VALUE:OPTION=VALUE...]")]
    pub format: Option<String>,

    /// Schema override from file
    #[clap(short = 's', long)]
    pub schema: Option<PathBuf>,

    /// Select an entity by path within the file
    #[clap(short = 'e', long)]
    pub entity: Option<String>,
}

impl ImportOpts {
    fn format_name(&self) -> Option<&str> {
        self.format.as_deref().and_then(|fmt| {
            fmt.split(':').next()
        })
    }

    fn format_options(&self) -> impl Iterator<Item = (&str, &str)> {
        self.format.as_deref()
            .unwrap_or("")
            .split(':')
            .skip(1)
            .map(|opt| opt.split_once('=').unwrap_or((opt, "")))
    }

    /// Find the format to use.
    ///
    /// This is either the importer explicitly specified by `--format`,
    /// or the first importer that matches the filename.
    pub fn format<'a>(&self, importers: ImportFormats<'a>) -> Result<&'a ImportFormat, String> {
        if let Some(ref fmt) = self.format_name() {
            importers.by_name(fmt).ok_or_else(|| {
                format!("No import format `{}`", fmt)
            })
        } else {
            importers.first_for_filename(self.filename.to_str().unwrap()).ok_or_else(|| {
                format!("No import format matched filename `{}`", self.filename.display())
            })
        }
    }

    pub fn importer(&self, importers: ImportFormats<'_>) -> Result<Box<dyn Importer>, String> {
        let mut importer = self.format(importers)?.importer();
        let mut errors = Vec::new();

        for (opt, val) in self.format_options() {
            match importer.set(opt, val) {
                Ok(()) => (),
                Err(e) => errors.push((opt, e)),
            }
        }

        if !errors.is_empty() {
            let mut msg = "Invalid import options".to_string();
            for (opt, err) in errors {
                msg.push_str(&format!("\n  - {}: {}", opt, err));
            }
            return Err(msg);
        }

        Ok(importer)
    }

    /// Get the source file.
    ///
    /// This is either the file specified by `filename`, or stdin if `filename` is `-`.
    pub async fn file(&self) -> Result<Arc<dyn ReadableFile>, String> {
        Ok(if self.filename == Path::from("-") {
            Arc::new(StdinFile::new()) as Arc<dyn ReadableFile>
        } else {
            Arc::new(FsFile::open(self.filename.clone())
                .await
                .map_err(|e| format!("Failed to open file {}: {}", self.filename.display(), e))?
            ) as Arc<dyn ReadableFile>
        })
    }

    /// Load the schema from the schema file specified on the command line, if any.
    pub async fn specified_schema(&self) -> Result<Option<EntitySchema>, String> {
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

    /// Load the schema from the specified file, or infer it from the data if no schema file is specified.
    pub async fn schema_or_inferred(&self, importers: ImportFormats<'_>) -> Result<EntitySchema, String> {
        let importer = self.importer(importers)?;
        let file = self.file().await?;

        let mut schema = if let Some(schema) = self.specified_schema().await? {
            schema
        } else {
            importer.load_schema(file).await.map_err(|e| e.to_string())?
        };

        if let Some(ref entity_path) = self.entity {
            schema = schema.select_owned(entity_path)
                .ok_or_else(|| format!("Entity not found: `{}`", entity_path))?;
        }

        Ok(schema)
    }

    /// Run the import.
    ///
    /// Returns the imported entity, as well as a future that completes when the import is fully done.
    pub async fn import(&self, importers: ImportFormats<'_>, pool: Arc<Pool>) -> Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), String> {
        let importer = self.importer(importers)?;
        let file = self.file().await?;
        let schema = self.specified_schema().await?;

        let (mut entity, completion) = importer.import(file, schema, pool).await
            .map_err(|e| format!("Failed to import {}: {}", self.filename.display(), e))?;

        if let Some(ref entity_path) = self.entity {
            entity = entity.select_owned(entity_path)
                .ok_or_else(|| format!("Entity not found: `{}`", entity_path))?;
        }

        Ok((entity, completion))
    }
}

#[derive(Args, Clone, Debug)]
#[group(requires = "out_filename")]
pub struct ExportOpts {
    #[arg(required = false)]
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
