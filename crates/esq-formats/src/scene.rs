//! What the exporters read from a model: the faces, edges and instances of each entities
//! collection (with SketchUp's rules for hidden geometry and hidden tags), texture
//! coordinates, and the instance tree with world transforms.

use std::collections::HashMap;

use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::meta::{FaceTex, side_uv};
use esq_core::model::{DefId, Entities, EntityRef, FaceId, InstanceId, LayerIdx, MaterialId, Model};
use esq_core::tess::triangulate_2d;

/// A face as exporters see it, in its collection's coordinates.
#[derive(Clone, Debug)]
pub struct XFace {
    pub outer: Vec<DVec3>,
    pub holes: Vec<Vec<DVec3>>,
    pub normal: DVec3,
    /// Painted materials (`None` = default material).
    pub front: Option<MaterialId>,
    pub back: Option<MaterialId>,
    pub tex: Option<FaceTex>,
    pub layer: LayerIdx,
    /// Triangles, as indices into [`XFace::points`], counter-clockwise around `normal`.
    pub tris: Vec<[usize; 3]>,
    /// Faces joined by smooth edges share this number (0 = no smooth edge).
    pub smooth: u32,
}

impl XFace {
    /// `outer` then the holes.
    pub fn points(&self) -> Vec<DVec3> {
        self.outer.iter().chain(self.holes.iter().flatten()).copied().collect()
    }

    /// Texture coordinates of [`XFace::points`] for the material `mat` on one side, when that
    /// material has a texture (SketchUp convention: v goes up the image).
    pub fn uvs(&self, model: &Model, mat: Option<MaterialId>, front: bool) -> Option<Vec<DVec2>> {
        let tex = model.materials.get(mat?)?.ext.texture.as_ref()?;
        // SketchUp's UV helper: the placement, or the default one in the frame of the normal.
        let map = side_uv(self.tex.as_ref(), front, self.normal, Some(tex.size));
        Some(self.points().iter().map(|p| map.at(*p)).collect())
    }

