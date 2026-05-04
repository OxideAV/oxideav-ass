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
