//! Preferences › General: backups and auto-save, checking models for problems, messages,
//! the welcome window and the toolbar settings, and what they do when files are saved and
//! opened.

use std::path::PathBuf;

use eframe::egui;

use super::{group, name_widget};
use crate::app::EsquisseApp;
use crate::settings::{self, Settings};
use crate::toolbars::DisplayMode;

#[derive(Default)]
pub struct GeneralState {
    /// When the model was last auto-saved (or started being changed), in seconds.
    pub last_autosave: Option<f64>,
    /// The auto-save file written for the open model.
    pub autosave_file: Option<PathBuf>,
    /// Ask to purge at save: the question is up (for File › Save As when true).
    pub purge_ask: Option<bool>,
    /// The question was answered: the save goes on without asking again.
    pub purge_answered: bool,
}

// ------------------------------------------------------------------------------------------
// Saving

/// Where the auto-save copy goes: next to the model, else in Preferences › Files › Models,
/// else in Esquisse's folder. Named like SketchUp's ("AutoSave_name.esq").
pub fn autosave_path(app: &EsquisseApp) -> Option<PathBuf> {
    let stem = app
        .doc
        .path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Sans titre".into());
    let dir = app
        .doc
        .path
        .as_ref()
        .and_then(|p| p.parent().map(PathBuf::from))
        .or_else(|| settings::with(|s| s.location("models")))
        .or_else(settings::user_dir)?;
    Some(dir.join(format!("AutoSave_{stem}.esq")))
}

/// Every frame: auto-saves the changed model every so many minutes.
pub fn tick(app: &mut EsquisseApp, now: f64) {
    let (on, minutes) = settings::with(|s| (s.autosave, s.autosave_minutes.max(1)));
    let st = &mut app.ui_state.extra.dialogs.prefs.general;
    if !on || !app.doc.dirty {
        st.last_autosave = None;
        return;
    }
    let since = *st.last_autosave.get_or_insert(now);
    if now - since < minutes as f64 * 60.0 {
        return;
    }
    st.last_autosave = Some(now);
    if let Some(p) = autosave_path(app) {
        autosave_to(app, p);
    }
}

/// Writes the auto-save copy (the document keeps its file and stays modified).
pub fn autosave_to(app: &mut EsquisseApp, p: PathBuf) {
    match esq_core::io::save_esq(&app.doc.model, &p) {
        Ok(()) => app.ui_state.extra.dialogs.prefs.general.autosave_file = Some(p),
        Err(e) => {
            app.message = Some((format!("Erreur d'enregistrement automatique : {e:#}"), 0.0));
        }
    }
}

/// The model was saved: its auto-save copy is no longer needed.
pub fn autosave_done(app: &mut EsquisseApp) {
    let st = &mut app.ui_state.extra.dialogs.prefs.general;
    st.last_autosave = None;
    if let Some(p) = st.autosave_file.take() {
        let _ = std::fs::remove_file(p);
    }
}

/// Ask to purge at save: true when the question is put (the save waits for the answer).
pub fn purge_prompt(app: &mut EsquisseApp, save_as: bool) -> bool {
    let st = &mut app.ui_state.extra.dialogs.prefs.general;
    if std::mem::take(&mut st.purge_answered) || !settings::with(|s| s.purge_on_save) {
        return false;
    }
    let mut probe = app.doc.model.clone();
    if probe.purge_unused() == 0 {
        return false;
    }
    st.purge_ask = Some(save_as);
    true
}

/// The question, then the save.
pub fn purge_dialog(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(save_as) = app.ui_state.extra.dialogs.prefs.general.purge_ask else { return };
    let mut answer = None;
    crate::ui::centered("Purger").show(ctx, |ui| {
        ui.label("Le modèle contient des éléments inutilisés (composants, matières, étiquettes).");
        ui.label("Les purger avant d'enregistrer ?");
        ui.horizontal(|ui| {
            if ui.button("Purger").clicked() {
                answer = Some(Some(true));
            }
            if ui.button("Ne pas purger").clicked() {
                answer = Some(Some(false));
            }
            if ui.button("Annuler").clicked() {
                answer = Some(None);
            }
        });
    });
    let Some(a) = answer else { return };
    let st = &mut app.ui_state.extra.dialogs.prefs.general;
    st.purge_ask = None;
    let Some(purge) = a else { return };
    if purge {
        app.doc.commit("Supprimer les actifs inutilisés", |m, _| {
            m.purge_unused();
        });
    }
    app.ui_state.extra.dialogs.prefs.general.purge_answered = true;
    app.save(ctx, save_as);
}

