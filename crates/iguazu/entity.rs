use crate::{IdxStream, AnyStream};

use indexmap::IndexMap;
use num_rational::{Rational64, Ratio};

pub type SampleRate = Ratio<u64>;

#[derive(Copy, Clone, PartialEq)]
pub enum NamedColor {
    White,
    Black,
    Red,
    Orange,
    Brown,
    Yellow,
    Green,
    Blue,
    Purple,
}

pub enum Entity {
    Group(IndexMap<String, Entity>),
    Record(Record),
    Timestamp(Timestamp),
    Bits(Bits),
    Scalar(Scalar),
    Complex(Complex),
    Enum(Enum),
    Packet(Packet),
}


pub struct Record {
    pub fields: IndexMap<String, Entity>,
    pub color: Option<NamedColor>,
}

pub struct Timestamp {
    pub data: IdxStream,
    pub base_clock: SampleRate,
    pub color: Option<NamedColor>,
}

pub struct Bits {
    pub data: AnyStream,
    pub width: u8,
    pub color: Option<NamedColor>,
}

pub struct Scalar {
    pub data: AnyStream,
    pub encoding: NumericEncoding,
    pub min: Rational64,
    pub max: Rational64,
    pub sample_rate: Option<SampleRate>,
    pub unit: Option<String>,
    pub color: Option<NamedColor>,
}

pub struct Complex {
    pub data: AnyStream,
    pub encoding: NumericEncoding,
    pub max_magnitude: Rational64,
    pub sample_rate: Option<SampleRate>,
    pub center_frequency: Option<Rational64>,
    pub color: Option<NamedColor>,
}

pub enum NumericEncoding {
    Float32,
    Float64,
    Unsigned {
        offset: u64,
        scale: Rational64,
    },
    Signed {
        scale: Rational64,
    }
}

pub struct Enum {
    pub data: AnyStream,
    pub sample_rate: Option<SampleRate>,
    pub variants: IndexMap<String, EnumVariant>,
}

#[derive(Clone)]
pub struct EnumVariant {
    pub color: Option<NamedColor>,
}

/// Stream of indexes delimiting the end of packets of a child record
pub struct Packet {
    pub data: IdxStream,
    pub sample_rate: Option<SampleRate>,
    pub inner: Box<Entity>,
}

