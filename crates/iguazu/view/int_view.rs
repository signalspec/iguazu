use std::sync::Arc;

use crate::{schema::EntityStream, Idx, IdxRange};

use super::View;

pub struct IntView {
    view: Arc<View>,
}

impl IntView {
    pub fn new(entity: &EntityStream, view: Arc<View>) -> Self {
        debug_assert!(Arc::ptr_eq(&entity.data, view.stream()));
        IntView { view }
    }

    pub fn range(&self) -> IdxRange {
        self.view.range()
    }

    pub fn get(&self, idx: Idx) -> Option<u64> {
        self.view.get(idx).filter(|b| b.len() <= 8).map(|b| {
            let mut data = [0; 8];
            data[..b.len()].copy_from_slice(b);
            u64::from_le_bytes(data)
        })
    }
}