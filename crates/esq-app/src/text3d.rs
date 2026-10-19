//! Tools › 3D Text. Like SketchUp, the dialog builds the text as a component and hands it
//! to the placement tool, which makes it follow the cursor until a click drops it.

use eframe::egui;
use esq_core::model::{DefId, Definition, Entities, Model, Units};
use esq_core::text3d::{self, Align, TextSpec};
use esq_core::units::{format_length, parse_length};

use crate::app::EsquisseApp;
use crate::fonts::{font_list, with_source};

/// What the dialog edits. Lengths stay as typed until the text is built.
#[derive(Clone, Debug, PartialEq)]
pub struct Text3dState {
    pub text: String,
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub align: Align,
    pub height: String,
    pub filled: bool,
    pub extruded: bool,
    pub extrusion: String,
    pub error: Option<String>,
}

impl Text3dState {
    /// SketchUp's defaults: 10" high, filled and extruded by 1".
    pub fn new(units: Units) -> Self {
        let list = font_list();
        let family = ["Arial", "Segoe UI"]
            .into_iter()
            .find(|f| list.families.contains_key(*f))
            .map(str::to_string)
            .or_else(|| list.families.keys().next().cloned())
            .unwrap_or_default();
        let d = TextSpec::default();
        Text3dState {
            text: "Entrez le texte".into(),
            family,
            bold: false,
            italic: false,
            align: Align::Left,
            height: format_length(d.height, units),
            filled: d.filled,
            extruded: true,
            extrusion: format_length(d.extrusion, units),
            error: None,
        }
    }

    pub fn spec(&self, units: Units) -> Result<TextSpec, String> {
        let height = parse_length(&self.height, units).ok_or("Hauteur invalide")?;
        let extrusion = if self.filled && self.extruded {
            parse_length(&self.extrusion, units).ok_or("Extrusion invalide")?
        } else {
            0.0
        };
        Ok(TextSpec {
            text: self.text.clone(),
            align: self.align,
            height,
            filled: self.filled,
            extrusion,
            ..TextSpec::default()
        })
    }
}

/// Builds the text as a new component definition of `model`.
pub fn make_definition(model: &mut Model, st: &Text3dState) -> Result<DefId, String> {
    let spec = st.spec(model.units)?;
    let file = font_list()
        .file(&st.family, st.bold, st.italic)
        .ok_or_else(|| format!("Police introuvable : {}", st.family))?;
    let ents = with_source(file, |src| {
        let mut e = Entities::default();
        text3d::build(src, &spec, &mut e);
        e
    })
    .ok_or("Impossible de lire la police")?;
    if ents.edges.is_empty() {
        return Err("Rien à créer : texte vide ou hauteur trop petite".into());
    }
    let name = model.unique_def_name("Texte 3D");
    Ok(model.defs.insert(Definition {
        name,
        is_group: false,
        description: String::new(),
        entities: ents,
        attributes: Default::default(),
        ext: Default::default(),
    }))
}

fn style_label(bold: bool, italic: bool) -> &'static str {
    match (bold, italic) {
        (false, false) => "Normal",
        (true, false) => "Gras",
        (false, true) => "Italique",
        (true, true) => "Gras italique",
    }
}

