use std::{pin::Pin, sync::Arc};
use std::future;

use crate::{io::ReadableFile, schema::EntitySchema, storage::{FlatFileOpts, FlatFileStream}};

use super::Importer;


pub struct FlatFileImporter {
    file: Arc<dyn ReadableFile>,
    schema: EntitySchema,
    opts: FlatFileOpts,
}

impl FlatFileImporter {
    fn new(file: Arc<dyn ReadableFile>, schema: EntitySchema) -> Self {
        FlatFileImporter {
            file,
            schema,
            opts: FlatFileOpts::default(),
        }
    }
}

impl Importer for FlatFileImporter {
    fn load_schema(&mut self) -> Pin<Box<dyn Future<Output = Result<EntitySchema, super::ImportError>> + Send>> {
        Box::pin(future::ready(Ok(self.schema.clone())))
    }

    fn import(self: Box<Self>, schema: Option<EntitySchema>) -> Pin<Box<dyn Future<Output = Result<crate::schema::EntityStream, super::ImportError>> + Send>> {
        Box::pin(async {
            FlatFileStream::entity(self.file, schema.unwrap_or(self.schema), self.opts).await
        })
    }
}

pub fn binary(file: Arc<dyn ReadableFile>) -> Box<dyn Importer> {
    Box::new(FlatFileImporter::new(file, EntitySchema::bytes()))
}

pub fn logic8(file: Arc<dyn ReadableFile>) -> Box<dyn Importer> {
    Box::new(FlatFileImporter::new(file, EntitySchema::logic8()))
}


