//! Les règles communes pour ce qui vient d'un fichier ou d'une extension (modèle téléchargé,
//! archive, bibliothèque partagée) : `docs/securite.md`.
//!
//! - Un chemin lu dans un fichier n'ouvre jamais un serveur ni un périphérique ([`is_remote`],
//!   [`local_path`], [`read_referenced`]) : un chemin UNC (`\\serveur\x`, `//serveur/x`,
//!   `\\?\UNC\…`) ferait envoyer par Windows le hachage NTLM de l'utilisateur au serveur, un
//!   nom de périphérique (`\\.\`, `CON`, `COM1`) bloquerait ou écrirait sur le matériel.
//! - Un nom d'entrée d'archive ne devient un chemin que sous une racine donnée
//!   ([`join_under`]) : ni `..`, ni chemin absolu, ni lecteur, ni nom réservé de Windows.
//! - Ce qu'un fichier compressé peut produire est borné ([`Budget`]) : une archive de quelques
//!   kilo-octets ne remplit pas la mémoire.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Whether `path` (read from a file) names a network location or a device: `\\server\share`,
/// `//server/share` (and mixed `\/`), `\\?\…`, `\\.\…`, `\??\…`, `file://server/…`. Opening one
/// makes Windows contact the server (and send the user's NTLM hash) or the device.
pub fn is_remote(path: &str) -> bool {
    let p = path.trim_start();
    let b = p.as_bytes();
    let sep = |c: u8| c == b'\\' || c == b'/';
    if b.len() >= 2 && sep(b[0]) && sep(b[1]) {
        return true;
    }
    if p.starts_with(r"\??\") {
        return true;
    }
    // file://host/path: a host other than localhost is a server; with an empty host
    // (file:///C:/x) the path follows, and may itself be a network path (file:////srv/x).
    if p.get(..7).is_some_and(|s| s.eq_ignore_ascii_case("file://")) {
        let rest = &p[7..];
        if rest.starts_with(['/', '\\']) {
            return is_remote(rest);
        }
        let host = rest.split(['/', '\\']).next().unwrap_or("");
        return !host.eq_ignore_ascii_case("localhost");
    }
    false
}

/// Whether a path component is a reserved device name of Windows (`CON`, `PRN`, `AUX`, `NUL`,
/// `COM0`–`COM9`, `LPT0`–`LPT9`, `CONIN$`, `CONOUT$`), with or without an extension: such a
/// "file" is the device.
pub fn is_reserved_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or("").trim_end_matches([' ', '.']).to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$" => true,
        s if s.len() == 4 && (s.starts_with("COM") || s.starts_with("LPT")) => s.as_bytes()[3].is_ascii_digit(),
        s => {
            // COM¹, LPT² …: superscript digits count too.
            (s.starts_with("COM") || s.starts_with("LPT")) && matches!(&s[3..], "¹" | "²" | "³")
        }
    }
}

/// A path read from a file that may be opened without asking: absolute, on a local drive,
/// and naming no device. `None` for a network or device path, or a relative one (it would
/// depend on the current folder).
pub fn local_path(path: &str) -> Option<PathBuf> {
    let p = path.trim();
    if p.is_empty() || p.contains('\0') || is_remote(p) {
        return None;
    }
    let pb = PathBuf::from(p);
    if !pb.is_absolute() {
        return None;
    }
    let reserved = pb.components().any(|c| match c {
        std::path::Component::Normal(n) => n.to_str().is_none_or(is_reserved_name),
        _ => false,
    });
    (!reserved).then_some(pb)
}

/// Whether `c` can be one component of a file path made from an archive or a model: not
/// empty, no `:` (drive, alternate stream), no character Windows refuses, no control
/// character, no trailing dot or space (Windows drops them: `.. ` would become `..`), not
/// `.`/`..`, not a reserved device name.
pub fn is_safe_component(c: &str) -> bool {
    !c.is_empty()
        && c.len() <= 255
        && c != "."
        && c != ".."
        && !c.ends_with(['.', ' '])
        && !c.chars().any(|ch| ch.is_control() || matches!(ch, ':' | '<' | '>' | '"' | '|' | '?' | '*' | '/' | '\\'))
        && !is_reserved_name(c)
}

/// The path of archive entry `name` under `root`: `/` and `\` separate folders, `.` and
/// empty parts are dropped. Refuses (with the reason) an absolute name, a drive, `..`, and
/// any part [`is_safe_component`] refuses: the result is always inside `root`.
pub fn join_under(root: &Path, name: &str) -> Result<PathBuf, String> {
    let n = name.replace('\\', "/");
    if n.contains('\0') {
        return Err(format!("nom d'entrée invalide : {name:?}"));
    }
    if n.starts_with('/') || is_remote(&n) {
        return Err(format!("chemin absolu refusé : {name}"));
    }
    let mut out = root.to_path_buf();
    let mut parts = 0;
    for c in n.split('/') {
        if c.is_empty() || c == "." {
            continue;
        }
        if c == ".." {
            return Err(format!("chemin qui sort du dossier refusé : {name}"));
        }
        if !is_safe_component(c) {
            return Err(format!("nom de fichier refusé : {name}"));
        }
        out.push(c);
        parts += 1;
    }
    if parts == 0 {
        return Err(format!("nom d'entrée vide : {name:?}"));
    }
    Ok(out)
}

