use std::sync::Arc;

use crate::{schema::EntityStream, Idx, IdxRange};

use super::View;

pub struct EnumView {
    view: Arc<View>,
}

impl EnumView {
    pub fn new(entity: &EntityStream, view: Arc<View>) -> Self {
        debug_assert!(Arc::ptr_eq(&entity.data, view.stream()));
        EnumView { view }
    }
    
    pub fn range(&self) -> IdxRange {
        self.view.range()
    }

    pub fn get(&self, idx: Idx) -> Option<(usize, Idx)> {
        let val = self.view.get(idx).filter(|b| b.len() <= 4).map(|b| {
            let mut data = [0; 4];
            data[..b.len()].copy_from_slice(b);
            u32::from_le_bytes(data) as usize
        })?;

        // TODO: get child index
        Some((val, 0))
    }
}