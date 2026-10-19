//! Window › Materials, as SketchUp's panel (`CMaterialBrowserPage`, `CMaterialEditPage`):
//! a header (the current material, its name, Create Material, the Sample Paint eyedropper)
//! and two tabs, Select (the materials of the model) and Edit:
//!
//! - Color: Color Wheel, HLS, HSB or RGB; "Match Color of object in model" (the material
//!   takes the colour of the face clicked next); "Match color on screen" (the colour of the
//!   pixel under the next press); "Undo Color Changes". On a textured
//!   material the colour colorizes the texture (hue, saturation and lightness shifts from
//!   the image's average), "Colorize" forces every texel to the colour's hue, "Reset Color"
//!   goes back to the image's colours.
//! - Texture: "Use texture image", "Browse for Material Image File", "Edit texture image in
//!   external editor" (reloaded when the file changes), width and height with
//!   "Lock/Unlock Aspect Ratio" and "Undo width/height changes"; a new image keeps the
//!   larger side of the old one.
//! - Opacity (0–100).
//! - Metalness, Roughness (with Invert), Normal (scale, Flip to DX/GL) and Ambient Occlusion,
//!   each with its enabling box, factor and map (Add / Replace / Remove), as the PBR section
//!   of SketchUp 2025+.
//! - The material's hatch pattern (section cuts).
//!
//! A change made by dragging is one undoable step, taken when the drag ends.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Color32, RichText, Sense, Stroke};
use esq_core::meta::{MaterialExt, TextureData};
use esq_core::model::{EntityRef, Material, MaterialId, Model};
use esq_core::photo::Blob;
use esq_core::units::{format_length, parse_length};

use crate::app::EsquisseApp;
use crate::textures::{hls, rgb};
use crate::tools::ToolId;

/// Pick colours with: SketchUp's four choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    Wheel,
    Hls,
    Hsb,
    Rgb,
}

impl ColorMode {
    const ALL: [ColorMode; 4] = [ColorMode::Wheel, ColorMode::Hls, ColorMode::Hsb, ColorMode::Rgb];

    fn label(self) -> &'static str {
        match self {
            ColorMode::Wheel => "Roue chromatique",
            ColorMode::Hls => "HLS",
            ColorMode::Hsb => "HSB",
            ColorMode::Rgb => "RGB",
        }
    }
}

struct State {
    edit_tab: bool,
    mode: ColorMode,
    lock_aspect: bool,
    /// The material as it was when it became the one edited ("Undo …" buttons).
    start: Option<(MaterialId, Material)>,
    /// A drag in progress: the material before it.
    live: Option<(MaterialId, Material)>,
    /// Texture size fields being typed.
    size_text: Option<(MaterialId, String, String)>,
    thumbs: HashMap<(usize, u8, [u64; 3]), egui::TextureHandle>,
    /// "Edit texture image in external editor": the file handed out and when it was written.
    external: Option<(MaterialId, PathBuf, std::time::SystemTime)>,
    /// "Match color on screen" in progress.
    screen_pick: Option<ScreenPick>,
    /// The collection the Select tab shows: a library folder, or the model's (None).
    collection: Option<PathBuf>,
    /// Library folders read once: folder → its materials.
    libraries: HashMap<PathBuf, Vec<Material>>,
    /// Collections opened in this session (menu › Open or create a collection...).
    opened: Vec<PathBuf>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        edit_tab: false,
        mode: ColorMode::Wheel,
        lock_aspect: true,
        start: None,
        live: None,
        size_text: None,
        thumbs: HashMap::new(),
        external: None,
        screen_pick: None,
        collection: initial_collection(),
        libraries: HashMap::new(),
        opened: Vec::new(),
    });
}

/// The collection the panel first shows; `ESQ_MAT_COLLECTION` (a collection's index, from 1)
/// opens it on a library, for screen captures.
fn initial_collection() -> Option<PathBuf> {
    let i: usize = std::env::var("ESQ_MAT_COLLECTION").ok()?.parse().ok()?;
    collections().get(i.checked_sub(1)?).map(|(_, p)| p.clone())
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

// ---------------------------------------------------------------------------------------
// What the controls do to a material (also what the tests drive).

/// The colour of a material. On a textured one it colorizes the texture: the shifts from the
/// image's average colour to `c` (hue, saturation, lightness), as SketchUp does.
pub fn set_color(m: &mut Material, c: [u8; 3]) {
    m.color = c;
    if let Some(t) = &m.ext.texture {
        let to = hls(c.map(|v| v as f64 / 255.0));
        let from = hls(t.avg.map(|v| v as f64 / 255.0));
        let mut dh = to[0] - from[0];
        if dh > 180.0 {
            dh -= 360.0;
        } else if dh < -180.0 {
            dh += 360.0;
        }
        m.ext.colorize_deltas = [dh, to[1] - from[1], to[2] - from[2]];
        if m.ext.colorize_deltas.iter().all(|d| d.abs() < 1e-9) {
            m.ext.colorize_deltas = [0.0; 3];
        }
    }
}

/// "Colorize": every texel takes the colour's hue (tint), else the colours are shifted.
pub fn set_colorize(m: &mut Material, tint: bool) {
    m.ext.colorize_type = tint as u8;
    if tint && m.ext.colorize_deltas == [0.0; 3] {
        // A tint of the image's own colour still changes it: make the deltas exist.
        let c = m.color;
        set_color(m, c);
        if m.ext.colorize_deltas == [0.0; 3] {
            m.ext.colorize_deltas = [0.0, 0.0, 1e-6];
        }
    }
}

/// "Reset Color": the image's own colours again.
pub fn reset_color(m: &mut Material) {
    if let Some(t) = &m.ext.texture {
        m.color = t.avg;
    }
    m.ext.colorize_deltas = [0.0; 3];
    m.ext.colorize_type = 0;
}

/// Width and/or height (mm) of a texture tile; with the aspect locked the other side follows.
pub fn set_texture_size(m: &mut Material, w: Option<f64>, h: Option<f64>, lock: bool) {
    let Some(t) = m.ext.texture.as_mut() else { return };
    let [ow, oh] = t.size;
    let ratio = if ow > 0.0 { oh / ow } else { 1.0 };
    match (w, h) {
        (Some(w), _) if w > 0.0 => {
            t.size[0] = w;
            if lock {
                t.size[1] = w * ratio;
            }
        }
        (_, Some(h)) if h > 0.0 => {
            t.size[1] = h;
            if lock && ratio > 0.0 {
                t.size[0] = h / ratio;
            }
        }
        _ => {}
    }
}

/// A texture made from an image file. Its tile is one inch per pixel, as SketchUp's
/// `Material#texture=` without a size.
pub fn load_texture(path: &Path) -> Result<TextureData, String> {
    let bytes = std::fs::read(path)
        .map_err(|e| format!("Impossible de charger le fichier image « {} » : {e}", path.display()))?;
    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    texture_from_bytes(&name, bytes)
}

pub fn texture_from_bytes(name: &str, bytes: Vec<u8>) -> Result<TextureData, String> {
    let img = crate::photo_img::decode(&bytes)?;
    let rgb8 = img.to_rgb8();
    let (w, h) = rgb8.dimensions();
    let step = ((w as u64 * h as u64) / 65_536).max(1) as usize;
    let (mut s, mut n) = ([0u64; 3], 0u64);
    for p in rgb8.pixels().step_by(step) {
        for (acc, v) in s.iter_mut().zip(p.0) {
            *acc += v as u64;
        }
        n += 1;
    }
    let n = n.max(1);
    Ok(TextureData {
        filename: esq_core::meta::base_name(name),
        image: Blob(Arc::new(bytes)),
        px: [w, h],
        size: [w as f64 * 25.4, h as f64 * 25.4],
        avg: [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8],
    })
}

/// A new image for a material: an existing texture keeps its larger side, the other follows
/// the new picture's proportions (SketchUp's); a first texture colours the
/// material with the image's average.
pub fn set_texture(m: &mut Material, mut t: TextureData) {
    match &m.ext.texture {
        Some(old) if t.px[0] > 0 && t.px[1] > 0 => {
            let ratio = t.px[1] as f64 / t.px[0] as f64;
            let [w, h] = old.size;
            t.size = if w >= h { [w, w * ratio] } else { [h / ratio, h] };
        }
        Some(_) => {}
        None => {
            m.color = t.avg;
            m.ext.colorize_deltas = [0.0; 3];
        }
    }
    m.ext.texture = Some(t);
}

/// "Use texture image" unticked.
pub fn remove_texture(m: &mut Material) {
    m.ext.texture = None;
    m.ext.colorize_deltas = [0.0; 3];
    m.ext.colorize_type = 0;
}

/// The PBR maps of the Edit tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PbrMap {
    Metalness,
    Roughness,
    Normal,
    Occlusion,
}

