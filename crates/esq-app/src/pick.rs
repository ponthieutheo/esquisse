//! Picking and the inference engine (snapping to endpoints, midpoints, edges, faces, axes).

use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::math::{Plane, TOL, line_closest_to_ray, project_on_segment, ray_triangle};
use esq_core::model::{EntityRef, FaceId, InstanceId, Model, SectionPlane};
use esq_core::section::clip_segment;
use esq_core::tess::face_triangles;
use esq_core::walk::visit_all;

use crate::bvh::{Aabb, Bvh};
use crate::camera::Camera;
use crate::style::{FaceStyle, ViewStyle};

pub const POINT_PX: f64 = 12.0;
pub const EDGE_PX: f64 = 7.0;
pub const AXIS_PX: f64 = 10.0;

/// Something under the cursor: an item of the collection reached through `path`.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub path: Vec<InstanceId>,
    pub item: EntityRef,
}

impl Hit {
    /// What clicking it selects in context `ctx`: the item itself, or the instance of the
    /// active context that contains it. `None` when it is outside the context.
    pub fn selectable(&self, ctx: &[InstanceId]) -> Option<EntityRef> {
        if !self.path.starts_with(ctx) {
            return None;
        }
        if self.path.len() == ctx.len() { Some(self.item) } else { Some(EntityRef::Instance(self.path[ctx.len()])) }
    }

    pub fn face_in(&self, ctx: &[InstanceId]) -> Option<FaceId> {
        match (self.path.as_slice() == ctx, self.item) {
            (true, EntityRef::Face(f)) => Some(f),
            _ => None,
        }
    }
}

struct PEdge {
    a: DVec3,
    b: DVec3,
    hit: u32,
}

struct PTri {
    v: [DVec3; 3],
    normal: DVec3,
    hit: u32,
    /// Index into `PickCache::cuts`: the section planes cutting this triangle.
    cuts: u32,
    /// Placed through a component that always faces the camera: it turns with the eye.
    turns: bool,
}

/// Whether what is placed through `path` turns with the eye (a component on the way always
/// faces the camera).
fn turns_with_eye(model: &Model, path: &[InstanceId]) -> bool {
    let mut ents = &model.root;
    for &i in path {
        let Some(d) = ents.instances.get(i).and_then(|x| model.defs.get(x.def)) else { return false };
        if d.ext.behavior.always_face_camera {
            return true;
        }
        ents = &d.entities;
    }
    false
}

/// The triangles that turn with the eye (`PTri::turns`), where the eye now puts them: with
/// the triangles of a picking cache built for another eye, what hides a point now.
#[derive(Default)]
pub struct Turning {
    tris: Vec<([DVec3; 3], usize)>,
    cuts: Vec<Vec<SectionPlane>>,
}

impl Turning {
    /// The triangles `PickCache::build` would make now for the collections that turn.
    pub fn build(model: &Model, style: &ViewStyle) -> Turning {
        let mut t = Turning::default();
        if !model.defs.values().any(|d| d.ext.behavior.always_face_camera) {
            return t;
        }
        let show_hidden = style.hidden_geometry;
        let faces = style.faces != FaceStyle::Wireframe;
        let shown_tags = model.layers_shown();
        visit_all(model, style.hidden_objects, |p| {
            if !faces || !turns_with_eye(model, &p.path) {
                return;
            }
            let vis = |l: usize| shown_tags.get(l).copied().unwrap_or(true);
            t.cuts.push(if style.section_cuts { p.cuts.clone() } else { Vec::new() });
            let cuts = t.cuts.len() - 1;
            for (f, face) in p.ents.faces.iter() {
                if (face.hidden && !show_hidden) || !vis(face.layer) {
                    continue;
                }
                for tri in face_triangles(p.ents, f) {
                    t.tris.push((tri.map(|v| p.xform.transform_point3(v)), cuts));
                }
            }
        });
        t
    }

    /// Whether one of these triangles is met by the ray nearer than `below`, outside what is
    /// cut away.
    fn hides(&self, o: DVec3, d: DVec3, below: f64) -> bool {
        self.tris.iter().any(|(v, c)| {
            ray_triangle(o, d, v[0], v[1], v[2])
                .is_some_and(|dist| dist < below && !self.cuts[*c].iter().any(|s| s.cuts(o + d * dist)))
        })
    }
}

/// A guide line in world coordinates (`dir` is unit length when infinite).
#[derive(Clone, Copy)]
pub struct PGuide {
    pub pos: DVec3,
    pub dir: DVec3,
    pub infinite: bool,
    hit: u32,
}

impl PGuide {
    /// Point of the guide closest to a ray.
    fn closest_to_ray(&self, ro: DVec3, rd: DVec3) -> Option<DVec3> {
        let t = line_closest_to_ray(self.pos, self.dir, ro, rd)?;
        let t = if self.infinite { t } else { t.clamp(0.0, 1.0) };
        Some(self.pos + self.dir * t)
    }

    pub fn contains(&self, p: DVec3) -> bool {
        if self.infinite {
            let t = (p - self.pos).dot(self.dir);
            p.distance(self.pos + self.dir * t) < TOL * 4.0
        } else {
            project_on_segment(p, self.pos, self.pos + self.dir).1 < TOL * 4.0
        }
    }
}

/// World-space copy of the model, rebuilt when the model changes.
#[derive(Default)]
pub struct PickCache {
    hits: Vec<Hit>,
    edges: Vec<PEdge>,
    tris: Vec<PTri>,
    points: Vec<DVec3>,
    guides: Vec<PGuide>,
    guide_points: Vec<(DVec3, u32)>,
    /// Dimensions and texts (world coordinates) with their label.
    annots: Vec<(esq_core::model::Annotation, String, u32)>,
    /// Outlines of the section planes (world corners).
    sections: Vec<([DVec3; 4], u32)>,
    /// Sets of active section planes; parts they cut away cannot be picked.
    cuts: Vec<Vec<SectionPlane>>,
    /// Snaps (world position, direction) and the outlines of spaces (world segments), drawn
    /// over the view (`tools/snap.rs`).
    snaps: Vec<(DVec3, DVec3, u32)>,
    spaces: Vec<(Vec<(DVec3, DVec3)>, u32)>,
    /// The modeling grid, when shown with snapping on (`model_grid.rs`).
    pub grid: Option<crate::model_grid::GridSnap>,
    /// Where to look for what is under the cursor (`bvh.rs`): the triangles, the edges, and
    /// the points the inference snaps to (`points`, then the middle of each edge, `mids`).
    tri_bvh: Bvh,
    edge_bvh: Bvh,
    point_bvh: Bvh,
    mids: Vec<DVec3>,
}

