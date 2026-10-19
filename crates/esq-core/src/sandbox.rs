//! Sandbox (terrain meshes): grids made from scratch, Delaunay triangulation of contours,
//! detail added to triangulated faces, and edge flipping. Pure geometry, no interface.
//!
//! Rewritten from the behaviour of SketchUp's Sandbox tools (From Scratch, From Contours,
//! Add Detail, Flip Edge). Lengths are in millimetres.

use std::collections::{HashMap, HashSet};

use glam::{DMat4, DVec2, DVec3};

use crate::math::{TOL, newell_normal, project_on_segment};
use crate::model::{DefId, Definition, EdgeId, Entities, FaceId, Instance, InstanceId, MaterialId, Model, VertexId};

/// Default grid spacing of the sandbox: 120 inches.
pub const DEFAULT_GRID_SPACING: f64 = 120.0 * 25.4;

/// A flip is refused when the new diagonal crosses the old edge too close to an end.
const FLIP_T_MIN: f64 = 0.05;
/// A vertex lies on the edge's line when it is nearer than 0.01 inch.
const ON_LINE: f64 = 0.01 * 25.4;

// ---------------------------------------------------------------------------------------
// Meshes

impl Entities {
    /// Adds triangles (counter-clockwise = front) on new vertices. `soft` lists index pairs
    /// of edges that are soft (hidden unless hidden geometry is shown).
    pub fn add_mesh(&mut self, pts: &[DVec3], tris: &[[usize; 3]], soft: &[(usize, usize)]) -> Vec<FaceId> {
        let ids: Vec<VertexId> = pts.iter().map(|p| self.add_vertex_raw(*p)).collect();
        let mut faces = Vec::new();
        for t in tris {
            if let Some(f) = self.add_face_raw(vec![ids[t[0]], ids[t[1]], ids[t[2]]], Vec::new()) {
                faces.push(f);
            }
        }
        for &(a, b) in soft {
            if let Some(e) = self.find_edge(ids[a], ids[b]) {
                self.edges[e].soft = true;
            }
        }
        faces
    }
}

/// A flat grid of `nx` by `ny` squares of side `spacing` in the XY plane, each cut into two
/// triangles. The diagonals (from the corner at +x to the corner at +y) are soft.
pub fn grid_entities(nx: usize, ny: usize, spacing: f64) -> Entities {
    let mut pts = Vec::with_capacity((nx + 1) * (ny + 1));
    for i in 0..=nx {
        for j in 0..=ny {
            pts.push(DVec3::new(i as f64 * spacing, j as f64 * spacing, 0.0));
        }
    }
    let at = |i: usize, j: usize| i * (ny + 1) + j;
    let mut tris = Vec::with_capacity(nx * ny * 2);
    let mut soft = Vec::with_capacity(nx * ny);
    for i in 0..nx {
        for j in 0..ny {
            tris.push([at(i, j), at(i + 1, j), at(i, j + 1)]);
            tris.push([at(i, j + 1), at(i + 1, j), at(i + 1, j + 1)]);
            soft.push((at(i + 1, j), at(i, j + 1)));
        }
    }
    let mut e = Entities::default();
    e.add_mesh(&pts, &tris, &soft);
    e
}

/// Number of whole grid squares that fit in `length`.
pub fn grid_count(length: f64, spacing: f64) -> usize {
    if spacing <= 0.0 { 0 } else { (length / spacing + 1e-9).floor() as usize }
}

/// Frame of a grid: origin and unit axes (the third is x cross y), or `None` when the two
/// directions are parallel.
pub fn grid_frame(origin: DVec3, xdir: DVec3, ydir: DVec3) -> Option<DMat4> {
    let z = xdir.cross(ydir);
    if z.length() < 1e-9 {
        return None;
    }
    Some(DMat4::from_cols(
        xdir.normalize().extend(0.0),
        ydir.normalize().extend(0.0),
        z.normalize().extend(0.0),
        origin.extend(1.0),
    ))
}

