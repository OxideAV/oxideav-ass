//! ASS drawing-mode parser.
//!
//! Inside `\p<scale>` blocks (and inside `\clip(drawing)`) ASS uses a
//! tiny mini-language to describe vector paths:
//!
//! | Cmd | Args                  | Meaning                                   |
//! |-----|-----------------------|-------------------------------------------|
//! | `m` | `x y`                 | move to absolute (close any open subpath) |
//! | `n` | `x y`                 | move to absolute (no close)               |
//! | `l` | `x y` (1 or more)     | line to (each pair = one segment)         |
//! | `b` | `x1 y1 x2 y2 x3 y3` … | cubic bezier (triplets of points)         |
//! | `s` | `x1 y1 x2 y2 x3 y3` … | extended cubic spline (cubic-to)          |
//! | `p` | `x y`                 | extend cubic spline (line-to)             |
//! | `c` | —                     | close current subpath                     |
//!
//! Drawings are scaled down by `2^(scale - 1)` (`\p2` = ÷2). The scale
//! exponent defaults to 1 (no scaling); `\clip(drawing)` accepts an
//! optional leading scale digit (`\clip(2, drawing)`).
//!
//! Tokens are separated by whitespace; coordinates may be integer or
//! decimal. Unknown commands are silently skipped — matching the
//! permissive behaviour of mainstream ASS renderers.

use oxideav_core::{Path, Point};

/// Parse an ASS drawing-mode string into a [`Path`] in user units.
///
/// `scale_exp` is the `\p<n>` scale exponent; coordinates are divided
/// by `2^(scale_exp - 1)`. Pass `1` for no scaling.
pub fn parse_drawing(s: &str, scale_exp: u32) -> Path {
    let mut path = Path::new();
    let div = if scale_exp == 0 {
        1.0
    } else {
        (1u32 << (scale_exp - 1)) as f32
    };
    let scale = if div == 0.0 { 1.0 } else { 1.0 / div };

    let tokens: Vec<&str> = s.split_whitespace().collect();
    let mut i = 0;
    let mut cur = Point::new(0.0, 0.0);
    let mut last_cmd: Option<char> = None;
    while i < tokens.len() {
        let head = tokens[i];
        let mut bytes = head.bytes();
        let first = match bytes.next() {
            Some(b) => b,
            None => {
                i += 1;
                continue;
            }
        };
        // A command character is a single ASCII letter token.
        if head.len() == 1 && first.is_ascii_alphabetic() {
            let cmd = (first as char).to_ascii_lowercase();
            i += 1;
            match cmd {
                'm' | 'n' => {
                    if let Some((p, ni)) = read_point(&tokens, i, scale) {
                        path.move_to(p);
                        cur = p;
                        i = ni;
                        last_cmd = Some(cmd);
                    }
                }
                'l' => {
                    while let Some((p, ni)) = read_point(&tokens, i, scale) {
                        path.line_to(p);
                        cur = p;
                        i = ni;
                    }
                    last_cmd = Some('l');
                }
                'b' => {
                    while let Some((p1, p2, p3, ni)) = read_three_points(&tokens, i, scale) {
                        path.cubic_to(p1, p2, p3);
                        cur = p3;
                        i = ni;
                    }
                    last_cmd = Some('b');
                }
                's' => {
                    // Extended cubic spline — at minimum three control
                    // points then any number of additional points (each
                    // forms another cubic with implicit smoothness).
                    // We approximate by chaining cubics on consecutive
                    // triplets; visually adequate for clip masks.
                    while let Some((p1, p2, p3, ni)) = read_three_points(&tokens, i, scale) {
                        path.cubic_to(p1, p2, p3);
                        cur = p3;
                        i = ni;
                    }
                    last_cmd = Some('s');
                }
                'p' => {
                    while let Some((p, ni)) = read_point(&tokens, i, scale) {
                        path.line_to(p);
                        cur = p;
                        i = ni;
                    }
                    last_cmd = Some('p');
                }
                'c' => {
                    path.close();
                    last_cmd = Some('c');
                }
                _ => {
                    // Skip unknown command and any contiguous numeric
                    // tail until the next command letter.
                    while i < tokens.len()
                        && !(tokens[i].len() == 1 && tokens[i].as_bytes()[0].is_ascii_alphabetic())
                    {
                        i += 1;
                    }
                }
            }
        } else {
            // Bare numeric token without a leading command — treat as
            // a continuation of the previous command. ASS allows
            // implicit repetition, e.g. `m 0 0 100 0 100 100` after a
            // `l` command continues line-to'ing.
            match last_cmd {
                Some('m') | Some('n') | Some('l') | Some('p') => {
                    if let Some((p, ni)) = read_point(&tokens, i, scale) {
                        if last_cmd == Some('m') || last_cmd == Some('n') {
                            // Subsequent numeric pairs after `m`/`n`
                            // become implicit line-to (matches the
                            // common ASS-renderer behaviour).
                            path.line_to(p);
                            last_cmd = Some('l');
                        } else {
                            path.line_to(p);
                        }
                        cur = p;
                        i = ni;
                    } else {
                        i += 1;
                    }
                }
                Some('b') | Some('s') => {
                    if let Some((p1, p2, p3, ni)) = read_three_points(&tokens, i, scale) {
                        path.cubic_to(p1, p2, p3);
                        cur = p3;
                        i = ni;
                    } else {
                        i += 1;
                    }
                }
                _ => i += 1,
            }
        }
    }
    let _ = cur;
    path
}

