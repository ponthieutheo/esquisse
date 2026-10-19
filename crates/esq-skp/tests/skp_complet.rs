//! A .skp of SketchUp's, opened then saved by Esquisse, keeps what SketchUp put in it: the
//! classes beyond the geometry (styles and the style of each scene, classifications, radial
//! dimensions, 3D polylines, fonts, the dimension style, texts' fonts, environments).
//!
//! For each file of the real corpus (`ESQ_SKP_REEL`, else `%USERPROFILE%\tools\skp-reel`, and
//! `tests/reel`): native reading; native writing in the file's own version and in 2026; the
//! SDK reads the three files and what it exposes of each class (`esq_skp::sdk_census`) must be
//! the same. Counters per class: files that have the class, rewritten identically in the same
//! version, in 2026.
//!
//! `skp_complet_sample` (always run, skipped without SketchUp): `tests/reel` and the smallest
//! files of each version. `skp_complet_all` (`--ignored`, best with `--release`): the whole
//! corpus. `ESQ_COMPLET_DIFF=classe` prints the differences of a class, `ESQ_REEL_FILTER` keeps
//! the files whose path contains it.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use esq_skp::SkpVersion;
use esq_skp::native::write::{Extras, write};

mod oracle;

fn committed_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("reel")
}

fn corpus_dir() -> Option<PathBuf> {
    std::env::var_os("ESQ_SKP_REEL")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("tools").join("skp-reel")))
}

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x0100_0000_01b3))
}

/// The version a file of major version `m` is written again in.
fn version_of(m: u32) -> Option<SkpVersion> {
    if m >= 21 {
        return Some(SkpVersion::Su2021);
    }
    SkpVersion::ALL.into_iter().find(|v| v.major() == m)
}

struct Real {
    path: PathBuf,
    major: u32,
    len: usize,
}

fn corpus() -> Vec<Real> {
    let mut files = Vec::new();
    for root in std::iter::once(committed_dir()).chain(corpus_dir()) {
        let mut found = Vec::new();
        oracle::walk(&root, &mut found, 0);
        found.sort();
        files.extend(found);
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for path in files {
        let Ok(b) = std::fs::read(&path) else { continue };
        if !seen.insert((b.len(), fnv(&b))) {
            continue;
        }
        let major = esq_skp::native::container::open(&b).map_or(0, |f| f.version.0);
        out.push(Real { path, major, len: b.len() });
    }
    if let Ok(f) = std::env::var("ESQ_REEL_FILTER") {
        out.retain(|r| r.path.to_string_lossy().contains(&f));
    }
    out
}

/// What the SDK does not expose, read back by the native reader: the items of each style
/// (rendering options, sketchy edges, watermarks with their images), the model's watermarks.
/// `legacy`: compared with an MFC archive (no ambient occlusion nor sections fills in its
/// styles; 7010 derived from 7017 and 7018).
fn native_census(bytes: &[u8], legacy: bool) -> BTreeMap<&'static str, Vec<String>> {
    use esq_skp::native::style::{ItemV, Wm};
    let mut out: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    let Ok(d) = esq_skp::native::read_doc(bytes, &Default::default()) else { return out };
    let crc = |b: &Option<std::sync::Arc<Vec<u8>>>| b.as_ref().map_or(0, |b| fnv(b));
    let wm = |w: &Option<Wm>| match w {
        None => "<modèle>".to_string(),
        Some(w) => format!(
            "{:?} {:?} {} {} {} {} {} {} {} {} {} {:x}",
            w.name,
            w.file,
            w.found,
            w.time,
            w.position,
            w.fitting,
            w.keep_aspect,
            w.background,
            w.mask,
            w.alpha,
            w.scale,
            crc(&w.image)
        ),
    };
    let skipped = |id: u32| legacy && [7010, 7019, 7020, 8100, 8102, 8103, 8105, 8106, 8107].contains(&id);
    let style = |s: &esq_skp::native::style::StyleRec| {
        let items: Vec<String> = s
            .items
            .iter()
            .filter(|i| !skipped(i.id))
            .map(|i| match &i.v {
                ItemV::Watermarks(l) => format!("{}[{}]", i.id, l.iter().map(wm).collect::<Vec<_>>().join(" | ")),
                ItemV::Npr(n) => format!(
                    "{} esquisse {} {} {} {} {} {} {:?} {:?}",
                    i.id,
                    n.dropout,
                    n.fade,
                    n.width,
                    n.halo,
                    n.halo_length,
                    n.protos,
                    n.lengths,
                    n.images.iter().map(|b| fnv(b)).collect::<Vec<_>>()
                ),
                v => format!("{}={:?}", i.id, v.f64()),
            })
            .collect();
        format!("{:?} {:?} {} {:?} {}", s.name, s.description, s.guid, s.path, items.join(" "))
    };
    // Dictionaries nested in others (the SDK does not list them): those of the model, of the
    // definitions and their instances, of the materials and of the tags.
    {
        let mut lines = Vec::new();
        let mut nested = |owner: &str, a: &esq_core::model::Attributes| {
            for (k, v) in a.iter().filter(|(k, _)| k.contains(esq_skp::native::doc::NEST)) {
                lines.push(format!("{owner} | {} | {v:?}", k.replace(esq_skp::native::doc::NEST, " > ")));
            }
        };
        nested("modèle", &d.attrs);
        // A group's definition by what it is (Esquisse names them anew, as the SDK does).
        fn label(x: &esq_skp::native::doc::Def) -> &str {
            if x.is_group { "groupe" } else { x.name.as_str() }
        }
        let names: BTreeMap<u64, &str> = d.defs.iter().map(|x| (x.id, label(x))).collect();
        let insts = |owner: &str,
                     e: &esq_skp::native::doc::Ents,
                     nested: &mut dyn FnMut(&str, &esq_core::model::Attributes)| {
            for i in e.groups.iter().chain(&e.instances) {
                nested(&format!("{owner} → {}", names.get(&i.def).copied().unwrap_or("?")), &i.el.attrs);
            }
        };
        insts("racine", &d.root, &mut nested);
        for x in &d.defs {
            nested(&format!("définition {}", label(x)), &x.attrs);
            insts(label(x), &x.ents, &mut nested);
        }
        for m in &d.materials {
            nested(&format!("matière {}", m.name), &m.attrs);
        }
        for l in &d.layers {
            nested(&format!("étiquette {}", l.name), &l.attrs);
        }
        if !lines.is_empty() {
            out.insert("dictionnaires imbriqués", lines);
        }
        // Thumbnails of the definitions (their image; a group's by kind) and of the materials.
        let mut thumbs = Vec::new();
        for x in &d.defs {
            if let Some(t) = x.thumbnail.as_ref().and_then(|t| t.image.as_ref()) {
                thumbs.push(format!("définition {} {:x}", label(x), fnv(t)));
            }
        }
        for m in &d.materials {
            if let Some(t) = &m.thumbnail {
                thumbs.push(format!("matière {} {:x}", m.name, fnv(t)));
            }
        }
        if !thumbs.is_empty() {
            out.insert("vignettes", thumbs);
        }
        // What components are glued to (relationships), by what they are.
        let mut glues = Vec::new();
        let mut glued = |owner: &str, e: &esq_skp::native::doc::Ents| {
            let inst = |id: u64| e.instances.iter().chain(&e.groups).find(|i| i.el.id == id);
            for [to, what] in &e.glues {
                let Some(w) = inst(*what) else { continue };
                let target = if e.faces.iter().any(|f| f.el.id == *to) {
                    "une face".to_string()
                } else if let Some(t) = inst(*to) {
                    names.get(&t.def).copied().unwrap_or("?").to_string()
                } else {
                    continue;
                };
                glues.push(format!("{owner} | {} → {target}", names.get(&w.def).copied().unwrap_or("?")));
            }
        };
        glued("racine", &d.root);
        for x in &d.defs {
            glued(label(x), &x.ents);
        }
        if !glues.is_empty() {
            out.insert("collages", glues);
        }
        // Where the definitions come from (GUID, file, time) and the GUIDs of the instances (not
        // compared with an MFC archive: those before SketchUp 5 have none, 2021+ files one each).
        let mut prov: Vec<String> = d
            .defs
            .iter()
            .filter(|x| !x.is_image)
            .map(|x| format!("définition {} {} {:?} {}", label(x), x.guid, x.path, x.time))
            .collect();
        let mut guids = |owner: &str, e: &esq_skp::native::doc::Ents| {
            for i in e.instances.iter().chain(&e.groups).filter(|_| !legacy) {
                prov.push(format!("{owner} → {} {}", names.get(&i.def).copied().unwrap_or("?"), i.guid));
            }
        };
        guids("racine", &d.root);
        for x in &d.defs {
            guids(label(x), &x.ents);
        }
        if !prov.is_empty() {
            out.insert("provenance", prov);
        }
        // The model's values whose use is not known, when they are not a new model's.
        if let Some(f) = d.model_flags.filter(|f| *f != Default::default()) {
            out.insert("valeurs du modèle", vec![format!("{f:?}")]);
        }
    }
    let st = &d.styles;
    // A file without styles: SketchUp makes one when it opens it (nothing to compare).
    if st.list.is_empty() {
        for v in out.values_mut() {
            v.sort();
        }
        return out;
    }
    let filigranes = out.entry("filigranes et éléments des styles").or_default();
    for s in &st.list {
        filigranes.push(format!("style {}", style(s)));
    }
    if let Some(a) = &st.active {
        filigranes.push(format!("actif {}", style(a)));
    }
    filigranes.push(format!("modèle [{}]", st.watermarks.iter().map(wm).collect::<Vec<_>>().join(" | ")));
    for v in out.values_mut() {
        v.sort();
    }
    out
}

