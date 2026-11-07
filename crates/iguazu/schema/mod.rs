//! Types for signal metadata
use std::{fmt::Debug, ops::Deref};

use attribute::AttributeValue;
use ecow::{eco_format, EcoString};
use futures_lite::{StreamExt, future};
use indexmap::IndexMap;
use jiff::Zoned;
use serde::{Deserialize, Serialize};
use num_traits::Zero;

pub mod attribute;
pub use attribute::AttributeMap;

pub mod fmt;

pub mod json_virtual;

use crate::{ElementSize, schema::attribute::{Attribute, core::ROLE}, send::{NonWasmSend, NonWasmSync}, stream::ArcStream, summary::{LiveSummary, LiveSummaryMap, StoredSummary, StoredSummaryMap}};

/// Types that can be used as the data of an `Entity`.
pub trait EntityData: Sized + Clone + NonWasmSend + NonWasmSync + 'static {
    fn skip_serialize(&self) -> bool { false }
}

pub trait IsEmpty {
    fn is_empty(&self) -> bool;
}

impl IsEmpty for Ignored {
    fn is_empty(&self) -> bool {
        true
    }
}

impl<K, V> IsEmpty for IndexMap<K, V> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl EntityData for ArcStream {}

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

impl EntityData for Ignored {
    fn skip_serialize(&self) -> bool { true }
}

/// Schema of an entity with only metadata.
pub type EntitySchema = Entity<Ignored, Ignored>;

/// Entity with metadata associated data streams and summaries.
pub type EntityStream = Entity<ArcStream, LiveSummaryMap>;

/// Generic entity type
///
/// See [`EntitySchema`] and [`EntityStream`] for common type aliases.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all="snake_case")]
pub enum Entity<D, S> {
    Group {
        children: IndexMap<EcoString, Entity<D, S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    FixedArray {
        elements: u32,
        inner: Box<Entity<D, S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    Tuple {
        names: Vec<EcoString>,
        inner: Box<Entity<D, S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    VariableArray {
        #[serde(skip_serializing_if = "EntityData::skip_serialize",  bound(serialize = "D: Serialize + EntityData", deserialize = "D: Deserialize<'de> + EntityData"))]
        data: D,
        inner: Box<Entity<D, S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    #[serde(untagged)]
    Data {
        #[serde(flatten)]
        field: Field,

        #[serde(skip_serializing_if = "EntityData::skip_serialize")]
        data: D,

        #[serde(skip_serializing_if = "IsEmpty::is_empty", default = "Default::default", bound(serialize = "S: Serialize + IsEmpty", deserialize = "S: Deserialize<'de> + Default"))]
        summaries: S,
    }
}

/// Field defining the intepretation of bits within each element of s stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    #[serde(flatten)]
    pub kind: FieldKind,

    #[serde(flatten)]
    pub attributes: AttributeMap,
}

/// Type of a [`Field`], defining the interpretation of bits within each element of a stream
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all="snake_case")]
pub enum FieldKind {
    Null,
    Bits {
        #[serde(default, skip_serializing_if = "Zero::is_zero")]
        pos: u8,
        bits: u8,
    },
    Character {
        #[serde(default, skip_serializing_if = "Zero::is_zero")]
        pos: u8
    },
    Timestamp,
    Int {
        #[serde(default, skip_serializing_if = "Zero::is_zero")]
        pos: u8,
        bits: u8,
    },
    Signed {
        #[serde(default, skip_serializing_if = "Zero::is_zero")]
        pos: u8,
        bits: u8,
    },
    Float32 {
        #[serde(default, skip_serializing_if = "Zero::is_zero")]
        pos: u8
    },
    Float64,
    Enum {
        #[serde(default, skip_serializing_if = "Zero::is_zero")]
        pos: u8,
        bits: u8,
        values: Vec<EcoString>,
        #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
        variants: IndexMap<EcoString, Field>,
    },
    BitStruct {
        children: IndexMap<EcoString, Field>,
    },
}

impl FieldKind {
    pub fn mask(&self) -> u64 {
        match *self {
            FieldKind::Null => 0,
            FieldKind::Bits { pos, bits, .. }
            | FieldKind::Int { pos, bits, .. }
            | FieldKind::Signed { pos, bits, .. } => ((1u64 << bits) - 1) << pos,
            FieldKind::Character { pos, .. } => 0xFF << pos,
            FieldKind::Timestamp { .. } => u64::MAX,
            FieldKind::Float32 { pos } => (u32::MAX as u64) << pos,
            FieldKind::Float64 => u64::MAX,
            FieldKind::Enum { pos, bits, ref variants, .. } => {
                let tag_mask = ((1u64 << bits) - 1) << pos;
                let inner_mask = variants.values().map(|v| v.kind.mask()).fold(0, |a, b| a | b);
                tag_mask | inner_mask
            }
            FieldKind::BitStruct { ref children } => {
                children.values().map(|f| f.kind.mask()).fold(0, |a, b| a | b)
            }
        }
    }

