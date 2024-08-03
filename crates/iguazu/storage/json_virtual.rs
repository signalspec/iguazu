use std::{io, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::{io::{ReadableFile, RelativePath}, schema::{Entity, EntityKind}, stream::ArcStream};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "storage", rename_all = "snake_case")]
enum StreamRef {
    FlatFile {
        file_name: RelativePath,
        element_size: usize,
    }
}

impl StreamRef {
    fn create(&self, src_file: &impl ReadableFile) -> Result<ArcStream, io::Error> {
        match *self {
            StreamRef::FlatFile { ref file_name, element_size } => {
                let file = src_file.relative(file_name)?;
                Ok(Arc::new(super::FlatFileStream::new(file, element_size)?))
            }
        }
    }
}

pub fn load(f: impl ReadableFile) -> Result<Entity, io::Error> {
    let data = f.read_at(0, 1<<20)?;
    let schema = serde_json::from_slice::<EntityKind<StreamRef>>(&data)?;
    schema.try_map_data(&mut |s| s.create(&f))
}
