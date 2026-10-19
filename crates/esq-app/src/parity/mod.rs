//! Parity with SketchUp 2026: every command, tool, file format, panel and settings page that
//! SketchUp has (read from its install: `resources/fr/commands.json`, the `C…Tool` / `…Page` /
//! `…Importer` classes of SketchUp.exe, the shipped importers and exporters), and what does
//! the same thing in Esquisse. `cargo test -p esq-app parity -- --nocapture` measures it;
//! `ESQ_WRITE_PARITY=1` also writes `docs/sketchup-parity.md`.
//!
//! One table per domain. An entry turns from
//! `Missing` to `Cmd`/`Tool`/`Panel`/`Feature` only when the function works like SketchUp's
//! and has a test.

use crate::commands::{Cmd, Panel};
use crate::tools::ToolId;

mod commands;
mod files;
mod panels;
mod tools;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub enum Impl {
    Cmd(Cmd),
    Tool(ToolId),
    Panel(Panel),
    /// Done outside the command system: names the module/function (and its test).
    Feature(&'static str),
    /// Not in Esquisse yet.
    Missing,
    /// In Esquisse but hollow: present without acting as in SketchUp,
    /// with what is wrong. Counted as missing until fixed.
    Partial(&'static str),
    /// A Trimble online service (cloud, comments, AI, 3D Warehouse…) whose Esquisse version, on
    /// servers the user chooses (`services/`), is not there yet. Counted as missing.
    Online,
}

pub struct Item {
    pub id: &'static str,
    pub label: &'static str,
    pub imp: Impl,
}

pub const DOMAINS: &[(&str, &[Item])] = &[
    ("Commandes (commands.json)", commands::TABLE),
    ("Outils (classes C…Tool)", tools::TABLE),
    ("Fichiers (import / export)", files::TABLE),
    ("Panneaux, Infos sur le modèle, Préférences", panels::TABLE),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parity() {
        let mut seen = std::collections::HashSet::new();
        for (_, t) in DOMAINS {
            for it in *t {
                assert!(seen.insert(it.id), "entrée en double : {}", it.id);
            }
        }
        // Every SketchUp command is in the table.
        let tsv = include_str!("../../../../docs/sketchup-commands.tsv");
        for line in tsv.lines().filter(|l| !l.is_empty()) {
            let key = line.split('\t').next().unwrap();
            assert!(seen.contains(format!("cmd:{key}").as_str()), "commande SketchUp absente de la table : {key}");
        }
        let (mut total, mut done, mut online) = (0, 0, 0);
        let mut report = String::from(
            "# Parité avec SketchUp 2026\n\nGénéré par `ESQ_WRITE_PARITY=1 cargo test -p esq-app parity`.\n",
        );
        let mut per = Vec::new();
        for (name, t) in DOMAINS {
            let (mut d_total, mut d_done, mut d_online) = (0, 0, 0);
            let mut missing = Vec::new();
            for it in *t {
                d_total += 1;
                match it.imp {
                    Impl::Missing => missing.push(format!("`{}` {}", it.id, it.label)),
                    Impl::Partial(why) => missing.push(format!("`{}` {} (creux : {why})", it.id, it.label)),
                    Impl::Online => {
                        d_online += 1;
                        missing.push(format!("`{}` {} (service en ligne à refaire)", it.id, it.label));
                    }
                    _ => d_done += 1,
                }
            }
            total += d_total;
            done += d_done;
            online += d_online;
            per.push(format!("{name} : {d_done}/{d_total} (dont {d_online} services en ligne à refaire)"));
            report.push_str(&format!("\n## {name} — {d_done}/{d_total}\n\n"));
            for m in missing {
                report.push_str(&format!("- {m}\n"));
            }
        }
        let pct = 100.0 * done as f64 / total as f64;
        eprintln!("Parité SketchUp : {done}/{total} ({pct:.1} %), {online} services en ligne encore à refaire");
        for p in &per {
            eprintln!("  {p}");
        }
        if std::env::var("ESQ_WRITE_PARITY").is_ok() {
            let head = format!(
                "\n**{done}/{total} ({pct:.1} %)**, dont {online} services en ligne de Trimble encore à refaire \
                 avec les services d'Esquisse.\n"
            );
            report.insert_str(report.find('\n').unwrap() + 1, &head);
            std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/sketchup-parity.md"), report).unwrap();
        }
        // Floor: parity must not go down (raise it as domains are completed; at most the exact
        // ratio, 332/343 = 96.79…).
        assert!(pct >= 96.7, "parité {pct:.1} %");
    }

    /// Audit of the labels: each command of the table that SketchUp puts
    /// in a menu shows that menu's French label (`menu` of commands.json, without its `&`),
    /// read from an installed SketchUp (the test is skipped without one).
    #[test]
    fn menu_labels_are_sketchups() {
        let file = std::path::Path::new(r"C:\Program Files\SketchUp\SketchUp 2026\SketchUp\resources\fr\commands.json");
        let Ok(text) = std::fs::read_to_string(file) else { return eprintln!("SketchUp absent : test sauté") };
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let norm = |s: &str| s.replace('&', "").replace('’', "'").trim_end_matches("...").trim().to_string();
        // Where Esquisse says its own name, or keeps a label SketchUp's menu shows elsewhere.
        let ours = ["welcome to sketchup"];
        let mut wrong = Vec::new();
        let mut compared = 0;
        for c in json["list"].as_array().unwrap() {
            let (Some(key), Some(menu)) = (c["key"].as_str(), c["menu"].as_str()) else { continue };
            if ours.contains(&key) {
                continue;
            }
            let Some(it) = commands::TABLE.iter().find(|i| i.id == format!("cmd:{key}")) else { continue };
            let label = match it.imp {
                Impl::Cmd(c) => c.label(),
                Impl::Tool(t) => t.label().to_string(),
                Impl::Panel(p) => p.label().to_string(),
                _ => continue,
            };
            compared += 1;
            if norm(&label) != norm(menu) {
                wrong.push(format!("{key}: « {label} » au lieu de « {} »", norm(menu)));
            }
        }
        assert!(compared > 100, "{compared} libellés comparés");
        assert!(wrong.is_empty(), "libellés différents de SketchUp :\n{}", wrong.join("\n"));
    }
}
