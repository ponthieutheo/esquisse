//! Stamp: presses the footprint of an object into a mesh. The mesh is cut along an outline
//! offset around the footprint; the ring between the footprint and the cut is filled with
//! triangles (Delaunay) that slope from the terrain up or down to the footprint, which can
//! then be moved vertically.

use std::collections::{HashMap, HashSet};

use glam::{DMat4, DVec2, DVec3};

use super::delaunay::triangulate;
use super::drape::{Surface, drape_segment};
use super::{mesh_items, plan, point_in_polygon, refresh_normals};
use crate::math::TOL;
use crate::model::{Entities, EntityRef, FaceId, MaterialId, VertexId};

const INCH: f64 = 25.4;

/// Tag of a point of the new mesh: it belongs to the footprint, or to the cut outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    Building,
    Offset,
}

/// Looks vertices up by position (within the tolerance).
struct PointIndex {
    cell: f64,
    map: HashMap<(i64, i64, i64), Vec<(VertexId, DVec3)>>,
}

impl PointIndex {
    fn new() -> Self {
        PointIndex { cell: TOL * 4.0, map: HashMap::new() }
    }

    fn key(&self, p: DVec3) -> (i64, i64, i64) {
        ((p.x / self.cell).floor() as i64, (p.y / self.cell).floor() as i64, (p.z / self.cell).floor() as i64)
    }

    fn from_entities(ents: &Entities) -> Self {
        let mut s = Self::new();
        for (id, v) in ents.verts.iter() {
            s.insert(id, v.pos);
        }
        s
    }

    fn insert(&mut self, id: VertexId, p: DVec3) {
        let k = self.key(p);
        self.map.entry(k).or_default().push((id, p));
    }