    pub fn element_size(&self) -> ElementSize {
        ElementSize::from_bits((u64::BITS - self.mask().leading_zeros()) as u8).unwrap()
    }
}

impl Into<Field> for FieldKind {
    fn into(self) -> Field {
        Field::new(self)
    }
}

impl Field {
    pub fn new(kind: FieldKind) -> Self {
        Field { kind, attributes: Default::default() }
    }

    pub fn bits(bits: u8) -> Self {
        Self::new(FieldKind::Bits { pos: 0, bits })
    }

    pub fn byte() -> Self {
        Self::bits(8)
    }

    pub fn r#enum(values: impl IntoIterator<Item = EcoString>) -> Self {
        let values: Vec<_> = values.into_iter().collect();
        let bits = (usize::BITS - values.len().leading_zeros()) as u8;
        Self::new(FieldKind::Enum { pos: 0, bits, values, variants: Default::default() })
    }

    pub fn character() -> Self {
        Self::new(FieldKind::Character { pos: 0 })
    }

    pub fn logic(bits: u8) -> Field {
        let children = (0..bits).map(|b| (
            eco_format!("bit{b}"),
            Field::new(FieldKind::Bits { bits: 1, pos: b })
                .with_attribute(attribute::display::COLOR, attribute::display::AccentColor::from_bit_position(b))
        )).collect();

        Field::new(FieldKind::BitStruct { children })
    }

    pub fn unsigned(bits: u8) -> Self {
        Self::new(FieldKind::Int { pos: 0, bits })
    }

    pub fn signed(bits: u8) -> Self {
        Self::new(FieldKind::Signed { pos: 0, bits })
    }

    pub fn float32() -> Self {
        Self::new(FieldKind::Float32 { pos: 0 })
    }

    pub fn float64() -> Self {
        Self::new(FieldKind::Float64)
    }

    pub fn float(bits: u8) -> Option<Self> {
        match bits {
            32 => Some(Self::float32()),
            64 => Some(Self::float64()),
            _ => None,
        }
    }

    pub fn timestamp(tick: f64, epoch: Option<Zoned>) -> Self {
        Self::new(FieldKind::Timestamp)
            .with_attribute(attribute::core::TIME_TICK, tick)
            .with_attribute_opt(attribute::core::TIME_EPOCH, epoch)
    }

    pub fn attribute<'a, A: TryFrom<&'a AttributeValue>>(&'a self, attr: Attribute<A>) -> Option<A> {
        self.attributes.get(attr.into())
    }

    pub fn set_attribute<A: Into<AttributeValue>>(&mut self, attr: Attribute<A>, val: A) {
        self.attributes.insert(attr.into(), val);
    }

    pub fn with_attribute<A: Into<AttributeValue>>(mut self, attr: Attribute<A>, val: A) -> Self {
        self.set_attribute(attr, val);
        self
    }

