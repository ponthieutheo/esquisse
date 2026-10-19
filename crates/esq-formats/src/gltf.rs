//! glTF 2.0: the binary .glb SketchUp exports (SimLab exporter: Y up, metres, node tree,
//! PBR materials with embedded textures) and the glTF importer (.glb and .gltf with its
//! buffers and images, triangles, strips, fans and lines, instancing, base colour textures).

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use esq_core::glam::{DMat4, DQuat, DVec2, DVec3};
use esq_core::model::{DefId, MaterialId, Model};
use serde_json::{Value as J, json};

use crate::build::{BuildOpts, DefIn, LineIn, MatIn, MeshIn, NodeIn, PolyIn, SceneIn};
use crate::obj::{BACK_COLOR, FRONT_COLOR};
use crate::options::GlbImport;
use crate::scene::{Cache, Collection, Opts, WorldFace};

// ------------------------------------------------------------------------------------------
// Colours

fn to_linear(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(c: f64) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

// ------------------------------------------------------------------------------------------
// Writing

/// Binary data of the file, with the views and accessors that describe it.
struct Bin {
    data: Vec<u8>,
    views: Vec<J>,
    accessors: Vec<J>,
}

impl Bin {
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.data.len().is_multiple_of(4) {
            self.data.push(0);
        }
        let mut v = json!({"buffer": 0, "byteOffset": self.data.len(), "byteLength": bytes.len()});
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        self.data.extend_from_slice(bytes);
        self.views.push(v);
        self.views.len() - 1
    }

    fn floats(&mut self, v: &[f32], kind: &str, n: usize, minmax: bool) -> usize {
        let bytes: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(34962));
        let mut a = json!({"bufferView": view, "componentType": 5126, "count": v.len() / n, "type": kind});
        if minmax && !v.is_empty() {
            let mut lo = vec![f32::MAX; n];
            let mut hi = vec![f32::MIN; n];
            for c in v.chunks(n) {
                for k in 0..n {
                    lo[k] = lo[k].min(c[k]);
                    hi[k] = hi[k].max(c[k]);
                }
            }
            a["min"] = json!(lo);
            a["max"] = json!(hi);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    fn indices(&mut self, v: &[u32]) -> usize {
        let bytes: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(34963));
        self.accessors.push(json!({"bufferView": view, "componentType": 5125, "count": v.len(), "type": "SCALAR"}));
        self.accessors.len() - 1
    }
}

struct Writer<'a> {
    model: &'a Model,
    bin: Bin,
    nodes: Vec<J>,
    meshes: Vec<J>,
    materials: Vec<J>,
    textures: Vec<J>,
    images: Vec<J>,
    mat_index: HashMap<(Option<MaterialId>, bool), usize>,
    image_index: HashMap<String, usize>,
    mesh_index: HashMap<(Option<DefId>, Option<MaterialId>), Option<usize>>,
    /// Z up, millimetres → Y up, metres.
    conv: DMat4,
}

