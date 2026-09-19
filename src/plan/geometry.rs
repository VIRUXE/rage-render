//! World -> page mapping, z-band clipping and the small geometric questions
//! the plan asks of a scene.

use rage_formats::{Vec2, Vec3};

use super::{Layer, RoomShape, Scene, Tri};

/// Maps world XY to page pixels: `+Y` is up on the page, so world y is flipped.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Transform {
    /// World x at the left edge of the map area.
    pub(crate) x0: f32,
    /// World y at the top edge of the map area.
    pub(crate) y1: f32,
    /// Pixels per metre.
    pub(crate) scale: f32,
    /// Page x of the map area's left edge.
    pub(crate) ox: f32,
    /// Page y of the map area's top edge.
    pub(crate) oy: f32,
}

impl Transform {
    /// World `(x, y)` in page pixels.
    pub(crate) fn to_px(&self, x: f32, y: f32) -> (f32, f32) {
        (self.ox + (x - self.x0) * self.scale, self.oy + (self.y1 - y) * self.scale)
    }
}

/// Clips a convex polygon against `keep`, interpolating crossing edges.
fn clip_plane(poly: &[Vec3], keep: impl Fn(Vec3) -> f32) -> Vec<Vec3> {
    let mut out: Vec<Vec3> = Vec::with_capacity(poly.len() + 1);
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let (da, db) = (keep(a), keep(b));
        if da >= 0.0 {
            out.push(a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            let t = da / (da - db);
            out.push(Vec3::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t));
        }
    }
    out
}

/// Clips a triangle to the z band `[lo, hi]`, returning the resulting polygon,
/// or `None` when nothing of it survives.
pub fn clip_tri_to_band(t: &Tri, lo: f32, hi: f32) -> Option<Vec<Vec3>> {
    let poly = clip_plane(&t.v, |v| v.z - lo);
    if poly.len() < 3 {
        return None;
    }
    let poly = clip_plane(&poly, |v| hi - v.z);
    if poly.len() < 3 {
        None
    } else {
        Some(poly)
    }
}

/// The four bottom corners of an axis-aligned box, run through `map` and
/// flattened to XY — a room's footprint in world space.
pub(crate) fn quad_footprint(bb_min: Vec3, bb_max: Vec3, map: impl Fn(Vec3) -> Vec3) -> [Vec2; 4] {
    let z = bb_min.z;
    let corners = [
        Vec3::new(bb_min.x, bb_min.y, z),
        Vec3::new(bb_max.x, bb_min.y, z),
        Vec3::new(bb_max.x, bb_max.y, z),
        Vec3::new(bb_min.x, bb_max.y, z),
    ];
    let mut out = [Vec2::default(); 4];
    for (o, c) in out.iter_mut().zip(corners) {
        let m = map(c);
        *o = Vec2::new(m.x, m.y);
    }
    out
}

/// The XY bounding box of a footprint.
fn footprint_bbox(f: &[Vec2; 4]) -> [f32; 4] {
    let mut bb = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for v in f {
        bb[0] = bb[0].min(v.x);
        bb[1] = bb[1].min(v.y);
        bb[2] = bb[2].max(v.x);
        bb[3] = bb[3].max(v.y);
    }
    bb
}

/// Pairs of room indices that sit on top of each other: overlapping XY
/// footprints but disjoint z ranges. Room 0 (the limbo room) is ignored.
pub fn rooms_stacked(rooms: &[RoomShape]) -> Vec<(usize, usize)> {
    let live: Vec<&RoomShape> = rooms.iter().filter(|r| r.index != 0).collect();
    let mut out = Vec::new();
    for (i, a) in live.iter().enumerate() {
        for b in live.iter().skip(i + 1) {
            let (pa, pb) = (footprint_bbox(&a.footprint), footprint_bbox(&b.footprint));
            let ox = (pa[2].min(pb[2]) - pa[0].max(pb[0])).max(0.0);
            let oy = (pa[3].min(pb[3]) - pa[1].max(pb[1])).max(0.0);
            let disjoint_z = a.z_hi <= b.z_lo || b.z_hi <= a.z_lo;
            if ox * oy > 0.5 && disjoint_z {
                out.push((a.index, b.index));
            }
        }
    }
    out
}

/// Grows `bb` to include the point `(x, y)`.
fn grow(bb: &mut Option<[f32; 4]>, x: f32, y: f32) {
    if !x.is_finite() || !y.is_finite() {
        return;
    }
    match bb {
        Some(b) => {
            b[0] = b[0].min(x);
            b[1] = b[1].min(y);
            b[2] = b[2].max(x);
            b[3] = b[3].max(y);
        }
        None => *bb = Some([x, y, x, y]),
    }
}

