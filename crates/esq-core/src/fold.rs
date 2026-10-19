//! Moving geometry that stays connected to the rest of the model (Move tool, after CMoveTool's
//! motion constraints and auto-fold).
//!
//! Without auto-fold, a displacement is restricted so that every neighbouring face stays flat
//! (free, parallel to a plane, parallel to a line, or no motion at all). With auto-fold, or when
//! no such displacement exists, the faces that stop being flat are folded: cut into flat pieces
//! by new edges.

use std::collections::HashSet;

use glam::{DVec2, DVec3};

use crate::math::{TOL, basis_for, newell_normal};
use crate::model::{Entities, FaceId, VertexId};

/// Displacements that keep the faces around a moved selection flat.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum MoveLimit {
    /// Any displacement.
    #[default]
    Free,
    /// Displacements perpendicular to this unit normal.
    Plane(DVec3),
    /// Displacements along this unit direction.
    Line(DVec3),
    /// No displacement keeps every face flat.
    Fixed,
}

impl MoveLimit {
    /// The allowed displacement closest to `d`.
    pub fn apply(&self, d: DVec3) -> DVec3 {
        match *self {
            MoveLimit::Free => d,
            MoveLimit::Plane(n) => d - n * d.dot(n),
            MoveLimit::Line(l) => l * d.dot(l),
            MoveLimit::Fixed => DVec3::ZERO,
        }
    }

    /// The same limit seen through a placement (group or component context to world).
    pub fn transformed(&self, m: &glam::DMat4) -> MoveLimit {
        match *self {
            MoveLimit::Plane(n) => {
                let w = m.inverse().transpose().transform_vector3(n);
                if w.length() < 1e-12 { MoveLimit::Fixed } else { MoveLimit::Plane(w.normalize()) }
            }
            MoveLimit::Line(l) => {
                let w = m.transform_vector3(l);
                if w.length() < 1e-12 { MoveLimit::Fixed } else { MoveLimit::Line(w.normalize()) }
            }
            other => other,
        }
    }

    fn with_normal(self, n: DVec3) -> MoveLimit {
        match self {
            MoveLimit::Free => MoveLimit::Plane(n),
            MoveLimit::Plane(p) => {
                let l = p.cross(n);
                if l.length() < 1e-6 { self } else { MoveLimit::Line(l.normalize()) }
            }
            MoveLimit::Line(l) => {
                if l.dot(n).abs() < 1e-6 {
                    self
                } else {
                    MoveLimit::Fixed
                }
            }
            MoveLimit::Fixed => MoveLimit::Fixed,
        }
    }
}

fn is_collinear(pts: &[DVec3]) -> bool {
    let Some(&a) = pts.first() else { return true };
    let Some(&b) = pts.iter().find(|p| p.distance(a) > TOL) else { return true };
    let d = (b - a).normalize();
    pts.iter().all(|p| (*p - a).cross(d).length() < TOL)
}

impl Entities {
    fn faces_touching(&self, verts: &HashSet<VertexId>) -> Vec<FaceId> {
        let mut out: HashSet<FaceId> = HashSet::new();
        for v in verts {
            let Some(vx) = self.verts.get(*v) else { continue };
            for &e in &vx.edges {
                if let Some(edge) = self.edges.get(e) {
                    out.extend(edge.faces.iter().copied());
                }
            }
        }
        out.into_iter().collect()
    }

    /// Which displacements of `moved` keep every face that is only partly moved flat.
    /// A face is pinned by its vertices that stay: three that are not aligned fix its plane.
    pub fn move_limit(&self, moved: &HashSet<VertexId>) -> MoveLimit {
        let mut limit = MoveLimit::Free;
        for f in self.faces_touching(moved) {
            let face = &self.faces[f];
            let (mut still, mut any_moved) = (Vec::new(), false);
            for v in face.all_vertices() {
                if moved.contains(&v) {
                    any_moved = true;
                } else {
                    still.push(self.verts[v].pos);
                }
            }
            if any_moved && still.len() >= 3 && !is_collinear(&still) {
                limit = limit.with_normal(face.normal.normalize());
            }
        }
        limit
    }

