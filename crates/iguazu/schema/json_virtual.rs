use serde::{Deserialize, Serialize};
use num_traits::Zero;

use crate::{io::RelativePath, ElementSize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "storage", rename_all = "snake_case")]
pub enum StreamRef {
    FlatFile {
        file_name: RelativePath,

        #[serde(alias = "element_type")] // Pre-0.1
        element_size: ElementSize,

        #[serde(default = "u64::zero", skip_serializing_if = "u64::is_zero")]
        offset: u64,
    }
}
