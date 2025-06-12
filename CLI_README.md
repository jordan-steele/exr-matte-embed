# EXR Matte Embed - CLI Usage Guide

The CLI version of EXR Matte Embed provides all the core functionality without the GUI dependencies, resulting in a much smaller executable size.

## Usage

### Basic Usage
```bash
# Process all sequences in a folder
./exr-matte-embed-cli /path/to/sequences

# Scan only (no processing)
./exr-matte-embed-cli /path/to/sequences --scan-only
```

### Advanced Options
```bash
# Custom compression and matte channel name
./exr-matte-embed-cli /path/to/sequences --compression zip --matte-channel alpha

# Use more CPU cores for faster processing
./exr-matte-embed-cli /path/to/sequences --processes 8

# Replace original folders (move to trash)
./exr-matte-embed-cli /path/to/sequences --replace-originals

# Quiet mode (minimal output)
./exr-matte-embed-cli /path/to/sequences --quiet

# Verbose mode (detailed progress)
./exr-matte-embed-cli /path/to/sequences --verbose
```

## Command Line Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--compression` | `-c` | Compression type for output EXR files | `piz` |
| `--matte-channel` | `-m` | Name for the matte channel in output files | `matte` |
| `--processes` | `-p` | Number of parallel processes | Half of CPU cores |
| `--replace-originals` | `-r` | Replace original folders (move to trash) | False |
| `--scan-only` | `-s` | Only scan and report sequences, do not process | False |
| `--quiet` | `-q` | Minimal output (errors and final status only) | False |
| `--verbose` | `-v` | Verbose output with detailed progress | False |
| `--version` |  | Show version and exit | |
| `--help` | `-h` | Show help and exit | |

## Compression Options

Available compression types:
- `none` - No compression (largest files, fastest)
- `rle` - Run-length encoding
- `zip` - ZIP compression
- `zips` - ZIP compression (scanline-based)
- `piz` - PIZ compression (default, good balance)
- `pxr24` - PXR24 compression
- `b44` - B44 compression
- `b44a` - B44A compression
- `dwaa` - DWAA compression (smallest files)

## Examples

### Process a single sequence
```bash
./exr-matte-embed-cli /renders/shot_001 --compression zip
```

### Batch process multiple sequences with custom settings
```bash
./exr-matte-embed-cli /renders/all_shots \
  --compression dwaa \
  --matte-channel mask \
  --processes 12 \
  --replace-originals
```

### Scan and preview before processing
```bash
# First, scan to see what will be processed
./exr-matte-embed-cli /renders/shot_001 --scan-only

# If results look good, process
./exr-matte-embed-cli /renders/shot_001
```

### Integration with shell scripts
```bash
#!/bin/bash
# Process multiple shot folders
for shot in /renders/shot_*; do
    echo "Processing $shot..."
    ./exr-matte-embed-cli "$shot" --quiet
    if [ $? -eq 0 ]; then
        echo "✓ $shot completed successfully"
    else
        echo "✗ $shot failed"
    fi
done
```

## Return Codes

The CLI returns standard exit codes:
- `0` - Success
- `1` - Error (processing failed, invalid arguments, etc.)
- `130` - Interrupted by user (Ctrl+C)

## Size Comparison

The CLI version is significantly smaller than the GUI version because it excludes:
- PySide6 (Qt framework)
- GUI theming libraries
- Window management dependencies

Typical size reduction: **~50-70% smaller** than the GUI version.

## Troubleshooting

### Numpy/PyInstaller Compatibility Issue

If you get an error like:
```
TypeError: argument docstring of add_docstring should be a str
```

This is a known compatibility issue between PyInstaller and recent numpy versions. **Quick fix:**

```bash
chmod +x fix_numpy.sh
./fix_numpy.sh
# Choose option 1 (stable numpy)
```

For more details, see `NUMPY_FIX_GUIDE.md`.

### Other Issues

- **Permission denied**: Make sure the CLI executable has execute permissions: `chmod +x exr-matte-embed-cli-*`
- **Module not found**: Ensure you're running the built executable, not the Python script
- **EXR files not found**: Check that your folder structure matches the expected pattern (base folder + `_matte*` folders)

## Building the CLI Version

See the main README for build instructions, or use the provided build scripts:

### Option 1: Advanced Build Script
```bash
# Build only CLI version
./build.sh --cli-only

# Build both CLI and GUI
./build.sh
```

### Option 2: Simple Build Script (Recommended)
```bash
# Build only CLI version
./build_simple.sh cli

# Build both CLI and GUI
./build_simple.sh

# Clean build directories
./build_simple.sh clean
```

The simple build script avoids potential conflicts by cleaning the dist folder between builds.
