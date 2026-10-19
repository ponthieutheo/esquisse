//! Associativity of dimensions and texts: an end of a dimension, or the point a text's leader
//! points to, can be attached to something of the model, and follows it.
//!
//! This is SketchUp's connection point (`docs/skp-format.md` §2.3.7):
//! a type, the entity it names, the path of instances that leads to it from the annotation's
//! collection, and a parameter (a fraction along an edge, of a face's extent…). SketchUp works
//! the point out from them whenever it is asked (in SketchUp.exe) and hears of
//! every change of the entities through observers (erased, replaced, split,
//! merged). Esquisse keeps the last point it worked out in the annotation itself (what the
//! view, the exports and the files read) and puts it in step after each change
//! ([`Model::sync_annotations`]):
//!
//! - the point is worked out again from what it names, through the path's transformations;
//! - what an operation replaced is followed ([`Replaced`], noted by the operations themselves:
//!   a vertex merged into another, an edge split or joined, faces rebuilt or pushed);
//! - when what it names, or an instance of its path, is gone, the end is detached and stays
//!   at its last point (type 1, the cached point);
//! - a linear dimension whose two ends come together is erased.
//!
//! Only the collections that hold attached annotations are visited ([`Index`]): the
//! geometry is never walked, and each end costs one lookup per instance of its path.

use std::collections::HashMap;

use glam::{DMat4, DVec2, DVec3};
use serde::{Deserialize, Serialize};

use crate::math::{TOL, point_in_polygon_2d, project_on_segment};
use crate::model::{
    AnnotId, AnnotKind, Annotation, DefId, EdgeId, Entities, FaceId, GuideId, InstanceId, Model, SectionId, VertexId,
};

/// What an end of an annotation is attached to, in the collection its path leads to. The
/// numbers are SketchUp's types of connection point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AttachTo {
    /// 2: a vertex.
    Vertex(VertexId),
    /// 3: a construction point.
    GuidePoint(GuideId),
    /// 4: the middle of an edge.
    EdgeMiddle(EdgeId),
    /// 5: the point at fraction `t` from the edge's first vertex to its second.
    OnEdge(EdgeId, f64),
    /// 6: the point `d` millimetres from a construction line's point, along its direction.
    OnGuide(GuideId, f64),
    /// 7: the point at fractions `u`, `v` of a face's extent, in the face's frame
    /// ([`face_frame`]).
    OnFace(FaceId, f64, f64),
    /// 10: the centre of the arc an edge belongs to.
    ArcCenter(EdgeId),
    /// The point at an angle (radians from its first point) of the arc an edge belongs to:
    /// the end of a radial dimension (SketchUp keeps the arc and the angle,
    /// `CDimensionRadial`).
    OnArc(EdgeId, f64),
    /// 11: a point (of the leaf's collection) projected on a section plane.
    Section(SectionId, DVec3),
    /// 12, 13: the origin of the innermost instance of the path.
    Origin,
}

/// An attached end: the instances from the annotation's collection to the collection of what
/// it is attached to (outermost first), and what it is attached to there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attach {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<InstanceId>,
    pub to: AttachTo,
}

impl Attach {
    pub fn new(path: Vec<InstanceId>, to: AttachTo) -> Attach {
        Attach { path, to }
    }

    /// Attached to something of the annotation's own collection.
    pub fn local(to: AttachTo) -> Attach {
        Attach { path: Vec::new(), to }
    }
}

/// The ends of an annotation that are attached: `[0]` the start of a dimension or the point
/// of a text, `[1]` the end of a dimension.
pub type Ends = [Option<Attach>; 2];

pub fn no_ends(e: &Ends) -> bool {
    e[0].is_none() && e[1].is_none()
}

/// Something an operation replaced, as SketchUp's observers hear of it:
/// the annotations attached to the old entity follow (see [`Model::sync_annotations`]). Kept
/// by the collection for the session only (never saved nor compared), until the next update.
#[derive(Clone, Debug, PartialEq)]
pub enum Replaced {
    /// A vertex merged into another (code 6).
    Vertex(VertexId, VertexId),
    /// An edge that another one replaces. `split`: the first piece of an edge split in two,
    /// which keeps the edge's start as SketchUp's `Edge#split` keeps the edge; an end on it
    /// stays on it at the same fraction or in its middle (code 4 leaves the connections of
    /// edges alone). Otherwise the edge two joined edges became, or the edge rebuilt when a
    /// vertex merged (code 6 moves the connections over): an end at a fraction stays where it
    /// was, brought onto the new edge.
    Edge { old: EdgeId, new: EdgeId, split: bool },
    /// A face rebuilt, split or merged (codes 3, 4, 6): an end on it goes to the face among
    /// `new` (and the old one, if it is still there) that holds its point; `moved` when the
    /// face itself went elsewhere (Push/Pull: only `new` counts).
    Face { old: FaceId, new: Vec<FaceId>, moved: bool },
}

/// Most entries a collection keeps (an operation that replaces more loses the oldest): the notes
/// of a collection no attached end leads to are only emptied there, or by the app after each
/// step (`Model::forget_replaced`).
const MAX_REPLACED: usize = 4096;

/// What an update changes of one annotation: its collection, its id, its ends, their points.
type Update = (Option<DefId>, AnnotId, Ends, [Option<DVec3>; 2]);

impl Entities {
    /// Notes that an operation replaced an entity (see [`Replaced`]).
    pub fn note_replaced(&mut self, r: Replaced) {
        if self.replaced.len() >= MAX_REPLACED {
            self.replaced.drain(..MAX_REPLACED / 2);
        }
        self.replaced.push(r);
    }
}

/// Which collections hold attached annotations, so that [`Model::sync_annotations`] never
/// looks at the others; worked out once (from the annotations alone) when not known. Not
/// saved: a model read from a file works it out at its first update.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Index {
    known: bool,
    colls: Vec<Option<DefId>>,
}

impl Index {
    /// A collection that now holds an attached annotation.
    pub fn note(&mut self, ctx: Option<DefId>) {
        if self.known && !self.colls.contains(&ctx) {
            self.colls.push(ctx);
        }
    }

    /// Forgets what it knew: worked out again at the next update.
    pub fn forget(&mut self) {
        self.known = false;
        self.colls.clear();
    }

    pub fn is_known(&self) -> bool {
        self.known
    }

    pub fn collections(&self) -> &[Option<DefId>] {
        &self.colls
    }
}

/// What an update did (the tests count it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sync {
    /// Collections whose annotations were looked at.
    pub collections: usize,
    /// Annotations looked at.
    pub annotations: usize,
    /// Entities looked up (each instance of a path, each leaf, each edge of an arc).
    pub lookups: usize,
    /// Ends whose point changed.
    pub moved: usize,
    /// Ends detached (what they named is gone).
    pub detached: usize,
    /// Dimensions erased (their two ends came together).
    pub erased: usize,
    /// The index was worked out again (every collection's annotations looked at).
    pub rebuilt: bool,
}

