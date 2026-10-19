//! Window › Environments (SketchUp's `EnvironmentsDlg`): the current environment, the
//! environments of the model and of the libraries (Esquisse's, the user's), and the settings
//! of the current one: skydome and its exposure, reflections and their exposure, rotation,
//! the sun linked to the panorama.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, Color32, RichText, Sense, Stroke};

use crate::app::EsquisseApp;
use crate::environments::{self as envs, Env, LibEnv, Source, Thumb};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Select,
    Edit,
}

#[derive(Clone, Debug, PartialEq, Default)]
enum Collection {
    #[default]
    InModel,
    Folder(PathBuf),
}

#[derive(Clone, Default)]
struct PanelState {
    tab: Tab,
    collection: Collection,
    /// Libraries read once: folder → its environments.
    folders: HashMap<PathBuf, Vec<LibEnv>>,
    message: Option<String>,
}

/// The libraries: the environments that come with Esquisse, the folder chosen in
/// Preferences › Files › Environments, then the user's.
fn libraries() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    if let Some(d) = envs::shipped_dir() {
        out.push(("Environnements".to_string(), d));
    }
    if let Some(d) = crate::settings::with(|s| s.location("environments"))
        && !out.iter().any(|(_, p)| *p == d)
    {
        let name = d.file_name().map_or_else(|| d.display().to_string(), |n| n.to_string_lossy().into_owned());
        out.push((name, d));
    }
    if let Some(u) = envs::user_dir() {
        out.push(("Mes environnements".to_string(), u));
    }
    out
}

/// An egui texture for a thumbnail (made once per key).
fn texture(ctx: &egui::Context, key: &str, t: &Thumb) -> egui::TextureId {
    type Cache = Arc<std::sync::Mutex<HashMap<String, egui::TextureHandle>>>;
    let cache: Cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(egui::Id::unique("env_thumbnails")).clone());
    let mut c = cache.lock().unwrap();
    c.entry(key.to_string())
        .or_insert_with(|| {
            crate::gpu_img::load_texture(ctx, key, [t.0 as usize, t.1 as usize], &t.2, egui::TextureOptions::LINEAR)
        })
        .id()
}

/// A tile: the environment's picture (2:1), its name below.
fn tile(ui: &mut egui::Ui, key: &str, thumb: Option<(Arc<Thumb>, bool)>, name: &str, selected: bool) -> egui::Response {
    let w = 96.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, w / 2.0 + 31.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, name));
    let pic = egui::Rect::from_min_size(rect.min, egui::vec2(w, w / 2.0));
    let p = ui.painter();
    match &thumb {
        Some((t, _)) => {
            // (Keyed by the picture too: the placeholder shown while it loads is replaced.)
            let id = texture(ui.ctx(), &format!("{key}:{:p}", Arc::as_ptr(t)), t);
            p.image(id, pic, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            p.rect_filled(pic, 0.0, Color32::from_gray(225));
        }
    }
    if thumb.as_ref().is_some_and(|(_, loading)| *loading) {
        p.text(pic.center(), egui::Align2::CENTER_CENTER, "…", egui::FontId::proportional(16.0), Color32::GRAY);
    }
    let stroke = if selected {
        Stroke::new(2.5, Color32::from_rgb(52, 110, 220))
    } else {
        Stroke::new(1.0, Color32::from_gray(160))
    };
    p.rect_stroke(pic, 2.0, stroke, egui::StrokeKind::Inside);
    let label = egui::Rect::from_min_max(egui::pos2(rect.min.x, pic.max.y), rect.max);
    let galley = p.layout(name.to_string(), egui::FontId::proportional(11.0), Color32::from_gray(40), w);
    p.with_clip_rect(label).galley(label.min + egui::vec2(2.0, 2.0), galley, Color32::BLACK);
    resp.on_hover_text(name)
}

/// The thumbnail of a source, with whether it is still being made (and a repaint then).
fn thumb_of(ui: &egui::Ui, src: Option<Source>) -> Option<(Arc<Thumb>, bool)> {
    let (t, pending) = envs::thumbnail(&src?);
    if pending {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
    }
    static GREY: std::sync::OnceLock<Arc<Thumb>> = std::sync::OnceLock::new();
    let grey = || GREY.get_or_init(|| Arc::new((1, 1, vec![225, 225, 225, 255]))).clone();
    Some((t.unwrap_or_else(grey), pending))
}

/// The panel as it first opens; `ESQ_ENV_TAB` (`select`, `edit`, and `,library index`)
/// opens it elsewhere, for screen captures.
fn initial_state() -> PanelState {
    let mut st = PanelState::default();
    if let Ok(v) = std::env::var("ESQ_ENV_TAB") {
        let (tab, lib) = v.split_once(',').unwrap_or((v.as_str(), ""));
        st.tab = if tab == "edit" { Tab::Edit } else { Tab::Select };
        if let Some((_, p)) = lib.parse::<usize>().ok().and_then(|i| libraries().get(i).cloned()) {
            st.collection = Collection::Folder(p);
        }
    }
    st
}