    fn find(&self, p: DVec3) -> Option<VertexId> {
        let (cx, cy, cz) = self.key(p);
        let mut best: Option<(f64, VertexId)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(list) = self.map.get(&(cx + dx, cy + dy, cz + dz)) else { continue };
                    for &(id, q) in list {
                        let d = q.distance(p);
                        if d < TOL && best.is_none_or(|b| d < b.0) {
                            best = Some((d, id));
                        }
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// The vertex at `p`, created when there is none.
    fn vertex(&mut self, ents: &mut Entities, p: DVec3) -> VertexId {
        if let Some(v) = self.find(p) {
            return v;
        }
        let v = ents.add_vertex_raw(p);
        self.insert(v, p);
        v
    }
}

// ---------------------------------------------------------------------------------------
// Footprint (FootprintFinder)

/// The underside of what is stamped, in world coordinates.
#[derive(Clone, Debug, Default)]
pub struct Footprint {
    /// Outer loops of the footprint faces.
    pub faces: Vec<Vec<DVec3>>,
    /// Outlines of the footprint (closed loops, starting on a corner), with their real heights…
    pub profile: Vec<Vec<DVec3>>,
    /// …and flattened at the height of the lowest point.
    pub flat_profile: Vec<Vec<DVec3>>,
    /// Normal of each flat outline, from its own winding.
    pub normals: Vec<DVec3>,
}

fn reversed_in(ents: &Entities, e: crate::model::EdgeId, f: FaceId) -> bool {
    let [a, b] = ents.edges[e].v;
    for l in ents.faces[f].loops() {
        for i in 0..l.len() {
            let (x, y) = (l[i], l[(i + 1) % l.len()]);
            if x == a && y == b {
                return false;
            }
            if x == b && y == a {
                return true;
            }
        }
    }
    false
}

/// Normal of a polygon from the fan of triangles around its first point.
fn poly_normal(p: &[DVec3]) -> DVec3 {
    let mut n = DVec3::ZERO;
    for i in 2..p.len() {
        n += (p[i - 1] - p[0]).cross(p[i] - p[0]);
    }
    n.normalize_or_zero()
}

impl Footprint {
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// Finds the footprint of the faces given in world coordinates: for every connected
    /// piece of geometry that is not under another one, the non-vertical faces at its lowest
    /// level and the faces that follow its underside from there.
    pub fn find(world_faces: &[Vec<DVec3>]) -> Footprint {
        // All the faces welded into one collection.
        let mut tmp = Entities::default();
        let mut idx = PointIndex::new();
        for f in world_faces {
            let mut lp: Vec<VertexId> = Vec::new();
            for p in f {
                let v = idx.vertex(&mut tmp, *p);
                if lp.last() != Some(&v) {
                    lp.push(v);
                }
            }
            while lp.len() > 1 && lp.first() == lp.last() {
                lp.pop();
            }
            if lp.len() >= 3 {
                tmp.add_face_raw(lp, Vec::new());
            }
        }
        // Connected pieces, and their boxes.
        let mut seen: HashSet<FaceId> = HashSet::new();
        let mut groups: Vec<(Vec<FaceId>, DVec3, DVec3)> = Vec::new();
        let mut ids: Vec<FaceId> = tmp.faces.keys().collect();
        ids.sort();
        for f in ids {
            if seen.contains(&f) {
                continue;
            }
            let faces: Vec<FaceId> = tmp
                .connected(EntityRef::Face(f))
                .into_iter()
                .filter_map(|e| if let EntityRef::Face(x) = e { Some(x) } else { None })
                .collect();
            seen.extend(faces.iter().copied());
            let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
            for &x in &faces {
                for v in tmp.faces[x].all_vertices() {
                    lo = lo.min(tmp.pos(v));
                    hi = hi.max(tmp.pos(v));
                }
            }
            groups.push((faces, lo, hi));
        }
        // A piece above another one (they overlap in plan) is not a footprint.
        let mut valid = vec![true; groups.len()];
        for i in 0..groups.len() {
            for j in (i + 1)..groups.len() {
                let (a, b) = (&groups[i], &groups[j]);
                let apart =
                    a.2.x < b.1.x - 0.01 || a.2.y < b.1.y - 0.01 || a.1.x > b.2.x + 0.01 || a.1.y > b.2.y + 0.01;
                if apart {
                    continue;
                }
                if a.1.z < b.1.z - 0.01 {
                    valid[j] = false;
                } else if a.1.z > b.1.z + 0.01 {
                    valid[i] = false;
                    break;
                }
            }
        }
        let mut out = Footprint::default();
        for (g, (faces, ..)) in groups.iter().enumerate() {
            if valid[g] {
                Self::group_footprint(&tmp, faces, &mut out);
            }
        }
        out
    }

    fn group_footprint(tmp: &Entities, faces: &[FaceId], out: &mut Footprint) {
        // The lowest vertices, and the non-vertical faces that touch them.
        let verts: HashSet<VertexId> = faces.iter().flat_map(|f| tmp.faces[*f].all_vertices()).collect();
        let low_z = verts.iter().map(|v| tmp.pos(*v).z).fold(f64::INFINITY, f64::min);
        let mut low: Vec<FaceId> = Vec::new();
        for &v in &verts {
            if (tmp.pos(v).z - low_z).abs() < TOL {
                for f in super::faces_of_vertex(tmp, v) {
                    if tmp.faces[f].normal.z.abs() >= 0.001 && !low.contains(&f) {
                        low.push(f);
                    }
                }
            }
        }
        low.sort();
        // From there, follow the underside: across each edge, the neighbour that looks the
        // most downwards (once the faces are oriented alike) is kept, the others are not.
        let mut arr = low;
        let mut included = vec![true; arr.len()];
        let mut i = 0;
        while i < arr.len() {
            if included[i] {
                let f = arr[i];
                let down = tmp.faces[f].normal.z < 0.0;
                let loops: Vec<Vec<VertexId>> = tmp.faces[f].loops().cloned().collect();
                for l in loops {
                    for k in 0..l.len() {
                        let Some(e) = tmp.find_edge(l[k], l[(k + 1) % l.len()]) else { continue };
                        let others: Vec<FaceId> = tmp.edges[e].faces.iter().copied().filter(|x| *x != f).collect();
                        let rev1 = reversed_in(tmp, e, f);
                        let mut best = (0.001, None);
                        for (j, &o) in others.iter().enumerate() {
                            let rev2 = reversed_in(tmp, e, o);
                            let mut n = tmp.faces[o].normal;
                            if (!down && rev1 != rev2) || (down && rev1 == rev2) {
                                n = -n;
                            }
                            let dot = -n.z;
                            if dot > best.0 {
                                best = (dot, Some(j));
                            }
                        }
                        for (j, &o) in others.iter().enumerate() {
                            let keep = best.1 == Some(j);
                            match arr.iter().position(|x| *x == o) {
                                Some(p) => {
                                    if included[p] {
                                        included[p] = keep;
                                    }
                                }
                                None => {
                                    arr.push(o);
                                    included.push(keep);
                                }
                            }
                        }
                    }
                }
            }
            i += 1;
        }
        let group: Vec<FaceId> = arr.iter().zip(&included).filter(|(_, k)| **k).map(|(f, _)| *f).collect();
        for &f in &group {
            out.faces.push(tmp.loop_points(&tmp.faces[f].outer));
        }
        Self::profile_loops(tmp, &group, low_z, out);
    }

    /// The outline of the footprint faces: edges that only one of them uses, chained into
    /// closed loops.
    fn profile_loops(tmp: &Entities, group: &[FaceId], low_z: f64, out: &mut Footprint) {
        let set: HashSet<FaceId> = group.iter().copied().collect();
        let mut edges: Vec<[VertexId; 2]> = Vec::new();
        for &f in group {
            let outer = &tmp.faces[f].outer;
            for k in 0..outer.len() {
                let Some(e) = tmp.find_edge(outer[k], outer[(k + 1) % outer.len()]) else { continue };
                if !tmp.edges[e].faces.iter().any(|x| *x != f && set.contains(x)) {
                    edges.push(tmp.edges[e].v);
                }
            }
        }
        let mut used = vec![false; edges.len()];
        let mut unused = edges.len();
        let mut loops: Vec<Vec<VertexId>> = Vec::new();
        while unused > 2 {
            let Some(first) = used.iter().position(|u| !u) else { break };
            used[first] = true;
            unused -= 1;
            let start = edges[first][0];
            let mut seek = edges[first][1];
            let mut lp = vec![start];
            let mut closed = false;
            while !closed {
                if seek == start {
                    closed = true;
                    break;
                }
                let next = (0..edges.len()).find(|&j| !used[j] && (edges[j][0] == seek || edges[j][1] == seek));
                let Some(j) = next else { break };
                used[j] = true;
                unused -= 1;
                lp.push(seek);
                seek = if edges[j][0] == seek { edges[j][1] } else { edges[j][0] };
            }
            if closed && lp.len() >= 3 {
                loops.push(lp);
            }
        }
        for lp in loops {
            let real: Vec<DVec3> = lp.iter().map(|v| tmp.pos(*v)).collect();
            let flat: Vec<DVec3> = real.iter().map(|p| DVec3::new(p.x, p.y, low_z)).collect();
            let normal = poly_normal(&flat);
            if normal == DVec3::ZERO {
                continue;
            }
            // Start on a corner.
            let n = flat.len();
            let mut dir1 = (flat[0] - flat[n - 1]).normalize_or_zero();
            let mut corner = 0;
            for i in 0..n {
                let dir0 = dir1;
                dir1 = (flat[(i + 1) % n] - flat[i]).normalize_or_zero();
                if dir0.cross(dir1).length() > 0.001 {
                    corner = i;
                    break;
                }
            }
            let mut real = real;
            let mut flat = flat;
            real.rotate_left(corner);
            flat.rotate_left(corner);
            out.profile.push(real);
            out.flat_profile.push(flat);
            out.normals.push(normal);
        }
    }

    /// The same footprint in another frame (a rigid transformation that keeps the vertical).
    pub fn transformed(&self, m: &DMat4) -> Footprint {
        let tp = |l: &Vec<DVec3>| l.iter().map(|p| m.transform_point3(*p)).collect::<Vec<_>>();
        Footprint {
            faces: self.faces.iter().map(tp).collect(),
            profile: self.profile.iter().map(tp).collect(),
            flat_profile: self.flat_profile.iter().map(tp).collect(),
            normals: self.normals.iter().map(|n| m.transform_vector3(*n).normalize_or_zero()).collect(),
        }
    }
}

/// The outline of a footprint pushed outwards by `dist`, in plan.
pub fn offset_outline(flat: &[DVec3], normal: DVec3, dist: f64) -> Vec<DVec2> {
    let n = flat.len();
    let off = |a: DVec3, b: DVec3| plan((b - a).cross(normal).normalize_or_zero() * dist);
    let line = |i: usize, j: usize| {
        let o = off(flat[i], flat[j]);
        (plan(flat[i]) + o, plan(flat[j]) + o)
    };
    let mut last = line(n - 1, 0);
    let mut last_dir = last.1 - last.0;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let next = line(i, (i + 1) % n);
        let next_dir = next.1 - next.0;
        let sin = next_dir.perp_dot(last_dir).abs() / (next_dir.length() * last_dir.length()).max(1e-300);
        if sin > 0.01 {
            // Intersection of the two offset lines.
            let s = (next.0 - last.0).perp_dot(next_dir) / last_dir.perp_dot(next_dir);
            out.push(last.0 + last_dir * s);
        } else {
            out.push(next.0);
        }
        last = next;
        last_dir = next_dir;
    }
    out
}

// ---------------------------------------------------------------------------------------
// Stamp

/// Which mesh a stamp goes into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainSel {
    /// Everything in the collection.
    All,
    /// What is connected to a face or an edge.
    Connected(EntityRef),
}

