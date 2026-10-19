//! Window › Styles (SketchUp's `CStylesDlg`): the active style's name and description, with
//! Create and Update; Select (the styles of the model, Esquisse's styles, the library that
//! comes with Esquisse, the user's styles), Edit (edges, faces, background, watermark,
//! modeling) and Mix (take one part of the settings from another style).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, RichText, Sense, Stroke};
use serde_json::{Map, Value as J};

use crate::app::EsquisseApp;
use crate::style::{EdgeColor, FaceStyle, MODEL_KEYS, ViewStyle};
use crate::style_file::{StyleFile, category_of};
use crate::styles::{ModelStyle, model_styles, save_styles, unique_name};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Select,
    Edit,
    Mix,
}

/// Where the Select tab takes its styles from.
#[derive(Clone, Debug, PartialEq)]
pub enum Collection {
    InModel,
    Esquisse,
    Folder(PathBuf),
}

/// A style of a collection.
#[derive(Clone)]
pub struct LibStyle {
    pub file: StyleFile,
    pub path: Option<PathBuf>,
    /// The style of the model it is (In Model).
    pub id: Option<i64>,
}

#[derive(Clone, Default)]
struct PanelState {
    tab: Tab,
    edit: usize,
    collection: Option<Collection>,
    /// The styles of `collection`, read once.
    loaded: Option<(Collection, Vec<LibStyle>)>,
    message: Option<String>,
}

/// The five parts of a style (Edit's pages, Mix's settings).
pub const PARTS: [&str; 5] = ["Arêtes", "Faces", "Arrière-plan", "Filigrane", "Modélisation"];

/// The style library that comes with Esquisse (`Styles` of its content: one folder per
/// collection, named as SketchUp names its own).
pub fn library() -> Option<PathBuf> {
    esq_core::content::folder("Styles")
}

/// The collections of a library folder: its sub-folders, else the folder itself.
fn library_collections(lib: &Path, prefix: &str, out: &mut Vec<(String, Collection)>) {
    let dirs = esq_core::content::subfolders(lib);
    if dirs.is_empty() {
        let name = lib.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        out.push((format!("{prefix}{}", crate::style_file::collection_name(&name)), Collection::Folder(lib.into())));
    }
    for d in dirs {
        let name = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        out.push((format!("{prefix}{}", crate::style_file::collection_name(&name)), Collection::Folder(d)));
    }
}

/// Where the user's styles are saved (`%APPDATA%\Esquisse\Styles`).
pub fn user_styles_dir() -> Option<PathBuf> {
    let d = crate::settings::user_dir()?.join("Styles");
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

/// The collections offered by the Select tab, with their names.
pub fn collections() -> Vec<(String, Collection)> {
    let mut out = vec![
        ("Dans le modèle".to_string(), Collection::InModel),
        ("Styles Esquisse".to_string(), Collection::Esquisse),
    ];
    if let Some(lib) = library() {
        library_collections(&lib, "", &mut out);
    }
    // Preferences › Files › Styles: another library (SketchUp's, a colleague's…).
    if let Some(lib) = crate::settings::with(|s| s.location("styles"))
        && Some(&lib) != library().as_ref()
    {
        library_collections(&lib, "Préférences › ", &mut out);
    }
    if let Some(d) = user_styles_dir() {
        out.push(("Mes styles".to_string(), Collection::Folder(d)));
    }
    out
}

/// Esquisse's own styles (made here, not taken from SketchUp).
pub fn builtin_styles() -> Vec<LibStyle> {
    let simple = ViewStyle::default();
    let mk = |name: &str, desc: &str, f: &dyn Fn(&mut ViewStyle)| {
        let mut s = simple.clone();
        f(&mut s);
        LibStyle {
            file: StyleFile {
                name: name.into(),
                description: desc.into(),
                ro: s.style_snapshot(),
                ..Default::default()
            },
            path: None,
            id: None,
        }
    };
    vec![
        mk("Simple", "Le style des modèles types Simple.", &|_| {}),
        mk("Filaire", "Arêtes seules, sans faces.", &|s| {
            s.set_faces(FaceStyle::Wireframe);
        }),
        mk("Ligne cachée", "Faces de la couleur du fond, sur fond blanc.", &|s| {
            s.set_faces(FaceStyle::HiddenLine);
            s.bg_color = [255, 255, 255];
            s.sky = false;
        }),
        mk("Ombré", "Couleurs des matières, sans textures.", &|s| {
            s.set_faces(FaceStyle::Shaded);
        }),
        mk("Monochrome", "Couleurs de recto et de verso par défaut.", &|s| {
            s.set_faces(FaceStyle::Monochrome);
        }),
        mk("Radiographie", "Faces transparentes.", &|s| s.xray = true),
        mk("Croquis", "Arêtes prolongées, extrémités marquées, lignes floues, sur papier.", &|s| {
            s.extension = true;
            s.extension_length = 6;
            s.endpoints = true;
            s.jitter = true;
            s.bg_color = [247, 244, 236];
            s.sky = false;
        }),
        mk("Plan", "Profils épais, fond blanc, sans ciel.", &|s| {
            s.profile_width = 3;
            s.bg_color = [255, 255, 255];
            s.sky = false;
        }),
        mk("Arêtes par axe", "Les arêtes prennent la couleur de l'axe qu'elles suivent.", &|s| {
            s.edge_color_mode = EdgeColor::ByAxis;
        }),
    ]
}

/// The `.style` files of a folder, sorted by name (unreadable files are left out).
pub fn read_folder(dir: &Path) -> Vec<LibStyle> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("style")))
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|p| {
            let mut f = crate::style_file::read(&std::fs::read(&p).ok()?).ok()?;
            if f.name.is_empty() {
                f.name = p.file_stem()?.to_string_lossy().into_owned();
            }
            Some(LibStyle { file: f, path: Some(p), id: None })
        })
        .collect()
}

