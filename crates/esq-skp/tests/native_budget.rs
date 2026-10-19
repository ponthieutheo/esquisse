//! What reading a forged file may cost: each test reads a small file built to make the reader
//! copy, decode or allocate far more than the file holds, and checks that it finishes quickly
//! with a bounded peak of memory (measured by the allocator of this test binary).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use esq_skp::native;

mod common;
use common::{Mfc, rec, skp_file};

/// Counts the bytes allocated now and at most.
struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(n: usize) {
    let now = NOW.fetch_add(n, Ordering::Relaxed) + n;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

// SAFETY: forwards to the system allocator, counting sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            grow(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            grow(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        NOW.fetch_sub(l.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            if new >= l.size() {
                grow(new - l.size());
            } else {
                NOW.fetch_sub(l.size() - new, Ordering::Relaxed);
            }
        }
        q
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// One measure at a time (the counters are global).
static SERIAL: Mutex<()> = Mutex::new(());

/// Runs `f`: its result, the peak of memory above what was allocated before, the time.
fn measure<T>(f: impl FnOnce() -> T) -> (T, usize, Duration) {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let base = NOW.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let t = Instant::now();
    let r = f();
    let elapsed = t.elapsed();
    (r, PEAK.load(Ordering::Relaxed).saturating_sub(base), elapsed)
}

const MB: usize = 1 << 20;

/// An edge between two vertices (`CEdge` schema 0), with the dictionaries `attrs` writes.
fn edge(m: &mut Mfc, attrs: impl FnOnce(&mut Mfc), start: impl FnOnce(&mut Mfc), end: impl FnOnce(&mut Mfc)) -> u32 {
    let e = m.new_obj("CEdge");
    attrs(m);
    m.bytes(&[0, 0]);
    start(m);
    end(m);
    e
}

fn vertex(m: &mut Mfc) -> u32 {
    let v = m.new_obj("CVertex");
    m.entity();
    m.bytes(&[0; 24]);
    v
}

/// Objects referred to again and again: an attribute container with a large dictionary shared
/// by 100 000 edges, a face whose 100 000-use loop is listed 100 000 times. Read naively, that
/// is 20 GB of copied dictionaries and 10^10 edge uses; SketchUp gives an object to one owner.
#[test]
fn shared_objects_are_given_once() {
    const M: u32 = 100_000;
    const N: u32 = 100_000;
    let big = "x".repeat(200_000);
    let file = Mfc::new().model(
        |_| 0,
        |_| 0,
        |m| {
            let mut container = 0;
            let mut ends = (0, 0);
            // The first edge holds the container; the others refer to it.
            let first = edge(
                m,
                |m| {
                    container = m.new_obj("CAttributeContainer");
                    m.entity();
                    m.new_obj("CAttributeNamed");
                    m.entity();
                    m.u32(0);
                    m.str("gros");
                    m.str("k");
                    m.u8(10);
                    m.str(&big);
                    m.str("");
                    m.u32(0);
                    m.null();
                },
                |m| ends.0 = vertex(m),
                |m| ends.1 = vertex(m),
            );
            for _ in 1..M {
                edge(m, |m| m.old(container), |m| m.old(ends.0), |m| m.old(ends.1));
            }
            // A face whose loop has N uses of the first edge, then N more references to it.
            let face = m.new_obj("CFace");
            m.drawing_element();
            m.bytes(&[0; 32]);
            m.u32(1);
            m.new_obj("CLoop");
            m.entity();
            m.bytes(&[0, 0]);
            for _ in 0..N {
                m.new_obj("CEdgeUse");
                m.entity();
                m.old(first);
                m.u8(0);
                m.null();
            }
            m.null();
            for _ in 0..N {
                m.old(face);
            }
            M + 1 + N
        },
    );
    assert!(file.len() < 4 * MB, "{}", file.len());
    let (doc, peak, t) = measure(|| native::read_doc(&file, &native::Options::default()));
    let doc = doc.expect("lu");
    assert_eq!(doc.root.edges.len(), M as usize);
    assert_eq!(doc.root.edges.iter().filter(|e| !e.el.attrs.is_empty()).count(), 1, "le dictionnaire, une fois");
    // The face once at most: its N uses are read, but its loop does not go round (one edge N
    // times) and Check Validity erases it, as SketchUp does when it opens the file
    // (`native::faces`).
    assert!(doc.root.faces.is_empty(), "{}", doc.root.faces.len());
    eprintln!("objets partagés : {} Mo de pointe, {t:?}", peak / MB);
    assert!(peak < 256 * MB, "{} Mo", peak / MB);
    assert!(t.as_secs() < 30, "{t:?}");
    let (m, _, t) = measure(|| native::build::model(&doc));
    assert!(m.is_ok() && t.as_secs() < 30, "{t:?}");
}

