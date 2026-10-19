//! Where SketchUp places the point an annotation (text, dimension) is attached to, from what
//! the attach point names (`docs/skp-format.md`, §3.4 « Point d'attache »): SketchUp keeps the
//! entity and the path of instances to it, and works the point out when the model is loaded
//! (in SketchUp.exe). The point written in the file is only used for some types.
//! Both readers (MFC archives, 2021+ records) give their entities through [`Scene`].

/// A point an annotation is attached to, as read: its type, the point of the file, the entity
/// and the path of instances to it (outermost first), and a second entity and path (the two
/// entities of an intersection).
pub(crate) struct Attach<K> {
    pub kind: i32,
    pub point: [f64; 3],
    pub leaf: Option<K>,
    pub path: Vec<K>,
    pub leaf2: Option<K>,
    pub path2: Vec<K>,
}

impl<K: Copy> Attach<K> {
    /// What the model keeps of it (`doc::Connection`): not an intersection (two entities),
    /// nor a point not attached (types 0 and 1).
    pub fn connection(&self, id: impl Fn(K) -> super::doc::Id) -> Option<super::doc::Connection> {
        (self.kind > 1 && self.kind != 9).then(|| super::doc::Connection {
            kind: self.kind,
            point: self.point,
            leaf: self.leaf.map(&id),
            path: self.path.iter().map(|k| id(*k)).collect(),
        })
    }
}

/// What an attach point can name, in the coordinates of its own collection.
pub(crate) enum Leaf {
    Vertex([f64; 3]),
    /// Its start and end vertices.
    Edge([f64; 3], [f64; 3]),
    GuidePoint([f64; 3]),
    /// A point and the direction, as SketchUp keeps the line.
    GuideLine([f64; 3], [f64; 3]),
    /// Its plane (a, b, c, d) and the start and end vertex of each use of its outer loop.
    Face([f64; 4], Vec<([f64; 3], [f64; 3])>),
    /// The centre of an arc.
    Arc([f64; 3]),
    Section([f64; 4]),
    /// An image: its transformation and its size (inches: one tile of its texture).
    Image([f64; 16], [f64; 2]),
    Other,
}

/// The entities and instances of a model, by the keys its attach points use.
pub(crate) trait Scene {
    type Key;
    fn leaf(&self, k: &Self::Key) -> Leaf;
    /// The transformation of an instance (column-major 4×4, as `SUTransformation`).
    fn xform(&self, k: &Self::Key) -> Option<[f64; 16]>;
}

pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub(crate) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let l = dot(a, a).sqrt();
    (l > 0.0 && l.is_finite()).then(|| a.map(|x| x / l))
}

const IDENTITY: [f64; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// `a · b`, column-major 4×4.
fn mat_mul(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    std::array::from_fn(|i| {
        let (c, r) = (i / 4, i % 4);
        (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum()
    })
}

/// A point moved by a homogeneous transformation (divided by its w).
fn transform_point(m: &[f64; 16], p: [f64; 3]) -> [f64; 3] {
    let w = m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15];
    let w = if w != 0.0 && w.is_finite() { w } else { 1.0 };
    std::array::from_fn(|r| (m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r]) / w)
}

fn transform_vector(m: &[f64; 16], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|r| m[r] * v[0] + m[4 + r] * v[1] + m[8 + r] * v[2])
}

/// The transformation of a path of instances: the product of theirs, the
/// outermost first; it stops at the first key that is no instance.
fn path_xform<S: Scene>(s: &S, path: &[S::Key]) -> [f64; 16] {
    let mut t = IDENTITY;
    for k in path {
        match s.xform(k) {
            Some(x) => t = mat_mul(&t, &x),
            None => break,
        }
    }
    t
}

/// The middle of the closest points of two lines, when they are within
/// 0.001" of each other; none for parallel lines.
fn closest_middle(l1: ([f64; 3], [f64; 3]), l2: ([f64; 3], [f64; 3])) -> Option<[f64; 3]> {
    let ((o1, d1), (o2, d2)) = (l1, l2);
    let w = sub(o1, o2);
    let (a, b, c, d, e) = (dot(d1, d1), dot(d1, d2), dot(d2, d2), dot(d1, w), dot(d2, w));
    let den = a * c - b * b;
    if den.abs() < 1e-12 {
        return None;
    }
    let (s, t) = ((b * e - c * d) / den, (a * e - b * d) / den);
    let p: [f64; 3] = std::array::from_fn(|i| o1[i] + s * d1[i]);
    let q: [f64; 3] = std::array::from_fn(|i| o2[i] + t * d2[i]);
    let gap = sub(p, q);
    if gap.iter().any(|g| g.abs() >= 1e-3) || dot(gap, gap) >= 1e-6 {
        return None;
    }
    Some(std::array::from_fn(|i| (p[i] + q[i]) * 0.5))
}

/// Where a line meets a plane (a point of it, its normal); none when parallel.
fn line_plane(l: ([f64; 3], [f64; 3]), pl: ([f64; 3], [f64; 3])) -> Option<[f64; 3]> {
    let ((o, d), (q, n)) = (l, pl);
    let den = dot(n, d);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = dot(n, sub(q, o)) / den;
    Some(std::array::from_fn(|i| o[i] + t * d[i]))
}

