use core::fmt;
use std::fmt::{Formatter, Write};
use ecow::EcoString;

use crate::schema::{Field, FieldKind};
pub struct TextFormat(Vec<Element>);

enum Element {
    Literal(String),
    Bin { pos: u8, bits: u8 },
    Hex { pos: u8, bits: u8 },
    Character { pos: u8 },
    Unsigned { pos: u8, bits: u8, offset: f64, scale: f64 },
    Signed { pos: u8, bits: u8, offset: f64, scale: f64 },
    Float32 { pos: u8 },
    Float64,
    Enum { pos: u8, bits: u8, values: Vec<EcoString> },
    Tagged { pos: u8, tag_bits: u8, inner: Vec<TextFormat> },
}

impl TextFormat {
    pub fn new(pos: u8, spec: &Field) -> TextFormat {
        fn inner(components: &mut Vec<Element>, pos: u8, spec: &Field) {
            if let Some(text) = spec.text() {
                parse(components, pos, spec, text)
            } else {
                this(components, pos, spec)
            }
        }

        fn parse(components: &mut Vec<Element>, pos: u8, spec: &Field, text: EcoString) {
            let mut text = text.as_str();
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
                        components.push(Element::Literal("‽".into()));
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

        fn this(components: &mut Vec<Element>, pos: u8, spec: &Field) {
            match spec.kind {
                FieldKind::Null => {}
                FieldKind::Bits { bits } => {
                    if bits <= 8 {
                        components.push(Element::Bin { pos, bits })
                    } else {
                        components.push(Element::Hex { pos, bits })
                    }
                }
                FieldKind::Int { bits } => {
                    // TODO: precision
                    let offset = spec.number_offset();
                    let scale = spec.number_scale();
                    components.push(Element::Unsigned { pos, bits, offset, scale })
                }
                FieldKind::Signed { bits } => {
                    // TODO: precision
                    let offset = spec.number_offset();
                    let scale = spec.number_scale();
                    components.push(Element::Signed { pos, bits, offset, scale })
                }
                FieldKind::Timestamp => {
                    // TODO: epoch
                    let time_rate = spec.time_rate().unwrap_or(1.0);
                    components.push(Element::Unsigned { pos, bits: 64, offset: 0.0, scale: 1.0 / time_rate });
                }
                FieldKind::Float32 => {
                    // TODO: precision
                    components.push(Element::Float32 { pos })
                }
                FieldKind::Float64 => {
                    // TODO: precision
                    components.push(Element::Float64)
                }
                FieldKind::Enum { bits, ref values } => {
                    components.push(Element::Enum{ pos, bits, values: values.clone() });
                }
                FieldKind::Tagged { tag_bits, ref values } => {
                    let inner = values.values()
                        .map(|f| TextFormat::new(pos + tag_bits, f))
                        .collect();
                    components.push(Element::Tagged { pos, tag_bits, inner })
                }
                FieldKind::Character => {
                    components.push(Element::Character { pos });
                }
                FieldKind::BitStruct { .. } => {}
            }
        }

        let mut components = Vec::new();
        inner(&mut components, pos, spec);
        TextFormat(components)
    }

    pub fn write(&self, fmt: &mut impl Write, val: u64) -> fmt::Result {
        fn extract(val: u64, pos: u8, bits: u8) -> u64 {
            (val >> pos) & ((1 << bits) - 1)
        }

        for e in &self.0 {
            match *e {
                Element::Literal(ref s) => write!(fmt, "{}", s)?,
                Element::Bin { pos, bits } => {
                    write!(fmt, "{:0width$b}", extract(val, pos, bits), width = bits as usize)?
                }
                Element::Hex { pos, bits } => {
                    write!(fmt, "{:0width$x}", extract(val, pos, bits), width = bits as usize / 4)?
                }
                Element::Unsigned { pos, bits, scale, offset } => {
                    let i = extract(val, pos, bits);
                    write!(fmt, "{}", (i as f64) * scale + offset)?
                }
                Element::Character { pos } => {
                    let i = extract(val, pos, 8);
                    write!(fmt, "{}", (i as u8).escape_ascii())?;
                }
                Element::Signed { pos, bits, scale, offset } => {
                    let i = extract(val, pos, bits);
                    let a = u64::BITS - bits as u32;
                    let s = ((i << a) as i64) >> a;
                    write!(fmt, "{}", (s as f64) * scale + offset)?;
                }
                Element::Float32 { pos } => {
                    let i = extract(val, pos, 32);
                    write!(fmt, "{}", f32::from_bits(i as u32))?;
                }
                Element::Float64 => {
                    write!(fmt, "{}", f64::from_bits(val as u64))?;
                }
                Element::Enum { pos, bits, ref values } => {
                    let i = extract(val, pos, bits);
                    if let Some(v) = values.get(i as usize) {
                        write!(fmt, "{}", v)?;
                    } else {
                        write!(fmt, "‽")?;
                    }
                }
                Element::Tagged { pos, tag_bits, ref inner } => {
                    let tag = extract(val, pos, tag_bits);
                    if let Some(e) = inner.get(tag as usize) {
                        e.write(fmt, val)?;
                    } else {
                        write!(fmt, "‽")?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn format<'a>(&'a self, val: u64) -> FormatValue<'a> {
        FormatValue(self, val)
    }
}

pub struct FormatValue<'a>(pub &'a TextFormat, pub u64);

impl std::fmt::Display for FormatValue<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.write(f, self.1)
    }
}

#[test]
fn test_textformat() {
    use indexmap::indexmap;

    assert_eq!(
        TextFormat::new(0, &Field::new(FieldKind::Bits { bits: 4 })).format(0b10).to_string(),
        "0010"
    );

    assert_eq!(
        TextFormat::new(0, &Field::new(FieldKind::Int { bits: 8 })).format(123).to_string(),
        "123"
    );

    assert_eq!(
        TextFormat::new(0, &Field::new(FieldKind::Int { bits: 8, }).with_attribute("number:scale", 0.01)).format(123).to_string(),
        "1.23"
    );

    assert_eq!(
        TextFormat::new(0, &Field::new(FieldKind::Signed { bits: 8 })).format(-123i8 as u8 as u64).to_string(),
        "-123"
    );

    assert_eq!(
        TextFormat::new(0, &Field::new(FieldKind::Float32)).format(3333.25f32.to_bits() as u64).to_string(),
        "3333.25"
    );

    assert_eq!(
        TextFormat::new(0,
            &Field::new(
                FieldKind::BitStruct { children: indexmap! {
                    "a".into() => Field::new(FieldKind::Bits { bits: 2 }),
                    "b".into() => Field::new(FieldKind::Bits { bits: 3 }),
                }}
            ).with_attribute("text", "test({b}, {a})")
        ).format(0b10111).to_string(),
        "test(101, 11)"
    );

    assert_eq!(
        TextFormat::new(0, &Field::new(FieldKind::Enum { bits: 2, values: vec!["a".into(), "b".into(), "c".into()] })).format(1).to_string(),
        "b"
    );

    let f = TextFormat::new(0,
        &Field::new(
            FieldKind::Tagged { tag_bits: 1, values: indexmap! {
                "a".into() => Field::new(FieldKind::Null)
                    .with_attribute("text", "a"),
                "b".into() => Field::new(FieldKind::Bits { bits: 2 }),
            }}
        ).with_attribute("text", "e:{}")
    );

    assert_eq!(f.format(0b000).to_string(), "e:a");
    assert_eq!(f.format(0b001).to_string(), "e:00");
    assert_eq!(f.format(0b101).to_string(), "e:10");
}