impl Writer<'_> {
    fn material(&mut self, mat: Option<MaterialId>, front: bool) -> usize {
        let key = (mat, mat.is_some() || front);
        if let Some(&i) = self.mat_index.get(&key) {
            return i;
        }
        let m = mat.and_then(|m| self.model.materials.get(m));
        let (c, a) = match m {
            Some(m) => (m.color, m.opacity as f64),
            None if front => (FRONT_COLOR, 1.0),
            None => (BACK_COLOR, 1.0),
        };
        let name = m.map_or(if front { "FrontColor" } else { "BackColor" }, |m| m.name.as_str());
        let mut pbr = json!({
            "baseColorFactor": [to_linear(c[0]), to_linear(c[1]), to_linear(c[2]), a],
            "metallicFactor": 0.0,
            "roughnessFactor": 1.0,
        });
        if let Some(m) = m
            && m.ext.workflow == 1
        {
            pbr["metallicFactor"] = json!(m.ext.pbr.metallic_factor);
            pbr["roughnessFactor"] = json!(m.ext.pbr.roughness_factor);
        }
        if let Some(t) = m.and_then(|m| m.ext.texture.as_ref()) {
            let img = match self.image_index.get(&t.filename) {
                Some(&i) => i,
                None => {
                    let view = self.bin.view(&t.image.0, None);
                    let mime = match crate::tex::image_ext(&t.image.0) {
                        "jpg" => "image/jpeg",
                        _ => "image/png",
                    };
                    // Only PNG and JPEG are allowed in glTF: other images are converted.
                    let view = if matches!(crate::tex::image_ext(&t.image.0), "jpg" | "png") {
                        view
                    } else {
                        let png = image::load_from_memory(&t.image.0).ok().and_then(|i| {
                            let mut out = Vec::new();
                            i.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok().map(|_| out)
                        });
                        png.map_or(view, |p| self.bin.view(&p, None))
                    };
                    self.images.push(json!({"bufferView": view, "mimeType": mime, "name": t.filename}));
                    self.image_index.insert(t.filename.clone(), self.images.len() - 1);
                    self.images.len() - 1
                }
            };
            self.textures.push(json!({"sampler": 0, "source": img}));
            pbr["baseColorTexture"] = json!({"index": self.textures.len() - 1});
            pbr["baseColorFactor"] = json!([1.0, 1.0, 1.0, a]);
        }
        let mut mj = json!({"name": name, "pbrMetallicRoughness": pbr, "doubleSided": false});
        if a < 1.0 {
            mj["alphaMode"] = json!("BLEND");
        }
        self.materials.push(mj);
        self.mat_index.insert(key, self.materials.len() - 1);
        self.materials.len() - 1
    }

    fn mesh(&mut self, coll: &Collection, inherited: Option<MaterialId>, name: &str) -> Option<usize> {
        if coll.faces.is_empty() {
            return None;
        }
        // material → (positions, normals, uvs, indices)
        type Prim = (Vec<f32>, Vec<f32>, Vec<f32>, Vec<u32>, bool);
        let mut prims: Vec<(usize, Prim)> = Vec::new();
        // Every front first, then the backs (importers keep the first of two coincident faces).
        for front in [true, false] {
            for face in &coll.faces {
                let wf = WorldFace {
                    face,
                    xform: DMat4::IDENTITY,
                    front: face.front.or(inherited),
                    back: face.back.or(inherited),
                };
                let mat = if front { wf.front } else { wf.back };
                let mi = self.material(mat, front);
                let uvs = face.uvs(self.model, mat, front);
                let n = self.conv.transform_vector3(if front { face.normal } else { -face.normal }).normalize_or_zero();
                let pts: Vec<DVec3> = face.points().iter().map(|p| self.conv.transform_point3(*p)).collect();
                let k = match prims.iter().position(|p| p.0 == mi) {
                    Some(k) => k,
                    None => {
                        prims.push((mi, (Vec::new(), Vec::new(), Vec::new(), Vec::new(), uvs.is_some())));
                        prims.len() - 1
                    }
                };
                let p = &mut prims[k].1;
                let base = (p.0.len() / 3) as u32;
                for (i, q) in pts.iter().enumerate() {
                    p.0.extend([q.x as f32, q.y as f32, q.z as f32]);
                    p.1.extend([n.x as f32, n.y as f32, n.z as f32]);
                    let t = uvs.as_ref().map_or(DVec2::ZERO, |u| u[i]);
                    // glTF's v goes down the image.
                    p.2.extend([t.x as f32, (1.0 - t.y) as f32]);
                }
                for t in wf.tris() {
                    let t = if front { t } else { [t[0], t[2], t[1]] };
                    p.3.extend(t.iter().map(|i| base + *i as u32));
                }
            }
        }
        let mut primitives = Vec::new();
        for (mi, (pos, nor, uv, idx, textured)) in prims {
            let pa = self.bin.floats(&pos, "VEC3", 3, true);
            let na = self.bin.floats(&nor, "VEC3", 3, false);
            let mut attrs = json!({"POSITION": pa, "NORMAL": na});
            if textured {
                attrs["TEXCOORD_0"] = json!(self.bin.floats(&uv, "VEC2", 2, false));
            }
            let ia = self.bin.indices(&idx);
            primitives.push(json!({"attributes": attrs, "indices": ia, "material": mi, "mode": 4}));
        }
        self.meshes.push(json!({"name": name, "primitives": primitives}));
        Some(self.meshes.len() - 1)
    }

    /// A node for a collection placed by `xform`; returns its index.
    fn node(
        &mut self,
        cache: &mut Cache,
        def: Option<DefId>,
        xform: &DMat4,
        inherited: Option<MaterialId>,
        name: &str,
        depth: usize,
    ) -> usize {
        let coll = cache.get(def).clone();
        let mesh = match self.mesh_index.get(&(def, inherited)) {
            Some(m) => *m,
            None => {
                let m = self.mesh(&coll, inherited, name);
                self.mesh_index.insert((def, inherited), m);
                m
            }
        };
        let mut children = Vec::new();
        if depth < 64 {
            for inst in &coll.instances {
                let d = &self.model.defs[inst.def];
                let n = if inst.name.is_empty() { d.name.clone() } else { inst.name.clone() };
                children.push(self.node(
                    cache,
                    Some(inst.def),
                    &inst.xform,
                    inst.material.or(inherited),
                    &n,
                    depth + 1,
                ));
            }
        }
        let m = self.conv * *xform * self.conv.inverse();
        let mut nj = json!({"name": name});
        if m != DMat4::IDENTITY {
            nj["matrix"] = json!(m.to_cols_array());
        }
        if let Some(mesh) = mesh {
            nj["mesh"] = json!(mesh);
        }
        if !children.is_empty() {
            nj["children"] = json!(children);
        }
        self.nodes.push(nj);
        self.nodes.len() - 1
    }
}

