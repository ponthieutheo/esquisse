//! Add Snap (`CSnapTool`): a snap is a point of a component with the direction it faces and
//! its up, where other components connect. Click the position (on a face it faces like the
//! face), then the forward direction, then the up direction; a double-click places the snap
//! as it is oriented at that moment. The Measurements box takes the angle of the up around
//! the forward direction. Snaps of the whole model are drawn over the view.

use eframe::egui::{Color32, Stroke};
use esq_core::glam::{DMat3, DVec2, DVec3};
use esq_core::model::{EntityRef, SnapPoint, Space};

use super::{Overlay, Tool, ToolCx, ToolId};
use crate::pick::Snap;

/// The up of a snap facing `dir`: the blue axis made perpendicular, or the green one when
/// the snap faces straight up or down.
pub fn default_up(dir: DVec3) -> DVec3 {
    let z = DVec3::Z;
    let up = z - dir * z.dot(dir);
    if up.length() < 1e-6 { (DVec3::Y - dir * DVec3::Y.dot(dir)).normalize() } else { up.normalize() }
}

#[derive(Default)]
pub struct SnapTool {
    /// 0: position, 1: forward direction, 2: up direction.
    state: u8,
    pos: DVec3,
    dir: DVec3,
    up: DVec3,
    snap: Option<Snap>,
}

impl SnapTool {
    /// The orientation shown at the cursor before the position is fixed: like the face under
    /// it, else facing the red axis.
    fn hover_orientation(s: &Snap) -> DVec3 {
        s.face.as_ref().map(|(_, n)| n.normalize()).unwrap_or(DVec3::X)
    }

    fn place(&mut self, cx: &mut ToolCx) {
        let x = cx.doc.ctx_xform().inverse();
        let snap = SnapPoint::new(
            x.transform_point3(self.pos),
            x.transform_vector3(self.dir).normalize(),
            x.transform_vector3(self.up).normalize(),
        );
        cx.doc.commit("Ajouter un ancrage", |m, d| {
            m.add_snap(d, snap);
        });
        self.state = 0;
    }
}

impl Tool for SnapTool {
    fn id(&self) -> ToolId {
        ToolId::AddSnap
    }
    fn status(&self) -> String {
        match self.state {
            0 => "Cliquez pour définir la position de l'ancrage ou double-cliquez pour placer un nouvel ancrage selon son orientation actuelle.",
            1 => "Cliquer pour définir la direction vers l'avant ou double-cliquer pour la définir comme orientée.",
            _ => "Cliquer pour définir la direction « vers le haut ».",
        }
        .into()
    }
    fn vcb_label(&self) -> &'static str {
        "Angle"
    }
    fn busy(&self) -> bool {
        self.state > 0
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let reference = (self.state > 0).then_some(self.pos);
        let s = cx.snap(m, reference, None);
        match self.state {
            0 => {
                self.pos = s.pos;
                self.dir = Self::hover_orientation(&s);
                self.up = default_up(self.dir);
            }
            1 => {
                let d = s.pos - self.pos;
                if d.length() > 1e-6 {
                    self.dir = d.normalize();
                    self.up = default_up(self.dir);
                }
            }
            _ => {
                let d = s.pos - self.pos;
                let u = d - self.dir * d.dot(self.dir);
                if u.length() > 1e-6 {
                    self.up = u.normalize();
                }
            }
        }
        self.snap = Some(s);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        match self.state {
            0 => self.state = 1,
            1 => self.state = 2,
            _ => self.place(cx),
        }
    }
    fn double_click(&mut self, cx: &mut ToolCx, _m: DVec2) {
        // The click before has fixed the position (or the direction): place as oriented.
        if self.state == 1 || self.state == 2 {
            if self.state == 1 {
                // The orientation stays the one shown when the position was clicked.
                self.dir = self.snap.as_ref().map(Self::hover_orientation).unwrap_or(self.dir);
                self.up = default_up(self.dir);
            }
            self.place(cx);
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        if self.state == 0 {
            return Err("Cliquez d'abord la position de l'ancrage".into());
        }
        let deg: f64 = text.trim().replace(',', ".").parse().map_err(|_| "Angle non valide.".to_string())?;
        let up = DMat3::from_axis_angle(self.dir, deg.to_radians()) * default_up(self.dir);
        self.up = up.normalize();
        self.place(cx);
        Ok(())
    }
    fn cancel(&mut self, _cx: &mut ToolCx) {
        self.state = 0;
    }
    fn overlay(&self, o: &mut Overlay) {
        let size = o.cam.pixel_size_at(self.pos) * 40.0;
        paint_snap(o, &SnapPoint::new(self.pos, self.dir, self.up), size, Color32::from_rgb(230, 120, 0));
    }
}

