//! AutoCAD drawings (.dwg R13–2018, .dxf R12–2018), read with the clean-room `cadkit`
//! readers and turned into SketchUp geometry the way SketchUp's DWG importer does: layers
//! become tags, blocks components, 3D faces / polyface meshes / hatches faces, lines, arcs,
//! circles, polylines and splines edges, points construction points; colours become
//! materials; the options of "Options d'importation de fichier AutoCAD DWG/DXF" apply.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::f64::consts::TAU;

use anyhow::{Result, anyhow, bail};
use cadkit_core::geom_ops::{Transform, arbitrary_axes};
use cadkit_core::{Color, Document, Entity, EntityKind, HatchEdge, LengthUnit, Point3, Vec3, Vertex};
use esq_core::glam::{DMat4, DVec3, DVec4};
use esq_core::model::{EdgeId, Entities, Model, Units};

use crate::build::{BuildOpts, DefIn, LineIn, MeshIn, NodeIn, NodeKind, PolyIn, SceneIn};
use crate::options::DwgImport;
use crate::units::LenUnit;

/// Segments of a full circle (SketchUp's default for circles and arcs).
pub const CIRCLE_SEGMENTS: usize = 24;

fn v(p: Point3) -> DVec3 {
    DVec3::new(p.x, p.y, p.z)
}

fn vv(p: Vec3) -> DVec3 {
    DVec3::new(p.x, p.y, p.z)
}

/// Reads a .dwg or a .dxf (by content).
pub fn read(bytes: &[u8]) -> Result<Document> {
    let options = cadkit_core::ReadOptions::default();
    if cadkit_dwg::sniff(bytes) {
        cadkit_dwg::read(bytes, &options).map_err(|e| anyhow!("fichier DWG illisible : {e}"))
    } else if cadkit_dxf::sniff(bytes) {
        cadkit_dxf::read(bytes, &options).map_err(|e| anyhow!("fichier DXF illisible : {e}"))
    } else {
        bail!("ni DWG ni DXF (version non prise en charge ?)")
    }
}

/// The counts of the import report ("Entités AutoCAD importées").
#[derive(Default)]
struct Stats {
    imported: BTreeMap<&'static str, usize>,
    ignored: BTreeMap<&'static str, usize>,
}

impl Stats {
    fn add(&mut self, k: &'static str) {
        *self.imported.entry(k).or_default() += 1;
    }
    fn skip(&mut self, k: &'static str) {
        *self.ignored.entry(k).or_default() += 1;
    }
    fn text(&self, blocks: usize, layers: usize) -> String {
        let mut s = String::from("Entités AutoCAD importées :\n");
        s.push_str(&format!("\tBlocs :\t{blocks}\n\tCalques :\t{layers}\n"));
        for (k, n) in &self.imported {
            s.push_str(&format!("\t{k} :\t{n}\n"));
        }
        if !self.ignored.is_empty() {
            s.push_str("\nEntités AutoCAD ignorées :\n");
            for (k, n) in &self.ignored {
                s.push_str(&format!("\t{k} :\t{n}\n"));
            }
        }
        s
    }
}

struct Conv<'a> {
    doc: &'a Document,
    o: &'a DwgImport,
    scene: SceneIn,
    stats: Stats,
    layer_color: HashMap<String, Color>,
    color_mat: HashMap<[u8; 3], u32>,
    block_index: HashMap<String, usize>,
    building: HashSet<String>,
}

fn rgb_of(c: Color) -> Option<[u8; 3]> {
    match c {
        Color::Aci { index } => {
            let (r, g, b) = cadkit_core::aci::to_rgb(index);
            Some([r, g, b])
        }
        Color::Rgb { r, g, b } => Some([r, g, b]),
        _ => None,
    }
}

/// Points of an arc from `a0` to `a1` (counter-clockwise, radians) in a plane.
fn arc_points(c: DVec3, ax: DVec3, ay: DVec3, rx: f64, ry: f64, a0: f64, a1: f64) -> Vec<DVec3> {
    let mut sweep = a1 - a0;
    if sweep <= 0.0 {
        sweep += TAU;
    }
    let n = ((CIRCLE_SEGMENTS as f64 * sweep / TAU).round() as usize).max(1);
    (0..=n)
        .map(|i| {
            let t = a0 + sweep * i as f64 / n as f64;
            c + ax * (rx * t.cos()) + ay * (ry * t.sin())
        })
        .collect()
}

