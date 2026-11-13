use serde::{Deserialize, Serialize};

use crate::ElementSize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Endian {
    #[default]
    Little,
    Big,
}

impl std::str::FromStr for Endian {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "little" | "le" => Ok(Endian::Little),
            "big" | "be" => Ok(Endian::Big),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for Endian {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Endian::Little => write!(f, "little"),
            Endian::Big => write!(f, "big"),
        }
    }
}

impl Endian {
    pub fn native() -> Self {
        cfg_select! {
            target_endian = "little" => Endian::Little,
            target_endian = "big" => Endian::Big,
        }
    }

    pub(crate) fn bswap(self, element_size: ElementSize, data: &mut [u8]) {
        if self != Endian::native() && !data.is_empty() {
            match element_size {
                ElementSize::U8 => (),
                ElementSize::U16 => bytemuck::cast_slice_mut::<u8, u16>(data).iter_mut().for_each(|e| *e = e.swap_bytes()),
                ElementSize::U32 => bytemuck::cast_slice_mut::<u8, u32>(data).iter_mut().for_each(|e| *e = e.swap_bytes()),
                ElementSize::U64 => bytemuck::cast_slice_mut::<u8, u64>(data).iter_mut().for_each(|e| *e = e.swap_bytes()),
            }
        }
    }
}
