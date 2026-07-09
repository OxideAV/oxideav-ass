# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- animate: hostile-input hardening on the override evaluator — a
  `\t(\t(\t(…` chain deeper than 8 levels no longer recurses
  unboundedly (stack exhaustion on ~100k nested openers); non-finite
  wire numbers (`nan`, `inf`, exponent overflow like `1e999`) are
  rejected at parse time instead of poisoning every interpolated
  field; a NaN / negative `accel` on a caller-constructed `\t` is
  sanitised so the interpolation factor stays finite inside `[0, 1]`
  and the pre-state still holds before `t1`
- animate: `\clip` / `\iclip` inside `\t(...)` are no longer dropped.
  The rectangle forms interpolate per-corner on the accelerated ramp
  (the override-tag reference lists both as animatable and notes only
  the rectangle versions animate); the vector-drawing forms and `\org`
  snap to the post-state at `t > t1` like the other non-animatable
  tags (`\q` / `\an` / `\fn`)

### Added

- drawing parser: the `m` command now auto-closes an open shape before
  moving (per the drawing-command spec), while `n` still moves without
  closing — so two adjacent `m`-separated subpaths fill independently
  with the correct contour closure
- drawing parser: the `s` / `p` / `c` uniform cubic B-spline is now
  converted through the proper B-spline → Bézier basis (cursor + every
  `s`/`p` point form the control polygon) instead of the previous
  chain-of-cubics approximation, so spline outlines match the spec
  curve; `c` closes the spline
- per-event style resolution on the structured model:
  `AssScript::style_by_name` (case-sensitive lookup) and
  `resolved_style_for(event) -> ResolvedStyle`, applying the spec's
  `*Default`-fallback rule (empty / `Default` / unknown style name) and
  the per-event margin-override chain (an all-zeroes `MarginL`/`R`/`V`
  keeps the style margin; a non-zero value supersedes it); a synthetic
  `Default` style backstops a script with no `Default` row
- SSA↔ASS dialect conversion on the structured model:
  `AssScript::to_ass()` / `to_ssa()` / `to_dialect(Dialect)` rewrite the
  `Format:` column set, the `[V4+ Styles]` vs `[V4 Styles]` header, the
  `Alignment` numbering scheme (numpad ↔ `+4`/`+8` bit scheme), the
  event leading column (`Layer` integer ↔ `Marked=N`), and the
  `ScriptType` header — field-preserving, so a round-trip back to the
  originating dialect restores dialect-specific columns
- `collision` module: typed resolver for the `[Script Info]`
  `Collisions` reposition policy. `resolve_layout(boxes, geometry,
  policy)` places overlapping bottom-anchored lines vertically per the
  `Normal` (stack up from the bottom margin, reuse a freed slot) and
  `Reverse` (latest line at the bottom, earlier lines pushed up to read
  top-down) policies, clamping at the top margin rather than drawing
  off-canvas. `CollisionBox::from_cue` + `resolve_cue_layout` bridge the
  resolver directly to a slice of shared-IR `SubtitleCue`s
- complete the structured-model `StyleDef` typed-accessor set:
  `alignment_typed` (numpad / legacy-SSA-bit), `transform_typed`
  (ScaleX / ScaleY / Spacing / Angle), `margins_typed`,
  `primary` / `secondary` / `outline` / `back_colour_typed`,
  `bold` / `italic` / `underline` / `strikeout_typed`, and
  `fontsize` / `outline` / `shadow_typed` — each accessor stays total,
  collapsing a malformed column to its documented default
- renderer honours the SSA `\q` wrap mode: the `AnimatedRenderedDecoder`
  now resolves the effective `WrapStyle` per line (per-line `\q<n>`
  override over the new `default_wrap_style` document default) and wraps
  accordingly — mode `2` (no-wrap) never auto-breaks, mode `1`
  (end-of-line) greedy-fills, and modes `0`/`3` (smart) balance the
  visual rows so they come out as even in width as the word boundaries
  allow, biased top-wider (`0`) or bottom-wider (`3`) on a tie

### Fixed

- preserve UTF-8 dialogue text: the segmenter no longer casts raw bytes
  to `char`, so multi-byte glyphs (CJK / accented Latin / emoji) survive
  instead of being split into Latin-1 mojibake

### Other

- typed accessor for the per-style Alignment column (ASS numpad + legacy
  SSA `+4`/`+8` schemes), surfacing the vertical row the base parser drops
