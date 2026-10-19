//! The default content Esquisse ships with (materials, styles, environments, templates,
//! classifications, hatch patterns, components), in folders named as SketchUp names its own:
//! `Materials`, `Styles`, `Environments`, `Templates`, `Classifications`, `HatchPatterns`,
//! `Components`.
//!
//! The content sits next to the executable (`tools/package.ps1` puts it there). Nothing is
//! looked for anywhere else on the machine: an installed SketchUp is never read at run time.

use std::path::{Path, PathBuf};

/// The folders of the content, as SketchUp names them.
pub const FOLDERS: [&str; 7] =
    ["Materials", "Styles", "Environments", "Templates", "Classifications", "HatchPatterns", "Components"];

/// Whether `dir` holds Esquisse's content (it has a `Materials` folder).
fn holds_content(dir: &Path) -> bool {
    dir.join("Materials").is_dir()
}

/// The folder of the content: `ESQ_CONTENT` when set, else the executable's folder; a
/// debug build run from the repository (`cargo run`, `cargo test`) takes the repository's
/// `content/` folder.
pub fn dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("ESQ_CONTENT") {
        let d = PathBuf::from(d);
        return holds_content(&d).then_some(d);
    }
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf))
        && holds_content(&d)
    {
        return Some(d);
    }
    #[cfg(debug_assertions)]
    {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("content");
        if holds_content(&repo) {
            return Some(repo);
        }
    }
    None
}

/// One folder of the content (`Materials`, `Styles`…), if it is there.
pub fn folder(name: &str) -> Option<PathBuf> {
    let d = dir()?.join(name);
    d.is_dir().then_some(d)
}

/// The sub-folders of a folder, sorted by name (a library's collections).
pub fn subfolders(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repository_content_is_found_in_debug_builds() {
        let d = dir().expect("contenu introuvable");
        for f in FOLDERS {
            assert!(d.join(f).is_dir(), "{f} manquant dans {}", d.display());
        }
        assert!(folder("Materials").is_some() && folder("Inexistant").is_none());
    }
}
