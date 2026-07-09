//! Extended hostile-input sweep (release-mode companion of
//! `tests/hostile.rs`).
//!
//! Runs the same xorshift mutation generator over the same seed
//! document for an arbitrary iteration count, checking parser
//! totality and the `serialise` re-parse fixpoint, and dumping any
//! failing input to `/tmp/oxideav-ass-fuzz-*.bin` for
//! `examples/hostile_minimize.rs`:
//!
//! ```sh
//! cargo run --release --example hostile_explore 5000000
//! ```
//!
//! The in-CI regression net is `tests/hostile.rs` (4000 seeded
//! inputs); this binary is for deeper local sweeps between rounds.

use std::panic;

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

const SEED: &str = "[Script Info]\n\
Title: fuzz seed\n\
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
                // splice a hostile token
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
                // duplicate a random slice
                let a = (rng.next() as usize) % v.len();
                let b = ((rng.next() as usize) % (v.len() - a)).min(64);
                let slice: Vec<u8> = v[a..a + b].to_vec();
                let i = (rng.next() as usize) % v.len();
                for (k, byte) in slice.into_iter().enumerate() {
                    v.insert(i + k, byte);
                }
            }
            _ => {
                // truncate
                let i = (rng.next() as usize) % v.len();
                v.truncate(i);
            }
        }
    }
    v
}

fn main() {
    panic::set_hook(Box::new(|_| {}));
    let mut rng = Rng(0x2545F4914F6CDD1D);
    let mut fails: Vec<(String, usize)> = Vec::new();
    let iters: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20000);
    for it in 0..iters {
        let input = mutate(&mut rng, SEED.as_bytes());
        let i2 = input.clone();
        let r = panic::catch_unwind(move || {
            let _ = oxideav_ass::parse(&i2);
        });
        if r.is_err() {
            fails.push((format!("parse it={it}"), it));
            std::fs::write(format!("/tmp/oxideav-ass-fuzz-parse-{it}.bin"), &input).ok();
        }
        let i3 = input.clone();
        let r = panic::catch_unwind(move || {
            let s = oxideav_ass::parse_script(&i3);
            let out = s.serialise();
            let s2 = oxideav_ass::parse_script(&out);
            assert_eq!(out, s2.serialise(), "serialise not a fixpoint");
        });
        if r.is_err() {
            fails.push((format!("script it={it}"), it));
            std::fs::write(format!("/tmp/oxideav-ass-fuzz-script-{it}.bin"), &input).ok();
        }
        // override + drawing parsers on the lossy-string view
        if let Ok(txt) = String::from_utf8(input.clone()) {
            let t2 = txt.clone();
            let r = panic::catch_unwind(move || {
                let mut tags = Vec::new();
                oxideav_ass::parse_overrides(&t2, &mut tags);
                let anim = oxideav_ass::CueAnimation { tags };
                let st = anim.evaluate_at(500, 1000);
                let _ = st;
            });
            if r.is_err() {
                fails.push((format!("overrides it={it}"), it));
                std::fs::write(format!("/tmp/oxideav-ass-fuzz-ovr-{it}.bin"), &input).ok();
            }
            let t3 = txt.clone();
            let r = panic::catch_unwind(move || {
                let _ = oxideav_ass::parse_drawing(&t3, 1);
                let _ = oxideav_ass::parse_drawing(&t3, 30);
            });
            if r.is_err() {
                fails.push((format!("drawing it={it}"), it));
                std::fs::write(format!("/tmp/oxideav-ass-fuzz-drw-{it}.bin"), &input).ok();
            }
        }
    }
    println!("iters={iters} fails={}", fails.len());
    for (what, _) in fails.iter().take(20) {
        println!("  {what}");
    }
}