impl Model {
    /// Puts `ents` in a new group of collection `ctx`, placed with `xform`.
    pub fn add_group_with(&mut self, ctx: Option<DefId>, ents: Entities, xform: DMat4) -> InstanceId {
        let name = self.unique_def_name("Groupe");
        let def = self.defs.insert(Definition {
            name,
            is_group: true,
            description: String::new(),
            entities: ents,
            attributes: Default::default(),
            ext: Default::default(),
        });
        self.entities_mut(ctx).instances.insert(Instance {
            def,
            xform,
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        })
    }

    /// Terrain from the selected edges: their points are triangulated (Delaunay, seen from
    /// above) into a new group. Returns the group.
    pub fn terrain_from_contours(&mut self, ctx: Option<DefId>, edges: &[EdgeId]) -> Result<InstanceId, String> {
        let ents = self.entities(ctx);
        let edges: Vec<EdgeId> = edges.iter().copied().filter(|e| ents.edges.contains_key(*e)).collect();
        if edges.is_empty() {
            return Err(MSG_NO_CONTOURS.into());
        }
        let pts = contour_points(ents, &edges);
        let pts = remove_non_functional(pts, true);
        let tris = triangulate_delaunay(&pts);
        if tris.is_empty() {
            return Err("Les contours sélectionnés ne permettent pas de créer un terrain.".into());
        }
        let mut mesh = Entities::default();
        mesh.add_mesh(&pts, &tris, &[]);
        Ok(self.add_group_with(ctx, mesh, DMat4::IDENTITY))
    }
}

/// Message shown by From Contours when nothing suitable is selected.
pub const MSG_NO_CONTOURS: &str =
    "Avant d’utiliser cet outil, vous devez sélectionner les contours servant à créer le bac à sable.";

/// The points of some edges: their ends, plus break points on the edges longer than the
/// average (up to 1000 edges), so that long segments do not leave holes in the terrain.
pub fn contour_points(ents: &Entities, edges: &[EdgeId]) -> Vec<DVec3> {
    let lengths: Vec<f64> = edges
        .iter()
        .map(|&e| {
            let (a, b) = ents.edge_points(e);
            a.distance(b)
        })
        .collect();
    let avg = lengths.iter().sum::<f64>() / edges.len().max(1) as f64;
    let mut out: Vec<DVec3> = Vec::new();
    let push = |p: DVec3, out: &mut Vec<DVec3>| {
        if !out.iter().any(|q| q.distance(p) < TOL) {
            out.push(p);
        }
    };
    let subdivide = edges.len() < 1000 && avg > 0.0;
    for (&e, &len) in edges.iter().zip(&lengths) {
        let (a, b) = ents.edge_points(e);
        push(a, &mut out);
        push(b, &mut out);
        if subdivide {
            let divisions = (len / avg).floor() as usize;
            let step = 1.0 / (divisions + 1) as f64;
            for j in 1..=divisions {
                push(a + (b - a) * (j as f64 * step), &mut out);
            }
        }
    }
    out
}

/// Keeps one point per place (same x and y within the tolerance): the highest one when
/// `prefer_higher`, else the lowest.
pub fn remove_non_functional(mut pts: Vec<DVec3>, prefer_higher: bool) -> Vec<DVec3> {
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)).then(a.z.total_cmp(&b.z)));
    let mut valid = vec![true; pts.len()];
    for i in 0..pts.len() {
        for j in i + 1..pts.len() {
            if !valid[j] {
                continue;
            }
            if pts[j].x > pts[i].x + TOL {
                break;
            }
            if (pts[j].y - pts[i].y).abs() > TOL {
                continue;
            }
            let i_higher = pts[i].z > pts[j].z;
            if i_higher == prefer_higher {
                valid[j] = false;
            } else {
                valid[i] = false;
                break;
            }
        }
    }
    pts.into_iter().zip(valid).filter(|(_, v)| *v).map(|(p, _)| p).collect()
}

fn circumcircle(a: DVec2, b: DVec2, c: DVec2) -> Option<(DVec2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (a.length_squared(), b.length_squared(), c.length_squared());
    let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let center = DVec2::new(ux, uy);
    Some((center, center.distance_squared(a)))
}

