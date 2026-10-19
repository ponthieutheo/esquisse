//! Paths written in a model file — the file a component was loaded from, an environment's or a
//! classification's file — come from whoever made the file. Esquisse touches the disk with one
//! only when it is a plain local path: `\\server\share\…` (or `//…`, `\\?\…`, `\\.\…`) would
//! make Windows connect to that server, and send it the user's NTLM hash, merely because a
//! panel is drawn.
//!
//! Every read of such a path goes through [`read`] or [`exists`], which count what they are
//! asked in tests ([`asked`]). The rule itself is the common one of `esq_core::untrusted`
//! (`docs/securite.md`), which also refuses device names (`CON`, `COM1`…) and
//! `file://serveur/` and keeps its own journal of refusals.

use std::path::{Component, Path};

/// The path, if it is a local absolute one (a drive letter on Windows); `None` for a network
/// (UNC) or device path, and for a relative one.
pub fn local(p: &str) -> Option<&Path> {
    let s = p.trim();
    // The common rule (it notes the refusal in its journal).
    esq_core::untrusted::check_referenced(s)?;
    let path = Path::new(s);
    let first = path.components().next()?;
    if cfg!(windows) {
        match first {
            Component::Prefix(px) => matches!(px.kind(), std::path::Prefix::Disk(_)).then_some(path),
            _ => None,
        }
    } else {
        matches!(first, Component::RootDir).then_some(path)
    }
}

#[cfg(test)]
thread_local! {
    static ASKED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn note(p: &str) {
    #[cfg(test)]
    ASKED.with(|a| a.borrow_mut().push(p.to_string()));
    let _ = p;
}

/// Tests: the model paths this thread asked to read or look at so far (and forgets them).
#[cfg(test)]
pub fn asked() -> Vec<String> {
    ASKED.with(|a| std::mem::take(&mut *a.borrow_mut()))
}

/// The bytes of a file named by a model, if its path is local.
pub fn read(p: &str) -> Option<Vec<u8>> {
    note(p);
    local(p)?;
    esq_core::untrusted::read_referenced(p.trim()).ok()
}

/// Whether a file named by a model is there, if its path is local.
pub fn exists(p: &str) -> bool {
    note(p);
    local(p).is_some() && esq_core::untrusted::referenced_is_file(p.trim())
}

/// When a file named by a model last changed, if its path is local.
pub fn modified(p: &str) -> Option<std::time::SystemTime> {
    note(p);
    local(p)?;
    esq_core::untrusted::referenced_modified(p.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_and_device_paths_are_refused() {
        for p in [
            r"\\127.0.0.1\x\y",
            r"\\attaquant\partage\x.png",
            "//attaquant/partage/x.png",
            r"\/attaquant/x",
            r"\\?\C:\x.png",
            r"\\?\UNC\serveur\x",
            r"\\.\pipe\x",
            r"\\.\C:\x",
            "relatif\\x.png",
            "",
        ] {
            assert!(local(p).is_none(), "{p}");
            assert!(read(p).is_none() && !exists(p) && modified(p).is_none(), "{p}");
        }
        if cfg!(windows) {
            assert!(local(r"C:\Users\x\Composants\chaise.esq").is_some());
            assert!(local("C:/Users/x/chaise.esq").is_some());
        }
        assert_eq!(asked().len(), 30);
    }
}
