use super::*;
use crate::header::VER2_SIGNATURE;

#[test]
fn encode_name_null_pads_short_names() {
    let encoded = IMGDirectoryEntry::encode_name_for_disk("file.txt");
    let mut expected = [0u8; NAME_FIELD_SIZE];
    expected[..8].copy_from_slice(b"file.txt");
    assert_eq!(encoded, expected);
}

#[test]
fn encode_name_truncates_to_23_bytes() {
    let name = "a".repeat(30);
    let encoded = IMGDirectoryEntry::encode_name_for_disk(&name);
    let expected = b"a".repeat(MAX_NAME_LEN);
    assert_eq!(&encoded[..MAX_NAME_LEN], expected.as_slice());
    assert_eq!(encoded[MAX_NAME_LEN], 0);
}

#[test]
fn directory_entry_roundtrip_preserves_name() {
    let entry = IMGDirectoryEntry {
        offset: 72,
        sectors: 1,
        size: 0,
        name: "hello.txt".to_string(),
    };
    let bytes = wincode::serialize(&entry).unwrap();
    assert_eq!(bytes.len(), DIRECTORY_ENTRY_SIZE);
    let decoded: IMGDirectoryEntry = wincode::deserialize(&bytes).unwrap();
    assert_eq!(decoded, entry);
}

#[test]
fn directory_entry_roundtrip_truncates_long_name_on_write() {
    let long_name = "a".repeat(30);
    let entry = IMGDirectoryEntry {
        offset: 72,
        sectors: 1,
        size: 0,
        name: long_name.clone(),
    };
    let decoded: IMGDirectoryEntry =
        wincode::deserialize(&wincode::serialize(&entry).unwrap()).unwrap();
    assert_eq!(decoded.name, "a".repeat(MAX_NAME_LEN));
    assert_ne!(decoded.name, long_name);
}

#[test]
fn validate_header_rejects_invalid_signature() {
    let header = crate::IMGHeader {
        sig: *b"BAD!",
        count: 0,
    };
    let err = IMGMetadata::validate_header_for_file_len(&header, HEADER_SIZE).unwrap_err();
    assert!(err.to_string().contains("invalid archive signature"));
    assert_ne!(header.sig, VER2_SIGNATURE);
}

#[test]
fn validate_header_rejects_excessive_entry_count() {
    let header = crate::IMGHeader {
        sig: VER2_SIGNATURE,
        count: 1,
    };
    let err = IMGMetadata::validate_header_for_file_len(&header, HEADER_SIZE).unwrap_err();
    assert!(err.to_string().contains("exceeds maximum"));
}

#[test]
fn directory_read_from_bytes_rejects_truncated_table() {
    let err = IMGDirectory::read_from_bytes(&[0u8; DIRECTORY_ENTRY_SIZE / 2], 1).unwrap_err();
    assert!(err.to_string().contains("truncated"));
}

#[test]
fn max_directory_entries_matches_file_size() {
    assert_eq!(IMGMetadata::max_directory_entries(HEADER_SIZE), 0);
    assert_eq!(
        IMGMetadata::max_directory_entries(HEADER_SIZE + DIRECTORY_ENTRY_SIZE),
        1
    );
    assert_eq!(
        IMGMetadata::max_directory_entries(HEADER_SIZE + 2 * DIRECTORY_ENTRY_SIZE),
        2
    );
}
