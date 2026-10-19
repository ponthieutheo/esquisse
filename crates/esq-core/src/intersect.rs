//! Intersect Faces: draws the edges where faces cross each other, so that the pieces can
//! then be selected, erased or pushed separately.

use glam::{DMat4, DVec3};

use crate::math::{Plane, TOL};
use crate::model::{DefId, Entities, EntityRef, Model};

/// A face as plain polygons in the active context's coordinates.
pub(crate) struct Poly {
    pub(crate) plane: Plane,
    pub(crate) loops: Vec<Vec<DVec3>>,
    min: DVec3,
    max: DVec3,
}

impl Poly {
    pub(crate) fn new(ents: &Entities, f: crate::model::FaceId, m: &DMat4) -> Option<Poly> {
        let face = ents.faces.get(f)?;
        let loops: Vec<Vec<DVec3>> =
            face.loops().map(|l| l.iter().map(|&v| m.transform_point3(ents.pos(v))).collect()).collect();
        let normal = m.transform_vector3(face.normal).normalize_or_zero();
        if normal == DVec3::ZERO {
            return None;
        }
        let pts = loops.iter().flatten();
        let min = pts.clone().fold(DVec3::splat(f64::INFINITY), |a, p| a.min(*p));
        let max = pts.fold(DVec3::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
        Some(Poly { plane: Plane::from_point_normal(loops[0][0], normal), loops, min, max })
    }

    fn overlaps(&self, o: &Poly) -> bool {
        (self.min - TOL).cmple(o.max).all() && (o.min - TOL).cmple(self.max).all()
    }

    /// Parts of the line `p + d·t` (d unit, in the polygon's plane) inside the polygon.
    pub(crate) fn intervals(&self, p: DVec3, d: DVec3) -> Vec<(f64, f64)> {
        let q = self.plane.n.cross(d);
        let mut ts = Vec::new();
        for l in &self.loops {
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                let (sa, sb) = ((a - p).dot(q), (b - p).dot(q));
                // Half-open rule: a vertex lying on the line counts on the negative side.
                if (sa > 0.0) != (sb > 0.0) {
                    let x = a + (b - a) * (sa / (sa - sb));
                    ts.push((x - p).dot(d));
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.as_chunks::<2>().0.iter().map(|c| (c[0], c[1])).filter(|(a, b)| b - a > TOL).collect()
    }
}

/// Where two planes meet: a point and a unit direction.
pub(crate) fn plane_line(a: &Plane, b: &Plane) -> Option<(DVec3, DVec3)> {
    let d = a.n.cross(b.n);
    let len2 = d.length_squared();
    if len2 < 1e-12 {
        return None;
    }
    let p = (b.n.cross(d) * a.d + d.cross(a.n) * b.d) / len2;
    Some((p, d / len2.sqrt()))
}

/// Segments where two faces cross.
pub(crate) fn crossing(a: &Poly, b: &Poly) -> Vec<(DVec3, DVec3)> {
    if !a.overlaps(b) {
        return Vec::new();
    }
    let Some((p, d)) = plane_line(&a.plane, &b.plane) else { return Vec::new() };
    let (ia, ib) = (a.intervals(p, d), b.intervals(p, d));
    let mut out = Vec::new();
    for &(a0, a1) in &ia {
        for &(b0, b1) in &ib {
            let (t0, t1) = (a0.max(b0), a1.min(b1));
            if t1 - t0 > TOL {
                out.push((p + d * t0, p + d * t1));
            }
        }
    }
    out
}

/// The switches of `Entities#intersect_with`: enter the groups and components of the
/// collection, and use its hidden geometry (hidden, or on a hidden tag).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IntersectOptions {
    pub recurse: bool,
    pub hidden: bool,
}

/// Which faces to intersect with the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntersectWith {
    /// Everything in the active context, groups and components included.
    Model,
    /// Only the other selected entities.
    Selection,
    /// The other faces of the active context, without entering groups.
    Context,
}

impl Model {
    /// Faces of a collection (and, with `deep`, of the groups/components it contains),
    /// in the collection's coordinates. Each face also gets the index of the selected item
    /// it belongs to, so that faces of the same item are not intersected together twice.
    fn collect_polys(&self, ents: &Entities, items: &[EntityRef], deep: bool, out: &mut Vec<(Poly, usize)>) {
        fn rec(model: &Model, ents: &Entities, m: &DMat4, tag: usize, depth: usize, out: &mut Vec<(Poly, usize)>) {
            for f in ents.faces.keys() {
                out.extend(Poly::new(ents, f, m).map(|p| (p, tag)));
            }
            if depth > 32 {
                return;
            }
            for inst in ents.instances.values() {
                if let Some(def) = model.defs.get(inst.def) {
                    rec(model, &def.entities, &(*m * inst.xform), tag, depth + 1, out);
                }
            }
        }
        for (tag, it) in items.iter().enumerate() {
            match *it {
                EntityRef::Face(f) => out.extend(Poly::new(ents, f, &DMat4::IDENTITY).map(|p| (p, tag))),
                EntityRef::Instance(i) if deep => {
                    if let Some(inst) = ents.instances.get(i)
                        && let Some(def) = self.defs.get(inst.def)
                    {
                        rec(self, &def.entities, &inst.xform, tag, 1, out);
                    }
                }
                _ => {}
            }
        }
    }

