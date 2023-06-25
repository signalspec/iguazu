use std::collections::VecDeque;
use std::sync::Arc;

use append_array::AppendArray;

use crate::{Idx, IdxRange, Stream};

mod int;
pub use int::IntView;

pub struct Cache<T> {
    /// Wrapped stream
    stream: Arc<dyn Stream<T>>,

    /// Cached blocks
    blocks: VecDeque<Arc<AppendArray<T>>>,

    /// Block index of first block in `blocks`
    offset: u64,

    ///
    range: IdxRange,
}

impl<T: Copy + 'static> Cache<T> {
    pub fn new(stream: Arc<dyn Stream<T>>) -> Self {
        Cache {
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
        let block_size = self.stream.block_size();
        let min_block = range.min / (block_size as u64);
        let max_block = range.max / (block_size as u64);

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
        while max_block >= self.offset + self.blocks.len() as u64 {
            let block = self.offset + self.blocks.len() as u64;
            self.blocks.push_back(self.stream.get_block(block).unwrap())
        }

        self.range = range;
    }

    pub fn for_each_elem(&self, mut f: impl FnMut(Idx, Option<T>)) {
        let block_size = self.stream.block_size();
        for (block_i, block) in self.blocks.iter().enumerate() {
            let idx = (self.offset + block_i as u64) * block_size as u64;
            let data = block.as_slice();

            let start = self.range.min.saturating_sub(idx) as usize;
            let end = self.range.max.saturating_sub(idx).min(data.len() as u64) as usize;

            for (i, v) in data[start..end].iter().enumerate() {
                f(idx + start as u64 + i as u64, Some(*v))
            }

            for i in (idx + end as u64)..(self.range.max.min(idx + block_size as u64)) {
                f(i, None)
            }
        }
    }
}
