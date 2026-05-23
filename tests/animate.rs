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

// -----------------------------------------------------------------------
// r76 typed-tag coverage: \bord, \shad, \fax/\fay, \iclip, \be — exercised
// end-to-end through the full parse → extract → evaluate pipeline against
// fixtures that mirror typical karaoke / typesetting subtitle authoring.

#[test]
fn typesetting_bord_shad_blur_combination() {
    // Common typesetting setup: text gets a thick outline + soft shadow
    // + Gaussian blur as a single override block.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\bord3\\shad2\\blur1.5\\be1}}typeset\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert_eq!(anim.tags.len(), 4, "tags: {:?}", anim.tags);
    let st = anim.evaluate_at(500, 1000);
    assert_eq!(st.border, Some((3.0, 3.0)));
    assert_eq!(st.shadow, Some((2.0, 2.0)));
    assert!((st.blur_sigma - 1.5).abs() < 1e-6);
    assert_eq!(st.be_strength, 1);
}

#[test]
fn t_animated_border_growth() {
    // Outline pulses from 0 to 4 over the cue — typical attention-grabber.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\bord0\\t(0,1000,\\bord4)}}grow\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    // 2 tags: the static \bord0 plus the \t(...) wrapper.
    assert_eq!(anim.tags.len(), 2);
    let st_q = anim.evaluate_at(250, 1000);
    assert_eq!(st_q.border, Some((1.0, 1.0)));
    let st_h = anim.evaluate_at(500, 1000);
    assert_eq!(st_h.border, Some((2.0, 2.0)));
    let st_e = anim.evaluate_at(1000, 1000);
    assert_eq!(st_e.border, Some((4.0, 4.0)));
}

#[test]
fn fax_skew_for_3d_perspective_label() {
    // \fax pseudo-3D skew — common signage typesetting trick.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\fax-0.3\\fay0.15}}skewed\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert_eq!(anim.tags.len(), 2);
    let st = anim.evaluate_at(500, 2000);
    assert!((st.shear.0 + 0.3).abs() < 1e-6);
    assert!((st.shear.1 - 0.15).abs() < 1e-6);
}

#[test]
fn iclip_rect_inverse_window() {
    // Inverse rectangular clip — used for "subtitle hides behind a
    // chyron" or "fade through a mask" effects.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\iclip(100,50,540,250)}}masked\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert_eq!(anim.tags.len(), 1);
    let st = anim.evaluate_at(0, 2000);
    let c = st.iclip_rect.unwrap();
    assert_eq!((c.x1, c.y1, c.x2, c.y2), (100.0, 50.0, 540.0, 250.0));
    // Forward clip stays untouched.
    assert!(st.clip_rect.is_none());
}

#[test]
fn iclip_drawing_path_preserved() {
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\iclip(m 0 0 l 50 0 l 50 50 l 0 50)}}vector\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert!(matches!(anim.tags[0], AnimatedTag::IClipDrawing(_)));
    let st = anim.evaluate_at(0, 2000);
    assert!(st.iclip_drawing.as_deref().unwrap().contains("m 0 0"));
}

#[test]
fn xbord_ybord_anamorphic_correction_pattern() {
    // Per Aegisub spec, \xbord+\ybord exist to correct anamorphic
    // displays. Authors sometimes follow with a \bord which overrides
    // both. Pin both behaviours.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\xbord2\\ybord4}}anamorphic\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    let st = anim.evaluate_at(0, 1000);
    assert_eq!(st.border, Some((2.0, 4.0)));

    let src2 = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\xbord2\\ybord4\\bord1}}override\n"
    );
    let t2 = ass::parse(src2.as_bytes()).unwrap();
    let st2 = extract_cue_animation(&t2.cues[0]).evaluate_at(0, 1000);
    assert_eq!(st2.border, Some((1.0, 1.0)));
}

#[test]
fn xshad_yshad_directional_shadow() {
    // Negative \xshad/\yshad places the shadow to the top-left.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\xshad-3\\yshad-2}}drop\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    let st = anim.evaluate_at(0, 1000);
    assert_eq!(st.shadow, Some((-3.0, -2.0)));
}

