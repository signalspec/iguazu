use indexmap::IndexMap;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use super::{Entity, EntityKind};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
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

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

pub trait Attribute: Clone + PartialEq + Serialize + DeserializeOwned {
    const NAME: &'static str;

    fn default<S>(_entity: &Entity<S>) -> Option<Self> {
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

    /// A row showing changes in the value, displayed as text
    Trace,

    /// Bits are shown as logic traces
    Logic,

    /// Each value is displayed as a discrete event
    Events,
}

impl Attribute for TimelineRow {
    const NAME: &'static str = "display:timeline_row";
    
    fn default<S>(entity: &Entity<S>) -> Option<Self> {
        match entity.kind {
            EntityKind::Record { .. } if entity.attribute::<Time>().is_some() => Some(TimelineRow::Events),
            EntityKind::Group { .. }
            | EntityKind::Record { .. } => Some(TimelineRow::Group),
            EntityKind::Logic { .. } => Some(TimelineRow::Logic),
            EntityKind::Signed { .. }
            | EntityKind::Unsigned { .. }
            | EntityKind::Float { .. } => Some(TimelineRow::YAxis),
            EntityKind::Bits { .. }
            | EntityKind::Enum { .. } => Some(TimelineRow::Trace),
            _ => None,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "view", rename_all = "snake_case")]
pub enum DefaultView {
    Timeline,
    Table,
}

impl Attribute for DefaultView {
    const NAME: &'static str = "display:default";
    
    fn default<S>(entity: &Entity<S>) -> Option<Self> {
        if entity.attribute::<SampleRate>().is_some() || entity.attribute::<Time>().is_some() {
            Some(DefaultView::Timeline)
        } else if matches!(entity.kind, EntityKind::Record {..}) {
            Some(DefaultView::Table)
        } else {
            None
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SampleRate(pub f64);

impl Attribute for SampleRate {
    const NAME: &'static str = "sample_rate";
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Time(pub String);

impl Attribute for Time {
    const NAME: &'static str = "time";
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Text(pub String);

impl Attribute for Text {
    const NAME: &'static str = "text";
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct NumberRange {
    pub min: f64,
    pub max: f64,
}

impl Attribute for NumberRange {
    const NAME: &'static str = "number:range";
}

