//! A bounding volume hierarchy over boxes: the picking cache finds what lies under the
//! cursor without looking at every triangle, edge and point of the model (`pick.rs`).
//!
//! The tree only says where to look: the items it hands over are then tested exactly as
//! before, and the callers keep the result the plain scan over all items would give (same
//! nearest item, same order between equal ones).

use esq_core::glam::DVec3;

/// Items per leaf.
const LEAF: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb { min: DVec3::splat(f64::INFINITY), max: DVec3::splat(f64::NEG_INFINITY) };

    pub fn of_points(pts: &[DVec3]) -> Aabb {
        pts.iter().fold(Aabb::EMPTY, |b, p| Aabb { min: b.min.min(*p), max: b.max.max(*p) })
    }

    fn union(self, o: Aabb) -> Aabb {
        Aabb { min: self.min.min(o.min), max: self.max.max(o.max) }
    }

    /// The box grown by a margin that covers the rounding of tests made on its items.
    pub fn padded(self) -> Aabb {
        let m = 1e-6 * (1.0 + self.min.abs().max(self.max.abs()).max_element() + (self.max - self.min).max_element());
        Aabb { min: self.min - DVec3::splat(m), max: self.max + DVec3::splat(m) }
    }

    pub fn corners(&self) -> [DVec3; 8] {
        std::array::from_fn(|i| {
            DVec3::new(
                if i & 1 == 0 { self.min.x } else { self.max.x },
                if i & 2 == 0 { self.min.y } else { self.max.y },
                if i & 4 == 0 { self.min.z } else { self.max.z },
            )
        })
    }

    /// Where the ray `o + t·d` (t ≥ 0) enters and leaves the box, if it crosses it.
    pub fn ray(&self, o: DVec3, d: DVec3) -> Option<(f64, f64)> {
        let (mut t0, mut t1) = (0.0f64, f64::INFINITY);
        for k in 0..3 {
            let (lo, hi, ok, dk) = (self.min[k], self.max[k], o[k], d[k]);
            if dk == 0.0 {
                if ok < lo || ok > hi {
                    return None;
                }
                continue;
            }
            let (a, b) = ((lo - ok) / dk, (hi - ok) / dk);
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            t0 = t0.max(a);
            t1 = t1.min(b);
            // NaN (an infinite box side) keeps the box: the test only prunes.
            if t0 > t1 {
                return None;
            }
        }
        Some((t0, t1))
    }
}

#[derive(Clone, Copy)]
struct Node {
    bounds: Aabb,
    /// Leaf: its items are `items[first..first + count]`; inner node (count 0): its
    /// children are the nodes `first` and `second`.
    first: u32,
    second: u32,
    count: u32,
}

/// Parts of the tree bigger than this are built on threads of their own (the top levels).
const PARALLEL: usize = 65_536;

#[derive(Default)]
pub struct Bvh {
    nodes: Vec<Node>,
    items: Vec<u32>,
}

impl Bvh {
    /// The tree over item boxes (item `i` is `boxes[i]`, already padded by the caller when
    /// its tests need it).
    pub fn build(boxes: &[Aabb]) -> Bvh {
        if boxes.is_empty() {
            return Bvh::default();
        }
        let centre: Vec<DVec3> = boxes.iter().map(|b| (b.min + b.max) * 0.5).collect();
        let mut items: Vec<u32> = (0..boxes.len() as u32).collect();
        let nodes = Self::part(boxes, &centre, &mut items, 0);
        Bvh { nodes, items }
    }

    /// The tree over `items` (which start at `base` in the whole list), its root first.
    /// Split in halves at the median of the longest side of the centres: the depth is the
    /// logarithm of the number of items. The top levels are built side by side.
    fn part(boxes: &[Aabb], centre: &[DVec3], items: &mut [u32], base: usize) -> Vec<Node> {
        let mut nodes = vec![Node { bounds: Aabb::EMPTY, first: base as u32, second: 0, count: items.len() as u32 }];
        let mut stack = vec![0usize];
        while let Some(n) = stack.pop() {
            let (first, count) = (nodes[n].first as usize - base, nodes[n].count as usize);
            let span = &mut items[first..first + count];
            let bounds = span.iter().fold(Aabb::EMPTY, |b, &i| b.union(boxes[i as usize]));
            nodes[n].bounds = bounds;
            if count <= LEAF {
                continue;
            }
            let c = span.iter().fold(Aabb::EMPTY, |b, &i| {
                let p = centre[i as usize];
                b.union(Aabb { min: p, max: p })
            });
            let ext = c.max - c.min;
            let axis = if ext.x >= ext.y && ext.x >= ext.z {
                0
            } else if ext.y >= ext.z {
                1
            } else {
                2
            };
            let mid = count / 2;
            span.select_nth_unstable_by(mid, |a, b| centre[*a as usize][axis].total_cmp(&centre[*b as usize][axis]));
            let (lo, hi) = (base + first, base + first + mid);
            if count > PARALLEL {
                // Both halves side by side, their nodes put after these.
                let (l, r) = span.split_at_mut(mid);
                let (ln, rn) = std::thread::scope(|s| {
                    let right = s.spawn(|| Self::part(boxes, centre, r, hi));
                    (Self::part(boxes, centre, l, lo), right.join().expect("moitié de l'arbre"))
                });
                let (left, right) = (nodes.len(), nodes.len() + ln.len());
                for (part, at) in [(ln, left), (rn, right)] {
                    nodes.extend(part.into_iter().map(|mut x| {
                        if x.count == 0 {
                            x.first += at as u32;
                            x.second += at as u32;
                        }
                        x
                    }));
                }
                nodes[n] = Node { bounds, first: left as u32, second: right as u32, count: 0 };
                continue;
            }
            let left = nodes.len();
            nodes.push(Node { bounds: Aabb::EMPTY, first: lo as u32, second: 0, count: mid as u32 });
            nodes.push(Node { bounds: Aabb::EMPTY, first: hi as u32, second: 0, count: (count - mid) as u32 });
            nodes[n] = Node { bounds, first: left as u32, second: left as u32 + 1, count: 0 };
            stack.push(left);
            stack.push(left + 1);
        }
        nodes
    }

