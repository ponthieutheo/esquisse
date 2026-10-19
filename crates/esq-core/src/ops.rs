//! "Sticky geometry" operations: drawing edges splits what they cross, closed coplanar
//! loops become faces, edges drawn across a face split it, erasing merges or removes faces.

use std::collections::{HashMap, HashSet};

use glam::{DMat4, DVec2, DVec3};

use crate::math::{Plane, TOL, point_in_polygon_2d, project_on_segment, segment_segment, signed_area_2d};
use crate::model::{EdgeId, Entities, EntityRef, FaceId, InstanceId, VertexId};
use crate::tess;

type EdgeKey = (VertexId, VertexId);

/// Whether new edges split the edges they cross (`Sketchup.break_edges=`, SketchUp's
/// preference `Preferences/IntersectEdges`, on by default): a setting of the application, so
/// one switch for every model and thread, as in SketchUp.
static BREAK_EDGES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_break_edges(on: bool) {
    BREAK_EDGES.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn break_edges() -> bool {
    BREAK_EDGES.load(std::sync::atomic::Ordering::Relaxed)
}

fn edge_key(a: VertexId, b: VertexId) -> EdgeKey {
    if a < b { (a, b) } else { (b, a) }
}

/// A face `rebuild_plane` made (or kept for a piece), over which old face, and its loops in
/// the plane: for the annotations attached to the old faces (`attach.rs`).
type Made = (FaceId, Option<FaceId>, Vec<DVec2>, Vec<Vec<DVec2>>);

/// A region of a plane found by tracing the planar edge graph.
struct Region {
    outer: Vec<VertexId>,
    holes: Vec<Vec<VertexId>>,
}

impl Entities {
    pub fn find_vertex(&self, p: DVec3) -> Option<VertexId> {
        let mut best = None;
        let mut best_d = TOL;
        for (id, v) in self.verts.iter() {
            let d = v.pos.distance(p);
            if d < best_d {
                best_d = d;
                best = Some(id);
            }
        }
        best
    }

    /// Vertex at `p`: an existing one, a new one splitting the edge that passes through `p`,
    /// or a free new vertex.
    pub fn vertex_at(&mut self, p: DVec3) -> VertexId {
        if let Some(v) = self.find_vertex(p) {
            return v;
        }
        let hit = self.edges.iter().find_map(|(e, edge)| {
            let (a, b) = (self.verts[edge.v[0]].pos, self.verts[edge.v[1]].pos);
            let (t, d) = project_on_segment(p, a, b);
            (d < TOL && t > 0.0 && t < 1.0).then_some(e)
        });
        match hit {
            Some(e) => self.split_edge(e, p),
            None => self.add_vertex_raw(p),
        }
    }

    /// Splits edge `e` at `p`, keeping faces that use it consistent.
    pub fn split_edge(&mut self, e: EdgeId, p: DVec3) -> VertexId {
        let edge = self.edges[e].clone();
        let [a, b] = edge.v;
        let v = self.add_vertex_raw(p);
        self.remove_edge_raw(e);
        for (x, y) in [(a, v), (v, b)] {
            if let Some(ne) = self.add_edge_raw(x, y) {
                let n = &mut self.edges[ne];
                n.soft = edge.soft;
                n.smooth = edge.smooth;
                n.hidden = edge.hidden;
                n.layer = edge.layer;
                n.curve = edge.curve;
                // The first piece is the edge, for what is attached to it.
                if x == a {
                    self.note_replaced(crate::attach::Replaced::Edge { old: e, new: ne, split: true });
                }
            }
        }
        for &f in &edge.faces {
            let Some(face) = self.faces.get_mut(f) else { continue };
            for l in std::iter::once(&mut face.outer).chain(face.holes.iter_mut()) {
                let n = l.len();
                if let Some(i) = (0..n).find(|&i| {
                    let (x, y) = (l[i], l[(i + 1) % n]);
                    (x == a && y == b) || (x == b && y == a)
                }) {
                    l.insert(i + 1, v);
                }
            }
            for ne in [self.find_edge(a, v), self.find_edge(v, b)].into_iter().flatten() {
                if !self.edges[ne].faces.contains(&f) {
                    self.edges[ne].faces.push(f);
                }
            }
        }
        v
    }

    /// Draws a line from `p` to `q` the way SketchUp does: it snaps to existing vertices,
    /// splits the edges it touches or crosses, and creates or splits faces.
    /// Returns the edges that now make up the line.
    pub fn add_line(&mut self, p: DVec3, q: DVec3) -> Vec<EdgeId> {
        let chain = self.insert_line_edges(p, q);
        self.heal_faces_around(&chain);
        chain
    }

    /// Draws several connected segments as one operation (rectangles, circles, polylines).
    pub fn add_polyline(&mut self, pts: &[DVec3], closed: bool, as_curve: bool) -> Vec<EdgeId> {
        self.polyline(pts, closed, as_curve, true)
    }

    /// Like [`Self::add_polyline`] for the Ruby API: the new edges split the faces they lie on,
    /// but a loop of loose edges makes no new face. SketchUp's `Entities#add_edges`,
    /// `#add_line`, `#add_circle`… leave that to `Edge#find_faces` and `Entities#add_face`.
    pub fn add_polyline_edges(&mut self, pts: &[DVec3], closed: bool, as_curve: bool) -> Vec<EdgeId> {
        self.polyline(pts, closed, as_curve, false)
    }

    fn polyline(&mut self, pts: &[DVec3], closed: bool, as_curve: bool, new_faces: bool) -> Vec<EdgeId> {
        let mut all = Vec::new();
        let n = pts.len();
        let segs = if closed { n } else { n.saturating_sub(1) };
        for i in 0..segs {
            all.extend(self.insert_line_edges(pts[i], pts[(i + 1) % n]));
        }
        if as_curve && !all.is_empty() {
            let id = self.new_curve_id();
            for &e in &all {
                if let Some(edge) = self.edges.get_mut(e) {
                    edge.curve = Some(id);
                }
            }
        }
        all.retain(|e| self.edges.contains_key(*e));
        self.heal_faces(&all, new_faces);
        all.retain(|e| self.edges.contains_key(*e));
        all
    }

    fn insert_line_edges(&mut self, p: DVec3, q: DVec3) -> Vec<EdgeId> {
        if p.distance(q) < TOL {
            return Vec::new();
        }
        let va = self.vertex_at(p);
        let vb = self.vertex_at(q);
        if va == vb {
            return Vec::new();
        }
        let (p, q) = (self.pos(va), self.pos(vb));
        let len = p.distance(q);
        let eps = TOL / len;
        let mut stops: Vec<(f64, VertexId)> = vec![(0.0, va), (1.0, vb)];
        for (id, v) in self.verts.iter() {
            if id == va || id == vb {
                continue;
            }
            let (t, d) = project_on_segment(v.pos, p, q);
            if d < TOL && t > eps && t < 1.0 - eps {
                stops.push((t, id));
            }
        }
        // With break edges off, the edges it crosses stay whole (SketchUp skips the
        // intersection pass, when IntersectEdges is off).
        let candidates: Vec<EdgeId> = if break_edges() { self.edges.keys().collect() } else { Vec::new() };
        for e in candidates {
            let Some(edge) = self.edges.get(e) else { continue };
            let [ea, eb] = edge.v;
            if ea == va || ea == vb || eb == va || eb == vb {
                continue;
            }
            let (a, b) = (self.pos(ea), self.pos(eb));
            let (s, t, d) = segment_segment(p, q, a, b);
            let elen = a.distance(b);
            if d < TOL && s > eps && s < 1.0 - eps && t * elen > TOL && (1.0 - t) * elen > TOL {
                let x = p + (q - p) * s;
                let v = self.split_edge(e, x);
                stops.push((s, v));
            }
        }
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        stops.dedup_by_key(|s| s.1);
        let mut out = Vec::new();
        for w in stops.windows(2) {
            if let Some(e) = self.add_edge_raw(w[0].1, w[1].1) {
                out.push(e);
            }
        }
        out
    }

    /// Re-detects faces in every plane that contains one of `fresh` edges and another edge
    /// touching it (or a face it lies on).
    pub fn heal_faces_around(&mut self, fresh: &[EdgeId]) {
        self.heal_faces(fresh, true);
    }

    /// Splits the faces `fresh` edges lie on, making no face of a loop of loose edges: a face
    /// added inside another of its plane (`Entities#add_face`) gives it a hole.
    pub fn split_faces_with(&mut self, fresh: &[EdgeId]) {
        self.heal_faces(fresh, false);
    }

    /// `new_faces`: whether a loop of loose edges that closes becomes a face; when false, only
    /// the faces the edges lie on are split.
    fn heal_faces(&mut self, fresh: &[EdgeId], new_faces: bool) {
        // `rebuild_plane` only fills a region that no face covered when it holds a fresh edge.
        let fresh_set: HashSet<EdgeId> = if new_faces { fresh.iter().copied().collect() } else { HashSet::new() };
        let mut planes: Vec<Plane> = Vec::new();
        let push_plane = |pl: Plane, planes: &mut Vec<Plane>| {
            if !planes.iter().any(|x| x.same_as(&pl)) {
                planes.push(pl);
            }
        };
        for &e in fresh {
            let Some(edge) = self.edges.get(e) else { continue };
            let (a, b) = (self.pos(edge.v[0]), self.pos(edge.v[1]));
            let dir = (b - a).normalize();
            for &v in &edge.v {
                let pv = self.pos(v);
                for &e2 in &self.verts[v].edges {
                    if e2 == e {
                        continue;
                    }
                    let o = self.pos(self.other_vertex(e2, v));
                    let n = dir.cross((o - pv).normalize());
                    if n.length() > 1e-6 {
                        push_plane(Plane::from_point_normal(a, n), &mut planes);
                    }
                }
            }
            for (_, face) in self.faces.iter() {
                let pl = Plane::from_point_normal(self.pos(face.outer[0]), face.normal);
                if pl.contains(a) && pl.contains(b) {
                    push_plane(pl, &mut planes);
                }
            }
        }
        for pl in planes {
            self.rebuild_plane(&pl, &fresh_set);
        }
    }

    /// Recomputes the faces of one plane from its edge graph.
    fn rebuild_plane(&mut self, plane: &Plane, fresh: &HashSet<EdgeId>) {
        let plane = plane.canonical();
        let (bu, bv) = plane.basis();
        let to2 = |p: DVec3| DVec2::new(p.dot(bu), p.dot(bv));

        // Edge graph restricted to the plane.
        let mut adj: HashMap<VertexId, Vec<VertexId>> = HashMap::new();
        for (_, edge) in self.edges.iter() {
            let [a, b] = edge.v;
            if plane.contains(self.pos(a)) && plane.contains(self.pos(b)) {
                adj.entry(a).or_default().push(b);
                adj.entry(b).or_default().push(a);
            }
        }
        // Dangling edges never bound a face.
        loop {
            let leaves: Vec<VertexId> = adj.iter().filter(|(_, n)| n.len() <= 1).map(|(k, _)| *k).collect();
            if leaves.is_empty() {
                break;
            }
            for l in leaves {
                if let Some(ns) = adj.remove(&l) {
                    for n in ns {
                        if let Some(v) = adj.get_mut(&n) {
                            v.retain(|&x| x != l);
                        }
                    }
                }
            }
        }
        let p2: HashMap<VertexId, DVec2> = adj.keys().map(|&v| (v, to2(self.pos(v)))).collect();

        // Trace every directed half-edge into cycles (bounded regions are counter-clockwise).
        let mut visited: HashSet<(VertexId, VertexId)> = HashSet::new();
        let mut ccw: Vec<Vec<VertexId>> = Vec::new();
        let mut cw: Vec<Vec<VertexId>> = Vec::new();
        let starts: Vec<(VertexId, VertexId)> =
            adj.iter().flat_map(|(&u, ns)| ns.iter().map(move |&v| (u, v))).collect();
        for start in starts {
            if visited.contains(&start) {
                continue;
            }
            let mut cycle = Vec::new();
            let (mut u, mut v) = start;
            let mut guard = 0;
            loop {
                visited.insert((u, v));
                cycle.push(u);
                let back = p2[&u] - p2[&v];
                let a_in = back.y.atan2(back.x);
                let mut best: Option<(f64, VertexId)> = None;
                for &w in &adj[&v] {
                    let d = p2[&w] - p2[&v];
                    let mut cwa = (a_in - d.y.atan2(d.x)).rem_euclid(std::f64::consts::TAU);
                    if w == u || cwa < 1e-12 {
                        cwa = std::f64::consts::TAU;
                    }
                    if best.is_none_or(|(b, _)| cwa < b) {
                        best = Some((cwa, w));
                    }
                }
                let w = best.map(|b| b.1).unwrap_or(u);
                u = v;
                v = w;
                guard += 1;
                if (u, v) == start || guard > 100_000 {
                    break;
                }
            }
            let pts: Vec<DVec2> = cycle.iter().map(|v| p2[v]).collect();
            let area = signed_area_2d(&pts);
            if area > TOL * TOL {
                ccw.push(cycle);
            } else if area < -TOL * TOL {
                cw.push(cycle);
            }
        }

        // Each clockwise cycle is the outside of a connected piece; it is a hole of the
        // smallest counter-clockwise cycle (from another piece) that contains it.
        let poly = |l: &Vec<VertexId>| -> Vec<DVec2> { l.iter().map(|v| p2[v]).collect() };
        let ccw_polys: Vec<Vec<DVec2>> = ccw.iter().map(poly).collect();
        let ccw_area: Vec<f64> = ccw_polys.iter().map(|p| signed_area_2d(p)).collect();
        let mut regions: Vec<Region> = ccw.iter().map(|l| Region { outer: l.clone(), holes: Vec::new() }).collect();
        for hole in &cw {
            let members: HashSet<VertexId> = hole.iter().copied().collect();
            let probe = p2[&hole[0]];
            let mut best: Option<usize> = None;
            for (i, outer) in ccw.iter().enumerate() {
                if outer.iter().any(|v| members.contains(v)) {
                    continue;
                }
                if point_in_polygon_2d(probe, &ccw_polys[i]) && best.is_none_or(|b| ccw_area[i] < ccw_area[b]) {
                    best = Some(i);
                }
            }
            if let Some(i) = best {
                regions[i].holes.push(hole.clone());
            }
        }

        // Existing faces of this plane.
        let old: Vec<FaceId> = self
            .faces
            .iter()
            .filter(|(_, f)| f.normal.dot(plane.n).abs() > 1.0 - 1e-6 && plane.contains(self.pos(f.outer[0])))
            .map(|(k, _)| k)
            .collect();
        let old_polys: Vec<(FaceId, Vec<DVec2>, Vec<Vec<DVec2>>)> = old
            .iter()
            .map(|&f| {
                let face = &self.faces[f];
                let o = face.outer.iter().map(|&v| to2(self.pos(v))).collect();
                let h = face.holes.iter().map(|l| l.iter().map(|&v| to2(self.pos(v))).collect()).collect();
                (f, o, h)
            })
            .collect();
        let inside = |p: DVec2, outer: &[DVec2], holes: &[Vec<DVec2>]| {
            point_in_polygon_2d(p, outer) && !holes.iter().any(|h| point_in_polygon_2d(p, h))
        };
        let old_keys: Vec<HashSet<EdgeKey>> = old.iter().map(|&f| loop_keys(self.faces[f].loops())).collect();

        let mut kept: HashSet<FaceId> = HashSet::new();
        let mut filled_polys: Vec<(Vec<DVec2>, Vec<Vec<DVec2>>)> = Vec::new();
        let mut to_create: Vec<(Region, Option<FaceId>)> = Vec::new();
        for r in regions {
            let o2: Vec<DVec2> = r.outer.iter().map(|v| p2[v]).collect();
            let h2: Vec<Vec<DVec2>> = r.holes.iter().map(|l| l.iter().map(|v| p2[v]).collect()).collect();
            let Some(sample) = tess::interior_point(&o2, &h2) else { continue };
            let cover = old_polys.iter().find(|(_, o, h)| inside(sample, o, h)).map(|x| x.0);
            let keys = region_keys(&r);
            let is_fresh = keys.iter().any(|&(a, b)| self.find_edge(a, b).is_some_and(|e| fresh.contains(&e)));
            if cover.is_none() && !is_fresh {
                continue;
            }
            if let Some(i) = old_keys.iter().position(|k| *k == keys) {
                kept.insert(old[i]);
            } else {
                to_create.push((r, cover));
            }
            filled_polys.push((o2, h2));
        }

        // Old faces superseded by the new regions disappear; untouched ones stay as they are.
        let mut superseded = Vec::new();
        for (f, o, h) in &old_polys {
            if kept.contains(f) {
                continue;
            }
            let Some(s) = tess::interior_point(o, h) else { continue };
            if filled_polys.iter().any(|(fo, fh)| inside(s, fo, fh)) {
                superseded.push(*f);
            }
        }
        let attrs: HashMap<FaceId, crate::model::Face> = old.iter().map(|&f| (f, self.faces[f].clone())).collect();
        // Like SketchUp, a face that is cut stays the same face for one of its pieces: the
        // one that keeps most of its outer loop (then the largest). A face that gets a hole
        // is still the face the scripts hold.
        let mut reuse: HashMap<usize, FaceId> = HashMap::new();
        for &f in &superseded {
            let outer = loop_keys(std::iter::once(&attrs[&f].outer));
            let score = |r: &Region| {
                let shared = loop_keys(std::iter::once(&r.outer)).intersection(&outer).count();
                let area = signed_area_2d(&r.outer.iter().map(|v| p2[v]).collect::<Vec<_>>()).abs();
                (shared, area)
            };
            let best = to_create
                .iter()
                .enumerate()
                .filter(|(i, (_, cover))| *cover == Some(f) && !reuse.contains_key(i))
                .map(|(i, (r, _))| (i, score(r)))
                .max_by(|a, b| a.1.0.cmp(&b.1.0).then(a.1.1.total_cmp(&b.1.1)));
            if let Some((i, _)) = best {
                reuse.insert(i, f);
            }
        }
        let reused: HashSet<FaceId> = reuse.values().copied().collect();
        // Where the old faces' samples are, for what is attached to them (`attach.rs`).
        let samples: Vec<(FaceId, Option<DVec2>)> = superseded
            .iter()
            .map(|f| (*f, old_polys.iter().find(|x| x.0 == *f).and_then(|(_, o, h)| tess::interior_point(o, h))))
            .collect();
        let mut made: Vec<Made> = Vec::new();
        for f in superseded {
            if reused.contains(&f) {
                for e in self.face_edges(f) {
                    if let Some(edge) = self.edges.get_mut(e) {
                        edge.faces.retain(|&x| x != f);
                    }
                }
            } else {
                self.remove_face(f);
            }
        }
        for (i, (r, cover)) in to_create.into_iter().enumerate() {
            let flip = match cover {
                Some(c) => attrs[&c].normal.dot(plane.n) < 0.0,
                None => self.orientation_from_neighbours(&r).unwrap_or_else(|| default_flip(&plane)),
            };
            let (mut outer, mut holes) = (r.outer, r.holes);
            if flip {
                outer.reverse();
                for h in holes.iter_mut() {
                    h.reverse();
                }
            }
            let o2: Vec<DVec2> = outer.iter().map(|v| p2[v]).collect();
            let h2: Vec<Vec<DVec2>> = holes.iter().map(|l| l.iter().map(|v| p2[v]).collect()).collect();
            if let Some(&f) = reuse.get(&i) {
                let face = &mut self.faces[f];
                face.outer = outer;
                face.holes = holes;
                face.normal = if flip { -plane.n } else { plane.n };
                self.link_face(f);
                made.push((f, cover, o2, h2));
                continue;
            }
            if let Some(nf) = self.add_face_raw(outer, holes) {
                made.push((nf, cover, o2, h2));
                self.faces[nf].normal = if flip { -plane.n } else { plane.n };
                if let Some(c) = cover {
                    let a = &attrs[&c];
                    let face = &mut self.faces[nf];
                    face.front = a.front;
                    face.back = a.back;
                    face.layer = a.layer;
                    face.hidden = a.hidden;
                }
            }
        }
        // Each old face is now the faces made over it, or that hold its sample (a merge).
        for (f, sample) in samples {
            let new: Vec<FaceId> = made
                .iter()
                .filter(|(_, cover, o, h)| *cover == Some(f) || sample.is_some_and(|s| inside(s, o, h)))
                .map(|x| x.0)
                .collect();
            if !new.is_empty() {
                self.note_replaced(crate::attach::Replaced::Face { old: f, new, moved: false });
            }
        }
    }

    /// Orientation that agrees with a face already attached to the region's outer loop.
    /// Returns `Some(true)` if the region (counter-clockwise in plane basis) must be flipped.
    fn orientation_from_neighbours(&self, r: &Region) -> Option<bool> {
        let n = r.outer.len();
        for i in 0..n {
            let (a, b) = (r.outer[i], r.outer[(i + 1) % n]);
            let Some(e) = self.find_edge(a, b) else { continue };
            for &f in &self.edges[e].faces {
                let Some(face) = self.faces.get(f) else { continue };
                for l in face.loops() {
                    let m = l.len();
                    for j in 0..m {
                        let (x, y) = (l[j], l[(j + 1) % m]);
                        if x == a && y == b {
                            return Some(true);
                        }
                        if x == b && y == a {
                            return Some(false);
                        }
                    }
                }
            }
        }
        None
    }

    /// Erases edges. A face bounded by an erased edge disappears, except when the edge sat
    /// between two coplanar faces: they merge into one.
    pub fn erase_edges(&mut self, edges: &[EdgeId]) {
        let mut merge: Vec<Plane> = Vec::new();
        let mut touched: HashSet<VertexId> = HashSet::new();
        for &e in edges {
            let Some(edge) = self.edges.get(e) else { continue };
            let faces = edge.faces.clone();
            touched.extend(edge.v);
            let coplanar = faces.len() == 2
                && self.faces.contains_key(faces[0])
                && self.faces.contains_key(faces[1])
                && self.face_plane(faces[0]).same_as(&self.face_plane(faces[1]));
            if coplanar {
                let pl = self.face_plane(faces[0]);
                if !merge.iter().any(|m| m.same_as(&pl)) {
                    merge.push(pl);
                }
            } else {
                for f in faces {
                    if self.faces.contains_key(f) {
                        self.remove_face(f);
                    }
                }
            }
            self.remove_edge_raw(e);
        }
        for pl in merge {
            self.rebuild_plane(&pl, &HashSet::new());
        }
        self.heal_collinear(&touched);
        self.remove_orphan_vertices();
    }

    pub fn erase(&mut self, items: &[EntityRef]) {
        let mut edges = Vec::new();
        for it in items {
            match *it {
                EntityRef::Face(f) => {
                    if self.faces.contains_key(f) {
                        self.remove_face(f);
                    }
                }
                EntityRef::Instance(i) => {
                    if self.instances.get(i).is_some_and(|x| !x.locked) {
                        self.instances.remove(i);
                    }
                }
                EntityRef::Guide(g) => {
                    self.guides.remove(g);
                }
                EntityRef::Annot(a) => {
                    self.annots.remove(a);
                }
                EntityRef::Section(x) => {
                    self.sections.remove(x);
                    if self.active_section == Some(x) {
                        self.active_section = None;
                    }
                }
                EntityRef::Snap(id) => {
                    self.snaps.retain(|s| s.id != id);
                    self.props.remove(&format!("n{id}"));
                }
                EntityRef::Space(id) => {
                    self.spaces.retain(|s| s.id != id);
                    self.props.remove(&format!("p{id}"));
                }
                EntityRef::Edge(e) => edges.push(e),
            }
        }
        self.erase_edges(&edges);
        self.remove_orphan_vertices();
    }

    /// Joins two collinear edges meeting at a vertex that nothing else uses.
    pub fn heal_collinear(&mut self, candidates: &HashSet<VertexId>) {
        for &v in candidates {
            let Some(vx) = self.verts.get(v) else { continue };
            if vx.edges.len() != 2 {
                continue;
            }
            let (e1, e2) = (vx.edges[0], vx.edges[1]);
            let (a, b) = (self.other_vertex(e1, v), self.other_vertex(e2, v));
            let (pa, pv, pb) = (self.pos(a), self.pos(v), self.pos(b));
            let (_, d) = project_on_segment(pv, pa, pb);
            let straight = (pv - pa).dot(pb - pv) > 0.0;
            let (x, y) = (&self.edges[e1], &self.edges[e2]);
            let same = x.soft == y.soft && x.smooth == y.smooth && x.layer == y.layer && x.hidden == y.hidden;
            if d > TOL || !straight || !same || self.find_edge(a, b).is_some() {
                continue;
            }
            let faces: Vec<FaceId> = x.faces.clone();
            let (soft, smooth, hidden, layer, curve) = (x.soft, x.smooth, x.hidden, x.layer, x.curve);
            self.remove_edge_raw(e1);
            self.remove_edge_raw(e2);
            if let Some(ne) = self.add_edge_raw(a, b) {
                self.note_replaced(crate::attach::Replaced::Edge { old: e1, new: ne, split: false });
                self.note_replaced(crate::attach::Replaced::Edge { old: e2, new: ne, split: false });
                let n = &mut self.edges[ne];
                (n.soft, n.smooth, n.hidden, n.layer, n.curve) = (soft, smooth, hidden, layer, curve);
                for &f in &faces {
                    if let Some(face) = self.faces.get_mut(f) {
                        face.outer.retain(|&x| x != v);
                        for h in face.holes.iter_mut() {
                            h.retain(|&x| x != v);
                        }
                        self.edges[ne].faces.push(f);
                    }
                }
            }
            self.verts.remove(v);
        }
    }

    /// Push/Pull. Positive `dist` goes along the face normal. Without `keep_base`, a face
    /// whose neighbours are all perpendicular is moved (its sides stretch), as in SketchUp.
    /// Returns the face at the new position.
    pub fn push_pull(&mut self, f: FaceId, dist: f64, keep_base: bool) -> Option<FaceId> {
        if dist.abs() < TOL || !self.faces.contains_key(f) {
            return None;
        }
        let face = self.faces[f].clone();
        let n = face.normal;
        let offset = n * dist;

        let face_verts: HashSet<VertexId> = face.all_vertices().collect();
        let neighbours_perpendicular = face_verts.iter().all(|&v| {
            self.verts[v]
                .edges
                .iter()
                .all(|&e| self.edges[e].faces.iter().all(|&g| g == f || self.faces[g].normal.dot(n).abs() < 1e-6))
        });
        let has_neighbours = self.face_edges(f).iter().any(|&e| self.edges[e].faces.len() > 1);
        let side_edges_ok = face_verts.iter().all(|&v| {
            self.verts[v].edges.iter().all(|&e| {
                let o = self.other_vertex(e, v);
                face_verts.contains(&o) || (self.pos(o) - self.pos(v)).normalize().dot(n).abs() > 1.0 - 1e-6
            })
        });
        if !keep_base && has_neighbours && neighbours_perpendicular && side_edges_ok {
            for &v in &face_verts {
                self.verts[v].pos += offset;
            }
            return Some(f);
        }

        // Extrude: new cap, one side face per loop edge.
        let mut map: HashMap<VertexId, VertexId> = HashMap::new();
        for &v in &face_verts {
            let p = self.pos(v) + offset;
            let nv = self.find_vertex(p).unwrap_or_else(|| self.add_vertex_raw(p));
            map.insert(v, nv);
        }
        let mut curve_map: HashMap<u32, u32> = HashMap::new();
        for l in face.loops() {
            let m = l.len();
            for i in 0..m {
                let (a, b) = (l[i], l[(i + 1) % m]);
                let quad = if dist > 0.0 { vec![a, b, map[&b], map[&a]] } else { vec![map[&a], map[&b], b, a] };
                if let Some(sf) = self.add_face_raw(quad, Vec::new()) {
                    self.faces[sf].layer = face.layer;
                }
                // The cap edge inherits the curve (a polygon stays a polygon); the side edge
                // between two segments of the same curve is softened so cylinders look round,
                // but not those of a polygon, whose sides SketchUp keeps hard.
                let curve = self.find_edge(a, b).and_then(|e| self.edges[e].curve);
                if let Some(c) = curve {
                    let kind = self.prop(&format!("c{c}"), "kind").cloned();
                    let polygon = matches!(&kind, Some(crate::model::AttrValue::Text(t)) if t == "polygon");
                    let nc = match curve_map.get(&c) {
                        Some(&nc) => nc,
                        None => {
                            let nc = self.new_curve_id();
                            if let Some(k) = kind {
                                self.set_prop(&format!("c{nc}"), "kind", k);
                            }
                            curve_map.insert(c, nc);
                            nc
                        }
                    };
                    if let Some(ce) = self.find_edge(map[&a], map[&b]) {
                        self.edges[ce].curve = Some(nc);
                    }
                    let prev = self.find_edge(l[(i + m - 1) % m], a).and_then(|e| self.edges[e].curve);
                    if prev == Some(c)
                        && !polygon
                        && let Some(se) = self.find_edge(a, map[&a])
                    {
                        self.edges[se].soft = true;
                        self.edges[se].smooth = true;
                    }
                }
            }
        }
        let mut cap_outer: Vec<VertexId> = face.outer.iter().map(|v| map[v]).collect();
        let mut cap_holes: Vec<Vec<VertexId>> = face.holes.iter().map(|h| h.iter().map(|v| map[v]).collect()).collect();
        if dist < 0.0 {
            cap_outer.reverse();
            for h in cap_holes.iter_mut() {
                h.reverse();
            }
        }
        let cap = self.add_face_raw(cap_outer, cap_holes);
        if let Some(c) = cap {
            let cf = &mut self.faces[c];
            cf.front = face.front;
            cf.back = face.back;
            cf.layer = face.layer;
            // The face moved along its normal keeps its texture: a placement does not change
            // along the normal of its frame, and a projected one stays where it is projected.
            cf.tex = face.tex.clone();
            // The pushed face is the cap now, for what is attached to it (SketchUp moves the
            // face itself).
            self.note_replaced(crate::attach::Replaced::Face { old: f, new: vec![c], moved: true });
        }
        if has_neighbours && !keep_base {
            self.remove_face(f);
        } else if dist > 0.0 {
            self.reverse_face(f);
        }
        cap
    }

    /// Push/Pull in stretch mode: the face moves along its normal with its edges attached,
    /// whatever surrounds it, and nothing is extruded.
    pub fn stretch_face(&mut self, f: FaceId, dist: f64) -> bool {
        let Some(face) = self.faces.get(f) else { return false };
        if dist.abs() < TOL {
            return false;
        }
        let m = DMat4::from_translation(face.normal * dist);
        let verts: HashSet<VertexId> = face.all_vertices().collect();
        self.transform_vertices(&verts, &m);
        true
    }

    /// Applies a transformation to a set of vertices, then welds them onto vertices they now
    /// coincide with.
    pub fn transform_vertices(&mut self, verts: &HashSet<VertexId>, m: &DMat4) {
        for &v in verts {
            if let Some(vx) = self.verts.get_mut(v) {
                vx.pos = m.transform_point3(vx.pos);
            }
        }
        self.settle_moved(verts);
    }

    /// Moves each vertex by its own transformation, all at the same time, as SketchUp's
    /// `Entities#transform_by_vectors` does: a vertex listed twice keeps its
    /// first one. The faces around are then re-oriented and the moved vertices welded, as by
    /// [`Self::transform_vertices`].
    pub fn transform_vertices_each(&mut self, moves: &[(VertexId, DMat4)]) {
        let mut moved = HashSet::new();
        for &(v, m) in moves {
            if moved.insert(v)
                && let Some(vx) = self.verts.get_mut(v)
            {
                vx.pos = m.transform_point3(vx.pos);
            }
        }
        self.settle_moved(&moved);
    }

    /// After vertices moved: the normals of the faces around, then the welding.
    fn settle_moved(&mut self, verts: &HashSet<VertexId>) {
        let faces: HashSet<FaceId> = verts
            .iter()
            .filter_map(|v| self.verts.get(*v))
            .flat_map(|v| v.edges.iter().flat_map(|&e| self.edges[e].faces.iter().copied()))
            .collect();
        for f in faces {
            if self.faces.contains_key(f) {
                self.recompute_normal(f);
            }
        }
        self.weld(verts);
    }

    /// Merges each listed vertex into another vertex at the same place, if any: the first one
    /// of the collection (in the order of its slots) closer than `TOL` among those not listed.
    pub fn weld(&mut self, verts: &HashSet<VertexId>) {
        // The vertices not listed (merging never moves nor removes them), by cell of side
        // 2·TOL: one closer than TOL to a point is in the point's cell or a neighbouring one.
        let side = 2.0 * TOL;
        let cell = |p: DVec3| [p.x, p.y, p.z].map(|c| (c / side).floor() as i64);
        let mut grid: HashMap<[i64; 3], Vec<VertexId>> = HashMap::new();
        for (id, x) in self.verts.iter() {
            if !verts.contains(&id) {
                grid.entry(cell(x.pos)).or_default().push(id);
            }
        }
        if grid.is_empty() {
            return;
        }
        let slot = |id: VertexId| slotmap::Key::data(&id).as_ffi() as u32;
        for &v in verts {
            let Some(p) = self.verts.get(v).map(|x| x.pos) else { continue };
            let c = cell(p);
            let mut target: Option<VertexId> = None;
            for d in 0..27 {
                let k = [d % 3, d / 3 % 3, d / 9].map(|o| o as i64 - 1);
                let key = [0, 1, 2].map(|i| c[i].saturating_add(k[i]));
                for &id in grid.get(&key).into_iter().flatten() {
                    #[cfg(test)]
                    weld_tests::LOOKS.with(|n| n.set(n.get() + 1));
                    if id != v
                        && self.verts.get(id).is_some_and(|x| x.pos.distance(p) < TOL)
                        && target.is_none_or(|t| slot(id) < slot(t))
                    {
                        target = Some(id);
                    }
                }
            }
            if let Some(t) = target {
                self.merge_vertex(v, t);
            }
        }
    }

    /// Replaces every use of `from` by `to`.
    pub fn merge_vertex(&mut self, from: VertexId, to: VertexId) {
        let edges = self.verts[from].edges.clone();
        for e in edges {
            let edge = self.edges[e].clone();
            let o = if edge.v[0] == from { edge.v[1] } else { edge.v[0] };
            self.remove_edge_raw(e);
            if o != to
                && let Some(ne) = self.add_edge_raw(o, to)
            {
                self.note_replaced(crate::attach::Replaced::Edge { old: e, new: ne, split: false });
                let n = &mut self.edges[ne];
                n.soft |= edge.soft;
                n.smooth |= edge.smooth;
                n.curve = n.curve.or(edge.curve);
            }
        }
        self.note_replaced(crate::attach::Replaced::Vertex(from, to));
        let fids: Vec<FaceId> = self.faces.keys().collect();
        for f in fids {
            let face = &mut self.faces[f];
            let mut changed = false;
            for l in std::iter::once(&mut face.outer).chain(face.holes.iter_mut()) {
                for x in l.iter_mut() {
                    if *x == from {
                        *x = to;
                        changed = true;
                    }
                }
                l.dedup();
                while l.len() > 1 && l.first() == l.last() {
                    l.pop();
                }
            }
            if changed {
                if face.outer.len() < 3 {
                    self.remove_face(f);
                } else {
                    self.link_face(f);
                }
            }
        }
        self.verts.remove(from);
    }

    /// Vertices used by a selection (faces bring their loops, edges their ends,
    /// instances nothing).
    pub fn vertices_of(&self, items: &[EntityRef]) -> HashSet<VertexId> {
        let mut out = HashSet::new();
        for it in items {
            match *it {
                EntityRef::Edge(e) => {
                    if let Some(edge) = self.edges.get(e) {
                        out.extend(edge.v);
                    }
                }
                EntityRef::Face(f) => {
                    if let Some(face) = self.faces.get(f) {
                        out.extend(face.all_vertices());
                    }
                }
                EntityRef::Instance(_)
                | EntityRef::Guide(_)
                | EntityRef::Annot(_)
                | EntityRef::Section(_)
                | EntityRef::Snap(_)
                | EntityRef::Space(_) => {}
            }
        }
        out
    }

    /// Moves/rotates/scales a selection: geometry vertices, instance placements and guides.
    pub fn transform_selection(&mut self, items: &[EntityRef], m: &DMat4) {
        let verts = self.vertices_of(items);
        for it in items {
            match *it {
                EntityRef::Instance(i) => {
                    if let Some(inst) = self.instances.get_mut(i).filter(|x| !x.locked) {
                        inst.xform = *m * inst.xform;
                    }
                }
                EntityRef::Guide(g) => {
                    if let Some(guide) = self.guides.get_mut(g) {
                        *guide = guide.transformed(m);
                    }
                }
                EntityRef::Annot(a) => {
                    if let Some(an) = self.annots.get_mut(a) {
                        *an = an.transformed(m);
                    }
                }
                EntityRef::Section(x) => {
                    if let Some(sp) = self.sections.get_mut(x) {
                        *sp = sp.transformed(m);
                    }
                }
                EntityRef::Snap(id) => {
                    if let Some(i) = self.snap_index(id) {
                        self.snaps[i] = self.snaps[i].transformed(m);
                    }
                }
                EntityRef::Space(id) => {
                    if let Some(s) = self.space_mut(id) {
                        *s = s.transformed(m);
                    }
                }
                _ => {}
            }
        }
        self.transform_vertices(&verts, m);
        // A mirror (Flip, negative scale) turns faces inside out: turn them back.
        if m.determinant() < 0.0 {
            let faces: Vec<FaceId> = self
                .faces
                .iter()
                .filter(|(_, f)| f.all_vertices().all(|v| verts.contains(&v)))
                .map(|(k, _)| k)
                .collect();
            for f in faces {
                let face = &mut self.faces[f];
                face.outer.reverse();
                face.holes.iter_mut().for_each(|h| h.reverse());
                self.recompute_normal(f);
            }
        }
    }

    /// Duplicates a selection inside the same collection; returns the copies.
    pub fn duplicate(&mut self, items: &[EntityRef]) -> Vec<EntityRef> {
        let mut map: HashMap<VertexId, VertexId> = HashMap::new();
        let verts = self.vertices_of(items);
        for v in verts {
            let p = self.pos(v);
            map.insert(v, self.add_vertex_raw(p));
        }
        let mut out = Vec::new();
        // What was copied, for the annotations copied along (`attach.rs`).
        let mut copied = crate::attach::Copied { verts: map.clone(), ..Default::default() };
        let mut annots = Vec::new();
        let mut edges_done: HashSet<EdgeId> = HashSet::new();
        for it in items {
            match *it {
                EntityRef::Face(f) => {
                    let Some(face) = self.faces.get(f).cloned() else { continue };
                    for e in self.face_edges(f) {
                        edges_done.insert(e);
                        let edge = self.edges[e].clone();
                        if let Some(ne) = self.add_edge_raw(map[&edge.v[0]], map[&edge.v[1]]) {
                            copy_edge_attrs(&edge, &mut self.edges[ne]);
                            copied.edges.insert(e, ne);
                        }
                    }
                    let outer = face.outer.iter().map(|v| map[v]).collect();
                    let holes = face.holes.iter().map(|h| h.iter().map(|v| map[v]).collect()).collect();
                    if let Some(nf) = self.add_face_raw(outer, holes) {
                        copied.faces.insert(f, nf);
                        let n = &mut self.faces[nf];
                        n.normal = face.normal;
                        n.front = face.front;
                        n.back = face.back;
                        n.layer = face.layer;
                        out.push(EntityRef::Face(nf));
                    }
                }
                EntityRef::Instance(i) => {
                    if let Some(mut inst) = self.instances.get(i).cloned() {
                        // A copy is a new instance: not the GUID a file gave the original.
                        inst.ext.guid.clear();
                        let id = self.instances.insert(inst);
                        copied.insts.insert(i, id);
                        out.push(EntityRef::Instance(id));
                    }
                }
                EntityRef::Guide(g) => {
                    if let Some(guide) = self.guides.get(g).cloned() {
                        let id = self.guides.insert(guide);
                        copied.guides.insert(g, id);
                        out.push(EntityRef::Guide(id));
                    }
                }
                EntityRef::Annot(a) => {
                    if let Some(an) = self.annots.get(a).cloned() {
                        let id = self.annots.insert(an);
                        annots.push(id);
                        out.push(EntityRef::Annot(id));
                    }
                }
                EntityRef::Section(x) => {
                    if let Some(sp) = self.sections.get(x).cloned() {
                        let id = self.sections.insert(sp);
                        copied.sections.insert(x, id);
                        out.push(EntityRef::Section(id));
                    }
                }
                EntityRef::Snap(id) => {
                    if let Some(mut s) = self.snap(id).copied() {
                        s.id = self.new_mark_id();
                        self.snaps.push(s);
                        out.push(EntityRef::Snap(s.id));
                    }
                }
                EntityRef::Space(id) => {
                    if let Some(mut s) = self.space(id).cloned() {
                        s.id = self.new_mark_id();
                        out.push(EntityRef::Space(s.id));
                        self.spaces.push(s);
                    }
                }
                EntityRef::Edge(_) => {}
            }
        }
        for it in items {
            if let EntityRef::Edge(e) = *it {
                if edges_done.contains(&e) {
                    continue;
                }
                let Some(edge) = self.edges.get(e).cloned() else { continue };
                if let Some(ne) = self.add_edge_raw(map[&edge.v[0]], map[&edge.v[1]]) {
                    copy_edge_attrs(&edge, &mut self.edges[ne]);
                    copied.edges.insert(e, ne);
                    out.push(EntityRef::Edge(ne));
                }
            }
        }
        // A copied annotation goes with the copies of what it is attached to; else it stays
        // attached to the originals (SketchUp).
        for id in annots {
            let an = &mut self.annots[id];
            for w in 0..2 {
                if let Some(a) = &an.attach[w] {
                    an.attach[w] = Some(copied.end(a, |t| copied.leaf(t)).unwrap_or_else(|| a.clone()));
                }
            }
        }
        out
    }

    /// Selection plus the edges bounding its faces (what SketchUp moves along with a face).
    pub fn with_bounding_edges(&self, items: &[EntityRef]) -> Vec<EntityRef> {
        let mut out: Vec<EntityRef> = items.to_vec();
        let mut seen: HashSet<EntityRef> = items.iter().copied().collect();
        for it in items {
            if let EntityRef::Face(f) = *it {
                for e in self.face_edges(f) {
                    if seen.insert(EntityRef::Edge(e)) {
                        out.push(EntityRef::Edge(e));
                    }
                }
            }
        }
        out
    }

    /// Everything connected to `start` through edges and faces.
    pub fn connected(&self, start: EntityRef) -> Vec<EntityRef> {
        let mut verts: Vec<VertexId> = self.vertices_of(&[start]).into_iter().collect();
        let mut seen: HashSet<VertexId> = verts.iter().copied().collect();
        let mut out: HashSet<EntityRef> = HashSet::new();
        while let Some(v) = verts.pop() {
            for &e in &self.verts[v].edges {
                out.insert(EntityRef::Edge(e));
                for &f in &self.edges[e].faces {
                    out.insert(EntityRef::Face(f));
                }
                let o = self.other_vertex(e, v);
                if seen.insert(o) {
                    verts.push(o);
                }
            }
        }
        let mut v: Vec<EntityRef> = out.into_iter().collect();
        v.sort();
        v
    }

    pub fn instance_ids(&self) -> Vec<InstanceId> {
        self.instances.keys().collect()
    }

    /// Offsets a face's outer loop by `dist` (positive = inwards) and draws the result,
    /// which splits the face.
    pub fn offset_face(&mut self, f: FaceId, dist: f64) -> Vec<EdgeId> {
        let Some(face) = self.faces.get(f) else { return Vec::new() };
        let pts = self.loop_points(&face.outer);
        let n = face.normal;
        let off = offset_polygon(&pts, n, dist);
        if off.len() < 3 {
            return Vec::new();
        }
        self.add_polyline(&off, true, false)
    }
}

fn copy_edge_attrs(src: &crate::model::Edge, dst: &mut crate::model::Edge) {
    dst.soft = src.soft;
    dst.smooth = src.smooth;
    dst.hidden = src.hidden;
    dst.layer = src.layer;
    dst.curve = src.curve;
}

/// Miter offset of a planar polygon, counter-clockwise around `n`. Positive goes inwards.
pub fn offset_polygon(pts: &[DVec3], n: DVec3, dist: f64) -> Vec<DVec3> {
    let m = pts.len();
    let mut out = Vec::with_capacity(m);
    for i in 0..m {
        let prev = pts[(i + m - 1) % m];
        let cur = pts[i];
        let next = pts[(i + 1) % m];
        let d1 = (cur - prev).normalize();
        let d2 = (next - cur).normalize();
        let in1 = n.cross(d1);
        let in2 = n.cross(d2);
        let bis = (in1 + in2).normalize_or_zero();
        let cos = bis.dot(in1);
        if bis == DVec3::ZERO || cos.abs() < 1e-6 {
            out.push(cur + in1 * dist);
        } else {
            out.push(cur + bis * (dist / cos));
        }
    }
    out
}

fn loop_keys<'a>(loops: impl Iterator<Item = &'a Vec<VertexId>>) -> HashSet<EdgeKey> {
    let mut s = HashSet::new();
    for l in loops {
        let n = l.len();
        for i in 0..n {
            s.insert(edge_key(l[i], l[(i + 1) % n]));
        }
    }
    s
}

