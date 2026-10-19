//! The objects of a drawing, coded for one release: header variables (spec chapter 9),
//! classes (chapter 10) and the object stream (chapter 20). The containers ([`super::r13`],
//! [`super::r2004`], [`super::r2007`]) only lay these out in the file.
//!
//! Every handle is written absolute. Before 2004 entities are chained with explicit
//! previous / next links and polylines point to their first and last vertex; from 2004
//! owners list what they own. Releases before 2004 have no true colours: entities carry
//! an AutoCAD Color Index, which later releases keep.

use std::collections::HashMap;

use anyhow::{Result, bail};
use glam::{DVec2, DVec3};

use super::bits::{Bits, crc16, encode_ansi, modular_char_u, modular_short};
use super::{DwgBlock, DwgColor, DwgDoc, DwgEntity, DwgGeom, DwgLayer, DwgVersion};
use DwgVersion::{R12, R13, R14, R2000, R2004, R2007, R2010, R2013, R2018};

// ---------------------------------------------------------------------------------------
// Handles of the objects every drawing has.

pub(super) const BLOCK_CONTROL: u64 = 0x1;
pub(super) const LAYER_CONTROL: u64 = 0x2;
pub(super) const STYLE_CONTROL: u64 = 0x3;
pub(super) const LTYPE_CONTROL: u64 = 0x5;
pub(super) const VIEW_CONTROL: u64 = 0x6;
pub(super) const UCS_CONTROL: u64 = 0x7;
pub(super) const VPORT_CONTROL: u64 = 0x8;
pub(super) const APPID_CONTROL: u64 = 0x9;
pub(super) const DIMSTYLE_CONTROL: u64 = 0xA;
pub(super) const VPENT_CONTROL: u64 = 0xB;
pub(super) const NAMED_DICT: u64 = 0xC;
pub(super) const GROUP_DICT: u64 = 0xD;
pub(super) const MLSTYLE_DICT: u64 = 0xE;
pub(super) const LAYER0: u64 = 0xF;
pub(super) const STYLE_STANDARD: u64 = 0x10;
pub(super) const APPID_ACAD: u64 = 0x11;
pub(super) const LT_BYBLOCK: u64 = 0x12;
pub(super) const LT_BYLAYER: u64 = 0x13;
pub(super) const LT_CONTINUOUS: u64 = 0x14;
pub(super) const DIMSTYLE_STANDARD: u64 = 0x15;
pub(super) const VPORT_ACTIVE: u64 = 0x16;
pub(super) const MLSTYLE_STANDARD: u64 = 0x17;
pub(super) const LAYOUT_DICT: u64 = 0x18;
pub(super) const PLOTSETTINGS_DICT: u64 = 0x19;
pub(super) const PLOTSTYLE_DICT: u64 = 0x1A;
pub(super) const PLOTSTYLE_NORMAL: u64 = 0x1B;
pub(super) const MODEL_SPACE: u64 = 0x1C;
pub(super) const PAPER_SPACE: u64 = 0x1D;
pub(super) const MS_BLOCK: u64 = 0x1E;
pub(super) const MS_ENDBLK: u64 = 0x1F;
pub(super) const PS_BLOCK: u64 = 0x20;
pub(super) const PS_ENDBLK: u64 = 0x21;
pub(super) const LAYOUT_MODEL: u64 = 0x22;
pub(super) const LAYOUT_PAPER: u64 = 0x23;
pub(super) const COLOR_DICT: u64 = 0x24;
pub(super) const MATERIAL_DICT: u64 = 0x25;
pub(super) const VISUALSTYLE_DICT: u64 = 0x26;
pub(super) const FIRST_FREE: u64 = 0x27;

// Object types (spec §20.3); the class-based ones are numbered by the classes section.
pub(super) const T_TEXT: u16 = 0x01;
const T_BLOCK: u16 = 0x04;
const T_ENDBLK: u16 = 0x05;
const T_SEQEND: u16 = 0x06;
const T_INSERT: u16 = 0x07;
const T_VERTEX_2D: u16 = 0x0A;
const T_VERTEX_3D: u16 = 0x0B;
const T_VERTEX_PFACE: u16 = 0x0D;
const T_PFACE_FACE: u16 = 0x0E;
const T_POLYLINE_2D: u16 = 0x0F;
const T_POLYLINE_3D: u16 = 0x10;
const T_ARC: u16 = 0x11;
const T_CIRCLE: u16 = 0x12;
pub(super) const T_LINE: u16 = 0x13;
const T_POINT: u16 = 0x1B;
const T_3DFACE: u16 = 0x1C;
const T_POLYLINE_PFACE: u16 = 0x1D;
const T_SOLID: u16 = 0x1F;
const T_XLINE: u16 = 0x29;
const T_DICTIONARY: u16 = 0x2A;
const T_BLOCK_CONTROL: u16 = 0x30;
const T_BLOCK_HEADER: u16 = 0x31;
const T_LAYER_CONTROL: u16 = 0x32;
const T_LAYER: u16 = 0x33;
const T_STYLE_CONTROL: u16 = 0x34;
const T_STYLE: u16 = 0x35;
const T_LTYPE_CONTROL: u16 = 0x38;
const T_LTYPE: u16 = 0x39;
const T_VIEW_CONTROL: u16 = 0x3C;
const T_UCS_CONTROL: u16 = 0x3E;
const T_VPORT_CONTROL: u16 = 0x40;
pub(super) const T_VPORT: u16 = 0x41;
const T_APPID_CONTROL: u16 = 0x42;
const T_APPID: u16 = 0x43;
const T_DIMSTYLE_CONTROL: u16 = 0x44;
pub(super) const T_DIMSTYLE: u16 = 0x45;
const T_VPENT_CONTROL: u16 = 0x46;
pub(super) const T_MLINESTYLE: u16 = 0x49;
const T_LWPOLYLINE: u16 = 0x4D;
const T_DICTIONARYWDFLT: u16 = 500;
pub(super) const T_PLACEHOLDER: u16 = 501;
const T_LAYOUT: u16 = 502;

/// Handle reference codes (spec §2.13).
const SOFT_OWNER: u8 = 2;
const HARD_OWNER: u8 = 3;
const SOFT_PTR: u8 = 4;
const HARD_PTR: u8 = 5;

pub(super) const HEADER_START: [u8; 16] =
    [0xCF, 0x7B, 0x1F, 0x23, 0xFD, 0xDE, 0x38, 0xA9, 0x5F, 0x7C, 0x68, 0xB8, 0x4E, 0x6D, 0x33, 0x5F];
pub(super) const HEADER_END: [u8; 16] =
    [0x30, 0x84, 0xE0, 0xDC, 0x02, 0x21, 0xC7, 0x56, 0xA0, 0x83, 0x97, 0x47, 0xB1, 0x92, 0xCC, 0xA0];
pub(super) const CLASSES_START: [u8; 16] =
    [0x8D, 0xA1, 0xC4, 0xB8, 0xC4, 0xA9, 0xF8, 0xC5, 0xC0, 0xDC, 0xF4, 0x5F, 0xE7, 0xCF, 0xB6, 0x8A];
pub(super) const CLASSES_END: [u8; 16] =
    [0x72, 0x5E, 0x3B, 0x47, 0x3B, 0x56, 0x07, 0x3A, 0x3F, 0x23, 0x0B, 0xA0, 0x18, 0x30, 0x49, 0x75];
pub(super) const IMAGE_START: [u8; 16] =
    [0x1F, 0x25, 0x6D, 0x07, 0xD4, 0x36, 0x28, 0x28, 0x9D, 0x57, 0xCA, 0x3F, 0x9D, 0x44, 0x10, 0x2B];
pub(super) const IMAGE_END: [u8; 16] =
    [0xE0, 0xDA, 0x92, 0xF8, 0x2B, 0xC9, 0xD7, 0xD7, 0x62, 0xA8, 0x35, 0xC0, 0x62, 0xBB, 0xEF, 0xD4];

/// Lineweight index of "ByLayer" in entities, and of "Default" in layers.
const LW_BYLAYER: u8 = 29;
const LW_DEFAULT: u8 = 31;

/// What the containers lay out.
pub(super) struct Built {
    pub version: DwgVersion,
    /// Every object, by handle.
    pub objects: Vec<(u64, Vec<u8>)>,
    /// The header variables section, sentinels included.
    pub header: Vec<u8>,
    /// The classes section, sentinels included.
    pub classes: Vec<u8>,
    pub handseed: u64,
    pub metric: bool,
    /// Julian day and milliseconds of the day of the export.
    pub date: (i32, i32),
}

impl Built {
    /// The empty preview (spec §14.2): sentinel, size, no image, sentinel.
    pub fn preview(&self) -> Vec<u8> {
        let mut out = IMAGE_START.to_vec();
        out.extend(1u32.to_le_bytes());
        out.push(0);
        out.extend_from_slice(&IMAGE_END);
        out
    }

    /// The template section (chapter 22): no description, then MEASUREMENT.
    pub fn template(&self) -> Vec<u8> {
        let mut out = 0u16.to_le_bytes().to_vec();
        out.extend(u16::from(self.metric).to_le_bytes());
        out
    }
}

// ---------------------------------------------------------------------------------------
// Colours.

#[derive(Clone, Copy)]
enum Cmc {
    ByLayer,
    ByBlock,
    Aci(u8),
}

/// CMC (spec §2.11): a colour index before 2004; from 2004 a zero index, the colour
/// with its method in the high byte, and no names.
fn cmc(b: &mut Bits, v: DwgVersion, c: Cmc) {
    if v < R2004 {
        b.bs(match c {
            Cmc::ByLayer => 256,
            Cmc::ByBlock => 0,
            Cmc::Aci(i) => i16::from(i),
        });
        return;
    }
    b.bs(0);
    b.bl(match c {
        Cmc::ByLayer => 0xC000_0000u32,
        Cmc::ByBlock => 0xC100_0000,
        Cmc::Aci(i) => 0xC300_0000 | u32::from(i),
    } as i32);
    b.rc(0);
}

/// What writes main data, strings and colours for one release.
trait Out {
    fn v(&self) -> DwgVersion;
    fn m(&mut self) -> &mut Bits;
    /// TV: in the main stream before 2007, in the string stream from 2007.
    fn tv(&mut self, s: &str);
    fn cmc(&mut self, c: Cmc) {
        let v = self.v();
        cmc(self.m(), v, c);
    }
}

// ---------------------------------------------------------------------------------------
// Objects.

