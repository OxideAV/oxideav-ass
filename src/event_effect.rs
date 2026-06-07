//! Typed accessor for the `Effect:` column of a `Dialogue:` event line.
//!
//! The base [`parse`](crate::parse) entry point reads the dialogue
//! `Format:` row, splits each `Dialogue:` line on commas, and drops the
//! `Effect` field on the floor — the round-trip writer fills the column
//! with the empty string because the shared `SubtitleCue` IR has no
//! slot for it. That is fine for the most common case (the column is
//! empty), but it loses any *transition effect* the original script
//! requested.
//!
//! The SSA v4.x specification documents three transition effects that
//! live in this column. They share a small grammar: a case-sensitive
//! keyword that names the effect, followed by semicolon-separated
//! parameters. The keyword does not carry quote marks.
//!
//! * `Karaoke` — successive per-word highlight. The spec calls this an
//!   obsolete effect (`\k` override tags are the live replacement);
//!   it still survives in historical scripts and is recognised here so
//!   a consumer can choose to honour or skip it.
//! * `Scroll up;y1;y2;delay[;fadeawayheight]` — the rendered line
//!   scrolls upwards inside a vertical region bounded by `y1` and `y2`.
//!   The spec notes that `y1` and `y2` may be supplied in either
//!   order, that both zero means "scroll the full height of the
//!   screen", and that `delay` is a `1..=100` slow-down knob with `0`
//!   meaning "as fast as possible". The optional `fadeawayheight`
//!   trailing field makes the top and bottom edges of the scrolling
//!   region transparent.
//! * `Scroll down;y1;y2;delay[;fadeawayheight]` — the downward sibling
//!   of `Scroll up`, same parameter shape.
//! * `Banner;delay[;lefttoright;fadeawaywidth]` — the line is forced
//!   into a single visual row and scrolled horizontally across the
//!   screen. `delay` follows the same `0..=100` rule. The optional
//!   `lefttoright` flag (`0` = right-to-left, the spec default;
//!   `1` = left-to-right) and `fadeawaywidth` make the left and right
//!   edges of the scrolling row transparent.
//!
//! Anything that does not match one of those four keywords (or the
//! empty / whitespace-only column, which is the dominant case) is
//! returned as [`EventEffect::Empty`] / [`EventEffect::Other`] — the
//! consumer keeps the raw bytes and can ignore them or pass them
//! through unchanged.
//!
//! Parameter clamping mirrors the spec text:
//!
//! * `delay` is documented as `1..=100` for both Scroll and Banner;
//!   we accept `0..=100` (the spec calls `0` "as fast as possible")
//!   and clamp anything above `100` down to `100`. Negative `delay`
//!   values fail the parse (the line falls back to `Other` so the
//!   caller can decide how to react).
//! * `y1` / `y2` are pixel coordinates; the spec does not restrict
//!   the order, so we surface both as written and a normalised
//!   `top` / `bottom` pair that places the smaller value on top.
//! * `lefttoright` clamps to `0` / `1` (boolean), `fadeawayheight` /
//!   `fadeawaywidth` clamp to `0..` (negative values are dropped to
//!   `0`).
//!
//! The keyword match is *case-sensitive* per the spec: `karaoke` lower
//! and `KARAOKE` upper both fall through to [`EventEffect::Other`].
//! Surrounding whitespace inside individual parameter slots is trimmed
//! before parsing the number; whitespace inside the keyword itself is
//! preserved (so `Scroll  up` is `Other`, not `ScrollUp`).
//!
//! [`parse_effect_field`] is the single entry point. It never returns
//! an error — bad input collapses to [`EventEffect::Other`] with the
//! original bytes captured so the consumer can re-emit them verbatim
//! or surface a warning to its user.

/// Direction parameter for the `Banner` transition effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BannerDirection {
    /// Default value per spec — text scrolls from the right edge of
    /// the screen towards the left. The spec phrasing is
    /// "scrolled from right to left accross the screen".
    #[default]
    RightToLeft,
    /// Set by the optional `lefttoright=1` parameter. The line scrolls
    /// from the left edge of the screen towards the right.
    LeftToRight,
}

