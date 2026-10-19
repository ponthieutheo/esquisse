//! The MFC archive of SketchUp 3 to 2020 files (`docs/skp-format.md`, §3): objects written by
//! `CArchive::WriteObject` (class tags, object indices) and the `Serialize` of each class, whose
//! fields depend on the class's schema version (`CVersionMap`).
//!
//! Every read is checked (no panic, no allocation from an unchecked count); nesting of objects
//! is bounded by [`MAX_DEPTH`].

use std::collections::HashMap;

use super::{Error, Result};

/// Deepest nesting of objects read inline (an object's first reference reads it in place): a
/// component inside a component reads its definition there. Far beyond real models; the
/// reading runs on a thread with a large stack (`super::on_big_stack`).
pub const MAX_DEPTH: usize = 20_000;
/// Most entries of the object map (MFC's own limit is 0x3FFFFFFE).
const MAX_MAP: usize = 50_000_000;

/// A class of the archive: name and the schema written with it.
#[derive(Clone, Debug)]
pub struct Class {
    pub name: String,
    pub schema: u16,
}

/// Most classes declared in one archive.
const MAX_CLASSES: usize = 10_000;

/// An object mapped but not read yet.
const PENDING: u32 = u32::MAX;

thread_local! {
    /// Offsets of the counts and string lengths read on this thread, when recorded
    /// ([`record_counts`]): where a damaged or forged archive can claim more than it holds.
    static COUNTS: std::cell::RefCell<Option<Vec<usize>>> = const { std::cell::RefCell::new(None) };
}

/// Starts (`true`) or stops recording the offsets of the counts and string lengths this thread
/// reads; returns what was recorded (robustness tests, format study).
pub fn record_counts(on: bool) -> Vec<usize> {
    COUNTS.with(|c| std::mem::replace(&mut *c.borrow_mut(), on.then(Vec::new)).unwrap_or_default())
}

fn note_count(at: usize) {
    COUNTS.with(|c| {
        if let Some(v) = c.borrow_mut().as_mut() {
            v.push(at);
        }
    });
}

/// What an index of the archive's map holds (8 bytes: the map has an entry per 2 bytes of the
/// file at most).
#[derive(Clone, Copy, Debug)]
enum Slot {
    Null,
    Class(u32),
    Object(u32),
}

/// The reading position in an archive and its maps.
pub struct Ar<'a> {
    d: &'a [u8],
    pub pos: usize,
    map: Vec<Slot>,
    pub classes: Vec<Class>,
    /// Schema of each class, from the file's `CVersionMap` (0 for a class it does not list).
    pub versions: HashMap<String, u32>,
    pub depth: usize,
}

