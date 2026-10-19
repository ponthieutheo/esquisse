//! Solid tools (Tools › Outer Shell, Tools › Solid Tools): booleans between groups or
//! components that are solids, i.e. closed volumes where every edge bounds two faces.
//!
//! Both solids are copied into the active context's coordinates, cut along the lines where
//! their faces cross, and each piece is classified as inside, outside or on the other
//! solid. The kept pieces are joined into a new group.

use std::collections::HashSet;

use glam::{DMat4, DVec2, DVec3};

use crate::intersect::{Poly, crossing};
use crate::math::{TOL, point_in_polygon_2d, ray_triangle};
use crate::model::{DefId, Definition, Entities, EntityRef, FaceId, Instance, InstanceId, Model, VertexId};
use crate::tess::face_triangles;

/// The six solid tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolidOp {
    /// Everything inside either solid; both are replaced.
    Union,
    /// What both solids share; both are replaced.
    Intersect,
    /// The second solid minus the first; the first is removed.
    Subtract,
    /// The second solid minus the first; the first stays.
    Trim,
    /// Three pieces: each solid minus the other, and what they share.
    Split,
}

/// Where a face lies relative to a solid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Inside,
    Outside,
    /// On the solid's surface, facing the same way.
    OnSame,
    /// On the solid's surface, facing the other way.
    OnOpposite,
}

/// Whether an entities collection is a closed volume: it has faces, and every edge bounds
/// exactly two of them.
pub fn is_solid(ents: &Entities) -> bool {
    !ents.faces.is_empty() && ents.edges.values().all(|e| e.faces.len() == 2)
}

/// Volume enclosed by a solid (positive when its faces look outwards).
pub fn volume(ents: &Entities) -> f64 {
    ents.faces.keys().flat_map(|f| face_triangles(ents, f)).map(|[a, b, c]| a.dot(b.cross(c))).sum::<f64>() / 6.0
}

/// A face's geometry, kept to rebuild it in another collection.
struct FaceCopy {
    loops: Vec<Vec<DVec3>>,
    front: Option<crate::model::MaterialId>,
    back: Option<crate::model::MaterialId>,
    layer: usize,
}

fn copy_face(ents: &Entities, f: FaceId) -> FaceCopy {
    let face = &ents.faces[f];
    FaceCopy {
        loops: face.loops().map(|l| ents.loop_points(l)).collect(),
        front: face.front,
        back: face.back,
        layer: face.layer,
    }
}

fn vertex(ents: &mut Entities, p: DVec3) -> VertexId {
    ents.find_vertex(p).unwrap_or_else(|| ents.add_vertex_raw(p))
}

/// Adds a face (optionally turned over) with its own vertices welded to existing ones.
fn paste_face(ents: &mut Entities, c: &FaceCopy, reversed: bool) -> Option<FaceId> {
    let mut loops: Vec<Vec<VertexId>> = c.loops.iter().map(|l| l.iter().map(|p| vertex(ents, *p)).collect()).collect();
    if reversed {
        loops.iter_mut().for_each(|l| l.reverse());
    }
    let outer = loops.remove(0);
    let f = ents.add_face_raw(outer, loops)?;
    let face = &mut ents.faces[f];
    (face.front, face.back) = if reversed { (c.back, c.front) } else { (c.front, c.back) };
    face.layer = c.layer;
    Some(f)
}

/// A solid copied into the active context's coordinates, with the soft edges it had.
struct Body {
    ents: Entities,
    soft: Vec<(DVec3, DVec3)>,
}

impl Body {
    fn polys(&self) -> Vec<Poly> {
        self.ents.faces.keys().filter_map(|f| Poly::new(&self.ents, f, &DMat4::IDENTITY)).collect()
    }

    fn triangles(&self) -> Vec<[DVec3; 3]> {
        self.ents.faces.keys().flat_map(|f| face_triangles(&self.ents, f)).collect()
    }
}

