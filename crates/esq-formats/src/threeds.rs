//! 3D Studio (.3ds), after the public chunk specification: SketchUp's 3DS importer and
//! exporter. Meshes are triangles with edge-visibility flags and smoothing groups; the
//! keyframer section carries the object hierarchy.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Result, bail};
use esq_core::glam::{DMat4, DQuat, DVec2, DVec3};
use esq_core::model::{LayerIdx, MaterialId, Model};

use crate::build::{BuildOpts, MatIn, MeshIn, NodeIn, NodeKind, PolyIn, SceneIn};
use crate::obj::{BACK_COLOR, FRONT_COLOR};
use crate::options::{Favor3ds, Hierarchy3ds, ThreeDsExport, ThreeDsImport, TwoSidedAs};
use crate::scene::{Cache, Opts, WorldFace, placements};
use crate::tex::{TexNames, find_beside, texture_from_bytes};

const MAIN: u16 = 0x4D4D;
const VERSION: u16 = 0x0002;
const EDIT: u16 = 0x3D3D;
const MESH_VERSION: u16 = 0x3D3E;
const MASTER_SCALE: u16 = 0x0100;
const NAMED_OBJECT: u16 = 0x4000;
const TRI_OBJECT: u16 = 0x4100;
const POINT_ARRAY: u16 = 0x4110;
const FACE_ARRAY: u16 = 0x4120;
const MSH_MAT_GROUP: u16 = 0x4130;
const TEX_VERTS: u16 = 0x4140;
const SMOOTH_GROUP: u16 = 0x4150;
const MESH_MATRIX: u16 = 0x4160;
const N_CAMERA: u16 = 0x4700;
const MATERIAL: u16 = 0xAFFF;
const MAT_NAME: u16 = 0xA000;
const MAT_AMBIENT: u16 = 0xA010;
const MAT_DIFFUSE: u16 = 0xA020;
const MAT_SPECULAR: u16 = 0xA030;
const MAT_TRANSPARENCY: u16 = 0xA050;
const MAT_TWO_SIDE: u16 = 0xA081;
const MAT_TEXMAP: u16 = 0xA200;
const MAT_MAPNAME: u16 = 0xA300;
const MAT_MAP_USCALE: u16 = 0xA354;
const MAT_MAP_VSCALE: u16 = 0xA356;
const MAT_MAP_UOFFSET: u16 = 0xA358;
const MAT_MAP_VOFFSET: u16 = 0xA35A;
const COLOR_F: u16 = 0x0010;
const COLOR_24: u16 = 0x0011;
const LIN_COLOR_24: u16 = 0x0012;
const INT_PERCENT: u16 = 0x0030;
const FLOAT_PERCENT: u16 = 0x0031;
const KFDATA: u16 = 0xB000;
const KFHDR: u16 = 0xB00A;
const KFSEG: u16 = 0xB008;
const OBJECT_NODE: u16 = 0xB002;
const NODE_ID: u16 = 0xB030;
const NODE_HDR: u16 = 0xB010;
const INSTANCE_NAME: u16 = 0xB011;
const PIVOT: u16 = 0xB013;
const POS_TRACK: u16 = 0xB020;
const ROT_TRACK: u16 = 0xB021;
const SCL_TRACK: u16 = 0xB022;

// ------------------------------------------------------------------------------------------
// Reading

struct Rd<'a> {
    b: &'a [u8],
}

impl Rd<'_> {
    fn u16(&self, i: usize) -> Option<u16> {
        Some(u16::from_le_bytes(self.b.get(i..i + 2)?.try_into().ok()?))
    }
    fn u32(&self, i: usize) -> Option<u32> {
        Some(u32::from_le_bytes(self.b.get(i..i + 4)?.try_into().ok()?))
    }
    fn f32(&self, i: usize) -> Option<f64> {
        Some(f32::from_le_bytes(self.b.get(i..i + 4)?.try_into().ok()?) as f64)
    }
    fn cstr(&self, i: usize) -> (String, usize) {
        let end = self.b[i.min(self.b.len())..].iter().position(|&c| c == 0).map_or(self.b.len(), |p| i + p);
        (String::from_utf8_lossy(&self.b[i.min(end)..end]).into_owned(), end + 1)
    }
    /// The sub-chunks of the chunk body `start..end`: (id, body start, end).
    fn chunks(&self, mut start: usize, end: usize) -> Vec<(u16, usize, usize)> {
        let mut out = Vec::new();
        let end = end.min(self.b.len());
        while start + 6 <= end {
            let (Some(id), Some(len)) = (self.u16(start), self.u32(start + 2)) else { break };
            let len = len as usize;
            if len < 6 || start + len > end {
                break;
            }
            out.push((id, start + 6, start + len));
            start += len;
        }
        out
    }
}

#[derive(Default)]
struct Mesh3ds {
    name: String,
    pts: Vec<DVec3>,
    uv: Vec<DVec2>,
    faces: Vec<([u16; 3], u16)>,
    groups: Vec<(String, Vec<u16>)>,
    smooth: Vec<u32>,
    matrix: DMat4,
}

#[derive(Default)]
struct Mat3ds {
    name: String,
    diffuse: [u8; 3],
    transparency: f64,
    two_sided: bool,
    map: Option<(String, [f64; 4])>,
}

struct Node3ds {
    id: u16,
    name: String,
    instance: String,
    parent: Option<u16>,
    pivot: DVec3,
    pos: DVec3,
    rot: DQuat,
    scl: DVec3,
}

