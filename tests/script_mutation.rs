//! Mutate-then-restore byte-identity on the structured document model,
//! plus SSA↔ASS dialect-conversion fidelity edges.
//!
//! The structured model's promise is *field-level* fidelity: editing a
//! field touches exactly that field's bytes, and undoing the edit
//! restores the document byte-for-byte. These tests pin that contract
//! and the dialect converter's originating-dialect restoration.

use oxideav_ass as ass;

const SCRIPT: &str = "[Script Info]\n\
; structured mutation test\n\
Title: Mutation\n\
ScriptType: v4.00+\n\
PlayResX: 640\n\
PlayResY: 480\n\
\n\
[Aegisub Project Garbage]\n\
Active Line: 2\n\
\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,2,1,2,10,10,10,1\n\
\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,{\\pos(320,240)}first line\n\
Dialogue: 1,0:00:02.00,0:00:04.00,Default,alice,10,10,20,Banner;30,second {\\i1}line{\\i0}\n";

fn style_mut(s: &mut ass::AssScript) -> &mut ass::StyleDef {
    for sec in s.sections.iter_mut() {
        if let ass::Section::Styles(t) = sec {
            return &mut t.styles[0];
        }
    }
    panic!("no style table");
}

fn event_mut(s: &mut ass::AssScript, idx: usize) -> &mut ass::Event {
    for sec in s.sections.iter_mut() {
        if let ass::Section::Events(t) = sec {
            return &mut t.events[idx];
        }
    }
    panic!("no event table");
}

#[test]
fn mutate_then_restore_is_byte_identical() {
    let mut s = ass::parse_script(SCRIPT.as_bytes());
    let original = s.serialise();
    assert_eq!(original, SCRIPT.as_bytes(), "baseline must be byte-stable");

    // Mutate a style column, an event column, the event text, an
    // info value and a raw-section body line.
    let old_size = std::mem::replace(&mut style_mut(&mut s).fontsize, "96".into());
    let old_text = std::mem::replace(&mut event_mut(&mut s, 0).text, "{\\pos(0,0)}edited".into());
    let old_margin = std::mem::replace(&mut event_mut(&mut s, 1).margin_v, "0150".into());
    let mut old_info = None;
    let mut old_raw = None;
    for sec in s.sections.iter_mut() {
        match sec {
            ass::Section::ScriptInfo(info) => {
                for line in info.lines.iter_mut() {
                    if let ass::InfoLine::Pair { key, value } = line {
                        if key == "Title" {
                            old_info = Some(std::mem::replace(value, "Changed".into()));
                        }
                    }
                }
            }
            ass::Section::Raw(r) if r.name == "Aegisub Project Garbage" => {
                old_raw = Some(std::mem::replace(&mut r.body[0], "Active Line: 9".into()));
            }
            _ => {}
        }
    }

    let mutated = s.serialise();
    assert_ne!(mutated, original);
    let mutated_txt = String::from_utf8(mutated).unwrap();
    assert!(mutated_txt.contains(",96,"));
    assert!(mutated_txt.contains("{\\pos(0,0)}edited"));
    assert!(mutated_txt.contains(",0150,"));
    assert!(mutated_txt.contains("Title: Changed"));
    assert!(mutated_txt.contains("Active Line: 9"));

    // Restore every field: the document must come back byte-for-byte.
    style_mut(&mut s).fontsize = old_size;
    event_mut(&mut s, 0).text = old_text;
    event_mut(&mut s, 1).margin_v = old_margin;
    for sec in s.sections.iter_mut() {
        match sec {
            ass::Section::ScriptInfo(info) => {
                for line in info.lines.iter_mut() {
                    if let ass::InfoLine::Pair { key, value } = line {
                        if key == "Title" {
                            *value = old_info.clone().unwrap();
                        }
                    }
                }
            }
            ass::Section::Raw(r) if r.name == "Aegisub Project Garbage" => {
                r.body[0] = old_raw.clone().unwrap();
            }
            _ => {}
        }
    }
    assert_eq!(s.serialise(), original);
}

