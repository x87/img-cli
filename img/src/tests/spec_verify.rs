//! On-disk layout validation for tests ([IMG_SPEC.md](../../../IMG_SPEC.md)).

use crate::header::HEADER_SIZE;
use crate::metadata::{DIRECTORY_ENTRY_SIZE, IMGDirectoryEntry};
use std::collections::BTreeMap;

/// Parses and validates archive bytes against the IMG V2 specification.
pub fn verify_on_disk_spec(bytes: &[u8], expected: &[(String, Vec<u8>)]) {
    assert!(bytes.len() >= HEADER_SIZE, "archive shorter than header");
    assert_eq!(&bytes[0..4], b"VER2", "invalid signature");

    let count =
        u32::from_le_bytes(bytes[4..HEADER_SIZE].try_into().expect("header count")) as usize;
    assert_eq!(count, expected.len(), "entry count mismatch");

    let payload_base = HEADER_SIZE + count * DIRECTORY_ENTRY_SIZE;
    let expected_by_name: BTreeMap<&str, &[u8]> = expected
        .iter()
        .map(|(name, data)| (name.as_str(), data.as_slice()))
        .collect();
    assert_eq!(expected_by_name.len(), expected.len(), "duplicate test names");

    let mut parsed_entries = Vec::with_capacity(count);
    let mut total_payload_len = 0usize;

    for index in 0..count {
        let start = HEADER_SIZE + index * DIRECTORY_ENTRY_SIZE;
        let entry_bytes = &bytes[start..start + DIRECTORY_ENTRY_SIZE];

        let offset = u32::from_le_bytes(entry_bytes[0..4].try_into().expect("entry offset"));
        let sectors = u16::from_le_bytes(entry_bytes[4..6].try_into().expect("entry sectors"));
        let size = u16::from_le_bytes(entry_bytes[6..8].try_into().expect("entry size"));
        assert_eq!(size, 0, "reserved size field must be zero on disk");

        let name_field: [u8; 24] = entry_bytes[8..32].try_into().expect("entry name");
        assert_eq!(
            name_field,
            IMGDirectoryEntry::encode_name_for_disk(
                std::str::from_utf8(
                    &name_field[..name_field
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(name_field.len())]
                )
                .expect("ascii name")
            ),
            "name must be null-padded ASCII"
        );

        let name_end = name_field.iter().position(|&b| b == 0).unwrap_or(name_field.len());
        let name = String::from_utf8_lossy(&name_field[..name_end]).into_owned();

        let original = expected_by_name
            .get(name.as_str())
            .unwrap_or_else(|| panic!("unexpected directory entry {name}"));
        let expected_sectors = original.len().div_ceil(crate::SECTOR_SIZE);
        assert_eq!(
            sectors as usize, expected_sectors,
            "sector count mismatch for {name}"
        );

        parsed_entries.push((name, offset, sectors));
        total_payload_len += sectors as usize * crate::SECTOR_SIZE;
    }

    assert_eq!(
        bytes.len(),
        payload_base + total_payload_len,
        "file length does not match header + directory + payloads"
    );

    let sorted_names: Vec<_> = parsed_entries.iter().map(|(name, _, _)| name.clone()).collect();
    let mut lex_sorted = sorted_names.clone();
    lex_sorted.sort();
    assert_eq!(
        sorted_names, lex_sorted,
        "directory entries must be sorted by name on disk"
    );

    let mut cursor = payload_base;
    for (name, offset, sectors) in &parsed_entries {
        assert_eq!(
            *offset as usize, cursor,
            "payload offset mismatch for {name}"
        );

        let padded_len = *sectors as usize * crate::SECTOR_SIZE;
        let payload = &bytes[cursor..cursor + padded_len];
        let original = expected_by_name[name.as_str()];

        assert_eq!(&payload[..original.len()], original, "payload bytes for {name}");
        assert!(
            payload[original.len()..].iter().all(|byte| *byte == 0),
            "payload padding must be zero for {name}"
        );

        cursor += padded_len;
    }
    assert_eq!(cursor, bytes.len());
}
