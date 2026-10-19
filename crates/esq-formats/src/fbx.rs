//! Autodesk FBX (binary, version 7.4), as SketchUp's FBX exporter: one model per group or
//! component with its local transform, meshes with normals, texture coordinates and material
//! layers, Phong materials and textures, the scene's up axis and unit in `GlobalSettings`.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use esq_core::glam::{DMat4, DVec2, DVec3, EulerRot};
use esq_core::model::{DefId, MaterialId, Model};

use crate::obj::{BACK_COLOR, FRONT_COLOR, MatNames, face_uvs};
use crate::options::FbxExport;
use crate::scene::{Cache, Collection, Opts, WorldFace, XFace, sides};
use crate::tex::TexNames;

// ------------------------------------------------------------------------------------------
// Binary encoding

/// A property of a node.
pub enum P {
    Bool(bool),
    I16(i16),
    I32(i32),
    I64(i64),
    F64(f64),
    Str(String),
    Raw(Vec<u8>),
    I32s(Vec<i32>),
    F64s(Vec<f64>),
}

pub struct Node {
    pub name: &'static str,
    pub props: Vec<P>,
    pub children: Vec<Node>,
}

impl Node {
    pub fn new(name: &'static str, props: Vec<P>) -> Node {
        Node { name, props, children: Vec::new() }
    }

    pub fn with(mut self, children: Vec<Node>) -> Node {
        self.children = children;
        self
    }

    pub fn push(&mut self, n: Node) {
        self.children.push(n);
    }
}

fn s(v: &str) -> P {
    P::Str(v.to_string())
}

/// "Name\0\1Class": how FBX names objects.
fn obj_name(name: &str, class: &str) -> P {
    P::Str(format!("{name}\u{0}\u{1}{class}"))
}

fn encode_props(props: &[P]) -> Vec<u8> {
    let mut b = Vec::new();
    let array = |b: &mut Vec<u8>, code: u8, n: usize, data: Vec<u8>| {
        b.push(code);
        b.extend_from_slice(&(n as u32).to_le_bytes());
        if data.len() > 128 {
            let z = miniz_oxide::deflate::compress_to_vec_zlib(&data, 1);
            b.extend_from_slice(&1u32.to_le_bytes());
            b.extend_from_slice(&(z.len() as u32).to_le_bytes());
            b.extend_from_slice(&z);
        } else {
            b.extend_from_slice(&0u32.to_le_bytes());
            b.extend_from_slice(&(data.len() as u32).to_le_bytes());
            b.extend_from_slice(&data);
        }
    };
    for p in props {
        match p {
            P::Bool(v) => {
                b.push(b'C');
                b.push(*v as u8);
            }
            P::I16(v) => {
                b.push(b'Y');
                b.extend_from_slice(&v.to_le_bytes());
            }
            P::I32(v) => {
                b.push(b'I');
                b.extend_from_slice(&v.to_le_bytes());
            }
            P::I64(v) => {
                b.push(b'L');
                b.extend_from_slice(&v.to_le_bytes());
            }
            P::F64(v) => {
                b.push(b'D');
                b.extend_from_slice(&v.to_le_bytes());
            }
            P::Str(v) => {
                b.push(b'S');
                b.extend_from_slice(&(v.len() as u32).to_le_bytes());
                b.extend_from_slice(v.as_bytes());
            }
            P::Raw(v) => {
                b.push(b'R');
                b.extend_from_slice(&(v.len() as u32).to_le_bytes());
                b.extend_from_slice(v);
            }
            P::I32s(v) => array(&mut b, b'i', v.len(), v.iter().flat_map(|x| x.to_le_bytes()).collect()),
            P::F64s(v) => array(&mut b, b'd', v.len(), v.iter().flat_map(|x| x.to_le_bytes()).collect()),
        }
    }
    b
}

const SENTINEL: [u8; 13] = [0; 13];

