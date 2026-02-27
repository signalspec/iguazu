use std::{pin::Pin, sync::Arc};

use async_executor::Executor;
use crate::{export::ExportError, io::WritableFile, schema::EntityStream};

pub(crate) fn export(executor: Arc<Executor<'static>>, entity: EntityStream, file: Box<dyn WritableFile>) -> Pin<Box<dyn Future<Output=Result<(), ExportError>> + Send>> {
    Box::pin(async move {
        let writer = file.writer().await?;
        crate::izs::export(executor, entity, writer).await
    })
}
