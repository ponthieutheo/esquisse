//! Prints every option of a .skp (rendering, units, page options, …) as read by the SDK:
//! `cargo run -p esq-skp --example dump_options -- model.skp [Provider]`
//! Handy to see what SketchUp stores in a style (e.g. `RenderingOptions`).

use std::ffi::{CString, c_char, c_void};
use std::path::PathBuf;

use libloading::Library;

type Ref = *mut c_void;

fn main() {
    let mut args = std::env::args().skip(1);
    let file = args.next().expect("usage: dump_options model.skp [Provider]");
    let only = args.next();
    let dll = PathBuf::from(
        std::env::var("ESQ_SKETCHUP_API")
            .unwrap_or_else(|_| r"C:\Program Files\SketchUp\SketchUp 2026\SketchUp\SketchUpAPI.dll".to_string()),
    );
    #[cfg(windows)]
    let lib: Library = unsafe {
        libloading::os::windows::Library::load_with_flags(&dll, libloading::os::windows::LOAD_WITH_ALTERED_SEARCH_PATH)
            .expect("SketchUpAPI.dll")
            .into()
    };
    #[cfg(not(windows))]
    let lib = unsafe { Library::new(&dll).expect("SketchUpAPI.dll") };
    macro_rules! f {
        ($name:literal, $ty:ty) => {
            *unsafe { lib.get::<$ty>(concat!($name, "\0").as_bytes()) }.expect($name)
        };
    }
    let init = f!("SUInitialize", unsafe extern "C" fn());
    let create = f!("SUModelCreateFromFileWithStatus", unsafe extern "C" fn(*mut Ref, *const c_char, *mut i32) -> i32);
    let get_om = f!("SUModelGetOptionsManager", unsafe extern "C" fn(Ref, *mut Ref) -> i32);
    let n_prov = f!("SUOptionsManagerGetNumOptionsProviders", unsafe extern "C" fn(Ref, *mut usize) -> i32);
    let get_names =
        f!("SUOptionsManagerGetOptionsProviderNames", unsafe extern "C" fn(Ref, usize, *mut Ref, *mut usize) -> i32);
    let by_name =
        f!("SUOptionsManagerGetOptionsProviderByName", unsafe extern "C" fn(Ref, *const c_char, *mut Ref) -> i32);
    let n_keys = f!("SUOptionsProviderGetNumKeys", unsafe extern "C" fn(Ref, *mut usize) -> i32);
    let get_keys = f!("SUOptionsProviderGetKeys", unsafe extern "C" fn(Ref, usize, *mut Ref, *mut usize) -> i32);
    let get_val = f!("SUOptionsProviderGetValue", unsafe extern "C" fn(Ref, *const c_char, *mut Ref) -> i32);
    let tv_create = f!("SUTypedValueCreate", unsafe extern "C" fn(*mut Ref) -> i32);
    let tv_type = f!("SUTypedValueGetType", unsafe extern "C" fn(Ref, *mut i32) -> i32);
    let tv_i32 = f!("SUTypedValueGetInt32", unsafe extern "C" fn(Ref, *mut i32) -> i32);
    let tv_f64 = f!("SUTypedValueGetDouble", unsafe extern "C" fn(Ref, *mut f64) -> i32);
    let tv_f32 = f!("SUTypedValueGetFloat", unsafe extern "C" fn(Ref, *mut f32) -> i32);
    let tv_bool = f!("SUTypedValueGetBool", unsafe extern "C" fn(Ref, *mut bool) -> i32);
    let tv_color = f!("SUTypedValueGetColor", unsafe extern "C" fn(Ref, *mut [u8; 4]) -> i32);
    let tv_str = f!("SUTypedValueGetString", unsafe extern "C" fn(Ref, *mut Ref) -> i32);
    let s_create = f!("SUStringCreate", unsafe extern "C" fn(*mut Ref) -> i32);
    let s_len = f!("SUStringGetUTF8Length", unsafe extern "C" fn(Ref, *mut usize) -> i32);
    let s_get = f!("SUStringGetUTF8", unsafe extern "C" fn(Ref, usize, *mut c_char, *mut usize) -> i32);

    let read = |s: Ref| -> String {
        let mut len = 0;
        unsafe { s_len(s, &mut len) };
        let mut buf = vec![0u8; len + 1];
        let mut n = 0;
        unsafe { s_get(s, buf.len(), buf.as_mut_ptr() as *mut c_char, &mut n) };
        buf.truncate(n.min(len));
        String::from_utf8_lossy(&buf).into_owned()
    };
    let text = |getter: &dyn Fn(*mut Ref) -> i32| -> String {
        let mut s: Ref = std::ptr::null_mut();
        unsafe { s_create(&mut s) };
        getter(&mut s);
        read(s)
    };

    unsafe { init() };
    let mut model: Ref = std::ptr::null_mut();
    let path = CString::new(file).unwrap();
    let mut status = 0;
    let r = unsafe { create(&mut model, path.as_ptr(), &mut status) };
    assert_eq!(r, 0, "cannot open the model (code {r})");
    let mut om: Ref = std::ptr::null_mut();
    unsafe { get_om(model, &mut om) };
    let mut n = 0;
    unsafe { n_prov(om, &mut n) };
    let mut names: Vec<Ref> = (0..n)
        .map(|_| {
            let mut s: Ref = std::ptr::null_mut();
            unsafe { s_create(&mut s) };
            s
        })
        .collect();
    let mut got = 0;
    unsafe { get_names(om, n, names.as_mut_ptr(), &mut got) };
    for &nm in &names[..got] {
        let name = read(nm);
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let mut p: Ref = std::ptr::null_mut();
        let cname = CString::new(name.clone()).unwrap();
        if unsafe { by_name(om, cname.as_ptr(), &mut p) } != 0 {
            continue;
        }
        println!("[{name}]");
        let mut nk = 0;
        unsafe { n_keys(p, &mut nk) };
        let mut keys = Vec::new();
        for _ in 0..nk {
            let mut s: Ref = std::ptr::null_mut();
            unsafe { s_create(&mut s) };
            keys.push(s);
        }
        let mut gk = 0;
        unsafe { get_keys(p, nk, keys.as_mut_ptr(), &mut gk) };
        for &k in &keys[..gk] {
            let key = read(k);
            let ck = CString::new(key.clone()).unwrap();
            let mut tv: Ref = std::ptr::null_mut();
            unsafe { tv_create(&mut tv) };
            if unsafe { get_val(p, ck.as_ptr(), &mut tv) } != 0 {
                println!("  {key} = ?");
                continue;
            }
            let mut ty = 0;
            unsafe { tv_type(tv, &mut ty) };
            let value = match ty {
                3 => {
                    let mut v = 0;
                    unsafe { tv_i32(tv, &mut v) };
                    v.to_string()
                }
                4 => {
                    let mut v = 0.0;
                    unsafe { tv_f32(tv, &mut v) };
                    v.to_string()
                }
                5 => {
                    let mut v = 0.0;
                    unsafe { tv_f64(tv, &mut v) };
                    v.to_string()
                }
                6 => {
                    let mut v = false;
                    unsafe { tv_bool(tv, &mut v) };
                    v.to_string()
                }
                7 => {
                    let mut c = [0u8; 4];
                    unsafe { tv_color(tv, &mut c) };
                    format!("rgba{c:?}")
                }
                9 => text(&|s| unsafe { tv_str(tv, s) }),
                t => format!("<type {t}>"),
            };
            println!("  {key} = {value}");
        }
    }
}
