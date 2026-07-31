# IMG V2 Format Specification

An IMG V2 file is a sector-aligned binary container for bundling named files. It has three regions:

```
┌──────────────┬─────────────────────────┬─────────────────────────┐
│   Header     │   Directory table       │   File payloads         │
│   (8 bytes)  │   (32 bytes × count)    │   (2048-byte sectors)   │
└──────────────┴─────────────────────────┴─────────────────────────┘
```

## Header (8 bytes)

| Offset | Size | Field     | Description                          |
| ------ | ---- | --------- | ------------------------------------ |
| 0x00   | 4    | signature | ASCII `VER2`                         |
| 0x04   | 4    | count     | Number of directory entries (u32 LE) |

## Directory entry (32 bytes each)

Written contiguously immediately after the header. Payload bytes are **not** inline; they follow the entire directory table.

| Offset | Size | Field   | Description                               |
| ------ | ---- | ------- | ----------------------------------------- |
| 0x00   | 4    | offset  | Byte offset of this entry's payload[1]    |
| 0x04   | 2    | sectors | Payload length in 2048-byte sectors       |
| 0x06   | 2    | size    | Reserved (always 0)                       |
| 0x08   | 24   | name    | Null-padded ASCII filename (max 23 chars) |

[1] relative to the start of the payload region

## Payloads

Each file's raw bytes are zero-padded to the next 2048-byte sector boundary. Payloads are stored back-to-back after the directory table, in the same order as the directory entries (sorted by name on write).