fn in_band(z: f32, band: Option<(f32, f32)>) -> bool {
    match band {
        Some((lo, hi)) => z >= lo && z <= hi,
        None => true,
    }
}

/// World-space `x0,y0,x1,y1` covering every enabled layer within the band.
pub(crate) fn scene_bounds(scene: &Scene, layers: &[Layer], band: Option<(f32, f32)>) -> Option<[f32; 4]> {
    let mut bb = None;
    for layer in layers {
        match layer {
            Layer::Rooms => {
                for r in &scene.rooms {
                    if band.is_some_and(|(lo, hi)| r.z_hi < lo || r.z_lo > hi) {
                        continue;
                    }
                    for v in &r.footprint {
                        grow(&mut bb, v.x, v.y);
                    }
                }
            }
            Layer::Portals => {
                for p in &scene.portals {
                    let lo = p.corners.iter().fold(f32::MAX, |m, v| m.min(v.z));
                    let hi = p.corners.iter().fold(f32::MIN, |m, v| m.max(v.z));
                    if band.is_some_and(|(blo, bhi)| hi < blo || lo > bhi) {
                        continue;
                    }
                    for v in &p.corners {
                        grow(&mut bb, v.x, v.y);
                    }
                }
            }
            Layer::Entities => {
                for e in &scene.entities {
                    if in_band(e.position.z, band) {
                        grow(&mut bb, e.position.x, e.position.y);
                    }
                }
            }
            Layer::Collision | Layer::Drawable => {
                let tris = if *layer == Layer::Collision { &scene.collision } else { &scene.drawable };
                for t in tris {
                    match band {
                        Some((lo, hi)) => {
                            if let Some(poly) = clip_tri_to_band(t, lo, hi) {
                                for v in poly {
                                    grow(&mut bb, v.x, v.y);
                                }
                            }
                        }
                        None => {
                            for v in t.v {
                                grow(&mut bb, v.x, v.y);
                            }
                        }
                    }
                }
            }
            Layer::Navmesh => {
                for n in &scene.navmesh {
                    if !n.vertices.iter().any(|v| in_band(v.z, band)) {
                        continue;
                    }
                    for v in &n.vertices {
                        grow(&mut bb, v.x, v.y);
                    }
                }
            }
        }
    }
    for m in &scene.markers {
        grow(&mut bb, m.x, m.y);
    }
    bb
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall_tri() -> Tri {
        // A vertical wall spanning z 0..3 in the plane y = 1, x in 0..4.
        Tri { v: [Vec3::new(0.0, 1.0, 0.0), Vec3::new(4.0, 1.0, 0.0), Vec3::new(4.0, 1.0, 3.0)] }
    }

    #[test]
    fn clips_a_wall_to_the_band() {
        let poly = clip_tri_to_band(&wall_tri(), 0.5, 2.0).expect("wall crosses the band");
        assert!(poly.len() >= 3);
        for v in &poly {
            assert!(v.z >= 0.5 - 1e-4 && v.z <= 2.0 + 1e-4, "z {} outside band", v.z);
            assert!((v.y - 1.0).abs() < 1e-4);
        }
        let x_min = poly.iter().fold(f32::MAX, |m, v| m.min(v.x));
        let x_max = poly.iter().fold(f32::MIN, |m, v| m.max(v.x));
        assert!(x_max - x_min > 3.0, "XY extent collapsed: {x_min}..{x_max}");
    }

    #[test]
    fn drops_a_triangle_above_the_band() {
        assert!(clip_tri_to_band(&wall_tri(), 5.0, 6.0).is_none());
    }

    #[test]
    fn keeps_a_triangle_inside_the_band_whole() {
        let poly = clip_tri_to_band(&wall_tri(), -1.0, 9.0).expect("inside");
        assert_eq!(poly.len(), 3);
    }

    fn room(index: usize, z_lo: f32, z_hi: f32) -> RoomShape {
        RoomShape {
            index,
            name: format!("room{index}"),
            footprint: [
                Vec2::new(0.0, 0.0),
                Vec2::new(4.0, 0.0),
                Vec2::new(4.0, 4.0),
                Vec2::new(0.0, 4.0),
            ],
            z_lo,
            z_hi,
        }
    }

    #[test]
    fn finds_stacked_rooms_and_ignores_room_zero() {
        let rooms = vec![room(0, 0.0, 3.0), room(1, 0.0, 3.0), room(2, 4.0, 7.0)];
        assert_eq!(rooms_stacked(&rooms), vec![(1, 2)]);
    }

    #[test]
    fn overlapping_z_is_not_stacked() {
        let rooms = vec![room(1, 0.0, 3.0), room(2, 2.0, 7.0)];
        assert!(rooms_stacked(&rooms).is_empty());
    }
}
