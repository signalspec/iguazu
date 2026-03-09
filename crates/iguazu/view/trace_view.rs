use std::num::NonZeroU64;

use crate::{Idx, IdxRange, stream::{ArcStream, BlockDesc, StreamState}, summary::{BorrowedSummary, StoredSummary}, view::ViewManager};

use super::IntView;

#[derive(Clone)]
pub struct TraceView<'a> {
    /// Highest resolution original stream
    base: IntView<'a>,
    summary: StoredSummary<IntView<'a>>,
}

impl<'a> TraceView<'a> {
    pub fn new(vm: &'a ViewManager, stream: ArcStream, summary: BorrowedSummary<'_, ArcStream>) -> Self {
        TraceView {
            base: IntView::new_from_stream(vm, &stream),
            summary: StoredSummary {
                base_level: summary.base_level,
                levels: summary.levels.iter().map(|s| IntView::new_from_stream(vm, s)).collect(),
            }
        }
    }

    pub fn desc(&self) -> &BlockDesc {
        self.base.desc()
    }

    pub fn state(&self) -> StreamState {
        self.base.state()
    }

    pub fn bounds(&self) -> IdxRange {
        IdxRange { min: 0, max: self.state().end }
    }

    pub fn scan(
        &self,
        range: IdxRange,
        mask: u64,
        min_width: NonZeroU64,
        mut f: impl FnMut(IdxRange, TraceElement)
    ) {
        let base_end = self.base.state().end;
        let mut pos = range.min;

        // Round down to the next-lower power of two
        let min_width_level = min_width.ilog2() as u8;

        let summary = self.summary.borrow();
        let min_summary_level = if summary.levels.is_empty() { 63 } else { summary.base_level.min(63) };
        let min_summary_skip = 1 << min_summary_level;

        let mut last_pos = range.min;
        let mut last_state = None;
        let mut emit = |pos: Idx, state: TraceElement| {
            if let Some(last_state) = last_state && state != last_state {
                f(IdxRange { min: last_pos, max: pos }, last_state);
                last_pos = pos;
            }
            last_state = Some(state);
        };

        let scan_end = range.max.min(base_end);

        if min_width_level >= summary.max_level() + 4 {
            // Not enough summary levels to avoid scanning too much, so wait for summary or give up
            emit(pos, TraceElement::Loading);
            pos = scan_end;
        }

        'scan: while pos < scan_end {
            let max_level = if pos == range.min {
                // On the first iteration, try all summaries
                63
            } else {
                // Skip coarse summaries that have already been tried at this position
                pos.trailing_zeros() as u8 + 1
            };

            for (level, s_view) in summary.limit_to_level(max_level).iter_levels().rev() {
                let width = 1u64 << level;
                let end = (pos & !(width - 1)).saturating_add(width);

                if end > s_view.state().end / 2 * width {
                    // This summary isn't built yet

                    if level > min_width_level || (scan_end - pos) / 16 < width {
                        // If near the end, use finer levels
                        continue;
                    } else {
                        // This would require scanning too much, so wait for summary to be built
                        emit(pos, TraceElement::Loading);
                        pos = scan_end;
                        break 'scan;
                    }
                }

                let (Some(lo), Some(hi)) = (s_view.get_u64(pos / width * 2), s_view.get_u64(pos / width * 2 + 1)) else {
                    emit(pos, TraceElement::Loading);
                    pos = end;
                    continue 'scan;
                };

                if lo & mask == hi & mask {
                    emit(pos, TraceElement::Value(lo & mask));
                    pos = end;
                    continue 'scan;
                }

                if level <= min_width_level {
                    emit(pos, TraceElement::Dense);
                    pos = end;
                    continue 'scan;
                }
            }

            let end = (pos & !(min_summary_skip - 1)).saturating_add(min_summary_skip).min(scan_end);

            while pos < end {
                if let Some(v) = self.base.get_u64(pos) {
                    emit(pos, TraceElement::Value(v & mask));
                } else {
                    emit(pos, TraceElement::Loading);
                }
                pos += 1;
            }
        }

