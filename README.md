# oxideav-ass

Pure-Rust **ASS / SSA** subtitle codec and container — parser and writer
for Advanced SubStation Alpha (`.ass`) and SubStation Alpha (`.ssa`)
text subtitle files. Zero C dependencies.

Part of the [oxideav](https://github.com/OxideAV/oxideav-workspace)
framework but usable standalone.

## Installation

```toml
[dependencies]
oxideav-core = "0.1"
oxideav-codec = "0.1"
oxideav-container = "0.1"
oxideav-subtitle = "0.0"
oxideav-ass = "0.0"
```

## Quick use

ASS is a text format rather than a bitstream, so "decode" means parsing
event lines + style metadata and "encode" means formatting back out to
the same text form. The container opens one `.ass` / `.ssa` file and
emits one packet per `Dialogue:` event; the codec on either side
converts packets to the shared `SubtitleCue` IR (from `oxideav-core`).

```rust
use oxideav_codec::CodecRegistry;
use oxideav_container::ContainerRegistry;
use oxideav_core::Frame;

let mut codecs = CodecRegistry::new();
let mut containers = ContainerRegistry::new();
oxideav_ass::register_codecs(&mut codecs);
oxideav_ass::register_containers(&mut containers);

let input: Box<dyn oxideav_container::ReadSeek> = Box::new(
    std::io::Cursor::new(std::fs::read("subtitle.ass")?),
);
let mut dmx = containers.open("ass", input)?;
let stream = &dmx.streams()[0];
let mut dec = codecs.make_decoder(&stream.params)?;

loop {
    match dmx.next_packet() {
        Ok(pkt) => {
            dec.send_packet(&pkt)?;
            while let Ok(Frame::Subtitle(cue)) = dec.receive_frame() {
                // cue.start_us / cue.end_us, cue.segments, cue.style_ref
            }
        }
        Err(oxideav_core::Error::Eof) => break,
        Err(e) => return Err(e.into()),
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Direct parse / write

If you just want the text format without the codec+container pipeline:

```rust
let track = oxideav_ass::parse(&std::fs::read("sub.ass")?)?;
let out_bytes = oxideav_ass::write(&track);
```

### Format conversion

Direct ASS / SRT and ASS / WebVTT conversion helpers are exposed — they
parse into the shared IR and re-emit in the target format.

```rust
let ass = oxideav_ass::srt_to_ass(&srt_bytes)?;
let srt = oxideav_ass::ass_to_srt(&ass_bytes)?;
let vtt = oxideav_ass::ass_to_webvtt(&ass_bytes)?;
let ass = oxideav_ass::webvtt_to_ass(&vtt_bytes)?;
```

## Feature coverage

What the parser understands and preserves on round-trip:

- `[Script Info]` — header key/value pairs captured as track metadata;
  comment lines (`;` / `!`) preserved inside extradata.
- **Unknown sections preserved** — editor-private blocks like
  `[Aegisub Project Garbage]`, `[Aegisub Extradata]`, `[Aegisub Style
  Storage]`, `[Fonts]`, `[Graphics]`, and any other named section not
  modelled by the parser have their body lines kept verbatim through
  `extradata`, so a parse → write round-trip emits them back unchanged
  (no dangling section headers, no lost editor state, no lost
  UU-encoded attachments).
- `[V4+ Styles]` and `[V4 Styles]` — `Format:`-aware per-`Style:`
  decode of name, font, size, primary / outline / back colours
  (`&HAABBGGRR` with ASS alpha inversion), bold / italic / underline /
  strikeout flags (including SSA's `-1` for true), alignment (both ASS
  `\an` and legacy SSA numpad schemes), margins, outline, and shadow
  widths.
- `[Events]` — `Format:`-aware; `Dialogue:` lines decode to
  `SubtitleCue` with start, end, style reference, and styled segments.
  `Comment:` events are dropped.
- Override tags inside dialogue text — `\b` (with both the `\b1`/`\b0`
  legacy toggle and the `\b<weight>` 100..900 integer form), `\i`,
  `\u`, `\s`, `\c` and `\1c` (primary colour), `\2c` / `\3c` / `\4c`
  (secondary / outline / shadow colour), `\alpha` and `\1a` / `\2a` /
  `\3a` / `\4a` (per-component alpha — ASS convention: 0 = opaque,
  255 = transparent), `\fn`, `\fe` (Windows charset ID for the
  glyph-mapping table), `\fs`, `\pos(x,y)`, `\an`, `\k` / `\kf` /
  `\ko` (karaoke timing markers), and `\r` / `\r<style>` (reset
  inline state; the named form switches the base style to a named
  definition from `[V4+ Styles]`). Unknown tags survive parsing as
  opaque pass-through so round-trip keeps them intact, even when
  mixed with tags the parser does interpret.
- **Typed face-state extraction** — `\fn<name>` (font family),
  `\fe<id>` (Windows charset ID, e.g. `128` = Shift-JIS, `134` =
  GB2312, `136` = BIG5), `\b<weight>` (font weight 100..900, with
  `\b1`/`\b0` mapping to 700/0), `\i<flag>` / `\u<flag>` /
  `\s<flag>` (italic / underline / strikeout boolean toggles), and
  `\r[<style>]` (reset to the line's base style or a named style).
  All seven surface through the `animate` module — call
  `evaluate_at(t_ms, dur_ms)` on the resulting `CueAnimation` and
  read `RenderState::font_name`, `font_encoding`, `bold_weight`,
  `italic`, `underline`, `strikeout`, and `reset_to_style`. None are
  animatable per spec; inside `\t(...)` they snap to the post-state
  at `t > t1`. The italic / underline / strikeout fields are
  `Option<bool>` — `None` means "fall back to the style's flag",
  while `Some(true)` / `Some(false)` carry the explicit override.
  The `Segment::Italic` / `Segment::Underline` / `Segment::Strike`
  wrappers the base parser emits for the byte-faithful round-trip
  are also walked, so a cue parsed straight from `\i1` text reaches
  `RenderState::italic = Some(true)` without a separate raw block.
  The full text round-trip continues to emit each tag verbatim
  through `Segment::Raw`.
- **Animated tags** — `\fad(t1,t2)`, `\fade(7-arg)`, `\pos(x,y)`
  (static line position; non-moving counterpart of `\move`, writes the
  same `translate` field), `\move(...)`,
  `\frz`, `\frx`, `\fry`, `\org(x,y)`, `\blur`, `\be`, `\bord`,
  `\xbord`, `\ybord`, `\shad`, `\xshad`, `\yshad`, `\fax`, `\fay`,
  `\fsp` (letter spacing, animatable), `\fscx` / `\fscy`, `\1c` /
  `\2c` / `\3c` / `\4c` (primary / secondary / outline / shadow
  colour), `\alpha` and `\1a` / `\2a` / `\3a` / `\4a` (per-component
  alpha), `\clip(rect)`, `\clip(drawing)`, `\iclip(rect)`,
  `\iclip(drawing)`, `\q` (line wrap-style override; static per spec),
  `\an<1..=9>` (numpad alignment) plus the legacy `\a<pos>` form
  (converted to the same numpad surface), `\pbo<y>` (drawing baseline
  Y-offset; positive = down, negative = up, applies only to `\p`
  drawing blocks), `\p<scale>` (drawing-mode toggle; `\p0` = text,
  `\p1` = drawing at native coordinates, `\p<N>` for `N >= 2` = drawing
  with sub-pixel scaling at `2^(N-1)`; static per spec), and `\t(...)` wrapping any of the animatable ones. These are exposed via the `animate` module:
  call `oxideav_ass::extract_cue_animation(&cue)` to get a typed
  `CueAnimation`, then `evaluate_at(t_ms, dur_ms)` to sample the
  resulting `RenderState` (alpha multiplier, `Transform2D`, optional
  clip + inverse-clip rect or drawing path, blur sigma, `\be`
  strength separate from `\blur`, per-axis border + shadow widths,
  `(fax, fay)` shear factors, additive letter spacing, line wrap
  style, line alignment as a numpad code, primary / secondary /
  outline / shadow colours, per-channel alphas independent of the
  `\fad` envelope, pivot, per-axis rotations, drawing baseline Y-
  offset, drawing-mode scale) at any timestamp. The
  textual round-trip continues to emit the original tags verbatim.
- **Karaoke timing** — the `\k` family (`\k` instant fill / `\kf` and
  the identical uppercase `\K` left-to-right sweep / `\ko` outline
  reveal) is extracted as typed `AnimatedTag::Karaoke { kind, cs }`
  markers (`KaraokeKind` + centisecond duration). Because karaoke is a
  per-syllable timeline rather than a per-frame state,
  `CueAnimation::karaoke_spans()` resolves the in-order markers into
  cumulative `KaraokeSpan`s (`start_ms`/`end_ms` from cue start), and
  `KaraokeSpan::progress(t)` gives the `0.0..=1.0` highlight position
  (the wipe fraction for a sweep syllable; the started/not-started
  boundary for the instant kinds). The evaluator leaves `RenderState`
  untouched for karaoke — renderers walk the spans. `\kt` is not
  modelled (undocumented per the Aegisub reference); it round-trips
  verbatim. Note: when karaoke is recovered through the base parser's
  collapsed `Segment::Karaoke` markers the family member is reported as
  the conservative `Fill` default (the core marker keeps only the
  duration); the full `KaraokeKind` survives when parsing raw override
  text directly via `parse_overrides`.
- **Drawing-mode parser** — the `\clip(drawing)` and `\p` mini
  language (`m`/`n`/`l`/`b`/`s`/`p`/`c`) is parsed via
  `oxideav_ass::parse_drawing(s, scale_exp)` into an
  `oxideav_core::Path`, ready to feed `oxideav-raster`'s clip stack.
- **Animated rasterisation** (`render` cargo feature, default-on) —
  `oxideav_ass::AnimatedRenderedDecoder` wraps another ASS subtitle
  decoder and produces RGBA `Frame::Video`s sampled at a
  caller-controlled cue-relative time; `set_offset_ms(t)` between
  `receive_frame` calls steps the animation forward. Internally it
  composes the evaluated `RenderState` (translate / scale / 3D
  rotations around `\org` / `\fax` / `\fay` shear pivoted on the
  cue's alignment point / clip path / opacity / `\an<n>` numpad
  alignment) onto a `VectorFrame` of shaped glyphs and rasterises
  through `oxideav-raster`. The shear pre-step uses the Aegisub
  override-tag reference's matrix `[[1, fax], [fay, 1]]` and pivots
  on the alignment point rather than `\org`, per the spec's "the
  coordinate system used for shearing is not affected by the
  rotation origin" rule. The numpad-alignment override (1..9; both
  `\an` and the legacy `\a` form land here) is decomposed into a
  horizontal column (left/centre/right) and a vertical row
  (bottom/middle/top): bottom-row cues sit above the canvas bottom
  margin (existing behaviour), top-row cues sit below the canvas
  top margin, and middle-row cues are centred on the canvas
  mid-line. The `\1a` primary-fill alpha override (Aegisub spec
  convention: `0 = opaque, 255 = transparent`) is baked into the
  rasterised fill colour as `final_a = 255 - ass_a`, while the
  cue-level `\fad` / `\fade` envelope stays on the animation
  `Group`'s `opacity` — the two compose multiplicatively per the
  formula `final_primary_alpha = primary_alpha.unwrap_or(style) *
  alpha_mul` documented on `RenderState::primary_alpha`. The
  `\blur<strength>` Gaussian edge-blur is applied as a post-step
  on the rasterised RGBA buffer via `oxideav-image-filter::Blur`:
  the wire `strength` is treated as the Gaussian sigma (in pixels,
  non-integer per the Aegisub spec), the separable-kernel radius
  is picked as `ceil(3 * sigma)` (3σ cutoff captures > 99.7% of
  the kernel mass), and the blur runs through all four RGBA
  channels — so the softened glyph edges land back via alpha,
  matching the spec's "blurs the edges of the text" effect for the
  no-`\bord` text path the renderer covers today. `\be`'s
  iterative box-blur strength is baked in as an N-pass 3×3 separable
  box average over the rasterised RGBA buffer (including alpha; runs
  *after* the `\blur` Gaussian step), matching the Aegisub spec's
  "regular effect, repeated `strength` times" definition. The two
  filters stay on independent `RenderState` channels (`blur_sigma`
  + `be_strength`) per the spec's "more advanced algorithm vs
  iterative" distinction. The `\fsp<spacing>` letter-spacing
  override is baked into the per-glyph X translation: each rendered
  glyph picks up an extra `index * fsp` shift on top of the shaper's
  cumulative pen position, so the gap between every pair of adjacent
  rendered glyphs grows by `fsp` script-resolution pixels (the spec
  describes the value as "the spacing between the individual
  letters", negative + decimal allowed). The widened line width is
  also folded into the alignment + greedy word-wrap step so a
  positive `\fsp` cannot fit more glyphs per visual line than the
  no-override baseline; `\fsp` ramps inside `\t(...)` per the spec
  also surface here, since the typed extractor already populates
  `RenderState::letter_spacing` at the sample time. The `\iclip(rect)` and `\iclip(drawing)`
  inverse-clip overrides are also baked in: the renderer constructs
  a compound clip path with an outer ring well past the canvas
  followed by the inverse subpath in reverse traversal direction so
  the rasteriser's NonZero fill rule sees the area *outside* the
  cut-out as the keep region. The clip-precedence chain is
  `\clip(drawing)` → `\clip(rect)` → `\iclip(drawing)` →
  `\iclip(rect)`; when both a positive `\clip` and an inverse
  `\iclip` appear on the same segment the positive form wins,
  matching the existing "last-set-wins" override model (the Aegisub
  override-tag reference describes each form independently and does
  not pin a co-occurrence rule). The `\shad<depth>` /
  `\xshad<depth>` / `\yshad<depth>` drop-shadow distance is baked
  in by pushing an extra translated-and-repainted shadow node
  *before* the primary fill node for each glyph on the line: the
  shadow colour comes from `\4c`
  (`RenderState::shadow_color`, defaulting to opaque black when the
  tag is absent), the shadow alpha follows the `\Xa` convention
  (wire `0` = opaque, `255` = transparent, mapped via
  `255 - ass_a`), and the per-axis offset is read straight off
  `RenderState::shadow`. Negative `\xshad` / `\yshad` values place
  the shadow above-left per the spec note that the per-axis forms
  accept negative depths; the shadow is disabled only when both X
  and Y distances are zero. The cue-level `\fad` / `\fade`
  envelope stays on the outer `Group::opacity` and composes
  multiplicatively over both the shadow and primary passes. Opt
  out via `default-features = false`.
- `\N` hard line break, `\h` hard space, `\n` soft break.
- ASS timestamp format `H:MM:SS.cc` (centiseconds).
- Commas inside the `Text` field are preserved (the CSV splitter stops
  at the per-format column count).

- **Typed `[Fonts]` / `[Graphics]` attachment accessor** — the SSA v4
  spec packs embedded font / picture files into the script via a
  printable-character encoding (Appendix B): three bytes are packed
  into a 24-bit value, split into four 6-bit fields, and each field
  is offset by 33 to produce an ASCII character. The base `parse`
  entry point still keeps each section body verbatim in `extradata`
  so a write loop emits the original printable lines unchanged, but
  `oxideav_ass::parse_attachments(&bytes)` now exposes the decoded
  binary form on top: it groups consecutive body lines under each
  `fontname: <name>` (Fonts) / `filename: <name>` (Graphics) marker
  and reverses the encoding back into `Vec<u8>`. The decoder handles
  all three input-length residues — multiples of three pack four
  characters into three bytes per quartet, a one-byte tail decodes
  from two characters (12-bit packed payload), and a two-byte tail
  decodes from three characters (18-bit packed payload). Body lines
  containing characters outside the SSA printable alphabet
  (`33..=126`, the spec's offset-of-33 rule) are skipped without
  killing the surrounding attachment. The returned `Attachment` carries
  `kind: AttachmentKind` (`Font` / `Graphics`), `name`, and the decoded
  `data: Vec<u8>` — consumers can feed font bytes straight into
  `oxideav-ttf` / `oxideav-otf` and picture bytes into the matching
  image decoder without re-implementing the SSA printable transform.
- **Typed `Effect:` column accessor** — the dialogue `Format:` row
  reserves a column for *transition effects* per the SSA v4.x spec.
  The base `parse` reads the `Format:` order, splits each event line,
  and drops the column on the floor (the shared IR has no slot for
  it). `oxideav_ass::parse_effect_field(field) -> EventEffect` lifts
  the column into a typed enum covering the four spec-defined
  effects: `Karaoke` (the obsolete per-word highlight from the SSA-v4
  era, replaced by the `\k` family of override tags), `Scroll
  up;y1;y2;delay[;fadeawayheight]` + the matching `Scroll
  down;…` sibling (vertical scroll inside a `[y1, y2]` pixel band,
  both zero means "scroll the full height of the screen"), and
  `Banner;delay[;lefttoright;fadeawaywidth]` (forced single-row
  horizontal scroll; `BannerDirection::RightToLeft` is the
  spec-default when the optional flag is missing). The keyword match
  is case-sensitive per the spec; anything that does not fit one of
  the four keywords surfaces on `EventEffect::Other(String)` with
  the raw bytes captured so a consumer can re-emit them verbatim
  through a write loop, and malformed payloads (missing parameters,
  non-numeric values, negative `delay`, invalid `lefttoright`)
  collapse to `Other` as well so the parser stays total. `delay`
  clamps to `0..=100` per the spec's slow-down knob range with `0`
  meaning "as fast as possible". A `scroll_region()` accessor
  returns a normalised `(top, bottom)` pair for both `Scroll`
  variants since the spec lets the script pass top / bottom in
  either order, and a `scrolls_full_height()` helper recognises the
  `y1 == y2 == 0` shorthand.
- **Typed per-event `Layer` accessor** — the dialogue `Format:` row
  reserves a column for the per-line `Layer` integer per the SSA v4.x
  spec ("any integer. Subtitles having different layer numbers will
  be ignored during the collision detection. Higher numbered layers
  will be drawn over the lower numbered."). The base `parse` reads
  the `Format:` order, splits each event line, and drops the column
  on the floor (the shared `SubtitleCue` IR has no slot for the
  per-event render-order integer), and the round-trip writer fills
  the column with a literal `0`.
  `oxideav_ass::parse_layer_field(field) -> LayerOverride` lifts the
  column into a typed two-state enum: `Default` (empty column /
  whitespace / the literal `0` in any sign form `0` / `+0` / `-0` —
  equivalent to "no per-event override; the base layer is `0` for
  both collision grouping and paint ordering") versus `Layer(i32)`
  (an explicit non-zero signed integer). The variant carries a
  signed `i32` because the spec's wording is "any integer";
  negative layers are legal and appear in hand-authored scripts as
  a deliberate "draw behind everything else" choice. The variant is
  `Copy + Eq` and a zero-cost `as_layer(self) -> Option<i32>`
  accessor plus a one-step `resolve(self) -> i32` "give me the
  effective render-order integer" path (with `Default` mapping to
  the spec's base `0`) round out the surface. Malformed columns
  (non-numeric content, bare `+` / `-`, `i32` overflow) collapse to
  `Default` so the parser stays total — the renderer transparently
  uses the base layer `0`, mirroring how the SSA reference treats an
  unset event-layer column. 20 unit tests cover empty,
  whitespace-only, the literal `0` in every sign form, explicit
  positive / negative / leading-`+` values, leading-zero magnitude
  padding (parsed as decimal, not octal), surrounding-whitespace
  tolerance, non-numeric rejection, `i32::MIN` and `i32::MAX`
  boundary round-trip, overflow rejection on both signs,
  bare-sign rejection, the `as_layer` accessor on both variants,
  the `resolve` accessor on both variants, the `Default` trait impl,
  `Copy + Eq` ergonomics, and the spec's two rendering ergonomics
  (`==` collision grouping + ascending `Ord` paint order).
- **Typed per-event margin accessor** — the dialogue `Format:` row
  reserves three columns for per-line `MarginL` / `MarginR` /
  `MarginV` overrides per the SSA v4.x spec. The base `parse` reads
  the `Format:` order, splits each event line, and drops the three
  columns on the floor (the shared IR has no slot for per-event
  margin overrides), and the round-trip writer fills the columns
  with literal `0`s.
  `oxideav_ass::parse_margin_field(field) -> MarginOverride` lifts
  any of the three columns into a typed two-state enum: `Default`
  (empty column / whitespace / the SSA `0` shorthand in any padded
  form — the spec's "4-figure" wording lets a script pad to a fixed
  width, so `0` / `00` / `000` / `0000` all mean "fall back to the
  style's matching margin") versus `Pixels(u32)` (an explicit
  non-zero pixel count). The variant is `Copy + Eq` and a
  zero-cost `as_pixels(self) -> Option<u32>` accessor plus a
  one-step `resolve_with_style(self, style_margin: u32) -> u32`
  fallback chain round out the surface. Malformed columns
  (negative integers, sign prefixes, non-numeric content, `u32`
  overflow) collapse to `Default` so the parser stays total — the
  renderer transparently picks up the style's matching margin,
  mirroring how the SSA reference treats the all-zero shorthand.
  The same function handles all three axes; the grammar is
  identical and callers select the axis at the call site by
  zipping `Format:` field names against split columns.
- **Typed per-event `Name` accessor** — the dialogue `Format:` row
  reserves a column for the per-line character / actor name per the
  SSA v4.x spec ("Field 5: Name — Character name. This is the name of
  the character who speaks the dialogue. It is for information only,
  to make the script easier to follow when editing/timing."). The
  base `parse` reads the `Format:` order, splits each event line, and
  drops the column on the floor (the shared `SubtitleCue` IR has no
  slot for the per-event speaker label), and the round-trip writer
  fills the column with an empty cell.
  `oxideav_ass::parse_name_field(field) -> NameOverride` lifts the
  column into a typed two-state enum: `Unset` (empty column or
  whitespace-only — the dominant case in real scripts; equivalent to
  "no per-event speaker label") versus `Name(String)` (an explicit
  non-empty character / actor name, surrounding whitespace trimmed).
  The accessors `as_name(&self) -> Option<&str>`,
  `into_name(self) -> Option<String>`, `is_set(&self) -> bool`, and
  `resolve(&self) -> &str` (with `Unset` mapping to the empty string)
  round out the surface. Per the spec the column is informational
  only and renderers ignore it; the accessor exists so editors and
  downstream tools that surface a per-line speaker column do not need
  to re-implement the dialogue-row split themselves. The parser is
  total — there is no error path; commas are not representable inside
  the column because the CSV split has already terminated it. 21 unit
  tests cover empty / whitespace-only columns, ASCII names, names
  containing inner spaces, surrounding-whitespace trimming, inner
  multi-space preservation, non-ASCII (CJK / Latin-with-diacritics /
  Greek) round-trip, punctuation (apostrophes / periods /
  parentheses), single-character names, the four accessors on both
  variants, the `Default` trait impl, `Eq` / `Clone` ergonomics,
  trim-equivalence between padded and unpadded forms, the
  `Unset`-vs-explicit-empty distinction at the constructor boundary,
  and a 1024-byte long-name round-trip.

Out of scope for this crate:

- (None on the blur axis — both `\blur<strength>` and `\be<strength>`
  are baked into the `AnimatedRenderedDecoder`; the two filters stay
  on separate channels per the Aegisub spec rather than being merged
  into one blur term.)
- 3D `\frx` / `\fry` rotations are reduced to a 2D affine via the
  orthographic small-angle approximation (axis-aligned `cos(α)`
  shrink), not a full perspective camera. Most subtitle use rotates
  <90° so the visual difference is small; consumers needing strict
  3D should bake their own perspective transform onto
  `RenderState::rotate_x_radians` / `rotate_y_radians`.
- Free-form `\p` drawing-mode rendering (the rasterisation of
  drawing blocks as decorative shapes) is parser-only — use
  `parse_drawing` to lift the path into your own scene. The
  `\p<scale>` toggle itself does surface on
  `RenderState::drawing_scale` so renderers know when to treat a
  text run as drawing commands (`Some(0)` = explicitly text mode;
  `Some(N)` for `N >= 1` = drawing mode with sub-pixel scale exponent
  `N - 1`, matching the `scale_exp` arg `parse_drawing` already
  accepts), and the baseline-offset companion tag `\pbo<y>` surfaces
  on `RenderState::drawing_baseline_offset`; renderers should
  translate their parsed path by
  `(0, drawing_baseline_offset.unwrap_or(0))` before rasterising.

### Codec / container IDs

- Codec: `"ass"`; media type `Subtitle`, intra-only, lossless.
- Container: `"ass"`, matches `.ass` and `.ssa` by extension and
  probes the `[Script Info]` header magic.

## License

MIT — see [LICENSE](LICENSE).
