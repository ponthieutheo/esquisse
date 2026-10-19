//! Model Info › Classifications: the classification systems (`.skc` schemas such as IFC)
//! loaded in the model. They are kept where the Ruby API (`model.classifications`,
//! `ComponentDefinition#add_classification`) keeps them: the "schemas" dictionary of the
//! model's "classifications" attribute owner, one entry per schema
//! (`[namespace, file, type…]`).

use std::path::{Path, PathBuf};

use eframe::egui;
use esq_core::model::{AttrValue, Model};
use serde_json::{Value, json};

use super::{group, name_widget};
use crate::app::EsquisseApp;

const OWNER: &str = "classifications";
const DICT: &str = "schemas";

/// A loaded classification system.
#[derive(Clone, Debug, PartialEq)]
pub struct Schema {
    pub name: String,
    pub namespace: String,
    /// The `.skc` file it was loaded from.
    pub path: String,
    pub types: Vec<String>,
}

pub use esq_core::classify::zip_entries;

/// Reads a `.skc` file: its title, namespace and the types it offers (the filter's list,
/// with the types they derive from; `esq_core::classify::parse_skc`).
#[cfg(test)]
pub fn read_schema(path: &Path) -> Option<Schema> {
    let s = esq_core::classify::parse_skc(&std::fs::read(path).ok()?, &path.display().to_string())?;
    Some(Schema { name: s.name, namespace: s.namespace, path: s.path, types: s.types })
}
fn enc_s(s: &str) -> Value {
    json!({"t": "s", "v": s})
}

/// The schemas loaded in a model, by name.
pub fn schemas(m: &Model) -> Vec<Schema> {
    let Some(d) = m.attr_store.get(OWNER).and_then(|a| a.get(DICT)) else { return Vec::new() };
    d.iter()
        .filter_map(|(name, v)| {
            let AttrValue::Json(j) = v else { return None };
            let items: Vec<String> =
                j["v"].as_array()?.iter().map(|x| x["v"].as_str().unwrap_or_default().to_string()).collect();
            let (ns, path, types) = (items.first()?.clone(), items.get(1)?.clone(), items[2..].to_vec());
            Some(Schema { name: name.clone(), namespace: ns, path, types })
        })
        .collect()
}

/// Loads a schema into the model (replacing one of the same name).
pub fn store(m: &mut Model, s: &Schema) {
    let mut v = vec![enc_s(&s.namespace), enc_s(&s.path)];
    v.extend(s.types.iter().map(|t| enc_s(t)));
    let at = m.attr_store.entry(OWNER.to_string()).or_default();
    esq_core::model::set_attr(at, DICT, &s.name, AttrValue::Json(json!({"t": "a", "v": v})));
}

pub fn unload(m: &mut Model, name: &str) {
    if let Some(at) = m.attr_store.get_mut(OWNER) {
        esq_core::model::remove_attr(at, DICT, name);
        if at.is_empty() {
            m.attr_store.remove(OWNER);
        }
    }
}

/// Where Import… starts: Preferences › Files › Classifications, else the schemas that come
/// with Esquisse (IFC 2x3 and IFC 4 of buildingSMART).
pub fn import_dir() -> Option<PathBuf> {
    crate::settings::with(|s| s.location("classifications")).or_else(|| esq_core::content::folder("Classifications"))
}

/// Import…: reads a `.skc` and loads it. The message says what happened.
pub fn import(app: &mut EsquisseApp, path: &Path) -> Result<String, String> {
    let bad = || format!("« {} » n'est pas un système de classement.", path.display());
    let data = std::fs::read(path).map_err(|_| bad())?;
    let s = esq_core::classify::parse_skc(&data, &path.display().to_string()).ok_or_else(bad)?;
    let s = Schema { name: s.name, namespace: s.namespace, path: s.path, types: s.types };
    let name = s.name.clone();
    // The file goes with the model: a .skp carries it (`esq_core::classify::file_key`).
    app.doc.commit("Importer un classement", |m, _| {
        store(m, &s);
        esq_core::classify::keep_file(m, &s.name, data);
    });
    Ok(name)
}

