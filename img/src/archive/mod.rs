use crate::header::HEADER_SIZE;
use crate::metadata::{
    DIRECTORY_ENTRY_SIZE, FLAG_FREE, FLAG_SCRATCH, ActiveDirectoryTable, IMGEntry, IMGMetadata,
    MAX_NAME_LEN,
};
use crate::IMGHeader;
use anyhow::{Context, Result, bail};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

mod schema;

/// Outcome of [`IMGArchive::add_file`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddFileResult {
    Added,
    DuplicateIgnored,
}

#[derive(Debug, Clone)]
enum ArchiveSource {
    Path(PathBuf),
    Buffer(Vec<u8>),
}

/// In-memory view of a full archive.
#[derive(Debug, Clone)]
pub struct IMGArchive {
    pub header: IMGHeader,
    pub(crate) directory: Vec<IMGEntry>,
    /// Absolute file offset where `payload_blob` begins.
    payload_base: u32,
    /// Contiguous on-disk payload region; loaded on first payload access.
    payload_blob: Option<Vec<u8>>,
    /// Pending additions; merged into `payload_blob` on rebase.
    scratch: Vec<u8>,
    /// Where to read the payload blob when it has not been loaded yet.
    source: Option<ArchiveSource>,
}

impl Default for IMGArchive {
    fn default() -> Self {
        Self {
            header: IMGHeader::default(),
            directory: Vec::new(),
            payload_base: HEADER_SIZE as u32,
            payload_blob: None,
            scratch: Vec::new(),
            source: None,
        }
    }
}

impl IMGArchive {
    /// Opens an archive from memory, parsing the directory only.
    ///
    /// Payload bytes are loaded as one blob on the first call to
    /// [`Self::load_payload`] or [`Self::load_payload_blob`].
    pub fn from_buf(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_SIZE {
            bail!("archive shorter than header");
        }
        let header: IMGHeader = wincode::deserialize(&bytes[..HEADER_SIZE])
            .context("failed to deserialize archive header")?;
        IMGMetadata::validate_header_for_file_len(&header, bytes.len())?;
        let metadata: IMGMetadata =
            wincode::deserialize(bytes).context("failed to deserialize archive metadata")?;
        let payload_base = metadata.payload_base();
        Ok(Self {
            header: metadata.header,
            directory: metadata.directory.into_entries(),
            payload_base,
            payload_blob: None,
            scratch: Vec::new(),
            source: Some(ArchiveSource::Buffer(bytes.to_vec())),
        })
    }

    /// Opens an archive from disk, reading the directory table only.
    ///
    /// Payload bytes are loaded as one blob on the first call to
    /// [`Self::load_payload`] or [`Self::load_payload_blob`].
    pub fn from_path(path: &Path) -> Result<Self> {
        let file_len = std::fs::metadata(path)
            .with_context(|| format!("failed to stat archive {}", path.display()))?
            .len() as usize;
        let mut file = BufReader::new(
            std::fs::File::open(path)
                .with_context(|| format!("failed to open archive {}", path.display()))?,
        );
        let header: IMGHeader = wincode::deserialize_from(&mut file)
            .with_context(|| format!("failed to read archive header from {}", path.display()))?;
        IMGMetadata::validate_header_for_file_len(&header, file_len)?;
        file.seek(SeekFrom::Start(0))
            .with_context(|| format!("failed to rewind archive {}", path.display()))?;
        let metadata: IMGMetadata = wincode::deserialize_from(&mut file)
            .with_context(|| format!("failed to read archive metadata from {}", path.display()))?;
        let payload_base = metadata.payload_base();
        Ok(Self {
            header: metadata.header,
            directory: metadata.directory.into_entries(),
            payload_base,
            payload_blob: None,
            scratch: Vec::new(),
            source: Some(ArchiveSource::Path(path.to_path_buf())),
        })
    }

    pub fn write(&mut self, path: &Path) -> Result<()> {
        self.rebase()?;
        let bytes = wincode::serialize(self).context("failed to serialize archive")?;
        std::fs::write(path, bytes)
            .with_context(|| format!("failed to write archive {}", path.display()))?;
        Ok(())
    }

