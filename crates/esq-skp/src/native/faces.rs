//! The check SketchUp makes of each face when it opens a model (`docs/skp-format.md` §3.5): Check
//! Validity (`CFace`, vtable slot 16), in the mode that fixes what it finds.
//! What of it changes what a reader sees:
//! - a loop that turns the wrong way around the face's normal is turned over (`CLoop` slot 16):
//!   the outer loop must turn counterclockwise, the inner ones clockwise, by
//!   their signed area; a face of one loop has its plane turned over instead;
//!   a loop of fewer than 3 edges, or whose edges do not follow each other
//!   around (a use given twice), is invalid: an inner one is removed, the face of an outer one
//!   erased;
//! - a face whose plane is not finite is erased;
//! - a face with a vertex 0.001" or more from its plane gets the plane of its outer loop, fitted by
//!   least squares and oriented by the loop. When no plane
//!   fits or the fitted one does not hold all its vertices, SketchUp splits the
//!   face and erases the pieces of less than 0.001 square inch:
//!   a face that small is erased (the pieces' areas add up to its own); the
//!   splitting of a bigger one is not reproduced, the face stays.

use std::collections::HashMap;

use super::curves::{cross, dot, sub};
use super::doc::{Ents, Face, Id};

/// SketchUp's tolerance on lengths (inches), and on areas.
const TOL: f64 = 0.001;
/// The smallest determinant of the plane fit.
const DET_MIN: f64 = 1e-10;

fn normal(pl: [f64; 4]) -> [f64; 3] {
    [pl[0], pl[1], pl[2]]
}

/// the distance from a plane to a point.
fn dist(pl: [f64; 4], p: [f64; 3]) -> f64 {
    (p[1] * pl[1] + p[0] * pl[0] + p[2] * pl[2] + pl[3]).abs()
}

/// the signed area of a loop around a normal, by triangles from its first vertex
/// (positive when it turns counterclockwise).
fn loop_area(pts: &[[f64; 3]], n: [f64; 3]) -> f64 {
    if pts.len() < 3 {
        return 0.0;
    }
    let p0 = pts[0];
    let mut prev = sub(pts[1], p0);
    let mut area = 0.0;
    for &p in &pts[2..] {
        let cur = sub(p, p0);
        area += ((cur[2] * prev[1] - cur[1] * prev[2]) * n[0]
            + (cur[0] * prev[2] - cur[2] * prev[0]) * n[1]
            + (cur[1] * prev[0] - cur[0] * prev[1]) * n[2])
            * 0.5;
        prev = cur;
    }
    area
}

/// The area vector of a loop (half the sum of its triangles' cross products from its first
/// vertex: its length is the area, whatever the plane).
fn area_vector(pts: &[[f64; 3]]) -> [f64; 3] {
    let mut s = [0.0; 3];
    for w in pts.windows(2).skip(1) {
        let c = cross(sub(w[0], pts[0]), sub(w[1], pts[0]));
        s = [0, 1, 2].map(|k| s[k] + c[k] * 0.5);
    }
    s
}

/// the normal and constant of the plane of least squares, from the sums of the
/// points' coordinates (`sx`…), of their squares and products (`sxx`, `sxy`…) and their count `n`;
/// none when the points are on a line (the largest of the three 2×2 determinants under 1e-10).
#[allow(clippy::too_many_arguments)]
fn solve(
    sxx: f64,
    syy: f64,
    szz: f64,
    n: f64,
    sxy: f64,
    sxz: f64,
    sx: f64,
    syz: f64,
    sy: f64,
    sz: f64,
) -> Option<[f64; 4]> {
    let a = sxx - (sx * sx) / n;
    let b = syy - (sy * sy) / n;
    let c = szz - (sz * sz) / n;
    let d = sxy - (sy * sx) / n;
    let e = sxz - (sz * sx) / n;
    let f = syz - (sz * sy) / n;
    let det_z = b * a - d * d;
    let det_y = c * a - e * e;
    let det_x = c * b - f * f;
    let (nx, ny, nz);
    if det_z.abs() < det_y.abs() || det_z.abs() < det_x.abs() {
        if det_y.abs() < det_x.abs() {
            if det_x.abs() < DET_MIN {
                return None;
            }
            nx = 1.0;
            ny = -(d * c - f * e) / det_x;
            nz = -(e * b - f * d) / det_x;
        } else {
            if det_y.abs() < DET_MIN {
                return None;
            }
            nx = -(d * c - f * e) / det_y;
            ny = 1.0;
            nz = -(f * a - e * d) / det_y;
        }
    } else {
        if det_z.abs() < DET_MIN {
            return None;
        }
        nx = -(e * b - f * d) / det_z;
        ny = -(f * a - e * d) / det_z;
        nz = 1.0;
    }
    let len = (ny * ny + nx * nx + nz * nz).sqrt();
    let k = sy * ny + sx * nx + sz * nz;
    Some([nx / len, ny / len, nz / len, (-k / n) / len])
}

