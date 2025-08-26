use std::{pin::Pin, sync::Arc};

use async_executor:: Executor;

use crate::{io::{ReadableFile}, schema::{json_virtual::StreamRef, Entity, EntitySchema, EntityStream}, storage::{ FlatFileOpts, FlatFileStream }, stream::ArcStream};

use super::{ImportError, Importer};

pub struct VirtualImporter {
    file: Arc<dyn ReadableFile>,
    schema: Option<Entity<StreamRef>>,
}

impl VirtualImporter {
    async fn load(&mut self) -> Result<&mut Entity<StreamRef>, ImportError> {
        if let Some(ref mut schema) = self.schema {
            return Ok(schema);
        }

        let data = self.file.clone().read_all(1024 * 1024 * 16).await?;
        let schema = serde_json::from_slice::<Entity<StreamRef>>(&data).map_err(|e| ImportError::InvalidFile(e.to_string()))?;
        Ok(self.schema.insert(schema))
    }
}

impl Importer for VirtualImporter {
    fn load_schema(&mut self) -> Pin<Box<dyn Future<Output = Result<EntitySchema, ImportError>> + Send + '_>> {
        Box::pin(async move {
            self.load().await.map(|schema| schema.schema())
        })
    }

    fn import(mut self: Box<Self>, _schema: Option<EntitySchema>, executor: Arc<Executor<'static>>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send>> {
        Box::pin(async move {
            self.load().await?;
            let file = self.file;
            let schema = self.schema.unwrap();

            let entity = schema.try_map_data_async(move |s| create_stream(file.clone(), executor.clone(), s)).await?;
            Ok((entity, Box::pin(async move {Ok(())}) as Pin<Box<_>>))
        })
    }
}

pub fn importer(file: Arc<dyn ReadableFile>) -> Box::<dyn Importer> {
    Box::new(VirtualImporter { file, schema: None })
}

async fn create_stream(src_file: Arc<dyn ReadableFile>, io_executor: Arc<Executor<'static>>, stream: StreamRef) -> Result<ArcStream, ImportError> {
    match stream {
        StreamRef::FlatFile { ref file_name, element_type, offset } => {
            let file = src_file.relative(file_name).await?;
            let mut opts = FlatFileOpts::default();
            opts.element_type = element_type;
            opts.offset = offset;
            Ok(Arc::new(FlatFileStream::new(file, io_executor, opts).await?))
        }
    }
}
