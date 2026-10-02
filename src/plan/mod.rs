//! A 2D top-down "plan" of a GTA V interior: rooms, portals, entities,
//! collision and drawable meshes, navmesh and path nodes, drawn as a
//! readable floor plan.
//!
//! The caller fills a [`Scene`] with world-space geometry and calls
//! [`plan_png`] for an image or [`plan_svg`] for a hybrid SVG (vector page,
//! the triangle-mesh underlay embedded as one PNG). Nothing here touches the
//! filesystem, so the crate still builds for wasm.

mod cartography;
mod canvas;
mod contours;
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

pub use contours::{contour_levels, contour_segments, contour_step};
pub use geometry::{clip_tri_to_band, quad_footprint, rooms_stacked, scene_bounds};

/// A drawable layer of the plan. Layers are drawn bottom-up in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Rooms,
    Portals,
    Entities,
    Collision,
    Drawable,
    Navmesh,
    /// Vehicle and pedestrian path nodes and the links between them.
    Paths,
    /// The water quads of `water.xml`.
    Water,
    /// Height contours from the world heightmap.
    Terrain,
}

impl Layer {
    /// Every layer, in declaration order.
    pub const ALL: [Layer; 9] = [
        Layer::Rooms,
        Layer::Portals,
        Layer::Entities,
        Layer::Collision,
        Layer::Drawable,
        Layer::Navmesh,
        Layer::Paths,
        Layer::Water,
        Layer::Terrain,
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
            Layer::Paths => "paths",
            Layer::Water => "water",
            Layer::Terrain => "terrain",
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

/// What a path node is for, which is what CodeWalker colours it by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathNodeKind {
    Vehicle,
    Ped,
    /// Either of the node's two "disabled" bits is set.
    Disabled,
}

/// One path node, drawn as a dot; a junction gets a ring.
#[derive(Debug, Clone)]
pub struct PathNodeMark {
    pub position: Vec3,
    pub kind: PathNodeKind,
    pub junction: bool,
}

/// What a path link is, which picks its colour and stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathLinkKind {
    Road,
    OffRoad,
    Ped,
    Shortcut,
    /// Flagged "don't use for navigation".
    NoNavigation,
    /// Either end is a disabled node.
    Disabled,
}

/// One path link, drawn as a line whose width grows with its lane count.
#[derive(Debug, Clone)]
pub struct PathLinkShape {
    pub from: Vec3,
    pub to: Vec3,
    pub kind: PathLinkKind,
    /// Lanes forward plus backward.
    pub lanes: u8,
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

/// One water quad: an axis-aligned rectangle of water at `z`.
#[derive(Debug, Clone, Copy)]
pub struct WaterQuadShape {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub z: f32,
    /// The game simulates but never draws it; drawn as a dashed outline.
    pub invisible: bool,
}

/// A regular grid of ground heights, row-major with row 0 at `y0`, from
/// which the terrain layer draws contours. Cells with no ground may hold
/// `NaN`; no contour crosses them.
#[derive(Debug, Clone, Default)]
pub struct HeightField {
    pub x0: f32,
    pub y0: f32,
    pub step_x: f32,
    pub step_y: f32,
    pub width: usize,
    pub height: usize,
    pub z: Vec<f32>,
}

/// Everything the plan can draw, in world space.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    /// The heading drawn at the top of the page. Plain ASCII reads best: the
    /// bitmap font `plan_png` draws with covers printable ASCII, and anything
    /// else is folded to the nearest character it can draw.
    pub title: String,
    pub caption: Vec<String>,
    pub rooms: Vec<RoomShape>,
    pub portals: Vec<PortalShape>,
    pub entities: Vec<EntityMark>,
    pub entity_set_names: Vec<String>,
    pub collision: Vec<Tri>,
    pub drawable: Vec<Tri>,
    pub navmesh: Vec<NavShape>,
    pub path_nodes: Vec<PathNodeMark>,
    pub path_links: Vec<PathLinkShape>,
    pub markers: Vec<Marker>,
    pub water: Vec<WaterQuadShape>,
    pub terrain: Option<HeightField>,
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
    pub(crate) path_nodes: Vec<usize>,
    pub(crate) path_links: Vec<usize>,
    pub(crate) mesh: Vec<MeshPoly>,
    pub(crate) water: Vec<usize>,
    /// One entry per contour level drawn.
    pub(crate) contours: Vec<ContourLevel>,
    /// Per enabled layer, how much of it is being drawn.
    pub(crate) counts: Vec<(Layer, usize)>,
    /// Entity set indices present among the drawn entities, ascending.
    pub(crate) sets: Vec<usize>,
}

