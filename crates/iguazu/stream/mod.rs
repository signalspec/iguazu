use serde::{Deserialize, Serialize};
use std::{fmt::Debug, sync::Arc, task::Waker};
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

    fn begin(&mut self, waker: &Waker);

    fn end(&mut self);
}

#[derive(Clone)]
pub struct StreamDesc {
    pub element_type: ElementType,
    pub block_size: usize,
}

pub struct StreamState {
    pub end: Idx,
    pub streaming: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ElementType {
    U8,
    U16,
    U32,
    U64,
}

impl ElementType {
    #[inline]
    pub fn bytes(&self) -> usize {
        match self {
            ElementType::U8 => 1,
            ElementType::U16 => 2,
            ElementType::U32 => 4,
            ElementType::U64 => 8,
        }
    }

    #[inline]
    pub fn bits(&self) -> usize {
        self.bytes() * 8
    }

    #[inline]
    pub fn from_bits(bits: u8) -> Option<Self> {
        match bits {
            ..=8 => Some(ElementType::U8),
            ..=16 => Some(ElementType::U16),
            ..=32 => Some(ElementType::U32),
            ..=64 => Some(ElementType::U64),
            _ => None,
        }
    }
}

pub trait Element: bytemuck::Pod + bytemuck::NoUninit + bytemuck::Zeroable + bytemuck::AnyBitPattern {
    const ELEMENT_TYPE: ElementType;
}

impl Element for u8 {
    const ELEMENT_TYPE: ElementType = ElementType::U8;
}
impl Element for u16 {
    const ELEMENT_TYPE: ElementType = ElementType::U16;
}
impl Element for u32 {
    const ELEMENT_TYPE: ElementType = ElementType::U32;
}
impl Element for u64 {
    const ELEMENT_TYPE: ElementType = ElementType::U64;
}
impl Element for i8 {
    const ELEMENT_TYPE: ElementType = ElementType::U8;
}
impl Element for i16 {
    const ELEMENT_TYPE: ElementType = ElementType::U16;
}
impl Element for i32 {
    const ELEMENT_TYPE: ElementType = ElementType::U32;
}
impl Element for i64 {
    const ELEMENT_TYPE: ElementType = ElementType::U64;
}
impl Element for f32 {
    const ELEMENT_TYPE: ElementType = ElementType::U32;
}
impl Element for f64 {
    const ELEMENT_TYPE: ElementType = ElementType::U64;
}