//! File › Import › TrimBIM (.trb), Trimble's exchange format (Trimble Connect, Tekla, Quadri):
//! a FlatBuffers buffer tagged `TRB8`, read here as SketchUp's `trbimporter` reads it.
//!
//! What SketchUp makes of a file, as its importer was observed doing it:
//! - lengths are metres;
//! - every entity becomes a component, placed by the entity's global placement, holding one
//!   instance per geometry instance of that entity; every geometry definition becomes a
//!   component ("Component1", "Component2"… then the entities' ones), placed by its local
//!   placement, painted with its surface material ("<auto>1"…) and on its layer (a tag);
//! - triangle meshes keep their triangles, faceted B-reps their polygons with holes (those
//!   with texture coordinates are cut into triangles), graphics containers give their lines
//!   and arcs (5° segments); swept disks, alignments, grids, points, billboards and bolts are
//!   left out;
//! - an edge between two faces whose normals differ by 17.56° or more stays visible, every
//!   other edge (boundaries included) is softened; textured meshes have all their edges hidden;
//! - a textured material shows its image at one tile per inch, the texture coordinates of the
//!   file being ignored; the image is kept in PNG when it has an alpha channel, in JPEG
//!   otherwise; the material's colour is the image's average.
//!
//! The schema (`Trimble.Technology.TrimBim.Fbs`) is the one the reader library SketchUp
//! installs describes in its FlatBuffers reflection tables; only the parts used are read.

use std::collections::HashMap;

use anyhow::{Result, bail};
use esq_core::glam::{DMat4, DVec2, DVec3, DVec4};
use esq_core::model::{Definition, Entities, Instance, LayerIdx, MaterialId, Model, Units, VertexId};

/// Edges between faces whose normals' dot product is above this are softened: SketchUp's
/// importer softens a fold of 17.5605° and keeps one of 17.561°, at any size.
const SOFT_COS: f64 = 0.953_397_5;
/// Segments of the arcs of a graphics container.
const ARC_STEP: f64 = 5.0 * std::f64::consts::PI / 180.0;
const NONE: u32 = u32::MAX;

// ---------------------------------------------------------------------------------------
// FlatBuffers reading, bounds-checked.

#[derive(Clone, Copy)]
struct Buf<'a>(&'a [u8]);

/// A table: its position and its vtable's.
#[derive(Clone, Copy)]
struct Table {
    pos: usize,
    vt: usize,
    vt_len: usize,
}

impl<'a> Buf<'a> {
    fn bytes(&self, at: usize, n: usize) -> Result<&'a [u8]> {
        match at.checked_add(n).and_then(|e| self.0.get(at..e)) {
            Some(b) => Ok(b),
            None => bail!("fichier TrimBIM tronqué (octet {at})"),
        }
    }
    fn u8(&self, at: usize) -> Result<u8> {
        Ok(self.bytes(at, 1)?[0])
    }
    fn u16(&self, at: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(at, 2)?.try_into()?))
    }
    fn u32(&self, at: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(at, 4)?.try_into()?))
    }
    fn i32(&self, at: usize) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(at, 4)?.try_into()?))
    }
    fn f32(&self, at: usize) -> Result<f64> {
        Ok(f32::from_le_bytes(self.bytes(at, 4)?.try_into()?) as f64)
    }
    fn f64(&self, at: usize) -> Result<f64> {
        Ok(f64::from_le_bytes(self.bytes(at, 8)?.try_into()?))
    }
    fn vec3f(&self, at: usize) -> Result<DVec3> {
        Ok(DVec3::new(self.f32(at)?, self.f32(at + 4)?, self.f32(at + 8)?))
    }
    fn vec3d(&self, at: usize) -> Result<DVec3> {
        Ok(DVec3::new(self.f64(at)?, self.f64(at + 8)?, self.f64(at + 16)?))
    }

    /// The position an unsigned offset stored at `at` points to.
    fn follow(&self, at: usize) -> Result<usize> {
        Ok(at + self.u32(at)? as usize)
    }

    fn table(&self, pos: usize) -> Result<Table> {
        let vt = pos as i64 - self.i32(pos)? as i64;
        if vt < 0 {
            bail!("table TrimBIM invalide (octet {pos})");
        }
        let vt = vt as usize;
        let vt_len = self.u16(vt)? as usize;
        Ok(Table { pos, vt, vt_len })
    }

    /// Where field `i` of a table is, if present.
    fn field(&self, t: Table, i: usize) -> Result<Option<usize>> {
        let at = 4 + 2 * i;
        if at + 2 > t.vt_len {
            return Ok(None);
        }
        let off = self.u16(t.vt + at)? as usize;
        Ok((off != 0).then_some(t.pos + off))
    }

    fn sub_table(&self, t: Table, i: usize) -> Result<Option<Table>> {
        match self.field(t, i)? {
            Some(at) => Ok(Some(self.table(self.follow(at)?)?)),
            None => Ok(None),
        }
    }

    /// A vector field: the position of its first element and its length (elements of
    /// `size` bytes, checked against the buffer).
    fn vector(&self, t: Table, i: usize, size: usize) -> Result<(usize, usize)> {
        let Some(at) = self.field(t, i)? else { return Ok((0, 0)) };
        let v = self.follow(at)?;
        let n = self.u32(v)? as usize;
        self.bytes(v + 4, n.saturating_mul(size))?;
        Ok((v + 4, n))
    }

    fn tables(&self, t: Table, i: usize) -> Result<Vec<Table>> {
        let (start, n) = self.vector(t, i, 4)?;
        (0..n).map(|k| self.table(self.follow(start + 4 * k)?)).collect()
    }

    fn string_at(&self, at: usize) -> Result<String> {
        let s = self.follow(at)?;
        let n = self.u32(s)? as usize;
        Ok(String::from_utf8_lossy(self.bytes(s + 4, n)?).into_owned())
    }

    fn strings(&self, t: Table, i: usize) -> Result<Vec<String>> {
        let (start, n) = self.vector(t, i, 4)?;
        (0..n).map(|k| self.string_at(start + 4 * k)).collect()
    }

    fn u16s(&self, t: Table, i: usize) -> Result<Vec<u32>> {
        let (start, n) = self.vector(t, i, 2)?;
        (0..n).map(|k| self.u16(start + 2 * k).map(u32::from)).collect()
    }

    fn u8s(&self, t: Table, i: usize) -> Result<Vec<u32>> {
        let (start, n) = self.vector(t, i, 1)?;
        (0..n).map(|k| self.u8(start + k).map(u32::from)).collect()
    }

    fn vec3fs(&self, t: Table, i: usize) -> Result<Vec<DVec3>> {
        let (start, n) = self.vector(t, i, 12)?;
        (0..n).map(|k| self.vec3f(start + 12 * k)).collect()
    }
}

