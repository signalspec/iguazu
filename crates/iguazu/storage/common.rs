use std::{sync::Arc, task::Context };

use crate::{ stream::{BlockDesc, IterState, Stream, StreamIter},  storage::block::BlockState };

pub(crate) struct CommonStreamIter {
    stream: Arc<dyn Stream>,
    block_desc: BlockDesc,
    state: BlockState,
    block: u64,
    pos_in_block: usize,
}

impl CommonStreamIter {
    pub fn new(stream: Arc<dyn Stream>) -> Self {
        Self {
            block_desc: stream.desc(),
            stream,
            state: BlockState::Pending,
            block: 0,
            pos_in_block: 0,
        }
    }
}

impl StreamIter for CommonStreamIter {
    fn desc(&self) -> BlockDesc {
        self.stream.desc()
    }

    fn poll_next(&mut self, cx: &mut Context) -> IterState<'_> {
        if self.state.needs_refresh() {
            self.state = self.stream.get_block(self.block, cx.waker());
        }

        match &self.state {
            BlockState::Pending => IterState::Partial(&[]),
            BlockState::End => IterState::Complete(&[]),
            BlockState::Partial(buf) => IterState::Partial(&buf[self.pos_in_block * self.block_desc.element_size.bytes()..]),
            BlockState::Complete(buf) => IterState::Complete(&buf[self.pos_in_block * self.block_desc.element_size.bytes()..]),
            BlockState::Error => {
                IterState::Error("")
            }
        }
    }

    fn consume(&mut self, count: usize) {
        debug_assert!(self.pos_in_block + count <= self.block_desc.count);
        self.pos_in_block += count;

        if self.pos_in_block >= self.block_desc.count {
            self.block += 1;
            self.pos_in_block = 0;
            self.state = BlockState::Pending;
        }
    }
}