/// The chosen schema, when its `.skc` file can be exported (copied): the path comes from the
/// model, so it is looked at only when it is local (`esq_core::untrusted`). It was checked
/// at every frame, and a model naming `\\serveur\x.skc` made Windows send the user's NTLM
/// hash as soon as the schema was selected.
fn exportable<'a>(list: &'a [Schema], chosen: Option<&str>) -> Option<&'a Schema> {
    let s = list.iter().find(|s| Some(s.name.as_str()) == chosen)?;
    crate::safe_path::exists(&s.path).then_some(s)
}

pub fn classifications_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    group(ui, "Classements");
    ui.label("Choisissez un système de classement à charger dans ce modèle.");
    let list = schemas(&app.doc.model);
    let st = &mut app.ui_state.extra.dialogs.info;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_height(120.0);
        ui.set_min_width(380.0);
        if list.is_empty() {
            ui.label(egui::RichText::new("Aucun système de classement chargé.").weak());
        }
        for s in &list {
            let r = ui.selectable_label(st.schema.as_deref() == Some(&s.name), &s.name);
            name_widget(ui, &r, &format!("Classement {}", s.name), Some(&format!("{} types", s.types.len())));
            if r.clicked() {
                st.schema = Some(s.name.clone());
            }
        }
    });
    let chosen = st.schema.clone().filter(|n| list.iter().any(|s| &s.name == n));
    ui.horizontal(|ui| {
        if ui.add_enabled(chosen.is_some(), egui::Button::new("Supprimer")).clicked()
            && let Some(n) = &chosen
        {
            let n = n.clone();
            app.doc.commit("Supprimer un classement", |m, _| unload(m, &n));
        }
        if ui.button("Importer...").clicked() {
            let mut d = rfd::FileDialog::new().add_filter("Classements SketchUp", &["skc"]);
            if let Some(dir) = import_dir() {
                d = d.set_directory(dir);
            }
            if let Some(p) = d.pick_file() {
                let msg = match import(app, &p) {
                    Ok(n) => format!("Classement « {n} » chargé"),
                    Err(e) => e,
                };
                app.message = Some((msg, 0.0));
            }
        }
        let exportable = exportable(&list, chosen.as_deref());
        if ui.add_enabled(exportable.is_some(), egui::Button::new("Exporter...")).clicked()
            && let Some(s) = exportable
            && let Some(dest) = crate::settings::dialog("classifications")
                .add_filter("Classements SketchUp", &["skc"])
                .set_file_name(format!("{}.skc", s.name))
                .save_file()
        {
            let msg = match crate::safe_path::read(&s.path)
                .ok_or_else(|| std::io::Error::other("fichier illisible"))
                .and_then(|b| std::fs::write(&dest, b))
            {
                Ok(_) => format!("Classement exporté : {}", dest.display()),
                Err(e) => format!("Échec de l'export : {e}"),
            };
            app.message = Some((msg, 0.0));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model whose classification names its `.skc` by a network path: selecting it in
    /// Model Info › Classifications looked at `\\serveur\…` at every frame.
    #[test]
    fn a_network_schema_path_is_never_looked_at() {
        let unc = format!(r"\\esq-test-skc-{}\partage\ifc.skc", std::process::id());
        let mut m = Model::new();
        store(&mut m, &Schema { name: "IFC".into(), namespace: "urn:x".into(), path: unc.clone(), types: vec![] });
        let list = schemas(&m);
        assert_eq!(list[0].path, unc);
        assert!(exportable(&list, Some("IFC")).is_none());
        assert!(esq_core::untrusted::refused_paths().contains(&unc));
        assert!(!esq_core::untrusted::opened_paths().contains(&unc));
    }
}