/// The frame of a face in which SketchUp measures a point on it: origin at
/// the start of the first edge of its outer loop, x along that edge, z the face's normal.
/// Returns it with the extent of the outer loop along x and y, from that origin.
pub fn face_frame(ents: &Entities, f: FaceId) -> Option<(DMat4, DVec2)> {
    let face = ents.faces.get(f)?;
    let pts: Vec<DVec3> = face.outer.iter().map(|v| ents.verts.get(*v).map(|x| x.pos)).collect::<Option<_>>()?;
    let (o, next) = (*pts.first()?, *pts.get(1)?);
    let z = face.normal.normalize_or_zero();
    let x = (next - o).normalize_or_zero();
    let y = z.cross(x).normalize_or_zero();
    if x == DVec3::ZERO || y == DVec3::ZERO {
        return None;
    }
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in &pts {
        let d = *p - o;
        let q = DVec2::new(d.dot(x), d.dot(y));
        lo = lo.min(q);
        hi = hi.max(q);
    }
    let extent = (hi - lo).max(DVec2::ZERO);
    Some((DMat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), o.extend(1.0)), extent))
}

/// The fractions of a face's extent that name `p`: the inverse of
/// [`AttachTo::OnFace`].
pub fn face_fractions(ents: &Entities, f: FaceId, p: DVec3) -> Option<(f64, f64)> {
    let (frame, ext) = face_frame(ents, f)?;
    let l = frame.inverse().transform_point3(p);
    let div = |a: f64, b: f64| if b > 0.0 { a / b } else { 0.0 };
    Some((div(l.x, ext.x), div(l.y, ext.y)))
}

/// The edges of the curve `e` belongs to, found from `e` along its vertices (a curve never
/// branches): as many lookups as the curve has edges, never the whole collection.
fn curve_of(ents: &Entities, e: EdgeId, lookups: &mut usize) -> Option<Vec<EdgeId>> {
    let id = ents.edges.get(e)?.curve?;
    let mut out = vec![e];
    let mut seen = std::collections::HashSet::from([e]);
    let mut todo = vec![e];
    while let Some(x) = todo.pop() {
        for v in ents.edges[x].v {
            for &n in ents.verts.get(v).map(|v| &v.edges[..]).unwrap_or(&[]) {
                *lookups += 1;
                if ents.edges.get(n).is_some_and(|y| y.curve == Some(id)) && seen.insert(n) {
                    out.push(n);
                    todo.push(n);
                }
            }
        }
        if out.len() > 100_000 {
            return None;
        }
    }
    Some(out)
}

/// Where `to` is in its collection.
fn leaf_point(ents: &Entities, to: &AttachTo, lookups: &mut usize) -> Option<DVec3> {
    *lookups += 1;
    let vertex = |v: VertexId| ents.verts.get(v).map(|x| x.pos);
    let edge = |e: EdgeId| -> Option<(DVec3, DVec3)> {
        let x = ents.edges.get(e)?;
        Some((vertex(x.v[0])?, vertex(x.v[1])?))
    };
    match *to {
        AttachTo::Vertex(v) => vertex(v),
        AttachTo::GuidePoint(g) => Some(ents.guides.get(g)?.pos),
        AttachTo::EdgeMiddle(e) => edge(e).map(|(a, b)| (a + b) * 0.5),
        AttachTo::OnEdge(e, t) => edge(e).map(|(a, b)| a + (b - a) * t),
        AttachTo::OnGuide(g, d) => {
            let g = ents.guides.get(g)?;
            Some(g.pos + g.unit_dir()? * d)
        }
        AttachTo::OnFace(f, u, v) => {
            let (frame, ext) = face_frame(ents, f)?;
            Some(frame.transform_point3(DVec3::new(u * ext.x, v * ext.y, 0.0)))
        }
        AttachTo::ArcCenter(e) | AttachTo::OnArc(e, _) => {
            let edges = curve_of(ents, e, lookups)?;
            let fit = crate::curves::arc_fit_edges(ents, &edges)?;
            Some(match *to {
                AttachTo::OnArc(_, angle) => fit.point(angle),
                _ => fit.center,
            })
        }
        AttachTo::Section(s, p) => {
            let s = ents.sections.get(s)?;
            Some(p - s.normal * (p - s.pos).dot(s.normal))
        }
        AttachTo::Origin => Some(DVec3::ZERO),
    }
}

impl Model {
    /// The collection `ctx` names, if it is still there.
    pub fn entities_get(&self, ctx: Option<DefId>) -> Option<&Entities> {
        match ctx {
            Some(d) => self.defs.get(d).map(|d| &d.entities),
            None => Some(&self.root),
        }
    }

    /// The collection the path of `a` leads to from `ctx`, with the path's transformation;
    /// none when an instance of the path is gone (or the path is longer than a model can nest).
    pub fn attach_leaf(&self, ctx: Option<DefId>, a: &Attach) -> Option<(Option<DefId>, &Entities, DMat4)> {
        self.leaf_of(ctx, &a.path, &mut 0)
    }

    fn leaf_of(
        &self,
        ctx: Option<DefId>,
        path: &[InstanceId],
        lookups: &mut usize,
    ) -> Option<(Option<DefId>, &Entities, DMat4)> {
        if path.len() > crate::repair::MAX_DEPTH {
            return None;
        }
        let mut ents = self.entities_get(ctx)?;
        let (mut def, mut m) = (ctx, DMat4::IDENTITY);
        for &i in path {
            *lookups += 1;
            let inst = ents.instances.get(i)?;
            m *= inst.xform;
            ents = &self.defs.get(inst.def)?.entities;
            def = Some(inst.def);
        }
        Some((def, ents, m))
    }

    /// Where an attached end is, in the coordinates of the collection `ctx` that holds the
    /// annotation; none when what it names is gone.
    pub fn attach_point(&self, ctx: Option<DefId>, a: &Attach) -> Option<DVec3> {
        self.attach_point_counted(ctx, a, &mut 0)
    }

    fn attach_point_counted(&self, ctx: Option<DefId>, a: &Attach, lookups: &mut usize) -> Option<DVec3> {
        if a.to == AttachTo::Origin && a.path.is_empty() {
            return None;
        }
        let (_, ents, m) = self.leaf_of(ctx, &a.path, lookups)?;
        let p = leaf_point(ents, &a.to, lookups)?;
        p.is_finite().then(|| m.transform_point3(p))
    }