/// Typed view of the `Effect:` column on one dialogue event.
///
/// Produced by [`parse_effect_field`]. The variant captures the
/// keyword the script asked for and any parsed parameters; the
/// catch-all [`EventEffect::Other`] keeps the original bytes so a
/// consumer that wants to re-emit them can do so without losing
/// information.
#[derive(Debug, Clone, PartialEq)]
pub enum EventEffect {
    /// Column was empty or whitespace-only — by far the dominant case
    /// in real scripts.
    Empty,
    /// `Karaoke` — the SSA-v4 successive per-word highlight effect.
    /// The spec marks this as obsolete; the `\k` family of override
    /// tags is the live replacement. Recognised here for fidelity on
    /// historical scripts.
    Karaoke,
    /// `Scroll up;y1;y2;delay[;fadeawayheight]`.
    ScrollUp {
        /// `y1` value exactly as written in the script (the script
        /// may pass top / bottom in either order).
        y1: u32,
        /// `y2` value exactly as written in the script.
        y2: u32,
        /// `delay`, clamped to `0..=100`. `0` means "no delay,
        /// scroll as fast as possible" per the spec.
        delay: u8,
        /// Optional trailing `fadeawayheight` — number of pixels at
        /// the top and bottom of the scroll region to fade out for
        /// soft edges. `None` when the column ended after `delay`.
        fadeawayheight: Option<u32>,
    },
    /// `Scroll down;y1;y2;delay[;fadeawayheight]` — the downward
    /// sibling of [`ScrollUp`](EventEffect::ScrollUp).
    ScrollDown {
        y1: u32,
        y2: u32,
        delay: u8,
        fadeawayheight: Option<u32>,
    },
    /// `Banner;delay[;lefttoright;fadeawaywidth]`.
    Banner {
        /// `delay`, clamped to `0..=100` per the spec.
        delay: u8,
        /// Scroll direction — defaults to
        /// [`BannerDirection::RightToLeft`] when the optional
        /// `lefttoright` parameter is missing.
        direction: BannerDirection,
        /// Optional trailing `fadeawaywidth` — number of pixels at
        /// the left and right of the banner to fade out for soft
        /// edges. `None` when the column ended before the field, or
        /// when `direction` itself was absent.
        fadeawaywidth: Option<u32>,
    },
    /// Catch-all for empty-keyword content the parser did not
    /// recognise. The string is the original column bytes minus
    /// leading/trailing whitespace — passing it through `format!`
    /// reproduces the script's writing.
    Other(String),
}

/// Resolve the `Effect:` column on a dialogue event into a typed
/// [`EventEffect`].
///
/// The input is the raw bytes between the eighth and ninth comma on a
/// `Dialogue:` line (i.e. the column the format row labels `Effect`).
/// Empty / whitespace-only input maps to [`EventEffect::Empty`];
/// anything else is matched against the four SSA-v4 keywords and falls
/// back to [`EventEffect::Other`] when nothing fits.
///
/// The parser is total — it never panics and never returns an error.
/// Malformed payloads (missing required parameters, non-numeric
/// values, negative `delay`) collapse to [`EventEffect::Other`] so the
/// caller can decide whether to drop the line, surface a diagnostic,
/// or simply re-emit the bytes through a write loop.
pub fn parse_effect_field(field: &str) -> EventEffect {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return EventEffect::Empty;
    }
    // Keyword + params split on the first `;`. Case is *significant*
    // per the spec's "effect names are case sensitive and must appear
    // exactly as shown" rule.
    let (head, rest) = match trimmed.split_once(';') {
        Some((h, r)) => (h, Some(r)),
        None => (trimmed, None),
    };
    let head = head.trim_end(); // tolerate trailing space before `;`
    match head {
        "Karaoke" => EventEffect::Karaoke,
        "Scroll up" => parse_scroll(rest, /*down=*/ false)
            .unwrap_or_else(|| EventEffect::Other(trimmed.to_string())),
        "Scroll down" => parse_scroll(rest, /*down=*/ true)
            .unwrap_or_else(|| EventEffect::Other(trimmed.to_string())),
        "Banner" => parse_banner(rest).unwrap_or_else(|| EventEffect::Other(trimmed.to_string())),
        _ => EventEffect::Other(trimmed.to_string()),
    }
}

/// Parse the parameter tail of `Scroll up` / `Scroll down`. Returns
/// `None` when the required `y1 ; y2 ; delay` triplet is missing or
/// malformed, so the caller can fall back to
/// [`EventEffect::Other`].
fn parse_scroll(rest: Option<&str>, down: bool) -> Option<EventEffect> {
    let params: Vec<&str> = rest?.split(';').collect();
    if params.len() < 3 {
        return None;
    }
    let y1: u32 = params[0].trim().parse().ok()?;
    let y2: u32 = params[1].trim().parse().ok()?;
    let delay = parse_delay(params[2])?;
    let fadeawayheight = if params.len() >= 4 {
        Some(parse_nonneg(params[3]).unwrap_or(0))
    } else {
        None
    };
    Some(if down {
        EventEffect::ScrollDown {
            y1,
            y2,
            delay,
            fadeawayheight,
        }
    } else {
        EventEffect::ScrollUp {
            y1,
            y2,
            delay,
            fadeawayheight,
        }
    })
}

