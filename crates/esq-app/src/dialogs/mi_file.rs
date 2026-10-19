//! Model Info › File (where the model is, its name and description, its behaviour as a
//! component) and Model Info › Geo-location (where on Earth it stands).

use eframe::egui::{self, RichText};
use esq_core::model::{Behavior, Model};
use esq_core::sun::ShadowInfo;
use serde_json::{Value, json};

use super::{combo, group, name_widget, text_field};
use crate::app::EsquisseApp;

// ------------------------------------------------------------------------------------------
// File

/// "Glue to" choices: none, then SketchUp's `snapto` values.
pub const GLUE: [&str; 5] = ["Aucune", "N'importe lequel", "Horizontal", "Vertical", "Inclinée"];

/// The "Glue to" entry of a behaviour (0 = none).
pub fn glue_of(b: &Behavior) -> usize {
    if b.is2d { (b.snapto.clamp(0, 3) + 1) as usize } else { 0 }
}

pub fn set_glue(b: &mut Behavior, glue: usize) {
    b.is2d = glue > 0;
    if glue > 0 {
        b.snapto = glue as i32 - 1;
    } else {
        b.cuts_opening = false;
    }
}

/// "Redefine thumbnail on save" (kept with the model).
pub fn redefine_thumbnail(m: &Model) -> bool {
    m.view_ext.get("file").and_then(|f| f.get("redefine_thumbnail")).and_then(Value::as_bool).unwrap_or(false)
}

fn set_file_flag(m: &mut Model, key: &str, v: Value) {
    let e = m.view_ext.entry("file".to_string()).or_insert_with(|| json!({}));
    if !e.is_object() {
        *e = json!({});
    }
    e[key] = v;
}

/// Before saving: with "Redefine thumbnail on save", the model's thumbnail view becomes the
/// current one (eye, target, up, in mm).
pub fn before_save(app: &mut EsquisseApp) {
    if redefine_thumbnail(&app.doc.model) {
        let v = app.cam.to_view();
        let p = |x: esq_core::glam::DVec3| json!([x.x, x.y, x.z]);
        let cam = json!({"eye": p(v.eye), "target": p(v.target), "up": p(v.up)});
        set_file_flag(&mut app.doc.model, "thumbnail_camera", cam);
    }
}

/// "%1Ko (%2 octets)", as SketchUp writes the size.
pub fn size_text(bytes: u64) -> String {
    let kb = (bytes as f64 / 1024.0).round() as u64;
    format!("{kb}Ko ({bytes} octets)")
}

pub fn file_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    group(ui, "Général");
    let path = app.doc.path.clone();
    let st = &mut app.ui_state.extra.dialogs.info;
    let mut name = st.name_text.clone().unwrap_or_else(|| app.doc.model.meta.name.clone());
    let mut desc = st.desc_text.clone().unwrap_or_else(|| app.doc.model.meta.description.clone());
    let mut done_name = false;
    let mut done_desc = false;
    egui::Grid::new("file_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label("Emplacement:");
        let loc = path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(Jamais enregistré)".into());
        let r = ui.label(&loc);
        name_widget(ui, &r, "Emplacement", Some(&loc));
        ui.end_row();
        ui.label("Version :");
        ui.label(format!("Esquisse {}", env!("CARGO_PKG_VERSION")));
        ui.end_row();
        ui.label("Taille:");
        let size = path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| size_text(m.len()))
            .unwrap_or_else(|| "(Jamais enregistré)".into());
        let r = ui.label(&size);
        name_widget(ui, &r, "Taille", Some(&size));
        ui.end_row();
        ui.label("Nom :");
        let (done, focused) = text_field(ui, "Nom du modèle", &mut name, 280.0);
        done_name = done;
        if focused {
            st.name_text = Some(name.clone());
        }
        ui.end_row();
        ui.label("Description:");
        let r = ui.add(egui::TextEdit::multiline(&mut desc).desired_rows(3).desired_width(280.0));
        name_widget(ui, &r, "Description du modèle", None);
        if r.has_focus() {
            st.desc_text = Some(desc.clone());
        }
        done_desc = r.lost_focus();
        ui.end_row();
    });
    if done_name {
        app.ui_state.extra.dialogs.info.name_text = None;
        if name != app.doc.model.meta.name {
            app.doc.commit("Nom du modèle", |m, _| m.meta.name = name);
        }
    }
    if done_desc {
        app.ui_state.extra.dialogs.info.desc_text = None;
        if desc != app.doc.model.meta.description {
            app.doc.commit("Description du modèle", |m, _| m.meta.description = desc);
        }
    }
    let mut thumb = redefine_thumbnail(&app.doc.model);
    if ui.checkbox(&mut thumb, "Redéfinir la vignette à l'enregistrement").changed() {
        app.doc.commit("Vignette", |m, _| set_file_flag(m, "redefine_thumbnail", json!(thumb)));
    }
    if !app.doc.model.meta.guid.is_empty() {
        ui.label(RichText::new(format!("GUID : {}", app.doc.model.meta.guid)).weak().small());
    }

    group(ui, "Alignement");
    let before = app.doc.model.behavior();
    let mut b = before;
    alignment_ui(ui, &mut b);
    if b != before {
        app.doc.commit("Alignement", |m, _| m.set_behavior(b));
    }
}

