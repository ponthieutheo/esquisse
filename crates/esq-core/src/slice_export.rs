//! File › Export › Section Slice: the cut of the active section plane as a 2D drawing, as
//! SketchUp's "Acad Section Exporter" (`skp2acad`, options `COptionsDlgSection`, preferences
//! `ExportSectionType`, `FullScale`, `InDrawing`/`InModel`, `Width`/`Height`,
//! `ExportSectionLineType`, `SectionWidth`, `SectionAutomatic`, `ExportAcadVersion`) writes it:
//! DWG or DXF, plus an SVG of Esquisse's own. The true section is seen from the cut-away side,
//! looking along the arrows, the way one reads a section; the screen projection is the cut as
//! the view shows it.

use std::fmt::Write as _;

use glam::{DVec2, DVec3};

use crate::math::Plane;
use crate::model::{DefId, Model, SectionId, Units};
use crate::section::chain_loops;
use crate::vector2d::{CadDrawing, CadLine, CadWide, dwg_doc};

/// The cut, flattened: x to the right, y up, millimetres of the model. For the true section,
/// the origin is the plane's projection of the world origin.
#[derive(Clone, Debug, Default)]
pub struct SliceDrawing {
    pub segments: Vec<[DVec2; 2]>,
    /// Closed outlines (for the fill); nested outlines alternate solid and hole.
    pub loops: Vec<Vec<DVec2>>,
    /// The area a drawing "of width W" spans (the view for the screen projection); `None`:
    /// the cut's own extent.
    pub frame: Option<[DVec2; 2]>,
}

/// Which side the drawing is seen from: x = -u, y = v, with (u, v, n) the plane's basis, so
/// that the viewer looks along the normal and vertical planes keep Z pointing up.
fn flatten(plane: &Plane, p: glam::DVec3) -> DVec2 {
    let (u, v) = plane.basis();
    DVec2::new(-p.dot(u), p.dot(v))
}

impl SliceDrawing {
    /// The true section (orthographic) of plane `sec` of the collection `ctx` (`None` =
    /// model root).
    pub fn of_section(model: &Model, ctx: Option<DefId>, sec: SectionId) -> Option<SliceDrawing> {
        let plane = model.entities(ctx).sections.get(sec)?.plane();
        Self::mapped(model, ctx, sec, |p| Some(flatten(&plane, p)))
    }

    /// The cut of plane `sec` drawn through `map` (model point → drawing point; `None` drops
    /// the segment, as a point behind a perspective camera).
    pub fn mapped(
        model: &Model,
        ctx: Option<DefId>,
        sec: SectionId,
        map: impl Fn(DVec3) -> Option<DVec2>,
    ) -> Option<SliceDrawing> {
        model.entities(ctx).sections.get(sec)?;
        let segs = model.slice(ctx, sec);
        let segments: Vec<[DVec2; 2]> = segs.iter().filter_map(|(a, b)| Some([map(*a)?, map(*b)?])).collect();
        if segments.is_empty() {
            return None;
        }
        let loops = chain_loops(&segs).into_iter().filter_map(|l| l.iter().map(|p| map(*p)).collect()).collect();
        Some(SliceDrawing { segments, loops, frame: None })
    }

    /// Bounding box (min, max), `None` when empty.
    pub fn bounds(&self) -> Option<(DVec2, DVec2)> {
        let mut it = self.segments.iter().flatten();
        let first = *it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))))
    }

    /// What the drawing's width spans: the frame, else the cut's extent.
    pub fn extent(&self) -> DVec2 {
        match self.frame {
            Some([lo, hi]) => hi - lo,
            None => self.bounds().map_or(DVec2::ZERO, |(lo, hi)| hi - lo),
        }
    }
}

/// "Section réelle (orthographique)" or "Projection à l’écran" (`ExportSectionType` 0 / 1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SliceProjection {
    #[default]
    TrueSection,
    Screen,
}

