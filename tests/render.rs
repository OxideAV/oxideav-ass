//! Integration tests for the AnimatedRenderedDecoder + animated-tag
//! rendering wiring (#419 / #420).
//!
//! These tests use the bundled DejaVuSans TTF fixture from the
//! `oxideav-ttf` workspace crate. If the fixture is missing the test
//! returns early (as a soft skip) — the workspace always ships it, so
//! the soft-skip path is just defence in depth for downstream
//! standalone consumers.

#![cfg(feature = "render")]

use oxideav_ass as ass;
use oxideav_ass::AnimatedRenderedDecoder;
use oxideav_core::{CodecId, CodecParameters, Decoder, Frame, Packet, TimeBase};

const HEADER: &str = r"[Script Info]
ScriptType: v4.00+

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, Alignment, MarginL, MarginR, MarginV, Outline, Shadow
Style: Default,DejaVuSans,32,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,2,10,10,10,1,0

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
";

fn try_load_face_bytes() -> Option<Vec<u8>> {
    let candidates = [
        "../oxideav-ttf/tests/fixtures/DejaVuSans.ttf",
        "../../crates/oxideav-ttf/tests/fixtures/DejaVuSans.ttf",
    ];
    for p in candidates {
        if let Ok(b) = std::fs::read(p) {
            return Some(b);
        }
    }
    None
}

fn load_face() -> Option<oxideav_scribe::FaceChain> {
    let bytes = try_load_face_bytes()?;
    let face = oxideav_scribe::Face::from_ttf_bytes(bytes).ok()?;
    Some(oxideav_scribe::FaceChain::new(face))
}

fn build_decoder(ass_text: &str) -> Box<dyn Decoder> {
    // Parse the script, take cue 0, hand-build a packet for it, feed it
    // into a fresh ASS decoder.
    let track = ass::parse(ass_text.as_bytes()).expect("parse");
    assert!(!track.cues.is_empty());
    let cue = &track.cues[0];
    let line = ass::cue_to_bytes_pub(cue);
    let params = CodecParameters::subtitle(CodecId::new("ass"));
    let mut dec = ass::codec::make_decoder(&params).expect("make_decoder");
    let pkt = Packet::new(0, TimeBase::new(1, 1_000_000), line);
    dec.send_packet(&pkt).expect("send_packet");
    dec
}

/// Sum of alpha channel values across the frame — quick "how much ink
/// is on the canvas" metric.
fn alpha_mass(frame: &Frame) -> u64 {
    let vf = match frame {
        Frame::Video(v) => v,
        _ => return 0,
    };
    let plane = match vf.planes.first() {
        Some(p) => p,
        None => return 0,
    };
    plane.data.chunks_exact(4).map(|p| p[3] as u64).sum()
}