- bake \i synthetic-italic oblique slant into rasterised RGBA glyphs
- typed document-level `[Script Info]` accessors (`script_info` module):
  `WrapStyle` (0..3, matching `\q`), `Collisions` (Normal/Reverse),
  `PlayResX` / `PlayResY` / `PlayDepth`, and `Timer` (percentage →
  multiplier), surfaced on `ScriptInfo::{wrap_style, collisions,
  play_res_x, play_res_y, play_depth, timer}`

## [0.0.9](https://github.com/OxideAV/oxideav-ass/compare/v0.0.8...v0.0.9) - 2026-06-14

### Other

- typed accessor for the per-style Encoding column (Field 18)
- typed accessors for ScaleX/ScaleY/Spacing/Angle style columns
- bake \u underline / \s strikeout decoration bars into rasterised RGBA
- rasterise \p drawing-mode blocks as filled vector shapes
- bake \bord / \xbord / \ybord border ring into rasterised RGBA
- typed BorderStyle style-column accessor (1=outline+shadow, 3=opaque box)
- typed accessor for the per-event Dialogue Name column
- typed accessor for the Dialogue Layer column
- typed accessor for per-event MarginL/R/V columns
- drop release-plz.toml — use release-plz defaults across the workspace
- typed accessor for the Dialogue `Effect:` column
- bake \shad / \xshad / \yshad drop-shadow into rasterised RGBA
- typed [Fonts] / [Graphics] accessor with SSA-Appendix-B decode
- bake \fsp letter-spacing into the per-glyph X translation
- rephrase "matches the Aegisub spec note" → neutral spec citation
- bake \iclip(rect) and \iclip(drawing) into the rasterised clip
- bake \be iterative box-blur into the rasterised RGBA post-step
- bake \blur Gaussian edge-blur into the rasterised RGBA post-step
- bake the typed \1a primary-fill alpha into the rasterised fill
- bake the typed \an numpad alignment into the renderer
- bake \fax / \fay shear into the per-cue affine

### Added

- Typed accessor for the per-style `Encoding` column (Field 18) of a
  `[V4+ Styles]` / `[V4 Styles]` `Style:` line, which the base `parse`
  reads past (the shared `SubtitleStyle` IR has no slot for the per-style
  font character set). `parse_encoding_field` resolves the column into a
  `StyleEncoding` carrying the Windows charset numeric ID
  (`charset: u8`), with `as_code()`, `is_ansi()`, and a `charset_name()`
  that names the documented common slots (`0` ANSI / `1` Default / `2`
  Symbol / `128` Shift-JIS / `134` GB2312 / `136` BIG5 / `162` Turkish /
  `163` Vietnamese / `177` Hebrew / `178` Arabic). The style-level
  baseline for the per-segment `\fe<id>` override; the override wins when
  present. The parser is total — empty / whitespace / non-numeric /
  out-of-`0..=255`-range columns collapse to ANSI (`0`), the spec's
  "usually 0 for English (Western, ANSI)" default. 16 unit tests.
- Typed accessors for the per-style `ScaleX` / `ScaleY` / `Spacing` /
  `Angle` geometry columns of a `[V4+ Styles]` `Style:` line, which the
  base `parse` reads past (the shared `SubtitleStyle` IR has no slot for
  them). `parse_scale_field`, `parse_spacing_field`, and
  `parse_angle_field` resolve a single column to an `f64`;
  `parse_style_transform` lifts all four at once into a `StyleTransform`
  struct (`scale_x` / `scale_y` / `spacing` / `angle`, plus an
  `is_identity()` helper and an identity `Default`). These are the
  style-level baselines for the `\fscx` / `\fscy` / `\fsp` / `\frz`
  override tags already surfaced through the `animate` module. The
  parsers are total — empty, whitespace, non-numeric, and non-finite
  columns fall back per-axis to the neutral value (`100` scale, `0`
  spacing, `0` angle); every resolved field is finite. Fractional,
  signed, leading-`+`, and leading-zero magnitudes are accepted.
- `AnimatedRenderedDecoder` now bakes the `\u` underline and `\s`
  strikeout text decorations into the rasterised RGBA output. Both are
  drawn as a filled horizontal bar spanning each visual line's shaped
  width in the primary fill colour (decorations inherit the text
  colour). The spec defines only the on/off toggle, so the bar geometry
  is derived from the font metrics already on the face: thickness
  `max(1px, size / 18)`, the underline `descent * 0.5` below the
  baseline, and the strikeout centred `ascent * 0.3` above the baseline
  (through the x-height band). The bars ride the same animation group as
  the glyphs, so the `\fad` / `\frz` / `\clip` envelope and the per-cue
  transform compose over them as over text, and an active drop-shadow
  casts a congruent shadow copy. A `None` override (no `\u` / `\s`)
  resolves to "off"; the style's `Underline` / `StrikeOut` columns are
  not yet plumbed through to the renderer.
