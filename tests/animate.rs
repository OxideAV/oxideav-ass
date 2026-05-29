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

// -----------------------------------------------------------------------
// r131 typed-tag coverage: \frx / \fry — the X- and Y-axis 3D rotation
// pair from the Aegisub override-tag reference. \frx and \fry are
// documented alongside \frz as the "text rotation" family (X / Y / Z
// axis, in degrees). The parser already produces typed
// `AnimatedTag::Frx` / `Fry` variants; these tests pin the static-
// extraction path, the `\t(...)` interpolation path, and the textual
// round-trip behaviour, mirroring the existing `\frz` tests above so a
// regression in either family fails an explicit assertion.

#[test]
fn frx_static_rotation() {
    // \frx45 surfaces as `AnimatedTag::Frx(45.0)` and writes
    // `rotate_x_radians = π/4` on the resolved RenderState — parallel
    // to the \frz45 case above.
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\frx45}}flip-x\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert!(matches!(anim.tags[0], AnimatedTag::Frx(45.0)));
    let st = anim.evaluate_at(500, 1000);
    assert!((st.rotate_x_radians - std::f32::consts::FRAC_PI_4).abs() < 1e-5);
    // The Z-axis rotation stays untouched (independent state field).
    assert!(st.rotate_radians.abs() < 1e-6);
    assert!(st.rotate_y_radians.abs() < 1e-6);
}

#[test]
fn fry_static_rotation() {
    // \fry-45 — negative angle per the Aegisub example
    // "rotate the text 45 degrees in opposite direction on the Y axis".
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\fry-45}}flip-y\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert!(matches!(anim.tags[0], AnimatedTag::Fry(-45.0)));
    let st = anim.evaluate_at(500, 1000);
    assert!((st.rotate_y_radians + std::f32::consts::FRAC_PI_4).abs() < 1e-5);
    assert!(st.rotate_x_radians.abs() < 1e-6);
    assert!(st.rotate_radians.abs() < 1e-6);
}

#[test]
fn frx_fry_combined_independent_fields() {
    // `{\frx30\fry45}` — both axes set in the same override block. The
    // two axes are independent state fields on the RenderState; one
    // must not clobber the other.
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\frx30\\fry45}}xy\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert_eq!(anim.tags.len(), 2, "tags: {:?}", anim.tags);
    assert!(matches!(anim.tags[0], AnimatedTag::Frx(30.0)));
    assert!(matches!(anim.tags[1], AnimatedTag::Fry(45.0)));
    let st = anim.evaluate_at(0, 1000);
    assert!((st.rotate_x_radians - 30.0_f32.to_radians()).abs() < 1e-5);
    assert!((st.rotate_y_radians - 45.0_f32.to_radians()).abs() < 1e-5);
    // No Z-axis rotation: only the explicit axes are touched.
    assert!(st.rotate_radians.abs() < 1e-6);
}

