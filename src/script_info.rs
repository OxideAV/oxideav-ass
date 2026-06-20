//! Typed accessors for the document-level `[Script Info]` header fields.
//!
//! The base [`parse`](crate::parse) entry point captures the header as a
//! flat list of `Key: Value` pairs (surfaced on the shared track's
//! `metadata`), and the structured model's [`ScriptInfo`](crate::ScriptInfo)
//! keeps every line in source order for a byte-stable round-trip. Neither
//! interprets the *meaning* of the individual header keys — a renderer that
//! needs the script's wrapping mode, collision policy, or coordinate
//! resolution has to re-parse the raw value itself.
//!
//! This module lifts the header fields the SSA v4.x / ASS script-format
//! specification documents under the `[Script Info]` section into typed
//! values. These are *document-level* render parameters: unlike the
//! per-style columns or the per-segment override tags, they apply to the
//! whole script at once and form the coordinate space + timing base that
//! every override tag operates against.
//!
//! The spec documents these `[Script Info]` keys:
//!
//! * **`WrapStyle`** — the default line-wrapping mode (`0`..=`3`). The
//!   numbering matches the per-line `\q` override exactly (smart-even /
//!   end-of-line / no-wrap / smart-wide). Lifted by
//!   [`parse_wrap_style_field`] into [`WrapStyle`].
//! * **`Collisions`** — how overlapping subtitles are repositioned to
//!   avoid covering each other (`Normal` stacks upward from the bottom
//!   margin, `Reverse` shifts earlier lines up to make room). Lifted by
//!   [`parse_collisions_field`] into [`Collisions`].
//! * **`PlayResX` / `PlayResY`** — the script-resolution width / height
//!   the author laid the script out against. This is the pixel space all
//!   `\pos` / `\move` / `\clip` / `\org` coordinates live in. Lifted by
//!   [`parse_play_res_field`].
//! * **`PlayDepth`** — the colour depth (bits) the author used. Lifted by
//!   [`parse_play_depth_field`].
//! * **`Timer`** — the playback timer speed as a percentage
//!   (`"100.0000"` = exactly 100%). A time multiplier applied to the
//!   script clock. Lifted by [`parse_timer_field`].
//!
//! Every parser is **total**: a missing, empty, whitespace-only,
//! non-numeric, out-of-range, or otherwise malformed value collapses to
//! the spec's documented default rather than erroring, so a consumer can
//! call these on any script and always get a usable render parameter.

/// Typed view of the `[Script Info]` `WrapStyle` header.
///
/// Produced by [`parse_wrap_style_field`]. The four variants encode the
/// spec's four documented wrapping modes; the numbering is identical to
/// the per-line [`\q`](crate::AnimatedTag) override so a renderer can
/// reason about one wrapping model whether the mode arrives via the
/// document header or a per-line tag.
///
/// The [`Default`](WrapStyle::SmartEven) is
/// [`SmartEven`](WrapStyle::SmartEven) (`0`) — the spec's mode-`0`
/// behaviour and the implicit mode when the header is absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WrapStyle {
    /// `0` — smart wrapping: lines are broken so the visual rows come out
    /// as even in length as the breaks allow. The fall-back when the
    /// header is missing, empty, or malformed.
    #[default]
    SmartEven,
    /// `1` — end-of-line wrapping: text wraps only at the right edge, and
    /// only an explicit `\N` forces an earlier break.
    EndOfLine,
    /// `2` — no word wrapping at all: both `\n` and `\N` act as hard
    /// breaks, and text that runs past the edge is *not* re-flowed.
    NoWrap,
    /// `3` — same even-break logic as [`SmartEven`](WrapStyle::SmartEven)
    /// but biased so the *lower* of two rows is the wider one.
    SmartWide,
}

impl WrapStyle {
    /// The raw spec integer (`0`..=`3`) for this mode — the value the
    /// round-trip writer emits back into the `WrapStyle` header.
    #[inline]
    pub fn as_code(self) -> u8 {
        match self {
            WrapStyle::SmartEven => 0,
            WrapStyle::EndOfLine => 1,
            WrapStyle::NoWrap => 2,
            WrapStyle::SmartWide => 3,
        }
    }

    /// Whether this mode performs automatic word wrapping at the line
    /// edge. `true` for [`SmartEven`](WrapStyle::SmartEven) /
    /// [`EndOfLine`](WrapStyle::EndOfLine) / [`SmartWide`](WrapStyle::SmartWide)
    /// and `false` for [`NoWrap`](WrapStyle::NoWrap), where the only line
    /// breaks come from explicit `\n` / `\N` markers.
    #[inline]
    pub fn wraps_automatically(self) -> bool {
        !matches!(self, WrapStyle::NoWrap)
    }
}

