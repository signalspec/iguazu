use ecow::EcoString;
use jiff::Timestamp;

use crate::{schema::{attribute::AttributeValue, Entity, Field}, time::Time};
use super::{ Attribute, AttributeMap };

pub const SAMPLE_RATE: Attribute<f64> = Attribute::named("sample_rate");
pub const TIME: Attribute<EcoString> = Attribute::named("time");
pub const TIME_RATE: Attribute<f64> = Attribute::named("time:rate");
pub const TIME_EPOCH: Attribute<Timestamp> = Attribute::named("time:epoch");
pub const TEXT: Attribute<EcoString> = Attribute::named("text");

pub const NUMBER_RANGE: Attribute<NumberRange> = Attribute::named("number:range");
pub const NUMBER_SCALE: Attribute<f64> = Attribute::named("number:scale");
pub const NUMBER_OFFSET: Attribute<f64> = Attribute::named("number:offset");

#[derive(Clone, Copy, Debug)]
pub struct NumberRange {
    pub min: f64,
    pub max: f64,
}

impl TryFrom<&AttributeValue> for NumberRange {
    type Error = ();

    fn try_from(value: &AttributeValue) -> Result<Self, Self::Error> {
        let o: AttributeMap = value.try_into().map_err(|_| ())?;
        Ok(NumberRange {
            min: o.get("min").ok_or(())?,
            max: o.get("max").ok_or(())?,
        })
    }
}

impl From<NumberRange> for AttributeValue {
    fn from(range: NumberRange) -> AttributeValue {
        AttributeMap::from_iter([
            ("min".into(), range.min.into()),
            ("max".into(), range.max.into()),
        ]).into()
    }
}

impl<D> Entity<D> {
    pub fn sample_rate(&self) -> Option<f64> {
        self.attribute(SAMPLE_RATE)
    }

    pub fn sample_rate_as_period(&self) -> Option<Time> {
        self.sample_rate().map(|sr| Time::period_float(sr))
    }

    pub fn time(&self) -> Option<EcoString> {
        self.attribute(TIME)
    }

    pub fn text(&self) -> Option<EcoString> {
        self.attribute(TEXT)
    }
}

impl Field {
    pub fn number_range(&self) -> Option<NumberRange> {
        self.attribute(NUMBER_RANGE)
    }

    pub fn time_rate(&self) -> Option<f64> {
        self.attribute(TIME_RATE)
    }

    pub fn time_rate_as_period(&self) -> Option<Time> {
        self.time_rate().map(|tr| Time::period_float(tr))
    }

    pub fn time_epoch(&self) -> Option<Timestamp> {
        self.attribute(TIME_EPOCH)
    }

    pub fn number_scale(&self) -> f64 {
        self.attribute(NUMBER_SCALE).unwrap_or(1.0)
    }

    pub fn number_offset(&self) -> f64 {
        self.attribute(NUMBER_OFFSET).unwrap_or(0.0)
    }

    pub fn text(&self) -> Option<EcoString> {
        self.attribute(TEXT)
    }
}
