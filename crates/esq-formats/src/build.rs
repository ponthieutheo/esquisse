//! Turning the meshes of an imported file into Esquisse geometry, the way SketchUp's
//! importers do: vertices are welded, polygons become faces (non-planar ones are split into
//! triangles whose diagonals are softened), the node tree becomes groups and components,
//! and the options of the import dialogs apply ("Fusionner les faces coplanaires",
//! "Orienter les faces de façon cohérente", units, "Permuter les coordonnées YZ").

use std::collections::{HashMap, HashSet};

use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::math::{TOL, newell_normal, point_in_polygon_2d, signed_area_2d};
use esq_core::meta::{FaceTex, TextureData, UvMap, uv_from_pairs};
use esq_core::model::{
    DefId, Definition, EdgeId, Entities, FaceId, Instance, LayerIdx, Material, MaterialId, Model, VertexId,
};
use esq_core::tess::triangulate_2d;

/// A material of the file.
#[derive(Clone, Debug, Default)]
pub struct MatIn {
    pub name: String,
    pub color: [u8; 3],
    pub opacity: f32,
    pub texture: Option<TextureData>,
}

/// One polygon: indices into its mesh's positions. Holes may be given in either direction.
#[derive(Clone, Debug, Default)]
pub struct PolyIn {
    pub outer: Vec<u32>,
    pub holes: Vec<Vec<u32>>,
    /// Index into [`SceneIn::materials`].
    pub mat: Option<u32>,
    pub back: Option<u32>,
    /// Texture coordinates of `outer` then `holes` (SketchUp convention: v goes up the
    /// image), or empty.
    pub uv: Vec<DVec2>,
    /// Vertex normals of `outer`, or empty: where two faces share their normals along an
    /// edge, the edge is softened and smoothed.
    pub normals: Vec<DVec3>,
    /// Smoothing groups (3DS): faces sharing a bit are smooth across their common edges.
    pub smooth: u32,
    /// Visibility of each edge of `outer` (edge i joins i and i+1), or empty for all visible.
    pub edge_vis: Vec<bool>,
    /// Index into [`SceneIn::layers`].
    pub layer: Option<u32>,
}

