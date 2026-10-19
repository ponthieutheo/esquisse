//! Window › Components, as SketchUp's panel (`CompContentBrowserPage`, `CCompDefEditPage`,
//! `CCompInstStatsPage`): Select browses the components of the model ("Dans le modèle") and
//! the collections of component files — the library that comes with Esquisse (`Components` of
//! its content, one collection per folder), the folder of Preferences › Files › Components and
//! the local collections opened from the panel's menu —; Edit is `dialogs::comp_edit`;
//! Statistics counts what the component is made of.
//!
//! A click on a component of a collection loads its file into the model, once (a component
//! already loaded from that file is taken again, as SketchUp does), and places it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, RichText, Sense, Stroke};
use esq_core::model::{DefId, Model};

use crate::app::EsquisseApp;

/// A component file of a collection.
#[derive(Clone, Debug)]
pub struct LibItem {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    /// PNG: the `.png` beside an `.esq`, the thumbnail inside a `.skp` (2021 and later).
    pub thumbnail: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Collection {
    InModel,
    Folder(PathBuf),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Tab {
    #[default]
    Select,
    Edit,
    Stats,
}

#[derive(Clone, Default)]
struct PanelState {
    tab: Tab,
    collection: Option<Collection>,
    /// Local collections opened in this session (menu › Open or Create a Local Collection).
    opened: Vec<PathBuf>,
    /// The items of `collection`, read once.
    loaded: Option<(Collection, Vec<LibItem>)>,
    /// What the header shows: the component last clicked.
    chosen: Option<(String, String)>,
    /// Statistics › Expand nested components.
    expand: bool,
    message: Option<String>,
    /// The collections, listed once (again after Refresh or Open).
    list: Option<Vec<(String, Collection)>>,
}

/// What the collections listed: their component files (the only ones the panel loads) and
/// their thumbnails, by path. A component of the model shows the thumbnail of the file it was
/// loaded from only through this list: the panel never reads a path written in a model
/// (`safe_path`), nor reads anything while it draws.
#[derive(Default)]
struct Listed {
    files: std::collections::HashSet<String>,
    thumbnails: HashMap<String, std::sync::Arc<Vec<u8>>>,
}

fn listed() -> std::sync::MutexGuard<'static, Listed> {
    static L: std::sync::OnceLock<std::sync::Mutex<Listed>> = std::sync::OnceLock::new();
    L.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
thread_local! {
    static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Tests: how many files the panel read on this thread so far.
#[cfg(test)]
pub fn disk_reads() -> usize {
    READS.with(|r| r.get())
}

/// Every file the panel reads (those of the folders the collections list) goes through here.
fn read_file(p: &Path) -> Option<Vec<u8>> {
    #[cfg(test)]
    READS.with(|r| r.set(r.get() + 1));
    std::fs::read(p).ok()
}

/// The thumbnail of a component file a collection listed (memory only).
fn listed_thumbnail(path: &str) -> Option<std::sync::Arc<Vec<u8>>> {
    listed().thumbnails.get(path).cloned()
}

/// The library that comes with Esquisse.
pub fn library() -> Option<PathBuf> {
    esq_core::content::folder("Components")
}

fn folder_collections(lib: &Path, prefix: &str, out: &mut Vec<(String, Collection)>) {
    let name_of = |d: &Path| {
        let n = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        format!("{prefix}{}", crate::i18n::fr_or_same(&n))
    };
    let dirs = esq_core::content::subfolders(lib);
    let has_files = std::fs::read_dir(lib).is_ok_and(|rd| rd.flatten().any(|e| is_component_file(&e.path())));
    if dirs.is_empty() || has_files {
        out.push((name_of(lib), Collection::Folder(lib.into())));
    }
    for d in dirs {
        out.push((name_of(&d), Collection::Folder(d)));
    }
}

/// The collections of the Select tab, with their names.
pub fn collections(opened: &[PathBuf]) -> Vec<(String, Collection)> {
    let mut out = vec![("Dans le modèle".to_string(), Collection::InModel)];
    if let Some(lib) = library() {
        for d in esq_core::content::subfolders(&lib) {
            let n = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            out.push((crate::i18n::fr_or_same(&n), Collection::Folder(d)));
        }
    }
    // Preferences › Files › Components: the user's library (SketchUp's, a colleague's…).
    if let Some(lib) = crate::settings::with(|s| s.location("components"))
        && Some(&lib) != library().as_ref()
    {
        folder_collections(&lib, "Préférences › ", &mut out);
    }
    for d in opened {
        if !out.iter().any(|(_, c)| *c == Collection::Folder(d.clone())) {
            let n = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            out.push((n, Collection::Folder(d.clone())));
        }
    }
    out
}

fn is_component_file(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("esq") || e.eq_ignore_ascii_case("skp"))
}

/// The thumbnail a .skp of SketchUp 2021 or later holds (`meta/model_thumbnail.png`).
fn skp_thumbnail(path: &Path) -> Option<Vec<u8>> {
    let data = read_file(path)?;
    let start = data.windows(4).position(|w| w == b"PK\x03\x04")?;
    let files = crate::zip::read_only(&data[start..], |n| n == "meta/model_thumbnail.png").ok()?;
    files.into_values().next()
}

/// The component files of a folder (`.esq` and `.skp`), sorted by name.
pub fn read_folder(dir: &Path) -> Vec<LibItem> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && is_component_file(p)).collect())
        .unwrap_or_default();
    files.sort();
    let mut out: Vec<LibItem> = files
        .into_iter()
        .map(|path| {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let esq = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("esq"));
            let (name, description, thumbnail) = if esq {
                let m = read_file(&path).and_then(|b| serde_json::from_slice::<Model>(&b).ok());
                let name = m.as_ref().map(|m| m.meta.name.clone()).filter(|n| !n.is_empty()).unwrap_or(stem);
                let desc = m.map(|m| m.meta.description).unwrap_or_default();
                (name, desc, read_file(&path.with_extension("png")))
            } else {
                (stem, String::new(), skp_thumbnail(&path))
            };
            LibItem { name, description, path, thumbnail }
        })
        .collect();
    out.sort_by_key(|i| i.name.to_lowercase());
    let mut l = listed();
    for it in &out {
        let key = it.path.to_string_lossy().into_owned();
        if let Some(t) = &it.thumbnail {
            l.thumbnails.insert(key.clone(), std::sync::Arc::new(t.clone()));
        }
        l.files.insert(key);
    }
    out
}

