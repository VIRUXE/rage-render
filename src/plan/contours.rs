//! Height contours from a regular grid of heights, by marching squares:
//! each grid cell contributes the straight segments where the surface,
//! taken as bilinear between its four corners, crosses a level.

use super::HeightField;

/// Round contour intervals, in metres.
const STEPS: [f32; 8] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0];

/// How many levels a page should carry at most: enough to read the relief,
/// few enough that the lines stay apart.
const MAX_LEVELS: f32 = 12.0;

/// Every fifth level is drawn heavier, as on a topographic map.
pub(crate) const MAJOR_EVERY: i32 = 5;

/// The round interval that gives at most [`MAX_LEVELS`] levels over `lo..hi`.
pub fn contour_step(lo: f32, hi: f32) -> f32 {
    let span = (hi - lo).max(0.0);
    STEPS.into_iter().find(|s| span / s <= MAX_LEVELS).unwrap_or(STEPS[STEPS.len() - 1])
}

/// The levels `step` apart that fall strictly inside `lo..hi`, with whether
/// each is a major one (a multiple of `step * MAJOR_EVERY`).
pub fn contour_levels(lo: f32, hi: f32, step: f32) -> Vec<(f32, bool)> {
    if !(step > 0.0) || !lo.is_finite() || !hi.is_finite() || hi <= lo {
        return Vec::new();
    }
    let first = (lo / step).floor() as i32 + 1;
    let last = (hi / step).ceil() as i32 - 1;
    (first..=last).map(|k| (k as f32 * step, k % MAJOR_EVERY == 0)).collect()
}

