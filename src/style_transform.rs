//! Typed accessors for the per-style geometry columns of a
//! `[V4+ Styles]` `Style:` definition: `ScaleX`, `ScaleY`, `Spacing`,
//! and `Angle`.
//!
//! The base [`parse`](crate::parse) entry point reads the styles
//! `Format:` row, splits each `Style:` line on commas, and decodes the
//! columns the shared `SubtitleStyle` IR has a slot for (name, font,
//! sizes, colours, the bold / italic / underline / strikeout flags,
//! alignment, margins, outline and shadow widths). The four geometry
//! columns are read past — the shared IR carries no field for the
//! per-style horizontal / vertical scale, the inter-character spacing,
//! or the baseline rotation, so a renderer that needs them has to
//! re-split the `Style:` row itself.
//!
//! These four columns are the *style-level* counterparts of override
//! tags that already surface through the [`animate`](crate::animate)
//! module on a per-segment basis: `ScaleX` / `ScaleY` mirror the
//! `\fscx` / `\fscy` font-scale overrides, `Spacing` mirrors the `\fsp`
//! letter-spacing override, and `Angle` mirrors the `\frz` Z-rotation
//! override. The override form, when present on a dialogue segment,
//! takes precedence over the style column; the style column supplies
//! the per-line baseline.
//!
//! The SSA v4.x / ASS specification documents the four columns as:
//!
//! > *ScaleX. Modifies the width of the font. \[percent]*
//! > *ScaleY. Modifies the height of the font. \[percent]*
//! > *Spacing. Extra space between characters. \[pixels]*
//! > *Angle. The origin of the rotation is defined by the alignment.
//! > Can be a floating point number. \[degrees]*
//!
//! The spec gives the units but does not pin an explicit default value
//! for a missing column. The neutral / identity transform is the
//! obvious fall-back: a `100` percent scale leaves the glyph
//! unmodified, a `0` pixel spacing adds no inter-character gap, and a
//! `0` degree angle applies no rotation. Each accessor resolves an
//! empty, whitespace-only, or malformed column to that identity value
//! so the parser stays total.

/// Typed view of the four per-style geometry columns on a
/// `[V4+ Styles]` `Style:` line: [`scale_x`](StyleTransform::scale_x),
/// [`scale_y`](StyleTransform::scale_y),
/// [`spacing`](StyleTransform::spacing), and
/// [`angle`](StyleTransform::angle).
///
/// Produced field-by-field with [`parse_scale_field`],
/// [`parse_spacing_field`], and [`parse_angle_field`], or all four at
/// once from the matching `Format:` columns with
/// [`parse_style_transform`].
///
/// The [`Default`] impl is the identity transform: `100` percent on
/// both scale axes, `0` pixel spacing, `0` degree angle. This is the
/// resolved value when every column is absent, and it leaves a glyph
/// rendered exactly as its font + size dictate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StyleTransform {
    /// Horizontal font scale as a percentage (`100.0` = unscaled).
    /// From the `ScaleX` column; the spec describes it as *"modifies
    /// the width of the font \[percent]"*. The per-segment `\fscx`
    /// override takes precedence when present.
    pub scale_x: f64,
    /// Vertical font scale as a percentage (`100.0` = unscaled). From
    /// the `ScaleY` column; the spec describes it as *"modifies the
    /// height of the font \[percent]"*. The per-segment `\fscy`
    /// override takes precedence when present.
    pub scale_y: f64,
    /// Extra space between characters in pixels (`0.0` = none). From
    /// the `Spacing` column. May be negative to tighten the text. The
    /// per-segment `\fsp` override takes precedence when present.
    pub spacing: f64,
    /// Baseline rotation in degrees (`0.0` = upright). From the `Angle`
    /// column; the spec notes *"the origin of the rotation is defined
    /// by the alignment"* and that it *"can be a floating point
    /// number"*. The per-segment `\frz` override takes precedence when
    /// present.
    pub angle: f64,
}

