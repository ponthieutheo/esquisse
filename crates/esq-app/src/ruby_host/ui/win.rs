//! The little bit of Win32 that `UI.*` needs and that no dependency of the app provides:
//! message boxes, the system beep and sounds, the clipboard, the window's DPI and a window
//! capture. Declared by hand (user32/kernel32/winmm/gdi32), like `esq-ruby`'s Ruby loader.

#![allow(clippy::upper_case_acronyms)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};

type HWND = isize;

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(hwnd: HWND, text: *const u16, caption: *const u16, utype: u32) -> i32;
    fn MessageBeep(utype: u32) -> i32;
    fn OpenClipboard(hwnd: HWND) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn GetClipboardData(format: u32) -> *mut c_void;
    fn SetClipboardData(format: u32, mem: *mut c_void) -> *mut c_void;
    fn GetActiveWindow() -> HWND;
    fn GetDpiForWindow(hwnd: HWND) -> u32;
    fn GetWindowRect(hwnd: HWND, rect: *mut [i32; 4]) -> i32;
    fn GetDC(hwnd: HWND) -> *mut c_void;
    fn ReleaseDC(hwnd: HWND, dc: *mut c_void) -> i32;
    fn PrintWindow(hwnd: HWND, dc: *mut c_void, flags: u32) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
    fn GlobalLock(mem: *mut c_void) -> *mut c_void;
    fn GlobalUnlock(mem: *mut c_void) -> i32;
    fn GlobalFree(mem: *mut c_void) -> *mut c_void;
}

#[link(name = "winmm")]
unsafe extern "system" {
    fn PlaySoundW(name: *const u16, module: isize, flags: u32) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(dc: *mut c_void) -> *mut c_void;
    fn CreateCompatibleBitmap(dc: *mut c_void, w: i32, h: i32) -> *mut c_void;
    fn SelectObject(dc: *mut c_void, obj: *mut c_void) -> *mut c_void;
    fn DeleteObject(obj: *mut c_void) -> i32;
    fn DeleteDC(dc: *mut c_void) -> i32;
    fn GetDIBits(
        dc: *mut c_void,
        bmp: *mut c_void,
        start: u32,
        lines: u32,
        bits: *mut c_void,
        info: *mut BitmapInfo,
        usage: u32,
    ) -> i32;
}

#[repr(C)]
struct BitmapInfo {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_ppm: i32,
    y_ppm: i32,
    clr_used: u32,
    clr_important: u32,
    colors: [u32; 3],
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);

/// Remembers the app's window; called from the UI thread, where `GetActiveWindow` is it.
pub fn note_main_window() {
    if MAIN_WINDOW.load(Ordering::Relaxed) == 0 {
        // SAFETY: plain query on the calling thread.
        let h = unsafe { GetActiveWindow() };
        if h != 0 {
            MAIN_WINDOW.store(h, Ordering::Relaxed);
        }
    }
}

pub fn main_window() -> isize {
    MAIN_WINDOW.load(Ordering::Relaxed)
}

/// `MessageBoxW`; `kind` is SketchUp's `MB_*` (the same numbers as Win32's). Returns `ID*`.
pub fn message_box(text: &str, caption: &str, kind: u32) -> i32 {
    let (t, c) = (wide(text), wide(caption));
    // MB_SETFOREGROUND: the box comes up in front even though the app's thread is busy.
    // SAFETY: both strings are NUL-terminated and live for the call.
    unsafe { MessageBoxW(0, t.as_ptr(), c.as_ptr(), (kind & 0xF) | 0x0001_0000) }
}

pub fn beep() {
    // SAFETY: plain call.
    unsafe { MessageBeep(0) };
}

/// Plays a .wav file without blocking.
pub fn play_sound(path: &str) -> bool {
    let p = wide(path);
    // SND_ASYNC | SND_FILENAME | SND_NODEFAULT
    // SAFETY: NUL-terminated path.
    unsafe { PlaySoundW(p.as_ptr(), 0, 0x0001 | 0x0002_0000 | 0x0002) != 0 }
}

pub fn clipboard_get() -> Option<String> {
    // SAFETY: standard clipboard protocol; the global block is only read while locked.
    unsafe {
        if OpenClipboard(0) == 0 {
            return None;
        }
        let mut out = None;
        let h = GetClipboardData(13); // CF_UNICODETEXT
        if !h.is_null() {
            let p = GlobalLock(h) as *const u16;
            if !p.is_null() {
                let mut n = 0;
                while *p.add(n) != 0 {
                    n += 1;
                }
                out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                GlobalUnlock(h);
            }
        }
        CloseClipboard();
        out
    }
}

pub fn clipboard_set(text: &str) -> bool {
    let w = wide(text);
    // SAFETY: the block is allocated for exactly `w`; on success the clipboard owns it.
    unsafe {
        if OpenClipboard(0) == 0 {
            return false;
        }
        EmptyClipboard();
        let mem = GlobalAlloc(0x0002, w.len() * 2); // GMEM_MOVEABLE
        let mut ok = false;
        if !mem.is_null() {
            let p = GlobalLock(mem) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
                GlobalUnlock(mem);
                ok = !SetClipboardData(13, mem).is_null();
            }
            if !ok {
                GlobalFree(mem);
            }
        }
        CloseClipboard();
        ok
    }
}

/// Display scale (1.0 = 96 dpi) of the app's window, 1.0 when unknown.
pub fn scale_factor() -> f64 {
    let h = main_window();
    if h == 0 {
        return 1.0;
    }
    // SAFETY: plain query.
    let dpi = unsafe { GetDpiForWindow(h) };
    if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 }
}

/// Screenshot of a window (the web view included) as RGBA pixels.
pub fn capture_window(hwnd: isize) -> Option<(u32, u32, Vec<u8>)> {
    // SAFETY: GDI objects are created, used and released inside this block.
    unsafe {
        let mut r = [0i32; 4];
        if GetWindowRect(hwnd, &mut r) == 0 {
            return None;
        }
        let (w, h) = ((r[2] - r[0]).max(1), (r[3] - r[1]).max(1));
        let screen = GetDC(0);
        let dc = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(dc, bmp);
        // PW_RENDERFULLCONTENT: includes DirectComposition content (WebView2).
        let ok = PrintWindow(hwnd, dc, 2) != 0;
        let mut info = BitmapInfo {
            size: std::mem::size_of::<BitmapInfo>() as u32 - 12,
            width: w,
            height: -h,
            planes: 1,
            bit_count: 32,
            compression: 0,
            size_image: 0,
            x_ppm: 0,
            y_ppm: 0,
            clr_used: 0,
            clr_important: 0,
            colors: [0; 3],
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let lines = if ok { GetDIBits(dc, bmp, 0, h as u32, px.as_mut_ptr() as *mut c_void, &mut info, 0) } else { 0 };
        SelectObject(dc, old);
        DeleteObject(bmp);
        DeleteDC(dc);
        ReleaseDC(0, screen);
        if lines == 0 {
            return None;
        }
        for p in px.as_chunks_mut::<4>().0 {
            p.swap(0, 2); // BGRA -> RGBA
            p[3] = 255;
        }
        Some((w as u32, h as u32, px))
    }
}