impl PickCache {
    pub fn build(model: &Model, style: &ViewStyle) -> Self {
        crate::perf::count(|c| c.pick_builds += 1);
        let mut c = PickCache { grid: crate::model_grid::snap_of(model, style.modeling_grid), ..Default::default() };
        let show_hidden = style.hidden_geometry;
        let faces = style.faces != FaceStyle::Wireframe;
        c.cuts.push(Vec::new());
        let mut tess: std::collections::HashMap<usize, Vec<Option<Vec<[DVec3; 3]>>>> = Default::default();
        let shown_tags = model.layers_shown();
        visit_all(model, style.hidden_objects, |p| {
            let vis = |l: usize| shown_tags.get(l).copied().unwrap_or(true);
            let cuts: &[SectionPlane] = if style.section_cuts { &p.cuts } else { &[] };
            let cut_idx = match c.cuts.iter().position(|x| {
                x.len() == cuts.len() && x.iter().zip(cuts).all(|(a, b)| a.pos == b.pos && a.normal == b.normal)
            }) {
                Some(i) => i as u32,
                None => {
                    c.cuts.push(cuts.to_vec());
                    (c.cuts.len() - 1) as u32
                }
            };
            if style.section_planes {
                for (id, sp) in p.ents.sections.iter() {
                    if sp.hidden || !vis(sp.layer) {
                        continue;
                    }
                    c.hits.push(Hit { path: p.path.clone(), item: EntityRef::Section(id) });
                    let r = crate::scene::section_rect(model, p.ents, &p.xform, sp);
                    c.sections.push((r, (c.hits.len() - 1) as u32));
                }
            }
            if !p.hidden {
                for s in p.ents.snaps.iter().filter(|s| crate::tools::snap::shown(model, s)) {
                    c.hits.push(Hit { path: p.path.clone(), item: EntityRef::Snap(s.id) });
                    let w = s.transformed(&p.xform);
                    c.snaps.push((w.pos, w.dir, (c.hits.len() - 1) as u32));
                }
                for s in p.ents.spaces.iter().filter(|s| crate::tools::snap::space_shown(model, s)) {
                    c.hits.push(Hit { path: p.path.clone(), item: EntityRef::Space(s.id) });
                    c.spaces.push((s.transformed(&p.xform).outline(), (c.hits.len() - 1) as u32));
                }
            }
            for (g, guide) in p.ents.guides.iter() {
                if !style.guides || guide.hidden || !vis(guide.layer) {
                    continue;
                }
                c.hits.push(Hit { path: p.path.clone(), item: EntityRef::Guide(g) });
                let hit = (c.hits.len() - 1) as u32;
                let w = guide.transformed(&p.xform);
                if w.is_point() {
                    c.guide_points.push((w.pos, hit));
                } else {
                    c.guides.push(PGuide { pos: w.pos, dir: w.dir, infinite: w.infinite, hit });
                }
            }
            for (id, a) in p.ents.annots.iter() {
                if a.hidden || !vis(a.layer) {
                    continue;
                }
                c.hits.push(Hit { path: p.path.clone(), item: EntityRef::Annot(id) });
                let w = a.transformed(&p.xform);
                let text = crate::annot::label(&w, model.units);
                c.annots.push((w, text, (c.hits.len() - 1) as u32));
            }
            for (e, edge) in p.ents.edges.iter() {
                if (edge.hidden && !show_hidden) || !vis(edge.layer) {
                    continue;
                }
                let (a, b) = p.ents.edge_points(e);
                let (a, b) = (p.xform.transform_point3(a), p.xform.transform_point3(b));
                let Some((ca, cb)) = clip_segment(a, b, cuts) else { continue };
                for (q, kept) in [(a, ca), (b, cb)] {
                    if q.distance(kept) < 1e-9 {
                        c.points.push(q);
                    }
                }
                let (a, b) = (ca, cb);
                if edge.soft && !show_hidden {
                    continue;
                }
                c.hits.push(Hit { path: p.path.clone(), item: EntityRef::Edge(e) });
                c.edges.push(PEdge { a, b, hit: (c.hits.len() - 1) as u32 });
            }
            // The triangles of a collection's faces, made once however many times it is placed.
            let tess = tess.entry(p.ents as *const _ as usize).or_insert_with(|| vec![None; p.ents.faces.len()]);
            let turns = turns_with_eye(model, &p.path);
            for (k, (f, face)) in p.ents.faces.iter().enumerate() {
                if !faces || (face.hidden && !show_hidden) || !vis(face.layer) {
                    continue;
                }
                c.hits.push(Hit { path: p.path.clone(), item: EntityRef::Face(f) });
                let hit = (c.hits.len() - 1) as u32;
                let normal = p.xform.transform_vector3(face.normal).normalize_or_zero();
                let tris = tess[k].get_or_insert_with(|| {
                    crate::perf::count(|c| c.tessellations += 1);
                    face_triangles(p.ents, f)
                });
                for t in tris.iter() {
                    c.tris.push(PTri { v: t.map(|v| p.xform.transform_point3(v)), normal, hit, cuts: cut_idx, turns });
                }
            }
        });
        c.points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)).then(a.z.total_cmp(&b.z)));
        c.points.dedup_by(|a, b| a.distance(*b) < 1e-9);
        c.mids = c.edges.iter().map(|e| (e.a + e.b) * 0.5).collect();
        c.build_trees();
        c
    }

    /// `build` on another thread while `f` runs on this one (both only read the model). What
    /// the build reads from this thread goes with it: the eye that components facing the
    /// camera turn to (`esq_core::walk`) and the units of the texts (`esq_core::units`); what
    /// it counts (`perf`) comes back.
    pub fn build_beside<R>(model: &Model, style: &ViewStyle, f: impl FnOnce() -> R) -> (Self, R) {
        let eye = esq_core::walk::eye();
        let units = esq_core::units::display();
        std::thread::scope(|s| {
            let h = s.spawn(move || {
                esq_core::walk::set_eye(eye);
                esq_core::units::set_display(units);
                let before = crate::perf::get();
                let c = Self::build(model, style);
                (c, crate::perf::get().since(before))
            });
            let r = f();
            let (c, counted) = h.join().unwrap_or_else(|e| std::panic::resume_unwind(e));
            crate::perf::count(|n| n.add(counted));
            (c, r)
        })
    }

    /// Tests: what the cache holds, bit for bit (positions, labels, what each item hits),
    /// to compare two caches.
    #[cfg(test)]
    pub(crate) fn contents(&self) -> Vec<String> {
        let b = |p: DVec3| p.to_array().map(f64::to_bits);
        let mut v = vec![format!("{:?}", self.hits)];
        v.extend(
            self.tris.iter().map(|t| format!("t {:?} {:?} {} {} {}", t.v.map(b), b(t.normal), t.hit, t.cuts, t.turns)),
        );
        v.extend(self.edges.iter().map(|e| format!("e {:?} {:?} {}", b(e.a), b(e.b), e.hit)));
        v.extend(self.points.iter().chain(&self.mids).map(|p| format!("p {:?}", b(*p))));
        v.extend(self.guide_points.iter().map(|(p, h)| format!("g {:?} {h}", b(*p))));
        v.extend(self.annots.iter().map(|(a, text, h)| format!("a {a:?} {text} {h}")));
        v
    }

    /// The trees over the triangles, the edges and the points (one thread each).
    pub(crate) fn build_trees(&mut self) {
        let (tris, edges, points, mids) = (&self.tris, &self.edges, &self.points, &self.mids);
        let (t, e, p) = std::thread::scope(|s| {
            let t = s.spawn(|| Bvh::build(&tris.iter().map(|t| Aabb::of_points(&t.v).padded()).collect::<Vec<_>>()));
            let e = s.spawn(|| Bvh::build(&edges.iter().map(|e| Aabb::of_points(&[e.a, e.b])).collect::<Vec<_>>()));
            let p = Bvh::build(&points.iter().chain(mids).map(|p| Aabb { min: *p, max: *p }).collect::<Vec<_>>());
            (t.join().expect("arbre des triangles"), e.join().expect("arbre des arêtes"), p)
        });
        (self.tri_bvh, self.edge_bvh, self.point_bvh) = (t, e, p);
    }

    /// Nearest face along a ray: (distance, hit, world normal). The nearest of the triangles
    /// the ray meets outside the parts cut away, the first one built among equally near ones.
    pub fn ray_face(&self, o: DVec3, d: DVec3) -> Option<(f64, &Hit, DVec3)> {
        let best: std::cell::Cell<Option<(f64, u32)>> = std::cell::Cell::new(None);
        let mut tested = 0u64;
        let nodes = self.tri_bvh.ray(
            o,
            d,
            || best.get().map_or(f64::INFINITY, |b| b.0),
            |i| {
                tested += 1;
                let t = &self.tris[i as usize];
                if let Some(dist) = ray_triangle(o, d, t.v[0], t.v[1], t.v[2])
                    && best.get().is_none_or(|b| dist < b.0 || (dist == b.0 && i < b.1))
                    && !self.cuts[t.cuts as usize].iter().any(|s| s.cuts(o + d * dist))
                {
                    best.set(Some((dist, i)));
                }
            },
        );
        crate::perf::count(|c| c.pick_visits += tested + nodes);
        best.get().map(|(dist, i)| {
            let t = &self.tris[i as usize];
            (dist, &self.hits[t.hit as usize], t.normal)
        })
    }

    /// Whether a face hides what lies at `below` along the ray: some triangle met nearer,
    /// outside what is cut away. `turning`: the triangles that turn with the eye, where the
    /// eye now puts them (this cache's are where it was when the cache was built).
    pub fn hides(&self, o: DVec3, d: DVec3, below: f64, turning: Option<&Turning>) -> bool {
        let Some(turning) = turning else {
            return self.ray_face(o, d).is_some_and(|(t, _, _)| t < below);
        };
        // Some hit nearer than `below`: the nearest one is, and the other way round.
        let found = std::cell::Cell::new(false);
        let nodes = self.tri_bvh.ray(
            o,
            d,
            || if found.get() { f64::NEG_INFINITY } else { below },
            |i| {
                let t = &self.tris[i as usize];
                if !t.turns
                    && !found.get()
                    && let Some(dist) = ray_triangle(o, d, t.v[0], t.v[1], t.v[2])
                    && dist < below
                    && !self.cuts[t.cuts as usize].iter().any(|s| s.cuts(o + d * dist))
                {
                    found.set(true);
                }
            },
        );
        crate::perf::count(|c| c.pick_visits += nodes);
        found.get() || turning.hides(o, d, below)
    }

    /// Nearest edge on screen within `px` pixels: (screen distance, point on edge, hit).
    pub fn screen_edge(&self, cam: &Camera, m: DVec2, px: f64) -> Option<(f64, DVec3, &Hit, (DVec3, DVec3))> {
        let (ro, rd) = cam.ray(m);
        let pj = cam.projector();
        // The edges within `px` pixels, looked at in the order they were built.
        let mut near: Vec<(u32, f64)> = Vec::new();
        let nodes = self.edge_bvh.query(
            |b| pj.box_near(b, m, px),
            |i| {
                let e = &self.edges[i as usize];
                if let (Some(a2), Some(b2)) = (pj.project(e.a), pj.project(e.b)) {
                    let d = dist_point_segment_2d(m, a2, b2);
                    if d < px {
                        near.push((i, d));
                    }
                }
            },
        );
        crate::perf::count(|c| c.pick_visits += nodes + near.len() as u64);
        near.sort_unstable_by_key(|n| n.0);
        let mut best: Option<(f64, DVec3, u32, (DVec3, DVec3))> = None;
        for (i, d) in near {
            let e = &self.edges[i as usize];
            if d < px && best.is_none_or(|b| d < b.0) {
                let dir = e.b - e.a;
                let t = line_closest_to_ray(e.a, dir, ro, rd).unwrap_or(0.0).clamp(0.0, 1.0);
                best = Some((d, e.a + dir * t, e.hit, (e.a, e.b)));
            }
        }
        best.map(|(d, p, h, s)| (d, p, &self.hits[h as usize], s))
    }

    /// The points the inference snaps to (`points`, then `mids` numbered after them) that
    /// show within `POINT_PX` of the cursor, in that order.
    fn near_points(&self, pj: &crate::camera::Projector, m: DVec2) -> Vec<u32> {
        let n = self.points.len();
        let mut near: Vec<u32> = Vec::new();
        let nodes = self.point_bvh.query(
            |b| pj.box_near(b, m, POINT_PX),
            |i| {
                let p = if (i as usize) < n { self.points[i as usize] } else { self.mids[i as usize - n] };
                if pj.project(p).is_some_and(|s| s.distance(m) < POINT_PX) {
                    near.push(i);
                }
            },
        );
        crate::perf::count(|c| c.pick_visits += nodes + near.len() as u64);
        near.sort_unstable();
        near
    }

    /// Guide lines passing within `px` pixels of the cursor, nearest first:
    /// (screen distance, point on the guide, guide).
    pub fn screen_guides(&self, cam: &Camera, m: DVec2, px: f64) -> Vec<(f64, DVec3, &PGuide)> {
        let (ro, rd) = cam.ray(m);
        let mut out: Vec<(f64, DVec3, &PGuide)> = self
            .guides
            .iter()
            .filter_map(|g| {
                let p = g.closest_to_ray(ro, rd)?;
                let d = cam.project(p)?.distance(m);
                (d < px).then_some((d, p, g))
            })
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }

    /// Section plane whose outline passes under the cursor.
    pub fn screen_section(&self, cam: &Camera, m: DVec2, px: f64) -> Option<&Hit> {
        let mut best: Option<(f64, u32)> = None;
        for (r, hit) in &self.sections {
            for i in 0..4 {
                let (Some(a), Some(b)) = (cam.project(r[i]), cam.project(r[(i + 1) % 4])) else { continue };
                let d = dist_point_segment_2d(m, a, b);
                if d < px && best.is_none_or(|x| d < x.0) {
                    best = Some((d, *hit));
                }
            }
        }
        best.map(|(_, h)| &self.hits[h as usize])
    }

    /// Snap under the cursor (its point or its arrow, drawn over everything).
    pub fn screen_snap(&self, cam: &Camera, m: DVec2, px: f64) -> Option<&Hit> {
        let mut best: Option<(f64, u32)> = None;
        for &(p, dir, hit) in &self.snaps {
            let Some(a) = cam.project(p) else { continue };
            let tip = p + dir * cam.pixel_size_at(p) * crate::tools::snap::SNAP_ARROW_PX;
            let d = cam.project(tip).map_or(a.distance(m), |b| dist_point_segment_2d(m, a, b));
            if d < px && best.is_none_or(|x| d < x.0) {
                best = Some((d, hit));
            }
        }
        best.map(|(_, h)| &self.hits[h as usize])
    }

    /// Space whose outline passes under the cursor.
    pub fn screen_space(&self, cam: &Camera, m: DVec2, px: f64) -> Option<&Hit> {
        let mut best: Option<(f64, u32)> = None;
        for (segs, hit) in &self.spaces {
            for &(a, b) in segs {
                let (Some(a), Some(b)) = (cam.project(a), cam.project(b)) else { continue };
                let d = dist_point_segment_2d(m, a, b);
                if d < px && best.is_none_or(|x| d < x.0) {
                    best = Some((d, *hit));
                }
            }
        }
        best.map(|(_, h)| &self.hits[h as usize])
    }

    /// Dimension or text under the cursor (they are drawn over everything).
    pub fn screen_annot(&self, cam: &Camera, m: DVec2, px: f64) -> Option<&Hit> {
        self.annots
            .iter()
            .filter_map(|(a, t, h)| crate::annot::screen_distance(cam, a, t, m).map(|d| (d, *h)))
            .filter(|(d, _)| *d < px)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, h)| &self.hits[h as usize])
    }

    /// Guide (line or point) under the cursor: (screen distance, point, hit).
    pub fn screen_guide(&self, cam: &Camera, m: DVec2, px: f64) -> Option<(f64, DVec3, &Hit)> {
        let mut best: Option<(f64, DVec3, u32)> = None;
        for &(p, hit) in &self.guide_points {
            if let Some(s) = cam.project(p) {
                let d = s.distance(m);
                if d < px.max(POINT_PX * 0.6) && best.is_none_or(|b| d < b.0) {
                    best = Some((d, p, hit));
                }
            }
        }
        if let Some((d, p, g)) = self.screen_guides(cam, m, px).first()
            && best.is_none_or(|b| *d < b.0)
        {
            best = Some((*d, *p, g.hit));
        }
        best.map(|(d, p, h)| (d, p, &self.hits[h as usize]))
    }

    /// Like `hits_in_rect`, with a free-form loop (the Lasso).
    pub fn hits_in_poly(&self, cam: &Camera, poly: &[DVec2], crossing: bool) -> Vec<&Hit> {
        let inside = |p: DVec2| esq_core::math::point_in_polygon_2d(p, poly);
        let n = poly.len();
        let crosses = |a: Option<DVec2>, b: Option<DVec2>| {
            let (Some(a), Some(b)) = (a, b) else { return false };
            (0..n).any(|i| segments_cross(a, b, poly[i], poly[(i + 1) % n]))
        };
        let mut by_hit: std::collections::HashMap<u32, (bool, bool)> = std::collections::HashMap::new();
        let mut note = |h: u32, pts: &[DVec3]| {
            let proj: Vec<Option<DVec2>> = pts.iter().map(|p| cam.project(*p)).collect();
            let all = proj.iter().all(|p| p.is_some_and(inside));
            let any = proj.iter().any(|p| p.is_some_and(inside))
                || (crossing && proj.len() == 2 && crosses(proj[0], proj[1]));
            let e = by_hit.entry(h).or_insert((true, false));
            e.0 &= all;
            e.1 |= any;
        };
        for e in &self.edges {
            note(e.hit, &[e.a, e.b]);
        }
        for t in &self.tris {
            note(t.hit, &t.v);
        }
        for &(p, hit) in &self.guide_points {
            note(hit, &[p]);
        }
        for g in self.guides.iter().filter(|g| !g.infinite) {
            note(g.hit, &[g.pos, g.pos + g.dir]);
        }
        for (r, hit) in &self.sections {
            note(*hit, r);
        }
        for &(p, _, hit) in &self.snaps {
            note(hit, &[p]);
        }
        for (segs, hit) in &self.spaces {
            for &(a, b) in segs {
                note(*hit, &[a, b]);
            }
        }
        let mut out: Vec<&Hit> = by_hit
            .into_iter()
            .filter(|(_, (all, any))| if crossing { *any } else { *all })
            .map(|(h, _)| &self.hits[h as usize])
            .collect();
        out.sort_by_key(|h| format!("{:?}", h.item));
        out
    }

    /// For each hit seen in the window: (all its primitives inside, any inside or crossed).
    fn rect_flags(
        &self,
        cam: &Camera,
        min: DVec2,
        max: DVec2,
        crossing: bool,
    ) -> std::collections::HashMap<u32, (bool, bool)> {
        let inside = |p: DVec2| p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y;
        let mut by_hit: std::collections::HashMap<u32, (bool, bool)> = std::collections::HashMap::new();
        let mut note = |h: u32, pts: &[DVec3]| {
            let proj: Vec<Option<DVec2>> = pts.iter().map(|p| cam.project(*p)).collect();
            let all = proj.iter().all(|p| p.is_some_and(inside));
            let any = proj.iter().any(|p| p.is_some_and(inside))
                || (crossing && proj.len() == 2 && segment_hits_rect(proj[0], proj[1], min, max));
            let e = by_hit.entry(h).or_insert((true, false));
            e.0 &= all;
            e.1 |= any;
        };
        for e in &self.edges {
            note(e.hit, &[e.a, e.b]);
        }
        for t in &self.tris {
            note(t.hit, &t.v);
        }
        for &(p, hit) in &self.guide_points {
            note(hit, &[p]);
        }
        for (r, hit) in &self.sections {
            note(*hit, r);
        }
        for &(p, _, hit) in &self.snaps {
            note(hit, &[p]);
        }
        for (segs, hit) in &self.spaces {
            for &(a, b) in segs {
                note(*hit, &[a, b]);
            }
        }
        for (a, _, h) in &self.annots {
            use esq_core::model::AnnotKind;
            match &a.kind {
                AnnotKind::Linear { a, b, offset } => note(*h, &[*a, *b, *a + *offset, *b + *offset]),
                AnnotKind::Text { point, leader } => note(*h, &[*point, *point + *leader]),
                AnnotKind::ScreenText { .. } => {}
            }
        }
        let mut crossed = Vec::new();
        for g in &self.guides {
            if g.infinite {
                // An infinite guide is never inside a window; a crossing box catches it.
                if crossing {
                    let k = cam.distance * 20.0;
                    let a = cam.project(g.pos - g.dir * k).or_else(|| cam.project(g.pos));
                    let b = cam.project(g.pos + g.dir * k).or_else(|| cam.project(g.pos));
                    if segment_hits_rect(a, b, min, max) {
                        crossed.push(g.hit);
                    }
                }
            } else {
                note(g.hit, &[g.pos, g.pos + g.dir]);
            }
        }
        for h in crossed {
            by_hit.insert(h, (false, true));
        }
        by_hit
    }

    pub fn hits_in_rect(&self, cam: &Camera, min: DVec2, max: DVec2, crossing: bool) -> Vec<&Hit> {
        let mut out: Vec<&Hit> = self
            .rect_flags(cam, min, max, crossing)
            .into_iter()
            .filter(|(_, (all, any))| if crossing { *any } else { *all })
            .map(|(h, _)| &self.hits[h as usize])
            .collect();
        out.sort_by_key(|h| format!("{:?}", h.item));
        out
    }

    /// What a window picks among the entities of context `ctx` (`PickHelper#window_pick`):
    /// an instance counts as a whole, inside when all it shows is, crossing
    /// when any of it is.
    pub fn items_in_rect(
        &self,
        cam: &Camera,
        min: DVec2,
        max: DVec2,
        crossing: bool,
        ctx: &[InstanceId],
    ) -> Vec<EntityRef> {
        let mut by_item: std::collections::BTreeMap<EntityRef, (bool, bool)> = Default::default();
        for (h, (all, any)) in self.rect_flags(cam, min, max, crossing) {
            if let Some(e) = self.hits[h as usize].selectable(ctx) {
                let f = by_item.entry(e).or_insert((true, false));
                f.0 &= all;
                f.1 |= any;
            }
        }
        by_item.into_iter().filter(|(_, (all, any))| if crossing { *any } else { *all }).map(|(e, _)| e).collect()
    }
}

