use std::{ sync::Arc, task::Waker};

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

pub struct ViewManager {
    waker: Waker,
    streams: FrozenMap<usize, Box<dyn StreamAccess>>,
}

impl ViewManager {
    pub fn new(waker: Waker) -> Self {
        ViewManager {
            waker,
            streams: FrozenMap::default(),
        }
    }

    pub fn begin(&mut self) {
        for (_, stream) in self.streams.as_mut().iter_mut() {
            stream.begin(&self.waker);
        }
    }

    pub fn end(&mut self) {
        for (_, stream) in self.streams.as_mut().iter_mut() {
            stream.end();
        }
        // TODO: clear stale streams
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
    
    pub fn int_view<'a>(&'a self, entity: &EntityStream) -> Option<IntView<'a>> {
        IntView::new(self, entity)
    }

    pub fn number_view<'a>(&'a self, entity: &EntityStream) -> Option<NumberView<'a>> {
        NumberView::new(self, entity)
    }

    pub fn enum_view<'a>(&'a self, entity: &EntityStream) -> Option<EnumView<'a>> {
        EnumView::new(self, entity)
    }

    pub fn text_view<'a>(&'a self, entity: &EntityStream) -> TextView<'a> {
        TextView::new(self, entity)
    }

    pub fn event_view<'a>(&'a self, entity: &EntityStream) -> Option<EventView<'a>> {
        EventView::new(self, entity)
    }
}