/// Bounding box of the pixels with alpha > 0. Returns
/// `(min_x, min_y, max_x, max_y)`.
fn alpha_bbox(frame: &Frame, width: u32) -> Option<(u32, u32, u32, u32)> {
    let vf = match frame {
        Frame::Video(v) => v,
        _ => return None,
    };
    let plane = vf.planes.first()?;
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut any = false;
    for (i, px) in plane.data.chunks_exact(4).enumerate() {
        if px[3] == 0 {
            continue;
        }
        let x = (i as u32) % width;
        let y = (i as u32) / width;
        if x < min_x {
            min_x = x;
        }
        if y < min_y {
            min_y = y;
        }
        if x > max_x {
            max_x = x;
        }
        if y > max_y {
            max_y = y;
        }
        any = true;
    }
    if any {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

#[test]
fn rendered_decoder_emits_different_frames_at_different_times() {
    let face = match load_face() {
        Some(f) => f,
        None => return,
    };
    // \fad(500, 500) on a 2-second cue. At t=0 alpha_mul = 0 → no
    // ink; at t=1000 (mid-cue) alpha_mul = 1 → full ink.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\fad(500,500)}}TEST\n"
    );
    let inner = build_decoder(&src);
    let mut dec = AnimatedRenderedDecoder::new(inner, 320, 120, face);

    // Frame at t=0 (fully transparent).
    dec.set_offset_ms(0);
    let f0 = dec.receive_frame().expect("frame at t=0");
    let mass_0 = alpha_mass(&f0);

    // Frame at t=1000 (fully opaque).
    dec.set_offset_ms(1000);
    let f1 = dec.receive_frame().expect("frame at t=1000");
    let mass_1 = alpha_mass(&f1);

    // Frame at t=1900 (fading out — should be partial).
    dec.set_offset_ms(1900);
    let f2 = dec.receive_frame().expect("frame at t=1900");
    let mass_2 = alpha_mass(&f2);

    assert!(mass_0 < mass_1, "fade-in: {mass_0} >= {mass_1}");
    assert!(mass_2 < mass_1, "fade-out: {mass_2} >= {mass_1}");
    assert!(mass_2 > 0, "fade-out partial should still be > 0");
}

#[test]
fn move_animates_position_across_time() {
    let face = match load_face() {
        Some(f) => f,
        None => return,
    };
    // \move from (60, 60) to (260, 60) over the cue's 1s lifetime.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\move(60,60,260,60)}}M\n"
    );
    let inner = build_decoder(&src);
    let mut dec = AnimatedRenderedDecoder::new(inner, 320, 120, face);

    dec.set_offset_ms(0);
    let bbox_start = alpha_bbox(&dec.receive_frame().expect("t=0"), 320).expect("ink at t=0");
    dec.set_offset_ms(1000);
    let bbox_end = alpha_bbox(&dec.receive_frame().expect("t=1000"), 320).expect("ink at t=1000");

    // Glyph horizontal centre should shift by roughly the move delta
    // (≈200 px). Allow some slack for kerning + glyph extent.
    let cx_start = (bbox_start.0 + bbox_start.2) as i32 / 2;
    let cx_end = (bbox_end.0 + bbox_end.2) as i32 / 2;
    assert!(
        (cx_end - cx_start) > 100,
        "expected move-induced shift; got start={cx_start} end={cx_end}"
    );
}

#[test]
fn clip_drawing_masks_rasterised_glyphs() {
    let face_a = match load_face() {
        Some(f) => f,
        None => return,
    };
    let face_b = match load_face() {
        Some(f) => f,
        None => return,
    };
    // \clip drawing — a triangle rectangle that covers only the
    // bottom-left quarter of the canvas. The unmasked baseline cue
    // puts ink in the bottom-centre band; with the clip, ink outside
    // x∈[0, 80) y∈[60, 120) must be suppressed.
    let unmasked_src =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,WIDETEXT\n");
    let masked_src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\clip(m 0 60 l 80 60 l 80 120 l 0 120 c)}}WIDETEXT\n"
    );

    let inner_u = build_decoder(&unmasked_src);
    let mut dec_u = AnimatedRenderedDecoder::new(inner_u, 320, 120, face_a);
    let f_u = dec_u.receive_frame().expect("unmasked");
    let mass_u = alpha_mass(&f_u);

    let inner_m = build_decoder(&masked_src);
    let mut dec_m = AnimatedRenderedDecoder::new(inner_m, 320, 120, face_b);
    let f_m = dec_m.receive_frame().expect("masked");
    let mass_m = alpha_mass(&f_m);

    assert!(mass_u > 0, "unmasked frame had no ink");
    assert!(
        mass_m < mass_u,
        "clip should reduce ink mass: masked={mass_m} unmasked={mass_u}"
    );
    let vf = match &f_m {
        Frame::Video(v) => v,
        _ => panic!(),
    };
    let plane = &vf.planes[0];
    let mut outside_lit = 0;
    for (i, px) in plane.data.chunks_exact(4).enumerate() {
        if px[3] == 0 {
            continue;
        }
        let x = (i as u32) % 320;
        let y = (i as u32) / 320;
        if !(x < 80 && (60..120).contains(&y)) {
            outside_lit += 1;
        }
    }
    // Allow a couple of edge-pixel leaks from AA roundoff.
    assert!(
        outside_lit < 8,
        "{outside_lit} lit pixels leaked outside the clip drawing rectangle"
    );
}

