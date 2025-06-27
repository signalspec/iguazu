use std::{cell::RefCell, u64};

use crate::{schema::EntityStream, stream::{ArcStream, StreamAccess, StreamDesc, StreamState}, Idx, IdxRange};

use super::ViewManager;

pub struct IntView<'a> {
    view: &'a dyn StreamAccess,
    desc: StreamDesc,
    cache: RefCell<(u64, &'a [u8])>,
}

impl<'a> IntView<'a> {
    pub fn new_from_stream(vm: &'a ViewManager, stream: &ArcStream) -> Self {
        let view = vm.stream(stream);
        let desc = stream.desc();
        let cache = RefCell::new((u64::MAX, &[][..]));
        IntView { view, desc, cache }
    }

    pub fn new(vm: &'a ViewManager, entity: &EntityStream) -> Option<Self> {
        let stream = entity.data()?;
        Some(Self::new_from_stream(vm, stream))
    }

    pub fn desc(&self) -> &StreamDesc {
        &self.desc
    }

    pub fn state(&self) -> StreamState {
        self.view.state()
    }

    pub fn bounds(&self) -> IdxRange {
        IdxRange { min: 0, max: self.state().end }
    }

    pub fn get_u64(&self, idx: Idx) -> Option<u64> {
        let block = idx / self.desc.block_size as Idx;
        let pos = idx % self.desc.block_size as Idx;
        let byte_pos = pos as usize * self.desc.element_size.bytes();

        let mut cache = self.cache.borrow_mut();

        if cache.0 != block {
            cache.0 = block;
            cache.1 = self.view.get_block(block);
        }

        let elem = cache.1.get(byte_pos .. byte_pos + self.desc.element_size.bytes())?;
        let mut data = [0; 8];
        data[..elem.len()].copy_from_slice(elem);
        Some(u64::from_le_bytes(data))
    }

    pub fn for_each_elem(&self, range: IdxRange, mut f: impl FnMut(Idx, Option<u64>)) {
        let min_block = range.min / self.desc.block_size as Idx;
        let max_block  = range.max.div_ceil(self.desc.block_size as Idx);

        for block_i in min_block..max_block {
            let block = self.view.get_block(block_i);
            let idx = (block_i as u64) * self.desc.block_size as u64;

            let start = range.min.saturating_sub(idx).min((block.len() / self.desc.element_size.bytes()) as u64) as usize;
            let end = range.max.saturating_sub(idx).min((block.len() / self.desc.element_size.bytes()) as u64) as usize;

            for (i, v) in block[start * self.desc.element_size.bytes() .. end * self.desc.element_size.bytes()].chunks_exact(self.desc.element_size.bytes()).enumerate() {
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