/// Tag folders, eight deep, each listing the next one fifty times: 50^8 folders read naively.
#[test]
fn folders_listed_many_times_are_not_multiplied() {
    fn level(m: &mut Mfc, depth: u32) {
        m.entity();
        m.str(&format!("D{depth}"));
        if depth < 8 {
            m.u32(50);
            let child = m.new_obj("CLayerGroup");
            level(m, depth + 1);
            for _ in 1..50 {
                m.old(child);
            }
        } else {
            m.u32(0);
        }
        m.u32(0);
        m.bytes(&[0, 0]);
    }
    let file = Mfc::new().model(
        |_| 0,
        |m| {
            let top = m.new_obj("CLayerGroup");
            level(m, 1);
            for _ in 1..50 {
                m.old(top);
            }
            50
        },
        |_| 0,
    );
    let (doc, peak, t) = measure(|| native::read_doc(&file, &native::Options::default()));
    let doc = doc.expect("lu");
    let mut depth = 0;
    let mut f = &doc.folders;
    while let Some(x) = f.first() {
        assert_eq!(f.len(), 1);
        depth += 1;
        f = &x.folders;
    }
    assert_eq!(depth, 8);
    assert!(peak < 16 * MB && t.as_secs() < 10, "{} Mo, {t:?}", peak / MB);
}

/// A dictionary holding an array of millions of empty values (one byte each in the file, a node
/// of JSON each in memory): refused when what it makes passes the file's budget.
#[test]
fn a_flood_of_values_is_refused() {
    const N: u32 = 2_000_000;
    let file = Mfc::new().model(
        |_| 0,
        |_| 0,
        |m| {
            edge(
                m,
                |m| {
                    m.new_obj("CAttributeContainer");
                    m.entity();
                    m.new_obj("CAttributeNamed");
                    m.entity();
                    m.u32(0);
                    m.str("d");
                    m.str("k");
                    m.u8(11);
                    m.u32(N);
                    m.bytes(&vec![0; N as usize]);
                    m.str("");
                    m.u32(0);
                    m.null();
                },
                |m| {
                    vertex(m);
                },
                |m| {
                    vertex(m);
                },
            );
            1
        },
    );
    let (r, peak, t) = measure(|| native::read_doc(&file, &native::Options::default()).map(|_| ()));
    let e = r.expect_err("accepté");
    assert!(e.to_string().contains("produit"), "{e}");
    eprintln!("valeurs : {} Mo de pointe, {t:?}", peak / MB);
    let budget = native::Budget::for_input(file.len()).bytes_left() as usize;
    assert!(peak < budget + 32 * MB, "{} Mo pour un budget de {} Mo", peak / MB, budget / MB);
    assert!(t.as_secs() < 30, "{t:?}");
}

/// A PNG whose header claims `w` × `h` pixels, without the pixels.
fn png_claiming(w: u32, h: u32) -> Vec<u8> {
    let chunk = |kind: &[u8], data: &[u8]| {
        let mut c = (data.len() as u32).to_be_bytes().to_vec();
        c.extend(kind);
        c.extend(data);
        c.extend(native::zip::crc32(&[kind, data].concat()).to_be_bytes());
        c
    };
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    [b"\x89PNG\r\n\x1a\n".to_vec(), chunk(b"IHDR", &ihdr), chunk(b"IDAT", &[0x78, 0x9c, 0x63, 0x00])].concat()
}

/// A colorized material of SketchUp 2020 (`CMaterial` 12, `CTexture` 4, `CDib` 2) whose
/// texture is `image`: SketchUp works out its average colour from the pixels.
fn colorized(m: &mut Mfc, name: &str, image: &[u8]) {
    m.new_obj("CMaterial");
    m.entity();
    m.str(name);
    m.u8(1);
    m.u8(1);
    m.u32(4);
    m.u32(image.len() as u32);
    m.bytes(image);
    m.f64(10.0);
    m.f64(10.0);
    m.str("t.png");
    m.bytes(&[40, 160, 200, 255]);
    m.u8(0);
    m.bytes(&[40, 160, 200, 255]);
    m.str("");
    m.u32(2);
    m.u32(0);
    m.f64(0.0);
    m.u8(0);
}

/// Two thousand colorized textures, each claiming 4096 × 4096 pixels: a few hundred bytes of
/// file each, 48 MB to decode each. The pixels come from the file's budget before anything is
/// decoded: past it, images are left as they are.
#[test]
fn images_claiming_large_sizes_are_not_all_decoded() {
    let png = png_claiming(4096, 4096);
    let file = Mfc::new().model(
        |m| {
            for k in 0..2000 {
                colorized(m, &format!("M{k}"), &png);
            }
            2000
        },
        |_| 0,
        |_| 0,
    );
    let (doc, peak, t) = measure(|| native::read_doc(&file, &native::Options::default()));
    let doc = doc.expect("lu");
    assert_eq!(doc.materials.len(), 2000);
    eprintln!("images : {} Mo de pointe, {t:?}", peak / MB);
    assert!(peak < 128 * MB, "{} Mo", peak / MB);
    assert!(t.as_secs() < 30, "{t:?}");
}