    /// Whether the face can be written as one polygon (no holes).
    pub fn is_simple(&self) -> bool {
        self.holes.is_empty()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct XEdge {
    pub a: DVec3,
    pub b: DVec3,
    pub layer: LayerIdx,
    /// Bounds no face.
    pub standalone: bool,
    pub soft: bool,
    pub smooth: bool,
}

#[derive(Clone, Debug)]
pub struct XInst {
    pub id: InstanceId,
    pub def: DefId,
    pub xform: DMat4,
    pub name: String,
    pub material: Option<MaterialId>,
    pub layer: LayerIdx,
}

/// The exported content of one entities collection.
#[derive(Clone, Debug, Default)]
pub struct Collection {
    pub faces: Vec<XFace>,
    /// Visible edges (not soft, smooth or hidden).
    pub edges: Vec<XEdge>,
    pub instances: Vec<XInst>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Opts {
    /// "Exporter la géométrie cachée": hidden entities and the ones on hidden tags too.
    pub hidden: bool,
}

fn layer_visible(model: &Model, l: LayerIdx) -> bool {
    model.layers.get(l).is_none_or(|x| x.visible)
}

/// Faces, edges and instances of `ents` that the export includes.
pub fn collection(model: &Model, ents: &Entities, opts: Opts) -> Collection {
    let shown = |hidden: bool, layer: LayerIdx| opts.hidden || (!hidden && layer_visible(model, layer));
    let mut c = Collection::default();
    // Surfaces: faces joined by smooth edges.
    let ids: Vec<FaceId> = ents.faces.keys().collect();
    let index: HashMap<FaceId, usize> = ids.iter().enumerate().map(|(i, f)| (*f, i)).collect();
    let mut parent: Vec<usize> = (0..ids.len()).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut smooth_face = vec![false; ids.len()];
    for edge in ents.edges.values() {
        if edge.smooth && edge.faces.len() == 2 {
            let (Some(&a), Some(&b)) = (index.get(&edge.faces[0]), index.get(&edge.faces[1])) else { continue };
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            parent[ra] = rb;
            smooth_face[a] = true;
            smooth_face[b] = true;
        }
    }
    let mut surface: HashMap<usize, u32> = HashMap::new();
    for (i, (_, face)) in ents.faces.iter().enumerate() {
        if !shown(face.hidden, face.layer) {
            continue;
        }
        let smooth = if smooth_face[i] {
            let r = root(&mut parent, i);
            let next = surface.len() as u32 + 1;
            *surface.entry(r).or_insert(next)
        } else {
            0
        };
        let outer: Vec<DVec3> = face.outer.iter().map(|v| ents.verts[*v].pos).collect();
        let holes: Vec<Vec<DVec3>> =
            face.holes.iter().map(|h| h.iter().map(|v| ents.verts[*v].pos).collect()).collect();
        let tris = triangles(&outer, &holes, face.normal);
        c.faces.push(XFace {
            smooth,
            outer,
            holes,
            normal: face.normal,
            front: face.front,
            back: face.back,
            tex: face.tex.as_deref().cloned(),
            layer: face.layer,
            tris,
        });
    }
    for edge in ents.edges.values() {
        if !shown(edge.hidden, edge.layer) {
            continue;
        }
        let standalone = edge.faces.iter().all(|f| ents.faces.get(*f).is_none_or(|x| !shown(x.hidden, x.layer)));
        if (edge.soft || edge.smooth) && !standalone {
            continue;
        }
        let (a, b) = (ents.verts[edge.v[0]].pos, ents.verts[edge.v[1]].pos);
        c.edges.push(XEdge { a, b, layer: edge.layer, standalone, soft: edge.soft, smooth: edge.smooth });
    }
    for (id, inst) in ents.instances.iter() {
        if !shown(inst.hidden, inst.layer) || !model.defs.contains_key(inst.def) {
            continue;
        }
        c.instances.push(XInst {
            id,
            def: inst.def,
            xform: inst.xform,
            name: inst.name.clone(),
            material: inst.material,
            layer: inst.layer,
        });
    }
    c
}

/// Triangles of a polygon with holes, counter-clockwise around `n`.
pub fn triangles(outer: &[DVec3], holes: &[Vec<DVec3>], n: DVec3) -> Vec<[usize; 3]> {
    let (u, v) = esq_core::math::basis_for(n);
    let to2 = |p: &DVec3| DVec2::new(p.dot(u), p.dot(v));
    let o2: Vec<DVec2> = outer.iter().map(to2).collect();
    let h2: Vec<Vec<DVec2>> = holes.iter().map(|h| h.iter().map(to2).collect()).collect();
    let all: Vec<DVec3> = outer.iter().chain(holes.iter().flatten()).copied().collect();
    triangulate_2d(&o2, &h2)
        .into_iter()
        .map(|[a, b, c]| if (all[b] - all[a]).cross(all[c] - all[a]).dot(n) < 0.0 { [a, c, b] } else { [a, b, c] })
        .collect()
}

/// The collections of a model, computed once each.
pub struct Cache<'a> {
    pub model: &'a Model,
    pub opts: Opts,
    map: HashMap<Option<DefId>, Collection>,
}

impl<'a> Cache<'a> {
    pub fn new(model: &'a Model, opts: Opts) -> Self {
        Cache { model, opts, map: HashMap::new() }
    }

    pub fn get(&mut self, def: Option<DefId>) -> &Collection {
        let model = self.model;
        let opts = self.opts;
        self.map.entry(def).or_insert_with(|| collection(model, model.entities(def), opts))
    }
}

/// One collection as placed in the world.
#[derive(Clone, Debug)]
pub struct Placed {
    pub def: Option<DefId>,
    pub xform: DMat4,
    /// Material of the nearest enclosing instance that has one.
    pub inherited: Option<MaterialId>,
    /// Instances from the root.
    pub path: Vec<InstanceId>,
    /// Tag of the outermost instance (or 0 for the root).
    pub top_layer: LayerIdx,
}

/// Every collection of the exported tree, depth first, the root first.
pub fn placements(cache: &mut Cache) -> Vec<Placed> {
    let mut out = Vec::new();
    let mut stack = vec![Placed { def: None, xform: DMat4::IDENTITY, inherited: None, path: Vec::new(), top_layer: 0 }];
    while let Some(p) = stack.pop() {
        if p.path.len() > 64 {
            continue;
        }
        let insts = cache.get(p.def).instances.clone();
        for i in insts.iter().rev() {
            let mut path = p.path.clone();
            path.push(i.id);
            stack.push(Placed {
                def: Some(i.def),
                xform: p.xform * i.xform,
                inherited: i.material.or(p.inherited),
                top_layer: if p.path.is_empty() { i.layer } else { p.top_layer },
                path,
            });
        }
        out.push(p);
    }
    out
}

/// A face in world coordinates, with its effective materials.
pub struct WorldFace<'a> {
    pub face: &'a XFace,
    pub xform: DMat4,
    pub front: Option<MaterialId>,
    pub back: Option<MaterialId>,
}

impl WorldFace<'_> {
    pub fn points(&self) -> Vec<DVec3> {
        self.face.points().iter().map(|p| self.xform.transform_point3(*p)).collect()
    }

