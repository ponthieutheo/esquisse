//! The open document: model, undo history, editing context and selection.

use std::path::PathBuf;

use esq_core::glam::DMat4;
use esq_core::model::{DefId, Entities, EntityRef, InstanceId, Model};
use esq_core::walk::{path_def, path_xform};

use crate::history::{self, Frozen};

const UNDO_DEPTH: usize = 100;

/// Fingerprint of each model as it was when the script running on it started
/// (`Model#modified?`), by the model's address.
static SCRIPT_BASE: std::sync::Mutex<Vec<(usize, u64)>> = std::sync::Mutex::new(Vec::new());

fn bases() -> std::sync::MutexGuard<'static, Vec<(usize, u64)>> {
    SCRIPT_BASE.lock().unwrap_or_else(|e| e.into_inner())
}

fn fingerprint(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// Whether the running script changed the model (None outside a script).
pub fn script_changed(model: &Model) -> Option<bool> {
    let key = model as *const Model as usize;
    let base = bases().iter().find(|(k, _)| *k == key)?.1;
    Some(serde_json::to_vec(model).map_or(true, |b| fingerprint(&b) != base))
}

struct Snapshot {
    label: String,
    /// The model as it was (`history.rs`: the collections that did not change are shared
    /// with the other snapshots).
    state: Frozen,
    ctx: Vec<InstanceId>,
}

pub struct Document {
    pub model: Model,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The state the history froze last: freezing the model again copies only what
    /// differs from it.
    frozen: Option<Frozen>,
    /// Instance path of the group/component being edited (empty = whole model).
    pub ctx: Vec<InstanceId>,
    /// Selection inside the active context.
    pub sel: Vec<EntityRef>,
    /// Geometry shown instead of the model while a tool is dragging.
    pub preview: Option<Model>,
    /// Bumped on any visible change.
    pub rev: u64,
    /// Bumped when the model itself changes.
    pub model_rev: u64,
    /// The bumps of `rev` made by `select` (the view then updates only what shows the
    /// selection: code that changes the model without an undo step bumps `rev` itself).
    pub sel_rev: u64,
    /// This document, among those of the session (the view's mesh is of one of them).
    pub id: u64,
    /// What happened since the app last looked (Ruby observers, see `ruby_tools.rs`).
    pub events: Vec<DocEvent>,
}

/// Documents made so far (`Document::id`).
static DOCUMENTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Something observers of the model want to hear about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocEvent {
    /// A change made through the interface (one undo step).
    Commit,
    Undo,
    Redo,
    /// A group/component opened for editing: the collection holding it, and the instance.
    Open(Option<DefId>, InstanceId),
    Close(Option<DefId>, InstanceId),
}

impl Document {
    pub fn new(model: Model) -> Self {
        Document {
            model,
            path: None,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            frozen: None,
            ctx: Vec::new(),
            sel: Vec::new(),
            preview: None,
            rev: 1,
            model_rev: 1,
            sel_rev: 0,
            id: DOCUMENTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1,
            events: Vec::new(),
        }
    }

    pub fn ctx_def(&self) -> Option<DefId> {
        path_def(&self.model, &self.ctx)
    }

    /// World transform of the active context.
    pub fn ctx_xform(&self) -> DMat4 {
        path_xform(&self.model, &self.ctx)
    }

    pub fn ents(&self) -> &Entities {
        self.model.entities(self.ctx_def())
    }

    pub fn display_model(&self) -> &Model {
        self.preview.as_ref().unwrap_or(&self.model)
    }

    pub fn set_preview(&mut self, m: Option<Model>) {
        self.preview = m;
        // The dimensions and texts attached to what a tool drags follow it while it drags.
        if let Some(p) = &mut self.preview {
            p.sync_annotations();
        }
        self.rev += 1;
    }

    /// The model's state for the history, sharing with the last one what did not change.
    fn freeze(&mut self) -> Frozen {
        let s = history::freeze(&mut self.model, self.frozen.as_ref());
        crate::perf::count(|c| c.undo_bytes += s.bytes_beyond(self.frozen.as_ref()));
        self.frozen = Some(s.clone());
        s
    }

    /// The model becomes state `s` again (`live`: its state as it is now).
    fn restore(&mut self, s: &Frozen, live: &Frozen) {
        let old = std::mem::take(&mut self.model);
        self.model = history::thaw(s, live, old);
        self.frozen = Some(s.clone());
    }

    fn push_undo(&mut self, label: &str, state: Frozen) {
        self.undo.push(Snapshot { label: label.into(), state, ctx: self.ctx.clone() });
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
    }

    /// Applies a change as one undoable step.
    pub fn commit(&mut self, label: &str, f: impl FnOnce(&mut Model, Option<DefId>)) {
        let before = self.freeze();
        self.push_undo(label, before);
        self.redo.clear();
        let def = self.ctx_def();
        f(&mut self.model, def);
        // What the change created receives the active tag (Window › Tags).
        if let Some(before) = self.undo.last() {
            crate::dialogs::tag_dialog::tag_new(before.state.keys(def), &mut self.model, def);
        }
        self.after_change();
        self.events.push(DocEvent::Commit);
    }

    /// `run_script_steps` for a script that commits no operation.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn run_script<R>(&mut self, label: &str, f: impl FnOnce(&mut Model, &mut Vec<EntityRef>) -> R) -> R {
        self.run_script_steps(label, |m, s| (f(m, s), Vec::new()))
    }

    /// Runs a script that edits the model and the selection (the Ruby console and plugins).
    /// The script reports the operations it committed (`Model#start_operation` …
    /// `commit_operation`), each with the model as it was before it: every operation becomes
    /// its own undo step, named after it. Without operations, the whole run is one undo step,
    /// recorded only if the model actually changed.
    pub fn run_script_steps<R>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Model, &mut Vec<EntityRef>) -> (R, Vec<(String, Model)>),
    ) -> R {
        // Scenes, tags and the model get their identity (for `Sketchup::Page`, `Layer`,
        // `Model#guid`) before the "before" snapshot, so that merely reading them is not a change.
        self.model.ensure_scene_uids();
        self.model.ensure_ruby_ids();
        let before = self.model.clone();
        let before_bytes = serde_json::to_vec(&before).ok();
        let key = &self.model as *const Model as usize;
        if let Some(b) = before_bytes.as_deref() {
            bases().push((key, fingerprint(b)));
        }
        let mut sel = self.sel.clone();
        let (r, steps) = f(&mut self.model, &mut sel);
        bases().retain(|(k, _)| *k != key);
        let same = |a: &Model, b: &Model| serde_json::to_vec(a).ok() == serde_json::to_vec(b).ok();
        let changed = before_bytes != serde_json::to_vec(&self.model).ok();
        if changed || !steps.is_empty() {
            let first_differs = steps.first().is_none_or(|(_, m)| !same(&before, m));
            let mut models = Vec::new();
            if first_differs {
                models.push((label.to_string(), before));
            }
            models.extend(steps);
            // Each state shares with the one before it what the operation left alone.
            for (name, mut m) in models {
                let s = history::freeze(&mut m, self.frozen.as_ref());
                crate::perf::count(|c| c.undo_bytes += s.bytes_beyond(self.frozen.as_ref()));
                self.frozen = Some(s.clone());
                self.undo.push(Snapshot { label: name, state: s, ctx: self.ctx.clone() });
            }
            while self.undo.len() > UNDO_DEPTH {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.after_change();
        }
        sel.sort();
        sel.dedup();
        if sel != self.sel {
            self.select_after_script(sel);
            self.prune_selection();
        }
        r
    }

    /// Like `commit`, but leaves the model untouched (and the history unchanged) when the
    /// change fails.
    pub fn try_commit<E>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Model, Option<DefId>) -> Result<(), E>,
    ) -> Result<(), E> {
        let before = self.freeze();
        let def = self.ctx_def();
        if let Err(e) = f(&mut self.model, def) {
            let live = history::freeze(&mut self.model, Some(&before));
            self.restore(&before, &live);
            return Err(e);
        }
        self.push_undo(label, before);
        self.redo.clear();
        self.after_change();
        self.events.push(DocEvent::Commit);
        Ok(())
    }

    /// Like `run_script` for scripts that seldom change the model (a Ruby tool's mouse moves
    /// and drawing, every frame): the change test only compares element counts, so it costs
    /// a copy of the model but no serialization.
    pub fn run_script_light<R>(&mut self, label: &str, f: impl FnOnce(&mut Model, &mut Vec<EntityRef>) -> R) -> R {
        fn counts(m: &Model) -> Vec<usize> {
            use slotmap::Key;
            let mut v = vec![m.defs.len(), m.materials.len(), m.layers.len(), m.scenes.len()];
            for e in std::iter::once(&m.root).chain(m.defs.values().map(|d| &d.entities)) {
                v.extend([
                    e.verts.len(),
                    e.edges.len(),
                    e.faces.len(),
                    e.instances.len(),
                    e.guides.len(),
                    e.annots.len(),
                    // A section plane added and activated by a Ruby tool cuts the view at once.
                    e.sections.len(),
                    e.active_section.map_or(0, |s| s.data().as_ffi() as usize),
                    e.snaps.len(),
                    e.spaces.len(),
                    e.lights.len(),
                ]);
            }
            v
        }
        self.model.ensure_scene_uids();
        self.model.ensure_ruby_ids();
        let before_counts = counts(&self.model);
        let before = self.freeze();
        let mut sel = self.sel.clone();
        let r = f(&mut self.model, &mut sel);
        if before_counts != counts(&self.model) {
            self.push_undo(label, before);
            self.redo.clear();
            self.after_change();
        }
        sel.sort();
        sel.dedup();
        if sel != self.sel {
            self.select_after_script(sel);
            self.prune_selection();
        }
        r
    }

    fn after_change(&mut self) {
        // The dimensions and texts attached to what changed follow it (`esq_core::attach`).
        let s = self.model.sync_annotations();
        crate::perf::count(|c| {
            c.attach_annots += s.annotations as u64;
            c.attach_lookups += s.lookups as u64;
        });
        self.preview = None;
        self.dirty = true;
        self.rev += 1;
        self.model_rev += 1;
        self.validate_ctx();
        // What the step replaced is followed by now (the notes of the open context, where the
        // tools work, go; those of other collections are bounded, `attach::Replaced`).
        self.model.forget_replaced(self.ctx_def());
        self.prune_selection();
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    /// Steps that can be undone and redone (each keeps the model as it was: what it shares
    /// with the other steps once, `history.rs`).
    pub fn history_len(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }

    /// Optimize Memory: keeps only the `keep` most recent undo steps and forgets the redo
    /// ones. Returns how many copies of the model were freed.
    pub fn trim_history(&mut self, keep: usize) -> usize {
        let drop = self.undo.len().saturating_sub(keep);
        self.undo.drain(..drop);
        let freed = drop + self.redo.len();
        self.redo.clear();
        freed
    }

    /// The model becomes snapshot `s`; returns the snapshot of the model as it was.
    fn swap_in(&mut self, s: Snapshot) -> Snapshot {
        let live = self.freeze();
        self.restore(&s.state, &live);
        let cur = Snapshot { label: s.label, state: live, ctx: std::mem::replace(&mut self.ctx, s.ctx) };
        self.sel.clear();
        self.after_change();
        cur
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            let cur = self.swap_in(s);
            self.redo.push(cur);
            self.events.push(DocEvent::Undo);
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            let cur = self.swap_in(s);
            self.undo.push(cur);
            self.events.push(DocEvent::Redo);
        }
    }

    pub fn select(&mut self, sel: Vec<EntityRef>) {
        self.sel = sel;
        self.sel.sort();
        self.sel.dedup();
        self.rev += 1;
        self.sel_rev += 1;
    }

    /// `select` after a script, which may have changed the model without an undo step (a
    /// Ruby tool's light run): not a change of the selection alone for the view.
    fn select_after_script(&mut self, sel: Vec<EntityRef>) {
        self.select(sel);
        self.rev += 1;
    }

    pub fn is_selected(&self, e: EntityRef) -> bool {
        self.sel.binary_search(&e).is_ok()
    }

    fn validate_ctx(&mut self) {
        let mut ents = &self.model.root;
        for (i, &inst) in self.ctx.iter().enumerate() {
            match ents.instances.get(inst) {
                Some(x) => ents = &self.model.defs[x.def].entities,
                None => {
                    self.ctx.truncate(i);
                    break;
                }
            }
        }
    }

    fn prune_selection(&mut self) {
        let ents = self.model.entities(self.ctx_def());
        self.sel.retain(|e| ents.contains(*e));
    }

    /// Opens a group/component for editing. Groups get their own definition first, as in
    /// SketchUp (editing one copy of a group never changes the others).
    pub fn enter(&mut self, inst: InstanceId) {
        let def = self.ctx_def();
        let Some(i) = self.model.entities(def).instances.get(inst) else { return };
        if self.model.defs[i.def].is_group && self.model.instance_count(i.def) > 1 {
            self.commit("Rendre unique", |m, d| {
                m.make_unique(d, inst);
            });
        }
        self.ctx.push(inst);
        self.sel.clear();
        self.rev += 1;
        self.model_rev += 1;
        self.events.push(DocEvent::Open(def, inst));
    }

    pub fn exit(&mut self) {
        if let Some(inst) = self.ctx.pop() {
            self.sel = vec![EntityRef::Instance(inst)];
            self.rev += 1;
            self.model_rev += 1;
            let def = self.ctx_def();
            self.events.push(DocEvent::Close(def, inst));
        }
    }

    pub fn exit_all(&mut self) {
        while !self.ctx.is_empty() {
            self.exit();
        }
    }

    pub fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Sans titre".into());
        format!("{}{} — Esquisse", if self.dirty { "• " } else { "" }, name)
    }
}
