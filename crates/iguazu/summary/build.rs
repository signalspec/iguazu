use std::{future::poll_fn, ops::{BitAnd, BitOr}, sync::Arc, task::Poll};

use async_executor::Executor;
use ecow::EcoString;
use futures_lite::ready;
use indexmap::map::Entry;
use num_traits::Float;
use once_array::OnceArrayWriter;
use crate::{Element, ElementSize, schema::{Entity, EntityStream, Field, FieldKind}, storage::Storage, stream::{ArcStream, StreamIter, StreamWriter}, summary::{LiveSummary, LiveSummaryMap}, util::task_set::{TaskSet, TaskSetBuilder}};

impl EntityStream {
    pub fn build_summaries(&mut self, executor: &Arc<Executor<'static>>, storage: &Arc<dyn Storage>) -> TaskSet<String> {
        fn inner(this: &mut EntityStream, tasks: &mut TaskSetBuilder<String>, storage: &Arc<dyn Storage>) {
            match *this {
                Entity::Group { ref mut children, .. } => {
                    for child in children.values_mut() {
                        inner(child, tasks, storage);
                    }
                }
                Entity::FixedArray { ref mut child, .. } | Entity::Tuple { ref mut child, .. } => {
                    inner(child, tasks, storage);
                }
                Entity::VariableArray { ref mut child, .. } => {
                    inner(child, tasks, storage);
                }
                Entity::Data { ref mut summaries, ref field, ref data } => {
                    match field.kind {
                        FieldKind::Bits { .. } | FieldKind::BitStruct { .. } => {
                            make_summary(tasks, storage, data, field, summaries, "bit_and_or", 2, bit_summary);
                        }
                        FieldKind::Int { pos: 0, .. } | FieldKind::Signed { pos: 0, .. } | FieldKind::Float32 { pos: 0, .. } | FieldKind::Float64 => {
                            make_summary(tasks, storage, data, field, summaries, "range", 2, range_summary);
                        }
                        FieldKind::Timestamp => {
                            make_summary(tasks, storage, data, field, summaries, "skip", 3, skip_summary);
                        }
                        _ => { }
                    }
                }
            }
        }

        let mut tasks = TaskSetBuilder::new(executor.clone());
        inner(self, &mut tasks, storage);
        tasks.task_set
    }
}

/// When the initial stream or prior summary reaches this number of elements, a new summary level is created.
const MIN_SUMMARY_SIZE: u64 = 1024;


/// Check if a summary needs to be built, and use the provided function to build it.
///
/// The convoluted nested calls are necessary because the types differ between element sizes, etc., and therefore
/// we can only call downward, and can't return a common type until the type is erased when spawning the task.
fn make_summary(
    tasks: &mut TaskSetBuilder<String>,
    storage: &Arc<dyn Storage>,
    stream: &ArcStream,
    field: &Field,
    summaries: &mut LiveSummaryMap,
    key: &'static str,
    initial_level: u8,
    func: impl FnOnce(SummaryGen, &Field)
) {
    let Entry::Vacant(entry) = summaries.0.entry(key.into()) else { return; };

    let state = stream.state();
    let capacity = if state.streaming {
        // The stream is still growing. Assume it will reach 2^64 elements because we can't know when it will end.
        (u64::BITS as u8).saturating_sub(MIN_SUMMARY_SIZE.ilog2() as u8).saturating_sub(initial_level)
    } else if state.end >= MIN_SUMMARY_SIZE {
        (state.end.checked_ilog2().unwrap_or(0) as u8 + 1).saturating_sub(initial_level).saturating_sub(MIN_SUMMARY_SIZE.ilog2() as u8) + 1
    } else { 0 };

    if capacity > 0 {
        func(SummaryGen { tasks, storage: storage.clone(), stream: stream.clone(), initial_level, capacity, entry, key }, field)
    }
}

/// State prepared to build a summary.
struct SummaryGen<'a> {
    tasks: &'a mut TaskSetBuilder<String>,
    storage: Arc<dyn Storage>,
    initial_level: u8,
    capacity: u8,
    stream: ArcStream,
    entry: indexmap::map::VacantEntry<'a, EcoString, LiveSummary>,
    key: &'static str,
}

