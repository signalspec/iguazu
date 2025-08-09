use std::{future::poll_fn, ops::{BitAnd, BitOr}, task::Poll};

use async_executor::{Executor, Task};
use ecow::EcoString;
use futures_lite::ready;
use indexmap::IndexMap;

use crate::{schema::{Field, FieldKind, Summary}, storage::MemoryStreamWriter, stream::ArcStream, Element, ElementType};

pub fn build_default_summaries(executor: &Executor, stream: &ArcStream, field: &Field, summaries: &mut IndexMap<EcoString, Summary<ArcStream>>) {
    match field.kind {
        FieldKind::Bits{..} | FieldKind::BitStruct{..} => {
            let wanted_levels = stream.state().end.ilog2().saturating_sub(8) as usize;
            let summary = summaries.entry("bit_and_or".into()).or_insert(Summary::empty());

            log::info!("Building bitwise summary to level {}", wanted_levels);

            if summary.levels.is_empty() && wanted_levels > 0 {
                log::info!("Building initial bitwise summary at level 2");
                let (task, stream) = bit_summary1(executor, stream.clone());
                task.detach();
                summary.levels.push(stream);
                summary.base_level = 2;
            }

            while summary.base_level as usize + summary.levels.len() < wanted_levels {
                log::info!("Building bitwise summary at level {}", summary.base_level as usize + summary.levels.len());
                let (task, stream) = bit_summary_reduce(executor, summary.levels.last().unwrap().clone());
                task.detach();
                summary.levels.push(stream);
            }
        }
        _ => { log::info!("No summaries for field kind {:?}", field.kind); }
    }
}

fn make_summary<T: Element + Default, const N: usize, const R: usize>(executor: &Executor, stream: ArcStream, mut f: impl FnMut([T; N]) -> [T; R] + Send + 'static) -> (Task<Result<(), String>>, ArcStream) {
    let input_len = stream.state().end;
    let mut iter = stream.iter();
    let mut buffer = [T::default(); N];
    let mut pos = 0;
    let mut output = MemoryStreamWriter::new(T::ELEMENT_TYPE);
    let output_stream = output.stream().clone() as ArcStream;

    let task = executor.spawn(poll_fn(move |cx| {
        loop {
            let r = ready!(iter.poll_next(cx));
            match r {
                Ok(data) if data.is_empty() => {
                    log::info!("Summary task completed, input={}, len = {}", input_len, output.pos());
                    return Poll::Ready(Ok(()));
                }
                Err(e) => return Poll::Ready(Err(e)),
                Ok(mut src) => {
                    loop {
                        let (copy, rest) = src.split_at((N * size_of::<T>() - pos).min(src.len()));
                        bytemuck::cast_slice_mut(&mut buffer)[pos..(pos + copy.len())].copy_from_slice(copy);
                        pos = pos + copy.len();
                        if pos == N * size_of::<T>() {
                            let r = f(buffer);
                            output.extend_from_slice(bytemuck::cast_slice(&r[..]));
                            pos = 0;
                            src = rest;
                        } else {
                            break;
                        }
                    }
                }
            }
        }
    }));

    (task, output_stream)
}

pub fn bit_summary1(executor: &Executor, stream: ArcStream) -> (Task<Result<(), String>>, ArcStream)  {
    fn bit_and_or_initial<T: Copy + BitOr<Output = T> + BitAnd<Output = T>>([a, b, c, d]: [T; 4]) -> [T; 2] {
        [a & b & c & d, a | b | c | d]
    }

    match stream.desc().element_type {
        ElementType::U8 => make_summary(executor, stream, bit_and_or_initial::<u8>),
        ElementType::U16 => make_summary(executor, stream, bit_and_or_initial::<u16>),
        ElementType::U32 => make_summary(executor, stream, bit_and_or_initial::<u32>),
        ElementType::U64 => make_summary(executor, stream, bit_and_or_initial::<u64>),
    }
}

pub fn bit_summary_reduce(executor: &Executor, stream: ArcStream) -> (Task<Result<(), String>>, ArcStream)  {
    fn bit_and_or_reduce<T: Copy + BitOr<Output = T> + BitAnd<Output = T>>([min1, max1, min2, max2]: [T; 4]) -> [T; 2] {
        [min1 & min2, max1 | max2]
    }

    match stream.desc().element_type {
        ElementType::U8 => make_summary(executor, stream, bit_and_or_reduce::<u8>),
        ElementType::U16 => make_summary(executor, stream, bit_and_or_reduce::<u16>),
        ElementType::U32 => make_summary(executor, stream, bit_and_or_reduce::<u32>),
        ElementType::U64 => make_summary(executor, stream, bit_and_or_reduce::<u64>),
    }
}