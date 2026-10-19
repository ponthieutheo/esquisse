//! Window › Hatch Patterns (SketchUp's `HatchPatternBrowserPage`, `HatchPatternEditPage`,
//! `CEditHatchPatternDlg`): the patterns of the model as thumbnails (or small thumbnails),
//! Create New Hatch Pattern (from a .pat or an image), Import (.skh), Duplicate, Delete,
//! Purge Unused, Save As (.skh), Apply to the selected groups and components; and the
//! selected pattern's settings: name, colour, pattern or image file (browse, replace,
//! delete), rotation angle, x and y scales with their lock.

use std::cell::RefCell;
use std::collections::HashMap;

use eframe::egui::{self, Color32, RichText, Sense, Stroke};
use esq_core::hatch::{natural_extent, parse_pat, picture};
use esq_core::meta::HatchPattern;
use esq_core::model::{Entities, EntityRef, Model};

use crate::app::EsquisseApp;

struct State {
    selected: Option<u32>,
    small: bool,
    lock_scale: bool,
    name: Option<(u32, String)>,
    error: Option<String>,
    thumbs: HashMap<(u32, u64), egui::TextureHandle>,
    /// A slider being dragged: the pattern before it.
    live: Option<HatchPattern>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        selected: None,
        small: false,
        lock_scale: true,
        name: None,
        error: None,
        thumbs: HashMap::new(),
        live: None,
    });
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

// ---------------------------------------------------------------------------------------
// What the commands do to the model.

fn names(m: &Model) -> Vec<String> {
    m.meta.hatch_patterns.iter().map(|h| h.name.clone()).collect()
}

fn find(m: &mut Model, uid: u32) -> Option<&mut HatchPattern> {
    m.meta.hatch_patterns.iter_mut().find(|h| h.uid == uid)
}

