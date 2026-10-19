//! Window › Overlays: the overlays that extensions add to the model (`Sketchup::Overlay`,
//! `model.overlays.add`), each turned on or off here; the one picked shows its description
//! and the extension it comes from.

use eframe::egui::{self, Color32, RichText};

use crate::app::EsquisseApp;

/// Window › Overlays.
pub fn show(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let id = egui::Id::unique("overlays_panel");
    let mut picked: Option<String> = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    let st = &app.ui_state.extra.ruby_tools;
    if st.overlays.is_empty() {
        ui.label(RichText::new("Aucune incrustation : les extensions en ajoutent au modèle.").italics());
        return;
    }
    // The first one is picked until another is.
    if !picked.as_ref().is_some_and(|p| st.overlays.iter().any(|o| &o.0 == p)) {
        picked = st.overlays.first().map(|o| o.0.clone());
    }
    let mut toggle = None;
    for (oid, name, on) in &st.overlays {
        ui.horizontal(|ui| {
            let mut v = *on;
            if ui.checkbox(&mut v, "").on_hover_text(if v { "Désactiver" } else { "Activer" }).changed() {
                toggle = Some((oid.clone(), v));
            }
            if ui.selectable_label(picked.as_deref() == Some(oid.as_str()), name).clicked() {
                picked = Some(oid.clone());
            }
        });
    }
    if let Some(oid) = &picked
        && let Some((desc, source)) = st.overlay_info.get(oid)
    {
        ui.separator();
        if !desc.is_empty() {
            ui.label(desc);
        }
        let source = if source.is_empty() { "—" } else { source.as_str() };
        ui.label(RichText::new(format!("Source : {source}")).small().color(Color32::from_gray(100)));
    }
    if toggle.is_some() {
        app.ui_state.extra.ruby_tools.toggle = toggle;
    }
    ui.data_mut(|d| d.insert_temp(id, picked));
}
