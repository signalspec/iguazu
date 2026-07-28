use std::range::Range;
use crate::{Idx, schema::{Entity, EntityStream, Field, FieldKind}};

use super::{ViewManager, TimestampView, timestamp_view::SkipResult};

pub struct EventView<'v> {
    inner: TimestampView<'v>,
}

impl<'v> EventView<'v> {
    pub fn new(vm: &'v ViewManager, mut entity: &EntityStream) -> Option<Self> {
        while let Some(time_field) = entity.time_field() {
            entity = entity.child(&time_field)?;
        };

        let Entity::Tuple { child, .. } = &entity else { return None };

        let Entity::Data { field: field @ Field { kind: FieldKind::Timestamp, .. }, data, summaries } = &**child else {
            return None;
        };

        let time_rate = field.time_rate()?;

        let inner = TimestampView::new_from_stream(vm, time_rate, data, summaries)?;

        Some(EventView { inner })
    }

    pub fn time_rate(&self) -> f64 { self.inner.time_rate() }

    pub fn latest_timestamp(&self) -> Option<u64> {
        self.inner.latest_timestamp()
    }

    pub fn range<'a>(&'a self, time_range: Range<u64>, min_width: u64) -> EventViewIter<'a, 'v> {
        let Range { start: time_start, end: time_end } = time_range;

        EventViewIter {
            view: &self.inner,
            next_idx: 0,
            time_start,
            time_end,
            min_width,
        }
    }
}

pub struct EventViewIter<'a, 'v> {
    view: &'a TimestampView<'v>,
    next_idx: Idx,
    time_start: u64,
    time_end: u64,
    min_width: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Event(Range<u64>, Idx),
    Dense(Range<u64>),
    Loading(Range<u64>),
}

impl Iterator for EventViewIter<'_, '_> {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        let mut first = false;
        if self.next_idx == 0 {
            if let Some(first_time) = self.view.first_timestamp() && self.time_start <= first_time {
                self.time_start = first_time;
            } else {
                first = true;
            }
        };

        let mut time = self.time_start;

        #[derive(Debug, Clone, Copy)]
        enum Pending {
            None,
            Dense,
        }

        let mut pending = Pending::None;

        while time < self.time_end {
            let idx = self.next_idx;
            let skip = self.view.skip(idx, time + if first { 0 } else { self.min_width });
            match (skip, pending) {
                (SkipResult::Dense(_level, next_idx, t), _) if first => {
                    // We're skipping towards the first event in range.
                    self.next_idx = next_idx;

                    if self.time_start.saturating_sub(t) < self.min_width {
                        time = t;
                        first = false;
                    }
                }
                (SkipResult::Dense(_level, next_idx, t), Pending::None) if idx % 2 == 1 && next_idx - idx == 1 => {
                    // This is a single gap between two events. Advance past it without emitting anything.
                    time = t;
                    self.next_idx = next_idx;
                    self.time_start = t;
                }
                (SkipResult::Dense(_level, next_idx, t), Pending::None | Pending::Dense) => {
                    // In a dense range. `self.start_time` is the start time, but `time` advances.
                    pending = Pending::Dense;
                    time = t;
                    self.next_idx = next_idx;
                }
                (_, Pending::Dense) => {
                    break;
                }
                (SkipResult::Loading(_level), Pending::None) => {
                    // TODO: find next loaded value to skip to
                    self.time_start = self.time_end;
                    return Some(Event::Loading(Range { start: time, end: self.time_end }));
                }
                // Sparse means idx -> idx+1 is wider than min_width
                (SkipResult::Sparse(t), Pending::None) if idx % 2 == 0 => {
                    // if idx is even, `t` is the end of an event
                    self.time_start = t;
                    self.next_idx = idx + 1;
                    // `time` should be the same except for an event that starts before
                    // `time_start`, where we need to get the real start time
                    let start = self.view.get_base(idx).unwrap_or(time);
                    return Some(Event::Event(Range { start, end: t }, idx / 2));
                }
                (SkipResult::Sparse(t), Pending::None) => {
                    // if idx is odd, `t` is the end of a gap
                    first = false;
                    time = t;
                    self.time_start = t;
                    self.next_idx = idx + 1;
                }
                (SkipResult::End, Pending::None) => {
                    return None;
                }
            }
        }

