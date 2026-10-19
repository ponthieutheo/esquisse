//! Toolbar, status bar, the default tray (Entity Info, Materials, Components, Styles,
//! Tags, Soften Edges, Outliner) and the dialogs. The menus are in `menus.rs`.

use std::collections::HashSet;

use eframe::egui::{self, Color32, RichText, Sense, Stroke};

use crate::app::EsquisseApp;
use crate::commands::{Cmd, Panel};
use crate::prefs::{self, Template};

pub struct UiState {
    pub ask_new: bool,
    pub show_about: bool,
    pub welcome: bool,
    pub templates: bool,
    pub save_template: Option<String>,
    pub model_info: bool,
    pub preferences: bool,
    /// Help › Search: the text typed so far.
    pub search: Option<String>,
    /// Panels shown in the tray, and whether the tray is shown at all.
    pub panels: HashSet<Panel>,
    pub tray: bool,
    /// Panel to unfold at the next frame (opened from the Window menu).
    pub unfold: Option<Panel>,
    /// Window › Soften Edges: angle in degrees, smooth normals, soften coplanar.
    pub soften: (f64, bool, bool),
    /// The text being edited (Text tool, double-click on a text or a dimension).
    pub edit_text: Option<(esq_core::model::AnnotId, String)>,
    pub last_wheel: f64,
    /// Export, print, toolbar and attribute dialogs (see `extra.rs`).
    pub extra: crate::extra::Extra,
    /// Tools › 3D Text: the open dialog, and the settings of the last use.
    pub text3d: Option<crate::text3d::Text3dState>,
    pub text3d_last: Option<crate::text3d::Text3dState>,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            ask_new: false,
            show_about: false,
            welcome: false,
            templates: false,
            save_template: None,
            model_info: false,
            preferences: false,
            search: None,
            panels: crate::dialogs::trays::initial_default().0,
            tray: crate::dialogs::trays::initial_default().1,
            unfold: None,
            soften: (20.0, true, true),
            edit_text: None,
            last_wheel: 0.0,
            extra: Default::default(),
            text3d: None,
            text3d_last: None,
        }
    }
}

/// Segoe UI as on Windows (and as SketchUp), with Segoe UI Symbol for the glyphs egui's own
/// fonts lack (arrows, check marks, hearts). Read from the system, never shipped.
fn setup_fonts(ctx: &egui::Context) {
    let windows = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot"));
    let dir = std::path::PathBuf::from(windows.unwrap_or_else(|| r"C:\Windows".into())).join("Fonts");
    let mut fonts = egui::FontDefinitions::default();
    let mut add = |name: &str, file: &str, primary: bool| {
        let Ok(bytes) = std::fs::read(dir.join(file)) else { return };
        fonts.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            let list = fonts.families.entry(fam.clone()).or_default();
            if primary && fam == egui::FontFamily::Proportional {
                list.insert(0, name.into());
            } else {
                list.push(name.into());
            }
        }
    };
    add("segoe-ui", "segoeui.ttf", true);
    add("segoe-ui-symbol", "seguisym.ttf", false);
    ctx.set_fonts(fonts);
}

pub fn setup_style(ctx: &egui::Context) {
    setup_fonts(ctx);
    ctx.set_theme(egui::Theme::Light);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
        s.visuals.panel_fill = Color32::from_rgb(246, 247, 249);
        s.visuals.selection.bg_fill = Color32::from_rgb(52, 110, 220);
        s.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    });
}

/// The tool bars (see `toolbars.rs`).
pub fn toolbar(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    crate::toolbars::draw(app, ui);
}

