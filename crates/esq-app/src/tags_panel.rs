//! Window › Tags (SketchUp's `TagDialog`): the tags and their folders, with their visibility,
//! colour and dashes; adding tags and folders, deleting them (and what their entities
//! become), purging; Color by Tag and Show New Tags in the details menu.

use eframe::egui::{self, RichText};
use esq_core::meta::LayerFolder;
use esq_core::model::Model;
use serde_json::{Value as J, json};

use crate::app::EsquisseApp;
use crate::scene::{DASHES, TAG_GREY, dash_pattern};
use crate::view_cmds::ViewCmd;

/// Colours given to new tags, in turn.
pub const TAG_PALETTE: [[u8; 3]; 12] = [
    [220, 76, 70],
    [77, 145, 214],
    [96, 178, 92],
    [235, 167, 54],
    [154, 98, 198],
    [62, 184, 184],
    [214, 112, 166],
    [140, 120, 86],
    [120, 168, 52],
    [202, 140, 214],
    [70, 110, 160],
    [190, 190, 70],
];

/// Whether tags added to the model are visible in its existing scenes (Show New Tags).
pub fn show_new_tags(m: &Model) -> bool {
    m.view_ext.get("ShowNewTags").and_then(J::as_bool).unwrap_or(true)
}

#[derive(Clone, Default)]
struct PanelState {
    selected: Option<u32>,
    renaming: Option<(u32, String)>,
}

impl EsquisseApp {
    /// Tags › Add Tag: "Étiquette", "Étiquette1"… with the next colour; hidden in the
    /// existing scenes unless Show New Tags is on. In folder `folder` when given.
    pub fn add_tag(&mut self, folder: Option<u32>) -> usize {
        let mut idx = 0;
        let show = show_new_tags(&self.doc.model);
        self.doc.commit("Ajouter une étiquette", |m, _| {
            let taken: Vec<String> = m.layers.iter().map(|l| l.name.clone()).collect();
            let name = Model::unique_name("Étiquette", &taken);
            let colour = TAG_PALETTE[(m.layers.len() - 1) % TAG_PALETTE.len()];
            idx = m.add_layer(&name);
            m.layers[idx].color = Some(colour);
            m.layers[idx].folder = folder;
            m.layers[idx].line_style = DASHES[0].0.to_string();
            if !show {
                for s in m.scenes.iter_mut() {
                    if let Some(h) = &mut s.hidden_layers {
                        h.push(idx);
                    }
                }
            }
        });
        idx
    }

    /// Tags › Add Tag Folder.
    pub fn add_tag_folder(&mut self) -> u32 {
        let mut uid = 0;
        self.doc.commit("Ajouter un dossier d'étiquettes", |m, _| {
            let taken: Vec<String> = m.meta.folders.iter().map(|f| f.name.clone()).collect();
            let name = Model::unique_name("Dossier", &taken);
            uid = m.new_uid();
            m.meta.folders.push(LayerFolder { uid, name, parent: None, visible: true, visible_on_new_pages: true });
        });
        uid
    }

    /// Shows or hides a tag folder and everything in it.
    pub fn set_folder_visible(&mut self, uid: u32, visible: bool) {
        self.doc.commit("Visibilité du dossier d'étiquettes", |m, _| {
            let mut stack = vec![uid];
            while let Some(u) = stack.pop() {
                for l in m.layers.iter_mut().filter(|l| l.folder == Some(u)) {
                    l.visible = visible;
                }
                for f in m.meta.folders.iter_mut().filter(|f| f.uid == u) {
                    f.visible = visible;
                }
                stack.extend(m.meta.folders.iter().filter(|f| f.parent == Some(u)).map(|f| f.uid));
            }
        });
    }

