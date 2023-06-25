pub mod stream;
pub use stream::{Stream, AnyStream};

mod time;
pub use time::TimeType;

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


