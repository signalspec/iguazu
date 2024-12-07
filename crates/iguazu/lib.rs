pub mod stream;
pub mod schema;
pub mod import;
pub mod storage;
pub mod io;
pub mod view;

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
    
    pub fn contains(&self, other: IdxRange) -> bool {
        self.min <= other.min && other.max <= self.max
    }
}