/// A pattern from a .pat or an image file; named after the .pat's pattern or the file.
pub fn create(m: &mut Model, file_name: &str, bytes: Vec<u8>) -> Result<u32, String> {
    let image = esq_core::hatch::looks_like_image(&bytes);
    let stem =
        std::path::Path::new(file_name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let base = if image {
        stem
    } else {
        let (n, _) = parse_pat(&String::from_utf8_lossy(&bytes))?;
        if n.is_empty() { stem } else { n }
    };
    let base = if base.trim().is_empty() { "Motif de hachures".to_string() } else { base };
    let name = Model::unique_name(&base, &names(m));
    let uid = m.new_uid();
    m.meta.hatch_patterns.push(HatchPattern::new(uid, &name, file_name, bytes));
    Ok(uid)
}

/// Import Hatch Pattern (.skh).
pub fn import(m: &mut Model, bytes: &[u8]) -> Result<u32, String> {
    let uid = m.new_uid();
    let mut h = crate::skh::read(bytes, uid)?;
    h.name = Model::unique_name(&h.name, &names(m));
    m.meta.hatch_patterns.push(h);
    Ok(uid)
}

pub fn duplicate(m: &mut Model, uid: u32) -> Option<u32> {
    let mut h = m.meta.hatch_patterns.iter().find(|h| h.uid == uid)?.clone();
    h.name = Model::unique_name(&h.name, &names(m));
    h.uid = m.new_uid();
    let new = h.uid;
    m.meta.hatch_patterns.push(h);
    Some(new)
}

/// Renames a pattern; the groups and components that use it follow.
pub fn rename(m: &mut Model, uid: u32, name: &str) -> Result<(), String> {
    let name = name.trim();
    let old = m.meta.hatch_patterns.iter().find(|h| h.uid == uid).map(|h| h.name.clone()).ok_or("motif disparu")?;
    if name.is_empty() || m.meta.hatch_patterns.iter().any(|h| h.uid != uid && h.name == name) {
        return Err("Le nom du motif est vide ou existe déjà".into());
    }
    for_instances(m, &mut |e| {
        for i in e.instances.values_mut() {
            if i.ext.hatch.as_deref() == Some(old.as_str()) {
                i.ext.hatch = Some(name.to_string());
            }
        }
    });
    find(m, uid).ok_or("motif disparu")?.name = name.to_string();
    Ok(())
}

fn for_instances(m: &mut Model, f: &mut dyn FnMut(&mut Entities)) {
    f(&mut m.root);
    for d in m.defs.values_mut() {
        f(&mut d.entities);
    }
}

/// Delete Hatch Pattern: materials and groups that used it have none any more.
pub fn remove(m: &mut Model, uid: u32) {
    let Some(name) = m.meta.hatch_patterns.iter().find(|h| h.uid == uid).map(|h| h.name.clone()) else { return };
    for mat in m.materials.values_mut() {
        if mat.ext.hatch == Some(uid) {
            mat.ext.hatch = None;
        }
    }
    for_instances(m, &mut |e| {
        for i in e.instances.values_mut() {
            if i.ext.hatch.as_deref() == Some(name.as_str()) {
                i.ext.hatch = None;
            }
        }
    });
    m.meta.hatch_patterns.retain(|h| h.uid != uid);
}

/// Purge Unused Hatch Patterns: those no material, group or component uses.
pub fn purge(m: &mut Model) -> usize {
    let mut used: Vec<String> = Vec::new();
    let mut note = |e: &Entities| used.extend(e.instances.values().filter_map(|i| i.ext.hatch.clone()));
    note(&m.root);
    for d in m.defs.values() {
        note(&d.entities);
    }
    let by_mat: Vec<u32> = m.materials.values().filter_map(|x| x.ext.hatch).collect();
    let before = m.meta.hatch_patterns.len();
    m.meta.hatch_patterns.retain(|h| by_mat.contains(&h.uid) || used.contains(&h.name));
    before - m.meta.hatch_patterns.len()
}

/// Apply Hatch Pattern to the selected groups and components (their section cuts).
pub fn apply(m: &mut Model, def: Option<esq_core::model::DefId>, sel: &[EntityRef], uid: u32) -> usize {
    let Some(name) = m.meta.hatch_patterns.iter().find(|h| h.uid == uid).map(|h| h.name.clone()) else { return 0 };
    let ents = m.entities_mut(def);
    let mut n = 0;
    for e in sel {
        if let EntityRef::Instance(i) = e
            && let Some(x) = ents.instances.get_mut(*i)
        {
            x.ext.hatch = Some(name.clone());
            n += 1;
        }
    }
    n
}

/// A picture (RGBA, `side` pixels) of a pattern with its settings.
pub fn thumbnail(h: &HatchPattern, side: u32) -> Vec<u8> {
    if h.image {
        if let Ok(img) = crate::photo_img::decode(&h.data.0) {
            let img = img.to_rgba8();
            return image::imageops::resize(&img, side, side, image::imageops::FilterType::Triangle).into_raw();
        }
        return vec![200; (side * side * 4) as usize];
    }
    match parse_pat(&String::from_utf8_lossy(&h.data.0)) {
        Ok((_, lines)) => {
            let extent = natural_extent(&lines) * h.size[0].max(h.size[1]).max(1e-9);
            picture(&lines, h.color, h.rotation, h.size, side, extent)
        }
        Err(_) => h.color.iter().copied().chain([255]).cycle().take((side * side * 4) as usize).collect(),
    }
}

// ---------------------------------------------------------------------------------------
// The panel.

fn key(h: &HatchPattern) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut s = std::collections::hash_map::DefaultHasher::new();
    (std::sync::Arc::as_ptr(&h.data.0) as usize).hash(&mut s);
    h.color.hash(&mut s);
    h.rotation.to_bits().hash(&mut s);
    h.size.map(f64::to_bits).hash(&mut s);
    h.image.hash(&mut s);
    s.finish()
}

fn texture(ui: &egui::Ui, h: &HatchPattern) -> egui::TextureHandle {
    let k = (h.uid, key(h));
    if let Some(t) = with_state(|s| s.thumbs.get(&k).cloned()) {
        return t;
    }
    let side = 64;
    let px = thumbnail(h, side);
    let t = crate::gpu_img::load_texture(
        ui.ctx(),
        format!("hachures-{}", h.uid),
        [side as usize, side as usize],
        &px,
        egui::TextureOptions::LINEAR,
    );
    with_state(|s| {
        s.thumbs.retain(|(u, _), _| *u != h.uid);
        s.thumbs.insert(k, t.clone());
    });
    t
}

fn commit(app: &mut EsquisseApp, label: &str, f: impl FnOnce(&mut Model)) {
    app.doc.commit(label, |m, _| f(m));
}

