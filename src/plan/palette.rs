//! Every colour the plan uses, in one place.

use super::canvas::Rgba8;

pub(crate) const BACKGROUND: Rgba8 = [255, 255, 255, 255];
pub(crate) const GRID_MINOR: Rgba8 = [0xEC, 0xEC, 0xEC, 255];
pub(crate) const GRID_MAJOR: Rgba8 = [0xD5, 0xD5, 0xD5, 255];
pub(crate) const AXIS_TEXT: Rgba8 = [0x55, 0x55, 0x55, 255];
pub(crate) const INK: Rgba8 = [0x20, 0x20, 0x20, 255];

pub(crate) const NAV_INTERIOR_FILL: Rgba8 = [90, 190, 110, 115]; // 0.45 alpha
pub(crate) const NAV_INTERIOR_STROKE: Rgba8 = [0x1E, 0x7A, 0x3C, 255];
pub(crate) const NAV_EXTERIOR_STROKE: Rgba8 = [0xC0, 0xC0, 0xC0, 255];
pub(crate) const NAV_SUNK_STROKE: Rgba8 = [0xD0, 0x28, 0x28, 255];
pub(crate) const NAV_SUNK_DASH: (f32, f32) = (4.0, 3.0);

pub(crate) const PORTAL_STROKE: Rgba8 = [0x6A, 0x1B, 0x9A, 255];
pub(crate) const PORTAL_DASH: (f32, f32) = (5.0, 3.0);
pub(crate) const LIMBO_STROKE: Rgba8 = [0x80, 0x80, 0x80, 255];
pub(crate) const LIMBO_DASH: (f32, f32) = (6.0, 4.0);

/// The ten categorical room hues, picked by room index.
pub(crate) const ROOM_HUES: [Rgba8; 10] = [
    [0x1F, 0x77, 0xB4, 255],
    [0xFF, 0x7F, 0x0E, 255],
    [0x2C, 0xA0, 0x2C, 255],
    [0xD6, 0x27, 0x28, 255],
    [0x94, 0x67, 0xBD, 255],
    [0x8C, 0x56, 0x4B, 255],
    [0xE3, 0x77, 0xC2, 255],
    [0x7F, 0x7F, 0x7F, 255],
    [0xBC, 0xBD, 0x22, 255],
    [0x17, 0xBE, 0xCF, 255],
];

/// The categorical hue for a room or entity-set index.
pub(crate) fn hue(index: usize) -> Rgba8 {
    ROOM_HUES[index % ROOM_HUES.len()]
}

/// `c` at the given 0..1 opacity.
pub(crate) fn with_alpha(c: Rgba8, alpha: f32) -> Rgba8 {
    [c[0], c[1], c[2], (alpha.clamp(0.0, 1.0) * 255.0).round() as u8]
}

/// Component-wise average of two colours, keeping `alpha`.
pub(crate) fn mix(a: Rgba8, b: Rgba8, alpha: f32) -> Rgba8 {
    with_alpha(
        [
            ((a[0] as u16 + b[0] as u16) / 2) as u8,
            ((a[1] as u16 + b[1] as u16) / 2) as u8,
            ((a[2] as u16 + b[2] as u16) / 2) as u8,
            255,
        ],
        alpha,
    )
}

/// A darker version of `c`, for entity rings.
pub(crate) fn darken(c: Rgba8) -> Rgba8 {
    [c[0] / 2, c[1] / 2, c[2] / 2, c[3]]
}

/// Which kind of surface a mesh triangle is, by its normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Facing {
    Floor,
    Wall,
    Slope,
}

/// Classifies a triangle normal: near-horizontal up is floor, near-vertical is
/// wall, anything else (including downward faces) is a slope.
pub(crate) fn facing(normal_z: f32) -> Facing {
    if normal_z > 0.7 {
        Facing::Floor
    } else if normal_z.abs() < 0.7 {
        Facing::Wall
    } else {
        Facing::Slope
    }
}

/// Which mesh a triangle came from; they are shaded in different families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mesh {
    Collision,
    Drawable,
}

impl Mesh {
    fn floor_ramp(self) -> (Rgba8, Rgba8) {
        match self {
            Mesh::Collision => ([0xB8, 0xC0, 0xCC, 255], [0xE4, 0xE8, 0xEE, 255]),
            Mesh::Drawable => ([0xEA, 0xDF, 0xCB, 255], [0xF4, 0xEC, 0xDC, 255]),
        }
    }

    fn wall(self) -> Rgba8 {
        match self {
            Mesh::Collision => [0x4B, 0x56, 0x73, 255],
            Mesh::Drawable => [0x8B, 0x6B, 0x4A, 255],
        }
    }

    fn slope(self) -> Rgba8 {
        match self {
            Mesh::Collision => [0x9A, 0xA3, 0xB5, 255],
            Mesh::Drawable => [0xC0, 0xA8, 0x88, 255],
        }
    }

    /// The legend swatch for this mesh: its wall colour.
    pub(crate) fn swatch(self) -> Rgba8 {
        self.wall()
    }

    /// The colour of a triangle facing `facing`, at height fraction `t` (0 at
    /// the bottom of the z range, 1 at the top; only floors use it).
    pub(crate) fn colour(self, facing: Facing, t: f32) -> Rgba8 {
        match facing {
            Facing::Floor => {
                let (lo, hi) = self.floor_ramp();
                let t = t.clamp(0.0, 1.0);
                [
                    (lo[0] as f32 + (hi[0] as f32 - lo[0] as f32) * t).round() as u8,
                    (lo[1] as f32 + (hi[1] as f32 - lo[1] as f32) * t).round() as u8,
                    (lo[2] as f32 + (hi[2] as f32 - lo[2] as f32) * t).round() as u8,
                    255,
                ]
            }
            Facing::Wall => self.wall(),
            Facing::Slope => self.slope(),
        }
    }
}
