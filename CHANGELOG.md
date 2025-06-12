# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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