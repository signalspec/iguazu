use append_array::AppendArray;
use std::{fmt::Debug, sync::Arc};
use crate::Idx;

mod cache;
pub use cache::Cache;

pub trait Stream: Send + Sync + Debug {
    fn desc(&self) -> StreamDesc;

    fn state(&self) -> StreamState;

    fn get_block(&self, block: u64) -> Option<Arc<AppendArray<u8>>>;
}

pub type ArcStream = Arc<dyn Stream>;

pub struct StreamDesc {
    pub element_size: usize,
    pub block_size: usize,
}

pub struct StreamState {
    pub end: Idx,
    pub streaming: bool,
}