// ------------------------------------------------------------------------------------------
// Opening

/// Check for problems: a model just opened is repaired when asked to, and the user told if
/// asked to.
pub fn after_open(app: &mut EsquisseApp) {
    let (fix, notify) = settings::with(|s| (s.auto_fix, s.notify_fix));
    if !fix {
        return;
    }
    let n = super::mi_stats::fix_problems(app, false);
    if n > 0 {
        // Repairing what was read is not a change the user made.
        app.doc.dirty = false;
        if notify {
            app.message = Some((format!("Esquisse a corrigé {n} problème(s) dans ce modèle."), 0.0));
        }
    }
}

// ------------------------------------------------------------------------------------------
// The page

pub fn general_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let before = settings::get();
    let mut s: Settings = before.clone();
    group(ui, "Enregistrement");
    ui.checkbox(&mut s.create_backup, "Créer une sauvegarde");
    ui.horizontal(|ui| {
        ui.checkbox(&mut s.autosave, "Enregistrement automatique");
        ui.add_enabled_ui(s.autosave, |ui| {
            ui.label("Toutes les");
            let r = ui.add(egui::DragValue::new(&mut s.autosave_minutes).range(1..=600));
            name_widget(ui, &r, "Minutes entre les enregistrements", Some(&s.autosave_minutes.to_string()));
            ui.label("minutes");
        });
    });
    ui.checkbox(&mut s.purge_on_save, "Demander de purger lors de la sauvegarde");

    group(ui, "Vérifier si des problèmes existent");
    ui.checkbox(&mut s.auto_fix, "Résoudre automatiquement les problèmes lorsqu'ils sont détectés");
    ui.add_enabled_ui(s.auto_fix, |ui| ui.checkbox(&mut s.notify_fix, "M'avertir lorsque les problèmes sont résolus"));

    group(ui, "Messages");
    if ui
        .add_enabled(!s.dismissed.is_empty(), egui::Button::new("Réinitialiser tous les messages d'avertissement"))
        .clicked()
    {
        s.dismissed.clear();
    }
    ui.checkbox(&mut s.urgent_only, "Afficher uniquement les notifications urgentes")
        .on_hover_text("Notifications des services en ligne de Trimble : Esquisse n'en reçoit pas.");

    group(ui, "Mises à jour du logiciel");
    ui.checkbox(&mut s.check_updates, "Permettre la recherche de mises à jour")
        .on_hover_text("Service en ligne de Trimble : sans effet dans Esquisse.");

    group(ui, "Fenêtre d'accueil");
    ui.checkbox(&mut s.show_welcome, "Afficher la fenêtre d'accueil");

    group(ui, "Paramètres des barres d'outils");
    crate::toolbars::ensure_loaded(&mut app.ui_state.extra.toolbars);
    let tb_before = app.ui_state.extra.toolbars.clone();
    let tb = &mut app.ui_state.extra.toolbars;
    ui.checkbox(&mut tb.tooltips, "Afficher les info-bulles sur les barres d'outils");
    ui.checkbox(&mut tb.large_icons, "Utiliser de grandes icônes");
    ui.label("Mode d'affichage des barres d'outils :");
    for (m, label) in [
        (DisplayMode::TextAndIcon, "Utiliser du texte et une icône"),
        (DisplayMode::TextOnly, "Utiliser du texte uniquement"),
        (DisplayMode::IconOnly, "Utiliser une icône uniquement"),
    ] {
        ui.radio_value(&mut tb.mode, m, label);
    }
    if *tb != tb_before {
        crate::toolbars::save(tb);
    }

    super::preferences::image_editor_group(ui, &mut s);

    if s != before {
        super::preferences::apply(app, s);
    }
}
