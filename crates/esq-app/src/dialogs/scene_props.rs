//! Scene properties (SketchUp's `PagePropertiesPage` in Window › Scenes, and `PageUpdateDlg`,
//! "Scene Update"): a scene's name, description, whether the animation shows it, and which
//! properties it saves — camera location, top-level hidden geometry, hidden objects, visible
//! tags, active section planes, style and fog, shadow settings, environment, axes location.
//! They are kept as Ruby's `Sketchup::Page` keeps them (`Scene::ext.props`: "flags" with the
//! `PAGE_USE_*` bits, "animate", "ro", "shadow", "hidden_ents", "sections", "env", "axes"),
//! so that scripts and the application see the same scene; going to a scene puts back only
//! what it saves.

use eframe::egui::{self, RichText};
use esq_core::glam::DMat4;
use esq_core::model::{DefId, Entities, Model, Scene};
use serde_json::{Value as J, json};
use slotmap::Key;

use super::{name_widget, text_field};
use crate::app::EsquisseApp;

pub const CAMERA: i64 = 0x1;
pub const RENDERING: i64 = 0x2;
pub const SHADOW: i64 = 0x4;
pub const AXES: i64 = 0x8;
pub const HIDDEN: i64 = 0x10;
pub const LAYERS: i64 = 0x20;
pub const SECTIONS: i64 = 0x40;
pub const HIDDEN_GEOMETRY: i64 = 0x80;
pub const HIDDEN_OBJECTS: i64 = 0x100;
pub const ENVIRONMENT: i64 = 0x200;
pub const STYLE: i64 = 0x400;
pub const ALL: i64 = 0xfff;

/// "Propriétés pour sauvegarder", in SketchUp's order: bits, label, tooltip.
pub const PROPS: [(i64, &str, &str); 9] = [
    (CAMERA, "Emplacement de la caméra", "Enregistrer l’orientation de la caméra avec la scène"),
    (
        HIDDEN_GEOMETRY,
        "Géométrie cachée de niveau supérieur",
        "Enregistrer l'état de la géométrie cachée non imbriquée dans les groupes/composants",
    ),
    (HIDDEN_OBJECTS, "Objets masqués", "Enregistrer l’état des entités cachées"),
    (LAYERS, "Étiquettes visibles", "Enregistrer les étiquettes qui sont masquées"),
    (SECTIONS, "Plans de section actifs", "Enregistrer les plans de section actifs"),
    (RENDERING | STYLE, "Style et brouillard", "Enregistrer les paramètres d’affichage avec la scène"),
    (SHADOW, "Paramètres d'ombre", "Enregistrer les informations d’ombrage avec la scène"),
    (ENVIRONMENT, "Paramètres de l'environnement", "Enregistrer l'environnement avec la scène"),
    (AXES, "Emplacement des axes", "Enregistre des infos sur l’affichage ou non des axes de dessin"),
];

/// The rendering options that are settings of the application (`ViewCx::ro_get`).
const RO_KEYS: [&str; 23] = [
    "DrawHiddenGeometry",
    "DrawHiddenObjects",
    "DisplaySketchAxes",
    "DrawBackEdges",
    "DrawSilhouettes",
    "ExtendLines",
    "ModelTransparency",
    "DisplayFog",
    "DrawDepthQue",
    "DepthQueWidth",
    "AmbientOcclusion",
    "AmbientOcclusionDistance",
    "AmbientOcclusionMultiplier",
    "AmbientOcclusionIntensity",
    "DisplaySectionPlanes",
    "DisplaySectionCuts",
    "SectionCutFilled",
    "DrawHorizon",
    "InactiveHidden",
    "InstanceHidden",
    "EdgeDisplayMode",
    "RenderMode",
    "Texture",
];

/// What a scene saves (Ruby's default: everything).
pub fn flags(s: &Scene) -> i64 {
    s.ext.props.get("flags").and_then(J::as_i64).unwrap_or(ALL)
}