fn write_node(out: &mut Vec<u8>, n: &Node, is_last: bool) {
    let start = out.len();
    let props = encode_props(&n.props);
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(n.props.len() as u32).to_le_bytes());
    out.extend_from_slice(&(props.len() as u32).to_le_bytes());
    out.push(n.name.len() as u8);
    out.extend_from_slice(n.name.as_bytes());
    out.extend_from_slice(&props);
    if !n.children.is_empty() {
        let k = n.children.len();
        for (i, c) in n.children.iter().enumerate() {
            write_node(out, c, i + 1 == k);
        }
        out.extend_from_slice(&SENTINEL);
    } else if n.props.is_empty() && !is_last {
        out.extend_from_slice(&SENTINEL);
    }
    let end = out.len() as u32;
    out[start..start + 4].copy_from_slice(&end.to_le_bytes());
}

/// The fixed identifiers that make FBX readers accept a file written at any time.
const FILE_ID: [u8; 16] =
    [0x28, 0xb3, 0x2a, 0xeb, 0xb6, 0x24, 0xcc, 0xc2, 0xbf, 0xc8, 0xb0, 0x2a, 0xa9, 0x2b, 0xfc, 0xf1];
const TIME_ID: &str = "1970-01-01 10:00:00:000";
const FOOT_ID: [u8; 16] =
    [0xfa, 0xbc, 0xab, 0x09, 0xd0, 0xc8, 0xd4, 0x66, 0xb1, 0x76, 0xfb, 0x83, 0x1c, 0xf7, 0x26, 0x7e];
const TRAILER: [u8; 16] =
    [0xf8, 0x5a, 0x8c, 0x6a, 0xde, 0xf5, 0xd9, 0x7e, 0xec, 0xe9, 0x0c, 0xe3, 0x75, 0x8f, 0x29, 0x0b];
pub const VERSION: u32 = 7400;

/// A whole binary FBX file from its top-level nodes.
pub fn encode(nodes: &[Node]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"Kaydara FBX Binary  \x00\x1a\x00");
    out.extend_from_slice(&VERSION.to_le_bytes());
    let k = nodes.len();
    for (i, n) in nodes.iter().enumerate() {
        write_node(&mut out, n, i + 1 == k);
    }
    out.extend_from_slice(&SENTINEL);
    out.extend_from_slice(&FOOT_ID);
    out.extend_from_slice(&[0; 4]);
    let pad = 16 - out.len() % 16;
    out.extend(std::iter::repeat_n(0u8, if pad == 0 { 16 } else { pad }));
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&[0; 120]);
    out.extend_from_slice(&TRAILER);
    out
}

fn p70(entries: Vec<Node>) -> Node {
    Node::new("Properties70", vec![]).with(entries)
}

fn prop(name: &str, ty: &str, label: &str, flags: &str, vals: Vec<P>) -> Node {
    let mut v = vec![s(name), s(ty), s(label), s(flags)];
    v.extend(vals);
    Node::new("P", v)
}

fn vec3_prop(name: &str, v: DVec3) -> Node {
    prop(name, name, "", "A", vec![P::F64(v.x), P::F64(v.y), P::F64(v.z)])
}

// ------------------------------------------------------------------------------------------
// Scene

/// A mesh written once: (definition, material it inherits, surface number).
type GeoKey = (Option<DefId>, Option<MaterialId>, usize);

struct Exp<'a> {
    model: &'a Model,
    o: &'a FbxExport,
    conv: DMat4,
    next_id: i64,
    objects: Vec<Node>,
    connections: Vec<Node>,
    mats: MatNames,
    mat_ids: HashMap<String, i64>,
    tex_dir: std::path::PathBuf,
    tex_rel: String,
    texnames: TexNames,
    textures: usize,
    geometries: HashMap<GeoKey, (i64, Vec<String>)>,
    models: usize,
    faces: usize,
    names: Vec<String>,
}

