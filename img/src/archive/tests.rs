use super::*;
use crate::tests::spec_verify::verify_on_disk_spec;
use std::fs;
use std::path::{Path, PathBuf};

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("img-lib-{name}-{}", std::process::id()))
}

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("img-lib-{name}-{}", std::process::id()))
}

fn temp_path_nested(name: &str, parts: &[&str]) -> PathBuf {
    let path = parts
        .iter()
        .fold(temp_root(name), |base, part| base.join(part));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    path
}

fn cleanup(path: &Path) {
    let _ = fs::remove_file(path);
}

fn cleanup_tree(name: &str) {
    let _ = fs::remove_dir_all(temp_root(name));
}

fn directory_index(archive: &IMGArchive, name: &str) -> usize {
    archive
        .directory
        .iter()
        .position(|entry| entry.name == name)
        .expect("entry in directory")
}

fn error_chain_contains(err: &anyhow::Error, needle: &str) -> bool {
    format!("{err:#}").contains(needle)
}

#[test]
fn stored_size_uses_sector_count_when_size_is_zero() {
    let entry = IMGEntry {
        sectors: 2,
        ..Default::default()
    };
    assert_eq!(entry.stored_size(), 4096);
}

#[test]
fn stored_size_uses_explicit_size_when_set() {
    let entry = IMGEntry {
        sectors: 2,
        size: 100,
        ..Default::default()
    };
    assert_eq!(entry.stored_size(), 100);
}

#[test]
fn empty_archive_roundtrip() {
    let path = temp_path("empty");
    cleanup(&path);

    let mut archive = IMGArchive::default();
    archive.write(&path).unwrap();

    let loaded = IMGArchive::from_path(&path).unwrap();
    assert_eq!(loaded.header.sig, *b"VER2");
    assert_eq!(loaded.header.count, 0);
    assert!(loaded.entries().is_empty());

    cleanup(&path);
}

#[test]
fn from_buf_defers_payload_until_access() {
    let path = temp_path("from-buf");
    cleanup(&path);

    let mut archive = IMGArchive::default();
    archive.add_file(b"payload", "file.txt").unwrap();
    archive.write(&path).unwrap();

    let bytes = fs::read(&path).unwrap();
    let mut loaded = IMGArchive::from_buf(&bytes).unwrap();
    assert_eq!(loaded.header.sig, *b"VER2");
    assert_eq!(loaded.entries().len(), 1);
    assert_eq!(loaded.entries()[0].name, "file.txt");
    assert!(!loaded.payload_loaded());

    let index = directory_index(&loaded, "file.txt");
    let payload = loaded.load_payload(index).unwrap();
    assert_eq!(&payload[..7], b"payload");
    assert!(loaded.payload_loaded());

    cleanup(&path);
}

#[test]
fn from_path_lists_without_loading_payload() {
    let path = temp_path("list-shallow");
    cleanup(&path);

    let mut archive = IMGArchive::default();
    archive.add_file(b"one", "a.txt").unwrap();
    archive.add_file(b"two", "b.txt").unwrap();
    archive.write(&path).unwrap();

    let archive = IMGArchive::from_path(&path).unwrap();
    assert!(!archive.payload_loaded());
    let listing = archive.list_entries();
    assert_eq!(listing.len(), 2);
    assert_eq!(listing[0].1.name, "a.txt");
    assert_eq!(listing[1].1.name, "b.txt");

    cleanup(&path);
}

#[test]
fn write_read_roundtrip_with_payload_blob() {
    let path = temp_path("blob");
    cleanup(&path);

    let content = b"hello world";

    let mut archive = IMGArchive::default();
    archive.add_file(content, "hello.txt").unwrap();
    archive.write(&path).unwrap();

    let mut loaded = IMGArchive::from_path(&path).unwrap();
    assert_eq!(loaded.entries().len(), 1);
    assert_eq!(loaded.entries()[0].name, "hello.txt");
    assert_eq!(loaded.entries()[0].stored_size(), crate::SECTOR_SIZE as u32);
    assert!(!loaded.payload_loaded());

    let index = directory_index(&loaded, "hello.txt");
    let payload = loaded.load_payload(index).unwrap();
    assert_eq!(&payload[..content.len()], content);
    assert_eq!(payload.len(), crate::SECTOR_SIZE);
    assert!(payload[content.len()..].iter().all(|byte| *byte == 0));

    cleanup(&path);
}

