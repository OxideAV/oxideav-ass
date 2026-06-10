//! Typed accessor for the `BorderStyle` column of a `[V4+ Styles]` /
//! `[V4 Styles]` `Style:` definition.
//!
//! The base [`parse`](crate::parse) entry point reads the styles
//! `Format:` row, splits each `Style:` line on commas, and decodes the
//! columns it has a slot for in the shared `SubtitleStyle` IR (name,
//! font, sizes, colours, the bold / italic / underline / strikeout
//! flags, alignment, margins, outline and shadow widths). The
//! `BorderStyle` column is read past — the shared IR carries no field
//! for the rendering mode, so a renderer that needs it has to re-split
//! the `Style:` row itself.
//!
//! The SSA v4.x / ASS specification documents the column as:
//!
//! > *BorderStyle. 1 = Outline + drop shadow, 3 = Opaque box.*
//!
//! Two facts fall out of that wording:
//!
//! * Only two values are defined: `1` selects the outline-plus-drop-
//!   shadow rendering mode (the dominant case — every well-known
//!   export tool emits `1`), and `3` selects the opaque-box mode (the
//!   subtitle text sits on a filled rectangle in the outline colour).
//! * The neighbouring `Outline` and `Shadow` width columns are only
//!   meaningful when `BorderStyle` is `1`; under the opaque-box mode
//!   the box itself supplies the backdrop. The typed accessor captures
//!   only the mode — the width columns continue to flow through the
//!   existing `SubtitleStyle::outline` / `shadow` fields.
//!
//! [`parse_border_style_field`] resolves the column into a
//! [`BorderStyle`] enum:
//!
//! * [`BorderStyle::OutlineDropShadow`] — column was the literal `1`
//!   (or empty / whitespace / malformed, which all fall back to the
//!   spec's dominant rendering mode).
//! * [`BorderStyle::OpaqueBox`] — column was the literal `3`.
//!
//! Malformed columns (non-numeric content, any integer other than the
//! two spec-defined values, overflow) collapse to
//! [`BorderStyle::OutlineDropShadow`] so the parser stays total — the
//! renderer falls back to the dominant outline + drop-shadow mode,
//! mirroring how the SSA reference treats an unrecognised value.
//! Surrounding whitespace inside the column is trimmed before the
//! integer parse.

/// Typed view of the `BorderStyle` column on a `[V4+ Styles]` /
/// `[V4 Styles]` `Style:` line.
///
/// Produced by [`parse_border_style_field`]. The two variants encode
/// the spec's two defined rendering modes. The
/// [`Default`](BorderStyle::OutlineDropShadow) impl is
/// [`OutlineDropShadow`](BorderStyle::OutlineDropShadow), matching the
/// dominant case in real scripts (every well-known export tool emits
/// `1` for the border-style column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderStyle {
    /// Column carried the literal `1` — the text is drawn with an
    /// outline (width from the `Outline` column) and a drop shadow
    /// (depth from the `Shadow` column). This is the spec's dominant
    /// mode and the fall-back when the column is empty, whitespace, or
    /// carries an unrecognised value.
    #[default]
    OutlineDropShadow,
    /// Column carried the literal `3` — the subtitle text sits on an
    /// opaque filled rectangle (rendered in the outline colour). Under
    /// this mode the `Outline` / `Shadow` width columns no longer
    /// describe an outline + drop shadow; the box itself provides the
    /// backdrop.
    OpaqueBox,
}

impl BorderStyle {
    /// Returns the raw spec integer for this mode: `1` for
    /// [`OutlineDropShadow`](BorderStyle::OutlineDropShadow), `3` for
    /// [`OpaqueBox`](BorderStyle::OpaqueBox). This is the value the
    /// round-trip writer emits back into the `BorderStyle` column.
    #[inline]
    pub fn as_code(self) -> u8 {
        match self {
            BorderStyle::OutlineDropShadow => 1,
            BorderStyle::OpaqueBox => 3,
        }
    }

    /// Whether this mode renders the text on a filled rectangle
    /// (`true` for [`OpaqueBox`](BorderStyle::OpaqueBox)). A renderer
    /// can branch on this to decide between the outline + drop-shadow
    /// path and the box-backdrop path.
    #[inline]
    pub fn is_opaque_box(self) -> bool {
        matches!(self, BorderStyle::OpaqueBox)
    }
}

