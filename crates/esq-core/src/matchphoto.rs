//! Match Photo: a scene whose background is a photograph and whose camera is calibrated on it.
//!
//! The calibration is the classical one for a pinhole camera looking at a box-like scene:
//! two (or three) vanishing points of mutually orthogonal world axes give the focal length
//! (the principal point being the image centre, or the orthocentre of the three vanishing
//! points), then the rotation; the origin pixel and a scale give the position.
//!
//! Conventions: image pixels have their origin at the top-left corner, `y` pointing down.
//! The camera frame is `x` right, `y` down, `z` forward (computer-vision convention), and
//! a pixel is `(cx + f·x/z, cy + f·y/z)`. World units are millimetres.

use std::fmt;

use glam::{DMat3, DVec2, DVec3};
use serde::{Deserialize, Serialize};

use crate::model::SceneCamera;

/// A line drawn on the photo, from `a` to `b` (image pixels).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line2 {
    pub a: DVec2,
    pub b: DVec2,
}

/// How the position of the camera along its ray through the origin is fixed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Scale {
    /// Distance from the camera to the origin, in mm.
    Distance(f64),
    /// The point seen at `pixel` lies on the world axis `axis` (0 = red, 1 = green,
    /// 2 = blue) at `length` mm from the origin.
    Reference { axis: usize, pixel: DVec2, length: f64 },
}

/// Everything the user gives to the calibration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    /// Image size in pixels.
    pub size: DVec2,
    /// Optical centre; the image centre when `None`.
    pub principal: Option<DVec2>,
    /// For each world axis (red, green, blue), two lines parallel to it. Two axes at least.
    pub lines: [Option<[Line2; 2]>; 3],
    /// Pixel where the world origin is.
    pub origin: DVec2,
    pub scale: Scale,
    /// Which of the four right-handed orientations to take; `None` = the most plausible one
    /// (blue axis up in the image, red axis to the right, green axis away from the camera).
    pub variant: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolveError {
    /// Fewer than two axes have their two lines.
    NotEnoughAxes,
    /// Two lines of the same axis are identical.
    DegenerateLines,
    /// A vanishing point at infinity: the focal length cannot be recovered.
    VanishingPointAtInfinity,
    /// The vanishing points are not compatible with orthogonal axes (both on the same
    /// side of the optical centre).
    Inconsistent,
    /// The reference of the scale does not give a length.
    BadScale,
}

impl fmt::Display for SolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SolveError::NotEnoughAxes => "Il faut deux axes avec leurs deux lignes.",
            SolveError::DegenerateLines => "Deux lignes d'un même axe sont confondues.",
            SolveError::VanishingPointAtInfinity => "Un point de fuite est trop loin : écartez les lignes.",
            SolveError::Inconsistent => "Les points de fuite ne correspondent pas à des axes perpendiculaires.",
            SolveError::BadScale => "L'échelle est inutilisable.",
        })
    }
}

/// A calibrated camera.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhotoCamera {
    /// World → camera rotation; its columns are the world axes in the camera frame.
    pub rot: DMat3,
    /// World origin in the camera frame.
    pub trans: DVec3,
    /// Focal length in pixels.
    pub focal: f64,
    /// Optical centre in image pixels.
    pub principal: DVec2,
    /// Image size in pixels.
    pub size: DVec2,
}

impl PhotoCamera {
    pub fn eye(&self) -> DVec3 {
        -(self.rot.transpose() * self.trans)
    }

    /// Direction the camera looks at, `up` and `right` in world coordinates.
    pub fn forward(&self) -> DVec3 {
        self.rot.transpose() * DVec3::Z
    }
    pub fn right(&self) -> DVec3 {
        self.rot.transpose() * DVec3::X
    }
    pub fn up(&self) -> DVec3 {
        -(self.rot.transpose() * DVec3::Y)
    }

