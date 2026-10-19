//! The classification systems: IFC 2x3 (TC1) and IFC 4 (ADD2 TC1), as `.skc` files. The XML
//! schemas are buildingSMART's, unchanged (`tools/content/ifc/`, see its SOURCE.txt for where
//! they come from and their licence); the list of types offered, the descriptions and the
//! thumbnail are ours.

use std::collections::{HashMap, HashSet};

use super::noise::png_rgba;

pub const COUNT: usize = 2;

fn source_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/content/ifc")
}

/// The value of attribute `k` in an element's opening tag.
fn attr<'a>(tag: &'a str, k: &str) -> Option<&'a str> {
    let i = tag.find(&format!(" {k}=\""))? + k.len() + 3;
    Some(&tag[i..i + tag[i..].find('"')?])
}

/// The types a component can be given: every element of the schema that is not abstract and
/// whose substitution group leads to `IfcProduct` (physical elements, spaces, storeys…).
pub fn product_types(xsd: &str) -> Vec<String> {
    let mut parent = HashMap::new();
    let mut abstract_ = HashSet::new();
    let mut order = Vec::new();
    for part in xsd.split("<xs:element ").skip(1) {
        let tag = &part[..part.find('>').unwrap_or(part.len())];
        let tag = format!(" {tag}");
        let Some(name) = attr(&tag, "name") else { continue };
        if !name.starts_with("Ifc") {
            continue;
        }
        if let Some(group) = attr(&tag, "substitutionGroup") {
            parent.insert(name.to_string(), group.rsplit(':').next().unwrap_or(group).to_string());
        }
        if attr(&tag, "abstract") == Some("true") {
            abstract_.insert(name.to_string());
        }
        order.push(name.to_string());
    }
    let is_product = |n: &str| {
        let mut cur = n.to_string();
        for _ in 0..32 {
            if cur == "IfcProduct" {
                return true;
            }
            match parent.get(&cur) {
                Some(p) => cur = p.clone(),
                None => return false,
            }
        }
        false
    };
    let mut seen = HashSet::new();
    order
        .into_iter()
        .filter(|n| !abstract_.contains(n) && n != "IfcProduct" && is_product(n) && seen.insert(n.clone()))
        .collect()
}

fn filter(types: &[String]) -> String {
    let mut s = String::new();
    for t in types {
        s.push_str(t);
        s.push('\n');
    }
    // Attributes of the XML encoding (ISO 10303-28), not of the building: never shown.
    s.push_str("\n// Blacklist Attributes\n{\n  id\n  ref\n  href\n  proxy\n  edo\n  cType\n  itemType\n  arraySize\n  pos\n}\n");
    s
}

fn icon() -> Vec<u8> {
    let n = 64u32;
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            // Three stacked slabs on a pale square.
            let slab = (y as i32 - 12).rem_euclid(16) < 10 && (12..56).contains(&y) && (10..54).contains(&x);
            let c = if slab { [40, 90, 160] } else { [230, 236, 242] };
            px.extend([c[0], c[1], c[2], 255]);
        }
    }
    png_rgba(n, n, px)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const NOTICE: &str = "Schéma IFC de buildingSMART International Ltd., inclus sans modification.\r\n\
Copyright (c) buildingSMART International Ltd. Licence : Creative Commons Attribution -\r\n\
Pas de modification 4.0 International (CC BY-ND 4.0), https://creativecommons.org/licenses/by-nd/4.0/\r\n\
Source : https://github.com/buildingSMART/Standards-builds (miroir de standards.buildingsmart.org).\r\n\
La liste des types (.xsd.filter), les descriptions et la vignette sont d'Esquisse (MIT).\r\n";

fn skc(title: &str, description: &str, schema: (&str, Vec<u8>), extra: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let xsd = String::from_utf8_lossy(&schema.1).into_owned();
    let types = product_types(&xsd);
    assert!(types.iter().any(|t| t == "IfcWall") && types.len() > 50, "{title} : {} types", types.len());
    let doc = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<classificationDocument xmlns=\"http://www.sketchup.com/schemas/sketchup/1.0/classification\" xmlns:r=\"http://www.sketchup.com/schemas/1.0/references\" xmlns:cls=\"http://www.sketchup.com/schemas/sketchup/1.0/classification\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n  <cls:Classification xsdFile=\"Schemas/{}\"></cls:Classification>\n</classificationDocument>\n",
        schema.0
    );
    // Every element SketchUp writes, in its order: it loads no schema whose
    // properties lack one (a .skp carrying such a schema opened without it).
    let props = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<documentProperties xmlns=\"http://www.sketchup.com/schemas/1.0/documentproperties\" xmlns:dp=\"http://www.sketchup.com/schemas/1.0/documentproperties\">\n  <dp:title>{}</dp:title>\n  <dp:description>{}</dp:description>\n  <dp:creator>buildingSMART International</dp:creator>\n  <dp:keywords></dp:keywords>\n  <dp:lastModifiedBy></dp:lastModifiedBy>\n  <dp:revision>1</dp:revision>\n  <dp:created>2026-01-01T00:00:00</dp:created>\n  <dp:modified>2026-01-01T00:00:00</dp:modified>\n  <dp:thumbnail>doc_thumbnail.png</dp:thumbnail>\n  <dp:generator dp:name=\"Classification\" dp:version=\"1\"/>\n</documentProperties>\n",
        escape(title),
        escape(description)
    );
    let refs = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<references xmlns=\"http://www.sketchup.com/schemas/1.0/references\" />\n";
    let mut files = vec![
        ("document.xml".to_string(), doc.into_bytes()),
        ("documentProperties.xml".to_string(), props.into_bytes()),
        ("doc_thumbnail.png".to_string(), icon()),
        ("references.xml".to_string(), refs.as_bytes().to_vec()),
        (format!("Schemas/{}", schema.0), schema.1.clone()),
        (format!("Schemas/{}.filter", schema.0), filter(&types).into_bytes()),
        ("Schemas/LICENCE-buildingSMART.txt".to_string(), NOTICE.as_bytes().to_vec()),
    ];
    for (name, bytes) in extra {
        files.push((format!("Schemas/{name}"), bytes.clone()));
    }
    crate::zip::write(&files)
}

/// (file stem, bytes) of each classification.
pub fn all() -> Vec<(String, Vec<u8>)> {
    let d = source_dir();
    let read = |n: &str| std::fs::read(d.join(n)).unwrap_or_else(|e| panic!("{n} : {e}"));
    vec![
        (
            "IFC 2x3".into(),
            skc(
                "IFC 2x3",
                "Industry Foundation Classes 2x3 (TC1), le format d'échange openBIM de buildingSMART.",
                ("IFC2X3.xsd", read("IFC2X3.xsd")),
                &[("ex.xsd", read("ex.xsd"))],
            ),
        ),
        (
            "IFC 4".into(),
            skc(
                "IFC 4",
                "Industry Foundation Classes 4 (ADD2 TC1, ISO 16739-1:2018), le format d'échange openBIM de buildingSMART.",
                ("IFC4_ADD2_TC1.xsd", read("IFC4_ADD2_TC1.xsd")),
                &[],
            ),
        ),
    ]
}
