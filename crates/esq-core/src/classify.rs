//! Classifications (IFC…): the schemas loaded in the model and the types given to component
//! and group definitions, stored like the Ruby API stores them (`Sketchup::Classifications`,
//! `ComponentDefinition#add_classification`): the schemas in the model's side table
//! (`classifications` › `schemas`, one tagged array `[namespace, path, type…]` per schema),
//! the type of a definition in its `AppliedSchemaTypes` dictionary (schema → type), the
//! attributes of the type in its dictionary named after the schema, nested
//! ([`crate::classify_tree`]; the side table keeps the nested ones, `ad:<owner>|<dictionary>`).
//! The `.skc` file of a schema is kept with the model (`Model::files`, [`file_key`]): a .skp
//! carries it, and SketchUp reads the schema from it.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::classify_tree::{Classified, Dict, Kind};
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

/// The owner of a definition's dictionaries in the side table of nested ones
/// (`ad:<owner>|<dictionary>`, the key the Ruby bridge and the `.skp` reader use).
fn def_owner(def: DefId) -> String {
    format!("d:0:{}", slotmap::Key::data(&def).as_ffi())
}

/// Writes `dicts` inside the dictionary `name` of `owner`, and what they hold.
fn write_nested(m: &mut Model, owner: &str, name: &str, dicts: &[Dict]) {
    if dicts.is_empty() {
        return;
    }
    let key = format!("ad:{owner}|{name}");
    let at = m.attr_store.entry(key.clone()).or_default();
    for d in dicts {
        at.insert(d.name.clone(), d.attrs.clone());
    }
    for d in dicts {
        write_nested(m, &key, &d.name, &d.children);
    }
}

