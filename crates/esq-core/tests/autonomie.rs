//! Esquisse runs on a PC where SketchUp was never installed, with only what is shipped next
//! to its executable. This test fails when the code the application runs names a place of
//! an installed SketchUp (`Program Files\SketchUp…`, `ProgramData`), a user's folder
//! (`C:\Users\…`, `USERPROFILE`) or a developer's Ruby (`ruby32`, RubyInstaller).
//!
//! Test code may use an installed SketchUp as an oracle and is left out: `#[cfg(test)]` items
//! and modules (and the files of such modules), the crates' `tests/` and `examples/` folders.
//! Comments are left out too. `docs/autonomie.md` lists what the application reads.

use std::path::{Path, PathBuf};

/// Substrings (lower case) that betray such a path, and what they are.
const FORBIDDEN: &[(&str, &str)] = &[
    ("program files", "dossier Program Files"),
    ("programfiles", "variable ProgramFiles"),
    ("programw6432", "variable ProgramW6432"),
    ("programdata", "dossier ou variable ProgramData"),
    (":\\users\\", "profil d'utilisateur écrit en dur"),
    (":\\\\users\\\\", "profil d'utilisateur écrit en dur"),
    (":/users/", "profil d'utilisateur écrit en dur"),
    ("userprofile", "variable USERPROFILE"),
    ("\\ruby32", "Ruby installé par le développeur"),
    ("/ruby32", "Ruby installé par le développeur"),
    ("\"ruby32\"", "Ruby installé par le développeur"),
    ("\\rubyinstaller-", "Ruby installé par le développeur"),
    ("/rubyinstaller-", "Ruby installé par le développeur"),
    ("shippedclassifications", "dossier d'installation de SketchUp"),
    ("shippedextensions", "dossier d'installation de SketchUp"),
    ("\"sketchup 20", "dossier d'installation de SketchUp"),
    ("sketchup 2026\\", "dossier d'installation de SketchUp"),
    ("sketchup 2026/", "dossier d'installation de SketchUp"),
];

/// Lines still allowed, with the reason. Only the SDK of an installed SketchUp
/// (`SketchUpAPI.dll`) is looked for there, and only with `ESQ_SKP_SDK=1` (see
/// [`SDK_CALLS`]): .skp files are read and written natively.
const ALLOWED: &[(&str, &str, &str)] = &[(
    "crates/esq-skp/src/lib.rs",
    "PathBuf::from(std::env::var(\"ProgramFiles\").unwrap_or_else(|_| r\"C:\\Program Files\".into())).join(\"SketchUp\");",
    "SketchUpAPI.dll, chargée seulement avec ESQ_SKP_SDK=1 (référence des tests du lecteur et de l'écrivain natifs)",
)];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Code,
    Literal,
    Comment,
}

