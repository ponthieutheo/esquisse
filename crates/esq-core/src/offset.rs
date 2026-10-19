//! Offset of selected edges (Offset tool with edges selected beforehand): a parallel copy of a
//! chain of connected, coplanar edges. An open chain is joined to its copy by a line at each
//! end, so the copy and the original bound a region that becomes a face.

use std::collections::HashMap;

use glam::DVec3;

use crate::math::TOL;
use crate::model::{EdgeId, Entities, VertexId};

/// Selected edges put in travel order.
#[derive(Clone, Debug)]
pub struct Chain {
    pub verts: Vec<VertexId>,
    /// A loop: the last vertex joins the first.
    pub closed: bool,
}

/// Normal of the plane of a chain, or `None` if its points are aligned or not coplanar.
pub fn chain_normal(pts: &[DVec3]) -> Option<DVec3> {
    let mut n = DVec3::ZERO;
    for i in 1..pts.len().saturating_sub(1) {
        n += (pts[i] - pts[0]).cross(pts[i + 1] - pts[0]);
    }
    let n = n.normalize_or_zero();
    if n == DVec3::ZERO || pts.iter().any(|p| (*p - pts[0]).dot(n).abs() > TOL) {
        return None;
    }
    Some(n)
}

/// Offset points of a chain. A positive `dist` goes to the left of the direction of travel,
/// as seen with `n` pointing at the viewer; corners are mitred and the ends of an open chain
/// move perpendicularly.
pub fn offset_chain(pts: &[DVec3], closed: bool, n: DVec3, dist: f64) -> Vec<DVec3> {
    let m = pts.len();
    let left = |a: DVec3, b: DVec3| n.cross((b - a).normalize_or_zero());
    (0..m)
        .map(|i| {
            let cur = pts[i];
            let in_prev = if closed || i > 0 { Some(left(pts[(i + m - 1) % m], cur)) } else { None };
            let in_next = if closed || i + 1 < m { Some(left(cur, pts[(i + 1) % m])) } else { None };
            match (in_prev, in_next) {
                (Some(a), Some(b)) => {
                    let bis = (a + b).normalize_or_zero();
                    let cos = bis.dot(a);
                    if bis == DVec3::ZERO || cos.abs() < 1e-6 { cur + a * dist } else { cur + bis * (dist / cos) }
                }
                (Some(a), None) | (None, Some(a)) => cur + a * dist,
                (None, None) => cur,
            }
        })
        .collect()
}

/// Signed area of a planar loop around `n` (positive counter-clockwise seen from `n`).
fn signed_area(pts: &[DVec3], n: DVec3) -> f64 {
    let m = pts.len();
    (0..m).map(|i| pts[i].cross(pts[(i + 1) % m]).dot(n)).sum::<f64>() * 0.5
}

/// Trimmed overlap of an offset loop (the Offset tool's default; Alt allows the overlap): a
/// mitred offset that crosses itself is cut at its crossings into simple loops, and only those
/// turning the way of the original loop (`orig_area_sign`, see [`loop_area_sign`]) are kept;
/// the others are the parts folded back over themselves.
pub fn trim_overlap(off: &[DVec3], n: DVec3, orig_area_sign: f64) -> Vec<Vec<DVec3>> {
    let m = off.len();
    if m < 3 {
        return Vec::new();
    }
    let (u, v) = crate::math::basis_for(n);
    let p2 = |p: DVec3| (p.dot(u), p.dot(v));
    let mut nodes: Vec<DVec3> = off.to_vec();
    // Crossings on each segment: (parameter, node).
    let mut on_seg: Vec<Vec<(f64, usize)>> = vec![Vec::new(); m];
    const EPS: f64 = 1e-9;
    for i in 0..m {
        for j in i + 2..m {
            if i == 0 && j == m - 1 {
                continue; // adjacent through the closing segment
            }
            let (a, b) = (p2(off[i]), p2(off[(i + 1) % m]));
            let (c, d) = (p2(off[j]), p2(off[(j + 1) % m]));
            let r = (b.0 - a.0, b.1 - a.1);
            let s = (d.0 - c.0, d.1 - c.1);
            let den = r.0 * s.1 - r.1 * s.0;
            if den.abs() < 1e-12 {
                continue;
            }
            let t = ((c.0 - a.0) * s.1 - (c.1 - a.1) * s.0) / den;
            let w = ((c.0 - a.0) * r.1 - (c.1 - a.1) * r.0) / den;
            if t > EPS && t < 1.0 - EPS && w > EPS && w < 1.0 - EPS {
                let id = nodes.len();
                nodes.push(off[i].lerp(off[(i + 1) % m], t));
                on_seg[i].push((t, id));
                on_seg[j].push((w, id));
            }
        }
    }
    let mut seq = Vec::with_capacity(nodes.len() * 2);
    for (i, crossings) in on_seg.iter_mut().enumerate() {
        seq.push(i);
        crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
        seq.extend(crossings.iter().map(|c| c.1));
    }
    // Cut at each node met twice until every loop is simple.
    let mut work = vec![seq];
    let mut out = Vec::new();
    while let Some(l) = work.pop() {
        let mut seen: HashMap<usize, usize> = HashMap::new();
        let split = l.iter().enumerate().find_map(|(k, id)| seen.insert(*id, k).map(|first| (first, k)));
        match split {
            Some((a, b)) => {
                work.push(l[a..b].to_vec());
                work.push(l[..a].iter().chain(&l[b..]).copied().collect());
            }
            None if l.len() >= 3 => {
                let pts: Vec<DVec3> = l.iter().map(|id| nodes[*id]).collect();
                let area = signed_area(&pts, n);
                if area.abs() > TOL * TOL && area.signum() == orig_area_sign.signum() {
                    out.push(pts);
                }
            }
            None => {}
        }
    }
    out
}

