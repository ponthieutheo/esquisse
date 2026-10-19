//! Groups and components: creating them from a selection, exploding, making unique.

use std::collections::{HashMap, HashSet};

use glam::{DMat4, DVec3};

use crate::model::{
    AnnotKind, DefId, Definition, EdgeId, Entities, EntityRef, Instance, InstanceId, LayerIdx, Material, MaterialId,
    Model, VertexId,
};

impl Model {
    /// Moves the selection of collection `ctx` into a new definition and places one
    /// instance of it where the geometry was.
    pub fn make_group(
        &mut self,
        ctx: Option<DefId>,
        items: &[EntityRef],
        is_group: bool,
        name: Option<String>,
    ) -> Option<InstanceId> {
        if items.is_empty() {
            return None;
        }
        let src = self.entities(ctx);
        let items = src.with_bounding_edges(items);
        let verts = src.vertices_of(&items);

        let mut origin = DVec3::splat(f64::INFINITY);
        for &v in &verts {
            origin = origin.min(src.pos(v));
        }
        for it in &items {
            match *it {
                EntityRef::Instance(i) => origin = origin.min(src.instances[i].xform.w_axis.truncate()),
                EntityRef::Guide(g) => origin = origin.min(src.guides[g].pos),
                EntityRef::Section(x) => origin = origin.min(src.sections[x].pos),
                EntityRef::Snap(id) => {
                    if let Some(s) = src.snap(id) {
                        origin = origin.min(s.pos);
                    }
                }
                EntityRef::Space(id) => {
                    for p in src.space(id).iter().flat_map(|s| &s.perimeter) {
                        origin = origin.min(*p);
                    }
                }
                EntityRef::Annot(a) => {
                    if let AnnotKind::Linear { a: p, .. } | AnnotKind::Text { point: p, .. } = src.annots[a].kind {
                        origin = origin.min(p);
                    }
                }
                _ => {}
            }
        }
        if !origin.is_finite() {
            origin = DVec3::ZERO;
        }
        let to_local = DMat4::from_translation(-origin);

        let mut ents = Entities::default();
        let mut map: HashMap<VertexId, VertexId> = HashMap::new();
        for &v in &verts {
            map.insert(v, ents.add_vertex_raw(src.pos(v) - origin));
        }
        // What went where, for the annotations attached to it (`attach.rs`).
        let mut copied = crate::attach::Copied { verts: map.clone(), ..Default::default() };
        let mut moved_annots = Vec::new();
        // The edges first: a face would otherwise create its edges in the order of its loop,
        // turning around those of a face that looks down. SketchUp keeps them as they were.
        let is_edge = |it: &&EntityRef| matches!(it, EntityRef::Edge(_));
        for it in items.iter().filter(is_edge).chain(items.iter().filter(|it| !is_edge(it))) {
            match *it {
                EntityRef::Edge(e) => {
                    let edge = &src.edges[e];
                    if let Some(ne) = ents.add_edge_raw(map[&edge.v[0]], map[&edge.v[1]]) {
                        copied.edges.insert(e, ne);
                        let n = &mut ents.edges[ne];
                        (n.soft, n.smooth, n.hidden, n.layer, n.curve) =
                            (edge.soft, edge.smooth, edge.hidden, edge.layer, edge.curve);
                    }
                }
                EntityRef::Face(f) => {
                    let face = &src.faces[f];
                    let outer = face.outer.iter().map(|v| map[v]).collect();
                    let holes = face.holes.iter().map(|h| h.iter().map(|v| map[v]).collect()).collect();
                    if let Some(nf) = ents.add_face_raw(outer, holes) {
                        copied.faces.insert(f, nf);
                        let n = &mut ents.faces[nf];
                        (n.normal, n.front, n.back, n.layer, n.hidden) =
                            (face.normal, face.front, face.back, face.layer, face.hidden);
                    }
                }
                EntityRef::Instance(i) => {
                    let mut inst = src.instances[i].clone();
                    inst.xform = to_local * inst.xform;
                    copied.insts.insert(i, ents.instances.insert(inst));
                }
                EntityRef::Guide(g) => {
                    copied.guides.insert(g, ents.guides.insert(src.guides[g].transformed(&to_local)));
                }
                EntityRef::Annot(a) => {
                    moved_annots.push(ents.annots.insert(src.annots[a].transformed(&to_local)));
                }
                EntityRef::Section(x) => {
                    let id = ents.sections.insert(src.sections[x].transformed(&to_local));
                    copied.sections.insert(x, id);
                    if src.active_section == Some(x) {
                        ents.active_section = Some(id);
                    }
                }
                // Snaps and spaces keep their id (unique in the source, so in the group too).
                EntityRef::Snap(id) => {
                    if let Some(s) = src.snap(id) {
                        ents.snaps.push(s.transformed(&to_local));
                        ents.last_mark_id = ents.last_mark_id.max(id);
                    }
                }
                EntityRef::Space(id) => {
                    if let Some(s) = src.space(id) {
                        ents.spaces.push(s.transformed(&to_local));
                        ents.last_mark_id = ents.last_mark_id.max(id);
                    }
                }
            }
        }
        // Their attributes and Ruby properties go with them.
        for it in &items {
            let key = match *it {
                EntityRef::Snap(id) => format!("n{id}"),
                EntityRef::Space(id) => format!("p{id}"),
                _ => continue,
            };
            if let Some(p) = src.props.get(&key) {
                ents.props.insert(key, p.clone());
            }
        }

        let name = name.unwrap_or_else(|| self.unique_def_name(if is_group { "Groupe" } else { "Composant" }));
        let def = self.defs.insert(Definition {
            name,
            is_group,
            description: String::new(),
            entities: ents,
            attributes: Default::default(),
            ext: Default::default(),
        });

        let dst = self.entities_mut(ctx);
        let mut edges: Vec<EdgeId> = Vec::new();
        for it in &items {
            match *it {
                EntityRef::Face(f) => dst.remove_face(f),
                EntityRef::Instance(i) => {
                    dst.instances.remove(i);
                }
                EntityRef::Guide(g) => {
                    dst.guides.remove(g);
                }
                EntityRef::Annot(a) => {
                    dst.annots.remove(a);
                }
                EntityRef::Section(x) => {
                    dst.sections.remove(x);
                    if dst.active_section == Some(x) {
                        dst.active_section = None;
                    }
                }
                EntityRef::Snap(_) | EntityRef::Space(_) => dst.erase(&[*it]),
                EntityRef::Edge(e) => edges.push(e),
            }
        }
        for e in edges {
            if dst.edges.get(e).is_some_and(|x| x.faces.is_empty()) {
                dst.remove_edge_raw(e);
            }
        }
        dst.remove_orphan_vertices();
        let group = dst.instances.insert(Instance {
            def,
            xform: DMat4::from_translation(origin),
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
        self.attach_after_group(ctx, group, &copied, &moved_annots);
        Some(group)
    }

    /// Replaces an instance by a copy of its geometry in the parent collection.
    pub fn explode(&mut self, ctx: Option<DefId>, inst: InstanceId) -> Vec<EntityRef> {
        if self.entities(ctx).instances.get(inst).is_none_or(|i| i.locked) {
            return Vec::new();
        }
        let Some(instance) = self.entities_mut(ctx).instances.remove(inst) else { return Vec::new() };
        let def = self.defs[instance.def].entities.clone();
        let m = instance.xform;
        let mirrored = m.determinant() < 0.0;
        // Textured faces keep their look once in the parent's coordinates.
        let texs: HashMap<crate::model::FaceId, Box<crate::meta::FaceTex>> = def
            .faces
            .iter()
            .filter_map(|(f, face)| Some((f, crate::image::exploded_tex(self, face, instance.material, &m)?)))
            .collect();
        let image = self.defs[instance.def].ext.image.is_some();
        if image {
            // The picture becomes an ordinary textured face, its material an ordinary one.
            for face in def.faces.values() {
                for mat in [face.front, face.back].into_iter().flatten() {
                    if let Some(x) = self.materials.get_mut(mat) {
                        x.ext.owner_image = false;
                    }
                }
            }
        }
        let dst = self.entities_mut(ctx);
        let mut out = Vec::new();
        let mut map: HashMap<VertexId, VertexId> = HashMap::new();
        for (v, vx) in def.verts.iter() {
            let p = m.transform_point3(vx.pos);
            let nv = dst.find_vertex(p).unwrap_or_else(|| dst.add_vertex_raw(p));
            map.insert(v, nv);
        }
        // What became of each entity, for the annotations attached to it (`attach.rs`).
        let mut copied = crate::attach::Copied { verts: map.clone(), ..Default::default() };
        let mut fresh = Vec::new();
        for (ek, edge) in def.edges.iter() {
            if let Some(ne) = dst.add_edge_raw(map[&edge.v[0]], map[&edge.v[1]]) {
                copied.edges.insert(ek, ne);
                if dst.edges[ne].v[0] != map[&edge.v[0]] {
                    copied.flipped.insert(ne);
                }
                let n = &mut dst.edges[ne];
                (n.soft, n.smooth, n.hidden, n.curve) = (edge.soft, edge.smooth, edge.hidden, edge.curve);
                if n.layer == 0 {
                    n.layer = edge.layer;
                }
                out.push(EntityRef::Edge(ne));
            }
        }
        for (fid, face) in def.faces.iter() {
            let mut outer: Vec<VertexId> = face.outer.iter().map(|v| map[v]).collect();
            let mut holes: Vec<Vec<VertexId>> = face.holes.iter().map(|h| h.iter().map(|v| map[v]).collect()).collect();
            if mirrored {
                outer.reverse();
                for h in holes.iter_mut() {
                    h.reverse();
                }
            }
            if let Some(nf) = dst.add_face_raw(outer, holes) {
                copied.faces.insert(fid, nf);
                let n = &mut dst.faces[nf];
                n.front = face.front.or(instance.material);
                n.back = face.back;
                n.layer = face.layer;
                n.hidden = face.hidden;
                n.tex = texs.get(&fid).cloned();
                out.push(EntityRef::Face(nf));
            }
        }
        for (ck, child) in def.instances.iter() {
            let mut c = child.clone();
            c.xform = m * c.xform;
            c.material = c.material.or(instance.material);
            let id = dst.instances.insert(c);
            copied.insts.insert(ck, id);
            out.push(EntityRef::Instance(id));
        }
        for (gk, g) in def.guides.iter() {
            let id = dst.guides.insert(g.transformed(&m));
            copied.guides.insert(gk, id);
            out.push(EntityRef::Guide(id));
        }
        for (_, a) in def.annots.iter() {
            let id = dst.annots.insert(a.transformed(&m));
            fresh.push(id);
            out.push(EntityRef::Annot(id));
        }
        for (x, sp) in def.sections.iter() {
            let id = dst.sections.insert(sp.transformed(&m));
            copied.sections.insert(x, id);
            if def.active_section == Some(x) && dst.active_section.is_none() {
                dst.active_section = Some(id);
            }
            out.push(EntityRef::Section(id));
        }
        // Snaps and spaces too (with ids of their own: the definition may keep its own).
        let dst = self.entities_mut(ctx);
        for s in &def.snaps {
            let mut s = s.transformed(&m);
            s.id = dst.new_mark_id();
            dst.snaps.push(s);
            out.push(EntityRef::Snap(s.id));
        }
        for s in &def.spaces {
            let mut s = s.transformed(&m);
            s.id = dst.new_mark_id();
            out.push(EntityRef::Space(s.id));
            dst.spaces.push(s);
        }
        // Polylines (not entities one can select) go with the rest.
        dst.polylines.extend(def.polylines.iter().map(|p| p.transformed(&m)));
        // A point on a face is measured in the face's frame, which the copy may turn (a
        // mirror reverses its loops): the same point, named on the copy.
        let dst = self.entities(ctx);
        let frames: HashMap<crate::model::FaceId, (DMat4, glam::DVec2, DMat4, glam::DVec2)> = copied
            .faces
            .iter()
            .filter_map(|(f, nf)| {
                let (a, ea) = crate::attach::face_frame(&def, *f)?;
                let (b, eb) = crate::attach::face_frame(dst, *nf)?;
                Some((*f, (m * a, ea, b.inverse(), eb)))
            })
            .collect();
        let scale = |g: &crate::model::GuideId| {
            def.guides.get(*g).and_then(|g| g.unit_dir()).map_or(1.0, |d| m.transform_vector3(d).length())
        };
        let leaf = |to: &crate::attach::AttachTo| -> Option<crate::attach::AttachTo> {
            use crate::attach::AttachTo;
            match *to {
                AttachTo::OnFace(f, u, v) => {
                    let (a, ea, b, eb) = frames.get(&f)?;
                    let l = b.transform_point3(a.transform_point3(DVec3::new(u * ea.x, v * ea.y, 0.0)));
                    let div = |x: f64, e: f64| if e > 0.0 { x / e } else { 0.0 };
                    Some(AttachTo::OnFace(*copied.faces.get(&f)?, div(l.x, eb.x), div(l.y, eb.y)))
                }
                AttachTo::OnGuide(g, d) => Some(AttachTo::OnGuide(*copied.guides.get(&g)?, d * scale(&g))),
                _ => copied.leaf(to),
            }
        };
        self.attach_after_explode(ctx, inst, &copied, &leaf, &fresh);
        if (self.defs[instance.def].is_group || image) && self.instance_count(instance.def) == 0 {
            self.defs.remove(instance.def);
        }
        out
    }

    /// Gives an instance its own copy of the definition.
    pub fn make_unique(&mut self, ctx: Option<DefId>, inst: InstanceId) -> DefId {
        let def = self.entities(ctx).instances[inst].def;
        if self.instance_count(def) <= 1 {
            return def;
        }
        let mut copy = self.defs[def].clone();
        if !copy.is_group {
            copy.name = self.unique_def_name(copy.name.split('#').next().unwrap_or("Composant"));
        }
        let attached = copy.entities.annots.values().any(|a| a.is_attached());
        let nd = self.defs.insert(copy);
        if attached {
            self.attached.note(Some(nd));
        }
        self.entities_mut(ctx).instances[inst].def = nd;
        nd
    }

    /// Definitions reachable from the root, for listing in the Outliner.
    pub fn used_defs(&self) -> HashSet<DefId> {
        let mut out = HashSet::new();
        let mut stack: Vec<&Entities> = vec![&self.root];
        while let Some(e) = stack.pop() {
            for i in e.instances.values() {
                if out.insert(i.def)
                    && let Some(d) = self.defs.get(i.def)
                {
                    stack.push(&d.entities);
                }
            }
        }
        out
    }

    /// Copies entities coming from another model into this one, remapping definitions,
    /// materials and tags (matched by name).
    pub fn import_entities(&mut self, src: &Model, ents: &Entities, map: &mut ImportMap) -> Entities {
        let mut out = ents.clone();
        for (_, f) in out.faces.iter_mut() {
            f.front = f.front.map(|m| self.import_material(src, m, map));
            f.back = f.back.map(|m| self.import_material(src, m, map));
            f.layer = self.import_layer(src, f.layer);
        }
        for (_, e) in out.edges.iter_mut() {
            e.layer = self.import_layer(src, e.layer);
        }
        for (_, g) in out.guides.iter_mut() {
            g.layer = self.import_layer(src, g.layer);
        }
        for (_, a) in out.annots.iter_mut() {
            a.layer = self.import_layer(src, a.layer);
        }
        for (_, x) in out.sections.iter_mut() {
            x.layer = self.import_layer(src, x.layer);
        }
        for x in out.snaps.iter_mut() {
            x.layer = self.import_layer(src, x.layer);
        }
        for x in out.spaces.iter_mut() {
            x.layer = self.import_layer(src, x.layer);
        }
        for x in out.polylines.iter_mut() {
            x.layer = self.import_layer(src, x.layer);
        }
        let ids: Vec<InstanceId> = out.instances.keys().collect();
        for i in ids {
            let (def, mat, layer) = {
                let inst = &out.instances[i];
                (inst.def, inst.material, inst.layer)
            };
            let nd = self.import_def(src, def, map);
            let nm = mat.map(|m| self.import_material(src, m, map));
            let nl = self.import_layer(src, layer);
            let inst = &mut out.instances[i];
            inst.def = nd;
            inst.material = nm;
            inst.layer = nl;
        }
        out.rebuild_topology();
        // Attached annotations keep their ends (the keys are copied as they were); where the
        // copy goes, the index is worked out again.
        if out.annots.values().any(|a| a.is_attached()) {
            self.attached.forget();
        }
        out
    }

    pub fn import_def(&mut self, src: &Model, def: DefId, map: &mut ImportMap) -> DefId {
        if let Some(&d) = map.defs.get(&def) {
            return d;
        }
        let s = &src.defs[def];
        let ents = self.import_entities(src, &s.entities, map);
        let name = if s.is_group || !self.defs.values().any(|d| d.name == s.name) {
            s.name.clone()
        } else {
            self.unique_def_name(s.name.split('#').next().unwrap_or("Composant"))
        };
        let d = self.defs.insert(Definition {
            name,
            is_group: s.is_group,
            description: s.description.clone(),
            attributes: s.attributes.clone(),
            ext: s.ext.clone(),
            entities: ents,
        });
        map.defs.insert(def, d);
        d
    }

    /// SketchUp's rule (`CSketchUpModel::MergeLayersMaterialsAndDefinitions`): a material
    /// identical to one of this model (same name without case, same colour, opacity, texture,
    /// colorization, PBR…) is reused; any other is added, renamed when its name is taken
    /// ([`Model::unique_material_name`]): an imported "Brique" never takes the model's own,
    /// different "Brique".
    fn import_material(&mut self, src: &Model, m: MaterialId, map: &mut ImportMap) -> MaterialId {
        if let Some(&x) = map.materials.get(&m) {
            return x;
        }
        let sm = &src.materials[m];
        let id = match self.materials.iter().find(|(_, x)| same_material(x, sm)) {
            Some((id, _)) => id,
            None => {
                let taken: Vec<String> = self.materials.values().map(|x| x.name.clone()).collect();
                let mut copy = sm.clone();
                copy.name = Model::unique_material_name(&sm.name, &taken);
                self.materials.insert(copy)
            }
        };
        map.materials.insert(m, id);
        id
    }

    fn import_layer(&mut self, src: &Model, l: LayerIdx) -> LayerIdx {
        if l == 0 {
            return 0;
        }
        let Some(sl) = src.layers.get(l) else { return 0 };
        if let Some(i) = self.layers.iter().position(|x| x.name == sl.name) {
            return i;
        }
        self.layers.push(sl.clone());
        self.layers.len() - 1
    }

    /// Wraps a whole model into one component definition of this model.
    pub fn import_as_component(&mut self, src: &Model, name: &str) -> DefId {
        let mut map = ImportMap::default();
        let ents = self.import_entities(src, &src.root, &mut map);
        let name =
            if self.defs.values().any(|d| d.name == name) { self.unique_def_name(name) } else { name.to_string() };
        // The component takes the model's description and behaviour (Model Info › File), and
        // its attribute dictionaries (a live or dynamic component saved as a file keeps its
        // own there: TestUp TC_Sketchup_Entity, LiveComponent.skp).
        self.defs.insert(Definition {
            name,
            is_group: false,
            description: src.meta.description.clone(),
            entities: ents,
            attributes: src.attr_store.get("model").cloned().unwrap_or_default(),
            ext: crate::model::DefExt { behavior: src.behavior(), ..Default::default() },
        })
    }
}

#[derive(Default)]
pub struct ImportMap {
    pub defs: HashMap<DefId, DefId>,
    pub materials: HashMap<MaterialId, MaterialId>,
}

/// Two materials SketchUp takes for one when models merge: same name
/// (without case) and same properties.
fn same_material(a: &Material, b: &Material) -> bool {
    a.name.to_uppercase() == b.name.to_uppercase() && a.color == b.color && a.opacity == b.opacity && a.ext == b.ext
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model whose root face is painted with `mat`.
    fn painted(mat: Material) -> Model {
        let mut m = Model::new();
        let v: Vec<_> = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]
            .iter()
            .map(|(x, y)| m.root.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
            .collect();
        let f = m.root.add_face_raw(v, vec![]).unwrap();
        let id = m.materials.insert(mat);
        m.root.faces[f].front = Some(id);
        m
    }

    fn mat(name: &str, color: [u8; 3]) -> Material {
        Material { name: name.into(), color, opacity: 1.0, ext: Default::default() }
    }

    fn imported_material(m: &Model, def: DefId) -> &Material {
        let f = m.defs[def].entities.faces.values().next().unwrap();
        &m.materials[f.front.unwrap()]
    }

    #[test]
    fn imported_materials_keep_their_own_look() {
        let mut m = Model::new();
        let mut brick = mat("Brique", [180, 90, 40]);
        brick.ext.colorize_deltas = [10.0, 0.0, 0.0];
        m.materials.insert(brick);
        m.materials.insert(mat("Verre", [200, 220, 255]));
        // A different "brique" (case aside) comes in: added as "brique1", the model's stays.
        let d = m.import_as_component(&painted(mat("brique", [150, 150, 150])), "A");
        let got = imported_material(&m, d);
        assert_eq!((got.name.as_str(), got.color), ("brique1", [150, 150, 150]));
        assert_eq!(m.materials.len(), 3);
        // An identical one is reused.
        let d = m.import_as_component(&painted(mat("Verre", [200, 220, 255])), "B");
        assert_eq!(imported_material(&m, d).name, "Verre");
        assert_eq!(m.materials.len(), 3);
        // Same name, other colour, while "Brique1" is taken: the next free index.
        let d = m.import_as_component(&painted(mat("Brique1", [1, 2, 3])), "C");
        assert_eq!(imported_material(&m, d).name, "Brique2");
        // The name is part of the identity: same look as "Brique2" but named "Brique".
        let d = m.import_as_component(&painted(mat("Brique", [1, 2, 3])), "D");
        assert_eq!(imported_material(&m, d).name, "Brique3");
        assert_eq!(m.materials.len(), 5);
    }

    /// A model loaded as a component gives it its attribute dictionaries (a live component
    /// saved as a file keeps `SU_LiveComponent` on the model).
    #[test]
    fn a_component_takes_the_models_dictionaries() {
        let mut src = painted(mat("Brique", [180, 90, 40]));
        let mut dicts = crate::model::Attributes::default();
        dicts
            .entry("SU_LiveComponent".into())
            .or_default()
            .insert("Parameters".into(), crate::model::AttrValue::Text("{}".into()));
        src.attr_store.insert("model".into(), dicts);
        let mut m = Model::new();
        let d = m.import_as_component(&src, "Vivant");
        assert_eq!(
            m.defs[d].attributes.get("SU_LiveComponent").and_then(|x| x.get("Parameters")),
            Some(&crate::model::AttrValue::Text("{}".into()))
        );
    }
}
