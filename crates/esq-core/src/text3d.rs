//! 3D text (Tools › 3D Text, `Entities#add_3d_text`): glyph outlines laid out into closed
//! polygons, then turned into edges, curves, faces with holes and an optional extrusion.
//!
//! The rules come from what SketchUp does with its own text:
//! - the font is taken at an em of 2048 units and the text is scaled so that the font's
//!   *ascent* equals the requested height (the height is not the cap height);
//! - lines are `ascent + |descent| + internal leading + line gap` apart, glyphs advance by
//!   their cell increment with no kerning, the whole text is then moved so its lowest point
//!   sits at y = 0, and centred / right-aligned lines move by the difference between the
//!   widest line's extent and their own;
//! - a tolerance of 0 means `height / 200`; a height under 0.001" does nothing;
//! - contours nested at an odd depth are holes. Outer contours run counter-clockwise, holes
//!   clockwise; a vertex whose two edges turn by more than 30° is a corner, and the runs of
//!   smooth vertices between corners become curves (one curve when there are fewer than two
//!   corners);
//! - the extrusion pushes the filled faces along +Z.

use glam::{DVec2, DVec3};

use crate::math::{TOL, point_in_polygon_2d, signed_area_2d};
use crate::model::{EdgeId, Entities, FaceId, VertexId};

/// Em square SketchUp asks Windows for (`lfHeight = -2048`); glyph coordinates are in it.
pub const FONT_EM: f64 = 2048.0;
/// Edges meeting at more than this angle (degrees) make a corner.
pub const CORNER_DEG: f64 = 30.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// One piece of a glyph outline, from the previous point.
#[derive(Clone, Copy, Debug)]
pub enum Seg {
    Line(DVec2),
    /// Quadratic Bézier: control point, end point.
    Quad(DVec2, DVec2),
    /// Cubic Bézier: two control points, end point.
    Cubic(DVec2, DVec2, DVec2),
}

/// A closed contour of a glyph.
#[derive(Clone, Debug, Default)]
pub struct Outline {
    pub start: DVec2,
    pub segs: Vec<Seg>,
}

#[derive(Clone, Debug, Default)]
pub struct Glyph {
    /// Cell increment, in the same units as the outline.
    pub advance: f64,
    pub outlines: Vec<Outline>,
}

/// Font metrics in the same units as the glyphs (an em of [`FONT_EM`]).
#[derive(Clone, Copy, Debug)]
pub struct FontMetrics {
    pub ascent: f64,
    /// Positive distance below the baseline.
    pub descent: f64,
    pub internal_leading: f64,
    pub line_gap: f64,
}

pub trait GlyphSource {
    fn metrics(&self) -> FontMetrics;
    fn glyph(&self, c: char) -> Glyph;
}

#[derive(Clone, Debug)]
pub struct TextSpec {
    pub text: String,
    pub align: Align,
    /// Model units (mm). The font's ascent measures this much.
    pub height: f64,
    /// Curve flattening tolerance; 0 = `height / 200`.
    pub tolerance: f64,
    pub z: f64,
    pub filled: bool,
    pub extrusion: f64,
}

impl Default for TextSpec {
    fn default() -> Self {
        // SketchUp's defaults: 10" high, filled and extruded by 1".
        TextSpec {
            text: String::new(),
            align: Align::Left,
            height: 254.0,
            tolerance: 0.0,
            z: 0.0,
            filled: true,
            extrusion: 25.4,
        }
    }
}

fn flatten_quad(p0: DVec2, p1: DVec2, p2: DVec2, tol: f64, out: &mut Vec<DVec2>) {
    let dd = (p0 - 2.0 * p1 + p2).length();
    let n = ((dd / (4.0 * tol)).sqrt().ceil() as usize).clamp(1, 256);
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let u = 1.0 - t;
        out.push(p0 * (u * u) + p1 * (2.0 * u * t) + p2 * (t * t));
    }
}

fn flatten_cubic(p0: DVec2, p1: DVec2, p2: DVec2, p3: DVec2, tol: f64, out: &mut Vec<DVec2>) {
    let m = (p0 - 2.0 * p1 + p2).length().max((p1 - 2.0 * p2 + p3).length());
    let n = ((6.0 * m / (8.0 * tol)).sqrt().ceil() as usize).clamp(1, 256);
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let u = 1.0 - t;
        out.push(p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t));
    }
}