/// One object being written: its handle and EED (and, for entities, the graphics bit),
/// its data, its strings and its handles.
struct Obj {
    v: DwgVersion,
    kind: u16,
    head: Bits,
    data: Bits,
    strs: Bits,
    hs: Bits,
}

/// Common data of an entity (spec §20.4.1–2).
#[derive(Clone, Copy)]
struct Common {
    /// 0: owner handle given; 1 paper space; 2 model space.
    mode: u8,
    owner: u64,
    prev: u64,
    next: u64,
    layer: u64,
    color: DwgColor,
}

impl Out for Obj {
    fn v(&self) -> DwgVersion {
        self.v
    }
    fn m(&mut self) -> &mut Bits {
        &mut self.data
    }
    fn tv(&mut self, s: &str) {
        if self.v >= R2007 {
            self.strs.tu(s);
        } else {
            self.data.t8(&encode_ansi(s));
        }
    }
}

impl Obj {
    fn new(v: DwgVersion, kind: u16, h: u64) -> Obj {
        let mut head = Bits::default();
        head.h(0, h);
        head.bs(0); // no extended data
        Obj { v, kind, head, data: Bits::default(), strs: Bits::default(), hs: Bits::default() }
    }

    /// A non-entity object owned by `owner` (spec §20.1).
    fn object(v: DwgVersion, kind: u16, h: u64, owner: u64) -> Obj {
        let mut o = Obj::new(v, kind, h);
        o.data.bl(0); // reactors
        if v >= R2004 {
            o.data.b(true); // no extension dictionary
        }
        if v >= R2013 {
            o.data.b(false); // nothing in the data store
        }
        o.hs.h(SOFT_PTR, owner);
        if v < R2004 {
            o.hs.h(HARD_OWNER, 0); // extension dictionary
        }
        o
    }

    /// An entity (spec §20.4.1–2).
    fn entity(v: DwgVersion, kind: u16, h: u64, c: &Common) -> Obj {
        let mut o = Obj::new(v, kind, h);
        o.head.b(false); // no graphics
        let d = &mut o.data;
        d.bb(c.mode);
        d.bl(0); // reactors
        if v >= R2004 {
            d.b(true); // no extension dictionary
        }
        if v >= R2013 {
            d.b(false); // nothing in the data store
        }
        if v <= R14 {
            d.b(true); // linetype by layer
        }
        if v <= R2000 {
            d.b(false); // links stored
        }
        // An index; from 2004 (ENC, spec §2.11) a true colour is flagged 0x8000 and follows,
        // with its method byte (0xC2: RGB) on top.
        match c.color.rgb() {
            Some(rgb) if v >= R2004 => {
                d.bs((0x8000 | c.color.index() as u16) as i16);
                d.bl((0xC200_0000 | rgb) as i32);
            }
            _ => d.bs(c.color.index()),
        }
        d.bd(1.0); // linetype scale
        if v >= R2000 {
            d.bb(0); // linetype by layer
            d.bb(0); // plot style by layer
        }
        if v >= R2007 {
            d.bb(0); // material by layer
            d.rc(0); // casts and receives shadows
        }
        if v >= R2010 {
            for _ in 0..3 {
                d.b(false); // no visual styles
            }
        }
        d.bs(0); // visible
        if v >= R2000 {
            d.rc(LW_BYLAYER);
        }
        if c.mode == 0 {
            o.hs.h(SOFT_PTR, c.owner);
        }
        if v < R2004 {
            o.hs.h(HARD_OWNER, 0); // extension dictionary
        }
        if v <= R14 {
            o.hs.h(HARD_PTR, c.layer);
        }
        if v <= R2000 {
            o.hs.h(SOFT_PTR, c.prev);
            o.hs.h(SOFT_PTR, c.next);
        }
        if v >= R2000 {
            o.hs.h(HARD_PTR, c.layer);
        }
        o
    }

    /// BT: a BD before 2000, then one bit when 0.
    fn bt(&mut self, t: f64) {
        if self.v < R2000 {
            self.data.bd(t);
        } else if t == 0.0 {
            self.data.b(true);
        } else {
            self.data.b(false);
            self.data.bd(t);
        }
    }

    /// BE: 3BD before 2000, then one bit when +Z.
    fn be(&mut self, n: DVec3) {
        if self.v < R2000 {
            self.data.bd3(n);
        } else if n == DVec3::Z {
            self.data.b(true);
        } else {
            self.data.b(false);
            self.data.bd3(n);
        }
    }

    /// Name and xref fields of a table entry: from 2007 the xref flags are one BS.
    fn table_entry(&mut self, name: &str) {
        self.tv(name);
        if self.v >= R2007 {
            self.data.bs(0);
        } else {
            self.data.b(false); // 64-flag
            self.data.bs(0); // xref index + 1
            self.data.b(false); // xref dependent
        }
    }

    /// The object: size, (2010+) handle stream size, type, (2000–2007) data size in bits,
    /// handle, data, (2007+) strings, handles, CRC.
    fn frame(self) -> Vec<u8> {
        let v = self.v;
        let mut t = Bits::default();
        if v >= R2010 {
            t.ot(self.kind);
        } else {
            t.bs(self.kind as i16);
        }
        match v {
            R13 | R14 => {
                t.append(&self.head);
                let bits = t.len + 32 + self.data.len;
                t.rl(bits as u32);
                t.append(&self.data);
            }
            R2000 | R2004 => {
                let bits = t.len + 32 + self.head.len + self.data.len;
                t.rl(bits as u32);
                t.append(&self.head);
                t.append(&self.data);
            }
            R2007 => {
                let mut rest = self.head;
                rest.append(&self.data);
                rest.string_stream(&self.strs);
                let bits = t.len + 32 + rest.len;
                t.rl(bits as u32);
                t.append(&rest);
            }
            _ => {
                t.append(&self.head);
                t.append(&self.data);
                t.string_stream(&self.strs);
            }
        }
        let handle_start = t.len;
        t.append(&self.hs);
        let size = t.bytes.len();
        let mut out = modular_short(size);
        if v >= R2010 {
            modular_char_u((size * 8 - handle_start) as u64, &mut out);
        }
        out.extend_from_slice(&t.bytes);
        let crc = crc16(0xC0C1, &out);
        out.extend(crc.to_le_bytes());
        out
    }
}

/// A table control object: its entries, then `extra` handles.
fn control(v: DwgVersion, kind: u16, h: u64, entries: &[u64], extra: &[u64]) -> Vec<u8> {
    let mut o = Obj::object(v, kind, h, 0);
    o.data.bl(entries.len() as i32);
    if kind == T_DIMSTYLE_CONTROL && v >= R2000 {
        o.data.rc(0); // no extra handles
    }
    for &e in entries {
        o.hs.h(SOFT_OWNER, e);
    }
    for &e in extra {
        o.hs.h(HARD_OWNER, e);
    }
    o.frame()
}

fn dictionary(v: DwgVersion, kind: u16, h: u64, owner: u64, entries: &[(&str, u64)], default: Option<u64>) -> Vec<u8> {
    let mut o = Obj::object(v, kind, h, owner);
    o.data.bl(entries.len() as i32);
    if v == R14 {
        o.data.rc(0);
    }
    if v >= R2000 {
        o.data.bs(1); // cloning: keep existing
        o.data.rc(0); // entries soft-owned
    }
    for (name, e) in entries {
        o.tv(name);
        o.hs.h(SOFT_OWNER, *e);
    }
    if let Some(d) = default {
        o.hs.h(HARD_PTR, d);
    }
    o.frame()
}

/// The dimension variables, as AutoCAD's Standard style has them.
pub(super) struct Dims {
    pub scale: f64,
    pub asz: f64,
    pub exo: f64,
    pub dli: f64,
    pub exe: f64,
    pub txt: f64,
    pub cen: f64,
    pub gap: f64,
    pub altf: f64,
    pub dec: i16,
}

impl Dims {
    pub(super) fn new(metric: bool) -> Dims {
        if metric {
            Dims {
                scale: 1.0,
                asz: 2.5,
                exo: 0.625,
                dli: 3.75,
                exe: 1.25,
                txt: 2.5,
                cen: 2.5,
                gap: 0.625,
                altf: 0.03937007874,
                dec: 2,
            }
        } else {
            Dims {
                scale: 1.0,
                asz: 0.18,
                exo: 0.0625,
                dli: 0.38,
                exe: 0.18,
                txt: 0.18,
                cen: 0.09,
                gap: 0.09,
                altf: 25.4,
                dec: 4,
            }
        }
    }

    /// DIMSCALE … DIMTM.
    fn first(&self) -> [f64; 9] {
        [self.scale, self.asz, self.exo, self.dli, self.exe, 0.0, 0.0, 0.0, 0.0]
    }

    /// DIMTXT … DIMGAP.
    fn second(&self) -> [f64; 8] {
        [self.txt, self.cen, 0.0, self.altf, 1.0, 0.0, 1.0, self.gap]
    }

    /// The R13/R14 flags: B DIMTOL…DIMSOXD, RC DIMALTD DIMZIN, B DIMSD1 DIMSD2, RC DIMTOLJ
    /// DIMJUST DIMFIT, B DIMUPT, RC DIMTZIN DIMALTZ DIMALTTZ DIMTAD, BS DIMUNIT…DIMALTTD.
    fn write_r13_flags(&self, m: &mut Bits) {
        for v in [false, false, true, true, false, false, false, false, false, false, false] {
            m.b(v);
        }
        m.rc(2); // DIMALTD
        m.rc(0); // DIMZIN
        m.b(false);
        m.b(false);
        for v in [1, 0, 3] {
            m.rc(v); // DIMTOLJ DIMJUST DIMFIT
        }
        m.b(false); // DIMUPT
        for _ in 0..4 {
            m.rc(0); // DIMTZIN DIMALTZ DIMALTTZ DIMTAD
        }
        for v in [2, 0, self.dec, self.dec, 2, 2] {
            m.bs(v); // DIMUNIT DIMAUNIT DIMDEC DIMTDEC DIMALTU DIMALTTD
        }
    }

