//! Window › Shadows, and the geo-location page of Model Info.

use eframe::egui::{self, RichText};
use esq_core::sun::ShadowInfo;

use crate::app::EsquisseApp;

/// The time zones offered by SketchUp, in hours from UTC.
const ZONES: &[f64] = &[
    -12.0, -11.0, -10.0, -9.5, -9.0, -8.0, -7.0, -6.0, -5.0, -4.0, -3.5, -3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 3.5,
    4.0, 4.5, 5.0, 5.5, 5.75, 6.0, 6.5, 7.0, 8.0, 8.75, 9.0, 9.5, 10.0, 10.5, 11.0, 12.0, 12.75, 13.0, 14.0,
];

pub fn zone_label(h: f64) -> String {
    let m = (h.abs() * 60.0).round() as i64;
    format!("UTC{}{:02}:{:02}", if h < 0.0 { '-' } else { '+' }, m / 60, m % 60)
}

fn hhmm(minutes: f64) -> String {
    let m = minutes.round().rem_euclid(1440.0) as i64;
    format!("{:02}:{:02}", m / 60, m % 60)
}

const MONTHS: [&str; 12] = ["J", "F", "M", "A", "M", "J", "J", "A", "S", "O", "N", "D"];

impl EsquisseApp {
    /// Applies new shadow settings: live while a slider moves, as one undo step at the end.
    fn set_shadow(&mut self, s: ShadowInfo, done: bool) {
        if self.shadow_edit.is_none() {
            self.shadow_edit = Some(self.doc.model.shadow.clone());
        }
        self.doc.model.shadow = s;
        if done && let Some(before) = self.shadow_edit.take() {
            let after = std::mem::replace(&mut self.doc.model.shadow, before);
            if after != self.doc.model.shadow {
                self.doc.commit("Ombres", |m, _| m.shadow = after);
            }
        }
    }

    /// View › Shadows.
    pub fn toggle_shadows(&mut self) {
        let on = !self.doc.model.shadow.display;
        self.doc.commit("Ombres", |m, _| m.shadow.display = on);
    }

    pub fn shadows_panel(&mut self, ui: &mut egui::Ui) {
        let mut s = self.doc.model.shadow.clone();
        let mut changed = false;
        let mut done = false;
        let track = |r: egui::Response, changed: &mut bool, done: &mut bool| {
            *changed |= r.changed();
            *done |= r.drag_stopped() || (r.changed() && !r.dragged());
        };
        ui.horizontal(|ui| {
            let r = ui.toggle_value(&mut s.display, "Afficher les ombres");
            track(r, &mut changed, &mut done);
            egui::ComboBox::from_id_salt("tz").selected_text(zone_label(s.tz_offset)).show_ui(ui, |ui| {
                for &z in ZONES {
                    let r = ui.selectable_value(&mut s.tz_offset, z, zone_label(z));
                    track(r, &mut changed, &mut done);
                }
            });
        });

        let (y, m, d, minutes) = s.local();
        let mut t = minutes;
        egui::Grid::new("shadow_time").num_columns(3).show(ui, |ui| {
            ui.label("Heure");
            let r = ui.add(egui::Slider::new(&mut t, 0.0..=1439.0).show_value(false));
            track(r, &mut changed, &mut done);
            ui.label(hhmm(t));
            ui.end_row();
            ui.label("");
            if let Some((rise, set)) = s.sunrise_sunset() {
                ui.label(
                    RichText::new(format!("Lever {}   Coucher {}", hhmm(rise), hhmm(set)))
                        .small()
                        .color(egui::Color32::from_gray(110)),
                );
            }
            ui.end_row();

            ui.label("Date");
            let year_start = esq_core::sun::days_from_civil(y, 1, 1);
            let days_in_year = (esq_core::sun::days_from_civil(y + 1, 1, 1) - year_start) as f64;
            let mut doy = (esq_core::sun::days_from_civil(y, m, d) - year_start) as f64;
            let r = ui.add(egui::Slider::new(&mut doy, 0.0..=days_in_year - 1.0).show_value(false));
            track(r.clone(), &mut changed, &mut done);
            ui.label(format!("{d:02}/{m:02}"));
            ui.end_row();
            ui.label("");
            ui.label(RichText::new(MONTHS.join("     ")).small().color(egui::Color32::from_gray(110)));
            ui.end_row();
            if r.changed() || t != minutes {
                let (ny, nm, nd) = esq_core::sun::civil_from_days(year_start + doy.round() as i64);
                s.set_local(ny, nm, nd, t.round());
            }

            ui.label("Clair");
            let r = ui.add(egui::Slider::new(&mut s.light, 0..=100));
            track(r, &mut changed, &mut done);
            ui.end_row();
            ui.label("Sombre");
            let r = ui.add(egui::Slider::new(&mut s.dark, 0..=100));
            track(r, &mut changed, &mut done);
            ui.end_row();
        });
        let r = ui.checkbox(&mut s.use_sun_for_shading, "Utiliser le soleil pour l'ombrage");
        track(r, &mut changed, &mut done);
        ui.label("Afficher :");
        ui.horizontal_wrapped(|ui| {
            for (v, label) in [
                (&mut s.on_faces, "Sur les faces"),
                (&mut s.on_ground, "Sur le sol"),
                (&mut s.from_edges, "À partir des arêtes"),
            ] {
                let r = ui.checkbox(v, label);
                track(r, &mut changed, &mut done);
            }
        });
        if changed || (done && self.shadow_edit.is_some()) {
            self.set_shadow(s, done);
        }
    }

    /// Model Info › Geo-location: where the model stands, for the sun.
    pub fn geo_location(&mut self, ui: &mut egui::Ui) {
        let mut s = self.doc.model.shadow.clone();
        let mut done = false;
        egui::Grid::new("geo").num_columns(2).show(ui, |ui| {
            ui.label("Pays");
            done |= ui.text_edit_singleline(&mut s.country).lost_focus();
            ui.end_row();
            ui.label("Lieu");
            done |= ui.text_edit_singleline(&mut s.city).lost_focus();
            ui.end_row();
            ui.label("Latitude");
            let r = ui.add(egui::DragValue::new(&mut s.latitude).range(-90.0..=90.0).speed(0.01).suffix("°"));
            done |= r.drag_stopped() || r.lost_focus();
            ui.end_row();
            ui.label("Longitude");
            let r = ui.add(egui::DragValue::new(&mut s.longitude).range(-180.0..=180.0).speed(0.01).suffix("°"));
            done |= r.drag_stopped() || r.lost_focus();
            ui.end_row();
            ui.label("Angle du nord");
            let r = ui.add(egui::DragValue::new(&mut s.north_angle).range(-360.0..=360.0).speed(0.5).suffix("°"));
            done |= r.drag_stopped() || r.lost_focus();
            ui.end_row();
        });
        if s != self.doc.model.shadow {
            self.set_shadow(s, done);
        } else if done && self.shadow_edit.is_some() {
            let s = self.doc.model.shadow.clone();
            self.set_shadow(s, true);
        }
    }
}
