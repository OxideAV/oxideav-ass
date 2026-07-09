//! Hostile-input suite: every parser entry point must be *total*
//! (no panic on any byte sequence) and `AssScript::serialise` must be
//! a fixpoint under re-parse, exactly as the crate README documents.
//!
//! Two layers:
//!
//! * Targeted regressions — each case here reproduced a real panic or
//!   fixpoint violation found by the mutation sweep below (the fix
//!   landed in the same commit as its test).
//! * A deterministic xorshift-mutation sweep over a rich seed script,
//!   driving `parse`, `parse_script` (+ fixpoint check),
//!   `parse_overrides` (+ evaluation) and `parse_drawing`. The PRNG
//!   is seeded with a constant so a CI failure replays locally.

/// Serialise must reproduce itself under re-parse.
fn assert_fixpoint(input: &[u8]) {
    let o1 = oxideav_ass::parse_script(input).serialise();
    let o2 = oxideav_ass::parse_script(&o1).serialise();
    assert_eq!(
        String::from_utf8_lossy(&o1),
        String::from_utf8_lossy(&o2),
        "serialise not a fixpoint for input {:?}",
        String::from_utf8_lossy(input)
    );
}

#[test]
fn empty_bracket_pair_is_not_a_section_header() {
    // `[]` used to open an empty-named RawSection which serialised to
    // *nothing* — the two header bytes could never round-trip. It now
    // flows through as an ordinary body line.
    assert_fixpoint(b"[]");
    assert_fixpoint(b"[V4 Styles]\n[]\n");
    assert_fixpoint(b"[Script Info]\n[]\n:");
    assert_fixpoint(b"[Events]\n[]\n");
    // A whitespace-only name is still a header and still round-trips.
    assert_fixpoint(b"[ ]\nbody\n");
}

#[test]
fn duplicate_format_columns_dedupe_stably() {
    // A duplicated `Text` column emitted the text once per column
    // while the parser folded everything from the first slot back
    // into one value — each round-trip grew the line.
    assert_fixpoint(b"[Events]\nFormat:Text,Text\nDialogue:,,");
    assert_fixpoint(b"[Events]\nFormat: Layer, Text, Layer, Text\nDialogue: 0,a,b\n");
    assert_fixpoint(b"[V4+ Styles]\nFormat: Name, Name\nStyle: A,B\n");
    // Dedupe keeps the first occurrence.
    let s = oxideav_ass::parse_script(b"[Events]\nFormat: Layer, Text, Text\nDialogue: 1,,x\n");
    let out = String::from_utf8(s.serialise()).unwrap();
    assert!(
        out.contains("Format: Layer, Text\n"),
        "expected deduped format in {out:?}"
    );
}

#[test]
fn second_format_line_does_not_reshape_parsed_rows() {
    // A later `Format:` line used to replace the table's column set
    // after rows were already parsed under the first one, so the rows
    // re-serialised with a different column count. The first Format
    // wins (the spec places Format first in the section).
    assert_fixpoint(b"[Events]\nFormat:Name\nDialogue:,\nFormat:Name,");
    assert_fixpoint(b"[V4 Styles]\nFormat:Name\nStyle:,\nFormat:Name,");
    assert_fixpoint(b"[Events]\nFormat:Start\nDialogue:,\nFormat:L,Start,");
    let s = oxideav_ass::parse_script(
        b"[Events]\nFormat: Layer, Text\nDialogue: 1,x\nFormat: Text\nDialogue: 2,y\n",
    );
    let out = String::from_utf8(s.serialise()).unwrap();
    assert!(
        out.contains("Format: Layer, Text\n"),
        "first Format must win in {out:?}"
    );
}

#[test]
fn trailing_newline_does_not_accumulate() {
    // The phantom final `""` segment of a `\n`-terminated document
    // used to land in the last section's body as a real blank line,
    // growing the output by one `\n` per round-trip.
    assert_fixpoint(b"");
    assert_fixpoint(b"[X]\nbody\n");
    assert_fixpoint(b"[X]\nbody");
    assert_fixpoint(b"[X]\nbody\n\n");
    assert_fixpoint(b"[Script Info]\nTitle: t\n");
    assert_fixpoint(b"[X]\na\n\n[Y]\nb\n");
    // Well-formed `\n`-terminated documents serialise byte-identical
    // on the FIRST pass, not just at the fixpoint.
    let src: &[u8] = b"[Script Info]\nTitle: t\n\n[X]\nbody\n";
    assert_eq!(oxideav_ass::parse_script(src).serialise(), src);
}

#[test]
fn hostile_structural_fragments_are_total() {
    let cases: &[&[u8]] = &[
        b"[",
        b"]",
        b"[Script Info",
        b"Dialogue: ",
        b"[Events]\nFormat:\nDialogue: a",
        b"[Events]\nDialogue: 0,0:00:01.00,0:00:02.00,S,,0,0,0,,x", // no Format
        b"[Events]\nFormat: ,,,,\nDialogue: ,,,,",
        b"\xEF\xBB\xBF[Script Info]\nTitle: bom\n",
        b"\0\0\0\0",
        b"[Script Info]\r\nTitle: crlf\r\n\r\n[Events]\r\n",
        b"[Fonts]\nfontname: a_0\n!garbage!!\n",
        b"{\\pos(1",
        b"[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,S,,0,0,0,,{\\t(0,100,{\\clip(m",
    ];
    for c in cases {
        let _ = oxideav_ass::parse(c);
        assert_fixpoint(c);
    }
}

