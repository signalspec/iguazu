mod in_memory;

pub use in_memory::{ MemoryStorage, MemoryStream, MemoryStreamWriter };

mod flat_file;
pub use flat_file::{ FlatFileStream, FlatFileOpts };

use crate::{stream::StreamWriter, ElementType};

pub mod izs;

/// A storage backend that can create writable streams
pub trait Storage: Send {
    fn create_stream(&self, element_type: ElementType) -> Box<dyn StreamWriter>;
}
