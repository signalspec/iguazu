use crate::{schema::Field, stream::ElementType};

use super::{EntityKind, EntityStream};

pub struct EntityValueText<'a> {
    value: u64,
    kind: ValueFormatter<'a>,
}

#[derive(Clone, Copy)]
pub enum ValueFormatter<'a> {
    Binary { bits: u8 },
    Hex { digits: u8 },
    Unsigned { scale: f64, offset: f64 },
    Signed { bits: u32, scale: f64, offset: f64 },
    Float32,
    Float64,
    Enum { values: &'a Vec<Field> },
}

impl <'a> ValueFormatter<'a> {
    pub fn bits(bits: u8) -> ValueFormatter<'a> {
        if bits % 4 == 0 {
            ValueFormatter::Hex { digits: bits / 4 }
        } else {
            ValueFormatter::Binary { bits }
        }
    }

    pub(in super) fn new(entity: &'a EntityStream) -> Option<ValueFormatter<'a>> {
        match entity.kind {
            EntityKind::Bits { ref bits, .. } => Some(Self::bits(bits.width())),
            EntityKind::Number { ref data, .. } => {
                use ElementType::*;
                let scale = entity.number_scale();
                let offset = entity.number_offset();
                match data.desc().element_type {
                    U8 | U16 | U32 | U64 => Some(ValueFormatter::Unsigned { scale, offset }),
                    t @ (I8 | I16 | I32 | I64) => Some(ValueFormatter::Signed { bits: t.bits() as u32, scale, offset }),
                    F32 => Some(ValueFormatter::Float32),
                    F64 => Some(ValueFormatter::Float64),
                }
            }
            EntityKind::Enum { ref values, .. } => Some(ValueFormatter::Enum { values }),
            _ => None,
        }
    }

    pub fn format(&self, value: u64) -> EntityValueText<'a> {
        EntityValueText { kind: *self, value }
    }
}


fn get_i64(v: u64, bits: u32) -> i64 {
    let shift = bits - u64::BITS;
    (v << shift) as i64 >> shift
}

impl<'a> std::fmt::Display for EntityValueText<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {            
            ValueFormatter::Binary { bits, .. } => {
                write!(f, "{:0width$b}", self.value, width = bits as usize)
            }

            ValueFormatter::Hex { digits } => {
                write!(f, "{:0width$X}", self.value, width = digits as usize)
            }

            ValueFormatter::Unsigned { scale, offset, .. } => {
                let d = self.value as f64 * scale + offset;
                write!(f, "{d}")
            }

            ValueFormatter::Signed { bits, scale, offset, .. } => {
                let d = get_i64(self.value, bits) as f64 * offset + scale;
                write!(f, "{d}")
            }

            ValueFormatter::Float32 => {
                let v = f32::from_bits(self.value as u32);
                write!(f, "{v}")
            }

            ValueFormatter::Float64 => {
                let v = f64::from_bits(self.value);
                write!(f, "{v}")
            }

            ValueFormatter::Enum { ref values, .. } => {
                let name = values.get(self.value as usize).map_or("‽", |f| &f.name);
                write!(f, "{name}")
            }
        }
    }
}

#[test]
fn test_format() {
    use crate::schema::Field;

    assert_eq!(ValueFormatter::Hex { digits: 4 }.format(0x1234).to_string(), "1234");
    assert_eq!(ValueFormatter::Binary { bits: 3 }.format(0x5).to_string(), "101");

    assert_eq!(ValueFormatter::Unsigned { scale: 1.0, offset: 0.0 }.format(5).to_string(), "5");
    assert_eq!(ValueFormatter::Unsigned { scale: 0.25, offset: -0.5 }.format(5).to_string(), "0.75");
    assert_eq!(ValueFormatter::Unsigned { scale: 1.0, offset: 0.0 }.format(0x0505).to_string(), "1285");

    assert_eq!(ValueFormatter::Enum { values: &vec![
        Field { name: "a".into(), attributes: Default::default()},
        Field { name: "b".into(), attributes: Default::default()},
    ]}.format(1).to_string(), "b");
}