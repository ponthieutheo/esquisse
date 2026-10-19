//! Curves (edges that share a curve id) and the edge commands of SketchUp's right-click menu:
//! Weld Edges (`CBaseEditDoc` "Weld Edges", one curve per chain), Orient Faces
//! (`OrientFacesConsistently`).

use std::collections::{HashMap, HashSet};

use glam::DVec3;

use crate::math::TOL;
use crate::model::{EdgeId, Entities, FaceId, VertexId};

/// Edges of curve `id`.
pub fn curve_edges(ents: &Entities, id: u32) -> Vec<EdgeId> {
    ents.edges.iter().filter(|(_, e)| e.curve == Some(id)).map(|(k, _)| k).collect()
}

/// Splits `edges` into chains: a chain goes on through a vertex only where exactly two of
/// the edges meet (a curve never branches). Each chain is returned in order.
pub fn chains(ents: &Entities, edges: &[EdgeId]) -> Vec<Vec<EdgeId>> {
    let set: HashSet<EdgeId> = edges.iter().copied().filter(|e| ents.edges.contains_key(*e)).collect();
    // Edges of the set around each vertex.
    let mut at: HashMap<VertexId, Vec<EdgeId>> = HashMap::new();
    for &e in edges {
        if set.contains(&e) {
            for v in ents.edges[e].v {
                let l = at.entry(v).or_default();
                if !l.contains(&e) {
                    l.push(e);
                }
            }
        }
    }
    let through = |v: VertexId| at.get(&v).is_some_and(|l| l.len() == 2);
    let mut left = set.clone();
    let mut out = Vec::new();
    // Walks from edge `e` leaving vertex `from`, while the chain goes on.
    let walk = |start: EdgeId, from: VertexId, left: &mut HashSet<EdgeId>| {
        let mut run = Vec::new();
        let (mut e, mut v) = (start, from);
        loop {
            let next_v = ents.other_vertex(e, v);
            if !through(next_v) {
                break;
            }
            let Some(&n) = at[&next_v].iter().find(|&&x| x != e) else { break };
            if !left.remove(&n) {
                break;
            }
            run.push(n);
            e = n;
            v = next_v;
        }
        run
    };
    for &start in edges {
        if !left.remove(&start) {
            continue;
        }
        let [a, b] = ents.edges[start].v;
        let fwd = walk(start, a, &mut left);
        let back = walk(start, b, &mut left);
        let mut chain: Vec<EdgeId> = back.into_iter().rev().collect();
        chain.push(start);
        chain.extend(fwd);
        out.push(chain);
    }
    out
}

/// Gives curves whose edges are no longer connected one id per connected piece.
fn split_disconnected(ents: &mut Entities, ids: &HashSet<u32>) {
    for &id in ids {
        let edges = curve_edges(ents, id);
        let mut pieces = chains(ents, &edges);
        if pieces.len() > 1 {
            pieces.sort_by_key(|p| std::cmp::Reverse(p.len()));
            for p in pieces.into_iter().skip(1) {
                let nid = ents.new_curve_id();
                for e in p {
                    ents.edges[e].curve = Some(nid);
                }
            }
        }
    }
}

/// Edit › Weld Edges: each chain of the given edges becomes one curve (its edges select and
/// move together). The curves the edges belonged to lose them. Returns the new curve ids.
pub fn weld(ents: &mut Entities, edges: &[EdgeId]) -> Vec<u32> {
    let old: HashSet<u32> = edges.iter().filter_map(|e| ents.edges.get(*e).and_then(|x| x.curve)).collect();
    let mut ids = Vec::new();
    for chain in chains(ents, edges) {
        if chain.len() < 2 {
            // A lone edge is not a curve: it leaves the curve it was part of.
            ents.edges[chain[0]].curve = None;
            continue;
        }
        let id = ents.new_curve_id();
        for e in chain {
            ents.edges[e].curve = Some(id);
        }
        ids.push(id);
    }
    split_disconnected(ents, &old);
    ids
}

