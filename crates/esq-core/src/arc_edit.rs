//! Entity Info's Radius and Segments of an arc or a circle, as SketchUp edits a `CArcCurve`
//! (the page `CArcPP`, read in SketchUp.exe):
//!
//! - **Segments** : the curve is redrawn with that many equal edges between
//!   the same two ends (the same vertex for a circle), and every face it bounds keeps its
//!   loops, the old edges replaced by the new ones in order. The field may be typed only
//!   when every face along the curve lies in the curve's plane (`CArcCurve` slot 23): a circle
//!   already pushed into a cylinder keeps its segments.
//! - **Radius** : a circle keeps its centre and grows or shrinks; an open
//!   arc keeps its ends where they are free, and stays tangent to a straight edge it meets
//!   tangentially (a fillet): free at both ends, both ends stay and the centre moves along the
//!   chord's bisector; tangent at one end, the free end stays and the arc slides along the
//!   tangent line; tangent at both, the centre is where the two lines offset by the radius
//!   cross and the edges are trimmed or lengthened to the new tangent points.
//! - Both checked first as does, in inches: radius at most 10^9", each
//!   segment at least 0.001" long, a circle's area at least 0.001 in², and each segment's
//!   sagitta over 0.001".

use std::f64::consts::{PI, TAU};

use glam::DVec3;

use crate::curves::{ArcFit, arc_fit, chain_vertices, chains, curve_edges};
use crate::model::{Entities, VertexId};

/// SketchUp's tolerance (0.001") and largest radius (10^9"), in inches.
const TOL_IN: f64 = 0.001;
const MAX_RADIUS_IN: f64 = 1.0e9;

/// A curve that is an arc or a circle: its fit and its vertices in order (the first one
/// again at the end of a circle is not repeated).
pub struct ArcCurve {
    pub fit: ArcFit,
    pub verts: Vec<VertexId>,
}

impl ArcCurve {
    pub fn of(ents: &Entities, curve: u32) -> Option<ArcCurve> {
        let fit = arc_fit(ents, curve)?;
        let edges = curve_edges(ents, curve);
        let ch = chains(ents, &edges);
        let (verts, _) = chain_vertices(ents, ch.first()?);
        Some(ArcCurve { fit, verts })
    }

    /// How many edges the curve has.
    pub fn segments(&self) -> usize {
        if self.fit.closed { self.verts.len() } else { self.verts.len() - 1 }
    }

    /// The fewest segments Entity Info accepts: 3 for a circle, 2 for an arc.
    pub fn min_segments(&self) -> usize {
        if self.fit.closed { 3 } else { 2 }
    }

    /// The angle the arc sweeps (2π for a circle).
    pub fn sweep(&self) -> f64 {
        if self.fit.closed { TAU } else { self.fit.sweep }
    }

    /// Circumference of a circle, radius × angle of an arc (`CArcCurve` slot 29).
    pub fn length(&self) -> f64 {
        self.fit.radius * self.sweep()
    }
}

/// Whether radius `r` (mm) and `n` segments make an arc of `sweep` radians SketchUp accepts,
/// else its message.
pub fn check(r: f64, n: usize, sweep: f64) -> Result<(), String> {
    let r_in = r / 25.4;
    if r_in > MAX_RADIUS_IN {
        return Err("Rayon trop grand.".into());
    }
    if n == 0 || r_in * sweep / n as f64 - 0.0 < TOL_IN {
        return Err("Le rayon est trop petit ou le nombre de segments est trop élevé pour un angle donné.".into());
    }
    if (sweep - TAU).abs() < TOL_IN && (r_in * PI * r_in).abs() < TOL_IN {
        return Err("Le rayon est trop petit.".into());
    }
    if (1.0 - (sweep / n as f64).cos()) * r_in <= TOL_IN {
        return Err("Le nombre de segments est trop élevé pour l'angle et le rayon donnés.".into());
    }
    Ok(())
}