#[test]
fn frx_rotates_around_x_axis() {
    let face_a = match load_face() {
        Some(f) => f,
        None => return,
    };
    let face_b = match load_face() {
        Some(f) => f,
        None => return,
    };
    // Without rotation: text occupies its natural bbox. With \frx60 the
    // visible y-extent should compress (cos(60°) = 0.5).
    let no_rot = format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,STAIRS\n");
    let with_rot =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\frx60}}STAIRS\n");
    let inner_a = build_decoder(&no_rot);
    let mut dec_a = AnimatedRenderedDecoder::new(inner_a, 320, 200, face_a);
    let f_a = dec_a.receive_frame().expect("a");
    let bbox_a = alpha_bbox(&f_a, 320).expect("ink a");

    let inner_b = build_decoder(&with_rot);
    let mut dec_b = AnimatedRenderedDecoder::new(inner_b, 320, 200, face_b);
    let f_b = dec_b.receive_frame().expect("b");
    let bbox_b = alpha_bbox(&f_b, 320).expect("ink b");

    let h_a = (bbox_a.3 - bbox_a.1) as f32;
    let h_b = (bbox_b.3 - bbox_b.1) as f32;
    // Compressed height must be smaller. cos(60°) = 0.5 → expect
    // ≈half, but allow slack for shaper extent and AA.
    assert!(
        h_b < h_a,
        "expected rotated bbox height to compress: rotated={h_b} flat={h_a}"
    );
}

#[test]
fn org_changes_pivot() {
    let face_a = match load_face() {
        Some(f) => f,
        None => return,
    };
    let face_b = match load_face() {
        Some(f) => f,
        None => return,
    };
    // Two cues: one with \frz30 and the default pivot (alignment
    // point); the other with \frz30 and \org(80,80) (well above the
    // alignment point). 30° keeps both renders on-canvas; the rotated
    // bbox centre still shifts noticeably between the two pivots.
    let no_org =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\frz30}}OOO\n");
    let with_org = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\frz30\\org(80,80)}}OOO\n"
    );

    let inner_a = build_decoder(&no_org);
    let mut dec_a = AnimatedRenderedDecoder::new(inner_a, 320, 200, face_a);
    let f_a = dec_a.receive_frame().expect("no_org");
    let bbox_a = alpha_bbox(&f_a, 320).expect("ink no_org");

    let inner_b = build_decoder(&with_org);
    let mut dec_b = AnimatedRenderedDecoder::new(inner_b, 320, 200, face_b);
    let f_b = dec_b.receive_frame().expect("with_org");
    let bbox_b = alpha_bbox(&f_b, 320).expect("ink with_org");

    let cy_a = (bbox_a.1 + bbox_a.3) as i32 / 2;
    let cy_b = (bbox_b.1 + bbox_b.3) as i32 / 2;
    let cx_a = (bbox_a.0 + bbox_a.2) as i32 / 2;
    let cx_b = (bbox_b.0 + bbox_b.2) as i32 / 2;
    let dx = (cx_a - cx_b).abs();
    let dy = (cy_a - cy_b).abs();
    // The pivots are far apart (~150 px in y, ~140 px in x); the
    // rotated bbox centres must shift accordingly.
    assert!(
        dx + dy > 30,
        "expected \\org to displace the rotated bbox; got dx={dx} dy={dy}"
    );
}

#[test]
fn fax_shears_x_distortion_widens_bbox() {
    let face_a = match load_face() {
        Some(f) => f,
        None => return,
    };
    let face_b = match load_face() {
        Some(f) => f,
        None => return,
    };
    // Baseline text against a `\fax(0.7)` cue with the same content.
    // The Aegisub spec describes `\fax` as a horizontal shear: each
    // glyph row offset by `fax * y` from the anchor. Vertical glyph
    // extent is ≈ size_px (32 here), so the visible x-range should
    // widen by ~0.7 * 32 ≈ 22 px on top of the baseline width.
    let plain = format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,LEAN\n");
    let sheared =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\fax0.7}}LEAN\n");

    let inner_a = build_decoder(&plain);
    let mut dec_a = AnimatedRenderedDecoder::new(inner_a, 320, 200, face_a);
    let f_a = dec_a.receive_frame().expect("plain");
    let bbox_a = alpha_bbox(&f_a, 320).expect("ink plain");

    let inner_b = build_decoder(&sheared);
    let mut dec_b = AnimatedRenderedDecoder::new(inner_b, 320, 200, face_b);
    let f_b = dec_b.receive_frame().expect("sheared");
    let bbox_b = alpha_bbox(&f_b, 320).expect("ink sheared");

    let w_a = (bbox_a.2 - bbox_a.0) as i32;
    let w_b = (bbox_b.2 - bbox_b.0) as i32;
    assert!(
        w_b > w_a,
        "expected shear to widen the bbox: plain_w={w_a} sheared_w={w_b}"
    );
    // Vertical extent should be unchanged — a pure `\fax` shear only
    // displaces along x. Allow a couple of pixels of AA slack.
    let h_a = (bbox_a.3 - bbox_a.1) as i32;
    let h_b = (bbox_b.3 - bbox_b.1) as i32;
    assert!(
        (h_a - h_b).abs() <= 3,
        "\\fax should not change y-extent meaningfully: h_plain={h_a} h_sheared={h_b}"
    );
}