    /// Empties the notes of what was replaced in collection `ctx` (once the annotations are in
    /// step, nothing reads them any more).
    pub fn forget_replaced(&mut self, ctx: Option<DefId>) {
        let e = match ctx {
            Some(d) => self.defs.get_mut(d).map(|d| &mut d.entities),
            None => Some(&mut self.root),
        };
        if let Some(e) = e {
            e.replaced.clear();
        }
    }

    /// Works the index out if it is not known (each collection's annotations, never their
    /// geometry); true when it did.
    fn know_attached(&mut self) -> bool {
        if self.attached.known {
            return false;
        }
        let mut colls = Vec::new();
        if self.root.annots.values().any(Annotation::is_attached) {
            colls.push(None);
        }
        for (d, def) in self.defs.iter() {
            if def.entities.annots.values().any(Annotation::is_attached) {
                colls.push(Some(d));
            }
        }
        self.attached = Index { known: true, colls };
        true
    }

    /// Rewrites every attached end of the model: `f(ctx, end)` gives `Some(new)` to change it
    /// (`Some(None)` detaches it), `None` to leave it.
    fn rewrite_ends(&mut self, f: impl Fn(&Model, Option<DefId>, AnnotId, &Attach) -> Option<Option<Attach>>) {
        self.know_attached();
        let mut changes = Vec::new();
        for &ctx in &self.attached.colls {
            let Some(ents) = self.entities_get(ctx) else { continue };
            for (id, an) in ents.annots.iter() {
                for w in 0..2 {
                    if let Some(a) = &an.attach[w]
                        && let Some(n) = f(self, ctx, id, a)
                    {
                        changes.push((ctx, id, w, n));
                    }
                }
            }
        }
        for (ctx, id, w, n) in changes {
            let ents = match ctx {
                Some(d) => &mut self.defs[d].entities,
                None => &mut self.root,
            };
            if let Some(an) = ents.annots.get_mut(id) {
                an.attach[w] = n;
            }
        }
    }

    /// The ends attached through what Make Group moved from collection `ctx` into the new
    /// instance `group` follow it: a path that reached `ctx` goes on through
    /// the group, and the annotations moved along (`moved`: their ids in the group's
    /// definition) are attached to the copies, or detached.
    pub fn attach_after_group(&mut self, ctx: Option<DefId>, group: InstanceId, copied: &Copied, moved: &[AnnotId]) {
        let Some(def) = self.entities_get(ctx).and_then(|e| e.instances.get(group)).map(|i| i.def) else { return };
        self.rewrite_ends(|m, actx, _, a| {
            let (k, at) = m.path_reaches(actx, &a.path, ctx)?;
            if actx == Some(def) {
                return None;
            }
            let mut path = a.path[..k].to_vec();
            match at {
                // Through an instance moved into the group.
                Some(i) => {
                    let n = copied.insts.get(&i)?;
                    path.extend([group, *n]);
                    path.extend_from_slice(&a.path[k + 1..]);
                    Some(Some(Attach { path, to: a.to }))
                }
                // To something of `ctx` that went into the group.
                None => {
                    let still = m.entities_get(ctx).is_some_and(|e| leaf_point(e, &a.to, &mut 0).is_some());
                    if still {
                        return None;
                    }
                    path.push(group);
                    Some(copied.leaf(&a.to).map(|to| Attach { path, to }))
                }
            }
        });
        let ents = &mut self.defs[def].entities;
        let mut any = false;
        for &id in moved {
            if let Some(an) = ents.annots.get_mut(id) {
                for w in 0..2 {
                    an.attach[w] = an.attach[w].as_ref().and_then(|a| copied.end(a, |t| copied.leaf(t)));
                    any |= an.attach[w].is_some();
                }
            }
        }
        if any {
            self.attached.note(Some(def));
        }
    }

    /// The ends attached through instance `inst` of collection `ctx`, exploded, go to the
    /// copies of its entities (`copied`: from its definition's keys to `ctx`'s);
    /// `leaf` maps what the path ended on. The annotations of the definition
    /// copied into `ctx` (`fresh`) are attached the same way.
    pub fn attach_after_explode(
        &mut self,
        ctx: Option<DefId>,
        inst: InstanceId,
        copied: &Copied,
        leaf: &dyn Fn(&AttachTo) -> Option<AttachTo>,
        fresh: &[AnnotId],
    ) {
        self.rewrite_ends(|m, actx, id, a| {
            let (k, at) = m.path_reaches(actx, &a.path, ctx)?;
            if at != Some(inst) || (actx == ctx && fresh.contains(&id)) {
                return None;
            }
            let mut path = a.path[..k].to_vec();
            Some(match a.path.get(k + 1) {
                Some(next) => copied.insts.get(next).map(|n| {
                    path.push(*n);
                    path.extend_from_slice(&a.path[k + 2..]);
                    Attach { path, to: a.to }
                }),
                None => leaf(&a.to).map(|to| Attach { path, to }),
            })
        });
        let ents = match ctx {
            Some(d) => &mut self.defs[d].entities,
            None => &mut self.root,
        };
        let mut any = false;
        for &id in fresh {
            if let Some(an) = ents.annots.get_mut(id) {
                for w in 0..2 {
                    an.attach[w] = an.attach[w].as_ref().and_then(|a| copied.end(a, leaf));
                    any |= an.attach[w].is_some();
                }
            }
        }
        if any {
            self.attached.note(ctx);
        }
    }

    /// Where the path `path` of an annotation of collection `actx` passes through collection
    /// `ctx`: the index `k` of the path at which it is in `ctx`, and the instance of `ctx` it
    /// goes through there (`None`: it ends there, on its leaf).
    fn path_reaches(
        &self,
        actx: Option<DefId>,
        path: &[InstanceId],
        ctx: Option<DefId>,
    ) -> Option<(usize, Option<InstanceId>)> {
        let mut c = actx;
        for (k, &i) in path.iter().enumerate() {
            if c == ctx {
                return Some((k, Some(i)));
            }
            c = Some(self.entities_get(c)?.instances.get(i)?.def);
        }
        (c == ctx).then_some((path.len(), None))
    }

    /// Attaches an end of annotation `id` of collection `ctx` (`which`: 0 the start of a
    /// dimension or the point of a text, 1 the end of a dimension), or detaches it with `None`.
    /// The annotation's point moves to where the end now is.
    pub fn set_attach(&mut self, ctx: Option<DefId>, id: AnnotId, which: usize, a: Option<Attach>) {
        let p = a.as_ref().and_then(|a| self.attach_point(ctx, a));
        let Some(ents) = (match ctx {
            Some(d) => self.defs.get_mut(d).map(|d| &mut d.entities),
            None => Some(&mut self.root),
        }) else {
            return;
        };
        let Some(an) = ents.annots.get_mut(id) else { return };
        if which > 1 || (which == 1 && !matches!(an.kind, AnnotKind::Linear { .. })) {
            return;
        }
        if let Some(p) = p {
            an.set_end(which, p);
        }
        let attached = a.is_some();
        an.attach[which] = a.filter(|_| p.is_some());
        if attached {
            self.attached.note(ctx);
        }
    }

