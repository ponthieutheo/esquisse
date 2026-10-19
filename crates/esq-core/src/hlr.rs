//! Hidden-line drawings for the vector exports (File › Export › 2D Graphic as PDF, EPS, DWG
//! or DXF; SketchUp's `CHLRExport*`): what the camera sees, as 2D line segments (edges,
//! profiles, section cuts) and filled polygons (faces, section fills) in back-to-front
//! order, in the pixels of the picture (y down).
//!
//! Edges are cut where they cross the outline of a face or go through it, and every piece is
//! kept or dropped by testing its middle against the faces in front of it: the visible parts
//! are exact. Faces are ordered with a BSP tree, which splits them where they overlap in
//! depth, so that painting them in order hides what is behind.

use glam::{DMat4, DVec2, DVec3};

use crate::model::{Model, SectionPlane};
use crate::section::{chain_loops, clip_segment, face_cut, fill_triangles};
use crate::tess::face_triangles;
use crate::walk::visit;

/// How the picture is taken.
#[derive(Clone, Copy, Debug)]
pub struct HlrView {
    /// World → clip transform of the camera.
    pub view_proj: DMat4,
    pub eye: DVec3,
    /// Unit viewing direction.
    pub forward: DVec3,
    pub perspective: bool,
    /// Size of the picture, pixels.
    pub size: DVec2,
}

impl HlrView {
    fn depth(&self, p: DVec3) -> f64 {
        (p - self.eye).dot(self.forward)
    }

    /// Pixel of a point (y down); `None` behind a perspective camera.
    pub fn screen(&self, p: DVec3) -> Option<DVec2> {
        let c = self.view_proj * p.extend(1.0);
        if c.w.abs() < 1e-12 || (self.perspective && c.w <= 0.0) {
            return None;
        }
        let n = c.truncate() / c.w;
        Some(DVec2::new((n.x + 1.0) * 0.5 * self.size.x, (1.0 - n.y) * 0.5 * self.size.y))
    }

    /// Whether the side of a face with normal `n` at `p` faces the camera.
    fn faces_eye(&self, p: DVec3, n: DVec3) -> bool {
        let to_eye = if self.perspective { self.eye - p } else { -self.forward };
        n.dot(to_eye) > 0.0
    }
}

/// How faces are lit, as the 3D view does (`shader.wgsl`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shading {
    None,
    /// A light tied to the camera (the default lighting).
    Camera(DVec3),
    /// The sun (Window › Shadows › Use sun for shading): direction, light and dark (0–1).
    Sun {
        dir: DVec3,
        light: f64,
        dark: f64,
    },
}

/// What faces are filled with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fill {
    /// Wireframe: no fill.
    None,
    /// Hidden line: every face hides what is behind it with this colour.
    Color([u8; 3]),
    /// The faces' materials (front and back colours by default; only those when `materials`
    /// is off: the monochrome style), lit.
    Materials { front: [u8; 3], back: [u8; 3], shading: Shading, materials: bool },
}

#[derive(Clone, Debug)]
pub struct HlrSettings {
    pub fill: Fill,
    /// Draw the edges (View › Edges); profiles are drawn anyway when `profiles` is set.
    pub edges: bool,
    pub profiles: bool,
    /// Active section planes cut the model and draw their cut lines.
    pub section_cuts: bool,
    /// Colour of the section fill, if it is shown.
    pub section_fill: Option<[u8; 3]>,
    /// Colour of edges and profiles.
    pub edge_color: [u8; 3],
}

