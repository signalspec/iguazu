use crate::{schema::{EntityKind, EntityStream}, Idx, IdxRange};

use super::{IntView, ViewManager};

pub struct NumberView<'a> {
    view: IntView<'a>,
    format: Format,
}

enum Format {
    None,
    UInt { scale: f64, offset: f64 },
    SInt { shift: u8, scale: f64, offset: f64 },
    F32,
    F64,
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
            Format::F32 => {
                f32::from_bits(v as u32) as f64
            }
            Format::F64 => {
                f64::from_bits(v)
            }
            Format::None => f64::NAN
        }
    }
}

impl<'a> NumberView<'a> {
    pub fn new(vm: &'a ViewManager, entity: &EntityStream) -> Self {
        let view = vm.int_view(entity);
        let format = match entity.kind {
            EntityKind::Signed { scale, offset, .. } => {
                let shift = 64 - 8 * entity.data.desc().element_size as u8;
                Format::SInt { scale, offset, shift  }
            }
            EntityKind::Unsigned { scale, offset, .. } => {
                Format::UInt { scale, offset }
            }
            EntityKind::Float { bits: 32 } => Format::F32,
            EntityKind::Float { bits: 64 } => Format::F64,
            EntityKind::Timestamp { sample_rate } => {
                Format::UInt { scale: 1.0 / sample_rate, offset: 0.0 }
            }
            _ => Format::None,
        };

        NumberView { view, format }
    }

    pub fn get(&self, idx: Idx) -> Option<f64> {
        Some(self.format.decode(self.view.get_u64(idx)?))
    }

    pub fn for_each_elem(&'a self, range: IdxRange, mut f: impl FnMut(Idx, Option<f64>)) {
        self.view.for_each_elem(range, |i, elem| {
            let v = elem.map(|elem| {
                self.format.decode(elem)
            });
            
            f(i, v)
        })
    }
}