impl EsquisseApp {
    /// Makes a library environment current: added to the model first if it is not there
    /// (by its name). Undoable, as SketchUp's "Change Active Environment".
    pub fn use_environment_file(&mut self, path: &std::path::Path) -> Result<i64, String> {
        let (env, file) = envs::from_file(path)?;
        let existing = envs::list(&self.doc.model).into_iter().find(|e| e.name == env.name).map(|e| e.id);
        let mut id = 0;
        self.doc.commit("Modifier l'environnement actif", |m, _| {
            id = existing.unwrap_or_else(|| envs::add(m, env, Some(file)));
            envs::set_current(m, Some(id));
        });
        self.doc.rev += 1;
        Ok(id)
    }

    /// Makes an environment of the model current (or none).
    pub fn set_current_environment(&mut self, id: Option<i64>) {
        if envs::current_id(&self.doc.model) == id {
            return;
        }
        self.doc.commit("Modifier l'environnement actif", |m, _| envs::set_current(m, id));
        self.doc.rev += 1;
    }

    /// Changes the settings of an environment at once (sliders, check boxes).
    pub fn set_environment(&mut self, e: &Env) {
        envs::update(&mut self.doc.model, e);
        self.doc.rev += 1;
    }
}

/// Window › Environments.
pub fn show(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let id = egui::Id::unique("env_panel");
    let mut st: PanelState = ui.data_mut(|d| d.get_temp(id)).unwrap_or_else(initial_state);
    let current = envs::current(&app.doc.model);

    // The current environment: its picture, name and description.
    ui.horizontal(|ui| {
        let src = current.as_ref().and_then(|e| envs::source(&app.doc.model, e));
        let name = current.as_ref().map_or("Pas d'environnement".to_string(), |e| e.name.clone());
        tile(ui, &format!("cur:{}", current.as_ref().map_or(0, |e| e.id)), thumb_of(ui, src), "", false);
        ui.vertical(|ui| match current.clone() {
            Some(mut e) => {
                let mut n = e.name.clone();
                let others: Vec<String> =
                    envs::list(&app.doc.model).into_iter().filter(|o| o.id != e.id).map(|o| o.name).collect();
                if ui.add(egui::TextEdit::singleline(&mut n).desired_width(f32::INFINITY)).changed() {
                    if n.trim().is_empty() || others.contains(&n) {
                        st.message =
                            Some("Nom non valide.  Les noms d'environnement doivent être uniques et non vides.".into());
                    } else {
                        e.name = n;
                        st.message = None;
                        app.set_environment(&e);
                    }
                }
                let mut d = e.description.clone();
                if ui.add(egui::TextEdit::multiline(&mut d).desired_rows(2).desired_width(f32::INFINITY)).changed() {
                    e.description = d;
                    app.set_environment(&e);
                }
            }
            None => {
                ui.label(name);
            }
        });
    });
    ui.separator();
    ui.horizontal(|ui| {
        ui.selectable_value(&mut st.tab, Tab::Select, "Sélectionner");
        ui.add_enabled_ui(current.is_some(), |ui| ui.selectable_value(&mut st.tab, Tab::Edit, "Modifier"));
    });
    ui.separator();
    match (st.tab, current) {
        (Tab::Edit, Some(e)) => edit_tab(app, ui, e),
        _ => select_tab(app, ui, &mut st),
    }
    if let Some(m) = &st.message {
        ui.label(RichText::new(m).small().color(Color32::from_rgb(170, 60, 0)));
    }
    ui.data_mut(|d| d.insert_temp(id, st));
}

