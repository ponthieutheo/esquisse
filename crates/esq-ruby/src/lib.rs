//! Ruby scripting for Esquisse: an embedded MRI interpreter (Ruby 3.2, loaded from the
//! `x64-ucrt-ruby320.dll` SketchUp itself uses) with a SketchUp-compatible API on top.
//!
//! The interpreter lives on its own thread (Ruby wants one stack for its garbage collector).
//! `Interpreter::run` hands it the model and blocks until the script is done, so scripts see
//! the document exactly as the UI left it and the UI sees what the script did.

mod bridge;
mod host;
pub use host::{Host, with_host};
mod ffi;

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{Sender, channel};

use anyhow::{Context, Result, anyhow};
use esq_core::model::{EntityRef, Model};
use serde_json::{Value as J, json};

use bridge::Ctx;
use ffi::{Api, Ruby, Value};

pub use bridge::{shadow_restore, shadow_snapshot};
pub use ffi::candidate_dlls;

mod prelude;

/// Result of one script run.
#[derive(Clone, Debug, Default)]
pub struct RunOutput {
    pub ok: bool,
    /// `inspect` of the last expression.
    pub result: String,
    /// Everything the script printed.
    pub out: String,
    /// Exception message and backtrace when `ok` is false.
    pub error: String,
}

/// A menu entry registered by a script (`UI.menu("Plugins").add_item(...)`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuCommand {
    pub id: i64,
    /// Menu path: `Tools`, `Extensions/My plugin`… (first segment: File, Edit, View, Camera, Draw,
    /// Tools, Window, Extensions, Help).
    pub menu: String,
    pub title: String,
    pub separator: bool,
    pub tooltip: String,
    pub status: String,
    pub small_icon: String,
    pub large_icon: String,
    /// `MF_*` flags from the validation proc (0 = enabled, 1/2 = greyed, 8 = checked).
    pub flags: i64,
}

impl MenuCommand {
    pub fn enabled(&self) -> bool {
        self.flags & 3 == 0
    }
    pub fn checked(&self) -> bool {
        self.flags & 8 != 0
    }
    fn from_json(c: &J) -> MenuCommand {
        let s = |k: &str| c[k].as_str().unwrap_or_default().to_string();
        MenuCommand {
            id: c["id"].as_i64().unwrap_or(0),
            menu: s("menu"),
            title: s("title"),
            separator: c["sep"].as_bool().unwrap_or(false),
            tooltip: s("tooltip"),
            status: s("status"),
            small_icon: s("small_icon"),
            large_icon: s("large_icon"),
            flags: c["flags"].as_i64().unwrap_or(0),
        }
    }
}

/// A toolbar built by a script (`UI::Toolbar`); `None` items are separators.
#[derive(Clone, Debug, Default)]
pub struct ToolbarSpec {
    pub name: String,
    pub items: Vec<Option<MenuCommand>>,
}

struct Job {
    code: String,
    file: String,
    model: usize,
    sel: usize,
    kind: JobKind,
    reply: Sender<Result<RunOutput, String>>,
}