impl PbrMap {
    const ALL: [PbrMap; 4] = [PbrMap::Metalness, PbrMap::Roughness, PbrMap::Normal, PbrMap::Occlusion];

    fn title(self) -> &'static str {
        match self {
            PbrMap::Metalness => "Aspect métallique",
            PbrMap::Roughness => "Rugosité",
            PbrMap::Normal => "Normale",
            PbrMap::Occlusion => "Occlusion ambiante",
        }
    }

    fn enable_tip(self) -> &'static str {
        match self {
            PbrMap::Metalness => "Activer les propriétés d'aspect métallique",
            PbrMap::Roughness => "Activer les propriétés de rugosité",
            PbrMap::Normal => "Activer la carte normale",
            PbrMap::Occlusion => "Activer la carte d'occlusion",
        }
    }

    fn factor_tip(self) -> &'static str {
        match self {
            PbrMap::Metalness => "Coefficient d'aspect métallique",
            PbrMap::Roughness => "Coefficient de rugosité",
            PbrMap::Normal => "Échelle normale",
            PbrMap::Occlusion => "Intensité de l'occlusion",
        }
    }

    fn add_label(self) -> &'static str {
        match self {
            PbrMap::Metalness => "Ajouter la carte Metalness",
            PbrMap::Roughness => "Ajouter une carte de rugosité",
            PbrMap::Normal => "Ajouter une carte de normalité",
            PbrMap::Occlusion => "Ajouter une carte d'occlusion ambiante",
        }
    }

    fn replace_label(self) -> &'static str {
        match self {
            PbrMap::Metalness => "Remplacer la carte d'aspect métallique",
            PbrMap::Roughness => "Remplacer la carte de rugosité",
            PbrMap::Normal => "Remplacer la carte normale",
            PbrMap::Occlusion => "Remplacer la carte d'occlusion",
        }
    }

    fn remove_label(self) -> &'static str {
        match self {
            PbrMap::Metalness => "Suppression de la carte de métallicité",
            PbrMap::Roughness => "Supprimer la carte de rugosité",
            PbrMap::Normal => "Supprimer la carte des normales",
            PbrMap::Occlusion => "Supprimer la carte d'occlusion",
        }
    }

    fn range(self) -> std::ops::RangeInclusive<f64> {
        match self {
            PbrMap::Normal => 0.0..=5.0,
            _ => 0.0..=1.0,
        }
    }

    /// (enabled, factor, map) of this map in a material.
    pub fn parts(self, e: &mut MaterialExt) -> (&mut bool, &mut f64, &mut Option<TextureData>) {
        let p = &mut e.pbr;
        match self {
            PbrMap::Metalness => (&mut p.metalness_enabled, &mut p.metallic_factor, &mut p.metallic_texture),
            PbrMap::Roughness => (&mut p.roughness_enabled, &mut p.roughness_factor, &mut p.roughness_texture),
            PbrMap::Normal => (&mut p.normal_enabled, &mut p.normal_scale, &mut p.normal_texture),
            PbrMap::Occlusion => (&mut p.ao_enabled, &mut p.ao_strength, &mut p.ao_texture),
        }
    }
}

/// Turns a PBR property on or off; metalness or roughness makes the material a PBR one.
pub fn set_pbr_enabled(m: &mut Material, map: PbrMap, on: bool) {
    let (enabled, _, tex) = map.parts(&mut m.ext);
    // A normal or occlusion map does nothing without its image (SketchUp: "Failed to enable
    // normal mapping: normal texture not set.").
    *enabled = on && (tex.is_some() || matches!(map, PbrMap::Metalness | PbrMap::Roughness));
    let p = &m.ext.pbr;
    m.ext.workflow = (p.metalness_enabled || p.roughness_enabled) as u8;
}

/// Adds, replaces (`Some`) or removes (`None`) the image of a PBR map. Adding one turns the
/// property on.
pub fn set_pbr_map(m: &mut Material, map: PbrMap, t: Option<TextureData>) {
    let (enabled, _, tex) = map.parts(&mut m.ext);
    let adding = t.is_some();
    *tex = t;
    if adding {
        *enabled = true;
    } else if matches!(map, PbrMap::Normal | PbrMap::Occlusion) {
        *enabled = false;
    }
    let p = &m.ext.pbr;
    m.ext.workflow = (p.metalness_enabled || p.roughness_enabled) as u8;
}

/// "Create Material…": a copy of the current material (or a new grey one) under a free
/// name, which becomes the current one.
pub fn create_material(model: &mut Model, from: Option<MaterialId>) -> MaterialId {
    let src = from.and_then(|id| model.materials.get(id)).cloned();
    let taken: Vec<String> = model.materials.values().map(|m| m.name.clone()).collect();
    let base = src.as_ref().map_or("Matière", |m| m.name.as_str()).to_string();
    let name = Model::unique_name(&base, &taken);
    let mut m =
        src.unwrap_or(Material { name: String::new(), color: [128, 128, 128], opacity: 1.0, ext: Default::default() });
    m.name = name;
    m.ext.owner_image = false;
    model.materials.insert(m)
}

// ---------------------------------------------------------------------------------------
// The panel.

/// The materials Window › Materials lists (an image's own material is not one of them).
pub fn listed(model: &Model) -> Vec<MaterialId> {
    let mut v: Vec<(String, MaterialId)> =
        model.materials.iter().filter(|(_, m)| !m.ext.owner_image).map(|(k, m)| (m.name.to_lowercase(), k)).collect();
    v.sort();
    v.into_iter().map(|x| x.1).collect()
}

