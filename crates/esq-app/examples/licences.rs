//! The licences of the Rust libraries built into `esquisse.exe`, for `tools/package.ps1`:
//! `cargo run -p esq-app --example licences -- <fichier>`.
//!
//! Without network: the list comes from `cargo metadata --offline` (the packages esq-app
//! depends on for Windows x64, following normal dependencies only — build scripts' and tests'
//! dependencies are not in the executable), the texts from each package's sources that Cargo
//! already downloaded (`LICENSE*`, `COPYING*`, `NOTICE*`, `UNLICENSE*`). Identical texts are
//! written once with the packages that carry them.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn main() {
    let out = std::env::args().nth(1).expect("fichier de sortie");
    let meta = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--offline", "--filter-platform", "x86_64-pc-windows-msvc"])
        .output()
        .expect("cargo metadata");
    assert!(meta.status.success(), "cargo metadata : {}", String::from_utf8_lossy(&meta.stderr));
    let m: Value = serde_json::from_slice(&meta.stdout).expect("JSON de cargo metadata");
    let packages: BTreeMap<&str, &Value> =
        m["packages"].as_array().unwrap().iter().map(|p| (p["id"].as_str().unwrap(), p)).collect();
    let members: BTreeSet<&str> = m["workspace_members"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    let nodes: BTreeMap<&str, &Value> =
        m["resolve"]["nodes"].as_array().unwrap().iter().map(|n| (n["id"].as_str().unwrap(), n)).collect();
    let root = packages.values().find(|p| p["name"] == "esq-app").expect("esq-app")["id"].as_str().unwrap();
    // Normal dependencies reachable from esq-app.
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([root]);
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        for d in nodes[id]["deps"].as_array().unwrap() {
            let normal = d["dep_kinds"].as_array().unwrap().iter().any(|k| k["kind"].is_null());
            if normal {
                queue.push_back(d["pkg"].as_str().unwrap());
            }
        }
    }
    // Text → the packages that carry it.
    let mut texts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut list = Vec::new();
    for id in &seen {
        if members.contains(id) {
            continue;
        }
        let p = packages[id];
        let name = format!("{} {}", p["name"].as_str().unwrap(), p["version"].as_str().unwrap());
        let licence = p["license"].as_str().unwrap_or("(voir les fichiers)");
        let repo = p["repository"].as_str().unwrap_or("");
        list.push(format!("{name:<40} {licence:<45} {repo}"));
        let dir = Path::new(p["manifest_path"].as_str().unwrap()).parent().unwrap().to_path_buf();
        let mut found = false;
        let mut files: Vec<_> =
            std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        files.sort();
        for f in files {
            let n = f.file_name().unwrap().to_string_lossy().to_uppercase();
            let is_licence = ["LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE"].iter().any(|k| n.starts_with(k));
            if f.is_file()
                && is_licence
                && let Ok(t) = std::fs::read_to_string(&f)
            {
                texts
                    .entry(t.trim().replace("\r\n", "\n"))
                    .or_default()
                    .push(format!("{name} ({})", f.file_name().unwrap().to_string_lossy()));
                found = true;
            }
        }
        if !found {
            texts
                .entry(format!("(Pas de fichier de licence dans les sources ; licence déclarée : {licence}.)"))
                .or_default()
                .push(name);
        }
    }
    let mut s = String::new();
    s.push_str("Bibliothèques Rust compilées dans esquisse.exe\r\n");
    s.push_str("===============================================\r\n\r\n");
    s.push_str("Liste tirée de `cargo metadata --offline` (dépendances normales de esq-app pour Windows x64),\r\n");
    s.push_str("textes copiés des sources de chaque bibliothèque. Esquisse lui-même est sous licence MIT (LICENCE.txt).\r\n\r\n");
    s.push_str(&format!("{} bibliothèques :\r\n\r\n", list.len()));
    for l in &list {
        s.push_str(l.trim_end());
        s.push_str("\r\n");
    }
    s.push_str("\r\n\r\nTextes des licences\r\n===================\r\n");
    for (text, who) in &texts {
        s.push_str("\r\n------------------------------------------------------------------------------\r\n");
        s.push_str(&format!("Pour : {}\r\n\r\n", who.join(", ")));
        s.push_str(&text.replace('\n', "\r\n"));
        s.push_str("\r\n");
    }
    std::fs::write(&out, s).expect("écriture");
    println!("{} bibliothèques, {} textes de licence distincts -> {out}", list.len(), texts.len());
}