/// Twenty thousand material records naming one `material.xml` that holds a dictionary of 1 MB:
/// 20 GB of copies read naively. Each copy is counted against the file's budget.
#[test]
fn material_records_sharing_large_dictionaries_are_counted() {
    let xml = format!(
        "<materialDocument><mat:material name=\"M\" type=\"0\" colorRed=\"1\" colorGreen=\"2\" colorBlue=\"3\">\
         <AttributeDictionaries><AttributeDictionary name=\"d\"><Attribute key=\"k\" type=\"10\">{}</Attribute>\
         </AttributeDictionary></AttributeDictionaries></mat:material></materialDocument>",
        "a".repeat(1 << 20)
    );
    let mut list = Vec::new();
    for k in 0..20_000u32 {
        list.extend(rec(0x32c8, &[rec(0x5dc, &rec(0x5de, &(k + 1).to_le_bytes())), rec(0x32cc, b"m")].concat()));
    }
    let mats = rec(0x1f7, &rec(0x30d4, &rec(0x30d5, &list)));
    let model = rec(0x1f4, &[mats, rec(0x1f6, &rec(0x1388, &[]))].concat());
    let file = skp_file(&model, &[("materials/m/material.xml", xml.into_bytes(), true)]);
    assert!(file.len() < MB, "{}", file.len());
    let (r, peak, t) = measure(|| native::read_doc(&file, &native::Options::default()).map(|d| d.materials.len()));
    let e = r.expect_err("accepté");
    assert!(e.to_string().contains("produit"), "{e}");
    eprintln!("matières : {} Mo de pointe, {t:?}", peak / MB);
    let budget = native::Budget::for_input(file.len()).bytes_left() as usize;
    assert!(peak < budget + 32 * MB, "{} Mo", peak / MB);
    assert!(t.as_secs() < 30, "{t:?}");
}

/// Real files use a small part of their budget: the peak of memory of reading the samples (and
/// the largest files of the corpus, when present) stays below a quarter of it.
#[test]
fn real_files_stay_far_below_the_budget() {
    let mut files: Vec<std::path::PathBuf> = ["kitchen-2021.skp", "kitchen-2020.skp", "kitchen-8.skp", "kitchen-3.skp"]
        .iter()
        .map(|n| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data").join(n))
        .collect();
    if let Some(home) = std::env::var_os("USERPROFILE") {
        let corpus = std::path::PathBuf::from(home).join("tools").join("skp-corpus").join("gen");
        for v in ["3", "8", "2020"] {
            files.push(corpus.join(v).join("texture_cube.skp"));
        }
    }
    for p in files.iter().filter(|p| p.exists()) {
        let data = std::fs::read(p).unwrap();
        let budget = native::Budget::for_input(data.len()).bytes_left() as usize;
        let (r, peak, _) = measure(|| native::read_doc(&data, &native::Options::default()).map(|_| ()));
        assert!(r.is_ok(), "{}", p.display());
        eprintln!("{} : {} o, pointe {} Ko, budget {} Mo", p.display(), data.len(), peak >> 10, budget / MB);
        assert!(peak < budget / 4, "{} : {} Ko", p.display(), peak >> 10);
    }
}

/// Counts and lengths read from the file are checked against what is left of it before
/// anything is allocated or looped over.
#[test]
fn huge_counts_and_lengths_are_refused_at_once() {
    let forged = [
        // An item list claiming 2^31 items.
        Mfc::new().model(|_| 0, |_| 0, |_| 0x7fff_ffff),
        // A dictionary array claiming 2^31 values, a string claiming 2^30 characters.
        Mfc::new().model(
            |_| 0,
            |_| 0,
            |m| {
                edge(
                    m,
                    |m| {
                        m.new_obj("CAttributeContainer");
                        m.entity();
                        m.new_obj("CAttributeNamed");
                        m.entity();
                        m.u32(0);
                        m.str("d");
                        m.str("k");
                        m.u8(11);
                        m.u32(0x7fff_ffff);
                        m.u8(10);
                        m.bytes(&[0xff, 0xfe, 0xff, 0xff, 0xff, 0xff]);
                        m.u32(0x3fff_ffff);
                    },
                    |_| {},
                    |_| {},
                );
                1
            },
        ),
    ];
    for file in forged {
        let (r, peak, t) = measure(|| native::read_doc(&file, &native::Options::default()).map(|_| ()));
        assert!(r.is_err());
        assert!(peak < 16 * MB && t.as_secs() < 5, "{} Mo, {t:?}", peak / MB);
    }
}