/// The options of the section slice export (SketchUp's names and first values: true
/// section, full scale 1:1, plain lines of automatic width, AutoCAD 2000).
#[derive(Clone, Debug, PartialEq)]
pub struct SliceOptions {
    pub projection: SliceProjection,
    /// "Échelle réelle (1 : 1)": the scale `drawing_mm` in the drawing for `model_mm` in the
    /// model; else the drawing is `width_mm` wide (the height follows).
    pub full_scale: bool,
    pub drawing_mm: f64,
    pub model_mm: f64,
    pub width_mm: f64,
    /// Unit of the DWG/DXF coordinates.
    pub unit: Units,
    /// `ExportAcadVersion`: the version of the DWG / DXF, 0 (Version 12) to 8 (AutoCAD 2018),
    /// 3 (AutoCAD 2000) by default as in SketchUp.
    pub acad_version: usize,
    /// "Lignes de coupe": plain lines, polylines with width or thick lines.
    pub line_type: CadWide,
    /// "Automatique": the width the screen shows; else `line_width`.
    pub line_auto: bool,
    /// Width of the section lines on paper, millimetres.
    pub line_width: f64,
    /// SVG only: colour of the lines, and the fill of the closed outlines.
    pub color: [u8; 3],
    pub fill: bool,
    pub fill_color: [u8; 3],
}

impl Default for SliceOptions {
    fn default() -> Self {
        SliceOptions {
            projection: SliceProjection::TrueSection,
            full_scale: true,
            drawing_mm: 1.0,
            model_mm: 1.0,
            width_mm: 254.0,
            unit: Units::Millimeters,
            acad_version: 3,
            line_type: CadWide::None,
            line_auto: true,
            line_width: 0.35,
            color: [0, 0, 0],
            fill: false,
            fill_color: [200, 200, 200],
        }
    }
}

impl SliceOptions {
    /// The DWG / DXF version chosen.
    pub fn version(&self) -> crate::dwg::DwgVersion {
        crate::dwg::DwgVersion::from_code(self.acad_version).unwrap_or_default()
    }

    /// Paper millimetres per millimetre of the drawing `d`.
    pub fn k(&self, d: &SliceDrawing) -> f64 {
        if self.full_scale { self.drawing_mm / self.model_mm.max(1e-9) } else { self.width_mm / d.extent().x.max(1e-9) }
    }

