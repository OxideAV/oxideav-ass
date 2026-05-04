# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `animate` module: typed extraction + time-evaluation of ASS *animated*
  override tags. New API: `AnimatedTag` enum, `CueAnimation`,
  `RenderState`, `ClipRect`, `extract_cue_animation()`,
  `parse_overrides()`. Tags handled: `\fad`, `\fade`, `\move`, `\frz`,
  `\blur`, `\fscx`, `\fscy`, `\clip(rect)`, `\c` / `\1c`, `\fs`, and
  `\t(...)` interpolation wrapping any of the above. The textual
  round-trip path is unchanged — animated tags are still preserved as
  `Segment::Raw` so encode-side output stays bit-faithful.

## [0.0.4](https://github.com/OxideAV/oxideav-ass/compare/v0.0.3...v0.0.4) - 2026-05-03

### Other

- bump oxideav-subtitle dep to 0.1
- replace never-match regex with semver_check = false
- migrate to centralized OxideAV/.github reusable workflows
- pin release-plz to patch-only bumps

## [0.0.3](https://github.com/OxideAV/oxideav-ass/releases/tag/v0.0.3) - 2026-04-25

### Other

- drop oxideav-codec/oxideav-container shims, import from oxideav-core
- bump oxideav-container dep to "0.1"
- bump oxideav-core / oxideav-codec dep examples to "0.1"
- migrate register() to CodecInfo builder
- bump oxideav-core + oxideav-codec deps to "0.1"
- thread &dyn CodecResolver through open()
- preserve unknown overrides + advertise decode/encode caps
- make crate standalone (pin deps, add CI + release-plz + LICENSE)
- add Decoder::reset overrides for subtitle decoders
- move repo to OxideAV/oxideav-workspace
- add publish metadata (readme/homepage/keywords/categories)
- final two collapsible_match sites (rust 1.95)
- address workspace-wide lints to unblock CI
- cargo fmt across the workspace
- 13 text formats + 3 bitmap formats + render infra; ASS→own crate
