//! 3D AutoCAD export (File › Export › 3D Model › DWG / DXF), as SketchUp's AutoCAD exporter
//! writes it: tags become layers, groups and components blocks inserted where they stand,
//! faces polyface meshes, edges lines (arcs and circles stay arcs and circles, other
//! curves 3D polylines), construction geometry xlines, lines and points, dimensions and
//! texts, material colours on the entities; coordinates in the model's units.
//!
//! The drawing is first described by [`CadDoc`]; `acad_dwg.rs` makes it the drawing of
//! `esq_core::dwg`, which writes it as DWG or as DXF ([`write_dxf`]) in every version from
//! Release 12 to 2018: the DXF has the entities of the DWG.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use esq_core::glam::{DMat4, DVec3};
use esq_core::model::{AnnotKind, DefId, EdgeId, Entities, LayerIdx, MaterialId, Model, Units};

use crate::options::{AcadExport, AcadVersion};
use crate::scene::{Opts, triangles};

/// What an entity is.
#[derive(Clone, Debug, PartialEq)]
pub enum CadEnt {
    Line {
        a: DVec3,
        b: DVec3,
    },
    Polyline3d {
        pts: Vec<DVec3>,
        closed: bool,
    },
    /// Angles in radians, counter-clockwise about `normal` from the arbitrary-axis X axis.
    Arc {
        center: DVec3,
        normal: DVec3,
        radius: f64,
        start: f64,
        end: f64,
    },
    Circle {
        center: DVec3,
        normal: DVec3,
        radius: f64,
    },
    /// 1-based vertex indices; a negative index hides the edge that starts there.
    PolyFace {
        verts: Vec<DVec3>,
        faces: Vec<[i32; 4]>,
    },
    Point {
        p: DVec3,
    },
    XLine {
        p: DVec3,
        dir: DVec3,
    },
    MText {
        p: DVec3,
        text: String,
        height: f64,
    },
    Leader {
        pts: Vec<DVec3>,
    },
    /// An aligned dimension between `a` and `b`, its line through `line`.
    Dimension {
        a: DVec3,
        b: DVec3,
        line: DVec3,
        text: String,
    },
    /// A block reference: insertion point and extrusion in world terms of the parent.
    Insert {
        block: String,
        xform: DMat4,
    },
}

#[derive(Clone, Debug)]
pub struct CadItem {
    pub ent: CadEnt,
    pub layer: String,
    pub color: Option<[u8; 3]>,
}