fn segment_hits_rect(a: Option<DVec2>, b: Option<DVec2>, min: DVec2, max: DVec2) -> bool {
    let (Some(a), Some(b)) = (a, b) else { return false };
    let corners = [min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)];
    (0..4).any(|i| segments_cross(a, b, corners[i], corners[(i + 1) % 4]))
}

fn segments_cross(p: DVec2, p2: DVec2, q: DVec2, q2: DVec2) -> bool {
    let d = |a: DVec2, b: DVec2, c: DVec2| (b - a).perp_dot(c - a);
    let (d1, d2, d3, d4) = (d(q, q2, p), d(q, q2, p2), d(p, p2, q), d(p, p2, q2));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

pub fn dist_point_segment_2d(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = if ab.length_squared() < 1e-12 { 0.0 } else { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) };
    p.distance(a + ab * t)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapKind {
    Endpoint,
    Midpoint,
    OnEdge,
    OnFace,
    Origin,
    GuidePoint,
    Intersection,
    OnGuide,
    /// Along an axis from the reference point: 0 = red, 1 = green, 2 = blue.
    Axis(usize),
    Parallel,
    Perpendicular,
    /// An intersection of the modeling grid (`CInputPointGridPoint`).
    GridPoint,
    /// A point the Move tool takes a selected group (`true`) or component by
    /// (`tools/move_handles.rs`).
    Handle(Handle, bool),
    Ground,
    Free,
}

/// What a selected group or component is taken by with the Move tool (CMoveTool's grips and
/// snaps; the names are those of SketchUp's tooltips).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Corner,
    Midpoint,
    SideCenter,
    Center,
    Origin,
    /// A snap of the component (`Sketchup::Snap`).
    Snap,
}

