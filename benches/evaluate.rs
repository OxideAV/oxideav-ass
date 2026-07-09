//! Criterion benchmarks for the override-tag evaluation hot path.
//!
//! An animated renderer calls `extract_cue_animation` once per cue
//! and `evaluate_at` once per *frame* — at 24 fps a 5-second karaoke
//! line samples the evaluator 120 times, so `evaluate_at` dominates.
//! Every scenario builds its inputs in the setup step; only the
//! call under test sits in the timed region.
//!
//! Scenarios:
//!
//!   - **parse_overrides_typical**: one realistic override block
//!     (`\pos` + accelerated `\t(\frz\clip)` + `\fad`) through the
//!     typed tag reader.
//!   - **parse_overrides_karaoke**: a 20-syllable `\k`/`\kf`/`\ko`
//!     line — the karaoke-heavy authoring shape.
//!   - **evaluate_at_static**: sampling a cue with only static
//!     overrides (the common dialogue case).
//!   - **evaluate_at_animated**: sampling mid-ramp through `\t`
//!     (accelerated, with clip-rect interpolation) + `\move` +
//!     `\fad` — the worst per-frame case.
//!   - **karaoke_spans**: resolving the syllable timeline of the
//!     20-syllable line.
//!   - **track_parse_100_events**: full-document `parse` of a
//!     100-event script (container-open cost).
//!   - **collision_resolve_64**: `resolve_layout` over 64 boxes in
//!     8 overlapping clusters, both policies.

use criterion::{criterion_group, criterion_main, Criterion};

fn typical_block() -> &'static str {
    "\\pos(320,240)\\t(0,1000,2,\\frz360\\clip(0,0,100,100))\\fad(200,300)"
}

fn karaoke_block() -> String {
    let mut s = String::new();
    for i in 0..20 {
        s.push_str(match i % 3 {
            0 => "\\k20",
            1 => "\\kf35",
            _ => "\\ko15",
        });
    }
    s
}

fn script_100_events() -> Vec<u8> {
    let mut doc = String::from(
        "[Script Info]\n\
ScriptType: v4.00+\n\
PlayResX: 640\n\
PlayResY: 480\n\
\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,Arial,20,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,10,1\n\
\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
    );
    for i in 0..100 {
        let s = i % 60;
        let m = i / 60;
        doc.push_str(&format!(
            "Dialogue: 0,0:{m:02}:{s:02}.00,0:{m:02}:{s:02}.90,Default,,0,0,0,,\
{{\\pos(320,{})\\t(0,900,\\fscx120)\\fad(100,100)}}line {i} with some text\n",
            200 + (i % 200)
        ));
    }
    doc.into_bytes()
}

fn bench_all(c: &mut Criterion) {
    // --- parse_overrides ---------------------------------------------------
    c.bench_function("parse_overrides_typical", |b| {
        let block = typical_block();
        b.iter(|| {
            let mut tags = Vec::new();
            oxideav_ass::parse_overrides(std::hint::black_box(block), &mut tags);
            std::hint::black_box(tags)
        })
    });
    c.bench_function("parse_overrides_karaoke", |b| {
        let block = karaoke_block();
        b.iter(|| {
            let mut tags = Vec::new();
            oxideav_ass::parse_overrides(std::hint::black_box(&block), &mut tags);
            std::hint::black_box(tags)
        })
    });

    // --- evaluate_at -------------------------------------------------------
    {
        let mut tags = Vec::new();
        oxideav_ass::parse_overrides("\\an8\\fs32\\1c&H0000FF&\\bord2\\shad1\\blur1.5", &mut tags);
        let anim = oxideav_ass::CueAnimation { tags };
        c.bench_function("evaluate_at_static", |b| {
            b.iter(|| std::hint::black_box(&anim).evaluate_at(std::hint::black_box(2500), 5000))
        });
    }
    {
        let mut tags = Vec::new();
        oxideav_ass::parse_overrides(
            "\\move(0,0,640,480,0,5000)\\t(0,5000,2,\\frz360\\fscx200\\1c&H00FF00&\\clip(0,0,320,240))\\fad(300,300)",
            &mut tags,
        );
        let anim = oxideav_ass::CueAnimation { tags };
        c.bench_function("evaluate_at_animated", |b| {
            b.iter(|| std::hint::black_box(&anim).evaluate_at(std::hint::black_box(2500), 5000))
        });
    }

    // --- karaoke span resolution --------------------------------------------
    {
        let mut tags = Vec::new();
        oxideav_ass::parse_overrides(&karaoke_block(), &mut tags);
        let anim = oxideav_ass::CueAnimation { tags };
        c.bench_function("karaoke_spans", |b| {
            b.iter(|| std::hint::black_box(&anim).karaoke_spans())
        });
    }

    // --- full-document parse -------------------------------------------------
    {
        let doc = script_100_events();
        c.bench_function("track_parse_100_events", |b| {
            b.iter(|| oxideav_ass::parse(std::hint::black_box(&doc)).unwrap())
        });
    }

    // --- collision resolver ----------------------------------------------
    {
        // 64 boxes in 8 clusters of 8 mutually-overlapping lines.
        let boxes: Vec<oxideav_ass::CollisionBox> = (0..64)
            .map(|i| {
                let cluster = (i / 8) as i64;
                let k = (i % 8) as i64;
                oxideav_ass::CollisionBox::new(
                    cluster * 10_000 + k * 500,
                    cluster * 10_000 + 8_000,
                    30,
                )
            })
            .collect();
        let geo = oxideav_ass::CanvasGeometry {
            height_px: 480,
            bottom_margin_px: 20,
            top_margin_px: 0,
        };
        c.bench_function("collision_resolve_64_normal", |b| {
            b.iter(|| {
                oxideav_ass::resolve_layout(
                    std::hint::black_box(&boxes),
                    geo,
                    oxideav_ass::script_info::Collisions::Normal,
                )
            })
        });
        c.bench_function("collision_resolve_64_reverse", |b| {
            b.iter(|| {
                oxideav_ass::resolve_layout(
                    std::hint::black_box(&boxes),
                    geo,
                    oxideav_ass::script_info::Collisions::Reverse,
                )
            })
        });
    }
}

criterion_group!(benches, bench_all);
criterion_main!(benches);
