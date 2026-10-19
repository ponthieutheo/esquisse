//! Sandbox tools (Tools › Sandbox): Smoove, Stamp and Drape work on a mesh of faces, most
//! often a triangulated terrain. This module holds the pure geometry; the interactive tools
//! live in `esq-app`.
//!
//! Everything here is expressed in the coordinates of the entities collection being edited
//! (the "container"), like SketchUp's sandbox scripts do.

use std::collections::HashSet;
use std::f64::consts::FRAC_PI_2;

use glam::{DMat4, DVec2, DVec3};

use crate::math::{TOL, point_in_polygon_2d};
use crate::model::{EdgeId, Entities, EntityRef, FaceId, Model, Units, VertexId};

pub mod delaunay;
pub mod drape;
pub mod stamp;

/// Plan position of a point.
pub(crate) fn plan(p: DVec3) -> DVec2 {
    DVec2::new(p.x, p.y)
}

/// Where a point lies relative to a polygon seen from above.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Where {
    Inside,
    /// Within the tolerance of an edge.
    Border,
    Outside,
}

pub fn classify_point(poly: &[DVec2], p: DVec2) -> Where {
    if poly.len() < 3 {
        return Where::Outside;
    }
    for i in 0..poly.len() {
        if plan_dist_to_segment(p, poly[i], poly[(i + 1) % poly.len()]) < TOL {
            return Where::Border;
        }
    }
    if point_in_polygon_2d(p, poly) { Where::Inside } else { Where::Outside }
}

/// `Geom.point_in_polygon_2D`: a point on the border counts as inside only with
/// `check_border`.
pub fn point_in_polygon(poly: &[DVec2], p: DVec2, check_border: bool) -> bool {
    match classify_point(poly, p) {
        Where::Inside => true,
        Where::Border => check_border,
        Where::Outside => false,
    }
}

/// Sandbox defaults (SandboxDefaults.ini), in millimetres. SketchUp keeps two sets: one
/// for models in inches/feet and one for metric models.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SandboxDefaults {
    pub smoove_radius: f64,
    pub stamp_offset: f64,
    pub grid_spacing: f64,
}

