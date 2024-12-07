use egui::{util::cache::CacheTrait, Id, Ui};
use log::{debug, warn};
use std::{
    collections::{btree_map::Entry, BTreeMap},
    sync::Arc,
};

use iguazu::{
    stream::ArcStream,
    view::{View, ViewManager},
    IdxRange,
};

#[derive(Default)]
struct ViewCacheMem {
    generation: u32,
    cache: BTreeMap<(u64, usize), CacheEntry>,
}

struct CacheEntry {
    view: Arc<View>,
    generation: u32,
}

fn key(id: Id, s: &ArcStream) -> (u64, usize) {
    (id.value(), Arc::as_ptr(s) as *const () as usize)
}

impl ViewCacheMem {
    fn evict_cache(&mut self) {
        let current_generation = self.generation;
        self.cache.retain(|(id, _), cached| {
            let keep = cached.generation == current_generation; // only keep those that were used this frame
            if !keep {
                debug!("evicting view of {:?} for {:04X}", cached.view.stream(), *id as u16);
            }
            keep
        });
        self.generation = self.generation.wrapping_add(1);
    }
}

impl ViewCacheMem {
    pub fn get(&mut self, id: Id, stream: &ArcStream, range: IdxRange) -> Arc<View> {
        let rc_view = match self.cache.entry(key(id, stream)) {
            Entry::Occupied(entry) => {
                let cached = entry.into_mut();
                cached.generation = self.generation;
                &mut cached.view
            }
            Entry::Vacant(entry) => {
                debug!("creating view of {stream:?} for {id:?} ");
                let view = Arc::new(View::new(stream.clone()));
                &mut entry
                    .insert(CacheEntry {
                        view,
                        generation: self.generation,
                    })
                    .view
            }
        };

        if let Some(mut_view) = Arc::get_mut(rc_view) {
            mut_view.set_range(range);
        } else if !rc_view.range().contains(range) {
            warn!("view for {id:?} {stream:?} requested range {range:?} but {existing_range:?} already in use", existing_range = rc_view.range());
        }

        rc_view.clone()
    }
}

impl CacheTrait for ViewCacheMem {
    fn update(&mut self) {
        self.evict_cache();
    }

    fn len(&self) -> usize {
        self.cache.len()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub struct ViewCache<'a> {
    ui: &'a Ui,
}

impl<'a> ViewCache<'a> {
    pub fn with(ui: &'a Ui) -> ViewCache<'a> {
        ViewCache { ui }
    }
}

impl ViewManager for ViewCache<'_> {
    fn view(&mut self, stream: &ArcStream, range: IdxRange) -> Arc<View> {
        self.ui.memory_mut(|mem| {
            let mem = mem.caches.cache::<ViewCacheMem>();
            mem.get(self.ui.id(), stream, range)
        })
    }
}