/// The style settings of `ro` (the model's own, such as fog, left out).
fn style_part(ro: &Map<String, J>) -> Map<String, J> {
    ro.iter().filter(|(k, _)| !MODEL_KEYS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect()
}

impl EsquisseApp {
    /// Adds a style to the model (or takes the model's style of that name), selects it and
    /// applies it, as clicking a style of a collection does.
    pub fn use_style(&mut self, f: &StyleFile) {
        let (mut list, _) = model_styles(&mut self.doc.model, &self.style);
        let id = match list.iter().find(|s| s.name == f.name) {
            Some(s) => s.id,
            None => {
                let id = list.iter().map(|s| s.id).max().unwrap_or(0) + 1;
                list.push(ModelStyle {
                    id,
                    name: f.name.clone(),
                    description: f.description.clone(),
                    ro: style_part(&f.ro),
                    watermarks: f.watermarks.clone(),
                });
                id
            }
        };
        self.select_style(&mut list, id);
    }

    /// Makes a style of the model the active one: its settings become the view's.
    fn select_style(&mut self, list: &mut [ModelStyle], id: i64) {
        if let Some(s) = list.iter().find(|s| s.id == id) {
            s.apply(&mut self.style);
        }
        save_styles(&mut self.doc.model, list, id);
        self.doc.rev += 1;
    }

    /// Styles › Create Style: a new style of the model from the current settings.
    pub fn create_style(&mut self) {
        let (mut list, _) = model_styles(&mut self.doc.model, &self.style);
        let id = list.iter().map(|s| s.id).max().unwrap_or(0) + 1;
        let name = unique_name("Style", &list);
        list.push(ModelStyle::of(id, &name, &self.style));
        save_styles(&mut self.doc.model, &list, id);
    }

    /// Styles › Update Style with Changes.
    pub fn update_style(&mut self) {
        let (mut list, sel) = model_styles(&mut self.doc.model, &self.style);
        if let Some(s) = list.iter_mut().find(|s| s.id == sel) {
            let keep = (s.name.clone(), s.description.clone());
            *s = ModelStyle::of(sel, &keep.0, &self.style);
            s.description = keep.1;
        }
        save_styles(&mut self.doc.model, &list, sel);
    }

    /// Styles › Mix: part `part` of the settings (see `PARTS`) is taken from `from`.
    pub fn mix_style(&mut self, part: usize, from: &StyleFile) {
        if part == 3 {
            self.style.watermark_list = from.watermarks.clone();
        }
        for (k, v) in &from.ro {
            if category_of(k) == Some(part) {
                self.style.ro_set(k, v);
            }
        }
        self.doc.rev += 1;
    }

    /// Styles › Purge Unused: removes the styles that are neither active nor used by a scene.
    pub fn purge_styles(&mut self) -> usize {
        let (mut list, sel) = model_styles(&mut self.doc.model, &self.style);
        let used: Vec<String> = self
            .doc
            .model
            .scenes
            .iter()
            .filter_map(|s| s.ext.props.get("style")?.as_str().map(str::to_string))
            .collect();
        let before = list.len();
        list.retain(|s| s.id == sel || used.contains(&s.name));
        save_styles(&mut self.doc.model, &list, sel);
        before - list.len()
    }
}

fn texture_for(ctx: &egui::Context, key: &str, png: &[u8]) -> Option<egui::TextureId> {
    type Cache = std::sync::Arc<std::sync::Mutex<HashMap<String, Option<egui::TextureHandle>>>>;
    let cache: Cache =
        ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(egui::Id::unique("style_thumbnails")).clone());
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

/// A style's tile: its thumbnail, or a small picture of its colours.
fn tile(ui: &mut egui::Ui, key: &str, f: &StyleFile, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(56.0, 56.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &f.name));
    let p = ui.painter();
    let thumb = f.thumbnail.as_deref().and_then(|t| texture_for(ui.ctx(), key, t));
    if let Some(id) = thumb {
        p.image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    } else {
        let mut s = ViewStyle::default();
        s.restore(&f.ro);
        let c = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
        let (top, bottom) = rect.split_top_bottom_at_fraction(0.5);
        p.rect_filled(top, 0.0, if s.sky { c(s.sky_color) } else { c(s.bg_color) });
        p.rect_filled(bottom, 0.0, if s.ground { c(s.ground_color) } else { c(s.bg_color) });
        // A little box: its faces in the style's colours, its edges in the edge colour.
        let b = egui::Rect::from_center_size(rect.center() + egui::vec2(0.0, 4.0), egui::vec2(26.0, 22.0));
        let face = match s.faces {
            FaceStyle::Wireframe => None,
            FaceStyle::HiddenLine => Some(c(s.bg_color)),
            _ => Some(c(s.front_color)),
        };
        if let Some(fc) = face {
            let fc = if s.xray { fc.gamma_multiply(0.5) } else { fc };
            p.rect_filled(b, 0.0, fc);
        }
        let w = if s.profiles { s.profile_width as f32 } else { 1.0 };
        p.rect_stroke(b, 0.0, Stroke::new(w, c(s.edge_color)), egui::StrokeKind::Middle);
    }
    let stroke = if selected {
        Stroke::new(2.5, Color32::from_rgb(52, 110, 220))
    } else {
        Stroke::new(1.0, Color32::from_gray(160))
    };
    p.rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
    resp.on_hover_text(if f.description.is_empty() { f.name.clone() } else { format!("{}\n{}", f.name, f.description) })
}

