use wincode::{SchemaRead, SchemaWrite};

/// Expected on-disk signature for IMG V2 archives.
pub const VER2_SIGNATURE: [u8; 4] = *b"VER2";

/// On-disk archive header.
///
/// ```text
/// offset  size  field
/// 0x00    4     signature (ASCII "VER2")
/// 0x04    4     entry count (u32, little-endian)
/// ```
#[derive(Debug, Clone, PartialEq, Eq, SchemaRead, SchemaWrite)]
pub struct IMGHeader {
    pub sig: [u8; 4],
    pub count: u32,
}

pub(crate) const HEADER_SIZE: usize = std::mem::size_of::<IMGHeader>();

impl Default for IMGHeader {
    fn default() -> Self {
        Self {
            sig: VER2_SIGNATURE,
            count: 0,
        }
    }
}

impl IMGHeader {
    pub fn is_ver2(&self) -> bool {
        self.sig == VER2_SIGNATURE
    }
}
