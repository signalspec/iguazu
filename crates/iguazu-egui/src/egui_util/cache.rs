use hashbrown::{HashMap, hash_map::Entry};
use std::hash::Hash;

use egui::util::cache::CacheTrait;

/// Caches the results of a computation for one frame.
/// If it is still used next frame, it is not recomputed.
/// If it is not used next frame, it is evicted from the cache to save memory.
/// 
/// A customized version of [`egui::util::cache::FrameCache`]
/// that allows !Send keys and takes a closure instead of
/// requiring a trait
pub struct FrameCache<Key, Value> {
    generation: u32,
    cache: HashMap<Key, (u32, Value)>,
}

impl<Key, Value> FrameCache<Key, Value> {
    pub fn new() -> Self {
        FrameCache { generation: 0, cache: HashMap::new() }
    }

    /// Must be called once per frame to clear the cache.
    fn evict_cache(&mut self) {
        let current_generation = self.generation;
        self.cache.retain(|_key, cached| {
            cached.0 == current_generation // only keep those that were used this frame
        });
        self.generation = self.generation.wrapping_add(1);
    }
}

impl<Key, Value> Default for FrameCache<Key, Value> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Key, Value> FrameCache<Key, Value> {
    /// Get from cache (if the same key was used last frame)
    /// or recompute and store in the cache.
    pub fn get<'a>(&mut self, key: Key, f: fn(&Key) -> Value) -> &mut Value where Key: Eq + Hash {
        match self.cache.entry(key) {
            Entry::Occupied(entry) => {
                let cached = entry.into_mut();
                cached.0 = self.generation;
                &mut cached.1
            }
            Entry::Vacant(entry) => {
                let value = f(entry.key());
                &mut entry.insert((self.generation, value)).1
            }
        }
    }
}

impl<Value: 'static + Send + Sync, Computer: 'static + Send + Sync> CacheTrait
    for FrameCache<Value, Computer>
{
    fn update(&mut self) {
        self.evict_cache();
    }

    fn len(&self) -> usize {
        self.cache.len()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
