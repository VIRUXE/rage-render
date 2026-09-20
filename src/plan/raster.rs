//! The pixel backend: everything the plan draws, blended into an `RgbaImage`.

use image::{Rgba, RgbaImage};

use super::canvas::{drawable_text, measure, Canvas, Rgba8, TextSize};

/// A [`Canvas`] that draws into an image.
pub(crate) struct RasterCanvas {
    pub(crate) img: RgbaImage,
    /// Pixel bounds `x0, y0, x1, y1` (x1/y1 exclusive) drawing is confined to.
    clip: Option<[i32; 4]>,
}

impl RasterCanvas {
    /// A canvas of `w` x `h` pixels filled with `background`.
    pub(crate) fn new(w: u32, h: u32, background: Rgba8) -> Self {
        Self { img: RgbaImage::from_pixel(w.max(1), h.max(1), Rgba(background)), clip: None }
    }

    /// A fully transparent canvas, for the mesh underlay.
    pub(crate) fn transparent(w: u32, h: u32) -> Self {
        Self { img: RgbaImage::from_pixel(w.max(1), h.max(1), Rgba([0, 0, 0, 0])), clip: None }
    }

    /// Blends one pixel, unless the clip rect excludes it.
    fn put(&mut self, x: i32, y: i32, c: Rgba8) {
        if let Some(r) = self.clip {
            if x < r[0] || y < r[1] || x >= r[2] || y >= r[3] {
                return;
            }
        }
        blend(&mut self.img, x, y, c);
    }

    /// Stamps `s`'s glyphs with their top-left corner at `(x, y)`, clipped.
    /// Text is opaque ink rather than paint: each pixel replaces what is
    /// under it, as `font::draw_text` does.
    fn glyphs(&mut self, x: i32, y: i32, s: &str, scale: u32, colour: Rgba8) {
        let clip = self.clip;
        let img = &mut self.img;
        let (w, h) = (img.width() as i32, img.height() as i32);
        crate::font::text_pixels(x, y, s, scale, |px, py| {
            if let Some(r) = clip {
                if px < r[0] || py < r[1] || px >= r[2] || py >= r[3] {
                    return;
                }
            }
            if px >= 0 && py >= 0 && px < w && py < h {
                img.put_pixel(px as u32, py as u32, Rgba(colour));
            }
        });
    }

    /// Scanline even-odd fill of a simple polygon.
    fn fill(&mut self, pts: &[(f32, f32)], colour: Rgba8) {
        if pts.len() < 3 {
            return;
        }
        let (w, h) = (self.img.width() as i32, self.img.height() as i32);
        let y_min = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let y_max = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil().min(h as f32 - 1.0) as i32;
        let mut xs: Vec<f32> = Vec::new();
        for y in y_min..=y_max {
            let sy = y as f32 + 0.5;
            xs.clear();
            for i in 0..pts.len() {
                let (ax, ay) = pts[i];
                let (bx, by) = pts[(i + 1) % pts.len()];
                if (ay <= sy && by > sy) || (by <= sy && ay > sy) {
                    xs.push(ax + (sy - ay) / (by - ay) * (bx - ax));
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            for pair in xs.chunks(2) {
                if pair.len() < 2 {
                    break;
                }
                let xa = pair[0].round().max(0.0) as i32;
                let xb = pair[1].round().min(w as f32 - 1.0) as i32;
                for x in xa..=xb {
                    self.put(x, y, colour);
                }
            }
        }
    }

    /// Stamps a disc of radius `r` (a thick line's pen) at `(x, y)`.
    fn pen(&mut self, x: f32, y: f32, r: f32, colour: Rgba8) {
        if r <= 0.5 {
            self.put(x.round() as i32, y.round() as i32, colour);
            return;
        }
        let rr = r * r;
        let span = r.ceil() as i32;
        for dy in -span..=span {
            for dx in -span..=span {
                if (dx * dx + dy * dy) as f32 <= rr {
                    self.put(x.round() as i32 + dx, y.round() as i32 + dy, colour);
                }
            }
        }
    }

    /// Walks a segment a pixel at a time, honouring an optional on/off dash
    /// pattern measured in pixels. `phase` carries the dash position between
    /// segments so a dashed polygon does not restart at every corner.
    fn segment(&mut self, a: (f32, f32), b: (f32, f32), colour: Rgba8, width: f32, dash: Option<(f32, f32)>, phase: &mut f32) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt();
        if !len.is_finite() {
            return;
        }
        let steps = len.ceil().max(1.0) as i32;
        let r = (width / 2.0).max(0.5);
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let d = *phase + len * t;
            let on = match dash {
                Some((on, off)) if on + off > 0.0 => d % (on + off) < on,
                _ => true,
            };
            if on {
                self.pen(a.0 + dx * t, a.1 + dy * t, r, colour);
            }
        }
        *phase += len;
    }
}