/// The polygon of an outline, closing point removed.
fn flatten(o: &Outline, tol: f64, offset: DVec2) -> Vec<DVec2> {
    let mut pts = vec![o.start];
    let mut cur = o.start;
    for s in &o.segs {
        match *s {
            Seg::Line(p) => {
                pts.push(p);
                cur = p;
            }
            Seg::Quad(c, p) => {
                flatten_quad(cur, c, p, tol, &mut pts);
                cur = p;
            }
            Seg::Cubic(c1, c2, p) => {
                flatten_cubic(cur, c1, c2, p, tol, &mut pts);
                cur = p;
            }
        }
    }
    let mut out: Vec<DVec2> = Vec::with_capacity(pts.len());
    for p in pts {
        let p = p + offset;
        if out.last().is_none_or(|q| q.distance(p) > 1e-9) {
            out.push(p);
        }
    }
    while out.len() > 1 && out[0].distance(out[out.len() - 1]) <= 1e-9 {
        out.pop();
    }
    out
}

fn extent(rings: &[Vec<DVec2>]) -> Option<(DVec2, DVec2)> {
    let mut it = rings.iter().flatten();
    let first = *it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))))
}

/// Lays the text out: closed polygons in the XY plane, in model units, lowest point on y = 0.
pub fn layout(src: &dyn GlyphSource, spec: &TextSpec) -> Vec<Vec<DVec2>> {
    let m = src.metrics();
    if spec.height.abs() < TOL || m.ascent <= 0.0 {
        return Vec::new();
    }
    let scale = spec.height / m.ascent;
    let tol = if spec.tolerance == 0.0 { spec.height.abs() / 200.0 } else { spec.tolerance };
    let tol_fu = (tol / scale.abs()).max(1e-6);
    let line_h = m.ascent + m.descent + m.internal_leading + m.line_gap;

    let mut lines: Vec<Vec<Vec<DVec2>>> = vec![Vec::new()];
    let (mut pen_x, mut pen_y) = (0.0, 0.0);
    for c in spec.text.chars() {
        match c {
            '\n' => {
                pen_y -= line_h;
                pen_x = 0.0;
                lines.push(Vec::new());
            }
            '\r' => {}
            _ => {
                let g = src.glyph(c);
                let line = lines.last_mut().expect("one line at least");
                for o in &g.outlines {
                    let ring = flatten(o, tol_fu, DVec2::new(pen_x, pen_y));
                    if ring.len() >= 3 {
                        line.push(ring);
                    }
                }
                pen_x += g.advance;
            }
        }
    }

    let all: Vec<Vec<DVec2>> = lines.iter().flatten().cloned().collect();
    let Some((lo, hi)) = extent(&all) else { return Vec::new() };
    let total_w = hi.x - lo.x;
    let factor = match spec.align {
        Align::Left => 0.0,
        Align::Center => 0.5,
        Align::Right => 1.0,
    };
    let mut out = Vec::with_capacity(all.len());
    for line in &lines {
        let dx = match extent(line) {
            Some((l, h)) => (total_w - (h.x - l.x)) * factor,
            None => continue,
        };
        for ring in line {
            out.push(ring.iter().map(|p| DVec2::new((p.x + dx) * scale, (p.y - lo.y) * scale)).collect());
        }
    }
    out
}

/// What was added by [`add_geometry`].
#[derive(Debug, Default)]
pub struct TextGeometry {
    pub faces: Vec<FaceId>,
    pub edges: Vec<EdgeId>,
}

/// Is the vertex a corner: the turn between the incoming and the outgoing edge exceeds
/// [`CORNER_DEG`]. Edges shorter than 0.001" count as straight.
fn is_corner(prev: DVec2, cur: DVec2, next: DVec2) -> bool {
    let a = cur - prev;
    let b = next - cur;
    let (la, lb) = (a.length(), b.length());
    if la < TOL || lb < TOL {
        return false;
    }
    let c = (a.dot(b) / (la * lb)).clamp(-1.0, 1.0);
    c.acos().to_degrees() > CORNER_DEG
}

