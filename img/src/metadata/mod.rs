use crate::header::HEADER_SIZE;
use anyhow::{Context, Result, bail};
use wincode::config::Config;
use wincode::io::Reader;
use wincode::{ReadResult, SchemaRead};

mod schema;

pub(crate) const FLAG_FREE: u8 = 1;
pub(crate) const FLAG_SCRATCH: u8 = 2;

/// Maximum filename length stored in a directory entry (excluding NUL).
pub const MAX_NAME_LEN: usize = 23;

const NAME_FIELD_SIZE: usize = 24;

/// On-disk directory entry size in bytes.
pub(crate) const DIRECTORY_ENTRY_SIZE: usize = 32;

/// Directory entry.
///
/// In memory the name is a [`String`]. On disk the name is a fixed 24-byte,
/// null-padded ASCII field (see [`encode_name_for_disk`]).
///
/// ```text
/// offset  size  field
/// 0x00    4     offset (byte position of payload in archive)
/// 0x04    2     sectors (payload length in 2048-byte sectors)
/// 0x06    2     size (reserved; always 0 on disk)
/// 0x08   24     name (null-padded ASCII, max 23 chars + NUL)
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IMGDirectoryEntry {
    pub offset: u32,
    pub sectors: u16,
    pub size: u16,
    pub name: String,
}

impl IMGDirectoryEntry {
    pub fn encode_name_for_disk(name: &str) -> [u8; NAME_FIELD_SIZE] {
        let mut array = [0u8; NAME_FIELD_SIZE];
        for (index, byte) in name.bytes().take(MAX_NAME_LEN).enumerate() {
            array[index] = byte;
        }
        array
    }

    fn decode_name_from_disk(name: &[u8; NAME_FIELD_SIZE]) -> String {
        let end = name.iter().position(|&byte| byte == 0).unwrap_or(name.len());
        String::from_utf8_lossy(&name[..end]).into_owned()
    }
}

/// On-disk directory table: a contiguous sequence of [`IMGDirectoryEntry`] records.
///
/// Entry count comes from [`IMGHeader::count`](crate::IMGHeader::count); the table has no
/// separate length prefix.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IMGDirectory {
    pub entries: Vec<IMGDirectoryEntry>,
}

impl IMGDirectory {
    pub fn read_from_bytes(bytes: &[u8], count: u32) -> Result<Self> {
        let expected_len = count as usize * DIRECTORY_ENTRY_SIZE;
        if bytes.len() < expected_len {
            bail!("directory table truncated");
        }

        let mut entries = Vec::with_capacity(count as usize);
        for index in 0..count as usize {
            let start = index * DIRECTORY_ENTRY_SIZE;
            let end = start + DIRECTORY_ENTRY_SIZE;
            entries.push(
                wincode::deserialize(&bytes[start..end])
                    .context("failed to deserialize directory entry")?,
            );
        }
        Ok(Self { entries })
    }

    pub fn read_from<'de, C: Config>(
        mut reader: impl Reader<'de>,
        count: u32,
    ) -> ReadResult<Self> {
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            entries.push(<IMGDirectoryEntry as SchemaRead<'de, C>>::get(
                reader.by_ref(),
            )?);
        }
        Ok(Self { entries })
    }

    pub fn into_entries(self) -> Vec<IMGEntry> {
        self.entries.into_iter().map(IMGEntry::from).collect()
    }

    /// Total byte length of on-disk payload regions referenced by this directory table.
    pub fn on_disk_payload_len(&self) -> usize {
        self.entries
            .iter()
            .map(|entry| entry.sectors as usize * crate::SECTOR_SIZE)
            .sum()
    }
}

/// In-memory directory tables that may include scratch entries.
pub(crate) trait ActiveDirectoryTable {
    fn on_disk_payload_len(&self) -> usize;
}

impl ActiveDirectoryTable for Vec<IMGEntry> {
    fn on_disk_payload_len(&self) -> usize {
        self.iter()
            .filter(|entry| !entry.is_scratch())
            .map(|entry| entry.sectors as usize * crate::SECTOR_SIZE)
            .sum()
    }
}

/// Header plus directory table — the metadata prefix of an archive, without payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IMGMetadata {
    pub header: crate::IMGHeader,
    pub directory: IMGDirectory,
}

impl IMGMetadata {
    pub fn payload_base(&self) -> u32 {
        (HEADER_SIZE + self.header.count as usize * DIRECTORY_ENTRY_SIZE) as u32
    }

    /// Upper bound on directory entries that fit in `file_len` bytes.
    pub fn max_directory_entries(file_len: usize) -> u32 {
        if file_len <= HEADER_SIZE {
            0
        } else {
            ((file_len - HEADER_SIZE) / DIRECTORY_ENTRY_SIZE) as u32
        }
    }

    pub fn validate_header_for_file_len(header: &crate::IMGHeader, file_len: usize) -> Result<()> {
        if !header.is_ver2() {
            bail!("invalid archive signature: expected VER2");
        }
        let max_entries = Self::max_directory_entries(file_len);
        if header.count > max_entries {
            bail!(
                "directory entry count {} exceeds maximum {} for a {}-byte file",
                header.count,
                max_entries,
                file_len
            );
        }
        let metadata_len = HEADER_SIZE + header.count as usize * DIRECTORY_ENTRY_SIZE;
        if file_len < metadata_len {
            bail!("archive truncated: metadata requires {metadata_len} bytes, file has {file_len}");
        }
        Ok(())
    }

    pub fn validate_for_file_len(&self, file_len: usize) -> Result<()> {
        Self::validate_header_for_file_len(&self.header, file_len)
    }
}

/// Per-file directory record (in-memory; may carry scratch/free flags).
///
/// On-disk layout is [`IMGDirectoryEntry`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IMGEntry {
    pub offset: u32,
    pub sectors: u16,
    pub size: u16,
    pub name: String,
    pub(crate) flags: u8,
}

impl From<IMGDirectoryEntry> for IMGEntry {
    fn from(entry: IMGDirectoryEntry) -> Self {
        Self {
            offset: entry.offset,
            sectors: entry.sectors,
            size: entry.size,
            name: entry.name,
            flags: 0,
        }
    }
}

impl From<&IMGEntry> for IMGDirectoryEntry {
    fn from(entry: &IMGEntry) -> Self {
        Self {
            offset: entry.offset,
            sectors: entry.sectors,
            size: entry.size,
            name: entry.name.clone(),
        }
    }
}

impl IMGEntry {
    pub fn is_free(&self) -> bool {
        self.flags & FLAG_FREE != 0
    }

    pub(crate) fn is_scratch(&self) -> bool {
        self.flags & FLAG_SCRATCH != 0
    }

    /// Stored size in bytes, using sector count when `size` is unset.
    pub fn stored_size(&self) -> u32 {
        if self.size == 0 {
            self.sectors as u32 * crate::SECTOR_SIZE as u32
        } else {
            self.size as u32
        }
    }

    /// Logical content length in bytes, trimming sector padding when `size` is unset.
    pub fn content_len(&self, payload: &[u8]) -> usize {
        if self.size != 0 {
            return self.size as usize;
        }
        payload
            .iter()
            .rposition(|&byte| byte != 0)
            .map(|index| index + 1)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests;