#[test]
fn single_field_edit_touches_exactly_one_line() {
    let mut s = ass::parse_script(SCRIPT.as_bytes());
    event_mut(&mut s, 1).text = "replaced".into();
    let out = String::from_utf8(s.serialise()).unwrap();
    let before: Vec<&str> = SCRIPT.lines().collect();
    let after: Vec<&str> = out.lines().collect();
    assert_eq!(before.len(), after.len());
    let mut diffs = Vec::new();
    for (i, (a, b)) in before.iter().zip(after.iter()).enumerate() {
        if a != b {
            diffs.push(i);
        }
    }
    assert_eq!(diffs.len(), 1, "exactly one line must change: {diffs:?}");
    assert!(after[diffs[0]].ends_with(",Banner;30,replaced"));
}

#[test]
fn dialect_round_trip_ass_restores_bytes() {
    // ASS → SSA → ASS: the converter documents that a round-trip back
    // to the originating dialect restores dialect-specific columns;
    // on a byte-stable source that means full byte identity.
    let s = ass::parse_script(SCRIPT.as_bytes());
    let back = s.to_ssa().to_ass();
    assert_eq!(
        String::from_utf8(back.serialise()).unwrap(),
        SCRIPT,
        "ASS → SSA → ASS must restore the original bytes"
    );
}

#[test]
fn dialect_round_trip_ssa_restores_key_columns() {
    // A legacy SSA v4 script through ASS and back: the SSA-only
    // AlphaLevel column and the Marked event column must survive.
    let ssa: &str = "[Script Info]\n\
Title: legacy\n\
ScriptType: v4.00\n\
\n\
[V4 Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, AlphaLevel, Encoding\n\
Style: Default,Arial,20,16777215,255,0,0,-1,0,1,2,1,2,10,10,10,64,1\n\
\n\
[Events]\n\
Format: Marked, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: Marked=0,0:00:01.00,0:00:03.00,Default,,0,0,0,,legacy line\n";
    let s = ass::parse_script(ssa.as_bytes());
    assert_eq!(s.serialise(), ssa.as_bytes(), "SSA baseline byte-stable");
    let back = s.to_ass().to_ssa();
    let out = String::from_utf8(back.serialise()).unwrap();
    assert!(out.contains("[V4 Styles]"), "header restored: {out}");
    assert!(out.contains("AlphaLevel"), "AlphaLevel column kept: {out}");
    assert!(out.contains(",64,"), "AlphaLevel value kept: {out}");
    assert!(out.contains("Marked=0,"), "Marked column restored: {out}");
    assert!(out.contains("legacy line"), "text kept: {out}");
}

#[test]
fn dialect_conversion_maps_mid_and_top_alignment_rows() {
    // Legacy SSA codes: subtitles 1-3, toptitles +4 (5-7), midtitles
    // +8 (9-11). ASS numpad: bottom 1-3, mid 4-6, top 7-9. The
    // converter must keep the anchor stable through both schemes.
    for (ssa_code, numpad) in [
        ("1", "1"),
        ("2", "2"),
        ("3", "3"),
        ("5", "7"),
        ("6", "8"),
        ("7", "9"),
        ("9", "4"),
        ("10", "5"),
        ("11", "6"),
    ] {
        let ssa = format!(
            "[V4 Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, AlphaLevel, Encoding\n\
Style: A,Arial,20,0,0,0,0,0,0,1,2,1,{ssa_code},10,10,10,0,1\n"
        );
        let s = ass::parse_script(ssa.as_bytes());
        let ass_doc = String::from_utf8(s.to_ass().serialise()).unwrap();
        let style_line = ass_doc
            .lines()
            .find(|l| l.starts_with("Style:"))
            .unwrap()
            .to_string();
        let cols: Vec<&str> = style_line.trim_start_matches("Style:").split(',').collect();
        // ASS Format puts Alignment at index 18 of the 23-column set.
        assert_eq!(
            cols[18], numpad,
            "SSA {ssa_code} → numpad {numpad}, got line {style_line}"
        );
        // And back again.
        let restored = String::from_utf8(s.to_ass().to_ssa().serialise()).unwrap();
        let back_line = restored
            .lines()
            .find(|l| l.starts_with("Style:"))
            .unwrap()
            .to_string();
        let back_cols: Vec<&str> = back_line.trim_start_matches("Style:").split(',').collect();
        assert_eq!(
            back_cols[12], ssa_code,
            "numpad {numpad} → SSA {ssa_code}, got line {back_line}"
        );
    }
}