/// The glTF document of a model (Y up, metres) and its binary buffer.
pub fn document(model: &Model) -> (J, Vec<u8>) {
    let mut cache = Cache::new(model, Opts::default());
    let conv = crate::build::y_up_to_z_up().inverse() * DMat4::from_scale(DVec3::splat(0.001));
    let mut w = Writer {
        model,
        bin: Bin { data: Vec::new(), views: Vec::new(), accessors: Vec::new() },
        nodes: Vec::new(),
        meshes: Vec::new(),
        materials: Vec::new(),
        textures: Vec::new(),
        images: Vec::new(),
        mat_index: HashMap::new(),
        image_index: HashMap::new(),
        mesh_index: HashMap::new(),
        conv,
    };
    let root = w.node(&mut cache, None, &DMat4::IDENTITY, None, "SketchUp", 0);
    while !w.bin.data.len().is_multiple_of(4) {
        w.bin.data.push(0);
    }
    let mut doc = json!({
        "asset": {"version": "2.0", "generator": "SketchUp glTF exporter (SimLab)"},
        "scene": 0,
        "scenes": [{"name": "Scene", "nodes": [root]}],
        "nodes": w.nodes,
        "meshes": w.meshes,
        "materials": w.materials,
        "accessors": w.bin.accessors,
        "bufferViews": w.bin.views,
        "buffers": [{"byteLength": w.bin.data.len()}],
    });
    if !w.images.is_empty() {
        doc["images"] = json!(w.images);
        doc["textures"] = json!(w.textures);
        doc["samplers"] = json!([{"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}]);
    }
    if doc["meshes"].as_array().is_some_and(|m| m.is_empty()) {
        let o = doc.as_object_mut().unwrap();
        for k in ["meshes", "materials", "accessors", "bufferViews", "buffers"] {
            o.remove(k);
        }
    }
    (doc, w.bin.data)
}

/// A .glb file: JSON chunk then BIN chunk.
pub fn glb(doc: &J, bin: &[u8]) -> Vec<u8> {
    let mut json = serde_json::to_vec(doc).unwrap_or_default();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let total = 12 + 8 + json.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    if !bin.is_empty() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(bin);
    }
    out
}

/// File › Export › 3D Model › glTF binary.
pub fn export(model: &Model, path: &Path) -> Result<()> {
    let (doc, bin) = document(model);
    std::fs::write(path, glb(&doc, &bin))?;
    Ok(())
}

// ------------------------------------------------------------------------------------------
// Reading

fn b64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

struct Gltf<'a> {
    doc: J,
    buffers: Vec<Vec<u8>>,
    file: Option<&'a Path>,
}

impl Gltf<'_> {
    fn view(&self, i: usize) -> Option<(&[u8], usize)> {
        let v = self.doc["bufferViews"].get(i)?;
        let buf = self.buffers.get(usize::try_from(v["buffer"].as_u64()?).ok()?)?;
        let off = usize::try_from(v["byteOffset"].as_u64().unwrap_or(0)).ok()?;
        let len = usize::try_from(v["byteLength"].as_u64()?).ok()?;
        let stride = usize::try_from(v["byteStride"].as_u64().unwrap_or(0)).ok()?;
        Some((buf.get(off..off.checked_add(len)?)?, stride))
    }

    /// The bytes of every buffer: what an accessor without data may at most stand for.
    fn buffer_bytes(&self) -> usize {
        self.buffers.iter().map(Vec::len).sum()
    }

    /// The values of an accessor, as floats, `n` per element.
    fn accessor(&self, i: usize) -> Option<(Vec<f64>, usize)> {
        let a = self.doc["accessors"].get(i)?;
        let count = a["count"].as_u64()? as usize;
        let n = match a["type"].as_str()? {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            _ => return None,
        };
        let ct = a["componentType"].as_u64()?;
        let size = match ct {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return None,
        };
        let norm = a["normalized"].as_bool().unwrap_or(false);
        let read = |b: &[u8]| -> f64 {
            match ct {
                5120 => {
                    let v = b[0] as i8 as f64;
                    if norm { (v / 127.0).max(-1.0) } else { v }
                }
                5121 => {
                    let v = b[0] as f64;
                    if norm { v / 255.0 } else { v }
                }
                5122 => {
                    let v = i16::from_le_bytes([b[0], b[1]]) as f64;
                    if norm { (v / 32767.0).max(-1.0) } else { v }
                }
                5123 => {
                    let v = u16::from_le_bytes([b[0], b[1]]) as f64;
                    if norm { v / 65535.0 } else { v }
                }
                5125 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
                _ => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
            }
        };
        // The count comes from the file: it is checked against the data before anything is
        // made (a few bytes of JSON asked for `count * n` floats, gigabytes).
        let total = count.checked_mul(n)?;
        let view = match a["bufferView"].as_u64() {
            Some(vi) => {
                let (data, stride) = self.view(usize::try_from(vi).ok()?)?;
                let off = usize::try_from(a["byteOffset"].as_u64().unwrap_or(0)).ok()?;
                let stride = if stride == 0 { n * size } else { stride };
                if count > 0 {
                    let last = off.checked_add((count - 1).checked_mul(stride)?)?.checked_add(n * size)?;
                    if last > data.len() {
                        return None;
                    }
                }
                Some((data, off, stride))
            }
            None => None,
        };
        // Without data the accessor is zeros (then sparse values): no more of them than the
        // file's buffers could describe.
        if view.is_none() && total > (self.buffer_bytes() * 4).max(1 << 20) {
            return None;
        }
        let mut out = vec![0.0; total];
        if let Some((data, off, stride)) = view {
            for e in 0..count {
                for k in 0..n {
                    let p = off + e * stride + k * size;
                    out[e * n + k] = read(data.get(p..p + size)?);
                }
            }
        }
        // Sparse substitutions.
        let sp = &a["sparse"];
        if let Some(sc) = sp["count"].as_u64() {
            let ii = &sp["indices"];
            let vv = &sp["values"];
            let (idata, _) = self.view(ii["bufferView"].as_u64()? as usize)?;
            let (vdata, _) = self.view(vv["bufferView"].as_u64()? as usize)?;
            let ioff = usize::try_from(ii["byteOffset"].as_u64().unwrap_or(0)).ok()?;
            let voff = usize::try_from(vv["byteOffset"].as_u64().unwrap_or(0)).ok()?;
            let isize = match ii["componentType"].as_u64()? {
                5121 => 1,
                5123 => 2,
                _ => 4,
            };
            for k in 0..usize::try_from(sc).ok()? {
                let p = ioff.checked_add(k.checked_mul(isize)?)?;
                let ib = idata.get(p..p.checked_add(isize)?)?;
                let idx = match isize {
                    1 => ib[0] as usize,
                    2 => u16::from_le_bytes([ib[0], ib[1]]) as usize,
                    _ => u32::from_le_bytes([ib[0], ib[1], ib[2], ib[3]]) as usize,
                };
                for c in 0..n {
                    let q = (k * n + c).checked_mul(size).and_then(|x| x.checked_add(voff))?;
                    let slot = idx.checked_mul(n).and_then(|x| x.checked_add(c));
                    if let (Some(slot), Some(b)) = (slot.and_then(|s| out.get_mut(s)), vdata.get(q..q + size)) {
                        *slot = read(b);
                    }
                }
            }
        }
        Some((out, n))
    }

    fn image(&self, i: usize) -> Option<(String, Vec<u8>)> {
        let img = self.doc["images"].get(i)?;
        let name = img["name"].as_str().unwrap_or("texture").to_string();
        if let Some(v) = img["bufferView"].as_u64() {
            let (data, _) = self.view(v as usize)?;
            let ext = crate::tex::image_ext(data);
            let name = if name.contains('.') { name } else { format!("{name}.{ext}") };
            return Some((name, data.to_vec()));
        }
        let uri = img["uri"].as_str()?;
        if let Some(rest) = uri.strip_prefix("data:") {
            let data = b64(rest.split_once(',')?.1)?;
            let ext = crate::tex::image_ext(&data);
            return Some((format!("{name}.{ext}"), data));
        }
        crate::tex::find_beside(self.file?, uri)
    }

    fn material(&self, i: usize) -> MatIn {
        let m = &self.doc["materials"][i];
        let name = m["name"].as_str().map_or_else(|| format!("Matière {i}"), str::to_string);
        let pbr = &m["pbrMetallicRoughness"];
        let sg = &m["extensions"]["KHR_materials_pbrSpecularGlossiness"];
        let f = if pbr["baseColorFactor"].is_array() { &pbr["baseColorFactor"] } else { &sg["diffuseFactor"] };
        let c: Vec<f64> = f.as_array().map(|a| a.iter().filter_map(J::as_f64).collect()).unwrap_or_default();
        let (mut color, opacity) = if c.len() >= 3 {
            ([to_srgb(c[0]), to_srgb(c[1]), to_srgb(c[2])], c.get(3).copied().unwrap_or(1.0))
        } else {
            ([255, 255, 255], 1.0)
        };
        let tex = if pbr["baseColorTexture"].is_object() { &pbr["baseColorTexture"] } else { &sg["diffuseTexture"] };
        let texture = tex["index"]
            .as_u64()
            .and_then(|t| self.doc["textures"][t as usize]["source"].as_u64())
            .and_then(|s| self.image(s as usize))
            .and_then(|(n, b)| crate::tex::texture_from_bytes(&n, b, None));
        if let Some(t) = &texture
            && c.is_empty()
        {
            color = t.avg;
        }
        let opacity = if m["alphaMode"].as_str() == Some("BLEND") { opacity } else { 1.0 };
        MatIn { name, color, opacity: opacity as f32, texture }
    }

    fn mesh(&self, i: usize, mats: &[MatIn]) -> MeshIn {
        let mut out = MeshIn::default();
        let Some(prims) = self.doc["meshes"][i]["primitives"].as_array() else { return out };
        for p in prims {
            let mode = p["mode"].as_u64().unwrap_or(4);
            let Some((pos, 3)) = p["attributes"]["POSITION"].as_u64().and_then(|a| self.accessor(a as usize)) else {
                continue;
            };
            let nor = p["attributes"]["NORMAL"].as_u64().and_then(|a| self.accessor(a as usize)).filter(|x| x.1 == 3);
            let uv =
                p["attributes"]["TEXCOORD_0"].as_u64().and_then(|a| self.accessor(a as usize)).filter(|x| x.1 == 2);
            let count = pos.len() / 3;
            let idx: Vec<u32> = match p["indices"].as_u64().and_then(|a| self.accessor(a as usize)) {
                Some((v, 1)) => v.into_iter().map(|x| x as u32).collect(),
                _ => (0..count as u32).collect(),
            };
            let base = out.pos.len() as u32;
            for k in 0..count {
                out.pos.push(DVec3::new(pos[k * 3], pos[k * 3 + 1], pos[k * 3 + 2]));
            }
            let mat = p["material"].as_u64().map(|m| m as u32);
            let textured = mat.and_then(|m| mats.get(m as usize)).is_some_and(|m| m.texture.is_some());
            // The indices come from the file: a normal or a coordinate that is not there is
            // dropped (the normals and UV accessors may be shorter than the positions; indexing
            // them blindly panicked).
            let poly = |t: [u32; 3]| -> PolyIn {
                let normals = nor
                    .as_ref()
                    .and_then(|(n, _)| {
                        t.iter()
                            .map(|&k| {
                                let i = k as usize * 3;
                                Some(DVec3::new(*n.get(i)?, *n.get(i + 1)?, *n.get(i + 2)?))
                            })
                            .collect::<Option<Vec<_>>>()
                    })
                    .unwrap_or_default();
                let uv = match (&uv, textured) {
                    (Some((u, _)), true) => t
                        .iter()
                        .map(|&k| {
                            let i = k as usize * 2;
                            Some(DVec2::new(*u.get(i)?, 1.0 - *u.get(i + 1)?))
                        })
                        .collect::<Option<Vec<_>>>()
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                PolyIn { outer: t.iter().map(|k| base + k).collect(), mat, uv, normals, ..Default::default() }
            };
            match mode {
                4 => {
                    for t in idx.as_chunks::<3>().0 {
                        out.polys.push(poly([t[0], t[1], t[2]]));
                    }
                }
                5 => {
                    for k in 2..idx.len() {
                        let t = if k % 2 == 0 {
                            [idx[k - 2], idx[k - 1], idx[k]]
                        } else {
                            [idx[k - 1], idx[k - 2], idx[k]]
                        };
                        out.polys.push(poly(t));
                    }
                }
                6 => {
                    for k in 2..idx.len() {
                        out.polys.push(poly([idx[0], idx[k - 1], idx[k]]));
                    }
                }
                1 => {
                    for l in idx.as_chunks::<2>().0 {
                        out.lines.push(LineIn { pts: vec![base + l[0], base + l[1]], closed: false, layer: None });
                    }
                }
                2 | 3 => {
                    out.lines.push(LineIn {
                        pts: idx.iter().map(|k| base + k).collect(),
                        closed: mode == 2,
                        layer: None,
                    });
                }
                _ => {}
            }
        }
        out
    }
}

fn node_matrix(n: &J) -> DMat4 {
    if let Some(m) = n["matrix"].as_array() {
        let v: Vec<f64> = m.iter().filter_map(J::as_f64).collect();
        if v.len() == 16 {
            return DMat4::from_cols_slice(&v);
        }
    }
    let arr = |k: &str, d: &[f64]| -> Vec<f64> {
        n[k].as_array()
            .map(|a| a.iter().filter_map(J::as_f64).collect())
            .filter(|v: &Vec<f64>| v.len() == d.len())
            .unwrap_or_else(|| d.to_vec())
    };
    let t = arr("translation", &[0.0, 0.0, 0.0]);
    let r = arr("rotation", &[0.0, 0.0, 0.0, 1.0]);
    let s = arr("scale", &[1.0, 1.0, 1.0]);
    DMat4::from_scale_rotation_translation(
        DVec3::new(s[0], s[1], s[2]),
        DQuat::from_xyzw(r[0], r[1], r[2], r[3]).normalize(),
        DVec3::new(t[0], t[1], t[2]),
    )
}

/// Reads a .glb or .gltf file (Y up, metres).
pub fn read(bytes: &[u8], file: Option<&Path>) -> Result<SceneIn> {
    let (doc, bin): (J, Option<Vec<u8>>) = if bytes.starts_with(b"glTF") {
        if bytes.len() < 20 {
            bail!("fichier GLB tronqué");
        }
        let jlen = u32::from_le_bytes(bytes[12..16].try_into()?) as usize;
        if &bytes[16..20] != b"JSON" || bytes.len() < 20 + jlen {
            bail!("fichier GLB invalide");
        }
        let doc = serde_json::from_slice(&bytes[20..20 + jlen]).context("JSON du GLB illisible")?;
        let rest = &bytes[20 + jlen..];
        let bin = if rest.len() >= 8 && &rest[4..8] == b"BIN\0" {
            let blen = u32::from_le_bytes(rest[0..4].try_into()?) as usize;
            Some(rest.get(8..8 + blen).context("bloc binaire tronqué")?.to_vec())
        } else {
            None
        };
        (doc, bin)
    } else {
        (serde_json::from_slice(bytes).context("fichier glTF illisible")?, None)
    };
    if !doc["asset"]["version"].as_str().is_some_and(|v| v.starts_with('2')) {
        bail!("seul glTF 2.0 est pris en charge");
    }
    let mut buffers = Vec::new();
    for (i, b) in doc["buffers"].as_array().cloned().unwrap_or_default().iter().enumerate() {
        let data = match b["uri"].as_str() {
            None if i == 0 => bin.clone().unwrap_or_default(),
            None => Vec::new(),
            Some(u) if u.starts_with("data:") => u.split_once(',').and_then(|x| b64(x.1)).unwrap_or_default(),
            Some(u) => file.and_then(|f| crate::tex::find_beside(f, u)).map(|x| x.1).unwrap_or_default(),
        };
        buffers.push(data);
    }
    let g = Gltf { doc, buffers, file };
    let mut scene = SceneIn::default();
    let nmat = g.doc["materials"].as_array().map_or(0, Vec::len);
    for i in 0..nmat {
        let m = g.material(i);
        scene.materials.push(m);
    }
    // Meshes used by several nodes become components.
    let nodes = g.doc["nodes"].as_array().cloned().unwrap_or_default();
    let mut uses: HashMap<u64, usize> = HashMap::new();
    for n in &nodes {
        if let Some(m) = n["mesh"].as_u64() {
            *uses.entry(m).or_default() += 1;
        }
    }
    let nmesh = g.doc["meshes"].as_array().map_or(0, Vec::len);
    for i in 0..nmesh {
        let m = g.mesh(i, &scene.materials);
        scene.meshes.push(m);
    }
    let mut defs: HashMap<u64, usize> = HashMap::new();
    // glTF nodes form a tree (a node has one parent at most): each is entered once. A file
    // where every node lists the next one twice made 2^64 visits (the depth limit alone
    // did not stop the fan-out).
    let mut seen = vec![false; nodes.len()];
    #[allow(clippy::too_many_arguments)]
    fn walk(
        i: usize,
        nodes: &[J],
        uses: &HashMap<u64, usize>,
        defs: &mut HashMap<u64, usize>,
        scene: &mut SceneIn,
        g: &Gltf,
        depth: usize,
        seen: &mut [bool],
    ) -> NodeIn {
        seen[i] = true;
        let n = &nodes[i];
        let mut out =
            NodeIn { name: n["name"].as_str().unwrap_or("").to_string(), xform: node_matrix(n), ..Default::default() };
        if let Some(m) = n["mesh"].as_u64()
            && (m as usize) < scene.meshes.len()
        {
            if uses.get(&m).copied().unwrap_or(0) > 1 {
                let d = *defs.entry(m).or_insert_with(|| {
                    let name = g.doc["meshes"][m as usize]["name"].as_str().unwrap_or("Composant").to_string();
                    scene.defs.push(DefIn { name, meshes: vec![m as usize], children: Vec::new() });
                    scene.defs.len() - 1
                });
                out.children.push(NodeIn { def: Some(d), ..Default::default() });
            } else {
                out.meshes.push(m as usize);
            }
        }
        if depth < 64 {
            for c in n["children"].as_array().cloned().unwrap_or_default() {
                if let Some(c) = c.as_u64().map(|c| c as usize).filter(|c| *c < nodes.len() && !seen[*c]) {
                    let child = walk(c, nodes, uses, defs, scene, g, depth + 1, seen);
                    out.children.push(child);
                }
            }
        }
        out
    }
    let si = g.doc["scene"].as_u64().unwrap_or(0) as usize;
    let roots: Vec<usize> = match g.doc["scenes"].get(si) {
        Some(s) => s["nodes"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_u64().map(|x| x as usize)).collect())
            .unwrap_or_default(),
        None => (0..nodes.len()).collect(),
    };
    for r in roots.into_iter().filter(|r| *r < nodes.len()) {
        if seen[r] {
            continue;
        }
        let n = walk(r, &nodes, &uses, &mut defs, &mut scene, &g, 0, &mut seen);
        scene.root.children.push(n);
    }
    Ok(scene)
}

