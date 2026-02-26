use std::{future::poll_fn, ops::{BitAnd, BitOr}, task::Poll};

use async_executor::{Executor, Task};
use ecow::EcoString;
use futures_lite::ready;
use indexmap::IndexMap;
use num_traits::Float;

use crate::{schema::{Field, FieldKind, Summary}, storage::Storage, stream::ArcStream, Element, ElementSize};

pub fn build_default_summaries(executor: &Executor<'static>, storage: &dyn Storage, stream: &ArcStream, field: &Field, summaries: &mut IndexMap<EcoString, Summary<ArcStream>>) {
    match field.kind {
        FieldKind::Bits{..} | FieldKind::BitStruct{..} => {
            make_summary_levels(executor, storage, stream, field, summaries, "bit_and_or", bit_summary1, bit_summary_reduce);
        }
        FieldKind::Int { .. } | FieldKind::Signed { .. } | FieldKind::Float32 | FieldKind::Float64 => {
            make_summary_levels(executor, storage, stream, field, summaries, "range", range_summary1, range_summary_reduce);
        }
        _ => { log::info!("No summaries for field kind {:?}", field.kind); }
    }
}

fn make_summary_levels(
    executor: &Executor<'static>,
    storage: &dyn Storage,
    stream: &ArcStream,
    field: &Field,
    summaries: &mut IndexMap<EcoString, Summary<ArcStream>>,
    key: &str,
    initial: impl Fn(&Executor<'static>, &dyn Storage, ArcStream, &Field) -> Option<(Task<Result<(), String>>, ArcStream)>,
    reduce: impl Fn(&Executor<'static>, &dyn Storage, ArcStream, &Field) -> Option<(Task<Result<(), String>>, ArcStream)>,
) {
    let wanted_levels = stream.state().end.checked_ilog2().unwrap_or(0).saturating_sub(8) as usize;
    let summary = summaries.entry(key.into()).or_insert(Summary::empty());

    log::info!("Building {key} summary to level {}", wanted_levels);

    if summary.levels.is_empty() && wanted_levels > 0 {
        log::info!("Building initial {key} summary at level 2");
        if let Some((task, stream)) = initial(executor, storage, stream.clone(), field) {
            task.detach();
            summary.levels.push(stream);
            summary.base_level = 2;
        }
    }

    while summary.base_level as usize + summary.levels.len() < wanted_levels {
        log::info!("Building {key} summary at level {}", summary.base_level as usize + summary.levels.len());
        if let Some((task, stream)) = reduce(executor, storage, summary.levels.last().unwrap().clone(), field) {
            task.detach();
            summary.levels.push(stream);
        }
    }
}

fn make_summary<T: Element + Default, const N: usize, const R: usize>(
    executor: &Executor<'static>,
    storage: &dyn Storage,
    stream: ArcStream,
    mut f: impl FnMut([T; N]) -> [T; R] + Send + 'static
) -> (Task<Result<(), String>>, ArcStream) {

    let mut output = storage.create_stream(T::ELEMENT_SIZE);
    let output_stream = output.stream();

    let task = executor.spawn(async move {
        let mut iter = stream.iter().await.map_err(|e| e.to_string())?;

        poll_fn(move |cx| {
            loop {
                let r = ready!(iter.poll_next(cx).at_least(N * size_of::<T>()));
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
                        iter.consume(consumed);
                    }
                    Ok(_) => {
                        log::info!("Summary task completed, len = {}", output.pos());
                        return Poll::Ready(Ok(()));
                    }
                }
            }
        }).await
    });

    (task, output_stream)
}

pub(crate) fn bit_summary1(executor: &Executor<'static>, storage: &dyn Storage, stream: ArcStream, _field: &Field) -> Option<(Task<Result<(), String>>, ArcStream)> {
    fn bit_and_or_initial<T: Copy + BitOr<Output = T> + BitAnd<Output = T>>([a, b, c, d]: [T; 4]) -> [T; 2] {
        [a & b & c & d, a | b | c | d]
    }

    Some(match stream.desc().element_size {
        ElementSize::U8 => make_summary(executor, storage, stream, bit_and_or_initial::<u8>),
        ElementSize::U16 => make_summary(executor, storage, stream, bit_and_or_initial::<u16>),
        ElementSize::U32 => make_summary(executor, storage, stream, bit_and_or_initial::<u32>),
        ElementSize::U64 => make_summary(executor,storage, stream, bit_and_or_initial::<u64>),
    })
}