#[test]
fn t_interpolates_frx() {
    // `{\t(0,1000,\frx90)}` — linear interpolation of the X-axis
    // rotation from 0 to π/2 over the cue. Same machinery the
    // existing `\frz` interpolation test exercises, applied to the
    // X-axis state field. Mid-cue the renderer should see π/4.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\t(0,1000,\\frx90)}}tilt-x\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert_eq!(anim.tags.len(), 1);
    let dur_ms = ((t.cues[0].end_us - t.cues[0].start_us) / 1000) as i32;
    let st0 = anim.evaluate_at(0, dur_ms);
    assert!(st0.rotate_x_radians.abs() < 1e-6);
    let st_mid = anim.evaluate_at(500, dur_ms);
    assert!((st_mid.rotate_x_radians - std::f32::consts::FRAC_PI_4).abs() < 1e-5);
    let st_end = anim.evaluate_at(1000, dur_ms);
    assert!((st_end.rotate_x_radians - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    // Y-axis and Z-axis untouched.
    assert!(st_end.rotate_y_radians.abs() < 1e-6);
    assert!(st_end.rotate_radians.abs() < 1e-6);
}

#[test]
fn t_interpolates_fry() {
    // Parallel of the `\frz` `\t` interpolation, on the Y axis. Mid-
    // cue the renderer should see 30° = π/6 (half of 60°).
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\t(0,2000,\\fry60)}}spin-y\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    let dur_ms = ((t.cues[0].end_us - t.cues[0].start_us) / 1000) as i32;
    let st0 = anim.evaluate_at(0, dur_ms);
    assert!(st0.rotate_y_radians.abs() < 1e-6);
    let st_mid = anim.evaluate_at(1000, dur_ms);
    assert!((st_mid.rotate_y_radians - 30.0_f32.to_radians()).abs() < 1e-5);
    let st_end = anim.evaluate_at(2000, dur_ms);
    assert!((st_end.rotate_y_radians - 60.0_f32.to_radians()).abs() < 1e-5);
    assert!(st_end.rotate_x_radians.abs() < 1e-6);
}

#[test]
fn t_interpolates_frx_and_fry_together() {
    // Both axes interpolated in the same `\t(...)` envelope. Each
    // axis carries its own pre / post snapshot through the lerp.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\t(0,1000,\\frx90\\fry-90)}}swivel\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    let dur_ms = ((t.cues[0].end_us - t.cues[0].start_us) / 1000) as i32;
    let st_mid = anim.evaluate_at(500, dur_ms);
    assert!((st_mid.rotate_x_radians - std::f32::consts::FRAC_PI_4).abs() < 1e-5);
    assert!((st_mid.rotate_y_radians + std::f32::consts::FRAC_PI_4).abs() < 1e-5);
}

#[test]
fn frx_fry_round_trip_preserves_raw_block() {
    // Textual round-trip: a `{\frx30\fry45}` block must re-emit
    // verbatim through `ass::write`, and re-parsing the output must
    // still produce the same typed AnimatedTag values. Parallel of
    // the `round_trip_preserves_animated_tags` test that pins \frz.
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\frx30\\fry45}}xy\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let out = String::from_utf8(ass::write(&t)).unwrap();
    for needle in ["\\frx30", "\\fry45", "xy"] {
        assert!(out.contains(needle), "missing {needle:?} in:\n{out}");
    }
    let t2 = ass::parse(out.as_bytes()).unwrap();
    let anim2 = extract_cue_animation(&t2.cues[0]);
    assert!(anim2
        .tags
        .iter()
        .any(|t| matches!(t, AnimatedTag::Frx(v) if (*v - 30.0).abs() < 1e-6)));
    assert!(anim2
        .tags
        .iter()
        .any(|t| matches!(t, AnimatedTag::Fry(v) if (*v - 45.0).abs() < 1e-6)));
}

#[test]
fn frx_fry_inside_t_round_trip() {
    // `\t(0,1000,\frx90)` should round-trip the `\t` envelope as-is
    // (the writer keeps animated tags via Segment::Raw).
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\t(0,1000,\\frx90\\fry-90)}}swivel\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let out = String::from_utf8(ass::write(&t)).unwrap();
    assert!(
        out.contains("\\t(0,1000,\\frx90\\fry-90)"),
        "writer output missing \\t envelope:\n{out}"
    );
}

#[test]
fn pbo_static_positive_offset() {
    // `\pbo100` — Aegisub example: draws 100 px below the specified
    // position. Surfaces on `RenderState::drawing_baseline_offset`.
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\pbo100}}shape\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert!(
        anim.tags.iter().any(|t| matches!(t, AnimatedTag::Pbo(100))),
        "missing Pbo(100): {:?}",
        anim.tags
    );
    let st = anim.evaluate_at(500, 1000);
    assert_eq!(st.drawing_baseline_offset, Some(100));
}

