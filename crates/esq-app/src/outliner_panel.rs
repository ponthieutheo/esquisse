//! Window › Outliner, as SketchUp's: the tree of the groups and components of the model, each
//! named as SketchUp names it (a group by its instance name, else « Groupe »; a component
//! « nom <Définition> »), a « Filtre : » field that keeps the entries whose name matches (and
//! what leads to them), the menu « Tout développer », « Tout réduire », « Trier par nom »,
//! an eye per entry that hides or shows it (« Masquer » / « Révéler »), a padlock on the
//! locked ones. A click selects the entry, opening the group or component that holds it.
//!
//! Whatever the shape of the model (a definition holding two copies of the next one, 32 levels
//! deep, makes 2^32 entries), the panel stays quick: what matches the filter is worked out once
//! per definition, when the filter or the model changes, and at most [`MAX_ROWS`] rows are
//! drawn.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, RichText};
use esq_core::model::{DefId, EntityRef, Instance, InstanceId, Model};

use crate::app::EsquisseApp;

/// The most rows drawn in a frame; the rest shows as « … ».
pub const MAX_ROWS: usize = 500;

#[derive(Clone, Default)]
struct State {
    filter: String,
    /// « Trier par nom » unticked: the model's order.
    unsorted: bool,
    /// Expand All (true) / Collapse All (false): the entries' state, unless toggled since.
    all_open: bool,
    /// The entries opened or closed by hand since (their path from the model).
    toggled: HashSet<Vec<InstanceId>>,
    /// For the filter and the version of the model it was worked out for: whether something
    /// inside each definition matches.
    memo: Option<(String, u64, HashMap<DefId, bool>)>,
}

/// The name an entry shows: SketchUp's `<Definition>` for components.
pub fn entry_name(m: &Model, inst: &Instance) -> String {
    let Some(d) = m.defs.get(inst.def) else { return inst.name.clone() };
    if d.is_group {
        if inst.name.is_empty() { "Groupe".into() } else { inst.name.clone() }
    } else if inst.name.is_empty() {
        format!("<{}>", d.name)
    } else {
        format!("{} <{}>", inst.name, d.name)
    }
}

#[cfg(test)]
thread_local! {
    static VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Tests: how many definitions the filter looked into on this thread so far.
#[cfg(test)]
pub fn visits() -> usize {
    VISITS.with(|v| v.get())
}

/// Whether something inside definition `d` matches the filter: each definition looked into
/// once (`memo`), whatever how often it is used.
fn inside_matches(m: &Model, d: DefId, filter: &str, memo: &mut HashMap<DefId, bool>, depth: usize) -> bool {
    if let Some(v) = memo.get(&d) {
        return *v;
    }
    // A cycle or an absurd depth: nothing more to find there.
    memo.insert(d, false);
    if depth > 64 {
        return false;
    }
    #[cfg(test)]
    VISITS.with(|v| v.set(v.get() + 1));
    let kids: Vec<(String, DefId)> = m
        .defs
        .get(d)
        .map(|def| def.entities.instances.values().map(|i| (entry_name(m, i).to_lowercase(), i.def)).collect())
        .unwrap_or_default();
    let mut seen = HashSet::new();
    let found = kids.iter().any(|(n, _)| n.contains(filter))
        || kids.iter().any(|(_, k)| seen.insert(*k) && inside_matches(m, *k, filter, memo, depth + 1));
    memo.insert(d, found);
    found
}

/// Selects an entry: the group or component holding it opens first (`path` from the model).
fn select_at(app: &mut EsquisseApp, path: &[InstanceId]) {
    let Some((&last, parents)) = path.split_last() else { return };
    if app.doc.ctx != parents {
        app.doc.exit_all();
        for &i in parents {
            app.doc.enter(i);
        }
    }
    app.doc.select(vec![EntityRef::Instance(last)]);
}

pub fn show(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let id = egui::Id::unique("outliner_panel");
    let mut st: State = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        let l = ui.label("Filtre :");
        let r = ui.add(egui::TextEdit::singleline(&mut st.filter).desired_width(150.0)).labelled_by(l.id);
        crate::dialogs::name_widget(ui, &r, "Filtre :", Some(&st.filter.clone()));
        ui.menu_button("☰", |ui| {
            if ui.button("Tout développer").clicked() {
                (st.all_open, st.toggled) = (true, HashSet::new());
                ui.close();
            }
            if ui.button("Tout réduire").clicked() {
                (st.all_open, st.toggled) = (false, HashSet::new());
                ui.close();
            }
            let mut sorted = !st.unsorted;
            if ui.checkbox(&mut sorted, "Trier par nom").clicked() {
                st.unsorted = !sorted;
                ui.close();
            }
        });
    });
    let filter = st.filter.trim().to_lowercase();
    // What matches, worked out again only when the filter or the model changed.
    let rev = app.doc.rev;
    let mut memo = match st.memo.take() {
        Some((f, r, memo)) if f == filter && r == rev => memo,
        _ => HashMap::new(),
    };
    let root: Vec<(InstanceId, Instance)> = app.doc.model.root.instances.iter().map(|(k, v)| (k, v.clone())).collect();
    if root.is_empty() {
        ui.label("Aucun groupe ni composant.");
    }
    let mut actions = Vec::new();
    let mut drawn = 0;
    let mut cx = Rows { st: &st, filter: &filter, memo: &mut memo, actions: &mut actions, drawn: &mut drawn };
    rows(app, ui, &mut cx, &root, &mut Vec::new());
    st.memo = Some((filter, rev, memo));
    for a in actions {
        match a {
            Action::Select(path) => select_at(app, &path),
            Action::Toggle(path) => {
                if !st.toggled.remove(&path) {
                    st.toggled.insert(path);
                }
            }
            Action::Hide(path, hide) => {
                let Some((&last, parents)) = path.split_last() else { continue };
                let parents = parents.to_vec();
                app.doc.commit(if hide { "Masquer" } else { "Révéler" }, |m, _| {
                    let mut ents = &mut m.root;
                    for &i in &parents {
                        let Some(d) = ents.instances.get(i).map(|x| x.def) else { return };
                        ents = &mut m.defs[d].entities;
                    }
                    if let Some(x) = ents.instances.get_mut(last) {
                        x.hidden = hide;
                    }
                });
            }
        }
    }
    ui.data_mut(|d| d.insert_temp(id, st));
}

