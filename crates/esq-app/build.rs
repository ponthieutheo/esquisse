//! The logo's files in the build: `assets/logo` (or the folder named by `ESQ_LOGO_ASSETS`, to try
//! a logo without putting it in the repository) gives
//! - `OUT_DIR/logo_files.rs`: the PNG of the window icon and of the welcome window's brand
//!   (`src/logo.rs`), `None` for each one missing;
//! - `OUT_DIR/esquisse.res`: the icon of `esquisse.exe` (Explorer, taskbar, shortcuts), made from
//!   `esquisse.ico` (see `src/winres.rs`), handed to link.exe which takes `.res` files as input.
//!
//! Without these files the application keeps eframe's icon and the welcome window shows no brand.

use std::path::{Path, PathBuf};

#[allow(dead_code)] // the crate's tests use the rest (write_ico)
#[path = "src/winres.rs"]
mod winres;

/// Writes `data` unless the file already holds it: an unchanged file keeps its date, so rustc
/// does not rebuild the crate for nothing.
fn put(path: &Path, data: &[u8]) {
    if std::fs::read(path).ok().as_deref() != Some(data) {
        std::fs::write(path, data).unwrap_or_else(|e| panic!("{} : {e}", path.display()));
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/winres.rs");
    println!("cargo:rerun-if-env-changed=ESQ_LOGO_ASSETS");
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let dir = match std::env::var_os("ESQ_LOGO_ASSETS") {
        Some(d) => PathBuf::from(d),
        None => manifest.join("assets/logo"),
    };
    // A folder is watched whole; `assets` always exists, `assets/logo` only once a logo is chosen.
    let watched = if dir.exists() { dir.clone() } else { manifest.join("assets") };
    println!("cargo:rerun-if-changed={}", watched.display());

    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let found = |name: &str| Some(dir.join(name)).filter(|p| p.is_file());
    let bytes = |p: Option<PathBuf>| match p {
        Some(p) => format!("Some(include_bytes!({:?}))", p.display().to_string()),
        None => "None".into(),
    };
    let ico = found("esquisse.ico");
    let mut code = String::from("// Écrit par build.rs : les fichiers du logo trouvés au moment de la compilation.\n");
    code += &format!("pub const ICON_PNG: Option<&[u8]> = {};\n", bytes(found("esquisse-256.png")));
    code += &format!("pub const WORD_LIGHT_PNG: Option<&[u8]> = {};\n", bytes(found("esquisse-mot-clair.png")));
    code += &format!("pub const WORD_DARK_PNG: Option<&[u8]> = {};\n", bytes(found("esquisse-mot-sombre.png")));
    let source = ico.as_ref().map(|p| format!("Some({:?})", p.display().to_string()));
    code += &format!(
        "#[allow(dead_code)]\npub const ICO_SOURCE: Option<&str> = {};\n",
        source.as_deref().unwrap_or("None")
    );
    put(&out.join("logo_files.rs"), code.as_bytes());

    let res = out.join("esquisse.res");
    let Some(ico) = ico else {
        let _ = std::fs::remove_file(&res);
        return;
    };
    let data = std::fs::read(&ico).unwrap_or_else(|e| panic!("{} : {e}", ico.display()));
    put(&res, &winres::icon_res(&data).unwrap_or_else(|e| panic!("{} : {e}", ico.display())));
    let target = |k: &str| std::env::var(k).unwrap_or_default();
    if target("CARGO_CFG_TARGET_OS") == "windows" && target("CARGO_CFG_TARGET_ENV") == "msvc" {
        println!("cargo:rustc-link-arg-bins={}", res.display());
    }
}
