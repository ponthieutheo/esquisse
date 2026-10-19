//! The check SketchUp makes of each arc when it opens a model (`docs/skp-format.md` §3.5): Check
//! Validity runs after the loading and, for an arc
//! (`CArcCurve`, vtable slot 16), fixes what it finds by dissolving the curve:
//! its edges stay, without a curve. An arc is wrong when
//! - its start or end vertex is not where the ray from its centre through its point at the start
//!   (or end) angle crosses its edges (the crossing found on the chain of its
//!   vertices, within 0.001"): an arc whose angles run against its edges, or whose end vertex left
//!   its plane;
//! - its x axis is not as long as its radius;
//! - its plane is not that of its centre and x × y, or its normal is null;
//! - it is open and its end angle is below its start angle (measured on real files).
//!
//! The curve's start and end vertices are those of its first and last edges, along the arc
//! (the direction of an edge).

use std::collections::{HashMap, HashSet};

use super::doc::{Arc, Ents, Id};

/// SketchUp's tolerance on lengths (inches).
const TOL: f64 = 0.001;

pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn len(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// A unit vector as SketchUp makes one (times the inverse of the length); a null one stays null.
pub(crate) fn unit_or_zero(a: [f64; 3]) -> [f64; 3] {
    let l = len(a);
    if l == 0.0 {
        return a;
    }
    let k = 1.0 / l;
    a.map(|x| x * k)
}

/// Two points within the tolerance of each other (each coordinate and the distance).
fn same(a: [f64; 3], b: [f64; 3]) -> bool {
    let d = sub(a, b);
    d.iter().all(|x| x.abs() < TOL) && dot(d, d) < TOL * TOL
}

/// The angle of a point around an arc: projected on the arc's plane, from its
/// x axis towards its y axis, in [0, 2π].
pub(crate) fn arc_param(a: &Arc, p: [f64; 3]) -> f64 {
    let n = a.normal;
    let off = dot(sub(p, a.center), n);
    let q = [p[0] - off * n[0], p[1] - off * n[1], p[2] - off * n[2]];
    let d = sub(q, a.center);
    let t = dot(cross(a.xaxis, d), n).atan2(dot(a.xaxis, d));
    if !t.is_finite() {
        return t;
    }
    let tau = std::f64::consts::TAU;
    let mut t = t;
    while t < 0.0 {
        t += tau;
    }
    while t > tau {
        t -= tau;
    }
    t
}

/// Whether an edge from `p` to `q` runs along the arc, towards growing angles (
/// the angles of its vertices, compared within π).
pub(crate) fn forward(a: &Arc, p: [f64; 3], q: [f64; 3]) -> bool {
    let (d1, d2) = (arc_param(a, p), arc_param(a, q));
    if (d1 - d2).abs() <= std::f64::consts::PI { d2 > d1 } else { d1 > d2 }
}

/// The point of an arc at an angle.
fn point_at(a: &Arc, t: f64) -> [f64; 3] {
    let (s, c) = t.sin_cos();
    [0, 1, 2].map(|k| c * a.xaxis[k] + a.center[k] + s * a.yaxis[k])
}

/// Where the ray from the arc's centre through `p` (projected on its plane) crosses the chain
/// of vertices `chain`: `None` when it crosses nothing.
fn crossing(a: &Arc, plane_d: f64, p: [f64; 3], chain: &[[f64; 3]]) -> Option<[f64; 3]> {
    let n = a.normal;
    let off = dot(n, p) + plane_d;
    let p = [p[0] - off * n[0], p[1] - off * n[1], p[2] - off * n[2]];
    let o = a.center;
    let d = sub(p, o);
    if d.iter().all(|x| x.abs() < TOL) && dot(d, d) < TOL * TOL {
        return None;
    }
    let u = unit_or_zero(d);
    for w in chain.windows(2) {
        let (p, q) = (w[0], w[1]);
        let Some(t) = line_meets_segment(o, u, p, q) else { continue };
        if t > 0.0 {
            return Some([0, 1, 2].map(|k| o[k] + t * u[k]));
        }
    }
    None
}

/// whether the line `o + t·u` meets the segment `p`–`q`, and its parameter t
/// there (of the point of the line nearest the segment).
fn line_meets_segment(o: [f64; 3], u: [f64; 3], p: [f64; 3], q: [f64; 3]) -> Option<f64> {
    let d = sub(q, p);
    if dot(d, d) <= 0.0 {
        // A segment of one point: whether the line goes through its middle.
        let m = [0, 1, 2].map(|k| (q[k] + p[k]) * 0.5);
        let t = dot(sub(m, o), u);
        let y = [0, 1, 2].map(|k| o[k] + t * u[k]);
        return same(y, m).then_some(t);
    }
    let w = cross(d, u);
    let ww = dot(w, w);
    if ww.abs() < 1e-10 {
        // Parallel: whether the line goes through the segment's start.
        let t = dot(sub(p, o), u);
        let y = [0, 1, 2].map(|k| o[k] + t * u[k]);
        return same(y, p).then_some(t);
    }
    let s = dot(cross(sub(o, p), u), w) / ww;
    let x = [0, 1, 2].map(|k| s * d[k] + p[k]);
    if (s < 0.0 || s > 1.0) && !same(x, p) && !same(x, q) {
        return None;
    }
    Some(dot(sub(x, o), u))
}

/// The plane equations of SketchUp compared (without orientation): the normals
/// equal or opposite within the tolerance, and their constants accordingly.
fn same_plane(n1: [f64; 3], d1: f64, n2: [f64; 3], d2: f64) -> bool {
    let close = |a: [f64; 3], b: [f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() <= TOL);
    let flip = if d1.abs() >= TOL { (d1 < 0.0) != (d2 < 0.0) } else { dot(n1, n2) < 0.0 };
    if flip { close(n1.map(|x| -x), n2) && (-d1 - d2).abs() <= TOL } else { close(n1, n2) && (d1 - d2).abs() <= TOL }
}

/// Dissolves the arcs of a collection that SketchUp's Check Validity finds wrong (module doc).
/// The curves' first and last edges are those of the file (2021+) or of the walk (3–2020).
pub(crate) fn check_arcs(e: &mut Ents) {
    if e.curves.iter().all(|c| c.arc.is_none()) {
        return;
    }
    let pos: HashMap<Id, [f64; 3]> = e.verts.iter().map(|v| (v.id, v.pos)).collect();
    let by_id: HashMap<Id, usize> = e.edges.iter().enumerate().map(|(i, x)| (x.el.id, i)).collect();
    // The edges of each curve at each vertex, in the edges' order.
    let mut at: HashMap<(Id, Id), Vec<usize>> = HashMap::new();
    for (i, x) in e.edges.iter().enumerate() {
        if let Some(c) = x.curve {
            at.entry((x.start, c)).or_default().push(i);
            if x.end != x.start {
                at.entry((x.end, c)).or_default().push(i);
            }
        }
    }
    let mut wrong = HashSet::new();
    for c in &e.curves {
        let Some(a) = &c.arc else { continue };
        let (Some(&fi), Some(&li)) = (by_id.get(&c.first), by_id.get(&c.last)) else { continue };
        let edges = &e.edges;
        if edges[fi].curve != Some(c.id) || edges[li].curve != Some(c.id) {
            continue;
        }
        let p = |v: Id| pos.get(&v).copied();
        let (Some(fs), Some(fe), Some(ls), Some(le)) =
            (p(edges[fi].start), p(edges[fi].end), p(edges[li].start), p(edges[li].end))
        else {
            continue;
        };
        let start_v = if forward(a, fs, fe) { edges[fi].start } else { edges[fi].end };
        let end_v = if forward(a, ls, le) { edges[li].end } else { edges[li].start };
        // The chain of vertices from the start.
        let other = |i: usize, v: Id| if edges[i].start == v { edges[i].end } else { edges[i].start };
        let mut chain = vec![start_v, other(fi, start_v)];
        if fi != li {
            let (mut cur, mut v) = (fi, chain[1]);
            for _ in 0..edges.len() {
                let mine = at.get(&(v, c.id)).map_or(&[][..], Vec::as_slice);
                let Some(&nx) = mine.iter().rev().find(|&&j| j != cur) else { break };
                if mine.len() > 2 {
                    break;
                }
                v = other(nx, v);
                chain.push(v);
                cur = nx;
                if cur == li {
                    break;
                }
            }
        }
        let chain: Vec<[f64; 3]> = chain.iter().filter_map(|&v| p(v)).collect();
        let plane_d = -dot(a.normal, a.center);
        let mut bad = false;
        if let (Some(x0), Some(x1)) =
            (crossing(a, plane_d, point_at(a, a.start), &chain), crossing(a, plane_d, point_at(a, a.end), &chain))
        {
            bad |= !(p(start_v).is_some_and(|s| same(s, x0)) && p(end_v).is_some_and(|s| same(s, x1)));
        }
        bad |= (len(a.xaxis) - a.radius).abs() >= TOL;
        // An open arc whose end angle is below its start angle (seen in 2021+ files, which keep
        // the angles as written): dissolved by SketchUp, closed ones kept. Measured on the real
        // corpus (`docs/skp-format.md` §3.5), not yet found in SketchUp.exe.
        bad |= a.end < a.start && start_v != end_v;
        let n = unit_or_zero(cross(a.xaxis, a.yaxis));
        bad |= len(n) < 1e-10 || !same_plane(n, -dot(n, a.center), a.normal, plane_d);
        if bad {
            wrong.insert(c.id);
        }
    }
    if wrong.is_empty() {
        return;
    }
    e.curves.retain(|c| !wrong.contains(&c.id));
    for x in &mut e.edges {
        if x.curve.is_some_and(|c| wrong.contains(&c)) {
            x.curve = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::doc::{Curve, Edge, El, Vertex};
    use std::f64::consts::FRAC_PI_2;

    /// A quarter of a circle of radius 2 around the origin, in 4 edges from angle 0 to π/2, its
    /// points at the angles given, edges in that order.
    fn quarter(a: Arc, angles: &[f64]) -> Ents {
        let mut e = Ents::default();
        for (i, t) in angles.iter().enumerate() {
            e.verts.push(Vertex { id: 1 + i as Id, pos: [2.0 * t.cos(), 2.0 * t.sin(), 0.0] });
        }
        for i in 0..angles.len() - 1 {
            let el = El { id: 100 + i as Id, ..Default::default() };
            e.edges.push(Edge { el, start: 1 + i as Id, end: 2 + i as Id, curve: Some(50) });
        }
        let last = 100 + angles.len() as Id - 2;
        e.curves.push(Curve { id: 50, segments: 4, kind: 0, first: 100, last, arc: Some(a) });
        e
    }

    fn arc() -> Arc {
        Arc {
            center: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
            xaxis: [2.0, 0.0, 0.0],
            yaxis: [0.0, 2.0, 0.0],
            radius: 2.0,
            start: 0.0,
            end: FRAC_PI_2,
        }
    }

    const ANGLES: [f64; 5] = [0.0, 0.3, 0.7, 1.2, FRAC_PI_2];

    fn kept(e: &Ents) -> bool {
        e.curves.len() == 1 && e.edges.iter().all(|x| x.curve == Some(50))
    }

    #[test]
    fn a_right_arc_is_kept() {
        let mut e = quarter(arc(), &ANGLES);
        check_arcs(&mut e);
        assert!(kept(&e));
        // Its edges the other way round, from π/2 to 0: the first edge along the arc is the
        // last one.
        let mut e = quarter(arc(), &ANGLES.map(|t| FRAC_PI_2 - t));
        let c = &mut e.curves[0];
        (c.first, c.last) = (c.last, c.first);
        check_arcs(&mut e);
        assert!(kept(&e));
    }

    #[test]
    fn an_arc_whose_ends_are_not_its_vertices_is_dissolved() {
        // Its angles stop at 1 (between its vertices at 0.7 and 1.2), its last vertex is at π/2.
        let mut e = quarter(Arc { end: 1.0, ..arc() }, &ANGLES);
        check_arcs(&mut e);
        assert!(e.curves.is_empty() && e.edges.iter().all(|x| x.curve.is_none()));
    }

    #[test]
    fn an_open_arc_whose_angles_run_backwards_is_dissolved() {
        // Its edges below the x axis, its frame turned over (normal -z, y = -y), its angles from
        // 0 to -π/2: nothing else is wrong (its end, by its angle, is above the axis: no ray
        // crosses its edges there).
        let a = Arc { normal: [0.0, 0.0, -1.0], yaxis: [0.0, -2.0, 0.0], end: -FRAC_PI_2, ..arc() };
        let mut e = quarter(Arc { end: FRAC_PI_2, ..a.clone() }, &ANGLES.map(|t| -t));
        check_arcs(&mut e);
        assert!(kept(&e));
        let mut e = quarter(a, &ANGLES.map(|t| -t));
        check_arcs(&mut e);
        assert!(e.curves.is_empty());
        // Closed (a circle from 0 to -2π): kept.
        let mut e = quarter(Arc { end: -std::f64::consts::TAU, ..arc() }, &[0.0, 1.5, 3.0, 4.5]);
        e.edges.push(Edge { el: El { id: 103, ..Default::default() }, start: 4, end: 1, curve: Some(50) });
        e.curves[0].last = 103;
        check_arcs(&mut e);
        assert!(kept(&e));
    }

    #[test]
    fn an_arc_whose_axis_is_not_its_radius_is_dissolved() {
        let mut e = quarter(Arc { radius: 2.01, ..arc() }, &ANGLES);
        check_arcs(&mut e);
        assert!(e.curves.is_empty());
        // Within the tolerance: kept.
        let mut e = quarter(Arc { radius: 2.0005, ..arc() }, &ANGLES);
        check_arcs(&mut e);
        assert!(kept(&e));
    }

    #[test]
    fn an_arc_whose_normal_is_not_its_plane_is_dissolved() {
        let mut e = quarter(Arc { normal: [0.0, 0.6, 0.8], ..arc() }, &ANGLES);
        check_arcs(&mut e);
        assert!(e.curves.is_empty());
        let mut e = quarter(Arc { yaxis: [0.0; 3], ..arc() }, &ANGLES);
        check_arcs(&mut e);
        assert!(e.curves.is_empty());
    }
}