/// `Placement`: origin (doubles), X and Y axes (floats); Z completes them.
fn placement(b: Buf, at: usize) -> Result<DMat4> {
    let o = b.vec3d(at)? * 1000.0;
    let x = b.vec3f(at + 24)?;
    let y = b.vec3f(at + 36)?;
    let z = x.cross(y);
    Ok(DMat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), DVec4::new(o.x, o.y, o.z, 1.0)))
}

// Geometry types (`GeometryType`).
const G_BREP: u8 = 1;
const G_MESH: u8 = 2;
const G_MESH8: u8 = 3;
const G_GRAPHICS: u8 = 5;
const G_BREP_UV: u8 = 6;
const G_BREP_NORMALS: u8 = 7;
const G_TEX_MESH: u8 = 10;
const G_TEX_MESH8: u8 = 11;

// Fields of `ModelGeometry`.
const F_PLACEMENTS: usize = 0;
const F_MATERIALS: usize = 1;
const F_LAYERS: usize = 2;
const F_DEFINITIONS: usize = 3;
const F_INSTANCES: usize = 4;
const F_TEXTURES: usize = 5;
const F_BREPS: usize = 6;
const F_MESHES: usize = 7;
const F_MESHES8: usize = 8;
const F_TEX_MESHES: usize = 9;
const F_TEX_MESHES8: usize = 10;
const F_BREPS_UV: usize = 12;
const F_BREPS_NORMALS: usize = 13;
const F_GRAPHICS: usize = 14;
const F_LAYER_VISIBILITY: usize = 23;

/// Faces of one definition, vertices merged where they coincide (millimetres).
struct Builder {
    e: Entities,
    at: HashMap<[i64; 3], VertexId>,
}

impl Builder {
    fn new() -> Builder {
        Builder { e: Entities::default(), at: HashMap::new() }
    }

    fn vertex(&mut self, p: DVec3) -> VertexId {
        let key = [(p.x * 1000.0).round() as i64, (p.y * 1000.0).round() as i64, (p.z * 1000.0).round() as i64];
        *self.at.entry(key).or_insert_with(|| self.e.add_vertex_raw(p))
    }

    fn face(&mut self, outer: &[DVec3], holes: &[Vec<DVec3>]) {
        let o: Vec<VertexId> = outer.iter().map(|p| self.vertex(*p * 1000.0)).collect();
        let h: Vec<Vec<VertexId>> =
            holes.iter().map(|l| l.iter().map(|p| self.vertex(*p * 1000.0)).collect()).collect();
        let mut o = o;
        o.dedup();
        if o.len() > 1 && o[0] == o[o.len() - 1] {
            o.pop();
        }
        if o.len() >= 3 {
            self.e.add_face_raw(o, h.into_iter().filter(|l| l.len() >= 3).collect());
        }
    }

    /// Softens what SketchUp softens; `hidden` hides every edge instead.
    fn finish(mut self, hidden: bool) -> Entities {
        let ids: Vec<_> = self.e.edges.keys().collect();
        for id in ids {
            let faces = self.e.edges[id].faces.clone();
            let normals: Vec<DVec3> = faces.iter().filter_map(|f| self.e.faces.get(*f)).map(|f| f.normal).collect();
            let creased = normals.iter().enumerate().any(|(i, a)| {
                normals[i + 1..].iter().any(|b| a.normalize_or_zero().dot(b.normalize_or_zero()) <= SOFT_COS)
            });
            let edge = &mut self.e.edges[id];
            if hidden {
                edge.hidden = true;
            } else if !creased {
                edge.soft = true;
            }
        }
        self.e
    }
}

/// A triangle mesh (`TriangleMesh`, its 8-bit and textured variants): positions, then
/// vertices (position index first, normal index second), then triangles of vertex indices.
fn mesh(b: Buf, t: Table, small: bool, textured: bool) -> Result<Entities> {
    let positions = b.vec3fs(t, 0)?;
    // Textured meshes have their texture coordinates before the vertices.
    let (fv, fi) = if textured { (3, 4) } else { (2, 3) };
    let vertices: Vec<u32> = if small {
        let (start, n) = b.vector(t, fv, 2)?;
        (0..n).map(|k| b.u8(start + 2 * k).map(u32::from)).collect::<Result<_>>()?
    } else {
        let (start, n) = b.vector(t, fv, 4)?;
        (0..n).map(|k| b.u16(start + 4 * k).map(u32::from)).collect::<Result<_>>()?
    };
    let indices = if small { b.u8s(t, fi)? } else { b.u16s(t, fi)? };
    let mut out = Builder::new();
    for tri in indices.as_chunks::<3>().0 {
        let pts: Option<Vec<DVec3>> =
            tri.iter().map(|&i| vertices.get(i as usize).and_then(|&p| positions.get(p as usize)).copied()).collect();
        if let Some(pts) = pts {
            out.face(&pts, &[]);
        }
    }
    Ok(out.finish(textured))
}

