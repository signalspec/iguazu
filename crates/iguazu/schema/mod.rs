//! Types for signal metadata
use std::{convert::Infallible, fmt::Debug, ops::Deref};

use attribute::AttributeValue;
use ecow::{eco_format, EcoString};
use futures_lite::future;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use num_traits::Zero;

pub mod attribute;
pub use attribute::AttributeMap;

pub mod fmt;

pub mod json_virtual;

use crate::{ElementSize, schema::attribute::Attribute, stream::ArcStream, summary::{BorrowedSummary, LiveSummaryMap, StoredSummary, StoredSummaryMap, Summary, SummaryMap}};

/// Types that can be used as the data of an `Entity`.
pub trait EntityData: Sized + Clone + Send + Sync + 'static {
    type SummaryMap: SummaryMap<Data = Self>;
    fn skip_serialize(&self) -> bool { false }
}

impl EntityData for ArcStream {
    type SummaryMap = LiveSummaryMap;
}

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
    type SummaryMap = Ignored;

    fn skip_serialize(&self) -> bool {
        true
    }
}

impl SummaryMap for Ignored {
    type Data = Ignored;

    fn iter(&self) -> impl Iterator<Item = (EcoString, BorrowedSummary<'_, Self::Data>)> {
        std::iter::empty()
    }

    fn is_empty(&self) -> bool {
        true
    }
}

