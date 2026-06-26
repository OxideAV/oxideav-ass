//! `[Script Info]` `Collisions` reposition resolver.
//!
//! The SSA v4.x spec (`docs/subtitles/ass/`) documents the `Collisions`
//! header as the policy a renderer uses to keep overlapping bottom-
//! anchored subtitle lines from covering each other:
//!
//! * **`Normal`** — the spec describes the lines as stacking up one
//!   above the other while staying positioned as close to the vertical
//!   (bottom) margin as possible, filling in gaps in other subtitles
//!   when one large enough is available. Each line wants the bottom;
//!   when it overlaps in time with already-placed lines it takes the
//!   lowest free vertical slot, so a later line stacks *above* the
//!   earlier ones and an expiring earlier line frees its slot for a
//!   newcomer to fall back into.
//! * **`Reverse`** — the spec describes the lines as shifted upwards to
//!   make room for subsequent overlapping subtitles, so they can nearly
//!   always be read top-down. The newest overlapping line takes the
//!   bottom slot and the earlier lines it overlaps are pushed up above
//!   it, so a block of simultaneous lines reads in event order from top
//!   to bottom.
//!
//! Both policies only move bottom-aligned lines that carry no explicit
//! `\pos` / `\move` / non-bottom `\an` placement (an explicitly
//! positioned line opts out of collision handling). This module resolves
//! the vertical band for a set of such lines; the caller supplies the
//! per-line time interval + measured pixel height and the canvas
//! geometry, and gets back the resolved top-left Y of each line's box in
//! event order. Horizontal placement, alignment, and the non-bottom rows
//! are the caller's concern — collision repositioning is purely vertical
//! per the spec.

use crate::script_info::Collisions;

/// One bottom-anchored subtitle line competing for vertical space.
///
/// `start_us` / `end_us` are the line's on-screen interval in
/// microseconds (the same units as [`oxideav_core::SubtitleCue`]); a
/// half-open `[start, end)` overlap test decides which lines collide.
/// `height_px` is the line's measured box height in canvas pixels (after
/// word-wrap, so a two-row line is twice a one-row line).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionBox {
    /// Line on-screen start, microseconds.
    pub start_us: i64,
    /// Line on-screen end, microseconds.
    pub end_us: i64,
    /// Measured box height in canvas pixels.
    pub height_px: u32,
}

impl CollisionBox {
    /// Whether this line is on screen at the same time as `other` (a
    /// half-open `[start, end)` interval overlap). Two lines that merely
    /// touch at an instant (one ends exactly when the other starts) do
    /// **not** collide.
    #[inline]
    pub fn overlaps(&self, other: &CollisionBox) -> bool {
        self.start_us < other.end_us && other.start_us < self.end_us
    }
}

/// The geometry the resolver places lines into.
///
/// `height_px` is the canvas (script-resolution) height. `bottom_margin_px`
/// is the gap between the canvas bottom and the lowest line's bottom edge
/// — the line's resting place when nothing collides. `top_margin_px`
/// caps how far up lines may be pushed; a line that would cross it is
/// clamped (the spec notes `Reverse` "can use a lot of screen area", but
/// a renderer cannot draw off-canvas).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasGeometry {
    /// Canvas height in pixels.
    pub height_px: u32,
    /// Bottom margin in pixels.
    pub bottom_margin_px: u32,
    /// Top margin in pixels (the clamp ceiling).
    pub top_margin_px: u32,
}

/// Resolve the vertical layout of `boxes` under `policy`.
///
/// Returns one resolved top-left Y per input box, **in the same order**.
/// A box that does not overlap any other simply rests against the bottom
/// margin. Overlapping boxes are stacked per the policy:
///
/// * [`Collisions::Normal`] — each box (in event order) takes the lowest
///   free vertical slot above any earlier still-on-screen box, so newer
///   lines pile upward and an expired line's slot is reused.
/// * [`Collisions::Reverse`] — the boxes of each overlapping group are
///   ordered with the latest-appearing at the bottom and earlier ones
///   above it, so the group reads top-down in event order.
///
/// The resolver is total and allocation-light: it is `O(n²)` in the
/// number of boxes (one overlap scan per box), which is ample for the
/// handful of simultaneous lines a real script ever stacks. Heights that
/// would push a box past `top_margin_px` clamp it at the top margin
/// rather than drawing off-canvas.
pub fn resolve_layout(
    boxes: &[CollisionBox],
    geometry: CanvasGeometry,
    policy: Collisions,
) -> Vec<u32> {
    match policy {
        Collisions::Normal => resolve_normal(boxes, geometry),
        Collisions::Reverse => resolve_reverse(boxes, geometry),
    }
}

/// Top-of-stack ceiling in pixels: lines may not be pushed above this.
#[inline]
fn ceiling(geometry: CanvasGeometry) -> i64 {
    geometry.top_margin_px as i64
}

