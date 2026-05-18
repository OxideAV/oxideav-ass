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
//! * `\clip(x1, y1, x2, y2)` — restrict rendering to the rectangle
//!   `[x1..x2] x [y1..y2]`. The drawing-path form is recognised but
//!   stored verbatim (round 2).
//! * `\iclip(x1, y1, x2, y2)` — *inverse* rectangular clip: the cue
//!   is hidden inside the rectangle. Vector-drawing form is also
//!   accepted and stored verbatim in [`RenderState::iclip_drawing`].
//! * `\fscx(percent)` / `\fscy(percent)` — non-uniform scale.
//! * `\t(t1, t2, [accel,] tags)` — interpolate the inner tags over
//!   `[t1, t2]` within the cue. Inner tags supported in this round:
//!   `\fscx`, `\fscy`, `\frz`, `\c` / `\1c`, `\fs`, `\blur`, `\bord`,
//!   `\xbord`, `\ybord`, `\shad`, `\xshad`, `\yshad`, `\fax`, `\fay`.
//!   Other inner tags are stored verbatim and applied as a static
//!   override for `t >= t1`.
//!
//! Times in `\fad`, `\move`, `\t` are milliseconds *from the cue
//! start*. The ASS spec uses "ms from cue start" as the canonical
//! reference, matching libass / Aegisub.

use oxideav_core::{Segment, SubtitleCue, Transform2D};

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
            | Segment::Class { children, .. }
            | Segment::Karaoke { children, .. } => walk_segments(children, out),
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
        if let Some(t) = parse_one(&name_lc, &param) {
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

fn parse_one(name_lc: &str, param: &str) -> Option<AnimatedTag> {
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
        "c" | "1c" => parse_color_rgb(param).map(AnimatedTag::Color1),
        "clip" => parse_clip(param, false),
        "iclip" => parse_clip(param, true),
        "t" => parse_t(param),
        _ => None,
    }
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
}
