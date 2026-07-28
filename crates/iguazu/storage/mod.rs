//! Data storage and access backends.
pub(crate) mod common;
pub mod block;

use block::{BlockState, BlockTask, CachePool};

use std::{future::Future, io, sync::{Arc, Mutex, Weak}};
use async_executor::Executor;
use once_array::OnceArray;

mod in_memory;
pub use in_memory::{ MemoryStorage, MemoryStream, MemoryStreamWriter };

mod flat_file;
pub use flat_file::{ FlatFileStream, FlatFileOpts };

use crate::{ElementSize, stream::StreamWriter};

#[cfg(feature = "izs")]
pub mod izs;

#[cfg(feature = "srzip")]
pub mod srzip;

/// A storage backend that can create writable streams
pub trait Storage: Send + Sync {
    fn create_stream(&self, element_type: ElementSize) -> Box<dyn StreamWriter>;
}

pub struct Pool {
    cache: Mutex<CachePool>,
    pub executor: Arc<Executor<'static>>,
}

impl Pool {
    pub fn new(executor: Arc<Executor<'static>>, cache_limit: usize) -> Self {
        Pool { cache: Mutex::new(CachePool::new(cache_limit)), executor }
    }

    pub fn stats(&self) -> PoolStats {
        let cache = self.cache.lock().unwrap();
        PoolStats {
            cache_limit: cache.limit(),
            cache_usage: cache.total(),
        }
    }

    pub fn spawn_block_fetch(self: Arc<Self>, fut: impl Future<Output = Result<Arc<OnceArray<u8>>, io::Error>> + Send + 'static) -> Arc<BlockTask> {
        Arc::new_cyclic(move |t: &Weak<BlockTask>| {
            let t = t.clone();
            BlockTask::new(self.executor.clone().spawn(async move {
                let block = fut.await;
                if let Some(t) = t.upgrade() {
                    if let Ok(block) = &block {
                        self.cache.lock().unwrap().insert(block.clone());
                    }
                    t.complete(block);
                }
            }))
        })
    }
}

pub struct PoolStats {
    pub cache_limit: usize,
    pub cache_usage: usize,
}