/// A small picture of a material: its texture (colorized) or its colour.
fn thumb(ui: &egui::Ui, m: &Material) -> Option<egui::TextureHandle> {
    let t = m.ext.texture.as_ref()?;
    let key = (Arc::as_ptr(&t.image.0) as usize, m.ext.colorize_type, m.ext.colorize_deltas.map(f64::to_bits));
    if let Some(h) = with_state(|s| s.thumbs.get(&key).cloned()) {
        return Some(h);
    }
    let texels = crate::textures::of_material(&m.ext)?;
    let side = 64;
    let px = crate::textures::resized(&texels, side);
    let h = crate::gpu_img::load_texture(
        ui.ctx(),
        format!("matiere-{}", key.0),
        [side as usize, side as usize],
        &px,
        egui::TextureOptions::LINEAR,
    );
    with_state(|s| s.thumbs.insert(key, h.clone()));
    Some(h)
}

pub(crate) fn swatch(ui: &mut egui::Ui, m: Option<&Material>, size: f32, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), Sense::click());
    let name = m.map_or("Défaut".to_string(), |m| m.name.clone());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
    let p = ui.painter();
    match m {
        Some(m) => match thumb(ui, m) {
            Some(t) => {
                p.image(
                    t.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                p.rect_filled(rect, 4.0, crate::tools::c32(m.color));
            }
        },
        None => {
            p.rect_filled(rect, 4.0, crate::tools::c32(crate::scene::FRONT));
            p.line_segment([rect.left_bottom(), rect.right_top()], Stroke::new(1.0, Color32::from_gray(150)));
        }
    }
    let stroke = if selected {
        Stroke::new(2.5, Color32::from_rgb(52, 110, 220))
    } else {
        Stroke::new(1.0, Color32::from_gray(160))
    };
    p.rect_stroke(rect, 4.0, stroke, egui::StrokeKind::Inside);
    resp
}

/// A small button showing a tool's icon.
fn icon_button(ui: &mut egui::Ui, tool: ToolId, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, tool.label()));
    let v = ui.style().interact_selectable(&resp, selected);
    ui.painter().rect(rect, 3.0, v.weak_bg_fill, v.bg_stroke, egui::StrokeKind::Inside);
    crate::icons::draw(ui.painter(), rect.shrink(2.0), tool);
    resp
}

/// Picks the current material (Select tab, swatches): the Paint Bucket takes it.
fn choose(app: &mut EsquisseApp, id: Option<MaterialId>) {
    app.material = id;
    crate::tools::sample::forget();
    app.set_tool(ToolId::Paint);
}

pub fn show(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    screen_pick_frame(app, ui.ctx());
    let current = app.material.filter(|id| app.doc.model.materials.contains_key(*id));
    // The edited material changed: remember it for the Undo buttons.
    with_state(|s| {
        if s.start.as_ref().map(|x| x.0) != current {
            s.start = current.map(|id| (id, app.doc.model.materials[id].clone()));
            s.size_text = None;
        }
    });
    reload_external(app);

    // Header: the current material, its name, Create Material, Sample Paint.
    ui.horizontal(|ui| {
        let m = current.map(|id| app.doc.model.materials[id].clone());
        swatch(ui, m.as_ref(), 40.0, false).on_hover_text(m.as_ref().map_or("Défaut", |m| m.name.as_str()));
        ui.vertical(|ui| {
            match current {
                Some(id) => {
                    let mut name = app.doc.model.materials[id].name.clone();
                    let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(150.0));
                    if r.lost_focus() && !name.trim().is_empty() && name != app.doc.model.materials[id].name {
                        let taken: Vec<String> = app
                            .doc
                            .model
                            .materials
                            .iter()
                            .filter(|(k, _)| *k != id)
                            .map(|(_, m)| m.name.clone())
                            .collect();
                        let name = Model::unique_name(name.trim(), &taken);
                        app.doc.commit("Renommer la matière", |m, _| {
                            let old = std::mem::replace(&mut m.materials[id].name, name.clone());
                            m.rename_attr_owner(&format!("mat:{old}"), &format!("mat:{name}"));
                        });
                    }
                }
                None => {
                    ui.label("Défaut");
                }
            }
            ui.horizontal(|ui| {
                if ui.button("Créer une matière...").on_hover_text("Créer une matière").clicked() {
                    let mut made = None;
                    app.doc.commit("Créer une matière", |m, _| made = Some(create_material(m, current)));
                    app.material = made;
                    with_state(|s| s.edit_tab = true);
                }
                let sampling = app.tool.id() == ToolId::SampleMaterial;
                if icon_button(ui, ToolId::SampleMaterial, sampling).on_hover_text("Échantillon de coloriage").clicked()
                {
                    app.set_tool(if sampling { ToolId::Paint } else { ToolId::SampleMaterial });
                }
            });
        });
    });
    let edit = with_state(|s| s.edit_tab);
    ui.horizontal(|ui| {
        if ui.selectable_label(!edit, "Sélectionner").clicked() {
            with_state(|s| s.edit_tab = false);
        }
        if ui.add_enabled(current.is_some(), egui::Button::selectable(edit, "Éditer")).clicked() {
            with_state(|s| s.edit_tab = true);
        }
    });
    ui.separator();
    if !edit || current.is_none() {
        select_tab(app, ui, current);
    } else if let Some(id) = current {
        edit_tab(app, ui, id);
    }
    finish_live(app, ui);
}

/// The collections of the Select tab after "In Model": the library that comes with Esquisse
/// (one per folder), then the folder chosen in Preferences › Files › Materials.
pub fn collections() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut add = |lib: &Path, prefix: &str| {
        let dirs = esq_core::content::subfolders(lib);
        let dirs = if dirs.is_empty() { vec![lib.to_path_buf()] } else { dirs };
        for d in dirs {
            let name = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            out.push((format!("{prefix}{}", crate::i18n::fr_or_same(&name)), d));
        }
    };
    let lib = crate::skm::library();
    if let Some(l) = &lib {
        add(l, "");
    }
    if let Some(l) = crate::settings::with(|s| s.location("materials"))
        && Some(&l) != lib.as_ref()
    {
        add(&l, "Préférences › ");
    }
    out
}

/// A material of a library becomes the current one: the model's material of that name, else
/// a copy added to the model (one undoable step).
pub fn use_library_material(app: &mut EsquisseApp, m: &Material) -> MaterialId {
    if let Some(id) =
        app.doc.model.materials.iter().find(|(_, x)| x.name == m.name && !x.ext.owner_image).map(|(k, _)| k)
    {
        choose(app, Some(id));
        return id;
    }
    let mut made = None;
    app.doc.commit("Ajouter une matière", |model, _| {
        let taken: Vec<String> = model.materials.values().map(|x| x.name.clone()).collect();
        let mut copy = m.clone();
        copy.name = Model::unique_name(&m.name, &taken);
        made = Some(model.materials.insert(copy));
    });
    choose(app, made);
    made.expect("matière ajoutée")
}