pub fn status_bar(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let now = ui.input(|i| i.time);
    egui::Panel::bottom("status").exact_size(30.0).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            if let Some((_, t)) = &mut app.message
                && *t == 0.0
            {
                *t = now;
            }
            let msg = app.message.as_ref().filter(|(_, t)| now - *t < 6.0).map(|(m, _)| m.clone());
            // SketchUp's messages list every modifier: cut them short before the
            // Measurements box (the whole text shows on hover).
            let width = (ui.available_width() - 320.0).max(60.0);
            let layout = egui::Layout::left_to_right(egui::Align::Center);
            ui.allocate_ui_with_layout(egui::vec2(width, ui.available_height()), layout, |ui| {
                let text = match msg {
                    Some(m) => RichText::new(m).color(Color32::from_rgb(170, 60, 0)),
                    None => RichText::new(app.tool.status()),
                };
                ui.add(egui::Label::new(text).truncate());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let typing = !app.vcb.is_empty();
                let value = if typing { app.vcb.clone() } else { app.tool.vcb_value(app.doc.model.units) };
                let frame = egui::Frame::new()
                    .fill(if typing { Color32::from_rgb(255, 252, 225) } else { Color32::WHITE })
                    .stroke(Stroke::new(1.0, Color32::from_gray(170)))
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .corner_radius(3.0);
                frame.show(ui, |ui| {
                    ui.set_min_width(170.0);
                    ui.label(RichText::new(value).monospace());
                });
                ui.label(RichText::new(app.tool.vcb_label_text()).strong());
            });
        });
    });
}

/// Window › Default Tray, and the trays made with Window › New Tray (`dialogs/trays.rs`).
pub fn tray(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    crate::dialogs::trays::show(app, ui);
}

/// The panels of a tray, one below the other, each folding under its title.
pub fn tray_panels(app: &mut EsquisseApp, ui: &mut egui::Ui, id: &str, panels: &[Panel]) {
    egui::ScrollArea::vertical().id_salt(id).show(ui, |ui| {
        for &p in panels {
            {
                // `ESQ_OPEN_PANELS="Composants,Styles"`: panels open from the start, for screen
                // captures (egui's panel headers do not answer UI Automation).
                let open = matches!(p, Panel::EntityInfo | Panel::Materials)
                    || std::env::var("ESQ_OPEN_PANELS").is_ok_and(|v| v.split(',').any(|n| n.trim() == p.label()));
                let unfold = (app.ui_state.unfold == Some(p)).then_some(true);
                if unfold.is_some() {
                    app.ui_state.unfold = None;
                }
                egui::CollapsingHeader::new(RichText::new(p.label()).strong()).default_open(open).open(unfold).show(
                    ui,
                    |ui| match p {
                        Panel::EntityInfo => crate::entity_info::show(app, ui),
                        Panel::Materials => materials(app, ui),
                        Panel::Components => components(app, ui),
                        Panel::Styles => crate::styles_panel::show(app, ui),
                        Panel::Tags => crate::tags_panel::show(app, ui),
                        Panel::Shadows => app.shadows_panel(ui),
                        Panel::Scenes => app.scenes_panel(ui),
                        Panel::MatchPhoto => app.photo_panel(ui),
                        Panel::Soften => crate::soften_panel::show(app, ui),
                        Panel::Outliner => crate::outliner_panel::show(app, ui),
                        Panel::Fog => app.fog_panel(ui),
                        Panel::HatchPatterns => crate::hatch_panel::show(app, ui),
                        Panel::Environments => crate::env_panel::show(app, ui),
                        Panel::Overlays => crate::overlays_panel::show(app, ui),
                        Panel::ModelGrid => crate::model_grid::show(app, ui),
                        Panel::Instructor => crate::dialogs::instructor::show(app, ui),
                    },
                );
            }
        }
    });
}

/// Window › Materials (`mat_panel.rs`).
fn materials(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    crate::mat_panel::show(app, ui);
}

/// Window › Components (`comp_library.rs`).
fn components(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    crate::comp_library::show(app, ui);
}

pub(crate) fn centered(title: &str) -> egui::Window<'_> {
    egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
}