/// The world-space segments of one level's contour across `field`.
pub fn contour_segments(field: &HeightField, level: f32) -> Vec<[(f32, f32); 2]> {
    let (w, h) = (field.width, field.height);
    if w < 2 || h < 2 || field.z.len() < w * h {
        return Vec::new();
    }
    let at = |ix: usize, iy: usize| field.z[iy * w + ix];
    let pos = |ix: usize, iy: usize| (field.x0 + ix as f32 * field.step_x, field.y0 + iy as f32 * field.step_y);
    // Where the level crosses the edge from corner `a` to corner `b`.
    let cross = |a: (f32, f32, f32), b: (f32, f32, f32)| {
        let t = if (b.2 - a.2).abs() < f32::EPSILON { 0.5 } else { ((level - a.2) / (b.2 - a.2)).clamp(0.0, 1.0) };
        (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
    };
    let mut out = Vec::new();
    for iy in 0..h - 1 {
        for ix in 0..w - 1 {
            // Corners anticlockwise from the bottom left: 0 (ix,iy), 1 (ix+1,iy), 2 (ix+1,iy+1), 3 (ix,iy+1).
            let corners = [(ix, iy), (ix + 1, iy), (ix + 1, iy + 1), (ix, iy + 1)].map(|(x, y)| {
                let (px, py) = pos(x, y);
                (px, py, at(x, y))
            });
            if corners.iter().any(|c| !c.2.is_finite()) {
                continue;
            }
            let mut case = 0u8;
            for (i, c) in corners.iter().enumerate() {
                if c.2 >= level {
                    case |= 1 << i;
                }
            }
            if case == 0 || case == 15 {
                continue;
            }
            // Edge k runs from corner k to corner k+1.
            let edge = |k: usize| cross(corners[k], corners[(k + 1) % 4]);
            let mut push = |a: usize, b: usize| out.push([edge(a), edge(b)]);
            match case {
                1 | 14 => push(3, 0),
                2 | 13 => push(0, 1),
                3 | 12 => push(3, 1),
                4 | 11 => push(1, 2),
                6 | 9 => push(0, 2),
                7 | 8 => push(3, 2),
                // The two saddles: the centre's own height says which way
                // the surface connects.
                5 | 10 => {
                    let centre = corners.iter().map(|c| c.2).sum::<f32>() / 4.0;
                    if (centre >= level) == (case == 5) {
                        push(3, 0);
                        push(1, 2);
                    } else {
                        push(0, 1);
                        push(2, 3);
                    }
                }
                _ => unreachable!("4-bit case"),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(w: usize, h: usize, z: Vec<f32>) -> HeightField {
        HeightField { x0: 0.0, y0: 0.0, step_x: 10.0, step_y: 10.0, width: w, height: h, z }
    }

    #[test]
    fn the_step_keeps_the_level_count_down() {
        assert_eq!(contour_step(0.0, 5.0), 1.0);
        assert_eq!(contour_step(0.0, 60.0), 5.0);
        assert_eq!(contour_step(0.0, 400.0), 50.0);
        assert_eq!(contour_step(0.0, 816.0), 100.0);
        assert_eq!(contour_step(3.0, 3.0), 1.0, "a flat field still gets a step");
    }

    #[test]
    fn levels_fall_strictly_inside_the_range_and_mark_every_fifth() {
        let levels = contour_levels(0.0, 50.0, 10.0);
        assert_eq!(levels, vec![(10.0, false), (20.0, false), (30.0, false), (40.0, false)]);
        let levels = contour_levels(-3.0, 103.0, 20.0);
        assert_eq!(levels.len(), 6);
        assert_eq!(levels[0], (0.0, true));
        assert_eq!(levels[5], (100.0, true));
        assert!(contour_levels(5.0, 5.0, 1.0).is_empty());
        assert!(contour_levels(0.0, 10.0, 0.0).is_empty());
    }

    #[test]
    fn a_slope_gives_a_straight_contour() {
        // z rises with x: 0, 10, 20 across three columns, two rows.
        let f = field(3, 2, vec![0.0, 10.0, 20.0, 0.0, 10.0, 20.0]);
        let segs = contour_segments(&f, 5.0);
        assert_eq!(segs.len(), 1, "{segs:?}");
        let [a, b] = segs[0];
        assert!((a.0 - 5.0).abs() < 1e-5 && (b.0 - 5.0).abs() < 1e-5, "the 5 m line is at x = 5: {segs:?}");
        assert_eq!((a.1.min(b.1), a.1.max(b.1)), (0.0, 10.0));
        assert!(contour_segments(&f, 25.0).is_empty(), "above everything");
        assert!(contour_segments(&f, -1.0).is_empty(), "below everything");
    }

    #[test]
    fn a_peak_is_ringed() {
        // One high corner cell in the middle of a flat field.
        let mut z = vec![0.0; 9];
        z[4] = 100.0;
        let segs = contour_segments(&field(3, 3, z), 50.0);
        assert_eq!(segs.len(), 4, "one segment per cell around the peak: {segs:?}");
        for [a, b] in &segs {
            for p in [a, b] {
                // Halfway between the peak (10,10) and a neighbour.
                assert!((p.0 - 10.0).abs() < 1e-5 && (p.1 - 5.0).abs() < 1e-5 || (p.0 - 10.0).abs() < 1e-5 && (p.1 - 15.0).abs() < 1e-5
                    || (p.1 - 10.0).abs() < 1e-5 && ((p.0 - 5.0).abs() < 1e-5 || (p.0 - 15.0).abs() < 1e-5), "{p:?}");
            }
        }
    }

    #[test]
    fn a_saddle_splits_by_its_centre() {
        // High at two opposite corners, low at the others, centre low: two
        // separate arcs, not a cross.
        let segs = contour_segments(&field(2, 2, vec![10.0, 0.0, 10.0, 0.0]), 5.0);
        assert_eq!(segs.len(), 1, "two highs on one edge share a contour: {segs:?}");
        let segs = contour_segments(&field(2, 2, vec![10.0, 0.0, 0.0, 10.0]), 5.0);
        assert_eq!(segs.len(), 2, "a saddle makes two segments: {segs:?}");
        assert!(contour_segments(&field(1, 1, vec![1.0]), 0.5).is_empty(), "no cell, no contour");
    }
}
