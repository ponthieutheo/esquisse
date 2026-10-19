//! `Model#start_operation` / `commit_operation` / `abort_operation`.
//!
//! An operation remembers the model as it was when it started. Aborting puts that state
//! back; committing keeps it as one undo step. The steps of a script are handed to the
//! application when the script ends (`take_ops`), so that each committed operation is one
//! entry of the undo history, as in SketchUp.

use std::sync::Mutex;

use esq_core::model::{EntityRef, Model};
use serde_json::{Value as J, json};

use super::raise;
use crate::bridge::{Ctx, R};

struct Open {
    name: String,
    before: Model,
    sel: Vec<EntityRef>,
    transparent: bool,
}

#[derive(Default)]
struct State {
    open: Option<Open>,
    log: Vec<(String, Model)>,
}

static STATE: Mutex<State> = Mutex::new(State { open: None, log: Vec::new() });

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Called before each script: forgets what the previous one left.
pub fn begin_run() {
    let mut s = state();
    s.open = None;
    s.log.clear();
}

/// Called after each script: an operation the script left open is committed.
pub fn finish_run() {
    let mut s = state();
    if let Some(o) = s.open.take() {
        push(&mut s, o);
    }
}

/// The operations committed by the last script, oldest first, each with the model as it was
/// before the operation.
pub fn take_ops() -> Vec<(String, Model)> {
    std::mem::take(&mut state().log)
}

fn push(s: &mut State, o: Open) {
    // A transparent operation joins the one before it.
    if !(o.transparent && !s.log.is_empty()) {
        s.log.push((o.name, o.before));
    }
}

pub fn run(c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    Some(match op {
        "op.start" => Ok({
            let mut s = state();
            let name = a["name"].as_str().unwrap_or("").to_string();
            let mut warn = J::Null;
            if let Some(prev) = s.open.take() {
                warn = json!(format!(
                    "New operation (\"{}\") started while an existing operation (\"{}\") was still open",
                    name, prev.name
                ));
                push(&mut s, prev);
            }
            s.open = Some(Open {
                name,
                before: c.model.clone(),
                sel: c.sel.clone(),
                transparent: a.get("transparent").and_then(J::as_bool).unwrap_or(false),
            });
            json!({"warn": warn})
        }),
        "op.commit" => Ok({
            let mut s = state();
            match s.open.take() {
                Some(o) => {
                    push(&mut s, o);
                    json!(true)
                }
                None => json!(false),
            }
        }),
        "op.abort" => {
            let mut s = state();
            match s.open.take() {
                Some(o) => {
                    *c.model = o.before;
                    *c.sel = o.sel;
                    Ok(json!(true))
                }
                None => Err(raise("RuntimeError", "no operation to abort")),
            }
        }
        "op.is_open" => Ok(json!(state().open.is_some())),
        _ => return None,
    })
}