/// Details › Save collection as...: every material of the model as a `.skm` file of `dir`
/// (named after the material); the number written.
pub fn save_collection(m: &Model, dir: &Path) -> usize {
    let _ = std::fs::create_dir_all(dir);
    listed(m)
        .into_iter()
        .filter(|id| {
            let mat = &m.materials[*id];
            let clean: String = mat.name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
            std::fs::write(dir.join(format!("{}.skm", clean.trim())), crate::skm::write(mat, None)).is_ok()
        })
        .count()
}

/// Details › Purge Unused: the model's materials that paint nothing go (one operation).
pub fn purge_materials(app: &mut EsquisseApp) -> usize {
    let unused: Vec<MaterialId> =
        listed(&app.doc.model).into_iter().filter(|id| painted_with(&app.doc.model, *id) == 0).collect();
    if !unused.is_empty() {
        app.doc.commit("Purger les matières", |m, _| {
            for id in &unused {
                if let Some(x) = m.materials.remove(*id) {
                    m.attr_store.remove(&format!("mat:{}", x.name));
                }
            }
        });
    }
    unused.len()
}

fn select_tab(app: &mut EsquisseApp, ui: &mut egui::Ui, current: Option<MaterialId>) {
    let mut all = collections();
    for d in with_state(|s| s.opened.clone()) {
        if !all.iter().any(|(_, p)| *p == d) {
            let n = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            all.push((n, d));
        }
    }
    let chosen = with_state(|s| s.collection.clone()).filter(|c| all.iter().any(|(_, p)| p == c));
    let label = chosen.as_ref().and_then(|c| all.iter().find(|(_, p)| p == c)).map_or("Dans le modèle", |x| &x.0);
    let mut pick = None;
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("materials_collection").selected_text(label).width(200.0).show_ui(ui, |ui| {
            if ui.selectable_label(chosen.is_none(), "Dans le modèle").clicked() {
                pick = Some(None);
            }
            for (name, p) in &all {
                if ui.selectable_label(chosen.as_ref() == Some(p), name).clicked() {
                    pick = Some(Some(p.clone()));
                }
            }
        });
        // The details menu of SketchUp's browser: collections and the model's materials.
        ui.menu_button("☰", |ui| {
            if ui.button("Ouvrir ou créer une collection...").clicked() {
                ui.close();
                if let Some(d) = crate::settings::dialog("materials").pick_folder() {
                    with_state(|s| {
                        s.opened.push(d.clone());
                        s.libraries.remove(&d);
                    });
                    pick = Some(Some(d));
                }
            }
            if ui.button("Enregistrer la collection sous...").clicked() {
                ui.close();
                if let Some(d) = crate::settings::dialog("materials").pick_folder() {
                    let n = save_collection(&app.doc.model, &d);
                    with_state(|s| s.libraries.remove(&d));
                    app.message = Some((format!("{n} matière(s) enregistrée(s) dans {}", d.display()), 0.0));
                }
            }
            if ui.button("Purger les éléments inutilisés").clicked() {
                ui.close();
                purge_materials(app);
            }
            if ui.button("Actualiser").clicked() {
                ui.close();
                with_state(|s| s.libraries.clear());
            }
        });
    });
    if let Some(p) = pick {
        with_state(|s| s.collection = p);
    }
    if let Some(dir) = with_state(|s| s.collection.clone()) {
        let items =
            with_state(|s| s.libraries.entry(dir.clone()).or_insert_with(|| crate::skm::read_folder(&dir)).clone());
        if items.is_empty() {
            ui.label(RichText::new("Aucune matière dans ce dossier.").italics());
        }
        let cur_name = current.map(|id| app.doc.model.materials[id].name.clone());
        let mut clicked = None;
        egui::ScrollArea::vertical().max_height(320.0).id_salt("materials_library").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (i, m) in items.iter().enumerate() {
                    let selected = cur_name.as_deref() == Some(m.name.as_str());
                    if swatch(ui, Some(m), 32.0, selected).on_hover_text(&m.name).clicked() {
                        clicked = Some(i);
                    }
                }
            });
        });
        if let Some(i) = clicked {
            use_library_material(app, &items[i]);
        }
        return;
    }
    let ids = listed(&app.doc.model);
    ui.horizontal_wrapped(|ui| {
        if swatch(ui, None, 32.0, current.is_none()).on_hover_text("Défaut").clicked() {
            choose(app, None);
        }
        for id in ids {
            let m = app.doc.model.materials[id].clone();
            let r = swatch(ui, Some(&m), 32.0, current == Some(id)).on_hover_text(&m.name);
            if r.clicked() {
                choose(app, Some(id));
            }
            if r.double_clicked() {
                with_state(|s| s.edit_tab = true);
            }
            r.context_menu(|ui| material_menu(app, ui, id));
        }
    });
}

/// Where a material is used: faces (either side) and groups/components, in every collection.
fn painted_with(m: &Model, id: MaterialId) -> usize {
    let count = |e: &esq_core::model::Entities| {
        e.faces.values().filter(|f| f.front == Some(id) || f.back == Some(id)).count()
            + e.instances.values().filter(|i| i.material == Some(id)).count()
    };
    count(&m.root) + m.defs.values().map(|d| count(&d.entities)).sum::<usize>()
}

/// The right-click menu of a material of the model: Delete, Save As, Export
/// Texture Image, Edit Texture Image..., then Area and Select (greyed while it is unused).
fn material_menu(app: &mut EsquisseApp, ui: &mut egui::Ui, id: MaterialId) {
    let Some(m) = app.doc.model.materials.get(id).cloned() else { return };
    let used = painted_with(&app.doc.model, id) > 0;
    let textured = m.ext.texture.is_some();
    if ui.button("Supprimer").clicked() {
        ui.close();
        delete_material(app, id);
    }
    if ui.button("Enregistrer sous").clicked() {
        ui.close();
        if let Some(p) = crate::settings::dialog("materials")
            .add_filter("Matière SketchUp (*.skm)", &["skm"])
            .set_file_name(format!("{}.skm", m.name))
            .save_file()
            && let Err(e) = std::fs::write(&p, crate::skm::write(&m, None))
        {
            app.message = Some((format!("Échec de l'enregistrement : {e}"), 0.0));
        }
    }
    if ui.add_enabled(textured, egui::Button::new("Exporter l'image de texture")).clicked() {
        ui.close();
        if let Some(t) = &m.ext.texture {
            let ext = Path::new(&t.filename).extension().and_then(|e| e.to_str()).unwrap_or("png").to_string();
            if let Some(p) = crate::settings::dialog("textures")
                .add_filter("Image", &[ext.as_str()])
                .set_file_name(if t.filename.is_empty() { format!("{}.{ext}", m.name) } else { t.filename.clone() })
                .save_file()
                && let Err(e) = std::fs::write(&p, &t.image.0[..])
            {
                app.message = Some((format!("Échec de l'export : {e}"), 0.0));
            }
        }
    }
    if ui.add_enabled(textured, egui::Button::new("Modifier l'image de texture...")).clicked() {
        ui.close();
        edit_externally(app, id);
    }
    ui.separator();
    if ui.add_enabled(used, egui::Button::new("Surface")).clicked() {
        ui.close();
        let a = material_area(&app.doc.model, id);
        app.message = Some((format!("Surface : {}", esq_core::units::format_area(a, app.doc.model.units)), 0.0));
    }
    if ui.add_enabled(used, egui::Button::new("Sélectionner")).clicked() {
        ui.close();
        select_painted(app, id);
    }
}

