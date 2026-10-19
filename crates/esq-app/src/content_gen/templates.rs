//! The templates: SketchUp 2026's 21, with the same names, units and settings (rendering
//! options, units options, shadows). Those are facts: they were read once from SketchUp's
//! templates through its SDK into `tools/content/modeles-types.json`
//! (`ESQ_WRITE_TEMPLATE_FACTS=1`), and the `.esq` files are made from that table alone —
//! nothing else of SketchUp's files (no geometry, component, material or picture).

use esq_core::model::{Model, Units};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const COUNT: usize = 21;

/// What a template of SketchUp sets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    /// SketchUp's file (`Temp03c - Arch.skp`): its number orders the list.
    pub file: String,
    pub name: String,
    pub description: String,
    pub units: Units,
    pub units_options: Map<String, Value>,
    /// Rendering options.
    pub ro: Map<String, Value>,
    pub shadow: Value,
}

pub fn facts_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/content/modeles-types.json")
}

/// SketchUp's templates, when it is installed (the oracle).
pub fn sketchup_templates() -> Option<Vec<std::path::PathBuf>> {
    let dir = std::path::Path::new(r"C:\Program Files\SketchUp\SketchUp 2026\SketchUp\resources\en-US\Templates");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "skp"))
        .collect();
    files.sort();
    (!files.is_empty()).then_some(files)
}

/// The facts of one of SketchUp's templates, read through its SDK.
pub fn read_fact(path: &std::path::Path) -> Fact {
    let m = esq_skp::import_skp(path).expect("modèle type lisible");
    Fact {
        file: path.file_name().unwrap().to_string_lossy().into_owned(),
        name: m.meta.name.clone(),
        description: m.meta.description.clone(),
        units: m.units,
        units_options: m
            .meta
            .options
            .get("UnitsOptions")
            .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default(),
        ro: m.view_ext.get("ro").and_then(Value::as_object).cloned().unwrap_or_default(),
        shadow: serde_json::to_value(&m.shadow).unwrap(),
    }
}

pub fn read_facts() -> Vec<Fact> {
    let bytes = std::fs::read(facts_path()).expect("tools/content/modeles-types.json");
    serde_json::from_slice(&bytes).expect("table des modèles types")
}

/// The template a fact makes: its file name and its model.
pub fn build(f: &Fact) -> (String, Model) {
    let mut m = Model { units: f.units, ..Model::new() };
    m.meta.name = crate::i18n::fr_or_same(&f.name);
    m.meta.description = crate::i18n::fr_or_same(&f.description);
    let opts = m.meta.options.entry("UnitsOptions".into()).or_default();
    for (k, v) in &f.units_options {
        opts.insert(k.clone(), v.clone());
    }
    m.view_ext.insert("ro".into(), Value::Object(f.ro.clone()));
    m.shadow = serde_json::from_value(f.shadow.clone()).unwrap_or_default();
    m.meta.mipmapping = true;
    // "Temp03c - Arch.skp" → "03c".
    let code: String = f.file.trim_start_matches("Temp").chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    let stem = super::materials::file_stem(&format!("{code} {} - {}", m.meta.name, f.units.name()));
    (stem, m)
}
