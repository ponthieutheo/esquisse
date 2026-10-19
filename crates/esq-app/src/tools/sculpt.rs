//! Sandbox tools (Tools › Sandbox): Smoove, Stamp and Drape.

use std::cell::Cell;

use eframe::egui::{self, Color32};
use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::math::{TOL, line_closest_to_ray};
use esq_core::model::{EdgeId, EntityRef, FaceId, Units, VertexId};
use esq_core::sculpt::{SandboxDefaults, SmooveField, SmooveSeed, average_normal, faces_of_vertex};
use esq_core::units::format_length;

use super::{ClickDrag, Overlay, Tool, ToolCx, ToolId};
use crate::pick::{Snap, SnapKind};

thread_local! {
    /// Radius typed by the user, kept between two uses of the tool (SandboxDefaults.ini).
    static SMOOVE_RADIUS: Cell<Option<f64>> = const { Cell::new(None) };
}

fn smoove_radius(units: Units) -> f64 {
    SMOOVE_RADIUS.with(|r| r.get()).unwrap_or_else(|| SandboxDefaults::for_units(units).smoove_radius)
}

/// Linear part of the context transform: world vector → context vector.
fn ctx_inverse(cx: &ToolCx) -> DMat4 {
    cx.doc.ctx_xform().inverse()
}

/// Scale of the active context (a scaled group measures distances in its own units).
fn ctx_scale(cx: &ToolCx) -> f64 {
    cx.doc.ctx_xform().transform_vector3(DVec3::X).length().max(1e-12)
}

// ---------------------------------------------------------------------------------------
// Smoove (SmooveTool)

/// What the cursor designates on the mesh.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pick {
    Vertex(VertexId),
    Edge(EdgeId),
    Face(FaceId),
}

impl Pick {
    fn seed(self, cx: &ToolCx) -> SmooveSeed {
        match self {
            Pick::Vertex(v) => SmooveSeed::Vertex(v),
            Pick::Edge(e) => SmooveSeed::Edges(vec![e]),
            Pick::Face(f) => SmooveSeed::Edges(cx.doc.ents().face_edges(f)),
        }
    }

    fn faces(self, cx: &ToolCx) -> Vec<FaceId> {
        let ents = cx.doc.ents();
        match self {
            Pick::Vertex(v) => faces_of_vertex(ents, v),
            Pick::Edge(e) => ents.edges.get(e).map(|x| x.faces.clone()).unwrap_or_default(),
            Pick::Face(f) => vec![f],
        }
    }
}

/// The vertex, edge or face of the active context under the cursor (in that order).
fn pick_mesh(cx: &ToolCx, s: &Snap) -> Option<Pick> {
    let ents = cx.doc.ents();
    let ctx = &cx.doc.ctx;
    if let Some((hit, _)) = &s.edge
        && hit.path.as_slice() == ctx.as_slice()
        && let EntityRef::Edge(e) = hit.item
        && ents.edges.contains_key(e)
    {
        if s.kind == SnapKind::Endpoint {
            let p = cx.to_local(s.pos);
            let near = ents.edges[e]
                .v
                .into_iter()
                .min_by(|a, b| ents.pos(*a).distance(p).total_cmp(&ents.pos(*b).distance(p)));
            if let Some(v) = near.filter(|v| ents.pos(*v).distance(p) < TOL * 4.0) {
                return Some(Pick::Vertex(v));
            }
        }
        return Some(Pick::Edge(e));
    }
    s.face.as_ref().and_then(|(h, _)| h.face_in(ctx)).map(Pick::Face)
}

/// Edges and faces selected beforehand (groups and components do not count).
fn selected_seed(cx: &ToolCx) -> Option<SmooveSeed> {
    let ents = cx.doc.ents();
    let mut edges: Vec<EdgeId> = Vec::new();
    for e in &cx.doc.sel {
        match *e {
            EntityRef::Edge(x) if ents.edges.contains_key(x) => edges.push(x),
            EntityRef::Face(f) if ents.faces.contains_key(f) => edges.extend(ents.face_edges(f)),
            _ => {}
        }
    }
    edges.sort();
    edges.dedup();
    (!edges.is_empty()).then_some(SmooveSeed::Edges(edges))
}

