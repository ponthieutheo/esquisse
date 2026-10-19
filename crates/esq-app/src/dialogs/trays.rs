//! Trays: the default tray and the ones the user makes with Window › New Tray, managed with
//! Window › Manage Trays (SketchUp's `CNewTrayDialog`, `CRenameTrayDialog`, `CTrayDialog`
//! and its docking trays). A panel lives in one tray at a time: adding it to a tray takes it
//! out of the others. The default tray is `ui_state.panels` / `ui_state.tray`; every tray is
//! kept in the preferences (`Settings::default_tray`, `Settings::trays`), so that the layout
//! comes back at the next start.
//!
//! Trays are numbered 0 (the default tray) then 1.. (`Settings::trays`).

use std::collections::HashSet;

use eframe::egui::{self, Color32, RichText};

use super::name_widget;
use crate::app::EsquisseApp;
use crate::commands::Panel;
use crate::settings::{self, Dock, TraySpec};

pub const DEFAULT_TRAY: &str = "Palette par défaut";

/// The name a panel is saved under.
pub fn key(p: Panel) -> String {
    format!("{p:?}")
}

pub fn panel(key_: &str) -> Option<Panel> {
    Panel::ALL.into_iter().find(|p| key(*p) == key_)
}

fn keys(panels: &[Panel]) -> Vec<String> {
    panels.iter().map(|p| key(*p)).collect()
}

fn panels_of(spec: &TraySpec) -> Vec<Panel> {
    spec.panels.iter().filter_map(|k| panel(k)).collect()
}

/// The default tray as installed: every panel but Match Photo, Hatch Patterns, Instructor (and
/// the Modeling Grid, behind a feature switch in SketchUp 2026).
fn installed_default() -> TraySpec {
    let panels: Vec<Panel> = Panel::ALL
        .into_iter()
        .filter(|p| !matches!(p, Panel::MatchPhoto | Panel::HatchPatterns | Panel::Instructor | Panel::ModelGrid))
        .collect();
    TraySpec { name: DEFAULT_TRAY.into(), panels: keys(&panels), visible: true, dock: Dock::Right }
}

fn saved_default() -> TraySpec {
    settings::with(|s| s.default_tray.clone()).unwrap_or_else(installed_default)
}

/// The panels of the default tray at start-up, and whether it shows.
pub fn initial_default() -> (HashSet<Panel>, bool) {
    let d = saved_default();
    (panels_of(&d).into_iter().collect(), d.visible)
}

/// The default tray as the application has it now.
fn default_now(app: &EsquisseApp) -> TraySpec {
    let panels: Vec<Panel> = Panel::ALL.into_iter().filter(|p| app.ui_state.panels.contains(p)).collect();
    TraySpec {
        name: DEFAULT_TRAY.into(),
        panels: keys(&panels),
        visible: app.ui_state.tray,
        dock: saved_default().dock,
    }
}

/// Every tray, the default one first.
pub fn all(app: &EsquisseApp) -> Vec<TraySpec> {
    let mut v = vec![default_now(app)];
    v.extend(settings::with(|s| s.trays.clone()));
    v
}

/// Called every frame: what Window › Default Tray changed is saved, and a panel put back in
/// the default tray leaves the tray it was in.
pub fn sync(app: &EsquisseApp) {
    let now = default_now(app);
    let before = saved_default();
    if now == before {
        return;
    }
    let added: Vec<String> = now.panels.iter().filter(|k| !before.panels.contains(k)).cloned().collect();
    settings::update(|s| {
        for t in &mut s.trays {
            t.panels.retain(|k| !added.contains(k));
        }
        s.default_tray = Some(now);
    });
}

/// Makes the trays these, the default one first (a workspace, Preferences › Workspace ›
/// Reset to Default).
pub fn set_all(app: &mut EsquisseApp, trays: &[TraySpec]) {
    let (default, others) = match trays.split_first() {
        Some((d, o)) => (d.clone(), o.to_vec()),
        None => (installed_default(), Vec::new()),
    };
    app.ui_state.panels = panels_of(&default).into_iter().collect();
    app.ui_state.tray = default.visible;
    settings::update(|s| {
        s.default_tray = Some(TraySpec { name: DEFAULT_TRAY.into(), ..default });
        s.trays = others;
    });
}