    /// Calls `leaf` for every item of the leaves whose box `enter` accepts (inner nodes
    /// whose box it refuses are skipped with all they contain). Returns how many nodes were
    /// looked at.
    pub fn query(&self, mut enter: impl FnMut(&Aabb) -> bool, mut leaf: impl FnMut(u32)) -> u64 {
        let mut visited = 0;
        if self.nodes.is_empty() {
            return 0;
        }
        let mut stack = vec![0u32];
        while let Some(n) = stack.pop() {
            let node = &self.nodes[n as usize];
            visited += 1;
            if !enter(&node.bounds) {
                continue;
            }
            if node.count > 0 {
                for &i in &self.items[node.first as usize..(node.first + node.count) as usize] {
                    leaf(i);
                }
            } else {
                stack.push(node.second);
                stack.push(node.first);
            }
        }
        visited
    }

    /// Like `query` for a ray: the boxes the ray crosses at a distance up to `best()` (read
    /// again before each box, so that a nearer hit found meanwhile prunes the rest), the
    /// nearer child first. `leaf` gets the items. Returns how many nodes were looked at.
    pub fn ray(&self, o: DVec3, d: DVec3, best: impl Fn() -> f64, mut leaf: impl FnMut(u32)) -> u64 {
        let mut visited = 0;
        if self.nodes.is_empty() {
            return 0;
        }
        let mut stack: Vec<(u32, f64)> = Vec::with_capacity(64);
        if let Some((t0, _)) = self.nodes[0].bounds.ray(o, d) {
            stack.push((0, t0));
        }
        while let Some((n, t0)) = stack.pop() {
            visited += 1;
            if t0 > best() {
                continue;
            }
            let node = &self.nodes[n as usize];
            if node.count > 0 {
                for &i in &self.items[node.first as usize..(node.first + node.count) as usize] {
                    leaf(i);
                }
                continue;
            }
            let a = self.nodes[node.first as usize].bounds.ray(o, d).map(|(t, _)| (node.first, t));
            let b = self.nodes[node.second as usize].bounds.ray(o, d).map(|(t, _)| (node.second, t));
            match (a, b) {
                (Some(a), Some(b)) => {
                    // Farther pushed first: the nearer one is looked at next.
                    let (near, far) = if a.1 <= b.1 { (a, b) } else { (b, a) };
                    stack.push(far);
                    stack.push(near);
                }
                (Some(x), None) | (None, Some(x)) => stack.push(x),
                (None, None) => {}
            }
        }
        visited
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_finds_what_a_full_scan_finds() {
        // Boxes on a jittered grid; a query box anywhere sees exactly the overlapping ones.
        let mut boxes = Vec::new();
        for i in 0..1000 {
            let p = DVec3::new((i % 10) as f64 * 10.0, (i / 10 % 10) as f64 * 10.0, (i / 100) as f64 * 10.0)
                + DVec3::splat((i * 7 % 13) as f64 * 0.3);
            boxes.push(Aabb { min: p, max: p + DVec3::splat(4.0) });
        }
        let bvh = Bvh::build(&boxes);
        let q = Aabb { min: DVec3::new(15.0, 22.0, 31.0), max: DVec3::new(48.0, 40.0, 77.0) };
        let overlaps = |b: &Aabb| (b.min.cmple(q.max) & b.max.cmpge(q.min)).all();
        let mut found = Vec::new();
        let visited = bvh.query(overlaps, |i| {
            if overlaps(&boxes[i as usize]) {
                found.push(i)
            }
        });
        found.sort();
        let all: Vec<u32> = (0..1000).filter(|&i| overlaps(&boxes[i as usize])).collect();
        assert_eq!(found, all);
        assert!(visited < 600, "{visited} nœuds vus sur {}", 2 * 1000 / LEAF);
    }

    #[test]
    fn a_tree_built_on_several_threads_finds_the_same() {
        // More boxes than one thread builds: the halves of the top levels are built apart.
        let n = 3 * PARALLEL;
        let boxes: Vec<Aabb> = (0..n)
            .map(|i| {
                let p = DVec3::new((i % 97) as f64, (i / 97 % 89) as f64, (i * 31 % 71) as f64);
                Aabb { min: p, max: p + DVec3::splat(0.5) }
            })
            .collect();
        let bvh = Bvh::build(&boxes);
        let mut seen = vec![0u8; n];
        bvh.query(|_| true, |i| seen[i as usize] += 1);
        assert!(seen.iter().all(|&s| s == 1), "chaque élément une fois");
        let (o, d) = (DVec3::new(-5.0, 20.3, 10.2), DVec3::new(1.0, 0.01, 0.02).normalize());
        let mut hit = Vec::new();
        bvh.ray(
            o,
            d,
            || f64::INFINITY,
            |i| {
                if boxes[i as usize].ray(o, d).is_some() {
                    hit.push(i)
                }
            },
        );
        hit.sort();
        let all: Vec<u32> = (0..n as u32).filter(|&i| boxes[i as usize].ray(o, d).is_some()).collect();
        assert!(!all.is_empty());
        assert_eq!(hit, all);
    }
}
