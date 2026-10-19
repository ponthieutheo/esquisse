//! Drape: projects edges straight down (or up) onto a mesh. The edges become edges of the
//! mesh, which splits the faces they cross.

use glam::{DVec2, DVec3};

use super::{Where, classify_point, plan};
use crate::math::TOL;
use crate::model::{EdgeId, Entities, FaceId};

struct SurfFace {
    outer: Vec<DVec2>,
    holes: Vec<Vec<DVec2>>,
    /// Plane `n · p = d`, only for faces that are not vertical.
    plane: Option<(DVec3, f64)>,
    min: DVec2,
    max: DVec2,
}

/// A snapshot of the faces of a mesh seen from above: where the surface is, and where a
/// vertical wall cuts it. The snapshot does not change when the mesh is edited afterwards.
pub struct Surface {
    faces: Vec<SurfFace>,
    edges: Vec<(DVec3, DVec3)>,
}

impl Surface {
    pub fn from_faces(ents: &Entities, faces: &[FaceId]) -> Surface {
        let mut out = Vec::new();
        let mut edges: Vec<(DVec3, DVec3)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for &f in faces {
            let Some(face) = ents.faces.get(f) else { continue };
            let outer3 = ents.loop_points(&face.outer);
            let outer: Vec<DVec2> = outer3.iter().map(|p| plan(*p)).collect();
            let (mut min, mut max) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
            for p in &outer {
                min = min.min(*p);
                max = max.max(*p);
            }
            let plane = (face.normal.z.abs() > 1e-9).then(|| (face.normal, face.normal.dot(outer3[0])));
            let holes = face.holes.iter().map(|h| ents.loop_points(h).iter().map(|p| plan(*p)).collect()).collect();
            out.push(SurfFace { outer, holes, plane, min, max });
            for l in face.loops() {
                for i in 0..l.len() {
                    let (a, b) = (l[i], l[(i + 1) % l.len()]);
                    let key = if a < b { (a, b) } else { (b, a) };
                    if seen.insert(key) {
                        edges.push((ents.pos(a), ents.pos(b)));
                    }
                }
            }
        }
        Surface { faces: out, edges }
    }

    fn face_contains(f: &SurfFace, p: DVec2) -> bool {
        if p.x < f.min.x - TOL || p.y < f.min.y - TOL || p.x > f.max.x + TOL || p.y > f.max.y + TOL {
            return false;
        }
        if classify_point(&f.outer, p) == Where::Outside {
            return false;
        }
        !f.holes.iter().any(|h| classify_point(h, p) == Where::Inside)
    }

    /// Height of the surface above the plan point `p`, if the mesh is there.
    pub fn height_at(&self, p: DVec2) -> Option<f64> {
        self.faces.iter().find_map(|f| {
            let (n, d) = f.plane?;
            Self::face_contains(f, p).then(|| (d - n.x * p.x - n.y * p.y) / n.z)
        })
    }

    /// The path a vertical wall over the plan segment `a`-`b` draws on the surface: the
    /// ends of the segment and every place where it crosses an edge of the mesh, in order.
    /// Stretches that leave the mesh are not part of the path (the result is split there by
    /// consecutive points that are not joined: see `drape_segment`).
    pub fn crossings(&self, a: DVec2, b: DVec2) -> Vec<DVec3> {
        let dir = b - a;
        let len = dir.length();
        if len < TOL {
            return Vec::new();
        }
        let mut events: Vec<(f64, DVec3)> = Vec::new();
        let eps_t = TOL / len;
        for (p, q) in &self.edges {
            let (p2, q2) = (plan(*p), plan(*q));
            let e = q2 - p2;
            let elen = e.length();
            if elen < 1e-12 {
                continue;
            }
            let denom = dir.perp_dot(e);
            if denom.abs() < 1e-12 * len * elen {
                continue; // parallel or collinear: the neighbours' crossings give the ends
            }
            let w = p2 - a;
            let t = w.perp_dot(e) / denom;
            let u = w.perp_dot(dir) / denom;
            let eps_u = TOL / elen;
            if t < -eps_t || t > 1.0 + eps_t || u < -eps_u || u > 1.0 + eps_u {
                continue;
            }
            let u = u.clamp(0.0, 1.0);
            let t = t.clamp(0.0, 1.0);
            let z = p.z + (q.z - p.z) * u;
            events.push((t, DVec3::new(a.x + dir.x * t, a.y + dir.y * t, z)));
        }
        for (t, pt) in [(0.0, a), (1.0, b)] {
            if let Some(z) = self.height_at(pt) {
                events.push((t, DVec3::new(pt.x, pt.y, z)));
            }
        }
        events.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut out: Vec<DVec3> = Vec::new();
        for (_, p) in events {
            if out.last().is_none_or(|q| q.distance(p) >= TOL) {
                out.push(p);
            }
        }
        out
    }

