use std::{cell::RefCell, rc::Rc, sync::Arc, u64};

use append_array::AppendArray;

use crate::{schema::EntityStream, stream::StreamDesc, Idx, IdxRange};

use super::{StreamAccess, ViewManager};

pub struct IntView {
    view: Rc<dyn StreamAccess>,
    desc: StreamDesc,
    cache: RefCell<(u64, Option<Arc<AppendArray<u8>>>)>,
}

impl IntView {
    pub fn new(vm: &mut impl ViewManager, entity: &EntityStream) -> Self {
        let view = vm.stream(&entity.data);
        let desc = view.stream().desc();
        let cache = RefCell::new((u64::MAX, None));
        IntView { view, desc, cache }
    }

    pub fn get_u64(&self, idx: Idx) -> Option<u64> {
        let block = idx / self.desc.block_size as Idx;
        let pos = idx % self.desc.block_size as Idx;
        let byte_pos = pos as usize * self.desc.element_size;

        let mut cache = self.cache.borrow_mut();

        if cache.0 != block {
            cache.0 = block;
            cache.1 = self.view.get_block(block);
        }

        let elem = cache.1.as_ref()?.get( byte_pos .. byte_pos + self.desc.element_size)?;
        let mut data = [0; 8];
        data[..elem.len()].copy_from_slice(elem);
        Some(u64::from_le_bytes(data))
    }

    pub fn for_each_elem<'a>(&'a self, range: IdxRange, mut f: impl FnMut(Idx, Option<u64>)) {
        let min_block = range.min / self.desc.block_size as Idx;
        let max_block  = range.max.div_ceil(self.desc.block_size as Idx);

        for block_i in min_block..max_block {
            let block = self.view.get_block(block_i);
            let idx = (block_i as u64) * self.desc.block_size as u64;

            let data = block.as_ref().map_or(&[] as &[u8], |x| x.as_slice());

            let start = range.min.saturating_sub(idx).min((data.len() / self.desc.element_size) as u64) as usize;
            let end = range.max.saturating_sub(idx).min((data.len() / self.desc.element_size) as u64) as usize;

            for (i, v) in data[start * self.desc.element_size .. end * self.desc.element_size].chunks_exact(self.desc.element_size).enumerate() {
                let mut data = [0; 8];
                data[..v.len()].copy_from_slice(v);
                f(idx + start as u64 + i as u64, Some(u64::from_le_bytes(data)))
            }

            for i in (idx + end as u64)..(range.max.min(idx + self.desc.block_size as u64)) {
                f(i, None)
            }
        }
    }
}