/// The "Alignement" settings of a component (Model Info › File for the model itself,
/// Components › Edit for a definition).
pub fn alignment_ui(ui: &mut egui::Ui, b: &mut Behavior) {
    let mut glue = glue_of(b);
    ui.horizontal(|ui| {
        ui.label("Liaison:");
        let choices: Vec<(usize, String)> = GLUE.iter().enumerate().map(|(i, s)| (i, s.to_string())).collect();
        combo(ui, "Liaison", &mut glue, &choices);
    });
    set_glue(b, glue);
    ui.add_enabled_ui(glue > 0, |ui| ui.checkbox(&mut b.cuts_opening, "Découper l’ouverture"));
    ui.checkbox(&mut b.always_face_camera, "Toujours faire face à la caméra");
    if !b.always_face_camera {
        b.shadows_face_sun = false;
    }
    ui.add_enabled_ui(b.always_face_camera, |ui| ui.checkbox(&mut b.shadows_face_sun, "Ombres face au soleil"));
}

// ------------------------------------------------------------------------------------------
// Geo-location

/// Whether the model was given a location (else it keeps the templates' one, Boulder).
pub fn geolocated(m: &Model) -> bool {
    let t = ShadowInfo::default();
    let s = &m.shadow;
    (s.latitude - t.latitude).abs() > 1e-9 || (s.longitude - t.longitude).abs() > 1e-9
}

/// "35.2590N", "120.6915W", as SketchUp writes coordinates.
pub fn coord_text(v: f64, latitude: bool) -> String {
    let (pos, neg) = if latitude { ("N", "S") } else { ("E", "W") };
    format!("{:.4}{}", v.abs(), if v < 0.0 { neg } else { pos })
}

/// Model Info › Geo-location › Use solar orientation: the sun follows the model's north
/// (kept with the shadow settings, `view_ext["shadow"]`).
pub fn solar_orientation(m: &Model) -> bool {
    m.view_ext.get("shadow").and_then(|s| s.get("SolarOrientation")).and_then(Value::as_bool).unwrap_or(true)
}

/// Clear Location: back to the templates' location, unplaced.
pub fn clear_location(m: &mut Model) {
    let t = ShadowInfo::default();
    let s = &mut m.shadow;
    (s.latitude, s.longitude, s.city, s.country, s.tz_offset) =
        (t.latitude, t.longitude, t.city, t.country, t.tz_offset);
}

pub fn geolocation_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    group(ui, "Position géographique");
    let placed = geolocated(&app.doc.model);
    let status =
        if placed { "Ce modèle est géopositionné manuellement" } else { "Ce modèle n'est pas géopositionné" };
    let r = ui.label(RichText::new(status).strong());
    name_widget(ui, &r, "État de la géoposition", Some(status));
    if !placed {
        ui.label(
            RichText::new(
                "En géopositionnant ce modèle, vous définissez l'emplacement précis de ce modèle sur terre. Les \
                 études solaires seront plus précises.",
            )
            .weak(),
        );
    }
    let s = app.doc.model.shadow.clone();
    egui::Grid::new("geo_grid").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
        ui.label("Emplacement:");
        ui.label(if placed { s.city.clone() } else { String::new() });
        ui.end_row();
        ui.label("Pays :");
        ui.label(if placed { s.country.clone() } else { String::new() });
        ui.end_row();
        ui.label("Latitude:");
        let lat = if placed { coord_text(s.latitude, true) } else { String::new() };
        let r = ui.label(&lat);
        name_widget(ui, &r, "Latitude", Some(&lat));
        ui.end_row();
        ui.label("Longitude :");
        let lon = if placed { coord_text(s.longitude, false) } else { String::new() };
        let r = ui.label(&lon);
        name_widget(ui, &r, "Longitude", Some(&lon));
        ui.end_row();
    });
    ui.horizontal(|ui| {
        if ui.add_enabled(placed, egui::Button::new("Effacer l'emplacement")).clicked() {
            app.doc.commit("Effacer l'emplacement", |m, _| clear_location(m));
        }
    });
    group(ui, "Paramètres avancés");
    let st = &mut app.ui_state.extra.dialogs.info;
    if ui.selectable_label(st.manual_location, "Définir un emplacement manuel...").clicked() {
        st.manual_location = !st.manual_location;
    }
    if app.ui_state.extra.dialogs.info.manual_location {
        egui::Frame::group(ui.style()).show(ui, |ui| app.geo_location(ui));
    }
    let mut solar = solar_orientation(&app.doc.model);
    if ui.checkbox(&mut solar, "Utiliser l'orientation du soleil").changed() {
        app.doc.commit("Orientation du soleil", |m, _| {
            let e = m.view_ext.entry("shadow".to_string()).or_insert_with(|| json!({}));
            if !e.is_object() {
                *e = json!({});
            }
            e["SolarOrientation"] = json!(solar);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glue_and_coordinates() {
        let mut b = Behavior::default();
        set_glue(&mut b, 3);
        assert!(b.is2d && b.snapto == 2 && glue_of(&b) == 3);
        b.cuts_opening = true;
        set_glue(&mut b, 0);
        assert!(!b.is2d && !b.cuts_opening);
        assert_eq!(coord_text(35.259, true), "35.2590N");
        assert_eq!(coord_text(-120.6915, false), "120.6915W");
        assert_eq!(size_text(2048), "2Ko (2048 octets)");
    }
}
