//! The map furniture: page layout, round grid steps, scale bars and the
//! greedy label placer.

use super::canvas::{Rgba8, TextSize};
use super::geometry::Transform;

/// Round world-space steps a grid or scale bar may use.
const NICE_STEPS: [f32; 10] = [0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0];
const BAR_STEPS: [f32; 7] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0];

/// Page furniture sizes, in pixels.
pub(crate) const MARGIN: f32 = 8.0;
pub(crate) const TITLE_H: f32 = 36.0;
pub(crate) const AXIS_L: f32 = 56.0;
pub(crate) const AXIS_B: f32 = 32.0;
pub(crate) const LEGEND_W: f32 = 190.0;
pub(crate) const CAPTION_LINE_H: f32 = 14.0;
/// Height of one legend row.
pub(crate) const LEGEND_ROW_H: f32 = 18.0;
/// A grid line every `nice_step` that is at least this far apart on the page.
pub(crate) const MIN_GRID_GAP: f32 = 50.0;

/// The smallest round step whose on-page spacing is at least `min_gap_px`.
pub fn nice_step(px_per_m: f32, min_gap_px: f32) -> f32 {
    NICE_STEPS
        .into_iter()
        .find(|s| s * px_per_m >= min_gap_px)
        .unwrap_or(NICE_STEPS[NICE_STEPS.len() - 1])
}

/// The largest round scale-bar length that fits in a quarter of the map width.
pub fn scale_bar_metres(map_width_m: f32) -> f32 {
    let quarter = map_width_m / 4.0;
    BAR_STEPS.into_iter().filter(|s| *s <= quarter).last().unwrap_or(BAR_STEPS[0])
}

/// An axis-aligned page rectangle.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Rect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

impl Rect {
    pub(crate) fn contains(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        x >= self.x && y >= self.y && x + w <= self.x + self.w && y + h <= self.y + self.h
    }

    pub(crate) fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}

/// Where everything sits on the page.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Layout {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) map: Rect,
    pub(crate) transform: Transform,
    /// Left edge of the legend column (equal to the page width when there is none).
    pub(crate) legend_x: f32,
    /// Top of the first caption line.
    pub(crate) caption_y: f32,
}

/// Sizes the page around a map of `region` at `scale` pixels per metre.
pub(crate) fn layout(region: [f32; 4], scale: f32, legend_rows: usize, caption_lines: usize) -> Layout {
    let map_w = ((region[2] - region[0]) * scale).round().max(1.0);
    let map_h = ((region[3] - region[1]) * scale).round().max(1.0);
    let legend_w = if legend_rows > 0 { LEGEND_W } else { 0.0 };
    let map = Rect { x: MARGIN + AXIS_L, y: MARGIN + TITLE_H, w: map_w, h: map_h };
    let width = map.x + map_w + legend_w + MARGIN;
    let height = map.y + map_h + AXIS_B + caption_lines as f32 * CAPTION_LINE_H + MARGIN;
    Layout {
        width: width.ceil() as u32,
        height: height.ceil() as u32,
        map,
        transform: Transform { x0: region[0], y1: region[3], scale, ox: map.x, oy: map.y },
        legend_x: map.x + map_w + MARGIN,
        caption_y: map.y + map_h + AXIS_B,
    }
}

/// A label asking to be drawn somewhere near `anchor`.
#[derive(Debug, Clone)]
pub(crate) struct LabelRequest {
    pub(crate) anchor: (f32, f32),
    pub(crate) text: String,
    pub(crate) size: TextSize,
    pub(crate) color: Rgba8,
    /// Lower is placed first, and so wins contested space.
    pub(crate) priority: u8,
    /// Try centring the label on the anchor before the eight offsets.
    pub(crate) centred_first: bool,
}

/// A label with a page position it may be drawn at.
#[derive(Debug, Clone)]
pub(crate) struct PlacedLabel {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) text: String,
    pub(crate) size: TextSize,
    pub(crate) color: Rgba8,
}

