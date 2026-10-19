//! The native writer next to the SDK writer (format study): for each .skp given (or found in
//! the folders given), the model read natively is written by both in a version, then read back
//! by the SDK and by the native reader; the classes that differ are shown.
//!
//! `cargo run -p esq-skp --example skp_wcmp -- [--version 2020] [--out dir] [--show classe,…] files…`

use std::collections::BTreeMap;
use std::path::PathBuf;

use esq_skp::SkpVersion;
use esq_skp::native::canon;

fn files(args: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                stack.extend(rd.flatten().map(|e| e.path()));
            }
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("skp")) {
            out.push(p);
        }
    }
    out.sort();
    out
}

fn show_diff(
    label: &str,
    a: &BTreeMap<&str, Vec<String>>,
    b: &BTreeMap<&str, Vec<String>>,
    show: &[String],
) -> Vec<String> {
    let mut bad = Vec::new();
    for class in canon::CLASSES {
        if a[class] != b[class] {
            bad.push(class.to_string());
            if show.iter().any(|s| s == class || s == "all") {
                let (x, y) = canon::diff(&a[class], &b[class]);
                eprintln!("   {label} {class}:");
                for l in x.iter().take(6) {
                    eprintln!("      - {}", tail(l));
                }
                for l in y.iter().take(6) {
                    eprintln!("      + {}", tail(l));
                }
            }
        }
    }
    bad
}

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut take = |flag: &str| -> Option<String> {
        let i = args.iter().position(|a| a == flag)?;
        let v = args.get(i + 1).cloned();
        args.drain(i..(i + 2).min(args.len()));
        v
    };
    let version = take("--version")
        .map(|v| SkpVersion::ALL.into_iter().find(|x| x.name() == v).unwrap_or_else(|| panic!("version {v} inconnue")));
    let out = take("--out").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("esq-wcmp"));
    let show: Vec<String> = take("--show").map(|s| s.split(',').map(String::from).collect()).unwrap_or_default();
    std::fs::create_dir_all(&out)?;
    let list = files(&args);
    let (mut same_sdk, mut same_native, mut n) = (0, 0, 0);
    for f in &list {
        let data = std::fs::read(f)?;
        let Ok((m, view)) = esq_skp::native::read_bytes(&data) else {
            println!("{} : illisible", f.display());
            continue;
        };
        n += 1;
        let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or("x");
        let a = out.join(format!("{stem}.sdk.skp"));
        let b = out.join(format!("{stem}.natif.skp"));
        let sdk_ok = esq_skp::sdk_export_skp_version(&m, &a, view.as_ref(), version).is_ok();
        let bytes = match esq_skp::native::write::write(&m, view.as_ref(), version, &Default::default()) {
            Ok(x) => x,
            Err(e) => {
                println!("{} : écriture native impossible : {e}", f.display());
                continue;
            }
        };
        std::fs::write(&b, &bytes)?;
        let mut line = format!("{stem:<28}");
        // The structure of the two files: every path of record tags, with how many times.
        if sdk_ok && version.is_none_or(|v| v == SkpVersion::Su2021) {
            let pa = paths(&std::fs::read(&a)?);
            let pb = paths(&bytes);
            let mut d = Vec::new();
            for (k, n) in &pa {
                let m = pb.get(k).copied().unwrap_or(0);
                if *n != m {
                    d.push(format!("{k} {n}≠{m}"));
                }
            }
            for (k, m) in &pb {
                if !pa.contains_key(k) {
                    d.push(format!("{k} 0≠{m}"));
                }
            }
            if !d.is_empty() {
                line.push_str(&format!(" structure≠{}", d.len()));
                if show.iter().any(|s| s == "structure") {
                    for x in d.iter().take(30) {
                        eprintln!("      {x}");
                    }
                }
            }
        }
        // Read back by the SDK, both files.
        if sdk_ok {
            match (esq_skp::sdk_import_skp_view(&a), esq_skp::sdk_import_skp_view(&b)) {
                (Ok((ma, _)), Ok((mb, _))) => {
                    let mut ca = canon::canon(&ma);
                    let mut cb = canon::canon(&mb);
                    for c in [&mut ca, &mut cb] {
                        if let Some(v) = c.get_mut("modèle") {
                            v.iter_mut().for_each(|l| *l = mask_guid(l));
                        }
                    }
                    let bad = show_diff("SDK", &ca, &cb, &show);
                    if bad.is_empty() {
                        same_sdk += 1;
                        line.push_str(" SDK=");
                    } else {
                        line.push_str(&format!(" SDK≠{bad:?}"));
                    }
                }
                (_, Err(e)) => line.push_str(&format!(" SDK n'ouvre pas : {e:#}")),
                (Err(e), _) => line.push_str(&format!(" (écrivain du SDK : {e:#})")),
            }
        }
        // Read back natively: the SDK's file and the native one.
        if sdk_ok
            && let (Ok((ma, _)), Ok((mb, _))) =
                (esq_skp::native::read_bytes(&std::fs::read(&a)?), esq_skp::native::read_bytes(&bytes))
        {
            let mut ca = canon::canon(&ma);
            let mut cb = canon::canon(&mb);
            for c in [&mut ca, &mut cb] {
                if let Some(v) = c.get_mut("modèle") {
                    v.iter_mut().for_each(|l| *l = mask_guid(l));
                }
            }
            let bad = show_diff("natif/natif", &ca, &cb, &show);
            if bad.is_empty() {
                line.push_str(" nn=");
            } else {
                line.push_str(&format!(" nn≠{bad:?}"));
            }
        }
        // Read back natively: the model it came from.
        match esq_skp::native::read_bytes(&bytes) {
            Ok((mb, _)) => {
                let mut ca = canon::canon(&m);
                let mut cb = canon::canon(&mb);
                for c in [&mut ca, &mut cb] {
                    if let Some(v) = c.get_mut("modèle") {
                        v.iter_mut().for_each(|l| *l = mask_guid(l));
                    }
                }
                let bad = show_diff("natif", &ca, &cb, &show);
                if bad.is_empty() {
                    same_native += 1;
                    line.push_str(" natif=");
                } else {
                    line.push_str(&format!(" natif≠{bad:?}"));
                }
            }
            Err(e) => line.push_str(&format!(" relecture native : {e}")),
        }
        println!("{line}");
    }
    println!(
        "{n} fichiers : relus par le SDK comme l'écrivain du SDK {same_sdk}, relus nativement à l'identique {same_native}"
    );
    Ok(())
}

