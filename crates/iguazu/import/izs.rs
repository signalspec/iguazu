use std::{pin::Pin, sync::Arc};
use async_executor::Task;

use crate::{config::Configurable, import::{ImportError, Importer}, io::ReadableFile, schema::{EntitySchema, EntityStream}, storage::{Pool}, izs::IzsFile};

/// Importer for the native Iguazu `izs` format.
pub struct IzsImporter {
    file: Arc<dyn ReadableFile>,
}

impl IzsImporter {
    pub fn new(file: Arc<dyn ReadableFile>) -> Self {
        Self { file }
    }
}

impl Configurable for IzsImporter {}

impl Importer for IzsImporter {
    fn load_schema(&self) -> Pin<Box<dyn Future<Output = Result<EntitySchema, ImportError>> + Send + '_>> {
        Box::pin(async move {
            IzsFile::new(self.file.clone()).await?.load_schema().await
        })
    }

    fn import(&self, _schema: Option<EntitySchema>, pool: Arc<Pool>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Task<Result<(), ImportError>>), ImportError>> + Send>> {
        let file = self.file.clone();
        Box::pin(async move {
            let task = pool.executor.spawn(async move { Ok(()) });
            let entity = Arc::new(IzsFile::new(file).await?).load_entity(pool).await?;
            Ok((entity, task))
        })
    }

    fn file(&self) -> &Arc<dyn ReadableFile> {
        &self.file
    }
}