/// Delaunay triangulation of points seen from above (Bowyer-Watson, sweeping in x). The
/// triangles come out counter-clockwise, so they face up. Triangles that touch the
/// enclosing super-triangle are dropped, which can leave the outline slightly concave.
pub fn triangulate_delaunay(pts: &[DVec3]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    // Work on indices sorted by x, then map back.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| pts[a].x.total_cmp(&pts[b].x).then(pts[a].y.total_cmp(&pts[b].y)));
    let mut p: Vec<DVec2> = order.iter().map(|&i| DVec2::new(pts[i].x, pts[i].y)).collect();

    let (mut lo, mut hi) = (p[0], p[0]);
    for q in &p {
        lo = lo.min(*q);
        hi = hi.max(*q);
    }
    let dmax = (hi.x - lo.x).max(hi.y - lo.y);
    if dmax <= 0.0 {
        return Vec::new();
    }
    let mid = (lo + hi) * 0.5;
    p.push(DVec2::new(mid.x - 2.0 * dmax, mid.y - dmax));
    p.push(DVec2::new(mid.x, mid.y + 2.0 * dmax));
    p.push(DVec2::new(mid.x + 2.0 * dmax, mid.y - dmax));

    struct Tri {
        v: [usize; 3],
        circle: Option<(DVec2, f64)>,
    }
    let make = |v: [usize; 3], p: &[DVec2]| Tri { v, circle: circumcircle(p[v[0]], p[v[1]], p[v[2]]) };
    let mut active = vec![make([n, n + 1, n + 2], &p)];
    let mut done: Vec<[usize; 3]> = Vec::new();

    for i in 0..n {
        let q = p[i];
        let mut edges: Vec<(usize, usize)> = Vec::new();
        let mut keep = Vec::with_capacity(active.len());
        for t in active {
            match t.circle {
                // A degenerate triangle never contains anything: forget it.
                None => {}
                Some((c, r2)) => {
                    // Points come sorted in x: once the sweep is past the circle, the
                    // triangle is final.
                    if c.x + r2.sqrt() < q.x {
                        done.push(t.v);
                    } else if c.distance_squared(q) <= r2 {
                        edges.push((t.v[0], t.v[1]));
                        edges.push((t.v[1], t.v[2]));
                        edges.push((t.v[2], t.v[0]));
                    } else {
                        keep.push(t);
                    }
                }
            }
        }
        active = keep;
        // An edge shared by two removed triangles is inside the hole: drop both copies.
        let mut counts: HashMap<(usize, usize), usize> = HashMap::new();
        for &(a, b) in &edges {
            *counts.entry((a.min(b), a.max(b))).or_default() += 1;
        }
        for (a, b) in edges {
            if counts[&(a.min(b), a.max(b))] == 1 {
                active.push(make([a, b, i], &p));
            }
        }
    }
    done.extend(active.into_iter().map(|t| t.v));

    done.into_iter()
        .filter(|t| t.iter().all(|&v| v < n))
        .filter_map(|t| {
            let (a, b, c) = (p[t[0]], p[t[1]], p[t[2]]);
            let area = (b - a).perp_dot(c - a);
            if area.abs() < 1e-12 {
                return None;
            }
            let t = if area > 0.0 { t } else { [t[0], t[2], t[1]] };
            Some([order[t[0]], order[t[1]], order[t[2]]])
        })
        .collect()
}

// ---------------------------------------------------------------------------------------
// Add Detail

/// What the Add Detail tool was pointed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailTarget {
    Vertex(VertexId),
    Edge(EdgeId),
    Face(FaceId),
}

struct FaceAttrs {
    front: Option<MaterialId>,
    back: Option<MaterialId>,
    layer: usize,
    hidden: bool,
}

impl Entities {
    fn is_triangle(&self, f: FaceId) -> bool {
        self.faces.get(f).is_some_and(|x| x.outer.len() == 3 && x.holes.is_empty())
    }

    fn attrs_of(&self, f: FaceId) -> FaceAttrs {
        let x = &self.faces[f];
        FaceAttrs { front: x.front, back: x.back, layer: x.layer, hidden: x.hidden }
    }