/// A snap: a square across its direction, an arrow forward and a tick up.
pub fn paint_snap(o: &Overlay, s: &SnapPoint, size: f64, color: Color32) {
    let right = s.dir.cross(s.up).normalize_or_zero();
    let h = size * 0.35;
    let quad = [
        s.pos + (right + s.up) * h,
        s.pos + (-right + s.up) * h,
        s.pos + (-right - s.up) * h,
        s.pos + (right - s.up) * h,
    ];
    o.polyline(&quad, true, color, 1.5);
    o.line(s.pos, s.pos + s.dir * size, color, 2.0);
    o.line(s.pos, s.pos + s.up * size * 0.6, Color32::from_rgb(30, 50, 230), 1.5);
    if let Some(p) = o.screen(s.pos) {
        o.painter.circle(p, 3.0, color, Stroke::new(1.0, Color32::WHITE));
    }
}

/// A snap shows (and the Move tool offers it) unless it is hidden or its tag is.
pub fn shown(model: &esq_core::model::Model, s: &SnapPoint) -> bool {
    !s.hidden && model.layers.get(s.layer).is_none_or(|l| l.visible)
}

/// A space shows unless it is hidden or its tag is.
pub fn space_shown(model: &esq_core::model::Model, s: &Space) -> bool {
    !s.hidden && model.layers.get(s.layer).is_none_or(|l| l.visible)
}

/// Length of a snap's arrow on screen (pixels), as drawn and as picked.
pub const SNAP_ARROW_PX: f64 = 30.0;

/// A space (`SpaceDrawer` of SketchUp.exe draws it as a see-through volume): its floor and
/// ceiling outlines with the vertical edges, over a light fill of the floor.
pub fn paint_space(o: &Overlay, s: &Space, color: Color32) {
    let [r, g, b, _] = color.to_array();
    o.fill_polygon(&s.perimeter, Color32::from_rgba_unmultiplied(r, g, b, 40));
    for (a, b) in s.outline() {
        o.line(a, b, color, 1.5);
    }
}

/// Whether a snap or a space of the collection reached through `path` is selected (the
/// selection holds entities of the active context).
fn mark_selected(o: &Overlay, path: &[esq_core::model::InstanceId], e: EntityRef) -> bool {
    path == o.doc.ctx.as_slice() && o.doc.is_selected(e)
}

/// The snaps, spaces and lights of the model, drawn over the view (they are not geometry).
/// Selected ones take the selection colour.
pub fn paint_marks(o: &Overlay, model: &esq_core::model::Model, highlight: Color32) {
    // Drawn every image: the walk of the whole tree only when some collection has marks.
    let marked = |e: &esq_core::model::Entities| !(e.spaces.is_empty() && e.snaps.is_empty() && e.lights.is_empty());
    if !marked(&model.root) && !model.defs.values().any(|d| marked(&d.entities)) {
        return;
    }
    esq_core::walk::visit_all(model, false, |p| {
        for s in p.ents.spaces.iter().filter(|s| space_shown(model, s)) {
            let sel = mark_selected(o, &p.path, EntityRef::Space(s.id));
            paint_space(o, &s.transformed(&p.xform), if sel { highlight } else { Color32::from_rgb(70, 130, 180) });
        }
        for s in p.ents.snaps.iter().filter(|s| shown(model, s)) {
            let sel = mark_selected(o, &p.path, EntityRef::Snap(s.id));
            let s = s.transformed(&p.xform);
            let size = o.cam.pixel_size_at(s.pos) * SNAP_ARROW_PX;
            paint_snap(o, &s, size, if sel { highlight } else { Color32::from_rgb(0, 150, 200) });
        }
        for l in &p.ents.lights {
            let at = p.xform.transform_point3(l.pos);
            if let Some(c) = o.screen(at) {
                super::light::paint_light(&o.painter, c, l);
            }
        }
    });
}