impl<'a> SummaryGen<'a> {
    pub fn build<T: Element + Default, const N1: usize, const R1: usize, const N2: usize, const R2: usize>(
        self,
        f_initial: impl FnMut([T; N1]) -> [T; R1] + Copy + Send + Sync + 'static,
        f_reduce: impl FnMut([T; N2]) -> [T; R2] + Copy + Send + Sync + 'static,
    ) {
        log::debug!("Spawning summary task for {}", self.key);
        let SummaryGen { tasks, storage, stream, entry, initial_level, capacity, key, .. } = self;
        let (summary, writer) = LiveSummary::with_capacity(initial_level, capacity as usize);
        entry.insert(summary);
        tasks.spawn(make_summary_initial(tasks.executor.clone(), storage.clone(), stream, key, initial_level, writer, f_initial, f_reduce))
    }
}

/// Build the first level of the summary, and when it reaches `MIN_SUMMARY_SIZE`, spawn a task to build the next level.
async fn make_summary_initial<T: Element + Default, T2: Element + Default, const N1: usize, const R1: usize, const N2: usize, const R2: usize>(
    executor: Arc<Executor<'static>>,
    storage: Arc<dyn Storage>,
    base: ArcStream,
    key: &'static str,
    level: u8,
    mut summaries: OnceArrayWriter<ArcStream>,
    f_initial: impl FnMut([T; N1]) -> [T2; R1] + Copy + Send + Sync + 'static,
    f_reduce: impl FnMut([T2; N2]) -> [T2; R2] + Copy + Send + Sync + 'static,
) -> Result<(), String> {
    let mut src_iter = base.iter().await.map_err(|e| e.to_string())?;

    // Do nothing until the underlying stream reaches `MIN_SUMMARY_SIZE`.
    poll_fn(|cx| -> Poll<Result<(), String>> {
        ready!(src_iter.poll_next(cx).at_least(MIN_SUMMARY_SIZE as usize)).map_err(|e| e.to_string())?;
        Poll::Ready(Ok(()))
   }).await?;

    let mut output = storage.create_stream(T::ELEMENT_SIZE);
    summaries.try_push(output.stream()).map_err(|_| "Summary capacity exceeded".to_string())?;
    summaries.commit();

    log::info!("Building {key} summary level {level}");

    // taken when starting next level
    let mut summaries = Some(summaries);
    let mut next = None;

    poll_fn(|cx| {
        let r = poll_process_summary(cx, &mut src_iter, &mut output, f_initial);

        if output.pos() >= MIN_SUMMARY_SIZE * (R2 as u64) && let Some(summaries) = summaries.take() {
            next = Some(executor.spawn(make_next_summary_level(executor.clone(), storage.clone(), output.stream(), key, level+1, summaries, f_reduce)));
        }

        r
    }).await?;

    log::info!("Completed {key} summary level {level}, len {}", output.pos() / (R2 as u64));
    drop(output);

    if let Some(next) = next {
        next.await
    } else {
        log::info!("Completed {key} summary");
        Ok(())
    }
}

/// Build a subsequent level of summary.
fn make_next_summary_level<T: Element + Default, const N: usize, const R: usize>(
    executor: Arc<Executor<'static>>,
    storage: Arc<dyn Storage>,
    src: ArcStream,
    key: &'static str,
    level: u8,
    mut summaries: OnceArrayWriter<ArcStream>,
    f: impl FnMut([T; N]) -> [T; R] + Copy + Send + Sync + 'static
) -> impl Future<Output = Result<(), String>> + Send + 'static { async move {
    let mut src_iter = src.iter().await.map_err(|e| e.to_string())?;

    let mut output = storage.create_stream(T::ELEMENT_SIZE);
    summaries.try_push(output.stream()).map_err(|_| "Summary capacity exceeded".to_string())?;
    summaries.commit();

    log::info!("Building {key} summary level {level}");

    // taken when starting next level
    let mut summaries = Some(summaries);
    let mut next = None;

    poll_fn(|cx| {
        let r = poll_process_summary(cx, &mut src_iter, &mut output, f);

        if output.pos() >= MIN_SUMMARY_SIZE * (R as u64) && let Some(summaries) = summaries.take() {
            next = Some(executor.spawn(make_next_summary_level(executor.clone(), storage.clone(), output.stream(), key, level+1, summaries, f)));
        }

        r
    }).await?;

    log::info!("Completed {key} summary level {level}, len {}", output.pos() / (R as u64));
    drop(output);

    if let Some(next) = next {
        next.await
    } else {
        log::info!("Completed {key} summary last level");
        Ok(())
    }
}}

