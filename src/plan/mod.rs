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
mod palette;
mod raster;
mod svg;

use std::str::FromStr;

use image::RgbaImage;
use rage_formats::{Vec2, Vec3};

pub use geometry::{clip_tri_to_band, rooms_stacked};

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

/// Draws the plan as an image.
pub fn plan_png(_scene: &Scene, _opts: &PlanOptions) -> anyhow::Result<(RgbaImage, PlanReport)> {
    todo!()
}

/// Draws the plan as a hybrid SVG: vector page, mesh underlay as one PNG.
pub fn plan_svg(_scene: &Scene, _opts: &PlanOptions) -> anyhow::Result<(String, PlanReport)> {
    todo!()
}