/// Back to the trays as installed.
pub fn reset(app: &mut EsquisseApp) {
    set_all(app, &[installed_default()]);
}

/// Why SketchUp refuses a tray name: blank, with one of `" ' &`, or already taken.
pub fn name_error(name: &str, taken: &[String]) -> Option<&'static str> {
    if name.chars().all(|c| c == ' ') || name.contains(['\'', '"', '&']) {
        return Some(
            "Le nom du plateau n'est pas valide. Les noms de plateaux ne doivent pas être vides ou contenir les caractères \" ' &",
        );
    }
    if taken.iter().any(|t| t == name) {
        return Some("Nom de plateau invalide. Les noms des plateaux doivent être uniques.");
    }
    None
}

fn names(app: &EsquisseApp) -> Vec<String> {
    all(app).into_iter().map(|t| t.name).collect()
}

/// Puts a panel in a tray (taking it out of the others) or takes it out.
pub fn set_panel(app: &mut EsquisseApp, tray: usize, p: Panel, on: bool) {
    let k = key(p);
    if tray == 0 {
        if on {
            app.ui_state.panels.insert(p);
        } else {
            app.ui_state.panels.remove(&p);
        }
        sync(app);
        return;
    }
    if on {
        app.ui_state.panels.remove(&p);
        sync(app);
    }
    settings::update(|s| {
        for (i, t) in s.trays.iter_mut().enumerate() {
            let had = t.panels.contains(&k);
            if i + 1 == tray && on && !had {
                t.panels.push(k.clone());
            } else if (i + 1 != tray && on) || (i + 1 == tray && !on) {
                t.panels.retain(|x| *x != k);
            }
        }
    });
}

/// Window › New Tray: a new tray, shown, with these panels.
pub fn create(app: &mut EsquisseApp, name: &str, panels: &[Panel]) -> Result<usize, &'static str> {
    if let Some(e) = name_error(name, &names(app)) {
        return Err(e);
    }
    settings::update(|s| {
        s.trays.push(TraySpec { name: name.into(), panels: Vec::new(), visible: true, dock: Dock::Right })
    });
    let t = all(app).len() - 1;
    for p in panels {
        set_panel(app, t, *p, true);
    }
    Ok(t)
}

pub fn rename(app: &mut EsquisseApp, tray: usize, name: &str) -> Result<(), &'static str> {
    let mut taken = names(app);
    taken.remove(tray);
    if let Some(e) = name_error(name, &taken) {
        return Err(e);
    }
    settings::update(|s| {
        if let Some(t) = s.trays.get_mut(tray.wrapping_sub(1)) {
            t.name = name.into();
        }
    });
    Ok(())
}

/// Deletes trays (never the default one); their panels are closed.
pub fn delete(app: &mut EsquisseApp, trays: &[usize]) {
    let _ = app;
    settings::update(|s| {
        let mut i = 0;
        s.trays.retain(|_| {
            i += 1;
            !trays.contains(&i)
        });
    });
}

pub fn set_visible(app: &mut EsquisseApp, tray: usize, on: bool) {
    if tray == 0 {
        app.ui_state.tray = on;
        sync(app);
        return;
    }
    settings::update(|s| {
        if let Some(t) = s.trays.get_mut(tray - 1) {
            t.visible = on;
        }
    });
}

pub fn set_dock(app: &mut EsquisseApp, tray: usize, dock: Dock) {
    if tray == 0 {
        let mut d = default_now(app);
        d.dock = dock;
        settings::update(|s| s.default_tray = Some(d));
        return;
    }
    settings::update(|s| {
        if let Some(t) = s.trays.get_mut(tray - 1) {
            t.dock = dock;
        }
    });
}

/// "A", "A et B", "A, B, et C" (SketchUp's list of trays to delete).
fn join(names: &[String]) -> String {
    let mut out = String::new();
    for (i, n) in names.iter().enumerate() {
        if !out.is_empty() {
            if i == names.len() - 1 {
                if names.len() > 2 {
                    out.push(',');
                }
                out.push_str(" et ");
            } else {
                out.push_str(", ");
            }
        }
        out.push_str(n);
    }
    out
}