impl Default for HlrSettings {
    fn default() -> Self {
        HlrSettings {
            fill: Fill::Color([255, 255, 255]),
            edges: true,
            profiles: true,
            section_cuts: true,
            section_fill: None,
            edge_color: [0, 0, 0],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Edge,
    Profile,
    Section,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HlrLine {
    pub a: DVec2,
    pub b: DVec2,
    pub kind: LineKind,
    pub color: [u8; 3],
    /// Whether each end is an end of the edge (not where it goes behind a face): edge
    /// extensions only lengthen those.
    pub ends: [bool; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct HlrPolygon {
    pub pts: Vec<DVec2>,
    pub color: [u8; 3],
}

/// A text of the drawing (texts and dimensions of the model), centred on `pos`.
#[derive(Clone, Debug, PartialEq)]
pub struct HlrText {
    pub pos: DVec2,
    pub lines: Vec<String>,
    /// Height of the font, pixels.
    pub size: f64,
    pub color: [u8; 3],
}

#[derive(Clone, Debug, Default)]
pub struct HlrDrawing {
    pub size: DVec2,
    /// Back to front.
    pub polygons: Vec<HlrPolygon>,
    pub lines: Vec<HlrLine>,
    /// Filled marks over everything (arrowheads of dimensions, leader dots).
    pub marks: Vec<HlrPolygon>,
    pub texts: Vec<HlrText>,
}

// ------------------------------------------------------------------------------------------
// Gathering what is seen.

struct Seg {
    a: DVec3,
    b: DVec3,
    kind: LineKind,
}

struct Poly3 {
    pts: Vec<DVec3>,
    color: [u8; 3],
}

/// Keeps the part of a convex polygon on the kept side of the section planes.
fn clip_polygon(mut pts: Vec<DVec3>, cuts: &[SectionPlane]) -> Vec<DVec3> {
    for c in cuts {
        if pts.len() < 3 {
            break;
        }
        let d: Vec<f64> = pts.iter().map(|p| (*p - c.pos).dot(c.normal)).collect();
        if d.iter().all(|x| *x >= -1e-9) {
            continue;
        }
        let mut out = Vec::with_capacity(pts.len() + 2);
        for i in 0..pts.len() {
            let j = (i + 1) % pts.len();
            if d[i] >= 0.0 {
                out.push(pts[i]);
            }
            if (d[i] >= 0.0) != (d[j] >= 0.0) {
                let t = d[i] / (d[i] - d[j]);
                out.push(pts[i] + (pts[j] - pts[i]) * t);
            }
        }
        pts = out;
    }
    pts
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// A colour as the view shows a side of normal `n` (`shader.wgsl`).
pub fn shade(c: [u8; 3], n: DVec3, s: &Shading) -> [u8; 3] {
    let (lin, gamma) = match s {
        Shading::None => (1.0, 1.0),
        Shading::Camera(l) => {
            let diffuse = n.dot(l.normalize_or_zero()).max(0.0);
            let fill = n.dot(DVec3::new(-0.4, -0.2, 0.3).normalize()).max(0.0);
            (0.58 + 0.34 * diffuse + 0.08 * fill, 1.0)
        }
        Shading::Sun { dir, light, dark } => {
            let cosine = if dir.z > 0.0 { n.dot(*dir).max(0.0) } else { 0.0 };
            (1.0, (0.2 + dark + light * cosine).min(1.0))
        }
    };
    c.map(|v| {
        let g = v as f64 / 255.0 * gamma;
        (linear_to_srgb((srgb_to_linear(g) * lin).min(1.0)) * 255.0).round().clamp(0.0, 255.0) as u8
    })
}

fn gather(model: &Model, view: &HlrView, s: &HlrSettings) -> (Vec<Seg>, Vec<Poly3>, Vec<[DVec3; 3]>) {
    let mut segs = Vec::new();
    let mut polys = Vec::new();
    let mut occluders: Vec<[DVec3; 3]> = Vec::new();
    let layer_visible = |l: usize| model.layers.get(l).is_none_or(|x| x.visible);
    visit(model, |p| {
        let cuts: &[SectionPlane] = if s.section_cuts { &p.cuts } else { &[] };
        let normal_m = p.xform.inverse().transpose();
        let world_n = |n: DVec3| normal_m.transform_vector3(n).normalize_or_zero();
        // Faces: occluders for the lines, and fills.
        for (f, face) in p.ents.faces.iter() {
            if face.hidden || !layer_visible(face.layer) {
                continue;
            }
            let n = world_n(face.normal);
            for t in face_triangles(p.ents, f) {
                let w = t.map(|v| p.xform.transform_point3(v));
                let clipped = clip_polygon(w.to_vec(), cuts);
                if clipped.len() < 3 {
                    continue;
                }
                for k in 1..clipped.len() - 1 {
                    occluders.push([clipped[0], clipped[k], clipped[k + 1]]);
                }
                let color = match s.fill {
                    Fill::None => continue,
                    Fill::Color(c) => c,
                    Fill::Materials { front, back, shading, materials } => {
                        let seen_front = view.faces_eye(clipped[0], n);
                        let (mat, default, side_n) = if seen_front {
                            (face.front.or(p.inherited), front, n)
                        } else {
                            (face.back.or(p.inherited), back, -n)
                        };
                        let mat = mat.filter(|_| materials);
                        let c = mat.and_then(|m| model.materials.get(m)).map_or(default, |m| m.color);
                        shade(c, side_n, &shading)
                    }
                };
                polys.push(Poly3 { pts: clipped, color });
            }
        }
        // Edges and profiles.
        for (e, edge) in p.ents.edges.iter() {
            if edge.hidden || edge.soft || edge.smooth || !layer_visible(edge.layer) {
                continue;
            }
            let (a, b) = p.ents.edge_points(e);
            let (a, b) = (p.xform.transform_point3(a), p.xform.transform_point3(b));
            let around: Vec<DVec3> = edge
                .faces
                .iter()
                .filter_map(|&f| p.ents.faces.get(f))
                .filter(|f| !f.hidden && layer_visible(f.layer))
                .map(|f| world_n(f.normal))
                .collect();
            let mid = (a + b) * 0.5;
            let profile = match around.as_slice() {
                [] | [_] => true,
                [n1, n2] => view.faces_eye(mid, *n1) != view.faces_eye(mid, *n2),
                _ => false,
            };
            let kind = if s.profiles && profile { LineKind::Profile } else { LineKind::Edge };
            if kind == LineKind::Edge && !s.edges {
                continue;
            }
            if let Some((a, b)) = clip_segment(a, b, cuts) {
                segs.push(Seg { a, b, kind });
            }
        }
        // Section cuts.
        for (k, cut) in cuts.iter().enumerate() {
            let plane = cut.plane();
            let others: Vec<SectionPlane> =
                cuts.iter().enumerate().filter(|(j, _)| *j != k).map(|(_, c)| c.clone()).collect();
            let mut cut_segs = Vec::new();
            for (f, face) in p.ents.faces.iter() {
                if layer_visible(face.layer) && !face.hidden {
                    cut_segs.extend(face_cut(p.ents, f, &p.xform, &plane));
                }
            }
            for &(a, b) in &cut_segs {
                if let Some((a, b)) = clip_segment(a, b, &others) {
                    segs.push(Seg { a, b, kind: LineKind::Section });
                }
            }
            if let Some(c) = s.section_fill {
                for t in fill_triangles(&plane, &chain_loops(&cut_segs)) {
                    let clipped = clip_polygon(t.to_vec(), &others);
                    if clipped.len() >= 3 {
                        polys.push(Poly3 { pts: clipped, color: c });
                    }
                }
            }
        }
    });
    (segs, polys, occluders)
}

// ------------------------------------------------------------------------------------------
// Visibility of the lines.

struct STri {
    s: [DVec2; 3],
    d: [f64; 3],
    w: [DVec3; 3],
    lo: DVec2,
    hi: DVec2,
}

struct Grid {
    n: usize,
    lo: DVec2,
    cell: DVec2,
    cells: Vec<Vec<u32>>,
}

impl Grid {
    fn new(lo: DVec2, hi: DVec2, n: usize) -> Grid {
        let cell = ((hi - lo) / n as f64).max(DVec2::splat(1e-9));
        Grid { n, lo, cell, cells: vec![Vec::new(); n * n] }
    }

    fn range(&self, lo: DVec2, hi: DVec2) -> Option<(usize, usize, usize, usize)> {
        let a = ((lo - self.lo) / self.cell).floor();
        let b = ((hi - self.lo) / self.cell).floor();
        let max = (self.n - 1) as f64;
        if b.x < 0.0 || b.y < 0.0 || a.x > max || a.y > max {
            return None;
        }
        let c = |v: f64| v.clamp(0.0, max) as usize;
        Some((c(a.x), c(b.x), c(a.y), c(b.y)))
    }

    fn insert(&mut self, i: u32, lo: DVec2, hi: DVec2) {
        if let Some((x0, x1, y0, y1)) = self.range(lo, hi) {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    self.cells[y * self.n + x].push(i);
                }
            }
        }
    }

    fn query(&self, lo: DVec2, hi: DVec2, out: &mut Vec<u32>) {
        out.clear();
        if let Some((x0, x1, y0, y1)) = self.range(lo, hi) {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    out.extend_from_slice(&self.cells[y * self.n + x]);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

/// The part of a world segment in front of the camera's near plane.
fn near_clip(view: &HlrView, a: DVec3, b: DVec3, near: f64) -> Option<(DVec3, DVec3, f64, f64)> {
    if !view.perspective {
        return Some((a, b, 0.0, 1.0));
    }
    let (da, db) = (view.depth(a) - near, view.depth(b) - near);
    if da < 0.0 && db < 0.0 {
        return None;
    }
    let (mut t0, mut t1) = (0.0, 1.0);
    if da < 0.0 {
        t0 = da / (da - db);
    }
    if db < 0.0 {
        t1 = da / (da - db);
    }
    Some((a + (b - a) * t0, a + (b - a) * t1, t0, t1))
}

fn cross2(a: DVec2, b: DVec2) -> f64 {
    a.x * b.y - a.y * b.x
}

/// Parameter on `p0 → p1` where it crosses `q0 → q1`, if it does.
fn seg_seg(p0: DVec2, p1: DVec2, q0: DVec2, q1: DVec2) -> Option<f64> {
    let r = p1 - p0;
    let s = q1 - q0;
    let den = cross2(r, s);
    if den.abs() < 1e-14 {
        return None;
    }
    let t = cross2(q0 - p0, s) / den;
    let u = cross2(q0 - p0, r) / den;
    ((0.0..=1.0).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u)).then_some(t)
}

impl STri {
    /// Barycentric weights of the corners at a pixel, if it is inside (with a small tolerance).
    fn bary(&self, p: DVec2) -> Option<[f64; 3]> {
        let [a, b, c] = self.s;
        let area = cross2(b - a, c - a);
        if area.abs() < 1e-12 {
            return None;
        }
        let wa = cross2(c - b, p - b) / area;
        let wb = cross2(a - c, p - c) / area;
        let wc = 1.0 - wa - wb;
        let tol = -1e-7;
        (wa >= tol && wb >= tol && wc >= tol).then_some([wa, wb, wc])
    }

    /// Depth of the triangle at a pixel (perspective-correct).
    fn depth_at(&self, w: [f64; 3], perspective: bool) -> f64 {
        if perspective {
            let inv: f64 = (0..3).map(|i| w[i] / self.d[i].max(1e-12)).sum();
            1.0 / inv.max(1e-300)
        } else {
            (0..3).map(|i| w[i] * self.d[i]).sum()
        }
    }
}

/// The parameter `t` along a 3D segment whose projection is at `s` along the 2D one.
fn t_of_s(s: f64, da: f64, db: f64, perspective: bool) -> f64 {
    if !perspective {
        return s;
    }
    let den = db + s * (da - db);
    if den.abs() < 1e-300 { s } else { s * da / den }
}

fn s_of_t(t: f64, da: f64, db: f64, perspective: bool) -> f64 {
    if !perspective {
        return t;
    }
    let den = da + t * (db - da);
    if den.abs() < 1e-300 { t } else { t * db / den }
}

/// Clips a 2D segment to the picture: parameter range kept.
fn clip_rect(a: DVec2, b: DVec2, size: DVec2) -> Option<(f64, f64)> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let d = b - a;
    for (p, q) in [(-d.x, a.x), (d.x, size.x - a.x), (-d.y, a.y), (d.y, size.y - a.y)] {
        if p.abs() < 1e-14 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    (t1 > t0).then_some((t0, t1))
}

fn visible_lines(view: &HlrView, segs: &[Seg], tris: &[STri], grid: &Grid, color: [u8; 3]) -> Vec<HlrLine> {
    let mut out = Vec::new();
    let mut cand = Vec::new();
    let near = 1e-3;
    for sg in segs {
        let Some((a, b, _, _)) = near_clip(view, sg.a, sg.b, near) else { continue };
        let (Some(sa), Some(sb)) = (view.screen(a), view.screen(b)) else { continue };
        let (da, db) = (view.depth(a), view.depth(b));
        let Some((r0, r1)) = clip_rect(sa, sb, view.size) else { continue };
        let lo = sa.min(sb);
        let hi = sa.max(sb);
        grid.query(lo, hi, &mut cand);
        let mut cuts = vec![r0, r1];
        for &i in &cand {
            let t = &tris[i as usize];
            if t.hi.x < lo.x || t.lo.x > hi.x || t.hi.y < lo.y || t.lo.y > hi.y {
                continue;
            }
            for k in 0..3 {
                if let Some(s) = seg_seg(sa, sb, t.s[k], t.s[(k + 1) % 3]) {
                    cuts.push(s);
                }
            }
            // Where the segment goes through the triangle.
            let n = (t.w[1] - t.w[0]).cross(t.w[2] - t.w[0]);
            let (ea, eb) = ((a - t.w[0]).dot(n), (b - t.w[0]).dot(n));
            if ea * eb < 0.0 {
                let tt = ea / (ea - eb);
                cuts.push(s_of_t(tt, da, db, view.perspective));
            }
        }
        cuts.retain(|s| *s >= r0 - 1e-12 && *s <= r1 + 1e-12);
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
        let mut run: Option<(f64, f64)> = None;
        let len_px = sa.distance(sb);
        let mut flush = |run: Option<(f64, f64)>| {
            if let Some((s0, s1)) = run
                && (s1 - s0) * len_px > 1e-6
            {
                out.push(HlrLine {
                    a: sa + (sb - sa) * s0,
                    b: sa + (sb - sa) * s1,
                    kind: sg.kind,
                    color,
                    ends: [s0 <= 1e-9 && view.depth(sg.a) >= near, s1 >= 1.0 - 1e-9 && view.depth(sg.b) >= near],
                });
            }
        };
        for w in cuts.windows(2) {
            let (s0, s1) = (w[0], w[1]);
            if s1 - s0 < 1e-12 {
                continue;
            }
            let sm = (s0 + s1) * 0.5;
            let pm = sa + (sb - sa) * sm;
            let tm = t_of_s(sm, da, db, view.perspective);
            let dm = da + (db - da) * tm;
            let eps = 1e-5 * dm.abs() + 0.05;
            let hidden = cand.iter().any(|&i| {
                let t = &tris[i as usize];
                if pm.x < t.lo.x || pm.x > t.hi.x || pm.y < t.lo.y || pm.y > t.hi.y {
                    return false;
                }
                t.bary(pm).is_some_and(|l| t.depth_at(l, view.perspective) < dm - eps)
            });
            if hidden {
                flush(run.take());
            } else {
                run = Some(match run {
                    Some((r0, _)) => (r0, s1),
                    None => (s0, s1),
                });
            }
        }
        flush(run);
    }
    out
}

// ------------------------------------------------------------------------------------------
// Painter's order of the faces.

const BSP_EPS: f64 = 1e-4;
/// Past this many fragments the BSP gives up for a plain depth sort.
const BSP_MAX: usize = 400_000;

struct Node {
    n: DVec3,
    d: f64,
    on: Vec<usize>,
    front: Option<usize>,
    back: Option<usize>,
}

fn split(p: &Poly3, n: DVec3, d: f64) -> (Option<Poly3>, Option<Poly3>) {
    let dist: Vec<f64> = p.pts.iter().map(|q| q.dot(n) - d).collect();
    let (mut f, mut b) = (Vec::new(), Vec::new());
    for i in 0..p.pts.len() {
        let j = (i + 1) % p.pts.len();
        let (di, dj) = (dist[i], dist[j]);
        if di >= -BSP_EPS {
            f.push(p.pts[i]);
        }
        if di <= BSP_EPS {
            b.push(p.pts[i]);
        }
        if (di > BSP_EPS && dj < -BSP_EPS) || (di < -BSP_EPS && dj > BSP_EPS) {
            let x = p.pts[i] + (p.pts[j] - p.pts[i]) * (di / (di - dj));
            f.push(x);
            b.push(x);
        }
    }
    let mk = |pts: Vec<DVec3>| (pts.len() >= 3).then_some(Poly3 { pts, color: p.color });
    (mk(f), mk(b))
}

fn plane_of(p: &Poly3) -> Option<(DVec3, f64)> {
    let n = crate::math::newell_normal(&p.pts);
    (n.length_squared() > 1e-24).then(|| {
        let n = n.normalize();
        (n, n.dot(p.pts[0]))
    })
}

/// Polygons in back-to-front order seen from `view`.
fn painter(view: &HlrView, polys: Vec<Poly3>) -> Vec<Poly3> {
    let mut store: Vec<Poly3> = Vec::with_capacity(polys.len());
    let mut nodes: Vec<Node> = Vec::new();
    let mut work: Vec<(usize, Vec<usize>)> = Vec::new();
    let ids: Vec<usize> = polys
        .into_iter()
        .map(|p| {
            store.push(p);
            store.len() - 1
        })
        .collect::<Vec<_>>();
    let new_node = |nodes: &mut Vec<Node>, store: &[Poly3], list: &[usize]| -> Option<usize> {
        let (n, d) = list.iter().find_map(|&i| plane_of(&store[i]))?;
        nodes.push(Node { n, d, on: Vec::new(), front: None, back: None });
        Some(nodes.len() - 1)
    };
    let root = new_node(&mut nodes, &store, &ids);
    let Some(root) = root else { return Vec::new() };
    work.push((root, ids));
    let mut gave_up = false;
    while let Some((ni, list)) = work.pop() {
        let (n, d) = (nodes[ni].n, nodes[ni].d);
        let (mut fl, mut bl) = (Vec::new(), Vec::new());
        for i in list {
            let dist: Vec<f64> = store[i].pts.iter().map(|q| q.dot(n) - d).collect();
            let (mn, mx) = dist.iter().fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
            if mn >= -BSP_EPS && mx <= BSP_EPS {
                nodes[ni].on.push(i);
            } else if mn >= -BSP_EPS {
                fl.push(i);
            } else if mx <= BSP_EPS {
                bl.push(i);
            } else {
                let (f, b) = split(&store[i], n, d);
                if let Some(f) = f {
                    store.push(f);
                    fl.push(store.len() - 1);
                }
                if let Some(b) = b {
                    store.push(b);
                    bl.push(store.len() - 1);
                }
            }
        }
        if store.len() > BSP_MAX {
            gave_up = true;
            break;
        }
        for (list, front) in [(fl, true), (bl, false)] {
            if list.is_empty() {
                continue;
            }
            match new_node(&mut nodes, &store, &list) {
                Some(c) => {
                    if front {
                        nodes[ni].front = Some(c);
                    } else {
                        nodes[ni].back = Some(c);
                    }
                    work.push((c, list));
                }
                None => nodes[ni].on.extend(list),
            }
        }
    }
    if gave_up {
        // Far first, by the depth of each polygon's farthest point.
        let mut all: Vec<Poly3> = store.into_iter().collect();
        all.sort_by(|a, b| {
            let fa = a.pts.iter().map(|p| view.depth(*p)).fold(f64::MIN, f64::max);
            let fb = b.pts.iter().map(|p| view.depth(*p)).fold(f64::MIN, f64::max);
            fb.total_cmp(&fa)
        });
        return all;
    }
    // Back to front: at each node, the side away from the eye first.
    let mut order = Vec::new();
    enum Step {
        Visit(usize),
        Emit(usize),
    }
    let mut stack = vec![Step::Visit(root)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Emit(ni) => order.extend(nodes[ni].on.iter().copied()),
            Step::Visit(ni) => {
                let nd = &nodes[ni];
                let eye_front =
                    if view.perspective { view.eye.dot(nd.n) - nd.d > 0.0 } else { view.forward.dot(nd.n) < 0.0 };
                let (first, last) = if eye_front { (nd.back, nd.front) } else { (nd.front, nd.back) };
                // Pushed in reverse: `first`, then this node, then `last`.
                if let Some(l) = last {
                    stack.push(Step::Visit(l));
                }
                stack.push(Step::Emit(ni));
                if let Some(f) = first {
                    stack.push(Step::Visit(f));
                }
            }
        }
    }
    let mut store: Vec<Option<Poly3>> = store.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| store[i].take()).collect()
}

/// Splits a polygon by the near plane of a perspective camera.
fn near_poly(view: &HlrView, pts: &[DVec3], near: f64) -> Vec<DVec3> {
    if !view.perspective {
        return pts.to_vec();
    }
    let d: Vec<f64> = pts.iter().map(|p| view.depth(*p) - near).collect();
    if d.iter().all(|x| *x >= 0.0) {
        return pts.to_vec();
    }
    let mut out = Vec::new();
    for i in 0..pts.len() {
        let j = (i + 1) % pts.len();
        if d[i] >= 0.0 {
            out.push(pts[i]);
        }
        if (d[i] >= 0.0) != (d[j] >= 0.0) {
            out.push(pts[i] + (pts[j] - pts[i]) * (d[i] / (d[i] - d[j])));
        }
    }
    out
}

/// The hidden-line drawing of `model` seen from `view`.
pub fn hidden_lines(model: &Model, view: &HlrView, s: &HlrSettings) -> HlrDrawing {
    let near = 1e-3;
    let (segs, polys, occluders) = gather(model, view, s);
    // Occluders in screen space.
    let mut tris = Vec::new();
    for t in &occluders {
        let pts = near_poly(view, t, near);
        if pts.len() < 3 {
            continue;
        }
        for k in 1..pts.len() - 1 {
            let w = [pts[0], pts[k], pts[k + 1]];
            let (Some(a), Some(b), Some(c)) = (view.screen(w[0]), view.screen(w[1]), view.screen(w[2])) else {
                continue;
            };
            let sc = [a, b, c];
            if cross2(b - a, c - a).abs() < 1e-12 {
                continue;
            }
            let lo = a.min(b).min(c);
            let hi = a.max(b).max(c);
            tris.push(STri { s: sc, d: w.map(|p| view.depth(p)), w, lo, hi });
        }
    }
    let margin = view.size * 0.05;
    let mut grid = Grid::new(-margin, view.size + margin, 96);
    for (i, t) in tris.iter().enumerate() {
        grid.insert(i as u32, t.lo, t.hi);
    }
    let lines = visible_lines(view, &segs, &tris, &grid, s.edge_color);
    let polygons = painter(view, polys)
        .into_iter()
        .filter_map(|p| {
            let pts = near_poly(view, &p.pts, near);
            let pts: Option<Vec<DVec2>> = pts.iter().map(|q| view.screen(*q)).collect();
            let pts = pts?;
            if pts.len() < 3 {
                return None;
            }
            let lo = pts.iter().fold(DVec2::splat(f64::MAX), |a, b| a.min(*b));
            let hi = pts.iter().fold(DVec2::splat(f64::MIN), |a, b| a.max(*b));
            let outside = hi.x < 0.0 || hi.y < 0.0 || lo.x > view.size.x || lo.y > view.size.y;
            (!outside).then_some(HlrPolygon { pts, color: p.color })
        })
        .collect();
    HlrDrawing { size: view.size, polygons, lines, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;
    use glam::dcamera::rh::{proj::directx, view};

    /// A camera looking down -Y from (0, -5000, h), parallel, `size` px for `extent` mm.
    fn front_view(extent: f64, size: f64) -> HlrView {
        let eye = DVec3::new(0.0, -5000.0, 0.0);
        let view = view::look_at_mat4(eye, DVec3::ZERO, DVec3::Z);
        let h = extent / 2.0;
        let proj = directx::orthographic(-h, h, -h, h, 0.1, 20000.0);
        HlrView { view_proj: proj * view, eye, forward: DVec3::Y, perspective: false, size: DVec2::splat(size) }
    }

    fn persp_view(eye: DVec3, target: DVec3, size: f64) -> HlrView {
        let view = view::look_at_mat4(eye, target, DVec3::Z);
        let proj = directx::perspective(40f64.to_radians(), 1.0, 10.0, 100000.0);
        HlrView {
            view_proj: proj * view,
            eye,
            forward: (target - eye).normalize(),
            perspective: true,
            size: DVec2::splat(size),
        }
    }

    /// A box from `lo` to `hi`.
    fn add_box(m: &mut Model, lo: DVec3, hi: DVec3) {
        let e = &mut m.root;
        let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
        let v: Vec<_> = [
            p(lo.x, lo.y, lo.z),
            p(hi.x, lo.y, lo.z),
            p(hi.x, hi.y, lo.z),
            p(lo.x, hi.y, lo.z),
            p(lo.x, lo.y, hi.z),
            p(hi.x, lo.y, hi.z),
            p(hi.x, hi.y, hi.z),
            p(lo.x, hi.y, hi.z),
        ]
        .into_iter()
        .map(|q| e.add_vertex_raw(q))
        .collect();
        for l in [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]] {
            e.add_face_raw(l.iter().map(|&i| v[i]).collect(), Vec::new()).unwrap();
        }
    }

    fn total_len(d: &HlrDrawing, kind: Option<LineKind>) -> f64 {
        d.lines.iter().filter(|l| kind.is_none_or(|k| l.kind == k)).map(|l| l.a.distance(l.b)).sum()
    }

    #[test]
    fn a_box_seen_from_the_front_shows_its_outline_only() {
        let mut m = Model::new();
        add_box(&mut m, DVec3::new(-500.0, 0.0, -500.0), DVec3::new(500.0, 1000.0, 500.0));
        let view = front_view(2000.0, 200.0);
        let d = hidden_lines(&m, &view, &HlrSettings::default());
        // The front face is a 100 × 100 px square: 400 px of outline, the back edges hidden.
        let len = total_len(&d, None);
        assert!((len - 400.0).abs() < 1.0, "{len} {:?}", d.lines);
        // Seen square on, every outline edge is a profile (its faces turn away).
        assert!((total_len(&d, Some(LineKind::Profile)) - 400.0).abs() < 1.0);
        // One white face covers the middle, drawn last.
        assert!(!d.polygons.is_empty());
    }

    #[test]
    fn an_edge_behind_a_wall_is_cut_where_the_wall_hides_it() {
        let mut m = Model::new();
        // A wall from x -200 to 200, z -200 to 200, at y = 0, facing -Y.
        let e = &mut m.root;
        let w: Vec<_> = [(-200.0, -200.0), (200.0, -200.0), (200.0, 200.0), (-200.0, 200.0)]
            .iter()
            .map(|&(x, z)| e.add_vertex_raw(DVec3::new(x, 0.0, z)))
            .collect();
        e.add_face_raw(w, Vec::new()).unwrap();
        // A horizontal line behind it, longer than the wall.
        e.add_line(DVec3::new(-600.0, 300.0, 0.0), DVec3::new(600.0, 300.0, 0.0));
        let view = front_view(2000.0, 200.0);
        let s = HlrSettings { profiles: false, ..Default::default() };
        let d = hidden_lines(&m, &view, &s);
        // The line is 120 px long; 40 px of it are behind the wall.
        let mid: Vec<&HlrLine> =
            d.lines.iter().filter(|l| (l.a.y - 100.0).abs() < 1e-6 && (l.b.y - 100.0).abs() < 1e-6).collect();
        let len: f64 = mid.iter().map(|l| l.a.distance(l.b)).sum();
        assert!((len - 80.0).abs() < 1e-6, "{mid:?}");
        assert_eq!(mid.len(), 2);
        // The ends at the wall are not ends of the edge.
        assert!(mid.iter().all(|l| l.ends[0] != l.ends[1]), "{mid:?}");
    }

    #[test]
    fn a_line_through_a_face_disappears_behind_it() {
        let mut m = Model::new();
        let e = &mut m.root;
        let w: Vec<_> = [(-200.0, -200.0), (200.0, -200.0), (200.0, 200.0), (-200.0, 200.0)]
            .iter()
            .map(|&(x, z)| e.add_vertex_raw(DVec3::new(x, 0.0, z)))
            .collect();
        e.add_face_raw(w, Vec::new()).unwrap();
        // A line piercing the wall at its centre, at an angle.
        e.add_line(DVec3::new(-100.0, -400.0, 0.0), DVec3::new(100.0, 400.0, 0.0));
        let view = front_view(2000.0, 200.0);
        let d = hidden_lines(&m, &view, &HlrSettings { profiles: false, ..Default::default() });
        let piercing: Vec<&HlrLine> =
            d.lines.iter().filter(|l| (l.a.y - 100.0).abs() < 1e-6 && (l.b.y - 100.0).abs() < 1e-6).collect();
        // Only the front half (x from -10 to 0 px around the centre) is seen.
        let len: f64 = piercing.iter().map(|l| l.a.distance(l.b)).sum();
        assert!((len - 10.0).abs() < 1e-6, "{piercing:?}");
    }

    #[test]
    fn faces_are_painted_far_to_near() {
        let mut m = Model::new();
        let red = m.add_material("rouge", [255, 0, 0]);
        let blue = m.add_material("bleu", [0, 0, 255]);
        let e = &mut m.root;
        let quad = |e: &mut crate::model::Entities, y: f64, x0: f64| {
            let v: Vec<_> = [(x0, -100.0), (x0 + 400.0, -100.0), (x0 + 400.0, 100.0), (x0, 100.0)]
                .iter()
                .map(|&(x, z)| e.add_vertex_raw(DVec3::new(x, y, z)))
                .collect();
            e.add_face_raw(v, Vec::new()).unwrap()
        };
        let near = quad(e, 0.0, -300.0);
        let far = quad(e, 500.0, -100.0);
        e.faces[near].front = Some(red);
        e.faces[near].back = Some(red);
        e.faces[far].front = Some(blue);
        e.faces[far].back = Some(blue);
        let view = persp_view(DVec3::new(0.0, -3000.0, 0.0), DVec3::ZERO, 400.0);
        let s = HlrSettings {
            fill: Fill::Materials { front: [255; 3], back: [200; 3], shading: Shading::None, materials: true },
            ..Default::default()
        };
        let d = hidden_lines(&m, &view, &s);
        let first_red = d.polygons.iter().position(|p| p.color == [255, 0, 0]).unwrap();
        let last_blue = d.polygons.iter().rposition(|p| p.color == [0, 0, 255]).unwrap();
        assert!(
            last_blue < first_red,
            "the far face first: {:?}",
            d.polygons.iter().map(|p| p.color).collect::<Vec<_>>()
        );
    }

    #[test]
    fn section_cuts_hide_what_they_remove_and_draw_their_lines() {
        let mut m = Model::new();
        add_box(&mut m, DVec3::new(-500.0, 0.0, -500.0), DVec3::new(500.0, 1000.0, 500.0));
        // Cut at z = 0, keeping what is below.
        let s = m.root.sections.insert(SectionPlane::new(DVec3::ZERO, -DVec3::Z));
        m.root.active_section = Some(s);
        let view = front_view(2000.0, 200.0);
        let d = hidden_lines(&m, &view, &HlrSettings::default());
        assert!(d.lines.iter().all(|l| l.a.y >= 100.0 - 1e-6 && l.b.y >= 100.0 - 1e-6), "nothing above the cut");
        assert!(d.lines.iter().any(|l| l.kind == LineKind::Section), "the cut is drawn");
        let off = hidden_lines(&m, &view, &HlrSettings { section_cuts: false, ..Default::default() });
        assert!(off.lines.iter().any(|l| l.a.y < 60.0), "without cuts the whole box shows");
    }

    #[test]
    fn shading_matches_the_view() {
        assert_eq!(shade([200, 100, 50], DVec3::Z, &Shading::None), [200, 100, 50]);
        let lit = shade([200, 200, 200], DVec3::Z, &Shading::Camera(DVec3::Z));
        let dark = shade([200, 200, 200], -DVec3::Z, &Shading::Camera(DVec3::Z));
        assert!(lit[0] > dark[0]);
        // Sun: 0.2 + dark + light × cos, capped at 1.
        let s = Shading::Sun { dir: DVec3::Z, light: 0.8, dark: 0.2 };
        assert_eq!(shade([100, 100, 100], DVec3::Z, &s), [100, 100, 100]);
        assert_eq!(shade([100, 100, 100], DVec3::X, &s), [40, 40, 40]);
    }
}
