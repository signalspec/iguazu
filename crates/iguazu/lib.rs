pub mod stream;
pub mod entity;
pub use stream::{Stream, IdxStream, AnyStream};

pub mod in_memory;

pub type Idx = u64;

/// End-exclusive range of indexes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdxRange {
    pub min: Idx,
    pub max: Idx,
}

impl IdxRange {
    pub fn len(&self) -> u64 {
        self.max - self.min
    }
}