/// Resolve the `[Script Info]` `WrapStyle` value into a typed
/// [`WrapStyle`].
///
/// The input is the raw value text after the `WrapStyle:` key. The
/// integers `0`..=`3` map to the four spec modes; the parser is total —
/// empty, whitespace-only, non-numeric, or out-of-range values all
/// collapse to [`WrapStyle::SmartEven`] (mode `0`), the spec's implicit
/// default when the header is absent. Surrounding whitespace is trimmed
/// and a leading `+` on the magnitude is accepted.
///
/// # Examples
///
/// ```
/// use oxideav_ass::script_info::{parse_wrap_style_field, WrapStyle};
///
/// assert_eq!(parse_wrap_style_field("0"), WrapStyle::SmartEven);
/// assert_eq!(parse_wrap_style_field("1"), WrapStyle::EndOfLine);
/// assert_eq!(parse_wrap_style_field("2"), WrapStyle::NoWrap);
/// assert_eq!(parse_wrap_style_field("3"), WrapStyle::SmartWide);
///
/// // Malformed / out-of-range collapse to the default mode.
/// assert_eq!(parse_wrap_style_field(""), WrapStyle::SmartEven);
/// assert_eq!(parse_wrap_style_field("9"), WrapStyle::SmartEven);
///
/// assert!(!parse_wrap_style_field("2").wraps_automatically());
/// assert_eq!(parse_wrap_style_field("3").as_code(), 3);
/// ```
pub fn parse_wrap_style_field(field: &str) -> WrapStyle {
    match field.trim().parse::<i64>() {
        Ok(1) => WrapStyle::EndOfLine,
        Ok(2) => WrapStyle::NoWrap,
        Ok(3) => WrapStyle::SmartWide,
        // `0` and every other value (including the empty / malformed
        // parse-error path) resolve to the default smart-even mode.
        _ => WrapStyle::SmartEven,
    }
}

/// Typed view of the `[Script Info]` `Collisions` header.
///
/// Produced by [`parse_collisions_field`]. The spec documents two
/// collision-prevention policies for how overlapping subtitle lines are
/// repositioned so they do not cover each other.
///
/// The [`Default`](Collisions::Normal) is [`Normal`](Collisions::Normal),
/// matching the spec's behaviour when the header is absent or
/// unrecognised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Collisions {
    /// `Normal` — subtitles stack upward from the bottom margin: each new
    /// overlapping line sits above the previous one, but always as close
    /// to the vertical (bottom) margin as possible, filling gaps as
    /// earlier lines expire. The spec's default policy.
    #[default]
    Normal,
    /// `Reverse` — earlier subtitles are shifted *up* to make room for
    /// later overlapping lines, so the lines can be read top-down. Uses
    /// more screen area and can place the first line high on the screen
    /// before the later lines appear.
    Reverse,
}

impl Collisions {
    /// The canonical spec keyword for this policy — `"Normal"` or
    /// `"Reverse"`. This is the value the round-trip writer emits back
    /// into the `Collisions` header.
    #[inline]
    pub fn as_keyword(self) -> &'static str {
        match self {
            Collisions::Normal => "Normal",
            Collisions::Reverse => "Reverse",
        }
    }

    /// Whether earlier lines move *up* to make room (`true` for
    /// [`Reverse`](Collisions::Reverse)).
    #[inline]
    pub fn is_reverse(self) -> bool {
        matches!(self, Collisions::Reverse)
    }
}

/// Resolve the `[Script Info]` `Collisions` value into a typed
/// [`Collisions`].
///
/// The keyword match is case-insensitive (the spec capitalises the
/// keywords but real scripts vary). The parser is total — any value other
/// than `Reverse` (including empty, whitespace, or an unrecognised
/// keyword) collapses to [`Collisions::Normal`], the spec's default
/// policy.
///
/// # Examples
///
/// ```
/// use oxideav_ass::script_info::{parse_collisions_field, Collisions};
///
/// assert_eq!(parse_collisions_field("Normal"), Collisions::Normal);
/// assert_eq!(parse_collisions_field("Reverse"), Collisions::Reverse);
/// assert_eq!(parse_collisions_field("reverse"), Collisions::Reverse);
///
/// // Malformed / unknown collapse to the default.
/// assert_eq!(parse_collisions_field(""), Collisions::Normal);
/// assert_eq!(parse_collisions_field("Up"), Collisions::Normal);
///
/// assert_eq!(parse_collisions_field("Reverse").as_keyword(), "Reverse");
/// ```
pub fn parse_collisions_field(field: &str) -> Collisions {
    if field.trim().eq_ignore_ascii_case("Reverse") {
        Collisions::Reverse
    } else {
        Collisions::Normal
    }
}

