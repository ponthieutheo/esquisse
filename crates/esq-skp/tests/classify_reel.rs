//! The dictionaries of a classification (`esq_core::classify_tree`) against those SketchUp 2024
//! wrote in real files: for each classified definition of the corpus, the tree built from the
//! `.skc` the file carries (IFC 2x3, IFC 4, IFC 4.3, bSDD classifications) is the one in the
//! file, dictionary by dictionary: names and order, `attribute_type`, `options`, `is_hidden`.
//! What the extension that classified them (BIM-Tools' IFC Manager) added or emptied is left
//! aside: property sets beside the attributes (`Pset_…`, `SizeSet`), a value written or
//! removed, an `IfcText` it put under an IFC 4 `Description`. Skipped without the corpus.

use std::path::PathBuf;

use esq_core::classify_tree::{Dict, build};
use esq_core::model::{AttrValue, Model};
use slotmap::Key;

fn corpus_dir() -> Option<PathBuf> {
    std::env::var_os("ESQ_SKP_REEL")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("tools").join("skp-reel")))
}

/// The nested dictionaries of `name` (owner `owner`), as the reader keeps them.
fn file_tree(m: &Model, owner: &str, name: &str, depth: usize) -> Vec<Dict> {
    let key = format!("ad:{owner}|{name}");
    let Some(a) = m.attr_store.get(&key) else { return Vec::new() };
    a.iter()
        .map(|(n, kv)| Dict {
            name: n.clone(),
            attrs: kv.clone(),
            children: if depth > 8 { Vec::new() } else { file_tree(m, &key, n, depth + 1) },
        })
        .collect()
}

fn added_by_extension(parent: &str, d: &Dict) -> bool {
    d.name.starts_with("Pset_") || d.name == "SizeSet" || (parent == "Description" && d.name == "IfcText")
}

fn compare(ours: &[Dict], theirs: &[Dict], parent: &str, path: &str, out: &mut Vec<String>) {
    let ours_has = |n: &str| ours.iter().any(|d| d.name == n);
    let theirs: Vec<&Dict> = theirs.iter().filter(|d| ours_has(&d.name) || !added_by_extension(parent, d)).collect();
    let a: Vec<&str> = ours.iter().map(|d| d.name.as_str()).collect();
    let b: Vec<&str> = theirs.iter().map(|d| d.name.as_str()).collect();
    if a != b {
        out.push(format!("{path} : {a:?} ≠ {b:?}"));
        return;
    }
    for (x, y) in ours.iter().zip(theirs) {
        let p = format!("{path}/{}", x.name);
        for k in ["attribute_type", "options", "is_hidden"] {
            if x.attrs.get(k) != y.attrs.get(k) {
                out.push(format!("{p} {k} : {:?} ≠ {:?}", x.attrs.get(k), y.attrs.get(k)));
            }
        }
        compare(&x.children, &y.children, &x.name, &p, out);
    }
}

/// A schema the file names otherwise than its `.skc` does (« gbXML 5.10 » beside the title
/// « gbXML » in TestUp's MultipleClassifications.skp): SketchUp knows it by the title
/// (`Classifications#keys`, the keys of `AppliedSchemaTypes`), and so does the reader.
#[test]
fn schemas_are_named_by_their_title() {
    let dir = esq_core::content::folder("Classifications").expect("classements livrés avec Esquisse");
    let data = std::fs::read(dir.join("IFC 2x3.skc")).unwrap();
    let mut m = Model::new();
    let mut s = esq_core::classify::parse_skc(&data, "C:/x/IFC 2x3.skc").unwrap();
    s.name = "IFC 2x3 (copie)".into();
    esq_core::classify::store_schema(&mut m, &s);
    esq_core::classify::keep_file(&mut m, &s.name, data);
    let bytes = esq_skp::native::write::write(&m, None, None, &Default::default()).unwrap();
    let (back, _) = esq_skp::native::read_bytes(&bytes).unwrap();
    let names: Vec<String> = esq_core::classify::schemas(&back).into_iter().map(|s| s.name).collect();
    assert_eq!(names, vec!["IFC 2x3".to_string()]);
    assert!(esq_core::classify::file_of(&back, "IFC 2x3").is_some());
}

#[test]
fn classification_trees_are_sketchups() {
    let Some(dir) = corpus_dir() else { return };
    let files = [
        "BIM-Tools_SketchUp-IFC-Manager/555f0e51_ifc-silly-sample-scene.skp",
        "BIM-Tools_SketchUp-IFC-Manager/952a47bb_ifc-hbim-convento-textures.skp",
    ];
    let (mut trees, mut diffs) = (0, Vec::new());
    for f in files {
        let Ok(bytes) = std::fs::read(dir.join(f)) else {
            eprintln!("{f} absent : sauté");
            continue;
        };
        let (m, _) = esq_skp::native::read_bytes(&bytes).unwrap();
        for (id, d) in m.defs.iter() {
            let Some(applied) = d.attributes.get("AppliedSchemaTypes") else { continue };
            let owner = format!("d:0:{}", id.data().as_ffi());
            for (schema, ty) in applied {
                let AttrValue::Text(ty) = ty else { continue };
                let skc = m.files.get(&esq_core::classify::file_key(schema)).expect("le .skc du schéma");
                let entries = esq_core::classify::zip_entries(&skc.0).unwrap();
                let c = build(&entries, ty, 0).unwrap_or_else(|| panic!("{f} : {schema} {ty} introuvable"));
                assert_eq!(
                    d.attributes[schema.as_str()].get("SchemaType"),
                    Some(&AttrValue::Text(format!("{}:{}", c.namespace, c.name))),
                    "{f} : {}",
                    d.name
                );
                let mut out = Vec::new();
                compare(&c.children, &file_tree(&m, &owner, schema, 0), "", "", &mut out);
                diffs.extend(out.into_iter().map(|x| format!("{f} {} {schema} {ty} {x}", d.name)));
                trees += 1;
            }
        }
    }
    assert!(diffs.is_empty(), "{} écarts :\n{}", diffs.len(), diffs[..diffs.len().min(20)].join("\n"));
    eprintln!("{trees} arbres identiques");
}