    /// World point → image pixel, if it is in front of the camera.
    pub fn project(&self, p: DVec3) -> Option<DVec2> {
        let c = self.rot * p + self.trans;
        (c.z > 1e-9).then(|| self.principal + DVec2::new(c.x, c.y) * (self.focal / c.z))
    }

    /// Ray through an image pixel: (origin, unit direction) in world coordinates.
    pub fn ray(&self, px: DVec2) -> (DVec3, DVec3) {
        let d = DVec3::new((px.x - self.principal.x) / self.focal, (px.y - self.principal.y) / self.focal, 1.0);
        (self.eye(), (self.rot.transpose() * d).normalize())
    }

    /// Vertical field of view of the photo, in degrees.
    pub fn fov_y_deg(&self) -> f64 {
        2.0 * (self.size.y * 0.5 / self.focal).atan().to_degrees()
    }

    /// Where the optical centre is, relative to the image centre, as a fraction of the
    /// image size (`(0, 0)` when the photo is not cropped).
    pub fn centre_offset(&self) -> DVec2 {
        (self.principal - self.size * 0.5) / self.size
    }

    /// The point of view to save in the scene; `distance` is how far the target is.
    pub fn scene_camera(&self, distance: f64) -> SceneCamera {
        let eye = self.eye();
        SceneCamera {
            eye,
            target: eye + self.forward() * distance,
            up: self.up(),
            fov_deg: self.fov_y_deg(),
            perspective: true,
        }
    }
}

/// Homogeneous vanishing point of two image lines; `None` if they coincide.
fn vanishing(lines: &[Line2; 2]) -> Option<DVec3> {
    let l = |ln: &Line2| DVec3::new(ln.a.x, ln.a.y, 1.0).cross(DVec3::new(ln.b.x, ln.b.y, 1.0));
    let (l1, l2) = (l(&lines[0]), l(&lines[1]));
    let v = l1.cross(l2);
    // Relative to the product of the line norms: coincident lines give ~0.
    (v.length() > 1e-12 * l1.length() * l2.length()).then_some(v)
}

/// Pixel position of a homogeneous vanishing point, `None` at infinity.
pub fn vanishing_pixel(lines: &[Line2; 2]) -> Option<DVec2> {
    let v = vanishing(lines)?;
    (v.z.abs() > 1e-9 * v.truncate().length().max(1e-300)).then(|| v.truncate() / v.z)
}

/// Orthocentre of a triangle (the optical centre when the three vanishing points are known).
fn orthocentre(a: DVec2, b: DVec2, c: DVec2) -> Option<DVec2> {
    // (h − a)·(b − c) = 0 and (h − b)·(a − c) = 0.
    let (n1, n2) = (b - c, a - c);
    let det = n1.x * n2.y - n1.y * n2.x;
    if det.abs() < 1e-12 {
        return None;
    }
    let (r1, r2) = (a.dot(n1), b.dot(n2));
    Some(DVec2::new((r1 * n2.y - r2 * n1.y) / det, (n1.x * r2 - n2.x * r1) / det))
}