/// A polyline with bulges, as points (arcs segmented).
fn polyline_points(vs: &[Vertex], closed: bool, normal: DVec3) -> Vec<DVec3> {
    let mut out = Vec::new();
    let n = vs.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for i in 0..n {
        let a = v(vs[i].position);
        out.push(a);
        if i >= segs {
            continue;
        }
        let b = v(vs[(i + 1) % n].position);
        let bulge = vs[i].bulge;
        if bulge.abs() > 1e-9 && a.distance(b) > 1e-12 {
            // Arc of included angle 4·atan(bulge), counter-clockwise about the normal when
            // positive.
            let theta = 4.0 * bulge.atan();
            let chord = b - a;
            let nrm = normal.normalize_or(DVec3::Z);
            let mid = (a + b) * 0.5;
            let d = chord.length();
            let r = d / (2.0 * (theta / 2.0).sin()).abs();
            let h = (r * r - d * d / 4.0).max(0.0).sqrt();
            let side = nrm.cross(chord).normalize_or_zero();
            let center =
                mid + side * (h * if theta.abs() > std::f64::consts::PI { -1.0 } else { 1.0 } * theta.signum());
            let segments = ((CIRCLE_SEGMENTS as f64 * theta.abs() / TAU).round() as usize).max(1);
            let ra = a - center;
            for k in 1..segments {
                let t = theta * k as f64 / segments as f64;
                let rot = esq_core::glam::DQuat::from_axis_angle(nrm, t);
                out.push(center + rot * ra);
            }
        }
    }
    if closed && out.len() > 1 && out.first().unwrap().distance(*out.last().unwrap()) < 1e-12 {
        out.pop();
    }
    out
}

/// Points along a NURBS curve (or its fit points).
fn spline_points(degree: u32, knots: &[f64], cps: &[Point3], weights: &[f64], fit: &[Point3]) -> Vec<DVec3> {
    let p = degree as usize;
    if cps.len() < 2 || knots.len() != cps.len() + p + 1 || p == 0 {
        let src = if fit.len() >= 2 { fit } else { cps };
        return src.iter().map(|x| v(*x)).collect();
    }
    let w = |i: usize| weights.get(i).copied().filter(|x| *x > 0.0).unwrap_or(1.0);
    let (t0, t1) = (knots[p], knots[cps.len()]);
    let spans = cps.len() - p;
    let n = (spans * 8).clamp(8, 512);
    let eval = |t: f64| -> DVec3 {
        // De Boor on homogeneous points.
        let mut k = p;
        while k < cps.len() - 1 && t >= knots[k + 1] {
            k += 1;
        }
        let mut d: Vec<DVec4> = (0..=p)
            .map(|j| {
                let i = k + j - p;
                let c = v(cps[i]);
                DVec4::new(c.x * w(i), c.y * w(i), c.z * w(i), w(i))
            })
            .collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = k + j - p;
                let den = knots[i + p + 1 - r] - knots[i];
                let a = if den.abs() < 1e-15 { 0.0 } else { (t - knots[i]) / den };
                d[j] = d[j - 1] * (1.0 - a) + d[j] * a;
            }
        }
        let h = d[p];
        DVec3::new(h.x, h.y, h.z) / if h.w.abs() < 1e-15 { 1.0 } else { h.w }
    };
    (0..=n).map(|i| eval(t0 + (t1 - t0) * i as f64 / n as f64)).collect()
}

