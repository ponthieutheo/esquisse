//! A model read from a file (`.esq`, `.skp`) is checked before anything uses it: the keys
//! it holds must name entities it has, a definition must not contain itself, and the
//! instance tree must not unfold into an absurd number of placements. The rest of Esquisse
//! indexes entities by their keys and walks the tree recursively: a forged file would make
//! it panic, overflow the stack or never end (`docs/securite.md`).

use std::collections::{HashMap, HashSet};

use crate::model::{DefId, Entities, GlueRef, InstanceId, Model};

/// The most placements of definitions a model may unfold into (each instance, at every
/// level): far above real models (a city of a million placed components), far below what
/// a few nested definitions instancing the next one twice make (2^60).
pub const MAX_PLACEMENTS: u64 = 100_000_000;

/// The deepest nesting of groups and components kept (the native `.skp` reader's limit;
/// SketchUp documents none, real models stay under twenty).
pub const MAX_DEPTH: usize = 256;

/// Models the tests of every crate open, render, export and give to Ruby.
#[doc(hidden)]
pub mod samples {
    use super::*;
    use crate::model::{Definition, Instance};
    use glam::{DMat4, DVec3};

    fn def(m: &mut Model, name: &str, face: bool) -> DefId {
        let mut e = Entities::default();
        if face {
            let v: Vec<_> = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]
                .iter()
                .map(|(x, y)| e.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
                .collect();
            e.add_face_raw(v, vec![]);
        }
        m.defs.insert(Definition {
            name: name.into(),
            is_group: false,
            description: String::new(),
            entities: e,
            attributes: Default::default(),
            ext: Default::default(),
        })
    }

    fn inst(def: DefId, dz: f64) -> Instance {
        Instance {
            def,
            xform: DMat4::from_translation(DVec3::new(0.0, 0.0, dz)),
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        }
    }

    /// `n` definitions, each holding the next one (and a square every hundred levels), the
    /// first placed in the model.
    pub fn chain(n: usize) -> Model {
        let mut m = Model::new();
        let ds: Vec<DefId> = (0..n).map(|i| def(&mut m, &format!("Niveau {i}"), i % 100 == 0 || i + 1 == n)).collect();
        for w in ds.windows(2) {
            m.defs[w[0]].entities.instances.insert(inst(w[1], 10.0));
        }
        m.root.instances.insert(inst(ds[0], 0.0));
        m
    }

    /// A holds B, B holds A (twice), A placed in the model.
    pub fn cycle() -> Model {
        let mut m = Model::new();
        let (a, b) = (def(&mut m, "A", true), def(&mut m, "B", true));
        m.defs[a].entities.instances.insert(inst(b, 10.0));
        m.defs[b].entities.instances.insert(inst(a, 10.0));
        m.defs[b].entities.instances.insert(inst(a, 20.0));
        m.root.instances.insert(inst(a, 0.0));
        m
    }
}

/// Removes from `e` the edges and faces that name a vertex it does not have (the topology
/// cannot be rebuilt with them), and the references nothing else repairs. The rest (tags,
/// materials, instances of missing definitions) is left to `crate::validity`, which the
/// application runs after opening and which tells the user what it fixed. Returns how many
/// entities went.
fn repair_entities(e: &mut Entities) -> usize {
    let mut gone = 0;
    let bad_edges: Vec<_> =
        e.edges.iter().filter(|(_, x)| x.v.iter().any(|v| !e.verts.contains_key(*v))).map(|(k, _)| k).collect();
    for k in bad_edges {
        e.edges.remove(k);
        gone += 1;
    }
    let bad_faces: Vec<_> = e
        .faces
        .iter()
        .filter(|(_, f)| f.outer.len() < 3 || f.all_vertices().any(|v| !e.verts.contains_key(v)))
        .map(|(k, _)| k)
        .collect();
    for k in bad_faces {
        e.faces.remove(k);
        gone += 1;
    }
    let (faces, insts): (HashSet<_>, HashSet<_>) = (e.faces.keys().collect(), e.instances.keys().collect());
    for i in e.instances.values_mut() {
        i.ext.glued_to = i.ext.glued_to.filter(|g| match g {
            GlueRef::Face(f) => faces.contains(f),
            GlueRef::Instance(x) => insts.contains(x),
        });
    }
    if e.active_section.is_some_and(|s| !e.sections.contains_key(s)) {
        e.active_section = None;
    }
    gone
}
impl Model {
    /// Drops the edges and faces that name a vertex the model does not have, and the glue
    /// and section references that point nowhere. Returns how many entities went (0 for a
    /// sound model).
    pub fn repair_references(&mut self) -> usize {
        if self.layers.is_empty() {
            self.layers = Model::default().layers;
        }
        let mut gone = repair_entities(&mut self.root);
        for d in self.defs.values_mut() {
            gone += repair_entities(&mut d.entities);
        }
        gone
    }