fn poll_process_summary<T: Element + Default, T2: Element + Default, const N: usize, const R: usize>(
    cx: &mut std::task::Context<'_>,
    src_iter: &mut Box<dyn StreamIter>,
    output: &mut Box<dyn StreamWriter>,
    mut f: impl FnMut([T; N]) -> [T2; R] + Copy + Send + Sync + 'static,
) -> Poll<Result<(), String>> {
    loop {
        let r = ready!(src_iter.poll_next(cx).at_least(N * size_of::<T>()));
        let out_buf = ready!(output.poll_buf(cx))?;
        match r {
            Err(e) => return Poll::Ready(Err(e.to_string())),
            Ok(mut src) if src.len() > N * size_of::<T>() => {
                let mut consumed = 0;
                while out_buf.remaining_capacity() > 0 && let Some(copy) = src.split_off(..N * size_of::<T>()) {
                    let mut buffer = [T::default(); N];
                    bytemuck::cast_slice_mut(&mut buffer).copy_from_slice(copy);
                    consumed += N;
                    let r = f(buffer);
                    out_buf.extend_from_slice(bytemuck::cast_slice(&r[..]));
                }
                output.commit();
                src_iter.consume(consumed);
            }
            Ok(_) => {
                return Poll::Ready(Ok(()));
            }
        }
    }
}


fn bit_summary(s: SummaryGen, _field: &Field) {
    fn bit_and_or_initial<T: Copy + BitOr<Output = T> + BitAnd<Output = T>>([a, b, c, d]: [T; 4]) -> [T; 2] {
        [a & b & c & d, a | b | c | d]
    }

    fn bit_and_or_reduce<T: Copy + BitOr<Output = T> + BitAnd<Output = T>>([min1, max1, min2, max2]: [T; 4]) -> [T; 2] {
        [min1 & min2, max1 | max2]
    }

    match s.stream.desc().element_size {
        ElementSize::U8 => s.build(bit_and_or_initial::<u8>, bit_and_or_reduce::<u8>),
        ElementSize::U16 => s.build(bit_and_or_initial::<u16>, bit_and_or_reduce::<u16>),
        ElementSize::U32 => s.build(bit_and_or_initial::<u32>, bit_and_or_reduce::<u32>),
        ElementSize::U64 => s.build(bit_and_or_initial::<u64>, bit_and_or_reduce::<u64>),
    }
}


fn range_summary(s: SummaryGen, field: &Field) {
    fn min_max_initial<T: Copy + Ord>([a, b, c, d]: [T; 4]) -> [T; 2] {
        [a.min(b).min(c).min(d), a.max(b).max(c).max(d)]
    }

    fn min_max_initial_float<T: Copy + Float>([a, b, c, d]: [T; 4]) -> [T; 2] {
        [a.min(b).min(c).min(d), a.max(b).max(c).max(d)]
    }

    fn min_max_reduce<T: Copy + Ord>([min1, max1, min2, max2]: [T; 4]) -> [T; 2] {
        [min1.min(min2), max1.max(max2)]
    }

    fn min_max_reduce_float<T: Copy + Float>([min1, max1, min2, max2]: [T; 4]) -> [T; 2] {
        [min1.min(min2), max1.max(max2)]
    }

    match (&field.kind, s.stream.desc().element_size) {
        (FieldKind::Int { pos: 0, .. }, ElementSize::U8) => s.build(min_max_initial::<u8>, min_max_reduce::<u8>),
        (FieldKind::Int { pos: 0, .. }, ElementSize::U16) => s.build(min_max_initial::<u16>, min_max_reduce::<u16>),
        (FieldKind::Int { pos: 0, .. }, ElementSize::U32) => s.build(min_max_initial::<u32>, min_max_reduce::<u32>),
        (FieldKind::Int { pos: 0, .. }, ElementSize::U64) => s.build(min_max_initial::<u64>, min_max_reduce::<u64>),
        (FieldKind::Signed { pos: 0, bits: 8 }, ElementSize::U8) => s.build(min_max_initial::<i8>, min_max_reduce::<i8>),
        (FieldKind::Signed { pos: 0, bits: 16 }, ElementSize::U16) => s.build(min_max_initial::<i16>, min_max_reduce::<i16>),
        (FieldKind::Signed { pos: 0, bits: 32 }, ElementSize::U32) => s.build(min_max_initial::<i32>, min_max_reduce::<i32>),
        (FieldKind::Signed { pos: 0, bits: 64 }, ElementSize::U64) => s.build(min_max_initial::<i64>, min_max_reduce::<i64>),
        (FieldKind::Float32 { pos: 0 }, ElementSize::U32) => s.build(min_max_initial_float::<f32>, min_max_reduce_float::<f32>),
        (FieldKind::Float64, ElementSize::U64) => s.build(min_max_initial_float::<f64>, min_max_reduce_float::<f64>),
        _ => {}
    }
}