impl SnapKind {
    pub fn label(self) -> &'static str {
        match self {
            SnapKind::Handle(h, group) => match (h, group) {
                (Handle::Corner, false) => "Coin du composant",
                (Handle::Corner, true) => "Angle du groupe",
                (Handle::Midpoint, false) => "Point médian du composant",
                (Handle::Midpoint, true) => "Point médian du groupe",
                (Handle::SideCenter, false) => "Centre latéral du composant",
                (Handle::SideCenter, true) => "Centre latéral du groupe",
                (Handle::Center, false) => "Centre du composant",
                (Handle::Center, true) => "Centre du groupe",
                (Handle::Origin, false) => "Origine dans le composant",
                (Handle::Origin, true) => "Origine dans le groupe",
                (Handle::Snap, _) => "Ancrage",
            },
            SnapKind::Endpoint => "Extrémité",
            SnapKind::Midpoint => "Milieu",
            SnapKind::OnEdge => "Sur l'arête",
            SnapKind::OnFace => "Sur la face",
            SnapKind::Origin => "Origine",
            SnapKind::GuidePoint => "Point de construction",
            SnapKind::Intersection => "Intersection",
            SnapKind::OnGuide => "Sur la ligne de construction",
            SnapKind::Axis(0) => "Sur l'axe rouge",
            SnapKind::Axis(1) => "Sur l'axe vert",
            SnapKind::Axis(_) => "Sur l'axe bleu",
            SnapKind::Parallel => "Parallèle à l'arête",
            SnapKind::Perpendicular => "Perpendiculaire à l'arête",
            SnapKind::GridPoint => "Point de grille",
            SnapKind::Ground | SnapKind::Free => "",
        }
    }

    /// Marker colour (sRGB).
    pub fn color(self) -> [u8; 3] {
        match self {
            SnapKind::Endpoint => [0, 170, 0],
            SnapKind::Midpoint => [0, 190, 210],
            SnapKind::OnEdge => [220, 0, 0],
            SnapKind::OnFace => [30, 60, 255],
            SnapKind::Origin => [230, 160, 0],
            SnapKind::GuidePoint | SnapKind::Intersection => [30, 30, 30],
            SnapKind::OnGuide => [90, 90, 90],
            // Preferences › Accessibility.
            SnapKind::Axis(i) => crate::settings::axis_color(i),
            SnapKind::Parallel | SnapKind::Perpendicular => crate::settings::parallel_color(),
            SnapKind::GridPoint => [40, 40, 40],
            SnapKind::Handle(Handle::Snap, _) => [230, 120, 0],
            SnapKind::Handle(..) => [0, 170, 0],
            SnapKind::Ground | SnapKind::Free => [60, 60, 60],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Snap {
    pub pos: DVec3,
    pub kind: SnapKind,
    /// Face under the cursor, if any (world normal).
    pub face: Option<(Hit, DVec3)>,
    pub edge: Option<(Hit, (DVec3, DVec3))>,
    /// Dotted line to draw from the reference point (axis or locked direction).
    pub guide: Option<(DVec3, DVec3, [u8; 3])>,
}

/// A direction the user locked with Shift or the arrow keys.
#[derive(Clone, Copy, Debug)]
pub struct Lock {
    pub dir: DVec3,
    pub color: [u8; 3],
    pub kind: SnapKind,
}

/// The last edge the cursor rested on, which "teaches" the parallel and perpendicular
/// inferences, as in SketchUp.
#[derive(Clone, Copy, Debug)]
pub struct Recent {
    pub dir: DVec3,
    /// Normal of the face under the cursor when the edge was hovered (or the ground).
    pub normal: DVec3,
}

pub struct InferCx<'a> {
    pub cam: &'a Camera,
    pub pick: &'a PickCache,
    /// Axes of the active context (columns = red, green, blue; translation = origin).
    pub axes: DMat4,
    pub recent: Option<Recent>,
}

/// The linear inferences offered from the reference point (the Line tool's Alt cycles them,
/// after CSketchTool: "All On", "Parallel/Perpendicular Only", "All Off").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Linear {
    /// Along the axes, parallel and perpendicular to the last edge.
    #[default]
    All,
    /// Parallel and perpendicular to the last edge only (no axis).
    ParallelPerpendicular,
    /// None: only the points, edges and faces under the cursor.
    Off,
}

