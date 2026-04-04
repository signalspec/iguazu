
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ElementSize {
    U8,
    U16,
    U32,
    U64,
}

impl ElementSize {
    #[inline]
    pub fn bytes(&self) -> usize {
        match self {
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
    pub fn from_bits(bits: u8) -> Option<Self> {
        match bits {
            ..=8 => Some(ElementSize::U8),
            ..=16 => Some(ElementSize::U16),
            ..=32 => Some(ElementSize::U32),
            ..=64 => Some(ElementSize::U64),
            _ => None,
        }
    }

    #[inline]
    pub fn from_bytes(bits: u8) -> Option<Self> {
        match bits {
            ..=1 => Some(ElementSize::U8),
            ..=2 => Some(ElementSize::U16),
            ..=4 => Some(ElementSize::U32),
            ..=8 => Some(ElementSize::U64),
            _ => None,
        }
    }
}

pub trait Element: Send + Sync + bytemuck::Pod + bytemuck::NoUninit + bytemuck::Zeroable + bytemuck::AnyBitPattern + bytemuck::NoUninit {
    const ELEMENT_SIZE: ElementSize;
}

impl Element for u8 {
    const ELEMENT_SIZE: ElementSize = ElementSize::U8;
}
impl Element for u16 {
    const ELEMENT_SIZE: ElementSize = ElementSize::U16;
}
impl Element for u32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::U32;
}
impl Element for u64 {
    const ELEMENT_SIZE: ElementSize = ElementSize::U64;
}
impl Element for i8 {
    const ELEMENT_SIZE: ElementSize = ElementSize::U8;
}
impl Element for i16 {
    const ELEMENT_SIZE: ElementSize = ElementSize::U16;
}
impl Element for i32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::U32;
}
impl Element for i64 {
    const ELEMENT_SIZE: ElementSize = ElementSize::U64;
}
impl Element for f32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::U32;
}
impl Element for f64 {
    const ELEMENT_SIZE: ElementSize = ElementSize::U64;
}