    /// Width of the section lines on paper: `automatic` (what the screen shows) or the
    /// chosen one.
    pub fn line_mm(&self, automatic: f64) -> f64 {
        if self.line_auto { automatic } else { self.line_width }
    }
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn num(x: f64) -> String {
    let s = format!("{x:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.into() }
}

/// SVG, in millimetres on paper (a 1:50 drawing of a 5 m wall is 100 mm wide); `auto_mm`: the
/// automatic width of the lines.
pub fn to_svg(d: &SliceDrawing, o: &SliceOptions, auto_mm: f64) -> String {
    let s = o.k(d);
    let line = o.line_mm(auto_mm);
    let (lo, hi) = d.bounds().unwrap_or_default();
    let margin = (line * 4.0).max(2.0);
    let (w, h) = ((hi.x - lo.x) * s + 2.0 * margin, (hi.y - lo.y) * s + 2.0 * margin);
    // Paper x, y (y down) of a drawing point.
    let pt = |p: DVec2| ((p.x - lo.x) * s + margin, (hi.y - p.y) * s + margin);
    let mut out = String::new();
    let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\">",
        num(w),
        num(h),
        num(w),
        num(h)
    );
    let _ = writeln!(out, "<title>Tranche de section (1:{})</title>", num(1.0 / s.max(1e-12)));
    if o.fill && !d.loops.is_empty() {
        let mut path = String::new();
        for l in &d.loops {
            for (i, p) in l.iter().enumerate() {
                let (x, y) = pt(*p);
                let _ = write!(path, "{}{} {} ", if i == 0 { "M" } else { "L" }, num(x), num(y));
            }
            path.push_str("Z ");
        }
        let _ = writeln!(
            out,
            "<path d=\"{}\" fill=\"{}\" fill-rule=\"evenodd\" stroke=\"none\"/>",
            path.trim_end(),
            hex(o.fill_color)
        );
    }
    let mut path = String::new();
    for [a, b] in &d.segments {
        let ((x0, y0), (x1, y1)) = (pt(*a), pt(*b));
        let _ = write!(path, "M{} {} L{} {} ", num(x0), num(y0), num(x1), num(y1));
    }
    let _ = writeln!(
        out,
        "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"round\"/>",
        path.trim_end(),
        hex(o.color),
        num(line)
    );
    out.push_str("</svg>\n");
    out
}

/// AutoCAD colour index nearest to an RGB colour, among the seven basic ones.
pub fn nearest_aci(c: [u8; 3]) -> u8 {
    const BASIC: [(u8, [i32; 3]); 7] = [
        (1, [255, 0, 0]),
        (2, [255, 255, 0]),
        (3, [0, 255, 0]),
        (4, [0, 255, 255]),
        (5, [0, 0, 255]),
        (6, [255, 0, 255]),
        (7, [0, 0, 0]),
    ];
    // Black and white both map to 7, which CAD programs draw in contrast with the background.
    if c.iter().all(|&x| x > 200) {
        return 7;
    }
    BASIC
        .iter()
        .min_by_key(|(_, k)| (0..3).map(|i| (k[i] - c[i] as i32).pow(2)).sum::<i32>())
        .map(|(a, _)| *a)
        .unwrap_or(7)
}

/// The cut for AutoCAD, in `o.unit` of paper: the section lines on layer 0 (the section
/// exporter has no layer option), as plain lines, polylines with width or thick lines.
pub fn cad_slice(d: &SliceDrawing, o: &SliceOptions, auto_mm: f64) -> CadDrawing {
    let k = o.k(d) / o.unit.factor();
    let width = o.line_mm(auto_mm) / o.unit.factor();
    let lines = d
        .segments
        .iter()
        .map(|[a, b]| CadLine { a: *a * k, b: *b * k, layer: "0", wide: o.line_type, width })
        .collect();
    CadDrawing { lines, ..Default::default() }
}

/// DXF of the chosen version, by the writer of the hidden-line drawings.
pub fn to_dxf(d: &SliceDrawing, o: &SliceOptions, auto_mm: f64) -> anyhow::Result<Vec<u8>> {
    crate::vector2d::to_dxf(&cad_slice(d, o, auto_mm), o.unit, o.version())
}

/// DWG of the chosen version, by the one DWG writer of Esquisse ([`crate::dwg`]).
pub fn to_dwg(d: &SliceDrawing, o: &SliceOptions, auto_mm: f64) -> anyhow::Result<Vec<u8>> {
    crate::dwg::write(&dwg_doc(&cad_slice(d, o, auto_mm), o.unit), o.version())
}

/// Writes the drawing; the format follows the extension (`.dwg`, `.dxf` or `.svg`).
pub fn export_slice(d: &SliceDrawing, o: &SliceOptions, auto_mm: f64, path: &std::path::Path) -> anyhow::Result<()> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let bytes = match ext.as_str() {
        "dwg" => to_dwg(d, o, auto_mm)?,
        "dxf" => to_dxf(d, o, auto_mm)?,
        "svg" => to_svg(d, o, auto_mm).into_bytes(),
        _ => anyhow::bail!("format non pris en charge : .{ext}"),
    };
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> SliceDrawing {
        let p = |x, y| DVec2::new(x, y);
        let l = vec![p(0.0, 0.0), p(1000.0, 0.0), p(1000.0, 600.0), p(0.0, 600.0)];
        let segments = (0..4).map(|i| [l[i], l[(i + 1) % 4]]).collect();
        SliceDrawing { segments, loops: vec![l], frame: None }
    }

