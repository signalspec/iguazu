use std::{pin::Pin, sync::Arc};
use std::future;

use crate::schema::EntityStream;
use crate::{io::ReadableFile, schema::EntitySchema, storage::{FlatFileOpts, FlatFileStream}};

use super::{ImportError, Importer};


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
    fn load_schema(&mut self) -> Pin<Box<dyn Future<Output = Result<EntitySchema, ImportError>> + Send>> {
        Box::pin(future::ready(Ok(self.schema.clone())))
    }

    fn import(self: Box<Self>, schema: Option<EntitySchema>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send>> {
        Box::pin(async {
            let entity = FlatFileStream::entity(self.file, schema.unwrap_or(self.schema), self.opts).await?;
            Ok((entity, Box::pin(async move {Ok(())}) as Pin<Box<_>>))
        })
    }
}

pub fn binary(file: Arc<dyn ReadableFile>) -> Box<dyn Importer> {
    Box::new(FlatFileImporter::new(file, EntitySchema::bytes()))
}

pub fn logic8(file: Arc<dyn ReadableFile>) -> Box<dyn Importer> {
    Box::new(FlatFileImporter::new(file, EntitySchema::logic8()))
}


