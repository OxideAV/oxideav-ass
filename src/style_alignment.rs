//! Typed accessor for the `Alignment` column of a `[V4+ Styles]` /
//! `[V4 Styles]` `Style:` definition.
//!
//! The base [`parse`](crate::parse) entry point reads the styles
//! `Format:` row, splits each `Style:` line on commas, and decodes the
//! columns it has a slot for in the shared `SubtitleStyle` IR. For the
//! `Alignment` column it keeps only the *horizontal* justification
//! (left / centre / right) in `SubtitleStyle::align` — the shared
//! `TextAlign` IR has no slot for the *vertical* placement, so the
//! top / middle / bottom row the column also carries is dropped on the
//! floor. A renderer that needs the full on-screen anchor point has to
//! re-split the `Style:` row itself.
//!
//! The SSA v4.x / ASS specification documents two different numbering
//! schemes for this column, depending on which styles section the
//! `Style:` line belongs to:
//!
//! * **`[V4+ Styles]` (ASS)** — *"Alignment, but after the layout of
//!   the numpad (1-3 sub, 4-6 mid, 7-9 top)."* The value is a numpad
//!   code `1..=9`: the row is `1..=3` bottom, `4..=6` middle, `7..=9`
//!   top, and inside each row the column is left / centre / right. This
//!   is the same numbering the `\an<n>` override tag uses.
//! * **`[V4 Styles]` (legacy SSA)** — *"Values may be 1=Left,
//!   2=Centered, 3=Right. Add 4 to the value for a "Toptitle". Add 8 to
//!   the value for a "Midtitle"."* The low part picks the horizontal
//!   justification, the `+4` bit selects the top row and the `+8` bit
//!   selects the middle row; with neither bit set the text sits on the
//!   bottom row.
//!
//! [`parse_alignment_field`] resolves either scheme into a single
//! [`StyleAlignment`] carrying a normalised numpad position
//! ([`AlignH`] column × [`AlignV`] row), so a downstream renderer can
//! reason about one anchor model regardless of which styles dialect the
//! script used. The two schemes are selected at the call site by the
//! `is_ssa` flag, mirroring how the base parser already branches between
//! [`ass_alignment`](StyleAlignment::from_ass_numpad) and the SSA
//! legacy mapping.
//!
//! Malformed columns (empty, whitespace, non-numeric, out-of-range)
//! collapse to the spec's dominant anchor — bottom-centre (numpad `2`)
//! — so the parser stays total, mirroring how the base parser falls
//! back to `unwrap_or(2)` for an unparseable column.

/// Horizontal justification carried by the `Alignment` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignH {
    /// Left-justified within the L/R margins.
    Left,
    /// Centred within the L/R margins (the spec's dominant case).
    Center,
    /// Right-justified within the L/R margins.
    Right,
}

/// Vertical placement carried by the `Alignment` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignV {
    /// The bottom row — the "subtitle" position (the spec's dominant
    /// case; numpad `1..=3`, legacy SSA with neither the `+4` nor the
    /// `+8` bit set).
    Bottom,
    /// The middle row — a "midtitle" (numpad `4..=6`, legacy SSA `+8`).
    Middle,
    /// The top row — a "toptitle" (numpad `7..=9`, legacy SSA `+4`).
    Top,
}

/// Typed view of the `Alignment` column on a `[V4+ Styles]` /
/// `[V4 Styles]` `Style:` line.
///
/// Produced by [`parse_alignment_field`]. The value is normalised to a
/// numpad anchor — a horizontal [`AlignH`] column and a vertical
/// [`AlignV`] row — regardless of whether the source script used the
/// ASS numpad numbering or the legacy SSA `+4` / `+8` bit scheme.
///
/// The [`Default`] is bottom-centre (numpad `2`), the spec's dominant
/// on-screen anchor and the fall-back the base parser uses for an
/// unparseable column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleAlignment {
    /// Horizontal justification within the L/R margins.
    pub horizontal: AlignH,
    /// Vertical placement within the screen.
    pub vertical: AlignV,
}

impl Default for StyleAlignment {
    #[inline]
    fn default() -> Self {
        // Bottom-centre = numpad 2, the spec's dominant anchor.
        StyleAlignment {
            horizontal: AlignH::Center,
            vertical: AlignV::Bottom,
        }
    }
}

