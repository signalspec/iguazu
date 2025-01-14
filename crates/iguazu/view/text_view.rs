use core::fmt;
use std::fmt::{Formatter, Write};

use crate::{schema::{attribute::Text, EntityKind, EntityStream}, Idx};

use super::{EnumView, IntView, NumberView, ViewManager};
pub struct TextView(Vec<Element>);

enum Element {
    Literal(String),
    Bin(IntView, u32),
    Hex(IntView, u32),
    Num(NumberView),
    Enum(EnumView, Vec<TextView>),
}

impl TextView {
    pub fn literal(v: String) -> TextView {
        TextView(vec![Element::Literal(v)])
    }

    pub fn new(vm: &mut impl ViewManager, entity: &EntityStream) -> TextView {
        fn inner(vm: &mut impl ViewManager, elements: &mut Vec<Element>, entity: &EntityStream) {
            if let Some(text) = entity.attribute::<Text>() {
                parse(vm, elements, entity, &text.0)
            } else {
                this(vm, elements, entity)
            }
        }

        fn parse(vm: &mut impl ViewManager, elements: &mut Vec<Element>, entity: &EntityStream, mut text: &str) {
            while let Some((before, rest)) = text.split_once('{') {
                if !before.is_empty() {
                    elements.push(Element::Literal(before.replace("}}", "}")));
                }

                if let Some(rest) = rest.strip_prefix('{') {
                    elements.push(Element::Literal("{".into()));
                    text = rest;
                } else if let Some((key, rest)) = rest.split_once('}') {
                    if key.is_empty() {
                        this(vm, elements, entity);
                    } else if let Some(child) = entity.children.get(key) {
                        inner(vm, elements, child);
                    } else {
                        // unknown key
                        elements.push(Element::Literal("⌧".into()));
                    }
                    text = rest;
                } else {
                    text = rest;
                    // unmatched '}'
                }
            }

            if !text.is_empty() {
                elements.push(Element::Literal(text.replace("}}", "}")));
            }
        }

        fn this(vm: &mut impl ViewManager, elements: &mut Vec<Element>, entity: &EntityStream) {
            match entity.kind {
                EntityKind::Group | EntityKind::Record => {}
                EntityKind::Bits { bits } => {
                    if bits % 4 == 0 {
                        elements.push(Element::Hex(vm.int_view(entity), bits / 4))
                    } else {
                        elements.push(Element::Bin(vm.int_view(entity), bits))
                    }
                },
                EntityKind::Logic { ref bits } => {
                    elements.push(Element::Bin(vm.int_view(entity), bits.len() as u32))
                },
                EntityKind::Signed { .. } | EntityKind::Unsigned {.. } | EntityKind::Float { .. } | EntityKind::Timestamp { .. } => {
                    elements.push(Element::Num(vm.number_view(entity)))
                }
                EntityKind::Enum { bits, ref values } => {
                    // TODO: format inner
                    let inner = values.iter()
                        .map(|variant| TextView::literal(variant.name.clone()))
                        .collect();
                    
                    elements.push(Element::Enum(vm.enum_view(entity), inner))
                }
                EntityKind::FixedArray { elements } => {
                    // TODO
                }
                EntityKind::Tuple { ref fields } => {
                    // TODO
                }
                EntityKind::VariableArray { bits } => {
                    // TODO
                }
            }
        }

        let mut components = Vec::new();
        inner(vm, &mut components, entity);
        TextView(components)
    }

