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
  drawing blocks), and `\t(...)` wrapping any of the animatable ones. These are exposed via the `animate` module:
  call `oxideav_ass::extract_cue_animation(&cue)` to get a typed
  `CueAnimation`, then `evaluate_at(t_ms, dur_ms)` to sample the
  resulting `RenderState` (alpha multiplier, `Transform2D`, optional
  clip + inverse-clip rect or drawing path, blur sigma, `\be`
  strength separate from `\blur`, per-axis border + shadow widths,
  `(fax, fay)` shear factors, additive letter spacing, line wrap
  style, line alignment as a numpad code, primary / secondary /
  outline / shadow colours, per-channel alphas independent of the
  `\fad` envelope, pivot, per-axis rotations, drawing baseline Y-
  offset) at any timestamp. The
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
  rotations around `\org` / clip path / opacity) onto a
  `VectorFrame` of shaped glyphs and rasterises through
  `oxideav-raster`. Opt out via `default-features = false`.
- `\N` hard line break, `\h` hard space, `\n` soft break.
- ASS timestamp format `H:MM:SS.cc` (centiseconds).
- Commas inside the `Text` field are preserved (the CSV splitter stops
  at the per-format column count).

Out of scope for this crate:

- `[Fonts]` / `[Graphics]` UU-encoded attachment payloads are kept as
  opaque bytes (round-tripped verbatim via extradata) — the parser
  does not decode the embedded font / image data into typed objects.
- Gaussian blur (`\blur`) post-step is not applied by the
  `AnimatedRenderedDecoder` — `RenderState::blur_sigma` is exposed,
  feed it into `oxideav-image-filter::blur` if you need the visual
  effect.
- 3D `\frx` / `\fry` rotations are reduced to a 2D affine via the
  orthographic small-angle approximation (axis-aligned `cos(α)`
  shrink), not a full perspective camera. Most subtitle use rotates
  <90° so the visual difference is small; consumers needing strict
  3D should bake their own perspective transform onto
  `RenderState::rotate_x_radians` / `rotate_y_radians`.
- Free-form `\p` drawing-mode rendering (the rasterisation of
  drawing blocks as decorative shapes) is parser-only — use
  `parse_drawing` to lift the path into your own scene. The
  baseline-offset companion tag `\pbo<y>` does surface on
  `RenderState::drawing_baseline_offset`; renderers should translate
  their parsed path by `(0, drawing_baseline_offset.unwrap_or(0))`
  before rasterising.

### Codec / container IDs

- Codec: `"ass"`; media type `Subtitle`, intra-only, lossless.
- Container: `"ass"`, matches `.ass` and `.ssa` by extension and
  probes the `[Script Info]` header magic.

## License

MIT — see [LICENSE](LICENSE).
