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
//! `\clip` overrides at successive moments in the cue's lifetime.
//!
//! Pipeline (per `receive_frame`):
//!
//! 1. Pull the next [`SubtitleCue`] from the wrapped inner decoder.
//! 2. [`crate::extract_cue_animation`] the cue.
//! 3. Sample the animation at `(cue.start + eval_offset_ms)` clamped
//!    into the cue's `[start, end]` lifetime.
//! 4. Build a [`oxideav_core::VectorFrame`] containing the cue's
//!    shaped glyph nodes (via the supplied [`oxideav_scribe::FaceChain`])
//!    placed line-by-line, then wrap them in a `Group` whose:
//!    - `transform` composes the animation's `move` ∘ pivoted
//!      `\frx`/`\fry`/`\frz` ∘ `\fscx`/`\fscy` matrix (3D rotations
//!      reduced to a 2D affine via a small-angle approximation around
//!      the pivot, so the renderer stays purely 2D);
//!    - `opacity` is `RenderState::alpha_mul`;
//!    - `clip` is the `\clip(rect)` rectangle path or, if
//!      `\clip(drawing)` is active, the drawing-path parsed by
//!      [`crate::drawing::parse_drawing`].
//! 5. Rasterise via [`oxideav_raster::Renderer`].
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

        // Default alignment.
        let align = cue
            .positioning
            .as_ref()
            .map(|p| p.align)
            .unwrap_or(TextAlign::Center);

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
        let mut visual_lines: Vec<String> = Vec::new();
        for line in &logical_lines {
            for v in wrap_line(line, face, size_px, max_text_w as f32) {
                visual_lines.push(v);
            }
        }
        if visual_lines.is_empty() {
            return wrap_buf(buf, self.width, cue.start_us);
        }
        // Layout vertical: stack from bottom up using face metrics.
        let face_line_h = face.primary().line_height_px(size_px).ceil().max(1.0) as u32;
        let face_descent_abs = (-face.primary().descent_px(size_px)).ceil().max(0.0) as u32;
        let line_h = face_line_h.max(1);
        let n_lines = visual_lines.len();
        let last_baseline = self
            .height
            .saturating_sub(self.bottom_margin_px)
            .saturating_sub(face_descent_abs);

        // Assemble per-glyph nodes inside an inner Group at canvas coords.
        let mut inner = Group::default();
        let mut anchor_x = self.width as f32 / 2.0;
        let anchor_y = last_baseline as f32;
        let primary_color = state
            .primary_color
            .map(|(r, g, b)| [r, g, b, 255])
            .unwrap_or(self.default_color);
        for (i, line) in visual_lines.iter().enumerate() {
            let line_w_px = measure(face, line, size_px);
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
            let glyphs = Shaper::shape_to_paths(face, line, size_px);
            let fill = Paint::Solid(rgba_to_core(primary_color));
            for (_face_idx, node, glyph_xform) in glyphs {
                let absolute = Transform2D::translate(pen_x, baseline_y).compose(&glyph_xform);
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
        // \org-supplied pivot).
        let pivot = state.pivot.unwrap_or((anchor_x, last_baseline as f32));
        let anim_xf = animation_transform(state, pivot);

        // Optional clip: prefer drawing-path over rect when both set.
        let clip_path = if let Some(s) = state.clip_drawing.as_ref() {
            let (scale, body) = drawing::split_clip_arg(s);
            Some(drawing::parse_drawing(body, scale))
        } else {
            state.clip_rect.as_ref().map(rect_to_path)
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
        wrap_buf(buf, self.width, cue.start_us)
    }
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

/// Greedy word-wrap by shaped width. Returns visual lines.
fn wrap_line(line: &str, face: &FaceChain, size_px: f32, max_w: f32) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }
    if measure(face, line, size_px) <= max_w {
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
        if measure(face, &candidate, size_px) <= max_w {
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
/// translate / scale / 3D rotations around `pivot`.
///
/// The 2D affine pipeline we apply (right-to-left) is:
///
/// 1. translate(-pivot)
/// 2. scale(sx, sy)
/// 3. shear/squeeze approximating `\fry` (X scale by cos α_y) and
///    `\frx` (Y scale by cos α_x). True 3D would project onto a
///    perspective camera; here we use the small-angle / orthographic
///    approximation: the visible width shrinks by `cos(α_y)` for a
///    rotation around Y and the visible height by `cos(α_x)` for a
///    rotation around X. This is the standard "fold in half" effect
///    most ASS renderers fall back on when no perspective camera is
///    configured.
/// 4. rotate(α_z) (`\frz`)
/// 5. translate(+pivot)
/// 6. translate(extra_translate) when `\pos` / `\move` set one.
fn animation_transform(state: &RenderState, pivot: (f32, f32)) -> Transform2D {
    let (px, py) = pivot;
    let mut t = Transform2D::translate(-px, -py);
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
        let t = animation_transform(&st, (10.0, 10.0));
        let p = t.apply(Point::new(10.0, 10.0));
        assert!((p.x - 10.0).abs() < 1e-4);
        assert!((p.y - 10.0).abs() < 1e-4);
    }

    #[test]
    fn frx_compresses_y() {
        // 60° \frx → cos(60°) = 0.5: y distances around pivot halve.
        let mut st = RenderState::identity();
        st.rotate_x_radians = std::f32::consts::FRAC_PI_3;
        let t = animation_transform(&st, (0.0, 0.0));
        let p = t.apply(Point::new(0.0, 100.0));
        assert!((p.y - 50.0).abs() < 1e-3, "got y={}", p.y);
    }

    #[test]
    fn fry_compresses_x() {
        let mut st = RenderState::identity();
        st.rotate_y_radians = std::f32::consts::FRAC_PI_3;
        let t = animation_transform(&st, (0.0, 0.0));
        let p = t.apply(Point::new(100.0, 0.0));
        assert!((p.x - 50.0).abs() < 1e-3, "got x={}", p.x);
    }

    #[test]
    fn org_overrides_anchor_pivot() {
        let mut st = RenderState::identity();
        st.rotate_radians = std::f32::consts::FRAC_PI_2;
        st.pivot = Some((100.0, 100.0));
        let t = animation_transform(&st, st.pivot.unwrap());
        let p = t.apply(Point::new(100.0, 100.0));
        assert!((p.x - 100.0).abs() < 1e-4);
        assert!((p.y - 100.0).abs() < 1e-4);
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
}
