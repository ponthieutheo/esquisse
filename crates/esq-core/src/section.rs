//! Section planes: where a plane cuts faces, the closed loops of a cut (for the section
//! fill), and Create Group from Slice.

use std::collections::HashMap;

use glam::{DMat4, DVec2, DVec3};

use crate::intersect::{Poly, plane_line};
use crate::math::{Plane, TOL, point_in_polygon_2d};
use crate::model::{DefId, Definition, Entities, FaceId, Instance, InstanceId, Model, SectionId, SectionPlane};
use crate::tess::triangulate_2d;

/// Segments where a face (placed by `m`) crosses a plane.
pub fn face_cut(ents: &Entities, f: FaceId, m: &DMat4, plane: &Plane) -> Vec<(DVec3, DVec3)> {
    let Some(poly) = Poly::new(ents, f, m) else { return Vec::new() };
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in poly.loops.iter().flatten() {
        let d = plane.distance(*p);
        lo = lo.min(d);
        hi = hi.max(d);
    }
    if lo > -TOL || hi < TOL {
        return Vec::new();
    }
    let Some((p, d)) = plane_line(&poly.plane, plane) else { return Vec::new() };
    poly.intervals(p, d).into_iter().map(|(t0, t1)| (p + d * t0, p + d * t1)).collect()
}

/// The part of a segment that no plane of `cuts` removes.
pub fn clip_segment(a: DVec3, b: DVec3, cuts: &[SectionPlane]) -> Option<(DVec3, DVec3)> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for c in cuts {
        let (da, db) = ((a - c.pos).dot(c.normal), (b - c.pos).dot(c.normal));
        if da < -TOL && db < -TOL {
            return None;
        }
        if (da < 0.0) != (db < 0.0) {
            let t = da / (da - db);
            if da < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t1 - t0 > 1e-9).then(|| (a + (b - a) * t0, a + (b - a) * t1))
}

/// Joins segments end to end into closed loops (open chains are dropped).
pub fn chain_loops(segs: &[(DVec3, DVec3)]) -> Vec<Vec<DVec3>> {
    // Weld endpoints closer than the tolerance.
    let mut pts: Vec<DVec3> = Vec::new();
    let mut id = |p: DVec3| -> usize {
        if let Some(i) = pts.iter().position(|q| q.distance(p) < TOL * 4.0) {
            return i;
        }
        pts.push(p);
        pts.len() - 1
    };
    let edges: Vec<(usize, usize)> = segs.iter().map(|&(a, b)| (id(a), id(b))).filter(|(a, b)| a != b).collect();
    let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
    for (k, &(a, b)) in edges.iter().enumerate() {
        adj.entry(a).or_default().push(k);
        adj.entry(b).or_default().push(k);
    }
    let mut used = vec![false; edges.len()];
    let mut out = Vec::new();
    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let (first, mut cur) = edges[start];
        let mut lp = vec![first];
        let mut closed = false;
        while let Some(&next) = adj[&cur].iter().find(|&&k| !used[k]) {
            used[next] = true;
            lp.push(cur);
            let (a, b) = edges[next];
            cur = if a == cur { b } else { a };
            if cur == first {
                closed = true;
                break;
            }
        }
        if closed && lp.len() >= 3 {
            out.push(lp.into_iter().map(|i| pts[i]).collect());
        }
    }
    out
}

/// The segments that keep a cut from closing into loops: those of open chains, found by
/// taking away again and again the segments with a free end (what remains belongs to loops).
pub fn open_segments(segs: &[(DVec3, DVec3)]) -> Vec<(DVec3, DVec3)> {
    let mut pts: Vec<DVec3> = Vec::new();
    let mut id = |p: DVec3| -> usize {
        if let Some(i) = pts.iter().position(|q| q.distance(p) < TOL * 4.0) {
            return i;
        }
        pts.push(p);
        pts.len() - 1
    };
    let ends: Vec<(usize, usize)> = segs.iter().map(|&(a, b)| (id(a), id(b))).collect();
    let mut degree: HashMap<usize, usize> = HashMap::new();
    for &(a, b) in &ends {
        if a != b {
            *degree.entry(a).or_default() += 1;
            *degree.entry(b).or_default() += 1;
        }
    }
    let mut gone = vec![false; ends.len()];
    loop {
        let mut changed = false;
        for (k, &(a, b)) in ends.iter().enumerate() {
            if gone[k] || a == b || (degree[&a] > 1 && degree[&b] > 1) {
                continue;
            }
            gone[k] = true;
            changed = true;
            *degree.get_mut(&a).unwrap() -= 1;
            *degree.get_mut(&b).unwrap() -= 1;
        }
        if !changed {
            break;
        }
    }
    segs.iter().zip(gone).filter(|(_, g)| *g).map(|(s, _)| *s).collect()
}