/// Orient Faces: the faces connected to `seeds` (through edges bounding exactly two faces)
/// are turned to agree with the seed they are reached from. Returns how many were reversed.
pub fn orient_faces(ents: &mut Entities, seeds: &[FaceId]) -> usize {
    let mut seen: HashSet<FaceId> = HashSet::new();
    let mut flipped = 0;
    for &seed in seeds {
        if !ents.faces.contains_key(seed) || !seen.insert(seed) {
            continue;
        }
        let mut stack = vec![seed];
        while let Some(f) = stack.pop() {
            for e in ents.face_edges(f) {
                let faces = ents.edges[e].faces.clone();
                if faces.len() != 2 {
                    continue;
                }
                let g = if faces[0] == f { faces[1] } else { faces[0] };
                if seen.contains(&g) {
                    continue;
                }
                // Consistent neighbours use their common edge in opposite directions.
                let [a, b] = ents.edges[e].v;
                let dir = |face: FaceId| -> Option<bool> {
                    ents.faces[face].loops().find_map(|l| {
                        (0..l.len()).find_map(|i| {
                            let (x, y) = (l[i], l[(i + 1) % l.len()]);
                            if x == a && y == b {
                                Some(true)
                            } else if x == b && y == a {
                                Some(false)
                            } else {
                                None
                            }
                        })
                    })
                };
                if let (Some(df), Some(dg)) = (dir(f), dir(g))
                    && df == dg
                {
                    ents.reverse_face(g);
                    flipped += 1;
                }
                seen.insert(g);
                stack.push(g);
            }
        }
    }
    flipped
}

/// The vertices of an ordered chain of edges, and whether it closes on itself.
pub fn chain_vertices(ents: &Entities, chain: &[EdgeId]) -> (Vec<VertexId>, bool) {
    if chain.is_empty() {
        return (Vec::new(), false);
    }
    let [a, b] = ents.edges[chain[0]].v;
    // Start at the end of the first edge that is not shared with the second one.
    let start = match chain.get(1) {
        Some(&n) if ents.edges[n].v.contains(&a) => b,
        _ => a,
    };
    let mut out = vec![start];
    let mut v = start;
    for &e in chain {
        v = ents.other_vertex(e, v);
        out.push(v);
    }
    let closed = chain.len() > 2 && out.first() == out.last();
    if closed {
        out.pop();
    }
    (out, closed)
}

/// An arc (or a circle) found on a curve: its points lie on a circle at regular steps.
#[derive(Clone, Copy, Debug)]
pub struct ArcFit {
    pub center: DVec3,
    pub normal: DVec3,
    pub radius: f64,
    /// Direction of the first point from the centre (angle 0).
    pub xaxis: DVec3,
    /// Angle swept from the first point to the last (2π for a circle).
    pub sweep: f64,
    pub closed: bool,
}

impl ArcFit {
    pub fn point(&self, angle: f64) -> DVec3 {
        let y = self.normal.cross(self.xaxis);
        self.center + (self.xaxis * angle.cos() + y * angle.sin()) * self.radius
    }

    /// Angle of `p` around the arc, from its first point, in 0..2π.
    pub fn angle_of(&self, p: DVec3) -> f64 {
        let d = p - self.center;
        let y = self.normal.cross(self.xaxis);
        d.dot(y).atan2(d.dot(self.xaxis)).rem_euclid(std::f64::consts::TAU)
    }
}

fn circumcircle(a: DVec3, b: DVec3, c: DVec3) -> Option<(DVec3, DVec3)> {
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    let d = 2.0 * n.length_squared();
    if d < 1e-12 {
        return None;
    }
    let centre = a + (n.cross(ab) * ac.length_squared() + ac.cross(n) * ab.length_squared()) / d;
    Some((centre, n.normalize()))
}

/// The arc a curve follows, if it is one (SketchUp offers Divide only on single edges and
/// on arcs and circles, not on other curves).
pub fn arc_fit(ents: &Entities, curve: u32) -> Option<ArcFit> {
    arc_fit_edges(ents, &curve_edges(ents, curve))
}

