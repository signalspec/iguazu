use std::convert::Infallible;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub mod attribute;
pub use attribute::{ Attributes, Attribute };

mod fmt;
pub use fmt::EntityValueText;

use crate::{storage::MemoryStream, stream::{ArcStream, ElementSize}};

pub type Name = String;
pub type Path = String;

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
    pub kind: EntityKind,

    pub data: S,

    #[serde(default = "IndexMap::new", skip_serializing_if = "IndexMap::is_empty")]
    pub children: IndexMap<String, Entity<S>>,
    
    #[serde(flatten)]
    pub attributes: Attributes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all="snake_case")]
pub enum EntityKind {
    Group,
    Record,
    Bits { bits: u32 },
    Logic {
        bits: Vec<Field>,
    },
    Timestamp {
        sample_rate: f64,
    },
    Unsigned {
        bits: u32,
        #[serde(default = "One::one", skip_serializing_if = "One::is_one")]
        scale: f64,
        #[serde(default = "Zero::zero", skip_serializing_if = "Zero::is_zero")]
        offset: f64,
    },
    Signed {
        bits: u32,
        #[serde(default = "One::one", skip_serializing_if = "One::is_one")]
        scale: f64,
        #[serde(default = "Zero::zero", skip_serializing_if = "Zero::is_zero")]
        offset: f64,
    },
    Float {
        bits: u32,
    },
    Enum {
        values: Vec<Field>,
    },
    FixedArray {
        elements: u32,
    },
    Tuple {
        fields: Vec<Field>,
    },
    VariableArray {
        bits: u32,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    
    #[serde(flatten)]
    pub attributes: Attributes,
}
impl Field {
    pub fn with_attribute<A: Attribute>(mut self, a: &A) -> Self {
        self.set_attribute(a);
        self
    }

    pub fn attribute<A: Attribute>(&self) -> Option<A> {
        self.attributes.get::<A>()
            .and_then(|a| a.ok())
    }

    pub fn set_attribute<A: Attribute>(&mut self, a: &A) {
        self.attributes.set(a)
    }
}

impl<S> Entity<S> {
    pub fn with_attribute<A: Attribute>(mut self, a: &A) -> Self {
        self.set_attribute(a);
        self
    }

    pub fn with_child(mut self, name: String, child: Entity<S>) -> Self {
        self.children.insert(name, child);
        self
    }

    pub fn attribute<A: Attribute>(&self) -> Option<A> {
        self.attributes.get::<A>()
            .and_then(|a| a.ok())
            .or_else(|| A::default(self))
    }

    pub fn set_attribute<A: Attribute>(&mut self, a: &A) {
        self.attributes.set(a)
    }

    pub fn try_map_children<T, E>(&self, mut f: impl FnMut(&str, &Entity<S>) -> Result<T, E>) -> Result<IndexMap<String, T>, E> {
        self.children.iter().map(|(k, v)| {
            Ok((k.clone(), f(k, v)?))
        }).collect()
    }

    pub fn try_map_data<T, E>(&self, f: &mut impl FnMut(&S) -> Result<T, E>) -> Result<Entity<T>, E> {
        let data = f(&self.data)?;

        let children = self.try_map_children(|_, c| c.try_map_data(f))?;

        let kind = self.kind.clone();
        let attributes = self.attributes.clone();

        Ok(Entity { data, kind, attributes, children })
    }

    pub fn only_child(&self) -> Option<&Entity<S>> {
        if self.children.len() == 1 {
            self.children.values().next()
        } else {
            None
        }
    }

    pub fn schema(&self) -> EntitySchema {
        self.try_map_data(&mut |_| Ok::<Ignored, Infallible>(Ignored)).unwrap()
    }
}

impl EntityKind {
    pub fn format<'a>(&'a self, value: u64) -> EntityValueText<'a> {
        EntityValueText { value, kind: self }
    }
}

impl EntityStream {
    pub fn new(kind: EntityKind, data: ArcStream) -> Self {
        Self { kind, attributes: Default::default(), children: Default::default(), data }
    }

    pub fn record() -> Self {
        Self::new(EntityKind::Record, MemoryStream::new(ElementSize::Null, &[]))
    }

    pub fn group() -> Self {
        Self::new(EntityKind::Group, MemoryStream::new(ElementSize::Null, &[]))
    }

    pub fn tuple(fields: Vec<Field>) -> Self {
        Self::new(EntityKind::Tuple { fields }, MemoryStream::new(ElementSize::Null, &[]))
    }
}

impl EntitySchema {
    pub fn new(kind: EntityKind) -> Self {
        Self { kind, attributes: Default::default(), children: Default::default(), data: Ignored }
    }

    pub fn group() -> Self {
        Self::new(EntityKind::Group)
    }

    pub fn record() -> Self {
        Self::new(EntityKind::Record)
    }

    pub fn bytes() -> Self {
        EntitySchema::new(EntityKind::Bits { bits: 8 })
    }

    pub fn logic8() -> Self {
        Self::new(EntityKind::Logic { bits: (0..8).map(|b| Field { name: format!("{b}"), attributes: Default::default() }).collect() })
    }

    pub fn single_stream(&self) -> Option<(&EntityKind, usize)> {
        match self.kind {
            EntityKind::Group | EntityKind::Record | EntityKind::VariableArray { .. } => None,
            EntityKind::Bits { .. } | EntityKind::Signed { .. } | EntityKind::Unsigned { .. } | EntityKind::Timestamp { .. } | EntityKind::Logic { .. } | EntityKind::Float { .. } | EntityKind::Enum { .. } => {
                Some((&self.kind, 1))
            }
            EntityKind::FixedArray { elements } => {
                let (kind, stride) = self.only_child()?.single_stream()?;
                Some((kind, stride * elements as usize))
            }
            EntityKind::Tuple { ref fields } => {
                let (kind, stride) = self.only_child()?.single_stream()?;
                Some((kind, stride * fields.len()))
            }
        }
    }

    pub fn wrap_single(&self, data: ArcStream) -> Option<EntityStream> {
        match self.kind {
            EntityKind::Group | EntityKind::Record | EntityKind::VariableArray { .. } => None,
            EntityKind::Bits { .. } | EntityKind::Signed { .. } | EntityKind::Unsigned { .. } | EntityKind::Timestamp { .. } | EntityKind::Logic { .. } | EntityKind::Float { .. } | EntityKind::Enum { .. } => {
                Some(Entity { data, kind: self.kind.clone(), attributes: self.attributes.clone(), children: IndexMap::new() })
            }
            EntityKind::FixedArray { .. } | EntityKind::Tuple { .. } => {
                let child = self.only_child()?.wrap_single(data)?;
                let child_name = self.children.iter().next().unwrap().0.clone();
                Some(Entity { data: MemoryStream::new(ElementSize::Null, &[]) as ArcStream, kind: self.kind.clone(), attributes: self.attributes.clone(), children: IndexMap::new() }.with_child(child_name, child))

            }
        }
    }
}