/// Per class: files that have it, identical in the same version, identical in 2026.
#[derive(Default)]
struct Count {
    files: usize,
    same: usize,
    current: usize,
}

#[derive(Default)]
struct Tally {
    read: usize,
    sdk_refused: usize,
    native_refused: Vec<String>,
    write_failed: Vec<String>,
    per_class: BTreeMap<&'static str, Count>,
    /// « file (version) : class » for each difference.
    failures: Vec<String>,
}

fn show_diff(path: &Path, what: &str, class: &str, a: &[String], b: &[String]) {
    let (only_a, only_b) = esq_skp::native::canon::diff(a, b);
    eprintln!("== {} ({class}, {what}) : {} lignes / {}", path.display(), a.len(), b.len());
    // Paired lines: the words that differ, after the line's first words.
    for i in 0..only_a.len().max(only_b.len()).min(6) {
        let (x, y) = (only_a.get(i).map_or("", |s| s), only_b.get(i).map_or("", |s| s));
        let (wx, wy): (Vec<&str>, Vec<&str>) = (x.split(' ').collect(), y.split(' ').collect());
        let head: String = wx.iter().take(3).copied().collect::<Vec<_>>().join(" ");
        let dx: Vec<&str> = wx.iter().filter(|w| !wy.contains(w)).copied().collect();
        let dy: Vec<&str> = wy.iter().filter(|w| !wx.contains(w)).copied().collect();
        let cut = |v: Vec<&str>| v.join(" ").chars().take(300).collect::<String>();
        eprintln!("  {head}\n    SketchUp  {}\n    Esquisse  {}", cut(dx), cut(dy));
    }
}

