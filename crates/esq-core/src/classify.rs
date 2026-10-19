//! Classifications (IFC…): the schemas loaded in the model and the types given to component
//! and group definitions, stored like the Ruby API stores them (`Sketchup::Classifications`,
//! `ComponentDefinition#add_classification`): the schemas in the model's side table
//! (`classifications` › `schemas`, one tagged array `[namespace, path, type…]` per schema),
//! the type of a definition in its `AppliedSchemaTypes` dictionary (schema → type). The
//! `.skc` file of a schema is kept with the model (`Model::files`, [`file_key`]): a .skp
//! carries it, and SketchUp reads the schema from it.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::model::{AttrValue, DefId, Model, remove_attr, set_attr};

pub const APPLIED: &str = "AppliedSchemaTypes";
const OWNER: &str = "classifications";
const DICT: &str = "schemas";

/// A classification system loaded in the model.
#[derive(Clone, Debug, PartialEq)]
pub struct Schema {
    pub name: String,
    pub namespace: String,
    /// The file it was loaded from.
    pub path: String,
    /// The types it offers (`IfcWall`…), in file order.
    pub types: Vec<String>,
}

fn text(v: &Value) -> Option<String> {
    v.get("v").and_then(Value::as_str).map(str::to_string)
}

/// The schemas loaded in the model, by name.
pub fn schemas(m: &Model) -> Vec<Schema> {
    let Some(d) = m.attr_store.get(OWNER).and_then(|a| a.get(DICT)) else { return Vec::new() };
    d.iter()
        .filter_map(|(name, v)| {
            let AttrValue::Json(j) = v else { return None };
            let items = j.get("v")?.as_array()?;
            let mut it = items.iter().map(text);
            let namespace = it.next()??;
            let path = it.next()??;
            let types = it.flatten().collect();
            Some(Schema { name: name.clone(), namespace, path, types })
        })
        .collect()
}

/// Where the `.skc` file of the schema `name` is kept in `Model::files`.
pub fn file_key(name: &str) -> String {
    format!("skc:{name}")
}

/// Keeps the `.skc` file of a loaded schema with the model.
pub fn keep_file(m: &mut Model, name: &str, data: Vec<u8>) {
    m.files.insert(file_key(name), crate::photo::Blob(Arc::new(data)));
}

/// The `.skc` file of a loaded schema, when the model has it.
pub fn file_of<'m>(m: &'m Model, name: &str) -> Option<&'m [u8]> {
    m.files.get(&file_key(name)).map(|b| b.0.as_slice())
}

/// The files of a zip archive (a `.skc` is one), stored or deflated, by name: the shared
/// reader of `crate::zip`, within one decompression budget.
pub fn zip_entries(data: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    let a = crate::zip::Archive::open(data).ok()?;
    let budget = crate::untrusted::Budget::for_input(data.len());
    let mut out = Vec::new();
    for e in a.entries.iter().filter(|e| matches!(e.method, 0 | 8)) {
        out.push((e.name.clone(), a.read(e, &budget).ok()?));
    }
    Some(out)
}

/// The types of `types` with every type they derive from (`substitutionGroup` parents in
/// `parents`), each once: a cycle of parents ends.
pub fn with_parents(mut types: Vec<String>, parents: &HashMap<String, String>) -> Vec<String> {
    let mut i = 0;
    while i < types.len() {
        if let Some(p) = parents.get(&types[i])
            && !types.contains(p)
        {
            types.push(p.clone());
        }
        i += 1;
    }
    types
}

fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let a = s.find(open)? + open.len();
    let b = s[a..].find(close)? + a;
    Some(&s[a..b])
}

/// The types a `.xsd.filter` offers: one name per line, before `// Blacklist`; the
/// `{ … }` block that may follow a type lists the attributes shown for it, not types.
pub fn filter_types(f: &str) -> Vec<String> {
    let mut types: Vec<String> = Vec::new();
    let mut depth = 0usize;
    for l in f.lines() {
        if l.starts_with("// Blacklist") {
            break;
        }
        let t = l.trim();
        if t.starts_with('{') {
            depth += 1;
        } else if t.starts_with('}') {
            depth = depth.saturating_sub(1);
        } else if depth == 0
            && !t.is_empty()
            && t.chars().all(|c| c.is_alphanumeric() || c == '_')
            && !types.iter().any(|x| x == t)
        {
            types.push(t.to_string());
        }
    }
    types
}