/// The panel as it first opens. `ESQ_STYLES_TAB` (`select`, `edit:0`..`edit:4`, `mix`, and
/// `,collection index`) opens it elsewhere, for screen captures.
fn initial_state() -> PanelState {
    let mut st = PanelState::default();
    if let Ok(v) = std::env::var("ESQ_STYLES_TAB") {
        let (tab, coll) = v.split_once(',').unwrap_or((v.as_str(), ""));
        st.tab = match tab.split(':').next() {
            Some("edit") => Tab::Edit,
            Some("mix") => Tab::Mix,
            _ => Tab::Select,
        };
        st.edit = tab.split(':').nth(1).and_then(|n| n.parse().ok()).unwrap_or(0);
        st.collection = coll.parse::<usize>().ok().and_then(|i| collections().get(i).map(|c| c.1.clone()));
    }
    st
}

/// Window › Styles.
pub fn show(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let id = egui::Id::unique("styles_panel");
    let mut st: PanelState = ui.data_mut(|d| d.get_temp(id)).unwrap_or_else(initial_state);
    let before = app.style.clone();
    let (mut list, sel) = model_styles(&mut app.doc.model, &app.style);
    let Some(k) = list.iter().position(|s| s.id == sel) else { return };
    let changed = list[k].differs(&app.style);

    // The active style: name, description, Create and Update.
    ui.horizontal(|ui| {
        let f = StyleFile { name: list[k].name.clone(), ro: app.style.style_snapshot(), ..Default::default() };
        tile(ui, "active", &f, false);
        ui.vertical(|ui| {
            // A name of SketchUp's own (« [Simple Style] ») is shown translated.
            let mut name = crate::style_file::localized(&list[k].name);
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
            if r.changed() && !name.trim().is_empty() && !list.iter().any(|s| s.name == name && s.id != sel) {
                list[k].name = name;
                save_styles(&mut app.doc.model, &list, sel);
            }
            let mut desc = list[k].description.clone();
            if ui.add(egui::TextEdit::multiline(&mut desc).desired_rows(2).desired_width(f32::INFINITY)).changed() {
                list[k].description = desc;
                save_styles(&mut app.doc.model, &list, sel);
            }
        });
    });
    ui.horizontal(|ui| {
        if ui.button("Créer un style").on_hover_text("Crée un style à partir des réglages actuels").clicked() {
            app.create_style();
        }
        if ui
            .add_enabled(changed, egui::Button::new("Mettre à jour le style avec les modifications"))
            .on_hover_text("Le style sélectionné prend les réglages actuels")
            .clicked()
        {
            app.update_style();
        }
    });
    ui.separator();
    ui.horizontal(|ui| {
        for (t, label) in [(Tab::Select, "Sélectionner"), (Tab::Edit, "Modifier"), (Tab::Mix, "Mélanger")] {
            ui.selectable_value(&mut st.tab, t, label);
        }
    });
    ui.separator();
    match st.tab {
        Tab::Select => select_tab(app, ui, &mut st),
        Tab::Edit => edit_tab(app, ui, &mut st),
        Tab::Mix => mix_tab(app, ui, &mut st),
    }
    if let Some(m) = &st.message {
        ui.label(RichText::new(m).small().color(Color32::from_rgb(170, 60, 0)));
    }
    if app.style != before {
        app.doc.rev += 1;
    }
    ui.data_mut(|d| d.insert_temp(id, st));
}