impl Tally {
    fn check(&mut self, r: &Real, tmp: &Path) {
        let Some(orig) = esq_skp::sdk_census(&r.path) else {
            self.sdk_refused += 1;
            return;
        };
        // What changes from one opening to the next (the GUID of the style SketchUp makes for a
        // file that has none) is not compared.
        let again = esq_skp::sdk_census(&r.path).unwrap_or_default();
        let unstable: Vec<&str> = orig.iter().filter(|(k, v)| again.get(*k) != Some(v)).map(|(k, _)| *k).collect();
        let mask = |class: &str, l: &[String]| -> Vec<String> {
            if !unstable.contains(&class) {
                return l.to_vec();
            }
            let mut v: Vec<String> = l
                .iter()
                .map(|s| match s.find("guid=") {
                    Some(i) => {
                        let end = s[i..].find(' ').map_or(s.len(), |e| i + e);
                        format!("{}guid=?{}", &s[..i], &s[end..])
                    }
                    None => s.clone(),
                })
                .collect();
            v.sort();
            v
        };
        let bytes = std::fs::read(&r.path).unwrap_or_default();
        let (model, view) = match esq_skp::native::read_bytes(&bytes) {
            Ok(x) => x,
            Err(e) => {
                self.native_refused.push(format!("{} : {e}", r.path.display()));
                return;
            }
        };
        self.read += 1;
        let show = std::env::var("ESQ_COMPLET_DIFF").ok();
        let mut rewritten = Vec::new();
        for (what, v) in [("même version", version_of(r.major)), ("2026", Some(SkpVersion::Su2021))] {
            let Some(v) = v else { continue };
            let out = tmp.join(format!("{}.skp", if what == "2026" { "courant" } else { "meme" }));
            let written = write(&model, view.as_ref(), Some(v), &Extras::default()).map_err(|e| e.to_string());
            let census = written
                .clone()
                .and_then(|b| std::fs::write(&out, b).map_err(|e| e.to_string()))
                .and_then(|_| esq_skp::sdk_census(&out).ok_or_else(|| "refusé par le SDK".to_string()));
            // What the SDK does not show, read back natively.
            if let Ok(b) = &written {
                let legacy = r.major < 21 || v != SkpVersion::Su2021;
                let (x, y) = (native_census(&bytes, legacy), native_census(b, legacy));
                for (class, a) in &x {
                    let e = self.per_class.entry(class).or_default();
                    if what != "2026" {
                        e.files += 1;
                    }
                    if Some(a) == y.get(class) {
                        if what == "2026" {
                            e.current += 1;
                        } else {
                            e.same += 1;
                        }
                    } else {
                        self.failures.push(format!("{} ({what}) : {class}", r.path.display()));
                        if show.as_deref().is_some_and(|s| s.split(',').any(|x| x == "natif")) {
                            show_diff(&r.path, what, class, a, &y[class]);
                        }
                    }
                }
            }
            // What SketchUp itself makes of the file in that version (its own conversion).
            let by_sketchup = tmp.join("sketchup.skp");
            let theirs = esq_skp::sdk_resave(&r.path, &by_sketchup, Some(v))
                .ok()
                .and_then(|_| esq_skp::sdk_census(&by_sketchup));
            match census {
                Ok(c) => rewritten.push((what, c, theirs)),
                Err(e) => self.write_failed.push(format!("{} ({what}) : {e}", r.path.display())),
            }
        }
        for class in esq_skp::CENSUS_CLASSES {
            let a = &orig[class];
            let has = !a.is_empty() || rewritten.iter().any(|(_, c, _)| !c[class].is_empty());
            if !has {
                continue;
            }
            let e = self.per_class.entry(class).or_default();
            e.files += 1;
            for (what, c, theirs) in &rewritten {
                // As the file was, or as SketchUp converts it.
                let (ours, was) = (mask(class, &c[class]), mask(class, a));
                if ours == was || theirs.as_ref().is_some_and(|t| mask(class, &t[class]) == ours) {
                    if *what == "2026" {
                        e.current += 1;
                    } else {
                        e.same += 1;
                    }
                } else {
                    self.failures.push(format!("{} ({what}) : {class}", r.path.display()));
                    if show.as_deref().is_some_and(|s| s.split(',').any(|x| x == *class)) {
                        show_diff(&r.path, what, class, a, &c[class]);
                    }
                }
            }
        }
    }

    fn print(&self, title: &str) {
        println!("{title} : {} lus nativement, {} refusés par le SDK", self.read, self.sdk_refused);
        println!("  classe                  fichiers  même version  2026");
        for (c, n) in &self.per_class {
            println!("  {c:<22} {:>9} {:>13} {:>5}", n.files, n.same, n.current);
        }
        if !self.native_refused.is_empty() {
            println!("  refusés par le lecteur natif : {}", self.native_refused.len());
        }
        if !self.write_failed.is_empty() {
            println!("  écritures en échec : {}", self.write_failed.len());
            for f in self.write_failed.iter().take(10) {
                println!("    {f}");
            }
        }
    }
}