/// Entity Info › Segments may be typed: every face along the curve lies in its plane
/// (`CArcCurve` slot 23).
pub fn segments_editable(ents: &Entities, curve: u32) -> bool {
    let Some(a) = ArcCurve::of(ents, curve) else { return false };
    let n = a.fit.normal;
    curve_edges(ents, curve).iter().all(|e| {
        ents.edges[*e].faces.iter().all(|f| {
            let face = &ents.faces[*f];
            let p = ents.pos(face.outer[0]);
            face.normal.normalize_or_zero().dot(n).abs() > 1.0 - 1e-9
                && (p - a.fit.center).dot(n).abs() < crate::math::TOL
        })
    })
}

/// Entity Info › Segments: the curve redrawn with `n` equal segments, its faces kept.
/// The caller checks [`check`] and [`segments_editable`] first.
pub fn set_segments(ents: &mut Entities, curve: u32, n: usize) -> bool {
    let Some(a) = ArcCurve::of(ents, curve) else { return false };
    if n == a.segments() || n < a.min_segments() {
        return false;
    }
    let like = curve_edges(ents, curve)[0];
    let inner: Vec<DVec3> = (1..n).map(|i| a.fit.point(a.sweep() * i as f64 / n as f64)).collect();
    crate::curves::replace_chain(ents, &a.verts, a.fit.closed, &inner, like);
    true
}

/// The straight edge meeting the arc at `v` in the arc's plane along its tangent `t` (a
/// fillet's), as looks for one: its direction away from `v`.
fn tangent_line(ents: &Entities, curve: u32, v: VertexId, t: DVec3, normal: DVec3) -> Option<DVec3> {
    ents.verts[v].edges.iter().find_map(|e| {
        let edge = &ents.edges[*e];
        if edge.curve == Some(curve) {
            return None;
        }
        let d = (ents.pos(ents.other_vertex(*e, v)) - ents.pos(v)).normalize_or_zero();
        (d.dot(normal).abs() < 1e-6 && d.cross(t).length() < 1e-6).then_some(d)
    })
}

/// The point of line (p, d) nearest to `q`.
fn project(p: DVec3, d: DVec3, q: DVec3) -> DVec3 {
    p + d * (q - p).dot(d)
}