impl InferCx<'_> {
    /// The point the cursor designates. `reference` enables axis inferences from that point,
    /// `plane` replaces the ground plane when nothing is under the cursor.
    pub fn infer(&self, m: DVec2, reference: Option<DVec3>, lock: Option<Lock>, plane: Option<Plane>) -> Snap {
        self.infer_linear(m, reference, lock, plane, Linear::All)
    }

    /// [`InferCx::infer`] with only some of the linear inferences from `reference`.
    pub fn infer_linear(
        &self,
        m: DVec2,
        reference: Option<DVec3>,
        lock: Option<Lock>,
        plane: Option<Plane>,
        linear: Linear,
    ) -> Snap {
        let cam = self.cam;
        let (ro, rd) = cam.ray(m);
        let face = self.pick.ray_face(ro, rd);
        // A point is hidden when a face sits between it and the eye.
        let visible = |p: DVec3| {
            let (o, d, dist) = cam.ray_to(p);
            match self.pick.ray_face(o, d) {
                Some((t, _, _)) => t >= dist - (dist * 1e-4).max(0.5),
                None => true,
            }
        };
        let face_info = face.as_ref().map(|(_, h, n)| ((*h).clone(), *n));

        // Point inferences.
        let pj = cam.projector();
        let mut best_pt: Option<(f64, DVec3, SnapKind)> = None;
        let consider = |p: DVec3, kind: SnapKind, best: &mut Option<(f64, DVec3, SnapKind)>| {
            if let Some(s) = pj.project(p) {
                let d = s.distance(m);
                if d < POINT_PX && best.is_none_or(|b| d < b.0 - 0.5) && visible(p) {
                    *best = Some((d, p, kind));
                }
            }
        };
        // Endpoints, then the middles of the edges: those within reach of the cursor, in
        // that order (the others cannot be chosen).
        let n = self.pick.points.len();
        for i in self.pick.near_points(&pj, m) {
            if (i as usize) < n {
                consider(self.pick.points[i as usize], SnapKind::Endpoint, &mut best_pt);
            } else {
                consider(self.pick.mids[i as usize - n], SnapKind::Midpoint, &mut best_pt);
            }
        }
        consider(self.axes.transform_point3(DVec3::ZERO), SnapKind::Origin, &mut best_pt);
        for &(p, _) in &self.pick.guide_points {
            consider(p, SnapKind::GuidePoint, &mut best_pt);
        }

        let edge = self.pick.screen_edge(cam, m, EDGE_PX).filter(|e| visible(e.1));
        let edge_info = edge.as_ref().map(|e| (e.2.clone(), e.3));

        // Guides near the cursor, and where they cross each other or the edge under it.
        let guides: Vec<(f64, DVec3, PGuide)> = self
            .pick
            .screen_guides(cam, m, EDGE_PX * 1.5)
            .into_iter()
            .filter(|g| visible(g.1))
            .map(|(d, p, g)| (d, p, *g))
            .collect();
        for (i, (_, _, g)) in guides.iter().enumerate() {
            for (_, _, h) in &guides[i + 1..] {
                if let Some(x) = line_line_point(g.pos, g.dir, h.pos, h.dir)
                    && g.contains(x)
                    && h.contains(x)
                {
                    consider(x, SnapKind::Intersection, &mut best_pt);
                }
            }
            if let Some((_, _, _, (a, b))) = &edge
                && let Some(x) = line_line_point(g.pos, g.dir, *a, *b - *a)
                && g.contains(x)
                && project_on_segment(x, *a, *b).1 < TOL * 4.0
            {
                consider(x, SnapKind::Intersection, &mut best_pt);
            }
        }

        // Grid snapping: the grid's nearest intersection, after the model's own points.
        if best_pt.is_none()
            && let Some(gs) = &self.pick.grid
            && let Some(p) = gs.at_ray(ro, rd)
        {
            consider(p, SnapKind::GridPoint, &mut best_pt);
        }

        if let (Some(r), Some(lock)) = (reference, lock) {
            let pos = match best_pt {
                Some((_, p, _)) => r + lock.dir * (p - r).dot(lock.dir),
                None => r + lock.dir * line_closest_to_ray(r, lock.dir, ro, rd).unwrap_or(0.0),
            };
            return Snap { pos, kind: lock.kind, face: face_info, edge: edge_info, guide: Some((r, pos, lock.color)) };
        }

        if let Some((_, p, kind)) = best_pt {
            return Snap { pos: p, kind, face: face_info, edge: edge_info, guide: None };
        }

        // Directions from the reference point: the axes, then parallel or perpendicular to
        // the last edge the cursor rested on.
        if let Some(r) = reference.filter(|_| linear != Linear::Off) {
            let axis = |i: usize| self.axes.col(i).truncate().normalize();
            let mut dirs: Vec<(DVec3, SnapKind)> = (0..3).map(|i| (axis(i), SnapKind::Axis(i))).collect();
            if linear == Linear::ParallelPerpendicular {
                dirs.clear();
            }
            if let Some(rec) = self.recent {
                let push = |d: DVec3, kind: SnapKind, dirs: &mut Vec<(DVec3, SnapKind)>| {
                    let d = d.normalize_or_zero();
                    if d != DVec3::ZERO && !dirs.iter().any(|(x, _)| x.dot(d).abs() > 1.0 - 1e-6) {
                        dirs.push((d, kind));
                    }
                };
                push(rec.dir, SnapKind::Parallel, &mut dirs);
                push(rec.dir.cross(rec.normal), SnapKind::Perpendicular, &mut dirs);
                push(rec.dir.cross(axis(2)), SnapKind::Perpendicular, &mut dirs);
            }
            let mut best_dir: Option<(f64, DVec3, SnapKind)> = None;
            for (dir, kind) in dirs {
                let Some(t) = line_closest_to_ray(r, dir, ro, rd) else { continue };
                let p = r + dir * t;
                if let Some(s) = cam.project(p) {
                    let d = s.distance(m);
                    if d < AXIS_PX && best_dir.is_none_or(|b| d < b.0) {
                        best_dir = Some((d, p, kind));
                    }
                }
            }
            if let Some((d, p, kind)) = best_dir
                && edge.as_ref().is_none_or(|e| d < e.0)
            {
                return Snap { pos: p, kind, face: face_info, edge: edge_info, guide: Some((r, p, kind.color())) };
            }
        }

        if let Some((_, p, _, _)) = edge {
            return Snap { pos: p, kind: SnapKind::OnEdge, face: face_info, edge: edge_info, guide: None };
        }
        if let Some((_, p, g)) = guides.first() {
            let line = Some((self.pick.hits[g.hit as usize].clone(), (g.pos, g.pos + g.dir)));
            return Snap { pos: *p, kind: SnapKind::OnGuide, face: face_info, edge: line, guide: None };
        }
        if let Some((t, _, _)) = face {
            return Snap { pos: ro + rd * t, kind: SnapKind::OnFace, face: face_info, edge: None, guide: None };
        }
        let ground = plane.unwrap_or_else(|| {
            Plane::from_point_normal(self.axes.transform_point3(DVec3::ZERO), self.axes.col(2).truncate())
        });
        if let Some(t) = ground.intersect_ray(ro, rd).filter(|t| *t > 0.0) {
            return Snap { pos: ro + rd * t, kind: SnapKind::Ground, face: None, edge: None, guide: None };
        }
        let fallback = Plane::from_point_normal(cam.target, cam.forward());
        let t = fallback.intersect_ray(ro, rd).unwrap_or(cam.distance);
        Snap { pos: ro + rd * t, kind: SnapKind::Free, face: None, edge: None, guide: None }
    }
}

