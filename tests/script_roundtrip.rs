//! Integration coverage for the structured ASS/SSA document model
//! (`parse_script` / `AssScript::serialise` / `AssScript::to_track`).
//!
//! These exercise the public surface end-to-end on a realistic script
//! carrying every modelled section plus editor-private + UU-encoded
//! blocks, proving the structured parse → serialise → re-parse path is a
//! fixpoint and that the IR projection matches the base `parse`.

use oxideav_ass as ass;
use oxideav_ass::{EventKind, Section};

const SCRIPT: &str = "\u{feff}[Script Info]\n\
; Demo script for the structured round-trip test\n\
Title: Round Trip\n\
ScriptType: v4.00+\n\
WrapStyle: 0\n\
ScaledBorderAndShadow: yes\n\
PlayResX: 1920\n\
PlayResY: 1080\n\
\n\
[Aegisub Project Garbage]\n\
Last Style Storage: Default\n\
Scroll Position: 3\n\
Active Line: 1\n\
\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,Arial,54,&H00FFFFFF,&H000000FF,&H00000000,&H64000000,0,0,0,0,100,100,0,0,1,2,1,2,60,60,40,1\n\
Style: Sign,Times New Roman,40,&H00FFD700,&H000000FF,&H00202020,&H00000000,-1,0,0,0,110,110,3,15,3,4,0,7,30,30,30,0\n\
\n\
[Fonts]\n\
fontname: demo_B0.ttf\n\
M0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ=\n\
\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:03.50,Default,Narrator,0,0,0,,{\\b1}Welcome,{\\b0} everyone\n\
Comment: 0,0:00:03.50,0:00:04.00,Default,,0,0,0,,a note for the editor\n\
Dialogue: 2,0:00:05.00,0:00:09.00,Sign,,120,120,80,Banner;30,{\\pos(960,120)\\3c&H00FF00&\\fad(200,200)}On screen sign, with comma\n";

#[test]
fn structured_parse_reads_every_section() {
    let s = ass::parse_script(SCRIPT.as_bytes());
    // Script Info, two raw sections (Aegisub + Fonts), styles, events.
    assert!(s.script_info().is_some());
    assert!(s.style_table().is_some());
    assert!(s.event_table().is_some());
    assert_eq!(s.styles().len(), 2);
    assert_eq!(s.events().len(), 3);

    let raw_names: Vec<&str> = s
        .sections
        .iter()
        .filter_map(|sec| match sec {
            Section::Raw(r) => Some(r.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(raw_names, vec!["Aegisub Project Garbage", "Fonts"]);
}

#[test]
fn structured_serialise_is_a_reparse_fixpoint() {
    let s1 = ass::parse_script(SCRIPT.as_bytes());
    let bytes = s1.serialise();
    let s2 = ass::parse_script(&bytes);
    // A second serialise of the re-parsed model is byte-identical to the
    // first serialise — the canonical form is stable.
    assert_eq!(
        bytes,
        s2.serialise(),
        "serialise is not a fixpoint:\n{}",
        String::from_utf8_lossy(&bytes)
    );
    // And the structured models agree.
    assert_eq!(s1, s2);
}

#[test]
fn structured_serialise_preserves_every_field() {
    let s = ass::parse_script(SCRIPT.as_bytes());
    let out = String::from_utf8(s.serialise()).unwrap();
    for needle in [
        "Title: Round Trip",
        "ScaledBorderAndShadow: yes",
        "[Aegisub Project Garbage]",
        "Last Style Storage: Default",
        "[Fonts]",
        "fontname: demo_B0.ttf",
        "M0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ=",
        "Style: Sign,Times New Roman,40",
        "Banner;30",
        // The comma-bearing text + override blocks survive whole.
        "On screen sign, with comma",
        "{\\pos(960,120)\\3c&H00FF00&\\fad(200,200)}",
    ] {
        assert!(out.contains(needle), "lost `{needle}`:\n{out}");
    }
    // The Comment event keeps its descriptor.
    assert!(out.contains("Comment: 0,0:00:03.50"));
}

#[test]
fn structured_typed_accessors() {
    let s = ass::parse_script(SCRIPT.as_bytes());
    let events = s.events();

    // Layer integers.
    assert_eq!(events[0].layer_typed().resolve(), 0);
    assert_eq!(events[2].layer_typed().resolve(), 2);

    // Per-event margins on the sign.
    let (l, r, v) = events[2].margins_typed();
    assert_eq!(l.as_pixels(), Some(120));
    assert_eq!(r.as_pixels(), Some(120));
    assert_eq!(v.as_pixels(), Some(80));

    // Effect column → Banner.
    assert!(matches!(
        events[2].effect_typed(),
        ass::EventEffect::Banner { .. }
    ));

    // Override-tag stream on the sign event.
    let tags = events[2].override_tags();
    assert!(tags
        .iter()
        .any(|t| matches!(t, ass::AnimatedTag::Pos { .. })));
    assert!(tags
        .iter()
        .any(|t| matches!(t, ass::AnimatedTag::Fad { .. })));

    // Style typed columns: the Sign is an opaque box with a non-ANSI
    // encoding slot.
    let sign = s.styles().into_iter().find(|s| s.name == "Sign").unwrap();
    assert!(sign.border_style_typed().is_opaque_box());
    assert_eq!(sign.encoding_typed().as_code(), 0);
}

#[test]
fn to_track_matches_base_parser_cue_stream() {
    let structured = ass::parse_script(SCRIPT.as_bytes()).to_track();
    let base = ass::parse(SCRIPT.as_bytes()).unwrap();
    // Both projections yield the same dialogue-only cue stream.
    assert_eq!(structured.cues.len(), base.cues.len());
    assert_eq!(structured.cues.len(), 2);
    for (a, b) in structured.cues.iter().zip(base.cues.iter()) {
        assert_eq!(a.start_us, b.start_us);
        assert_eq!(a.end_us, b.end_us);
        assert_eq!(a.style_ref, b.style_ref);
    }
    // Same style names.
    let mut s_names: Vec<&str> = structured.styles.iter().map(|s| s.name.as_str()).collect();
    let mut b_names: Vec<&str> = base.styles.iter().map(|s| s.name.as_str()).collect();
    s_names.sort_unstable();
    b_names.sort_unstable();
    assert_eq!(s_names, b_names);
}

#[test]
fn event_kind_descriptors_round_trip() {
    for kind in [
        EventKind::Dialogue,
        EventKind::Comment,
        EventKind::Picture,
        EventKind::Sound,
        EventKind::Movie,
        EventKind::Command,
    ] {
        let d = kind.descriptor();
        assert_eq!(EventKind::from_descriptor(d), Some(kind));
        // Case-insensitive.
        assert_eq!(
            EventKind::from_descriptor(&d.to_lowercase()),
            Some(kind),
            "lowercase {d} should parse"
        );
    }
    assert_eq!(EventKind::from_descriptor("Picture:"), None); // colon not stripped here
    assert_eq!(EventKind::from_descriptor("nonsense"), None);
}
