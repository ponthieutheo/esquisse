//! Release 12 files (`AC1009`), which the ODA's specification does not cover: the pre-R13
//! format, laid out as the ODA's own writer lays it out (SketchUp's exporter), with the
//! record layouts the `acadrust` reader documents:
//!
//! - a header of fixed layout (0x6BD bytes: the addresses of the sections, the locators of
//!   the ten symbol tables, the header variables at fixed offsets), its CRC, a sentinel;
//! - the model space entities: type, flags, size, layer index and option bits, then what
//!   the flags (colour, elevation, thickness, handle) and the options announce, and a
//!   CRC. Points are 2D at an elevation, but those of lines, points and 3D faces;
//! - the symbol tables, records of fixed size with a CRC each, between sentinels;
//! - the block definitions (BLOCK, entities, ENDBLK), which the BLOCK table addresses;
//! - an empty section of "extra" entities and an auxiliary header repeating the addresses.
//!
//! Every CRC is a CRC-16 seeded with 0xC0C1 and every closing sentinel the complement of
//! its opening one. Release 12 has no infinite lines: they are left out.

use anyhow::{Result, bail};
use glam::{DVec2, DVec3};

use super::bits::{crc16, encode_ansi};
use super::objects::{Dims, PAPER_SPACE, PS_BLOCK, PS_ENDBLK, Placed, Plan, julian_now, plan, to_ocs};
use super::{DwgColor, DwgDoc, DwgGeom, DwgVersion};

const VARIABLES_END: usize = 0x6BD;
const ENTITIES_START: usize = 0x6CF;

const ENTITIES: [u8; 16] =
    [0xC4, 0x6E, 0x68, 0x54, 0xF8, 0x6E, 0x33, 0x30, 0x63, 0x3E, 0xC1, 0x85, 0x2A, 0xDC, 0x94, 0x01];
const BLOCKS: [u8; 16] =
    [0x72, 0x2B, 0x7D, 0xEC, 0x3E, 0x8C, 0x88, 0x6C, 0x7A, 0x72, 0x0A, 0xFD, 0xC8, 0x6C, 0x84, 0x26];
const EXTRAS: [u8; 16] =
    [0xD5, 0xF9, 0xD3, 0xBB, 0x0A, 0xA9, 0x69, 0xA6, 0xCD, 0x1C, 0x87, 0xC7, 0xEE, 0x80, 0x4B, 0x17];
const AUX: [u8; 16] = [0x29, 0x8D, 0xD1, 0x49, 0xA9, 0x73, 0x1F, 0xEA, 0x99, 0xDE, 0x32, 0xF9, 0x4D, 0x0A, 0xE0, 0x19];
/// Opening sentinels of the tables: an empty pair first, then BLOCK, LAYER, STYLE, LTYPE,
/// VIEW, UCS, VPORT, APPID and DIMSTYLE.
const TABLE_SENTINELS: [[u8; 16]; 10] = [
    [0xDB, 0xEF, 0xB3, 0xF0, 0xC7, 0x3E, 0x6D, 0xA6, 0xC9, 0xB6, 0x24, 0x5C, 0x4C, 0x6F, 0x32, 0xCB],
    [0x0E, 0xC4, 0x64, 0x6F, 0xBB, 0x1D, 0xD3, 0x8B, 0x00, 0x49, 0xC2, 0xEF, 0x18, 0xEA, 0x6F, 0xFB],
    [0xE2, 0x3E, 0xC1, 0x82, 0x43, 0x9F, 0x61, 0x77, 0x50, 0xAB, 0xC7, 0x66, 0x96, 0x00, 0x06, 0x18],
    [0xAC, 0x90, 0x1A, 0xCA, 0x1C, 0xBD, 0x95, 0x15, 0x16, 0x16, 0x4C, 0x14, 0xCE, 0x18, 0x88, 0xAF],
    [0xC1, 0x3C, 0xAA, 0x56, 0x68, 0xF4, 0xB4, 0x1E, 0x4B, 0x74, 0xF4, 0x08, 0x42, 0x4D, 0xBF, 0xA5],
    [0x60, 0x4A, 0xFA, 0x3D, 0x84, 0x90, 0xCC, 0x5B, 0xEF, 0xE7, 0xD6, 0xA5, 0x7F, 0x1E, 0x61, 0xCD],
    [0xF6, 0xED, 0x44, 0x61, 0x2A, 0xDC, 0xE4, 0x7B, 0x4E, 0xB9, 0x2B, 0xBB, 0x66, 0x60, 0x63, 0x8D],
    [0xE1, 0x25, 0xC2, 0x50, 0x36, 0x68, 0x6C, 0x0C, 0x3B, 0xD3, 0x5D, 0x56, 0xC1, 0x79, 0x1C, 0x3A],
    [0xB4, 0x18, 0x3E, 0x42, 0xC9, 0x9F, 0xFF, 0xE5, 0xB6, 0xE2, 0xCB, 0xB3, 0x75, 0xC3, 0xC3, 0xB0],
    [0xE0, 0xCA, 0x36, 0x7C, 0xCE, 0xE7, 0x58, 0x6F, 0x2B, 0x7D, 0x74, 0x55, 0x05, 0xF1, 0x44, 0x7F],
];

