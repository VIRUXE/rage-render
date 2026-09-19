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

/// The bitmap font covers printable ASCII only, and draws anything else as a
/// box. Folding the text first turns a stray em dash into a hyphen rather
/// than a box, in both backends, so the two still agree on what they drew.
pub(crate) fn drawable_text(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' '..='~' => c,
            '\u{2012}'..='\u{2015}' | '\u{2212}' => '-',
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            '\u{2192}' => '>',
            '\u{00A0}' => ' ',
            _ => '?',
        })
        .collect()
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
    /// Confines what is drawn next to `(x, y, w, h)` in page pixels, or lifts
    /// the clip when `None`. The map's content is clipped to the map area so
    /// that geometry outside the region cannot run over the axes, the legend
    /// or the edge of the page.
    fn clip(&mut self, rect: Option<(f32, f32, f32, f32)>);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_text_the_font_cannot_draw() {
        assert_eq!(drawable_text("v_cafe \u{2014} all heights"), "v_cafe - all heights");
        assert_eq!(drawable_text("3\u{2192}7 \u{201C}q\u{201D} \u{fc}"), "3>7 \"q\" ?");
        assert_eq!(drawable_text("plain ASCII ~!@"), "plain ASCII ~!@");
    }

    #[test]
    fn folding_keeps_the_width() {
        let raw = "a \u{2014} b";
        assert_eq!(measure(raw, TextSize::Body), measure(&drawable_text(raw), TextSize::Body));
    }
}