/// Free edges (lines, polylines) of a mesh.
#[derive(Clone, Debug, Default)]
pub struct LineIn {
    pub pts: Vec<u32>,
    pub closed: bool,
    pub layer: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct MeshIn {
    pub pos: Vec<DVec3>,
    pub polys: Vec<PolyIn>,
    pub lines: Vec<LineIn>,
    /// Construction points (CAD points).
    pub guides: Vec<DVec3>,
    /// Texts pinned at a point (CAD texts), with their tag.
    pub texts: Vec<(DVec3, String, Option<u32>)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NodeKind {
    /// A group (its own definition).
    #[default]
    Group,
    /// Contents merged into the parent collection (transform applied).
    Flatten,
}

/// A node of the file's scene tree. With `def`, the node is an instance of a shared
/// definition (a SketchUp component); otherwise its meshes and children are its own.
#[derive(Clone, Debug)]
pub struct NodeIn {
    pub name: String,
    pub xform: DMat4,
    pub meshes: Vec<usize>,
    pub children: Vec<NodeIn>,
    pub def: Option<usize>,
    pub kind: NodeKind,
    /// Material painted on the group/component.
    pub material: Option<u32>,
    pub layer: Option<u32>,
    pub hidden: bool,
}

impl Default for NodeIn {
    fn default() -> Self {
        NodeIn {
            name: String::new(),
            xform: DMat4::IDENTITY,
            meshes: Vec::new(),
            children: Vec::new(),
            def: None,
            kind: NodeKind::Group,
            material: None,
            layer: None,
            hidden: false,
        }
    }
}

/// A component definition shared by several nodes.
#[derive(Clone, Debug, Default)]
pub struct DefIn {
    pub name: String,
    pub meshes: Vec<usize>,
    pub children: Vec<NodeIn>,
}

/// Everything read from a file, in the file's own units.
#[derive(Clone, Debug, Default)]
pub struct SceneIn {
    pub materials: Vec<MatIn>,
    pub layers: Vec<String>,
    pub meshes: Vec<MeshIn>,
    pub defs: Vec<DefIn>,
    pub root: NodeIn,
}

impl SceneIn {
    pub fn add_mesh(&mut self, m: MeshIn) -> usize {
        self.meshes.push(m);
        self.meshes.len() - 1
    }

    pub fn material_index(&mut self, name: &str, color: [u8; 3], opacity: f32) -> u32 {
        if let Some(i) = self.materials.iter().position(|m| m.name == name) {
            return i as u32;
        }
        self.materials.push(MatIn { name: name.to_string(), color, opacity, texture: None });
        self.materials.len() as u32 - 1
    }

    pub fn layer_index(&mut self, name: &str) -> u32 {
        if let Some(i) = self.layers.iter().position(|m| m == name) {
            return i as u32;
        }
        self.layers.push(name.to_string());
        self.layers.len() as u32 - 1
    }

    /// Converts the scene to millimetres (`s` mm per file unit): positions and translations.
    pub fn rescale(&mut self, s: f64) {
        fn node(n: &mut NodeIn, s: f64) {
            n.xform = scaled(&n.xform, s);
            n.children.iter_mut().for_each(|c| node(c, s));
        }
        for m in &mut self.meshes {
            m.pos.iter_mut().for_each(|p| *p *= s);
        }
        node(&mut self.root, s);
        for d in &mut self.defs {
            d.children.iter_mut().for_each(|c| node(c, s));
        }
    }

    /// Number of polygons (for the import reports).
    pub fn poly_count(&self) -> usize {
        self.meshes.iter().map(|m| m.polys.len()).sum()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BuildOpts {
    /// Millimetres per file unit.
    pub scale: f64,
    pub swap_yz: bool,
    pub merge_coplanar: bool,
    pub orient_faces: bool,
}

impl Default for BuildOpts {
    fn default() -> Self {
        BuildOpts { scale: 1.0, swap_yz: false, merge_coplanar: false, orient_faces: false }
    }
}

/// (x, y, z) of a Y-up file → Esquisse's Z-up (x, -z, y).
pub fn y_up_to_z_up() -> DMat4 {
    DMat4::from_cols_array(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0])
}

struct Builder<'a> {
    scene: &'a SceneIn,
    opts: BuildOpts,
    model: Model,
    mats: HashMap<u32, MaterialId>,
    layers: HashMap<u32, LayerIdx>,
    defs: HashMap<usize, DefId>,
    /// The definitions being built: one that contains itself (directly or not) is cut there.
    building: HashSet<usize>,
}

/// Builds a model whose root holds the file's content.
pub fn build(scene: &SceneIn, opts: BuildOpts) -> Model {
    build_mapped(scene, opts).0
}

/// [`build`], also giving the definition made for each of `scene.defs` (by index).
pub fn build_mapped(scene: &SceneIn, opts: BuildOpts) -> (Model, HashMap<usize, DefId>) {
    let mut b = Builder {
        scene,
        opts,
        model: Model::new(),
        mats: HashMap::new(),
        layers: HashMap::new(),
        defs: HashMap::new(),
        building: HashSet::new(),
    };
    let mut root = Entities::default();
    let top = if opts.swap_yz { y_up_to_z_up() } else { DMat4::IDENTITY };
    b.fill(&mut root, &scene.root.meshes, &scene.root.children, top * scaled(&scene.root.xform, opts.scale));
    b.finish(&mut root);
    b.model.root = root;
    (b.model, b.defs)
}

/// The box around everything in the model (world coordinates).
pub fn bounds(model: &Model) -> Option<(DVec3, DVec3)> {
    let mut b: Option<(DVec3, DVec3)> = None;
    esq_core::walk::visit_all(model, true, |pl| {
        for v in pl.ents.verts.values() {
            let p = pl.xform.transform_point3(v.pos);
            b = Some(match b {
                Some((lo, hi)) => (lo.min(p), hi.max(p)),
                None => (p, p),
            });
        }
    });
    b
}

/// Without "Préserver l'origine du dessin", the imported content is moved so that the
/// corner of its box (lowest x, y and z) is its origin: that is the point the cursor holds.
pub fn recenter(model: &mut Model) {
    let Some((lo, _)) = bounds(model) else { return };
    if lo.length() < 1e-9 {
        return;
    }
    let m = DMat4::from_translation(-lo);
    for v in model.root.verts.values_mut() {
        v.pos -= lo;
    }
    for i in model.root.instances.values_mut() {
        i.xform = m * i.xform;
    }
    for g in model.root.guides.values_mut() {
        g.pos -= lo;
    }
    for f in model.root.faces.values_mut() {
        if let Some(t) = f.tex.as_mut() {
            for map in [&mut t.front, &mut t.back].into_iter().flatten() {
                // The texture stays on the face: uv(p) = map(p + lo).
                map.u[3] += map.u[0] * lo.x + map.u[1] * lo.y + map.u[2] * lo.z;
                map.v[3] += map.v[0] * lo.x + map.v[1] * lo.y + map.v[2] * lo.z;
                map.q[3] += map.q[0] * lo.x + map.q[1] * lo.y + map.q[2] * lo.z;
            }
        }
    }
}

/// A transform of the file with its translation converted to millimetres.
fn scaled(m: &DMat4, s: f64) -> DMat4 {
    let mut c = *m;
    c.w_axis.x *= s;
    c.w_axis.y *= s;
    c.w_axis.z *= s;
    c
}

impl Builder<'_> {
    fn material(&mut self, i: u32) -> Option<MaterialId> {
        if let Some(&m) = self.mats.get(&i) {
            return Some(m);
        }
        let src = self.scene.materials.get(i as usize)?;
        let taken: Vec<String> = self.model.materials.values().map(|m| m.name.clone()).collect();
        let base = if src.name.trim().is_empty() { "Matière".to_string() } else { src.name.clone() };
        let name = if taken.contains(&base) { Model::unique_name(&base, &taken) } else { base };
        let mut ext = esq_core::meta::MaterialExt { texture: src.texture.clone(), ..Default::default() };
        ext.sanitize_texture_names();
        let id =
            self.model.materials.insert(Material { name, color: src.color, opacity: src.opacity.clamp(0.0, 1.0), ext });
        self.mats.insert(i, id);
        Some(id)
    }

    fn layer(&mut self, i: Option<u32>) -> LayerIdx {
        let Some(i) = i else { return 0 };
        if let Some(&l) = self.layers.get(&i) {
            return l;
        }
        let name = self.scene.layers.get(i as usize).cloned().unwrap_or_default();
        let l = match self.model.layers.iter().position(|x| x.name == name) {
            Some(l) => l,
            None if name.is_empty() || name == "0" => 0,
            None => self.model.add_layer(&name),
        };
        self.layers.insert(i, l);
        l
    }

    /// Adds meshes (transformed by `m`, in file units → mm) and child nodes to `ents`.
    fn fill(&mut self, ents: &mut Entities, meshes: &[usize], children: &[NodeIn], m: DMat4) {
        let s = self.opts.scale;
        let to_mm = m * DMat4::from_scale(DVec3::splat(s));
        for &i in meshes {
            if let Some(mesh) = self.scene.meshes.get(i) {
                self.add_mesh(ents, mesh, &to_mm);
            }
        }
        for c in children {
            let local = scaled(&c.xform, s);
            if c.kind == NodeKind::Flatten && c.def.is_none() {
                self.fill(ents, &c.meshes, &c.children, m * local);
                continue;
            }
            let def = match c.def {
                Some(d) => match self.def(d) {
                    Some(x) => x,
                    // Not in the file, or the definition being built itself.
                    None => continue,
                },
                None => {
                    let mut e = Entities::default();
                    self.fill(&mut e, &c.meshes, &c.children, DMat4::IDENTITY);
                    self.finish(&mut e);
                    if e.is_empty() {
                        continue;
                    }
                    let name = if c.name.is_empty() { self.model.unique_def_name("Groupe") } else { c.name.clone() };
                    self.model.defs.insert(Definition {
                        name,
                        is_group: true,
                        description: String::new(),
                        entities: e,
                        attributes: Default::default(),
                        ext: Default::default(),
                    })
                }
            };
            let material = c.material.and_then(|x| self.material(x));
            let layer = self.layer(c.layer);
            ents.instances.insert(Instance {
                def,
                xform: m * local,
                name: if c.def.is_some() { c.name.clone() } else { String::new() },
                material,
                layer,
                hidden: c.hidden,
                locked: false,
                attributes: Default::default(),
                ext: Default::default(),
            });
        }
    }

    /// The definition made for `scene.defs[d]`. `None` for an index the file does not have,
    /// and for a definition that contains itself (a COLLADA `<instance_node>` of its own
    /// node, an IFC mapped item of its own map…): SketchUp has no recursive components, and
    /// building one recursed until the stack overflowed (the application died).
    fn def(&mut self, d: usize) -> Option<DefId> {
        if let Some(&x) = self.defs.get(&d) {
            return Some(x);
        }
        let src = self.scene.defs.get(d)?;
        if !self.building.insert(d) {
            return None;
        }
        let mut e = Entities::default();
        self.fill(&mut e, &src.meshes, &src.children, DMat4::IDENTITY);
        self.building.remove(&d);
        self.finish(&mut e);
        let name = if src.name.trim().is_empty() || self.model.defs.values().any(|x| x.name == src.name) {
            self.model.unique_def_name(if src.name.trim().is_empty() { "Composant" } else { &src.name })
        } else {
            src.name.clone()
        };
        let id = self.model.defs.insert(Definition {
            name,
            is_group: false,
            description: String::new(),
            entities: e,
            attributes: Default::default(),
            ext: Default::default(),
        });
        self.defs.insert(d, id);
        Some(id)
    }

    fn finish(&mut self, e: &mut Entities) {
        if self.opts.orient_faces {
            orient_faces(e);
        }
        if self.opts.merge_coplanar {
            merge_coplanar(e);
        }
    }

    fn add_mesh(&mut self, ents: &mut Entities, mesh: &MeshIn, m: &DMat4) {
        let mirrored = m.determinant() < 0.0;
        let mut weld = Weld::new(ents);
        let ids: Vec<VertexId> = mesh.pos.iter().map(|&p| weld.vertex(ents, m.transform_point3(p))).collect();
        let mut seen: HashMap<Vec<VertexId>, FaceId> = ents
            .faces
            .iter()
            .map(|(id, f)| {
                let mut k = f.outer.clone();
                k.sort();
                (k, id)
            })
            .collect();
        // Smoothing information per new face: (group bits, normal at each outer vertex).
        let mut smooth_info: Vec<Smooth> = Vec::new();
        let nm = DMat4::from_mat3(esq_core::glam::DMat3::from_mat4(*m).inverse().transpose());
        for poly in &mesh.polys {
            let get = |i: &u32| ids.get(*i as usize).copied();
            let Some(mut outer) = poly.outer.iter().map(get).collect::<Option<Vec<_>>>() else { continue };
            let mut holes: Vec<Vec<VertexId>> = Vec::new();
            let mut ok = true;
            for h in &poly.holes {
                match h.iter().map(get).collect::<Option<Vec<_>>>() {
                    Some(h) => holes.push(h),
                    None => ok = false,
                }
            }
            if !ok {
                continue;
            }
            let mut uv: Vec<(VertexId, DVec2)> = Vec::new();
            if !poly.uv.is_empty() {
                let all: Vec<VertexId> = outer.iter().chain(holes.iter().flatten()).copied().collect();
                uv = all.iter().copied().zip(poly.uv.iter().copied()).collect();
            }
            let normals: Vec<(VertexId, DVec3)> = outer
                .iter()
                .copied()
                .zip(poly.normals.iter().map(|n| nm.transform_vector3(*n).normalize_or_zero()))
                .collect();
            let mut vis: Vec<(VertexId, VertexId, bool)> = Vec::new();
            if poly.edge_vis.len() == outer.len() {
                for i in 0..outer.len() {
                    vis.push((outer[i], outer[(i + 1) % outer.len()], poly.edge_vis[i]));
                }
            }
            if mirrored {
                outer.reverse();
                holes.iter_mut().for_each(|h| h.reverse());
            }
            clean_loop(&mut outer);
            holes.iter_mut().for_each(clean_loop);
            holes.retain(|h| h.len() >= 3);
            if outer.len() < 3 {
                continue;
            }
            let front = poly.mat.and_then(|x| self.material(x));
            let back = poly.back.and_then(|x| self.material(x));
            let layer = self.layer(poly.layer);
            let (made, backs) = add_polygon(ents, outer, holes, &mut seen);
            // The same face given again, turned the other way: its back.
            for f in &backs {
                let face = &mut ents.faces[*f];
                if face.back.is_none() {
                    face.back = front;
                    if uv.len() >= 3 {
                        let tile = front
                            .and_then(|x| self.model.materials.get(x))
                            .and_then(|x| x.ext.texture.as_ref())
                            .map_or([25.4, 25.4], |t| t.size);
                        set_face_uv(ents, *f, &uv, tile, false);
                    }
                }
            }
            for f in &made {
                let face = &mut ents.faces[*f];
                face.front = front;
                face.back = back;
                face.layer = layer;
            }
            if uv.len() >= 3 {
                let tile = front
                    .and_then(|x| self.model.materials.get(x))
                    .and_then(|x| x.ext.texture.as_ref())
                    .map_or([25.4, 25.4], |t| t.size);
                for f in &made {
                    set_face_uv(ents, *f, &uv, tile, true);
                }
            }
            for &(a, b, visible) in &vis {
                if !visible && let Some(e) = ents.find_edge(a, b) {
                    ents.edges[e].hidden = true;
                }
            }
            if poly.smooth != 0 || !normals.is_empty() {
                for f in &made {
                    smooth_info.push((*f, poly.smooth, normals.clone()));
                }
            }
            if layer != 0 {
                for f in &made {
                    for e in ents.face_edges(*f) {
                        if ents.edges[e].layer == 0 {
                            ents.edges[e].layer = layer;
                        }
                    }
                }
            }
        }
        apply_smoothing(ents, &smooth_info);
        for line in &mesh.lines {
            let vs: Vec<VertexId> = line.pts.iter().filter_map(|i| ids.get(*i as usize).copied()).collect();
            let layer = self.layer(line.layer);
            let n = vs.len();
            let segs = if line.closed { n } else { n.saturating_sub(1) };
            let curve = (segs > 1).then(|| ents.new_curve_id());
            for i in 0..segs {
                let (a, b) = (vs[i], vs[(i + 1) % n]);
                if let Some(e) = ents.add_edge_raw(a, b) {
                    let edge = &mut ents.edges[e];
                    if edge.faces.is_empty() {
                        edge.layer = layer;
                        edge.curve = curve;
                    }
                }
            }
        }
        for p in &mesh.guides {
            ents.guides.insert(esq_core::model::Guide::point(m.transform_point3(*p)));
        }
        for (p, text, layer) in &mesh.texts {
            let layer = self.layer(*layer);
            ents.annots.insert(esq_core::model::Annotation {
                kind: esq_core::model::AnnotKind::Text { point: m.transform_point3(*p), leader: DVec3::ZERO },
                text: text.clone(),
                layer,
                hidden: false,
                attach: Default::default(),
            });
        }
        ents.remove_orphan_vertices();
    }
}

/// Welds vertices closer than a hundredth of SketchUp's tolerance.
struct Weld {
    grid: HashMap<(i64, i64, i64), VertexId>,
}

impl Weld {
    const CELL: f64 = TOL / 8.0;

