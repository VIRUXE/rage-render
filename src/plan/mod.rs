//! A 2D top-down "plan" of a GTA V interior: rooms, portals, entities,
//! collision and drawable meshes, navmesh, drawn as a readable floor plan.
//!
//! The caller fills a [`Scene`] with world-space geometry and calls
//! [`plan_png`] for an image or [`plan_svg`] for a hybrid SVG (vector page,
//! the triangle-mesh underlay embedded as one PNG). Nothing here touches the
//! filesystem, so the crate still builds for wasm.

mod cartography;
mod canvas;
mod geometry;
mod layers;
mod palette;
mod raster;
mod svg;

use std::str::FromStr;

use image::RgbaImage;
use rage_formats::{Vec2, Vec3};

use canvas::{Canvas, TextSize};
use geometry::in_band;
use cartography::{LabelRequest, Layout, LegendRow};
use palette::{Facing, Mesh};
use raster::RasterCanvas;
use svg::SvgCanvas;

pub use geometry::{clip_tri_to_band, quad_footprint, rooms_stacked};

/// A drawable layer of the plan. Layers are drawn bottom-up in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Rooms,
    Portals,
    Entities,
    Collision,
    Drawable,
    Navmesh,
}

impl Layer {
    /// Every layer, in declaration order.
    pub const ALL: [Layer; 6] = [
        Layer::Rooms,
        Layer::Portals,
        Layer::Entities,
        Layer::Collision,
        Layer::Drawable,
        Layer::Navmesh,
    ];

    /// The lowercase name used on the command line and in the legend.
    pub fn name(self) -> &'static str {
        match self {
            Layer::Rooms => "rooms",
            Layer::Portals => "portals",
            Layer::Entities => "entities",
            Layer::Collision => "collision",
            Layer::Drawable => "drawable",
            Layer::Navmesh => "navmesh",
        }
    }
}

impl std::fmt::Display for Layer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Layer {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Layer::ALL
            .into_iter()
            .find(|l| l.name() == s.to_ascii_lowercase())
            .ok_or_else(|| {
                let names: Vec<&str> = Layer::ALL.iter().map(|l| l.name()).collect();
                anyhow::anyhow!("unknown layer '{s}'; valid layers are {}", names.join(", "))
            })
    }
}

/// One world-space triangle of a collision or drawable mesh.
#[derive(Debug, Clone, Copy)]
pub struct Tri {
    pub v: [Vec3; 3],
}

/// How a navmesh polygon relates to the interior being plotted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavClass {
    Interior,
    Exterior,
    Sunk,
}

/// One navmesh polygon.
#[derive(Debug, Clone)]
pub struct NavShape {
    pub vertices: Vec<Vec3>,
    pub class: NavClass,
}

/// One MLO room: its XY footprint and the z range it occupies.
#[derive(Debug, Clone)]
pub struct RoomShape {
    pub index: usize,
    pub name: String,
    pub footprint: [Vec2; 4],
    pub z_lo: f32,
    pub z_hi: f32,
}

/// One MLO portal, as the polygon of its corners.
#[derive(Debug, Clone)]
pub struct PortalShape {
    pub index: usize,
    pub room_from: usize,
    pub room_to: usize,
    pub corners: Vec<Vec3>,
}

/// One placed entity, drawn as a dot with an optional label.
#[derive(Debug, Clone)]
pub struct EntityMark {
    pub position: Vec3,
    pub label: String,
    pub set: Option<usize>,
    pub faded: bool,
}

/// A caller-supplied annotation, always drawn.
#[derive(Debug, Clone)]
pub struct Marker {
    pub x: f32,
    pub y: f32,
    pub label: String,
}

/// Everything the plan can draw, in world space.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub title: String,
    pub caption: Vec<String>,
    pub rooms: Vec<RoomShape>,
    pub portals: Vec<PortalShape>,
    pub entities: Vec<EntityMark>,
    pub entity_set_names: Vec<String>,
    pub collision: Vec<Tri>,
    pub drawable: Vec<Tri>,
    pub navmesh: Vec<NavShape>,
    pub markers: Vec<Marker>,
}

/// How to draw the plan.
#[derive(Debug, Clone)]
pub struct PlanOptions {
    /// World-space `x0,y0,x1,y1`; the scene's own bounds plus a margin when `None`.
    pub region: Option<[f32; 4]>,
    /// Pixels per metre.
    pub scale: f32,
    /// Only draw geometry within this z band.
    pub z_band: Option<(f32, f32)>,
    pub layers: Vec<Layer>,
    /// Draw portal and entity labels (room and marker labels are always drawn).
    pub labels: bool,
    /// Refuse to render a map area larger than this many pixels.
    pub max_pixels: u64,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            region: None,
            scale: 30.0,
            z_band: None,
            layers: Layer::ALL.to_vec(),
            labels: false,
            max_pixels: 40_000_000,
        }
    }
}