    /// Puts the attached annotations in step with what they are attached to (see the module's
    /// documentation). Called after every change of the model.
    pub fn sync_annotations(&mut self) -> Sync {
        let mut s = Sync { rebuilt: self.know_attached(), ..Default::default() };
        let mut keep = Vec::new();
        let mut updates: Vec<Update> = Vec::new();
        // The collections whose notes of replaced entities were read.
        let mut read_notes: Vec<Option<DefId>> = Vec::new();
        for ctx in self.attached.colls.clone() {
            let Some(ents) = self.entities_get(ctx) else { continue };
            s.collections += 1;
            let mut any = false;
            for (id, an) in ents.annots.iter() {
                if no_ends(&an.attach) {
                    continue;
                }
                any = true;
                s.annotations += 1;
                let mut ends = an.attach.clone();
                let mut pts = [None, None];
                for w in 0..2 {
                    let Some(a) = &an.attach[w] else { continue };
                    // The path once; then the replacements, then the point.
                    let p = self.leaf_of(ctx, &a.path, &mut s.lookups).and_then(|(def, leaf, m)| {
                        if !leaf.replaced.is_empty() && !read_notes.contains(&def) {
                            read_notes.push(def);
                        }
                        let a = follow(leaf, a, an.end(w), &m, &mut s.lookups);
                        if a.to == AttachTo::Origin && a.path.is_empty() {
                            return None;
                        }
                        let p = leaf_point(leaf, &a.to, &mut s.lookups)?;
                        p.is_finite().then(|| (m.transform_point3(p), a))
                    });
                    match p {
                        Some((p, a)) => {
                            pts[w] = Some(p);
                            ends[w] = Some(a);
                        }
                        None => ends[w] = None,
                    }
                }
                updates.push((ctx, id, ends, pts));
            }
            if any {
                keep.push(ctx);
            }
        }
        for (ctx, id, ends, pts) in updates {
            let ents = match ctx {
                Some(d) => &mut self.defs[d].entities,
                None => &mut self.root,
            };
            let an = &mut ents.annots[id];
            let mut changed = false;
            for w in 0..2 {
                if an.attach[w].is_some() && ends[w].is_none() {
                    s.detached += 1;
                }
                if let Some(p) = pts[w]
                    && an.end(w).is_none_or(|q| q.distance_squared(p) > 0.0)
                {
                    an.set_end(w, p);
                    s.moved += 1;
                    changed = true;
                }
            }
            an.attach = ends;
            // A dimension whose ends came together goes away.
            if changed
                && let AnnotKind::Linear { a, b, .. } = an.kind
                && a.distance(b) < TOL
            {
                ents.annots.remove(id);
                s.erased += 1;
            }
        }
        for ctx in read_notes {
            if let Some(e) = match ctx {
                Some(d) => self.defs.get_mut(d).map(|d| &mut d.entities),
                None => Some(&mut self.root),
            } {
                e.replaced.clear();
            }
        }
        self.attached.colls = keep;
        s
    }
}

/// The end `a` once the replacements noted in its leaf's collection `ents` are followed
/// (`last`: the end's last point, in the annotation's collection; `m`: the path's
/// transformation).
fn follow(ents: &Entities, a: &Attach, last: Option<DVec3>, m: &DMat4, lookups: &mut usize) -> Attach {
    if ents.replaced.is_empty() {
        return a.clone();
    }
    let mut to = a.to;
    // Each entry once, in order: an entity can be replaced several times in one operation.
    for r in &ents.replaced {
        *lookups += 1;
        to = match (r, to) {
            (Replaced::Vertex(old, new), AttachTo::Vertex(v)) if *old == v => AttachTo::Vertex(*new),
            (Replaced::Edge { old, new, split }, x) => match x {
                AttachTo::EdgeMiddle(e) if e == *old => AttachTo::EdgeMiddle(*new),
                AttachTo::OnEdge(e, t) if e == *old => {
                    let at = last.map(|p| m.inverse().transform_point3(p));
                    match (split, at.and_then(|p| Some((p, ents.edges.get(*new)?)))) {
                        (false, Some((p, x))) => {
                            let (Some(a), Some(b)) = (ents.verts.get(x.v[0]), ents.verts.get(x.v[1])) else {
                                continue;
                            };
                            AttachTo::OnEdge(*new, project_on_segment(p, a.pos, b.pos).0.clamp(0.0, 1.0))
                        }
                        _ => AttachTo::OnEdge(*new, t),
                    }
                }
                AttachTo::ArcCenter(e) if e == *old => AttachTo::ArcCenter(*new),
                AttachTo::OnArc(e, t) if e == *old => AttachTo::OnArc(*new, t),
                x => x,
            },
            (Replaced::Face { old, new, moved }, AttachTo::OnFace(f, ..)) if *old == f => {
                // The point as it was, in the leaf's collection, and the
                // fractions that name it on the face that holds it.
                let Some(p) = last.map(|p| m.inverse().transform_point3(p)) else { continue };
                let mut faces = new.clone();
                if !moved && ents.faces.contains_key(f) {
                    faces.push(f);
                }
                match face_holding(ents, &faces, p).and_then(|nf| Some((nf, face_fractions(ents, nf, p)?))) {
                    Some((nf, (u, v))) => AttachTo::OnFace(nf, u, v),
                    None => to,
                }
            }
            _ => to,
        };
    }
    Attach { path: a.path.clone(), to }
}

/// The face among `new` that holds `p` (projected on its plane); the only one when there is
/// one (code 6 with one face).
fn face_holding(ents: &Entities, new: &[FaceId], p: DVec3) -> Option<FaceId> {
    let live: Vec<FaceId> = new.iter().copied().filter(|f| ents.faces.contains_key(*f)).collect();
    for &f in &live {
        let face = &ents.faces[f];
        let n = face.normal.normalize_or_zero();
        let (u, v) = crate::math::basis_for(n);
        let to2 = |q: DVec3| DVec2::new(q.dot(u), q.dot(v));
        let ring = |l: &[VertexId]| -> Option<Vec<DVec2>> {
            l.iter().map(|x| ents.verts.get(*x).map(|x| to2(x.pos))).collect()
        };
        let Some(outer) = ring(&face.outer) else { continue };
        let q = to2(p);
        if point_in_polygon_2d(q, &outer)
            && !face.holes.iter().any(|h| ring(h).is_some_and(|h| point_in_polygon_2d(q, &h)))
        {
            return Some(f);
        }
    }
    (live.len() == 1).then(|| live[0])
}

