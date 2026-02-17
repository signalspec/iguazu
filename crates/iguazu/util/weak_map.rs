use std::{hash::Hash, sync::{Arc, Weak}};

use hashbrown::HashMap;

/// A map that holds `Weak` references to its values.
///
/// Entries whose values have no strong references elswhere will behave as if
/// removed from the map, and will be lazily cleaned up when inserting new
/// items or when `prune` is called.
pub struct WeakMap<K, V> {
    map: HashMap<K, Weak<V>>,
}

impl<K, V> WeakMap<K, V> where K: Eq + Hash {
    pub fn new() -> Self {
        WeakMap { map: HashMap::new() }
    }

    pub fn get(&self, key: &K) -> Option<Arc<V>> {
        self.map.get(key)?.upgrade()
    }

    pub fn insert(&mut self, key: K, value: Arc<V>) {
        let len = self.map.len();
        if self.map.capacity() == len {
            self.map.retain(|_, v| v.strong_count() > 0);

            if self.map.len() < len {
                // If items were removed, force it to re-hash now.
                // Deleting elements leaves a tombstone entry but doesn't increase capacity, so if the insert
                // doesn't hit a tombstone, the next insert would trigger another linear scan. In the steady
                // state this `reserve` will simply be a re-hash to clear tombstones, rather than reallocating.
                self.map.reserve(1);
            }
        }

        self.map.insert(key, Arc::downgrade(&value));
    }
}

#[test]
fn test_weak_map() {
    let mut map = WeakMap::new();
    dbg!(map.map.capacity());

    let value0 = Arc::new(0);
    let value1 = Arc::new(1);
    let value2 = Arc::new(2);
    let value3 = Arc::new(2);

    map.insert(0, value0.clone());
    map.insert(1, value1.clone());
    assert_eq!(map.get(&1).as_deref(), Some(&1));

    drop(value1);

    assert_eq!(map.get(&1), None);
    assert_eq!(map.map.contains_key(&1), true);

    for i in 10..20 { map.insert(i, value2.clone()); } // at capacity, triggers lazy cleanup
    assert_eq!(map.get(&10).as_deref(), Some(&2));
    assert_eq!(map.map.contains_key(&1), false);
    assert_eq!(map.map.len(), 11);
    dbg!(map.map.capacity());

    drop(value2);

    for i in 20..30 { map.insert(i, value3.clone()); } // at capacity, triggers lazy cleanup
    assert_eq!(map.map.contains_key(&10), false);
    dbg!(map.map.capacity());

    assert_eq!(map.get(&0).as_deref(), Some(&0));
}
