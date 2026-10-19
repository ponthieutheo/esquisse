//! Window › Model Info › Statistics › Fix Problems, and Preferences › General › Check for
//! problems: finds what is broken in a model (references to things that are gone, faces
//! that are no longer faces, edges of no length…) and repairs it, as SketchUp's validity
//! check does.

use std::collections::{HashMap, HashSet};

use glam::DVec3;

use crate::math::newell_normal;
use crate::model::{DefId, EdgeId, Entities, FaceId, Model, VertexId};

/// One problem found, described for the user.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub text: String,
}

const TOL: f64 = 1e-6;

/// Where a collection is, for the messages.
fn place(m: &Model, d: Option<DefId>) -> String {
    match d {
        None => "le modèle".into(),
        Some(d) => format!("« {} »", m.defs[d].name),
    }
}

fn collections(m: &Model) -> Vec<Option<DefId>> {
    std::iter::once(None).chain(m.defs.keys().map(Some)).collect()
}

/// Lists the problems of a model without changing it.
pub fn check(m: &Model) -> Vec<Problem> {
    let mut c = m.clone();
    fix_model(&mut c)
}

/// Repairs a model; returns what was repaired.
pub fn fix_model(m: &mut Model) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut say = |t: String| out.push(Problem { text: t });
    let layers = m.layers.len().max(1);
    let mats: HashSet<_> = m.materials.keys().collect();
    let defs: HashSet<DefId> = m.defs.keys().collect();
    // Definitions that contain themselves (directly or not) would recurse forever.
    let cyclic = cyclic_defs(m);
    for d in collections(m) {
        let where_ = place(m, d);
        let ents = m.entities_mut(d);
        let n = fix_entities(ents, layers, &mats);
        for (what, count) in n {
            if count > 0 {
                say(format!("{count} {what} dans {where_}"));
            }
        }
        // Instances of definitions that are gone, or of a definition that contains them.
        let bad: Vec<_> = ents
            .instances
            .iter()
            .filter(|(_, i)| !defs.contains(&i.def) || d.is_some_and(|me| cyclic.contains(&(me, i.def))))
            .map(|(k, _)| k)
            .collect();
        if !bad.is_empty() {
            say(format!("{} copie(s) de composant invalide(s) supprimée(s) dans {where_}", bad.len()));
            for k in bad {
                ents.instances.remove(k);
            }
        }
    }
    // Scenes hiding tags that are gone.
    let mut scene_fix = 0;
    for s in m.scenes.iter_mut() {
        if let Some(h) = &mut s.hidden_layers {
            let before = h.len();
            h.retain(|l| *l < layers);
            scene_fix += before - h.len();
        }
    }
    if scene_fix > 0 {
        say(format!("{scene_fix} référence(s) d'étiquette de scène corrigée(s)"));
    }
    out
}

/// (container, contained) pairs where `contained` is, or contains, `container`.
fn cyclic_defs(m: &Model) -> HashSet<(DefId, DefId)> {
    let mut out = HashSet::new();
    let children: HashMap<DefId, Vec<DefId>> =
        m.defs.iter().map(|(k, d)| (k, d.entities.instances.values().map(|i| i.def).collect())).collect();
    for (&d, kids) in &children {
        for &k in kids {
            // Does `k` reach `d`?
            let mut stack = vec![k];
            let mut seen = HashSet::new();
            while let Some(x) = stack.pop() {
                if x == d {
                    out.insert((d, k));
                    break;
                }
                if seen.insert(x) {
                    stack.extend(children.get(&x).into_iter().flatten().copied());
                }
            }
        }
    }
    out
}