impl Annotation {
    /// The point of end `which` (0: a dimension's start or a text's point; 1: a dimension's end).
    pub fn end(&self, which: usize) -> Option<DVec3> {
        match (&self.kind, which) {
            (AnnotKind::Linear { a, .. }, 0) => Some(*a),
            (AnnotKind::Linear { b, .. }, 1) => Some(*b),
            (AnnotKind::Text { point, .. }, 0) => Some(*point),
            _ => None,
        }
    }

    /// Moves end `which` (the leader of a text and the offset of a dimension stay).
    pub fn set_end(&mut self, which: usize, p: DVec3) {
        match (&mut self.kind, which) {
            (AnnotKind::Linear { a, .. }, 0) => *a = p,
            (AnnotKind::Linear { b, .. }, 1) => *b = p,
            (AnnotKind::Text { point, .. }, 0) => *point = p,
            _ => {}
        }
    }

    /// Whether an end is attached (a dimension shown as non-associated has none).
    pub fn is_attached(&self) -> bool {
        !no_ends(&self.attach)
    }
}

/// What an attach point names for a point `p` of collection `ents` on edge `e`: its vertex
/// when `p` is at an end, its middle, or the fraction along it (the edge
/// case of `Sketchup::Entities#add_dimension_linear`).
pub fn on_edge(ents: &Entities, e: EdgeId, p: DVec3) -> Option<AttachTo> {
    let x = ents.edges.get(e)?;
    let (a, b) = (ents.verts.get(x.v[0])?.pos, ents.verts.get(x.v[1])?.pos);
    if p.distance(a) < TOL {
        return Some(AttachTo::Vertex(x.v[0]));
    }
    if p.distance(b) < TOL {
        return Some(AttachTo::Vertex(x.v[1]));
    }
    if p.distance((a + b) * 0.5) < TOL {
        return Some(AttachTo::EdgeMiddle(e));
    }
    along_edge(ents, e, p)
}

/// The point of edge `e`'s line nearest `p`, by its fraction from the edge's first vertex
/// (below 0 or above 1 beyond its ends: SketchUp's type 5 does not bound it).
pub fn along_edge(ents: &Entities, e: EdgeId, p: DVec3) -> Option<AttachTo> {
    let x = ents.edges.get(e)?;
    let (a, b) = (ents.verts.get(x.v[0])?.pos, ents.verts.get(x.v[1])?.pos);
    let len2 = (b - a).length_squared();
    let t = if len2 > 0.0 { (p - a).dot(b - a) / len2 } else { 0.0 };
    Some(AttachTo::OnEdge(e, t))
}

/// What names the point of the arc edge `e` belongs to in the direction of `p` from its
/// centre (the end of a radial dimension).
pub fn on_arc(ents: &Entities, e: EdgeId, p: DVec3) -> Option<AttachTo> {
    let edges = curve_of(ents, e, &mut 0)?;
    let fit = crate::curves::arc_fit_edges(ents, &edges)?;
    Some(AttachTo::OnArc(e, fit.angle_of(p)))
}

/// What names a point `p` of a construction line `g` (its parameter along it).
pub fn on_guide(ents: &Entities, g: GuideId, p: DVec3) -> Option<AttachTo> {
    let x = ents.guides.get(g)?;
    if x.is_point() {
        return Some(AttachTo::GuidePoint(g));
    }
    Some(AttachTo::OnGuide(g, (p - x.pos).dot(x.unit_dir()?)))
}

/// What names a point `p` of face `f`.
pub fn on_face(ents: &Entities, f: FaceId, p: DVec3) -> Option<AttachTo> {
    let (u, v) = face_fractions(ents, f, p)?;
    Some(AttachTo::OnFace(f, u, v))
}

/// What a copy of entities made of each (Make Group, Explode, Move with Copy…): the ends
/// attached to the originals go to the copies (SketchUp's observers).
#[derive(Clone, Debug, Default)]
pub struct Copied {
    pub verts: HashMap<VertexId, VertexId>,
    pub edges: HashMap<EdgeId, EdgeId>,
    /// Copies of edges that run the other way (a fraction along them is turned round).
    pub flipped: std::collections::HashSet<EdgeId>,
    pub faces: HashMap<FaceId, FaceId>,
    pub guides: HashMap<GuideId, GuideId>,
    pub sections: HashMap<SectionId, SectionId>,
    pub insts: HashMap<InstanceId, InstanceId>,
}

impl Copied {
    /// The copy of what an end names (none when it was not copied).
    pub fn leaf(&self, to: &AttachTo) -> Option<AttachTo> {
        let edge = |e: &EdgeId| self.edges.get(e).copied();
        let along = |e: &EdgeId, t: f64| {
            let n = edge(e)?;
            Some((n, if self.flipped.contains(&n) { 1.0 - t } else { t }))
        };
        Some(match *to {
            AttachTo::Vertex(v) => AttachTo::Vertex(*self.verts.get(&v)?),
            AttachTo::GuidePoint(g) => AttachTo::GuidePoint(*self.guides.get(&g)?),
            AttachTo::EdgeMiddle(e) => AttachTo::EdgeMiddle(edge(&e)?),
            AttachTo::OnEdge(e, t) => {
                let (n, t) = along(&e, t)?;
                AttachTo::OnEdge(n, t)
            }
            AttachTo::OnGuide(g, d) => AttachTo::OnGuide(*self.guides.get(&g)?, d),
            AttachTo::OnFace(f, u, v) => AttachTo::OnFace(*self.faces.get(&f)?, u, v),
            AttachTo::ArcCenter(e) => AttachTo::ArcCenter(edge(&e)?),
            AttachTo::OnArc(e, a) => AttachTo::OnArc(edge(&e)?, a),
            AttachTo::Section(s, p) => AttachTo::Section(*self.sections.get(&s)?, p),
            AttachTo::Origin => return None,
        })
    }