#[derive(Default)]
pub struct SmooveTool {
    hover: Option<Pick>,
    cursor: DVec2,
    snap: Option<Snap>,
    /// State 1: the vertices that follow the move, the base point, the current point and
    /// the faces whose normal gives the Shift direction (context coordinates).
    field: Option<SmooveField>,
    base: DVec3,
    cur: DVec3,
    normal: DVec3,
    inferring: bool,
    shift: bool,
    /// Last offset applied (context coordinates), repeated by a double click.
    last_delta: Option<DVec3>,
    cd: ClickDrag,
}

impl SmooveTool {
    fn running(&self) -> bool {
        self.field.is_some()
    }

    fn delta(&self) -> DVec3 {
        self.cur - self.base
    }

    /// Direction the move follows: the vertical, or with Shift the normal of the entity
    /// the base point was taken on (only when it was designated with the mouse).
    fn line_dir(&self) -> DVec3 {
        if self.shift && self.normal != DVec3::ZERO { self.normal } else { DVec3::Z }
    }

    fn radius_local(&self, cx: &ToolCx) -> f64 {
        smoove_radius(cx.doc.model.units) / ctx_scale(cx)
    }

    fn start(&mut self, cx: &mut ToolCx, m: DVec2) {
        let s = cx.snap(m, None, None);
        let pick = pick_mesh(cx, &s);
        let (seed, normal) = match selected_seed(cx) {
            Some(seed) => (seed, DVec3::ZERO),
            None => {
                let Some(p) = pick else { return };
                (p.seed(cx), average_normal(cx.doc.ents(), &p.faces(cx)))
            }
        };
        let Some(field) = SmooveField::new(cx.doc.ents(), &seed) else {
            *cx.message = Some("Ce n'est pas une maille.".into());
            return;
        };
        self.base = cx.to_local(s.pos);
        self.cur = self.base;
        self.normal = normal;
        self.shift = cx.mods.shift;
        self.field = Some(field);
        self.hover = None;
    }

    /// Point of the move line the mouse designates.
    fn track(&mut self, cx: &ToolCx, m: DVec2, s: &Snap) {
        self.shift = cx.mods.shift;
        let dir = self.line_dir();
        let inv = ctx_inverse(cx);
        let radius = self.radius_local(cx);
        let project = |p: DVec3| self.base + dir * (p - self.base).dot(dir);
        if s.kind == SnapKind::Endpoint && self.base.distance(inv.transform_point3(s.pos)) > radius {
            self.inferring = true;
            self.cur = project(inv.transform_point3(s.pos));
            return;
        }
        self.inferring = false;
        let (ro, rd) = cx.cam().ray(m);
        let (ro, rd) = (inv.transform_point3(ro), inv.transform_vector3(rd));
        if let Some(t) = line_closest_to_ray(self.base, dir, ro, rd) {
            self.cur = self.base + dir * t;
        }
    }

    fn preview(&self, cx: &mut ToolCx) {
        let Some(field) = &self.field else { return };
        let mut model = cx.doc.model.clone();
        let def = cx.doc.ctx_def();
        field.apply(model.entities_mut(def), self.delta(), self.radius_local(cx));
        cx.doc.set_preview(Some(model));
    }

    /// Applies `delta` for good, as one undoable step.
    fn commit(&mut self, cx: &mut ToolCx, delta: DVec3) {
        let Some(field) = self.field.take() else { return };
        let radius = self.radius_local(cx);
        cx.doc.commit("Modeler", |m, def| field.apply(m.entities_mut(def), delta, radius));
        self.last_delta = Some(delta);
        self.reset(cx);
    }

    fn reset(&mut self, cx: &mut ToolCx) {
        self.field = None;
        self.inferring = false;
        self.hover = None;
        cx.doc.set_preview(None);
    }
}

