//! Types for accessing and interpreting signal data
use std::{ sync::Arc, task::Waker};

use elsa::sync::FrozenMap;

use crate::{schema::{Entity, EntityStream}, stream::{ArcStream, StreamAccess}};

mod int_view;
pub use int_view:: { IntView, LoadedChunkIter };

mod number_view;
pub use number_view::NumberView;

mod timestamp_view;
pub use timestamp_view::TimestampView;

mod enum_view;
pub use enum_view::EnumView;

mod event_view;
pub use event_view::{ EventView, EventViewIter, Event };

mod text_view;
pub use text_view::TextView;

mod trace_view;
pub use trace_view::{TraceView, TraceElement};

mod range_view;
pub use range_view::{ RangeView, RangeElement };

pub struct ViewManager {
    waker: Waker,
    streams: FrozenMap<usize, Box<dyn StreamAccess>>,
}

impl ViewManager {
    pub fn new() -> Self {
        ViewManager {
            waker: Waker::noop().clone(),
            streams: FrozenMap::default(),
        }
    }

    pub fn begin(&mut self, waker: &Waker) {
        self.waker.clone_from(waker);
        for (_, stream) in self.streams.as_mut().iter_mut() {
            stream.begin(waker);
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
            s
        } else {
            let mut s = stream.clone().access();
            s.begin(&self.waker);
            self.streams.insert(key(stream), s)
        }
    }

    pub fn int_view<'a>(&'a self, entity: &EntityStream) -> Option<IntView<'a>> {
        IntView::new(self, entity)
    }

    pub fn number_view<'a>(&'a self, entity: &EntityStream) -> Option<NumberView<'a>> {
        if let Entity::Data { field, data, .. } = entity {
            NumberView::new(self, data, field)
        } else {
            None
        }
    }

    pub fn enum_view<'a>(&'a self, entity: &EntityStream) -> Option<EnumView<'a>> {
        EnumView::new(self, entity)
    }

    pub fn text_view<'a>(&'a self, entity: &EntityStream) -> TextView<'a> {
        TextView::new(self, entity)
    }

    pub fn timestamp_view(&self, ts: &EntityStream) -> Option<TimestampView<'_>> {
        TimestampView::new(self, ts)
    }

    pub fn event_view<'a>(&'a self, entity: &EntityStream) -> Option<EventView<'a>> {
        EventView::new(self, entity)
    }
}