- `AnimatedRenderedDecoder` now rasterises `\p` drawing-mode blocks as
  filled vector shapes instead of treating them as glyph text. When a
  cue's resolved `RenderState::drawing_scale` is `Some(N)` with
  `N >= 1`, the renderer feeds the cue's text run through
  `parse_drawing` (with the `\p<N>` `2^(N-1)` scale exponent),
  auto-closes each subpath the way an ASS fill does — the override-tag
  reference says "when you close the line formed, it fills it with the
  primary color", and a new `m` / end-of-run implicitly closes the
  prior shape — then paints three congruent copies under the same
  animation `Group` the glyph path uses: a `\4c` / `\shad` drop
  shadow (drawn first), a `\3c` / `\bord` outline ring (filled **and**
  stroked at twice the width so a translucent interior shows the ring
  colour), and the `\1c` / `\1a` primary fill on top, exactly per the
  reference's "drawing commands use the primary color for fill and
  outline color for borders. They also display shadow." The drawing is
  anchored at the `\move` / `\pos` point (or the cue's static
  `\pos(x,y)`, falling back to the alignment-derived margin anchor for
  a bare `{\p1}m …`), and the `\pbo<y>` baseline offset is baked into
  the path's Y coordinates (positive = down). The `\fad` / `\fade`
  opacity, `\frz` / `\frx` / `\fry` rotation, `\fscx` / `\fscy` scale,
  `\fax` / `\fay` shear, `\clip` / `\iclip` precedence chain, and
  `\blur` / `\be` edge-softening post-steps all compose over a drawing
  identically to glyph text. New `close_subpaths` / `translate_path`
  helpers back the path assembly; 7 integration tests cover a filled
  square, solid-fill density, the `\pbo` Y-shift, the `\p2` half-scale
  rule, a `\p0`-disabled cue staying on the glyph path, `\clip`
  masking the shape, and a `\bord` ring widening the bounding box.
- `AnimatedRenderedDecoder` now bakes the typed `\bord<width>` /
  `\xbord<width>` / `\ybord<width>` border into the rasterised RGBA
  output. Per the override-tag reference, `\bord<size>` "changes the
  width of the border around the text" (decimal widths allowed, never
  negative; `0` disables the border entirely), and the per-axis
  `\xbord` / `\ybord` forms exist "for correcting the border size for
  anamorphic rendering". For every glyph the renderer pushes a border
  node *under* the primary fill (and after the shadow node): the full
  glyph silhouette filled **and** stroked in the `\3c` border colour,
  with the stroke centred on the glyph edge at twice the border width
  so the visible ring extends exactly `width` pixels outward once the
  fill covers the inner half. Round caps + joins keep the ring width
  uniform at sharp corners (a miter join would spike). The ring
  colour defaults to opaque black when `\3c` is absent (the same
  fallback the shadow pass uses for `\4c`), and the ring alpha
  follows the `\Xa` wire convention via `\3a` (`0` = opaque, `255` =
  transparent, mapped as `255 - ass_a`). An unequal `\xbord` /
  `\ybord` pair is reduced to an isotropic ring at the larger width —
  a stroked outline has a single width, and the spec's anamorphic-
  correction use keeps real pairs close. When both `\bord` and
  `\shad` are active the shadow copy carries the same stroke
  repainted in the shadow colour, so the shadow is cast by the
  *bordered* silhouette (the spec notes `\shad` "works similar to
  \bord"). Because the rasteriser interprets stroke widths in
  path-local units, the canvas-pixel width is divided by each glyph
  transform's scale factor (`sqrt(|det|)`, with a degenerate-matrix
  fallback to `1.0`) before it lands on the node. `\bord` was already
  typed + animatable (`RenderState::border` since 0.0.7); this lands
  the rasterisation. Eight new integration tests in `tests/render.rs`
  cover: `\bord0` matching the baseline bbox exactly; `\bord4`
  growing the ink bbox by ~4 px on all four edges; the `\3c` red ring
  + surviving white fill; the default-black ring + surviving white
  fill; `\3a&HFF&` muting the ring back to the baseline bbox; the
  isotropic `\xbord5\ybord0` == `\bord5` reduction; a
  `{\bord0\t(\bord6)}` ramp growing the bbox across time; and
  `\bord3\shad6` extending the shadow's max_x / max_y beyond
  `\shad6` alone. Five new unit tests pin the `sqrt(|det|)` scale
  factor (identity / uniform / anisotropic / rotation / degenerate),
  the round-cap + round-join stroke builder, and the recursive
  fill+stroke repaint.

### Fixed

