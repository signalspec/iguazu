use std::{pin::Pin, sync::Arc};

use async_executor::{ Executor, Task };
use futures_lite::{stream, StreamExt};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use num_traits::Zero;

use crate::{io::{ReadableFile, RelativePath}, schema::{Entity, EntityKind, EntitySchema, EntityStream}, storage::{ FlatFileOpts, FlatFileStream }, stream::{ArcStream, ElementType}};

use super::{ImportError, Importer};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "storage", rename_all = "snake_case")]
pub enum StreamRef {
    FlatFile {
        file_name: RelativePath,
        element_type: ElementType,

        #[serde(default = "u64::zero", skip_serializing_if = "u64::is_zero")]
        offset: u64,
    }
}

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

            let entity = load(file, executor, schema).await?;
            Ok((entity, Box::pin(async move {Ok(())}) as Pin<Box<_>>))
        })
    }
}

pub fn importer(file: Arc<dyn ReadableFile>) -> Box::<dyn Importer> {
    Box::new(VirtualImporter { file, schema: None })
}

pub async fn load(file: Arc<dyn ReadableFile>, io_executor: Arc<Executor<'static>>, schema: Entity<StreamRef>) -> Result<EntityStream, ImportError> {
    let executor = Arc::new(Executor::new());

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

    async fn create_children(ex: Arc<Executor<'static>>, io_executor: Arc<Executor<'static>>,src_file: Arc<dyn ReadableFile>, children: IndexMap<String, Entity<StreamRef>>) -> Result<IndexMap<String, EntityStream>, ImportError> {
        let child_tasks: Vec<(String, Task<_>)> = children.into_iter().map(|(k, v)| {
            (k, ex.spawn(create(ex.clone(), io_executor.clone(), src_file.clone(), v)))
        }).collect();

        stream::iter(child_tasks)
            .then(|(k, f_v)| async move { Ok::<_, ImportError>((k, f_v.await?)) })
            .try_collect().await
    }

    fn create(ex: Arc<Executor<'static>>, io_executor: Arc<Executor<'static>>, src_file: Arc<dyn ReadableFile>, schema: Entity<StreamRef>) -> impl Future<Output = Result<EntityStream, ImportError>> + Send {
        async move {
            let attributes = schema.attributes;

            match schema.kind {
                EntityKind::Group { children } => {
                    let children = create_children(ex, io_executor, src_file, children).await?;
                    Ok(EntityStream { kind: EntityKind::Group { children }, attributes })
                }
                EntityKind::Record { children } => {
                    let children = create_children(ex, io_executor, src_file, children).await?;
                    Ok(EntityStream { kind: EntityKind::Record { children }, attributes })
                }
                EntityKind::Bits { data, bits } => {
                    let data = create_stream(src_file, io_executor, data).await?;
                    Ok(EntityStream { kind: EntityKind::Bits { bits, data }, attributes })
                }
                EntityKind::Logic { data, bits } => {
                    let data = create_stream(src_file, io_executor, data).await?;
                    Ok(EntityStream { kind: EntityKind::Logic { bits, data }, attributes })
                }
                EntityKind::Character { data } => {
                    let data = create_stream(src_file, io_executor, data).await?;
                    Ok(EntityStream { kind: EntityKind::Character { data }, attributes })
                }
                EntityKind::Timestamp { data, sample_rate } => {
                    let data = create_stream(src_file, io_executor, data).await?;
                    Ok(EntityStream { kind: EntityKind::Timestamp { sample_rate, data }, attributes })
                }
                EntityKind::Number { data } => {
                    let data = create_stream(src_file, io_executor, data).await?;
                    Ok(EntityStream { kind: EntityKind::Number { data }, attributes })
                }
                EntityKind::Enum { data, values } => {
                    let data = create_stream(src_file, io_executor, data).await?;
                    Ok(EntityStream { kind: EntityKind::Enum { data, values }, attributes })
                }
                EntityKind::FixedArray { elements, child } => {
                    let child = Box::new(ex.spawn(create(ex.clone(), io_executor, src_file, *child)).await?);
                    Ok(EntityStream { kind: EntityKind::FixedArray { elements, child }, attributes })
                }
                EntityKind::Tuple { fields, child } => {
                    let child = Box::new(ex.spawn(create(ex.clone(), io_executor, src_file, *child)).await?);
                    Ok(EntityStream { kind: EntityKind::Tuple { fields, child }, attributes })
                }
                EntityKind::VariableArray { data, child } => {
                    let data = create_stream(src_file.clone(), io_executor.clone(), data).await?;
                    let child = Box::new(ex.spawn(create(ex.clone(), io_executor, src_file, *child)).await?);
                    Ok(EntityStream { kind: EntityKind::VariableArray { data, child }, attributes })
                }
            }
        }
    }

    executor.run(create(executor.clone(), io_executor, file, schema)).await
}
