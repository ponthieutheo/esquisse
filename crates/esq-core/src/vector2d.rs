//! Writing a hidden-line drawing (`hlr.rs`) as a vector file, the way SketchUp's 2D exports
//! do: PDF and EPS (`CHlr2Pdflib`, `CHlr2Eps`: filled faces, then lines; profiles and
//! section cuts thicker; edges optionally lengthened past their ends) and the lines alone for
//! AutoCAD (DXF).
//!
//! Sizes: the drawing is `size` pixels; `Layout::k` is how many paper millimetres one pixel
//! makes. Line widths and extension lengths are given in paper millimetres.

use std::fmt::Write as _;

use glam::DVec2;

use crate::dwg::{DwgDoc, DwgEntity, DwgGeom, DwgLayer, DwgVersion};
use crate::hlr::{HlrDrawing, HlrLine, HlrPolygon, HlrText, LineKind};
use crate::model::Units;

/// Paper (or CAD) size of a drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// Millimetres per pixel of the drawing.
    pub k: f64,
}

impl Layout {
    pub fn page(&self, d: &HlrDrawing) -> DVec2 {
        d.size * self.k
    }
}

/// Widths and lengths on paper, millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strokes {
    pub edge: f64,
    /// `None`: profiles are drawn like the other edges.
    pub profile: Option<f64>,
    /// `None`: section cuts are not exported.
    pub section: Option<f64>,
    /// `None`: no extension.
    pub extension: Option<f64>,
}

impl Default for Strokes {
    fn default() -> Self {
        Strokes { edge: 0.25, profile: Some(0.7), section: Some(1.0), extension: None }
    }
}

impl Strokes {
    fn width(&self, l: &HlrLine) -> Option<f64> {
        match l.kind {
            LineKind::Edge => Some(self.edge),
            LineKind::Profile => Some(self.profile.unwrap_or(self.edge)),
            LineKind::Section => self.section,
        }
    }
}

/// The line on paper (mm, y down), lengthened at its real ends by `ext` mm.
fn paper_line(l: &HlrLine, lay: &Layout, ext: Option<f64>) -> (DVec2, DVec2) {
    let (mut a, mut b) = (l.a * lay.k, l.b * lay.k);
    if let Some(e) = ext.filter(|e| *e > 0.0)
        && l.kind != LineKind::Section
    {
        let d = (b - a).normalize_or_zero();
        if l.ends[0] {
            a -= d * e;
        }
        if l.ends[1] {
            b += d * e;
        }
    }
    (a, b)
}

const PT: f64 = 72.0 / 25.4;

fn num(x: f64) -> String {
    let s = format!("{x:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.into() }
}

fn rgb(c: [u8; 3]) -> String {
    c.map(|v| num(v as f64 / 255.0)).join(" ")
}

// ------------------------------------------------------------------------------------------
// Texts: Helvetica, the PDF and PostScript base font.

/// Helvetica's advance widths, thousandths of the font size, of the characters 32 to 126
/// (Adobe's published metrics).
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // space – /
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, // 0 – ?
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, // @ – O
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, // P – _
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, // ` – o
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584, // p – ~
];

/// The plain letter an accented one is as wide as.
fn base_letter(c: char) -> char {
    match c {
        'à'..='å' => 'a',
        'ç' => 'c',
        'è'..='ë' => 'e',
        'ì'..='ï' => 'i',
        'ñ' => 'n',
        'ò'..='ö' => 'o',
        'ù'..='ü' => 'u',
        'ý' | 'ÿ' => 'y',
        'À'..='Å' => 'A',
        'Ç' => 'C',
        'È'..='Ë' => 'E',
        'Ì'..='Ï' => 'I',
        'Ñ' => 'N',
        'Ò'..='Ö' => 'O',
        'Ù'..='Ü' => 'U',
        'Ý' => 'Y',
        '\u{a0}' => ' ',
        _ => c,
    }
}

/// Width of a line of text, in font sizes.
pub fn text_width(s: &str) -> f64 {
    s.chars()
        .map(|c| match base_letter(c) {
            c @ ' '..='~' => HELVETICA[c as usize - 32],
            '°' => 400,
            '²' | '³' | '“' | '”' => 333,
            '’' | '‘' => 222,
            '—' | '…' | 'Œ' | 'Æ' => 1000,
            'œ' => 944,
            'æ' => 889,
            '×' | '±' => 584,
            _ => 556,
        } as f64)
        .sum::<f64>()
        / 1000.0
}

