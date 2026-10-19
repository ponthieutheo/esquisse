//! The native .skp reader on its own (no SketchUp needed): the samples of `tests/data` are read
//! as expected, and truncated, corrupted or forged files are refused cleanly — no panic, no
//! runaway allocation or recursion.

use std::path::Path;

use esq_skp::native::{self, container, tlv, vff};

mod common;
use common::{rec, skp_file};

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data").join(name)).unwrap()
}

/// A small deterministic generator (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

#[test]
fn the_2021_sample_is_read() {
    let (m, view) = native::read_bytes(&sample("kitchen-2021.skp")).unwrap();
    assert_eq!(m.meta.name, "Cuisine");
    assert!(view.is_some());
    let names: Vec<&str> = m.scenes.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Face", "Stylée"]);
    assert!(m.find_material("Brique").is_some_and(|b| m.materials[b].ext.texture.is_some()));
    let pied = m.defs.values().find(|d| d.name == "Pied").expect("component");
    assert_eq!(pied.description, "Pied de table");
    assert_eq!(pied.entities.instances.len(), 1, "the group inside the component");
    assert!(m.root.faces.values().filter(|f| f.tex.is_some()).count() >= 3, "texture placements");
    assert_eq!(m.root.sections.len(), 1);
    assert!(m.root.active_section.is_some());
    assert_eq!(m.root.annots.len(), 3);
    assert_eq!(m.root.guides.len(), 3);
    assert!(m.layers.iter().any(|l| l.name == "Murs" && l.folder.is_some()));
    assert!(m.defs.values().any(|d| d.ext.image.is_some()), "the image");
    assert!((m.shadow.latitude - 48.8566).abs() < 1e-9);
}

/// The same kitchen saved by SketchUp 2020, 8 and 3 (MFC archives).
#[test]
fn the_legacy_samples_are_read() {
    for (name, version) in [("kitchen-2020.skp", 20), ("kitchen-8.skp", 8), ("kitchen-3.skp", 3)] {
        let data = sample(name);
        let (m, view) = native::read_bytes(&data).unwrap_or_else(|e| panic!("{name} : {e}"));
        assert!(view.is_some(), "{name}");
        let names: Vec<&str> = m.scenes.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Face", "Stylée"], "{name}");
        assert!(m.find_material("Brique").is_some_and(|b| m.materials[b].ext.texture.is_some()), "{name}");
        let pied = m.defs.values().find(|d| d.name == "Pied").expect("component");
        assert_eq!(pied.description, "Pied de table", "{name}");
        // Texture placements exist from SketchUp 4 (`CFaceTextureCoords`).
        if version >= 8 {
            assert!(m.root.faces.values().filter(|f| f.tex.is_some()).count() >= 3, "{name} : placements");
        }
        assert_eq!(m.root.sections.len(), 1, "{name}");
        assert_eq!(m.root.guides.len(), 3, "{name}");
        assert!(m.layers.iter().any(|l| l.name == "Murs"), "{name}");
        assert!(m.defs.values().any(|d| d.ext.image.is_some()), "{name} : image");
        assert!((m.shadow.latitude - 48.8566).abs() < 1e-9, "{name}");
        // Model dictionaries (name, folders of tags) exist from SketchUp 7 and 2020.
        if version >= 8 {
            assert_eq!(m.meta.name, "Cuisine", "{name}");
        }
        if version >= 20 {
            assert!(m.layers.iter().any(|l| l.name == "Murs" && l.folder.is_some()), "{name}");
        }
        // The archive is read to its last byte.
        let f = container::open(&data).unwrap();
        let container::Body::Legacy { guid, rest } = &f.body else { panic!("{name} : archive MFC attendue") };
        let (_, left) = native::legacy::read_counted(rest, guid, f.version, &native::Options::default()).unwrap();
        assert_eq!(left, 0, "{name}");
    }
}