/// A faceted B-rep: outer wires are faces, inner wires their holes. `stride` is the size of
/// a vertex (position first); `triangulate` cuts the faces into triangles.
fn brep(b: Buf, t: Table, stride: usize, triangulate: bool) -> Result<Entities> {
    let (start, n) = b.vector(t, 2, stride)?;
    let verts: Vec<DVec3> = (0..n).map(|k| b.vec3f(start + stride * k)).collect::<Result<_>>()?;
    let pts = |idx: &[u32]| -> Vec<DVec3> { idx.iter().filter_map(|&i| verts.get(i as usize).copied()).collect() };
    let outers: Vec<Vec<DVec3>> =
        b.tables(t, 0)?.into_iter().map(|w| b.u16s(w, 0).map(|v| pts(&v))).collect::<Result<_>>()?;
    let mut holes: Vec<Vec<Vec<DVec3>>> = vec![Vec::new(); outers.len()];
    for w in b.tables(t, 1)? {
        let owner = match b.field(w, 0)? {
            Some(at) => b.u16(at)? as usize,
            None => 0,
        };
        if let Some(h) = holes.get_mut(owner) {
            h.push(pts(&b.u16s(w, 1)?));
        }
    }
    let mut out = Builder::new();
    for (outer, holes) in outers.iter().zip(&holes) {
        if !triangulate {
            out.face(outer, holes);
        } else if holes.is_empty() {
            for i in 1..outer.len().saturating_sub(1) {
                out.face(&[outer[0], outer[i], outer[i + 1]], &[]);
            }
        } else {
            for tri in triangles(outer, holes) {
                out.face(&tri, &[]);
            }
        }
    }
    Ok(out.finish(false))
}

/// Triangles of a polygon with holes, in its own plane.
fn triangles(outer: &[DVec3], holes: &[Vec<DVec3>]) -> Vec<[DVec3; 3]> {
    let n = esq_core::math::newell_normal(outer);
    if n == DVec3::ZERO {
        return Vec::new();
    }
    let u = n.any_orthonormal_vector();
    let v = n.normalize().cross(u);
    let flat = |p: &DVec3| DVec2::new(p.dot(u), p.dot(v));
    let all: Vec<DVec3> = outer.iter().chain(holes.iter().flatten()).copied().collect();
    let o: Vec<DVec2> = outer.iter().map(flat).collect();
    let h: Vec<Vec<DVec2>> = holes.iter().map(|l| l.iter().map(flat).collect()).collect();
    esq_core::tess::triangulate_2d(&o, &h).into_iter().map(|[a, b, c]| [all[a], all[b], all[c]]).collect()
}
/// A graphics container: line segments and arcs as edges.
fn graphics(b: Buf, t: Table) -> Result<Entities> {
    let mut e = Entities::default();
    let (start, n) = b.vector(t, 0, 24)?;
    for k in 0..n {
        let (a, c) = (b.vec3f(start + 24 * k)? * 1000.0, b.vec3f(start + 24 * k + 12)? * 1000.0);
        if a.distance(c) > 1e-6 {
            e.add_polyline(&[a, c], false, false);
        }
    }
    let (start, n) = b.vector(t, 1, 44)?;
    for k in 0..n {
        let at = start + 44 * k;
        let (center, axis, refd) = (b.vec3f(at)? * 1000.0, b.vec3f(at + 12)?, b.vec3f(at + 24)?);
        let (radius, angle) = (b.f32(at + 36)? * 1000.0, b.f32(at + 40)?);
        // An arc of more than a turn is a circle: an angle of 3e38 asked for 10^39 segments
        // (the list of points filled the memory).
        let angle = if angle.is_finite() { angle.clamp(-std::f64::consts::TAU, std::f64::consts::TAU) } else { 0.0 };
        let axis = axis.normalize_or(DVec3::Z);
        let x = (refd - axis * refd.dot(axis)).normalize_or(axis.any_orthonormal_vector());
        let y = axis.cross(x);
        let segs = ((angle.abs() / ARC_STEP).round() as usize).max(1);
        let pts: Vec<DVec3> = (0..=segs)
            .map(|i| {
                let a = angle * i as f64 / segs as f64;
                center + (x * a.cos() + y * a.sin()) * radius
            })
            .collect();
        if radius > 1e-6 {
            e.add_polyline(&pts, false, true);
        }
    }
    Ok(e)
}

/// The texture file SketchUp keeps for an image: PNG when the image has an alpha channel,
/// otherwise the image in JPEG ("Texture.png" / "Texture.jpg").
fn texture_file(bytes: &[u8]) -> Option<(&'static str, Vec<u8>)> {
    let img = image::load_from_memory(bytes).ok()?;
    if img.color().has_alpha() {
        return Some(("Texture.png", bytes.to_vec()));
    }
    let mut out = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
    image::DynamicImage::ImageRgb8(img.to_rgb8()).write_with_encoder(enc).ok()?;
    Some(("Texture.jpg", out))
}