impl Conv<'_> {
    fn color_of(&self, e: &Entity, by_block: Option<[u8; 3]>) -> Option<[u8; 3]> {
        match e.color {
            Color::ByLayer => e.layer.as_ref().and_then(|l| self.layer_color.get(l)).and_then(|c| rgb_of(*c)),
            Color::ByBlock => by_block,
            c => rgb_of(c),
        }
    }

    fn material(&mut self, rgb: Option<[u8; 3]>) -> Option<u32> {
        if !self.o.materials {
            return None;
        }
        let rgb = rgb?;
        if let Some(&m) = self.color_mat.get(&rgb) {
            return Some(m);
        }
        let name = format!("Couleur_{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
        let i = self.scene.material_index(&name, rgb, 1.0);
        self.color_mat.insert(rgb, i);
        Some(i)
    }

    fn layer(&mut self, e: &Entity) -> Option<u32> {
        let l = e.layer.as_deref().unwrap_or("0");
        Some(self.scene.layer_index(l))
    }

    fn flat(&self, mut pts: Vec<DVec3>) -> Vec<DVec3> {
        if self.o.flatten {
            pts.iter_mut().for_each(|p| p.z = 0.0);
        }
        pts
    }

    fn line(&mut self, mesh: &mut MeshIn, pts: Vec<DVec3>, closed: bool, e: &Entity) {
        let pts = self.flat(pts);
        if pts.len() < 2 {
            return;
        }
        let base = mesh.pos.len() as u32;
        let n = pts.len() as u32;
        mesh.pos.extend(pts);
        let layer = self.layer(e);
        mesh.lines.push(LineIn { pts: (base..base + n).collect(), closed, layer });
    }

    fn face(&mut self, mesh: &mut MeshIn, loops: Vec<Vec<DVec3>>, mat: Option<u32>, e: &Entity, vis: Vec<bool>) {
        let mut it = loops.into_iter().filter(|l| l.len() >= 3);
        let Some(outer) = it.next() else { return };
        let mut idx = |l: Vec<DVec3>| -> Vec<u32> {
            let b = mesh.pos.len() as u32;
            let n = l.len() as u32;
            mesh.pos.extend(l);
            (b..b + n).collect()
        };
        let o = idx(outer);
        let holes: Vec<Vec<u32>> = it.map(&mut idx).collect();
        let layer = self.layer(e);
        let edge_vis = if vis.len() == o.len() { vis } else { Vec::new() };
        mesh.polys.push(PolyIn { outer: o, holes, mat, back: mat, layer, edge_vis, ..Default::default() });
    }

    fn hatch_loop(edges: &[HatchEdge], normal: DVec3) -> Vec<DVec3> {
        let (ax, ay, _) = arbitrary_axes(Vec3::new(normal.x, normal.y, normal.z));
        let (ax, ay) = (vv(ax), vv(ay));
        let mut out: Vec<DVec3> = Vec::new();
        let mut push = |pts: Vec<DVec3>| {
            for p in pts {
                if out.last().is_none_or(|q| q.distance(p) > 1e-9) {
                    out.push(p);
                }
            }
        };
        for e in edges {
            match e {
                HatchEdge::Line { start, end } => push(vec![v(*start), v(*end)]),
                HatchEdge::Arc { center, radius, start_angle, end_angle, ccw } => {
                    let mut p = arc_points(v(*center), ax, ay, *radius, *radius, *start_angle, *end_angle);
                    if !ccw {
                        // Clockwise arcs give their angles mirrored.
                        p = arc_points(v(*center), ax, -ay, *radius, *radius, *start_angle, *end_angle);
                    }
                    push(p);
                }
                HatchEdge::Ellipse { center, major_axis, ratio, start_param, end_param, ccw } => {
                    let maj = vv(*major_axis);
                    let r = maj.length();
                    let ux = maj.normalize_or(ax);
                    let uy = normal.cross(ux).normalize_or(ay) * if *ccw { 1.0 } else { -1.0 };
                    push(arc_points(v(*center), ux, uy, r, r * ratio, *start_param, *end_param));
                }
                HatchEdge::Spline { degree, knots, control_points, weights } => {
                    push(spline_points(*degree, knots, control_points, weights, &[]))
                }
                HatchEdge::Polyline { vertices, closed } => push(polyline_points(vertices, *closed, normal)),
            }
        }
        if out.len() > 2 && out[0].distance(*out.last().unwrap()) < 1e-9 {
            out.pop();
        }
        out
    }

    /// Converts entities into `mesh` and child nodes.
    fn entities(
        &mut self,
        ents: &[Entity],
        mesh: &mut MeshIn,
        nodes: &mut Vec<NodeIn>,
        by_block: Option<[u8; 3]>,
        depth: usize,
    ) {
        for e in ents {
            if !e.visible {
                continue;
            }
            let rgb = self.color_of(e, by_block);
            match &e.kind {
                EntityKind::Point { position } => {
                    mesh.guides.push(v(*position));
                    self.stats.add("Points");
                }
                EntityKind::Line { start, end } => {
                    self.line(mesh, vec![v(*start), v(*end)], false, e);
                    self.stats.add("Lignes");
                }
                EntityKind::Polyline { vertices, closed, normal } => {
                    let pts = polyline_points(vertices, *closed, vv(*normal));
                    let wide = vertices.iter().any(|x| x.start_width > 0.0 || x.end_width > 0.0);
                    self.line(mesh, pts, *closed, e);
                    self.stats.add(if wide { "Polylignes de largeur variable" } else { "Polylignes" });
                }
                EntityKind::Circle { center, radius, normal } => {
                    let (ax, ay, _) = arbitrary_axes(*normal);
                    let mut pts = arc_points(v(*center), vv(ax), vv(ay), *radius, *radius, 0.0, TAU);
                    pts.pop();
                    self.line(mesh, pts, true, e);
                    self.stats.add("Cercles");
                }
                EntityKind::Arc { center, radius, start_angle, end_angle, normal } => {
                    let (ax, ay, _) = arbitrary_axes(*normal);
                    let pts = arc_points(v(*center), vv(ax), vv(ay), *radius, *radius, *start_angle, *end_angle);
                    self.line(mesh, pts, false, e);
                    self.stats.add("Arcs");
                }
                EntityKind::Ellipse { center, major_axis, ratio, start_param, end_param, normal } => {
                    let maj = vv(*major_axis);
                    let ux = maj.normalize_or(DVec3::X);
                    let uy = vv(*normal).cross(ux).normalize_or(DVec3::Y);
                    let full = (end_param - start_param - TAU).abs() < 1e-9;
                    let mut pts =
                        arc_points(v(*center), ux, uy, maj.length(), maj.length() * ratio, *start_param, *end_param);
                    if full {
                        pts.pop();
                    }
                    self.line(mesh, pts, full, e);
                    self.stats.add("Ellipses");
                }
                EntityKind::Spline { degree, knots, control_points, weights, fit_points, closed } => {
                    let mut pts = spline_points(*degree, knots, control_points, weights, fit_points);
                    if *closed && pts.len() > 2 && pts[0].distance(*pts.last().unwrap()) < 1e-9 {
                        pts.pop();
                    }
                    self.line(mesh, pts, *closed, e);
                    self.stats.add("Splines");
                }
                EntityKind::Face { points, filled } => {
                    let pts: Vec<DVec3> = points.iter().map(|p| v(*p)).collect();
                    let mat = self.material(rgb);
                    // 3DFACE edge visibility (bit i: edge i hidden).
                    let flags = ["dwg.invisible_edges", "dxf.invisible_edges"]
                        .iter()
                        .find_map(|k| match e.props.get(*k) {
                            Some(cadkit_core::Value::Int(i)) => Some(*i),
                            _ => None,
                        })
                        .unwrap_or(0);
                    let vis = (0..pts.len()).map(|i| flags & (1 << i) == 0).collect();
                    self.face(mesh, vec![pts], mat, e, vis);
                    self.stats.add(if *filled { "Solides 2D" } else { "Faces 3D" });
                }
                EntityKind::Mesh { vertices, faces } => {
                    let mat = self.material(rgb);
                    let pts: Vec<DVec3> = vertices.iter().map(|p| v(*p)).collect();
                    for f in faces {
                        let l: Vec<DVec3> = f.iter().filter_map(|i| pts.get(*i as usize).copied()).collect();
                        self.face(mesh, vec![l], mat, e, Vec::new());
                    }
                    self.stats.add("Maillages polyface");
                }
                EntityKind::Polygon { exterior, interiors } => {
                    let mat = self.material(rgb);
                    let mut loops = vec![exterior.iter().map(|p| v(*p)).collect::<Vec<_>>()];
                    loops.extend(interiors.iter().map(|l| l.iter().map(|p| v(*p)).collect()));
                    self.face(mesh, loops, mat, e, Vec::new());
                    self.stats.add("Régions");
                }
                EntityKind::Hatch { loops, normal, .. } => {
                    let n = vv(*normal);
                    let mut ls: Vec<(bool, Vec<DVec3>)> =
                        loops.iter().map(|l| (l.external, Self::hatch_loop(&l.edges, n))).collect();
                    // The outermost loop first.
                    ls.sort_by_key(|l| !l.0);
                    let mat = self.material(rgb);
                    self.face(mesh, ls.into_iter().map(|l| l.1).collect(), mat, e, Vec::new());
                    self.stats.add("Hachures");
                }
                EntityKind::Text { position, value, .. } => {
                    let layer = self.layer(e);
                    mesh.texts.push((v(*position), value.clone(), layer));
                    self.stats.add("Textes");
                }
                EntityKind::MText { position, plain, .. } => {
                    let layer = self.layer(e);
                    mesh.texts.push((v(*position), plain.clone(), layer));
                    self.stats.add("Textes multilignes");
                }
                EntityKind::Insert {
                    block,
                    position,
                    scale,
                    rotation,
                    normal,
                    columns,
                    rows,
                    column_spacing,
                    row_spacing,
                } => {
                    let Some(bi) = self.block(block, depth) else {
                        self.stats.skip("Blocs vides");
                        continue;
                    };
                    let base =
                        self.doc.blocks.iter().find(|b| &b.name == block).map(|b| b.base_point).unwrap_or_default();
                    for c in 0..(*columns).max(1) {
                        for r in 0..(*rows).max(1) {
                            let off = Vec3::new(c as f64 * column_spacing, r as f64 * row_spacing, 0.0);
                            let t = Transform::from_insert_offset(*position, *scale, *rotation, *normal, base, off);
                            let m = DMat4::from_cols(
                                vv(t.x).extend(0.0),
                                vv(t.y).extend(0.0),
                                vv(t.z).extend(0.0),
                                vv(t.t).extend(1.0),
                            );
                            let material = rgb.and_then(|c| self.material(Some(c)));
                            nodes.push(NodeIn {
                                name: block.clone(),
                                xform: m,
                                def: Some(bi),
                                layer: self.layer(e),
                                material: if matches!(e.color, Color::ByLayer | Color::ByBlock) {
                                    None
                                } else {
                                    material
                                },
                                ..Default::default()
                            });
                        }
                    }
                    self.stats.add("Insertions");
                }
                EntityKind::Group { children, .. } => self.entities(children, mesh, nodes, rgb.or(by_block), depth),
                EntityKind::Dimension { .. } => self.stats.skip("Cotations"),
                EntityKind::Leader { vertices, .. } => {
                    let pts = vertices.iter().map(|p| v(*p)).collect();
                    self.line(mesh, pts, false, e);
                    self.stats.add("Lignes de repère");
                }
                EntityKind::Image { .. } => self.stats.skip("Images"),
                EntityKind::Viewport { .. } => self.stats.skip("Fenêtres"),
                EntityKind::Unknown { type_name } => {
                    let t = type_name.trim_start_matches("dwg.").trim_start_matches("dxf.");
                    let k: &'static str = match t {
                        "3DSOLID" => "Solides 3D",
                        "REGION" => "Régions",
                        "BODY" => "Corps",
                        "SURFACE" | "PLANESURFACE" | "EXTRUDEDSURFACE" | "LOFTEDSURFACE" | "REVOLVEDSURFACE"
                        | "SWEPTSURFACE" => "Surfaces",
                        "XLINE" => "Droites",
                        "RAY" => "Demi-droites",
                        "MLINE" => "Multilignes",
                        "MULTILEADER" => "Lignes de repère multiples",
                        "TOLERANCE" => "Tolérances",
                        "ATTDEF" => "Définitions d'attributs",
                        "ACAD_TABLE" => "Tableaux",
                        "WIPEOUT" => "Masques",
                        "LIGHT" => "Lumières",
                        "OLE2FRAME" | "OLEFRAME" => "Cadres OLE",
                        "ARC_DIMENSION" => "Cotations",
                        "PROXY" | "ACAD_PROXY_ENTITY" => "Objets proxy",
                        _ => "Autres",
                    };
                    self.stats.skip(k);
                }
            }
        }
    }

    /// The definition of a block (made once; recursion guarded).
    fn block(&mut self, name: &str, depth: usize) -> Option<usize> {
        if let Some(&i) = self.block_index.get(name) {
            return Some(i);
        }
        if depth > 32 || self.building.contains(name) {
            return None;
        }
        let b = self.doc.blocks.iter().find(|b| b.name == name)?;
        if b.entities.is_empty() {
            return None;
        }
        self.building.insert(name.to_string());
        let mut mesh = MeshIn::default();
        let mut nodes = Vec::new();
        let ents = b.entities.clone();
        self.entities(&ents, &mut mesh, &mut nodes, None, depth + 1);
        self.building.remove(name);
        let m = self.scene.add_mesh(mesh);
        let i = self.scene.defs.len();
        self.scene.defs.push(DefIn { name: name.to_string(), meshes: vec![m], children: nodes });
        self.block_index.insert(name.to_string(), i);
        Some(i)
    }
}