    /// The variables from DIMPOST to DIMMZS (2000+), in the order the header and
    /// DIMSTYLE share.
    fn write_r2000(&self, o: &mut impl Out) {
        let v = o.v();
        o.tv(""); // DIMPOST
        o.tv(""); // DIMAPOST
        for x in self.first() {
            o.m().bd(x);
        }
        if v >= R2007 {
            o.m().bd(1.0); // DIMFXL
            o.m().bd(std::f64::consts::FRAC_PI_4); // DIMJOGANG
            o.m().bs(0); // DIMTFILL
            o.cmc(Cmc::ByBlock); // DIMTFILLCLR
        }
        for x in [false, false, true, true, false, false] {
            o.m().b(x); // DIMTOL DIMLIM DIMTIH DIMTOH DIMSE1 DIMSE2
        }
        for x in [0, 0, 0] {
            o.m().bs(x); // DIMTAD DIMZIN DIMAZIN
        }
        if v >= R2007 {
            o.m().bs(0); // DIMARCSYM
        }
        for x in self.second() {
            o.m().bd(x);
        }
        o.m().bd(0.0); // DIMALTRND
        o.m().b(false); // DIMALT
        o.m().bs(2); // DIMALTD
        for x in [false, false, false, false] {
            o.m().b(x); // DIMTOFL DIMSAH DIMTIX DIMSOXD
        }
        for _ in 0..3 {
            o.cmc(Cmc::ByBlock); // DIMCLRD DIMCLRE DIMCLRT
        }
        for x in [0, self.dec, self.dec, 2, 2, 0, 0, 2, b'.' as i16, 0, 0] {
            o.m().bs(x); // DIMADEC DIMDEC DIMTDEC DIMALTU DIMALTTD DIMAUNIT DIMFRAC DIMLUNIT DIMDSEP DIMTMOVE DIMJUST
        }
        o.m().b(false); // DIMSD1
        o.m().b(false); // DIMSD2
        for x in [1, 0, 0, 0] {
            o.m().bs(x); // DIMTOLJ DIMTZIN DIMALTZ DIMALTTZ
        }
        o.m().b(false); // DIMUPT
        o.m().bs(3); // DIMATFIT
        if v >= R2007 {
            o.m().b(false); // DIMFXLON
        }
        if v >= R2010 {
            o.m().b(false); // DIMTXTDIRECTION
            o.m().bd(100.0); // DIMALTMZF
            o.tv(""); // DIMALTMZS
            o.m().bd(100.0); // DIMMZF
            o.tv(""); // DIMMZS
        }
    }
}

/// An entity placed in a block or model space, with the handles of what it owns.
pub(super) struct Placed<'a> {
    pub e: &'a DwgEntity,
    pub h: u64,
    /// Vertices and faces of a polyline.
    pub subs: Vec<u64>,
    pub seqend: u64,
}

/// A block: its block record (header), BLOCK and ENDBLK entities, its entities and the
/// INSERTs of it.
pub(super) struct Space<'a> {
    pub header: u64,
    pub block: u64,
    pub endblk: u64,
    pub name: String,
    pub base: DVec3,
    pub ents: Vec<Placed<'a>>,
    pub inserts: Vec<u64>,
}

/// The X and Y axes of the plane of `n` by AutoCAD's arbitrary axis algorithm.
pub(super) fn arbitrary_axes(n: DVec3) -> (DVec3, DVec3) {
    let n = n.normalize_or(DVec3::Z);
    if n == DVec3::Z {
        return (DVec3::X, DVec3::Y);
    }
    let ax = if n.x.abs() < 1.0 / 64.0 && n.y.abs() < 1.0 / 64.0 { DVec3::Y.cross(n) } else { DVec3::Z.cross(n) }
        .normalize();
    (ax, n.cross(ax).normalize())
}

/// A world point in the object coordinate system of `n`.
pub(super) fn to_ocs(p: DVec3, n: DVec3) -> DVec3 {
    let (ax, ay) = arbitrary_axes(n);
    DVec3::new(p.dot(ax), p.dot(ay), p.dot(n.normalize_or(DVec3::Z)))
}

/// Points that bound a list of entities, inserted blocks transformed.
fn bounds(ents: &[DwgEntity], blocks: &HashMap<String, &DwgBlock>, depth: u32, out: &mut Vec<DVec3>) {
    for e in ents {
        match &e.geom {
            DwgGeom::Line { a, b, .. } => out.extend([*a, *b]),
            DwgGeom::LwPolyline { pts, elevation, .. } => out.extend(pts.iter().map(|p| p.extend(*elevation))),
            DwgGeom::Polyline3d { pts, .. } => out.extend(pts),
            DwgGeom::Face3d { corners, .. } => out.extend(corners),
            DwgGeom::PolyFace { verts, .. } => out.extend(verts),
            DwgGeom::Solid { corners, elevation } => out.extend(corners.iter().map(|p| p.extend(*elevation))),
            DwgGeom::Text { pos, .. } => out.push(*pos),
            DwgGeom::Arc { center, radius, .. } | DwgGeom::Circle { center, radius, .. } => {
                out.extend([*center - DVec3::splat(*radius), *center + DVec3::splat(*radius)])
            }
            DwgGeom::Point { p } | DwgGeom::XLine { p, .. } => out.push(*p),
            DwgGeom::Insert { block, pos, scale, rotation, normal } => {
                let Some(b) = blocks.get(&block.to_uppercase()) else { continue };
                if depth > 16 {
                    continue;
                }
                let mut inner = Vec::new();
                bounds(&b.entities, blocks, depth + 1, &mut inner);
                let (ax, ay) = arbitrary_axes(*normal);
                let (c, s) = (rotation.cos(), rotation.sin());
                let (x, y) = (ax * c + ay * s, ay * c - ax * s);
                let z = normal.normalize_or(DVec3::Z);
                out.extend(inner.into_iter().map(|p| {
                    let q = (p - b.base) * *scale;
                    *pos + x * q.x + y * q.y + z * q.z
                }));
            }
        }
    }
}

/// Julian day and milliseconds of the day, now.
pub(super) fn julian_now() -> (i32, i32) {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let day = 2_440_588 + (ms / 86_400_000) as i32;
    (day, (ms % 86_400_000) as i32)
}

/// A GUID in AutoCAD's text form.
fn guid(seed: u64) -> String {
    let mut x = seed ^ 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let (a, b) = (next(), next());
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:04X}-{:012X}}}",
        a >> 32,
        (a >> 16) & 0xFFFF,
        a & 0xFFFF,
        b >> 48,
        b & 0xFFFF_FFFF_FFFF
    )
}

/// Whether release `v` writes `g` as several objects (vertices, faces, SEQEND): before
/// release 14 there are no lightweight polylines.
fn has_subs(g: &DwgGeom, v: DwgVersion) -> bool {
    matches!(g, DwgGeom::Polyline3d { .. } | DwgGeom::PolyFace { .. })
        || (v <= R13 && matches!(g, DwgGeom::LwPolyline { .. }))
}

/// What a release can write of a drawing: release 12 has no infinite lines.
pub(super) fn written(g: &DwgGeom, v: DwgVersion) -> bool {
    !(v == R12 && matches!(g, DwgGeom::XLine { .. }))
}

/// The names of one symbol table as release `v` writes them. Up to release 14 a name is
/// in capitals, letters, digits, `$`, `-` and `_` only, 31 characters at most: like the
/// ODA's libraries (SketchUp's exporter), every other character becomes `_`, and a
/// number tells apart names that end up the same.
pub(super) struct Names {
    v: DwgVersion,
    taken: std::collections::HashSet<String>,
}

impl Names {
    pub(super) fn new(v: DwgVersion, reserved: &[&str]) -> Names {
        Names { v, taken: reserved.iter().map(|s| s.to_uppercase()).collect() }
    }

    /// The name `name` is written under (`name` not given before, case aside).
    pub(super) fn add(&mut self, name: &str) -> String {
        if self.v >= R2000 {
            self.taken.insert(name.to_uppercase());
            return name.to_string();
        }
        let base: String = name
            .chars()
            .map(|c| {
                let c = c.to_ascii_uppercase();
                if c.is_ascii_alphanumeric() || matches!(c, '$' | '-' | '_') { c } else { '_' }
            })
            .take(31)
            .collect();
        let mut out = base.clone();
        let mut n = 1;
        while self.taken.contains(&out) {
            let suffix = format!("_{n}");
            out = format!("{}{suffix}", &base[..base.len().min(31 - suffix.len())]);
            n += 1;
        }
        self.taken.insert(out.clone());
        out
    }
}

/// A name every drawing has, as release `v` writes it ("Standard", "*Model_Space"…).
pub(super) fn fixed(v: DwgVersion, name: &str) -> String {
    if v <= R14 { name.to_uppercase() } else { name.to_string() }
}

/// Where everything of a drawing goes, the same in the DWG and the DXF of a release:
/// handles, names, the entities of each block.
pub(super) struct Plan<'a> {
    /// Layer "0" first; names as the release writes them.
    pub layers: Vec<(DwgLayer, u64)>,
    /// Index in `layers` of a layer name of the document, in capitals.
    layer_of: HashMap<String, usize>,
    /// The blocks of the document, then model space and paper space.
    pub spaces: Vec<Space<'a>>,
    /// Index in `spaces` of a block name of the document, in capitals.
    block_of: HashMap<String, usize>,
    /// Index of model space in `spaces`.
    pub model: usize,
    pub handseed: u64,
    pub metric: bool,
    /// Extents of model space, if it has anything.
    pub extents: Option<(DVec3, DVec3)>,
    pub limits: DVec2,
    pub text_size: f64,
}

impl Plan<'_> {
    /// The layer an entity names: its index in `layers`.
    pub fn layer(&self, name: &str) -> usize {
        let name = if name.trim().is_empty() { "0" } else { name };
        self.layer_of.get(&name.to_uppercase()).copied().unwrap_or(0)
    }

    /// The block an INSERT names.
    pub fn block(&self, name: &str) -> &Space<'_> {
        &self.spaces[self.block_of[&name.to_uppercase()]]
    }

    /// The index in `spaces` of the block an INSERT names.
    pub fn block_index(&self, name: &str) -> usize {
        self.block_of[&name.to_uppercase()]
    }

    /// Lower and upper corners of model space, AutoCAD's "nothing" when it is empty.
    pub fn ext(&self) -> (DVec3, DVec3) {
        self.extents.unwrap_or((DVec3::splat(1e20), DVec3::splat(-1e20)))
    }

    /// The view of the *Active viewport, on the extents: centre, height, aspect ratio.
    pub fn view(&self) -> (DVec2, f64, f64) {
        let (center, size) = match self.extents {
            None => (self.limits / 2.0, self.limits),
            Some((lo, hi)) => ((lo.truncate() + hi.truncate()) / 2.0, (hi - lo).truncate().max(DVec2::splat(1e-6))),
        };
        let aspect = 1.6;
        (center, (size.y.max(size.x / aspect) * 1.05).max(1e-6), aspect)
    }
}

