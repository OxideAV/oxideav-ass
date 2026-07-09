//! Lossless structured ASS/SSA script document model.
//!
//! The crate's base [`parse`](crate::parse) / [`write`](crate::write)
//! pair targets the shared `oxideav-core` subtitle IR
//! (`SubtitleTrack` / `SubtitleCue` / `SubtitleStyle`). That IR is
//! deliberately format-agnostic, so several ASS/SSA-specific columns
//! have no slot in it (per-event `Layer` / `Name` / `Effect` /
//! per-event margins, the per-style `ScaleX` / `ScaleY` / `Spacing` /
//! `Angle` / `BorderStyle` / `Encoding` columns, the SSA-era
//! `SecondaryColour` / `AlphaLevel`, the event kind, …). The base
//! parser surfaces those through standalone typed-accessor modules
//! ([`crate::dialogue_layer`], [`crate::style_transform`], …) and keeps
//! the original header verbatim in `extradata` so a round-trip replays
//! it untouched.
//!
//! This module offers the complementary *structured* path: a fully
//! typed document model that captures every field of every line so a
//! caller can read, edit, and re-serialise an ASS/SSA script with
//! field-level fidelity — without the IR's lossy projection and without
//! depending on the verbatim-`extradata` replay trick.
//!
//! ```
//! let bytes = b"[Script Info]\n\
//! ScriptType: v4.00+\n\
//! \n\
//! [V4+ Styles]\n\
//! Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
//! Style: Default,Arial,20,&H00FFFFFF,&H00000000,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,10,1\n\
//! \n\
//! [Events]\n\
//! Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
//! Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,hello\n";
//! let script = oxideav_ass::parse_script(bytes);
//! assert_eq!(script.styles().len(), 1);
//! assert_eq!(script.events().len(), 1);
//! // Serialising back produces a byte-faithful, re-parseable script.
//! let out = script.serialise();
//! let reparsed = oxideav_ass::parse_script(&out);
//! assert_eq!(reparsed.events().len(), 1);
//! ```
//!
//! Field meanings follow the SSA v4.00+ script-format specification and
//! its `[v4 Styles]` / `[v4 Styles+]` style-line definitions.

use crate::dialogue_layer::{parse_layer_field, LayerOverride};
use crate::dialogue_margin::{parse_margin_field, MarginOverride};
use crate::event_effect::{parse_effect_field, EventEffect};
use crate::style_border::{parse_border_style_field, BorderStyle};
use crate::style_encoding::{parse_encoding_field, StyleEncoding};

/// One parsed ASS/SSA script section, preserved in source order.
///
/// The document keeps the original order and grouping of sections so a
/// serialise step can replay them where the modelled-section data is
/// not enough on its own (editor-private blocks like `[Aegisub Project
/// Garbage]`, the UU-encoded `[Fonts]` / `[Graphics]` bodies).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Section {
    /// `[Script Info]` — ordered key/value (and comment) header lines.
    ScriptInfo(ScriptInfo),
    /// `[V4+ Styles]` (ASS) or `[V4 Styles]` (legacy SSA) — a style
    /// table with its `Format:` order and the decoded style rows.
    Styles(StyleTable),
    /// `[Events]` — the event table with its `Format:` order and the
    /// decoded event rows.
    Events(EventTable),
    /// Any other section (`[Fonts]`, `[Graphics]`, `[Aegisub …]`, …)
    /// preserved verbatim by header name + raw body lines.
    Raw(RawSection),
}

/// `[Script Info]` header lines, kept in source order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptInfo {
    /// Ordered header lines. Each entry is either a `Key: Value` pair
    /// or a verbatim comment / blank line (see [`InfoLine`]).
    pub lines: Vec<InfoLine>,
}

/// A single line inside the `[Script Info]` section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InfoLine {
    /// A `Key: Value` header entry. `key` keeps its original casing /
    /// spelling so a round-trip emits it unchanged.
    Pair { key: String, value: String },
    /// A comment line (`;` or `!:` prefix) preserved verbatim
    /// (including the prefix character).
    Comment(String),
    /// An empty line preserved so blank-line spacing round-trips.
    Blank,
}

impl ScriptInfo {
    /// Look up the first value for a header key (case-insensitive).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().find_map(|l| match l {
            InfoLine::Pair { key: k, value } if k.eq_ignore_ascii_case(key) => Some(value.as_str()),
            _ => None,
        })
    }

    /// `true` when the `ScriptType` header names a v4.00+ (ASS) script.
    /// A missing header, or one naming `v4.00` (legacy SSA), returns
    /// `false`.
    pub fn is_ass(&self) -> bool {
        self.get("ScriptType")
            .map(|v| v.trim().contains('+'))
            .unwrap_or(false)
    }

    /// Typed [`WrapStyle`](crate::script_info::WrapStyle) for the
    /// `WrapStyle` header. A missing header resolves to the spec's
    /// default smart-even wrapping mode.
    pub fn wrap_style(&self) -> crate::script_info::WrapStyle {
        crate::script_info::parse_wrap_style_field(self.get("WrapStyle").unwrap_or(""))
    }

    /// Typed [`Collisions`](crate::script_info::Collisions) policy for the
    /// `Collisions` header. A missing header resolves to the spec's
    /// default `Normal` policy.
    pub fn collisions(&self) -> crate::script_info::Collisions {
        crate::script_info::parse_collisions_field(self.get("Collisions").unwrap_or(""))
    }

    /// Script-resolution width from the `PlayResX` header, or [`None`]
    /// when the header is absent or carries a non-positive / malformed
    /// value (the caller falls back to the video resolution).
    pub fn play_res_x(&self) -> Option<u32> {
        crate::script_info::parse_play_res_field(self.get("PlayResX")?)
    }

    /// Script-resolution height from the `PlayResY` header, or [`None`]
    /// when the header is absent or carries a non-positive / malformed
    /// value (the caller falls back to the video resolution).
    pub fn play_res_y(&self) -> Option<u32> {
        crate::script_info::parse_play_res_field(self.get("PlayResY")?)
    }

    /// Colour depth (bits) from the `PlayDepth` header, or [`None`] when
    /// the header is absent or malformed.
    pub fn play_depth(&self) -> Option<u32> {
        crate::script_info::parse_play_depth_field(self.get("PlayDepth")?)
    }

    /// Playback timer speed as a fractional multiplier from the `Timer`
    /// header (the documented percentage divided by 100; `"100.0000"` →
    /// `1.0`). A missing or malformed header resolves to `1.0` (100%).
    pub fn timer(&self) -> f64 {
        crate::script_info::parse_timer_field(self.get("Timer").unwrap_or(""))
    }
}

/// A style table: the `Format:` field order plus the decoded rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StyleTable {
    /// `true` for an ASS `[V4+ Styles]` section, `false` for a legacy
    /// SSA `[V4 Styles]` section. The two dialects carry slightly
    /// different `Format:` columns (SSA has `SecondaryColour` /
    /// `TertiaryColour` / `AlphaLevel`, ASS has `OutlineColour` /
    /// `ScaleX` / `ScaleY` / `Spacing` / `Angle`).
    pub ass: bool,
    /// The `Format:` row, field names in order.
    pub format: Vec<String>,
    /// Decoded style rows in source order.
    pub styles: Vec<StyleDef>,
}

/// A fully-typed `Style:` row.
///
/// Every column the SSA v4.x / ASS spec defines is captured. Columns
/// absent from a given dialect's `Format:` row keep their type default.
/// Colour columns keep the raw `&HAABBGGRR` wire token so a round-trip
/// re-emits the author's exact spelling (including leading-zero / case
/// variations) rather than a canonicalised form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StyleDef {
    pub name: String,
    pub fontname: String,
    pub fontsize: String,
    /// `PrimaryColour` — main fill, raw wire token.
    pub primary_colour: String,
    /// `SecondaryColour` (SSA) — karaoke pre-highlight colour, raw token.
    pub secondary_colour: String,
    /// `OutlineColour` (ASS) / `TertiaryColour` (SSA) — outline, raw token.
    pub outline_colour: String,
    /// `BackColour` — shadow / box-backdrop colour, raw token.
    pub back_colour: String,
    /// `-1` / `0` (or weight for `Bold`) kept as the raw integer token.
    pub bold: String,
    pub italic: String,
    pub underline: String,
    pub strikeout: String,
    pub scale_x: String,
    pub scale_y: String,
    pub spacing: String,
    pub angle: String,
    /// `BorderStyle` column raw token (`1` outline+shadow / `3` box).
    pub border_style: String,
    pub outline: String,
    pub shadow: String,
    /// `Alignment` raw token (numpad for ASS, bit scheme for SSA).
    pub alignment: String,
    pub margin_l: String,
    pub margin_r: String,
    pub margin_v: String,
    /// SSA-only `AlphaLevel` column (transparency), raw token.
    pub alpha_level: String,
    pub encoding: String,
}

impl StyleDef {
    /// Typed [`BorderStyle`] for the `BorderStyle` column.
    pub fn border_style_typed(&self) -> BorderStyle {
        parse_border_style_field(&self.border_style)
    }

    /// Typed [`StyleEncoding`] for the `Encoding` column.
    pub fn encoding_typed(&self) -> StyleEncoding {
        parse_encoding_field(&self.encoding)
    }

