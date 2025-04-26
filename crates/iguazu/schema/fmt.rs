use super::EntityKind;

pub struct EntityValueText<'a> {
    pub(in super) value: u64,
    pub(in super) kind: &'a EntityKind,
}

fn get_i64(v: u64, bits: u32) -> i64 {
    let shift = bits - u64::BITS;
    (v << shift) as i64 >> shift
}

impl<'a> std::fmt::Display for EntityValueText<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self.kind {            
            EntityKind::Bits { bits } => {
                if bits % 4 == 0 {
                    write!(f, "{:0width$X}", self.value, width = bits as usize / 4)
                } else {
                    write!(f, "{:0width$b}", self.value, width = bits as usize)
                }
            }

            EntityKind::Logic { ref bits } => {
                write!(f, "{:0width$b}", self.value, width = bits.len())
            }

            EntityKind::Unsigned { scale, offset, .. } => {
                let d = self.value as f64 * scale + offset;
                write!(f, "{d}")
            }

            EntityKind::Signed { bits, scale, offset, .. } => {
                let d = get_i64(self.value, bits) as f64 * offset + scale;
                write!(f, "{d}")
            }

            EntityKind::Float { bits: 32 } => {
                let v = f32::from_bits(self.value as u32);
                write!(f, "{v}")
            }

            EntityKind::Float { bits: 64 } => {
                let v = f64::from_bits(self.value);
                write!(f, "{v}")
            }

            EntityKind::Enum { ref values, .. } => {
                let name = values.get(self.value as usize).map_or("‽", |f| &f.name);
                write!(f, "{name}")
            }

            _ => Ok(())
        }
    }
}

#[test]
fn test_format() {
    use crate::schema::Field;

    assert_eq!(EntityKind::Bits { bits: 16 }.format(0x1234).to_string(), "1234");
    assert_eq!(EntityKind::Bits { bits: 4 }.format(0xA).to_string(), "A");
    assert_eq!(EntityKind::Bits { bits: 3 }.format(0x5).to_string(), "101");
    
    assert_eq!(EntityKind::Logic { bits: vec![
        Field { name: "a".into(), attributes: Default::default()},
        Field { name: "b".into(), attributes: Default::default()},
    ] }.format(0x2).to_string(), "10");

    assert_eq!(EntityKind::Unsigned { bits: 8, scale: 1.0, offset: 0.0 }.format(5).to_string(), "5");
    assert_eq!(EntityKind::Unsigned { bits: 8, scale: 0.25, offset: -0.5 }.format(5).to_string(), "0.75");
    assert_eq!(EntityKind::Unsigned { bits: 32, scale: 1.0, offset: 0.0 }.format(0x0505).to_string(), "1285");

    assert_eq!(EntityKind::Enum { values: vec![
        Field { name: "a".into(), attributes: Default::default()},
        Field { name: "b".into(), attributes: Default::default()},
    ]}.format(1).to_string(), "b");
}