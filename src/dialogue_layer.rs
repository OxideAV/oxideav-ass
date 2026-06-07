//! Typed accessor for the per-event `Layer` column of a `Dialogue:`
//! line.
//!
//! The base [`parse`](crate::parse) entry point reads the dialogue
//! `Format:` row, splits each `Dialogue:` line on commas, and drops
//! the `Layer` column on the floor — the shared `SubtitleCue` IR
//! currently has no slot for the per-event render-order integer. The
//! round-trip writer fills the column with a literal `0`. That is
//! fine for the dominant case (every well-known export tool emits
//! `0` for unset layers), but it loses any per-line render-order the
//! original script requested.
//!
//! The SSA v4.x specification documents the column as:
//!
//! > *Layer (any integer). Subtitles having different layer numbers
//! > will be ignored during the collision detection. Higher numbered
//! > layers will be drawn over the lower numbered.*
//!
//! Two semantic facts fall out of that wording:
//!
//! * The column accepts **any** integer — including negative numbers
//!   (which the spec text does not forbid and which appear in
//!   hand-edited scripts as a deliberate "push behind everything else"
//!   choice). The typed surface uses `i32` so the round-trip stays
//!   exact across the negative-positive range.
//! * The column controls two distinct renderer behaviours:
//!   collision-detection grouping (lines that share a `Layer` collide;
//!   lines with different `Layer`s do not) and paint order (higher
//!   `Layer`s paint on top of lower `Layer`s). The typed accessor
//!   captures the raw signed integer so a renderer can implement both
//!   rules without re-parsing.
//!
//! [`parse_layer_field`] resolves the column into an [`LayerOverride`]
//! enum:
//!
//! * [`LayerOverride::Default`] — column was empty, whitespace-only,
//!   or the literal `0` (any sign / padding form). Equivalent to "no
//!   per-event override; use the base layer 0".
//! * [`LayerOverride::Layer(n)`] — column carried a non-zero signed
//!   integer. The value is exposed as `i32`.
//!
//! Malformed columns (non-numeric content, `i32` overflow) collapse to
//! [`LayerOverride::Default`] so the parser stays total — the
//! renderer falls back to layer 0, mirroring how the SSA reference
//! treats an unset event-layer column. Surrounding whitespace inside
//! the column is trimmed before the integer parse.
//!
//! Unlike the per-event margin columns, the `Layer` value is *not*
//! restricted to `0` shorthand padding — `0` / `+0` / `-0` are all
//! the same as no override, but the spec's "any integer" wording lets
//! the script emit a leading sign on a non-zero value (`-1`, `+3`),
//! so the parser accepts both.
//!
//! The parser is total — it never panics and never returns an error.
//! Negative values are preserved; overflow and stray non-digit
//! characters all decay to [`LayerOverride::Default`].

/// Typed view of the per-event `Layer` column on a `Dialogue:` line.
///
/// Produced by [`parse_layer_field`]. The two variants encode the
/// spec's two semantic states: "no per-event override; the base layer
/// is `0`" and "use this exact signed integer for collision grouping
/// and paint ordering". The [`Default`](LayerOverride::Default) impl
/// is `Default`, matching the dominant case in real scripts (every
/// well-known export tool emits `0` for the unset layer column).
///
/// The variant carries a signed `i32` because the SSA v4.x wording
/// is "any integer" — negative layers are legal and appear in
/// hand-authored scripts as a deliberate "draw behind everything
/// else" choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayerOverride {
    /// Column was empty, whitespace-only, or carried a literal `0`
    /// (in any sign / padding form: `0`, `+0`, `-0`). Equivalent to
    /// "no per-event override; the base layer is `0` for both
    /// collision grouping and paint ordering". Also the fall-back when
    /// the parse fails on a malformed column.
    #[default]
    Default,
    /// Column carried a non-zero signed integer. The renderer groups
    /// cues sharing this value for collision detection (cues at
    /// different `Layer`s ignore each other) and paints in ascending
    /// order (higher values on top).
    Layer(i32),
}

impl LayerOverride {
    /// Returns the signed layer value, if explicitly set. The
    /// [`Default`](LayerOverride::Default) variant returns `None` so
    /// the caller can substitute a fallback in a
    /// `event.layer.or(override.as_layer())` chain.
    #[inline]
    pub fn as_layer(self) -> Option<i32> {
        match self {
            LayerOverride::Default => None,
            LayerOverride::Layer(n) => Some(n),
        }
    }