    pub fn normal(&self) -> DVec3 {
        let m = esq_core::glam::DMat3::from_mat4(self.xform).inverse().transpose();
        (m * self.face.normal).normalize_or_zero()
    }

    /// Triangles in world winding (a mirroring transform reverses them).
    pub fn tris(&self) -> Vec<[usize; 3]> {
        if self.xform.determinant() < 0.0 {
            self.face.tris.iter().map(|t| [t[0], t[2], t[1]]).collect()
        } else {
            self.face.tris.clone()
        }
    }

    /// The outer loop and holes in world winding.
    pub fn loops(&self) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        let mut k = 0;
        for l in std::iter::once(&self.face.outer).chain(self.face.holes.iter()) {
            let mut idx: Vec<usize> = (k..k + l.len()).collect();
            if self.xform.determinant() < 0.0 {
                idx.reverse();
            }
            k += l.len();
            out.push(idx);
        }
        out
    }
}

/// Runs `f` on every exported face in world coordinates.
pub fn for_each_world_face(cache: &mut Cache, mut f: impl FnMut(&Placed, &WorldFace)) {
    let placed = placements(cache);
    for p in &placed {
        let coll = cache.get(p.def).clone();
        for face in &coll.faces {
            let wf =
                WorldFace { face, xform: p.xform, front: face.front.or(p.inherited), back: face.back.or(p.inherited) };
            f(p, &wf);
        }
    }
}

/// Runs `f` on every exported edge in world coordinates.
pub fn for_each_world_edge(cache: &mut Cache, mut f: impl FnMut(&Placed, DVec3, DVec3, &XEdge)) {
    let placed = placements(cache);
    for p in &placed {
        let coll = cache.get(p.def).clone();
        for e in &coll.edges {
            f(p, p.xform.transform_point3(e.a), p.xform.transform_point3(e.b), e);
        }
    }
}

/// "Exporter uniquement la sélection": a copy of the model whose root holds only the selected
/// entities of the active context `ctx` (instances from the root), placed where they are.
pub fn selection_model(model: &Model, ctx: &[InstanceId], sel: &[EntityRef]) -> Model {
    let def = esq_core::walk::path_def(model, ctx);
    let xform = esq_core::walk::path_xform(model, ctx);
    let mut e = model.entities(def).clone();
    let keep = |r: EntityRef| sel.contains(&r);
    let faces: Vec<_> = e.faces.keys().collect();
    let mut bounding = std::collections::HashSet::new();
    for f in faces {
        if keep(EntityRef::Face(f)) {
            bounding.extend(e.face_edges(f));
        } else {
            e.remove_face(f);
        }
    }
    let edges: Vec<_> = e.edges.keys().collect();
    for x in edges {
        if !keep(EntityRef::Edge(x)) && !bounding.contains(&x) {
            e.remove_edge_raw(x);
        }
    }
    e.instances.retain(|i, _| keep(EntityRef::Instance(i)));
    e.guides.retain(|i, _| keep(EntityRef::Guide(i)));
    e.annots.retain(|i, _| keep(EntityRef::Annot(i)));
    e.sections.retain(|i, _| keep(EntityRef::Section(i)));
    e.active_section = None;
    e.remove_orphan_vertices();
    let mut out = model.clone();
    if xform == DMat4::IDENTITY {
        out.root = e;
    } else {
        for v in e.verts.values_mut() {
            v.pos = xform.transform_point3(v.pos);
        }
        let n = esq_core::glam::DMat3::from_mat4(xform).inverse().transpose();
        let flip = xform.determinant() < 0.0;
        let ids: Vec<_> = e.faces.keys().collect();
        for f in ids {
            let face = &mut e.faces[f];
            face.normal = (n * face.normal).normalize_or_zero();
            if flip {
                face.outer.reverse();
                face.holes.iter_mut().for_each(|h| h.reverse());
            }
            // Positioned textures follow the face.
            if let Some(t) = face.tex.as_mut() {
                let inv = xform.inverse();
                for m in [&mut t.front, &mut t.back].into_iter().flatten() {
                    *m = compose_uv(m, &inv);
                }
            }
        }
        for i in e.instances.values_mut() {
            i.xform = xform * i.xform;
        }
        out.root = e;
    }
    out.scenes.clear();
    out
}

