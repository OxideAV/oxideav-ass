# oxideav-ass benchmarks

Criterion benchmarks for the override-tag evaluation hot path
(`benches/evaluate.rs`). Run with:

```sh
cargo bench --bench evaluate
```

An animated renderer calls `extract_cue_animation` once per cue and
`evaluate_at` once per **frame** — at 24 fps a 5-second line samples
the evaluator 120 times — so the per-sample cost is the number that
matters. All inputs are synthesised in the setup step; no fixture
files.

## Results (r401 baseline)

Apple Silicon (aarch64-apple-darwin), rustc stable, `--release`.

| bench | time | notes |
|---|---|---|
| `parse_overrides_typical` | ~455 ns | `\pos` + accelerated `\t(\frz\clip)` + `\fad` block → typed tags |
| `parse_overrides_karaoke` | ~894 ns | 20-syllable `\k`/`\kf`/`\ko` line |
| `evaluate_at_static` | ~31 ns | static overrides only (common dialogue case) |
| `evaluate_at_animated` | ~83 ns | mid-ramp `\t` (accel + clip-rect lerp) + `\move` + `\fad` |
| `karaoke_spans` | ~113 ns | syllable timeline of the 20-syllable line |
| `track_parse_100_events` | ~87 µs | full-document `parse` of a 100-event script |
| `collision_resolve_64_normal` | ~1.9 µs | 64 boxes, 8 clusters of 8 overlapping lines |
| `collision_resolve_64_reverse` | ~750 ns | same boxes, `Reverse` policy |

Reading of the numbers:

- A worst-case animated sample costs ~83 ns, so a full 30-line
  on-screen stack at 60 fps spends ~150 µs/s in the evaluator —
  negligible next to rasterisation.
- Parsing dominates only at container-open time (~0.9 µs/event);
  per-frame work never re-parses (the typed `CueAnimation` is
  extracted once per cue).
- The `O(n²)` collision scan is ~2 µs at 64 simultaneous boxes —
  far beyond any real script's simultaneous line count, so the
  simple scan stays the right trade-off.
