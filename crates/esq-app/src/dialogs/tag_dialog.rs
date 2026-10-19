//! Window › Tags beyond the list (SketchUp's `TagDialog` panel and `LayerDeleteDlg`): the
//! active tag — the one new objects receive (`ModelMeta::active_layer`, Ruby's
//! `Model#active_layer`), which cannot be hidden — and deleting a tag that tags entities,
//! which asks whether to give them another tag or to delete them.

use eframe::egui::{self, RichText};
use esq_core::model::{DefId, Entities, LayerIdx, Model};

use super::combo;
use crate::app::EsquisseApp;

#[derive(Default)]
pub struct TagState {
    /// "Supprimer les balises contenant des entités": the tag, the choice (None = delete the
    /// entities, Some = the tag they receive).
    pub delete: Option<(LayerIdx, Option<LayerIdx>)>,
    /// The tag is used by locked groups or components: asked first.
    pub locked: Option<LayerIdx>,
}

/// Index of the active tag.
pub fn active(m: &Model) -> LayerIdx {
    let uid = m.meta.active_layer;
    if uid == 0 { 0 } else { m.layers.iter().position(|l| l.uid == uid).unwrap_or(0) }
}

/// Makes a tag the active one (SketchUp's "Set Active Tag", one undo step).
pub fn set_active(app: &mut EsquisseApp, idx: LayerIdx) {
    if idx == active(&app.doc.model) || idx >= app.doc.model.layers.len() {
        return;
    }
    app.doc.commit("Définir l'étiquette active", |m, _| {
        m.ensure_ruby_ids();
        m.layers[idx].visible = true;
        m.meta.active_layer = if idx == 0 { 0 } else { m.layers[idx].uid };
    });
}

/// After a change: the objects it created in the open collection receive the active tag
/// (those it gave a tag of their own keep it). `old`: the keys of the open collection before
/// the change (`None`: it did not exist).
pub fn tag_new(old: Option<crate::history::Keys>, after: &mut Model, ctx: Option<DefId>) {
    let tag = active(after);
    if tag == 0 {
        return;
    }
    let Some(old) = old else { return };
    let e = after.entities_mut(ctx);
    macro_rules! retag {
        ($field:ident) => {
            for (k, x) in e.$field.iter_mut() {
                if x.layer == 0 && !old.$field.contains_key(k) {
                    x.layer = tag;
                }
            }
        };
    }
    retag!(edges);
    retag!(faces);
    retag!(instances);
    retag!(guides);
    retag!(annots);
    retag!(sections);
}

/// How many entities a tag tags, and whether a locked group or component is among them.
pub fn uses(m: &Model, idx: LayerIdx) -> (usize, bool) {
    let mut n = 0;
    let mut locked = false;
    for e in std::iter::once(&m.root).chain(m.defs.values().map(|d| &d.entities)) {
        n += e.edges.values().filter(|x| x.layer == idx).count()
            + e.faces.values().filter(|x| x.layer == idx).count()
            + e.guides.values().filter(|x| x.layer == idx).count()
            + e.annots.values().filter(|x| x.layer == idx).count()
            + e.sections.values().filter(|x| x.layer == idx).count();
        for i in e.instances.values().filter(|x| x.layer == idx) {
            n += 1;
            locked |= i.locked;
        }
    }
    (n, locked)
}