    fn new(ents: &Entities) -> Weld {
        let mut grid = HashMap::new();
        for (id, v) in ents.verts.iter() {
            grid.insert(Self::key(v.pos), id);
        }
        Weld { grid }
    }

    fn key(p: DVec3) -> (i64, i64, i64) {
        ((p.x / Self::CELL).round() as i64, (p.y / Self::CELL).round() as i64, (p.z / Self::CELL).round() as i64)
    }

    fn vertex(&mut self, ents: &mut Entities, p: DVec3) -> VertexId {
        let k = Self::key(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    // Saturating: a coordinate of 1e300 (or "inf" in an STL) gives i64::MAX here.
                    let cell = (k.0.saturating_add(dx), k.1.saturating_add(dy), k.2.saturating_add(dz));
                    if let Some(&v) = self.grid.get(&cell)
                        && ents.verts.get(v).is_some_and(|x| x.pos.distance(p) < Self::CELL)
                    {
                        return v;
                    }
                }
            }
        }
        let v = ents.add_vertex_raw(p);
        self.grid.insert(k, v);
        v
    }
}

/// Drops repeated consecutive vertices (and a closing repeat).
fn clean_loop(l: &mut Vec<VertexId>) {
    l.dedup();
    while l.len() > 1 && l.first() == l.last() {
        l.pop();
    }
}