/// Millimetres per drawing unit, for the "Unités" option.
fn unit_mm(doc: &Document, o: &DwgImport, model: Units) -> f64 {
    // "Unités du modèle": the drawing's own unit when it has one.
    if o.units == LenUnit::Model
        && doc.units.unit != LengthUnit::Unitless
        && let Some(m) = doc.units.meters_per_unit().or_else(|| doc.units.unit.meters())
    {
        return m * 1000.0;
    }
    o.units.mm(model)
}

/// "Rompre les arêtes": edges that cross (or touch the inside of) another free edge are
/// split there.
pub fn break_edges(e: &mut Entities) {
    use esq_core::math::{TOL, segment_segment};
    let ids: Vec<EdgeId> = e.edges.iter().filter(|(_, x)| x.faces.is_empty()).map(|(k, _)| k).collect();
    let seg = |e: &Entities, k: EdgeId| e.edge_points(k);
    // Sort by the lowest x for a sweep.
    let mut order: Vec<(f64, f64, EdgeId)> = ids
        .iter()
        .map(|&k| {
            let (a, b) = seg(e, k);
            (a.x.min(b.x) - TOL, a.x.max(b.x) + TOL, k)
        })
        .collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cuts: HashMap<EdgeId, Vec<DVec3>> = HashMap::new();
    for i in 0..order.len() {
        let (_, hi, k1) = order[i];
        for &(lo2, _, k2) in &order[i + 1..] {
            if lo2 > hi {
                break;
            }
            let (a, b) = seg(e, k1);
            let (c, d) = seg(e, k2);
            let (s, t, dist) = segment_segment(a, b, c, d);
            if dist > TOL {
                continue;
            }
            let p = a + (b - a) * s;
            let inside = |x: f64, len: f64| x * len > TOL && (1.0 - x) * len > TOL;
            if inside(s, a.distance(b)) {
                cuts.entry(k1).or_default().push(p);
            }
            if inside(t, c.distance(d)) {
                cuts.entry(k2).or_default().push(c + (d - c) * t);
            }
        }
    }
    for (k, mut pts) in cuts {
        let (a, _) = seg(e, k);
        pts.sort_by(|p, q| p.distance(a).total_cmp(&q.distance(a)));
        let mut cur = k;
        for p in pts {
            if !e.edges.contains_key(cur) {
                break;
            }
            let (x, y) = e.edge_points(cur);
            let nv = e.split_edge(cur, p);
            // Continue with the piece that still goes on.
            let next = e.verts[nv].edges.iter().copied().find(|&ed| {
                let (u, w) = e.edge_points(ed);
                let far = if u.distance(p) < TOL { w } else { u };
                far.distance(y) < TOL || (far.distance(x) > TOL && far.distance(y) < x.distance(y))
            });
            match next {
                Some(n) => cur = n,
                None => break,
            }
        }
    }
}

