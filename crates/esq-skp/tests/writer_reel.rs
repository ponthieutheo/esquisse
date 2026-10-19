//! The native writer on a real file (`tests/reel/stool-depth-8.skp`, written by SketchUp 8):
//! read natively, then written by the native writer and by the SDK's in a version, the curves
//! the SDK reads back from our file must be those of the SDK's file, or else those of the file
//! itself (the rule of `native_writer.rs`). The case (`docs/skp-format.md` §8.7): two quarter
//! arcs whose middle vertices are those of a polygon around the arc, not on it; the writer keeps
//! the arc as SketchUp wrote it.
//!
//! `every_real_file_keeps_its_curves` (`--ignored`, best in release): the same on the whole real
//! corpus (`ESQ_SKP_REEL`, else `%USERPROFILE%\tools\skp-reel`), in 2026: the curves the SDK
//! reads in our file are those it reads in the original, or in SketchUp's own conversion of it,
//! or those of the model read (where the native reader reads the original otherwise).
//! `ESQ_REEL_FILES` (a list of paths) narrows it, `ESQ_COMPLET_DIFF` shows the differences.
use std::path::Path;

use esq_skp::SkpVersion;
use esq_skp::native::canon;

/// The curves (class « courbes ») the SDK reads in a file.
fn sdk_curves(path: &Path) -> Vec<String> {
    let (m, _) = esq_skp::sdk_import_skp_view(path).expect("le SDK ouvre le fichier");
    canon::canon(&m).remove("courbes").unwrap_or_default()
}

#[test]
fn a_real_file_rewritten_natively_keeps_its_curves() {
    if !esq_skp::available() {
        eprintln!("SketchUp absent : test ignoré");
        return;
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("reel").join("stool-depth-8.skp");
    let data = std::fs::read(&src).unwrap();
    let (m, view) = esq_skp::native::read_bytes(&data).unwrap();
    // What the file itself holds, as the SDK reads it (the native reader agrees: `native_reel`).
    let source = sdk_curves(&src);
    let dir = std::env::temp_dir().join(format!("esq-writer-reel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut failures = Vec::new();
    for v in [SkpVersion::Su2021, SkpVersion::Su2017, SkpVersion::Su6] {
        let ours = dir.join(format!("natif-{}.skp", v.name()));
        let theirs = dir.join(format!("sdk-{}.skp", v.name()));
        let bytes = esq_skp::native::write::write(&m, view.as_ref(), Some(v), &Default::default()).unwrap();
        std::fs::write(&ours, bytes).unwrap();
        esq_skp::sdk_export_skp_version(&m, &theirs, view.as_ref(), Some(v)).unwrap();
        let (got, oracle) = (sdk_curves(&ours), sdk_curves(&theirs));
        if got != oracle && got != source {
            let (only_ours, only_theirs) = canon::diff(&got, &oracle);
            let (_, only_source) = canon::diff(&got, &source);
            eprintln!("== {} : relu par le SDK", v.name());
            for (who, lines) in
                [("notre fichier", &only_ours), ("le sien", &only_theirs), ("le fichier d'origine", &only_source)]
            {
                for l in lines.iter().take(3) {
                    eprintln!("  {who:<20} {}", l.chars().take(260).collect::<String>());
                }
            }
            failures.push(v.name());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(failures.is_empty(), "courbes relues par le SDK ni comme son fichier ni comme l'original en {failures:?}");
}

mod oracle;

#[test]
#[ignore]
fn every_real_file_keeps_its_curves() {
    if !esq_skp::available() {
        eprintln!("SketchUp absent : test ignoré");
        return;
    }
    let Some(dir) = std::env::var_os("ESQ_SKP_REEL").map(std::path::PathBuf::from).or_else(|| {
        std::env::var_os("USERPROFILE").map(|h| std::path::PathBuf::from(h).join("tools").join("skp-reel"))
    }) else {
        return;
    };
    let mut files = Vec::new();
    oracle::walk(&dir, &mut files, 0);
    // `ESQ_REEL_FILES`: a file listing the paths to check (one per line).
    if let Some(list) = std::env::var_os("ESQ_REEL_FILES").and_then(|f| std::fs::read_to_string(f).ok()) {
        let keep: std::collections::HashSet<&str> = list.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        files.retain(|f| keep.contains(f.to_string_lossy().as_ref()));
    }
    files.sort();
    let show = std::env::var_os("ESQ_COMPLET_DIFF").is_some();
    let tmp = std::env::temp_dir().join(format!("esq-writer-reel-all-{}.skp", std::process::id()));
    let resaved = std::env::temp_dir().join(format!("esq-writer-reel-all-{}-sdk.skp", std::process::id()));
    let (mut n, mut same, mut as_sketchup, mut as_read, mut diff) = (0, 0, 0, 0, Vec::new());
    for f in &files {
        let Ok(data) = std::fs::read(f) else { continue };
        let Ok((m, view)) = esq_skp::native::read_bytes(&data) else { continue };
        let Ok((orig, _)) = esq_skp::sdk_import_skp_view(f) else { continue };
        let Ok(bytes) = esq_skp::native::write::write(&m, view.as_ref(), None, &Default::default()) else { continue };
        if std::fs::write(&tmp, bytes).is_err() {
            continue;
        }
        let Ok((back, _)) = esq_skp::sdk_import_skp_view(&tmp) else {
            diff.push(format!("{} : refusé par le SDK", f.display()));
            continue;
        };
        n += 1;
        let (a, b) = (canon::canon(&orig), canon::canon(&back));
        if a["courbes"] == b["courbes"] {
            same += 1;
            continue;
        }
        // Else as SketchUp's own conversion of the file to 2026 (it recomputes some arcs).
        let theirs =
            esq_skp::sdk_resave(f, &resaved, None).ok().and_then(|_| esq_skp::sdk_import_skp_view(&resaved).ok());
        if let Some((t, _)) = &theirs
            && canon::canon(t)["courbes"] == b["courbes"]
        {
            as_sketchup += 1;
            continue;
        }
        // Else as the model written, where the native reader reads the original otherwise than
        // the SDK (a reading difference, `native_reel`, not a loss of the writer).
        let quirks = esq_skp::native::Options { sdk_quirks: true };
        if esq_skp::native::read_bytes_with(&data, &quirks)
            .is_ok_and(|(q, _)| canon::canon(&q)["courbes"] == b["courbes"])
        {
            as_read += 1;
            continue;
        }
        diff.push(format!("{} : courbes", f.display()));
        if show {
            let base = theirs.as_ref().map_or(a["courbes"].clone(), |(t, _)| canon::canon(t)["courbes"].clone());
            let (x, y) = canon::diff(&base, &b["courbes"]);
            for l in x.iter().take(3) {
                println!("    SketchUp {}", l.chars().take(300).collect::<String>());
            }
            for l in y.iter().take(3) {
                println!("    réécrit  {}", l.chars().take(300).collect::<String>());
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(&resaved);
    println!(
        "courbes relues par le SDK comme l'original : {same}/{n}, comme la conversion de SketchUp : {as_sketchup}, comme le modèle lu : {as_read}, autres : {}",
        diff.len()
    );
    for d in &diff {
        println!("  {d}");
    }
    assert!(diff.is_empty(), "{} fichiers réécrits dont le SDK ne relit pas les courbes", diff.len());
}