pub fn show(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let pats = app.doc.model.meta.hatch_patterns.clone();
    let mut selected = with_state(|s| s.selected.filter(|u| pats.iter().any(|h| h.uid == *u)));
    if selected.is_none() {
        selected = pats.first().map(|h| h.uid);
        with_state(|s| s.selected = selected);
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button("+").on_hover_text("Créer un nouveau modèle de hachures").clicked() {
            let pick = rfd::FileDialog::new()
                .set_title("Choisir un motif ou une image")
                .add_filter(
                    "Tous les types de motifs de hachures pris en charge",
                    &["pat", "bmp", "jpg", "jpeg", "png", "tif", "tiff", "tga"],
                )
                .pick_file();
            if let Some(p) = pick {
                match std::fs::read(&p) {
                    Ok(bytes) => {
                        let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                        let mut made = None;
                        let r = app.doc.try_commit("Ajouter un motif de hachure", |m, _| {
                            made = Some(create(m, &name, bytes)?);
                            Ok::<(), String>(())
                        });
                        match r {
                            Ok(()) => with_state(|s| s.selected = made),
                            Err(e) => app.message = Some((e, 0.0)),
                        }
                    }
                    Err(e) => app.message = Some((e.to_string(), 0.0)),
                }
            }
        }
        if ui.button("Importer").on_hover_text("Importer un modèle de hachures").clicked()
            && let Some(p) =
                rfd::FileDialog::new().add_filter("Modèle de hachures SketchUp (*.skh)", &["skh"]).pick_file()
        {
            let bytes = std::fs::read(&p).unwrap_or_default();
            let mut made = None;
            let r = app.doc.try_commit("Ajouter un motif de hachure", |m, _| {
                made = Some(import(m, &bytes)?);
                Ok::<(), String>(())
            });
            match r {
                Ok(()) => with_state(|s| s.selected = made),
                Err(e) => app.message = Some((e, 0.0)),
            }
        }
        let has = selected.is_some();
        if ui
            .add_enabled(has, egui::Button::new("Dupliquer"))
            .on_hover_text("Dupliquer le motif de hachure sélectionné")
            .clicked()
            && let Some(u) = selected
        {
            let mut made = None;
            commit(app, "Dupliquer le motif de la hachure", |m| made = duplicate(m, u));
            with_state(|s| s.selected = made);
        }
        if ui.add_enabled(has, egui::Button::new("🗑")).on_hover_text("Supprimer le motif de hachures").clicked()
            && let Some(u) = selected
        {
            commit(app, "Supprimer le motif de hachures", |m| remove(m, u));
            with_state(|s| s.selected = None);
        }
        if ui
            .add_enabled(!pats.is_empty(), egui::Button::new("Purger"))
            .on_hover_text("Purger les modèles de hachures inutilisés")
            .clicked()
        {
            let mut n = 0;
            commit(app, "Purger les modèles de hachures inutilisés", |m| n = purge(m));
            app.message = Some((format!("{n} modèle(s) de hachures supprimé(s)"), 0.0));
        }
        if ui
            .add_enabled(has, egui::Button::new("💾"))
            .on_hover_text("Enregistrer le modèle de trappe sous...")
            .clicked()
            && let Some(h) = pats.iter().find(|h| Some(h.uid) == selected)
        {
            let file: String = h.name.chars().map(|c| if "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("Fichiers de modèles de trappes (*.skh)", &["skh"])
                .set_file_name(format!("{file}.skh"))
                .save_file()
            {
                let thumb = crate::skh::png(thumbnail(h, 128), 128);
                let r = crate::skh::write(h, &thumb).and_then(|b| std::fs::write(&p, b).map_err(|e| e.to_string()));
                if let Err(e) = r {
                    app.message = Some((e, 0.0));
                }
            }
        }
        let mut small = with_state(|s| s.small);
        if ui
            .toggle_value(&mut small, "▦")
            .on_hover_text(if small { "Vignettes" } else { "Petites vignettes" })
            .changed()
        {
            with_state(|s| s.small = small);
        }
    });
    ui.label(RichText::new("Dans le modèle").small().color(Color32::from_gray(110)));
    if pats.is_empty() {
        ui.label("Aucun modèle de hachures dans le modèle.");
        return;
    }
    let size = if with_state(|s| s.small) { 32.0 } else { 56.0 };
    ui.horizontal_wrapped(|ui| {
        for h in &pats {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected == Some(h.uid), &h.name));
            let t = texture(ui, h);
            ui.painter().image(
                t.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            let stroke = if selected == Some(h.uid) {
                Stroke::new(2.5, Color32::from_rgb(52, 110, 220))
            } else {
                Stroke::new(1.0, Color32::from_gray(160))
            };
            ui.painter().rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
            if resp.on_hover_text(&h.name).clicked() {
                with_state(|s| {
                    s.selected = Some(h.uid);
                    s.name = None;
                    s.error = None;
                });
            }
        }
    });
    let Some(h) = pats.iter().find(|h| Some(h.uid) == selected).cloned() else { return };
    ui.separator();
    edit(app, ui, h);
    finish_live(app, ui);
}

