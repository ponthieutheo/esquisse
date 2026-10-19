//! Differential test of the native .skp reader on real files: models saved by the SketchUp
//! application (not made by the SDK), of every version from SketchUp 3, gathered from public
//! sources under a clear licence. The list (source, licence, author, SHA-256, version) is
//! `tests/data/skp-reel.json`; `tools/skp-reel.ps1` downloads it again.
//!
//! Corpus: `ESQ_SKP_REEL` (or `%USERPROFILE%\tools\skp-reel`) and `tests/reel` (the few
//! files whose licence lets them live in the repository). Every file the SDK opens must be read
//! by the native reader exactly as the SDK reads it (the comparison of `native_oracle.rs`),
//! except the differences the manifest lists (`ecart`: what is not understood yet).
//!
//! `real_files_sample_reads_as_the_sdk` (always run): the files of `tests/reel` and the
//! smallest files of each version of the corpus. `all_real_files_read_as_the_sdk` (`--ignored`,
//! best in release): every file, and the counts by family of versions (`docs/skp-format.md`).
//! Skipped without SketchUp or without files. `ESQ_ORACLE_DIFF=<classe>` prints the
//! differences of one class; `ESQ_REEL_LIST=1` the outcome of each file; `ESQ_REEL_FILTER` keeps
//! the files whose path contains it, `ESQ_REEL_FILES` those listed (one per line) in a file.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

mod oracle;

/// Smallest files of each major version the always-run test takes from the corpus.
const SAMPLE_PER_VERSION: usize = 4;

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data")
}

/// The real files kept in the repository (apart from `tests/data`, the corpus made by the SDK).
fn committed_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("reel")
}

/// The folder of the downloaded corpus.
fn corpus_dir() -> Option<PathBuf> {
    std::env::var_os("ESQ_SKP_REEL")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("tools").join("skp-reel")))
}

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x0100_0000_01b3))
}

/// The differences the manifest lists as not understood yet, by file of the corpus (relative
/// path, `/`-separated): the classes that differ (`"ecart":"classes : matières, courbes"`), or
/// `refusé` for a file the native reader refuses.
fn known_differences() -> HashMap<String, Vec<String>> {
    let Ok(text) = std::fs::read_to_string(data_dir().join("skp-reel.json")) else { return HashMap::new() };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { return HashMap::new() };
    let mut out = HashMap::new();
    for f in json["fichiers"].as_array().into_iter().flatten() {
        if let (Some(file), Some(why)) = (f["fichier"].as_str(), f["ecart"].as_str()) {
            let classes = why.strip_prefix("classes :").unwrap_or(why);
            out.insert(file.to_string(), classes.split(',').map(|c| c.trim().to_string()).collect());
        }
    }
    out
}

/// A file of the corpus: path, name relative to the corpus folder, major version, size.
struct Real {
    path: PathBuf,
    rel: String,
    major: u32,
    len: usize,
}