/// The styles of a collection (read once, kept in the panel's state).
fn collection_styles(app: &mut EsquisseApp, st: &mut PanelState, c: &Collection) -> Vec<LibStyle> {
    match c {
        Collection::InModel => {
            let (list, _) = model_styles(&mut app.doc.model, &app.style);
            list.into_iter()
                .map(|s| LibStyle {
                    id: Some(s.id),
                    file: StyleFile {
                        name: crate::style_file::localized(&s.name),
                        description: crate::style_file::localized(&s.description),
                        ro: s.ro,
                        watermarks: s.watermarks,
                        ..Default::default()
                    },
                    path: None,
                })
                .collect()
        }
        Collection::Esquisse => builtin_styles(),
        Collection::Folder(d) => {
            if st.loaded.as_ref().is_none_or(|(lc, _)| lc != c) {
                st.loaded = Some((c.clone(), read_folder(d)));
            }
            st.loaded.as_ref().map(|(_, l)| l.clone()).unwrap_or_default()
        }
    }
}

fn select_tab(app: &mut EsquisseApp, ui: &mut egui::Ui, st: &mut PanelState) {
    let all = collections();
    let current = st.collection.clone().unwrap_or(Collection::InModel);
    let label = all.iter().find(|(_, c)| *c == current).map(|(n, _)| n.clone()).unwrap_or_default();
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("styles_collection").selected_text(label).width(200.0).show_ui(ui, |ui| {
            for (name, c) in &all {
                if ui.selectable_label(*c == current, name).clicked() {
                    st.collection = Some(c.clone());
                }
            }
        });
        ui.menu_button("☰", |ui| {
            if ui.button("Ouvrir un style…").clicked() {
                ui.close();
                if let Some(p) =
                    rfd::FileDialog::new().add_filter("Fichiers de style (*.style)", &["style"]).pick_file()
                {
                    match std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| crate::style_file::read(&b)) {
                        Ok(mut f) => {
                            if f.name.is_empty() {
                                f.name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                            }
                            app.use_style(&f);
                            st.message = None;
                        }
                        Err(e) => st.message = Some(format!("Style illisible : {e}")),
                    }
                }
            }
            if ui.button("Enregistrer comme…").clicked() {
                ui.close();
                st.message = save_style_dialog(app).err();
            }
            if ui.button("Purger les éléments inutilisés").clicked() {
                ui.close();
                let n = app.purge_styles();
                st.message = Some(format!("{n} style(s) supprimé(s)"));
            }
        });
    });
    let styles = collection_styles(app, st, &current);
    let (_, sel) = model_styles(&mut app.doc.model, &app.style);
    let mut clicked = None;
    let mut remove = None;
    egui::ScrollArea::vertical().max_height(260.0).id_salt("styles_tiles").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for (i, s) in styles.iter().enumerate() {
                let key =
                    s.path.as_ref().map_or_else(|| format!("{current:?}/{}", s.file.name), |p| p.display().to_string());
                let selected = s.id == Some(sel);
                let r = tile(ui, &key, &s.file, selected);
                if r.clicked() {
                    clicked = Some(i);
                }
                if current == Collection::InModel {
                    r.context_menu(|ui| {
                        if ui.button("Supprimer").clicked() {
                            remove = s.id;
                            ui.close();
                        }
                    });
                }
            }
        });
    });
    if let Some(i) = clicked {
        match styles[i].id {
            Some(id) => {
                let (mut list, _) = model_styles(&mut app.doc.model, &app.style);
                app.select_style(&mut list, id);
            }
            None => app.use_style(&styles[i].file),
        }
    }
    if let Some(id) = remove {
        let (mut list, sel) = model_styles(&mut app.doc.model, &app.style);
        if list.len() > 1 {
            list.retain(|s| s.id != id);
            let sel = if list.iter().any(|s| s.id == sel) { sel } else { list[0].id };
            app.select_style(&mut list, sel);
        }
    }
}