    /// Tags › Purge: the unused tags and empty folders.
    pub fn purge_tags(&mut self) -> usize {
        let mut n = 0;
        self.doc.commit("Purger les étiquettes", |m, _| {
            for i in m.unused_layers() {
                m.remove_layer(i, false);
                n += 1;
            }
            loop {
                let empty: Vec<u32> = m
                    .meta
                    .folders
                    .iter()
                    .filter(|f| {
                        !m.layers.iter().any(|l| l.folder == Some(f.uid))
                            && !m.meta.folders.iter().any(|x| x.parent == Some(f.uid))
                    })
                    .map(|f| f.uid)
                    .collect();
                if empty.is_empty() {
                    break;
                }
                n += empty.len();
                m.meta.folders.retain(|f| !empty.contains(&f.uid));
            }
        });
        n
    }

    /// `show new tags`: new tags visible in the existing scenes or not.
    pub fn toggle_show_new_tags(&mut self) {
        let on = !show_new_tags(&self.doc.model);
        self.doc.commit("Afficher les nouvelles étiquettes", |m, _| {
            m.view_ext.insert("ShowNewTags".into(), json!(on));
        });
    }
}

/// Window › Tags.
pub fn show(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let id = egui::Id::unique("tags_panel");
    let mut st: PanelState = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    app.doc.model.ensure_layer_uids();
    let ctx = ui.ctx().clone();
    ui.horizontal(|ui| {
        if ui.button("+").on_hover_text("Ajouter une étiquette").clicked() {
            let folder = st.selected.and_then(|u| app.doc.model.meta.folders.iter().any(|f| f.uid == u).then_some(u));
            let i = app.add_tag(folder);
            let uid = app.doc.model.layers[i].uid;
            st.renaming = Some((uid, app.doc.model.layers[i].name.clone()));
            st.selected = Some(uid);
        }
        if ui.button("📁").on_hover_text("Ajouter un dossier d'étiquettes").clicked() {
            st.selected = Some(app.add_tag_folder());
        }
        let deletable = st.selected.and_then(|u| app.doc.model.layer_index_of_uid(u)).filter(|i| *i > 0);
        let folder_sel = st.selected.filter(|u| app.doc.model.meta.folders.iter().any(|f| f.uid == *u));
        let r = ui
            .add_enabled(deletable.is_some() || folder_sel.is_some(), egui::Button::new("−"))
            .on_hover_text("Supprimer l'étiquette");
        crate::dialogs::name_widget(ui, &r, "Supprimer l'étiquette", None);
        if r.clicked() {
            if let Some(i) = deletable {
                // `LayerDeleteDlg`: asked when the tag tags entities (`dialogs::tag_dialog`).
                crate::dialogs::tag_dialog::ask_delete(app, i);
                st.selected = None;
            } else if let Some(u) = folder_sel {
                app.doc.commit("Supprimer le dossier d'étiquettes", |m, _| m.remove_folder(u));
                st.selected = None;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("☰", |ui| {
                if ui.button("Tout sélectionner").clicked() {
                    ui.close();
                    select_tagged(app, st.selected.and_then(|u| app.doc.model.layer_index_of_uid(u)));
                }
                if ui.button("Purger").clicked() {
                    ui.close();
                    let n = app.purge_tags();
                    app.say(&ctx, format!("{n} étiquette(s) ou dossier(s) supprimé(s)"));
                }
                let mut by_tag = app.style.color_by_tag;
                if ui.checkbox(&mut by_tag, "Colorer par étiquette").changed() {
                    app.run(&ctx, crate::commands::Cmd::View(ViewCmd::ColorByTag));
                }
                let mut show = show_new_tags(&app.doc.model);
                if ui
                    .checkbox(&mut show, "Afficher les nouvelles étiquettes")
                    .on_hover_text("Les nouvelles étiquettes sont visibles dans les scènes existantes")
                    .changed()
                {
                    app.toggle_show_new_tags();
                }
            });
        });
    });
    // The table: folders (with what they hold), then the tags outside folders.
    egui::Grid::new("tags_table").num_columns(4).striped(true).show(ui, |ui| {
        ui.label(RichText::new("Nom").small());
        ui.label(RichText::new("").small());
        ui.label(RichText::new("Couleur").small());
        ui.label(RichText::new("Tirets").small());
        ui.end_row();
        rows(app, ui, &mut st, None, 0);
    });
    ui.data_mut(|d| d.insert_temp(id, st));
}