/// Builds the geometry for the polygons of [`layout`] inside `ents`, at height `z`.
pub fn add_geometry(ents: &mut Entities, rings: &[Vec<DVec2>], z: f64, filled: bool, extrusion: f64) -> TextGeometry {
    let mut rings: Vec<Vec<DVec2>> =
        rings.iter().filter(|r| r.len() >= 3 && signed_area_2d(r).abs() > 1e-9).cloned().collect();
    let n = rings.len();
    let area: Vec<f64> = rings.iter().map(|r| signed_area_2d(r).abs()).collect();

    // Nesting: how many other contours enclose each one.
    let mut depth = vec![0usize; n];
    let mut parent: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        let mut containers: Vec<usize> =
            (0..n).filter(|&j| j != i && area[j] > area[i] && point_in_polygon_2d(rings[i][0], &rings[j])).collect();
        depth[i] = containers.len();
        containers.sort_by(|&a, &b| area[a].total_cmp(&area[b]));
        parent[i] = containers.first().copied();
    }
    let is_hole: Vec<bool> = depth.iter().map(|d| d % 2 == 1).collect();

    // Outer contours counter-clockwise, holes clockwise.
    for i in 0..n {
        let ccw = signed_area_2d(&rings[i]) > 0.0;
        if ccw == is_hole[i] {
            rings[i].reverse();
        }
    }

    let mut geo = TextGeometry::default();
    let mut ids: Vec<Vec<VertexId>> = Vec::with_capacity(n);
    for ring in &rings {
        let vs: Vec<VertexId> = ring.iter().map(|p| ents.add_vertex_raw(DVec3::new(p.x, p.y, z))).collect();
        let m = ring.len();
        let corner: Vec<bool> = (0..m).map(|k| is_corner(ring[(k + m - 1) % m], ring[k], ring[(k + 1) % m])).collect();
        let n_corners = corner.iter().filter(|c| **c).count();
        let edge_of = |a: usize, b: usize, ents: &mut Entities| ents.add_edge_raw(vs[a], vs[b]);
        let push = |e: Option<EdgeId>, geo: &mut TextGeometry| {
            if let Some(e) = e {
                geo.edges.push(e);
            }
        };
        if n_corners == m {
            for k in 0..m {
                let e = edge_of(k, (k + 1) % m, ents);
                push(e, &mut geo);
            }
        } else if n_corners < 2 {
            let id = ents.new_curve_id();
            for k in 0..m {
                let e = edge_of(k, (k + 1) % m, ents);
                if let Some(e) = e {
                    ents.edges[e].curve = Some(id);
                }
                push(e, &mut geo);
            }
        } else {
            // Runs of edges between one corner and the next.
            let start = corner.iter().position(|c| *c).expect("a corner");
            let mut k = 0;
            while k < m {
                let mut run = vec![(start + k) % m];
                let mut j = k + 1;
                loop {
                    let v = (start + j) % m;
                    run.push(v);
                    j += 1;
                    if corner[v] || j > m {
                        break;
                    }
                }
                let id = (run.len() > 2).then(|| ents.new_curve_id());
                for w in run.windows(2) {
                    let e = edge_of(w[0], w[1], ents);
                    if let (Some(e), Some(id)) = (e, id) {
                        ents.edges[e].curve = Some(id);
                    }
                    push(e, &mut geo);
                }
                k = j - 1;
            }
        }
        ids.push(vs);
    }

    if filled {
        for i in 0..n {
            if is_hole[i] {
                continue;
            }
            let holes: Vec<Vec<VertexId>> =
                (0..n).filter(|&j| is_hole[j] && parent[j] == Some(i)).map(|j| ids[j].clone()).collect();
            if let Some(f) = ents.add_face_raw(ids[i].clone(), holes) {
                geo.faces.push(f);
            }
        }
        if extrusion.abs() >= TOL {
            let faces = std::mem::take(&mut geo.faces);
            for f in faces {
                if let Some(cap) = ents.push_pull(f, extrusion, false) {
                    geo.faces.push(cap);
                }
            }
        }
    }
    geo
}

