//! Workspaces: a layout of the window (trays, toolbars, Ruby toolbars, window geometry)
//! saved under a name and brought back in one click, from Window › Workspace. SketchUp's
//! `WorkspaceConfigDialog` sets what a workspace keeps: its ID, its name, whether the user
//! may change it, the window geometry (unchanged, maximized, absolute), which Ruby toolbars
//! it tracks and what becomes of the others. Workspaces are JSON files in the Workspaces
//! folder of Preferences › Files (else `%APPDATA%\Esquisse\Espaces de travail`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eframe::egui;
use serde::{Deserialize, Serialize};

use super::{combo, name_widget, text_field};
use crate::app::EsquisseApp;
use crate::settings::{self, TraySpec};

pub const UNNAMED: &str = "Espace de travail sans nom";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Geometry {
    #[default]
    Unchanged,
    Maximized,
    Absolute,
}

/// Which Ruby toolbars the workspace keeps the state of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Track {
    #[default]
    All,
    None,
    Selected,
}

/// What becomes of the Ruby toolbars the workspace does not track.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Untracked {
    #[default]
    Unchanged,
    Hide,
    Show,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub user_editable: bool,
    pub geometry: Geometry,
    /// Window position and size (x, y, width, height), for `Geometry::Absolute`.
    pub window: Option<[f32; 4]>,
    /// The toolbars (`toolbars::ToolbarConfig::to_text`).
    pub toolbars: String,
    /// The trays, the default one first.
    pub trays: Vec<TraySpec>,
    pub ruby_track: Track,
    pub ruby_selected: Vec<String>,
    /// Shown or not, for each tracked Ruby toolbar.
    pub ruby_toolbars: BTreeMap<String, bool>,
    pub ruby_untracked: Untracked,
}

impl Default for Workspace {
    fn default() -> Self {
        Workspace {
            id: esq_core::meta::new_guid(),
            name: UNNAMED.into(),
            user_editable: true,
            geometry: Geometry::Unchanged,
            window: None,
            toolbars: String::new(),
            trays: Vec::new(),
            ruby_track: Track::All,
            ruby_selected: Vec::new(),
            ruby_toolbars: BTreeMap::new(),
            ruby_untracked: Untracked::Unchanged,
        }
    }
}

/// SketchUp's ID mask: `HHHHHHHH-HHHH-HHHH-HHHH-HHHHHHHHHHHH`.
pub fn valid_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12]) && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_hexdigit()))
}

impl Workspace {
    fn tracks(&self, toolbar: &str) -> bool {
        match self.ruby_track {
            Track::All => true,
            Track::None => false,
            Track::Selected => self.ruby_selected.iter().any(|n| n == toolbar),
        }
    }

    /// The Ruby toolbars to show or hide when the workspace is applied, among `names`.
    pub fn ruby_states(&self, names: &[String]) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        for n in names {
            match self.ruby_toolbars.get(n).filter(|_| self.tracks(n)) {
                Some(on) => out.push((n.clone(), *on)),
                None => match self.ruby_untracked {
                    Untracked::Unchanged => {}
                    Untracked::Hide => out.push((n.clone(), false)),
                    Untracked::Show => out.push((n.clone(), true)),
                },
            }
        }
        out
    }
}

// ------------------------------------------------------------------------------------------
// Files

/// The folder of the workspaces (Preferences › Files › Workspaces).
pub fn folder() -> Option<PathBuf> {
    let d = match settings::with(|s| s.location("workspaces")) {
        Some(d) => d,
        None => settings::user_dir()?.join("Espaces de travail"),
    };
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

/// Reads a workspace file: JSON, or SketchUp's `.sws` (a zip holding the JSON).
pub fn read(path: &Path) -> Result<Workspace, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let json = if bytes.starts_with(b"PK") {
        super::mi_classif::zip_entries(&bytes)
            .unwrap_or_default()
            .into_iter()
            .find(|(n, _)| n.to_lowercase().ends_with(".json"))
            .map(|(_, b)| b)
            .ok_or("pas de fichier JSON dans l'archive")?
    } else {
        bytes
    };
    let v: serde_json::Value =
        serde_json::from_slice(json.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&json)).map_err(|e| e.to_string())?;
    // SketchUp's "Error parsing workspace. No ID found".
    if !v.get("id").and_then(|i| i.as_str()).is_some_and(valid_id) {
        return Err("aucun identifiant".into());
    }
    serde_json::from_value(v).map_err(|e| e.to_string())
}