#[derive(Clone, Debug, Default)]
pub struct CadLayer {
    pub name: String,
    pub color: Option<[u8; 3]>,
    pub visible: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CadDoc {
    pub layers: Vec<CadLayer>,
    /// Blocks in dependency order (a block comes after the blocks it inserts).
    pub blocks: Vec<(String, Vec<CadItem>)>,
    pub model: Vec<CadItem>,
    pub units: Units,
}

/// The layer name of a tag ("0" for Untagged).
fn layer_name(model: &Model, l: LayerIdx) -> String {
    if l == 0 { "0".into() } else { model.layers.get(l).map_or("0".into(), |x| clean(&x.name)) }
}

/// AutoCAD names: no `<>/\":;?*|,=` characters.
pub fn clean(s: &str) -> String {
    let c: String = s.chars().map(|c| if "<>/\\\":;?*|,=`".contains(c) || c.is_control() { '_' } else { c }).collect();
    if c.trim().is_empty() { "_".into() } else { c }
}

/// An insert placement: (insertion point in OCS, x/y/z scale, rotation, normal), when the
/// transform has no shear.
pub fn insert_params(m: &DMat4) -> Option<(DVec3, DVec3, f64, DVec3)> {
    let (x, y, z) = (m.x_axis.truncate(), m.y_axis.truncate(), m.z_axis.truncate());
    let (sx, sy, sz) = (x.length(), y.length(), z.length());
    if sx < 1e-12 || sy < 1e-12 || sz < 1e-12 {
        return None;
    }
    let (xn, yn, zn) = (x / sx, y / sy, z / sz);
    if xn.dot(yn).abs() > 1e-6 || xn.dot(zn).abs() > 1e-6 || yn.dot(zn).abs() > 1e-6 {
        return None;
    }
    // A mirrored transform: the Y scale is negative.
    let sy = if xn.cross(yn).dot(zn) < 0.0 { -sy } else { sy };
    let (ax, ay) = arbitrary_axes(zn);
    let rotation = xn.dot(ay).atan2(xn.dot(ax));
    let t = m.w_axis.truncate();
    let ocs = DVec3::new(t.dot(ax), t.dot(ay), t.dot(zn));
    Some((ocs, DVec3::new(sx, sy, sz), rotation, zn))
}

/// DXF's arbitrary axis algorithm: the OCS X and Y axes of an extrusion direction.
pub fn arbitrary_axes(n: DVec3) -> (DVec3, DVec3) {
    let n = n.normalize_or(DVec3::Z);
    let ax = if n.x.abs() < 1.0 / 64.0 && n.y.abs() < 1.0 / 64.0 { DVec3::Y.cross(n) } else { DVec3::Z.cross(n) }
        .normalize();
    (ax, n.cross(ax).normalize())
}

/// A chain of edges that lies on one circle: (center, normal, radius, closed, points).
fn as_circle(pts: &[DVec3], closed: bool) -> Option<(DVec3, DVec3, f64)> {
    if pts.len() < (if closed { 6 } else { 4 }) {
        return None;
    }
    let (a, b, c) = (pts[0], pts[pts.len() / 3], pts[2 * pts.len() / 3]);
    let n = (b - a).cross(c - a);
    if n.length() < 1e-9 {
        return None;
    }
    let n = n.normalize();
    // Circumcentre of a, b, c.
    let (ab, ac) = (b - a, c - a);
    let d = 2.0 * ab.cross(ac).length_squared();
    let center = a + (ac.length_squared() * ab.cross(ac).cross(ab) + ab.length_squared() * ac.cross(ab.cross(ac))) / d;
    let r = center.distance(a);
    let tol = 1e-6 * r.max(1.0);
    if pts.iter().all(|p| (p.distance(center) - r).abs() < tol && (*p - center).dot(n).abs() < tol) {
        Some((center, n, r))
    } else {
        None
    }
}

/// A polyface being assembled: vertices, their index by position, faces.
type MeshAcc = (Vec<DVec3>, HashMap<[u64; 3], i32>, Vec<[i32; 4]>);

struct Builder<'a> {
    model: &'a Model,
    o: &'a AcadExport,
    /// Millimetres per drawing unit.
    k: f64,
    blocks: Vec<(String, Vec<CadItem>)>,
    block_names: HashMap<(DefId, Option<MaterialId>), String>,
    taken: HashSet<String>,
    opts: Opts,
}