        if let Some(last_state) = last_state {
            f(IdxRange { min: last_pos, max: pos }, last_state);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceElement {
    Value(u64),
    Dense,
    Loading
}


#[test]
fn test_traceview() {
    env_logger::builder().is_test(true).filter_module("iguazu", log::LevelFilter::Debug).try_init().ok();

    use crate::{ stream::ArcStream, storage::Storage, schema::{Entity, FieldKind, EntityStream}, storage::{MemoryStorage, MemoryStreamWriter} };
    use std::task::Waker;
    use std::sync::Arc;
    use async_executor::Executor;
    use futures_lite::future::block_on;

    let mut vm = super::ViewManager::new();
    vm.begin(&Waker::noop().clone());

    let mut writer = MemoryStreamWriter::new(crate::ElementSize::U8);
    writer.extend_from_slice(&[0b101; 50]);
    writer.extend_from_slice(&[0b100; 100]);
    writer.extend_from_slice(&[0b110; 2]);
    writer.extend_from_slice(&[0b010; 50]);
    writer.extend_from_slice(&[0b000; 4000]); // pad it such that it builds two summary levels
    writer.commit();
    let stream: ArcStream = writer.stream().clone();
    drop(writer);

    let trace_view = TraceView::new(&vm, stream.clone(), BorrowedSummary::empty());

    let mut results = Vec::new();
    trace_view.scan(IdxRange { min: 0, max: 250 },
        0b111,
        NonZeroU64::new(1).unwrap(),
        |range, elem| {
            results.push((range, elem));
        }
    );
    assert_eq!(&results[..], &[
        (IdxRange { min: 0, max: 50 }, TraceElement::Value(0b101)),
        (IdxRange { min: 50, max: 150 }, TraceElement::Value(0b100)),
        (IdxRange { min: 150, max: 152 }, TraceElement::Value(0b110)),
        (IdxRange { min: 152, max: 202 }, TraceElement::Value(0b010)),
        (IdxRange { min: 202, max: 250 }, TraceElement::Value(0b000)),
    ]);

    let executor = Arc::new(Executor::new());
    let storage = Arc::new(MemoryStorage) as Arc<dyn Storage>;
    let mut entity = EntityStream::field_data(FieldKind::Bits { bits: 8 }, stream);

    block_on(executor.run(entity.build_summaries(&executor, &storage))).unwrap();

    let Entity::Data { data, summaries, .. } = &entity else { unreachable!() };
    let summary = summaries.get("bit_and_or");
    assert_eq!(summary.levels.len(), 2);

    let trace_view = TraceView::new(&vm, data.clone(), summary);

    let mut results = Vec::new();
    trace_view.scan(IdxRange { min: 0, max: 200 },
        0b111,
        NonZeroU64::new(1).unwrap(),
        |range, elem| {
            results.push((range, elem));
        }
    );
    assert_eq!(&results[..], &[
        (IdxRange { min: 0, max: 50 }, TraceElement::Value(0b101)),
        (IdxRange { min: 50, max: 150 }, TraceElement::Value(0b100)),
        (IdxRange { min: 150, max: 152 }, TraceElement::Value(0b110)),
        (IdxRange { min: 152, max: 200 }, TraceElement::Value(0b010)),
    ]);

    let mut results = Vec::new();
    trace_view.scan(IdxRange { min: 0, max: 200 },
        0b111,
        NonZeroU64::new(8).unwrap(),
        |range, elem| {
            results.push((range, elem));
        }
    );
    assert_eq!(&results[..], &[
        (IdxRange { min: 0, max: 48 }, TraceElement::Value(0b101)),
        (IdxRange { min: 48, max: 56 }, TraceElement::Dense),
        (IdxRange { min: 56, max: 144 }, TraceElement::Value(0b100)),
        (IdxRange { min: 144, max: 152 }, TraceElement::Dense),
        (IdxRange { min: 152, max: 200 }, TraceElement::Value(0b010)),
    ]);
}

#[test]
fn test_traceview_zoom() {
    use crate::{ schema::{Entity, EntityStream, FieldKind}, storage::{MemoryStorage, MemoryStream, Storage} };
    use std::task::Waker;
    use std::sync::Arc;
    use async_executor::Executor;
    use futures_lite::future::block_on;

    env_logger::builder().is_test(true).filter_module("iguazu", log::LevelFilter::Debug).try_init().ok();

    let executor = Arc::new(Executor::new());
    let storage = Arc::new(MemoryStorage) as Arc<dyn Storage>;
    let mut vm = super::ViewManager::new();
    vm.begin(&Waker::noop().clone());

    let mut input = vec![0u8; 120_001];
    input[45678] = 1;
    input[76543..77777].fill(2);
    let stream = MemoryStream::new(&input[..]);
    let mut entity = EntityStream::field_data(FieldKind::Bits { bits: 8 }, stream);
    block_on(executor.run(entity.build_summaries(&executor, &storage))).unwrap();

    let Entity::Data { data, summaries, .. } = &entity else { unreachable!() };
    let summary = summaries.get("bit_and_or");
    assert_eq!(summary.levels.len(), (input.len() / 1024).ilog2() as usize);

    let trace_view = TraceView::new(&vm, data.clone(), summary);

    let scan = |mask, min_width, range| {
        let mut results = Vec::new();
        trace_view.scan(range,
            mask,
            NonZeroU64::new(min_width).unwrap(),
            |range, elem| {
                results.push((range.max, elem));
            }
        );
        results
    };

    use TraceElement::*;

    assert_eq!(scan(0b10, 1, IdxRange { min: 0, max: 200_000 }), vec![
        (76543, TraceElement::Value(0)),
        (77777, TraceElement::Value(2)),
        (120_001, TraceElement::Value(0)),
    ]);

    assert_eq!(scan(0b10, 64, IdxRange { min: 0, max: 200_000 }), vec![
        (76543 & !63, Value(0)),
        ((76543 & !63) + 64, Dense),
        (77777 & !63, Value(2)),
        ((77777 & !63) + 64, Dense),
        (120_001, Value(0)),
    ]);
}
