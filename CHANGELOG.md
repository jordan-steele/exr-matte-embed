# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[Unreleased]: https://github.com/jordan-steele/exr-matte-embed/compare/v1.0.1...HEAD
[1.0.1]: https://github.com/jordan-steele/exr-matte-embed/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/jordan-steele/exr-matte-embed/releases/tag/v1.0.0