/// Where a point lies relative to a solid: on one of its faces (with that face's normal),
/// or inside/outside (by counting crossings along a few rays and taking the majority).
fn locate(p: DVec3, polys: &[Poly], tris: &[[DVec3; 3]]) -> Result<DVec3, bool> {
    for poly in polys {
        if poly.plane.distance(p).abs() < TOL * 2.0 {
            let (u, v) = poly.plane.basis();
            let to2 = |q: DVec3| DVec2::new(q.dot(u), q.dot(v));
            let mut inside = false;
            for l in &poly.loops {
                let l2: Vec<DVec2> = l.iter().map(|q| to2(*q)).collect();
                inside ^= point_in_polygon_2d(to2(p), &l2);
            }
            if inside {
                return Ok(poly.plane.n);
            }
        }
    }
    let dirs =
        [DVec3::new(0.5361, 0.6627, 0.5223), DVec3::new(-0.7123, 0.3012, 0.6398), DVec3::new(0.2113, -0.8121, 0.5440)];
    let votes = dirs
        .iter()
        .filter(|d| {
            let d = d.normalize();
            tris.iter().filter(|t| ray_triangle(p, d, t[0], t[1], t[2]).is_some_and(|x| x > TOL)).count() % 2 == 1
        })
        .count();
    Err(votes >= 2)
}

fn side_of(ents: &Entities, f: FaceId, other_polys: &[Poly], other_tris: &[[DVec3; 3]]) -> Side {
    // The centre of the face's largest triangle lies inside the face.
    let tris = face_triangles(ents, f);
    let Some(t) = tris.iter().max_by(|a, b| {
        let area = |t: &&[DVec3; 3]| (t[1] - t[0]).cross(t[2] - t[0]).length();
        area(a).total_cmp(&area(b))
    }) else {
        return Side::Outside;
    };
    let p = (t[0] + t[1] + t[2]) / 3.0;
    match locate(p, other_polys, other_tris) {
        Ok(n) if n.dot(ents.faces[f].normal) > 0.0 => Side::OnSame,
        Ok(_) => Side::OnOpposite,
        Err(true) => Side::Inside,
        Err(false) => Side::Outside,
    }
}

/// Cuts two bodies where their faces cross, so that every face lies wholly inside, outside
/// or on the other body.
fn cut_together(a: &mut Body, b: &mut Body) {
    let (pa, pb) = (a.polys(), b.polys());
    let mut segs = Vec::new();
    for x in &pa {
        for y in &pb {
            segs.extend(crossing(x, y));
        }
    }
    // Edges of one body lying flat in a face of the other, which no plane crossing finds.
    segs.extend(imprints(&a.ents, &pb));
    segs.extend(imprints(&b.ents, &pa));
    let planes = |e: &Entities| -> Vec<(crate::math::Plane, DVec3)> {
        e.faces.keys().map(|f| (e.face_plane(f), e.faces[f].normal)).collect()
    };
    let (before_a, before_b) = (planes(&a.ents), planes(&b.ents));
    for (p, q) in segs {
        a.ents.add_line(p, q);
        b.ents.add_line(p, q);
    }
    // Drawing the cut closes loops of edges inside each body, which makes new faces there,
    // as drawing does in SketchUp: they are not part of the solid.
    for (body, before) in [(a, before_a), (b, before_b)] {
        let extra: Vec<FaceId> = body
            .ents
            .faces
            .keys()
            .filter(|&f| {
                let (pl, n) = (body.ents.face_plane(f), body.ents.faces[f].normal);
                !before.iter().any(|(q, m)| q.same_as(&pl) && m.dot(n) > 0.0)
            })
            .collect();
        for f in extra {
            body.ents.remove_face(f);
        }
    }
}

/// The parts of `ents`' edges that lie inside a face of `polys`, in its plane.
fn imprints(ents: &Entities, polys: &[Poly]) -> Vec<(DVec3, DVec3)> {
    let mut out = Vec::new();
    for e in ents.edges.values() {
        let (p, q) = (ents.pos(e.v[0]), ents.pos(e.v[1]));
        let len = p.distance(q);
        let d = (q - p) / len;
        for poly in polys {
            if poly.plane.distance(p).abs() > TOL || poly.plane.distance(q).abs() > TOL {
                continue;
            }
            for (t0, t1) in poly.intervals(p, d) {
                let (t0, t1) = (t0.max(0.0), t1.min(len));
                if t1 - t0 > TOL {
                    out.push((p + d * t0, p + d * t1));
                }
            }
        }
    }
    out
}

