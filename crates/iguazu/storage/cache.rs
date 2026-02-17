use std::{collections::{VecDeque}, sync::Arc};
use once_array::{OnceArray};

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