/// Reads a component file.
pub fn load(path: &Path) -> anyhow::Result<Model> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("skp")) {
        esq_skp::import_skp(path)
    } else {
        esq_core::io::load_esq(path)
    }
}

/// The definition of a component file a collection listed: the one already loaded from it
/// (a definition whose source is that same listed file: a string compared, nothing read), else
/// the file read now (one undoable step).
pub fn load_into(app: &mut EsquisseApp, path: &Path) -> Result<DefId, String> {
    let key = path.to_string_lossy().into_owned();
    if !listed().files.contains(&key) {
        return Err(format!("{key} : fichier hors des collections"));
    }
    if let Some((d, _)) = app.doc.model.defs.iter().find(|(_, d)| d.ext.path == key) {
        return Ok(d);
    }
    let src = load(path).map_err(|e| format!("{e:#}"))?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Composant".into());
    let name = if src.meta.name.is_empty() { stem } else { src.meta.name.clone() };
    let mut made = None;
    app.doc.commit("Charger le composant", |m, _| {
        let d = m.import_as_component(&src, &name);
        let def = &mut m.defs[d];
        def.ext.path = key.clone();
        def.ext.loaded_at =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |t| t.as_secs_f64());
        made = Some(d);
    });
    made.ok_or_else(|| "composant illisible".to_string())
}

fn texture_for(ctx: &egui::Context, key: &str, png: &[u8]) -> Option<egui::TextureId> {
    type Cache = std::sync::Arc<std::sync::Mutex<HashMap<String, Option<egui::TextureHandle>>>>;
    let cache: Cache =
        ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(egui::Id::unique("component_thumbnails")).clone());
    let mut c = cache.lock().ok()?;
    if let Some(h) = c.get(key) {
        return h.as_ref().map(|h| h.id());
    }
    let h = crate::photo_img::decode(png).ok().map(|img| {
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        crate::gpu_img::load_texture(ctx, key, size, rgba.as_raw(), egui::TextureOptions::LINEAR)
    });
    let id = h.as_ref().map(|h| h.id());
    c.insert(key.to_string(), h);
    id
}