/// The kind of every byte of Rust source: code, string or char literal, comment.
fn classify(src: &[u8]) -> Vec<Kind> {
    let mut k = vec![Kind::Code; src.len()];
    let n = src.len();
    let mut i = 0;
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    while i < n {
        let c = src[i];
        if c == b'/' && i + 1 < n && src[i + 1] == b'/' {
            let s = i;
            while i < n && src[i] != b'\n' {
                i += 1;
            }
            k[s..i].fill(Kind::Comment);
        } else if c == b'/' && i + 1 < n && src[i + 1] == b'*' {
            let s = i;
            let mut depth = 0;
            while i < n {
                if src[i] == b'/' && i + 1 < n && src[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if src[i] == b'*' && i + 1 < n && src[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            k[s..i.min(n)].fill(Kind::Comment);
        } else if c == b'r' && (i == 0 || !ident(src[i - 1])) && {
            let mut j = i + 1;
            while j < n && src[j] == b'#' {
                j += 1;
            }
            j < n && src[j] == b'"'
        } {
            // Raw string r#"…"#.
            let s = i;
            let mut j = i + 1;
            let mut hashes = 0;
            while src[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            j += 1;
            loop {
                if j >= n {
                    break;
                }
                if src[j] == b'"' && src[j + 1..].iter().take(hashes).filter(|&&b| b == b'#').count() == hashes {
                    j += 1 + hashes;
                    break;
                }
                j += 1;
            }
            i = j.min(n);
            k[s..i].fill(Kind::Literal);
        } else if c == b'"' {
            let s = i;
            i += 1;
            while i < n && src[i] != b'"' {
                if src[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(n);
            k[s..i].fill(Kind::Literal);
        } else if c == b'\'' {
            // A char literal ('x', '\n', 'é') or a lifetime ('a).
            let s = i;
            if i + 1 < n && src[i + 1] == b'\\' {
                i += 2;
                while i < n && src[i] != b'\'' {
                    i += 1;
                }
                i = (i + 1).min(n);
                k[s..i].fill(Kind::Literal);
            } else {
                let len = std::str::from_utf8(&src[i + 1..(i + 5).min(n)])
                    .ok()
                    .or_else(|| std::str::from_utf8(&src[i + 1..(i + 4).min(n)]).ok())
                    .or_else(|| std::str::from_utf8(&src[i + 1..(i + 3).min(n)]).ok())
                    .or_else(|| std::str::from_utf8(&src[i + 1..(i + 2).min(n)]).ok())
                    .and_then(|t| t.chars().next())
                    .map_or(1, char::len_utf8);
                if i + 1 + len < n && src[i + 1 + len] == b'\'' {
                    i += 2 + len;
                    k[s..i].fill(Kind::Literal);
                } else {
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
    }
    k
}

/// The byte ranges of `#[cfg(test)]` items, and the names of `#[cfg(test)] mod name;` files.
fn test_items(src: &[u8], kind: &[Kind]) -> (Vec<(usize, usize)>, Vec<String>) {
    let text = String::from_utf8_lossy(src);
    let mut ranges = Vec::new();
    let mut modules = Vec::new();
    let mut from = 0;
    while let Some(p) = text[from..].find("#[cfg(test)]") {
        let start = from + p;
        from = start + 1;
        if kind[start] != Kind::Code {
            continue;
        }
        let mut i = start + "#[cfg(test)]".len();
        while i < src.len() && !(kind[i] == Kind::Code && (src[i] == b'{' || src[i] == b';')) {
            i += 1;
        }
        if i >= src.len() {
            break;
        }
        if src[i] == b';' {
            let item = text[start + 12..i].trim();
            if let Some(name) = item.rsplit("mod ").next().filter(|_| item.contains("mod ")) {
                modules.push(name.trim().to_string());
            }
            ranges.push((start, i + 1));
            continue;
        }
        let mut depth = 0;
        let mut j = i;
        while j < src.len() {
            if kind[j] == Kind::Code {
                if src[j] == b'{' {
                    depth += 1;
                } else if src[j] == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            j += 1;
        }
        ranges.push((start, (j + 1).min(src.len())));
    }
    (ranges, modules)
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs" || x == "rb") {
            out.push(p);
        }
    }
}

/// Folders holding the files of `#[cfg(test)] mod name;` declared in `file`.
fn module_paths(file: &Path, name: &str) -> [PathBuf; 2] {
    let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = if matches!(stem.as_str(), "lib" | "main" | "mod") {
        file.parent().unwrap().to_path_buf()
    } else {
        file.parent().unwrap().join(stem)
    };
    [dir.join(format!("{name}.rs")), dir.join(name)]
}

/// The offending lines of the shipped code: (file, line number, line, what).
fn offences(root: &Path) -> Vec<(String, usize, String, &'static str)> {
    let mut files = Vec::new();
    for c in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        rust_files(&c.path().join("src"), &mut files);
    }
    files.sort();
    // Test-only modules declared in other files.
    let mut excluded: Vec<PathBuf> = Vec::new();
    for f in &files {
        if f.extension().is_some_and(|x| x == "rs") {
            let src = std::fs::read(f).unwrap();
            let kind = classify(&src);
            for m in test_items(&src, &kind).1 {
                excluded.extend(module_paths(f, &m));
            }
        }
    }
    let mut out = Vec::new();
    let tools = ["package.ps1", "ruby-dev.ps1", "ruby-runtime.ps1"].map(|f| root.join("tools").join(f));
    for f in files.iter().chain(tools.iter().filter(|p| p.is_file())) {
        if excluded.iter().any(|e| f.starts_with(e)) {
            continue;
        }
        let rel = f.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        let src = std::fs::read(f).unwrap();
        let ext = f.extension().and_then(|x| x.to_str()).unwrap_or("");
        let mut keep: Vec<bool> = match ext {
            "rs" => {
                let kind = classify(&src);
                let mut keep: Vec<bool> = kind.iter().map(|k| *k != Kind::Comment).collect();
                for (a, b) in test_items(&src, &kind).0 {
                    keep[a..b].fill(false);
                }
                keep
            }
            _ => vec![true; src.len()],
        };
        // Ruby and PowerShell: whole comment lines are left out.
        if ext != "rs" {
            let mut start = 0;
            for line in src.split(|&b| b == b'\n') {
                // (A byte order mark may start the file.)
                if line.iter().find(|b| !b.is_ascii_whitespace() && ![0xEF, 0xBB, 0xBF].contains(*b)) == Some(&b'#') {
                    keep[start..start + line.len()].fill(false);
                }
                start += line.len() + 1;
            }
        }
        // Bytes as chars: the patterns are ASCII, and line breaks are kept to count lines.
        let text: String =
            src.iter().zip(&keep).map(|(&b, &k)| (if k || b == b'\n' { b } else { b' ' }) as char).collect();
        let lines: Vec<&str> = std::str::from_utf8(&src).unwrap_or("").lines().collect();
        for (no, line) in text.lines().enumerate() {
            let low = line.to_lowercase();
            for (pat, what) in FORBIDDEN {
                if low.contains(pat) {
                    let original = lines.get(no).map_or("", |l| l.trim());
                    if ALLOWED.iter().any(|(file, l, _)| *file == rel && *l == original) {
                        continue;
                    }
                    out.push((rel.clone(), no + 1, original.to_string(), *what));
                }
            }
        }
    }
    out
}

#[test]
fn no_path_to_sketchup_or_to_a_users_folder_in_the_shipped_code() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let found = offences(&root);
    let report: Vec<String> = found.iter().map(|(f, n, l, w)| format!("{f}:{n} : {w}\n    {l}")).collect();
    assert!(found.is_empty(), "chemins interdits hors des tests :\n{}", report.join("\n"));
}

#[test]
fn the_checker_sees_paths_and_skips_tests_and_comments() {
    let dir = std::env::temp_dir().join(format!("esq-autonomie-{}", std::process::id()));
    let src = dir.join("crates").join("x").join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("lib.rs"),
        "// C:\\Program Files\\SketchUp in a comment\n\
         fn a() { let p = r\"C:\\Program Files\\SketchUp\"; }\n\
         fn b() -> char { '{' }\n\
         fn c() { let q = \"C:\\\\Users\\\\alice\\\\tools\"; }\n\
         #[cfg(test)]\nmod tests { fn t() { let p = r\"C:\\ProgramData\\SketchUp\"; let s = \"}\"; } }\n\
         #[cfg(test)]\nmod oracle;\n\
         fn d() { std::env::var(\"USERPROFILE\"); }\n",
    )
    .unwrap();
    std::fs::write(src.join("oracle.rs"), "fn o() { let p = \"C:/Program Files/SketchUp\"; }\n").unwrap();
    let found = offences(&dir);
    let lines: Vec<usize> = found.iter().map(|x| x.1).collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(lines, vec![2, 4, 9], "{found:?}");
}

// ---------------------------------------------------------------------------------------------
// SketchUp's SDK is loaded only with ESQ_SKP_SDK=1

/// What reaches SketchUp's SDK (`SketchUpAPI.dll`) in `esq-skp`: its loader and the functions
/// that go through it.
const SDK_CALLS: &[&str] = &[
    "api()",
    "available()",
    "api_version()",
    "space_api()",
    "find_sketchup_api(",
    "sdk_import_skp_view(",
    "sdk_export_skp_version(",
    "sketchupapi.dll",
];

/// The functions of `esq-skp` that are the SDK's own (its loader, the SDK reader and writer the
/// tests compare with): they may reach it.
const SDK_FUNCTIONS: &[&str] = &[
    "find_sketchup_api",
    "api",
    "available",
    "api_version",
    "space_api",
    "read_spaces",
    "sdk_import_skp_view",
    "sdk_export_skp_version",
    "sdk_census",
    "sdk_guids",
    "sdk_attach_ends",
    "sdk_resave",
    "sdk_edit_faces",
    "sdk_corpus_extras",
    "sdk_decode_image",
];

/// The functions the application calls that may go to the SDK, and only after
/// `sdk_reader_forced()` (`ESQ_SKP_SDK=1`) said so.
const SDK_GATED: &[&str] = &["read_guid", "import_skp_view", "export_skp_with"];

/// The innermost function around each byte of code: (name, start of its body, end).
fn functions(src: &[u8], kind: &[Kind]) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while i + 3 < src.len() {
        let is_fn = kind[i] == Kind::Code && &src[i..i + 3] == b"fn " && (i == 0 || !ident(src[i - 1]));
        if !is_fn {
            i += 1;
            continue;
        }
        let mut j = i + 3;
        let start = j;
        while j < src.len() && ident(src[j]) {
            j += 1;
        }
        let name = String::from_utf8_lossy(&src[start..j]).into_owned();
        // Its body: the first `{` of code (a `;` first: a declaration without one).
        while j < src.len() && !(kind[j] == Kind::Code && (src[j] == b'{' || src[j] == b';')) {
            j += 1;
        }
        if j >= src.len() || src[j] == b';' {
            i = j;
            continue;
        }
        let body = j;
        let mut depth = 0;
        while j < src.len() {
            if kind[j] == Kind::Code {
                if src[j] == b'{' {
                    depth += 1;
                } else if src[j] == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            j += 1;
        }
        out.push((name, body, j));
        i = body;
    }
    out
}

/// The calls that may reach SketchUp's SDK outside its own functions or its gate: (file,
/// line number, line).
fn sdk_offences(root: &Path) -> Vec<(String, usize, String)> {
    let mut files = Vec::new();
    for c in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        rust_files(&c.path().join("src"), &mut files);
    }
    files.retain(|f| f.extension().is_some_and(|x| x == "rs"));
    files.sort();
    let mut excluded: Vec<PathBuf> = Vec::new();
    for f in &files {
        let src = std::fs::read(f).unwrap();
        let kind = classify(&src);
        for m in test_items(&src, &kind).1 {
            excluded.extend(module_paths(f, &m));
        }
    }
    let mut out = Vec::new();
    for f in &files {
        if excluded.iter().any(|e| f.starts_with(e)) {
            continue;
        }
        let rel = f.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        let in_skp = rel.starts_with("crates/esq-skp/");
        let src = std::fs::read(f).unwrap();
        let kind = classify(&src);
        let tests = test_items(&src, &kind).0;
        let fns = functions(&src, &kind);
        // (Lower case of ASCII only: the offsets stay those of `kind`.)
        let low = src.to_ascii_lowercase();
        let text = String::from_utf8_lossy(&src).into_owned();
        for pat in SDK_CALLS {
            let mut from = 0;
            while let Some(p) = std::str::from_utf8(&low[from..]).ok().and_then(|s| s.find(pat)) {
                let at = from + p;
                from = at + 1;
                // Code (or a string naming the DLL), not a comment nor a test; a whole name
                // (`api()` is not `space_api()`), not a method of another type (`ruby.api()`).
                let ok_kind = kind[at] == Kind::Code || (pat.ends_with(".dll") && kind[at] == Kind::Literal);
                let whole = at == 0 || !(low[at - 1].is_ascii_alphanumeric() || matches!(low[at - 1], b'_' | b'.'));
                if !ok_kind || !whole || tests.iter().any(|(a, b)| at >= *a && at < *b) {
                    continue;
                }
                // The name of a function being defined.
                if text[..at].ends_with("fn ") {
                    continue;
                }
                let inner = fns.iter().filter(|(_, a, b)| at > *a && at < *b).min_by_key(|(_, a, b)| b - a);
                let allowed = in_skp
                    && inner.is_some_and(|(name, a, _)| {
                        SDK_FUNCTIONS.contains(&name.as_str())
                            || (SDK_GATED.contains(&name.as_str()) && text[*a..at].contains("sdk_reader_forced()"))
                    });
                if !allowed {
                    let no = text[..at].matches('\n').count();
                    out.push((rel.clone(), no + 1, text.lines().nth(no).unwrap_or("").trim().to_string()));
                }
            }
        }
    }
    out
}

#[test]
fn sketchups_sdk_is_loaded_only_with_esq_skp_sdk() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let found = sdk_offences(&root);
    let report: Vec<String> = found.iter().map(|(f, n, l)| format!("{f}:{n}\n    {l}")).collect();
    assert!(found.is_empty(), "appels qui peuvent charger SketchUpAPI.dll sans ESQ_SKP_SDK=1 :\n{}", report.join("\n"));
}

#[test]
fn the_sdk_checker_sees_ungated_calls() {
    let dir = std::env::temp_dir().join(format!("esq-autonomie-sdk-{}", std::process::id()));
    let src = dir.join("crates").join("esq-skp").join("src");
    let app = dir.join("crates").join("esq-app").join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(
        src.join("lib.rs"),
        "fn api() -> u8 { 0 }\n\
         pub fn available() -> bool { api() == 0 }\n\
         pub fn import_skp_view() { if !sdk_reader_forced() || !available() { return; } sdk_import_skp_view(); }\n\
         pub fn export_skp_with() { if available() { sdk_export_skp_version(); } }\n\
         fn space_api() { api(); }\n\
         // api() in a comment\n\
         #[cfg(test)]\nmod tests { fn t() { super::api(); } }\n",
    )
    .unwrap();
    std::fs::write(app.join("main.rs"), "fn main() { let _ = esq_skp::available(); }\n").unwrap();
    let found = sdk_offences(&dir);
    let lines: Vec<(String, usize)> = found.iter().map(|x| (x.0.clone(), x.1)).collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        lines,
        vec![
            ("crates/esq-app/src/main.rs".to_string(), 1),
            ("crates/esq-skp/src/lib.rs".to_string(), 4),
            ("crates/esq-skp/src/lib.rs".to_string(), 4)
        ],
        "{found:?}"
    );
}
