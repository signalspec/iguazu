use core::fmt;
use std::fmt::{Formatter, Write};

use arrayvec::ArrayVec;

use crate::{schema::{EntityKind, EntityStream }, stream::{ElementType, StreamState}, Idx, IdxRange};

use super::{EnumView, IntView, NumberView, ViewManager};
use crate::util::utf8::DisplayUtf8Lossy;
pub struct TextView<'a>(Vec<Element<'a>>);

enum Element<'a> {
    Literal(String),
    Bin(IntView<'a>, u32),
    Hex(IntView<'a>, u32),
    Num(NumberView<'a>),
    Enum(EnumView<'a>, Vec<TextView<'a>>),
    Utf8Char(IntView<'a>),
    Utf8Str { ends: IntView<'a>, chars: IntView<'a>},
}

impl<'a> TextView<'a> {
    pub fn literal(v: String) -> TextView<'a> {
        TextView(vec![Element::Literal(v)])
    }

    pub fn new(vm: &'a ViewManager, entity: &EntityStream) -> TextView<'a> {
        fn inner<'a>(vm: &'a ViewManager, elements: &mut Vec<Element<'a>>, entity: &EntityStream) {
            if let Some(text) = entity.text() {
                parse(vm, elements, entity, &text)
            } else {
                this(vm, elements, entity)
            }
        }

        fn parse<'a>(vm: &'a ViewManager, elements: &mut Vec<Element<'a>>, entity: &EntityStream, mut text: &str) {
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
                    } else if let Some(child) = entity.child(key) {
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

        fn this<'a>(vm: &'a ViewManager, elements: &mut Vec<Element<'a>>, entity: &EntityStream) {
            match entity.kind {
                EntityKind::Group { .. } | EntityKind::Record { .. } => {}
                EntityKind::Bits { bits, ref data } => {
                    if bits % 4 == 0 {
                        elements.push(Element::Hex(IntView::new_from_stream(vm, data), bits / 4))
                    } else {
                        elements.push(Element::Bin(IntView::new_from_stream(vm, data), bits))
                    }
                },
                EntityKind::Logic { ref bits, ref data } => {
                    elements.push(Element::Bin(IntView::new_from_stream(vm, data), bits.len() as u32))
                },
                EntityKind::Character { ref data } => {
                    elements.push(Element::Utf8Char(IntView::new_from_stream(vm, data)))
                },
                EntityKind::Number { .. } | EntityKind::Timestamp { .. } => {
                    if let Some(num) = vm.number_view(entity) {
                        elements.push(Element::Num(num))
                    } else {
                        elements.push(Element::Literal("‽".into()));
                    }
                }
                EntityKind::Enum { ref values, .. } => {
                    // TODO: format inner
                    let inner = values.iter()
                        .map(|variant| TextView::literal(variant.name.clone()))
                        .collect();

                    if let Some(view) = vm.enum_view(entity) {
                        elements.push(Element::Enum(view, inner))
                    } else {
                        elements.push(Element::Literal("‽".into()));
                    }
                }
                EntityKind::FixedArray { .. } => {
                    // TODO
                }
                EntityKind::Tuple { .. } => {
                    // TODO
                }
                EntityKind::VariableArray { data: ref ends, ref child } => {
                    match child.kind {
                        EntityKind::Character { data: ref chars } if chars.desc().element_type == ElementType::U8 => {
                            elements.push(Element::Utf8Str {
                                ends: IntView::new_from_stream(vm, ends),
                                chars: IntView::new_from_stream(vm, chars)
                            });
                        }
                        _ => {} // TODO
                    }
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
                Element::Utf8Char(ref view) => {
                    if let Some(v) = view.get_u64(idx) {
                        write!(fmt, "{}", (v as u8).escape_ascii())?;
                    } else {
                        write!(fmt, "…")?;
                    }
                }
                Element::Utf8Str { ref ends, ref chars} => {
                    const MAX_LEN: usize = 255;
                    let start = if idx == 0 { Some(0) } else { ends.get_u64(idx - 1) };
                    let end = ends.get_u64(idx);
                    
                    let (Some(min), Some(max)) = (start, end) else {
                        write!(fmt, "…")?;
                        continue;
                    };

                    let full_len = max - min;
                    let mut buf = ArrayVec::<_, MAX_LEN>::new();

                    for chunk in chars.loaded_chunks::<u8>(IdxRange { min, max: (min + full_len.min(MAX_LEN as u64)) }) {
                        buf.try_extend_from_slice(chunk).unwrap();
                    }

                    if (buf.len() as u64) < full_len {
                        write!(fmt, "{}…", DisplayUtf8Lossy::truncated(&buf))?;
                    } else {
                        write!(fmt, "{}", DisplayUtf8Lossy::new(&buf))?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn format<'b>(&'b self, idx: Idx) -> FormatValue<'a, 'b> {
        FormatValue(self, idx)
    }

    pub fn state(&self) -> StreamState {
        self.0.iter().map(|e| match e {
            Element::Literal(_) => StreamState { end: 0, streaming: false },
            Element::Bin(v, _) | Element::Hex(v, _) => v.state(),
            Element::Num(v) => v.state(),
            Element::Enum(v, _) => v.state(),
            Element::Utf8Char(v) => v.state(),
            Element::Utf8Str { ends, .. } => ends.state(),
        }).reduce(|a, b| StreamState {
            end: a.end.max(b.end),
            streaming: a.streaming || b.streaming,
        }).unwrap_or(StreamState { end: 0, streaming: false })
    }
}

pub struct FormatValue<'a, 'b>(pub &'b TextView<'a>, pub Idx);

impl std::fmt::Display for FormatValue<'_, '_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.write(f, self.1)
    }
}

#[test]
fn test_textview() {
    use crate::storage::MemoryStream;
    use std::task::Waker;

    let vm = super::ViewManager::new(Waker::noop().clone());

    let bits = EntityStream::new(
        EntityKind::Bits { bits: 2, data: MemoryStream::new::<u8>(&[0b10, 0b01, 0b00]) },
    );

    let ints = EntityStream::new(
        EntityKind::Number { data: MemoryStream::new::<u8>(&[1, 10, 99, 123]) },
    );

    let scaled_ints = EntityStream::new(
        EntityKind::Number {data: MemoryStream::new::<u8>(&[1, 10, 99, 123])},
    ).with_attribute("number:scale", 0.01);

    let signed_ints = EntityStream::new(
        EntityKind::Number { data: MemoryStream::new::<i16>(&[-10, 456, -1280, 9999])},
    );

    let floats = EntityStream::new(
        EntityKind::Number { data: MemoryStream::new::<f32>(&[3333.25, 12.0, 0.5])},
    );

    let chars = EntityStream::new(
        EntityKind::Character { data: MemoryStream::new::<u8>(b"abc1234") },
    );

    let strings = EntityStream::new(
        EntityKind::VariableArray {
            data: MemoryStream::new::<u64>(&[3, 7]),
            child: Box::new(chars.clone()),
        }
    );

    let literal = bits.clone().with_attribute("text", "test");
    let literal_view = vm.text_view(&literal);
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
        .with_attribute("text", "test({b}, {a})");

    let record_view = vm.text_view(&record);
    assert_eq!(record_view.format(0).to_string(), "test(1, 10)");
    assert_eq!(record_view.format(1).to_string(), "test(10, 01)");
    assert_eq!(record_view.format(2).to_string(), "test(99, 00)");
    assert_eq!(record_view.format(3).to_string(), "test(123, …)");

    let char_view = vm.text_view(&chars);
    assert_eq!(char_view.format(0).to_string(), "a");

    let str_view = vm.text_view(&strings);
    assert_eq!(str_view.format(0).to_string(), "abc");
    assert_eq!(str_view.format(1).to_string(), "1234");
}
