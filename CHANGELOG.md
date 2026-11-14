# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0](https://github.com/water-rs/svg/compare/v0.4.0...v0.5.0) - 2026-11-14

### Added

- port to the render-target-neutral recording contract
- [**breaking**] port to WaterUI's final scene-resource API
- port rendering to the cherenkov engine

### Fixed

- tint currentColor in full markup without a viewbox override
- [**breaking**] report SVG markup that does not parse instead of panicking
- *(scene)* implement SceneContent::rebuild_for_engine for the 1406 contract
- *(ci)* make the MSRV job build with the declared floor ([#31](https://github.com/water-rs/svg/pull/31))

### Other

- *(deps)* point migrated Batch 2 git dependencies to main ([#56](https://github.com/water-rs/svg/pull/56))
- skip cargo-semver-checks on waterui-svg
- release from main and retire dev ([#51](https://github.com/water-rs/svg/pull/51))
- restore the Hydrolysis test fonts from the Actions cache
- *(deps)* point migrated Batch 1 git dependencies to main ([#47](https://github.com/water-rs/svg/pull/47))
- follow waterui dev's toolchain floor and Windows GPU resolution
- record SVG documents onto the Canvas and drop the scene renderer
- pass the release pull request's gate without CI
- resolve nami 0.12 from crates.io
- drop the remaining Cherenkov mentions
- install Hydrolysis test fonts before tests
- *(deps)* repin water-rs/waterui to 1e359bca
- *(scene)* keep the trailing newline rustfmt wants
- *(scene)* record through Content::record on the in-tree recorder API
- *(deps)* repin the engine and framework onto in-tree Cherenkov
- *(deps)* pin hydrolysis, hydrolysis-m3 and video-gpu at the repinned heads
- pin cherenkov to 52236561
- install SPIRV-Tools for cherenkov-gpu's shader build on non-Apple legs
- run the mounted checks unconditionally
- repin to the Cherenkov cutover, restore host-e2e deps
- repin to waterui 8fd2a851 and cherenkov 8d00976e
- repin cherenkov to its rewritten history
- repin the vello fork to the glifo canonical-location fix
- consume the 0.5.0 framework wave from crates.io ([#30](https://github.com/water-rs/svg/pull/30))

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
