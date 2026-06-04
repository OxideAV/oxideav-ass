//! Animated subtitle decoder: wraps a base ASS subtitle decoder and
//! emits rasterised RGBA `Frame::Video`s sampled at a caller-controlled
//! moment in cue-local time.
//!
//! This decoder closes the gap between the static
//! [`oxideav_subtitle::RenderedSubtitleDecoder`] (one frame per cue) and
//! the time-varying state produced by
//! [`crate::CueAnimation::evaluate_at`]: callers can step the
//! `eval_offset_ms` field between calls to get a series of frames that
//! reflect the `\t` / `\fad` / `\move` / `\frx` / `\fry` / `\frz` /
//! `\fax` / `\fay` / `\blur` / `\be` / `\clip` overrides at successive
//! moments in the cue's lifetime.
//!
//! Pipeline (per `receive_frame`):
//!
//! 1. Pull the next [`SubtitleCue`] from the wrapped inner decoder.
//! 2. [`crate::extract_cue_animation`] the cue.
//! 3. Sample the animation at `(cue.start + eval_offset_ms)` clamped
//!    into the cue's `[start, end]` lifetime.
//! 4. Build a [`oxideav_core::VectorFrame`] containing the cue's
//!    shaped glyph nodes (via the supplied [`oxideav_scribe::FaceChain`])
//!    placed line-by-line — with `RenderState::letter_spacing`
//!    (`\fsp<spacing>`) injected as an extra `fsp` script-pixel gap
//!    between each pair of adjacent rendered glyphs and folded into
//!    each line's measured width so the word-wrap step picks the same
//!    breakpoints the placement loop will produce — then wrap them in
//!    a `Group` whose:
//!    - `transform` composes the animation's `move` ∘ pivoted
//!      `\frx`/`\fry`/`\frz` ∘ `\fscx`/`\fscy` matrix (3D rotations
//!      reduced to a 2D affine via a small-angle approximation around
//!      the pivot, so the renderer stays purely 2D) and a `\fax` /
//!      `\fay` shear pre-step pivoted on the cue's alignment point
//!      (independent of `\org`, per the Aegisub spec);
//!    - `opacity` is `RenderState::alpha_mul`;
//!    - `clip` is the `\clip(rect)` rectangle path or, if
//!      `\clip(drawing)` is active, the drawing-path parsed by
//!      [`crate::drawing::parse_drawing`].
//! 5. Rasterise via [`oxideav_raster::Renderer`].
//! 6. If `RenderState::blur_sigma > 0`, post-process the RGBA buffer
//!    through a separable Gaussian blur from
//!    [`oxideav_image_filter::Blur`]. The Aegisub override-tag
//!    reference describes `\blur<strength>` as a Gaussian edge-blur
//!    whose `strength` is non-integer — we treat that wire value as
//!    the kernel's sigma in pixels and pick the kernel radius as
//!    `ceil(3 * sigma)` (a 3σ cutoff captures > 99.7% of the kernel
//!    mass per the standard normal distribution). The blur runs on
//!    the rasterised RGBA buffer including the alpha channel so the
//!    softened edges land back through alpha — matching the spec's
//!    "blurs the edges of the text" behaviour for the no-border case
//!    the renderer covers today.
//! 7. If `RenderState::be_strength > 0`, post-process the RGBA buffer
//!    again with `N` iterations of a 3×3 box-blur. Per the Aegisub
//!    override-tag reference, `\be<strength>` is *"the number of times
//!    to apply the regular effect"* — a separable 1-pixel-radius box
//!    average. The renderer runs the box pass through all four RGBA
//!    channels (including alpha) so the softened silhouette falls
//!    back through alpha for the no-border text case, matching the
//!    spec's *"blurs the edges of the text"* behaviour and pairing
//!    with the Gaussian `\blur` step that runs first. The two filters
//!    stay on independent channels per the reference's *"more advanced
//!    algorithm vs iterative"* distinction; composing them in this
//!    fixed Gaussian-then-iterative order matches the order an author
//!    typically reads them as a final touch-up rather than an explicit
//!    spec ordering (none is given).
//!
//! The returned `Frame::Video` carries the cue's `start_us` as PTS.

use std::collections::VecDeque;

use oxideav_core::{
    CodecId, Decoder, Frame, Group, Node, Packet, Paint, Path, PathNode, Point, Result,
    Rgba as CoreRgba, Segment, SubtitleCue, TextAlign, TimeBase, Transform2D, VectorFrame,
    VideoFrame, VideoPlane,
};
use oxideav_scribe::{FaceChain, Shaper};

use crate::animate::{ClipRect, RenderState};
use crate::{drawing, extract_cue_animation};

/// Animated subtitle decoder. See module docs.
pub struct AnimatedRenderedDecoder {
    inner: Box<dyn Decoder>,
    codec_id: CodecId,
    width: u32,
    height: u32,
    face: FaceChain,
    /// Pre-cue queue holding the decoded cue + its extracted animation
    /// so multiple `receive_frame` calls at different `eval_offset_ms`
    /// settings reuse the same cue without re-pulling it.
    queue: VecDeque<CachedCue>,
    /// Offset from the current cue's `start_us`, in milliseconds, at
    /// which to sample the animation on the next `receive_frame` call.
    /// Defaults to `0` (cue start). Set via [`Self::set_offset_ms`].
    eval_offset_ms: i32,
    /// Default text colour when no `\c` override is active.
    pub default_color: [u8; 4],
    /// Nominal font size in pixels passed to the shaper.
    pub font_size_px: f32,
    /// Pixel margin between the canvas edge and the text bounding box.
    pub side_margin_px: u32,
    /// Pixel margin between the canvas bottom and the lowest baseline.
    pub bottom_margin_px: u32,
}

/// One decoded cue + its lazily-evaluated animation.
struct CachedCue {
    cue: SubtitleCue,
}

impl AnimatedRenderedDecoder {
    /// Build a new `AnimatedRenderedDecoder` wrapping `inner` and
    /// rendering at `width × height` using `face`.
    pub fn new(inner: Box<dyn Decoder>, width: u32, height: u32, face: FaceChain) -> Self {
        let codec_id = inner.codec_id().clone();
        Self {
            inner,
            codec_id,
            width,
            height,
            face,
            queue: VecDeque::new(),
            eval_offset_ms: 0,
            default_color: [255, 255, 255, 255],
            font_size_px: 24.0,
            side_margin_px: 8,
            bottom_margin_px: 24,
        }
    }

    /// Set the cue-relative time at which the *next* `receive_frame`
    /// call will sample the animation. Subsequent calls keep this
    /// offset until it's changed.
    pub fn set_offset_ms(&mut self, offset_ms: i32) {
        self.eval_offset_ms = offset_ms;
    }

    /// Current sampling offset in cue-relative milliseconds.
    pub fn offset_ms(&self) -> i32 {
        self.eval_offset_ms
    }
}

impl Decoder for AnimatedRenderedDecoder {
    fn codec_id(&self) -> &CodecId {
        &self.codec_id
    }

    fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        self.inner.send_packet(packet)
    }

    fn receive_frame(&mut self) -> Result<Frame> {
        // Top up the queue.
        if self.queue.is_empty() {
            match self.inner.receive_frame()? {
                Frame::Subtitle(c) => self.queue.push_back(CachedCue { cue: c }),
                other => return Ok(other),
            }
        }
        let entry = self.queue.front().expect("queue non-empty");
        let cue = &entry.cue;
        let dur_ms = ((cue.end_us - cue.start_us) / 1000).max(0) as i32;
        let t = self.eval_offset_ms.clamp(0, dur_ms);
        let anim = extract_cue_animation(cue);
        let state = anim.evaluate_at(t, dur_ms);
        let vf = self.render_cue_animated(cue, &state);
        Ok(Frame::Video(vf))
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }

    fn reset(&mut self) -> Result<()> {
        self.queue.clear();
        self.eval_offset_ms = 0;
        self.inner.reset()
    }
}