    pub fn with_attribute_opt<A: Into<AttributeValue>>(mut self, attr: Attribute<A>, val: Option<A>) -> Self {
        if let Some(val) = val {
            self.set_attribute(attr, val);
        }
        self
    }

    pub fn child(&self, child: &str) -> Option<&Field> {
        match self.kind {
            FieldKind::BitStruct { ref children, .. } => {
                children.get(child)
            }
            _ => None,
        }
    }
}

impl<D, S> Entity<D, S> {
    pub fn record(children: impl IntoIterator<Item = (EcoString, Entity<D, S>)>) -> Self {
        Entity::Group { children: IndexMap::from_iter(children), attributes: Default::default() }
            .with_attribute(attribute::core::ROLE, attribute::core::Role::Record)
    }

    pub fn group() -> Self {
        Entity::Group { children: IndexMap::new(), attributes: Default::default() }
    }

    pub fn field_data(field: impl Into<Field>, data: D) -> Self where S: Default {
        Entity::Data {
            data,
            field: field.into(),
            summaries: Default::default(),
        }
    }

    pub fn attributes(&self) -> &AttributeMap {
        match self {
            Entity::Group { attributes, .. }
            | Entity::FixedArray { attributes, .. }
            | Entity::Tuple { attributes, .. }
            | Entity::VariableArray { attributes, .. } => attributes,
            Entity::Data { field, .. } => &field.attributes,
        }
    }

    pub fn attributes_mut(&mut self) -> &mut AttributeMap {
        match self {
            Entity::Group { attributes, .. }
            | Entity::FixedArray { attributes, .. }
            | Entity::Tuple { attributes, .. }
            | Entity::VariableArray { attributes, .. } => attributes,
            Entity::Data { field, .. } => &mut field.attributes,
        }
    }

    pub fn tuple(inner: Entity<D, S>, names: Vec<EcoString>) -> Self {
        Entity::Tuple { inner: Box::new(inner), names, attributes: Default::default() }
    }

    pub fn attribute<'a, A: TryFrom<&'a AttributeValue>>(&'a self, attr: Attribute<A>) -> Option<A> {
        self.attributes().get(attr.into())
    }

    pub fn remove_attribute<A: Into<AttributeValue>>(&mut self, attr: Attribute<A>) {
        self.attributes_mut().remove(attr.into());
    }

    pub fn set_attribute<A: Into<AttributeValue>>(&mut self, attr: Attribute<A>, val: A) {
        self.attributes_mut().insert(attr.into(), val);
    }

    pub fn with_attribute<A: Into<AttributeValue>>(mut self, attr: Attribute<A>, val: A) -> Self {
        self.set_attribute(attr, val);
        self
    }

    pub fn with_attribute_opt<A: Into<AttributeValue>>(self, attr: Attribute<A>, val: Option<A>) -> Self {
        match val {
            Some(val) => self.with_attribute(attr, val),
            None => self
        }
    }

    pub fn child(&self, child: &str) -> Option<&Entity<D, S>> {
        match *self {
            Entity::Group { ref children, .. } => {
                children.get(child)
            }
            Entity::FixedArray { ref inner, .. }
            | Entity::Tuple { ref inner, .. }
            | Entity::VariableArray { ref inner, .. } => {
                Some(inner)
            }
            _ => None,
        }
    }