/// Every path of class and list records of a 2021+ file (`model.dat`, `meta/meta.dat`), with
/// the number of times it is met, and the archive's entries by kind.
fn paths(data: &[u8]) -> BTreeMap<String, usize> {
    use esq_skp::native::container::{self, Body};
    use esq_skp::native::tlv::{Records, is_records};
    fn walk(d: &[u8], at: &str, depth: usize, out: &mut BTreeMap<String, usize>) {
        for r in Records::new(d, 0) {
            let p = format!("{at}/{:04x}", r.tag);
            *out.entry(p.clone()).or_default() += 1;
            // Values that happen to read as records are left alone: only containers recurse.
            if depth < 40 && is_records(r.data) && r.tag != 0x09c5 && r.data.len() != 24 {
                walk(r.data, &p, depth + 1, out);
            }
        }
    }
    let mut out = BTreeMap::new();
    let Ok(f) = container::open(data) else { return out };
    if let Body::Zip { archive, .. } = &f.body {
        for e in &archive.entries {
            let kind = e.name.rsplit_once('/').map_or(e.name.as_str(), |(_, n)| n);
            let kind = if e.name.starts_with("materials/") && kind != "material.xml" { "image" } else { kind };
            *out.entry(format!("zip:{kind}")).or_default() += 1;
        }
        for name in ["model.dat", "meta/meta.dat"] {
            if let Ok(d) = archive.read_name(name) {
                walk(&d, name, 0, &mut out);
            }
        }
    }
    out
}

/// The end of a long line (where face lines keep their texture maps).
fn tail(l: &str) -> String {
    let n = l.chars().count();
    if n <= 400 {
        l.to_string()
    } else {
        format!("{}…{}", l.chars().take(60).collect::<String>(), l.chars().skip(n - 330).collect::<String>())
    }
}

fn mask_guid(l: &str) -> String {
    match l.find("guid=") {
        Some(i) => {
            let rest = &l[i..];
            let end = rest.find(" axes=").unwrap_or(rest.len());
            format!("{}guid=…{}", &l[..i], &rest[end..])
        }
        None => l.to_string(),
    }
}