/// One contour level, ready to draw.
pub(crate) struct ContourLevel {
    pub(crate) level: f32,
    pub(crate) major: bool,
    pub(crate) segments: Vec<[(f32, f32); 2]>,
}

/// The contour levels of `field` over the part of it inside `region`: the
/// step is picked from the heights within the region (plus one cell each
/// way, so the lines run to the edge), and only segments touching the
/// region are kept.
fn terrain_contours(field: &HeightField, region: [f32; 4], band: Option<(f32, f32)>) -> Vec<ContourLevel> {
    let (w, h) = (field.width, field.height);
    if w < 2 || h < 2 || field.z.len() < w * h || !(field.step_x > 0.0) || !(field.step_y > 0.0) {
        return Vec::new();
    }
    let col = |x: f32| ((x - field.x0) / field.step_x).floor();
    let row = |y: f32| ((y - field.y0) / field.step_y).floor();
    let ix0 = (col(region[0]) - 1.0).max(0.0) as usize;
    let iy0 = (row(region[1]) - 1.0).max(0.0) as usize;
    let ix1 = ((col(region[2]) + 2.0).max(0.0) as usize).min(w);
    let iy1 = ((row(region[3]) + 2.0).max(0.0) as usize).min(h);
    if ix1 <= ix0 + 1 || iy1 <= iy0 + 1 {
        return Vec::new();
    }
    let mut z = Vec::with_capacity((ix1 - ix0) * (iy1 - iy0));
    for iy in iy0..iy1 {
        z.extend_from_slice(&field.z[iy * w + ix0..iy * w + ix1]);
    }
    let cut = HeightField {
        x0: field.x0 + ix0 as f32 * field.step_x,
        y0: field.y0 + iy0 as f32 * field.step_y,
        step_x: field.step_x,
        step_y: field.step_y,
        width: ix1 - ix0,
        height: iy1 - iy0,
        z,
    };
    let (lo, hi) = cut.z.iter().filter(|v| v.is_finite()).fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    if lo > hi {
        return Vec::new();
    }
    let (lo, hi) = match band {
        Some((blo, bhi)) => (lo.max(blo), hi.min(bhi)),
        None => (lo, hi),
    };
    let step = contours::contour_step(lo, hi);
    let touches = |[a, b]: &[(f32, f32); 2]| {
        a.0.max(b.0) >= region[0] && a.0.min(b.0) <= region[2] && a.1.max(b.1) >= region[1] && a.1.min(b.1) <= region[3]
    };
    contours::contour_levels(lo, hi, step)
        .into_iter()
        .map(|(level, major)| {
            let segments = contours::contour_segments(&cut, level).into_iter().filter(touches).collect();
            ContourLevel { level, major, segments }
        })
        .filter(|c| !c.segments.is_empty())
        .collect()
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
    // A point region (one entity, one marker) still deserves a page. Half a
    // metre is below f32's resolution far from the origin, so the padding
    // grows with the magnitude of the coordinates it has to separate.
    for (lo, hi) in [(0, 2), (1, 3)] {
        if region[hi] - region[lo] < 0.5 {
            let c = (region[lo] + region[hi]) / 2.0;
            let pad = 0.25f32.max(c.abs() * f32::EPSILON * 8.0);
            region[lo] = c - pad;
            region[hi] = c + pad;
        }
    }
    if !(opts.scale > 0.0) {
        anyhow::bail!("scale must be greater than zero");
    }

    // An extent wide enough to overflow f32 would saturate every later cast,
    // so it is refused here rather than turned into a u32::MAX canvas.
    let extent = [region[2] - region[0], region[3] - region[1]];
    if !extent.iter().all(|e| e.is_finite() && *e > 0.0) {
        anyhow::bail!(
            "a region spanning {}x{} m is too large; shrink --region",
            extent[0],
            extent[1]
        );
    }
    let map_w = (extent[0] * opts.scale).round().max(1.0);
    let map_h = (extent[1] * opts.scale).round().max(1.0);
    if !map_w.is_finite() || !map_h.is_finite() || map_w as f64 * map_h as f64 > opts.max_pixels as f64 {
        anyhow::bail!("{map_w:.0}x{map_h:.0} px is too large; lower --scale or shrink --region");
    }

    let mut rooms = Vec::new();
    if enabled(Layer::Rooms) {
        for (i, r) in scene.rooms.iter().enumerate() {
            let outside = matches!(band, Some((lo, hi)) if r.z_hi < lo || r.z_lo > hi);
            if !outside && r.footprint.iter().all(|v| v.x.is_finite() && v.y.is_finite()) {
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
            if !outside && p.corners.iter().all(|v| geometry::finite(*v)) {
                portals.push(i);
            }
        }
    }
    let mut entities = Vec::new();
    if enabled(Layer::Entities) {
        for (i, e) in scene.entities.iter().enumerate() {
            if in_band(e.position.z, band) && geometry::finite(e.position) {
                entities.push(i);
            }
        }
    }
    let mut navmesh = Vec::new();
    if enabled(Layer::Navmesh) {
        for (i, n) in scene.navmesh.iter().enumerate() {
            if n.vertices.len() >= 3
                && n.vertices.iter().any(|v| in_band(v.z, band))
                && n.vertices.iter().all(|v| geometry::finite(*v))
            {
                navmesh.push(i);
            }
        }
    }
    let mut path_nodes = Vec::new();
    let mut path_links = Vec::new();
    if enabled(Layer::Paths) {
        for (i, n) in scene.path_nodes.iter().enumerate() {
            if in_band(n.position.z, band) && geometry::finite(n.position) {
                path_nodes.push(i);
            }
        }
        for (i, l) in scene.path_links.iter().enumerate() {
            if (in_band(l.from.z, band) || in_band(l.to.z, band)) && geometry::finite(l.from) && geometry::finite(l.to) {
                path_links.push(i);
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
    let mut water = Vec::new();
    if enabled(Layer::Water) {
        for (i, q) in scene.water.iter().enumerate() {
            let finite = [q.x0, q.y0, q.x1, q.y1, q.z].iter().all(|v| v.is_finite());
            let meets = q.x0.min(q.x1) <= region[2] && q.x0.max(q.x1) >= region[0]
                && q.y0.min(q.y1) <= region[3] && q.y0.max(q.y1) >= region[1];
            if finite && meets && in_band(q.z, band) {
                water.push(i);
            }
        }
    }
    let contours = match (&scene.terrain, enabled(Layer::Terrain)) {
        (Some(field), true) => terrain_contours(field, region, band),
        _ => Vec::new(),
    };

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
                Layer::Paths => path_nodes.len(),
                Layer::Water => water.len(),
                Layer::Terrain => contours.len(),
            };
            (l, n)
        })
        .collect();

    let mut sets: Vec<usize> = entities.iter().filter_map(|i| scene.entities[*i].set).collect();
    sets.sort_unstable();
    sets.dedup();

    let legend_rows = counts.iter().filter(|(_, n)| *n > 0).count() + sets.len();
    let layout = cartography::layout(region, opts.scale, legend_rows, scene.caption.len());

    Ok(Prepared { region, layout, rooms, portals, entities, navmesh, path_nodes, path_links, mesh, water, contours, counts, sets })
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
                Layer::Paths => palette::PATH_ROAD,
                Layer::Water => palette::WATER_STROKE,
                Layer::Terrain => palette::TERRAIN_MAJOR,
            },
            text: match layer {
                // The step tells the reader what a line means.
                Layer::Terrain => match contour_step_of(prep) {
                    Some(step) => format!("terrain {n} @ {step:.0} m"),
                    None => format!("terrain {n}"),
                },
                _ => format!("{} {n}", layer.name()),
            },
        })
        .collect();
    for set in &prep.sets {
        let name = scene.entity_set_names.get(*set).cloned().unwrap_or_else(|| format!("set {set}"));
        rows.push(LegendRow { swatch: palette::hue(*set), text: name });
    }
    rows
}