/// Selects everything on a tag (Tags › Select All), in the open context.
fn select_tagged(app: &mut EsquisseApp, tag: Option<usize>) {
    use esq_core::model::EntityRef;
    let Some(t) = tag else { return };
    let e = app.doc.ents();
    let mut sel: Vec<EntityRef> =
        e.faces.iter().filter(|(_, x)| x.layer == t).map(|(k, _)| EntityRef::Face(k)).collect();
    sel.extend(e.edges.iter().filter(|(_, x)| x.layer == t).map(|(k, _)| EntityRef::Edge(k)));
    sel.extend(e.instances.iter().filter(|(_, x)| x.layer == t).map(|(k, _)| EntityRef::Instance(k)));
    sel.extend(e.guides.iter().filter(|(_, x)| x.layer == t).map(|(k, _)| EntityRef::Guide(k)));
    sel.extend(e.annots.iter().filter(|(_, x)| x.layer == t).map(|(k, _)| EntityRef::Annot(k)));
    app.doc.select(sel);
}

fn rows(app: &mut EsquisseApp, ui: &mut egui::Ui, st: &mut PanelState, folder: Option<u32>, depth: usize) {
    if depth > 16 {
        return;
    }
    let indent = depth as f32 * 14.0;
    let folders: Vec<LayerFolder> = app.doc.model.meta.folders.iter().filter(|f| f.parent == folder).cloned().collect();
    for f in folders {
        ui.horizontal(|ui| {
            ui.add_space(indent);
            let mut v = f.visible;
            if ui.checkbox(&mut v, "").on_hover_text("Visible").changed() {
                app.set_folder_visible(f.uid, v);
            }
            let text = RichText::new(format!("📁 {}", f.name)).strong();
            if ui.selectable_label(st.selected == Some(f.uid), text).clicked() {
                st.selected = Some(f.uid);
            }
        });
        ui.label("");
        ui.label("");
        ui.label("");
        ui.end_row();
        rows(app, ui, st, Some(f.uid), depth + 1);
    }
    let tags: Vec<usize> =
        (0..app.doc.model.layers.len()).filter(|&i| app.doc.model.layers[i].folder == folder).collect();
    let active = crate::dialogs::tag_dialog::active(&app.doc.model);
    for i in tags {
        let l = app.doc.model.layers[i].clone();
        ui.horizontal(|ui| {
            ui.add_space(indent);
            let mut v = l.visible;
            let r = ui.checkbox(&mut v, "").on_hover_text("Visible");
            crate::dialogs::name_widget(ui, &r, &format!("Visible : {}", l.name), None);
            if r.changed() {
                if i == active && !v {
                    // SketchUp's « Warn Hide Current Layer » (`FuturePromptDlg`, preference
                    // `NoPromptHideCurLayer`): the tag stays visible.
                    crate::warnings::queue(crate::warnings::Message::question(
                        "NoPromptHideCurLayer",
                        "Esquisse",
                        "Vous ne pouvez pas masquer l'étiquette actuelle.",
                        &["OK"],
                        "Ne plus proposer",
                    ));
                } else {
                    app.doc.commit("Visibilité de l'étiquette", |m, _| m.layers[i].visible = v);
                }
            }
            match &mut st.renaming {
                Some((u, text)) if *u == l.uid && i > 0 => {
                    let r = ui.text_edit_singleline(text);
                    if r.lost_focus() {
                        let name = text.trim().to_string();
                        if !name.is_empty() && !app.doc.model.layers.iter().any(|x| x.name == name && x.uid != l.uid) {
                            app.doc.commit("Renommer l'étiquette", |m, _| {
                                let old = std::mem::replace(&mut m.layers[i].name, name.clone());
                                m.rename_attr_owner(&format!("layer:{old}"), &format!("layer:{name}"));
                            });
                        }
                        st.renaming = None;
                    } else {
                        r.request_focus();
                    }
                }
                _ => {
                    let r = ui.selectable_label(st.selected == Some(l.uid), &l.name);
                    if r.clicked() {
                        st.selected = Some(l.uid);
                    }
                    if r.double_clicked() && i > 0 {
                        st.renaming = Some((l.uid, l.name.clone()));
                    }
                }
            }
        });
        // The pencil: the active tag, the one new objects receive (`dialogs::tag_dialog`).
        let r = ui.radio(i == active, "✏").on_hover_text("Modifier l'étiquette actuelle.");
        crate::dialogs::name_widget(ui, &r, &format!("Étiquette active : {}", l.name), None);
        if r.clicked() {
            crate::dialogs::tag_dialog::set_active(app, i);
        }
        let c = l.color.unwrap_or(TAG_GREY);
        let mut edit = c;
        ui.color_edit_button_srgb(&mut edit).on_hover_text("Couleur de l'étiquette");
        if edit != c {
            app.doc.commit("Couleur de l'étiquette", |m, _| m.layers[i].color = Some(edit));
        }
        let cur = dash_pattern(&l.line_style) as usize;
        let mut pick = cur;
        egui::ComboBox::from_id_salt(("dashes", l.uid)).selected_text(DASHES[cur].1).width(110.0).show_ui(ui, |ui| {
            for (k, (_, label)) in DASHES.iter().enumerate() {
                ui.selectable_value(&mut pick, k, *label);
            }
        });
        if pick != cur {
            app.doc.commit("Tirets de l'étiquette", |m, _| m.layers[i].line_style = DASHES[pick].0.to_string());
        }
        ui.end_row();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::glam::DVec3;
    use esq_core::model::Entities;

    #[test]
    fn tags_are_added_coloured_hidden_in_scenes_deleted_and_purged() {
        let mut app = EsquisseApp::bare(None, None);
        let a = app.add_tag(None);
        let b = app.add_tag(None);
        let m = &app.doc.model;
        assert_eq!((m.layers[a].name.as_str(), m.layers[b].name.as_str()), ("Étiquette", "Étiquette1"));
        assert_ne!(m.layers[a].color, m.layers[b].color);
        assert_eq!(dash_pattern(&m.layers[a].line_style), 0);
        app.doc.undo();
        assert_eq!(app.doc.model.layers.len(), 2, "undoable");
        // Show New Tags off: a new tag is hidden in the scenes that exist.
        let ctx = egui::Context::default();
        app.run(&ctx, crate::commands::Cmd::AddScene);
        app.toggle_show_new_tags();
        let c = app.add_tag(None);
        assert_eq!(app.doc.model.scenes[0].hidden_layers.as_deref(), Some(&[c][..]));
        // Deleting a tag with entities: they go to another tag, or are erased.
        let mut e = Entities::default();
        let v1 = e.add_vertex_raw(DVec3::ZERO);
        let v2 = e.add_vertex_raw(DVec3::X);
        let edge = e.add_edge_raw(v1, v2).unwrap();
        e.edges[edge].layer = c;
        app.doc.model.root = e;
        app.doc.commit("t", |m, _| crate::dialogs::tag_dialog::delete_tag(m, c, Some(a)));
        assert_eq!(app.doc.model.root.edges[edge].layer, a);
        app.doc.commit("t", |m, _| crate::dialogs::tag_dialog::delete_tag(m, a, None));
        assert!(app.doc.model.root.edges.is_empty());
        // Folders: hiding one hides its tags; purge removes unused tags and empty folders.
        let f = app.add_tag_folder();
        let t = app.add_tag(Some(f));
        app.set_folder_visible(f, false);
        assert!(!app.doc.model.layers[t].visible);
        let empty = app.add_tag_folder();
        assert!(app.purge_tags() >= 3, "tags and folders");
        assert!(!app.doc.model.meta.folders.iter().any(|x| x.uid == empty || x.uid == f));
        assert_eq!(app.doc.model.layers.len(), 1);
    }
}
