//! Window › Soften Edges: softens the selected edges whose faces meet at less than the angle,
//! as SketchUp's panel — « Angle entre les normales: », « Lisser les normales », « Adoucir
//! les faces coplanaires » —, also the edges inside the selected groups and components (the
//! right-click menu offers the panel for them), their definitions changed.

use std::collections::HashSet;

use eframe::egui;
use esq_core::model::{DefId, EdgeId, Entities, EntityRef};

use crate::app::EsquisseApp;

/// Softens (or not) the edges `targets` of `ents` by the panel's rule.
fn soften_edges(ents: &mut Entities, targets: &[EdgeId], angle: f64, smooth: bool, coplanar: bool) {
    for &e in targets {
        let Some(edge) = ents.edges.get(e) else { continue };
        if edge.faces.len() != 2 {
            continue;
        }
        let (a, b) = (ents.faces[edge.faces[0]].normal, ents.faces[edge.faces[1]].normal);
        let deg = a.angle_between(b).to_degrees();
        let soft = if deg < 0.01 { coplanar } else { deg <= angle };
        let x = &mut ents.edges[e];
        x.soft = soft;
        x.smooth = soft && smooth;
    }
}

/// The definitions of the selected groups and components, and those nested in them.
fn selected_defs(app: &EsquisseApp) -> Vec<DefId> {
    let m = &app.doc.model;
    let ents = app.doc.ents();
    let mut seen = HashSet::new();
    let mut todo: Vec<DefId> = app
        .doc
        .sel
        .iter()
        .filter_map(|e| if let EntityRef::Instance(i) = e { ents.instances.get(*i).map(|x| x.def) } else { None })
        .collect();
    while let Some(d) = todo.pop() {
        if !seen.insert(d) {
            continue;
        }
        if let Some(def) = m.defs.get(d) {
            todo.extend(def.entities.instances.values().map(|i| i.def));
        }
    }
    seen.into_iter().filter(|d| !m.is_image_def(*d)).collect()
}

pub fn show(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let edges: Vec<_> =
        app.doc.sel.iter().filter_map(|e| if let EntityRef::Edge(x) = e { Some(*x) } else { None }).collect();
    let faces: Vec<_> =
        app.doc.sel.iter().filter_map(|e| if let EntityRef::Face(x) = e { Some(*x) } else { None }).collect();
    let defs = selected_defs(app);
    let (mut angle, mut smooth, mut coplanar) = app.ui_state.soften;
    let enabled = !edges.is_empty() || !faces.is_empty() || !defs.is_empty();
    let mut changed = false;
    ui.add_enabled_ui(enabled, |ui| {
        let l = ui.label("Angle entre les normales:");
        let r = ui.add(egui::Slider::new(&mut angle, 0.0..=180.0).suffix("°")).labelled_by(l.id);
        changed |= r.changed();
        changed |= ui.checkbox(&mut smooth, "Lisser les normales").changed();
        changed |= ui.checkbox(&mut coplanar, "Adoucir les faces coplanaires").changed();
    });
    app.ui_state.soften = (angle, smooth, coplanar);
    if changed && enabled {
        app.doc.commit("Adoucir les arêtes", |m, d| {
            let ents = m.entities_mut(d);
            let mut targets = edges.clone();
            for f in &faces {
                targets.extend(ents.face_edges(*f));
            }
            soften_edges(ents, &targets, angle, smooth, coplanar);
            for def in &defs {
                let e = &mut m.defs[*def].entities;
                let all: Vec<EdgeId> = e.edges.keys().collect();
                soften_edges(e, &all, angle, smooth, coplanar);
            }
        });
    }
}