/// Reads a TrimBIM file into a model (millimetres), one component per entity.
pub fn import(bytes: &[u8]) -> Result<Model> {
    let b = Buf(bytes);
    if bytes.len() < 8 || &bytes[4..8] != b"TRB8" {
        bail!("ce fichier n'est pas un fichier TrimBIM");
    }
    let root = b.table(b.follow(0)?)?;
    let Some(entities) = b.sub_table(root, 0)? else { bail!("fichier TrimBIM sans entités") };
    let Some(geo) = b.sub_table(root, 2)? else { bail!("fichier TrimBIM sans géométrie") };

    let mut m = Model::new();
    m.units = Units::Meters;

    // Tags.
    let names = b.strings(geo, F_LAYERS)?;
    let (vis_start, vis_n) = b.vector(geo, F_LAYER_VISIBILITY, 1)?;
    let mut layers: Vec<LayerIdx> = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let l = m.add_layer(name);
        if i < vis_n {
            m.layers[l].visible = b.u8(vis_start + i)? != 0;
        }
        layers.push(l);
    }

    // Geometry definitions, numbered as SketchUp names them.
    let (def_start, ndefs) = b.vector(geo, F_DEFINITIONS, 32)?;
    let tables = |f: usize| b.tables(geo, f);
    let (breps, meshes, meshes8) = (tables(F_BREPS)?, tables(F_MESHES)?, tables(F_MESHES8)?);
    let (tex, tex8) = (tables(F_TEX_MESHES)?, tables(F_TEX_MESHES8)?);
    let (breps_uv, breps_n, gfx) = (tables(F_BREPS_UV)?, tables(F_BREPS_NORMALS)?, tables(F_GRAPHICS)?);
    let mut count = 0;
    let mut defs = Vec::with_capacity(ndefs);
    for k in 0..ndefs {
        let at = def_start + 32 * k;
        let (kind, index) = (b.u8(at)?, b.u32(at + 4)? as usize);
        let get = |list: &[Table]| list.get(index).copied();
        let ents = match kind {
            G_MESH => get(&meshes).map(|t| mesh(b, t, false, false)),
            G_MESH8 => get(&meshes8).map(|t| mesh(b, t, true, false)),
            G_TEX_MESH => get(&tex).map(|t| mesh(b, t, false, true)),
            G_TEX_MESH8 => get(&tex8).map(|t| mesh(b, t, true, true)),
            G_BREP => get(&breps).map(|t| brep(b, t, 12, false)),
            G_BREP_NORMALS => get(&breps_n).map(|t| brep(b, t, 24, false)),
            G_BREP_UV => get(&breps_uv).map(|t| brep(b, t, 20, true)),
            G_GRAPHICS => get(&gfx).map(|t| graphics(b, t)),
            _ => None,
        };
        defs.push(match ents {
            Some(e) => {
                count += 1;
                Some(m.defs.insert(Definition {
                    name: format!("Component{count}"),
                    is_group: false,
                    description: String::new(),
                    entities: e?,
                    attributes: Default::default(),
                    ext: Default::default(),
                }))
            }
            None => None,
        });
    }

    // Materials, made when first used.
    let (mat_start, nmats) = b.vector(geo, F_MATERIALS, 20)?;
    let textures = tables(F_TEXTURES)?;
    let mut materials: HashMap<usize, MaterialId> = HashMap::new();
    let mut material = |m: &mut Model, i: usize| -> Result<Option<MaterialId>> {
        if i >= nmats {
            return Ok(None);
        }
        if let Some(id) = materials.get(&i) {
            return Ok(Some(*id));
        }
        let at = mat_start + 20 * i;
        let rgba = b.bytes(at, 4)?;
        let id = m.add_material(&format!("<auto>{}", materials.len() + 1), [rgba[0], rgba[1], rgba[2]]);
        m.materials[id].opacity = rgba[3] as f32 / 255.0;
        let tex = b.u32(at + 4)?;
        if tex != NONE
            && let Some(t) = textures.get(tex as usize)
        {
            let (start, n) = b.vector(*t, 1, 1)?;
            let original = b.bytes(start, n)?;
            if let Some((name, data)) = texture_file(original)
                && let Some(mut td) = crate::tex::texture_from_bytes(name, original.to_vec(), Some([25.4, 25.4]))
            {
                // The colour is the average of the image as given.
                td.image = esq_core::photo::Blob(std::sync::Arc::new(data));
                m.materials[id].color = td.avg;
                m.materials[id].ext.texture = Some(td);
            }
        }
        materials.insert(i, id);
        Ok(Some(id))
    };

    // Entities and their instances.
    let (ent_start, nents) = b.vector(entities, 0, 64)?;
    let (pl_start, npl) = b.vector(geo, F_PLACEMENTS, 48)?;
    let (inst_start, ninst) = b.vector(geo, F_INSTANCES, 20)?;
    let mut by_entity: Vec<Vec<usize>> = vec![Vec::new(); nents];
    for k in 0..ninst {
        let e = b.u32(inst_start + 20 * k + 4)? as usize;
        if let Some(v) = by_entity.get_mut(e) {
            v.push(k);
        }
    }
    for (e, insts) in by_entity.iter().enumerate() {
        let mut inner = Entities::default();
        for &k in insts {
            let at = inst_start + 20 * k;
            let (pl, def, mat, layer) =
                (b.u32(at)? as usize, b.u32(at + 8)? as usize, b.u32(at + 12)? as usize, b.u32(at + 16)? as usize);
            let Some(Some(def)) = defs.get(def).copied() else { continue };
            let xform = if pl < npl { placement(b, pl_start + 48 * pl)? } else { DMat4::IDENTITY };
            inner.instances.insert(Instance {
                def,
                xform,
                name: String::new(),
                material: material(&mut m, mat)?,
                layer: layers.get(layer).copied().unwrap_or(0),
                hidden: false,
                locked: false,
                attributes: Default::default(),
                ext: Default::default(),
            });
        }
        count += 1;
        let d = m.defs.insert(Definition {
            name: format!("Component{count}"),
            is_group: false,
            description: String::new(),
            entities: inner,
            attributes: Default::default(),
            ext: Default::default(),
        });
        let xform = placement(b, ent_start + 64 * e + 16)?;
        m.root.instances.insert(Instance {
            def: d,
            xform,
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
    }
    Ok(m)
}

/// The "Résultats de l'importation" of SketchUp's TrimBIM importer.
pub fn report(ok: bool) -> String {
    if ok { "Importation TrimBIM réussie".into() } else { "Échec de l'importation de TrimBIM".into() }
}

#[cfg(test)]
mod tests {
    //! Files are built here with a small FlatBuffers writer; the expected results are what
    //! SketchUp 2026's own TrimBIM importer made of the same files (read back from the .skp
    //! it wrote).

    use super::*;

    /// A FlatBuffers value: a table (fields by index), inline bytes (scalar or struct), a
    /// vector of inline elements, of tables or of strings.
    enum V {
        Tab(Vec<Option<V>>),
        Inline(Vec<u8>),
        Elems(Vec<u8>),
        Tabs(Vec<V>),
        Strs(Vec<String>),
    }

    fn pad(out: &mut Vec<u8>, align: usize) {
        while !out.len().is_multiple_of(align) {
            out.push(0);
        }
    }

    /// Writes `v` after what `out` holds; returns where it starts (tables: the table,
    /// vectors: their length).
    fn write(v: &V, out: &mut Vec<u8>) -> usize {
        match v {
            V::Tab(fields) => {
                let n = fields.len();
                // The vtable, then the table right after it.
                pad(out, 2);
                let vt = out.len();
                out.extend([0u8; 4]);
                out.extend(vec![0u8; 2 * n]);
                pad(out, 8);
                let pos = out.len();
                out.extend(((pos - vt) as i32).to_le_bytes());
                let mut later = Vec::new();
                for (i, f) in fields.iter().enumerate() {
                    let Some(f) = f else { continue };
                    let at = match f {
                        V::Inline(bytes) => {
                            pad(out, bytes.len().clamp(1, 8));
                            let at = out.len();
                            out.extend(bytes);
                            at
                        }
                        _ => {
                            pad(out, 4);
                            let at = out.len();
                            out.extend([0u8; 4]);
                            later.push((at, f));
                            at
                        }
                    };
                    out[vt + 4 + 2 * i..vt + 6 + 2 * i].copy_from_slice(&((at - pos) as u16).to_le_bytes());
                }
                out[vt..vt + 2].copy_from_slice(&((4 + 2 * n) as u16).to_le_bytes());
                let len = (out.len() - pos) as u16;
                out[vt + 2..vt + 4].copy_from_slice(&len.to_le_bytes());
                for (at, f) in later {
                    let child = write(f, out);
                    out[at..at + 4].copy_from_slice(&((child - at) as u32).to_le_bytes());
                }
                pos
            }
            V::Inline(_) => unreachable!("inline values live in tables"),
            V::Elems(bytes) => {
                // The count, then elements aligned for doubles.
                while out.len() % 8 != 4 {
                    out.push(0);
                }
                let pos = out.len();
                out.extend(bytes);
                pos
            }
            V::Tabs(list) => {
                pad(out, 4);
                let pos = out.len();
                out.extend((list.len() as u32).to_le_bytes());
                let slots: Vec<usize> = (0..list.len())
                    .map(|_| {
                        out.extend([0u8; 4]);
                        out.len() - 4
                    })
                    .collect();
                for (t, at) in list.iter().zip(slots) {
                    let child = write(t, out);
                    out[at..at + 4].copy_from_slice(&((child - at) as u32).to_le_bytes());
                }
                pos
            }
            V::Strs(list) => {
                pad(out, 4);
                let pos = out.len();
                out.extend((list.len() as u32).to_le_bytes());
                let slots: Vec<usize> = (0..list.len())
                    .map(|_| {
                        out.extend([0u8; 4]);
                        out.len() - 4
                    })
                    .collect();
                for (s, at) in list.iter().zip(slots) {
                    pad(out, 4);
                    let child = out.len();
                    out.extend((s.len() as u32).to_le_bytes());
                    out.extend(s.as_bytes());
                    out.push(0);
                    out[at..at + 4].copy_from_slice(&((child - at) as u32).to_le_bytes());
                }
                pos
            }
        }
    }

    /// A vector of `count` inline elements: their count, then their bytes.
    fn elems(count: usize, bytes: Vec<u8>) -> V {
        let mut v = (count as u32).to_le_bytes().to_vec();
        v.extend(bytes);
        V::Elems(v)
    }

    fn file(model: V) -> Vec<u8> {
        let mut out = vec![0u8; 8];
        out[4..8].copy_from_slice(b"TRB8");
        let root = write(&model, &mut out);
        out[0..4].copy_from_slice(&(root as u32).to_le_bytes());
        out
    }

    fn f32s(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn placement(o: [f64; 3], x: [f32; 3], y: [f32; 3]) -> Vec<u8> {
        let mut b: Vec<u8> = o.iter().flat_map(|v| v.to_le_bytes()).collect();
        b.extend(f32s(&x));
        b.extend(f32s(&y));
        b
    }

    const ID: ([f64; 3], [f32; 3], [f32; 3]) = ([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);

    fn entity(o: [f64; 3]) -> Vec<u8> {
        let mut b = vec![1u8, 0, 0, 0]; // String identifier
        b.extend([0u8; 12]);
        b.extend(placement(o, ID.1, ID.2));
        b
    }

    fn material(rgba: [u8; 4], texture: u32) -> Vec<u8> {
        let mut b = rgba.to_vec();
        b.extend(texture.to_le_bytes());
        b.extend([1, 0, 0, 0]);
        b.extend(f32s(&[0.0, 1.0]));
        b
    }

    fn definition(kind: u8, index: u32) -> Vec<u8> {
        let mut b = vec![kind, 0, 0, 0];
        b.extend(index.to_le_bytes());
        b.extend([0u8; 24]);
        b
    }

    fn instance(pl: u32, ent: u32, def: u32, mat: u32, layer: u32) -> Vec<u8> {
        [pl, ent, def, mat, layer].iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    fn points(p: &[[f32; 3]]) -> V {
        elems(p.len(), p.iter().flat_map(|x| f32s(x)).collect())
    }

    /// A triangle mesh whose vertices are its positions (one normal).
    fn tri_mesh(p: &[[f32; 3]], tris: &[u16]) -> V {
        let verts: Vec<u8> = (0..p.len() as u16).flat_map(|i| [i.to_le_bytes(), 0u16.to_le_bytes()].concat()).collect();
        V::Tab(vec![
            Some(points(p)),
            Some(points(&[[0.0, 0.0, 1.0]])),
            Some(elems(p.len(), verts)),
            Some(elems(tris.len(), tris.iter().flat_map(|i| i.to_le_bytes()).collect())),
        ])
    }

    fn wire(idx: &[u16]) -> V {
        V::Tab(vec![Some(elems(idx.len(), idx.iter().flat_map(|i| i.to_le_bytes()).collect()))])
    }

    struct Geo {
        materials: Vec<Vec<u8>>,
        layers: Vec<&'static str>,
        placements: Vec<Vec<u8>>,
        definitions: Vec<Vec<u8>>,
        instances: Vec<Vec<u8>>,
        fields: Vec<(usize, V)>,
        entities: Vec<Vec<u8>>,
    }

    fn model(g: Geo) -> Vec<u8> {
        let cat = |v: &[Vec<u8>]| v.concat();
        // Every vector the schema requires, empty unless given.
        let empty = |i: usize| match i {
            F_LAYERS => V::Strs(vec![]),
            F_TEXTURES | F_BREPS | F_MESHES | F_MESHES8 | F_TEX_MESHES | F_TEX_MESHES8 | 11..=22 => V::Tabs(vec![]),
            _ => elems(0, vec![]),
        };
        let mut geo: Vec<Option<V>> = (0..23).map(|i| Some(empty(i))).collect();
        geo[F_PLACEMENTS] = Some(elems(g.placements.len(), cat(&g.placements)));
        geo[F_MATERIALS] = Some(elems(g.materials.len(), cat(&g.materials)));
        geo[F_LAYERS] = Some(V::Strs(g.layers.iter().map(|s| s.to_string()).collect()));
        geo[F_DEFINITIONS] = Some(elems(g.definitions.len(), cat(&g.definitions)));
        geo[F_INSTANCES] = Some(elems(g.instances.len(), cat(&g.instances)));
        for (i, v) in g.fields {
            geo[i] = Some(v);
        }
        let n = g.entities.len();
        let ents = V::Tab(vec![
            Some(elems(n, cat(&g.entities))),
            Some(elems(0, vec![])),                                   // hierarchies
            Some(elems(0, vec![])),                                   // GUIDs
            Some(V::Strs((0..n).map(|i| format!("E{i}")).collect())), // string identifiers
            Some(elems(0, vec![])),                                   // spatial hashes
            Some(elems(0, vec![])),                                   // DWG handles
            Some(V::Strs(vec!["IfcBuildingElementProxy".into()])),
        ]);
        let properties = V::Tab(
            (0..19)
                .map(|i| Some(if [4, 5, 6, 8, 9, 16].contains(&i) { V::Strs(vec![]) } else { elems(0, vec![]) }))
                .collect(),
        );
        let bytes = file(V::Tab(vec![
            Some(ents),
            Some(properties),
            Some(V::Tab(geo)),
            Some(V::Tabs(vec![])),
            Some(V::Tabs(vec![])),
        ]));
        // ESQ_TRB_DIR keeps the files, to open them elsewhere.
        if let Ok(d) = std::env::var("ESQ_TRB_DIR") {
            std::fs::write(std::path::Path::new(&d).join(format!("test-{}.trb", bytes.len())), &bytes).unwrap();
        }
        bytes
    }

    fn def<'m>(m: &'m Model, name: &str) -> &'m Definition {
        m.defs.values().find(|d| d.name == name).unwrap_or_else(|| panic!("{name}"))
    }

    fn edges(e: &Entities) -> (usize, usize, usize) {
        let v: Vec<_> = e.edges.values().collect();
        (v.len(), v.iter().filter(|x| x.soft).count(), v.iter().filter(|x| x.hidden).count())
    }

    const SQUARE: [[f32; 3]; 4] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];

    #[test]
    fn components_in_metres_with_materials_and_tags() {
        let bytes = model(Geo {
            materials: vec![material([255, 0, 0, 255], NONE), material([0, 0, 255, 128], NONE)],
            layers: vec!["Murs", "Portes"],
            placements: vec![
                placement(ID.0, ID.1, ID.2),
                placement([5.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]),
            ],
            definitions: vec![definition(G_MESH, 0), definition(G_BREP, 0)],
            instances: vec![instance(0, 0, 0, 0, 0), instance(1, 0, 0, 1, 0), instance(0, 1, 1, 1, 1)],
            fields: vec![
                (F_MESHES, V::Tabs(vec![tri_mesh(&SQUARE, &[0, 1, 2, 0, 2, 3])])),
                (
                    F_BREPS,
                    V::Tabs(vec![V::Tab(vec![
                        Some(V::Tabs(vec![wire(&[0, 1, 2, 3])])),
                        Some(V::Tabs(vec![V::Tab(vec![
                            Some(V::Inline(0u16.to_le_bytes().to_vec())),
                            Some(elems(4, [4u16, 7, 6, 5].iter().flat_map(|i| i.to_le_bytes()).collect())),
                        ])])),
                        Some(points(&[
                            [0.0, 0.0, 0.0],
                            [2.0, 0.0, 0.0],
                            [2.0, 2.0, 0.0],
                            [0.0, 2.0, 0.0],
                            [0.5, 0.5, 0.0],
                            [1.5, 0.5, 0.0],
                            [1.5, 1.5, 0.0],
                            [0.5, 1.5, 0.0],
                        ])),
                    ])]),
                ),
            ],
            entities: vec![entity([0.0; 3]), entity([0.0; 3])],
        });

        let m = import(&bytes).unwrap();
        assert_eq!(m.units, Units::Meters);
        let names: Vec<&str> = m.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names[1..], ["Murs", "Portes"]);
        // One component per entity at the root, in order.
        let roots: Vec<&str> = m.root.instances.values().map(|i| m.defs[i.def].name.as_str()).collect();
        assert_eq!(roots, ["Component3", "Component4"]);
        // The mesh, used twice: its triangles, all edges softened; placed and painted.
        let mesh = def(&m, "Component1");
        assert_eq!(mesh.entities.faces.len(), 2);
        assert_eq!(edges(&mesh.entities), (5, 5, 0));
        let wall = def(&m, "Component3");
        let insts: Vec<_> = wall.entities.instances.values().collect();
        assert_eq!(insts.len(), 2);
        let red = &m.materials[insts[0].material.unwrap()];
        assert_eq!((red.name.as_str(), red.color, red.opacity), ("<auto>1", [255, 0, 0], 1.0));
        let blue = &m.materials[insts[1].material.unwrap()];
        assert_eq!((blue.name.as_str(), blue.color), ("<auto>2", [0, 0, 255]));
        assert!((blue.opacity - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(m.layers[insts[0].layer].name, "Murs");
        let p = insts[1].xform.transform_point3(DVec3::new(1000.0, 0.0, 0.0));
        assert!(p.distance(DVec3::new(5000.0, 1000.0, 0.0)) < 1e-6, "{p}");
        // The B-rep: its polygon with its hole.
        let door = def(&m, "Component2");
        let f = door.entities.faces.values().next().unwrap();
        assert_eq!((door.entities.faces.len(), f.outer.len(), f.holes.len()), (1, 4, 1));
        assert_eq!(edges(&door.entities), (8, 8, 0));
        let area = esq_core::tess::face_area(&door.entities, door.entities.faces.keys().next().unwrap());
        assert!((area - 3.0e6).abs() < 1.0, "{area}");
    }

    /// A fold between two triangles turned by `deg` degrees.
    fn fold(deg: f64) -> V {
        let r = deg.to_radians();
        let p = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.5, 1.0, 0.0], [0.5, -r.cos() as f32, r.sin() as f32]];
        tri_mesh(&p, &[0, 1, 2, 1, 0, 3])
    }

    #[test]
    fn edges_stay_visible_from_17_56_degrees() {
        let cube: Vec<[f32; 3]> = (0..8)
            .map(|i| {
                let (x, y) = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)][i % 4];
                [x, y, (i / 4) as f32]
            })
            .collect();
        let quads: [[u16; 4]; 6] = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]];
        let tris: Vec<u16> = quads.iter().flat_map(|q| [q[0], q[1], q[2], q[0], q[2], q[3]]).collect();
        let bytes = model(Geo {
            materials: vec![material([200, 200, 200, 255], NONE)],
            layers: vec!["L"],
            placements: vec![placement(ID.0, ID.1, ID.2)],
            definitions: vec![
                definition(G_MESH, 0),
                definition(G_MESH, 1),
                definition(G_MESH, 2),
                definition(G_BREP, 0),
            ],
            instances: (0..4).map(|d| instance(0, 0, d, 0, 0)).collect(),
            fields: vec![
                (F_MESHES, V::Tabs(vec![fold(17.5), fold(17.6), tri_mesh(&cube, &tris)])),
                (
                    F_BREPS,
                    V::Tabs(vec![V::Tab(vec![
                        Some(V::Tabs(quads.iter().map(|q| wire(q)).collect())),
                        Some(V::Tabs(vec![])),
                        Some(points(&cube)),
                    ])]),
                ),
            ],
            entities: vec![entity([0.0; 3])],
        });
        let m = import(&bytes).unwrap();
        // Turned by 17.5°: softened; by 17.6°: the fold shows. Boundaries are softened.
        assert_eq!(edges(&def(&m, "Component1").entities), (5, 5, 0));
        assert_eq!(edges(&def(&m, "Component2").entities), (5, 4, 0));
        // A cube of triangles: only the diagonals are soft; as a B-rep: six squares.
        assert_eq!(edges(&def(&m, "Component3").entities), (18, 6, 0));
        let b = &def(&m, "Component4").entities;
        assert_eq!((b.faces.len(), edges(b)), (6, (12, 0, 0)));
    }

    #[test]
    fn textures_placements_lines_and_what_is_left_out() {
        let mut png = Vec::new();
        let img = image::RgbImage::from_fn(2, 2, |x, y| {
            image::Rgb([[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]][(x + 2 * y) as usize])
        });
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let p = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [4.0, 2.0, 0.0], [0.0, 2.0, 0.0], [4.0, 1.0, 1.0]];
        let verts: Vec<u8> = (0..5u16).flat_map(|i| [i.to_le_bytes(), 0u16.to_le_bytes()].concat()).collect();
        let tris: Vec<u8> = [0u16, 1, 2, 0, 2, 3, 1, 4, 2].iter().flat_map(|i| i.to_le_bytes()).collect();
        let textured = V::Tab(vec![
            Some(points(&p)),
            Some(points(&[[0.0, 0.0, 1.0]])),
            Some(elems(5, f32s(&[0.0, 0.0, 2.0, 0.0, 2.0, 1.0, 0.0, 1.0, 1.0, 1.0]))),
            Some(elems(5, verts)),
            Some(elems(9, tris)),
        ]);
        let gfx = V::Tab(vec![
            Some(elems(1, f32s(&[0.0, 0.0, 2.0, 1.0, 0.0, 2.0]))),
            Some(elems(1, f32s(&[0.0, 0.0, 3.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, std::f32::consts::FRAC_PI_2]))),
        ]);
        // A pipe along Z: a directrix of one line segment, radius 10 cm.
        let swept = V::Tab(vec![
            Some(V::Tabs(vec![V::Tab(vec![
                Some(elems(1, vec![0])),
                Some(elems(1, 0u32.to_le_bytes().to_vec())),
                Some(elems(1, f32s(&[0.0, 0.0, 0.0, 0.0, 0.0, 5.0]))),
                Some(V::Tabs(vec![])),
                Some(elems(0, vec![])),
            ])])),
            Some(elems(1, 0.1f64.to_le_bytes().to_vec())),
        ]);
        let bytes = model(Geo {
            materials: vec![material([200, 200, 200, 255], 0)],
            layers: vec!["L"],
            placements: vec![placement(ID.0, ID.1, ID.2)],
            // A swept disk (left out), a textured mesh, a graphics container.
            definitions: vec![definition(4, 0), definition(G_TEX_MESH, 0), definition(G_GRAPHICS, 0)],
            instances: vec![instance(0, 0, 0, 0, 0), instance(0, 0, 1, 0, 0), instance(0, 0, 2, 0, 0)],
            fields: vec![
                (F_TEXTURES, V::Tabs(vec![V::Tab(vec![Some(V::Inline(vec![1])), Some(elems(png.len(), png))])])),
                (F_TEX_MESHES, V::Tabs(vec![textured])),
                (F_GRAPHICS, V::Tabs(vec![gfx])),
                (11, V::Tabs(vec![swept])),
            ],
            // The second entity has no geometry: an empty component all the same.
            entities: vec![entity([100.0, 0.0, 0.0]), entity([0.0; 3])],
        });
        let m = import(&bytes).unwrap();
        let roots: Vec<_> = m.root.instances.values().collect();
        assert_eq!(roots.len(), 2);
        assert_eq!(m.defs[roots[0].def].name, "Component3");
        assert!(roots[0].xform.w_axis.truncate().distance(DVec3::new(100_000.0, 0.0, 0.0)) < 1e-6);
        assert!(m.defs[roots[1].def].entities.is_empty());
        // The textured mesh: every edge hidden; the image (no alpha: kept in JPEG) tiled every
        // inch, its average as colour.
        let t = &def(&m, "Component1").entities;
        assert_eq!((t.faces.len(), edges(t)), (3, (7, 0, 7)));
        let inst = m.defs[roots[0].def].entities.instances.values().next().unwrap();
        let mat = &m.materials[inst.material.unwrap()];
        let tex = mat.ext.texture.as_ref().unwrap();
        assert_eq!((tex.filename.as_str(), tex.px, tex.size), ("Texture.jpg", [2, 2], [25.4, 25.4]));
        assert_eq!(mat.color, [127, 127, 127]);
        // The line and the quarter circle in 5° steps.
        let g = &def(&m, "Component2").entities;
        assert_eq!((g.faces.len(), g.edges.len()), (0, 1 + 18));
    }

    /// A graphics container with an arc of 3·10^38 radians: as many 5° segments, made at once.
    #[test]
    fn a_huge_arc_angle_is_one_turn() {
        let gfx = V::Tab(vec![
            Some(elems(0, vec![])),
            Some(elems(1, f32s(&[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, f32::MAX]))),
        ]);
        let bytes = model(Geo {
            materials: vec![material([200, 200, 200, 255], 0)],
            layers: vec!["L"],
            placements: vec![placement(ID.0, ID.1, ID.2)],
            definitions: vec![definition(G_GRAPHICS, 0)],
            instances: vec![instance(0, 0, 0, 0, 0)],
            fields: vec![(F_GRAPHICS, V::Tabs(vec![gfx]))],
            entities: vec![entity([0.0; 3])],
        });
        let m = import(&bytes).unwrap();
        let n: usize = m.defs.values().map(|d| d.entities.edges.len()).sum();
        assert_eq!(n, 72, "a full circle in 5° steps");
    }

    #[test]
    fn other_files_are_refused() {
        assert!(import(b"PK\x03\x04 pas un TRB").is_err());
        assert!(import(&[0, 0, 0, 0, b'T', b'R', b'B', b'8']).is_err());
        let mut truncated = model(Geo {
            materials: vec![],
            layers: vec![],
            placements: vec![],
            definitions: vec![],
            instances: vec![],
            fields: vec![],
            entities: vec![entity([0.0; 3])],
        });
        assert!(import(&truncated).is_ok());
        truncated.truncate(truncated.len() / 2);
        assert!(import(&truncated).is_err());
    }
}
