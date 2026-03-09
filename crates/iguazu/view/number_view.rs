use crate::{schema::{Field, FieldKind}, stream::{ArcStream, StreamState}, Idx, IdxRange};

use super::{IntView, ViewManager};

pub struct NumberView<'a> {
    view: IntView<'a>,
    format: Format,
}

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
            FieldKind::Int { .. } => Some(Format::UInt { scale, offset }),
            FieldKind::Signed { bits } => Some(Format::SInt { shift: 64 - bits, scale, offset }),
            FieldKind::Float32 => Some(Format::F32 { scale, offset }),
            FieldKind::Float64 => Some(Format::F64 { scale, offset }),
            FieldKind::Timestamp => {
                Some(Format::UInt { scale, offset: 0.0 })
            }
            _ => None,
        }?;

        Some(NumberView { view, format })
    }

    pub fn get(&self, idx: Idx) -> Option<f64> {
        Some(self.format.decode(self.view.get_u64(idx)?))
    }

    pub fn for_each_elem(&self, range: IdxRange, mut f: impl FnMut(Idx, Option<f64>)) {
        self.view.for_each_elem(range, |i, elem| {
            let v = elem.map(|elem| {
                self.format.decode(elem)
            });

            f(i, v)
        })
    }

    pub fn state(&self) -> StreamState {
        self.view.state()
    }
}
