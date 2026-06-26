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
    // Whether the current subpath has had geometry drawn into it since the
    // last `move_to` / `close`. The `m` command auto-closes an open shape
    // before starting a new one (per the drawing-command spec: *"If you
    // have an unclosed shape, it will automatically be closed"*); the `n`
    // command moves without closing. Tracking this lets `m` emit the
    // implied `close()` while `n` does not.
    let mut subpath_open = false;
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
                        // `m` auto-closes an open shape before moving; `n`
                        // leaves it open.
                        if cmd == 'm' && subpath_open {
                            path.close();
                        }
                        path.move_to(p);
                        cur = p;
                        i = ni;
                        last_cmd = Some(cmd);
                        subpath_open = false;
                    }
                }
                'l' => {
                    while let Some((p, ni)) = read_point(&tokens, i, scale) {
                        path.line_to(p);
                        cur = p;
                        i = ni;
                        subpath_open = true;
                    }
                    last_cmd = Some('l');
                }
                'b' => {
                    while let Some((p1, p2, p3, ni)) = read_three_points(&tokens, i, scale) {
                        path.cubic_to(p1, p2, p3);
                        cur = p3;
                        i = ni;
                        subpath_open = true;
                    }
                    last_cmd = Some('b');
                }
                's' => {
                    // Uniform cubic B-spline. The spec: `s` takes at
                    // least three coordinate pairs; `p` extends the
                    // b-spline; `c` closes it. The b-spline's control
                    // polygon is the current cursor followed by every
                    // `s`/`p` point. We gather the whole run here
                    // (consuming any contiguous `p` continuations) and
                    // convert each interior span to a Bézier via the
                    // standard B-spline → Bézier basis.
                    let mut ctrl = vec![cur];
                    while let Some((p, ni)) = read_point(&tokens, i, scale) {
                        ctrl.push(p);
                        i = ni;
                    }
                    // Absorb following `p` extension commands.
                    while i < tokens.len() && tokens[i] == "p" {
                        i += 1;
                        while let Some((p, ni)) = read_point(&tokens, i, scale) {
                            ctrl.push(p);
                            i = ni;
                        }
                    }
                    if let Some(end) = emit_bspline(&mut path, &ctrl) {
                        cur = end;
                        subpath_open = true;
                    }
                    last_cmd = Some('s');
                }
                'p' => {
                    // A bare `p` with no preceding `s` is degenerate; the
                    // spec only defines `p` as a b-spline extension. Treat
                    // the points as line segments so they still round-trip
                    // visually rather than vanishing.
                    while let Some((p, ni)) = read_point(&tokens, i, scale) {
                        path.line_to(p);
                        cur = p;
                        i = ni;
                        subpath_open = true;
                    }
                    last_cmd = Some('p');
                }
                'c' => {
                    path.close();
                    last_cmd = Some('c');
                    subpath_open = false;
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
                        subpath_open = true;
                    } else {
                        i += 1;
                    }
                }
                Some('b') => {
                    if let Some((p1, p2, p3, ni)) = read_three_points(&tokens, i, scale) {
                        path.cubic_to(p1, p2, p3);
                        cur = p3;
                        i = ni;
                        subpath_open = true;
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

/// Emit a uniform cubic B-spline through control polygon `ctrl` (the
/// current cursor followed by the `s` / `p` points) as a chain of Bézier
/// segments, returning the curve's end point (or `None` when there are
/// too few control points to form a segment).
///
/// For four consecutive control points `P0..P3` the open uniform cubic
/// B-spline segment is the cubic Bézier with control points (the standard
/// B-spline → Bézier basis, all weights summing to 1):
///
/// * `B0 = (P0 + 4·P1 + P2) / 6`
/// * `B1 = (4·P1 + 2·P2) / 6`
/// * `B2 = (2·P1 + 4·P2) / 6`
/// * `B3 = (P1 + 4·P2 + P3) / 6`
///
/// Consecutive segments share an endpoint (`C0`-continuous), so the chain
/// is laid down with a single `line_to(B0)` to reach the spline start
/// (the cursor is generally off the curve) followed by one `cubic_to`
/// per interior span.
fn emit_bspline(path: &mut Path, ctrl: &[Point]) -> Option<Point> {
    if ctrl.len() < 4 {
        return None;
    }
    let bez = |a: Point, wa: f32, b: Point, wb: f32, c: Point, wc: f32| {
        Point::new(
            (a.x * wa + b.x * wb + c.x * wc) / 6.0,
            (a.y * wa + b.y * wb + c.y * wc) / 6.0,
        )
    };
    let mut end = None;
    for w in ctrl.windows(4) {
        let (p0, p1, p2, p3) = (w[0], w[1], w[2], w[3]);
        let b0 = bez(p0, 1.0, p1, 4.0, p2, 1.0);
        let b1 = bez(p1, 4.0, p2, 2.0, p2, 0.0); // (4·P1 + 2·P2)/6
        let b2 = bez(p1, 2.0, p2, 4.0, p2, 0.0); // (2·P1 + 4·P2)/6
        let b3 = bez(p1, 1.0, p2, 4.0, p3, 1.0);
        if end.is_none() {
            // Reach the spline start before the first cubic.
            path.line_to(b0);
        }
        path.cubic_to(b1, b2, b3);
        end = Some(b3);
    }
    end
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
    fn m_auto_closes_previous_open_shape() {
        // Two triangles separated by a second `m`. The `m` before the
        // second triangle must auto-close the first open shape.
        let p = parse_drawing("m 0 0 l 10 0 10 10 m 20 20 l 30 20 30 30", 1);
        let closes = p
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::Close))
            .count();
        // One implicit close before the second `m`. (The trailing shape
        // is left open — only `m` / `c` close, and there is no second
        // `m` after it.)
        assert_eq!(closes, 1);
        // The close must sit immediately before the second MoveTo.
        let move_idxs: Vec<usize> = p
            .commands
            .iter()
            .enumerate()
            .filter_map(|(i, c)| matches!(c, PathCommand::MoveTo(_)).then_some(i))
            .collect();
        assert_eq!(move_idxs.len(), 2);
        let second_move = move_idxs[1];
        assert!(matches!(p.commands[second_move - 1], PathCommand::Close));
    }

    #[test]
    fn n_moves_without_closing() {
        // `n` between two line runs must NOT emit a close.
        let p = parse_drawing("m 0 0 l 10 0 10 10 n 20 20 l 30 20 30 30", 1);
        let closes = p
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::Close))
            .count();
        assert_eq!(closes, 0, "n must not auto-close the prior shape");
    }

    #[test]
    fn leading_m_does_not_emit_spurious_close() {
        // The very first `m` has no open shape to close.
        let p = parse_drawing("m 0 0 l 10 0 10 10", 1);
        assert!(!matches!(p.commands[0], PathCommand::Close));
        assert!(matches!(p.commands[0], PathCommand::MoveTo(_)));
    }

    #[test]
    fn explicit_c_then_m_does_not_double_close() {
        // An explicit `c` already closed the shape; the following `m`
        // must not emit a second redundant close.
        let p = parse_drawing("m 0 0 l 10 0 10 10 c m 20 20 l 30 20 30 30", 1);
        let closes = p
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::Close))
            .count();
        assert_eq!(closes, 1, "explicit c then m must not double-close");
    }

    #[test]
    fn s_spline_uses_bspline_basis() {
        // Cursor at (0,0); s 60 0 60 60 0 60. Control polygon is
        // P0=(0,0) P1=(60,0) P2=(60,60) P3=(0,60) → one Bézier segment.
        let p = parse_drawing("m 0 0 s 60 0 60 60 0 60", 1);
        // Commands: MoveTo(0,0), LineTo(B0), CubicCurveTo(B1,B2,B3).
        assert!(matches!(p.commands[0], PathCommand::MoveTo(_)));
        let b0 = match p.commands[1] {
            PathCommand::LineTo(pt) => pt,
            _ => panic!("expected LineTo to the spline start"),
        };
        // B0 = (P0 + 4P1 + P2)/6 = ((0+240+60)/6, (0+0+60)/6) = (50, 10).
        assert!((b0.x - 50.0).abs() < 1e-3, "b0.x = {}", b0.x);
        assert!((b0.y - 10.0).abs() < 1e-3, "b0.y = {}", b0.y);
        match p.commands[2] {
            PathCommand::CubicCurveTo { c1, c2, end } => {
                // B1 = (4P1 + 2P2)/6 = ((240+120)/6,(0+120)/6) = (60, 20).
                assert!((c1.x - 60.0).abs() < 1e-3);
                assert!((c1.y - 20.0).abs() < 1e-3);
                // B2 = (2P1 + 4P2)/6 = ((120+240)/6,(0+240)/6) = (60, 40).
                assert!((c2.x - 60.0).abs() < 1e-3);
                assert!((c2.y - 40.0).abs() < 1e-3);
                // B3 = (P1 + 4P2 + P3)/6 = ((60+240+0)/6,(0+240+60)/6)
                //    = (50, 50).
                assert!((end.x - 50.0).abs() < 1e-3, "end.x = {}", end.x);
                assert!((end.y - 50.0).abs() < 1e-3, "end.y = {}", end.y);
            }
            _ => panic!("expected a cubic for the spline segment"),
        }
    }

    #[test]
    fn s_then_p_extends_the_spline() {
        // `s` with three points then `p` with one more → five control
        // points (incl. cursor) → two Bézier segments.
        let p = parse_drawing("m 0 0 s 30 0 30 30 0 30 p 0 0", 1);
        let cubics = p
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::CubicCurveTo { .. }))
            .count();
        assert_eq!(cubics, 2, "two interior spans → two cubics");
        // Exactly one LineTo precedes the cubic chain (the spline start).
        let lines = p
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::LineTo(_)))
            .count();
        assert_eq!(lines, 1);
    }

    #[test]
    fn s_with_too_few_points_emits_nothing() {
        // `s` with only one point pair → control polygon of 2 (incl.
        // cursor) → no segment.
        let p = parse_drawing("m 0 0 s 10 10", 1);
        assert!(!p
            .commands
            .iter()
            .any(|c| matches!(c, PathCommand::CubicCurveTo { .. })));
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