    /// An end of the source collection, for the collection of the copies: through the copy of
    /// its first instance, or on the copy of its leaf (`leaf` maps it); none when not copied.
    pub fn end(&self, a: &Attach, leaf: impl Fn(&AttachTo) -> Option<AttachTo>) -> Option<Attach> {
        match a.path.split_first() {
            Some((i, rest)) => {
                let mut path = vec![*self.insts.get(i)?];
                path.extend_from_slice(rest);
                Some(Attach { path, to: a.to })
            }
            None => Some(Attach { path: Vec::new(), to: leaf(&a.to)? }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Definition, Instance};

    fn square(e: &mut Entities, size: f64) -> FaceId {
        let c = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y].map(|q| e.add_vertex_raw(q * size));
        e.add_face_raw(c.to_vec(), Vec::new()).unwrap()
    }

    fn component(m: &mut Model, e: Entities) -> DefId {
        m.defs.insert(Definition {
            name: "C".into(),
            is_group: false,
            description: String::new(),
            entities: e,
            attributes: Default::default(),
            ext: Default::default(),
        })
    }

    fn place(m: &mut Model, d: DefId, at: DVec3) -> InstanceId {
        m.root.instances.insert(Instance {
            def: d,
            xform: DMat4::from_translation(at),
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        })
    }

    fn dim(m: &mut Model, a: DVec3, b: DVec3) -> AnnotId {
        m.root.annots.insert(Annotation {
            kind: AnnotKind::Linear { a, b, offset: DVec3::Y * 10.0 },
            text: String::new(),
            layer: 0,
            hidden: false,
            attach: Default::default(),
        })
    }

    fn ends(m: &Model, id: AnnotId) -> (DVec3, DVec3) {
        match m.root.annots[id].kind {
            AnnotKind::Linear { a, b, .. } => (a, b),
            _ => unreachable!(),
        }
    }

    #[test]
    fn a_face_point_round_trips_through_its_fractions() {
        let mut e = Entities::default();
        let f = square(&mut e, 100.0);
        let p = DVec3::new(30.0, 70.0, 0.0);
        let AttachTo::OnFace(_, u, v) = on_face(&e, f, p).unwrap() else { panic!() };
        assert!((u - 0.3).abs() < 1e-12 && (v - 0.7).abs() < 1e-12, "{u} {v}");
        assert!(leaf_point(&e, &AttachTo::OnFace(f, u, v), &mut 0).unwrap().distance(p) < 1e-9);
    }

    /// The case of `test_add_dimension_linear_instance_path_after_move`: both ends on an edge
    /// of a component, by path; the instance moves, the dimension follows, its offset stays.
    #[test]
    fn a_dimension_follows_the_instance_it_is_attached_through() {
        let mut m = Model::new();
        let mut e = Entities::default();
        let a = e.add_vertex_raw(DVec3::new(9.0, 0.0, 0.0));
        let b = e.add_vertex_raw(DVec3::new(9.0, 9.0, 0.0));
        e.add_edge_raw(a, b);
        let d = component(&mut m, e);
        let i = place(&mut m, d, DVec3::ZERO);
        let id = dim(&mut m, DVec3::new(9.0, 0.0, 0.0), DVec3::new(9.0, 9.0, 0.0));
        m.set_attach(None, id, 0, Some(Attach::new(vec![i], AttachTo::Vertex(a))));
        m.set_attach(None, id, 1, Some(Attach::new(vec![i], AttachTo::Vertex(b))));
        m.root.instances[i].xform = DMat4::from_translation(DVec3::splat(12.0));
        let s = m.sync_annotations();
        assert_eq!(ends(&m, id), (DVec3::new(21.0, 12.0, 12.0), DVec3::new(21.0, 21.0, 12.0)));
        assert_eq!((s.moved, s.detached, s.erased), (2, 0, 0));
        let AnnotKind::Linear { offset, .. } = m.root.annots[id].kind else { panic!() };
        assert_eq!(offset, DVec3::Y * 10.0);
        // Editing the definition moves it too (every instance's annotations).
        m.defs[d].entities.verts[b].pos.y = 20.0;
        m.sync_annotations();
        assert_eq!(ends(&m, id).1, DVec3::new(21.0, 32.0, 12.0));
    }

    /// Erasing what an end names detaches it where it was; an instance of
    /// the path erased does the same.
    #[test]
    fn erasing_detaches_and_ends_together_erase_a_dimension() {
        let mut m = Model::new();
        let a = m.root.add_vertex_raw(DVec3::ZERO);
        let b = m.root.add_vertex_raw(DVec3::X * 50.0);
        let e = m.root.add_edge_raw(a, b).unwrap();
        let id = dim(&mut m, DVec3::ZERO, DVec3::X * 50.0);
        m.set_attach(None, id, 0, Some(Attach::local(AttachTo::Vertex(a))));
        m.set_attach(None, id, 1, Some(Attach::local(AttachTo::EdgeMiddle(e))));
        assert_eq!(ends(&m, id).1, DVec3::X * 25.0);
        m.root.erase(&[crate::model::EntityRef::Edge(e)]);
        let s = m.sync_annotations();
        assert_eq!(s.detached, 2);
        assert_eq!(ends(&m, id), (DVec3::ZERO, DVec3::X * 25.0));
        assert!(!m.root.annots[id].is_attached());
        // Two ends brought together: the dimension goes.
        let p = m.root.add_vertex_raw(DVec3::ZERO);
        let q = m.root.add_vertex_raw(DVec3::Y * 5.0);
        let id = dim(&mut m, DVec3::ZERO, DVec3::Y * 5.0);
        m.set_attach(None, id, 0, Some(Attach::local(AttachTo::Vertex(p))));
        m.set_attach(None, id, 1, Some(Attach::local(AttachTo::Vertex(q))));
        m.root.verts[q].pos = DVec3::ZERO;
        assert_eq!(m.sync_annotations().erased, 1);
        assert!(!m.root.annots.contains_key(id));
    }

    /// A big model: thousands of definitions, a root of 40 000 faces, many annotations that
    /// are not attached and two that are. Each update looks only at the root's annotations and
    /// at the two ends' paths: a fixed count, whatever the size of the model.
    #[test]
    fn an_update_never_walks_the_model() {
        let mut m = Model::new();
        for i in 0..40_000 {
            let c = [DVec3::ZERO, DVec3::X, DVec3::Y].map(|q| m.root.add_vertex_raw(q + DVec3::Z * i as f64));
            m.root.add_face_raw(c.to_vec(), Vec::new());
        }
        let mut e = Entities::default();
        let v = e.add_vertex_raw(DVec3::X);
        let d = component(&mut m, e);
        for _ in 0..3000 {
            let mut e = Entities::default();
            square(&mut e, 5.0);
            let x = component(&mut m, e);
            for k in 0..3 {
                m.defs[x].entities.annots.insert(Annotation {
                    kind: AnnotKind::Text { point: DVec3::X * k as f64, leader: DVec3::Y },
                    text: "libre".into(),
                    layer: 0,
                    hidden: false,
                    attach: Default::default(),
                });
            }
        }
        let i = place(&mut m, d, DVec3::ZERO);
        let id = dim(&mut m, DVec3::X, DVec3::Y * 1000.0);
        m.set_attach(None, id, 0, Some(Attach::new(vec![i], AttachTo::Vertex(v))));
        // The first update works the index out (every collection's annotations, once).
        assert!(m.sync_annotations().rebuilt);
        let mut total = Sync::default();
        for k in 0..50 {
            m.root.instances[i].xform = DMat4::from_translation(DVec3::X * k as f64);
            let s = m.sync_annotations();
            assert!(!s.rebuilt);
            total.collections += s.collections;
            total.annotations += s.annotations;
            total.lookups += s.lookups;
        }
        // One collection, one annotation, the instance and the vertex: per update.
        assert_eq!((total.collections, total.annotations, total.lookups), (50, 50, 100));
        assert_eq!(ends(&m, id).0, DVec3::X * 50.0);
    }

    #[test]
    fn a_text_on_a_face_goes_to_the_piece_that_holds_it() {
        let mut m = Model::new();
        let f = square(&mut m.root, 100.0);
        let id = m.root.annots.insert(Annotation {
            kind: AnnotKind::Text { point: DVec3::new(80.0, 50.0, 0.0), leader: DVec3::Z * 20.0 },
            text: "t".into(),
            layer: 0,
            hidden: false,
            attach: Default::default(),
        });
        let to = on_face(&m.root, f, DVec3::new(80.0, 50.0, 0.0)).unwrap();
        m.set_attach(None, id, 0, Some(Attach::local(to)));
        // A line across the face splits it in two.
        m.root.add_line(DVec3::new(50.0, 0.0, 0.0), DVec3::new(50.0, 100.0, 0.0));
        assert!(!m.root.faces.contains_key(f) || m.root.faces.len() == 2);
        m.sync_annotations();
        let a = m.root.annots[id].attach[0].clone().expect("toujours attaché");
        let AttachTo::OnFace(nf, ..) = a.to else { panic!() };
        assert!(m.root.faces.contains_key(nf));
        assert!(m.attach_point(None, &a).unwrap().distance(DVec3::new(80.0, 50.0, 0.0)) < 1e-6);
        // Pushed up, it follows the face.
        let pushed = m.root.push_pull(nf, 30.0, false).unwrap();
        m.sync_annotations();
        let AnnotKind::Text { point, .. } = m.root.annots[id].kind else { panic!() };
        assert!(point.distance(DVec3::new(80.0, 50.0, 30.0)) < 1e-6, "{point}");
        assert_eq!(m.root.annots[id].attach[0].as_ref().unwrap().to, {
            let AttachTo::OnFace(_, u, v) = m.root.annots[id].attach[0].as_ref().unwrap().to else { panic!() };
            AttachTo::OnFace(pushed, u, v)
        });
    }

    /// A box edge of 30 in a component, a dimension of the root on its two ends by path.
    fn dimensioned_component() -> (Model, InstanceId, AnnotId) {
        let mut m = Model::new();
        let mut e = Entities::default();
        let a = e.add_vertex_raw(DVec3::ZERO);
        let b = e.add_vertex_raw(DVec3::X * 30.0);
        e.add_edge_raw(a, b);
        let d = component(&mut m, e);
        let i = place(&mut m, d, DVec3::Y * 100.0);
        let id = dim(&mut m, DVec3::Y * 100.0, DVec3::new(30.0, 100.0, 0.0));
        m.set_attach(None, id, 0, Some(Attach::new(vec![i], AttachTo::Vertex(a))));
        m.set_attach(
            None,
            id,
            1,
            Some(Attach::new(vec![i], AttachTo::EdgeMiddle(m.defs[d].entities.edges.keys().next().unwrap()))),
        );
        assert_eq!(ends(&m, id).1, DVec3::new(15.0, 100.0, 0.0));
        (m, i, id)
    }

    /// Exploded, the instance's copies take its place: the ends are attached to them, in the
    /// root, and follow them.
    #[test]
    fn exploding_hands_the_ends_to_the_copies() {
        let (mut m, i, id) = dimensioned_component();
        m.explode(None, i);
        m.sync_annotations();
        let an = &m.root.annots[id];
        let (Some(a), Some(b)) = (&an.attach[0], &an.attach[1]) else { panic!("{:?}", an.attach) };
        assert!(a.path.is_empty() && b.path.is_empty());
        let AttachTo::Vertex(v) = a.to else { panic!() };
        m.root.verts[v].pos.x = -20.0;
        m.sync_annotations();
        assert_eq!(ends(&m, id), (DVec3::new(-20.0, 100.0, 0.0), DVec3::new(5.0, 100.0, 0.0)));
    }

    /// Made into a group, the geometry takes the ends with it: the path goes through the group.
    #[test]
    fn grouping_takes_the_ends_along() {
        let mut m = Model::new();
        let a = m.root.add_vertex_raw(DVec3::ZERO);
        let b = m.root.add_vertex_raw(DVec3::X * 40.0);
        let e = m.root.add_edge_raw(a, b).unwrap();
        let id = dim(&mut m, DVec3::ZERO, DVec3::X * 40.0);
        m.set_attach(None, id, 0, Some(Attach::local(AttachTo::Vertex(a))));
        m.set_attach(None, id, 1, Some(Attach::local(AttachTo::OnEdge(e, 0.75))));
        let g = m.make_group(None, &[crate::model::EntityRef::Edge(e)], true, None).unwrap();
        let s = m.sync_annotations();
        assert_eq!(s.detached, 0);
        let an = &m.root.annots[id];
        assert_eq!(an.attach[0].as_ref().unwrap().path, vec![g]);
        m.root.instances[g].xform = DMat4::from_translation(DVec3::Z * 7.0);
        m.sync_annotations();
        assert_eq!(ends(&m, id), (DVec3::Z * 7.0, DVec3::new(30.0, 0.0, 7.0)));
    }

    /// Copied with what it is attached to, a dimension goes with the copies; copied alone, it
    /// stays on the originals.
    #[test]
    fn a_copy_follows_the_copies() {
        let mut m = Model::new();
        let a = m.root.add_vertex_raw(DVec3::ZERO);
        let b = m.root.add_vertex_raw(DVec3::X * 40.0);
        let e = m.root.add_edge_raw(a, b).unwrap();
        let id = dim(&mut m, DVec3::ZERO, DVec3::X * 40.0);
        m.set_attach(None, id, 0, Some(Attach::local(AttachTo::Vertex(a))));
        m.set_attach(None, id, 1, Some(Attach::local(AttachTo::Vertex(b))));
        use crate::model::EntityRef as R;
        let copies = m.root.duplicate(&[R::Edge(e), R::Annot(id)]);
        let moved = DMat4::from_translation(DVec3::Y * 50.0);
        m.root.transform_selection(&copies, &moved);
        let alone = m.root.duplicate(&[R::Annot(id)]);
        m.sync_annotations();
        let get = |r: &R| match *r {
            R::Annot(x) => ends(&m, x),
            _ => unreachable!(),
        };
        let copy = copies.iter().find(|r| matches!(r, R::Annot(_))).unwrap();
        assert_eq!(get(copy), (DVec3::Y * 50.0, DVec3::new(40.0, 50.0, 0.0)));
        assert_eq!(get(&alone[0]), (DVec3::ZERO, DVec3::X * 40.0));
    }

    /// A vertex merged into another (moved onto it) hands it its ends (code 6).
    #[test]
    fn a_welded_vertex_hands_over_its_ends() {
        let mut m = Model::new();
        let p = [DVec3::ZERO, DVec3::X * 10.0, DVec3::new(30.0, 0.0, 0.0), DVec3::new(30.0, 5.0, 0.0)]
            .map(|q| m.root.add_vertex_raw(q));
        m.root.add_edge_raw(p[0], p[1]);
        m.root.add_edge_raw(p[2], p[3]);
        let id = dim(&mut m, DVec3::ZERO, DVec3::X * 10.0);
        m.set_attach(None, id, 1, Some(Attach::local(AttachTo::Vertex(p[1]))));
        // p[1] moves onto p[2] and merges into it.
        let set = std::collections::HashSet::from([p[1]]);
        m.root.transform_vertices(&set, &DMat4::from_translation(DVec3::X * 20.0));
        assert!(!m.root.verts.contains_key(p[1]));
        m.sync_annotations();
        assert_eq!(m.root.annots[id].attach[1].as_ref().unwrap().to, AttachTo::Vertex(p[2]));
        m.root.verts[p[2]].pos.y = 9.0;
        m.sync_annotations();
        assert_eq!(ends(&m, id).1, DVec3::new(30.0, 9.0, 0.0));
    }
}

#[cfg(test)]
mod operations {
    use super::*;
    use crate::model::EntityRef;

