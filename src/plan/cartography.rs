//! The map furniture: page layout, round grid steps, scale bars and the
//! greedy label placer.

use super::canvas::{Canvas, Rgba8, TextSize};
use super::palette;
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

/// One legend entry: a colour swatch and its caption.
pub(crate) struct LegendRow {
    pub(crate) swatch: Rgba8,
    pub(crate) text: String,
}

/// The grid or tick lines across `[lo, hi]`, as `(index, world)` pairs. The
/// count is settled up front in f64 and capped at the number of pixels the
/// axis has, so a region far from the origin — where consecutive world values
/// round to the same f32 — or one far wider than its page can neither stall
/// nor draw more lines than there are pixels to draw them on.
fn axis_lines(lo: f32, hi: f32, step: f32, axis_px: f32) -> Vec<(i64, f32)> {
    if !(step > 0.0) || !lo.is_finite() || !hi.is_finite() || hi < lo {
        return Vec::new();
    }
    let (lo, hi, step) = (lo as f64, hi as f64, step as f64);
    let k0 = (lo / step).ceil();
    let count = ((hi - k0 * step) / step).floor() + 1.0;
    let count = count.min(axis_px as f64).max(0.0) as i64;
    (0..count)
        .map(|i| ((k0 as i64).saturating_add(i), ((k0 + i as f64) * step) as f32))
        .collect()
}

/// The minor/major grid, drawn under everything else.
pub(crate) fn draw_grid(canvas: &mut dyn Canvas, l: &Layout, region: [f32; 4]) {
    let step = nice_step(l.transform.scale, MIN_GRID_GAP);
    let (map, t) = (l.map, l.transform);

    for (k, world) in axis_lines(region[0], region[2], step, map.w) {
        let x = t.to_px(world, region[3]).0;
        let colour = if k % 5 == 0 { palette::GRID_MAJOR } else { palette::GRID_MINOR };
        canvas.line((x, map.y), (x, map.y + map.h), colour, 1.0, None);
    }
    for (k, world) in axis_lines(region[1], region[3], step, map.h) {
        let y = t.to_px(region[0], world).1;
        let colour = if k % 5 == 0 { palette::GRID_MAJOR } else { palette::GRID_MINOR };
        canvas.line((map.x, y), (map.x + map.w, y), colour, 1.0, None);
    }
}

/// Formats a world coordinate for a tick label.
fn tick_text(v: f32) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

/// The map border and the world-coordinate ticks on the left and bottom axes.
pub(crate) fn draw_axes(canvas: &mut dyn Canvas, l: &Layout, region: [f32; 4]) {
    let step = nice_step(l.transform.scale, MIN_GRID_GAP) * 5.0;
    let (map, t) = (l.map, l.transform);
    canvas.rect(map.x, map.y, map.w, map.h, None, Some(palette::AXIS_TEXT));

    for (_, world) in axis_lines(region[0], region[2], step, map.w) {
        let x = t.to_px(world, region[3]).0;
        canvas.line((x, map.y + map.h), (x, map.y + map.h + 4.0), palette::AXIS_TEXT, 1.0, None);
        let text = tick_text(world);
        let w = canvas.text_width(&text, TextSize::Small);
        canvas.text(x - w / 2.0, map.y + map.h + 7.0, &text, TextSize::Small, palette::AXIS_TEXT, false);
    }
    for (_, world) in axis_lines(region[1], region[3], step, map.h) {
        let y = t.to_px(region[0], world).1;
        canvas.line((map.x - 4.0, y), (map.x, y), palette::AXIS_TEXT, 1.0, None);
        let text = tick_text(world);
        let w = canvas.text_width(&text, TextSize::Small);
        canvas.text(map.x - 7.0 - w, y - TextSize::Small.height() / 2.0, &text, TextSize::Small, palette::AXIS_TEXT, false);
    }
}

/// The scale bar, bottom-left inside the map area.
pub(crate) fn draw_scale_bar(canvas: &mut dyn Canvas, l: &Layout) {
    let map = l.map;
    let metres = scale_bar_metres(map.w / l.transform.scale);
    let bar = metres * l.transform.scale;
    let (x, y, h) = (map.x + 10.0, map.y + map.h - 20.0, 6.0);
    let seg = bar / 4.0;
    for i in 0..4 {
        let fill = if i % 2 == 0 { palette::INK } else { palette::BACKGROUND };
        canvas.rect(x + i as f32 * seg, y, seg, h, Some(fill), Some(palette::INK));
    }
    canvas.text(x, y - TextSize::Small.height() - 3.0, &format!("{} m", tick_text(metres)), TextSize::Small, palette::INK, true);
}

/// The north arrow, top-right inside the map area. World +Y is up on the page.
pub(crate) fn draw_north_arrow(canvas: &mut dyn Canvas, l: &Layout) {
    let map = l.map;
    let (cx, cy) = (map.x + map.w - 18.0, map.y + 16.0);
    canvas.fill_polygon(&[(cx, cy - 10.0), (cx - 6.0, cy + 8.0), (cx, cy + 4.0), (cx + 6.0, cy + 8.0)], palette::INK);
    let w = canvas.text_width("N", TextSize::Small);
    canvas.text(cx - w / 2.0, cy + 11.0, "N", TextSize::Small, palette::INK, true);
}

/// The legend column, one row per drawn layer and entity set.
pub(crate) fn draw_legend(canvas: &mut dyn Canvas, l: &Layout, rows: &[LegendRow]) {
    for (i, row) in rows.iter().enumerate() {
        let y = l.map.y + i as f32 * LEGEND_ROW_H;
        canvas.rect(l.legend_x, y + 1.0, 12.0, 12.0, Some(row.swatch), Some(palette::AXIS_TEXT));
        canvas.text(l.legend_x + 18.0, y + 4.0, &row.text, TextSize::Small, palette::INK, false);
    }
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
    fn axis_lines_are_bounded_by_the_page() {
        let lines = axis_lines(0.0, 10.0, 2.0, 300.0);
        assert_eq!(lines, vec![(0, 0.0), (1, 2.0), (2, 4.0), (3, 6.0), (4, 8.0), (5, 10.0)]);
        // A region far wider than its page draws at most one line per pixel.
        assert_eq!(axis_lines(0.0, 1e12, 500.0, 1000.0).len(), 1000);
        // Far from the origin f32 cannot even hold the ten-metre span, so one
        // line is all there is to draw — and, crucially, the count ends.
        assert_eq!(axis_lines(1e12, 1e12 + 10.0, 2.0, 300.0).len(), 1);
        assert!((1..=300).contains(&axis_lines(1e9, 1e9 + 500.0, 2.0, 300.0).len()));
        assert!(axis_lines(0.0, 10.0, 0.0, 300.0).is_empty());
        assert!(axis_lines(f32::NAN, 10.0, 2.0, 300.0).is_empty());
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
