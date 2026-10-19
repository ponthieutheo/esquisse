//! Angles typed or picked with a protractor (Rotate, after CRotateTool).

use std::f64::consts::{PI, TAU};

/// Protractor radius on screen, in pixels (the cursor is "on the rim" within `SNAP_TOLERANCE_PX`).
pub const PROTRACTOR_RADIUS_PX: f64 = 75.0;
/// Tolerance of the protractor's angle snapping, in pixels.
pub const SNAP_TOLERANCE_PX: f64 = 6.0;
/// Default angle snapping step (the model's "angle snapping" setting).
pub const DEFAULT_SNAP_STEP: f64 = 15.0 * PI / 180.0;

/// Angle picked by the cursor on a protractor.
///
/// `angle` is the free angle of the cursor around the centre, `r` its distance to the centre,
/// `radius` the protractor radius and `tol` the snapping tolerance (same unit as `r`).
/// The angle snaps to multiples of `step` when the cursor is on the rim (within `tol` of
/// `radius`) or within `tol` of a snapped direction; elsewhere it stays free (rounded to 0.1 deg).
pub fn protractor_angle(angle: f64, r: f64, radius: f64, tol: f64, step: f64) -> f64 {
    let snapped = (angle.abs() / step).round() * step * angle.signum();
    let chord = 2.0 * r * ((angle - snapped).abs() * 0.5).sin();
    if (r - radius).abs() > tol && chord > tol {
        let tenth = (1.0f64).to_radians() * 0.1;
        (angle / tenth).round() * tenth
    } else {
        snapped
    }
}

/// Keeps `angle` continuous with the previous one: the cursor may go past 180 degrees
/// (rotations go up to a full turn either way).
pub fn continuous_angle(raw: f64, previous: f64) -> f64 {
    let mut a = raw;
    while a - previous > PI {
        a -= TAU;
    }
    while previous - a > PI {
        a += TAU;
    }
    if a.abs() >= TAU { a - TAU * a.signum() } else { a }
}

/// An angle typed in the Measurements box: degrees ("45", "-12,5", "30°"), or a slope as
/// rise:run ("3:12" is the angle whose tangent is 3/12). The result is in radians.
pub fn parse_angle(text: &str) -> Option<f64> {
    let t = text.trim().trim_end_matches(['°', 'd']).trim().replace(',', ".");
    if let Some((rise, run)) = t.split_once(':') {
        let rise: f64 = rise.trim().parse().ok()?;
        let run: f64 = run.trim().parse().ok()?;
        if run == 0.0 && rise == 0.0 {
            return None;
        }
        return Some(rise.atan2(run));
    }
    let deg: f64 = t.parse().ok()?;
    deg.is_finite().then(|| deg.to_radians())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deg(a: f64) -> f64 {
        a.to_radians()
    }

    #[test]
    fn snaps_everywhere_on_the_rim() {
        // 37 degrees on the rim: nearest multiple of 15 is 30.
        let a = protractor_angle(deg(37.0), 75.0, 75.0, 6.0, DEFAULT_SNAP_STEP);
        assert!((a - deg(30.0)).abs() < 1e-9);
        let a = protractor_angle(deg(-52.0), 78.0, 75.0, 6.0, DEFAULT_SNAP_STEP);
        assert!((a + deg(45.0)).abs() < 1e-9);
    }

    #[test]
    fn inside_the_rim_the_angle_is_free_unless_close_to_a_snapped_direction() {
        // Inside the rim, 37 degrees: 7 px away from the 30 degrees direction -> free, 0.1 degree steps.
        let a = protractor_angle(deg(37.04), 60.0, 75.0, 6.0, DEFAULT_SNAP_STEP);
        assert!((a - deg(37.0)).abs() < 1e-9, "{}", a.to_degrees());
        // Within 6 px of the 30 degrees ray: snaps (chord = 2 r sin(da/2) = 40 * 0.0698 = 2.8 px).
        let a = protractor_angle(deg(34.0), 40.0, 75.0, 6.0, DEFAULT_SNAP_STEP);
        assert!((a - deg(30.0)).abs() < 1e-9, "{}", a.to_degrees());
    }

    #[test]
    fn far_outside_only_the_rays_snap() {
        let free = protractor_angle(deg(37.0), 200.0, 75.0, 6.0, DEFAULT_SNAP_STEP);
        assert!((free - deg(37.0)).abs() < 1e-9);
        let on_ray = protractor_angle(deg(30.5), 200.0, 75.0, 6.0, DEFAULT_SNAP_STEP);
        assert!((on_ray - deg(30.0)).abs() < 1e-9);
    }

    #[test]
    fn angles_stay_continuous_past_half_a_turn() {
        let a = continuous_angle(deg(-170.0), deg(170.0));
        assert!((a - deg(190.0)).abs() < 1e-9);
        let b = continuous_angle(deg(100.0), deg(190.0));
        assert!((b - deg(100.0)).abs() < 1e-9);
        // A full turn wraps.
        let c = continuous_angle(deg(5.0), deg(355.0));
        assert!(c.abs() < deg(10.0), "{}", c.to_degrees());
    }

    #[test]
    fn typed_angles_are_degrees_or_slopes() {
        assert!((parse_angle("45").unwrap() - deg(45.0)).abs() < 1e-12);
        assert!((parse_angle("-12,5°").unwrap() + deg(12.5)).abs() < 1e-12);
        assert!((parse_angle("1:1").unwrap() - deg(45.0)).abs() < 1e-12);
        assert!((parse_angle("3:12").unwrap() - (0.25f64).atan()).abs() < 1e-12);
        assert!(parse_angle("abc").is_none());
        assert!(parse_angle("0:0").is_none());
    }
}