#[test]
fn rebase_sorts_entries_and_assigns_offsets() {
    let mut archive = IMGArchive::default();
    archive.add_file(b"z", "z.txt").unwrap();
    archive.add_file(b"a", "a.txt").unwrap();
    archive.add_file(b"m", "m.txt").unwrap();
    archive.rebase().unwrap();

    let names: Vec<_> = archive
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, vec!["a.txt", "m.txt", "z.txt"]);
    assert_eq!(archive.header.count, 3);

    // Offsets are relative to the start of the payload region.
    assert_eq!(archive.entries()[0].offset, 0);
    assert_eq!(archive.entries()[1].offset, crate::SECTOR_SIZE as u32);
    assert_eq!(archive.entries()[2].offset, 2 * crate::SECTOR_SIZE as u32);
}

#[test]
fn list_entries_computes_offsets_without_mutating_archive() {
    let mut archive = IMGArchive::default();
    archive.add_file(b"a", "b.txt").unwrap();
    archive.add_file(b"b", "a.txt").unwrap();
    let original_offsets: Vec<_> = archive.entries().iter().map(|entry| entry.offset).collect();

    let listing = archive.list_entries();
    assert_eq!(listing.len(), 2);
    assert_eq!(listing[0].1.name, "a.txt");
    assert_eq!(listing[1].1.name, "b.txt");

    let first_offset = crate::metadata::payload_base_for_count(2);
    assert_eq!(listing[0].0, first_offset as u32);
    assert_eq!(listing[1].0, (first_offset + crate::SECTOR_SIZE) as u32);
    assert_eq!(
        archive
            .entries()
            .iter()
            .map(|entry| entry.offset)
            .collect::<Vec<_>>(),
        original_offsets
    );
}

#[test]
fn remove_file_marks_free_without_touching_blob_until_rebase() {
    let path = temp_path("remove-deferred");
    cleanup(&path);

    let mut archive = IMGArchive::default();
    archive.add_file(b"keep", "keep.txt").unwrap();
    archive.add_file(b"drop", "drop.txt").unwrap();
    archive.write(&path).unwrap();

    let mut archive = IMGArchive::from_path(&path).unwrap();
    assert!(!archive.payload_loaded());
    archive.remove_file("drop.txt");
    assert!(!archive.payload_loaded());
    assert_eq!(archive.entries().len(), 1);
    assert_eq!(archive.directory.len(), 2);

    archive.rebase().unwrap();
    assert_eq!(archive.directory.len(), 1);
    assert_eq!(archive.entries()[0].name, "keep.txt");
    assert_eq!(
        archive.payload_blob.as_ref().unwrap().len(),
        crate::SECTOR_SIZE
    );

    cleanup(&path);
}

#[test]
fn remove_file_then_write_updates_archive() {
    let path = temp_path("remove");
    cleanup(&path);

    let mut archive = IMGArchive::default();
    archive.add_file(b"keep", "keep.txt").unwrap();
    archive.add_file(b"drop", "drop.txt").unwrap();
    archive.write(&path).unwrap();

    let mut archive = IMGArchive::from_path(&path).unwrap();
    archive.remove_file("drop.txt");
    archive.write(&path).unwrap();

    let mut loaded = IMGArchive::from_path(&path).unwrap();
    assert_eq!(loaded.entries().len(), 1);
    assert_eq!(loaded.entries()[0].name, "keep.txt");

    let index = directory_index(&loaded, "keep.txt");
    let payload = loaded.load_payload(index).unwrap();
    assert_eq!(payload[..4], *b"keep");

    cleanup(&path);
}

#[test]
fn add_to_existing_archive_uses_scratch_until_rebase() {
    let path = temp_path("append");
    cleanup(&path);

    let mut archive = IMGArchive::default();
    archive.add_file(b"first", "first.txt").unwrap();
    archive.write(&path).unwrap();

    let mut archive = IMGArchive::from_path(&path).unwrap();
    assert!(!archive.payload_loaded());
    archive.add_file(b"second", "second.txt").unwrap();
    assert!(!archive.payload_loaded());
    assert!(!archive.scratch.is_empty());
    assert_eq!(archive.entries().len(), 2);

    archive.write(&path).unwrap();

    let mut loaded = IMGArchive::from_path(&path).unwrap();
    assert_eq!(loaded.entries().len(), 2);
    assert_eq!(loaded.entries()[0].name, "first.txt");
    assert_eq!(loaded.entries()[1].name, "second.txt");
    assert_eq!(
        &loaded
            .load_payload(directory_index(&loaded, "first.txt"))
            .unwrap()[..5],
        b"first"
    );
    assert_eq!(
        &loaded
            .load_payload(directory_index(&loaded, "second.txt"))
            .unwrap()[..6],
        b"second"
    );

    cleanup(&path);
}

