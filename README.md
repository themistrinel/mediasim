# mediasim

Find duplicate and similar videos using perceptual hashing. Compares frames (and optionally audio) to detect re-encoded, resized, or slightly edited copies.

## Requirements

- Rust (stable)
- `ffmpeg` in PATH

## Install

```sh
cargo install --path .
```

## Usage

```sh
# Scan a directory
vidsim /path/to/videos

# Cross-compare two directories
vidsim /dir/a /dir/b

# Precise mode (slower, more accurate)
vidsim /path/to/videos --mode precise

# Output as JSON
vidsim /path/to/videos --json

# Move duplicates to another folder (keeps largest file)
vidsim /path/to/videos --move-to /path/to/dupes

# Delete duplicates (asks confirmation, keeps largest file)
vidsim /path/to/videos --delete

# Interactive review with mpv
vidsim /path/to/videos --review
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `-m, --mode` | `fast` | `fast` (3 frames) or `precise` (8 frames + audio) |
| `-t, --threshold` | `10` | Similarity threshold 0–64, lower = stricter |
| `-j, --jobs` | `4` | Max concurrent ffmpeg processes |
| `--min-duration` | `5.0` | Minimum video duration in seconds |
| `--min-size-mb` | `1.0` | Minimum file size in MB |
| `--json` | — | Output results as JSON |
| `--move-to <dir>` | — | Move duplicates to directory |
| `--delete` | — | Delete duplicates (with confirmation) |
| `--review` | — | Interactive review using mpv |
| `--cache <path>` | `~/.vidsim_cache.db` | SQLite cache file |

## How it works

1. Scans directories for video files
2. Extracts frames with ffmpeg and computes [dHash](http://www.hackerfactor.com/blog/index.php?/archives/529-Kind-of-Like-That.html) for each
3. In `precise` mode, also fingerprints audio
4. Groups videos whose hashes are within the threshold (Hamming distance)
5. Within each group, keeps the largest file as the original

Results are cached in a SQLite database so re-runs on unchanged files are instant.

## License

MIT
# mediasim
