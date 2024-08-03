use attribute::Attribute;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub mod attribute;
pub use attribute::Attributes;

mod field;
pub use field::{Field, NestedField};

mod field_text;
pub use field_text::{ TextFormat, FormatValue };

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

    pub fn try_map_data<T, E>(&self, f: &mut impl FnMut(&S) -> Result<T, E>) -> Result<EntityKind<T>, E> {
        match self {
            EntityKind::Group { attributes, children } => {
                let children = children.iter().map(|(k, v)| {
                    Ok((k.clone(), v.try_map_data(f)?))
                }).collect::<Result<IndexMap<String, EntityKind<T>>, E>>()?;
                Ok(EntityKind::Group { attributes: attributes.clone(), children })
            }
            EntityKind::Data { data, encoding } => {
                Ok(EntityKind::Data { data: f(data)?, encoding: encoding.clone() })
            }
        }
    }
}