pub fn set_flags(s: &mut Scene, f: i64) {
    s.ext.props.insert("flags".into(), json!(f));
}

/// "Inclure dans l'animation".
pub fn animated(s: &Scene) -> bool {
    s.ext.props.get("animate").and_then(J::as_bool).unwrap_or(true)
}

/// The next scene the animation shows after `from` (None when it shows none).
pub fn next_animated(m: &Model, from: Option<usize>) -> Option<usize> {
    let n = m.scenes.len();
    let start = from.map_or(0, |i| i + 1);
    (0..n).map(|k| (start + k) % n).find(|i| animated(&m.scenes[*i]))
}

fn handle(kind: char, ctx: Option<DefId>, id: u64) -> String {
    format!("{kind}:{}:{id}", ctx.map_or("0".to_string(), |d| d.data().as_ffi().to_string()))
}

/// Handles (Ruby's) of the hidden top-level geometry and objects.
fn hidden_handles(e: &Entities, geometry: bool, objects: bool) -> Vec<J> {
    let mut v = Vec::new();
    if geometry {
        v.extend(e.edges.iter().filter(|(_, x)| x.hidden).map(|(k, _)| handle('e', None, k.data().as_ffi())));
        v.extend(e.faces.iter().filter(|(_, x)| x.hidden).map(|(k, _)| handle('f', None, k.data().as_ffi())));
        v.extend(e.guides.iter().filter(|(_, x)| x.hidden).map(|(k, _)| handle('g', None, k.data().as_ffi())));
    }
    if objects {
        v.extend(e.instances.iter().filter(|(_, x)| x.hidden).map(|(k, _)| handle('i', None, k.data().as_ffi())));
    }
    v.into_iter().map(J::from).collect()
}

/// Handles of the active section planes of every collection.
fn active_sections(m: &Model) -> Vec<J> {
    let mut v: Vec<J> = Vec::new();
    if let Some(s) = m.root.active_section {
        v.push(handle('s', None, s.data().as_ffi()).into());
    }
    for (d, def) in m.defs.iter() {
        if let Some(s) = def.entities.active_section {
            v.push(handle('s', Some(d), s.data().as_ffi()).into());
        }
    }
    v
}

fn view_cx(app: &mut EsquisseApp) -> crate::ruby_host::view::ViewCx<'_> {
    crate::ruby_host::view::ViewCx {
        cam: &mut app.cam,
        style: &mut app.style,
        active_scene: &mut app.active_scene,
        anim: (app.anim.transition, app.anim.delay),
        vs: &mut app.ui_state.extra.view,
    }
}

/// The rendering options as a scene saves them (Ruby's `RenderingOptions#__snapshot`).
fn ro_snapshot(app: &mut EsquisseApp) -> J {
    let mut o = app.doc.model.view_ext.get("ro").and_then(J::as_object).cloned().unwrap_or_default();
    let cx = view_cx(app);
    for k in RO_KEYS {
        if let Some(v) = cx.ro_get(k).get("value") {
            o.insert(k.to_string(), v.clone());
        }
    }
    J::Object(o)
}

fn ro_restore(app: &mut EsquisseApp, h: &J) {
    let mut unmapped = serde_json::Map::new();
    {
        let mut cx = view_cx(app);
        for (k, v) in h.as_object().into_iter().flatten() {
            if cx.ro_set(k, v).get("mapped") != Some(&J::Bool(true)) {
                unmapped.insert(k.clone(), v.clone());
            }
        }
    }
    if !unmapped.is_empty() {
        let ro = app.doc.model.view_ext.entry("ro".to_string()).or_insert_with(|| json!({}));
        if let Some(o) = ro.as_object_mut() {
            o.extend(unmapped);
        }
    }
}

/// The drawing axes as a scene saves them: Ruby's `Geom::Transformation` (16 numbers, the
/// translation in inches), so that `Page#axes` and `Pages#selected_page=` read the same.
fn axes_json(m: &DMat4) -> J {
    let mut v = m.to_cols_array();
    for t in &mut v[12..15] {
        *t /= 25.4;
    }
    json!(v)
}

