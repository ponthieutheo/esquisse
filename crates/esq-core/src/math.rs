//! Geometric primitives and tolerances. All lengths are millimetres.

use glam::{DMat4, DVec2, DVec3};
use serde::{Deserialize, Serialize};

/// Two points closer than this are the same point (SketchUp uses 0.001").
pub const TOL: f64 = 0.0254;
/// Tolerance on unit-vector comparisons.
pub const ANG_TOL: f64 = 1e-6;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub n: DVec3,
    pub d: f64,
}

impl Plane {
    pub fn from_point_normal(p: DVec3, n: DVec3) -> Self {
        let n = n.normalize();
        Plane { n, d: n.dot(p) }
    }

    /// Same plane with the normal sign fixed, so two descriptions of one plane compare equal.
    pub fn canonical(self) -> Self {
        let n = self.n;
        let flip = if n.x.abs() > ANG_TOL {
            n.x < 0.0
        } else if n.y.abs() > ANG_TOL {
            n.y < 0.0
        } else {
            n.z < 0.0
        };
        if flip { Plane { n: -n, d: -self.d } } else { self }
    }

    pub fn distance(&self, p: DVec3) -> f64 {
        self.n.dot(p) - self.d
    }

    pub fn contains(&self, p: DVec3) -> bool {
        self.distance(p).abs() < TOL
    }

    pub fn same_as(&self, o: &Plane) -> bool {
        let a = self.canonical();
        let b = o.canonical();
        a.n.dot(b.n) > 1.0 - 1e-9 && (a.d - b.d).abs() < TOL
    }

    pub fn origin(&self) -> DVec3 {
        self.n * self.d
    }

    /// Orthonormal (u, v) such that u × v = n.
    pub fn basis(&self) -> (DVec3, DVec3) {
        basis_for(self.n)
    }

    pub fn to_2d(&self, p: DVec3) -> DVec2 {
        let (u, v) = self.basis();
        DVec2::new(p.dot(u), p.dot(v))
    }

    pub fn intersect_ray(&self, origin: DVec3, dir: DVec3) -> Option<f64> {
        let denom = self.n.dot(dir);
        if denom.abs() < 1e-12 {
            return None;
        }
        Some((self.d - self.n.dot(origin)) / denom)
    }
}

pub fn basis_for(n: DVec3) -> (DVec3, DVec3) {
    // SketchUp's "arbitrary axis algorithm" keeps horizontal planes aligned with world X.
    let u = if n.x.abs() < 1.0 / 64.0 && n.y.abs() < 1.0 / 64.0 { DVec3::Y.cross(n) } else { DVec3::Z.cross(n) }
        .normalize();
    let v = n.cross(u);
    (u, v)
}

/// Newell's method: robust normal for a (possibly slightly non-planar) polygon.
pub fn newell_normal(pts: &[DVec3]) -> DVec3 {
    let mut n = DVec3::ZERO;
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    n.normalize_or_zero()
}

pub fn signed_area_2d(pts: &[DVec2]) -> f64 {
    let mut a = 0.0;
    for i in 0..pts.len() {
        let p = pts[i];
        let q = pts[(i + 1) % pts.len()];
        a += p.x * q.y - q.x * p.y;
    }
    a * 0.5
}

/// Even-odd point in polygon test.
pub fn point_in_polygon_2d(p: DVec2, poly: &[DVec2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Parameter t in [0,1] of the projection of p on segment ab, and the distance to it.
pub fn project_on_segment(p: DVec3, a: DVec3, b: DVec3) -> (f64, f64) {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 < 1e-18 {
        return (0.0, p.distance(a));
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (t, p.distance(a + ab * t))
}

/// Closest points between segments p1p2 and q1q2: (s, t, distance).
pub fn segment_segment(p1: DVec3, p2: DVec3, q1: DVec3, q2: DVec3) -> (f64, f64, f64) {
    let d1 = p2 - p1;
    let d2 = q2 - q1;
    let r = p1 - q1;
    let a = d1.dot(d1);
    let e = d2.dot(d2);
    let f = d2.dot(r);
    let (mut s, mut t);
    if a < 1e-18 && e < 1e-18 {
        return (0.0, 0.0, p1.distance(q1));
    }
    if a < 1e-18 {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(r);
        if e < 1e-18 {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            s = if denom.abs() > 1e-12 { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
            t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
        }
    }
    let dist = (p1 + d1 * s).distance(q1 + d2 * t);
    (s, t, dist)
}

/// Closest point on an infinite line (origin o, direction d) to a ray (ro, rd).
/// Returns the parameter along the line.
pub fn line_closest_to_ray(o: DVec3, d: DVec3, ro: DVec3, rd: DVec3) -> Option<f64> {
    let w = o - ro;
    let a = d.dot(d);
    let b = d.dot(rd);
    let c = rd.dot(rd);
    let dd = d.dot(w);
    let e = rd.dot(w);
    let denom = a * c - b * b;
    if denom.abs() < 1e-12 {
        return None;
    }
    Some((b * e - c * dd) / denom)
}

pub fn transform_point(m: &DMat4, p: DVec3) -> DVec3 {
    m.transform_point3(p)
}

pub fn ray_triangle(o: DVec3, d: DVec3, a: DVec3, b: DVec3, c: DVec3) -> Option<f64> {
    let e1 = b - a;
    let e2 = c - a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(-1e-9..=1.0 + 1e-9).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 1e-9).then_some(t)
}