- Repainted glyph copies in the animated renderer kept the shaper's
  producer `Group::cache_key`. Per the `oxideav-core` contract that
  key hashes the producer's identity tuple (glyph + size) — not the
  paint — and a downstream rasteriser is free to memoise the rendered
  bitmap under it, so two differently-painted copies of the same
  glyph (shadow vs border vs primary fill) could alias one
  memoised rendering: a `\bord` + fill pair came out entirely in the
  border colour with the white fill lost, and a fully-transparent
  copy could blank every later copy of the same glyph. Both repaint
  helpers now clear `cache_key` to `None` ("do not cache; render
  fresh every time") on every group they descend into, so each
  differently-painted copy rasterises independently. The new
  white-fill-survives assertions in the border colour tests pin the
  fix.

- Typed accessor for the `BorderStyle` column of a `[V4+ Styles]` /
  `[V4 Styles]` `Style:` definition
  (`oxideav_ass::parse_border_style_field(&str) -> BorderStyle`). The
  base `parse` entry point decodes the style columns the shared
  `SubtitleStyle` IR has a slot for (name, font, sizes, colours, the
  bold / italic / underline / strikeout flags, alignment, margins,
  outline and shadow widths) and reads past the `BorderStyle` column —
  the IR carries no field for the rendering mode. The SSA v4.x / ASS
  specification documents the column as *"BorderStyle. 1 = Outline +
  drop shadow, 3 = Opaque box."* The new `style_border` module lifts
  the column into a `BorderStyle` enum: `OutlineDropShadow` (the
  literal `1`, and the spec's dominant mode where the text is drawn
  with an outline and a drop shadow whose widths come from the
  neighbouring `Outline` / `Shadow` columns) versus `OpaqueBox` (the
  literal `3`, where the subtitle text sits on a filled rectangle in
  the outline colour so the `Outline` / `Shadow` widths no longer
  describe an outline + drop shadow). The enum is `Copy + Eq + Default`
  (defaulting to `OutlineDropShadow`). Helper accessors round out the
  surface: `as_code(self) -> u8` round-trips the raw spec integer (`1`
  or `3`) back into the column, and `is_opaque_box(self) -> bool` lets
  a renderer branch between the outline + drop-shadow path and the
  box-backdrop path. The parser is total — empty, whitespace-only,
  non-numeric, out-of-range, and any integer other than the two
  spec-defined values all collapse to `OutlineDropShadow`, mirroring
  how the SSA reference treats an unrecognised value; a leading `+` on
  the magnitude and leading-zero decimal padding are tolerated. 15
  unit tests cover the two spec values, empty / whitespace columns,
  surrounding-whitespace trimming, the leading-`+` and leading-zero
  forms, the SSA-era `0` and the unused `2` / `4` collapsing to the
  default mode, non-numeric and overflow rejection, both accessors on
  both variants, the `Default` trait impl, `Copy + Eq` ergonomics, and
  an invariant check that `as_code` only ever emits a valid spec
  integer.

- Typed accessor for the per-event `Layer` column on `Dialogue:` event
  lines (`oxideav_ass::parse_layer_field(&str) -> LayerOverride`). The
  base `parse` entry point reads the dialogue `Format:` row, splits
  each event line on commas, and drops the `Layer` column on the floor
  because the shared `SubtitleCue` IR has no slot for the per-event
  render-order integer. The round-trip writer fills the column with a
  literal `0`, which is fine for the dominant case but loses any
  per-line render-order the original script requested. The SSA v4.x
  specification documents the column as *"Layer (any integer).
  Subtitles having different layer numbers will be ignored during the
  collision detection. Higher numbered layers will be drawn over the
  lower numbered."* — two distinct renderer behaviours hang off the
  single integer: collision-detection grouping (lines that share a
  `Layer` collide; lines with different `Layer`s do not) and paint
  order (higher `Layer`s paint on top of lower `Layer`s). The new
  `dialogue_layer` module captures the column as a `LayerOverride`
  enum: `Default` (empty column / whitespace / the literal `0` in any
  sign form `0` / `+0` / `-0` — equivalent to "no per-event override;
  the base layer is `0`") versus `Layer(i32)` (an explicit non-zero
  signed integer). The signed `i32` carrier preserves the spec's "any
  integer" wording so negative layers (legal and appearing in
  hand-authored scripts as a deliberate "draw behind everything else"
  choice) round-trip exactly. The variant is `Copy + Eq` so it flows
  freely through structs and matches. Helper accessors round out the
  surface: `as_layer(self) -> Option<i32>` for the
  `event.layer.or(override.as_layer())` chain, and `resolve(self) ->
  i32` (Default → 0, Layer(n) → n) for the dominant render-loop path
  where a comparison against other cues' resolved layers drives both
  collision grouping (`==`) and paint order (ascending `Ord`).
  Malformed columns (non-numeric content, bare `+` / `-`, `i32`
  overflow) collapse to `Default` so the parser stays total — the
  renderer transparently uses the base layer `0`, mirroring how the
  SSA reference treats an unset event-layer column. 20 unit tests
  cover empty, whitespace-only, the literal `0` in every sign form,
  explicit positive / negative / leading-`+` values, leading-zero
  magnitude padding (parsed as decimal, not octal), surrounding
  whitespace tolerance, non-numeric rejection (`abc`, `1.5`, `0xFF`,
  `1e3`, `5px`), `i32::MIN` and `i32::MAX` boundary round-trip,
  overflow rejection on both signs, bare-sign rejection, the
  `as_layer` accessor on both variants, the `resolve` accessor on
  both variants, the `Default` trait impl, `Copy + Eq` ergonomics, and
  the spec's two rendering ergonomics (`==` collision grouping +
  ascending `Ord` paint order through a four-element `sort` sample).