fn closing(s: &[u8; 16]) -> [u8; 16] {
    s.map(|b| !b)
}

/// A symbol table: where its locator is in the header, its number in the auxiliary header,
/// the size of its records, its opening sentinel.
struct Table {
    locator: usize,
    id: u16,
    size: u16,
    sentinel: usize,
}

const BLOCK: Table = Table { locator: 0x2C, id: 1, size: 45, sentinel: 1 };
const LAYER: Table = Table { locator: 0x36, id: 3, size: 41, sentinel: 2 };
const STYLE: Table = Table { locator: 0x40, id: 5, size: 198, sentinel: 3 };
const LTYPE: Table = Table { locator: 0x4A, id: 2, size: 191, sentinel: 4 };
const VIEW: Table = Table { locator: 0x54, id: 6, size: 153, sentinel: 5 };
const UCS: Table = Table { locator: 0x3EF, id: 7, size: 109, sentinel: 6 };
const VPORT: Table = Table { locator: 0x500, id: 8, size: 253, sentinel: 7 };
const APPID: Table = Table { locator: 0x512, id: 9, size: 37, sentinel: 8 };
const DIMSTYLE: Table = Table { locator: 0x522, id: 10, size: 324, sentinel: 9 };
const VX: Table = Table { locator: 0x69F, id: 11, size: 43, sentinel: 0 };

/// Entity types.
const E_LINE: u8 = 1;
const E_POINT: u8 = 2;
const E_CIRCLE: u8 = 3;
const E_TEXT: u8 = 7;
const E_ARC: u8 = 8;
const E_SOLID: u8 = 11;
const E_BLOCK: u8 = 12;
const E_ENDBLK: u8 = 13;
const E_INSERT: u8 = 14;
const E_SEQEND: u8 = 17;
const E_POLYLINE: u8 = 19;
const E_VERTEX: u8 = 20;
const E_3DFACE: u8 = 22;

/// Entity flags.
const F_COLOR: u8 = 1;
const F_ELEVATION: u8 = 4;
const F_THICKNESS: u8 = 8;
const F_HANDLE: u8 = 0x20;
const F_EXTRA: u8 = 0x40;

/// Little-endian writing.
#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn rc(&mut self, v: u8) {
        self.0.push(v);
    }
    fn rs(&mut self, v: i16) {
        self.0.extend(v.to_le_bytes());
    }
    fn rl(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }
    fn rd(&mut self, v: f64) {
        self.0.extend(v.to_le_bytes());
    }
    fn rd2(&mut self, p: DVec2) {
        self.rd(p.x);
        self.rd(p.y);
    }
    fn rd3(&mut self, p: DVec3) {
        self.rd(p.x);
        self.rd(p.y);
        self.rd(p.z);
    }
    /// A string of fixed size, NUL-padded, in the code page.
    fn fixed(&mut self, s: &str, size: usize) {
        let mut b = encode_ansi(s);
        b.truncate(size - 1);
        b.resize(size, 0);
        self.0.extend(b);
    }
    /// A string of an entity: its length, then its bytes in the code page.
    fn text(&mut self, s: &str) {
        let b = encode_ansi(s);
        self.rs(b.len() as i16);
        self.0.extend(b);
    }
}

/// One entity: the common part, `data`, the size and the CRC.
struct Ent {
    kind: u8,
    flag: u8,
    layer: i16,
    opts: u16,
    color: Option<u8>,
    elevation: f64,
    thickness: f64,
    handle: u64,
    in_block: bool,
    data: W,
}

impl Ent {
    fn new(kind: u8, handle: u64, layer: usize, color: DwgColor, in_block: bool) -> Ent {
        let color = match color {
            DwgColor::ByLayer => None,
            DwgColor::ByBlock => Some(0),
            DwgColor::Aci(i) | DwgColor::Rgb { aci: i, .. } => Some(i.max(1)),
        };
        Ent {
            kind,
            flag: 0,
            layer: layer as i16,
            opts: 0,
            color,
            elevation: 0.0,
            thickness: 0.0,
            handle,
            in_block,
            data: W::default(),
        }
    }

