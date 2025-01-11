use egui::{util::cache::CacheTrait, Context, Ui};
use fragile::Fragile;
use hashbrown::HashMap;
use log::debug;
use std::{cell::{Cell, RefCell}, mem, rc::Rc, sync::Arc};

use iguazu::{
    stream::{ArcStream, Block, Stream},
    view::{StreamAccess, ViewManager},
};

#[derive(Default)]
struct ViewCacheMem {
    cache: Fragile<HashMap<usize, Rc<CachedStream>>>,
}

struct CachedStream {
    stream: Arc<dyn Stream>,
    cached_blocks: RefCell<HashMap<u64, CachedBlock>>,
    used: Cell<bool>,
}

struct CachedBlock {
    block: Option<Block>,
    used: bool,
}

fn key(s: &ArcStream) -> usize {
    Arc::as_ptr(s) as *const () as usize
}

impl ViewCacheMem {
    fn evict_cache(&mut self) {
        self.cache.get_mut().retain(|_, cs| {
            if cs.evict_cache() {
                true
            } else {
                debug!("evicting view of {:?}", cs.stream);
                false
            }
        });
    }

    pub fn get(&mut self, stream: &ArcStream) -> Rc<CachedStream> {
        let sc = self.cache.get_mut().entry(key(stream)).or_insert_with(|| {
            debug!("creating view of {stream:?}");
            Rc::new(CachedStream::new(stream.clone()))
        });
        sc.used.set(true);
        sc.clone()
    }
}

impl CacheTrait for ViewCacheMem {
    fn update(&mut self) {
        self.evict_cache();
    }

    fn len(&self) -> usize {
        self.cache.get().len()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl CachedStream {
    fn new(stream: ArcStream) -> Self {
        CachedStream {
            stream,
            cached_blocks: RefCell::new(HashMap::new()),
            used: Cell::new(true),
        }
    }

    fn evict_cache(&self) -> bool {
        let mut cb = self.cached_blocks.borrow_mut();
        cb.retain(|_, b| {
            mem::replace(&mut b.used, false)
        });
        self.used.replace(false)
    }
}

impl StreamAccess for CachedStream {
    fn stream(&self) -> &Arc<dyn Stream> {
        &self.stream
    }

    fn get_block(&self, block: u64) -> Option<Block> {
        let mut cb = self.cached_blocks.borrow_mut();
        let entry = cb.entry(block).or_insert_with(|| {
            CachedBlock { block: self.stream.get_block(block), used: true }
        });
        entry.used = true;
        entry.block.clone()
    }
}


pub struct ViewCache {
    ctx: Context,
}

impl ViewCache {
    pub fn with(ui: &Ui) -> ViewCache {
        ViewCache { ctx: ui.ctx().clone() }
    }
}

impl ViewManager for ViewCache {
    fn stream(&mut self, stream: &ArcStream) -> Rc<dyn StreamAccess> {
        self.ctx.memory_mut(|mem| {
            let mem = mem.caches.cache::<ViewCacheMem>();
            mem.get(stream)
        })
    }
}