/// A component's tile: its thumbnail (or its initial), its name below.
fn tile(ui: &mut egui::Ui, key: &str, name: &str, desc: &str, png: Option<&[u8]>) -> egui::Response {
    let (cell, resp) = ui.allocate_exact_size(egui::vec2(64.0, 80.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    let rect = egui::Rect::from_min_size(cell.min, egui::vec2(64.0, 64.0));
    let p = ui.painter();
    p.rect_filled(rect, 2.0, Color32::WHITE);
    match png.and_then(|t| texture_for(ui.ctx(), key, t)) {
        Some(id) => {
            p.image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            p.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                initial,
                egui::FontId::proportional(26.0),
                Color32::from_gray(120),
            );
        }
    }
    let stroke = if resp.hovered() {
        Stroke::new(2.0, Color32::from_rgb(52, 110, 220))
    } else {
        Stroke::new(1.0, Color32::from_gray(170))
    };
    p.rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
    // The name under the thumbnail, cut to the tile's width.
    let font = egui::FontId::proportional(10.0);
    let galley = ui.painter().layout_job(egui::text::LayoutJob {
        wrap: egui::text::TextWrapping::truncate_at_width(64.0),
        ..egui::text::LayoutJob::simple_singleline(name.to_string(), font, ui.visuals().text_color())
    });
    ui.painter().galley(egui::pos2(cell.min.x, rect.max.y + 2.0), galley, ui.visuals().text_color());
    resp.on_hover_text(if desc.is_empty() { name.to_string() } else { format!("{name}\n{desc}") })
}

/// Window › Components.
pub fn show(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let id = egui::Id::unique("components_panel");
    let mut st: PanelState = ui.data_mut(|d| d.get_temp(id)).unwrap_or_else(|| {
        // `ESQ_COMPONENTS_COLLECTION` (an index of the list): the collection first shown, for
        // screen captures (egui's lists do not answer UI Automation).
        let i = std::env::var("ESQ_COMPONENTS_COLLECTION").ok().and_then(|v| v.parse::<usize>().ok());
        PanelState { collection: i.and_then(|i| collections(&[]).get(i).map(|c| c.1.clone())), ..Default::default() }
    });
    // Header: the component chosen last, its name and description.
    let (name, desc) = st
        .chosen
        .clone()
        .or_else(|| {
            crate::dialogs::comp_edit::edited(app).map(|d| {
                let def = &app.doc.model.defs[d];
                (def.name.clone(), def.description.clone())
            })
        })
        .unwrap_or_default();
    ui.label(RichText::new(if name.is_empty() { "Composants" } else { name.as_str() }).strong());
    if !desc.is_empty() {
        ui.label(RichText::new(desc).small());
    }
    ui.horizontal(|ui| {
        for (t, label) in [(Tab::Select, "Sélectionner"), (Tab::Edit, "Modifier"), (Tab::Stats, "Statistiques")] {
            ui.selectable_value(&mut st.tab, t, label);
        }
    });
    ui.separator();
    match st.tab {
        Tab::Select => select_tab(app, ui, &mut st),
        Tab::Edit => crate::dialogs::comp_edit::edit_section(app, ui),
        Tab::Stats => stats_tab(app, ui, &mut st),
    }
    if let Some(m) = &st.message {
        ui.label(RichText::new(m).small().color(Color32::from_rgb(170, 60, 0)));
    }
    ui.data_mut(|d| d.insert_temp(id, st));
}

fn select_tab(app: &mut EsquisseApp, ui: &mut egui::Ui, st: &mut PanelState) {
    let all = st.list.get_or_insert_with(|| collections(&st.opened)).clone();
    let current = st.collection.clone().filter(|c| all.iter().any(|(_, x)| x == c)).unwrap_or(Collection::InModel);
    let choices: Vec<(usize, String)> = all.iter().enumerate().map(|(i, (n, _))| (i, n.clone())).collect();
    let mut k = all.iter().position(|(_, c)| *c == current).unwrap_or(0);
    ui.horizontal(|ui| {
        let home = ui.button("⌂").on_hover_text("Dans le modèle");
        crate::dialogs::name_widget(ui, &home, "Dans le modèle", None);
        if home.clicked() {
            k = 0;
        }
        if crate::dialogs::combo(ui, "Collection de composants", &mut k, &choices) || k == 0 {
            st.collection = Some(all[k].1.clone());
        }
        ui.menu_button("☰", |ui| {
            if ui.button("Ouvrir ou créer une collection locale...").clicked() {
                ui.close();
                if let Some(d) = crate::settings::dialog("components").pick_folder() {
                    st.collection = Some(Collection::Folder(d.clone()));
                    st.opened.push(d);
                    st.list = None;
                }
            }
            if ui.button("Purger les éléments inutilisés").clicked() {
                ui.close();
                let mut n = 0;
                app.doc.commit("Purger les composants", |m, _| {
                    let before = m.defs.len();
                    m.purge_unused_defs();
                    n = before - m.defs.len();
                });
                st.message = Some(format!("{n} composant(s) supprimé(s)"));
            }
            if ui.button("Actualiser").clicked() {
                ui.close();
                st.loaded = None;
                st.list = None;
            }
        });
    });
    let current = st.collection.clone().unwrap_or(Collection::InModel);
    let mut place = None;
    let mut load = None;
    egui::ScrollArea::vertical().max_height(300.0).id_salt("component_tiles").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| match &current {
            Collection::InModel => {
                let mut defs: Vec<(DefId, String, String)> = app
                    .doc
                    .model
                    .defs
                    .iter()
                    .filter(|(k, d)| !d.is_group && !app.doc.model.is_image_def(*k))
                    .map(|(k, d)| (k, d.name.clone(), d.description.clone()))
                    .collect();
                defs.sort_by_key(|d| d.1.to_lowercase());
                if defs.is_empty() {
                    ui.label("Aucun composant dans le modèle.");
                }
                for (d, name, desc) in defs {
                    // The thumbnail of the listed file it came from, if any: never a read of
                    // the path the model holds.
                    let src = app.doc.model.defs[d].ext.path.clone();
                    // Else its own picture, rendered offscreen and kept (`def_thumbs.rs`).
                    let (key, png) = match listed_thumbnail(&src) {
                        Some(p) => (src, Some(p)),
                        None => match crate::def_thumbs::thumbnail(app, d) {
                            Some((k, p)) => (k, Some(p)),
                            None => (format!("def{d:?}"), None),
                        },
                    };
                    if tile(ui, &key, &name, &desc, png.as_deref().map(Vec::as_slice)).clicked() {
                        place = Some((d, name, desc));
                    }
                }
            }
            Collection::Folder(dir) => {
                if st.loaded.as_ref().is_none_or(|(c, _)| *c != current) {
                    st.loaded = Some((current.clone(), read_folder(dir)));
                }
                let items: &[LibItem] = st.loaded.as_ref().map_or(&[], |(_, l)| l.as_slice());
                if items.is_empty() {
                    ui.label("Aucun composant dans cette collection.");
                }
                for it in items {
                    let key = it.path.display().to_string();
                    if tile(ui, &key, &it.name, &it.description, it.thumbnail.as_deref()).clicked() {
                        load = Some(it.clone());
                    }
                }
            }
        });
    });
    if let Some(it) = load {
        match load_into(app, &it.path) {
            Ok(d) => place = Some((d, it.name, it.description)),
            Err(e) => st.message = Some(format!("Composant illisible : {e}")),
        }
    }
    if let Some((d, name, desc)) = place {
        st.chosen = Some((name, desc));
        st.message = None;
        app.ui_state.extra.dialogs.comp_edit.def = Some(d);
        app.place_component(d);
    }
}