/// The interval between the contour levels drawn, from the first two.
fn contour_step_of(prep: &Prepared) -> Option<f32> {
    match prep.contours.as_slice() {
        [a, b, ..] => Some(b.level - a.level),
        _ => None,
    }
}

/// Draws the whole plan onto `canvas`, which must be the size `prep`'s
/// layout states. `prep` is passed in rather than computed here so that
/// `plan_png`, which has to size its image up front, prepares only once.
fn draw(prep: Prepared, scene: &Scene, opts: &PlanOptions, canvas: &mut dyn Canvas) -> PlanReport {
    let l = prep.layout;
    let mut warnings = Vec::new();

    canvas.rect(0.0, 0.0, l.width as f32, l.height as f32, Some(palette::BACKGROUND), None);

    // Everything in map space is clipped to the map area: a navmesh polygon
    // or a room that reaches past the region must not run over the axes, the
    // legend or the edge of the page.
    canvas.clip(Some((l.map.x, l.map.y, l.map.w, l.map.h)));
    cartography::draw_grid(canvas, &l, prep.region);

    // The ground first, the water on it, then everything built on either.
    layers::terrain(&prep, canvas);
    layers::water(scene, &prep, canvas);

    if let Some(underlay) = layers::mesh_underlay(&prep) {
        canvas.image(l.map.x, l.map.y, &underlay);
    }

    let mut labels: Vec<LabelRequest> = Vec::new();
    layers::navmesh(scene, &prep, canvas);
    layers::paths(scene, &prep, canvas);
    layers::rooms(scene, &prep, canvas, &mut labels);
    layers::portals(scene, &prep, opts, canvas, &mut labels);
    layers::entities(scene, &prep, opts, canvas, &mut labels);
    layers::markers(scene, &prep, canvas, &mut labels);

    // Every label is placed together, after the artwork, so none is buried.
    let (placed, drops) =
        cartography::place_labels(&mut labels, l.map, |s, size| (canvas::measure(s, size), size.height()));
    for label in placed {
        canvas.text(label.x, label.y, &label.text, label.size, label.color, true);
    }
    if drops.crowded > 0 {
        warnings.push(format!("{} labels hidden to avoid overlap", drops.crowded));
    }
    if drops.off_map > 0 {
        warnings.push(format!("{} labels off the map", drops.off_map));
    }
    canvas.clip(None);

    cartography::draw_axes(canvas, &l, prep.region);
    cartography::draw_scale_bar(canvas, &l);
    cartography::draw_north_arrow(canvas, &l);
    cartography::draw_legend(canvas, &l, &legend_rows(scene, &prep));

    if !scene.title.is_empty() {
        let width = l.width as f32 - 2.0 * cartography::MARGIN;
        let (size, title) = cartography::fit_text(&scene.title, width, TextSize::Title);
        canvas.text(cartography::MARGIN, cartography::MARGIN, &title, size, palette::INK, false);
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
    let clip_id = svg::clip_id(&scene.title, prep.region, (prep.layout.width, prep.layout.height));
    let mut canvas = SvgCanvas::new(clip_id);
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

    /// A pond over a slope: the quad is filled blue, the slope is contoured
    /// at a round step, and both are counted and listed in the legend.
    #[test]
    fn water_and_terrain_are_drawn_counted_and_listed() {
        // z rises 0..60 m across 7 columns, 10 m per column, 4 rows.
        let z: Vec<f32> = (0..4).flat_map(|_| (0..7).map(|ix| ix as f32 * 10.0)).collect();
        let scene = Scene {
            title: "slope".into(),
            water: vec![
                WaterQuadShape { x0: 10.0, y0: 5.0, x1: 20.0, y1: 15.0, z: 12.0, invisible: false },
                WaterQuadShape { x0: 40.0, y0: 5.0, x1: 50.0, y1: 15.0, z: 12.0, invisible: true },
                WaterQuadShape { x0: 500.0, y0: 500.0, x1: 600.0, y1: 600.0, z: 0.0, invisible: false },
            ],
            terrain: Some(HeightField { x0: 0.0, y0: 0.0, step_x: 10.0, step_y: 10.0, width: 7, height: 4, z }),
            ..Default::default()
        };
        let opts = PlanOptions { region: Some([0.0, 0.0, 60.0, 30.0]), scale: 10.0, ..Default::default() };
        let (img, report) = plan_png(&scene, &opts).expect("a plan");

        let count = |layer: Layer| report.drawn.iter().find(|(l, _)| *l == layer).map(|(_, n)| *n);
        assert_eq!(count(Layer::Water), Some(2), "the far quad is outside the region");
        // 0..60 m at the 5 m step: 5 10 .. 55, eleven levels.
        assert_eq!(count(Layer::Terrain), Some(11));

        let layout = cartography::layout(report.region, 10.0, 2, 0);
        let (cx, cy) = layout.transform.to_px(17.5, 10.0); // between the 15 and 20 m contours
        let p = img.get_pixel(cx as u32, cy as u32).0;
        assert!(p[2] > p[0] && p[2] > p[1], "the pond is not blueish: {p:?}");
        // The 20 m contour runs down x = 20; a pixel on it is brown, not white.
        let (lx, ly) = layout.transform.to_px(20.0, 25.0);
        let on_line = (-1..=1).any(|dx| {
            let q = img.get_pixel((lx as i32 + dx) as u32, ly as u32).0;
            q[0] > q[2] && q[0] < 250
        });
        assert!(on_line, "no contour at x = 20");

        let (svg, _) = plan_svg(&scene, &opts).expect("a plan");
        assert!(svg.contains("water 2"), "no water legend row");
        assert!(svg.contains("terrain 11 @ 5 m"), "no terrain legend row");
        assert!(svg.contains("stroke-dasharray"), "the invisible quad is not dashed");
    }

    #[test]
    fn scene_bounds_frame_water_when_nothing_else_is_there() {
        let scene = Scene {
            water: vec![WaterQuadShape { x0: 10.0, y0: 5.0, x1: 20.0, y1: 15.0, z: 0.0, invisible: false }],
            ..Default::default()
        };
        assert_eq!(scene_bounds(&scene, &Layer::ALL, None), Some([10.0, 5.0, 20.0, 15.0]));
        assert_eq!(scene_bounds(&scene, &[Layer::Terrain], None), None, "a height field never frames a page");
    }

    #[test]
    fn two_plans_get_their_own_clip_id() {
        let clip_id = |svg: &str| -> String {
            let start = svg.find("<clipPath id=\"").expect("a clip path") + "<clipPath id=\"".len();
            let rest = &svg[start..];
            rest[..rest.find('"').expect("a closing quote")].to_string()
        };
        let mut a = room_scene();
        a.title = "v_office".into();
        let mut b = room_scene();
        b.title = "v_kitchen".into();
        let (sa, _) = plan_svg(&a, &PlanOptions::default()).expect("a plan");
        let (sb, _) = plan_svg(&b, &PlanOptions::default()).expect("a plan");

        let (ia, ib) = (clip_id(&sa), clip_id(&sb));
        for id in [&ia, &ib] {
            let hex = id.strip_prefix("plan-map-").unwrap_or_else(|| panic!("odd clip id '{id}'"));
            assert_eq!(hex.len(), 8, "clip id '{id}' is not eight hex digits");
            assert!(hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "clip id '{id}'");
        }
        assert_ne!(ia, ib, "two plans share a clip id, so inlining both in one page breaks one");

        // Each document refers to its own id, and only to its own.
        assert_eq!(sa.matches(&format!("url(#{ia})")).count(), 1);
        assert_eq!(sb.matches(&format!("url(#{ib})")).count(), 1);
        assert!(!sa.contains(&ib) && !sb.contains(&ia));
        // The same scene twice is the same document, id included.
        let (sa2, _) = plan_svg(&a, &PlanOptions::default()).expect("a plan");
        assert_eq!(clip_id(&sa2), ia);
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
    fn svg_text_is_spaced_not_stretched() {
        let (svg, _) = plan_svg(&room_scene(), &PlanOptions::default()).expect("a plan");
        assert!(svg.contains("<text"), "no text at all");
        assert!(
            !svg.contains("spacingAndGlyphs"),
            "glyphs are still stretched to the bitmap width"
        );
        assert!(svg.contains("lengthAdjust=\"spacing\""), "textLength is adjusted some other way");
        // The width the label placer measured is still asserted on the text.
        assert!(svg.contains("textLength="), "textLength is gone, so labels no longer match the PNG");
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

    /// A rectangle in world space, as a navmesh polygon.
    fn nav_rect(x0: f32, y0: f32, x1: f32, y1: f32, class: NavClass) -> NavShape {
        NavShape {
            vertices: vec![
                Vec3::new(x0, y0, 1.0),
                Vec3::new(x1, y0, 1.0),
                Vec3::new(x1, y1, 1.0),
                Vec3::new(x0, y1, 1.0),
            ],
            class,
        }
    }

    #[test]
    fn map_content_is_clipped_to_the_map_area() {
        // An exterior polygon sprawling far past the region: its outline must
        // stop at the map rect instead of crossing the axes and the legend.
        let mut scene = nav_scene();
        scene.caption.clear();
        scene.navmesh.push(nav_rect(-400.0, -400.0, 400.0, 400.0, NavClass::Exterior));
        let opts = PlanOptions { region: Some([0.0, 0.0, 6.0, 4.0]), scale: 20.0, ..Default::default() };
        let (img, report) = plan_png(&scene, &opts).expect("a plan");

        let rows = report.drawn.iter().filter(|(_, n)| *n > 0).count();
        let map = cartography::layout(report.region, 20.0, rows, 0).map;
        let (x0, y0) = (map.x.round() as u32, map.y.round() as u32);
        let (x1, y1) = ((map.x + map.w).round() as u32, (map.y + map.h).round() as u32);
        for (x, y, p) in img.enumerate_pixels() {
            let inside = x >= x0 && x < x1 && y >= y0 && y < y1;
            assert!(
                inside || p.0 != palette::NAV_EXTERIOR_STROKE,
                "navmesh ink at ({x}, {y}), outside the map rect"
            );
        }

        let (svg, _) = plan_svg(&scene, &opts).expect("a plan");
        let id = svg::clip_id(&scene.title, report.region, (report.width, report.height));
        assert!(svg.contains(&format!("<clipPath id=\"{id}\">")), "no clip path");
        assert!(svg.contains(&format!("<g clip-path=\"url(#{id})\">")), "the map content is not clipped");
        assert_eq!(svg.matches("</g>").count(), 1, "the clip group is left open or closed twice");
    }

    #[test]
    fn a_title_the_font_cannot_draw_is_folded() {
        let mut scene = room_scene();
        scene.title = "navmesh[108][96].ynv \u{2014} all heights".into();
        let (svg, _) = plan_svg(&scene, &PlanOptions::default()).expect("a plan");
        assert!(svg.contains("navmesh[108][96].ynv - all heights"), "the em dash was not folded");
        assert!(!svg.contains('\u{2014}'));
    }

    #[test]
    fn the_default_region_ignores_sprawling_exterior_navmesh() {
        let scene = Scene {
            navmesh: vec![
                nav_rect(0.0, 0.0, 6.0, 4.0, NavClass::Interior),
                nav_rect(-500.0, -500.0, 500.0, 500.0, NavClass::Exterior),
            ],
            ..Default::default()
        };
        let (_, report) = plan_png(&scene, &PlanOptions::default()).expect("a plan");
        assert_eq!(report.region, [-2.0, -2.0, 8.0, 6.0], "framed the exterior sprawl");
    }

    #[test]
    fn the_default_region_frames_the_room_not_the_world_collision() {
        // An MLO folder often carries vanilla world-space collision chunks;
        // framing those makes the interior a speck on a 22941x38106 px page.
        let mut scene = room_scene();
        scene.collision = vec![
            Tri { v: [Vec3::new(-800.0, -1200.0, 0.0), Vec3::new(500.0, -1200.0, 0.0), Vec3::new(500.0, 600.0, 0.0)] },
            Tri { v: [Vec3::new(-800.0, -1200.0, 0.0), Vec3::new(500.0, 600.0, 0.0), Vec3::new(-800.0, 600.0, 0.0)] },
        ];
        let (_, report) = plan_png(&scene, &PlanOptions::default()).expect("a plan");
        assert_eq!(report.region, [-2.0, -2.0, 8.0, 6.0], "framed the world collision");
    }

    #[test]
    fn collision_frames_the_page_when_there_is_no_interior() {
        let scene = Scene {
            collision: vec![Tri {
                v: [Vec3::new(0.0, 0.0, 0.0), Vec3::new(6.0, 0.0, 0.0), Vec3::new(6.0, 4.0, 0.0)],
            }],
            ..Default::default()
        };
        let (_, report) = plan_png(&scene, &PlanOptions::default()).expect("a plan");
        assert_eq!(report.region, [-2.0, -2.0, 8.0, 6.0]);
    }

    #[test]
    fn exterior_navmesh_frames_the_page_when_it_is_all_there_is() {
        let scene = Scene { navmesh: vec![nav_rect(0.0, 0.0, 6.0, 4.0, NavClass::Exterior)], ..Default::default() };
        let (_, report) = plan_png(&scene, &PlanOptions::default()).expect("a plan");
        assert_eq!(report.region, [-2.0, -2.0, 8.0, 6.0]);
    }

    #[test]
    fn a_title_wider_than_the_page_is_cut_to_fit() {
        let mut scene = room_scene();
        scene.title = "v_office ".repeat(40);
        let (svg, report) = plan_svg(&scene, &PlanOptions::default()).expect("a plan");
        assert!(svg.contains("..</text>"), "the title was not cut short");
        let drawn = svg.split("<text").nth(1).expect("a title");
        let length: f32 = drawn
            .split("textLength=\"")
            .nth(1)
            .and_then(|t| t.split('"').next())
            .and_then(|t| t.parse().ok())
            .expect("a measured title");
        assert!(length <= report.width as f32 - 16.0, "the title is {length} px on a {} px page", report.width);
    }

    #[test]
    fn a_map_bigger_than_max_pixels_is_refused() {
        let opts = PlanOptions { scale: 500.0, max_pixels: 10_000, ..Default::default() };
        let err = plan_png(&room_scene(), &opts).unwrap_err().to_string();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn an_enormous_region_is_refused_not_overflowed() {
        // The extent overflows f32 to infinity; the guard must catch that
        // rather than saturate into a u32::MAX canvas.
        let opts = PlanOptions { region: Some([-2e38, -2e38, 2e38, 2e38]), ..Default::default() };
        let err = plan_png(&room_scene(), &opts).unwrap_err().to_string();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn a_huge_region_on_a_small_page_still_terminates() {
        // A trillion metres at a billionth of a pixel per metre. The grid step
        // caps out at 500 m, so a loop that walks one step at a time would draw
        // two billion lines onto a thousand-pixel page.
        let opts = PlanOptions { region: Some([0.0, 0.0, 1e12, 1e10]), scale: 1e-9, ..Default::default() };
        let (img, report) = plan_png(&Scene::default(), &opts).expect("a plan");
        let expected = cartography::layout(report.region, 1e-9, 0, 0);
        assert_eq!((img.width(), img.height()), (expected.width, expected.height));
        assert_eq!(img.width(), 1072, "a thousand-pixel map, its axis and margins");
    }

    #[test]
    fn a_region_far_from_the_origin_still_terminates() {
        // At 1e9 metres the f32 spacing is wider than the grid step, so a
        // grid loop that advances by adding the step never finishes.
        // Padding a region f32 cannot resolve makes it kilometres wide, so it
        // needs a scale that still fits on a page.
        let opts = PlanOptions { region: Some([1e9, 1e9, 1e9 + 10.0, 1e9 + 8.0]), scale: 0.1, ..Default::default() };
        let (img, report) = plan_png(&Scene::default(), &opts).expect("a plan");
        assert!(img.width() > 0 && img.height() > 0);
        // f32 cannot resolve ten metres at a billion, so the region is padded
        // out to something it can, rather than collapsing to nothing.
        assert!(report.region[2] > report.region[0] && report.region[3] > report.region[1]);
    }

    #[test]
    fn shapes_with_non_finite_vertices_are_dropped() {
        let mut scene = room_scene();
        scene.rooms.push(RoomShape {
            index: 2,
            name: "broken".into(),
            footprint: [
                Vec2::new(f32::NAN, 0.0),
                Vec2::new(6.0, 0.0),
                Vec2::new(6.0, 4.0),
                Vec2::new(0.0, 4.0),
            ],
            z_lo: 0.0,
            z_hi: 3.0,
        });
        scene.portals.push(PortalShape {
            index: 0,
            room_from: 1,
            room_to: 2,
            corners: vec![
                Vec3::new(1.0, 1.0, 1.0),
                Vec3::new(f32::INFINITY, 1.0, 1.0),
                Vec3::new(3.0, 3.0, 2.0),
            ],
        });
        scene.navmesh.push(NavShape {
            vertices: vec![
                Vec3::new(1.0, 1.0, 1.0),
                Vec3::new(3.0, 1.0, 1.0),
                Vec3::new(3.0, f32::NAN, 1.0),
            ],
            class: NavClass::Interior,
        });

        let (svg, report) = plan_svg(&scene, &PlanOptions::default()).expect("a plan");
        assert!(!svg.contains("NaN") && !svg.contains("inf"), "non-finite coordinates reached the markup");
        let count = |l: Layer| report.drawn.iter().find(|(k, _)| *k == l).map(|(_, n)| *n);
        assert_eq!(count(Layer::Rooms), Some(1), "the broken room is still counted");
        assert_eq!(count(Layer::Portals), Some(0));
        assert_eq!(count(Layer::Navmesh), Some(0));
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
    fn paths_are_drawn_counted_and_framed() {
        let node = |x: f32, y: f32, kind: PathNodeKind| PathNodeMark { position: Vec3::new(x, y, 1.0), kind, junction: false };
        let scene = Scene {
            title: "nodes489".into(),
            path_nodes: vec![
                node(0.0, 0.0, PathNodeKind::Vehicle),
                node(6.0, 4.0, PathNodeKind::Ped),
                PathNodeMark { position: Vec3::new(6.0, 0.0, 1.0), kind: PathNodeKind::Disabled, junction: true },
                node(3.0, 2.0, PathNodeKind::Vehicle),
            ],
            path_links: vec![
                PathLinkShape { from: Vec3::new(0.0, 0.0, 1.0), to: Vec3::new(6.0, 0.0, 1.0), kind: PathLinkKind::Road, lanes: 4 },
                PathLinkShape { from: Vec3::new(6.0, 0.0, 1.0), to: Vec3::new(6.0, 4.0, 1.0), kind: PathLinkKind::Ped, lanes: 0 },
                PathLinkShape { from: Vec3::new(0.0, 0.0, 1.0), to: Vec3::new(6.0, 4.0, 1.0), kind: PathLinkKind::Shortcut, lanes: 1 },
            ],
            ..Default::default()
        };
        let opts = PlanOptions { scale: 10.0, ..Default::default() };
        let (img, report) = plan_png(&scene, &opts).expect("a plan");
        assert_eq!(report.region, [-2.0, -2.0, 8.0, 6.0], "the nodes frame the page");
        assert_eq!(report.drawn.iter().find(|(l, _)| *l == Layer::Paths).map(|(_, n)| *n), Some(4));

        // The road link runs along y = 0 from x 0 to 6: its middle is road-coloured.
        let layout = cartography::layout(report.region, 10.0, 1, 0);
        let (cx, cy) = layout.transform.to_px(3.0, 0.0);
        let p = img.get_pixel(cx as u32, cy as u32).0;
        assert_eq!(p, palette::PATH_ROAD, "no road ink at the middle of the link: {p:?}");

        let (svg, _) = plan_svg(&scene, &opts).expect("a plan");
        assert!(svg.contains("<line"), "links are lines in the SVG");
        assert!(svg.contains("stroke-dasharray"), "the shortcut is dashed");

        // Out of the band, nothing is drawn or framed.
        let opts = PlanOptions { z_band: Some((10.0, 12.0)), ..Default::default() };
        let err = plan_png(&scene, &opts).unwrap_err().to_string();
        assert!(err.contains("nothing to draw"), "{err}");
    }

    #[test]
    fn disabled_layers_are_not_drawn() {
        let opts = PlanOptions { layers: vec![Layer::Navmesh], ..Default::default() };
        let (_, report) = plan_png(&nav_scene(), &opts).expect("a plan");
        assert_eq!(report.drawn.len(), 1);
        assert_eq!(report.drawn[0].0, Layer::Navmesh);
    }
}