    fn bytes(self) -> Vec<u8> {
        let mut flag = self.flag | F_HANDLE;
        if self.color.is_some() {
            flag |= F_COLOR;
        }
        if self.elevation != 0.0 {
            flag |= F_ELEVATION;
        }
        if self.thickness != 0.0 {
            flag |= F_THICKNESS;
        }
        if self.in_block {
            // As the ODA's writer: entities of blocks say they are in viewport 1.
            flag |= F_EXTRA;
        }
        let mut w = W::default();
        w.rc(self.kind);
        w.rc(flag);
        w.rs(0); // size, below
        w.rs(self.layer);
        w.rs(self.opts as i16);
        if self.in_block {
            w.rc(4);
        }
        if let Some(c) = self.color {
            w.rc(c);
        }
        if self.elevation != 0.0 {
            w.rd(self.elevation);
        }
        if self.thickness != 0.0 {
            w.rd(self.thickness);
        }
        // The handle: its significant bytes, most significant first.
        let h = self.handle.to_be_bytes();
        let first = h.iter().position(|b| *b != 0).unwrap_or(7);
        w.rc((8 - first) as u8);
        w.0.extend_from_slice(&h[first..]);
        if self.in_block {
            w.rs(1);
        }
        w.0.extend(self.data.0);
        let size = (w.0.len() + 2) as u16;
        w.0[2..4].copy_from_slice(&size.to_le_bytes());
        let crc = crc16(0xC0C1, &w.0);
        w.0.extend(crc.to_le_bytes());
        w.0
    }
}

/// The index of a block in the BLOCK table: *PAPER_SPACE, then the blocks of the drawing.
fn block_index(plan: &Plan, name: &str) -> i16 {
    plan.block_index(name) as i16 + 1
}