/// Alpha-blends `c` over the pixel at `(x, y)`, ignoring out-of-bounds writes.
pub(crate) fn blend(img: &mut RgbaImage, x: i32, y: i32, c: Rgba8) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 || c[3] == 0 {
        return;
    }
    let a = c[3] as u32;
    let p = img.get_pixel_mut(x as u32, y as u32);
    let dst_a = p.0[3] as u32;
    // Source-over on straight alpha; with an opaque destination this is the
    // usual lerp, and over a transparent one it keeps the source colour.
    let out_a = a + dst_a * (255 - a) / 255;
    for k in 0..3 {
        p.0[k] = if out_a == 0 {
            0
        } else {
            ((c[k] as u32 * a + p.0[k] as u32 * dst_a * (255 - a) / 255) / out_a) as u8
        };
    }
    p.0[3] = out_a as u8;
}

impl Canvas for RasterCanvas {
    fn fill_polygon(&mut self, pts: &[(f32, f32)], fill_colour: Rgba8) {
        self.fill(pts, fill_colour);
    }

    fn stroke_polygon(&mut self, pts: &[(f32, f32)], stroke: Rgba8, width: f32, dash: Option<(f32, f32)>) {
        if pts.len() < 3 {
            return;
        }
        let mut phase = 0.0;
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            self.segment(a, b, stroke, width, dash, &mut phase);
        }
    }

    fn line(&mut self, a: (f32, f32), b: (f32, f32), stroke: Rgba8, width: f32, dash: Option<(f32, f32)>) {
        let mut phase = 0.0;
        self.segment(a, b, stroke, width, dash, &mut phase);
    }

    fn circle(&mut self, c: (f32, f32), r: f32, fill_colour: Rgba8, stroke: Option<Rgba8>) {
        let ri = r.ceil() as i32;
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                let d = ((dx * dx + dy * dy) as f32).sqrt();
                let colour = match stroke {
                    Some(s) if d > r - 1.0 && d <= r + 0.5 => Some(s),
                    _ if d <= r => Some(fill_colour),
                    _ => None,
                };
                if let Some(colour) = colour {
                    self.put(c.0.round() as i32 + dx, c.1.round() as i32 + dy, colour);
                }
            }
        }
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, fill_colour: Option<Rgba8>, stroke: Option<Rgba8>) {
        let pts = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
        if let Some(f) = fill_colour {
            self.fill(&pts, f);
        }
        if let Some(s) = stroke {
            self.stroke_polygon(&pts, s, 1.0, None);
        }
    }

    fn text(&mut self, x: f32, y: f32, s: &str, size: TextSize, color: Rgba8, halo: bool) {
        let scale = size.scale();
        let s = &drawable_text(s);
        let (x, y) = (x.round() as i32, y.round() as i32);
        // Glyph pixels go through `glyphs` rather than straight onto the
        // image, so text obeys the clip rect like every other primitive: a
        // label near the edge of the map used to spill over the axes.
        if halo {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx != 0 || dy != 0 {
                        self.glyphs(x + dx, y + dy, s, scale, [255, 255, 255, 255]);
                    }
                }
            }
        }
        self.glyphs(x, y, s, scale, color);
    }

    fn text_width(&self, s: &str, size: TextSize) -> f32 {
        measure(s, size)
    }

    fn image(&mut self, x: f32, y: f32, img: &RgbaImage) {
        // Blitted a pixel at a time rather than with `imageops::overlay`, so
        // that the clip rect applies to the underlay as well.
        let (ox, oy) = (x.round() as i32, y.round() as i32);
        for (px, py, pixel) in img.enumerate_pixels() {
            self.put(ox + px as i32, oy + py as i32, pixel.0);
        }
    }

    fn clip(&mut self, rect: Option<(f32, f32, f32, f32)>) {
        self.clip = rect.map(|(x, y, w, h)| {
            [x.round() as i32, y.round() as i32, (x + w).round() as i32, (y + h).round() as i32]
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_confined_to_the_clip_rect() {
        const BG: Rgba8 = [0, 128, 0, 255];
        let mut canvas = RasterCanvas::new(80, 40, BG);
        let rect = (20.0, 12.0, 20.0, 16.0);
        canvas.clip(Some(rect));
        // Straddles the left edge of the clip and, haloed, its top as well.
        canvas.text(6.0, 12.0, "WWWWWWWW", TextSize::Body, [0, 0, 0, 255], true);
        canvas.clip(None);

        let (x0, y0) = (rect.0 as u32, rect.1 as u32);
        let (x1, y1) = ((rect.0 + rect.2) as u32, (rect.1 + rect.3) as u32);
        let mut inked = 0;
        for (x, y, p) in canvas.img.enumerate_pixels() {
            let inside = x >= x0 && x < x1 && y >= y0 && y < y1;
            if p.0 != BG {
                assert!(inside, "text ink at ({x}, {y}), outside the clip rect");
                inked += 1;
            }
        }
        assert!(inked > 0, "the clip swallowed the text entirely");
    }
}
