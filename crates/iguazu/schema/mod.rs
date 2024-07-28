use attribute::Attribute;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub mod attribute;
pub use attribute::Attributes;

mod field;
pub use field::{Field, NestedField};

use crate::stream::ArcStream;

pub type Name = String;
pub type Path = String;

pub type Entity = EntityKind<ArcStream>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all="snake_case")]
pub enum EntityKind<S> {
    Group {
        attributes: Attributes,
        children: IndexMap<String, EntityKind<S>>
    },
    Data {
        #[serde(flatten)]
        encoding: NestedField,
        data: S,
    },
}

impl<S> EntityKind<S> {
    pub fn attributes(&self) -> &Attributes {
        match self {
            EntityKind::Group { attributes, .. } => attributes,
            EntityKind::Data { encoding, .. } => &encoding.attributes,
        }
    }

    pub fn attributes_mut(&mut self) -> &mut Attributes {
        match self {
            EntityKind::Group { attributes, .. } => attributes,
            EntityKind::Data { encoding, .. } => &mut encoding.attributes,
        }
    }

    pub fn attribute<A: Attribute>(&self) -> Option<A> {
        self.attributes().get::<A>()
            .and_then(|a| a.ok())
            .or_else(|| A::entity_default(self))
    }

    pub fn set_attribute<A: Attribute>(&mut self, a: &A) {
        self.attributes_mut().set(a)
    }

    fn map_data<T>(&self, mut f: impl FnMut(&S) -> T) -> EntityKind<T> {
        match self {
            EntityKind::Group { attributes, children } => {
                let children = children.iter().map(|(k, v)| {
                    (k.clone(), v.map_data(|x| f(x)))
                }).collect();
                EntityKind::Group { attributes: attributes.clone(), children }
            }
            EntityKind::Data { data, encoding } => {
                EntityKind::Data { data: f(data), encoding: encoding.clone() }
            }
        }
    }
}