/// Entity Info › Radius, `r` in mm. Returns false when nothing could move
/// (an arc whose free ends are further apart than the new diameter).
pub fn set_radius(ents: &mut Entities, curve: u32, r: f64) -> bool {
    let Some(a) = ArcCurve::of(ents, curve) else { return false };
    let f = a.fit;
    if r <= 0.0 || (r - f.radius).abs() < 1e-12 {
        return false;
    }
    let n = f.normal;
    let moved: Vec<(VertexId, DVec3)> = if f.closed {
        a.verts.iter().map(|v| (*v, f.center + (ents.pos(*v) - f.center).normalize_or_zero() * r)).collect()
    } else {
        let (va, vb) = (a.verts[0], *a.verts.last().unwrap());
        let (pa, pb) = (ents.pos(va), ents.pos(vb));
        // Tangents along the arc at its two ends, and the straight edges along them.
        let ta = n.cross((pa - f.center).normalize());
        let tb = n.cross((pb - f.center).normalize());
        let la = tangent_line(ents, curve, va, ta, n);
        let lb = tangent_line(ents, curve, vb, tb, n);
        // The side of each tangent line the centre is on.
        let inward = |p: DVec3| (f.center - p).normalize();
        let (c, a2, b2) = match (la, lb) {
            (None, None) => {
                // Both ends stay: the centre on the chord's bisector, across the chord from
                // the bulge (on its side for an arc of more than half a turn).
                let m = (pa + pb) * 0.5;
                let h = pa.distance(pb) * 0.5;
                if r < h {
                    return false;
                }
                let d = (r * r - h * h).sqrt();
                let bulge = (f.point(f.sweep * 0.5) - m).normalize();
                let dir = if f.sweep > PI { bulge } else { -bulge };
                (m + dir * d, pa, pb)
            }
            (Some(_), Some(_)) => {
                // A fillet: tangent to both lines, the edges trimmed to the new tangent points.
                let (oa, ob) = (pa + inward(pa) * r, pb + inward(pb) * r);
                let den = ta.cross(tb).dot(n);
                if den.abs() < 1e-12 {
                    return false;
                }
                let s = (ob - oa).cross(tb).dot(n) / den;
                let c = oa + ta * s;
                (c, project(pa, ta, c), project(pb, tb, c))
            }
            (Some(_), None) | (None, Some(_)) => {
                // The free end stays; the arc stays tangent to the line at the other end.
                let (free, tied, t_tied) = if la.is_some() { (pb, pa, ta) } else { (pa, pb, tb) };
                let off = tied + inward(tied) * r;
                // Centres at distance r from the free end, on the offset line: the one nearest
                // to where the old centre was.
                let w = off - free;
                let b = w.dot(t_tied);
                let disc = b * b - (w.length_squared() - r * r);
                if disc < 0.0 {
                    return false;
                }
                let s = disc.sqrt();
                let cands = [off + t_tied * (-b + s), off + t_tied * (-b - s)];
                let c = if cands[0].distance(f.center) <= cands[1].distance(f.center) { cands[0] } else { cands[1] };
                let tied2 = project(tied, t_tied, c);
                if la.is_some() { (c, tied2, free) } else { (c, free, tied2) }
            }
        };
        // The new arc from a2 to b2 around c, the same way round.
        let x = (a2 - c).normalize();
        let y = n.cross(x);
        let e = b2 - c;
        let mut sweep = e.dot(y).atan2(e.dot(x));
        if sweep <= 0.0 {
            sweep += TAU;
        }
        let k = a.verts.len() - 1;
        a.verts
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let t = sweep * i as f64 / k as f64;
                (*v, c + (x * t.cos() + y * t.sin()) * r)
            })
            .collect()
    };
    for (v, p) in &moved {
        ents.verts[*v].pos = *p;
    }
    // The faces that moved keep their orientation: their normals follow.
    let mut faces: Vec<_> = moved
        .iter()
        .flat_map(|(v, _)| ents.verts[*v].edges.clone())
        .flat_map(|e| ents.edges[e].faces.clone())
        .collect();
    faces.sort();
    faces.dedup();
    for face in faces {
        let before = ents.faces[face].normal;
        ents.recompute_normal(face);
        if ents.faces[face].normal.dot(before) < 0.0 {
            ents.faces[face].normal = -ents.faces[face].normal;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(e: &mut Entities, r: f64, n: usize, sweep: f64, face: bool) -> u32 {
        let closed = (sweep - TAU).abs() < 1e-9;
        let pts = if closed { n } else { n + 1 };
        let v: Vec<_> = (0..pts)
            .map(|i| {
                let t = i as f64 * sweep / n as f64;
                e.add_vertex_raw(DVec3::new(r * t.cos(), r * t.sin(), 0.0))
            })
            .collect();
        let c = e.new_curve_id();
        for i in 0..n {
            let x = e.add_edge_raw(v[i], v[(i + 1) % pts]).unwrap();
            e.edges[x].curve = Some(c);
        }
        if face {
            e.add_face_raw(v, vec![]).unwrap();
        }
        c
    }

    #[test]
    fn segments_redraw_the_circle_and_keep_its_face() {
        let mut e = Entities::default();
        let c = circle(&mut e, 500.0, 24, TAU, true);
        assert!(segments_editable(&e, c));
        assert!(set_segments(&mut e, c, 48));
        e.rebuild_topology();
        let a = ArcCurve::of(&e, c).unwrap();
        assert_eq!(a.segments(), 48);
        assert_eq!(e.faces.len(), 1, "the disc is kept");
        let f = e.faces.values().next().unwrap();
        assert_eq!(f.outer.len(), 48, "its loop follows the new edges");
        assert!((a.fit.radius - 500.0).abs() < 1e-6);
    }

    #[test]
    fn a_pushed_circle_keeps_its_segments() {
        let mut e = Entities::default();
        let c = circle(&mut e, 500.0, 12, TAU, false);
        // One side face standing on the first edge.
        let first = curve_edges(&e, c)[0];
        let [a, b] = e.edges[first].v;
        let (pa, pb) = (e.pos(a), e.pos(b));
        let ta = e.add_vertex_raw(pa + DVec3::Z * 300.0);
        let tb = e.add_vertex_raw(pb + DVec3::Z * 300.0);
        e.add_face_raw(vec![a, b, tb, ta], vec![]).unwrap();
        e.rebuild_topology();
        assert!(!segments_editable(&e, c));
    }

    #[test]
    fn radius_of_a_circle_keeps_its_centre_and_face() {
        let mut e = Entities::default();
        let c = circle(&mut e, 500.0, 24, TAU, true);
        e.rebuild_topology();
        assert!(set_radius(&mut e, c, 800.0));
        let a = ArcCurve::of(&e, c).unwrap();
        assert!((a.fit.radius - 800.0).abs() < 1e-6 && a.fit.center.length() < 1e-6);
        assert!(
            (crate::tess::face_area(&e, e.faces.keys().next().unwrap())
                - 800.0 * 800.0 * 24.0 * (PI / 24.0).sin() * (PI / 24.0).cos())
            .abs()
                < 1.0
        );
    }

    #[test]
    fn radius_of_a_free_arc_keeps_its_ends() {
        let mut e = Entities::default();
        let c = circle(&mut e, 500.0, 6, PI / 2.0, false);
        e.rebuild_topology();
        let a0 = ArcCurve::of(&e, c).unwrap();
        let (p0, p1) = (e.pos(a0.verts[0]), e.pos(*a0.verts.last().unwrap()));
        assert!(set_radius(&mut e, c, 1000.0));
        let a = ArcCurve::of(&e, c).unwrap();
        assert!((a.fit.radius - 1000.0).abs() < 1e-6);
        assert!(e.pos(a.verts[0]).distance(p0) < 1e-9 && e.pos(*a.verts.last().unwrap()).distance(p1) < 1e-9);
        // Smaller than half the chord: nothing moves.
        assert!(!set_radius(&mut e, c, 100.0));
    }

    #[test]
    fn radius_of_a_fillet_stays_tangent_to_both_edges() {
        // The corner of an L, rounded with radius 200 between (0, 1000)-(0, 200) and (200, 0)-(1000, 0).
        let mut e = Entities::default();
        let k = 8;
        let v: Vec<_> = (0..=k)
            .map(|i| {
                let t = PI + (PI / 2.0) * i as f64 / k as f64;
                e.add_vertex_raw(DVec3::new(200.0 + 200.0 * t.cos(), 200.0 + 200.0 * t.sin(), 0.0))
            })
            .collect();
        let c = e.new_curve_id();
        for i in 0..k {
            let x = e.add_edge_raw(v[i], v[i + 1]).unwrap();
            e.edges[x].curve = Some(c);
        }
        let top = e.add_vertex_raw(DVec3::new(0.0, 1000.0, 0.0));
        let right = e.add_vertex_raw(DVec3::new(1000.0, 0.0, 0.0));
        e.add_edge_raw(top, v[0]).unwrap();
        e.add_edge_raw(v[k], right).unwrap();
        e.rebuild_topology();
        assert!(set_radius(&mut e, c, 400.0));
        let a = ArcCurve::of(&e, c).unwrap();
        assert!((a.fit.radius - 400.0).abs() < 1e-6, "{}", a.fit.radius);
        assert!(a.fit.center.distance(DVec3::new(400.0, 400.0, 0.0)) < 1e-6, "{:?}", a.fit.center);
        // The straight edges were shortened to the new tangent points.
        assert!(e.pos(v[0]).distance(DVec3::new(0.0, 400.0, 0.0)) < 1e-6, "{:?}", e.pos(v[0]));
        assert!(e.pos(v[k]).distance(DVec3::new(400.0, 0.0, 0.0)) < 1e-6);
    }

    #[test]
    fn checks_are_sketchups() {
        assert!(check(500.0, 24, TAU).is_ok());
        assert_eq!(check(2.0e9 * 25.4, 24, TAU).unwrap_err(), "Rayon trop grand.");
        assert!(check(0.01, 999, TAU).unwrap_err().starts_with("Le rayon est trop petit ou"));
        assert!(check(25.4, 999, TAU).unwrap_err().starts_with("Le nombre de segments est trop élevé"));
    }
}
