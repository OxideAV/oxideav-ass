# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `AnimatedRenderedDecoder` now honours the typed `\an<n>` / legacy
  `\a<n>` numpad-alignment override on the per-cue
  `RenderState::alignment` field, anchoring the line-stack's
  vertical position by the numpad row in addition to the existing
  horizontal column. The 1..=9 code is decomposed into a
  horizontal `TextAlign` (column 1/2/3 = left/centre/right) and a
  `VerticalRow` (rows 1-3 / 4-6 / 7-9 = bottom / middle / top per
  the Aegisub override-tag reference's "1/2/3 = bottom-{l,c,r};
  4/5/6 = middle-{l,c,r}; 7/8/9 = top-{l,c,r}" mapping). Bottom-row
  cues keep the existing layout (last baseline = `height -
  bottom_margin_px - descent`); top-row cues anchor the *first*
  baseline at `bottom_margin_px + ascent` (using the same field as
  a symmetric top/bottom margin so the additive API stays minimal);
  middle-row cues centre the full block height (`(n_lines - 1) *
  line_h + ascent + descent`) on the canvas mid-line. When no
  numpad override is active the renderer falls back to the cue's
  `CuePosition::align` hint and keeps the bottom-row baseline (the
  existing default behaviour). New unit test in `render.rs` pins
  the numpad decomposition; new integration tests in
  `tests/render.rs` exercise the three rows (`\an2` / `\an5` /
  `\an8`) and the three columns (`\an1` / `\an2` / `\an3`)
  end-to-end against the rasterised bbox.

- `AnimatedRenderedDecoder` now bakes the `\fax` / `\fay` shear into
  the per-cue affine. The shear is applied as a pre-step pivoted on
  the cue's alignment point (independent of `\org`, per the Aegisub
  override-tag reference's "the coordinate system used for shearing
  is not affected by the rotation origin" rule); the rotation step
  then carries the distortion along, matching the spec's "shearing
  is performed after rotation, on the rotated coordinates" effect.
  A pure `\fax` widens the visible x-extent while leaving the y-
  extent unchanged; a pure `\fay` shears y by x and stretches the
  visible y-extent. `RenderState::shear` was already populated by
  the typed extractor — this commit only wires it into
  `animation_transform` so the rasterised output reflects it. New
  unit tests in `render.rs` pin the matrix layout
  (`[[1, fax], [fay, 1]]` per the Aegisub reference) and verify the
  anchor stays invariant under a pure shear even when `\org` is far
  away; new integration tests in `tests/render.rs` exercise the
  pipeline end-to-end with a TTF.

## [0.0.8](https://github.com/OxideAV/oxideav-ass/compare/v0.0.7...v0.0.8) - 2026-05-29

### Other

- typed extraction for the \p<scale> drawing-mode toggle
- typed extraction for \i / \u / \s italic/underline/strikeout toggles
- typed extraction for \pbo drawing baseline offset
- typed extraction for \fn / \fe / \b<weight> / \r[<style>]
- align r131 frx/fry tests to Linux rustfmt reflow
- pin \frx / \fry parallel test coverage

### Added

- Typed extraction for the `\p<scale>` drawing-mode toggle override
  tag from the Aegisub override-tag reference. `\p<scale>` is the
  switch between text rendering and ASS vector drawing-mode: `\p0`
  disables drawing mode (text after the override block renders as
  glyphs), `\p1` enables drawing mode with native pixel coordinates,
  and `\p<N>` for `N >= 2` enables drawing mode with sub-pixel
  coordinates scaled by `2^(N-1)` per the Aegisub spec's "interpreted
  as the scale, in `2^(value-1)` mode" rule. Surfaces as a new
  `AnimatedTag::P(u8)` variant alongside the existing animated set,
  with `RenderState::drawing_scale: Option<u8>` exposing the resolved
  value (`None` = no override — renderer assumes text mode;
  `Some(0)` = drawing mode explicitly disabled by `\p0`; `Some(N)`
  for `N >= 1` = drawing mode at scale exponent `N - 1`). The raw
  `N` argument surfaces verbatim so consumers using
  `oxideav_ass::parse_drawing(s, scale_exp)` can pass
  `scale_exp = N - 1` straight from the typed slot. `\p` is non-
  animatable per spec — a binary drawing-vs-text mode switch has no
  meaningful in-between value — so inside `\t(...)` it snaps to the
  post-state at `t > t1`, mirroring the existing `\q` / `\an` /
  `\fn` handling. The parser rejects negative arguments and values
  above the `u8` ceiling (any sensible authoring stays well under
  `255` since the scale grows exponentially); rejected values still
  survive verbatim through `Segment::Raw` so the round-trip writer
  re-emits the original bytes. Round-tripping `{\\p2\\pbo10}m 0 0`
  re-emits both tags through the passthrough block and the typed
  surface still recovers both on the second parse. Closes the
  long-standing "renderer doesn't know when drawing mode begins or
  ends" gap left after `\pbo` (r182) lifted the companion baseline
  offset into the typed surface.

- Typed extraction for the `\i<flag>` / `\u<flag>` / `\s<flag>`
  italic / underline / strikeout face-flag toggles from the Aegisub
  override-tag reference. Each surfaces as a new
  `AnimatedTag::I(bool)` / `U(bool)` / `S(bool)` variant alongside
  the existing animated set, with `RenderState::italic`,
  `RenderState::underline`, and `RenderState::strikeout` exposing
  the resolved override as `Option<bool>` (`None` = inherit the
  style's flag; `Some(true)` / `Some(false)` = explicit on / off).
  The boolean toggle reaches the animate path through two doors:
  the base parser still consumes a standalone `\i1` / `\u1` /
  `\s1` into the matching `Segment::Italic` / `Segment::Underline`
  / `Segment::Strike` wrapper for the byte-faithful text round-trip,
  and `walk_segments` now emits `AnimatedTag::I(true)` /
  `U(true)` / `S(true)` when it descends into those wrappers, so a
  cue parsed directly from the dialogue text reaches
  `RenderState::italic = Some(true)` without needing a separate
  `Segment::Raw` block; an explicit `\i0` (or any later override)
  arriving through a raw passthrough block still overrides the
  wrapper-derived toggle via the standard last-writer-wins
  `apply_tag` model. None of the three are animatable per the
  Aegisub spec — a boolean face flag has no meaningful in-between
  value — so inside `\t(...)` each snaps to the post-state value at
  `t > t1`, mirroring the existing `\b` / `\fn` / `\q` handling.
  The parser rejects anything outside the `0` / `1` toggle (the
  spec's only documented values for these tags) so the renderer
  keeps the style's flag for malformed inputs like `\i2` or
  `\sfoo`. Round-tripping a `{\\i1}it{\\u1}u{\\s1}s` line re-emits
  the original `\i1` / `\u1` / `\s1` bytes through the segment
  wrappers and the typed surface still recovers all three on the
  second parse.

- Typed extraction for the `\pbo<y>` drawing-mode baseline-offset
  override tag from the Aegisub override-tag reference (no entry in
  the Kotus / TCAX spec — `\pbo` is an Aegisub-era extension). The
  tag carries a Y-axis pixel offset applied to every coordinate
  emitted inside a `\p<scale>` drawing block: per the Aegisub
  examples, `\pbo-50` draws "50 pixels above specified" and
  `\pbo100` draws "100 pixels below". Surfaces as a new
  `AnimatedTag::Pbo(i32)` variant alongside the existing animated
  set, with `RenderState::drawing_baseline_offset: Option<i32>`
  exposing the resolved value (`None` = no offset; renderers leave
  drawing coordinates untranslated). Animatable inside `\t(...)`:
  the offset ramps linearly between the pre- and post-state values
  and is round-clamped back into `i32` at each sample, mirroring
  the integer-strength handling of `\be`. When the pre-state slot
  is `None`, the lerp uses `0` as the implicit baseline so a
  `\t(0,t2,\pbo100)` ramp climbs from `0` up to `100` instead of
  snapping to `100` immediately. `\pbo` is unknown to the base
  parser, so it survives via `Segment::Raw` and re-emits verbatim
  through the writer (including when combined with `\p1` in the
  same override block). The drawing-mode rasterisation is still
  out of scope per the existing `\p` opt-out, but consumers using
  `parse_drawing` to lift the path can translate by
  `(0, drawing_baseline_offset.unwrap_or(0))` before rasterising.

- Typed extraction for four more override tags from the Aegisub /
  Kotus tag reference: `\fn<name>` (font family), `\fe<id>` (Windows
  font-encoding / charset ID for the glyph-mapping table),
  `\b<weight>` (font weight, integer per the Aegisub spec —
  `100..900` in steps of 100, with `400` = normal and `700` = bold;
  the legacy `\b1` / `\b0` toggle maps to `Some(700)` / `Some(0)`),
  and `\r[<style>]` (style reset; bare `\r` drops back to the line's
  base style, the `\r<style>` form switches to a named definition
  from the script's `[V4+ Styles]` block). New
  `AnimatedTag::Fn(String)` / `Fe(u8)` / `B(u16)` /
  `R(Option<String>)` variants surface alongside the existing
  animated set. `RenderState` gains `font_name: Option<String>`,
  `font_encoding: Option<u8>`, `bold_weight: Option<u16>`, and
  `reset_to_style: Option<Option<String>>` (outer `Some` records that
  a reset fired; inner `Option` is the bare-vs-named target). Applying
  an `AnimatedTag::R` also wipes every other override field back to
  identity per the Aegisub spec rule "cancels all style overrides in
  effect, including animations, for all following text" — the
  `reset_to_style` slot stays set so callers can spot the reset.
  None of the four are animatable per spec (a typeface / weight /
  encoding change cannot be interpolated meaningfully); inside
  `\t(...)` they snap to the post-state at `t > t1`, mirroring the
  existing `\q` / `\an` / `\a` behaviour. The new
  `parse_one` prefix-split handles the spec's "no separator between
  `\fn` and the family name" / "no separator between `\r` and the
  style name" rule — `{\fnTimes New Roman}` and `{\rAlternate}`
  parse correctly into their typed surfaces.

### Fixed

- The base parser's `\b` handler used to silently consume any
  numeric argument as a boolean (anything non-zero became
  `Segment::Bold`), throwing away the Aegisub `\b<weight>` form's
  weight value on the round-trip. The handler now only consumes
  exact `0` / `1` parameters into `Segment::Bold`; explicit weights
  (`\b100`, `\b500`, `\b700`, `\b900`, …) fall through to the
  passthrough block so the writer keeps them verbatim and the
  `animate::extract_cue_animation` typed surface can recover them as
  `AnimatedTag::B(weight)`.


## [0.0.7](https://github.com/OxideAV/oxideav-ass/compare/v0.0.6...v0.0.7) - 2026-05-24

### Other

- typed extraction of the \k karaoke-timing family
- typed extraction for \an / \a line-alignment overrides
- drop renderer-name from \pos decimal-tolerance comment
- typed extraction for \pos static line position
- typed extraction for \fsp letter-spacing + \q wrap-style
- drop libass / Aegisub cross-reference from module docstring
- typed extraction for \2c/\3c/\4c colours + \alpha/\1a..\4a per-component alpha
- typed extraction for \bord/\xbord/\ybord, \shad/\xshad/\yshad, \be, \fax/\fay, \iclip
- preserve unknown / Fonts / Graphics section bodies on round-trip

### Added

- Test coverage for the `\frx` / `\fry` X- and Y-axis 3D-rotation
  override tags from the Aegisub override-tag reference. The two tags
  already had `AnimatedTag::Frx` / `Fry` variants, `parse_overrides()`
  wiring, and `\t(...)` interpolation through the shared `apply_t`
  machinery (alongside `\frz`); this round pins the static-extraction
  path (`{\frx45}`, `{\fry-45}` per Aegisub's "opposite direction"
  example), the combined two-axis path (`{\frx30\fry45}`), `\t`
  interpolation on each axis individually and together
  (`{\t(0,1000,\frx90\fry-90)}` swivel mid-cue at π/4 on each axis),
  and the textual round-trip (parse → `ass::write` → re-parse keeps
  both tags verbatim through `Segment::Raw` and re-extracts the same
  typed values). The X / Y / Z `RenderState` rotation fields stay
  independent — setting one does not leak into the others. No
  behaviour change; tests only.

- Typed extraction for the `\k` karaoke-timing family (`\k`, `\K`,
  `\kf`, `\ko`) from the Aegisub override-tag reference. Each marker
  surfaces as a new `AnimatedTag::Karaoke { kind, cs }` carrying the
  syllable duration in centiseconds and a `KaraokeKind` discriminant —
  `Fill` for the instant secondary→primary switch (`\k`), `Sweep` for
  the left-to-right secondary→primary wipe (`\kf`, and the identical
  uppercase `\K`), and `Outline` for the border-reveal variant (`\ko`).
  The case-sensitive `\K` vs `\k` distinction is preserved (the base
  parser lowercases tag names, so the original case is now threaded
  through to the karaoke arm). A new `CueAnimation::karaoke_spans()`
  resolves the in-order markers into cumulative `KaraokeSpan`s
  (`start_ms`/`end_ms` from the cue start, exact centisecond→ms
  conversion), and `KaraokeSpan::progress(t)` gives the `0.0..=1.0`
  highlight position for the active syllable (the left-to-right wipe
  fraction for `Sweep`; the started/not-started boundary for
  `Fill`/`Outline`). Karaoke is a timeline-level concept, so the
  evaluator treats it as a no-op on `RenderState` (the affine / colour /
  alpha transform stays at identity) — renderers walk the spans. The
  full parse path also resolves karaoke through the base parser's
  `Segment::Karaoke` markers (kind reported as the conservative `Fill`
  default, since the core marker drops the family member; the
  centisecond duration survives). Negative durations clamp to `0`; `\kt`
  is deliberately not handled per the Aegisub note that it is
  undocumented/unsupported (the textual round-trip keeps it verbatim via
  `Segment::Raw`).
- Typed extraction for the `\an<pos>` (numpad) and `\a<pos>` (legacy
  SubStation-Alpha) line-alignment override tags from the Aegisub
  reference. `\an1..\an9` map straight to a new
  `AnimatedTag::An(u8)` carrying the numpad code; `\a<pos>` is kept
  as `AnimatedTag::A(u8)` and converted to the same numpad surface
  on apply (low nibble = L/C/R, `+4` = top row, `+8` = mid row, so
  `\a6` = top-center = numpad `8`, identical to `\an8`). `RenderState`
  gains `alignment: Option<u8>` (1..=9, `None` = fall back to the
  style's `Alignment` field) — renderers can now anchor `\pos` /
  `\move` translation against the documented numpad corner instead
  of guessing from `cue.positioning.align`'s `Left`/`Center`/`Right`
  reduction. Both tags are static per spec; inside `\t(...)` they
  snap to the post-state value at `t > t1` rather than interpolating,
  mirroring `\q`. Out-of-range `\an` codes (`0`, `10+`) and
  unrecognised `\a` codes (`4`, `8`, `12+`) drop the override so the
  renderer keeps the script-style alignment. The base parser now
  emits both tags into `Segment::Raw` (in addition to populating the
  existing `cue.positioning.align`), which closes a long-standing
  round-trip gap: previously the writer dropped the vertical row of
  the numpad alignment entirely, so a parse → write cycle turned
  `\an7` into a plain bottom-row default; the tag now survives
  verbatim.
- Typed extraction for the `\pos(x, y)` static-positioning override tag
  from the Aegisub / Kotus reference. `\pos` sets the line's position in
  the script-resolution coordinate system (the alignment point is
  anchored there); it is the non-moving counterpart of `\move` and now
  surfaces a new `AnimatedTag::Pos { x, y }` variant that writes the
  same `RenderState::translate` field `\move` populates. A `\pos`-only
  cue therefore yields a usable `translate` from `evaluate_at` instead
  of `None`. `\pos` is static (not animatable per spec) so the position
  is constant across time; when both `\pos` and `\move` are present the
  later tag wins (last-writer-wins, matching the module's static-
  override model). Coordinates parse as floats so decimal values seen
  in the wild are tolerated even though the spec asks for integers;
  wrong-arity forms are dropped (the textual round-trip still keeps
  them verbatim via `Segment::Raw`).
- Typed extraction for two more override tags from the Aegisub /
  TCAX spec: `\fsp(spacing)` (additive letter-spacing in script-
  resolution pixels — may be negative or decimal, fully animatable
  per the `\t(...)` interpolation table) and `\q(mode)` (line-level
  wrap-style override `0`/`1`/`2`/`3`; static, not animatable per
  spec — out-of-range modes are dropped so the renderer keeps the
  script's `WrapStyle` header). New `AnimatedTag::Fsp(f32)` and
  `AnimatedTag::Q(u8)` variants. `RenderState` gains
  `letter_spacing: Option<f32>` and `wrap_style: Option<u8>`, both
  defaulting to `None` (= fall back to the style's `Spacing` field
  and the script header's `WrapStyle`). `\fsp` interpolates linearly
  when nested inside `\t(...)`; `\q` snaps to the post-state value at
  `t > t1` because the spec treats it as non-animatable. The textual
  round-trip path is unchanged — animated tags are still preserved
  as `Segment::Raw` so encode-side output stays bit-faithful.
- Typed extraction for the per-component colour and alpha override-tag
  families from the Aegisub tag reference: `\2c` / `\3c` / `\4c`
  (secondary / outline / shadow fill colours) and `\alpha` plus
  `\1a` / `\2a` / `\3a` / `\4a` (per-component alpha; ASS convention
  0 = opaque, 255 = transparent, with `\alpha` setting all four
  channels at once). New `AnimatedTag::Color2` / `Color3` / `Color4`
  / `Alpha` / `Alpha1` / `Alpha2` / `Alpha3` / `Alpha4` variants
  surface alongside the existing `Color1`. `RenderState` gains
  `secondary_color` / `outline_color` / `shadow_color` (RGB,
  `Option<(u8,u8,u8)>`) and `primary_alpha` / `secondary_alpha` /
  `outline_alpha` / `shadow_alpha` (`Option<u8>`); the per-component
  alphas are kept independent from `alpha_mul` (the `\fad` / `\fade`
  cue-level envelope) so renderers compose
  `final_alpha = component_alpha * alpha_mul` themselves. All eight
  new tags interpolate correctly inside `\t(...)` per the Aegisub
  spec's animatable-tag table (`\1c`..`\4c`, `\alpha`, `\1a`..`\4a`).
  The textual round-trip path is unchanged — animated tags are still
  preserved as `Segment::Raw` so encode-side output stays
  bit-faithful.
- Typed extraction for nine more override tags from the Aegisub /
  Kotus tag reference: `\bord` / `\xbord` / `\ybord` (uniform + per-
  axis border widths, with the "later `\bord` overrides earlier
  `\xbord`/`\ybord`" rule honoured), `\shad` / `\xshad` / `\yshad`
  (uniform + per-axis shadow distances; `\shad` clamps non-negative
  per spec while `\xshad`/`\yshad` may go negative for top/left
  shadows), `\fax` / `\fay` (X/Y shearing factors applied after
  rotation), and `\iclip(rect)` / `\iclip(drawing)` (inverse
  rectangular / vector clip). `\be` (iterative box-blur, integer
  strength) is now a distinct `AnimatedTag::Be` variant rather than
  being silently folded into the Gaussian `\blur` channel — both
  filters surface on `RenderState` as `be_strength: u8` and
  `blur_sigma: f32` respectively so renderers can wire them to
  separate passes. The new fields on `RenderState` (`border`,
  `shadow`, `be_strength`, `shear`, `iclip_rect`, `iclip_drawing`)
  default to "no override" so existing consumers continue to compile
  unchanged. All new tags also interpolate correctly when wrapped in
  `\t(...)`. The textual round-trip is unchanged — animated tags are
  still preserved as `Segment::Raw` so encode-side output stays
  bit-faithful.

### Fixed

- Unknown sections (e.g. `[Aegisub Project Garbage]`, `[Aegisub
  Extradata]`, `[Aegisub Style Storage]`, `[Fonts]`, `[Graphics]`)
  used to drop their body lines on parse, leaving a dangling section
  header in the writer's output and losing editor state plus embedded
  font / graphic attachments. Body lines are now preserved verbatim
  through `SubtitleTrack::extradata` so a parse → write round-trip is
  byte-faithful for these blocks. The `[Events]` body is still
  reconstructed from the typed `cues` list (unchanged behaviour).

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
