use itertools::Itertools;

use crate::{Idx, IdxRange, schema::FieldRef, stream::StreamState, summary::{StoredSummary, SummaryLevel}, view::{NumberView, ViewManager}};

pub struct RangeView<'a> {
    view: NumberView<'a>,
    summary: StoredSummary<NumberView<'a>>,
}

impl<'a> RangeView<'a> {
    pub fn new(vm: &'a ViewManager, field: FieldRef<'_>) -> Option<Self> {
        let view = NumberView::new(vm, field.data, field.field)?;
        let summary = field.summaries.get("range").map(|s| NumberView::new_like(vm, s, &view));
        Some(RangeView { view, summary })
    }

    pub fn state(&self) -> StreamState {
        self.view.state()
    }

    pub fn len(&self) -> Idx {
        self.view.len()
    }

    pub fn get_base(&self, idx: Idx) -> Option<f64> {
        self.view.get(idx)
    }

    pub fn iter_base(&self, range: IdxRange) -> impl Iterator<Item = Option<f64>> {
        self.view.iter(range)
    }

    pub fn get_at_level(&self, level: u8, idx: Idx) -> Option<(f64, f64)> {
        match self.summary.borrow().get(level) {
            SummaryLevel::Base => {
                value_iter_min_max(self.iter_base(IdxRange { min: idx, max: idx + (1 << level) }))
            }
            SummaryLevel::Level(s) => {
                Some((s.get((idx >> level) * 2)?, s.get((idx >> level) * 2 + 1)?))
            }
            SummaryLevel::Above { last_level, last_view, above } => {
                let i = idx >> last_level;
                min_max_iter_min_max(last_view.iter(IdxRange { min: i * 2, max: (i + (1 << above)) * 2 }))
            },
        }
    }

    /// Get the overall minimum and maximum.
    ///
    /// It looks at only 4096 points, so may return an under-approximation if overviews are not fully generated.
    ///
    /// Returns `None` if no values are loaded.
    pub fn bounds(&self) -> Option<(f64, f64)> {
        let (min, max) = if let Some((_, summary)) = self.summary.borrow().last() {
            min_max_iter_min_max(summary.iter(IdxRange { min: 0, max: 4096.min(summary.len()) }))?
        } else {
            value_iter_min_max(self.iter_base(IdxRange { min: 0, max: 4096.min(self.len()) }))?
        };

        (min.is_finite() && max.is_finite()).then_some((min, max))
    }
}

fn value_iter_min_max(iter: impl Iterator<Item = Option<f64>>) -> Option<(f64, f64)> {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for v in iter {
        min = f64::min(min, v?);
        max = f64::max(max, v?);
    }
    Some((min, max))
}

fn min_max_iter_min_max(mut iter: impl Iterator<Item = Option<f64>>) -> Option<(f64, f64)> {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    while let Some([lo, hi]) = iter.next_array() {
        min = f64::min(min, lo?);
        max = f64::max(max, hi?);
    }
    Some((min, max))
}

#[test]
fn test_range_view() {
    env_logger::builder().is_test(true).filter_module("iguazu", log::LevelFilter::Debug).try_init().ok();

    use crate::{ schema::{EntityStream, Field}, summary::LiveSummaryMap, storage::{MemoryStorage, MemoryStreamWriter, Storage}, stream::ArcStream };
    use std::task::Waker;
    use std::sync::Arc;
    use async_executor::Executor;
    use futures_lite::future::block_on;

    let executor = Arc::new(Executor::new());
    let storage = Arc::new(MemoryStorage) as Arc<dyn Storage>;

    let mut vm = super::ViewManager::new();
    vm.begin(&Waker::noop().clone());

    let mut writer = MemoryStreamWriter::new(crate::ElementSize::U32);
    for i in 0..10000 {
        writer.extend_from_slice(&(((i as f32) / 1000.0 * std::f32::consts::PI).sin() * 2.0 + 1.0).to_le_bytes());
    }
    writer.commit();
    let stream: ArcStream = writer.stream().clone();
    drop(writer);

    let field = Field::float(32).unwrap();

    fn check_get_at_level(range_view: &RangeView<'_>) {
        let v0 = range_view.get_base(0).unwrap();
        let v1 = range_view.get_base(1).unwrap();
        let v2 = range_view.get_base(2).unwrap();
        let v3 = range_view.get_base(3).unwrap();

        assert_eq!(range_view.get_at_level(0, 0), Some((v0, v0)));
        assert_eq!(range_view.get_at_level(1, 0), Some((v0.min(v1), v0.max(v1))));
        assert_eq!(range_view.get_at_level(1, 2), Some((v2.min(v3), v2.max(v3))));
        assert_eq!(range_view.get_at_level(2, 0), Some((v0.min(v1).min(v2).min(v3), v0.max(v1).max(v2).max(v3))));
        assert_eq!(range_view.get_at_level(13, 0), Some((-1.0, 3.0)));
        assert_eq!(range_view.get_at_level(0, 10000), None);
    }

    {
        let range_view_no_summary = RangeView::new(&vm, FieldRef { data: &stream, field: &field, summaries: &LiveSummaryMap::default()}).unwrap();
        assert_eq!(range_view_no_summary.bounds(), Some((-1.0, 3.0)));
        check_get_at_level(&range_view_no_summary);
    }

    let mut entity = EntityStream::field_data(field, stream);
    block_on(executor.run(entity.build_summaries(&executor, &storage))).unwrap();

    {
        let range_view = RangeView::new(&vm, entity.as_field().unwrap()).unwrap();
        assert_eq!(range_view.bounds(), Some((-1.0, 3.0)));
        check_get_at_level(&range_view);

    }
}
