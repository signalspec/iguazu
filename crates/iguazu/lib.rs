//! Tools for viewing, storing, and sharing mixed-signal time series data
#![allow(clippy::useless_format, clippy::type_complexity, clippy::manual_async_fn)]

pub mod stream;
pub mod schema;
pub mod import;
pub mod export;
pub mod storage;
pub mod io;
pub mod view;
pub mod summary;
pub mod time;
mod util;
mod element;
pub mod izs;
pub mod config;
pub use element::{Element, ElementSize};

#[cfg(all(feature="clap", any(target_family = "unix", target_family = "windows")))]
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
