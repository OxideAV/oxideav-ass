//! Typed extraction + time-evaluation of ASS *animated* override tags.
//!
//! The base parser in [`crate`] preserves animated tags as opaque
//! [`Segment::Raw`] blocks so the round-trip back to text stays
//! bit-faithful. This module adds a *renderer-facing* view: it walks
//! those `Raw` blocks (and any inline `\frz` / `\blur` / `\fscx` /
//! `\fscy` / `\clip` / `\fad` / `\move` / `\t` tags found in the
//! original dialogue text) and produces a typed [`CueAnimation`]
//! struct that downstream rasterizers can sample at any timestamp.
//!
//! The set of tags supported in this round:
//!
//! * `\fad(t1, t2)` — fade in over `t1` ms, fade out over `t2` ms,
//!   modulating the cue alpha multiplier.
//! * `\fade(a1, a2, a3, t1, t2, t3, t4)` — full 7-arg variant; alpha
//!   `a1` until `t1`, ramps to `a2` by `t2`, holds `a2` until `t3`,
//!   ramps to `a3` by `t4`. Alpha values use the ASS convention
//!   (`0` = opaque, `255` = transparent).
//! * `\pos(x, y)` — set the static line position (script-resolution
//!   coordinates). The non-moving counterpart of `\move`; both write
//!   [`RenderState::translate`]. Static, not animatable.
//! * `\move(x1, y1, x2, y2[, t1, t2])` — translate the rendered text
//!   from `(x1, y1)` at `t1` to `(x2, y2)` at `t2` (defaults: t1 = 0,
//!   t2 = cue duration).
//! * `\frz(angle)` — rotate around the Z axis by `angle` degrees.
//! * `\blur(strength)` — Gaussian blur sigma in pixels (`0` = no
//!   blur).
//! * `\be(strength)` — iterative box-blur strength (integer, edge-only
//!   softening). Distinct from `\blur` per Aegisub spec — exposed in
//!   [`RenderState::be_strength`] without merging into `blur_sigma`.
//! * `\bord(w)` / `\xbord(w)` / `\ybord(w)` — text border width (px).
//!   `\bord` sets both axes, `\xbord` and `\ybord` set X or Y only.
//!   Per Aegisub spec, a `\bord` after `\xbord`/`\ybord` overrides
//!   both axes again.
//! * `\shad(d)` / `\xshad(d)` / `\yshad(d)` — text shadow distance
//!   (px). `\shad` sets both axes uniformly (non-negative); the
//!   `\xshad`/`\yshad` per-axis tags permit negative values, which
//!   place the shadow to the top or left of the text.
//! * `\fax(f)` / `\fay(f)` — shear (perspective-distortion) factor on
//!   the X / Y axis. Applied after rotation, on rotated coordinates.
//! * `\fsp(spacing)` — letter-spacing in script-resolution pixels.
//!   Spacing may be negative or decimal; default `0` (no additional
//!   advance between letters). Animatable per the Aegisub / TCAX
//!   spec.
//! * `\q(style)` — wrap style override for the line. `0`/`1`/`2`/`3`
//!   map to the SSA spec wrap modes (smart-top / EOL / no-wrap /
//!   smart-bottom). Static, not animatable.
//! * `\an(pos)` — line alignment, numpad layout per the Aegisub spec:
//!   `1`/`2`/`3` = bottom-left/center/right, `4`/`5`/`6` = middle-
//!   left/center/right, `7`/`8`/`9` = top-left/center/right. Surfaces
//!   on [`RenderState::alignment`] as the same numpad value 1..=9 so
//!   the renderer can anchor the cue's `\pos`/`\move` translate at the
//!   correct corner. Static, not animatable per spec.
//! * `\a(pos)` — legacy SubStation-Alpha alignment code (still
//!   recognised by Aegisub). Calculation per spec: low nibble `1`/`2`/
//!   `3` for left/center/right; add `4` for top, add `8` for mid. The
//!   parser converts to the equivalent numpad value and writes the
//!   same [`RenderState::alignment`] field — so a cue with `\a6`
//!   surfaces as `alignment = Some(8)` (top-center), matching `\an8`.
//! * `\2c(&Hbbggrr&)` / `\3c(&Hbbggrr&)` / `\4c(&Hbbggrr&)` —
//!   secondary fill, border, and shadow colours. `\1c` (alias `\c`) is
//!   already in this set; the four together cover the four colour
//!   components an ASS glyph carries.
//! * `\alpha(&Haa&)` / `\1a(&Haa&)` / `\2a(&Haa&)` / `\3a(&Haa&)` /
//!   `\4a(&Haa&)` — per-component alpha overrides. ASS uses 0 = opaque,
//!   255 = transparent; renderers translate to their own opacity
//!   convention. `\alpha` sets all four channels at once; `\1a` /
//!   `\2a` / `\3a` / `\4a` set the primary / secondary / border /
//!   shadow alpha individually. These per-component alphas are
//!   independent of the cue-level `\fad` / `\fade` envelope (which
//!   keeps multiplying [`RenderState::alpha_mul`]).
//! * `\clip(x1, y1, x2, y2)` — restrict rendering to the rectangle
//!   `[x1..x2] x [y1..y2]`. The drawing-path form is recognised but
//!   stored verbatim (round 2).
//! * `\iclip(x1, y1, x2, y2)` — *inverse* rectangular clip: the cue
//!   is hidden inside the rectangle. Vector-drawing form is also
//!   accepted and stored verbatim in [`RenderState::iclip_drawing`].
//! * `\fscx(percent)` / `\fscy(percent)` — non-uniform scale.
//! * `\fn<name>` — font family override for the following text. The
//!   parameter is read literally up to the next `\` or end of block
//!   (per the Aegisub spec "no space between `\fn` and the font
//!   name"). Surfaces on [`RenderState::font_name`]. Not animatable;
//!   inside `\t(...)` the new face snaps in at `t > t1`.
//! * `\fe<id>` — Windows font-encoding (charset) ID for the glyph-
//!   mapping table. Common slots: `0` ANSI, `128` Shift-JIS, `134`
//!   GB2312, `136` BIG5. Surfaces on [`RenderState::font_encoding`].
//!   Not animatable.
//! * `\b<weight>` — bold weight override, integer per the Aegisub
//!   spec (`100..900`, `400` = normal, `700` = bold). The legacy
//!   shortcut `\b1` surfaces as `Some(700)`, `\b0` as `Some(0)`.
//!   Surfaces on [`RenderState::bold_weight`]. Not animatable.
//! * `\r[<style>]` — reset all override state for the following text;
//!   the optional name argument switches the base style to a named
//!   definition from `[V4+ Styles]`. Surfaces on
//!   [`RenderState::reset_to_style`] (`Some(None)` for bare `\r`,
//!   `Some(Some(name))` for `\r<name>`); applying a `\r` also clears
//!   every other override field per the spec's "cancels all style
//!   overrides in effect" rule.
//! * `\t(t1, t2, [accel,] tags)` — interpolate the inner tags over
//!   `[t1, t2]` within the cue. Inner tags supported in this round:
//!   `\fscx`, `\fscy`, `\frz`, `\c` / `\1c` / `\2c` / `\3c` / `\4c`,
//!   `\alpha` / `\1a` / `\2a` / `\3a` / `\4a`, `\fs`, `\blur`,
//!   `\bord`, `\xbord`, `\ybord`, `\shad`, `\xshad`, `\yshad`, `\fax`,
//!   `\fay`, `\fsp`. Other inner tags are stored verbatim and applied
//!   as a static override for `t >= t1`. `\q`, `\an` / `\a`, `\fn`,
//!   `\fe`, `\b`, and `\r` are static (non-animated) settings per
//!   spec; they snap to the post-state at `t > t1` rather than
//!   interpolating.
//!
//! Times in `\fad`, `\move`, `\t` are milliseconds *from the cue
//! start*. The ASS spec uses "ms from cue start" as the canonical
//! reference for every animation tag.

use oxideav_core::{Segment, SubtitleCue, Transform2D};

/// Which member of the `\k` karaoke-timing family produced a syllable
/// marker.
///
/// Per the Aegisub override-tag reference, the `\k` family marks up a
/// dialogue line for karaoke by giving the duration of each syllable;
/// the four members differ only in the *visual* transition they ask the
/// renderer for, not in the timing they encode:
///
/// * [`Fill`](KaraokeKind::Fill) (`\k`) — before the syllable's
///   highlight the glyphs use the secondary colour + alpha; when the
///   syllable starts, the fill switches *instantly* to the primary
///   colour + alpha.
/// * [`Sweep`](KaraokeKind::Sweep) (`\kf`, and the identical `\K`) — the
///   fill starts secondary and sweeps left-to-right from secondary to
///   primary across the syllable's duration, finishing exactly when the
///   syllable time is over.
/// * [`Outline`](KaraokeKind::Outline) (`\ko`) — like `\k`, except the
///   glyph border/outline is *removed* before highlight and appears
///   instantly when the syllable starts.
///
/// The base parser collapses all three (plus `\K`) into a single
/// `oxideav_core::Segment::Karaoke` marker that does not record which
/// member was used, so the kind is only recoverable when parsing raw
/// override text directly (e.g. through [`parse_overrides`]). Karaoke
/// markers recovered from already-parsed `Segment::Karaoke` segments
/// therefore report [`KaraokeKind::Fill`] as the conservative default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KaraokeKind {
    /// `\k` — instant fill switch at the syllable boundary.
    Fill,
    /// `\kf` / `\K` — left-to-right secondary→primary sweep across the
    /// syllable.
    Sweep,
    /// `\ko` — outline removed before highlight, appears instantly.
    Outline,
}

/// One karaoke syllable's resolved timing span within a cue.
///
/// Produced by [`CueAnimation::karaoke_spans`]. Times are milliseconds
/// from the cue start; each span runs `[start_ms, end_ms)` and the next
/// syllable begins exactly where the previous one ends (the `\k`
/// durations are cumulative per the Aegisub spec, which gives each
/// syllable's duration in centiseconds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KaraokeSpan {
    /// Which `\k` member produced this syllable.
    pub kind: KaraokeKind,
    /// Start of the syllable, ms from cue start.
    pub start_ms: u32,
    /// End of the syllable (= start of the next syllable), ms from cue
    /// start.
    pub end_ms: u32,
}

impl KaraokeSpan {
    /// Fraction (`0.0..=1.0`) of the way through this syllable at
    /// `t_in_cue_ms`, milliseconds from the cue start.
    ///
    /// `0.0` before the syllable starts, `1.0` at or after its end. For
    /// a [`KaraokeKind::Sweep`] syllable this is the left-to-right wipe
    /// position; for [`KaraokeKind::Fill`] / [`KaraokeKind::Outline`]
    /// the renderer only needs to know whether the value crossed `0.0`
    /// (i.e. whether the syllable has started), since those switch
    /// instantly.
    pub fn progress(&self, t_in_cue_ms: i32) -> f32 {
        if t_in_cue_ms <= self.start_ms as i32 {
            return 0.0;
        }
        if t_in_cue_ms >= self.end_ms as i32 || self.end_ms <= self.start_ms {
            return 1.0;
        }
        (t_in_cue_ms - self.start_ms as i32) as f32 / (self.end_ms - self.start_ms) as f32
    }
}

/// One typed animated-tag occurrence found in a cue.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimatedTag {
    /// `\fad(t1, t2)` — alpha 0 → 255 over `t1` ms then 255 → 0 over
    /// `t2` ms (ASS alpha; converted to a `0.0..=1.0` multiplier in
    /// the evaluator).
    Fad { t1_ms: u32, t2_ms: u32 },
    /// `\fade(a1, a2, a3, t1, t2, t3, t4)` — full variant.
    Fade {
        a1: u8,
        a2: u8,
        a3: u8,
        t1_ms: i32,
        t2_ms: i32,
        t3_ms: i32,
        t4_ms: i32,
    },
    /// `\pos(x, y)` — set the static position of the line. Per the
    /// Aegisub spec the coordinates are in the script-resolution
    /// coordinate system and the line's alignment point is anchored
    /// there. Static (not animatable); it is the non-moving
    /// counterpart of [`AnimatedTag::Move`] and writes the same
    /// [`RenderState::translate`] field.
    Pos { x: f32, y: f32 },
    /// `\move(x1, y1, x2, y2[, t1, t2])`. `t1`/`t2` default to the cue
    /// span when omitted.
    Move {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        t1_ms: Option<i32>,
        t2_ms: Option<i32>,
    },
    /// `\frz(degrees)` — rotation around Z, applied as a static
    /// override at all times unless wrapped in `\t`.
    Frz(f32),
    /// `\blur(sigma)` — Gaussian blur sigma in px.
    Blur(f32),
    /// `\fscx(percent)` — horizontal scale, 100 = identity.
    Fscx(f32),
    /// `\fscy(percent)` — vertical scale, 100 = identity.
    Fscy(f32),
    /// `\c&Hbbggrr&` / `\1c...` — primary colour as RGB.
    Color1((u8, u8, u8)),
    /// `\fs(size)` — font size override (ignored by the evaluator
    /// transform, but exposed for scale recovery).
    Fs(f32),
    /// `\clip(x1, y1, x2, y2)` rectangle.
    ClipRect { x1: f32, y1: f32, x2: f32, y2: f32 },
    /// `\clip(drawing)` — drawing path form, stored verbatim. The
    /// renderer parses this through [`crate::drawing::parse_drawing`]
    /// into an `oxideav_core::Path` and uses it as a `Group::clip`
    /// mask.
    ClipDrawing(String),
    /// `\frx(degrees)` — rotation around the X axis (3D). Combined
    /// with `\frz`/`\fry` and projected to 2D via a perspective
    /// camera in the renderer.
    Frx(f32),
    /// `\fry(degrees)` — rotation around the Y axis (3D).
    Fry(f32),
    /// `\org(x, y)` — pivot for `\frx` / `\fry` / `\frz`. Without
    /// `\org`, the pivot is the cue's alignment point.
    Org { x: f32, y: f32 },
    /// `\bord(w)` — text border width in px (sets both X and Y).
    Bord(f32),
    /// `\xbord(w)` — X-axis border width (px).
    Xbord(f32),
    /// `\ybord(w)` — Y-axis border width (px).
    Ybord(f32),
    /// `\shad(d)` — text shadow distance in px (sets both axes, must
    /// be non-negative per the Aegisub spec).
    Shad(f32),
    /// `\xshad(d)` — X-axis shadow distance (px, may be negative).
    Xshad(f32),
    /// `\yshad(d)` — Y-axis shadow distance (px, may be negative).
    Yshad(f32),
    /// `\be(strength)` — iterative box-blur strength (integer). Edge-
    /// softening filter, kept separate from `\blur` per Aegisub spec.
    Be(u8),
    /// `\fax(factor)` — X-axis shear (perspective distortion).
    Fax(f32),
    /// `\fay(factor)` — Y-axis shear.
    Fay(f32),
    /// `\iclip(x1, y1, x2, y2)` — inverse rectangular clip; the cue
    /// is hidden inside the rectangle.
    IClipRect { x1: f32, y1: f32, x2: f32, y2: f32 },
    /// `\iclip(drawing)` — inverse vector-drawing clip, stored
    /// verbatim. Parse with [`crate::drawing::parse_drawing`] if a
    /// path is needed.
    IClipDrawing(String),
    /// `\2c&Hbbggrr&` — secondary fill colour (RGB).
    Color2((u8, u8, u8)),
    /// `\3c&Hbbggrr&` — border / outline colour (RGB).
    Color3((u8, u8, u8)),
    /// `\4c&Hbbggrr&` — shadow colour (RGB).
    Color4((u8, u8, u8)),
    /// `\alpha&Haa&` — sets the alpha of all four colour components at
    /// once (primary / secondary / border / shadow). ASS convention:
    /// 0 = opaque, 255 = transparent.
    Alpha(u8),
    /// `\1a&Haa&` — primary fill alpha. ASS convention.
    Alpha1(u8),
    /// `\2a&Haa&` — secondary fill alpha (pre-highlight karaoke).
    Alpha2(u8),
    /// `\3a&Haa&` — border alpha.
    Alpha3(u8),
    /// `\4a&Haa&` — shadow alpha.
    Alpha4(u8),
    /// `\fsp(spacing)` — additional advance between letters in
    /// script-resolution pixels. May be negative or decimal; default
    /// `0`. Animatable.
    Fsp(f32),
    /// `\q(style)` — wrap style for the line. Values per SSA spec:
    /// `0` = smart wrap balanced top-wider, `1` = end-of-line wrap,
    /// `2` = no wrapping, `3` = smart wrap balanced bottom-wider.
    /// Static (not animatable).
    Q(u8),
    /// `\an<pos>` — line alignment using "numpad" values per the
    /// Aegisub spec:
    ///
    /// * `1` = bottom-left,  `2` = bottom-center,  `3` = bottom-right
    /// * `4` = middle-left,  `5` = middle-center,  `6` = middle-right
    /// * `7` = top-left,     `8` = top-center,     `9` = top-right
    ///
    /// Out-of-range values are dropped by the parser (the static
    /// override path then keeps the script-style alignment). Static,
    /// not animatable per spec.
    An(u8),
    /// `\a<pos>` — legacy SubStation-Alpha alignment code. The parser
    /// converts each recognised legacy code to its numpad equivalent
    /// (`1`/`2`/`3` = bottom row; `+4` = top row; `+8` = middle row)
    /// so the renderer only ever has to inspect
    /// [`RenderState::alignment`]'s 1..=9 surface. Unrecognised codes
    /// are dropped.
    A(u8),
    /// `\fn<name>` — font family override for the following text. The
    /// Aegisub spec is explicit that no space sits between `\fn` and
    /// the name, and that surrounding parentheses are not part of the
    /// value, so the parameter is read verbatim up to the next `\` or
    /// the end of the override block. Empty names drop the tag (the
    /// renderer keeps the style's `Fontname`). Not animatable per
    /// spec — a typeface change cannot be interpolated; inside
    /// `\t(...)` the new face snaps in at `t > t1`, mirroring `\q`.
    Fn(String),
    /// `\fe<id>` — Windows font-encoding (charset) ID to use for the
    /// glyph-mapping table. Per the Aegisub spec, common values are
    /// `0` ANSI / `1` Default / `2` Symbol / `128` Shift-JIS / `134`
    /// GB2312 / `136` BIG5 / `162` Turkish / `163` Vietnamese / `177`
    /// Hebrew / `178` Arabic. The full Win32 charset numeric range is
    /// `0..=255`; values outside drop the override. Not animatable per
    /// spec — the encoding determines the glyph-mapping table and
    /// cannot be interpolated; inside `\t(...)` it snaps at `t > t1`.
    Fe(u8),
    /// `\r[<style>]` — reset all override state for the following
    /// text. The bare `\r` form drops back to the line's base style;
    /// the `\r<style>` form switches the base style to the named
    /// definition from the script `[V4+ Styles]` block (the typed
    /// surface here only carries the name — looking it up against the
    /// track's style table is the renderer's job). The parser strips
    /// surrounding whitespace from the name; an empty name decays to
    /// the bare-`\r` variant.
    R(Option<String>),
    /// `\b<weight>` — bold weight as an integer (per the Aegisub
    /// spec: `100..900` in steps of 100, where `400` = normal and
    /// `700` = bold). The legacy `\b1` / `\b0` toggles surface as
    /// `Some(700)` / `Some(0)` so the base parser's boolean is still
    /// honoured: any non-zero value renders bold, weight `0` (or
    /// anything below `100` rounded down) drops back to "not bold".
    /// The full integer weight is exposed on
    /// [`RenderState::bold_weight`] for renderers that pick a font
    /// face by weight; values outside the spec range are still
    /// surfaced verbatim so downstream code can decide its own
    /// fallback. Not animatable per spec — a typeface weight change
    /// cannot be interpolated meaningfully; inside `\t(...)` the
    /// post-state value snaps in at `t > t1`.
    B(u16),
    /// `\k` / `\K` / `\kf` / `\ko` — a karaoke syllable timing marker.
    /// `cs` is the syllable's duration in **centiseconds** (the unit the
    /// `\k` family uses; `100` = one second), and `kind` records which
    /// member of the family produced it. These markers appear once per
    /// syllable in document order; [`CueAnimation::karaoke_spans`]
    /// resolves them into cumulative millisecond [`KaraokeSpan`]s.
    ///
    /// Unlike the transform / colour tags this is a timeline-level
    /// concept rather than a per-frame state, so [`apply_tag`] treats it
    /// as a no-op on [`RenderState`]; renderers walk the spans instead.
    Karaoke { kind: KaraokeKind, cs: u32 },
    /// `\t([t1,t2,[accel,]] inner_tags)` — interpolate the inner tags
    /// over `[t1, t2]`. When `t1`/`t2` are omitted ASS treats them as
    /// `[0, cue_duration]`. `accel` defaults to 1.0 (linear).
    T {
        t1_ms: Option<i32>,
        t2_ms: Option<i32>,
        accel: f32,
        inner: Vec<AnimatedTag>,
    },
}