/// Deletes a tag: its entities receive `target`, or are deleted (`None`).
pub fn delete_tag(m: &mut Model, idx: LayerIdx, target: Option<LayerIdx>) {
    match target {
        Some(t) if t != idx => {
            let give = |e: &mut Entities| {
                e.edges.values_mut().filter(|x| x.layer == idx).for_each(|x| x.layer = t);
                e.faces.values_mut().filter(|x| x.layer == idx).for_each(|x| x.layer = t);
                e.instances.values_mut().filter(|x| x.layer == idx).for_each(|x| x.layer = t);
                e.guides.values_mut().filter(|x| x.layer == idx).for_each(|x| x.layer = t);
                e.annots.values_mut().filter(|x| x.layer == idx).for_each(|x| x.layer = t);
                e.sections.values_mut().filter(|x| x.layer == idx).for_each(|x| x.layer = t);
                e.snaps.iter_mut().filter(|x| x.layer == idx).for_each(|x| x.layer = t);
                e.spaces.iter_mut().filter(|x| x.layer == idx).for_each(|x| x.layer = t);
            };
            give(&mut m.root);
            for d in m.defs.values_mut() {
                give(&mut d.entities);
            }
            m.remove_layer(idx, false);
        }
        _ => m.remove_layer(idx, true),
    }
}

/// Asks what to do before deleting a tag that tags entities.
pub fn ask_delete(app: &mut EsquisseApp, idx: LayerIdx) {
    if idx == 0 || idx >= app.doc.model.layers.len() {
        return;
    }
    let (n, locked) = uses(&app.doc.model, idx);
    let st = &mut app.ui_state.extra.dialogs.tags;
    if locked {
        st.locked = Some(idx);
    } else if n > 0 {
        st.delete = Some((idx, Some(0)));
    } else {
        app.doc.commit("Supprimer l'étiquette", |m, _| delete_tag(m, idx, None));
    }
}

/// The windows of the tags, with the other dialogs.
pub fn windows(app: &mut EsquisseApp, ctx: &egui::Context) {
    if let Some(idx) = app.ui_state.extra.dialogs.tags.locked {
        let mut answer = None;
        crate::ui::centered("Supprimer l'étiquette").show(ctx, |ui| {
            ui.label(
                "Les étiquettes à supprimer sont utilisées par des groupes ou des composants verrouillés.  Continuer ?",
            );
            ui.horizontal(|ui| {
                if ui.button("Oui").clicked() {
                    answer = Some(true);
                }
                if ui.button("Non").clicked() {
                    answer = Some(false);
                }
            });
        });
        if let Some(yes) = answer {
            let st = &mut app.ui_state.extra.dialogs.tags;
            st.locked = None;
            if yes {
                st.delete = Some((idx, Some(0)));
            }
        }
    }
    let Some((idx, mut choice)) = app.ui_state.extra.dialogs.tags.delete.take() else { return };
    if idx >= app.doc.model.layers.len() {
        return;
    }
    let names: Vec<(LayerIdx, String)> = app
        .doc
        .model
        .layers
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != idx)
        .map(|(i, l)| (i, if i == 0 { "Sans étiquette".to_string() } else { l.name.clone() }))
        .collect();
    let mut done = None;
    crate::ui::centered("Supprimer les balises contenant des entités").show(ctx, |ui| {
        ui.set_max_width(420.0);
        ui.label(
            "Des entités dans le modèle ont été étiquetées avec des balises à supprimer. Que souhaitez-vous faire avec ces entités ?",
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.radio(choice.is_some(), "Attribuer une autre étiquette :").clicked() {
                choice = Some(choice.unwrap_or(0));
            }
            let mut t = choice.unwrap_or(0);
            ui.add_enabled_ui(choice.is_some(), |ui| {
                if combo(ui, "Autre étiquette", &mut t, &names) {
                    choice = Some(t);
                }
            });
        });
        if ui.radio(choice.is_none(), "Supprimer les entités").clicked() {
            choice = None;
        }
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                done = Some(true);
            }
            if ui.button("Annuler").clicked() {
                done = Some(false);
            }
        });
        ui.label(RichText::new(format!("Étiquette : {}", app.doc.model.layers[idx].name)).weak());
    });
    match done {
        Some(true) => app.doc.commit("Supprimer l'étiquette", |m, _| delete_tag(m, idx, choice)),
        Some(false) => {}
        None => app.ui_state.extra.dialogs.tags.delete = Some((idx, choice)),
    }
}