    /// Removes the instances that make a definition contain itself, directly or through
    /// others (SketchUp has no recursive components). Returns how many went.
    pub fn break_definition_cycles(&mut self) -> usize {
        let cut = self.walk_definitions().1;
        for (d, i) in &cut {
            self.defs[*d].entities.instances.remove(*i);
        }
        cut.len()
    }

    /// Removes the instances placed deeper than [`MAX_DEPTH`] levels from the model (a
    /// definition that is not placed counts as placed at the first level). Many walks of
    /// the tree are recursive (display, exports, solids, Ruby): a chain of 100 000 nested
    /// definitions overflowed the stack. Call it on a model without cycles. Returns how
    /// many instances went.
    pub fn cut_deep_nesting(&mut self) -> usize {
        let order = self.walk_definitions().0;
        let mut depth: HashMap<DefId, usize> = self.defs.keys().map(|d| (d, 1)).collect();
        // Parents before children: the reverse of `order`.
        for d in order.iter().rev() {
            let here = depth[d];
            let kids: Vec<DefId> = self.defs[*d].entities.instances.values().map(|i| i.def).collect();
            for k in kids {
                if let Some(x) = depth.get_mut(&k) {
                    *x = (*x).max(here + 1);
                }
            }
        }
        let placed_before = self.placed_definitions();
        let mut cut = 0;
        for (d, def) in self.defs.iter_mut() {
            if depth[&d] >= MAX_DEPTH {
                cut += def.entities.instances.len();
                def.entities.instances.clear();
            }
        }
        if cut > 0 {
            // What the cut left unplaced is the deep part of the chain: it goes too (a model
            // keeping 100 000 unplaced definitions slows every list of components down).
            let placed = self.placed_definitions();
            let gone: HashSet<DefId> = placed_before.difference(&placed).copied().collect();
            for d in &gone {
                self.defs.remove(*d);
            }
            let drop = |e: &mut Entities| e.instances.retain(|_, i| !gone.contains(&i.def));
            drop(&mut self.root);
            for def in self.defs.values_mut() {
                drop(&mut def.entities);
            }
        }
        cut
    }

    /// The definitions placed in the model (reachable from its root), walked iteratively.
    fn placed_definitions(&self) -> HashSet<DefId> {
        let mut seen = HashSet::new();
        let mut stack: Vec<&Entities> = vec![&self.root];
        while let Some(e) = stack.pop() {
            for i in e.instances.values() {
                if let Some(d) = self.defs.get(i.def)
                    && seen.insert(i.def)
                {
                    stack.push(&d.entities);
                }
            }
        }
        seen
    }

    /// How many placements of definitions the model unfolds into (saturating); call it on a
    /// model without cycles.
    pub fn placements(&self) -> u64 {
        let order = self.walk_definitions().0;
        let mut count: HashMap<DefId, u64> = HashMap::new();
        let sum = |e: &Entities, count: &HashMap<DefId, u64>| {
            e.instances.values().fold(0u64, |s, i| s.saturating_add(count.get(&i.def).copied().unwrap_or(1)))
        };
        // Children come before their parents in `order`.
        for d in order {
            let n = 1u64.saturating_add(sum(&self.defs[d].entities, &count));
            count.insert(d, n);
        }
        sum(&self.root, &count)
    }