#[test]
fn many_removals_rebase_once() {
    let path = temp_path("many-remove");
    cleanup(&path);

    let mut archive = IMGArchive::default();
    for i in 0..10 {
        let name = format!("file{i:02}.txt");
        archive
            .add_file(format!("data{i}").as_bytes(), &name)
            .unwrap();
    }
    archive.write(&path).unwrap();

    let mut archive = IMGArchive::from_path(&path).unwrap();
    assert!(!archive.payload_loaded());
    for i in (0..10).step_by(2) {
        let name = format!("file{i:02}.txt");
        archive.remove_file(&name);
    }
    assert!(!archive.payload_loaded());
    assert_eq!(archive.entries().len(), 5);

    archive.rebase().unwrap();
    assert_eq!(
        archive.payload_blob.as_ref().unwrap().len(),
        5 * crate::SECTOR_SIZE
    );
    assert_eq!(archive.directory.len(), 5);

    cleanup(&path);
}

fn assert_padded(content: &[u8], expected: &[u8]) {
    assert!(
        content.len() >= expected.len(),
        "content {}B shorter than expected {}B",
        content.len(),
        expected.len()
    );
    assert_eq!(
        &content[..expected.len()],
        expected,
        "content prefix mismatch"
    );
    assert!(
        content[expected.len()..].iter().all(|b| *b == 0),
        "trailing bytes must be zero padding"
    );
}

#[test]
fn read_file_returns_logical_bytes() {
    let path = temp_path("read-file");
    cleanup(&path);

    let mut archive = IMGArchive::default();
    archive.add_file(b"hello", "greet.txt").unwrap();
    archive.write(&path).unwrap();

    let mut loaded = IMGArchive::from_path(&path).unwrap();
    assert_padded(&loaded.read_file("greet.txt").unwrap(), b"hello");

    cleanup(&path);
}

#[test]
fn load_payload_by_name_returns_entry_data() {
    let path = temp_path("by-name");
    cleanup(&path);

    let mut archive = IMGArchive::default();
    archive.add_file(b"hello", "greet.txt").unwrap();
    archive.write(&path).unwrap();

    let mut loaded = IMGArchive::from_path(&path).unwrap();
    let payload = loaded.load_payload_by_name("greet.txt").unwrap();
    assert_eq!(&payload[..5], b"hello");

    cleanup(&path);
}

#[test]
fn load_payload_by_name_errors_when_missing() {
    let path = temp_path("by-name-missing");
    cleanup(&path);

    let mut archive = IMGArchive::default();
    archive.write(&path).unwrap();

    let mut loaded = IMGArchive::from_path(&path).unwrap();
    let err = loaded.load_payload_by_name("missing.txt").unwrap_err();
    assert!(error_chain_contains(&err, "not found in archive"));

    cleanup(&path);
}

#[test]
fn hundred_file_archive_matches_spec_on_disk() {
    let path = temp_path("hundred-spec");
    cleanup(&path);

    let files: Vec<(String, Vec<u8>)> = (0..100)
        .map(|index| {
            let name = format!("file{index:02}.txt");
            // Vary payload lengths to exercise sector padding (1..=512 bytes).
            let content: Vec<u8> = (0..((index * 17 % 512) + 1)).map(|b| b as u8).collect();
            (name, content)
        })
        .collect();

    let mut archive = IMGArchive::default();
    // Add in reverse order; write/rebase must sort by name on disk.
    for (name, content) in files.iter().rev() {
        archive.add_file(content, name.clone()).unwrap();
    }
    archive.write(&path).unwrap();

    let bytes = fs::read(&path).unwrap();
    verify_on_disk_spec(&bytes, &files);

    let mut loaded = IMGArchive::from_path(&path).unwrap();
    assert_eq!(loaded.entries().len(), 100);
    for (name, content) in &files {
        let index = directory_index(&loaded, name);
        let payload = loaded.load_payload(index).unwrap();
        assert_eq!(&payload[..content.len()], content.as_slice());
    }

    cleanup(&path);
}