/// The byte of a printable character in Windows code page 1252 (PDF's `WinAnsiEncoding`).
pub(crate) fn win_ansi(c: char) -> Option<u8> {
    Some(match c {
        ' '..='~' | '\u{a0}'..='ÿ' => c as u8,
        '€' => 0x80,
        '‚' => 0x82,
        'ƒ' => 0x83,
        '„' => 0x84,
        '…' => 0x85,
        '†' => 0x86,
        '‡' => 0x87,
        'ˆ' => 0x88,
        '‰' => 0x89,
        'Š' => 0x8A,
        '‹' => 0x8B,
        'Œ' => 0x8C,
        'Ž' => 0x8E,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '˜' => 0x98,
        '™' => 0x99,
        'š' => 0x9A,
        '›' => 0x9B,
        'œ' => 0x9C,
        'ž' => 0x9E,
        'Ÿ' => 0x9F,
        _ => return None,
    })
}

/// The byte of a character in `WinAnsiEncoding` (PDF) or, `iso` set, `ISOLatin1Encoding`
/// (PostScript); `?` when it has none.
fn single_byte(c: char, iso: bool) -> u8 {
    match c as u32 {
        0x20..=0x7E | 0xA0..=0xFF => c as u8,
        _ if iso => b'?',
        _ => win_ansi(c).unwrap_or(b'?'),
    }
}

/// A string literal of PDF or PostScript, in one-byte encoding.
fn ps_string(s: &str, iso: bool) -> String {
    let mut out = String::from("(");
    for c in s.chars() {
        match single_byte(c, iso) {
            b @ (b'(' | b')' | b'\\') => {
                out.push('\\');
                out.push(b as char);
            }
            b if b >= 0x80 => {
                let _ = write!(out, "\\{b:03o}");
            }
            b => out.push(b as char),
        }
    }
    out.push(')');
    out
}

/// Where each line of a text starts on paper (mm, y down) and its font size (mm): centred
/// on the text's position.
fn text_lines<'a>(t: &'a HlrText, lay: &Layout) -> impl Iterator<Item = (DVec2, f64, &'a str)> + 'a {
    let size = t.size * lay.k;
    let lh = size * 1.2;
    let top = t.pos.y * lay.k - lh * t.lines.len() as f64 / 2.0;
    let x = t.pos.x * lay.k;
    t.lines.iter().enumerate().map(move |(i, s)| {
        let base = top + lh * (i as f64 + 0.5) + 0.35 * size;
        (DVec2::new(x - text_width(s) * size / 2.0, base), size, s.as_str())
    })
}

/// The page content in PDF operators (points, y up): faces filled (and outlined with their own
/// colour, so that neighbouring pieces leave no gap), then lines, then texts and dimensions.
fn pdf_content(d: &HlrDrawing, lay: &Layout, st: &Strokes) -> String {
    let h = d.size.y * lay.k * PT;
    let p = |v: DVec2| format!("{} {}", num(v.x * lay.k * PT), num(h - v.y * lay.k * PT));
    let mut out = String::from("1 J 1 j\n");
    let fill = |out: &mut String, poly: &HlrPolygon| {
        let c = rgb(poly.color);
        let _ = write!(out, "{c} rg {c} RG 0.1 w {} m", p(poly.pts[0]));
        for q in &poly.pts[1..] {
            let _ = write!(out, " {} l", p(*q));
        }
        out.push_str(" h B\n");
    };
    for poly in &d.polygons {
        fill(&mut out, poly);
    }
    for l in &d.lines {
        let Some(w) = st.width(l) else { continue };
        let (a, b) = paper_line(l, lay, st.extension);
        let pp = |v: DVec2| format!("{} {}", num(v.x * PT), num(h - v.y * PT));
        let _ = writeln!(out, "{} RG {} w {} m {} l S", rgb(l.color), num(w * PT), pp(a), pp(b));
    }
    for poly in &d.marks {
        fill(&mut out, poly);
    }
    for t in &d.texts {
        for (at, size, s) in text_lines(t, lay) {
            let _ = writeln!(
                out,
                "BT /F1 {} Tf {} rg {} {} Td {} Tj ET",
                num(size * PT),
                rgb(t.color),
                num(at.x * PT),
                num(h - at.y * PT),
                ps_string(s, false)
            );
        }
    }
    out
}

/// A one-page vector PDF.
pub fn to_pdf(d: &HlrDrawing, lay: &Layout, st: &Strokes, title: &str) -> Vec<u8> {
    let page = lay.page(d) * PT;
    let content = pdf_content(d, lay, st);
    let data = miniz_oxide::deflate::compress_to_vec_zlib(content.as_bytes(), 6);
    let title: String = title.chars().filter(|c| c.is_ascii() && !"()\\".contains(*c)).collect();
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    let mut obj = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    obj(
        &mut out,
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents 4 0 R /Resources << /Font << /F1 6 0 R >> >> >>",
            num(page.x),
            num(page.y)
        )
        .as_bytes(),
    );
    let mut stream = format!("<< /Length {} /Filter /FlateDecode >>\nstream\n", data.len()).into_bytes();
    stream.extend_from_slice(&data);
    stream.extend_from_slice(b"\nendstream");
    obj(&mut out, &stream);
    obj(&mut out, format!("<< /Title ({title}) /Producer (Esquisse) >>").as_bytes());
    obj(&mut out, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>");
    let xref = out.len();
    let _ =
        std::io::Write::write_all(&mut out, format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1)
            .as_bytes(),
    );
    out
}

