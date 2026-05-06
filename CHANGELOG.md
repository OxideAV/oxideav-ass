# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.6](https://github.com/OxideAV/oxideav-ass/compare/v0.0.5...v0.0.6) - 2026-05-06

### Other

- drop stale REGISTRARS / with_all_features intra-doc links
- drop dead `linkme` dep
- auto-register via oxideav_core::register! macro (linkme distributed slice)
- unify entry point on register(&mut RuntimeContext) ([#502](https://github.com/OxideAV/oxideav-ass/pull/502))

## [0.0.5](https://github.com/OxideAV/oxideav-ass/compare/v0.0.4...v0.0.5) - 2026-05-04

### Other

- AnimatedRenderedDecoder + \clip(drawing) + \frx/\fry/\org
- typed extraction + time-evaluation of animated override tags

### Added

- `render` module (default-on `render` cargo feature): new
  `AnimatedRenderedDecoder` wraps an inner ASS subtitle decoder and
  emits rasterised RGBA `Frame::Video`s sampled at a caller-controlled
  cue-relative time (`set_offset_ms`). Each cue is fed through
  `extract_cue_animation` + `evaluate_at(t)`, the resulting state
  drives the affine transform / opacity / clip on a `VectorFrame`
  populated with shaped glyphs from `oxideav-scribe`, and the whole
  scene is rasterised via `oxideav-raster`. Available behind the
  `render` feature so parser-only consumers can opt out via
  `default-features = false`.
- `drawing` module: ASS drawing-mode mini-language parser (`m`, `n`,
  `l`, `b`, `s`, `p`, `c` commands + implicit-continuation handling
  + `\p<scale>` exponent). New `parse_drawing()` returns an
  `oxideav_core::Path`. Used by `\clip(drawing)` to feed the renderer's
  clip stack with a vector mask; also reusable by callers that want to
  rasterise `\p` drawing blocks directly.
- 3D rotations and explicit pivot: `\frx`/`\fry` (X/Y axis rotations
  in degrees) and `\org(x,y)` (pivot for `\frz`/`\frx`/`\fry`). The
  3D rotations are projected to a 2D affine via the orthographic
  small-angle approximation (`cos(α)`-shrink along the rotation axis)
  for renderers that don't ship a perspective camera. New
  `AnimatedTag::Frx`/`Fry`/`Org` variants and `RenderState`
  `rotate_x_radians`/`rotate_y_radians`/`pivot` fields.
- `animate` module: typed extraction + time-evaluation of ASS *animated*
  override tags. New API: `AnimatedTag` enum, `CueAnimation`,
  `RenderState`, `ClipRect`, `extract_cue_animation()`,
  `parse_overrides()`. Tags handled: `\fad`, `\fade`, `\move`, `\frz`,
  `\frx`, `\fry`, `\org`, `\blur`, `\fscx`, `\fscy`, `\clip(rect)`,
  `\clip(drawing)`, `\c` / `\1c`, `\fs`, and `\t(...)` interpolation
  wrapping any of the above. The textual round-trip path is unchanged
  — animated tags are still preserved as `Segment::Raw` so encode-side
  output stays bit-faithful.
- `cue_to_bytes_pub()`: public alias for the crate-private cue
  serialiser so external code can build packets directly without
  going through the demuxer.

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