impl Exp<'_> {
    fn id(&mut self) -> i64 {
        self.next_id += 1;
        self.next_id
    }

    fn unique(&mut self, base: &str) -> String {
        let mut n = base.to_string();
        let mut i = 1;
        while self.names.contains(&n) {
            n = format!("{base}{i}");
            i += 1;
        }
        self.names.push(n.clone());
        n
    }

    fn connect(&mut self, child: i64, parent: i64) {
        self.connections.push(Node::new("C", vec![s("OO"), P::I64(child), P::I64(parent)]));
    }

    /// The FBX material named `name` (made on first use).
    fn material(&mut self, name: &str, mat: Option<MaterialId>, front: bool) -> i64 {
        if let Some(&id) = self.mat_ids.get(name) {
            return id;
        }
        let id = self.id();
        self.mat_ids.insert(name.to_string(), id);
        let m = mat.and_then(|m| self.model.materials.get(m));
        let (c, opacity) = match m {
            Some(m) => (m.color, m.opacity as f64),
            None if front => (FRONT_COLOR, 1.0),
            None => (BACK_COLOR, 1.0),
        };
        let c = DVec3::new(c[0] as f64, c[1] as f64, c[2] as f64) / 255.0;
        let color = |n: &str, v: DVec3| prop(n, "Color", "", "A", vec![P::F64(v.x), P::F64(v.y), P::F64(v.z)]);
        let node = Node::new("Material", vec![P::I64(id), obj_name(name, "Material"), s("")]).with(vec![
            Node::new("Version", vec![P::I32(102)]),
            Node::new("ShadingModel", vec![s("Phong")]),
            Node::new("MultiLayer", vec![P::I32(0)]),
            p70(vec![
                color("DiffuseColor", c),
                prop("DiffuseFactor", "Number", "", "A", vec![P::F64(1.0)]),
                color("AmbientColor", c),
                prop("TransparencyFactor", "Number", "", "A", vec![P::F64(1.0 - opacity)]),
                prop("Opacity", "double", "Number", "", vec![P::F64(opacity)]),
                color("SpecularColor", DVec3::ZERO),
            ]),
        ]);
        self.objects.push(node);
        if self.o.texture_maps
            && let Some(t) = m.and_then(|m| m.ext.texture.as_ref())
        {
            let file = self.texnames.name_for(t);
            if std::fs::create_dir_all(&self.tex_dir).is_ok() {
                let _ = std::fs::write(self.tex_dir.join(&file), &t.image.0[..]);
            }
            let rel = format!("{}/{}", self.tex_rel, file);
            let full = self.tex_dir.join(&file).to_string_lossy().into_owned();
            let (tid, vid) = (self.id(), self.id());
            self.objects.push(Node::new("Video", vec![P::I64(vid), obj_name(&file, "Video"), s("Clip")]).with(vec![
                Node::new("Type", vec![s("Clip")]),
                p70(vec![prop("Path", "KString", "XRefUrl", "", vec![s(&full)])]),
                Node::new("UseMipMap", vec![P::I32(0)]),
                Node::new("Filename", vec![s(&full)]),
                Node::new("RelativeFilename", vec![s(&rel)]),
            ]));
            self.objects.push(Node::new("Texture", vec![P::I64(tid), obj_name(&file, "Texture"), s("")]).with(vec![
                Node::new("Type", vec![s("TextureVideoClip")]),
                Node::new("Version", vec![P::I32(202)]),
                Node::new("TextureName", vec![obj_name(&file, "Texture")]),
                p70(vec![prop("UseMaterial", "bool", "", "", vec![P::I32(1)])]),
                Node::new("Media", vec![obj_name(&file, "Video")]),
                Node::new("FileName", vec![s(&full)]),
                Node::new("RelativeFilename", vec![s(&rel)]),
                Node::new("ModelUVTranslation", vec![P::F64(0.0), P::F64(0.0)]),
                Node::new("ModelUVScaling", vec![P::F64(1.0), P::F64(1.0)]),
                Node::new("Texture_Alpha_Source", vec![s("None")]),
                Node::new("Cropping", vec![P::I32(0), P::I32(0), P::I32(0), P::I32(0)]),
            ]));
            self.connect(vid, tid);
            self.connections.push(Node::new("C", vec![s("OP"), P::I64(tid), P::I64(id), s("DiffuseColor")]));
            self.textures += 1;
        }
        id
    }

    /// Mesh of some faces of a collection: geometry id and the material names in layer order.
    fn geometry(&mut self, name: &str, faces: &[&XFace], inherited: Option<MaterialId>) -> (i64, Vec<String>) {
        let mut pos: Vec<DVec3> = Vec::new();
        let mut index: HashMap<[u64; 3], i32> = HashMap::new();
        let mut pvi: Vec<i32> = Vec::new();
        let mut normals: Vec<f64> = Vec::new();
        let mut uv: Vec<DVec2> = Vec::new();
        let mut uvi: Vec<i32> = Vec::new();
        let mut uvmap: HashMap<[u64; 2], i32> = HashMap::new();
        let mut poly_mat: Vec<i32> = Vec::new();
        let mut mat_names: Vec<String> = Vec::new();
        for face in faces {
            let wf = WorldFace {
                face,
                xform: DMat4::IDENTITY,
                front: face.front.or(inherited),
                back: face.back.or(inherited),
            };
            let pts: Vec<DVec3> = face.points().iter().map(|p| self.conv.transform_point3(*p)).collect();
            for &front in sides(self.o.two_sided) {
                let mat = if front { wf.front } else { wf.back };
                let mname = self.mats.get(self.model, mat, front);
                self.material(&mname, mat, front);
                let mi = match mat_names.iter().position(|m| *m == mname) {
                    Some(i) => i,
                    None => {
                        mat_names.push(mname.clone());
                        mat_names.len() - 1
                    }
                } as i32;
                let uvs = face_uvs(self.model, &wf, mat, front);
                let n = self.conv.transform_vector3(if front { face.normal } else { -face.normal }).normalize_or_zero();
                let polys: Vec<Vec<usize>> = if !self.o.triangulate && face.is_simple() {
                    let mut l = wf.loops().remove(0);
                    if !front {
                        l.reverse();
                    }
                    vec![l]
                } else {
                    wf.tris().into_iter().map(|t| if front { t.to_vec() } else { vec![t[0], t[2], t[1]] }).collect()
                };
                for p in polys {
                    let k = p.len();
                    for (j, &c) in p.iter().enumerate() {
                        let q = pts[c];
                        let vi = *index.entry(q.to_array().map(f64::to_bits)).or_insert_with(|| {
                            pos.push(q);
                            pos.len() as i32 - 1
                        });
                        pvi.push(if j + 1 == k { !vi } else { vi });
                        normals.extend([n.x, n.y, n.z]);
                        let t = uvs[c];
                        let ti = *uvmap.entry(t.to_array().map(f64::to_bits)).or_insert_with(|| {
                            uv.push(t);
                            uv.len() as i32 - 1
                        });
                        uvi.push(ti);
                    }
                    poly_mat.push(mi);
                    self.faces += 1;
                }
            }
        }
        let gid = self.id();
        let layer_ref = |t: &str| {
            Node::new("LayerElement", vec![])
                .with(vec![Node::new("Type", vec![s(t)]), Node::new("TypedIndex", vec![P::I32(0)])])
        };
        let node = Node::new("Geometry", vec![P::I64(gid), obj_name(name, "Geometry"), s("Mesh")]).with(vec![
            p70(vec![]),
            Node::new("GeometryVersion", vec![P::I32(124)]),
            Node::new("Vertices", vec![P::F64s(pos.iter().flat_map(|p| [p.x, p.y, p.z]).collect())]),
            Node::new("PolygonVertexIndex", vec![P::I32s(pvi)]),
            Node::new("LayerElementNormal", vec![P::I32(0)]).with(vec![
                Node::new("Version", vec![P::I32(102)]),
                Node::new("Name", vec![s("")]),
                Node::new("MappingInformationType", vec![s("ByPolygonVertex")]),
                Node::new("ReferenceInformationType", vec![s("Direct")]),
                Node::new("Normals", vec![P::F64s(normals)]),
            ]),
            Node::new("LayerElementUV", vec![P::I32(0)]).with(vec![
                Node::new("Version", vec![P::I32(101)]),
                Node::new("Name", vec![s("UVSet0")]),
                Node::new("MappingInformationType", vec![s("ByPolygonVertex")]),
                Node::new("ReferenceInformationType", vec![s("IndexToDirect")]),
                Node::new("UV", vec![P::F64s(uv.iter().flat_map(|t| [t.x, t.y]).collect())]),
                Node::new("UVIndex", vec![P::I32s(uvi)]),
            ]),
            Node::new("LayerElementMaterial", vec![P::I32(0)]).with(vec![
                Node::new("Version", vec![P::I32(101)]),
                Node::new("Name", vec![s("")]),
                Node::new("MappingInformationType", vec![s("ByPolygon")]),
                Node::new("ReferenceInformationType", vec![s("IndexToDirect")]),
                Node::new("Materials", vec![P::I32s(poly_mat)]),
            ]),
            Node::new("Layer", vec![P::I32(0)]).with(vec![
                Node::new("Version", vec![P::I32(100)]),
                layer_ref("LayerElementNormal"),
                layer_ref("LayerElementMaterial"),
                layer_ref("LayerElementUV"),
            ]),
        ]);
        self.objects.push(node);
        (gid, mat_names)
    }

    /// An FBX model: transform, optional mesh, materials, children.
    fn model_node(&mut self, name: &str, xform: &DMat4, geo: Option<&(i64, Vec<String>)>, parent: i64) -> i64 {
        let mid = self.id();
        let name = self.unique(name);
        let c = self.conv * *xform * self.conv.inverse();
        let (sc, r, t) = c.to_scale_rotation_translation();
        let (z, y, x) = r.to_euler(EulerRot::ZYX);
        let node = Node::new(
            "Model",
            vec![P::I64(mid), obj_name(&name, "Model"), s(if geo.is_some() { "Mesh" } else { "Null" })],
        )
        .with(vec![
            Node::new("Version", vec![P::I32(232)]),
            p70(vec![
                vec3_prop("Lcl Translation", t),
                vec3_prop("Lcl Rotation", DVec3::new(x, y, z) * (180.0 / std::f64::consts::PI)),
                vec3_prop("Lcl Scaling", sc),
                prop("DefaultAttributeIndex", "int", "Integer", "", vec![P::I32(0)]),
                prop("InheritType", "enum", "", "", vec![P::I32(1)]),
            ]),
            Node::new("Shading", vec![P::Bool(true)]),
            Node::new("Culling", vec![s("CullingOff")]),
        ]);
        self.objects.push(node);
        self.connect(mid, parent);
        self.models += 1;
        if let Some((gid, mats)) = geo {
            self.connect(*gid, mid);
            for m in mats.clone() {
                let id = self.mat_ids[&m];
                self.connect(id, mid);
            }
        }
        mid
    }

    /// A collection: its meshes (split per connected surface when asked) and its instances.
    fn collection(
        &mut self,
        cache: &mut Cache,
        def: Option<DefId>,
        inherited: Option<MaterialId>,
        name: &str,
        parent: i64,
        depth: usize,
    ) {
        let coll: Collection = cache.get(def).clone();
        let groups: Vec<Vec<&XFace>> = if self.o.separate_disconnected {
            connected(&coll.faces)
        } else if coll.faces.is_empty() {
            Vec::new()
        } else {
            vec![coll.faces.iter().collect()]
        };
        for (k, faces) in groups.iter().enumerate() {
            let key = (def, inherited, k);
            let geo = match self.geometries.get(&key) {
                Some(g) => g.clone(),
                None => {
                    let g = self.geometry(
                        &format!("{name}{}", if k == 0 { String::new() } else { k.to_string() }),
                        faces,
                        inherited,
                    );
                    self.geometries.insert(key, g.clone());
                    g
                }
            };
            let mname = if groups.len() > 1 { format!("{name}-Mesh{}", k + 1) } else { format!("{name}-Mesh") };
            self.model_node(&mname, &DMat4::IDENTITY, Some(&geo), parent);
        }
        if depth > 64 {
            return;
        }
        for inst in &coll.instances {
            let d = &self.model.defs[inst.def];
            let n = if inst.name.is_empty() { d.name.clone() } else { inst.name.clone() };
            let id = self.model_node(&n, &inst.xform, None, parent);
            self.collection(cache, Some(inst.def), inst.material.or(inherited), &d.name.clone(), id, depth + 1);
        }
    }
}