/// Bottom resting baseline (top-left Y of a single line sitting against
/// the bottom margin) for a box of `height_px`.
#[inline]
fn bottom_rest(geometry: CanvasGeometry, height_px: u32) -> i64 {
    geometry.height_px as i64 - geometry.bottom_margin_px as i64 - height_px as i64
}

/// Clamp a top-left Y into `[ceiling, bottom_rest_of_zero_height]`.
#[inline]
fn clamp_top(geometry: CanvasGeometry, top: i64) -> u32 {
    let max_top = geometry.height_px as i64 - geometry.bottom_margin_px as i64;
    top.clamp(ceiling(geometry), max_top.max(0)) as u32
}

fn resolve_normal(boxes: &[CollisionBox], geometry: CanvasGeometry) -> Vec<u32> {
    // For each box in event order, find the lowest non-overlapping
    // vertical slot. We track the placed boxes' (interval, top, bottom)
    // so a newcomer scans upward from the bottom margin until it finds a
    // gap that clears every time-overlapping placed box.
    let mut placed: Vec<PlacedBox> = Vec::with_capacity(boxes.len());
    let mut out = Vec::with_capacity(boxes.len());
    for b in boxes {
        // Candidate bottom edge starts at the bottom-margin resting spot.
        let mut top = bottom_rest(geometry, b.height_px);
        // Repeatedly raise above any time-overlapping placed box whose
        // band intersects the candidate band, restarting the scan after
        // each lift (a higher placement may now clash with a different
        // box). Bounded by the placed count.
        loop {
            let mut bumped = false;
            for p in &placed {
                if !time_overlap(b, p) {
                    continue;
                }
                let cand_bottom = top + b.height_px as i64;
                // Bands intersect when neither is wholly above the other.
                if top < p.bottom && p.top < cand_bottom {
                    // Lift this box to sit directly above the placed one.
                    top = p.top - b.height_px as i64;
                    bumped = true;
                }
            }
            if !bumped {
                break;
            }
            if top <= ceiling(geometry) {
                top = ceiling(geometry);
                break;
            }
        }
        let placed_top = top.max(ceiling(geometry));
        placed.push(PlacedBox {
            start_us: b.start_us,
            end_us: b.end_us,
            top: placed_top,
            bottom: placed_top + b.height_px as i64,
        });
        out.push(clamp_top(geometry, placed_top));
    }
    out
}

fn resolve_reverse(boxes: &[CollisionBox], geometry: CanvasGeometry) -> Vec<u32> {
    // Reverse: the latest box of an overlapping run sits at the bottom and
    // earlier ones stack above it. We group boxes into maximal time-
    // connected runs, place the *last* box of each run at the bottom
    // margin, then walk backward through the run stacking each earlier
    // box on top of the one below it.
    let mut out = vec![0u32; boxes.len()];
    let mut idx = 0usize;
    while idx < boxes.len() {
        // Grow a run of indices that are pairwise time-connected with the
        // run so far (transitive overlap chain in event order).
        let mut run = vec![idx];
        let mut run_end = boxes[idx].end_us;
        let mut j = idx + 1;
        while j < boxes.len() && boxes[j].start_us < run_end {
            run.push(j);
            run_end = run_end.max(boxes[j].end_us);
            j += 1;
        }
        // Bottom slot holds the last (latest-appearing) box; walk up.
        let mut bottom_edge = geometry.height_px as i64 - geometry.bottom_margin_px as i64;
        for &k in run.iter().rev() {
            let h = boxes[k].height_px as i64;
            let top = (bottom_edge - h).max(ceiling(geometry));
            out[k] = clamp_top(geometry, top);
            bottom_edge = top;
        }
        idx = j;
    }
    out
}

/// A placed box plus its resolved vertical band, used by the Normal scan.
struct PlacedBox {
    start_us: i64,
    end_us: i64,
    top: i64,
    bottom: i64,
}

