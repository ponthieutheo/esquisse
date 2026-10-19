//! Model Info › Statistics: what the model is made of (whole model or only components,
//! nested components expanded or not), Purge Unused and Fix Problems.

use eframe::egui::{self, RichText};
use esq_core::model::{AnnotKind, DefId, Entities, Model};

use super::{combo, group, name_widget};
use crate::app::EsquisseApp;

/// The rows of SketchUp's statistics table, in its order.
pub const ROWS: [&str; 17] = [
    "Arêtes",
    "Faces",
    "Instances de composants",
    "Guides",
    "Points de guide",
    "Groupes",
    "Images",
    "Polylignes 3D",
    "Plans de section",
    "Cotations",
    "Texte",
    "Définitions de composants",
    "Étiquettes",
    "Dossiers d'étiquettes",
    "Matières",
    "Styles",
    "Environnements",
];

#[derive(Default)]
pub struct StatsState {
    /// "Composants" (true) or "Tout le modèle".
    pub only_components: bool,
    pub nested: bool,
    /// What the last Fix Problems found.
    pub report: Option<Vec<String>>,
}

/// Counts of each row.
pub fn statistics(m: &Model, only_components: bool, nested: bool) -> Vec<(&'static str, usize)> {
    let mut c = [0usize; 17];
    let add = |e: &Entities, c: &mut [usize; 17]| {
        c[0] += e.edges.len();
        c[1] += e.faces.len();
        for i in e.instances.values() {
            match m.defs.get(i.def) {
                Some(_) if m.is_image_def(i.def) => c[6] += 1,
                Some(d) if d.is_group => c[5] += 1,
                Some(_) => c[2] += 1,
                None => {}
            }
        }
        for g in e.guides.values() {
            if g.is_point() { c[4] += 1 } else { c[3] += 1 }
        }
        c[8] += e.sections.len();
        for a in e.annots.values() {
            match a.kind {
                AnnotKind::Linear { .. } => c[9] += 1,
                _ => c[10] += 1,
            }
        }
        // 3D polylines (c[7]) come from imported CAD files: Esquisse has none yet.
    };
    // The collections counted: the model's own geometry unless only components; and the
    // definitions' geometry when components are expanded (each definition once).
    if !only_components {
        add(&m.root, &mut c);
    }
    if nested || only_components {
        let used: Vec<DefId> = m.defs.keys().filter(|d| m.instance_count(*d) > 0).collect();
        for d in used {
            let def = &m.defs[d];
            if only_components && (def.is_group || m.is_image_def(d)) && !nested {
                continue;
            }
            add(&def.entities, &mut c);
        }
    }
    c[11] = m.defs.iter().filter(|(k, d)| !d.is_group && !m.is_image_def(*k)).count();
    c[12] = m.layers.len();
    c[13] = m.meta.folders.len();
    c[14] = m.materials.len();
    let list = |k: &str| m.view_ext.get(k).and_then(|v| v.as_array()).map_or(0, |a| a.len());
    c[15] = list("styles").max(1);
    c[16] = list("envs");
    ROWS.iter().copied().zip(c).collect()
}

pub fn statistics_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let st = &mut app.ui_state.extra.dialogs.info.stats;
    ui.horizontal(|ui| {
        let kinds = [(false, "Tout le modèle".to_string()), (true, "Composants".to_string())];
        combo(ui, "Statistiques de", &mut st.only_components, &kinds);
        ui.checkbox(&mut st.nested, "Développer la hiérarchie des composants");
    });
    let rows = statistics(&app.doc.model, st.only_components, st.nested);
    egui::Grid::new("stats_grid").striped(true).num_columns(2).min_col_width(180.0).show(ui, |ui| {
        ui.label(RichText::new("Nom").strong());
        ui.label(RichText::new("Nombre").strong());
        ui.end_row();
        for (name, n) in &rows {
            ui.label(*name);
            let r = ui.label(n.to_string());
            name_widget(ui, &r, &format!("Nombre de {name}"), Some(&n.to_string()));
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("Purger les éléments inutilisés").clicked() {
            let ctx = ui.ctx().clone();
            app.run(&ctx, crate::commands::Cmd::PurgeUnused);
        }
        if ui.button("Corriger les problèmes").clicked() {
            fix_problems(app, true);
        }
    });
    if let Some(r) = &app.ui_state.extra.dialogs.info.stats.report {
        group(ui, "Résultats de la validation");
        if r.is_empty() {
            ui.label("Aucun problème trouvé.");
        }
        for line in r {
            ui.label(line);
        }
    }
}

/// Fix Problems: repairs the model (one undo step) and reports what was done.
pub fn fix_problems(app: &mut EsquisseApp, report: bool) -> usize {
    let found = esq_core::validity::check(&app.doc.model);
    if !found.is_empty() {
        app.doc.commit("Corriger les problèmes", |m, _| {
            esq_core::validity::fix_model(m);
        });
    }
    if report {
        app.ui_state.extra.dialogs.info.stats.report = Some(found.iter().map(|p| p.text.clone()).collect());
    }
    found.len()
}
