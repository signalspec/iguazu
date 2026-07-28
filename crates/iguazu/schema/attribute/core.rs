use ecow::EcoString;
use jiff::Zoned;
use strum::{EnumString, IntoStaticStr};

use crate::{schema::{Entity, Field}, time::Time};
use super::{ Attribute, string_attribute };

pub const ROLE: Attribute<Role> = Attribute::named("core:role");
pub const TEXT: Attribute<EcoString> = Attribute::named("core:text");

pub const TIME_FIELD: Attribute<EcoString> = Attribute::named("time:field");
pub const TIME_RATE: Attribute<f64> = Attribute::named("time:rate");
pub const TIME_EPOCH: Attribute<Zoned> = Attribute::named("time:epoch");
pub const TIME_DISPLAY: Attribute<TimeDisplay> = Attribute::named("time:display");

pub const NUMBER_MIN: Attribute<f64> = Attribute::named("number:min");
pub const NUMBER_MAX: Attribute<f64> = Attribute::named("number:max");
pub const NUMBER_SCALE: Attribute<f64> = Attribute::named("number:scale");
pub const NUMBER_OFFSET: Attribute<f64> = Attribute::named("number:offset");

#[derive(Clone, Copy, Debug, IntoStaticStr, EnumString, PartialEq, Eq)]
#[strum(serialize_all = "snake_case")]
pub enum Role {
    /// Group where children represent time-aligned columns
    Record,

    /// Group where children represent independent series captured
    /// simultaneously, but not necessarily sampled at the same rate
    Capture,
}

string_attribute!(Role);

#[derive(Clone, Copy, Debug, IntoStaticStr, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum TimeDisplay {
    /// ISO 8601 / RFC 3339 absolute timestamp
    Iso,

    /// Relative to epoch in HH:MM:SS.sss format
    Relative,

    /// Raw integer ticks
    Raw,
}

string_attribute!(TimeDisplay);

impl<D, S> Entity<D, S> {
    pub fn time_rate(&self) -> Option<f64> {
        self.attribute(TIME_RATE)
    }

    pub fn time_rate_as_period(&self) -> Option<Time> {
        self.time_rate().map(Time::period_float)
    }

    pub fn time_field(&self) -> Option<EcoString> {
        self.attribute(TIME_FIELD)
    }

    pub fn text(&self) -> Option<EcoString> {
        self.attribute(TEXT)
    }
}

impl Field {
    pub fn time_rate(&self) -> Option<f64> {
        self.attribute(TIME_RATE)
    }

    pub fn time_rate_as_period(&self) -> Option<Time> {
        self.time_rate().map(Time::period_float)
    }

    pub fn time_epoch(&self) -> Option<Zoned> {
        self.attribute(TIME_EPOCH)
    }

    pub fn time_display(&self) -> TimeDisplay {
        self.attribute(TIME_DISPLAY).unwrap_or({
            if self.time_rate().is_none() {
                TimeDisplay::Raw
            } else if self.time_epoch().is_none() {
                TimeDisplay::Relative
            } else {
                TimeDisplay::Iso
            }
        })
    }

    pub fn number_scale(&self) -> f64 {
        self.attribute(NUMBER_SCALE).unwrap_or(1.0)
    }

    pub fn number_offset(&self) -> f64 {
        self.attribute(NUMBER_OFFSET).unwrap_or(0.0)
    }

    pub fn number_min(&self) -> Option<f64> {
        self.attribute(NUMBER_MIN)
    }

    pub fn number_max(&self) -> Option<f64> {
        self.attribute(NUMBER_MAX)
    }

    pub fn number_range(&self) -> Option<(f64, f64)> {
        Some((self.attribute(NUMBER_MIN)?, self.attribute(NUMBER_MAX)?))
    }

    pub fn text(&self) -> Option<EcoString> {
        self.attribute(TEXT)
    }
}