/// Encapsulated PostScript, with the operators SketchUp's `CHlr2Eps` writes.
pub fn to_eps(d: &HlrDrawing, lay: &Layout, st: &Strokes, title: &str) -> String {
    let page = lay.page(d) * PT;
    let h = page.y;
    let p = |v: DVec2| format!("{} {}", num(v.x * lay.k * PT), num(h - v.y * lay.k * PT));
    let mut out = String::new();
    let _ = writeln!(out, "%!PS-Adobe-3.0 EPSF-3.0");
    let _ = writeln!(out, "%%BoundingBox: 0 0 {} {}", page.x.ceil() as i64, page.y.ceil() as i64);
    let _ = writeln!(out, "%%HiResBoundingBox: 0 0 {} {}", num(page.x), num(page.y));
    let title: String = title.chars().filter(|c| c.is_ascii() && *c != '\n').collect();
    let _ = writeln!(out, "%%Title: {title}");
    let _ = writeln!(out, "%%Creator: Esquisse");
    let _ = writeln!(out, "%%Pages: 1");
    let _ = writeln!(out, "%%EndComments");
    if !d.texts.is_empty() {
        // Helvetica with the accented letters of Latin-1.
        out.push_str(
            "/Helvetica findfont dup length dict begin {1 index /FID ne {def} {pop pop} ifelse} forall\n\
             /Encoding ISOLatin1Encoding def currentdict end /Helvetica-Latin1 exch definefont pop\n",
        );
    }
    let _ = writeln!(out, "1 setlinecap 1 setlinejoin");
    let fill = |out: &mut String, poly: &HlrPolygon| {
        let _ = writeln!(out, "{} setrgbcolor", rgb(poly.color));
        let _ = writeln!(out, "newpath {} moveto", p(poly.pts[0]));
        for q in &poly.pts[1..] {
            let _ = writeln!(out, "{} lineto", p(*q));
        }
        let _ = writeln!(out, "closepath gsave fill grestore 0.1 setlinewidth stroke");
    };
    for poly in &d.polygons {
        fill(&mut out, poly);
    }
    for l in &d.lines {
        let Some(w) = st.width(l) else { continue };
        let (a, b) = paper_line(l, lay, st.extension);
        let pp = |v: DVec2| format!("{} {}", num(v.x * PT), num(h - v.y * PT));
        let _ = writeln!(out, "{} setrgbcolor", rgb(l.color));
        let _ = writeln!(out, "{} setlinewidth", num(w * PT));
        let _ = writeln!(out, "newpath {} moveto {} lineto stroke", pp(a), pp(b));
    }
    for poly in &d.marks {
        fill(&mut out, poly);
    }
    for t in &d.texts {
        for (at, size, s) in text_lines(t, lay) {
            let _ = writeln!(
                out,
                "/Helvetica-Latin1 findfont {} scalefont setfont {} setrgbcolor {} {} moveto {} show",
                num(size * PT),
                rgb(t.color),
                num(at.x * PT),
                num(h - at.y * PT),
                ps_string(s, true)
            );
        }
    }
    out.push_str("showpage\n%%EOF\n");
    out
}