/// What a plan run produced.
#[derive(Debug, Clone)]
pub struct PlanReport {
    pub width: u32,
    pub height: u32,
    pub region: [f32; 4],
    pub drawn: Vec<(Layer, usize)>,
    pub warnings: Vec<String>,
}

/// `--floor-z Z` means the band `(Z - 0.3, Z + 2.0)`: a little tolerance below
/// the floor, and high enough to stay clear of the ceiling above it.
pub const FLOOR_BAND: (f32, f32) = (-0.3, 2.0);

// --- drawing ---------------------------------------------------------------

/// One band-clipped mesh triangle, ready to fill.
pub(crate) struct MeshPoly {
    pub(crate) mesh: Mesh,
    pub(crate) facing: Facing,
    /// Mean z, which orders the floors and picks their shade.
    pub(crate) z: f32,
    pub(crate) poly: Vec<Vec3>,
}

/// Everything decided before a single pixel is drawn: the region, the page
/// layout, and which of the scene's parts survive the layer and band filters.
pub(crate) struct Prepared {
    pub(crate) region: [f32; 4],
    pub(crate) layout: Layout,
    pub(crate) rooms: Vec<usize>,
    pub(crate) portals: Vec<usize>,
    pub(crate) entities: Vec<usize>,
    pub(crate) navmesh: Vec<usize>,
    pub(crate) mesh: Vec<MeshPoly>,
    /// Per enabled layer, how much of it is being drawn.
    pub(crate) counts: Vec<(Layer, usize)>,
    /// Entity set indices present among the drawn entities, ascending.
    pub(crate) sets: Vec<usize>,
}

/// The z-facing of a triangle, from its normal.
fn facing_of(t: &Tri) -> Facing {
    let n = (t.v[1] - t.v[0]).cross(t.v[2] - t.v[0]);
    let len = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
    palette::facing(if len > 0.0 { n.z / len } else { 0.0 })
}

/// Collects the band-clipped triangles of one mesh.
fn mesh_polys(tris: &[Tri], mesh: Mesh, band: Option<(f32, f32)>, out: &mut Vec<MeshPoly>) {
    for t in tris {
        let poly = match band {
            Some((lo, hi)) => match clip_tri_to_band(t, lo, hi) {
                Some(p) => p,
                None => continue,
            },
            None => t.v.to_vec(),
        };
        if poly.iter().any(|v| !v.x.is_finite() || !v.y.is_finite() || !v.z.is_finite()) {
            continue;
        }
        let z = poly.iter().map(|v| v.z).sum::<f32>() / poly.len() as f32;
        out.push(MeshPoly { mesh, facing: facing_of(t), z, poly });
    }
}