/// Repairs one collection; returns (what, how many) for each kind of repair.
fn fix_entities(
    e: &mut Entities,
    layers: usize,
    mats: &HashSet<crate::model::MaterialId>,
) -> Vec<(&'static str, usize)> {
    // Edges whose vertices are gone, or of no length.
    let bad_edges: Vec<EdgeId> = e
        .edges
        .iter()
        .filter(|(_, x)| {
            !e.verts.contains_key(x.v[0])
                || !e.verts.contains_key(x.v[1])
                || x.v[0] == x.v[1]
                || e.verts[x.v[0]].pos.distance(e.verts[x.v[1]].pos) < TOL
        })
        .map(|(k, _)| k)
        .collect();
    let n_edges = bad_edges.len();
    for k in bad_edges {
        e.remove_edge_raw(k);
    }
    // Duplicate edges (same two vertices).
    let mut seen = HashMap::new();
    let mut dups = Vec::new();
    for (k, x) in e.edges.iter() {
        let key = if x.v[0] < x.v[1] { (x.v[0], x.v[1]) } else { (x.v[1], x.v[0]) };
        if seen.insert(key, k).is_some() {
            dups.push(k);
        }
    }
    let n_dups = dups.len();
    for k in dups {
        e.remove_edge_raw(k);
    }
    // Faces with missing vertices, fewer than three corners, no area, or not flat.
    let bad_faces: Vec<FaceId> = e
        .faces
        .iter()
        .filter(|(_, f)| {
            let all: Vec<VertexId> = f.all_vertices().collect();
            if f.outer.len() < 3 || all.iter().any(|v| !e.verts.contains_key(*v)) {
                return true;
            }
            let pts = e.loop_points(&f.outer);
            let n = newell_normal(&pts);
            if n == DVec3::ZERO {
                return true;
            }
            // SketchUp's tolerance (0.001") or one millionth of the face's size.
            let o = pts[0];
            let size = pts.iter().map(|p| p.distance(o)).fold(0.0, f64::max);
            let tol = (size * 1e-6).max(0.0254);
            all.iter().any(|v| (e.verts[*v].pos - o).dot(n.normalize()).abs() > tol)
        })
        .map(|(k, _)| k)
        .collect();
    let n_faces = bad_faces.len();
    for f in bad_faces {
        e.remove_face(f);
    }
    // Faces whose normal no longer matches their outer loop.
    let mut n_normals = 0;
    let ids: Vec<FaceId> = e.faces.keys().collect();
    for f in ids {
        let n = newell_normal(&e.loop_points(&e.faces[f].outer));
        if n != DVec3::ZERO && n.dot(e.faces[f].normal) < 1.0 - 1e-6 {
            e.faces[f].normal = n;
            n_normals += 1;
        }
    }
    // References to tags and materials that are gone.
    let mut n_refs = 0;
    let mut fix_layer = |l: &mut usize| {
        if *l >= layers {
            *l = 0;
            n_refs += 1;
        }
    };
    e.faces.values_mut().for_each(|f| fix_layer(&mut f.layer));
    e.edges.values_mut().for_each(|x| fix_layer(&mut x.layer));
    e.instances.values_mut().for_each(|i| fix_layer(&mut i.layer));
    e.guides.values_mut().for_each(|g| fix_layer(&mut g.layer));
    e.annots.values_mut().for_each(|a| fix_layer(&mut a.layer));
    e.sections.values_mut().for_each(|s| fix_layer(&mut s.layer));
    e.snaps.iter_mut().for_each(|s| fix_layer(&mut s.layer));
    e.spaces.iter_mut().for_each(|s| fix_layer(&mut s.layer));
    let mut n_mats = 0;
    for f in e.faces.values_mut() {
        for side in [&mut f.front, &mut f.back] {
            if side.is_some_and(|m| !mats.contains(&m)) {
                *side = None;
                n_mats += 1;
            }
        }
    }
    for i in e.instances.values_mut() {
        if i.material.is_some_and(|m| !mats.contains(&m)) {
            i.material = None;
            n_mats += 1;
        }
    }
    // An active section plane that is gone.
    let mut n_sec = 0;
    if e.active_section.is_some_and(|s| !e.sections.contains_key(s)) {
        e.active_section = None;
        n_sec = 1;
    }
    e.rebuild_topology();
    // Vertices left alone.
    let before = e.verts.len();
    e.remove_orphan_vertices();
    let n_verts = before - e.verts.len();
    vec![
        ("arête(s) invalide(s) supprimée(s)", n_edges),
        ("arête(s) en double supprimée(s)", n_dups),
        ("face(s) invalide(s) supprimée(s)", n_faces),
        ("normale(s) de face corrigée(s)", n_normals),
        ("référence(s) d'étiquette corrigée(s)", n_refs),
        ("référence(s) de matière corrigée(s)", n_mats),
        ("plan de section actif corrigé", n_sec),
        ("sommet(s) isolé(s) supprimé(s)", n_verts),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;

    fn square(e: &mut Entities, z: f64) -> FaceId {
        let v: Vec<_> = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
            .iter()
            .map(|(x, y)| e.add_vertex_raw(DVec3::new(*x, *y, z)))
            .collect();
        e.add_face_raw(v, vec![]).unwrap()
    }

    #[test]
    fn a_sound_model_has_no_problem() {
        let mut m = Model::new();
        square(&mut m.root, 0.0);
        assert!(check(&m).is_empty(), "{:?}", check(&m));
    }

    #[test]
    fn broken_references_and_degenerate_geometry_are_repaired() {
        let mut m = Model::new();
        let f = square(&mut m.root, 0.0);
        let mat = m.add_material("Rouge", [255, 0, 0]);
        m.root.faces[f].front = Some(mat);
        m.materials.remove(mat);
        m.root.faces[f].layer = 7;
        // A stray vertex and a zero-length edge.
        let a = m.root.add_vertex_raw(DVec3::new(50.0, 0.0, 0.0));
        let b = m.root.add_vertex_raw(DVec3::new(50.0, 0.0, 0.0));
        m.root.add_edge_raw(a, b);
        m.root.add_vertex_raw(DVec3::new(99.0, 0.0, 0.0));
        // A face whose normal is wrong.
        m.root.faces[f].normal = -m.root.faces[f].normal;
        let found = check(&m);
        assert!(found.len() >= 4, "{found:?}");
        assert!(m.root.faces[f].front.is_some(), "check leaves the model alone");
        let fixed = fix_model(&mut m);
        assert_eq!(fixed, found);
        assert_eq!((m.root.faces[f].front, m.root.faces[f].layer), (None, 0));
        assert!(m.root.faces[f].normal.z > 0.99);
        assert_eq!(m.root.edges.len(), 4);
        assert_eq!(m.root.verts.len(), 4);
        assert!(check(&m).is_empty());
    }
}
