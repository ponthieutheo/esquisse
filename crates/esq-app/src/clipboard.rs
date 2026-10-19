//! Edit › Copy / Paste between two windows of Esquisse, through the Windows clipboard, as
//! SketchUp does it (`QtClipboard`, read in SketchUp.exe): SketchUp puts its selection under
//! a private format, `SketchUp_Data` (Qt registers the MIME name as a Windows clipboard format,
//! `RegisterClipboardFormatW(L"SketchUp_Data")`), with a plain text for other programs
//! (« [SketchUp Geometry - Paste inside SketchUp to view] ») and an HTML copy whose
//! `data-su-blob-v1="skp:…"` attribute holds the same bytes in base 64; Paste reads the
//! format, else that attribute.
//!
//! Esquisse does the same with its own format, `Esquisse_Data`: the selection as a model of
//! its own (the copied geometry, its definitions, materials and tags), compressed. Its content
//! is not SketchUp's (a `.skp`), so it does not take SketchUp's name, which would make
//! SketchUp read it; but a selection SketchUp copied (`SketchUp_Data`, a `.skp`) pastes into
//! Esquisse through the native `.skp` reader.
//!
//! What another window put on the clipboard is untrusted, like a file opened (`docs/securite.md`):
//! its size is bounded, it inflates within an `untrusted::Budget`, and the model goes through
//! `esq_core::io::load_esq_bytes` (`Model::check_after_reading`) or the `.skp` reader's own
//! checks before anything uses it; nothing in it can panic.

use esq_core::model::{DefId, Model};

/// Our private clipboard format.
pub const FORMAT: &str = "Esquisse_Data";
/// SketchUp's (read only).
pub const SKETCHUP_FORMAT: &str = "SketchUp_Data";
/// What other programs see.
const TEXT: &str = "[Géométrie Esquisse - collez-la dans Esquisse pour la voir]";
/// The header of our bytes, and the largest clipboard content read (as a model file would be).
const MAGIC: &[u8; 8] = b"ESQCLIP1";
const MAX_BYTES: usize = 1 << 30;

/// Where the bytes go: the Windows clipboard, or in tests a clipboard of the process (the
/// tests never touch the user's clipboard).
pub trait Store {
    /// Replaces the clipboard's content: `bytes` under `format`, `text` for other programs.
    fn put(&self, format: &str, bytes: &[u8], text: &str) -> bool;
    /// The bytes under `format`, if any (at most `max`).
    fn get(&self, format: &str, max: usize) -> Option<Vec<u8>>;
    /// Whether the clipboard holds `format`.
    fn has(&self, format: &str) -> bool;
    /// A number that changes with every change of the clipboard.
    fn sequence(&self) -> u32;
}

/// The selection copied as bytes: magic, the length of the model, the model (`.esq` JSON)
/// deflated. `clip` holds one instance of `def` at its root.
pub fn encode(clip: &Model, def: DefId) -> Option<Vec<u8>> {
    let mut m = clip.clone();
    // The root holds one copy of the copied definition: what paste takes.
    m.root = Default::default();
    m.root.instances.insert(esq_core::model::Instance {
        def,
        xform: esq_core::glam::DMat4::IDENTITY,
        name: String::new(),
        material: None,
        layer: 0,
        hidden: false,
        locked: false,
        attributes: Default::default(),
        ext: Default::default(),
    });
    let json = serde_json::to_vec(&m).ok()?;
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&(json.len() as u64).to_le_bytes());
    out.extend(miniz_oxide::deflate::compress_to_vec(&json, 6));
    Some(out)
}

/// What [`encode`] wrote, read back as a model of a file would be. `Err` says why not.
pub fn decode(bytes: &[u8]) -> Result<(Model, DefId), String> {
    if bytes.len() > MAX_BYTES {
        return Err("le presse-papiers contient beaucoup trop de données".into());
    }
    let rest = bytes.strip_prefix(MAGIC.as_slice()).ok_or("contenu du presse-papiers inconnu")?;
    let (len, raw) = rest.split_first_chunk::<8>().ok_or("contenu du presse-papiers tronqué")?;
    let len = u64::from_le_bytes(*len);
    let budget = esq_core::untrusted::Budget::for_input(bytes.len());
    let json = budget.inflate(raw, Some(len), false)?;
    if json.len() as u64 != len {
        return Err("contenu du presse-papiers tronqué".into());
    }
    let m = esq_core::io::load_esq_bytes(&json).map_err(|e| format!("{e:#}"))?;
    let mut roots = m.root.instances.values().map(|i| i.def);
    match (roots.next(), roots.next()) {
        (Some(d), None) if m.defs.contains_key(d) => Ok((m, d)),
        _ => Err("contenu du presse-papiers invalide".into()),
    }
}

