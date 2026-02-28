use std::{pin::Pin, sync::Arc};

use crate::{io::ReadableFile, schema::{Entity, EntitySchema, EntityStream, json_virtual::StreamRef}, storage::{ FlatFileOpts, FlatFileStream, Pool }, stream::ArcStream};

use super::{ImportError, Importer};

/// Importer for a JSON file that references other files on disk.
pub struct VirtualImporter {}

impl VirtualImporter {
    pub fn new() -> Self {
        Self { }
    }
}

impl Importer for VirtualImporter {
    fn load_schema(&self, file: Arc<dyn ReadableFile>) -> Pin<Box<dyn Future<Output = Result<EntitySchema, ImportError>> + Send + '_>> {
        Box::pin(async move {
            load(file).await.map(|schema| schema.schema())
        })
    }

    fn import(&self, file: Arc<dyn ReadableFile>, _schema: Option<EntitySchema>, pool: Arc<Pool>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send>> {
        Box::pin(async move {
            let schema = load(file.clone()).await?;
            let entity = schema.try_map_data_async(move |s| pool.executor.spawn(create_stream(file.clone(), pool.clone(), s.clone()))).await?;
            Ok((entity, Box::pin(async move {Ok(())}) as Pin<Box<_>>))
        })
    }
}

async fn load(file: Arc<dyn ReadableFile>) -> Result<Entity<StreamRef>, ImportError> {
    let data = file.read_all(1024 * 1024 * 16).await?;
    serde_json::from_slice::<Entity<StreamRef>>(&data).map_err(|e| ImportError::InvalidFile(e.to_string()))
}

async fn create_stream(src_file: Arc<dyn ReadableFile>, pool: Arc<Pool>, stream: StreamRef) -> Result<ArcStream, ImportError> {
    match stream {
        StreamRef::FlatFile { ref file_name, element_size, offset, count } => {
            let file = src_file.relative(file_name).await?;
            let mut opts = FlatFileOpts::default();
            opts.offset = offset;
            opts.count = count;
            Ok(Arc::new(FlatFileStream::new(file, pool, element_size, &opts).await?))
        }
    }
}