impl Builder<'_> {
    fn color(&self, m: Option<MaterialId>) -> Option<[u8; 3]> {
        if !self.o.materials {
            return None;
        }
        m.and_then(|m| self.model.materials.get(m)).map(|x| x.color)
    }

    /// Entities of a collection, placed by `xform` (identity inside a block).
    fn collection(
        &mut self,
        ents: &Entities,
        xform: DMat4,
        inherited: Option<MaterialId>,
        out: &mut Vec<CadItem>,
        depth: usize,
    ) {
        let model = self.model;
        let k = self.k;
        let p = |q: DVec3| xform.transform_point3(q) / k;
        let show_hidden = self.opts.hidden;
        let visible =
            |hidden: bool, l: LayerIdx| show_hidden || (!hidden && model.layers.get(l).is_none_or(|x| x.visible));
        if self.o.faces {
            // Faces by (layer, colour) → polyface meshes.
            let mut groups: BTreeMap<(LayerIdx, Option<[u8; 3]>), MeshAcc> = BTreeMap::new();
            let flip = xform.determinant() < 0.0;
            for face in ents.faces.values() {
                if !visible(face.hidden, face.layer) {
                    continue;
                }
                let color = self.color(face.front.or(inherited));
                let g = groups.entry((face.layer, color)).or_default();
                let outer: Vec<DVec3> = face.outer.iter().map(|v| p(ents.verts[*v].pos)).collect();
                let holes: Vec<Vec<DVec3>> =
                    face.holes.iter().map(|h| h.iter().map(|v| p(ents.verts[*v].pos)).collect()).collect();
                let idx = |q: DVec3, g: &mut MeshAcc| -> i32 {
                    *g.1.entry(q.to_array().map(f64::to_bits)).or_insert_with(|| {
                        g.0.push(q);
                        g.0.len() as i32
                    })
                };
                if holes.is_empty() && outer.len() <= 4 {
                    let mut ids: Vec<i32> = outer.iter().map(|q| idx(*q, g)).collect();
                    if flip {
                        ids.reverse();
                    }
                    let mut f = [0; 4];
                    f[..ids.len()].copy_from_slice(&ids);
                    g.2.push(f);
                } else {
                    // Larger faces in triangles; the inner edges are hidden.
                    let all: Vec<DVec3> = outer.iter().chain(holes.iter().flatten()).copied().collect();
                    let n = xform.transform_vector3(face.normal);
                    let mut boundary = HashSet::new();
                    let mut base = 0;
                    for l in std::iter::once(&outer).chain(holes.iter()) {
                        for i in 0..l.len() {
                            let (a, b) = (base + i, base + (i + 1) % l.len());
                            boundary.insert((a.min(b), a.max(b)));
                        }
                        base += l.len();
                    }
                    for t in triangles(&outer, &holes, n) {
                        let mut f = [0; 4];
                        for (j, &c) in t.iter().enumerate() {
                            let next = t[(j + 1) % 3];
                            let id = idx(all[c], g);
                            f[j] = if boundary.contains(&(c.min(next), c.max(next))) { id } else { -id };
                        }
                        g.2.push(f);
                    }
                }
            }
            for ((layer, color), (verts, _, faces)) in groups {
                if !faces.is_empty() {
                    out.push(CadItem {
                        ent: CadEnt::PolyFace { verts, faces },
                        layer: layer_name(model, layer),
                        color,
                    });
                }
            }
        }
        if self.o.edges {
            // Curves (edges sharing a curve id) first: arcs, circles, 3D polylines.
            let mut by_curve: BTreeMap<u32, Vec<EdgeId>> = BTreeMap::new();
            let mut single = Vec::new();
            for (id, e) in ents.edges.iter() {
                if !visible(e.hidden, e.layer) || e.soft || e.smooth {
                    continue;
                }
                match e.curve {
                    Some(c) => by_curve.entry(c).or_default().push(id),
                    None => single.push(id),
                }
            }
            for (_, list) in by_curve {
                let layer = layer_name(model, ents.edges[list[0]].layer);
                match chain(ents, &list) {
                    Some((verts, closed)) => {
                        let pts: Vec<DVec3> = verts.iter().map(|q| p(*q)).collect();
                        let ent = match as_circle(&pts, closed) {
                            Some((center, normal, radius)) if closed => CadEnt::Circle { center, normal, radius },
                            Some((center, normal, radius)) => {
                                let (ax, ay) = arbitrary_axes(normal);
                                let ang = |q: DVec3| (q - center).dot(ay).atan2((q - center).dot(ax));
                                // Counter-clockwise about the normal from the first to the last point.
                                let mid = pts[pts.len() / 2];
                                let (s, e, m) = (ang(pts[0]), ang(*pts.last().unwrap()), ang(mid));
                                let ccw = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
                                if ccw(s, m) < ccw(s, e) {
                                    CadEnt::Arc { center, normal, radius, start: s, end: e }
                                } else {
                                    CadEnt::Arc { center, normal, radius, start: e, end: s }
                                }
                            }
                            None => CadEnt::Polyline3d { pts, closed },
                        };
                        out.push(CadItem { ent, layer, color: None });
                    }
                    None => single.extend(list),
                }
            }
            for id in single {
                let (a, b) = ents.edge_points(id);
                out.push(CadItem {
                    ent: CadEnt::Line { a: p(a), b: p(b) },
                    layer: layer_name(model, ents.edges[id].layer),
                    color: None,
                });
            }
        }
        if self.o.construction {
            for g in ents.guides.values() {
                if !visible(g.hidden, g.layer) {
                    continue;
                }
                let ent = if g.is_point() {
                    CadEnt::Point { p: p(g.pos) }
                } else if g.infinite {
                    CadEnt::XLine { p: p(g.pos), dir: xform.transform_vector3(g.dir).normalize_or(DVec3::X) }
                } else {
                    CadEnt::Line { a: p(g.pos), b: p(g.pos + g.dir) }
                };
                out.push(CadItem { ent, layer: layer_name(model, g.layer), color: None });
            }
        }
        for a in ents.annots.values() {
            if !visible(a.hidden, a.layer) {
                continue;
            }
            let layer = layer_name(model, a.layer);
            match &a.kind {
                AnnotKind::Linear { a: pa, b: pb, offset } if self.o.dimensions => out.push(CadItem {
                    ent: CadEnt::Dimension { a: p(*pa), b: p(*pb), line: p(*pb + *offset), text: a.text.clone() },
                    layer,
                    color: None,
                }),
                AnnotKind::Text { point, leader } if self.o.text => {
                    let at = p(*point + *leader);
                    if leader.length() > 1e-9 {
                        out.push(CadItem {
                            ent: CadEnt::Leader { pts: vec![p(*point), at] },
                            layer: layer.clone(),
                            color: None,
                        });
                    }
                    out.push(CadItem {
                        ent: CadEnt::MText { p: at, text: a.text.clone(), height: 120.0 / self.k },
                        layer,
                        color: None,
                    });
                }
                _ => {}
            }
        }
        if depth > 64 {
            return;
        }
        for inst in ents.instances.values() {
            if !visible(inst.hidden, inst.layer) || !model.defs.contains_key(inst.def) {
                continue;
            }
            let inh = inst.material.or(inherited);
            let m = xform * inst.xform;
            // In drawing units.
            let mut mu = m;
            mu.w_axis.x /= self.k;
            mu.w_axis.y /= self.k;
            mu.w_axis.z /= self.k;
            if insert_params(&inst.xform).is_some() && insert_params(&mu).is_some() {
                let block = self.block(inst.def, inh, depth + 1);
                let local = {
                    let mut l = inst.xform;
                    l.w_axis.x /= self.k;
                    l.w_axis.y /= self.k;
                    l.w_axis.z /= self.k;
                    // Inside a block the parent transform is the identity; at the top it is `xform`.
                    let mut px = xform;
                    px.w_axis.x /= self.k;
                    px.w_axis.y /= self.k;
                    px.w_axis.z /= self.k;
                    px * l
                };
                out.push(CadItem {
                    ent: CadEnt::Insert { block, xform: local },
                    layer: layer_name(model, inst.layer),
                    color: self.color(inst.material),
                });
            } else {
                // Sheared: written in place.
                let d = &model.defs[inst.def];
                self.collection(&d.entities, m, inh, out, depth + 1);
            }
        }
    }

    fn block(&mut self, def: DefId, inherited: Option<MaterialId>, depth: usize) -> String {
        if let Some(n) = self.block_names.get(&(def, inherited)) {
            return n.clone();
        }
        let d = &self.model.defs[def];
        let base = clean(&d.name);
        let mut name = base.clone();
        let mut i = 1;
        while self.taken.contains(&name.to_lowercase()) {
            name = format!("{base}_{i}");
            i += 1;
        }
        self.taken.insert(name.to_lowercase());
        self.block_names.insert((def, inherited), name.clone());
        let mut items = Vec::new();
        let ents = &self.model.defs[def].entities;
        self.collection(ents, DMat4::IDENTITY, inherited, &mut items, depth);
        self.blocks.push((name.clone(), items));
        name
    }
}