/// The selected pattern's settings (HatchPatternEditPage).
fn edit(app: &mut EsquisseApp, ui: &mut egui::Ui, h: HatchPattern) {
    let uid = h.uid;
    // Name.
    let mut name = with_state(|s| match &s.name {
        Some((u, n)) if *u == uid => n.clone(),
        _ => h.name.clone(),
    });
    ui.horizontal(|ui| {
        ui.label("Nom :");
        let r = ui.text_edit_singleline(&mut name);
        if r.changed() {
            with_state(|s| s.name = Some((uid, name.clone())));
        }
        if r.lost_focus() && name != h.name {
            let n = name.clone();
            let res = app.doc.try_commit("Renommer le motif de hachures", |m, _| rename(m, uid, &n));
            with_state(|s| {
                s.error = res.err();
                s.name = None;
            });
        }
    });
    if let Some(e) = with_state(|s| s.error.clone()) {
        ui.label(RichText::new(e).color(Color32::from_rgb(190, 40, 40)));
    }
    // Colour.
    ui.horizontal(|ui| {
        ui.label("Couleur");
        let mut c = h.color;
        if ui.color_edit_button_srgb(&mut c).changed() {
            change(app, ui, uid, |p| p.color = c);
        }
    });
    // Pattern or image file.
    ui.horizontal(|ui| {
        let what = if h.source.is_empty() { "(aucun fichier)".to_string() } else { h.source.clone() };
        ui.add(egui::Label::new(RichText::new(what).monospace()).truncate());
        if ui.button("📂").on_hover_text("Remplacer le motif ou le fichier image").clicked()
            && let Some(p) = rfd::FileDialog::new()
                .add_filter(
                    "Tous les types de motifs de hachures pris en charge",
                    &["pat", "bmp", "jpg", "jpeg", "png", "tif", "tiff", "tga"],
                )
                .pick_file()
        {
            let bytes = std::fs::read(&p).unwrap_or_default();
            let image = esq_core::hatch::looks_like_image(&bytes);
            if !image && let Err(e) = parse_pat(&String::from_utf8_lossy(&bytes)) {
                app.message = Some((e, 0.0));
            } else {
                let src = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                commit(app, "Modifier le modèle de hachures", |m| {
                    if let Some(x) = find(m, uid) {
                        x.data = esq_core::photo::Blob(std::sync::Arc::new(bytes));
                        x.image = image;
                        x.source = src;
                    }
                });
            }
        }
        if ui
            .add_enabled(!h.source.is_empty(), egui::Button::new("🗑"))
            .on_hover_text("Supprimer le motif ou le fichier image")
            .clicked()
        {
            commit(app, "Modifier le modèle de hachures", |m| {
                if let Some(x) = find(m, uid) {
                    x.data = Default::default();
                    x.image = false;
                    x.source.clear();
                }
            });
        }
    });
    // Rotation angle and scales.
    egui::Grid::new("reglages-hachures").num_columns(3).show(ui, |ui| {
        ui.label("Angle de rotation");
        let mut a = h.rotation;
        if ui.add(egui::Slider::new(&mut a, 0.0..=360.0).suffix("°")).changed() {
            change(app, ui, uid, |p| p.rotation = a);
        }
        ui.end_row();
        let lock = with_state(|s| s.lock_scale);
        for (i, label) in ["Échelle X", "Échelle Y"].iter().enumerate() {
            ui.label(*label);
            let mut v = h.size[i];
            if ui.add(egui::Slider::new(&mut v, 0.01..=100.0).logarithmic(true)).changed() && v > 0.0 {
                change(app, ui, uid, |p| {
                    let ratio = if p.size[i] > 0.0 { v / p.size[i] } else { 1.0 };
                    p.size[i] = v;
                    if lock {
                        p.size[1 - i] *= ratio;
                    }
                });
            }
            if i == 0 {
                let mut l = lock;
                let icon = if l { "🔗" } else { "⛓" };
                if ui.toggle_value(&mut l, icon).on_hover_text("Verrouiller/déverrouiller les proportions").changed() {
                    with_state(|s| s.lock_scale = l);
                }
            }
            ui.end_row();
        }
    });
    // Apply to the selected groups and components.
    let n = app.doc.sel.iter().filter(|e| matches!(e, EntityRef::Instance(_))).count();
    if ui.add_enabled(n > 0, egui::Button::new("Appliquer un motif de hachure")).clicked() {
        let sel = app.doc.sel.clone();
        app.doc.commit("Appliquer un motif de hachure", |m, d| {
            apply(m, d, &sel, uid);
        });
    }
}