/// `%XX` escapes decoded (byte by byte: a `%` before a non-ASCII character stays as it is).
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(h) = b.get(i + 1..i + 3)
            && let Some(v) = std::str::from_utf8(h).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ------------------------------------------------------------------------------ accesses

static REFUSED: Mutex<Vec<String>> = Mutex::new(Vec::new());
static OPENED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn note(log: &Mutex<Vec<String>>, p: &str) {
    let mut l = log.lock().unwrap_or_else(|e| e.into_inner());
    // A short memory: the tests look for their own (unique) paths.
    if l.len() >= 4096 {
        l.drain(..2048);
    }
    l.push(p.to_string());
}

/// Paths a file named and that were refused (network, device, relative), most recent last:
/// what the tests check, never a real connection.
#[doc(hidden)]
pub fn refused_paths() -> Vec<String> {
    REFUSED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Paths a file named that were opened.
#[doc(hidden)]
pub fn opened_paths() -> Vec<String> {
    OPENED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Notes a path a file named that was refused (the journal the tests read).
pub fn note_refused(path: &str) {
    note(&REFUSED, path);
}

/// `path` without the `\\?\` prefix Windows' `canonicalize` puts before a local drive
/// (`\\?\C:\x` → `C:\x`), so that a canonical local path is not taken for a device one.
/// `\\?\UNC\…` stays as it is.
pub fn strip_verbatim(path: &str) -> &str {
    match path.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') && rest.as_bytes()[0].is_ascii_alphabetic() => rest,
        _ => path,
    }
}

/// [`local_path`], noting the refusals.
pub fn check_referenced(path: &str) -> Option<PathBuf> {
    match local_path(path) {
        Some(p) => Some(p),
        None => {
            note(&REFUSED, path);
            None
        }
    }
}

/// Reads a file whose path a model or another file names (an environment's panorama, a
/// classification…): only a [`local_path`].
pub fn read_referenced(path: &str) -> std::io::Result<Vec<u8>> {
    let Some(p) = check_referenced(path) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("chemin refusé (réseau ou périphérique) : {path}"),
        ));
    };
    note(&OPENED, path);
    std::fs::read(p)
}

/// Whether a file a model names exists ([`local_path`] only).
pub fn referenced_is_file(path: &str) -> bool {
    check_referenced(path).is_some_and(|p| {
        note(&OPENED, path);
        p.is_file()
    })
}

