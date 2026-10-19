//! What the .skp files of a folder hold, by class: for each class, the files that have it and
//! the objects in all (MFC archives: the objects of the archive; 2021+: the class records of
//! `model.dat`). `--files classe` lists the files that have one.
//!
//! `cargo run -p esq-skp --example skp_classes -- dossier [--files CSkpStyle]`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use esq_skp::native::{container, legacy, vff};

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() && depth < 8 {
            walk(&p, out, depth + 1);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("skp")) {
            out.push(p);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(args.first().expect("dossier"));
    let list_of = args.iter().position(|a| a == "--files").and_then(|i| args.get(i + 1)).cloned();
    let mut files = Vec::new();
    walk(&dir, &mut files, 0);
    files.sort();
    // class → (files, objects), for MFC and 2021+.
    let mut mfc: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut zip: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let (mut n_mfc, mut n_zip) = (0, 0);
    for p in &files {
        let Ok(data) = std::fs::read(p) else { continue };
        let Ok(f) = container::open(&data) else { continue };
        let counts: BTreeMap<String, usize> = match &f.body {
            container::Body::Zip { archive, .. } => {
                let Ok(model) = archive.read_name("model.dat") else { continue };
                let mut met = BTreeMap::new();
                vff::class_census(&model, &mut met);
                n_zip += 1;
                met.into_iter()
                    .map(|(t, n)| {
                        let name = vff::CLASSES.iter().find(|c| c.0 == t).map_or("?", |c| c.1);
                        (format!("{name} {t:#06x}"), n)
                    })
                    .collect()
            }
            container::Body::Legacy { guid, rest } => {
                let Ok(c) = legacy::object_census(rest, guid, f.version) else { continue };
                n_mfc += 1;
                c
            }
        };
        let table = if matches!(f.body, container::Body::Zip { .. }) { &mut zip } else { &mut mfc };
        for (k, n) in counts {
            if list_of.as_deref().is_some_and(|c| k.starts_with(c)) {
                println!("{n:>6} {k} {}", p.display());
            }
            let e = table.entry(k).or_default();
            e.0 += 1;
            e.1 += n;
        }
    }
    if list_of.is_some() {
        return;
    }
    println!("Archives MFC : {n_mfc} fichiers (classe : fichiers, objets)");
    for (k, (f, n)) in &mfc {
        println!("  {k:<28} {f:>5} {n:>9}");
    }
    println!("2021+ : {n_zip} fichiers");
    for (k, (f, n)) in &zip {
        println!("  {k:<36} {f:>5} {n:>9}");
    }
}