impl Default for StyleTransform {
    #[inline]
    fn default() -> Self {
        StyleTransform {
            scale_x: 100.0,
            scale_y: 100.0,
            spacing: 0.0,
            angle: 0.0,
        }
    }
}

impl StyleTransform {
    /// Whether this transform is the identity (no scaling, no spacing,
    /// no rotation). A renderer can skip the per-glyph transform step
    /// entirely when this holds.
    #[inline]
    pub fn is_identity(self) -> bool {
        self.scale_x == 100.0 && self.scale_y == 100.0 && self.spacing == 0.0 && self.angle == 0.0
    }
}

/// Parse a finite floating-point geometry column, returning `None` for
/// an empty / whitespace-only / non-numeric / non-finite column so the
/// caller can substitute the field's identity default.
///
/// A leading `+` and a leading-zero magnitude are tolerated (decimal,
/// never octal). `NaN` / `inf` spellings and overflow to a non-finite
/// value are rejected so the resolved transform always carries finite
/// numbers.
fn parse_finite(field: &str) -> Option<f64> {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse::<f64>() {
        Ok(v) if v.is_finite() => Some(v),
        _ => None,
    }
}

/// Resolve a `ScaleX` or `ScaleY` column into a percentage.
///
/// The input is the raw bytes between two adjacent commas on a
/// `Style:` line at the column position the styles `Format:` row labels
/// `ScaleX` / `ScaleY`. The spec describes the value as a percent that
/// *"modifies the width / height of the font"*. The parser is total —
/// an empty, whitespace-only, non-numeric, or non-finite column falls
/// back to `100.0` (the identity scale, i.e. the glyph rendered at its
/// natural font size).
///
/// Surrounding whitespace inside the column is trimmed before parsing.
/// A leading `+` and a leading-zero magnitude are accepted; the value
/// may be fractional (e.g. `87.5`) per the floating-point grammar the
/// spec uses for the neighbouring `Angle` column.
///
/// # Examples
///
/// ```
/// use oxideav_ass::style_transform::parse_scale_field;
///
/// assert_eq!(parse_scale_field("100"), 100.0);
/// assert_eq!(parse_scale_field("87.5"), 87.5);
/// // Missing / malformed columns fall back to the identity scale.
/// assert_eq!(parse_scale_field(""), 100.0);
/// assert_eq!(parse_scale_field("wide"), 100.0);
/// ```
#[inline]
pub fn parse_scale_field(field: &str) -> f64 {
    parse_finite(field).unwrap_or(100.0)
}

/// Resolve the `Spacing` column into a pixel count.
///
/// The spec describes the value as *"extra space between characters
/// \[pixels]"*. The parser is total — an empty, whitespace-only,
/// non-numeric, or non-finite column falls back to `0.0` (no extra
/// spacing). The value may be negative to tighten the text below its
/// natural advance, and may be fractional.
///
/// # Examples
///
/// ```
/// use oxideav_ass::style_transform::parse_spacing_field;
///
/// assert_eq!(parse_spacing_field("0"), 0.0);
/// assert_eq!(parse_spacing_field("2.5"), 2.5);
/// assert_eq!(parse_spacing_field("-1"), -1.0);
/// // Missing / malformed columns fall back to no spacing.
/// assert_eq!(parse_spacing_field(""), 0.0);
/// ```
#[inline]
pub fn parse_spacing_field(field: &str) -> f64 {
    parse_finite(field).unwrap_or(0.0)
}

