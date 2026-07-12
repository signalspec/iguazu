use serde::{Deserialize, Serialize};
use num_traits::Zero;

use crate::{ElementSize, io::RelativePath, schema::EntityData, summary::StoredSummaryMap};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "storage", rename_all = "snake_case")]
pub enum StreamRef {
    FlatFile {
        file_name: RelativePath,

        #[serde(alias = "element_type")] // Pre-0.1
        element_size: ElementSize,

        #[serde(default = "u64::zero", skip_serializing_if = "u64::is_zero")]
        offset: u64,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<u64>,
    },
    Inline(InlineData)
}

impl EntityData for StreamRef {
    type SummaryMap = StoredSummaryMap<StreamRef>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InlineData {
    F32(Vec<f32>),
    F64(Vec<f64>),
    U8(Vec<u8>),
    U16(Vec<u16>),
    U32(Vec<u32>),
    U64(Vec<u64>),
}
