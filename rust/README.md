# Rust backend prototype

The migration starts with a shared Rust library using `exr` 1.74.2 and a
single-frame command-line tool. Batch scanning and the `eframe`/`egui` UI are the
next migration steps.

## Build and embed a frame

```sh
cargo build --release

target/release/exr-matte-prototype \
  --base "/path/to/deliveries/SHOT/SHOT.1000.exr" \
  --matte "matte=/path/to/deliveries/SHOT_matte/SHOT_matte.1000.exr" \
  --output "target/example-embedded.exr"
```

Repeat `--matte CHANNEL=FILE` to add channels such as `matte.hero`. PIZ is the
default. `--compression` also accepts `none`, `rle`, `zip`, `zips`, `pxr24`,
`b44`, `b44a`, `dwaa`, and `dwab`. ZIP maps explicitly to 16 scanlines and ZIPS
to one scanline.

The core retains all original channel types and metadata, takes each matte's
red channel, and stores that matte as HALF. It replaces only explicitly named
matte channels and keeps other existing mattes. Choosing lossy compression can
change image samples. No color conversion is performed.

This prototype accepts one flat scanline part with no channel subsampling. It
rejects unsupported layouts before loading pixels and requires identical base
and matte data windows, including their origins. It writes to a temporary file
in the destination directory and publishes the completed file without replacing
an existing destination.

## Independent validation

The reference checker requires the Python `OpenEXR` and `numpy` packages. It
reads inputs without modifying them and writes its outputs and `report.json`
under a new directory outside the source folder:

```sh
python3 scripts/validate_exr_backend.py \
  "/path/to/deliveries" \
  --output-root target/exr-validation/next-run
```

On October 2, 2026, validation with OpenEXR 3.3.2 established:

- The source folder contains three matched sequences, each with 353 base frames
  and 353 matte frames. All 2,118 source headers describe 3840 × 2160 HALF RGB
  scanline images with PIZ compression.
- Frames 1000, 1176, and 1352 from each sequence were embedded using Rust. The
  reference library decoded all nine outputs with byte-identical original RGB
  values and matte values. All original non-channel, non-compression header
  attributes were byte-identical, including timecode, frame rate, and windows.
- Synthetic fixtures preserved mixed HALF/FLOAT/UINT channels, alpha, custom
  metadata, and negative data-window origins. Multiple mattes were added and
  an unrelated existing `matte.keep` channel survived. PIZ, uncompressed, RLE,
  ZIP, and ZIPS outputs passed exact pixel and metadata comparisons.
- Existing output files were protected. Mismatched data windows, missing red
  channels, and attempts to replace original RGBA channels were rejected.
- A Rust-written DWAA output opened in the reference library with exact matte
  pixels and metadata. RGB differences were expected because DWAA is lossy.
- Decoding a reference-written DWAA frame with Rust produced 142 differing
  RGB samples out of 24,883,200. Maximum absolute error was 0.0009765625. This
  diagnostic is recorded explicitly; it is not a claim of bit-identical DWA
  decoders. PIZ remains the validated delivery default.

An existing `_embedded` folder in the validation dataset contained 35 files:
33 readable RGB files with no matte channel and two zero-byte files (frames 1048
and 1179). The sample checks used the original base and matte folders.

The checker writes the detailed `report.json` and generated EXRs to the selected
output directory. The receiving post-production application's own import
behavior still needs a manual check.