/// Places as many labels as fit inside `map` without overlapping, highest
/// priority first. Returns the placements and how many were dropped.
pub(crate) fn place_labels(
    reqs: &mut Vec<LabelRequest>,
    map: Rect,
    measure: impl Fn(&str, TextSize) -> (f32, f32),
) -> (Vec<PlacedLabel>, usize) {
    const GAP: f32 = 6.0;
    const PAD: f32 = 2.0;

    reqs.sort_by_key(|r| r.priority);
    let mut taken: Vec<Rect> = Vec::with_capacity(reqs.len());
    let mut placed = Vec::with_capacity(reqs.len());
    let mut dropped = 0;

    for req in reqs.iter() {
        let (w, h) = measure(&req.text, req.size);
        let (ax, ay) = req.anchor;
        let mut candidates: Vec<(f32, f32)> = Vec::with_capacity(9);
        if req.centred_first {
            candidates.push((ax - w / 2.0, ay - h / 2.0));
        }
        candidates.extend([
            (ax + GAP, ay - h / 2.0),               // right
            (ax - GAP - w, ay - h / 2.0),           // left
            (ax - w / 2.0, ay - GAP - h),           // above
            (ax - w / 2.0, ay + GAP),               // below
            (ax + GAP, ay + GAP),                   // below-right
            (ax - GAP - w, ay + GAP),               // below-left
            (ax + GAP, ay - GAP - h),               // above-right
            (ax - GAP - w, ay - GAP - h),           // above-left
        ]);

        let spot = candidates.into_iter().find(|&(x, y)| {
            if !map.contains(x, y, w, h) {
                return false;
            }
            let box_ = Rect { x, y, w, h };
            !taken.iter().any(|t| {
                Rect { x: t.x - PAD, y: t.y - PAD, w: t.w + 2.0 * PAD, h: t.h + 2.0 * PAD }.overlaps(&box_)
            })
        });

        match spot {
            Some((x, y)) => {
                taken.push(Rect { x, y, w, h });
                placed.push(PlacedLabel {
                    x,
                    y,
                    text: req.text.clone(),
                    size: req.size,
                    color: req.color,
                });
            }
            None => dropped += 1,
        }
    }
    (placed, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_steps_are_round_and_wide_enough() {
        assert_eq!(nice_step(30.0, 50.0), 2.0);
        assert_eq!(nice_step(10.0, 50.0), 5.0);
        assert_eq!(nice_step(3.0, 50.0), 20.0);
    }

    #[test]
    fn scale_bar_fits_a_quarter_of_the_map() {
        assert_eq!(scale_bar_metres(34.0), 5.0);
        assert_eq!(scale_bar_metres(400.0), 100.0);
        assert_eq!(scale_bar_metres(1.0), 1.0);
    }

    #[test]
    fn transform_maps_the_region_onto_the_map_area() {
        let l = layout([10.0, 20.0, 30.0, 50.0], 4.0, 0, 0);
        let (px, py) = l.transform.to_px(10.0, 50.0);
        assert!((px - l.map.x).abs() < 1e-3 && (py - l.map.y).abs() < 1e-3);
        let (px, py) = l.transform.to_px(30.0, 20.0);
        assert!((px - (l.map.x + l.map.w)).abs() < 1e-3);
        assert!((py - (l.map.y + l.map.h)).abs() < 1e-3);
        assert_eq!((l.map.w, l.map.h), (80.0, 120.0));
    }

    #[test]
    fn coincident_labels_do_not_overlap() {
        let map = Rect { x: 0.0, y: 0.0, w: 400.0, h: 400.0 };
        let mut reqs = vec![
            LabelRequest {
                anchor: (200.0, 200.0),
                text: "one".into(),
                size: TextSize::Small,
                color: [0, 0, 0, 255],
                priority: 1,
                centred_first: false,
            },
            LabelRequest {
                anchor: (200.0, 200.0),
                text: "two".into(),
                size: TextSize::Small,
                color: [0, 0, 0, 255],
                priority: 1,
                centred_first: false,
            },
        ];
        let (placed, dropped) = place_labels(&mut reqs, map, |s, size| {
            (super::super::canvas::measure(s, size), size.height())
        });
        assert_eq!(dropped, 0);
        assert_eq!(placed.len(), 2);
        let a = Rect { x: placed[0].x, y: placed[0].y, w: 17.0, h: 7.0 };
        let b = Rect { x: placed[1].x, y: placed[1].y, w: 17.0, h: 7.0 };
        assert!(!a.overlaps(&b), "labels overlap: {a:?} {b:?}");
    }
}