/// Writes the entities of one block or of model space at `base` (file offset of `out`).
fn entities(out: &mut Vec<u8>, base: usize, plan: &Plan, ents: &[Placed], in_block: bool) -> Result<()> {
    for p in ents {
        let layer = plan.layer(&p.e.layer);
        let new = |kind: u8, h: u64| Ent::new(kind, h, layer, p.e.color, in_block);
        let at = base + out.len();
        let mut subs: Vec<Ent> = Vec::new();
        let e = match &p.e.geom {
            DwgGeom::Line { a, b, thickness } => {
                let mut e = new(E_LINE, p.h);
                e.thickness = *thickness;
                e.data.rd3(*a);
                e.data.rd3(*b);
                e
            }
            DwgGeom::LwPolyline { pts, closed, width, elevation } => {
                // No lightweight polylines: a 2D POLYLINE.
                let mut e = new(E_POLYLINE, p.h);
                e.elevation = *elevation;
                if *closed {
                    e.opts |= 1;
                    e.data.rc(1);
                }
                if *width != 0.0 {
                    e.opts |= 2 | 4;
                    e.data.rd(*width);
                    e.data.rd(*width);
                }
                for (q, h) in pts.iter().zip(&p.subs) {
                    let mut v = new(E_VERTEX, *h);
                    v.data.rd2(*q);
                    if *width != 0.0 {
                        v.opts |= 1 | 2;
                        v.data.rd(*width);
                        v.data.rd(*width);
                    }
                    subs.push(v);
                }
                e
            }
            DwgGeom::Polyline3d { pts, closed } => {
                let mut e = new(E_POLYLINE, p.h);
                e.opts |= 1;
                e.data.rc(8 | u8::from(*closed));
                for (q, h) in pts.iter().zip(&p.subs) {
                    let mut v = new(E_VERTEX, *h);
                    v.elevation = q.z;
                    v.opts |= 8;
                    v.data.rd2(q.truncate());
                    v.data.rc(32);
                    subs.push(v);
                }
                e
            }
            DwgGeom::PolyFace { verts, faces } => {
                let mut e = new(E_POLYLINE, p.h);
                e.opts |= 1 | 16 | 32;
                e.data.rc(64);
                e.data.rs(verts.len() as i16);
                e.data.rs(faces.len() as i16);
                for (q, h) in verts.iter().zip(&p.subs) {
                    let mut v = new(E_VERTEX, *h);
                    v.elevation = q.z;
                    v.opts |= 8;
                    v.data.rd2(q.truncate());
                    v.data.rc(192);
                    subs.push(v);
                }
                for (f, h) in faces.iter().zip(&p.subs[verts.len()..]) {
                    // A face record has no point (option 0x4000).
                    let mut v = new(E_VERTEX, *h);
                    v.opts |= 0x4000 | 8;
                    v.data.rc(128);
                    for (k, &i) in f.iter().enumerate() {
                        if k < 3 || i != 0 {
                            v.opts |= 32 << k;
                            v.data.rs(i.clamp(-32_767, 32_767) as i16);
                        }
                    }
                    subs.push(v);
                }
                e
            }
            DwgGeom::Face3d { corners, hidden } => {
                let mut e = new(E_3DFACE, p.h);
                for q in corners {
                    e.data.rd3(*q);
                }
                if *hidden & 0xF != 0 {
                    e.opts |= 1;
                    e.data.rs(i16::from(*hidden & 0xF));
                }
                e
            }
            DwgGeom::Solid { corners, elevation } => {
                let mut e = new(E_SOLID, p.h);
                e.elevation = *elevation;
                for q in corners {
                    e.data.rd2(*q);
                }
                e
            }
            DwgGeom::Text { pos, align, height, rotation, halign, valign, text } => {
                let mut e = new(E_TEXT, p.h);
                e.elevation = pos.z;
                e.data.rd2(pos.truncate());
                e.data.rd(*height);
                e.data.text(text);
                if *rotation != 0.0 {
                    e.opts |= 1;
                    e.data.rd(*rotation);
                }
                if *halign != 0 {
                    e.opts |= 32;
                    e.data.rc(*halign);
                }
                if *halign != 0 || *valign != 0 {
                    e.opts |= 64;
                    e.data.rd2(align.truncate());
                }
                if *valign != 0 {
                    e.opts |= 256;
                    e.data.rc(*valign);
                }
                e
            }
            DwgGeom::Arc { center, normal, radius, start, end } => {
                let n = normal.normalize_or(DVec3::Z);
                let c = to_ocs(*center, n);
                let mut e = new(E_ARC, p.h);
                e.elevation = c.z;
                e.data.rd2(c.truncate());
                e.data.rd(*radius);
                e.data.rd(*start);
                e.data.rd(*end);
                if n != DVec3::Z {
                    e.opts |= 1;
                    e.data.rd3(n);
                }
                e
            }
            DwgGeom::Circle { center, normal, radius } => {
                let n = normal.normalize_or(DVec3::Z);
                let c = to_ocs(*center, n);
                let mut e = new(E_CIRCLE, p.h);
                e.elevation = c.z;
                e.data.rd2(c.truncate());
                e.data.rd(*radius);
                if n != DVec3::Z {
                    e.opts |= 1;
                    e.data.rd3(n);
                }
                e
            }
            DwgGeom::Point { p: q } => {
                let mut e = new(E_POINT, p.h);
                e.data.rd3(*q);
                e
            }
            DwgGeom::XLine { .. } => bail!("la version 12 n'a pas de lignes infinies"),
            DwgGeom::Insert { block, pos, scale, rotation, normal } => {
                let n = normal.normalize_or(DVec3::Z);
                let o = to_ocs(*pos, n);
                let mut e = new(E_INSERT, p.h);
                e.elevation = o.z;
                e.data.rs(block_index(plan, block));
                e.data.rd2(o.truncate());
                if scale.x != 1.0 {
                    e.opts |= 1;
                    e.data.rd(scale.x);
                }
                if scale.y != 1.0 {
                    e.opts |= 2;
                    e.data.rd(scale.y);
                }
                if *rotation != 0.0 {
                    e.opts |= 4;
                    e.data.rd(*rotation);
                }
                if scale.z != 1.0 {
                    e.opts |= 8;
                    e.data.rd(scale.z);
                }
                if n != DVec3::Z {
                    e.opts |= 256;
                    e.data.rd3(n);
                }
                e
            }
        };
        out.extend(e.bytes());
        if p.seqend != 0 {
            for v in subs {
                out.extend(v.bytes());
            }
            // SEQEND: the file offset of its POLYLINE.
            let mut s = new(E_SEQEND, p.seqend);
            s.data.rl(at as u32);
            out.extend(s.bytes());
        }
    }
    Ok(())
}

