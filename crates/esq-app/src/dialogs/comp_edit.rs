//! Window › Components › Edit (SketchUp's `CompDefEditPage`): the name, description and
//! alignment of a component definition — what faces it glues to, cut opening, always face
//! camera, shadows face sun —, the file it was loaded from, and Save As. The settings live
//! in the definition (`DefExt::behavior`, read by Ruby's `Sketchup::Behavior`) and act:
//! a gluing component placed on a face of the right slope lies on it and stays glued, a
//! cutting one opens the face it is glued to, a face-me one turns to the camera.

use std::collections::HashMap;
use std::path::Path;

use eframe::egui::{self, RichText};
use esq_core::glam::{DVec2, DVec3};
use esq_core::math::basis_for;
use esq_core::model::{Behavior, DefId, Entities, EntityRef, FaceId, GlueRef, Model, VertexId};

use super::{combo, group, name_widget, text_field};
use crate::app::EsquisseApp;

#[derive(Default)]
pub struct CompEditState {
    /// The definition chosen in the page's list.
    pub def: Option<DefId>,
    name: Option<String>,
    desc: Option<String>,
}

/// The definition the page edits: the one chosen in its list, else the selected component's.
pub fn edited(app: &EsquisseApp) -> Option<DefId> {
    let m = &app.doc.model;
    let chosen = app.ui_state.extra.dialogs.comp_edit.def.filter(|d| m.defs.get(*d).is_some_and(|x| !x.is_group));
    chosen.or_else(|| match app.doc.sel.as_slice() {
        [EntityRef::Instance(i)] => {
            app.doc.ents().instances.get(*i).map(|x| x.def).filter(|d| m.defs.get(*d).is_some_and(|x| !x.is_group))
        }
        _ => None,
    })
}

/// The "Modifier" part of Window › Components.
pub fn edit_section(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    group(ui, "Modifier");
    let mut defs: Vec<(DefId, String)> = app
        .doc
        .model
        .defs
        .iter()
        .filter(|(_, d)| !d.is_group && d.ext.image.is_none())
        .map(|(k, d)| (k, d.name.clone()))
        .collect();
    defs.sort_by_key(|(_, n)| n.to_lowercase());
    let Some(first) = defs.first().map(|(k, _)| *k) else { return };
    let mut d = edited(app).unwrap_or(first);
    if combo(ui, "Composant à modifier", &mut d, &defs) {
        app.ui_state.extra.dialogs.comp_edit = CompEditState { def: Some(d), ..Default::default() };
    }
    let def = &app.doc.model.defs[d];
    let st = &mut app.ui_state.extra.dialogs.comp_edit;
    let mut name = st.name.clone().unwrap_or_else(|| def.name.clone());
    let mut desc = st.desc.clone().unwrap_or_else(|| def.description.clone());
    let path = def.ext.path.clone();
    let before = def.ext.behavior;
    let (mut done_name, mut done_desc) = (false, false);
    egui::Grid::new("comp_edit").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Nom :");
        let (done, focused) = text_field(ui, "Nom du composant", &mut name, 190.0);
        done_name = done;
        if focused {
            st.name = Some(name.clone());
        }
        ui.end_row();
        ui.label("Description :");
        let r = ui.add(egui::TextEdit::multiline(&mut desc).desired_rows(2).desired_width(190.0));
        name_widget(ui, &r, "Description du composant", None);
        if r.has_focus() {
            st.desc = Some(desc.clone());
        }
        done_desc = r.lost_focus();
        ui.end_row();
    });
    if done_name {
        app.ui_state.extra.dialogs.comp_edit.name = None;
        let name = name.trim().to_string();
        if !name.is_empty() && name != app.doc.model.defs[d].name {
            app.doc.commit("Nom du composant", |m, _| {
                let taken = m.defs.iter().any(|(k, x)| k != d && x.name == name);
                m.defs[d].name = if taken { m.unique_def_name(&name) } else { name };
            });
        }
    }
    if done_desc {
        app.ui_state.extra.dialogs.comp_edit.desc = None;
        if desc != app.doc.model.defs[d].description {
            app.doc.commit("Description du composant", |m, _| m.defs[d].description = desc);
        }
    }
    ui.label(RichText::new("Alignement").strong());
    let mut b = before;
    super::mi_file::alignment_ui(ui, &mut b);
    if b != before {
        app.doc.commit("Alignement du composant", |m, _| m.defs[d].ext.behavior = b);
    }
    ui.horizontal(|ui| {
        ui.label("Chargé depuis :");
        let shown = if path.is_empty() { "(ce modèle)".to_string() } else { path.clone() };
        let r = ui.label(RichText::new(&shown).weak());
        name_widget(ui, &r, "Chargé depuis", Some(&shown));
    });
    if ui.button("Enregistrer sous...").clicked()
        && let Some(p) = crate::settings::dialog("components")
            .add_filter("Esquisse", &["esq"])
            .add_filter("SketchUp", &["skp"])
            .set_file_name(format!("{}.esq", app.doc.model.defs[d].name))
            .save_file()
    {
        match save_definition(&app.doc.model, d, &p) {
            Ok(()) => {
                let s = p.display().to_string();
                app.doc.commit("Enregistrer le composant", |m, _| m.defs[d].ext.path = s);
            }
            Err(e) => app.message = Some((format!("Échec de l'enregistrement : {e}"), 0.0)),
        }
    }
}

