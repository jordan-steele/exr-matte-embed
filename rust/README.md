# Rust backend and validation

The desktop app and CLI share a Rust scanner, batch scheduler, and embedding
library using `exr` 1.74.2. The single-frame tool remains available for independent
validation. See the [application README](../README.md) for the desktop workflow
and [VALIDATION.md](VALIDATION.md) for native application checks.

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

The core retains all original channel types and metadata and takes each
matte's red channel with its own sample type (HALF, FLOAT or UINT) and pLinear
flag. Python 1.x read and wrote every channel as HALF; the Rust core converts
nothing. It replaces only explicitly named matte channels and keeps other
existing mattes. Choosing lossy compression can change image samples. No color
conversion is performed.

The backend accepts one flat scanline part with no channel subsampling. It
rejects unsupported layouts before loading pixels and requires identical base
and matte data windows, including their origins. It writes to a temporary file
in the destination directory and publishes the completed file without replacing
an existing destination. Frames are not flushed to disk one by one, which kept
an `F_FULLFSYNC` per frame off the critical path on macOS; replacing originals
flushes every output of a sequence before anything moves.

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

## Python versus Rust benchmark

The benchmark runs the unchanged Python 1.1.0 multiprocessing processor and
the existing Rust `embed_file` core on identical frame pairs with PIZ output.
It extracts the Python processor from the local `python-1.1.0` archive branch
or its `origin/` tracking ref, so future edits to the working Python files
cannot alter the baseline. That archive ref must be available locally.
The Python environment needs `OpenEXR`, `numpy`, and `send2trash`. Build the
release binaries first, then use a new output directory outside the inputs:

```sh
cargo build --release
python3 scripts/benchmark_exr.py \
  "/path/to/deliveries" \
  --output-root target/benchmarks/first-run \
  --frames-per-sequence 8 \
  --repeats 5
```

The runner samples evenly spaced frames from every matched `SHOT` / `SHOT_matte`
folder pair. It warms the source file cache before the sweep, shuffles profile
order with a fixed seed, and measures worker creation, reads, embedding,
compression, and writes. Validation and cleanup happen outside the timer.
Output pixels and metadata are checked with the independent OpenEXR reference
library in the first trial of each profile. Sources are read through symlinks
and remain unchanged; generated outputs are removed between trials.

For a complete sequence and a smaller set of profiles:

```sh
python3 scripts/benchmark_exr.py \
  "/path/to/deliveries" \
  --output-root target/benchmarks/full-sequence \
  --sequence-name SHOT \
  --frames-per-sequence 100000 \
  --repeats 3 \
  --profiles python-default python-full rust-current rust-four \
  --verify-frames 9
```

An output root on another volume can include that volume's write performance.
Use a new directory each time. `--verify-frames 9` checks nine evenly spaced
outputs per profile; omit it to check all outputs. Every trial must still
produce every requested file. The runner retains its archived Python snapshot,
`jobs.tsv`, `report.json`, and
`report.md`, including a comparison of the fastest measured Python and Rust
medians. Reports under `target/` are ignored by Git.

`rust-current` processes one frame at a time using the available logical CPU
count for EXR codec threads. The other Rust profiles use the benchmark batch
driver to process several frames concurrently. The `exr` crate creates codec
pools per operation, so `RAYON_NUM_THREADS` is set per process and frame worker
counts are bounded to avoid multiplying the CPU count unintentionally.

See [BENCHMARK.md](BENCHMARK.md) for the measured results and their limits.

If the provided bases are RGB, `--add-alpha-from-matte` creates separate PIZ
fixtures with their original channels plus a varying HALF alpha copied from
matte R. This allows an additional RGBA performance check without changing any
source file. Fixture generation is excluded from timing, and reports mark
these inputs as generated. Omit the flag when benchmarking actual RGBA inputs.