/// Faces in groups that share edges (by position).
fn connected(faces: &[XFace]) -> Vec<Vec<&XFace>> {
    let key = |p: &DVec3| p.to_array().map(f64::to_bits);
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut first: HashMap<[u64; 3], usize> = HashMap::new();
    for (i, f) in faces.iter().enumerate() {
        for p in f.points() {
            match first.get(&key(&p)) {
                Some(&j) => {
                    let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                    parent[a] = b;
                }
                None => {
                    first.insert(key(&p), i);
                }
            }
        }
    }
    let mut groups: Vec<(usize, Vec<&XFace>)> = Vec::new();
    for (i, f) in faces.iter().enumerate() {
        let r = root(&mut parent, i);
        match groups.iter_mut().find(|g| g.0 == r) {
            Some(g) => g.1.push(f),
            None => groups.push((r, vec![f])),
        }
    }
    groups.into_iter().map(|g| g.1).collect()
}

/// File › Export › 3D Model › FBX.
pub fn export(model: &Model, path: &Path, o: &FbxExport) -> Result<String> {
    let unit = o.units.resolve(model.units);
    let k = unit.mm(model.units);
    let swap = if o.swap_yz { crate::build::y_up_to_z_up().inverse() } else { DMat4::IDENTITY };
    let conv = swap * DMat4::from_scale(DVec3::splat(1.0 / k));
    let mut cache = Cache::new(model, Opts::default());
    let processed = crate::report::processed(&mut cache, 0);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "model".into());
    let mut e = Exp {
        model,
        o,
        conv,
        next_id: 1_000_000,
        objects: Vec::new(),
        connections: Vec::new(),
        mats: MatNames::new(),
        mat_ids: HashMap::new(),
        tex_dir: path.with_file_name(&stem),
        tex_rel: stem.clone(),
        texnames: TexNames::new(),
        textures: 0,
        geometries: HashMap::new(),
        models: 0,
        faces: 0,
        names: Vec::new(),
    };
    e.collection(&mut cache, None, None, "SketchUp", 0, 0);
    // Global settings: axes and the size of a file unit in centimetres.
    let (up, front, front_sign) = if o.swap_yz { (1, 2, 1) } else { (2, 1, -1) };
    let int = |n: &str, v: i32| prop(n, "int", "Integer", "", vec![P::I32(v)]);
    let global = Node::new("GlobalSettings", vec![]).with(vec![
        Node::new("Version", vec![P::I32(1000)]),
        p70(vec![
            int("UpAxis", up),
            int("UpAxisSign", 1),
            int("FrontAxis", front),
            int("FrontAxisSign", front_sign),
            int("CoordAxis", 0),
            int("CoordAxisSign", 1),
            int("OriginalUpAxis", up),
            int("OriginalUpAxisSign", 1),
            prop("UnitScaleFactor", "double", "Number", "", vec![P::F64(k / 10.0)]),
            prop("OriginalUnitScaleFactor", "double", "Number", "", vec![P::F64(k / 10.0)]),
        ]),
    ]);
    let now = time_now();
    let header = Node::new("FBXHeaderExtension", vec![]).with(vec![
        Node::new("FBXHeaderVersion", vec![P::I32(1003)]),
        Node::new("FBXVersion", vec![P::I32(VERSION as i32)]),
        Node::new("EncryptionType", vec![P::I32(0)]),
        Node::new("CreationTimeStamp", vec![]).with(vec![
            Node::new("Version", vec![P::I32(1000)]),
            Node::new("Year", vec![P::I32(now[0])]),
            Node::new("Month", vec![P::I32(now[1])]),
            Node::new("Day", vec![P::I32(now[2])]),
            Node::new("Hour", vec![P::I32(now[3])]),
            Node::new("Minute", vec![P::I32(now[4])]),
            Node::new("Second", vec![P::I32(now[5])]),
            Node::new("Millisecond", vec![P::I32(0)]),
        ]),
        Node::new("Creator", vec![s("SketchUp FBX Exporter")]),
    ]);
    let count = |t: &str| e.objects.iter().filter(|n| n.name == t).count() as i32;
    let mut defs = vec![
        Node::new("Version", vec![P::I32(100)]),
        Node::new("Count", vec![P::I32(e.objects.len() as i32 + 1)]),
        Node::new("ObjectType", vec![s("GlobalSettings")]).with(vec![Node::new("Count", vec![P::I32(1)])]),
    ];
    for t in ["Model", "Geometry", "Material", "Texture", "Video"] {
        let n = count(t);
        if n > 0 {
            defs.push(Node::new("ObjectType", vec![s(t)]).with(vec![Node::new("Count", vec![P::I32(n)])]));
        }
    }
    let doc_id = 999_999i64;
    let nodes = vec![
        header,
        Node::new("FileId", vec![P::Raw(FILE_ID.to_vec())]),
        Node::new("CreationTime", vec![s(TIME_ID)]),
        Node::new("Creator", vec![s("SketchUp FBX Exporter")]),
        global,
        Node::new("Documents", vec![]).with(vec![
            Node::new("Count", vec![P::I32(1)]),
            Node::new("Document", vec![P::I64(doc_id), s(""), s("Scene")]).with(vec![
                p70(vec![
                    prop("SourceObject", "object", "", "", vec![]),
                    prop("ActiveAnimStackName", "KString", "", "", vec![s("")]),
                ]),
                Node::new("RootNode", vec![P::I64(0)]),
            ]),
        ]),
        Node::new("References", vec![]),
        Node::new("Definitions", vec![]).with(defs),
        Node::new("Objects", vec![]).with(std::mem::take(&mut e.objects)),
        Node::new("Connections", vec![]).with(std::mem::take(&mut e.connections)),
        Node::new("Takes", vec![]).with(vec![Node::new("Current", vec![s("")])]),
    ];
    std::fs::write(path, encode(&nodes))?;
    Ok(crate::report::text(
        "FBX",
        &processed,
        &[("Groupes", e.models), ("Faces", e.faces), ("Matières", e.mats.names.len()), ("Textures", e.textures)],
    ))
}