/// The objects of an MFC archive corrupted after the header: bytes changed, counts set to large
/// values, slices cut out. Refused or read, never a panic.
#[test]
fn corrupted_legacy_archives_do_not_panic() {
    for name in ["kitchen-2020.skp", "kitchen-3.skp"] {
        let data = sample(name);
        let f = container::open(&data).unwrap();
        let container::Body::Legacy { guid, rest } = &f.body else { panic!("archive MFC attendue") };
        let opts = native::Options::default();
        let mut rng = Rng(7);
        for round in 0..2000 {
            let mut d = rest.to_vec();
            match round % 3 {
                0 => {
                    for _ in 0..1 + rng.below(16) {
                        let i = rng.below(d.len());
                        d[i] = rng.next() as u8;
                    }
                }
                1 => {
                    let i = rng.below(d.len().saturating_sub(4));
                    let v = [0xffff_ffffu32, 0x7fff_ffff, 0x8000_0001, 0xffff, rng.next() as u32][rng.below(5)];
                    d[i..i + 4].copy_from_slice(&v.to_le_bytes());
                }
                _ => {
                    let a = rng.below(d.len());
                    let b = (a + rng.below(64)).min(d.len());
                    d.drain(a..b);
                }
            }
            let _ = native::legacy::read(&d, guid, f.version, &opts).map(|doc| native::build::model(&doc));
        }
    }
}

/// An MFC archive whose objects nest 100 000 deep (each attribute container holding the next
/// one in its entity part): refused at the nesting limit, on a caller thread of 1 MB of stack,
/// without a crash.
#[test]
fn deep_legacy_nesting_is_bounded() {
    let cstring = |s: &str| {
        let mut v = vec![0xff, 0xfe, 0xff, s.encode_utf16().count() as u8];
        v.extend(s.encode_utf16().flat_map(|u| u.to_le_bytes()));
        v
    };
    let new_class = |name: &str| {
        let mut v = vec![0xff, 0xff, 0, 0];
        v.extend((name.len() as u16).to_le_bytes());
        v.extend(name.as_bytes());
        v
    };
    let mut a = [cstring("SketchUp Model"), cstring("{20.0.1}")].concat();
    a.extend([7u8; 16]);
    a.extend(cstring(""));
    a.extend(0u32.to_le_bytes());
    // The version map (class 1, object 2).
    a.extend(new_class("CVersionMap"));
    for (class, v) in [("CEntity", 5u32), ("CSketchUpModel", 29), ("CComponentBehavior", 5)] {
        a.extend(cstring(class));
        a.extend(v.to_le_bytes());
    }
    a.extend(cstring("End-Of-Version-Map"));
    a.extend(0u32.to_le_bytes());
    // The model up to its dictionaries.
    a.extend([0u8; 20]);
    a.extend([0, 0, 1]);
    a.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    a.extend(cstring(""));
    a.extend(3u32.to_le_bytes());
    a.extend(0u32.to_le_bytes());
    // Attribute containers, each the first field of the previous one (class 3).
    const N: usize = 100_000;
    a.extend(new_class("CAttributeContainer"));
    for _ in 1..N {
        a.extend([0x03, 0x80]);
    }
    a.extend([0, 0]);
    for _ in 0..N {
        a.extend([0, 0, 0]);
    }
    let start = std::time::Instant::now();
    let h = std::thread::Builder::new().stack_size(1 << 20).spawn(move || native::read_bytes(&a).map(|_| ())).unwrap();
    let r = h.join().expect("le fil de 1 Mo a survécu");
    let e = r.expect_err("imbrication acceptée");
    assert!(e.to_string().contains("imbriqués"), "{e}");
    assert!(start.elapsed().as_secs() < 30, "{:?}", start.elapsed());
}

#[test]
fn truncated_files_are_refused() {
    for name in ["kitchen-2021.skp", "kitchen-2020.skp", "kitchen-8.skp", "kitchen-3.skp"] {
        let data = sample(name);
        for len in (0..data.len()).step_by(7).chain(data.len().saturating_sub(64)..data.len()) {
            let r = native::read_bytes(&data[..len]);
            assert!(r.is_err() || len + 22 >= data.len(), "{name} tronqué à {len} accepté");
        }
    }
}

#[test]
fn corrupted_files_do_not_panic() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for name in ["kitchen-2021.skp", "kitchen-2020.skp", "kitchen-8.skp", "kitchen-3.skp"] {
        let data = sample(name);
        for _ in 0..300 {
            let mut d = data.clone();
            for _ in 0..1 + rng.below(8) {
                let i = rng.below(d.len());
                d[i] = rng.next() as u8;
            }
            let _ = native::read_bytes(&d);
        }
    }
}