#[test]
fn fay_shears_y_distortion_extends_bbox() {
    let face_a = match load_face() {
        Some(f) => f,
        None => return,
    };
    let face_b = match load_face() {
        Some(f) => f,
        None => return,
    };
    // `\fay(0.4)` shears y by x. Two adjacent glyph columns sit at
    // different x relative to the anchor, so the rendered y-extent of
    // the line widens.
    let plain = format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,STRETCH\n");
    let sheared =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\fay0.4}}STRETCH\n");

    let inner_a = build_decoder(&plain);
    let mut dec_a = AnimatedRenderedDecoder::new(inner_a, 320, 200, face_a);
    let f_a = dec_a.receive_frame().expect("plain");
    let bbox_a = alpha_bbox(&f_a, 320).expect("ink plain");

    let inner_b = build_decoder(&sheared);
    let mut dec_b = AnimatedRenderedDecoder::new(inner_b, 320, 200, face_b);
    let f_b = dec_b.receive_frame().expect("sheared");
    let bbox_b = alpha_bbox(&f_b, 320).expect("ink sheared");

    let h_a = (bbox_a.3 - bbox_a.1) as i32;
    let h_b = (bbox_b.3 - bbox_b.1) as i32;
    assert!(
        h_b > h_a,
        "expected \\fay shear to widen the y-extent: plain_h={h_a} sheared_h={h_b}"
    );
}

#[test]
fn an_numpad_anchors_vertical_row() {
    // Three cues at \an2 (bottom-centre), \an5 (middle-centre), and
    // \an8 (top-centre). The rendered bbox vertical centre should
    // climb monotonically from bottom row → top row.
    let face_a = match load_face() {
        Some(f) => f,
        None => return,
    };
    let face_b = match load_face() {
        Some(f) => f,
        None => return,
    };
    let face_c = match load_face() {
        Some(f) => f,
        None => return,
    };
    let bottom =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\an2}}ANCHOR\n");
    let middle =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\an5}}ANCHOR\n");
    let top =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\an8}}ANCHOR\n");

    let h = 240u32;
    let inner_b = build_decoder(&bottom);
    let mut dec_b = AnimatedRenderedDecoder::new(inner_b, 320, h, face_a);
    let bbox_b = alpha_bbox(&dec_b.receive_frame().expect("an2"), 320).expect("ink an2");

    let inner_m = build_decoder(&middle);
    let mut dec_m = AnimatedRenderedDecoder::new(inner_m, 320, h, face_b);
    let bbox_m = alpha_bbox(&dec_m.receive_frame().expect("an5"), 320).expect("ink an5");

    let inner_t = build_decoder(&top);
    let mut dec_t = AnimatedRenderedDecoder::new(inner_t, 320, h, face_c);
    let bbox_t = alpha_bbox(&dec_t.receive_frame().expect("an8"), 320).expect("ink an8");

    let cy_b = (bbox_b.1 + bbox_b.3) as i32 / 2;
    let cy_m = (bbox_m.1 + bbox_m.3) as i32 / 2;
    let cy_t = (bbox_t.1 + bbox_t.3) as i32 / 2;
    // Bottom row sits in the lower third; top row in the upper third;
    // middle row near the half-line. The exact numbers depend on the
    // shaper metrics, so test by ordering + by canvas region.
    assert!(cy_t < cy_m, "expected an8 above an5: top={cy_t} mid={cy_m}");
    assert!(cy_m < cy_b, "expected an5 above an2: mid={cy_m} bot={cy_b}");
    let third = h as i32 / 3;
    assert!(
        cy_t < third,
        "an8 bbox centre y={cy_t} should sit in the top third (< {third})"
    );
    assert!(
        cy_b > 2 * third,
        "an2 bbox centre y={cy_b} should sit in the bottom third (> {})",
        2 * third
    );
    assert!(
        cy_m > third && cy_m < 2 * third,
        "an5 bbox centre y={cy_m} should sit in the middle third"
    );
}