impl AnimatedRenderedDecoder {
    fn render_cue_animated(&self, cue: &SubtitleCue, state: &RenderState) -> VideoFrame {
        let mut buf = vec![0u8; (self.width as usize) * (self.height as usize) * 4];

        // Pick the working alignment. Per the Aegisub override-tag
        // reference, an `\an<pos>` / `\a<pos>` override on the line
        // wins over the style's `Alignment` field; the typed extractor
        // already resolved both into `RenderState::alignment` as a
        // 1..=9 numpad code. Fall back to the cue's positioning hint
        // (Left/Center/Right) — kept as the bottom row — when no
        // numpad override is active.
        let (align, vrow) = match state.alignment {
            Some(n) if (1..=9).contains(&n) => numpad_to_align(n),
            _ => {
                let h = cue
                    .positioning
                    .as_ref()
                    .map(|p| p.align)
                    .unwrap_or(TextAlign::Center);
                (h, VerticalRow::Bottom)
            }
        };

        // Flatten visible text from the cue's segments.
        let text = collect_visible_text(&cue.segments);
        if text.is_empty() {
            return wrap_buf(buf, self.width, cue.start_us);
        }

        // Lay out one or more visual lines (split on \n; greedy wrap by
        // shaped width).
        let face = &self.face;
        let max_text_w = self.width.saturating_sub(self.side_margin_px * 2);
        if max_text_w == 0 {
            return wrap_buf(buf, self.width, cue.start_us);
        }
        let logical_lines = text.split('\n').collect::<Vec<_>>();
        let size_px = if state.font_size.unwrap_or(self.font_size_px) > 0.0 {
            state.font_size.unwrap_or(self.font_size_px)
        } else {
            self.font_size_px
        };
        // Letter-spacing override (`\fsp<spacing>`). Per the Aegisub
        // override-tag reference, the value is an additional gap in
        // script-resolution pixels inserted between each pair of
        // adjacent glyphs (the spec text reads "the spacing between
        // the individual letters"). The value may be negative and may
        // be a decimal. `None` here means "fall back to the style's
        // `Spacing` field" — we don't have that field plumbed through
        // to the renderer yet, so a `None` falls all the way to zero
        // and leaves the shaper's natural advances untouched.
        //
        // We pass the value down to `wrap_line` / `measure` so the
        // greedy word-wrap uses the same widened width that the per-
        // glyph placement loop will produce — otherwise a positive
        // `\fsp` could fit fewer glyphs per visual line than the
        // wrapper thought.
        let fsp = state.letter_spacing.unwrap_or(0.0);
        let mut visual_lines: Vec<String> = Vec::new();
        for line in &logical_lines {
            for v in wrap_line(line, face, size_px, max_text_w as f32, fsp) {
                visual_lines.push(v);
            }
        }
        if visual_lines.is_empty() {
            return wrap_buf(buf, self.width, cue.start_us);
        }
        // Vertical layout depends on the alignment row. All three rows
        // share the same line-height stride; only the anchor of the
        // *last* baseline (= the bottom line's baseline) changes:
        //
        //   * Bottom row (numpad 1/2/3, or no override): the last
        //     baseline sits `bottom_margin_px + descent` above the
        //     canvas bottom — the existing behaviour.
        //   * Top row (numpad 7/8/9): the *first* baseline sits
        //     `top_margin_px + ascent` below the canvas top; the last
        //     baseline is therefore `top + (n-1) * line_h`.
        //   * Middle row (numpad 4/5/6): the line stack is centred
        //     vertically around the canvas mid-point — the centre of
        //     the stack (top of the first line + half the full block
        //     height) sits at `height / 2`.
        //
        // The renderer's existing `bottom_margin_px` doubles as the
        // top margin so a `\an7` cue mirrors a `\an1` cue's edge gap;
        // we deliberately do not introduce a separate field to keep
        // the API additive.
        let face_line_h = face.primary().line_height_px(size_px).ceil().max(1.0) as u32;
        let face_descent_abs = (-face.primary().descent_px(size_px)).ceil().max(0.0) as u32;
        let face_ascent_abs = face.primary().ascent_px(size_px).ceil().max(0.0) as u32;
        let line_h = face_line_h.max(1);
        let n_lines = visual_lines.len();
        let last_baseline = match vrow {
            VerticalRow::Bottom => self
                .height
                .saturating_sub(self.bottom_margin_px)
                .saturating_sub(face_descent_abs),
            VerticalRow::Top => {
                // First baseline at top_margin + ascent; last baseline
                // is `(n_lines - 1) * line_h` further down.
                let first = self.bottom_margin_px.saturating_add(face_ascent_abs);
                first.saturating_add(((n_lines - 1) as u32) * line_h)
            }
            VerticalRow::Middle => {
                // The line stack occupies `(n_lines - 1) * line_h +
                // (ascent + descent)` vertically. Centre that block on
                // the canvas mid-line, then pin the *last* baseline
                // accordingly: last = centre + (block_height / 2) -
                // descent.
                let block_h = ((n_lines - 1) as u32)
                    .saturating_mul(line_h)
                    .saturating_add(face_ascent_abs)
                    .saturating_add(face_descent_abs);
                let centre = self.height / 2;
                centre
                    .saturating_add(block_h / 2)
                    .saturating_sub(face_descent_abs)
            }
        };

        // Assemble per-glyph nodes inside an inner Group at canvas coords.
        let mut inner = Group::default();
        let mut anchor_x = self.width as f32 / 2.0;
        let anchor_y = last_baseline as f32;
        // Per-cue primary fill colour. RGB comes from `state.primary_color`
        // when `\c` / `\1c` set one; otherwise the decoder's
        // `default_color` (which the constructor seeds to opaque white).
        //
        // Per the override-tag reference, ASS encodes per-fill alpha as
        // `\1a&Haa&` with `0 = opaque, 255 = transparent` — the inverse
        // of the rasteriser's RGBA alpha channel, so the wire byte is
        // mapped via `255 - ass_a`. The cue-level `\fad` / `\fade`
        // envelope is tracked separately in `state.alpha_mul` and
        // applied as the animation `Group`'s `opacity`; the two compose
        // multiplicatively (see `RenderState::primary_alpha` for the
        // per-spec compose formula).
        let primary_color = {
            let (r, g, b) = state.primary_color.unwrap_or((
                self.default_color[0],
                self.default_color[1],
                self.default_color[2],
            ));
            let a = match state.primary_alpha {
                Some(ass_a) => 255u8.saturating_sub(ass_a),
                None => {
                    if state.primary_color.is_some() {
                        255
                    } else {
                        self.default_color[3]
                    }
                }
            };
            [r, g, b, a]
        };
        for (i, line) in visual_lines.iter().enumerate() {
            let glyphs = Shaper::shape_to_paths(face, line, size_px);
            // Per the Aegisub override-tag reference, `\fsp` inserts an
            // extra gap of `fsp` script-resolution pixels between each
            // adjacent pair of glyphs. `shape_to_paths` filters out
            // non-rendering glyphs (SPACE, etc.) but accumulates their
            // advances into the rendering glyphs that follow — so
            // adding `fsp_index * fsp` to each *rendered* glyph's X
            // gives one extra `fsp` gap between every pair of rendered
            // glyphs in the line. The line's overall width then grows
            // by `(n_glyphs - 1) * fsp` where `n_glyphs` is the count
            // of rendered glyphs returned by `shape_to_paths`. The
            // value can be negative; a sufficiently negative `fsp`
            // can overlap glyphs, which is the spec-described "spread
            // the text more out visually" tag used in reverse.
            let n_glyphs = glyphs.len();
            let extra_w = if n_glyphs > 1 {
                (n_glyphs as f32 - 1.0) * fsp
            } else {
                0.0
            };
            let line_w_px = measure(face, line, size_px) + extra_w;
            let line_x = match align {
                TextAlign::Left | TextAlign::Start => self.side_margin_px as f32,
                TextAlign::Right | TextAlign::End => {
                    (self.width as f32 - line_w_px - self.side_margin_px as f32)
                        .max(self.side_margin_px as f32)
                }
                TextAlign::Center => ((self.width as f32 - line_w_px) / 2.0).max(0.0),
            };
            let baseline_y =
                last_baseline.saturating_sub(((n_lines - 1 - i) as u32) * line_h) as f32;
            // Pick the anchor (= alignment point) from the last line for
            // pivot fallback.
            anchor_x = line_x + line_w_px / 2.0;
            let _ = anchor_y;

            let mut pen_x = line_x;
            let fill = Paint::Solid(rgba_to_core(primary_color));
            for (gi, (_face_idx, node, glyph_xform)) in glyphs.into_iter().enumerate() {
                let fsp_shift = (gi as f32) * fsp;
                let absolute =
                    Transform2D::translate(pen_x + fsp_shift, baseline_y).compose(&glyph_xform);
                let painted = repaint_node(node, &fill);
                inner.children.push(Node::Group(Group {
                    transform: absolute,
                    children: vec![painted],
                    ..Group::default()
                }));
            }
            pen_x += line_w_px;
            let _ = pen_x; // silence unused
        }

        // Compose the animation transform around the anchor (or
        // \org-supplied pivot). The anchor (the cue's alignment point)
        // is passed separately so the `\fax` / `\fay` shear step can
        // pivot on it regardless of where `\org` puts the rotation
        // pivot — per the Aegisub override-tag reference, "the
        // coordinate system used for shearing is not affected by the
        // rotation origin".
        let anchor = (anchor_x, last_baseline as f32);
        let pivot = state.pivot.unwrap_or(anchor);
        let anim_xf = animation_transform(state, pivot, anchor);

        // Optional clip. Precedence, matching the existing
        // "drawing beats rect" rule on the positive `\clip` side and
        // extending it to `\iclip`: `\clip(drawing)` →
        // `\clip(rect)` → `\iclip(drawing)` → `\iclip(rect)`. The
        // positive forms win when both a clip and an inverse clip
        // are set on the same segment — the renderer keeps the
        // existing "last-set-wins" model for the override pair
        // rather than trying to compose the intersection (the
        // Aegisub override-tag reference describes each form
        // independently and does not pin a co-occurrence rule).
        //
        // The inverse paths are built as compound paths with the
        // outer ring wound CCW and the inner ring CW so the
        // rasteriser's NonZero fill rule sees the area outside the
        // cut-out as the keep region. The outer ring extends well
        // past the canvas in script coordinates so any reasonable
        // animation transform leaves the keep region covering the
        // visible viewport.
        let canvas_w = self.width as f32;
        let canvas_h = self.height as f32;
        let clip_path = if let Some(s) = state.clip_drawing.as_ref() {
            let (scale, body) = drawing::split_clip_arg(s);
            Some(drawing::parse_drawing(body, scale))
        } else if let Some(r) = state.clip_rect.as_ref() {
            Some(rect_to_path(r))
        } else if let Some(s) = state.iclip_drawing.as_ref() {
            let (scale, body) = drawing::split_clip_arg(s);
            let inner = drawing::parse_drawing(body, scale);
            Some(inverse_path_from_inner(canvas_w, canvas_h, &inner))
        } else {
            state
                .iclip_rect
                .as_ref()
                .map(|r| inverse_rect_path(canvas_w, canvas_h, r))
        };

        let group = Group {
            transform: anim_xf,
            opacity: state.alpha_mul.clamp(0.0, 1.0),
            clip: clip_path,
            children: vec![Node::Group(inner)],
            ..Group::default()
        };

        // Rasterise.
        let frame = VectorFrame {
            width: self.width as f32,
            height: self.height as f32,
            view_box: None,
            root: Group {
                children: vec![Node::Group(group)],
                ..Group::default()
            },
            pts: None,
            time_base: TimeBase::new(1, 1),
        };
        let renderer = oxideav_raster::Renderer::new(self.width, self.height);
        let rendered = renderer.render(&frame);
        if let Some(plane) = rendered.planes.first() {
            // The renderer hands us the rasterised output sized to the
            // canvas; copy it straight into our buffer.
            let n = (self.width as usize) * (self.height as usize) * 4;
            let want = n.min(plane.data.len()).min(buf.len());
            buf[..want].copy_from_slice(&plane.data[..want]);
        }

        // Gaussian blur post-step (`\blur<strength>`). The Aegisub
        // override-tag reference describes the strength as the
        // Gaussian sigma (non-integer allowed). We pick the kernel
        // radius as `ceil(3 * sigma)` — the standard 3σ cutoff that
        // captures > 99.7% of the kernel mass — and clamp it to the
        // canvas's shorter side so a runaway value can't blow the
        // memory budget. `\be` is the iterative box-blur companion
        // and stays a separate channel on `RenderState`; renderers
        // wanting both should compose `\be` themselves (the
        // strength_count loop is one Box / equiv-radius pass each).
        if state.blur_sigma > 0.0 {
            apply_blur_post(&mut buf, self.width, self.height, state.blur_sigma);
        }

        // Iterative box-blur post-step (`\be<strength>`). Per the
        // Aegisub override-tag reference, strength is the number of
        // times to apply the "regular" softening — a separable
        // 1-pixel-radius box average. Running it after the Gaussian
        // pass lets the two filters compose without either stomping
        // the other's working buffer; the renderer chooses this order
        // because the spec does not pin one but `\be` reads as a final
        // mild touch-up rather than a primary edge-softener at the
        // strengths the reference describes as "isn't always very
        // visible". The box pass touches all four RGBA channels so the
        // softened silhouette falls back through alpha, matching the
        // spec's no-border "blurs the edges of the text" behaviour.
        if state.be_strength > 0 {
            apply_be_post(&mut buf, self.width, self.height, state.be_strength);
        }

        wrap_buf(buf, self.width, cue.start_us)
    }
}

