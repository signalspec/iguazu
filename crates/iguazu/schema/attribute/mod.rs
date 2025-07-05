use ecow::EcoString;
use indexmap::IndexMap;
use serde::{Serialize, Deserialize};
use strum::{EnumString, IntoStaticStr};

use super::{Entity, EntityKind, Field};

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct AttributeMap {
    #[serde(flatten)]
    pub attributes: IndexMap<EcoString, AttributeValue>,
}

impl AttributeMap {
    pub fn get<'a, A: TryFrom<&'a AttributeValue>>(&'a self, attr: &str) -> Option<A> {
        self.attributes.get(attr)
        .and_then(|v| A::try_from(v).ok())
    }
    
    pub fn insert(&mut self, attr: &str, val: impl Into<AttributeValue>) {
        self.attributes.insert(attr.into(), val.into());
    }

    pub fn items(&self) -> impl Iterator<Item = (&str, &AttributeValue)> {
        self.attributes.iter().map(|(k, v)| (k.as_ref(), v))
    }

    pub fn len(&self) -> usize {
        self.attributes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.attributes.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    String(EcoString),
    Float(f64),
    Bool(bool),
    Object(AttributeMap),
    Array(Vec<AttributeValue>),
}

impl From<&str> for AttributeValue {
    fn from(value: &str) -> Self {
        AttributeValue::String(EcoString::from(value))
    }
}

impl From<EcoString> for AttributeValue {
    fn from(value: EcoString) -> Self {
        AttributeValue::String(value)
    }
}

impl TryFrom<&AttributeValue> for EcoString {
    type Error = ();

    fn try_from(value: &AttributeValue) -> Result<Self, Self::Error> {
        match value {
            AttributeValue::String(s) => Ok(s.clone()),
            _ => Err(()),
        }
    }
}

impl From<f64> for AttributeValue {
    fn from(value: f64) -> Self {
        AttributeValue::Float(value)
    }
}

impl TryFrom<&AttributeValue> for f64 {
    type Error = ();

    fn try_from(value: &AttributeValue) -> Result<Self, Self::Error> {
        match value {
            AttributeValue::Float(f) => Ok(*f),
            _ => Err(()),
        }
    }
}

impl TryFrom<&AttributeValue> for u64 {
    type Error = ();

    fn try_from(value: &AttributeValue) -> Result<Self, Self::Error> {
        match value {
            AttributeValue::Float(f) if *f >= 0.0 && *f <= u64::MAX as f64 && f.fract() == 0.0  => {
                Ok(*f as u64)
            }
            _ => Err(()),
        }
    }
}

impl From<bool> for AttributeValue {
    fn from(value: bool) -> Self {
        AttributeValue::Bool(value)
    }
}

impl TryFrom<&AttributeValue> for bool {
    type Error = ();

    fn try_from(value: &AttributeValue) -> Result<Self, Self::Error> {
        match value {
            AttributeValue::Bool(b) => Ok(*b),
            _ => Err(()),
        }
    }
}

impl From<AttributeMap> for AttributeValue {
    fn from(value: AttributeMap) -> Self {
        AttributeValue::Object(value)
    }
}

impl TryFrom<&AttributeValue> for AttributeMap {
    type Error = ();

    fn try_from(value: &AttributeValue) -> Result<Self, Self::Error> {
        match value {
            AttributeValue::Object(map) => Ok(map.clone()),
            _ => Err(()),
        }
    }
}

impl<'de> Deserialize<'de> for AttributeValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        struct AttributeValueVisitor;

        impl<'de> serde::de::Visitor<'de> for AttributeValueVisitor {
            type Value = AttributeValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string, number, bool, or object")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(AttributeValue::String(EcoString::from(value)))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(AttributeValue::Float(value))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(AttributeValue::Float(value as f64))
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(AttributeValue::Bool(value))
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let mut attributes = IndexMap::new();
                while let Some((key, value)) = map.next_entry::<String, AttributeValue>()? {
                    attributes.insert(EcoString::from(key), value);
                }
                Ok(AttributeValue::Object(AttributeMap { attributes }))
            }

            fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
            where
                S: serde::de::SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<AttributeValue>()? {
                    values.push(value);
                }
                Ok(AttributeValue::Array(values))
            }
        }

        deserializer.deserialize_any(AttributeValueVisitor)
    }
}