/// File › Import of a .dwg or .dxf file.
pub fn import(bytes: &[u8], o: &DwgImport, model_units: Units) -> Result<(Model, String)> {
    let doc = read(bytes)?;
    let Some(model_space) = doc.models.iter().find(|m| m.kind == cadkit_core::ModelKind::Model).or(doc.models.first())
    else {
        bail!("Import Failed, no model space data was found.");
    };
    let mut c = Conv {
        doc: &doc,
        o,
        scene: SceneIn::default(),
        stats: Stats::default(),
        layer_color: doc.layers.iter().map(|l| (l.name.clone(), l.color)).collect(),
        color_mat: HashMap::new(),
        block_index: HashMap::new(),
        building: HashSet::new(),
    };
    let ents = model_space.entities.clone();
    if o.group_tag_elements {
        // "Importer des couches en tant que groupes": one group per layer.
        let mut by_layer: BTreeMap<String, Vec<Entity>> = BTreeMap::new();
        for e in ents {
            by_layer.entry(e.layer.clone().unwrap_or_else(|| "0".into())).or_default().push(e);
        }
        for (layer, es) in by_layer {
            let mut mesh = MeshIn::default();
            let mut nodes = Vec::new();
            c.entities(&es, &mut mesh, &mut nodes, None, 0);
            let m = c.scene.add_mesh(mesh);
            let li = c.scene.layer_index(&layer);
            c.scene.root.children.push(NodeIn {
                name: layer,
                meshes: vec![m],
                children: nodes,
                layer: Some(li),
                ..Default::default()
            });
        }
    } else {
        let mut mesh = MeshIn::default();
        let mut nodes = Vec::new();
        c.entities(&ents, &mut mesh, &mut nodes, None, 0);
        let m = c.scene.add_mesh(mesh);
        c.scene.root.children.push(NodeIn {
            meshes: vec![m],
            children: nodes,
            kind: NodeKind::Flatten,
            ..Default::default()
        });
    }
    let blocks = c.block_index.len();
    let layers = doc.layers.len();
    let report = c.stats.text(blocks, layers);
    let scene = std::mem::take(&mut c.scene);
    let mm = unit_mm(&doc, o, model_units);
    let mut model = crate::build::build(
        &scene,
        BuildOpts { scale: mm, swap_yz: false, merge_coplanar: o.merge_coplanar, orient_faces: o.orient_faces },
    );
    // Layers that are off or frozen give hidden tags.
    for l in &doc.layers {
        if (!l.visible || l.frozen)
            && let Some(t) = model.layers.iter_mut().find(|t| t.name == l.name)
        {
            t.visible = false;
        }
    }
    for l in model.layers.iter_mut() {
        if let Some(dl) = doc.layers.iter().find(|d| d.name == l.name)
            && let Some(rgb) = rgb_of(dl.color)
        {
            l.color = Some(rgb);
        }
    }
    if o.break_edges {
        break_edges(&mut model.root);
        for d in model.defs.values_mut() {
            break_edges(&mut d.entities);
        }
    }
    Ok((model, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal R12 DXF: a layer, a 3D face, a line, a circle, a block and two inserts.
    fn dxf() -> String {
        let mut s = String::new();
        let mut g = |c: i32, v: &str| s.push_str(&format!("{c}\n{v}\n"));
        g(0, "SECTION");
        g(2, "HEADER");
        g(9, "$ACADVER");
        g(1, "AC1009");
        g(9, "$INSUNITS");
        g(70, "4");
        g(0, "ENDSEC");
        g(0, "SECTION");
        g(2, "TABLES");
        g(0, "TABLE");
        g(2, "LAYER");
        g(70, "1");
        g(0, "LAYER");
        g(2, "Murs");
        g(70, "0");
        g(62, "1");
        g(6, "CONTINUOUS");
        g(0, "ENDTAB");
        g(0, "ENDSEC");
        g(0, "SECTION");
        g(2, "BLOCKS");
        g(0, "BLOCK");
        g(8, "0");
        g(2, "Poteau");
        g(70, "0");
        g(10, "0");
        g(20, "0");
        g(30, "0");
        g(0, "LINE");
        g(8, "0");
        g(10, "0");
        g(20, "0");
        g(30, "0");
        g(11, "0");
        g(21, "0");
        g(31, "1000");
        g(0, "ENDBLK");
        g(8, "0");
        g(0, "ENDSEC");
        g(0, "SECTION");
        g(2, "ENTITIES");
        g(0, "3DFACE");
        g(8, "Murs");
        for (c, val) in [
            (10, "0"),
            (20, "0"),
            (30, "0"),
            (11, "2000"),
            (21, "0"),
            (31, "0"),
            (12, "2000"),
            (22, "1000"),
            (32, "0"),
            (13, "0"),
            (23, "1000"),
            (33, "0"),
        ] {
            g(c, val);
        }
        g(0, "LINE");
        g(8, "Murs");
        for (c, val) in [(10, "0"), (20, "2000"), (30, "0"), (11, "3000"), (21, "2000"), (31, "0")] {
            g(c, val);
        }
        g(0, "CIRCLE");
        g(8, "0");
        for (c, val) in [(10, "5000"), (20, "0"), (30, "0"), (40, "500")] {
            g(c, val);
        }
        for x in ["100", "1900"] {
            g(0, "INSERT");
            g(8, "0");
            g(2, "Poteau");
            g(10, x);
            g(20, "100");
            g(30, "0");
        }
        g(0, "ENDSEC");
        g(0, "EOF");
        s
    }

    #[test]
    fn reads_layers_faces_lines_circles_and_blocks() {
        let text = dxf();
        let o = DwgImport { preserve_origin: true, ..Default::default() };
        let (m, report) = import(text.as_bytes(), &o, Units::Millimeters).unwrap();
        assert!(report.contains("Faces 3D :\t1"), "{report}");
        assert!(report.contains("Insertions :\t2"));
        // The tag of the layer, its colour, the material of the face (red: ACI 1).
        let murs = m.layers.iter().position(|l| l.name == "Murs").unwrap();
        assert_eq!(m.layers[murs].color, Some([255, 0, 0]));
        let face = m.root.faces.values().next().unwrap();
        assert_eq!(face.layer, murs);
        assert_eq!(m.materials[face.front.unwrap()].color, [255, 0, 0]);
        // INSUNITS 4 = millimetres; the face is 2 m × 1 m.
        let area = esq_core::tess::face_area(&m.root, m.root.faces.keys().next().unwrap());
        assert!((area - 2.0e6).abs() < 1e-3, "{area}");
        // Line + circle (24 segments) + face edges, and two instances of the block component.
        assert_eq!(m.root.instances.len(), 2);
        let d = m.defs.values().next().unwrap();
        assert_eq!(d.name, "Poteau");
        assert_eq!(d.entities.edges.len(), 1);
        assert_eq!(m.root.edges.values().filter(|e| e.faces.is_empty()).count(), 1 + CIRCLE_SEGMENTS);
    }

    #[test]
    fn options_flatten_group_and_break() {
        let text = dxf();
        let o = DwgImport { group_tag_elements: true, preserve_origin: true, ..Default::default() };
        let (m, _) = import(text.as_bytes(), &o, Units::Millimeters).unwrap();
        // One group per layer: "0" (circle, inserts) and "Murs" (face, line).
        let groups: Vec<&str> = m.defs.values().filter(|d| d.is_group).map(|d| d.name.as_str()).collect();
        assert!(groups.contains(&"Murs") && groups.contains(&"0"), "{groups:?}");
        // Two crossing lines are split where they cross.
        let mut e = Entities::default();
        e.add_line(DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 10.0, 0.0));
        let a = e.add_vertex_raw(DVec3::new(0.0, 10.0, 0.0));
        let b = e.add_vertex_raw(DVec3::new(10.0, 0.0, 0.0));
        e.add_edge_raw(a, b);
        assert_eq!(e.edges.len(), 2);
        break_edges(&mut e);
        assert_eq!(e.edges.len(), 4);
    }

    #[test]
    fn bulges_and_splines() {
        // A half circle by bulge 1 from (0,0) to (2,0): its middle point is (1,-1) or (1,1).
        let vs = [
            Vertex { position: Point3::xy(0.0, 0.0), bulge: 1.0, ..Default::default() },
            Vertex::at(Point3::xy(2.0, 0.0)),
        ];
        let p = polyline_points(&vs, false, DVec3::Z);
        assert_eq!(p.len(), 13);
        let mid = p[6];
        assert!((mid.x - 1.0).abs() < 1e-9 && (mid.y.abs() - 1.0).abs() < 1e-9, "{mid:?}");
        // Counter-clockwise from (0,0) to (2,0) passes below the chord.
        assert!(mid.y < 0.0);
        // A quadratic B-spline through its end points.
        let cps = [Point3::xy(0.0, 0.0), Point3::xy(1.0, 2.0), Point3::xy(2.0, 0.0)];
        let s = spline_points(2, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &cps, &[], &[]);
        assert!(s[0].distance(DVec3::ZERO) < 1e-12 && s.last().unwrap().distance(DVec3::new(2.0, 0.0, 0.0)) < 1e-12);
        let top = s.iter().map(|q| q.y).fold(0.0, f64::max);
        assert!((top - 1.0).abs() < 1e-9);
    }
}