/// An edge the cursor rests on becomes the reference for parallel/perpendicular inferences.
pub fn recent_from(s: &Snap) -> Option<Recent> {
    if !matches!(s.kind, SnapKind::OnEdge | SnapKind::Midpoint) {
        return None;
    }
    let (_, (a, b)) = s.edge.as_ref()?;
    let dir = (*b - *a).normalize_or_zero();
    let normal = s.face.as_ref().map(|f| f.1).unwrap_or(DVec3::Z);
    (dir != DVec3::ZERO).then_some(Recent { dir, normal })
}

/// Where two lines (point, direction) meet, if they do (within tolerance).
pub fn line_line_point(p: DVec3, d: DVec3, q: DVec3, e: DVec3) -> Option<DVec3> {
    let (d, e) = (d.normalize_or_zero(), e.normalize_or_zero());
    if d.cross(e).length_squared() < 1e-12 {
        return None;
    }
    // Closest points between the two lines.
    let w = p - q;
    let (b, dd, ee) = (d.dot(e), d.dot(w), e.dot(w));
    let denom = 1.0 - b * b;
    let s = (b * ee - dd) / denom;
    let t = (ee - b * dd) / denom;
    let (x, y) = (p + d * s, q + e * t);
    (x.distance(y) < TOL * 4.0).then(|| (x + y) * 0.5)
}