impl Serialize for AttributeValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        match self {
            AttributeValue::String(s) => serializer.serialize_str(s),
            AttributeValue::Float(f) => serializer.serialize_f64(*f),
            AttributeValue::Bool(b) => serializer.serialize_bool(*b),
            AttributeValue::Object(map) => {
                use serde::ser::SerializeMap;
                let mut ser_map = serializer.serialize_map(Some(map.len()))?;
                for (k, v) in &map.attributes {
                    ser_map.serialize_entry(&**k, v)?;
                }
                ser_map.end()
            }
            AttributeValue::Array(arr) => {
                use serde::ser::SerializeSeq;
                let mut ser_seq = serializer.serialize_seq(Some(arr.len()))?;
                for v in arr {
                    ser_seq.serialize_element(v)?;
                }
                ser_seq.end()
            }
        }
    }
}

/// Core attributes
impl<D> Entity<D> {
    pub fn sample_rate(&self) -> Option<f64> {
        self.attribute("sample_rate")
    }

    pub fn time(&self) -> Option<EcoString> {
        self.attribute("time")
    }

    pub fn text(&self) -> Option<EcoString> {
        self.attribute("text")
    }

    pub fn number_range(&self) -> Option<NumberRange> {
        let o: AttributeMap = self.attribute("number:range")?;
        Some(NumberRange {
            min: o.get("min")?,
            max: o.get("max")?,
        })
    }

    pub fn number_scale(&self) -> f64 {
        self.attribute("number:scale").unwrap_or(1.0)
    }

    pub fn number_offset(&self) -> f64 {
        self.attribute("number:offset").unwrap_or(0.0)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct NumberRange {
    pub min: f64,
    pub max: f64,
}

impl<D> Entity<D> {
    pub fn display_default(&self) -> Option<DefaultView> {
        let o: Option<AttributeMap> = self.attribute("display:default");
        o.and_then(|o| o.get("view")).or(
            if self.time().is_some() || self.sample_rate().is_some() {
                Some(DefaultView::Timeline)
            } else if matches!(self.kind, EntityKind::Record {..}) {
                Some(DefaultView::Table)
            } else {
                None
            }
        )
    }

    pub fn accent_color(&self) -> Option<AccentColor> {
        self.attribute("display:accent_color")
    }
}

impl Field {
    pub fn accent_color(&self) -> Option<AccentColor> {
        self.attribute("display:accent_color")
    }
}

/// Timeline attributes
impl<D> Entity<D> {
    pub fn timeline_row(&self) -> Option<TimelineRow> {
        self.attribute("display:timeline_row").or(
            match self.kind {
                EntityKind::Record { .. } if self.time().is_some() => Some(TimelineRow::Events),
                EntityKind::Group { .. }
                | EntityKind::Record { .. } => Some(TimelineRow::Group),
                EntityKind::Logic { .. } => Some(TimelineRow::Logic),
                EntityKind::Number { .. } => Some(TimelineRow::YAxis),
                EntityKind::Bits { .. }
                | EntityKind::Enum { .. } => Some(TimelineRow::Trace),
                _ => None,
            }
        )
    }
}

macro_rules! string_attribute {
    ($name:ty) => {
        impl TryFrom<&AttributeValue> for $name {
            type Error = ();

            fn try_from(value: &AttributeValue) -> Result<Self, Self::Error> {
                match value {
                    AttributeValue::String(s) => s.parse().map_err(|_| ()),
                    _ => Err(()),
                }
            }
        }

        impl From<$name> for AttributeValue {
            fn from(value: $name) -> AttributeValue {
                AttributeValue::String(<&str>::from(value).into())
            }
        }
    };
}

#[derive(Copy, Clone, PartialEq, Debug, IntoStaticStr, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum AccentColor {
    Red,
    Orange,
    Brown,
    Yellow,
    Green,
    Blue,
    Purple,
}

string_attribute!(AccentColor);

#[derive(Copy, Clone, PartialEq, Debug, IntoStaticStr, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum TimelineRow {
    /// Children are displayed as separate timeline rows.
    Group,

    /// Analog Y axis. If applied to an entity with children,
    /// the children are plotted on the same Y axis.
    YAxis,

    /// A row showing changes in the value, displayed as text
    Trace,

    /// Bits are shown as logic traces
    Logic,

    /// Each value is displayed as a discrete event
    Events,
}

string_attribute!(TimelineRow);

#[derive(Copy, Clone, PartialEq, Debug, IntoStaticStr, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum DefaultView {
    Timeline,
    Table,
}

string_attribute!(DefaultView);