/// Resolve a `[Script Info]` `PlayResX` / `PlayResY` value into the
/// script-resolution pixel count.
///
/// `PlayResX` / `PlayResY` give the screen dimensions the author laid the
/// script out against — the pixel space all `\pos` / `\move` / `\clip` /
/// `\org` coordinates live in. The spec gives no explicit numeric default
/// (a renderer that finds the header missing falls back to the video
/// resolution), so this parser returns [`None`] for a missing /
/// malformed / non-positive value rather than inventing a number; the
/// caller decides the fall-back. A resolution must be a positive integer,
/// so `0` and negatives are rejected.
///
/// Surrounding whitespace is trimmed and a leading `+` is accepted.
///
/// # Examples
///
/// ```
/// use oxideav_ass::script_info::parse_play_res_field;
///
/// assert_eq!(parse_play_res_field("1920"), Some(1920));
/// assert_eq!(parse_play_res_field("  384 "), Some(384));
///
/// // No usable resolution.
/// assert_eq!(parse_play_res_field(""), None);
/// assert_eq!(parse_play_res_field("0"), None);
/// assert_eq!(parse_play_res_field("-720"), None);
/// assert_eq!(parse_play_res_field("auto"), None);
/// ```
pub fn parse_play_res_field(field: &str) -> Option<u32> {
    match field.trim().parse::<i64>() {
        Ok(n) if n > 0 && n <= u32::MAX as i64 => Some(n as u32),
        _ => None,
    }
}

/// Resolve a `[Script Info]` `PlayDepth` value into the colour depth in
/// bits.
///
/// `PlayDepth` records the colour depth the author used when playing the
/// script. The spec documents the field but pins no default, so a
/// missing / malformed / non-positive value returns [`None`].
///
/// # Examples
///
/// ```
/// use oxideav_ass::script_info::parse_play_depth_field;
///
/// assert_eq!(parse_play_depth_field("32"), Some(32));
/// assert_eq!(parse_play_depth_field(""), None);
/// assert_eq!(parse_play_depth_field("0"), None);
/// ```
pub fn parse_play_depth_field(field: &str) -> Option<u32> {
    match field.trim().parse::<i64>() {
        Ok(n) if n > 0 && n <= u32::MAX as i64 => Some(n as u32),
        _ => None,
    }
}

