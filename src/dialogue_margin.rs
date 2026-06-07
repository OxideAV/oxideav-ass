//! Typed accessor for the per-event `MarginL` / `MarginR` / `MarginV`
//! columns of a `Dialogue:` line.
//!
//! The base [`parse`](crate::parse) entry point reads the dialogue
//! `Format:` row, splits each `Dialogue:` line on commas, and drops
//! the three margin columns on the floor — the shared `SubtitleCue`
//! IR currently has no slot for per-event margin overrides. The
//! round-trip writer fills the columns with literal `0`s. That is fine
//! for the dominant case (the columns are all zero in the wild), but
//! it loses any per-line margin override the original script
//! requested.
//!
//! The SSA v4.x specification defines each column the same way:
//!
//! > *MarginL — 4-figure Left Margin override. The values are in
//! > pixels. All zeroes means the default margins defined by the
//! > style are used.*
//!
//! `MarginR` and `MarginV` differ only in the axis they override
//! (right-edge distance and vertical-edge distance respectively). The
//! "all zeroes" shorthand is the load-bearing semantic carve-out: a
//! literal `0` (or any of the padded forms `00`, `000`, `0000`) means
//! "fall back to the style's `MarginL` / `MarginR` / `MarginV`", not
//! "render flush against the screen edge".
//!
//! [`parse_margin_field`] resolves one of these columns into a typed
//! [`MarginOverride`] enum:
//!
//! * [`MarginOverride::Default`] — column was empty, whitespace-only,
//!   or the `0` shorthand. The renderer reuses the named style's
//!   matching margin.
//! * [`MarginOverride::Pixels(n)`] — column carried a non-zero,
//!   non-negative pixel count. `n` is exposed as `u32`.
//!
//! Malformed columns (negative integers, non-numeric content) collapse
//! to [`MarginOverride::Default`] so the parser stays total — the
//! renderer falls back to the style margin, mirroring how the SSA
//! reference treats the all-zero shorthand. Surrounding whitespace
//! inside the column is trimmed before the integer parse. Leading
//! zeroes are allowed (the spec's "4-figure" wording lets a script
//! pad to a fixed width — `0150` and `150` resolve identically).
//!
//! The parser is total — it never panics and never returns an error.
//! Negative values, overflow, and stray non-digit characters all
//! decay to [`MarginOverride::Default`].

/// Typed view of one per-event margin column on a `Dialogue:` line.
///
/// Produced by [`parse_margin_field`]. The two variants encode the
/// spec's two semantic states: "fall back to the style's matching
/// `MarginL` / `MarginR` / `MarginV`" and "use this exact pixel
/// override". The [`Default`](MarginOverride::Default) impl is
/// `Default`, matching the dominant case in real scripts (every
/// well-known export tool emits `0,0,0` for unset per-event margins).
///
/// [`MarginOverride`] does not name an axis — the same enum carries
/// the typed value for any of `MarginL`, `MarginR`, or `MarginV`.
/// Callers that need the axis simply pair the result with the
/// `Format:`-row column name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarginOverride {
    /// Column was empty, whitespace-only, or carried the SSA `0`
    /// shorthand (`0`, `00`, `000`, `0000` — the spec's "4-figure"
    /// wording allows fixed-width padding). The renderer falls back
    /// to the style's `MarginL` / `MarginR` / `MarginV` for the
    /// matching axis. Also the fall-back when the parse fails on a
    /// malformed column.
    #[default]
    Default,
    /// Column carried a non-zero, non-negative pixel count. The
    /// renderer uses this value instead of the style's matching
    /// margin. The spec's wording is "4-figure" pixel value but does
    /// not pin an upper bound; the typed surface is `u32` so any
    /// non-negative integer that fits without overflow round-trips
    /// exactly.
    Pixels(u32),
}

