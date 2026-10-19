//! Follow Me: sweeps a face (the profile) along a chain of edges (the path).

use std::collections::{HashMap, HashSet};

use glam::DVec3;

use crate::math::TOL;
use crate::model::{EdgeId, Entities, FaceId, VertexId};

/// A path for Follow Me: ordered points, and whether it is smooth at each point
/// (the two edges meeting there belong to the same arc or circle).
#[derive(Clone, Debug)]
pub struct Path {
    pub points: Vec<DVec3>,
    pub closed: bool,
    pub smooth: Vec<bool>,
}

impl Entities {
    /// Orders a set of edges into a path without branches.
    pub fn edge_path(&self, edges: &[EdgeId]) -> Result<Path, &'static str> {
        let set: HashSet<EdgeId> = edges.iter().copied().filter(|e| self.edges.contains_key(*e)).collect();
        if set.is_empty() {
            return Err("Le chemin est vide");
        }
        let mut adj: HashMap<VertexId, Vec<EdgeId>> = HashMap::new();
        for &e in &set {
            for v in self.edges[e].v {
                adj.entry(v).or_default().push(e);
            }
        }
        if adj.values().any(|l| l.len() > 2) {
            return Err("Le chemin ne doit pas avoir d'embranchement");
        }
        let ends: Vec<VertexId> = adj.iter().filter(|(_, l)| l.len() == 1).map(|(v, _)| *v).collect();
        let closed = ends.is_empty();
        let start = if closed { self.edges[*set.iter().next().unwrap()].v[0] } else { ends[0] };
        let mut verts = vec![start];
        let mut used: Vec<EdgeId> = Vec::new();
        let mut cur = start;
        while let Some(&e) = adj[&cur].iter().find(|e| !used.contains(e)) {
            used.push(e);
            cur = self.other_vertex(e, cur);
            if cur == start {
                break;
            }
            verts.push(cur);
        }
        if used.len() != set.len() {
            return Err("Le chemin doit être d'un seul tenant");
        }
        let n = verts.len();
        let smooth = (0..n)
            .map(|k| {
                let around: Vec<EdgeId> = if closed {
                    vec![used[(k + n - 1) % n], used[k]]
                } else if k == 0 || k == n - 1 {
                    return false;
                } else {
                    vec![used[k - 1], used[k]]
                };
                let c = |e: EdgeId| self.edges[e].curve;
                c(around[0]).is_some() && c(around[0]) == c(around[1])
            })
            .collect();
        Ok(Path { points: verts.iter().map(|&v| self.pos(v)).collect(), closed, smooth })
    }

    /// Edges of a chain without branches going through `e` (what SketchUp follows when you
    /// click an edge after the profile).
    pub fn chain_through(&self, e: EdgeId, exclude: &HashSet<EdgeId>) -> Vec<EdgeId> {
        let mut out = vec![e];
        for start in self.edges[e].v {
            let (mut v, mut prev) = (start, e);
            loop {
                let next: Vec<EdgeId> =
                    self.verts[v].edges.iter().copied().filter(|x| *x != prev && !exclude.contains(x)).collect();
                if next.len() != 1 || out.contains(&next[0]) {
                    break;
                }
                prev = next[0];
                out.push(prev);
                v = self.other_vertex(prev, v);
            }
        }
        out
    }

    /// Follow Me: extrudes face `profile` along `path`, with mitred corners. An open path
    /// keeps the profile as the start cap and closes the end; a closed path makes a ring and
    /// removes the profile. Returns the new faces.
    pub fn follow_me(&mut self, profile: FaceId, path: &Path) -> Result<Vec<FaceId>, &'static str> {
        let face = self.faces.get(profile).ok_or("Le profil n'existe plus")?.clone();
        let mut pts = path.points.clone();
        let mut smooth = path.smooth.clone();
        // Drop repeated points.
        let mut k = 1;
        while k < pts.len() {
            if pts[k].distance(pts[k - 1]) < TOL {
                pts.remove(k);
                smooth.remove(k);
            } else {
                k += 1;
            }
        }
        if path.closed && pts.len() > 2 && pts[0].distance(*pts.last().unwrap()) < TOL {
            pts.pop();
            smooth.pop();
        }
        let n = pts.len();
        if n < 2 || (path.closed && n < 3) {
            return Err("Le chemin est trop court");
        }

        // Start where the profile is: the end of the path nearest to it, or for a loop the
        // nearest point, going the way that leaves the profile's plane most steeply.
        let outer_pts = self.loop_points(&face.outer);
        let centre = outer_pts.iter().copied().sum::<DVec3>() / outer_pts.len() as f64;
        if path.closed {
            let i = (0..n).min_by(|&a, &b| pts[a].distance(centre).total_cmp(&pts[b].distance(centre))).unwrap();
            pts.rotate_left(i);
            smooth.rotate_left(i);
            let fwd = (pts[1] - pts[0]).normalize().dot(face.normal).abs();
            let back = (pts[n - 1] - pts[0]).normalize().dot(face.normal).abs();
            if back > fwd {
                pts[1..].reverse();
                smooth[1..].reverse();
            }
        } else if pts[n - 1].distance(centre) < pts[0].distance(centre) {
            pts.reverse();
            smooth.reverse();
        }

        let segs = if path.closed { n } else { n - 1 };
        let dirs: Vec<DVec3> = (0..segs).map(|k| (pts[(k + 1) % n] - pts[k]).normalize()).collect();
        if dirs[0].dot(face.normal).abs() < 1e-6 {
            return Err("Le profil ne doit pas être dans le prolongement du chemin");
        }
        // Mitre plane normal at each path point.
        let mitre = |k: usize| -> DVec3 {
            let (before, after) = if path.closed {
                (dirs[(k + segs - 1) % segs], dirs[k])
            } else if k == 0 {
                (dirs[0], dirs[0])
            } else if k == n - 1 {
                (dirs[segs - 1], dirs[segs - 1])
            } else {
                (dirs[k - 1], dirs[k])
            };
            (before + after).normalize_or_zero()
        };
        let project = |x: DVec3, d: DVec3, k: usize| -> Result<DVec3, &'static str> {
            let m = mitre(k);
            let den = m.dot(d);
            if m == DVec3::ZERO || den.abs() < 1e-9 {
                return Err("Le chemin fait demi-tour sur lui-même");
            }
            Ok(x + d * (m.dot(pts[k] - x) / den))
        };

        // Profile loops turning counter-clockwise around the direction of travel.
        let flip = face.normal.dot(dirs[0]) < 0.0;
        let mut loops: Vec<Vec<VertexId>> = face.loops().cloned().collect();
        if flip {
            loops.iter_mut().for_each(|l| l.reverse());
        }
        let loop_pts: Vec<Vec<DVec3>> = loops.iter().map(|l| self.loop_points(l)).collect();
        // A profile point is smooth when both profile edges around it belong to one curve.
        let profile_smooth: Vec<Vec<bool>> = loops
            .iter()
            .map(|l| {
                let m = l.len();
                (0..m)
                    .map(|i| {
                        let c = |a: VertexId, b: VertexId| self.find_edge(a, b).and_then(|e| self.edges[e].curve);
                        let (x, y) = (c(l[(i + m - 1) % m], l[i]), c(l[i], l[(i + 1) % m]));
                        x.is_some() && x == y
                    })
                    .collect()
            })
            .collect();

        // Rings of points, one per path point.
        let mut rings: Vec<Vec<Vec<DVec3>>> = Vec::with_capacity(n);
        let first: Vec<Vec<DVec3>> = if path.closed {
            loop_pts.iter().map(|l| l.iter().map(|&x| project(x, dirs[0], 0)).collect()).collect::<Result<_, _>>()?
        } else {
            loop_pts.clone()
        };
        rings.push(first);
        for k in 1..n {
            let prev = &rings[k - 1];
            let next: Vec<Vec<DVec3>> = prev
                .iter()
                .map(|l| l.iter().map(|&x| project(x, dirs[k - 1], k)).collect())
                .collect::<Result<_, _>>()?;
            rings.push(next);
        }

        // Vertices: an open path reuses the profile for its first ring.
        let mut ids: Vec<Vec<Vec<VertexId>>> = Vec::with_capacity(n);
        for (k, ring) in rings.iter().enumerate() {
            if k == 0 && !path.closed {
                ids.push(loops.clone());
                continue;
            }
            let r = ring
                .iter()
                .map(|l| l.iter().map(|&p| self.find_vertex(p).unwrap_or_else(|| self.add_vertex_raw(p))).collect())
                .collect();
            ids.push(r);
        }

        let mut out = Vec::new();
        for s in 0..segs {
            let (a, b) = (s, (s + 1) % n);
            for (li, l) in ids[a].iter().enumerate() {
                let m = l.len();
                for i in 0..m {
                    let mut quad = vec![l[i], l[(i + 1) % m], ids[b][li][(i + 1) % m], ids[b][li][i]];
                    quad.dedup();
                    if quad.first() == quad.last() {
                        quad.pop();
                    }
                    if let Some(f) = self.add_face_raw(quad, Vec::new()) {
                        out.push(f);
                    }
                    // Lengthwise edges of a curved profile, and crosswise edges where the path
                    // curves, are softened.
                    if profile_smooth[li][i]
                        && let Some(e) = self.find_edge(l[i], ids[b][li][i])
                    {
                        let edge = &mut self.edges[e];
                        (edge.soft, edge.smooth) = (true, true);
                    }
                    if smooth[b]
                        && (path.closed || b < n - 1)
                        && let Some(e) = self.find_edge(ids[b][li][i], ids[b][li][(i + 1) % m])
                    {
                        let edge = &mut self.edges[e];
                        (edge.soft, edge.smooth) = (true, true);
                    }
                }
            }
        }

        if path.closed {
            let edges = self.face_edges(profile);
            self.remove_face(profile);
            let lonely: Vec<EdgeId> =
                edges.into_iter().filter(|e| self.edges.get(*e).is_some_and(|x| x.faces.is_empty())).collect();
            for e in lonely {
                self.remove_edge_raw(e);
            }
            self.remove_orphan_vertices();
        } else {
            // Start cap faces backwards, end cap forwards.
            if !flip {
                self.reverse_face(profile);
            }
            let end = &ids[n - 1];
            if let Some(f) = self.add_face_raw(end[0].clone(), end[1..].to_vec()) {
                out.push(f);
            }
        }
        Ok(out)
    }
}