/// Joins the chosen faces of two bodies into one collection, merging the coplanar pieces
/// the cut left behind, and keeping soft edges soft.
fn assemble(parts: &[(&Body, Vec<(FaceId, bool)>)]) -> Entities {
    let mut out = Entities::default();
    for (body, faces) in parts {
        for &(f, reversed) in faces {
            paste_face(&mut out, &copy_face(&body.ents, f), reversed);
        }
    }
    let seams: Vec<_> = out
        .edges
        .iter()
        .filter(|(_, e)| {
            e.faces.len() == 2 && {
                let (f, g) = (e.faces[0], e.faces[1]);
                out.faces[f].normal.dot(out.faces[g].normal) > 1.0 - 1e-9
                    && out.face_plane(f).same_as(&out.face_plane(g))
            }
        })
        .map(|(k, _)| k)
        .collect();
    if !seams.is_empty() {
        out.erase_edges(&seams);
    }
    let soft: Vec<(DVec3, DVec3)> = parts.iter().flat_map(|(b, _)| b.soft.iter().copied()).collect();
    let on_soft = |p: DVec3| soft.iter().any(|(a, b)| crate::math::project_on_segment(p, *a, *b).1 < TOL);
    let ids: Vec<_> = out.edges.keys().collect();
    for e in ids {
        let (a, b) = out.edge_points(e);
        if on_soft(a) && on_soft(b) && on_soft((a + b) * 0.5) {
            let edge = &mut out.edges[e];
            (edge.soft, edge.smooth) = (true, true);
        }
    }
    out
}

impl Model {
    /// Copy of everything a group/component contains (nested ones included) in the
    /// coordinates of the collection `ctx` holding it; faces without material take the
    /// instance's.
    fn flatten(&self, ctx: Option<DefId>, inst: InstanceId) -> Option<Body> {
        let i = self.entities(ctx).instances.get(inst)?;
        let mut body = Body { ents: Entities::default(), soft: Vec::new() };
        fn rec(
            model: &Model,
            ents: &Entities,
            m: &DMat4,
            mat: Option<crate::model::MaterialId>,
            depth: usize,
            out: &mut Body,
        ) {
            let mirrored = m.determinant() < 0.0;
            for f in ents.faces.keys() {
                let mut c = copy_face(ents, f);
                c.loops.iter_mut().for_each(|l| l.iter_mut().for_each(|p| *p = m.transform_point3(*p)));
                c.front = c.front.or(mat);
                paste_face(&mut out.ents, &c, mirrored);
            }
            for e in ents.edges.values().filter(|e| e.soft) {
                let (a, b) = (ents.pos(e.v[0]), ents.pos(e.v[1]));
                out.soft.push((m.transform_point3(a), m.transform_point3(b)));
            }
            if depth > 16 {
                return;
            }
            for child in ents.instances.values() {
                if let Some(d) = model.defs.get(child.def) {
                    rec(model, &d.entities, &(*m * child.xform), child.material.or(mat), depth + 1, out);
                }
            }
        }
        rec(self, &self.defs.get(i.def)?.entities, &i.xform, i.material, 0, &mut body);
        Some(body)
    }

    /// Whether an instance of the collection `ctx` is a solid group/component.
    pub fn is_solid_instance(&self, ctx: Option<DefId>, inst: InstanceId) -> bool {
        self.flatten(ctx, inst).is_some_and(|b| is_solid(&b.ents))
    }

    /// Volume of a solid group/component, in its context's units (mm³).
    pub fn solid_volume(&self, ctx: Option<DefId>, inst: InstanceId) -> Option<f64> {
        let b = self.flatten(ctx, inst)?;
        is_solid(&b.ents).then(|| volume(&b.ents).abs())
    }

    fn solid_body(&self, ctx: Option<DefId>, inst: InstanceId) -> Result<Body, String> {
        match self.flatten(ctx, inst) {
            Some(b) if is_solid(&b.ents) => Ok(b),
            _ => Err("Ce n'est pas un solide.".into()),
        }
    }

    /// Adds a new group holding `ents` (in `ctx` coordinates) to `ctx`, placed at the
    /// lowest corner of its contents, with the tag of `like`.
    fn add_result_group(&mut self, ctx: Option<DefId>, mut ents: Entities, like: &Instance) -> InstanceId {
        let origin = ents.verts.values().fold(DVec3::splat(f64::INFINITY), |a, v| a.min(v.pos));
        let origin = if origin.is_finite() { origin } else { DVec3::ZERO };
        let to_local = DMat4::from_translation(-origin);
        let all: HashSet<VertexId> = ents.verts.keys().collect();
        ents.transform_vertices(&all, &to_local);
        let name = self.unique_def_name("Groupe");
        let def = self.defs.insert(Definition {
            name,
            is_group: true,
            description: String::new(),
            entities: ents,
            attributes: Default::default(),
            ext: Default::default(),
        });
        self.entities_mut(ctx).instances.insert(Instance {
            def,
            xform: DMat4::from_translation(origin),
            name: String::new(),
            material: None,
            layer: like.layer,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        })
    }

