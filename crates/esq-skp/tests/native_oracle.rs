//! Differential test of the native .skp reader: on every .skp of the corpus, the model it reads
//! is compared, class by class, with the one the SDK reader (SketchUpAPI.dll) reads.
//!
//! Corpus: `ESQ_SKP_CORPUS` (or `%USERPROFILE%\tools\skp-corpus`, made by the `skp_corpus`
//! example), the .skp files shipped with SketchUp, and `tests/data`. Skipped without SketchUp
//! or without files. `cargo test -p esq-skp --test native_oracle -- --nocapture` prints the
//! coverage; `ESQ_ORACLE_DIFF=<classe>` prints the differences of one class.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod oracle;
use oracle::walk;

fn corpus() -> Vec<PathBuf> {
    let mut files = Vec::new();
    let dir = std::env::var_os("ESQ_SKP_CORPUS")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("tools").join("skp-corpus")));
    if let Some(d) = dir {
        walk(&d, &mut files, 0);
    }
    if let Some(dll) = esq_skp::find_sketchup_api()
        && let Some(su) = dll.parent()
    {
        walk(su, &mut files, 0);
    }
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data"), &mut files, 0);
    files.sort();
    files.dedup();
    files
}

#[test]
fn native_reader_matches_the_sdk() {
    if !esq_skp::available() {
        eprintln!("SketchUp absent : test ignoré");
        return;
    }
    let files = corpus();
    if files.is_empty() {
        eprintln!("corpus absent : test ignoré");
        return;
    }
    let show = std::env::var("ESQ_ORACLE_DIFF").ok();
    let mut tally = oracle::Tally::default();
    for path in &files {
        tally.compare(path, show.as_deref());
    }
    tally.print("Lecteur natif — corpus");
    // Every class of every file the native reader opens, as the SDK reads it.
    assert!(tally.failures.is_empty(), "classes différentes de l'oracle :\n{}", tally.failures.join("\n"));
}

/// The classes of the file format met in the corpus, and those the native reader reads
/// (`-- --nocapture` prints them). Needs the corpus only.
#[test]
fn class_coverage() {
    use esq_skp::native::{container, legacy, vff};
    let files = corpus();
    let mut met: BTreeMap<u16, usize> = BTreeMap::new();
    let mut zipped = 0;
    // MFC archives: the classes of their version maps.
    let mut mfc_met: BTreeMap<String, usize> = BTreeMap::new();
    let mut archives = 0;
    for path in &files {
        let Ok(data) = std::fs::read(path) else { continue };
        let Ok(f) = container::open(&data) else { continue };
        match &f.body {
            container::Body::Zip { archive, .. } => {
                if let Ok(model) = archive.read_name("model.dat") {
                    vff::class_census(&model, &mut met);
                    zipped += 1;
                }
            }
            container::Body::Legacy { rest, .. } => {
                if let Ok((_, map)) = legacy::header(rest) {
                    for (class, _) in map {
                        *mfc_met.entry(class).or_default() += 1;
                    }
                    archives += 1;
                }
            }
        }
    }
    if !mfc_met.is_empty() {
        let known = |c: &str| legacy::CLASSES.iter().find(|k| k.0 == c);
        let parsed = mfc_met.keys().filter(|c| known(c).is_some()).count();
        let kept = mfc_met.keys().filter(|c| known(c).is_some_and(|k| k.1)).count();
        println!(
            "Classes des archives MFC 3–2020 ({archives} fichiers) : parcourues {parsed} / rencontrées {}, dont {kept} dont les données sont lues",
            mfc_met.len()
        );
        for (c, n) in &mfc_met {
            match known(c) {
                None => println!("    inconnue : {c} ({n} fichiers)"),
                Some((_, false)) => println!("    parcourue sans être gardée : {c} ({n} fichiers)"),
                _ => {}
            }
        }
        assert_eq!(parsed, mfc_met.len(), "classes que le lecteur MFC ne sait pas parcourir");
    }
    if met.is_empty() {
        eprintln!("corpus absent : test ignoré");
        return;
    }
    let name = |t: u16| vff::CLASSES.iter().find(|c| c.0 == t).map_or("?", |c| c.1);
    let read: Vec<u16> = met.keys().copied().filter(|t| vff::CLASSES.iter().any(|c| c.0 == *t && c.2)).collect();
    println!("Classes du format 2021+ ({zipped} fichiers) : lues {} / rencontrées {}", read.len(), met.len());
    for (t, n) in &met {
        if !read.contains(t) {
            println!("    non lue : {t:#06x} {} ({n} fois)", name(*t));
        }
    }
}