    fn max_off_plane(&self, l: &[VertexId]) -> f64 {
        let pts = self.loop_points(l);
        let n = newell_normal(&pts);
        let c = pts.iter().copied().sum::<DVec3>() / pts.len() as f64;
        pts.iter().map(|p| (*p - c).dot(n).abs()).fold(0.0, f64::max)
    }

    /// Cuts the faces that touch `moved` and are no longer flat into flat pieces, ear by ear
    /// at the moved vertices so that the part that did not move stays in one piece.
    /// Returns the number of faces folded.
    pub fn fold_nonplanar(&mut self, moved: &HashSet<VertexId>) -> usize {
        let moved: HashSet<VertexId> = moved.iter().copied().filter(|v| self.verts.contains_key(*v)).collect();
        let mut count = 0;
        for f in self.faces_touching(&moved) {
            let Some(face) = self.faces.get(f) else { continue };
            if !face.holes.is_empty() || face.outer.len() < 4 || self.max_off_plane(&face.outer) <= TOL {
                continue;
            }
            if self.fold_face(f, &moved) {
                count += 1;
            }
        }
        count
    }

    fn polygon_is_flat(&self, poly: &[VertexId]) -> bool {
        poly.len() <= 3 || self.max_off_plane(poly) <= TOL
    }

    /// Whether the corner `i` of `poly` can be cut off as a triangle.
    fn is_ear(&self, poly: &[VertexId], i: usize, n: DVec3) -> bool {
        let m = poly.len();
        let (a, b, c) = (poly[(i + m - 1) % m], poly[i], poly[(i + 1) % m]);
        let (pa, pb, pc) = (self.pos(a), self.pos(b), self.pos(c));
        if (pb - pa).cross(pc - pb).dot(n) <= 1e-9 {
            return false;
        }
        let (u, v) = basis_for(n);
        let to2 = |p: DVec3| DVec2::new(p.dot(u), p.dot(v));
        let (qa, qb, qc) = (to2(pa), to2(pb), to2(pc));
        let side = |p: DVec2, s: DVec2, t: DVec2| (t - s).perp_dot(p - s);
        poly.iter().all(|&w| {
            if w == a || w == b || w == c {
                return true;
            }
            let q = to2(self.pos(w));
            !(side(q, qa, qb) >= 0.0 && side(q, qb, qc) >= 0.0 && side(q, qc, qa) >= 0.0)
        })
    }

