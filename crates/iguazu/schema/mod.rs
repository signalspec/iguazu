use std::{convert::Infallible, sync::Arc};

use async_executor::{ Executor, Task };
use attribute::AttributeValue;
use ecow::{eco_format, EcoString};
use futures_lite::{future, stream, StreamExt};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub mod attribute;
pub use attribute::AttributeMap;

pub mod fmt;

pub mod json_virtual;

use crate::{storage::Storage, stream::ArcStream};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all="snake_case")]
pub enum Entity<S> {
    Group {
        children: IndexMap<EcoString, Entity<S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    Record {
        children: IndexMap<EcoString, Entity<S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    Union {
        data: S,
        variants: IndexMap<EcoString, Entity<S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    FixedArray {
        elements: u32,
        child: Box<Entity<S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    Tuple {
        fields: IndexMap<EcoString, AttributeMap>,
        child: Box<Entity<S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    VariableArray {
        data: S,
        child: Box<Entity<S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    #[serde(untagged)]
    Data {
        #[serde(flatten)]
        field: Field,
        data: S,

        #[serde(skip_serializing_if = "IndexMap::is_empty", default = "Default::default")]
        summaries: IndexMap<EcoString, Summary<S>>,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    #[serde(flatten)]
    pub kind: FieldKind,

    #[serde(flatten)]
    pub attributes: AttributeMap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all="snake_case")]
pub enum FieldKind {
    Null,
    Bits {
        bits: u8,
    },
    Character,
    Timestamp,
    Int {
        bits: u8,
    },
    Signed {
        bits: u8,
    },
    Float32,
    Float64,
    Enum {
        bits: u8,
        values: Vec<EcoString>,
    },
    Tagged {
        tag_bits: u8,
        values: IndexMap<EcoString, Field>,
    },
    BitStruct {
        children: IndexMap<EcoString, Field>,
    },
}

impl FieldKind {
    pub fn width(&self) -> u8 {
        match *self {
            FieldKind::Null => 0,
            FieldKind::Bits { bits } => bits,
            FieldKind::Character => 8,
            FieldKind::Timestamp { .. } => 64,
            FieldKind::Int { bits } => bits,
            FieldKind::Signed { bits } => bits,
            FieldKind::Float32 => 32,
            FieldKind::Float64 => 64,
            FieldKind::Enum { bits, .. } => bits,
            FieldKind::Tagged { tag_bits, ref values } => {
                tag_bits + values.values().map(|v| v.kind.width()).fold(0, u8::max)
            }
            FieldKind::BitStruct { ref children } => {
                children.values().map(|f| f.kind.width()).fold(0, u8::saturating_add)
            }
        }
    }
}

impl Field {
    pub fn new(kind: FieldKind) -> Self {
        Field { kind, attributes: Default::default() }
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

    pub fn child(&self, child: &str) -> Option<(u8, &Field)> {
        match self.kind {
            FieldKind::BitStruct { ref children, .. } => {
                let (i, _, child) = children.get_full(child)?;
                let offset = children.values().take(i).map(|f| f.kind.width()).sum::<u8>();
                Some((offset, child))
            }
            _ => None,
        }
    }
}

impl<S> Entity<S> {
    pub fn record() -> Self {
        Entity::Record { children: IndexMap::new(), attributes: Default::default() }
    }

    pub fn group() -> Self {
        Entity::Group { children: IndexMap::new(), attributes: Default::default() }
    }

    pub fn field_data(kind: FieldKind, data: S) -> Self {
        Entity::Data {
            data,
            field: Field {
                kind,
                attributes: Default::default(),
            },
            summaries: Default::default(),
        }
    }

    pub fn attributes(&self) -> &AttributeMap {
        match self {
            Entity::Group { attributes, .. }
            | Entity::Record { attributes, .. }
            | Entity::Union { attributes, .. }
            | Entity::FixedArray { attributes, .. }
            | Entity::Tuple { attributes, .. }
            | Entity::VariableArray { attributes, .. } => attributes,
            Entity::Data { field, .. } => &field.attributes,
        }
    }

    pub fn attributes_mut(&mut self) -> &mut AttributeMap {
        match self {
            Entity::Group { attributes, .. }
            | Entity::Record { attributes, .. }
            | Entity::Union { attributes, .. }
            | Entity::FixedArray { attributes, .. }
            | Entity::Tuple { attributes, .. }
            | Entity::VariableArray { attributes, .. } => attributes,
            Entity::Data { field, .. } => &mut field.attributes,
        }
    }

    pub fn tuple(child: Entity<S>, fields: IndexMap<EcoString, AttributeMap>) -> Self {
        Entity::Tuple { child: Box::new(child), fields, attributes: Default::default() }
    }

    pub fn attribute<'a, A: TryFrom<&'a AttributeValue>>(&'a self, attr: &str) -> Option<A> {
        self.attributes().get(attr)
    }

    pub fn set_attribute(&mut self, attr: &str, val: impl Into<AttributeValue>) {
        self.attributes_mut().insert(attr, val);
    }

    pub fn with_attribute(mut self, attr: &str, val: impl Into<AttributeValue>) -> Self {
        self.set_attribute(attr, val);
        self
    }

    pub fn child(&self, child: &str) -> Option<&Entity<S>> {
        match *self {
            Entity::Group { ref children, .. } | Entity::Record { ref children, .. } => {
                children.get(child)
            }
            Entity::FixedArray { ref child, .. }
            | Entity::Tuple { ref child, .. }
            | Entity::VariableArray { ref child, .. } => {
                Some(child)
            }
            _ => None,
        }
    }

    pub fn child_mut(&mut self, child: &str) -> Option<&mut Entity<S>> {
        match *self {
            Entity::Group { ref mut children, .. } | Entity::Record { ref mut children, .. } => {
                children.get_mut(child)
            }
            Entity::FixedArray { ref mut child, .. }
            | Entity::Tuple { ref mut child, .. }
            | Entity::VariableArray { ref mut child, .. } => {
                Some(child)
            }
            _ => None,
        }
    }

    pub fn child_owned(self, child: &str) -> Option<Self> {
        match self {
            Entity::Group { mut children, .. } | Entity::Record { mut children, .. } => {
                children.swap_remove(child)
            }
            Entity::FixedArray { child, .. }
            | Entity::Tuple { child, .. }
            | Entity::VariableArray { child, .. } => {
                Some(*child)
            }
            _ => None,
        }
    }

    pub fn with_child(mut self, name: EcoString, child: Entity<S>) -> Self {
        match &mut self {
            Entity::Group { children, .. }
            | Entity::Record { children, .. } => {
                children.insert(name, child);
            }
            _ => panic!("Cannot add child to non-group or non-record entity"),
        }
        self
    }

    pub fn select(&self, path: &str) -> Option<&Entity<S>> {
        let mut current = self;
        for part in path.split('.') {
            current = current.child(part)?;
        }
        Some(current)
    }

    pub fn select_mut(&mut self, path: &str) -> Option<&mut Entity<S>> {
        let mut current = self;
        for part in path.split('.') {
            current = current.child_mut(part)?;
        }
        Some(current)
    }

    pub fn select_owned(self, path: &str) -> Option<Self> {
        let mut current = self;
        for part in path.split('.') {
            current = current.child_owned(part)?;
        }
        Some(current)
    }

    pub fn data(&self) -> Option<&S> {
        match &self {
            Entity::Data { data, .. } => Some(data),
            Entity::Union { data, .. } => Some(data),
            Entity::VariableArray { data, .. } => Some(data),
            _ => None,
        }
    }

    pub fn try_map_data<T: Send, E: Send>(&self, f: &mut impl FnMut(&S) -> Result<T, E>) -> Result<Entity<T>, E> {
        match self {
            Entity::Group { children, attributes } => {
                let children = children.iter()
                    .map(|(name, child)| child.try_map_data(f).map(|c| (name.clone(), c)))
                    .collect::<Result<IndexMap<_, _>, E>>()?;
                Ok(Entity::Group { children, attributes: attributes.clone() })
            }
            Entity::Record { children, attributes } => {
                let children = children.iter()
                    .map(|(name, child)| child.try_map_data(f).map(|c| (name.clone(), c)))
                    .collect::<Result<IndexMap<_, _>, E>>()?;
                Ok(Entity::Record { children, attributes: attributes.clone() })
            }
            Entity::Union { data, variants, attributes } => {
                let data = f(data)?;
                let variants = variants.iter()
                    .map(|(name, variant)| variant.try_map_data(f).map(|c| (name.clone(), c)))
                    .collect::<Result<IndexMap<_, _>, E>>()?;
                Ok(Entity::Union { data, variants, attributes: attributes.clone() })
            },
            Entity::FixedArray { elements, child, attributes } => {
                let child = Box::new(child.try_map_data(f)?);
                Ok(Entity::FixedArray { elements: *elements, child, attributes: attributes.clone() })
            }
            Entity::Tuple { fields, child, attributes } => {
                let child = Box::new(child.try_map_data(f)?);
                let fields = fields.clone();
                Ok(Entity::Tuple { fields, child, attributes: attributes.clone() })
            }
            Entity::VariableArray { data, child, attributes } => {
                let data = f(data)?;
                let child = Box::new(child.try_map_data(f)?);
                Ok(Entity::VariableArray { data, child, attributes: attributes.clone() })
            }
            Entity::Data { data, field, summaries } => {
                let data = f(data)?;
                let summaries = summaries.iter()
                    .map(|(name, summary)| {
                        summary.try_map_data(f).map(|s| (name.clone(), s))
                    })
                    .collect::<Result<IndexMap<_, _>, E>>()?;
                Ok(Entity::Data { data, field: field.clone(), summaries })
            }
        }
    }

    pub async fn try_map_data_async<T, E, F, R>(self, f: F) -> Result<Entity<T>, E>
    where
        S: Send + 'static,
        T: Send + 'static,
        E: Send + 'static,
        F: Fn(S) -> R + Send + Clone + 'static,
        R: Future<Output = Result<T, E>> + Send + 'static,
    {
        let executor = Arc::new(Executor::new());

        async fn map_children<S, T, E, F, R>(
            ex: Arc<Executor<'static>>,
            children: IndexMap<EcoString, Entity<S>>,
            f: F
        ) -> Result<IndexMap<EcoString, Entity<T>>, E> where
            S: Send + 'static,
            T: Send + 'static,
            E: Send + 'static,
            F: Fn(S) -> R + Send + Clone + 'static,
            R: Future<Output = Result<T, E>> + Send + 'static
        {
            let child_tasks: Vec<(EcoString, Task<_>)> = children.into_iter().map(|(k, v)| {
                (k, ex.spawn(map_entity(ex.clone(), v, f.clone())))
            }).collect();

            stream::iter(child_tasks)
                .then(|(k, f_v)| async move { Ok::<_, E>((k, f_v.await?)) })
                .try_collect().await
        }

        async fn map_summary<S, T, E, F, R>(
            ex: Arc<Executor<'static>>,
            summary: Summary<S>,
            f: F
        ) -> Result<Summary<T>, E> where
            S: Send + 'static,
            T: Send + 'static,
            E: Send + 'static,
            F: Fn(S) -> R + Send + Clone + 'static,
            R: Future<Output = Result<T, E>> + Send + 'static
        {
            let Summary { base_level, levels } = summary;
            let child_tasks: Vec<Task<_>> = levels.into_iter().map(|s| {
                ex.spawn(f(s))
            }).collect();

            let levels = stream::iter(child_tasks).then(|f| f).try_collect().await?;
            Ok(Summary { base_level, levels })
        }

        async fn map_summaries<S, T, E, F, R>(ex: Arc<Executor<'static>>, summaries: IndexMap<EcoString, Summary<S>>, f: F) -> Result<IndexMap<EcoString, Summary<T>>, E>
        where
            S: Send + 'static,
            T: Send + 'static,
            E: Send + 'static,
            F: Fn(S) -> R + Send + Clone + 'static,
            R: Future<Output = Result<T, E>> + Send + 'static
        {
            let child_tasks: Vec<(EcoString, Task<_>)> = summaries.into_iter().map(|(k, v)| {
                (k, ex.spawn(map_summary(ex.clone(), v, f.clone())))
            }).collect();

            stream::iter(child_tasks)
                .then(|(k, f_v)| async move { Ok::<_, E>((k, f_v.await?)) })
                .try_collect().await
        }

        fn map_entity<S, T, E, F, R>(
            ex: Arc<Executor<'static>>,
            schema: Entity<S>,
            f: F
        ) -> impl Future<Output = Result<Entity<T>, E>> + Send where
            S: Send + 'static,
            T: Send + 'static,
            E: Send + 'static,
            F: Fn(S) -> R + Send + Clone + 'static,
            R: Future<Output = Result<T, E>> + Send + 'static
        {
            async move {
                match schema {
                    Entity::Group { children, attributes } => {
                        let children = map_children(ex, children, f).await?;
                        Ok(Entity::Group { children, attributes: attributes.clone() })
                    }
                    Entity::Record { children, attributes } => {
                        let children = map_children(ex, children, f).await?;
                        Ok(Entity::Record { children, attributes: attributes.clone() })
                    }
                    Entity::Data { data, ref field, summaries } => {
                        let (data, summaries) = future::try_zip(
                            f(data),
                            map_summaries(ex, summaries, f),
                        ).await?;
                        Ok(Entity::Data { field: field.clone(), data, summaries })
                    }
                    Entity::Union { data, variants, attributes } => {
                        let (data, variants) = future::try_zip(
                            f(data),
                            map_children(ex, variants, f)
                        ).await?;
                        Ok(Entity::Union { data, variants, attributes: attributes.clone() })
                    }
                    Entity::FixedArray { elements, child, attributes } => {
                        let child = Box::new(ex.spawn(map_entity(ex.clone(), *child, f)).await?);
                        Ok(Entity::FixedArray { elements, child, attributes: attributes.clone() })
                    }
                    Entity::Tuple { fields, child, attributes } => {
                        let child = Box::new(ex.spawn(map_entity(ex.clone(), *child, f)).await?);
                        Ok(Entity::Tuple { fields, child, attributes: attributes.clone() })
                    }
                    Entity::VariableArray { data, child, attributes } => {
                        let (data, child) = future::try_zip(
                            f(data),
                            ex.spawn(map_entity(ex.clone(), *child, f))
                        ).await?;
                        Ok(Entity::VariableArray { data, child: Box::new(child), attributes: attributes.clone() })
                    }
                }
            }
        }

        executor.run(map_entity(executor.clone(), self, f)).await
    }

    pub fn schema(&self) -> EntitySchema {
        self.try_map_data(&mut |_| Ok::<Ignored, Infallible>(Ignored)).unwrap()
    }
}

impl EntitySchema {
    pub fn bytes() -> Self {
        Self::field(FieldKind::Bits { bits: 8 })
    }

    pub fn logic8() -> Self {
        let children = (0..8).map(|b| (
            eco_format!("bit{b}"),
            Field {
                attributes: Default::default(),
                kind: FieldKind::Bits { bits: 1 },
            }
        )).collect();
        Self::field(FieldKind::BitStruct { children })
    }

    pub fn field(kind: FieldKind) -> Self {
        Entity::Data {
            data: Ignored,
            field: Field {
                kind,
                attributes: Default::default(),
            },
            summaries: Default::default(),
        }
    }

    pub fn single_stream(&self) -> Option<(&Field, usize)> {
        match self {
            Entity::Group { .. } | Entity::Record { .. } | Entity::Union { .. } | Entity::VariableArray { .. } => None,
            Entity::FixedArray { elements, child, .. } => {
                let (field, stride) = child.single_stream()?;
                Some((field, stride * (*elements as usize)))
            }
            Entity::Tuple { fields, child, .. } => {
                let (field, stride) = child.single_stream()?;
                Some((field, stride * fields.len()))
            }
            Entity::Data { data: Ignored, field, .. } => Some((field, 1)),
        }
    }

    pub fn wrap_single(&self, data: ArcStream) -> Option<EntityStream> {
        match *self {
            Entity::Group { .. } | Entity::Record { .. } | Entity::Union { .. } | Entity::VariableArray { .. } => None,
            Entity::Data { data: Ignored, ref field, .. } => {
                Some(Entity::Data { data, field: field.clone(), summaries: Default::default() })
            }
            Entity::FixedArray { ref child, elements, ref attributes } => {
                let child = Box::new(child.wrap_single(data)?);
                Some(Entity::FixedArray { elements, child, attributes: attributes.clone() })
            }
            Entity::Tuple { ref fields, ref child, ref attributes } => {
                let child = Box::new(child.wrap_single(data)?);
                Some(Entity::Tuple { fields: fields.clone(), child, attributes: attributes.clone() })
            }
        }
    }
}

impl EntityStream {
    pub fn build_summaries(&mut self, executor: &Executor<'static>, storage: &dyn Storage) {
        match *self {
            Entity::Group { ref mut children, .. } | Entity::Record { ref mut children, .. } => {
                for child in children.values_mut() {
                    child.build_summaries(executor, storage);
                }
            }
            Entity::Union { ref mut variants, .. } => {
                for variant in variants.values_mut() {
                    variant.build_summaries(executor, storage);
                }
            }
            Entity::FixedArray { ref mut child, .. } | Entity::Tuple { ref mut child, .. } => {
                child.build_summaries(executor, storage);
            }
            Entity::VariableArray { ref mut child, .. } => {
                child.build_summaries(executor, storage);
            }
            Entity::Data { ref mut summaries, ref field, ref data } => {
                crate::summary::build_default_summaries(executor, storage, data, field, summaries);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary<S> {
    pub base_level: u8,
    pub levels: Vec<S>,
}

impl<S> Summary<S> {
    pub const fn empty() -> Self {
        Summary { base_level: 255, levels: Vec::new() }
    }

    fn try_map_data<T, E>(&self, f: &mut impl FnMut(&S) -> Result<T, E>) -> Result<Summary<T>, E> {
        let base_level = self.base_level;
        let levels = self.levels.iter().map(|level| f(level)).collect::<Result<Vec<_>, _>>()?;
        Ok(Summary { base_level, levels })
    }
}