/// Lays out `doc` for release `v`.
pub(super) fn plan(doc: &DwgDoc, v: DwgVersion) -> Result<Plan<'_>> {
    let mut next = FIRST_FREE;
    let mut alloc = || {
        let h = next;
        next += 1;
        h
    };

    // Layers: "0" first, then the listed ones and those the entities name.
    let mut layers: Vec<(DwgLayer, u64)> = Vec::new();
    let mut layer_of: HashMap<String, usize> = HashMap::new();
    let mut layer_names = Names::new(v, &["0"]);
    let zero = doc.layers.iter().find(|l| l.name == "0").cloned().unwrap_or_else(|| DwgLayer::new("0", 7));
    layer_of.insert("0".into(), 0);
    layers.push((zero, LAYER0));
    let used = doc.blocks.iter().flat_map(|b| b.entities.iter()).chain(&doc.entities).map(|e| &e.layer);
    for (name, color, visible) in
        doc.layers.iter().map(|l| (&l.name, l.color, l.visible)).chain(used.map(|n| (n, 7, true)))
    {
        let name = if name.trim().is_empty() { "0" } else { name.as_str() };
        if let std::collections::hash_map::Entry::Vacant(e) = layer_of.entry(name.to_uppercase()) {
            e.insert(layers.len());
            let h = alloc();
            layers.push((DwgLayer { name: layer_names.add(name), color: color.max(1), visible }, h));
        }
    }

    // Blocks and spaces.
    let mut spaces: Vec<Space> = Vec::new();
    let mut block_of: HashMap<String, usize> = HashMap::new();
    let mut by_name: HashMap<String, &DwgBlock> = HashMap::new();
    let (model_name, paper_name) =
        if v == R12 { ("$MODEL_SPACE", "$PAPER_SPACE") } else { ("*Model_Space", "*Paper_Space") };
    let mut block_names = Names::new(v, &[model_name, paper_name]);
    for b in &doc.blocks {
        let key = b.name.to_uppercase();
        if b.name.is_empty() || b.name.starts_with('*') || block_of.contains_key(&key) {
            bail!("nom de bloc invalide ou répété : « {} »", b.name);
        }
        block_of.insert(key.clone(), spaces.len());
        by_name.insert(key, b);
        let (header, block, endblk) = (alloc(), alloc(), alloc());
        spaces.push(Space {
            header,
            block,
            endblk,
            name: block_names.add(&b.name),
            base: b.base,
            ents: Vec::new(),
            inserts: Vec::new(),
        });
    }
    let model = spaces.len();
    for (header, block, endblk, name) in
        [(MODEL_SPACE, MS_BLOCK, MS_ENDBLK, model_name), (PAPER_SPACE, PS_BLOCK, PS_ENDBLK, paper_name)]
    {
        spaces.push(Space {
            header,
            block,
            endblk,
            name: fixed(v, name),
            base: DVec3::ZERO,
            ents: Vec::new(),
            inserts: Vec::new(),
        });
    }
    let lists: Vec<(usize, &[DwgEntity])> = doc
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (i, b.entities.as_slice()))
        .chain([(model, doc.entities.as_slice())])
        .collect();
    for (si, ents) in lists {
        for e in ents.iter().filter(|e| written(&e.geom, v)) {
            let h = alloc();
            let (subs, seqend) = if has_subs(&e.geom, v) {
                let n = match &e.geom {
                    DwgGeom::Polyline3d { pts, .. } => pts.len(),
                    DwgGeom::LwPolyline { pts, .. } => pts.len(),
                    DwgGeom::PolyFace { verts, faces } => {
                        if verts.len() > 32_767 || faces.len() > 32_767 {
                            bail!("maillage de {} sommets et {} faces : 32 767 au plus", verts.len(), faces.len());
                        }
                        verts.len() + faces.len()
                    }
                    _ => 0,
                };
                ((0..n).map(|_| alloc()).collect(), alloc())
            } else {
                (Vec::new(), 0)
            };
            if let DwgGeom::Insert { block, .. } = &e.geom {
                let Some(&bi) = block_of.get(&block.to_uppercase()) else { bail!("bloc inconnu : « {block} »") };
                if si != model && bi >= si {
                    bail!("le bloc « {} » doit précéder le bloc « {} » qui l'insère", block, spaces[si].name);
                }
                spaces[bi].inserts.push(h);
            }
            spaces[si].ents.push(Placed { e, h, subs, seqend });
        }
    }

    // Extents of model space.
    let mut pts = Vec::new();
    bounds(&doc.entities, &by_name, 0, &mut pts);
    let pts: Vec<DVec3> = pts.into_iter().filter(|p| p.is_finite()).collect();
    let extents = (!pts.is_empty()).then(|| {
        (
            pts.iter().fold(DVec3::splat(f64::MAX), |a, p| a.min(*p)),
            pts.iter().fold(DVec3::splat(f64::MIN), |a, p| a.max(*p)),
        )
    });
    let metric = doc.metric();
    Ok(Plan {
        layers,
        layer_of,
        spaces,
        block_of,
        model,
        handseed: next,
        metric,
        extents,
        limits: if metric { DVec2::new(420.0, 297.0) } else { DVec2::new(12.0, 9.0) },
        text_size: if metric { 2.5 } else { 0.2 },
    })
}

/// The layouts (2000+): handle, name, tab, block record, whether it is model space. Both
/// plot on ISO A4 with no plotter ("none_device"), model space as displayed, the layout 1:1.
pub(super) const LAYOUTS: [(u64, &str, i32, u64, bool); 2] =
    [(LAYOUT_MODEL, "Model", 0, MODEL_SPACE, true), (LAYOUT_PAPER, "Layout1", 1, PAPER_SPACE, false)];

/// The entries of the named object dictionary of release `v`, in AutoCAD's alphabetical
/// order; every one is an empty dictionary but ACAD_MLINESTYLE (the Standard style),
/// ACAD_LAYOUT (the two layouts) and ACAD_PLOTSTYLENAME (Normal, its default).
pub(super) fn named_objects(v: DwgVersion) -> Vec<(&'static str, u64)> {
    let mut named = Vec::new();
    if v >= R2004 {
        named.push(("ACAD_COLOR", COLOR_DICT));
    }
    named.push(("ACAD_GROUP", GROUP_DICT));
    if v >= R2000 {
        named.push(("ACAD_LAYOUT", LAYOUT_DICT));
    }
    if v >= R2004 {
        named.push(("ACAD_MATERIAL", MATERIAL_DICT));
    }
    named.push(("ACAD_MLINESTYLE", MLSTYLE_DICT));
    if v >= R2000 {
        named.push(("ACAD_PLOTSETTINGS", PLOTSETTINGS_DICT));
        named.push(("ACAD_PLOTSTYLENAME", PLOTSTYLE_DICT));
    }
    if v >= R2007 {
        named.push(("ACAD_VISUALSTYLE", VISUALSTYLE_DICT));
    }
    named
}

