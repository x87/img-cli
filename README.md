# IMG V2 CLI

Command-line tool for creating and managing **IMG V2** archives - simple, sector-aligned binary containers for bundling named files.

## Format

An IMG V2 file has three regions:

```
┌──────────────┬─────────────────────────┬─────────────────────────┐
│   Header     │   Directory table       │   File payloads         │
│   (8 bytes)  │   (34 bytes × count)    │   (2048-byte sectors)   │
└──────────────┴─────────────────────────┴─────────────────────────┘
```

### Header (8 bytes)

| Offset | Size | Field     | Description                          |
|--------|------|-----------|--------------------------------------|
| 0x00   | 4    | signature | ASCII `VER2`                         |
| 0x04   | 4    | count     | Number of directory entries (u32 LE) |

### Directory entry (34 bytes each)

Written contiguously immediately after the header. Payload bytes are **not** inline; they follow the entire directory table.

| Offset | Size | Field   | Description                                      |
|--------|------|---------|--------------------------------------------------|
| 0x00   | 4    | offset  | Byte offset of this entry's payload in the file  |
| 0x04   | 2    | sectors | Payload length in 2048-byte sectors              |
| 0x06   | 4    | size    | Reserved (always 0)                              |
| 0x0A   | 24   | name    | Null-padded ASCII filename (max 23 chars)        |

### Payloads

Each file's raw bytes are zero-padded to the next 2048-byte sector boundary. Payloads are stored back-to-back after the directory table, in the same order as the directory entries (sorted by name on write).

## Build

Requires Rust 1.85+ (edition 2024).

```bash
cargo build --release
```

## Usage

```bash
# Create an empty archive
img new myarchive.img

# Add one or more files (entry name is the path string passed on the CLI)
img add myarchive.img file1.txt file2.bin

# List entries with computed offsets and padded sizes
img list myarchive.img

# Remove entries by name
img remove myarchive.img file1.txt
```

### Example output

```
00000200: y.scm (2048 bytes)
00002248: y.scm (2048 bytes)
00004296: z.scm (2048 bytes)
```

Sizes reflect sector-padded storage (actual file length may be smaller).