/// Solves the camera. See the module documentation.
pub fn solve(s: &Setup) -> Result<PhotoCamera, SolveError> {
    let known: Vec<usize> = (0..3).filter(|&i| s.lines[i].is_some()).collect();
    if known.len() < 2 {
        return Err(SolveError::NotEnoughAxes);
    }
    let mut vp = [None::<DVec3>; 3];
    for &i in &known {
        let lines = s.lines[i].as_ref().expect("known");
        vp[i] = Some(vanishing(lines).ok_or(SolveError::DegenerateLines)?);
    }
    let finite = |v: DVec3| v.z.abs() > 1e-9 * v.truncate().length().max(1e-300);
    // Optical centre.
    let mut c = s.principal.unwrap_or(s.size * 0.5);
    if known.len() == 3 {
        let pts: Vec<DVec2> = vp.iter().flatten().filter(|v| finite(**v)).map(|v| v.truncate() / v.z).collect();
        if pts.len() == 3 {
            c = orthocentre(pts[0], pts[1], pts[2]).ok_or(SolveError::Inconsistent)?;
        }
    }
    // Focal length from each pair of orthogonal directions: f² = −(p_i − c)·(p_j − c).
    let mut f2 = Vec::new();
    for (n, &i) in known.iter().enumerate() {
        for &j in &known[n + 1..] {
            let (vi, vj) = (vp[i].expect("vp"), vp[j].expect("vp"));
            if !finite(vi) || !finite(vj) {
                continue;
            }
            let (pi, pj) = (vi.truncate() / vi.z - c, vj.truncate() / vj.z - c);
            f2.push(-pi.dot(pj));
        }
    }
    if f2.is_empty() {
        return Err(SolveError::VanishingPointAtInfinity);
    }
    let f2 = f2.iter().sum::<f64>() / f2.len() as f64;
    if f2 <= 1e-9 {
        return Err(SolveError::Inconsistent);
    }
    let focal = f2.sqrt();
    // Directions of the two first axes (up to sign), the third from the cross product.
    let dir = |v: DVec3| DVec3::new(v.x - c.x * v.z, v.y - c.y * v.z, focal * v.z).normalize();
    let (i, j) = (known[0], known[1]);
    let (di, dj) = (dir(vp[i].expect("vp")), dir(vp[j].expect("vp")));
    let build = |flip: u8| -> DMat3 {
        let a = if flip & 1 != 0 { -di } else { di };
        let b = if flip & 2 != 0 { -dj } else { dj };
        let mut cols = [DVec3::ZERO; 3];
        cols[i] = a;
        cols[j] = b;
        let k = 3 - i - j;
        // Right-handed: X×Y = Z, Y×Z = X, Z×X = Y.
        cols[k] = if j == (i + 1) % 3 { a.cross(b) } else { b.cross(a) };
        DMat3::from_cols(cols[0], cols[1], cols[2])
    };
    let variant = match s.variant {
        Some(v) => v & 3,
        None => (0..4u8)
            .max_by_key(|&v| {
                let r = build(v);
                let (x, y, z) = (r.col(0), r.col(1), r.col(2));
                // Blue axis up in the image counts most, then red to the right, then green
                // pointing away from the camera.
                (z.y < 0.0) as i32 * 4 + (x.x > 0.0) as i32 * 2 + (y.z > 0.0) as i32
            })
            .unwrap_or(0),
    };
    let rot = build(variant);
    // Position: the origin is on the ray through `s.origin`.
    let ray = |p: DVec2| DVec3::new((p.x - c.x) / focal, (p.y - c.y) / focal, 1.0).normalize();
    let r_o = ray(s.origin);
    let dist = match s.scale {
        Scale::Distance(d) => d,
        Scale::Reference { axis, pixel, length } => {
            // k·r_o + t·axis = λ·u for the unit scale k = 1: least squares for (t, λ).
            let (a, u) = (rot.col(axis.min(2)), ray(pixel));
            let (m00, m01, m11) = (a.dot(a), -a.dot(u), u.dot(u));
            let (b0, b1) = (-a.dot(r_o), u.dot(r_o));
            let det = m00 * m11 - m01 * m01;
            if det.abs() < 1e-12 {
                return Err(SolveError::BadScale);
            }
            let t = (b0 * m11 - m01 * b1) / det;
            if t.abs() < 1e-9 {
                return Err(SolveError::BadScale);
            }
            length.abs() / t.abs()
        }
    };
    if !(dist.is_finite() && dist > 0.0) {
        return Err(SolveError::BadScale);
    }
    Ok(PhotoCamera { rot, trans: r_o * dist, focal, principal: c, size: s.size })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rot_from_angles(yaw: f64, pitch: f64, roll: f64) -> DMat3 {
        // World → camera for a Z-up world, camera looking along `forward`.
        let forward = DVec3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), pitch.sin());
        let right0 = forward.cross(DVec3::Z).normalize();
        let up0 = right0.cross(forward);
        let right = right0 * roll.cos() + up0 * roll.sin();
        let up = up0 * roll.cos() - right0 * roll.sin();
        DMat3::from_cols(right, -up, forward).transpose()
    }

    fn camera(eye: DVec3, yaw: f64, pitch: f64, roll: f64, focal: f64, principal: DVec2) -> PhotoCamera {
        let rot = rot_from_angles(yaw, pitch, roll);
        PhotoCamera { rot, trans: -(rot * eye), focal, principal, size: DVec2::new(1600.0, 1200.0) }
    }

    /// Two lines parallel to `axis`, through points in front of the camera.
    fn lines_for(cam: &PhotoCamera, axis: usize, anchors: [DVec3; 2]) -> [Line2; 2] {
        let d = [DVec3::X, DVec3::Y, DVec3::Z][axis];
        let seg = |p: DVec3| {
            let a = cam.project(p - d * 700.0).expect("in front");
            let b = cam.project(p + d * 700.0).expect("in front");
            Line2 { a, b }
        };
        [seg(anchors[0]), seg(anchors[1])]
    }

    fn setup_for(cam: &PhotoCamera, axes: &[usize], scale: Scale) -> Setup {
        let anchors = |axis: usize| match axis {
            0 => [DVec3::new(0.0, 500.0, 0.0), DVec3::new(0.0, -300.0, 900.0)],
            1 => [DVec3::new(600.0, 0.0, 100.0), DVec3::new(-400.0, 0.0, 700.0)],
            _ => [DVec3::new(500.0, 400.0, 0.0), DVec3::new(-400.0, -300.0, 0.0)],
        };
        let mut lines = [None; 3];
        for &a in axes {
            lines[a] = Some(lines_for(cam, a, anchors(a)));
        }
        Setup {
            size: cam.size,
            principal: None,
            lines,
            origin: cam.project(DVec3::ZERO).expect("origin visible"),
            scale,
            variant: None,
        }
    }

    fn close(a: DVec3, b: DVec3, tol: f64) -> bool {
        (a - b).length() < tol
    }

    fn check(truth: &PhotoCamera, got: &PhotoCamera) {
        assert!((truth.focal - got.focal).abs() < 1e-6 * truth.focal, "focal {} vs {}", truth.focal, got.focal);
        assert!(close(truth.eye(), got.eye(), 1e-4), "eye {:?} vs {:?}", truth.eye(), got.eye());
        assert!(close(truth.forward(), got.forward(), 1e-9));
        assert!(close(truth.up(), got.up(), 1e-9));
        assert!((truth.principal - got.principal).length() < 1e-6);
        for p in [DVec3::new(300.0, 200.0, 100.0), DVec3::new(-500.0, 800.0, 300.0)] {
            assert!((truth.project(p).unwrap() - got.project(p).unwrap()).length() < 1e-6);
        }
    }

    #[test]
    fn recovers_a_camera_from_red_and_green() {
        let truth = camera(DVec3::new(4000.0, -6000.0, 1600.0), 1.9, -0.12, 0.0, 1500.0, DVec2::new(800.0, 600.0));
        let s = setup_for(&truth, &[0, 1], Scale::Distance(truth.trans.length()));
        check(&truth, &solve(&s).expect("solved"));
    }

    #[test]
    fn recovers_a_rolled_camera_from_red_and_blue() {
        let truth = camera(DVec3::new(-3000.0, -5000.0, 2200.0), 1.2, -0.2, 0.05, 1100.0, DVec2::new(800.0, 600.0));
        let s = setup_for(&truth, &[0, 2], Scale::Distance(truth.trans.length()));
        check(&truth, &solve(&s).expect("solved"));
    }

    #[test]
    fn recovers_a_camera_from_green_and_blue() {
        let truth = camera(
            DVec3::new(6000.0, 1000.0, 1700.0),
            std::f64::consts::PI - 0.3,
            -0.05,
            -0.03,
            2200.0,
            DVec2::new(800.0, 600.0),
        );
        let s = setup_for(&truth, &[1, 2], Scale::Distance(truth.trans.length()));
        check(&truth, &solve(&s).expect("solved"));
    }

    #[test]
    fn three_vanishing_points_give_the_optical_centre() {
        let truth = camera(
            DVec3::new(3500.0, -4500.0, 3000.0),
            1.7,
            -0.45,
            0.0,
            1300.0,
            DVec2::new(900.0, 540.0), // cropped photo: not the image centre
        );
        let mut s = setup_for(&truth, &[0, 1, 2], Scale::Distance(truth.trans.length()));
        s.principal = Some(DVec2::new(800.0, 600.0)); // wrong on purpose: ignored with 3 axes
        check(&truth, &solve(&s).expect("solved"));
        let off = solve(&s).unwrap().centre_offset();
        assert!((off - DVec2::new(100.0 / 1600.0, -60.0 / 1200.0)).length() < 1e-9);
    }

    #[test]
    fn reference_length_fixes_the_scale() {
        let truth = camera(DVec3::new(4000.0, -6000.0, 1600.0), 1.9, -0.12, 0.0, 1500.0, DVec2::new(800.0, 600.0));
        // A 2 m long segment on the red axis, seen from the origin.
        let pixel = truth.project(DVec3::new(2000.0, 0.0, 0.0)).unwrap();
        let s = setup_for(&truth, &[0, 1], Scale::Reference { axis: 0, pixel, length: 2000.0 });
        check(&truth, &solve(&s).expect("solved"));
    }

    #[test]
    fn explicit_variant_flips_the_orientation() {
        let truth = camera(DVec3::new(4000.0, -6000.0, 1600.0), 1.9, -0.12, 0.0, 1500.0, DVec2::new(800.0, 600.0));
        let mut s = setup_for(&truth, &[0, 1], Scale::Distance(7000.0));
        s.variant = Some(3); // both directions flipped: the scene turned by 180 degrees about blue
        let flipped = solve(&s).unwrap();
        assert!(close(flipped.rot.col(2), solve(&Setup { variant: None, ..s.clone() }).unwrap().rot.col(2), 1e-9));
        assert!(close(flipped.rot.col(0), -solve(&Setup { variant: None, ..s }).unwrap().rot.col(0), 1e-9));
    }

    #[test]
    fn rejects_bad_input() {
        let truth = camera(DVec3::new(4000.0, -6000.0, 1600.0), 1.9, -0.12, 0.0, 1500.0, DVec2::new(800.0, 600.0));
        let mut s = setup_for(&truth, &[0], Scale::Distance(1.0));
        assert_eq!(solve(&s), Err(SolveError::NotEnoughAxes));
        s = setup_for(&truth, &[0, 1], Scale::Distance(-3.0));
        assert_eq!(solve(&s), Err(SolveError::BadScale));
        // Same line twice.
        s = setup_for(&truth, &[0, 1], Scale::Distance(1000.0));
        let l = s.lines[0].unwrap()[0];
        s.lines[0] = Some([l, l]);
        assert_eq!(solve(&s), Err(SolveError::DegenerateLines));
    }

    #[test]
    fn scene_camera_looks_at_the_origin() {
        let truth = camera(DVec3::new(4000.0, -6000.0, 1600.0), 1.9, -0.12, 0.0, 1500.0, DVec2::new(800.0, 600.0));
        let sc = truth.scene_camera(1000.0);
        assert!(close((sc.target - sc.eye) / 1000.0, truth.forward(), 1e-9));
        assert!((sc.fov_deg - 2.0 * (600.0f64 / 1500.0).atan().to_degrees()).abs() < 1e-9);
    }
}