/// Saves the active style as a `.style` file (Styles › Save As).
fn save_style_dialog(app: &mut EsquisseApp) -> Result<(), String> {
    let (list, sel) = model_styles(&mut app.doc.model, &app.style);
    let s = list.iter().find(|s| s.id == sel).cloned().ok_or("pas de style")?;
    let mut d = rfd::FileDialog::new()
        .add_filter("Fichiers de style (*.style)", &["style"])
        .set_file_name(format!("{}.style", s.name));
    if let Some(dir) = user_styles_dir() {
        d = d.set_directory(dir);
    }
    let Some(p) = d.save_file() else { return Ok(()) };
    let f = StyleFile {
        name: s.name,
        description: s.description,
        ro: app.style.style_snapshot(),
        watermarks: app.style.watermark_list.clone(),
        ..Default::default()
    };
    std::fs::write(&p, crate::style_file::write(&f)).map_err(|e| format!("Échec de l'enregistrement : {e}"))
}

fn color_row(ui: &mut egui::Ui, label: &str, c: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        ui.color_edit_button_srgb(c);
        ui.label(label);
    });
}

fn num_row(
    ui: &mut egui::Ui,
    on: Option<&mut bool>,
    label: &str,
    n: Option<&mut u32>,
    range: std::ops::RangeInclusive<u32>,
) {
    ui.horizontal(|ui| {
        let enabled = match on {
            Some(b) => {
                ui.checkbox(b, label);
                *b
            }
            None => {
                ui.label(label);
                true
            }
        };
        if let Some(n) = n {
            ui.add_enabled(enabled, egui::DragValue::new(n).range(range));
        }
    });
}

