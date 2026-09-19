//! The drawing surface the plan is composed onto. Implemented twice: once
//! onto pixels ([`super::raster::RasterCanvas`]) and once into SVG markup
//! ([`super::svg::SvgCanvas`]).

use image::RgbaImage;

/// Straight (non-premultiplied) RGBA.
pub(crate) type Rgba8 = [u8; 4];

/// The three text sizes the plan uses, as bitmap-font scales.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextSize {
    Small,
    Body,
    Title,
}

impl TextSize {
    /// The `crate::font` integer scale this size draws at.
    pub(crate) fn scale(self) -> u32 {
        match self {
            TextSize::Small => 1,
            TextSize::Body => 2,
            TextSize::Title => 3,
        }
    }

    /// Height of a line of text at this size, in pixels.
    pub(crate) fn height(self) -> f32 {
        (crate::font::GLYPH_H * self.scale()) as f32
    }
}

/// Width `s` occupies at `size`. Both backends measure with the bitmap font so
/// that label placement computed once is valid for either.
pub(crate) fn measure(s: &str, size: TextSize) -> f32 {
    crate::font::text_width(s, size.scale()) as f32
}

/// Everything the plan draws with. Coordinates are page pixels, y down.
pub(crate) trait Canvas {
    fn fill_polygon(&mut self, pts: &[(f32, f32)], fill: Rgba8);
    fn stroke_polygon(&mut self, pts: &[(f32, f32)], stroke: Rgba8, width: f32, dash: Option<(f32, f32)>);
    fn line(&mut self, a: (f32, f32), b: (f32, f32), stroke: Rgba8, width: f32, dash: Option<(f32, f32)>);
    fn circle(&mut self, c: (f32, f32), r: f32, fill: Rgba8, stroke: Option<Rgba8>);
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Option<Rgba8>, stroke: Option<Rgba8>);
    /// Draws `s` with its top-left corner at `(x, y)`.
    fn text(&mut self, x: f32, y: f32, s: &str, size: TextSize, color: Rgba8, halo: bool);
    fn text_width(&self, s: &str, size: TextSize) -> f32;
    fn image(&mut self, x: f32, y: f32, img: &RgbaImage);
}
