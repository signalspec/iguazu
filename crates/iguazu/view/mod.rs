use std::{mem, sync::Arc};

use crate::{schema::EntityStream, stream::{ArcStream, StreamAccess}};

mod int_view;
use elsa::FrozenMap;
pub use int_view::IntView;

mod number_view;
pub use number_view::NumberView;

mod enum_view;
pub use enum_view::EnumView;

mod event_view;
pub use event_view::{ EventView, EventViewIter, Event };

mod text_view;
pub use text_view::TextView;

#[derive(Default)]
pub struct ViewManager {
    streams: FrozenMap<usize, Box<dyn StreamAccess>>,
}

impl ViewManager {
    pub fn new() -> Self {
        ViewManager {
            streams: FrozenMap::default(),
        }
    }

    pub fn update(&mut self) {
        let mut streams = mem::take(&mut self.streams).into_map();
        for (_, stream) in streams.iter_mut() {
            stream.reset();
        }
        // TODO: clear stale streams
        self.streams = streams.into();
    }
}

fn key(s: &ArcStream) -> usize {
    Arc::as_ptr(s) as *const () as usize
}

impl ViewManager {
    pub fn stream<'a>(&'a self, stream: &ArcStream) -> &'a dyn StreamAccess {
        if let Some(s) = self.streams.get(&key(stream)) {
            return s;
        } else {
            self.streams.insert(key(stream), stream.clone().access())
        }
    }
    
    pub fn int_view<'a>(&'a self, entity: &EntityStream) -> IntView<'a> {
        IntView::new(self, entity)
    }

    pub fn number_view<'a>(&'a self, entity: &EntityStream) -> NumberView<'a> {
        NumberView::new(self, entity)
    }

    pub fn enum_view<'a>(&'a self, entity: &EntityStream) -> EnumView<'a> {
        EnumView::new(self, entity)
    }

    pub fn text_view<'a>(&'a self, entity: &EntityStream) -> TextView<'a> {
        TextView::new(self, entity)
    }

    pub fn event_view<'a>(&'a self, entity: &EntityStream) -> Option<EventView<'a>> {
        EventView::new(self, entity)
    }
}