/// The map that gives at `p` what `m` gives at `inv * p`.
fn compose_uv(m: &esq_core::meta::UvMap, inv: &DMat4) -> esq_core::meta::UvMap {
    let row = |c: &[f64; 4]| {
        let a = inv.to_cols_array();
        // c · (inv * [x y z 1])
        let mut r = [0.0; 4];
        for (j, item) in r.iter_mut().enumerate() {
            *item = c[0] * a[j * 4] + c[1] * a[j * 4 + 1] + c[2] * a[j * 4 + 2] + if j == 3 { c[3] } else { 0.0 };
        }
        r
    };
    esq_core::meta::UvMap { u: row(&m.u), v: row(&m.v), q: row(&m.q) }
}

/// Faces of the export split by the two-sided rule: with `two_sided`, each face is also
/// written facing backwards with its back material.
pub fn sides(two_sided: bool) -> &'static [bool] {
    if two_sided { &[true, false] } else { &[true] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::model::Instance;

    pub fn box_model() -> Model {
        let mut m = Model::new();
        let e = &mut m.root;
        let p = |x: f64, y: f64| DVec3::new(x, y, 0.0);
        let v: Vec<_> = [p(0.0, 0.0), p(1000.0, 0.0), p(1000.0, 1000.0), p(0.0, 1000.0)]
            .into_iter()
            .map(|x| e.add_vertex_raw(x))
            .collect();
        e.add_face_raw(v.clone(), Vec::new()).unwrap();
        let a = e.add_vertex_raw(DVec3::new(0.0, 0.0, 500.0));
        let b = e.add_vertex_raw(DVec3::new(0.0, 0.0, 900.0));
        e.add_edge_raw(a, b);
        m
    }

    #[test]
    fn collections_and_selection() {
        let m = box_model();
        let c = collection(&m, &m.root, Opts::default());
        assert_eq!(c.faces.len(), 1);
        assert_eq!(c.faces[0].tris.len(), 2);
        assert_eq!(c.edges.len(), 5);
        assert_eq!(c.edges.iter().filter(|e| e.standalone).count(), 1);
        let face = m.root.faces.keys().next().unwrap();
        let s = selection_model(&m, &[], &[EntityRef::Face(face)]);
        assert_eq!(s.root.faces.len(), 1);
        assert_eq!(s.root.edges.len(), 4);
    }

    #[test]
    fn placements_walk_instances_with_world_transforms() {
        let mut m = box_model();
        let ents = m.root.clone();
        let d = m.defs.insert(esq_core::model::Definition {
            name: "B".into(),
            is_group: false,
            description: String::new(),
            entities: ents,
            attributes: Default::default(),
            ext: Default::default(),
        });
        let mat = m.add_material("Rouge", [255, 0, 0]);
        m.root.instances.insert(Instance {
            def: d,
            xform: DMat4::from_translation(DVec3::new(0.0, 0.0, 3000.0)),
            name: String::new(),
            material: Some(mat),
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
        let mut cache = Cache::new(&m, Opts::default());
        let mut zs = Vec::new();
        for_each_world_face(&mut cache, |_, f| zs.push((f.points()[0].z, f.front)));
        assert_eq!(zs.len(), 2);
        assert!(zs.contains(&(3000.0, Some(mat))));
        assert!(zs.contains(&(0.0, None)));
    }
}