/// All animated tags found in a single cue, in the order parsed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CueAnimation {
    pub tags: Vec<AnimatedTag>,
}

impl CueAnimation {
    /// `true` iff there are no tags.
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    /// Resolve the cue's `\k` family markers into cumulative
    /// [`KaraokeSpan`]s, milliseconds from the cue start.
    ///
    /// Every [`AnimatedTag::Karaoke`] in `tags` (in document order)
    /// becomes one span; each span begins where the previous one ended,
    /// so the centisecond durations the `\k` tags carry add up into a
    /// continuous syllable timeline. Cues with no karaoke markers yield
    /// an empty vector. The centisecond → millisecond conversion is
    /// exact (`cs * 10`).
    pub fn karaoke_spans(&self) -> Vec<KaraokeSpan> {
        let mut spans = Vec::new();
        let mut cursor_ms: u32 = 0;
        for tag in &self.tags {
            if let AnimatedTag::Karaoke { kind, cs } = tag {
                let end_ms = cursor_ms.saturating_add(cs.saturating_mul(10));
                spans.push(KaraokeSpan {
                    kind: *kind,
                    start_ms: cursor_ms,
                    end_ms,
                });
                cursor_ms = end_ms;
            }
        }
        spans
    }
}

/// Resolved state of the cue at a particular timestamp.
///
/// All quantities are expressed in the cue's local coordinate space
/// (the same space `\pos` / `\move` use). `transform` composes
/// `move` ∘ `scale` ∘ `rotate` so the rotation pivot is the
/// translation point.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderState {
    /// `1.0` = fully opaque, `0.0` = fully transparent.
    pub alpha_mul: f32,
    /// Combined affine transform to apply to the rendered text glyph
    /// group.
    pub transform: Transform2D,
    /// `\frz` rotation in radians (also baked into `transform` but
    /// exposed for renderers that compose their own matrix).
    pub rotate_radians: f32,
    /// `\frx` rotation in radians (X axis, 3D). Renderers project
    /// this to 2D via a perspective camera anchored at `pivot`.
    pub rotate_x_radians: f32,
    /// `\fry` rotation in radians (Y axis, 3D).
    pub rotate_y_radians: f32,
    /// `(sx, sy)` scale factors, where `1.0` = 100%.
    pub scale: (f32, f32),
    /// `(tx, ty)` translation. `None` when neither `\pos` nor `\move`
    /// applied, in which case the renderer falls back to the cue's
    /// style margins.
    pub translate: Option<(f32, f32)>,
    /// Gaussian blur sigma in pixels. `0.0` = no blur.
    pub blur_sigma: f32,
    /// Active rectangular clip in cue local coordinates, if any.
    pub clip_rect: Option<ClipRect>,
    /// `\clip(drawing)` raw drawing string, if active. Parse through
    /// [`crate::drawing::parse_drawing`] for a vector path mask.
    pub clip_drawing: Option<String>,
    /// `\c` primary-colour override, if active.
    pub primary_color: Option<(u8, u8, u8)>,
    /// `\fs` size override, if active.
    pub font_size: Option<f32>,
    /// `\org(x, y)` pivot point for `\frz` / `\frx` / `\fry`. `None`
    /// means "use the alignment point" (the renderer fills it in).
    pub pivot: Option<(f32, f32)>,
    /// `(x_border, y_border)` per-axis text border width in px from
    /// `\bord` / `\xbord` / `\ybord`. `None` = fall back to style.
    pub border: Option<(f32, f32)>,
    /// `(x_shadow, y_shadow)` per-axis shadow distance in px from
    /// `\shad` / `\xshad` / `\yshad`. `None` = fall back to style.
    /// Per-axis values may be negative; `\shad` itself is clamped to
    /// non-negative per spec.
    pub shadow: Option<(f32, f32)>,
    /// `\be(N)` iterative box-blur strength (0 = off). Distinct from
    /// `blur_sigma` (`\blur`).
    pub be_strength: u8,
    /// `(fax, fay)` shear factors applied after rotation. `(0.0, 0.0)`
    /// = no shear.
    pub shear: (f32, f32),
    /// Active inverse rectangular clip from `\iclip(x1,y1,x2,y2)`.
    /// Renderers should hide pixels *inside* this rectangle.
    pub iclip_rect: Option<ClipRect>,
    /// `\iclip(drawing)` raw drawing string; renderer parses to a
    /// path and masks against its inverse.
    pub iclip_drawing: Option<String>,
    /// `\2c` secondary fill colour override, if active.
    pub secondary_color: Option<(u8, u8, u8)>,
    /// `\3c` border / outline colour override, if active.
    pub outline_color: Option<(u8, u8, u8)>,
    /// `\4c` shadow colour override, if active.
    pub shadow_color: Option<(u8, u8, u8)>,
    /// `\1a` primary fill alpha (0 = opaque, 255 = transparent), if
    /// set. `None` means "fall back to style alpha". Independent of
    /// [`Self::alpha_mul`], which is the `\fad` / `\fade` cue-level
    /// envelope. Renderers compose:
    /// `final_primary_alpha = primary_alpha.unwrap_or(style) * alpha_mul`.
    pub primary_alpha: Option<u8>,
    /// `\2a` secondary fill alpha, if set.
    pub secondary_alpha: Option<u8>,
    /// `\3a` border / outline alpha, if set.
    pub outline_alpha: Option<u8>,
    /// `\4a` shadow alpha, if set.
    pub shadow_alpha: Option<u8>,
    /// `\fsp` additional letter-spacing in script-resolution pixels,
    /// if set. `None` = use the style's `Spacing` field. May be
    /// negative or decimal.
    pub letter_spacing: Option<f32>,
    /// `\q` wrap-style override for the line, if set. `None` = use
    /// the script's `WrapStyle` header. Values per SSA spec:
    /// `0` smart-top / `1` EOL / `2` no-wrap / `3` smart-bottom.
    /// Not animatable per spec.
    pub wrap_style: Option<u8>,
    /// `\an<pos>` (or its legacy `\a<pos>` form, converted to numpad)
    /// alignment override for the line, if set. `None` = fall back to
    /// the cue's style `Alignment`. Values are the Aegisub numpad
    /// codes 1..=9:
    ///
    /// * `1`/`2`/`3` — bottom-left / bottom-center / bottom-right
    /// * `4`/`5`/`6` — middle-left / middle-center / middle-right
    /// * `7`/`8`/`9` — top-left  / top-center  / top-right
    ///
    /// The alignment doubles as the anchor point for `\pos` / `\move`
    /// translation per the Aegisub spec, so renderers should look here
    /// to decide which glyph corner sits on the `translate` point.
    /// Static, not animatable.
    pub alignment: Option<u8>,
    /// `\fn<name>` font family override for this segment, if set.
    /// `None` = fall back to the style's `Fontname`. Empty / whitespace-
    /// only names are dropped by the parser.
    pub font_name: Option<String>,
    /// `\fe<id>` Windows charset ID for the glyph-mapping table, if
    /// set. `None` = fall back to the style's `Encoding`. Valid range
    /// `0..=255` per the Win32 charset enum; values outside drop the
    /// override.
    pub font_encoding: Option<u8>,
    /// `\b<weight>` font-weight override, if set. `None` = fall back
    /// to the style's `Bold` field. `0` = explicitly not-bold; the
    /// Aegisub spec's named slots are `100`/`300`/`500`/`700`/`900`;
    /// `\b1` shortcut surfaces as `Some(700)`. Renderers pick the
    /// closest available face weight.
    pub bold_weight: Option<u16>,
    /// `\r[<style>]` style-reset target, if a `\r` was seen on this
    /// segment. `Some(None)` means a bare `\r` (reset to the line's
    /// base style); `Some(Some(name))` means `\r<name>` (reset to the
    /// named style from the script's `[V4+ Styles]` block). The
    /// renderer is responsible for looking the name up against the
    /// track's style table — the typed surface here only carries the
    /// requested target. Applying a `\r` also clears every other
    /// override field on this state (back to identity) per the spec
    /// "cancels all style overrides in effect" rule; the
    /// `reset_to_style` slot stays set so callers can tell a reset
    /// happened.
    pub reset_to_style: Option<Option<String>>,
}

impl RenderState {
    /// State with no animated overrides.
    pub fn identity() -> Self {
        Self {
            alpha_mul: 1.0,
            transform: Transform2D::identity(),
            rotate_radians: 0.0,
            rotate_x_radians: 0.0,
            rotate_y_radians: 0.0,
            scale: (1.0, 1.0),
            translate: None,
            blur_sigma: 0.0,
            clip_rect: None,
            clip_drawing: None,
            primary_color: None,
            font_size: None,
            pivot: None,
            border: None,
            shadow: None,
            be_strength: 0,
            shear: (0.0, 0.0),
            iclip_rect: None,
            iclip_drawing: None,
            secondary_color: None,
            outline_color: None,
            shadow_color: None,
            primary_alpha: None,
            secondary_alpha: None,
            outline_alpha: None,
            shadow_alpha: None,
            letter_spacing: None,
            wrap_style: None,
            alignment: None,
            font_name: None,
            font_encoding: None,
            bold_weight: None,
            reset_to_style: None,
        }
    }
}

impl Default for RenderState {
    fn default() -> Self {
        Self::identity()
    }
}

/// Active rectangular clip region, normalised so x1 <= x2 and
/// y1 <= y2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipRect {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl CueAnimation {
    /// Sample the cue at `t_in_cue_ms` (milliseconds from cue start).
    ///
    /// `cue_duration_ms` is needed because `\move` and `\t` accept
    /// `t1`/`t2` arguments that default to "the entire cue".
    pub fn evaluate_at(&self, t_in_cue_ms: i32, cue_duration_ms: i32) -> RenderState {
        let mut st = RenderState::identity();
        for tag in &self.tags {
            apply_tag(&mut st, tag, t_in_cue_ms, cue_duration_ms);
        }
        st.transform = compose_transform(&st);
        st
    }
}

fn compose_transform(st: &RenderState) -> Transform2D {
    let (sx, sy) = st.scale;
    let mut t = Transform2D::identity();
    if (sx - 1.0).abs() > f32::EPSILON || (sy - 1.0).abs() > f32::EPSILON {
        t = t.compose(&Transform2D::scale(sx, sy));
    }
    if st.rotate_radians.abs() > f32::EPSILON {
        t = Transform2D::rotate(st.rotate_radians).compose(&t);
    }
    if let Some((tx, ty)) = st.translate {
        t = Transform2D::translate(tx, ty).compose(&t);
    }
    t
}