fn edit_tab(app: &mut EsquisseApp, ui: &mut egui::Ui, st: &mut PanelState) {
    ui.horizontal(|ui| {
        for (i, p) in PARTS.iter().enumerate() {
            ui.selectable_value(&mut st.edit, i, *p);
        }
    });
    ui.separator();
    if st.edit == 3 {
        crate::watermarks::edit(app, ui);
        return;
    }
    let mut ask_photoreal = false;
    let s = &mut app.style;
    match st.edit {
        0 => {
            ui.checkbox(&mut s.edges, "Arêtes");
            ui.checkbox(&mut s.back_edges, "Arêtes arrière");
            num_row(ui, Some(&mut s.profiles), "Profils", Some(&mut s.profile_width), 1..=999);
            num_row(ui, Some(&mut s.depth_cue), "Effet de profondeur", Some(&mut s.depth_cue_width), 1..=4);
            num_row(ui, Some(&mut s.extension), "Extension", Some(&mut s.extension_length), 1..=999);
            num_row(ui, Some(&mut s.endpoints), "Extrémités", Some(&mut s.endpoint_length), 1..=999);
            ui.checkbox(&mut s.jitter, "Lignes floues");
            ui.checkbox(&mut s.dashes, "Tirets");
            ui.horizontal(|ui| {
                ui.label("Couleur :");
                egui::ComboBox::from_id_salt("edge_color").selected_text(s.edge_color_mode.label()).show_ui(ui, |ui| {
                    for m in EdgeColor::ALL {
                        ui.selectable_value(&mut s.edge_color_mode, m, m.label());
                    }
                });
                ui.color_edit_button_srgb(&mut s.edge_color);
            });
        }
        1 => {
            color_row(ui, "Couleur du recto", &mut s.front_color);
            color_row(ui, "Couleur du verso", &mut s.back_color);
            ui.label(RichText::new("Style").strong());
            ui.horizontal_wrapped(|ui| {
                for f in FaceStyle::ALL {
                    if ui.selectable_label(s.faces == f, f.label()).clicked() {
                        s.set_faces(f);
                    }
                }
                // Turned on: the question of `warnings::turn_photoreal_on` first.
                if ui.toggle_value(&mut s.photoreal, "Matériaux photoréalistes").changed() && s.photoreal {
                    s.photoreal = false;
                    ask_photoreal = true;
                }
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.xray, "Rayons X");
                // Its opacity can be set only with « Meilleure »: « Rapide »
                // shows X-ray at 50 %.
                let mut pct = (s.xray_opacity * 100.0).round();
                let can = s.xray && crate::oit::nicer(s.transparency_sort);
                if ui
                    .add_enabled(can, egui::Slider::new(&mut pct, 20.0..=80.0).fixed_decimals(0).text("Opacité"))
                    .changed()
                {
                    s.xray_opacity = pct / 100.0;
                }
            });
            ui.checkbox(&mut s.transparency, "Activer la transparence");
            // `CStylesEditFacePage`: « Rapide » (values 0 and 1) and « Meilleure » (2); the
            // list counts for X-ray too.
            ui.add_enabled_ui(s.transparency || s.xray, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Qualité de transparence:");
                    let names = ["Rapide", "Meilleure"];
                    let was = usize::from(crate::oit::nicer(s.transparency_sort));
                    let mut q = was;
                    egui::ComboBox::from_id_salt("transparency_quality").selected_text(names[q]).show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut q, i, *n);
                        }
                    });
                    if q != was {
                        s.transparency_sort = q as i64 + 1;
                    }
                });
            });
            ui.checkbox(&mut s.ambient_occlusion, "Occlusion ambiante");
        }
        2 => {
            color_row(ui, "Arrière-plan", &mut s.bg_color);
            ui.horizontal(|ui| {
                ui.color_edit_button_srgb(&mut s.sky_color);
                ui.checkbox(&mut s.sky, "Ciel");
            });
            ui.horizontal(|ui| {
                ui.color_edit_button_srgb(&mut s.ground_color);
                ui.checkbox(&mut s.ground, "Sol");
            });
            ui.add_enabled_ui(s.ground, |ui| {
                ui.add(egui::Slider::new(&mut s.ground_transparency, 0..=100).text("Transparence"));
                ui.checkbox(&mut s.underground, "Montrer le sol par en dessous");
            });
        }
        3 => {}
        _ => {
            color_row(ui, "Sélectionné", &mut s.highlight_color);
            color_row(ui, "Verrouillé", &mut s.locked_color);
            color_row(ui, "Guides", &mut s.guide_color);
            color_row(ui, "Section inactive", &mut s.section_inactive_color);
            color_row(ui, "Section active", &mut s.section_active_color);
            color_row(ui, "Coupes", &mut s.section_cut_color);
            color_row(ui, "Remplissage coupe", &mut s.section_fill_color);
            num_row(ui, None, "Largeur de la ligne de coupe", Some(&mut s.section_cut_width), 1..=999);
            ui.separator();
            ui.checkbox(&mut s.hidden_geometry, "Géométrie cachée");
            ui.checkbox(&mut s.color_by_tag, "Colorer par étiquette");
            ui.checkbox(&mut s.section_planes, "Plans de section");
            ui.checkbox(&mut s.section_cuts, "Coupes");
            ui.checkbox(&mut s.section_fill, "Remplissage coupe");
            ui.checkbox(&mut s.guides, "Guides");
            ui.checkbox(&mut s.axes, "Axes du modèle");
            // `StylesEditModelingPage`: the foreground photo (over the faces, `photo_fg.rs`),
            // then the background one, each with its opacity.
            ui.label(RichText::new("Adapter une photo").strong());
            for (on, opacity, label) in [
                (&mut s.photo_fg, &mut s.photo_fg_opacity, "Photo de premier plan"),
                (&mut s.photo_bg, &mut s.photo_bg_opacity, "Photo d'arrière-plan"),
            ] {
                ui.horizontal(|ui| {
                    ui.checkbox(on, label);
                    let mut v = *opacity * 100.0;
                    let slider = egui::Slider::new(&mut v, 0.0..=100.0).fixed_decimals(0).text("Opacité");
                    if ui.add_enabled(*on, slider).changed() {
                        *opacity = v / 100.0;
                    }
                });
            }
        }
    }
    if ask_photoreal {
        crate::warnings::turn_photoreal_on(app);
    }
}