/// Resolve the `BorderStyle` column into a typed [`BorderStyle`].
///
/// The input is the raw bytes between two adjacent commas on a
/// `Style:` line at the column position the styles `Format:` row
/// labels `BorderStyle`. The literal `1` maps to
/// [`BorderStyle::OutlineDropShadow`] and the literal `3` maps to
/// [`BorderStyle::OpaqueBox`]. The parser is total — empty,
/// whitespace-only, non-numeric, or out-of-range columns all fall back
/// to [`BorderStyle::OutlineDropShadow`] so the renderer transparently
/// uses the dominant outline + drop-shadow mode.
///
/// Surrounding whitespace inside the column is trimmed before parsing.
/// The spec defines exactly two values, so a leading `+` on the
/// magnitude (`+1`, `+3`) is accepted and any other integer (`0`, `2`,
/// `4`, negative values) collapses to the default mode.
///
/// # Examples
///
/// ```
/// use oxideav_ass::style_border::{parse_border_style_field, BorderStyle};
///
/// // The two spec-defined values.
/// assert_eq!(parse_border_style_field("1"), BorderStyle::OutlineDropShadow);
/// assert_eq!(parse_border_style_field("3"), BorderStyle::OpaqueBox);
///
/// // Empty / malformed columns fall back to the dominant mode.
/// assert_eq!(parse_border_style_field(""), BorderStyle::OutlineDropShadow);
/// assert_eq!(parse_border_style_field("2"), BorderStyle::OutlineDropShadow);
///
/// // Round-trip the raw spec integer.
/// assert_eq!(parse_border_style_field("3").as_code(), 3);
/// assert!(parse_border_style_field("3").is_opaque_box());
/// ```
pub fn parse_border_style_field(field: &str) -> BorderStyle {
    let trimmed = field.trim();
    match trimmed.parse::<i64>() {
        Ok(3) => BorderStyle::OpaqueBox,
        // `1` and every other value (including the empty/malformed
        // parse-error path below) resolve to the dominant mode.
        _ => BorderStyle::OutlineDropShadow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_one_is_outline_drop_shadow() {
        assert_eq!(
            parse_border_style_field("1"),
            BorderStyle::OutlineDropShadow
        );
    }

    #[test]
    fn value_three_is_opaque_box() {
        assert_eq!(parse_border_style_field("3"), BorderStyle::OpaqueBox);
    }

    #[test]
    fn empty_column_is_default_mode() {
        assert_eq!(parse_border_style_field(""), BorderStyle::OutlineDropShadow);
    }

    #[test]
    fn whitespace_only_is_default_mode() {
        assert_eq!(
            parse_border_style_field("   "),
            BorderStyle::OutlineDropShadow
        );
        assert_eq!(
            parse_border_style_field("\t"),
            BorderStyle::OutlineDropShadow
        );
    }

    #[test]
    fn surrounding_whitespace_trimmed() {
        assert_eq!(parse_border_style_field("  3  "), BorderStyle::OpaqueBox);
        assert_eq!(
            parse_border_style_field("\t1"),
            BorderStyle::OutlineDropShadow
        );
    }

    #[test]
    fn leading_plus_accepted() {
        // A leading `+` on the magnitude parses to the same value.
        assert_eq!(
            parse_border_style_field("+1"),
            BorderStyle::OutlineDropShadow
        );
        assert_eq!(parse_border_style_field("+3"), BorderStyle::OpaqueBox);
    }

    #[test]
    fn other_integers_collapse_to_default_mode() {
        // Only `1` and `3` are spec-defined; everything else (including
        // the SSA-era `0`, the unused `2` / `4`, and negatives) falls
        // back to the dominant outline + drop-shadow mode.
        for raw in ["0", "2", "4", "5", "-1", "-3", "100"] {
            assert_eq!(
                parse_border_style_field(raw),
                BorderStyle::OutlineDropShadow,
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn non_numeric_collapses_to_default_mode() {
        for raw in ["box", "outline", "1.0", "0x3", "3px", "three"] {
            assert_eq!(
                parse_border_style_field(raw),
                BorderStyle::OutlineDropShadow,
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn overflowing_value_collapses_to_default_mode() {
        // The parse target is `i64`; anything outside it fails and
        // decays to the dominant mode.
        assert_eq!(
            parse_border_style_field("99999999999999999999"),
            BorderStyle::OutlineDropShadow
        );
    }

    #[test]
    fn leading_zero_magnitude_parsed_as_decimal() {
        // `03` is decimal `3`, not octal — it selects the opaque box.
        assert_eq!(parse_border_style_field("03"), BorderStyle::OpaqueBox);
        assert_eq!(
            parse_border_style_field("001"),
            BorderStyle::OutlineDropShadow
        );
    }

    #[test]
    fn as_code_round_trips_the_spec_integer() {
        assert_eq!(parse_border_style_field("1").as_code(), 1);
        assert_eq!(parse_border_style_field("3").as_code(), 3);
        // The default fall-back also emits `1`.
        assert_eq!(parse_border_style_field("garbage").as_code(), 1);
    }

    #[test]
    fn is_opaque_box_accessor() {
        assert!(parse_border_style_field("3").is_opaque_box());
        assert!(!parse_border_style_field("1").is_opaque_box());
        assert!(!parse_border_style_field("").is_opaque_box());
    }

    #[test]
    fn default_trait_matches_dominant_mode() {
        assert_eq!(BorderStyle::default(), BorderStyle::OutlineDropShadow);
        assert_eq!(BorderStyle::default(), parse_border_style_field("1"));
    }

    #[test]
    fn copy_eq_traits_are_usable() {
        let a = parse_border_style_field("3");
        let b = a;
        assert_eq!(a, b);
        match a {
            BorderStyle::OpaqueBox => {}
            BorderStyle::OutlineDropShadow => panic!("expected opaque box"),
        }
    }

    #[test]
    fn as_code_is_a_valid_spec_value() {
        // Whatever the input, `as_code` only ever emits one of the two
        // spec-defined integers — never an out-of-range value.
        for raw in ["", "1", "3", "2", "garbage", "-1"] {
            let code = parse_border_style_field(raw).as_code();
            assert!(code == 1 || code == 3, "raw = {raw:?}, code = {code}");
        }
    }
}