/// How profiles and section lines go to AutoCAD (SketchUp's DWG/DXF options).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CadWide {
    /// "Aucune": plain lines.
    #[default]
    None,
    /// "Polylignes avec largeur".
    Polyline,
    /// "Entités ligne épaisse": lines with a thickness.
    WideLine,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CadLines {
    pub mode: CadWide,
    /// Width in drawing units; `None` = automatic (as on screen).
    pub width: Option<f64>,
    /// "Placer sur un calque séparé".
    pub own_layer: bool,
}

impl Default for CadLines {
    fn default() -> Self {
        CadLines { mode: CadWide::None, width: None, own_layer: false }
    }
}

/// Options of a 2D drawing for AutoCAD.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CadOptions {
    /// Unit of the drawing's coordinates.
    pub unit: Units,
    pub profiles: CadLines,
    pub sections: CadLines,
    /// Extension of edges, drawing units (`None`: none).
    pub extension: Option<f64>,
}

impl Default for CadOptions {
    fn default() -> Self {
        CadOptions {
            unit: Units::Millimeters,
            profiles: CadLines::default(),
            sections: CadLines::default(),
            extension: None,
        }
    }
}

fn insunits(u: Units) -> u8 {
    match u {
        Units::Inches => 1,
        Units::Feet => 2,
        Units::Millimeters => 4,
        Units::Centimeters => 5,
        Units::Meters => 6,
        Units::Yards => 10,
    }
}

/// The lines of the drawing in CAD coordinates (y up, `unit`), with their layer and width
/// (`automatic` widths are what the screen shows, given in drawing units).
pub struct CadLine {
    pub a: DVec2,
    pub b: DVec2,
    pub layer: &'static str,
    pub wide: CadWide,
    pub width: f64,
}

/// One line of text, centred on `pos` (drawing units, y up).
pub struct CadText {
    pub pos: DVec2,
    pub height: f64,
    pub text: String,
}

/// A drawing for AutoCAD: lines, filled triangles (arrowheads), texts.
#[derive(Default)]
pub struct CadDrawing {
    pub lines: Vec<CadLine>,
    pub solids: Vec<[DVec2; 3]>,
    pub texts: Vec<CadText>,
}

impl CadDrawing {
    /// Lower and upper corners of everything drawn.
    pub fn extents(&self) -> (DVec2, DVec2) {
        let pts = self
            .lines
            .iter()
            .flat_map(|l| [l.a, l.b])
            .chain(self.solids.iter().flatten().copied())
            .chain(self.texts.iter().map(|t| t.pos));
        let (lo, hi) = pts.fold((DVec2::splat(f64::MAX), DVec2::splat(f64::MIN)), |(lo, hi), p| (lo.min(p), hi.max(p)));
        if lo.x > hi.x { (DVec2::ZERO, DVec2::ZERO) } else { (lo, hi) }
    }
}

/// The drawing in CAD coordinates (`unit`, y up).
pub fn cad_drawing(d: &HlrDrawing, lay: &Layout, o: &CadOptions, automatic: [f64; 2]) -> CadDrawing {
    let k = lay.k / o.unit.factor();
    let flip = |v: DVec2| DVec2::new(v.x * k, (d.size.y - v.y) * k);
    let mut solids = Vec::new();
    for m in &d.marks {
        for i in 1..m.pts.len().saturating_sub(1) {
            solids.push([flip(m.pts[0]), flip(m.pts[i]), flip(m.pts[i + 1])]);
        }
    }
    let mut texts = Vec::new();
    for t in &d.texts {
        let lh = t.size * 1.2;
        let top = t.pos.y - lh * t.lines.len() as f64 / 2.0;
        for (i, s) in t.lines.iter().enumerate() {
            let pos = flip(DVec2::new(t.pos.x, top + lh * (i as f64 + 0.5)));
            texts.push(CadText { pos, height: t.size * 0.7 * k, text: s.clone() });
        }
    }
    CadDrawing { lines: cad_lines(d, lay, o, automatic), solids, texts }
}