/// A change: at once as one step, or live while the mouse is down (one step at the end).
fn change(app: &mut EsquisseApp, ui: &egui::Ui, uid: u32, f: impl FnOnce(&mut HatchPattern)) {
    if ui.input(|i| i.pointer.any_down()) {
        if with_state(|s| s.live.as_ref().is_none_or(|l| l.uid != uid))
            && let Some(h) = app.doc.model.meta.hatch_patterns.iter().find(|h| h.uid == uid).cloned()
        {
            with_state(|s| s.live = Some(h));
        }
        if let Some(h) = find(&mut app.doc.model, uid) {
            f(h);
        }
        app.doc.rev += 1;
    } else {
        commit(app, "Modifier le modèle de hachures", |m| {
            if let Some(h) = find(m, uid) {
                f(h);
            }
        });
    }
}

fn finish_live(app: &mut EsquisseApp, ui: &egui::Ui) {
    if ui.input(|i| i.pointer.any_down()) {
        return;
    }
    let Some(before) = with_state(|s| s.live.take()) else { return };
    let uid = before.uid;
    let Some(now) = app.doc.model.meta.hatch_patterns.iter().find(|h| h.uid == uid).cloned() else { return };
    if let Some(h) = find(&mut app.doc.model, uid) {
        *h = before;
    }
    commit(app, "Modifier le modèle de hachures", |m| {
        if let Some(h) = find(m, uid) {
            *h = now;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_duplicate_rename_remove_purge() {
        let mut m = Model::new();
        let a = create(&mut m, "ANSI31.pat", b"*ANSI31\n45, 0,0, 0,.125\n".to_vec()).unwrap();
        assert_eq!(m.meta.hatch_patterns[0].name, "ANSI31");
        assert!(create(&mut m, "rien.pat", b"pas un motif".to_vec()).is_err());
        let b = duplicate(&mut m, a).unwrap();
        assert_eq!(m.meta.hatch_patterns[1].name, "ANSI311");
        assert!(rename(&mut m, b, "ANSI31").is_err(), "nom déjà pris");
        assert!(rename(&mut m, b, "  ").is_err());
        rename(&mut m, b, "Béton").unwrap();
        let mat = m.add_material("Coupe", [9, 9, 9]);
        m.materials[mat].ext.hatch = Some(b);
        assert_eq!(purge(&mut m), 1, "ANSI31 ne sert à rien");
        remove(&mut m, b);
        assert!(m.meta.hatch_patterns.is_empty());
        assert_eq!(m.materials[mat].ext.hatch, None);
    }

    #[test]
    fn import_and_thumbnails() {
        let mut m = Model::new();
        let mut h = HatchPattern::new(3, "Croix", "CROSS.pat", b"*CROSS\n0, 0,0, .25,.25, .125,-.375\n".to_vec());
        h.color = [255, 255, 255];
        let skh = crate::skh::write(&h, &crate::skh::png(thumbnail(&h, 8), 8)).unwrap();
        let u = import(&mut m, &skh).unwrap();
        import(&mut m, &skh).unwrap();
        assert_eq!(names(&m), vec!["Croix".to_string(), "Croix1".to_string()]);
        let px = thumbnail(&m.meta.hatch_patterns[0], 32);
        assert!(px.chunks(4).any(|p| p[0] == 0) && px.chunks(4).any(|p| p[0] == 255), "traits noirs sur fond blanc");
        // Turning the pattern changes the picture.
        find(&mut m, u).unwrap().rotation = 45.0;
        assert_ne!(thumbnail(&m.meta.hatch_patterns[0], 32), px);
    }
}