/// Reads a `.skc` file's bytes (`Classifications#load_schema`): its title, namespace and the
/// types it offers (the filter's list, with the types they derive from). `path` is where it
/// was loaded from (kept, never opened again).
pub fn parse_skc(data: &[u8], path: &str) -> Option<Schema> {
    let files = zip_entries(data)?;
    let text = |pred: &dyn Fn(&str) -> bool| {
        files.iter().find(|(n, _)| pred(n)).map(|(_, b)| String::from_utf8_lossy(b).into_owned())
    };
    let props = text(&|n| n == "documentProperties.xml")?;
    let title = between(&props, "<dp:title>", "</dp:title>")?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let xsd = text(&|n| n.starts_with("Schemas/") && n.ends_with(".xsd") && !n.ends_with("ex.xsd"));
    let namespace = xsd.as_deref().and_then(|x| between(x, "targetNamespace=\"", "\"")).unwrap_or("").to_string();
    let mut types: Vec<String> = Vec::new();
    if let Some(f) = text(&|n| n.ends_with(".xsd.filter") && !n.ends_with("ex.xsd.filter")) {
        types = filter_types(&f);
    }
    // Parents of the listed types (substitution groups of the schema).
    if let Some(x) = &xsd {
        let mut parents = HashMap::new();
        for part in x.split("<xs:element substitutionGroup=\"").skip(1) {
            let group = part.split('"').next().unwrap_or("");
            let parent = group.rsplit(':').next().unwrap_or(group).to_string();
            if let Some(n) = between(part, "name=\"", "\"") {
                parents.insert(n.to_string(), parent);
            }
        }
        types = with_parents(types, &parents);
    }
    Some(Schema { name: title, namespace, path: path.to_string(), types })
}

/// Loads (or replaces) a schema in the model.
pub fn store_schema(m: &mut Model, s: &Schema) {
    let mut items = vec![json!({"t": "s", "v": s.namespace}), json!({"t": "s", "v": s.path})];
    items.extend(s.types.iter().map(|t| json!({"t": "s", "v": t})));
    let attrs = m.attr_store.entry(OWNER.to_string()).or_default();
    set_attr(attrs, DICT, &s.name, AttrValue::Json(json!({"t": "a", "v": items})));
}

/// The types given to a definition: (schema, type).
pub fn applied(m: &Model, def: DefId) -> Vec<(String, String)> {
    let Some(d) = m.defs.get(def).and_then(|d| d.attributes.get(APPLIED)) else { return Vec::new() };
    d.iter()
        .filter_map(|(schema, v)| match v {
            AttrValue::Text(t) => Some((schema.clone(), t.clone())),
            AttrValue::Json(j) => text(j).map(|t| (schema.clone(), t)),
            _ => None,
        })
        .collect()
}

/// Gives `def` the type `ty` of `schema` (one type per schema). False if the schema is not
/// loaded or does not have that type.
pub fn apply(m: &mut Model, def: DefId, schema: &str, ty: &str) -> bool {
    if !schemas(m).iter().any(|s| s.name == schema && s.types.iter().any(|t| t == ty)) {
        return false;
    }
    let Some(d) = m.defs.get_mut(def) else { return false };
    // A new type drops the values filled in for the old one.
    if d.attributes.get(APPLIED).and_then(|x| x.get(schema)) != Some(&AttrValue::Text(ty.to_string())) {
        d.attributes.shift_remove(schema);
    }
    set_attr(&mut d.attributes, APPLIED, schema, AttrValue::Text(ty.to_string()));
    true
}

/// Removes every type of `def` (and the values that went with them). False if it had none.
pub fn remove_all(m: &mut Model, def: DefId) -> bool {
    let types = applied(m, def);
    let Some(d) = m.defs.get_mut(def) else { return false };
    for (schema, _) in &types {
        remove_attr(&mut d.attributes, APPLIED, schema);
        d.attributes.shift_remove(schema.as_str());
    }
    !types.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ifc() -> Schema {
        Schema {
            name: "IFC Test".into(),
            namespace: "http://example.org/ifc".into(),
            path: "C:/x.skc".into(),
            types: vec!["IfcWall".into(), "IfcDoor".into()],
        }
    }

    #[test]
    fn schemas_round_trip_and_types_apply_once_per_schema() {
        let mut m = Model::new();
        store_schema(&mut m, &ifc());
        assert_eq!(schemas(&m), vec![ifc()]);
        let d = m.defs.insert(crate::model::Definition {
            name: "Mur".into(),
            is_group: false,
            description: String::new(),
            entities: Default::default(),
            attributes: Default::default(),
            ext: Default::default(),
        });
        assert!(!apply(&mut m, d, "IFC Test", "IfcBeam"), "not a type of the schema");
        assert!(!apply(&mut m, d, "Autre", "IfcWall"), "schema not loaded");
        assert!(apply(&mut m, d, "IFC Test", "IfcWall"));
        assert!(apply(&mut m, d, "IFC Test", "IfcDoor"));
        assert_eq!(applied(&m, d), vec![("IFC Test".to_string(), "IfcDoor".to_string())]);
        assert!(remove_all(&mut m, d));
        assert!(applied(&m, d).is_empty() && m.defs[d].attributes.is_empty());
        assert!(!remove_all(&mut m, d));
    }
}
