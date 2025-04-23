mod in_memory;
pub use in_memory::MemoryStream;

mod flat_file;
pub use flat_file::{ FlatFileStream, FlatFileOpts };