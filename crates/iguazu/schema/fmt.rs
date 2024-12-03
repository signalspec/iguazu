use super::EntityKind;

pub struct EntityValueText<'a> {
    pub(in super) value: &'a [u8],
    pub(in super) kind: &'a EntityKind,
}

fn get_u64(v: &[u8]) -> u64 {
    let mut data = [0; 8];
    data[..v.len()].copy_from_slice(v);
    u64::from_le_bytes(data)
}

fn get_i64(v: &[u8]) -> i64 {
    let shift = v.len() * 8 - u64::BITS as usize;
    (get_u64(v) << shift) as i64 >> shift
}

impl<'a> std::fmt::Display for EntityValueText<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self.kind {            
            EntityKind::Bits { bits } => {
                let val = get_u64(self.value);
                if bits % 4 == 0 {
                    write!(f, "{:0width$X}", val, width = bits as usize / 4)
                } else {
                    write!(f, "{:0width$b}", val, width = bits as usize)
                }
            }

            EntityKind::Logic { ref bits } => {
                let val = get_u64(self.value);
                write!(f, "{:0width$b}", val, width = bits.len())
            }

            EntityKind::Unsigned { scale, offset, .. } if self.value.len() <= 8 => {
                let d = get_u64(self.value) as f64 * scale + offset;
                write!(f, "{d}")
            }

            EntityKind::Signed { scale, offset, .. } if self.value.len() <= 8 => {
                let d = get_i64(self.value) as f64 * offset + scale;
                write!(f, "{d}")
            }

            EntityKind::Float { bits: 32 } if self.value.len() == 4 => {
                let v = f32::from_le_bytes(self.value.try_into().unwrap());
                write!(f, "{v}")
            }

            EntityKind::Float { bits: 64 } if self.value.len() == 8 => {
                let v = f64::from_le_bytes(self.value.try_into().unwrap());
                write!(f, "{v}")
            }

            EntityKind::Enum { ref values, .. } if self.value.len() <= 8 => {
                let d = get_u64(self.value) as usize;
                let name = values.get(d).map_or("‽", |f| &f.name);
                write!(f, "{name}")
            }

            _ => Ok(())
        }
    }
}

#[test]
fn test_format() {
    use crate::schema::Field;

    assert_eq!(EntityKind::Bits { bits: 16 }.format(&[0x34, 0x12]).to_string(), "1234");
    assert_eq!(EntityKind::Bits { bits: 4 }.format(&[0xA]).to_string(), "A");
    assert_eq!(EntityKind::Bits { bits: 3 }.format(&[0x5]).to_string(), "101");
    
    assert_eq!(EntityKind::Logic { bits: vec![
        Field { name: "a".into(), attributes: Default::default()},
        Field { name: "b".into(), attributes: Default::default()},
    ] }.format(&[0x2]).to_string(), "10");

    assert_eq!(EntityKind::Unsigned { bits: 8, scale: 1.0, offset: 0.0 }.format(&[5]).to_string(), "5");
    assert_eq!(EntityKind::Unsigned { bits: 8, scale: 0.25, offset: -0.5 }.format(&[5]).to_string(), "0.75");
    assert_eq!(EntityKind::Unsigned { bits: 32, scale: 1.0, offset: 0.0 }.format(&[5, 5, 0, 0]).to_string(), "1285");

    assert_eq!(EntityKind::Enum { bits: 8, values: vec![
        Field { name: "a".into(), attributes: Default::default()},
        Field { name: "b".into(), attributes: Default::default()},
    ]}.format(&[0x1]).to_string(), "b");
}