    /// Runs a solid tool on two groups/components of the collection `ctx`. Returns the
    /// groups it made.
    pub fn solid_op(
        &mut self,
        ctx: Option<DefId>,
        first: InstanceId,
        second: InstanceId,
        op: SolidOp,
    ) -> Result<Vec<InstanceId>, String> {
        if first == second {
            return Err("Choisissez deux solides différents.".into());
        }
        let ents = self.entities(ctx);
        let (Some(i1), Some(i2)) = (ents.instances.get(first).cloned(), ents.instances.get(second).cloned()) else {
            return Err("Ce n'est pas un solide.".into());
        };
        if i1.locked || i2.locked {
            return Err("Un des solides est verrouillé.".into());
        }
        let mut a = self.solid_body(ctx, first)?;
        let mut b = self.solid_body(ctx, second)?;
        cut_together(&mut a, &mut b);
        let (pa, pb) = (a.polys(), b.polys());
        let (ta, tb) = (a.triangles(), b.triangles());
        let sa: Vec<(FaceId, Side)> = a.ents.faces.keys().map(|f| (f, side_of(&a.ents, f, &pb, &tb))).collect();
        let sb: Vec<(FaceId, Side)> = b.ents.faces.keys().map(|f| (f, side_of(&b.ents, f, &pa, &ta))).collect();
        let pick = |s: &[(FaceId, Side)], keep: &[Side], reversed: bool| -> Vec<(FaceId, bool)> {
            s.iter().filter(|(_, x)| keep.contains(x)).map(|(f, _)| (*f, reversed)).collect()
        };
        use Side::*;
        let union = || assemble(&[(&a, pick(&sa, &[Outside, OnSame], false)), (&b, pick(&sb, &[Outside], false))]);
        let common = || assemble(&[(&a, pick(&sa, &[Inside, OnSame], false)), (&b, pick(&sb, &[Inside], false))]);
        // `x` minus `y`: what of x lies outside y, closed by y's faces inside x, turned over.
        let minus = |x: &Body, sx: &[(FaceId, Side)], y: &Body, sy: &[(FaceId, Side)]| {
            assemble(&[(x, pick(sx, &[Outside, OnOpposite], false)), (y, pick(sy, &[Inside], true))])
        };
        let results: Vec<(Entities, &Instance)> = match op {
            SolidOp::Union => vec![(union(), &i1)],
            SolidOp::Intersect => vec![(common(), &i1)],
            SolidOp::Subtract | SolidOp::Trim => vec![(minus(&b, &sb, &a, &sa), &i2)],
            SolidOp::Split => vec![(minus(&a, &sa, &b, &sb), &i1), (minus(&b, &sb, &a, &sa), &i2), (common(), &i1)],
        };
        let results: Vec<(Entities, Instance)> =
            results.into_iter().filter(|(e, _)| !e.faces.is_empty()).map(|(e, i)| (e, i.clone())).collect();
        if results.is_empty() && op == SolidOp::Intersect {
            return Err("Les solides ne se chevauchent pas.".into());
        }
        let removed: Vec<EntityRef> = match op {
            SolidOp::Trim => vec![EntityRef::Instance(second)],
            _ => vec![EntityRef::Instance(first), EntityRef::Instance(second)],
        };
        self.entities_mut(ctx).erase(&removed);
        let made = results.into_iter().map(|(e, like)| self.add_result_group(ctx, e, &like)).collect();
        self.purge_unused_defs();
        Ok(made)
    }

    /// Tools › Outer Shell: merges solids into one, keeping only their outer surface.
    pub fn outer_shell(&mut self, ctx: Option<DefId>, insts: &[InstanceId]) -> Result<InstanceId, String> {
        let [first, rest @ ..] = insts else { return Err("Sélectionnez au moins deux solides.".into()) };
        if rest.is_empty() {
            return Err("Sélectionnez au moins deux solides.".into());
        }
        for &i in insts {
            self.solid_body(ctx, i)?;
        }
        let mut acc = *first;
        for &next in rest {
            acc = *self.solid_op(ctx, acc, next, SolidOp::Union)?.first().ok_or("Échec de l'union")?;
        }
        Ok(acc)
    }
}
