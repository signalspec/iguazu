use std::collections::VecDeque;
use std::sync::Arc;

use append_array::AppendArray;

use crate::{Idx, IdxRange, stream::{ Stream, ArcStream }};

pub struct View {
    /// Wrapped stream
    stream: Arc<dyn Stream>,

    /// Cached blocks
    blocks: VecDeque<Arc<AppendArray<u8>>>,

    /// Block index of first block in `blocks`
    offset: u64,

    /// Selected range
    range: IdxRange,
}

impl View {
    pub fn new(stream: Arc<dyn Stream>) -> Self {
        View {
            stream,
            blocks: VecDeque::new(),
            offset: 0,
            range: IdxRange {
                min: 0,
                max: 0,
            },
        }
    }

    pub fn set_range(&mut self, range: IdxRange) {
        let desc = self.stream.desc();
        let block_size = desc.block_size;
        let min_block = range.min / (block_size as u64);
        let max_block = range.max.div_ceil(block_size as u64);

        if max_block < self.offset || min_block >= self.offset + self.blocks.len() as u64 {
            // no overlap with existing range; start over
            self.blocks.clear();
            self.offset = min_block;
        }

        // remove blocks from start if start has moved forward
        while min_block > self.offset {
            self.blocks.pop_front();
            self.offset += 1;
        }

        // remove blocks from end if end has shifted backwards
        while max_block < self.offset + self.blocks.len() as u64 {
            self.blocks.pop_back();
        }

        // load blocks at start
        while min_block < self.offset {
            let block = self.offset - 1;
            self.blocks
                .push_front(self.stream.get_block(block).unwrap());
            self.offset = block;
        }

        // load blocks at end
        while max_block > self.offset + self.blocks.len() as u64 {
            let block = self.offset + self.blocks.len() as u64;
            self.blocks.push_back(self.stream.get_block(block).unwrap())
        }

        self.range = range;
    }

    #[inline]
    pub fn stream(&self) -> &ArcStream {
        &self.stream
    }

    #[inline]
    pub fn range(&self) -> IdxRange {
        self.range
    }

    pub fn get(&self, idx: Idx) -> Option<&[u8]> {
        let desc = self.stream.desc();
        let block = idx / desc.block_size as Idx;
        let pos = idx % desc.block_size as Idx;
        let block = self.blocks.get(block.checked_sub(self.offset)? as usize)?;

        let byte_pos = pos as usize * desc.element_size;
        block.get( byte_pos .. byte_pos + desc.element_size)
    }

    pub fn for_each_elem<'a>(&'a self, mut f: impl FnMut(Idx, Option<&'a [u8]>)) {
        let desc = self.stream.desc();

        for (block_i, block) in self.blocks.iter().enumerate() {
            let idx = (self.offset + block_i as u64) * desc.block_size as u64;
            let data = block.as_slice();

            let start = self.range.min.saturating_sub(idx).min((data.len() / desc.element_size) as u64) as usize;
            let end = self.range.max.saturating_sub(idx).min((data.len() / desc.element_size) as u64) as usize;

            for (i, v) in data[start * desc.element_size .. end * desc.element_size].chunks_exact(desc.element_size).enumerate() {
                f(idx + start as u64 + i as u64, Some(v))
            }

            for i in (idx + end as u64)..(self.range.max.min(idx + desc.block_size as u64)) {
                f(i, None)
            }
        }
    }
}