/// Sign of the area of a loop around `n` (+1 counter-clockwise seen from `n`).
pub fn loop_area_sign(pts: &[DVec3], n: DVec3) -> f64 {
    signed_area(pts, n).signum()
}

impl Entities {
    /// The loops the Offset tool draws for face `f` at `dist` (positive inwards): its mitred
    /// outer loop, trimmed where it overlaps itself unless `allow_overlap`.
    pub fn face_offset_loops(&self, f: crate::model::FaceId, dist: f64, allow_overlap: bool) -> Vec<Vec<DVec3>> {
        let Some(face) = self.faces.get(f) else { return Vec::new() };
        let pts = self.loop_points(&face.outer);
        let off = crate::ops::offset_polygon(&pts, face.normal, dist);
        if allow_overlap {
            return vec![off];
        }
        trim_overlap(&off, face.normal, loop_area_sign(&pts, face.normal))
    }

    /// Offsets face `f` like the Offset tool (see [`Entities::face_offset_loops`]).
    pub fn offset_face_with(&mut self, f: crate::model::FaceId, dist: f64, allow_overlap: bool) -> Vec<EdgeId> {
        let loops = self.face_offset_loops(f, dist, allow_overlap);
        loops.iter().filter(|l| l.len() >= 3).flat_map(|l| self.add_polyline(l, true, false)).collect()
    }

