//! Window › Preferences: the user's settings (`settings.rs`), page by page, with SketchUp's
//! Import… / Export… of chosen groups of settings. Changes apply at once; Cancel puts back
//! the settings the window opened with.

use eframe::egui::{self, RichText};

use super::{PrefPage, group, page_list};
use crate::app::EsquisseApp;
use crate::prefs;
use crate::settings::{self, EXPORT_GROUPS, Settings};

#[derive(Default)]
pub struct PrefState {
    /// The settings when the window opened (Cancel restores them).
    pub opened_with: Option<Settings>,
    /// Import… / Export…: which one, the groups ticked, and for an import the file read.
    pub transfer: Option<Transfer>,
    pub general: super::pref_general::GeneralState,
    pub shortcuts: super::pref_shortcuts::ShortcutState,
    /// Graphics › Graphics Engine Details shown.
    pub gpu_details: bool,
    /// Workspace › Reset to Default: asking, then confirmed (done on OK).
    pub confirm_reset: bool,
    pub reset_workspace: bool,
}

pub struct Transfer {
    pub export: bool,
    pub groups: Vec<bool>,
    pub data: Option<serde_json::Value>,
}

pub fn window(app: &mut EsquisseApp, ctx: &egui::Context) {
    let st = &mut app.ui_state.extra.dialogs.prefs;
    if st.opened_with.is_none() {
        st.opened_with = Some(settings::get());
    }
    let pages: Vec<(PrefPage, &str)> = PrefPage::ALL.iter().map(|p| (*p, p.label())).collect();
    let mut page = app.ui_state.extra.dialogs.pref_page;
    let mut open = true;
    let mut close = None;
    egui::Window::new("Préférences d'Esquisse")
        .id(egui::Id::unique("preferences_window"))
        .collapsible(false)
        .resizable(true)
        .default_size([680.0, 500.0])
        .default_pos([200.0, 90.0])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                // The vertical separator takes all the height it is given: without a limit the
                // window would grow past the screen and hide OK / Cancel.
                ui.set_max_height(450.0);
                page_list(ui, &pages, &mut page);
                ui.separator();
                egui::ScrollArea::vertical().id_salt("prefs_page").max_height(440.0).show(ui, |ui| {
                    ui.set_min_width(460.0);
                    ui.vertical(|ui| page_ui(app, ui, page));
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Importer...").clicked() {
                    import_dialog(app);
                }
                if ui.button("Exporter...").clicked() {
                    app.ui_state.extra.dialogs.prefs.transfer =
                        Some(Transfer { export: true, groups: vec![true; EXPORT_GROUPS.len()], data: None });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Annuler").clicked() {
                        close = Some(false);
                    }
                    if ui.button("OK").clicked() {
                        close = Some(true);
                    }
                });
            });
        });
    app.ui_state.extra.dialogs.pref_page = page;
    transfer_dialog(app, ctx);
    if !open {
        close = Some(true);
    }
    if let Some(keep) = close {
        let st = &mut app.ui_state.extra.dialogs.prefs;
        let reset = std::mem::take(&mut st.reset_workspace);
        st.confirm_reset = false;
        if let Some(mut before) = st.opened_with.take()
            && !keep
        {
            // The trays are not on these pages: they stay as they are now.
            settings::with(|now| {
                before.default_tray = now.default_tray.clone();
                before.trays = now.trays.clone();
            });
            apply(app, before);
        }
        if keep && reset {
            super::pref_pages::reset_workspace(app);
        }
        app.ui_state.preferences = false;
    }
}

/// Makes settings current, and redoes what depends on them (scene colours…).
pub fn apply(app: &mut EsquisseApp, s: Settings) {
    let colors_changed = settings::with(|c| {
        c.axis_colors != s.axis_colors || c.parallel_color != s.parallel_color || c.tangent_color != s.tangent_color
    });
    settings::set(s);
    if colors_changed {
        app.doc.rev += 1;
    }
}

fn page_ui(app: &mut EsquisseApp, ui: &mut egui::Ui, page: PrefPage) {
    match page {
        PrefPage::Template => template_page(app, ui),
        PrefPage::General => super::pref_general::general_page(app, ui),
        PrefPage::Shortcuts => super::pref_shortcuts::shortcuts_page(app, ui),
        PrefPage::Accessibility => super::pref_pages::accessibility_page(app, ui),
        PrefPage::Compatibility => super::pref_pages::compatibility_page(app, ui),
        PrefPage::Drawing => super::pref_pages::drawing_page(app, ui),
        PrefPage::Applications => super::pref_pages::applications_page(app, ui),
        PrefPage::Files => super::pref_pages::files_page(app, ui),
        PrefPage::Graphics => super::pref_pages::graphics_page(app, ui),
        PrefPage::Workspace => super::pref_pages::workspace_page(app, ui),
    }
}

