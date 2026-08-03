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

# Add one or more files (entry name is the file basename, normalized to lowercase
img add myarchive.img file1.txt file2.bin

# Glob patterns are expanded relative to the current directory
img add myarchive.img folder/*.scm
img add myarchive.img folder/* --exclude '*.bak'

# List entries with computed offsets and padded sizes
img list myarchive.img
img list myarchive.img --json

# Print entry contents to stdout (supports glob patterns)
img cat myarchive.img file1.txt
img cat myarchive.img '*.md'

# Extract entries to the current directory or -o DIR
img extract myarchive.img file1.txt file2.bin
img extract myarchive.img '*.scm' -o ./out

# Remove entries by name (supports glob patterns)
img remove myarchive.img file1.txt
img remove myarchive.img '*.scm'
img remove myarchive.img '*.scm' --exclude 'init.scm'

# Find entries by name (exact match by default; use wildcards for glob)
img find myarchive.img player.dff
img find myarchive.img '*player*'
img find myarchive.img '*.scm' --json
```

### Example `list` output

```
00000028: a.txt (2048 bytes)
00000828: b.txt (2048 bytes)
```

Sizes reflect sector-padded storage (actual file length may be smaller).

### Example `list --json` with jq

```bash
# Entry names, one per line
img list myarchive.img --json | jq -r '.[].name'

# Entries matching a suffix
img list myarchive.img --json | jq '.[] | select(.name | endswith(".scm"))'

# Total stored bytes (including sector padding)
img list myarchive.img --json | jq '[.[].size] | add'
```
