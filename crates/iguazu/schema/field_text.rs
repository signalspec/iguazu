use core::fmt;
use std::fmt::{Formatter, Write};
use crate::{schema::Field, stream::FieldVal};

use super::{attribute::Text, NestedField};
pub struct TextFormat(Vec<Element>);

enum Element {
    Literal(String),
    Bin { pos: u16, bits: u16 },
    Hex { pos: u16, bits: u16 },
    Unsigned { pos: u16, bits: u16, zero: f64, scale: f64 },
    Signed { pos: u16, bits: u16, scale: f64 },
    Float32 { pos: u16 },
    Tagged { pos: u16, tag_bits: u16, inner: Vec<TextFormat> },
}

impl TextFormat {
    pub fn new(pos: u16, spec: &NestedField) -> TextFormat {
        fn inner(components: &mut Vec<Element>, pos: u16, spec: &NestedField) {
            if let Some(text) = spec.attribute::<Text>() {
                parse(components, pos, spec, &text.0)
            } else {
                this(components, pos, spec)
            }
        }

        fn parse(components: &mut Vec<Element>, pos: u16, spec: &NestedField, mut text: &str) {
            while let Some((before, rest)) = text.split_once('{') {
                if !before.is_empty() {
                    components.push(Element::Literal(before.replace("}}", "}")));
                }

                if let Some(rest) = rest.strip_prefix('{') {
                    components.push(Element::Literal("{".into()));
                    text = rest;
                } else if let Some((key, rest)) = rest.split_once('}') {
                    if key.is_empty() {
                        this(components, pos, spec);
                    } else if let Some((child_offset, child)) = spec.child(key) {
                        inner(components, pos + child_offset, child);
                    } else {
                        // unknown key
                        components.push(Element::Literal("?".into()));
                    }
                    text = rest;
                } else {
                    text = rest;
                    // unmatched '}'
                }
            }

            if !text.is_empty() {
                components.push(Element::Literal(text.replace("}}", "}")));
            }
        }

        fn this(components: &mut Vec<Element>, pos: u16, spec: &NestedField) {
            match spec.kind {
                Field::Null => {}
                Field::Bits { bits } => {
                    // TODO: bin / hex / dec / ascii
                    components.push(Element::Bin { pos, bits })
                }
                Field::Unsigned { bits, zero, scale } => {
                    // TODO: precision
                    components.push(Element::Unsigned { pos, bits, zero, scale })
                }
                Field::Signed { bits, scale } => {
                    // TODO: precision
                    components.push(Element::Signed { pos, bits, scale })
                }
                Field::Timestamp { bits, scale } => {
                    // TODO: epoch
                    components.push(Element::Unsigned { pos, bits, zero: 0.0, scale })
                }
                Field::Float32 => {
                    // TODO: precision
                    components.push(Element::Float32 { pos })
                }
                Field::Tagged { tag_bits, ref values } => {
                    let inner = values.values()
                        .map(|f| TextFormat::new(pos + tag_bits, f))
                        .collect();
                    components.push(Element::Tagged { pos, tag_bits, inner })
                }
                Field::Struct { .. } => {}
            }
        }

        let mut components = Vec::new();
        inner(&mut components, pos, spec);
        TextFormat(components)
    }


