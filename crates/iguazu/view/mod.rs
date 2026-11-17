//! Types for accessing and interpreting signal data
use std::{ sync::Arc, task::Waker};

use elsa::sync::FrozenMap;

use crate::{io::IoError, schema::{Entity, EntityStream}, stream::ArcStream, summary::{LiveSummaryMap, StoredSummary}};

mod block_view;
pub use block_view::{BlockView, LoadingState};

mod int_view;
pub use int_view:: { IntView, LoadedChunkIter };

mod number_view;
pub use number_view::NumberView;

mod timestamp_view;
pub use timestamp_view::{ TimestampView, Span };

mod event_view;
pub use event_view::{ EventView, EventViewIter, Event };

mod text_view;
pub use text_view::TextView;

mod trace_view;
pub use trace_view::{TraceView, TraceElement};

mod range_view;
pub use range_view::{ RangeView };

#[cfg(feature = "spectrogram")]
mod spectrogram_view;
#[cfg(feature = "spectrogram")]
pub use spectrogram_view::SpectrogramView;

pub struct ViewManager {
    waker: Waker,
    streams: FrozenMap<usize, Box<BlockView>>,
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
        self.streams.as_mut().retain(|_, stream| stream.end());
    }

    pub fn loading_state(&mut self) -> LoadingState {
        self.streams.as_mut().values().map(|stream| stream.loading_state()).max().unwrap_or(LoadingState::Ready)
    }

    pub fn errors(&mut self) -> Vec<Arc<IoError>> {
        self.streams.as_mut().values_mut().filter_map(|stream| stream.error()).collect()
    }
}

fn key(s: &ArcStream) -> usize {
    Arc::as_ptr(s) as *const () as usize
}

impl ViewManager {
    pub fn stream<'a>(&'a self, stream: &ArcStream) -> &'a BlockView {
        if let Some(s) = self.streams.get(&key(stream)) {
            s
        } else {
            let v = BlockView::new(stream.clone(), self.waker.clone());
            self.streams.insert(key(stream), Box::new(v))
        }
    }

    pub fn summary<T>(&self, summaries: &LiveSummaryMap, key: &str, f: impl FnMut(&ArcStream) -> T) -> StoredSummary<T> {
        if let Some(summary) = summaries.get(key) {
            summary.map(&self.waker, f)
        } else {
            StoredSummary::empty()
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

    pub fn text_view<'a>(&'a self, entity: &EntityStream) -> TextView<'a> {
        TextView::new(self, entity)
    }

    pub fn timestamp_view(&self, ts: &EntityStream) -> Option<TimestampView<'_>> {
        TimestampView::new(self, ts)
    }

    pub fn timebase<'a>(&'a self, entity: &EntityStream) -> Option<Timebase<'a>> {
        if let Some(rate) = entity.time_rate() {
            Some(Timebase::Fixed(rate))
        } else if let Some(timestamps) = self.timestamp_view(entity) {
            Some(Timebase::Nonuniform(timestamps))
        } else {
            None
        }
    }

    pub fn event_view<'a>(&'a self, entity: &EntityStream) -> Option<EventView<'a>> {
        EventView::new(self, entity)
    }
}

#[derive(Clone)]
pub enum Timebase<'a> {
    Fixed(f64),
    Nonuniform(TimestampView<'a>)
}
impl Timebase<'_> {
    pub fn uniform_sample_rate(&self) -> Option<f64> {
        match self {
            Timebase::Fixed(rate) => Some(*rate),
            Timebase::Nonuniform(_) => None,
        }
    }
}
