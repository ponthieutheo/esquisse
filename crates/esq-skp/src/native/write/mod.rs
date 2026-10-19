//! Native writing of .skp files, without SketchUp: the current format (SketchUp 2021+, a ZIP
//! archive of tagged records, `vff`) and the MFC archive of every older version SketchUp 2026
//! saves (`legacy`). The model is first turned into SketchUp's objects (`plan`), as the SDK
//! writer makes them, then written in the version asked for. `docs/skp-format.md`, §8.
//!
//! The writer works on any model Esquisse holds: an empty name, unicode, a material without
//! its image are written; what a file cannot hold (a coordinate that is not finite, a
//! definition that contains itself) is an [`Error`](super::Error), never a panic.

pub mod legacy;
pub mod mfc;
pub mod plan;
pub mod rec;
pub mod versions;
pub mod vff;
pub mod xml;
pub mod zip;

use esq_core::model::Model;

use super::{Error, Result};
use crate::SkpVersion;

/// Stack of the thread that writes a model: the plan and the MFC archive are made recursively
/// over nested definitions (a definition is written where it is first placed). Reserved, not
/// committed: it costs address space only.
pub const WRITE_STACK: usize = 256 << 20;

/// Deepest nesting of definitions written; a model nested deeper is an error. The models
/// Esquisse reads are cut at 256 levels (`Model::check_after_reading`), a script can build
/// deeper ones. Far within `WRITE_STACK` even in a debug build: 4 096 levels are written in
/// every version (`tests/native_writer.rs`, `deep_nesting_is_written_from_a_small_stack`).
pub const MAX_NESTING: usize = 4096;

/// Runs `f` on a thread of `WRITE_STACK` and waits for it: the caller's stack (1 MB for the
/// main thread on Windows) is not used, and a panic becomes an error.
fn on_big_stack<T: Send>(f: impl FnOnce() -> T + Send) -> Result<T> {
    std::thread::scope(|s| {
        let h = std::thread::Builder::new()
            .name("esq-skp-ecriture".into())
            .stack_size(WRITE_STACK)
            .spawn_scoped(s, f)
            .map_err(|e| Error(format!("fil d'écriture : {e}")))?;
        h.join().map_err(|_| Error("erreur interne de l'écrivain".into()))
    })
}

/// What to write besides the model.
#[derive(Clone, Default)]
pub struct Extras {
    /// The model's preview (PNG), for the Explorer and SketchUp's dialogs.
    pub thumbnail: Option<Vec<u8>>,
}

/// The bytes of a .skp of `version` (`None`: the current format).
pub fn write(
    model: &Model,
    view: Option<&crate::View>,
    version: Option<SkpVersion>,
    extras: &Extras,
) -> Result<Vec<u8>> {
    // One thread for the plan and the file: an MFC archive writes a definition where it is
    // first referred to, inside the instance that refers to it, so the writing nests as deep
    // as the model (`MAX_NESTING` at most, checked by the plan).
    on_big_stack(|| {
        let mut p = plan::plan_here(model, view)?;
        p.thumbnail = extras.thumbnail.clone();
        Ok(match version {
            None | Some(SkpVersion::Su2021) => vff::file(&p),
            Some(v) => legacy::file(&p, v),
        })
    })?
}