    pub fn write(&self, fmt: &mut impl Write, val: FieldVal) -> fmt::Result {
        for e in &self.0 {
            match *e {
                Element::Literal(ref s) => write!(fmt, "{}", s)?,
                Element::Bin { pos, bits } => {
                    write!(fmt, "{:0width$b}", val.field(pos, bits).as_u64(), width = bits as usize)?
                }
                Element::Hex { pos, bits } => {
                    write!(fmt, "{:0width$x}", val.field(pos, bits).as_u64(), width = bits as usize / 4)?
                }
                Element::Unsigned { pos, bits, zero, scale } => {
                    let i = val.field(pos, bits).as_u64();
                    write!(fmt, "{}", (i as f64 - zero) * scale)?
                }
                Element::Signed { pos, bits, scale } => {
                    let i = val.field(pos, bits).as_u64();
                    let a = u64::BITS - bits as u32;
                    let s = ((i << a) as i64) >> a;
                    write!(fmt, "{}", (s as f64) * scale)?;
                }
                Element::Float32 { pos } => {
                    let i = val.field(pos, 32).as_u64();
                    write!(fmt, "{}", f32::from_bits(i as u32))?;
                }
                Element::Tagged { pos, tag_bits, ref inner } => {
                    let tag = val.field(pos, tag_bits).as_u64();
                    if tag < inner.len() as u64 {
                        inner[tag as usize].write(fmt, val)?;
                    } else {
                        write!(fmt, "?")?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn format<'a>(&'a self, val: FieldVal<'a>) -> FormatValue<'a> {
        FormatValue(self, val)
    }
}

pub struct FormatValue<'a>(pub &'a TextFormat, pub FieldVal<'a>);

impl std::fmt::Display for FormatValue<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.write(f, self.1)
    }
}

#[test]
fn test_textformat() {
    use indexmap::indexmap;

    assert_eq!(
        TextFormat::new(0, &NestedField::new(Field::Null)
            .with_attribute(&Text("test".to_owned()))
        ).format(FieldVal::from_slice(&[0b10])).to_string(),
        "test"
    );

    assert_eq!(
        TextFormat::new(0, &NestedField::new(Field::Bits { bits: 4 })).format(FieldVal::from_slice(&[0b10])).to_string(),
        "0010"
    );

    assert_eq!(
        TextFormat::new(0, &NestedField::new(Field::Unsigned { bits: 8, zero: 0.0, scale: 1.0 })).format(FieldVal::from_slice(&[123])).to_string(),
        "123"
    );

    assert_eq!(
        TextFormat::new(0, &NestedField::new(Field::Unsigned { bits: 8, zero: 0.0, scale: 0.01 })).format(FieldVal::from_slice(&[123])).to_string(),
        "1.23"
    );

    assert_eq!(
        TextFormat::new(0, &NestedField::new(Field::Signed { bits: 8, scale: 1.0 })).format(FieldVal::from_slice(&[123])).to_string(),
        "123"
    );

    assert_eq!(
        TextFormat::new(0, &NestedField::new(Field::Signed { bits: 8, scale: 1.0 })).format(FieldVal::from_slice(&[133])).to_string(),
        "-123"
    );

    assert_eq!(
        TextFormat::new(0, &NestedField::new(Field::Float32)).format(FieldVal::from_slice(&(3333.25f32.to_bits() as u64).to_le_bytes())).to_string(),
        "3333.25"
    );

    assert_eq!(
        TextFormat::new(0,
            &NestedField::new(
                Field::Struct { children: indexmap! {
                    "a".to_owned() => NestedField::new(Field::Bits { bits: 2 }),
                    "b".to_owned() => NestedField::new(Field::Bits { bits: 3 }),
                }}
            ).with_attribute(&Text("test({b}, {a})".to_owned()))
        ).format(FieldVal::from_slice(&[0b10111])).to_string(),
        "test(101, 11)"
    );

    let f = TextFormat::new(0,
        &NestedField::new(
            Field::Tagged { tag_bits: 1, values: indexmap! {
                "a".to_owned() => NestedField::new(Field::Null)
                    .with_attribute(&Text("a".to_owned())),
                "b".to_owned() => NestedField::new(Field::Bits { bits: 2 }),
            }}
        ).with_attribute(&Text("e:{}".to_owned()))
    );

    assert_eq!(f.format(FieldVal::from_slice(&[0b000])).to_string(), "e:a");
    assert_eq!(f.format(FieldVal::from_slice(&[0b001])).to_string(), "e:00");
    assert_eq!(f.format(FieldVal::from_slice(&[0b101])).to_string(), "e:10");
}
