use crate::{Idx, IdxRange, AnyStream};
use super::Cache;

pub enum IntView {
    I8(Cache<u8>),
    I16(Cache<u16>),
    I32(Cache<u32>),
    I64(Cache<u64>),
}

impl IntView {
    pub fn new(stream: AnyStream) -> IntView {
        match stream {
            AnyStream::I8(s) => IntView::I8(Cache::new(s)),
            AnyStream::I16(s) => IntView::I16(Cache::new(s)),
            AnyStream::I32(s) => IntView::I32(Cache::new(s)),
            AnyStream::I64(s) => IntView::I64(Cache::new(s)),
        }
    }

    pub fn set_range(&mut self, range: IdxRange) {
        match self {
            IntView::I8(v) => v.set_range(range),
            IntView::I16(v) => v.set_range(range),
            IntView::I32(v) => v.set_range(range),
            IntView::I64(v) => v.set_range(range),
        }
    }

    pub fn range(&self) -> IdxRange {
        match self {
            IntView::I8(v) => v.range(),
            IntView::I16(v) => v.range(),
            IntView::I32(v) => v.range(),
            IntView::I64(v) => v.range(),
        }
    }

    pub fn get(&self, idx: Idx) -> Option<u64> {
        match self {
            IntView::I8(v) => v.get(idx).map(|v| v as u64),
            IntView::I16(v) => v.get(idx).map(|v| v as u64),
            IntView::I32(v) => v.get(idx).map(|v| v as u64),
            IntView::I64(v) => v.get(idx),
        }
    }

    pub fn for_each_elem(&self, mut f: impl FnMut(Idx, Option<u64>)) {
        match self {
            IntView::I8(c) => c.for_each_elem(|i, v| f(i, v.map(|v| v as u64))),
            IntView::I16(c) => c.for_each_elem(|i, v| f(i, v.map(|v| v as u64))),
            IntView::I32(c) => c.for_each_elem(|i, v| f(i, v.map(|v| v as u64))),
            IntView::I64(c) => c.for_each_elem(|i, v| f(i, v.map(|v| v as u64))),
        }
    }
}
