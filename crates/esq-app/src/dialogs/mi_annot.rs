//! Model Info › Text and Model Info › Dimensions: the look of new texts and dimensions, the
//! buttons that select them all or give the selected ones that look, and the expert
//! dimension settings. Defaults are kept with the model; "Update selected" writes each
//! annotation's own properties (those of the Ruby API's `Text#font`, `#arrow_type`…).

use eframe::egui;
use esq_core::model::{AnnotKind, AttrValue, EntityRef, Model};

use super::{combo, group, name_widget};
use crate::annot::{
    ARROWS, DimDefaults, TextDefaults, TextFont, apply_style, dim_defaults, set_dim_defaults, set_text_defaults,
    text_defaults,
};
use crate::app::EsquisseApp;

/// Which font the font editor ("Polices...") is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontTarget {
    Screen,
    Leader,
    Dimension,
    /// Entity Info's text or dimension (« Changer de police... »).
    Selected,
}

/// Kinds of annotations the "Select all" buttons take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotSort {
    Screen,
    Leader,
    Dimension,
}

fn sort_of(k: &AnnotKind) -> AnnotSort {
    match k {
        AnnotKind::Linear { .. } => AnnotSort::Dimension,
        AnnotKind::Text { .. } => AnnotSort::Leader,
        AnnotKind::ScreenText { .. } => AnnotSort::Screen,
    }
}

/// Selects every annotation of a kind in the active context.
pub fn select_all(app: &mut EsquisseApp, sort: AnnotSort) {
    let sel: Vec<EntityRef> = app
        .doc
        .ents()
        .annots
        .iter()
        .filter(|(_, a)| !a.hidden && sort_of(&a.kind) == sort)
        .map(|(id, _)| EntityRef::Annot(id))
        .collect();
    app.doc.select(sel);
}

/// Before the defaults change, the annotations that follow them keep their present look
/// (in SketchUp, Model Info only changes new texts and dimensions).
fn freeze_existing(m: &mut Model) {
    let (t, d) = (text_defaults(m), dim_defaults(m));
    let mut all: Vec<&mut esq_core::model::Entities> = vec![&mut m.root];
    all.extend(m.defs.values_mut().map(|d| &mut d.entities));
    for ents in all {
        let ids: Vec<_> = ents
            .annots
            .iter()
            .filter(|(id, _)| ents.prop(&crate::annot::prop_key(*id), "font_name").is_none())
            .map(|(id, a)| (id, sort_of(&a.kind)))
            .collect();
        for (id, sort) in ids {
            match sort {
                AnnotSort::Screen => apply_style(ents, id, &t.screen, 0, &[]),
                AnnotSort::Leader => apply_style(
                    ents,
                    id,
                    &t.leader,
                    t.arrow,
                    &[("leader_type", AttrValue::Number(t.leader_type as f64))],
                ),
                AnnotSort::Dimension => apply_style(
                    ents,
                    id,
                    &d.font,
                    d.arrow,
                    &[
                        ("aligned", AttrValue::Bool(!d.align_screen)),
                        ("aligned_text_position", AttrValue::Number(d.position as f64)),
                    ],
                ),
            }
        }
    }
}

pub fn set_text(app: &mut EsquisseApp, d: &TextDefaults) {
    let d = d.clone();
    app.doc.commit("Style de texte", |m, _| {
        freeze_existing(m);
        set_text_defaults(m, &d);
    });
}

pub fn set_dims(app: &mut EsquisseApp, d: &DimDefaults) {
    let d = d.clone();
    app.doc.commit("Style de cotation", |m, _| {
        freeze_existing(m);
        set_dim_defaults(m, &d);
    });
}

/// "Update selected text" / "Update selected dimensions": the selected annotations of the
/// page's kinds take the page's look.
pub fn update_selected(app: &mut EsquisseApp, dims: bool) {
    let (t, d) = (text_defaults(&app.doc.model), dim_defaults(&app.doc.model));
    let sel = app.doc.sel.clone();
    app.doc.commit(if dims { "Actualiser les cotations" } else { "Actualiser le texte" }, |m, ctx| {
        let ents = m.entities_mut(ctx);
        for e in sel {
            let EntityRef::Annot(id) = e else { continue };
            let Some(sort) = ents.annots.get(id).map(|a| sort_of(&a.kind)) else { continue };
            match (sort, dims) {
                (AnnotSort::Screen, false) => apply_style(ents, id, &t.screen, 0, &[]),
                (AnnotSort::Leader, false) => apply_style(
                    ents,
                    id,
                    &t.leader,
                    t.arrow,
                    &[("leader_type", AttrValue::Number(t.leader_type as f64))],
                ),
                (AnnotSort::Dimension, true) => apply_style(
                    ents,
                    id,
                    &d.font,
                    d.arrow,
                    &[
                        ("aligned", AttrValue::Bool(!d.align_screen)),
                        ("aligned_text_position", AttrValue::Number(d.position as f64)),
                    ],
                ),
                _ => {}
            }
        }
    });
}

