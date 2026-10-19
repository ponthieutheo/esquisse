//! Arc settings and checks shared by the arc tools (after CArcTool, CFit3PointArcTool,
//! CArc3PointTool and CArc3PointPieTool).

use std::f64::consts::TAU;

use glam::DVec3;

use crate::math::TOL;

/// Fewest and most segments an arc or a circle can have ("Curve segments must be in the range
/// from 2 to 999").
pub const MIN_SIDES: usize = 2;
pub const MAX_SIDES: usize = 999;
/// Default number of segments in an arc.
pub const DEFAULT_ARC_SIDES: usize = 12;
/// Largest radius, 10^9 inches.
pub const MAX_RADIUS: f64 = 1.0e9 * 25.4;

/// What the number of sides counts: the arc itself ("12s") or the full circle it belongs to
/// ("24c").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidesIn {
    Arc,
    Circle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sides {
    pub n: usize,
    pub per: SidesIn,
}

impl Default for Sides {
    fn default() -> Self {
        Sides { n: DEFAULT_ARC_SIDES, per: SidesIn::Arc }
    }
}

pub fn sides_out_of_range() -> String {
    format!("Les segments de la courbe doivent être compris entre {MIN_SIDES} et {MAX_SIDES}.")
}

impl Sides {
    /// Number of segments of an arc that sweeps `sweep` radians.
    pub fn segments(&self, sweep: f64) -> usize {
        match self.per {
            SidesIn::Arc => self.n,
            SidesIn::Circle => ((self.n as f64 * sweep.abs() / TAU).round() as usize).max(1),
        }
    }

    /// Status text: "12 côtés dans l'arc".
    pub fn status(&self) -> String {
        format!("{} côtés dans {}", self.n, if self.per == SidesIn::Arc { "l'arc" } else { "le cercle" })
    }

    /// The value shown in the Measurements box ("12s", "24c").
    pub fn vcb(&self) -> String {
        format!("{}{}", self.n, if self.per == SidesIn::Arc { 's' } else { 'c' })
    }

    /// One more or one less side (Ctrl + / Ctrl -), refused outside 2..=999.
    pub fn stepped(&self, delta: i32) -> Result<Sides, String> {
        let n = self.n as i64 + delta as i64;
        if (MIN_SIDES as i64..=MAX_SIDES as i64).contains(&n) {
            Ok(Sides { n: n as usize, per: self.per })
        } else {
            Err(sides_out_of_range())
        }
    }
}

/// "12s" (sides in the arc) or "24c" (sides in the circle). `None` when the text is not a number
/// of sides; an error when it is one but out of range.
pub fn parse_sides(text: &str) -> Option<Result<Sides, String>> {
    let t = text.trim().to_lowercase();
    let (digits, per) =
        t.strip_suffix('s').map(|d| (d, SidesIn::Arc)).or_else(|| t.strip_suffix('c').map(|d| (d, SidesIn::Circle)))?;
    let n: i64 = digits.trim().parse().ok()?;
    if (MIN_SIDES as i64..=MAX_SIDES as i64).contains(&n) {
        Some(Ok(Sides { n: n as usize, per }))
    } else {
        Some(Err(sides_out_of_range()))
    }
}

/// Whether an arc of this radius (mm) and sweep (radians) can be drawn with this many segments.
pub fn check_arc(radius: f64, sweep: f64, segments: usize) -> Result<(), String> {
    if radius > MAX_RADIUS {
        return Err("Rayon trop grand.".into());
    }
    let sweep = sweep.abs();
    if radius * sweep / (segments as f64) < TOL {
        return Err("Rayon trop petit ou trop de segments pour cet angle.".into());
    }
    // 0.001 square inch.
    if (sweep - TAU).abs() < 0.001 && std::f64::consts::PI * radius * radius < 0.001 * 25.4 * 25.4 {
        return Err("Rayon trop petit.".into());
    }
    if (1.0 - (sweep / segments as f64).cos()) * radius <= TOL {
        return Err("Trop de segments pour cet angle et ce rayon.".into());
    }
    Ok(())
}

/// Points of an arc: `segments + 1` points from `start` (unit vector from the centre) over `sweep`.
pub fn arc_points(c: DVec3, n: DVec3, start: DVec3, radius: f64, sweep: f64, segments: usize) -> Vec<DVec3> {
    let v = n.cross(start);
    (0..=segments)
        .map(|i| {
            let a = sweep * i as f64 / segments as f64;
            c + (start * a.cos() + v * a.sin()) * radius
        })
        .collect()
}