/// A selection SketchUp copied: its `.skp`, the whole of it one definition to paste.
pub fn decode_sketchup(bytes: &[u8]) -> Result<(Model, DefId), String> {
    if bytes.len() > MAX_BYTES {
        return Err("le presse-papiers contient beaucoup trop de données".into());
    }
    let (mut src, _) = esq_skp::native::read_bytes(bytes).map_err(|e| e.0.clone())?;
    // The same checks as a .skp opened (esq_skp::import_skp_view).
    if src.check_after_reading()? > 0 {
        src.rebuild_topology();
    }
    let mut m = Model::new();
    let d = m.import_as_component(&src, "Presse-papiers");
    Ok((m, d))
}

/// Puts the copied selection on the clipboard.
pub fn put(store: &dyn Store, clip: &Model, def: DefId) -> bool {
    encode(clip, def).is_some_and(|b| store.put(FORMAT, &b, TEXT))
}

/// What the clipboard holds that Esquisse can paste: ours first, else SketchUp's.
pub fn take(store: &dyn Store) -> Option<Result<(Model, DefId), String>> {
    if let Some(b) = store.get(FORMAT, MAX_BYTES) {
        return Some(decode(&b));
    }
    store.get(SKETCHUP_FORMAT, MAX_BYTES).map(|b| decode_sketchup(&b))
}

/// Whether the clipboard holds something to paste.
pub fn has_any(store: &dyn Store) -> bool {
    store.has(FORMAT) || store.has(SKETCHUP_FORMAT)
}

// ------------------------------------------------------------------------------------------
// The Windows clipboard.

#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
#[allow(clippy::upper_case_acronyms)]
pub mod win {

    use std::ffi::c_void;

    type HWND = isize;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn OpenClipboard(hwnd: HWND) -> i32;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn GetClipboardData(format: u32) -> *mut c_void;
        fn SetClipboardData(format: u32, mem: *mut c_void) -> *mut c_void;
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
        fn GetClipboardSequenceNumber() -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
        fn GlobalLock(mem: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(mem: *mut c_void) -> i32;
        fn GlobalFree(mem: *mut c_void) -> *mut c_void;
        fn GlobalSize(mem: *mut c_void) -> usize;
    }

    const CF_UNICODETEXT: u32 = 13;
    const GMEM_MOVEABLE: u32 = 0x0002;

    fn format_id(name: &str) -> u32 {
        let w: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // SAFETY: NUL-terminated name.
        unsafe { RegisterClipboardFormatW(w.as_ptr()) }
    }