/// Codes the objects, header variables and classes of `doc` for release `v` (R13 or later).
pub(super) fn build(doc: &DwgDoc, v: DwgVersion) -> Result<Built> {
    let plan = plan(doc, v)?;
    let Plan { ref layers, ref spaces, model, handseed, metric, limits, text_size, .. } = plan;
    let (ext_min, ext_max) = plan.ext();
    let date = julian_now();

    let mut objects: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut put = |h: u64, bytes: Vec<u8>| objects.push((h, bytes));

    // Table controls.
    let user_blocks: Vec<u64> = spaces[..model].iter().map(|s| s.header).collect();
    put(BLOCK_CONTROL, control(v, T_BLOCK_CONTROL, BLOCK_CONTROL, &user_blocks, &[MODEL_SPACE, PAPER_SPACE]));
    let layer_handles: Vec<u64> = layers.iter().map(|l| l.1).collect();
    put(LAYER_CONTROL, control(v, T_LAYER_CONTROL, LAYER_CONTROL, &layer_handles, &[]));
    put(STYLE_CONTROL, control(v, T_STYLE_CONTROL, STYLE_CONTROL, &[STYLE_STANDARD], &[]));
    put(LTYPE_CONTROL, control(v, T_LTYPE_CONTROL, LTYPE_CONTROL, &[LT_CONTINUOUS], &[LT_BYLAYER, LT_BYBLOCK]));
    put(VIEW_CONTROL, control(v, T_VIEW_CONTROL, VIEW_CONTROL, &[], &[]));
    put(UCS_CONTROL, control(v, T_UCS_CONTROL, UCS_CONTROL, &[], &[]));
    put(VPORT_CONTROL, control(v, T_VPORT_CONTROL, VPORT_CONTROL, &[VPORT_ACTIVE], &[]));
    put(APPID_CONTROL, control(v, T_APPID_CONTROL, APPID_CONTROL, &[APPID_ACAD], &[]));
    put(DIMSTYLE_CONTROL, control(v, T_DIMSTYLE_CONTROL, DIMSTYLE_CONTROL, &[DIMSTYLE_STANDARD], &[]));
    if v < R2004 {
        put(VPENT_CONTROL, control(v, T_VPENT_CONTROL, VPENT_CONTROL, &[], &[]));
    }

    // Dictionaries.
    put(NAMED_DICT, dictionary(v, T_DICTIONARY, NAMED_DICT, 0, &named_objects(v), None));
    put(GROUP_DICT, dictionary(v, T_DICTIONARY, GROUP_DICT, NAMED_DICT, &[], None));
    put(MLSTYLE_DICT, dictionary(v, T_DICTIONARY, MLSTYLE_DICT, NAMED_DICT, &[("Standard", MLSTYLE_STANDARD)], None));
    if v >= R2000 {
        put(
            LAYOUT_DICT,
            dictionary(
                v,
                T_DICTIONARY,
                LAYOUT_DICT,
                NAMED_DICT,
                &[("Layout1", LAYOUT_PAPER), ("Model", LAYOUT_MODEL)],
                None,
            ),
        );
        put(PLOTSETTINGS_DICT, dictionary(v, T_DICTIONARY, PLOTSETTINGS_DICT, NAMED_DICT, &[], None));
        put(
            PLOTSTYLE_DICT,
            dictionary(
                v,
                T_DICTIONARYWDFLT,
                PLOTSTYLE_DICT,
                NAMED_DICT,
                &[("Normal", PLOTSTYLE_NORMAL)],
                Some(PLOTSTYLE_NORMAL),
            ),
        );
        put(PLOTSTYLE_NORMAL, Obj::object(v, T_PLACEHOLDER, PLOTSTYLE_NORMAL, PLOTSTYLE_DICT).frame());
    }
    if v >= R2004 {
        put(COLOR_DICT, dictionary(v, T_DICTIONARY, COLOR_DICT, NAMED_DICT, &[], None));
        put(MATERIAL_DICT, dictionary(v, T_DICTIONARY, MATERIAL_DICT, NAMED_DICT, &[], None));
    }
    if v >= R2007 {
        put(VISUALSTYLE_DICT, dictionary(v, T_DICTIONARY, VISUALSTYLE_DICT, NAMED_DICT, &[], None));
    }

    // Layers.
    for (l, h) in layers {
        let mut o = Obj::object(v, T_LAYER, *h, LAYER_CONTROL);
        o.table_entry(&l.name);
        if v <= R14 {
            o.data.b(false); // frozen
            o.data.b(!l.visible); // off
            o.data.b(false); // frozen in new viewports
            o.data.b(false); // locked
        } else {
            let off = if l.visible { 0 } else { 2 };
            o.data.bs(off | 0x10 | (i16::from(LW_DEFAULT) << 5));
        }
        o.cmc(Cmc::Aci(l.color.max(1)));
        o.hs.h(HARD_PTR, 0); // xref block
        if v >= R2000 {
            o.hs.h(HARD_PTR, PLOTSTYLE_NORMAL);
        }
        if v >= R2007 {
            o.hs.h(HARD_PTR, 0); // material
        }
        o.hs.h(HARD_PTR, LT_CONTINUOUS);
        if v >= R2013 {
            o.hs.h(HARD_PTR, 0);
        }
        put(*h, o.frame());
    }

    // Text style.
    {
        let mut o = Obj::object(v, T_STYLE, STYLE_STANDARD, STYLE_CONTROL);
        o.table_entry(&fixed(v, "Standard"));
        o.data.b(false); // shape file
        o.data.b(false); // vertical
        o.data.bd(0.0); // fixed height
        o.data.bd(1.0); // width factor
        o.data.bd(0.0); // oblique
        o.data.rc(0); // generation
        o.data.bd(text_size); // last height
        o.tv(&doc.font);
        o.tv(""); // big font
        o.hs.h(HARD_PTR, 0);
        put(STYLE_STANDARD, o.frame());
    }

    // Linetypes.
    for (h, name, desc) in
        [(LT_BYBLOCK, "ByBlock", ""), (LT_BYLAYER, "ByLayer", ""), (LT_CONTINUOUS, "Continuous", "Solid line")]
    {
        let mut o = Obj::object(v, T_LTYPE, h, LTYPE_CONTROL);
        o.table_entry(&fixed(v, name));
        o.tv(desc);
        o.data.bd(0.0); // pattern length
        o.data.rc(b'A'); // alignment
        o.data.rc(0); // dashes
        if v < R2007 {
            o.data.raw(&[0; 256]); // text area
        }
        o.hs.h(HARD_PTR, 0);
        put(h, o.frame());
    }

    // Application.
    {
        let mut o = Obj::object(v, T_APPID, APPID_ACAD, APPID_CONTROL);
        o.table_entry("ACAD");
        o.data.rc(0);
        o.hs.h(HARD_PTR, 0);
        put(APPID_ACAD, o.frame());
    }

    // Dimension style.
    let dims = Dims::new(metric);
    {
        let mut o = Obj::object(v, T_DIMSTYLE, DIMSTYLE_STANDARD, DIMSTYLE_CONTROL);
        o.table_entry(&fixed(v, "Standard"));
        if v <= R14 {
            dims.write_r13_flags(&mut o.data);
            for x in dims.first().into_iter().chain(dims.second()) {
                o.data.bd(x);
            }
            for _ in 0..5 {
                o.tv(""); // DIMPOST DIMAPOST DIMBLK DIMBLK1 DIMBLK2
            }
            for _ in 0..3 {
                o.data.bs(0); // DIMCLRD DIMCLRE DIMCLRT: by block
            }
        } else {
            dims.write_r2000(&mut o);
            o.data.bs(-2); // DIMLWD: by block
            o.data.bs(-2); // DIMLWE
        }
        o.data.b(false);
        o.hs.h(HARD_PTR, 0); // xref block
        o.hs.h(HARD_PTR, STYLE_STANDARD); // DIMTXSTY
        if v >= R2000 {
            for _ in 0..4 {
                o.hs.h(HARD_PTR, 0); // DIMLDRBLK DIMBLK DIMBLK1 DIMBLK2
            }
        }
        if v >= R2007 {
            for _ in 0..3 {
                o.hs.h(HARD_PTR, 0); // DIMLTYPE DIMLTEX1 DIMLTEX2
            }
        }
        put(DIMSTYLE_STANDARD, o.frame());
    }

    // The *Active viewport, showing the extents.
    {
        let (center, height, aspect) = plan.view();
        let mut o = Obj::object(v, T_VPORT, VPORT_ACTIVE, VPORT_CONTROL);
        o.table_entry(&fixed(v, "*Active"));
        let m = &mut o.data;
        m.bd(height);
        m.bd(aspect * height);
        m.rd2(center);
        m.bd3(DVec3::ZERO); // target
        m.bd3(DVec3::Z); // view direction
        m.bd(0.0); // twist
        m.bd(50.0); // lens length
        m.bd(0.0); // front clip
        m.bd(0.0); // back clip
        m.bits(0b0001, 4); // view mode: front clip at the eye
        if v >= R2000 {
            m.rc(0); // render mode
        }
        if v >= R2007 {
            o.data.b(true); // default lighting
            o.data.rc(1); // one distant light
            o.data.bd(0.0); // brightness
            o.data.bd(0.0); // contrast
            o.cmc(Cmc::Aci(250)); // ambient colour
        }
        let m = &mut o.data;
        m.rd2(DVec2::ZERO); // lower left
        m.rd2(DVec2::ONE); // upper right
        m.b(false); // UCSFOLLOW
        m.bs(1000); // circle zoom
        m.b(true); // fast zoom
        m.bits(0b11, 2); // UCSICON: on, at the origin
        m.b(false); // grid
        m.rd2(DVec2::splat(if metric { 10.0 } else { 0.5 }));
        m.b(false); // snap
        m.b(false); // snap style
        m.bs(0); // snap isopair
        m.bd(0.0); // snap rotation
        m.rd2(DVec2::ZERO); // snap base
        m.rd2(DVec2::splat(if metric { 10.0 } else { 0.5 }));
        if v >= R2000 {
            m.b(false);
            m.b(true); // UCS per viewport
            m.bd3(DVec3::ZERO);
            m.bd3(DVec3::X);
            m.bd3(DVec3::Y);
            m.bd(0.0); // UCS elevation
            m.bs(0); // UCS orthographic type
        }
        if v >= R2007 {
            m.bs(3); // grid flags
            m.bs(5); // major grid lines
        }
        o.hs.h(HARD_PTR, 0); // xref block
        if v >= R2007 {
            o.hs.h(SOFT_PTR, 0); // background
            o.hs.h(HARD_PTR, 0); // visual style
            o.hs.h(HARD_OWNER, 0); // sun
        }
        if v >= R2000 {
            o.hs.h(HARD_PTR, 0); // named UCS
            o.hs.h(HARD_PTR, 0); // base UCS
        }
        put(VPORT_ACTIVE, o.frame());
    }

    // Multiline style.
    {
        let mut o = Obj::object(v, T_MLINESTYLE, MLSTYLE_STANDARD, MLSTYLE_DICT);
        o.tv("Standard");
        o.tv("");
        o.data.bs(0); // flags
        o.cmc(Cmc::ByLayer); // fill colour
        o.data.bd(std::f64::consts::FRAC_PI_2);
        o.data.bd(std::f64::consts::FRAC_PI_2);
        o.data.rc(2);
        for offset in [0.5, -0.5] {
            o.data.bd(offset);
            o.cmc(Cmc::ByLayer);
            if v >= R2018 {
                o.hs.h(HARD_PTR, LT_BYLAYER);
            } else {
                o.data.bs(0x7FFF); // linetype by layer
            }
        }
        put(MLSTYLE_STANDARD, o.frame());
    }

    // Layouts.
    if v >= R2000 {
        for (h, name, tab, block, model_space) in LAYOUTS {
            let mut o = Obj::object(v, T_LAYOUT, h, LAYOUT_DICT);
            o.tv(""); // page setup
            o.tv("none_device");
            o.data.bs(if model_space { 1712 } else { 688 }); // plot flags
            for x in [7.5, 20.0, 7.5, 20.0] {
                o.data.bd(x); // margins
            }
            o.data.bd(210.0);
            o.data.bd(297.0);
            o.tv("ISO_A4_(210.00_x_297.00_MM)");
            o.data.bd2(DVec2::ZERO); // plot origin
            o.data.bs(1); // millimetres
            o.data.bs(0); // rotation
            o.data.bs(if model_space { 0 } else { 5 }); // plot type: display, layout
            o.data.bd2(DVec2::ZERO); // window
            o.data.bd2(DVec2::ZERO);
            if v < R2004 {
                o.tv(""); // plot view
            }
            o.data.bd(1.0); // paper units
            o.data.bd(1.0); // drawing units
            o.tv(""); // plot style table
            o.data.bs(if model_space { 0 } else { 16 }); // fit, 1:1
            o.data.bd(1.0); // scale factor
            o.data.bd2(DVec2::ZERO); // paper image origin
            if v >= R2004 {
                o.data.bs(0); // shade plot: as displayed
                o.data.bs(2); // normal resolution
                o.data.bs(300); // custom DPI
            }
            o.tv(name);
            let m = &mut o.data;
            m.bl(tab);
            m.bs(1); // PSLTSCALE
            m.bd3(DVec3::ZERO); // UCS origin
            m.rd2(DVec2::ZERO); // limits
            m.rd2(limits);
            m.bd3(DVec3::ZERO); // insertion base
            m.bd3(DVec3::X);
            m.bd3(DVec3::Y);
            m.bd(0.0); // elevation
            m.bs(0); // orthographic type
            if model_space {
                m.bd3(ext_min);
                m.bd3(ext_max);
            } else {
                m.bd3(DVec3::splat(1e20));
                m.bd3(DVec3::splat(-1e20));
            }
            if v >= R2004 {
                m.bl(0); // viewports
            }
            if v >= R2004 {
                o.hs.h(HARD_PTR, 0); // plot view
            }
            if v >= R2007 {
                o.hs.h(SOFT_PTR, 0); // visual style
            }
            o.hs.h(SOFT_PTR, block);
            o.hs.h(SOFT_PTR, 0); // active viewport
            o.hs.h(HARD_PTR, 0); // base UCS
            o.hs.h(HARD_PTR, 0); // named UCS
            put(h, o.frame());
        }
    }

    // Blocks, their BLOCK / ENDBLK entities and their entities.
    for (si, s) in spaces.iter().enumerate() {
        let is_space = si >= model;
        {
            let mut o = Obj::object(v, T_BLOCK_HEADER, s.header, BLOCK_CONTROL);
            o.table_entry(&s.name);
            o.data.b(false); // anonymous
            o.data.b(false); // attribute definitions
            o.data.b(false); // xref
            o.data.b(false); // overlaid
            if v >= R2000 {
                o.data.b(false); // unloaded
            }
            if v >= R2004 {
                o.data.bl(s.ents.len() as i32); // owned entities
            }
            o.data.bd3(s.base);
            o.tv(""); // xref path
            if v >= R2000 {
                for _ in &s.inserts {
                    o.data.rc(1);
                }
                o.data.rc(0);
                o.tv(""); // description
                o.data.bl(0); // preview
            }
            if v >= R2007 {
                o.data.bs(0); // insert units
                o.data.b(true); // explodable
                o.data.rc(0); // any scaling
            }
            o.hs.h(HARD_PTR, 0); // xref block
            o.hs.h(HARD_OWNER, s.block);
            if v < R2004 {
                o.hs.h(SOFT_PTR, s.ents.first().map_or(0, |p| p.h));
                o.hs.h(SOFT_PTR, s.ents.last().map_or(0, |p| p.h));
            } else {
                for p in &s.ents {
                    o.hs.h(HARD_OWNER, p.h);
                }
            }
            o.hs.h(HARD_OWNER, s.endblk);
            if v >= R2000 {
                for &i in &s.inserts {
                    o.hs.h(SOFT_PTR, i);
                }
                o.hs.h(
                    HARD_PTR,
                    match s.header {
                        MODEL_SPACE => LAYOUT_MODEL,
                        PAPER_SPACE => LAYOUT_PAPER,
                        _ => 0,
                    },
                );
            }
            put(s.header, o.frame());
        }
        let own = Common { mode: 0, owner: s.header, prev: 0, next: 0, layer: LAYER0, color: DwgColor::ByLayer };
        {
            let mut o = Obj::entity(v, T_BLOCK, s.block, &own);
            o.tv(&s.name);
            put(s.block, o.frame());
            put(s.endblk, Obj::entity(v, T_ENDBLK, s.endblk, &own).frame());
        }
        for (i, p) in s.ents.iter().enumerate() {
            let c = Common {
                mode: if si == model {
                    2
                } else if is_space {
                    1
                } else {
                    0
                },
                owner: s.header,
                prev: if i > 0 { s.ents[i - 1].h } else { 0 },
                next: s.ents.get(i + 1).map_or(0, |n| n.h),
                layer: layers[plan.layer(&p.e.layer)].1,
                color: p.e.color,
            };
            for (h, bytes) in entity(v, p, &c, &plan) {
                put(h, bytes);
            }
        }
    }
    objects.sort_by_key(|o| o.0);

    let header =
        header_section(v, &HeaderInfo { handseed, date, metric, ext_min, ext_max, limits, text_size }, &dims, doc);
    let classes = classes_section(v);
    Ok(Built { version: v, objects, header, classes, handseed, metric, date })
}