pub fn write(ws: &Workspace, path: &Path) -> Result<(), String> {
    let b = serde_json::to_vec_pretty(ws).map_err(|e| e.to_string())?;
    std::fs::write(path, b).map_err(|e| e.to_string())
}

/// The saved workspaces, by name.
pub fn list() -> Vec<Workspace> {
    let Some(d) = folder() else { return Vec::new() };
    let mut v: Vec<Workspace> = std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("json")))
        .filter_map(|e| read(&e.path()).ok())
        .collect();
    v.sort_by_key(|w| w.name.to_lowercase());
    v
}

/// Keeps a workspace in the folder (replacing the one with the same ID).
pub fn store(ws: &Workspace) -> Result<(), String> {
    let d = folder().ok_or("dossier des espaces de travail inaccessible")?;
    write(ws, &d.join(format!("{}.json", ws.id)))
}

pub fn current_id() -> String {
    settings::with(|s| s.workspace.clone())
}

fn current() -> Option<Workspace> {
    let id = current_id();
    list().into_iter().find(|w| w.id == id)
}

// ------------------------------------------------------------------------------------------
// Taking and applying a layout

/// Puts the present layout of the window in a workspace.
pub fn capture(app: &mut EsquisseApp, ctx: &egui::Context, ws: &mut Workspace) {
    crate::toolbars::ensure_loaded(&mut app.ui_state.extra.toolbars);
    ws.toolbars = app.ui_state.extra.toolbars.to_text();
    ws.trays = super::trays::all(app);
    let names: Vec<String> = app.ui_state.extra.ruby.toolbars.iter().map(|b| b.name.clone()).collect();
    ws.ruby_toolbars =
        names.iter().filter(|n| ws.tracks(n)).map(|n| (n.clone(), crate::ruby_host::plugin_toolbar_shown(n))).collect();
    if ws.geometry == Geometry::Absolute {
        ws.window = ctx.input(|i| i.viewport().outer_rect).map(|r| [r.min.x, r.min.y, r.width(), r.height()]);
    }
}

/// Lays the window out as a workspace says, and makes it the current one.
pub fn apply(app: &mut EsquisseApp, ctx: &egui::Context, ws: &Workspace) {
    if !ws.trays.is_empty() {
        super::trays::set_all(app, &ws.trays);
    }
    if !ws.toolbars.is_empty() {
        let open = app.ui_state.extra.toolbars.open;
        let mut tb = crate::toolbars::ToolbarConfig::from_text(&ws.toolbars);
        tb.open = open;
        crate::toolbars::save(&tb);
        app.ui_state.extra.toolbars = tb;
    }
    let names: Vec<String> = app.ui_state.extra.ruby.toolbars.iter().map(|b| b.name.clone()).collect();
    for (n, on) in ws.ruby_states(&names) {
        crate::ruby_host::set_plugin_toolbar_shown(&n, on);
    }
    match (ws.geometry, ws.window) {
        (Geometry::Maximized, _) => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true)),
        (Geometry::Absolute, Some([x, y, w, h])) => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
        }
        _ => {}
    }
    let id = ws.id.clone();
    settings::update(|s| s.workspace = id);
}

/// Window › Workspace › Load Workspace: a workspace file taken in. When its ID is already
/// there, SketchUp asks whether to replace it or keep a copy.
pub fn load_file(app: &mut EsquisseApp, ctx: &egui::Context, path: &Path) {
    match read(path) {
        Ok(ws) => {
            if list().iter().any(|w| w.id == ws.id) {
                app.ui_state.extra.dialogs.workspaces.exists = Some(ws);
            } else {
                take_in(app, ctx, ws);
            }
        }
        Err(_) => {
            app.ui_state.extra.dialogs.workspaces.error = Some(format!(
                "Échec de l'importation de l'espace de travail depuis {}. Le fichier est peut-être endommagé ou dans un format non valide.",
                path.display()
            ))
        }
    }
}

fn take_in(app: &mut EsquisseApp, ctx: &egui::Context, ws: Workspace) {
    match store(&ws) {
        Ok(()) => apply(app, ctx, &ws),
        Err(e) => app.ui_state.extra.dialogs.workspaces.error = Some(e),
    }
}

/// Window › Workspace › Save Workspace: the present layout in a file of the user's choice.
pub fn save_file(app: &mut EsquisseApp, ctx: &egui::Context, path: &Path) -> Result<(), String> {
    let mut ws = current().unwrap_or_default();
    capture(app, ctx, &mut ws);
    write(&ws, path)
}

// ------------------------------------------------------------------------------------------
// Windows

