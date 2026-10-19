//! Dimensions and Text tools.

use eframe::egui::Color32;
use esq_core::attach::{Attach, AttachTo, Ends};
use esq_core::glam::{DVec2, DVec3};
use esq_core::math::{Plane, TOL};
use esq_core::model::{AnnotKind, Annotation, EntityRef, Units};
use esq_core::tess::face_area;
use esq_core::units::format_length;

use super::{Overlay, Tool, ToolCx, ToolId};
use crate::pick::{Snap, SnapKind};

/// Where the cursor ray meets the plane through `a`–`b` that faces the camera most.
fn facing_point(cx: &ToolCx, m: DVec2, a: DVec3, b: DVec3) -> Option<DVec3> {
    let d = (b - a).normalize_or_zero();
    let f = cx.cam().forward();
    let n = (f - d * f.dot(d)).normalize_or_zero();
    if n == DVec3::ZERO {
        return None;
    }
    let (ro, rd) = cx.cam().ray(m);
    Plane::from_point_normal(a, n).intersect_ray(ro, rd).map(|t| ro + rd * t)
}

/// Adds an annotation to the active context, its ends attached as `ends` says.
fn add_annot(cx: &mut ToolCx, a: Annotation, ends: Ends, label: &str) -> Option<esq_core::model::AnnotId> {
    let x = cx.doc.ctx_xform().inverse();
    let local = a.transformed(&x);
    let mut id = None;
    cx.doc.commit(label, |m, d| {
        let i = m.entities_mut(d).annots.insert(local);
        for (w, e) in ends.into_iter().enumerate() {
            if e.is_some() {
                m.set_attach(d, i, w, e);
            }
        }
        id = Some(i);
    });
    if let Some(i) = id {
        cx.doc.select(vec![EntityRef::Annot(i)]);
    }
    id
}