/// The objects of one entity: itself, then its vertices / faces and SEQEND.
fn entity(v: DwgVersion, p: &Placed, c: &Common, plan: &Plan) -> Vec<(u64, Vec<u8>)> {
    let mut out = Vec::new();
    // The vertices of a polyline: before 2004 its first and last, from 2004 all of them.
    let vertex_handles = |o: &mut Obj| {
        if v >= R2004 {
            for &s in &p.subs {
                o.hs.h(HARD_OWNER, s);
            }
        } else {
            o.hs.h(SOFT_PTR, p.subs.first().copied().unwrap_or(0));
            o.hs.h(SOFT_PTR, p.subs.last().copied().unwrap_or(0));
        }
        o.hs.h(HARD_OWNER, p.seqend);
    };
    let o = match &p.e.geom {
        DwgGeom::Line { a, b, thickness } => {
            let mut o = Obj::entity(v, T_LINE, p.h, c);
            if v <= R14 {
                o.data.bd3(*a);
                o.data.bd3(*b);
            } else {
                let m = &mut o.data;
                let z_zero = a.z == 0.0 && b.z == 0.0;
                m.b(z_zero);
                m.rd(a.x);
                m.dd(b.x, a.x);
                m.rd(a.y);
                m.dd(b.y, a.y);
                if !z_zero {
                    m.rd(a.z);
                    m.dd(b.z, a.z);
                }
            }
            o.bt(*thickness);
            o.be(DVec3::Z);
            o
        }
        DwgGeom::LwPolyline { pts, closed, width, elevation } if v <= R13 => {
            // Release 13 has no lightweight polylines: a 2D POLYLINE with its vertices.
            let mut o = Obj::entity(v, T_POLYLINE_2D, p.h, c);
            o.data.bs(i16::from(*closed));
            o.data.bs(0); // curve type
            o.data.bd(*width);
            o.data.bd(*width);
            o.bt(0.0);
            o.data.bd(*elevation);
            o.be(DVec3::Z);
            vertex_handles(&mut o);
            for (i, q) in pts.iter().enumerate() {
                out.push(vertex(v, p, c, i, T_VERTEX_2D, |m| {
                    m.rc(0);
                    m.bd3(q.extend(0.0));
                    if *width > 0.0 {
                        m.bd(-*width); // the same start and end width
                    } else {
                        m.bd(0.0);
                        m.bd(0.0);
                    }
                    m.bd(0.0); // bulge
                    m.bd(0.0); // tangent direction
                }));
            }
            o
        }
        DwgGeom::LwPolyline { pts, closed, width, elevation } => {
            let mut o = Obj::entity(v, T_LWPOLYLINE, p.h, c);
            let m = &mut o.data;
            let flags = (if *closed { 0x200 } else { 0 })
                | (if *width != 0.0 { 4 } else { 0 })
                | (if *elevation != 0.0 { 8 } else { 0 });
            m.bs(flags);
            if *width != 0.0 {
                m.bd(*width);
            }
            if *elevation != 0.0 {
                m.bd(*elevation);
            }
            m.bl(pts.len() as i32);
            for (i, q) in pts.iter().enumerate() {
                if i == 0 || v <= R14 {
                    m.rd2(*q);
                } else {
                    m.dd2(*q, pts[i - 1]);
                }
            }
            o
        }
        DwgGeom::Polyline3d { pts, closed } => {
            let mut o = Obj::entity(v, T_POLYLINE_3D, p.h, c);
            o.data.rc(0); // not a spline
            o.data.rc(u8::from(*closed));
            if v >= R2004 {
                o.data.bl(p.subs.len() as i32);
            }
            vertex_handles(&mut o);
            for (i, q) in pts.iter().enumerate() {
                out.push(vertex(v, p, c, i, T_VERTEX_3D, |m| {
                    m.rc(32); // 3D polyline vertex
                    m.bd3(*q);
                }));
            }
            o
        }
        DwgGeom::PolyFace { verts, faces } => {
            let mut o = Obj::entity(v, T_POLYLINE_PFACE, p.h, c);
            o.data.bs(verts.len() as i16);
            o.data.bs(faces.len() as i16);
            if v >= R2004 {
                o.data.bl(p.subs.len() as i32);
            }
            vertex_handles(&mut o);
            for (i, q) in verts.iter().enumerate() {
                out.push(vertex(v, p, c, i, T_VERTEX_PFACE, |m| {
                    m.rc(192); // polyface mesh vertex
                    m.bd3(*q);
                }));
            }
            for (i, f) in faces.iter().enumerate() {
                out.push(vertex(v, p, c, verts.len() + i, T_PFACE_FACE, |m| {
                    for k in f {
                        m.bs((*k).clamp(-32_767, 32_767) as i16);
                    }
                }));
            }
            o
        }
        DwgGeom::Face3d { corners, hidden } => {
            let mut o = Obj::entity(v, T_3DFACE, p.h, c);
            let m = &mut o.data;
            if v <= R14 {
                for q in corners {
                    m.bd3(*q);
                }
                m.bs(i16::from(*hidden & 0xF));
            } else {
                m.b(*hidden == 0);
                let z_zero = corners[0].z == 0.0;
                m.b(z_zero);
                m.rd(corners[0].x);
                m.rd(corners[0].y);
                if !z_zero {
                    m.rd(corners[0].z);
                }
                for w in corners.windows(2) {
                    for (x, d) in w[1].to_array().into_iter().zip(w[0].to_array()) {
                        m.dd(x, d);
                    }
                }
                if *hidden != 0 {
                    m.bs(i16::from(*hidden & 0xF));
                }
            }
            o
        }
        DwgGeom::Solid { corners, elevation } => {
            let mut o = Obj::entity(v, T_SOLID, p.h, c);
            o.bt(0.0);
            o.data.bd(*elevation);
            for q in corners {
                o.data.rd2(*q);
            }
            o.be(DVec3::Z);
            o
        }
        DwgGeom::Text { pos, align, height, rotation, halign, valign, text } => {
            let mut o = Obj::entity(v, T_TEXT, p.h, c);
            let aligned = *halign != 0 || *valign != 0;
            if v <= R14 {
                let m = &mut o.data;
                m.bd(pos.z);
                m.rd2(pos.truncate());
                m.rd2(if aligned { align.truncate() } else { DVec2::ZERO });
                m.bd3(DVec3::Z);
                m.bd(0.0); // thickness
                m.bd(0.0); // oblique
                m.bd(*rotation);
                m.bd(*height);
                m.bd(1.0); // width factor
                o.tv(text);
                o.data.bs(0); // generation
                o.data.bs(i16::from(*halign));
                o.data.bs(i16::from(*valign));
            } else {
                let mut flags = 0x04 | 0x10 | 0x20; // no oblique, width factor 1, normal generation
                if pos.z == 0.0 {
                    flags |= 0x01;
                }
                if !aligned {
                    flags |= 0x02;
                }
                if *rotation == 0.0 {
                    flags |= 0x08;
                }
                if *halign == 0 {
                    flags |= 0x40;
                }
                if *valign == 0 {
                    flags |= 0x80;
                }
                o.data.rc(flags);
                if pos.z != 0.0 {
                    o.data.rd(pos.z);
                }
                o.data.rd2(pos.truncate());
                if aligned {
                    o.data.dd2(align.truncate(), pos.truncate());
                }
                o.be(DVec3::Z);
                o.bt(0.0);
                if *rotation != 0.0 {
                    o.data.rd(*rotation);
                }
                o.data.rd(*height);
                o.tv(text);
                if *halign != 0 {
                    o.data.bs(i16::from(*halign));
                }
                if *valign != 0 {
                    o.data.bs(i16::from(*valign));
                }
            }
            o.hs.h(HARD_PTR, STYLE_STANDARD);
            o
        }
        DwgGeom::Arc { center, normal, radius, start, end } => {
            let mut o = Obj::entity(v, T_ARC, p.h, c);
            let n = normal.normalize_or(DVec3::Z);
            o.data.bd3(to_ocs(*center, n));
            o.data.bd(*radius);
            o.bt(0.0);
            o.be(n);
            o.data.bd(*start);
            o.data.bd(*end);
            o
        }
        DwgGeom::Circle { center, normal, radius } => {
            let mut o = Obj::entity(v, T_CIRCLE, p.h, c);
            let n = normal.normalize_or(DVec3::Z);
            o.data.bd3(to_ocs(*center, n));
            o.data.bd(*radius);
            o.bt(0.0);
            o.be(n);
            o
        }
        DwgGeom::Point { p: pt } => {
            let mut o = Obj::entity(v, T_POINT, p.h, c);
            o.data.bd3(*pt);
            o.bt(0.0);
            o.be(DVec3::Z);
            o.data.bd(0.0); // X axis angle
            o
        }
        DwgGeom::XLine { p: pt, dir } => {
            let mut o = Obj::entity(v, T_XLINE, p.h, c);
            o.data.bd3(*pt);
            o.data.bd3(dir.normalize_or(DVec3::X));
            o
        }
        DwgGeom::Insert { block, pos, scale, rotation, normal } => {
            let mut o = Obj::entity(v, T_INSERT, p.h, c);
            let n = normal.normalize_or(DVec3::Z);
            let m = &mut o.data;
            m.bd3(to_ocs(*pos, n));
            if v <= R14 {
                m.bd3(*scale);
            } else if *scale == DVec3::ONE {
                m.bb(3);
            } else if scale.x == 1.0 {
                m.bb(1);
                m.dd(scale.y, 1.0);
                m.dd(scale.z, 1.0);
            } else if scale.x == scale.y && scale.x == scale.z {
                m.bb(2);
                m.rd(scale.x);
            } else {
                m.bb(0);
                m.rd(scale.x);
                m.dd(scale.y, scale.x);
                m.dd(scale.z, scale.x);
            }
            m.bd(*rotation);
            m.bd3(n);
            m.b(false); // no attributes
            o.hs.h(HARD_PTR, plan.block(block).header);
            o
        }
    };
    let mut all = vec![(p.h, o.frame())];
    all.append(&mut out);
    if p.seqend != 0 {
        let sc = Common { mode: 0, owner: p.h, prev: 0, next: 0, ..*c };
        all.push((p.seqend, Obj::entity(v, T_SEQEND, p.seqend, &sc).frame()));
    }
    all
}

