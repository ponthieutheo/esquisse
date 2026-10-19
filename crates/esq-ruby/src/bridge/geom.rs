//! Pont natif : opérations de la géométrie. Renvoie `None` si
//! l'opération n'est pas de ce module.
#![allow(unused_imports)]

use glam::{DVec2, DVec3};
use serde_json::{Value as J, json};

use esq_core::tess::triangulate_2d;

use super::{Ctx, R, out_pt, pts};

pub fn dispatch(_c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    Some(match op {
        "geom.tesselate" => tesselate(a),
        _ => return None,
    })
}

/// `Geom.tesselate(outer, loop,...)` : SketchUp runs the GLU tesselator with
/// the positive winding rule (`GLU_TESS_WINDING_POSITIVE`) and no normal. As
/// libtess does then, the normal is found from the points (`gl_normal`), the points are
/// projected along its largest axis (coincident ones merge), the orientation is the one of the
/// larger area, and a region is filled when its winding is positive: a loop turning like the
/// one around it is filled, one turning the other way is a hole. Nothing comes of degenerate
/// loops. Points arrive in inches (`pts` makes millimetres: equal coordinates stay equal).
fn tesselate(a: &J) -> R {
    let loops: Vec<Vec<DVec3>> =
        a["loops"].as_array().ok_or("boucles attendues")?.iter().map(pts).collect::<Result<_, _>>()?;
    let all: Vec<DVec3> = loops.iter().flatten().copied().collect();
    let norm = gl_normal(&all);
    let axis = long_axis(norm);
    let (si, ti) = ((axis + 1) % 3, (axis + 2) % 3);
    let tsign = if norm[axis] > 0.0 { 1.0 } else { -1.0 };
    struct Loop {
        pts: Vec<DVec2>,
        idx: Vec<usize>,
        area: f64,
    }
    let mut ls: Vec<Loop> = Vec::new();
    let mut base = 0;
    for l in &loops {
        let (mut p2, mut idx) = (Vec::new(), Vec::new());
        for (k, p) in l.iter().enumerate() {
            let q = DVec2::new(p[si], tsign * p[ti]);
            if p2.last() != Some(&q) {
                p2.push(q);
                idx.push(base + k);
            }
        }
        while p2.len() > 1 && p2.first() == p2.last() {
            p2.pop();
            idx.pop();
        }
        base += l.len();
        let area = signed_area(&p2);
        if p2.len() >= 3 && area != 0.0 {
            ls.push(Loop { pts: p2, idx, area });
        }
    }
    // libtess turns the projection so that the contours' areas add up to a positive one.
    let flip = ls.iter().map(|l| l.area).sum::<f64>() < 0.0;
    let turn = |l: &Loop| if (l.area > 0.0) != flip { 1i32 } else { -1 };
    // Each loop lies in the smallest loop around it; the winding just inside it is its own turn
    // plus the winding of the loop around it.
    let parent: Vec<Option<usize>> = (0..ls.len())
        .map(|i| {
            (0..ls.len())
                .filter(|&j| j != i && ls[j].area.abs() > ls[i].area.abs() && inside(&ls[j].pts, ls[i].pts[0]))
                .min_by(|&x, &y| ls[x].area.abs().total_cmp(&ls[y].area.abs()))
        })
        .collect();
    fn winding(i: usize, parent: &[Option<usize>], turn: &dyn Fn(usize) -> i32, depth: usize) -> i32 {
        let up = match parent[i] {
            Some(p) if depth < 256 => winding(p, parent, turn, depth + 1),
            _ => 0,
        };
        turn(i) + up
    }
    let turn_of = |i: usize| turn(&ls[i]);
    let mut out = Vec::new();
    for i in 0..ls.len() {
        if winding(i, &parent, &turn_of, 0) <= 0 {
            continue;
        }
        let children: Vec<usize> = (0..ls.len()).filter(|&j| parent[j] == Some(i)).collect();
        let holes: Vec<Vec<DVec2>> = children.iter().map(|&j| ls[j].pts.clone()).collect();
        let idx: Vec<usize> =
            ls[i].idx.iter().chain(children.iter().flat_map(|&j| ls[j].idx.iter())).copied().collect();
        let flat: Vec<DVec2> = ls[i].pts.iter().chain(holes.iter().flatten()).copied().collect();
        for [x, y, z] in conforming(&flat, triangulate_2d(&ls[i].pts, &holes)) {
            // Counter-clockwise in the turned projection, as libtess gives its triangles.
            let ccw = (flat[y] - flat[x]).perp_dot(flat[z] - flat[x]) > 0.0;
            let (p, q, r) = if ccw != flip { (x, y, z) } else { (x, z, y) };
            out.push(json!([out_pt(all[idx[p]]), out_pt(all[idx[q]]), out_pt(all[idx[r]])]));
        }
    }
    Ok(J::Array(out))
}