impl StyleAlignment {
    /// Build from an ASS numpad code `1..=9` (the `[V4+ Styles]`
    /// scheme, also used by the `\an<n>` override tag). Any value
    /// outside `1..=9` falls back to the default bottom-centre anchor.
    #[inline]
    pub fn from_ass_numpad(n: i64) -> Self {
        if !(1..=9).contains(&n) {
            return StyleAlignment::default();
        }
        let horizontal = match (n - 1) % 3 {
            0 => AlignH::Left,
            1 => AlignH::Center,
            _ => AlignH::Right,
        };
        let vertical = match (n - 1) / 3 {
            0 => AlignV::Bottom,
            1 => AlignV::Middle,
            _ => AlignV::Top,
        };
        StyleAlignment {
            horizontal,
            vertical,
        }
    }

    /// Build from a legacy SSA code (the `[V4 Styles]` scheme):
    /// `1`/`2`/`3` pick left / centre / right, the `+4` bit selects the
    /// top row, the `+8` bit selects the middle row, and with neither
    /// bit set the text sits on the bottom row. A column whose low bits
    /// do not name a valid justification (or carry both row bits)
    /// resolves the justification to centre.
    #[inline]
    pub fn from_ssa(n: i64) -> Self {
        let horizontal = match n & 0x03 {
            1 => AlignH::Left,
            3 => AlignH::Right,
            // `2` (centre) and the malformed `0` both centre, matching
            // the base parser's `_ => Center` fall-back.
            _ => AlignH::Center,
        };
        // `+8` (midtitle) takes precedence over `+4` (toptitle) when
        // both bits are somehow set, matching the spec's separate "Add 8
        // for a Midtitle" clause; with neither bit set the text is a
        // bottom subtitle.
        let vertical = if n & 0x08 != 0 {
            AlignV::Middle
        } else if n & 0x04 != 0 {
            AlignV::Top
        } else {
            AlignV::Bottom
        };
        StyleAlignment {
            horizontal,
            vertical,
        }
    }

    /// Return the ASS numpad code `1..=9` for this anchor — the value
    /// the `[V4+ Styles]` round-trip writer emits back into the
    /// `Alignment` column, and the same numbering the `\an<n>` override
    /// uses. Bottom-centre round-trips to `2`.
    #[inline]
    pub fn as_numpad(self) -> u8 {
        let col = match self.horizontal {
            AlignH::Left => 0,
            AlignH::Center => 1,
            AlignH::Right => 2,
        };
        let row = match self.vertical {
            AlignV::Bottom => 0,
            AlignV::Middle => 1,
            AlignV::Top => 2,
        };
        (row * 3 + col + 1) as u8
    }

    /// Return the legacy SSA code for this anchor — the value the
    /// `[V4 Styles]` round-trip writer emits back into the `Alignment`
    /// column. The horizontal part is `1`/`2`/`3`, with `+4` added for
    /// a toptitle and `+8` for a midtitle. Bottom-centre round-trips to
    /// `2`.
    #[inline]
    pub fn as_ssa(self) -> u8 {
        let base = match self.horizontal {
            AlignH::Left => 1,
            AlignH::Center => 2,
            AlignH::Right => 3,
        };
        let row_bits = match self.vertical {
            AlignV::Bottom => 0,
            AlignV::Top => 4,
            AlignV::Middle => 8,
        };
        base + row_bits
    }

    /// Whether this anchor sits on the bottom row (the dominant
    /// "subtitle" position).
    #[inline]
    pub fn is_bottom(self) -> bool {
        matches!(self.vertical, AlignV::Bottom)
    }
}

