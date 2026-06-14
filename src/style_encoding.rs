//! Typed accessor for the `Encoding` column of a `[V4+ Styles]` /
//! `[V4 Styles]` `Style:` definition (Field 18 in the SSA v4.x style
//! row).
//!
//! The base [`parse`](crate::parse) entry point reads the styles
//! `Format:` row, splits each `Style:` line on commas, and decodes the
//! columns it has a slot for in the shared `SubtitleStyle` IR (name,
//! font, sizes, colours, the bold / italic / underline / strikeout
//! flags, alignment, margins, outline and shadow widths). The
//! `Encoding` column is read past — the shared IR carries no field for
//! the per-style font character set, so a renderer that needs it has to
//! re-split the `Style:` row itself.
//!
//! The `Encoding` column is the *style-level* counterpart of the `\fe`
//! override that already surfaces through the [`animate`](crate::animate)
//! module on a per-segment basis: both carry a Windows charset
//! (font-encoding) numeric ID that selects the glyph-mapping table. The
//! per-segment `\fe<id>` override, when present on a dialogue segment,
//! takes precedence over the style column; the style column supplies the
//! per-line baseline.
//!
//! The SSA v4.x / ASS specification documents the column as:
//!
//! > *Encoding. This specifies the font character set or encoding and on
//! > multi-lingual Windows installations it provides access to
//! > characters used in more than one language. It is usually 0 (zero)
//! > for English (Western, ANSI) Windows.*
//!
//! Two facts fall out of that wording:
//!
//! * The value is a Windows charset numeric ID. The full Win32 charset
//!   enum spans `0..=255`; the same common slots the `\fe` override
//!   documents apply here (`0` ANSI / `1` Default / `2` Symbol / `128`
//!   Shift-JIS / `134` GB2312 / `136` BIG5 / `162` Turkish / `163`
//!   Vietnamese / `177` Hebrew / `178` Arabic).
//! * The spec pins the dominant value — `0` for English (Western, ANSI)
//!   Windows — but no explicit fall-back for a missing column. `0`
//!   (ANSI) is the obvious neutral default, matching the spec's "usually
//!   0" wording.
//!
//! [`parse_encoding_field`] resolves the column into a typed
//! [`StyleEncoding`] carrying the charset ID; a malformed column
//! collapses to ANSI (`0`) so the parser stays total, mirroring how the
//! SSA reference treats an unset / unrecognised value.

/// Typed view of the `Encoding` column on a `[V4+ Styles]` /
/// `[V4 Styles]` `Style:` line: a Windows charset (font-encoding)
/// numeric ID in the Win32 `0..=255` range.
///
/// Produced by [`parse_encoding_field`]. The style-level counterpart of
/// the per-segment `\fe<id>` override; the override wins when present on
/// a dialogue segment, the style column supplies the per-line baseline.
///
/// The [`Default`] is ANSI (`0`), matching the spec's *"usually 0 (zero)
/// for English (Western, ANSI) Windows"* wording and the value a
/// malformed column collapses to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleEncoding {
    /// The Windows charset numeric ID. `0` = ANSI (Western), the spec's
    /// dominant value. The full Win32 charset enum spans `0..=255`; see
    /// [`StyleEncoding::charset_name`] for the common named slots.
    pub charset: u8,
}

impl Default for StyleEncoding {
    #[inline]
    fn default() -> Self {
        // ANSI (Western) — the spec's "usually 0" default.
        StyleEncoding { charset: 0 }
    }
}

impl StyleEncoding {
    /// Construct a [`StyleEncoding`] from a raw charset ID.
    #[inline]
    pub fn from_charset(charset: u8) -> Self {
        StyleEncoding { charset }
    }

    /// Returns the raw charset ID. This is the value the round-trip
    /// writer emits back into the `Encoding` column.
    #[inline]
    pub fn as_code(self) -> u8 {
        self.charset
    }

    /// Whether this is the ANSI (Western) charset — the spec's dominant
    /// value (`0`). A renderer can branch on this to skip charset-aware
    /// glyph mapping for the common Western case.
    #[inline]
    pub fn is_ansi(self) -> bool {
        self.charset == 0
    }