    /// Typed [`StyleAlignment`](crate::style_alignment::StyleAlignment)
    /// for the `Alignment` column.
    ///
    /// `is_ssa` selects the numbering scheme: pass `true` for a legacy
    /// `[V4 Styles]` table (the bit scheme — `1`/`2`/`3` = L/C/R,
    /// `+4` toptitle, `+8` midtitle) and `false` for an ASS
    /// `[V4+ Styles]` table (the numpad `1..=9` scheme). The caller
    /// reads the owning [`StyleTable::ass`] flag and passes its negation
    /// (`!table.ass`). Both schemes normalise to the same
    /// [`StyleAlignment`](crate::style_alignment::StyleAlignment), so a
    /// renderer reasons about one anchor model regardless of dialect.
    pub fn alignment_typed(&self, is_ssa: bool) -> crate::style_alignment::StyleAlignment {
        crate::style_alignment::parse_alignment_field(&self.alignment, is_ssa)
    }

    /// Typed [`StyleTransform`](crate::style_transform::StyleTransform)
    /// lifting the `ScaleX` / `ScaleY` / `Spacing` / `Angle` columns at
    /// once. These are the style-level baselines the per-segment
    /// `\fscx` / `\fscy` / `\fsp` / `\frz` override tags supersede.
    pub fn transform_typed(&self) -> crate::style_transform::StyleTransform {
        crate::style_transform::parse_style_transform(
            &self.scale_x,
            &self.scale_y,
            &self.spacing,
            &self.angle,
        )
    }

    /// Typed per-style margins `(MarginL, MarginR, MarginV)`, each a
    /// [`MarginOverride`](crate::dialogue_margin::MarginOverride). A
    /// per-event margin column supersedes the matching style margin when
    /// present; this accessor surfaces the style-level baseline.
    pub fn margins_typed(
        &self,
    ) -> (
        crate::dialogue_margin::MarginOverride,
        crate::dialogue_margin::MarginOverride,
        crate::dialogue_margin::MarginOverride,
    ) {
        (
            crate::dialogue_margin::parse_margin_field(&self.margin_l),
            crate::dialogue_margin::parse_margin_field(&self.margin_r),
            crate::dialogue_margin::parse_margin_field(&self.margin_v),
        )
    }

    /// `PrimaryColour` decoded to `(r, g, b, a)` with the ASS alpha
    /// inversion already applied (`a = 255 - wire_alpha`), or `None` when
    /// the wire token is malformed.
    pub fn primary_colour_typed(&self) -> Option<(u8, u8, u8, u8)> {
        crate::parse_ass_color(&self.primary_colour)
    }

    /// `SecondaryColour` (the SSA karaoke pre-highlight colour) decoded
    /// to `(r, g, b, a)`, or `None` when malformed.
    pub fn secondary_colour_typed(&self) -> Option<(u8, u8, u8, u8)> {
        crate::parse_ass_color(&self.secondary_colour)
    }

    /// `OutlineColour` (ASS) / `TertiaryColour` (SSA) decoded to
    /// `(r, g, b, a)`, or `None` when malformed.
    pub fn outline_colour_typed(&self) -> Option<(u8, u8, u8, u8)> {
        crate::parse_ass_color(&self.outline_colour)
    }

    /// `BackColour` (the shadow / opaque-box backdrop colour) decoded to
    /// `(r, g, b, a)`, or `None` when malformed.
    pub fn back_colour_typed(&self) -> Option<(u8, u8, u8, u8)> {
        crate::parse_ass_color(&self.back_colour)
    }

    /// `true` when the `Bold` column is set. The SSA wire convention is
    /// `-1` for true / `0` for false; a non-zero weight (e.g. `700`) also
    /// reads as bold.
    pub fn bold_typed(&self) -> bool {
        crate::parse_bool_flag(&self.bold)
    }

    /// `true` when the `Italic` column is set (SSA `-1` / `0`).
    pub fn italic_typed(&self) -> bool {
        crate::parse_bool_flag(&self.italic)
    }

    /// `true` when the `Underline` column is set (SSA `-1` / `0`).
    pub fn underline_typed(&self) -> bool {
        crate::parse_bool_flag(&self.underline)
    }

    /// `true` when the `StrikeOut` column is set (SSA `-1` / `0`).
    pub fn strikeout_typed(&self) -> bool {
        crate::parse_bool_flag(&self.strikeout)
    }

    /// `Fontsize` parsed to an `f64`, or `None` when the column is empty
    /// or non-numeric.
    pub fn fontsize_typed(&self) -> Option<f64> {
        let t = self.fontsize.trim();
        t.parse::<f64>().ok().filter(|v| v.is_finite())
    }

    /// `Outline` (border width) parsed to an `f64`, or `None` when the
    /// column is empty or non-numeric.
    pub fn outline_typed(&self) -> Option<f64> {
        let t = self.outline.trim();
        t.parse::<f64>().ok().filter(|v| v.is_finite())
    }

    /// `Shadow` (drop-shadow distance) parsed to an `f64`, or `None` when
    /// the column is empty or non-numeric.
    pub fn shadow_typed(&self) -> Option<f64> {
        let t = self.shadow.trim();
        t.parse::<f64>().ok().filter(|v| v.is_finite())
    }
}

/// An event table: the `Format:` field order plus the decoded rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventTable {
    /// The `Format:` row, field names in order.
    pub format: Vec<String>,
    /// Decoded event rows in source order (Dialogue, Comment, …).
    pub events: Vec<Event>,
}

/// The line descriptor of an `[Events]` row.
///
/// The SSA v4.x spec lists six event line types. They all share the
/// same field layout; only the descriptor and the rendering semantics
/// differ.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EventKind {
    /// `Dialogue:` — subtitle text.
    #[default]
    Dialogue,
    /// `Comment:` — same fields, ignored during playback.
    Comment,
    /// `Picture:` — `Text` field holds a picture path.
    Picture,
    /// `Sound:` — `Text` field holds a wav path.
    Sound,
    /// `Movie:` — `Text` field holds an avi path.
    Movie,
    /// `Command:` — `Text` field holds a program / `SSA:` command.
    Command,
}

impl EventKind {
    /// The line descriptor keyword (without the trailing colon).
    pub fn descriptor(self) -> &'static str {
        match self {
            EventKind::Dialogue => "Dialogue",
            EventKind::Comment => "Comment",
            EventKind::Picture => "Picture",
            EventKind::Sound => "Sound",
            EventKind::Movie => "Movie",
            EventKind::Command => "Command",
        }
    }

    /// Parse a line descriptor (case-insensitive) into a kind.
    pub fn from_descriptor(s: &str) -> Option<EventKind> {
        let s = s.trim();
        [
            EventKind::Dialogue,
            EventKind::Comment,
            EventKind::Picture,
            EventKind::Sound,
            EventKind::Movie,
            EventKind::Command,
        ]
        .into_iter()
        .find(|&k| s.eq_ignore_ascii_case(k.descriptor()))
    }
}

/// A fully-typed event row.
///
/// All columns are captured as raw tokens so a serialise step re-emits
/// the author's exact spelling; typed accessors lift the common ones on
/// demand. The `text` column keeps the entire post-9th-comma remainder
/// verbatim (override blocks intact).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Event {
    pub kind: EventKind,
    /// `Layer` (ASS) or `Marked` (SSA) — raw token.
    pub layer: String,
    pub start: String,
    pub end: String,
    pub style: String,
    pub name: String,
    pub margin_l: String,
    pub margin_r: String,
    pub margin_v: String,
    pub effect: String,
    pub text: String,
}

impl Event {
    /// Typed [`LayerOverride`] for the `Layer` column.
    pub fn layer_typed(&self) -> LayerOverride {
        parse_layer_field(&self.layer)
    }

    /// Typed [`EventEffect`] for the `Effect` column.
    pub fn effect_typed(&self) -> EventEffect {
        parse_effect_field(&self.effect)
    }

    /// Typed left/right/vertical per-event margin overrides.
    pub fn margins_typed(&self) -> (MarginOverride, MarginOverride, MarginOverride) {
        (
            parse_margin_field(&self.margin_l),
            parse_margin_field(&self.margin_r),
            parse_margin_field(&self.margin_v),
        )
    }

    /// Extract every override tag from the event's `Text` column as a
    /// typed [`AnimatedTag`] stream, in document order.
    ///
    /// The `Text` field is scanned for `{...}` override blocks; the
    /// contents of each block run through the same override-tag reader
    /// the `animate` module uses, so the full documented tag surface is
    /// recognised (`\pos` / `\move` / `\fad` / `\fade` / `\t` / `\clip`
    /// / `\iclip` / the `\1c`–`\4c` colours / `\1a`–`\4a` + `\alpha`
    /// alphas / the `\fscx` / `\fscy` / `\frx` / `\fry` / `\frz`
    /// transforms / `\bord` / `\shad` / `\be` / `\blur` / `\fsp` /
    /// `\fn` / `\fe` / `\b` / `\i` / `\u` / `\s` / `\k` family / `\r`,
    /// …). Tags inside an animation wrapper are surfaced via the
    /// `\t(...)` token alongside their inner modifiers.
    ///
    /// The text outside the blocks (the actual subtitle glyphs) is not
    /// returned here — use [`Event::to_subtitle_cue`] for the styled
    /// segment stream, or read [`Event::text`] for the verbatim source.
    pub fn override_tags(&self) -> Vec<crate::AnimatedTag> {
        let mut out = Vec::new();
        let bytes = self.text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'{' {
                // Find the closing brace; an unterminated `{` is literal
                // text, so stop scanning blocks past it.
                let Some(rel) = self.text[i + 1..].find('}') else {
                    break;
                };
                let end = i + 1 + rel;
                crate::parse_overrides(&self.text[i + 1..end], &mut out);
                i = end + 1;
            } else {
                i += 1;
            }
        }
        out
    }
}

