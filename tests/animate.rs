//! End-to-end animated-tag tests: parse a Dialogue line, extract the
//! typed animation, evaluate it at several timestamps, and verify the
//! resulting `RenderState`.

use oxideav_ass as ass;
use oxideav_ass::{extract_cue_animation, AnimatedTag, ClipRect};

const HEADER: &str = r"[Script Info]
ScriptType: v4.00+

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, Alignment, MarginL, MarginR, MarginV, Outline, Shadow
Style: Default,Arial,20,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,2,10,10,10,1,0

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
";

#[test]
fn fad_evaluates_alpha_curve() {
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\fad(200,300)}}hi\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let cue = &t.cues[0];
    let anim = extract_cue_animation(cue);
    // 1 tag: \fad(200, 300).
    assert_eq!(anim.tags.len(), 1, "tags: {:?}", anim.tags);
    assert!(matches!(
        anim.tags[0],
        AnimatedTag::Fad {
            t1_ms: 200,
            t2_ms: 300
        }
    ));

    let dur_ms = ((cue.end_us - cue.start_us) / 1000) as i32;
    assert_eq!(dur_ms, 2000);
    // At t=0 fully transparent.
    assert!((anim.evaluate_at(0, dur_ms).alpha_mul - 0.0).abs() < 1e-6);
    // After fade-in finished: opaque.
    assert!((anim.evaluate_at(200, dur_ms).alpha_mul - 1.0).abs() < 1e-6);
    // Mid-cue: opaque.
    assert!((anim.evaluate_at(1000, dur_ms).alpha_mul - 1.0).abs() < 1e-6);
    // Halfway through fade-out (last 300ms):
    assert!((anim.evaluate_at(1850, dur_ms).alpha_mul - 0.5).abs() < 1e-6);
    // At end: transparent.
    assert!((anim.evaluate_at(2000, dur_ms).alpha_mul - 0.0).abs() < 1e-6);
}

#[test]
fn move_evaluates_translation() {
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\move(0,0,200,400)}}hello\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert!(matches!(anim.tags[0], AnimatedTag::Move { .. }));
    let s_start = anim.evaluate_at(0, 1000);
    assert_eq!(s_start.translate, Some((0.0, 0.0)));
    let s_mid = anim.evaluate_at(500, 1000);
    assert_eq!(s_mid.translate, Some((100.0, 200.0)));
    let s_end = anim.evaluate_at(1000, 1000);
    assert_eq!(s_end.translate, Some((200.0, 400.0)));
}

#[test]
fn frz_static_rotation() {
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\frz45}}rotated\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert!(matches!(anim.tags[0], AnimatedTag::Frz(45.0)));
    let st = anim.evaluate_at(500, 1000);
    assert!((st.rotate_radians - std::f32::consts::FRAC_PI_4).abs() < 1e-5);
}

#[test]
fn t_interpolates_scale_and_rotate() {
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\t(0,2000,\\fscx200\\frz90)}}grow\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    // Should have one T tag with two inner tags.
    assert_eq!(anim.tags.len(), 1);
    let dur_ms = ((t.cues[0].end_us - t.cues[0].start_us) / 1000) as i32;
    let st0 = anim.evaluate_at(0, dur_ms);
    assert_eq!(st0.scale, (1.0, 1.0));
    assert!(st0.rotate_radians.abs() < 1e-6);

    let st_mid = anim.evaluate_at(1000, dur_ms);
    // Halfway: scale.x = 1.5, rotate = 45deg.
    assert!((st_mid.scale.0 - 1.5).abs() < 1e-6);
    assert!((st_mid.rotate_radians - std::f32::consts::FRAC_PI_4).abs() < 1e-5);

    let st_end = anim.evaluate_at(2000, dur_ms);
    assert!((st_end.scale.0 - 2.0).abs() < 1e-6);
    assert!((st_end.rotate_radians - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
}

#[test]
fn clip_rect_is_extracted() {
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\clip(10,20,300,200)}}clipped\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    let st = anim.evaluate_at(0, 1000);
    assert_eq!(
        st.clip_rect,
        Some(ClipRect {
            x1: 10.0,
            y1: 20.0,
            x2: 300.0,
            y2: 200.0,
        })
    );
}

#[test]
fn blur_is_extracted() {
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\blur4.5}}fuzzy\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    let st = anim.evaluate_at(0, 1000);
    assert!((st.blur_sigma - 4.5).abs() < 1e-6);
}

#[test]
fn fscx_fscy_static() {
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\fscx200\\fscy50}}stretched\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    let st = anim.evaluate_at(0, 1000);
    assert_eq!(st.scale, (2.0, 0.5));
}

#[test]
fn round_trip_preserves_animated_tags() {
    // After implementing the renderer view, the textual round-trip
    // must still emit the original animated tags verbatim — the
    // encoder side is unchanged.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\fad(100,200)\\move(0,0,100,100)\\frz45\\blur2}}hello\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let out = String::from_utf8(ass::write(&t)).unwrap();
    for needle in [
        "\\fad(100,200)",
        "\\move(0,0,100,100)",
        "\\frz45",
        "\\blur2",
        "hello",
    ] {
        assert!(out.contains(needle), "missing {needle:?} in:\n{out}");
    }
    // Re-extraction works on the re-parsed text.
    let t2 = ass::parse(out.as_bytes()).unwrap();
    let anim2 = extract_cue_animation(&t2.cues[0]);
    assert!(anim2
        .tags
        .iter()
        .any(|t| matches!(t, AnimatedTag::Fad { .. })));
    assert!(anim2
        .tags
        .iter()
        .any(|t| matches!(t, AnimatedTag::Move { .. })));
    assert!(anim2.tags.iter().any(|t| matches!(t, AnimatedTag::Frz(_))));
    assert!(anim2.tags.iter().any(|t| matches!(t, AnimatedTag::Blur(_))));
}

#[test]
fn t_inside_dialogue_round_trip_ok() {
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\t(0,2000,\\fscx200)}}grow\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let out = String::from_utf8(ass::write(&t)).unwrap();
    assert!(out.contains("\\t(0,2000,\\fscx200)"), "got:\n{out}");
}

#[test]
fn move_with_default_times_uses_full_cue() {
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\move(0,0,100,200)}}drift\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    let dur_ms = ((t.cues[0].end_us - t.cues[0].start_us) / 1000) as i32;
    let st = anim.evaluate_at(1000, dur_ms);
    assert_eq!(st.translate, Some((50.0, 100.0)));
}

#[test]
fn empty_cue_yields_empty_animation() {
    let src = format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,plain text\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert!(anim.is_empty());
    let st = anim.evaluate_at(500, 1000);
    assert_eq!(st.alpha_mul, 1.0);
    assert!(st.transform.is_identity());
    assert!(st.translate.is_none());
}