/// The rows of the Statistics tab for definition `d`: what it holds, and with `expand` what
/// the components and groups nested in it hold too (each nested definition once).
pub fn component_statistics(m: &Model, d: DefId, expand: bool) -> Vec<(&'static str, usize)> {
    // The model as the component alone: its geometry at the root, only its nested definitions.
    let mut nested = std::collections::HashSet::new();
    let mut todo = vec![d];
    while let Some(x) = todo.pop() {
        for i in m.defs[x].entities.instances.values() {
            if nested.insert(i.def) {
                todo.push(i.def);
            }
        }
    }
    let mut defs = m.defs.clone();
    defs.retain(|k, _| nested.contains(&k));
    let probe = Model { root: m.defs[d].entities.clone(), defs, layers: m.layers.clone(), ..Model::new() };
    crate::dialogs::mi_stats::statistics(&probe, false, expand).into_iter().take(11).collect()
}

/// Statistics: what the component's definition holds (nested components counted inside it
/// with Expand).
fn stats_tab(app: &mut EsquisseApp, ui: &mut egui::Ui, st: &mut PanelState) {
    let Some(d) = crate::dialogs::comp_edit::edited(app) else {
        ui.label("Sélectionnez un composant.");
        return;
    };
    let rows = component_statistics(&app.doc.model, d, st.expand);
    ui.checkbox(&mut st.expand, "Développer");
    egui::Grid::new("component_stats").num_columns(2).striped(true).show(ui, |ui| {
        for (label, n) in rows {
            ui.label(label);
            ui.label(n.to_string());
            ui.end_row();
        }
    });
}