fn skip_summary(s: SummaryGen, _field: &Field) {
    fn skip_initial<T: Copy + BitOr<Output = T> + BitAnd<Output = T>>(arr: [T; 8]) -> [T; 1] {
        [arr[0]]
    }

    fn skip_reduce<T: Copy + BitOr<Output = T> + BitAnd<Output = T>>(arr: [T; 2]) -> [T; 1] {
        [arr[0]]
    }

    match s.stream.desc().element_size {
        ElementSize::U8 => s.build(skip_initial::<u8>, skip_reduce::<u8>),
        ElementSize::U16 => s.build(skip_initial::<u16>, skip_reduce::<u16>),
        ElementSize::U32 => s.build(skip_initial::<u32>, skip_reduce::<u32>),
        ElementSize::U64 => s.build(skip_initial::<u64>, skip_reduce::<u64>),
    }
}

#[test]
fn test_build_summary_live() {
    env_logger::builder().is_test(true).filter_module("iguazu", log::LevelFilter::Debug).try_init().ok();

    use futures_lite::future::block_on;
    use crate::{ storage::{MemoryStorage, MemoryStreamWriter}, stream::StreamState, schema::FieldKind };

    let executor = Arc::new(Executor::new());
    let storage = Arc::new(MemoryStorage) as Arc<dyn Storage>;

    let mut writer = MemoryStreamWriter::new(crate::ElementSize::U8);
    let stream: ArcStream = writer.stream().clone();
    let mut entity = EntityStream::field_data(FieldKind::Bits { pos: 0, bits: 8 }, stream);

    let summary_tasks = entity.build_summaries(&executor, &storage);
    assert_eq!(summary_tasks.len(), 1);

    let Entity::Data { summaries, .. } = &entity else { unreachable!() };
    let summary = summaries.get("bit_and_or");

    assert_eq!(summary.first_level, 2);
    assert_eq!(summary.levels.len(), 0);

    writer.extend_from_slice(&[0xFF; 600]);
    writer.commit();

    while executor.try_tick() { }
    let summary = summaries.get("bit_and_or");
    assert_eq!(summary.levels.len(), 0);

    writer.extend_from_slice(&[0xFF; 600]);
    writer.commit();

    while executor.try_tick() { }
    let summary = summaries.get("bit_and_or");
    assert_eq!(summary.levels.len(), 1);
    assert_eq!(summary.levels[0].state(), StreamState { end: 1200 / 4 * 2, streaming: true});

    writer.extend_from_slice(&[0xFF; 3800]);
    writer.commit();
    while executor.try_tick() { }
    let summary = summaries.get("bit_and_or");
    assert_eq!(summary.levels.len(), 2);
    assert_eq!(summary.levels[0].state(), StreamState { end: 5000 / 4 * 2, streaming: true});
    assert_eq!(summary.levels[1].state(), StreamState { end: 5000 / 4 / 2 * 2, streaming: true});

    drop(writer);
    block_on(executor.run(summary_tasks)).unwrap();
}


#[test]
fn test_build_summary_completed() {
    env_logger::builder().is_test(true).filter_module("iguazu", log::LevelFilter::Debug).try_init().ok();

    use futures_lite::future::block_on;
    use crate::{ storage::{MemoryStorage, MemoryStreamWriter}, stream::StreamState, schema::FieldKind };

    let executor = Arc::new(Executor::new());
    let storage = Arc::new(MemoryStorage) as Arc<dyn Storage>;

    let mut writer = MemoryStreamWriter::new(crate::ElementSize::U8);
    writer.extend_from_slice(&[0xFF; 5000]);
    writer.commit();
    let stream: ArcStream = writer.stream().clone();
    drop(writer);
    let mut entity = EntityStream::field_data(FieldKind::Bits { pos: 0, bits: 8 }, stream);

    let summary_tasks = entity.build_summaries(&executor, &storage);
    assert_eq!(summary_tasks.len(), 1);
    block_on(executor.run(summary_tasks)).unwrap();

    let Entity::Data { summaries, .. } = &entity else { unreachable!() };
    let summary = summaries.get("bit_and_or");
    assert_eq!(summary.levels.len(), 2);
    assert_eq!(summary.levels[0].state(), StreamState { end: 5000 / 4 * 2, streaming: false});
    assert_eq!(summary.levels[1].state(), StreamState { end: 5000 / 4 / 2 * 2, streaming: false});
}
