use serde::{Deserialize, Serialize};
use std::{fmt::Debug, sync::Arc};
use crate::Idx;

pub trait Stream: Send + Sync + Debug {
    fn desc(&self) -> StreamDesc;

    fn state(&self) -> StreamState;

    fn access(self: Arc<Self>) -> Box<dyn StreamAccess>;
}

pub type ArcStream = Arc<dyn Stream>;

pub trait StreamAccess: Send  {
    fn get_block(&self, block: u64) -> &[u8];

    fn state(&self) -> StreamState;

    fn reset(&mut self);
}

pub struct StreamDesc {
    pub element_size: ElementSize,
    pub block_size: usize,
}

pub struct StreamState {
    pub end: Idx,
    pub streaming: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum ElementSize {
    Null,
    U8,
    U16,
    U32,
    U64,
}

impl<'de> Deserialize<'de> for ElementSize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bits = u8::deserialize(deserializer)?;
        ElementSize::from_exact_bits(bits).ok_or(serde::de::Error::custom(format!("expected valid element size")))
    }
}

impl Serialize for ElementSize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(self.bits() as u8)
    }
}

impl ElementSize {
    #[inline]
    pub fn bytes(&self) -> usize {
        match self {
            ElementSize::Null => 0,
            ElementSize::U8 => 1,
            ElementSize::U16 => 2,
            ElementSize::U32 => 4,
            ElementSize::U64 => 8,
        }
    }

    #[inline]
    pub fn bits(&self) -> usize {
        self.bytes() * 8
    }

    #[inline]
    pub fn from_exact_bits(bits: u8) -> Option<Self> {
        match bits {
            0 => Some(ElementSize::Null),
            8 => Some(ElementSize::U8),
            16 => Some(ElementSize::U16),
            32 => Some(ElementSize::U32),
            64 => Some(ElementSize::U64),
            _ => None,
        }
    }

    #[inline]
    pub fn from_bits(bits: u8) -> Option<Self> {
        match bits {
            0 => Some(ElementSize::Null),
            ..=8 => Some(ElementSize::U8),
            ..=16 => Some(ElementSize::U16),
            ..=32 => Some(ElementSize::U32),
            ..=64 => Some(ElementSize::U64),
            _ => None,
        }
    }
}
