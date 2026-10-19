//! The default content Esquisse ships (the repository's `content/`, put next to the
//! executable by `tools/package.ps1`): as many materials, styles, environments, templates,
//! classifications and hatch patterns as SketchUp 2026, in the same collections, made by this
//! code (`docs/autonomie.md` says what comes from where).
//!
//! - `ESQ_WRITE_CONTENT=1 cargo test -p esq-app content_gen::write_content -- --nocapture`
//!   writes it again; `ESQ_WRITE_CONTENT=styles,templates` only those parts.
//! - `ESQ_WRITE_TEMPLATE_FACTS=1 cargo test -p esq-app content_gen::write_template_facts`
//!   reads the templates of an installed SketchUp into `tools/content/modeles-types.json`.

mod classifications;
mod components;
mod environments;
mod hatches;
mod materials;
mod noise;
mod styles;
mod templates;

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("content")
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap_or_else(|e| panic!("{} : {e}", path.display()));
}

/// Empties a generated folder of the content before it is written again.
fn fresh(folder: &str) -> PathBuf {
    let d = root().join(folder);
    if d.is_dir() {
        std::fs::remove_dir_all(&d).unwrap();
    }
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn write_template_facts() {
    if std::env::var_os("ESQ_WRITE_TEMPLATE_FACTS").is_none() {
        return;
    }
    let files = templates::sketchup_templates().expect("SketchUp 2026 installé");
    let facts: Vec<_> = files.iter().map(|f| templates::read_fact(f)).collect();
    let json = serde_json::to_string_pretty(&facts).unwrap();
    write(&templates::facts_path(), json.as_bytes());
}

#[test]
fn write_content() {
    let Ok(parts) = std::env::var("ESQ_WRITE_CONTENT") else { return };
    let want = |p: &str| parts == "1" || parts.split(',').any(|x| x.trim() == p);
    if want("materials") {
        let d = fresh("Materials");
        for e in materials::all() {
            write(&d.join(e.folder).join(format!("{}.skm", e.file)), &crate::skm::write(&e.material, None));
        }
    }
    if want("styles") {
        let mut gpu = crate::tests::gpu::Gpu::new().expect("une carte graphique pour les vignettes");
        let d = fresh("Styles");
        for e in styles::all(&mut gpu) {
            write(&d.join(e.folder).join(format!("{}.style", e.file)), &crate::style_file::write(&e.style));
        }
    }
    if want("environments") {
        let d = fresh("Environments");
        for e in environments::all() {
            write(&d.join(format!("{}.ske", e.file)), &e.bytes);
        }
    }
    if want("templates") {
        let d = fresh("Templates");
        for f in templates::read_facts() {
            let (stem, m) = templates::build(&f);
            esq_core::io::save_esq(&m, &d.join(format!("{stem}.esq"))).unwrap();
        }
    }
    if want("classifications") {
        let d = fresh("Classifications");
        for (stem, bytes) in classifications::all() {
            write(&d.join(format!("{stem}.skc")), &bytes);
        }
    }
    if want("hatches") {
        let d = fresh("HatchPatterns");
        for (stem, bytes) in hatches::all() {
            write(&d.join("Default Hatching").join(format!("{stem}.skh")), &bytes);
        }
    }
    if want("components") {
        let mut gpu = crate::tests::gpu::Gpu::new().expect("une carte graphique pour les vignettes");
        let d = fresh("Components");
        for e in components::all(&mut gpu) {
            let dir = d.join(e.folder);
            std::fs::create_dir_all(&dir).unwrap();
            esq_core::io::save_esq(&e.model, &dir.join(format!("{}.esq", e.file))).unwrap();
            write(&dir.join(format!("{}.png", e.file)), &e.thumbnail);
        }
    }
}

fn files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == ext)).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn size_of(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| {
                    let p = e.path();
                    if p.is_dir() { size_of(&p) } else { e.metadata().map_or(0, |m| m.len()) }
                })
                .sum()
        })
        .unwrap_or(0)
}