/// The point of a face at the fractions `u`, `v` of its outer loop's extent, in the frame of
/// the face's first edge use (origin at its start, x along it, z the face's
/// normal), measured from that origin.
fn face_point(plane: [f64; 4], uses: &[([f64; 3], [f64; 3])], u: f64, v: f64) -> Option<[f64; 3]> {
    let n = [plane[0], plane[1], plane[2]];
    let &(o, end) = uses.first()?;
    let xa = unit(sub(end, o))?;
    let ya = unit(cross(n, xa))?;
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for &(s, _) in uses {
        let d = sub(s, o);
        for (k, axis) in [xa, ya].into_iter().enumerate() {
            let c = dot(d, axis);
            lo[k] = lo[k].min(c);
            hi[k] = hi[k].max(c);
        }
    }
    let extent = |k: usize| if hi[k] > lo[k] { hi[k] - lo[k] } else { 0.0 };
    let (a, b) = (u * extent(0), v * extent(1));
    Some(std::array::from_fn(|i| o[i] + a * xa[i] + b * ya[i]))
}

/// Where an attach point is, by its type: 1 the point itself; 2 a vertex; 3 a
/// guide point; 4 the middle of an edge, 5 a point of an edge (at the fraction x of the point);
/// 6 a point of a guide line (at x); 7 a point of a face (the fractions x, y of its outer
/// loop's extent, see [`face_point`]); 8 a point of an image (the fractions x, y of its size,
/// through its transformation); 10 the centre of an arc; 11 the point projected on a
/// section plane; 9 the intersection of two lines (edges, guide lines: the middle of their
/// closest points, within 0.001") or of a line and a plane (face, section plane), each moved by
/// its path; 12 and 13 the point itself, or with a path the origin of its instances. The others
/// (0): the origin. Except for 9, 12 and 13, the result is then moved by the path.
pub(crate) fn position<S: Scene>(s: &S, a: &Attach<S::Key>) -> [f64; 3] {
    let [x, y, _] = a.point;
    let leaf = a.leaf.as_ref().map_or(Leaf::Other, |k| s.leaf(k));
    let mut p = [0.0; 3];
    match (a.kind, leaf) {
        (1, _) => p = a.point,
        (2, Leaf::Vertex(v)) | (3, Leaf::GuidePoint(v)) | (10, Leaf::Arc(v)) => p = v,
        (4, Leaf::Edge(s, e)) => p = std::array::from_fn(|i| s[i] * 0.5 + e[i] * 0.5),
        (5, Leaf::Edge(s, e)) => p = std::array::from_fn(|i| x * e[i] + (1.0 - x) * s[i]),
        (6, Leaf::GuideLine(o, d)) => p = std::array::from_fn(|i| o[i] + x * d[i]),
        (7, Leaf::Face(plane, uses)) => p = face_point(plane, &uses, x, y).unwrap_or(p),
        // 8: a point of an image, at the fractions x, y of its size.
        (8, Leaf::Image(t, [w, h])) => p = transform_point(&t, [x * w, y * h, 0.0]),
        (11, Leaf::Section(pl)) => {
            let n = [pl[0], pl[1], pl[2]];
            let off = dot(n, a.point) + pl[3];
            p = std::array::from_fn(|i| a.point[i] - off * n[i]);
        }
        (9, _) => return intersection(s, a).unwrap_or([0.0; 3]),
        (12 | 13, _) => {
            return if a.path.is_empty() { a.point } else { transform_point(&path_xform(s, &a.path), [0.0; 3]) };
        }
        _ => {}
    }
    if a.path.is_empty() { p } else { transform_point(&path_xform(s, &a.path), p) }
}

/// The intersection of the two entities of an attach point (type 9).
fn intersection<S: Scene>(s: &S, a: &Attach<S::Key>) -> Option<[f64; 3]> {
    let (t1, t2) = (path_xform(s, &a.path), path_xform(s, &a.path2));
    let leaf = |k: &Option<S::Key>| k.as_ref().map_or(Leaf::Other, |k| s.leaf(k));
    let (l1, l2) = (leaf(&a.leaf), leaf(&a.leaf2));
    let line = |l: &Leaf, t: &[f64; 16]| -> Option<([f64; 3], [f64; 3])> {
        let (o, d) = match l {
            Leaf::Edge(s, e) => (*s, unit(sub(*e, *s))?),
            Leaf::GuideLine(o, d) => (*o, *d),
            _ => return None,
        };
        Some((transform_point(t, o), unit(transform_vector(t, d))?))
    };
    let plane = |l: &Leaf, t: &[f64; 16]| -> Option<([f64; 3], [f64; 3])> {
        let pl = match l {
            Leaf::Face(pl, _) | Leaf::Section(pl) => *pl,
            _ => return None,
        };
        let n = [pl[0], pl[1], pl[2]];
        Some((transform_point(t, n.map(|x| -x * pl[3])), unit(transform_vector(t, n))?))
    };
    match (line(&l1, &t1), line(&l2, &t2)) {
        (Some(a), Some(b)) => closest_middle(a, b),
        (Some(l), None) => plane(&l2, &t2).and_then(|p| line_plane(l, p)),
        (None, Some(l)) => plane(&l1, &t1).and_then(|p| line_plane(l, p)),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One image, 40 × 20", by its transformation.
    struct Img([f64; 16]);

    impl Scene for Img {
        type Key = u8;
        fn leaf(&self, _: &u8) -> Leaf {
            Leaf::Image(self.0, [40.0, 20.0])
        }
        fn xform(&self, _: &u8) -> Option<[f64; 16]> {
            None
        }
    }

    #[test]
    fn a_point_of_an_image_is_at_fractions_of_its_size() {
        // The image turned a quarter around z and moved to (100, 0, 0).
        let t = [0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 100.0, 0.0, 0.0, 1.0];
        let a = Attach { kind: 8, point: [0.5, 0.25, 0.0], leaf: Some(0), path: vec![], leaf2: None, path2: vec![] };
        assert_eq!(position(&Img(t), &a), [95.0, 20.0, 0.0]);
    }
}