/// The definitions of the blocks: *PAPER_SPACE, then those of the drawing, and where each
/// starts in the section.
fn blocks(base: usize, plan: &Plan) -> Result<(Vec<u8>, Vec<u32>)> {
    let mut out = Vec::new();
    let mut starts = Vec::new();
    let begin_end = |out: &mut Vec<u8>, block: u64, endblk: u64, base_pt: DVec3, ents: &[Placed]| -> Result<()> {
        let mut b = Ent::new(E_BLOCK, block, 0, DwgColor::ByLayer, true);
        b.elevation = base_pt.z;
        b.data.rd2(base_pt.truncate());
        out.extend(b.bytes());
        entities(out, base, plan, ents, true)?;
        out.extend(Ent::new(E_ENDBLK, endblk, 0, DwgColor::ByLayer, true).bytes());
        Ok(())
    };
    starts.push(out.len() as u32);
    begin_end(&mut out, PS_BLOCK, PS_ENDBLK, DVec3::ZERO, &[])?;
    for s in &plan.spaces[..plan.model] {
        starts.push(out.len() as u32);
        begin_end(&mut out, s.block, s.endblk, s.base, &s.ents)?;
    }
    Ok((out, starts))
}

/// A table record: flag, name, the "used" counter (-1), `data`, CRC.
fn record(t: &Table, name: &str, data: W) -> Vec<u8> {
    let mut w = W::default();
    w.rc(0);
    w.fixed(name, 32);
    w.rs(-1);
    w.0.extend(data.0);
    debug_assert_eq!(w.0.len() + 2, usize::from(t.size));
    let crc = crc16(0xC0C1, &w.0);
    w.0.extend(crc.to_le_bytes());
    w.0
}

