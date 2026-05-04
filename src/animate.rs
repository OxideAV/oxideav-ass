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
//! * `\clip(x1, y1, x2, y2)` — restrict rendering to the rectangle
//!   `[x1..x2] x [y1..y2]`. The drawing-path form is recognised but
//!   stored verbatim (round 2).
//! * `\fscx(percent)` / `\fscy(percent)` — non-uniform scale.
//! * `\t(t1, t2, [accel,] tags)` — interpolate the inner tags over
//!   `[t1, t2]` within the cue. Inner tags supported in this round:
//!   `\fscx`, `\fscy`, `\frz`, `\c` / `\1c`, `\fs`, `\blur`. Other
//!   inner tags are stored verbatim and applied as a static override
//!   for `t >= t1`.
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
    /// `\clip(drawing)` — drawing path form, stored verbatim. Round 2
    /// will translate this to a `Path` mask.
    ClipDrawing(String),
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
    /// `\c` primary-colour override, if active.
    pub primary_color: Option<(u8, u8, u8)>,
    /// `\fs` size override, if active.
    pub font_size: Option<f32>,
}

impl RenderState {
    /// State with no animated overrides.
    pub fn identity() -> Self {
        Self {
            alpha_mul: 1.0,
            transform: Transform2D::identity(),
            rotate_radians: 0.0,
            scale: (1.0, 1.0),
            translate: None,
            blur_sigma: 0.0,
            clip_rect: None,
            primary_color: None,
            font_size: None,
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
        AnimatedTag::ClipDrawing(_) => {
            // Round 2 — drawing-path clip not yet rasterised. We
            // intentionally do nothing here so existing behaviour is
            // preserved; the renderer can still see the tag verbatim
            // via `CueAnimation::tags`.
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
        "blur" | "be" => {
            // `\be(N)` is iterative box-blur in libass; we approximate
            // both as Gaussian sigma. `\be` strength N is roughly
            // sigma ≈ 0.6*sqrt(N) for the visual closeness; keep it
            // simple at sigma = N for now.
            param.trim().parse::<f32>().ok().map(AnimatedTag::Blur)
        }
        "fscx" => param.trim().parse::<f32>().ok().map(AnimatedTag::Fscx),
        "fscy" => param.trim().parse::<f32>().ok().map(AnimatedTag::Fscy),
        "fs" => param.trim().parse::<f32>().ok().map(AnimatedTag::Fs),
        "c" | "1c" => parse_color_rgb(param).map(AnimatedTag::Color1),
        "clip" => parse_clip(param),
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

fn parse_clip(param: &str) -> Option<AnimatedTag> {
    // `\clip(x1, y1, x2, y2)` rectangle (4 numeric args) or
    // `\clip([scale,] drawing)` path.
    let parts: Vec<&str> = param.split(',').map(|s| s.trim()).collect();
    if parts.len() == 4 {
        let n: Vec<Option<f32>> = parts.iter().map(|p| p.parse::<f32>().ok()).collect();
        if n.iter().all(|x| x.is_some()) {
            let n: Vec<f32> = n.into_iter().map(|x| x.unwrap()).collect();
            return Some(AnimatedTag::ClipRect {
                x1: n[0],
                y1: n[1],
                x2: n[2],
                y2: n[3],
            });
        }
    }
    Some(AnimatedTag::ClipDrawing(param.to_string()))
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
}