    /// Definitions in an order where every definition comes after those it instances, and
    /// the instances that close a cycle. Iterative: a chain of 100 000 nested definitions
    /// does not overflow the stack.
    fn walk_definitions(&self) -> (Vec<DefId>, Vec<(DefId, InstanceId)>) {
        let children = |d: DefId| -> Vec<(InstanceId, DefId)> {
            self.defs[d]
                .entities
                .instances
                .iter()
                .filter(|(_, i)| self.defs.contains_key(i.def))
                .map(|(k, i)| (k, i.def))
                .collect()
        };
        // 1 = being walked, 2 = done.
        let mut state: HashMap<DefId, u8> = HashMap::new();
        let (mut order, mut cut) = (Vec::new(), Vec::new());
        for start in self.defs.keys() {
            if state.contains_key(&start) {
                continue;
            }
            state.insert(start, 1);
            let mut stack = vec![(start, children(start), 0usize)];
            while let Some(top) = stack.last_mut() {
                if top.2 < top.1.len() {
                    let (inst, child) = top.1[top.2];
                    top.2 += 1;
                    let parent = top.0;
                    match state.get(&child) {
                        Some(1) => cut.push((parent, inst)),
                        Some(_) => {}
                        None => {
                            state.insert(child, 1);
                            let c = children(child);
                            stack.push((child, c, 0));
                        }
                    }
                } else {
                    state.insert(top.0, 2);
                    order.push(top.0);
                    stack.pop();
                }
            }
        }
        (order, cut)
    }

    /// Everything above, for a model just read from a file: how many entities went (the
    /// topology must then be rebuilt), or an error when it unfolds into more than
    /// [`MAX_PLACEMENTS`].
    pub fn check_after_reading(&mut self) -> Result<usize, String> {
        let fixed = self.repair_references() + self.break_definition_cycles() + self.cut_deep_nesting();
        let n = self.placements();
        if n > MAX_PLACEMENTS {
            return Err(format!(
                "le modèle déplierait {} placements de composants (au plus {MAX_PLACEMENTS}) : fichier refusé",
                if n == u64::MAX { "plus de 10^19".to_string() } else { n.to_string() }
            ));
        }
        Ok(fixed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Definition, Instance};
    use glam::{DMat4, DVec3};

    fn def(m: &mut Model, name: &str) -> DefId {
        m.defs.insert(Definition {
            name: name.into(),
            is_group: false,
            description: String::new(),
            entities: Entities::default(),
            attributes: Default::default(),
            ext: Default::default(),
        })
    }

    fn inst(def: DefId) -> Instance {
        Instance {
            def,
            xform: DMat4::IDENTITY,
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        }
    }

    #[test]
    fn cycles_are_cut_and_placements_counted() {
        let mut m = Model::new();
        let (a, b) = (def(&mut m, "A"), def(&mut m, "B"));
        m.defs[a].entities.instances.insert(inst(b));
        m.defs[b].entities.instances.insert(inst(a));
        m.defs[b].entities.instances.insert(inst(b));
        m.root.instances.insert(inst(a));
        assert_eq!(m.break_definition_cycles(), 2);
        assert_eq!(m.placements(), 2);
        // 64 definitions each instancing the next one twice: 2^64 placements.
        let mut m = Model::new();
        let ds: Vec<DefId> = (0..64).map(|i| def(&mut m, &format!("D{i}"))).collect();
        for w in ds.windows(2) {
            m.defs[w[0]].entities.instances.insert(inst(w[1]));
            m.defs[w[0]].entities.instances.insert(inst(w[1]));
        }
        m.root.instances.insert(inst(ds[0]));
        assert_eq!(m.placements(), u64::MAX);
        assert!(m.check_after_reading().is_err());
    }

    #[test]
    fn dangling_keys_are_dropped() {
        let mut m = Model::new();
        let v: Vec<_> = (0..3).map(|i| m.root.add_vertex_raw(DVec3::new(i as f64, (i * i) as f64, 0.0))).collect();
        m.root.add_face_raw(v.clone(), vec![]).unwrap();
        m.root.verts.remove(v[0]);
        // The face and its two edges at the removed vertex.
        assert_eq!(m.repair_references(), 3);
        assert!(m.root.faces.is_empty() && m.root.edges.len() == 1);
        m.rebuild_topology();
    }
}