    fn add_triangle_like(&mut self, tri: [VertexId; 3], a: &FaceAttrs) -> Option<FaceId> {
        let f = self.add_face_raw(tri.to_vec(), Vec::new())?;
        let x = &mut self.faces[f];
        (x.front, x.back, x.layer, x.hidden) = (a.front, a.back, a.layer, a.hidden);
        Some(f)
    }

    /// The faces a Detail click concerns: the face itself, the faces on an edge, or all the
    /// faces around a vertex.
    pub fn detail_faces(&self, t: DetailTarget) -> Vec<FaceId> {
        match t {
            DetailTarget::Face(f) => vec![f],
            DetailTarget::Edge(e) => self.edges.get(e).map(|x| x.faces.clone()).unwrap_or_default(),
            DetailTarget::Vertex(v) => {
                let mut out: Vec<FaceId> = Vec::new();
                if let Some(vx) = self.verts.get(v) {
                    for &e in &vx.edges {
                        for &f in &self.edges[e].faces {
                            if !out.contains(&f) {
                                out.push(f);
                            }
                        }
                    }
                }
                out
            }
        }
    }

    /// Whether the Add Detail tool can work on this target: every face concerned must be a
    /// triangle, and there must be at least one.
    pub fn detail_possible(&self, t: DetailTarget) -> bool {
        let faces = self.detail_faces(t);
        !faces.is_empty() && faces.iter().all(|&f| self.is_triangle(f))
    }

    /// Average of the normals of the faces concerned, as a unit vector.
    pub fn detail_normal(&self, t: DetailTarget) -> DVec3 {
        let faces = self.detail_faces(t);
        let sum: DVec3 = faces.iter().map(|&f| self.faces[f].normal.normalize_or_zero()).sum();
        sum.normalize_or(DVec3::Z)
    }

    /// Adds a vertex at `pos` and joins it to the corners of the triangles around: a click
    /// on a face cuts it in three, on an edge cuts each face on it in two. A click on a
    /// vertex adds nothing and returns that vertex. Returns the vertex to offset.
    pub fn add_detail_vertex(&mut self, t: DetailTarget, pos: DVec3) -> Result<VertexId, String> {
        if !self.detail_possible(t) {
            return Err(MSG_DETAIL_INVALID.into());
        }
        match t {
            DetailTarget::Vertex(v) => Ok(v),
            DetailTarget::Face(f) => {
                let attrs = self.attrs_of(f);
                let [a, b, c] = [self.faces[f].outer[0], self.faces[f].outer[1], self.faces[f].outer[2]];
                let plane = self.face_plane(f);
                let pos = pos - plane.n * plane.distance(pos);
                // The click lies on a corner or an edge: treat it as that.
                for (x, y) in [(a, b), (b, c), (c, a)] {
                    let (s, d) = project_on_segment(pos, self.pos(x), self.pos(y));
                    if d < TOL {
                        let Some(e) = self.find_edge(x, y) else { continue };
                        if s <= 0.0 {
                            return self.add_detail_vertex(DetailTarget::Vertex(x), pos);
                        }
                        if s >= 1.0 {
                            return self.add_detail_vertex(DetailTarget::Vertex(y), pos);
                        }
                        return self.add_detail_vertex(DetailTarget::Edge(e), pos);
                    }
                }
                self.remove_face(f);
                let v = self.add_vertex_raw(pos);
                for tri in [[a, b, v], [b, c, v], [c, a, v]] {
                    self.add_triangle_like(tri, &attrs);
                }
                Ok(v)
            }
            DetailTarget::Edge(e) => {
                let [a, b] = self.edges[e].v;
                // A click that is not exactly on the edge is taken as its closest point.
                let (s, _) = project_on_segment(pos, self.pos(a), self.pos(b));
                if s * self.pos(a).distance(self.pos(b)) < TOL {
                    return Ok(a);
                }
                if (1.0 - s) * self.pos(a).distance(self.pos(b)) < TOL {
                    return Ok(b);
                }
                let at = self.pos(a).lerp(self.pos(b), s);
                // Faces on the edge, as (attributes, directed edge, opposite corner).
                let mut parts = Vec::new();
                for f in self.edges[e].faces.clone() {
                    let l = self.faces[f].outer.clone();
                    let i = (0..3).find(|&i| {
                        let (x, y) = (l[i], l[(i + 1) % 3]);
                        (x == a && y == b) || (x == b && y == a)
                    });
                    let Some(i) = i else { continue };
                    parts.push((self.attrs_of(f), l[i], l[(i + 1) % 3], l[(i + 2) % 3]));
                    self.remove_face(f);
                }
                let v = self.split_edge(e, at);
                for (attrs, x, y, opp) in parts {
                    self.add_triangle_like([x, v, opp], &attrs);
                    self.add_triangle_like([v, y, opp], &attrs);
                }
                Ok(v)
            }
        }
    }

