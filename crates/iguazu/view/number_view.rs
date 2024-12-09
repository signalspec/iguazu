use std::sync::Arc;

use crate::{schema::{EntityKind, EntityStream}, Idx, IdxRange};

use super::View;

pub struct NumberView {
    view: Arc<View>,
    format: Format,
}

enum Format {
    None,
    UInt { scale: f64, offset: f64 },
    SInt { scale: f64, offset: f64 },
    F32,
    F64,
}

impl Format {
    fn decode(&self, v: &[u8]) -> f64 {
        match self {
            Format::UInt { scale, offset } if v.len() <= 8 => {
                let mut data = [0; 8];
                data[..v.len()].copy_from_slice(v);
                u64::from_le_bytes(data) as f64 * scale + offset
            },
            Format::SInt { scale, offset } if v.len() <= 8 => {
                let mut data = [0; 8];
                data[..v.len()].copy_from_slice(v);
                let shift = 8 * (data.len() - v.len());
                ((u64::from_le_bytes(data) << shift) as i64 >> shift) as f64 * scale + offset
            }
            Format::F32 if v.len() == 4 => {
                f32::from_le_bytes(v.try_into().unwrap()) as f64
            }
            Format::F64 if v.len() == 8 => {
                f64::from_le_bytes(v.try_into().unwrap())
            }
            _ => f64::NAN,
        }
    }
}

impl NumberView {
    pub fn new(entity: &EntityStream, view: Arc<View>) -> Self {
        debug_assert!(Arc::ptr_eq(&entity.data, view.stream()));

        let format = match entity.kind {
            EntityKind::Signed { scale, offset, .. } => Format::SInt { scale, offset },
            EntityKind::Unsigned { scale, offset, .. } => Format::UInt { scale, offset },
            EntityKind::Float { bits: 32 } => Format::F32,
            EntityKind::Float { bits: 64 } => Format::F64,
            _ => Format::None,
        };

        NumberView { view, format }
    }

    pub fn range(&self) -> IdxRange {
        self.view.range()
    }

    pub fn get(&self, idx: Idx) -> Option<f64> {
        Some(self.format.decode(self.view.get(idx)?))
    }

    pub fn for_each_elem<'a>(&'a self, mut f: impl FnMut(Idx, Option<f64>)) {
        self.view.for_each_elem(|i, v| f(i, v.map(|v| self.format.decode(v))))
    }
}