fn apply_tag(st: &mut RenderState, tag: &AnimatedTag, t_ms: i32, dur_ms: i32) {
    match tag {
        AnimatedTag::Fad { t1_ms, t2_ms } => {
            st.alpha_mul *= fad_alpha(*t1_ms as i32, *t2_ms as i32, t_ms, dur_ms);
        }
        AnimatedTag::Fade {
            a1,
            a2,
            a3,
            t1_ms,
            t2_ms,
            t3_ms,
            t4_ms,
        } => {
            let a = fade_alpha(*a1, *a2, *a3, *t1_ms, *t2_ms, *t3_ms, *t4_ms, t_ms);
            st.alpha_mul *= ass_alpha_to_mul(a);
        }
        AnimatedTag::Pos { x, y } => {
            // \pos is the static counterpart of \move; both write the
            // line position into `translate`. Last writer wins, matching
            // the rest of this module's static-override model — so a
            // later \move (or \pos) overrides an earlier \pos.
            st.translate = Some((*x, *y));
        }
        AnimatedTag::Move {
            x1,
            y1,
            x2,
            y2,
            t1_ms,
            t2_ms,
        } => {
            let t1 = t1_ms.unwrap_or(0);
            let t2 = t2_ms.unwrap_or(dur_ms);
            let p = lerp_xy((*x1, *y1), (*x2, *y2), t1, t2, t_ms);
            st.translate = Some(p);
        }
        AnimatedTag::Frz(deg) => {
            st.rotate_radians = deg.to_radians();
        }
        AnimatedTag::Blur(sigma) => {
            st.blur_sigma = sigma.max(0.0);
        }
        AnimatedTag::Fscx(pct) => {
            st.scale.0 = pct / 100.0;
        }
        AnimatedTag::Fscy(pct) => {
            st.scale.1 = pct / 100.0;
        }
        AnimatedTag::Color1(rgb) => {
            st.primary_color = Some(*rgb);
        }
        AnimatedTag::Fs(size) => {
            st.font_size = Some(*size);
        }
        AnimatedTag::ClipRect { x1, y1, x2, y2 } => {
            let (lo_x, hi_x) = if x1 <= x2 { (*x1, *x2) } else { (*x2, *x1) };
            let (lo_y, hi_y) = if y1 <= y2 { (*y1, *y2) } else { (*y2, *y1) };
            st.clip_rect = Some(ClipRect {
                x1: lo_x,
                y1: lo_y,
                x2: hi_x,
                y2: hi_y,
            });
        }
        AnimatedTag::ClipDrawing(s) => {
            st.clip_drawing = Some(s.clone());
        }
        AnimatedTag::Frx(deg) => {
            st.rotate_x_radians = deg.to_radians();
        }
        AnimatedTag::Fry(deg) => {
            st.rotate_y_radians = deg.to_radians();
        }
        AnimatedTag::Org { x, y } => {
            st.pivot = Some((*x, *y));
        }
        AnimatedTag::Bord(w) => {
            // \bord sets both axes — per Aegisub spec, "if you use
            // \bord after \xbord or \ybord, it will [override] them".
            let w = w.max(0.0);
            st.border = Some((w, w));
        }
        AnimatedTag::Xbord(w) => {
            let w = w.max(0.0);
            let (_, y) = st.border.unwrap_or((0.0, 0.0));
            st.border = Some((w, y));
        }
        AnimatedTag::Ybord(w) => {
            let w = w.max(0.0);
            let (x, _) = st.border.unwrap_or((0.0, 0.0));
            st.border = Some((x, w));
        }
        AnimatedTag::Shad(d) => {
            // \shad is non-negative per spec.
            let d = d.max(0.0);
            st.shadow = Some((d, d));
        }
        AnimatedTag::Xshad(d) => {
            // \xshad / \yshad may be negative.
            let (_, y) = st.shadow.unwrap_or((0.0, 0.0));
            st.shadow = Some((*d, y));
        }
        AnimatedTag::Yshad(d) => {
            let (x, _) = st.shadow.unwrap_or((0.0, 0.0));
            st.shadow = Some((x, *d));
        }
        AnimatedTag::Be(n) => {
            st.be_strength = *n;
        }
        AnimatedTag::Fax(f) => {
            st.shear.0 = *f;
        }
        AnimatedTag::Fay(f) => {
            st.shear.1 = *f;
        }
        AnimatedTag::IClipRect { x1, y1, x2, y2 } => {
            let (lo_x, hi_x) = if x1 <= x2 { (*x1, *x2) } else { (*x2, *x1) };
            let (lo_y, hi_y) = if y1 <= y2 { (*y1, *y2) } else { (*y2, *y1) };
            st.iclip_rect = Some(ClipRect {
                x1: lo_x,
                y1: lo_y,
                x2: hi_x,
                y2: hi_y,
            });
        }
        AnimatedTag::IClipDrawing(s) => {
            st.iclip_drawing = Some(s.clone());
        }
        AnimatedTag::Color2(rgb) => {
            st.secondary_color = Some(*rgb);
        }
        AnimatedTag::Color3(rgb) => {
            st.outline_color = Some(*rgb);
        }
        AnimatedTag::Color4(rgb) => {
            st.shadow_color = Some(*rgb);
        }
        AnimatedTag::Alpha(a) => {
            // \alpha sets all four channels at once.
            st.primary_alpha = Some(*a);
            st.secondary_alpha = Some(*a);
            st.outline_alpha = Some(*a);
            st.shadow_alpha = Some(*a);
        }
        AnimatedTag::Alpha1(a) => {
            st.primary_alpha = Some(*a);
        }
        AnimatedTag::Alpha2(a) => {
            st.secondary_alpha = Some(*a);
        }
        AnimatedTag::Alpha3(a) => {
            st.outline_alpha = Some(*a);
        }
        AnimatedTag::Alpha4(a) => {
            st.shadow_alpha = Some(*a);
        }
        AnimatedTag::Fsp(s) => {
            st.letter_spacing = Some(*s);
        }
        AnimatedTag::Q(mode) => {
            // Clamp to the four spec values; out-of-range modes fall
            // back to the script header's WrapStyle (no override).
            if *mode <= 3 {
                st.wrap_style = Some(*mode);
            }
        }
        AnimatedTag::An(n) => {
            // ASS numpad alignment: 1..=9 valid; values outside drop
            // the override (renderer keeps the style's Alignment).
            if (1..=9).contains(n) {
                st.alignment = Some(*n);
            }
        }
        AnimatedTag::A(n) => {
            // Legacy SSA alignment: convert to the equivalent numpad
            // code per the Aegisub spec. Low nibble = L/C/R, +4 = top,
            // +8 = mid (= ASS bot/mid/top rows are 1-3 / 7-9 / 4-6 on
            // the numpad). Unrecognised codes drop the override.
            if let Some(numpad) = ssa_alignment_to_numpad(*n) {
                st.alignment = Some(numpad);
            }
        }
        AnimatedTag::Karaoke { .. } => {
            // Timeline-level concept: the per-syllable highlight timing
            // lives on the cue, not on the single-instant RenderState.
            // Renderers walk CueAnimation::karaoke_spans() to find which
            // syllable is active and how far its highlight has advanced.
            // Nothing to apply to the affine / colour / alpha state here.
        }
        AnimatedTag::Fn(name) => {
            // Whitespace-only names already dropped by `parse_one`, so
            // anything reaching the evaluator is a renderable family
            // request. Clone into the state — the renderer borrows it.
            st.font_name = Some(name.clone());
        }
        AnimatedTag::Fe(id) => {
            // Win32 charset IDs are documented 0..=255; the parser
            // already clamped to a u8 so the slot is always valid.
            st.font_encoding = Some(*id);
        }
        AnimatedTag::B(weight) => {
            st.bold_weight = Some(*weight);
        }
        AnimatedTag::R(name) => {
            // Aegisub spec: "cancels all style overrides in effect,
            // including animations, for all following text." Reset
            // everything to identity, then record the target so the
            // renderer can either drop back to the line's style (None)
            // or look the named style up against the script's
            // `[V4+ Styles]` block (Some(name)).
            *st = RenderState::identity();
            st.reset_to_style = Some(name.clone());
        }
        AnimatedTag::T {
            t1_ms,
            t2_ms,
            accel,
            inner,
        } => {
            apply_t(st, *t1_ms, *t2_ms, *accel, inner, t_ms, dur_ms);
        }
    }
}

fn apply_t(
    st: &mut RenderState,
    t1: Option<i32>,
    t2: Option<i32>,
    accel: f32,
    inner: &[AnimatedTag],
    t_ms: i32,
    dur_ms: i32,
) {
    let start = t1.unwrap_or(0);
    let end = t2.unwrap_or(dur_ms);
    // Snapshot pre-transition state for interpolation source.
    let pre = st.clone();
    // Apply each inner tag to get the post-state.
    let mut post = pre.clone();
    for tag in inner {
        apply_tag(&mut post, tag, t_ms, dur_ms);
    }
    // Compute the interpolation factor in [0,1].
    let raw = if end <= start {
        if t_ms >= end {
            1.0
        } else {
            0.0
        }
    } else if t_ms <= start {
        0.0
    } else if t_ms >= end {
        1.0
    } else {
        (t_ms - start) as f32 / (end - start) as f32
    };
    let k = if accel.abs() < f32::EPSILON {
        raw
    } else {
        raw.powf(accel)
    };
    // Interpolate every field that the inner tags could have touched.
    st.scale.0 = lerp_f32(pre.scale.0, post.scale.0, k);
    st.scale.1 = lerp_f32(pre.scale.1, post.scale.1, k);
    st.rotate_radians = lerp_f32(pre.rotate_radians, post.rotate_radians, k);
    st.rotate_x_radians = lerp_f32(pre.rotate_x_radians, post.rotate_x_radians, k);
    st.rotate_y_radians = lerp_f32(pre.rotate_y_radians, post.rotate_y_radians, k);
    st.blur_sigma = lerp_f32(pre.blur_sigma, post.blur_sigma, k).max(0.0);
    st.alpha_mul = lerp_f32(pre.alpha_mul, post.alpha_mul, k);
    if let Some(c) = post.primary_color {
        let from = pre.primary_color.unwrap_or(c);
        st.primary_color = Some(lerp_rgb(from, c, k));
    }
    if let Some(s) = post.font_size {
        let from = pre.font_size.unwrap_or(s);
        st.font_size = Some(lerp_f32(from, s, k));
    }
    if let Some((px, py)) = post.translate {
        let (fx, fy) = pre.translate.unwrap_or((px, py));
        st.translate = Some((lerp_f32(fx, px, k), lerp_f32(fy, py, k)));
    }
    // Border / shadow / be / shear interpolation. \bord and \shad
    // ramp linearly per axis; for \be the integer strength is
    // round-clamped at each sample.
    if let Some((px, py)) = post.border {
        let (fx, fy) = pre.border.unwrap_or((px, py));
        st.border = Some((lerp_f32(fx, px, k), lerp_f32(fy, py, k)));
    }
    if let Some((px, py)) = post.shadow {
        let (fx, fy) = pre.shadow.unwrap_or((px, py));
        st.shadow = Some((lerp_f32(fx, px, k), lerp_f32(fy, py, k)));
    }
    if post.be_strength != pre.be_strength {
        let from = pre.be_strength as f32;
        let to = post.be_strength as f32;
        st.be_strength = lerp_f32(from, to, k).clamp(0.0, 255.0).round() as u8;
    }
    st.shear.0 = lerp_f32(pre.shear.0, post.shear.0, k);
    st.shear.1 = lerp_f32(pre.shear.1, post.shear.1, k);
    // Per-component colours \2c / \3c / \4c interpolate just like \1c.
    if let Some(c) = post.secondary_color {
        let from = pre.secondary_color.unwrap_or(c);
        st.secondary_color = Some(lerp_rgb(from, c, k));
    }
    if let Some(c) = post.outline_color {
        let from = pre.outline_color.unwrap_or(c);
        st.outline_color = Some(lerp_rgb(from, c, k));
    }
    if let Some(c) = post.shadow_color {
        let from = pre.shadow_color.unwrap_or(c);
        st.shadow_color = Some(lerp_rgb(from, c, k));
    }
    // Per-component alphas \1a..\4a interpolate as u8 linearly.
    if let Some(a) = post.primary_alpha {
        let from = pre.primary_alpha.unwrap_or(a);
        st.primary_alpha = Some(lerp_u8(from, a, k));
    }
    if let Some(a) = post.secondary_alpha {
        let from = pre.secondary_alpha.unwrap_or(a);
        st.secondary_alpha = Some(lerp_u8(from, a, k));
    }
    if let Some(a) = post.outline_alpha {
        let from = pre.outline_alpha.unwrap_or(a);
        st.outline_alpha = Some(lerp_u8(from, a, k));
    }
    if let Some(a) = post.shadow_alpha {
        let from = pre.shadow_alpha.unwrap_or(a);
        st.shadow_alpha = Some(lerp_u8(from, a, k));
    }
    // \fsp ramps linearly per spec; falls back to pre when post has no
    // override.
    if let Some(s) = post.letter_spacing {
        let from = pre.letter_spacing.unwrap_or(s);
        st.letter_spacing = Some(lerp_f32(from, s, k));
    }
    // \q is non-animatable: snap to the post-state value at t >= t1
    // (k > 0), keep pre below.
    if post.wrap_style != pre.wrap_style {
        st.wrap_style = if k > 0.0 {
            post.wrap_style
        } else {
            pre.wrap_style
        };
    }
    // \an / \a are non-animatable per spec — snap on the same k > 0
    // boundary as \q.
    if post.alignment != pre.alignment {
        st.alignment = if k > 0.0 {
            post.alignment
        } else {
            pre.alignment
        };
    }
    // \fn / \fe / \b are typeface-changing tags — a font face cannot
    // be interpolated, so they snap at t > t1 like \q / \an. Per the
    // Aegisub spec these tags are explicitly listed under the
    // non-animatable group in the override-tag reference.
    if post.font_name != pre.font_name {
        st.font_name = if k > 0.0 {
            post.font_name.clone()
        } else {
            pre.font_name.clone()
        };
    }
    if post.font_encoding != pre.font_encoding {
        st.font_encoding = if k > 0.0 {
            post.font_encoding
        } else {
            pre.font_encoding
        };
    }
    if post.bold_weight != pre.bold_weight {
        st.bold_weight = if k > 0.0 {
            post.bold_weight
        } else {
            pre.bold_weight
        };
    }
    // \r is special: it resets the entire state. Snap the reset target
    // on the same k > 0 boundary; when the reset fires, every other
    // field is already at identity (apply_tag wiped them in the post
    // pass) so the surrounding interpolation falls through cleanly.
    if post.reset_to_style != pre.reset_to_style {
        st.reset_to_style = if k > 0.0 {
            post.reset_to_style.clone()
        } else {
            pre.reset_to_style.clone()
        };
    }
}

/// Convert a legacy SSA `\a<pos>` code to the equivalent ASS numpad
/// (`\an<N>`) value, per the Aegisub spec:
///
/// > Use 1 for left-alignment, 2 for center alignment and 3 for
/// > right-alignment. … To get top-titles, add 4 to the number, to
/// > get mid-titles add 8 to the number.
///
/// Returns `None` for codes that do not match a documented legacy
/// alignment slot.
fn ssa_alignment_to_numpad(n: u8) -> Option<u8> {
    // Sub-titles (bottom row): 1, 2, 3 → numpad 1, 2, 3.
    // Top-titles (+4):         5, 6, 7 → numpad 7, 8, 9.
    // Mid-titles (+8):         9, 10, 11 → numpad 4, 5, 6.
    match n {
        1 => Some(1),
        2 => Some(2),
        3 => Some(3),
        5 => Some(7),
        6 => Some(8),
        7 => Some(9),
        9 => Some(4),
        10 => Some(5),
        11 => Some(6),
        _ => None,
    }
}

fn lerp_u8(a: u8, b: u8, k: f32) -> u8 {
    let v = a as f32 + (b as f32 - a as f32) * k;
    v.clamp(0.0, 255.0).round() as u8
}