/// Adds a polygon as one face, or as triangles when it is not planar (their inner edges are
/// softened). Skips faces that already exist. Returns the faces made.
fn add_polygon(
    ents: &mut Entities,
    outer: Vec<VertexId>,
    holes: Vec<Vec<VertexId>>,
    seen: &mut HashMap<Vec<VertexId>, FaceId>,
) -> (Vec<FaceId>, Vec<FaceId>) {
    let pos = |v: &VertexId| ents.verts[*v].pos;
    let opts: Vec<DVec3> = outer.iter().map(pos).collect();
    let n = newell_normal(&opts);
    if n == DVec3::ZERO {
        return (Vec::new(), Vec::new());
    }
    let all: Vec<DVec3> = outer.iter().chain(holes.iter().flatten()).map(pos).collect();
    let planar = all.iter().all(|p| (*p - opts[0]).dot(n).abs() < TOL);
    let has_dup = {
        let mut s = HashSet::new();
        !outer.iter().chain(holes.iter().flatten()).all(|v| s.insert(*v))
    };
    if planar && !has_dup {
        let holes: Vec<Vec<VertexId>> = holes
            .into_iter()
            .map(|mut h| {
                let hp: Vec<DVec3> = h.iter().map(pos).collect();
                if newell_normal(&hp).dot(n) > 0.0 {
                    h.reverse();
                }
                h
            })
            .collect();
        let mut key = outer.clone();
        key.sort();
        if let Some(&f) = seen.get(&key) {
            let back = ents.faces.get(f).is_some_and(|x| x.normal.dot(n) < 0.0);
            return (Vec::new(), if back { vec![f] } else { Vec::new() });
        }
        let made: Vec<FaceId> = ents.add_face_raw(outer, holes).into_iter().collect();
        if let Some(&f) = made.first() {
            seen.insert(key, f);
        }
        return (made, Vec::new());
    }
    // Triangles, in the polygon's best-fit plane.
    let (u, v) = esq_core::math::basis_for(n);
    let to2 = |p: DVec3| DVec2::new(p.dot(u), p.dot(v));
    let o2: Vec<DVec2> = outer.iter().map(|x| to2(pos(x))).collect();
    let h2: Vec<Vec<DVec2>> = holes.iter().map(|h| h.iter().map(|x| to2(pos(x))).collect()).collect();
    let ids: Vec<VertexId> = outer.iter().chain(holes.iter().flatten()).copied().collect();
    let mut boundary: HashSet<(VertexId, VertexId)> = HashSet::new();
    for l in std::iter::once(&outer).chain(holes.iter()) {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            boundary.insert((a.min(b), a.max(b)));
        }
    }
    let mut out = Vec::new();
    for [a, b, c] in triangulate_2d(&o2, &h2) {
        let mut t = vec![ids[a], ids[b], ids[c]];
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            continue;
        }
        if newell_normal(&[all[a], all[b], all[c]]).dot(n) < 0.0 {
            t.reverse();
        }
        let mut key = t.clone();
        key.sort();
        if seen.contains_key(&key) {
            continue;
        }
        if let Some(f) = ents.add_face_raw(t.clone(), Vec::new()) {
            seen.insert(key, f);
            out.push(f);
            for i in 0..3 {
                let (x, y) = (t[i], t[(i + 1) % 3]);
                if !boundary.contains(&(x.min(y), x.max(y)))
                    && let Some(e) = ents.find_edge(x, y)
                {
                    ents.edges[e].soft = true;
                    ents.edges[e].smooth = true;
                }
            }
        }
    }
    (out, Vec::new())
}

