use append_array::AppendArray;
use std::{fmt::Debug, sync::Arc, hash::Hash};
use crate::Idx;

pub mod cache;

pub trait Stream<T>: Send + Sync + Debug {
    fn block_size(&self) -> usize;

    fn state(&self) -> StreamState;

    fn get_block(&self, block: u64) -> Option<Arc<AppendArray<T>>>;
}

pub struct StreamState {
    pub end: Idx,
    pub streaming: bool,
}

pub type IdxStream = Arc<dyn Stream<u64>>;

#[derive(Clone)]
pub enum AnyStream {
    I8(Arc<dyn Stream<u8>>),
    I16(Arc<dyn Stream<u16>>),
    I32(Arc<dyn Stream<u32>>),
    I64(Arc<dyn Stream<u64>>),
}

impl AnyStream {
    /// Address used for pointer equality and hash
    fn addr(&self) -> *const () {
        match self {
            AnyStream::I8(x) => Arc::as_ptr(x) as *const (),
            AnyStream::I16(x) => Arc::as_ptr(x) as *const (),
            AnyStream::I32(x) => Arc::as_ptr(x) as *const (),
            AnyStream::I64(x) => Arc::as_ptr(x) as *const (),
        }
    }
}

impl PartialEq for AnyStream {
    fn eq(&self, other: &Self) -> bool {
        self.addr() == other.addr()
    }
}
impl Eq for AnyStream {}

impl Hash for AnyStream {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.addr().hash(state);
    }
}

impl From<Arc<dyn Stream<u8>>> for AnyStream {
    fn from(s: Arc<dyn Stream<u8>>) -> Self {
        AnyStream::I8(s)
    }
}

impl From<Arc<dyn Stream<u16>>> for AnyStream {
    fn from(s: Arc<dyn Stream<u16>>) -> Self {
        AnyStream::I16(s)
    }
}

impl From<Arc<dyn Stream<u32>>> for AnyStream {
    fn from(s: Arc<dyn Stream<u32>>) -> Self {
        AnyStream::I32(s)
    }
}

impl From<Arc<dyn Stream<u64>>> for AnyStream {
    fn from(s: Arc<dyn Stream<u64>>) -> Self {
        AnyStream::I64(s)
    }
}