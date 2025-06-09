pub mod stream;
pub mod schema;
pub mod import;
pub mod storage;
pub mod io;
pub mod view;

#[cfg(feature = "clap")]
pub mod cli;

pub type Idx = u64;

/// End-exclusive range of indexes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdxRange {
    pub min: Idx,
    pub max: Idx,
}

impl IdxRange {
    pub fn is_empty(&self) -> bool {
        self.min >= self.max
    }

    pub fn len(&self) -> u64 {
        self.max - self.min
    }
    
    pub fn contains(&self, other: IdxRange) -> bool {
        self.min <= other.min && other.max <= self.max
    }

    pub fn divide(&self, by: u64) -> IdxRange {
        IdxRange { min: self.min / by, max: self.max.div_ceil(by) }
    }
    
    fn multiply(&self, by: u64) -> IdxRange {
        IdxRange { min: self.min * by, max: self.max * by }
    }
}