impl SandboxDefaults {
    pub fn for_units(units: Units) -> Self {
        match units {
            Units::Inches | Units::Feet | Units::Yards => {
                SandboxDefaults { smoove_radius: 360.0 * 25.4, stamp_offset: 12.0 * 25.4, grid_spacing: 120.0 * 25.4 }
            }
            Units::Millimeters | Units::Centimeters | Units::Meters => {
                SandboxDefaults { smoove_radius: 10_000.0, stamp_offset: 1_000.0, grid_spacing: 3_000.0 }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// Smoove

/// Share of the move a vertex receives at `dist` from the base: sin² of a quarter wave,
/// 1 at the base, 0 at the radius and beyond.
pub fn smoove_falloff(dist: f64, radius: f64) -> f64 {
    if radius <= 0.0 || dist >= radius {
        return 0.0;
    }
    let s = ((1.0 - dist / radius) * FRAC_PI_2).sin();
    s * s
}

/// What the distances of a Smoove are measured from.
#[derive(Clone, Debug)]
pub enum SmooveSeed {
    /// Distance to a vertex, in plan.
    Vertex(VertexId),
    /// Distance to the nearest of some edges, in plan.
    Edges(Vec<EdgeId>),
}

#[derive(Clone, Copy, Debug)]
pub struct SmooveVertex {
    pub id: VertexId,
    /// Position when the Smoove started.
    pub origin: DVec3,
    /// Plan distance to the seed.
    pub dist: f64,
}

/// Every vertex of the mesh connected to the seed, with its starting position and its plan
/// distance to the seed. Applying a move from the starting positions keeps the operation
/// repeatable (the mouse and the radius can change while it runs).
#[derive(Clone, Debug)]
pub struct SmooveField {
    pub verts: Vec<SmooveVertex>,
}

/// Plan distance from `p` to the segment `a`-`b`: to the line when the foot of the
/// perpendicular lies on the segment, else to the nearer end.
fn plan_dist_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let dir = b - a;
    let len2 = dir.length_squared();
    if len2 < 1e-18 {
        return p.distance(b);
    }
    let t = (p - a).dot(dir) / len2;
    if (0.0..=1.0).contains(&t) {
        (p - a).perp_dot(dir).abs() / len2.sqrt()
    } else if t <= 0.5 {
        p.distance(a)
    } else {
        p.distance(b)
    }
}

impl SmooveField {
    pub fn new(ents: &Entities, seed: &SmooveSeed) -> Option<SmooveField> {
        let first = match seed {
            SmooveSeed::Vertex(v) => *ents.verts.get(*v)?.edges.first()?,
            SmooveSeed::Edges(es) => *es.iter().find(|e| ents.edges.contains_key(**e))?,
        };
        let mut ids: HashSet<VertexId> = HashSet::new();
        for item in ents.connected(EntityRef::Edge(first)) {
            if let EntityRef::Edge(e) = item {
                ids.extend(ents.edges[e].v);
            }
        }
        let mut ids: Vec<VertexId> = ids.into_iter().collect();
        ids.sort();
        let plan = |p: DVec3| DVec2::new(p.x, p.y);
        let seg: Vec<(DVec2, DVec2)> = match seed {
            SmooveSeed::Vertex(_) => Vec::new(),
            SmooveSeed::Edges(es) => es
                .iter()
                .filter(|e| ents.edges.contains_key(**e))
                .map(|e| {
                    let (a, b) = ents.edge_points(*e);
                    (plan(a), plan(b))
                })
                .collect(),
        };
        let verts = ids
            .into_iter()
            .map(|id| {
                let origin = ents.pos(id);
                let p = plan(origin);
                let dist = match seed {
                    SmooveSeed::Vertex(v) => p.distance(plan(ents.pos(*v))),
                    SmooveSeed::Edges(_) => {
                        seg.iter().map(|(a, b)| plan_dist_to_segment(p, *a, *b)).fold(f64::INFINITY, f64::min)
                    }
                };
                SmooveVertex { id, origin, dist }
            })
            .collect();
        Some(SmooveField { verts })
    }

    /// The vertices a Smoove of this radius moves.
    pub fn affected(&self, radius: f64) -> impl Iterator<Item = &SmooveVertex> {
        self.verts.iter().filter(move |v| v.dist <= radius)
    }

    /// Puts every vertex at its starting position plus `delta` scaled by the falloff.
    /// Vertices beyond the radius go back to where they started.
    pub fn apply(&self, ents: &mut Entities, delta: DVec3, radius: f64) {
        let mut moved: Vec<VertexId> = Vec::new();
        for v in &self.verts {
            let Some(vx) = ents.verts.get_mut(v.id) else { continue };
            let pos = v.origin + delta * smoove_falloff(v.dist, radius);
            if vx.pos != pos {
                vx.pos = pos;
                moved.push(v.id);
            }
        }
        refresh_normals(ents, &moved);
    }
}

/// Recomputes the normals of the faces around some vertices (after they moved).
pub fn refresh_normals(ents: &mut Entities, verts: &[VertexId]) {
    let mut faces: HashSet<FaceId> = HashSet::new();
    for v in verts {
        let Some(vx) = ents.verts.get(*v) else { continue };
        for &e in &vx.edges {
            faces.extend(ents.edges[e].faces.iter().copied());
        }
    }
    for f in faces {
        if ents.faces.contains_key(f) {
            ents.recompute_normal(f);
        }
    }
}

/// Average of the normals of some faces, unit length (zero when there is none).
pub fn average_normal(ents: &Entities, faces: &[FaceId]) -> DVec3 {
    let sum: DVec3 = faces.iter().filter_map(|f| ents.faces.get(*f)).map(|f| f.normal).sum();
    sum.normalize_or_zero()
}

/// Faces touching a vertex.
pub fn faces_of_vertex(ents: &Entities, v: VertexId) -> Vec<FaceId> {
    let mut out: Vec<FaceId> = Vec::new();
    if let Some(vx) = ents.verts.get(v) {
        for &e in &vx.edges {
            for &f in &ents.edges[e].faces {
                if !out.contains(&f) {
                    out.push(f);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// Gathering what the user selected (Drape and Stamp)

const MAX_DEPTH: usize = 32;

/// Edges to drape, in world coordinates: the visible edges (not hidden, not soft) of the
/// selected edges, faces and, inside groups and components, of everything they hold. Edges
/// that are almost vertical have no projection and are left out.
pub fn collect_edges(model: &Model, ents: &Entities, xform: &DMat4, items: &[EntityRef]) -> Vec<(DVec3, DVec3)> {
    fn add(ents: &Entities, e: EdgeId, xform: &DMat4, out: &mut Vec<(DVec3, DVec3)>) {
        let Some(edge) = ents.edges.get(e) else { return };
        if edge.hidden || edge.soft {
            return;
        }
        let (a, b) = ents.edge_points(e);
        let (a, b) = (xform.transform_point3(a), xform.transform_point3(b));
        // 0.1 square inch of run in plan.
        if (b - a).truncate().length_squared() <= 0.1 * 25.4 * 25.4 {
            return;
        }
        let same = |p: &(DVec3, DVec3)| {
            (p.0.distance(a) < TOL && p.1.distance(b) < TOL) || (p.0.distance(b) < TOL && p.1.distance(a) < TOL)
        };
        if !out.iter().any(same) {
            out.push((a, b));
        }
    }
    fn walk(
        model: &Model,
        ents: &Entities,
        xform: &DMat4,
        items: &[EntityRef],
        out: &mut Vec<(DVec3, DVec3)>,
        depth: usize,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        for it in items {
            match *it {
                EntityRef::Edge(e) => add(ents, e, xform, out),
                EntityRef::Face(f) => {
                    if ents.faces.contains_key(f) {
                        for e in ents.face_edges(f) {
                            add(ents, e, xform, out);
                        }
                    }
                }
                EntityRef::Instance(i) => {
                    let Some(inst) = ents.instances.get(i) else { continue };
                    let Some(def) = model.defs.get(inst.def) else { continue };
                    let all: Vec<EntityRef> = def
                        .entities
                        .edges
                        .keys()
                        .map(EntityRef::Edge)
                        .chain(def.entities.instances.keys().map(EntityRef::Instance))
                        .collect();
                    walk(model, &def.entities, &(*xform * inst.xform), &all, out, depth + 1);
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(model, ents, xform, items, &mut out, 0);
    out
}

/// Outer loops of the selected faces, in world coordinates; groups and components bring
/// all their faces.
pub fn collect_faces(model: &Model, ents: &Entities, xform: &DMat4, items: &[EntityRef]) -> Vec<Vec<DVec3>> {
    fn walk(
        model: &Model,
        ents: &Entities,
        xform: &DMat4,
        items: &[EntityRef],
        out: &mut Vec<Vec<DVec3>>,
        depth: usize,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        for it in items {
            match *it {
                EntityRef::Face(f) => {
                    if let Some(face) = ents.faces.get(f) {
                        out.push(ents.loop_points(&face.outer).iter().map(|p| xform.transform_point3(*p)).collect());
                    }
                }
                EntityRef::Instance(i) => {
                    let Some(inst) = ents.instances.get(i) else { continue };
                    let Some(def) = model.defs.get(inst.def) else { continue };
                    let all: Vec<EntityRef> = def
                        .entities
                        .faces
                        .keys()
                        .map(EntityRef::Face)
                        .chain(def.entities.instances.keys().map(EntityRef::Instance))
                        .collect();
                    walk(model, &def.entities, &(*xform * inst.xform), &all, out, depth + 1);
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(model, ents, xform, items, &mut out, 0);
    out
}

/// The faces and edges of the mesh a tool works on: everything connected to `seed` in
/// `ents`, or the whole collection.
pub fn mesh_items(ents: &Entities, seed: Option<EntityRef>) -> (Vec<FaceId>, Vec<EdgeId>) {
    match seed {
        Some(s) => {
            let mut faces = Vec::new();
            let mut edges = Vec::new();
            for it in ents.connected(s) {
                match it {
                    EntityRef::Face(f) => faces.push(f),
                    EntityRef::Edge(e) => edges.push(e),
                    _ => {}
                }
            }
            (faces, edges)
        }
        None => (ents.faces.keys().collect(), ents.edges.keys().collect()),
    }
}

// ---------------------------------------------------------------------------------------
// Shared helpers

/// A flat grid of triangles `nx` × `ny` cells wide, lower corner at `origin`, `z(i, j)` the
/// height of the node `(i, j)`. Test helper: returns the vertex ids, `(nx + 1)` per row.
pub fn triangle_grid(
    ents: &mut Entities,
    origin: DVec3,
    nx: usize,
    ny: usize,
    spacing: f64,
    z: impl Fn(usize, usize) -> f64,
) -> Vec<VertexId> {
    let mut ids = Vec::with_capacity((nx + 1) * (ny + 1));
    for j in 0..=ny {
        for i in 0..=nx {
            ids.push(ents.add_vertex_raw(origin + DVec3::new(i as f64 * spacing, j as f64 * spacing, z(i, j))));
        }
    }
    let at = |i: usize, j: usize| ids[j * (nx + 1) + i];
    for j in 0..ny {
        for i in 0..nx {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            ents.add_face_raw(vec![a, b, c], Vec::new());
            ents.add_face_raw(vec![a, c, d], Vec::new());
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falloff_profile() {
        assert!((smoove_falloff(0.0, 100.0) - 1.0).abs() < 1e-12);
        assert!((smoove_falloff(50.0, 100.0) - 0.5).abs() < 1e-12);
        assert_eq!(smoove_falloff(100.0, 100.0), 0.0);
        assert_eq!(smoove_falloff(250.0, 100.0), 0.0);
        // sin² of a quarter wave: 25 % of the radius from the base gives sin²(3π/8).
        let expect = (3.0 * std::f64::consts::PI / 8.0).sin().powi(2);
        assert!((smoove_falloff(25.0, 100.0) - expect).abs() < 1e-12);
    }

    #[test]
    fn defaults_follow_the_model_units() {
        assert_eq!(SandboxDefaults::for_units(Units::Inches).smoove_radius, 9144.0);
        assert!((SandboxDefaults::for_units(Units::Inches).stamp_offset - 304.8).abs() < 1e-9);
        assert_eq!(SandboxDefaults::for_units(Units::Millimeters).smoove_radius, 10_000.0);
    }

    #[test]
    fn plan_distance_to_a_segment() {
        let (a, b) = (DVec2::new(0.0, 0.0), DVec2::new(10.0, 0.0));
        assert!((plan_dist_to_segment(DVec2::new(5.0, 3.0), a, b) - 3.0).abs() < 1e-12);
        assert!((plan_dist_to_segment(DVec2::new(-3.0, 4.0), a, b) - 5.0).abs() < 1e-12);
        assert!((plan_dist_to_segment(DVec2::new(14.0, 3.0), a, b) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn smoove_moves_the_vertices_under_the_radius_only() {
        let mut ents = Entities::default();
        let ids = triangle_grid(&mut ents, DVec3::ZERO, 6, 6, 100.0, |_, _| 0.0);
        let centre = ids[3 * 7 + 3];
        let field = SmooveField::new(&ents, &SmooveSeed::Vertex(centre)).unwrap();
        assert_eq!(field.verts.len(), 49);
        field.apply(&mut ents, DVec3::new(0.0, 0.0, 80.0), 250.0);
        assert!((ents.pos(centre).z - 80.0).abs() < 1e-9);
        // One cell away: 100 of 250.
        let near = ids[3 * 7 + 4];
        assert!((ents.pos(near).z - 80.0 * smoove_falloff(100.0, 250.0)).abs() < 1e-9);
        // Beyond the radius: untouched.
        assert_eq!(ents.pos(ids[0]).z, 0.0);
        // Smaller radius afterwards: the vertex that fell out goes back.
        field.apply(&mut ents, DVec3::new(0.0, 0.0, 80.0), 150.0);
        assert_eq!(ents.pos(ids[3 * 7 + 5]).z, 0.0);
        assert!(ents.pos(near).z > 0.0);
        // Faces follow: the normals are recomputed.
        assert!(ents.faces.values().any(|f| (f.normal.z - 1.0).abs() > 1e-6));
    }

    #[test]
    fn smoove_from_an_edge_measures_to_the_segment() {
        let mut ents = Entities::default();
        let ids = triangle_grid(&mut ents, DVec3::ZERO, 4, 4, 100.0, |_, _| 0.0);
        let e = ents.find_edge(ids[2 * 5], ids[2 * 5 + 1]).unwrap();
        let field = SmooveField::new(&ents, &SmooveSeed::Edges(vec![e])).unwrap();
        let by = |id| field.verts.iter().find(|v| v.id == id).unwrap().dist;
        assert!(by(ids[2 * 5]).abs() < 1e-9);
        assert!((by(ids[2 * 5 + 4]) - 300.0).abs() < 1e-9);
        assert!((by(ids[4 * 5 + 1]) - 200.0).abs() < 1e-9);
    }
}
