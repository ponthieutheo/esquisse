//! Delaunay triangulation of points in plan (Bowyer-Watson), as the Stamp tool uses it to
//! fill the ring between the footprint and the cut in the terrain.

use glam::DVec2;

#[derive(Clone, Copy)]
struct Tri {
    v: [usize; 3],
    centre: DVec2,
    r2: f64,
}

/// Circumscribed circle of three points; `None` when they are aligned.
fn circumcircle(a: DVec2, b: DVec2, c: DVec2) -> Option<(DVec2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    let scale = (a - b).length_squared().max((b - c).length_squared()).max((c - a).length_squared());
    if d.abs() <= 1e-12 * scale.max(1e-300) {
        return None;
    }
    let (a2, b2, c2) = (a.length_squared(), b.length_squared(), c.length_squared());
    let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let centre = DVec2::new(ux, uy);
    Some((centre, centre.distance_squared(a)))
}

/// Triangles (indices into `points`, counter-clockwise) of the Delaunay triangulation.
/// Points on the circle of a triangle count as inside it, like in the original tool.
pub fn triangulate(points: &[DVec2]) -> Vec<[usize; 3]> {
    let n = points.len();
    if n < 3 {
        return Vec::new();
    }
    let (mut min, mut max) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in points {
        min = min.min(*p);
        max = max.max(*p);
    }
    let dmax = (max - min).max_element().max(1e-6);
    let mid = (min + max) * 0.5;
    let mut pts: Vec<DVec2> = points.to_vec();
    // The big triangle that contains everything; the further it is, the fewer triangles
    // of the hull are lost.
    let k = 50.0 * dmax;
    pts.push(DVec2::new(mid.x - 2.0 * k, mid.y - k));
    pts.push(DVec2::new(mid.x, mid.y + 2.0 * k));
    pts.push(DVec2::new(mid.x + 2.0 * k, mid.y - k));
    let make = |v: [usize; 3], pts: &[DVec2]| {
        circumcircle(pts[v[0]], pts[v[1]], pts[v[2]]).map(|(centre, r2)| Tri { v, centre, r2 })
    };
    let mut tris: Vec<Tri> = make([n, n + 1, n + 2], &pts).into_iter().collect();
    for pi in 0..n {
        let p = pts[pi];
        let mut edges: Vec<(usize, usize)> = Vec::new();
        let mut keep: Vec<Tri> = Vec::with_capacity(tris.len());
        for t in &tris {
            if t.centre.distance_squared(p) <= t.r2 * (1.0 + 1e-12) {
                edges.extend([(t.v[0], t.v[1]), (t.v[1], t.v[2]), (t.v[2], t.v[0])]);
            } else {
                keep.push(*t);
            }
        }
        // The boundary of the hole: the edges that no other bad triangle shares.
        for (e, &(a, b)) in edges.iter().enumerate() {
            if edges.iter().enumerate().any(|(f, &(c, d))| f != e && a == d && b == c) {
                continue;
            }
            if let Some(t) = make([a, b, pi], &pts) {
                keep.push(t);
            }
        }
        tris = keep;
    }
    tris.into_iter()
        .filter(|t| t.v.iter().all(|&i| i < n))
        .map(|t| {
            let [a, b, c] = t.v;
            if (pts[b] - pts[a]).perp_dot(pts[c] - pts[a]) >= 0.0 { [a, b, c] } else { [a, c, b] }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(p: &[DVec2], t: [usize; 3]) -> f64 {
        (p[t[1]] - p[t[0]]).perp_dot(p[t[2]] - p[t[0]]) * 0.5
    }

    #[test]
    fn a_square_gives_two_triangles() {
        let p = [DVec2::new(0.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(10.0, 10.0), DVec2::new(0.0, 10.0)];
        let t = triangulate(&p);
        assert_eq!(t.len(), 2);
        let total: f64 = t.iter().map(|t| area(&p, *t)).sum();
        assert!((total - 100.0).abs() < 1e-9);
        assert!(t.iter().all(|t| area(&p, *t) > 0.0), "counter-clockwise");
    }

    #[test]
    fn the_hull_is_covered() {
        // A square with a point in the middle and some on the edges.
        let mut p = vec![DVec2::new(0.0, 0.0), DVec2::new(40.0, 0.0), DVec2::new(40.0, 40.0), DVec2::new(0.0, 40.0)];
        p.extend([DVec2::new(20.0, 20.0), DVec2::new(20.0, 0.0), DVec2::new(40.0, 13.0), DVec2::new(7.0, 40.0)]);
        let t = triangulate(&p);
        let total: f64 = t.iter().map(|t| area(&p, *t)).sum();
        assert!((total - 1600.0).abs() < 1e-6, "{total}");
        // Euler: for a triangulated polygon with b boundary points and i inner points,
        // there are b + 2i - 2 triangles.
        assert_eq!(t.len(), 7 + 2 - 2);
    }

    #[test]
    fn delaunay_property_holds() {
        let p: Vec<DVec2> = (0..30)
            .map(|i| {
                let a = i as f64 * 2.399963;
                DVec2::new(a.cos() * (1.0 + i as f64), a.sin() * (1.0 + i as f64))
            })
            .collect();
        let t = triangulate(&p);
        for tri in &t {
            let (c, r2) = circumcircle(p[tri[0]], p[tri[1]], p[tri[2]]).unwrap();
            for (i, q) in p.iter().enumerate() {
                if !tri.contains(&i) {
                    assert!(c.distance_squared(*q) >= r2 * (1.0 - 1e-9), "point {i} inside a circumcircle");
                }
            }
        }
    }
}