/// What SketchUp attaches an end to when it is placed on the inference `s` (the input point's
/// entity and its path from the active context, as `CDimensionTool` and `CTextTool` keep
/// them): a vertex for an endpoint, the middle of an edge, a point along an edge or a
/// construction line, a construction point, a point of a face. Nothing for a point of the
/// ground or of space, an intersection, an axis, or what lies outside the open context.
pub(crate) fn attach_of(cx: &ToolCx, m: DVec2, s: &Snap) -> Option<Attach> {
    let model = &cx.doc.model;
    let ctx = &cx.doc.ctx;
    let ents_at = |path: &[esq_core::model::InstanceId]| model.entities(esq_core::walk::path_def(model, path));
    let local = |path: &[esq_core::model::InstanceId]| esq_core::walk::path_xform(model, path).inverse();
    let rel = |path: &[esq_core::model::InstanceId]| path.strip_prefix(ctx.as_slice()).map(|p| p.to_vec());
    let edge_hit = s.edge.as_ref().map(|(h, _)| h);
    let face_hit = s.face.as_ref().map(|(h, _)| h);
    let on = |h: &crate::pick::Hit, f: &dyn Fn(&esq_core::model::Entities, DVec3) -> Option<AttachTo>| {
        let path = rel(&h.path)?;
        let p = local(&h.path).transform_point3(s.pos);
        Some(Attach::new(path, f(ents_at(&h.path), p)?))
    };
    match s.kind {
        SnapKind::Endpoint => {
            // The vertex at the point, among the edge's or the face's.
            let vertex = |h: &crate::pick::Hit| {
                on(h, &|e, p| {
                    let near = |v: &esq_core::model::VertexId| e.verts.get(*v).is_some_and(|x| x.pos.distance(p) < TOL);
                    let vs: Vec<esq_core::model::VertexId> = match h.item {
                        EntityRef::Edge(x) => e.edges.get(x)?.v.to_vec(),
                        EntityRef::Face(x) => e.faces.get(x)?.all_vertices().collect(),
                        _ => Vec::new(),
                    };
                    vs.into_iter().find(near).map(AttachTo::Vertex)
                })
            };
            edge_hit.and_then(vertex).or_else(|| face_hit.and_then(vertex))
        }
        SnapKind::Midpoint | SnapKind::OnEdge => edge_hit.and_then(|h| match h.item {
            EntityRef::Edge(x) => on(h, &|e, p| esq_core::attach::on_edge(e, x, p)),
            _ => None,
        }),
        SnapKind::OnGuide => edge_hit.and_then(|h| match h.item {
            EntityRef::Guide(g) => on(h, &|e, p| esq_core::attach::on_guide(e, g, p)),
            _ => None,
        }),
        SnapKind::GuidePoint => {
            let (_, _, h) = cx.infer.pick.screen_guide(cx.cam(), m, crate::pick::POINT_PX)?;
            match h.item {
                EntityRef::Guide(g) => on(h, &|e, p| {
                    let x = e.guides.get(g)?;
                    (x.is_point() && x.pos.distance(p) < TOL).then_some(AttachTo::GuidePoint(g))
                }),
                _ => None,
            }
        }
        SnapKind::OnFace => face_hit.and_then(|h| match h.item {
            EntityRef::Face(f) => on(h, &|e, p| esq_core::attach::on_face(e, f, p)),
            _ => None,
        }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------

/// Dimensions: click two points (or an edge), then pull the dimension line out.
#[derive(Default)]
pub struct DimensionTool {
    a: Option<DVec3>,
    b: Option<DVec3>,
    offset: DVec3,
    snap: Option<Snap>,
    /// What each end is attached to (the inference it was placed on).
    ends: Ends,
}

impl Tool for DimensionTool {
    fn id(&self) -> ToolId {
        ToolId::Dimension
    }
    fn status(&self) -> String {
        // CDimensionTool.
        match (self.a.is_some(), self.b.is_some()) {
            (false, _) => "Sélectionnez une arête, une courbe ou deux points pour coter, ou faites glisser un élément pour le déplacer.".into(),
            (true, false) => "Sélectionnez le second point de la cotation linéaire.".into(),
            _ => "Placez la cotation.".into(),
        }
    }
    fn vcb_label(&self) -> &'static str {
        "Longueur"
    }
    fn vcb_value(&self, units: Units) -> String {
        match (self.a, self.b, &self.snap) {
            (Some(a), Some(b), _) => format_length(a.distance(b), units),
            (Some(a), None, Some(s)) => format_length(a.distance(s.pos), units),
            _ => String::new(),
        }
    }
    fn busy(&self) -> bool {
        self.a.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let s = cx.snap(m, self.a, None);
        if let (Some(a), Some(b)) = (self.a, self.b)
            && let Some(q) = facing_point(cx, m, a, b)
        {
            let d = (b - a).normalize();
            let v = q - a;
            self.offset = v - d * v.dot(d);
        }
        self.snap = Some(s);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(s) = self.snap.clone() else { return };
        match (self.a, self.b) {
            (None, _) => {
                if s.kind == SnapKind::OnEdge
                    && let Some((_, (p, q))) = s.edge
                {
                    // The edge's two ends, attached to its vertices.
                    self.ends =
                        [p, q].map(|x| attach_of(cx, m, &Snap { pos: x, kind: SnapKind::Endpoint, ..s.clone() }));
                    self.a = Some(p);
                    self.b = Some(q);
                    self.offset = DVec3::ZERO;
                } else {
                    self.ends = [attach_of(cx, m, &s), None];
                    self.a = Some(s.pos);
                }
            }
            (Some(a), None) => {
                if s.pos.distance(a) > TOL {
                    self.ends[1] = attach_of(cx, m, &s);
                    self.b = Some(s.pos);
                    self.offset = DVec3::ZERO;
                }
            }
            (Some(a), Some(b)) => {
                let annot = Annotation {
                    kind: AnnotKind::Linear { a, b, offset: self.offset },
                    text: String::new(),
                    layer: 0,
                    hidden: false,
                    attach: Default::default(),
                };
                add_annot(cx, annot, std::mem::take(&mut self.ends), "Cotation");
                self.a = None;
                self.b = None;
            }
        }
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.a = None;
        self.b = None;
        self.ends = Default::default();
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        match (self.a, self.b, &self.snap) {
            (Some(a), None, Some(s)) => o.dashed(a, s.pos, Color32::from_gray(40), 1.5),
            (Some(a), Some(b), _) => {
                let annot = Annotation {
                    kind: AnnotKind::Linear { a, b, offset: self.offset },
                    text: String::new(),
                    layer: 0,
                    hidden: false,
                    attach: Default::default(),
                };
                let text = crate::annot::label(&annot, o.units);
                let st = crate::annot::default_style(&o.doc.model, &annot);
                crate::annot::draw_styled(&o.painter, o.origin, o.cam, &annot, &text, false, &st);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------

/// Text: click what to point at (the text proposes its length, area or position), then
/// where to put the text; click empty space for a text fixed on the screen.
#[derive(Default)]
pub struct TextTool {
    /// The point clicked, the text proposed for it, and what the point is attached to.
    point: Option<(DVec3, String, Option<Attach>)>,
    snap: Option<Snap>,
    cursor: DVec2,
}

impl TextTool {
    /// What SketchUp writes by default for what was clicked.
    fn default_text(cx: &ToolCx, s: &Snap) -> String {
        let ents = cx.doc.ents();
        let x = cx.doc.ctx_xform();
        let ctx = &cx.doc.ctx;
        match s.kind {
            SnapKind::OnEdge => {
                if let Some((h, (a, b))) = &s.edge
                    && h.selectable(ctx).is_some()
                {
                    return format_length(a.distance(*b), cx.units);
                }
            }
            SnapKind::OnFace => {
                if let Some((h, _)) = &s.face {
                    match h.selectable(ctx) {
                        Some(EntityRef::Face(f)) => {
                            // In the area unit and precision of Model Info › Units.
                            let d = esq_core::units::UnitsDisplay::of(&cx.doc.model);
                            return d.area(face_area(ents, f));
                        }
                        Some(EntityRef::Instance(i)) => return cx.doc.model.defs[ents.instances[i].def].name.clone(),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        let p = x.inverse().transform_point3(s.pos);
        format!("({}; {}; {})", cx.len(p.x), cx.len(p.y), cx.len(p.z))
    }
}

impl Tool for TextTool {
    fn id(&self) -> ToolId {
        ToolId::Text
    }
    fn status(&self) -> String {
        // CTextTool.
        if self.point.is_none() {
            "Sélectionnez l’objet pour y attacher le texte ou le positionner à l’écran.".into()
        } else {
            "Positionnez le texte.".into()
        }
    }
    fn busy(&self) -> bool {
        self.point.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.cursor = m;
        self.snap = Some(cx.snap(m, None, None));
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(s) = self.snap.clone() else { return };
        match self.point.take() {
            None => {
                if matches!(s.kind, SnapKind::Ground | SnapKind::Free) && s.face.is_none() {
                    let size = cx.cam().size;
                    let annot = Annotation {
                        kind: AnnotKind::ScreenText { x: m.x / size.x, y: m.y / size.y },
                        text: "Entrez le texte".into(),
                        layer: 0,
                        hidden: false,
                        attach: Default::default(),
                    };
                    let id = add_annot(cx, annot, Default::default(), "Texte");
                    *cx.edit_text = id;
                } else {
                    self.point = Some((s.pos, Self::default_text(cx, &s), attach_of(cx, m, &s)));
                }
            }
            Some((p, text, at)) => {
                let cam = cx.cam();
                let pl = Plane::from_point_normal(p, cam.forward());
                let (ro, rd) = cam.ray(m);
                let q = pl.intersect_ray(ro, rd).map(|t| ro + rd * t).unwrap_or(p);
                let annot = Annotation {
                    kind: AnnotKind::Text { point: p, leader: q - p },
                    text,
                    layer: 0,
                    hidden: false,
                    attach: Default::default(),
                };
                let id = add_annot(cx, annot, [at, None], "Texte");
                *cx.edit_text = id;
            }
        }
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.point = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        if let Some((p, text, _)) = &self.point {
            let pl = Plane::from_point_normal(*p, o.cam.forward());
            let (ro, rd) = o.cam.ray(self.cursor);
            let q = pl.intersect_ray(ro, rd).map(|t| ro + rd * t).unwrap_or(*p);
            let annot = Annotation {
                kind: AnnotKind::Text { point: *p, leader: q - *p },
                text: text.clone(),
                layer: 0,
                hidden: false,
                attach: Default::default(),
            };
            let st = crate::annot::default_style(&o.doc.model, &annot);
            crate::annot::draw_styled(&o.painter, o.origin, o.cam, &annot, text, false, &st);
        }
    }
}
