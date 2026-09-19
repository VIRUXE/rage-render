//! Painting the scene's own content: the mesh underlay, navmesh, rooms,
//! portals, entities and markers, plus the label requests they raise.

use image::RgbaImage;
use rage_formats::Vec3;

use super::canvas::{Canvas, TextSize};
use super::cartography::LabelRequest;
use super::geometry::Transform;
use super::palette::{self, Facing};
use super::raster::RasterCanvas;
use super::{NavClass, PlanOptions, Prepared, Scene};

/// World polygon -> page points.
fn to_px(poly: &[Vec3], t: &Transform) -> Vec<(f32, f32)> {
    poly.iter().map(|v| t.to_px(v.x, v.y)).collect()
}

/// Rasterises the collision and drawable triangles into one transparent
/// overlay the size of the map area, or `None` when nothing was drawn.
pub(crate) fn mesh_underlay(prep: &Prepared) -> Option<RgbaImage> {
    if prep.mesh.is_empty() {
        return None;
    }
    let map = prep.layout.map;
    let t = Transform { ox: 0.0, oy: 0.0, ..prep.layout.transform };
    let mut canvas = RasterCanvas::transparent(map.w as u32, map.h as u32);

    // Floors bottom-up first so upper storeys read as lighter, then the
    // slopes, then the walls, which are the outlines that carry the shape.
    let (lo, hi) = prep.mesh.iter().fold((f32::MAX, f32::MIN), |(lo, hi), m| (lo.min(m.z), hi.max(m.z)));
    let span = if hi - lo > 1e-3 { hi - lo } else { 1.0 };

    let mut floors: Vec<&super::MeshPoly> = prep.mesh.iter().filter(|m| m.facing == Facing::Floor).collect();
    floors.sort_by(|a, b| a.z.partial_cmp(&b.z).unwrap_or(std::cmp::Ordering::Equal));
    let rest = [Facing::Slope, Facing::Wall];

    for m in floors.into_iter().chain(rest.iter().flat_map(|f| prep.mesh.iter().filter(move |m| m.facing == *f))) {
        let colour = m.mesh.colour(m.facing, (m.z - lo) / span);
        canvas.fill_polygon(&to_px(&m.poly, &t), colour);
    }
    Some(canvas.img)
}

/// Navmesh polygons, exterior first so the interior reads on top.
pub(crate) fn navmesh(scene: &Scene, prep: &Prepared, canvas: &mut dyn Canvas) {
    let t = &prep.layout.transform;
    for class in [NavClass::Exterior, NavClass::Interior, NavClass::Sunk] {
        for shape in prep.navmesh.iter().map(|i| &scene.navmesh[*i]).filter(|s| s.class == class) {
            let pts = to_px(&shape.vertices, t);
            match class {
                NavClass::Interior => {
                    canvas.fill_polygon(&pts, palette::NAV_INTERIOR_FILL);
                    canvas.stroke_polygon(&pts, palette::NAV_INTERIOR_STROKE, 1.0, None);
                }
                NavClass::Exterior => canvas.stroke_polygon(&pts, palette::NAV_EXTERIOR_STROKE, 1.0, None),
                NavClass::Sunk => {
                    canvas.stroke_polygon(&pts, palette::NAV_SUNK_STROKE, 1.0, Some(palette::NAV_SUNK_DASH))
                }
            }
        }
    }
}

