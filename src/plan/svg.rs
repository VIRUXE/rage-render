//! The vector backend: the same drawing calls, written out as SVG markup.
//!
//! Hybrid by design — [`SvgCanvas::image`] embeds the rasterised mesh
//! underlay as one base64 PNG, so the heavy triangle soup stays a bitmap
//! while rooms, portals and labels stay selectable vector shapes.

use image::RgbaImage;

use super::canvas::{measure, Canvas, Rgba8, TextSize};

/// A [`Canvas`] that appends SVG elements to a document body.
pub(crate) struct SvgCanvas {
    body: String,
}

impl SvgCanvas {
    pub(crate) fn new() -> Self {
        Self { body: String::new() }
    }

    /// Wraps everything drawn so far in an `<svg>` root of `width` x `height`.
    pub(crate) fn finish(self, width: u32, height: u32) -> String {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
             viewBox=\"0 0 {width} {height}\">\n{}</svg>\n",
            self.body
        )
    }

    fn push(&mut self, el: &str) {
        self.body.push_str(el);
        self.body.push('\n');
    }
}

/// `fill`/`stroke` attributes for a colour, with its alpha as an opacity.
fn paint(kind: &str, c: Rgba8) -> String {
    format!(
        "{kind}=\"rgb({},{},{})\" {kind}-opacity=\"{:.3}\"",
        c[0],
        c[1],
        c[2],
        c[3] as f32 / 255.0
    )
}

fn dash_attr(dash: Option<(f32, f32)>) -> String {
    match dash {
        Some((on, off)) => format!(" stroke-dasharray=\"{on},{off}\""),
        None => String::new(),
    }
}

fn points(pts: &[(f32, f32)]) -> String {
    pts.iter()
        .map(|(x, y)| format!("{x:.2},{y:.2}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Escapes the five characters that may not appear literally in XML text.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with `=` padding.
pub(crate) fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        let idx = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        for (i, k) in idx.iter().enumerate() {
            if i <= chunk.len() {
                out.push(B64[*k as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Encodes `img` as a PNG byte stream.
fn png_bytes(img: &RgbaImage) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    image::ImageEncoder::write_image(
        encoder,
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgba8,
    )
    .ok()?;
    Some(buf)
}

impl Canvas for SvgCanvas {
    fn fill_polygon(&mut self, pts: &[(f32, f32)], fill: Rgba8) {
        if pts.len() < 3 {
            return;
        }
        let el = format!("<polygon points=\"{}\" {} stroke=\"none\"/>", points(pts), paint("fill", fill));
        self.push(&el);
    }

    fn stroke_polygon(&mut self, pts: &[(f32, f32)], stroke: Rgba8, width: f32, dash: Option<(f32, f32)>) {
        if pts.len() < 3 {
            return;
        }
        let el = format!(
            "<polygon points=\"{}\" fill=\"none\" {} stroke-width=\"{width}\"{}/>",
            points(pts),
            paint("stroke", stroke),
            dash_attr(dash)
        );
        self.push(&el);
    }

    fn line(&mut self, a: (f32, f32), b: (f32, f32), stroke: Rgba8, width: f32, dash: Option<(f32, f32)>) {
        let el = format!(
            "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" {} stroke-width=\"{width}\"{}/>",
            a.0,
            a.1,
            b.0,
            b.1,
            paint("stroke", stroke),
            dash_attr(dash)
        );
        self.push(&el);
    }

    fn circle(&mut self, c: (f32, f32), r: f32, fill: Rgba8, stroke: Option<Rgba8>) {
        let stroke = match stroke {
            Some(s) => format!(" {} stroke-width=\"1\"", paint("stroke", s)),
            None => String::new(),
        };
        let el = format!(
            "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{r:.2}\" {}{stroke}/>",
            c.0,
            c.1,
            paint("fill", fill)
        );
        self.push(&el);
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Option<Rgba8>, stroke: Option<Rgba8>) {
        let fill = match fill {
            Some(f) => paint("fill", f),
            None => "fill=\"none\"".to_string(),
        };
        let stroke = match stroke {
            Some(s) => format!(" {} stroke-width=\"1\"", paint("stroke", s)),
            None => String::new(),
        };
        let el = format!("<rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{w:.2}\" height=\"{h:.2}\" {fill}{stroke}/>");
        self.push(&el);
    }

    fn text(&mut self, x: f32, y: f32, s: &str, size: TextSize, color: Rgba8, halo: bool) {
        if s.is_empty() {
            return;
        }
        let halo = if halo { " stroke=\"white\" stroke-width=\"3\" paint-order=\"stroke\"" } else { "" };
        let el = format!(
            "<text x=\"{x:.2}\" y=\"{y:.2}\" font-family=\"monospace\" font-size=\"{}\" \
             textLength=\"{:.2}\" lengthAdjust=\"spacingAndGlyphs\" dominant-baseline=\"hanging\" {}{halo}>{}</text>",
            crate::font::GLYPH_H * size.scale(),
            measure(s, size),
            paint("fill", color),
            escape(s)
        );
        self.push(&el);
    }

    fn text_width(&self, s: &str, size: TextSize) -> f32 {
        measure(s, size)
    }

    fn image(&mut self, x: f32, y: f32, img: &RgbaImage) {
        let Some(png) = png_bytes(img) else { return };
        let el = format!(
            "<image x=\"{x:.2}\" y=\"{y:.2}\" width=\"{}\" height=\"{}\" \
             href=\"data:image/png;base64,{}\" image-rendering=\"pixelated\"/>",
            img.width(),
            img.height(),
            base64(&png)
        );
        self.push(&el);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_pads_the_tail() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"any carnal pleasure."), "YW55IGNhcm5hbCBwbGVhc3VyZS4=");
    }

    #[test]
    fn text_is_escaped() {
        assert_eq!(escape("a & b < \"c\""), "a &amp; b &lt; &quot;c&quot;");
    }
}
