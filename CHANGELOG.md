# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/water-rs/svg/compare/v0.1.0...v0.3.0) - 2026-09-11

### Added

- offer an SVG's root <title> as the picture's accessible name
- [**breaking**] draw through the framework's Picture primitive
- [**breaking**] give the SVG scene contents an intrinsic size
- state the usvg text and font feature policy instead of inheriting it
- *(graphics)* render scenes on the CPU/GPU split engine where compute is missing
- *(navigation)* [**breaking**] make the navigation example a tour of native chrome

### Fixed

- *(tests)* lay the text fixtures out from a bundled font
- *(release)* verify registry-only package graph
- clear the pre-existing red on dev CI

### Other

- consume the released framework crates instead of a monorepo revision ([#8](https://github.com/water-rs/svg/pull/8))
- refresh the waterui git dependency lock after the upstream history rewrite
- update Linux package matrix and add dxc on Windows
- setup standalone crate files, CI workflows, and release-plz
- [**breaking**] ungate Scene2D from the GPU stack and drop its Vello escape hatches
- ship the licence texts in every published crate
- Format the workspace
- *(graphics)* share one scene renderer per device, and draw SVG through Scene2D
- Fix workspace CI failures
- Add cross-platform shader AOT with Shaderloom
- clean up clippy warnings across the workspace
- SubView: Send + Sync; decouple GpuView from SubView
- Lean dependency graph for embedded: gpu/widgets/gestures features
- Fix Hydrolysis example rendering and macOS acceptance
- Restore WaterUI CI gates and reactive map API
- reorganize the project