/// Year, month, day, hour, minute, second (UTC).
fn time_now() -> [i32; 6] {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    [y as i32, m as i32, d as i32, (rem / 3600) as i32, (rem / 60 % 60) as i32, (rem % 60) as i32]
}

// ------------------------------------------------------------------------------------------
// Reading (for the tests: decodes the node tree back)

#[cfg(test)]
pub mod read {
    pub struct RNode {
        pub name: String,
        pub props: Vec<String>,
        pub arrays: Vec<Vec<f64>>,
        pub children: Vec<RNode>,
    }

    fn arr(b: &[u8], i: &mut usize, size: usize, f: impl Fn(&[u8]) -> f64) -> Vec<f64> {
        let n = u32::from_le_bytes(b[*i..*i + 4].try_into().unwrap()) as usize;
        let enc = u32::from_le_bytes(b[*i + 4..*i + 8].try_into().unwrap());
        let len = u32::from_le_bytes(b[*i + 8..*i + 12].try_into().unwrap()) as usize;
        let raw = &b[*i + 12..*i + 12 + len];
        *i += 12 + len;
        let data = if enc == 1 { miniz_oxide::inflate::decompress_to_vec_zlib(raw).unwrap() } else { raw.to_vec() };
        assert_eq!(data.len(), n * size);
        data.chunks(size).map(f).collect()
    }