/// Where two coplanar, non-parallel lines meet.
pub fn line_intersection(p1: DVec3, d1: DVec3, p2: DVec3, d2: DVec3) -> Option<DVec3> {
    let n = d1.cross(d2);
    let n2 = n.length_squared();
    if n2 < 1e-12 {
        return None;
    }
    let w = p2 - p1;
    if w.dot(n).abs() > TOL * n2.sqrt() {
        return None;
    }
    let t = w.cross(d2).dot(n) / n2;
    Some(p1 + d1 * t)
}

/// A rounded corner: the arc tangent to two straight edges that meet at a corner.
#[derive(Clone, Copy, Debug)]
pub struct Fillet {
    pub t1: DVec3,
    pub t2: DVec3,
    pub center: DVec3,
    pub radius: f64,
    /// Unit vector from the centre to `t1`.
    pub start: DVec3,
    pub normal: DVec3,
    /// Signed sweep from `t1` to `t2` around `normal` (the short way).
    pub sweep: f64,
}

/// Half the angle between the two rays leaving the corner.
fn half_angle(u1: DVec3, u2: DVec3) -> f64 {
    u1.dot(u2).clamp(-1.0, 1.0).acos() * 0.5
}

/// The fillet of radius `r` in the corner `v` between the unit rays `u1` and `u2`.
pub fn fillet(v: DVec3, u1: DVec3, u2: DVec3, r: f64) -> Option<Fillet> {
    let normal = u1.cross(u2).try_normalize()?;
    let h = half_angle(u1, u2);
    if h < 1e-6 || h > std::f64::consts::FRAC_PI_2 - 1e-6 || r <= 0.0 {
        return None;
    }
    let d = r / h.tan();
    let (t1, t2) = (v + u1 * d, v + u2 * d);
    let center = v + (u1 + u2).normalize() * (r / h.sin());
    let (a, b) = ((t1 - center) / r, (t2 - center) / r);
    let sweep = a.cross(b).dot(normal).atan2(a.dot(b));
    Some(Fillet { t1, t2, center, radius: r, start: a, normal, sweep })
}

/// Radius of the fillet whose arc passes `apex` away from the corner (its point closest to the corner).
pub fn fillet_radius_for_apex(u1: DVec3, u2: DVec3, apex: f64) -> f64 {
    let s = half_angle(u1, u2).sin();
    apex / (1.0 / s - 1.0)
}

/// Largest radius that keeps the tangent points on the edges (`len1`, `len2` from the corner).
pub fn fillet_max_radius(u1: DVec3, u2: DVec3, len1: f64, len2: f64) -> f64 {
    len1.min(len2) * half_angle(u1, u2).tan()
}

/// Tangent length from the corner for a given radius.
pub fn fillet_tangent_length(u1: DVec3, u2: DVec3, r: f64) -> f64 {
    r / half_angle(u1, u2).tan()
}

/// Radius of the circle tangent to both rays at distance `d` from the corner.
pub fn fillet_radius_for_tangent_length(u1: DVec3, u2: DVec3, d: f64) -> f64 {
    d * half_angle(u1, u2).tan()
}

