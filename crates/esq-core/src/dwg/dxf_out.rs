//! AutoCAD DXF of a [`DwgDoc`] in every release of SketchUp's list, with the handles,
//! names and objects of the DWG of the same release ([`super::objects::plan`]):
//!
//! - release 12: header, tables, blocks and entities without subclass markers, handles on
//!   the entities only (`$HANDLING`), model and paper space as `$MODEL_SPACE` and
//!   `$PAPER_SPACE`, 2D polylines for lightweight ones and no infinite lines;
//! - from release 13: handles and owners (5 / 330) everywhere, AcDb subclass markers, the
//!   CLASSES section, the BLOCK_RECORD table and the OBJECTS section (named object
//!   dictionary, multiline style, from 2000 the layouts and the plot style dictionary);
//! - up to release 14, names in capitals ([`super::objects::Names`]);
//! - before 2007, text in Windows-1252 with `\U+XXXX` for other characters; from 2007, UTF-8.

use std::io::Write;

use anyhow::Result;
use glam::{DVec2, DVec3};

use super::bits::encode_ansi;
use super::objects::*;
use super::{DwgColor, DwgDoc, DwgGeom, DwgVersion};
use DwgVersion::{R12, R13, R14, R2000, R2004, R2007, R2018};

/// Group codes of 32-bit integers (others are 16-bit).
fn long_code(c: u16) -> bool {
    matches!(c, 90..=99 | 420..=429 | 440..=449 | 1071)
}

