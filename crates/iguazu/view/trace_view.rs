use crate::{stream::{ArcStream, StreamDesc, StreamState}, view::ViewManager, Idx, IdxRange};

use super::IntView;

#[derive(Clone)]
pub struct TraceView<'a> {
    /// Highest resolution original stream
    base: IntView<'a>,

    /// Summaries from densest to coarsest
    summaries: Vec<IntView<'a>>,
}

impl<'a> TraceView<'a> {
    pub fn new(vm: &'a ViewManager, stream: ArcStream, summaries: &[ArcStream]) -> Self {
        TraceView { 
            base: IntView::new_from_stream(vm, &stream),
            summaries: summaries.iter().map(|s| IntView::new_from_stream(vm, s)).collect(),
        }
    }

    pub fn desc(&self) -> &StreamDesc {
        &self.base.desc()
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
        min_width: u64,
        mut f: impl FnMut(IdxRange, TraceElement)
    ) {
        let max_end = self.base.state().end;
        let mut pos = range.min;

        let mut last_pos = range.min;
        let mut last_state = None;

        let mut emit = |pos: Idx, state: TraceElement| {
            if let Some(last_state) = last_state && state != last_state {
                f(IdxRange { min: last_pos, max: pos }, last_state);
                last_pos = pos;
            }
            last_state = Some(state);
        };

        'scan: while pos < range.max.min(max_end) {
            let max_level = if pos == range.min { 
                // On the first iteration, try all summaries
                self.summaries.len()
            } else {
                // Skip summaries that have already been tried at this position
                pos.trailing_zeros().saturating_sub(1) as usize
            };

            for (level, summary) in self.summaries.iter().take(max_level).enumerate().rev() {
                let width = 1 << (level + 2);

                let end = (pos & !(width - 1)) + width;

                if end > max_end {
                    // This summary isn't built yet
                    continue;
                }

                let (Some(lo), Some(hi)) = (summary.get_u64(pos / width * 2), summary.get_u64(pos / width * 2 + 1)) else {
                    emit(pos, TraceElement::Loading);
                    pos = end;
                    continue 'scan;
                };

                if lo & mask == hi & mask {
                    emit(pos, TraceElement::Value(lo & mask));
                    pos = end;
                    continue 'scan;
                }

                if width / 2 < min_width {
                    emit(pos, TraceElement::Dense);
                    pos = end;
                    continue 'scan;
                }
            }

            let end = ((pos & !(4 - 1)) + 4).min(max_end);

            if min_width > 4 {
                let mut val = TraceElement::Loading;
                for i in pos..end {
                    if let Some(v) = self.base.get_u64(i) {
                        if let TraceElement::Value(lv) = val && v & mask != lv {
                            val = TraceElement::Dense;
                            break;
                        }
                        val = TraceElement::Value(v & mask);
                    } else {
                        val = TraceElement::Loading;
                        break;
                    }
                }
                pos = end;
                emit(pos, val);
            } else {
                for _ in pos..end {
                    if let Some(v) = self.base.get_u64(pos) {
                        emit(pos, TraceElement::Value(v & mask));
                    } else {
                        emit(pos, TraceElement::Loading);
                    }
                    pos += 1;
                }
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
    use crate::{ stream::ArcStream, storage::MemoryStreamWriter };
    use std::task::Waker;
    use async_executor::Executor;
    use futures_lite::future::block_on;

    let vm = super::ViewManager::new(Waker::noop().clone());

    let mut writer = MemoryStreamWriter::new(crate::ElementType::U8);
    writer.extend_from_slice(&[0b101; 50]);
    writer.extend_from_slice(&[0b100; 100]);
    writer.extend_from_slice(&[0b110; 2]);
    writer.extend_from_slice(&[0b010; 50]);
    let stream: ArcStream = writer.stream().clone();
    drop(writer);

    let trace_view = TraceView::new(&vm, stream.clone(), &[]);

    let mut results = Vec::new();
    trace_view.scan(IdxRange { min: 0, max: 250 },
        0b111,
        1,
        |range, elem| {
            results.push((range, elem));
        }
    );
    assert_eq!(&results[..], &[
        (IdxRange { min: 0, max: 50 }, TraceElement::Value(0b101)),
        (IdxRange { min: 50, max: 150 }, TraceElement::Value(0b100)),
        (IdxRange { min: 150, max: 152 }, TraceElement::Value(0b110)),
        (IdxRange { min: 152, max: 202 }, TraceElement::Value(0b010)),
    ]);

    let executor = Executor::new();

    let (task, summary1) = crate::summary::bit_summary1(&executor, stream.clone());
    block_on(executor.run(task)).unwrap();

    let (task, summary2) = crate::summary::bit_summary_reduce(&executor, summary1.clone());
    block_on(executor.run(task)).unwrap();

    let trace_view = TraceView::new(&vm, stream, &[summary1, summary2]);

    let mut results = Vec::new();
    trace_view.scan(IdxRange { min: 0, max: 200 },
        0b111,
        1,
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
        8,
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
