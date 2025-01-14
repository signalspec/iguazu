use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub mod attribute;
pub use attribute::{ Attributes, Attribute };

mod fmt;
pub use fmt::EntityValueText;

use crate::{storage::MemoryStream, stream::ArcStream};

pub type Name = String;
pub type Path = String;

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
    Bits {
        bits: u32,
    },
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
        bits: u32,
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
    pub fn new(kind: EntityKind, data: S) -> Self {
        Self { kind, attributes: Default::default(), children: Default::default(), data }
    }

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

    pub fn try_map_data<T, E>(&self, f: &mut impl FnMut(&S) -> Result<T, E>) -> Result<Entity<T>, E> {
        let data = f(&self.data)?;

        let children = self.children.iter().map(|(k, v)| {
            Ok((k.clone(), v.try_map_data(f)?))
        }).collect::<Result<IndexMap<String, _>, E>>()?;

        let kind = self.kind.clone();
        let attributes = self.attributes.clone();

        Ok(Entity { data, kind, attributes, children })
    }
}

impl Entity<ArcStream> {
    pub fn record() -> Self {
        Self::new(EntityKind::Record, MemoryStream::new(1, &[]))
    }

    pub fn group() -> Self {
        Self::new(EntityKind::Group, MemoryStream::new(1, &[]))
    }

    pub fn tuple(fields: Vec<Field>) -> Self {
        Self::new(EntityKind::Tuple { fields }, MemoryStream::new(1, &[]))
    }
}

impl EntityKind {
    pub fn element_size(&self) -> usize {
        match self {
            EntityKind::Group | EntityKind::Record => 0,
            EntityKind::Bits { bits }
            | EntityKind::Signed { bits, .. }
            | EntityKind::Unsigned { bits, .. } => bits.div_ceil(8) as usize,
            EntityKind::Timestamp { .. } => 8,
            EntityKind::Logic { bits } => bits.len().div_ceil(8) as usize,
            EntityKind::Float { bits } => bits.div_ceil(8) as usize,
            EntityKind::Enum { bits, .. } => bits.div_ceil(8) as usize,
            EntityKind::FixedArray { .. } | EntityKind::Tuple { .. } => 0,
            EntityKind::VariableArray { bits } => bits.div_ceil(8) as usize,
        }
    }

    pub fn format<'a>(&'a self, value: u64) -> EntityValueText<'a> {
        EntityValueText { value, kind: self }
    }
}