/// Positions the texture of a face from the texture coordinates of its vertices.
fn set_face_uv(ents: &mut Entities, f: FaceId, uv: &[(VertexId, DVec2)], tile: [f64; 2], front: bool) {
    let face = &ents.faces[f];
    let pairs: Vec<(DVec3, DVec2)> = face
        .all_vertices()
        .filter_map(|v| uv.iter().find(|(x, _)| *x == v).map(|(_, t)| (ents.verts[v].pos, *t)))
        .collect();
    if pairs.len() < 3 {
        return;
    }
    // Three well-spread, non-aligned corners.
    let a = 0;
    let b =
        (1..pairs.len()).max_by(|&i, &j| pairs[i].0.distance(pairs[a].0).total_cmp(&pairs[j].0.distance(pairs[a].0)));
    let Some(b) = b else { return };
    let ab = pairs[b].0 - pairs[a].0;
    let c = (1..pairs.len()).filter(|&i| i != b).max_by(|&i, &j| {
        ab.cross(pairs[i].0 - pairs[a].0).length().total_cmp(&ab.cross(pairs[j].0 - pairs[a].0).length())
    });
    let Some(c) = c else { return };
    let n = if front { face.normal } else { -face.normal };
    if let Some(map) = uv_from_pairs(n, tile, &[pairs[a], pairs[b], pairs[c]]) {
        let t = ents.faces[f].tex.get_or_insert_with(|| Box::new(FaceTex::default()));
        if front {
            t.front = Some(map);
        } else {
            t.back = Some(map);
        }
    }
}

/// Softens and smooths the edges between faces of a common smoothing group, or whose vertex
/// normals agree at both ends of the edge.
/// A face made from a polygon, with its smoothing group and vertex normals.
type Smooth = (FaceId, u32, Vec<(VertexId, DVec3)>);