impl MarginOverride {
    /// Returns the override pixel count, if any. The
    /// [`Default`](MarginOverride::Default) variant returns `None` so
    /// the caller can substitute the style's matching margin in a
    /// `style.margin_x.or(override.as_pixels())` chain.
    #[inline]
    pub fn as_pixels(self) -> Option<u32> {
        match self {
            MarginOverride::Default => None,
            MarginOverride::Pixels(n) => Some(n),
        }
    }

    /// Resolve this override against a style fallback. The fallback
    /// is the matching `MarginL` / `MarginR` / `MarginV` from the
    /// referenced `[V4+ Styles]` entry. Returns the override when
    /// [`Pixels`](MarginOverride::Pixels), or the fallback when
    /// [`Default`](MarginOverride::Default).
    #[inline]
    pub fn resolve_with_style(self, style_margin: u32) -> u32 {
        match self {
            MarginOverride::Default => style_margin,
            MarginOverride::Pixels(n) => n,
        }
    }
}

/// Resolve one of the `MarginL` / `MarginR` / `MarginV` columns into
/// a typed [`MarginOverride`].
///
/// The input is the raw bytes between two adjacent commas on a
/// `Dialogue:` line at the column position the `Format:` row labels
/// `MarginL`, `MarginR`, or `MarginV`. Empty / whitespace-only / the
/// `0` shorthand all map to [`MarginOverride::Default`]; anything
/// that successfully parses as a non-zero, non-negative integer maps
/// to [`MarginOverride::Pixels`]. The parser is total — malformed
/// input falls back to [`MarginOverride::Default`] so the renderer
/// transparently picks up the style's matching margin.
///
/// The same function handles all three axes; the spec defines them
/// with identical grammars. Callers select the axis at the call site
/// (e.g. by zipping `Format:` field names against split columns).
///
/// # Examples
///
/// ```
/// use oxideav_ass::dialogue_margin::{parse_margin_field, MarginOverride};
///
/// // Empty column — fall back to the style's matching margin.
/// assert_eq!(parse_margin_field(""), MarginOverride::Default);
///
/// // SSA `0` shorthand, in any of the spec's padded forms.
/// assert_eq!(parse_margin_field("0"), MarginOverride::Default);
/// assert_eq!(parse_margin_field("0000"), MarginOverride::Default);
///
/// // Explicit pixel override.
/// assert_eq!(parse_margin_field("150"), MarginOverride::Pixels(150));
/// assert_eq!(parse_margin_field("0150"), MarginOverride::Pixels(150));
///
/// // Resolve against the style.
/// let m = parse_margin_field("0");
/// assert_eq!(m.resolve_with_style(20), 20);
/// let m = parse_margin_field("75");
/// assert_eq!(m.resolve_with_style(20), 75);
/// ```
pub fn parse_margin_field(field: &str) -> MarginOverride {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return MarginOverride::Default;
    }
    // The spec writes the margin as a plain decimal pixel count and
    // does not allow a sign prefix. `u32::from_str_radix` happens to
    // accept `+`, so reject any non-digit first byte up front. Leading
    // zeroes ARE allowed — the spec's "4-figure" wording lets a script
    // pad to a fixed width (`0150` and `150` resolve identically).
    let first = trimmed.as_bytes().first().copied();
    if !matches!(first, Some(b'0'..=b'9')) {
        return MarginOverride::Default;
    }
    let n = match trimmed.parse::<u32>() {
        Ok(n) => n,
        Err(_) => return MarginOverride::Default,
    };
    if n == 0 {
        MarginOverride::Default
    } else {
        MarginOverride::Pixels(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_column_is_default() {
        assert_eq!(parse_margin_field(""), MarginOverride::Default);
    }

    #[test]
    fn whitespace_only_is_default() {
        assert_eq!(parse_margin_field("   "), MarginOverride::Default);
        assert_eq!(parse_margin_field("\t"), MarginOverride::Default);
    }

    #[test]
    fn zero_shorthand_is_default_in_all_padded_forms() {
        // The spec wording is "4-figure" margin override; padding to
        // four digits is allowed but not required. Both forms read as
        // "fall back to the style".
        for raw in ["0", "00", "000", "0000"] {
            assert_eq!(
                parse_margin_field(raw),
                MarginOverride::Default,
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn explicit_non_zero_pixel_value() {
        assert_eq!(parse_margin_field("150"), MarginOverride::Pixels(150));
        assert_eq!(parse_margin_field("1"), MarginOverride::Pixels(1));
        assert_eq!(parse_margin_field("9999"), MarginOverride::Pixels(9999));
    }

    #[test]
    fn leading_zeroes_preserved_as_decimal_value() {
        // `0150` is the spec's 4-figure padded form. Read as decimal,
        // not octal.
        assert_eq!(parse_margin_field("0150"), MarginOverride::Pixels(150));
        assert_eq!(parse_margin_field("0010"), MarginOverride::Pixels(10));
    }

    #[test]
    fn surrounding_whitespace_trimmed() {
        // Real-world authoring tools sometimes pad the CSV column
        // with a trailing space (`0, 0, 0,` style).
        assert_eq!(parse_margin_field("  150  "), MarginOverride::Pixels(150));
        assert_eq!(parse_margin_field("\t75"), MarginOverride::Pixels(75));
    }

    #[test]
    fn negative_value_collapses_to_default() {
        // Margins cannot be negative; treat as malformed and fall
        // back to the style.
        assert_eq!(parse_margin_field("-50"), MarginOverride::Default);
    }

    #[test]
    fn non_numeric_collapses_to_default() {
        // Decimal / hex / scientific / alpha all fall back to the
        // style margin — the parser stays total.
        for raw in ["abc", "12.5", "0xFF", "1e3", "150px", "hi"] {
            assert_eq!(
                parse_margin_field(raw),
                MarginOverride::Default,
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn overflowing_value_collapses_to_default() {
        // `u32::MAX` is 4_294_967_295. Anything larger fails the
        // parse → the renderer keeps the style margin.
        assert_eq!(
            parse_margin_field("9999999999999999"),
            MarginOverride::Default
        );
    }

    #[test]
    fn explicit_plus_sign_collapses_to_default() {
        // `+150` is not the SSA serialisation; reject it so the
        // round-trip stays well-defined.
        assert_eq!(parse_margin_field("+150"), MarginOverride::Default);
    }

    #[test]
    fn as_pixels_accessor_returns_none_for_default() {
        assert_eq!(parse_margin_field("0").as_pixels(), None);
        assert_eq!(parse_margin_field("").as_pixels(), None);
    }

    #[test]
    fn as_pixels_accessor_returns_some_for_explicit_value() {
        assert_eq!(parse_margin_field("150").as_pixels(), Some(150));
    }

    #[test]
    fn resolve_with_style_fallback_path() {
        // Default → use the style value.
        assert_eq!(parse_margin_field("0").resolve_with_style(25), 25);
        // Pixels → use the override.
        assert_eq!(parse_margin_field("75").resolve_with_style(25), 75);
        // Default also wins on malformed input.
        assert_eq!(parse_margin_field("-1").resolve_with_style(40), 40);
    }

    #[test]
    fn default_trait_matches_empty_column() {
        // `MarginOverride::default()` is the same as an empty
        // column; this lets struct literals stay terse.
        assert_eq!(MarginOverride::default(), MarginOverride::Default);
        assert_eq!(MarginOverride::default(), parse_margin_field(""));
    }

    #[test]
    fn copy_eq_traits_are_usable() {
        // The type is `Copy` + `Eq` — values can be matched / passed
        // freely without explicit clones.
        let a = parse_margin_field("100");
        let b = a;
        assert_eq!(a, b);
        match a {
            MarginOverride::Pixels(n) => assert_eq!(n, 100),
            MarginOverride::Default => panic!("expected explicit override"),
        }
    }
}