    /// Resolve this override to the effective signed layer value the
    /// renderer should use. [`Default`](LayerOverride::Default) maps
    /// to `0` (the spec's base layer); [`Layer(n)`](LayerOverride::Layer)
    /// maps to `n`. This is the convenience accessor for the
    /// dominant render-loop path; pair it with a comparison against
    /// other cues' resolved layers to drive both collision grouping
    /// and paint order.
    #[inline]
    pub fn resolve(self) -> i32 {
        match self {
            LayerOverride::Default => 0,
            LayerOverride::Layer(n) => n,
        }
    }
}

/// Resolve the `Layer` column into a typed [`LayerOverride`].
///
/// The input is the raw bytes between two adjacent commas on a
/// `Dialogue:` line at the column position the `Format:` row labels
/// `Layer`. Empty / whitespace-only / the `0` literal (any sign or
/// padding form) all map to [`LayerOverride::Default`]; anything that
/// successfully parses as a non-zero signed integer maps to
/// [`LayerOverride::Layer`]. The parser is total — malformed input
/// falls back to [`LayerOverride::Default`] so the renderer
/// transparently uses the base layer `0`.
///
/// Surrounding whitespace inside the column is trimmed before
/// parsing. The spec describes the column as "any integer", so a
/// leading `+` / `-` is accepted; leading zeroes are allowed on the
/// magnitude and parsed as decimal (the spec does not call out any
/// non-decimal form).
///
/// # Examples
///
/// ```
/// use oxideav_ass::dialogue_layer::{parse_layer_field, LayerOverride};
///
/// // Empty column — no per-event override.
/// assert_eq!(parse_layer_field(""), LayerOverride::Default);
///
/// // The literal `0`, in every padding / sign form.
/// assert_eq!(parse_layer_field("0"), LayerOverride::Default);
/// assert_eq!(parse_layer_field("+0"), LayerOverride::Default);
/// assert_eq!(parse_layer_field("-0"), LayerOverride::Default);
///
/// // Explicit non-zero values, including negative.
/// assert_eq!(parse_layer_field("3"), LayerOverride::Layer(3));
/// assert_eq!(parse_layer_field("-1"), LayerOverride::Layer(-1));
/// assert_eq!(parse_layer_field("+5"), LayerOverride::Layer(5));
///
/// // Resolve to the effective render-order integer.
/// assert_eq!(parse_layer_field("").resolve(), 0);
/// assert_eq!(parse_layer_field("2").resolve(), 2);
/// assert_eq!(parse_layer_field("-1").resolve(), -1);
/// ```
pub fn parse_layer_field(field: &str) -> LayerOverride {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return LayerOverride::Default;
    }
    let n = match trimmed.parse::<i32>() {
        Ok(n) => n,
        Err(_) => return LayerOverride::Default,
    };
    if n == 0 {
        LayerOverride::Default
    } else {
        LayerOverride::Layer(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_column_is_default() {
        assert_eq!(parse_layer_field(""), LayerOverride::Default);
    }

    #[test]
    fn whitespace_only_is_default() {
        assert_eq!(parse_layer_field("   "), LayerOverride::Default);
        assert_eq!(parse_layer_field("\t"), LayerOverride::Default);
    }

    #[test]
    fn zero_literal_is_default_in_every_sign_form() {
        // `0`, `+0`, `-0` are all the same as no override.
        for raw in ["0", "+0", "-0"] {
            assert_eq!(
                parse_layer_field(raw),
                LayerOverride::Default,
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn explicit_positive_integer() {
        assert_eq!(parse_layer_field("1"), LayerOverride::Layer(1));
        assert_eq!(parse_layer_field("3"), LayerOverride::Layer(3));
        assert_eq!(parse_layer_field("999"), LayerOverride::Layer(999));
    }

    #[test]
    fn explicit_negative_integer() {
        // The spec's wording is "any integer" — negative values are
        // allowed and appear in hand-edited scripts.
        assert_eq!(parse_layer_field("-1"), LayerOverride::Layer(-1));
        assert_eq!(parse_layer_field("-50"), LayerOverride::Layer(-50));
    }

    #[test]
    fn explicit_plus_sign_accepted_for_non_zero() {
        // The spec does not forbid a leading `+` for a positive
        // value; round-trip stays well-defined.
        assert_eq!(parse_layer_field("+5"), LayerOverride::Layer(5));
        assert_eq!(parse_layer_field("+1"), LayerOverride::Layer(1));
    }

    #[test]
    fn leading_zeroes_preserved_as_decimal_value() {
        // The column is parsed as decimal, not octal — `007` is `7`.
        assert_eq!(parse_layer_field("007"), LayerOverride::Layer(7));
        assert_eq!(parse_layer_field("0010"), LayerOverride::Layer(10));
        assert_eq!(parse_layer_field("-007"), LayerOverride::Layer(-7));
    }

    #[test]
    fn surrounding_whitespace_trimmed() {
        // Real-world authoring tools sometimes pad the CSV column
        // with a trailing space.
        assert_eq!(parse_layer_field("  3  "), LayerOverride::Layer(3));
        assert_eq!(parse_layer_field("\t-1"), LayerOverride::Layer(-1));
    }

    #[test]
    fn non_numeric_collapses_to_default() {
        // Hex / decimal / scientific / alpha all fall back to the
        // base layer — the parser stays total.
        for raw in ["abc", "1.5", "0xFF", "1e3", "5px", "hi"] {
            assert_eq!(
                parse_layer_field(raw),
                LayerOverride::Default,
                "raw = {raw:?}"
            );
        }
    }

    #[test]
    fn overflowing_value_collapses_to_default() {
        // `i32::MAX` is 2_147_483_647. Anything larger fails the
        // parse → the renderer keeps the base layer.
        assert_eq!(
            parse_layer_field("9999999999999999"),
            LayerOverride::Default
        );
        assert_eq!(
            parse_layer_field("-9999999999999999"),
            LayerOverride::Default
        );
    }

    #[test]
    fn boundary_values_round_trip() {
        // `i32::MIN` and `i32::MAX` are inside the typed range.
        assert_eq!(
            parse_layer_field("2147483647"),
            LayerOverride::Layer(i32::MAX)
        );
        assert_eq!(
            parse_layer_field("-2147483648"),
            LayerOverride::Layer(i32::MIN)
        );
    }

    #[test]
    fn as_layer_accessor_returns_none_for_default() {
        assert_eq!(parse_layer_field("0").as_layer(), None);
        assert_eq!(parse_layer_field("").as_layer(), None);
        assert_eq!(parse_layer_field("not-a-number").as_layer(), None);
    }

    #[test]
    fn as_layer_accessor_returns_some_for_explicit_value() {
        assert_eq!(parse_layer_field("3").as_layer(), Some(3));
        assert_eq!(parse_layer_field("-2").as_layer(), Some(-2));
    }

    #[test]
    fn resolve_default_maps_to_zero() {
        // The spec's base layer is `0`; `Default` resolves to it.
        assert_eq!(parse_layer_field("").resolve(), 0);
        assert_eq!(parse_layer_field("0").resolve(), 0);
        assert_eq!(parse_layer_field("-0").resolve(), 0);
        assert_eq!(parse_layer_field("garbage").resolve(), 0);
    }

    #[test]
    fn resolve_explicit_value_maps_through() {
        assert_eq!(parse_layer_field("5").resolve(), 5);
        assert_eq!(parse_layer_field("-3").resolve(), -3);
    }

    #[test]
    fn default_trait_matches_empty_column() {
        // `LayerOverride::default()` is the same as an empty column.
        assert_eq!(LayerOverride::default(), LayerOverride::Default);
        assert_eq!(LayerOverride::default(), parse_layer_field(""));
    }

    #[test]
    fn copy_eq_traits_are_usable() {
        // The type is `Copy` + `Eq` — values can be matched / passed
        // freely without explicit clones.
        let a = parse_layer_field("7");
        let b = a;
        assert_eq!(a, b);
        match a {
            LayerOverride::Layer(n) => assert_eq!(n, 7),
            LayerOverride::Default => panic!("expected explicit layer"),
        }
    }

    #[test]
    fn collision_grouping_two_layers_compare_distinctly() {
        // The spec rule "cues at different `Layer`s ignore each other
        // for collision detection" lands as a simple `==` over
        // `resolve()`. Document the expected ergonomic here.
        let a = parse_layer_field("1");
        let b = parse_layer_field("2");
        let c = parse_layer_field("1");
        assert_ne!(a.resolve(), b.resolve(), "different layers do not collide");
        assert_eq!(a.resolve(), c.resolve(), "same layer collides");
    }

    #[test]
    fn paint_order_ascending_by_resolved_layer() {
        // Higher `Layer` paints on top: `Ord` over the resolved
        // integer is the renderer's z-sort key.
        let mut cues = [
            parse_layer_field("2").resolve(),
            parse_layer_field("-1").resolve(),
            parse_layer_field("0").resolve(),
            parse_layer_field("3").resolve(),
        ];
        cues.sort();
        assert_eq!(cues, [-1, 0, 2, 3]);
    }

    #[test]
    fn just_plus_or_minus_collapses_to_default() {
        // A bare sign is not a valid integer; total-parser fall-back.
        assert_eq!(parse_layer_field("+"), LayerOverride::Default);
        assert_eq!(parse_layer_field("-"), LayerOverride::Default);
    }
}