/// A vertex or face record of a polyline, chained to its siblings.
fn vertex(v: DwgVersion, p: &Placed, c: &Common, i: usize, kind: u16, data: impl FnOnce(&mut Bits)) -> (u64, Vec<u8>) {
    let h = p.subs[i];
    let vc = Common {
        mode: 0,
        owner: p.h,
        prev: if i > 0 { p.subs[i - 1] } else { 0 },
        next: p.subs.get(i + 1).copied().unwrap_or(0),
        ..*c
    };
    let mut o = Obj::entity(v, kind, h, &vc);
    data(&mut o.data);
    (h, o.frame())
}

// ---------------------------------------------------------------------------------------
// Header variables (chapter 9).

struct HeaderInfo {
    handseed: u64,
    date: (i32, i32),
    metric: bool,
    ext_min: DVec3,
    ext_max: DVec3,
    limits: DVec2,
    text_size: f64,
}

/// The header variables: main data, and from 2007 strings and handles apart.
struct Hdr {
    v: DwgVersion,
    main: Bits,
    strs: Bits,
    hs: Bits,
}

impl Out for Hdr {
    fn v(&self) -> DwgVersion {
        self.v
    }
    fn m(&mut self) -> &mut Bits {
        &mut self.main
    }
    fn tv(&mut self, s: &str) {
        if self.v >= R2007 {
            self.strs.tu(s);
        } else {
            self.main.t8(&encode_ansi(s));
        }
    }
}

impl Hdr {
    fn h(&mut self, code: u8, x: u64) {
        if self.v >= R2007 {
            self.hs.h(code, x);
        } else {
            self.main.h(code, x);
        }
    }
    fn bools(&mut self, xs: &[bool]) {
        for &x in xs {
            self.main.b(x);
        }
    }
    fn shorts(&mut self, xs: &[i16]) {
        for &x in xs {
            self.main.bs(x);
        }
    }
    fn doubles(&mut self, xs: &[f64]) {
        for &x in xs {
            self.main.bd(x);
        }
    }
}