/// The records of every table, in the order of the file.
fn tables(plan: &Plan, doc: &DwgDoc, block_starts: &[u32]) -> Vec<(&'static Table, Vec<Vec<u8>>)> {
    let metric = plan.metric;
    let d = Dims::new(metric);
    let mut out: Vec<(&'static Table, Vec<Vec<u8>>)> = Vec::new();

    let mut names = vec!["*PAPER_SPACE".to_string()];
    names.extend(plan.spaces[..plan.model].iter().map(|s| s.name.clone()));
    let blocks = names
        .iter()
        .zip(block_starts)
        .map(|(n, at)| {
            let mut w = W::default();
            w.rl(at | 0x4000_0000);
            w.rs(0);
            w.rs(0);
            record(&BLOCK, n, w)
        })
        .collect();
    out.push((&BLOCK, blocks));

    let mut w = W::default();
    w.fixed("Solid line", 48);
    w.rc(b'A');
    w.rc(0); // dashes
    w.rd(0.0); // pattern length
    for _ in 0..12 {
        w.rd(0.0);
    }
    out.push((&LTYPE, vec![record(&LTYPE, "CONTINUOUS", w)]));

    let layers = plan
        .layers
        .iter()
        .map(|(l, _)| {
            let mut w = W::default();
            let c = i16::from(l.color.max(1));
            w.rs(if l.visible { c } else { -c });
            w.rs(0); // CONTINUOUS
            record(&LAYER, &l.name, w)
        })
        .collect();
    out.push((&LAYER, layers));

    let mut w = W::default();
    w.rd(0.0); // fixed height
    w.rd(1.0); // width factor
    w.rd(0.0); // oblique
    w.rc(0); // generation
    w.rd(plan.text_size);
    w.fixed(doc.font(DwgVersion::R12), 64);
    w.fixed("", 64);
    out.push((&STYLE, vec![record(&STYLE, "STANDARD", w)]));

    out.push((&VIEW, Vec::new()));
    out.push((&UCS, Vec::new()));

    let (center, height, aspect) = plan.view();
    let grid = DVec2::splat(if metric { 10.0 } else { 0.5 });
    let mut w = W::default();
    w.rd2(DVec2::ZERO);
    w.rd2(DVec2::ONE);
    w.rd3(DVec3::ZERO); // target
    w.rd3(DVec3::Z); // direction
    w.rd(0.0); // twist
    w.rd(height);
    w.rd2(center);
    w.rd(1.0 / aspect); // height over width, as the ODA writes it
    w.rd(50.0); // lens
    w.rd(0.0); // front clip
    w.rd(0.0); // back clip
    for v in [0, 100, 1, 3, 0, 0, 0, 0] {
        w.rs(v); // view mode, circle zoom, fast zoom, UCS icon, snap, grid, snap style, isopair
    }
    w.rd(0.0); // snap rotation
    w.rd2(DVec2::ZERO);
    w.rd2(grid);
    w.rd2(grid);
    out.push((&VPORT, vec![record(&VPORT, "*ACTIVE", w)]));

    out.push((&APPID, vec![record(&APPID, "ACAD", W::default())]));

    let mut w = W::default();
    for x in [d.scale, d.asz, d.exo, d.dli, d.exe, 0.0, 0.0, 0.0, 0.0, d.txt, d.cen, 0.0, d.altf, 1.0, 0.0] {
        w.rd(x); // DIMSCALE … DIMTSZ, DIMTXT DIMCEN DIMTSZ DIMALTF DIMLFAC DIMTVP
    }
    for x in [0, 0, 1, 1, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0] {
        w.rc(x); // DIMTOL … DIMZIN, DIMALT DIMALTD DIMTOFL DIMSAH DIMTIX DIMSOXD
    }
    for size in [16, 16, 16, 16, 66] {
        w.fixed("", size); // DIMPOST DIMAPOST DIMBLK DIMBLK1 DIMBLK2
    }
    for _ in 0..3 {
        w.rs(0); // DIMCLRD DIMCLRE DIMCLRT: by block
    }
    w.rc(0);
    w.rd(1.0); // DIMTFAC
    w.rd(d.gap);
    out.push((&DIMSTYLE, vec![record(&DIMSTYLE, "STANDARD", w)]));
    out
}

/// The header (up to its CRC and the sentinel of the entities).
struct Header {
    b: Vec<u8>,
    at: usize,
}

impl Header {
    fn seek(&mut self, at: usize) {
        self.at = at;
    }
    fn put(&mut self, bytes: &[u8]) {
        self.b[self.at..self.at + bytes.len()].copy_from_slice(bytes);
        self.at += bytes.len();
    }
    fn rs(&mut self, v: i16) {
        self.put(&v.to_le_bytes());
    }
    fn rl(&mut self, v: u32) {
        self.put(&v.to_le_bytes());
    }
    fn rd(&mut self, v: f64) {
        self.put(&v.to_le_bytes());
    }
    fn rds(&mut self, vs: &[f64]) {
        for v in vs {
            self.rd(*v);
        }
    }
    fn rss(&mut self, vs: &[i16]) {
        for v in vs {
            self.rs(*v);
        }
    }
    fn rd2(&mut self, p: DVec2) {
        self.rds(&[p.x, p.y]);
    }
    fn rd3(&mut self, p: DVec3) {
        self.rds(&[p.x, p.y, p.z]);
    }
    fn text(&mut self, s: &str, size: usize) {
        let mut b = encode_ansi(s);
        b.truncate(size - 1);
        b.resize(size, 0);
        self.put(&b);
    }
    fn locator(&mut self, size: u16, count: usize, address: usize) {
        self.rs(size as i16);
        self.rs(count as i16);
        self.rs(0);
        self.rl(address as u32);
    }
}

struct Layout {
    entities_end: usize,
    blocks_start: usize,
    blocks_size: usize,
    /// Size, count and address of every table, by locator offset.
    tables: Vec<(usize, u16, usize, usize)>,
}

fn header(plan: &Plan, l: &Layout) -> Vec<u8> {
    let d = Dims::new(plan.metric);
    let (lo, hi) = plan.ext();
    let (center, height, _) = plan.view();
    let grid = DVec2::splat(if plan.metric { 10.0 } else { 0.5 });
    let (day, ms) = julian_now();
    let mut h = Header { b: vec![0; VARIABLES_END], at: 0 };
    h.put(b"AC1009");
    h.seek(0x0C);
    h.put(&[1]);
    h.rss(&[3, 5, 205]); // entity sections, tables of the first block of the header, variables
    h.put(&[0]);
    h.rl(ENTITIES_START as u32);
    h.rl(l.entities_end as u32);
    h.rl(l.blocks_start as u32);
    h.rl(l.blocks_size as u32 | 0x4000_0000);
    h.rl(0); // no extra entities
    h.rl(0x8000_0000);
    for &(at, size, count, address) in &l.tables {
        h.seek(at);
        h.locator(size, count, address);
    }
    h.seek(0x5E);
    h.rd3(DVec3::ZERO); // INSBASE
    h.rs(0); // PLINEGEN
    h.rd3(lo); // EXTMIN
    h.rd3(hi); // EXTMAX
    h.rd2(DVec2::ZERO); // LIMMIN
    h.rd2(plan.limits); // LIMMAX
    h.rd3(center.extend(0.0)); // VIEWCTR
    h.rd(height); // VIEWSIZE
    h.rs(0); // SNAPMODE
    h.rd2(grid); // SNAPUNIT
    h.rd2(DVec2::ZERO); // SNAPBASE
    h.rd(0.0); // SNAPANG
    h.rss(&[0, 0, 0]); // SNAPSTYLE SNAPISOPAIR GRIDMODE
    h.rd2(grid); // GRIDUNIT
    h.rss(&[0, 1, 1, 0, 2]); // ORTHOMODE REGENMODE FILLMODE QTEXTMODE DRAGMODE
    h.rds(&[1.0, plan.text_size, 0.05]); // LTSCALE TEXTSIZE TRACEWID
    h.rs(0); // CLAYER: layer 0
    h.put(&[0x0F, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // (as the ODA writes it)
    h.rss(&[1, 3020]); // PSLTSCALE TREEDEPTH
    h.seek(0x164);
    h.rss(&[2, 4]); // LUNITS LUPREC
    h.seek(0x17A);
    h.rds(&[0.1, 0.0]); // SKETCHINC FILLETRAD
    h.rss(&[0, 0, 0, 37, 1]); // AUNITS AUPREC TEXTSTYLE OSMODE ATTMODE
    h.text("acad", 15); // MENU
    h.rds(&[d.scale, d.asz, d.exo, d.dli, d.exe, 0.0, 0.0, d.txt, d.cen, 0.0]); // DIMSCALE … DIMTSZ
    h.put(&[0, 0, 1, 1, 0, 0, 0, 0]); // DIMTOL DIMLIM DIMTIH DIMTOH DIMSE1 DIMSE2 DIMTAD LIMCHECK
    h.seek(0x229);
    h.rds(&[0.0, 0.0]); // ELEVATION THICKNESS
    h.rd3(DVec3::Z); // VIEWDIR
    h.seek(0x2E3);
    h.rs(0); // BLIPMODE
    h.put(&[0]); // DIMZIN
    h.rds(&[0.0, 0.0]); // DIMRND DIMDLE
    h.text("", 33); // DIMBLK
    h.seek(0x319);
    h.rss(&[1, 256, 32767]); // COORDS CECOLOR CELTYPE (by layer)
    for t in [day, ms, day, ms, 0, 0, 0, 0] {
        h.rl(t as u32); // TDCREATE TDUPDATE TDINDWG TDUSRTIMER
    }
    h.rss(&[1, 1, 0]); // USRTIMER, (as the ODA writes it), SKPOLY
    h.seek(0x353);
    h.rd(0.0); // ANGBASE
    h.rss(&[0, 0]); // ANGDIR PDMODE
    h.rds(&[0.0, 0.0]); // PDSIZE PLINEWID
    h.seek(0x3A1);
    h.put(&[0, 2, 1, 1]); // DIMALT DIMALTD DIMASO DIMSHO
    h.text("", 16); // DIMPOST
    h.text("", 16); // DIMAPOST
    h.rds(&[d.altf, 1.0]); // DIMALTF DIMLFAC
    h.rss(&[8, 0, 1, 1]); // SPLINESEGS SPLFRAME ATTREQ ATTDIA
    h.rds(&[0.0, 0.0]); // CHAMFERA CHAMFERB
    h.rs(1); // MIRRTEXT
    h.seek(0x3F9);
    h.rs(30); // DWGCODEPAGE: ANSI_1252
    h.rd3(DVec3::ZERO); // UCSORG
    h.rd3(DVec3::X);
    h.rd3(DVec3::Y);
    h.rd3(DVec3::ZERO); // TARGET
    h.rds(&[50.0, 0.0, 0.0, 0.0]); // LENSLENGTH VIEWTWIST FRONTZ BACKZ
    h.rs(0); // VIEWMODE
    h.put(&[0]); // DIMTOFL
    h.text("", 33); // DIMBLK1
    h.text("", 33); // DIMBLK2
    h.put(&[0, 0, 0]); // DIMSAH DIMTIX DIMSOXD
    h.rd(0.0); // DIMTVP
    h.seek(0x4EC);
    h.rs(1); // HANDLING
    h.put(&plan.handseed.to_le_bytes()); // HANDSEED
    h.rss(&[6, 6, 6, 6, 6]); // SURFU SURFV SURFTYPE SURFTAB1 SURFTAB2
    h.seek(0x50C);
    h.rss(&[6, 3, -1]); // SPLINETYPE UCSICON UCSNAME
    h.seek(0x51C);
    h.rss(&[0, 5, 1]); // (as the ODA writes them)
    h.seek(0x531);
    h.rss(&[0, 0, 0, 3, 70]); // DIMCLRD DIMCLRE DIMCLRT SHADEDGE SHADEDIF
    h.seek(0x541);
    h.rs(0x1C); // (as the ODA writes them)
    h.put(&[0x7E, 0xFD, 0x57, 0x42]);
    h.seek(0x547);
    h.rds(&[1.0, 1.0, 1.0]);
    h.rs(0x6D);
    h.seek(0x5DF);
    h.rd(1.0); // DIMTFAC
    h.rd3(DVec3::ZERO); // PUCSORG
    h.rd3(DVec3::X);
    h.rd3(DVec3::Y);
    h.rss(&[-1, 1, 0, 0]); // PUCSNAME TILEMODE PLIMCHECK
    h.rd3(DVec3::splat(1e20)); // PEXTMIN
    h.rd3(DVec3::splat(-1e20)); // PEXTMAX
    h.rd2(DVec2::ZERO); // PLIMMIN
    h.rd2(plan.limits); // PLIMMAX
    h.rd3(DVec3::ZERO); // PINSBASE
    h.seek(0x6A9);
    h.rs(64); // MAXACTVP
    h.rds(&[d.gap, 0.0]); // DIMGAP PELEVATION
    h.rs(1); // VISRETAIN
    debug_assert_eq!(h.at, VARIABLES_END);
    let crc = crc16(0xC0C1, &h.b);
    h.b.extend(crc.to_le_bytes());
    h.b.extend(ENTITIES);
    h.b
}

/// The auxiliary header: the addresses again, the handle seed, the tables.
fn aux(l: &Layout, extras: usize, handseed: u64, at: usize, order: &[&Table]) -> Vec<u8> {
    let mut w = W::default();
    w.rs(16);
    w.rs(138);
    for a in [ENTITIES_START, l.entities_end, l.blocks_start, extras] {
        w.rl(a as u32);
    }
    w.rs(1);
    w.0.extend(handseed.to_le_bytes());
    w.rs(order.len() as i16);
    for t in order {
        let &(_, size, count, _) = l.tables.iter().find(|x| x.0 == t.locator).unwrap();
        w.rs(t.id as i16);
        w.rs(size as i16);
        w.rs(count as i16);
        w.rl(0);
    }
    w.rl(at as u32);
    let mut out = AUX.to_vec();
    let crc = crc16(0xC0C1, &w.0);
    out.extend(w.0);
    out.extend(crc.to_le_bytes());
    out.extend(closing(&AUX));
    out
}

pub(super) fn write(doc: &DwgDoc) -> Result<Vec<u8>> {
    let plan = plan(doc, DwgVersion::R12)?;
    // Entities of model space.
    let mut ents = Vec::new();
    entities(&mut ents, ENTITIES_START, &plan, &plan.spaces[plan.model].ents, false)?;
    let entities_end = ENTITIES_START + ents.len();
    debug_assert!(plan.spaces[plan.model + 1].header == PAPER_SPACE);

    // The tables come next, then the blocks: their sizes first.
    let n_blocks = plan.model + 1;
    let counts = [n_blocks, 1, plan.layers.len(), 1, 0, 0, 1, 1, 1];
    let order = [&BLOCK, &LTYPE, &LAYER, &STYLE, &VIEW, &UCS, &VPORT, &APPID, &DIMSTYLE];
    let tables_size: usize = 32 + order.iter().zip(counts).map(|(t, n)| 32 + usize::from(t.size) * n).sum::<usize>();
    let blocks_start = entities_end + 16 + tables_size + 16;
    let (block_bytes, block_starts) = blocks(blocks_start, &plan)?;

    let mut tail = Vec::new();
    tail.extend(closing(&ENTITIES));
    tail.extend(TABLE_SENTINELS[0]);
    tail.extend(closing(&TABLE_SENTINELS[0]));
    let mut located = Vec::new();
    for (t, records) in tables(&plan, doc, &block_starts) {
        let s = TABLE_SENTINELS[t.sentinel];
        tail.extend(s);
        located.push((t.locator, t.size, records.len(), entities_end + tail.len()));
        for r in records {
            tail.extend(r);
        }
        tail.extend(closing(&s));
    }
    debug_assert_eq!(entities_end + tail.len() + 16, blocks_start);
    located.push((VX.locator, VX.size, 0, blocks_start));
    tail.extend(BLOCKS);
    tail.extend(&block_bytes);
    tail.extend(closing(&BLOCKS));
    tail.extend(EXTRAS);
    let extras = entities_end + tail.len();
    tail.extend(closing(&EXTRAS));
    let layout = Layout { entities_end, blocks_start, blocks_size: block_bytes.len(), tables: located };
    let aux_at = entities_end + tail.len();
    let mut all_tables: Vec<&Table> = order.to_vec();
    all_tables.push(&VX);
    tail.extend(aux(&layout, extras, plan.handseed, aux_at, &all_tables));

    let mut out = header(&plan, &layout);
    debug_assert_eq!(out.len(), ENTITIES_START);
    out.extend(ents);
    out.extend(tail);
    Ok(out)
}