impl<'a> Ar<'a> {
    pub fn new(d: &'a [u8]) -> Ar<'a> {
        Ar { d, pos: 0, map: vec![Slot::Null], classes: Vec::new(), versions: HashMap::new(), depth: 0 }
    }

    pub fn err(&self, what: &str) -> Error {
        Error(format!("{what} (octet {})", self.pos))
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or_else(|| self.err("longueur"))?;
        let b = self.d.get(self.pos..end).ok_or_else(|| self.err("archive tronquée"))?;
        self.pos = end;
        Ok(b)
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    pub fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }

    pub fn u16(&mut self) -> Result<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self) -> Result<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }

    pub fn u64(&mut self) -> Result<u64> {
        let b = self.bytes(8)?;
        Ok(u64::from_le_bytes(b.try_into().map_err(|_| self.err("u64"))?))
    }

    pub fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.u64()?))
    }

    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    pub fn v3(&mut self) -> Result<[f64; 3]> {
        Ok([self.f64()?, self.f64()?, self.f64()?])
    }

    pub fn v4(&mut self) -> Result<[f64; 4]> {
        Ok([self.f64()?, self.f64()?, self.f64()?, self.f64()?])
    }

    /// `n` doubles.
    pub fn f64s<const N: usize>(&mut self) -> Result<[f64; N]> {
        let mut out = [0.0; N];
        for o in out.iter_mut() {
            *o = self.f64()?;
        }
        Ok(out)
    }

    /// A transformation as SketchUp stores it: the 3×3 part by rows, the translation, the scale
    /// (13 doubles). Returned column-major 4×4, as `SUTransformation` (see [`xform_of`]).
    pub fn xform(&mut self) -> Result<[f64; 16]> {
        Ok(xform_of(&self.f64s::<13>()?))
    }

    /// A count that must fit in what is left of the archive (`unit` bytes per item at least),
    /// read as a signed number: negative counts are empty.
    pub fn icount(&mut self, unit: usize) -> Result<usize> {
        note_count(self.pos);
        let n = self.i32()?;
        if n <= 0 {
            return Ok(0);
        }
        let n = n as usize;
        if n.saturating_mul(unit.max(1)) > self.left() {
            return Err(self.err("nombre d'éléments démesuré"));
        }
        Ok(n)
    }

    /// A count that must fit in what is left of the archive (`unit` bytes per item at least).
    pub fn count(&mut self, unit: usize) -> Result<usize> {
        note_count(self.pos);
        let n = self.u32()? as usize;
        if n.saturating_mul(unit.max(1)) > self.d.len().saturating_sub(self.pos) {
            return Err(self.err("nombre d'éléments démesuré"));
        }
        Ok(n)
    }

    /// Colour: 4 bytes R, G, B, A.
    pub fn color(&mut self) -> Result<[u8; 4]> {
        let b = self.bytes(4)?;
        Ok([b[0], b[1], b[2], b[3]])
    }

    /// A GUID (Data1 u32, Data2 u16, Data3 u16, 8 bytes) as text.
    pub fn guid(&mut self) -> Result<String> {
        let b = self.bytes(16)?;
        let a = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let c = u16::from_le_bytes([b[4], b[5]]);
        let e = u16::from_le_bytes([b[6], b[7]]);
        Ok(format!(
            "{a:08x}-{c:04x}-{e:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
        ))
    }

    /// An MFC `CString` (see `container::mfc_string`).
    pub fn string(&mut self) -> Result<String> {
        note_count(self.pos);
        let (s, n) = super::container::mfc_string(&self.d[self.pos.min(self.d.len())..])?;
        self.pos += n;
        Ok(s)
    }

    /// A persistent id written compactly: a mask byte, then the non-zero bytes it names.
    pub fn compact_id(&mut self) -> Result<u64> {
        let mask = self.u8()?;
        let mut v = 0u64;
        for i in 0..8 {
            if mask & (1 << i) != 0 {
                v |= (self.u8()? as u64) << (8 * i);
            }
        }
        Ok(v)
    }

    /// The class of the next object, or what the tag refers to (`CArchive::ReadClass`).
    /// Returns `Ok(Err(index))` for a reference to an object already read, `Ok(Ok(None))` for
    /// null, `Ok(Ok(Some(class)))` for a new object of that class.
    fn read_tag(&mut self) -> Result<std::result::Result<Option<usize>, usize>> {
        let w = self.u16()?;
        if w == 0 {
            return Ok(Ok(None));
        }
        let tag: u32 = if w == 0x7fff { self.u32()? } else { ((w as u32 & 0x8000) << 16) | (w as u32 & 0x7fff) };
        if w == 0xffff {
            // A new class: schema, name length, name.
            let schema = self.u16()?;
            let len = self.u16()? as usize;
            if len >= 0x40 {
                return Err(self.err("nom de classe trop long"));
            }
            let name = String::from_utf8_lossy(self.bytes(len)?).into_owned();
            // A file declares each of its classes once (SketchUp has about a hundred).
            if self.classes.len() >= MAX_CLASSES {
                return Err(self.err("trop de classes"));
            }
            self.classes.push(Class { name, schema });
            self.push(Slot::Class((self.classes.len() - 1) as u32))?;
            return Ok(Ok(Some(self.classes.len() - 1)));
        }
        let idx = (tag & 0x7fff_ffff) as usize;
        match self.map.get(idx) {
            Some(Slot::Class(c)) if tag & 0x8000_0000 != 0 => Ok(Ok(Some(*c as usize))),
            Some(Slot::Object(_)) if tag & 0x8000_0000 == 0 => Ok(Err(idx)),
            _ => Err(self.err("référence d'objet invalide")),
        }
    }

    fn push(&mut self, s: Slot) -> Result<usize> {
        if self.map.len() >= MAX_MAP {
            return Err(self.err("trop d'objets"));
        }
        self.map.push(s);
        Ok(self.map.len() - 1)
    }

    /// Reserves the map index of a new object (before its fields, as MFC does: the object can
    /// be referred to while it is read).
    pub fn reserve(&mut self) -> Result<usize> {
        self.push(Slot::Object(PENDING))
    }

    /// Records the object (an index below `MAX_MAP`) read for map index `index`.
    pub fn set_object(&mut self, index: usize, obj: usize) {
        if let Some(s) = self.map.get_mut(index)
            && let Ok(o) = u32::try_from(obj)
            && o != PENDING
        {
            *s = Slot::Object(o);
        }
    }

    /// The object stored at map index `index`, if read.
    pub fn object_at(&self, index: usize) -> Option<usize> {
        match self.map.get(index) {
            Some(Slot::Object(o)) if *o != PENDING => Some(*o as usize),
            _ => None,
        }
    }

    /// `CArchive::MapObject`: an object written in place (not through `WriteObject`) still
    /// takes an index of the map.
    pub fn map_object(&mut self) -> Result<usize> {
        self.push(Slot::Object(PENDING))
    }

    /// Reads an object reference: `Next::Null`, `Next::Old(index)` or `Next::New(class, index)`
    /// (the caller then reads the object's fields).
    pub fn read_ref(&mut self) -> Result<Next> {
        match self.read_tag()? {
            Ok(None) => Ok(Next::Null),
            Err(idx) => Ok(Next::Old(idx)),
            Ok(Some(c)) => {
                let idx = self.reserve()?;
                Ok(Next::New(self.classes[c].name.clone(), idx))
            }
        }
    }

    /// Schema version of a class, as the file's version map gives it.
    pub fn version(&self, class: &str) -> u32 {
        self.versions.get(class).copied().unwrap_or(0)
    }

    pub fn left(&self) -> usize {
        self.d.len().saturating_sub(self.pos)
    }
}

