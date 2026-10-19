//! Model Info › Components, Rendering and Credits.
//!
//! - Components: how the rest of the model and the other copies of a component look while
//!   one is open (`InactiveFade`, `InstanceFade`, `InactiveHidden`, `InstanceHidden`,
//!   `DisplayInstanceAxes` of the Ruby API's rendering options).
//! - Rendering: anti-aliased textures (`Model#mipmapping`).
//! - Credits: who made the model and its components; "Claim Credit" signs with the Windows
//!   user name (SketchUp uses the Trimble account).

use eframe::egui::{self, RichText};
use esq_core::glam::{DMat4, DVec3};
use esq_core::model::{DefId, Model};
use serde_json::{Value, json};

use super::{group, name_widget};
use crate::app::EsquisseApp;

// ------------------------------------------------------------------------------------------
// Components

/// Fade of the rest of the model and of the similar components while editing (0 = none,
/// 1 = invisible): SketchUp's `InactiveFade` and `InstanceFade` (0.25 and 0.5 for a new
/// model), rendering options kept by the view style (`ViewStyle::ro_get`).
pub fn edit_fades(s: &crate::style::ViewStyle) -> (f32, f32) {
    (s.inactive_fade as f32, s.instance_fade as f32)
}

/// Model Info › Components › Show component axes (`DisplayInstanceAxes`).
pub fn show_instance_axes(s: &crate::style::ViewStyle) -> bool {
    s.instance_axes
}

/// Short axes at a component's origin (a fifth of its size), in the axis colours.
pub fn instance_axes(m: &Model, d: DefId, x: &DMat4, push: &mut dyn FnMut(DVec3, DVec3, [u8; 3])) {
    let ents = &m.defs[d].entities;
    let (lo, hi) =
        ents.verts.values().fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(a, b), v| (a.min(v.pos), b.max(v.pos)));
    let size = if lo.is_finite() { (hi - lo).max_element() } else { 100.0 };
    let len = (size * 0.2).max(10.0);
    let o = x.transform_point3(DVec3::ZERO);
    for (i, axis) in [DVec3::X, DVec3::Y, DVec3::Z].into_iter().enumerate() {
        let dir = x.transform_vector3(axis).normalize_or_zero();
        push(o, o + dir * len, crate::settings::axis_color(i));
    }
}

/// A "Lighter — Darker" slider; `fade` is how much the geometry fades (0.0 to 1.0).
fn fade_slider(ui: &mut egui::Ui, name: &str, fade: &mut f32, enabled: bool) -> bool {
    let mut dark = 1.0 - *fade;
    let mut done = false;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Éclaircir");
            let r = ui.add(egui::Slider::new(&mut dark, 0.0..=1.0).show_value(false));
            name_widget(ui, &r, name, Some(&format!("{:.2}", 1.0 - dark)));
            ui.label("Foncer");
            done = r.changed();
        });
    });
    *fade = 1.0 - dark;
    done
}

pub fn components_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    group(ui, "Modification de composants/groupes");
    let (mut rest, mut similar) = edit_fades(&app.style);
    ui.label("Estomper les composants similaires");
    let changed_similar = fade_slider(ui, "Estomper les composants similaires", &mut similar, !app.style.hide_similar);
    let mut hide = app.style.hide_similar;
    if ui.checkbox(&mut hide, "Masquer les composants similaires").changed() {
        app.run(&ctx, crate::commands::Cmd::HideSimilar);
    }
    ui.add_space(4.0);
    ui.label("Estomper le reste du modèle");
    let changed_rest = fade_slider(ui, "Estomper le reste du modèle", &mut rest, !app.style.hide_rest);
    let mut hide = app.style.hide_rest;
    if ui.checkbox(&mut hide, "Masquer le reste du modèle").changed() {
        app.run(&ctx, crate::commands::Cmd::HideRest);
    }
    if changed_rest || changed_similar {
        app.style.inactive_fade = rest as f64;
        app.style.instance_fade = similar as f64;
        app.doc.rev += 1;
    }
    group(ui, "Axes du composant");
    let mut axes = show_instance_axes(&app.style);
    if ui.checkbox(&mut axes, "Afficher les axes du composant").changed() {
        app.style.instance_axes = axes;
        app.doc.rev += 1;
    }
}

// ------------------------------------------------------------------------------------------
// Rendering

pub fn rendering_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    group(ui, "Textures anticrénelées");
    let mut on = app.doc.model.meta.mipmapping;
    if ui.checkbox(&mut on, "Utiliser des textures anticrénelées").changed() {
        app.doc.commit("Textures anticrénelées", |m, _| m.meta.mipmapping = on);
    }
    ui.label(
        RichText::new(
            "L'activation de cette fonction peut améliorer les performances et la qualité des textures. Si \
             vous constatez que les textures sont floues ou si l'affichage est anormal, nous vous conseillons \
             de désactiver les textures anticrénelées.",
        )
        .weak(),
    );
}

// ------------------------------------------------------------------------------------------
// Credits

/// Model authors and component authors (by definition name), kept with the model.
pub fn credits(m: &Model) -> (Vec<String>, Vec<(String, String)>) {
    let c = m.view_ext.get("Credits");
    let authors = c
        .and_then(|c| c.get("authors"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let mut comps: Vec<(String, String)> = c
        .and_then(|c| c.get("components"))
        .and_then(Value::as_object)
        .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    // Only the components still in the model.
    comps.retain(|(name, _)| m.defs.values().any(|d| !d.is_group && d.name == *name));
    (authors, comps)
}

/// Claim Credit: the user becomes an author of the model and of the components no one claimed.
pub fn claim_credit(m: &mut Model, who: &str) {
    let mut authors = credits(m).0;
    if !authors.iter().any(|a| a == who) {
        authors.push(who.to_string());
    }
    let mut comps = m
        .view_ext
        .get("Credits")
        .and_then(|c| c.get("components"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for d in m.defs.values().filter(|d| !d.is_group) {
        comps.entry(d.name.clone()).or_insert_with(|| json!(who));
    }
    m.view_ext.insert("Credits".into(), json!({"authors": authors, "components": comps}));
}

pub fn credits_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let (authors, comps) = credits(&app.doc.model);
    group(ui, "Auteurs du modèle");
    let text =
        if authors.is_empty() { "Auteurs: Inconnus".to_string() } else { format!("Auteurs: {}", authors.join(", ")) };
    let r = ui.label(text.clone());
    name_widget(ui, &r, "Auteurs du modèle", Some(&text));
    let who = crate::settings::user_name();
    if ui
        .add_enabled(!authors.contains(&who), egui::Button::new("Revendiquer le crédit"))
        .on_hover_text(format!("Signer le modèle et ses composants au nom de « {who} »"))
        .clicked()
    {
        app.doc.commit("Revendiquer le crédit", |m, _| claim_credit(m, &who));
    }
    group(ui, "Auteurs des composants");
    if comps.is_empty() {
        ui.label("Aucun auteur de composant.");
    }
    egui::Grid::new("credits_grid").striped(true).num_columns(2).show(ui, |ui| {
        ui.label(RichText::new("Composant").strong());
        ui.label(RichText::new("Auteurs").strong());
        ui.end_row();
        for (c, a) in &comps {
            ui.label(c);
            ui.label(a);
            ui.end_row();
        }
    });
}