- Typed accessor for the per-event `MarginL` / `MarginR` / `MarginV`
  columns on `Dialogue:` event lines
  (`oxideav_ass::parse_margin_field(&str) -> MarginOverride`). The
  base `parse` entry point reads the dialogue `Format:` row, splits
  each event line on commas, and drops the three margin columns on
  the floor because the shared `SubtitleCue` IR has no slot for
  per-event margin overrides. The round-trip writer fills each
  column with a literal `0`, which is fine for the dominant case but
  loses any per-line override the original script requested. The SSA
  v4.x specification defines each column the same way: a "4-figure"
  pixel pad with the carve-out *"All zeroes means the default
  margins defined by the style are used"*. The new `dialogue_margin`
  module captures that two-state semantic as a `MarginOverride`
  enum: `Default` (empty column / whitespace / the `0` shorthand
  in any padded form `0`/`00`/`000`/`0000`) versus `Pixels(u32)`
  (an explicit, non-zero pixel count). The variant is `Copy + Eq`
  so it round-trips freely through structs and matches. Helper
  accessors round out the surface: `as_pixels(self) -> Option<u32>`
  for the `style.margin.or(override.as_pixels())` chain, and
  `resolve_with_style(self, style_margin: u32) -> u32` for the
  one-step "give me the final pixel value" path the renderer
  needs. Malformed columns (negative integers, sign prefixes,
  non-numeric content, `u32` overflow) collapse to `Default` so the
  parser stays total — the renderer transparently picks up the
  style's matching margin, mirroring how the SSA reference treats
  the all-zero shorthand. The same function handles all three axes
  (the grammar is identical); callers select the axis at the call
  site by zipping `Format:` field names against split columns. 15
  unit tests cover empty, whitespace-only, every padded zero form,
  explicit pixel values with and without leading zero padding,
  surrounding whitespace tolerance, negative-sign rejection,
  `+`-sign rejection, alpha / hex / scientific / decimal-point
  rejection, `u32` overflow, the `as_pixels` accessor on both
  variants, the `resolve_with_style` fallback chain, the `Default`
  trait impl, and `Copy + Eq` ergonomics.