/// The records of model.dat corrupted after decompression (past the ZIP's checksums).
#[test]
fn corrupted_records_do_not_panic() {
    let data = sample("kitchen-2021.skp");
    let f = container::open(&data).unwrap();
    let container::Body::Zip { archive, .. } = &f.body else { panic!("zip attendu") };
    let model = archive.read_name("model.dat").unwrap().to_vec();
    let opts = native::Options::default();
    assert!(vff::read(&model, Some(archive), f.version, &opts).is_ok());
    let mut rng = Rng(42);
    for round in 0..3000 {
        let mut d = model.clone();
        match round % 3 {
            // Random bytes.
            0 => {
                for _ in 0..1 + rng.below(16) {
                    let i = rng.below(d.len());
                    d[i] = rng.next() as u8;
                }
            }
            // A length field set to a large or odd value.
            1 => {
                let i = rng.below(d.len().saturating_sub(6));
                let v = [0xffff_ffffu32, 0x7fff_ffff, 1, 0, rng.next() as u32][rng.below(5)];
                d[i + 2..i + 6].copy_from_slice(&v.to_le_bytes());
            }
            // A slice cut out.
            _ => {
                let a = rng.below(d.len());
                let b = (a + rng.below(64)).min(d.len());
                d.drain(a..b);
            }
        }
        let _ = vff::read(&d, Some(archive), f.version, &opts).map(|doc| native::build::model(&doc));
    }
}

/// Values nested far deeper than any model: refused, not a stack overflow.
#[test]
fn deep_nesting_is_bounded() {
    let mut v = rec(0x38a7, &1i32.to_le_bytes());
    for _ in 0..2_000 {
        v = rec(0x38a4, &rec(0x38ae, &v));
    }
    let dict = rec(0x36b3, &[rec(0x36b4, b"d"), rec(0x36b5, &[rec(0x36b6, b"k"), v].concat())].concat());
    let attrs = rec(0x5dd, &rec(0x36b1, &rec(0x36b2, &dict)));
    let ents = rec(0x1388, &rec(0x7d0, &rec(0x5dc, &attrs)));
    let model = rec(0x1f4, &rec(0x1f6, &ents));
    let r = vff::read(&model, None, (26, 0, 0), &native::Options::default());
    assert!(r.is_err(), "nesting accepted");
    // The record tree itself is walked iteratively by the readers that do not recurse.
    assert!(tlv::is_records(&model));
}

/// Definitions that contain each other (a forged file): read once, not forever.
#[test]
fn definition_cycles_are_cut() {
    let id = |n: u8| rec(0x5de, &[n]);
    let instance_of = |def: u8| rec(0x138c, &rec(0x1964, &[rec(0x7d0, &[]), rec(0x1967, &[def])].concat()));
    let def = |own: u8, other: u8| {
        rec(
            0x157c,
            &[rec(0x1388, &[rec(0x7d0, &rec(0x5dc, &id(own))), instance_of(other)].concat()), rec(0x157e, b"D")]
                .concat(),
        )
    };
    let defs = rec(0x1f9, &rec(0x1770, &rec(0x1771, &[def(1, 2), def(2, 1)].concat())));
    let root = rec(0x1f6, &rec(0x1388, &instance_of(1)));
    let model = rec(0x1f4, &[defs, root].concat());
    let doc = vff::read(&model, None, (26, 0, 0), &native::Options::default()).unwrap();
    let m = native::build::model(&doc).unwrap();
    assert!(m.defs.len() <= 2);
}

/// A ZIP entry that claims a huge size is refused before anything is inflated.
#[test]
fn zip_bombs_are_refused() {
    let data = sample("kitchen-2021.skp");
    let f = container::open(&data).unwrap();
    let container::Body::Zip { archive, .. } = &f.body else { panic!("zip attendu") };
    let mut e = archive.find("model.dat").unwrap().clone();
    e.size = 1 << 40;
    assert!(archive.read(&e).is_err());
    e.size = e.compressed * 2000;
    assert!(archive.read(&e).is_err());
}

/// Each definition holds an instance of the next one, thousands deep: the model is built
/// (cut at the nesting limit) on a caller thread of 1 MB of stack, quickly, without a crash.
#[test]
fn deep_definition_chains_do_not_overflow() {
    const N: u32 = 10_000;
    let id = |n: u32| rec(0x5de, &n.to_le_bytes());
    let instance_of =
        |def: u32| rec(0x138c, &rec(0x1964, &[rec(0x7d0, &[]), rec(0x1967, &def.to_le_bytes())].concat()));
    let mut defs = Vec::new();
    for k in 1..=N {
        let ents = [rec(0x7d0, &rec(0x5dc, &id(k))), instance_of(k + 1)].concat();
        defs.extend(rec(0x157c, &[rec(0x1388, &ents), rec(0x157e, format!("D{k}").as_bytes())].concat()));
    }
    let defs = rec(0x1f9, &rec(0x1770, &rec(0x1771, &defs)));
    let root = rec(0x1f6, &rec(0x1388, &instance_of(1)));
    let model = rec(0x1f4, &[defs, root].concat());
    let file = skp_file(&model, &[]);
    let start = std::time::Instant::now();
    let h = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || native::read_bytes(&file).map(|(m, _)| m.defs.len()))
        .unwrap();
    let n = h.join().expect("le fil de 1 Mo a survécu").expect("lu");
    assert!(n > 100 && n <= N as usize, "{n} définitions");
    assert!(start.elapsed().as_secs() < 30, "{:?}", start.elapsed());
}