    pub fn child_mut<'a>(&'a mut self, child: &str) -> Option<&'a mut Entity<D, S>> {
        match *self {
            Entity::Group { ref mut children, .. } => {
                children.get_mut(child)
            }
            Entity::FixedArray { ref mut inner, .. }
            | Entity::Tuple { ref mut inner, .. }
            | Entity::VariableArray { ref mut inner, .. } => {
                Some(inner)
            }
            _ => None,
        }
    }

    pub fn child_owned(self, child: &str) -> Option<Self> {
        match self {
            Entity::Group { mut children, .. } => {
                children.swap_remove(child)
            }
            Entity::FixedArray { inner, .. }
            | Entity::Tuple { inner, .. }
            | Entity::VariableArray { inner, .. } => {
                Some(*inner)
            }
            _ => None,
        }
    }

    pub fn with_child(mut self, name: EcoString, child: Entity<D, S>) -> Self {
        match &mut self {
            Entity::Group { children, .. } => {
                children.insert(name, child);
            }
            _ => panic!("Cannot add child to non-group or non-record entity"),
        }
        self
    }

    pub fn select<'a>(&self, path: impl IntoIterator<Item = impl AsRef<str>>) -> Option<&Entity<D, S>> {
        let mut current = self;
        for part in path {
            current = current.child(part.as_ref())?;
        }
        Some(current)
    }

    pub fn select_mut<'a>(&mut self, path: impl IntoIterator<Item = impl AsRef<str>>) -> Option<&mut Entity<D, S>> {
        let mut current = self;
        for part in path {
            current = current.child_mut(part.as_ref())?;
        }
        Some(current)
    }

    pub fn select_owned<'a>(self, path: impl IntoIterator<Item = impl AsRef<str>>) -> Option<Self> {
        let mut current = self;
        for part in path {
            current = current.child_owned(part.as_ref())?;
        }
        Some(current)
    }

    pub fn data(&self) -> Option<&D> {
        match self {
            Entity::Data { data, .. } => Some(data),
            Entity::VariableArray { data, .. } => Some(data),
            _ => None,
        }
    }

    pub fn schema(&self) -> EntitySchema {
        match self {
            Entity::Group { children, attributes } => {
                let children = children.iter().map(|(name, child)| (name.clone(), child.schema())).collect();
                Entity::Group { children, attributes: attributes.clone() }
            }
            Entity::FixedArray { elements, inner, attributes } => {
                Entity::FixedArray { elements: *elements, inner: Box::new(inner.schema()), attributes: attributes.clone() }
            }
            Entity::Tuple { names, inner, attributes } => {
                Entity::Tuple { names: names.clone(), inner: Box::new(inner.schema()), attributes: attributes.clone() }
            }
            Entity::VariableArray { inner, attributes, .. } => {
                Entity::VariableArray { data: Ignored, inner: Box::new(inner.schema()), attributes: attributes.clone() }
            }
            Entity::Data { field, .. } => {
                Entity::Data { data: Ignored, field: field.clone(), summaries: Ignored }
            }
        }
    }
}

impl<D: EntityData> Entity<D, StoredSummaryMap<D>> {
    pub async fn try_map_data_async<E, F, R>(&self, f: &F) -> Result<EntityStream, E>
    where
        D: NonWasmSend + NonWasmSync + 'static,
        E: Send + 'static,
        F: Fn(&D) -> R + NonWasmSend + NonWasmSync + Clone,
        R: Future<Output = Result<ArcStream, E>> + NonWasmSend,
    {
        async fn map_summaries<D, E, F, R>(summaries: &StoredSummaryMap<D>, f: &F) -> Result<LiveSummaryMap, E>
        where
            D: NonWasmSend + NonWasmSync + 'static,
            E: Send + 'static,
            F: Fn(&D) -> R + NonWasmSend + NonWasmSync + Clone,
            R: Future<Output = Result<ArcStream, E>> + NonWasmSend
        {
            let summaries = futures_util::future::try_join_all(summaries.iter().map(|(k, v)| async move {
                let levels = futures_util::future::try_join_all(v.levels.iter().map(f)).await?;
                Ok((k.clone(), LiveSummary::new(v.first_level, levels)))
            })).await?;
            Ok(summaries.into_iter().collect())
        }

        match *self {
            Entity::Group { ref children, ref attributes } => {
                let children = futures_util::future::try_join_all(children.iter().map(|(k, v)| async {
                    Ok::<_, E>((k.clone(), v.try_map_data_async(f).await?))
                })).await?.into_iter().collect();
                Ok(Entity::Group { children, attributes: attributes.clone() })
            }
            Entity::Data { ref data, ref field, ref summaries } => {
                let (data, summaries) = future::try_zip(
                    f(data),
                    map_summaries(summaries, f),
                ).await?;
                Ok(Entity::Data { field: field.clone(), data, summaries })
            }
            Entity::FixedArray { elements, ref inner, ref attributes } => {
                let inner = Box::new(Box::pin(inner.try_map_data_async(f)).await?);
                Ok(Entity::FixedArray { elements, inner, attributes: attributes.clone() })
            }
            Entity::Tuple { ref names, ref inner, ref attributes } => {
                let inner = Box::new(Box::pin(inner.try_map_data_async(f)).await?);
                Ok(Entity::Tuple { names: names.clone(), inner, attributes: attributes.clone() })
            }
            Entity::VariableArray { ref data, ref inner, ref attributes } => {
                let (data, inner) = future::try_zip(
                    f(data),
                    Box::pin(inner.try_map_data_async(f))
                ).await?;
                Ok(Entity::VariableArray { data, inner: Box::new(inner), attributes: attributes.clone() })
            }
        }
    }