/// "Default Image Editor": the program texture images open with (General and Applications).
pub fn image_editor_group(ui: &mut egui::Ui, s: &mut Settings) {
    group(ui, "Éditeur d'images par défaut");
    ui.label("Un éditeur d'image externe peut être utilisé pour personnaliser des images de texture.");
    ui.horizontal(|ui| {
        let shown = if s.image_editor.is_empty() { "(aucun)".to_string() } else { s.image_editor.clone() };
        let r = ui.label(RichText::new(&shown).monospace());
        super::name_widget(ui, &r, "Éditeur d'images", Some(&shown));
        if ui.button("Choisir...").clicked()
            && let Some(p) = rfd::FileDialog::new().add_filter("Programmes", &["exe"]).pick_file()
        {
            s.image_editor = p.display().to_string();
        }
        if ui
            .add_enabled(!s.image_editor.is_empty(), egui::Button::new("Effacer"))
            .on_hover_text("Définit l'éditeur d'images sélectionné sur Aucun.")
            .clicked()
        {
            s.image_editor.clear();
        }
    });
}

// ------------------------------------------------------------------------------------------
// Import / Export

/// The file of Import… / Export… (SketchUp's `CImportPrefsDialog` / `CExportPrefsDialog`, with
/// workspaces: no Options button, OK "Exporter" for an export).
pub fn prefs_file_request(export: bool) -> crate::filedlg::Request {
    use crate::filedlg::{FileType, Request};
    if export {
        Request::save("Exporter les préférences", "Preferences", vec![FileType::new("Préférences", &["dat"])], 0, "")
            .ok_label("Exporter")
    } else {
        Request::open("Importer les préférences", vec![FileType::new("Préférences", &["dat", "json"])], 0, "")
    }
}

fn import_dialog(app: &mut EsquisseApp) {
    let Some(p) = crate::filedlg::run(prefs_file_request(false)).map(|c| c.path) else { return };
    let data = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    match data {
        Some(d) => {
            app.ui_state.extra.dialogs.prefs.transfer =
                Some(Transfer { export: false, groups: vec![true; EXPORT_GROUPS.len()], data: Some(d) })
        }
        None => app.message = Some(("Fichier de préférences illisible".into(), 0.0)),
    }
}

/// "Préférences d'exportation" / "Préférences d'importation": which groups to move.
fn transfer_dialog(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(t) = app.ui_state.extra.dialogs.prefs.transfer.as_mut() else { return };
    let title = if t.export { "Préférences d'exportation" } else { "Préférences d'importation" };
    let mut done = None;
    crate::ui::centered(title).show(ctx, |ui| {
        ui.label(if t.export {
            "Exporte toutes les préférences sélectionnées."
        } else {
            "Importe toutes les préférences sélectionnées."
        });
        let mut all = t.groups.iter().all(|g| *g);
        if ui.checkbox(&mut all, "Tout sélectionner").changed() {
            t.groups.iter_mut().for_each(|g| *g = all);
        }
        for (i, name) in EXPORT_GROUPS.iter().enumerate() {
            ui.checkbox(&mut t.groups[i], *name);
        }
        ui.horizontal(|ui| {
            if ui.button(if t.export { "Exporter" } else { "Importer" }).clicked() {
                done = Some(true);
            }
            if ui.button("Annuler").clicked() {
                done = Some(false);
            }
        });
    });
    let Some(ok) = done else { return };
    let t = app.ui_state.extra.dialogs.prefs.transfer.take().unwrap();
    if !ok {
        return;
    }
    let groups: Vec<&str> = EXPORT_GROUPS.iter().zip(&t.groups).filter(|(_, on)| **on).map(|(g, _)| *g).collect();
    if t.export {
        let Some(p) = crate::filedlg::run(prefs_file_request(true)).map(|c| c.path) else {
            return;
        };
        let j = settings::get().export_json(&groups);
        let msg = match std::fs::write(&p, serde_json::to_vec_pretty(&j).unwrap_or_default()) {
            Ok(()) => format!("Préférences exportées : {}", p.display()),
            Err(e) => format!("Échec de l'export : {e}"),
        };
        app.message = Some((msg, 0.0));
    } else if let Some(d) = t.data {
        let mut s = settings::get();
        s.import_json(&d, &groups);
        apply(app, s);
    }
}

// ------------------------------------------------------------------------------------------
// Template

/// Preferences › Template: the template File › New starts from.
fn template_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    group(ui, "Modèle type de dessin par défaut");
    let fav = prefs::default_template();
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_width(420.0);
        for t in prefs::all_templates() {
            let text = format!("{}  —  {}", t.title(), t.subtitle());
            let r = ui.selectable_label(t == fav, RichText::new(&text));
            super::name_widget(ui, &r, &t.name, Some(&text));
            if r.clicked() {
                prefs::set_default_template(&t);
            }
        }
    });
    ui.label(RichText::new(format!("Fichier › Nouveau part de « {} ».", fav.name)).weak());
    if ui.button("Parcourir...").clicked()
        && let Some(p) = settings::dialog("templates").add_filter("Modèles", &["esq", "skp"]).pick_file()
    {
        match prefs::add_template_file(&p) {
            Ok(t) => prefs::set_default_template(&t),
            Err(e) => app.message = Some((format!("Modèle type illisible : {e:#}"), 0.0)),
        }
    }
}