    /// Returns active (non-free) directory entries.
    pub fn entries(&self) -> Vec<&IMGEntry> {
        self.directory
            .iter()
            .filter(|entry| !entry.is_free())
            .collect()
    }

    fn directory_index(&self, name: &str) -> Result<usize> {
        let entry = self
            .entry_by_name(name)
            .with_context(|| format!("{name} not found in archive"))?;
        self.directory
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .context("active entry missing from directory table")
    }

    fn entry_is_active(&self, entry: &IMGEntry) -> bool {
        self.entries()
            .iter()
            .any(|active| std::ptr::eq(*active, entry))
    }

    /// Returns active entries sorted by name with computed layout offsets for display.
    pub fn list_entries(&self) -> Vec<(u32, &IMGEntry)> {
        let mut entries: Vec<_> = self.entries();
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        let mut offset = self.get_payload_start();
        entries
            .into_iter()
            .map(|entry| {
                let entry_offset = offset as u32;
                offset += entry.sectors as usize * crate::SECTOR_SIZE;
                (entry_offset, entry)
            })
            .collect()
    }

    /// Loads the full on-disk payload blob if it has not been loaded yet.
    pub fn load_payload_blob(&mut self) -> Result<&[u8]> {
        self.ensure_payload_loaded()?;
        Ok(self.payload_blob.as_deref().expect("payload loaded"))
    }

    /// Returns the payload for a directory-table index.
    pub fn load_payload(&mut self, index: usize) -> Result<&[u8]> {
        self.ensure_payload_loaded()?;
        self.payload_at(index)
            .with_context(|| format!("failed to load payload for entry {index}"))
    }

    /// Returns the active entry with `name`, if present.
    pub fn entry_by_name(&self, name: &str) -> Option<&IMGEntry> {
        self.entries().into_iter().find(|entry| entry.name == name)
    }

    /// Returns the payload for an active entry by name.
    pub fn load_payload_by_name(&mut self, name: &str) -> Result<&[u8]> {
        let index = self.directory_index(name)?;
        self.load_payload(index)
    }

    /// Returns the logical file bytes for an active entry by name.
    pub fn read_file(&mut self, name: &str) -> Result<Vec<u8>> {
        let index = self.directory_index(name)?;
        self.ensure_payload_loaded()?;
        let entry = &self.directory[index];
        let payload = self.payload_at(index)?;
        let len = entry.content_len(payload);
        Ok(payload[..len].to_vec())
    }

    /// Returns the payload for a directory-table index when the blob is already loaded.
    pub fn payload(&self, index: usize) -> Option<&[u8]> {
        self.payload_blob.as_ref()?;
        self.payload_at(index).ok()
    }

    /// Returns true when the on-disk payload blob is not in memory yet.
    pub fn payload_loaded(&self) -> bool {
        self.payload_blob.is_some()
    }

    /// Appends a file to the scratch buffer. `offset` is scratch-relative until
    /// [`Self::rebase`] runs.
    ///
    /// Returns [`AddFileResult::DuplicateIgnored`] when an active entry with the
    /// same name already exists. Rejects names longer than [`MAX_NAME_LEN`] bytes.
    pub fn add_file(&mut self, buf: &[u8], name: impl Into<String>) -> Result<AddFileResult> {
        let name = name.into();
        if name.len() > MAX_NAME_LEN {
            bail!("filename exceeds maximum length of {MAX_NAME_LEN} bytes: {name}");
        }
        if self.entries().iter().any(|entry| entry.name == name) {
            return Ok(AddFileResult::DuplicateIgnored);
        }

        let sectors = buf.len().div_ceil(crate::SECTOR_SIZE);
        let padded_len = sectors * crate::SECTOR_SIZE;

        let scratch_offset = self.scratch.len() as u32;
        self.scratch.extend_from_slice(buf);
        self.scratch.resize(self.scratch.len() + padded_len - buf.len(), 0);

        self.directory.push(IMGEntry {
            sectors: sectors as u16,
            offset: scratch_offset,
            size: 0,
            name,
            flags: FLAG_SCRATCH,
        });
        self.sync_header_count();
        Ok(AddFileResult::Added)
    }

