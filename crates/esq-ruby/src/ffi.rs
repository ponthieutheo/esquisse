//! Minimal binding to the Ruby C API, loaded at run time from `x64-ucrt-ruby320.dll`
//! (the same runtime SketchUp ships, so extensions see the interpreter they expect).

use std::ffi::{CStr, CString, c_char, c_int, c_long, c_void};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use libloading::Library;

pub type Value = usize;
/// Native method taking `self` and two arguments.
pub type Method2 = unsafe extern "C" fn(Value, Value, Value) -> Value;

pub const QNIL: Value = 4;
pub const QTRUE: Value = 0x14;

type StrCstr = unsafe extern "C" fn(*mut Value) -> *const c_char;
type StrNew = unsafe extern "C" fn(*const c_char, c_long) -> Value;
/// A function run under `rb_protect`. "C-unwind": Ruby leaves it by `longjmp` when it
/// raises, which only crosses this frame (no destructor in it).
type Protected = unsafe extern "C-unwind" fn(Value) -> Value;
type Protect = unsafe extern "C" fn(Protected, Value, *mut c_int) -> Value;

/// The C functions the protected calls need (set once, when the interpreter starts).
static FNS: std::sync::OnceLock<(StrCstr, unsafe extern "C" fn(Value) -> Value)> = std::sync::OnceLock::new();

/// `rb_string_value_cstr`, run by `rb_protect`: it raises for a string with a NUL byte.
unsafe extern "C-unwind" fn cstr_of(v: Value) -> Value {
    let mut v = v;
    match FNS.get() {
        // SAFETY: called by rb_protect on the interpreter thread with a String.
        Some((cstr, _)) => unsafe { cstr(&mut v) as Value },
        None => 0,
    }
}

/// `rb_inspect`, run by `rb_protect`: an object's `inspect` may raise.
unsafe extern "C-unwind" fn inspect_of(v: Value) -> Value {
    match FNS.get() {
        // SAFETY: called by rb_protect on the interpreter thread.
        Some((_, inspect)) => unsafe { inspect(v) },
        None => QNIL,
    }
}

pub struct Ruby {
    _lib: Library,
    define_module: unsafe extern "C" fn(*const c_char) -> Value,
    define_module_function: unsafe extern "C" fn(Value, *const c_char, Method2, c_int),
    eval_string_protect: unsafe extern "C" fn(*const c_char, *mut c_int) -> Value,
    utf8_str_new: StrNew,
    errinfo: unsafe extern "C" fn() -> Value,
    set_errinfo: unsafe extern "C" fn(Value),
    strings: Strings,
}

/// What reading a Ruby string safely takes: a `rb_string_value_cstr` that raised (an object
/// that is not a String, a string with a NUL byte) left the Rust function that called it by
/// `longjmp`, skipping its destructors (undefined behaviour). Now the class is checked first
/// (`rb_obj_is_kind_of`, which raises nothing) and the conversion runs under `rb_protect`.
#[derive(Clone, Copy)]
struct Strings {
    protect: Protect,
    is_kind_of: unsafe extern "C" fn(Value, Value) -> Value,
    /// Address of the `rb_cString` global.
    c_string: *const Value,
    set_errinfo: unsafe extern "C" fn(Value),
}

// SAFETY: plain function pointers and the address of a global of the dll, used only on the
// interpreter's thread.
unsafe impl Send for Strings {}
unsafe impl Sync for Strings {}

impl Strings {
    /// Runs `f(v)` under `rb_protect`; `None` when it raised (the error is cleared).
    fn protect(&self, f: Protected, v: Value) -> Option<Value> {
        let mut state: c_int = 0;
        // SAFETY: rb_protect catches what `f` raises; `f` has no destructor to skip.
        let r = unsafe { (self.protect)(f, v, &mut state) };
        if state != 0 {
            // SAFETY: interpreter thread.
            unsafe { (self.set_errinfo)(QNIL) };
            return None;
        }
        Some(r)
    }