    pub fn node(b: &[u8], at: usize) -> Option<(RNode, usize)> {
        let end = u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize;
        if end == 0 {
            return None;
        }
        let np = u32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap());
        let plen = u32::from_le_bytes(b[at + 8..at + 12].try_into().unwrap()) as usize;
        let nl = b[at + 12] as usize;
        let name = String::from_utf8_lossy(&b[at + 13..at + 13 + nl]).into_owned();
        let mut i = at + 13 + nl;
        let pend = i + plen;
        let (mut props, mut arrays) = (Vec::new(), Vec::new());
        for _ in 0..np {
            let t = b[i];
            i += 1;
            match t {
                b'C' => {
                    props.push(b[i].to_string());
                    i += 1;
                }
                b'Y' => {
                    props.push(i16::from_le_bytes(b[i..i + 2].try_into().unwrap()).to_string());
                    i += 2;
                }
                b'I' => {
                    props.push(i32::from_le_bytes(b[i..i + 4].try_into().unwrap()).to_string());
                    i += 4;
                }
                b'L' => {
                    props.push(i64::from_le_bytes(b[i..i + 8].try_into().unwrap()).to_string());
                    i += 8;
                }
                b'D' => {
                    props.push(f64::from_le_bytes(b[i..i + 8].try_into().unwrap()).to_string());
                    i += 8;
                }
                b'S' | b'R' => {
                    let l = u32::from_le_bytes(b[i..i + 4].try_into().unwrap()) as usize;
                    props.push(String::from_utf8_lossy(&b[i + 4..i + 4 + l]).into_owned());
                    i += 4 + l;
                }
                b'i' => arrays.push(arr(b, &mut i, 4, |c| i32::from_le_bytes(c.try_into().unwrap()) as f64)),
                b'd' => arrays.push(arr(b, &mut i, 8, |c| f64::from_le_bytes(c.try_into().unwrap()))),
                t => panic!("type {t}"),
            }
        }
        assert_eq!(i, pend);
        let mut children = Vec::new();
        while i < end {
            match node(b, i) {
                Some((c, next)) => {
                    children.push(c);
                    i = next;
                }
                None => i += 13,
            }
        }
        Some((RNode { name, props, arrays, children }, end))
    }

    pub fn top(b: &[u8]) -> Vec<RNode> {
        assert!(b.starts_with(b"Kaydara FBX Binary  \x00\x1a\x00"));
        let mut i = 27;
        let mut out = Vec::new();
        while let Some((n, next)) = node(b, i) {
            out.push(n);
            i = next;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::model::{Definition, Instance};

    #[test]
    fn writes_a_readable_binary_scene() {
        let mut m = Model::new();
        let mut sq = esq_core::model::Entities::default();
        for x0 in [0.0, 500.0] {
            let v: Vec<_> = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]
                .iter()
                .map(|(x, y)| sq.add_vertex_raw(DVec3::new(x0 + *x, *y, 0.0)))
                .collect();
            sq.add_face_raw(v, vec![]);
        }
        let d = m.defs.insert(Definition {
            name: "Deux".into(),
            is_group: false,
            description: String::new(),
            entities: sq,
            attributes: Default::default(),
            ext: Default::default(),
        });
        m.root.instances.insert(Instance {
            def: d,
            xform: DMat4::from_translation(DVec3::new(0.0, 0.0, 1000.0)),
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
        let dir = std::env::temp_dir().join(format!("esq_fbx_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("deux.fbx");
        let rep =
            export(&m, &p, &FbxExport { units: crate::units::LenUnit::Millimeters, ..Default::default() }).unwrap();
        assert!(rep.contains("Entités FBX exportées"));
        let b = std::fs::read(&p).unwrap();
        assert_eq!(&b[b.len() - 16..], &TRAILER);
        let top = read::top(&b);
        let names: Vec<&str> = top.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "FBXHeaderExtension",
                "FileId",
                "CreationTime",
                "Creator",
                "GlobalSettings",
                "Documents",
                "References",
                "Definitions",
                "Objects",
                "Connections",
                "Takes"
            ]
        );
        let objects = &top[8];
        let geos: Vec<&read::RNode> = objects.children.iter().filter(|n| n.name == "Geometry").collect();
        assert_eq!(geos.len(), 1);
        let verts = &geos[0].children.iter().find(|n| n.name == "Vertices").unwrap().arrays[0];
        assert_eq!(verts.len(), 8 * 3);
        // Swap YZ (on by default): z = -y in the file.
        assert!(verts.chunks(3).all(|v| v[1] == 0.0));
        let pvi = &geos[0].children.iter().find(|n| n.name == "PolygonVertexIndex").unwrap().arrays[0];
        assert_eq!(pvi.len(), 8);
        assert_eq!(pvi.iter().filter(|x| **x < 0.0).count(), 2);
        // Separated: two meshes.
        let o = FbxExport { separate_disconnected: true, triangulate: true, ..Default::default() };
        export(&m, &p, &o).unwrap();
        let top = read::top(&std::fs::read(&p).unwrap());
        let geos = top[8].children.iter().filter(|n| n.name == "Geometry").count();
        assert_eq!(geos, 2);
        std::fs::remove_dir_all(&dir).ok();
    }
}