/// The files of `tests/reel` and of the corpus, each content once (the copies in the
/// repository first).
fn corpus() -> Vec<Real> {
    let mut files = Vec::new();
    let roots = std::iter::once(committed_dir()).chain(corpus_dir());
    for root in roots {
        let mut found = Vec::new();
        oracle::walk(&root, &mut found, 0);
        found.sort();
        for p in found {
            let rel = p.strip_prefix(&root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            files.push((p, rel));
        }
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (path, rel) in files {
        let Ok(b) = std::fs::read(&path) else { continue };
        if !seen.insert((b.len(), fnv(&b))) {
            continue;
        }
        let major = esq_skp::native::container::open(&b).map_or(0, |f| f.version.0);
        out.push(Real { path, rel, major, len: b.len() });
    }
    if let Ok(f) = std::env::var("ESQ_REEL_FILTER") {
        out.retain(|r| r.path.to_string_lossy().contains(&f));
    }
    if let Some(list) = std::env::var_os("ESQ_REEL_FILES").and_then(|f| std::fs::read_to_string(f).ok()) {
        let keep: HashSet<&str> = list.lines().map(str::trim).collect();
        out.retain(|r| keep.contains(r.path.to_string_lossy().as_ref()));
    }
    out
}

/// The family of versions of a major version of the header (rank, name).
fn family(major: u32) -> (u8, &'static str) {
    match major {
        0..=7 => (0, "3–7"),
        8 => (1, "8"),
        9..=20 => (2, "2013–2020"),
        _ => (3, "2021+"),
    }
}

/// Compares the files; a class that differs and that the manifest does not list for the file
/// fails the test, and so does a class the manifest lists that no longer differs (the
/// tolerance must then go).
fn check(files: &[&Real], known: &HashMap<String, Vec<String>>, title: &str) {
    let show = std::env::var("ESQ_ORACLE_DIFF").ok();
    let list = std::env::var_os("ESQ_REEL_LIST").is_some();
    let mut tally = oracle::Tally::default();
    let mut unexpected = Vec::new();
    let mut lifted = Vec::new();
    for r in files {
        let t = std::time::Instant::now();
        let out = tally.compare(&r.path, show.as_deref());
        if list {
            println!("{out:?}\t{:.1} s\t{}", t.elapsed().as_secs_f64(), r.path.display());
        }
        let found: Vec<String> = match &out {
            oracle::Outcome::NativeRefused(_) => vec!["refusé".to_string()],
            oracle::Outcome::Differs(classes) => classes.iter().map(|c| c.to_string()).collect(),
            _ => Vec::new(),
        };
        let listed = known.get(&r.rel).cloned().unwrap_or_default();
        if found.iter().any(|c| !listed.contains(c)) {
            unexpected.push(format!("{} : {out:?}", r.path.display()));
        }
        if !matches!(out, oracle::Outcome::SdkRefused) {
            let gone: Vec<&String> = listed.iter().filter(|c| !found.contains(c)).collect();
            if !gone.is_empty() {
                lifted.push(format!("{} : {gone:?}", r.rel));
            }
        }
    }
    tally.print(title);
    let mut fam: BTreeMap<(u8, &str), (usize, usize, usize)> = BTreeMap::new();
    for (v, (o, r, i)) in &tally.per_version {
        let e = fam.entry(family(*v)).or_default();
        e.0 += o;
        e.1 += r;
        e.2 += i;
    }
    println!("  par famille (identiques / ouverts par le SDK, lus) :");
    for ((_, f), (o, r, i)) in &fam {
        println!("    {f:<10} {i}/{o} (lus {r})");
    }
    assert!(unexpected.is_empty(), "différences avec le SDK :\n{}", unexpected.join("\n"));
    assert!(lifted.is_empty(), "écarts levés, à retirer du manifeste (skp-reel.json) :\n{}", lifted.join("\n"));
}

#[test]
fn real_files_sample_reads_as_the_sdk() {
    if !esq_skp::available() {
        eprintln!("SketchUp absent : test ignoré");
        return;
    }
    let files = corpus();
    let known = known_differences();
    let committed = committed_dir();
    let mut by_version: BTreeMap<u32, Vec<&Real>> = BTreeMap::new();
    let mut sample: Vec<&Real> = Vec::new();
    for r in &files {
        if r.path.starts_with(&committed) {
            sample.push(r);
        } else if !known.contains_key(&r.rel) {
            by_version.entry(r.major).or_default().push(r);
        }
    }
    for list in by_version.values_mut() {
        list.sort_by_key(|r| r.len);
        sample.extend(list.iter().take(SAMPLE_PER_VERSION));
    }
    if sample.is_empty() {
        eprintln!("corpus réel absent : test ignoré");
        return;
    }
    check(&sample, &known, "Lecteur natif — fichiers réels (échantillon)");
}

#[test]
#[ignore]
fn all_real_files_read_as_the_sdk() {
    if !esq_skp::available() {
        eprintln!("SketchUp absent : test ignoré");
        return;
    }
    let files = corpus();
    if files.is_empty() {
        eprintln!("corpus réel absent : test ignoré");
        return;
    }
    let all: Vec<&Real> = files.iter().collect();
    check(&all, &known_differences(), "Lecteur natif — fichiers réels");
}

/// A text and a dimension whose font the model's font manager does not list
/// (`legacy_annotations.skp`, SketchUp 8): SketchUp adds the font to the model's
/// (`SUModelGetFonts` gives it after the listed ones) and the annotations keep it — Tahoma 8.
/// Lost from 9e030ca (fonts of the annotations read) until the reader added those fonts.
#[test]
fn annotation_fonts_the_model_does_not_list_are_kept() {
    let Some(p) = corpus_dir().map(|d| d.join("iamahsanmehmood_openskp").join("64ff0b19_legacy_annotations.skp"))
    else {
        return;
    };
    let Ok(b) = std::fs::read(&p) else {
        eprintln!("{} absent : test ignoré", p.display());
        return;
    };
    let d = esq_skp::native::read_doc(&b, &Default::default()).unwrap();
    let used: Vec<esq_skp::native::doc::Id> =
        d.root.texts.iter().map(|t| t.font).chain(d.root.dims.iter().map(|x| x.font)).flatten().collect();
    assert_eq!(used.len(), 2, "un texte et une cote avec leur police");
    for f in &used {
        let font = d.fonts.iter().find(|x| x.id == *f).expect("police dans celles du modèle");
        assert_eq!((font.face.as_str(), font.size), ("Tahoma", 8));
    }
    if esq_skp::available() {
        let out = oracle::Tally::default().compare(&p, None);
        assert!(matches!(out, oracle::Outcome::Identical), "{out:?}");
    }
}