fn select_tab(app: &mut EsquisseApp, ui: &mut egui::Ui, st: &mut PanelState) {
    let libs = libraries();
    ui.horizontal(|ui| {
        let label = match &st.collection {
            Collection::InModel => "Dans le modèle".to_string(),
            Collection::Folder(p) => {
                libs.iter().find(|(_, q)| q == p).map_or_else(|| p.display().to_string(), |l| l.0.clone())
            }
        };
        egui::ComboBox::from_id_salt("env_collection").selected_text(label).show_ui(ui, |ui| {
            ui.selectable_value(&mut st.collection, Collection::InModel, "Dans le modèle");
            for (name, p) in &libs {
                ui.selectable_value(&mut st.collection, Collection::Folder(p.clone()), name);
            }
        });
        if ui
            .button("Ajouter un environnement…")
            .on_hover_text("Environnement SketchUp (*.ske), image HDR (*.hdr, *.exr)")
            .clicked()
            && let Some(p) = rfd::FileDialog::new()
                .set_title("Ajouter un environnement")
                .add_filter("Tous les types de fichiers HDR pris en charge", &["ske", "hdr", "exr"])
                .pick_file()
        {
            st.message = app.use_environment_file(&p).err();
        }
    });
    ui.add_space(4.0);
    let current = envs::current_id(&app.doc.model);
    let mut pick: Option<Option<i64>> = None;
    let mut pick_file: Option<PathBuf> = None;
    let mut delete: Option<i64> = None;
    match st.collection.clone() {
        Collection::InModel => {
            let list = envs::list(&app.doc.model);
            ui.horizontal_wrapped(|ui| {
                if tile(ui, "none", None, "Pas d'environnement", current.is_none()).clicked() {
                    pick = Some(None);
                }
                for e in &list {
                    let src = envs::source(&app.doc.model, e);
                    let r = tile(
                        ui,
                        &format!("env:{}:{}", e.id, e.path),
                        thumb_of(ui, src),
                        &e.name,
                        current == Some(e.id),
                    );
                    if r.clicked() {
                        pick = Some(Some(e.id));
                    }
                    r.context_menu(|ui| {
                        if ui.button("Supprimer l'environnement").clicked() {
                            delete = Some(e.id);
                            ui.close();
                        }
                    });
                }
            });
            ui.add_space(4.0);
            if ui.button("Purger les environnements inutilisés").clicked() {
                let mut n = 0;
                app.doc.commit("Purger les environnements", |m, _| n = envs::purge_unused(m));
                app.doc.rev += 1;
                st.message = Some(format!("{n} environnement(s) supprimé(s)"));
            }
        }
        Collection::Folder(dir) => {
            let items = st.folders.entry(dir.clone()).or_insert_with(|| envs::read_folder(&dir)).clone();
            if items.is_empty() {
                ui.label(RichText::new("Aucun environnement dans ce dossier.").italics());
            }
            let names: Vec<String> = envs::list(&app.doc.model).into_iter().map(|e| e.name).collect();
            let cur_name = envs::current(&app.doc.model).map(|e| e.name);
            ui.horizontal_wrapped(|ui| {
                for it in &items {
                    let src = Some(Source::Library(it.path.to_string_lossy().into_owned()));
                    let selected = cur_name.as_deref() == Some(it.name.as_str());
                    let r = tile(ui, &format!("lib:{}", it.path.display()), thumb_of(ui, src), &it.name, selected);
                    if r.clicked() {
                        pick_file = Some(it.path.clone());
                    }
                    r.context_menu(|ui| {
                        if ui.add_enabled(!names.contains(&it.name), egui::Button::new("Ajouter au modèle")).clicked()
                        {
                            let path = it.path.clone();
                            match envs::from_file(&path) {
                                Ok((env, file)) => {
                                    app.doc.commit("Ajouter un environnement", |m, _| {
                                        envs::add(m, env, Some(file));
                                    });
                                }
                                Err(e) => st.message = Some(e),
                            }
                            ui.close();
                        }
                    });
                }
            });
        }
    }
    if let Some(id) = pick {
        app.set_current_environment(id);
    }
    if let Some(p) = pick_file {
        st.message = app.use_environment_file(&p).err();
    }
    if let Some(id) = delete {
        app.doc.commit("Supprimer l'environnement", |m, _| envs::remove(m, id));
        app.doc.rev += 1;
    }
}

fn edit_tab(app: &mut EsquisseApp, ui: &mut egui::Ui, mut e: Env) {
    let before = e.clone();
    ui.label(RichText::new("Paramètres de l'environnement").strong());
    ui.checkbox(&mut e.skydome, "Utiliser l'environnement comme Skydome");
    ui.add_enabled_ui(e.skydome, |ui| {
        ui.horizontal(|ui| {
            ui.label("Skydome - Exposition");
            ui.add(egui::Slider::new(&mut e.skydome_exposure, 0.0..=20.0).step_by(0.01).max_decimals(2));
        });
    });
    ui.checkbox(&mut e.reflections, "Utiliser l'environnement pour la réflexion");
    ui.add_enabled_ui(e.reflections, |ui| {
        ui.horizontal(|ui| {
            ui.label("Réflexions - Exposition");
            ui.add(egui::Slider::new(&mut e.reflection_exposure, 0.0..=10.0).step_by(0.01).max_decimals(2));
        });
    });
    ui.horizontal(|ui| {
        ui.label("Angle de rotation");
        ui.add(egui::Slider::new(&mut e.rotation, 0.0..=360.0).step_by(1.0).suffix("°"));
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.checkbox(&mut e.linked_sun, "Associé").on_hover_text("Le soleil (ombres) suit l'environnement");
        if ui.button("Emplacement du soleil couchant…").clicked() {
            app.ui_state.extra.ops.sun = true;
        }
    });
    if !e.reflections || !app.style.photoreal {
        ui.label(
            RichText::new("Les reflets de l'environnement s'affichent avec les matériaux photoréalistes.")
                .small()
                .color(Color32::from_gray(110)),
        );
    }
    if e != before {
        app.set_environment(&e);
    }
}