/// Materials › Delete: what it paints goes back to the default material.
pub fn delete_material(app: &mut EsquisseApp, id: MaterialId) {
    app.doc.commit("Supprimer la matière", |m, _| {
        let forget = |e: &mut esq_core::model::Entities| {
            for f in e.faces.values_mut() {
                if f.front == Some(id) {
                    f.front = None;
                }
                if f.back == Some(id) {
                    f.back = None;
                }
            }
            for i in e.instances.values_mut() {
                if i.material == Some(id) {
                    i.material = None;
                }
            }
        };
        forget(&mut m.root);
        for d in m.defs.values_mut() {
            forget(&mut d.entities);
        }
        if let Some(x) = m.materials.remove(id) {
            m.attr_store.remove(&format!("mat:{}", x.name));
        }
        if m.meta.current_material == Some(id) {
            m.meta.current_material = None;
        }
    });
    if app.material == Some(id) {
        app.material = None;
    }
}

/// Materials › Area: the area of every face painted with it (on either side), placed in the
/// model. Each definition is measured once (its copies scale it by their own size), so a
/// model of many nested copies costs no more than its definitions.
pub fn material_area(m: &Model, id: MaterialId) -> f64 {
    fn own(e: &esq_core::model::Entities, id: MaterialId) -> f64 {
        e.faces
            .iter()
            .filter(|(_, f)| f.front == Some(id) || f.back == Some(id))
            .map(|(k, _)| esq_core::tess::face_area(e, k))
            .sum()
    }
    /// Area inside a collection, in its own coordinates; a copy's scale is the cube root of
    /// its transformation's determinant.
    fn inside(
        m: &Model,
        e: &esq_core::model::Entities,
        id: MaterialId,
        memo: &mut HashMap<esq_core::model::DefId, f64>,
        depth: usize,
    ) -> f64 {
        let mut a = own(e, id);
        if depth > 64 {
            return a;
        }
        for i in e.instances.values() {
            let s = i.xform.determinant().abs().cbrt();
            let d = match memo.get(&i.def) {
                Some(v) => *v,
                None => {
                    memo.insert(i.def, 0.0);
                    let v = m.defs.get(i.def).map_or(0.0, |d| inside(m, &d.entities, id, memo, depth + 1));
                    memo.insert(i.def, v);
                    v
                }
            };
            a += d * s * s;
        }
        a
    }
    inside(m, &m.root, id, &mut HashMap::new(), 0)
}

/// Materials › Select: what is painted with it in the open context (faces, groups and
/// components).
pub fn select_painted(app: &mut EsquisseApp, id: MaterialId) {
    let e = app.doc.ents();
    let mut sel: Vec<EntityRef> = e
        .faces
        .iter()
        .filter(|(_, f)| f.front == Some(id) || f.back == Some(id))
        .map(|(k, _)| EntityRef::Face(k))
        .collect();
    sel.extend(e.instances.iter().filter(|(_, i)| i.material == Some(id)).map(|(k, _)| EntityRef::Instance(k)));
    app.doc.select(sel);
}

/// Applies a change at once, as one step: or, while the mouse is down (a drag), live
/// without history, the step being made when the drag ends ([`finish_live`]).
fn change(app: &mut EsquisseApp, ui: &egui::Ui, id: MaterialId, f: impl FnOnce(&mut Material)) {
    if ui.input(|i| i.pointer.any_down()) {
        with_state(|s| {
            if s.live.as_ref().is_none_or(|l| l.0 != id) {
                s.live = Some((id, app.doc.model.materials[id].clone()));
            }
        });
        f(&mut app.doc.model.materials[id]);
        app.doc.rev += 1;
        app.doc.model_rev += 1;
    } else {
        app.doc.commit("Modifier la matière", |m, _| f(&mut m.materials[id]));
    }
}

fn finish_live(app: &mut EsquisseApp, ui: &egui::Ui) {
    if ui.input(|i| i.pointer.any_down()) {
        return;
    }
    let Some((id, before)) = with_state(|s| s.live.take()) else { return };
    let Some(now) = app.doc.model.materials.get(id).cloned() else { return };
    app.doc.model.materials[id] = before;
    app.doc.commit("Modifier la matière", |m, _| m.materials[id] = now);
}