pub fn cad_lines(d: &HlrDrawing, lay: &Layout, o: &CadOptions, automatic: [f64; 2]) -> Vec<CadLine> {
    let k = lay.k / o.unit.factor();
    let h = d.size.y;
    let mut out = Vec::new();
    for l in &d.lines {
        let (lines, auto, layer) = match l.kind {
            LineKind::Edge => (None, 0.0, "0"),
            LineKind::Profile => (Some(o.profiles), automatic[0], "PROFILES"),
            LineKind::Section => (Some(o.sections), automatic[1], "SECTIONS"),
        };
        let ext = o.extension.map(|e| e * o.unit.factor() / lay.k);
        let mut l2 = *l;
        if l.kind == LineKind::Section {
            l2.ends = [false, false];
        }
        let (a, b) = paper_line(&l2, &Layout { k: 1.0 }, ext);
        let flip = |v: DVec2| DVec2::new(v.x * k, (h - v.y) * k);
        let (wide, width, layer) = match lines {
            None => (CadWide::None, 0.0, layer),
            Some(c) => (c.mode, c.width.unwrap_or(auto), if c.own_layer { layer } else { "0" }),
        };
        out.push(CadLine { a: flip(a), b: flip(b), layer, wide, width });
    }
    out
}

/// Height of Arial's capitals for a size of 1: AutoCAD makes them as high as the text.
const ARIAL_CAP: f64 = 0.716;

/// The drawing as a DWG ([`crate::dwg`]): the same entities as [`to_dxf`], wide polylines as
/// LWPOLYLINE of constant width; texts centred in Arial, the font of SketchUp's AutoCAD
/// exporter (its widths are Helvetica's, so the start of each text is where AutoCAD puts it).
pub fn dwg_doc(c: &CadDrawing, unit: Units) -> DwgDoc {
    let mut entities = Vec::new();
    for l in &c.lines {
        let geom = match l.wide {
            CadWide::Polyline if l.width > 0.0 => {
                DwgGeom::LwPolyline { pts: vec![l.a, l.b], closed: false, width: l.width, elevation: 0.0 }
            }
            CadWide::WideLine if l.width > 0.0 => {
                DwgGeom::Line { a: l.a.extend(0.0), b: l.b.extend(0.0), thickness: l.width }
            }
            _ => DwgGeom::Line { a: l.a.extend(0.0), b: l.b.extend(0.0), thickness: 0.0 },
        };
        entities.push(DwgEntity::new(geom, l.layer));
    }
    for s in &c.solids {
        entities.push(DwgEntity::new(DwgGeom::Solid { corners: [s[0], s[1], s[2], s[2]], elevation: 0.0 }, "0"));
    }
    for t in &c.texts {
        let w = text_width(&t.text) * t.height / ARIAL_CAP;
        let pos = DVec2::new(t.pos.x - w / 2.0, t.pos.y - t.height / 2.0);
        let geom = DwgGeom::Text {
            pos: pos.extend(0.0),
            align: t.pos.extend(0.0),
            height: t.height,
            rotation: 0.0,
            halign: 1,
            valign: 2,
            text: t.text.clone(),
        };
        entities.push(DwgEntity::new(geom, "0"));
    }
    DwgDoc {
        layers: vec![DwgLayer::new("0", 7)],
        entities,
        insunits: i16::from(insunits(unit)),
        font: "Arial.ttf".into(),
        ..DwgDoc::default()
    }
}

