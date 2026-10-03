# Python 1.1.0 versus Rust EXR benchmark

Measured on October 2, 2026. The existing Rust embedding core was faster than
the original Python processor on both supplied delivery datasets. Four
concurrent frame workers with six EXR codec threads each gave the best measured
Rust throughput on this machine.

## Environment and baseline

- Apple M2 Ultra, 24 logical CPUs, 128 GiB memory, macOS 15.5.
- Python 3.11.7, OpenEXR 3.3.2, NumPy 2.2.0, Send2Trash 1.8.3.
- Rust 1.97.1, `exr` 1.74.2, release build with thin LTO.
- Python processor: exact source from the `python-1.1.0` archive at
  `b12e2484eb3fcbaf9b9070a9acd72bc2befeeb7b`.
- Rust embedding library: unchanged from
  `2b5992bad761837e7fb32a56d3a20fb5e8b806b0`. The benchmark adds a bounded batch
  driver around `embed_file`; it does not change its image-processing code.

The Python baseline uses the application's actual multiprocessing method. Its
default is 12 processes on this machine; 24 processes were also measured. The
initial runs verified the working Python module against the archived source.
The reusable runner now extracts that archived module into its output folder,
so later migration changes cannot change the Python baseline. A separate
three-frame run verified the extracted baseline and its outputs.

## Complete sequence results

Each figure is the median of three trials. Both engines read the same frame
pairs and write PIZ outputs to the same destination filesystem within each test.

| Dataset and destination | Frames | Configuration | Median seconds | Trial range | Frames/s |
|---|---:|---|---:|---:|---:|
| Centaur inputs, workspace outputs | 353 | Python default: 12 processes | 29.778 | 26.367–33.200 | 11.85 |
| Centaur inputs, workspace outputs | 353 | Python: 24 processes | 25.232 | 24.973–26.006 | 13.99 |
| Centaur inputs, workspace outputs | 353 | Rust current prototype: 1 frame × 24 codec threads | 22.313 | 22.127–22.617 | 15.82 |
| Centaur inputs, workspace outputs | 353 | Rust batch: 4 frames × 6 codec threads | 19.289 | 17.871–20.482 | 18.30 |
| Hermes inputs and outputs | 378 | Python: 24 processes | 26.234 | 26.092–26.510 | 14.41 |
| Hermes inputs and outputs | 378 | Rust batch: 4 frames × 6 codec threads | 18.299 | 18.201–18.319 | 20.66 |

The Rust batch driver is **1.54× as fast as Python's default** on the complete
Centaur sequence. Against Python with 24 processes, it is **1.31× as fast on
Centaur** and **1.43× as fast on Hermes**, including writes to Hermes. The current
Rust prototype's one-frame scheduling also beats both Python configurations on
the complete Centaur sequence.

## Samples across sequences and RGBA coverage

Each sample sweep uses five trials and eight evenly spaced frames per sequence.
The Centaur sweep covers all three sequences; Hermes covers all eleven. Every
provided base and matte input is 3840 × 2160 HALF RGB with PIZ compression.

| Test | Frames per trial | Python: 24 processes | Rust: 4 frames × 6 codec threads | Rust speedup |
|---|---:|---:|---:|---:|
| Centaur, 3 sequences | 24 | 1.830 s | 0.956 s | 1.91× |
| Hermes, 11 sequences | 88 | 6.971 s | 3.681 s | 1.89× |
| Generated RGBA fixtures, 3 sequences | 24 | 2.138 s | 1.108 s | 1.93× |

The RGBA fixtures preserve the original Centaur RGB samples and add a varying
HALF alpha from each matte's red channel. They are separate lossless input
files generated outside the timer and verified against their source values.
These cover the extra alpha channel but are not original RGBA delivery files.
RGBA embedding produces five channels: RGBA plus `matte`.

The first Centaur sweep also compared one-thread processing and other Rust
worker arrangements:

| Configuration | Median seconds for 24 frames |
|---|---:|
| Python: 1 process | 14.357 |
| Rust: 1 frame × 1 codec thread | 13.710 |
| Python default: 12 processes | 2.331 |
| Rust current prototype: 1 frame × 24 codec threads | 1.299 |
| Rust: 12 frames × 2 codec threads | 1.000 |
| Rust: 24 frames × 1 codec thread | 1.177 |

Single-thread Rust is about 5% faster here. The larger batch advantage depends
on the implementation's parallel scheduling; it is not a general claim about
language speed. The `exr` crate creates codec pools per operation, so the frame
worker count and per-operation codec thread count must be budgeted together.

## Correctness and measurement scope

The five sweeps completed 9,464 frame operations. OpenEXR independently decoded
646 outputs: all outputs in the first trial of each sample configuration, and
nine evenly spaced outputs per configuration in each complete-sequence test.
Every checked original channel, including fixture alpha, and every added HALF
matte matched exactly. Existing channel descriptions and source metadata also
matched, except for the Python processor's intentional omission of `writer`.
Rust retains that attribute. Every trial produced every requested output.

Timing includes worker/pool creation, file reads, embedding, PIZ compression,
and writes. Rust also retains atomic output publication and `fsync`; Python's
original processor does neither. Interpreter startup, source discovery,
manifest loading, builds, cache warming, fixture generation, validation, and
cleanup are excluded. Raw reports also record subprocess elapsed time.

The source file cache was warmed before each sweep; cache eviction is not
controlled. These results measure processing with prewarmed inputs, including
the selected output filesystem's write behavior. They do not establish cold
read speed or isolate the two drives' performance. Shot content differs between
datasets. The UI is not included, and the results apply to these flat HALF/PIZ
workloads with one matte per frame.

Source deliveries were only read. Generated outputs were removed between
trials and after completion, within separate benchmark directories.

## Reproduction and recorded results

See [README.md](README.md#python-versus-rust-benchmark) for commands and options.
Use `--profiles python-default python-full rust-current rust-four` for the four
main configurations, or omit it to run the full concurrency sweep. Use
`--add-alpha-from-matte` to reproduce the generated alpha fixtures from RGB
inputs. Every run requires a new output directory outside the source root.

[2026-10-02.json](benchmarks/2026-10-02.json) records each trial, median, range,
validation result, environment, and baseline source hashes. Detailed per-run
reports and manifests are retained under `target/benchmarks/`. The Hermes
external-write report is also copied there so it remains available when the
drive is disconnected.

The measured performance gate passes. Use bounded concurrent frame processing
for the batch migration and retain this benchmark as the backend develops.
