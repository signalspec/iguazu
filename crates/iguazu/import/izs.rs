use std::{pin::Pin, sync::Arc};

use crate::{import::{ImportError, Importer}, io::ReadableFile, schema::{EntitySchema, EntityStream}, storage::Pool};

pub fn importer(file: Arc<dyn ReadableFile>) -> Box::<dyn Importer> {
    Box::new(IzsImporter { file })
}

struct IzsImporter {
    file: Arc<dyn ReadableFile>,
}

impl Importer for IzsImporter {
    fn load_schema(&mut self) -> Pin<Box<dyn Future<Output = Result<EntitySchema, ImportError>> + Send + '_>> {
        Box::pin(async move {
            let meta = crate::storage::izs::load_meta(self.file.clone()).await?;
            Ok(meta.entity.schema())
        })
    }

    fn import(self: Box<Self>, _schema: Option<EntitySchema>, pool: Arc<Pool>) -> Pin<Box<dyn Future<Output = Result<(EntityStream, Pin<Box<dyn Future<Output = Result<(), ImportError>> + Send>>), ImportError>> + Send>> {
        Box::pin(async move {
            let entity = crate::storage::izs::load(self.file, pool).await?;
            Ok((entity, Box::pin(async move {Ok(())}) as Pin<Box<_>>))
        })
    }
}
