//! What Ruby scripts can reach beyond the model: camera, views, dialogs, tools, timers…
//! Ruby calls `Esq.call('host.view.zoom_extents', …)`; the op lands in the module named by
//! its first segment (`view`, `ui`, `tools`).
//!
//! While a script runs, the document is moved out of the app (`app.doc` is an empty
//! placeholder): use the `model` and `sel` passed to `call` instead.

use esq_core::model::{EntityRef, Model};
use esq_ruby::Host;
use serde_json::Value as J;

use crate::app::EsquisseApp;
use crate::doc::Document;

mod entities;
pub mod formats;
pub mod model;
pub mod tools;
mod ui;
pub mod view;

pub use ui::{
    draw_toolbars, frame as ui_frame, has_events as ui_has_events, plugin_toolbar_shown, set_plugin_toolbar_shown,
};

pub struct AppHost<'a> {
    pub app: &'a mut EsquisseApp,
    /// File, edit path… of the document the script works on (see `model.rs`).
    pub st: &'a mut model::ScriptState,
}

impl Host for AppHost<'_> {
    fn call(&mut self, model: &mut Model, sel: &mut Vec<EntityRef>, op: &str, args: &J) -> Result<J, String> {
        match op.split('.').next().unwrap_or("") {
            "model" => model::call(self.app, self.st, model, sel, op, args),
            "entities" => entities::call(self.app, model, sel, op, args),
            "view" => view::call(self.app, model, sel, op, args),
            "ui" => ui::call(self.app, model, sel, op, args),
            // Needs the script state: the tool is set once the script is over.
            "tools" if op == "tools.place_component" => model::place_component(self.st, model, args),
            "tools" => tools::call(self.app, model, sel, op, args),
            _ => Err(format!("opération hôte inconnue: {op}")),
        }
    }
}

/// Runs a script on the document as one undo step, with the app reachable from Ruby.
pub fn run_script<R>(app: &mut EsquisseApp, label: &str, f: impl FnOnce(&mut Model, &mut Vec<EntityRef>) -> R) -> R {
    let mut doc = std::mem::replace(&mut app.doc, Document::new(Model::new()));
    let mut st =
        model::ScriptState { path: doc.path.clone(), dirty: doc.dirty, ctx: doc.ctx.clone(), ..Default::default() };
    let r = {
        let mut host = AppHost { app: &mut *app, st: &mut st };
        // Each operation the script committed is one undo step.
        esq_ruby::with_host(&mut host, || doc.run_script_steps(label, |m, s| (f(m, s), esq_ruby::take_ops())))
    };
    match st.replace.take() {
        Some(rep) => {
            match rep {
                model::Replace::New => app.new_document(),
                model::Replace::Open(p) => app.open_path(&p),
            }
            let mut fresh = std::mem::replace(&mut app.doc, Document::new(Model::new()));
            // The script may have gone on editing the new document.
            fresh.model = doc.model;
            fresh.path = st.path.clone();
            fresh.dirty = st.dirty;
            fresh.rev += 1;
            fresh.model_rev += 1;
            app.doc = fresh;
        }
        None => {
            doc.path = st.path.clone();
            if st.saved {
                doc.dirty = false;
            }
            if st.ctx_changed {
                doc.ctx = st.ctx.clone();
                doc.rev += 1;
                doc.model_rev += 1;
            }
            app.doc = doc;
            for _ in 0..st.undo {
                app.doc.undo();
            }
            for _ in 0..st.redo {
                app.doc.redo();
            }
            // `Model#import` of a file placed with the cursor, as File › Import does, and
            // `Model#place_component`.
            if let Some(d) = st.place.filter(|d| app.doc.model.defs.contains_key(*d)) {
                app.place_component(d);
                if st.place_repeat {
                    app.tool = Box::new(crate::tools::PlaceTool::repeating(d));
                }
            }
        }
    }
    if st.quit {
        app.request_quit();
    }
    app.pending_cmds.append(&mut st.actions);
    r
}

/// Like [`run_script`] for what runs every frame (a Ruby tool's mouse moves and drawing):
/// see `Document::run_script_light`.
pub fn run_light<R>(app: &mut EsquisseApp, label: &str, f: impl FnOnce(&mut Model, &mut Vec<EntityRef>) -> R) -> R {
    let mut doc = std::mem::replace(&mut app.doc, Document::new(Model::new()));
    let mut st =
        model::ScriptState { path: doc.path.clone(), dirty: doc.dirty, ctx: doc.ctx.clone(), ..Default::default() };
    let r = {
        let mut host = AppHost { app: &mut *app, st: &mut st };
        esq_ruby::with_host(&mut host, || doc.run_script_light(label, f))
    };
    // What a tool does in a mouse move that needs the full treatment (a file opened, the
    // edit path changed…) is rare: it is applied as by `run_script`.
    doc.path = st.path.clone();
    if st.ctx_changed {
        doc.ctx = st.ctx.clone();
        doc.rev += 1;
        doc.model_rev += 1;
    }
    app.doc = doc;
    for _ in 0..st.undo {
        app.doc.undo();
    }
    for _ in 0..st.redo {
        app.doc.redo();
    }
    app.pending_cmds.append(&mut st.actions);
    // Operations committed during a light run: they are not separate undo steps here.
    let _ = esq_ruby::take_ops();
    r
}