/// When a file a model names was last modified ([`local_path`] only).
pub fn referenced_modified(path: &str) -> Option<std::time::SystemTime> {
    let p = check_referenced(path)?;
    note(&OPENED, path);
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

// ------------------------------------------------------------------------------ budget

/// What a file may make by decompressing, in all (an archive's entries, a compressed
/// stream): [`Budget::for_input`] grants about a hundred times the file's size, which real
/// files stay far below (XML deflates about 10:1, images not at all), and refuses the bombs
/// (deflate expands up to 1032 times, and an archive can name the same data again and again).
#[derive(Debug)]
pub struct Budget {
    left: AtomicU64,
}

impl Budget {
    /// Bytes allowed per byte of input.
    pub const RATIO: u64 = 100;
    /// Allowed whatever the input's size.
    pub const MARGIN: u64 = 256 << 20;
    /// Never more than this, whatever the input's size.
    pub const MAX: u64 = 4 << 30;

    pub fn new(bytes: u64) -> Budget {
        Budget { left: AtomicU64::new(bytes) }
    }

    /// The budget of an input of `len` bytes.
    pub fn for_input(len: usize) -> Budget {
        Budget::new((len as u64).saturating_mul(Self::RATIO).saturating_add(Self::MARGIN).min(Self::MAX))
    }

    pub fn left(&self) -> u64 {
        self.left.load(Ordering::Relaxed)
    }

    /// Spends `n` bytes, or refuses when fewer are left.
    pub fn take(&self, n: u64) -> Result<(), String> {
        self.left
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |l| l.checked_sub(n))
            .map(|_| ())
            .map_err(|_| "le fichier se décompresse en beaucoup trop de données".to_string())
    }

    /// Inflates raw deflate data (`zlib`: with its zlib header), at most `expected` bytes
    /// when the file declares the size, and what is left of the budget.
    pub fn inflate(&self, raw: &[u8], expected: Option<u64>, zlib: bool) -> Result<Vec<u8>, String> {
        let limit = self.left().min(expected.unwrap_or(u64::MAX));
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        let out = if zlib {
            miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(raw, limit)
        } else {
            miniz_oxide::inflate::decompress_to_vec_with_limit(raw, limit)
        };
        let out = out.map_err(|e| match e.status {
            miniz_oxide::inflate::TINFLStatus::HasMoreOutput => {
                if expected.is_some_and(|x| x <= self.left()) {
                    "données compressées plus grandes que la taille annoncée".to_string()
                } else {
                    "le fichier se décompresse en beaucoup trop de données".to_string()
                }
            }
            s => format!("données compressées invalides ({s:?})"),
        })?;
        self.take(out.len() as u64)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_and_device_paths_are_remote() {
        for p in [
            r"\\serveur\partage\x.hdr",
            "//serveur/partage/x.hdr",
            r"\/serveur\x",
            r"/\serveur\x",
            r"\\?\UNC\serveur\x",
            r"\\?\C:\x",
            r"\\.\pipe\x",
            r"\\.\COM1",
            r"\??\C:\x",
            "  \\\\serveur\\x",
            "file://serveur/x.png",
            "FILE://serveur/x.png",
            "file:////serveur/x.png",
            r"file:///\\serveur\x",
        ] {
            assert!(is_remote(p), "{p}");
            assert!(local_path(p).is_none(), "{p}");
        }
        for p in [r"C:\Users\x\a.hdr", "C:/x/a.png", "file:///C:/x/a.png", "a.png", r"x\..\y"] {
            assert!(!is_remote(p), "{p}");
        }
        assert!(local_path(r"C:\Users\x\a.hdr").is_some());
        assert_eq!(strip_verbatim(r"\\?\C:\x\a.usda"), r"C:\x\a.usda");
        assert!(is_remote(strip_verbatim(r"\\?\UNC\serveur\x")));
        // Relative (depends on the current folder), devices, nothing.
        for p in ["a.png", r"\x\a.png", r"C:\x\CON", r"C:\x\com1.txt", r"C:\x\NUL.png", "", "C:\\a\0b"] {
            assert!(local_path(p).is_none(), "{p:?}");
        }
    }

    #[test]
    fn reserved_names() {
        for n in ["CON", "con", "Con.txt", "NUL", "nul.tar.gz", "COM1", "lpt9.png", "AUX ", "CONIN$", "COM¹"] {
            assert!(is_reserved_name(n), "{n}");
        }
        for n in ["CONSOLE", "COM", "COM10", "LPTX", "nul_", "aux1", "bois.png"] {
            assert!(!is_reserved_name(n), "{n}");
        }
    }

    #[test]
    fn archive_names_stay_under_the_root() {
        let root = Path::new(r"C:\racine");
        assert_eq!(join_under(root, "a/b.rb").unwrap(), root.join("a").join("b.rb"));
        assert_eq!(join_under(root, r"./a\\b.rb").unwrap(), root.join("a").join("b.rb"));
        for bad in [
            "../evil.rb",
            "a/../../evil.rb",
            "/evil.rb",
            r"\evil.rb",
            r"C:\Windows\evil.dll",
            "C:evil.rb",
            r"\\serveur\partage\evil.rb",
            "//serveur/x",
            "a/CON",
            "a/nul.rb",
            "x.rb:Zone.Identifier",
            "a/.. /b",
            "a/b.",
            "a/b ",
            "a\u{1}b",
            "a/b?c",
            "",
            "./",
            "a\0b",
        ] {
            assert!(join_under(root, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn percent_escapes() {
        assert_eq!(percent_decode("bois%20clair.jpg"), "bois clair.jpg");
        // No panic on a '%' before a multibyte character, or at the end.
        assert_eq!(percent_decode("%aé%"), "%aé%");
        assert_eq!(percent_decode("%é1"), "%é1");
        assert_eq!(percent_decode("%C3%A9"), "é");
    }

    #[test]
    fn the_budget_is_shared_and_bounded() {
        let b = Budget::new(1000);
        b.take(600).unwrap();
        assert!(b.take(500).is_err());
        assert_eq!(b.left(), 400);
        // 1 MB of zeros deflates to about a kilobyte: refused past the budget.
        let packed = miniz_oxide::deflate::compress_to_vec(&vec![0u8; 1 << 20], 9);
        assert!(packed.len() < 4096);
        let b = Budget::new(64 * 1024);
        assert!(b.inflate(&packed, None, false).is_err());
        let b = Budget::new(4 << 20);
        assert_eq!(b.inflate(&packed, Some(1 << 20), false).unwrap().len(), 1 << 20);
        assert!(b.inflate(&packed, Some(1000), false).is_err(), "more than the declared size");
        let z = miniz_oxide::deflate::compress_to_vec_zlib(b"bonjour", 6);
        assert_eq!(Budget::new(100).inflate(&z, None, true).unwrap(), b"bonjour");
        assert!(Budget::for_input(10).left() >= Budget::MARGIN);
        assert_eq!(Budget::for_input(usize::MAX).left(), Budget::MAX);
    }

    #[test]
    fn referenced_files_are_read_only_when_local() {
        let unc = r"\\esq-test-untrusted\partage\x.hdr";
        assert!(read_referenced(unc).is_err());
        assert!(!referenced_is_file(unc));
        assert!(referenced_modified(unc).is_none());
        assert!(refused_paths().iter().any(|p| p == unc));
        assert!(!opened_paths().iter().any(|p| p == unc));
    }
}