/// Sagitta (signed, positive towards `side`) of the arc that starts at A tangent to the unit
/// direction `t` and ends at B. `side` is the unit vector perpendicular to the chord on which
/// positive bulges lie. `None` when the tangent runs along the chord.
pub fn tangent_bulge(a: DVec3, b: DVec3, t: DVec3, side: DVec3) -> Option<f64> {
    let chord = b - a;
    let half = chord.length() * 0.5;
    if half < TOL {
        return None;
    }
    let cd = chord / (2.0 * half);
    let alpha = t.cross(cd).length().atan2(t.dot(cd));
    if alpha < 1e-6 || alpha > std::f64::consts::PI - 1e-6 {
        return None;
    }
    let s = half * (alpha * 0.5).tan();
    Some(if t.dot(side) < 0.0 { -s } else { s })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sides_parse_in_arc_and_in_circle() {
        assert_eq!(parse_sides("12s"), Some(Ok(Sides { n: 12, per: SidesIn::Arc })));
        assert_eq!(parse_sides(" 24C "), Some(Ok(Sides { n: 24, per: SidesIn::Circle })));
        assert_eq!(parse_sides("500"), None);
        assert_eq!(parse_sides("5cm"), None);
        assert!(matches!(parse_sides("1s"), Some(Err(_))));
        assert!(matches!(parse_sides("1000c"), Some(Err(_))));
        assert_eq!(parse_sides("2s").unwrap().unwrap().n, 2);
        assert_eq!(parse_sides("999s").unwrap().unwrap().n, 999);
    }

    #[test]
    fn sides_in_a_circle_scale_with_the_sweep() {
        let s = Sides { n: 24, per: SidesIn::Circle };
        assert_eq!(s.segments(TAU), 24);
        assert_eq!(s.segments(TAU / 4.0), 6);
        assert_eq!(s.segments(-TAU / 4.0), 6);
        assert_eq!(s.segments(0.001), 1);
        assert_eq!(Sides { n: 7, per: SidesIn::Arc }.segments(1.0), 7);
    }

    #[test]
    fn stepping_stops_at_the_limits() {
        let s = Sides { n: 2, per: SidesIn::Arc };
        assert!(s.stepped(-1).is_err());
        assert_eq!(s.stepped(1).unwrap().n, 3);
        assert!(Sides { n: 999, per: SidesIn::Arc }.stepped(1).is_err());
    }

    #[test]
    fn checks_name_the_problem() {
        assert!(check_arc(500.0, 1.5, 12).is_ok());
        assert!(check_arc(1.0e12, 1.0, 12).unwrap_err().contains("grand"));
        assert!(check_arc(0.05, 0.1, 100).unwrap_err().contains("petit"));
        assert!(check_arc(0.3, TAU, 12).is_err());
        // Tiny sagitta: many segments on a small angle.
        assert!(check_arc(100.0, 0.01, 999).unwrap_err().contains("segments"));
    }

    #[test]
    fn square_corner_fillet() {
        // Corner at the origin between +X and +Y, radius 100.
        let f = fillet(DVec3::ZERO, DVec3::X, DVec3::Y, 100.0).unwrap();
        assert!(f.t1.distance(DVec3::new(100.0, 0.0, 0.0)) < 1e-9);
        assert!(f.t2.distance(DVec3::new(0.0, 100.0, 0.0)) < 1e-9);
        assert!(f.center.distance(DVec3::new(100.0, 100.0, 0.0)) < 1e-9);
        // The short way round: a quarter turn.
        assert!((f.sweep.abs() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        let pts = arc_points(f.center, f.normal, f.start, f.radius, f.sweep, 6);
        assert!(pts[0].distance(f.t1) < 1e-9 && pts[6].distance(f.t2) < 1e-9);
        // Its middle is on the bisector, closest to the corner.
        let apex = (pts[3] - DVec3::ZERO).length();
        assert!((apex - 100.0 * (2f64.sqrt() - 1.0)).abs() < 1e-9);
        assert!((fillet_radius_for_apex(DVec3::X, DVec3::Y, apex) - 100.0).abs() < 1e-9);
        assert!((fillet_tangent_length(DVec3::X, DVec3::Y, 100.0) - 100.0).abs() < 1e-9);
        assert!((fillet_max_radius(DVec3::X, DVec3::Y, 300.0, 500.0) - 300.0).abs() < 1e-9);
    }

    #[test]
    fn fillet_in_a_sharp_corner_is_longer_to_reach() {
        let u2 = DVec3::new(0.5, 3f64.sqrt() * 0.5, 0.0);
        let f = fillet(DVec3::ZERO, DVec3::X, u2, 50.0).unwrap();
        // Corner angle 60 degrees: tangent length r / tan(30 degrees).
        assert!((f.t1.length() - 50.0 / (30f64.to_radians()).tan()).abs() < 1e-9);
        assert!(f.center.distance(f.t1).abs() - 50.0 < 1e-9);
        assert!(fillet(DVec3::ZERO, DVec3::X, DVec3::X, 50.0).is_none());
    }

    #[test]
    fn tangent_arc_leaves_along_the_edge() {
        // From A=(0,0) going along +X to B=(100,100): a quarter circle centred (0,100).
        let a = DVec3::ZERO;
        let b = DVec3::new(100.0, 100.0, 0.0);
        let s = tangent_bulge(a, b, DVec3::X, DVec3::new(-1.0, 1.0, 0.0).normalize()).unwrap();
        // Quarter circle of radius 100 over a chord of 141.4: sagitta = r (1 - cos 45) = 29.29,
        // on the side of the tangent (towards +X / -Y of the chord).
        assert!((s.abs() - 100.0 * (1.0 - 45f64.to_radians().cos())).abs() < 1e-9, "{s}");
        assert!(s < 0.0);
        assert!(tangent_bulge(a, b, DVec3::new(1.0, 1.0, 0.0).normalize(), DVec3::Y).is_none());
    }

    #[test]
    fn lines_meet_only_when_coplanar() {
        let p = line_intersection(DVec3::ZERO, DVec3::X, DVec3::new(5.0, 3.0, 0.0), DVec3::Y).unwrap();
        assert!(p.distance(DVec3::new(5.0, 0.0, 0.0)) < 1e-9);
        assert!(line_intersection(DVec3::ZERO, DVec3::X, DVec3::new(5.0, 3.0, 2.0), DVec3::Y).is_none());
    }
}
