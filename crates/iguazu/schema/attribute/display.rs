use ecow::EcoString;
use strum::{EnumString, IntoStaticStr};

use super::{Attribute, string_attribute};
use crate::schema::{AttributeMap, Entity, Field, FieldKind, attribute::{AttributeValue, core::{ROLE, Role}}};

pub const LAYOUT: Attribute<Layout> = Attribute::named("display:layout");
pub const COLOR: Attribute<AccentColor> = Attribute::named("display:color");
pub const TIMELINE_ROW: Attribute<TimelineRow> = Attribute::named("display:timeline:row");

#[derive(Copy, Clone, PartialEq, Debug, IntoStaticStr, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum Layout {
    Timeline,
    Table,
}

impl<'a> TryFrom<&'a AttributeValue> for Layout {
    type Error = ();

    fn try_from(value: &'a AttributeValue) -> Result<Self, Self::Error> {
        let m: AttributeMap = value.try_into().map_err(|_| ())?;
        match m.get("view").ok_or(())? {
            "timeline" => Ok(Layout::Timeline),
            "table" => Ok(Layout::Table),
            _ => Err(()),
        }
    }
}

impl From<Layout> for AttributeValue {
    fn from(value: Layout) -> Self {
        let mut m = AttributeMap::from_iter([]);
        let view_str = match value {
            Layout::Timeline => "timeline",
            Layout::Table => "table",
        };
        m.insert("view", EcoString::from(view_str));
        AttributeValue::from(m)
    }
}


#[derive(Copy, Clone, PartialEq, Debug, IntoStaticStr, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum AccentColor {
    /// White / Black, depending on the theme.
    Neutral,
    Brown,
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
}

string_attribute!(AccentColor);

impl AccentColor {
    pub fn from_bit_position(pos: u8) -> Self {
        match pos % 8 {
            0 => AccentColor::Neutral,
            1 => AccentColor::Brown,
            2 => AccentColor::Red,
            3 => AccentColor::Orange,
            4 => AccentColor::Yellow,
            5 => AccentColor::Green,
            6 => AccentColor::Blue,
            7 => AccentColor::Purple,
            _ => unreachable!(),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug, IntoStaticStr, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum TimelineRow {
    Hidden,

    /// Children are displayed as separate timeline rows.
    Stack,

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

impl<D, S> Entity<D, S> {
    pub fn display_default(&self) -> Option<Layout> {
        self.attribute(LAYOUT)
            .or(if self.time_rate().is_some() || self.time_point().is_some() || self.time_span().is_some() || self.role() == Some(Role::Capture) {
                Some(Layout::Timeline)
            } else if matches!(self, Entity::Group { .. }) && self.role() == Some(Role::Record) {
                Some(Layout::Table)
            } else {
                None
            })
    }

    pub fn accent_color(&self) -> Option<AccentColor> {
        self.attribute(COLOR)
    }

    pub fn timeline_row(&self) -> TimelineRow {
        self.attribute(TIMELINE_ROW).unwrap_or_else(|| match self {
            Entity::Group { .. } if self.role() == Some(Role::Record) && self.time_span().is_some() => TimelineRow::Events,
            Entity::Group { .. } => TimelineRow::Stack,
            Entity::Data { field, .. } => field.timeline_row(),
            _ => TimelineRow::Hidden,
        })
    }

    pub fn role(&self) -> Option<Role> {
        self.attribute(ROLE)
    }
}

impl Field {
    pub fn accent_color(&self) -> Option<AccentColor> {
        self.attribute(COLOR)
    }

    pub fn timeline_row(&self) -> TimelineRow {
        self.attribute(TIMELINE_ROW)
            .unwrap_or_else(|| match &self.kind {
                FieldKind::Null => TimelineRow::Hidden,
                FieldKind::BitStruct { .. } => TimelineRow::Stack,
                FieldKind::Bits { bits: 1, .. } => TimelineRow::Logic,
                FieldKind::Bits { .. }
                | FieldKind::Character { .. }
                | FieldKind::Enum { .. } => TimelineRow::Trace,
                FieldKind::Timestamp => TimelineRow::Hidden,
                FieldKind::Int { .. }
                | FieldKind::Signed { .. }
                | FieldKind::Float32 { .. }
                | FieldKind::Float64 => TimelineRow::YAxis,
            })
    }
}