/// A real as AutoCAD writes it: "0.0", "12.5", "1.000000000000000E+20".
fn real(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return "0.0".into();
    }
    let a = x.abs();
    if !(1e-10..1e15).contains(&a) {
        let s = format!("{x:.15E}");
        let (m, e) = s.split_at(s.find('E').unwrap_or(s.len()));
        let e = e.trim_start_matches('E');
        return if e.starts_with('-') { format!("{m}E{e}") } else { format!("{m}E+{e}") };
    }
    let s = format!("{x}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

struct W {
    v: DwgVersion,
    out: Vec<u8>,
}

impl W {
    fn code(&mut self, c: u16) {
        let _ = write!(self.out, "{c:>3}\r\n");
    }

    fn raw(&mut self, c: u16, s: &str) {
        self.code(c);
        self.out.extend_from_slice(s.as_bytes());
        self.out.extend_from_slice(b"\r\n");
    }

    /// A string: in the code page before 2007, in UTF-8 from 2007, on one line.
    fn s(&mut self, c: u16, s: &str) {
        let s: String = s.chars().map(|ch| if ch.is_control() { ' ' } else { ch }).collect();
        self.code(c);
        if self.v >= R2007 {
            self.out.extend_from_slice(s.as_bytes());
        } else {
            self.out.extend(encode_ansi(&s));
        }
        self.out.extend_from_slice(b"\r\n");
    }

    fn i(&mut self, c: u16, x: i64) {
        self.code(c);
        let _ = if long_code(c) { write!(self.out, "{x:>9}\r\n") } else { write!(self.out, "{x:>6}\r\n") };
    }

    fn f(&mut self, c: u16, x: f64) {
        self.raw(c, &real(x));
    }

    fn h(&mut self, c: u16, h: u64) {
        self.raw(c, &format!("{h:X}"));
    }

    fn p(&mut self, c: u16, p: DVec3) {
        self.f(c, p.x);
        self.f(c + 10, p.y);
        self.f(c + 20, p.z);
    }

    fn p2(&mut self, c: u16, p: DVec2) {
        self.f(c, p.x);
        self.f(c + 10, p.y);
    }

    /// A subclass marker (release 13 on).
    fn sub(&mut self, marker: &str) {
        if self.v >= R13 {
            self.s(100, marker);
        }
    }

    fn var(&mut self, name: &str) {
        self.raw(9, name);
    }

    fn section(&mut self, name: &str) {
        self.raw(0, "SECTION");
        self.raw(2, name);
    }

    fn end(&mut self) {
        self.raw(0, "ENDSEC");
    }

    /// The start of a table: from release 13 its handle and subclass.
    fn table(&mut self, name: &str, h: u64, count: usize) {
        self.raw(0, "TABLE");
        self.raw(2, name);
        if self.v >= R13 {
            self.h(5, h);
            self.h(330, 0);
            self.sub("AcDbSymbolTable");
        }
        self.i(70, count as i64);
        if name == "DIMSTYLE" && self.v >= R2000 {
            self.sub("AcDbDimStyleTable");
        }
    }

    /// The start of a table entry: from release 13 its handle, owner and subclasses
    /// (release 12 table entries have no handle).
    fn record(&mut self, kind: &str, h: u64, owner: u64, sub: &str, name: &str) {
        self.raw(0, kind);
        if self.v >= R13 {
            self.h(if kind == "DIMSTYLE" { 105 } else { 5 }, h);
            self.h(330, owner);
            self.sub("AcDbSymbolTableRecord");
            self.sub(sub);
        }
        self.s(2, name);
        self.i(70, 0);
    }

    /// The start of an object of the OBJECTS section, owned by `owner`.
    fn object(&mut self, kind: &str, h: u64, owner: u64) {
        self.raw(0, kind);
        self.h(5, h);
        if owner != 0 {
            self.raw(102, "{ACAD_REACTORS");
            self.h(330, owner);
            self.raw(102, "}");
        }
        self.h(330, owner);
    }
}

/// Writes `doc` as a DXF file of `version`.
pub(super) fn write(doc: &DwgDoc, version: DwgVersion) -> Result<Vec<u8>> {
    let plan = plan(doc, version)?;
    let mut w = W { v: version, out: Vec::new() };
    header(&mut w, &plan, doc);
    if version >= R13 {
        classes(&mut w);
    }
    tables(&mut w, &plan, doc);
    blocks(&mut w, &plan);
    w.section("ENTITIES");
    for p in &plan.spaces[plan.model].ents {
        entity(&mut w, &plan, p, MODEL_SPACE, false);
    }
    w.end();
    if version >= R13 {
        objects(&mut w, &plan);
    }
    w.raw(0, "EOF");
    Ok(w.out)
}

fn header(w: &mut W, plan: &Plan, doc: &DwgDoc) {
    let v = w.v;
    w.section("HEADER");
    w.var("$ACADVER");
    w.raw(1, v.magic());
    if v >= R14 {
        w.var("$ACADMAINTVER");
        w.i(if v >= R2018 { 90 } else { 70 }, i64::from(v.maintenance()));
    }
    w.var("$DWGCODEPAGE");
    w.raw(3, if v <= R13 { "ansi_1252" } else { "ANSI_1252" });
    w.var("$INSBASE");
    w.p(10, DVec3::ZERO);
    let (lo, hi) = plan.ext();
    w.var("$EXTMIN");
    w.p(10, lo);
    w.var("$EXTMAX");
    w.p(10, hi);
    w.var("$LIMMIN");
    w.p2(10, DVec2::ZERO);
    w.var("$LIMMAX");
    w.p2(10, plan.limits);
    w.var("$TEXTSIZE");
    w.f(40, plan.text_size);
    w.var("$TEXTSTYLE");
    w.s(7, &fixed(v, "Standard"));
    w.var("$CLAYER");
    w.raw(8, "0");
    w.var("$CELTYPE");
    w.s(6, &fixed(v, "ByLayer"));
    w.var("$CECOLOR");
    w.i(62, 256);
    w.var("$DIMSTYLE");
    w.s(2, &fixed(v, "Standard"));
    w.var("$LUNITS");
    w.i(70, 2);
    w.var("$LUPREC");
    w.i(70, 4);
    if v <= R14 {
        w.var("$HANDLING");
        w.i(70, 1);
    }
    w.var("$HANDSEED");
    w.h(5, plan.handseed);
    if v >= R14 {
        w.var("$MEASUREMENT");
        w.i(70, i64::from(plan.metric));
    }
    if v >= R2000 {
        w.var("$INSUNITS");
        w.i(70, i64::from(doc.insunits));
        w.var("$EXTNAMES");
        w.i(290, 1);
    }
    w.end();
}

fn classes(w: &mut W) {
    w.section("CLASSES");
    if w.v >= R2000 {
        for (_, dxf, cpp, count) in CLASSES {
            w.raw(0, "CLASS");
            w.raw(1, dxf);
            w.raw(2, cpp);
            w.raw(3, "ObjectDBX Classes");
            w.i(90, 0); // proxy flags
            if w.v >= R2004 {
                w.i(91, i64::from(count));
            }
            w.i(280, 0); // not a zombie
            w.i(281, 0); // not an entity
        }
    }
    w.end();
}

fn tables(w: &mut W, plan: &Plan, doc: &DwgDoc) {
    let v = w.v;
    w.section("TABLES");

    // The *Active viewport, on the extents.
    w.table("VPORT", VPORT_CONTROL, 1);
    {
        let (center, height, aspect) = plan.view();
        let grid = DVec2::splat(if plan.metric { 10.0 } else { 0.5 });
        w.record("VPORT", VPORT_ACTIVE, VPORT_CONTROL, "AcDbViewportTableRecord", &fixed(v, "*Active"));
        w.p2(10, DVec2::ZERO);
        w.p2(11, DVec2::ONE);
        w.p2(12, center);
        w.p2(13, DVec2::ZERO); // snap base
        w.p2(14, grid); // snap spacing
        w.p2(15, grid); // grid spacing
        w.p(16, DVec3::Z); // view direction
        w.p(17, DVec3::ZERO); // target
        w.f(40, height);
        w.f(41, aspect);
        w.f(42, 50.0); // lens
        w.f(43, 0.0); // front clip
        w.f(44, 0.0); // back clip
        w.f(50, 0.0); // snap rotation
        w.f(51, 0.0); // twist
        for (c, x) in [(71, 0), (72, 1000), (73, 1), (74, 3), (75, 0), (76, 0), (77, 0), (78, 0)] {
            w.i(c, x);
        }
        if v >= R2000 {
            w.i(281, 0); // render mode
            w.i(65, 1); // UCS per viewport
            w.p(110, DVec3::ZERO);
            w.p(111, DVec3::X);
            w.p(112, DVec3::Y);
            w.i(79, 0); // orthographic type
            w.f(146, 0.0); // elevation
        }
    }
    w.raw(0, "ENDTAB");

    // Linetypes: release 12 has no ByBlock and ByLayer entries.
    w.table("LTYPE", LTYPE_CONTROL, 1);
    let ltypes: &[(u64, &str, &str)] = if v == R12 {
        &[(LT_CONTINUOUS, "Continuous", "Solid line")]
    } else {
        &[(LT_BYBLOCK, "ByBlock", ""), (LT_BYLAYER, "ByLayer", ""), (LT_CONTINUOUS, "Continuous", "Solid line")]
    };
    for &(h, name, desc) in ltypes {
        w.record("LTYPE", h, LTYPE_CONTROL, "AcDbLinetypeTableRecord", &fixed(v, name));
        w.s(3, desc);
        w.i(72, 65);
        w.i(73, 0);
        w.f(40, 0.0);
    }
    w.raw(0, "ENDTAB");

    w.table("LAYER", LAYER_CONTROL, plan.layers.len());
    for (l, h) in &plan.layers {
        w.record("LAYER", *h, LAYER_CONTROL, "AcDbLayerTableRecord", &l.name);
        let c = i64::from(l.color.max(1));
        w.i(62, if l.visible { c } else { -c });
        w.s(6, &fixed(v, "Continuous"));
        if v >= R2000 {
            w.i(370, -3); // default lineweight
            w.h(390, PLOTSTYLE_NORMAL);
        }
    }
    w.raw(0, "ENDTAB");

    w.table("STYLE", STYLE_CONTROL, 1);
    w.record("STYLE", STYLE_STANDARD, STYLE_CONTROL, "AcDbTextStyleTableRecord", &fixed(v, "Standard"));
    w.f(40, 0.0); // fixed height
    w.f(41, 1.0); // width factor
    w.f(50, 0.0); // oblique
    w.i(71, 0); // generation
    w.f(42, plan.text_size); // last height
    w.s(3, doc.font(v));
    w.raw(4, ""); // big font
    w.raw(0, "ENDTAB");

    w.table("VIEW", VIEW_CONTROL, 0);
    w.raw(0, "ENDTAB");
    w.table("UCS", UCS_CONTROL, 0);
    w.raw(0, "ENDTAB");

    w.table("APPID", APPID_CONTROL, 1);
    w.record("APPID", APPID_ACAD, APPID_CONTROL, "AcDbRegAppTableRecord", "ACAD");
    w.raw(0, "ENDTAB");

    w.table("DIMSTYLE", DIMSTYLE_CONTROL, 1);
    dimstyle(w, &Dims::new(plan.metric));
    w.raw(0, "ENDTAB");

    if v >= R13 {
        // Model and paper space first, as AutoCAD writes them.
        let user = plan.model;
        w.table("BLOCK_RECORD", BLOCK_CONTROL, user);
        for s in plan.spaces[user..].iter().chain(&plan.spaces[..user]) {
            w.raw(0, "BLOCK_RECORD");
            w.h(5, s.header);
            w.h(330, BLOCK_CONTROL);
            w.sub("AcDbSymbolTableRecord");
            w.sub("AcDbBlockTableRecord");
            w.s(2, &s.name);
            if v >= R2000 {
                let layout = match s.header {
                    MODEL_SPACE => LAYOUT_MODEL,
                    PAPER_SPACE => LAYOUT_PAPER,
                    _ => 0,
                };
                w.h(340, layout);
            }
            if v >= R2007 {
                w.i(70, 0); // insert units
                w.i(280, 1); // explodable
                w.i(281, 0); // any scaling
            }
        }
        w.raw(0, "ENDTAB");
    }
    w.end();
}

/// The Standard dimension style: the variables of the DWG's, under the codes of the release.
fn dimstyle(w: &mut W, d: &Dims) {
    let v = w.v;
    w.record("DIMSTYLE", DIMSTYLE_STANDARD, DIMSTYLE_CONTROL, "AcDbDimStyleTableRecord", &fixed(v, "Standard"));
    w.raw(3, ""); // DIMPOST
    w.raw(4, ""); // DIMAPOST
    if v <= R14 {
        for c in [5, 6, 7] {
            w.raw(c, ""); // DIMBLK DIMBLK1 DIMBLK2
        }
    }
    for (c, x) in
        [(40, d.scale), (41, d.asz), (42, d.exo), (43, d.dli), (44, d.exe), (45, 0.0), (46, 0.0), (47, 0.0), (48, 0.0)]
    {
        w.f(c, x);
    }
    for (c, x) in
        [(140, d.txt), (141, d.cen), (142, 0.0), (143, d.altf), (144, 1.0), (145, 0.0), (146, 1.0), (147, d.gap)]
    {
        w.f(c, x);
    }
    // DIMTOL DIMLIM DIMTIH DIMTOH DIMSE1 DIMSE2 DIMTAD DIMZIN
    for (c, x) in [(71, 0), (72, 0), (73, 1), (74, 1), (75, 0), (76, 0), (77, 0), (78, 0)] {
        w.i(c, x);
    }
    // DIMALT DIMALTD DIMTOFL DIMSAH DIMTIX DIMSOXD DIMCLRD DIMCLRE DIMCLRT
    for (c, x) in [(170, 0), (171, 2), (172, 0), (173, 0), (174, 0), (175, 0), (176, 0), (177, 0), (178, 0)] {
        w.i(c, x);
    }
    if v == R12 {
        return;
    }
    let dec = i64::from(d.dec);
    if v >= R2000 {
        w.i(179, 0); // DIMADEC
    } else {
        w.i(270, 2); // DIMUNIT
    }
    // DIMDEC DIMTDEC DIMALTU DIMALTTD DIMAUNIT
    for (c, x) in [(271, dec), (272, dec), (273, 2), (274, 2), (275, 0)] {
        w.i(c, x);
    }
    if v >= R2000 {
        // DIMFRAC DIMLUNIT DIMDSEP DIMTMOVE
        for (c, x) in [(276, 0), (277, 2), (278, i64::from(b'.')), (279, 0)] {
            w.i(c, x);
        }
    }
    // DIMJUST DIMSD1 DIMSD2 DIMTOLJ DIMTZIN DIMALTZ DIMALTTZ, DIMFIT (R13–R14), DIMUPT,
    // DIMATFIT (2000+)
    for (c, x) in [(280, 0), (281, 0), (282, 0), (283, 1), (284, 0), (285, 0), (286, 0)] {
        w.i(c, x);
    }
    if v <= R14 {
        w.i(287, 3);
    }
    w.i(288, 0);
    if v >= R2000 {
        w.i(289, 3);
    }
    w.h(340, STYLE_STANDARD); // DIMTXSTY
    if v >= R2000 {
        w.i(371, -2); // DIMLWD
        w.i(372, -2); // DIMLWE
    }
}

fn blocks(w: &mut W, plan: &Plan) {
    let v = w.v;
    w.section("BLOCKS");
    let user = plan.model;
    for s in plan.spaces[user..].iter().chain(&plan.spaces[..user]) {
        let paper = s.header == PAPER_SPACE;
        w.raw(0, "BLOCK");
        if v >= R13 {
            w.h(5, s.block);
            w.h(330, s.header);
            w.sub("AcDbEntity");
        }
        if paper {
            w.i(67, 1);
        }
        w.raw(8, "0");
        w.sub("AcDbBlockBegin");
        w.s(2, &s.name);
        w.i(70, 0);
        w.p(10, s.base);
        w.s(3, &s.name);
        w.raw(1, ""); // xref path
        if s.header != MODEL_SPACE && !paper {
            for p in &s.ents {
                entity(w, plan, p, s.header, false);
            }
        }
        w.raw(0, "ENDBLK");
        w.h(5, s.endblk);
        if v >= R13 {
            w.h(330, s.header);
            w.sub("AcDbEntity");
        }
        if paper {
            w.i(67, 1);
        }
        w.raw(8, "0");
        w.sub("AcDbBlockEnd");
    }
    w.end();
}

/// One entity, its vertices and SEQEND.
fn entity(w: &mut W, plan: &Plan, p: &Placed, owner: u64, paper: bool) {
    let v = w.v;
    let layer = plan.layers[plan.layer(&p.e.layer)].0.name.clone();
    let color = p.e.color;
    let head = |w: &mut W, kind: &str, h: u64, owner: u64| {
        w.raw(0, kind);
        w.h(5, h);
        if v >= R13 {
            w.h(330, owner);
            w.sub("AcDbEntity");
        }
        if paper {
            w.i(67, 1);
        }
        w.s(8, &layer);
        match color {
            DwgColor::ByLayer => {}
            DwgColor::ByBlock => w.i(62, 0),
            DwgColor::Aci(i) => w.i(62, i64::from(i.max(1))),
            DwgColor::Rgb { aci, .. } => {
                w.i(62, i64::from(aci.max(1)));
                if v >= R2004 {
                    w.i(420, i64::from(color.rgb().unwrap_or(0)));
                }
            }
        }
    };
    let seqend = |w: &mut W| head(w, "SEQEND", p.seqend, p.h);
    match &p.e.geom {
        DwgGeom::Line { a, b, thickness } => {
            head(w, "LINE", p.h, owner);
            w.sub("AcDbLine");
            if *thickness != 0.0 {
                w.f(39, *thickness);
            }
            w.p(10, *a);
            w.p(11, *b);
        }
        DwgGeom::LwPolyline { pts, closed, width, elevation } if v <= R13 => {
            // No lightweight polylines before release 14: a 2D POLYLINE.
            head(w, "POLYLINE", p.h, owner);
            w.sub("AcDb2dPolyline");
            w.i(66, 1);
            w.p(10, DVec3::new(0.0, 0.0, *elevation));
            w.i(70, i64::from(*closed));
            if *width != 0.0 {
                w.f(40, *width);
                w.f(41, *width);
            }
            for (q, h) in pts.iter().zip(&p.subs) {
                head(w, "VERTEX", *h, p.h);
                w.sub("AcDbVertex");
                w.sub("AcDb2dVertex");
                w.p(10, q.extend(0.0));
                if *width != 0.0 {
                    w.f(40, *width);
                    w.f(41, *width);
                }
                w.i(70, 0);
            }
            seqend(w);
        }
        DwgGeom::LwPolyline { pts, closed, width, elevation } => {
            head(w, "LWPOLYLINE", p.h, owner);
            w.sub("AcDbPolyline");
            w.i(90, pts.len() as i64);
            w.i(70, i64::from(*closed));
            if *width != 0.0 {
                w.f(43, *width);
            }
            if *elevation != 0.0 {
                w.f(38, *elevation);
            }
            for q in pts {
                w.p2(10, *q);
            }
        }
        DwgGeom::Polyline3d { pts, closed } => {
            head(w, "POLYLINE", p.h, owner);
            w.sub("AcDb3dPolyline");
            w.i(66, 1);
            w.p(10, DVec3::ZERO);
            w.i(70, 8 | i64::from(*closed));
            for (q, h) in pts.iter().zip(&p.subs) {
                head(w, "VERTEX", *h, p.h);
                w.sub("AcDbVertex");
                w.sub("AcDb3dPolylineVertex");
                w.p(10, *q);
                w.i(70, 32);
            }
            seqend(w);
        }
        DwgGeom::PolyFace { verts, faces } => {
            head(w, "POLYLINE", p.h, owner);
            w.sub("AcDbPolyFaceMesh");
            w.i(66, 1);
            w.p(10, DVec3::ZERO);
            w.i(70, 64);
            w.i(71, verts.len() as i64);
            w.i(72, faces.len() as i64);
            for (q, h) in verts.iter().zip(&p.subs) {
                head(w, "VERTEX", *h, p.h);
                w.sub("AcDbVertex");
                w.sub("AcDbPolyFaceMeshVertex");
                w.p(10, *q);
                w.i(70, 192);
            }
            for (f, h) in faces.iter().zip(&p.subs[verts.len()..]) {
                head(w, "VERTEX", *h, p.h);
                w.sub("AcDbFaceRecord");
                w.p(10, DVec3::ZERO);
                w.i(70, 128);
                for (k, &i) in f.iter().enumerate() {
                    if k < 3 || i != 0 {
                        w.i(71 + k as u16, i64::from(i.clamp(-32_767, 32_767)));
                    }
                }
            }
            seqend(w);
        }
        DwgGeom::Face3d { corners, hidden } => {
            head(w, "3DFACE", p.h, owner);
            w.sub("AcDbFace");
            for (k, q) in corners.iter().enumerate() {
                w.p(10 + k as u16, *q);
            }
            if *hidden & 0xF != 0 {
                w.i(70, i64::from(*hidden & 0xF));
            }
        }
        DwgGeom::Solid { corners, elevation } => {
            head(w, "SOLID", p.h, owner);
            w.sub("AcDbTrace");
            for (k, q) in corners.iter().enumerate() {
                w.p(10 + k as u16, q.extend(*elevation));
            }
        }
        DwgGeom::Text { pos, align, height, rotation, halign, valign, text } => {
            head(w, "TEXT", p.h, owner);
            w.sub("AcDbText");
            w.p(10, *pos);
            w.f(40, *height);
            w.s(1, text);
            if *rotation != 0.0 {
                w.f(50, rotation.to_degrees());
            }
            w.s(7, &fixed(v, "Standard"));
            if *halign != 0 {
                w.i(72, i64::from(*halign));
            }
            if *halign != 0 || *valign != 0 {
                w.p(11, *align);
            }
            w.sub("AcDbText");
            if *valign != 0 {
                w.i(73, i64::from(*valign));
            }
        }
        DwgGeom::Arc { center, normal, radius, start, end } => {
            let n = normal.normalize_or(DVec3::Z);
            head(w, "ARC", p.h, owner);
            w.sub("AcDbCircle");
            w.p(10, to_ocs(*center, n));
            w.f(40, *radius);
            if n != DVec3::Z {
                w.p(210, n);
            }
            w.sub("AcDbArc");
            w.f(50, start.to_degrees());
            w.f(51, end.to_degrees());
        }
        DwgGeom::Circle { center, normal, radius } => {
            let n = normal.normalize_or(DVec3::Z);
            head(w, "CIRCLE", p.h, owner);
            w.sub("AcDbCircle");
            w.p(10, to_ocs(*center, n));
            w.f(40, *radius);
            if n != DVec3::Z {
                w.p(210, n);
            }
        }
        DwgGeom::Point { p: pt } => {
            head(w, "POINT", p.h, owner);
            w.sub("AcDbPoint");
            w.p(10, *pt);
        }
        DwgGeom::XLine { p: pt, dir } => {
            head(w, "XLINE", p.h, owner);
            w.sub("AcDbXline");
            w.p(10, *pt);
            w.p(11, dir.normalize_or(DVec3::X));
        }
        DwgGeom::Insert { block, pos, scale, rotation, normal } => {
            let n = normal.normalize_or(DVec3::Z);
            head(w, "INSERT", p.h, owner);
            w.sub("AcDbBlockReference");
            w.s(2, &plan.block(block).name);
            w.p(10, to_ocs(*pos, n));
            for (c, s) in [(41, scale.x), (42, scale.y), (43, scale.z)] {
                if s != 1.0 {
                    w.f(c, s);
                }
            }
            if *rotation != 0.0 {
                w.f(50, rotation.to_degrees());
            }
            if n != DVec3::Z {
                w.p(210, n);
            }
        }
    }
}

/// The OBJECTS section (release 13 on): the objects of the DWG of the release.
fn objects(w: &mut W, plan: &Plan) {
    let v = w.v;
    w.section("OBJECTS");
    let dict = |w: &mut W, kind: &str, h: u64, owner: u64, entries: &[(&str, u64)], default: Option<u64>| {
        w.object(kind, h, owner);
        w.sub("AcDbDictionary");
        if w.v >= R2000 {
            w.i(281, 1); // cloning: keep existing
        }
        for (name, e) in entries {
            w.s(3, name);
            w.h(350, *e);
        }
        if let Some(d) = default {
            w.sub("AcDbDictionaryWithDefault");
            w.h(340, d);
        }
    };
    dict(w, "DICTIONARY", NAMED_DICT, 0, &named_objects(v), None);
    dict(w, "DICTIONARY", GROUP_DICT, NAMED_DICT, &[], None);
    dict(w, "DICTIONARY", MLSTYLE_DICT, NAMED_DICT, &[("Standard", MLSTYLE_STANDARD)], None);
    if v >= R2000 {
        dict(w, "DICTIONARY", LAYOUT_DICT, NAMED_DICT, &[("Layout1", LAYOUT_PAPER), ("Model", LAYOUT_MODEL)], None);
        dict(w, "DICTIONARY", PLOTSETTINGS_DICT, NAMED_DICT, &[], None);
        dict(
            w,
            "ACDBDICTIONARYWDFLT",
            PLOTSTYLE_DICT,
            NAMED_DICT,
            &[("Normal", PLOTSTYLE_NORMAL)],
            Some(PLOTSTYLE_NORMAL),
        );
        w.object("ACDBPLACEHOLDER", PLOTSTYLE_NORMAL, PLOTSTYLE_DICT);
    }
    if v >= R2004 {
        dict(w, "DICTIONARY", COLOR_DICT, NAMED_DICT, &[], None);
        dict(w, "DICTIONARY", MATERIAL_DICT, NAMED_DICT, &[], None);
    }
    if v >= R2007 {
        dict(w, "DICTIONARY", super::objects::VISUALSTYLE_DICT, NAMED_DICT, &[], None);
    }

    w.object("MLINESTYLE", MLSTYLE_STANDARD, MLSTYLE_DICT);
    w.sub("AcDbMlineStyle");
    w.raw(2, "Standard");
    w.i(70, 0);
    w.raw(3, "");
    w.i(62, 256); // fill colour
    w.f(51, 90.0);
    w.f(52, 90.0);
    w.i(71, 2);
    for offset in [0.5, -0.5] {
        w.f(49, offset);
        w.i(62, 256);
        w.raw(6, "BYLAYER");
    }

    if v >= R2000 {
        let (lo, hi) = plan.ext();
        for (h, name, tab, block, model_space) in LAYOUTS {
            w.object("LAYOUT", h, LAYOUT_DICT);
            w.sub("AcDbPlotSettings");
            w.raw(1, ""); // page setup
            w.raw(2, "none_device");
            w.raw(4, "ISO_A4_(210.00_x_297.00_MM)");
            w.raw(6, ""); // plot view
            for (c, x) in [(40, 7.5), (41, 20.0), (42, 7.5), (43, 20.0), (44, 210.0), (45, 297.0)] {
                w.f(c, x); // margins, paper size
            }
            for c in [46, 47, 48, 49, 140, 141] {
                w.f(c, 0.0); // plot origin, window
            }
            w.f(142, 1.0); // paper units
            w.f(143, 1.0); // drawing units
            w.i(70, if model_space { 1712 } else { 688 });
            w.i(72, 1); // millimetres
            w.i(73, 0); // rotation
            w.i(74, if model_space { 0 } else { 5 }); // display, layout
            w.raw(7, ""); // plot style table
            w.i(75, if model_space { 0 } else { 16 }); // fit, 1:1
            if v >= R2004 {
                w.i(76, 0); // shade plot: as displayed
                w.i(77, 2); // normal resolution
                w.i(78, 300); // DPI
            }
            w.f(147, 1.0);
            w.f(148, 0.0);
            w.f(149, 0.0);
            w.sub("AcDbLayout");
            w.raw(1, name);
            w.i(70, 1); // PSLTSCALE
            w.i(71, i64::from(tab));
            w.p2(10, DVec2::ZERO);
            w.p2(11, plan.limits);
            w.p(12, DVec3::ZERO);
            let (a, b) = if model_space { (lo, hi) } else { (DVec3::splat(1e20), DVec3::splat(-1e20)) };
            w.p(14, a);
            w.p(15, b);
            w.f(146, 0.0); // elevation
            w.p(13, DVec3::ZERO); // UCS
            w.p(16, DVec3::X);
            w.p(17, DVec3::Y);
            w.i(76, 0); // orthographic type
            w.h(330, block);
        }
    }
    w.end();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reals_are_written_as_autocad_does() {
        assert_eq!(real(0.0), "0.0");
        assert_eq!(real(-0.0), "0.0");
        assert_eq!(real(12.0), "12.0");
        assert_eq!(real(0.37), "0.37");
        assert_eq!(real(-2.25), "-2.25");
        assert_eq!(real(1e20), "1.000000000000000E+20");
        assert_eq!(real(-1e20), "-1.000000000000000E+20");
        assert_eq!(real(1.5e-12), "1.500000000000000E-12");
    }
}