// ------------------------------------------------------------------------------------------
// Windows

#[derive(Default)]
pub struct TrayState {
    /// Window › New Tray: the name typed, the panels ticked.
    pub new: Option<(String, Vec<Panel>)>,
    /// Window › Manage Trays: the trays selected in the list.
    pub manage: Option<Vec<usize>>,
    /// Rename Tray: which tray, the name typed.
    pub rename: Option<(usize, String)>,
    /// Trays waiting for "Oui" before they are deleted.
    pub delete: Option<Vec<usize>>,
    /// "Nom de palette non valide".
    pub error: Option<&'static str>,
}

pub fn open_new(app: &mut EsquisseApp) {
    app.ui_state.extra.dialogs.trays.new = Some((String::new(), Vec::new()));
}

pub fn open_manage(app: &mut EsquisseApp) {
    app.ui_state.extra.dialogs.trays.manage = Some(vec![0]);
}

fn modal_open(app: &EsquisseApp) -> bool {
    let st = &app.ui_state.extra.dialogs.trays;
    st.rename.is_some() || st.delete.is_some() || st.error.is_some()
}

fn small_window(title: &str, pos: [f32; 2]) -> egui::Window<'_> {
    egui::Window::new(title).collapsible(false).resizable(false).default_pos(pos)
}

/// The tray windows, with the other dialogs.
pub fn windows(app: &mut EsquisseApp, ctx: &egui::Context) {
    new_window(app, ctx);
    manage_window(app, ctx);
    rename_window(app, ctx);
    delete_window(app, ctx);
    if let Some(msg) = app.ui_state.extra.dialogs.trays.error {
        small_window("Nom de palette non valide", [420.0, 260.0]).show(ctx, |ui| {
            ui.label(msg);
            if ui.button("OK").clicked() {
                app.ui_state.extra.dialogs.trays.error = None;
            }
        });
    }
}

fn panel_checks(ui: &mut egui::Ui, ticked: &[Panel], mut on_change: impl FnMut(Panel, bool)) {
    for p in Panel::ALL {
        let mut on = ticked.contains(&p);
        if ui.checkbox(&mut on, p.label()).changed() {
            on_change(p, on);
        }
    }
}

fn new_window(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some((mut name, mut ticked)) = app.ui_state.extra.dialogs.trays.new.take() else { return };
    let modal = modal_open(app);
    let mut keep = true;
    let mut add = false;
    small_window("Nouvelle palette", [360.0, 140.0]).show(ctx, |ui| {
        ui.add_enabled_ui(!modal, |ui| {
            ui.horizontal(|ui| {
                ui.label("Nom :");
                super::text_field(ui, "Nom :", &mut name, 200.0);
            });
            ui.label("Dialogues :");
            ui.separator();
            panel_checks(ui, &ticked.clone(), |p, on| {
                ticked.retain(|x| *x != p);
                if on {
                    ticked.push(p);
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Tout sélectionner").clicked() {
                    ticked = Panel::ALL.to_vec();
                }
                add = ui.button("Ajouter").clicked();
                if ui.button("Annuler").clicked() {
                    keep = false;
                }
            });
        });
    });
    if add {
        let panels: Vec<Panel> = Panel::ALL.into_iter().filter(|p| ticked.contains(p)).collect();
        match create(app, &name, &panels) {
            Ok(t) => {
                keep = false;
                if let Some(sel) = &mut app.ui_state.extra.dialogs.trays.manage {
                    *sel = vec![t];
                }
            }
            Err(e) => app.ui_state.extra.dialogs.trays.error = Some(e),
        }
    }
    if keep {
        app.ui_state.extra.dialogs.trays.new = Some((name, ticked));
    }
}