fn region_keys(r: &Region) -> HashSet<EdgeKey> {
    loop_keys(std::iter::once(&r.outer).chain(r.holes.iter()))
}

/// SketchUp draws faces on the ground facing down; elsewhere we keep the canonical normal.
fn default_flip(plane: &Plane) -> bool {
    if plane.n.z.abs() > 1.0 - 1e-6 {
        let ground = plane.d.abs() < TOL;
        let up = plane.n.z > 0.0;
        if ground { up } else { !up }
    } else {
        false
    }
}

#[cfg(test)]
mod weld_tests {
    use super::*;

    thread_local! {
        /// Vertices `weld` looked at, as candidates to merge into.
        pub static LOOKS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }

    /// What `weld` did before its grid: every listed vertex compared with every vertex.
    fn weld_by_scan(e: &mut Entities, verts: &HashSet<VertexId>) {
        for &v in verts {
            let Some(p) = e.verts.get(v).map(|x| x.pos) else { continue };
            let target = e
                .verts
                .iter()
                .find(|(id, x)| *id != v && !verts.contains(id) && x.pos.distance(p) < TOL)
                .map(|(id, _)| id);
            if let Some(t) = target {
                e.merge_vertex(v, t);
            }
        }
    }

    /// Squares of fixed vertices; moved squares land on some of them (within the tolerance,
    /// sometimes near two at once), others land nowhere.
    fn scene() -> (Entities, HashSet<VertexId>) {
        let mut e = Entities::default();
        let mut moved = HashSet::new();
        for i in 0..30 {
            for j in 0..30 {
                let p = DVec3::new(i as f64 * 10.0, j as f64 * 10.0, 0.0);
                // Sometimes a second fixed vertex close to the first, before or after it: the
                // first one in slot order wins.
                let close = p + DVec3::new(0.012, 0.0, 0.0);
                if (i + j) % 7 == 0 {
                    e.add_vertex_raw(close);
                }
                for k in [DVec3::ZERO, DVec3::X * 3.0, DVec3::new(3.0, 3.0, 0.0), DVec3::Y * 3.0] {
                    e.add_vertex_raw(p + k);
                }
                if (i + j) % 7 == 1 {
                    e.add_vertex_raw(close);
                }
                let z = if (i * j) % 3 == 0 { 0.0 } else { 5.0 };
                let a = e.add_vertex_raw(p + DVec3::new(0.01, 0.0, z));
                let b = e.add_vertex_raw(p + DVec3::new(0.01, 7.0, z));
                e.add_edge_raw(a, b);
                moved.extend([a, b]);
            }
        }
        (e, moved)
    }