/// Works out what will be drawn and how big the page has to be.
fn prepare(scene: &Scene, opts: &PlanOptions) -> anyhow::Result<Prepared> {
    let band = opts.z_band;
    let enabled = |l: Layer| opts.layers.contains(&l);

    let mut region = match opts.region {
        Some(r) => [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])],
        None => {
            let b = geometry::scene_bounds(scene, &opts.layers, band).ok_or_else(|| {
                anyhow::anyhow!("nothing to draw: no geometry in the chosen layers and z band")
            })?;
            [b[0] - 2.0, b[1] - 2.0, b[2] + 2.0, b[3] + 2.0]
        }
    };
    if !region.iter().all(|v| v.is_finite()) {
        anyhow::bail!("nothing to draw: the region is not a finite rectangle");
    }
    // A point region (one entity, one marker) still deserves a page.
    if region[2] - region[0] < 0.5 {
        let c = (region[0] + region[2]) / 2.0;
        region[0] = c - 0.25;
        region[2] = c + 0.25;
    }
    if region[3] - region[1] < 0.5 {
        let c = (region[1] + region[3]) / 2.0;
        region[1] = c - 0.25;
        region[3] = c + 0.25;
    }
    if !(opts.scale > 0.0) {
        anyhow::bail!("scale must be greater than zero");
    }

    let map_w = ((region[2] - region[0]) * opts.scale).round().max(1.0);
    let map_h = ((region[3] - region[1]) * opts.scale).round().max(1.0);
    if map_w as u64 * map_h as u64 > opts.max_pixels {
        anyhow::bail!("{}x{} px is too large; lower --scale or shrink --region", map_w as u64, map_h as u64);
    }

    let mut rooms = Vec::new();
    if enabled(Layer::Rooms) {
        for (i, r) in scene.rooms.iter().enumerate() {
            let outside = matches!(band, Some((lo, hi)) if r.z_hi < lo || r.z_lo > hi);
            if !outside {
                rooms.push(i);
            }
        }
    }
    let mut portals = Vec::new();
    if enabled(Layer::Portals) {
        for (i, p) in scene.portals.iter().enumerate() {
            if p.corners.is_empty() {
                continue;
            }
            let lo = p.corners.iter().fold(f32::MAX, |m, v| m.min(v.z));
            let hi = p.corners.iter().fold(f32::MIN, |m, v| m.max(v.z));
            let outside = matches!(band, Some((blo, bhi)) if hi < blo || lo > bhi);
            if !outside {
                portals.push(i);
            }
        }
    }
    let mut entities = Vec::new();
    if enabled(Layer::Entities) {
        for (i, e) in scene.entities.iter().enumerate() {
            if in_band(e.position.z, band) {
                entities.push(i);
            }
        }
    }
    let mut navmesh = Vec::new();
    if enabled(Layer::Navmesh) {
        for (i, n) in scene.navmesh.iter().enumerate() {
            if n.vertices.len() >= 3 && n.vertices.iter().any(|v| in_band(v.z, band)) {
                navmesh.push(i);
            }
        }
    }
    let mut mesh = Vec::new();
    if enabled(Layer::Collision) {
        mesh_polys(&scene.collision, Mesh::Collision, band, &mut mesh);
    }
    let collision_count = mesh.len();
    if enabled(Layer::Drawable) {
        mesh_polys(&scene.drawable, Mesh::Drawable, band, &mut mesh);
    }

    let counts: Vec<(Layer, usize)> = Layer::ALL
        .into_iter()
        .filter(|l| enabled(*l))
        .map(|l| {
            let n = match l {
                Layer::Rooms => rooms.len(),
                Layer::Portals => portals.len(),
                Layer::Entities => entities.len(),
                Layer::Collision => collision_count,
                Layer::Drawable => mesh.len() - collision_count,
                Layer::Navmesh => navmesh.len(),
            };
            (l, n)
        })
        .collect();

    let mut sets: Vec<usize> = entities.iter().filter_map(|i| scene.entities[*i].set).collect();
    sets.sort_unstable();
    sets.dedup();

    let legend_rows = counts.iter().filter(|(_, n)| *n > 0).count() + sets.len();
    let layout = cartography::layout(region, opts.scale, legend_rows, scene.caption.len());

    Ok(Prepared { region, layout, rooms, portals, entities, navmesh, mesh, counts, sets })
}

/// The legend rows for what was drawn.
fn legend_rows(scene: &Scene, prep: &Prepared) -> Vec<LegendRow> {
    let mut rows: Vec<LegendRow> = prep
        .counts
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(layer, n)| LegendRow {
            swatch: match layer {
                Layer::Rooms => palette::hue(1),
                Layer::Portals => palette::PORTAL_STROKE,
                Layer::Entities => palette::INK,
                Layer::Collision => Mesh::Collision.swatch(),
                Layer::Drawable => Mesh::Drawable.swatch(),
                Layer::Navmesh => palette::NAV_INTERIOR_STROKE,
            },
            text: format!("{} {n}", layer.name()),
        })
        .collect();
    for set in &prep.sets {
        let name = scene.entity_set_names.get(*set).cloned().unwrap_or_else(|| format!("set {set}"));
        rows.push(LegendRow { swatch: palette::hue(*set), text: name });
    }
    rows
}

