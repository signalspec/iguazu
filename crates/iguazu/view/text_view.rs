use core::fmt;
use std::fmt::{Formatter, Write};

use arrayvec::ArrayVec;
use ecow::EcoString;

use crate::{schema::{fmt::TextFormat, Entity, EntityStream, FieldKind }, stream::StreamState, Idx, IdxRange};

use super::{EnumView, IntView, ViewManager};
use crate::util::utf8::DisplayUtf8Lossy;
pub struct TextView<'a>(Vec<Element<'a>>);

enum Element<'a> {
    Literal(EcoString),
    Field(IntView<'a>, TextFormat),
    Enum(EnumView<'a>, Vec<TextView<'a>>),
    Utf8Str { ends: IntView<'a>, chars: IntView<'a> },
}

impl<'a> TextView<'a> {
    pub fn literal(v: EcoString) -> TextView<'a> {
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
                    elements.push(Element::Literal(EcoString::from(before).replace("}}", "}")));
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
                elements.push(Element::Literal(text.replace("}}", "}").into()));
            }
        }

        fn this<'a>(vm: &'a ViewManager, elements: &mut Vec<Element<'a>>, entity: &EntityStream) {
            match *entity {
                Entity::Group { .. } | Entity::Record { .. } => {}
                Entity::Data { ref data, ref field, .. } => {
                    elements.push(Element::Field(IntView::new_from_stream(vm, data), TextFormat::new(field)));
                },
                Entity::Union { ref variants, .. } => {
                    // TODO: format inner
                    let inner = variants.iter()
                        .map(|(name, _)| TextView::literal(name.clone()))
                        .collect();

                    if let Some(view) = vm.enum_view(entity) {
                        elements.push(Element::Enum(view, inner))
                    } else {
                        elements.push(Element::Literal("‽".into()));
                    }
                }
                Entity::FixedArray { .. } => {
                    // TODO
                }
                Entity::Tuple { .. } => {
                    // TODO
                }
                Entity::VariableArray { data: ref ends, ref child, .. } => {
                    match **child {
                        Entity::Data { data: ref inner_data, ref field, .. } => {
                            match field.kind {
                                FieldKind::Character { pos: 0 }=> {
                                    elements.push(Element::Utf8Str {
                                        ends: IntView::new_from_stream(vm, ends),
                                        chars: IntView::new_from_stream(vm, inner_data)
                                    });
                                }
                                _ => {}
                            }

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
                Element::Field(ref view, ref f) => {
                    if let Some(v) = view.get_u64(idx) {
                        f.write(fmt, v)?;
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
                Element::Utf8Str { ref ends, ref chars} => {
                    const MAX_LEN: usize = 255;
                    let start = if idx == 0 { Some(0) } else { ends.get_u64(idx - 1) };
                    let end = ends.get_u64(idx);

                    if let Some(min) = start && let Some(max) = end && let Some(full_len) = max.checked_sub(min) {
                        let mut buf = ArrayVec::<_, MAX_LEN>::new();

                        for chunk in chars.loaded_chunks::<u8>(IdxRange { min, max: (min + full_len.min(MAX_LEN as u64)) }) {
                            buf.try_extend_from_slice(chunk).unwrap();
                        }

                        if (buf.len() as u64) < full_len {
                            write!(fmt, "{}…", DisplayUtf8Lossy::truncated(&buf))?;
                        } else {
                            write!(fmt, "{}", DisplayUtf8Lossy::new(&buf))?;
                        }
                    } else {
                        write!(fmt, "…")?;
                        continue;
                    };
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
            Element::Field(v, _) => v.state(),
            Element::Enum(v, _) => v.state(),
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
    use crate::stream::ArcStream;
    use std::task::Waker;

    let mut vm = super::ViewManager::new();
    vm.begin(&Waker::noop().clone());

    let bits = EntityStream::field_data(
        FieldKind::Bits { pos: 0, bits: 2 }, MemoryStream::new::<u8>(&[0b10, 0b01, 0b00]),
    );

    let ints = EntityStream::field_data(
        FieldKind::Int { pos: 0, bits: 8 }, MemoryStream::new::<u8>(&[1, 10, 99, 123]),
    );

    let scaled_ints = EntityStream::field_data(
        FieldKind::Int { pos: 0, bits: 8 },
        MemoryStream::new::<u8>(&[1, 10, 99, 123]),
    ).with_attribute(crate::schema::attribute::core::NUMBER_SCALE, 0.01);

    let signed_ints = EntityStream::field_data(
        FieldKind::Signed { pos: 0, bits: 16 }, MemoryStream::new::<i16>(&[-10, 456, -1280, 9999]),
    );

    let floats = EntityStream::field_data(
        FieldKind::Float32 { pos: 0 }, MemoryStream::new::<f32>(&[3333.25, 12.0, 0.5]),
    );

    let chars = EntityStream::field_data(
        FieldKind::Character { pos: 0 }, MemoryStream::new::<u8>(b"abc1234"),
    );

    let strings = Entity::VariableArray {
        data: MemoryStream::new::<u64>(&[3, 7]) as ArcStream,
        child: Box::new(chars.clone()),
        attributes: Default::default(),
    };

    let literal = bits.clone().with_attribute(crate::schema::attribute::core::TEXT, "test".into());
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
        .with_attribute(crate::schema::attribute::core::TEXT, "test({b}, {a})".into());

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