    /// Cuts every selected triangle into four (at the midpoints of its edges). The
    /// neighbours that are not selected are cut along the new points so that nothing is
    /// left cracked. Returns how many triangles were subdivided.
    pub fn subdivide_triangles(&mut self, selected: &[FaceId]) -> usize {
        let sel: HashSet<FaceId> = selected.iter().copied().filter(|&f| self.is_triangle(f)).collect();
        if sel.is_empty() {
            return 0;
        }
        let mut sel_order: Vec<FaceId> = selected.iter().copied().filter(|f| sel.contains(f)).collect();
        sel_order.dedup();
        // Edges to split, in a stable order.
        let mut split: Vec<EdgeId> = Vec::new();
        for &f in &sel_order {
            for e in self.face_edges(f) {
                if !split.contains(&e) {
                    split.push(e);
                }
            }
        }
        let split_set: HashSet<EdgeId> = split.iter().copied().collect();
        // Unselected neighbours of those edges.
        let mut neighbours: Vec<FaceId> = Vec::new();
        for &e in &split {
            for &f in &self.edges[e].faces {
                if !sel.contains(&f) && !neighbours.contains(&f) {
                    neighbours.push(f);
                }
            }
        }
        // A triangle with three split edges is subdivided like a selected one.
        let mut quads: Vec<FaceId> = sel_order.clone();
        let mut fans: Vec<FaceId> = Vec::new();
        for &f in &neighbours {
            if !self.is_triangle(f) {
                continue;
            }
            let k = self.face_edges(f).iter().filter(|e| split_set.contains(e)).count();
            if k >= 3 {
                quads.push(f);
            } else {
                fans.push(f);
            }
        }
        // Midpoint of each split edge is created before the faces are replaced.
        let mut jobs: Vec<([VertexId; 3], FaceAttrs, bool)> = Vec::new();
        for &f in quads.iter().chain(fans.iter()) {
            let l = &self.faces[f].outer;
            jobs.push(([l[0], l[1], l[2]], self.attrs_of(f), quads.contains(&f)));
        }
        let all_removed: Vec<FaceId> = quads.iter().chain(fans.iter()).copied().collect();
        for f in all_removed {
            self.remove_face(f);
        }
        let mut mids: HashMap<(VertexId, VertexId), VertexId> = HashMap::new();
        for &e in &split {
            let [a, b] = self.edges[e].v;
            let at = (self.pos(a) + self.pos(b)) * 0.5;
            let m = self.split_edge(e, at);
            mids.insert((a.min(b), a.max(b)), m);
            for h in [self.find_edge(a, m), self.find_edge(m, b)].into_iter().flatten() {
                self.edges[h].soft = false;
            }
        }
        let mid = |a: VertexId, b: VertexId| mids.get(&(a.min(b), a.max(b))).copied();
        for (v, attrs, four) in jobs {
            let [a, b, c] = v;
            if four {
                let (Some(ab), Some(bc), Some(ca)) = (mid(a, b), mid(b, c), mid(c, a)) else { continue };
                for tri in [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]] {
                    self.add_triangle_like(tri, &attrs);
                }
            } else {
                // Polygon made of the corners and the midpoints, fanned from the first midpoint.
                let mut poly: Vec<VertexId> = Vec::new();
                for (x, y) in [(a, b), (b, c), (c, a)] {
                    poly.push(x);
                    if let Some(m) = mid(x, y) {
                        poly.push(m);
                    }
                }
                let Some(start) = poly.iter().position(|p| mids.values().any(|m| m == p)) else { continue };
                poly.rotate_left(start);
                for i in 1..poly.len() - 1 {
                    self.add_triangle_like([poly[0], poly[i], poly[i + 1]], &attrs);
                }
            }
        }
        sel_order.len()
    }
}

