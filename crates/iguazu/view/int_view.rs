use std::{cell::RefCell, marker::PhantomData, u64};

use crate::{Element, schema::EntityStream, stream::{ArcStream, StreamAccess, StreamDesc, StreamState}, Idx, IdxRange};

use super::ViewManager;

#[derive(Clone)]
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

    fn block(&self, block: u64) -> &'a [u8] {
        let mut cache = self.cache.borrow_mut();

        if cache.0 != block {
            cache.0 = block;
            cache.1 = self.view.get_block(block);
        }

        cache.1
    }

    pub fn get_u64(&self, idx: Idx) -> Option<u64> {
        let block = idx / self.desc.block_size as Idx;
        let pos = idx % self.desc.block_size as Idx;
        let byte_pos = pos as usize * self.desc.element_type.bytes();

        let elem = self.block(block).get(byte_pos .. byte_pos + self.desc.element_type.bytes())?;
        let mut data = [0; 8];
        data[..elem.len()].copy_from_slice(elem);
        Some(u64::from_le_bytes(data))
    }
    
    pub fn loaded_chunks<'v, T: Element>(&'v self, range: IdxRange) -> LoadedChunkIter<'v, 'a, T> {
        LoadedChunkIter::new(self, range)
    }

    pub fn for_each_elem(&self, range: IdxRange, mut f: impl FnMut(Idx, Option<u64>)) {
        let min_block = range.min / self.desc.block_size as Idx;
        let max_block  = range.max.div_ceil(self.desc.block_size as Idx);

        for block_i in min_block..max_block {
            let block = self.view.get_block(block_i);
            let idx = (block_i as u64) * self.desc.block_size as u64;

            let start = range.min.saturating_sub(idx).min((block.len() / self.desc.element_type.bytes()) as u64) as usize;
            let end = range.max.saturating_sub(idx).min((block.len() / self.desc.element_type.bytes()) as u64) as usize;

            for (i, v) in block[start * self.desc.element_type.bytes() .. end * self.desc.element_type.bytes()].chunks_exact(self.desc.element_type.bytes()).enumerate() {
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


pub struct LoadedChunkIter<'v, 'a, T> {
    view: &'v IntView<'a>,
    block: u64,
    pos: usize,
    remaining: u64,
    dtype: PhantomData<T>,
}


impl<'v, 'a, T: Element> LoadedChunkIter<'v, 'a, T> {
    fn new(view: &'v IntView<'a>, range: IdxRange) -> Self {
        assert_eq!(view.desc.element_type, T::ELEMENT_TYPE, "Element type mismatch in ChunkIter");
        let block_size = view.desc.block_size as u64;
        let block = range.min / block_size;
        let pos = (range.min % block_size) as usize;
        let remaining = range.len();
        LoadedChunkIter { view, block, pos, remaining, dtype: PhantomData }
    }
}

impl<'v, 'a, T: Element> Iterator for LoadedChunkIter<'v, 'a, T> {
    type Item = &'a [T];

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        let data = bytemuck::cast_slice::<u8, T>(self.view.block(self.block));
        let is_fully_loaded = data.len() == self.view.desc.block_size;

        if data.len() <= self.pos {
            return None;
        }

        let data = &data[self.pos..];
        let data = &data[..self.remaining.min(data.len() as u64) as usize];

        if is_fully_loaded {
            self.block += 1;
            self.pos = 0;
            self.remaining -= data.len() as u64;
        } else {
            self.remaining = 0;
        }

        Some(data)
    }
}