/// [`arc_fit`] of the edges of a curve, found by the caller.
pub fn arc_fit_edges(ents: &Entities, edges: &[EdgeId]) -> Option<ArcFit> {
    let ch = chains(ents, edges);
    if ch.len() != 1 {
        return None;
    }
    let (vs, closed) = chain_vertices(ents, &ch[0]);
    let pts: Vec<DVec3> = vs.iter().map(|&v| ents.pos(v)).collect();
    let n = pts.len();
    if n < 3 {
        return None;
    }
    let (center, mut normal) =
        circumcircle(pts[0], pts[n / 2], pts[n - 1]).or_else(|| circumcircle(pts[0], pts[1], pts[2]))?;
    let radius = pts[0].distance(center);
    let tol = TOL.max(radius * 1e-6);
    if radius < TOL
        || pts.iter().any(|p| (p.distance(center) - radius).abs() > tol || (*p - center).dot(normal).abs() > tol)
    {
        return None;
    }
    let xaxis = (pts[0] - center).normalize();
    let mut fit = ArcFit { center, normal, radius, xaxis, sweep: 0.0, closed };
    // Turn the normal so that the points go counter-clockwise around it.
    if fit.angle_of(pts[1]) > std::f64::consts::PI {
        normal = -normal;
        fit.normal = normal;
    }
    let step = fit.angle_of(pts[1]);
    for (i, p) in pts.iter().enumerate().skip(1) {
        let a = fit.angle_of(*p);
        let want = (step * i as f64).rem_euclid(std::f64::consts::TAU);
        let diff = (a - want).abs();
        if diff.min(std::f64::consts::TAU - diff) > 1e-6 {
            return None;
        }
    }
    fit.sweep = if closed { std::f64::consts::TAU } else { step * (n - 1) as f64 };
    Some(fit)
}

/// The number of segments the Divide tool shows for a cursor at fraction `t` of the edge
/// (or arc): the nearer an end, the more segments, `1/t` rounded, at most 100 (read in
/// `CDivideLineTool`).
pub fn divide_count(t: f64) -> usize {
    if !(0.0..=1.0).contains(&t) || t < 1e-9 || 1.0 - t < 1e-9 {
        return 1;
    }
    let t = if t > 0.5 { 1.0 - t } else { t };
    ((1.0 / t + 0.5) as usize).clamp(1, 100)
}

/// Replaces the inner points of the chain `vs` (closed or not) by `inner`, keeping the faces
/// it bounds; the new edges take the attributes of `like`.
pub(crate) fn replace_chain(ents: &mut Entities, vs: &[VertexId], closed: bool, inner: &[DVec3], like: EdgeId) {
    let first = vs[0];
    let last = if closed { first } else { *vs.last().unwrap() };
    let old_inner: Vec<VertexId> = if closed { vs[1..].to_vec() } else { vs[1..vs.len() - 1].to_vec() };
    let attrs = ents.edges[like].clone();
    let mut old_edges = Vec::new();
    for i in 0..vs.len() - usize::from(!closed) {
        if let Some(e) = ents.find_edge(vs[i], vs[(i + 1) % vs.len()]) {
            old_edges.push(e);
        }
    }
    let new_inner: Vec<VertexId> = inner.iter().map(|&p| ents.add_vertex_raw(p)).collect();
    // Face loops: the run first, old_inner…, last becomes first, new_inner…, last.
    let fids: Vec<FaceId> = ents.faces.keys().collect();
    for f in fids {
        let face = &mut ents.faces[f];
        for l in std::iter::once(&mut face.outer).chain(face.holes.iter_mut()) {
            let m = l.len();
            let Some(i) = (0..m).find(|&i| l[i] == first) else { continue };
            let fwd = old_inner.first().is_some_and(|&v| l[(i + 1) % m] == v);
            let back = old_inner.first().is_some_and(|&v| l[(i + m - 1) % m] == v);
            if !fwd && !back {
                continue;
            }
            let mut seq = new_inner.clone();
            if back {
                seq.reverse();
            }
            let run: HashSet<VertexId> = old_inner.iter().copied().collect();
            let rest: Vec<VertexId> = l.iter().copied().filter(|v| !run.contains(v)).collect();
            // Insert the new points after (or before) `first` in what is left of the loop.
            let at = rest.iter().position(|&v| v == first).unwrap();
            let mut nl = rest.clone();
            if fwd {
                for (k, v) in seq.into_iter().enumerate() {
                    nl.insert(at + 1 + k, v);
                }
            } else {
                for (k, v) in seq.into_iter().enumerate() {
                    nl.insert(at + k, v);
                }
            }
            *l = nl;
        }
    }
    for e in old_edges {
        ents.remove_edge_raw(e);
    }
    for v in old_inner {
        if ents.verts.get(v).is_some_and(|x| x.edges.is_empty()) {
            ents.verts.remove(v);
        }
    }
    let mut path = vec![first];
    path.extend(&new_inner);
    path.push(last);
    for w in path.windows(2) {
        if let Some(e) = ents.add_edge_raw(w[0], w[1]) {
            let n = &mut ents.edges[e];
            n.soft = attrs.soft;
            n.smooth = attrs.smooth;
            n.hidden = attrs.hidden;
            n.layer = attrs.layer;
            n.curve = attrs.curve;
        }
    }
    ents.rebuild_topology();
}

