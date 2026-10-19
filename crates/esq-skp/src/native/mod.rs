//! Native reading of .skp files, without SketchUp: the container (`container`, `zip`), the
//! tagged model of SketchUp 2021+ (`tlv`, `vff`) and the MFC archive of the earlier versions.
//! Both are read into a [`doc::Doc`] (SketchUp's own objects), which `build` turns into an
//! Esquisse model the way the SDK reader does. The format is described in `docs/skp-format.md`.
//!
//! Every reader works on untrusted bytes: lengths, counts and indices are checked, recursion
//! is bounded, and a malformed file gives an [`Error`], never a panic.

use std::path::Path;

use esq_core::model::Model;

pub(crate) mod attach;
pub mod build;
pub mod canon;
pub mod container;
pub(crate) mod curves;
pub mod doc;
pub(crate) mod faces;
pub mod fonts;
pub mod jpeg;
pub mod legacy;
pub mod mfc;
pub mod style;
pub mod tlv;
pub mod vff;
pub mod write;
pub mod xml;
pub mod zip;

/// A malformed or unsupported file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, ".skp illisible : {}", self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn bad(what: &str) -> Error {
    Error(what.to_string())
}

/// What reading one file may make beyond the file itself, shared by everything the reading of
/// that file does: the bytes of the objects it builds (an object referred to several times and
/// copied for each reference is counted each time) and the pixels it decodes. A forged file
/// that would exceed the bytes is refused; past the pixels, images are no longer decoded.
pub struct Budget {
    bytes: std::cell::Cell<u64>,
    pixels: std::cell::Cell<u64>,
}

/// Bytes of objects allowed per byte of the file (real files stay well below: an edge of an
/// MFC archive takes ~30 bytes of the file and ~150 in memory), plus a fixed allowance.
const BYTES_PER_INPUT_BYTE: u64 = 32;
const BYTES_FIXED: u64 = 64 << 20;
/// Pixels decoded per file: the textures whose average colour or transparency SketchUp works
/// out from the image (files before 2021). Some seconds of decoding at most; a small file
/// gets a few megapixels (its images are small too).
pub const MAX_PIXELS_PER_FILE: u64 = 1 << 28;
const PIXELS_FIXED: u64 = 1 << 24;
const PIXELS_PER_INPUT_BYTE: u64 = 32;

impl Budget {
    /// The budget of a file of `len` bytes.
    pub fn for_input(len: usize) -> Budget {
        let len = len as u64;
        let bytes = len.saturating_mul(BYTES_PER_INPUT_BYTE).saturating_add(BYTES_FIXED);
        let pixels = len.saturating_mul(PIXELS_PER_INPUT_BYTE).saturating_add(PIXELS_FIXED).min(MAX_PIXELS_PER_FILE);
        Budget { bytes: std::cell::Cell::new(bytes), pixels: std::cell::Cell::new(pixels) }
    }

    /// Counts `n` bytes made; an error past the budget.
    pub fn charge(&self, n: u64) -> Result<()> {
        let left = self.bytes.get();
        if n > left {
            self.bytes.set(0);
            return Err(bad("le fichier produit bien plus de données qu'il n'en contient"));
        }
        self.bytes.set(left - n);
        Ok(())
    }

    /// Takes `n` pixels to decode, if the budget has them.
    pub fn take_pixels(&self, n: u64) -> bool {
        let left = self.pixels.get();
        if n > left {
            return false;
        }
        self.pixels.set(left - n);
        true
    }

    /// Bytes still allowed (tests).
    pub fn bytes_left(&self) -> u64 {
        self.bytes.get()
    }
}

/// How to read.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Reproduce what the SDK reader loses through the C API's own defects (the differential
    /// test compares like with like). Off by default: the native reader keeps what the file
    /// holds. Known defect: `SUEntityGetAttributeDictionaries` drops the dictionaries that
    /// follow a texture placement in a face's attribute container.
    pub sdk_quirks: bool,
}

/// The GUID of the model of a .skp file, without reading the model: in the header of an MFC
/// archive, in `meta/meta.dat` for 2021+.
pub fn guid(data: &[u8]) -> Result<String> {
    let f = container::open(data)?;
    Ok(match &f.body {
        container::Body::Zip { archive, .. } => vff::guid(archive),
        container::Body::Legacy { guid, .. } => legacy::guid_text(guid),
    })
}

/// The objects of a .skp file.
pub fn read_doc(data: &[u8], opts: &Options) -> Result<doc::Doc> {
    let f = container::open(data)?;
    match &f.body {
        container::Body::Zip { archive, .. } => {
            let model = archive.read_name("model.dat")?;
            vff::read(&model, Some(archive), f.version, opts)
        }
        // Objects are read where they are first referred to: a component inside a component
        // reads its definition there, so the reading nests as deep as the model.
        container::Body::Legacy { guid, rest } => legacy::read(rest, guid, f.version, opts),
    }
}

/// Stack of the thread that builds a model (recursive over nested definitions): reserved, not
/// committed, so it costs address space only.
pub(crate) const BIG_STACK: usize = 256 << 20;

/// Runs `f` on a thread with a large stack and waits for it: deep (forged) nesting cannot
/// overflow the caller's stack, and a panic becomes an error.
pub(crate) fn on_big_stack<T: Send>(f: impl FnOnce() -> T + Send) -> Result<T> {
    std::thread::scope(|s| {
        let h = std::thread::Builder::new()
            .name("esq-skp-lecture".into())
            .stack_size(BIG_STACK)
            .spawn_scoped(s, f)
            .map_err(|e| Error(format!("fil de lecture : {e}")))?;
        h.join().map_err(|_| bad("erreur interne du lecteur"))
    })
}

/// Opens a .skp file's bytes: the model and the camera it was saved with.
pub fn read_bytes_with(data: &[u8], opts: &Options) -> Result<(Model, Option<crate::View>)> {
    let doc = read_doc(data, opts)?;
    Ok((build::model(&doc)?, build::view(&doc)))
}

/// Opens a .skp file's bytes.
pub fn read_bytes(data: &[u8]) -> Result<(Model, Option<crate::View>)> {
    read_bytes_with(data, &Options::default())
}

/// Opens a .skp file.
pub fn read_file(path: &Path) -> anyhow::Result<(Model, Option<crate::View>)> {
    let data = std::fs::read(path)?;
    Ok(read_bytes(&data)?)
}