fn manage_window(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(mut sel) = app.ui_state.extra.dialogs.trays.manage.take() else { return };
    let modal = modal_open(app) || app.ui_state.extra.dialogs.trays.new.is_some();
    let trays = all(app);
    sel.retain(|t| *t < trays.len());
    let mut keep = true;
    small_window("Gérer les palettes", [300.0, 120.0]).show(ctx, |ui| {
        ui.add_enabled_ui(!modal, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.label("Plateaux :");
                    egui::Frame::new().stroke(egui::Stroke::new(1.0, Color32::from_gray(170))).inner_margin(4).show(
                        ui,
                        |ui| {
                            ui.set_min_size(egui::vec2(170.0, 160.0));
                            for (i, t) in trays.iter().enumerate() {
                                let r = ui.selectable_label(sel.contains(&i), &t.name);
                                if r.clicked() {
                                    if ui.input(|i| i.modifiers.command || i.modifiers.shift) {
                                        if !sel.contains(&i) {
                                            sel.push(i);
                                        } else {
                                            sel.retain(|x| *x != i);
                                        }
                                    } else {
                                        sel = vec![i];
                                    }
                                }
                            }
                        },
                    );
                });
                ui.vertical(|ui| {
                    ui.label("Dialogues :");
                    // The panels of the selected tray (one tray at a time).
                    ui.add_enabled_ui(sel.len() == 1, |ui| {
                        let t = sel.first().copied().unwrap_or(0);
                        let ticked = if sel.len() == 1 { panels_of(&trays[t]) } else { Vec::new() };
                        let mut change = None;
                        panel_checks(ui, &ticked, |p, on| change = Some((p, on)));
                        if let Some((p, on)) = change {
                            set_panel(app, t, p, on);
                        }
                    });
                });
                ui.vertical(|ui| {
                    let button = |text: &str| egui::Button::new(text).min_size(egui::vec2(96.0, 0.0));
                    if ui.add(button("Nouveau")).clicked() {
                        open_new(app);
                    }
                    let one_user = sel.len() == 1 && sel[0] > 0;
                    if ui.add_enabled(one_user, button("Renommer")).clicked() {
                        app.ui_state.extra.dialogs.trays.rename = Some((sel[0], trays[sel[0]].name.clone()));
                    }
                    let users = !sel.is_empty() && !sel.contains(&0);
                    if ui.add_enabled(users, button("Supprimer")).clicked() {
                        let mut d = sel.clone();
                        d.sort();
                        app.ui_state.extra.dialogs.trays.delete = Some(d);
                    }
                    ui.add_space(12.0);
                    if ui.add(button("Fermer")).clicked() {
                        keep = false;
                    }
                });
            });
        });
    });
    if keep {
        app.ui_state.extra.dialogs.trays.manage = Some(sel);
    }
}

fn rename_window(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some((tray, mut name)) = app.ui_state.extra.dialogs.trays.rename.take() else { return };
    let mut keep = true;
    let mut ok = false;
    let error = app.ui_state.extra.dialogs.trays.error.is_some();
    small_window("Renommer le plateau", [380.0, 200.0]).show(ctx, |ui| {
        ui.add_enabled_ui(!error, |ui| {
            ui.label(RichText::new("Nom :").strong());
            let (done, _) = super::text_field(ui, "Nom :", &mut name, 220.0);
            ok = done && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                ok |= ui.button("Renommer").clicked();
                if ui.button("Annuler").clicked() {
                    keep = false;
                }
            });
        });
    });
    if ok {
        match rename(app, tray, &name) {
            Ok(()) => keep = false,
            Err(e) => app.ui_state.extra.dialogs.trays.error = Some(e),
        }
    }
    if keep {
        app.ui_state.extra.dialogs.trays.rename = Some((tray, name));
    }
}

fn delete_window(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(list) = app.ui_state.extra.dialogs.trays.delete.take() else { return };
    let trays = all(app);
    let names: Vec<String> = list.iter().filter_map(|t| trays.get(*t)).map(|t| t.name.clone()).collect();
    let text = if names.len() == 1 {
        format!("Souhaitez-vous supprimer le plateau : {} ?", join(&names))
    } else {
        format!("Souhaitez-vous supprimer les plateaux : {} ?", join(&names))
    };
    let mut answer = None;
    small_window("Esquisse", [420.0, 240.0]).show(ctx, |ui| {
        ui.label(text);
        ui.horizontal(|ui| {
            if ui.button("Oui").clicked() {
                answer = Some(true);
            }
            if ui.button("Non").clicked() {
                answer = Some(false);
            }
        });
    });
    match answer {
        Some(true) => {
            delete(app, &list);
            if let Some(sel) = &mut app.ui_state.extra.dialogs.trays.manage {
                *sel = vec![0];
            }
        }
        Some(false) => {}
        None => app.ui_state.extra.dialogs.trays.delete = Some(list),
    }
}