/// A verbatim section: header name + raw body lines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawSection {
    /// The section name without brackets (e.g. `Fonts`, `Aegisub
    /// Project Garbage`), original casing preserved.
    pub name: String,
    /// Every body line, verbatim, in source order.
    pub body: Vec<String>,
}

/// A complete parsed ASS/SSA script.
///
/// Sections are kept in source order; convenience accessors pull the
/// modelled sections out without the caller having to walk the list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssScript {
    /// All sections in source order.
    pub sections: Vec<Section>,
}

impl AssScript {
    /// First `[Script Info]` section, if any.
    pub fn script_info(&self) -> Option<&ScriptInfo> {
        self.sections.iter().find_map(|s| match s {
            Section::ScriptInfo(i) => Some(i),
            _ => None,
        })
    }

    /// First style table, if any.
    pub fn style_table(&self) -> Option<&StyleTable> {
        self.sections.iter().find_map(|s| match s {
            Section::Styles(t) => Some(t),
            _ => None,
        })
    }

    /// First event table, if any.
    pub fn event_table(&self) -> Option<&EventTable> {
        self.sections.iter().find_map(|s| match s {
            Section::Events(t) => Some(t),
            _ => None,
        })
    }
}

/// All style rows across every style table, in source order. The IR
/// convenience accessors below let a caller reach the rows without
/// matching on [`Section`].
impl AssScript {
    /// Flattened view of every `StyleDef` in the script.
    pub fn styles(&self) -> Vec<&StyleDef> {
        self.sections
            .iter()
            .filter_map(|s| match s {
                Section::Styles(t) => Some(&t.styles),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Flattened view of every [`Event`] in the script.
    pub fn events(&self) -> Vec<&Event> {
        self.sections
            .iter()
            .filter_map(|s| match s {
                Section::Events(t) => Some(&t.events),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Look up a style row by exact (case-sensitive) name.
    ///
    /// Per the SSA v4.x spec a style name is *"Case sensitive"*. Returns
    /// the first matching [`StyleDef`] across all style tables in source
    /// order, or `None` when no row carries that name.
    pub fn style_by_name(&self, name: &str) -> Option<&StyleDef> {
        self.styles().into_iter().find(|s| s.name == name)
    }

    /// Resolve the effective style for an event, applying the spec's
    /// name-fallback and per-event margin-override rules.
    ///
    /// Resolution follows the SSA v4.x spec:
    ///
    /// * The event's `Style` column names a style row. An empty column,
    ///   the literal `Default`, or a name that matches no row all fall
    ///   back to the script's own `Default` style (the spec's *"`*Default`
    ///   style will be substituted"* rule). If the script has no
    ///   `Default` row either, the resolver synthesises a default
    ///   [`StyleDef`] so the result is always defined.
    /// * Each per-event `MarginL` / `MarginR` / `MarginV` override
    ///   supersedes the matching style margin *unless* it is the
    ///   all-zeroes shorthand, in which case the style's margin is kept
    ///   (the spec: *"All zeroes means the default margins defined by the
    ///   style are used"*).
    ///
    /// The returned [`ResolvedStyle`] borrows the chosen base style and
    /// carries the three resolved margins as concrete pixel values
    /// (falling back to `0` when the style margin column is itself empty
    /// / non-numeric — the spec's neutral default).
    pub fn resolved_style_for<'a>(&'a self, event: &Event) -> ResolvedStyle<'a> {
        let name = event.style.trim();
        let base = if name.is_empty() || name.eq_ignore_ascii_case("Default") {
            self.style_by_name("Default")
        } else {
            self.style_by_name(name)
                .or_else(|| self.style_by_name("Default"))
        };
        let base = base.unwrap_or(&DEFAULT_STYLE);

        let style_margin = |raw: &str| raw.trim().parse::<u32>().unwrap_or(0);
        let resolve =
            |ov: MarginOverride, style_raw: &str| ov.resolve_with_style(style_margin(style_raw));
        let (ml, mr, mv) = event.margins_typed();
        ResolvedStyle {
            base,
            margin_l: resolve(ml, &base.margin_l),
            margin_r: resolve(mr, &base.margin_r),
            margin_v: resolve(mv, &base.margin_v),
        }
    }
}

/// The synthesised fallback style, used when an event names no style and
/// the script carries no `Default` row. All columns hold their neutral
/// defaults; the `name` is `Default`.
static DEFAULT_STYLE: std::sync::LazyLock<StyleDef> = std::sync::LazyLock::new(|| StyleDef {
    name: "Default".to_string(),
    ..StyleDef::default()
});

/// An event's effective style: the resolved base [`StyleDef`] plus the
/// three margins after the per-event override / style-fallback chain.
///
/// Produced by [`AssScript::resolved_style_for`]. The `base` reference
/// points at the chosen style row (or the synthesised default); the
/// `margin_*` fields are concrete pixel values with the spec's all-zeroes
/// fallback already applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedStyle<'a> {
    /// The chosen base style row.
    pub base: &'a StyleDef,
    /// Effective left margin in pixels.
    pub margin_l: u32,
    /// Effective right margin in pixels.
    pub margin_r: u32,
    /// Effective bottom (vertical) margin in pixels.
    pub margin_v: u32,
}

// ---------------------------------------------------------------------------
// Bridge to the shared subtitle IR

use oxideav_core::SubtitleStyle;
use oxideav_subtitle::ir::{SourceFormat, SubtitleTrack};

impl StyleDef {
    /// Project this fully-typed style row onto the lossy shared
    /// [`SubtitleStyle`] IR. Columns the IR cannot hold (`ScaleX` /
    /// `ScaleY` / `Spacing` / `Angle` / `BorderStyle` / `Encoding` /
    /// `SecondaryColour` / `AlphaLevel`) stay reachable on the
    /// `StyleDef` itself; the projection captures what the IR models.
    ///
    /// `ssa` selects the alignment numbering scheme (the legacy SSA bit
    /// layout vs the ASS numpad layout) — pass the owning
    /// [`StyleTable::ass`] negated.
    pub fn to_subtitle_style(&self, ssa: bool) -> SubtitleStyle {
        let align_n: i32 = self.alignment.trim().parse().unwrap_or(2);
        let align = if ssa {
            crate::ssa_alignment_to_textalign(align_n)
        } else {
            crate::ass_alignment_to_textalign(align_n)
        };
        SubtitleStyle {
            name: if self.name.is_empty() {
                "Default".to_string()
            } else {
                self.name.clone()
            },
            font_family: (!self.fontname.is_empty()).then(|| self.fontname.clone()),
            font_size: self.fontsize.parse().ok(),
            primary_color: crate::parse_ass_color(&self.primary_colour),
            outline_color: crate::parse_ass_color(&self.outline_colour),
            back_color: crate::parse_ass_color(&self.back_colour),
            bold: crate::parse_bool_flag(&self.bold),
            italic: crate::parse_bool_flag(&self.italic),
            underline: crate::parse_bool_flag(&self.underline),
            strike: crate::parse_bool_flag(&self.strikeout),
            align,
            margin_l: self.margin_l.trim().parse().ok(),
            margin_r: self.margin_r.trim().parse().ok(),
            margin_v: self.margin_v.trim().parse().ok(),
            outline: self.outline.trim().parse().ok(),
            shadow: self.shadow.trim().parse().ok(),
        }
    }
}

impl Event {
    /// Project a `Dialogue:` event onto a shared [`SubtitleCue`].
    ///
    /// Returns `None` for non-`Dialogue` event kinds (Comment events,
    /// Picture / Sound / Movie / Command lines) which the IR cue path
    /// does not represent. Timing parses through the same `H:MM:SS.cc`
    /// reader the base parser uses; the `Text` column runs through the
    /// override-tag segmenter so the cue carries styled segments +
    /// positioning.
    pub fn to_subtitle_cue(&self) -> Option<oxideav_core::SubtitleCue> {
        if self.kind != EventKind::Dialogue {
            return None;
        }
        let start_us = crate::parse_ass_timestamp(self.start.trim()).unwrap_or(0);
        let end_us = crate::parse_ass_timestamp(self.end.trim()).unwrap_or(0);
        let style_ref = if self.style.trim().is_empty() {
            None
        } else {
            Some(self.style.trim().to_string())
        };
        let (segments, positioning) = crate::parse_ass_text(&self.text);
        Some(oxideav_core::SubtitleCue {
            start_us,
            end_us,
            style_ref,
            positioning,
            segments,
        })
    }
}

impl AssScript {
    /// Project the whole script onto the shared [`SubtitleTrack`] IR.
    ///
    /// `[Script Info]` `Key: Value` pairs become track metadata (keys
    /// lower-cased with spaces folded to `_`, matching the base
    /// [`parse`](crate::parse) convention), every style row becomes a
    /// [`SubtitleStyle`], and every `Dialogue:` event becomes a
    /// [`SubtitleCue`]. Comment / Picture / Sound / Movie / Command
    /// events are skipped (the IR cue stream is dialogue-only), matching
    /// the base parser's behaviour.
    ///
    /// This is the lossy projection; the structured [`AssScript`] keeps
    /// the full field set, so a caller wanting field-level fidelity
    /// should serialise the [`AssScript`] directly rather than going
    /// through the IR.
    pub fn to_track(&self) -> SubtitleTrack {
        let mut track = SubtitleTrack {
            source: Some(SourceFormat::AssOrSsa),
            ..SubtitleTrack::default()
        };
        for section in &self.sections {
            match section {
                Section::ScriptInfo(info) => {
                    for line in &info.lines {
                        if let InfoLine::Pair { key, value } = line {
                            track.metadata.push((
                                key.trim().to_ascii_lowercase().replace(' ', "_"),
                                value.trim().to_string(),
                            ));
                        }
                    }
                }
                Section::Styles(t) => {
                    let ssa = !t.ass;
                    for s in &t.styles {
                        track.styles.push(s.to_subtitle_style(ssa));
                    }
                }
                Section::Events(t) => {
                    for ev in &t.events {
                        if let Some(cue) = ev.to_subtitle_cue() {
                            track.cues.push(cue);
                        }
                    }
                }
                Section::Raw(_) => {}
            }
        }
        track
    }

    /// Convert the whole script into the ASS `[V4+ Styles]` dialect.
    ///
    /// Returns a new [`AssScript`]; `self` is left unchanged. Equivalent
    /// to [`to_dialect(Dialect::Ass)`](Self::to_dialect).
    pub fn to_ass(&self) -> AssScript {
        self.to_dialect(Dialect::Ass)
    }

    /// Convert the whole script into the legacy SSA `[V4 Styles]` dialect.
    ///
    /// Returns a new [`AssScript`]; `self` is left unchanged. Equivalent
    /// to [`to_dialect(Dialect::Ssa)`](Self::to_dialect).
    pub fn to_ssa(&self) -> AssScript {
        self.to_dialect(Dialect::Ssa)
    }

    /// Convert the whole script into the requested [`Dialect`].
    ///
    /// The two dialects carry the *same* underlying style + event field
    /// data; they differ only in the `Format:` column set, the
    /// `[V4+ Styles]` vs `[V4 Styles]` section header, the `Alignment`
    /// numbering scheme (numpad for ASS, the `+4`/`+8` bit scheme for
    /// SSA), the event leading column (`Layer` integer for ASS,
    /// `Marked=N` for SSA), and the `[Script Info]` `ScriptType` header
    /// (`v4.00+` vs `v4.00`). The structured [`StyleDef`] already keeps
    /// every column of both dialects, so the conversion is field-
    /// preserving: re-emitting the result with [`serialise`](Self::serialise)
    /// produces a syntactically valid file in the target dialect.
    ///
    /// Columns absent from the target dialect (e.g. ASS-only `ScaleX` /
    /// `ScaleY` / `Spacing` / `Angle` / `Underline` / `StrikeOut` when
    /// converting to SSA, or SSA-only `AlphaLevel` when converting to
    /// ASS) are simply not emitted in the target `Format:` row; their
    /// data is retained on the [`StyleDef`] so a later round-trip back to
    /// the originating dialect restores them.
    ///
    /// Raw sections (`[Fonts]`, `[Graphics]`, editor-private blocks) and
    /// `[Script Info]` ordering are preserved verbatim apart from the
    /// `ScriptType` value.
    pub fn to_dialect(&self, target: Dialect) -> AssScript {
        let want_ass = target == Dialect::Ass;
        let mut out = self.clone();
        for section in &mut out.sections {
            match section {
                Section::ScriptInfo(info) => {
                    for line in &mut info.lines {
                        if let InfoLine::Pair { key, value } = line {
                            if key.eq_ignore_ascii_case("ScriptType") {
                                *value = if want_ass {
                                    "v4.00+".to_string()
                                } else {
                                    "v4.00".to_string()
                                };
                            }
                        }
                    }
                }
                Section::Styles(table) => {
                    let was_ssa = !table.ass;
                    for s in &mut table.styles {
                        // Re-encode the Alignment column into the target
                        // scheme. The typed StyleAlignment normalises both
                        // schemes to the same anchor, so we decode in the
                        // source scheme and re-encode in the target one.
                        let al =
                            crate::style_alignment::parse_alignment_field(&s.alignment, was_ssa);
                        s.alignment = if want_ass {
                            al.as_numpad().to_string()
                        } else {
                            al.as_ssa().to_string()
                        };
                    }
                    table.ass = want_ass;
                    table.format = if want_ass {
                        ass_style_format()
                    } else {
                        ssa_style_format()
                    };
                }
                Section::Events(table) => {
                    for ev in &mut table.events {
                        ev.layer = convert_event_lead(&ev.layer, want_ass);
                    }
                    table.format = if want_ass {
                        ass_event_format()
                    } else {
                        ssa_event_format()
                    };
                }
                Section::Raw(_) => {}
            }
        }
        out
    }
}

/// The two ASS/SSA style dialects.
///
/// Selects the `Format:` column set, section header, `Alignment`
/// numbering scheme, and event leading column. See
/// [`AssScript::to_dialect`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    /// ASS `[V4+ Styles]` — numpad alignment, `Layer` event column,
    /// `ScriptType: v4.00+`.
    Ass,
    /// Legacy SSA `[V4 Styles]` — bit-scheme alignment, `Marked` event
    /// column, `ScriptType: v4.00`.
    Ssa,
}

/// Canonical ASS `[V4+ Styles]` `Format:` column names.
fn ass_style_format() -> Vec<String> {
    [
        "Name",
        "Fontname",
        "Fontsize",
        "PrimaryColour",
        "SecondaryColour",
        "OutlineColour",
        "BackColour",
        "Bold",
        "Italic",
        "Underline",
        "StrikeOut",
        "ScaleX",
        "ScaleY",
        "Spacing",
        "Angle",
        "BorderStyle",
        "Outline",
        "Shadow",
        "Alignment",
        "MarginL",
        "MarginR",
        "MarginV",
        "Encoding",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Canonical legacy SSA `[V4 Styles]` `Format:` column names.
fn ssa_style_format() -> Vec<String> {
    [
        "Name",
        "Fontname",
        "Fontsize",
        "PrimaryColour",
        "SecondaryColour",
        "TertiaryColour",
        "BackColour",
        "Bold",
        "Italic",
        "BorderStyle",
        "Outline",
        "Shadow",
        "Alignment",
        "MarginL",
        "MarginR",
        "MarginV",
        "AlphaLevel",
        "Encoding",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Canonical ASS `[Events]` `Format:` column names.
fn ass_event_format() -> Vec<String> {
    [
        "Layer", "Start", "End", "Style", "Name", "MarginL", "MarginR", "MarginV", "Effect", "Text",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Canonical legacy SSA `[Events]` `Format:` column names.
fn ssa_event_format() -> Vec<String> {
    [
        "Marked", "Start", "End", "Style", "Name", "MarginL", "MarginR", "MarginV", "Effect",
        "Text",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Convert the event leading column between the ASS `Layer` integer and
/// the SSA `Marked=N` form.
///
/// ASS-target: an SSA `Marked=N` token becomes the bare integer `N`
/// (defaulting to `0` when the value is malformed); an already-bare
/// integer passes through. SSA-target: a bare integer `N` becomes
/// `Marked=N`; an already-`Marked=N` token passes through. The
/// conversion is total — anything unrecognised maps to the `0` layer.
fn convert_event_lead(raw: &str, want_ass: bool) -> String {
    let t = raw.trim();
    // Pull the numeric value out of either form.
    let value = if let Some(rest) = t.strip_prefix("Marked=").or_else(|| {
        // Tolerate `Marked =` / case variations.
        t.split_once('=')
            .filter(|(k, _)| k.trim().eq_ignore_ascii_case("Marked"))
            .map(|(_, v)| v)
    }) {
        rest.trim().parse::<i32>().unwrap_or(0)
    } else {
        t.parse::<i32>().unwrap_or(0)
    };
    if want_ass {
        value.to_string()
    } else {
        format!("Marked={value}")
    }
}

// ---------------------------------------------------------------------------
// Parsing

/// Parse raw bytes into a structured [`AssScript`].
///
/// The parser is total — it never returns an error. Lines that do not
/// fit a modelled grammar are preserved verbatim (inside the nearest
/// [`Section::Raw`]) so a serialise step replays them. A leading UTF-8
/// BOM is stripped; the rest is decoded with UTF-8 lossy replacement so
/// a stray non-UTF-8 byte cannot abort the parse.
pub fn parse_script(bytes: &[u8]) -> AssScript {
    let text = decode_lossy(bytes);
    let mut sections: Vec<Section> = Vec::new();

    // Section being accumulated. We buffer lines for the current
    // section and flush a typed `Section` when the header changes or at
    // EOF, so the source order is preserved exactly.
    enum Acc {
        None,
        Info(ScriptInfo),
        Styles(StyleTable),
        Events(EventTable),
        Raw(RawSection),
    }
    let mut acc = Acc::None;

    fn flush(acc: &mut Acc, out: &mut Vec<Section>) {
        match std::mem::replace(acc, Acc::None) {
            Acc::None => {}
            Acc::Info(i) => out.push(Section::ScriptInfo(i)),
            Acc::Styles(t) => out.push(Section::Styles(t)),
            Acc::Events(t) => out.push(Section::Events(t)),
            Acc::Raw(r) => out.push(Section::Raw(r)),
        }
    }

    // `split('\n')` yields a phantom final `""` segment for a
    // `\n`-terminated document; that segment used to land in the last
    // section's body as a real blank line, so each parse -> serialise
    // round appended one more trailing blank (unbounded growth,
    // breaking the documented re-parse fixpoint). Strip exactly one
    // trailing newline: `...\n` and `...` parse identically, and
    // `serialise` re-emits the canonical terminator. A document that
    // really ends in a blank line (`...\n\n`) keeps the blank -- only
    // the final terminator is consumed.
    let text = text.strip_suffix('\n').unwrap_or(&text);

    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim();

        // Section header: needs a non-empty name — a bare `[]` is not
        // a header (an empty-named `RawSection` would serialise to
        // nothing, so the header byte pair could not round-trip); it
        // falls through as an ordinary body line instead.
        if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() > 2 {
            flush(&mut acc, &mut sections);
            let name = &trimmed[1..trimmed.len() - 1];
            let lc = name.to_ascii_lowercase();
            acc = match lc.as_str() {
                "script info" => Acc::Info(ScriptInfo::default()),
                "v4+ styles" | "v4 styles" | "v4.00+ styles" | "v4.00 styles" => {
                    Acc::Styles(StyleTable {
                        ass: lc.contains('+'),
                        ..StyleTable::default()
                    })
                }
                "events" => Acc::Events(EventTable::default()),
                _ => Acc::Raw(RawSection {
                    name: name.to_string(),
                    body: Vec::new(),
                }),
            };
            continue;
        }

        match &mut acc {
            Acc::None => {
                // Content before any section header — keep it in a
                // nameless raw section so it round-trips.
                let r = RawSection {
                    name: String::new(),
                    body: vec![line.to_string()],
                };
                acc = Acc::Raw(r);
            }
            Acc::Info(info) => {
                if trimmed.is_empty() {
                    info.lines.push(InfoLine::Blank);
                } else if trimmed.starts_with(';') || trimmed.starts_with('!') {
                    info.lines.push(InfoLine::Comment(line.to_string()));
                } else if let Some((k, v)) = line.split_once(':') {
                    info.lines.push(InfoLine::Pair {
                        key: k.trim().to_string(),
                        value: v.trim().to_string(),
                    });
                } else {
                    // A non-conforming line; keep it as a comment so it
                    // survives without inventing a key.
                    info.lines.push(InfoLine::Comment(line.to_string()));
                }
            }
            Acc::Styles(table) => {
                if let Some(rest) = strip_descriptor(trimmed, "Format") {
                    // Only the first Format line counts (the spec puts
                    // it first in the section). Letting a later Format
                    // replace the column set would re-serialise rows
                    // already parsed under the first set with a
                    // different column count — unstable under
                    // re-parse.
                    if table.format.is_empty() {
                        table.format = dedupe_fields(split_fields(rest));
                    }
                } else if let Some(rest) = strip_descriptor(trimmed, "Style") {
                    if let Some(s) = parse_style_row(rest, &table.format) {
                        table.styles.push(s);
                    }
                }
                // Blank / unknown lines inside a style table are dropped
                // (the serialiser re-synthesises the canonical blank
                // separators); the spec discards unrecognised lines.
            }
            Acc::Events(table) => {
                if let Some(rest) = strip_descriptor(trimmed, "Format") {
                    // First Format wins — see the styles arm above.
                    if table.format.is_empty() {
                        table.format = dedupe_fields(split_fields(rest));
                    }
                } else if let Some((desc, rest)) = trimmed.split_once(':') {
                    if let Some(kind) = EventKind::from_descriptor(desc) {
                        if let Some(ev) = parse_event_row(kind, rest.trim_start(), &table.format) {
                            table.events.push(ev);
                        }
                    }
                }
            }
            Acc::Raw(r) => {
                r.body.push(line.to_string());
            }
        }
    }
    flush(&mut acc, &mut sections);
    AssScript { sections }
}

/// `true` if `out` ends with a blank line (a `\n` immediately preceded
/// by another `\n`, or the buffer is exactly one trailing `\n` after the
/// start). Used to avoid emitting a duplicate inter-section separator.
fn ends_with_blank(out: &str) -> bool {
    out.ends_with("\n\n")
}

fn decode_lossy(bytes: &[u8]) -> String {
    let stripped = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    };
    String::from_utf8_lossy(stripped).into_owned()
}

/// Strip a `Descriptor:` prefix (case-insensitive) and return the
/// trimmed remainder.
fn strip_descriptor<'a>(line: &'a str, desc: &str) -> Option<&'a str> {
    let (head, rest) = line.split_once(':')?;
    if head.trim().eq_ignore_ascii_case(desc) {
        Some(rest.trim_start())
    } else {
        None
    }
}

/// Split a `Format:` field list on commas, trimming each name.
fn split_fields(s: &str) -> Vec<String> {
    s.split(',').map(|f| f.trim().to_string()).collect()
}

/// Drop repeated column names from a `Format:` field list (first
/// occurrence wins; comparison matches the row mappers' key derivation
/// — ASCII-lowercased, spaces removed). A duplicated column is
/// malformed input, and the last-field-takes-the-rest CSV convention
/// makes a duplicated `Text` column unstable under re-serialisation:
/// the row emits the text once per column while the parser folds
/// everything from the first `Text` slot back into one value, so each
/// round-trip grew the line. Deduping restores the serialise fixpoint.
fn dedupe_fields(fields: Vec<String>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::with_capacity(fields.len());
    let mut out = Vec::with_capacity(fields.len());
    for f in fields {
        let key = f.to_ascii_lowercase().replace(' ', "");
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(f);
    }
    out
}

/// Split a body line into `n` comma-separated columns, leaving the
/// final column whole (so a `Text` field with commas stays intact).
fn split_columns(line: &str, n: usize) -> Vec<&str> {
    if n == 0 {
        return vec![line];
    }
    let mut out: Vec<&str> = Vec::with_capacity(n);
    let mut cursor = line;
    for _ in 0..n - 1 {
        if let Some(i) = cursor.find(',') {
            out.push(&cursor[..i]);
            cursor = &cursor[i + 1..];
        } else {
            out.push(cursor);
            cursor = "";
        }
    }
    out.push(cursor);
    out
}

fn parse_style_row(rest: &str, fmt: &[String]) -> Option<StyleDef> {
    if fmt.is_empty() {
        return None;
    }
    let cols = split_columns(rest, fmt.len());
    if cols.len() < fmt.len() {
        return None;
    }
    let mut s = StyleDef::default();
    for (k, v) in fmt.iter().zip(cols.iter()) {
        let key = k.to_ascii_lowercase().replace(' ', "");
        let val = v.trim().to_string();
        match key.as_str() {
            "name" => s.name = val,
            "fontname" => s.fontname = val,
            "fontsize" => s.fontsize = val,
            "primarycolour" | "primarycolor" => s.primary_colour = val,
            "secondarycolour" | "secondarycolor" => s.secondary_colour = val,
            "outlinecolour" | "outlinecolor" | "tertiarycolour" | "tertiarycolor" => {
                s.outline_colour = val
            }
            "backcolour" | "backcolor" => s.back_colour = val,
            "bold" => s.bold = val,
            "italic" => s.italic = val,
            "underline" => s.underline = val,
            "strikeout" | "strikethrough" => s.strikeout = val,
            "scalex" => s.scale_x = val,
            "scaley" => s.scale_y = val,
            "spacing" => s.spacing = val,
            "angle" => s.angle = val,
            "borderstyle" => s.border_style = val,
            "outline" => s.outline = val,
            "shadow" => s.shadow = val,
            "alignment" => s.alignment = val,
            "marginl" => s.margin_l = val,
            "marginr" => s.margin_r = val,
            "marginv" => s.margin_v = val,
            "alphalevel" => s.alpha_level = val,
            "encoding" => s.encoding = val,
            _ => {}
        }
    }
    Some(s)
}

fn parse_event_row(kind: EventKind, rest: &str, fmt: &[String]) -> Option<Event> {
    if fmt.is_empty() {
        return None;
    }
    let cols = split_columns(rest, fmt.len());
    if cols.len() < fmt.len() {
        return None;
    }
    let mut ev = Event {
        kind,
        ..Event::default()
    };
    for (k, v) in fmt.iter().zip(cols.iter()) {
        let key = k.to_ascii_lowercase().replace(' ', "");
        // `Text` keeps surrounding spaces; all other columns are
        // trimmed because the CSV separators carry no meaningful
        // whitespace.
        match key.as_str() {
            "layer" | "marked" => ev.layer = v.trim().to_string(),
            "start" => ev.start = v.trim().to_string(),
            "end" => ev.end = v.trim().to_string(),
            "style" => ev.style = v.trim().to_string(),
            "name" => ev.name = v.trim().to_string(),
            "marginl" => ev.margin_l = v.trim().to_string(),
            "marginr" => ev.margin_r = v.trim().to_string(),
            "marginv" => ev.margin_v = v.trim().to_string(),
            "effect" => ev.effect = v.trim().to_string(),
            "text" => ev.text = v.to_string(),
            _ => {}
        }
    }
    Some(ev)
}

// ---------------------------------------------------------------------------
// Serialisation

impl AssScript {
    /// Serialise the structured script back to ASS/SSA bytes.
    ///
    /// Sections are emitted in their stored order. A modelled section
    /// re-synthesises its `Format:` row from the stored field order and
    /// fills each column from the typed row; a [`Section::Raw`] replays
    /// its body verbatim. The output is `\n`-terminated and
    /// re-parseable into an equivalent [`AssScript`].
    pub fn serialise(&self) -> Vec<u8> {
        let mut out = String::new();
        for (idx, section) in self.sections.iter().enumerate() {
            // Emit a blank separator before each section after the
            // first, *unless* the output already ends on a blank line
            // (the `[Script Info]` line list captures its own trailing
            // blank, so an extra separator would accumulate a second
            // blank on each round-trip). This keeps `serialise` a
            // fixpoint under re-parse.
            if idx > 0 && !ends_with_blank(&out) {
                out.push('\n');
            }
            match section {
                Section::ScriptInfo(info) => {
                    out.push_str("[Script Info]\n");
                    for line in &info.lines {
                        match line {
                            InfoLine::Pair { key, value } => {
                                out.push_str(key);
                                out.push_str(": ");
                                out.push_str(value);
                                out.push('\n');
                            }
                            InfoLine::Comment(c) => {
                                out.push_str(c);
                                out.push('\n');
                            }
                            InfoLine::Blank => out.push('\n'),
                        }
                    }
                }
                Section::Styles(table) => {
                    out.push_str(if table.ass {
                        "[V4+ Styles]\n"
                    } else {
                        "[V4 Styles]\n"
                    });
                    if !table.format.is_empty() {
                        out.push_str("Format: ");
                        out.push_str(&table.format.join(", "));
                        out.push('\n');
                        for s in &table.styles {
                            out.push_str("Style: ");
                            out.push_str(&serialise_style_row(s, &table.format));
                            out.push('\n');
                        }
                    }
                }
                Section::Events(table) => {
                    out.push_str("[Events]\n");
                    if !table.format.is_empty() {
                        out.push_str("Format: ");
                        out.push_str(&table.format.join(", "));
                        out.push('\n');
                        for ev in &table.events {
                            out.push_str(ev.kind.descriptor());
                            out.push_str(": ");
                            out.push_str(&serialise_event_row(ev, &table.format));
                            out.push('\n');
                        }
                    }
                }
                Section::Raw(r) => {
                    if !r.name.is_empty() {
                        out.push('[');
                        out.push_str(&r.name);
                        out.push_str("]\n");
                    }
                    for b in &r.body {
                        out.push_str(b);
                        out.push('\n');
                    }
                }
            }
        }
        out.into_bytes()
    }
}

fn serialise_style_row(s: &StyleDef, fmt: &[String]) -> String {
    let cols: Vec<&str> = fmt
        .iter()
        .map(|f| {
            let key = f.to_ascii_lowercase().replace(' ', "");
            match key.as_str() {
                "name" => s.name.as_str(),
                "fontname" => s.fontname.as_str(),
                "fontsize" => s.fontsize.as_str(),
                "primarycolour" | "primarycolor" => s.primary_colour.as_str(),
                "secondarycolour" | "secondarycolor" => s.secondary_colour.as_str(),
                "outlinecolour" | "outlinecolor" | "tertiarycolour" | "tertiarycolor" => {
                    s.outline_colour.as_str()
                }
                "backcolour" | "backcolor" => s.back_colour.as_str(),
                "bold" => s.bold.as_str(),
                "italic" => s.italic.as_str(),
                "underline" => s.underline.as_str(),
                "strikeout" | "strikethrough" => s.strikeout.as_str(),
                "scalex" => s.scale_x.as_str(),
                "scaley" => s.scale_y.as_str(),
                "spacing" => s.spacing.as_str(),
                "angle" => s.angle.as_str(),
                "borderstyle" => s.border_style.as_str(),
                "outline" => s.outline.as_str(),
                "shadow" => s.shadow.as_str(),
                "alignment" => s.alignment.as_str(),
                "marginl" => s.margin_l.as_str(),
                "marginr" => s.margin_r.as_str(),
                "marginv" => s.margin_v.as_str(),
                "alphalevel" => s.alpha_level.as_str(),
                "encoding" => s.encoding.as_str(),
                _ => "",
            }
        })
        .collect();
    cols.join(",")
}

fn serialise_event_row(ev: &Event, fmt: &[String]) -> String {
    let cols: Vec<&str> = fmt
        .iter()
        .map(|f| {
            let key = f.to_ascii_lowercase().replace(' ', "");
            match key.as_str() {
                "layer" | "marked" => ev.layer.as_str(),
                "start" => ev.start.as_str(),
                "end" => ev.end.as_str(),
                "style" => ev.style.as_str(),
                "name" => ev.name.as_str(),
                "marginl" => ev.margin_l.as_str(),
                "marginr" => ev.margin_r.as_str(),
                "marginv" => ev.margin_v.as_str(),
                "effect" => ev.effect.as_str(),
                "text" => ev.text.as_str(),
                _ => "",
            }
        })
        .collect();
    cols.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASS: &str = "[Script Info]\n\
; A comment line\n\
Title: Demo\n\
ScriptType: v4.00+\n\
PlayResX: 1280\n\
PlayResY: 720\n\
\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,Arial,48,&H00FFFFFF,&H00000000,&H64000000,0,0,0,0,100,100,0,0,1,2,1,2,30,30,30,1\n\
Style: Title,Verdana,72,&H0000D7FF,&H00000000,&H00000000,-1,0,0,0,120,120,2,0,3,4,0,8,30,30,40,0\n\
\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:03.00,Default,Bob,0,0,0,,{\\b1}Hello{\\b0}, world\n\
Comment: 0,0:00:03.00,0:00:04.00,Default,,0,0,0,,a comment event\n\
Dialogue: 1,0:00:04.00,0:00:06.00,Title,,0,0,0,Banner;50,Scrolling, text here\n";

    #[test]
    fn parses_all_sections() {
        let s = parse_script(ASS.as_bytes());
        assert!(s.script_info().is_some());
        assert!(s.style_table().is_some());
        assert!(s.event_table().is_some());
        assert_eq!(s.styles().len(), 2);
        assert_eq!(s.events().len(), 3);
    }

    #[test]
    fn script_info_fields() {
        let s = parse_script(ASS.as_bytes());
        let info = s.script_info().unwrap();
        assert_eq!(info.get("Title"), Some("Demo"));
        assert_eq!(info.get("PlayResX"), Some("1280"));
        // case-insensitive lookup.
        assert_eq!(info.get("playresy"), Some("720"));
        assert!(info.is_ass());
        // The comment line is preserved.
        assert!(info
            .lines
            .iter()
            .any(|l| matches!(l, InfoLine::Comment(c) if c.contains("A comment line"))));
    }

    #[test]
    fn script_info_typed_document_fields() {
        use crate::script_info::{Collisions, WrapStyle};
        let src = "[Script Info]\n\
ScriptType: v4.00+\n\
PlayResX: 1920\n\
PlayResY: 1080\n\
PlayDepth: 32\n\
WrapStyle: 2\n\
Collisions: Reverse\n\
Timer: 100.0000\n";
        let s = parse_script(src.as_bytes());
        let info = s.script_info().unwrap();
        assert_eq!(info.play_res_x(), Some(1920));
        assert_eq!(info.play_res_y(), Some(1080));
        assert_eq!(info.play_depth(), Some(32));
        assert_eq!(info.wrap_style(), WrapStyle::NoWrap);
        assert!(!info.wrap_style().wraps_automatically());
        assert_eq!(info.collisions(), Collisions::Reverse);
        assert!(info.collisions().is_reverse());
        assert!((info.timer() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn wrap_style_document_default_resolved_against_per_line_q() {
        use crate::script_info::WrapStyle;
        // Document default is \q2 (no-wrap); one event carries a per-line
        // \q1 override and another carries none. The effective wrap style
        // is the per-line override when present, else the document
        // default.
        let src = "[Script Info]\n\
ScriptType: v4.00+\n\
WrapStyle: 2\n\
\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\q1}end of line wrap\n\
Dialogue: 0,0:00:02.00,0:00:03.00,Default,,0,0,0,,no override here\n";
        let s = parse_script(src.as_bytes());
        let doc_default = s.script_info().unwrap().wrap_style();
        assert_eq!(doc_default, WrapStyle::NoWrap);

        let events = s.events();
        // Event 0: \q1 override → EndOfLine, superseding the doc default.
        let q0 = events[0].override_tags().iter().find_map(|t| match t {
            crate::AnimatedTag::Q(n) => Some(*n),
            _ => None,
        });
        assert_eq!(doc_default.resolve_override(q0), WrapStyle::EndOfLine);

        // Event 1: no override → keeps the doc default (NoWrap).
        let q1 = events[1].override_tags().iter().find_map(|t| match t {
            crate::AnimatedTag::Q(n) => Some(*n),
            _ => None,
        });
        assert_eq!(q1, None);
        assert_eq!(doc_default.resolve_override(q1), WrapStyle::NoWrap);
    }

    #[test]
    fn script_info_typed_defaults_when_headers_absent() {
        use crate::script_info::{Collisions, WrapStyle};
        // A header-light script: only ScriptType present. Every typed
        // document accessor resolves to the spec default.
        let src = "[Script Info]\nScriptType: v4.00+\n";
        let s = parse_script(src.as_bytes());
        let info = s.script_info().unwrap();
        assert_eq!(info.wrap_style(), WrapStyle::SmartEven);
        assert_eq!(info.collisions(), Collisions::Normal);
        assert_eq!(info.play_res_x(), None);
        assert_eq!(info.play_res_y(), None);
        assert_eq!(info.play_depth(), None);
        assert!((info.timer() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn style_rows_fully_decoded() {
        let s = parse_script(ASS.as_bytes());
        let styles = s.styles();
        let title = styles.iter().find(|s| s.name == "Title").unwrap();
        assert_eq!(title.fontname, "Verdana");
        assert_eq!(title.fontsize, "72");
        assert_eq!(title.primary_colour, "&H0000D7FF");
        assert_eq!(title.scale_x, "120");
        assert_eq!(title.spacing, "2");
        assert_eq!(title.border_style, "3");
        assert_eq!(title.alignment, "8");
        assert_eq!(title.bold, "-1");
        // Typed accessor surfaces the BorderStyle.
        assert!(title.border_style_typed().is_opaque_box());
    }

    #[test]
    fn style_def_typed_accessors() {
        use crate::dialogue_margin::MarginOverride;
        let s = parse_script(ASS.as_bytes());
        let table = s.style_table().unwrap();
        let is_ssa = !table.ass;
        let title = table.styles.iter().find(|s| s.name == "Title").unwrap();

        // Alignment column `8` is top-centre on the ASS numpad scheme.
        let al = title.alignment_typed(is_ssa);
        assert_eq!(al.as_numpad(), 8);
        assert!(!al.is_bottom());

        // ScaleX/Y/Spacing/Angle → 120 / 120 / 2 / 0.
        let xf = title.transform_typed();
        assert!((xf.scale_x - 120.0).abs() < 1e-9);
        assert!((xf.scale_y - 120.0).abs() < 1e-9);
        assert!((xf.spacing - 2.0).abs() < 1e-9);
        assert!((xf.angle - 0.0).abs() < 1e-9);
        assert!(!xf.is_identity());

        // MarginL/R/V → 30 / 30 / 40 pixels.
        let (ml, mr, mv) = title.margins_typed();
        assert_eq!(ml, MarginOverride::Pixels(30));
        assert_eq!(mr, MarginOverride::Pixels(30));
        assert_eq!(mv, MarginOverride::Pixels(40));

        // PrimaryColour `&H0000D7FF` is opaque (wire alpha `00` → 255)
        // with BGR bytes `D7 FF 00` → RGB `(255, 215, 0)` (gold).
        let (r, g, b, a) = title.primary_colour_typed().unwrap();
        assert_eq!((r, g, b, a), (255, 215, 0, 255));

        // Bold column `-1` is the SSA "true" sentinel; the rest are off.
        assert!(title.bold_typed());
        assert!(!title.italic_typed());
        assert!(!title.underline_typed());
        assert!(!title.strikeout_typed());

        // Numeric columns.
        assert_eq!(title.fontsize_typed(), Some(72.0));
        assert_eq!(title.outline_typed(), Some(4.0));
        assert_eq!(title.shadow_typed(), Some(0.0));
        assert_eq!(title.encoding_typed().as_code(), 0);
    }

    #[test]
    fn style_def_typed_accessors_total_on_garbage() {
        // Every typed accessor stays total: an all-garbage row collapses
        // each column to its documented default rather than panicking.
        let sd = StyleDef {
            fontname: "X".into(),
            fontsize: "junk".into(),
            primary_colour: "not-a-colour".into(),
            secondary_colour: String::new(),
            outline_colour: "&H00".into(),
            back_colour: "zzz".into(),
            bold: "junk".into(),
            italic: "junk".into(),
            underline: String::new(),
            strikeout: "  ".into(),
            scale_x: "junk".into(),
            scale_y: String::new(),
            spacing: "NaN".into(),
            angle: "inf".into(),
            outline: "junk".into(),
            shadow: String::new(),
            alignment: "junk".into(),
            margin_l: "-5".into(),
            margin_r: String::new(),
            margin_v: "junk".into(),
            encoding: "999".into(),
            ..StyleDef::default()
        };
        // Alignment collapses to bottom-centre (numpad 2).
        assert_eq!(sd.alignment_typed(false).as_numpad(), 2);
        // Transform collapses to identity (100/100/0/0).
        assert!(sd.transform_typed().is_identity());
        // Colours collapse to None.
        assert_eq!(sd.primary_colour_typed(), None);
        assert_eq!(sd.back_colour_typed(), None);
        // Bool flags off; numerics None.
        assert!(!sd.bold_typed());
        assert!(!sd.italic_typed());
        assert_eq!(sd.fontsize_typed(), None);
        assert_eq!(sd.outline_typed(), None);
        assert_eq!(sd.shadow_typed(), None);
        // Encoding `999` is out of `0..=255` so collapses to ANSI 0.
        assert_eq!(sd.encoding_typed().as_code(), 0);
    }

    #[test]
    fn event_rows_fully_decoded() {
        let s = parse_script(ASS.as_bytes());
        let events = s.events();
        assert_eq!(events[0].kind, EventKind::Dialogue);
        assert_eq!(events[0].name, "Bob");
        assert_eq!(events[0].style, "Default");
        // Override block and trailing comma in text survive whole.
        assert_eq!(events[0].text, "{\\b1}Hello{\\b0}, world");
        // Comment event recognised by kind.
        assert_eq!(events[1].kind, EventKind::Comment);
        // Effect column + comma-bearing text on the third event.
        assert_eq!(events[2].layer, "1");
        assert_eq!(events[2].effect, "Banner;50");
        assert_eq!(events[2].text, "Scrolling, text here");
    }

    #[test]
    fn round_trip_is_reparseable_and_equal() {
        let s1 = parse_script(ASS.as_bytes());
        let bytes = s1.serialise();
        let s2 = parse_script(&bytes);
        // The structured model is identical after a serialise + reparse.
        assert_eq!(
            s1,
            s2,
            "round-trip changed the model:\n{}",
            String::from_utf8_lossy(&bytes)
        );
    }

    #[test]
    fn raw_section_round_trips() {
        let src = "[Script Info]\n\
ScriptType: v4.00+\n\
\n\
[Aegisub Project Garbage]\n\
Last Style Storage: Default\n\
Scroll Position: 12\n\
\n\
[Fonts]\n\
fontname: Demo_B.ttf\n\
M0123456789ABCDEF=\n\
\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,hi\n";
        let s = parse_script(src.as_bytes());
        // Two raw sections preserved by name.
        let raw_names: Vec<&str> = s
            .sections
            .iter()
            .filter_map(|sec| match sec {
                Section::Raw(r) => Some(r.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(raw_names, vec!["Aegisub Project Garbage", "Fonts"]);
        let out = String::from_utf8(s.serialise()).unwrap();
        assert!(out.contains("[Aegisub Project Garbage]"));
        assert!(out.contains("Last Style Storage: Default"));
        assert!(out.contains("Scroll Position: 12"));
        assert!(out.contains("[Fonts]"));
        assert!(out.contains("fontname: Demo_B.ttf"));
        assert!(out.contains("M0123456789ABCDEF="));
    }

    #[test]
    fn ssa_v4_styles_dialect() {
        let src = "[Script Info]\n\
ScriptType: v4.00\n\
\n\
[V4 Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, AlphaLevel, Encoding\n\
Style: Def,Arial,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,-1,0,1,2,1,2,20,20,20,0,0\n\
\n\
[Events]\n\
Format: Marked, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: Marked=0,0:00:01.00,0:00:02.00,Def,,0,0,0,,hi\n";
        let s = parse_script(src.as_bytes());
        let table = s.style_table().unwrap();
        assert!(!table.ass, "SSA [V4 Styles] must not be flagged as ASS");
        let style = &table.styles[0];
        assert_eq!(style.secondary_colour, "&H0000FFFF");
        assert_eq!(style.outline_colour, "&H00000000"); // TertiaryColour → outline.
        assert_eq!(style.alpha_level, "0");
        // The SSA `Marked=0` value rides through the layer column raw.
        assert_eq!(s.events()[0].layer, "Marked=0");
        // Round-trips structurally.
        let s2 = parse_script(&s.serialise());
        assert_eq!(s, s2);
    }

    #[test]
    fn dialect_ass_to_ssa_rewrites_format_and_header() {
        let s = parse_script(ASS.as_bytes());
        let ssa = s.to_ssa();

        // Style table flips to the SSA dialect + format.
        let st = ssa.style_table().unwrap();
        assert!(!st.ass);
        assert_eq!(st.format, super::ssa_style_format());
        // The Title row's ASS numpad alignment `8` (top-centre) becomes
        // the SSA bit form `2 + 4 = 6` (centre + toptitle).
        let title = st.styles.iter().find(|s| s.name == "Title").unwrap();
        assert_eq!(title.alignment, "6");

        // Events flip to the SSA `Marked` lead.
        let et = ssa.event_table().unwrap();
        assert_eq!(et.format, super::ssa_event_format());
        assert_eq!(et.events[0].layer, "Marked=0");
        // Layer `1` on the third event carries through as `Marked=1`.
        assert_eq!(et.events[2].layer, "Marked=1");

        // ScriptType header rewritten.
        assert_eq!(ssa.script_info().unwrap().get("ScriptType"), Some("v4.00"));

        // Serialises to a valid SSA file that re-parses as SSA.
        let bytes = ssa.serialise();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("[V4 Styles]"));
        assert!(!text.contains("[V4+ Styles]"));
        let re = parse_script(&bytes);
        assert!(!re.style_table().unwrap().ass);
    }

    #[test]
    fn dialect_ssa_to_ass_rewrites_format_and_header() {
        let src = "[Script Info]\n\
ScriptType: v4.00\n\
\n\
[V4 Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, AlphaLevel, Encoding\n\
Style: Def,Arial,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,-1,0,1,2,1,6,20,20,20,0,0\n\
\n\
[Events]\n\
Format: Marked, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: Marked=0,0:00:01.00,0:00:02.00,Def,,0,0,0,,hi\n";
        let ssa = parse_script(src.as_bytes());
        let ass = ssa.to_ass();

        let st = ass.style_table().unwrap();
        assert!(st.ass);
        assert_eq!(st.format, super::ass_style_format());
        // SSA bit alignment `6` (centre + toptitle) becomes ASS numpad
        // `8` (top-centre).
        assert_eq!(st.styles[0].alignment, "8");
        // SSA-only AlphaLevel data is retained on the StyleDef even
        // though the ASS Format: row no longer lists it.
        assert_eq!(st.styles[0].alpha_level, "0");

        let et = ass.event_table().unwrap();
        assert_eq!(et.format, super::ass_event_format());
        // `Marked=0` lead becomes the bare ASS `Layer` integer `0`.
        assert_eq!(et.events[0].layer, "0");

        assert_eq!(ass.script_info().unwrap().get("ScriptType"), Some("v4.00+"));

        let text = String::from_utf8(ass.serialise()).unwrap();
        assert!(text.contains("[V4+ Styles]"));
    }

    #[test]
    fn dialect_round_trip_preserves_alignment_anchor() {
        // ASS → SSA → ASS keeps the alignment anchor (the bit/numpad
        // schemes are lossless under StyleAlignment normalisation).
        let s = parse_script(ASS.as_bytes());
        let back = s.to_ssa().to_ass();
        let orig_title = s
            .style_table()
            .unwrap()
            .styles
            .iter()
            .find(|s| s.name == "Title")
            .unwrap();
        let back_title = back
            .style_table()
            .unwrap()
            .styles
            .iter()
            .find(|s| s.name == "Title")
            .unwrap();
        assert_eq!(orig_title.alignment, back_title.alignment);
        // Event lead survives the ASS → SSA → ASS trip.
        assert_eq!(
            s.event_table().unwrap().events[2].layer,
            back.event_table().unwrap().events[2].layer
        );
    }

    #[test]
    fn dialect_to_ass_is_idempotent_on_ass_input() {
        let s = parse_script(ASS.as_bytes());
        let once = s.to_ass();
        let twice = once.to_ass();
        assert_eq!(once, twice);
    }

    #[test]
    fn convert_event_lead_is_total() {
        // Malformed leads collapse to the 0 layer in either direction.
        assert_eq!(super::convert_event_lead("junk", true), "0");
        assert_eq!(super::convert_event_lead("junk", false), "Marked=0");
        assert_eq!(super::convert_event_lead("Marked=7", true), "7");
        assert_eq!(super::convert_event_lead("3", false), "Marked=3");
        // Already in target form passes through.
        assert_eq!(super::convert_event_lead("5", true), "5");
        assert_eq!(super::convert_event_lead("Marked=2", false), "Marked=2");
    }

    #[test]
    fn style_by_name_is_case_sensitive() {
        let s = parse_script(ASS.as_bytes());
        assert!(s.style_by_name("Title").is_some());
        assert!(s.style_by_name("Default").is_some());
        // Case-sensitive per the spec.
        assert!(s.style_by_name("title").is_none());
        assert!(s.style_by_name("Missing").is_none());
    }

    #[test]
    fn resolved_style_applies_margin_override_chain() {
        // Build a script with a style carrying margins 30/30/40 and two
        // events: one with no margin override, one overriding MarginL.
        let src = "[Script Info]\n\
ScriptType: v4.00+\n\
\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Box,Arial,48,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,1,2,30,35,40,1\n\
\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:02.00,Box,,0,0,0,,uses style margins\n\
Dialogue: 0,0:00:02.00,0:00:03.00,Box,,99,0,0,,overrides MarginL\n";
        let s = parse_script(src.as_bytes());
        let events = s.events();

        // Event 0: all-zero overrides → keep the style margins.
        let r0 = s.resolved_style_for(events[0]);
        assert_eq!(r0.base.name, "Box");
        assert_eq!((r0.margin_l, r0.margin_r, r0.margin_v), (30, 35, 40));

        // Event 1: explicit MarginL=99 supersedes; the rest stay.
        let r1 = s.resolved_style_for(events[1]);
        assert_eq!((r1.margin_l, r1.margin_r, r1.margin_v), (99, 35, 40));
    }

    #[test]
    fn resolved_style_falls_back_to_default_then_synthetic() {
        // A script with a Default style; an event naming a missing style
        // falls back to Default. An empty style column also uses Default.
        let src = "[Script Info]\n\
ScriptType: v4.00+\n\
\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,Arial,48,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,1,2,10,10,10,1\n\
\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:02.00,NoSuchStyle,,0,0,0,,missing style\n\
Dialogue: 0,0:00:02.00,0:00:03.00,,,0,0,0,,empty style column\n";
        let s = parse_script(src.as_bytes());
        let events = s.events();
        // Missing style name → Default row.
        assert_eq!(s.resolved_style_for(events[0]).base.name, "Default");
        assert_eq!(s.resolved_style_for(events[0]).margin_v, 10);
        // Empty column → Default row.
        assert_eq!(s.resolved_style_for(events[1]).base.name, "Default");

        // With no Default row at all, the resolver synthesises one.
        let src2 = "[Script Info]\nScriptType: v4.00+\n\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:02.00,Ghost,,0,0,0,,no styles section\n";
        let s2 = parse_script(src2.as_bytes());
        let ev = s2.events();
        let r = s2.resolved_style_for(ev[0]);
        assert_eq!(r.base.name, "Default");
        assert_eq!((r.margin_l, r.margin_r, r.margin_v), (0, 0, 0));
    }

    #[test]
    fn to_track_projects_metadata_styles_and_cues() {
        let s = parse_script(ASS.as_bytes());
        let track = s.to_track();
        // Script Info pairs → metadata with lower_snake keys.
        assert!(track
            .metadata
            .iter()
            .any(|(k, v)| k == "title" && v == "Demo"));
        assert!(track
            .metadata
            .iter()
            .any(|(k, v)| k == "playresx" && v == "1280"));
        // Both style rows projected.
        assert_eq!(track.styles.len(), 2);
        let title = track.styles.iter().find(|s| s.name == "Title").unwrap();
        assert_eq!(title.font_family.as_deref(), Some("Verdana"));
        assert_eq!(title.font_size, Some(72.0));
        assert!(title.bold, "SSA -1 must read as bold true");
        // Only the two Dialogue events become cues (the Comment is
        // skipped, matching the IR dialogue-only convention).
        assert_eq!(track.cues.len(), 2);
        assert_eq!(track.cues[0].start_us, 1_000_000);
        assert_eq!(track.cues[0].end_us, 3_000_000);
        assert_eq!(track.cues[0].style_ref.as_deref(), Some("Default"));
        assert_eq!(track.cues[1].style_ref.as_deref(), Some("Title"));
    }

    #[test]
    fn to_subtitle_cue_skips_non_dialogue() {
        let ev = Event {
            kind: EventKind::Comment,
            start: "0:00:01.00".into(),
            end: "0:00:02.00".into(),
            ..Event::default()
        };
        assert!(ev.to_subtitle_cue().is_none());
        let dlg = Event {
            kind: EventKind::Dialogue,
            ..ev
        };
        assert!(dlg.to_subtitle_cue().is_some());
    }

    #[test]
    fn override_tags_extracts_full_tag_stream() {
        use crate::AnimatedTag;
        let ev = Event {
            kind: EventKind::Dialogue,
            text: "{\\pos(100,200)\\3c&H0000FF&}border {\\fad(150,150)}red".into(),
            ..Event::default()
        };
        let tags = ev.override_tags();
        // Spans both override blocks in document order.
        assert!(tags
            .iter()
            .any(|t| matches!(t, AnimatedTag::Pos { x, y } if *x == 100.0 && *y == 200.0)));
        // `&H0000FF&` is `&Hbbggrr&` → rr=FF → rgb (255, 0, 0).
        assert!(tags
            .iter()
            .any(|t| matches!(t, AnimatedTag::Color3((255, 0, 0)))));
        assert!(tags.iter().any(|t| matches!(
            t,
            AnimatedTag::Fad {
                t1_ms: 150,
                t2_ms: 150
            }
        )));
    }

    #[test]
    fn override_tags_handles_unterminated_brace() {
        let ev = Event {
            kind: EventKind::Dialogue,
            text: "{\\b1}ok then {unterminated".into(),
            ..Event::default()
        };
        // The first block parses; the stray `{` does not panic and is
        // treated as literal text.
        let tags = ev.override_tags();
        assert!(!tags.is_empty());
    }

    #[test]
    fn override_tags_empty_when_no_blocks() {
        let ev = Event {
            kind: EventKind::Dialogue,
            text: "plain text, no overrides".into(),
            ..Event::default()
        };
        assert!(ev.override_tags().is_empty());
    }

    #[test]
    fn to_subtitle_style_colour_and_alignment() {
        // &H00FF0000 → opaque blue; ASS numpad 8 → top-centre, which the
        // IR's horizontal-only TextAlign captures as Center.
        let sd = StyleDef {
            name: "X".into(),
            fontname: "Arial".into(),
            fontsize: "20".into(),
            primary_colour: "&H00FF0000".into(),
            alignment: "8".into(),
            ..StyleDef::default()
        };
        let style = sd.to_subtitle_style(false);
        assert_eq!(style.primary_color, Some((0, 0, 255, 255)));
        assert_eq!(style.align, oxideav_core::TextAlign::Center);
        // Empty name falls back to Default.
        let empty = StyleDef::default().to_subtitle_style(false);
        assert_eq!(empty.name, "Default");
    }
}
