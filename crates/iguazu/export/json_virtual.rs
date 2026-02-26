use std::{any::Any, pin::Pin, sync::Arc};

use async_executor::Executor;
use futures_lite::AsyncWriteExt;
use url::Url;

use crate::{io::{RelativePath, WritableFile}, schema::{json_virtual::StreamRef, EntityStream}, stream::ArcStream};
use super::ExportError;

pub(crate) fn export(_executor: Arc<Executor<'static>>, entity: EntityStream, file: Box<dyn WritableFile>) -> Pin<Box<dyn Future<Output=Result<(), ExportError>> + Send>> {
    Box::pin(async move {
        let url = file.url()
            .ok_or_else(|| ExportError::InvalidFile(format!("Cannot get URL of output file")))?;
        let contents = entity.try_map_data(&mut |stream| stream_ref(stream, &url))?;
        let json = serde_json::to_string(&contents).unwrap();

        let mut writer = file.writer().await?;
        writer.write_all(json.as_bytes()).await?;
        writer.flush().await?;

        Ok(())
    })
}

fn stream_ref(stream: &ArcStream, base: &Url) -> Result<StreamRef, ExportError> {
    if let Some(stream) = (&**stream as &dyn Any).downcast_ref::<crate::storage::FlatFileStream>() {
        let url = stream.url()
            .ok_or_else(|| ExportError::UnsupportedStream(format!("Stream has no URL")))?;
        let relative = RelativePath::from_base(base, &url)
            .ok_or_else(|| ExportError::UnsupportedStream(format!("Stream from `{url}` is not relative to `{base}`")))?;
        Ok(StreamRef::FlatFile {
            file_name: relative,
            element_size: stream.element_size(),
            offset: stream.offset(),
            count: stream.count(),
        })
    } else {
        Err(ExportError::UnsupportedStream(format!("Stream `{:?}` is not supported in Virtual export", stream)))
    }
}