enum Action {
    Select(Vec<InstanceId>),
    Toggle(Vec<InstanceId>),
    Hide(Vec<InstanceId>, bool),
}

struct Rows<'a> {
    st: &'a State,
    filter: &'a str,
    memo: &'a mut HashMap<DefId, bool>,
    actions: &'a mut Vec<Action>,
    /// Rows drawn so far this frame.
    drawn: &'a mut usize,
}

fn rows(
    app: &EsquisseApp,
    ui: &mut egui::Ui,
    cx: &mut Rows,
    items: &[(InstanceId, Instance)],
    path: &mut Vec<InstanceId>,
) {
    let m = &app.doc.model;
    let mut list: Vec<(InstanceId, &Instance, String)> =
        items.iter().map(|(i, inst)| (*i, inst, entry_name(m, inst))).collect();
    if !cx.st.unsorted {
        list.sort_by_key(|x| x.2.to_lowercase());
    }
    for (i, inst, name) in list {
        let own = cx.filter.is_empty() || name.to_lowercase().contains(cx.filter);
        let inside = !cx.filter.is_empty() && inside_matches(m, inst.def, cx.filter, cx.memo, 0);
        if !own && !inside {
            continue;
        }
        if *cx.drawn >= MAX_ROWS {
            if *cx.drawn == MAX_ROWS {
                ui.label("…");
                *cx.drawn += 1;
            }
            return;
        }
        *cx.drawn += 1;
        path.push(i);
        let has_kids = m.defs.get(inst.def).is_some_and(|d| !d.entities.instances.is_empty());
        // A filter opens what leads to the matching entries.
        let open = has_kids && (inside || (cx.st.all_open != cx.st.toggled.contains(path.as_slice())));
        let selected = app.doc.ctx.as_slice() == &path[..path.len() - 1] && app.doc.is_selected(EntityRef::Instance(i));
        let is_group = m.defs.get(inst.def).is_some_and(|d| d.is_group);
        ui.horizontal(|ui| {
            ui.add_space(14.0 * (path.len() - 1) as f32);
            if has_kids {
                let arrow = ui.selectable_label(false, if open { "▾" } else { "▸" });
                crate::dialogs::name_widget(ui, &arrow, &format!("Développer {name}"), None);
                if arrow.clicked() {
                    cx.actions.push(Action::Toggle(path.clone()));
                }
            } else {
                ui.add_space(14.0);
            }
            let eye = ui.selectable_label(false, if inst.hidden { "◌" } else { "👁" });
            let label = if inst.hidden { "Révéler" } else { "Masquer" };
            crate::dialogs::name_widget(ui, &eye, &format!("{label} {name}"), None);
            if eye.on_hover_text(label).clicked() {
                cx.actions.push(Action::Hide(path.clone(), !inst.hidden));
            }
            let mut text = RichText::new(&name);
            if !is_group {
                text = text.italics();
            }
            if inst.hidden {
                text = text.weak();
            }
            let r = ui.selectable_label(selected, text).on_hover_text(if is_group { "Groupe" } else { "Composant" });
            if inst.locked {
                ui.label("🔒").on_hover_text("Verrouillé");
            }
            if r.clicked() {
                cx.actions.push(Action::Select(path.clone()));
            }
        });
        if open && path.len() < 64 {
            let kids: Vec<(InstanceId, Instance)> = m
                .defs
                .get(inst.def)
                .map(|d| d.entities.instances.iter().map(|(k, v)| (k, v.clone())).collect())
                .unwrap_or_default();
            rows(app, ui, cx, &kids, path);
        }
        path.pop();
    }
}