/// One template, as on SketchUp's welcome screen: a tile with its name and units.
fn template_tile(ui: &mut egui::Ui, t: &Template, favourite: bool) -> (bool, bool) {
    let mut open = false;
    let mut fav = false;
    ui.vertical(|ui| {
        ui.set_width(120.0);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(120.0, 80.0), Sense::click());
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{} — {}", t.title(), t.subtitle()))
        });
        let p = ui.painter();
        p.rect_filled(
            rect,
            4.0,
            if resp.hovered() { Color32::from_rgb(214, 226, 248) } else { Color32::from_rgb(228, 232, 238) },
        );
        let o = rect.left_bottom() + egui::vec2(30.0, -18.0);
        p.line_segment([o, o + egui::vec2(80.0, -30.0)], Stroke::new(1.5, Color32::from_rgb(220, 30, 30)));
        p.line_segment([o, o + egui::vec2(-25.0, -12.0)], Stroke::new(1.5, Color32::from_rgb(20, 150, 40)));
        p.line_segment([o, o + egui::vec2(0.0, -55.0)], Stroke::new(1.5, Color32::from_rgb(30, 50, 230)));
        open = resp.on_hover_text("Créer un modèle à partir de ce modèle type").clicked();
        ui.horizontal(|ui| {
            if ui
                .selectable_label(favourite, if favourite { "♥" } else { "♡" })
                .on_hover_text("Modèle type par défaut")
                .clicked()
            {
                fav = true;
            }
            ui.label(RichText::new(t.title()).strong());
        });
        ui.label(RichText::new(t.subtitle()).small());
    });
    (open, fav)
}

fn template_grid(app: &mut EsquisseApp, ui: &mut egui::Ui) -> bool {
    let all = prefs::all_templates();
    let fav = prefs::default_template();
    let mut chosen = None;
    // Five tiles a row, the rows scrolled: the templates that come with Esquisse are many.
    egui::ScrollArea::vertical().max_height(330.0).id_salt("welcome_templates").show(ui, |ui| {
        for row in all.chunks(5) {
            ui.horizontal(|ui| {
                for t in row {
                    let (open, make_fav) = template_tile(ui, t, *t == fav);
                    if make_fav {
                        prefs::set_default_template(t);
                    }
                    if open {
                        chosen = Some(t.clone());
                    }
                }
            });
        }
    });
    if let Some(t) = chosen {
        app.new_from_template(&t);
        return true;
    }
    false
}