impl FromIterator<(EcoString, StoredSummary<Ignored>)> for Ignored {
    fn from_iter<T: IntoIterator<Item = (EcoString, StoredSummary<Ignored>)>>(_iter: T) -> Self {
        Ignored
    }
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
    Record {
        children: IndexMap<EcoString, Entity<D, S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    Union {
        #[serde(skip_serializing_if = "EntityData::skip_serialize", bound(serialize = "D: Serialize + EntityData", deserialize = "D: Deserialize<'de> + EntityData"))]
        data: D,

        variants: IndexMap<EcoString, Entity<D, S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    FixedArray {
        elements: u32,
        child: Box<Entity<D, S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    Tuple {
        fields: IndexMap<EcoString, AttributeMap>,
        child: Box<Entity<D, S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    VariableArray {
        #[serde(skip_serializing_if = "EntityData::skip_serialize",  bound(serialize = "D: Serialize + EntityData", deserialize = "D: Deserialize<'de> + EntityData"))]
        data: D,
        child: Box<Entity<D, S>>,

        #[serde(flatten)]
        attributes: AttributeMap,
    },
    #[serde(untagged)]
    Data {
        #[serde(flatten)]
        field: Field,

        #[serde(skip_serializing_if = "EntityData::skip_serialize")]
        data: D,

        #[serde(skip_serializing_if = "SummaryMap::is_empty", default = "Default::default", bound(serialize = "S: Serialize + SummaryMap", deserialize = "S: Deserialize<'de> + SummaryMap + Default"))]
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
    },
    Tagged {
        #[serde(default, skip_serializing_if = "Zero::is_zero")]
        tag_pos: u8,
        tag_bits: u8,
        values: IndexMap<EcoString, Field>,
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
            | FieldKind::Signed { pos, bits, .. }
            | FieldKind::Enum { pos, bits, .. } => ((1u64 << bits) - 1) << pos,
            FieldKind::Character { pos, .. } => 0xFF << pos,
            FieldKind::Timestamp { .. } => u64::MAX,
            FieldKind::Float32 { pos } => (u32::MAX as u64) << pos,
            FieldKind::Float64 => u64::MAX,
            FieldKind::Tagged { tag_pos, tag_bits, ref values, .. } => {
                let tag_mask = ((1u64 << tag_bits) - 1) << tag_pos;
                let inner_mask = values.values().map(|v| v.kind.mask()).fold(0, |a, b| a | b);
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

    pub fn logic(bits: u8) -> Field {
        let children = (0..bits).map(|b| (
            eco_format!("bit{b}"),
            Field {
                attributes: Default::default(),
                kind: FieldKind::Bits { bits: 1, pos: b },
            }
        )).collect();

        Field::new(FieldKind::BitStruct { children })
    }

    pub fn unsigned(bits: u8) -> Self {
        Self::new(FieldKind::Int { pos: 0, bits })
    }

    pub fn signed(bits: u8) -> Self {
        Self::new(FieldKind::Signed { pos: 0, bits })
    }

    pub fn float(bits: u8) -> Option<Self> {
        match bits {
            32 => Some(Self::new(FieldKind::Float32 { pos: 0 })),
            64 => Some(Self::new(FieldKind::Float64)),
            _ => None,
        }
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
    pub fn record() -> Self {
        Entity::Record { children: IndexMap::new(), attributes: Default::default() }
    }

    pub fn group() -> Self {
        Entity::Group { children: IndexMap::new(), attributes: Default::default() }
    }

    pub fn field_data(kind: FieldKind, data: D) -> Self where S: Default {
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

    pub fn tuple(child: Entity<D, S>, fields: IndexMap<EcoString, AttributeMap>) -> Self {
        Entity::Tuple { child: Box::new(child), fields, attributes: Default::default() }
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

    pub fn child(&self, child: &str) -> Option<&Entity<D, S>> {
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

    pub fn child_mut(&mut self, child: &str) -> Option<&mut Entity<D, S>> {
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

    pub fn with_child(mut self, name: EcoString, child: Entity<D, S>) -> Self {
        match &mut self {
            Entity::Group { children, .. }
            | Entity::Record { children, .. } => {
                children.insert(name, child);
            }
            _ => panic!("Cannot add child to non-group or non-record entity"),
        }
        self
    }

    pub fn select(&self, path: &str) -> Option<&Entity<D, S>> {
        let mut current = self;
        for part in path.split('.') {
            current = current.child(part)?;
        }
        Some(current)
    }

    pub fn select_mut(&mut self, path: &str) -> Option<&mut Entity<D, S>> {
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
}

impl<D: EntityData, S: SummaryMap<Data = D>> Entity<D, S> {
    pub fn data(&self) -> Option<&D> {
        match self {
            Entity::Data { data, .. } => Some(data),
            Entity::Union { data, .. } => Some(data),
            Entity::VariableArray { data, .. } => Some(data),
            _ => None,
        }
    }

    pub fn try_map_data<T: EntityData + Send, E: Send>(&self, f: &mut impl FnMut(&D) -> Result<T, E>) -> Result<Entity<T, T::SummaryMap>, E> {
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
                        let levels = summary.levels.iter().map(&mut *f).collect::<Result<Vec<_>, E>>()?;
                        Ok((name.clone(), Summary { base_level: summary.base_level, levels: levels.into_boxed_slice() }))
                    })
                    .collect::<Result<T::SummaryMap, E>>()?;
                Ok(Entity::Data { data, field: field.clone(), summaries })
            }
        }
    }

    pub async fn try_map_data_async<T, TM, E, F, R>(&self, f: F) -> Result<Entity<T, TM>, E>
    where
        D: Send + Sync + 'static,
        S: Send + Sync + 'static,
        T: EntityData<SummaryMap = TM> + Send + Sync + 'static,
        TM: SummaryMap<Data = T> + Send + Sync + 'static,
        E: Send + 'static,
        F: Fn(&D) -> R + Send + Sync + Clone,
        R: Future<Output = Result<T, E>> + Send,
    {
        async fn map_children<D, S, T, TM, E, F, R>(
            children: &IndexMap<EcoString, Entity<D, S>>,
            f: &F
        ) -> Result<IndexMap<EcoString, Entity<T, TM>>, E> where
            D: EntityData + Send + Sync + 'static,
            S: SummaryMap<Data = D> + Send + Sync + 'static,
            T: EntityData + Send + Sync + 'static,
            TM: SummaryMap<Data = T> + Send + Sync + 'static,
            E: Send + 'static,
            F: Fn(&D) -> R + Send + Sync + Clone,
            R: Future<Output = Result<T, E>> + Send
        {
            Ok(futures_util::future::try_join_all(children.iter().map(|(k, v)| async {
                let v = map_entity(v, f).await?;
                Ok::<_, E>((k.clone(), v))
            })).await?.into_iter().collect())
        }

        async fn map_summaries<D, S, T, TM, E, F, R>(summaries: &S, f: &F) -> Result<TM, E>
        where
            D: EntityData + Send + Sync + 'static,
            S: SummaryMap<Data = D> + Send + Sync + 'static,
            T: EntityData + Send + Sync + 'static,
            TM: SummaryMap<Data = T> + Send + Sync + 'static,
            E: Send + 'static,
            F: Fn(&D) -> R + Send + Sync + Clone,
            R: Future<Output = Result<T, E>> + Send
        {
            let summaries: Vec<(EcoString, StoredSummary<T>)> = futures_util::future::try_join_all(summaries.iter().map(|(k, v)| async move {
                let levels: Vec<T> = futures_util::future::try_join_all(v.levels.iter().map(f)).await?;
                Ok((k.clone(), Summary { base_level: v.base_level, levels: levels.into_boxed_slice() }))
            })).await?;
            Ok(summaries.into_iter().collect())
        }

        fn map_entity<D, S, T, TM, E, F, R>(
            schema: &Entity<D, S>,
            f: &F
        ) -> impl Future<Output = Result<Entity<T, TM>, E>> + Send where
            D: EntityData + Send + Sync + 'static,
            S: SummaryMap<Data = D> + Send + Sync + 'static,
            T: EntityData + Send + Sync + 'static,
            TM: SummaryMap<Data = T> + Send + Sync + 'static,
            E: Send + 'static,
            F: Fn(&D) -> R + Send + Sync + Clone,
            R: Future<Output = Result<T, E>> + Send
        {
            async move {
                match *schema {
                    Entity::Group { ref children, ref attributes } => {
                        let children = map_children(children, f).await?;
                        Ok(Entity::Group { children, attributes: attributes.clone() })
                    }
                    Entity::Record { ref children, ref attributes } => {
                        let children = map_children(children, f).await?;
                        Ok(Entity::Record { children, attributes: attributes.clone() })
                    }
                    Entity::Data { ref data, ref field, ref summaries } => {
                        let (data, summaries) = future::try_zip(
                            f(data),
                            map_summaries(summaries, f),
                        ).await?;
                        Ok(Entity::Data { field: field.clone(), data, summaries })
                    }
                    Entity::Union { ref data, ref variants, ref attributes } => {
                        let (data, variants) = future::try_zip(
                            f(data),
                            map_children(variants, f)
                        ).await?;
                        Ok(Entity::Union { data, variants, attributes: attributes.clone() })
                    }
                    Entity::FixedArray { elements, ref child, ref attributes } => {
                        let child = Box::new(Box::pin(map_entity(child, f)).await?);
                        Ok(Entity::FixedArray { elements, child, attributes: attributes.clone() })
                    }
                    Entity::Tuple { ref fields, ref child, ref attributes } => {
                        let child = Box::new(Box::pin(map_entity(child, f)).await?);
                        Ok(Entity::Tuple { fields: fields.clone(), child, attributes: attributes.clone() })
                    }
                    Entity::VariableArray { ref data, ref child, ref attributes } => {
                        let (data, child) = future::try_zip(
                            f(data),
                            Box::pin(map_entity(child, f))
                        ).await?;
                        Ok(Entity::VariableArray { data, child: Box::new(child), attributes: attributes.clone() })
                    }
                }
            }
        }

        map_entity(self, &f).await
    }

    pub fn schema(&self) -> EntitySchema {
        self.try_map_data(&mut |_| Ok::<Ignored, Infallible>(Ignored)).unwrap()
    }
}

impl<D: EntityData> Entity<D, StoredSummaryMap<D>> {
    pub fn each_data_mut(&mut self, mut f: &mut impl FnMut(&mut D)) {
        match self {
            Entity::Group { children, .. } | Entity::Record { children, ..} => {
                for child in children.values_mut() {
                    child.each_data_mut(f);
                }
            }
            Entity::Union { data, .. } => f(data),
            Entity::FixedArray { child, .. } | Entity::Tuple { child, .. } => {
                child.each_data_mut(f)
            }
            Entity::VariableArray { data, child, .. } => {
                f(data);
                child.each_data_mut(f);
            }
            Entity::Data { data, summaries, .. } => {
                f(data);
                for s in summaries.0.values_mut() {
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

    pub fn complex(field: impl Into<Field>) -> Self {
        let data = Self::field(field);
        Entity::Tuple {
            fields: IndexMap::from_iter([
                ("re".into(), Default::default()),
                ("im".into(), Default::default()),
            ]),
            child: Box::new(data),
            attributes: Default::default(),
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
    pub fn as_field(&self) -> Option<FieldRef<'_>> {
        match self {
            Entity::Data { data, field, summaries } => Some(FieldRef { data, field, summaries }),
            _ => None,
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