    /// Intersect Faces: draws, in the active context, the edges where the selected faces
    /// (and the faces of the selected groups/components) cross the faces chosen by `with`.
    /// Returns the number of segments drawn.
    pub fn intersect_faces(&mut self, ctx: Option<DefId>, items: &[EntityRef], with: IntersectWith) -> usize {
        let ents = self.entities(ctx);
        let mut sel = Vec::new();
        self.collect_polys(ents, items, true, &mut sel);
        let mut others = Vec::new();
        if with != IntersectWith::Selection {
            let all: Vec<EntityRef> = ents
                .faces
                .keys()
                .map(EntityRef::Face)
                .chain(ents.instances.keys().map(EntityRef::Instance))
                .filter(|e| !items.contains(e))
                .collect();
            self.collect_polys(ents, &all, with == IntersectWith::Model, &mut others);
        }
        let mut segs: Vec<(DVec3, DVec3)> = Vec::new();
        for (i, (a, ta)) in sel.iter().enumerate() {
            for (b, tb) in &sel[i + 1..] {
                if ta != tb {
                    segs.extend(crossing(a, b));
                }
            }
            for (b, _) in &others {
                segs.extend(crossing(a, b));
            }
        }
        let n = segs.len();
        let ents = self.entities_mut(ctx);
        for (a, b) in segs {
            ents.add_line(a, b);
        }
        n
    }

    /// The faces of a collection that `intersect_with` crosses, placed by `m`: without
    /// `hidden`, hidden faces and groups and those on hidden tags are left out; with `recurse`,
    /// the faces of the groups and components too (SketchUp's).
    fn own_polys(
        &self,
        ents: &Entities,
        m: &DMat4,
        opts: IntersectOptions,
        depth: usize,
        out: &mut Vec<(Poly, usize)>,
    ) {
        let shown =
            |hidden: bool, layer: usize| opts.hidden || (!hidden && self.layers.get(layer).is_none_or(|l| l.visible));
        for (f, face) in ents.faces.iter() {
            if shown(face.hidden, face.layer) {
                out.extend(Poly::new(ents, f, m).map(|p| (p, 0)));
            }
        }
        if !opts.recurse || depth > 32 {
            return;
        }
        for inst in ents.instances.values().filter(|i| shown(i.hidden, i.layer)) {
            if let Some(def) = self.defs.get(inst.def) {
                self.own_polys(&def.entities, &(*m * inst.xform), opts, depth + 1, out);
            }
        }
    }

    /// Ruby's `Entities#intersect_with(recurse, t1, dest, t2, hidden, others)` (SketchUp's):
    /// the faces of collection `src`, placed by `t_src` (and, with `recurse`,
    /// those of its groups and components), are crossed with the faces of `others` (each an
    /// entity of its own collection, taken in that collection's coordinates, groups and
    /// components with their contents); the segments are drawn into collection `dest`, which
    /// `t_dest` places. Returns the edges that appeared in `dest`.
    ///
    /// SketchUp walks the groups with the same intersector (SketchUp calls itself with the
    /// composed transformation): the cuts of nested faces also go into `dest`, not into the
    /// groups that hold them.
    pub fn intersect_with(
        &mut self,
        src: Option<DefId>,
        t_src: &DMat4,
        opts: IntersectOptions,
        others: &[(Option<DefId>, EntityRef)],
        dest: Option<DefId>,
        t_dest: &DMat4,
    ) -> Vec<crate::model::EdgeId> {
        let ents = self.entities(src);
        let mut mine = Vec::new();
        self.own_polys(ents, &DMat4::IDENTITY, opts, 0, &mut mine);
        for (p, _) in &mut mine {
            *p = p.transformed(t_src);
        }
        let mut theirs = Vec::new();
        for (ctx, e) in others {
            if let Some(c) = ctx.map_or(Some(&self.root), |d| self.defs.get(d).map(|d| &d.entities)) {
                self.collect_polys(c, &[*e], true, &mut theirs);
            }
        }
        let back = t_dest.inverse();
        let mut segs = Vec::new();
        for (a, _) in &mine {
            for (b, _) in &theirs {
                segs.extend(
                    crossing(a, b).into_iter().map(|(p, q)| (back.transform_point3(p), back.transform_point3(q))),
                );
            }
        }
        let before: std::collections::HashSet<_> = self.entities(dest).edges.keys().collect();
        let out = self.entities_mut(dest);
        for &(a, b) in &segs {
            out.add_line(a, b);
        }
        // The new edges along the cuts (not the halves of the edges the cuts split).
        let on_cut = |p: DVec3| segs.iter().any(|&(a, b)| crate::math::project_on_segment(p, a, b).1 <= TOL);
        out.edges
            .keys()
            .filter(|k| !before.contains(k))
            .filter(|&k| {
                let (p, q) = out.edge_points(k);
                on_cut(p) && on_cut(q)
            })
            .collect()
    }
}

impl Poly {
    /// The same polygon placed by `m`.
    fn transformed(&self, m: &DMat4) -> Poly {
        let loops: Vec<Vec<DVec3>> =
            self.loops.iter().map(|l| l.iter().map(|p| m.transform_point3(*p)).collect()).collect();
        let normal = m.inverse().transpose().transform_vector3(self.plane.n).normalize_or_zero();
        let pts = loops.iter().flatten();
        let min = pts.clone().fold(DVec3::splat(f64::INFINITY), |a, p| a.min(*p));
        let max = pts.fold(DVec3::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
        Poly { plane: Plane::from_point_normal(loops[0][0], normal), loops, min, max }
    }
}