/// A transformation as both formats store it (13 doubles: the 3×3 part by rows, the
/// translation, a scale that multiplies the transformed point), column-major 4×4 as
/// `SUTransformation`, whose last element divides: it is the inverse of the scale. Settled as
/// SketchUp settles it when it reads it ([`settle_xform`]).
pub fn xform_of(t: &[f64; 13]) -> [f64; 16] {
    let mut t = *t;
    settle_xform(&mut t);
    let w = if t[12] != 0.0 && t[12].is_finite() { 1.0 / t[12] } else { t[12] };
    [t[0], t[3], t[6], 0.0, t[1], t[4], t[7], 0.0, t[2], t[5], t[8], 0.0, t[9], t[10], t[11], w]
}

/// What SketchUp does to a transformation it reads: nearly
/// the identity (3×3 part, translation and scale within 0.001) becomes the
/// identity, the scale kept; nearly a rotation (rows of unit length and perpendicular within
/// 0.001) is made exactly one: the identity if its 3×3 part is within 1e-10
/// of it, otherwise its axes (columns) made unit, then y = unit(z × x), x = unit(y × z), or for a
/// mirror x = unit(z × y), y = unit(x × z). The translation and scale are kept.
pub fn settle_xform(t: &mut [f64; 13]) {
    const TOL: f64 = 1e-3;
    let near_identity = |t: &[f64; 13], tol: f64| {
        (0..3).all(|i| (0..3).all(|j| (t[3 * i + j] - if i == j { 1.0 } else { 0.0 }).abs() < tol))
    };
    const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    if near_identity(t, TOL) && t[9..12].iter().all(|x| x.abs() < TOL) && (t[12] - 1.0).abs() < TOL {
        t[..9].copy_from_slice(&IDENTITY);
        t[9..12].fill(0.0);
        return;
    }
    let row = |i: usize| [t[3 * i], t[3 * i + 1], t[3 * i + 2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[1] * b[1] + a[0] * b[0] + a[2] * b[2];
    let (r0, r1, r2) = (row(0), row(1), row(2));
    let rotation = (dot(r0, r0) - 1.0).abs() < TOL
        && (dot(r1, r1) - 1.0).abs() < TOL
        && (dot(r2, r2) - 1.0).abs() < TOL
        && dot(r1, r0).abs() < TOL
        && dot(r2, r0).abs() < TOL
        && dot(r2, r1).abs() < TOL;
    if !rotation {
        return;
    }
    if near_identity(t, 1e-10) {
        t[..9].copy_from_slice(&IDENTITY);
        return;
    }
    let unit = |v: [f64; 3]| {
        let l = (v[1] * v[1] + v[0] * v[0] + v[2] * v[2]).sqrt();
        if l != 0.0 {
            let k = 1.0 / l;
            [v[0] * k, v[1] * k, v[2] * k]
        } else {
            v
        }
    };
    let cross =
        |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - b[0] * a[1]];
    let col = |j: usize| unit([t[j], t[3 + j], t[6 + j]]);
    let (mut x, mut y, z) = (col(0), col(1), col(2));
    let c = cross(x, y);
    if z[1] * c[1] + z[0] * c[0] + z[2] * c[2] >= 0.0 {
        y = unit(cross(z, x));
        x = unit(cross(y, z));
    } else {
        x = unit(cross(z, y));
        y = unit(cross(x, z));
    }
    for i in 0..3 {
        t[3 * i] = x[i];
        t[3 * i + 1] = y[i];
        t[3 * i + 2] = z[i];
    }
}

/// What the next object reference is.
pub enum Next {
    Null,
    /// An object already read (map index).
    Old(usize),
    /// A new object of this class, at this map index.
    New(String, usize),
}