#[derive(Default)]
pub struct WorkspaceState {
    /// The configuration window: the workspace being set up, and the ID it had (None for a
    /// new one).
    pub edit: Option<(Workspace, Option<String>)>,
    /// A loaded workspace whose ID is already there ("L'espace de travail existe").
    pub exists: Option<Workspace>,
    pub error: Option<String>,
}

/// Window › Workspace › New Workspace…
pub fn open_new(app: &mut EsquisseApp) {
    app.ui_state.extra.dialogs.workspaces.edit = Some((Workspace::default(), None));
}

/// Window › Workspace › Configure Workspace…: the current one (if the user may change it).
pub fn open_configure(app: &mut EsquisseApp) {
    match current() {
        Some(ws) if ws.user_editable => {
            let id = ws.id.clone();
            app.ui_state.extra.dialogs.workspaces.edit = Some((ws, Some(id)));
        }
        Some(_) => {
            app.ui_state.extra.dialogs.workspaces.error = Some("Cet espace de travail n'est pas modifiable.".into())
        }
        None => open_new(app),
    }
}

pub fn save_dialog(app: &mut EsquisseApp, ctx: &egui::Context) {
    let name = current().map_or(UNNAMED.to_string(), |w| w.name);
    if let Some(p) = settings::dialog("workspaces")
        .add_filter("Espace de travail", &["json"])
        .set_file_name(format!("{name}.json"))
        .save_file()
        && let Err(e) = save_file(app, ctx, &p)
    {
        app.ui_state.extra.dialogs.workspaces.error = Some(e);
    }
}

pub fn load_dialog(app: &mut EsquisseApp, ctx: &egui::Context) {
    if let Some(p) =
        settings::dialog("workspaces").add_filter("Fichiers d'espace de travail", &["json", "sws"]).pick_file()
    {
        load_file(app, ctx, &p);
    }
}

/// Window › Workspace: the saved workspaces, the current one ticked.
pub fn menu_items(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let cur = current_id();
    let all = list();
    if all.is_empty() {
        ui.add_enabled(false, egui::Button::new("(aucun espace de travail)"));
    }
    for ws in all {
        let tick = if ws.id == cur { "✔  " } else { "     " };
        let r = ui.button(format!("{tick}{}", ws.name));
        name_widget(ui, &r, &ws.name, None);
        if r.clicked() {
            ui.close();
            let ctx = ui.ctx().clone();
            apply(app, &ctx, &ws);
        }
    }
}

/// The windows of this module, with the other dialogs.
pub fn windows(app: &mut EsquisseApp, ctx: &egui::Context) {
    config_window(app, ctx);
    exists_window(app, ctx);
    if let Some(msg) = app.ui_state.extra.dialogs.workspaces.error.clone() {
        crate::ui::centered("Espaces de travail").show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.label(msg);
            if ui.button("OK").clicked() {
                app.ui_state.extra.dialogs.workspaces.error = None;
            }
        });
    }
}