/// File › Import of a glTF file.
pub fn import(bytes: &[u8], file: Option<&Path>, _o: &GlbImport) -> Result<Model> {
    let scene = read(bytes, file)?;
    Ok(crate::build::build(
        &scene,
        BuildOpts { scale: 1000.0, swap_yz: true, merge_coplanar: true, orient_faces: false },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::model::{Definition, Instance};

    fn model() -> Model {
        let mut m = Model::new();
        let mut e = esq_core::model::Entities::default();
        let v: Vec<_> = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)]
            .iter()
            .map(|(x, y)| e.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
            .collect();
        let f = e.add_face_raw(v, vec![]).unwrap();
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 2, image::Rgb([10, 200, 30])))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let grass = m.add_material("Herbe", [10, 200, 30]);
        m.materials[grass].ext.texture = crate::tex::texture_from_bytes("herbe.png", png, Some([250.0, 250.0]));
        e.faces[f].front = Some(grass);
        let d = m.defs.insert(Definition {
            name: "Dalle".into(),
            is_group: false,
            description: String::new(),
            entities: e,
            attributes: Default::default(),
            ext: Default::default(),
        });
        for x in [0.0, 3000.0] {
            m.root.instances.insert(Instance {
                def: d,
                xform: DMat4::from_translation(DVec3::new(x, 0.0, 500.0)),
                name: String::new(),
                material: None,
                layer: 0,
                hidden: false,
                locked: false,
                attributes: Default::default(),
                ext: Default::default(),
            });
        }
        m
    }

    #[test]
    fn glb_round_trip() {
        let m = model();
        let (doc, bin) = document(&m);
        assert_eq!(doc["asset"]["version"], "2.0");
        // One mesh shared by both nodes; Y up: the slab lies at y = 0.5 m.
        assert_eq!(doc["meshes"].as_array().unwrap().len(), 1);
        let nodes = doc["nodes"].as_array().unwrap();
        assert_eq!(nodes.iter().filter(|n| n["mesh"] == 0).count(), 2);
        assert!(nodes.iter().any(|n| n["matrix"][13].as_f64().is_some_and(|y| (y - 0.5).abs() < 1e-9)));
        let bytes = glb(&doc, &bin);
        assert_eq!(&bytes[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize, bytes.len());
        let back = import(&bytes, None, &GlbImport::default()).unwrap();
        // Two instances of one component, with the textured slab.
        assert_eq!(back.defs.values().filter(|d| !d.is_group).count(), 1);
        let mut found = Vec::new();
        esq_core::walk::visit(&back, |pl| {
            for f in pl.ents.faces.values() {
                let z = pl.xform.transform_point3(pl.ents.verts[f.outer[0]].pos).z;
                let mat = &back.materials[f.front.unwrap()];
                found.push((z, mat.name.clone(), mat.ext.texture.is_some(), f.tex.is_some()));
            }
        });
        // Front and back faces of the slab merge into one face each side.
        assert!(found.iter().any(|f| (f.0 - 500.0).abs() < 1e-3 && f.1 == "Herbe" && f.2 && f.3), "{found:?}");
    }

    #[test]
    fn reads_gltf_with_data_uri_and_strip() {
        // A square as a triangle strip, positions in a base64 buffer.
        let pos: [f32; 12] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 1.0, 0.0, -1.0];
        let bytes: Vec<u8> = pos.iter().flat_map(|x| x.to_le_bytes()).collect();
        let enc: String = {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            for c in bytes.chunks(3) {
                let n = c.iter().enumerate().fold(0u32, |a, (i, b)| a | (*b as u32) << (16 - i * 8));
                for k in 0..4 {
                    if k <= c.len() {
                        s.push(T[(n >> (18 - k * 6) & 63) as usize] as char);
                    } else {
                        s.push('=');
                    }
                }
            }
            s
        };
        let doc = json!({
            "asset": {"version": "2.0"},
            "buffers": [{"byteLength": 48, "uri": format!("data:application/octet-stream;base64,{enc}")}],
            "bufferViews": [{"buffer": 0, "byteLength": 48}],
            "accessors": [{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3"}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "mode": 5}]}],
            "nodes": [{"mesh": 0, "translation": [0.0, 2.0, 0.0]}],
            "scenes": [{"nodes": [0]}],
        });
        let m = import(&serde_json::to_vec(&doc).unwrap(), None, &GlbImport::default()).unwrap();
        let mut faces = Vec::new();
        esq_core::walk::visit(&m, |pl| {
            for f in pl.ents.faces.values() {
                faces.push((
                    pl.xform.transform_vector3(f.normal),
                    pl.xform.transform_point3(pl.ents.verts[f.outer[0]].pos),
                ));
            }
        });
        assert_eq!(faces.len(), 1, "strip triangles merge into the square");
        assert!(faces[0].0.z.abs() > 0.99);
        assert!((faces[0].1.z - 2000.0).abs() < 1e-6);
    }

    /// One triangle: three positions and a single normal, as a data URI.
    fn triangle_doc(extra: J) -> J {
        let mut bin = Vec::new();
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] {
            bin.extend(v.to_le_bytes());
        }
        let uri = format!("data:application/octet-stream;base64,{}", esq_core::photo::base64_encode(&bin));
        let mut doc = json!({
            "asset": {"version": "2.0"},
            "buffers": [{"uri": uri, "byteLength": 48}],
            "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}, {"buffer": 0, "byteOffset": 36, "byteLength": 12}],
            "accessors": [
                {"bufferView": 0, "count": 3, "type": "VEC3", "componentType": 5126},
                {"bufferView": 1, "count": 1, "type": "VEC3", "componentType": 5126}
            ],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}}]}],
            "nodes": [{"mesh": 0}],
            "scenes": [{"nodes": [0]}],
        });
        if let (Some(d), Some(e)) = (doc.as_object_mut(), extra.as_object()) {
            d.extend(e.clone());
        }
        doc
    }

    /// A normals accessor shorter than the positions: reading the second corner's normal
    /// indexed past its end and the import panicked.
    #[test]
    fn short_normals_do_not_panic() {
        let m = import(&serde_json::to_vec(&triangle_doc(json!({}))).unwrap(), None, &GlbImport::default()).unwrap();
        let mut n = 0;
        esq_core::walk::visit(&m, |pl| n += pl.ents.faces.len());
        assert_eq!(n, 1);
    }

    /// An accessor of four billion VEC3 without data (a few bytes of JSON): the reader
    /// allocated `count × 3` floats at once (96 GB), the process died.
    #[test]
    fn a_huge_declared_count_is_refused() {
        let doc = triangle_doc(json!({
            "accessors": [{"count": 4_000_000_000u64, "type": "VEC3", "componentType": 5126}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
        }));
        let s = read(&serde_json::to_vec(&doc).unwrap(), None).unwrap();
        assert!(s.meshes.iter().all(|m| m.pos.is_empty()));
        // With data: a count larger than the view is refused too.
        let doc = triangle_doc(json!({
            "accessors": [{"bufferView": 0, "count": 4_000_000_000u64, "type": "VEC3", "componentType": 5126}],
        }));
        let s = read(&serde_json::to_vec(&doc).unwrap(), None).unwrap();
        assert!(s.meshes.iter().all(|m| m.pos.is_empty()));
    }

    /// 64 nodes, each listing the next one twice: the walk made 2^64 visits (the depth
    /// limit does not stop the fan-out). Each node is entered once.
    #[test]
    fn nodes_listed_many_times_are_entered_once() {
        let nodes: Vec<J> =
            (0..64).map(|i| if i < 63 { json!({"children": [i + 1, i + 1]}) } else { json!({"mesh": 0}) }).collect();
        let doc = triangle_doc(json!({ "nodes": nodes }));
        let (tx, rx) = std::sync::mpsc::channel();
        let bytes = serde_json::to_vec(&doc).unwrap();
        std::thread::spawn(move || {
            let _ = tx.send(read(&bytes, None).map(|s| s.root.children.len()));
        });
        let roots = rx.recv_timeout(std::time::Duration::from_secs(20)).expect("the reading ends").unwrap();
        assert_eq!(roots, 1);
    }
}