#[inline]
fn time_overlap(b: &CollisionBox, p: &PlacedBox) -> bool {
    b.start_us < p.end_us && p.start_us < b.end_us
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEO: CanvasGeometry = CanvasGeometry {
        height_px: 480,
        bottom_margin_px: 20,
        top_margin_px: 0,
    };

    fn b(start: i64, end: i64, h: u32) -> CollisionBox {
        CollisionBox {
            start_us: start,
            end_us: end,
            height_px: h,
        }
    }

    #[test]
    fn overlaps_is_half_open() {
        let a = b(0, 100, 10);
        // Touching at the boundary does not collide.
        assert!(!a.overlaps(&b(100, 200, 10)));
        assert!(!b(100, 200, 10).overlaps(&a));
        // Genuine overlap does.
        assert!(a.overlaps(&b(50, 150, 10)));
    }

    #[test]
    fn single_box_rests_against_bottom_margin() {
        let out = resolve_layout(&[b(0, 100, 30)], GEO, Collisions::Normal);
        // 480 - 20 - 30 = 430.
        assert_eq!(out, vec![430]);
    }

    #[test]
    fn non_overlapping_boxes_both_rest_at_bottom() {
        let out = resolve_layout(&[b(0, 100, 30), b(100, 200, 40)], GEO, Collisions::Normal);
        assert_eq!(out, vec![430, 480 - 20 - 40]);
    }

    #[test]
    fn normal_stacks_later_line_above_earlier() {
        // Two overlapping lines: the first rests at the bottom, the
        // second (later, overlapping) stacks directly above it.
        let out = resolve_layout(&[b(0, 200, 30), b(100, 300, 30)], GEO, Collisions::Normal);
        // First: 480 - 20 - 30 = 430. Second sits above: 430 - 30 = 400.
        assert_eq!(out, vec![430, 400]);
    }

    #[test]
    fn normal_fills_gap_when_earlier_line_expired() {
        // Line A overlaps B (B stacks above A). Line C starts after A has
        // expired but still overlaps B — C should fall back into A's
        // freed bottom slot rather than stacking a third level up.
        let out = resolve_layout(
            &[
                b(0, 100, 30),   // A: bottom
                b(50, 300, 30),  // B: above A
                b(150, 400, 30), // C: A expired by 150, falls to bottom
            ],
            GEO,
            Collisions::Normal,
        );
        assert_eq!(out[0], 430); // A bottom
        assert_eq!(out[1], 400); // B above A
        assert_eq!(out[2], 430); // C reuses A's freed bottom slot
    }

    #[test]
    fn normal_three_way_overlap_stacks_three_high() {
        let out = resolve_layout(
            &[b(0, 400, 30), b(10, 400, 30), b(20, 400, 30)],
            GEO,
            Collisions::Normal,
        );
        assert_eq!(out, vec![430, 400, 370]);
    }

    #[test]
    fn reverse_puts_latest_at_bottom_earlier_above() {
        // Reverse: the later line takes the bottom, the earlier one is
        // pushed up above it (so the pair reads top-down in event order).
        let out = resolve_layout(&[b(0, 300, 30), b(100, 300, 30)], GEO, Collisions::Reverse);
        // Second (latest) at bottom 430; first pushed above to 400.
        assert_eq!(out, vec![400, 430]);
    }

    #[test]
    fn reverse_three_way_reads_top_down() {
        let out = resolve_layout(
            &[b(0, 400, 30), b(10, 400, 30), b(20, 400, 30)],
            GEO,
            Collisions::Reverse,
        );
        // Latest (index 2) at bottom; index 0 highest.
        assert_eq!(out, vec![370, 400, 430]);
    }

    #[test]
    fn reverse_separate_runs_each_anchor_bottom() {
        // Two disjoint overlapping runs: each independently anchors its
        // latest line at the bottom margin.
        let out = resolve_layout(
            &[
                b(0, 100, 30),
                b(50, 100, 30), // run 1
                b(200, 300, 30),
                b(250, 300, 30), // run 2
            ],
            GEO,
            Collisions::Reverse,
        );
        // Run 1: idx1 bottom (430), idx0 above (400).
        assert_eq!((out[0], out[1]), (400, 430));
        // Run 2: idx3 bottom (430), idx2 above (400).
        assert_eq!((out[2], out[3]), (400, 430));
    }

    #[test]
    fn clamps_at_top_margin_when_stack_too_tall() {
        // A stack taller than the canvas clamps the top line at the top
        // margin rather than going negative / off-canvas.
        let geo = CanvasGeometry {
            height_px: 100,
            bottom_margin_px: 10,
            top_margin_px: 5,
        };
        let out = resolve_layout(
            &[b(0, 400, 40), b(0, 400, 40), b(0, 400, 40)],
            geo,
            Collisions::Normal,
        );
        // Bottom box: 100 - 10 - 40 = 50. Next: 10. Third would be -30 →
        // clamped to the top margin 5.
        assert_eq!(out, vec![50, 10, 5]);
    }

    #[test]
    fn empty_input_yields_empty_output() {
        assert!(resolve_layout(&[], GEO, Collisions::Normal).is_empty());
        assert!(resolve_layout(&[], GEO, Collisions::Reverse).is_empty());
    }

    #[test]
    fn two_byte_height_boxes_stack_by_measured_height() {
        // A two-row line (taller box) and a one-row line overlapping: the
        // taller one resting at bottom pushes the later short one up by
        // its own (short) height above the tall box's top.
        let out = resolve_layout(&[b(0, 300, 60), b(100, 300, 30)], GEO, Collisions::Normal);
        // Tall box bottom: 480 - 20 - 60 = 400. Short box above: 400 - 30 = 370.
        assert_eq!(out, vec![400, 370]);
    }
}