    /// The well-known short name for this charset ID, if it is one of
    /// the slots the spec / `\fe` documentation lists. Returns `None`
    /// for any other ID in the `0..=255` range (still a legal Win32
    /// charset, just not one of the documented common slots).
    ///
    /// The named slots mirror the `\fe` override documentation: `0`
    /// ANSI / `1` Default / `2` Symbol / `128` Shift-JIS / `134`
    /// GB2312 / `136` BIG5 / `162` Turkish / `163` Vietnamese / `177`
    /// Hebrew / `178` Arabic.
    #[inline]
    pub fn charset_name(self) -> Option<&'static str> {
        match self.charset {
            0 => Some("ANSI"),
            1 => Some("Default"),
            2 => Some("Symbol"),
            128 => Some("Shift-JIS"),
            134 => Some("GB2312"),
            136 => Some("BIG5"),
            162 => Some("Turkish"),
            163 => Some("Vietnamese"),
            177 => Some("Hebrew"),
            178 => Some("Arabic"),
            _ => None,
        }
    }
}

/// Resolve the `Encoding` column into a typed [`StyleEncoding`].
///
/// The input is the raw bytes between two adjacent commas on a
/// `Style:` line at the column position the styles `Format:` row labels
/// `Encoding`. The column is a Windows charset numeric ID in the Win32
/// `0..=255` range; the literal `0` selects ANSI (Western), the spec's
/// dominant value.
///
/// The parser is total — empty, whitespace-only, non-numeric, or
/// out-of-range columns all fall back to ANSI (`0`) so the renderer
/// transparently uses the spec's "usually 0" default, mirroring how the
/// SSA reference treats an unset / unrecognised value.
///
/// Surrounding whitespace inside the column is trimmed before parsing.
/// A leading `+` and a leading-zero magnitude are tolerated (decimal,
/// never octal). A value outside `0..=255` (the Win32 charset range)
/// collapses to ANSI, matching the `\fe` override's range handling.
///
/// # Examples
///
/// ```
/// use oxideav_ass::style_encoding::{parse_encoding_field, StyleEncoding};
///
/// // The spec's dominant value.
/// assert_eq!(parse_encoding_field("0"), StyleEncoding::from_charset(0));
/// assert!(parse_encoding_field("0").is_ansi());
///
/// // A common non-Western charset.
/// let jis = parse_encoding_field("128");
/// assert_eq!(jis.charset, 128);
/// assert_eq!(jis.charset_name(), Some("Shift-JIS"));
///
/// // Empty / malformed columns fall back to ANSI.
/// assert_eq!(parse_encoding_field(""), StyleEncoding::default());
/// assert_eq!(parse_encoding_field("utf8"), StyleEncoding::default());
/// ```
pub fn parse_encoding_field(field: &str) -> StyleEncoding {
    let trimmed = field.trim();
    match trimmed.parse::<i64>() {
        // Win32 charset enum is 0..=255; anything else (out of range or
        // a parse error) decays to ANSI (0).
        Ok(v) if (0..=255).contains(&v) => StyleEncoding { charset: v as u8 },
        _ => StyleEncoding::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_zero_is_default() {
        assert_eq!(parse_encoding_field("0"), StyleEncoding { charset: 0 });
        assert!(parse_encoding_field("0").is_ansi());
        assert_eq!(parse_encoding_field("0"), StyleEncoding::default());
    }

    #[test]
    fn common_charset_ids_parse() {
        assert_eq!(parse_encoding_field("1").charset, 1);
        assert_eq!(parse_encoding_field("2").charset, 2);
        assert_eq!(parse_encoding_field("128").charset, 128);
        assert_eq!(parse_encoding_field("134").charset, 134);
        assert_eq!(parse_encoding_field("136").charset, 136);
        assert_eq!(parse_encoding_field("255").charset, 255);
    }

    #[test]
    fn empty_and_whitespace_default_to_ansi() {
        assert_eq!(parse_encoding_field(""), StyleEncoding::default());
        assert_eq!(parse_encoding_field("   "), StyleEncoding::default());
        assert_eq!(parse_encoding_field("\t"), StyleEncoding::default());
    }

    #[test]
    fn surrounding_whitespace_trimmed() {
        assert_eq!(parse_encoding_field("  128  ").charset, 128);
        assert_eq!(parse_encoding_field("\t0").charset, 0);
    }

    #[test]
    fn leading_plus_and_zero_accepted() {
        assert_eq!(parse_encoding_field("+128").charset, 128);
        assert_eq!(parse_encoding_field("0128").charset, 128);
        assert_eq!(parse_encoding_field("00").charset, 0);
    }

    #[test]
    fn out_of_range_collapses_to_ansi() {
        // The Win32 charset enum is 0..=255; anything outside decays to
        // ANSI, mirroring the `\fe` override's range handling.
        for raw in ["256", "1000", "-1", "-128"] {
            assert_eq!(
                parse_encoding_field(raw),
                StyleEncoding::default(),
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn non_numeric_collapses_to_ansi() {
        for raw in ["utf8", "ansi", "0x80", "128px", "shift_jis", "1.0"] {
            assert_eq!(
                parse_encoding_field(raw),
                StyleEncoding::default(),
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn overflow_collapses_to_ansi() {
        assert_eq!(
            parse_encoding_field("99999999999999999999"),
            StyleEncoding::default()
        );
    }

    #[test]
    fn as_code_round_trips_the_charset_id() {
        assert_eq!(parse_encoding_field("0").as_code(), 0);
        assert_eq!(parse_encoding_field("128").as_code(), 128);
        assert_eq!(parse_encoding_field("255").as_code(), 255);
        // The default fall-back emits ANSI (0).
        assert_eq!(parse_encoding_field("garbage").as_code(), 0);
    }

    #[test]
    fn charset_name_lists_common_slots() {
        assert_eq!(parse_encoding_field("0").charset_name(), Some("ANSI"));
        assert_eq!(parse_encoding_field("1").charset_name(), Some("Default"));
        assert_eq!(parse_encoding_field("2").charset_name(), Some("Symbol"));
        assert_eq!(
            parse_encoding_field("128").charset_name(),
            Some("Shift-JIS")
        );
        assert_eq!(parse_encoding_field("134").charset_name(), Some("GB2312"));
        assert_eq!(parse_encoding_field("136").charset_name(), Some("BIG5"));
        assert_eq!(parse_encoding_field("162").charset_name(), Some("Turkish"));
        assert_eq!(
            parse_encoding_field("163").charset_name(),
            Some("Vietnamese")
        );
        assert_eq!(parse_encoding_field("177").charset_name(), Some("Hebrew"));
        assert_eq!(parse_encoding_field("178").charset_name(), Some("Arabic"));
    }

    #[test]
    fn charset_name_none_for_undocumented_slots() {
        // Legal Win32 IDs that are not one of the documented common
        // slots have no short name.
        for raw in ["3", "100", "200", "255"] {
            assert_eq!(
                parse_encoding_field(raw).charset_name(),
                None,
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn is_ansi_only_for_zero() {
        assert!(parse_encoding_field("0").is_ansi());
        assert!(!parse_encoding_field("1").is_ansi());
        assert!(!parse_encoding_field("128").is_ansi());
        // A malformed column falls back to ANSI.
        assert!(parse_encoding_field("garbage").is_ansi());
    }

    #[test]
    fn from_charset_constructor() {
        assert_eq!(StyleEncoding::from_charset(134).charset, 134);
        assert_eq!(StyleEncoding::from_charset(0), StyleEncoding::default());
    }

    #[test]
    fn default_trait_matches_ansi() {
        assert_eq!(StyleEncoding::default(), StyleEncoding { charset: 0 });
        assert_eq!(StyleEncoding::default(), parse_encoding_field("0"));
    }

    #[test]
    fn copy_eq_traits_are_usable() {
        let a = parse_encoding_field("128");
        let b = a;
        assert_eq!(a, b);
        let c = a;
        assert_eq!(c.charset, 128);
    }

    #[test]
    fn out_of_range_and_garbage_decay_to_ansi() {
        // Inputs outside the Win32 `0..=255` charset range (or
        // unparseable) all decay to ANSI rather than wrapping into a
        // surprise charset ID. Combined with the `u8` field type, this
        // pins the parser's range guarantee end-to-end.
        for raw in ["256", "-1", "garbage", "99999999999999999999", ""] {
            let st = parse_encoding_field(raw);
            assert_eq!(st.as_code(), 0, "raw = {raw:?}");
            assert!(st.is_ansi(), "raw = {raw:?}");
        }
        // In-range inputs round-trip their charset ID unchanged.
        for code in [0u8, 1, 128, 134, 255] {
            assert_eq!(
                parse_encoding_field(&code.to_string()).as_code(),
                code,
                "code = {code}"
            );
        }
    }
}