/// Run `oxideav-image-filter`'s separable Gaussian blur over the
/// rasterised RGBA buffer in place. See [`AnimatedRenderedDecoder`]'s
/// module-level pipeline notes (step 6) for the strength-to-sigma
/// mapping the Aegisub spec calls for.
fn apply_blur_post(buf: &mut [u8], width: u32, height: u32, sigma: f32) {
    // Empty canvas → nothing to blur. Belt-and-braces: the caller
    // already gates on `sigma > 0`, but the filter would also no-op
    // on a 0×0 canvas — keep the early return so we don't allocate a
    // VideoFrame for nothing.
    if width == 0 || height == 0 {
        return;
    }
    let expected = (width as usize) * (height as usize) * 4;
    if buf.len() < expected {
        return;
    }
    let raw_radius = (3.0 * sigma).ceil() as i64;
    let max_radius = (width.min(height) / 2).max(1) as i64;
    let radius = raw_radius.clamp(1, max_radius) as u32;

    let input = oxideav_core::VideoFrame {
        pts: None,
        planes: vec![oxideav_core::VideoPlane {
            stride: (width as usize) * 4,
            data: buf[..expected].to_vec(),
        }],
    };
    let params = oxideav_image_filter::VideoStreamParams {
        format: oxideav_core::PixelFormat::Rgba,
        width,
        height,
    };
    let filter = oxideav_image_filter::Blur::new(radius).with_sigma(sigma);
    if let Ok(out) = oxideav_image_filter::ImageFilter::apply(&filter, &input, params) {
        if let Some(plane) = out.planes.first() {
            // The Blur filter ships a tight-stride output (= width *
            // bpp), so copy row-by-row only if its stride differs
            // from our canvas's tight stride. They match for the
            // RGBA full-resolution single-plane case, so the fast
            // path here is one straight copy.
            let want = expected.min(plane.data.len());
            buf[..want].copy_from_slice(&plane.data[..want]);
        }
    }
}