fn edit_tab(app: &mut EsquisseApp, ui: &mut egui::Ui, id: MaterialId) {
    let m = app.doc.model.materials[id].clone();
    let units = app.doc.model.units;

    // --- Colour.
    ui.horizontal(|ui| {
        ui.label("Couleur :");
        let mut mode = with_state(|s| s.mode);
        egui::ComboBox::from_id_salt("mode-couleur").selected_text(mode.label()).show_ui(ui, |ui| {
            for c in ColorMode::ALL {
                ui.selectable_value(&mut mode, c, c.label());
            }
        });
        with_state(|s| s.mode = mode);
    });
    let mode = with_state(|s| s.mode);
    if let Some(c) = color_controls(ui, mode, m.color) {
        change(app, ui, id, |mat| set_color(mat, c));
    }
    ui.horizontal(|ui| {
        if icon_button(ui, ToolId::SampleMaterial, false)
            .on_hover_text("Faire correspondre avec la couleur de l’objet du modèle")
            .clicked()
        {
            app.cancel_tool();
            app.tool = Box::new(crate::tools::sample::SampleTool::for_color(id, app.tool.id()));
        }
        let armed = with_state(|s| s.screen_pick.is_some());
        if screen_button(ui, armed).on_hover_text(SCREEN_PICK).clicked() {
            with_state(|s| s.screen_pick = if armed { None } else { Some(ScreenPick::Armed(id)) });
        }
        let start = with_state(|s| s.start.as_ref().filter(|x| x.0 == id).map(|x| x.1.clone()));
        let changed =
            start.as_ref().is_some_and(|s| s.color != m.color || s.ext.colorize_deltas != m.ext.colorize_deltas);
        if ui.add_enabled(changed, egui::Button::new("↶")).on_hover_text("Annuler les changements de couleur").clicked()
            && let Some(s) = start
        {
            change(app, ui, id, |mat| {
                mat.color = s.color;
                mat.ext.colorize_deltas = s.ext.colorize_deltas;
                mat.ext.colorize_type = s.ext.colorize_type;
            });
        }
    });
    ui.separator();

    // --- Texture.
    let mut use_tex = m.ext.texture.is_some();
    if ui.checkbox(&mut use_tex, "Utiliser l'image de texture").changed() {
        if use_tex {
            browse_texture(app, id);
        } else {
            app.doc.commit("Modifier la matière", |md, _| remove_texture(&mut md.materials[id]));
        }
    }
    if let Some(t) = &m.ext.texture {
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(RichText::new(&t.filename).monospace()).truncate());
            if ui.button("📂").on_hover_text("Rechercher le fichier d’image de la matière").clicked() {
                browse_texture(app, id);
            }
            if ui.button("🖊").on_hover_text("Modifier l'image de texture dans un éditeur externe").clicked() {
                edit_externally(app, id);
            }
        });
        texture_size(app, ui, id, t, units);
        ui.horizontal(|ui| {
            let mut tint = m.ext.colorize_type == 1;
            if ui
                .checkbox(&mut tint, "Coloriser")
                .on_hover_text("Forcer toutes les couleurs de texture sur la même nuance")
                .changed()
            {
                change(app, ui, id, |mat| set_colorize(mat, tint));
            }
            let colorized = m.ext.colorize_deltas != [0.0; 3];
            if ui
                .add_enabled(colorized, egui::Button::new("Réinitialiser la couleur"))
                .on_hover_text("Rétablir la couleur d’image d’origine")
                .clicked()
            {
                change(app, ui, id, reset_color);
            }
        });
    }
    ui.separator();

    // --- Opacity.
    ui.horizontal(|ui| {
        ui.label("Opacité");
        let mut o = (m.opacity * 100.0).round() as i32;
        let r = ui.add(egui::Slider::new(&mut o, 0..=100));
        if r.changed() {
            change(app, ui, id, |mat| mat.opacity = o as f32 / 100.0);
        }
    });
    ui.separator();

    // --- PBR.
    for map in PbrMap::ALL {
        let mut ext = m.ext.clone();
        let (on, factor, tex) = map.parts(&mut ext);
        let (on, factor, has) = (*on, *factor, tex.is_some());
        egui::CollapsingHeader::new(map.title()).id_salt(map.title()).default_open(on).show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut v = on;
                if ui.checkbox(&mut v, "").on_hover_text(map.enable_tip()).changed() {
                    change(app, ui, id, |mat| set_pbr_enabled(mat, map, v));
                }
                let mut f = factor;
                let r = ui.add_enabled(on, egui::Slider::new(&mut f, map.range())).on_hover_text(map.factor_tip());
                if r.changed() {
                    change(app, ui, id, |mat| *map.parts(&mut mat.ext).1 = f);
                }
            });
            ui.horizontal(|ui| {
                let label = if has { map.replace_label() } else { map.add_label() };
                if ui.button(if has { "Remplacer" } else { "Ajouter" }).on_hover_text(label).clicked()
                    && let Some(p) =
                        crate::settings::dialog("textures").add_filter("Images", IMAGE_EXTENSIONS).pick_file()
                {
                    match load_texture(&p) {
                        Ok(t) => app
                            .doc
                            .commit("Modifier la matière", |md, _| set_pbr_map(&mut md.materials[id], map, Some(t))),
                        Err(e) => app.message = Some((e, 0.0)),
                    }
                }
                if has && ui.button("🗑").on_hover_text(map.remove_label()).clicked() {
                    app.doc.commit("Modifier la matière", |md, _| set_pbr_map(&mut md.materials[id], map, None));
                }
                if map == PbrMap::Roughness {
                    // "Invert roughness map": a glossiness map.
                    let mut inv = m.ext.pbr.roughness_inverted;
                    if ui.checkbox(&mut inv, "Inverser").on_hover_text("Inverser la carte de rugosité").changed() {
                        change(app, ui, id, |mat| mat.ext.pbr.roughness_inverted = inv);
                    }
                }
                if map == PbrMap::Normal {
                    let dx = m.ext.pbr.normal_style == 1;
                    let tip = if dx { "Retourner au GL" } else { "Retourner à DX" };
                    if ui.button(if dx { "DX" } else { "GL" }).on_hover_text(tip).clicked() {
                        change(app, ui, id, |mat| mat.ext.pbr.normal_style = 1 - mat.ext.pbr.normal_style.min(1));
                    }
                }
            });
        });
    }
    ui.separator();

    // --- Hatch pattern (section cuts).
    let pats: Vec<(u32, String)> = app.doc.model.meta.hatch_patterns.iter().map(|h| (h.uid, h.name.clone())).collect();
    let cur = m.ext.hatch;
    let name = cur.and_then(|u| pats.iter().find(|p| p.0 == u)).map_or("Aucun".to_string(), |p| p.1.clone());
    let mut chosen = cur;
    egui::ComboBox::from_label("Motif de hachures").selected_text(name).show_ui(ui, |ui| {
        ui.selectable_value(&mut chosen, None, "Aucun");
        for (u, n) in &pats {
            ui.selectable_value(&mut chosen, Some(*u), n);
        }
    });
    if chosen != cur {
        app.doc.commit("Définir le schéma de hachurage du matériau", |md, _| md.materials[id].ext.hatch = chosen);
    }
}

/// The colour controls of the chosen mode; `Some` when the colour was changed.
fn color_controls(ui: &mut egui::Ui, mode: ColorMode, c: [u8; 3]) -> Option<[u8; 3]> {
    let mut out = None;
    match mode {
        ColorMode::Wheel => {
            let mut col = Color32::from_rgb(c[0], c[1], c[2]);
            if egui::color_picker::color_picker_color32(ui, &mut col, egui::color_picker::Alpha::Opaque) {
                out = Some([col.r(), col.g(), col.b()]);
            }
        }
        ColorMode::Hls => {
            let [h, s, l] = hls(c.map(|v| v as f64 / 255.0));
            let (mut h, mut l, mut s) = (h, l * 100.0, s * 100.0);
            let mut ch = false;
            ch |= ui.add(egui::Slider::new(&mut h, 0.0..=360.0).text("H")).changed();
            ch |= ui.add(egui::Slider::new(&mut l, 0.0..=100.0).text("L")).changed();
            ch |= ui.add(egui::Slider::new(&mut s, 0.0..=100.0).text("S")).changed();
            if ch {
                out = Some(rgb([h, s / 100.0, l / 100.0]).map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8));
            }
        }
        ColorMode::Hsb => {
            let hsv = egui::ecolor::Hsva::from_srgb(c);
            let (mut h, mut s, mut b) = (hsv.h as f64 * 360.0, hsv.s as f64 * 100.0, hsv.v as f64 * 100.0);
            let mut ch = false;
            ch |= ui.add(egui::Slider::new(&mut h, 0.0..=360.0).text("H")).changed();
            ch |= ui.add(egui::Slider::new(&mut s, 0.0..=100.0).text("S")).changed();
            ch |= ui.add(egui::Slider::new(&mut b, 0.0..=100.0).text("B")).changed();
            if ch {
                let v = egui::ecolor::Hsva::new((h / 360.0) as f32, (s / 100.0) as f32, (b / 100.0) as f32, 1.0);
                out = Some(v.to_srgb());
            }
        }
        ColorMode::Rgb => {
            let mut v = c;
            let mut ch = false;
            for (i, n) in ["R", "G", "B"].iter().enumerate() {
                ch |= ui.add(egui::Slider::new(&mut v[i], 0..=255).text(*n)).changed();
            }
            if ch {
                out = Some(v);
            }
        }
    }
    out
}