    /// Marks all active entries with `name` as free without modifying the main payload blob.
    ///
    /// Returns the number of entries tombstoned.
    pub fn remove_file(&mut self, name: impl AsRef<str>) -> usize {
        let name = name.as_ref();
        let removed = self.entries().iter().filter(|entry| entry.name == name).count();
        if removed == 0 {
            return 0;
        }
        for entry in &mut self.directory {
            if entry.name == name {
                entry.flags |= FLAG_FREE;
            }
        }
        self.sync_header_count();
        removed
    }

    /// Drops free entries, merges scratch additions, and rebuilds the payload blob.
    ///
    /// Entries are sorted by name (null-padded, lexicographic) so listing
    /// and on-disk layout stay deterministic.
    pub fn rebase(&mut self) -> Result<()> {
        let needs_on_disk_blob = self.entries().iter().any(|entry| !entry.is_scratch());
        if needs_on_disk_blob {
            self.ensure_payload_loaded()?;
        }

        let payload_start = self.get_payload_start();

        let mut entries: Vec<IMGEntry> = self.entries().iter().map(|entry| (*entry).clone()).collect();
        self.directory.clear();
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        let mut new_blob = Vec::new();
        let mut offset = payload_start;

        for entry in &mut entries {
            let len = entry.sectors as usize * crate::SECTOR_SIZE;
            let data = if entry.is_scratch() {
                let start = entry.offset as usize;
                self.scratch[start..start + len].to_vec()
            } else {
                let blob = self
                    .payload_blob
                    .as_ref()
                    .context("main blob present for on-disk entry")?;
                let start = entry.offset as usize - self.payload_base as usize;
                blob[start..start + len].to_vec()
            };

            entry.offset = offset as u32;
            entry.flags &= !FLAG_SCRATCH;
            offset += len;
            new_blob.extend_from_slice(&data);
        }

        self.directory = entries;
        self.payload_base = payload_start as u32;
        self.payload_blob = Some(new_blob);
        self.scratch.clear();
        self.source = None;
        self.sync_header_count();
        Ok(())
    }

    fn ensure_payload_loaded(&mut self) -> Result<()> {
        if self.payload_blob.is_some() {
            return Ok(());
        }

        let payload_len = self.directory.on_disk_payload_len();
        if payload_len == 0 {
            self.payload_blob = Some(Vec::new());
            self.source = None;
            return Ok(());
        }

        let Some(source) = self.source.take() else {
            bail!("no payload source available");
        };

        let blob = match source {
            ArchiveSource::Path(path) => {
                let mut file = std::fs::File::open(&path)
                    .with_context(|| format!("failed to open archive {}", path.display()))?;
                file.seek(SeekFrom::Start(self.payload_base as u64))
                    .with_context(|| format!("failed to seek to payload at {}", path.display()))?;
                read_exact(&mut file, payload_len).with_context(|| {
                    format!("failed to read payload blob from {}", path.display())
                })?
            }
            ArchiveSource::Buffer(bytes) => {
                let start = self.payload_base as usize;
                let end = start + payload_len;
                if bytes.len() < end {
                    bail!("archive buffer truncated at payload region");
                }
                bytes[start..end].to_vec()
            }
        };

        self.payload_blob = Some(blob);
        Ok(())
    }

    fn payload_at(&self, index: usize) -> Result<&[u8]> {
        let entry = self
            .directory
            .get(index)
            .with_context(|| format!("entry index {index} out of range"))?;
        if !self.entry_is_active(entry) {
            bail!("entry {index} is marked free");
        }

        let len = entry.sectors as usize * crate::SECTOR_SIZE;
        if entry.is_scratch() {
            let start = entry.offset as usize;
            return Ok(&self.scratch[start..start + len]);
        }

        let blob = self
            .payload_blob
            .as_ref()
            .context("no payload blob available")?;
        let start = entry.offset as usize - self.payload_base as usize;
        Ok(&blob[start..start + len])
    }

    fn get_payload_start(&self) -> usize {
        HEADER_SIZE + self.entries().len() * DIRECTORY_ENTRY_SIZE
    }

    fn sync_header_count(&mut self) {
        self.header.count = self.entries().len() as u32;
    }
}

fn read_exact(mut reader: impl Read, len: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .context("failed to read expected number of bytes")?;
    Ok(buf)
}

#[cfg(test)]
mod tests;