/// The drawing as AutoCAD DXF of release `v`, by the DXF writer of [`crate::dwg`]: the
/// entities of the DWG ([`dwg_doc`]).
pub fn to_dxf(c: &CadDrawing, unit: Units, v: DwgVersion) -> anyhow::Result<Vec<u8>> {
    crate::dwg::write_dxf(&dwg_doc(c, unit), v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hlr::{HlrPolygon, LineKind};

    fn drawing() -> HlrDrawing {
        HlrDrawing {
            size: DVec2::new(200.0, 100.0),
            polygons: vec![HlrPolygon {
                pts: vec![DVec2::new(10.0, 10.0), DVec2::new(50.0, 10.0), DVec2::new(50.0, 50.0)],
                color: [255, 0, 0],
            }],
            lines: vec![
                HlrLine {
                    a: DVec2::new(0.0, 100.0),
                    b: DVec2::new(100.0, 100.0),
                    kind: LineKind::Edge,
                    color: [0, 0, 0],
                    ends: [true, false],
                },
                HlrLine {
                    a: DVec2::new(0.0, 0.0),
                    b: DVec2::new(0.0, 50.0),
                    kind: LineKind::Profile,
                    color: [0, 0, 0],
                    ends: [true, true],
                },
            ],
            marks: vec![HlrPolygon {
                pts: vec![
                    DVec2::new(150.0, 50.0),
                    DVec2::new(160.0, 50.0),
                    DVec2::new(160.0, 60.0),
                    DVec2::new(150.0, 60.0),
                ],
                color: [20, 20, 20],
            }],
            texts: vec![HlrText {
                pos: DVec2::new(100.0, 20.0),
                lines: vec!["Été (1)".into()],
                size: 10.0,
                color: [20, 20, 20],
            }],
        }
    }

    #[test]
    fn texts_are_centred_in_helvetica() {
        assert!((text_width("Été") - (0.667 + 0.278 + 0.556)).abs() < 1e-9);
        assert_eq!(ps_string("Été (1) €", false), "(\\311t\\351 \\(1\\) \\200)");
        assert_eq!(ps_string("€", true), "(?)");
        let lay = Layout { k: 1.0 };
        let (at, size, _) = text_lines(&drawing().texts[0], &lay).next().unwrap();
        assert_eq!(size, 10.0);
        assert!((at.x + text_width("Été (1)") * 5.0 - 100.0).abs() < 1e-9);
        assert!((at.y - 23.5).abs() < 1e-9, "baseline {}", at.y);
    }

    #[test]
    fn pdf_page_and_lines_are_on_paper() {
        // 1 px = 1 mm: a 200 × 100 mm page.
        let lay = Layout { k: 1.0 };
        let pdf = to_pdf(&drawing(), &lay, &Strokes::default(), "Vue");
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/MediaBox [0 0 566.929 283.465]"), "{text}");
        assert!(text.trim_end().ends_with("%%EOF"));
        // The content stream inflates back to the operators.
        let key = b"/Filter /FlateDecode >>\nstream\n";
        let at = pdf.windows(key.len()).position(|w| w == key).unwrap() + key.len();
        let end = pdf[at..].windows(10).position(|w| w == b"\nendstream").unwrap() + at;
        let content = String::from_utf8(miniz_oxide::inflate::decompress_to_vec_zlib(&pdf[at..end]).unwrap()).unwrap();
        assert!(content.contains("1 0 0 rg"), "{content}");
        // The edge along the bottom of the drawing is at y = 0 on the page.
        assert!(content.contains("0 0 0 RG 0.709 w 0 0 m 283.465 0 l S"), "{content}");
        // The profile is thicker.
        assert!(content.contains("0 0 0 RG 1.984 w 0 283.465 m 0 141.732 l S"), "{content}");
        // The text, in the base font.
        assert!(content.contains("BT /F1 28.346 Tf"), "{content}");
        assert!(content.contains("Tj ET"));
        assert!(text.contains("/BaseFont /Helvetica /Encoding /WinAnsiEncoding"));
    }

    #[test]
    fn extensions_lengthen_the_real_ends_only() {
        let st = Strokes { extension: Some(5.0), ..Default::default() };
        let eps = to_eps(&drawing(), &Layout { k: 1.0 }, &st, "Vue");
        assert!(eps.starts_with("%!PS-Adobe-3.0 EPSF-3.0"));
        assert!(eps.contains("%%BoundingBox: 0 0 567 284"));
        // The bottom edge starts 5 mm before x = 0 and still ends at x = 100 mm.
        assert!(eps.contains("newpath -14.173 0 moveto 283.465 0 lineto stroke"), "{eps}");
        assert!(eps.contains("closepath gsave fill grestore"));
        assert!(eps.contains("/Helvetica-Latin1 findfont 28.346 scalefont setfont"));
        assert!(eps.contains("(\\311t\\351 \\(1\\)) show"), "{eps}");
        assert!(eps.trim_end().ends_with("%%EOF"));
    }

    #[test]
    fn dxf_puts_wide_profiles_on_their_layer() {
        let o = CadOptions {
            unit: Units::Centimeters,
            profiles: CadLines { mode: CadWide::Polyline, width: None, own_layer: true },
            ..Default::default()
        };
        let cad = cad_drawing(&drawing(), &Layout { k: 10.0 }, &o, [0.3, 0.6]);
        let lines = &cad.lines;
        // 1 px = 10 mm = 1 cm; y up.
        assert_eq!((lines[0].a, lines[0].b), (DVec2::new(0.0, 0.0), DVec2::new(100.0, 0.0)));
        assert_eq!(lines[1].layer, "PROFILES");
        // The square mark is two triangles; the text is centred where it was.
        assert_eq!(cad.solids.len(), 2);
        assert_eq!(cad.texts[0].pos, DVec2::new(100.0, 80.0));
        assert_eq!(cad.extents(), (DVec2::ZERO, DVec2::new(160.0, 100.0)));
        // Release 12: a 2D polyline, the text in Windows-1252, no units.
        let r12 = to_dxf(&cad, o.unit, DwgVersion::R12).unwrap();
        let text: &[u8] = b"\r\n  1\r\n\xC9t\xE9 (1)\r\n";
        assert!(r12.windows(text.len()).any(|w| w == text));
        let dxf = String::from_utf8_lossy(&r12);
        assert!(dxf.contains("$ACADVER\r\n  1\r\nAC1009\r\n"));
        assert_eq!(dxf.matches("\r\nSOLID\r\n").count(), 2);
        assert!(!dxf.contains("$INSUNITS"));
        assert_eq!(dxf.matches("\r\nLINE\r\n").count(), 1);
        assert_eq!(dxf.matches("\r\nPOLYLINE\r\n").count(), 1);
        assert!(dxf.contains(" 40\r\n0.3\r\n"), "automatic width");
        assert!(dxf.contains("\r\nPROFILES\r\n"));
        assert!(dxf.ends_with("EOF\r\n"));
        // AutoCAD 2000: a lightweight polyline, the units; 2007: UTF-8.
        let dxf = String::from_utf8_lossy(&to_dxf(&cad, o.unit, DwgVersion::R2000).unwrap()).into_owned();
        assert!(dxf.contains("$ACADVER\r\n  1\r\nAC1015\r\n"));
        assert!(dxf.contains("$INSUNITS\r\n 70\r\n     5\r\n"));
        assert_eq!(dxf.matches("\r\nLWPOLYLINE\r\n").count(), 1);
        assert!(dxf.contains(" 43\r\n0.3\r\n"), "automatic width");
        let dxf = String::from_utf8(to_dxf(&cad, o.unit, DwgVersion::R2007).unwrap()).unwrap();
        assert!(dxf.contains("\r\n  1\r\nÉté (1)\r\n"));
    }

    #[test]
    fn dwg_has_the_entities_of_the_dxf() {
        use cadkit_core::{Color, EntityKind, Point3};
        let o = CadOptions {
            unit: Units::Centimeters,
            profiles: CadLines { mode: CadWide::Polyline, width: None, own_layer: true },
            ..Default::default()
        };
        let cad = cad_drawing(&drawing(), &Layout { k: 10.0 }, &o, [0.3, 0.6]);
        let bytes = crate::dwg::write(&dwg_doc(&cad, o.unit), crate::dwg::DwgVersion::R2000).unwrap();
        let d = cadkit_dwg::read(&bytes, &cadkit_core::ReadOptions::default()).unwrap();
        assert!(d.warnings.is_empty(), "{:#?}", d.warnings);
        assert_eq!(d.units.unit, cadkit_core::LengthUnit::Centimeter);
        let names: Vec<&str> = d.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["0", "PROFILES"]);
        assert!(d.layers.iter().all(|l| l.color == Color::Aci { index: 7 }));
        let e = &d.models[0].entities;
        assert_eq!(e.len(), 5);
        assert!(matches!(&e[0].kind, EntityKind::Line { end, .. } if *end == Point3::new(100.0, 0.0, 0.0)));
        match &e[1].kind {
            EntityKind::Polyline { vertices, .. } => {
                assert_eq!(vertices.len(), 2);
                assert_eq!(vertices[0].start_width, 0.3);
            }
            k => panic!("{k:?}"),
        }
        assert_eq!(e[1].layer.as_deref(), Some("PROFILES"));
        assert!(matches!(e[2].kind, EntityKind::Face { filled: true, .. }));
        assert!(matches!(e[3].kind, EntityKind::Face { filled: true, .. }));
        match &e[4].kind {
            EntityKind::Text { position, value, height, .. } => {
                assert_eq!(*position, Point3::new(100.0, 80.0, 0.0));
                assert_eq!(value, "Été (1)");
                // 10 px of type, 1 px = 1 cm: capitals of 7 cm.
                assert!((height - 7.0).abs() < 1e-9, "{height}");
            }
            k => panic!("{k:?}"),
        }
        assert_eq!(d.text_styles[0].font.as_deref(), Some("Arial.ttf"));
    }
}