/// Message of Add Detail when the target is not made of triangles.
pub const MSG_DETAIL_INVALID: &str = "Pour utiliser cet outil, vous devez sélectionner une face triangulaire, une arête entre deux triangles ou un sommet relié à des triangles.";

// ---------------------------------------------------------------------------------------
// Flip Edge

/// Why an edge cannot be flipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlipError {
    NotTriangles,
    Invalid,
}

impl FlipError {
    pub fn message(self) -> &'static str {
        match self {
            FlipError::NotTriangles => {
                "Impossible de retourner cette arête. Les faces de chaque côté de l’arête doivent être des triangles."
            }
            FlipError::Invalid => {
                "Impossible de retourner cette arête. Le retournement créerait une géométrie non valide."
            }
        }
    }
}

impl Entities {
    /// The two faces on each side of an edge and their corners opposite to it.
    fn flip_geometry(&self, e: EdgeId) -> Result<([FaceId; 2], [VertexId; 2]), FlipError> {
        let edge = self.edges.get(e).ok_or(FlipError::NotTriangles)?;
        if edge.faces.len() != 2 || !edge.faces.iter().all(|&f| self.is_triangle(f)) {
            return Err(FlipError::NotTriangles);
        }
        let (p, q) = self.edge_points(e);
        let dir = q - p;
        let mut opp = [None, None];
        for (k, &f) in edge.faces.iter().enumerate() {
            for &v in &self.faces[f].outer {
                let pos = self.pos(v);
                let off = pos - p;
                let dist = (off - dir * (off.dot(dir) / dir.length_squared())).length();
                if dist > ON_LINE {
                    opp[k] = Some(v);
                }
            }
        }
        let (Some(o0), Some(o1)) = (opp[0], opp[1]) else { return Err(FlipError::Invalid) };
        if o0 == o1 {
            return Err(FlipError::Invalid);
        }
        // Where the new diagonal crosses the edge's line.
        let (r, s) = (self.pos(o0), self.pos(o1) - self.pos(o0));
        let w = p - r;
        let (a, b, c) = (dir.dot(dir), dir.dot(s), s.dot(s));
        let (d, f) = (dir.dot(w), s.dot(w));
        let den = a * c - b * b;
        if den.abs() < 1e-12 {
            return Err(FlipError::Invalid);
        }
        let t = (b * f - c * d) / den;
        if t > FLIP_T_MIN && t < 1.0 - FLIP_T_MIN {
            Ok(([edge.faces[0], edge.faces[1]], [o0, o1]))
        } else {
            Err(FlipError::Invalid)
        }
    }

    /// Whether the edge can be flipped.
    pub fn can_flip_edge(&self, e: EdgeId) -> Result<(), FlipError> {
        self.flip_geometry(e).map(|_| ())
    }

    /// Replaces the edge between two triangles by the other diagonal of their quad, keeping
    /// the edge's flags and the first face's attributes.
    pub fn flip_edge(&mut self, e: EdgeId) -> Result<(), FlipError> {
        let ([f0, f1], [o0, o1]) = self.flip_geometry(e)?;
        let [a, b] = self.edges[e].v;
        let old = self.edges[e].clone();
        let attrs = self.attrs_of(f0);
        let (n0, n1) = (self.faces[f0].normal, self.faces[f1].normal);
        self.remove_face(f0);
        self.remove_face(f1);
        self.remove_edge_raw(e);
        for (end, n) in [(b, n0), (a, n1)] {
            let mut tri = [o0, o1, end];
            let pts: Vec<DVec3> = tri.iter().map(|&v| self.pos(v)).collect();
            if newell_normal(&pts).dot(n) < 0.0 {
                tri = [o1, o0, end];
            }
            self.add_triangle_like(tri, &attrs);
        }
        if let Some(d) = self.find_edge(o0, o1) {
            let x = &mut self.edges[d];
            (x.soft, x.smooth, x.hidden, x.layer) = (old.soft, old.smooth, old.hidden, old.layer);
        }
        Ok(())
    }
}
