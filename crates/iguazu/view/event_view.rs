use std::mem;
use crate::{schema::{EntityKind, EntityStream}, Idx, IdxRange};

use super::{IntView, ViewManager};

pub struct EventView<'v> {
    view: IntView<'v>,
    sample_rate: f64,
}

impl<'v> EventView<'v> {
    pub fn new(vm: &'v ViewManager, mut entity: &EntityStream) -> Option<Self> {
        while let Some(time_field) = entity.time() {
            entity = entity.child(&*time_field)?;
        };

        let EntityKind::Tuple { child, .. } = &entity.kind else { return None };

        let EntityKind::Timestamp { sample_rate, .. } = child.kind else { return None };

        let view = vm.int_view(child)?;

        Some(EventView { view, sample_rate })
    }

    pub fn sample_rate(&self) -> f64 { self.sample_rate }

    pub fn latest_idx(&self) -> Idx {
        self.view.get_u64(self.view.state().end)
            .map(|v| v as Idx)
            .unwrap_or(0)
    }

    /// Finds the smallest index within `bounds` whose value is equal to or greater than `search`
    fn binary_search(&self, bounds: IdxRange, search: Idx) -> Option<Idx> {
        if bounds.max <= bounds.min {
            return None;
        }

        let mut base = bounds.min;
        let mut size = bounds.max - bounds.min;

        while size > 1 {
            let step = size / 2;
            let mid = base + step;
            let val = self.view.get_u64(mid)?;
            base = if val > search { base } else { mid };
            size -= step;
        }

        let val = self.view.get_u64(base)?;

        if val >= search {
            Some(base)
        } else {
            Some(base + 1)
        }
    }

    fn binary_search_bounds(&self, val_range: IdxRange) -> Option<IdxRange> {
        let bounds = self.view.bounds();
        let min = self.binary_search(bounds, val_range.min)?;
        let max = self.binary_search(IdxRange { min, ..bounds }, val_range.max)?;
        Some(IdxRange { min, max })
    }

    fn get_pair(&self, idx: Idx) -> Option<IdxRange> {
        let min = self.view.get_u64(idx * 2)?;
        let max = self.view.get_u64(idx * 2 + 1)?.max(min);
        Some(IdxRange { min, max })
    }

    pub fn range(&self, val_range: IdxRange, min_width: u64) -> EventViewIter<'_, 'v> {
        let idx_range = self.binary_search_bounds(val_range)
            .map(|r| r.divide(2));
        EventViewIter {
            view: self,
            min_width,
            idx_range,
            val_range,
        }
    }
}

pub struct EventViewIter<'a, 'v> {
    view: &'a EventView<'v>,
    val_range: IdxRange,
    idx_range: Option<IdxRange>,
    min_width: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Event(IdxRange, Idx),
    Dense(IdxRange),
    Loading(IdxRange),
}

impl Iterator for EventViewIter<'_, '_> {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        if self.val_range.is_empty() {
            return None;
        }

        let Some(ref mut idx_range) = self.idx_range else {
            let val_range = mem::replace(&mut self.val_range, IdxRange { min: 0, max: 0 });
            return Some(Event::Loading(val_range));
        };

        if idx_range.is_empty() {
            return None;
        }

        let Some(mut evt_val_range) = self.view.get_pair(idx_range.min) else {
            let val_range = mem::replace(&mut self.val_range, IdxRange { min: 0, max: 0 });
            self.idx_range = None;
            return Some(Event::Loading(val_range));
        };

        let idx = idx_range.min;
        idx_range.min += 1;

        if evt_val_range.len() >= self.min_width {
            return Some(Event::Event(evt_val_range, idx));
        }

        while let Some(next_idx) = self.view.binary_search(idx_range.multiply(2), evt_val_range.max + self.min_width) {
            idx_range.min = next_idx / 2;
            if let Some(val) = self.view.get_pair(idx_range.min) {
                let is_near = val.min <= evt_val_range.max + self.min_width;
                let is_small = val.len() < self.min_width;
                if is_near && is_small {
                    idx_range.min += 1;
                    evt_val_range.max = val.max;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        return Some(Event::Dense(evt_val_range))
    }
}

#[test]
fn test_event_view() {
    use crate::storage::MemoryStream;
    use crate::schema::{ EntityKind, Field };

    let vm = super::ViewManager::new();

    let data = MemoryStream::new(&[
        1000u64, 1010,
        1010, 1020,
        1030, 1040,
        1100, 1200,
        4000, 4100,
    ]);

    let ts = EntityStream::new(
        EntityKind::Timestamp { data, sample_rate: 1e6 },
    );

    let tuple = EntityStream::tuple(ts, vec![
        Field { name: "start".into(), attributes: Default::default() },
        Field { name: "end".into(), attributes: Default::default() }
    ]);

    let ev = vm.event_view(&tuple).unwrap();

    assert_eq!(ev.binary_search(IdxRange { min: 0, max: 10 }, 500), Some(0));
    assert_eq!(ev.binary_search(IdxRange { min: 0, max: 10 }, 1000), Some(0));
    assert_eq!(ev.binary_search(IdxRange { min: 0, max: 10 }, 1001), Some(1));
    assert_eq!(ev.binary_search(IdxRange { min: 1, max: 10 }, 500), Some(1));
    assert_eq!(ev.binary_search(IdxRange { min: 0, max: 10 }, 9000), Some(10));
    assert_eq!(ev.binary_search(IdxRange { min: 5, max: 5 }, 500), None);

    assert_eq!(ev.range(IdxRange { min: 0, max: 100 }, 0).collect::<Vec<_>>(), vec![]);
    assert_eq!(ev.range(IdxRange { min: 3000, max: 5000 }, 0).collect::<Vec<_>>(), vec![
        Event::Event(IdxRange { min: 4000, max: 4100 }, 4)
    ]);
    assert_eq!(ev.range(IdxRange { min: 4050, max: 5000 }, 0).collect::<Vec<_>>(), vec![
        Event::Event(IdxRange { min: 4000, max: 4100 }, 4)
    ]);
    assert_eq!(ev.range(IdxRange { min: 3000, max: 4050 }, 0).collect::<Vec<_>>(), vec![
        Event::Event(IdxRange { min: 4000, max: 4100 }, 4)
    ]);
    assert_eq!(ev.range(IdxRange { min: 1000, max: 2000 }, 0).collect::<Vec<_>>(), vec![
        Event::Event(IdxRange { min: 1000, max: 1010 }, 0),
        Event::Event(IdxRange { min: 1010, max: 1020 }, 1),
        Event::Event(IdxRange { min: 1030, max: 1040 }, 2),
        Event::Event(IdxRange { min: 1100, max: 1200 }, 3),
    ]);
    assert_eq!(ev.range(IdxRange { min: 1000, max: 2000 }, 25).collect::<Vec<_>>(), vec![
        Event::Dense(IdxRange { min: 1000, max: 1040 }),
        Event::Event(IdxRange { min: 1100, max: 1200 }, 3),
    ]);
    assert_eq!(ev.range(IdxRange { min: 0, max: 5000 }, 101).collect::<Vec<_>>(), vec![
        Event::Dense(IdxRange { min: 1000, max: 1200 }),
        Event::Dense(IdxRange { min: 4000, max: 4100 }),
    ]);
}