/// Apply `N` iterations of a separable 1-pixel-radius box blur to the
/// rasterised RGBA buffer in place — the renderer's `\be<strength>`
/// post-step. Each iteration is one horizontal then one vertical
/// 3-tap uniform mean (kernel `[1, 1, 1] / 3`), with edge samples
/// clamped to the nearest in-bounds pixel. All four channels including
/// alpha are blurred so the softened glyph silhouette lands back via
/// the alpha plane.
///
/// The repeated 1-pixel-radius pass is the "regular" softening the
/// Aegisub override-tag reference repeats `strength` times. We use a
/// uniform box rather than the `[1, 2, 1] / 4` variant because the
/// spec text reads as a basic box ("the iterative box-blur companion"
/// to the "more advanced" Gaussian `\blur`), and the [1, 2, 1] form
/// would converge to a Gaussian — overlapping `\blur`'s job.
///
/// Strength is `u8` to match `RenderState::be_strength`. Each
/// iteration costs `O(width * height * channels)`, so very large
/// values do degrade quickly; the spec itself warns *"at high values
/// the effect de-generates into nothingness, and generally isn't very
/// useful"*. We don't cap the strength here — the wire decoder already
/// clamps to `u8` and the cost is linear in the strength.
fn apply_be_post(buf: &mut [u8], width: u32, height: u32, strength: u8) {
    if width == 0 || height == 0 || strength == 0 {
        return;
    }
    let expected = (width as usize) * (height as usize) * 4;
    if buf.len() < expected {
        return;
    }
    let w = width as usize;
    let h = height as usize;
    let row = w * 4;
    let mut scratch = vec![0u8; expected];
    for _ in 0..strength {
        // Pass 1: horizontal 3-tap box into scratch.
        for y in 0..h {
            let src_row = &buf[y * row..(y + 1) * row];
            let dst_row = &mut scratch[y * row..(y + 1) * row];
            for x in 0..w {
                let xl = x.saturating_sub(1);
                let xr = (x + 1).min(w - 1);
                for ch in 0..4 {
                    let a = src_row[xl * 4 + ch] as u32;
                    let b = src_row[x * 4 + ch] as u32;
                    let c = src_row[xr * 4 + ch] as u32;
                    // (a + b + c + 1) / 3 — round-to-nearest with a +1
                    // bias on the divisor's edge of the rounding range.
                    // Plain integer division here is fine for a "subtle
                    // softening" but biases the result slightly down;
                    // the +1 keeps the mean centred so a constant patch
                    // is preserved exactly (3*v + 1)/3 == v.
                    dst_row[x * 4 + ch] = ((a + b + c + 1) / 3) as u8;
                }
            }
        }
        // Pass 2: vertical 3-tap box back into buf.
        for y in 0..h {
            let yu = y.saturating_sub(1);
            let yd = (y + 1).min(h - 1);
            let up_row = &scratch[yu * row..(yu + 1) * row];
            let mid_row = &scratch[y * row..(y + 1) * row];
            let dn_row = &scratch[yd * row..(yd + 1) * row];
            let dst_row = &mut buf[y * row..(y + 1) * row];
            for x in 0..w {
                for ch in 0..4 {
                    let a = up_row[x * 4 + ch] as u32;
                    let b = mid_row[x * 4 + ch] as u32;
                    let c = dn_row[x * 4 + ch] as u32;
                    dst_row[x * 4 + ch] = ((a + b + c + 1) / 3) as u8;
                }
            }
        }
    }
}

/// Which row of the Aegisub numpad-alignment table a cue is anchored
/// to. Decomposed from the 1..=9 code by [`numpad_to_align`]; drives
/// the renderer's vertical-baseline pick (see
/// `AnimatedRenderedDecoder::render_cue_animated`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum VerticalRow {
    /// Numpad 1/2/3 — text sits above the canvas bottom margin.
    Bottom,
    /// Numpad 4/5/6 — text is centred vertically on the canvas.
    Middle,
    /// Numpad 7/8/9 — text sits below the canvas top margin.
    Top,
}

/// Decompose an Aegisub numpad alignment code (1..=9) into the
/// horizontal `TextAlign` and the [`VerticalRow`] anchor per the
/// override-tag reference's "1/2/3 = bottom-{left,center,right};
/// 4/5/6 = middle-{left,center,right}; 7/8/9 = top-{left,center,
/// right}" mapping.
///
/// Values outside `1..=9` fall through as `(Center, Bottom)`; callers
/// must filter unknown codes ahead of time (the typed extractor
/// already drops out-of-range codes from `RenderState::alignment`).
fn numpad_to_align(n: u8) -> (TextAlign, VerticalRow) {
    let row = match (n - 1) / 3 {
        0 => VerticalRow::Bottom,
        1 => VerticalRow::Middle,
        _ => VerticalRow::Top,
    };
    let col = match (n - 1) % 3 {
        0 => TextAlign::Left,
        1 => TextAlign::Center,
        _ => TextAlign::Right,
    };
    (col, row)
}

fn wrap_buf(data: Vec<u8>, width: u32, start_us: i64) -> VideoFrame {
    let stride = (width as usize) * 4;
    VideoFrame {
        pts: Some(start_us),
        planes: vec![VideoPlane { stride, data }],
    }
}

fn rgba_to_core(c: [u8; 4]) -> CoreRgba {
    CoreRgba::new(c[0], c[1], c[2], c[3])
}

fn rect_to_path(r: &ClipRect) -> Path {
    let mut p = Path::new();
    p.move_to(Point::new(r.x1, r.y1));
    p.line_to(Point::new(r.x2, r.y1));
    p.line_to(Point::new(r.x2, r.y2));
    p.line_to(Point::new(r.x1, r.y2));
    p.close();
    p
}

/// Outer-ring extents used by the inverse-clip builders.
///
/// The outer ring extends well past the canvas (`[-canvas, +2 *
/// canvas]`) so any reasonable animation transform — translate,
/// scale, rotation — applied to the group still leaves the viewport
/// inside the outer ring. A `0 × 0` canvas degrades to a tiny but
/// non-empty extent so the rasteriser's flatten + fill steps still
/// have something to chew on.
fn inverse_outer_extents(canvas_w: f32, canvas_h: f32) -> (f32, f32, f32, f32) {
    let w = if canvas_w > 0.0 { canvas_w } else { 1.0 };
    let h = if canvas_h > 0.0 { canvas_h } else { 1.0 };
    (-w, -h, 2.0 * w, 2.0 * h)
}

/// Build the inverse-rect clip path: an outer ring well past the
/// canvas (CW in screen-space, matching [`rect_to_path`]) followed
/// by the inner cut-out ring (CCW — reverse direction). With the
/// rasteriser's NonZero fill rule the donut interior is everything
/// **outside** the inner rectangle but inside the outer extents —
/// i.e. the keep region the `\iclip(rect)` override calls for.
fn inverse_rect_path(canvas_w: f32, canvas_h: f32, r: &ClipRect) -> Path {
    let (ox1, oy1, ox2, oy2) = inverse_outer_extents(canvas_w, canvas_h);
    let mut p = Path::new();
    // Outer ring — same direction as `rect_to_path` (the positive form
    // that fills the rectangle interior under NonZero).
    p.move_to(Point::new(ox1, oy1));
    p.line_to(Point::new(ox2, oy1));
    p.line_to(Point::new(ox2, oy2));
    p.line_to(Point::new(ox1, oy2));
    p.close();
    // Inner ring — reverse direction so its winding cancels the outer
    // ring's inside the cut-out, leaving zero winding (no fill) there.
    p.move_to(Point::new(r.x1, r.y1));
    p.line_to(Point::new(r.x1, r.y2));
    p.line_to(Point::new(r.x2, r.y2));
    p.line_to(Point::new(r.x2, r.y1));
    p.close();
    p
}

/// Build the inverse-drawing clip path: an outer ring well past the
/// canvas followed by the inner drawing's commands. The outer ring
/// is wound the same way as [`rect_to_path`] (the positive form);
/// the renderer relies on the drawing's natural winding cancelling
/// it inside the drawing's interior under NonZero. Drawings whose
/// outer subpath happens to share the rect's winding direction will
/// stack rather than cancel — the spec notes that the inverse-drawing
/// form mirrors the positive `\clip` drawing parser; co-wound paths
/// are not a common authoring case.
fn inverse_path_from_inner(canvas_w: f32, canvas_h: f32, inner: &Path) -> Path {
    let (ox1, oy1, ox2, oy2) = inverse_outer_extents(canvas_w, canvas_h);
    let mut p = Path::new();
    p.move_to(Point::new(ox1, oy1));
    p.line_to(Point::new(ox2, oy1));
    p.line_to(Point::new(ox2, oy2));
    p.line_to(Point::new(ox1, oy2));
    p.close();
    // Append the inner path commands in reverse traversal so its
    // winding flips relative to its natural orientation; the inner
    // and outer thus disagree on direction and the NonZero rule
    // cuts the inner shape out of the outer fill.
    for cmd in reversed_path_commands(inner) {
        p.commands.push(cmd);
    }
    p
}