#[test]
fn from_buf_rejects_invalid_signature() {
    let mut bytes = vec![0u8; HEADER_SIZE];
    bytes[0..4].copy_from_slice(b"BAD!");
    bytes[4..8].copy_from_slice(&0u32.to_le_bytes());
    let err = IMGArchive::from_buf(&bytes).unwrap_err();
    assert!(err.to_string().contains("invalid archive signature"));
}

#[test]
fn from_buf_rejects_excessive_entry_count() {
    let mut bytes = vec![0u8; HEADER_SIZE];
    bytes[0..4].copy_from_slice(b"VER2");
    bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
    let err = IMGArchive::from_buf(&bytes).unwrap_err();
    assert!(err.to_string().contains("exceeds maximum"));
}

#[test]
fn add_file_rejects_long_name() {
    let mut archive = IMGArchive::default();
    let long_name = "a".repeat(MAX_NAME_LEN + 1);
    let err = archive.add_file(b"x", long_name).unwrap_err();
    assert!(err.to_string().contains("maximum length"));
}

#[test]
fn add_file_ignores_duplicate_name() {
    let mut archive = IMGArchive::default();
    assert_eq!(
        archive.add_file(b"first", "file.txt").unwrap(),
        AddFileResult::Added
    );
    assert_eq!(
        archive.add_file(b"second", "file.txt").unwrap(),
        AddFileResult::DuplicateIgnored
    );
    assert_eq!(archive.entries().len(), 1);
}

#[test]
fn remove_file_tombstones_all_matching_names() {
    let mut archive = IMGArchive::default();
    archive.add_file(b"a", "dup.txt").unwrap();
    archive.add_file(b"b", "other.txt").unwrap();
    // Force a duplicate by manipulating directory directly (add_file would ignore).
    archive.directory.push(IMGEntry {
        sectors: 1,
        offset: 0,
        size: 0,
        name: "dup.txt".to_string(),
        flags: FLAG_SCRATCH,
    });
    archive.sync_header_count();

    assert_eq!(archive.remove_file("dup.txt"), 2);
    assert_eq!(archive.entries().len(), 1);
    assert_eq!(archive.entries()[0].name, "other.txt");
}

#[test]
fn empty_file_has_zero_sectors_and_no_payload() {
    let path = temp_path("empty-file");
    cleanup(&path);

    let mut archive = IMGArchive::default();
    archive.add_file(b"", "empty.txt").unwrap();
    archive.write(&path).unwrap();

    let bytes = fs::read(&path).unwrap();
    verify_on_disk_spec(&bytes, &[("empty.txt".to_string(), Vec::new())]);

    let mut loaded = IMGArchive::from_path(&path).unwrap();
    let index = directory_index(&loaded, "empty.txt");
    assert_eq!(loaded.entries()[0].sectors, 0);
    let payload = loaded.load_payload(index).unwrap();
    assert!(payload.is_empty());

    cleanup(&path);
}

#[test]
fn rebase_propagates_payload_load_error() {
    let mut archive = IMGArchive::default();
    archive.directory.push(IMGEntry {
        offset: HEADER_SIZE as u32,
        sectors: 1,
        size: 0,
        name: "missing.txt".to_string(),
        flags: 0,
    });
    archive.header.count = 1;
    archive.sync_header_count();

    let err = archive.rebase().unwrap_err();
    assert!(err.to_string().contains("no payload source"));
}

#[test]
fn from_buf_rejects_buffer_shorter_than_header() {
    let err = IMGArchive::from_buf(&[0u8; HEADER_SIZE - 1]).unwrap_err();
    assert!(err.to_string().contains("shorter than header"));
}

#[test]
fn from_path_rejects_invalid_signature() {
    let path = temp_path("bad-sig");
    cleanup(&path);

    let mut bytes = vec![0u8; HEADER_SIZE];
    bytes[0..4].copy_from_slice(b"BAD!");
    fs::write(&path, bytes).unwrap();

    let err = IMGArchive::from_path(&path).unwrap_err();
    assert!(error_chain_contains(&err, "invalid archive signature"));

    cleanup(&path);
}

#[test]
fn from_path_rejects_missing_file() {
    let path = temp_path("does-not-exist");
    cleanup(&path);

    let err = IMGArchive::from_path(&path).unwrap_err();
    assert!(error_chain_contains(&err, "failed to stat archive"));

    cleanup(&path);
}

#[test]
fn load_payload_errors_on_out_of_range_index() {
    let mut archive = IMGArchive::default();
    archive.add_file(b"x", "a.txt").unwrap();

    let err = archive.load_payload(99).unwrap_err();
    assert!(error_chain_contains(&err, "entry index 99 out of range"));
}