impl Tool for SmooveTool {
    fn id(&self) -> ToolId {
        ToolId::Smoove
    }
    fn status(&self) -> String {
        if self.running() {
            "Modelez la maille vers le haut ou vers le bas. Maj = modelage perpendiculaire à l'entité sélectionnée. \
             Taper [10']=décaler 10', [10'r]=10' rayon de modelage, [10'd]=10' diamètre de modelage."
                .into()
        } else {
            "Cliquez pour sélectionner le point de départ du modelage ou tapez un nouveau rayon de modelage.".into()
        }
    }
    fn vcb_label(&self) -> &'static str {
        if self.running() { "Décalage" } else { "Rayon" }
    }
    fn vcb_value(&self, units: Units) -> String {
        if self.running() {
            format_length(self.delta().length(), units)
        } else {
            format_length(smoove_radius(units), units)
        }
    }
    fn busy(&self) -> bool {
        self.running()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.cursor = m;
        let s = cx.snap(m, None, None);
        if self.running() {
            self.track(cx, m, &s);
            self.preview(cx);
        } else {
            self.hover = pick_mesh(cx, &s);
        }
        self.snap = Some(s);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        if !self.running() {
            // With nothing selected, the click must be on the mesh; with a selection, it
            // can be anywhere.
            self.start(cx, m);
            self.cd.press(m, self.running());
            return;
        }
        let delta = self.delta();
        if delta.length() < TOL {
            // A click that goes nowhere: a double click repeats the last move here.
            self.cd.press(m, false);
            return;
        }
        self.commit(cx, delta);
        self.cd.press(m, false);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        // Press, drag, release counts as two clicks.
        if self.cd.release(m) && self.running() && self.delta().length() >= TOL {
            let delta = self.delta();
            self.commit(cx, delta);
        }
    }
    fn double_click(&mut self, cx: &mut ToolCx, _m: DVec2) {
        if let (true, Some(d)) = (self.running(), self.last_delta) {
            self.commit(cx, d);
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let t = text.trim().to_lowercase();
        let (num, radius, diameter) = if let Some(i) = t.find('r') {
            (&t[..i], true, false)
        } else if let Some(i) = t.find('d') {
            (&t[..i], true, true)
        } else {
            (t.as_str(), !self.running(), false)
        };
        let value = cx.parse_len(num)?;
        if radius {
            if value <= 0.0 {
                return Err("Le rayon doit être positif.".into());
            }
            let r = if diameter { value / 2.0 } else { value };
            SMOOVE_RADIUS.with(|c| c.set(Some(r)));
            if self.running() {
                self.preview(cx);
            }
            return Ok(());
        }
        // A typed offset is always vertical.
        self.commit(cx, DVec3::new(0.0, 0.0, value / ctx_scale(cx)));
        Ok(())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.reset(cx);
    }
    fn overlay(&self, o: &mut Overlay) {
        let x = o.doc.ctx_xform();
        let radius = smoove_radius(o.units) / x.transform_vector3(DVec3::X).length().max(1e-12);
        if let Some(field) = &self.field {
            // Guide line of the move, and the vertices that follow.
            let color = if self.shift && self.normal != DVec3::ZERO {
                Color32::from_rgb(220, 0, 220)
            } else {
                Color32::from_rgb(0, 0, 255)
            };
            o.dashed(x.transform_point3(self.base), x.transform_point3(self.cur), color, 1.5);
            let ents = o.doc.display_model().entities(o.doc.ctx_def());
            for v in field.affected(radius) {
                let Some(vx) = ents.verts.get(v.id) else { continue };
                let Some(p) = o.screen(x.transform_point3(vx.pos)) else { continue };
                let size = (8.0 * (1.0 - v.dist / radius) + 4.0).floor() as f32;
                o.painter.rect_filled(
                    egui::Rect::from_center_size(p, egui::vec2(size, size)),
                    0.0,
                    Color32::from_rgb(255, 230, 0),
                );
            }
            return;
        }
        if let (Some(_), Some(s)) = (self.hover, &self.snap) {
            // Radius of the future move, drawn flat around the cursor.
            let centre = x.inverse().transform_point3(s.pos);
            let ring: Vec<DVec3> = (0..=36)
                .map(|i| {
                    let a = (i as f64) * 10.0_f64.to_radians();
                    x.transform_point3(centre + DVec3::new(a.cos(), a.sin(), 0.0) * radius)
                })
                .collect();
            o.polyline(&ring, false, Color32::RED, 1.5);
            o.marker(s);
        }
    }
}