#[test]
fn unknown_tags_alongside_typed_tags_dont_panic() {
    // The base parser stuffs unknown tags into Raw blocks; animate
    // skips what it doesn't recognise. Verify a mix is handled cleanly.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\bord2\\xyz(1,2)\\fax0.1\\unknown}}mixed\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    // Only \bord and \fax should surface as typed tags.
    assert_eq!(anim.tags.len(), 2);
    let st = anim.evaluate_at(0, 1000);
    assert_eq!(st.border, Some((2.0, 2.0)));
    assert!((st.shear.0 - 0.1).abs() < 1e-6);
}

#[test]
fn typed_tags_survive_round_trip_as_passthrough() {
    // The typed tags aren't re-emitted by the writer (the round-trip
    // path is via Segment::Raw). Confirm the raw block is preserved
    // verbatim so the output script remains semantically identical.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\bord3\\shad2\\iclip(10,10,100,100)}}roundtrip\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let out = String::from_utf8(ass::write(&t)).unwrap();
    assert!(out.contains("\\bord3"), "out:\n{out}");
    assert!(out.contains("\\shad2"));
    assert!(out.contains("\\iclip(10,10,100,100)"));
    // Re-parse and re-extract: the typed values should match.
    let t2 = ass::parse(out.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t2.cues[0]);
    let st = anim.evaluate_at(0, 1000);
    assert_eq!(st.border, Some((3.0, 3.0)));
    assert_eq!(st.shadow, Some((2.0, 2.0)));
    assert!(st.iclip_rect.is_some());
}

#[test]
fn an_surfaces_alignment_on_render_state() {
    // End-to-end: the base parser previously consumed `\an` and only
    // kept the L/C/R nibble on `cue.positioning.align`; the full
    // numpad value (which tells the renderer which corner to anchor
    // `\pos`/`\move` against) was lost. The animate-module surface
    // now exposes it.
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\an7}}top-left\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let cue = &t.cues[0];
    let anim = extract_cue_animation(cue);
    assert!(
        anim.tags.contains(&AnimatedTag::An(7)),
        "tags: {:?}",
        anim.tags
    );
    let dur_ms = ((cue.end_us - cue.start_us) / 1000) as i32;
    let st = anim.evaluate_at(dur_ms / 2, dur_ms);
    assert_eq!(st.alignment, Some(7));
    // The cue-level CuePosition.align is still set so existing
    // consumers that read cue.positioning unchanged.
    let cp = cue.positioning.as_ref().expect("positioning set");
    assert_eq!(cp.align, oxideav_core::TextAlign::Left);
}

#[test]
fn an_round_trip_preserves_full_numpad() {
    // The writer used to lose the vertical row of the numpad
    // alignment (`\an7` came back as `\an1` in practice — and
    // actually wasn't re-emitted at all because the writer didn't
    // know how to spell it). Now the tag survives via Segment::Raw,
    // so a parse → write → reparse cycle preserves the numpad code.
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\an8}}top-center\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let out = String::from_utf8(ass::write(&t)).unwrap();
    assert!(out.contains("\\an8"), "writer output missing \\an8:\n{out}");

    let t2 = ass::parse(out.as_bytes()).unwrap();
    let anim2 = extract_cue_animation(&t2.cues[0]);
    let st2 = anim2.evaluate_at(0, 1000);
    assert_eq!(st2.alignment, Some(8));
}

#[test]
fn legacy_a_surfaces_alignment_on_render_state() {
    // `\a6` is the canonical legacy "top-center" code (sub-position
    // 2 + top-flag 4); per Aegisub it should behave identically to
    // `\an8`. The animate module converts on apply so renderers only
    // ever see the numpad value 1..=9.
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\a6}}top-center\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let cue = &t.cues[0];
    let anim = extract_cue_animation(cue);
    assert!(
        anim.tags.contains(&AnimatedTag::A(6)),
        "tags: {:?}",
        anim.tags
    );
    let st = anim.evaluate_at(0, 1000);
    assert_eq!(st.alignment, Some(8));
}

// Suppress an unused-import warning when only some helper types are used.
#[allow(dead_code)]
fn _ensure_cliprect_import_used(_: ClipRect) {}