fn texture_size(
    app: &mut EsquisseApp,
    ui: &mut egui::Ui,
    id: MaterialId,
    t: &TextureData,
    units: esq_core::model::Units,
) {
    let (mut w, mut h) = with_state(|s| match &s.size_text {
        Some((m, w, h)) if *m == id => (w.clone(), h.clone()),
        _ => (format_length(t.size[0], units), format_length(t.size[1], units)),
    });
    let lock = with_state(|s| s.lock_aspect);
    let mut set = None;
    egui::Grid::new("taille-texture").num_columns(3).show(ui, |ui| {
        ui.label("↔");
        let rw = ui.add_sized([120.0, 20.0], egui::TextEdit::singleline(&mut w));
        let mut l = lock;
        let icon = if l { "🔗" } else { "⛓" };
        if ui.toggle_value(&mut l, icon).on_hover_text("Verrouiller/déverrouiller les proportions").changed() {
            with_state(|s| s.lock_aspect = l);
        }
        ui.end_row();
        ui.label("↕");
        let rh = ui.add_sized([120.0, 20.0], egui::TextEdit::singleline(&mut h));
        let start = with_state(|s| {
            s.start.as_ref().filter(|x| x.0 == id).and_then(|x| x.1.ext.texture.as_ref().map(|t| t.size))
        });
        let differs = start.is_some_and(|s| s != t.size);
        if ui
            .add_enabled(differs, egui::Button::new("↶"))
            .on_hover_text("Annuler les modifications de largeur/hauteur")
            .clicked()
            && let Some(s) = start
        {
            set = Some((Some(s[0]), Some(s[1]), false));
        }
        ui.end_row();
        if rw.lost_focus()
            && let Some(v) = parse_length(&w, units).filter(|v| *v > 0.0)
        {
            set = Some((Some(v), None, lock));
        }
        if rh.lost_focus()
            && let Some(v) = parse_length(&h, units).filter(|v| *v > 0.0)
        {
            set = Some((None, Some(v), lock));
        }
        let typing = rw.has_focus() || rh.has_focus();
        with_state(|s| s.size_text = if typing { Some((id, w.clone(), h.clone())) } else { None });
    });
    if let Some((w, h, lock)) = set {
        app.doc.commit("Modifier la matière", |md, _| {
            let m = &mut md.materials[id];
            match (w, h) {
                (Some(w), Some(h)) => {
                    if let Some(t) = m.ext.texture.as_mut() {
                        t.size = [w, h];
                    }
                }
                _ => set_texture_size(m, w, h, lock),
            }
        });
    }
}

/// The image files SketchUp's "Choose Image" accepts and Esquisse can read.
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp", "tga", "tif", "tiff"];

fn browse_texture(app: &mut EsquisseApp, id: MaterialId) {
    let Some(p) = crate::settings::dialog("textures")
        .set_title("Choisir l'image")
        .add_filter("Images", IMAGE_EXTENSIONS)
        .pick_file()
    else {
        return;
    };
    match load_texture(&p) {
        Ok(t) => app.doc.commit("Modifier la matière", |md, _| set_texture(&mut md.materials[id], t)),
        Err(e) => app.message = Some((format!("Impossible de charger le fichier image « {} ». {e}", p.display()), 0.0)),
    }
}

/// "Edit texture image in external editor": the image goes to a temporary file opened with
/// the system's editor for its type; the material takes it back each time it is saved.
pub fn edit_externally(app: &mut EsquisseApp, id: MaterialId) {
    let Some(t) = app.doc.model.materials[id].ext.texture.clone() else { return };
    let dir = std::env::temp_dir().join("esquisse-textures");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(esq_core::meta::base_name(&t.filename));
    if std::fs::write(&path, &t.image.0[..]).is_err() {
        return;
    }
    let when = std::fs::metadata(&path).and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    with_state(|s| s.external = Some((id, path.clone(), when)));
    // Preferences › Applications › Default Image Editor (else Windows' program for the type).
    crate::settings::open_image(&path);
}

