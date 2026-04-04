//! Data storage and access backends.
mod cache;
mod common;

mod in_memory;

use std::sync::{Arc, Mutex};

use async_executor::Executor;
pub use in_memory::{ MemoryStorage, MemoryStream, MemoryStreamWriter };

mod flat_file;
pub use flat_file::{ FlatFileStream, FlatFileOpts };

use crate::{stream::StreamWriter, ElementSize};

#[cfg(feature = "izs")]
pub mod izs;

#[cfg(feature = "srzip")]
pub mod srzip;

/// A storage backend that can create writable streams
pub trait Storage: Send + Sync {
    fn create_stream(&self, element_type: ElementSize) -> Box<dyn StreamWriter>;
}

pub struct Pool {
    cache: Mutex<cache::CachePool>,
    pub executor: Arc<Executor<'static>>,
}

impl Pool {
    pub fn new(executor: Arc<Executor<'static>>, cache_limit: usize) -> Self {
        Pool { cache: Mutex::new(cache::CachePool::new(cache_limit)), executor }
    }

    pub fn stats(&self) -> PoolStats {
        let cache = self.cache.lock().unwrap();
        PoolStats {
            cache_limit: cache.limit(),
            cache_usage: cache.total(),
        }
    }
}

pub struct PoolStats {
    pub cache_limit: usize,
    pub cache_usage: usize,
}