/// Resolve a `[Script Info]` `Timer` value into the playback timer speed
/// as a fractional multiplier.
///
/// The spec documents `Timer` as a percentage — `"100.0000"` is exactly
/// 100%, i.e. a time multiplier of `1.0` applied to the script clock.
/// This parser returns the multiplier (the percentage divided by 100), so
/// `"100.0000"` → `1.0`, `"200"` → `2.0`, `"50"` → `0.5`.
///
/// The parser is total — a missing, empty, whitespace-only, non-numeric,
/// non-finite, or negative value collapses to `1.0` (the 100% default a
/// renderer assumes when the header is absent). The returned multiplier
/// is always finite and non-negative.
///
/// # Examples
///
/// ```
/// use oxideav_ass::script_info::parse_timer_field;
///
/// assert!((parse_timer_field("100.0000") - 1.0).abs() < 1e-9);
/// assert!((parse_timer_field("200") - 2.0).abs() < 1e-9);
/// assert!((parse_timer_field("50") - 0.5).abs() < 1e-9);
///
/// // Malformed collapses to the 100% default.
/// assert!((parse_timer_field("") - 1.0).abs() < 1e-9);
/// assert!((parse_timer_field("fast") - 1.0).abs() < 1e-9);
/// assert!((parse_timer_field("-10") - 1.0).abs() < 1e-9);
/// ```
pub fn parse_timer_field(field: &str) -> f64 {
    match field.trim().parse::<f64>() {
        Ok(pct) if pct.is_finite() && pct >= 0.0 => pct / 100.0,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- WrapStyle -------------------------------------------------------

    #[test]
    fn wrap_style_spec_values() {
        assert_eq!(parse_wrap_style_field("0"), WrapStyle::SmartEven);
        assert_eq!(parse_wrap_style_field("1"), WrapStyle::EndOfLine);
        assert_eq!(parse_wrap_style_field("2"), WrapStyle::NoWrap);
        assert_eq!(parse_wrap_style_field("3"), WrapStyle::SmartWide);
    }

    #[test]
    fn wrap_style_empty_is_default() {
        assert_eq!(parse_wrap_style_field(""), WrapStyle::SmartEven);
        assert_eq!(parse_wrap_style_field("   "), WrapStyle::SmartEven);
        assert_eq!(parse_wrap_style_field("\t"), WrapStyle::SmartEven);
    }

    #[test]
    fn wrap_style_out_of_range_collapses() {
        for raw in ["4", "5", "9", "-1", "100"] {
            assert_eq!(
                parse_wrap_style_field(raw),
                WrapStyle::SmartEven,
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn wrap_style_non_numeric_collapses() {
        for raw in ["smart", "1.0", "0x2", "two", "2px"] {
            assert_eq!(
                parse_wrap_style_field(raw),
                WrapStyle::SmartEven,
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn wrap_style_whitespace_trimmed_and_plus_accepted() {
        assert_eq!(parse_wrap_style_field("  2  "), WrapStyle::NoWrap);
        assert_eq!(parse_wrap_style_field("+1"), WrapStyle::EndOfLine);
        // Leading-zero magnitude is decimal, not octal.
        assert_eq!(parse_wrap_style_field("03"), WrapStyle::SmartWide);
    }

    #[test]
    fn wrap_style_overflow_collapses() {
        assert_eq!(
            parse_wrap_style_field("99999999999999999999"),
            WrapStyle::SmartEven
        );
    }

    #[test]
    fn wrap_style_as_code_round_trips() {
        for raw in ["0", "1", "2", "3"] {
            let ws = parse_wrap_style_field(raw);
            assert_eq!(ws.as_code().to_string(), raw);
        }
        // Default fall-back emits 0.
        assert_eq!(parse_wrap_style_field("garbage").as_code(), 0);
    }

    #[test]
    fn wrap_style_as_code_is_valid_spec_value() {
        for raw in ["", "0", "1", "2", "3", "9", "garbage"] {
            assert!(parse_wrap_style_field(raw).as_code() <= 3, "raw = {raw:?}");
        }
    }

    #[test]
    fn wrap_style_wraps_automatically_accessor() {
        assert!(parse_wrap_style_field("0").wraps_automatically());
        assert!(parse_wrap_style_field("1").wraps_automatically());
        assert!(!parse_wrap_style_field("2").wraps_automatically());
        assert!(parse_wrap_style_field("3").wraps_automatically());
    }

    #[test]
    fn wrap_style_default_trait() {
        assert_eq!(WrapStyle::default(), WrapStyle::SmartEven);
        assert_eq!(WrapStyle::default(), parse_wrap_style_field("0"));
    }

    #[test]
    fn wrap_style_copy_eq() {
        let a = parse_wrap_style_field("2");
        let b = a;
        assert_eq!(a, b);
    }

    // --- Collisions ------------------------------------------------------

    #[test]
    fn collisions_spec_keywords() {
        assert_eq!(parse_collisions_field("Normal"), Collisions::Normal);
        assert_eq!(parse_collisions_field("Reverse"), Collisions::Reverse);
    }

    #[test]
    fn collisions_case_insensitive() {
        for raw in ["reverse", "REVERSE", "ReVeRsE", "  Reverse  "] {
            assert_eq!(
                parse_collisions_field(raw),
                Collisions::Reverse,
                "raw = {raw:?}"
            );
        }
        for raw in ["normal", "NORMAL", "  Normal "] {
            assert_eq!(
                parse_collisions_field(raw),
                Collisions::Normal,
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn collisions_unknown_collapses_to_normal() {
        for raw in ["", "   ", "Up", "Stack", "0", "1"] {
            assert_eq!(
                parse_collisions_field(raw),
                Collisions::Normal,
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn collisions_as_keyword_round_trips() {
        assert_eq!(parse_collisions_field("Normal").as_keyword(), "Normal");
        assert_eq!(parse_collisions_field("Reverse").as_keyword(), "Reverse");
        // Re-parsing the canonical keyword is a fixpoint.
        assert_eq!(
            parse_collisions_field(Collisions::Reverse.as_keyword()),
            Collisions::Reverse
        );
        assert_eq!(
            parse_collisions_field(Collisions::Normal.as_keyword()),
            Collisions::Normal
        );
    }

    #[test]
    fn collisions_is_reverse_accessor() {
        assert!(parse_collisions_field("Reverse").is_reverse());
        assert!(!parse_collisions_field("Normal").is_reverse());
        assert!(!parse_collisions_field("").is_reverse());
    }

    #[test]
    fn collisions_default_trait() {
        assert_eq!(Collisions::default(), Collisions::Normal);
    }

    // --- PlayResX / PlayResY ---------------------------------------------

    #[test]
    fn play_res_positive_integers() {
        assert_eq!(parse_play_res_field("1920"), Some(1920));
        assert_eq!(parse_play_res_field("1080"), Some(1080));
        assert_eq!(parse_play_res_field("384"), Some(384));
        assert_eq!(parse_play_res_field("1"), Some(1));
    }

    #[test]
    fn play_res_whitespace_and_plus() {
        assert_eq!(parse_play_res_field("  720 "), Some(720));
        assert_eq!(parse_play_res_field("+640"), Some(640));
    }

    #[test]
    fn play_res_rejects_zero_and_negative() {
        assert_eq!(parse_play_res_field("0"), None);
        assert_eq!(parse_play_res_field("-1"), None);
        assert_eq!(parse_play_res_field("-1920"), None);
    }

    #[test]
    fn play_res_rejects_malformed() {
        for raw in ["", "   ", "auto", "1920.0", "0x780", "720px"] {
            assert_eq!(parse_play_res_field(raw), None, "raw = {raw:?}");
        }
    }

    #[test]
    fn play_res_accepts_u32_max_boundary() {
        assert_eq!(parse_play_res_field("4294967295"), Some(u32::MAX));
        // One past u32::MAX overflows the resolution slot.
        assert_eq!(parse_play_res_field("4294967296"), None);
    }

    // --- PlayDepth -------------------------------------------------------

    #[test]
    fn play_depth_values() {
        assert_eq!(parse_play_depth_field("32"), Some(32));
        assert_eq!(parse_play_depth_field("24"), Some(24));
        assert_eq!(parse_play_depth_field("8"), Some(8));
    }

    #[test]
    fn play_depth_rejects_malformed_and_nonpositive() {
        for raw in ["", "0", "-8", "deep", "32bit"] {
            assert_eq!(parse_play_depth_field(raw), None, "raw = {raw:?}");
        }
    }

    // --- Timer -----------------------------------------------------------

    #[test]
    fn timer_percentage_to_multiplier() {
        assert!((parse_timer_field("100.0000") - 1.0).abs() < 1e-9);
        assert!((parse_timer_field("100") - 1.0).abs() < 1e-9);
        assert!((parse_timer_field("200") - 2.0).abs() < 1e-9);
        assert!((parse_timer_field("50") - 0.5).abs() < 1e-9);
        assert!((parse_timer_field("150.5") - 1.505).abs() < 1e-9);
    }

    #[test]
    fn timer_zero_is_legal() {
        // A 0% timer is degenerate but numerically valid (>= 0).
        assert!((parse_timer_field("0") - 0.0).abs() < 1e-9);
    }

    #[test]
    fn timer_malformed_collapses_to_one() {
        for raw in ["", "   ", "fast", "100%", "1e"] {
            assert!((parse_timer_field(raw) - 1.0).abs() < 1e-9, "raw = {raw:?}");
        }
    }

    #[test]
    fn timer_negative_collapses_to_one() {
        assert!((parse_timer_field("-10") - 1.0).abs() < 1e-9);
        assert!((parse_timer_field("-100") - 1.0).abs() < 1e-9);
    }

    #[test]
    fn timer_non_finite_collapses_to_one() {
        for raw in ["inf", "-inf", "NaN"] {
            assert!((parse_timer_field(raw) - 1.0).abs() < 1e-9, "raw = {raw:?}");
        }
    }

    #[test]
    fn timer_whitespace_trimmed() {
        assert!((parse_timer_field("  100.0  ") - 1.0).abs() < 1e-9);
    }

    #[test]
    fn timer_result_is_finite_and_nonnegative() {
        for raw in ["", "100", "0", "-5", "inf", "abc", "300.5"] {
            let m = parse_timer_field(raw);
            assert!(m.is_finite() && m >= 0.0, "raw = {raw:?} -> {m}");
        }
    }
}