/// Components › Save As: the definition as a model of its own, which keeps its description
/// and its behaviour (so that it glues, cuts and turns again once loaded).
pub fn save_definition(model: &Model, d: DefId, path: &Path) -> Result<(), String> {
    let def = &model.defs[d];
    let mut out = Model::new();
    let mut map = esq_core::groups::ImportMap::default();
    out.root = out.import_entities(model, &def.entities, &mut map);
    out.rebuild_topology();
    out.meta.name = def.name.clone();
    out.meta.description = def.description.clone();
    out.set_behavior(def.ext.behavior);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "skp" => esq_skp::export_skp(&out, path).map_err(|e| e.to_string()),
        _ => esq_core::io::save_esq(&out, path).map_err(|e| e.to_string()),
    }
}

// ------------------------------------------------------------------------------------------
// Glue

/// Whether a gluing component may lie on a face of normal `n` (world): any face, or only
/// the horizontal, vertical or sloped ones (`Behavior::snapto`).
pub fn glue_allows(b: &Behavior, n: DVec3) -> bool {
    if !b.is2d {
        return false;
    }
    let z = n.normalize_or_zero().z.abs();
    match b.snapto {
        1 => z > 0.9999,
        2 => z < 1e-4,
        3 => (1e-4..=0.9999).contains(&z),
        _ => true,
    }
}

/// The face a component placed on `item` (reached through `path`) glues to, in the open
/// collection `ctx`.
pub fn glue_target(
    path: &[esq_core::model::InstanceId],
    ctx: &[esq_core::model::InstanceId],
    item: EntityRef,
) -> Option<GlueRef> {
    match item {
        EntityRef::Face(f) if path == ctx => Some(GlueRef::Face(f)),
        _ => None,
    }
}

// ------------------------------------------------------------------------------------------
// Cut opening

/// The loops a definition draws in its gluing plane (z = 0): the opening it cuts. Loops
/// inside another one are left out.
pub fn opening_loops(model: &Model, d: DefId) -> Vec<Vec<DVec3>> {
    let e = &model.defs[d].entities;
    let flat = |v: VertexId| e.verts.get(v).is_some_and(|x| x.pos.z.abs() < 1e-3);
    let mut next: HashMap<VertexId, Vec<VertexId>> = HashMap::new();
    for edge in e.edges.values() {
        let [a, b] = edge.v;
        if flat(a) && flat(b) && a != b {
            next.entry(a).or_default().push(b);
            next.entry(b).or_default().push(a);
        }
    }
    let mut used: std::collections::HashSet<(VertexId, VertexId)> = Default::default();
    let mut loops: Vec<Vec<DVec3>> = Vec::new();
    let starts: Vec<VertexId> = next.keys().copied().collect();
    for s in starts {
        for &first in next[&s].clone().iter() {
            if used.contains(&(s, first)) {
                continue;
            }
            let mut lp = vec![s];
            let (mut prev, mut cur) = (s, first);
            used.insert((s, first));
            used.insert((first, s));
            let closed = loop {
                if cur == s {
                    break true;
                }
                lp.push(cur);
                let Some(&n) = next[&cur].iter().find(|&&n| n != prev && !used.contains(&(cur, n))) else {
                    break false;
                };
                used.insert((cur, n));
                used.insert((n, cur));
                prev = cur;
                cur = n;
                if lp.len() > 10_000 {
                    break false;
                }
            };
            if closed && lp.len() >= 3 {
                loops.push(lp.iter().map(|v| e.verts[*v].pos).collect());
            }
        }
    }
    let to2 = |l: &Vec<DVec3>| l.iter().map(|p| p.truncate()).collect::<Vec<DVec2>>();
    let flat_loops: Vec<Vec<DVec2>> = loops.iter().map(to2).collect();
    loops
        .into_iter()
        .enumerate()
        .filter(|(i, l)| {
            !flat_loops
                .iter()
                .enumerate()
                .any(|(j, o)| j != *i && o.len() >= 3 && inside(l[0].truncate(), o) && area(o) > area(&flat_loops[*i]))
        })
        .map(|(_, l)| l)
        .collect()
}