fn mix_tab(app: &mut EsquisseApp, ui: &mut egui::Ui, st: &mut PanelState) {
    ui.label("Choisissez un style pour chaque partie : ses réglages remplacent ceux du style actif.");
    let current = st.collection.clone().unwrap_or(Collection::InModel);
    let mut sources = collection_styles(app, st, &Collection::InModel);
    if current != Collection::InModel {
        sources.extend(collection_styles(app, st, &current));
    }
    let mut apply = None;
    egui::Grid::new("styles_mix").num_columns(2).show(ui, |ui| {
        for (part, name) in PARTS.iter().enumerate() {
            ui.label(format!("Paramètres : {}", name.to_lowercase()));
            egui::ComboBox::from_id_salt(("mix", part)).selected_text("Choisir un style…").width(180.0).show_ui(
                ui,
                |ui| {
                    for (i, s) in sources.iter().enumerate() {
                        if ui.selectable_label(false, &s.file.name).clicked() {
                            apply = Some((part, i));
                        }
                    }
                },
            );
            ui.end_row();
        }
    });
    if let Some((part, i)) = apply {
        app.mix_style(part, &sources[i].file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_are_used_created_updated_mixed_and_purged() {
        let mut app = EsquisseApp::bare(None, None);
        let lib = builtin_styles();
        let sketch = lib.iter().find(|s| s.file.name == "Croquis").unwrap();
        app.use_style(&sketch.file);
        assert!(app.style.jitter && app.style.endpoints && !app.style.sky);
        let (list, sel) = model_styles(&mut app.doc.model, &app.style);
        assert_eq!(list.len(), 2);
        assert_eq!(list.iter().find(|s| s.id == sel).unwrap().name, "Croquis");
        // Fog is the model's: a style leaves it alone.
        app.style.fog = true;
        app.use_style(&lib[0].file);
        assert!(app.style.fog && !app.style.jitter);
        // Changed settings, then Update: the style takes them.
        app.style.bg_color = [1, 2, 3];
        let (list, sel) = model_styles(&mut app.doc.model, &app.style);
        assert!(list.iter().find(|s| s.id == sel).unwrap().differs(&app.style));
        app.update_style();
        let (list, sel) = model_styles(&mut app.doc.model, &app.style);
        assert!(!list.iter().find(|s| s.id == sel).unwrap().differs(&app.style));
        // Create: a new style, the selection stays on the new one.
        app.create_style();
        let (list, sel) = model_styles(&mut app.doc.model, &app.style);
        assert_eq!(list.iter().find(|s| s.id == sel).unwrap().name, "Style");
        // Mix: the edges of the sketch style, nothing else.
        app.mix_style(0, &sketch.file);
        assert!(app.style.jitter && app.style.extension && app.style.bg_color == [1, 2, 3]);
        // Purge: only the active style stays.
        assert_eq!(app.purge_styles(), 3);
        assert_eq!(model_styles(&mut app.doc.model, &app.style).0.len(), 1);
    }

    #[test]
    fn collections_list_the_model_esquisse_and_the_libraries() {
        let c = collections();
        assert_eq!(c[0].1, Collection::InModel);
        assert_eq!(c[1].1, Collection::Esquisse);
        // The library that comes with Esquisse, its collections named in French.
        let lib = library().expect("styles livrés avec Esquisse");
        assert!(c.iter().any(|(n, _)| n == "Styles par défaut"));
        assert!(c.iter().any(|(n, _)| n == "Arêtes esquissées"));
        let styles = read_folder(&lib.join("Default Styles"));
        assert!(styles.iter().any(|s| s.file.name == "Style simple" && s.file.thumbnail.is_some()));
    }
}