fn run(files: &[&Real], title: &str) -> Tally {
    let tmp = std::env::temp_dir().join(format!("esq-skp-complet-{}-{}", std::process::id(), title.len()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut t = Tally::default();
    for r in files {
        t.check(r, &tmp);
    }
    let _ = std::fs::remove_dir_all(&tmp);
    t.print(title);
    t
}

#[test]
fn skp_complet_sample() {
    if !esq_skp::available() {
        eprintln!("SketchUp absent : test ignoré");
        return;
    }
    let files = corpus();
    let committed = committed_dir();
    let mut by_version: BTreeMap<u32, Vec<&Real>> = BTreeMap::new();
    let mut sample: Vec<&Real> = Vec::new();
    for r in &files {
        if r.path.starts_with(&committed) {
            sample.push(r);
        } else {
            by_version.entry(r.major).or_default().push(r);
        }
    }
    for list in by_version.values_mut() {
        list.sort_by_key(|r| r.len);
        sample.extend(list.iter().take(2));
    }
    let t = run(&sample, "skp-complet — échantillon");
    let _ = t;
}

#[test]
#[ignore]
fn skp_complet_all() {
    if !esq_skp::available() {
        eprintln!("SketchUp absent : test ignoré");
        return;
    }
    let files = corpus();
    let all: Vec<&Real> = files.iter().collect();
    let t = run(&all, "skp-complet — corpus réel");
    if std::env::var_os("ESQ_COMPLET_LIST").is_some() {
        for f in &t.failures {
            println!("{f}");
        }
    }
}

/// The dictionaries nested in others (`AttributeDictionary#attribute_dictionaries`, where
/// SketchUp keeps the values of a classification) go through a .skp of every version that has
/// dictionaries: those of the model, of a component's definition and of its instance, three deep.
#[test]
fn nested_dictionaries_go_through_every_version() {
    use esq_core::glam::DVec3;
    use esq_core::model::{AttrValue, EntityRef, Model, set_attr};
    let mut m = Model::new();
    let pts = [DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, 500.0, 0.0), DVec3::new(0.0, 500.0, 0.0)];
    m.root.add_polyline(&pts, true, false);
    let edges: Vec<EntityRef> = m.root.edges.keys().map(EntityRef::Edge).collect();
    let inst = m.make_group(None, &edges, true, Some("Mur".into())).unwrap();
    let def = m.root.instances[inst].def;
    let d = format!("d:0:{}", slotmap::Key::data(&def).as_ffi());
    let i = format!("i:0:{}", slotmap::Key::data(&inst).as_ffi());
    let n = |v: f64| AttrValue::Number(v);
    set_attr(&mut m.defs[def].attributes, "AppliedSchemaTypes", "IFC 2x3", AttrValue::Text("IfcWall".into()));
    m.defs[def].attributes.entry("IFC 2x3".into()).or_default();
    set_attr(m.attr_store.entry(format!("ad:{d}|IFC 2x3")).or_default(), "Depth", "value", n(0.2));
    set_attr(
        m.attr_store.entry(format!("ad:ad:{d}|IFC 2x3|Depth")).or_default(),
        "IfcLengthMeasure",
        "attribute_type",
        AttrValue::Text("double".into()),
    );
    set_attr(m.attr_store.entry("model".into()).or_default(), "fiche", "auteur", AttrValue::Text("Théo".into()));
    set_attr(m.attr_store.entry("ad:model|fiche".into()).or_default(), "détail", "page", n(3.0));
    set_attr(&mut m.root.instances[inst].attributes, "repère", "n", n(1.0));
    set_attr(m.attr_store.entry(format!("ad:{i}|repère")).or_default(), "sous", "x", n(2.0));
    // The nested ones as paths from their owner, whatever the ids.
    let nested = |m: &Model| -> Vec<String> {
        let def = m.defs.iter().find(|(_, x)| x.attributes.contains_key("IFC 2x3")).map(|(k, _)| k).unwrap();
        let inst = m.root.instances.iter().find(|(_, x)| x.def == def).map(|(k, _)| k).unwrap();
        let d = format!("d:0:{}", slotmap::Key::data(&def).as_ffi());
        let i = format!("i:0:{}", slotmap::Key::data(&inst).as_ffi());
        let mut out: Vec<String> = m
            .attr_store
            .iter()
            .filter(|(k, _)| k.starts_with("ad:"))
            .map(|(k, a)| format!("{} {a:?}", k.replace(&d, "définition").replace(&i, "instance")))
            .collect();
        out.sort();
        out
    };
    let want = nested(&m);
    assert_eq!(want.len(), 4);
    for v in SkpVersion::ALL.into_iter().filter(|v| v.major() >= 4) {
        let bytes = esq_skp::native::write::write(&m, None, Some(v), &Default::default()).unwrap();
        let (back, _) = esq_skp::native::read_bytes(&bytes).unwrap();
        assert_eq!(nested(&back), want, "{v:?}");
    }
}

/// A radial dimension made in Esquisse (`Entities#add_dimension_radial`: from the centre to a
/// point of the arc, `radial` the arc) is one in a .skp of every version: SketchUp reads a
/// radial dimension of that arc, and Esquisse reads it back as it was.
#[test]
fn a_radial_dimension_goes_through_every_version() {
    use esq_core::glam::DVec3;
    use esq_core::model::{AnnotKind, Annotation, AttrValue, Model};
    let mut m = Model::new();
    let r = 500.0;
    let ring: Vec<DVec3> = (0..24)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / 24.0;
            DVec3::new(a.cos(), a.sin(), 0.0) * r
        })
        .collect();
    m.root.add_polyline(&ring, true, true);
    let cid = m.root.edges.values().find_map(|e| e.curve).unwrap();
    let t = DVec3::new(r, 0.0, 0.0);
    let id = m.root.annots.insert(Annotation {
        kind: AnnotKind::Linear { a: DVec3::ZERO, b: t, offset: DVec3::ZERO },
        text: String::new(),
        layer: 0,
        hidden: false,
        attach: Default::default(),
    });
    let key = format!("a{}", slotmap::Key::data(&id).as_ffi());
    m.root.set_prop(&key, "radial", AttrValue::Text(format!("c:0:{cid}")));
    m.root.set_prop(&key, "arrow_type", AttrValue::Number(3.0));
    m.root.set_prop(&key, "diameter", AttrValue::Bool(true));
    for v in SkpVersion::ALL {
        let bytes = esq_skp::native::write::write(&m, None, Some(v), &Default::default()).unwrap();
        let (back, _) = esq_skp::native::read_bytes(&bytes).unwrap();
        let radial: Vec<_> = back
            .root
            .annots
            .iter()
            .filter(|(k, _)| back.root.prop(&format!("a{}", slotmap::Key::data(k).as_ffi()), "radial").is_some())
            .collect();
        assert_eq!(radial.len(), 1, "{v:?}");
        let (k, a) = radial[0];
        let key = format!("a{}", slotmap::Key::data(&k).as_ffi());
        let AnnotKind::Linear { a: c, b, .. } = a.kind else { panic!("{v:?}") };
        assert!(c.length() < 1e-6 && b.distance(t) < 1e-3, "{v:?} : {c} {b}");
        assert_eq!(back.root.prop(&key, "diameter"), Some(&AttrValue::Bool(true)), "{v:?}");
        let Some(AttrValue::Text(h)) = back.root.prop(&key, "radial") else { panic!("{v:?}") };
        let c: u32 = h.strip_prefix("c:0:").and_then(|x| x.parse().ok()).expect("l'arc de la racine");
        assert!(back.root.edges.values().any(|e| e.curve == Some(c)), "{v:?}");
        if esq_skp::available() {
            let p = std::env::temp_dir().join(format!("esq-radiale-{}-{}.skp", std::process::id(), v.name()));
            std::fs::write(&p, &bytes).unwrap();
            let census = esq_skp::sdk_census(&p).expect("le SDK ouvre le fichier");
            let _ = std::fs::remove_file(&p);
            let lines = &census["cotes radiales"];
            assert_eq!(lines.len(), 1, "{v:?}");
            assert!(lines[0].contains("diamètre=true flèche=3"), "{v:?} : {}", lines[0]);
        }
    }
}

