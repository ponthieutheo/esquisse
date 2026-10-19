//! `Sketchup::Selection`.

use std::collections::HashSet;

use esq_core::model::{DefId, Entities, EntityRef};
use serde_json::{Value as J, json};

use super::misc::active_def;
use crate::bridge::{Ctx, R, eref_handle, parse_h, to_eref};

pub fn run(c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    if op.starts_with("sel.") {
        prune(c);
    }
    Some(match op {
        "sel.get" => {
            let ctx = active_def(c);
            Ok(json!(c.sel.iter().filter_map(|&e| eref_handle(ctx, e)).collect::<Vec<_>>()))
        }
        "sel.set" => set(c, a),
        "sel.invert" => {
            let ctx = active_def(c);
            let all = all_refs(c.model.entities(ctx));
            let cur: HashSet<EntityRef> = c.sel.iter().copied().collect();
            *c.sel = all.into_iter().filter(|e| !cur.contains(e)).collect();
            Ok(J::Null)
        }
        "sel.kind" => Ok(kind(c)),
        _ => return None,
    })
}

/// What a script erased (or exploded, or cleared) leaves the selection, as in SketchUp.
fn prune(c: &mut Ctx) {
    let ctx = active_def(c);
    if ctx.is_some_and(|d| !c.model.defs.contains_key(d)) {
        c.sel.clear();
        return;
    }
    let ents = c.model.entities(ctx);
    c.sel.retain(|e| ents.contains(*e));
}

fn set(c: &mut Ctx, a: &J) -> R {
    let mut v = Vec::new();
    for h in a["hs"].as_array().ok_or("hs attendu")? {
        v.push(to_eref(&parse_h(h.as_str().ok_or("poignée attendue")?)?)?);
    }
    *c.sel = v;
    Ok(J::Null)
}

/// Everything that can be selected in a collection.
pub fn all_refs(e: &Entities) -> Vec<EntityRef> {
    e.instances
        .keys()
        .map(EntityRef::Instance)
        .chain(e.faces.keys().map(EntityRef::Face))
        .chain(e.edges.keys().map(EntityRef::Edge))
        .chain(e.guides.keys().map(EntityRef::Guide))
        .chain(e.annots.keys().map(EntityRef::Annot))
        .chain(e.sections.keys().map(EntityRef::Section))
        .chain(e.mark_refs())
        .collect()
}

/// `is_curve?` / `is_surface?`.
fn kind(c: &mut Ctx) -> J {
    let ctx: Option<DefId> = active_def(c);
    let e = c.model.entities(ctx);
    let edges: Vec<_> =
        c.sel.iter().filter_map(|x| if let EntityRef::Edge(id) = x { Some(*id) } else { None }).collect();
    let faces: Vec<_> =
        c.sel.iter().filter_map(|x| if let EntityRef::Face(id) = x { Some(*id) } else { None }).collect();
    let curve = !edges.is_empty() && faces.is_empty() && edges.len() == c.sel.len() && {
        let first = e.edges.get(edges[0]).and_then(|x| x.curve);
        first.is_some()
            && edges.iter().all(|id| e.edges.get(*id).and_then(|x| x.curve) == first)
            && e.edges.values().filter(|x| x.curve == first).count() == edges.len()
    };
    let surface = !faces.is_empty() && faces.len() == c.sel.len() && {
        // Connected through shared edges.
        let set: HashSet<_> = faces.iter().copied().collect();
        let mut seen = HashSet::from([faces[0]]);
        let mut stack = vec![faces[0]];
        while let Some(f) = stack.pop() {
            for ed in e.face_edges(f) {
                for g in &e.edges[ed].faces {
                    if set.contains(g) && seen.insert(*g) {
                        stack.push(*g);
                    }
                }
            }
        }
        seen.len() == set.len()
    };
    json!({"curve": curve, "surface": surface})
}