/// Groups nested 3 000 deep (each group definition holding a group of the next one), and
/// 3 000 groups side by side, read through a whole file. Naming each new group definition used
/// to scan every name taken for every index tried, a time cubic in the number of groups (150 s
/// here); the linear work is proved by counters (`native::build` tests), the time bound only
/// catches a hang (other builds may slow the tests down many times).
#[test]
fn many_groups_are_read() {
    const N: u32 = 3000;
    let id = |n: u32| rec(0x5de, &n.to_le_bytes());
    let group_of = |def: u32| rec(0x1d4c, &rec(0x1964, &[rec(0x7d0, &[]), rec(0x1967, &def.to_le_bytes())].concat()));
    let def = |k: u32, inside: &[u8]| {
        let ents = [rec(0x7d0, &rec(0x5dc, &id(k))), inside.to_vec()].concat();
        rec(0x157c, &[rec(0x1388, &ents), rec(0x157e, b"")].concat())
    };
    // Nested: definition k holds a group of k + 1; side by side: the root holds a group of each.
    let nested: Vec<u8> = (1..=N).flat_map(|k| def(k, &rec(0x138d, &group_of(k + 1)))).collect();
    let flat: Vec<u8> = (1..=N).flat_map(|k| def(k, &[])).collect();
    let all: Vec<u8> = (1..=N).flat_map(group_of).collect();
    for (defs, root) in [(nested, rec(0x138d, &group_of(1))), (flat, rec(0x138d, &all))] {
        let defs = rec(0x1f9, &rec(0x1770, &rec(0x1771, &defs)));
        let model = rec(0x1f4, &[defs, rec(0x1f6, &rec(0x1388, &root))].concat());
        let file = skp_file(&model, &[]);
        let start = std::time::Instant::now();
        let (m, _) = native::read_bytes(&file).expect("lu");
        let took = start.elapsed();
        assert!(m.defs.len() >= N as usize, "{} définitions", m.defs.len());
        assert!(m.defs.values().any(|d| d.name == format!("Groupe#{N}")));
        assert!(took.as_secs() < 60, "{took:?}");
    }
}

/// Thousands of materials naming the same large entry: it is inflated and parsed once.
#[test]
fn materials_sharing_one_large_entry_are_read_once() {
    let mut xml = String::from(
        "<materialDocument><mat:material name=\"M\" type=\"0\" colorRed=\"1\" colorGreen=\"2\" colorBlue=\"3\">",
    );
    // 8 MB of padding: about 8 kB deflated.
    xml.push_str(&format!("<!--{}-->", " ".repeat(8 << 20)));
    xml.push_str("</mat:material></materialDocument>");
    let mut list = Vec::new();
    for k in 0..20_000u32 {
        list.extend(rec(0x32c8, &[rec(0x5dc, &rec(0x5de, &(k + 1).to_le_bytes())), rec(0x32cc, b"m")].concat()));
    }
    let mats = rec(0x1f7, &rec(0x30d4, &rec(0x30d5, &list)));
    let model = rec(0x1f4, &[mats, rec(0x1f6, &rec(0x1388, &[]))].concat());
    let file = skp_file(&model, &[("materials/m/material.xml", xml.into_bytes(), true)]);
    assert!(file.len() < 1 << 20, "{}", file.len());
    let start = std::time::Instant::now();
    let (m, _) = native::read_bytes(&file).unwrap();
    assert_eq!(m.materials.len(), 20_000);
    assert!(m.materials.values().all(|x| x.color == [1, 2, 3]));
    assert!(start.elapsed().as_secs() < 30, "{:?}", start.elapsed());
}

#[test]
fn not_a_sketchup_file() {
    assert!(native::read_bytes(b"").is_err());
    assert!(native::read_bytes(b"PK\x03\x04 hello").is_err());
    assert!(native::read_bytes(&[0xff, 0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]).is_err());
}