    fn text_at(m: &mut Model, p: DVec3) -> AnnotId {
        m.root.annots.insert(Annotation {
            kind: AnnotKind::Text { point: p, leader: DVec3::Z * 20.0 },
            text: "t".into(),
            layer: 0,
            hidden: false,
            attach: Default::default(),
        })
    }

    fn point(m: &Model, id: AnnotId, w: usize) -> DVec3 {
        m.root.annots[id].end(w).unwrap()
    }

    /// An edge split by a line drawn across it stays the first piece for what is attached to
    /// it (SketchUp keeps the edge from its start: `Edge#split`; its connections stay on it,
    /// code 4): its middle is the first piece's, a fraction the same fraction.
    #[test]
    fn an_edge_split_keeps_its_ends_on_the_first_piece() {
        let mut m = Model::new();
        let a = m.root.add_vertex_raw(DVec3::ZERO);
        let b = m.root.add_vertex_raw(DVec3::X * 100.0);
        let e = m.root.add_edge_raw(a, b).unwrap();
        let id = m.root.annots.insert(Annotation {
            kind: AnnotKind::Linear { a: DVec3::ZERO, b: DVec3::X, offset: DVec3::Y },
            text: String::new(),
            layer: 0,
            hidden: false,
            attach: Default::default(),
        });
        m.set_attach(None, id, 0, Some(Attach::local(AttachTo::EdgeMiddle(e))));
        m.set_attach(None, id, 1, Some(Attach::local(AttachTo::OnEdge(e, 0.75))));
        m.root.add_line(DVec3::new(40.0, -10.0, 0.0), DVec3::new(40.0, 10.0, 0.0));
        assert!(!m.root.edges.contains_key(e));
        let s = m.sync_annotations();
        assert_eq!(s.detached, 0);
        assert!(point(&m, id, 0).distance(DVec3::X * 20.0) < 1e-9, "{}", point(&m, id, 0));
        assert!(point(&m, id, 1).distance(DVec3::X * 30.0) < 1e-9, "{}", point(&m, id, 1));
    }