/// Parse the parameter tail of `Banner`. Returns `None` when the
/// required `delay` slot is missing or malformed.
fn parse_banner(rest: Option<&str>) -> Option<EventEffect> {
    let params: Vec<&str> = rest?.split(';').collect();
    if params.is_empty() {
        return None;
    }
    let delay = parse_delay(params[0])?;
    let direction = if params.len() >= 2 {
        match params[1].trim().parse::<i32>().ok()? {
            0 => BannerDirection::RightToLeft,
            1 => BannerDirection::LeftToRight,
            _ => return None,
        }
    } else {
        BannerDirection::RightToLeft
    };
    let fadeawaywidth = if params.len() >= 3 {
        Some(parse_nonneg(params[2]).unwrap_or(0))
    } else {
        None
    };
    Some(EventEffect::Banner {
        delay,
        direction,
        fadeawaywidth,
    })
}

/// Parse a `delay` slot, clamping to the spec's `0..=100` range.
/// Negative values are rejected (the parameter is documented as a
/// non-negative slow-down knob).
fn parse_delay(s: &str) -> Option<u8> {
    let n: i32 = s.trim().parse().ok()?;
    if n < 0 {
        return None;
    }
    Some(n.min(100) as u8)
}

/// Parse a non-negative pixel count. Negative inputs collapse to `0`
/// (the field is a width / height — there is no meaningful negative).
fn parse_nonneg(s: &str) -> Option<u32> {
    let n: i32 = s.trim().parse().ok()?;
    Some(n.max(0) as u32)
}

impl EventEffect {
    /// `(top, bottom)` — the smaller of `y1` / `y2` first. Returns
    /// `None` for variants that do not carry a Y region.
    ///
    /// The Kotus spec calls out that the script may pass the top and
    /// bottom values in either order: "it doesn't matter which value
    /// (top or bottom) comes first". This accessor performs the
    /// normalisation so a consumer can clip on `[top, bottom]`
    /// without re-reading the variant manually.
    pub fn scroll_region(&self) -> Option<(u32, u32)> {
        match self {
            EventEffect::ScrollUp { y1, y2, .. } | EventEffect::ScrollDown { y1, y2, .. } => {
                Some((*y1.min(y2), *y1.max(y2)))
            }
            _ => None,
        }
    }

