//! Triangulation of planar faces (with holes).

use glam::{DVec2, DVec3};

use crate::math::basis_for;
use crate::model::{Entities, FaceId};

/// Triangles as index triples into the concatenation `outer ++ holes...`.
pub fn triangulate_2d(outer: &[DVec2], holes: &[Vec<DVec2>]) -> Vec<[usize; 3]> {
    let mut flat = Vec::with_capacity((outer.len() + holes.iter().map(Vec::len).sum::<usize>()) * 2);
    let mut hole_idx = Vec::with_capacity(holes.len());
    for p in outer {
        flat.extend([p.x, p.y]);
    }
    for h in holes {
        hole_idx.push(flat.len() / 2);
        for p in h {
            flat.extend([p.x, p.y]);
        }
    }
    let Ok(idx) = earcutr::earcut(&flat, &hole_idx, 2) else { return Vec::new() };
    idx.as_chunks::<3>().0.iter().map(|c| [c[0], c[1], c[2]]).collect()
}

/// A point strictly inside the region (centroid of its largest triangle).
pub fn interior_point(outer: &[DVec2], holes: &[Vec<DVec2>]) -> Option<DVec2> {
    let all: Vec<DVec2> = outer.iter().chain(holes.iter().flatten()).copied().collect();
    triangulate_2d(outer, holes)
        .into_iter()
        .map(|[a, b, c]| {
            let (pa, pb, pc) = (all[a], all[b], all[c]);
            ((pb - pa).perp_dot(pc - pa).abs(), (pa + pb + pc) / 3.0)
        })
        .max_by(|x, y| x.0.total_cmp(&y.0))
        .map(|x| x.1)
}

/// World-space triangles of a face, wound counter-clockwise around its normal.
///
/// A face of at least [`BIG`] points keeps its triangles ([`big`]): one with thousands of holes
/// takes seconds to triangulate, and the view, the picking cache and the tools ask for it
/// again after every change of the model.
pub fn face_triangles(ents: &Entities, f: FaceId) -> Vec<[DVec3; 3]> {
    let face = &ents.faces[f];
    if face.outer.len() + face.holes.iter().map(Vec::len).sum::<usize>() >= BIG {
        return big::triangles(ents, face);
    }
    triangulate_face(ents, face)
}

fn triangulate_face(ents: &Entities, face: &crate::model::Face) -> Vec<[DVec3; 3]> {
    let (u, v) = basis_for(face.normal);
    let pts3: Vec<DVec3> = face.loops().flat_map(|l| l.iter().map(|&x| ents.verts[x].pos)).collect();
    let to2 = |p: &DVec3| DVec2::new(p.dot(u), p.dot(v));
    let outer: Vec<DVec2> = face.outer.iter().map(|&x| to2(&ents.verts[x].pos)).collect();
    let holes: Vec<Vec<DVec2>> =
        face.holes.iter().map(|h| h.iter().map(|&x| to2(&ents.verts[x].pos)).collect()).collect();
    triangulate_2d(&outer, &holes)
        .into_iter()
        .map(|[a, b, c]| {
            let t = [pts3[a], pts3[b], pts3[c]];
            if (t[1] - t[0]).cross(t[2] - t[0]).dot(face.normal) < 0.0 { [t[0], t[2], t[1]] } else { t }
        })
        .collect()
}

pub fn face_area(ents: &Entities, f: FaceId) -> f64 {
    face_triangles(ents, f).iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).length() * 0.5).sum()
}

/// Points (outer loop and holes) from which a face's triangles are kept.
pub const BIG: usize = 128;

/// The triangles of the big faces met lately, for any model of the process. The triangles
/// of a face depend only on its normal and on the positions of its loops, in their order:
/// a face whose normal and points are the same, bit for bit, gets them back unchanged.
mod big {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use glam::DVec3;

    use crate::model::{Entities, Face};

    /// What is kept at most (points and triangles), the least recently used going first.
    const MAX_BYTES: usize = 128 << 20;

    /// What the triangles are made from: the normal, the length of each loop, the points.
    #[derive(PartialEq, Eq)]
    struct Key {
        normal: [u64; 3],
        loops: Vec<usize>,
        points: Vec<[u64; 3]>,
    }

    impl Key {
        fn of(ents: &Entities, face: &Face) -> Key {
            let bits = |p: DVec3| p.to_array().map(f64::to_bits);
            Key {
                normal: bits(face.normal),
                loops: face.loops().map(Vec::len).collect(),
                points: face.loops().flatten().map(|&x| bits(ents.verts[x].pos)).collect(),
            }
        }

        fn hash(&self) -> u64 {
            let mut h = 0xcbf2_9ce4_8422_2325u64;
            let mut add = |x: u64| h = (h.rotate_left(5) ^ x).wrapping_mul(0x5851_f42d_4c95_7f2d);
            self.normal.iter().for_each(|&x| add(x));
            self.loops.iter().for_each(|&x| add(x as u64));
            self.points.iter().flatten().for_each(|&x| add(x));
            h
        }

        fn bytes(&self) -> usize {
            self.points.len() * 24 + self.loops.len() * 8
        }
    }

    struct Kept {
        key: Key,
        tris: Arc<Vec<[DVec3; 3]>>,
        used: u64,
    }

    #[derive(Default)]
    struct Cache {
        by_hash: HashMap<u64, Vec<Kept>>,
        bytes: usize,
        tick: u64,
    }