/// Drops what the dictionary `name` of `owner` holds, at every level (iteratively: the side
/// table comes from files).
fn purge_nested(m: &mut Model, owner: &str, name: &str) {
    let mut stack = vec![format!("ad:{owner}|{name}")];
    while let Some(k) = stack.pop() {
        if let Some(at) = m.attr_store.remove(&k) {
            stack.extend(at.keys().map(|n| format!("ad:{k}|{n}")));
        }
    }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The type named `ty` (`IfcDoor`, `ifc:IfcDoor`) of a loaded schema, with the tree it gives:
/// read from the schema's `.skc` when the model keeps it, else known by name only (a schema
/// stored with its list of types and no file: no tree).
fn find_type(m: &Model, s: &Schema, ty: &str) -> Option<Classified> {
    if let Some(files) = file_of(m, &s.name).and_then(zip_entries) {
        return crate::classify_tree::build(&files, ty, now());
    }
    let local = ty.rsplit(':').next().unwrap_or(ty);
    s.types.iter().any(|t| t == local).then(|| Classified {
        namespace: s.namespace.clone(),
        name: local.to_string(),
        children: Vec::new(),
    })
}

/// Gives `def` the type `ty` of `schema`, as `ComponentDefinition#add_classification` and the
/// Classifier do: the type in `AppliedSchemaTypes` (one per schema), the
/// dictionary `schema` holding `SchemaType` (`namespace:type`) and one dictionary per attribute
/// of the type ([`crate::classify_tree`]); the IFC schemas also get `Classifications` and
/// `PropertySets`. A type given again starts afresh. False if the schema is not loaded or
/// does not have that type.
pub fn apply(m: &mut Model, def: DefId, schema: &str, ty: &str) -> bool {
    let Some(s) = schemas(m).into_iter().find(|s| s.name == schema) else { return false };
    if !m.defs.contains_key(def) {
        return false;
    }
    let Some(c) = find_type(m, &s, ty) else { return false };
    let owner = def_owner(def);
    purge_nested(m, &owner, schema);
    let d = &mut m.defs[def];
    d.attributes.shift_remove(schema);
    set_attr(&mut d.attributes, APPLIED, schema, AttrValue::Text(c.name.clone()));
    set_attr(&mut d.attributes, schema, "SchemaType", AttrValue::Text(format!("{}:{}", c.namespace, c.name)));
    let mut children = c.children;
    if schema == "IFC 2x3" || schema == "IFC 4" {
        for name in ["Classifications", "PropertySets"] {
            let attrs = [("is_hidden".to_string(), AttrValue::Bool(false))].into_iter().collect();
            children.push(Dict { name: name.into(), attrs, children: Vec::new() });
        }
    }
    write_nested(m, &owner, schema, &children);
    true
}

fn text_value(v: &AttrValue) -> Option<String> {
    match v {
        AttrValue::Text(t) => Some(t.clone()),
        AttrValue::Json(j) => j.get("v").and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

/// Takes the type of `schema` from `def` (`ComponentDefinition#remove_classification`):
/// only when the schema is loaded, `ty` (when given, not
/// empty) is one of its types and is the type `def` has. The dictionary `schema` goes with
/// all it holds, `AppliedSchemaTypes` too when it is left empty.
pub fn remove(m: &mut Model, def: DefId, schema: &str, ty: Option<&str>) -> bool {
    let Some(s) = schemas(m).into_iter().find(|s| s.name == schema) else { return false };
    let wanted = match ty.filter(|t| !t.is_empty()) {
        Some(t) => match find_type(m, &s, t) {
            Some(c) => Some(c.name),
            None => return false,
        },
        None => None,
    };
    let Some(d) = m.defs.get_mut(def) else { return false };
    if !d.attributes.contains_key(schema) {
        return false;
    }
    let Some(has) = d.attributes.get(APPLIED).and_then(|a| a.get(schema)).and_then(text_value) else { return false };
    if wanted.is_some_and(|w| has.rsplit(':').next() != Some(w.as_str())) {
        return false;
    }
    d.attributes.shift_remove(schema);
    remove_attr(&mut d.attributes, APPLIED, schema);
    purge_nested(m, &def_owner(def), schema);
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
    for (schema, _) in &types {
        purge_nested(m, &def_owner(def), schema);
    }
    !types.is_empty()
}

/// Where the dictionary a classification path names is: `[schema, type,
/// attribute…]`, at least three names, the type being the one `def` has for that schema
/// (its `SchemaType` without the namespace). The dictionary is `m.attr_store[owner][name]`.
fn path_dict(m: &Model, def: DefId, path: &[String]) -> Option<(String, String)> {
    let [schema, ty, rest @ ..] = path else { return None };
    let first = rest.first()?;
    let d = m.defs.get(def)?;
    d.attributes.get(APPLIED)?.get(schema)?;
    let st = d.attributes.get(schema.as_str())?.get("SchemaType").and_then(|v| match v {
        AttrValue::Text(t) => Some(t),
        _ => None,
    })?;
    if st.rsplit(':').next() != Some(ty.as_str()) {
        return None;
    }
    let mut owner = format!("ad:{}|{schema}", def_owner(def));
    let mut name = first.clone();
    m.attr_store.get(&owner)?.get(&name)?;
    for next in &rest[1..] {
        owner = format!("ad:{owner}|{name}");
        name = next.clone();
        m.attr_store.get(&owner)?.get(&name)?;
    }
    Some((owner, name))
}

/// `ComponentDefinition#get_classification_value`: the `value` of the dictionary a path
/// names, None when there is none.
pub fn get_value(m: &Model, def: DefId, path: &[String]) -> Option<AttrValue> {
    let (owner, name) = path_dict(m, def, path)?;
    m.attr_store.get(&owner)?.get(&name)?.get("value").cloned()
}

/// Why `set_classification_value` refuses a value.
#[derive(Clone, Debug, PartialEq)]
pub enum SetError {
    /// The attribute is a `choice` (`ClassificationNotImplemented`).
    Choice,
    /// The value is not of the attribute's kind (`InvalidClassificationValueType`).
    WrongType,
    /// The dictionary's `attribute_type` is missing or unknown (the message SketchUp gives).
    Corrupt(&'static str),
}

/// The kind of value a Ruby value makes (numbered like `CTypedValue`): text
/// 10, float 6, integer 4, boolean 7, time 9, length 12, array 11, colour 8, nil 0.
fn value_code(v: &AttrValue) -> u8 {
    match v {
        AttrValue::Text(_) => 10,
        AttrValue::Number(_) => 6,
        AttrValue::Bool(_) => 7,
        AttrValue::Json(j) => match j.get("t").and_then(Value::as_str) {
            Some("s") => 10,
            Some("f") | Some("nf") => 6,
            Some("i") => 4,
            Some("b") => 7,
            Some("time") => 9,
            Some("l") => 12,
            Some("a") => 11,
            Some("c") => 8,
            Some("p") | Some("vec") => 17,
            _ => 0,
        },
    }
}

fn kind_code(k: Kind) -> u8 {
    match k {
        Kind::Byte => 2,
        Kind::Short => 3,
        Kind::Long => 4,
        Kind::Float => 5,
        Kind::Double => 6,
        Kind::Boolean => 7,
        Kind::DateTime => 9,
        Kind::String => 10,
        Kind::Unknown => 0,
    }
}

/// `ComponentDefinition#set_classification_value`: sets the `value` of the dictionary a path
/// names, when it has one and `v` is of its `attribute_type` (`unknownType`: of the kind of
/// the value it holds). Ok(false) when the path names no dictionary with a value.
pub fn set_value(m: &mut Model, def: DefId, path: &[String], v: AttrValue) -> Result<bool, SetError> {
    let Some((owner, name)) = path_dict(m, def, path) else { return Ok(false) };
    let Some(d) = m.attr_store.get_mut(&owner).and_then(|a| a.get_mut(&name)) else { return Ok(false) };
    let Some(old) = d.get("value") else { return Ok(false) };
    let ty = d.get("attribute_type");
    if ty == Some(&AttrValue::Text("choice".into())) {
        return Err(SetError::Choice);
    }
    let ty = match ty {
        None => return Err(SetError::Corrupt("Expected 'attribute_type' missing")),
        Some(AttrValue::Text(t)) => t,
        Some(_) => return Err(SetError::Corrupt("Expected 'attribute_type' to be a string")),
    };
    let Some(kind) = Kind::from_name(ty) else { return Err(SetError::Corrupt("Unexpected attribute type")) };
    let want = if kind == Kind::Unknown { value_code(old) } else { kind_code(kind) };
    if value_code(&v) != want {
        return Err(SetError::WrongType);
    }
    d.insert("value".into(), v);
    Ok(true)
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

    /// A classification whose `.skc` the model keeps: the type brings its tree, values are
    /// read and set with SketchUp's checks, and the type goes with everything it held.
    #[test]
    fn a_type_brings_its_tree_and_takes_it_away() {
        use crate::classify_tree::tests::{EX, T};
        let skc = crate::zip::tests::archive(&[
            ("documentProperties.xml", b"<documentProperties><dp:title>IFC 2x3</dp:title></documentProperties>", false),
            (
                "document.xml",
                b"<classificationDocument><cls:Classification xsdFile=\"Schemas/T.xsd\"/></classificationDocument>",
                false,
            ),
            ("Schemas/ex.xsd", EX.as_bytes(), true),
            ("Schemas/T.xsd", T.as_bytes(), true),
        ]);
        let mut m = Model::new();
        let s = parse_skc(&skc, "C:/t.skc").expect("classement lisible");
        store_schema(&mut m, &s);
        keep_file(&mut m, &s.name, skc);
        let d = m.defs.insert(crate::model::Definition {
            name: "Porte".into(),
            is_group: false,
            description: String::new(),
            entities: Default::default(),
            attributes: Default::default(),
            ext: Default::default(),
        });
        let owner = def_owner(d);
        assert!(!apply(&mut m, d, "IFC 2x3", "Window"), "pas un type du schéma");
        assert!(apply(&mut m, d, "IFC 2x3", "t:Door"));
        assert_eq!(applied(&m, d), vec![("IFC 2x3".to_string(), "Door".to_string())]);
        assert_eq!(m.defs[d].attributes["IFC 2x3"]["SchemaType"], AttrValue::Text("urn:t:Door".into()));
        let top = &m.attr_store[&format!("ad:{owner}|IFC 2x3")];
        assert!(
            ["Height", "Name", "instanceAttributes", "Classifications", "PropertySets"]
                .iter()
                .all(|k| top.contains_key(*k))
        );
        let p = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let label = p(&["IFC 2x3", "Door", "Name", "Label"]);
        assert_eq!(get_value(&m, d, &label), Some(AttrValue::Text(String::new())));
        assert_eq!(set_value(&mut m, d, &label, AttrValue::Text("Porte 1".into())), Ok(true));
        assert_eq!(get_value(&m, d, &label), Some(AttrValue::Text("Porte 1".into())));
        let int = AttrValue::Json(json!({"t": "i", "v": 3}));
        assert_eq!(set_value(&mut m, d, &label, int), Err(SetError::WrongType));
        let pos = p(&["IFC 2x3", "Door", "Name", "instanceAttributes", "pos"]);
        assert_eq!(set_value(&mut m, d, &pos, AttrValue::Text("integer".into())), Err(SetError::Choice));
        let kind = p(&["IFC 2x3", "Door", "Kind", "DoorKind"]);
        assert_eq!(
            set_value(&mut m, d, &kind, AttrValue::Text("double".into())),
            Err(SetError::Corrupt("Unexpected attribute type"))
        );
        let at = format!("ad:ad:{owner}|IFC 2x3|Name");
        m.attr_store
            .get_mut(&at)
            .unwrap()
            .get_mut("Label")
            .unwrap()
            .insert("attribute_type".into(), AttrValue::Number(1.0));
        assert_eq!(
            set_value(&mut m, d, &label, AttrValue::Text("x".into())),
            Err(SetError::Corrupt("Expected 'attribute_type' to be a string"))
        );
        // Another type, too short a path, a dictionary without value.
        assert_eq!(get_value(&m, d, &p(&["IFC 2x3", "Window", "Name", "Label"])), None);
        assert_eq!(get_value(&m, d, &p(&["IFC 2x3", "Door"])), None);
        assert_eq!(set_value(&mut m, d, &p(&["IFC 2x3", "Door", "Name"]), AttrValue::Text("x".into())), Ok(false));
        // Given again, the type starts afresh.
        assert!(apply(&mut m, d, "IFC 2x3", "Door"));
        assert_eq!(get_value(&m, d, &label), Some(AttrValue::Text(String::new())));
        // Removal: not by another type of the schema nor another schema; by its own type.
        assert!(!remove(&mut m, d, "IFC 2x3", Some("Root")));
        assert!(!remove(&mut m, d, "Autre", None));
        assert!(remove(&mut m, d, "IFC 2x3", Some("Door")));
        assert!(m.defs[d].attributes.is_empty());
        assert!(!m.attr_store.keys().any(|k| k.contains(&owner)), "rien ne reste du type");
        assert!(!remove(&mut m, d, "IFC 2x3", None));
    }
}