/// Draws the whole plan onto `canvas`, which must be the size `prep`'s
/// layout states. `prep` is passed in rather than computed here so that
/// `plan_png`, which has to size its image up front, prepares only once.
fn draw(prep: Prepared, scene: &Scene, opts: &PlanOptions, canvas: &mut dyn Canvas) -> PlanReport {
    let l = prep.layout;
    let mut warnings = Vec::new();

    canvas.rect(0.0, 0.0, l.width as f32, l.height as f32, Some(palette::BACKGROUND), None);
    cartography::draw_grid(canvas, &l, prep.region);

    if let Some(underlay) = layers::mesh_underlay(&prep) {
        canvas.image(l.map.x, l.map.y, &underlay);
    }

    let mut labels: Vec<LabelRequest> = Vec::new();
    layers::navmesh(scene, &prep, canvas);
    layers::rooms(scene, &prep, canvas, &mut labels);
    layers::portals(scene, &prep, opts, canvas, &mut labels);
    layers::entities(scene, &prep, opts, canvas, &mut labels);
    layers::markers(scene, &prep, canvas, &mut labels);

    // Every label is placed together, after the artwork, so none is buried.
    let (placed, dropped) =
        cartography::place_labels(&mut labels, l.map, |s, size| (canvas::measure(s, size), size.height()));
    for label in placed {
        canvas.text(label.x, label.y, &label.text, label.size, label.color, true);
    }
    if dropped > 0 {
        warnings.push(format!("{dropped} labels hidden to avoid overlap"));
    }

    cartography::draw_axes(canvas, &l, prep.region);
    cartography::draw_scale_bar(canvas, &l);
    cartography::draw_north_arrow(canvas, &l);
    cartography::draw_legend(canvas, &l, &legend_rows(scene, &prep));

    if !scene.title.is_empty() {
        canvas.text(cartography::MARGIN, cartography::MARGIN, &scene.title, TextSize::Title, palette::INK, false);
    }
    for (i, line) in scene.caption.iter().enumerate() {
        let y = l.caption_y + i as f32 * cartography::CAPTION_LINE_H;
        canvas.text(l.map.x, y, line, TextSize::Small, palette::AXIS_TEXT, false);
    }

    PlanReport { width: l.width, height: l.height, region: prep.region, drawn: prep.counts, warnings }
}

/// Draws the plan as an image.
pub fn plan_png(scene: &Scene, opts: &PlanOptions) -> anyhow::Result<(RgbaImage, PlanReport)> {
    let prep = prepare(scene, opts)?;
    let size = prep.layout;
    let mut canvas = RasterCanvas::new(size.width, size.height, palette::BACKGROUND);
    let report = draw(prep, scene, opts, &mut canvas);
    Ok((canvas.img, report))
}