/// Triangles filling closed loops lying in `plane` (nested loops alternate between solid
/// and hole), wound counter-clockwise around the plane normal.
pub fn fill_triangles(plane: &Plane, loops: &[Vec<DVec3>]) -> Vec<[DVec3; 3]> {
    let (u, v) = plane.basis();
    let flat: Vec<Vec<DVec2>> =
        loops.iter().map(|l| l.iter().map(|p| DVec2::new(p.dot(u), p.dot(v))).collect()).collect();
    let area = |l: &[DVec2]| (0..l.len()).map(|i| l[i].perp_dot(l[(i + 1) % l.len()])).sum::<f64>().abs() * 0.5;
    // For each loop, the loops containing it.
    let parents: Vec<Vec<usize>> = (0..flat.len())
        .map(|i| (0..flat.len()).filter(|&j| j != i && point_in_polygon_2d(flat[i][0], &flat[j])).collect())
        .collect();
    let mut out = Vec::new();
    for i in 0..flat.len() {
        if parents[i].len() % 2 == 1 {
            continue;
        }
        // Holes: loops one level deeper whose innermost container is this loop.
        let hole_ids: Vec<usize> = (0..flat.len())
            .filter(|&j| {
                parents[j].len() == parents[i].len() + 1
                    && parents[j].iter().copied().min_by(|&a, &b| area(&flat[a]).total_cmp(&area(&flat[b]))) == Some(i)
            })
            .collect();
        let holes: Vec<Vec<DVec2>> = hole_ids.iter().map(|&j| flat[j].clone()).collect();
        let pts3: Vec<DVec3> = loops[i].iter().chain(hole_ids.iter().flat_map(|&j| loops[j].iter())).copied().collect();
        for [a, b, c] in triangulate_2d(&flat[i], &holes) {
            let t = [pts3[a], pts3[b], pts3[c]];
            out.push(if (t[1] - t[0]).cross(t[2] - t[0]).dot(plane.n) < 0.0 { [t[0], t[2], t[1]] } else { t });
        }
    }
    out
}

impl Model {
    /// Segments where a section plane cuts its collection and everything it contains, in
    /// the collection's coordinates.
    pub fn slice(&self, ctx: Option<DefId>, sec: SectionId) -> Vec<(DVec3, DVec3)> {
        let ents = self.entities(ctx);
        let Some(sp) = ents.sections.get(sec) else { return Vec::new() };
        let plane = sp.plane();
        let mut out = Vec::new();
        fn rec(model: &Model, ents: &Entities, m: &DMat4, plane: &Plane, depth: usize, out: &mut Vec<(DVec3, DVec3)>) {
            for f in ents.faces.keys() {
                out.extend(face_cut(ents, f, m, plane));
            }
            if depth > 32 {
                return;
            }
            for inst in ents.instances.values() {
                if let Some(def) = model.defs.get(inst.def) {
                    rec(model, &def.entities, &(*m * inst.xform), plane, depth + 1, out);
                }
            }
        }
        rec(self, ents, &DMat4::IDENTITY, &plane, 0, &mut out);
        out
    }

    /// Section plane › Create Group from Slice: a group holding the edges of the cut.
    pub fn group_from_slice(&mut self, ctx: Option<DefId>, sec: SectionId) -> Option<InstanceId> {
        let segs = self.slice(ctx, sec);
        if segs.is_empty() {
            return None;
        }
        let mut ents = Entities::default();
        for (a, b) in segs {
            let va = ents.find_vertex(a).unwrap_or_else(|| ents.add_vertex_raw(a));
            let vb = ents.find_vertex(b).unwrap_or_else(|| ents.add_vertex_raw(b));
            ents.add_edge_raw(va, vb);
        }
        let name = self.unique_def_name("Groupe");
        let def = self.defs.insert(Definition {
            name,
            is_group: true,
            description: String::new(),
            entities: ents,
            attributes: Default::default(),
            ext: Default::default(),
        });
        Some(self.entities_mut(ctx).instances.insert(Instance {
            def,
            xform: DMat4::IDENTITY,
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        }))
    }

    /// Next default symbol for a section plane: "1", "2"…
    pub fn next_section_symbol(&self) -> String {
        let mut used = std::collections::HashSet::new();
        let mut note = |e: &Entities| used.extend(e.sections.values().map(|s| s.symbol.clone()));
        note(&self.root);
        self.defs.values().for_each(|d| note(&d.entities));
        (1..).map(|i| i.to_string()).find(|s| !used.contains(s)).unwrap()
    }
}
