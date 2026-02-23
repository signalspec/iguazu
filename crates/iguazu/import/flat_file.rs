use std::{pin::Pin, sync::Arc};
use std::future;
use crate::schema::EntityStream;
use crate::{io::ReadableFile, schema::EntitySchema, storage::{Pool, FlatFileOpts, FlatFileStream}};

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
}

impl Importer for FlatFileImporter {
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