    impl Cache {
        fn get(&mut self, h: u64, key: &Key) -> Option<Arc<Vec<[DVec3; 3]>>> {
            self.tick += 1;
            let k = self.by_hash.get_mut(&h)?.iter_mut().find(|k| k.key == *key)?;
            k.used = self.tick;
            Some(k.tris.clone())
        }

        fn put(&mut self, h: u64, key: Key, tris: Arc<Vec<[DVec3; 3]>>) {
            let size = key.bytes() + tris.len() * 72;
            if size > MAX_BYTES || self.by_hash.get(&h).is_some_and(|v| v.iter().any(|k| k.key == key)) {
                return;
            }
            while self.bytes + size > MAX_BYTES {
                let Some((&oldest, i)) = self
                    .by_hash
                    .iter()
                    .flat_map(|(h, v)| v.iter().enumerate().map(move |(i, k)| (h, i, k.used)))
                    .min_by_key(|x| x.2)
                    .map(|(h, i, _)| (h, i))
                else {
                    break;
                };
                let v = self.by_hash.get_mut(&oldest).expect("présent");
                let k = v.swap_remove(i);
                self.bytes -= k.key.bytes() + k.tris.len() * 72;
                if v.is_empty() {
                    self.by_hash.remove(&oldest);
                }
            }
            self.tick += 1;
            self.bytes += size;
            self.by_hash.entry(h).or_default().push(Kept { key, tris, used: self.tick });
        }
    }

    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

    thread_local! {
        static MADE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }

    fn with<R>(f: impl FnOnce(&mut Cache) -> R) -> R {
        let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        f(c.get_or_insert_with(Cache::default))
    }

    pub(super) fn triangles(ents: &Entities, face: &Face) -> Vec<[DVec3; 3]> {
        let key = Key::of(ents, face);
        let h = key.hash();
        if let Some(t) = with(|c| c.get(h, &key)) {
            return t.to_vec();
        }
        // Triangulated outside the lock: two threads can work on two faces at once.
        let t = super::triangulate_face(ents, face);
        MADE.with(|n| n.set(n.get() + 1));
        with(|c| c.put(h, key, Arc::new(t.clone())));
        t
    }

    /// Big faces this thread triangulated (not found among the kept ones).
    pub fn made() -> u64 {
        MADE.with(|n| n.get())
    }
}

/// Big faces ([`BIG`]) this thread triangulated rather than found already done.
pub fn big_faces_triangulated() -> u64 {
    big::made()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Entities;

    /// A square with `n` square holes: `4 + 4n` points.
    fn holed(e: &mut Entities, n: usize, shift: f64) -> FaceId {
        let side = (n as f64).sqrt().ceil() as usize;
        let size = side as f64 * 30.0 + 10.0;
        let outer = [DVec3::ZERO, DVec3::new(size, 0.0, 0.0), DVec3::new(size, size, 0.0), DVec3::new(0.0, size, 0.0)]
            .map(|p| e.add_vertex_raw(p + DVec3::new(shift, 0.0, 0.0)));
        let holes = (0..n)
            .map(|i| {
                let o = DVec3::new(10.0 + (i % side) as f64 * 30.0 + shift, 10.0 + (i / side) as f64 * 30.0, 0.0);
                [DVec3::ZERO, DVec3::new(0.0, 20.0, 0.0), DVec3::new(20.0, 20.0, 0.0), DVec3::new(20.0, 0.0, 0.0)]
                    .map(|p| e.add_vertex_raw(o + p))
                    .to_vec()
            })
            .collect();
        e.add_face_raw(outer.to_vec(), holes).expect("face")
    }

    #[test]
    fn a_big_face_is_triangulated_once_and_again_when_a_point_moves() {
        let mut e = Entities::default();
        let f = holed(&mut e, 40, 0.0);
        let fresh = triangulate_face(&e, &e.faces[f]);
        let n0 = big_faces_triangulated();
        assert_eq!(face_triangles(&e, f), fresh);
        let n1 = big_faces_triangulated();
        assert!(n1 - n0 <= 1, "{}", n1 - n0);
        // Asked again, here and in another collection with the same points: found.
        assert_eq!(face_triangles(&e, f), fresh);
        let mut other = Entities::default();
        let g = holed(&mut other, 40, 0.0);
        assert_eq!(face_triangles(&other, g), fresh);
        assert_eq!(big_faces_triangulated(), n1);
        // A point moved by the smallest step: triangulated again, as a new face would be.
        let v = e.faces[f].holes[3][2];
        let p = e.verts[v].pos;
        e.verts[v].pos = DVec3::new(f64::from_bits(p.x.to_bits() + 1), p.y, p.z);
        let moved = triangulate_face(&e, &e.faces[f]);
        assert_eq!(face_triangles(&e, f), moved);
        assert_eq!(big_faces_triangulated(), n1 + 1);
        // The same points with another normal (the face reversed): its own triangles.
        let mut flipped = Entities::default();
        let h = holed(&mut flipped, 40, 0.0);
        flipped.faces[h].normal = -flipped.faces[h].normal;
        assert_eq!(face_triangles(&flipped, h), triangulate_face(&flipped, &flipped.faces[h]));
        // Small faces are not kept.
        let mut small = Entities::default();
        let s = holed(&mut small, 2, 5.0);
        let before = big_faces_triangulated();
        let _ = face_triangles(&small, s);
        assert_eq!(big_faces_triangulated(), before);
    }
}