    pub fn each_data_mut(&mut self, mut f: &mut impl FnMut(&mut D)) {
        match self {
            Entity::Group { children, .. } => {
                for child in children.values_mut() {
                    child.each_data_mut(f);
                }
            }
            Entity::FixedArray { inner, .. } | Entity::Tuple { inner, .. } => {
                inner.each_data_mut(f)
            }
            Entity::VariableArray { data, inner, .. } => {
                f(data);
                inner.each_data_mut(f);
            }
            Entity::Data { data, summaries, .. } => {
                f(data);
                for s in summaries.values_mut() {
                    s.levels.iter_mut().for_each(&mut f);
                }
            }
        }
    }
}

impl EntitySchema {
    pub fn bits(bits: u8) -> Self {
        Self::field(FieldKind::Bits { bits, pos: 0 })
    }

    pub fn bytes() -> Self {
        Self::field(FieldKind::Bits { bits: 8, pos: 0 })
    }

    pub fn field(field: impl Into<Field>) -> Self {
        Entity::Data {
            data: Ignored,
            field: field.into(),
            summaries: Default::default(),
        }
    }

    pub fn string() -> Self {
        Entity::VariableArray {
            data: Ignored,
            inner: Box::new(Entity::field(Field::character())),
            attributes: Default::default()
        }
    }

    pub fn complex(field: impl Into<Field>) -> Self {
        let data = Self::field(field);
        Entity::Tuple {
            names: vec!["re".into(), "im".into()],
            inner: Box::new(data),
            attributes: Default::default(),
        }.with_attribute(ROLE, attribute::core::Role::Complex)
    }

    pub fn single_stream(&self) -> Option<(&Field, usize)> {
        match self {
            Entity::Group { .. } | Entity::VariableArray { .. } => None,
            Entity::FixedArray { elements, inner, .. } => {
                let (field, stride) = inner.single_stream()?;
                Some((field, stride * (*elements as usize)))
            }
            Entity::Tuple { names, inner, .. } => {
                let (field, stride) = inner.single_stream()?;
                Some((field, stride * names.len()))
            }
            Entity::Data { data: Ignored, field, .. } => Some((field, 1)),
        }
    }

    pub fn wrap_single(&self, data: ArcStream) -> Option<EntityStream> {
        match *self {
            Entity::Group { .. } | Entity::VariableArray { .. } => None,
            Entity::Data { data: Ignored, ref field, .. } => {
                Some(Entity::Data { data, field: field.clone(), summaries: Default::default() })
            }
            Entity::FixedArray { ref inner, elements, ref attributes } => {
                let inner = Box::new(inner.wrap_single(data)?);
                Some(Entity::FixedArray { elements, inner, attributes: attributes.clone() })
            }
            Entity::Tuple { ref names, ref inner, ref attributes } => {
                let inner = Box::new(inner.wrap_single(data)?);
                Some(Entity::Tuple { names: names.clone(), inner, attributes: attributes.clone() })
            }
        }
    }
}