    pub fn write(&self, fmt: &mut impl Write, idx: Idx) -> fmt::Result {
        for e in &self.0 {
            match *e {
                Element::Literal(ref s) => write!(fmt, "{}", s)?,
                Element::Bin(ref view, digits) => {
                    if let Some(v) = view.get_u64(idx) {
                        write!(fmt, "{:0width$b}", v, width = digits as usize)?
                    } else {
                        write!(fmt, "…")?;
                    }
                }
                Element::Hex(ref view, digits) => {
                    if let Some(v) = view.get_u64(idx) {
                        write!(fmt, "{:0width$x}", v, width = digits as usize)?
                    } else {
                        write!(fmt, "…")?;
                    }
                }
                Element::Num(ref view) => {
                    if let Some(v) = view.get(idx) {
                        write!(fmt, "{v}")?
                    } else {
                        write!(fmt, "…")?;
                    }
                }
                Element::Enum(ref view, ref options) => {
                    if let Some((v, child_idx)) = view.get(idx) {
                        if let Some(opt) = options.get(v) {
                            opt.write(fmt, child_idx)?;
                        } else {
                            write!(fmt, "‽")?;
                        }
                    } else {
                        write!(fmt, "…")?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn format<'a>(&'a self, idx: Idx) -> FormatValue<'a> {
        FormatValue(self, idx)
    }
}

pub struct FormatValue<'a>(pub &'a TextView, pub Idx);

impl std::fmt::Display for FormatValue<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.write(f, self.1)
    }
}

#[test]
fn test_textview() {
    use crate::storage::MemoryStream;

    let mut vm = super::SimpleViewManager;

    let bits = EntityStream::new(
        EntityKind::Bits { bits: 2 },
        MemoryStream::new(1, &[0b10, 0b01, 0b00])
    );

    let ints = EntityStream::new(
        EntityKind::Unsigned { bits: 8, scale: 1.0, offset: 0.0 },
        MemoryStream::new(1, &[1, 10, 99, 123])
    );

    let scaled_ints = EntityStream::new(
        EntityKind::Unsigned { bits: 8, scale: 0.01, offset: 0.0 },
        MemoryStream::new(1, &[1, 10, 99, 123])
    );

    let signed_ints = EntityStream::new(
        EntityKind::Signed { bits: 16, scale: 1.0, offset: 0.0 },
        MemoryStream::new(2, &[-10, 456, -1280, 9999].into_iter().flat_map(i16::to_le_bytes).collect::<Vec<u8>>())
    );

    let floats = EntityStream::new(
        EntityKind::Float { bits: 32 },
        MemoryStream::new(4, &[3333.25, 12.0, 0.5].into_iter().flat_map(f32::to_le_bytes).collect::<Vec<u8>>())
    );
    
    let literal_view = vm.text_view(
        &bits.clone().with_attribute(&Text("test".into()))
    );
    assert_eq!(literal_view.format(0).to_string(), "test");
    assert_eq!(literal_view.format(100).to_string(), "test");

    let bits_view = vm.text_view(&bits);
    assert_eq!(bits_view.format(0).to_string(), "10");
    assert_eq!(bits_view.format(1).to_string(), "01");
    assert_eq!(bits_view.format(2).to_string(), "00");
    assert_eq!(bits_view.format(3).to_string(), "…");

    let ints_view = vm.text_view(&ints);
    assert_eq!(ints_view.format(3).to_string(), "123");

    let scaled_ints_view = vm.text_view(&scaled_ints);
    assert_eq!(scaled_ints_view.format(3).to_string(), "1.23");

    let signed_ints_view = vm.text_view(&signed_ints);
    assert_eq!(signed_ints_view.format(0).to_string(), "-10");
    assert_eq!(signed_ints_view.format(1).to_string(), "456");
    assert_eq!(signed_ints_view.format(2).to_string(), "-1280");

    let floats_view = vm.text_view(&floats);
    assert_eq!(floats_view.format(0).to_string(), "3333.25");
    assert_eq!(floats_view.format(2).to_string(), "0.5");

    let record = EntityStream::record()
        .with_child("a".into(), bits.clone())
        .with_child("b".into(), ints.clone())
        .with_attribute(&Text("test({b}, {a})".into()));

    let record_view = vm.text_view(&record);
    assert_eq!(record_view.format(0).to_string(), "test(1, 10)");
    assert_eq!(record_view.format(1).to_string(), "test(10, 01)");
    assert_eq!(record_view.format(2).to_string(), "test(99, 00)");
    assert_eq!(record_view.format(3).to_string(), "test(123, …)");
}