/// What is left to move once the mesh is cut and filled.
#[derive(Clone, Debug)]
pub struct StampState {
    /// Vertices of the footprint; moving them vertically sets the elevation of the stamp.
    pub building: Vec<VertexId>,
    /// How far the footprint was brought down: its underside ends one inch under the
    /// highest point of the mesh.
    pub level: f64,
}

/// Keeps one point per position in plan; of two points at the same place the higher stays.
fn remove_non_functional(mut rows: Vec<(DVec3, Tag)>) -> Vec<(DVec3, Tag)> {
    rows.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.0.y.total_cmp(&b.0.y)).then(a.0.z.total_cmp(&b.0.z)));
    let mut valid = vec![true; rows.len()];
    for i in 0..rows.len() {
        for j in (i + 1)..rows.len() {
            if !valid[j] {
                continue;
            }
            if rows[j].0.x > rows[i].0.x + TOL {
                break;
            }
            if (rows[j].0.y - rows[i].0.y).abs() > TOL {
                continue;
            }
            if rows[i].0.z > rows[j].0.z {
                valid[j] = false;
            } else {
                valid[i] = false;
                break;
            }
        }
    }
    rows.into_iter().zip(valid).filter(|(_, v)| *v).map(|(r, _)| r).collect()
}

fn close_to_any(list: &[(DVec3, Tag)], p: DVec3) -> bool {
    list.iter().any(|(q, _)| q.distance(p) < TOL)
}