enum JobKind {
    Code,
    Pump,
    Invoke(i64),
    /// Evaluates a Ruby expression that answers a JSON string (menu entries, toolbars…).
    Json(&'static str),
}

pub struct Interpreter {
    tx: Sender<Job>,
}

thread_local! {
    static PENDING: RefCell<(String, String)> = const { RefCell::new((String::new(), String::new())) };
    static CURRENT: Cell<*mut Ctx<'static>> = const { Cell::new(std::ptr::null_mut()) };
}

static API: OnceLock<Api> = OnceLock::new();

/// `Esq.__native(op, json_args)`: runs one bridge operation, answers `{"ok":…}` or `{"err":…}`.
unsafe extern "C" fn native(_self: Value, op: Value, arg: Value) -> Value {
    let Some(&api) = API.get() else { return ffi::QNIL };
    let (Some(op), Some(arg)) = (api.read(op), api.read(arg)) else {
        return api.make(r#"{"err":"ArgumentError: Esq.__native attend deux chaînes sans octet nul"}"#);
    };
    let reply = catch_unwind(AssertUnwindSafe(|| -> J {
        if op == "take" {
            let (code, file) = PENDING.with(|p| p.borrow().clone());
            return json!({"ok": {"code": code, "file": file}});
        }
        let ptr = CURRENT.with(Cell::get);
        if ptr.is_null() {
            return json!({"err": "aucun document actif"});
        }
        let args: J = serde_json::from_str(&arg).unwrap_or(J::Null);
        // SAFETY: CURRENT is set only while `run` holds the UI thread blocked on this job.
        let ctx = unsafe { &mut *ptr };
        match bridge::dispatch(ctx, &op, &args) {
            Ok(v) => json!({"ok": v}),
            Err(e) => json!({"err": e}),
        }
    }))
    .unwrap_or_else(|_| json!({"err": "panique dans le pont natif"}));
    api.make(&reply.to_string())
}

fn worker(rx: std::sync::mpsc::Receiver<Job>, ready: Sender<Result<(), String>>) {
    let marker = 0u8; // lives for the whole thread: Ruby's stack base
    let ruby = match Ruby::start(&marker as *const u8 as *mut _) {
        Ok(r) => r,
        Err(e) => {
            let _ = ready.send(Err(format!("{e:#}")));
            return;
        }
    };
    let _ = API.set(ruby.api());
    let esq = ruby.define_module("Esq");
    ruby.define_function(esq, "__native", native);
    // The prelude is evaluated with a document-less context; it only defines classes.
    let loaded = prelude::load(&mut |name, src| {
        // Not user input: `src` is one of our own prelude files embedded at compile time; the
        // eval only attaches the file name to Ruby backtraces.
        let code = format!("eval(<<'__ESQ__', TOPLEVEL_BINDING, {name:?}, 1)\n{src}\n__ESQ__\n''");
        ruby.eval_string(&code).map(|_| ()).map_err(|e| format!("{e:#}"))
    });
    if let Err(e) = loaded {
        let _ = ready.send(Err(format!("prélude Ruby: {e}")));
        return;
    }
    let _ = ready.send(Ok(()));
    while let Ok(job) = rx.recv() {
        // SAFETY: the sender blocks on `reply` while we use these pointers.
        let model = unsafe { &mut *(job.model as *mut Model) };
        let sel = unsafe { &mut *(job.sel as *mut Vec<EntityRef>) };
        let mut ctx = Ctx { model, sel };
        // SAFETY: lifetime erased for the thread-local; reset before returning.
        CURRENT.with(|c| c.set(&mut ctx as *mut Ctx<'_> as usize as *mut Ctx<'static>));
        if matches!(job.kind, JobKind::Code | JobKind::Invoke(_)) {
            bridge::begin_run();
        }
        let res = match job.kind {
            JobKind::Code => {
                PENDING.with(|p| *p.borrow_mut() = (job.code.clone(), job.file.clone()));
                ruby.eval_string("Esq.__run").map_err(|e| format!("{e:#}")).and_then(parse_run)
            }
            JobKind::Pump => {
                PENDING.with(|p| *p.borrow_mut() = ("Esq.pump".into(), "pump".into()));
                ruby.eval_string("Esq.__run").map_err(|e| format!("{e:#}")).and_then(parse_run)
            }
            JobKind::Invoke(id) => {
                PENDING.with(|p| *p.borrow_mut() = (format!("Esq.invoke_command({id})"), "commande".into()));
                ruby.eval_string("Esq.__run").map_err(|e| format!("{e:#}")).and_then(parse_run)
            }
            JobKind::Json(expr) => ruby
                .eval_string(expr)
                .map(|s| RunOutput { ok: true, result: s, ..Default::default() })
                .map_err(|e| format!("{e:#}")),
        };
        bridge::finish_run();
        CURRENT.with(|c| c.set(std::ptr::null_mut()));
        let _ = job.reply.send(res);
    }
}

fn parse_run(s: String) -> Result<RunOutput, String> {
    let j: J = serde_json::from_str(&s).map_err(|e| format!("réponse illisible: {e}"))?;
    Ok(RunOutput {
        ok: j["ok"].as_bool().unwrap_or(false),
        result: j["result"].as_str().unwrap_or_default().to_string(),
        out: j["out"].as_str().unwrap_or_default().to_string(),
        error: j["err"].as_str().unwrap_or_default().to_string(),
    })
}

impl Interpreter {
    /// Starts the interpreter thread. Fails when no Ruby 3.2 runtime can be found.
    pub fn start() -> Result<Interpreter> {
        let (tx, rx) = channel::<Job>();
        let (rtx, rrx) = channel();
        std::thread::Builder::new()
            .name("ruby".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || worker(rx, rtx))
            .context("thread Ruby")?;
        rrx.recv().context("thread Ruby arrêté")?.map_err(|e| anyhow!(e))?;
        Ok(Interpreter { tx })
    }

    fn submit(
        &self,
        code: &str,
        file: &str,
        kind: JobKind,
        model: &mut Model,
        sel: &mut Vec<EntityRef>,
    ) -> Result<RunOutput> {
        let (rtx, rrx) = channel();
        self.tx
            .send(Job {
                code: code.into(),
                file: file.into(),
                model: model as *mut Model as usize,
                sel: sel as *mut Vec<EntityRef> as usize,
                kind,
                reply: rtx,
            })
            .map_err(|_| anyhow!("l'interpréteur Ruby s'est arrêté"))?;
        rrx.recv().map_err(|_| anyhow!("l'interpréteur Ruby s'est arrêté"))?.map_err(|e| anyhow!(e))
    }

    /// Runs Ruby source. `file` names it in backtraces.
    pub fn run(&self, code: &str, file: &str, model: &mut Model, sel: &mut Vec<EntityRef>) -> Result<RunOutput> {
        self.submit(code, file, JobKind::Code, model, sel)
    }

    /// Loads every `*.rb` of a plugins folder (SketchUp's `Plugins` directory).
    pub fn load_plugins(&self, dir: &Path, model: &mut Model, sel: &mut Vec<EntityRef>) -> Result<RunOutput> {
        let d = dir.to_string_lossy().replace('\\', "/");
        self.run(&format!("Esq.load_plugins({d:?})"), "plugins", model, sel)
    }

    /// Menu entries registered by the scripts so far.
    pub fn commands(&self, model: &mut Model, sel: &mut Vec<EntityRef>) -> Result<Vec<MenuCommand>> {
        self.json_list("Esq.commands_json", model, sel)
    }

    /// Entries added by the scripts' context-menu handlers (`UI.add_context_menu_handler`),
    /// collected now so the handlers see the current selection.
    pub fn context_menu(&self, model: &mut Model, sel: &mut Vec<EntityRef>) -> Result<Vec<MenuCommand>> {
        self.json_list("Esq.context_menu_json", model, sel)
    }

    fn json_list(&self, expr: &'static str, model: &mut Model, sel: &mut Vec<EntityRef>) -> Result<Vec<MenuCommand>> {
        let out = self.submit("", "", JobKind::Json(expr), model, sel)?;
        let j: J = serde_json::from_str(&out.result)?;
        Ok(j.as_array().map(|a| a.iter().map(MenuCommand::from_json).collect()).unwrap_or_default())
    }

    /// Toolbars built by the scripts (`UI::Toolbar`).
    pub fn toolbars(&self, model: &mut Model, sel: &mut Vec<EntityRef>) -> Result<Vec<ToolbarSpec>> {
        let out = self.submit("", "", JobKind::Json("Esq.toolbars_json"), model, sel)?;
        let j: J = serde_json::from_str(&out.result)?;
        Ok(j.as_array()
            .map(|a| {
                a.iter()
                    .map(|t| ToolbarSpec {
                        name: t["name"].as_str().unwrap_or_default().into(),
                        items: t["items"]
                            .as_array()
                            .map(|i| {
                                i.iter()
                                    .map(|c| if c.is_null() { None } else { Some(MenuCommand::from_json(c)) })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Hands to Ruby what the network threads (`Sketchup::Http`, `SKSocket`) have received: runs
    /// the callbacks. Call it while `pending_events()` is true.
    pub fn pump(&self, model: &mut Model, sel: &mut Vec<EntityRef>) -> Result<RunOutput> {
        self.submit("", "", JobKind::Pump, model, sel)
    }

    /// Runs the block of a registered menu entry.
    pub fn invoke(&self, id: i64, model: &mut Model, sel: &mut Vec<EntityRef>) -> Result<RunOutput> {
        self.submit("", "", JobKind::Invoke(id), model, sel)
    }
}

/// The operations (`Model#start_operation` … `commit_operation`) the last script committed,
/// oldest first, each with the model as it was before the operation: one undo step each.
pub fn take_ops() -> Vec<(String, esq_core::model::Model)> {
    bridge::take_ops()
}

/// True while an HTTP request is in flight or a socket is open.
pub fn network_active() -> bool {
    bridge::network_active()
}

/// True when a network result waits for `Interpreter::pump`.
pub fn pending_events() -> bool {
    bridge::pending_events()
}

static SUPPORT_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Sets the folder that holds `Plugins` and the saved defaults (the host's user folder, a
/// temporary one in its tests). Only the first call counts.
pub fn set_support_dir(dir: PathBuf) {
    let _ = SUPPORT_DIR.set(dir);
}

/// Folder SketchUp-style plugins are loaded from: `%APPDATA%\Esquisse\Plugins` (see
/// [`set_support_dir`]; a folder of this process in this crate's tests).
pub fn plugins_dir() -> PathBuf {
    let base = SUPPORT_DIR.get().cloned().unwrap_or_else(|| {
        let root = if cfg!(test) {
            std::env::temp_dir().join(format!("esq-tests-{}", std::process::id()))
        } else {
            std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
        };
        root.join("Esquisse")
    });
    base.join("Plugins")
}

#[cfg(test)]
mod tests;
