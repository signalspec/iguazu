use serde::{Deserialize, Serialize};
use num_traits::Zero;

use crate::{io::RelativePath, ElementType};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "storage", rename_all = "snake_case")]
pub enum StreamRef {
    FlatFile {
        file_name: RelativePath,
        element_type: ElementType,

        #[serde(default = "u64::zero", skip_serializing_if = "u64::is_zero")]
        offset: u64,
    }
}