fn is_convex(pts: &[DVec3], normal: DVec3) -> bool {
    let n = pts.len();
    (0..n).all(|i| {
        let (a, b, c) = (pts[i], pts[(i + 1) % n], pts[(i + 2) % n]);
        (b - a).cross(c - b).dot(normal) >= -1e-9
    })
}

fn seed_point(ents: &Entities, s: EntityRef) -> Option<DVec3> {
    match s {
        EntityRef::Edge(e) => Some(ents.edge_points(e).0),
        EntityRef::Face(f) => Some(ents.pos(*ents.faces.get(f)?.outer.first()?)),
        _ => None,
    }
}

/// Cuts the mesh around the footprint and fills the gap, with the footprint one inch under
/// the highest point of the mesh. Move the result with `shift_building`.
pub fn prepare(ents: &mut Entities, sel: TerrainSel, fp: &Footprint, offset: f64) -> Result<StampState, String> {
    if fp.is_empty() || fp.flat_profile.is_empty() {
        return Err("Sélection incorrecte. Il n'y a aucune face non verticale à tamponner.".into());
    }
    let offset = if offset <= 0.0 { INCH } else { offset };
    let seed = match sel {
        TerrainSel::Connected(s) => Some(s),
        TerrainSel::All => None,
    };
    let seed_pos = seed.and_then(|s| seed_point(ents, s));
    let (faces0, edges0) = mesh_items(ents, seed);
    if faces0.is_empty() {
        return Err("Il n'y a pas de maille à tamponner.".into());
    }
    let mut max_z = f64::NEG_INFINITY;
    for &e in &edges0 {
        let (a, b) = ents.edge_points(e);
        max_z = max_z.max(a.z).max(b.z);
    }
    let up = faces0.iter().map(|f| ents.faces[*f].normal.z).sum::<f64>() >= 0.0;
    let material: (Option<MaterialId>, Option<MaterialId>) = faces0
        .iter()
        .find_map(|f| {
            let face = &ents.faces[*f];
            (face.front.is_some() || face.back.is_some()).then_some((face.front, face.back))
        })
        .unwrap_or((None, None));

    let polys: Vec<Vec<DVec2>> =
        (0..fp.flat_profile.len()).map(|i| offset_outline(&fp.flat_profile[i], fp.normals[i], offset)).collect();
    let level = fp.flat_profile[0][0].z - max_z + INCH;

    // A vertical wall over each side of the outline cuts the mesh: its points are the
    // places where the new triangles will meet the old mesh.
    let surface = Surface::from_faces(ents, &faces0);
    let mut offset_pts: Vec<DVec3> = Vec::new();
    for poly in &polys {
        for j in 0..poly.len() {
            let (a, b) = (poly[j], poly[(j + 1) % poly.len()]);
            if a.distance(b) < TOL {
                continue;
            }
            let (pts, _) = drape_segment(ents, &surface, a.extend(0.0), b.extend(0.0));
            for p in pts {
                if !offset_pts.iter().any(|q| q.distance(p) < TOL) {
                    offset_pts.push(p);
                }
            }
        }
    }

    // The mesh as it is now, cut.
    let seed_now = seed_pos.and_then(|p| ents.find_vertex(p)).and_then(|v| ents.verts[v].edges.first().copied());
    let (faces1, edges1) = match (seed, seed_now) {
        (Some(_), Some(e)) => mesh_items(ents, Some(EntityRef::Edge(e))),
        (Some(s), None) => mesh_items(ents, Some(s)),
        (None, _) => mesh_items(ents, None),
    };
    let visible = edges1.iter().filter(|e| !ents.edges[**e].hidden && !ents.edges[**e].soft).count();
    let mostly_visible = !edges1.is_empty() && visible as f64 / edges1.len() as f64 * 100.0 > 50.0;

    // Whatever is inside the outline goes: edges by their middle, faces by their centre.
    let inside = |p: DVec3| polys.iter().any(|poly| point_in_polygon(poly, plan(p), false));
    let mut rem_edges = Vec::new();
    let mut rem_faces: HashSet<FaceId> = HashSet::new();
    for &e in &edges1 {
        let (a, b) = ents.edge_points(e);
        if inside((a + b) * 0.5) {
            rem_edges.push(e);
            rem_faces.extend(ents.edges[e].faces.iter().copied());
        }
    }
    for &f in &faces1 {
        let face = &ents.faces[f];
        if rem_faces.contains(&f) || !face.holes.is_empty() {
            continue;
        }
        let pts = ents.loop_points(&face.outer);
        if is_convex(&pts, face.normal) && inside(pts.iter().copied().sum::<DVec3>() / pts.len() as f64) {
            rem_faces.insert(f);
        }
    }
    let mut touched: HashSet<VertexId> = HashSet::new();
    for f in rem_faces {
        if ents.faces.contains_key(f) {
            touched.extend(ents.faces[f].all_vertices());
            ents.remove_face(f);
        }
    }
    for e in rem_edges {
        if let Some(edge) = ents.edges.get(e) {
            touched.extend(edge.v);
            ents.remove_edge_raw(e);
        }
    }
    for v in touched {
        if ents.verts.get(v).is_some_and(|x| x.edges.is_empty()) {
            ents.verts.remove(v);
        }
    }

    // New points: the outline of the footprint, the places on it facing the cut, the cut.
    let interior: Vec<bool> = offset_pts.iter().map(|p| inside(*p)).collect();
    let mut pts: Vec<(DVec3, Tag)> = Vec::new();
    for l in &fp.profile {
        for p in l {
            let q = *p - DVec3::Z * level;
            if !close_to_any(&pts, q) {
                pts.push((q, Tag::Building));
            }
        }
    }
    for (q, _) in offset_pts.iter().zip(&interior).filter(|(_, i)| !**i) {
        let q2 = plan(*q);
        let mut best: Option<(f64, DVec3)> = None;
        for l in &fp.profile {
            for k in 0..l.len() {
                let (a, b) = (l[k], l[(k + 1) % l.len()]);
                let dir = b - a;
                let d2 = plan(dir);
                let len2 = d2.length_squared();
                if len2 == 0.0 {
                    continue;
                }
                let t = (q2 - plan(a)).dot(d2) / len2;
                if t > 0.01 && t < 0.99 {
                    let dist = (plan(a) + d2 * t - q2).length();
                    if best.is_none_or(|b| dist < b.0) {
                        best = Some((dist, a + dir * t - DVec3::Z * level));
                    }
                }
            }
        }
        if let Some((_, c)) = best
            && !close_to_any(&pts, c)
        {
            pts.push((c, Tag::Building));
        }
    }
    for (q, _) in offset_pts.iter().zip(&interior).filter(|(_, i)| !**i) {
        pts.push((*q, Tag::Offset));
    }
    let rows = remove_non_functional(pts);

    // Vertices (the ones of the mesh are reused), and the footprint faces.
    let mut index = PointIndex::from_entities(ents);
    let row_v: Vec<VertexId> = rows.iter().map(|(p, _)| index.vertex(ents, *p)).collect();
    let building_rows: Vec<(DVec3, VertexId)> =
        rows.iter().zip(&row_v).filter(|((_, t), _)| *t == Tag::Building).map(|((p, _), v)| (*p, *v)).collect();
    let mut building: HashSet<VertexId> = building_rows.iter().map(|(_, v)| *v).collect();
    let mut stamped: Vec<FaceId> = Vec::new();
    for face in &fp.faces {
        let mut lp: Vec<VertexId> = Vec::new();
        for k in 0..face.len() {
            let (a, b) = (face[k] - DVec3::Z * level, face[(k + 1) % face.len()] - DVec3::Z * level);
            lp.push(index.vertex(ents, a));
            // Points of the outline that fall on this side split it.
            let mut on: Vec<(f64, VertexId)> = building_rows
                .iter()
                .filter_map(|(p, v)| {
                    let (t, d) = crate::math::project_on_segment(*p, a, b);
                    let eps = TOL / a.distance(b).max(TOL);
                    (d < TOL && t > eps && t < 1.0 - eps).then_some((t, *v))
                })
                .collect();
            on.sort_by(|x, y| x.0.total_cmp(&y.0));
            lp.extend(on.into_iter().map(|(_, v)| v));
        }
        lp.dedup();
        while lp.len() > 1 && lp.first() == lp.last() {
            lp.pop();
        }
        if lp.len() < 3 {
            continue;
        }
        let pl: Vec<DVec2> = lp.iter().map(|v| plan(ents.pos(*v))).collect();
        if (crate::math::signed_area_2d(&pl) > 0.0) != up {
            lp.reverse();
        }
        building.extend(lp.iter().copied());
        if let Some(f) = ents.add_face_raw(lp, Vec::new()) {
            stamped.push(f);
        }
    }

    // The ring: Delaunay triangles whose centre is where the new surface belongs.
    let plan_pts: Vec<DVec2> = rows.iter().map(|(p, _)| plan(*p)).collect();
    let foot_polys: Vec<Vec<DVec2>> = fp.faces.iter().map(|f| f.iter().map(|p| plan(*p)).collect()).collect();
    let mut soft_pairs: Vec<(VertexId, VertexId)> = Vec::new();
    for t in triangulate(&plan_pts) {
        let tags = t.map(|i| rows[i].1);
        let buildings = tags.iter().filter(|g| **g == Tag::Building).count();
        let centre = t.iter().map(|&i| rows[i].0).sum::<DVec3>() / 3.0;
        let c2 = plan(centre);
        let in_offset = polys.iter().any(|p| point_in_polygon(p, c2, true));
        let bad = if buildings == 3 {
            foot_polys.iter().any(|p| point_in_polygon(p, c2, true)) || !in_offset
        } else {
            !in_offset
        };
        if bad {
            continue;
        }
        let [a, b, c] = t;
        let area2 = (plan_pts[b] - plan_pts[a]).perp_dot(plan_pts[c] - plan_pts[a]);
        if area2.abs() < TOL * TOL {
            continue;
        }
        let order = if (area2 > 0.0) == up { [a, b, c] } else { [a, c, b] };
        let vs = order.map(|i| row_v[i]);
        if let Some(f) = ents.add_face_raw(vs.to_vec(), Vec::new()) {
            stamped.push(f);
        }
        for k in 0..3 {
            let (i, j) = (order[k], order[(k + 1) % 3]);
            if rows[i].1 != rows[j].1 {
                soft_pairs.push((row_v[i], row_v[j]));
            }
        }
    }
    // Edges between the footprint and the cut are soft, unless the mesh shows most of its
    // edges.
    if !mostly_visible {
        for (a, b) in soft_pairs {
            if let Some(e) = ents.find_edge(a, b) {
                ents.edges[e].soft = true;
                ents.edges[e].smooth = true;
            }
        }
    }
    for f in stamped {
        ents.faces[f].front = material.0;
        ents.faces[f].back = material.1;
    }
    let mut building: Vec<VertexId> = building.into_iter().collect();
    building.sort();
    Ok(StampState { building, level })
}