#[test]
fn an_numpad_anchors_horizontal_column() {
    // \an1 (bottom-LEFT), \an2 (bottom-CENTRE), \an3 (bottom-RIGHT)
    // should each anchor the bbox centre on the appropriate canvas
    // column.
    let face_l = match load_face() {
        Some(f) => f,
        None => return,
    };
    let face_c = match load_face() {
        Some(f) => f,
        None => return,
    };
    let face_r = match load_face() {
        Some(f) => f,
        None => return,
    };
    let left = format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\an1}}EDGE\n");
    let centre =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\an2}}EDGE\n");
    let right =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\an3}}EDGE\n");

    let w = 320u32;
    let inner_l = build_decoder(&left);
    let mut dec_l = AnimatedRenderedDecoder::new(inner_l, w, 120, face_l);
    let bbox_l = alpha_bbox(&dec_l.receive_frame().expect("an1"), w).expect("ink an1");

    let inner_c = build_decoder(&centre);
    let mut dec_c = AnimatedRenderedDecoder::new(inner_c, w, 120, face_c);
    let bbox_c = alpha_bbox(&dec_c.receive_frame().expect("an2"), w).expect("ink an2");

    let inner_r = build_decoder(&right);
    let mut dec_r = AnimatedRenderedDecoder::new(inner_r, w, 120, face_r);
    let bbox_r = alpha_bbox(&dec_r.receive_frame().expect("an3"), w).expect("ink an3");

    let cx_l = (bbox_l.0 + bbox_l.2) as i32 / 2;
    let cx_c = (bbox_c.0 + bbox_c.2) as i32 / 2;
    let cx_r = (bbox_r.0 + bbox_r.2) as i32 / 2;
    assert!(
        cx_l < cx_c,
        "expected an1 left of an2: left={cx_l} centre={cx_c}"
    );
    assert!(
        cx_c < cx_r,
        "expected an2 left of an3: centre={cx_c} right={cx_r}"
    );
    let third = w as i32 / 3;
    assert!(
        cx_l < third,
        "an1 bbox centre x={cx_l} should sit in the left third"
    );
    assert!(
        cx_r > 2 * third,
        "an3 bbox centre x={cx_r} should sit in the right third"
    );
}

#[test]
fn empty_inner_yields_need_more() {
    let face = match load_face() {
        Some(f) => f,
        None => return,
    };
    // A decoder we never feed: receive_frame should return NeedMore.
    let params = CodecParameters::subtitle(CodecId::new("ass"));
    let inner = ass::codec::make_decoder(&params).expect("make_decoder");
    let mut dec = AnimatedRenderedDecoder::new(inner, 32, 16, face);
    assert!(matches!(
        dec.receive_frame(),
        Err(oxideav_core::Error::NeedMore)
    ));
}

#[test]
fn rendered_decoder_codec_id_matches_inner() {
    let face = match load_face() {
        Some(f) => f,
        None => return,
    };
    let params = CodecParameters::subtitle(CodecId::new("ass"));
    let inner = ass::codec::make_decoder(&params).expect("make_decoder");
    let dec = AnimatedRenderedDecoder::new(inner, 32, 16, face);
    assert_eq!(dec.codec_id().as_str(), "ass");
}

// `\1a` primary-fill alpha — per the override-tag reference the wire
// byte is `0 = opaque, 255 = transparent` and is independent of the
// cue-level `\fad` envelope. The renderer composes them
// multiplicatively, so a higher `\1a` value at a static (non-faded)
// cue must monotonically reduce the rasterised alpha mass; the
// reduction must be approximately linear in `255 - ass_a`.

fn render_first_frame(src: &str, w: u32, h: u32) -> Option<Frame> {
    let face = load_face()?;
    let inner = build_decoder(src);
    let mut dec = AnimatedRenderedDecoder::new(inner, w, h, face);
    dec.receive_frame().ok()
}

