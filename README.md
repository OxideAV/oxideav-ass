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
- `[V4+ Styles]` and `[V4 Styles]` — `Format:`-aware per-`Style:`
  decode of name, font, size, primary / outline / back colours
  (`&HAABBGGRR` with ASS alpha inversion), bold / italic / underline /
  strikeout flags (including SSA's `-1` for true), alignment (both ASS
  `\an` and legacy SSA numpad schemes), margins, outline, and shadow
  widths.
- `[Events]` — `Format:`-aware; `Dialogue:` lines decode to
  `SubtitleCue` with start, end, style reference, and styled segments.
  `Comment:` events are dropped.
- Override tags inside dialogue text — `\b`, `\i`, `\u`, `\s`, `\c`
  and `\1c` (primary colour), `\fn`, `\fs`, `\pos(x,y)`, `\an`, `\k`
  / `\kf` / `\ko` (karaoke timing markers), and `\r` (reset inline
  state). Unknown tags survive parsing as opaque pass-through so
  round-trip keeps them intact, even when mixed with tags the parser
  does interpret.
- **Animated tags** — `\fad(t1,t2)`, `\fade(7-arg)`, `\move(...)`,
  `\frz`, `\frx`, `\fry`, `\org(x,y)`, `\blur`, `\fscx` / `\fscy`,
  `\clip(rect)`, `\clip(drawing)`, and `\t(...)` wrapping any of the
  above. These are exposed via the `animate` module: call
  `oxideav_ass::extract_cue_animation(&cue)` to get a typed
  `CueAnimation`, then `evaluate_at(t_ms, dur_ms)` to sample the
  resulting `RenderState` (alpha multiplier, `Transform2D`, optional
  clip rect or drawing path, blur sigma, current colour, pivot,
  per-axis rotations) at any timestamp. The textual round-trip
  continues to emit the original tags verbatim.
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

- `[Fonts]` / `[Graphics]` UU-encoded attachments are parsed around
  (their lines are not copied into the re-emitted output).
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
  `parse_drawing` to lift the path into your own scene.

### Codec / container IDs

- Codec: `"ass"`; media type `Subtitle`, intra-only, lossless.
- Container: `"ass"`, matches `.ass` and `.ssa` by extension and
  probes the `[Script Info]` header magic.

## License

MIT — see [LICENSE](LICENSE).
