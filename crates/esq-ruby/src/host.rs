//! Access from Ruby to the application itself (camera, dialogs, tools…), which the model
//! alone does not hold. The app wraps each script run in `with_host`; Ruby reaches it through
//! `Esq.call('host.<op>', args)`.

use std::sync::Mutex;

use esq_core::model::{EntityRef, Model};
use serde_json::Value as J;

pub trait Host {
    /// Handles `host.<op>`. `model` and `sel` are the document the script is editing.
    fn call(&mut self, model: &mut Model, sel: &mut Vec<EntityRef>, op: &str, args: &J) -> Result<J, String>;
}

struct HostPtr(*mut dyn Host);
// SAFETY: the pointer is only dereferenced by the Ruby thread while the thread that set it
// is blocked inside `Interpreter::run` (see `with_host`).
unsafe impl Send for HostPtr {}

static HOST: Mutex<Option<HostPtr>> = Mutex::new(None);

/// Makes `host` reachable from scripts run (through an `Interpreter`) inside `f`.
pub fn with_host<R>(host: &mut dyn Host, f: impl FnOnce() -> R) -> R {
    // SAFETY: lifetime erased; the pointer is cleared before this function returns.
    let p: *mut (dyn Host + 'static) = unsafe { std::mem::transmute::<&mut dyn Host, &mut (dyn Host + 'static)>(host) };
    *HOST.lock().unwrap() = Some(HostPtr(p));
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            *HOST.lock().unwrap() = None;
        }
    }
    let _clear = Clear;
    f()
}

pub(crate) fn call(model: &mut Model, sel: &mut Vec<EntityRef>, op: &str, args: &J) -> Result<J, String> {
    let ptr = HOST.lock().unwrap().as_ref().map(|h| h.0);
    match ptr {
        // SAFETY: see `HostPtr`.
        Some(p) => unsafe { (*p).call(model, sel, op, args) },
        None => Err("l'application n'est pas disponible dans ce contexte".into()),
    }
}