    /// Puts `edges` in order, if they make one simple chain or loop.
    pub fn edge_chain(&self, edges: &[EdgeId]) -> Result<Chain, &'static str> {
        if edges.len() < 2 {
            return Err("Sélectionnez au moins deux arêtes connectées");
        }
        let mut adj: HashMap<VertexId, Vec<EdgeId>> = HashMap::new();
        for &e in edges {
            let Some(edge) = self.edges.get(e) else { return Err("Arête introuvable") };
            for v in edge.v {
                adj.entry(v).or_default().push(e);
            }
        }
        if adj.values().any(|l| l.len() > 2) {
            return Err("Les arêtes se ramifient : elles doivent former une seule chaîne");
        }
        let ends: Vec<VertexId> = adj.iter().filter(|(_, l)| l.len() == 1).map(|(v, _)| *v).collect();
        let closed = ends.is_empty();
        if !closed && ends.len() != 2 {
            return Err("Les arêtes doivent former une seule chaîne connectée");
        }
        let start = if closed { self.edges[edges[0]].v[0] } else { *ends.iter().min().expect("two ends") };
        let mut verts = vec![start];
        let mut used: Vec<EdgeId> = Vec::new();
        let mut cur = start;
        while let Some(next_edge) = adj[&cur].iter().copied().find(|e| !used.contains(e)) {
            used.push(next_edge);
            cur = self.other_vertex(next_edge, cur);
            if cur == start {
                break;
            }
            verts.push(cur);
        }
        if used.len() != edges.len() {
            return Err("Les arêtes doivent former une seule chaîne connectée");
        }
        Ok(Chain { verts, closed })
    }

    /// Offsets a chain of edges by `dist` (positive to the left of travel, see
    /// [`offset_chain`]). Returns the new edges.
    pub fn offset_edges(&mut self, edges: &[EdgeId], dist: f64) -> Result<Vec<EdgeId>, String> {
        if dist.abs() < TOL {
            return Err("Un décalage de longueur nulle n'est pas autorisé".into());
        }
        let chain = self.edge_chain(edges).map_err(str::to_string)?;
        let pts: Vec<DVec3> = chain.verts.iter().map(|v| self.pos(*v)).collect();
        let n = chain_normal(&pts).ok_or("Les arêtes doivent être coplanaires et non alignées")?;
        let off = offset_chain(&pts, chain.closed, n, dist);
        // The copy stays a curve when the original was one.
        let curve = self.edges[edges[0]].curve;
        let as_curve = curve.is_some() && edges.iter().all(|e| self.edges[*e].curve == curve);
        let mut made = self.add_polyline(&off, chain.closed, as_curve);
        if !chain.closed {
            let last = pts.len() - 1;
            made.extend(self.add_line(pts[0], off[0]));
            made.extend(self.add_line(pts[last], off[last]));
        }
        made.retain(|e| self.edges.contains_key(*e));
        Ok(made)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l_shape(e: &mut Entities) -> Vec<EdgeId> {
        let mut out = Vec::new();
        out.extend(e.add_line(DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0)));
        out.extend(e.add_line(DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, 600.0, 0.0)));
        out
    }

    #[test]
    fn edges_are_ordered_into_a_chain() {
        let mut e = Entities::default();
        let mut edges = l_shape(&mut e);
        edges.reverse();
        let c = e.edge_chain(&edges).unwrap();
        assert!(!c.closed && c.verts.len() == 3);
        // A single edge, or a branching set, is refused.
        assert!(e.edge_chain(&edges[..1]).is_err());
        let extra = e.add_line(DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, -300.0, 0.0));
        let mut all = edges.clone();
        all.extend(extra);
        assert!(e.edge_chain(&all).is_err(), "a T is not a chain");
    }

    #[test]
    fn offset_goes_left_of_travel_and_mitres_the_corner() {
        let n = DVec3::Z;
        let pts = [DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, 600.0, 0.0)];
        // Travelling +X then +Y, the left side is +Y then -X: the corner moves inwards.
        let left = offset_chain(&pts, false, n, 100.0);
        assert!(left[0].distance(DVec3::new(0.0, 100.0, 0.0)) < 1e-9);
        assert!(left[1].distance(DVec3::new(900.0, 100.0, 0.0)) < 1e-9, "{:?}", left[1]);
        assert!(left[2].distance(DVec3::new(900.0, 600.0, 0.0)) < 1e-9);
        let right = offset_chain(&pts, false, n, -100.0);
        assert!(right[1].distance(DVec3::new(1100.0, -100.0, 0.0)) < 1e-9, "{:?}", right[1]);
    }

    #[test]
    fn offsetting_an_open_chain_makes_a_band_face() {
        let mut e = Entities::default();
        let edges = l_shape(&mut e);
        let made = e.offset_edges(&edges, -100.0).unwrap();
        assert!(!made.is_empty());
        let has = |p: DVec3| e.verts.values().any(|v| v.pos.distance(p) < 1e-6);
        assert!(
            has(DVec3::new(0.0, -100.0, 0.0))
                && has(DVec3::new(1100.0, -100.0, 0.0))
                && has(DVec3::new(1100.0, 600.0, 0.0))
        );
        assert_eq!(e.faces.len(), 1, "the copy, the ends and the original bound one face");
    }

    #[test]
    fn offsetting_a_loop_of_edges_makes_a_second_loop() {
        let mut e = Entities::default();
        let sq =
            [DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, 1000.0, 0.0), DVec3::new(0.0, 1000.0, 0.0)];
        let edges = e.add_polyline(&sq, true, false);
        // The square has a face; offset its edges inwards (CCW: left is inside).
        let made = e.offset_edges(&edges, 100.0).unwrap();
        assert_eq!(made.len(), 4);
        assert!(e.verts.values().any(|v| v.pos.distance(DVec3::new(100.0, 100.0, 0.0)) < 1e-6));
    }

    #[test]
    fn a_loop_crossing_itself_keeps_the_part_turning_the_original_way() {
        let eight =
            [DVec3::ZERO, DVec3::new(1000.0, 1000.0, 0.0), DVec3::new(1000.0, 0.0, 0.0), DVec3::new(0.0, 1000.0, 0.0)];
        let kept = trim_overlap(&eight, DVec3::Z, 1.0);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].len(), 3);
        assert!(kept[0].iter().any(|p| p.distance(DVec3::new(500.0, 500.0, 0.0)) < 1e-9));
        assert!(kept[0].iter().any(|p| p.distance(DVec3::new(0.0, 1000.0, 0.0)) < 1e-9));
        assert_eq!(trim_overlap(&eight, DVec3::Z, -1.0)[0].len(), 3, "the other lobe");
    }

    #[test]
    fn offsetting_a_thin_face_too_far_inwards_is_trimmed_unless_overlap_is_allowed() {
        let mut e = Entities::default();
        let pts =
            [DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, 200.0, 0.0), DVec3::new(0.0, 200.0, 0.0)];
        e.add_polyline(&pts, true, false);
        let f = e.faces.keys().next().unwrap();
        assert_eq!(e.face_offset_loops(f, 50.0, false).len(), 1, "a plain offset is kept");
        assert!(e.face_offset_loops(f, 150.0, false).is_empty(), "folded over itself");
        assert_eq!(e.face_offset_loops(f, 150.0, true).len(), 1);
        assert!(e.offset_face_with(f, 150.0, false).is_empty());
    }

    #[test]
    fn collinear_edges_and_zero_distance_are_refused() {
        let mut e = Entities::default();
        let mut edges = e.add_line(DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0));
        edges.extend(e.add_line(DVec3::new(1000.0, 0.0, 0.0), DVec3::new(2000.0, 0.0, 0.0)));
        assert!(e.offset_edges(&edges, 100.0).is_err());
        let mut f = Entities::default();
        let l = l_shape(&mut f);
        assert!(f.offset_edges(&l, 0.0).is_err());
    }
}