#[test]
fn primary_alpha_zero_emits_fully_opaque_ink() {
    if load_face().is_none() {
        return;
    }
    // `\1a&H00&` — primary fill explicitly opaque. Must produce the
    // same mass as the baseline (no `\1a`).
    let base = format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,SUB\n");
    let amped =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\1a&H00&}}SUB\n");
    let m_base = alpha_mass(&render_first_frame(&base, 320, 64).expect("render base"));
    let m_amped = alpha_mass(&render_first_frame(&amped, 320, 64).expect("render amped"));
    assert!(
        m_base > 0,
        "baseline render must produce ink (mass = {m_base})"
    );
    // Equal up to a small rasteriser-rounding tolerance.
    let lo = (m_base as i64 * 95) / 100;
    let hi = (m_base as i64 * 105) / 100;
    assert!(
        (m_amped as i64) >= lo && (m_amped as i64) <= hi,
        "\\1a&H00& must match baseline ink mass: base = {m_base}, amped = {m_amped}"
    );
}

#[test]
fn primary_alpha_half_yields_half_ink() {
    if load_face().is_none() {
        return;
    }
    let opaque = format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,SUB\n");
    let half =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\1a&H80&}}SUB\n");
    let m_opaque = alpha_mass(&render_first_frame(&opaque, 320, 64).expect("render opaque"));
    let m_half = alpha_mass(&render_first_frame(&half, 320, 64).expect("render half"));
    assert!(m_opaque > 0, "opaque render must produce ink");
    // `\1a&H80&` ≈ 50% transparent: rasterised mass should land near
    // half the opaque mass. Allow a generous 35..65% window — the
    // exact ratio depends on the compositor's alpha math + glyph edge
    // anti-aliasing.
    let ratio_pct = ((m_half as f64) * 100.0 / (m_opaque as f64)) as i64;
    assert!(
        (35..=65).contains(&ratio_pct),
        "\\1a&H80& should roughly halve ink mass: opaque = {m_opaque}, half = {m_half}, ratio = {ratio_pct}%"
    );
}

#[test]
fn primary_alpha_full_yields_no_ink() {
    if load_face().is_none() {
        return;
    }
    // `\1a&HFF&` — primary fill is fully transparent. Ink mass must
    // be zero.
    let invisible =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\1a&HFF&}}SUB\n");
    let m = alpha_mass(&render_first_frame(&invisible, 320, 64).expect("render invisible"));
    assert_eq!(m, 0, "\\1a&HFF& should produce no ink, got mass = {m}");
}

#[test]
fn primary_alpha_compounds_with_fad_envelope() {
    if load_face().is_none() {
        return;
    }
    // `\1a&H80&\fad(500, 500)` — 50% per-fill alpha multiplied by the
    // fade-in envelope. At t = 0 ms the envelope is 0 → no ink
    // regardless of `\1a`. The renderer must therefore emit an empty
    // frame, demonstrating that the two compose multiplicatively
    // rather than `\1a` overriding the envelope (or vice versa).
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{{\\1a&H80&\\fad(500,500)}}SUB\n"
    );
    let face = load_face().expect("face");
    let inner = build_decoder(&src);
    let mut dec = AnimatedRenderedDecoder::new(inner, 320, 64, face);
    let f0 = dec.receive_frame().expect("frame at t=0");
    assert_eq!(
        alpha_mass(&f0),
        0,
        "fade-in at t=0 must produce no ink even with \\1a&H80&"
    );
}

#[test]
fn blur_zero_matches_baseline_bbox() {
    if load_face().is_none() {
        return;
    }
    // `\blur0` is the "off" form of the Gaussian post-step per the
    // Aegisub spec ("Set strength to 0 (zero) to disable the
    // effect"). The renderer should therefore behave like the
    // baseline cue — the ink-extent bbox must match within a couple
    // of edge pixels (the renderer does no AA work either way, so
    // the bbox is genuinely identical up to glyph layout
    // determinism).
    let baseline = format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,BLUR\n");
    let zero =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\blur0}}BLUR\n");

    let f_base = render_first_frame(&baseline, 320, 120).expect("baseline");
    let f_zero = render_first_frame(&zero, 320, 120).expect("zero");
    let bbox_base = alpha_bbox(&f_base, 320).expect("baseline ink");
    let bbox_zero = alpha_bbox(&f_zero, 320).expect("zero ink");

    let dx0 = (bbox_zero.0 as i32 - bbox_base.0 as i32).abs();
    let dy0 = (bbox_zero.1 as i32 - bbox_base.1 as i32).abs();
    let dx1 = (bbox_zero.2 as i32 - bbox_base.2 as i32).abs();
    let dy1 = (bbox_zero.3 as i32 - bbox_base.3 as i32).abs();
    assert!(
        dx0 <= 2 && dy0 <= 2 && dx1 <= 2 && dy1 <= 2,
        "blur=0 changed the bbox: base={bbox_base:?} zero={bbox_zero:?}"
    );
}