#[test]
fn pbo_static_negative_offset() {
    // `\pbo-50` — the Aegisub example for "above the specified
    // position" (negative Y).
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\pbo-50}}shape\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert!(
        anim.tags.iter().any(|t| matches!(t, AnimatedTag::Pbo(-50))),
        "missing Pbo(-50): {:?}",
        anim.tags
    );
    let st = anim.evaluate_at(500, 1000);
    assert_eq!(st.drawing_baseline_offset, Some(-50));
}

#[test]
fn pbo_zero_default_when_absent() {
    // Without a `\pbo` override the slot stays `None`, signalling that
    // the renderer should not translate drawing coordinates.
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\frz45}}rotated\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    let st = anim.evaluate_at(500, 1000);
    assert_eq!(st.drawing_baseline_offset, None);
}

#[test]
fn pbo_decimal_rounds_to_i32() {
    // Decimal payloads round to the nearest `i32` (mirrors `\be`'s
    // integer-strength rounding for floats from the wild).
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\pbo12.7}}shape\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert!(anim.tags.iter().any(|t| matches!(t, AnimatedTag::Pbo(13))));
}

#[test]
fn pbo_inside_t_interpolates_linearly() {
    // `\t(0,1000,\pbo100)` — ramp the drawing baseline offset from 0
    // (the pre-state default of "no override") to 100 over the cue.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\t(0,1000,\\pbo100)}}shape\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    let s_start = anim.evaluate_at(0, 1000);
    let s_mid = anim.evaluate_at(500, 1000);
    let s_end = anim.evaluate_at(1000, 1000);
    // At t == t1 the interpolation factor is 0, so the slot reflects
    // the pre-state baseline (no override) translated through the
    // post-state value of 100 — see apply_t's "fall back to pre"
    // behaviour for animatable post-only fields.
    assert_eq!(s_start.drawing_baseline_offset, Some(0));
    // Halfway: 50.
    assert_eq!(s_mid.drawing_baseline_offset, Some(50));
    // At t == t2 the post value snaps in.
    assert_eq!(s_end.drawing_baseline_offset, Some(100));
}

#[test]
fn pbo_round_trips_through_writer() {
    // `\pbo` is unknown to the base parser, so it must survive via
    // `Segment::Raw` and the writer must emit it back verbatim.
    let src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\pbo-25}}shape\n");
    let t = ass::parse(src.as_bytes()).unwrap();
    let out = String::from_utf8(ass::write(&t)).unwrap();
    assert!(out.contains("\\pbo-25"), "writer dropped \\pbo-25:\n{out}");
    // Re-parse should still surface the typed tag.
    let t2 = ass::parse(out.as_bytes()).unwrap();
    let anim2 = extract_cue_animation(&t2.cues[0]);
    assert!(anim2
        .tags
        .iter()
        .any(|t| matches!(t, AnimatedTag::Pbo(-25))));
}

#[test]
fn pbo_combined_with_p_drawing_mode() {
    // `\pbo` in the same override block as `\p1` (drawing mode on) —
    // the parser surfaces the Pbo tag from the raw passthrough and the
    // `\p1` toggle stays opaque; the round-trip keeps both.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\p1\\pbo20}}m 0 0 l 100 0 100 100 0 100\n"
    );
    let t = ass::parse(src.as_bytes()).unwrap();
    let anim = extract_cue_animation(&t.cues[0]);
    assert!(anim.tags.iter().any(|t| matches!(t, AnimatedTag::Pbo(20))));
    let out = String::from_utf8(ass::write(&t)).unwrap();
    assert!(out.contains("\\p1"), "writer dropped \\p1:\n{out}");
    assert!(out.contains("\\pbo20"), "writer dropped \\pbo20:\n{out}");
}

// Suppress an unused-import warning when only some helper types are used.
#[allow(dead_code)]
fn _ensure_cliprect_import_used(_: ClipRect) {}