/// The font line of a page: description, "Polices..." and the colour. True when changed.
pub(crate) fn font_row(
    app: &mut EsquisseApp,
    ui: &mut egui::Ui,
    target: FontTarget,
    f: &mut TextFont,
    name: &str,
    button: &str,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let r = egui::Frame::new()
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(190)))
            .inner_margin(egui::Margin::symmetric(6, 2))
            .show(ui, |ui| {
                ui.set_min_width(220.0);
                ui.label(f.describe())
            })
            .inner;
        name_widget(ui, &r, &format!("Police {name}"), Some(&f.describe()));
        let open = app.ui_state.extra.dialogs.info.font_editor == Some(target);
        let b = ui.selectable_label(open, button);
        name_widget(ui, &b, &format!("Polices {name}"), None);
        if b.clicked() {
            app.ui_state.extra.dialogs.info.font_editor = if open { None } else { Some(target) };
        }
        let mut c = f.color;
        let r = ui.color_edit_button_srgb(&mut c);
        name_widget(ui, &r, &format!("Couleur {name}"), None);
        if c != f.color {
            f.color = c;
            changed = true;
        }
    });
    if app.ui_state.extra.dialogs.info.font_editor == Some(target) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            let families: Vec<(usize, String)> =
                crate::fonts::font_list().families.keys().cloned().enumerate().collect();
            let mut i = families.iter().position(|(_, n)| *n == f.name).unwrap_or(usize::MAX);
            ui.horizontal(|ui| {
                ui.label("Police");
                if combo(ui, &format!("Famille {name}"), &mut i, &families)
                    && let Some((_, n)) = families.get(i)
                {
                    f.name = n.clone();
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Taille");
                let r = ui.add(egui::DragValue::new(&mut f.size).range(1.0..=200.0).speed(0.5).suffix(" points"));
                name_widget(ui, &r, &format!("Taille {name}"), Some(&format!("{}", f.size)));
                changed |= r.changed();
                changed |= ui.checkbox(&mut f.bold, "Gras").changed();
                changed |= ui.checkbox(&mut f.italic, "Italique").changed();
            });
        });
    }
    changed
}

fn arrow_choices(with_slash: bool) -> Vec<(u8, String)> {
    ARROWS.iter().enumerate().filter(|(i, _)| with_slash || *i != 1).map(|(i, s)| (i as u8, s.to_string())).collect()
}

pub fn text_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let before = text_defaults(&app.doc.model);
    let mut d = before.clone();
    group(ui, "Texte d'écran");
    font_row(app, ui, FontTarget::Screen, &mut d.screen, "du texte d'écran", "Polices...");
    if ui.button("Sélectionner tout le texte d'écran").clicked() {
        select_all(app, AnnotSort::Screen);
    }
    group(ui, "Texte de légende");
    font_row(app, ui, FontTarget::Leader, &mut d.leader, "du texte de légende", "Polices...");
    if ui.button("Sélectionner tout le texte des légendes").clicked() {
        select_all(app, AnnotSort::Leader);
    }
    group(ui, "Traits de légendes");
    egui::Grid::new("leader_grid").num_columns(2).show(ui, |ui| {
        ui.label("Extrémité :");
        combo(ui, "Extrémité", &mut d.arrow, &arrow_choices(false));
        ui.end_row();
        ui.label("Légende :");
        let kinds = [(1u8, "Suivant la vue".to_string()), (2u8, "Punaise".to_string())];
        combo(ui, "Légende", &mut d.leader_type, &kinds);
        ui.end_row();
    });
    ui.add_space(6.0);
    let any = app.doc.sel.iter().any(|e| {
        matches!(e, EntityRef::Annot(a) if app.doc.ents().annots.get(*a).is_some_and(|x| sort_of(&x.kind) != AnnotSort::Dimension))
    });
    if d != before {
        set_text(app, &d);
    }
    if ui.add_enabled(any, egui::Button::new("Actualiser le texte sélectionné")).clicked() {
        update_selected(app, false);
    }
}

pub fn dims_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let before = dim_defaults(&app.doc.model);
    let mut d = before.clone();
    group(ui, "Texte");
    font_row(app, ui, FontTarget::Dimension, &mut d.font, "des cotations", "Polices...");
    group(ui, "Traits de légendes");
    ui.horizontal(|ui| {
        ui.label("Extrémités :");
        combo(ui, "Extrémités", &mut d.arrow, &arrow_choices(true));
    });
    group(ui, "Cotation");
    ui.radio_value(&mut d.align_screen, true, "Aligner sur l'écran");
    ui.horizontal(|ui| {
        ui.radio_value(&mut d.align_screen, false, "Aligner sur la ligne de cotation");
        let pos = [(0u8, "Au-dessus".to_string()), (1, "Centré".to_string()), (2, "Extérieur".to_string())];
        ui.add_enabled_ui(!d.align_screen, |ui| combo(ui, "Position du texte", &mut d.position, &pos));
    });
    ui.add_space(4.0);
    if ui.button("Sélectionner toutes les cotations").clicked() {
        select_all(app, AnnotSort::Dimension);
    }
    let any = app.doc.sel.iter().any(|e| {
        matches!(e, EntityRef::Annot(a) if app.doc.ents().annots.get(*a).is_some_and(|x| sort_of(&x.kind) == AnnotSort::Dimension))
    });
    if ui.add_enabled(any, egui::Button::new("Actualiser les cotations sélectionnées")).clicked() {
        update_selected(app, true);
    }
    let st = &mut app.ui_state.extra.dialogs.info;
    if ui.selectable_label(st.expert_dims, "Paramètres de cotation avancés").clicked() {
        st.expert_dims = !st.expert_dims;
    }
    if app.ui_state.extra.dialogs.info.expert_dims {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(egui::RichText::new("Paramètres de cotation avancés").strong());
            ui.checkbox(&mut d.show_prefix, "Afficher le préfixe rayon/diamètre");
            ui.checkbox(&mut d.hide_foreshortened, "Masquer en vue perspective");
            ui.checkbox(&mut d.hide_small, "Masquer si réduction à l'affichage");
            ui.label(egui::RichText::new("Dépannage").strong());
            ui.horizontal(|ui| {
                ui.checkbox(&mut d.highlight_nonassociated, "Mettre les cotations non associées en évidence");
                let r = ui.color_edit_button_srgb(&mut d.highlight_color);
                name_widget(ui, &r, "Couleur des cotations non associées", None);
            });
        });
    }
    if d != before {
        set_dims(app, &d);
    }
}