/// Projection of a point on segment (for "on edge" distances in tools).
pub fn closest_on_segment(p: DVec3, a: DVec3, b: DVec3) -> DVec3 {
    let (t, _) = project_on_segment(p, a, b);
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Squares at various heights, some standing, on a `n` × `n` grid, and the same squares
    /// again in a component placed twice (one copy turned): many triangles, edges and points
    /// in front of one another.
    fn model(n: usize) -> Model {
        let mut m = Model::new();
        let square = |e: &mut esq_core::model::Entities, p: DVec3, u: DVec3, v: DVec3| {
            let c = [p, p + u, p + u + v, p + v].map(|q| e.add_vertex_raw(q));
            for k in 0..4 {
                e.add_edge_raw(c[k], c[(k + 1) % 4]);
            }
            e.add_face_raw(c.to_vec(), Vec::new());
        };
        for i in 0..n {
            for j in 0..n {
                let p = DVec3::new(i as f64 * 120.0, j as f64 * 120.0, ((i * 7 + j * 3) % 5) as f64 * 40.0);
                if (i + j) % 4 == 0 {
                    square(&mut m.root, p, DVec3::X * 100.0, DVec3::Z * 100.0);
                } else {
                    square(&mut m.root, p, DVec3::X * 100.0, DVec3::Y * 100.0);
                }
            }
        }
        let d = m.defs.insert(esq_core::model::Definition {
            name: "Carrés".into(),
            is_group: false,
            description: String::new(),
            entities: Default::default(),
            attributes: Default::default(),
            ext: Default::default(),
        });
        for i in 0..n / 2 {
            let p = DVec3::new(i as f64 * 200.0, 0.0, 0.0);
            square(&mut m.defs[d].entities, p, DVec3::X * 150.0, DVec3::new(0.0, 80.0, 60.0));
        }
        for xform in [DMat4::from_translation(DVec3::new(0.0, 300.0, 500.0)), DMat4::from_rotation_z(0.3)] {
            m.root.instances.insert(esq_core::model::Instance {
                def: d,
                xform,
                name: String::new(),
                material: None,
                layer: 0,
                hidden: false,
                locked: false,
                attributes: Default::default(),
                ext: Default::default(),
            });
        }
        m
    }

    fn camera(n: usize) -> Camera {
        Camera {
            size: DVec2::new(800.0, 600.0),
            target: DVec3::new(n as f64 * 60.0, n as f64 * 60.0, 50.0),
            distance: n as f64 * 200.0,
            yaw: (-57.0f64).to_radians(),
            pitch: 30.0f64.to_radians(),
            ..Default::default()
        }
    }

    /// What `ray_face` found before the tree: every triangle tried, in order.
    fn ray_face_scan(c: &PickCache, o: DVec3, d: DVec3) -> Option<(f64, u32)> {
        let mut best: Option<(f64, u32)> = None;
        for (i, t) in c.tris.iter().enumerate() {
            if let Some(dist) = ray_triangle(o, d, t.v[0], t.v[1], t.v[2])
                && best.is_none_or(|b| dist < b.0)
                && !c.cuts[t.cuts as usize].iter().any(|s| s.cuts(o + d * dist))
            {
                best = Some((dist, i as u32));
            }
        }
        best
    }

    /// What `screen_edge` found before the tree.
    fn screen_edge_scan(c: &PickCache, cam: &Camera, m: DVec2, px: f64) -> Option<(f64, DVec3, u32)> {
        let (ro, rd) = cam.ray(m);
        let mut best: Option<(f64, DVec3, u32)> = None;
        for e in &c.edges {
            let (Some(a2), Some(b2)) = (cam.project(e.a), cam.project(e.b)) else { continue };
            let d = dist_point_segment_2d(m, a2, b2);
            if d < px && best.is_none_or(|b| d < b.0) {
                let dir = e.b - e.a;
                let t = line_closest_to_ray(e.a, dir, ro, rd).unwrap_or(0.0).clamp(0.0, 1.0);
                best = Some((d, e.a + dir * t, e.hit));
            }
        }
        best
    }

    #[test]
    fn the_tree_finds_what_a_full_scan_finds() {
        let n = 24;
        let m = model(n);
        let c = PickCache::build(&m, &ViewStyle::default());
        let cam = camera(n);
        let pj = cam.projector();
        let (mut faces, mut edges, mut points) = (0, 0, 0);
        for k in 0..900 {
            let px = DVec2::new((k % 30) as f64 * 800.0 / 30.0 + 3.7, (k / 30) as f64 * 600.0 / 30.0 + 1.3);
            let (o, d) = cam.ray(px);
            let fast = c.ray_face(o, d).map(|(t, h, nrm)| (t.to_bits(), h.clone(), nrm));
            let slow = ray_face_scan(&c, o, d).map(|(t, i)| {
                let tri = &c.tris[i as usize];
                (t.to_bits(), c.hits[tri.hit as usize].clone(), tri.normal)
            });
            assert_eq!(fast, slow, "rayon {k}");
            faces += fast.is_some() as usize;
            let fast = c.screen_edge(&cam, px, EDGE_PX).map(|(d, p, h, _)| (d.to_bits(), p, h.clone()));
            let slow =
                screen_edge_scan(&c, &cam, px, EDGE_PX).map(|(d, p, h)| (d.to_bits(), p, c.hits[h as usize].clone()));
            assert_eq!(fast, slow, "arête {k}");
            edges += fast.is_some() as usize;
            let all: Vec<u32> = c
                .points
                .iter()
                .chain(&c.mids)
                .enumerate()
                .filter(|(_, p)| cam.project(**p).is_some_and(|s| s.distance(px) < POINT_PX))
                .map(|(i, _)| i as u32)
                .collect();
            assert_eq!(c.near_points(&pj, px), all, "points {k}");
            points += all.len();
        }
        // The cursor met something often enough for the comparison to mean something.
        assert!(faces > 150 && edges > 100 && points > 100, "{faces} faces, {edges} arêtes, {points} points");
    }

    /// Components that always face the camera turn with each image of an orbit: the
    /// picking cache follows them only when something looks under the cursor, and is then
    /// what rebuilding it at each image gave.
    #[test]
    fn an_orbit_does_not_rebuild_the_picking_until_it_is_used() {
        let mut m = model(10);
        let d = m.defs.keys().next().unwrap();
        m.defs[d].ext.behavior.always_face_camera = true;
        let mut app = crate::app::EsquisseApp::bare(None, None);
        app.doc = crate::doc::Document::new(m);
        app.cam = camera(10);
        app.follow_eye();
        let before = crate::perf::get().pick_builds;
        for _ in 0..30 {
            app.cam.orbit_with(5.0, 0.0, true);
            app.follow_eye();
        }
        assert_eq!(crate::perf::get().pick_builds - before, 0, "reconstruit pendant l'orbite");
        app.ensure_pick();
        assert_eq!(crate::perf::get().pick_builds - before, 1);
        let fresh = PickCache::build(&app.doc.model, &app.style);
        for k in 0..200 {
            let px = DVec2::new((k % 20) as f64 * 40.0 + 7.0, (k / 20) as f64 * 60.0 + 11.0);
            let (o, d) = app.cam.ray(px);
            let a = app.pick.ray_face(o, d).map(|(t, h, n)| (t.to_bits(), h.clone(), n));
            let b = fresh.ray_face(o, d).map(|(t, h, n)| (t.to_bits(), h.clone(), n));
            assert_eq!(a, b, "rayon {k}");
        }
    }

    /// A cache built for another eye and the triangles that turn, placed for this eye, hide
    /// exactly what a cache built for this eye hides.
    #[test]
    fn what_turns_with_the_eye_hides_as_a_new_cache_would() {
        let mut m = model(12);
        let d = m.defs.keys().next().unwrap();
        m.defs[d].ext.behavior.always_face_camera = true;
        let style = ViewStyle::default();
        let cam = camera(12);
        esq_core::walk::set_eye(Some(DVec3::new(-3000.0, 500.0, 900.0)));
        let stale = PickCache::build(&m, &style);
        esq_core::walk::set_eye(Some(cam.eye()));
        let fresh = PickCache::build(&m, &style);
        let turning = Turning::build(&m, &style);
        assert!(!turning.tris.is_empty());
        let (mut hidden, mut seen) = (0, 0);
        for k in 0..900 {
            let px = DVec2::new((k % 30) as f64 * 800.0 / 30.0 + 2.1, (k / 30) as f64 * 600.0 / 30.0 + 0.7);
            let (o, d) = cam.ray(px);
            for below in [500.0, 1500.0, 2500.0, 4000.0] {
                let new = fresh.ray_face(o, d).is_some_and(|(t, _, _)| t < below);
                assert_eq!(stale.hides(o, d, below, Some(&turning)), new, "rayon {k}, {below}");
                assert_eq!(fresh.hides(o, d, below, None), new);
                if new { hidden += 1 } else { seen += 1 }
            }
        }
        esq_core::walk::set_eye(None);
        assert!(hidden > 100 && seen > 100, "{hidden} cachés, {seen} vus");
    }

    #[test]
    fn an_inference_looks_at_a_small_part_of_the_model() {
        let n = 120;
        let m = model(n);
        let c = PickCache::build(&m, &ViewStyle::default());
        let cam = camera(n);
        let items = (c.tris.len() + c.edges.len() + c.points.len() + c.mids.len()) as u64;
        let cx = InferCx { cam: &cam, pick: &c, axes: DMat4::IDENTITY, recent: None };
        let mut worst = 0;
        for k in 0..20 {
            let px = DVec2::new(40.0 + k as f64 * 37.0, 300.0 + (k % 5) as f64 * 31.0);
            let before = crate::perf::get().pick_visits;
            let s = cx.infer(px, Some(DVec3::ZERO), None, None);
            assert_ne!(s.kind, SnapKind::Free);
            worst = worst.max(crate::perf::get().pick_visits - before);
        }
        // Without the trees every inference looked at every item, several times.
        assert!(worst * 20 < items, "{worst} éléments regardés sur {items}");
    }
}