#[test]
fn load_payload_errors_on_free_entry() {
    let mut archive = IMGArchive::default();
    archive.add_file(b"x", "a.txt").unwrap();
    archive.remove_file("a.txt");

    let err = archive.load_payload(0).unwrap_err();
    assert!(error_chain_contains(&err, "entry 0 is marked free"));
}

#[test]
fn load_payload_errors_on_truncated_buffer() {
    let path = temp_path("trunc-buf");
    cleanup(&path);

    let mut archive = IMGArchive::default();
    archive.add_file(b"hello", "a.txt").unwrap();
    archive.write(&path).unwrap();

    let mut bytes = fs::read(&path).unwrap();
    bytes.truncate(crate::metadata::metadata_len(1) + 16);
    let mut loaded = IMGArchive::from_buf(&bytes).unwrap();

    let err = loaded.load_payload(0).unwrap_err();
    assert!(err.to_string().contains("truncated at payload region"));

    cleanup(&path);
}

#[test]
fn load_payload_errors_on_truncated_file() {
    let path = temp_path("trunc-file");
    cleanup(&path);

    let mut archive = IMGArchive::default();
    archive.add_file(b"hello", "a.txt").unwrap();
    archive.write(&path).unwrap();

    let mut bytes = fs::read(&path).unwrap();
    bytes.truncate(crate::metadata::metadata_len(1) + 16);
    fs::write(&path, bytes).unwrap();

    let mut loaded = IMGArchive::from_path(&path).unwrap();
    let err = loaded.load_payload(0).unwrap_err();
    assert!(err.to_string().contains("failed to read payload blob"));

    cleanup(&path);
}

#[test]
fn remove_file_returns_zero_for_missing_name() {
    let mut archive = IMGArchive::default();
    archive.add_file(b"x", "a.txt").unwrap();
    assert_eq!(archive.remove_file("missing.txt"), 0);
    assert_eq!(archive.entries().len(), 1);
}

#[test]
fn add_file_rejects_name_at_max_plus_one() {
    let mut archive = IMGArchive::default();
    let name = "x".repeat(MAX_NAME_LEN + 1);
    assert!(archive.add_file(b"data", name).is_err());
}

#[test]
fn add_file_accepts_name_at_max_length() {
    let mut archive = IMGArchive::default();
    let name = "x".repeat(MAX_NAME_LEN);
    assert_eq!(
        archive.add_file(b"data", name).unwrap(),
        AddFileResult::Added
    );
    assert_eq!(archive.entries().len(), 1);
}

#[test]
fn write_read_roundtrip_in_nested_directory() {
    let name = "nested-archive";
    cleanup_tree(name);

    let path = temp_path_nested(name, &["var", "archives", "deep", "bundle.img"]);
    let mut archive = IMGArchive::default();
    archive.add_file(b"nested payload", "data.txt").unwrap();
    archive.write(&path).unwrap();

    let mut loaded = IMGArchive::from_path(&path).unwrap();
    assert_padded(&loaded.read_file("data.txt").unwrap(), b"nested payload");

    cleanup_tree(name);
}

#[test]
fn read_file_writes_to_nested_output_directory() {
    let name = "nested-extract";
    cleanup_tree(name);

    let archive_path = temp_path_nested(name, &["in", "store.img"]);
    let output_path = temp_path_nested(name, &["out", "extracted", "data.txt"]);

    let mut archive = IMGArchive::default();
    archive.add_file(b"extract me", "data.txt").unwrap();
    archive.write(&archive_path).unwrap();

    let mut loaded = IMGArchive::from_path(&archive_path).unwrap();
    let content = loaded.read_file("data.txt").unwrap();
    fs::write(&output_path, content).unwrap();
    assert_padded(&fs::read(&output_path).unwrap(), b"extract me");

    cleanup_tree(name);
}

#[test]
fn from_path_accepts_deeply_nested_source_path() {
    let name = "nested-source";
    cleanup_tree(name);

    let archive_path = temp_path_nested(name, &["a", "b", "c", "archive.img"]);
    let mut archive = IMGArchive::default();
    archive.add_file(b"hello", "file.txt").unwrap();
    archive.write(&archive_path).unwrap();

    let loaded = IMGArchive::from_path(&archive_path).unwrap();
    assert_eq!(loaded.entries().len(), 1);
    assert!(!loaded.payload_loaded());

    cleanup_tree(name);
}