pub(crate) fn bit_summary_reduce(executor: &Executor<'static>, storage: &dyn Storage, stream: ArcStream, _field: &Field) -> Option<(Task<Result<(), String>>, ArcStream)> {
    fn bit_and_or_reduce<T: Copy + BitOr<Output = T> + BitAnd<Output = T>>([min1, max1, min2, max2]: [T; 4]) -> [T; 2] {
        [min1 & min2, max1 | max2]
    }

    Some(match stream.desc().element_size {
        ElementSize::U8 => make_summary(executor, storage, stream, bit_and_or_reduce::<u8>),
        ElementSize::U16 => make_summary(executor, storage, stream, bit_and_or_reduce::<u16>),
        ElementSize::U32 => make_summary(executor, storage, stream, bit_and_or_reduce::<u32>),
        ElementSize::U64 => make_summary(executor, storage, stream, bit_and_or_reduce::<u64>),
    })
}

fn range_summary1(executor: &Executor<'static>, storage: &dyn Storage, stream: ArcStream, field: &Field) -> Option<(Task<Result<(), String>>, ArcStream)> {
    fn min_max_initial<T: Copy + Ord>([a, b, c, d]: [T; 4]) -> [T; 2] {
        [a.min(b).min(c).min(d), a.max(b).max(c).max(d)]
    }

    fn min_max_initial_float<T: Copy + Float>([a, b, c, d]: [T; 4]) -> [T; 2] {
        [a.min(b).min(c).min(d), a.max(b).max(c).max(d)]
    }

    Some(match (&field.kind, stream.desc().element_size) {
        (FieldKind::Int { .. }, ElementSize::U8) => make_summary(executor, storage, stream, min_max_initial::<u8>),
        (FieldKind::Int { .. }, ElementSize::U16) => make_summary(executor, storage, stream, min_max_initial::<u16>),
        (FieldKind::Int { .. }, ElementSize::U32) => make_summary(executor, storage, stream, min_max_initial::<u32>),
        (FieldKind::Int { .. }, ElementSize::U64) => make_summary(executor, storage, stream, min_max_initial::<u64>),
        (FieldKind::Signed { bits: 8 }, ElementSize::U8) => make_summary(executor, storage, stream, min_max_initial::<i8>),
        (FieldKind::Signed { bits: 16 }, ElementSize::U16) => make_summary(executor, storage, stream, min_max_initial::<i16>),
        (FieldKind::Signed { bits: 32 }, ElementSize::U32) => make_summary(executor, storage, stream, min_max_initial::<i32>),
        (FieldKind::Signed { bits: 64 }, ElementSize::U64) => make_summary(executor, storage, stream, min_max_initial::<i64>),
        (FieldKind::Float32, ElementSize::U32) => make_summary(executor, storage, stream, min_max_initial_float::<f32>),
        (FieldKind::Float64, ElementSize::U64) => make_summary(executor, storage, stream, min_max_initial_float::<f64>),
        _ => return None,
    })
}

fn range_summary_reduce(executor: &Executor<'static>, storage: &dyn Storage, stream: ArcStream, field: &Field) -> Option<(Task<Result<(), String>>, ArcStream)> {
    fn min_max_reduce<T: Copy + Ord>([min1, max1, min2, max2]: [T; 4]) -> [T; 2] {
        [min1.min(min2), max1.max(max2)]
    }

    fn min_max_reduce_float<T: Copy + Float>([min1, max1, min2, max2]: [T; 4]) -> [T; 2] {
        [min1.min(min2), max1.max(max2)]
    }

    Some(match (&field.kind, stream.desc().element_size) {
        (FieldKind::Int { .. }, ElementSize::U8) => make_summary(executor, storage, stream, min_max_reduce::<u8>),
        (FieldKind::Int { .. }, ElementSize::U16) => make_summary(executor, storage, stream, min_max_reduce::<u16>),
        (FieldKind::Int { .. }, ElementSize::U32) => make_summary(executor, storage, stream, min_max_reduce::<u32>),
        (FieldKind::Int { .. }, ElementSize::U64) => make_summary(executor, storage, stream, min_max_reduce::<u64>),
        (FieldKind::Signed { bits: 8 }, ElementSize::U8) => make_summary(executor, storage, stream, min_max_reduce::<i8>),
        (FieldKind::Signed { bits: 16 }, ElementSize::U16) => make_summary(executor, storage, stream, min_max_reduce::<i16>),
        (FieldKind::Signed { bits: 32 }, ElementSize::U32) => make_summary(executor, storage, stream, min_max_reduce::<i32>),
        (FieldKind::Signed { bits: 64 }, ElementSize::U64) => make_summary(executor, storage, stream, min_max_reduce::<i64>),
        (FieldKind::Float32, ElementSize::U32) => make_summary(executor, storage, stream, min_max_reduce_float::<f32>),
        (FieldKind::Float64, ElementSize::U64) => make_summary(executor, storage, stream, min_max_reduce_float::<f64>),
        _ => return None,
    })
}
