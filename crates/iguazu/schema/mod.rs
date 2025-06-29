use std::convert::Infallible;

use attribute::AttributeValue;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub mod attribute;
pub use attribute::AttributeMap;

pub mod fmt;

use crate::stream::ArcStream;

pub type Name = String;
pub type Path = String;

/// Placeholder for `data` field in `EntitySchema` which does not carry data of its own.
#[derive(Debug, Default, Clone)]
pub struct Ignored;

impl<'de> serde::Deserialize<'de> for Ignored {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_option(serde::de::IgnoredAny)?;
        Ok(Ignored)
    }
}

impl serde::Serialize for Ignored {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_none()
    }
}

pub type EntitySchema = Entity<Ignored>;
pub type EntityStream = Entity<ArcStream>;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Entity<S> {
    #[serde(flatten)]
    pub kind: EntityKind<S>,

    #[serde(flatten)]
    pub attributes: AttributeMap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all="snake_case")]
pub enum EntityKind<S> {
    Group {
        children: IndexMap<String, Entity<S>>,
    },
    Record {
        children: IndexMap<String, Entity<S>>,
    },
    Bits {
        data: S,
        bits: u32
    },
    Logic {
        data: S,
        bits: Vec<Field>,
    },
    Timestamp {
        data: S,
        sample_rate: f64,
    },
    Number {
        data: S,
    },
    Enum {
        data: S,
        values: Vec<Field>,
    },
    FixedArray {
        elements: u32,
        child: Box<Entity<S>>,
    },
    Tuple {
        fields: Vec<Field>,
        child: Box<Entity<S>>,
    },
    VariableArray {
        data: S,
        child: Box<Entity<S>>,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    
    #[serde(flatten)]
    pub attributes: AttributeMap,
}

impl Field {
    pub fn attribute<'a, A: TryFrom<&'a AttributeValue>>(&'a self, attr: &str) -> Option<A> {
        self.attributes.get(attr)
    }
    
    pub fn set_attribute(&mut self, attr: &str, val: impl Into<AttributeValue>) {
        self.attributes.insert(attr, val);
    }
    
    pub fn with_attribute(mut self, attr: &str, val: impl Into<AttributeValue>) -> Self {
        self.set_attribute(attr, val);
        self
    }
}

impl<S> Entity<S> {
    pub fn new(kind: EntityKind<S>) -> Self {
        Self { kind, attributes: Default::default() }
    }

    pub fn record() -> Self {
        Self::new(EntityKind::Record { children: IndexMap::new() })
    }

    pub fn group() -> Self {
        Self::new(EntityKind::Group { children: IndexMap::new() })
    }

    pub fn tuple(child: Entity<S>, fields: Vec<Field>) -> Self {
        Self::new(EntityKind::Tuple { child: Box::new(child), fields })
    }

    pub fn attribute<'a, A: TryFrom<&'a AttributeValue>>(&'a self, attr: &str) -> Option<A> {
        self.attributes.get(attr)
    }
    
    pub fn set_attribute(&mut self, attr: &str, val: impl Into<AttributeValue>) {
        self.attributes.insert(attr, val);
    }
    
    pub fn with_attribute(mut self, attr: &str, val: impl Into<AttributeValue>) -> Self {
        self.set_attribute(attr, val);
        self
    }

    pub fn child(&self, child: &str) -> Option<&Entity<S>> {
        match self.kind {
            EntityKind::Group { ref children } | EntityKind::Record { ref children } => {
                children.get(child)
            }
            EntityKind::FixedArray { ref child, .. } | EntityKind::Tuple { ref child, .. } | EntityKind::VariableArray { ref child, .. } => {
                Some(child)
            }
            _ => None,
        }
    }

    pub fn with_child(mut self, name: String, child: Entity<S>) -> Self {
        match &mut self.kind {
            EntityKind::Group { children } | EntityKind::Record { children } => {
                children.insert(name, child);
            }
            _ => panic!("Cannot add child to non-group or non-record entity"),
        }
        self
    }

    pub fn data(&self) -> Option<&S> {
        match &self.kind {
            EntityKind::Bits { data, .. } => Some(data),
            EntityKind::Logic { data, .. } => Some(data),
            EntityKind::Timestamp { data, .. } => Some(data),
            EntityKind::Number { data, .. } => Some(data),
            EntityKind::Enum { data, .. } => Some(data),
            _ => None,
        }
    }

