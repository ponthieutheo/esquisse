//! Light tool (`LightTool`): a click places a point light (`LightElement`: white,
//! intensity 1) where the cursor infers, in the open group or component.

use eframe::egui::{self, Color32, Pos2, Stroke};
use esq_core::glam::DVec2;
use esq_core::model::PointLight;

use super::{Overlay, Tool, ToolCx, ToolId};
use crate::pick::Snap;

#[derive(Default)]
pub struct LightTool {
    snap: Option<Snap>,
}

/// A light: a bulb with rays, in its colour.
pub fn paint_light(p: &egui::Painter, at: Pos2, l: &PointLight) {
    let c = Color32::from_rgb(
        (l.color[0].clamp(0.0, 1.0) * 255.0) as u8,
        (l.color[1].clamp(0.0, 1.0) * 255.0) as u8,
        (l.color[2].clamp(0.0, 1.0) * 255.0 * 0.6) as u8,
    );
    let ink = Stroke::new(1.0, Color32::from_gray(60));
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        let d = egui::vec2(a.cos(), a.sin());
        p.line_segment([at + d * 7.0, at + d * 11.0], Stroke::new(1.5, Color32::from_rgb(230, 170, 0)));
    }
    p.circle(at, 5.5, c, ink);
}

impl Tool for LightTool {
    fn id(&self) -> ToolId {
        ToolId::Light
    }
    fn status(&self) -> String {
        "Cliquez pour placer une lumière dans le modèle.".into()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.snap = Some(cx.snap(m, None, None));
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(s) = &self.snap else { return };
        let pos = cx.to_local(s.pos);
        cx.doc.commit("Placer la lumière", |model, d| model.entities_mut(d).lights.push(PointLight::at(pos)));
    }
    fn overlay(&self, o: &mut Overlay) {
        if let Some(s) = &self.snap
            && let Some(at) = o.screen(s.pos)
        {
            paint_light(&o.painter, at, &PointLight::at(s.pos));
        }
    }
}
