//! The value of a record path of `model.dat` in each 2021+ .skp of a folder (format study):
//! `cargo run -p esq-skp --example vff_get -- dossier 206/6978/697c` prints, per file, the
//! records found at that path (tags in hex, `*` matches any tag).

use std::path::{Path, PathBuf};

use esq_skp::native::container::{self, Body};
use esq_skp::native::tlv::{Records, is_records};

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

fn find<'a>(d: &'a [u8], path: &[Option<u16>], out: &mut Vec<&'a [u8]>) {
    let Some((first, rest)) = path.split_first() else { return };
    for r in Records::new(d, 0) {
        if first.is_none_or(|t| t == r.tag) {
            if rest.is_empty() {
                out.push(r.data);
            } else if is_records(r.data) {
                find(r.data, rest, out);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(&args[0]);
    let path: Vec<Option<u16>> =
        args[1].split('/').map(|t| if t == "*" { None } else { u16::from_str_radix(t, 16).ok() }).collect();
    let mut files = Vec::new();
    walk(&dir, &mut files, 0);
    files.sort();
    for p in files {
        let Ok(data) = std::fs::read(&p) else { continue };
        let Ok(f) = container::open(&data) else { continue };
        let Body::Zip { archive, .. } = &f.body else { continue };
        let Ok(model) = archive.read_name("model.dat") else { continue };
        let mut found = Vec::new();
        let mut full = vec![Some(0x1f4)];
        full.extend(path.iter().copied());
        find(&model, &full, &mut found);
        let vals: Vec<String> = found
            .iter()
            .map(|d| {
                let hex: String = d.iter().take(24).map(|b| format!("{b:02x}")).collect();
                if d.len() > 1 && d.iter().all(|&b| (0x20..0x7f).contains(&b)) {
                    format!("'{}'", String::from_utf8_lossy(d))
                } else {
                    hex
                }
            })
            .collect();
        println!("{}\t{}", vals.join(" "), p.display());
    }
}