fn read_color(r: &Rd, s: usize, e: usize) -> Option<[u8; 3]> {
    let mut best = None;
    for (id, a, _) in r.chunks(s, e) {
        match id {
            COLOR_24 | LIN_COLOR_24 if best.is_none() || id == COLOR_24 => {
                if let Some(c) = r.b.get(a..a + 3) {
                    best = Some([c[0], c[1], c[2]]);
                }
            }
            COLOR_F if best.is_none() => {
                let c = |k: usize| (r.f32(a + k * 4).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8;
                best = Some([c(0), c(1), c(2)]);
            }
            _ => {}
        }
    }
    best
}

fn read_percent(r: &Rd, s: usize, e: usize) -> Option<f64> {
    for (id, a, _) in r.chunks(s, e) {
        match id {
            INT_PERCENT => return Some(r.u16(a)? as f64 / 100.0),
            FLOAT_PERCENT => return Some(r.f32(a)? / 100.0),
            _ => {}
        }
    }
    None
}

/// What a 3DS file holds, plus the counts of the import report.
pub struct Read3ds {
    pub scene: SceneIn,
    pub materials: usize,
    pub nodes: usize,
    pub meshes: usize,
}

pub fn read(bytes: &[u8], file: Option<&Path>) -> Result<Read3ds> {
    let r = Rd { b: bytes };
    if r.u16(0) != Some(MAIN) {
        bail!("pas un fichier 3DS");
    }
    let end = (r.u32(2).unwrap_or(0) as usize).min(bytes.len());
    let mut meshes: Vec<Mesh3ds> = Vec::new();
    let mut mats: Vec<Mat3ds> = Vec::new();
    let mut nodes: Vec<Node3ds> = Vec::new();
    for (id, s, e) in r.chunks(6, end) {
        match id {
            EDIT => {
                for (id, s, e) in r.chunks(s, e) {
                    match id {
                        MATERIAL => mats.push(read_material(&r, s, e)),
                        NAMED_OBJECT => {
                            let (name, p) = r.cstr(s);
                            for (id, s2, e2) in r.chunks(p, e) {
                                if id == TRI_OBJECT {
                                    let mut m = read_mesh(&r, s2, e2);
                                    m.name = name.clone();
                                    meshes.push(m);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            KFDATA => {
                for (id, s, e) in r.chunks(s, e) {
                    if id == OBJECT_NODE
                        && let Some(n) = read_node(&r, s, e)
                    {
                        nodes.push(n);
                    }
                }
            }
            _ => {}
        }
    }
    if meshes.is_empty() {
        bail!("aucune maille dans le fichier 3DS");
    }
    let mut scene = SceneIn::default();
    for m in &mats {
        let texture = m.map.as_ref().and_then(|(name, _)| {
            let f = file?;
            let (n, bytes) = find_beside(f, name)?;
            texture_from_bytes(&n, bytes, None)
        });
        scene.materials.push(MatIn {
            name: m.name.clone(),
            color: m.diffuse,
            opacity: (1.0 - m.transparency).clamp(0.0, 1.0) as f32,
            texture,
        });
    }
    // One mesh per object, in the object's own frame (inverse of its mesh matrix).
    let mut mesh_of: HashMap<String, usize> = HashMap::new();
    for m in &meshes {
        let inv = m.matrix.inverse();
        let local = !nodes.is_empty();
        let pos: Vec<DVec3> = m.pts.iter().map(|p| if local { inv.transform_point3(*p) } else { *p }).collect();
        let mut polys = Vec::new();
        let mat_of: HashMap<u16, usize> = m
            .groups
            .iter()
            .filter_map(|(name, faces)| Some((mats.iter().position(|x| &x.name == name)?, faces)))
            .flat_map(|(mi, faces)| faces.iter().map(move |f| (*f, mi)))
            .collect();
        for (fi, (idx, flags)) in m.faces.iter().enumerate() {
            let mi = mat_of.get(&(fi as u16)).copied();
            let two = mi.is_some_and(|i| mats[i].two_sided);
            let uv = if m.uv.len() == m.pts.len() && mi.is_some_and(|i| scene.materials[i].texture.is_some()) {
                let [us, vs, uo, vo] = mi.and_then(|i| mats[i].map.as_ref()).map_or([1.0, 1.0, 0.0, 0.0], |x| x.1);
                // A face of a mesh without points (its corners set to 0) indexed an empty
                // list of coordinates and panicked.
                idx.iter()
                    .map(|&k| m.uv.get(k as usize).map(|t| DVec2::new(t.x * us + uo, t.y * vs + vo)))
                    .collect::<Option<Vec<_>>>()
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            polys.push(PolyIn {
                outer: idx.iter().map(|&k| k as u32).collect(),
                mat: mi.map(|i| i as u32),
                back: if two { mi.map(|i| i as u32) } else { None },
                uv,
                smooth: m.smooth.get(fi).copied().unwrap_or(0),
                // AB = bit 2, BC = bit 1, CA = bit 0.
                edge_vis: vec![flags & 4 != 0, flags & 2 != 0, flags & 1 != 0],
                ..Default::default()
            });
        }
        let i = scene.add_mesh(MeshIn { pos, polys, ..Default::default() });
        mesh_of.insert(m.name.clone(), i);
    }
    let nmeshes = meshes.len();
    if nodes.is_empty() {
        for m in &meshes {
            scene.root.children.push(NodeIn {
                name: m.name.clone(),
                meshes: vec![mesh_of[&m.name]],
                ..Default::default()
            });
        }
    } else {
        // Keyframer hierarchy, at frame 0. Objects used by several nodes become components.
        let mut uses: HashMap<&str, usize> = HashMap::new();
        for n in &nodes {
            *uses.entry(n.name.as_str()).or_default() += 1;
        }
        let mut def_of: HashMap<String, usize> = HashMap::new();
        let ids: HashSet<u16> = nodes.iter().map(|n| n.id).collect();
        // The children of each parent id, made once (each node looked through every node:
        // quadratic), and every node entered once: nodes sharing an id with their own parent
        // (or with each other) made the walk fan out exponentially down to the depth limit.
        let mut kids: HashMap<Option<u16>, Vec<usize>> = HashMap::new();
        for (i, n) in nodes.iter().enumerate() {
            let parent = n.parent.filter(|p| ids.contains(p));
            kids.entry(parent).or_default().push(i);
        }
        let mut seen = vec![false; nodes.len()];
        #[allow(clippy::too_many_arguments)]
        fn make(
            i: usize,
            nodes: &[Node3ds],
            kids: &HashMap<Option<u16>, Vec<usize>>,
            seen: &mut [bool],
            scene: &mut SceneIn,
            mesh_of: &HashMap<String, usize>,
            uses: &HashMap<&str, usize>,
            def_of: &mut HashMap<String, usize>,
            depth: usize,
        ) -> NodeIn {
            seen[i] = true;
            let n = &nodes[i];
            let m = DMat4::from_scale_rotation_translation(n.scl, n.rot, n.pos);
            let mut node = NodeIn {
                name: if n.instance.is_empty() { n.name.clone() } else { n.instance.clone() },
                xform: m,
                ..Default::default()
            };
            if let Some(&mesh) = mesh_of.get(&n.name) {
                let shifted = DMat4::from_translation(-n.pivot);
                let geo = NodeIn { xform: shifted, meshes: vec![mesh], kind: NodeKind::Flatten, ..Default::default() };
                if uses.get(n.name.as_str()).copied().unwrap_or(0) > 1 {
                    let d = *def_of.entry(n.name.clone()).or_insert_with(|| {
                        scene.defs.push(crate::build::DefIn {
                            name: n.name.clone(),
                            meshes: Vec::new(),
                            children: vec![geo.clone()],
                        });
                        scene.defs.len() - 1
                    });
                    node.children.push(NodeIn { def: Some(d), name: n.name.clone(), ..Default::default() });
                } else {
                    node.children.push(geo);
                }
            }
            if depth < 64 {
                for &c in kids.get(&Some(n.id)).map(Vec::as_slice).unwrap_or_default() {
                    if seen[c] {
                        continue;
                    }
                    // Children's tracks are relative to their parent.
                    let child = make(c, nodes, kids, seen, scene, mesh_of, uses, def_of, depth + 1);
                    node.children.push(child);
                }
            }
            node
        }
        for &i in kids.get(&None).map(Vec::as_slice).unwrap_or_default() {
            if seen[i] {
                continue;
            }
            let n = make(i, &nodes, &kids, &mut seen, &mut scene, &mesh_of, &uses, &mut def_of, 0);
            scene.root.children.push(n);
        }
        // Objects no node shows.
        let shown: HashSet<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        for m in &meshes {
            if !shown.contains(m.name.as_str()) {
                let node = NodeIn {
                    name: m.name.clone(),
                    xform: m.matrix,
                    meshes: vec![mesh_of[&m.name]],
                    ..Default::default()
                };
                scene.root.children.push(node);
            }
        }
    }
    Ok(Read3ds { materials: mats.len(), nodes: nodes.len(), meshes: nmeshes, scene })
}

fn read_material(r: &Rd, s: usize, e: usize) -> Mat3ds {
    let mut m = Mat3ds { diffuse: [192, 192, 192], ..Default::default() };
    for (id, a, b) in r.chunks(s, e) {
        match id {
            MAT_NAME => m.name = r.cstr(a).0,
            MAT_DIFFUSE => m.diffuse = read_color(r, a, b).unwrap_or(m.diffuse),
            MAT_TRANSPARENCY => m.transparency = read_percent(r, a, b).unwrap_or(0.0),
            MAT_TWO_SIDE => m.two_sided = true,
            MAT_TEXMAP => {
                let mut name = None;
                let mut t = [1.0, 1.0, 0.0, 0.0];
                for (id, a, _) in r.chunks(a, b) {
                    match id {
                        MAT_MAPNAME => name = Some(r.cstr(a).0),
                        MAT_MAP_USCALE => t[0] = r.f32(a).unwrap_or(1.0),
                        MAT_MAP_VSCALE => t[1] = r.f32(a).unwrap_or(1.0),
                        MAT_MAP_UOFFSET => t[2] = r.f32(a).unwrap_or(0.0),
                        MAT_MAP_VOFFSET => t[3] = r.f32(a).unwrap_or(0.0),
                        _ => {}
                    }
                }
                if let Some(n) = name {
                    m.map = Some((n, t));
                }
            }
            _ => {}
        }
    }
    m
}

fn read_mesh(r: &Rd, s: usize, e: usize) -> Mesh3ds {
    let mut m = Mesh3ds { matrix: DMat4::IDENTITY, ..Default::default() };
    for (id, a, b) in r.chunks(s, e) {
        match id {
            POINT_ARRAY => {
                let n = r.u16(a).unwrap_or(0) as usize;
                for i in 0..n {
                    let k = a + 2 + i * 12;
                    if let (Some(x), Some(y), Some(z)) = (r.f32(k), r.f32(k + 4), r.f32(k + 8)) {
                        m.pts.push(DVec3::new(x, y, z));
                    }
                }
            }
            TEX_VERTS => {
                let n = r.u16(a).unwrap_or(0) as usize;
                for i in 0..n {
                    let k = a + 2 + i * 8;
                    if let (Some(u), Some(v)) = (r.f32(k), r.f32(k + 4)) {
                        m.uv.push(DVec2::new(u, v));
                    }
                }
            }
            FACE_ARRAY => {
                let n = r.u16(a).unwrap_or(0) as usize;
                for i in 0..n {
                    let k = a + 2 + i * 8;
                    if let (Some(x), Some(y), Some(z), Some(f)) = (r.u16(k), r.u16(k + 2), r.u16(k + 4), r.u16(k + 6)) {
                        if [x, y, z].iter().all(|&v| (v as usize) < m.pts.len()) {
                            m.faces.push(([x, y, z], f));
                        } else {
                            m.faces.push(([0, 0, 0], 0));
                        }
                    }
                }
                for (id, c, _) in r.chunks(a + 2 + n * 8, b) {
                    match id {
                        MSH_MAT_GROUP => {
                            let (name, p) = r.cstr(c);
                            let k = r.u16(p).unwrap_or(0) as usize;
                            let list = (0..k).filter_map(|i| r.u16(p + 2 + i * 2)).collect();
                            m.groups.push((name, list));
                        }
                        SMOOTH_GROUP => {
                            m.smooth = (0..n).filter_map(|i| r.u32(c + i * 4)).collect();
                        }
                        _ => {}
                    }
                }
            }
            MESH_MATRIX => {
                let f: Vec<f64> = (0..12).filter_map(|i| r.f32(a + i * 4)).collect();
                if f.len() == 12 {
                    m.matrix = DMat4::from_cols_array(&[
                        f[0], f[1], f[2], 0.0, f[3], f[4], f[5], 0.0, f[6], f[7], f[8], 0.0, f[9], f[10], f[11], 1.0,
                    ]);
                    if m.matrix.determinant().abs() < 1e-12 {
                        m.matrix = DMat4::IDENTITY;
                    }
                }
            }
            _ => {}
        }
    }
    m
}

/// First key of a track: (frame, data offset after the key header).
fn first_key(r: &Rd, a: usize) -> Option<usize> {
    // flags u16, 8 unknown bytes, key count u32, then keys: frame u32, accel flags u16, then
    // one float per set bit of the low 5 bits.
    let n = r.u32(a + 10)?;
    if n == 0 {
        return None;
    }
    let k = a + 14;
    let flags = r.u16(k + 4)?;
    Some(k + 6 + (flags & 0x1F).count_ones() as usize * 4)
}

fn read_node(r: &Rd, s: usize, e: usize) -> Option<Node3ds> {
    let mut n = Node3ds {
        id: 0,
        name: String::new(),
        instance: String::new(),
        parent: None,
        pivot: DVec3::ZERO,
        pos: DVec3::ZERO,
        rot: DQuat::IDENTITY,
        scl: DVec3::ONE,
    };
    let mut has_id = false;
    let v3 = |k: usize| Some(DVec3::new(r.f32(k)?, r.f32(k + 4)?, r.f32(k + 8)?));
    for (id, a, _) in r.chunks(s, e) {
        match id {
            NODE_ID => {
                n.id = r.u16(a)?;
                has_id = true;
            }
            NODE_HDR => {
                let (name, p) = r.cstr(a);
                n.name = name;
                let parent = r.u16(p + 4)?;
                n.parent = (parent != 0xFFFF).then_some(parent);
            }
            INSTANCE_NAME => n.instance = r.cstr(a).0,
            PIVOT => n.pivot = v3(a)?,
            POS_TRACK => {
                if let Some(k) = first_key(r, a) {
                    n.pos = v3(k)?;
                }
            }
            ROT_TRACK => {
                if let Some(k) = first_key(r, a) {
                    let angle = r.f32(k)?;
                    let axis = v3(k + 4)?;
                    if axis.length() > 1e-12 {
                        n.rot = DQuat::from_axis_angle(axis.normalize(), -angle);
                    }
                }
            }
            SCL_TRACK => {
                if let Some(k) = first_key(r, a) {
                    n.scl = v3(k)?;
                }
            }
            _ => {}
        }
    }
    if n.name == "$$$DUMMY" || n.name.is_empty() {
        // Dummy nodes only group their children.
        n.name = format!("$$$DUMMY{}", n.id);
    }
    has_id.then_some(n)
}

/// File › Import of a 3DS file.
pub fn import(
    bytes: &[u8],
    file: Option<&Path>,
    o: &ThreeDsImport,
    model_units: esq_core::model::Units,
) -> Result<(Model, String)> {
    let r = read(bytes, file)?;
    let model = crate::build::build(
        &r.scene,
        BuildOpts { scale: o.units.mm(model_units), merge_coplanar: o.merge_coplanar, ..Default::default() },
    );
    let report = format!(
        "Entités 3ds importées\n\tMatières:\t{}\n\tNœuds:\t{}\n\tMeshes:\t{}\n",
        r.materials, r.nodes, r.meshes
    );
    Ok((model, report))
}

// ------------------------------------------------------------------------------------------
// Writing

struct W {
    b: Vec<u8>,
    stack: Vec<usize>,
}

impl W {
    fn new() -> Self {
        W { b: Vec::new(), stack: Vec::new() }
    }
    fn open(&mut self, id: u16) {
        self.stack.push(self.b.len());
        self.b.extend_from_slice(&id.to_le_bytes());
        self.b.extend_from_slice(&[0, 0, 0, 0]);
    }
    fn close(&mut self) {
        let s = self.stack.pop().unwrap();
        let len = (self.b.len() - s) as u32;
        self.b[s + 2..s + 6].copy_from_slice(&len.to_le_bytes());
    }
    fn u16(&mut self, v: u16) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f64) {
        self.b.extend_from_slice(&(v as f32).to_le_bytes());
    }
    fn cstr(&mut self, s: &str) {
        self.b.extend_from_slice(s.as_bytes());
        self.b.push(0);
    }
    fn chunk(&mut self, id: u16, f: impl FnOnce(&mut W)) {
        self.open(id);
        f(self);
        self.close();
    }
    fn color(&mut self, id: u16, c: [u8; 3]) {
        self.chunk(id, |w| {
            w.chunk(COLOR_24, |w| w.b.extend_from_slice(&c));
        });
    }
}

/// A 3DS name: ASCII, at most `max` characters, unique among `taken`.
fn short_name(s: &str, max: usize, taken: &mut HashSet<String>) -> String {
    let clean: String = s.chars().filter(|c| c.is_ascii_graphic()).take(max).collect();
    let clean = if clean.is_empty() { "Object".chars().take(max).collect() } else { clean };
    let mut name = clean.clone();
    let mut i = 1;
    while taken.contains(&name.to_ascii_lowercase()) {
        let suffix = i.to_string();
        name = format!("{}{}", clean.chars().take(max - suffix.len()).collect::<String>(), suffix);
        i += 1;
    }
    taken.insert(name.to_ascii_lowercase());
    name
}

/// A triangle mesh being assembled for one 3DS object.
#[derive(Default)]
struct OutMesh {
    name: String,
    parent: Option<usize>,
    pts: Vec<DVec3>,
    uv: Vec<DVec2>,
    faces: Vec<([u32; 3], u16, usize, u32)>,
    welded: HashMap<([u64; 3], [u64; 2]), u32>,
}

impl OutMesh {
    fn vertex(&mut self, p: DVec3, uv: DVec2, weld: bool) -> u32 {
        let key = (
            [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()],
            if weld { [0, 0] } else { [uv.x.to_bits(), uv.y.to_bits()] },
        );
        if let Some(&i) = self.welded.get(&key) {
            return i;
        }
        self.pts.push(p);
        self.uv.push(uv);
        let i = self.pts.len() as u32 - 1;
        self.welded.insert(key, i);
        i
    }
}

/// File › Export › 3D Model › 3DS.
pub fn export(model: &Model, path: &Path, o: &ThreeDsExport) -> Result<String> {
    let k = o.units.mm(model.units);
    let mut cache = Cache::new(model, Opts::default());
    let processed = crate::report::processed(&mut cache, if o.cameras { model.scenes.len() } else { 0 });
    let placed = placements(&mut cache);
    // Materials: (material, front?) → 3DS material index.
    let mut mat_list: Vec<(Option<MaterialId>, bool)> = Vec::new();
    let mut mat_index = |m: Option<MaterialId>, front: bool| -> usize {
        let key = (m, m.is_some() || front);
        match mat_list.iter().position(|x| *x == key) {
            Some(i) => i,
            None => {
                mat_list.push(key);
                mat_list.len() - 1
            }
        }
    };
    let weld = o.favor == Favor3ds::WeldVertices;
    let mut meshes: Vec<OutMesh> = Vec::new();
    let mut key_of: HashMap<String, usize> = HashMap::new();
    let mut taken = HashSet::new();
    // Object of the placed collection `pi` (by hierarchy mode).
    let mut mesh_for = |key: String, name: &str, parent: Option<usize>, meshes: &mut Vec<OutMesh>| -> usize {
        if let Some(&i) = key_of.get(&key) {
            return i;
        }
        meshes.push(OutMesh { name: short_name(name, 10, &mut taken), parent, ..Default::default() });
        key_of.insert(key, meshes.len() - 1);
        meshes.len() - 1
    };
    let mut placed_mesh: HashMap<usize, usize> = HashMap::new();
    let mut edges_out = 0;
    for (pi, p) in placed.iter().enumerate() {
        let coll = cache.get(p.def).clone();
        let (inst_name, parent_pi) = {
            let mut ents = &model.root;
            let mut name = "SketchUp".to_string();
            for id in &p.path {
                if let Some(i) = ents.instances.get(*id) {
                    let d = &model.defs[i.def];
                    name = if i.name.is_empty() { d.name.clone() } else { i.name.clone() };
                    ents = &d.entities;
                }
            }
            let parent = placed.iter().position(|q| q.path.len() + 1 == p.path.len() && p.path.starts_with(&q.path));
            (name, parent)
        };
        let sides: &[bool] =
            if o.two_sided && o.two_sided_as == TwoSidedAs::Geometry { &[true, false] } else { &[true] };
        // Every front first, then the backs (importers keep the first of two coincident faces).
        for &front in sides {
            for face in &coll.faces {
                let wf = WorldFace {
                    face,
                    xform: p.xform,
                    front: face.front.or(p.inherited),
                    back: face.back.or(p.inherited),
                };
                let pts: Vec<DVec3> = wf.points().iter().map(|x| *x / k).collect();
                let mat = if front { wf.front } else { wf.back };
                let mi = mat_index(mat, front);
                let layer: LayerIdx = if face.layer != 0 { face.layer } else { p.top_layer };
                let mesh = match o.hierarchy {
                    Hierarchy3ds::FullHierarchy => {
                        let parent = parent_pi.and_then(|q| placed_mesh.get(&q).copied());
                        let m = mesh_for(format!("p{pi}"), &inst_name, parent, &mut meshes);
                        placed_mesh.insert(pi, m);
                        m
                    }
                    Hierarchy3ds::ByTag => {
                        let name = model.layers.get(layer).map_or("Layer0".to_string(), |l| l.name.clone());
                        mesh_for(format!("l{layer}"), &name, None, &mut meshes)
                    }
                    Hierarchy3ds::ByMaterial => {
                        let name = mat
                            .and_then(|m| model.materials.get(m))
                            .map_or("FrontColor".to_string(), |m| m.name.clone());
                        mesh_for(format!("m{mi}"), &name, None, &mut meshes)
                    }
                    Hierarchy3ds::SingleObject => mesh_for("all".into(), "SketchUp", None, &mut meshes),
                };
                let uvs = if o.texture_maps { face.uvs(model, mat, front) } else { None };
                let boundary: HashSet<(usize, usize)> = wf
                    .loops()
                    .iter()
                    .flat_map(|l| {
                        (0..l.len()).map(move |i| (l[i].min(l[(i + 1) % l.len()]), l[i].max(l[(i + 1) % l.len()])))
                    })
                    .collect();
                for t in wf.tris() {
                    let t = if front { t } else { [t[0], t[2], t[1]] };
                    // Split objects at 65 535 vertices.
                    let m = &mut meshes[mesh];
                    let mut idx = [0u32; 3];
                    for (j, &c) in t.iter().enumerate() {
                        let uv = uvs.as_ref().map_or(DVec2::ZERO, |u| u[c]);
                        idx[j] = m.vertex(pts[c], uv, weld);
                    }
                    let vis = |a: usize, b: usize| boundary.contains(&(a.min(b), a.max(b)));
                    let flags = (vis(t[0], t[1]) as u16) << 2 | (vis(t[1], t[2]) as u16) << 1 | vis(t[2], t[0]) as u16;
                    let group = if face.smooth == 0 { 0 } else { 1u32 << ((face.smooth - 1) % 32) };
                    m.faces.push((idx, flags, mi, group));
                }
            }
        }
        if o.standalone_edges {
            for e in coll.edges.iter().filter(|e| e.standalone) {
                let mesh = match o.hierarchy {
                    Hierarchy3ds::FullHierarchy => {
                        let parent = parent_pi.and_then(|q| placed_mesh.get(&q).copied());
                        let m = mesh_for(format!("p{pi}"), &inst_name, parent, &mut meshes);
                        placed_mesh.insert(pi, m);
                        m
                    }
                    Hierarchy3ds::ByTag => {
                        let layer = if e.layer != 0 { e.layer } else { p.top_layer };
                        let name = model.layers.get(layer).map_or("Layer0".to_string(), |l| l.name.clone());
                        mesh_for(format!("l{layer}"), &name, None, &mut meshes)
                    }
                    _ => mesh_for("edges".into(), "Edges", None, &mut meshes),
                };
                let mi = mat_index(None, true);
                let m = &mut meshes[mesh];
                let a = m.vertex(p.xform.transform_point3(e.a) / k, DVec2::ZERO, true);
                let b = m.vertex(p.xform.transform_point3(e.b) / k, DVec2::ZERO, true);
                // A degenerate triangle whose only visible side is the edge.
                m.faces.push(([a, b, b], 4, mi, 0));
                edges_out += 1;
            }
        }
    }
    // Split meshes that exceed the 3DS limits.
    let meshes = split_large(meshes);
    // Materials.
    let mut w = W::new();
    w.open(MAIN);
    w.chunk(VERSION, |w| w.u32(3));
    w.open(EDIT);
    w.chunk(MESH_VERSION, |w| w.u32(3));
    let mut mtaken = HashSet::new();
    let mut texnames = TexNames::new();
    let mut textures = 0;
    let mat_names: Vec<String> = mat_list
        .iter()
        .map(|(m, front)| {
            let n = match m.and_then(|m| model.materials.get(m)) {
                Some(x) => x.name.clone(),
                None if *front => "FrontColor".into(),
                None => "BackColor".into(),
            };
            short_name(&n, 16, &mut mtaken)
        })
        .collect();
    let dir = path.parent().unwrap_or(Path::new("."));
    for ((m, front), name) in mat_list.iter().zip(&mat_names) {
        let mat = m.and_then(|m| model.materials.get(m));
        let (c, opacity) = match mat {
            Some(x) => (x.color, x.opacity as f64),
            None if *front => (FRONT_COLOR, 1.0),
            None => (BACK_COLOR, 1.0),
        };
        w.open(MATERIAL);
        w.chunk(MAT_NAME, |w| w.cstr(name));
        w.color(MAT_AMBIENT, c);
        w.color(MAT_DIFFUSE, c);
        w.color(MAT_SPECULAR, [255, 255, 255]);
        w.chunk(MAT_TRANSPARENCY, |w| w.chunk(INT_PERCENT, |w| w.u16(((1.0 - opacity) * 100.0).round() as u16)));
        if o.two_sided && o.two_sided_as == TwoSidedAs::Materials {
            w.chunk(MAT_TWO_SIDE, |_| {});
        }
        if o.texture_maps
            && let Some(t) = mat.and_then(|x| x.ext.texture.as_ref())
        {
            let file = texnames.name_for(t);
            std::fs::write(dir.join(&file), &t.image.0[..])?;
            textures += 1;
            w.chunk(MAT_TEXMAP, |w| {
                w.chunk(INT_PERCENT, |w| w.u16(100));
                w.chunk(MAT_MAPNAME, |w| w.cstr(&file));
            });
        }
        w.close();
    }
    w.chunk(MASTER_SCALE, |w| w.f32(1.0));
    let has_uv = o.texture_maps
        && mat_list
            .iter()
            .any(|(m, _)| m.and_then(|m| model.materials.get(m)).is_some_and(|x| x.ext.texture.is_some()));
    let mut tris_out = 0;
    for m in &meshes {
        w.open(NAMED_OBJECT);
        w.cstr(&m.name);
        w.open(TRI_OBJECT);
        w.chunk(POINT_ARRAY, |w| {
            w.u16(m.pts.len() as u16);
            for p in &m.pts {
                w.f32(p.x);
                w.f32(p.y);
                w.f32(p.z);
            }
        });
        if has_uv {
            w.chunk(TEX_VERTS, |w| {
                w.u16(m.uv.len() as u16);
                for t in &m.uv {
                    w.f32(t.x);
                    w.f32(t.y);
                }
            });
        }
        w.chunk(MESH_MATRIX, |w| {
            for v in [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0] {
                w.f32(v);
            }
        });
        w.open(FACE_ARRAY);
        w.u16(m.faces.len() as u16);
        for (idx, flags, _, _) in &m.faces {
            for v in idx {
                w.u16(*v as u16);
            }
            w.u16(*flags);
        }
        tris_out += m.faces.iter().filter(|f| f.0[1] != f.0[2]).count();
        let mut used: Vec<usize> = m.faces.iter().map(|f| f.2).collect();
        used.sort();
        used.dedup();
        for mi in used {
            w.chunk(MSH_MAT_GROUP, |w| {
                w.cstr(&mat_names[mi]);
                let list: Vec<u16> =
                    m.faces.iter().enumerate().filter(|(_, f)| f.2 == mi).map(|(i, _)| i as u16).collect();
                w.u16(list.len() as u16);
                for i in list {
                    w.u16(i);
                }
            });
        }
        w.chunk(SMOOTH_GROUP, |w| {
            for f in &m.faces {
                w.u32(f.3);
            }
        });
        w.close();
        w.close();
        w.close();
    }
    let mut cameras = 0;
    if o.cameras {
        let mut ctaken: HashSet<String> = meshes.iter().map(|m| m.name.to_ascii_lowercase()).collect();
        for s in &model.scenes {
            let c = &s.camera;
            w.open(NAMED_OBJECT);
            w.cstr(&short_name(&s.name, 10, &mut ctaken));
            w.chunk(N_CAMERA, |w| {
                for v in [c.eye / k, c.target / k] {
                    w.f32(v.x);
                    w.f32(v.y);
                    w.f32(v.z);
                }
                w.f32(0.0);
                w.f32(2400.0 / c.fov_deg.max(1.0));
            });
            w.close();
            cameras += 1;
        }
    }
    w.close();
    // Keyframer: the object hierarchy.
    w.open(KFDATA);
    w.chunk(KFHDR, |w| {
        w.u16(5);
        w.cstr("MAXSCENE");
        w.u32(100);
    });
    w.chunk(KFSEG, |w| {
        w.u32(0);
        w.u32(100);
    });
    for (i, m) in meshes.iter().enumerate() {
        w.open(OBJECT_NODE);
        w.chunk(NODE_ID, |w| w.u16(i as u16));
        w.chunk(NODE_HDR, |w| {
            w.cstr(&m.name);
            w.u16(0);
            w.u16(0);
            w.u16(m.parent.map_or(0xFFFF, |p| p as u16));
        });
        w.chunk(PIVOT, |w| {
            w.f32(0.0);
            w.f32(0.0);
            w.f32(0.0);
        });
        for (id, vals) in
            [(POS_TRACK, vec![0.0, 0.0, 0.0]), (ROT_TRACK, vec![0.0, 0.0, 0.0, 0.0]), (SCL_TRACK, vec![1.0, 1.0, 1.0])]
        {
            w.chunk(id, |w| {
                w.u16(0);
                w.u32(0);
                w.u32(0);
                w.u32(1);
                w.u32(0);
                w.u16(0);
                for v in vals {
                    w.f32(v);
                }
            });
        }
        w.close();
    }
    w.close();
    w.close();
    std::fs::write(path, &w.b)?;
    Ok(crate::report::text(
        "3DS",
        &processed,
        &[
            ("Triangles", tris_out),
            ("arêtes", edges_out),
            ("Caméras", cameras),
            ("Matières", mat_list.len()),
            ("Textures", textures),
        ],
    ))
}

/// 3DS objects hold at most 65 535 vertices and faces: larger ones are cut in pieces.
fn split_large(meshes: Vec<OutMesh>) -> Vec<OutMesh> {
    const MAX: usize = 65_535;
    let mut out = Vec::new();
    for m in meshes {
        if m.pts.len() <= MAX && m.faces.len() <= MAX {
            out.push(m);
            continue;
        }
        let mut part = OutMesh { name: m.name.clone(), parent: m.parent, ..Default::default() };
        let mut remap: HashMap<u32, u32> = HashMap::new();
        let mut n = 0;
        for f in &m.faces {
            if part.pts.len() + 3 > MAX || part.faces.len() + 1 > MAX {
                n += 1;
                let name: String = format!("{}{}", m.name.chars().take(8).collect::<String>(), n);
                out.push(std::mem::replace(&mut part, OutMesh { name, parent: m.parent, ..Default::default() }));
                remap.clear();
            }
            let mut idx = [0u32; 3];
            for (j, v) in f.0.iter().enumerate() {
                idx[j] = *remap.entry(*v).or_insert_with(|| {
                    part.pts.push(m.pts[*v as usize]);
                    part.uv.push(m.uv[*v as usize]);
                    part.pts.len() as u32 - 1
                });
            }
            part.faces.push((idx, f.1, f.2, f.3));
        }
        out.push(part);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::model::{Definition, Instance, Units};

    /// A triangle object `o` (three points unless `points` is false), with material `M`
    /// mapped to `t.png`.
    fn object(w: &mut W, points: bool) {
        w.chunk(MATERIAL, |w| {
            w.chunk(MAT_NAME, |w| w.cstr("M"));
            w.chunk(MAT_TEXMAP, |w| w.chunk(MAT_MAPNAME, |w| w.cstr("t.png")));
        });
        w.chunk(NAMED_OBJECT, |w| {
            w.cstr("o");
            w.chunk(TRI_OBJECT, |w| {
                if points {
                    w.chunk(POINT_ARRAY, |w| {
                        w.u16(3);
                        for v in [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
                            w.f32(v);
                        }
                    });
                }
                w.chunk(FACE_ARRAY, |w| {
                    w.u16(1);
                    for x in [0, 1, 2, 0] {
                        w.u16(x);
                    }
                    w.chunk(MSH_MAT_GROUP, |w| {
                        w.cstr("M");
                        w.u16(1);
                        w.u16(0);
                    });
                });
            });
        });
    }

    fn dir_with_texture(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("esq_3ds_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 2, image::Rgb([1, 2, 3])))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(dir.join("t.png"), png).unwrap();
        dir
    }

    /// A textured face in an object without points: its corners fall back to 0 and the
    /// texture coordinates (none) were indexed at 0: a panic.
    #[test]
    fn a_textured_face_without_points_does_not_panic() {
        let mut w = W::new();
        w.chunk(MAIN, |w| w.chunk(EDIT, |w| object(w, false)));
        let dir = dir_with_texture("sans_points");
        let _ = read(&w.b, Some(&dir.join("o.3ds")));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Forty keyframer nodes with the same id as their parent: each node listed all of them
    /// as children again, down to 64 levels (40^64 visits). Each node is entered once.
    #[test]
    fn keyframer_nodes_naming_themselves_as_parent_end() {
        let mut w = W::new();
        w.chunk(MAIN, |w| {
            w.chunk(EDIT, |w| object(w, true));
            w.chunk(KFDATA, |w| {
                for k in 0..41 {
                    w.chunk(OBJECT_NODE, |w| {
                        w.chunk(NODE_ID, |w| w.u16(1));
                        w.chunk(NODE_HDR, |w| {
                            w.cstr("o");
                            w.u16(0);
                            w.u16(0);
                            w.u16(if k == 0 { 0xFFFF } else { 1 });
                        });
                    });
                }
            });
        });
        let (tx, rx) = std::sync::mpsc::channel();
        let bytes = w.b.clone();
        std::thread::spawn(move || {
            let _ = tx.send(read(&bytes, None).map(|r| r.nodes));
        });
        let n = rx.recv_timeout(std::time::Duration::from_secs(20)).expect("the reading ends").unwrap();
        assert_eq!(n, 41);
    }

    fn cube(e: &mut esq_core::model::Entities, s: f64) {
        let p = [DVec3::new(0.0, 0.0, 0.0), DVec3::new(s, 0.0, 0.0), DVec3::new(s, s, 0.0), DVec3::new(0.0, s, 0.0)];
        let v: Vec<_> = p.iter().map(|x| e.add_vertex_raw(*x)).collect();
        let top: Vec<_> = p.iter().map(|x| e.add_vertex_raw(*x + DVec3::Z * s)).collect();
        e.add_face_raw(vec![v[0], v[3], v[2], v[1]], vec![]);
        e.add_face_raw(top.clone(), vec![]);
        for i in 0..4 {
            let j = (i + 1) % 4;
            e.add_face_raw(vec![v[i], v[j], top[j], top[i]], vec![]);
        }
    }

    fn model() -> Model {
        let mut m = Model::new();
        m.units = Units::Millimeters;
        cube(&mut m.root, 100.0);
        let mut inner = esq_core::model::Entities::default();
        cube(&mut inner, 50.0);
        let d = m.defs.insert(Definition {
            name: "Petit cube".into(),
            is_group: false,
            description: String::new(),
            entities: inner,
            attributes: Default::default(),
            ext: Default::default(),
        });
        let red = m.add_material("Rouge", [255, 0, 0]);
        m.root.instances.insert(Instance {
            def: d,
            xform: DMat4::from_translation(DVec3::new(500.0, 0.0, 0.0)),
            name: String::new(),
            material: Some(red),
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
        m
    }

    #[test]
    fn export_then_import_keeps_geometry_hierarchy_and_materials() {
        let dir = std::env::temp_dir().join(format!("esq_3ds_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("cubes.3ds");
        let m = model();
        let report =
            export(&m, &p, &ThreeDsExport { units: crate::units::LenUnit::Millimeters, ..Default::default() }).unwrap();
        assert!(report.contains("24 Triangles"), "{report}");
        let bytes = std::fs::read(&p).unwrap();
        assert_eq!(&bytes[..2], &[0x4D, 0x4D]);
        let r = read(&bytes, Some(&p)).unwrap();
        assert_eq!(r.meshes, 2);
        assert_eq!(r.nodes, 2);
        assert!(r.scene.materials.iter().any(|x| x.name == "Rouge" && x.color == [255, 0, 0]));
        let (back, rep) = import(
            &bytes,
            Some(&p),
            &ThreeDsImport { merge_coplanar: true, units: crate::units::LenUnit::Millimeters },
            Units::Millimeters,
        )
        .unwrap();
        assert!(rep.contains("Meshes:\t2"));
        // Two groups, six faces each once the triangles are merged; the hidden diagonals go.
        let mut faces = Vec::new();
        esq_core::walk::visit(&back, |pl| {
            if !pl.ents.faces.is_empty() {
                faces
                    .push((pl.ents.faces.len(), pl.xform.transform_point3(pl.ents.verts.values().next().unwrap().pos)));
            }
        });
        assert_eq!(faces.len(), 2, "{faces:?}");
        assert!(faces.iter().all(|f| f.0 == 6));
        let maxx = {
            let mut mx = f64::MIN;
            esq_core::walk::visit(&back, |pl| {
                for v in pl.ents.verts.values() {
                    mx = mx.max(pl.xform.transform_point3(v.pos).x);
                }
            });
            mx
        };
        assert!((maxx - 550.0).abs() < 1e-3, "{maxx}");
        // Without merging, quads keep their diagonal, hidden.
        let (raw, _) = import(
            &bytes,
            Some(&p),
            &ThreeDsImport { merge_coplanar: false, units: crate::units::LenUnit::Millimeters },
            Units::Millimeters,
        )
        .unwrap();
        let mut hidden = 0;
        esq_core::walk::visit(&raw, |pl| hidden += pl.ents.edges.values().filter(|e| e.hidden).count());
        assert_eq!(hidden, 12);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn modes_two_sided_and_cameras() {
        let dir = std::env::temp_dir().join(format!("esq_3ds2_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.3ds");
        let mut m = model();
        m.scenes.push(esq_core::model::Scene {
            name: "Vue de face très longue".into(),
            description: String::new(),
            camera: esq_core::model::SceneCamera {
                eye: DVec3::new(0.0, -1000.0, 500.0),
                target: DVec3::ZERO,
                up: DVec3::Z,
                fov_deg: 35.0,
                perspective: true,
            },
            hidden_layers: None,
            photo: None,
            ext: Default::default(),
        });
        let o = ThreeDsExport {
            hierarchy: Hierarchy3ds::SingleObject,
            two_sided: true,
            two_sided_as: TwoSidedAs::Geometry,
            ..Default::default()
        };
        let rep = export(&m, &p, &o).unwrap();
        assert!(rep.contains("48 Triangles") && rep.contains("1 Caméras"), "{rep}");
        let r = read(&std::fs::read(&p).unwrap(), None).unwrap();
        assert_eq!(r.meshes, 1);
        let o = ThreeDsExport { hierarchy: Hierarchy3ds::ByMaterial, ..Default::default() };
        export(&m, &p, &o).unwrap();
        let r = read(&std::fs::read(&p).unwrap(), None).unwrap();
        assert_eq!(r.meshes, 2);
        std::fs::remove_dir_all(&dir).ok();
    }
}
