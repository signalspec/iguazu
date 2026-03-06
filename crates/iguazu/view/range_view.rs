use std::num::NonZeroU64;

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

        if log_min_width <= self.base_level as u32 {
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
}

pub enum RangeElement {
    Loading(IdxRange),
    Single(Idx, f64),
    Range(IdxRange, f64, f64),
}
