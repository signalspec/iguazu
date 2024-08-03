use indexmap::IndexMap;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use super::{EntityKind, Field, NestedField};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Attributes {
    #[serde(flatten)]
    values: IndexMap<String, Value>,
}

impl Attributes {
    pub fn get<A: Attribute>(&self) -> Option<Result<A, serde_json::Error>> {
        self.values.get(A::NAME).map(A::deserialize)
    }

    pub fn set<A: Attribute>(&mut self, val: &A) {
        self.values.insert(A::NAME.to_owned(), serde_json::value::to_value(val).unwrap());
    }
}

pub trait Attribute: Clone + PartialEq + Serialize + DeserializeOwned {
    const NAME: &'static str;

    fn entity_default<S>(_entity: &EntityKind<S>) -> Option<Self> {
        None
    }

    fn field_default(_field: &NestedField) -> Option<Self> {
        None
    }
}

#[derive(Copy, Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccentColor {
    Red,
    Orange,
    Brown,
    Yellow,
    Green,
    Blue,
    Purple,
}

impl Attribute for AccentColor {
    const NAME: &'static str = "display:accent_color";
}

#[derive(Copy, Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineRow {
    /// Children are displayed as separate timeline rows.
    Group,

    /// Analog Y axis. If applied to an entity with children,
    /// the children are plotted on the same Y axis.
    YAxis,

    /// A row showing changes in the value
    Trace,

    /// Each value is displayed as a discrete event
    Events,
}

impl Attribute for TimelineRow {
    const NAME: &'static str = "display:timeline_row";
    
    fn entity_default<S>(_entity: &EntityKind<S>) -> Option<Self> {
        None
    }
    
    fn field_default(field: &NestedField) -> Option<Self> {
        match field.kind {
            Field::Struct { .. } => Some(TimelineRow::Group),
            _ => Some(TimelineRow::Trace)
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SampleRate(pub f64);

impl Attribute for SampleRate {
    const NAME: &'static str = "sample_rate";
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogicLevel {
    Low,
    High,
}

impl Attribute for LogicLevel {
    const NAME: &'static str = "logic_level";
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Text(pub String);

impl Attribute for Text {
    const NAME: &'static str = "text";
}