fn axes_from(j: &J) -> Option<DMat4> {
    let a = j.as_array().filter(|a| a.len() == 16)?;
    let mut v: Vec<f64> = a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect();
    for t in &mut v[12..15] {
        *t *= 25.4;
    }
    Some(DMat4::from_cols_slice(&v))
}

fn hidden_layers(m: &Model) -> Vec<usize> {
    m.layers.iter().enumerate().filter(|(_, l)| !l.visible).map(|(i, _)| i).collect()
}

/// Saves in a scene the present state of the properties `which` (View › Animation › Add or
/// Update Scene); the scene saves them from now on.
pub fn capture(app: &mut EsquisseApp, s: &mut Scene, which: i64) {
    if which & CAMERA != 0 {
        s.camera = app.cam.to_view();
    }
    if which & LAYERS != 0 {
        s.hidden_layers = Some(hidden_layers(&app.doc.model));
        // The tag folders switched off (their own switch, as `Page#update` saves them).
        let folders: Vec<J> = app.doc.model.meta.folders.iter().filter(|f| !f.visible).map(|f| json!(f.uid)).collect();
        s.ext.props.insert("hidden_folders".into(), J::Array(folders));
    }
    if which & (RENDERING | STYLE) != 0 {
        let ro = ro_snapshot(app);
        s.ext.props.insert("ro".into(), ro);
        // And the style it is (`Page#style`), as SketchUp keeps it with the scene.
        let (list, sel) = crate::styles::model_styles(&mut app.doc.model, &app.style);
        if let Some(st) = list.iter().find(|x| x.id == sel) {
            s.ext.props.insert("style".into(), json!(st.name));
        }
    }
    let m = &app.doc.model;
    if which & SHADOW != 0 {
        s.ext.props.insert("shadow".into(), esq_ruby::shadow_snapshot(m));
    }
    let (geometry, objects) = (which & (HIDDEN | HIDDEN_GEOMETRY) != 0, which & (HIDDEN | HIDDEN_OBJECTS) != 0);
    if geometry || objects {
        s.ext.props.insert("hidden_ents".into(), J::Array(hidden_handles(&m.root, geometry, objects)));
    }
    if which & SECTIONS != 0 {
        s.ext.props.insert("sections".into(), J::Array(active_sections(m)));
    }
    if which & ENVIRONMENT != 0 {
        let cur = m.view_ext.get("env_current").cloned().unwrap_or(J::Null);
        let name = m
            .view_ext
            .get("envs")
            .and_then(J::as_array)
            .and_then(|l| l.iter().find(|e| e.get("id") == Some(&cur)))
            .and_then(|e| e.get("name").cloned())
            .unwrap_or(J::Null);
        s.ext.props.insert("env".into(), name);
    }
    if which & AXES != 0 {
        s.ext.props.insert("axes".into(), axes_json(&m.axes));
    }
    let f = flags(s) | which;
    set_flags(s, f);
}