- Typed accessor for the `Effect:` column on `Dialogue:` event lines
  (`oxideav_ass::parse_effect_field(&str) -> EventEffect`). The base
  `parse` entry point reads the dialogue `Format:` row, splits each
  event line on commas, and drops the `Effect` field because the
  shared `SubtitleCue` IR has no slot for it — fine for the dominant
  empty-column case but it loses any *transition effect* the script
  asked for. The SSA v4.x specification documents four such effects
  in the column with a small grammar: a case-sensitive keyword
  followed by semicolon-separated parameters. The new
  `event_effect` module models all four as a typed enum: `Karaoke`
  (the obsolete per-word highlight from the SSA-v4 era, replaced by
  the `\k` family of override tags), `Scroll up;y1;y2;delay
  [;fadeawayheight]` and its `Scroll down;…` sibling (the rendered
  line scrolls upward or downward inside a vertical region bounded
  by `y1` and `y2`; both zero means "scroll the full height of the
  screen" per the spec), and `Banner;delay[;lefttoright;
  fadeawaywidth]` (the line is forced to a single visual row and
  scrolled horizontally; the optional `lefttoright` flag picks the
  direction, defaulting to right-to-left per the spec). `delay`
  clamps to `0..=100`, `lefttoright` clamps to `0` / `1` and
  surfaces as a `BannerDirection` enum, and the optional
  `fadeawayheight` / `fadeawaywidth` trailing fields ride as
  `Option<u32>`. The keyword match is case-sensitive per the spec's
  "effect names are case sensitive and must appear exactly as shown"
  rule, so `karaoke` lower / `KARAOKE` upper fall to a catch-all
  `EventEffect::Other(String)` variant that captures the raw bytes
  so a consumer can re-emit them verbatim through a write loop.
  Malformed payloads (missing parameters, non-numeric values,
  negative `delay`, invalid `lefttoright`) also collapse to `Other`
  so the parser stays total — it never panics and never returns an
  error. A `scroll_region()` accessor returns a normalised `(top,
  bottom)` pair (smaller value first) for both `Scroll up` and
  `Scroll down` variants since the spec calls out that "it doesn't
  matter which value (top or bottom) comes first", and a
  `scrolls_full_height()` accessor recognises the `y1 == y2 == 0`
  shorthand. Nineteen unit tests cover the empty-column case,
  every keyword variant with its required and optional parameters,
  case-sensitivity, the `0..=100` `delay` clamp, fallback to
  `Other` on malformed input, `BannerDirection` parsing of both
  flag values plus the missing-flag default, the
  `scroll_region()` normalisation, and the `scrolls_full_height()`
  shorthand. The base `parse` continues to drop the column on the
  IR-level cue; callers walk their own `Dialogue:` lines to feed
  the new accessor, mirroring the existing `parse_attachments`
  side-channel pattern.
- `AnimatedRenderedDecoder` bakes `\shad<depth>` /
  `\xshad<depth>` / `\yshad<depth>` drop-shadow distance into the
  rasterised RGBA output. For every glyph on the line the renderer
  pushes an extra translated-and-repainted node into the inner
  `Group` *before* the primary fill node, shifted by the typed
  `RenderState::shadow` offset on each axis. The shadow colour
  comes from `\4c` (`state.shadow_color`) and falls back to opaque
  black when the override is absent; the shadow alpha follows the
  `\Xa` convention — wire `0` is opaque, `255` is transparent,
  mapped via `255 - ass_a`. Negative `\xshad` / `\yshad` values
  position the shadow above-left per the spec note; the shadow is
  disabled only when both X and Y distances are zero. The
  cue-level `\fad` / `\fade` envelope stays on the outer
  `Group::opacity` so it composes multiplicatively over both the
  shadow and primary passes (consistent with the existing primary
  fill rule). Four integration tests cover the bake: `\shad0`
  leaves the baseline bbox unchanged, `\shad5` extends max_x /
  max_y by ~5 px on each axis, `\xshad-8\yshad-4` extends min_x
  by ~8 px and min_y by ~4 px while leaving the bottom-right edge
  pinned by the primary fill, and `\4a&HFF&` mutes the shadow
  contribution so the bbox snaps back to baseline.
- Typed `[Fonts]` / `[Graphics]` attachment accessor
  (`oxideav_ass::parse_attachments` → `Vec<Attachment>`). The base
  `parse` entry point still round-trips the section bodies verbatim
  through `extradata` so a write loop keeps the printable lines
  unchanged; the new side-channel reader walks the same header,
  groups consecutive body lines under each `fontname:` (font) or
  `filename:` (graphics) marker, and reverses the SSA Appendix-B
  character encoding into the original binary payload. The decoder
  handles all three input-length residues described in the spec:
  multiples of three pack four printable characters into three
  output bytes per quartet, a one-byte tail decodes from two
  characters (12-bit packed payload), and a two-byte tail decodes
  from three characters (18-bit packed payload). Lines whose
  contents fall outside the SSA printable alphabet (`33..=126`) are
  skipped per the spec's offset-of-33 rule. Three unit tests cover
  the three-byte-aligned font path, the one-byte-tail graphics
  path, and a multi-attachment Fonts section that splits on a
  repeated `fontname:` marker and joins a multi-line body; two more
  cover the no-attachments and empty-body edge cases. The `Attachment`
  struct exposes `kind: AttachmentKind` (`Font` / `Graphics`), `name`,
  and decoded `data: Vec<u8>` — downstream consumers can now feed the
  bytes straight into `oxideav-ttf` / `oxideav-otf` / `oxideav-png` /
  etc. without re-implementing the SSA-printable transform.
- `AnimatedRenderedDecoder` now bakes the typed `\fsp<spacing>`
  letter-spacing override into the rasterised output. Per the Aegisub
  override-tag reference, `\fsp` inserts an extra gap of `spacing`
  script-resolution pixels between each pair of adjacent letters
  (negative + decimal values allowed). The renderer reads
  `RenderState::letter_spacing` at sample time, adds `index * fsp`
  to each rendered glyph's X translation on top of the shaper's
  cumulative pen position, and folds the same `(n_glyphs - 1) * fsp`
  widening into the line-width measurement that drives alignment +
  greedy word-wrap — so a positive `\fsp` cannot fit more glyphs per
  visual line than the no-override baseline. The typed extractor
  already animated `\fsp` inside `\t(...)`, so a `\t(\fsp10)` ramp
  surfaces here without any further wiring. New integration tests in
  `tests/render.rs`: `\fsp0` matches the baseline ink bbox within ±2
  px; `\fsp6` on a 7-glyph run strictly widens the X-extent by at
  least `fsp - AA` while leaving the Y-extent unchanged within ±3
  px; `\fsp-1.5` strictly narrows the X-extent (the spec's "spread
  the text more out visually" tag used in reverse); and a
  `\t(\fsp10)` ramp produces a wider ink bbox at `t = cue_end` than
  at `t = 0`. Spaces (non-rendering glyphs from the shaper's
  perspective) don't get an explicit `fsp` gap added on either side,
  because `Shaper::shape_to_paths` filters non-rendering glyphs out
  of its output; the rendered-glyph-only iteration produces one
  `fsp` gap between every pair of adjacent rendered glyphs, which
  matches the pure-letter case exactly and behaves reasonably for
  spaced runs (each cluster of contiguous rendered glyphs gets the
  full per-pair widening). Files: `src/render.rs` (renderer pipeline
  comment, `render_cue_animated` glyph placement loop, `measure_with_fsp`
  helper, `wrap_line` `fsp` arg).

- `AnimatedRenderedDecoder` now bakes the typed `\iclip(rect)` and
  `\iclip(drawing)` inverse-clip overrides into the rasterised
  output. The renderer constructs a compound clip path with an
  outer ring well past the canvas (`[-canvas, +2 * canvas]` in
  script coordinates so any reasonable animation transform leaves
  the viewport inside it) followed by the inverse subpath in
  reverse traversal direction; the rasteriser's NonZero fill rule
  then sees the area *outside* the cut-out as the keep region,
  matching the Aegisub override-tag reference's "the cue is hidden
  *inside* the rectangle / path" semantics. The reverse-traversal
  builder handles each subpath independently: `LineTo` segments
  swap endpoints, `QuadCurveTo` keeps the same control point and
  swaps endpoints, `CubicCurveTo` swaps both control points and
  endpoints (so the curve passes through the same points in the
  opposite direction), and trailing `Close` markers stay where
  they were. The clip-precedence chain is `\clip(drawing)` →
  `\clip(rect)` → `\iclip(drawing)` → `\iclip(rect)`; when both a
  positive `\clip` and an inverse `\iclip` appear on the same
  segment the positive form wins, mirroring the existing
  "last-set-wins" override model — the Aegisub spec describes each
  form independently and does not pin a co-occurrence rule.
  `RenderState::iclip_rect` and `iclip_drawing` were already
  populated by the typed extractor; this commit wires them into
  the rasterisation step. New integration tests in
  `tests/render.rs`: `\iclip(0,60,320,120)` on centre-aligned
  bottom-band text reduces ink mass below the no-override
  baseline; `\iclip(0,0,40,20)` cutting a notch in the *upper*
  canvas (well clear of the text) leaves ink mass within ±5% of
  the baseline; `\iclip(m 0 60 l 320 60 l 320 120 l 0 120 c)`
  (drawing form covering the same bottom band) also reduces ink
  mass; and `\clip(0,60,320,120)\iclip(0,60,320,120)` on the same
  segment keeps the `\clip` keep region (ink mass within ±20% of
  the `\clip`-only mass — definitely not approaching zero, which
  is what the inverse form alone would have produced). New unit
  tests in `render.rs` pin the compound-path layout (outer ring at
  indices 0..5, inner ring at 5..10, outer-ring extents at
  `(-w, -h)..=(2w, 2h)` with a `0×0`-canvas fallback to a 1-unit
  square so the rasteriser gets a non-empty outer ring); the
  reverse-traversal helper's behaviour on a triangle (start at the
  last vertex, walk back through the others, trailing `Close`
  survives); and the subpath-count preservation on a two-subpath
  input.

- `AnimatedRenderedDecoder` now bakes the typed `\be<strength>`
  iterative box-blur into the rasterised RGBA buffer as a post-step,
  running after the `\blur` Gaussian step. Per the Aegisub override-tag
  reference, `\be<N>` is "the number of times to apply the regular
  effect" — a separable 1-pixel-radius 3×3 box average; the renderer
  iterates that pass `N` times over all four RGBA channels including
  alpha, so the softened glyph silhouette falls back through the alpha
  plane for the no-`\bord` text path the renderer covers today. The
  uniform `[1, 1, 1] / 3` kernel (rather than the `[1, 2, 1] / 4`
  variant that would converge to a Gaussian) keeps `\be` distinct from
  `\blur` per the spec's "iterative vs more advanced algorithm"
  distinction — the two filters stay on independent `RenderState`
  channels (`be_strength` + `blur_sigma`) and compose multiplicatively
  in the renderer (Gaussian first, then iterative box, in a fixed
  order the spec does not pin but reads naturally as "primary edge
  softener, then mild touch-up"). The integer kernel uses a
  `(a + b + c + 1) / 3` rounded mean so constant patches are exact
  fixed points (no slow erosion of a uniform fill over repeated
  passes). New integration tests in `tests/render.rs`: `\be0` matches
  the no-override baseline ink bbox within ±2 px on each side; `\be4`
  widens the alpha bbox (softer edges leak into previously-empty
  pixels); `\blur3\be3` produces a bbox area no smaller than `\blur3`
  alone (pins the "both post-steps actually run" contract against a
  future regression where one overwrites the other's working buffer).
  New unit tests in `render.rs` pin the zero-strength no-op, the
  zero-canvas guard, the short-buffer defensive guard, the
  constant-patch fixed-point property, the single-iteration alpha
  spread across a hard edge, and the monotonic-spread property
  (`\be2` reaches further than `\be1`).

- `AnimatedRenderedDecoder` now bakes the typed `\blur<strength>`
  Gaussian edge-blur into the rasterised RGBA buffer as a post-step
  via `oxideav-image-filter`'s separable `Blur` filter. Per the
  Aegisub override-tag reference, `\blur<strength>` is a Gaussian
  edge-softening filter whose `strength` may be non-integer; the
  renderer treats that wire value as the kernel's sigma in pixels
  and picks the kernel radius as `ceil(3 * sigma)` (the standard
  3σ cutoff captures > 99.7% of the kernel mass per the normal
  distribution). The blur runs through all four RGBA channels so
  the softened alpha-edge falls back through the alpha plane,
  matching the spec's "blurs the edges of the text" behaviour for
  the no-`\bord` text path the renderer covers today. The radius
  is clamped to the canvas's shorter side so a runaway strength
  can't blow the memory budget. `\be<strength>` (the iterative
  box-blur companion) is *not* folded into the same step — both
  filters stay on independent `RenderState` channels
  (`be_strength` + `blur_sigma`) per the Aegisub spec's
  "advanced algorithm vs iterative" distinction, leaving the
  composition to the caller when `\be` matters. New
  `oxideav-image-filter = "0.1"` `render`-gated optional
  dependency; opting out via `default-features = false` continues
  to drop the renderer entirely. New integration tests in
  `tests/render.rs`: `\blur0` matches the no-override baseline ink
  bbox within ±2 px on each side; `\blur3` widens the alpha bbox
  (softer edges leak into previously-empty pixels); a
  `\t(0,1000,\blur6)` ramp grows the bbox monotonically across
  three samples (t=0 / t=500 / t=1000). New unit test in
  `render.rs` pins the radius-from-sigma rule and the zero-sigma
  no-op.

- `AnimatedRenderedDecoder` now bakes the typed `\1a` primary-fill
  alpha override into the rasterised fill colour. Per the Aegisub
  override-tag reference, `\1a&Haa&` encodes alpha on the
  `0 = opaque, 255 = transparent` wire convention — the inverse of
  the rasteriser's RGBA channel, so the renderer maps the byte via
  `final_a = 255 - ass_a`. The cue-level `\fad` / `\fade` envelope
  stays on `RenderState::alpha_mul` and is applied as the animation
  `Group`'s `opacity`; the two compose multiplicatively per the
  per-spec formula documented on `RenderState::primary_alpha`
  (`final_primary_alpha = primary_alpha.unwrap_or(style) * alpha_mul`).
  When `\1a` is not set the renderer keeps the prior behaviour
  (opaque fill when `\c` / `\1c` set a primary colour; otherwise
  fall back to `default_color`'s alpha). New integration tests in
  `tests/render.rs`: `\1a&H00&` matches the no-override baseline
  ink mass within ±5%, `\1a&H80&` roughly halves it (35..65% of
  baseline), `\1a&HFF&` produces zero ink, and
  `\1a&H80&\fad(500,500)` at `t=0` still emits no ink — pinning the
  multiplicative compose against any future "override wins" or
  "envelope wins" regression.

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
- drop external-renderer cross-reference from module docstring
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