    /// Intersect Faces cuts a face: a text on it goes to the piece that holds its point.
    #[test]
    fn intersect_faces_hands_a_text_to_its_piece() {
        let mut m = Model::new();
        let c = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y].map(|q| m.root.add_vertex_raw(q * 100.0));
        let f = m.root.add_face_raw(c.to_vec(), Vec::new()).unwrap();
        let w = [(50.0, -10.0, -10.0), (50.0, 110.0, -10.0), (50.0, 110.0, 10.0), (50.0, -10.0, 10.0)]
            .map(|(x, y, z)| m.root.add_vertex_raw(DVec3::new(x, y, z)));
        m.root.add_face_raw(w.to_vec(), Vec::new()).unwrap();
        let id = text_at(&mut m, DVec3::new(80.0, 50.0, 0.0));
        let to = on_face(&m.root, f, DVec3::new(80.0, 50.0, 0.0)).unwrap();
        m.set_attach(None, id, 0, Some(Attach::local(to)));
        let n = m.intersect_faces(None, &[EntityRef::Face(f)], crate::intersect::IntersectWith::Model);
        assert!(n > 0);
        m.sync_annotations();
        let end = m.root.annots[id].attach[0].clone().expect("attaché");
        let AttachTo::OnFace(g, ..) = end.to else { panic!() };
        let xs: Vec<f64> = m.root.faces[g].outer.iter().map(|v| m.root.verts[*v].pos.x).collect();
        assert!(xs.iter().all(|x| *x >= 50.0 - 1e-9), "{xs:?}");
        assert!(point(&m, id, 0).distance(DVec3::new(80.0, 50.0, 0.0)) < 1e-6);
    }

    /// Follow Me along an open path keeps the profile as the start cap (the text stays on it);
    /// around a closed path the profile goes, and the text is detached where it was.
    #[test]
    fn follow_me_keeps_or_frees_a_text_on_the_profile() {
        for closed in [false, true] {
            let mut m = Model::new();
            let c = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
                .map(|(y, z)| m.root.add_vertex_raw(DVec3::new(0.0, y, z)));
            let f = m.root.add_face_raw(c.to_vec(), Vec::new()).unwrap();
            let p = DVec3::new(0.0, 5.0, 5.0);
            let id = text_at(&mut m, p);
            let to = on_face(&m.root, f, p).unwrap();
            m.set_attach(None, id, 0, Some(Attach::local(to)));
            let points = if closed {
                vec![DVec3::ZERO, DVec3::X * 100.0, DVec3::new(100.0, -100.0, 0.0), DVec3::new(0.0, -100.0, 0.0)]
            } else {
                vec![DVec3::ZERO, DVec3::X * 100.0]
            };
            let n = points.len();
            let path = crate::followme::Path { points, closed, smooth: vec![false; n] };
            m.root.follow_me(f, &path).unwrap();
            m.sync_annotations();
            assert_eq!(m.root.annots[id].attach[0].is_some(), !closed, "fermé : {closed}");
            assert!(point(&m, id, 0).distance(p) < 1e-6);
        }
    }
}