/// Puts back what a scene saves, except the camera (the caller moves it, smoothly or not).
pub fn apply(app: &mut EsquisseApp, s: &Scene) {
    let f = flags(s);
    let props = &s.ext.props;
    let mut changed = false;
    if f & LAYERS != 0
        && let Some(hidden) = &s.hidden_layers
    {
        for (k, l) in app.doc.model.layers.iter_mut().enumerate() {
            let v = !hidden.contains(&k);
            changed |= l.visible != v;
            l.visible = v;
        }
    }
    // The folders' own switches (`Page#selected_page=` does the same): their tags already have
    // the scene's state.
    if f & LAYERS != 0
        && let Some(list) = props.get("hidden_folders").and_then(J::as_array)
    {
        for fo in app.doc.model.meta.folders.iter_mut() {
            let v = !list.iter().any(|u| u.as_u64() == Some(u64::from(fo.uid)));
            changed |= fo.visible != v;
            fo.visible = v;
        }
    }
    if f & (RENDERING | STYLE) != 0
        && let Some(ro) = props.get("ro")
    {
        let before = app.style.clone();
        ro_restore(app, ro);
        changed |= app.style != before;
    }
    // The scene's style becomes the selected one, with its watermarks (SketchUp selects the
    // style of a scene that saves its style).
    if f & (RENDERING | STYLE) != 0
        && let Some(name) = props.get("style").and_then(J::as_str)
    {
        let (list, sel) = crate::styles::model_styles(&mut app.doc.model, &app.style);
        if let Some(st) = list.iter().find(|x| x.name == name) {
            if st.id != sel {
                crate::styles::save_styles(&mut app.doc.model, &list, st.id);
                changed = true;
            }
            if app.style.watermark_list != st.watermarks {
                app.style.watermark_list = st.watermarks.clone();
                changed = true;
            }
        }
    }
    let m = &mut app.doc.model;
    if f & SHADOW != 0
        && let Some(sh) = props.get("shadow")
    {
        esq_ruby::shadow_restore(m, sh);
        changed = true;
    }
    let (geometry, objects) = (f & (HIDDEN | HIDDEN_GEOMETRY) != 0, f & (HIDDEN | HIDDEN_OBJECTS) != 0);
    if (geometry || objects)
        && let Some(list) = props.get("hidden_ents").and_then(J::as_array)
    {
        let hidden = |kind: char, id: u64| list.iter().any(|h| h.as_str() == Some(&handle(kind, None, id)));
        let e = &mut m.root;
        if geometry {
            for (k, x) in e.edges.iter_mut() {
                x.hidden = hidden('e', k.data().as_ffi());
            }
            for (k, x) in e.faces.iter_mut() {
                x.hidden = hidden('f', k.data().as_ffi());
            }
            for (k, x) in e.guides.iter_mut() {
                x.hidden = hidden('g', k.data().as_ffi());
            }
        }
        if objects {
            for (k, x) in e.instances.iter_mut() {
                x.hidden = hidden('i', k.data().as_ffi());
            }
        }
        changed = true;
    }
    if f & SECTIONS != 0
        && let Some(list) = props.get("sections").and_then(J::as_array)
    {
        let on = |ctx: Option<DefId>, e: &Entities| {
            e.sections.keys().find(|k| list.iter().any(|h| h.as_str() == Some(&handle('s', ctx, k.data().as_ffi()))))
        };
        m.root.active_section = on(None, &m.root);
        let defs: Vec<DefId> = m.defs.keys().collect();
        for d in defs {
            let a = on(Some(d), &m.defs[d].entities);
            m.defs[d].entities.active_section = a;
        }
        changed = true;
    }
    if f & ENVIRONMENT != 0
        && let Some(name) = props.get("env")
    {
        let id = m
            .view_ext
            .get("envs")
            .and_then(J::as_array)
            .and_then(|l| l.iter().find(|e| e.get("name") == Some(name)))
            .and_then(|e| e.get("id").cloned())
            .unwrap_or(J::Null);
        m.view_ext.insert("env_current".into(), id);
        changed = true;
    }
    if f & AXES != 0
        && let Some(a) = props.get("axes").and_then(axes_from)
    {
        m.axes = a;
        changed = true;
    }
    if changed {
        app.doc.rev += 1;
        app.doc.model_rev += 1;
    }
}

// ------------------------------------------------------------------------------------------
// Window › Scenes

#[derive(Default)]
pub struct SceneEditState {
    name: Option<String>,
    desc: Option<String>,
    /// "Actualisation": the scene being updated and the properties ticked.
    pub update: Option<(usize, i64)>,
}

fn prop_checks(ui: &mut egui::Ui, f: &mut i64) {
    for (bits, label, tip) in PROPS {
        let mut on = *f & bits != 0;
        if ui.checkbox(&mut on, label).on_hover_text(tip).changed() {
            *f = if on { *f | bits } else { *f & !bits };
        }
    }
}