pub fn dialogs(app: &mut EsquisseApp, ctx: &egui::Context) {
    crate::extra::show(app, ctx);
    if app.confirm_close {
        centered("Modifications non enregistrées").show(ctx, |ui| {
            ui.label("Enregistrer les modifications avant de quitter ?");
            ui.horizontal(|ui| {
                if ui.button("Enregistrer").clicked() {
                    app.save(ctx, false);
                    if !app.doc.dirty {
                        app.force_close(ctx);
                    }
                }
                if ui.button("Ne pas enregistrer").clicked() {
                    app.force_close(ctx);
                }
                if ui.button("Annuler").clicked() {
                    app.confirm_close = false;
                }
            });
        });
    }
    if app.ui_state.ask_new {
        if !app.doc.dirty {
            app.ui_state.ask_new = false;
            app.new_document();
        } else {
            centered("Nouveau modèle").show(ctx, |ui| {
                ui.label("Le modèle actuel a des modifications non enregistrées.");
                ui.horizontal(|ui| {
                    if ui.button("Abandonner et créer").clicked() {
                        app.ui_state.ask_new = false;
                        app.new_document();
                    }
                    if ui.button("Annuler").clicked() {
                        app.ui_state.ask_new = false;
                    }
                });
            });
        }
    }
    if app.ui_state.welcome {
        let mut open = true;
        egui::Window::new("Bienvenue dans Esquisse")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(700.0);
                if crate::logo::brand(ui).is_some() {
                    ui.add_space(10.0);
                }
                ui.heading("Créer un nouveau modèle");
                if template_grid(app, ui) {
                    app.ui_state.welcome = false;
                }
                ui.add_space(8.0);
                if ui.button("Ouvrir le fichier…").clicked() {
                    app.ui_state.welcome = false;
                    app.open_dialog();
                }
                ui.add_space(8.0);
                ui.heading("Fichiers récents");
                let files = app.recent_files.clone();
                if files.is_empty() {
                    ui.label("Aucun fichier récent.");
                }
                for f in files {
                    let name = f.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    if ui.link(name).on_hover_text(f.display().to_string()).clicked() {
                        app.ui_state.welcome = false;
                        app.open_path(&f);
                    }
                }
            });
        if !open {
            app.ui_state.welcome = false;
        }
    }
    if app.ui_state.templates {
        let mut open = true;
        egui::Window::new("Nouveau à partir du modèle type")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(700.0);
                if template_grid(app, ui) {
                    app.ui_state.templates = false;
                }
            });
        if !open {
            app.ui_state.templates = false;
        }
    }
    if let Some(name) = &mut app.ui_state.save_template {
        let mut done = None;
        centered("Enregistrer comme modèle type").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Nom");
                ui.text_edit_singleline(name);
            });
            ui.label("Le modèle actuel (géométrie, unités, matières, étiquettes) servira de point de départ.");
            ui.horizontal(|ui| {
                if ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Enregistrer")).clicked() {
                    done = Some(true);
                }
                if ui.button("Annuler").clicked() {
                    done = Some(false);
                }
            });
        });
        if let Some(ok) = done {
            let name = app.ui_state.save_template.take().unwrap_or_default();
            if ok {
                app.style_to_model();
                let msg = match prefs::save_template(&app.doc.model, &name) {
                    Ok(_) => format!("Modèle type « {name} » enregistré"),
                    Err(e) => format!("Échec : {e:#}"),
                };
                app.say(ctx, msg);
            }
        }
    }
    if let Some((id, text)) = &mut app.ui_state.edit_text {
        let mut done = None;
        centered("Texte").show(ctx, |ui| {
            ui.label("Pour une cotation, laissez vide pour afficher la longueur mesurée.");
            let r = ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(320.0));
            r.request_focus();
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    done = Some(true);
                }
                if ui.button("Annuler").clicked() {
                    done = Some(false);
                }
            });
        });
        let id = *id;
        if let Some(ok) = done {
            let (_, text) = app.ui_state.edit_text.take().unwrap();
            if ok {
                app.doc.commit("Modifier le texte", |m, d| {
                    if let Some(a) = m.entities_mut(d).annots.get_mut(id) {
                        a.text = text;
                    }
                });
            }
        }
    }
    if app.ui_state.text3d.is_some() {
        crate::text3d::dialog(app, ctx);
    }
    if app.ui_state.search.is_some() {
        search(app, ctx);
    }
    if app.ui_state.show_about {
        egui::Window::new("À propos d'Esquisse")
            .collapsible(false)
            .resizable(false)
            .open(&mut app.ui_state.show_about)
            .show(ctx, |ui| {
                ui.heading("Esquisse");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("Modeleur 3D libre, écrit en Rust.");
                ui.label(
                    "Les fichiers .skp (SketchUp 3 à 2026) sont lus et écrits par Esquisse lui-même, sans SketchUp.",
                );
            });
    }
}

/// Help › Search: type part of a command name, Enter runs the first match.
fn search(app: &mut EsquisseApp, ctx: &egui::Context) {
    let mut run = None;
    let mut close = false;
    egui::Window::new("Rechercher dans Esquisse")
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 90.0])
        .show(ctx, |ui| {
            ui.set_width(460.0);
            let text = app.ui_state.search.as_mut().unwrap();
            let r = ui.add(egui::TextEdit::singleline(text).hint_text("Rechercher une commande…").desired_width(440.0));
            r.request_focus();
            let q = text.to_lowercase();
            let hits: Vec<(String, Cmd)> = crate::menus::command_paths()
                .into_iter()
                .filter(|(p, _)| !q.is_empty() && p.to_lowercase().contains(&q))
                .take(12)
                .collect();
            for (p, c) in &hits {
                let leaf = p.rsplit(" › ").next().unwrap_or(p).to_string();
                let resp = ui.add_enabled(
                    app.cmd_enabled(*c),
                    egui::Button::new(leaf).shortcut_text(c.shortcut_text()).frame(false),
                );
                if resp.on_hover_text(p).clicked() {
                    run = Some(*c);
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                run = hits.iter().find(|(_, c)| app.cmd_enabled(*c)).map(|(_, c)| *c);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
    if close || run.is_some() {
        app.ui_state.search = None;
    }
    if let Some(c) = run {
        app.run(ctx, c);
    }
}