pub fn dialog(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(mut st) = app.ui_state.text3d.take() else { return };
    let list = font_list();
    let mut done = None;
    super::ui::centered("Placer le texte 3D").show(ctx, |ui| {
        let r = ui.add(egui::TextEdit::multiline(&mut st.text).desired_rows(3).desired_width(340.0));
        r.request_focus();
        egui::Grid::new("text3d").num_columns(2).show(ui, |ui| {
            ui.label("Police");
            egui::ComboBox::from_id_salt("t3d_font").width(220.0).selected_text(st.family.clone()).show_ui(ui, |ui| {
                for f in list.families.keys() {
                    ui.selectable_value(&mut st.family, f.clone(), f);
                }
            });
            ui.end_row();
            ui.label("Style");
            let styles = list.styles(&st.family);
            if !styles.contains(&(st.bold, st.italic)) {
                let fallback = styles.first().copied().unwrap_or((false, false));
                (st.bold, st.italic) = fallback;
            }
            egui::ComboBox::from_id_salt("t3d_style")
                .width(220.0)
                .selected_text(style_label(st.bold, st.italic))
                .show_ui(ui, |ui| {
                    for (b, i) in styles {
                        if ui.selectable_label((st.bold, st.italic) == (b, i), style_label(b, i)).clicked() {
                            (st.bold, st.italic) = (b, i);
                        }
                    }
                });
            ui.end_row();
            ui.label("Alignement");
            egui::ComboBox::from_id_salt("t3d_align")
                .width(220.0)
                .selected_text(match st.align {
                    Align::Left => "Gauche",
                    Align::Center => "Centre",
                    Align::Right => "Droite",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut st.align, Align::Left, "Gauche");
                    ui.selectable_value(&mut st.align, Align::Center, "Centre");
                    ui.selectable_value(&mut st.align, Align::Right, "Droite");
                });
            ui.end_row();
            ui.label("Hauteur");
            ui.add(egui::TextEdit::singleline(&mut st.height).desired_width(220.0));
            ui.end_row();
            ui.label("Forme");
            ui.checkbox(&mut st.filled, "Rempli");
            ui.end_row();
            ui.label("");
            ui.horizontal(|ui| {
                ui.add_enabled(st.filled, egui::Checkbox::new(&mut st.extruded, "Extrudé"));
                ui.add_enabled(
                    st.filled && st.extruded,
                    egui::TextEdit::singleline(&mut st.extrusion).desired_width(110.0),
                );
            });
            ui.end_row();
        });
        if let Some(e) = &st.error {
            ui.colored_label(egui::Color32::from_rgb(200, 40, 40), e);
        }
        ui.horizontal(|ui| {
            if ui.button("Placer").clicked() {
                done = Some(true);
            }
            if ui.button("Annuler").clicked() {
                done = Some(false);
            }
        });
    });
    match done {
        None => app.ui_state.text3d = Some(st),
        Some(false) => app.ui_state.text3d_last = Some(st),
        Some(true) => {
            let mut made = None;
            let res = app.doc.try_commit("Texte 3D", |m, _| {
                made = Some(make_definition(m, &st)?);
                Ok::<(), String>(())
            });
            match (res, made) {
                (Ok(()), Some(def)) => {
                    app.ui_state.text3d_last = Some(st);
                    app.place_component_glued(def);
                }
                (Err(e), _) => {
                    st.error = Some(e);
                    app.ui_state.text3d = Some(st);
                }
                (Ok(()), None) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(text: &str) -> Text3dState {
        let mut s = Text3dState::new(Units::Millimeters);
        s.family = "Arial".into();
        s.text = text.into();
        s.height = "100".into();
        s.extrusion = "10".into();
        s
    }

    fn has_arial() -> bool {
        font_list().families.contains_key("Arial")
    }

    #[test]
    fn defaults_are_sketchups() {
        let s = Text3dState::new(Units::Inches);
        assert_eq!(s.height, "10 \"");
        assert_eq!(s.extrusion, "1 \"");
        assert!(s.filled && s.extruded);
        let spec = s.spec(Units::Inches).unwrap();
        assert!((spec.height - 254.0).abs() < 1e-9 && (spec.extrusion - 25.4).abs() < 1e-9);
    }

    #[test]
    fn the_height_is_measured_on_the_font_ascent() {
        if !has_arial() {
            return;
        }
        let mut m = Model::default();
        let def = make_definition(&mut m, &state("H")).unwrap();
        let ents = &m.defs[def].entities;
        let (lo, hi) = ents.verts.values().fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(v.pos.y), h.max(v.pos.y)));
        // Arial: cap height 1466, ascent 1854 (units of a 2048 em).
        let want = 100.0 * 1466.0 / 1854.0;
        assert!((hi - lo - want).abs() < 0.5, "H is {} high, expected {want}", hi - lo);
        let top = ents.verts.values().map(|v| v.pos.z).fold(f64::MIN, f64::max);
        assert!((top - 10.0).abs() < 1e-9, "extruded by 10");
        assert!(m.defs[def].name.starts_with("Texte 3D"));
    }

    #[test]
    fn letters_with_holes_and_curves_build_solids() {
        if !has_arial() {
            return;
        }
        let mut m = Model::default();
        let def = make_definition(&mut m, &state("Oa\nB")).unwrap();
        let ents = &m.defs[def].entities;
        assert!(ents.faces.values().any(|f| !f.holes.is_empty()), "O, a and B have counters");
        assert!(ents.edges.values().any(|e| e.curve.is_some()), "bowls are curves");
        assert!(ents.edges.values().any(|e| e.curve.is_none()), "stems are straight");
        assert!(ents.edges.values().all(|e| e.faces.len() >= 2), "closed solids");
    }

    #[test]
    fn empty_text_or_bad_height_is_refused() {
        if !has_arial() {
            return;
        }
        let mut m = Model::default();
        assert!(make_definition(&mut m, &state("")).is_err());
        assert!(make_definition(&mut m, &state("  ")).is_err());
        let mut s = state("A");
        s.height = "abc".into();
        assert!(make_definition(&mut m, &s).is_err());
        s.height = "0.001mm".into();
        assert!(make_definition(&mut m, &s).is_err());
        assert!(m.defs.is_empty(), "nothing left behind");
    }
}