/// The edges of a curve in order: points and whether the chain closes.
fn chain(ents: &Entities, list: &[EdgeId]) -> Option<(Vec<DVec3>, bool)> {
    let mut adj: HashMap<esq_core::model::VertexId, Vec<esq_core::model::VertexId>> = HashMap::new();
    for &e in list {
        let [a, b] = ents.edges[e].v;
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    }
    if adj.values().any(|v| v.len() > 2) {
        return None;
    }
    let start = adj.iter().find(|(_, v)| v.len() == 1).map(|(k, _)| *k).or_else(|| adj.keys().next().copied())?;
    let closed = adj.values().all(|v| v.len() == 2);
    let mut out = vec![start];
    let mut prev = None;
    let mut cur = start;
    loop {
        let next = adj[&cur].iter().copied().find(|v| Some(*v) != prev && (*v != start || !closed));
        match next {
            Some(n) if !out.contains(&n) => {
                prev = Some(cur);
                cur = n;
                out.push(n);
            }
            _ => break,
        }
    }
    if out.len() != adj.len() {
        return None;
    }
    Some((out.iter().map(|v| ents.verts[*v].pos).collect(), closed))
}

/// The drawing of a model.
pub fn document(model: &Model, o: &AcadExport) -> CadDoc {
    let mut b = Builder {
        model,
        o,
        k: model.units.factor(),
        blocks: Vec::new(),
        block_names: HashMap::new(),
        taken: HashSet::from(["*model_space".into(), "*paper_space".into()]),
        opts: Opts::default(),
    };
    let mut items = Vec::new();
    b.collection(&model.root, DMat4::IDENTITY, None, &mut items, 0);
    let mut layers = vec![CadLayer { name: "0".into(), color: None, visible: true }];
    for (i, l) in model.layers.iter().enumerate().skip(1) {
        layers.push(CadLayer { name: layer_name(model, i), color: l.color, visible: l.visible });
    }
    CadDoc { layers, blocks: b.blocks, model: items, units: model.units }
}

