# IMG V2

Command-line tool and library for **IMG V2** archives.

See [IMG_SPEC.md](IMG_SPEC.md) for the format specification.

## Workspace

| Crate         | Description               |
| ------------- | ------------------------- |
| [`img`](img/) | IMG V2 archive library    |
| [`cli`](cli/) | `img` command-line binary |

## Build

Requires Rust 1.85+ (edition 2024).

```bash
cargo build --release -p cli
```

## CLI usage

```bash
# Create an empty archive
img new myarchive.img

# Add one or more files (entry name is the file basename)
img add myarchive.img file1.txt file2.bin

# List entries with computed offsets and padded sizes
img list myarchive.img

# Print entry contents to stdout
img cat myarchive.img file1.txt

# Extract entries to the current directory or -o DIR
img extract myarchive.img file1.txt file2.bin
img extract myarchive.img file1.txt -o ./out

# Remove entries by name
img remove myarchive.img file1.txt
```

### Example `list` output

```
00000028: a.txt (2048 bytes)
00000828: b.txt (2048 bytes)
```

Sizes reflect sector-padded storage (actual file length may be smaller).