/// A radial dimension whose arc is gone (`radial` empty, its arc kept in `skp_arc`: files of
/// SketchUp 2017 have them) is written without a reference to an edge: SketchUp opens the file
/// in every version (an edge id 0 made SketchUp 2026 refuse the whole model).
#[test]
fn a_radial_dimension_without_its_arc_is_written_as_sketchup_writes_it() {
    use esq_core::glam::DVec3;
    use esq_core::model::{AnnotKind, Annotation, AttrValue, Model};
    let mut m = Model::new();
    m.root.add_line(DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0));
    let id = m.root.annots.insert(Annotation {
        kind: AnnotKind::Linear { a: DVec3::ZERO, b: DVec3::ZERO, offset: DVec3::ZERO },
        text: String::new(),
        layer: 0,
        hidden: false,
        attach: Default::default(),
    });
    let key = format!("a{}", slotmap::Key::data(&id).as_ffi());
    m.root.set_prop(&key, "radial", AttrValue::Text(String::new()));
    let zero_arc: Vec<serde_json::Value> = (0..15)
        .map(|k| {
            serde_json::json!(if k == 5 {
                1.0
            } else if k == 14 {
                std::f64::consts::TAU
            } else {
                0.0
            })
        })
        .collect();
    m.root.set_prop(&key, "skp_arc", AttrValue::Json(serde_json::Value::Array(zero_arc)));
    m.root.set_prop(&key, "skp_angle", AttrValue::Number(4.25));
    for v in SkpVersion::ALL {
        let bytes = esq_skp::native::write::write(&m, None, Some(v), &Default::default()).unwrap();
        if esq_skp::available() {
            let p = std::env::temp_dir().join(format!("esq-radiale-vide-{}-{}.skp", std::process::id(), v.name()));
            std::fs::write(&p, &bytes).unwrap();
            let census = esq_skp::sdk_census(&p);
            let _ = std::fs::remove_file(&p);
            assert!(census.is_some(), "{v:?} : le SDK refuse le fichier");
        }
    }
}
/// A 3D polyline (`CPolyline3d`, kept in `Entities::polylines`) goes through a .skp of every
/// version: Esquisse and SketchUp read the same points back.
#[test]
fn a_polyline_goes_through_every_version() {
    use esq_core::glam::DVec3;
    use esq_core::model::{Model, Polyline3d};
    let mut m = Model::new();
    let tag = m.add_layer("Tracés");
    let points = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(254.0, 0.0, 25.4), DVec3::new(254.0, 508.0, 50.8)];
    m.root.polylines.push(Polyline3d { points: points.clone(), layer: tag, hidden: false });
    for v in SkpVersion::ALL {
        let bytes = esq_skp::native::write::write(&m, None, Some(v), &Default::default()).unwrap();
        let (back, _) = esq_skp::native::read_bytes(&bytes).unwrap();
        assert_eq!(back.root.polylines.len(), 1, "{v:?}");
        let pl = &back.root.polylines[0];
        assert!(pl.points.iter().zip(&points).all(|(a, b)| a.distance(*b) < 1e-9), "{v:?} : {pl:?}");
        assert_eq!(back.layers[pl.layer].name, "Tracés", "{v:?}");
        if esq_skp::available() {
            let p = std::env::temp_dir().join(format!("esq-polyligne-{}-{}.skp", std::process::id(), v.name()));
            std::fs::write(&p, &bytes).unwrap();
            let census = esq_skp::sdk_census(&p).expect("le SDK ouvre le fichier");
            let _ = std::fs::remove_file(&p);
            assert_eq!(census["polylignes"], ["<racine> (0 0 0) (10 0 1) (10 20 2)"], "{v:?}");
        }
    }
}

/// The thumbnails a .skp keeps (a definition's, with its camera; a material's in 2021+) and the
/// components nothing places go through a .skp of every version.
#[test]
fn thumbnails_and_unplaced_components_go_through_every_version() {
    use esq_core::glam::DVec3;
    use esq_core::model::{Definition, Model};
    use esq_core::photo::Blob;
    use esq_skp::native::build::{def_thumbnail_key, mat_thumbnail_key};
    let mut m = Model::new();
    let pts = [DVec3::ZERO, DVec3::new(100.0, 0.0, 0.0), DVec3::new(100.0, 100.0, 0.0)];
    m.root.add_polyline(&pts, true, false);
    let edges: Vec<_> = m.root.edges.keys().map(esq_core::model::EntityRef::Edge).collect();
    let inst = m.make_group(None, &edges, false, Some("Tabouret".into())).unwrap();
    let placed = m.root.instances[inst].def;
    // A component of the model that nothing places.
    let mut spare = Definition {
        name: "Réserve".into(),
        is_group: false,
        description: "en attente".into(),
        attributes: Default::default(),
        ext: Default::default(),
        entities: Default::default(),
    };
    spare.entities.add_polyline(&pts, true, false);
    m.defs.insert(spare);
    let png = |seed: u8| {
        let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
        v.extend((0..64).map(|i| i ^ seed));
        v
    };
    m.defs[placed].ext.thumbnail_camera =
        Some([DVec3::new(254.0, -508.0, 762.0), DVec3::new(25.4, 25.4, 0.0), DVec3::Z]);
    m.files.insert(def_thumbnail_key("Tabouret"), Blob(std::sync::Arc::new(png(1))));
    m.files.insert(def_thumbnail_key("Réserve"), Blob(std::sync::Arc::new(png(2))));
    let wood = m.add_material("Bois", [150, 100, 50]);
    m.root.faces.values_mut().for_each(|f| f.front = Some(wood));
    m.files.insert(mat_thumbnail_key("Bois"), Blob(std::sync::Arc::new(png(3))));
    for v in SkpVersion::ALL.into_iter().filter(|v| v.major() >= 6) {
        let bytes = esq_skp::native::write::write(&m, None, Some(v), &Default::default()).unwrap();
        let (back, _) = esq_skp::native::read_bytes(&bytes).unwrap();
        let def = |n: &str| back.defs.values().find(|d| d.name == n).unwrap_or_else(|| panic!("{v:?} : {n}"));
        assert_eq!(def("Réserve").description, "en attente", "{v:?}");
        for (name, seed) in [("Tabouret", 1), ("Réserve", 2)] {
            let got = back.files.get(&def_thumbnail_key(name)).map(|b| b.0.as_slice().to_vec());
            assert_eq!(got, Some(png(seed)), "{v:?} : {name}");
        }
        let cam = def("Tabouret").ext.thumbnail_camera.unwrap();
        assert!(cam[0].distance(DVec3::new(254.0, -508.0, 762.0)) < 1e-6, "{v:?} : {cam:?}");
        // A material's thumbnail: kept by the 2021+ format only (beside its file).
        let mat = back.files.get(&mat_thumbnail_key("Bois")).map(|b| b.0.as_slice().to_vec());
        assert_eq!(mat, (v == SkpVersion::Su2021).then(|| png(3)), "{v:?}");
    }
}

