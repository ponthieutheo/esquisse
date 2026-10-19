//! Preferences › Shortcuts: SketchUp's shortcut editor. Filter the functions, pick one,
//! press a key combination in "Add Shortcut" and "+" it; "-" removes an assigned shortcut;
//! "Reset All" brings SketchUp's back. A shortcut used elsewhere is moved after a question.

use eframe::egui::{self, Key, RichText};

use super::{group, name_widget};
use crate::app::EsquisseApp;
use crate::commands::Cmd;
use crate::shortcuts::{self, RESERVED, Shortcut};

#[derive(Default)]
pub struct ShortcutState {
    pub filter: String,
    pub selected: Option<Cmd>,
    /// The combination pressed in "Add Shortcut".
    pub captured: Option<Shortcut>,
    /// The assigned shortcut chosen for "-".
    pub assigned: Option<Shortcut>,
    /// "Already assigned to …": the command asking, the shortcut, its present owner.
    pub conflict: Option<(Cmd, Shortcut, Cmd)>,
    pub confirm_reset: bool,
}

/// "+": gives the chosen function the captured shortcut (or asks first).
pub fn add(app: &mut EsquisseApp) {
    let st = &mut app.ui_state.extra.dialogs.prefs.shortcuts;
    let (Some(cmd), Some(s)) = (st.selected, st.captured) else { return };
    if RESERVED.contains(&s.1) {
        app.message = Some(("Ce raccourci ne peut pas être réaffecté.".into(), 0.0));
        return;
    }
    match shortcuts::owner(s, Some(cmd)) {
        Some(o) => st.conflict = Some((cmd, s, o)),
        None => {
            shortcuts::assign(cmd, s);
            st.captured = None;
        }
    }
}

pub fn shortcuts_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let paths = crate::menus::command_paths();
    let st = &mut app.ui_state.extra.dialogs.prefs.shortcuts;
    ui.horizontal(|ui| {
        ui.label("Filtre");
        let r = ui.add(egui::TextEdit::singleline(&mut st.filter).desired_width(300.0));
        name_widget(ui, &r, "Filtre", None);
    });
    let q = st.filter.to_lowercase();
    let mut do_add = false;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new("Fonction").strong());
            egui::ScrollArea::vertical().id_salt("shortcut_functions").max_height(300.0).show(ui, |ui| {
                ui.set_width(300.0);
                for (path, c) in &paths {
                    let leaf = path.replace(" › ", "/");
                    if !q.is_empty() && !leaf.to_lowercase().contains(&q) {
                        continue;
                    }
                    let r = ui.selectable_label(st.selected == Some(*c), &leaf);
                    name_widget(ui, &r, &leaf, Some(&crate::commands::Cmd::shortcut_text(*c)));
                    if r.clicked() {
                        st.selected = Some(*c);
                        st.assigned = None;
                    }
                }
            });
        });
        ui.vertical(|ui| {
            ui.label(RichText::new("Ajouter un raccourci").strong());
            ui.horizontal(|ui| {
                // A text field takes the keyboard (no command runs); the combination pressed
                // replaces what it shows.
                let mut shown = st.captured.map(shortcuts::text).unwrap_or_default();
                let r = ui.add(egui::TextEdit::singleline(&mut shown).desired_width(150.0).hint_text("Appuyez…"));
                name_widget(ui, &r, "Ajouter un raccourci", Some(&shown));
                if r.has_focus() {
                    let pressed = ui.input(|i| {
                        i.events.iter().rev().find_map(|e| match e {
                            egui::Event::Key { key, pressed: true, modifiers, .. } if *key != Key::Escape => {
                                Some((*modifiers, *key))
                            }
                            _ => None,
                        })
                    });
                    if let Some(p) = pressed {
                        st.captured = Some(p);
                    }
                }
                let plus = ui.add_enabled(st.selected.is_some() && st.captured.is_some(), egui::Button::new("+"));
                name_widget(ui, &plus, "Ajouter le raccourci", None);
                if plus.clicked() {
                    st.assigned = None;
                    st.confirm_reset = false;
                    do_add = true;
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Raccourcis affectés").strong());
            let assigned = st.selected.map(shortcuts::bindings).unwrap_or_default();
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_size(egui::vec2(150.0, 80.0));
                for s in &assigned {
                    let t = shortcuts::text(*s);
                    let r = ui.selectable_label(st.assigned == Some(*s), &t);
                    name_widget(ui, &r, &format!("Raccourci {t}"), Some(&t));
                    if r.clicked() {
                        st.assigned = Some(*s);
                    }
                }
            });
            let minus = ui.add_enabled(st.assigned.is_some(), egui::Button::new("-"));
            name_widget(ui, &minus, "Supprimer le raccourci", None);
            if minus.clicked()
                && let (Some(c), Some(s)) = (st.selected, st.assigned.take())
            {
                shortcuts::remove(c, s);
            }
        });
    });
    if do_add {
        add(app);
    }
    let st = &mut app.ui_state.extra.dialogs.prefs.shortcuts;
    if let Some((cmd, s, owner)) = st.conflict {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(format!(
                "Le raccourci {} est déjà affecté à « {} ». Voulez-vous le réaffecter à « {} » ?",
                shortcuts::text(s),
                owner.label(),
                cmd.label()
            ));
            ui.horizontal(|ui| {
                if ui.button("Oui").clicked() {
                    shortcuts::assign(cmd, s);
                    st.conflict = None;
                    st.captured = None;
                }
                if ui.button("Non").clicked() {
                    st.conflict = None;
                }
            });
        });
    }
    group(ui, "");
    if !st.confirm_reset {
        if ui
            .button("Tout réinitialiser")
            .on_hover_text("Réinitialise les préférences des raccourcis aux paramètres par défaut.")
            .clicked()
        {
            st.confirm_reset = true;
        }
    } else {
        ui.horizontal(|ui| {
            ui.label("Rétablir tous les raccourcis de SketchUp ?");
            if ui.button("Réinitialiser tous les raccourcis").clicked() {
                shortcuts::reset_all();
                st.confirm_reset = false;
            }
            if ui.button("Ne pas réinitialiser").clicked() {
                st.confirm_reset = false;
            }
        });
    }
}
