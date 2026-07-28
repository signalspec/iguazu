use std::{collections::VecDeque, io, sync::{Arc, Mutex, OnceLock, Weak}, task::Waker, fmt::Debug};
use once_array::OnceArray;

use hashbrown::HashMap;
use async_executor::Task;

use crate::util::waker_set::WakerSet;

/// Result of `[Stream::get_block]`.
#[derive(Clone)]
pub enum BlockState {
    /// Partial block with data still being added.
    Partial(Arc<OnceArray<u8>>),

    /// No more elements will be received: Either the block is full, or the stream has ended.
    Complete(Arc<OnceArray<u8>>),

    /// The block is currently being loaded from storage, or is a future block that may be created later.
    Pending,

    /// The requested block is past the end of a completed stream or is otherwise known to never be available.
    End,

    /// There was a problem retrieving the block from storage.
    Error,
}

impl BlockState {
    pub fn bytes(&self) -> &[u8] {
        match self {
            BlockState::Complete(arc) | BlockState::Partial(arc) => &*arc,
            BlockState::Error | BlockState::Pending | BlockState::End => &[],
        }
    }

    pub(crate) fn needs_refresh(&self) -> bool {
        matches!(self, BlockState::Pending | BlockState::Partial(_))
    }
}

impl Debug for BlockState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockState::Complete(arc) => write!(f, "Complete({} bytes)", arc.len()),
            BlockState::Partial(arc) => write!(f, "Streaming({} bytes)", arc.len()),
            BlockState::Pending => write!(f, "Pending"),
            BlockState::End => write!(f, "End"),
            BlockState::Error => write!(f, "Error"),
        }
    }
}

pub struct BlockTask {
    result: OnceLock<Result<Arc<OnceArray<u8>>, io::Error>>,
    wakers: WakerSet,
    _task: Task<()>,
}

impl BlockTask {
    pub(crate) fn new(task: Task<()>) -> Self {
        BlockTask {
            result: OnceLock::new(),
            wakers: WakerSet::new(),
            _task: task,
        }
    }

    pub fn subscribe(&self, waker: &Waker) {
        self.wakers.insert(waker);
    }

    pub(crate) fn complete(&self, result: Result<Arc<OnceArray<u8>>, io::Error>) {
        self.result.set(result).ok();
        self.wakers.wake_all();
    }
}

#[derive(Clone)]
enum CacheEntry {
    OnceArray(Weak<OnceArray<u8>>),
    Task(Arc<BlockTask>),
}

impl CacheEntry {
    fn upgrade(&mut self, waker: &Waker) -> Option<BlockState> {
        match self {
            CacheEntry::OnceArray(weak) => Some(BlockState::Complete(weak.upgrade()?)),
            CacheEntry::Task(task) => {
                match task.result.get() {
                    Some(Ok(inner)) => {
                        let block = BlockState::Complete(inner.clone());
                        *self = CacheEntry::OnceArray(Arc::downgrade(inner));
                        Some(block)
                    }
                    Some(Err(_)) => Some(BlockState::Error),
                    None => {
                        task.subscribe(waker);
                        Some(BlockState::Pending)
                    }
                }
            }
        }
    }

    fn validate(&mut self) -> bool {
        match self {
            CacheEntry::OnceArray(weak) => weak.strong_count() > 0,
            CacheEntry::Task(arc) if let Some(Ok(inner)) = arc.result.get() => {
                *self = CacheEntry::OnceArray(Arc::downgrade(inner));
                true
            }
            CacheEntry::Task(_) => true,
        }
    }

    fn is_error(&self) -> bool {
        match self {
            CacheEntry::Task(arc) if let Some(Err(_)) = arc.result.get() => true,
            _ => false,
        }
    }
}

/// Cache of loaded and loading blocks indexed by block number of specific stream.
///
/// This holds weak references to loaded blocks. The strong reference is held by the Pool
/// so it can manage blocks across all streams.
///
/// It holds strong references to tasks for blocks being loaded.
pub struct BlockCache {
    cache: Mutex<HashMap<u64, CacheEntry>>,
}

impl BlockCache {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn insert_task(&self, key: u64, task: Arc<BlockTask>) {
        let mut cache = self.cache.lock().unwrap();
        let len = cache.len();
        if cache.capacity() == len {
            // Prune references to dead entries before allocating more space.
            // `insert` below calls `reserve`, which will try to re-hash to clear tombstones.
            cache.retain(|_, v| v.validate());
        }

        cache.insert(key, CacheEntry::Task(task));
    }

    pub fn get(&self, key: u64, waker: &Waker) -> Option<BlockState> {
        let mut cache = self.cache.lock().unwrap();
        cache.get_mut(&key)?.upgrade(waker)
    }