/// The content that comes with Esquisse: SketchUp's collections and counts, every file
/// readable by Esquisse, names in French, a reasonable size.
#[test]
fn the_content_has_sketchups_collections_and_reads_back() {
    let root = esq_core::content::dir().expect("contenu");
    // Materials.
    for (folder, n) in materials::COLLECTIONS {
        let list = crate::skm::read_folder(&root.join("Materials").join(folder));
        assert_eq!(list.len(), n, "Matières › {folder}");
        assert!(crate::i18n::fr(folder).is_some(), "pas de nom français pour {folder}");
        let mut names: Vec<&str> = list.iter().map(|m| m.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n, "noms en double dans {folder}");
    }
    // Styles.
    for (folder, n) in styles::COLLECTIONS {
        let list = crate::styles_panel::read_folder(&root.join("Styles").join(folder));
        assert_eq!(list.len(), n, "Styles › {folder}");
        assert!(list.iter().all(|s| s.file.thumbnail.is_some() && !s.file.description.is_empty()));
        assert!(crate::i18n::fr(folder).is_some(), "pas de nom français pour {folder}");
    }
    // Environments.
    let envs = files(&root.join("Environments"), "ske");
    assert_eq!(envs.len(), environments::COUNT);
    for p in &envs {
        let bytes = std::fs::read(p).unwrap();
        let ske = crate::environments::read_ske(&bytes).unwrap();
        assert!(!ske.name.is_empty() && ske.thumbnail.is_some(), "{}", p.display());
        let (w, h, px) = crate::environments::decode(&bytes).unwrap();
        assert_eq!((w, h), (1024, 512));
        assert!(px.iter().all(|c| c.iter().all(|v| v.is_finite() && *v >= 0.0)));
    }
    // Templates: names and units of SketchUp's, our French names.
    let temps = files(&root.join("Templates"), "esq");
    assert_eq!(temps.len(), templates::COUNT);
    for p in &temps {
        let m = esq_core::io::load_esq(p).unwrap();
        assert!(!m.meta.name.is_empty() && m.view_ext.contains_key("ro"), "{}", p.display());
        assert!(
            m.defs.is_empty() && m.materials.is_empty() && m.root.faces.is_empty(),
            "rien d'autre que des réglages"
        );
    }
    // Classifications.
    let skc = files(&root.join("Classifications"), "skc");
    assert_eq!(skc.len(), classifications::COUNT);
    for p in &skc {
        let s = crate::dialogs::mi_classif::read_schema(p).expect("classement lisible");
        assert!(
            s.name.starts_with("IFC") && s.types.iter().any(|t| t == "IfcWall") && !s.types.iter().any(|t| t == "Name")
        );
    }
    // Hatch patterns.
    let skh = files(&root.join("HatchPatterns").join("Default Hatching"), "skh");
    assert_eq!(skh.len(), hatches::COUNT);
    for p in &skh {
        crate::skh::read(&std::fs::read(p).unwrap(), 1).expect("motif lisible");
    }
    // Components: models of their own, each with its thumbnail, named in French.
    for (folder, n) in components::COLLECTIONS {
        let items = crate::comp_library::read_folder(&root.join("Components").join(folder));
        assert_eq!(items.len(), n, "Composants › {folder}");
        assert!(crate::i18n::fr(folder).is_some(), "pas de nom français pour {folder}");
        for it in &items {
            let m = crate::comp_library::load(&it.path).expect("composant lisible");
            assert!(!m.root.faces.is_empty() && !m.meta.description.is_empty(), "{}", it.path.display());
            assert!(it.thumbnail.is_some(), "vignette de {}", it.path.display());
        }
    }
    // Everything together stays light enough for the repository.
    let total = size_of(&root);
    assert!(total < 40 << 20, "contenu : {} Mo", total >> 20);
}

/// The templates' table holds what SketchUp's own templates set (when it is installed).
#[test]
fn templates_table_matches_sketchup() {
    let Some(files) = templates::sketchup_templates() else { return eprintln!("SketchUp absent : test sauté") };
    let table = templates::read_facts();
    assert_eq!(table.len(), files.len());
    for (f, fact) in files.iter().zip(&table) {
        let read = templates::read_fact(f);
        assert_eq!((&read.file, &read.name, read.units), (&fact.file, &fact.name, fact.units));
        assert_eq!(read.ro, fact.ro, "{}", fact.file);
    }
}
