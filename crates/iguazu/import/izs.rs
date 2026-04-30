use std::{pin::Pin, sync::Arc};

use crate::{import::{ImportError, Importer}, io::ReadableFile, schema::{EntitySchema, EntityStream}, storage::{Pool}, izs::IzsFile};

/// Importer for the native Iguazu `izs` format.
pub struct IzsImporter {
    file: Arc<dyn ReadableFile>,
}

impl IzsImporter {
    pub fn new(file: Arc<dyn ReadableFile>) -> Self {
        Self { file }
    }
}

impl Importer for IzsImporter {
    fn load_schema(&self) -> Pin<Box<dyn Future<Output = Result<EntitySchema, ImportError>> + Send + '_>> {
        Box::pin(async move {
            IzsFile::new(self.file.clone()).await?.load_schema().await
        })
    }

    fn import(&self, _schema: Option<EntitySchema>, pool: Arc<Pool>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send>> {
        let file = self.file.clone();
        Box::pin(async move {
            let entity = Arc::new(IzsFile::new(file).await?).load_entity(pool).await?;
            Ok((entity, Box::pin(async move {Ok(())}) as Pin<Box<_>>))
        })
    }
}