fn apply_smoothing(ents: &mut Entities, info: &[Smooth]) {
    let by_face: HashMap<FaceId, &Smooth> = info.iter().map(|s| (s.0, s)).collect();
    let edges: Vec<EdgeId> = ents.edges.keys().collect();
    for e in edges {
        let edge = &ents.edges[e];
        if edge.faces.len() != 2 {
            continue;
        }
        let (Some(s1), Some(s2)) = (by_face.get(&edge.faces[0]), by_face.get(&edge.faces[1])) else {
            continue;
        };
        let (g1, n1, g2, n2) = (s1.1, &s1.2, s2.1, &s2.2);
        let (f1, f2) = (&ents.faces[edge.faces[0]], &ents.faces[edge.faces[1]]);
        if f1.normal.dot(f2.normal) > 1.0 - 1e-9 {
            continue;
        }
        let by_group = g1 & g2 != 0;
        let by_normals = !n1.is_empty()
            && !n2.is_empty()
            && edge.v.iter().all(|v| {
                let a = n1.iter().find(|(x, _)| x == v).map(|x| x.1);
                let b = n2.iter().find(|(x, _)| x == v).map(|x| x.1);
                matches!((a, b), (Some(a), Some(b)) if a != DVec3::ZERO && a.dot(b) > 1.0 - 1e-4)
            });
        if by_group || by_normals {
            let edge = &mut ents.edges[e];
            edge.soft = true;
            edge.smooth = true;
        }
    }
}

/// Makes the faces of each connected surface agree in orientation (neighbours run along
/// their common edge in opposite directions); a closed surface ends up facing outwards.
pub fn orient_faces(e: &mut Entities) {
    let faces: Vec<FaceId> = e.faces.keys().collect();
    let mut done: HashSet<FaceId> = HashSet::new();
    let dir = |e: &Entities, f: FaceId, a: VertexId, b: VertexId| -> Option<bool> {
        for l in e.faces[f].loops() {
            let n = l.len();
            for i in 0..n {
                let (x, y) = (l[i], l[(i + 1) % n]);
                if (x, y) == (a, b) {
                    return Some(true);
                }
                if (x, y) == (b, a) {
                    return Some(false);
                }
            }
        }
        None
    };
    for start in faces {
        if done.contains(&start) {
            continue;
        }
        let mut comp = vec![start];
        done.insert(start);
        let mut queue = vec![start];
        let mut closed = true;
        while let Some(f) = queue.pop() {
            for ed in e.face_edges(f) {
                let edge = &e.edges[ed];
                if edge.faces.len() != 2 {
                    closed = false;
                    continue;
                }
                let g = if edge.faces[0] == f { edge.faces[1] } else { edge.faces[0] };
                if done.contains(&g) || !e.faces.contains_key(g) {
                    continue;
                }
                let [a, b] = edge.v;
                if let (Some(df), Some(dg)) = (dir(e, f, a, b), dir(e, g, a, b))
                    && df == dg
                {
                    e.reverse_face(g);
                }
                done.insert(g);
                comp.push(g);
                queue.push(g);
            }
        }
        if closed && comp.len() > 1 {
            let mut vol = 0.0;
            for &f in &comp {
                for [a, b, c] in esq_core::tess::face_triangles(e, f) {
                    vol += a.dot(b.cross(c));
                }
            }
            if vol < 0.0 {
                for &f in &comp {
                    e.reverse_face(f);
                }
            }
        }
    }
}

/// "Fusionner les faces coplanaires": neighbouring faces of one plane, with the same
/// materials and texture positioning, become one face; the edges between them go away.
pub fn merge_coplanar(e: &mut Entities) {
    let ids: Vec<FaceId> = e.faces.keys().collect();
    let index: HashMap<FaceId, usize> = ids.iter().enumerate().map(|(i, f)| (*f, i)).collect();
    let mut parent: Vec<usize> = (0..ids.len()).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let same = |e: &Entities, f: FaceId, g: FaceId| {
        let (a, b) = (&e.faces[f], &e.faces[g]);
        a.normal.dot(b.normal) > 1.0 - 1e-9
            && ((e.verts[b.outer[0]].pos - e.verts[a.outer[0]].pos).dot(a.normal)).abs() < TOL
            && a.front == b.front
            && a.back == b.back
            && a.layer == b.layer
            && a.hidden == b.hidden
            && same_tex(a.tex.as_deref(), b.tex.as_deref())
    };
    let mut interior: HashSet<EdgeId> = HashSet::new();
    for (eid, edge) in e.edges.iter() {
        if edge.faces.len() == 2 && same(e, edge.faces[0], edge.faces[1]) {
            let (i, j) = (index[&edge.faces[0]], index[&edge.faces[1]]);
            let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
            parent[ri] = rj;
            interior.insert(eid);
        }
    }
    let mut groups: HashMap<usize, Vec<FaceId>> = HashMap::new();
    for (i, f) in ids.iter().enumerate() {
        let r = root(&mut parent, i);
        groups.entry(r).or_default().push(*f);
    }
    for (_, group) in groups {
        if group.len() < 2 {
            continue;
        }
        merge_group(e, &group);
    }
    for ed in interior {
        if e.edges.get(ed).is_some_and(|x| x.faces.is_empty()) {
            e.remove_edge_raw(ed);
        }
    }
    e.remove_orphan_vertices();
}

fn same_tex(a: Option<&FaceTex>, b: Option<&FaceTex>) -> bool {
    let eq = |x: Option<UvMap>, y: Option<UvMap>| match (x, y) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            x.u.iter().chain(x.v.iter()).zip(y.u.iter().chain(y.v.iter())).all(|(p, q)| (p - q).abs() < 1e-6)
        }
        _ => false,
    };
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => eq(a.front, b.front) && eq(a.back, b.back),
        _ => false,
    }
}