fn area(p: &[DVec2]) -> f64 {
    (0..p.len()).map(|i| p[i].perp_dot(p[(i + 1) % p.len()])).sum::<f64>().abs() * 0.5
}

fn inside(q: DVec2, poly: &[DVec2]) -> bool {
    let mut c = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) * (b.x - a.x) / (b.y - a.y) {
            c = !c;
        }
    }
    c
}

/// The openings the cutting components glued to the faces of `ents` make in them, in the
/// coordinates of `ents`.
pub fn openings(model: &Model, ents: &Entities) -> HashMap<FaceId, Vec<Vec<DVec3>>> {
    let mut out: HashMap<FaceId, Vec<Vec<DVec3>>> = HashMap::new();
    for inst in ents.instances.values() {
        let Some(GlueRef::Face(f)) = inst.ext.glued_to else { continue };
        let Some(def) = model.defs.get(inst.def) else { continue };
        if !def.ext.behavior.cuts_opening || !ents.faces.contains_key(f) {
            continue;
        }
        for l in opening_loops(model, inst.def) {
            out.entry(f).or_default().push(l.iter().map(|p| inst.xform.transform_point3(*p)).collect());
        }
    }
    out
}

/// The triangles of a face with openings cut in it (those within its outline).
pub fn cut_triangles(ents: &Entities, f: FaceId, openings: &[Vec<DVec3>]) -> Vec<[DVec3; 3]> {
    let face = &ents.faces[f];
    let n = face.normal;
    let (u, v) = basis_for(n);
    let origin = ents.verts[face.outer[0]].pos;
    let to2 = |p: DVec3| DVec2::new((p - origin).dot(u), (p - origin).dot(v));
    let from2 = |q: DVec2| origin + u * q.x + v * q.y;
    let outer: Vec<DVec2> = face.outer.iter().map(|x| to2(ents.verts[*x].pos)).collect();
    let mut holes: Vec<Vec<DVec2>> =
        face.holes.iter().map(|h| h.iter().map(|x| to2(ents.verts[*x].pos)).collect()).collect();
    for o in openings {
        // Only an opening lying on the face's plane and inside its outline.
        let flat = o.iter().all(|p| (*p - origin).dot(n).abs() < 1.0);
        let pts: Vec<DVec2> = o.iter().map(|p| to2(*p)).collect();
        if flat && pts.iter().all(|q| inside(*q, &outer)) {
            holes.push(pts);
        }
    }
    let all: Vec<DVec2> = outer.iter().chain(holes.iter().flatten()).copied().collect();
    esq_core::tess::triangulate_2d(&outer, &holes)
        .into_iter()
        .map(|[a, b, c]| {
            let t = [from2(all[a]), from2(all[b]), from2(all[c])];
            if (t[1] - t[0]).cross(t[2] - t[0]).dot(n) < 0.0 { [t[0], t[2], t[1]] } else { t }
        })
        .collect()
}

// ------------------------------------------------------------------------------------------
// Shadows face sun

/// Direction toward the sun, as the display lights the model (Geo-location's solar
/// orientation, a sun linked to the environment).
pub fn sun_direction(model: &Model) -> DVec3 {
    let s = &model.shadow;
    let date = if super::mi_file::solar_orientation(model) {
        s.sun_direction()
    } else {
        esq_core::sun::ShadowInfo { north_angle: 0.0, ..s.clone() }.sun_direction()
    };
    crate::edit_ops::linked_sun_direction(model).unwrap_or(date)
}

/// For a face-me component whose shadows face the sun, shown while shadows are on: where
/// its shadow-casting copy stands (turned to the sun instead of the camera). Its displayed
/// faces then cast no shadow.
pub fn sun_copy(
    model: &Model,
    def: Option<DefId>,
    path: &[esq_core::model::InstanceId],
) -> Option<esq_core::glam::DMat4> {
    let b = model.defs.get(def?)?.ext.behavior;
    if !(b.always_face_camera && b.shadows_face_sun && model.shadow.display) || esq_core::walk::eye().is_none() {
        return None;
    }
    let base = esq_core::walk::path_xform(model, path);
    let toward = base.transform_point3(DVec3::ZERO) + sun_direction(model) * 1e7;
    Some(base * esq_core::walk::face_camera(&base, toward))
}

// ------------------------------------------------------------------------------------------
// Always face camera

/// Called every frame with the camera's eye: face-me components turn to it. True when the
/// display and picking must be rebuilt (the eye moved and such components exist).
pub fn follow_camera(model: &Model, eye: DVec3) -> bool {
    let any = model.defs.values().any(|d| d.ext.behavior.always_face_camera);
    let before = esq_core::walk::eye();
    let now = any.then_some(eye);
    esq_core::walk::set_eye(now);
    match (before, now) {
        (Some(a), Some(b)) => a.distance(b) > 1e-6,
        (None, None) => false,
        _ => true,
    }
}
