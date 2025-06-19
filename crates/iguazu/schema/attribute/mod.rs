use std::sync::Arc;

use indexmap::IndexMap;
use serde::{Serialize, Deserialize};
use serde_json::Value;

use super::{Entity, EntityKind, Field};

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct AttributeMap {
    #[serde(flatten)]
    pub attributes: IndexMap<Arc<str>, Value>,
}

impl AttributeMap {
    pub fn get<'a, A: Deserialize<'a>>(&'a self, attr: &str) -> Option<A> {
        self.attributes.get(attr)
        .and_then(|v| A::deserialize(v).ok())
    }
    
    pub fn insert(&mut self, attr: &str, val: impl Serialize) {
        self.attributes.insert(attr.into(), serde_json::to_value(val).unwrap());
    }

    pub fn items(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.attributes.iter().map(|(k, v)| (k.as_ref(), v))
    }

    pub fn len(&self) -> usize {
        self.attributes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.attributes.is_empty()
    }
}

/// Core attributes
impl<D> Entity<D> {
    pub fn sample_rate(&self) -> Option<f64> {
        self.attribute("sample_rate")
    }

    pub fn time(&self) -> Option<Arc<str>> {
        self.attribute("time")
    }

    pub fn text(&self) -> Option<Arc<str>> {
        self.attribute("text")
    }

    pub fn number_range(&self) -> Option<NumberRange> {
        self.attribute("number:range")
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NumberRange {
    pub min: f64,
    pub max: f64,
}

impl<D> Entity<D> {
    pub fn display_default(&self) -> Option<DefaultView> {
        self.attribute("display:default").or(
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
                EntityKind::Signed { .. }
                | EntityKind::Unsigned { .. }
                | EntityKind::Float { .. } => Some(TimelineRow::YAxis),
                EntityKind::Bits { .. }
                | EntityKind::Enum { .. } => Some(TimelineRow::Trace),
                _ => None,
            }
        )
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

#[derive(Copy, Clone, PartialEq, Debug, Deserialize, Serialize)]
#[serde(tag = "view", rename_all = "snake_case")]
pub enum DefaultView {
    Timeline,
    Table,
}
