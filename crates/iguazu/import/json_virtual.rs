use std::{io, pin::Pin, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::{io::{ReadableFile, RelativePath}, schema::{Entity, EntitySchema, EntityStream}, storage::{ FlatFileOpts, FlatFileStream, MemoryStream }, stream::ArcStream};

use super::{ImportError, Importer};


#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "storage", rename_all = "snake_case")]
enum StreamRef {
    FlatFile {
        file_name: RelativePath,
        element_size: usize,
    }
}

impl StreamRef {
    fn create(&self, src_file: &Arc<dyn ReadableFile>) -> Result<ArcStream, ImportError> {
        match *self {
            StreamRef::FlatFile { ref file_name, element_size } => {
                let file = src_file.relative(file_name)?;
                let mut opts = FlatFileOpts::default();
                opts.element_size = Some(element_size);
                Ok(Arc::new(FlatFileStream::new(file, opts)?))
            }
        }
    }
}

pub struct VirtualImporter {
    file: Arc<dyn ReadableFile>,
    schema: Option<Entity<Option<StreamRef>>>,
}

impl VirtualImporter {
    fn load(&mut self) -> Result<&mut Entity<Option<StreamRef>>, ImportError> {
        if let Some(ref mut schema) = self.schema {
            return Ok(schema);
        }

        let data = self.file.read_at(0, 1<<20).map_err(ImportError::Io)?;
        let schema = serde_json::from_slice::<Entity<Option<StreamRef>>>(&data).map_err(|e| ImportError::InvalidFile(e.to_string()))?;
        Ok(self.schema.insert(schema))
    }
}

impl Importer for VirtualImporter {
    fn load_schema(&mut self) -> Pin<Box<dyn Future<Output = Result<EntitySchema, super::ImportError>> + Send + Sync + '_>> {
        Box::pin(async move {
            self.load().map(|schema| schema.schema())
        })
    }

    fn import(mut self: Box<Self>, schema: Option<EntitySchema>) -> Pin<Box<dyn Future<Output = Result<EntityStream, super::ImportError>> + Send + Sync>> {
        Box::pin(async move {
            self.load()?;
            let file = self.file;
            let schema = self.schema.unwrap();
            
            schema.try_map_data(&mut |s| {
                match s {
                    Some(s) => s.create(&file),
                    None => Ok(MemoryStream::new(1, &[]))
                }
            })
        })
    }
}

pub fn importer(file: Arc<dyn ReadableFile>) -> Box::<dyn Importer> {
    Box::new(VirtualImporter { file, schema: None })
}
