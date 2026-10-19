//! Forged .skp files for the tests of the native reader: records of 2021+ files and their ZIP
//! container, MFC archives of SketchUp 3–2020 files.

#![allow(dead_code)]

use std::collections::HashMap;

use esq_skp::native;

/// A record of `model.dat`: tag, length, payload.
pub fn rec(tag: u16, payload: &[u8]) -> Vec<u8> {
    let mut v = tag.to_le_bytes().to_vec();
    v.extend((payload.len() as u32).to_le_bytes());
    v.extend(payload);
    v
}

/// An MFC `CString` (wide).
pub fn cstring(s: &str) -> Vec<u8> {
    let n = s.encode_utf16().count();
    let mut v = vec![0xff, 0xfe, 0xff];
    if n < 0xff {
        v.push(n as u8);
    } else if n < 0xfffe {
        v.push(0xff);
        v.extend((n as u16).to_le_bytes());
    } else {
        v.extend([0xff, 0xff, 0xff]);
        v.extend((n as u32).to_le_bytes());
    }
    v.extend(s.encode_utf16().flat_map(|u| u.to_le_bytes()));
    v
}

/// A .skp of SketchUp 2026 made of `model.dat` and other entries (`true` = deflated).
pub fn skp_file(model: &[u8], entries: &[(&str, Vec<u8>, bool)]) -> Vec<u8> {
    let mut out = [cstring("SketchUp Model"), cstring("{26.2.243}")].concat();
    out.extend(b"VFF\x08\x00\x01\x00\x11\x00\x00\x00\x00\x00");
    let zip_start = out.len();
    let all: Vec<(&str, &[u8], bool)> = std::iter::once(("model.dat", model, true))
        .chain(entries.iter().map(|(n, d, z)| (*n, d.as_slice(), *z)))
        .collect();
    let mut central = Vec::new();
    for (name, data, deflate) in &all {
        let packed = if *deflate { miniz_oxide::deflate::compress_to_vec(data, 9) } else { data.to_vec() };
        let crc = native::zip::crc32(data);
        let local = (out.len() - zip_start) as u32;
        let head = |sig: u32| {
            let mut h = sig.to_le_bytes().to_vec();
            if sig == 0x0201_4b50 {
                h.extend(20u16.to_le_bytes());
            }
            h.extend(20u16.to_le_bytes());
            h.extend(0u16.to_le_bytes());
            h.extend(if *deflate { 8u16 } else { 0u16 }.to_le_bytes());
            h.extend([0u8; 4]);
            h.extend(crc.to_le_bytes());
            h.extend((packed.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes());
            h
        };
        out.extend(head(0x0403_4b50));
        out.extend(name.as_bytes());
        out.extend(&packed);
        let mut c = head(0x0201_4b50);
        c.extend([0u8; 10]);
        c.extend(local.to_le_bytes());
        c.extend(name.as_bytes());
        central.extend(c);
    }
    let cd_at = (out.len() - zip_start) as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((all.len() as u16).to_le_bytes());
    out.extend((all.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(cd_at.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

/// The schemas of the forged MFC archives: few fields per object.
pub const SCHEMAS: &[(&str, u32)] = &[
    ("CEntity", 3),
    ("CDrawingElement", 2),
    ("CComponent", 11),
    ("CSketchUpModel", 10),
    ("CEdge", 0),
    ("CEdgeUse", 1),
    ("CLoop", 1),
    ("CAttributeNamed", 1),
    ("CLayerManager", 7),
    ("CLayerGroup", 3),
    ("CMaterial", 12),
    ("CTexture", 4),
    ("CDib", 2),
];

/// An MFC archive of SketchUp 2020 being written: objects get the map indices MFC gives them.
pub struct Mfc {
    pub out: Vec<u8>,
    classes: HashMap<&'static str, u32>,
    next: u32,
}

impl Mfc {
    /// The header and the version map ([`SCHEMAS`]; classes it does not list have schema 0).
    pub fn new() -> Mfc {
        let mut out = [cstring("SketchUp Model"), cstring("{20.0.1}")].concat();
        out.extend([7u8; 16]);
        out.extend(cstring(""));
        out.extend(0u32.to_le_bytes());
        let mut m = Mfc { out, classes: HashMap::new(), next: 1 };
        m.new_obj("CVersionMap");
        for (class, v) in SCHEMAS {
            m.str(class);
            m.u32(*v);
        }
        m.str("End-Of-Version-Map");
        m.u32(0);
        m
    }

    pub fn bytes(&mut self, b: &[u8]) {
        self.out.extend(b);
    }
    pub fn u8(&mut self, v: u8) {
        self.out.push(v);
    }
    pub fn u32(&mut self, v: u32) {
        self.out.extend(v.to_le_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.out.extend(v.to_le_bytes());
    }
    pub fn str(&mut self, s: &str) {
        self.out.extend(cstring(s));
    }
    pub fn null(&mut self) {
        self.out.extend([0, 0]);
    }

    /// The tag of a new object of `class`; returns its map index.
    pub fn new_obj(&mut self, class: &'static str) -> u32 {
        match self.classes.get(class) {
            Some(&c) if c < 0x7fff => self.out.extend((0x8000u16 | c as u16).to_le_bytes()),
            // A class past the short tags: the big tag, then the index with the class bit.
            Some(&c) => {
                self.out.extend(0x7fffu16.to_le_bytes());
                self.out.extend((0x8000_0000u32 | c).to_le_bytes());
            }
            None => {
                self.out.extend([0xff, 0xff, 0, 0]);
                self.out.extend((class.len() as u16).to_le_bytes());
                self.out.extend(class.as_bytes());
                self.classes.insert(class, self.next);
                self.next += 1;
            }
        }
        self.next += 1;
        self.next - 1
    }

    /// A reference to an object already written.
    pub fn old(&mut self, idx: u32) {
        if idx < 0x7fff {
            self.out.extend((idx as u16).to_le_bytes());
        } else {
            self.out.extend(0x7fffu16.to_le_bytes());
            self.out.extend(idx.to_le_bytes());
        }
    }

    /// `CEntity` (schema 3): no dictionaries.
    pub fn entity(&mut self) {
        self.null();
    }

    /// `CDrawingElement` (schema 2): entity, two flags.
    pub fn drawing_element(&mut self) {
        self.entity();
        self.bytes(&[0, 0]);
    }

    /// The model (`CSketchUpModel` schema 10) whose root holds the items `items` writes
    /// (returning how many), with the folders of tags `folders` writes in the root folder
    /// (returning how many).
    pub fn model(
        mut self,
        materials: impl FnOnce(&mut Mfc) -> u32,
        folders: impl FnOnce(&mut Mfc) -> u32,
        items: impl FnOnce(&mut Mfc) -> u32,
    ) -> Vec<u8> {
        self.bytes(&[0; 8]);
        self.null();
        self.u8(0);
        // Behaviour, description.
        self.entity();
        self.u8(0);
        self.str("");
        // Camera in place: eye, direction, up, two numbers, perspective, field of view, two
        // numbers, a point.
        self.bytes(&[0; 72]);
        self.bytes(&[0; 16]);
        self.u8(1);
        self.f64(35.0);
        self.bytes(&[0; 8 + 24]);
        // Rendering options (schema 0).
        self.entity();
        self.u32(2);
        self.bytes(&[0; 3]);
        self.bytes(&[0; 12]);
        self.u32(0);
        // The root component: managers, items, relations, active section.
        self.drawing_element();
        let count = self.out.len();
        self.u32(0);
        let n = materials(&mut self);
        self.out[count..count + 4].copy_from_slice(&n.to_le_bytes());
        self.entity();
        self.u32(0);
        self.entity();
        self.str("");
        let count = self.out.len();
        self.u32(0);
        let n = folders(&mut self);
        self.out[count..count + 4].copy_from_slice(&n.to_le_bytes());
        self.u32(0);
        self.bytes(&[0, 0]);
        self.u32(0);
        let count = self.out.len();
        self.u32(0);
        let n = items(&mut self);
        self.out[count..count + 4].copy_from_slice(&n.to_le_bytes());
        self.u32(0);
        self.null();
        // Shadows, pages, axes, 16 numbers, options, dimension and text styles, fonts.
        self.entity();
        self.u32(0);
        self.u8(0);
        self.str("");
        self.str("");
        self.u32(0);
        self.drawing_element();
        self.bytes(&[0; 96]);
        self.bytes(&[0; 64]);
        self.u32(3);
        self.u32(0);
        self.entity();
        self.bytes(&[0; 4 + 1 + 1 + 4 + 4 + 4 + 1 + 4 + 1 + 1 + 8 + 1 + 8]);
        self.entity();
        self.null();
        self.bytes(&[0; 4 + 4 + 1 + 4 + 1]);
        self.entity();
        self.u32(0);
        self.out
    }
}