    /// Whether the plan point is over the mesh.
    pub fn covers(&self, p: DVec2) -> bool {
        self.faces.iter().any(|f| Self::face_contains(f, p))
    }
}

/// Draws on the mesh the projection of the segment `a`-`b` (only its plan position counts).
/// Returns the points of the path, and adds the edges that join them to `ents`.
pub fn drape_segment(ents: &mut Entities, surface: &Surface, a: DVec3, b: DVec3) -> (Vec<DVec3>, Vec<EdgeId>) {
    let pts = surface.crossings(plan(a), plan(b));
    let mut edges = Vec::new();
    for w in pts.windows(2) {
        // Two consecutive points are joined only when the mesh lies between them.
        let mid = (w[0] + w[1]) * 0.5;
        match surface.height_at(plan(mid)) {
            Some(z) if (z - mid.z).abs() < TOL * 2.0 => {}
            _ => continue,
        }
        edges.extend(ents.add_line(w[0], w[1]));
    }
    edges.retain(|e| ents.edges.contains_key(*e));
    (pts, edges)
}

/// Drapes several segments onto the faces `terrain`; returns how many edges were drawn.
pub fn drape_edges(ents: &mut Entities, terrain: &[FaceId], segments: &[(DVec3, DVec3)]) -> usize {
    let surface = Surface::from_faces(ents, terrain);
    let mut n = 0;
    for &(a, b) in segments {
        n += drape_segment(ents, &surface, a, b).1.len();
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sculpt::triangle_grid;

    fn terrain(z: impl Fn(usize, usize) -> f64) -> (Entities, Vec<FaceId>) {
        let mut ents = Entities::default();
        triangle_grid(&mut ents, DVec3::ZERO, 4, 4, 100.0, z);
        let faces = ents.faces.keys().collect();
        (ents, faces)
    }

    #[test]
    fn height_follows_the_faces() {
        let (ents, faces) = terrain(|i, _| i as f64 * 100.0);
        let s = Surface::from_faces(&ents, &faces);
        assert!((s.height_at(DVec2::new(250.0, 130.0)).unwrap() - 250.0).abs() < 1e-9);
        assert!(s.height_at(DVec2::new(450.0, 130.0)).is_none());
    }

    #[test]
    fn a_wall_cuts_the_faces_it_crosses() {
        let (mut ents, faces) = terrain(|_, _| 0.0);
        let before = ents.faces.len();
        let s = Surface::from_faces(&ents, &faces);
        // From the middle of a cell to the middle of another, along X, at y = 130.
        let (pts, edges) =
            drape_segment(&mut ents, &s, DVec3::new(50.0, 130.0, 900.0), DVec3::new(350.0, 130.0, 900.0));
        assert!(pts.len() >= 4, "{pts:?}");
        assert!(!edges.is_empty());
        // The path is on the surface.
        assert!(pts.iter().all(|p| p.z.abs() < 1e-9));
        // Faces were split along the path.
        assert!(ents.faces.len() > before, "{} faces", ents.faces.len());
        // Nothing was lost: the mesh still covers its 400 × 400.
        let area: f64 = ents.faces.keys().map(|f| crate::tess::face_area(&ents, f)).sum();
        assert!((area - 160_000.0).abs() < 1e-3, "{area}");
        // The ends of the path are vertices now.
        assert!(ents.find_vertex(DVec3::new(50.0, 130.0, 0.0)).is_some());
        assert!(ents.find_vertex(DVec3::new(350.0, 130.0, 0.0)).is_some());
    }

    #[test]
    fn the_path_follows_the_relief() {
        let (mut ents, faces) = terrain(|i, _| i as f64 * 100.0);
        let s = Surface::from_faces(&ents, &faces);
        let (pts, _) = drape_segment(&mut ents, &s, DVec3::new(20.0, 210.0, -50.0), DVec3::new(380.0, 210.0, -50.0));
        for p in &pts {
            assert!((p.z - p.x).abs() < 1e-6, "{p:?}");
        }
        assert!((pts.first().unwrap().z - 20.0).abs() < 1e-6);
        assert!((pts.last().unwrap().z - 380.0).abs() < 1e-6);
    }

    #[test]
    fn nothing_is_drawn_outside_the_mesh() {
        let (mut ents, faces) = terrain(|_, _| 0.0);
        let s = Surface::from_faces(&ents, &faces);
        // Leaves the mesh on the right and comes back on the left.
        let (pts, edges) = drape_segment(&mut ents, &s, DVec3::new(300.0, 100.0, 0.0), DVec3::new(600.0, 100.0, 0.0));
        assert_eq!(pts.last().unwrap().x, 400.0);
        assert!(edges.iter().all(|e| {
            let (p, q) = ents.edge_points(*e);
            p.x <= 400.0 + 1e-9 && q.x <= 400.0 + 1e-9
        }));
    }
}