    /// The text of `v` when it is a String without NUL byte.
    fn read(&self, v: Value) -> Option<String> {
        if v == QNIL || v == 0 {
            return None;
        }
        // SAFETY: rb_obj_is_kind_of compares classes without calling Ruby code; rb_cString is
        // set once the interpreter is initialised.
        if unsafe { (self.is_kind_of)(v, *self.c_string) } != QTRUE {
            return None;
        }
        let p = self.protect(cstr_of, v)? as *const c_char;
        if p.is_null() {
            return None;
        }
        // SAFETY: NUL-terminated by rb_string_value_cstr; `v` is alive during the call.
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
}

/// The Ruby runtime's dll (RubyInstaller's build, the one SketchUp ships).
pub const RUBY_DLL: &str = "x64-ucrt-ruby320.dll";

/// Where a Ruby 3.2 runtime might live, best first: `ESQ_RUBY_DLL`, then the runtime shipped
/// next to the executable (`ruby\bin\x64-ucrt-ruby320.dll` with its `lib\ruby`, as
/// `tools/package.ps1` lays it out). The test and example programs of a build tree
/// (`target\<profile>\deps`, `…\examples`) also look in the build folder above them, where
/// `tools/ruby-dev.ps1` puts a runtime for development.
pub fn candidate_dlls() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var("ESQ_RUBY_DLL") {
        v.push(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        v.push(dir.join("ruby").join("bin").join(RUBY_DLL));
        v.push(dir.join(RUBY_DLL));
        if dir.file_name().is_some_and(|n| n == "deps" || n == "examples")
            && let Some(up) = dir.parent()
        {
            v.push(up.join("ruby").join("bin").join(RUBY_DLL));
        }
    }
    // Deliberately nothing else: no `C:\Ruby32` and friends (any local user can create those
    // folders, so a planted DLL there would run inside Esquisse), no PATH, no installed
    // SketchUp. Only the application's own folder or an explicit ESQ_RUBY_DLL are trusted.
    v
}

impl Ruby {
    /// Loads and initialises the interpreter. Must be called from the thread that will keep
    /// using it, with `stack_marker` pointing into a frame that outlives every later call.
    pub fn start(stack_marker: *mut c_void) -> Result<Ruby> {
        let dll = candidate_dlls().into_iter().find(|p| p.is_file()).context(
            "Ruby 3.2 introuvable : le dossier « ruby » livré à côté d'esquisse.exe manque (ou poser ESQ_RUBY_DLL)",
        )?;
        let bin = dll.parent().context("dll sans dossier")?.to_path_buf();
        // The dll's own dependencies (libgmp, libwinpthread) sit in bin/ruby_builtin_dlls.
        let builtin = bin.join("ruby_builtin_dlls");
        let old = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin, builtin.clone()];
        paths.extend(std::env::split_paths(&old));
        // SAFETY: done once, before the interpreter exists.
        unsafe { std::env::set_var("PATH", std::env::join_paths(paths)?) };
        add_dll_directory(&builtin);
        // GUI toolkits often restrict the default dll search (SetDefaultDllDirectories), which
        // hides the dll's own dependencies. 0x1500 = DEFAULT_DIRS | USER_DIRS | DLL_LOAD_DIR
        // looks next to the dll; 0x8 = ALTERED_SEARCH_PATH is the classic fallback.
        let mut last = None;
        let mut loaded = None;
        for flags in [0x1500u32, 0x8] {
            // SAFETY: loading a dll we located; its initialisers are Ruby's own.
            match unsafe { libloading::os::windows::Library::load_with_flags(&dll, flags) } {
                Ok(l) => {
                    loaded = Some(l);
                    break;
                }
                Err(e) => {
                    last = Some(format!(
                        "{e} (code {:?}, drapeaux {flags:#x})",
                        std::io::Error::last_os_error().raw_os_error()
                    ))
                }
            }
        }
        let Some(lib) = loaded else {
            bail!("chargement de {} : {}", dll.display(), last.unwrap_or_default());
        };
        let lib: Library = lib.into();
        // SAFETY: symbol names/signatures are those of Ruby 3.2's public C API.
        unsafe {
            macro_rules! sym {
                ($n:expr) => {
                    *lib.get($n).map_err(|e| anyhow::anyhow!("symbole manquant: {e}"))?
                };
            }
            let sysinit: unsafe extern "C" fn(*mut c_int, *mut *mut *mut c_char) = sym!(b"ruby_sysinit\0");
            let init_stack: unsafe extern "C" fn(*mut c_void) = sym!(b"ruby_init_stack\0");
            let init: unsafe extern "C" fn() = sym!(b"ruby_init\0");
            let options: unsafe extern "C" fn(c_int, *mut *mut c_char) -> *mut c_void = sym!(b"ruby_options\0");
            let mut argc: c_int = 3;
            let prog = CString::new("esquisse")?;
            let e_flag = CString::new("-e")?;
            let empty = CString::new("")?;
            let mut argv_v: [*mut c_char; 3] =
                [prog.as_ptr() as *mut c_char, e_flag.as_ptr() as *mut c_char, empty.as_ptr() as *mut c_char];
            let mut argv: *mut *mut c_char = argv_v.as_mut_ptr();
            sysinit(&mut argc, &mut argv);
            init_stack(stack_marker);
            init();
            // Finishes the boot (encodings, load path, rubygems) as `ruby -e ""` would, without
            // running anything: the returned node is dropped.
            options(3, argv_v.as_mut_ptr());
            let string_value_cstr: StrCstr = sym!(b"rb_string_value_cstr\0");
            let inspect: unsafe extern "C" fn(Value) -> Value = sym!(b"rb_inspect\0");
            let set_errinfo: unsafe extern "C" fn(Value) = sym!(b"rb_set_errinfo\0");
            let _ = FNS.set((string_value_cstr, inspect));
            let strings = Strings {
                protect: sym!(b"rb_protect\0"),
                is_kind_of: sym!(b"rb_obj_is_kind_of\0"),
                c_string: sym!(b"rb_cString\0"),
                set_errinfo,
            };
            Ok(Ruby {
                define_module: sym!(b"rb_define_module\0"),
                define_module_function: sym!(b"rb_define_module_function\0"),
                eval_string_protect: sym!(b"rb_eval_string_protect\0"),
                utf8_str_new: sym!(b"rb_utf8_str_new\0"),
                errinfo: sym!(b"rb_errinfo\0"),
                set_errinfo,
                strings,
                _lib: lib,
            })
        }
    }

