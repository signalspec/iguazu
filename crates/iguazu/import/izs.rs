use std::{pin::Pin, sync::Arc};

use crate::{import::{ImportError, Importer}, io::ReadableFile, schema::{EntitySchema, EntityStream}, storage::Pool};

/// Importer for the native Iguazu `izs` format.
pub struct IzsImporter { }

impl IzsImporter {
    pub fn new() -> Self {
        Self {  }
    }
}

impl Importer for IzsImporter {
    fn load_schema(&self, file: Arc<dyn ReadableFile>) -> Pin<Box<dyn Future<Output = Result<EntitySchema, ImportError>> + Send + '_>> {
        Box::pin(async move {
            let meta = crate::storage::izs::load_meta(file).await?;
            Ok(meta.entity.schema())
        })
    }

    fn import(&self, file: Arc<dyn ReadableFile>, _schema: Option<EntitySchema>, pool: Arc<Pool>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send>> {
        Box::pin(async move {
            let entity = crate::storage::izs::load(file, pool).await?;
            Ok((entity, Box::pin(async move {Ok(())}) as Pin<Box<_>>))
        })
    }
}
