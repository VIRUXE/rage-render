//! Camera placement: fits a drawable's bounds into one of the fixed views.

use rage_formats::math::{Mat4, Vec3};
use crate::render::View;
use rage_formats::ydd::DrawableBounds;

/// Direction the light travels *toward* — i.e. the vector used in `dot(n, l)`.
const LIGHT_DIR: Vec3 = Vec3 { x: 0.4, y: -0.6, z: 0.8 };

/// The radius the camera fits into the frame: the stored sphere radius, or
/// the box half-diagonal when that is larger, never below 1e-3. Shared with
/// `render::cluster`'s shrink guard so the two agree by construction.
pub(crate) fn fit_radius(bounds: &DrawableBounds) -> f32 {
    let half_diagonal = (bounds.box_max - bounds.box_min).length() * 0.5;
    let radius = bounds.sphere_radius.max(half_diagonal).max(1e-3);
    if radius.is_finite() {
        radius
    } else {
        1.0
    }
}

/// Builds the view-projection matrix that frames `bounds` from `view`, for
/// a model facing +Y (the world's forward axis).
///
/// The world is Z-up with +Y forward. Returns the combined matrix, the eye
/// position and the normalized light direction.
pub(crate) fn camera_for(
    bounds: &DrawableBounds,
    view: View,
    aspect: f32,
    fov_deg: f32,
    margin: f32,
) -> (Mat4, Vec3, Vec3) {
    camera_for_facing(bounds, view, 1.0, aspect, fov_deg, margin)
}

/// The direction from the model's centre toward the eye for `view`, on a
/// model whose front points along `forward` on the Y axis (`1.0` or `-1.0`).
///
/// A relative view is an azimuth measured clockwise from the front as seen
/// from above (so `90` is the model's own right side) and an elevation above
/// the horizon. `Top` and `Iso` are world-fixed.
pub(crate) fn eye_direction(view: View, forward: f32) -> Vec3 {
    let forward = if forward < 0.0 { -1.0 } else { 1.0 };
    match view {
        View::Top => Vec3::new(0.0, 0.0, 1.0),
        View::Iso => Vec3::new(1.0, -1.0, 1.0).normalize(),
        _ => {
            let (azimuth, elevation) = view.angles().unwrap_or((0, 0));
            let (sin_az, cos_az) = (azimuth as f32).to_radians().sin_cos();
            let (sin_el, cos_el) = (elevation as f32).to_radians().sin_cos();
            // The model's own axes: its front and its right-hand side.
            let front = Vec3::new(0.0, forward, 0.0);
            let right = Vec3::new(forward, 0.0, 0.0);
            let mut direction = (front * cos_az + right * sin_az) * cos_el + Vec3::Z * sin_el;
            // Snap the exact axes the named views promise.
            for component in [&mut direction.x, &mut direction.y, &mut direction.z] {
                if component.abs() < 1e-6 {
                    *component = 0.0;
                }
            }
            direction.normalize()
        }
    }
}