fn reload_external(app: &mut EsquisseApp) {
    let Some((id, path, when)) = with_state(|s| s.external.clone()) else { return };
    let Ok(now) = std::fs::metadata(&path).and_then(|m| m.modified()) else { return };
    if now <= when || !app.doc.model.materials.contains_key(id) {
        return;
    }
    with_state(|s| s.external = Some((id, path.clone(), now)));
    if let Ok(t) = load_texture(&path) {
        let size = app.doc.model.materials[id].ext.texture.as_ref().map(|x| x.size);
        app.doc.commit("Modifier l'image de texture", |md, _| {
            let m = &mut md.materials[id];
            m.ext.texture = Some(TextureData { size: size.unwrap_or(t.size), ..t });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn textured(avg: [u8; 3]) -> Material {
        Material {
            name: "Brique".into(),
            color: avg,
            opacity: 1.0,
            ext: MaterialExt {
                texture: Some(TextureData {
                    filename: "b.png".into(),
                    image: Blob::default(),
                    px: [200, 100],
                    size: [400.0, 200.0],
                    avg,
                }),
                ..Default::default()
            },
        }
    }

    #[test]
    fn colour_of_a_textured_material_colorizes_it() {
        let mut m = textured([200, 0, 0]);
        set_color(&mut m, [0, 200, 0]);
        assert_eq!(m.color, [0, 200, 0]);
        assert!((m.ext.colorize_deltas[0] - 120.0).abs() < 1e-9, "{:?}", m.ext.colorize_deltas);
        set_colorize(&mut m, true);
        assert_eq!(m.ext.colorize_type, 1);
        reset_color(&mut m);
        assert_eq!((m.color, m.ext.colorize_deltas, m.ext.colorize_type), ([200, 0, 0], [0.0; 3], 0));
        // A plain colour is just a colour.
        let mut p = Material { name: "P".into(), color: [1, 2, 3], opacity: 1.0, ext: Default::default() };
        set_color(&mut p, [9, 9, 9]);
        assert_eq!((p.color, p.ext.colorize_deltas), ([9, 9, 9], [0.0; 3]));
    }

    #[test]
    fn texture_size_follows_the_lock_and_new_images_keep_the_larger_side() {
        let mut m = textured([10, 10, 10]);
        set_texture_size(&mut m, Some(800.0), None, true);
        assert_eq!(m.ext.texture.as_ref().unwrap().size, [800.0, 400.0]);
        set_texture_size(&mut m, None, Some(100.0), false);
        assert_eq!(m.ext.texture.as_ref().unwrap().size, [800.0, 100.0]);
        // A square picture: the larger side (800) stays.
        let mut sq = m.ext.texture.clone().unwrap();
        sq.px = [50, 50];
        set_texture(&mut m, sq);
        assert_eq!(m.ext.texture.as_ref().unwrap().size, [800.0, 800.0]);
        remove_texture(&mut m);
        assert!(m.ext.texture.is_none());
    }

    #[test]
    fn a_first_image_gives_its_average_colour_and_one_inch_per_pixel() {
        let png = crate::photo_img::tests::png_bytes(4, 2);
        let t = texture_from_bytes("dossier/brique.png", png).unwrap();
        assert_eq!((t.filename.as_str(), t.px, t.size), ("brique.png", [4, 2], [101.6, 50.8]));
        let mut m = Material { name: "M".into(), color: [1, 1, 1], opacity: 1.0, ext: Default::default() };
        let avg = t.avg;
        set_texture(&mut m, t);
        assert_eq!(m.color, avg);
    }

    #[test]
    fn pbr_maps_and_workflow() {
        let mut m = textured([10, 10, 10]);
        set_pbr_enabled(&mut m, PbrMap::Normal, true);
        assert!(!m.ext.pbr.normal_enabled, "pas de carte normale : refusé");
        set_pbr_enabled(&mut m, PbrMap::Roughness, true);
        assert_eq!(m.ext.workflow, 1);
        let map = m.ext.texture.clone();
        set_pbr_map(&mut m, PbrMap::Normal, map);
        assert!(m.ext.pbr.normal_enabled && m.ext.pbr.normal_texture.is_some());
        set_pbr_map(&mut m, PbrMap::Normal, None);
        assert!(!m.ext.pbr.normal_enabled);
        set_pbr_enabled(&mut m, PbrMap::Roughness, false);
        assert_eq!(m.ext.workflow, 0);
    }

    #[test]
    fn library_materials_are_listed_by_collection_and_painted_from() {
        let c = collections();
        // The collections of the library that comes with Esquisse, in French.
        assert!(c.iter().any(|(n, _)| n == "Brique et parement"));
        assert!(c.iter().any(|(n, _)| n == "Couleurs unies"));
        let (_, wood) = c.iter().find(|(n, _)| n == "Bois").expect("collection Bois");
        let items = crate::skm::read_folder(wood);
        assert_eq!(items.len(), 6);
        let mut app = EsquisseApp::bare(None, None);
        let id = use_library_material(&mut app, &items[0]);
        assert_eq!(app.material, Some(id));
        assert!(app.doc.model.materials[id].ext.texture.is_some());
        // Chosen again: the model's material, not a copy.
        assert_eq!(use_library_material(&mut app, &items[0]), id);
        assert_eq!(listed(&app.doc.model).len(), 1);
        // Undo takes it out of the model again.
        app.doc.undo();
        assert!(listed(&app.doc.model).is_empty());
    }

    #[test]
    fn create_material_copies_the_current_one() {
        let mut model = Model::new();
        let a = model.add_material("Brique", [1, 2, 3]);
        let b = create_material(&mut model, Some(a));
        assert_eq!((model.materials[b].name.as_str(), model.materials[b].color), ("Brique1", [1, 2, 3]));
        let c = create_material(&mut model, None);
        assert_eq!(model.materials[c].name, "Matière");
        assert_eq!(listed(&model), vec![a, b, c]);
    }
}

// ---------------------------------------------------------------------------------------
// "Match color on screen" (CMaterialEditPage): the checkable button sets
// the eyedropper cursor and grabs the mouse; the next left press anywhere
// takes the colour of the screen pixel under it, opaque, as the material's colour, and lets
// the button go. Here the pixel is read from the image of the window.

/// The image of the window asked for by a pick, once the integration has read it.
static SHOT: std::sync::Mutex<Option<Arc<egui::ColorImage>>> = std::sync::Mutex::new(None);

const SCREEN_PICK: &str = "Faire correspondre à la couleur à l’écran";

/// Where a screen colour pick is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScreenPick {
    /// The button is down: the next left press picks.
    Armed(MaterialId),
    /// Pressed there (points): the image of the window has been asked for.
    Waiting(MaterialId, egui::Pos2),
}

/// The colour of the pixel at `pos` (points) of a window image at `ppp` pixels per point.
pub fn pixel_at(img: &egui::ColorImage, pos: egui::Pos2, ppp: f32) -> Option<[u8; 3]> {
    let (x, y) = ((pos.x * ppp).floor(), (pos.y * ppp).floor());
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let ([w, h], x, y) = (img.size, x as usize, y as usize);
    (x < w && y < h).then(|| {
        let c = img.pixels[y * w + x];
        [c.r(), c.g(), c.b()]
    })
}

/// One step of a pick: a press while armed asks for the image of the window (`true`); the
/// image, once there, gives the material and its new colour.
pub fn screen_pick_step(
    st: &mut Option<ScreenPick>,
    press: Option<egui::Pos2>,
    image: Option<(&egui::ColorImage, f32)>,
) -> (bool, Option<(MaterialId, [u8; 3])>) {
    match *st {
        Some(ScreenPick::Armed(id)) => {
            if let Some(p) = press {
                *st = Some(ScreenPick::Waiting(id, p));
                return (true, None);
            }
        }
        Some(ScreenPick::Waiting(id, p)) => {
            if let Some((img, ppp)) = image {
                *st = None;
                return (false, pixel_at(img, p, ppp).map(|c| (id, c)));
            }
        }
        None => {}
    }
    (false, None)
}

/// The pick in progress (tests).
#[cfg(test)]
pub fn screen_pick_state() -> Option<ScreenPick> {
    with_state(|s| s.screen_pick)
}

/// Hands the image of the window over as the integration does (tests).
#[cfg(test)]
pub fn give_screenshot(img: egui::ColorImage) {
    *SHOT.lock().unwrap() = Some(Arc::new(img));
}

/// The button: a screen with the eyedropper's target, pressed while picking.
fn screen_button(ui: &mut egui::Ui, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, SCREEN_PICK));
    let v = ui.style().interact_selectable(&resp, selected);
    let p = ui.painter();
    p.rect(rect, 3.0, v.weak_bg_fill, v.bg_stroke, egui::StrokeKind::Inside);
    let fg = v.fg_stroke.color;
    let screen = egui::Rect::from_min_max(rect.min + egui::vec2(4.0, 5.0), rect.max - egui::vec2(4.0, 8.0));
    p.rect_stroke(screen, 1.0, Stroke::new(1.5, fg), egui::StrokeKind::Middle);
    p.line_segment(
        [rect.center_bottom() - egui::vec2(4.0, 4.0), rect.center_bottom() + egui::vec2(4.0, -4.0)],
        Stroke::new(1.5, fg),
    );
    p.circle_stroke(screen.center(), 2.5, Stroke::new(1.2, Color32::from_rgb(200, 40, 40)));
    resp
}

/// Every frame the panel shows: while armed the whole window takes the next press (with the
/// crosshair cursor), then the colour comes from the image of the window.
fn screen_pick_frame(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(st) = with_state(|s| s.screen_pick) else { return };
    let mut press = None;
    if let ScreenPick::Armed(_) = st {
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::unique("couleur-a-l-ecran"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.allocate_rect(screen, Sense::click_and_drag());
            });
        press = ctx.input(|i| if i.pointer.primary_pressed() { i.pointer.interact_pos() } else { None });
    }
    let image = SHOT.lock().ok().and_then(|mut s| s.take());
    let mut s = Some(st);
    let (ask, got) = screen_pick_step(&mut s, press, image.as_deref().map(|i| (i, ctx.pixels_per_point())));
    with_state(|x| x.screen_pick = s);
    if ask {
        let c = ctx.clone();
        ctx.request_screenshot(move |img| {
            if let Ok(mut s) = SHOT.lock() {
                *s = Some(img);
            }
            c.request_repaint();
        });
    }
    if let Some((id, c)) = got.filter(|(id, _)| app.doc.model.materials.contains_key(*id)) {
        app.doc.commit("Modifier la matière", |m, _| set_color(&mut m.materials[id], c));
    }
    ctx.request_repaint();
}
