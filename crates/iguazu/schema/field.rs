use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::{attribute::{Attribute, Text}, Attributes};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NestedField {
    #[serde(flatten)]
    pub kind: Field,
    pub attributes: Attributes,
}

impl NestedField {
    pub fn new(kind: Field) -> NestedField {
        NestedField {
            kind,
            attributes: Attributes::default(),
        }
    }

    pub fn attribute<A: Attribute>(&self) -> Option<A> {
        self.attributes.get::<A>()
            .and_then(|a| a.ok())
            .or_else(|| A::field_default(self))
    }

    pub fn set_attribute<A: Attribute>(&mut self, a: &A) {
        self.attributes.set(a)
    }

    pub fn with_attribute<A: Attribute>(mut self, a: &A) -> Self {
        self.set_attribute(a);
        self
    }
    
    pub fn child(&self, key: &str) -> Option<(u16, &NestedField)> {
        match self.kind {
            Field::Tagged { tag_bits, ref values } => {
                values.get(key).map(|f| (tag_bits, f))
            }
            Field::Struct { ref children } => {
                let mut offset = 0;
                for (k, v) in children {
                    if k == key {
                        return Some((offset, v))
                    } else {
                        offset += v.kind.bit_width();
                    }
                }
                None
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "encoding", rename_all="snake_case")]
pub enum Field {
    Null,
    Bits {
        bits: u16
    },
    Unsigned {
        bits: u16,
        zero: f64,
        scale: f64,
    },
    Signed {
        bits: u16,
        scale: f64,
    },
    Timestamp {
        bits: u16,
        scale: f64
    },
    Float32,
    Tagged {
        tag_bits: u16,
        values: IndexMap<String, NestedField>
    },
    Struct {
        children: IndexMap<String, NestedField>
    },
}

impl Field {
    pub fn bit_width(&self) -> u16 {
        match *self {
            Field::Null => 0,
            Field::Bits { bits } => bits,
            Field::Unsigned { bits, .. } => bits,
            Field::Signed { bits, .. } => bits,
            Field::Timestamp { bits, .. } => bits,
            Field::Float32 => 32,
            Field::Tagged { tag_bits, ref values } => {
                let inner = values.values()
                    .map(|v| v.kind.bit_width())
                    .max().unwrap_or(0);
                tag_bits + inner
            }
            Field::Struct { ref children } => {
                children.values().map(|v| v.kind.bit_width()).sum()
            }
        }
    }
    
    pub fn enum_named(bits: u16, names: &[&str]) -> Field {
        let values = names.iter().map(|&n| {
            let mut attributes = Attributes::default();
            attributes.set(&Text(n.to_owned()));
            (n.to_owned(), NestedField { kind: Field::Null, attributes })
        }).collect();
        Field::Tagged { tag_bits: bits, values }
    }
}
