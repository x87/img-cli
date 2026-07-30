//! IMG V2 archive library.
//!
//! Implements a sector-aligned container format for bundling named files into
//! a single binary archive. Payload bytes are held in one contiguous blob;
//! new files land in a scratch buffer and deleted files are tombstoned until
//! [`IMGArchive::rebase`] compacts the archive.

mod archive;
mod header;
mod metadata;

#[cfg(test)]
mod tests;

pub use archive::{AddFileResult, IMGArchive};
pub use header::{IMGHeader, VER2_SIGNATURE};
pub use metadata::{
    IMGDirectory, IMGDirectoryEntry, IMGEntry, IMGMetadata, MAX_NAME_LEN,
};

/// Sector size in bytes. All file payloads are padded to this boundary.
pub const SECTOR_SIZE: usize = 2048;
