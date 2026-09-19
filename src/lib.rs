//! Drawing RAGE drawables: a small software rasteriser producing PNG/JPG/WebP
//! views of a `.ydr`/`.ydd`/`.yft` (with its textures), contact sheets of
//! several renders, a bitmap font for labels, top-down floor plans of an
//! interior, and a wasm-exposed glTF exporter. All parsing lives in
//! `rage-formats`; this crate only draws.

pub mod render;
pub mod font;
pub mod sheet;
pub mod plan;
pub mod wasm;

pub use font::{draw_text, text_width, FONT_5X7, GLYPH_H, GLYPH_W};
pub use plan::{clip_tri_to_band, plan_png, plan_svg, quad_footprint, rooms_stacked, EntityMark, Layer,
               Marker, NavClass, NavShape, PlanOptions, PlanReport, PortalShape, RoomShape, Scene, Tri,
               FLOOR_BAND};
pub use render::{is_vehicle_paint_shader, render_drawable, render_parts, render_views, RenderOptions,
                 RenderPart, RenderReport, TextureSet, View, VEHICLE_PAINT_SPS};
pub use sheet::{compose_sheet, sheet_layout, SheetItem, SheetOptions};
pub use wasm::convert_to_gltf;
pub use image;

#[cfg(test)]
mod tests {
    #[test]
    fn font_table_shape() {
        assert_eq!(crate::GLYPH_W, 5);
        assert_eq!(crate::GLYPH_H, 7);
        assert_eq!(crate::FONT_5X7.len(), 96);
    }
}