fn read_point(tokens: &[&str], i: usize, scale: f32) -> Option<(Point, usize)> {
    if i + 1 >= tokens.len() {
        return None;
    }
    if !is_number(tokens[i]) || !is_number(tokens[i + 1]) {
        return None;
    }
    let x: f32 = tokens[i].parse().ok()?;
    let y: f32 = tokens[i + 1].parse().ok()?;
    Some((Point::new(x * scale, y * scale), i + 2))
}

fn read_three_points(
    tokens: &[&str],
    i: usize,
    scale: f32,
) -> Option<(Point, Point, Point, usize)> {
    let (p1, i1) = read_point(tokens, i, scale)?;
    let (p2, i2) = read_point(tokens, i1, scale)?;
    let (p3, i3) = read_point(tokens, i2, scale)?;
    Some((p1, p2, p3, i3))
}

fn is_number(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    // A "number" here is anything that parses as f32. Single-letter
    // command tokens are filtered upstream by callers checking the
    // token's first byte.
    t.parse::<f32>().is_ok()
}

/// Try to recognise `\clip(drawing)` arguments of the form
/// `[scale_exp,] drawing_str`. Returns the scale exponent (default 1)
/// and the drawing-mode body.
pub fn split_clip_arg(arg: &str) -> (u32, &str) {
    let trimmed = arg.trim_start();
    // The leading scale is a single integer followed by a comma.
    if let Some(comma) = trimmed.find(',') {
        let head = trimmed[..comma].trim();
        if !head.is_empty() && head.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = head.parse::<u32>() {
                return (n.max(1), trimmed[comma + 1..].trim_start());
            }
        }
    }
    (1, trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxideav_core::PathCommand;

    #[test]
    fn parses_simple_rectangle() {
        let p = parse_drawing("m 0 0 l 100 0 l 100 50 l 0 50 c", 1);
        assert_eq!(p.commands.len(), 5);
        assert!(matches!(p.commands[0], PathCommand::MoveTo(_)));
        assert!(matches!(p.commands[1], PathCommand::LineTo(_)));
        assert!(matches!(p.commands[4], PathCommand::Close));
    }

    #[test]
    fn parses_cubic() {
        let p = parse_drawing("m 0 0 b 10 0 20 10 30 30", 1);
        assert_eq!(p.commands.len(), 2);
        assert!(matches!(p.commands[1], PathCommand::CubicCurveTo { .. }));
    }

    #[test]
    fn scale_divides_coordinates() {
        // \p2 → divide by 2.
        let p = parse_drawing("m 0 0 l 200 0", 2);
        match p.commands[1] {
            PathCommand::LineTo(pt) => {
                assert!((pt.x - 100.0).abs() < 1e-6);
                assert!((pt.y - 0.0).abs() < 1e-6);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn implicit_line_to_after_move() {
        let p = parse_drawing("m 0 0 100 0 100 50", 1);
        // m + 2 implicit line-to.
        assert_eq!(p.commands.len(), 3);
        assert!(matches!(p.commands[0], PathCommand::MoveTo(_)));
        assert!(matches!(p.commands[1], PathCommand::LineTo(_)));
        assert!(matches!(p.commands[2], PathCommand::LineTo(_)));
    }

    #[test]
    fn split_clip_arg_no_scale() {
        let (s, body) = split_clip_arg("m 0 0 l 100 0");
        assert_eq!(s, 1);
        assert_eq!(body, "m 0 0 l 100 0");
    }

    #[test]
    fn split_clip_arg_with_scale() {
        let (s, body) = split_clip_arg("2, m 0 0 l 200 0");
        assert_eq!(s, 2);
        assert_eq!(body, "m 0 0 l 200 0");
    }

    #[test]
    fn unknown_command_skipped() {
        // `q` isn't a recognised drawing command.
        let p = parse_drawing("m 0 0 q 50 50 l 100 100", 1);
        assert!(p
            .commands
            .iter()
            .any(|c| matches!(c, PathCommand::MoveTo(_))));
        assert!(p
            .commands
            .iter()
            .any(|c| matches!(c, PathCommand::LineTo(_))));
    }

    #[test]
    fn empty_drawing_yields_empty_path() {
        let p = parse_drawing("", 1);
        assert!(p.commands.is_empty());
    }
}