    /// `true` when both `y1` and `y2` are zero — the spec's "scroll the
    /// full height of the screen" shorthand. Returns `false` for any
    /// other configuration and for the non-scroll variants.
    pub fn scrolls_full_height(&self) -> bool {
        matches!(
            self,
            EventEffect::ScrollUp { y1: 0, y2: 0, .. }
                | EventEffect::ScrollDown { y1: 0, y2: 0, .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_column_maps_to_empty_variant() {
        assert_eq!(parse_effect_field(""), EventEffect::Empty);
        assert_eq!(parse_effect_field("   "), EventEffect::Empty);
        assert_eq!(parse_effect_field("\t"), EventEffect::Empty);
    }

    #[test]
    fn karaoke_keyword_maps_to_karaoke_variant() {
        assert_eq!(parse_effect_field("Karaoke"), EventEffect::Karaoke);
        // Leading / trailing whitespace tolerated.
        assert_eq!(parse_effect_field("  Karaoke  "), EventEffect::Karaoke);
    }

    #[test]
    fn karaoke_keyword_is_case_sensitive() {
        // The spec's "effect names are case sensitive and must appear
        // exactly as shown" rule sends every case variant to `Other`.
        assert_eq!(
            parse_effect_field("karaoke"),
            EventEffect::Other("karaoke".to_string())
        );
        assert_eq!(
            parse_effect_field("KARAOKE"),
            EventEffect::Other("KARAOKE".to_string())
        );
    }

    #[test]
    fn scroll_up_three_required_params_parses() {
        let e = parse_effect_field("Scroll up;100;200;50");
        assert_eq!(
            e,
            EventEffect::ScrollUp {
                y1: 100,
                y2: 200,
                delay: 50,
                fadeawayheight: None,
            }
        );
    }

    #[test]
    fn scroll_up_with_optional_fadeawayheight() {
        let e = parse_effect_field("Scroll up;100;200;50;15");
        assert_eq!(
            e,
            EventEffect::ScrollUp {
                y1: 100,
                y2: 200,
                delay: 50,
                fadeawayheight: Some(15),
            }
        );
    }

    #[test]
    fn scroll_down_parses_same_shape_as_up() {
        let e = parse_effect_field("Scroll down;480;240;0;0");
        assert_eq!(
            e,
            EventEffect::ScrollDown {
                y1: 480,
                y2: 240,
                delay: 0,
                fadeawayheight: Some(0),
            }
        );
    }

    #[test]
    fn scroll_full_height_marker_recognised() {
        // y1 == y2 == 0 means "scroll the full height of the screen"
        // per the spec.
        let e = parse_effect_field("Scroll up;0;0;25");
        assert!(e.scrolls_full_height());
        assert_eq!(e.scroll_region(), Some((0, 0)));
    }

    #[test]
    fn scroll_region_accessor_normalises_top_and_bottom() {
        // Spec: "it doesn't matter which value (top or bottom) comes
        // first" — the accessor returns the smaller value first.
        let lo_first = parse_effect_field("Scroll up;100;200;5");
        let hi_first = parse_effect_field("Scroll up;200;100;5");
        assert_eq!(lo_first.scroll_region(), Some((100, 200)));
        assert_eq!(hi_first.scroll_region(), Some((100, 200)));
        // Banner has no Y region.
        assert_eq!(parse_effect_field("Banner;50").scroll_region(), None);
        assert_eq!(parse_effect_field("Karaoke").scroll_region(), None);
        assert_eq!(parse_effect_field("").scroll_region(), None);
    }

    #[test]
    fn delay_clamps_at_one_hundred() {
        let e = parse_effect_field("Scroll up;0;0;250");
        assert_eq!(
            e,
            EventEffect::ScrollUp {
                y1: 0,
                y2: 0,
                delay: 100,
                fadeawayheight: None,
            }
        );
    }

    #[test]
    fn negative_delay_collapses_to_other() {
        // Negative is outside the spec's `0..=100` slow-down knob.
        let raw = "Scroll up;0;0;-5";
        let e = parse_effect_field(raw);
        assert_eq!(e, EventEffect::Other(raw.to_string()));
    }

    #[test]
    fn missing_required_scroll_params_falls_back_to_other() {
        // Three params required (y1, y2, delay). Two means the line
        // is malformed and we keep the bytes.
        let raw = "Scroll up;0;0";
        assert_eq!(parse_effect_field(raw), EventEffect::Other(raw.to_string()));
        let raw = "Scroll up";
        assert_eq!(parse_effect_field(raw), EventEffect::Other(raw.to_string()));
    }

    #[test]
    fn banner_only_required_delay_parses() {
        assert_eq!(
            parse_effect_field("Banner;50"),
            EventEffect::Banner {
                delay: 50,
                direction: BannerDirection::RightToLeft,
                fadeawaywidth: None,
            }
        );
    }

    #[test]
    fn banner_with_lefttoright_flag_set() {
        assert_eq!(
            parse_effect_field("Banner;25;1"),
            EventEffect::Banner {
                delay: 25,
                direction: BannerDirection::LeftToRight,
                fadeawaywidth: None,
            }
        );
        assert_eq!(
            parse_effect_field("Banner;25;0"),
            EventEffect::Banner {
                delay: 25,
                direction: BannerDirection::RightToLeft,
                fadeawaywidth: None,
            }
        );
    }

    #[test]
    fn banner_with_fadeawaywidth() {
        assert_eq!(
            parse_effect_field("Banner;25;1;30"),
            EventEffect::Banner {
                delay: 25,
                direction: BannerDirection::LeftToRight,
                fadeawaywidth: Some(30),
            }
        );
    }

    #[test]
    fn banner_with_invalid_direction_falls_back_to_other() {
        // The flag is documented as `0` or `1` only; anything else is
        // out-of-spec and the column survives via Other so the caller
        // can decide how to react.
        let raw = "Banner;25;2";
        assert_eq!(parse_effect_field(raw), EventEffect::Other(raw.to_string()));
    }

    #[test]
    fn unknown_keyword_falls_back_to_other_keeping_bytes() {
        // Editor-specific scripted effects (e.g. fansub-author macros)
        // land here.
        let raw = "FadeIn;500";
        assert_eq!(parse_effect_field(raw), EventEffect::Other(raw.to_string()));
    }

    #[test]
    fn other_variant_trims_surrounding_whitespace() {
        // Internal whitespace stays put; only leading/trailing trim.
        assert_eq!(
            parse_effect_field("  custom thing  "),
            EventEffect::Other("custom thing".to_string())
        );
    }

    #[test]
    fn whitespace_before_semicolon_tolerated_for_keyword() {
        // `Scroll up ;0;0;5` is a common authoring artefact; the
        // spec wording is just "after the words Scroll up" so a
        // trailing space before the first semicolon is benign.
        let e = parse_effect_field("Scroll up ;0;0;5");
        assert_eq!(
            e,
            EventEffect::ScrollUp {
                y1: 0,
                y2: 0,
                delay: 5,
                fadeawayheight: None,
            }
        );
    }

    #[test]
    fn non_numeric_y_coordinate_falls_back_to_other() {
        let raw = "Scroll up;top;bottom;5";
        assert_eq!(parse_effect_field(raw), EventEffect::Other(raw.to_string()));
    }
}