/// the plane of least squares of points, turned the way they turn (their area
/// vector), when all are within the tolerance of it.
fn fit_plane(pts: &[[f64; 3]]) -> Option<[f64; 4]> {
    if pts.len() < 3 {
        return None;
    }
    let (mut sx, mut sy, mut sz) = (0.0, 0.0, 0.0);
    for p in pts {
        sx += p[0];
        sy += p[1];
        sz += p[2];
    }
    let inv = 1.0 / pts.len() as f64;
    // The sums of the points around their centre.
    let mut s = [0.0f64; 10];
    let [n, sxx, syy, szz, sxy, sxz, syz, cx, cy, cz] = &mut s;
    for p in pts {
        *n += 1.0;
        let (x, y, z) = (p[0] - sx * inv, p[1] - sy * inv, p[2] - sz * inv);
        *cx += x;
        *cy += y;
        *cz += z;
        *sxx += x * x;
        *syy += y * y;
        *szz += z * z;
        *sxy += y * x;
        *sxz += z * x;
        *syz += z * y;
    }
    if !s.iter().all(|v| v.is_finite()) {
        return None;
    }
    let [n, sxx, syy, szz, sxy, sxz, syz, cx, cy, cz] = s;
    let mut pl = solve(sxx, syy, szz, n, sxy, sxz, cx, syz, cy, cz)?;
    pl[3] -= sx * inv * pl[0] + pl[1] * sy * inv + pl[2] * sz * inv;
    if dot(area_vector(pts), normal(pl)) < 0.0 {
        pl = pl.map(|v| -v);
    }
    pts.iter().all(|&p| dist(pl, p) <= TOL).then_some(pl)
}

/// a loop turned over, from the same first use.
fn reverse(lp: &mut [(Id, bool)]) {
    if let Some((_, rest)) = lp.split_first_mut() {
        rest.reverse();
    }
    for u in lp.iter_mut() {
        u.1 = !u.1;
    }
}

/// Fixes a face as Check Validity does (module doc); false when SketchUp erases it.
fn check_face(f: &mut Face, pos: &HashMap<Id, [f64; 3]>, ends: &HashMap<Id, (Id, Id)>) -> bool {
    if f.loops.is_empty() {
        return true;
    }
    // The start and end vertices of each use.
    let use_ends = |&(edge, rev): &(Id, bool)| {
        let &(s, t) = ends.get(&edge)?;
        Some(if rev { (t, s) } else { (s, t) })
    };
    let Some(uses) =
        f.loops.iter().map(|l| l.iter().map(use_ends).collect::<Option<Vec<_>>>()).collect::<Option<Vec<_>>>()
    else {
        return true;
    };
    // A loop of fewer than 3 edges, or whose edges do not follow each other around (each one
    // starting where the one before ends, the first where the last ends), is invalid: as the outer
    // loop, its face is erased; an inner one is removed.
    let valid = |l: &[(Id, Id)]| l.len() >= 3 && (0..l.len()).all(|i| l[(i + l.len() - 1) % l.len()].1 == l[i].0);
    if !valid(&uses[0]) {
        return false;
    }
    let mut loops = Vec::with_capacity(uses.len());
    let mut k = 0;
    f.loops.retain(|_| {
        let keep = valid(&uses[k]);
        if keep {
            loops.push(uses[k].iter().filter_map(|(s, _)| pos.get(s).copied()).collect::<Vec<_>>());
        }
        k += 1;
        keep
    });
    if loops.iter().zip(&f.loops).any(|(p, l)| p.len() != l.len()) {
        return true;
    }
    // The orientation of the loops, the outer one first.
    for k in 0..loops.len() {
        if (loop_area(&loops[k], normal(f.plane)) > 0.0) == (k == 0) {
            continue;
        }
        if loops.len() == 1 {
            f.plane = f.plane.map(|v| -v);
        } else {
            // Its vertices v0 v1 … vn-1 become v1 v0 vn-1 … v2.
            reverse(&mut f.loops[k]);
            let l = &mut loops[k];
            l.rotate_left(2);
            l.reverse();
        }
    }
    if !f.plane.iter().all(|v| v.is_finite()) {
        return false;
    }
    // The plane equation.
    if loops.iter().flatten().all(|&p| dist(f.plane, p) < TOL) {
        return true;
    }
    let fitted = fit_plane(&loops[0]);
    if let Some(pl) = fitted {
        f.plane = pl;
    }
    if loop_area(&loops[0], normal(f.plane)) <= 0.0 {
        f.plane = f.plane.map(|v| -v);
    }
    // (A triangle's plane is also compared with the one of its three vertices,:
    // the same points, the same plane.)
    if fitted.is_some_and(|pl| loops.iter().flatten().all(|&p| dist(pl, p) < TOL)) {
        return true;
    }
    let len = |v: [f64; 3]| dot(v, v).sqrt();
    let area = len(area_vector(&loops[0])) - loops[1..].iter().map(|l| len(area_vector(l))).sum::<f64>();
    area >= TOL
}