    #[test]
    fn svg_is_scaled_to_paper() {
        let o = SliceOptions { model_mm: 10.0, ..Default::default() };
        let svg = to_svg(&square(), &o, 0.35);
        // 1000 x 600 mm at 1:10 = 100 x 60 mm, plus a 2 mm margin on each side.
        assert!(svg.contains("width=\"104mm\" height=\"64mm\""), "{svg}");
        assert_eq!(svg.matches("<path").count(), 1, "no fill unless asked");
        let svg = to_svg(&square(), &SliceOptions { fill: true, ..o.clone() }, 0.35);
        assert_eq!(svg.matches("<path").count(), 2);
        // Not at full scale: 50 mm wide.
        let svg = to_svg(&square(), &SliceOptions { full_scale: false, width_mm: 50.0, ..o }, 0.35);
        assert!(svg.contains("width=\"54mm\" height=\"34mm\""), "{svg}");
    }

    #[test]
    fn dxf_uses_the_chosen_unit_scale_and_lines() {
        let dxf = |o: &SliceOptions, auto| String::from_utf8(to_dxf(&square(), o, auto).unwrap()).unwrap();
        let o = SliceOptions { unit: Units::Meters, ..Default::default() };
        let text = dxf(&o, 0.35);
        assert!(text.contains("$ACADVER\r\n  1\r\nAC1015\r\n"), "AutoCAD 2000 by default");
        assert!(text.contains("$INSUNITS\r\n 70\r\n     6\r\n"));
        assert!(text.contains("\r\n 11\r\n1.0\r\n 21\r\n0.0\r\n"), "second point of the first line is (1, 0) m");
        assert_eq!(text.matches("\r\nLINE\r\n").count(), 4);
        assert!(text.ends_with("EOF\r\n"));
        // Polylines with width: 0.5 mm when chosen, the automatic width otherwise.
        let o = SliceOptions { line_type: CadWide::Polyline, ..Default::default() };
        let text = dxf(&o, 0.7);
        assert_eq!(text.matches("\r\nLWPOLYLINE\r\n").count(), 4);
        assert!(text.contains(" 43\r\n0.7\r\n"));
        let text = dxf(&SliceOptions { line_auto: false, line_width: 0.5, ..o.clone() }, 0.7);
        assert!(text.contains(" 43\r\n0.5\r\n"));
        // Thick lines: a thickness.
        let text = dxf(&SliceOptions { line_type: CadWide::WideLine, ..o.clone() }, 0.7);
        assert_eq!(text.matches(" 39\r\n0.7\r\n").count(), 4);
        // The version chosen: release 12 has no lightweight polylines.
        let text = dxf(&SliceOptions { acad_version: 0, ..o }, 0.7);
        assert!(text.contains("$ACADVER\r\n  1\r\nAC1009\r\n"));
        assert_eq!(text.matches("\r\nPOLYLINE\r\n").count(), 4);
    }

    /// Lines (max x, count) and polylines (count, width) of a DWG read back by cadkit.
    fn dwg_read(o: &SliceOptions) -> (f64, usize, usize, f64, cadkit_core::LengthUnit) {
        let bytes = to_dwg(&square(), o, 0.7).unwrap();
        let d = cadkit_dwg::read(&bytes, &cadkit_core::ReadOptions::default()).unwrap();
        assert!(d.warnings.is_empty(), "{:#?}", d.warnings);
        let (mut max_x, mut lines, mut polys, mut width) = (f64::MIN, 0, 0, 0.0);
        for e in &d.models[0].entities {
            match &e.kind {
                cadkit_core::EntityKind::Line { start, end } => {
                    lines += 1;
                    max_x = max_x.max(start.x).max(end.x);
                }
                cadkit_core::EntityKind::Polyline { vertices, .. } => {
                    polys += 1;
                    width = vertices[0].start_width;
                    max_x = vertices.iter().fold(max_x, |m, v| m.max(v.position.x));
                }
                _ => {}
            }
        }
        (max_x, lines, polys, width, d.units.unit)
    }