fn fad_alpha(t1: i32, t2: i32, t: i32, dur: i32) -> f32 {
    let t = t.max(0);
    let dur = dur.max(0);
    let mul_in = if t1 <= 0 {
        1.0
    } else if t < t1 {
        t as f32 / t1 as f32
    } else {
        1.0
    };
    let fade_out_start = (dur - t2).max(0);
    let mul_out = if t2 <= 0 {
        1.0
    } else if t >= dur {
        0.0
    } else if t > fade_out_start {
        ((dur - t) as f32 / t2 as f32).clamp(0.0, 1.0)
    } else {
        1.0
    };
    (mul_in * mul_out).clamp(0.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
fn fade_alpha(a1: u8, a2: u8, a3: u8, t1: i32, t2: i32, t3: i32, t4: i32, t: i32) -> u8 {
    let lerp_u8 = |from: u8, to: u8, k: f32| -> u8 {
        let v = from as f32 + (to as f32 - from as f32) * k;
        v.clamp(0.0, 255.0) as u8
    };
    if t < t1 {
        a1
    } else if t < t2 {
        let span = (t2 - t1).max(1);
        lerp_u8(a1, a2, (t - t1) as f32 / span as f32)
    } else if t < t3 {
        a2
    } else if t < t4 {
        let span = (t4 - t3).max(1);
        lerp_u8(a2, a3, (t - t3) as f32 / span as f32)
    } else {
        a3
    }
}

fn ass_alpha_to_mul(a: u8) -> f32 {
    // ASS: 0 = opaque, 255 = transparent. Our mul: 1.0 = opaque.
    1.0 - (a as f32 / 255.0)
}

fn lerp_f32(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}

fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), k: f32) -> (u8, u8, u8) {
    let lerp_c = |from: u8, to: u8| -> u8 {
        let v = from as f32 + (to as f32 - from as f32) * k;
        v.clamp(0.0, 255.0) as u8
    };
    (lerp_c(a.0, b.0), lerp_c(a.1, b.1), lerp_c(a.2, b.2))
}

fn lerp_xy(a: (f32, f32), b: (f32, f32), t1: i32, t2: i32, t: i32) -> (f32, f32) {
    let k = if t2 <= t1 {
        if t >= t2 {
            1.0
        } else {
            0.0
        }
    } else if t <= t1 {
        0.0
    } else if t >= t2 {
        1.0
    } else {
        (t - t1) as f32 / (t2 - t1) as f32
    };
    (lerp_f32(a.0, b.0, k), lerp_f32(a.1, b.1, k))
}

// ---------------------------------------------------------------------------
// Extraction from a SubtitleCue.

/// Walk `cue.segments` and pull out every animated tag stored in
/// `Segment::Raw` blocks.
///
/// The raw blocks were emitted by the parser as `{\fad(...)}` /
/// `{\move(...)}` / etc. so we re-parse them here to surface typed
/// values without losing the original text (the round-trip path keeps
/// using the `Raw` segments directly).
pub fn extract_cue_animation(cue: &SubtitleCue) -> CueAnimation {
    let mut tags: Vec<AnimatedTag> = Vec::new();
    walk_segments(&cue.segments, &mut tags);
    CueAnimation { tags }
}

fn walk_segments(segs: &[Segment], out: &mut Vec<AnimatedTag>) {
    for s in segs {
        match s {
            Segment::Raw(raw) => parse_raw_block(raw, out),
            Segment::Bold(c) | Segment::Italic(c) | Segment::Underline(c) | Segment::Strike(c) => {
                walk_segments(c, out)
            }
            Segment::Color { children, .. }
            | Segment::Font { children, .. }
            | Segment::Voice { children, .. }
            | Segment::Class { children, .. } => walk_segments(children, out),
            Segment::Karaoke { cs, children } => {
                // The base parser collapses `\k` / `\K` / `\kf` / `\ko`
                // into this marker without keeping which member it was,
                // so the kind is reported as the conservative Fill
                // default. The centisecond duration survives, which is
                // what `karaoke_spans` needs for the syllable timeline.
                out.push(AnimatedTag::Karaoke {
                    kind: KaraokeKind::Fill,
                    cs: *cs,
                });
                walk_segments(children, out);
            }
            _ => {}
        }
    }
}

fn parse_raw_block(raw: &str, out: &mut Vec<AnimatedTag>) {
    // Strip the wrapping `{` `}` if present; the parser emits both
    // `{\fad(...)}` and bare `\fad(...)` so handle both.
    let inner = raw.trim();
    let inner = inner.strip_prefix('{').unwrap_or(inner);
    let inner = inner.strip_suffix('}').unwrap_or(inner);
    parse_overrides(inner, out);
}

/// Parse animated overrides from a single override block (the bit
/// between `{` and `}` in a Dialogue line).
///
/// Tags this module doesn't recognise are silently skipped; the
/// round-trip text path retains them via the existing `Segment::Raw`
/// store.
pub fn parse_overrides(block: &str, out: &mut Vec<AnimatedTag>) {
    let bytes = block.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            i += 1;
            continue;
        }
        i += 1;
        // Tag name: optional leading digit then alphabetic.
        let name_start = i;
        if i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
        } else {
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
        }
        let name = &block[name_start..i];
        if name.is_empty() {
            continue;
        }
        let (param, advance) = read_param(&block[i..]);
        i += advance;
        let name_lc = name.to_ascii_lowercase();
        // `name` (original case) is passed alongside the lowercased form
        // because the karaoke family is case-sensitive: `\K` (uppercase)
        // is the secondary→primary sweep, identical to `\kf`, while `\k`
        // (lowercase) is the instant fill switch.
        if let Some(t) = parse_one(&name_lc, name, &param) {
            out.push(t);
        }
    }
}

/// Read a tag's parameter starting at `s` (after the tag name).
/// Returns `(param_text, bytes_consumed)`. Handles parenthesised
/// groups (which may contain commas + nested `\` for `\t(...)`).
fn read_param(s: &str) -> (String, usize) {
    let bytes = s.as_bytes();
    if bytes.first() == Some(&b'(') {
        // Parenthesised — find the matching `)` accounting for
        // nesting (`\t(0,500,\fscx(120))`).
        let mut depth: i32 = 0;
        let mut idx = 0;
        for (k, &b) in bytes.iter().enumerate() {
            if b == b'(' {
                depth += 1;
            } else if b == b')' {
                depth -= 1;
                if depth == 0 {
                    idx = k;
                    break;
                }
            }
        }
        if idx == 0 {
            // Unterminated — take to end.
            return (s[1..].to_string(), bytes.len());
        }
        (s[1..idx].to_string(), idx + 1)
    } else {
        // Bare parameter — until next `\` or end.
        let mut k = 0;
        while k < bytes.len() && bytes[k] != b'\\' {
            k += 1;
        }
        (s[..k].to_string(), k)
    }
}

fn parse_one(name_lc: &str, name_orig: &str, param: &str) -> Option<AnimatedTag> {
    // `\fn<name>` and `\r[<name>]` have no separator between the tag
    // and the inline name — the tokenizer greedily eats every
    // alphabetic byte into the tag-name slot, so they arrive here as
    // `name = "fnArial"` / `name = "rAlternate"` with `param = ""`.
    // Split the inline name back out before matching by short prefix.
    if name_lc.starts_with("fn") && name_lc.len() > 2 {
        // `\fnArial` → name = "fnArial", param = ""
        // `\fnTimes New Roman` → name = "fnTimes", param = " New Roman"
        //   (the tokenizer stops the name run at the first non-
        //   alphabetic, then read_param's bare-param mode picks up
        //   the rest until the next `\`).
        let head = &name_orig[2..];
        let full = if param.is_empty() {
            head.trim().to_string()
        } else {
            format!("{}{}", head, param).trim().to_string()
        };
        if full.is_empty() {
            return None;
        }
        return Some(AnimatedTag::Fn(full));
    }
    if name_lc.starts_with('r') && name_lc.len() > 1 && param.is_empty() {
        // Anything starting with `r` and a body — `\rAlt`, `\rDefault`
        // etc. — is the named-style reset. The bare `\r` matches the
        // "r" arm below (len == 1).
        let style = &name_orig[1..];
        let style = style.trim();
        if style.is_empty() {
            return Some(AnimatedTag::R(None));
        }
        return Some(AnimatedTag::R(Some(style.to_string())));
    }
    match name_lc {
        "fad" => {
            let nums = parse_int_list(param);
            if nums.len() >= 2 {
                Some(AnimatedTag::Fad {
                    t1_ms: nums[0].max(0) as u32,
                    t2_ms: nums[1].max(0) as u32,
                })
            } else {
                None
            }
        }
        "fade" => {
            let nums = parse_int_list(param);
            if nums.len() >= 7 {
                Some(AnimatedTag::Fade {
                    a1: nums[0].clamp(0, 255) as u8,
                    a2: nums[1].clamp(0, 255) as u8,
                    a3: nums[2].clamp(0, 255) as u8,
                    t1_ms: nums[3],
                    t2_ms: nums[4],
                    t3_ms: nums[5],
                    t4_ms: nums[6],
                })
            } else {
                None
            }
        }
        "move" => {
            let nums = parse_float_list(param);
            match nums.len() {
                4 => Some(AnimatedTag::Move {
                    x1: nums[0],
                    y1: nums[1],
                    x2: nums[2],
                    y2: nums[3],
                    t1_ms: None,
                    t2_ms: None,
                }),
                6 => Some(AnimatedTag::Move {
                    x1: nums[0],
                    y1: nums[1],
                    x2: nums[2],
                    y2: nums[3],
                    t1_ms: Some(nums[4] as i32),
                    t2_ms: Some(nums[5] as i32),
                }),
                _ => None,
            }
        }
        "frz" | "fr" => param.trim().parse::<f32>().ok().map(AnimatedTag::Frz),
        "frx" => param.trim().parse::<f32>().ok().map(AnimatedTag::Frx),
        "fry" => param.trim().parse::<f32>().ok().map(AnimatedTag::Fry),
        "pos" => {
            // `\pos(x, y)` — static line position. The spec requires
            // integer coordinates, but decimal values appear in the
            // wild, so parse as floats like \move / \org do.
            let n = parse_float_list(param);
            if n.len() == 2 {
                Some(AnimatedTag::Pos { x: n[0], y: n[1] })
            } else {
                None
            }
        }
        "org" => {
            let n = parse_float_list(param);
            if n.len() == 2 {
                Some(AnimatedTag::Org { x: n[0], y: n[1] })
            } else {
                None
            }
        }
        "blur" => param.trim().parse::<f32>().ok().map(AnimatedTag::Blur),
        "be" => {
            // `\be(N)` — iterative box-blur; the spec requires an
            // integer strength. Accept floats from the wild and round.
            let n = param.trim().parse::<f32>().ok()?;
            let n = n.clamp(0.0, 255.0).round() as u8;
            Some(AnimatedTag::Be(n))
        }
        "bord" => param.trim().parse::<f32>().ok().map(AnimatedTag::Bord),
        "xbord" => param.trim().parse::<f32>().ok().map(AnimatedTag::Xbord),
        "ybord" => param.trim().parse::<f32>().ok().map(AnimatedTag::Ybord),
        "shad" => param.trim().parse::<f32>().ok().map(AnimatedTag::Shad),
        "xshad" => param.trim().parse::<f32>().ok().map(AnimatedTag::Xshad),
        "yshad" => param.trim().parse::<f32>().ok().map(AnimatedTag::Yshad),
        "fax" => param.trim().parse::<f32>().ok().map(AnimatedTag::Fax),
        "fay" => param.trim().parse::<f32>().ok().map(AnimatedTag::Fay),
        "fscx" => param.trim().parse::<f32>().ok().map(AnimatedTag::Fscx),
        "fscy" => param.trim().parse::<f32>().ok().map(AnimatedTag::Fscy),
        "fs" => param.trim().parse::<f32>().ok().map(AnimatedTag::Fs),
        "fsp" => param.trim().parse::<f32>().ok().map(AnimatedTag::Fsp),
        "q" => {
            // `\q<mode>` — 0/1/2/3 per spec. Values outside that
            // range are skipped (the renderer falls back to the
            // script's WrapStyle header).
            let n: i32 = param.trim().parse().ok()?;
            if (0..=3).contains(&n) {
                Some(AnimatedTag::Q(n as u8))
            } else {
                None
            }
        }
        "an" => {
            // `\an<pos>` — numpad alignment 1..=9. Other values are
            // dropped (the renderer falls back to the style's
            // Alignment field).
            let n: i32 = param.trim().parse().ok()?;
            if (1..=9).contains(&n) {
                Some(AnimatedTag::An(n as u8))
            } else {
                None
            }
        }
        "a" => {
            // `\a<pos>` — legacy SubStation-Alpha alignment code. We
            // store the original code unchanged; the evaluator does
            // the numpad conversion (so the typed tag is still useful
            // for callers that want to inspect "was the legacy form
            // used?"). Negative values can never match a legacy slot
            // so they're rejected up front.
            let n: i32 = param.trim().parse().ok()?;
            if (0..=255).contains(&n) {
                Some(AnimatedTag::A(n as u8))
            } else {
                None
            }
        }
        "k" | "kf" | "ko" => {
            // `\k` family — per-syllable karaoke duration in
            // centiseconds. `\K` (uppercase) lowercases to `k` here, so
            // resolve the kind from the original-cased name: lowercase
            // `\k` = instant fill, `\K` = sweep (identical to `\kf`).
            // Negative durations clamp to 0. `\kt` is deliberately not
            // handled (Aegisub: "rarely useful … not documented").
            let cs = param.trim().parse::<f32>().ok()?;
            let cs = cs.max(0.0).round() as u32;
            let kind = match name_lc {
                "kf" => KaraokeKind::Sweep,
                "ko" => KaraokeKind::Outline,
                // bare "k": uppercase `\K` is the sweep variant.
                _ if name_orig == "K" => KaraokeKind::Sweep,
                _ => KaraokeKind::Fill,
            };
            Some(AnimatedTag::Karaoke { kind, cs })
        }
        "c" | "1c" => parse_color_rgb(param).map(AnimatedTag::Color1),
        "2c" => parse_color_rgb(param).map(AnimatedTag::Color2),
        "3c" => parse_color_rgb(param).map(AnimatedTag::Color3),
        "4c" => parse_color_rgb(param).map(AnimatedTag::Color4),
        "alpha" => parse_alpha_byte(param).map(AnimatedTag::Alpha),
        "1a" => parse_alpha_byte(param).map(AnimatedTag::Alpha1),
        "2a" => parse_alpha_byte(param).map(AnimatedTag::Alpha2),
        "3a" => parse_alpha_byte(param).map(AnimatedTag::Alpha3),
        "4a" => parse_alpha_byte(param).map(AnimatedTag::Alpha4),
        "clip" => parse_clip(param, false),
        "iclip" => parse_clip(param, true),
        "fe" => {
            // `\fe<id>` — Win32 charset ID. Spec range 0..=255; the
            // doc lists `0`/`1`/`2`/`128`/`129`/`130`/`134`/`136`/
            // `162`/`163`/`177`/`178` as the common slots.
            let n: i32 = param.trim().parse().ok()?;
            if (0..=255).contains(&n) {
                Some(AnimatedTag::Fe(n as u8))
            } else {
                None
            }
        }
        "b" => {
            // `\b<weight>` — bold weight. Per Aegisub spec, valid
            // weights are 100..900 in steps of 100, with the legacy
            // `\b1` / `\b0` shortcuts mapping to "bold" / "not bold".
            // Negative or oversized values are dropped. Empty
            // parameters drop the tag (the renderer keeps the
            // style's Bold field).
            let raw = param.trim();
            if raw.is_empty() {
                return None;
            }
            let n: i32 = raw.parse().ok()?;
            let weight = match n {
                0 => 0,
                1 => 700,
                w if (100..=900).contains(&w) => w as u16,
                _ => return None,
            };
            Some(AnimatedTag::B(weight))
        }
        "r" => {
            // `\r[<style>]` — reset all style overrides; the optional
            // name argument switches the line's base style to a named
            // definition. Per spec, the bare form (`\r`) drops back
            // to the line's own style; the named form (`\rAlternate`)
            // switches to the style called "Alternate" in the script
            // `[V4+ Styles]` block. The renderer is responsible for
            // looking the name up; the typed surface carries the
            // raw text.
            let name = param.trim();
            if name.is_empty() {
                Some(AnimatedTag::R(None))
            } else {
                Some(AnimatedTag::R(Some(name.to_string())))
            }
        }
        "t" => parse_t(param),
        _ => None,
    }
}