#[test]
fn blur_widens_ink_bbox() {
    if load_face().is_none() {
        return;
    }
    // A nonzero Gaussian sigma softens the glyph edges so previously
    // empty pixels around the silhouette pick up some alpha. The
    // alpha bbox must therefore grow on at least one side compared
    // to the baseline. We use a generous sigma (3.0) and a 120-tall
    // canvas with side margins so the blurred edge has room to
    // spread without bumping the canvas border.
    let baseline = format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,BLUR\n");
    let blurred =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\blur3}}BLUR\n");

    let f_base = render_first_frame(&baseline, 320, 120).expect("baseline");
    let f_blur = render_first_frame(&blurred, 320, 120).expect("blurred");
    let bbox_base = alpha_bbox(&f_base, 320).expect("baseline ink");
    let bbox_blur = alpha_bbox(&f_blur, 320).expect("blurred ink");

    // Width / height — at least one axis must grow by 2+ pixels.
    let w_base = bbox_base.2 - bbox_base.0;
    let h_base = bbox_base.3 - bbox_base.1;
    let w_blur = bbox_blur.2 - bbox_blur.0;
    let h_blur = bbox_blur.3 - bbox_blur.1;
    assert!(
        w_blur >= w_base + 2 || h_blur >= h_base + 2,
        "blur did not widen the alpha bbox: base wh=({w_base},{h_base}) blur wh=({w_blur},{h_blur})"
    );
}

#[test]
fn blur_t_animation_grows_bbox_monotonically() {
    if load_face().is_none() {
        return;
    }
    // `\t(0, 1000, \blur6)` ramps the Gaussian strength linearly from
    // 0 to 6 across the cue's 1-second lifetime. Sampling at t = 0 /
    // 500 / 1000 ms therefore exercises three distinct sigmas
    // (0 → 3 → 6) — the alpha bbox should grow at every step rather
    // than collapsing back at any point.
    let src = format!(
        "{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\t(0,1000,\\blur6)}}BLUR\n"
    );
    let face = load_face().expect("face");
    let inner = build_decoder(&src);
    let mut dec = AnimatedRenderedDecoder::new(inner, 320, 120, face);

    dec.set_offset_ms(0);
    let f0 = dec.receive_frame().expect("t=0");
    let bbox0 = alpha_bbox(&f0, 320).expect("ink at t=0");

    dec.set_offset_ms(500);
    let f1 = dec.receive_frame().expect("t=500");
    let bbox1 = alpha_bbox(&f1, 320).expect("ink at t=500");

    dec.set_offset_ms(1000);
    let f2 = dec.receive_frame().expect("t=1000");
    let bbox2 = alpha_bbox(&f2, 320).expect("ink at t=1000");

    let w = |b: (u32, u32, u32, u32)| b.2.saturating_sub(b.0);
    let h = |b: (u32, u32, u32, u32)| b.3.saturating_sub(b.1);
    let area = |b: (u32, u32, u32, u32)| (w(b) as u64) * (h(b) as u64);
    let a0 = area(bbox0);
    let a1 = area(bbox1);
    let a2 = area(bbox2);
    assert!(
        a1 >= a0,
        "blur ramp t=0 → t=500 did not grow bbox area: a0={a0} a1={a1}"
    );
    assert!(
        a2 >= a1,
        "blur ramp t=500 → t=1000 did not grow bbox area: a1={a1} a2={a2}"
    );
    assert!(
        a2 > a0,
        "blur ramp t=0 → t=1000 did not net-grow bbox area: a0={a0} a2={a2}"
    );
}