    #[test]
    fn dwg_options_change_the_file() {
        use cadkit_core::LengthUnit as L;
        // 1:1 in millimetres: the 1000 mm side, four plain lines.
        let o = SliceOptions::default();
        let (x, lines, polys, _, unit) = dwg_read(&o);
        assert_eq!((lines, polys, unit), (4, 0, L::Millimeter));
        assert!((x - 1000.0).abs() < 1e-9);
        // 1 in the drawing for 20 in the model: 50 mm.
        let (x, ..) = dwg_read(&SliceOptions { model_mm: 20.0, ..o.clone() });
        assert!((x - 50.0).abs() < 1e-9);
        // Not at full scale: 300 mm wide.
        let (x, ..) = dwg_read(&SliceOptions { full_scale: false, width_mm: 300.0, ..o.clone() });
        assert!((x - 300.0).abs() < 1e-9);
        // The model's unit.
        let (x, _, _, _, unit) = dwg_read(&SliceOptions { unit: Units::Meters, ..o.clone() });
        assert_eq!(unit, L::Meter);
        assert!((x - 1.0).abs() < 1e-9);
        // Polylines with width: automatic (0.7 mm) or chosen.
        let o = SliceOptions { line_type: CadWide::Polyline, ..o };
        let (_, lines, polys, w, _) = dwg_read(&o);
        assert_eq!((lines, polys), (0, 4));
        assert!((w - 0.7).abs() < 1e-9);
        let (_, _, _, w, _) = dwg_read(&SliceOptions { line_auto: false, line_width: 0.25, ..o });
        assert!((w - 0.25).abs() < 1e-9);
    }

    /// The DWG and the DXF of the cut are of the version chosen, with the same lines in every
    /// one (release 12 DWG, which cadkit does not read, is read back by the tests of `dwg`).
    #[test]
    fn dwg_and_dxf_of_every_version() {
        let summary = |d: &cadkit_core::Document| -> String {
            let layers: Vec<_> = d.layers.iter().map(|l| (&l.name, l.color)).collect();
            let ents: Vec<_> = d.models[0].entities.iter().map(|e| format!("{:?} {:?}", e.layer, e.kind)).collect();
            format!("{layers:?} {ents:?}")
        };
        let base = SliceOptions { line_type: CadWide::Polyline, ..Default::default() };
        let want = summary(&cadkit_dwg::read(&to_dwg(&square(), &base, 0.7).unwrap(), &Default::default()).unwrap());
        assert!(want.contains("Polyline"), "{want}");
        for v in crate::dwg::DwgVersion::ALL {
            let o = SliceOptions { acad_version: v.code(), ..base.clone() };
            let dwg = to_dwg(&square(), &o, 0.7).unwrap();
            assert!(dwg.starts_with(v.magic().as_bytes()), "{v:?}");
            if v > crate::dwg::DwgVersion::R12 {
                let d = cadkit_dwg::read(&dwg, &Default::default()).unwrap();
                assert!(d.warnings.is_empty(), "{v:?}: {:#?}", d.warnings);
                assert_eq!(summary(&d), want, "DWG {v:?}");
            }
            let dxf = to_dxf(&square(), &o, 0.7).unwrap();
            assert!(String::from_utf8_lossy(&dxf).contains(&format!("$ACADVER\r\n  1\r\n{}\r\n", v.magic())));
            let d = cadkit_dxf::read(&dxf, &Default::default()).unwrap();
            assert!(d.warnings.is_empty(), "{v:?}: {:#?}", d.warnings);
            assert_eq!(summary(&d), want, "DXF {v:?}");
        }
    }

    #[test]
    fn nearest_color_index() {
        assert_eq!(nearest_aci([0, 0, 0]), 7);
        assert_eq!(nearest_aci([255, 255, 255]), 7);
        assert_eq!(nearest_aci([250, 20, 10]), 1);
        assert_eq!(nearest_aci([10, 20, 240]), 5);
    }

    #[test]
    fn wall_cut_is_seen_upright() {
        // Plane normal +Y: looking towards +Y, east is on the right and Z is up.
        let plane = Plane::from_point_normal(glam::DVec3::ZERO, glam::DVec3::Y);
        let a = flatten(&plane, glam::DVec3::new(0.0, 0.0, 0.0));
        let b = flatten(&plane, glam::DVec3::new(100.0, 0.0, 300.0));
        assert!((b.x - a.x - 100.0).abs() < 1e-9 && (b.y - a.y - 300.0).abs() < 1e-9, "{a} {b}");
    }
}