/// Replaces coplanar, edge-connected faces by the faces bounded by their outline.
fn merge_group(e: &mut Entities, group: &[FaceId]) {
    let first = e.faces[group[0]].clone();
    let n = first.normal;
    // Directed boundary half-edges: the ones not cancelled by a reverse half-edge.
    let mut half: HashMap<(VertexId, VertexId), usize> = HashMap::new();
    for &f in group {
        for l in e.faces[f].loops() {
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                if let Some(c) = half.get_mut(&(b, a))
                    && *c > 0
                {
                    *c -= 1;
                    continue;
                }
                *half.entry((a, b)).or_default() += 1;
            }
        }
    }
    let mut out: HashMap<VertexId, Vec<VertexId>> = HashMap::new();
    for (&(a, b), &c) in &half {
        for _ in 0..c {
            out.entry(a).or_default().push(b);
        }
    }
    let (bu, bv) = esq_core::math::basis_for(n);
    let p2 = |v: VertexId| DVec2::new(e.verts[v].pos.dot(bu), e.verts[v].pos.dot(bv));
    let mut loops: Vec<Vec<VertexId>> = Vec::new();
    let starts: Vec<VertexId> = out.keys().copied().collect();
    for s in starts {
        while out.get(&s).is_some_and(|v| !v.is_empty()) {
            let mut l = vec![s];
            let mut prev = s;
            let mut cur = {
                let v = out.get_mut(&s).unwrap();
                v.remove(0)
            };
            let mut guard = 0;
            while cur != s && guard < 1_000_000 {
                guard += 1;
                l.push(cur);
                let Some(nexts) = out.get_mut(&cur) else { break };
                if nexts.is_empty() {
                    break;
                }
                // At a vertex with several ways out, take the first one clockwise from the
                // way back (keeps the region on the left).
                let back = p2(prev) - p2(cur);
                let a_in = back.y.atan2(back.x);
                let mut best = 0;
                let mut best_a = f64::MAX;
                for (k, &w) in nexts.iter().enumerate() {
                    let d = p2(w) - p2(cur);
                    let mut cw = (a_in - d.y.atan2(d.x)).rem_euclid(std::f64::consts::TAU);
                    if cw < 1e-12 {
                        cw = std::f64::consts::TAU;
                    }
                    if cw < best_a {
                        best_a = cw;
                        best = k;
                    }
                }
                let w = nexts.remove(best);
                prev = cur;
                cur = w;
            }
            if l.len() >= 3 {
                loops.push(l);
            }
        }
    }
    let area = |l: &Vec<VertexId>| signed_area_2d(&l.iter().map(|&v| p2(v)).collect::<Vec<_>>());
    let (outers, holes): (Vec<Vec<VertexId>>, Vec<Vec<VertexId>>) = loops.into_iter().partition(|l| area(l) > 0.0);
    if outers.is_empty() {
        return;
    }
    let mut assigned: Vec<Vec<Vec<VertexId>>> = vec![Vec::new(); outers.len()];
    for h in holes {
        let probe = p2(h[0]);
        let best = outers
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                !o.contains(&h[0]) && point_in_polygon_2d(probe, &o.iter().map(|&v| p2(v)).collect::<Vec<_>>())
            })
            .min_by(|a, b| area(a.1).total_cmp(&area(b.1)))
            .map(|x| x.0);
        match best {
            Some(i) => assigned[i].push(h),
            None => return,
        }
    }
    for &f in group {
        e.remove_face(f);
    }
    for (o, hs) in outers.into_iter().zip(assigned) {
        if let Some(nf) = e.add_face_raw(o, hs) {
            let face = &mut e.faces[nf];
            face.normal = n;
            face.front = first.front;
            face.back = first.back;
            face.layer = first.layer;
            face.hidden = first.hidden;
            face.tex = first.tex.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad_as_triangles() -> SceneIn {
        let mut s = SceneIn::default();
        let pos = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(1.0, 1.0, 0.0),
            DVec3::new(0.0, 1.0, 0.0),
        ];
        let t = |a: u32, b: u32, c: u32| PolyIn { outer: vec![a, b, c], ..Default::default() };
        let m = s.add_mesh(MeshIn { pos, polys: vec![t(0, 1, 2), t(0, 2, 3)], ..Default::default() });
        s.root.meshes.push(m);
        s
    }

    /// A definition that contains itself (what a COLLADA `<instance_node>` of its own node
    /// gives) and one that does not exist: building recursed until the stack overflowed
    /// (process killed), or indexed `scene.defs` out of bounds.
    #[test]
    fn a_definition_that_contains_itself_is_cut() {
        let mut s = quad_as_triangles();
        let mesh = s.root.meshes.pop().unwrap();
        s.defs.push(DefIn {
            name: "Boucle".into(),
            meshes: vec![mesh],
            children: vec![NodeIn { def: Some(0), ..Default::default() }],
        });
        s.root.children.push(NodeIn { def: Some(0), ..Default::default() });
        s.root.children.push(NodeIn { def: Some(99), ..Default::default() });
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let m = build(&s, BuildOpts { scale: 1.0, ..Default::default() });
            let _ = tx.send((
                m.defs.len(),
                m.root.instances.len(),
                m.defs.values().map(|d| d.entities.instances.len()).sum::<usize>(),
            ));
        });
        let (defs, top, inner) = rx.recv_timeout(std::time::Duration::from_secs(20)).expect("the building ends");
        assert_eq!((defs, top, inner), (1, 1, 0));
    }

    /// Coordinates at the end of the range (1e300, `inf` in an STL): the weld grid's
    /// neighbouring cells overflowed i64 (a panic of the debug builds).
    #[test]
    fn huge_coordinates_do_not_overflow_the_weld_grid() {
        let mut s = SceneIn::default();
        let pos =
            vec![DVec3::new(1e300, 0.0, 0.0), DVec3::new(f64::INFINITY, 1.0, 0.0), DVec3::new(-1e300, -1e300, 1e300)];
        let m = s.add_mesh(MeshIn {
            pos,
            polys: vec![PolyIn { outer: vec![0, 1, 2], ..Default::default() }],
            ..Default::default()
        });
        s.root.meshes.push(m);
        let _ = build(&s, BuildOpts { scale: 1.0, ..Default::default() });
        let stl = b"solid x\nfacet normal 0 0 1\nouter loop\nvertex inf 0 0\nvertex 0 1e300 0\nvertex 0 0 -inf\nendloop\nendfacet\nendsolid x\n";
        let _ = crate::stl::import(stl, &Default::default());
    }

    #[test]
    fn triangles_are_welded_and_merged_when_asked() {
        let s = quad_as_triangles();
        let m = build(&s, BuildOpts { scale: 1000.0, ..Default::default() });
        assert_eq!(m.root.faces.len(), 2);
        assert_eq!(m.root.verts.len(), 4);
        assert_eq!(m.root.edges.len(), 5);
        let m = build(&s, BuildOpts { scale: 1000.0, merge_coplanar: true, ..Default::default() });
        assert_eq!(m.root.faces.len(), 1);
        assert_eq!(m.root.edges.len(), 4);
        let f = m.root.faces.values().next().unwrap();
        assert_eq!(f.outer.len(), 4);
        assert!((f.normal - DVec3::Z).length() < 1e-9);
        let far = m.root.verts.values().map(|v| v.pos.length()).fold(0.0, f64::max);
        assert!((far - 1000.0 * 2f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn merging_keeps_holes() {
        // A 3x3 grid of unit squares without the middle one.
        let mut s = SceneIn::default();
        let mut pos = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                pos.push(DVec3::new(x as f64, y as f64, 0.0));
            }
        }
        let mut polys = Vec::new();
        for y in 0..3u32 {
            for x in 0..3u32 {
                if (x, y) == (1, 1) {
                    continue;
                }
                let i = y * 4 + x;
                polys.push(PolyIn { outer: vec![i, i + 1, i + 5, i + 4], ..Default::default() });
            }
        }
        let m = s.add_mesh(MeshIn { pos, polys, ..Default::default() });
        s.root.meshes.push(m);
        let m = build(&s, BuildOpts { scale: 100.0, merge_coplanar: true, ..Default::default() });
        assert_eq!(m.root.faces.len(), 1);
        let f = m.root.faces.values().next().unwrap();
        assert_eq!(f.holes.len(), 1);
        assert_eq!(f.outer.len(), 12);
        assert_eq!(f.holes[0].len(), 4);
        let area = esq_core::tess::face_area(&m.root, m.root.faces.keys().next().unwrap());
        assert!((area - 80_000.0).abs() < 1e-6);
    }

    #[test]
    fn swap_yz_and_orientation() {
        let mut s = quad_as_triangles();
        // Second triangle flipped: orient makes it agree.
        s.meshes[0].polys[1].outer = vec![0, 3, 2];
        let m = build(&s, BuildOpts { scale: 1.0, swap_yz: true, orient_faces: true, ..Default::default() });
        let normals: Vec<DVec3> = m.root.faces.values().map(|f| f.normal).collect();
        assert!(normals[0].dot(normals[1]) > 0.99);
        // y up in the file → z up in the model: the quad stands vertically.
        assert!(normals[0].z.abs() < 1e-9 && normals[0].y.abs() > 0.99);
    }

    #[test]
    fn non_planar_quads_become_soft_triangles() {
        let mut s = SceneIn::default();
        let pos = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 10.0, 3.0),
            DVec3::new(0.0, 10.0, 0.0),
        ];
        let m = s.add_mesh(MeshIn {
            pos,
            polys: vec![PolyIn { outer: vec![0, 1, 2, 3], ..Default::default() }],
            ..Default::default()
        });
        s.root.meshes.push(m);
        let m = build(&s, BuildOpts::default());
        assert_eq!(m.root.faces.len(), 2);
        assert_eq!(m.root.edges.values().filter(|e| e.soft && e.smooth).count(), 1);
    }

    #[test]
    fn nodes_become_groups_and_shared_definitions_components() {
        let mut s = quad_as_triangles();
        let mesh = s.root.meshes.pop().unwrap();
        s.defs.push(DefIn { name: "Carré".into(), meshes: vec![mesh], children: Vec::new() });
        for i in 0..2 {
            s.root.children.push(NodeIn {
                xform: DMat4::from_translation(DVec3::new(i as f64 * 5.0, 0.0, 0.0)),
                def: Some(0),
                ..Default::default()
            });
        }
        s.root.children.push(NodeIn { name: "G".into(), meshes: vec![mesh], ..Default::default() });
        let m = build(&s, BuildOpts { scale: 10.0, ..Default::default() });
        assert_eq!(m.root.instances.len(), 3);
        assert_eq!(m.defs.len(), 2);
        let comp = m.defs.values().find(|d| !d.is_group).unwrap();
        assert_eq!(comp.name, "Carré");
        assert_eq!(m.instance_count(m.defs.iter().find(|(_, d)| !d.is_group).unwrap().0), 2);
        let xs: Vec<f64> = m.root.instances.values().map(|i| i.xform.w_axis.x).collect();
        assert!(xs.contains(&50.0));
    }
}