/// Reverse the traversal direction of `path` while preserving its
/// subpath structure. `MoveTo` markers stay at the start of each
/// subpath; `LineTo` / `QuadCurveTo` / `CubicCurveTo` segments swap
/// endpoints (and Bezier control points reverse so the curve still
/// passes through the same set of points in the opposite direction);
/// `Close` markers stay where they were.
fn reversed_path_commands(path: &Path) -> Vec<oxideav_core::PathCommand> {
    use oxideav_core::PathCommand;
    // Split into subpaths first so each subpath can be reversed in
    // isolation. A subpath starts at a `MoveTo` and ends at the next
    // `MoveTo` boundary; a trailing `Close` belongs to the subpath
    // it closes.
    let mut subpaths: Vec<Vec<PathCommand>> = Vec::new();
    let mut current: Vec<PathCommand> = Vec::new();
    for cmd in &path.commands {
        match cmd {
            PathCommand::MoveTo(_) => {
                if !current.is_empty() {
                    subpaths.push(std::mem::take(&mut current));
                }
                current.push(*cmd);
            }
            _ => current.push(*cmd),
        }
    }
    if !current.is_empty() {
        subpaths.push(current);
    }

    let mut out: Vec<PathCommand> = Vec::new();
    for sub in subpaths {
        // Strip the trailing Close (it goes back on at the end).
        let (close, body) = match sub.last() {
            Some(PathCommand::Close) => (true, &sub[..sub.len() - 1]),
            _ => (false, &sub[..]),
        };
        // Collect the subpath's vertices in traversal order: the
        // MoveTo's anchor first, then each segment's endpoint.
        let mut verts: Vec<Point> = Vec::new();
        // First command is the MoveTo (subpaths always start with
        // one in well-formed paths; default to origin otherwise).
        let start = match body.first() {
            Some(PathCommand::MoveTo(p)) => *p,
            _ => Point::new(0.0, 0.0),
        };
        verts.push(start);
        for cmd in &body[1..] {
            match cmd {
                PathCommand::LineTo(p) => verts.push(*p),
                PathCommand::QuadCurveTo { end, .. } => verts.push(*end),
                PathCommand::CubicCurveTo { end, .. } => verts.push(*end),
                PathCommand::MoveTo(p) => verts.push(*p),
                _ => {}
            }
        }
        if verts.len() < 2 {
            // Degenerate subpath — keep as-is so we don't lose the
            // anchor point entirely.
            out.extend_from_slice(body);
            if close {
                out.push(PathCommand::Close);
            }
            continue;
        }

        // Emit reversed: start at the last vertex, walk backward.
        out.push(PathCommand::MoveTo(*verts.last().unwrap()));
        // For each original segment i (i in 1..verts.len()), the
        // reversed traversal walks from verts[i] back to verts[i-1].
        // We re-issue segments in reverse order to match.
        for i in (1..verts.len()).rev() {
            let orig_cmd = &body[i];
            match orig_cmd {
                PathCommand::LineTo(_) | PathCommand::MoveTo(_) => {
                    out.push(PathCommand::LineTo(verts[i - 1]));
                }
                PathCommand::QuadCurveTo { control, .. } => {
                    // Quad reversed: same control point, swap endpoints.
                    out.push(PathCommand::QuadCurveTo {
                        control: *control,
                        end: verts[i - 1],
                    });
                }
                PathCommand::CubicCurveTo { c1, c2, .. } => {
                    // Cubic reversed: swap control points and endpoints.
                    out.push(PathCommand::CubicCurveTo {
                        c1: *c2,
                        c2: *c1,
                        end: verts[i - 1],
                    });
                }
                _ => {}
            }
        }
        if close {
            out.push(PathCommand::Close);
        }
    }
    out
}

fn repaint_node(node: Node, paint: &Paint) -> Node {
    match node {
        Node::Path(PathNode {
            path,
            stroke,
            fill_rule,
            ..
        }) => Node::Path(PathNode {
            path,
            fill: Some(paint.clone()),
            stroke,
            fill_rule,
        }),
        Node::Group(mut g) => {
            g.children = g
                .children
                .into_iter()
                .map(|c| repaint_node(c, paint))
                .collect();
            Node::Group(g)
        }
        other => other,
    }
}

fn measure(face: &FaceChain, text: &str, size_px: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    match face.shape(text, size_px) {
        Ok(g) => oxideav_scribe::run_width(&g),
        Err(_) => 0.0,
    }
}

/// Measure `text` for layout, including an extra `fsp` script-pixel
/// gap between each pair of adjacent rendered glyphs (the renderer's
/// `\fsp<spacing>` letter-spacing surface). The rendered-glyph count
/// is the [`Shaper::shape_to_paths`] output length — non-rendering
/// glyphs (SPACE, etc.) don't get an extra gap added because the
/// placement loop in `render_cue_animated` iterates the rendered
/// nodes only. Returns the same value as [`measure`] when `fsp == 0`.
fn measure_with_fsp(face: &FaceChain, text: &str, size_px: f32, fsp: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let base = measure(face, text, size_px);
    if fsp == 0.0 {
        return base;
    }
    let n = Shaper::shape_to_paths(face, text, size_px).len();
    if n <= 1 {
        return base;
    }
    base + (n as f32 - 1.0) * fsp
}

