# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
### Fixed
- Embedded mattes keep the sample type (HALF, FLOAT or UINT) and pLinear flag of their source R channel. Python 1.x converted every channel to HALF, and the first Rust beta still converted mattes, so FLOAT mattes lost precision.
- Codec threads are budgeted from the frame workers actually started, so batches with fewer frames than workers use every core (a two-frame batch ran 28% faster).
- Preferences are saved once typing pauses instead of on every keystroke, removing a full disk flush from the UI thread.

### Changed
- Separators after `_matte` no longer reach channel names: `SHOT_matte_hero` embeds as `matte.hero` rather than Python 1.x's `matte._hero`. Folders that differ only by a separator are reported as a channel collision.
- Frames are published atomically without a per-frame disk flush (about 4% faster on 4K PIZ deliveries); replacement flushes every output before moving originals.
- Lossy codec warnings state exactly which sample types each codec changes.
- Sequences whose outputs already exist beside the source start unselected and cannot be queued until a separate destination is chosen.

### Added
- Source, channel and matte sample types in the sequence table, inspector and CLI scan listing.
- Keyboard shortcuts (open, rescan, filter), Enter-to-scan, scan cancellation, a drop overlay, row context menus, progress in the window title, and a Dock/taskbar alert when a background batch finishes.

## [2.0.0-beta.1] - 2026-10-02
### Changed
- Ported the desktop application from Python/PySide to Rust/egui and the headless CLI to the same Rust batch engine.
- Retained Python 1.1.0 on the permanent `python-1.1.0` branch.
- Default to up to four parallel frame workers with an automatically bounded codec thread budget, based on measured Centaur and Hermes performance.
- Migrated packaging and Jenkins artifact builds to Rust; beta builds do not publish releases automatically.

### Added
- Sequence selection, queue filtering, channel/destination inspection, custom output roots, light/dark appearance, cancellation, and JSON batch reports.
- Numeric frame matching across different prefixes and padding; early layout, matte R, and data-window checks.
- Atomic output publication without overwriting existing files; source backup retention if replacement cannot send originals to Trash.
- Independent OpenEXR validation, repeatable Python-versus-Rust benchmarks, and native application screenshots.

## [1.1.0] - 2025-06-12
### Added
- Added ability for arbitrary named mattes so matte layers aren't required to be matteR, matteG, etc. 
- Use name as channel name
- Added replace originals option to trash sources and remove _embedded from new version
- Add additional frame number validation to scanning
- Added cli for headless usage and for bundling in other apps

### Changed
- Scan folder for matches before processing now to confirm matching mattes. New panel lists matches.
- Use pyqtdarktheme fork for more modern looking UI

## [1.0.4] - 2025-04-07
### Fixed
- Fixed Intel macOS build

### Changed
- Migrated to Jenkins build system instead of GitHub actions

## [1.0.3] - 2025-01-09
### Fixed
- Fix for multi-processing on Windows

## [1.0.2] - 2025-01-08
### Fixed
- Use icon for application on Windows

## [1.0.1] - 2025-01-06
### Added
- Changelog

### Changed
- Changed to PySide6 instead of Tkinter to fix MacOS Intel building errors
- Does not require all 4 mattes in RGB matte mode anymore, any combination of R,G,B,A mattes will work

### Fixed
- RGB matte mode now works properly

## [1.0.0] - 2024-12-16
### Added
- Initial release candidate