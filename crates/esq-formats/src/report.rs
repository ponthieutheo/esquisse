//! The "Résultats d'exportation" texts of SketchUp's mesh exporters (OBJ, 3DS, FBX, XSI,
//! VRML): what was read from the model, then what the file holds.

use std::collections::HashSet;

use esq_core::model::{MaterialId, Model};

use crate::scene::{Cache, placements};

/// "Entités SketchUp traitées".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Processed {
    pub faces: usize,
    pub edges: usize,
    pub instances: usize,
    pub materials: usize,
    pub textures: usize,
    pub cameras: usize,
}

pub fn processed(cache: &mut Cache, cameras: usize) -> Processed {
    let mut p = Processed { cameras, ..Default::default() };
    let mut mats: HashSet<MaterialId> = HashSet::new();
    for pl in placements(cache) {
        let c = cache.get(pl.def);
        p.faces += c.faces.len();
        p.edges += c.edges.len();
        if pl.def.is_some() {
            p.instances += 1;
        }
        for f in &c.faces {
            mats.extend(f.front.or(pl.inherited));
            mats.extend(f.back.or(pl.inherited));
        }
    }
    let model: &Model = cache.model;
    p.materials = mats.len();
    p.textures = mats.iter().filter(|m| model.materials.get(**m).is_some_and(|x| x.ext.texture.is_some())).count();
    p
}

/// The whole text: `format` is "OBJ", "3DS"…; `exported` are (label, count) pairs such as
/// ("Groupes", 3).
pub fn text(format: &str, p: &Processed, exported: &[(&str, usize)]) -> String {
    let mut s = String::from("Entités SketchUp traitées : \n\n");
    for (label, n) in [
        ("Faces", p.faces),
        ("arêtes", p.edges),
        ("Cas", p.instances),
        ("Matières", p.materials),
        ("Textures", p.textures),
        ("Caméras", p.cameras),
    ] {
        s.push_str(&format!("  {n} {label}\n"));
    }
    s.push('\n');
    s.push_str(&match format {
        "3DS" => "Entités 3DS exportées :\n\n".to_string(),
        f => format!("Entités {f} exportées : \n\n"),
    });
    for (label, n) in exported {
        s.push_str(&format!("  {n} {label}\n"));
    }
    s
}
