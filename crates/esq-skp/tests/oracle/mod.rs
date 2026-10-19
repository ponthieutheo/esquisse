//! The comparison of the native reader with the SDK reader (SketchUpAPI.dll), shared by the
//! tests that run it over a corpus: `native_oracle.rs` (files made by the SDK) and
//! `native_reel.rs` (files saved by SketchUp itself).

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use esq_skp::native::canon::{self, CLASSES};

/// The .skp files under `dir` (8 levels deep at most).
pub fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
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

/// What the comparison of one file gave.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// The SDK does not open it: nothing to compare.
    SdkRefused,
    /// The SDK opens it, the native reader refuses it.
    NativeRefused(String),
    /// Both read it; these classes differ.
    Differs(Vec<&'static str>),
    Identical,
}

/// Counts over a corpus.
#[derive(Default)]
pub struct Tally {
    pub files: usize,
    pub sdk_ok: usize,
    pub read: usize,
    pub identical: usize,
    /// Per class: files equal, files compared.
    pub per_class: BTreeMap<&'static str, (usize, usize)>,
    /// Per major version of SketchUp (header): opened by the SDK, read, identical.
    pub per_version: BTreeMap<u32, (usize, usize, usize)>,
    /// « file : class » for each class that differs.
    pub failures: Vec<String>,
    /// « file : error » for each file the SDK opens and the native reader refuses.
    pub refused: Vec<String>,
    /// Files whose texture maps agree only to the rounding of doubles ([`texture_lines_close`]).
    pub textures_rounded: usize,
}

impl Tally {
    /// Compares the SDK's and the native reader's models of `path`. `show`: classes whose
    /// differences are printed (`ESQ_ORACLE_DIFF`, comma-separated).
    pub fn compare(&mut self, path: &Path, show: Option<&str>) -> Outcome {
        self.files += 1;
        let Ok((sdk, sdk_view)) = esq_skp::sdk_import_skp_view(path) else { return Outcome::SdkRefused };
        self.sdk_ok += 1;
        let bytes = std::fs::read(path).unwrap_or_default();
        let major = esq_skp::native::container::open(&bytes).map_or(0, |f| f.version.0);
        let pv = self.per_version.entry(major).or_default();
        pv.0 += 1;
        let opts = esq_skp::native::Options { sdk_quirks: true };
        let native = match esq_skp::native::read_bytes_with(&bytes, &opts) {
            Ok(x) => x,
            Err(e) => {
                if show.is_some() {
                    eprintln!("{} : {e:#}", path.display());
                }
                self.refused.push(format!("{} : {e}", path.display()));
                return Outcome::NativeRefused(e.to_string());
            }
        };
        self.read += 1;
        pv.1 += 1;
        let (a, b) = (canon::canon(&sdk), canon::canon(&native.0));
        let mut differ = Vec::new();
        for class in CLASSES {
            let (x, y) = (&a[class], &b[class]);
            let e = self.per_class.entry(class).or_default();
            e.1 += 1;
            let rounded = x != y && *class == "textures de faces" && texture_lines_close(x, y);
            self.textures_rounded += rounded as usize;
            if x == y || rounded {
                e.0 += 1;
                continue;
            }
            differ.push(*class);
            self.failures.push(format!("{} : {class}", path.display()));
            if show.is_some_and(|s| s.split(',').any(|x| x == *class)) {
                print_diff(path, class, x, y);
            }
        }
        let e = self.per_class.entry("caméra").or_default();
        e.1 += 1;
        let (va, vb) = (canon::view(&sdk_view), canon::view(&native.1));
        if va == vb {
            e.0 += 1;
        } else {
            differ.push("caméra");
            self.failures.push(format!("{} : caméra", path.display()));
            if show.is_some_and(|s| s.split(',').any(|x| x == "caméra")) {
                eprintln!("== {} caméra SDK {va} natif {vb}", path.display());
            }
        }
        if differ.is_empty() {
            self.identical += 1;
            self.per_version.entry(major).or_default().2 += 1;
            Outcome::Identical
        } else {
            Outcome::Differs(differ)
        }
    }

    /// The counts, by version and by class.
    pub fn print(&self, title: &str) {
        println!("{title} : {} fichiers, {} ouverts par le SDK", self.files, self.sdk_ok);
        println!(
            "  lus nativement : {}/{}, identiques à l'oracle : {}/{}",
            self.read, self.sdk_ok, self.identical, self.sdk_ok
        );
        println!("  textures de faces égales à l'arrondi des doubles près : {}", self.textures_rounded);
        println!("  par version (ouverts SDK / lus / identiques) :");
        for (v, (o, r, i)) in &self.per_version {
            println!("    {v:>4} : {o} / {r} / {i}");
        }
        println!("  par classe (fichiers identiques / comparés) :");
        for (c, (ok, n)) in &self.per_class {
            println!("    {c:<20} {ok}/{n}");
        }
    }
}

/// Texture maps read through SketchUp's UV helper and computed from the file agree to the
/// rounding of doubles, which a map far from its texture origin (tens of thousands of tiles)
/// shows in the ninth decimal (`docs/skp-format.md` §9): the lines must be the same but for
/// numbers within 2.5e-9 + 1e-9 of their size.
fn texture_lines_close(x: &[String], y: &[String]) -> bool {
    let close = |a: &str, b: &str| {
        let (pa, pb): (Vec<&str>, Vec<&str>) = (a.split('"').collect(), b.split('"').collect());
        pa.len() == pb.len()
            && pa.iter().zip(&pb).enumerate().all(|(i, (s, t))| {
                if s == t {
                    return true;
                }
                // Odd pieces are inside quotes: the numbers of the rows.
                match (i % 2 == 1, s.parse::<f64>(), t.parse::<f64>()) {
                    (true, Ok(p), Ok(q)) => (p - q).abs() <= 2.5e-9 + 1e-9 * p.abs().max(q.abs()),
                    _ => false,
                }
            })
    };
    x.len() == y.len() && x.iter().zip(y).all(|(a, b)| close(a, b))
}

/// The lines of one class that differ between the SDK and the native reader, paired, from the
/// first character that differs.
fn print_diff(path: &Path, class: &str, x: &[String], y: &[String]) {
    let (only_sdk, only_native) = canon::diff(x, y);
    eprintln!("== {} ({class})", path.display());
    let cut = |a: &str, b: &str| -> usize {
        let k = a.chars().zip(b.chars()).take_while(|(p, q)| p == q).count();
        a.char_indices().nth(k.saturating_sub(20)).map_or(0, |(i, _)| i)
    };
    // `ESQ_ORACLE_FULL=1`: whole lines.
    let full = std::env::var_os("ESQ_ORACLE_FULL").is_some();
    let cut = |a: &str, b: &str| if full { 0 } else { cut(a, b) };
    let short = |s: &str, at: usize| -> String {
        s[at.min(s.len())..].chars().take(if full { usize::MAX } else { 240 }).collect()
    };
    for i in 0..only_sdk.len().max(only_native.len()).min(8) {
        let (a, b) = (only_sdk.get(i).map_or("", |s| s), only_native.get(i).map_or("", |s| s));
        let k = cut(a, b);
        let kb = b.char_indices().nth(a[..k].chars().count()).map_or(b.len(), |(i, _)| i);
        eprintln!("  SDK    …{}", short(a, k));
        eprintln!("  natif  …{}", short(b, kb));
    }
}