/// Resolve the `Alignment` column into a typed [`StyleAlignment`].
///
/// `is_ssa` selects the numbering scheme: `true` reads the legacy SSA
/// `+4` / `+8` bit scheme (`[V4 Styles]`), `false` reads the ASS numpad
/// scheme `1..=9` (`[V4+ Styles]`). This mirrors the `is_ssa` branch
/// the base parser already uses for the column.
///
/// The parser is total — empty, whitespace-only, non-numeric, or
/// out-of-range columns all fall back to the default bottom-centre
/// anchor (numpad `2`), matching the base parser's `unwrap_or(2)`
/// fall-back. Surrounding whitespace inside the column is trimmed and a
/// leading `+` on the magnitude is accepted before the integer parse.
///
/// # Examples
///
/// ```
/// use oxideav_ass::style_alignment::{parse_alignment_field, AlignH, AlignV};
///
/// // ASS numpad: 7 = top-left.
/// let a = parse_alignment_field("7", false);
/// assert_eq!(a.horizontal, AlignH::Left);
/// assert_eq!(a.vertical, AlignV::Top);
/// assert_eq!(a.as_numpad(), 7);
///
/// // Legacy SSA: 1 + 4 = left toptitle.
/// let s = parse_alignment_field("5", true);
/// assert_eq!(s.horizontal, AlignH::Left);
/// assert_eq!(s.vertical, AlignV::Top);
///
/// // Malformed columns fall back to bottom-centre.
/// assert_eq!(parse_alignment_field("", false), Default::default());
/// ```
pub fn parse_alignment_field(field: &str, is_ssa: bool) -> StyleAlignment {
    let trimmed = field.trim();
    match trimmed.parse::<i64>() {
        Ok(n) => {
            if is_ssa {
                StyleAlignment::from_ssa(n)
            } else {
                StyleAlignment::from_ass_numpad(n)
            }
        }
        Err(_) => StyleAlignment::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ass_numpad_full_grid() {
        // The full 1..=9 numpad maps to the documented anchor grid:
        // 1-3 bottom, 4-6 middle, 7-9 top; left/centre/right per column.
        let cases = [
            (1, AlignH::Left, AlignV::Bottom),
            (2, AlignH::Center, AlignV::Bottom),
            (3, AlignH::Right, AlignV::Bottom),
            (4, AlignH::Left, AlignV::Middle),
            (5, AlignH::Center, AlignV::Middle),
            (6, AlignH::Right, AlignV::Middle),
            (7, AlignH::Left, AlignV::Top),
            (8, AlignH::Center, AlignV::Top),
            (9, AlignH::Right, AlignV::Top),
        ];
        for (n, h, v) in cases {
            let a = parse_alignment_field(&n.to_string(), false);
            assert_eq!(a.horizontal, h, "n = {n}");
            assert_eq!(a.vertical, v, "n = {n}");
        }
    }

    #[test]
    fn ass_numpad_round_trips() {
        for n in 1..=9u8 {
            let a = parse_alignment_field(&n.to_string(), false);
            assert_eq!(a.as_numpad(), n, "n = {n}");
        }
    }

    #[test]
    fn ssa_legacy_grid() {
        // 1/2/3 = L/C/R on the bottom row.
        let cases = [
            (1, AlignH::Left, AlignV::Bottom),
            (2, AlignH::Center, AlignV::Bottom),
            (3, AlignH::Right, AlignV::Bottom),
            // +4 = toptitle.
            (5, AlignH::Left, AlignV::Top),
            (6, AlignH::Center, AlignV::Top),
            (7, AlignH::Right, AlignV::Top),
            // +8 = midtitle.
            (9, AlignH::Left, AlignV::Middle),
            (10, AlignH::Center, AlignV::Middle),
            (11, AlignH::Right, AlignV::Middle),
        ];
        for (n, h, v) in cases {
            let a = parse_alignment_field(&n.to_string(), true);
            assert_eq!(a.horizontal, h, "ssa n = {n}");
            assert_eq!(a.vertical, v, "ssa n = {n}");
        }
    }

    #[test]
    fn ssa_round_trips_via_as_ssa() {
        for n in [1, 2, 3, 5, 6, 7, 9, 10, 11] {
            let a = parse_alignment_field(&n.to_string(), true);
            assert_eq!(a.as_ssa() as i64, n, "ssa n = {n}");
        }
    }

    #[test]
    fn spec_worked_example_five_is_left_toptitle() {
        // The spec's worked example: "5 = left-justified toptitle".
        let a = parse_alignment_field("5", true);
        assert_eq!(a.horizontal, AlignH::Left);
        assert_eq!(a.vertical, AlignV::Top);
    }

    #[test]
    fn empty_column_is_bottom_centre() {
        assert_eq!(parse_alignment_field("", false), StyleAlignment::default());
        assert_eq!(parse_alignment_field("", true), StyleAlignment::default());
    }

    #[test]
    fn whitespace_only_is_bottom_centre() {
        assert_eq!(
            parse_alignment_field("   ", false),
            StyleAlignment::default()
        );
        assert_eq!(parse_alignment_field("\t", true), StyleAlignment::default());
    }

    #[test]
    fn surrounding_whitespace_trimmed() {
        let a = parse_alignment_field("  7  ", false);
        assert_eq!(a.horizontal, AlignH::Left);
        assert_eq!(a.vertical, AlignV::Top);
    }

    #[test]
    fn leading_plus_accepted() {
        let a = parse_alignment_field("+9", false);
        assert_eq!(a.horizontal, AlignH::Right);
        assert_eq!(a.vertical, AlignV::Top);
    }

    #[test]
    fn ass_out_of_range_collapses_to_bottom_centre() {
        // ASS numpad is 1..=9; 0, negative, and >9 all fall back.
        for raw in ["0", "10", "-1", "100"] {
            assert_eq!(
                parse_alignment_field(raw, false),
                StyleAlignment::default(),
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn non_numeric_collapses_to_bottom_centre() {
        for raw in ["centre", "top", "1.0", "0x5", "5px"] {
            assert_eq!(
                parse_alignment_field(raw, false),
                StyleAlignment::default(),
                "raw = {raw:?}"
            );
            assert_eq!(
                parse_alignment_field(raw, true),
                StyleAlignment::default(),
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn overflow_collapses_to_bottom_centre() {
        assert_eq!(
            parse_alignment_field("99999999999999999999", false),
            StyleAlignment::default()
        );
    }

    #[test]
    fn leading_zero_parsed_as_decimal() {
        // `07` is decimal 7 (top-left), not octal.
        let a = parse_alignment_field("07", false);
        assert_eq!(a.horizontal, AlignH::Left);
        assert_eq!(a.vertical, AlignV::Top);
    }

    #[test]
    fn ssa_low_bits_zero_centres_horizontally() {
        // An SSA value whose low two bits are 0 (e.g. bare `4` =
        // toptitle with no justification bits, or `8` = midtitle)
        // centres horizontally, matching the base parser's `_ => Center`.
        let top = parse_alignment_field("4", true);
        assert_eq!(top.horizontal, AlignH::Center);
        assert_eq!(top.vertical, AlignV::Top);
        let mid = parse_alignment_field("8", true);
        assert_eq!(mid.horizontal, AlignH::Center);
        assert_eq!(mid.vertical, AlignV::Middle);
    }

    #[test]
    fn ssa_both_row_bits_prefers_middle() {
        // If both +4 and +8 are set, the midtitle clause wins.
        let a = parse_alignment_field("13", true); // 1 + 4 + 8
        assert_eq!(a.horizontal, AlignH::Left);
        assert_eq!(a.vertical, AlignV::Middle);
    }

    #[test]
    fn as_numpad_is_always_one_to_nine() {
        for raw in ["", "1", "5", "9", "garbage", "0", "10"] {
            let n = parse_alignment_field(raw, false).as_numpad();
            assert!((1..=9).contains(&n), "raw = {raw:?}, n = {n}");
        }
    }

    #[test]
    fn is_bottom_accessor() {
        assert!(parse_alignment_field("2", false).is_bottom());
        assert!(parse_alignment_field("1", false).is_bottom());
        assert!(!parse_alignment_field("5", false).is_bottom());
        assert!(!parse_alignment_field("8", false).is_bottom());
    }

    #[test]
    fn default_is_bottom_centre_numpad_two() {
        let d = StyleAlignment::default();
        assert_eq!(d.horizontal, AlignH::Center);
        assert_eq!(d.vertical, AlignV::Bottom);
        assert_eq!(d.as_numpad(), 2);
        assert_eq!(d.as_ssa(), 2);
    }

    #[test]
    fn copy_eq_traits_are_usable() {
        let a = parse_alignment_field("9", false);
        let b = a;
        assert_eq!(a, b);
        assert_eq!(b.horizontal, AlignH::Right);
    }

    #[test]
    fn cross_scheme_anchor_agreement() {
        // The same on-screen anchor reached through either scheme must
        // produce the same normalised StyleAlignment. Left-toptitle is
        // numpad 7 in ASS and 1+4=5 in SSA.
        let ass = parse_alignment_field("7", false);
        let ssa = parse_alignment_field("5", true);
        assert_eq!(ass, ssa);
        // Right-midtitle: numpad 6 vs SSA 3+8=11.
        let ass2 = parse_alignment_field("6", false);
        let ssa2 = parse_alignment_field("11", true);
        assert_eq!(ass2, ssa2);
    }
}