    /// A global block holding `data`, or null.
    ///
    /// # Safety
    /// The caller gives the block to the clipboard or frees it.
    unsafe fn global(data: &[u8]) -> *mut c_void {
        // SAFETY: the block is allocated for exactly `data` (at least one byte).
        unsafe {
            let mem = GlobalAlloc(GMEM_MOVEABLE, data.len().max(1));
            if mem.is_null() {
                return mem;
            }
            let p = GlobalLock(mem) as *mut u8;
            if p.is_null() {
                GlobalFree(mem);
                return std::ptr::null_mut();
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
            GlobalUnlock(mem);
            mem
        }
    }

    pub struct Windows;

    impl super::Store for Windows {
        fn put(&self, format: &str, bytes: &[u8], text: &str) -> bool {
            let f = format_id(format);
            if f == 0 {
                return false;
            }
            let wide: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
            // SAFETY: the clipboard protocol: open, empty, give each block (the clipboard owns
            // it once given; a block not given is freed), close.
            unsafe {
                if OpenClipboard(0) == 0 {
                    return false;
                }
                EmptyClipboard();
                let mut ok = false;
                for (fmt, data) in [(f, bytes), (CF_UNICODETEXT, wide.as_slice())] {
                    let mem = global(data);
                    if mem.is_null() {
                        continue;
                    }
                    if SetClipboardData(fmt, mem).is_null() {
                        GlobalFree(mem);
                    } else if fmt == f {
                        ok = true;
                    }
                }
                CloseClipboard();
                ok
            }
        }

        fn get(&self, format: &str, max: usize) -> Option<Vec<u8>> {
            let f = format_id(format);
            // SAFETY: the block is only read while locked, within its size.
            unsafe {
                if f == 0 || IsClipboardFormatAvailable(f) == 0 || OpenClipboard(0) == 0 {
                    return None;
                }
                let mut out = None;
                let h = GetClipboardData(f);
                if !h.is_null() {
                    let n = GlobalSize(h);
                    let p = GlobalLock(h) as *const u8;
                    if !p.is_null() {
                        if n <= max {
                            out = Some(std::slice::from_raw_parts(p, n).to_vec());
                        }
                        GlobalUnlock(h);
                    }
                }
                CloseClipboard();
                out
            }
        }

        fn has(&self, format: &str) -> bool {
            let f = format_id(format);
            // SAFETY: plain query.
            f != 0 && unsafe { IsClipboardFormatAvailable(f) } != 0
        }

        fn sequence(&self) -> u32 {
            // SAFETY: plain query.
            unsafe { GetClipboardSequenceNumber() }
        }
    }
}

// ------------------------------------------------------------------------------------------
// The clipboard of the running application.

/// The clipboard Esquisse uses: Windows' in the application, the test's own in tests.
pub fn with<R>(f: impl FnOnce(Option<&dyn Store>) -> R) -> R {
    #[cfg(test)]
    {
        let fake = testing::SHARED.with(|s| s.borrow().clone());
        f(fake.as_deref().map(|x| x as &dyn Store))
    }
    #[cfg(all(not(test), windows))]
    {
        f(Some(&win::Windows))
    }
    #[cfg(all(not(test), not(windows)))]
    {
        f(None)
    }
}

#[cfg(test)]
pub mod testing {
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A clipboard shared by the windows of a test, never the user's.
    #[derive(Default)]
    pub struct Fake {
        pub data: Mutex<(u32, HashMap<String, Vec<u8>>, String)>,
        /// How many times a window read the clipboard's bytes.
        pub reads: std::sync::atomic::AtomicUsize,
    }

    impl super::Store for Fake {
        fn put(&self, format: &str, bytes: &[u8], text: &str) -> bool {
            let mut d = self.data.lock().unwrap();
            d.0 += 1;
            d.1.clear();
            d.1.insert(format.into(), bytes.to_vec());
            d.2 = text.into();
            true
        }
        fn get(&self, format: &str, max: usize) -> Option<Vec<u8>> {
            self.reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.data.lock().unwrap().1.get(format).filter(|b| b.len() <= max).cloned()
        }
        fn has(&self, format: &str) -> bool {
            self.data.lock().unwrap().1.contains_key(format)
        }
        fn sequence(&self) -> u32 {
            self.data.lock().unwrap().0
        }
    }

    impl Fake {
        /// Another program copied something else (text).
        pub fn other_program_copies(&self, text: &str) {
            let mut d = self.data.lock().unwrap();
            d.0 += 1;
            d.1.clear();
            d.2 = text.into();
        }
        /// Raw bytes under a format (a hostile or foreign content).
        pub fn put_raw(&self, format: &str, bytes: Vec<u8>) {
            let mut d = self.data.lock().unwrap();
            d.0 += 1;
            d.1.clear();
            d.1.insert(format.into(), bytes);
        }
    }

    thread_local! {
        /// The clipboard the test's windows share (`set`): without one, a test's window has
        /// none (and never reaches the user's).
        pub static SHARED: std::cell::RefCell<Option<std::sync::Arc<Fake>>> = const { std::cell::RefCell::new(None) };
    }

    pub fn set(f: Option<std::sync::Arc<Fake>>) {
        SHARED.with(|s| *s.borrow_mut() = f);
    }
}