/// [`camera_for`] for a model facing `forward` on the Y axis: `1.0` for a
/// vehicle, `-1.0` for a prop.
pub(crate) fn camera_for_facing(
    bounds: &DrawableBounds,
    view: View,
    forward: f32,
    aspect: f32,
    fov_deg: f32,
    margin: f32,
) -> (Mat4, Vec3, Vec3) {
    let aspect = if aspect.is_finite() && aspect > 1e-4 { aspect } else { 1.0 };
    let margin = if margin.is_finite() && margin > 0.0 { margin } else { 1.0 };
    let fov = fov_deg.clamp(1.0, 170.0).to_radians();

    let center = bounds.center;
    let radius = fit_radius(bounds);

    let mut distance = radius / (fov * 0.5).sin() * margin;
    if aspect < 1.0 {
        distance /= aspect;
    }

    let direction = eye_direction(view, forward);

    let eye = center + direction * distance;
    // Looking straight down (or up), +Z is no longer a usable up vector: the
    // model's front goes to the top of the image instead, as `Top` always
    // put +Y there.
    let up = if view == View::Top {
        Vec3::Y
    } else if direction.x.abs() < 1e-6 && direction.y.abs() < 1e-6 {
        Vec3::new(0.0, if forward < 0.0 { -1.0 } else { 1.0 }, 0.0)
    } else {
        Vec3::Z
    };

    let near = (distance - 2.0 * radius).max(0.01);
    let far = distance + 2.0 * radius;

    let projection = Mat4::perspective_rh(fov, aspect, near, far);
    let look = Mat4::look_at_rh(eye, center, up);

    (projection.mul(&look), eye, LIGHT_DIR.normalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rage_formats::math::Vec4;

    fn bounds() -> DrawableBounds {
        DrawableBounds {
            center: Vec3::ZERO,
            sphere_radius: 1.0,
            box_min: Vec3::new(-1.0, -1.0, -1.0),
            box_max: Vec3::new(1.0, 1.0, 1.0),
        }
    }

    fn ndc(m: &Mat4, p: Vec3) -> Vec4 {
        let c = m.transform_point(p);
        Vec4::new(c.x / c.w, c.y / c.w, c.z / c.w, c.w)
    }

    /// Pins the exact eye direction and distance of every view, so a swapped
    /// axis or sign cannot hide behind a symmetric model.
    #[test]
    fn eye_sits_along_the_documented_direction_at_the_fitted_distance() {
        let bounds = DrawableBounds {
            center: Vec3::new(2.0, -3.0, 5.0),
            sphere_radius: 1.0,
            box_min: Vec3::new(1.0, -4.0, 4.0),
            box_max: Vec3::new(3.0, -2.0, 6.0),
        };
        let fov_deg = 40.0f32;
        let margin = 1.1f32;

        // r = max(sphere radius, half box diagonal) with a 2x2x2 box.
        let radius = (Vec3::new(2.0, 2.0, 2.0).length() * 0.5).max(1.0);
        let expected_distance = radius / (fov_deg.to_radians() * 0.5).sin() * margin;

        let expected = [
            (View::Front, Vec3::new(0.0, 1.0, 0.0)),
            (View::Back, Vec3::new(0.0, -1.0, 0.0)),
            (View::Left, Vec3::new(-1.0, 0.0, 0.0)),
            (View::Right, Vec3::new(1.0, 0.0, 0.0)),
            (View::Top, Vec3::new(0.0, 0.0, 1.0)),
            (View::Iso, Vec3::new(1.0, -1.0, 1.0).normalize()),
        ];

        for (view, direction) in expected {
            let (_, eye, _) = camera_for(&bounds, view, 1.0, fov_deg, margin);
            let offset = eye - bounds.center;
            assert!(
                (offset.length() - expected_distance).abs() < 1e-3,
                "{view}: distance {} != {expected_distance}",
                offset.length()
            );
            let unit = offset.normalize();
            assert!(
                (unit - direction).length() < 1e-5,
                "{view}: direction {unit:?} != {direction:?}"
            );
        }
    }

    /// The relative views turn with the model's facing: a prop's front is
    /// seen from -Y and its right side from -X, while Top and Iso stay put.
    #[test]
    fn relative_views_follow_the_facing() {
        let dir = |view, forward| eye_direction(view, forward);
        assert_eq!(dir(View::Front, 1.0), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(dir(View::Front, -1.0), Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(dir(View::Back, -1.0), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(dir(View::Right, 1.0), Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(dir(View::Right, -1.0), Vec3::new(-1.0, 0.0, 0.0));
        assert_eq!(dir(View::Left, -1.0), Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(dir(View::Top, -1.0), Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(dir(View::Iso, -1.0), Vec3::new(1.0, -1.0, 1.0).normalize());
        assert_eq!(dir(View::Angle { azimuth: 90, elevation: 0 }, 1.0), dir(View::Right, 1.0));
        assert_eq!(dir(View::Angle { azimuth: 180, elevation: 0 }, -1.0), dir(View::Back, -1.0));

        // A front-right three-quarter view from a little above, on a vehicle.
        let d = dir(View::Angle { azimuth: 30, elevation: 20 }, 1.0);
        assert!(d.x > 0.0 && d.y > 0.0 && d.z > 0.0, "{d:?}");
        assert!((d.length() - 1.0).abs() < 1e-5);
        let expected = Vec3::new(30f32.to_radians().sin() * 20f32.to_radians().cos(), 30f32.to_radians().cos() * 20f32.to_radians().cos(), 20f32.to_radians().sin());
        assert!((d - expected).length() < 1e-5, "{d:?} != {expected:?}");

        // Straight down from an angle still produces a usable camera.
        let (view_proj, _, _) = camera_for_facing(&bounds(), View::Angle { azimuth: 0, elevation: 90 }, -1.0, 1.0, 40.0, 1.1);
        let p = ndc(&view_proj, Vec3::new(0.0, -0.5, 0.0));
        assert!(p.w > 0.0 && p.y > 0.0, "a prop's front is at the top of a straight-down view: {p:?}");
    }

    /// The up vector: +Z is screen-up everywhere except Top, which uses +Y.
    #[test]
    fn up_axis_points_up_on_screen() {
        let up_axis = |view| match view {
            View::Top => Vec3::Y,
            _ => Vec3::Z,
        };

        for view in View::ALL {
            let (view_proj, _, _) = camera_for(&bounds(), view, 1.0, 40.0, 1.1);
            let p = ndc(&view_proj, up_axis(view) * 0.5);
            assert!(p.w > 0.0, "{view}: up sample behind the camera");
            assert!(p.y > 1e-3, "{view}: up axis did not project upward ({p:?})");
        }
    }

    #[test]
    fn every_view_places_the_eye_outside_the_bounds() {
        for view in View::ALL {
            let (_, eye, _) = camera_for(&bounds(), view, 1.0, 40.0, 1.1);
            assert!(eye.length() > 1.0, "{view}: eye inside bounds at {eye:?}");
        }
    }

    #[test]
    fn center_projects_in_front_of_the_camera_and_inside_ndc() {
        for view in View::ALL {
            let (view_proj, _, _) = camera_for(&bounds(), view, 1.0, 40.0, 1.1);
            let p = ndc(&view_proj, Vec3::ZERO);
            assert!(p.w > 0.0, "{view}: centre behind the camera");
            assert!(p.x.abs() < 1e-4 && p.y.abs() < 1e-4, "{view}: centre off-screen {p:?}");
            assert!(p.z > -1.0 && p.z < 1.0, "{view}: centre outside the depth range");
        }
    }

    #[test]
    fn bounds_fit_inside_the_frustum_with_margin() {
        for view in View::ALL {
            let (view_proj, _, _) = camera_for(&bounds(), view, 1.0, 40.0, 1.1);
            for corner in [
                Vec3::new(-1.0, -1.0, -1.0),
                Vec3::new(1.0, -1.0, -1.0),
                Vec3::new(-1.0, 1.0, -1.0),
                Vec3::new(1.0, 1.0, -1.0),
                Vec3::new(-1.0, -1.0, 1.0),
                Vec3::new(1.0, -1.0, 1.0),
                Vec3::new(-1.0, 1.0, 1.0),
                Vec3::new(1.0, 1.0, 1.0),
            ] {
                let p = ndc(&view_proj, corner);
                assert!(p.w > 0.0, "{view}: corner {corner:?} behind the camera");
                assert!(p.x.abs() <= 1.0 && p.y.abs() <= 1.0, "{view}: corner clipped {p:?}");
            }
        }
    }

    #[test]
    fn narrow_aspect_pulls_the_camera_back() {
        let (_, wide_eye, _) = camera_for(&bounds(), View::Front, 1.0, 40.0, 1.0);
        let (_, tall_eye, _) = camera_for(&bounds(), View::Front, 0.5, 40.0, 1.0);
        assert!(tall_eye.length() > wide_eye.length());
    }

    #[test]
    fn degenerate_bounds_still_produce_a_finite_camera() {
        let degenerate = DrawableBounds {
            center: Vec3::ZERO,
            sphere_radius: 0.0,
            box_min: Vec3::ZERO,
            box_max: Vec3::ZERO,
        };
        let (view_proj, eye, light) = camera_for(&degenerate, View::Iso, 1.0, 40.0, 1.1);
        assert!(view_proj.0.iter().all(|v| v.is_finite()));
        assert!(eye.length().is_finite() && eye.length() > 0.0);
        assert!((light.length() - 1.0).abs() < 1e-5);
    }

    /// `fit_radius` is what `camera_for` actually puts into the distance
    /// formula; pin the extraction against both branches of the max().
    #[test]
    fn fit_radius_matches_the_distance_the_camera_uses() {
        let sphere_dominant = DrawableBounds {
            center: Vec3::ZERO,
            sphere_radius: 5.0,
            box_min: Vec3::new(-1.0, -1.0, -1.0),
            box_max: Vec3::new(1.0, 1.0, 1.0),
        };
        assert!((fit_radius(&sphere_dominant) - 5.0).abs() < 1e-5);

        let box_dominant = DrawableBounds {
            center: Vec3::ZERO,
            sphere_radius: 0.1,
            box_min: Vec3::new(-2.0, -2.0, -2.0),
            box_max: Vec3::new(2.0, 2.0, 2.0),
        };
        let expected_half_diagonal = (Vec3::new(4.0, 4.0, 4.0).length()) * 0.5;
        assert!((fit_radius(&box_dominant) - expected_half_diagonal).abs() < 1e-4);

        for bounds in [&sphere_dominant, &box_dominant] {
            let fov_deg = 40.0f32;
            let margin = 1.1f32;
            let (_, eye, _) = camera_for(bounds, View::Front, 1.0, fov_deg, margin);
            let expected_distance = fit_radius(bounds) / (fov_deg.to_radians() * 0.5).sin() * margin;
            assert!(
                (eye.length() - expected_distance).abs() < 1e-3,
                "distance {} != {expected_distance}", eye.length()
            );
        }
    }
}