/// Room footprints, and one label request each.
pub(crate) fn rooms(scene: &Scene, prep: &Prepared, canvas: &mut dyn Canvas, labels: &mut Vec<LabelRequest>) {
    let t = &prep.layout.transform;
    for room in prep.rooms.iter().map(|i| &scene.rooms[*i]) {
        let pts: Vec<(f32, f32)> = room.footprint.iter().map(|v| t.to_px(v.x, v.y)).collect();
        let colour = palette::hue(room.index);
        if room.index == 0 {
            canvas.stroke_polygon(&pts, palette::LIMBO_STROKE, 2.0, Some(palette::LIMBO_DASH));
        } else {
            canvas.fill_polygon(&pts, palette::with_alpha(colour, 0.10));
            canvas.stroke_polygon(&pts, colour, 2.0, None);
        }
        let cx = pts.iter().map(|p| p.0).sum::<f32>() / pts.len() as f32;
        let cy = pts.iter().map(|p| p.1).sum::<f32>() / pts.len() as f32;
        let text = if room.name.is_empty() {
            format!("r{}", room.index)
        } else {
            format!("r{} {}", room.index, room.name)
        };
        labels.push(LabelRequest {
            anchor: (cx, cy),
            text,
            size: TextSize::Small,
            color: if room.index == 0 { palette::LIMBO_STROKE } else { palette::darken(colour) },
            priority: 1,
            centred_first: true,
        });
    }
}

/// Portal polygons, labelled `P4 3->7` when labels are on.
pub(crate) fn portals(scene: &Scene, prep: &Prepared, opts: &PlanOptions, canvas: &mut dyn Canvas, labels: &mut Vec<LabelRequest>) {
    let t = &prep.layout.transform;
    for portal in prep.portals.iter().map(|i| &scene.portals[*i]) {
        let pts = to_px(&portal.corners, t);
        if pts.len() < 2 {
            continue;
        }
        let limbo = portal.room_from == 0 || portal.room_to == 0;
        let fill = palette::mix(palette::hue(portal.room_from), palette::hue(portal.room_to), 0.35);
        canvas.fill_polygon(&pts, fill);
        let dash = if limbo { Some(palette::PORTAL_DASH) } else { None };
        canvas.stroke_polygon(&pts, palette::PORTAL_STROKE, 2.0, dash);

        if opts.labels {
            let cx = pts.iter().map(|p| p.0).sum::<f32>() / pts.len() as f32;
            let cy = pts.iter().map(|p| p.1).sum::<f32>() / pts.len() as f32;
            labels.push(LabelRequest {
                anchor: (cx, cy),
                text: format!("P{} {}\u{2192}{}", portal.index, portal.room_from, portal.room_to),
                size: TextSize::Small,
                color: palette::PORTAL_STROKE,
                priority: 2,
                centred_first: false,
            });
        }
    }
}

/// Entity dots, coloured by entity set.
pub(crate) fn entities(scene: &Scene, prep: &Prepared, opts: &PlanOptions, canvas: &mut dyn Canvas, labels: &mut Vec<LabelRequest>) {
    let t = &prep.layout.transform;
    for entity in prep.entities.iter().map(|i| &scene.entities[*i]) {
        let (px, py) = t.to_px(entity.position.x, entity.position.y);
        let alpha = if entity.faded { 0.4 } else { 1.0 };
        let (fill, ring) = match entity.set {
            Some(k) => {
                let c = palette::hue(k);
                (palette::with_alpha(c, alpha), Some(palette::with_alpha(palette::darken(c), alpha)))
            }
            None => (palette::with_alpha(palette::INK, alpha), None),
        };
        canvas.circle((px, py), 3.0, fill, ring);

        if opts.labels && !entity.label.is_empty() {
            labels.push(LabelRequest {
                anchor: (px, py),
                text: entity.label.clone(),
                size: TextSize::Small,
                color: palette::with_alpha(palette::INK, alpha),
                priority: 3,
                centred_first: false,
            });
        }
    }
}

/// Caller markers: a black diamond and a haloed label, always drawn.
pub(crate) fn markers(scene: &Scene, prep: &Prepared, canvas: &mut dyn Canvas, labels: &mut Vec<LabelRequest>) {
    let t = &prep.layout.transform;
    for marker in &scene.markers {
        let (px, py) = t.to_px(marker.x, marker.y);
        let r = 5.0;
        canvas.fill_polygon(&[(px, py - r), (px + r, py), (px, py + r), (px - r, py)], palette::INK);
        if !marker.label.is_empty() {
            labels.push(LabelRequest {
                anchor: (px, py),
                text: marker.label.clone(),
                size: TextSize::Body,
                color: palette::INK,
                priority: 0,
                centred_first: false,
            });
        }
    }
}
