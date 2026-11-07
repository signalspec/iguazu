use bytemuck::Pod;
use num_traits::AsPrimitive;

use crate::{ElementSize, Idx, IdxRange, schema::{Field, FieldKind}, stream::{ArcStream, StreamState}};

use super::{IntView, ViewManager};

pub struct NumberView<'a> {
    view: IntView<'a>,
    format: Format,
}

#[derive(Clone, Copy)]
enum Format {
    UInt { scale: f64, offset: f64 },
    SInt { shift: u8, scale: f64, offset: f64 },
    F32 { scale: f64, offset: f64 },
    F64 { scale: f64, offset: f64 },
}

impl Format {
    fn decode(&self, v: u64) -> f64 {
        match self {
            Format::UInt { scale, offset } => {
                v as f64 * scale + offset
            },
            Format::SInt { shift, scale, offset } => {
                ((v << shift) as i64 >> shift) as f64 * scale + offset
            }
            Format::F32 { scale, offset} => {
                f32::from_bits(v as u32) as f64 * scale + offset
            }
            Format::F64 { scale, offset}=> {
                f64::from_bits(v) * scale + offset
            }
        }
    }
}

impl<'a> NumberView<'a> {
    pub fn new(vm: &'a ViewManager, stream: &ArcStream, field: &Field) -> Option<Self> {
        let view = IntView::new_from_stream(vm, stream);

        let scale = field.number_scale();
        let offset = field.number_offset();

        let format = match field.kind {
            FieldKind::Int { pos: 0, .. } => Some(Format::UInt { scale, offset }),
            FieldKind::Signed { pos: 0, bits } => Some(Format::SInt { shift: 64 - bits, scale, offset }),
            FieldKind::Float32 { pos: 0 }=> Some(Format::F32 { scale, offset }),
            FieldKind::Float64 => Some(Format::F64 { scale, offset }),
            FieldKind::Timestamp => {
                Some(Format::UInt { scale, offset: 0.0 })
            }
            _ => None,
        }?;

        Some(NumberView { view, format })
    }


    pub fn new_like(vm: &'a ViewManager, stream: &ArcStream, base: &NumberView<'_>) -> Self {
        let view = IntView::new_from_stream(vm, stream);
        NumberView { view, format: base.format }
    }

    pub fn get(&self, idx: Idx) -> Option<f64> {
        Some(self.format.decode(self.view.get_u64(idx)?))
    }

    pub fn iter(&self, range: IdxRange) -> impl Iterator<Item = Option<f64>> {
        self.view.iter_u64(range).map(|elem| elem.map(|elem| self.format.decode(elem)))
    }

    pub fn read_f32_slice(&self, start: Idx, out: &mut [f32]) -> usize {
        match (self.format, self.view.desc().element_size) {
            (Format::F32 { scale, offset }, ElementSize::U32) => read_scaled_slice::<f32>(&self.view, start, out, scale, offset),
            (Format::F64 { scale, offset }, ElementSize::U64) => read_scaled_slice::<f64>(&self.view, start, out, scale, offset),
            (Format::UInt { scale, offset }, ElementSize::U8) => read_scaled_slice::<u8>(&self.view, start, out, scale, offset),
            (Format::UInt { scale, offset }, ElementSize::U16) => read_scaled_slice::<u16>(&self.view, start, out, scale, offset),
            (Format::UInt { scale, offset }, ElementSize::U32) => read_scaled_slice::<u32>(&self.view, start, out, scale, offset),
            (Format::UInt { scale, offset }, ElementSize::U64) => read_scaled_slice::<u64>(&self.view, start, out, scale, offset),
            (Format::SInt { scale, offset, shift: 56 }, ElementSize::U8) => read_scaled_slice::<i8>(&self.view, start, out, scale, offset),
            (Format::SInt { scale, offset, shift: 48 }, ElementSize::U16) => read_scaled_slice::<i16>(&self.view, start, out, scale, offset),
            (Format::SInt { scale, offset, shift: 32 }, ElementSize::U32) => read_scaled_slice::<i32>(&self.view, start, out, scale, offset),
            (Format::SInt { scale, offset, shift: 0 }, ElementSize::U64) => read_scaled_slice::<i64>(&self.view, start, out, scale, offset),
            _ => 0
        }
    }

    pub fn state(&self) -> StreamState {
        self.view.state()
    }

    pub fn len(&self) -> Idx {
        self.view.len()
    }
}

fn read_scaled_slice<T: Pod + AsPrimitive<f32>>(view: &IntView, start: Idx, out: &mut [f32], scale: f64, offset: f64) -> usize {
    let scale = scale as f32;
    let offset = offset as f32;

    let desc = view.desc();
    let mut block_i = start / desc.count as Idx;
    let mut src_pos = (start % desc.count as Idx) as usize;
    let mut dst_pos = 0;

    while dst_pos < out.len() {
        let block = view.block(block_i);
        if src_pos >= block.len() / size_of::<T>() {
            break;
        }
        let block = bytemuck::cast_slice::<u8, T>(block);

        while src_pos < block.len() && dst_pos < out.len() {
            out[dst_pos] = block[src_pos].as_() * scale + offset;
            dst_pos += 1;
            src_pos += 1;
        }

        block_i += 1;
        src_pos = 0;
    }

    dst_pos
}
