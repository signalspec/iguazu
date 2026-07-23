use std::{num::NonZeroU64};

use crate::{Idx, IdxRange, schema::FieldRef, stream::StreamState, view::{NumberView, ViewManager}};

pub struct RangeView<'a> {
    view: NumberView<'a>,

    /// Level of `summaries[0]`
    base_level: u8,

    /// Summaries from densest to coarsest
    summaries: Vec<NumberView<'a>>,
}

impl<'a> RangeView<'a> {
    pub fn new(vm: &'a ViewManager, field: FieldRef<'_>) -> Option<Self> {
        let summary = field.summaries.get("range");
        Some(RangeView {
            view: NumberView::new(vm, field.data, field.field)?,
            base_level: summary.base_level,
            summaries: summary.levels.iter().map(|s| NumberView::new(vm, s, field.field)).collect::<Option<Vec<_>>>()?,
        })
    }

    pub fn state(&self) -> StreamState {
        self.view.state()
    }

    pub fn for_each_elem(&self, range: IdxRange, min_width: NonZeroU64, mut f: impl FnMut(RangeElement)) {
        let log_min_width = min_width.ilog2();

        if log_min_width <= self.base_level as u32 || self.summaries.is_empty() {
            self.view.for_each_elem(range, |i, elem| {
                match elem {
                    Some(v) => f(RangeElement::Single(i, v)),
                    None => f(RangeElement::Loading(IdxRange { min: i, max: i + 1 })),
                }
            })
        } else {
            let level = ((log_min_width - self.base_level as u32) as usize).min(self.summaries.len() - 1);
            let l = level + self.base_level as usize;
            let summary = &self.summaries[level];
            for i in range.min >> l .. range.max >> l {
                let lo = summary.get(i * 2);
                let hi = summary.get(i * 2 + 1);
                let xrange = IdxRange { min: i << l, max: (i + 1) << l };
                match (lo, hi) {
                    (Some(lo), Some(hi)) => f(RangeElement::Range(xrange, lo, hi)),
                    _ => f(RangeElement::Loading(xrange)),
                }
            }
        }
    }

    /// Get the overall minimum and maximum.
    ///
    /// It looks at only 4096 points, so may return an under-approximation if overviews are not fully generated.
    ///
    /// Returns `None` if no values are loaded.
    pub fn bounds(&self) -> Option<(f64, f64)> {
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        if let Some(summary) = self.summaries.last() {
            for i in 0 .. (summary.state().end / 2).max(4096) {
                if let Some(v) = summary.get(i * 2) && v < min{
                    min = v;
                }
                if let Some(v) = summary.get(i * 2 + 1) && v > max {
                    max = v;
                }
            }
        } else {
            self.view.for_each_elem(IdxRange { min: 0, max: 4096 }, |_, elem| {
                if let Some(v) = elem && v < min {
                    min = v;
                }
                if let Some(v) = elem && v > max {
                    max = v;
                }
            })
        }

        (min.is_finite() && max.is_finite()).then_some((min, max))
    }
}

pub enum RangeElement {
    Loading(IdxRange),
    Single(Idx, f64),
    Range(IdxRange, f64, f64),
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

    {
        let range_view_no_summary = RangeView::new(&vm, FieldRef { data: &stream, field: &field, summaries: &LiveSummaryMap::default()}).unwrap();
        assert_eq!(range_view_no_summary.bounds(), Some((-1.0, 3.0)));
    }

    let mut entity = EntityStream::field_data(field, stream);
    block_on(executor.run(entity.build_summaries(&executor, &storage))).unwrap();

    {
        let range_view = RangeView::new(&vm, entity.as_field().unwrap()).unwrap();
        assert_eq!(range_view.bounds(), Some((-1.0, 3.0)));
    }
}