// ------------------------------------------------------------------------------------------
// The trays on screen

/// Window menu: one entry per tray the user made, ticked when it shows.
pub fn menu_items(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let trays = all(app);
    for (i, t) in trays.iter().enumerate().skip(1) {
        let r = ui.add(egui::Button::new(format!("{}{}", if t.visible { "✔  " } else { "     " }, t.name)));
        name_widget(ui, &r, &t.name, None);
        if r.clicked() {
            ui.close();
            set_visible(app, i, !t.visible);
        }
    }
}

/// The menu of a tray (its title bar's menu in SketchUp).
fn tray_menu(app: &mut EsquisseApp, ui: &mut egui::Ui, tray: usize, spec: &TraySpec) {
    if ui.button("Masquer le plateau").clicked() {
        ui.close();
        set_visible(app, tray, false);
    }
    if ui.add_enabled(tray > 0, egui::Button::new("Renommer le plateau")).clicked() {
        ui.close();
        app.ui_state.extra.dialogs.trays.rename = Some((tray, spec.name.clone()));
    }
    if ui.add_enabled(tray > 0, egui::Button::new("Supprimer le plateau")).clicked() {
        ui.close();
        app.ui_state.extra.dialogs.trays.delete = Some(vec![tray]);
    }
    ui.separator();
    for (dock, label) in
        [(Dock::Right, "Ancrer à droite"), (Dock::Left, "Ancrer à gauche"), (Dock::Floating, "Flottant")]
    {
        if ui.radio(spec.dock == dock, label).clicked() {
            ui.close();
            set_dock(app, tray, dock);
        }
    }
    ui.separator();
    let ticked = panels_of(spec);
    for p in Panel::ALL {
        let mut on = ticked.contains(&p);
        if ui.checkbox(&mut on, p.label()).changed() {
            set_panel(app, tray, p, on);
        }
    }
}

/// Draws every tray that shows, docked or floating.
pub fn show(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    for (i, spec) in all(app).into_iter().enumerate() {
        if !spec.visible {
            continue;
        }
        let id = if i == 0 { "tray".to_string() } else { format!("tray-{}", spec.name) };
        let body = |app: &mut EsquisseApp, ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::Label::new(RichText::new(&spec.name).small().color(Color32::from_gray(110)))
                        .sense(egui::Sense::click()),
                );
                r.context_menu(|ui| tray_menu(app, ui, i, &spec));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let m = ui.menu_button("☰", |ui| tray_menu(app, ui, i, &spec));
                    name_widget(ui, &m.response, &format!("Menu de {}", spec.name), None);
                });
            });
            crate::ui::tray_panels(app, ui, &id, &panels_of(&spec));
        };
        match spec.dock {
            Dock::Right => {
                egui::Panel::right(id.clone()).default_size(290.0).resizable(true).show(ui, |ui| body(app, ui));
            }
            Dock::Left => {
                egui::Panel::left(id.clone()).default_size(290.0).resizable(true).show(ui, |ui| body(app, ui));
            }
            Dock::Floating => {
                let mut open = true;
                egui::Window::new(&spec.name)
                    .id(egui::Id::unique(id.as_str()))
                    .default_size([290.0, 420.0])
                    .default_pos([900.0, 120.0])
                    .open(&mut open)
                    .show(ui.ctx(), |ui| body(app, ui));
                if !open {
                    set_visible(app, i, false);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_names_follow_sketchups_rules() {
        let taken = vec![DEFAULT_TRAY.to_string(), "Atelier".to_string()];
        assert!(name_error("Mes outils", &taken).is_none());
        for bad in ["", "   ", "A & B", "l'atelier", "\"x\""] {
            assert!(name_error(bad, &taken).unwrap().contains("vides"), "{bad}");
        }
        assert!(name_error("Atelier", &taken).unwrap().contains("uniques"));
        assert_eq!(join(&["A".into()]), "A");
        assert_eq!(join(&["A".into(), "B".into()]), "A et B");
        assert_eq!(join(&["A".into(), "B".into(), "C".into()]), "A, B, et C");
    }
}
