//! Tools › Component Attributes and Component Options: the attribute dictionaries of the
//! selected group or component (stored on the instance, and on the definition for a
//! component, like SketchUp's `AttributeDictionary`). "Attributes" edits every dictionary;
//! "Options" lists the user-facing values of the `dynamic_attributes` dictionary (keys that
//! do not start with an underscore), the definition's values overridden by the instance's.

use eframe::egui;
use esq_core::model::{AttrKind, AttrValue, Attributes, DefId, EntityRef, InstanceId, Model, remove_attr, set_attr};

use crate::app::EsquisseApp;

/// The dictionary SketchUp's Dynamic Components keep their options in.
pub const OPTIONS_DICT: &str = "dynamic_attributes";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// This instance only.
    Instance,
    /// Every instance of the component.
    Definition,
}

/// The attributes of `scope`, `None` if the instance does not exist.
pub fn read_scope(model: &Model, ctx: Option<DefId>, inst: InstanceId, scope: Scope) -> Option<Attributes> {
    let i = model.entities(ctx).instances.get(inst)?;
    Some(match scope {
        Scope::Instance => i.attributes.clone(),
        Scope::Definition => model.defs.get(i.def)?.attributes.clone(),
    })
}

pub fn write_scope(model: &mut Model, ctx: Option<DefId>, inst: InstanceId, scope: Scope, attrs: Attributes) {
    let Some(def) = model.entities(ctx).instances.get(inst).map(|i| i.def) else { return };
    match scope {
        Scope::Instance => model.entities_mut(ctx).instances[inst].attributes = attrs,
        Scope::Definition => {
            if let Some(d) = model.defs.get_mut(def) {
                d.attributes = attrs;
            }
        }
    }
}

/// The options shown by Component Options: (key, value), the instance overriding the definition.
pub fn options_of(model: &Model, ctx: Option<DefId>, inst: InstanceId) -> Vec<(String, AttrValue)> {
    let Some(i) = model.entities(ctx).instances.get(inst) else { return Vec::new() };
    let mut map = std::collections::BTreeMap::new();
    if let Some(d) = model.defs.get(i.def).and_then(|d| d.attributes.get(OPTIONS_DICT)) {
        map.extend(d.clone());
    }
    if let Some(d) = i.attributes.get(OPTIONS_DICT) {
        map.extend(d.clone());
    }
    map.into_iter().filter(|(k, _)| !k.starts_with('_')).collect()
}

/// Sets an option on the instance.
pub fn set_option(model: &mut Model, ctx: Option<DefId>, inst: InstanceId, key: &str, value: AttrValue) {
    if let Some(i) = model.entities_mut(ctx).instances.get_mut(inst) {
        set_attr(&mut i.attributes, OPTIONS_DICT, key, value);
    }
}

/// The selected group or component, if the selection is just that.
pub fn selected_instance(app: &EsquisseApp) -> Option<InstanceId> {
    match app.doc.sel.as_slice() {
        [EntityRef::Instance(i)] => Some(*i),
        _ => None,
    }
}

pub struct AttrDialog {
    /// Component Options rather than Component Attributes.
    pub options: bool,
    target: Option<InstanceId>,
    scope: Scope,
    draft: Attributes,
    original: Attributes,
    new_dict: String,
    new_key: String,
    new_kind: AttrKind,
}

impl AttrDialog {
    pub fn new(options: bool) -> Self {
        AttrDialog {
            options,
            target: None,
            scope: Scope::Instance,
            draft: Attributes::new(),
            original: Attributes::new(),
            new_dict: "fiche".into(),
            new_key: String::new(),
            new_kind: AttrKind::Text,
        }
    }

    fn reload(&mut self, model: &Model, ctx: Option<DefId>, inst: InstanceId) {
        self.target = Some(inst);
        self.draft = read_scope(model, ctx, inst, self.scope).unwrap_or_default();
        self.original = self.draft.clone();
    }
}

fn convert(v: &AttrValue, kind: AttrKind) -> AttrValue {
    AttrValue::parse(kind, &v.display()).unwrap_or(match kind {
        AttrKind::Text => AttrValue::Text(String::new()),
        AttrKind::Number => AttrValue::Number(0.0),
        AttrKind::Bool => AttrValue::Bool(false),
    })
}

/// Edits a value in place, with the widget that fits its kind.
fn value_editor(ui: &mut egui::Ui, v: &mut AttrValue) {
    match v {
        AttrValue::Text(s) => {
            ui.add(egui::TextEdit::singleline(s).desired_width(160.0));
        }
        AttrValue::Number(n) => {
            ui.add(egui::DragValue::new(n).speed(0.1));
        }
        AttrValue::Bool(b) => {
            ui.checkbox(b, "");
        }
        // Values set from Ruby (integers, colours, arrays…) are shown, not edited.
        AttrValue::Json(_) => {
            ui.label(v.display());
        }
    }
}

pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(mut dlg) = app.ui_state.extra.attrs.take() else { return };
    let cx = app.doc.ctx_def();
    let sel = selected_instance(app);
    let mut open = true;
    if dlg.options {
        show_options(app, ctx, &mut dlg, sel, cx, &mut open);
    } else {
        show_attributes(app, ctx, &mut dlg, sel, cx, &mut open);
    }
    if open {
        app.ui_state.extra.attrs = Some(dlg);
    }
}

fn show_options(
    app: &mut EsquisseApp,
    ctx: &egui::Context,
    dlg: &mut AttrDialog,
    sel: Option<InstanceId>,
    cx: Option<DefId>,
    open: &mut bool,
) {
    let mut change = None;
    egui::Window::new("Options du composant").default_width(320.0).open(open).show(ctx, |ui| {
        let Some(inst) = sel else {
            ui.label("Sélectionnez un groupe ou un composant.");
            return;
        };
        let opts = options_of(&app.doc.model, cx, inst);
        if opts.is_empty() {
            ui.label(format!(
                "Aucune option : ajoutez des valeurs au dictionnaire « {OPTIONS_DICT} » dans les attributs du composant."
            ));
            return;
        }
        egui::Grid::new("component_options").num_columns(2).show(ui, |ui| {
            for (k, v) in opts {
                ui.label(&k);
                let mut edited = v.clone();
                value_editor(ui, &mut edited);
                if edited != v {
                    change = Some((inst, k, edited));
                }
                ui.end_row();
            }
        });
    });
    let _ = dlg;
    if let Some((inst, k, v)) = change {
        app.doc.commit("Options du composant", |m, d| set_option(m, d, inst, &k, v));
    }
}