#[test]
fn hostile_override_and_drawing_fragments_are_total() {
    let cases = [
        "\\t(",
        "\\t(((((",
        "\\clip(",
        "\\pos(99999999999999999999,1e999)",
        "\\move(1,2,3,4,5,6,7,8,9,10)",
        "\\fad(-1,-1)\\fade(1,2)",
        "\\k-999999\\kf1e30\\ko0.0001",
        "\\1c&H&\\alpha&HZZ&",
        "\\fn\\fs\\fr\\b\\i\\u\\s\\r",
        "\\p-1\\pbo99999999999",
    ];
    for c in cases {
        let mut tags = Vec::new();
        oxideav_ass::parse_overrides(c, &mut tags);
        let anim = oxideav_ass::CueAnimation { tags };
        let st = anim.evaluate_at(500, 1000);
        assert!(st.alpha_mul.is_finite(), "non-finite alpha for {c:?}");
        assert!(
            st.scale.0.is_finite() && st.scale.1.is_finite(),
            "non-finite scale for {c:?}"
        );
    }
    for c in [
        "m",
        "m 1",
        "m 1 2 l",
        "b 1 2 3",
        "m 99999999999999999999999999 1e999 l nan inf",
        "s 1 2 3 4 5 6 c c c",
        "n 5 5 p 1 1 p 2 2",
    ] {
        let _ = oxideav_ass::parse_drawing(c, 1);
        let _ = oxideav_ass::parse_drawing(c, 30);
    }
}

// ---------------------------------------------------------------------------
// Deterministic mutation sweep

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

const SEED_DOC: &str = "[Script Info]\n\
Title: mutation seed\n\
ScriptType: v4.00+\n\
PlayResX: 640\n\
PlayResY: 480\n\
WrapStyle: 0\n\
Collisions: Normal\n\
Timer: 100.0000\n\
\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,1,2,10,10,10,1\n\
\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:05.00,Default,,0,0,0,,{\\pos(320,240)\\t(0,1000,2,\\frz360\\clip(0,0,100,100))\\fad(200,300)}Hello\n\
Dialogue: 1,0:00:02.00,0:00:06.00,Default,name,10,10,20,Scroll up;0;100;5,{\\move(0,0,100,100,0,500)\\k20\\kf30\\ko40}wor{\\p1}m 0 0 l 10 0 10 10 b 1 2 3 4 5 6 s 7 8 9 10 c{\\p0}ld\n\
Comment: 0,0:00:03.00,0:00:04.00,Default,,0,0,0,,note\n";

fn mutate(rng: &mut Rng, base: &[u8]) -> Vec<u8> {
    let mut v = base.to_vec();
    let n = 1 + (rng.next() % 16) as usize;
    for _ in 0..n {
        if v.is_empty() {
            break;
        }
        match rng.next() % 6 {
            0 => {
                let i = (rng.next() as usize) % v.len();
                v[i] = (rng.next() & 0xff) as u8;
            }
            1 => {
                let i = (rng.next() as usize) % v.len();
                v.remove(i);
            }
            2 => {
                let i = (rng.next() as usize) % v.len();
                v.insert(i, (rng.next() & 0xff) as u8);
            }
            3 => {
                let toks: [&[u8]; 12] = [
                    b"{\\t(",
                    b"\\clip(",
                    b"9999999999",
                    b"nan",
                    b"1e999",
                    b"&H",
                    b"\xc3",
                    b"\0",
                    b"\\p1",
                    b"m ",
                    b"[Events]",
                    b",,,,,,,,,",
                ];
                let t = toks[(rng.next() as usize) % toks.len()];
                let i = (rng.next() as usize) % v.len();
                for (k, &b) in t.iter().enumerate() {
                    v.insert(i + k, b);
                }
            }
            4 => {
                let a = (rng.next() as usize) % v.len();
                let b = ((rng.next() as usize) % (v.len() - a)).min(64);
                let slice: Vec<u8> = v[a..a + b].to_vec();
                let i = (rng.next() as usize) % v.len();
                for (k, byte) in slice.into_iter().enumerate() {
                    v.insert(i + k, byte);
                }
            }
            _ => {
                let i = (rng.next() as usize) % v.len();
                v.truncate(i);
            }
        }
    }
    v
}

#[test]
fn mutation_sweep_parsers_total_and_serialise_fixpoint() {
    // Fixed seed → identical corpus on every run; a failure here
    // replays locally byte-for-byte. The release-mode exploration run
    // of the same generator covered 2 000 000+ inputs; this in-CI
    // sweep keeps a representative regression net cheap enough for
    // the debug profile.
    let mut rng = Rng(0x2545F4914F6CDD1D);
    for _ in 0..4000 {
        let input = mutate(&mut rng, SEED_DOC.as_bytes());
        // Track parse: must be total.
        let _ = oxideav_ass::parse(&input);
        // Structured parse: total + serialise fixpoint.
        let o1 = oxideav_ass::parse_script(&input).serialise();
        let o2 = oxideav_ass::parse_script(&o1).serialise();
        assert_eq!(
            String::from_utf8_lossy(&o1),
            String::from_utf8_lossy(&o2),
            "fixpoint violated for {:?}",
            String::from_utf8_lossy(&input)
        );
        // Override + drawing parsers on the lossy text view: total,
        // and the sampled state stays finite.
        let txt = String::from_utf8_lossy(&input);
        let mut tags = Vec::new();
        oxideav_ass::parse_overrides(&txt, &mut tags);
        let st = oxideav_ass::CueAnimation { tags }.evaluate_at(500, 1000);
        assert!(st.alpha_mul.is_finite() && st.scale.0.is_finite());
        let _ = oxideav_ass::parse_drawing(&txt, 1);
    }
}
