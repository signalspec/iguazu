use std::{cell::RefCell, io, sync::Arc, task::{Context, Poll, Waker}};

use async_executor::Task;
use elsa::FrozenMap;
use futures_lite::FutureExt;
use hashbrown::{HashMap, HashSet, hash_map};
use once_array::OnceArray;

use crate::stream::{Stream, StreamAccess, StreamState};

pub trait LoadBlock: Stream {
    fn load_block(self: Arc<Self>, block: u64) -> LoadBlockRes;
}

pub enum LoadBlockRes {
    Loading(Task<Result<Arc<OnceArray<u8>>, io::Error>>),
    Cached(Arc<OnceArray<u8>>),
    NotFound,
}

pub struct CommonStreamAccess<S> {
    stream: Arc<S>,
    blocks: FrozenMap<u64, Arc<OnceArray<u8>>>,
    state: RefCell<CommonStreamAccessState>,
    waker: Waker,
}

struct CommonStreamAccessState {
    used: HashSet<u64>,
    loading: HashMap<u64, Task<Result<Arc<OnceArray<u8>>, io::Error>>>,
    error: Option<io::Error>,
}

impl<S> CommonStreamAccess<S> {
    pub fn new(stream: Arc<S>) -> Self {
        Self {
            stream,
            blocks: FrozenMap::new(),
            state: RefCell::new(CommonStreamAccessState {
                used: HashSet::new(),
                loading: HashMap::new(),
                error: None,
            }),
            waker: std::task::Waker::noop().clone(),
        }
    }
}

impl<S: LoadBlock + Send + Sync + 'static> StreamAccess for CommonStreamAccess<S> {
    fn get_block(&self, block: u64) -> &[u8] {
        let mut state = self.state.borrow_mut();
        state.used.insert(block);

        if let Some(buf) = self.blocks.get(&block) {
            // Block is already loaded
            return buf;
        }

        let is_error = state.error.is_some();
        let mut entry = match state.loading.entry(block) {
            hash_map::Entry::Occupied(entry) => entry,
            hash_map::Entry::Vacant(entry) => {
                if is_error {
                    return &[];
                }

                // Block is not loaded, start loading
                match self.stream.clone().load_block(block) {
                    LoadBlockRes::NotFound => {
                        return &[];
                    }
                    LoadBlockRes::Cached(buf) => {
                        return self.blocks.insert(block, buf)
                    }
                    LoadBlockRes::Loading(task) => {
                        entry.insert_entry(task)
                    }
                }
            }
        };

        let mut cx = Context::from_waker(&self.waker);
        if let Poll::Ready(res) = entry.get_mut().poll(&mut cx) {
            drop(entry.remove());
            match res {
                Ok(buf) => {
                    return self.blocks.insert(block, buf);
                }
                Err(e) => {
                    state.error = Some(e);
                }
            }
        }

        &[]
    }

    fn state(&self) -> StreamState {
        self.stream.state()
    }

    fn begin(&mut self, waker: &Waker) {
        self.waker.clone_from(waker);
    }

    fn end(&mut self) {
        let mut state = self.state.borrow_mut();
        self.blocks.as_mut().retain(|block, _| state.used.contains(block));
        state.used.clear();
    }
}