/// Fixes the faces of a collection as SketchUp's Check Validity does, and erases those it erases
/// (module doc).
pub(crate) fn check_faces(e: &mut Ents) {
    if e.faces.is_empty() {
        return;
    }
    let pos: HashMap<Id, [f64; 3]> = e.verts.iter().map(|v| (v.id, v.pos)).collect();
    let ends: HashMap<Id, (Id, Id)> = e.edges.iter().map(|x| (x.el.id, (x.start, x.end))).collect();
    let keep: Vec<bool> = e.faces.iter_mut().map(|f| check_face(f, &pos, &ends)).collect();
    if keep.iter().all(|&k| k) {
        return;
    }
    let mut it = keep.into_iter();
    e.faces.retain(|_| it.next().unwrap_or(true));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::doc::{Edge, El, Vertex};

    /// A collection of one face: its loops given by their points (the outer one first), each
    /// edge in the loop's direction.
    fn face(loops: &[&[[f64; 3]]], plane: [f64; 4]) -> Ents {
        let mut e = Ents::default();
        let mut f = Face { plane, ..Default::default() };
        for l in loops {
            let first = e.verts.len() as Id;
            for (i, &p) in l.iter().enumerate() {
                e.verts.push(Vertex { id: first + i as Id, pos: p });
            }
            let mut lp = Vec::new();
            for i in 0..l.len() {
                let id = 1000 + e.edges.len() as Id;
                let (s, t) = (first + i as Id, first + ((i + 1) % l.len()) as Id);
                e.edges.push(Edge { el: El { id, ..Default::default() }, start: s, end: t, curve: None });
                lp.push((id, false));
            }
            f.loops.push(lp);
        }
        e.faces.push(f);
        e
    }

    fn points(e: &Ents, k: usize) -> Vec<[f64; 3]> {
        let pos: HashMap<Id, [f64; 3]> = e.verts.iter().map(|v| (v.id, v.pos)).collect();
        e.faces[0].loops[k]
            .iter()
            .map(|&(id, rev)| {
                let x = e.edges.iter().find(|x| x.el.id == id).unwrap();
                pos[if rev { &x.end } else { &x.start }]
            })
            .collect()
    }

    const SQUARE: [[f64; 3]; 4] = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]];

    #[test]
    fn an_inner_loop_turning_as_the_outer_one_is_turned_over() {
        let hole = [[4.0, 4.0, 0.0], [6.0, 4.0, 0.0], [6.0, 6.0, 0.0], [4.0, 6.0, 0.0]];
        let mut e = face(&[&SQUARE, &hole], [0.0, 0.0, 1.0, 0.0]);
        check_faces(&mut e);
        assert_eq!(e.faces.len(), 1);
        assert_eq!(e.faces[0].plane, [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(points(&e, 0), SQUARE.to_vec());
        // From the same first use, the other way: its end first.
        assert_eq!(points(&e, 1), vec![hole[1], hole[0], hole[3], hole[2]]);
        assert!(loop_area(&points(&e, 1), [0.0, 0.0, 1.0]) < 0.0);
    }

    #[test]
    fn a_face_of_one_loop_turning_clockwise_has_its_plane_turned_over() {
        let mut e = face(&[&SQUARE], [0.0, 0.0, -1.0, 0.0]);
        check_faces(&mut e);
        assert_eq!(e.faces[0].plane, [-0.0, -0.0, 1.0, -0.0]);
        assert_eq!(points(&e, 0), SQUARE.to_vec());
    }

    #[test]
    fn an_outer_loop_turning_clockwise_around_a_hole_is_turned_over() {
        let hole = [[4.0, 4.0, 0.0], [6.0, 4.0, 0.0], [6.0, 6.0, 0.0], [4.0, 6.0, 0.0]];
        let mut e = face(&[&SQUARE, &hole], [0.0, 0.0, -1.0, 0.0]);
        check_faces(&mut e);
        // The outer loop turns counterclockwise around -z; the hole, clockwise, stays.
        assert_eq!(e.faces[0].plane, [0.0, 0.0, -1.0, 0.0]);
        assert!(loop_area(&points(&e, 0), [0.0, 0.0, -1.0]) > 0.0);
        assert_eq!(points(&e, 1), hole.to_vec());
    }

    #[test]
    fn a_wrong_plane_is_fitted_again_from_the_outer_loop() {
        // A square in the plane x = 5, written with the plane z = 0.
        let sq = [[5.0, 0.0, 0.0], [5.0, 10.0, 0.0], [5.0, 10.0, 10.0], [5.0, 0.0, 10.0]];
        let mut e = face(&[&sq], [0.0, 0.0, 1.0, 0.0]);
        check_faces(&mut e);
        assert_eq!(e.faces.len(), 1);
        let pl = e.faces[0].plane;
        assert!((pl[0] - 1.0).abs() < 1e-12 && pl[1].abs() < 1e-12 && pl[2].abs() < 1e-12, "{pl:?}");
        assert!((pl[3] + 5.0).abs() < 1e-12, "{pl:?}");
    }

    #[test]
    fn a_tiny_face_off_its_plane_is_erased_a_big_one_stays() {
        // Sides of 0.003": no plane fits (determinants under 1e-10), less than 0.001 sq. in.
        let tiny = [[1.0, 1.0, 1.0], [1.003, 1.0, 1.0005], [1.0, 1.003, 1.001]];
        let mut e = face(&[&tiny], [0.0, 0.0, 1.0, 0.0]);
        check_faces(&mut e);
        assert!(e.faces.is_empty());
        // Sides of 0.01": a plane fits, the face stays on it.
        let small = [[1.0, 1.0, 1.0], [1.01, 1.0, 1.002], [1.0, 1.01, 1.003]];
        let mut e = face(&[&small], [0.0, 0.0, 1.0, 0.0]);
        check_faces(&mut e);
        assert_eq!(e.faces.len(), 1);
        assert!(small.iter().all(|&p| dist(e.faces[0].plane, p) < 1e-12));
        // Not flat: no plane holds its vertices; SketchUp would split it, it stays.
        let warped = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 1.0], [0.0, 10.0, 0.0]];
        let mut e = face(&[&warped], [0.0, 0.0, 1.0, 0.0]);
        check_faces(&mut e);
        assert_eq!(e.faces.len(), 1);
    }

    #[test]
    fn a_face_whose_plane_is_not_finite_is_erased() {
        let mut e = face(&[&SQUARE], [f64::NAN, 0.0, 1.0, 0.0]);
        check_faces(&mut e);
        assert!(e.faces.is_empty());
        let mut e = face(&[&SQUARE[..2]], [0.0, 0.0, 1.0, 0.0]);
        check_faces(&mut e);
        assert!(e.faces.is_empty());
    }

    #[test]
    fn a_loop_whose_edges_do_not_follow_each_other_is_invalid() {
        // A use given twice (as in `PCB_Rough v2.skp`): the outer loop's face is erased.
        let mut e = face(&[&SQUARE], [0.0, 0.0, 1.0, 0.0]);
        let u = e.faces[0].loops[0][1];
        e.faces[0].loops[0].insert(1, u);
        check_faces(&mut e);
        assert!(e.faces.is_empty());
        // An inner one is removed.
        let hole = [[4.0, 4.0, 0.0], [4.0, 6.0, 0.0], [6.0, 6.0, 0.0], [6.0, 4.0, 0.0]];
        let mut e = face(&[&SQUARE, &hole], [0.0, 0.0, 1.0, 0.0]);
        e.faces[0].loops[1].swap(0, 2);
        check_faces(&mut e);
        assert_eq!(e.faces[0].loops.len(), 1);
    }
}
