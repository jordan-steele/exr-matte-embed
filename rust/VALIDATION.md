# Rust desktop and CLI validation

Validated October 2, 2026 on macOS 15.5, Apple M2 Ultra, Rust 1.97.1.

## Native application

The packaged `EXR Matte Embed.app` ran against the complete Hermes delivery folder: **11 sequences, 1,192 frames, zero frame errors**. All outputs used PIZ and a separate destination under the repository’s ignored `target/` folder. Source deliveries were only read. The app used four persistent Rust frame workers, each with six codec threads.

The independent OpenEXR 3.3.2 checker inspected **every output header** for channel names, PIZ compression, and exact preservation of original non-channel/non-compression attributes. It then decoded the first, middle, and last frame of every sequence (**33 frames**) and compared native original RGB samples and added HALF matte samples byte for byte. Every check passed. See [the Hermes validation record](validation/2026-10-02-hermes.json). Generated Hermes outputs were removed after verification to reclaim 30 GiB; reports and screenshots remain.

Generated 3840 × 2160 RGBA inputs from the earlier benchmark were scanned and processed with **two mattes per frame** through both the desktop app and production CLI. OpenEXR independently checked all **48 outputs**: exact RGBA including varying alpha, both HALF matte channels, source channel descriptions, and original attributes. A final run after the UI polish and report updates repeated those checks on another 48 outputs with `DI_Matte` and `DI_Matte.hero`. See [the final RGBA validation record](validation/2026-10-02-rgba-final.json). These are generated alpha fixtures, not original RGBA delivery files.

Native UI interaction checked the sun/moon appearance switch to light mode and the sun/moon icons in both themes, sequence filtering, selected-sequence inspection, and checkbox changes to the selected frame count. The Rust app also imported the existing Python preference format. Screenshot capture requests use the actual egui/Glow framebuffer; no mockups or image editing are involved.

The final screenshots show grouped processing controls, compact scan results, channel mapping, and icon-only appearance controls:

- [Dark appearance](../images/rust-ready.png)
- [Light appearance](../images/rust-light.png)
- [Processing RGBA](../images/rust-processing.png)
- [Completed RGBA batch](../images/rust-complete.png)

The complete Hermes run preceded the final layout polish; the final UI was rechecked with the same read-only Hermes scan and complete RGBA processing. UI changes do not change the EXR embedding algorithm.

## Automated and build checks

- `cargo test --locked --all-targets`: eleven integration tests passed.
- `cargo clippy --locked --all-targets --all-features -- -D warnings`: passed.
- `cargo fmt --all --check`: passed.
- Release GUI and CLI build: passed.
- CLI build with `--no-default-features`: passed.
- macOS app bundle Info.plist validation and ad-hoc signature verification: passed.

Workflow tests cover semantic matte naming, numeric pairing across filename prefixes/padding and uppercase extensions, Unicode filename prefixes, duplicate frames, channel-name collisions, existing-output protection, early missing-R/window validation, persistent worker output, cancellation before and during work, a corrupt later frame blocking replacement, incomplete replacement, and retained originals when Trash is unavailable. Successful replacement uses an isolated simulated Trash; failure tests retain originals in isolated fixtures. Production source folders were never replaced during validation.

The retained Python benchmark baseline is extracted from `python-1.1.0` rather than the working tree. This keeps the benchmark reproducible after removing the old runtime sources from this branch. A post-port smoke run processed three Centaur frames with each engine and independently validated all six outputs after the cleanup; that small run is a functional check, not another performance study. See [BENCHMARK.md](BENCHMARK.md) for the full performance study and measurement limits.

## Remaining platform and delivery checks

The macOS bundle is a locally built beta with an ad-hoc signature. Public distribution signing/notarization is separate release work. Windows, Linux, and Intel macOS runtime tests have not run in this local session; Jenkins artifact builds are configured for the existing Intel, Apple Silicon, and Windows agents, which need Rust installed. The receiving post house’s own application still needs an import check. PIZ remains the validated delivery default; DWA decoder differences are documented in [README.md](README.md#independent-validation).
