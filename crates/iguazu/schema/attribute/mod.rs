use std::{fmt::{Debug, Display}, marker::PhantomData, str::FromStr};

use ecow::{eco_format, EcoString};
use indexmap::IndexMap;
use jiff::Timestamp;
use serde::{Serialize, Deserialize};

pub mod core;
pub mod display;

#[derive(Clone, Copy)]
pub struct Attribute<T> {
    name: &'static str,
    value_type: PhantomData<T>,
}

impl<T> Attribute<T> {
    pub const fn named(name: &'static str) -> Self {
        Attribute {
            name,
            value_type: PhantomData,
        }
    }
}

impl<T> Debug for Attribute<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Attribute<{}>(\"{}\")", std::any::type_name::<T>(), self.name)
    }
}

impl<T> Display for Attribute<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl<T> From<Attribute<T>> for &'static str {
    fn from(attr: Attribute<T>) -> Self {
        attr.name
    }
}

impl<T> From<Attribute<T>> for EcoString {
    fn from(attr: Attribute<T>) -> Self {
        attr.name.into()
    }
}

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct AttributeMap {
    #[serde(flatten)]
    pub attributes: IndexMap<EcoString, AttributeValue>,
}

impl<'de> Deserialize<'de> for AttributeMap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = AttributeMap;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a map of attributes")
                }

                fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
                {
                    let mut attributes = IndexMap::new();
                    while let Some((key, value)) = map.next_entry::<EcoString, AttributeValue>()? {
                        if matches!(key.as_ref(),
                            | "type"
                            | "bits"
                            | "values"
                            | "tag_bits"
                            | "children"
                            | "variants"
                            | "elements"
                            | "child"
                            | "data"
                            | "summaries")
                        {
                            // Ignore keys from `Field` or `Entity` to avoid duplicate keys
                            // https://github.com/serde-rs/serde/issues/2200
                            continue;
                        }

                        attributes.insert(key, value);
                    }
                    Ok(AttributeMap { attributes })
                }
            }

            deserializer.deserialize_map(Visitor)
        }
}

impl AttributeMap {
    pub fn get<'a, A: TryFrom<&'a AttributeValue>>(&'a self, attr: &str) -> Option<A> {
        self.attributes.get(attr).and_then(|v| A::try_from(v).ok())
    }

    pub fn insert(&mut self, attr: &str, val: impl Into<AttributeValue>) {
        self.attributes.insert(attr.into(), val.into());
    }

    pub fn remove(&mut self, attr: &str) {
        self.attributes.shift_remove(attr);
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

impl FromIterator<(EcoString, AttributeValue)> for AttributeMap {
    fn from_iter<T: IntoIterator<Item = (EcoString, AttributeValue)>>(iter: T) -> Self {
        let attributes = IndexMap::from_iter(iter);
        AttributeMap { attributes }
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

impl<'a> TryFrom<&'a AttributeValue> for &'a str {
    type Error = ();

    fn try_from(value: &'a AttributeValue) -> Result<Self, Self::Error> {
        match value {
            AttributeValue::String(s) => Ok(s.as_ref()),
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

impl From<Timestamp> for AttributeValue {
    fn from(value: Timestamp) -> Self {
        AttributeValue::String(eco_format!("{}", value))
    }
}

impl TryFrom<&AttributeValue> for Timestamp {
    type Error = ();

    fn try_from(value: &AttributeValue) -> Result<Self, Self::Error> {
        match value {
            AttributeValue::String(s) => {
                Timestamp::from_str(s).map_err(|_| ())
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

macro_rules! string_attribute {
    ($name:ty) => {
        impl TryFrom<&$crate::schema::attribute::AttributeValue> for $name {
            type Error = ();

            fn try_from(value: &$crate::schema::attribute::AttributeValue) -> Result<Self, Self::Error> {
                match value {
                    $crate::schema::attribute::AttributeValue::String(s) => s.parse().map_err(|_| ()),
                    _ => Err(()),
                }
            }
        }

        impl From<$name> for $crate::schema::attribute::AttributeValue {
            fn from(value: $name) -> $crate::schema::attribute::AttributeValue {
                $crate::schema::attribute::AttributeValue::String(<&str>::from(value).into())
            }
        }
    };
}

pub(crate) use string_attribute;