        match pending {
            Pending::None => {
                None
            }
            Pending::Dense => {
                let start = self.time_start;
                self.time_start = time;
                Some(Event::Dense(Range { start, end: time }))
            }
        }
    }
}

#[test]
fn test_event_view() {
    use crate::storage::MemoryStream;
    use crate::schema:: FieldKind;
    use std::task::Waker;
    use indexmap::indexmap;

    let mut vm = super::ViewManager::new();
    vm.begin(&Waker::noop().clone());

    let data = MemoryStream::new(&[
        1000u64, 1010,
        1010, 1025,
        1030, 1040,
        1100, 1200,
        4000, 4100,
    ]);

    let ts = EntityStream::field_data(
        FieldKind::Timestamp, data
    ).with_attribute(crate::schema::attribute::core::TIME_RATE, 1e6);

    let tuple = EntityStream::tuple(ts, indexmap![
        "start".into() => Default::default(),
        "end".into() => Default::default(),
    ]);

    let ev = vm.event_view(&tuple).unwrap();

    assert_eq!(ev.range(Range { start: 0, end: 100 }, 0).collect::<Vec<_>>(), vec![]);
    assert_eq!(ev.range(Range { start: 3000, end: 5000 }, 0).collect::<Vec<_>>(), vec![
        Event::Event(Range { start: 4000, end: 4100 }, 4)
    ]);
    assert_eq!(ev.range(Range { start: 4050, end: 5000 }, 0).collect::<Vec<_>>(), vec![
        Event::Event(Range { start: 4000, end: 4100 }, 4)
    ]);
    assert_eq!(ev.range(Range { start: 4020, end: 4070 }, 0).collect::<Vec<_>>(), vec![
        Event::Event(Range { start: 4000, end: 4100 }, 4)
    ]);
    assert_eq!(ev.range(Range { start: 3000, end: 4050 }, 0).collect::<Vec<_>>(), vec![
        Event::Event(Range { start: 4000, end: 4100 }, 4)
    ]);
    assert_eq!(ev.range(Range { start: 1000, end: 2000 }, 0).collect::<Vec<_>>(), vec![
        Event::Event(Range { start: 1000, end: 1010 }, 0),
        Event::Event(Range { start: 1010, end: 1025 }, 1),
        Event::Event(Range { start: 1030, end: 1040 }, 2),
        Event::Event(Range { start: 1100, end: 1200 }, 3),
    ]);
    assert_eq!(ev.range(Range { start: 1000, end: 2000 }, 25).collect::<Vec<_>>(), vec![
        Event::Dense(Range { start: 1000, end: 1040 }),
        Event::Event(Range { start: 1100, end: 1200 }, 3),
    ]);
    assert_eq!(ev.range(Range { start: 0, end: 5000 }, 101).collect::<Vec<_>>(), vec![
        Event::Dense(Range { start: 1000, end: 1200 }),
        Event::Dense(Range { start: 4000, end: 4100 }),
    ]);
    assert_eq!(ev.range(Range { start: 1005, end: 1050 }, 5).collect::<Vec<_>>(), vec![
        Event::Event(Range { start: 1000, end: 1010 }, 0),
        Event::Event(Range { start: 1010, end: 1025 }, 1),
        Event::Event(Range { start: 1030, end: 1040 }, 2),
    ]);
    assert_eq!(ev.range(Range { start: 1000, end: 1200 }, 101).collect::<Vec<_>>(), vec![
        Event::Dense(Range { start: 1000, end: 1200 }),
    ]);
    assert_eq!(ev.range(Range { start: 2000, end: 5000 }, 101).collect::<Vec<_>>(), vec![
        Event::Dense(Range { start: 4000, end: 4100 }),
    ]);

    assert_eq!(ev.range(Range { start: 0, end: 800 }, 10).collect::<Vec<_>>(), vec![]);

    assert_eq!(ev.range(Range { start: 3000, end: 3999 }, 10).collect::<Vec<_>>(), vec![]);

    assert_eq!(ev.range(Range { start: 5000, end: 6000 }, 10).collect::<Vec<_>>(), vec![]);
}
