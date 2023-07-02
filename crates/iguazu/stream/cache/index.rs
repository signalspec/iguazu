use std::sync::Arc;

use crate::{ Stream, Idx, IdxRange };
use super::Cache;
pub struct IndexView {
    inner: Cache<u64>,
}

impl IndexView {
    pub fn new(stream: Arc<dyn Stream<u64>>) -> IndexView {
        IndexView {
            inner: Cache::new(stream)
        }
    }

    pub fn set_parent_range(&mut self, _range: IdxRange) {
        let block_size = self.inner.stream.block_size();
        self.inner.set_range(IdxRange { min: 0, max: block_size as u64 }); // Assume top-level overview is one block
    }

    pub fn for_each_range(&self, parent_range: IdxRange, min_duration: u64, mut f: impl FnMut(IdxRange, Event)) {
        let block = self.inner.blocks.front().map_or(&[] as &[u64], |b| b.as_slice());
        let offset = self.inner.offset * self.inner.stream.block_size() as u64;

        // Get index of the first range starting on or before `parent_range.min`
        let (mut parent_start, mut idx) = match block.binary_search(&parent_range.min) {
            Ok(i) => (block[i], i + 1),
            Err(0) => (0, 0),
            Err(i) => (block[i - 1], i),
        };

        let mut last_emitted = parent_start;

        while idx < block.len() && parent_start < parent_range.max {
            let parent_idx = block[idx];

            if parent_idx - parent_start < min_duration {
                idx += 1 + match block[idx + 1..].binary_search(&(parent_idx + min_duration)) {
                    Ok(i) => i,
                    Err(i) => i,
                };
                parent_start = block[idx - 1];
                continue;
            }

            if last_emitted < parent_start {
                f(IdxRange { min: last_emitted, max: parent_start }, Event::TooDense);
            }

            f(IdxRange { min: parent_start, max: parent_idx }, Event::Element(offset + idx as u64));

            parent_start = parent_idx;
            last_emitted = parent_idx;
            idx += 1;
        }

        if last_emitted < parent_start {
            f(IdxRange { min: last_emitted, max: parent_start }, Event::TooDense);
        }

        if parent_start < parent_range.max {
            f(IdxRange { min: parent_start, max: parent_range.max }, Event::Element(offset + idx as u64));
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Event {
    Element(Idx),
    Loading,
    TooDense,
}

#[test]
fn test_one_block() {
    use crate::in_memory::MemoryStream;
    let stream = MemoryStream::new(&[10u64, 20, 30, 35, 40, 42, 45, 50, 60]) as Arc<dyn Stream<u64>>;
    let mut view = IndexView::new(stream);
    view.set_parent_range(IdxRange { min: 0, max: 100 });

    let mut r = Vec::new();
    view.for_each_range(IdxRange { min: 0, max: 100 }, 0, |range, e| r.push((range, e)));
    assert_eq!(&r[..], &[
        (IdxRange { min: 0, max: 10 }, Event::Element(0)),
        (IdxRange { min: 10, max: 20 }, Event::Element(1)),
        (IdxRange { min: 20, max: 30 }, Event::Element(2)),
        (IdxRange { min: 30, max: 35 }, Event::Element(3)),
        (IdxRange { min: 35, max: 40 }, Event::Element(4)),
        (IdxRange { min: 40, max: 42 }, Event::Element(5)),
        (IdxRange { min: 42, max: 45 }, Event::Element(6)),
        (IdxRange { min: 45, max: 50 }, Event::Element(7)),
        (IdxRange { min: 50, max: 60 }, Event::Element(8)),
        (IdxRange { min: 60, max: 100 }, Event::Element(9)),
    ]);

    let mut r = Vec::new();
    view.for_each_range(IdxRange { min: 0, max: 100 }, 2, |range, e| r.push((range, e)));
    assert_eq!(&r[..], &[
        (IdxRange { min: 0, max: 10 }, Event::Element(0)),
        (IdxRange { min: 10, max: 20 }, Event::Element(1)),
        (IdxRange { min: 20, max: 30 }, Event::Element(2)),
        (IdxRange { min: 30, max: 35 }, Event::Element(3)),
        (IdxRange { min: 35, max: 40 }, Event::Element(4)),
        (IdxRange { min: 40, max: 42 }, Event::Element(5)),
        (IdxRange { min: 42, max: 45 }, Event::Element(6)),
        (IdxRange { min: 45, max: 50 }, Event::Element(7)),
        (IdxRange { min: 50, max: 60 }, Event::Element(8)),
        (IdxRange { min: 60, max: 100 }, Event::Element(9)),
    ]);

    let mut r = Vec::new();
    view.for_each_range(IdxRange { min: 15, max: 32 }, 2, |range, e| r.push((range, e)));
    assert_eq!(&r[..], &[
        (IdxRange { min: 10, max: 20 }, Event::Element(1)),
        (IdxRange { min: 20, max: 30 }, Event::Element(2)),
        (IdxRange { min: 30, max: 35 }, Event::Element(3)),
    ]);

    let mut r = Vec::new();
    view.for_each_range(IdxRange { min: 0, max: 100 }, 3, |range, e| r.push((range, e)));
    assert_eq!(&r[..], &[
        (IdxRange { min: 0, max: 10 }, Event::Element(0)),
        (IdxRange { min: 10, max: 20 }, Event::Element(1)),
        (IdxRange { min: 20, max: 30 }, Event::Element(2)),
        (IdxRange { min: 30, max: 35 }, Event::Element(3)),
        (IdxRange { min: 35, max: 40 }, Event::Element(4)),
        (IdxRange { min: 40, max: 42 }, Event::TooDense),
        (IdxRange { min: 42, max: 45 }, Event::Element(6)),
        (IdxRange { min: 45, max: 50 }, Event::Element(7)),
        (IdxRange { min: 50, max: 60 }, Event::Element(8)),
        (IdxRange { min: 60, max: 100 }, Event::Element(9)),

    ]);

    let mut r = Vec::new();
    view.for_each_range(IdxRange { min: 0, max: 100 }, 5, |range, e| r.push((range, e)));
    assert_eq!(&r[..], &[
        (IdxRange { min: 0, max: 10 }, Event::Element(0)),
        (IdxRange { min: 10, max: 20 }, Event::Element(1)),
        (IdxRange { min: 20, max: 30 }, Event::Element(2)),
        (IdxRange { min: 30, max: 35 }, Event::Element(3)),
        (IdxRange { min: 35, max: 40 }, Event::Element(4)),
        (IdxRange { min: 40, max: 45 }, Event::TooDense),
        (IdxRange { min: 45, max: 50 }, Event::Element(7)),
        (IdxRange { min: 50, max: 60 }, Event::Element(8)),
        (IdxRange { min: 60, max: 100 }, Event::Element(9)),
    ]);

    let mut r = Vec::new();
    view.for_each_range(IdxRange { min: 0, max: 100 }, 10, |range, e| r.push((range, e)));
    assert_eq!(&r[..], &[
        (IdxRange { min: 0, max: 10 }, Event::Element(0)),
        (IdxRange { min: 10, max: 20 }, Event::Element(1)),
        (IdxRange { min: 20, max: 30 }, Event::Element(2)),
        (IdxRange { min: 30, max: 50 }, Event::TooDense),
        (IdxRange { min: 50, max: 60 }, Event::Element(8)),
        (IdxRange { min: 60, max: 100 }, Event::Element(9)),
    ]);

    let mut r = Vec::new();
    view.for_each_range(IdxRange { min: 35, max: 48 }, 10, |range, e| r.push((range, e)));
    assert_eq!(&r[..], &[
        (IdxRange { min: 35, max: 50 }, Event::TooDense),
    ]);

    let mut r = Vec::new();
    view.for_each_range(IdxRange { min: 0, max: 100 }, 11, |range, e| r.push((range, e)));
    assert_eq!(&r[..], &[
        (IdxRange { min: 0, max: 60 }, Event::TooDense),
        (IdxRange { min: 60, max: 100 }, Event::Element(9)),
    ]);
}