fn show_attributes(
    app: &mut EsquisseApp,
    ctx: &egui::Context,
    dlg: &mut AttrDialog,
    sel: Option<InstanceId>,
    cx: Option<DefId>,
    open: &mut bool,
) {
    // Follow the selection; reload when it changes or when the model changed under us.
    if let Some(inst) = sel
        && (dlg.target != Some(inst)
            || dlg.draft == dlg.original
                && read_scope(&app.doc.model, cx, inst, dlg.scope).as_ref() != Some(&dlg.original))
    {
        dlg.reload(&app.doc.model, cx, inst);
    }
    let is_group = sel
        .and_then(|i| app.doc.ents().instances.get(i))
        .and_then(|i| app.doc.model.defs.get(i.def))
        .is_some_and(|d| d.is_group);
    let mut apply = false;
    let mut revert = false;
    let mut scope_changed = false;
    egui::Window::new("Attributs du composant").default_width(480.0).open(open).show(ctx, |ui| {
        let Some(inst) = sel else {
            ui.label("Sélectionnez un groupe ou un composant.");
            return;
        };
        let name = {
            let i = &app.doc.ents().instances[inst];
            if !i.name.is_empty() { i.name.clone() } else { app.doc.model.defs[i.def].name.clone() }
        };
        ui.strong(name);
        if !is_group {
            ui.horizontal(|ui| {
                scope_changed |= ui.selectable_value(&mut dlg.scope, Scope::Instance, "Cette instance").changed();
                scope_changed |= ui.selectable_value(&mut dlg.scope, Scope::Definition, "Tout le composant").changed();
            });
        } else if dlg.scope == Scope::Definition {
            dlg.scope = Scope::Instance;
            scope_changed = true;
        }
        ui.separator();
        let mut remove = None;
        let dicts: Vec<String> = dlg.draft.keys().cloned().collect();
        for dict in dicts {
            egui::CollapsingHeader::new(&dict).default_open(true).id_salt(("attr_dict", &dict)).show(ui, |ui| {
                egui::Grid::new(("attr_grid", &dict)).num_columns(4).show(ui, |ui| {
                    let keys: Vec<String> = dlg.draft[&dict].keys().cloned().collect();
                    for key in keys {
                        let Some(v) = dlg.draft.get_mut(&dict).and_then(|d| d.get_mut(&key)) else { continue };
                        ui.label(&key);
                        let mut kind = v.kind();
                        egui::ComboBox::from_id_salt(("attr_kind", &dict, &key))
                            .selected_text(kind.label())
                            .width(80.0)
                            .show_ui(ui, |ui| {
                                for k in AttrKind::ALL {
                                    ui.selectable_value(&mut kind, k, k.label());
                                }
                            });
                        if kind != v.kind() {
                            *v = convert(v, kind);
                        }
                        value_editor(ui, v);
                        if ui.button("🗑").on_hover_text("Supprimer cet attribut").clicked() {
                            remove = Some((dict.clone(), key.clone()));
                        }
                        ui.end_row();
                    }
                });
            });
        }
        if let Some((d, k)) = remove {
            remove_attr(&mut dlg.draft, &d, &k);
        }
        if dlg.draft.is_empty() {
            ui.label("Aucun attribut.");
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Dictionnaire");
            ui.add(egui::TextEdit::singleline(&mut dlg.new_dict).desired_width(100.0));
            ui.label("Nom");
            ui.add(egui::TextEdit::singleline(&mut dlg.new_key).desired_width(100.0));
            egui::ComboBox::from_id_salt("attr_new_kind").selected_text(dlg.new_kind.label()).width(80.0).show_ui(
                ui,
                |ui| {
                    for k in AttrKind::ALL {
                        ui.selectable_value(&mut dlg.new_kind, k, k.label());
                    }
                },
            );
            let ok = !dlg.new_dict.trim().is_empty()
                && !dlg.new_key.trim().is_empty()
                && !dlg.draft.get(dlg.new_dict.trim()).is_some_and(|d| d.contains_key(dlg.new_key.trim()));
            if ui.add_enabled(ok, egui::Button::new("Ajouter")).clicked() {
                let empty = convert(&AttrValue::Text(String::new()), dlg.new_kind);
                set_attr(&mut dlg.draft, dlg.new_dict.trim(), dlg.new_key.trim(), empty);
                dlg.new_key.clear();
            }
        });
        ui.separator();
        let dirty = dlg.draft != dlg.original;
        ui.horizontal(|ui| {
            apply = ui.add_enabled(dirty, egui::Button::new("Appliquer")).clicked();
            revert = ui.add_enabled(dirty, egui::Button::new("Rétablir")).clicked();
        });
    });
    let Some(inst) = sel else { return };
    if scope_changed || revert {
        dlg.reload(&app.doc.model, cx, inst);
    } else if apply {
        let (scope, attrs) = (dlg.scope, dlg.draft.clone());
        app.doc.commit("Attributs du composant", |m, d| write_scope(m, d, inst, scope, attrs));
        dlg.reload(&app.doc.model, cx, inst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::model::Model;

    /// A model with one component instance; returns it and the instance.
    fn model_with_instance() -> (Model, InstanceId) {
        use esq_core::glam::DVec3;
        let mut m = Model::new();
        m.root.add_polyline(
            &[DVec3::ZERO, DVec3::new(100.0, 0.0, 0.0), DVec3::new(100.0, 100.0, 0.0), DVec3::new(0.0, 100.0, 0.0)],
            true,
            false,
        );
        let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
        let i = m.make_group(None, &items, false, Some("Dalle".into())).unwrap();
        (m, i)
    }

    #[test]
    fn scopes_are_separate() {
        let (mut m, i) = model_with_instance();
        let mut a = Attributes::new();
        set_attr(&mut a, "fiche", "ref", AttrValue::Text("A1".into()));
        write_scope(&mut m, None, i, Scope::Instance, a.clone());
        assert_eq!(read_scope(&m, None, i, Scope::Instance), Some(a));
        assert_eq!(read_scope(&m, None, i, Scope::Definition), Some(Attributes::new()));
        let mut d = Attributes::new();
        set_attr(&mut d, OPTIONS_DICT, "couleur", AttrValue::Text("rouge".into()));
        write_scope(&mut m, None, i, Scope::Definition, d.clone());
        assert_eq!(read_scope(&m, None, i, Scope::Definition), Some(d));
        assert_eq!(read_scope(&m, None, InstanceId::default(), Scope::Instance), None);
    }

    #[test]
    fn options_merge_definition_and_instance_and_hide_internal_keys() {
        let (mut m, i) = model_with_instance();
        let def = m.root.instances[i].def;
        set_attr(&mut m.defs[def].attributes, OPTIONS_DICT, "couleur", AttrValue::Text("rouge".into()));
        set_attr(&mut m.defs[def].attributes, OPTIONS_DICT, "epaisseur", AttrValue::Number(20.0));
        set_attr(&mut m.defs[def].attributes, OPTIONS_DICT, "_name", AttrValue::Text("interne".into()));
        set_attr(&mut m.defs[def].attributes, "autre", "x", AttrValue::Bool(true));
        assert_eq!(options_of(&m, None, i).len(), 2);
        set_option(&mut m, None, i, "couleur", AttrValue::Text("bleu".into()));
        let opts = options_of(&m, None, i);
        assert_eq!(opts[0], ("couleur".to_string(), AttrValue::Text("bleu".into())));
        assert_eq!(opts[1], ("epaisseur".to_string(), AttrValue::Number(20.0)));
        // The definition keeps its own value.
        assert_eq!(m.defs[def].attributes[OPTIONS_DICT]["couleur"], AttrValue::Text("rouge".into()));
    }

    #[test]
    fn changing_the_kind_converts_the_value() {
        assert_eq!(convert(&AttrValue::Text("12,5".into()), AttrKind::Number), AttrValue::Number(12.5));
        assert_eq!(convert(&AttrValue::Text("abc".into()), AttrKind::Number), AttrValue::Number(0.0));
        assert_eq!(convert(&AttrValue::Number(1.0), AttrKind::Bool), AttrValue::Bool(true));
        assert_eq!(convert(&AttrValue::Bool(false), AttrKind::Text), AttrValue::Text("faux".into()));
    }
}