/// Resolve the `Angle` column into degrees.
///
/// The spec describes the value as a rotation in degrees whose origin
/// *"is defined by the alignment"* and notes it *"can be a floating
/// point number"*. The parser is total — an empty, whitespace-only,
/// non-numeric, or non-finite column falls back to `0.0` (upright, no
/// rotation). The value may be negative (clockwise vs counter-clockwise
/// per the renderer's convention) and fractional.
///
/// # Examples
///
/// ```
/// use oxideav_ass::style_transform::parse_angle_field;
///
/// assert_eq!(parse_angle_field("0"), 0.0);
/// assert_eq!(parse_angle_field("45"), 45.0);
/// assert_eq!(parse_angle_field("-12.5"), -12.5);
/// // Missing / malformed columns fall back to upright.
/// assert_eq!(parse_angle_field(""), 0.0);
/// ```
#[inline]
pub fn parse_angle_field(field: &str) -> f64 {
    parse_finite(field).unwrap_or(0.0)
}

/// Resolve all four geometry columns at once into a [`StyleTransform`].
///
/// Pass the raw column bytes in `Format:` order: `ScaleX`, `ScaleY`,
/// `Spacing`, `Angle`. Each column is resolved independently with the
/// matching per-field parser, so a malformed column only resets its own
/// axis to the identity value — the other three still parse.
///
/// # Examples
///
/// ```
/// use oxideav_ass::style_transform::{parse_style_transform, StyleTransform};
///
/// let t = parse_style_transform("110", "90", "1.5", "30");
/// assert_eq!(t.scale_x, 110.0);
/// assert_eq!(t.scale_y, 90.0);
/// assert_eq!(t.spacing, 1.5);
/// assert_eq!(t.angle, 30.0);
///
/// // Each axis falls back independently.
/// let t = parse_style_transform("", "x", "", "");
/// assert_eq!(t, StyleTransform::default());
/// ```
#[inline]
pub fn parse_style_transform(
    scale_x: &str,
    scale_y: &str,
    spacing: &str,
    angle: &str,
) -> StyleTransform {
    StyleTransform {
        scale_x: parse_scale_field(scale_x),
        scale_y: parse_scale_field(scale_y),
        spacing: parse_spacing_field(spacing),
        angle: parse_angle_field(angle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_plain_integers() {
        assert_eq!(parse_scale_field("100"), 100.0);
        assert_eq!(parse_scale_field("50"), 50.0);
        assert_eq!(parse_scale_field("200"), 200.0);
        assert_eq!(parse_scale_field("0"), 0.0);
    }

    #[test]
    fn scale_fractional() {
        assert_eq!(parse_scale_field("87.5"), 87.5);
        assert_eq!(parse_scale_field("133.33"), 133.33);
    }

    #[test]
    fn scale_empty_and_whitespace_default_100() {
        assert_eq!(parse_scale_field(""), 100.0);
        assert_eq!(parse_scale_field("   "), 100.0);
        assert_eq!(parse_scale_field("\t"), 100.0);
    }

    #[test]
    fn scale_surrounding_whitespace_trimmed() {
        assert_eq!(parse_scale_field("  120  "), 120.0);
        assert_eq!(parse_scale_field("\t80"), 80.0);
    }

    #[test]
    fn scale_leading_plus_and_zero() {
        assert_eq!(parse_scale_field("+100"), 100.0);
        assert_eq!(parse_scale_field("0100"), 100.0);
        assert_eq!(parse_scale_field("007.5"), 7.5);
    }

    #[test]
    fn scale_non_numeric_defaults_100() {
        for raw in ["wide", "100%", "1e", "0x64", "abc"] {
            assert_eq!(parse_scale_field(raw), 100.0, "raw = {raw:?}");
        }
    }

    #[test]
    fn scale_non_finite_defaults_100() {
        for raw in ["nan", "NaN", "inf", "-inf", "infinity"] {
            assert_eq!(parse_scale_field(raw), 100.0, "raw = {raw:?}");
        }
    }

    #[test]
    fn spacing_plain_and_negative() {
        assert_eq!(parse_spacing_field("0"), 0.0);
        assert_eq!(parse_spacing_field("3"), 3.0);
        assert_eq!(parse_spacing_field("-2"), -2.0);
        assert_eq!(parse_spacing_field("1.25"), 1.25);
        assert_eq!(parse_spacing_field("-0.5"), -0.5);
    }

    #[test]
    fn spacing_empty_and_malformed_default_0() {
        assert_eq!(parse_spacing_field(""), 0.0);
        assert_eq!(parse_spacing_field("  "), 0.0);
        assert_eq!(parse_spacing_field("gap"), 0.0);
        assert_eq!(parse_spacing_field("nan"), 0.0);
    }

    #[test]
    fn angle_plain_and_signed() {
        assert_eq!(parse_angle_field("0"), 0.0);
        assert_eq!(parse_angle_field("45"), 45.0);
        assert_eq!(parse_angle_field("360"), 360.0);
        assert_eq!(parse_angle_field("-12.5"), -12.5);
        assert_eq!(parse_angle_field("+90"), 90.0);
    }

    #[test]
    fn angle_empty_and_malformed_default_0() {
        assert_eq!(parse_angle_field(""), 0.0);
        assert_eq!(parse_angle_field("\t"), 0.0);
        assert_eq!(parse_angle_field("spin"), 0.0);
        assert_eq!(parse_angle_field("inf"), 0.0);
    }

    #[test]
    fn angle_fractional_round_trip() {
        assert_eq!(parse_angle_field("22.75"), 22.75);
    }

    #[test]
    fn struct_all_four_columns() {
        let t = parse_style_transform("110", "90", "1.5", "30");
        assert_eq!(
            t,
            StyleTransform {
                scale_x: 110.0,
                scale_y: 90.0,
                spacing: 1.5,
                angle: 30.0,
            }
        );
    }

    #[test]
    fn struct_each_axis_falls_back_independently() {
        // Second + fourth columns malformed; first + third still parse.
        let t = parse_style_transform("75", "bad", "4", "");
        assert_eq!(t.scale_x, 75.0);
        assert_eq!(t.scale_y, 100.0);
        assert_eq!(t.spacing, 4.0);
        assert_eq!(t.angle, 0.0);
    }

    #[test]
    fn default_is_identity_transform() {
        let d = StyleTransform::default();
        assert_eq!(d.scale_x, 100.0);
        assert_eq!(d.scale_y, 100.0);
        assert_eq!(d.spacing, 0.0);
        assert_eq!(d.angle, 0.0);
        assert!(d.is_identity());
    }

    #[test]
    fn all_empty_columns_resolve_to_default() {
        assert_eq!(
            parse_style_transform("", "", "", ""),
            StyleTransform::default()
        );
    }

    #[test]
    fn is_identity_detects_non_identity() {
        assert!(!parse_style_transform("110", "100", "0", "0").is_identity());
        assert!(!parse_style_transform("100", "100", "0", "5").is_identity());
        assert!(!parse_style_transform("100", "100", "2", "0").is_identity());
        assert!(parse_style_transform("100", "100", "0", "0").is_identity());
    }

    #[test]
    fn copy_and_clone_ergonomics() {
        let a = parse_style_transform("120", "120", "1", "10");
        let b = a;
        assert_eq!(a, b);
        let c = a;
        assert_eq!(c.scale_x, 120.0);
    }

    #[test]
    fn resolved_values_are_always_finite() {
        for raw in ["", "x", "nan", "inf", "-inf", "1e400", "100", "-5.5"] {
            assert!(parse_scale_field(raw).is_finite(), "scale raw = {raw:?}");
            assert!(
                parse_spacing_field(raw).is_finite(),
                "spacing raw = {raw:?}"
            );
            assert!(parse_angle_field(raw).is_finite(), "angle raw = {raw:?}");
        }
    }

    #[test]
    fn overflow_to_non_finite_defaults() {
        // A magnitude that overflows f64 parses to infinity and is
        // rejected back to the identity default.
        assert_eq!(parse_scale_field("1e400"), 100.0);
        assert_eq!(parse_spacing_field("1e400"), 0.0);
        assert_eq!(parse_angle_field("-1e400"), 0.0);
    }
}