fn header_section(v: DwgVersion, i: &HeaderInfo, dims: &Dims, doc: &DwgDoc) -> Vec<u8> {
    let mut x = Hdr { v, main: Bits::default(), strs: Bits::default(), hs: Bits::default() };
    let (day, ms) = i.date;
    if v >= R2013 {
        x.main.bll(0); // REQUIREDVERSIONS
    }
    x.doubles(&[412_148_564_080.0, 1.0, 1.0, 1.0]);
    x.tv("m");
    x.tv("");
    x.tv("");
    x.tv("");
    x.main.bl(24);
    x.main.bl(0);
    if v <= R14 {
        x.main.bs(0);
    }
    if v < R2004 {
        x.h(HARD_PTR, 0); // current viewport entity header
    }
    x.bools(&[true, true]); // DIMASO DIMSHO
    if v <= R14 {
        x.main.b(false); // DIMSAV
    }
    x.bools(&[false, false, true, true, false, true, false]); // PLINEGEN ORTHOMODE REGENMODE FILLMODE QTEXTMODE PSLTSCALE LIMCHECK
    if v <= R14 {
        x.main.b(false); // BLIPMODE
    }
    if v >= R2004 {
        x.main.b(false);
    }
    x.bools(&[true, false, false, false]); // USRTIMER SKPOLY ANGDIR SPLFRAME
    if v <= R14 {
        x.bools(&[true, false]); // ATTREQ ATTDIA
    }
    x.bools(&[false, true]); // MIRRTEXT WORLDVIEW
    if v <= R14 {
        x.main.b(false); // WIREFRAME
    }
    x.bools(&[true, false, true]); // TILEMODE PLIMCHECK VISRETAIN
    if v <= R14 {
        x.main.b(true); // DELOBJ
    }
    x.bools(&[false, false]); // DISPSILH PELLIPSE
    x.main.bs(1); // PROXYGRAPHICS
    if v <= R14 {
        x.main.bs(2); // DRAGMODE
    }
    x.shorts(&[3020, 2, 4, 0, 0]); // TREEDEPTH LUNITS LUPREC AUNITS AUPREC
    if v <= R14 {
        x.main.bs(37); // OSMODE
    }
    x.main.bs(1); // ATTMODE
    if v <= R14 {
        x.main.bs(1); // COORDS
    }
    x.main.bs(0); // PDMODE
    if v <= R14 {
        x.main.bs(1); // PICKSTYLE
    }
    if v >= R2004 {
        for _ in 0..3 {
            x.main.bl(0);
        }
    }
    // USERI1-5 SPLINESEGS SURFU SURFV SURFTYPE SURFTAB1 SURFTAB2 SPLINETYPE SHADEDGE SHADEDIF
    // UNITMODE MAXACTVP ISOLINES CMLJUST TEXTQLTY
    x.shorts(&[0, 0, 0, 0, 0, 8, 6, 6, 6, 6, 6, 6, 3, 70, 0, 64, 4, 0, 50]);
    // LTSCALE TEXTSIZE TRACEWID SKETCHINC FILLETRAD THICKNESS ANGBASE PDSIZE PLINEWID
    x.doubles(&[1.0, i.text_size, 0.05, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0]);
    x.doubles(&[0.0; 9]); // USERR1-5 CHAMFERA-D
    x.doubles(&[0.5, 1.0, 1.0]); // FACETRES CMLSCALE CELTSCALE
    x.tv("acad"); // MENUNAME
    for d in [day, ms, day, ms] {
        x.main.bl(d); // TDCREATE TDUPDATE
    }
    if v >= R2004 {
        for _ in 0..3 {
            x.main.bl(0);
        }
    }
    for _ in 0..4 {
        x.main.bl(0); // TDINDWG TDUSRTIMER
    }
    x.cmc(Cmc::ByLayer); // CECOLOR
    x.main.h(0, i.handseed); // HANDSEED, in the main stream in every release
    for h in [LAYER0, STYLE_STANDARD, LT_BYLAYER] {
        x.h(HARD_PTR, h); // CLAYER TEXTSTYLE CELTYPE
    }
    if v >= R2007 {
        x.h(HARD_PTR, 0); // CMATERIAL
    }
    x.h(HARD_PTR, DIMSTYLE_STANDARD);
    x.h(HARD_PTR, MLSTYLE_STANDARD);
    if v >= R2000 {
        x.main.bd(0.0); // PSVPSCALE
    }
    // Paper space, then model space.
    for space in 0..2 {
        if space == 0 {
            x.main.bd3(DVec3::ZERO); // PINSBASE
            x.main.bd3(DVec3::splat(1e20)); // PEXTMIN
            x.main.bd3(DVec3::splat(-1e20)); // PEXTMAX
            x.main.rd2(DVec2::ZERO); // PLIMMIN
            x.main.rd2(i.limits);
        } else {
            x.main.bd3(DVec3::ZERO); // INSBASE
            x.main.bd3(i.ext_min);
            x.main.bd3(i.ext_max);
            x.main.rd2(DVec2::ZERO); // LIMMIN
            x.main.rd2(i.limits);
        }
        x.main.bd(0.0); // (P)ELEVATION
        x.main.bd3(DVec3::ZERO); // (P)UCSORG
        x.main.bd3(DVec3::X);
        x.main.bd3(DVec3::Y);
        x.h(HARD_PTR, 0); // (P)UCSNAME
        if v >= R2000 {
            x.h(HARD_PTR, 0); // (P)UCSORTHOREF
            x.main.bs(0); // (P)UCSORTHOVIEW
            x.h(HARD_PTR, 0); // (P)UCSBASE
            for _ in 0..6 {
                x.main.bd3(DVec3::ZERO); // (P)UCSORGTOP…BACK
            }
        }
    }
    if v <= R14 {
        dims.write_r13_flags(&mut x.main);
        x.h(HARD_PTR, STYLE_STANDARD); // DIMTXSTY
        x.doubles(&dims.first());
        x.doubles(&dims.second());
        for _ in 0..5 {
            x.tv(""); // DIMPOST DIMAPOST DIMBLK DIMBLK1 DIMBLK2
        }
        for _ in 0..3 {
            x.cmc(Cmc::ByBlock); // DIMCLRD DIMCLRE DIMCLRT
        }
    } else {
        dims.write_r2000(&mut x);
        x.h(HARD_PTR, STYLE_STANDARD); // DIMTXSTY
        for _ in 0..4 {
            x.h(HARD_PTR, 0); // DIMLDRBLK DIMBLK DIMBLK1 DIMBLK2
        }
        if v >= R2007 {
            for _ in 0..3 {
                x.h(HARD_PTR, 0); // DIMLTYPE DIMLTEX1 DIMLTEX2
            }
        }
        x.main.bs(-2); // DIMLWD
        x.main.bs(-2); // DIMLWE
    }
    for h in [
        BLOCK_CONTROL,
        LAYER_CONTROL,
        STYLE_CONTROL,
        LTYPE_CONTROL,
        VIEW_CONTROL,
        UCS_CONTROL,
        VPORT_CONTROL,
        APPID_CONTROL,
        DIMSTYLE_CONTROL,
    ] {
        x.h(HARD_OWNER, h);
    }
    if v < R2004 {
        x.h(HARD_OWNER, VPENT_CONTROL);
    }
    x.h(HARD_PTR, GROUP_DICT);
    x.h(HARD_PTR, MLSTYLE_DICT);
    x.h(HARD_OWNER, NAMED_DICT);
    if v >= R2000 {
        x.main.bs(1); // TSTACKALIGN
        x.main.bs(70); // TSTACKSIZE
        x.tv(""); // HYPERLINKBASE
        x.tv(""); // STYLESHEET
        for h in [LAYOUT_DICT, PLOTSETTINGS_DICT, PLOTSTYLE_DICT] {
            x.h(HARD_PTR, h);
        }
    }
    if v >= R2004 {
        x.h(HARD_PTR, MATERIAL_DICT);
        x.h(HARD_PTR, COLOR_DICT);
    }
    if v >= R2007 {
        x.h(HARD_PTR, VISUALSTYLE_DICT);
    }
    if v >= R2013 {
        x.h(HARD_PTR, 0);
    }
    if v >= R2000 {
        // CELWEIGHT by layer, lineweights not displayed, extended names, colour-dependent
        // plot styles.
        x.main.bl(i32::from(LW_BYLAYER) | 0x200 | 0x800 | 0x2000);
        x.main.bs(doc.insunits);
        x.main.bs(0); // CEPSNTYPE: by layer
        let seed = ((day as u64) << 32) ^ (ms as u64) ^ (i.handseed << 20);
        x.tv(&guid(seed)); // FINGERPRINTGUID
        x.tv(&guid(seed.rotate_left(17) ^ 0xA5A5)); // VERSIONGUID
    }
    if v >= R2004 {
        // SORTENTS INDEXCTL HIDETEXT XCLIPFRAME DIMASSOC HALOGAP
        for b in [127, 0, 1, 0, 2, 0] {
            x.main.rc(b);
        }
        x.main.bs(257); // OBSCUREDCOLOR
        x.main.bs(257); // INTERSECTIONCOLOR
        x.main.rc(0); // OBSCUREDLTYPE
        x.main.rc(0); // INTERSECTIONDISPLAY
        x.tv(""); // PROJECTNAME
    }
    for h in [PAPER_SPACE, MODEL_SPACE, LT_BYLAYER, LT_BYBLOCK, LT_CONTINUOUS] {
        x.h(HARD_PTR, h);
    }
    if v >= R2007 {
        x.main.b(false); // CAMERADISPLAY
        x.main.bl(0);
        x.main.bl(0);
        x.main.bd(0.0);
        x.doubles(&[2.0, 6.0, 2.0, 50.0, 0.0]); // STEPSPERSEC STEPSIZE 3DDWFPREC LENSLENGTH CAMERAHEIGHT
        x.main.rc(1); // SOLIDHIST
        x.main.rc(1); // SHOWHIST
        let half_pi = std::f64::consts::FRAC_PI_2;
        let (w, hgt) = if i.metric { (5.0, 80.0) } else { (0.25, 4.0) };
        x.doubles(&[w, hgt, half_pi, half_pi, 0.0, 0.0]); // PSOLWIDTH PSOLHEIGHT LOFTANG1-2 LOFTMAG1-2
        x.main.bs(7); // LOFTPARAM
        x.main.rc(1); // LOFTNORMALS
        x.doubles(&[37.795, -122.394, 0.0]); // LATITUDE LONGITUDE NORTHDIRECTION
        x.main.bl(-8000); // TIMEZONE
        for b in [1, 1, 2, 0] {
            x.main.rc(b); // LIGHTGLYPHDISPLAY TILEMODELIGHTSYNCH DWFFRAME DGNFRAME
        }
        x.main.b(false);
        x.cmc(Cmc::Aci(1)); // INTERFERECOLOR
        for _ in 0..3 {
            x.h(HARD_PTR, 0); // INTERFEREOBJVS INTERFEREVPVS DRAGVS
        }
        x.main.rc(0); // CSHADOW
        x.main.bd(0.0); // SHADOWPLANELOCATION
    }

    // The section: from 2007 a size in bits, the strings after the variables and the
    // handles after them.
    let mut data = Bits::default();
    if v >= R2007 {
        let mut body = x.main;
        body.string_stream(&x.strs);
        data.rl((32 + body.len) as u32);
        data.append(&body);
        data.append(&x.hs);
    } else {
        data = x.main;
    }
    section(v, &HEADER_START, &data, &HEADER_END)
}

/// A header or classes section: sentinel, size, (2010+ when the maintenance release says
/// so, 2018+) a second long, data, CRC, sentinel.
fn section(v: DwgVersion, start: &[u8; 16], data: &Bits, end: &[u8; 16]) -> Vec<u8> {
    let mut body = (data.bytes.len() as u32).to_le_bytes().to_vec();
    if v.size_high() {
        body.extend(0u32.to_le_bytes());
    }
    body.extend_from_slice(&data.bytes);
    let crc = crc16(0xC0C1, &body);
    let mut out = start.to_vec();
    out.extend(body);
    out.extend(crc.to_le_bytes());
    out.extend_from_slice(end);
    out
}

// ---------------------------------------------------------------------------------------
// Classes (chapter 10).

/// The classes of the objects with no fixed type (2000+): the plot style dictionary and its
/// placeholder, the layouts. Number, DXF name, C++ name, objects of the class.
pub(super) const CLASSES: [(u16, &str, &str, i32); 3] = [
    (T_DICTIONARYWDFLT, "ACDBDICTIONARYWDFLT", "AcDbDictionaryWithDefault", 1),
    (T_PLACEHOLDER, "ACDBPLACEHOLDER", "AcDbPlaceHolder", 1),
    (T_LAYOUT, "LAYOUT", "AcDbLayout", 2),
];

fn classes_section(v: DwgVersion) -> Vec<u8> {
    struct Cls {
        v: DwgVersion,
        main: Bits,
        strs: Bits,
    }
    impl Out for Cls {
        fn v(&self) -> DwgVersion {
            self.v
        }
        fn m(&mut self) -> &mut Bits {
            &mut self.main
        }
        fn tv(&mut self, s: &str) {
            if self.v >= R2007 {
                self.strs.tu(s);
            } else {
                self.main.t8(&encode_ansi(s));
            }
        }
    }
    let list: &[(u16, &str, &str, i32)] = if v >= R2000 { &CLASSES } else { &[] };
    let mut c = Cls { v, main: Bits::default(), strs: Bits::default() };
    if v >= R2004 {
        // The highest class number, two zero bytes, true.
        c.main.bs(list.last().map_or(499, |l| l.0 as i16));
        c.main.rc(0);
        c.main.rc(0);
        c.main.b(true);
    }
    for &(num, dxf, cpp, count) in list {
        c.main.bs(num as i16);
        c.main.bs(0); // proxy flags
        c.tv("ObjectDBX Classes");
        c.tv(cpp);
        c.tv(dxf);
        c.main.b(false); // was a zombie
        c.main.bs(0x1F3); // object, not entity
        if v >= R2004 {
            c.main.bl(count); // objects of the class
            c.main.bl(0); // DWG version
            c.main.bl(0); // maintenance release
            c.main.bl(0);
            c.main.bl(0);
        }
    }
    let mut data = Bits::default();
    if v >= R2007 {
        let mut body = c.main;
        body.string_stream(&c.strs);
        data.rl((32 + body.len) as u32);
        data.append(&body);
    } else {
        data = c.main;
    }
    section(v, &CLASSES_START, &data, &CLASSES_END)
}

/// The object map (spec chapter 23): chunks of at most 2032 bytes of handle / offset
/// deltas, each with a big-endian size and CRC, then an empty chunk.
pub(super) fn object_map(map: &[(u64, u64)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chunk = |body: &[u8]| {
        let start = out.len();
        out.extend(((body.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(body);
        let crc = crc16(0xC0C1, &out[start..]);
        out.extend(crc.to_be_bytes());
    };
    let (mut body, mut last) = (Vec::new(), (0u64, 0i64));
    for &(h, o) in map {
        let mut pair = Vec::new();
        modular_char_u(h - last.0, &mut pair);
        super::bits::modular_char(o as i64 - last.1, &mut pair);
        if body.len() + pair.len() > 2030 {
            chunk(&body);
            body.clear();
            pair.clear();
            modular_char_u(h, &mut pair);
            super::bits::modular_char(o as i64, &mut pair);
        }
        body.extend(pair);
        last = (h, o as i64);
    }
    if !body.is_empty() {
        chunk(&body);
    }
    chunk(&[]);
    out
}