    #[test]
    fn welding_merges_what_a_full_scan_merges_and_looks_only_nearby() {
        let (mut a, moved) = scene();
        let (mut b, _) = scene();
        let n = a.verts.len() as u64;
        LOOKS.with(|l| l.set(0));
        a.weld(&moved);
        let looks = LOOKS.with(|l| l.get());
        weld_by_scan(&mut b, &moved);
        assert_eq!(serde_json::to_string(&a).unwrap(), serde_json::to_string(&b).unwrap());
        assert!(a.verts.len() < n as usize, "rien n'a été soudé");
        // Each moved vertex looks at the few vertices of its neighbouring cells, not at all.
        assert!(looks < moved.len() as u64 * 4, "{looks} sommets regardés pour {} déplacés sur {n}", moved.len());
    }

    /// Each vertex its own move, all at once: the first one given wins, and a vertex that
    /// lands on a fixed one is welded to it.
    #[test]
    fn vertices_move_each_by_their_own_transformation() {
        let mut e = Entities::default();
        let a = e.add_vertex_raw(DVec3::ZERO);
        let b = e.add_vertex_raw(DVec3::X * 9.0);
        let fixed = e.add_vertex_raw(DVec3::new(9.0, 5.0, 0.0));
        e.add_edge_raw(a, b);
        let up = DMat4::from_translation(DVec3::Y * 5.0);
        let left = DMat4::from_translation(DVec3::X * -2.0);
        e.transform_vertices_each(&[(a, left), (b, up), (a, up)]);
        assert_eq!(e.verts[a].pos, DVec3::X * -2.0);
        assert!(!e.verts.contains_key(b), "le sommet arrivé sur un autre n'a pas été soudé");
        assert_eq!(e.verts[fixed].edges.len(), 1);
    }
}