/// Lays out `spec` with `src` and builds it into `ents`.
pub fn build(src: &dyn GlyphSource, spec: &TextSpec, ents: &mut Entities) -> TextGeometry {
    let rings = layout(src, spec);
    add_geometry(ents, &rings, spec.z, spec.filled, spec.extrusion)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "I" is a bar, "O" a square ring, " " an empty cell; ascent 1000.
    struct Blocks;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64, ccw: bool) -> Outline {
        let mut p = [DVec2::new(x0, y0), DVec2::new(x1, y0), DVec2::new(x1, y1), DVec2::new(x0, y1)];
        if !ccw {
            p.reverse();
        }
        Outline { start: p[0], segs: p[1..].iter().map(|q| Seg::Line(*q)).collect() }
    }

    impl GlyphSource for Blocks {
        fn metrics(&self) -> FontMetrics {
            FontMetrics { ascent: 1000.0, descent: 200.0, internal_leading: 0.0, line_gap: 0.0 }
        }
        fn glyph(&self, c: char) -> Glyph {
            match c {
                'I' => Glyph { advance: 400.0, outlines: vec![rect(100.0, 0.0, 300.0, 700.0, true)] },
                'O' => Glyph {
                    advance: 800.0,
                    outlines: vec![rect(50.0, 0.0, 750.0, 700.0, true), rect(250.0, 200.0, 550.0, 500.0, true)],
                },
                _ => Glyph { advance: 500.0, outlines: Vec::new() },
            }
        }
    }

    fn spec(text: &str) -> TextSpec {
        TextSpec { text: text.into(), height: 1000.0, ..TextSpec::default() }
    }

    #[test]
    fn height_is_the_ascent_not_the_cap_height() {
        // ascent 1000 -> height 100: scale 0.1, so the 700-unit bar is 70 mm high.
        let rings = layout(&Blocks, &TextSpec { height: 100.0, ..spec("I") });
        let (lo, hi) = extent(&rings).unwrap();
        assert!((hi.y - lo.y - 70.0).abs() < 1e-9);
        assert!((lo.y).abs() < 1e-9, "lowest point on y = 0");
    }

    #[test]
    fn lines_are_ascent_plus_descent_apart_and_alignment_moves_short_lines() {
        let s = TextSpec { align: Align::Right, ..spec("OO\nI") };
        let rings = layout(&Blocks, &s);
        // Second line comes first in y order: it is below.
        let low = rings.iter().filter(|r| r.iter().all(|p| p.y < 800.0)).count();
        assert!(low >= 1);
        let bar = rings.iter().min_by(|a, b| a[0].y.total_cmp(&b[0].y)).unwrap();
        // "I" is 200 wide against 1500+ for "OO": right-aligned it sits far to the right.
        assert!(bar[0].x > 1000.0, "x = {}", bar[0].x);
        // Line step: 1000 + 200 = 1200 font units at scale 1 (outer "O" against the bar).
        let min_y = |r: &Vec<DVec2>| r.iter().map(|p| p.y).fold(f64::MAX, f64::min);
        let outer = rings.iter().max_by(|a, b| signed_area_2d(a).abs().total_cmp(&signed_area_2d(b).abs())).unwrap();
        assert!((min_y(outer) - min_y(bar) - 1200.0).abs() < 1e-6, "{}", min_y(outer) - min_y(bar));
    }

    #[test]
    fn tiny_height_does_nothing() {
        assert!(layout(&Blocks, &TextSpec { height: 0.01, ..spec("I") }).is_empty());
    }

    #[test]
    fn a_ring_becomes_one_face_with_a_hole() {
        let mut ents = Entities::default();
        let rings = layout(&Blocks, &TextSpec { extrusion: 0.0, ..spec("O") });
        let geo = add_geometry(&mut ents, &rings, 0.0, true, 0.0);
        assert_eq!(geo.faces.len(), 1);
        let f = &ents.faces[geo.faces[0]];
        assert_eq!(f.holes.len(), 1);
        assert!(f.normal.z > 0.99, "faces up");
    }

    #[test]
    fn extruded_text_is_a_closed_solid() {
        let mut ents = Entities::default();
        let spec = TextSpec { extrusion: 50.0, ..spec("I") };
        build(&Blocks, &spec, &mut ents);
        // A bar: top, bottom and four sides.
        assert_eq!(ents.faces.len(), 6);
        assert!(ents.edges.values().all(|e| e.faces.len() == 2), "every edge is shared by two faces");
        let top = ents.verts.values().map(|v| v.pos.z).fold(f64::MIN, f64::max);
        assert!((top - 50.0).abs() < 1e-9);
    }

    #[test]
    fn unfilled_text_is_outline_only() {
        let mut ents = Entities::default();
        let g = build(&Blocks, &TextSpec { filled: false, ..spec("O") }, &mut ents);
        assert!(ents.faces.is_empty());
        assert_eq!(g.edges.len(), 8, "two squares");
    }

    #[test]
    fn curves_group_smooth_runs_and_split_at_corners() {
        // A circle-ish glyph: 32 gentle vertices -> no corner -> one curve.
        let pts: Vec<DVec2> = (0..32)
            .map(|i| {
                let a = i as f64 / 32.0 * std::f64::consts::TAU;
                DVec2::new(a.cos(), a.sin()) * 100.0
            })
            .collect();
        let mut ents = Entities::default();
        add_geometry(&mut ents, &[pts], 0.0, false, 0.0);
        let ids: std::collections::HashSet<_> = ents.edges.values().map(|e| e.curve).collect();
        assert_eq!(ids.len(), 1);
        assert!(ids.iter().all(|c| c.is_some()));

        // A square has 4 corners: its edges are plain.
        let mut ents = Entities::default();
        let sq = vec![DVec2::new(0.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(10.0, 10.0), DVec2::new(0.0, 10.0)];
        add_geometry(&mut ents, &[sq], 0.0, false, 0.0);
        assert!(ents.edges.values().all(|e| e.curve.is_none()));
    }

    #[test]
    fn quads_are_flattened_within_tolerance() {
        let o = Outline {
            start: DVec2::new(0.0, 0.0),
            segs: vec![Seg::Quad(DVec2::new(50.0, 100.0), DVec2::new(100.0, 0.0)), Seg::Line(DVec2::new(0.0, 0.0))],
        };
        let ring = flatten(&o, 0.5, DVec2::ZERO);
        // The curve peaks at y = 50; every flattened point lies on it, chords stay close.
        let peak = ring.iter().map(|p| p.y).fold(f64::MIN, f64::max);
        assert!(peak > 49.0 && peak <= 50.0 + 1e-9, "{peak}");
        assert!(ring.len() > 4);
    }
}