/// The properties of the selected scene, under the list of Window › Scenes.
pub fn properties_ui(app: &mut EsquisseApp, ui: &mut egui::Ui, i: usize) {
    let s = app.doc.model.scenes[i].clone();
    let st = &mut app.ui_state.extra.dialogs.scene_edit;
    let mut anim = animated(&s);
    let anim_changed = ui
        .checkbox(&mut anim, "Inclure dans l'animation")
        .on_hover_text("Inclure cette scène dans les diaporamas")
        .changed();
    let mut name = st.name.clone().unwrap_or_else(|| s.name.clone());
    let mut desc = st.desc.clone().unwrap_or_else(|| s.description.clone());
    let (mut done_name, mut done_desc) = (false, false);
    egui::Grid::new("scene_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Nom :");
        let (done, focused) = text_field(ui, "Nom de la scène", &mut name, 190.0);
        done_name = done;
        if focused {
            st.name = Some(name.clone());
        }
        ui.end_row();
        ui.label("Description:");
        let r = ui.add(egui::TextEdit::multiline(&mut desc).desired_rows(2).desired_width(190.0));
        name_widget(ui, &r, "Description de la scène", None);
        if r.has_focus() {
            st.desc = Some(desc.clone());
        }
        done_desc = r.lost_focus();
        ui.end_row();
    });
    ui.label(RichText::new("Propriétés pour sauvegarder :").strong());
    let before = flags(&s);
    let mut f = before;
    prop_checks(ui, &mut f);
    let mut out = s.clone();
    let mut changed = false;
    if done_name {
        app.ui_state.extra.dialogs.scene_edit.name = None;
        let n = name.trim().to_string();
        if !n.is_empty() && n != s.name {
            let taken = app.doc.model.scenes.iter().enumerate().any(|(k, x)| k != i && x.name == n);
            out.name = if taken { app.doc.model.next_scene_name() } else { n };
            changed = true;
        }
    }
    if done_desc {
        app.ui_state.extra.dialogs.scene_edit.desc = None;
        if desc != s.description {
            out.description = desc;
            changed = true;
        }
    }
    if anim_changed {
        out.ext.props.insert("animate".into(), json!(anim));
        changed = true;
    }
    if f != before {
        // A property ticked is saved as it is now; one unticked is no longer restored.
        let added = f & !before;
        capture(app, &mut out, added);
        set_flags(&mut out, f);
        if f & LAYERS == 0 {
            out.hidden_layers = None;
        }
        changed = true;
    }
    if changed {
        app.doc.commit("Propriétés de la scène", |m, _| m.scenes[i] = out);
    }
}

/// The Scenes panel's Update: "Actualisation" asks which properties to update.
pub fn ask_update(app: &mut EsquisseApp) {
    let Some(i) = app.active_scene.filter(|i| *i < app.doc.model.scenes.len()) else { return };
    let f = flags(&app.doc.model.scenes[i]);
    app.ui_state.extra.dialogs.scene_edit.update = Some((i, f));
}

/// Updates a scene's properties `which` from the present state (one undo step).
pub fn update(app: &mut EsquisseApp, i: usize, which: i64) {
    let Some(mut s) = app.doc.model.scenes.get(i).cloned() else { return };
    capture(app, &mut s, which);
    app.doc.commit("Actualiser la scène", |m, _| m.scenes[i] = s);
}

/// SketchUp's `PageUpdateDlg`.
pub fn update_window(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some((i, mut f)) = app.ui_state.extra.dialogs.scene_edit.update.take() else { return };
    let mut done = None;
    crate::ui::centered("Actualisation").show(ctx, |ui| {
        ui.label("Propriétés à actualiser:");
        prop_checks(ui, &mut f);
        ui.horizontal(|ui| {
            if ui.button("Mettre à jour").clicked() {
                done = Some(true);
            }
            if ui.button("Annuler").clicked() {
                done = Some(false);
            }
        });
    });
    match done {
        Some(true) => update(app, i, f),
        Some(false) => {}
        None => app.ui_state.extra.dialogs.scene_edit.update = Some((i, f)),
    }
}
