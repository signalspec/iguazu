use std::{cell::RefCell, collections::{BTreeMap, btree_map::Entry}, mem::{replace, transmute}, sync::Arc, task::Waker};

use crate::{io::IoError, storage::block::BlockState, stream::ArcStream};

struct Block {
    state: BlockState,
    used: bool,
}

pub struct BlockView {
    waker: Waker,
    stream: ArcStream,
    blocks: RefCell<BTreeMap<u64, Block>>,
}

impl BlockView {
    pub fn new(stream: ArcStream) -> Self {
        Self {
            waker: Waker::noop().clone(),
            stream,
            blocks: RefCell::new(BTreeMap::new()),
        }
    }

    pub fn begin(&mut self, waker: &Waker) {
        self.waker.clone_from(waker);
    }

    pub fn end(&mut self) -> bool {
        self.blocks.get_mut().retain(|_, block| replace(&mut block.used, false));
        !self.blocks.get_mut().is_empty()
    }

    pub fn loading_state(&self) -> LoadingState {
        self.blocks.borrow().values().map(|block| match block.state {
            BlockState::Error(_) => LoadingState::Error,
            BlockState::Pending | BlockState::Partial(_) => LoadingState::Loading,
            BlockState::Complete(_) | BlockState::End => LoadingState::Ready,
        }).max().unwrap_or(LoadingState::Ready)
    }

    pub fn error(&self) -> Option<Arc<IoError>> {
        self.blocks.borrow().values().find_map(|block| {
            match &block.state {
                BlockState::Error(err) => Some(err.clone()),
                _ => None,
            }
        })
    }

    pub fn get_block<'a>(&'a self, block_i: u64) -> &'a [u8] {
        let mut blocks = self.blocks.borrow_mut();
        let block = match blocks.entry(block_i) {
            Entry::Vacant(e) => {
                let state = self.stream.get_block(block_i, &self.waker);
                e.insert(Block { state, used: true })
            },
            Entry::Occupied(mut e) => {
                let block = e.get_mut();
                if !block.used {
                    if block.state.needs_refresh() {
                        block.state = self.stream.get_block(block_i, &self.waker);
                    }
                    block.used = true;
                }
                e.into_mut()
            },
        };
        let bytes = block.state.bytes();

        // SAFETY: Extend the lifetime of the slice to `'a`:
        // * The slice is either empty or borrowed from a `Arc<OnceArray>` owned by
        //  `BlockState` in the map.
        // * Once `block.used` is set, the block remains in the map and will not be
        //   replaced as long as `&'a self` is valid.
        // * Blocks are removed and `block.used` is cleared only in `end()`, which takes
        //   `&mut self`, invalidating this reference.
        // * `OnceArray` has its own allocation and the buffer remains valid at a
        //   stable address even if the `BlockState` is moved while inserting other
        //   items in the map.
        unsafe { transmute::<&[u8], &'a [u8]>(bytes) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
pub enum LoadingState {
    Ready,
    Loading,
    Error,
}