/// Moves the footprint (and the slope that hangs on it) by `dz`.
pub fn shift_building(ents: &mut Entities, st: &StampState, dz: f64) {
    for &v in &st.building {
        if let Some(vx) = ents.verts.get_mut(v) {
            vx.pos.z += dz;
        }
    }
    refresh_normals(ents, &st.building);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sculpt::triangle_grid;

    fn square(cx: f64, cy: f64, half: f64, z: f64) -> Vec<DVec3> {
        vec![
            DVec3::new(cx - half, cy - half, z),
            DVec3::new(cx + half, cy - half, z),
            DVec3::new(cx + half, cy + half, z),
            DVec3::new(cx - half, cy + half, z),
        ]
    }

    fn terrain(z: impl Fn(usize, usize) -> f64) -> Entities {
        let mut ents = Entities::default();
        triangle_grid(&mut ents, DVec3::ZERO, 12, 12, 500.0, z);
        ents
    }

    /// How many faces cover each sample point of the plan (no gap, no overlap = 1).
    fn coverage(ents: &Entities, lo: f64, hi: f64) -> Vec<(DVec2, usize)> {
        let polys: Vec<Vec<DVec2>> =
            ents.faces.values().map(|f| ents.loop_points(&f.outer).iter().map(|p| plan(*p)).collect()).collect();
        let mut out = Vec::new();
        let mut y = lo + 13.7;
        while y < hi {
            let mut x = lo + 29.3;
            while x < hi {
                let p = DVec2::new(x, y);
                out.push((p, polys.iter().filter(|poly| point_in_polygon(poly, p, true)).count()));
                x += 211.3;
            }
            y += 197.9;
        }
        out
    }

    #[test]
    fn offset_outline_of_a_square() {
        let flat = square(0.0, 0.0, 500.0, 0.0);
        let n = poly_normal(&flat);
        let o = offset_outline(&flat, n, 100.0);
        let expect = [(-600.0, -600.0), (600.0, -600.0), (600.0, 600.0), (-600.0, 600.0)];
        for (p, e) in o.iter().zip(expect) {
            assert!(p.distance(DVec2::new(e.0, e.1)) < 1e-9, "{p:?}");
        }
        // The other winding offsets outwards as well.
        let rev: Vec<DVec3> = flat.iter().rev().copied().collect();
        let o2 = offset_outline(&rev, poly_normal(&rev), 100.0);
        assert!(o2.iter().all(|p| p.x.abs() > 599.0 && p.y.abs() > 599.0));
    }

    #[test]
    fn footprint_of_a_box_is_its_underside() {
        // A box 1000 × 800 × 600 standing at z = 200: the underside is the footprint,
        // whatever way the faces look.
        let b = |z: f64| {
            vec![
                DVec3::new(0.0, 0.0, z),
                DVec3::new(1000.0, 0.0, z),
                DVec3::new(1000.0, 800.0, z),
                DVec3::new(0.0, 800.0, z),
            ]
        };
        let mut faces = vec![b(200.0), b(800.0)];
        for i in 0..4 {
            let (p, q) = (b(200.0), b(800.0));
            faces.push(vec![p[i], p[(i + 1) % 4], q[(i + 1) % 4], q[i]]);
        }
        let fp = Footprint::find(&faces);
        assert_eq!(fp.faces.len(), 1);
        assert!(fp.faces[0].iter().all(|p| p.z == 200.0));
        assert_eq!(fp.profile.len(), 1);
        assert_eq!(fp.profile[0].len(), 4);
        assert!(fp.flat_profile[0].iter().all(|p| p.z == 200.0));
    }

    #[test]
    fn footprint_ignores_what_is_above_another_object() {
        let mut faces = vec![square(0.0, 0.0, 500.0, 0.0)];
        faces.push(square(0.0, 0.0, 200.0, 900.0));
        let fp = Footprint::find(&faces);
        assert_eq!(fp.faces.len(), 1);
        assert!(fp.faces[0].iter().all(|p| p.z == 0.0));
    }

    #[test]
    fn stamp_cuts_and_fills_a_flat_terrain() {
        let mut ents = terrain(|_, _| 0.0);
        let fp = Footprint::find(&[square(3000.0, 3000.0, 500.0, 300.0)]);
        let st = prepare(&mut ents, TerrainSel::All, &fp, 600.0).expect("stamp");
        // The footprint sits one inch under the highest point of the terrain.
        assert!((st.level - 325.4).abs() < 1e-9);
        let foot: Vec<DVec3> = st.building.iter().map(|v| ents.pos(*v)).collect();
        assert!(foot.len() >= 4);
        assert!(foot.iter().all(|p| (p.z + 25.4).abs() < 1e-9), "{foot:?}");
        // Every point of the plan is covered by exactly one face.
        for (p, n) in coverage(&ents, 0.0, 6000.0) {
            assert_eq!(n, 1, "{p:?} covered {n} times");
        }
        // The terrain faces inside the cut are gone: (2500, 2500) is inside the outline
        // (1900..4100: the footprint 2500..3500 pushed by 600).
        assert!(ents.find_vertex(DVec3::new(2500.0, 2500.0, 0.0)).is_none());
        assert!(ents.find_vertex(DVec3::new(1500.0, 1500.0, 0.0)).is_some());
        // The footprint face is there, flat, looking up.
        let f = ents
            .faces
            .values()
            .find(|f| ents.loop_points(&f.outer).iter().all(|p| (p.z + 25.4).abs() < 1e-9))
            .expect("footprint face");
        assert!(f.normal.z > 0.99);
        // The edges of the cut stay at the terrain's height, the slope goes down.
        assert!(ents.verts.values().any(|v| (v.pos.x - 1900.0).abs() < 1e-6 && v.pos.z == 0.0));
        // Moving the footprint up lifts the slope with it.
        shift_building(&mut ents, &st, 400.0);
        assert!(st.building.iter().all(|v| (ents.pos(*v).z - 374.6).abs() < 1e-9));
        assert!(ents.faces.values().any(|f| f.normal.z < 0.99), "the ring is sloped");
        for (p, n) in coverage(&ents, 0.0, 6000.0) {
            assert_eq!(n, 1, "{p:?} covered {n} times after the move");
        }
    }

    #[test]
    fn stamp_follows_a_sloping_terrain() {
        let mut ents = terrain(|i, j| i as f64 * 40.0 + j as f64 * 10.0);
        let max_z = ents.verts.values().map(|v| v.pos.z).fold(f64::MIN, f64::max);
        let fp = Footprint::find(&[square(3000.0, 2500.0, 700.0, 0.0)]);
        let st = prepare(&mut ents, TerrainSel::All, &fp, 400.0).expect("stamp");
        for (p, n) in coverage(&ents, 0.0, 6000.0) {
            assert_eq!(n, 1, "{p:?} covered {n} times");
        }
        for v in &st.building {
            assert!((ents.pos(*v).z - (max_z - 25.4)).abs() < 1e-9);
        }
        // The cut follows the terrain: its points are on the old surface.
        let cut: Vec<DVec3> = ents
            .verts
            .values()
            .map(|v| v.pos)
            .filter(|p| (p.x - 1900.0).abs() < 1e-6 && !st.building.iter().any(|b| ents.pos(*b) == *p))
            .collect();
        assert!(cut.len() >= 3);
        for p in cut {
            let expect = p.x / 500.0 * 40.0 + p.y / 500.0 * 10.0;
            assert!((p.z - expect).abs() < 1e-6, "{p:?}");
        }
    }

    #[test]
    fn stamp_an_l_shaped_footprint() {
        let mut ents = terrain(|i, j| (i as f64 * 0.7).sin() * 60.0 + j as f64 * 15.0);
        let l = vec![
            DVec3::new(2200.0, 2200.0, 0.0),
            DVec3::new(3800.0, 2200.0, 0.0),
            DVec3::new(3800.0, 2900.0, 0.0),
            DVec3::new(2900.0, 2900.0, 0.0),
            DVec3::new(2900.0, 3800.0, 0.0),
            DVec3::new(2200.0, 3800.0, 0.0),
        ];
        let fp = Footprint::find(&[l]);
        assert_eq!(fp.profile[0].len(), 6);
        let st = prepare(&mut ents, TerrainSel::All, &fp, 450.0).expect("stamp");
        for (p, n) in coverage(&ents, 0.0, 6000.0) {
            assert_eq!(n, 1, "{p:?} covered {n} times");
        }
        assert!(st.building.len() >= 6);
    }

    #[test]
    fn edges_of_the_slope_are_soft_when_the_terrain_hides_its_own() {
        let mut ents = terrain(|_, _| 0.0);
        for e in ents.edges.values_mut() {
            e.soft = true;
        }
        let fp = Footprint::find(&[square(3000.0, 3000.0, 500.0, 0.0)]);
        let st = prepare(&mut ents, TerrainSel::All, &fp, 600.0).expect("stamp");
        let building: HashSet<VertexId> = st.building.iter().copied().collect();
        let mut radial = 0;
        for e in ents.edges.values() {
            let (a, b) = (building.contains(&e.v[0]), building.contains(&e.v[1]));
            if a != b {
                assert!(e.soft && e.smooth, "edge from the footprint to the cut is soft");
                radial += 1;
            }
        }
        assert!(radial >= 4);
    }

    #[test]
    fn a_stamp_needs_a_footprint_and_a_mesh() {
        let mut ents = terrain(|_, _| 0.0);
        assert!(prepare(&mut ents, TerrainSel::All, &Footprint::default(), 100.0).is_err());
        let fp = Footprint::find(&[square(3000.0, 3000.0, 500.0, 0.0)]);
        let mut empty = Entities::default();
        assert!(prepare(&mut empty, TerrainSel::All, &fp, 100.0).is_err());
    }
}