    pub fn clear_errors(&self) {
        let mut cache = self.cache.lock().unwrap();
        cache.retain(|_, v| !v.is_error());
    }
}


#[test]
fn test_block_cache() {
    use std::assert_matches;
    let executor = async_executor::Executor::new();
    let map = BlockCache::new();
    dbg!(map.cache.lock().unwrap().capacity());

    let task0 = Arc::new(BlockTask::new(executor.spawn(async { })));
    let task1 = Arc::new(BlockTask::new(executor.spawn(async { })));
    let task2 = Arc::new(BlockTask::new(executor.spawn(async { })));
    let task3 = Arc::new(BlockTask::new(executor.spawn(async { })));

    map.insert_task(0, task0.clone());

    map.insert_task(1, task1.clone());
    assert_matches!(map.get(1, Waker::noop()), Some(BlockState::Pending));

    task1.complete(Ok(Arc::new(OnceArray::from(vec![1]))));
    assert_matches!(map.get(1, Waker::noop()), Some(BlockState::Complete(_)));

    drop(task1);
    assert_matches!(map.get(1, Waker::noop()), None);
    assert_eq!(map.cache.lock().unwrap().contains_key(&1), true);

    dbg!(map.cache.lock().unwrap().capacity());
    for i in 10..15 { map.insert_task(i, task2.clone()); } // at capacity, triggers lazy cleanup
    assert_matches!(map.get(10, Waker::noop()), Some(BlockState::Pending));
    assert_eq!(map.cache.lock().unwrap().contains_key(&1), false);
    assert_eq!(map.cache.lock().unwrap().len(), 6);
    dbg!(map.cache.lock().unwrap().capacity());

    task2.complete(Ok(Arc::new(OnceArray::from(vec![2]))));
    drop(task2);

    for i in 20..30 { map.insert_task(i, task3.clone()); } // at capacity, triggers lazy cleanup
    assert_eq!(map.cache.lock().unwrap().contains_key(&10), false);
    dbg!(map.cache.lock().unwrap().capacity());

    assert_matches!(map.get(0, Waker::noop()), Some(BlockState::Pending));
}

/// Shared pool for holding a limited number of blocks for the caches of their streams.
///
/// This holds strong references to blocks, but drops them if the pool is full and the pool holds
/// the last strong reference.
///
/// The cache policy avoids evicting the most recently inserted entries, but beyond that
/// is effectively random. The real cache policy is that views hold references to the
/// blocks used in the last frame and prevent their eviction.
pub struct CachePool {
    total: usize,
    limit: usize,
    entries: VecDeque<Arc<OnceArray<u8>>>,
}

impl CachePool {
    pub fn new(limit: usize) -> Self {
        Self {
            total: 0,
            limit,
            entries: VecDeque::new(),
        }
    }

    fn try_to_evict(&mut self) {
        if let Some(to_remove) = self.entries.iter().position(|entry| Arc::strong_count(entry) == 1) {
            self.entries.rotate_left(to_remove);
            let entry = self.entries.pop_front().unwrap();
            self.total -= entry.capacity();
        }
    }

    pub fn insert(&mut self, entry: Arc<OnceArray<u8>>) {
        self.total += entry.capacity();

        if self.total > self.limit {
            self.try_to_evict();
        }

        self.entries.push_back(entry);
    }

    pub fn total(&self) -> usize {
        self.total
    }

    pub fn limit(&self) -> usize {
        self.limit
    }
}

#[test]
fn test_cache_pool() {
    use once_array::OnceArrayWriter;
    let mut pool = CachePool::new(100);
    let entry1 = OnceArrayWriter::with_capacity(50).reader().clone();
    let entry2 = OnceArrayWriter::with_capacity(30).reader().clone();
    let entry3 = OnceArrayWriter::with_capacity(40).reader().clone();
    let entry4 = OnceArrayWriter::with_capacity(45).reader().clone();

    pool.insert(entry1.clone());
    assert_eq!(pool.total, 50);
    drop(entry1);

    pool.insert(entry2.clone());
    assert_eq!(pool.total, 80);

    pool.insert(entry3.clone());
    assert_eq!(pool.total, 70); // entry1 should be evicted

    assert_eq!(Arc::strong_count(&entry2), 2); // entry2 should still be in the pool
    assert_eq!(Arc::strong_count(&entry3), 2); // entry3 should still be in the pool

    pool.insert(entry4.clone());
    assert_eq!(pool.total, 115); // over limit, but all still referenced

    drop(entry3);
    drop(entry4);

    pool.try_to_evict();
    assert_eq!(pool.total, 75); // evicted entry3

    assert_eq!(Arc::strong_count(&entry2), 2); // entry2 should still be in the pool
}