/// Draws the plan as a hybrid SVG: vector page, mesh underlay as one PNG.
pub fn plan_svg(scene: &Scene, opts: &PlanOptions) -> anyhow::Result<(String, PlanReport)> {
    let prep = prepare(scene, opts)?;
    let mut canvas = SvgCanvas::new();
    let report = draw(prep, scene, opts, &mut canvas);
    Ok((canvas.finish(report.width, report.height), report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav_scene() -> Scene {
        Scene {
            title: "v_kitchen".into(),
            navmesh: vec![NavShape {
                vertices: vec![
                    Vec3::new(0.0, 0.0, 1.0),
                    Vec3::new(6.0, 0.0, 1.0),
                    Vec3::new(6.0, 4.0, 1.0),
                    Vec3::new(0.0, 4.0, 1.0),
                ],
                class: NavClass::Interior,
            }],
            ..Default::default()
        }
    }

    fn room_scene() -> Scene {
        Scene {
            title: "v_office".into(),
            caption: vec!["one line".into()],
            rooms: vec![RoomShape {
                index: 1,
                name: "kitchen".into(),
                footprint: [
                    Vec2::new(0.0, 0.0),
                    Vec2::new(6.0, 0.0),
                    Vec2::new(6.0, 4.0),
                    Vec2::new(0.0, 4.0),
                ],
                z_lo: 0.0,
                z_hi: 3.0,
            }],
            ..Default::default()
        }
    }

    fn collision_scene() -> Scene {
        Scene {
            title: "v_slab".into(),
            collision: vec![
                Tri { v: [Vec3::new(0.0, 0.0, 0.0), Vec3::new(6.0, 0.0, 0.0), Vec3::new(6.0, 4.0, 0.0)] },
                Tri { v: [Vec3::new(0.0, 0.0, 0.0), Vec3::new(6.0, 4.0, 0.0), Vec3::new(0.0, 4.0, 0.0)] },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn png_is_laid_out_and_paints_the_navmesh() {
        let scene = nav_scene();
        let opts = PlanOptions { scale: 10.0, ..Default::default() };
        let (img, report) = plan_png(&scene, &opts).expect("a plan");

        let expected = cartography::layout(report.region, 10.0, 1, 0);
        assert_eq!((img.width(), img.height()), (expected.width, expected.height));
        assert_eq!((report.width, report.height), (expected.width, expected.height));
        assert_eq!(report.region, [-2.0, -2.0, 8.0, 6.0], "scene bounds plus a 2 m margin");

        let (cx, cy) = expected.transform.to_px(3.0, 2.0);
        let p = img.get_pixel(cx as u32, cy as u32).0;
        assert!(p[1] > p[0] && p[1] > p[2], "navmesh centroid is not greenish: {p:?}");
        assert_eq!(report.drawn.iter().find(|(l, _)| *l == Layer::Navmesh).map(|(_, n)| *n), Some(1));
    }

    #[test]
    fn svg_embeds_the_mesh_underlay_as_a_png() {
        let (svg, _) = plan_svg(&collision_scene(), &PlanOptions::default()).expect("a plan");
        assert!(svg.contains("<image"), "no embedded image");
        assert!(svg.contains("data:image/png;base64,"), "image is not an inline PNG");
    }

    #[test]
    fn svg_without_a_mesh_has_no_image() {
        let (svg, _) = plan_svg(&room_scene(), &PlanOptions::default()).expect("a plan");
        assert!(!svg.contains("<image"), "unexpected embedded image");
    }

    #[test]
    fn svg_draws_the_title_and_the_room() {
        let (svg, _) = plan_svg(&room_scene(), &PlanOptions::default()).expect("a plan");
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.contains("<text"), "no text at all");
        assert!(svg.contains("v_office"), "the title is missing");
        assert!(svg.contains("<polygon"), "the room is missing");
        assert!(svg.contains("one line"), "the caption is missing");
        assert!(svg.ends_with("</svg>\n"));
    }

    #[test]
    fn portal_labels_are_ascii_so_the_bitmap_font_can_draw_them() {
        let mut scene = room_scene();
        scene.portals.push(PortalShape {
            index: 4,
            room_from: 3,
            room_to: 7,
            corners: vec![
                Vec3::new(1.0, 1.0, 1.0),
                Vec3::new(3.0, 1.0, 1.0),
                Vec3::new(3.0, 3.0, 2.0),
                Vec3::new(1.0, 3.0, 2.0),
            ],
        });
        let opts = PlanOptions { labels: true, ..Default::default() };
        let (svg, _) = plan_svg(&scene, &opts).expect("a plan");
        // `>` is escaped in XML text, so the markup carries the entity form.
        assert!(svg.contains("P4 3-&gt;7"), "portal label is missing or not ASCII");
        assert!(!svg.contains('\u{2192}'), "a glyph the 5x7 font cannot draw");
    }

    #[test]
    fn a_map_bigger_than_max_pixels_is_refused() {
        let opts = PlanOptions { scale: 500.0, max_pixels: 10_000, ..Default::default() };
        let err = plan_png(&room_scene(), &opts).unwrap_err().to_string();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn an_empty_scene_has_nothing_to_draw() {
        let err = plan_png(&Scene::default(), &PlanOptions::default()).unwrap_err().to_string();
        assert!(err.contains("nothing to draw"), "{err}");
    }

    #[test]
    fn a_zero_area_region_still_renders() {
        let opts = PlanOptions { region: Some([5.0, 5.0, 5.0, 5.0]), ..Default::default() };
        let (img, report) = plan_png(&room_scene(), &opts).expect("a plan");
        assert!(img.width() > 0 && img.height() > 0);
        assert!(report.region[2] > report.region[0] && report.region[3] > report.region[1]);
    }

    #[test]
    fn unknown_layer_names_list_the_valid_ones() {
        let err = "walls".parse::<Layer>().unwrap_err().to_string();
        assert!(err.contains("rooms") && err.contains("navmesh"), "{err}");
        assert_eq!("Rooms".parse::<Layer>().unwrap(), Layer::Rooms);
    }

    #[test]
    fn band_filtering_counts_only_what_is_inside() {
        let mut scene = room_scene();
        scene.entities = vec![
            EntityMark { position: Vec3::new(1.0, 1.0, 0.5), label: "lamp".into(), set: None, faded: false },
            EntityMark { position: Vec3::new(2.0, 2.0, 9.0), label: "upstairs".into(), set: Some(0), faded: true },
        ];
        let opts = PlanOptions { z_band: Some((-0.3, 2.0)), ..Default::default() };
        let (_, report) = plan_png(&scene, &opts).expect("a plan");
        assert_eq!(report.drawn.iter().find(|(l, _)| *l == Layer::Entities).map(|(_, n)| *n), Some(1));
    }

    #[test]
    fn disabled_layers_are_not_drawn() {
        let opts = PlanOptions { layers: vec![Layer::Navmesh], ..Default::default() };
        let (_, report) = plan_png(&nav_scene(), &opts).expect("a plan");
        assert_eq!(report.drawn.len(), 1);
        assert_eq!(report.drawn[0].0, Layer::Navmesh);
    }
}
