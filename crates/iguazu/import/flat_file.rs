use std::{pin::Pin, sync::Arc};
use std::future;
use crate::import::OptionDescription;
use crate::schema::EntityStream;
use crate::{io::ReadableFile, schema::{EntitySchema, attribute}, storage::{Pool, FlatFileOpts, FlatFileStream}};

use super::{ImportError, Importer};

/// Importer for flat files that are directly read from disk.
pub struct FlatFileImporter {
    schema: EntitySchema,
    opts: FlatFileOpts,
}

impl FlatFileImporter {
    pub fn new(schema: EntitySchema) -> Self {
        Self {
            schema,
            opts: FlatFileOpts::default(),
        }
    }

    pub fn binary() -> Self {
        Self::new(EntitySchema::bytes())
    }

    pub fn logic8() -> Self {
        Self::new(EntitySchema::logic8())
    }

    pub fn offset(&self) -> u64 {
        self.opts.offset
    }

    pub fn with_offset(mut self, offset: u64) -> Self {
        self.opts.offset = offset;
        self
    }

    pub fn count(&self) -> Option<u64> {
        self.opts.count
    }

    pub fn with_count(mut self, count: Option<u64>) -> Self {
        self.opts.count = count;
        self
    }

    pub fn block_size(&self) -> usize {
        self.opts.block_size
    }

    pub fn with_block_size(mut self, block_size: usize) -> Self {
        self.opts.block_size = block_size;
        self
    }
}

impl Importer for FlatFileImporter {
    fn options(&self) -> &'static [ super::OptionDescription ] {
        &[
            OptionDescription {
                name: "offset",
                description: "Byte offset in the file where the data starts.",
            },
            OptionDescription {
                name: "count",
                description: "Number of elements to read from the file. Empty means to read until the end of the file.",
            },
            OptionDescription {
                name: "block_size",
                description: "Number of elements to read in each block.",
            },
            OptionDescription {
                name: "sample_rate",
                description: "Sample rate",
            },
        ]
    }

    fn set(&mut self, option: &str, value: &str) -> Result<(), String> {
        match option {
            "offset" => self.opts.offset = value.parse().map_err(|_| "Invalid integer")?,
            "count" => self.opts.count = if value.is_empty() { None } else { Some(value.parse().map_err(|_| "Invalid integer")?) },
            "block_size" => self.opts.block_size = value.parse().map_err(|_| "Invalid integer")?,
            "sample_rate" => {
                if value.is_empty() {
                    self.schema.remove_attribute(attribute::core::SAMPLE_RATE);
                } else {
                    let val = value.parse().map_err(|_| "Invalid float")?;
                    self.schema.set_attribute(attribute::core::SAMPLE_RATE, val);
                }
            }
            _ => return Err("Unknown option".into()),
        }
        Ok(())
    }

    fn get(&self, option: &str) -> Option<String> {
        Some(match option {
            "offset" => self.opts.offset.to_string(),
            "count" => self.opts.count.map_or_else(|| "".into(), |c| c.to_string()),
            "block_size" => self.opts.block_size.to_string(),
            "sample_rate" => self.schema.attribute(attribute::core::SAMPLE_RATE)?.to_string(),
            _ => return None,
        })
    }

    fn load_schema(&self, _file: Arc<dyn ReadableFile>) -> Pin<Box<dyn Future<Output = Result<EntitySchema, ImportError>> + Send>> {
        Box::pin(future::ready(Ok(self.schema.clone())))
    }

    fn import(&self, file: Arc<dyn ReadableFile>, schema: Option<EntitySchema>, pool: Arc<Pool>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send + '_>> {
        Box::pin(async move {
            let schema = schema.unwrap_or_else(|| self.schema.clone());
            let entity = FlatFileStream::entity(file, pool, schema, &self.opts).await?;
            Ok((entity, Box::pin(async move {Ok(())}) as Pin<Box<_>>))
        })
    }
}