impl EntityStream {
    pub fn as_field(&self) -> Option<FieldRef<'_>> {
        match self {
            Entity::Data { data, field, summaries } => Some(FieldRef { data, field, summaries }),
            _ => None,
        }
    }

    pub async fn try_map_data_async<D, E, F, R>(&self, f: &F) -> Result<Entity<D, StoredSummaryMap<D>>, E>
    where
        D: NonWasmSend + NonWasmSync + 'static,
        E: NonWasmSend + 'static,
        F: Fn(ArcStream) -> R + NonWasmSend + NonWasmSync + Clone,
        R: Future<Output = Result<D, E>> + NonWasmSend,
    {
        async fn map_summaries<D, E, F, R>(summaries: &LiveSummaryMap, f: &F) -> Result<StoredSummaryMap<D>, E>
        where
            D: NonWasmSend + NonWasmSync + 'static,
            E: NonWasmSend + 'static,
            F: Fn(ArcStream) -> R + NonWasmSend + NonWasmSync + Clone,
            R: Future<Output = Result<D, E>> + NonWasmSend
        {
            let summaries = futures_util::future::try_join_all(summaries.iter().map(|(k, v)| async move {
                let level_futures = v.async_iter().map(|s| f(s)).collect::<Vec<_>>().await;
                let levels = futures_util::future::try_join_all(level_futures).await?;
                Ok((k.clone(), StoredSummary { first_level: v.first_level(), levels: levels.into_boxed_slice() }))
            })).await?;
            Ok(summaries.into_iter().collect())
        }

        match *self {
            Entity::Group { ref children, ref attributes } => {
                let children = futures_util::future::try_join_all(children.iter().map(|(k, v)| async {
                    Ok::<_, E>((k.clone(), v.try_map_data_async(f).await?))
                })).await?.into_iter().collect();
                Ok(Entity::Group { children, attributes: attributes.clone() })
            }
            Entity::Data { ref data, ref field, ref summaries } => {
                let (data, summaries) = future::try_zip(
                    f(data.clone()),
                    map_summaries(summaries, f),
                ).await?;
                Ok(Entity::Data { field: field.clone(), data, summaries })
            }
            Entity::FixedArray { elements, ref inner, ref attributes } => {
                let inner = Box::new(Box::pin(inner.try_map_data_async(f)).await?);
                Ok(Entity::FixedArray { elements, inner, attributes: attributes.clone() })
            }
            Entity::Tuple { ref names, ref inner, ref attributes } => {
                let inner = Box::new(Box::pin(inner.try_map_data_async(f)).await?);
                Ok(Entity::Tuple { names: names.clone(), inner, attributes: attributes.clone() })
            }
            Entity::VariableArray { ref data, ref inner, ref attributes } => {
                let (data, inner) = future::try_zip(
                    f(data.clone()),
                    Box::pin(inner.try_map_data_async(f))
                ).await?;
                Ok(Entity::VariableArray { data, inner: Box::new(inner), attributes: attributes.clone() })
            }
        }
    }
}

/// Reference to a field within a stream.
#[derive(Clone, Copy)]
pub struct FieldRef<'a> {
    /// Stream containing the data.
    pub data: &'a ArcStream,

    /// The field defining the interpretation of bits.
    pub field: &'a Field,

    /// The summaries at the level of the `Entity::Data` containing this field.
    pub summaries: &'a LiveSummaryMap,
}

impl Deref for FieldRef<'_> {
    type Target = Field;

    fn deref(&self) -> &Self::Target {
        self.field
    }
}

impl FieldRef<'_> {
    pub fn bit_struct_fields(&self) -> Option<impl Iterator<Item = (EcoString, FieldRef<'_>)>> {
        match self.kind {
            FieldKind::BitStruct { ref children } => {
                Some(children.iter().map(move |(name, field)| {
                    (name.clone(), FieldRef { field, ..*self })
                }))
            }
            _ => None,
        }
    }
}