// ------------------------------------------------------------------------------------------
// DXF

/// The nearest AutoCAD colour index of an RGB colour.
pub fn nearest_aci(c: [u8; 3]) -> u8 {
    let mut best = (7u8, u32::MAX);
    for i in 1..=255u8 {
        let (r, g, b) = cadkit_core::aci::to_rgb(i);
        let d = (r as i32 - c[0] as i32).pow(2) + (g as i32 - c[1] as i32).pow(2) + (b as i32 - c[2] as i32).pow(2);
        if (d as u32) < best.1 {
            best = (i, d as u32);
        }
    }
    best.0
}

/// The DXF text of a drawing in `version`: the entities of its DWG
/// ([`crate::acad_dwg::dwg_doc`]), by the DXF writer of `esq_core::dwg`.
pub fn write_dxf(doc: &CadDoc, version: AcadVersion) -> Result<Vec<u8>> {
    esq_core::dwg::write_dxf(&crate::acad_dwg::dwg_doc(doc), version.dwg())
}

/// File › Export › 3D Model › DXF.
pub fn export_dxf(model: &Model, path: &std::path::Path, o: &AcadExport) -> Result<()> {
    let doc = document(model, o);
    std::fs::write(path, write_dxf(&doc, o.version)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::model::{Definition, Instance};

    fn model() -> Model {
        let mut m = Model::new();
        m.units = Units::Millimeters;
        let mut e = Entities::default();
        let v: Vec<_> = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 500.0), (0.0, 500.0)]
            .iter()
            .map(|(x, y)| e.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
            .collect();
        e.add_face_raw(v, vec![]);
        // A circle as a curve of 24 edges.
        let pts: Vec<DVec3> = (0..24)
            .map(|i| {
                let a = i as f64 * std::f64::consts::TAU / 24.0;
                DVec3::new(3000.0 + 400.0 * a.cos(), 400.0 * a.sin(), 0.0)
            })
            .collect();
        e.add_polyline(&pts, true, true);
        let red = m.add_material("Rouge", [255, 0, 0]);
        let tag = m.add_layer("Mobilier");
        let d = m.defs.insert(Definition {
            name: "Table".into(),
            is_group: false,
            description: String::new(),
            entities: e,
            attributes: Default::default(),
            ext: Default::default(),
        });
        m.root.instances.insert(Instance {
            def: d,
            xform: DMat4::from_translation(DVec3::new(0.0, 2000.0, 0.0)) * DMat4::from_rotation_z(0.5),
            name: String::new(),
            material: Some(red),
            layer: tag,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
        m.root.guides.insert(esq_core::model::Guide::line(DVec3::ZERO, DVec3::X));
        m
    }

    #[test]
    fn drawing_has_layers_blocks_polyfaces_and_circles() {
        let m = model();
        let doc = document(&m, &AcadExport { construction: true, ..Default::default() });
        assert!(doc.layers.iter().any(|l| l.name == "Mobilier"));
        assert_eq!(doc.blocks.len(), 1);
        let (name, items) = &doc.blocks[0];
        assert_eq!(name, "Table");
        assert!(items.iter().any(|i| matches!(i.ent, CadEnt::PolyFace { .. }) && i.color == Some([255, 0, 0])));
        assert!(items.iter().any(|i| matches!(&i.ent, CadEnt::Circle { radius, .. } if (radius - 400.0).abs() < 1e-6)));
        assert_eq!(items.iter().filter(|i| matches!(i.ent, CadEnt::Line { .. })).count(), 4);
        assert!(doc.model.iter().any(|i| matches!(i.ent, CadEnt::Insert { .. }) && i.layer == "Mobilier"));
        assert!(doc.model.iter().any(|i| matches!(i.ent, CadEnt::XLine { .. })));
        let (ocs, s, rot, n) = insert_params(&DMat4::from_rotation_z(0.5)).unwrap();
        assert!((rot - 0.5).abs() < 1e-12 && n == DVec3::Z && s == DVec3::ONE && ocs == DVec3::ZERO);
    }

    #[test]
    fn dxf_of_every_version_reads_back() {
        let m = model();
        for v in AcadVersion::ALL {
            let doc = document(&m, &AcadExport { version: *v, ..Default::default() });
            let bytes = write_dxf(&doc, *v).unwrap();
            let back = crate::cad::read(&bytes).unwrap_or_else(|e| panic!("{v:?}: {e:#}"));
            // (Names in capitals up to release 14, as SketchUp's exporter writes them.)
            assert!(back.blocks.iter().any(|b| b.name == v.dwg().table_name("Table")), "{v:?}");
            let ms = &back.models[0];
            assert!(ms.entities.iter().any(|e| matches!(e.kind, cadkit_core::EntityKind::Insert { .. })), "{v:?}");
            // Through the importer: the table comes back as a component with a 1 m × 0.5 m face.
            let (imp, _) = crate::cad::import(
                &bytes,
                &crate::options::DwgImport { preserve_origin: true, ..Default::default() },
                Units::Millimeters,
            )
            .unwrap();
            let d =
                imp.defs.values().find(|d| d.name == v.dwg().table_name("Table")).unwrap_or_else(|| panic!("{v:?}"));
            let f = d.entities.faces.keys().next().unwrap_or_else(|| panic!("{v:?}: aucune face"));
            let area = esq_core::tess::face_area(&d.entities, f);
            assert!((area - 500_000.0).abs() < 1.0, "{v:?} {area}");
        }
    }
}