/// Parse an ASS alpha byte: `&HFF&` (preferred), `&HFF`, `H80`, `0xFF`,
/// or a bare hex string. Returns `0..=255`.
///
/// ASS only ever specifies alpha as hexadecimal (per Aegisub spec:
/// "in <a href='hexadecimal'>hexadecimal</a> ... `\1a&HFF&`"). Any
/// `&H` / `H` / `0x` prefix and `&` envelope are tolerated; the
/// underlying value is always parsed base-16.
fn parse_alpha_byte(s: &str) -> Option<u8> {
    let mut t = s.trim();
    t = t.trim_matches('&');
    t = t.trim_start_matches(['H', 'h']);
    t = t.trim_start_matches("0x");
    t = t.trim_matches('&').trim();
    if t.is_empty() {
        return None;
    }
    let v = u32::from_str_radix(t, 16).ok()?;
    Some(v.clamp(0, 255) as u8)
}

fn parse_int_list(s: &str) -> Vec<i32> {
    s.split(',')
        .map(|p| p.trim().parse::<i32>().ok())
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

fn parse_float_list(s: &str) -> Vec<f32> {
    s.split(',')
        .map(|p| p.trim().parse::<f32>().ok())
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

fn parse_color_rgb(s: &str) -> Option<(u8, u8, u8)> {
    // Reuse the same scheme as the main parser: `&Hbbggrr&`.
    let s = s.trim().trim_matches('&');
    let s = s.trim_start_matches(['H', 'h']);
    let s = s.trim_start_matches("0x");
    let s = s.trim_end_matches('&').trim();
    if s.is_empty() {
        return None;
    }
    let v: u32 = u32::from_str_radix(s, 16).ok()?;
    let b = ((v >> 16) & 0xFF) as u8;
    let g = ((v >> 8) & 0xFF) as u8;
    let r = (v & 0xFF) as u8;
    Some((r, g, b))
}

fn parse_clip(param: &str, inverse: bool) -> Option<AnimatedTag> {
    // `\clip(x1, y1, x2, y2)` rectangle (4 numeric args) or
    // `\clip([scale,] drawing)` path. `\iclip(...)` is the inverse
    // form: visible *outside* the rectangle / path.
    let parts: Vec<&str> = param.split(',').map(|s| s.trim()).collect();
    if parts.len() == 4 {
        let n: Vec<Option<f32>> = parts.iter().map(|p| p.parse::<f32>().ok()).collect();
        if n.iter().all(|x| x.is_some()) {
            let n: Vec<f32> = n.into_iter().map(|x| x.unwrap()).collect();
            return Some(if inverse {
                AnimatedTag::IClipRect {
                    x1: n[0],
                    y1: n[1],
                    x2: n[2],
                    y2: n[3],
                }
            } else {
                AnimatedTag::ClipRect {
                    x1: n[0],
                    y1: n[1],
                    x2: n[2],
                    y2: n[3],
                }
            });
        }
    }
    Some(if inverse {
        AnimatedTag::IClipDrawing(param.to_string())
    } else {
        AnimatedTag::ClipDrawing(param.to_string())
    })
}

fn parse_t(param: &str) -> Option<AnimatedTag> {
    // Possible shapes:
    //   \t(tags)
    //   \t(accel, tags)
    //   \t(t1, t2, tags)
    //   \t(t1, t2, accel, tags)
    // The "tags" segment may contain commas (e.g. `\clip(..)`), so we
    // can't naively split on `,`. Strategy: numeric-prefix parsing —
    // peel off leading numbers, then everything else is the tags
    // string.
    let (nums, tags_str) = peel_leading_numbers(param);
    let mut inner: Vec<AnimatedTag> = Vec::new();
    parse_overrides(tags_str, &mut inner);
    let (t1, t2, accel) = match nums.len() {
        0 => (None, None, 1.0_f32),
        1 => (None, None, nums[0]),
        2 => (Some(nums[0] as i32), Some(nums[1] as i32), 1.0),
        _ => (Some(nums[0] as i32), Some(nums[1] as i32), nums[2]),
    };
    Some(AnimatedTag::T {
        t1_ms: t1,
        t2_ms: t2,
        accel,
        inner,
    })
}

/// Peel leading comma-separated decimal numbers off `s`. Returns the
/// numbers and the remainder (with the leading comma stripped).
///
/// Stops at the first comma-separated token that doesn't parse as a
/// float, OR at a `\` (start of an inner tag), whichever comes first.
fn peel_leading_numbers(s: &str) -> (Vec<f32>, &str) {
    let mut nums = Vec::new();
    let mut cursor = s.trim_start();
    loop {
        // Find next `,` or `\` boundary.
        let bytes = cursor.as_bytes();
        let mut k = 0;
        while k < bytes.len() && bytes[k] != b',' && bytes[k] != b'\\' {
            k += 1;
        }
        let head = cursor[..k].trim();
        // If the head starts a tag (`\`), we're done with numbers.
        if head.is_empty() {
            // Empty leading token (e.g. starts with `,`) → done.
            if k == 0 {
                break;
            }
        }
        match head.parse::<f32>() {
            Ok(n) => {
                nums.push(n);
                if k >= bytes.len() {
                    cursor = "";
                    break;
                }
                if bytes[k] == b'\\' {
                    cursor = &cursor[k..];
                    break;
                }
                // bytes[k] == b','
                cursor = &cursor[k + 1..];
                cursor = cursor.trim_start();
            }
            Err(_) => break,
        }
    }
    (nums, cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_block(s: &str) -> Vec<AnimatedTag> {
        let mut out = Vec::new();
        parse_overrides(s, &mut out);
        out
    }

    #[test]
    fn parses_fad() {
        let v = parse_block(r"\fad(200,300)");
        assert_eq!(
            v,
            vec![AnimatedTag::Fad {
                t1_ms: 200,
                t2_ms: 300,
            }]
        );
    }

    #[test]
    fn parses_fade7() {
        let v = parse_block(r"\fade(255,0,255,0,500,1500,2000)");
        assert_eq!(
            v,
            vec![AnimatedTag::Fade {
                a1: 255,
                a2: 0,
                a3: 255,
                t1_ms: 0,
                t2_ms: 500,
                t3_ms: 1500,
                t4_ms: 2000,
            }]
        );
    }

    #[test]
    fn parses_move4_and_move6() {
        let v = parse_block(r"\move(10,20,100,200)");
        assert_eq!(v.len(), 1);
        match &v[0] {
            AnimatedTag::Move {
                x1,
                y1,
                x2,
                y2,
                t1_ms,
                t2_ms,
            } => {
                assert_eq!(*x1, 10.0);
                assert_eq!(*y1, 20.0);
                assert_eq!(*x2, 100.0);
                assert_eq!(*y2, 200.0);
                assert!(t1_ms.is_none());
                assert!(t2_ms.is_none());
            }
            _ => panic!(),
        }

        let v = parse_block(r"\move(10,20,100,200,500,1500)");
        match &v[0] {
            AnimatedTag::Move { t1_ms, t2_ms, .. } => {
                assert_eq!(*t1_ms, Some(500));
                assert_eq!(*t2_ms, Some(1500));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parses_frz_blur_fscx_fscy() {
        let v = parse_block(r"\frz45\blur2.5\fscx150\fscy75");
        assert_eq!(v.len(), 4);
        assert!(matches!(v[0], AnimatedTag::Frz(45.0)));
        assert!(matches!(v[1], AnimatedTag::Blur(b) if (b - 2.5).abs() < 1e-6));
        assert!(matches!(v[2], AnimatedTag::Fscx(150.0)));
        assert!(matches!(v[3], AnimatedTag::Fscy(75.0)));
    }

    #[test]
    fn parses_frx_fry() {
        // Per the Aegisub override-tag reference \frx / \fry are the X-
        // and Y-axis members of the rotation family; they parse with the
        // same numeric grammar as \frz. Negative angles are tolerated
        // ("rotate ... in opposite direction").
        let v = parse_block(r"\frx30\fry-45");
        assert_eq!(v.len(), 2);
        assert!(matches!(v[0], AnimatedTag::Frx(30.0)));
        assert!(matches!(v[1], AnimatedTag::Fry(-45.0)));
    }

    #[test]
    fn parses_t_with_frx_fry_inner() {
        // \frx / \fry must be recognised inside a \t(...) envelope so
        // the evaluator can lerp them like \frz.
        let v = parse_block(r"\t(0,1000,\frx90\fry-90)");
        assert_eq!(v.len(), 1);
        match &v[0] {
            AnimatedTag::T {
                t1_ms,
                t2_ms,
                inner,
                ..
            } => {
                assert_eq!(*t1_ms, Some(0));
                assert_eq!(*t2_ms, Some(1000));
                assert_eq!(inner.len(), 2);
                assert!(matches!(inner[0], AnimatedTag::Frx(90.0)));
                assert!(matches!(inner[1], AnimatedTag::Fry(-90.0)));
            }
            _ => panic!("expected T tag, got {:?}", v[0]),
        }
    }

    #[test]
    fn parses_clip_rect() {
        let v = parse_block(r"\clip(10,20,100,200)");
        assert_eq!(
            v,
            vec![AnimatedTag::ClipRect {
                x1: 10.0,
                y1: 20.0,
                x2: 100.0,
                y2: 200.0,
            }]
        );
    }

    #[test]
    fn parses_clip_drawing_passthrough() {
        let v = parse_block(r"\clip(m 0 0 l 100 0 l 100 100 l 0 100)");
        assert_eq!(v.len(), 1);
        assert!(matches!(v[0], AnimatedTag::ClipDrawing(_)));
    }

    #[test]
    fn parses_t_full() {
        let v = parse_block(r"\t(0,1000,1.5,\fscx200\frz90)");
        assert_eq!(v.len(), 1);
        match &v[0] {
            AnimatedTag::T {
                t1_ms,
                t2_ms,
                accel,
                inner,
            } => {
                assert_eq!(*t1_ms, Some(0));
                assert_eq!(*t2_ms, Some(1000));
                assert!((accel - 1.5).abs() < 1e-6);
                assert_eq!(inner.len(), 2);
                assert!(matches!(inner[0], AnimatedTag::Fscx(200.0)));
                assert!(matches!(inner[1], AnimatedTag::Frz(90.0)));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parses_t_no_times() {
        let v = parse_block(r"\t(\frz360)");
        match &v[0] {
            AnimatedTag::T {
                t1_ms,
                t2_ms,
                accel,
                inner,
            } => {
                assert!(t1_ms.is_none());
                assert!(t2_ms.is_none());
                assert!((accel - 1.0).abs() < 1e-6);
                assert_eq!(inner.len(), 1);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parses_t_two_times_no_accel() {
        let v = parse_block(r"\t(0,500,\frz45)");
        match &v[0] {
            AnimatedTag::T {
                t1_ms,
                t2_ms,
                accel,
                inner,
            } => {
                assert_eq!(*t1_ms, Some(0));
                assert_eq!(*t2_ms, Some(500));
                assert!((accel - 1.0).abs() < 1e-6);
                assert_eq!(inner.len(), 1);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parses_color() {
        let v = parse_block(r"\c&H0000FF&");
        assert_eq!(v, vec![AnimatedTag::Color1((255, 0, 0))]);
        let v = parse_block(r"\1c&HFF00FF&");
        assert_eq!(v, vec![AnimatedTag::Color1((255, 0, 255))]);
    }

    #[test]
    fn fad_alpha_curve() {
        // Cue 0..2000 ms, fade in 200, fade out 300.
        let dur = 2000;
        assert!((fad_alpha(200, 300, 0, dur) - 0.0).abs() < 1e-6);
        assert!((fad_alpha(200, 300, 100, dur) - 0.5).abs() < 1e-6);
        assert!((fad_alpha(200, 300, 200, dur) - 1.0).abs() < 1e-6);
        assert!((fad_alpha(200, 300, 1000, dur) - 1.0).abs() < 1e-6);
        assert!((fad_alpha(200, 300, 1700, dur) - 1.0).abs() < 1e-6);
        // Halfway through fade-out: dur-t = 150, t2 = 300 → 0.5
        assert!((fad_alpha(200, 300, 1850, dur) - 0.5).abs() < 1e-6);
        assert!((fad_alpha(200, 300, 2000, dur) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn evaluate_static_overrides() {
        let cue_anim = CueAnimation {
            tags: vec![
                AnimatedTag::Fscx(200.0),
                AnimatedTag::Fscy(50.0),
                AnimatedTag::Frz(90.0),
                AnimatedTag::Blur(3.0),
            ],
        };
        let st = cue_anim.evaluate_at(500, 1000);
        assert_eq!(st.scale, (2.0, 0.5));
        assert!((st.rotate_radians - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert_eq!(st.blur_sigma, 3.0);
    }

    #[test]
    fn evaluate_move() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Move {
                x1: 0.0,
                y1: 0.0,
                x2: 100.0,
                y2: 200.0,
                t1_ms: Some(0),
                t2_ms: Some(1000),
            }],
        };
        let st0 = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st0.translate, Some((0.0, 0.0)));
        let st_mid = cue_anim.evaluate_at(500, 1000);
        assert_eq!(st_mid.translate, Some((50.0, 100.0)));
        let st_end = cue_anim.evaluate_at(1000, 1000);
        assert_eq!(st_end.translate, Some((100.0, 200.0)));
        // Past end clamps.
        let st_after = cue_anim.evaluate_at(2000, 1000);
        assert_eq!(st_after.translate, Some((100.0, 200.0)));
    }

    #[test]
    fn evaluate_move_default_times() {
        // No t1/t2 given → animate over the whole cue.
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Move {
                x1: 0.0,
                y1: 0.0,
                x2: 100.0,
                y2: 100.0,
                t1_ms: None,
                t2_ms: None,
            }],
        };
        let st = cue_anim.evaluate_at(500, 1000);
        assert_eq!(st.translate, Some((50.0, 50.0)));
    }

    #[test]
    fn parses_pos() {
        let v = parse_block(r"\pos(320,240)");
        assert_eq!(v, vec![AnimatedTag::Pos { x: 320.0, y: 240.0 }]);
        // Decimals tolerated even though the spec asks for integers.
        let v = parse_block(r"\pos(12.5,-3.0)");
        assert_eq!(v, vec![AnimatedTag::Pos { x: 12.5, y: -3.0 }]);
        // Wrong arity → dropped (round-trip text path still keeps it raw).
        assert!(parse_block(r"\pos(320)").is_empty());
        assert!(parse_block(r"\pos(1,2,3)").is_empty());
    }

    #[test]
    fn evaluate_pos_is_static() {
        // \pos sets a constant position the renderer can anchor to; it
        // does not vary with time.
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Pos { x: 320.0, y: 240.0 }],
        };
        assert_eq!(
            cue_anim.evaluate_at(0, 1000).translate,
            Some((320.0, 240.0))
        );
        assert_eq!(
            cue_anim.evaluate_at(500, 1000).translate,
            Some((320.0, 240.0))
        );
        assert_eq!(
            cue_anim.evaluate_at(1000, 1000).translate,
            Some((320.0, 240.0))
        );
    }

    #[test]
    fn move_after_pos_overrides() {
        // \move and \pos both target the line position; the later tag
        // wins (last-writer-wins, matching the rest of the module).
        let cue_anim = CueAnimation {
            tags: vec![
                AnimatedTag::Pos { x: 10.0, y: 10.0 },
                AnimatedTag::Move {
                    x1: 0.0,
                    y1: 0.0,
                    x2: 100.0,
                    y2: 100.0,
                    t1_ms: Some(0),
                    t2_ms: Some(1000),
                },
            ],
        };
        // The \move drives translate, not the earlier \pos.
        assert_eq!(
            cue_anim.evaluate_at(500, 1000).translate,
            Some((50.0, 50.0))
        );
    }

    #[test]
    fn evaluate_fad() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Fad {
                t1_ms: 200,
                t2_ms: 300,
            }],
        };
        let dur = 2000;
        assert!((cue_anim.evaluate_at(0, dur).alpha_mul - 0.0).abs() < 1e-6);
        assert!((cue_anim.evaluate_at(100, dur).alpha_mul - 0.5).abs() < 1e-6);
        assert!((cue_anim.evaluate_at(1000, dur).alpha_mul - 1.0).abs() < 1e-6);
        assert!((cue_anim.evaluate_at(1850, dur).alpha_mul - 0.5).abs() < 1e-6);
    }

    #[test]
    fn evaluate_t_interpolates_scale() {
        // Initial fscx is implicit 100% (=1.0 scale). \t over [0,1000]
        // ramps to 200% (=2.0 scale).
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::T {
                t1_ms: Some(0),
                t2_ms: Some(1000),
                accel: 1.0,
                inner: vec![AnimatedTag::Fscx(200.0)],
            }],
        };
        assert_eq!(cue_anim.evaluate_at(0, 1000).scale.0, 1.0);
        assert!((cue_anim.evaluate_at(500, 1000).scale.0 - 1.5).abs() < 1e-6);
        assert_eq!(cue_anim.evaluate_at(1000, 1000).scale.0, 2.0);
        assert_eq!(cue_anim.evaluate_at(1500, 1000).scale.0, 2.0);
    }

    #[test]
    fn evaluate_t_interpolates_rotate() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::T {
                t1_ms: Some(0),
                t2_ms: Some(1000),
                accel: 1.0,
                inner: vec![AnimatedTag::Frz(90.0)],
            }],
        };
        let st_mid = cue_anim.evaluate_at(500, 1000);
        // 45 degrees in radians.
        assert!((st_mid.rotate_radians - std::f32::consts::FRAC_PI_4).abs() < 1e-5);
    }

    #[test]
    fn evaluate_t_interpolates_color() {
        let cue_anim = CueAnimation {
            tags: vec![
                AnimatedTag::Color1((255, 0, 0)), // start red
                AnimatedTag::T {
                    t1_ms: Some(0),
                    t2_ms: Some(1000),
                    accel: 1.0,
                    inner: vec![AnimatedTag::Color1((0, 0, 255))], // blue
                },
            ],
        };
        let st = cue_anim.evaluate_at(500, 1000);
        let rgb = st.primary_color.unwrap();
        // Halfway → roughly (127, 0, 127).
        assert!((rgb.0 as i32 - 127).abs() <= 1);
        assert_eq!(rgb.1, 0);
        assert!((rgb.2 as i32 - 127).abs() <= 1);
    }

    #[test]
    fn evaluate_t_no_times_uses_cue_span() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::T {
                t1_ms: None,
                t2_ms: None,
                accel: 1.0,
                inner: vec![AnimatedTag::Fscy(200.0)],
            }],
        };
        // Halfway through a 2000ms cue: scale.1 should be 1.5.
        let st = cue_anim.evaluate_at(1000, 2000);
        assert!((st.scale.1 - 1.5).abs() < 1e-6);
    }

    #[test]
    fn clip_rect_applies() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::ClipRect {
                x1: 10.0,
                y1: 20.0,
                x2: 100.0,
                y2: 200.0,
            }],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        let c = st.clip_rect.unwrap();
        assert_eq!((c.x1, c.y1, c.x2, c.y2), (10.0, 20.0, 100.0, 200.0));
    }

    #[test]
    fn clip_rect_normalises_swapped_corners() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::ClipRect {
                x1: 100.0,
                y1: 200.0,
                x2: 10.0,
                y2: 20.0,
            }],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        let c = st.clip_rect.unwrap();
        assert_eq!((c.x1, c.y1, c.x2, c.y2), (10.0, 20.0, 100.0, 200.0));
    }

    #[test]
    fn extract_from_cue_segments() {
        // Build a fake cue using the parser's output shape.
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 1_000_000,
            style_ref: None,
            positioning: None,
            segments: vec![
                Segment::Raw(r"{\fad(100,200)\frz30}".into()),
                Segment::Text("hello".into()),
                Segment::Raw(r"{\move(0,0,100,100)}".into()),
            ],
        };
        let anim = extract_cue_animation(&cue);
        assert_eq!(anim.tags.len(), 3);
        assert!(matches!(
            anim.tags[0],
            AnimatedTag::Fad {
                t1_ms: 100,
                t2_ms: 200
            }
        ));
        assert!(matches!(anim.tags[1], AnimatedTag::Frz(30.0)));
        assert!(matches!(anim.tags[2], AnimatedTag::Move { .. }));
    }

    #[test]
    fn extract_skips_non_animated_raw() {
        // Unknown tag like `\xyz` should not yield anything.
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 1_000_000,
            style_ref: None,
            positioning: None,
            segments: vec![Segment::Raw(r"{\xyz(1,2)}".into())],
        };
        let anim = extract_cue_animation(&cue);
        assert!(anim.is_empty());
    }

    #[test]
    fn extract_recurses_into_color_children() {
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 0,
            style_ref: None,
            positioning: None,
            segments: vec![Segment::Color {
                rgb: (1, 2, 3),
                children: vec![Segment::Raw(r"{\fad(50,50)}".into())],
            }],
        };
        let anim = extract_cue_animation(&cue);
        assert_eq!(anim.tags.len(), 1);
        assert!(matches!(
            anim.tags[0],
            AnimatedTag::Fad {
                t1_ms: 50,
                t2_ms: 50
            }
        ));
    }

    #[test]
    fn transform_composition_includes_translate() {
        let cue_anim = CueAnimation {
            tags: vec![
                AnimatedTag::Move {
                    x1: 100.0,
                    y1: 200.0,
                    x2: 100.0,
                    y2: 200.0,
                    t1_ms: None,
                    t2_ms: None,
                },
                AnimatedTag::Fscx(200.0),
            ],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        // Apply transform to origin → should land at (100, 200).
        let p = st.transform.apply(oxideav_core::Point { x: 0.0, y: 0.0 });
        assert!((p.x - 100.0).abs() < 1e-5);
        assert!((p.y - 200.0).abs() < 1e-5);
        // Apply transform to (1, 0) → scale 2x in x then translate.
        let p1 = st.transform.apply(oxideav_core::Point { x: 1.0, y: 0.0 });
        assert!((p1.x - 102.0).abs() < 1e-5);
        assert!((p1.y - 200.0).abs() < 1e-5);
    }

    // -----------------------------------------------------------------
    // r76 typed tag coverage: \bord/\xbord/\ybord, \shad/\xshad/\yshad,
    // \be (distinct from \blur), \fax/\fay, \iclip.

    #[test]
    fn parses_bord_uniform() {
        let v = parse_block(r"\bord3.5");
        assert_eq!(v, vec![AnimatedTag::Bord(3.5)]);
    }

    #[test]
    fn parses_xbord_ybord_pair() {
        let v = parse_block(r"\xbord2\ybord4");
        assert_eq!(v, vec![AnimatedTag::Xbord(2.0), AnimatedTag::Ybord(4.0)]);
    }

    #[test]
    fn parses_shad_uniform_and_per_axis() {
        let v = parse_block(r"\shad5\xshad-2.5\yshad3");
        assert_eq!(
            v,
            vec![
                AnimatedTag::Shad(5.0),
                AnimatedTag::Xshad(-2.5),
                AnimatedTag::Yshad(3.0),
            ]
        );
    }

    #[test]
    fn parses_blur_and_be_are_separate_variants() {
        // Per Aegisub spec these are different filters; the old impl
        // collapsed both into Blur, which lost \be vs \blur fidelity.
        let v = parse_block(r"\blur2.5\be3");
        assert_eq!(v.len(), 2);
        assert!(matches!(v[0], AnimatedTag::Blur(b) if (b - 2.5).abs() < 1e-6));
        assert!(matches!(v[1], AnimatedTag::Be(3)));
    }

    #[test]
    fn be_rounds_non_integer_strengths() {
        // Spec says integer; tolerate floats from the wild.
        let v = parse_block(r"\be2.7");
        assert!(matches!(v[0], AnimatedTag::Be(3)));
    }

    #[test]
    fn parses_fax_fay() {
        let v = parse_block(r"\fax0.5\fay-0.25");
        assert_eq!(v, vec![AnimatedTag::Fax(0.5), AnimatedTag::Fay(-0.25)]);
    }

    #[test]
    fn parses_iclip_rect() {
        let v = parse_block(r"\iclip(10,20,100,200)");
        assert_eq!(
            v,
            vec![AnimatedTag::IClipRect {
                x1: 10.0,
                y1: 20.0,
                x2: 100.0,
                y2: 200.0,
            }]
        );
    }

    #[test]
    fn parses_iclip_drawing_passthrough() {
        let v = parse_block(r"\iclip(m 0 0 l 100 0 l 100 100 l 0 100)");
        assert_eq!(v.len(), 1);
        assert!(matches!(v[0], AnimatedTag::IClipDrawing(_)));
    }

    #[test]
    fn parses_iclip_with_scale_prefix_is_drawing_form() {
        // `\iclip(scale, drawing)` — two-arg form is the scaled drawing
        // variant, NOT a rect (rect requires exactly 4 numeric args).
        let v = parse_block(r"\iclip(2,m 0 0 l 50 50)");
        assert!(matches!(v[0], AnimatedTag::IClipDrawing(_)));
    }

    #[test]
    fn evaluate_bord_sets_both_axes() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Bord(2.5)],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st.border, Some((2.5, 2.5)));
    }

    #[test]
    fn evaluate_xbord_then_ybord_combines() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Xbord(2.0), AnimatedTag::Ybord(4.0)],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st.border, Some((2.0, 4.0)));
    }

    #[test]
    fn evaluate_bord_after_xbord_ybord_overrides_both() {
        // Spec: "if you use \bord after \xbord or \ybord on a line, it
        // will [override them]".
        let cue_anim = CueAnimation {
            tags: vec![
                AnimatedTag::Xbord(2.0),
                AnimatedTag::Ybord(4.0),
                AnimatedTag::Bord(1.0),
            ],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st.border, Some((1.0, 1.0)));
    }

    #[test]
    fn evaluate_bord_clamps_negative_to_zero() {
        // Spec: "Border width cannot be negative."
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Bord(-3.0)],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st.border, Some((0.0, 0.0)));
    }

    #[test]
    fn evaluate_shad_uniform_and_xshad_yshad_negative() {
        // \shad uniform must be non-negative per spec; \xshad/\yshad
        // may be negative (shadow above/left of text).
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Shad(2.0)],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st.shadow, Some((2.0, 2.0)));

        let cue_anim2 = CueAnimation {
            tags: vec![AnimatedTag::Xshad(-3.5), AnimatedTag::Yshad(1.5)],
        };
        let st2 = cue_anim2.evaluate_at(0, 1000);
        assert_eq!(st2.shadow, Some((-3.5, 1.5)));

        // \shad must be clamped to >= 0 (spec).
        let cue_anim3 = CueAnimation {
            tags: vec![AnimatedTag::Shad(-2.0)],
        };
        let st3 = cue_anim3.evaluate_at(0, 1000);
        assert_eq!(st3.shadow, Some((0.0, 0.0)));
    }

    #[test]
    fn evaluate_be_strength() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Be(5)],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st.be_strength, 5);
        // And \be does NOT touch blur_sigma (which is \blur).
        assert_eq!(st.blur_sigma, 0.0);
    }

    #[test]
    fn evaluate_fax_fay_writes_shear() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Fax(0.5), AnimatedTag::Fay(-0.3)],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert!((st.shear.0 - 0.5).abs() < 1e-6);
        assert!((st.shear.1 + 0.3).abs() < 1e-6);
    }

    #[test]
    fn evaluate_iclip_rect_normalises() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::IClipRect {
                x1: 100.0,
                y1: 200.0,
                x2: 10.0,
                y2: 20.0,
            }],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        let c = st.iclip_rect.unwrap();
        assert_eq!((c.x1, c.y1, c.x2, c.y2), (10.0, 20.0, 100.0, 200.0));
        // \iclip and \clip are mutually exclusive in the cue but
        // independent fields on RenderState; only iclip_rect is set.
        assert!(st.clip_rect.is_none());
    }

    #[test]
    fn evaluate_iclip_drawing_stored() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::IClipDrawing("m 0 0 l 10 10".into())],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st.iclip_drawing.as_deref(), Some("m 0 0 l 10 10"));
        assert!(st.clip_drawing.is_none());
    }

    #[test]
    fn t_interpolates_bord() {
        // \bord(0) at t=0, ramps to \bord(4) at t=1000.
        let cue_anim = CueAnimation {
            tags: vec![
                AnimatedTag::Bord(0.0),
                AnimatedTag::T {
                    t1_ms: Some(0),
                    t2_ms: Some(1000),
                    accel: 1.0,
                    inner: vec![AnimatedTag::Bord(4.0)],
                },
            ],
        };
        let st_mid = cue_anim.evaluate_at(500, 1000);
        let (bx, by) = st_mid.border.unwrap();
        assert!((bx - 2.0).abs() < 1e-5, "bx = {}", bx);
        assert!((by - 2.0).abs() < 1e-5);
        let st_end = cue_anim.evaluate_at(1000, 1000);
        let (bx2, by2) = st_end.border.unwrap();
        assert!((bx2 - 4.0).abs() < 1e-5);
        assert!((by2 - 4.0).abs() < 1e-5);
    }

    #[test]
    fn t_interpolates_shad_per_axis() {
        let cue_anim = CueAnimation {
            tags: vec![
                AnimatedTag::Xshad(0.0),
                AnimatedTag::Yshad(0.0),
                AnimatedTag::T {
                    t1_ms: Some(0),
                    t2_ms: Some(1000),
                    accel: 1.0,
                    inner: vec![AnimatedTag::Xshad(6.0), AnimatedTag::Yshad(-2.0)],
                },
            ],
        };
        let st = cue_anim.evaluate_at(500, 1000);
        let (sx, sy) = st.shadow.unwrap();
        assert!((sx - 3.0).abs() < 1e-5);
        assert!((sy + 1.0).abs() < 1e-5);
    }

    #[test]
    fn t_interpolates_fax_fay() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::T {
                t1_ms: Some(0),
                t2_ms: Some(1000),
                accel: 1.0,
                inner: vec![AnimatedTag::Fax(1.0)],
            }],
        };
        let st = cue_anim.evaluate_at(500, 1000);
        // Starting shear is (0, 0); halfway to (1, 0) → 0.5.
        assert!((st.shear.0 - 0.5).abs() < 1e-5);
    }

    #[test]
    fn t_interpolates_be_rounds_to_integer() {
        let cue_anim = CueAnimation {
            tags: vec![
                AnimatedTag::Be(0),
                AnimatedTag::T {
                    t1_ms: Some(0),
                    t2_ms: Some(1000),
                    accel: 1.0,
                    inner: vec![AnimatedTag::Be(10)],
                },
            ],
        };
        let st = cue_anim.evaluate_at(500, 1000);
        // Halfway: 5 (rounded).
        assert_eq!(st.be_strength, 5);
        let st_q = cue_anim.evaluate_at(250, 1000);
        // Quarter: 2.5 → rounds to 3 (round-half-to-even per f32 round).
        assert!(st_q.be_strength == 2 || st_q.be_strength == 3);
    }

    #[test]
    fn extract_typed_tags_from_real_world_cue() {
        // A composite cue that exercises every new typed tag in a single
        // Dialogue line — representative of dense typesetting subs.
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 5_000_000,
            style_ref: None,
            positioning: None,
            segments: vec![
                Segment::Raw(
                    r"{\bord2\xbord3\ybord4\shad1\xshad-2\yshad2\blur1.5\be2\fax0.1\fay-0.1\iclip(0,0,640,480)}"
                        .into(),
                ),
                Segment::Text("text".into()),
            ],
        };
        let anim = extract_cue_animation(&cue);
        assert_eq!(anim.tags.len(), 11, "got {:?}", anim.tags);
        let st = anim.evaluate_at(0, 5000);
        // Border: \bord(2) then xbord=3,ybord=4 overrides → (3,4).
        assert_eq!(st.border, Some((3.0, 4.0)));
        // Shadow: \shad(1) then xshad=-2, yshad=2 → (-2, 2).
        assert_eq!(st.shadow, Some((-2.0, 2.0)));
        assert!((st.blur_sigma - 1.5).abs() < 1e-6);
        assert_eq!(st.be_strength, 2);
        assert!((st.shear.0 - 0.1).abs() < 1e-6);
        assert!((st.shear.1 + 0.1).abs() < 1e-6);
        let c = st.iclip_rect.unwrap();
        assert_eq!((c.x1, c.y1, c.x2, c.y2), (0.0, 0.0, 640.0, 480.0));
    }

    // -----------------------------------------------------------------
    // r81 typed tag coverage: \2c / \3c / \4c per-component colours +
    // \alpha + \1a..\4a per-component alphas.

    #[test]
    fn parses_color2_color3_color4() {
        let v = parse_block(r"\2c&H0000FF&\3c&H00FF00&\4c&HFF0000&");
        assert_eq!(
            v,
            vec![
                AnimatedTag::Color2((255, 0, 0)),
                AnimatedTag::Color3((0, 255, 0)),
                AnimatedTag::Color4((0, 0, 255)),
            ]
        );
    }

    #[test]
    fn parses_alpha_all_and_per_component() {
        let v = parse_block(r"\alpha&H80&\1a&HFF&\2a&H00&\3a&H40&\4a&HC0&");
        assert_eq!(
            v,
            vec![
                AnimatedTag::Alpha(0x80),
                AnimatedTag::Alpha1(0xFF),
                AnimatedTag::Alpha2(0x00),
                AnimatedTag::Alpha3(0x40),
                AnimatedTag::Alpha4(0xC0),
            ]
        );
    }

    #[test]
    fn parses_alpha_tolerates_envelope_variants() {
        // All four shapes the wild emits should parse identically.
        assert_eq!(parse_alpha_byte("&HFF&"), Some(0xFF));
        assert_eq!(parse_alpha_byte("&HFF"), Some(0xFF));
        assert_eq!(parse_alpha_byte("HFF"), Some(0xFF));
        assert_eq!(parse_alpha_byte("0xFF"), Some(0xFF));
        assert_eq!(parse_alpha_byte("ff"), Some(0xFF));
        assert_eq!(parse_alpha_byte(""), None);
    }

    #[test]
    fn evaluate_color2_color3_color4_writes_separate_fields() {
        let cue_anim = CueAnimation {
            tags: vec![
                AnimatedTag::Color2((10, 20, 30)),
                AnimatedTag::Color3((40, 50, 60)),
                AnimatedTag::Color4((70, 80, 90)),
            ],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st.secondary_color, Some((10, 20, 30)));
        assert_eq!(st.outline_color, Some((40, 50, 60)));
        assert_eq!(st.shadow_color, Some((70, 80, 90)));
        // \2c / \3c / \4c must not pollute \1c.
        assert_eq!(st.primary_color, None);
    }

    #[test]
    fn evaluate_alpha_global_sets_all_four_channels() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Alpha(0x80)],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st.primary_alpha, Some(0x80));
        assert_eq!(st.secondary_alpha, Some(0x80));
        assert_eq!(st.outline_alpha, Some(0x80));
        assert_eq!(st.shadow_alpha, Some(0x80));
    }

    #[test]
    fn evaluate_per_component_alpha_overrides_global() {
        // \alpha sets all four, then \3a&HFF& makes border transparent.
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Alpha(0x40), AnimatedTag::Alpha3(0xFF)],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st.primary_alpha, Some(0x40));
        assert_eq!(st.secondary_alpha, Some(0x40));
        assert_eq!(st.outline_alpha, Some(0xFF));
        assert_eq!(st.shadow_alpha, Some(0x40));
    }

    #[test]
    fn alpha_per_component_does_not_touch_alpha_mul() {
        // \fad alpha_mul is the cue-level envelope; per-component
        // alphas (\1a..\4a) are independent overrides on top.
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Alpha1(0x80), AnimatedTag::Alpha3(0xC0)],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st.alpha_mul, 1.0);
        assert_eq!(st.primary_alpha, Some(0x80));
        assert_eq!(st.outline_alpha, Some(0xC0));
    }

    #[test]
    fn t_interpolates_color3() {
        // Border colour interpolation: red → blue over [0, 1000].
        let cue_anim = CueAnimation {
            tags: vec![
                AnimatedTag::Color3((255, 0, 0)),
                AnimatedTag::T {
                    t1_ms: Some(0),
                    t2_ms: Some(1000),
                    accel: 1.0,
                    inner: vec![AnimatedTag::Color3((0, 0, 255))],
                },
            ],
        };
        let st = cue_anim.evaluate_at(500, 1000);
        let rgb = st.outline_color.unwrap();
        assert!((rgb.0 as i32 - 127).abs() <= 1);
        assert_eq!(rgb.1, 0);
        assert!((rgb.2 as i32 - 127).abs() <= 1);
    }

    #[test]
    fn t_interpolates_alpha1() {
        // Primary alpha 0x00 → 0xFF over [0, 1000]. At t=500 ≈ 0x80.
        let cue_anim = CueAnimation {
            tags: vec![
                AnimatedTag::Alpha1(0x00),
                AnimatedTag::T {
                    t1_ms: Some(0),
                    t2_ms: Some(1000),
                    accel: 1.0,
                    inner: vec![AnimatedTag::Alpha1(0xFF)],
                },
            ],
        };
        let st = cue_anim.evaluate_at(500, 1000);
        let a = st.primary_alpha.unwrap();
        assert!((a as i32 - 0x80).abs() <= 1, "got {:#x}", a);
        // Endpoint sanity.
        let st_end = cue_anim.evaluate_at(1000, 1000);
        assert_eq!(st_end.primary_alpha, Some(0xFF));
    }

    #[test]
    fn t_interpolates_alpha_global_writes_all_four() {
        // \alpha:&H00& → &HFF& halfway gives 0x80 on every channel.
        let cue_anim = CueAnimation {
            tags: vec![
                AnimatedTag::Alpha(0x00),
                AnimatedTag::T {
                    t1_ms: Some(0),
                    t2_ms: Some(1000),
                    accel: 1.0,
                    inner: vec![AnimatedTag::Alpha(0xFF)],
                },
            ],
        };
        let st = cue_anim.evaluate_at(500, 1000);
        for ch in [
            st.primary_alpha,
            st.secondary_alpha,
            st.outline_alpha,
            st.shadow_alpha,
        ] {
            let a = ch.unwrap();
            assert!((a as i32 - 0x80).abs() <= 1);
        }
    }

    #[test]
    fn extract_full_alpha_and_color_cue() {
        // Composite real-world cue: per-axis colours + per-channel
        // alphas all in a single override block.
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 2_000_000,
            style_ref: None,
            positioning: None,
            segments: vec![
                Segment::Raw(
                    r"{\1c&H0000FF&\2c&H00FF00&\3c&HFF0000&\4c&H808080&\alpha&H80&\3a&HFF&}".into(),
                ),
                Segment::Text("text".into()),
            ],
        };
        let anim = extract_cue_animation(&cue);
        assert_eq!(anim.tags.len(), 6, "got {:?}", anim.tags);
        let st = anim.evaluate_at(0, 2000);
        assert_eq!(st.primary_color, Some((255, 0, 0)));
        assert_eq!(st.secondary_color, Some((0, 255, 0)));
        assert_eq!(st.outline_color, Some((0, 0, 255)));
        assert_eq!(st.shadow_color, Some((128, 128, 128)));
        // \alpha 0x80 → all four channels 0x80, then \3a&HFF& overrides
        // the border channel only.
        assert_eq!(st.primary_alpha, Some(0x80));
        assert_eq!(st.secondary_alpha, Some(0x80));
        assert_eq!(st.outline_alpha, Some(0xFF));
        assert_eq!(st.shadow_alpha, Some(0x80));
    }

    #[test]
    fn unrecognised_color_or_alpha_payload_is_skipped() {
        // Empty payload or junk yields no AnimatedTag (parser drops it).
        assert!(parse_block(r"\2c&Hgggggg&").is_empty());
        assert!(parse_block(r"\1a").is_empty());
        assert!(parse_block(r"\3c").is_empty());
    }

    #[test]
    fn capital_k_karaoke_tag_is_recognised_as_kf() {
        // Per Aegisub: "\K and \kf are identical". Our base parser
        // lowercases tag names before matching, so \K already routes
        // through the \k handler — this test pins that contract.
        use crate::parse;
        let src = "[Script Info]\n\
ScriptType: v4.00+\n\
\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, Alignment, MarginL, MarginR, MarginV, Outline, Shadow\n\
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,2,10,10,10,1,0\n\
\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,{\\K50}sweep{\\K30}done\n";
        let t = parse(src.as_bytes()).unwrap();
        let segs = &t.cues[0].segments;
        // Should contain two Karaoke segments (one per \K marker).
        let karaoke_count = segs
            .iter()
            .filter(|s| matches!(s, Segment::Karaoke { .. }))
            .count();
        assert_eq!(karaoke_count, 2, "got segs = {:?}", segs);
    }

    // ---------------------------------------------------------------
    // \fsp letter-spacing + \q wrap-style coverage (round 88).

    #[test]
    fn parses_fsp_static() {
        let v = parse_block(r"\fsp3");
        assert_eq!(v, vec![AnimatedTag::Fsp(3.0)]);
        // Negative + decimal both accepted per Aegisub spec.
        let v = parse_block(r"\fsp-1.5");
        assert_eq!(v, vec![AnimatedTag::Fsp(-1.5)]);
    }

    #[test]
    fn parses_q_in_range() {
        for mode in 0..=3 {
            let src = format!(r"\q{mode}");
            let v = parse_block(&src);
            assert_eq!(v, vec![AnimatedTag::Q(mode as u8)]);
        }
    }

    #[test]
    fn parses_q_out_of_range_dropped() {
        // SSA only defines wrap modes 0..=3; anything else is ignored
        // so the renderer keeps using the script header's WrapStyle.
        assert!(parse_block(r"\q4").is_empty());
        assert!(parse_block(r"\q-1").is_empty());
    }

    #[test]
    fn evaluate_fsp_static_override() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Fsp(2.5)],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st.letter_spacing, Some(2.5));
        // Default state has no override.
        assert!(RenderState::identity().letter_spacing.is_none());
    }

    #[test]
    fn evaluate_q_static_override() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::Q(2)],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st.wrap_style, Some(2));
        assert!(RenderState::identity().wrap_style.is_none());
    }

    #[test]
    fn fsp_animatable_via_t() {
        // \t(0,1000,\fsp4) — letter-spacing should ramp 0 → 4 over
        // the cue. Without a pre-state \fsp, the source defaults to
        // the post-state value (no interpolation source), matching how
        // \blur etc. behave today.
        let v = parse_block(r"\fsp0\t(0,1000,\fsp4)");
        assert_eq!(v.len(), 2);
        let cue_anim = CueAnimation { tags: v };
        let st0 = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st0.letter_spacing, Some(0.0));
        let st_mid = cue_anim.evaluate_at(500, 1000);
        let mid = st_mid.letter_spacing.expect("set");
        assert!(
            (mid - 2.0).abs() < 1e-3,
            "expected 2.0 at midpoint, got {mid}"
        );
        let st_end = cue_anim.evaluate_at(1000, 1000);
        assert_eq!(st_end.letter_spacing, Some(4.0));
    }

    #[test]
    fn q_static_inside_t_snaps_post() {
        // \q is not animatable; if the spec value appears inside \t
        // it should snap to the post value once t1 has elapsed.
        let v = parse_block(r"\q0\t(500,1000,\q2)");
        assert_eq!(v.len(), 2);
        let cue_anim = CueAnimation { tags: v };
        // Before the transition starts: pre-value.
        let st_before = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st_before.wrap_style, Some(0));
        // Once t > t1 (k > 0): post-value.
        let st_mid = cue_anim.evaluate_at(750, 1000);
        assert_eq!(st_mid.wrap_style, Some(2));
        let st_end = cue_anim.evaluate_at(1000, 1000);
        assert_eq!(st_end.wrap_style, Some(2));
    }

    #[test]
    fn extract_fsp_q_from_cue_segment() {
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 1_000_000,
            style_ref: None,
            positioning: None,
            segments: vec![
                Segment::Raw(r"{\fsp2\q1}".into()),
                Segment::Text("spaced".into()),
            ],
        };
        let anim = extract_cue_animation(&cue);
        assert_eq!(anim.tags.len(), 2);
        let st = anim.evaluate_at(0, 1000);
        assert_eq!(st.letter_spacing, Some(2.0));
        assert_eq!(st.wrap_style, Some(1));
    }

    #[test]
    fn parses_an_in_range() {
        // Aegisub numpad spec: 1=bl, 2=bc, 3=br, 4=ml, 5=mc, 6=mr,
        // 7=tl, 8=tc, 9=tr. All nine should parse to AnimatedTag::An.
        for pos in 1..=9 {
            let src = format!(r"\an{pos}");
            let v = parse_block(&src);
            assert_eq!(v, vec![AnimatedTag::An(pos as u8)]);
        }
    }

    #[test]
    fn parses_an_out_of_range_dropped() {
        // Only 1..=9 are valid numpad positions per the Aegisub spec;
        // 0 and 10+ are dropped so the renderer keeps the style's
        // Alignment field.
        assert!(parse_block(r"\an0").is_empty());
        assert!(parse_block(r"\an10").is_empty());
        assert!(parse_block(r"\an-1").is_empty());
    }

    #[test]
    fn parses_legacy_a_known_codes() {
        // Per the Aegisub spec: low nibble = L/C/R (1/2/3), +4 = top,
        // +8 = mid. So the recognised legacy codes are
        // {1,2,3,5,6,7,9,10,11}.
        let cases: &[(u8, u8)] = &[
            (1, 1),
            (2, 2),
            (3, 3),
            (5, 7),
            (6, 8),
            (7, 9),
            (9, 4),
            (10, 5),
            (11, 6),
        ];
        for (legacy, numpad) in cases {
            let src = format!(r"\a{legacy}");
            let v = parse_block(&src);
            assert_eq!(
                v,
                vec![AnimatedTag::A(*legacy)],
                "legacy code {} should parse",
                legacy
            );
            // And the apply path must map it to the right numpad
            // value on RenderState::alignment.
            let st = CueAnimation { tags: v }.evaluate_at(0, 1000);
            assert_eq!(
                st.alignment,
                Some(*numpad),
                "legacy {} should map to numpad {}",
                legacy,
                numpad
            );
        }
    }

    #[test]
    fn parses_legacy_a_unknown_codes_drop_override() {
        // Codes 4, 8, 12+ are not documented legacy slots; the parser
        // still records the AnimatedTag::A but the evaluator drops the
        // alignment override (style alignment wins).
        for legacy in [4_u8, 8, 12, 20, 255] {
            let src = format!(r"\a{legacy}");
            let v = parse_block(&src);
            assert_eq!(v, vec![AnimatedTag::A(legacy)]);
            let st = CueAnimation { tags: v }.evaluate_at(0, 1000);
            assert!(
                st.alignment.is_none(),
                "legacy {} should not override alignment",
                legacy
            );
        }
    }

    #[test]
    fn evaluate_an_static_override() {
        let cue_anim = CueAnimation {
            tags: vec![AnimatedTag::An(7)],
        };
        let st = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st.alignment, Some(7));
        // Default identity has no override.
        assert!(RenderState::identity().alignment.is_none());
        // Static — does not vary across the cue.
        let st_mid = cue_anim.evaluate_at(500, 1000);
        let st_end = cue_anim.evaluate_at(1000, 1000);
        assert_eq!(st_mid.alignment, Some(7));
        assert_eq!(st_end.alignment, Some(7));
    }

    #[test]
    fn an_static_inside_t_snaps_post() {
        // \an is not animatable per spec (Aegisub: "Specify the
        // alignment of the line"); inside \t it should snap to the
        // post-value once t1 has elapsed, mirroring \q.
        let v = parse_block(r"\an2\t(500,1000,\an8)");
        assert_eq!(v.len(), 2);
        let cue_anim = CueAnimation { tags: v };
        // Pre-transition: pre-value (numpad 2 = bottom-center).
        let st_before = cue_anim.evaluate_at(0, 1000);
        assert_eq!(st_before.alignment, Some(2));
        // Once t > t1: post-value (numpad 8 = top-center).
        let st_mid = cue_anim.evaluate_at(750, 1000);
        assert_eq!(st_mid.alignment, Some(8));
        let st_end = cue_anim.evaluate_at(1000, 1000);
        assert_eq!(st_end.alignment, Some(8));
    }

    #[test]
    fn an_later_overrides_earlier_legacy_a() {
        // Last-writer-wins, matching the static-override model.
        let v = parse_block(r"\a6\an1");
        assert_eq!(v.len(), 2);
        let st = CueAnimation { tags: v }.evaluate_at(0, 1000);
        assert_eq!(st.alignment, Some(1));
    }

    #[test]
    fn extract_an_from_cue_segment() {
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 1_000_000,
            style_ref: None,
            positioning: None,
            segments: vec![
                Segment::Raw(r"{\an5}".into()),
                Segment::Text("centered".into()),
            ],
        };
        let anim = extract_cue_animation(&cue);
        assert_eq!(anim.tags, vec![AnimatedTag::An(5)]);
        let st = anim.evaluate_at(0, 1000);
        assert_eq!(st.alignment, Some(5));
    }

    // ---------------------------------------------------------------
    // \k karaoke-timing family coverage (round 115).

    #[test]
    fn parses_k_family_kinds() {
        // Lowercase \k = instant Fill; \kf = Sweep; \ko = Outline.
        assert_eq!(
            parse_block(r"\k50"),
            vec![AnimatedTag::Karaoke {
                kind: KaraokeKind::Fill,
                cs: 50,
            }]
        );
        assert_eq!(
            parse_block(r"\kf30"),
            vec![AnimatedTag::Karaoke {
                kind: KaraokeKind::Sweep,
                cs: 30,
            }]
        );
        assert_eq!(
            parse_block(r"\ko20"),
            vec![AnimatedTag::Karaoke {
                kind: KaraokeKind::Outline,
                cs: 20,
            }]
        );
    }

    #[test]
    fn capital_k_is_sweep_identical_to_kf() {
        // Aegisub: "\K and \kf are identical". The uppercase form must
        // resolve to Sweep, not the lowercase \k Fill.
        let cap = parse_block(r"\K40");
        let kf = parse_block(r"\kf40");
        assert_eq!(
            cap,
            vec![AnimatedTag::Karaoke {
                kind: KaraokeKind::Sweep,
                cs: 40,
            }]
        );
        assert_eq!(cap, kf);
    }

    #[test]
    fn k_negative_duration_clamps_to_zero() {
        assert_eq!(
            parse_block(r"\k-10"),
            vec![AnimatedTag::Karaoke {
                kind: KaraokeKind::Fill,
                cs: 0,
            }]
        );
    }

    #[test]
    fn kt_is_not_handled() {
        // Aegisub explicitly leaves \kt undocumented/unsupported; we
        // skip it (the round-trip text path keeps it verbatim via Raw).
        assert!(parse_block(r"\kt100").is_empty());
    }

    #[test]
    fn karaoke_spans_are_cumulative() {
        // Two syllables of 50cs then 30cs → [0,500), [500,800) ms.
        let v = parse_block(r"\k50\kf30");
        let anim = CueAnimation { tags: v };
        let spans = anim.karaoke_spans();
        assert_eq!(
            spans,
            vec![
                KaraokeSpan {
                    kind: KaraokeKind::Fill,
                    start_ms: 0,
                    end_ms: 500,
                },
                KaraokeSpan {
                    kind: KaraokeKind::Sweep,
                    start_ms: 500,
                    end_ms: 800,
                },
            ]
        );
    }

    #[test]
    fn karaoke_span_progress() {
        let span = KaraokeSpan {
            kind: KaraokeKind::Sweep,
            start_ms: 500,
            end_ms: 800,
        };
        assert_eq!(span.progress(400), 0.0); // before
        assert_eq!(span.progress(500), 0.0); // at start
        assert!((span.progress(650) - 0.5).abs() < 1e-6); // halfway
        assert_eq!(span.progress(800), 1.0); // at end
        assert_eq!(span.progress(900), 1.0); // after
    }

    #[test]
    fn karaoke_zero_length_span_progress_is_one_past_start() {
        let span = KaraokeSpan {
            kind: KaraokeKind::Fill,
            start_ms: 100,
            end_ms: 100,
        };
        assert_eq!(span.progress(50), 0.0);
        assert_eq!(span.progress(150), 1.0);
    }

    #[test]
    fn karaoke_is_noop_on_render_state() {
        // \k carries timeline info, not per-frame transform/colour
        // state; evaluate_at must leave RenderState at identity.
        let v = parse_block(r"\k50\kf30");
        let st = CueAnimation { tags: v }.evaluate_at(250, 1000);
        assert_eq!(st, RenderState::identity());
    }

    #[test]
    fn extract_karaoke_from_cue_segments() {
        // Through the full parse → extract path the base parser emits
        // Segment::Karaoke markers; karaoke_spans must still resolve
        // their cumulative timing (kind defaults to Fill since the
        // marker drops the family member).
        use crate::parse;
        let src = "[Script Info]\n\
ScriptType: v4.00+\n\
\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, Alignment, MarginL, MarginR, MarginV, Outline, Shadow\n\
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,2,10,10,10,1,0\n\
\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,{\\k50}la{\\kf30}la\n";
        let t = parse(src.as_bytes()).unwrap();
        let anim = extract_cue_animation(&t.cues[0]);
        let spans = anim.karaoke_spans();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].start_ms, 0);
        assert_eq!(spans[0].end_ms, 500);
        assert_eq!(spans[1].start_ms, 500);
        assert_eq!(spans[1].end_ms, 800);
    }

    // --- \fn / \fe / \b / \r typed extraction ---

    #[test]
    fn parses_fn_font_family() {
        let mut v = Vec::new();
        parse_overrides("\\fnArial", &mut v);
        assert_eq!(v, vec![AnimatedTag::Fn("Arial".to_string())]);
    }

    #[test]
    fn parses_fn_font_family_with_spaces_in_name() {
        // The Aegisub spec example: `\fnTimes New Roman`. The name
        // runs verbatim until the next `\` or the end of the block,
        // so spaces inside the family name survive.
        let mut v = Vec::new();
        parse_overrides("\\fnTimes New Roman", &mut v);
        assert_eq!(v, vec![AnimatedTag::Fn("Times New Roman".to_string())]);
    }

    #[test]
    fn parses_fn_stops_at_next_backslash() {
        // Mixed override block: `\fn` reads up to the next `\`, the
        // following tag is parsed independently.
        let mut v = Vec::new();
        parse_overrides("\\fnArial\\fs24", &mut v);
        assert_eq!(
            v,
            vec![AnimatedTag::Fn("Arial".to_string()), AnimatedTag::Fs(24.0)]
        );
    }

    #[test]
    fn empty_fn_is_dropped() {
        // `\fn` with no name keeps the style's Fontname (per spec).
        let mut v = Vec::new();
        parse_overrides("\\fn", &mut v);
        assert!(v.is_empty());
        // Whitespace-only param is the same as empty.
        let mut v = Vec::new();
        parse_overrides("\\fn   \\fs10", &mut v);
        assert_eq!(v, vec![AnimatedTag::Fs(10.0)]);
    }

    #[test]
    fn fn_writes_render_state_font_name() {
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 1_000_000,
            style_ref: None,
            segments: vec![Segment::Raw("{\\fnArial}".to_string())],
            positioning: Default::default(),
        };
        let anim = extract_cue_animation(&cue);
        let st = anim.evaluate_at(500, 1000);
        assert_eq!(st.font_name.as_deref(), Some("Arial"));
    }

    #[test]
    fn parses_fe_charset_id() {
        // The Aegisub doc lists 128 (Shift-JIS) as a common value.
        let mut v = Vec::new();
        parse_overrides("\\fe128", &mut v);
        assert_eq!(v, vec![AnimatedTag::Fe(128)]);
        let mut v = Vec::new();
        parse_overrides("\\fe0", &mut v);
        assert_eq!(v, vec![AnimatedTag::Fe(0)]);
    }

    #[test]
    fn fe_out_of_range_is_dropped() {
        // Win32 charset IDs sit in 0..=255; anything outside is the
        // parser's "drop the override" path.
        let mut v = Vec::new();
        parse_overrides("\\fe-1", &mut v);
        assert!(v.is_empty());
        let mut v = Vec::new();
        parse_overrides("\\fe999", &mut v);
        assert!(v.is_empty());
    }

    #[test]
    fn fe_writes_render_state_encoding() {
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 1_000_000,
            style_ref: None,
            segments: vec![Segment::Raw("{\\fe134}".to_string())],
            positioning: Default::default(),
        };
        let anim = extract_cue_animation(&cue);
        let st = anim.evaluate_at(500, 1000);
        assert_eq!(st.font_encoding, Some(134));
    }

    #[test]
    fn parses_b_weight_legacy_toggle() {
        // \b1 = bold (= weight 700), \b0 = not bold (= weight 0).
        let mut v = Vec::new();
        parse_overrides("\\b1", &mut v);
        assert_eq!(v, vec![AnimatedTag::B(700)]);
        let mut v = Vec::new();
        parse_overrides("\\b0", &mut v);
        assert_eq!(v, vec![AnimatedTag::B(0)]);
    }

    #[test]
    fn parses_b_weight_explicit() {
        // Aegisub example: `{\b100}{\b300}{\b500}{\b700}{\b900}`.
        let mut v = Vec::new();
        parse_overrides("\\b500", &mut v);
        assert_eq!(v, vec![AnimatedTag::B(500)]);
        let mut v = Vec::new();
        parse_overrides("\\b900", &mut v);
        assert_eq!(v, vec![AnimatedTag::B(900)]);
    }

    #[test]
    fn b_weight_out_of_range_is_dropped() {
        // 50 is below 100, 1000 is above 900 — both outside the spec
        // range; `\b2`/`\b3` etc. are also rejected (the legacy
        // shortcut only recognises 0/1).
        let mut v = Vec::new();
        parse_overrides("\\b50", &mut v);
        assert!(v.is_empty());
        let mut v = Vec::new();
        parse_overrides("\\b1000", &mut v);
        assert!(v.is_empty());
        let mut v = Vec::new();
        parse_overrides("\\b2", &mut v);
        assert!(v.is_empty());
    }

    #[test]
    fn b_writes_render_state_bold_weight() {
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 1_000_000,
            style_ref: None,
            segments: vec![Segment::Raw("{\\b700}".to_string())],
            positioning: Default::default(),
        };
        let anim = extract_cue_animation(&cue);
        let st = anim.evaluate_at(500, 1000);
        assert_eq!(st.bold_weight, Some(700));
    }

    #[test]
    fn parses_r_bare_reset() {
        // Bare `\r` resets to the line's base style.
        let mut v = Vec::new();
        parse_overrides("\\r", &mut v);
        assert_eq!(v, vec![AnimatedTag::R(None)]);
    }

    #[test]
    fn parses_r_named_reset() {
        // `\rAlternate` resets to the named style.
        let mut v = Vec::new();
        parse_overrides("\\rAlternate", &mut v);
        assert_eq!(v, vec![AnimatedTag::R(Some("Alternate".to_string()))]);
    }

    #[test]
    fn r_resets_render_state_to_identity() {
        // Aegisub spec: "cancels all style overrides in effect,
        // including animations, for all following text."
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 1_000_000,
            style_ref: None,
            // Set frz + blur + color, THEN reset.
            segments: vec![Segment::Raw("{\\frz45\\blur3\\c&H0000FF&\\r}".to_string())],
            positioning: Default::default(),
        };
        let anim = extract_cue_animation(&cue);
        let st = anim.evaluate_at(500, 1000);
        // Every transform-state field is back at identity:
        assert_eq!(st.rotate_radians, 0.0);
        assert_eq!(st.blur_sigma, 0.0);
        assert_eq!(st.primary_color, None);
        // ... but the reset target survives so callers can spot it.
        assert_eq!(st.reset_to_style, Some(None));
    }

    #[test]
    fn r_named_resets_and_records_name() {
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 1_000_000,
            style_ref: None,
            segments: vec![Segment::Raw("{\\frz30\\rAlt}".to_string())],
            positioning: Default::default(),
        };
        let anim = extract_cue_animation(&cue);
        let st = anim.evaluate_at(500, 1000);
        assert_eq!(st.rotate_radians, 0.0);
        assert_eq!(st.reset_to_style, Some(Some("Alt".to_string())));
    }

    #[test]
    fn fn_snaps_inside_t_at_post_state() {
        // \fn isn't animatable — inside \t it snaps in at t > t1
        // rather than interpolating.
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 1_000_000,
            style_ref: None,
            segments: vec![Segment::Raw("{\\fnArial\\t(0,500,\\fnTimes)}".to_string())],
            positioning: Default::default(),
        };
        let anim = extract_cue_animation(&cue);
        // At t = 0 we're still on the pre-transition value (k = 0).
        let st0 = anim.evaluate_at(0, 1000);
        assert_eq!(st0.font_name.as_deref(), Some("Arial"));
        // Mid-way and after t1 we're on the post value.
        let st1 = anim.evaluate_at(250, 1000);
        assert_eq!(st1.font_name.as_deref(), Some("Times"));
        let st2 = anim.evaluate_at(600, 1000);
        assert_eq!(st2.font_name.as_deref(), Some("Times"));
    }

    #[test]
    fn b_snaps_inside_t_at_post_state() {
        // Same non-animatable snap behaviour for the bold weight.
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 1_000_000,
            style_ref: None,
            segments: vec![Segment::Raw("{\\b100\\t(0,500,\\b900)}".to_string())],
            positioning: Default::default(),
        };
        let anim = extract_cue_animation(&cue);
        let st0 = anim.evaluate_at(0, 1000);
        assert_eq!(st0.bold_weight, Some(100));
        let st1 = anim.evaluate_at(250, 1000);
        assert_eq!(st1.bold_weight, Some(900));
    }

    #[test]
    fn fe_snaps_inside_t_at_post_state() {
        let cue = SubtitleCue {
            start_us: 0,
            end_us: 1_000_000,
            style_ref: None,
            segments: vec![Segment::Raw("{\\fe0\\t(0,500,\\fe128)}".to_string())],
            positioning: Default::default(),
        };
        let anim = extract_cue_animation(&cue);
        let st0 = anim.evaluate_at(0, 1000);
        assert_eq!(st0.font_encoding, Some(0));
        let st1 = anim.evaluate_at(250, 1000);
        assert_eq!(st1.font_encoding, Some(128));
    }

    #[test]
    fn round_trip_keeps_fn_fe_b_r_verbatim() {
        // The base parser stores all four tag families in Segment::Raw
        // for the text round-trip (it only types-out \b / \r via its
        // Bold / state-reset arms; the rest reach the animate path
        // through Raw blocks). Confirm a parse → write cycle still
        // includes every tag we care about.
        let src = "\
[Script Info]\n\
ScriptType: v4.00+\n\
\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, Alignment, MarginL, MarginR, MarginV, Outline, Shadow\n\
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,2,10,10,10,1,0\n\
\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,{\\fnArial\\fe128\\b500\\rAlt}hi\n";
        let t = crate::parse(src.as_bytes()).unwrap();
        let bytes = crate::write(&t);
        let out = String::from_utf8(bytes).unwrap();
        // \fn / \fe / \b500 / \rAlt all survive verbatim — they were
        // stashed in the Raw passthrough block; \b500 (non-toggle)
        // doesn't decode to Segment::Bold because that arm only
        // honours bool flags.
        assert!(out.contains("\\fnArial"), "missing \\fn in: {out}");
        assert!(out.contains("\\fe128"), "missing \\fe in: {out}");
        assert!(out.contains("\\b500"), "missing \\b500 in: {out}");
        assert!(out.contains("\\rAlt"), "missing \\rAlt in: {out}");
        // Re-parse the writer's output and confirm the typed surface
        // still recovers each tag. The override block runs in the
        // order `{\fnArial\fe128\b500\rAlt}`, so `\r` is the LAST
        // tag — per the Aegisub spec it "cancels all style overrides
        // in effect" for the following text. The font / encoding /
        // weight overrides set immediately before it therefore
        // collapse back to "no override", and only the reset target
        // survives on the typed state.
        let t2 = crate::parse(out.as_bytes()).unwrap();
        let anim = extract_cue_animation(&t2.cues[0]);
        let st = anim.evaluate_at(500, 2000);
        assert_eq!(st.font_name, None);
        assert_eq!(st.font_encoding, None);
        assert_eq!(st.bold_weight, None);
        assert_eq!(st.reset_to_style, Some(Some("Alt".to_string())));
    }
}
