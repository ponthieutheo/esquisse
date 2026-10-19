//! An MFC archive being written (`CArchive::WriteObject`, `docs/skp-format.md`, §3.1): a new
//! class is declared at its first object (`FF FF`, schema, name), takes an index of the map,
//! and so does each new object, before its fields; an object written once is referred to by
//! its index afterwards.

use std::collections::HashMap;

use super::versions::Map;

/// What an object of the archive is, to find it again: its class and its persistent id (or a
/// number of the writer for objects without one).
pub type Key = (&'static str, u64);

pub struct Ar {
    pub out: Vec<u8>,
    /// Next index of the map (the null tag is 0).
    next: u32,
    classes: HashMap<&'static str, u32>,
    objects: HashMap<Key, u32>,
    pub map: Map,
    /// Labelled offsets (format study: `ESQ_SKP_WTRACE`), and the ranges whose bytes differ
    /// from one writing to the next (GUIDs, times).
    pub trace: Option<Vec<(usize, String)>>,
    pub variable: Vec<(usize, usize)>,
    in_place: std::collections::BTreeSet<&'static str>,
}

const BIG: u32 = 0x7FFF;

impl Ar {
    pub fn new(map: Map) -> Ar {
        Ar {
            out: Vec::new(),
            next: 1,
            classes: HashMap::new(),
            objects: HashMap::new(),
            map,
            trace: std::env::var_os("ESQ_SKP_WTRACE").map(|_| Vec::new()),
            variable: Vec::new(),
            in_place: Default::default(),
        }
    }

    pub fn mark(&mut self, what: impl FnOnce() -> String) {
        if let Some(t) = self.trace.as_mut() {
            t.push((self.out.len(), what()));
        }
    }

    /// The schema of a class in the version written.
    pub fn schema(&self, class: &str) -> u32 {
        super::versions::schema(self.map, class)
    }

    pub fn u8(&mut self, v: u8) {
        self.out.push(v);
    }

    pub fn bool(&mut self, v: bool) {
        self.out.push(v as u8);
    }

    pub fn u16(&mut self, v: u16) {
        self.out.extend(v.to_le_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.out.extend(v.to_le_bytes());
    }

    pub fn i32(&mut self, v: i32) {
        self.out.extend(v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.out.extend(v.to_le_bytes());
    }

    pub fn f32(&mut self, v: f32) {
        self.out.extend(v.to_le_bytes());
    }

    pub fn f64(&mut self, v: f64) {
        self.out.extend(v.to_le_bytes());
    }

    pub fn f64s(&mut self, v: &[f64]) {
        for x in v {
            self.f64(*x);
        }
    }

    pub fn color(&mut self, c: [u8; 4]) {
        self.out.extend(c);
    }

    /// A `CString` (Unicode).
    pub fn str(&mut self, s: &str) {
        super::vff::cstring(&mut self.out, s);
    }

    pub fn guid(&mut self, g: &[u8; 16]) {
        self.variable.push((self.out.len(), 16));
        self.out.extend(g);
    }

    /// A double SketchUp works out from others, which may differ from it in its last bits
    /// (format study only: the comparison skips it).
    pub fn f64_derived(&mut self, v: f64) {
        self.variable.push((self.out.len(), 8));
        self.f64(v);
    }

    /// Doubles worked out from others (see [`Ar::f64_derived`]).
    pub fn f64s_derived(&mut self, v: &[f64]) {
        self.variable.push((self.out.len(), 8 * v.len()));
        self.f64s(v);
    }

    /// A time (seconds since 1970) that differs from one writing to the next.
    pub fn time(&mut self, t: u32) {
        self.variable.push((self.out.len(), 4));
        self.u32(t);
    }

    /// A persistent id written compactly: a mask of its non-zero bytes, then those bytes.
    pub fn compact_id(&mut self, id: u64) {
        // Ids are given in an order of SketchUp's own: the comparison skips them.
        self.variable.push((self.out.len(), 1 + (id.to_le_bytes().iter().filter(|b| **b != 0).count())));
        let b = id.to_le_bytes();
        let mut mask = 0u8;
        for (i, x) in b.iter().enumerate() {
            if *x != 0 {
                mask |= 1 << i;
            }
        }
        self.out.push(mask);
        for x in b {
            if x != 0 {
                self.out.push(x);
            }
        }
    }

    pub fn null(&mut self) {
        self.u16(0);
    }

    fn index(&mut self, i: u32) {
        if i < BIG {
            self.u16(i as u16);
        } else {
            self.u16(BIG as u16);
            self.u32(i);
        }
    }

    /// The classes whose fields were written so far: those of the objects declared, and those
    /// written in place or as the base of another (`Ar::wrote`).
    pub fn declared(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = self.classes.keys().copied().chain(self.in_place.iter().copied()).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Notes that the fields of `class` are being written, in place or as the base part of an
    /// object (coverage count only; a class the version does not know is written as a part of
    /// another, SketchUp 3's scene list for instance).
    pub fn wrote(&mut self, class: &'static str) {
        if super::versions::has(self.map, class) {
            self.in_place.insert(class);
        }
    }

    /// Whether an object is already in the archive.
    pub fn written(&self, key: Key) -> bool {
        self.objects.contains_key(&key)
    }

    /// Refers to an object: its index if written, else writes it (`f` writes its fields).
    pub fn obj(&mut self, key: Key, f: impl FnOnce(&mut Ar)) {
        if let Some(&i) = self.objects.get(&key) {
            self.index(i);
            return;
        }
        let class = key.0;
        match self.classes.get(class) {
            Some(&c) => {
                if c < BIG {
                    self.u16(0x8000 | c as u16);
                } else {
                    self.u16(BIG as u16);
                    self.u32(0x8000_0000 | c);
                }
            }
            None => {
                self.u16(0xFFFF);
                // The schema of the class as SketchUp 2026 declares it (its own, whatever the
                // version written: readers take the version map's).
                let schema = super::versions::declared(class);
                self.u16(schema as u16);
                self.u16(class.len() as u16);
                self.out.extend(class.as_bytes());
                self.classes.insert(class, self.next);
                self.next += 1;
            }
        }
        self.objects.insert(key, self.next);
        self.next += 1;
        self.mark(|| class.to_string());
        f(self);
    }
}