/// Divide: edge `e` cut into `n` equal edges, or, when it belongs to an arc or a circle,
/// the whole arc redrawn with `n` segments (`CDivideLineTool`, "Divide Edges"). Returns
/// false when there is nothing to do.
pub fn divide(ents: &mut Entities, e: EdgeId, n: usize) -> bool {
    if n < 2 || !ents.edges.contains_key(e) {
        return false;
    }
    if let Some(c) = ents.edges[e].curve {
        let Some(fit) = arc_fit(ents, c) else { return false };
        let edges = curve_edges(ents, c);
        let ch = chains(ents, &edges);
        let (vs, closed) = chain_vertices(ents, &ch[0]);
        if vs.len() - usize::from(!closed) == n {
            return false;
        }
        let inner: Vec<DVec3> = (1..n).map(|i| fit.point(fit.sweep * i as f64 / n as f64)).collect();
        replace_chain(ents, &vs, closed, &inner, e);
        return true;
    }
    let [va, vb] = ents.edges[e].v;
    let (a, b) = (ents.pos(va), ents.pos(vb));
    let mut cur = e;
    // Split from the far end: the piece left at `a` is always the one still to cut.
    for i in (1..n).rev() {
        let v = ents.split_edge(cur, a + (b - a) * (i as f64 / n as f64));
        let Some(next) = ents.find_edge(va, v) else { return true };
        cur = next;
    }
    true
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::*;

    fn polyline(ents: &mut Entities, pts: &[DVec3]) -> Vec<EdgeId> {
        let mut out = Vec::new();
        for w in pts.windows(2) {
            out.extend(ents.add_line(w[0], w[1]));
        }
        out
    }

    #[test]
    fn weld_makes_one_curve_per_chain_and_stops_at_branches() {
        let mut e = Entities::default();
        let x = |v: f64, w: f64| DVec3::new(v, w, 0.0);
        let a = polyline(&mut e, &[x(0.0, 0.0), x(100.0, 0.0), x(200.0, 50.0), x(300.0, 0.0)]);
        // A branch at (100, 0): three welded edges meet there, so the chains stop.
        let b = polyline(&mut e, &[x(100.0, 0.0), x(100.0, -100.0)]);
        let all: Vec<EdgeId> = a.iter().chain(&b).copied().collect();
        let ids = weld(&mut e, &all);
        assert_eq!(ids.len(), 1, "only (100,0)-(200,50)-(300,0) is a chain of two edges or more");
        assert_eq!(curve_edges(&e, ids[0]).len(), 2);
        assert!(e.edges[a[0]].curve.is_none() && e.edges[b[0]].curve.is_none());
        // Welding the three edges of the open polyline alone gives one curve.
        let ids = weld(&mut e, &a);
        assert_eq!(ids.len(), 1);
        assert_eq!(curve_edges(&e, ids[0]).len(), 3);
    }

    #[test]
    fn welding_part_of_a_curve_splits_what_is_left() {
        let mut e = Entities::default();
        let x = |v: f64| DVec3::new(v, (v * 0.01).sin() * 50.0, 0.0);
        let pts: Vec<DVec3> = (0..6).map(|i| x(i as f64 * 100.0)).collect();
        let edges = e.add_polyline(&pts, false, true);
        assert_eq!(edges.len(), 5);
        let old = e.edges[edges[0]].curve.unwrap();
        // Weld the middle edges 1-3: the old curve keeps edge 0 and edge 4, now apart.
        let ids = weld(&mut e, &edges[1..4]);
        assert_eq!(curve_edges(&e, ids[0]).len(), 3);
        let c0 = e.edges[edges[0]].curve;
        let c4 = e.edges[edges[4]].curve;
        assert!(c0.is_some() && c4.is_some() && c0 != c4, "{c0:?} {c4:?}");
        assert!(c0 == Some(old) || c4 == Some(old));
    }

    #[test]
    fn divide_count_follows_the_cursor_like_sketchup() {
        assert_eq!(divide_count(0.5), 2);
        assert_eq!(divide_count(0.3), 3);
        assert_eq!(divide_count(0.7), 3);
        assert_eq!(divide_count(0.1), 10);
        assert_eq!(divide_count(0.001), 100);
        assert_eq!(divide_count(0.0), 1);
        assert_eq!(divide_count(1.2), 1);
    }

    #[test]
    fn divide_cuts_an_edge_of_a_face_in_equal_parts() {
        let mut e = Entities::default();
        let p = |x: f64, y: f64| DVec3::new(x, y, 0.0);
        e.add_polyline(&[p(0.0, 0.0), p(1000.0, 0.0), p(1000.0, 500.0), p(0.0, 500.0)], true, false);
        let bottom = e.edges.keys().find(|&k| e.edge_points(k).0.y == 0.0 && e.edge_points(k).1.y == 0.0).unwrap();
        assert!(divide(&mut e, bottom, 4));
        assert_eq!(e.edges.len(), 7);
        assert_eq!(e.faces.len(), 1);
        let f = e.faces.keys().next().unwrap();
        assert_eq!(e.faces[f].outer.len(), 7);
        for x in [250.0, 500.0, 750.0] {
            assert!(e.find_vertex(p(x, 0.0)).is_some(), "{x}");
        }
        assert_eq!(e.edges.values().filter(|x| x.faces.len() == 1).count(), 7);
    }

    #[test]
    fn divide_redraws_a_whole_circle_or_arc() {
        let mut e = Entities::default();
        let pts: Vec<DVec3> = (0..24)
            .map(|i| {
                let a = i as f64 / 24.0 * std::f64::consts::TAU;
                DVec3::new(a.cos() * 500.0, a.sin() * 500.0, 0.0)
            })
            .collect();
        let edges = e.add_polyline(&pts, true, true);
        let c = e.edges[edges[0]].curve.unwrap();
        let fit = arc_fit(&e, c).expect("a circle");
        assert!(fit.closed && (fit.radius - 500.0).abs() < 1e-6);
        assert!(divide(&mut e, edges[3], 8));
        assert_eq!(e.edges.len(), 8);
        assert!(e.edges.values().all(|x| x.curve == Some(c) && x.faces.len() == 1));
        let f = e.faces.keys().next().unwrap();
        assert_eq!(e.faces[f].outer.len(), 8);
        let fit = arc_fit(&e, c).expect("still a circle");
        assert!((fit.radius - 500.0).abs() < 1e-6);

        // A half circle closed by its chord: only the arc changes.
        let mut e = Entities::default();
        let arc: Vec<DVec3> = (0..=12)
            .map(|i| {
                let a = i as f64 / 12.0 * std::f64::consts::PI;
                DVec3::new(a.cos() * 500.0, a.sin() * 500.0, 0.0)
            })
            .collect();
        let ae = e.add_polyline(&arc, false, true);
        e.add_line(arc[12], arc[0]);
        assert_eq!(e.faces.len(), 1);
        let c = e.edges[ae[0]].curve.unwrap();
        assert!((arc_fit(&e, c).unwrap().sweep - std::f64::consts::PI).abs() < 1e-9);
        assert!(divide(&mut e, ae[5], 5));
        assert_eq!(e.edges.len(), 6);
        assert_eq!(curve_edges(&e, c).len(), 5);
        let f = e.faces.keys().next().unwrap();
        assert_eq!(e.faces[f].outer.len(), 6);
        assert!(crate::tess::face_area(&e, f) > 0.0);
    }

    #[test]
    fn orient_faces_turns_the_connected_faces_like_the_seed() {
        let mut e = Entities::default();
        // Two squares sharing an edge, the second one reversed by hand.
        let p = |x: f64, y: f64| DVec3::new(x, y, 0.0);
        e.add_polyline(&[p(0.0, 0.0), p(100.0, 0.0), p(100.0, 100.0), p(0.0, 100.0)], true, false);
        e.add_polyline(&[p(100.0, 0.0), p(200.0, 0.0), p(200.0, 100.0), p(100.0, 100.0)], false, false);
        assert_eq!(e.faces.len(), 2);
        let fs: Vec<FaceId> = e.faces.keys().collect();
        let n0 = e.faces[fs[0]].normal;
        if e.faces[fs[1]].normal.dot(n0) > 0.0 {
            e.reverse_face(fs[1]);
        }
        assert_eq!(orient_faces(&mut e, &[fs[0]]), 1);
        assert!(e.faces[fs[1]].normal.dot(n0) > 0.99);
        assert_eq!(orient_faces(&mut e, &[fs[0]]), 0, "already consistent");
    }
}
