//! Section Plane tool: the plane follows the face under the cursor (arrows pointing into
//! it), the arrow keys set it perpendicular to an axis, Shift keeps its orientation.

use eframe::egui::Color32;
use esq_core::glam::{DVec2, DVec3};
use esq_core::model::{EntityRef, SectionPlane};

use super::{Overlay, Tool, ToolCx, ToolId};
use crate::pick::{Snap, SnapKind};
use crate::scene::{SECTION_PLANE, section_axes};

#[derive(Default)]
pub struct SectionPlaneTool {
    snap: Option<Snap>,
    /// World normal of the plane being placed.
    normal: DVec3,
    /// Orientation kept while Shift is held.
    held: Option<DVec3>,
    /// Size of the preview square, in world units.
    size: f64,
}

impl SectionPlaneTool {
    fn orientation(&self, cx: &ToolCx, s: &Snap) -> DVec3 {
        if let Some(n) = self.held {
            return n;
        }
        if let Some(l) = cx.lock.filter(|l| matches!(l.kind, SnapKind::Axis(_))) {
            return l.dir;
        }
        if let Some((_, n)) = &s.face {
            return -*n;
        }
        // Over nothing: across the axis closest to the view direction, facing away.
        let f = cx.cam().forward();
        (0..3)
            .map(|i| cx.axis(i))
            .max_by(|a, b| a.dot(f).abs().total_cmp(&b.dot(f).abs()))
            .map(|a| if a.dot(f) < 0.0 { -a } else { a })
            .unwrap_or(DVec3::Y)
    }
}

impl Tool for SectionPlaneTool {
    fn id(&self) -> ToolId {
        ToolId::SectionPlane
    }
    fn status(&self) -> String {
        // CSectionPlaneTool (the arrow keys also orient it along an axis).
        "Positionnez le plan de section sur la face.  Maj = verrouiller sur le plan.".into()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let s = cx.snap(m, None, None);
        if !cx.mods.shift {
            self.held = None;
        }
        self.normal = self.orientation(cx, &s);
        if cx.mods.shift && self.held.is_none() {
            self.held = Some(self.normal);
        }
        // About 120 pixels wide where the cursor is.
        let cam = cx.cam();
        let depth = if cam.perspective { (s.pos - cam.eye()).dot(cam.forward()).max(1.0) / cam.distance } else { 1.0 };
        self.size = cam.pixel_size() * depth * 120.0;
        self.snap = Some(s);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(s) = &self.snap else { return };
        let x = cx.doc.ctx_xform();
        let inv = x.inverse();
        let mut sp = SectionPlane::new(inv.transform_point3(s.pos), x.transpose().transform_vector3(self.normal));
        sp.symbol = cx.doc.model.next_section_symbol();
        let (name, symbol) = (sp.name.clone(), sp.symbol.clone());
        let mut id = None;
        cx.doc.commit("Plan de section", |m, d| {
            let ents = m.entities_mut(d);
            let s = ents.sections.insert(sp);
            ents.active_section = Some(s);
            id = Some(s);
        });
        if let Some(s) = id {
            cx.doc.select(vec![EntityRef::Section(s)]);
            // SketchUp then asks for its name and symbol (`SectionToolDlg`, « Section Naming »),
            // unless the user chose the default names (`ShowSectionToolDlg`).
            let m = crate::warnings::Message::question(
                "ShowSectionToolDlg",
                "Donner un nom au plan de section",
                "",
                &["OK"],
                "Ne plus afficher ce message.  Je vais utiliser les noms et symboles par défaut.",
            )
            .fields(&[("Nom :", &name), ("Symbole :", &symbol)])
            .then(move |app, a| {
                let (n, y) = (a.fields[0].clone(), a.fields[1].clone());
                if (n.as_str(), y.as_str()) == (name.as_str(), symbol.as_str()) {
                    return;
                }
                app.doc.commit("Nomenclature de la coupe", |m, d| {
                    if let Some(x) = m.entities_mut(d).sections.get_mut(s) {
                        x.name = n;
                        x.symbol = y;
                    }
                });
            });
            crate::warnings::queue(m);
        }
        // A new plane shows, even when View › Section Planes was off.
        *cx.show_sections = true;
    }
    fn overlay(&self, o: &mut Overlay) {
        let Some(s) = &self.snap else { return };
        let (u, v) = section_axes(self.normal);
        let h = self.size * 0.5;
        let corners = [(-h, -h), (h, -h), (h, h), (-h, h)].map(|(a, b)| s.pos + u * a + v * b);
        let c = Color32::from_rgb(SECTION_PLANE[0], SECTION_PLANE[1], SECTION_PLANE[2]);
        o.fill_polygon(
            &corners,
            Color32::from_rgba_unmultiplied(SECTION_PLANE[0], SECTION_PLANE[1], SECTION_PLANE[2], 40),
        );
        o.polyline(&corners, true, c, 1.5);
        let len = self.size * 0.15;
        for (i, p) in corners.iter().enumerate() {
            let side = (corners[(i + 1) % 4] - *p).normalize_or_zero();
            let tip = *p + self.normal * len;
            o.line(*p, tip, c, 1.5);
            o.line(tip, tip - self.normal * len * 0.4 + side * len * 0.25, c, 1.5);
            o.line(tip, tip - self.normal * len * 0.4 - side * len * 0.25, c, 1.5);
        }
    }
}