    fn fold_face(&mut self, f: FaceId, moved: &HashSet<VertexId>) -> bool {
        let old = self.faces[f].clone();
        let mut poly = old.outer.clone();
        let mut pieces: Vec<Vec<VertexId>> = Vec::new();
        while poly.len() > 3 && !self.polygon_is_flat(&poly) {
            let n = newell_normal(&self.loop_points(&poly));
            let m = poly.len();
            let ear = (0..m)
                .find(|&i| moved.contains(&poly[i]) && self.is_ear(&poly, i, n))
                .or_else(|| (0..m).find(|&i| self.is_ear(&poly, i, n)));
            let Some(i) = ear else {
                // Degenerate outline: fan it out.
                for k in 1..m - 1 {
                    pieces.push(vec![poly[0], poly[k], poly[k + 1]]);
                }
                poly.clear();
                break;
            };
            pieces.push(vec![poly[(i + m - 1) % m], poly[i], poly[(i + 1) % m]]);
            poly.remove(i);
        }
        if poly.len() >= 3 {
            pieces.push(poly);
        }
        self.remove_face(f);
        for p in pieces {
            if let Some(nf) = self.add_face_raw(p, Vec::new()) {
                let face = &mut self.faces[nf];
                face.front = old.front;
                face.back = old.back;
                face.layer = old.layer;
                face.hidden = old.hidden;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntityRef;

    fn p(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    fn box_1m() -> Entities {
        let mut e = Entities::default();
        e.add_polyline(
            &[p(0.0, 0.0, 0.0), p(1000.0, 0.0, 0.0), p(1000.0, 1000.0, 0.0), p(0.0, 1000.0, 0.0)],
            true,
            false,
        );
        let f = e.faces.keys().next().unwrap();
        e.push_pull(f, -1000.0, false);
        e
    }

    fn vertex_at(e: &Entities, q: DVec3) -> VertexId {
        e.verts.iter().find(|(_, v)| v.pos.distance(q) < 1e-6).map(|(k, _)| k).unwrap()
    }

    fn top_face(e: &Entities) -> FaceId {
        e.faces.iter().find(|(_, f)| f.normal.z > 0.99).map(|(k, _)| k).unwrap()
    }

    #[test]
    fn flat_rectangle_vertex_is_limited_to_its_plane() {
        let mut e = Entities::default();
        e.add_polyline(
            &[p(0.0, 0.0, 0.0), p(1000.0, 0.0, 0.0), p(1000.0, 1000.0, 0.0), p(0.0, 1000.0, 0.0)],
            true,
            false,
        );
        let v = vertex_at(&e, p(1000.0, 1000.0, 0.0));
        let lim = e.move_limit(&HashSet::from([v]));
        let MoveLimit::Plane(n) = lim else { panic!("{lim:?}") };
        assert!(n.z.abs() > 0.999);
        assert!(lim.apply(p(10.0, 20.0, 300.0)).distance(p(10.0, 20.0, 0.0)) < 1e-9);
    }

    #[test]
    fn box_top_face_moves_freely() {
        let e = box_1m();
        let verts = e.vertices_of(&[EntityRef::Face(top_face(&e))]);
        assert_eq!(e.move_limit(&verts), MoveLimit::Free);
    }

    #[test]
    fn box_corner_has_no_flat_move() {
        let e = box_1m();
        let v = vertex_at(&e, p(1000.0, 1000.0, 1000.0));
        assert_eq!(e.move_limit(&HashSet::from([v])), MoveLimit::Fixed);
    }

    #[test]
    fn box_top_edge_slides_across_but_not_along() {
        let e = box_1m();
        let (a, b) = (vertex_at(&e, p(0.0, 1000.0, 1000.0)), vertex_at(&e, p(1000.0, 1000.0, 1000.0)));
        let lim = e.move_limit(&HashSet::from([a, b]));
        // The two end faces keep the edge from moving along itself; the rest is free.
        let MoveLimit::Plane(n) = lim else { panic!("{lim:?}") };
        assert!(n.x.abs() > 0.999);
    }

    #[test]
    fn folding_a_lifted_corner_cuts_off_a_triangle() {
        let mut e = Entities::default();
        e.add_polyline(
            &[p(0.0, 0.0, 0.0), p(1000.0, 0.0, 0.0), p(1000.0, 1000.0, 0.0), p(0.0, 1000.0, 0.0)],
            true,
            false,
        );
        let v = vertex_at(&e, p(1000.0, 1000.0, 0.0));
        let moved = HashSet::from([v]);
        e.transform_vertices(&moved, &glam::DMat4::from_translation(p(0.0, 0.0, 400.0)));
        assert_eq!(e.fold_nonplanar(&moved), 1);
        assert_eq!(e.faces.len(), 2);
        assert_eq!(e.edges.len(), 5);
        // The part that did not move stays whole: one piece is the triangle of the 3 fixed corners.
        let still = e.faces.values().filter(|f| f.outer.iter().all(|&w| w != v)).count();
        assert_eq!(still, 1);
        for f in e.faces.keys() {
            assert!(e.max_off_plane(&e.faces[f].outer) < 1e-9);
        }
        // Folding twice does nothing.
        assert_eq!(e.fold_nonplanar(&moved), 0);
    }

    #[test]
    fn limit_combinations() {
        let l = MoveLimit::Free.with_normal(DVec3::Z).with_normal(DVec3::X);
        assert_eq!(l, MoveLimit::Line(DVec3::Z.cross(DVec3::X)));
        assert_eq!(l.with_normal(DVec3::Y), MoveLimit::Fixed);
        assert_eq!(l.with_normal(DVec3::X), l);
    }
}