/// SketchUp's `WorkspaceConfigDialog`.
fn config_window(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some((mut ws, old_id)) = app.ui_state.extra.dialogs.workspaces.edit.take() else { return };
    let names: Vec<String> = app.ui_state.extra.ruby.toolbars.iter().map(|b| b.name.clone()).collect();
    let mut done = None;
    egui::Window::new("Configuration de l'espace de travail")
        .collapsible(false)
        .resizable(false)
        .default_pos([340.0, 140.0])
        .show(ctx, |ui| {
            egui::Grid::new("ws_config").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("ID :");
                ui.horizontal(|ui| {
                    text_field(ui, "ID", &mut ws.id, 280.0);
                    if ui.button("Générer").clicked() {
                        ws.id = esq_core::meta::new_guid();
                    }
                });
                ui.end_row();
                ui.label("Nom :");
                text_field(ui, "Nom de l'espace de travail", &mut ws.name, 280.0);
                ui.end_row();
                ui.label("Modifiable par l'utilisateur :");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut ws.user_editable, true, "Oui");
                    ui.radio_value(&mut ws.user_editable, false, "Non");
                });
                ui.end_row();
                ui.label("Géométrie :");
                combo(
                    ui,
                    "Géométrie",
                    &mut ws.geometry,
                    &[
                        (Geometry::Unchanged, "Inchangé".into()),
                        (Geometry::Maximized, "Maximisé".into()),
                        (Geometry::Absolute, "Absolu".into()),
                    ],
                );
                ui.end_row();
            });
            ui.add_space(6.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(egui::RichText::new("Barres d'outils Ruby").strong());
                ui.horizontal(|ui| {
                    ui.label("Suivre :");
                    combo(
                        ui,
                        "Suivre",
                        &mut ws.ruby_track,
                        &[
                            (Track::All, "Tous".into()),
                            (Track::None, "Aucune".into()),
                            (Track::Selected, "Sélection".into()),
                        ],
                    )
                })
                .response
                .on_hover_text(
                    "Quelles sont les barres d'outils Ruby à suivre dans le cadre de cet espace de travail ?",
                );
                ui.add_enabled_ui(ws.ruby_track == Track::Selected, |ui| {
                    if names.is_empty() {
                        ui.label(egui::RichText::new("(aucune barre d'outils Ruby)").weak());
                    }
                    for n in &names {
                        let mut on = ws.ruby_selected.contains(n);
                        if ui.checkbox(&mut on, n).changed() {
                            ws.ruby_selected.retain(|x| x != n);
                            if on {
                                ws.ruby_selected.push(n.clone());
                            }
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Non suivies :");
                    combo(
                        ui,
                        "Non suivies",
                        &mut ws.ruby_untracked,
                        &[
                            (Untracked::Unchanged, "Inchangé".into()),
                            (Untracked::Hide, "Masquer".into()),
                            (Untracked::Show, "Afficher".into()),
                        ],
                    )
                })
                .response
                .on_hover_text(
                    "Comment gérer l'état de la barre d'outils Ruby qui n'est pas suivi par cet espace de travail ?",
                );
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let ok = valid_id(&ws.id);
                if ui.add_enabled(ok, egui::Button::new("OK")).clicked() {
                    done = Some(true);
                }
                if ui.button("Annuler").clicked() {
                    done = Some(false);
                }
                if !ok {
                    ui.label(egui::RichText::new("ID non valide").color(egui::Color32::from_rgb(170, 60, 0)));
                }
            });
        });
    match done {
        Some(true) => {
            if ws.name.trim().is_empty() {
                ws.name = UNNAMED.into();
            }
            // A new ID: the workspace leaves its old file.
            if let Some(old) = old_id.filter(|o| *o != ws.id)
                && let Some(d) = folder()
            {
                let _ = std::fs::remove_file(d.join(format!("{old}.json")));
            }
            capture(app, ctx, &mut ws);
            take_in(app, ctx, ws);
        }
        Some(false) => {}
        None => app.ui_state.extra.dialogs.workspaces.edit = Some((ws, old_id)),
    }
}

/// "L'espace de travail existe": replace it, keep a copy, or give up.
fn exists_window(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(ws) = app.ui_state.extra.dialogs.workspaces.exists.take() else { return };
    let mut answer = None;
    crate::ui::centered("L'espace de travail existe").show(ctx, |ui| {
        ui.label("Cet espace de travail est déjà présent. Souhaitez-vous remplacer l'espace de travail existant ?");
        ui.horizontal(|ui| {
            for (label, a) in [("Oui", 0), ("Non", 1), ("Annuler", 2)] {
                if ui.button(label).clicked() {
                    answer = Some(a);
                }
            }
        });
    });
    match answer {
        Some(0) => take_in(app, ctx, ws),
        Some(1) => {
            let copy = Workspace { id: esq_core::meta::new_guid(), name: format!("{} Copie", ws.name), ..ws };
            take_in(app, ctx, copy);
        }
        Some(_) => {}
        None => app.ui_state.extra.dialogs.workspaces.exists = Some(ws),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_ruby_toolbar_tracking() {
        assert!(valid_id(&esq_core::meta::new_guid()));
        assert!(!valid_id("1234") && !valid_id("ZZZZZZZZ-0000-0000-0000-000000000000"));

        let names: Vec<String> = ["Plan", "Coupe", "Rendu"].map(String::from).to_vec();
        let mut ws = Workspace {
            ruby_track: Track::Selected,
            ruby_selected: vec!["Plan".into()],
            ruby_toolbars: [("Plan".to_string(), true), ("Coupe".to_string(), true)].into(),
            ruby_untracked: Untracked::Hide,
            ..Default::default()
        };
        // "Coupe" was saved but is not tracked any more: it follows "Non suivies".
        assert_eq!(
            ws.ruby_states(&names),
            vec![("Plan".into(), true), ("Coupe".into(), false), ("Rendu".into(), false)]
        );
        ws.ruby_untracked = Untracked::Unchanged;
        ws.ruby_track = Track::All;
        assert_eq!(ws.ruby_states(&names), vec![("Plan".into(), true), ("Coupe".into(), true)]);
        ws.ruby_track = Track::None;
        ws.ruby_untracked = Untracked::Show;
        assert!(ws.ruby_states(&names).iter().all(|(_, on)| *on));
    }
}
