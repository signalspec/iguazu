use std::sync::Arc;

use crate::{export::ExportError, io::WritableFile, schema::EntityStream, send::{BoxFuture, Executor}};

pub(crate) fn export(executor: Arc<Executor<'static>>, entity: EntityStream, file: Box<dyn WritableFile>) -> BoxFuture<'static, Result<(), ExportError>> {
    Box::pin(async move {
        let writer = file.writer().await?;
        crate::izs::export(executor, entity, writer).await
    })
}