    pub fn define_module(&self, name: &str) -> Value {
        let c = CString::new(name).unwrap();
        // SAFETY: valid C string, interpreter initialised.
        unsafe { (self.define_module)(c.as_ptr()) }
    }

    pub fn define_function(&self, module: Value, name: &str, f: Method2) {
        let c = CString::new(name).unwrap();
        // SAFETY: `f` follows the arity-2 calling convention declared by the last argument.
        unsafe { (self.define_module_function)(module, c.as_ptr(), f, 2) }
    }

    /// Evaluates source text and returns the resulting String value (the code must end with
    /// a String). On a Ruby exception the message is returned as the error.
    pub fn eval_string(&self, src: &str) -> Result<String> {
        let c = CString::new(src).context("NUL dans le code")?;
        let mut state: c_int = 0;
        // SAFETY: protected evaluation; errors come back through `state`.
        let v = unsafe { (self.eval_string_protect)(c.as_ptr(), &mut state) };
        if state != 0 {
            // SAFETY: we are on the interpreter thread; the exception object is live. Its
            // `inspect` runs protected (it may raise too).
            let text = unsafe {
                let e = (self.errinfo)();
                (self.set_errinfo)(QNIL);
                self.strings.protect(inspect_of, e).and_then(|s| self.strings.read(s))
            };
            bail!("exception Ruby: {}", text.unwrap_or_else(|| "(illisible)".into()));
        }
        Ok(self.read(v))
    }

    /// The text of a String result ("" for anything else, or a string with a NUL byte).
    pub fn read(&self, v: Value) -> String {
        self.strings.read(v).unwrap_or_default()
    }

    /// Function pointers the native callbacks need (they only receive raw Values).
    pub fn api(&self) -> Api {
        Api { strings: self.strings, utf8_str_new: self.utf8_str_new }
    }
}

#[derive(Clone, Copy)]
pub struct Api {
    strings: Strings,
    utf8_str_new: StrNew,
}

impl Api {
    /// The text of an argument of a native method: `None` when it is not a String, or holds
    /// a NUL byte (`Esq.__native(1, nil)` from an extension raised through Rust frames).
    pub fn read(&self, v: Value) -> Option<String> {
        self.strings.read(v)
    }

    pub fn make(&self, s: &str) -> Value {
        // SAFETY: pointer/length of a live &str.
        unsafe { (self.utf8_str_new)(s.as_ptr() as *const c_char, s.len() as c_long) }
    }
}

/// Lets `LOAD_LIBRARY_SEARCH_USER_DIRS` loads find dependencies in `dir`.
fn add_dll_directory(dir: &Path) {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: kernel32 is always loaded; AddDllDirectory takes a NUL-terminated wide path.
    unsafe {
        if let Ok(k) = Library::new("kernel32.dll")
            && let Ok(f) = k.get::<unsafe extern "system" fn(*const u16) -> *mut c_void>(b"AddDllDirectory\0")
        {
            f(wide.as_ptr());
        }
    }
}
