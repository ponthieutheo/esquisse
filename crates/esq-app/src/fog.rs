//! Window › Fog (SketchUp's `CFogDlg`): display fog, the distance slider (two handles: where
//! fog starts and where it is complete) and the fog colour.
//!
//! The slider is not linear: SketchUp places the near end of the model at 20 % and its far
//! end at 80 %, the eye at 0 and infinity at 100 % (and its inverse).

use eframe::egui::{self, Color32, Sense, Stroke};
use esq_core::model::Model;

use crate::app::EsquisseApp;
use crate::camera::Camera;

/// Slider positions of the near and far ends of the model.
const NEAR_AT: f64 = 0.2;
const FAR_AT: f64 = 0.8;
/// The end handle stops just short of infinity.
const MAX_AT: f64 = 0.999999;
/// Far end used when there is no model (1000 inches).
const NO_MODEL_FAR: f64 = 25_400.0;

/// Nearest and farthest distance of the model along the view direction (mm), as SketchUp's
/// view keeps them for the fog slider.
pub fn model_range(model: &Model, cam: &Camera) -> (f64, f64) {
    let (lo, hi) = crate::scene::model_bounds(model);
    if !(lo.is_finite() && hi.is_finite()) {
        return (0.0, NO_MODEL_FAR);
    }
    let (near, far) = crate::render::depth_range(Some((lo, hi)), cam);
    (near.max(0.0), far.max(near + 1.0))
}

/// Slider position (0–1) of a distance from the eye.
pub fn to_slider(d: f64, near: f64, far: f64) -> f64 {
    let (k1, k2) = (1.0 / (1.0 - NEAR_AT), 1.0 / (1.0 - FAR_AT));
    if d <= near {
        1.0 - 1.0 / (d.max(0.0) / ((1.0 / NEAR_AT - 1.0) * near.max(1e-9)) + 1.0)
    } else if d < far {
        1.0 - 1.0 / ((k2 - k1) * (d - near) / (far - near) + k1)
    } else {
        1.0 - 1.0 / (d / ((1.0 / FAR_AT - 1.0) * far) + 1.0)
    }
}

/// Distance from the eye of a slider position (0–1).
pub fn from_slider(s: f64, near: f64, far: f64) -> f64 {
    let s = s.clamp(0.0, MAX_AT);
    if s <= NEAR_AT {
        (1.0 / NEAR_AT - 1.0) * near * s / (1.0 - s)
    } else if s >= FAR_AT {
        (1.0 / FAR_AT - 1.0) * far * s / (1.0 - s)
    } else {
        let k1 = 1.0 / (1.0 - NEAR_AT);
        (1.0 / (1.0 - s) - k1) * (far - near) / (1.0 / (1.0 - FAR_AT) - k1) + near
    }
}

/// A slider with two handles; returns which handle moved.
fn range_slider(ui: &mut egui::Ui, lo: &mut f64, hi: &mut f64) -> egui::Response {
    let width = ui.available_width().max(120.0);
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(width, 26.0), Sense::click_and_drag());
    let p = ui.painter();
    let track = rect.shrink2(egui::vec2(8.0, 0.0));
    let y = rect.center().y;
    p.line_segment(
        [egui::pos2(track.left(), y), egui::pos2(track.right(), y)],
        Stroke::new(3.0, Color32::from_gray(170)),
    );
    let x_of = |v: f64| track.left() + track.width() * v as f32;
    p.line_segment(
        [egui::pos2(x_of(*lo), y), egui::pos2(x_of(*hi), y)],
        Stroke::new(3.0, Color32::from_rgb(52, 110, 220)),
    );
    let id = resp.id.with("handle");
    if resp.drag_started()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let which = if (pos.x - x_of(*lo)).abs() <= (pos.x - x_of(*hi)).abs() { 0u8 } else { 1u8 };
        ui.data_mut(|d| d.insert_temp(id, which));
    }
    if (resp.dragged() || resp.clicked())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let v = (((pos.x - track.left()) / track.width()) as f64).clamp(0.0, 1.0);
        let which = ui.data(|d| d.get_temp::<u8>(id)).unwrap_or(if (v - *lo).abs() <= (v - *hi).abs() { 0 } else { 1 });
        if which == 0 {
            *lo = v.min(*hi);
        } else {
            *hi = v.max(*lo);
        }
        resp.mark_changed();
    }
    for v in [*lo, *hi] {
        let c = egui::pos2(x_of(v), y);
        p.circle_filled(c, 6.0, Color32::WHITE);
        p.circle_stroke(c, 6.0, Stroke::new(1.5, Color32::from_gray(90)));
    }
    resp
}

impl EsquisseApp {
    /// Window › Fog.
    pub fn fog_panel(&mut self, ui: &mut egui::Ui) {
        let mut on = self.style.fog;
        if ui.checkbox(&mut on, "Afficher le brouillard").changed() {
            self.toggle_fog();
        }
        let (near, far) = model_range(&self.doc.model, &self.cam);
        let s = &self.style;
        let mut lo = if s.fog_start < 0.0 { 0.0 } else { to_slider(s.fog_start, near, far) };
        let mut hi = if s.fog_end < 0.0 { 1.0 } else { to_slider(s.fog_end, near, far) };
        let mut changed = false;
        ui.add_enabled_ui(self.style.fog, |ui| {
            ui.label("Distance");
            changed = range_slider(ui, &mut lo, &mut hi).changed();
            ui.horizontal(|ui| {
                ui.label("0");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label("∞"));
            });
            ui.label("Couleur");
            ui.horizontal(|ui| {
                let s = &mut self.style;
                changed |= ui.checkbox(&mut s.fog_use_bg, "Utiliser la couleur d'arrière-plan").changed();
                ui.add_enabled_ui(!s.fog_use_bg, |ui| changed |= ui.color_edit_button_srgb(&mut s.fog_color).changed());
            });
        });
        if changed {
            self.style.fog_start = from_slider(lo, near, far);
            self.style.fog_end = from_slider(hi, near, far);
            self.doc.rev += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slider_puts_the_model_between_20_and_80_percent() {
        let (n, f) = (1000.0, 5000.0);
        assert!((to_slider(n, n, f) - 0.2).abs() < 1e-12);
        assert!((to_slider(f, n, f) - 0.8).abs() < 1e-12);
        assert_eq!(to_slider(0.0, n, f), 0.0);
        assert!(to_slider(1e12, n, f) > 0.9999);
        for s in [0.0, 0.05, 0.2, 0.37, 0.5, 0.8, 0.93, 0.999] {
            let d = from_slider(s, n, f);
            assert!((to_slider(d, n, f) - s).abs() < 1e-9, "{s} -> {d}");
        }
        // Infinity is never reached.
        assert!(from_slider(1.0, n, f).is_finite());
    }
}