/// The environments of a model (Window › Environments, one Esquisse ships) and the one a scene
/// keeps go through a 2021+ .skp, written natively or by the SDK: SketchUp lists them with their
/// settings, the current one and the scene's; Esquisse reads them back with their panorama. A
/// version before 2021 has none (SketchUp's conversion drops them too).
#[test]
fn environments_go_through_a_2021_file() {
    use esq_core::model::{Model, Scene};
    use esq_core::photo::Blob;
    let ske = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/Environments/Studio blanc.ske"))
        .expect("l'environnement livré");
    let mut m = Model::new();
    m.root.add_polyline(&[esq_core::glam::DVec3::ZERO, esq_core::glam::DVec3::new(1000.0, 0.0, 0.0)], false, false);
    m.view_ext.insert(
        "envs".into(),
        serde_json::json!([
            {"id": 1, "name": "Ciel", "description": "un ciel", "skydome": true, "reflections": false, "rotation": 30.0,
             "skydome_exposure": 1.5, "reflection_exposure": 0.75, "linked_sun": true, "sun_pos": [90.0, 45.0]},
            {"id": 2, "name": "Neige", "description": "", "skydome": false, "reflections": true, "rotation": 90.0,
             "skydome_exposure": 2.0, "reflection_exposure": 3.0, "linked_sun": false, "sun_pos": [0.0, 45.0]}
        ]),
    );
    m.view_ext.insert("env_current".into(), serde_json::json!(1));
    m.files.insert("env:1".into(), Blob(std::sync::Arc::new(ske.clone())));
    m.files.insert("env:2".into(), Blob(std::sync::Arc::new(ske.clone())));
    let mut scene = Scene {
        name: "Sous la neige".into(),
        description: String::new(),
        camera: esq_core::model::SceneCamera {
            eye: esq_core::glam::DVec3::new(-1000.0, -1000.0, 1000.0),
            target: esq_core::glam::DVec3::ZERO,
            up: esq_core::glam::DVec3::Z,
            fov_deg: 35.0,
            perspective: true,
        },
        hidden_layers: None,
        photo: None,
        ext: Default::default(),
    };
    scene.ext.props.insert("flags".into(), serde_json::json!(0x201));
    scene.ext.props.insert("env".into(), serde_json::json!("Neige"));
    m.scenes.push(scene);
    // The panorama the .ske carries (`ref/`), the one the .skp keeps.
    let zip = esq_core::zip::Archive::open(&ske).unwrap();
    let entry = zip.entries.iter().find(|e| e.name.starts_with("ref/")).expect("un panorama");
    let pano = zip.read(entry, &esq_core::untrusted::Budget::for_input(ske.len())).unwrap();
    let expected = [
        "\"Ciel\" desc=\"un ciel\" rotation=30 ciel=true 1.5 reflets=false 0.75 soleil=true",
        "\"Neige\" desc=\"\" rotation=90 ciel=false 2 reflets=true 3 soleil=false",
        "courant=\"Ciel\"",
    ];
    let census = |bytes: &[u8], what: &str| {
        let p = std::env::temp_dir().join(format!("esq-env-{}-{what}.skp", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        let c = esq_skp::sdk_census(&p).expect("le SDK ouvre le fichier");
        let _ = std::fs::remove_file(&p);
        c
    };
    for v in SkpVersion::ALL.into_iter().filter(|v| v.major() >= 3) {
        let bytes = esq_skp::native::write::write(&m, None, Some(v), &Default::default()).unwrap();
        let (back, _) = esq_skp::native::read_bytes(&bytes).unwrap();
        let envs = back.view_ext.get("envs").and_then(|l| l.as_array()).cloned().unwrap_or_default();
        if v != SkpVersion::Su2021 {
            assert!(envs.is_empty(), "{v:?} : {envs:?}");
            continue;
        }
        let names: Vec<_> = envs.iter().map(|e| e["name"].as_str().unwrap_or_default()).collect();
        assert_eq!(names, ["Ciel", "Neige"], "{v:?}");
        assert_eq!((envs[0]["rotation"].as_f64(), envs[0]["sun_pos"][0].as_f64()), (Some(30.0), Some(90.0)));
        assert_eq!(back.view_ext.get("env_current"), Some(&serde_json::json!(1)));
        for id in [1, 2] {
            assert_eq!(back.files.get(&format!("env:{id}")).map(|b| b.0.as_slice()), Some(pano.as_slice()), "{id}");
        }
        assert_eq!(back.scenes[0].ext.props.get("env"), Some(&serde_json::json!("Neige")));
        if esq_skp::available() {
            let c = census(&bytes, "natif");
            assert_eq!(c["environnements"], expected, "natif");
            assert!(
                c["styles des scènes"][0].contains("env=\"Neige\" utilise_env=true"),
                "{:?}",
                c["styles des scènes"]
            );
        }
    }
    if esq_skp::available() {
        let p = std::env::temp_dir().join(format!("esq-env-{}-sdk.skp", std::process::id()));
        esq_skp::sdk_export_skp_version(&m, &p, None, None).unwrap();
        let c = esq_skp::sdk_census(&p).expect("le SDK ouvre son fichier");
        let _ = std::fs::remove_file(&p);
        assert_eq!(c["environnements"], expected, "SDK");
        assert!(c["styles des scènes"][0].contains("env=\"Neige\" utilise_env=true"), "{:?}", c["styles des scènes"]);
    }
}

/// A matched photo made in Esquisse (Camera › Match New Photo) goes through a .skp of every
/// version that has them: SketchUp shows it in its scene, Esquisse reads it back (image, lines,
/// camera), with the scene's transition and delay times.
#[test]
fn a_matched_photo_goes_through_every_version() {
    use esq_core::glam::DVec2;
    use esq_core::matchphoto::{Line2, Scale, Setup, solve};
    use esq_core::model::{Model, Scene};
    use esq_core::photo::{Blob, MatchPhoto};
    let (w, h) = (640.0, 480.0);
    let l = |a: (f64, f64), b: (f64, f64)| Line2 { a: DVec2::new(a.0, a.1), b: DVec2::new(b.0, b.1) };
    let setup = Setup {
        size: DVec2::new(w, h),
        principal: None,
        lines: [
            Some([l((272.0, 171.0), (107.0, 202.0)), l((272.0, 309.0), (107.0, 278.0))]),
            Some([l((368.0, 309.0), (508.0, 133.0)), l((368.0, 171.0), (508.0, 347.0))]),
            Some([l((389.0, 192.0), (389.0, 35.0)), l((251.0, 192.0), (251.0, 35.0))]),
        ],
        origin: DVec2::new(320.0, 240.0),
        scale: Scale::Distance(3000.0),
        variant: None,
    };
    let cam = solve(&setup).expect("une caméra");
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(0..200u8);
    let photo = MatchPhoto {
        name: "façade.png".into(),
        image: Blob(std::sync::Arc::new(png.clone())),
        size: [640, 480],
        setup: setup.clone(),
        camera: cam,
        grid: Default::default(),
    };
    let mut m = Model::new();
    let mut scene = Scene {
        name: "Façade".into(),
        description: String::new(),
        camera: photo.scene_camera(),
        hidden_layers: None,
        photo: Some(photo),
        ext: Default::default(),
    };
    scene.ext.props.insert("transition".into(), serde_json::json!(2.5));
    scene.ext.props.insert("delay".into(), serde_json::json!(1.0));
    m.scenes.push(scene);
    m.ensure_scene_uids();
    for v in SkpVersion::ALL.into_iter().filter(|v| v.major() >= 7) {
        let bytes = esq_skp::native::write::write(&m, None, Some(v), &Default::default()).unwrap();
        let (back, _) = esq_skp::native::read_bytes(&bytes).unwrap();
        let s = &back.scenes[0];
        let ph = s.photo.as_ref().unwrap_or_else(|| panic!("{v:?} : pas de photo"));
        assert_eq!(
            (ph.image.0.as_slice(), ph.size, ph.name.as_str()),
            (png.as_slice(), [640, 480], "façade.png"),
            "{v:?}"
        );
        for (a, b) in ph.setup.lines.iter().flatten().flatten().zip(setup.lines.iter().flatten().flatten()) {
            assert!(a.a.distance(b.a) < 1e-6 && a.b.distance(b.b) < 1e-6, "{v:?}");
        }
        assert!((ph.camera.fov_y_deg() - cam.fov_y_deg()).abs() < 1e-6, "{v:?}");
        assert!(ph.camera.forward().distance(cam.forward()) < 1e-9, "{v:?}");
        assert_eq!(
            (s.ext.props.get("transition"), s.ext.props.get("delay")),
            (Some(&serde_json::json!(2.5)), Some(&serde_json::json!(1.0))),
            "{v:?}"
        );
        if esq_skp::available() {
            let p = std::env::temp_dir().join(format!("esq-photo-{}-{}.skp", std::process::id(), v.name()));
            std::fs::write(&p, &bytes).unwrap();
            let census = esq_skp::sdk_census(&p).expect("le SDK ouvre le fichier");
            let _ = std::fs::remove_file(&p);
            assert!(
                census["styles des scènes"][0].ends_with("photo=true"),
                "{v:?} : {:?}",
                census["styles des scènes"]
            );
        }
    }
}

/// The hatch patterns of a model (Window › Hatch Patterns: a .pat one whose numbers a float does
/// not hold exactly, an image one) and what uses them (a material, a group, a section plane, the
/// style) go through a 2026 .skp: Esquisse reads them back as written, and writes them again with
/// the same numbers. A version before 2021 has none (SketchUp's conversion drops them too).
#[test]
fn hatch_patterns_go_through_a_2026_file() {
    use esq_core::glam::DVec3;
    use esq_core::meta::HatchPattern;
    use esq_core::model::{AttrValue, EntityRef, Model, SectionPlane};
    let mut m = Model::new();
    let pts = [DVec3::ZERO, DVec3::new(100.0, 0.0, 0.0), DVec3::new(100.0, 100.0, 0.0)];
    m.root.add_polyline(&pts, true, false);
    let edges: Vec<_> = m.root.edges.keys().map(EntityRef::Edge).collect();
    let inst = m.make_group(None, &edges, false, Some("Mur".into())).unwrap();
    let pat = "*Brique\n0, 0, 0, 0, 0.0883883476\n90, 0, 0, 0.0883883476, 0.0883883476, 0.0883883476, -0.0883883476\n";
    let uid = m.new_uid();
    let mut brick = HatchPattern::new(uid, "Brique", "brique.pat", pat.as_bytes().to_vec());
    (brick.color, brick.size, brick.rotation) = ([200, 10, 30], [14.0, 7.5], 30.0);
    let png = {
        let img = image::RgbImage::from_fn(8, 8, |x, y| image::Rgb([(x * 30) as u8, (y * 30) as u8, 90]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    let uid2 = m.new_uid();
    let stone = HatchPattern::new(uid2, "Pierre", "pierre.png", png.clone());
    m.meta.hatch_patterns = vec![brick, stone];
    let concrete = m.add_material("Béton", [128, 128, 128]);
    m.materials[concrete].ext.hatch = Some(uid2);
    m.root.faces.values_mut().for_each(|f| f.front = Some(concrete));
    m.root.instances[inst].ext.hatch = Some("Brique".into());
    let s = m.root.sections.insert(SectionPlane::new(DVec3::ZERO, DVec3::X));
    let skey = |s: esq_core::model::SectionId| format!("s{}", slotmap::Key::data(&s).as_ffi());
    m.root.set_prop(&skey(s), "hatch", AttrValue::Text("Pierre".into()));
    m.view_ext.insert("ro_hatch".into(), serde_json::json!("Brique"));
    // What Esquisse has of them in a model read back.
    let seen = |back: &Model, what: &str| {
        let pats = &back.meta.hatch_patterns;
        let names: Vec<_> = pats.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["Brique", "Pierre"], "{what}");
        let (b, st) = (&pats[0], &pats[1]);
        assert_eq!(
            (b.color, b.size, b.rotation, b.image, b.source.as_str()),
            ([200, 10, 30], [14.0, 7.5], 30.0, false, "brique.pat"),
            "{what}"
        );
        let (_, lines) = esq_core::hatch::parse_pat(&String::from_utf8_lossy(&b.data.0)).unwrap();
        assert_eq!(lines.len(), 2, "{what}");
        assert!((lines[1].dashes[0] - 0.0883883476).abs() < 1e-7 && lines[1].dashes[1] < 0.0, "{what} : {lines:?}");
        assert_eq!((st.image, st.data.0.as_slice()), (true, png.as_slice()), "{what}");
        let mat = back.materials.values().find(|x| x.name == "Béton").unwrap();
        assert_eq!(mat.ext.hatch, Some(st.uid), "{what}");
        let g = back.root.instances.values().next().unwrap();
        assert_eq!(g.ext.hatch.as_deref(), Some("Brique"), "{what}");
        let (sid, _) = back.root.sections.iter().next().unwrap();
        assert_eq!(
            back.root.props.get(&skey(sid)).and_then(|p| p.get("hatch")),
            Some(&AttrValue::Text("Pierre".into())),
            "{what}"
        );
        assert_eq!(back.view_ext.get("ro_hatch"), Some(&serde_json::json!("Brique")), "{what}");
    };
    let mut current = None;
    for v in SkpVersion::ALL.into_iter().filter(|v| v.major() >= 3) {
        let bytes = esq_skp::native::write::write(&m, None, Some(v), &Default::default()).unwrap();
        let (back, _) = esq_skp::native::read_bytes(&bytes).unwrap();
        if v != SkpVersion::Su2021 {
            assert!(back.meta.hatch_patterns.is_empty(), "{v:?}");
            continue;
        }
        seen(&back, "natif");
        // Written again from what was read: the same numbers (those SketchUp's floats hold).
        let again = esq_skp::native::write::write(&back, None, Some(v), &Default::default()).unwrap();
        let (back2, _) = esq_skp::native::read_bytes(&again).unwrap();
        assert_eq!(
            back2.meta.hatch_patterns.iter().map(|h| (&h.name, &h.data)).collect::<Vec<_>>(),
            back.meta.hatch_patterns.iter().map(|h| (&h.name, &h.data)).collect::<Vec<_>>()
        );
        current = Some(bytes);
    }
    // SketchUp's SDK opens the file (it skips what it does not know): it reads and writes no
    // hatch pattern (SketchUpAPI.dll 26.2 drops records 0x211, 0x7388, 0x32cf, 0x1969, 0x4460).
    if esq_skp::available() {
        let dir = std::env::temp_dir();
        let a = dir.join(format!("esq-hachures-{}.skp", std::process::id()));
        let b = dir.join(format!("esq-hachures-{}-sdk.skp", std::process::id()));
        std::fs::write(&a, current.unwrap()).unwrap();
        esq_skp::sdk_resave(&a, &b, None).unwrap();
        let (sdk, _) = esq_skp::native::read_bytes(&std::fs::read(&b).unwrap()).unwrap();
        let _ = (std::fs::remove_file(&a), std::fs::remove_file(&b));
        assert!(sdk.meta.hatch_patterns.is_empty());
        assert_eq!(sdk.root.instances.len(), 1);
    }
}

/// The records of a file SketchUp 2026 made (`Sree.skp` of its Components Sampler, when
/// SketchUp is installed: read only) are written again byte for byte: the hatch pattern
/// settings of its rendering options (0x7388) and its empty hatch pattern manager (0x211).
#[test]
fn hatch_records_are_written_as_sketchup_writes_them() {
    use esq_skp::native::container::{self, Body};
    use esq_skp::native::tlv::Records;
    let path = Path::new(r"C:\ProgramData\SketchUp\SketchUp 2026\SketchUp\Components\Components Sampler\Sree.skp");
    let Ok(orig) = std::fs::read(path) else { return };
    let model_dat = |b: &[u8]| match container::open(b).unwrap().body {
        Body::Zip { archive, .. } => archive.read_name("model.dat").unwrap(),
        Body::Legacy { .. } => panic!("2021+ attendu"),
    };
    // A record of the root (`top`), or of one of its records (`top` then `inner`).
    fn find(d: &[u8], path: &[u16]) -> Option<Vec<u8>> {
        let mut data = d.to_vec();
        for (k, &t) in path.iter().enumerate() {
            let r = Records::new(&data, 0).find(|r| r.tag == t)?;
            if k + 1 == path.len() {
                return Some(r.data.to_vec());
            }
            data = r.data.to_vec();
        }
        None
    }
    let (m, view) = esq_skp::native::read_bytes(&orig).unwrap();
    let ours = esq_skp::native::write::write(&m, view.as_ref(), None, &Default::default()).unwrap();
    let (a, b) = (model_dat(&orig), model_dat(&ours));
    for path in [[0x1f4, 0x1fb, 0x733c, 0x7388].as_slice(), &[0x1f4, 0x211]] {
        let (x, y) = (find(&a, path), find(&b, path));
        assert!(x.is_some(), "{path:x?}");
        assert_eq!(x, y, "{path:x?}");
    }
}

/// Components glued to a face or to another component (`ComponentInstance#glued_to`, SketchUp's
/// relationships) go through a .skp of every version, and through SketchUp's own conversion.
#[test]
fn glued_components_go_through_every_version() {
    use esq_core::glam::DVec3;
    use esq_core::model::{GlueRef, Model};
    let mut m = Model::new();
    let pts =
        [DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, 1000.0, 0.0), DVec3::new(0.0, 1000.0, 0.0)];
    m.root.add_polyline(&pts, true, false);
    let face = m.root.faces.keys().next().expect("une face");
    // A component placed twice: on the face, and on the first one.
    let small = [DVec3::ZERO, DVec3::new(100.0, 0.0, 0.0), DVec3::new(100.0, 100.0, 0.0)];
    let mut inner = esq_core::model::Entities::default();
    inner.add_polyline(&small, true, false);
    let d = m.defs.insert(esq_core::model::Definition {
        name: "Cale".into(),
        is_group: false,
        description: String::new(),
        attributes: Default::default(),
        ext: Default::default(),
        entities: inner,
    });
    let place = |e: &mut esq_core::model::Entities, x: f64| {
        e.instances.insert(esq_core::model::Instance {
            def: d,
            xform: esq_core::glam::DMat4::from_translation(DVec3::new(x, 0.0, 0.0)),
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        })
    };
    let a = place(&mut m.root, 100.0);
    let b = place(&mut m.root, 300.0);
    m.root.instances[a].ext.glued_to = Some(GlueRef::Face(face));
    m.root.instances[b].ext.glued_to = Some(GlueRef::Instance(a));
    let glued = |back: &Model, what: &str| {
        let insts: Vec<_> = back.root.instances.iter().collect();
        assert_eq!(insts.len(), 2, "{what}");
        let to_face = insts
            .iter()
            .filter(|(_, i)| matches!(i.ext.glued_to, Some(GlueRef::Face(f)) if back.root.faces.contains_key(f)))
            .count();
        let to_inst: Vec<_> = insts
            .iter()
            .filter_map(|(k, i)| match i.ext.glued_to {
                Some(GlueRef::Instance(t)) => Some((*k, t)),
                _ => None,
            })
            .collect();
        assert_eq!(to_face, 1, "{what}");
        assert_eq!(to_inst.len(), 1, "{what}");
        assert!(back.root.instances.contains_key(to_inst[0].1) && to_inst[0].1 != to_inst[0].0, "{what}");
    };
    for v in SkpVersion::ALL.into_iter().filter(|v| v.major() >= 3) {
        let bytes = esq_skp::native::write::write(&m, None, Some(v), &Default::default()).unwrap();
        let (back, _) = esq_skp::native::read_bytes(&bytes).unwrap();
        glued(&back, &format!("{v:?}"));
        if esq_skp::available() {
            // The SDK opens it, and its own file keeps them (SketchUp's conversion keeps glue).
            let dir = std::env::temp_dir();
            let (p, q) = (
                dir.join(format!("esq-colle-{}-{}.skp", std::process::id(), v.name())),
                dir.join(format!("esq-colle-{}-{}-sdk.skp", std::process::id(), v.name())),
            );
            std::fs::write(&p, &bytes).unwrap();
            esq_skp::sdk_resave(&p, &q, Some(v)).unwrap();
            let (sdk, _) = esq_skp::native::read_bytes(&std::fs::read(&q).unwrap()).unwrap();
            let _ = (std::fs::remove_file(&p), std::fs::remove_file(&q));
            glued(&sdk, &format!("{v:?} réécrit par le SDK"));
        }
    }
}