/// Greedy word-wrap by shaped width. Returns visual lines. `fsp` is
/// the `\fsp<spacing>` letter-spacing in script-resolution pixels and
/// is added to the measured line width so the wrapper picks the same
/// breakpoints the per-glyph placement loop will hit.
fn wrap_line(line: &str, face: &FaceChain, size_px: f32, max_w: f32, fsp: f32) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }
    if measure_with_fsp(face, line, size_px, fsp) <= max_w {
        return vec![line.to_string()];
    }
    // Tokenise into space-separated words; greedy fill.
    let words: Vec<&str> = line.split(' ').collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for w in words {
        let candidate = if cur.is_empty() {
            w.to_string()
        } else {
            format!("{} {}", cur, w)
        };
        if measure_with_fsp(face, &candidate, size_px, fsp) <= max_w {
            cur = candidate;
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            cur = w.to_string();
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Walk the cue segments and return the visible text (LineBreak →
/// `\n`, override `Raw` blocks dropped). Used to feed the shaper.
fn collect_visible_text(segs: &[Segment]) -> String {
    let mut out = String::new();
    walk_text(segs, &mut out);
    out
}

fn walk_text(segs: &[Segment], out: &mut String) {
    for s in segs {
        match s {
            Segment::Text(t) => out.push_str(t),
            Segment::LineBreak => out.push('\n'),
            Segment::Bold(c) | Segment::Italic(c) | Segment::Underline(c) | Segment::Strike(c) => {
                walk_text(c, out)
            }
            Segment::Color { children, .. }
            | Segment::Font { children, .. }
            | Segment::Voice { children, .. }
            | Segment::Class { children, .. }
            | Segment::Karaoke { children, .. } => walk_text(children, out),
            Segment::Timestamp { .. } => {}
            // Override-tag round-trip blocks contribute no visible text.
            Segment::Raw(_) => {}
        }
    }
}

/// Build the affine 2D transform that approximates the animation's
/// translate / scale / 3D rotations around `pivot`, with a `\fax` /
/// `\fay` shear pre-step pivoted on `anchor`.
///
/// The 2D affine pipeline we apply (right-to-left) is:
///
/// 1. translate(-anchor) — shift the cue's alignment point to the
///    origin so the shear pivots on it.
/// 2. shear(fax, fay) — the per-tag-reference matrix
///    `[[1, fax], [fay, 1]]`. Per the Aegisub override-tag reference,
///    "the coordinate system used for shearing is not affected by the
///    rotation origin", so the shear's pivot is the alignment point
///    rather than `\org`. The shear is folded into the text-local
///    frame *before* rotation/scale; the subsequent rotation then
///    carries the distortion along, matching the spec's "after
///    rotation, on the rotated coordinates" effect.
/// 3. translate(+anchor) — undo the shear pivot.
/// 4. translate(-pivot)
/// 5. scale(sx, sy)
/// 6. shear/squeeze approximating `\fry` (X scale by cos α_y) and
///    `\frx` (Y scale by cos α_x). True 3D would project onto a
///    perspective camera; here we use the small-angle / orthographic
///    approximation: the visible width shrinks by `cos(α_y)` for a
///    rotation around Y and the visible height by `cos(α_x)` for a
///    rotation around X. This is the standard "fold in half" effect
///    most ASS renderers fall back on when no perspective camera is
///    configured.
/// 7. rotate(α_z) (`\frz`)
/// 8. translate(+pivot)
/// 9. translate(extra_translate) when `\pos` / `\move` set one.
fn animation_transform(state: &RenderState, pivot: (f32, f32), anchor: (f32, f32)) -> Transform2D {
    let (px, py) = pivot;
    let (ax, ay) = anchor;
    let (fax, fay) = state.shear;
    let has_shear = fax.abs() > f32::EPSILON || fay.abs() > f32::EPSILON;

    // Anchor-relative shear pre-step. Applied to glyph coords before
    // any rotation/scale, so the rotation carries the distortion with
    // the text.
    let mut t = Transform2D::identity();
    if has_shear {
        t = Transform2D::translate(-ax, -ay);
        t = shear_matrix(fax, fay).compose(&t);
        t = Transform2D::translate(ax, ay).compose(&t);
    }

    // Pivot-relative scale/3D/rotate.
    t = Transform2D::translate(-px, -py).compose(&t);
    let (sx, sy) = state.scale;
    if (sx - 1.0).abs() > f32::EPSILON || (sy - 1.0).abs() > f32::EPSILON {
        t = Transform2D::scale(sx, sy).compose(&t);
    }
    // 3D approximation: scale x by |cos(fry)|, y by |cos(frx)|.
    // (True foreshortening; sign change at >90° is not modelled — most
    // subtitle use cases rotate <90°.)
    let cy = state.rotate_y_radians.cos();
    let cx = state.rotate_x_radians.cos();
    if (cy - 1.0).abs() > 1e-6 || (cx - 1.0).abs() > 1e-6 {
        let fx = if cy.abs() < 1e-3 { 1e-3 } else { cy };
        let fy = if cx.abs() < 1e-3 { 1e-3 } else { cx };
        t = Transform2D::scale(fx, fy).compose(&t);
    }
    if state.rotate_radians.abs() > f32::EPSILON {
        t = Transform2D::rotate(state.rotate_radians).compose(&t);
    }
    t = Transform2D::translate(px, py).compose(&t);
    if let Some((tx, ty)) = state.translate {
        // \pos / \move sets an absolute target — translate the pivot
        // there.
        t = Transform2D::translate(tx - px, ty - py).compose(&t);
    }
    t
}

/// Build the `\fax` / `\fay` shear matrix in column-vector convention:
///
/// ```text
/// [ 1   fax ] [x]   [x + fax*y]
/// [ fay   1 ] [y] = [fay*x + y]
/// ```
///
/// Mapping into the `Transform2D` `(a, b, c, d, e, f)` layout (where
/// `apply(p) = (a*x + c*y + e, b*x + d*y + f)`): `a = 1`, `b = fay`,
/// `c = fax`, `d = 1`, `e = f = 0`. The matrix is centred at the
/// origin; the caller wraps it in the anchor translate pair to put
/// the shear's pivot at the cue's alignment point.
fn shear_matrix(fax: f32, fay: f32) -> Transform2D {
    Transform2D {
        a: 1.0,
        b: fay,
        c: fax,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    }
}

/// Factory helper: wrap an existing subtitle decoder + face into a
/// boxed [`AnimatedRenderedDecoder`].
pub fn make_animated_decoder(
    inner: Box<dyn Decoder>,
    width: u32,
    height: u32,
    face: FaceChain,
) -> Box<dyn Decoder> {
    Box::new(AnimatedRenderedDecoder::new(inner, width, height, face))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxideav_core::{CuePosition, SubtitleCue};

    fn dummy_cue() -> SubtitleCue {
        SubtitleCue {
            start_us: 0,
            end_us: 1_000_000,
            style_ref: None,
            positioning: Some(CuePosition {
                align: TextAlign::Center,
                ..Default::default()
            }),
            segments: vec![Segment::Text("hi".into())],
        }
    }

    #[test]
    fn animation_transform_pivots_around_anchor() {
        // 90° \frz around pivot (10,10): the pivot itself maps to itself.
        let mut st = RenderState::identity();
        st.rotate_radians = std::f32::consts::FRAC_PI_2;
        let t = animation_transform(&st, (10.0, 10.0), (10.0, 10.0));
        let p = t.apply(Point::new(10.0, 10.0));
        assert!((p.x - 10.0).abs() < 1e-4);
        assert!((p.y - 10.0).abs() < 1e-4);
    }

    #[test]
    fn frx_compresses_y() {
        // 60° \frx → cos(60°) = 0.5: y distances around pivot halve.
        let mut st = RenderState::identity();
        st.rotate_x_radians = std::f32::consts::FRAC_PI_3;
        let t = animation_transform(&st, (0.0, 0.0), (0.0, 0.0));
        let p = t.apply(Point::new(0.0, 100.0));
        assert!((p.y - 50.0).abs() < 1e-3, "got y={}", p.y);
    }

    #[test]
    fn fry_compresses_x() {
        let mut st = RenderState::identity();
        st.rotate_y_radians = std::f32::consts::FRAC_PI_3;
        let t = animation_transform(&st, (0.0, 0.0), (0.0, 0.0));
        let p = t.apply(Point::new(100.0, 0.0));
        assert!((p.x - 50.0).abs() < 1e-3, "got x={}", p.x);
    }

    #[test]
    fn org_overrides_anchor_pivot() {
        let mut st = RenderState::identity();
        st.rotate_radians = std::f32::consts::FRAC_PI_2;
        st.pivot = Some((100.0, 100.0));
        let t = animation_transform(&st, st.pivot.unwrap(), (0.0, 0.0));
        let p = t.apply(Point::new(100.0, 100.0));
        assert!((p.x - 100.0).abs() < 1e-4);
        assert!((p.y - 100.0).abs() < 1e-4);
    }

    #[test]
    fn fax_shears_x_by_y_around_anchor() {
        // \fax(0.5) at anchor (0,0): a point at y=100 shifts +50 in x.
        let mut st = RenderState::identity();
        st.shear = (0.5, 0.0);
        let t = animation_transform(&st, (0.0, 0.0), (0.0, 0.0));
        let p = t.apply(Point::new(0.0, 100.0));
        assert!((p.x - 50.0).abs() < 1e-4, "got x={}", p.x);
        assert!((p.y - 100.0).abs() < 1e-4, "got y={}", p.y);
        // The anchor itself maps to itself under a pure shear.
        let a = t.apply(Point::new(0.0, 0.0));
        assert!(a.x.abs() < 1e-4 && a.y.abs() < 1e-4);
    }

    #[test]
    fn fay_shears_y_by_x_around_anchor() {
        // \fay(-0.25) at anchor (50,50): a point at x=150 (Δx=+100)
        // shifts y by -0.25 * 100 = -25.
        let mut st = RenderState::identity();
        st.shear = (0.0, -0.25);
        let t = animation_transform(&st, (50.0, 50.0), (50.0, 50.0));
        let p = t.apply(Point::new(150.0, 50.0));
        assert!((p.x - 150.0).abs() < 1e-4, "got x={}", p.x);
        assert!((p.y - 25.0).abs() < 1e-4, "got y={}", p.y);
    }

    #[test]
    fn shear_pivots_on_anchor_not_org() {
        // \org(200,200) puts the rotation pivot far from the anchor
        // (10,10). \fax(0.5) shear must still pivot on the anchor —
        // the anchor itself must stay invariant under the pre-rotate
        // shear step (here with rotation disabled so the result is the
        // pure shear pipeline).
        let mut st = RenderState::identity();
        st.shear = (0.5, 0.0);
        st.pivot = Some((200.0, 200.0));
        let t = animation_transform(&st, (200.0, 200.0), (10.0, 10.0));
        let p = t.apply(Point::new(10.0, 10.0));
        assert!((p.x - 10.0).abs() < 1e-4, "anchor x not preserved: {}", p.x);
        assert!((p.y - 10.0).abs() < 1e-4, "anchor y not preserved: {}", p.y);
        // A point above the anchor (Δy=+50) shears by +25 in x and is
        // otherwise unchanged.
        let q = t.apply(Point::new(10.0, 60.0));
        assert!((q.x - 35.0).abs() < 1e-4, "got x={}", q.x);
        assert!((q.y - 60.0).abs() < 1e-4, "got y={}", q.y);
    }

    #[test]
    fn shear_matrix_layout_matches_spec() {
        // The shear matrix on its own — sanity check that the column-
        // vector convention from the Aegisub override-tag reference
        // round-trips through `Transform2D::apply`.
        let m = shear_matrix(0.3, -0.2);
        let p = m.apply(Point::new(1.0, 0.0));
        assert!((p.x - 1.0).abs() < 1e-6);
        assert!((p.y + 0.2).abs() < 1e-6);
        let q = m.apply(Point::new(0.0, 1.0));
        assert!((q.x - 0.3).abs() < 1e-6);
        assert!((q.y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn collects_visible_text() {
        let segs = vec![
            Segment::Text("a".into()),
            Segment::LineBreak,
            Segment::Bold(vec![Segment::Text("b".into())]),
            Segment::Raw("{\\fad(0,0)}".into()),
        ];
        assert_eq!(collect_visible_text(&segs), "a\nb");
    }

    #[test]
    fn rect_to_path_has_5_commands() {
        let r = ClipRect {
            x1: 0.0,
            y1: 0.0,
            x2: 10.0,
            y2: 10.0,
        };
        let p = rect_to_path(&r);
        assert_eq!(p.commands.len(), 5);
    }

    #[test]
    fn dummy_cue_yields_text() {
        // Smoke check.
        let c = dummy_cue();
        assert_eq!(collect_visible_text(&c.segments), "hi");
    }

    #[test]
    fn blur_post_step_no_ops_when_radius_clamp_yields_zero_canvas() {
        // 0×0 canvas — the helper must early-out without touching the
        // buffer (its assertion is "no panic, no allocation"). Use a
        // small dummy buffer so a debug build still flags an OOB read.
        let mut buf = vec![0u8; 16];
        let before = buf.clone();
        super::apply_blur_post(&mut buf, 0, 0, 4.0);
        assert_eq!(buf, before, "blur post-step touched a 0×0 buffer");
    }

    #[test]
    fn blur_post_step_short_buffer_is_no_op() {
        // Buffer shorter than width * height * 4 — the helper must not
        // touch it (defensive against a caller passing the wrong
        // canvas pair). The frame's contract is "stride = width*4",
        // so a short buffer is genuinely a bug, but the helper should
        // not paper over it by reading past the end.
        let mut buf = vec![0u8; 8]; // way smaller than 4*4*4 = 64
        let before = buf.clone();
        super::apply_blur_post(&mut buf, 4, 4, 1.5);
        assert_eq!(buf, before, "blur post-step touched a too-short buffer");
    }

    #[test]
    fn blur_post_step_softens_a_hard_edge() {
        // Construct a 16×8 RGBA buffer with a vertical hard edge —
        // left half opaque white, right half fully transparent. After
        // the Gaussian post-step the alpha along the seam should
        // smear across the boundary so the middle two columns pick up
        // some alpha. This pins the "blur > 0 actually mutates the
        // alpha plane" half of the contract independent of the full
        // renderer path tested in the integration suite.
        let w = 16u32;
        let h = 8u32;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                if x < w / 2 {
                    buf[i] = 255;
                    buf[i + 1] = 255;
                    buf[i + 2] = 255;
                    buf[i + 3] = 255;
                }
                // right half stays at zeros.
            }
        }
        let before = buf.clone();
        super::apply_blur_post(&mut buf, w, h, 1.5);
        // The seam column on the right (x = w/2) was 0 alpha; after
        // the blur it should pick up some alpha from the left
        // neighbours.
        let mid = ((3 * w + w / 2) * 4 + 3) as usize;
        assert!(
            buf[mid] > 0,
            "expected seam pixel alpha > 0 after blur, got {}",
            buf[mid]
        );
        assert_ne!(buf, before, "blur with sigma=1.5 was a no-op");
    }

    #[test]
    fn be_post_step_no_ops_on_zero_strength() {
        // strength = 0 must leave the buffer untouched.
        let mut buf = vec![0u8; 4 * 4 * 4];
        for (i, b) in buf.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7);
        }
        let before = buf.clone();
        super::apply_be_post(&mut buf, 4, 4, 0);
        assert_eq!(buf, before, "\\be0 post-step mutated the buffer");
    }

    #[test]
    fn be_post_step_no_ops_on_zero_canvas() {
        // 0×W or H×0 — the helper must early-out even with a non-zero
        // strength.
        let mut buf = vec![0u8; 16];
        let before = buf.clone();
        super::apply_be_post(&mut buf, 0, 0, 5);
        assert_eq!(buf, before, "\\be on a 0×0 canvas mutated the buffer");
    }

    #[test]
    fn be_post_step_short_buffer_is_no_op() {
        // Buffer shorter than width * height * 4 — defensive guard.
        let mut buf = vec![0u8; 8]; // way smaller than 4*4*4 = 64
        let before = buf.clone();
        super::apply_be_post(&mut buf, 4, 4, 3);
        assert_eq!(buf, before, "\\be touched a too-short buffer");
    }

    #[test]
    fn be_post_step_preserves_constant_canvas() {
        // A canvas of a single uniform colour must be a fixed point of
        // the box pass: every 3-tap window samples the same value, so
        // the rounded mean is exactly that value. Confirms the +1
        // bias in `(a+b+c+1)/3` keeps a constant patch invariant.
        let mut buf = vec![0u8; 8 * 6 * 4];
        for px in buf.chunks_exact_mut(4) {
            px[0] = 200;
            px[1] = 100;
            px[2] = 50;
            px[3] = 255;
        }
        let before = buf.clone();
        super::apply_be_post(&mut buf, 8, 6, 4);
        assert_eq!(buf, before, "\\be eroded a constant canvas");
    }

    #[test]
    fn be_post_step_softens_a_hard_edge() {
        // Construct a 16×8 RGBA buffer with a vertical hard edge —
        // left half opaque white, right half fully transparent. After
        // one \be iteration the alpha column on the right side of the
        // seam must pick up some alpha (the pass averages a 3-pixel
        // window, two of which are 0 alpha and one of which is 255 →
        // ~85 alpha).
        let w = 16u32;
        let h = 8u32;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                if x < w / 2 {
                    buf[i] = 255;
                    buf[i + 1] = 255;
                    buf[i + 2] = 255;
                    buf[i + 3] = 255;
                }
                // right half stays at zeros.
            }
        }
        let before = buf.clone();
        super::apply_be_post(&mut buf, w, h, 1);
        // The first transparent column (x = w/2) now sees the
        // last opaque column as one of its three neighbours and
        // picks up alpha; pin it to "strictly greater than zero".
        let seam = ((3 * w + w / 2) * 4 + 3) as usize;
        assert!(
            buf[seam] > 0,
            "expected seam pixel alpha > 0 after \\be1, got {}",
            buf[seam]
        );
        assert_ne!(buf, before, "\\be1 was a no-op");
    }

    #[test]
    fn be_post_step_more_iterations_spread_alpha_further() {
        // Two iterations on the same vertical-edge canvas must spread
        // the alpha at least one column further than a single
        // iteration — the 3-tap pass has a 1-pixel radius per
        // iteration, so N iterations have an N-pixel radius of
        // influence (plus the small +1 rounding leak). Pin that
        // monotonicity so a future regression that, e.g., copies
        // scratch back to buf at the wrong stride is caught.
        let w = 24u32;
        let h = 4u32;
        let make = || {
            let mut b = vec![0u8; (w * h * 4) as usize];
            for y in 0..h {
                for x in 0..w {
                    let i = ((y * w + x) * 4) as usize;
                    if x < w / 2 {
                        b[i] = 255;
                        b[i + 1] = 255;
                        b[i + 2] = 255;
                        b[i + 3] = 255;
                    }
                }
            }
            b
        };
        let mut one = make();
        let mut two = make();
        super::apply_be_post(&mut one, w, h, 1);
        super::apply_be_post(&mut two, w, h, 2);
        let count = |buf: &[u8]| -> u32 {
            (0..w)
                .filter(|x| {
                    let i = ((w + x) * 4 + 3) as usize;
                    buf[i] > 0
                })
                .count() as u32
        };
        let c1 = count(&one);
        let c2 = count(&two);
        assert!(
            c2 > c1,
            "expected more iterations to spread alpha further: c1={c1} c2={c2}"
        );
    }

    #[test]
    fn numpad_to_align_decomposes_rows_and_columns() {
        // Bottom row.
        assert_eq!(numpad_to_align(1), (TextAlign::Left, VerticalRow::Bottom));
        assert_eq!(numpad_to_align(2), (TextAlign::Center, VerticalRow::Bottom));
        assert_eq!(numpad_to_align(3), (TextAlign::Right, VerticalRow::Bottom));
        // Middle row.
        assert_eq!(numpad_to_align(4), (TextAlign::Left, VerticalRow::Middle));
        assert_eq!(numpad_to_align(5), (TextAlign::Center, VerticalRow::Middle));
        assert_eq!(numpad_to_align(6), (TextAlign::Right, VerticalRow::Middle));
        // Top row.
        assert_eq!(numpad_to_align(7), (TextAlign::Left, VerticalRow::Top));
        assert_eq!(numpad_to_align(8), (TextAlign::Center, VerticalRow::Top));
        assert_eq!(numpad_to_align(9), (TextAlign::Right, VerticalRow::Top));
    }

    #[test]
    fn inverse_rect_path_has_two_subpaths_and_ten_commands() {
        // Outer ring (move + 3 line + close = 5) + inner ring (same
        // shape = 5) = 10 commands.
        let r = ClipRect {
            x1: 10.0,
            y1: 20.0,
            x2: 50.0,
            y2: 60.0,
        };
        let p = super::inverse_rect_path(320.0, 200.0, &r);
        assert_eq!(p.commands.len(), 10);
        // First command is the outer ring's MoveTo, well past the
        // canvas's negative corner.
        match p.commands[0] {
            oxideav_core::PathCommand::MoveTo(pt) => {
                assert!(pt.x < 0.0 && pt.y < 0.0, "got ({}, {})", pt.x, pt.y);
            }
            _ => panic!("expected MoveTo at index 0"),
        }
        // Sixth command is the inner ring's MoveTo, at (x1, y1).
        match p.commands[5] {
            oxideav_core::PathCommand::MoveTo(pt) => {
                assert!((pt.x - 10.0).abs() < 1e-4 && (pt.y - 20.0).abs() < 1e-4);
            }
            _ => panic!("expected MoveTo at index 5"),
        }
    }

    #[test]
    fn inverse_rect_outer_extents_cover_double_canvas() {
        let (ox1, oy1, ox2, oy2) = super::inverse_outer_extents(320.0, 200.0);
        assert!((ox1 + 320.0).abs() < 1e-4, "got ox1={ox1}");
        assert!((oy1 + 200.0).abs() < 1e-4, "got oy1={oy1}");
        assert!((ox2 - 640.0).abs() < 1e-4, "got ox2={ox2}");
        assert!((oy2 - 400.0).abs() < 1e-4, "got oy2={oy2}");
    }

    #[test]
    fn inverse_rect_outer_extents_handle_zero_canvas() {
        // A 0×0 canvas degrades to a 1-unit fallback so the rasteriser
        // still gets a non-empty outer ring.
        let (ox1, oy1, ox2, oy2) = super::inverse_outer_extents(0.0, 0.0);
        assert!((ox1 + 1.0).abs() < 1e-4 && (oy1 + 1.0).abs() < 1e-4);
        assert!((ox2 - 2.0).abs() < 1e-4 && (oy2 - 2.0).abs() < 1e-4);
    }

    #[test]
    fn inverse_rect_inner_winding_is_reverse_of_rect_to_path() {
        // `rect_to_path` walks (x1,y1) → (x2,y1) → (x2,y2) → (x1,y2) →
        // close (clockwise in screen-space, the "fill" direction under
        // NonZero). The inverse builder's inner ring must walk the
        // opposite order so its winding cancels the outer ring's
        // inside the cut-out, leaving zero winding (no fill) there.
        let r = ClipRect {
            x1: 5.0,
            y1: 5.0,
            x2: 15.0,
            y2: 15.0,
        };
        let p = super::inverse_rect_path(100.0, 100.0, &r);
        // Inner ring is commands 5..10.
        let inner = &p.commands[5..10];
        let pts: Vec<(f32, f32)> = inner
            .iter()
            .filter_map(|c| match c {
                oxideav_core::PathCommand::MoveTo(pt) | oxideav_core::PathCommand::LineTo(pt) => {
                    Some((pt.x, pt.y))
                }
                _ => None,
            })
            .collect();
        // Inner walks (x1,y1) → (x1,y2) → (x2,y2) → (x2,y1) — reverse
        // of `rect_to_path`'s order.
        assert_eq!(
            pts,
            vec![(5.0, 5.0), (5.0, 15.0), (15.0, 15.0), (15.0, 5.0)]
        );
    }

    #[test]
    fn inverse_path_from_inner_starts_with_outer_ring() {
        // Feed in a simple triangle; the inverse path must begin with
        // the 5-command outer ring (move + 3 lines + close) followed
        // by the reversed-traversal inner.
        let mut tri = Path::new();
        tri.move_to(Point::new(0.0, 0.0));
        tri.line_to(Point::new(10.0, 0.0));
        tri.line_to(Point::new(5.0, 10.0));
        tri.close();

        let p = super::inverse_path_from_inner(100.0, 100.0, &tri);
        // Outer ring first (5 commands), then reversed triangle (5
        // commands too: move + 2 lines + close = 4 from the
        // reversed-line-and-close path, plus 1 for the kept Close).
        assert!(p.commands.len() >= 5);
        match p.commands[0] {
            oxideav_core::PathCommand::MoveTo(pt) => {
                assert!(pt.x < 0.0 && pt.y < 0.0);
            }
            _ => panic!("expected outer-ring MoveTo at index 0"),
        }
    }

    #[test]
    fn reversed_path_commands_flips_triangle_traversal() {
        // The reversed-traversal helper walks each subpath in the
        // opposite direction. For a triangle (0,0) → (10,0) → (5,10)
        // → close, the reverse starts at the last vertex and walks
        // back through the others.
        let mut tri = Path::new();
        tri.move_to(Point::new(0.0, 0.0));
        tri.line_to(Point::new(10.0, 0.0));
        tri.line_to(Point::new(5.0, 10.0));
        tri.close();
        let rev = super::reversed_path_commands(&tri);
        let pts: Vec<(f32, f32)> =
            rev.iter()
                .filter_map(|c| match c {
                    oxideav_core::PathCommand::MoveTo(pt)
                    | oxideav_core::PathCommand::LineTo(pt) => Some((pt.x, pt.y)),
                    _ => None,
                })
                .collect();
        // Reversed: start at (5,10), walk back through (10,0) and
        // (0,0).
        assert_eq!(pts, vec![(5.0, 10.0), (10.0, 0.0), (0.0, 0.0)]);
        // The trailing Close must survive the reversal.
        assert!(matches!(rev.last(), Some(oxideav_core::PathCommand::Close)));
    }

    #[test]
    fn reversed_path_commands_preserves_subpath_count() {
        // Two disjoint subpaths should both reverse independently.
        let mut p = Path::new();
        p.move_to(Point::new(0.0, 0.0));
        p.line_to(Point::new(1.0, 0.0));
        p.close();
        p.move_to(Point::new(5.0, 5.0));
        p.line_to(Point::new(6.0, 5.0));
        p.close();
        let rev = super::reversed_path_commands(&p);
        let move_count = rev
            .iter()
            .filter(|c| matches!(c, oxideav_core::PathCommand::MoveTo(_)))
            .count();
        let close_count = rev
            .iter()
            .filter(|c| matches!(c, oxideav_core::PathCommand::Close))
            .count();
        assert_eq!(move_count, 2);
        assert_eq!(close_count, 2);
    }
}