#[test]
fn be_zero_matches_baseline_bbox() {
    if load_face().is_none() {
        return;
    }
    // `\be0` is the "off" form of the iterative box-blur post-step
    // per the Aegisub spec ("0 disables the effect"). The renderer
    // should therefore behave like the baseline cue — the ink-extent
    // bbox must match within a couple of edge pixels (the renderer
    // does no AA work either way, so the bbox is genuinely identical
    // up to glyph layout determinism).
    let baseline = format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,BE\n");
    let zero = format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\be0}}BE\n");

    let f_base = render_first_frame(&baseline, 320, 120).expect("baseline");
    let f_zero = render_first_frame(&zero, 320, 120).expect("zero");
    let bbox_base = alpha_bbox(&f_base, 320).expect("baseline ink");
    let bbox_zero = alpha_bbox(&f_zero, 320).expect("zero ink");

    let dx0 = (bbox_zero.0 as i32 - bbox_base.0 as i32).abs();
    let dy0 = (bbox_zero.1 as i32 - bbox_base.1 as i32).abs();
    let dx1 = (bbox_zero.2 as i32 - bbox_base.2 as i32).abs();
    let dy1 = (bbox_zero.3 as i32 - bbox_base.3 as i32).abs();
    assert!(
        dx0 <= 2 && dy0 <= 2 && dx1 <= 2 && dy1 <= 2,
        "\\be0 changed the bbox: base={bbox_base:?} zero={bbox_zero:?}"
    );
}

#[test]
fn be_widens_ink_bbox() {
    if load_face().is_none() {
        return;
    }
    // A positive `\be` strength spreads the glyph silhouette through
    // the alpha channel, so the alpha bbox must grow on at least one
    // axis compared to the baseline. We use a strength of 4
    // iterations (the 1-pixel-radius box has an N-pixel radius of
    // influence over N passes) and a 120-tall canvas with side
    // margins so the softened edges have room to spread without
    // bumping the canvas border.
    let baseline = format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,BE\n");
    let blurred =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\be4}}BE\n");

    let f_base = render_first_frame(&baseline, 320, 120).expect("baseline");
    let f_be = render_first_frame(&blurred, 320, 120).expect("blurred");
    let bbox_base = alpha_bbox(&f_base, 320).expect("baseline ink");
    let bbox_be = alpha_bbox(&f_be, 320).expect("blurred ink");

    // Width / height — at least one axis must grow by 2+ pixels.
    let w_base = bbox_base.2 - bbox_base.0;
    let h_base = bbox_base.3 - bbox_base.1;
    let w_be = bbox_be.2 - bbox_be.0;
    let h_be = bbox_be.3 - bbox_be.1;
    assert!(
        w_be >= w_base + 2 || h_be >= h_base + 2,
        "\\be did not widen the alpha bbox: base wh=({w_base},{h_base}) be wh=({w_be},{h_be})"
    );
}

#[test]
fn be_and_blur_compose_independently() {
    if load_face().is_none() {
        return;
    }
    // `\blur` (Gaussian) and `\be` (iterative box) sit on independent
    // channels of `RenderState`. When both are set the renderer must
    // run *both* post-steps; the resulting ink bbox should be at
    // least as wide as either filter alone (each one strictly grows
    // the alpha silhouette). The pin: `(blur=3, be=3)` produces a
    // bbox area no smaller than `(blur=3, be=0)`'s — a mild guard
    // against a future regression where one post-step overwrites the
    // other's working buffer at the wrong stride.
    let only_blur =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\blur3}}BLUR\n");
    let both =
        format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{\\blur3\\be3}}BLUR\n");

    let f_blur = render_first_frame(&only_blur, 320, 120).expect("blur-only");
    let f_both = render_first_frame(&both, 320, 120).expect("blur+be");
    let bbox_blur = alpha_bbox(&f_blur, 320).expect("blur-only ink");
    let bbox_both = alpha_bbox(&f_both, 320).expect("both ink");

    let area = |b: (u32, u32, u32, u32)| (b.2 - b.0) as u64 * (b.3 - b.1) as u64;
    let a_blur = area(bbox_blur);
    let a_both = area(bbox_both);
    assert!(
        a_both >= a_blur,
        "\\blur3\\be3 shrank vs \\blur3 alone: blur-only={a_blur} both={a_both}"
    );
}
