use std::{future::poll_fn, ops::{BitAnd, BitOr}, task::Poll};

use async_executor::{Executor, Task};
use futures_lite::ready;

use crate::{storage::MemoryStreamWriter, stream::ArcStream, Element, ElementType};


fn make_summary<T: Element + Default, const N: usize, const R: usize>(executor: &Executor, stream: ArcStream, mut f: impl FnMut([T; N]) -> [T; R] + Send + 'static) -> (Task<Result<(), String>>, ArcStream) {
    let mut iter = stream.iter();
    let mut buffer = [T::default(); N];
    let mut pos = 0;
    let mut output = MemoryStreamWriter::new(T::ELEMENT_TYPE);
    let output_stream = output.stream().clone() as ArcStream;

    let task = executor.spawn(poll_fn(move |cx| {
        loop {
            let r = ready!(iter.poll_next(cx));
            match r {
                Ok(data) if data.is_empty() => return Poll::Ready(Ok(())),
                Err(e) => return Poll::Ready(Err(e)),
                Ok(mut src) => {
                    loop {
                        let (copy, rest) = src.split_at((N * size_of::<T>() - pos).min(src.len()));
                        bytemuck::cast_slice_mut(&mut buffer)[pos..(pos + copy.len())].copy_from_slice(copy);
                        pos = pos + copy.len();
                        if pos == N {
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