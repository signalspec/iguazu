use std::{pin::Pin, sync::Arc};

use async_executor::{ Executor, Task };
use futures_lite::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use num_traits::Zero;

use crate::{io::{ReadableFile, RelativePath}, schema::{Entity, EntitySchema, EntityStream}, storage::{ FlatFileOpts, FlatFileStream, MemoryStream }, stream::{ArcStream, ElementSize}};

use super::{ImportError, Importer};


#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "storage", rename_all = "snake_case")]
enum StreamRef {
    FlatFile {
        file_name: RelativePath,
        element_size: ElementSize,

        #[serde(default = "u64::zero", skip_serializing_if = "u64::is_zero")]
        offset: u64,
    }
}

pub struct VirtualImporter {
    file: Arc<dyn ReadableFile>,
    schema: Option<Entity<Option<StreamRef>>>,
}

impl VirtualImporter {
    async fn load(&mut self) -> Result<&mut Entity<Option<StreamRef>>, ImportError> {
        if let Some(ref mut schema) = self.schema {
            return Ok(schema);
        }

        let data = self.file.clone().read_at(0, 1<<20).await.map_err(ImportError::Io)?;
        let schema = serde_json::from_slice::<Entity<Option<StreamRef>>>(&data).map_err(|e| ImportError::InvalidFile(e.to_string()))?;
        Ok(self.schema.insert(schema))
    }
}

impl Importer for VirtualImporter {
    fn load_schema(&mut self) -> Pin<Box<dyn Future<Output = Result<EntitySchema, ImportError>> + Send + '_>> {
        Box::pin(async move {
            self.load().await.map(|schema| schema.schema())
        })
    }

    fn import(mut self: Box<Self>, schema: Option<EntitySchema>) -> Pin<Box<dyn Future<Output = Result<EntityStream, ImportError>> + Send>> {
        Box::pin(async move {
            self.load().await?;
            let file = self.file;
            let schema = self.schema.unwrap();

            let executor = Arc::new(Executor::new());

            fn create(ex: Arc<Executor<'static>>, src_file: Arc<dyn ReadableFile>, schema: Entity<Option<StreamRef>>) -> impl Future<Output = Result<EntityStream, ImportError>> + Send {
                async move {
                    let data: ArcStream = match schema.data {
                        Some(StreamRef::FlatFile { ref file_name, element_size, offset }) => {
                            let file = src_file.relative(file_name).await?;
                            let mut opts = FlatFileOpts::default();
                            opts.element_size = element_size;
                            opts.offset = offset;
                            Arc::new(FlatFileStream::new(file, opts).await?)
                        }
                        None => MemoryStream::new(ElementSize::Null, &[])
                    };

                    let child_tasks: Vec<(String, Task<_>)> = schema.children.into_iter().map(|(k, v)| {
                        (k, ex.spawn(create(ex.clone(), src_file.clone(), v)))
                    }).collect();

                    let children = stream::iter(child_tasks)
                        .then(|(k, f_v)| async move { Ok::<_, ImportError>((k, f_v.await?)) })
                        .try_collect().await?;

                    let kind = schema.kind;
                    let attributes = schema.attributes;

                    Ok(Entity { data, kind, attributes, children })
                }
            }

            executor.run(create(executor.clone(), file, schema)).await
        })
    }
}

pub fn importer(file: Arc<dyn ReadableFile>) -> Box::<dyn Importer> {
    Box::new(VirtualImporter { file, schema: None })
}
