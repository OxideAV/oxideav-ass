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

### Structured document model

For callers that need *field-level* fidelity — reading or editing every
column of every line and re-serialising without the shared IR's lossy
projection — the crate exposes a structured document model alongside the
IR path:

```rust
let script = oxideav_ass::parse_script(&std::fs::read("sub.ass")?);
// Section-by-section, fully-typed access:
for style in script.styles() {
    println!("{} @ {}pt, border-style {:?}", style.name, style.fontsize,
             style.border_style_typed());
}
for event in script.events() {
    let (l, r, v) = event.margins_typed();          // per-event margins
    let tags = event.override_tags();               // typed \pos / \fad / …
    println!("{:?} layer={} {:?}", event.kind, event.layer_typed().resolve(),
             event.effect_typed());
    let _ = (l, r, v, tags);
}
// Edit + re-serialise (byte-stable, re-parse fixpoint):
let out_bytes = script.serialise();
// Or project onto the shared IR (lossy, dialogue-only cue stream):
let track = script.to_track();
# Ok::<(), Box<dyn std::error::Error>>(())
```

`AssScript` keeps every section in source order — `[Script Info]` as
ordered key/value/comment/blank lines, `[V4+ Styles]` / legacy
`[V4 Styles]` as a `Format:`-aware table of `StyleDef` rows capturing
**all** SSA v4.x / ASS columns (incl. `SecondaryColour` / `AlphaLevel` /
`ScaleX` / `ScaleY` / `Spacing` / `Angle` / `BorderStyle` / `Encoding`),
`[Events]` as a `Format:`-aware table of `Event` rows tagged by
`EventKind` (`Dialogue` / `Comment` / `Picture` / `Sound` / `Movie` /
`Command`), and every other section (`[Fonts]`, `[Graphics]`, editor-
private `[Aegisub …]` blocks) verbatim as a `RawSection`. Colour and
numeric columns keep their raw wire token so re-serialisation preserves
the author's exact spelling, and `serialise()` is a fixpoint under
re-parse. Typed accessors (`layer_typed` / `effect_typed` /
`margins_typed` / `border_style_typed` / `encoding_typed` /
`override_tags`) reuse the per-column modules below.

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
- **Typed `[Script Info]` document-level accessors** (`script_info`
  module) — the SSA v4.x / ASS spec documents several `[Script Info]`
  keys as document-wide render parameters: `WrapStyle` (the default
  line-wrapping mode `0`–`3`, numbered identically to the per-line `\q`
  override — smart-even / end-of-line / no-wrap / smart-wide),
  `Collisions` (overlapping-subtitle reposition policy — `Normal` stacks
  upward from the bottom margin, `Reverse` shifts earlier lines up to
  make room), `PlayResX` / `PlayResY` (the script-resolution pixel space
  every `\pos` / `\move` / `\clip` / `\org` coordinate lives in),
  `PlayDepth` (colour depth in bits), and `Timer` (the playback timer
  speed as a percentage). The base `parse` keeps these as raw
  `metadata` strings; `oxideav_ass::parse_wrap_style_field` /
  `parse_collisions_field` / `parse_play_res_field` /
  `parse_play_depth_field` / `parse_timer_field` lift them into typed
  values, and the structured model surfaces them on
  `ScriptInfo::{wrap_style, collisions, play_res_x, play_res_y,
  play_depth, timer}`. The resolution / depth accessors return
  `Option<u32>` (`None` when the header is absent or non-positive, so
  the caller picks the video-resolution fall-back); `Timer` is returned
  as a fractional multiplier (`"100.0000"` → `1.0`). Every field parser
  is total — a malformed value collapses to the spec's documented
  default (`WrapStyle` → smart-even, `Collisions` → `Normal`, `Timer` →
  `1.0`). `WrapStyle::from_code` / `WrapStyle::resolve_override` bridge
  the document default to a per-line `\q` override (the per-line code
  wins when present), so a renderer reasons about one wrapping model
  whether the mode arrives via the header or a tag. `ScaledBorderAndShadow`
  is not modelled — it is absent from both mirrored spec documents.
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
  modelled (undocumented in the spec); it round-trips
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
  through `oxideav-raster`. The shear pre-step uses the override-tag spec's matrix `[[1, fax], [fay, 1]]` and pivots
  on the alignment point rather than `\org`, per the spec's "the
  coordinate system used for shearing is not affected by the
  rotation origin" rule. The numpad-alignment override (1..9; both
  `\an` and the legacy `\a` form land here) is decomposed into a
  horizontal column (left/centre/right) and a vertical row
  (bottom/middle/top): bottom-row cues sit above the canvas bottom
  margin (existing behaviour), top-row cues sit below the canvas
  top margin, and middle-row cues are centred on the canvas
  mid-line. The `\1a` primary-fill alpha override (spec convention: `0 = opaque, 255 = transparent`) is baked into the
  rasterised fill colour as `final_a = 255 - ass_a`, while the
  cue-level `\fad` / `\fade` envelope stays on the animation
  `Group`'s `opacity` — the two compose multiplicatively per the
  formula `final_primary_alpha = primary_alpha.unwrap_or(style) *
  alpha_mul` documented on `RenderState::primary_alpha`. The
  `\blur<strength>` Gaussian edge-blur is applied as a post-step
  on the rasterised RGBA buffer via `oxideav-image-filter::Blur`:
  the wire `strength` is treated as the Gaussian sigma (in pixels,
  non-integer per the spec), the separable-kernel radius
  is picked as `ceil(3 * sigma)` (3σ cutoff captures > 99.7% of
  the kernel mass), and the blur runs through all four RGBA
  channels — so the softened glyph edges land back via alpha,
  matching the spec's "blurs the edges of the text" effect (the
  `\bord` ring is baked into the same buffer, so bordered edges
  soften the same way). `\be`'s
  iterative box-blur strength is baked in as an N-pass 3×3 separable
  box average over the rasterised RGBA buffer (including alpha; runs
  *after* the `\blur` Gaussian step), matching the spec's
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
  matching the existing "last-set-wins" override model (the override-tag spec describes each form independently and does
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
  multiplicatively over both the shadow and primary passes. The
  `\bord<width>` / `\xbord<width>` / `\ybord<width>` border is
  baked in as a filled-and-stroked glyph silhouette pushed *under*
  each glyph's primary fill (and after the shadow node): the
  stroke is centred on the glyph edge at twice the border width so
  the visible ring extends exactly `width` pixels outward once the
  fill covers the inner half, with round caps + joins so the ring
  stays uniform at sharp corners. The ring colour comes from `\3c`
  (defaulting to opaque black like the shadow's `\4c` fallback),
  the ring alpha from `\3a` on the usual `255 - ass_a` wire
  mapping; `\bord0` (or no override) skips the pass entirely per
  the spec's "set the size to 0 to disable the border entirely"
  rule, and `\bord` ramps inside `\t(...)` resample the ring width
  at every frame. An unequal `\xbord` / `\ybord` pair is reduced
  to an isotropic ring at the larger width — a stroked outline has
  a single width, and the spec describes the per-axis form as an
  anamorphic-rendering correction, so real pairs stay close. When
  both `\bord` and `\shad` are active the shadow copy carries the
  same stroke repainted in the shadow colour, so the shadow is
  cast by the *bordered* silhouette (the spec notes `\shad` "works
  similar to \bord"). The `\u<flag>` underline and `\s<flag>`
  strikeout decorations (`RenderState::underline` /
  `RenderState::strikeout`, both `Option<bool>`) are baked in as a
  filled horizontal bar spanning each visual line's shaped width in
  the primary fill colour — decorations inherit the text colour. The
  spec pins only the on/off toggle (`\u1`/`\u0`, `\s1`/`\s0`) and no
  line geometry, so the placement is derived from the font metrics
  already on the face: thickness `max(1px, size / 18)`, the underline
  bar `descent * 0.5` below the baseline (upper descender band, clear
  of glyph bowls), and the strikeout bar centred `ascent * 0.3` above
  the baseline (through the x-height band). The bars ride the same
  inner group as the glyphs, so `\fad` opacity / `\frz` rotation /
  `\clip` / the animation transform compose over them as over text,
  and an active drop-shadow casts a congruent shadow copy of each
  bar. A `None` (no `\u` / `\s` override) resolves to "off" — the
  style's `Underline` / `StrikeOut` columns are not yet plumbed
  through to the renderer (the same gap `\fsp` falls through). The
  `\i<flag>` italic toggle (`RenderState::italic`, `Option<bool>`)
  is baked in as a synthetic oblique slant: the face chain carries a
  single upright cut with no italic variant to swap in, so an
  explicit `\i1` leans every glyph through a baseline-pivoted
  horizontal shear — the conventional faux-italic substitution a
  text engine applies when a true italic is unavailable. The spec
  pins only the on/off toggle (`\i1`/`\i0`) and no slant angle, so
  the lean is a renderer-derived `~12°` oblique (the same family of
  metric-derived constant as the `\u` / `\s` bar geometry above),
  applied in canvas space on top of each glyph's positioning
  transform so it composes under the `\frz` / `\fax` / `\fad` /
  `\clip` envelope, and the underline / strikeout bars (and the
  drop-shadow copies) ride the same shear so they stay congruent
  with the slanted text. `\i0` and a missing `\i` both render
  upright — the style's `Italic` column is not yet plumbed through
  to the renderer (the same gap `\u` / `\s` / `\fsp` fall through).
  Opt out via `default-features = false`.
- **`\q` wrap-mode word-wrap** (`render` cargo feature) — the
  `AnimatedRenderedDecoder` resolves the effective SSA wrap mode per
  rendered line and breaks accordingly instead of always greedy-
  wrapping. The per-line `\q<n>` override (`RenderState::wrap_style`)
  wins over the decoder's `default_wrap_style` field — the document-
  level `[Script Info]` `WrapStyle` header, defaulting to the spec's
  implicit mode `0` (smart-even) — via
  `WrapStyle::resolve_override`. The four modes follow the `\q`
  reference: mode `2` (no-wrap) never auto-breaks (the line runs past
  the edge; only explicit `\n` / `\N` split it), mode `1` (end-of-
  line) greedy-fills each row to the edge, and modes `0` (smart-even)
  / `3` (smart-wide) balance the visual rows so they come out as even
  in width as the word boundaries allow. Smart wrapping never uses
  more rows than the greedy fill would: it counts that row budget,
  binary-searches the tightest width that still fits the budget, and
  greedy-fills there so the early rows stop hogging words and the
  short tail evens out. Mode `3` reverses the fill so the leftover
  slack lands on the *upper* rows, making the lower rows the wider
  ones (mode `0` keeps the natural top-wider bias). A lone word wider
  than the canvas can't be split, so every mode keeps it intact on
  its own row. Opt out via `default-features = false`.
- **`\p` drawing-mode rasterisation** (`render` cargo feature) — a
  cue whose resolved `RenderState::drawing_scale` is `Some(N)` with
  `N >= 1` is no longer shaped as glyphs: the
  `AnimatedRenderedDecoder` parses its text run through
  `parse_drawing` (with the `\p<N>` `2^(N-1)` scale exponent),
  auto-closes each subpath the way an ASS fill does ("when you close
  the line formed, it fills it with the primary color"), and
  rasterises it as a filled vector shape. Per the override-tag spec, "drawing commands use the primary color for
  fill and outline color for borders. They also display shadow" — so
  the fill comes from `\1c` (`\1a` alpha on the usual `255 - ass_a`
  wire mapping), the border ring from `\3c` / `\bord` (stroked at
  twice the width and filled so a translucent interior shows the ring
  colour), and the drop shadow from `\4c` / `\shad` (drawn first,
  carrying the bordered stroke when `\bord` is active). The drawing
  is anchored at the `\move` / `\pos` point (or the cue's static
  `\pos(x,y)` from `positioning`, falling back to the alignment-
  derived margin anchor for a bare `{\p1}m …`), and the `\pbo<y>`
  baseline offset is baked straight into the path Y coordinates
  (positive = down). The whole drawing rides the *same* animation
  `Group` the glyph path uses — `\fad` / `\fade` opacity, `\frz` /
  `\frx` / `\fry` rotation, `\fscx` / `\fscy` scale, `\fax` / `\fay`
  shear, and the `\clip` / `\iclip` precedence chain all compose over
  a drawing exactly as over text, and `\blur` / `\be` soften its
  edges through the same post-steps. Opt out via
  `default-features = false`.
- `\N` hard line break, `\h` hard space, `\n` soft break.
- **UTF-8 dialogue text** — the text segmenter scans for the ASCII
  `{` override-block and `\` escape markers byte-wise, but emits literal
  runs one full Unicode scalar at a time, so multi-byte glyphs (CJK,
  Latin-with-diacritics, emoji, …) survive verbatim instead of being
  split into Latin-1 mojibake. Covered by three round-trip tests
  exercising the 2-, 3-, and 4-byte UTF-8 encodings around override
  blocks, `\N` breaks, and literal backslashes.
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
- **Typed `BorderStyle` style accessor** — the `[V4+ Styles]` /
  `[V4 Styles]` `Format:` row reserves a column for the per-style
  rendering mode per the SSA v4.x / ASS spec. The base `parse` decodes
  the style columns the shared `SubtitleStyle` IR has a slot for (name,
  font, sizes, colours, flags, alignment, margins, outline and shadow
  widths) and reads past the `BorderStyle` column — the IR has no field
  for the rendering mode. `oxideav_ass::parse_border_style_field(field)
  -> BorderStyle` lifts the column into a typed enum covering the two
  spec-defined values: `OutlineDropShadow` (`1` — the dominant mode;
  text drawn with an outline plus drop shadow whose widths come from
  the neighbouring `Outline` / `Shadow` columns) and `OpaqueBox` (`3` —
  text sits on a filled rectangle in the outline colour, so the
  `Outline` / `Shadow` widths no longer describe an outline + drop
  shadow). The enum is `Copy + Eq + Default` (defaulting to
  `OutlineDropShadow`); `as_code(self) -> u8` round-trips the raw spec
  integer back into the column and `is_opaque_box(self) -> bool` lets a
  renderer branch between the outline + drop-shadow path and the
  box-backdrop path. The parser is total — empty / whitespace /
  non-numeric / out-of-range columns and any integer other than the
  two spec values (the SSA-era `0`, the unused `2` / `4`, negatives)
  all collapse to `OutlineDropShadow`, mirroring how the SSA reference
  treats an unrecognised value; a leading `+` and leading-zero decimal
  padding are tolerated. 15 unit tests cover the two spec values,
  empty / whitespace columns, trimming, the `+` / leading-zero forms,
  the collapse cases, non-numeric + overflow rejection, both accessors
  on both variants, the `Default` impl, `Copy + Eq` ergonomics, and an
  invariant that `as_code` only ever emits a valid spec integer.
- **Typed per-style geometry accessor (`ScaleX` / `ScaleY` / `Spacing` /
  `Angle`)** — the `[V4+ Styles]` `Format:` row reserves four columns for
  the per-style font transform per the SSA v4.x / ASS spec: `ScaleX`
  ("modifies the width of the font [percent]"), `ScaleY` ("modifies the
  height of the font [percent]"), `Spacing` ("extra space between
  characters [pixels]"), and `Angle` (baseline rotation, "the origin of
  the rotation is defined by the alignment", floating point, degrees).
  The base `parse` reads past all four — the shared `SubtitleStyle` IR has
  no slot for them. These are the *style-level* counterparts of override
  tags already surfaced through the `animate` module: `ScaleX` / `ScaleY`
  mirror `\fscx` / `\fscy`, `Spacing` mirrors `\fsp`, and `Angle` mirrors
  `\frz`; the per-segment override wins when present, the style column
  supplies the per-line baseline. `oxideav_ass::parse_scale_field`,
  `parse_spacing_field`, and `parse_angle_field` resolve a single column
  to an `f64`, and `parse_style_transform(sx, sy, sp, an) -> StyleTransform`
  lifts all four at once into a `Copy` struct with `scale_x` / `scale_y`
  / `spacing` / `angle` fields plus an `is_identity()` helper. The
  `Default` is the identity transform (`100` / `100` / `0` / `0`). The
  spec gives units but pins no explicit default for a missing column, so
  each parser falls back independently to its neutral value (no scaling /
  no spacing / no rotation) — a malformed column resets only its own axis.
  Parsers are total: empty, whitespace, non-numeric, and non-finite
  (`NaN` / `inf` / overflow) columns all collapse to the identity value,
  and every resolved field is guaranteed finite. Fractional, signed
  (negative spacing / angle), leading-`+`, and leading-zero magnitudes
  are accepted. 20 unit tests cover plain / fractional / signed values,
  the per-axis fall-back, finiteness guarantees, overflow rejection, the
  `is_identity` helper, the `Default` impl, and `Copy`/`Clone` ergonomics.
- **Typed per-style `Encoding` accessor** — the `[V4+ Styles]` /
  `[V4 Styles]` `Format:` row reserves a column (Field 18) for the
  per-style font character set per the SSA v4.x / ASS spec
  (*"specifies the font character set or encoding… It is usually 0
  (zero) for English (Western, ANSI) Windows"*). The base `parse` reads
  past it — the shared `SubtitleStyle` IR has no slot for the per-style
  charset. This is the *style-level* counterpart of the per-segment
  `\fe<id>` override already surfaced through the `animate` module: both
  carry a Windows charset numeric ID that selects the glyph-mapping
  table, and the per-segment override wins when present.
  `oxideav_ass::parse_encoding_field(field) -> StyleEncoding` lifts the
  column into a `Copy + Eq` struct carrying `charset: u8` (the Win32
  charset ID), with `as_code()` (round-trips the raw ID back into the
  column), `is_ansi()` (branch on the dominant `0` case), and
  `charset_name()` returning the documented common slot name (`0` ANSI /
  `1` Default / `2` Symbol / `128` Shift-JIS / `134` GB2312 / `136`
  BIG5 / `162` Turkish / `163` Vietnamese / `177` Hebrew / `178`
  Arabic) or `None` for any other legal ID. The `Default` is ANSI (`0`).
  The parser is total — empty / whitespace / non-numeric / out-of-
  `0..=255`-range columns all collapse to ANSI, the spec's "usually 0"
  default, mirroring how the SSA reference treats an unset value. 16
  unit tests cover the ANSI default, the named common slots, the
  out-of-range / non-numeric / overflow collapse, leading-`+` /
  leading-zero magnitudes, whitespace trimming, both accessors on the
  named + unnamed slots, the `from_charset` constructor, the `Default`
  impl, and `Copy + Eq` ergonomics.
- **Typed per-style `Alignment` accessor** — the `[V4+ Styles]` /
  `[V4 Styles]` `Format:` row reserves a column for the on-screen anchor
  per the SSA v4.x / ASS spec. The base `parse` decodes the column into
  the shared `SubtitleStyle::align` IR but keeps only the *horizontal*
  justification (left / centre / right) — `TextAlign` has no slot for
  the *vertical* row, so the top / middle / bottom placement the column
  also carries is dropped. `oxideav_ass::parse_alignment_field(field,
  is_ssa) -> StyleAlignment` lifts the full numpad anchor into a typed
  `{ horizontal: AlignH, vertical: AlignV }` pair, handling both spec
  numbering schemes: the ASS `[V4+ Styles]` numpad code (`1..=9`, with
  `1-3` bottom / `4-6` middle / `7-9` top — the same numbering `\an<n>`
  uses) and the legacy SSA `[V4 Styles]` bit scheme (`1`/`2`/`3` =
  L/C/R, `+4` = toptitle, `+8` = midtitle, so the spec's worked example
  `5` = left-justified toptitle). The two schemes normalise to the same
  `StyleAlignment`, so a renderer reasons about one anchor model
  regardless of dialect. `as_numpad()` round-trips the ASS code,
  `as_ssa()` round-trips the legacy code, and `is_bottom()` branches the
  dominant subtitle row. The `Default` is bottom-centre (numpad `2`).
  The parser is total — empty / whitespace / non-numeric / out-of-range
  columns all collapse to bottom-centre, matching the base parser's
  `unwrap_or(2)` fall-back. 20 unit tests cover the full ASS 1..=9 grid,
  the SSA L/C/R × bottom/top/middle grid, both round-trips, the spec
  worked example, cross-scheme anchor agreement, the malformed-column
  collapse on both schemes, the SSA both-row-bits and centre-low-bits
  edge cases, leading-`+` / leading-zero magnitudes, whitespace
  trimming, the `is_bottom` accessor, the `Default` impl, and `Copy +
  Eq` ergonomics.

Out of scope for this crate:

- (None on the blur axis — both `\blur<strength>` and `\be<strength>`
  are baked into the `AnimatedRenderedDecoder`; the two filters stay
  on separate channels per the spec rather than being merged
  into one blur term.)
- 3D `\frx` / `\fry` rotations are reduced to a 2D affine via the
  orthographic small-angle approximation (axis-aligned `cos(α)`
  shrink), not a full perspective camera. Most subtitle use rotates
  <90° so the visual difference is small; consumers needing strict
  3D should bake their own perspective transform onto
  `RenderState::rotate_x_radians` / `rotate_y_radians`.
- Mixed text-and-drawing in a *single* cue (`{\p1}…{\p0}TEXT`) is
  resolved against the cue-wide `RenderState` rather than per
  segment: the renderer picks the drawing-fill path or the glyph
  path from the cue's *final* `\p` toggle, so a cue that flips back
  to text mode mid-line will shape its leftover drawing tokens as
  glyphs. Author each drawing block as its own cue (the dominant
  real-world layout) for clean output. Per-segment mode tracking is
  a future refinement.
- 3D `\frx` / `\fry` rotations on a drawing block share the glyph
  path's small-angle 2D-affine approximation rather than a full
  perspective camera (same caveat as text below).

### Codec / container IDs

- Codec: `"ass"`; media type `Subtitle`, intra-only, lossless.
- Container: `"ass"`, matches `.ass` and `.ssa` by extension and
  probes the `[Script Info]` header magic.

## License

MIT — see [LICENSE](LICENSE).