    pub fn try_map_data<T, E>(&self, f: &mut impl FnMut(&S) -> Result<T, E>) -> Result<Entity<T>, E> {
        let attributes = self.attributes.clone();
        match self.kind {
            EntityKind::Group { ref children } => {
                let children = children.iter()
                    .map(|(name, child)| child.try_map_data(f).map(|c| (name.clone(), c)))
                    .collect::<Result<IndexMap<_, _>, E>>()?;
                Ok(Entity { kind: EntityKind::Group { children }, attributes })
            }
            EntityKind::Record { ref children } => {
                let children = children.iter()
                    .map(|(name, child)| child.try_map_data(f).map(|c| (name.clone(), c)))
                    .collect::<Result<IndexMap<_, _>, E>>()?;
                Ok(Entity { kind: EntityKind::Record { children }, attributes })
            }
            EntityKind::Bits { ref data, bits} => {
                let data = f(data)?;
                Ok(Entity { kind: EntityKind::Bits { bits, data }, attributes })
            }
            EntityKind::Logic { ref data, ref bits, .. } => {
                let data = f(data)?;
                let bits = bits.clone();
                Ok(Entity { kind: EntityKind::Logic { bits, data }, attributes })
            }
            EntityKind::Timestamp { ref data, sample_rate } => {
                let data = f(data)?;
                Ok(Entity { kind: EntityKind::Timestamp { sample_rate, data }, attributes })
            }
            EntityKind::Number { ref data } => {
                let data = f(data)?;
                Ok(Entity { kind: EntityKind::Number { data }, attributes })
            },
            EntityKind::Enum { ref data, ref values } => {
                let data = f(data)?;
                let values = values.clone();
                Ok(Entity { kind: EntityKind::Enum { data, values }, attributes })
            },
            EntityKind::FixedArray { elements, ref child } => {
                let child = Box::new(child.try_map_data(f)?);
                Ok(Entity { kind: EntityKind::FixedArray { elements, child }, attributes })
            }
            EntityKind::Tuple { ref fields, ref child } => {
                let child = Box::new(child.try_map_data(f)?);
                let fields = fields.clone();
                Ok(Entity { kind: EntityKind::Tuple { fields, child }, attributes })
            }
            EntityKind::VariableArray { ref data, ref child } => {
                let data = f(data)?;
                let child = Box::new(child.try_map_data(f)?);
                Ok(Entity { kind: EntityKind::VariableArray { data, child }, attributes })
            }
        }
    }

    pub fn schema(&self) -> EntitySchema {
        self.try_map_data(&mut |_| Ok::<Ignored, Infallible>(Ignored)).unwrap()
    }
}

impl EntitySchema {
    pub fn bytes() -> Self {
        EntitySchema::new(EntityKind::Bits { data: Ignored, bits: 8 })
    }

    pub fn logic8() -> Self {
        Self::new(EntityKind::Logic { data: Ignored, bits: (0..8).map(|b| Field { name: format!("{b}"), attributes: Default::default() }).collect() })
    }

    pub fn single_stream(&self) -> Option<(&EntityKind<Ignored>, usize)> {
        match self.kind {
            EntityKind::Group { .. } | EntityKind::Record { .. } | EntityKind::VariableArray { .. } => None,
            EntityKind::Bits { .. } | EntityKind::Number { .. } | EntityKind::Timestamp { .. } | EntityKind::Logic { .. } | EntityKind::Enum { .. } => {
                Some((&self.kind, 1))
            }
            EntityKind::FixedArray { ref child, elements } => {
                let (kind, stride) = child.single_stream()?;
                Some((kind, stride * elements as usize))
            }
            EntityKind::Tuple { ref child, ref fields } => {
                let (kind, stride) = child.single_stream()?;
                Some((kind, stride * fields.len()))
            }
        }
    }

    pub fn wrap_single(&self, data: ArcStream) -> Option<EntityStream> {
        let attributes = self.attributes.clone();
        match self.kind {
            EntityKind::Group { .. } | EntityKind::Record { .. } | EntityKind::VariableArray { .. } => None,
            EntityKind::Bits { data: Ignored, bits  } => {
                Some(Entity { kind: EntityKind::Bits { data, bits }, attributes })
            }
            EntityKind::Number { data: Ignored } => {
                Some(Entity { kind: EntityKind::Number { data }, attributes })
            }
            EntityKind::Timestamp { data: Ignored, sample_rate } => {
                Some(Entity { kind: EntityKind::Timestamp { data, sample_rate }, attributes })
            }
            EntityKind::Logic { data: Ignored, ref bits } => {
                Some(Entity { kind: EntityKind::Logic { data, bits: bits.clone() }, attributes })
            }
            EntityKind::Enum { data: Ignored, ref values } => {
                Some(Entity { kind: EntityKind::Enum { data, values: values.clone() }, attributes })
            }
            EntityKind::FixedArray { ref child, elements } => {
                let child = Box::new(child.wrap_single(data)?);
                Some(Entity { kind: EntityKind::FixedArray { elements, child }, attributes })
            }
            EntityKind::Tuple { ref child, ref fields } => {
                let child = Box::new(child.wrap_single(data)?);
                Some(Entity { kind: EntityKind::Tuple { fields: fields.clone(), child }, attributes })

            }
        }
    }
}

impl EntityStream {
    pub fn formatter(&self) -> Option<fmt::ValueFormatter> {
        fmt::ValueFormatter::new(self)
    }
}


