# EXR Matte Embed CLI

The Rust CLI runs the same scanner and batch engine as the desktop app. It requires no GUI or Python runtime.

```sh
cargo build --release --locked --no-default-features --bin exr-matte-embed-cli
```

## Usage

```sh
# Preview matched sequences and resolved matte channels.
exr-matte-embed-cli "/path/to/Deliveries" --scan-only --matte-channel DI_Matte

# Write lossless PIZ outputs beside sources in *_embedded folders.
exr-matte-embed-cli "/path/to/Deliveries" --matte-channel DI_Matte --workers 4

# Keep source volumes read-only by using a separate destination.
exr-matte-embed-cli "/path/to/Deliveries" --output-root "/path/to/Embedded" --report batch.json

# Replace originals only after the entire batch embeds successfully.
exr-matte-embed-cli "/path/to/Deliveries" --replace-originals
```

| Option | Purpose |
|---|---|
| `-c, --compression` | `none`, `rle`, `zip`, `zips`, `piz` (default), `pxr24`, `b44`, `b44a`, `dwaa`, `dwab` |
| `-m, --matte-channel` | Matte prefix; default `matte` |
| `-p, --workers` | Concurrent frame workers; default up to four |
| `--processes` | Compatibility alias for `--workers` |
| `--output-root` | Destination root with one `*_embedded` folder per sequence |
| `-r, --replace-originals` | Publish sequences and move originals/mattes to Trash; incompatible with a custom destination |
| `-s, --scan-only` | Scan without writing EXRs |
| `--report` | Save scan or batch results as JSON to a new file |
| `-q, --quiet` | Suppress routine output; errors remain visible |
| `-v, --verbose` | Print paths and each completed frame |
| `--version`, `--help` | Version and usage |

Ctrl-C stops scheduling new work and waits for active frames to finish. Completed files remain available, and a stopped batch does not replace originals. Existing outputs and existing report files are never overwritten. Use a new destination for a rerun; automatic resume is not implemented.

Exit codes are `0` for success, `1` for invalid sequences or processing/replacement errors, `2` for argument syntax errors, and `130` for cancellation. Scan-only returns `1` if any sequence needs attention.

The CLI processes all discovered sequences and fails preflight if any is invalid; use the desktop queue for selecting individual sequences. Each matte folder’s frame numbers must match the base sequence exactly. Different filename prefixes and numeric padding are accepted. Multiple nested source folders with the same name require separate batches when using a custom destination.

The supported EXR layouts, naming rules, and replacement behavior are described in [README.md](README.md). The Python 1.1.0 CLI remains on the [archive branch](https://github.com/jordan-steele/exr-matte-embed/tree/python-1.1.0).