/// Splits the triangles whose side passes through another point of the region (a hole's
/// point on the line of a bridge): libtess keeps every point a vertex of its triangles, so a
/// region of n points and h holes always gives n + 2h − 2 triangles (TestUp
/// test_tesselate_concave_polygon_with_hole_array_points). At most one split per point.
fn conforming(p: &[DVec2], tris: Vec<[usize; 3]>) -> Vec<[usize; 3]> {
    let on_side = |a: usize, b: usize, k: usize| {
        let (pa, pb, pk) = (p[a], p[b], p[k]);
        if pk == pa || pk == pb {
            return false;
        }
        let d = pb - pa;
        let l2 = d.length_squared();
        let t = (pk - pa).dot(d);
        l2 > 0.0 && (pk - pa).perp_dot(d).abs() <= 1e-12 * l2 && t > 0.0 && t < l2
    };
    let mut todo = tris;
    let mut out = Vec::with_capacity(todo.len());
    let mut budget = p.len();
    while let Some([a, b, c]) = todo.pop() {
        let split = (0..p.len()).find_map(|k| {
            [(a, b, c), (b, c, a), (c, a, b)].into_iter().find(|&(u, v, _)| on_side(u, v, k)).map(|s| (k, s))
        });
        match split {
            Some((k, (u, v, w))) if budget > 0 => {
                budget -= 1;
                todo.push([u, k, w]);
                todo.push([k, v, w]);
            }
            _ => out.push([a, b, c]),
        }
    }
    out
}

/// libtess's `ComputeNormal`: the two points farthest apart along the axis of the largest
/// extent, and the third that makes the largest triangle with them; along that axis's line
/// when all lie on one, z when all are the same point.
fn gl_normal(v: &[DVec3]) -> DVec3 {
    let (mut lo, mut hi) = ([2e150f64; 3], [-2e150f64; 3]);
    let (mut lo_v, mut hi_v) = ([0usize; 3], [0usize; 3]);
    for (k, p) in v.iter().enumerate() {
        for i in 0..3 {
            if p[i] < lo[i] {
                lo[i] = p[i];
                lo_v[i] = k;
            }
            if p[i] > hi[i] {
                hi[i] = p[i];
                hi_v[i] = k;
            }
        }
    }
    let mut i = 0;
    if hi[1] - lo[1] > hi[0] - lo[0] {
        i = 1;
    }
    if hi[2] - lo[2] > hi[i] - lo[i] {
        i = 2;
    }
    if v.is_empty() || lo[i] >= hi[i] {
        return DVec3::Z;
    }
    let (v1, v2) = (v[lo_v[i]], v[hi_v[i]]);
    let d1 = v1 - v2;
    let mut best = 0.0;
    let mut norm = DVec3::ZERO;
    for p in v {
        let d2 = *p - v2;
        let t = DVec3::new(d1.y * d2.z - d1.z * d2.y, d1.z * d2.x - d1.x * d2.z, d1.x * d2.y - d1.y * d2.x);
        let l2 = t.x * t.x + t.y * t.y + t.z * t.z;
        if l2 > best {
            best = l2;
            norm = t;
        }
    }
    if best <= 0.0 {
        let mut n = DVec3::ZERO;
        n[long_axis(d1)] = 1.0;
        return n;
    }
    norm
}

/// libtess's `LongAxis`: the first axis of the largest component.
fn long_axis(v: DVec3) -> usize {
    let mut i = 0;
    if v.y.abs() > v.x.abs() {
        i = 1;
    }
    if v.z.abs() > v[i].abs() {
        i = 2;
    }
    i
}

/// The area of a loop, positive counter-clockwise (libtess's sum (s0 − s1)(t0 + t1), halved).
fn signed_area(p: &[DVec2]) -> f64 {
    (0..p.len()).map(|k| (p[k].x - p[(k + 1) % p.len()].x) * (p[k].y + p[(k + 1) % p.len()].y)).sum::<f64>() * 0.5
}

/// Is the point inside the loop (crossing number)?
fn inside(l: &[DVec2], q: DVec2) -> bool {
    let mut c = false;
    for k in 0..l.len() {
        let (a, b) = (l[k], l[(k + 1) % l.len()]);
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) * (b.x - a.x) / (b.y - a.y) {
            c = !c;
        }
    }
    c
}
#[cfg(test)]
mod tests {
    use super::*;

    /// A point on a triangle's side splits it: every point of the region is a vertex.
    #[test]
    fn a_point_on_a_side_splits_the_triangle() {
        let p = [DVec2::new(0.0, 0.0), DVec2::new(2.0, 0.0), DVec2::new(1.0, 1.0), DVec2::new(1.0, 0.0)];
        let mut t = conforming(&p, vec![[0, 1, 2]]);
        t.sort();
        assert_eq!(t, vec![[0, 3, 2], [3, 1, 2]]);
        assert_eq!(conforming(&p[..3], vec![[0, 1, 2]]), vec![[0, 1, 2]]);
    }
}
