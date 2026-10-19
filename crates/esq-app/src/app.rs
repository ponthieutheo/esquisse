//! Application state, viewport input handling and commands.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Key, Modifiers, PointerButton, Sense};
use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::model::{DefId, EntityRef, MaterialId, Model};

use crate::camera::Camera;
use crate::doc::Document;
use crate::pick::{InferCx, Lock, PickCache, Recent, SnapKind};
use crate::render::Viewport3d;
use crate::scene::{self, SceneOptions};
use crate::style::ViewStyle;
use crate::tools::{Overlay, Tool, ToolCx, ToolId};

/// What the view's mesh was built for, besides the selection and the camera's eye: when
/// only those changed, `prepare_scene` updates the parts that show them.
#[derive(PartialEq)]
struct ViewKey {
    doc: u64,
    /// `doc.rev` less the bumps of the selection and of the eye: any other change.
    rest: u64,
    model_rev: u64,
    style: ViewStyle,
    ctx: Vec<esq_core::model::InstanceId>,
    ctx_def: Option<DefId>,
    axes: [u64; 16],
    /// The view's uploads: something else uploaded a mesh meanwhile.
    upload: u64,
}

/// Most recent files kept in File › (recent files).
const RECENT_MAX: usize = 8;
const CAMERA_HISTORY: usize = 50;

pub struct EsquisseApp {
    pub doc: Document,
    pub cam: Camera,
    pub tool: Box<dyn Tool>,
    pub(crate) view: Option<Viewport3d>,
    built_rev: u64,
    /// What is under the cursor. Read it after `ensure_pick`: when the camera turns
    /// components that face it, it is rebuilt only once something asks.
    pub pick: PickCache,
    pick_rev: u64,
    pick_eye_stale: bool,
    /// The view's mesh: what it was built for and its parts (`prepare_scene`).
    scene_parts: Option<(ViewKey, scene::Parts)>,
    /// The bumps of `doc.rev` made by `follow_eye` (the eye moved).
    eye_rev: u64,
    /// What turns with the eye, placed for the eye of `eye_rev` (`ensure_overlay_pick`).
    turning: Option<(u64, crate::pick::Turning)>,
    pub vcb: String,
    pub message: Option<(String, f64)>,
    arrow_lock: Option<Lock>,
    shift_lock: Option<Lock>,
    pub style: ViewStyle,
    pick_style: ViewStyle,
    pub material: Option<MaterialId>,
    /// Current tag, used by the Tag tool.
    pub tag: usize,
    /// What Edit › Hide hid last (Edit › Reveal › Last).
    pub last_hidden: Vec<EntityRef>,
    /// Camera › Previous / Next.
    pub cam_back: Vec<Camera>,
    pub cam_fwd: Vec<Camera>,
    pub recent_files: Vec<PathBuf>,
    /// The window's GPU, for what renders outside the frame (`Model#export` of an image).
    pub(crate) gpu: Option<eframe::egui_wgpu::RenderState>,
    /// Pixels per point of the 3D view (the display's scale).
    pub(crate) ppp: f32,
    /// `Sketchup.quit` was called by a script.
    quit_requested: bool,
    /// Commands sent by scripts (`Sketchup.send_action`), run at the next frame.
    pub pending_cmds: Vec<crate::commands::Cmd>,
    pub active_scene: Option<usize>,
    /// Match Photo: the photo painted behind the view this frame, and where (points).
    pub photo_shown: Option<(esq_core::photo::Blob, [f32; 4])>,
    pub transition: Option<crate::scenes::Transition>,
    pub playing: Option<crate::scenes::Playing>,
    pub anim: crate::scenes::AnimSettings,
    /// Shadow settings before the slider being dragged in Window › Shadows.
    pub shadow_edit: Option<esq_core::sun::ShadowInfo>,
    recent: Option<Recent>,
    /// Guide points of the displayed model, drawn as crosses over the 3D view.
    guide_pts: Vec<(DVec3, bool)>,
    /// Dimensions and texts, drawn over the 3D view.
    annots: Vec<(esq_core::model::Annotation, bool, crate::annot::AnnotStyle)>,
    last_mouse: Option<DVec2>,
    /// Shift pressed and not yet used with anything else (Position Texture: Fixed Pins).
    shift_alone: bool,
    /// Where the right-click menu was opened (Texture › Position puts its pins there).
    pub(crate) context_point: Option<DVec2>,
    orbit_drag: Option<(DVec2, Option<DVec3>)>,
    prev_mods: Modifiers,
    ctrl_alone: bool,
    alt_alone: bool,
    pub clipboard: Option<(Model, DefId)>,
    /// The Windows clipboard's sequence number right after this window copied.
    clip_seq: Option<u32>,
    pub confirm_close: bool,
    allow_close: bool,
    snapshot: Option<PathBuf>,
    frames: u32,
    pub ui_state: crate::ui::UiState,
}

impl EsquisseApp {
    pub fn new(cc: &eframe::CreationContext, file: Option<PathBuf>, snapshot: Option<PathBuf>) -> Self {
        crate::ui::setup_style(&cc.egui_ctx);
        crate::tools::move_grips::load_grips_pref();
        // Preferences/IntersectEdges (`Sketchup.break_edges=`), for the tools and the scripts.
        esq_core::ops::set_break_edges(crate::settings::with(|s| s.intersect_edges));
        let mut app = Self::bare(cc.wgpu_render_state.as_ref().map(Viewport3d::new), snapshot);
        app.gpu = cc.wgpu_render_state.clone();
        match file {
            Some(f) => app.open_path(&f),
            None => {
                app.new_document();
                app.ui_state.welcome = app.snapshot.is_none() && crate::settings::with(|s| s.show_welcome);
            }
        }
        // `--snapshot` only: display effects to turn on, e.g. ESQ_FX=depth,ao,photoreal,twopoint.
        if app.snapshot.is_some()
            && let Ok(fx) = std::env::var("ESQ_FX")
        {
            crate::style::apply_fx(&fx, &mut app.style, &mut app.cam);
        }
        app
    }

    /// The application with an empty model, without a window (tests drive it headless).
    pub(crate) fn bare(view: Option<Viewport3d>, snapshot: Option<PathBuf>) -> Self {
        EsquisseApp {
            doc: Document::new(Model::new()),
            cam: Camera::default(),
            tool: ToolId::Select.make(),
            view,
            built_rev: 0,
            pick: PickCache::default(),
            pick_rev: 0,
            pick_eye_stale: false,
            scene_parts: None,
            eye_rev: 0,
            turning: None,
            vcb: String::new(),
            message: None,
            arrow_lock: None,
            shift_lock: None,
            style: ViewStyle::default(),
            pick_style: ViewStyle::default(),
            material: None,
            tag: 0,
            last_hidden: Vec::new(),
            cam_back: Vec::new(),
            cam_fwd: Vec::new(),
            recent_files: crate::prefs::load_recent(),
            gpu: None,
            ppp: 1.0,
            quit_requested: false,
            pending_cmds: Vec::new(),
            active_scene: None,
            photo_shown: None,
            transition: None,
            playing: None,
            anim: Default::default(),
            shadow_edit: None,
            recent: None,
            guide_pts: Vec::new(),
            annots: Vec::new(),
            last_mouse: None,
            shift_alone: false,
            context_point: None,
            orbit_drag: None,
            prev_mods: Modifiers::NONE,
            ctrl_alone: false,
            alt_alone: false,
            clipboard: None,
            clip_seq: None,
            confirm_close: false,
            allow_close: false,
            snapshot,
            frames: 0,
            ui_state: Default::default(),
        }
    }

    /// Window › Components: the chosen component follows the cursor until a click.
    pub fn place_component(&mut self, def: DefId) {
        self.set_tool(ToolId::Select);
        self.tool = Box::new(crate::tools::PlaceTool::new(def));
    }

    /// Like [`Self::place_component`], for 3D text: it lies on the face under the cursor.
    pub fn place_component_glued(&mut self, def: DefId) {
        self.set_tool(ToolId::Select);
        self.tool = Box::new(crate::tools::PlaceTool::glued(def));
    }

    /// Cancels what the active tool was doing.
    pub fn cancel_tool(&mut self) {
        self.with_cx(Modifiers::NONE, |t, cx| t.cancel(cx));
    }

    pub fn say(&mut self, ctx: &egui::Context, msg: impl Into<String>) {
        self.message = Some((msg.into(), ctx.input(|i| i.time)));
    }

    pub fn set_tool(&mut self, id: ToolId) {
        self.cancel_tool();
        self.tool = id.make();
        self.vcb.clear();
        self.arrow_lock = None;
    }

    // ---------------------------------------------------------------------------------
    // Files

    /// Reads a model file (.skp, else .esq) and the view it was saved with.
    pub(crate) fn read_model(path: &Path) -> anyhow::Result<(Model, Option<esq_skp::View>)> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        match ext.as_str() {
            "skp" => esq_skp::import_skp_view(path),
            _ => esq_core::io::load_esq(path).map(|m| (m, None)),
        }
    }

    pub fn open_path(&mut self, path: &Path) {
        match Self::read_model(path) {
            Ok((m, view)) => self.open_model(path, m, view),
            Err(e) => self.message = Some((format!("Impossible d'ouvrir : {e:#}"), 0.0)),
        }
    }

    /// The document becomes `m`, read from `path` (`read_model`).
    pub(crate) fn open_model(&mut self, path: &Path, m: Model, view: Option<esq_skp::View>) {
        self.doc = Document::new(m);
        self.style_from_model();
        self.ui_state.extra.ruby_tools.replaced = Some("open");
        self.doc.path = Some(path.to_path_buf());
        self.material = None;
        self.last_hidden.clear();
        self.cam_back.clear();
        self.cam_fwd.clear();
        self.refresh_caches();
        self.add_recent(path);
        // The scene selected when the file was saved (its tab is highlighted again).
        self.active_scene = self
            .doc
            .model
            .view_ext
            .get(esq_core::model::ACTIVE_SCENE)
            .and_then(|v| v.as_u64())
            .map(|i| i as usize)
            .filter(|i| *i < self.doc.model.scenes.len());
        self.playing = None;
        self.transition = None;
        match view {
            Some(v) => self.cam.set_from_view(&v),
            None => self.zoom_extents(),
        }
        self.open_photo_scene();
        self.message = Some((format!("Ouvert : {}", path.display()), 0.0));
        crate::dialogs::pref_general::after_open(self);
        crate::comments::after_open(self);
    }

    /// Writes the document as .esq or as .skp of a SketchUp version; the document then has
    /// that file.
    pub fn save_as_format(&mut self, path: &Path, fmt: crate::skpsave::SaveFormat) -> anyhow::Result<()> {
        crate::dialogs::mi_file::before_save(self);
        self.style_to_model();
        // The selected scene is saved with the document, as SketchUp does (SUModelSetActiveScene).
        let key = esq_core::model::ACTIVE_SCENE;
        match self.active_scene.filter(|i| *i < self.doc.model.scenes.len()) {
            Some(i) => self.doc.model.view_ext.insert(key.into(), i.into()),
            None => self.doc.model.view_ext.remove(key),
        };
        crate::settings::backup_before_save(path);
        match fmt {
            crate::skpsave::SaveFormat::Skp(v) => {
                let thumbnail = self.skp_thumbnail(None);
                esq_skp::export_skp_with(&self.doc.model, path, Some(&self.cam.to_view()), v, thumbnail.as_deref())?
            }
            crate::skpsave::SaveFormat::Esq => esq_core::io::save_esq(&self.doc.model, path)?,
        }
        self.doc.path = Some(path.to_path_buf());
        self.doc.dirty = false;
        crate::dialogs::pref_general::autosave_done(self);
        Ok(())
    }

    /// Saves to `path` in the format of its extension (.skp of the current version, .esq),
    /// or exports .obj / .stl without changing the document's file (the tests save so).
    #[cfg(test)]
    pub fn save_to(&mut self, path: &Path) -> anyhow::Result<()> {
        match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() {
            "obj" => esq_core::io::export_obj(&self.doc.model, path),
            "stl" => esq_core::io::export_stl(&self.doc.model, path),
            _ => self.save_as_format(path, crate::skpsave::format_of(path)),
        }
    }

    pub fn save(&mut self, ctx: &egui::Context, save_as: bool) {
        // Preferences › General › Ask to purge at save.
        if crate::dialogs::pref_general::purge_prompt(self, save_as) {
            return;
        }
        let target = match (&self.doc.path, save_as) {
            (Some(p), false) => Some((p.clone(), crate::skpsave::format_of(p))),
            _ => {
                let current = self.doc.path.as_deref().map(crate::skpsave::format_of);
                let name = self.doc.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
                crate::skpsave::ask(
                    "Enregistrer sous",
                    &name.unwrap_or_else(|| "modele".into()),
                    current.unwrap_or(crate::skpsave::SaveFormat::Esq),
                )
            }
        };
        if let Some((p, fmt)) = target {
            match self.save_as_format(&p, fmt) {
                Ok(()) => self.say(ctx, format!("Enregistré : {}", p.display())),
                Err(e) => self.say(ctx, format!("Échec de l'enregistrement : {e:#}")),
            }
        }
    }

    fn add_recent(&mut self, path: &Path) {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let path = PathBuf::from(path.to_string_lossy().trim_start_matches(r"\\?\").to_string());
        self.recent_files.retain(|p| p != &path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(RECENT_MAX);
        crate::prefs::save_recent(&self.recent_files);
    }

    /// File › Save a Copy As: writes the model elsewhere, the document keeps its file.
    pub fn save_copy(&mut self, ctx: &egui::Context) {
        let current = self.doc.path.as_deref().map(crate::skpsave::format_of);
        let Some((p, fmt)) = crate::skpsave::ask(
            "Enregistrer une copie sous",
            "copie",
            current.unwrap_or(crate::skpsave::SaveFormat::Esq),
        ) else {
            return;
        };
        let (path, dirty) = (self.doc.path.clone(), self.doc.dirty);
        let res = self.save_as_format(&p, fmt);
        (self.doc.path, self.doc.dirty) = (path, dirty);
        match res {
            Ok(()) => self.say(ctx, format!("Copie enregistrée : {}", p.display())),
            Err(e) => self.say(ctx, format!("Échec de l'enregistrement : {e:#}")),
        }
    }

    /// File › Revert: reloads the file from disk.
    pub fn revert(&mut self) {
        if let Some(p) = self.doc.path.clone() {
            let cam = self.cam.clone();
            self.open_path(&p);
            self.cam = cam;
        }
    }

    pub fn open_dialog(&mut self) {
        if let Some(p) = crate::settings::dialog("models").add_filter("Modèles", &["esq", "skp"]).pick_file() {
            self.open_path(&p);
        }
    }

    pub fn new_document(&mut self) {
        let t = crate::prefs::default_template();
        self.new_from_template(&t);
    }

    pub fn new_from_template(&mut self, t: &crate::prefs::Template) {
        self.doc = Document::new(t.model());
        self.style_from_model();
        self.ui_state.extra.ruby_tools.replaced = Some("new");
        self.active_scene = None;
        self.playing = None;
        self.transition = None;
        self.cam = Camera::default();
        self.last_hidden.clear();
        self.cam_back.clear();
        self.cam_fwd.clear();
        self.set_tool(ToolId::Select);
    }

    // ---------------------------------------------------------------------------------
    // Edit commands

    pub fn erase_selection(&mut self) {
        let items = self.doc.sel.clone();
        if items.is_empty() {
            return;
        }
        // Deleting a face alone keeps its edges; deleting edges also removes their faces.
        self.doc.commit("Supprimer", |m, d| m.entities_mut(d).erase(&items));
        self.doc.select(Vec::new());
        if self.doc.sel.is_empty() && self.locked_in(&items) {
            self.message = Some(("Les groupes et composants verrouillés n'ont pas été supprimés.".into(), 0.0));
        }
    }

    fn locked_in(&self, items: &[EntityRef]) -> bool {
        let ents = self.doc.ents();
        items.iter().any(|e| matches!(e, EntityRef::Instance(i) if ents.instances.get(*i).is_some_and(|x| x.locked)))
    }

    /// Edit › Lock / Unlock (the selected instances, or with `all` every instance of the
    /// active context).
    pub fn set_locked(&mut self, lock: bool, all: bool) {
        let ids: Vec<_> = if all {
            self.doc.ents().instances.keys().collect()
        } else {
            self.doc.sel.iter().filter_map(|e| if let EntityRef::Instance(i) = e { Some(*i) } else { None }).collect()
        };
        self.doc.commit(if lock { "Verrouiller" } else { "Déverrouiller" }, |m, d| {
            let ents = m.entities_mut(d);
            for i in ids {
                if let Some(x) = ents.instances.get_mut(i) {
                    x.locked = lock;
                }
            }
        });
    }

    pub fn is_hidden(&self, e: EntityRef) -> bool {
        self.doc.ents().is_hidden(e)
    }

    /// Edit › Reveal › Last.
    pub fn reveal_last(&mut self) {
        let items = std::mem::take(&mut self.last_hidden);
        let keep = self.doc.sel.clone();
        self.doc.sel = items;
        self.hide_selection(false);
        self.doc.select(keep);
    }

    /// Edit › Invert Selection, within the active context.
    pub fn invert_selection(&mut self) {
        let sel: std::collections::HashSet<EntityRef> = self.doc.sel.iter().copied().collect();
        let e = self.doc.ents();
        let all = e
            .edges
            .keys()
            .map(EntityRef::Edge)
            .chain(e.faces.keys().map(EntityRef::Face))
            .chain(e.instances.keys().map(EntityRef::Instance))
            .chain(e.guides.keys().map(EntityRef::Guide))
            .chain(e.annots.keys().map(EntityRef::Annot))
            .chain(e.sections.keys().map(EntityRef::Section))
            .chain(e.mark_refs());
        let inv: Vec<EntityRef> = all.filter(|x| !sel.contains(x) && !self.is_hidden(*x)).collect();
        self.doc.select(inv);
    }

    /// Edit › Intersect Faces.
    pub fn intersect(&mut self, ctx: &egui::Context, with: esq_core::intersect::IntersectWith) {
        let items = self.doc.sel.clone();
        let mut n = 0;
        self.doc.commit("Intersection des faces", |m, d| n = m.intersect_faces(d, &items, with));
        self.say(
            ctx,
            if n == 0 { "Aucune intersection trouvée".to_string() } else { format!("{n} segment(s) d'intersection") },
        );
    }

    pub fn make_group(&mut self, is_group: bool) {
        let items = self.doc.sel.clone();
        if items.is_empty() {
            return;
        }
        let mut new_inst = None;
        self.doc.commit(if is_group { "Créer un groupe" } else { "Créer un composant" }, |m, d| {
            new_inst = m.make_group(d, &items, is_group, None);
        });
        if let Some(i) = new_inst {
            self.doc.select(vec![EntityRef::Instance(i)]);
        }
    }

    pub fn explode_selection(&mut self) {
        let insts: Vec<_> =
            self.doc.sel.iter().filter_map(|e| if let EntityRef::Instance(i) = e { Some(*i) } else { None }).collect();
        if insts.is_empty() {
            return;
        }
        let mut out = Vec::new();
        self.doc.commit("Éclater", |m, d| {
            for i in insts {
                out.extend(m.explode(d, i));
            }
        });
        self.doc.select(out);
    }

    pub fn reverse_selected_faces(&mut self) {
        let faces: Vec<_> =
            self.doc.sel.iter().filter_map(|e| if let EntityRef::Face(f) = e { Some(*f) } else { None }).collect();
        if faces.is_empty() {
            return;
        }
        self.doc.commit("Inverser les faces", |m, d| {
            let ents = m.entities_mut(d);
            for f in faces {
                ents.flip_face(f);
            }
        });
    }

    pub fn select_all(&mut self) {
        let e = self.doc.ents();
        let mut all: Vec<EntityRef> = e.edges.keys().map(EntityRef::Edge).collect();
        all.extend(e.faces.keys().map(EntityRef::Face));
        all.extend(e.instances.keys().map(EntityRef::Instance));
        all.extend(e.guides.keys().map(EntityRef::Guide));
        all.extend(e.annots.keys().map(EntityRef::Annot));
        all.extend(e.sections.keys().map(EntityRef::Section));
        all.extend(e.mark_refs());
        self.doc.select(all);
    }

    pub fn hide_selection(&mut self, hide: bool) {
        let items = self.doc.sel.clone();
        if hide {
            self.last_hidden = items.clone();
        }
        self.doc.commit(if hide { "Masquer" } else { "Révéler" }, |m, d| {
            let ents = m.entities_mut(d);
            for it in items {
                ents.set_hidden(it, hide);
            }
        });
        if hide {
            self.doc.select(Vec::new());
        }
    }

    /// Edit › Delete Guides: every guide of the active context.
    pub fn delete_guides(&mut self) {
        if self.doc.ents().guides.is_empty() {
            return;
        }
        self.doc.commit("Supprimer les guides", |m, d| m.entities_mut(d).guides.clear());
    }

    pub fn unhide_all(&mut self) {
        self.doc.commit("Révéler tout", |m, d| {
            let ents = m.entities_mut(d);
            ents.edges.values_mut().for_each(|e| e.hidden = false);
            ents.faces.values_mut().for_each(|f| f.hidden = false);
            ents.instances.values_mut().for_each(|i| i.hidden = false);
            ents.guides.values_mut().for_each(|g| g.hidden = false);
            ents.annots.values_mut().for_each(|a| a.hidden = false);
            ents.snaps.iter_mut().for_each(|s| s.hidden = false);
            ents.spaces.iter_mut().for_each(|s| s.hidden = false);
        });
    }

    pub fn copy(&mut self) {
        if self.doc.sel.is_empty() {
            return;
        }
        let mut m = self.doc.model.clone();
        let def = self.doc.ctx_def();
        let items = self.doc.sel.clone();
        if let Some(inst) = m.make_group(def, &items, true, Some("Presse-papiers".into())) {
            let d = m.entities(def).instances[inst].def;
            let xf = m.entities(def).instances[inst].xform;
            // Keep the copied geometry where it was (in context coordinates).
            m.defs[d].entities.verts.values_mut().for_each(|v| v.pos = xf.transform_point3(v.pos));
            for i in m.defs[d].entities.instances.values_mut() {
                i.xform = xf * i.xform;
            }
            let e = &mut m.defs[d].entities;
            e.snaps.iter_mut().for_each(|s| *s = s.transformed(&xf));
            e.spaces.iter_mut().for_each(|s| *s = s.transformed(&xf));
            // Only what was copied is kept (its definitions, materials and tags), not a copy of
            // the whole model.
            let mut clip = esq_core::model::Model::new();
            let d = clip.import_def(&m, d, &mut esq_core::groups::ImportMap::default());
            // Also on the Windows clipboard, for the other windows (`clipboard.rs`).
            self.clip_seq =
                crate::clipboard::with(|s| s.filter(|s| crate::clipboard::put(*s, &clip, d)).map(|s| s.sequence()));
            self.clipboard = Some((clip, d));
        }
    }

    /// What Paste takes: what this window copied while the clipboard still holds it,
    /// else what another window of Esquisse (or SketchUp) put on it.
    fn clipboard_content(&mut self) -> Option<(Model, DefId)> {
        let seq = self.clip_seq;
        let foreign = crate::clipboard::with(|s| {
            let s = s?;
            if self.clipboard.is_some() && Some(s.sequence()) == seq {
                return None;
            }
            crate::clipboard::take(s)
        });
        match foreign {
            Some(Ok(c)) => Some(c),
            Some(Err(e)) => {
                self.message = Some((format!("Impossible de coller : {e}"), 0.0));
                None
            }
            None => self.clipboard.clone(),
        }
    }

    /// Whether Paste has something to paste.
    pub fn can_paste(&self) -> bool {
        self.clipboard.is_some() || crate::clipboard::with(|s| s.is_some_and(crate::clipboard::has_any))
    }

    pub fn cut(&mut self) {
        self.copy();
        self.erase_selection();
    }

    /// Edit › Paste (then the Move tool takes the pasted geometry) or Paste In Place.
    pub fn paste(&mut self, then_move: bool) {
        let Some((src, d)) = self.clipboard_content() else { return };
        if then_move {
            // Before pasting: a paste still following the cursor is cancelled first.
            self.set_tool(ToolId::Move);
        }
        let mut pasted = Vec::new();
        self.doc.commit("Coller", |m, ctx| {
            let mut map = esq_core::groups::ImportMap::default();
            let def = m.import_def(&src, d, &mut map);
            let inst = m.entities_mut(ctx).instances.insert(esq_core::model::Instance {
                def,
                xform: DMat4::IDENTITY,
                name: String::new(),
                material: None,
                layer: 0,
                hidden: false,
                locked: false,
                attributes: Default::default(),
                ext: Default::default(),
            });
            pasted = m.explode(ctx, inst);
            m.defs.remove(def);
        });
        self.doc.select(pasted);
        if then_move {
            // CPasteTool: the pasted geometry follows the cursor until a click places it.
            self.tool = Box::new(crate::tools::MoveTool::pasting());
        }
    }

    pub fn enter_selected(&mut self) {
        if let [EntityRef::Instance(i)] = self.doc.sel.as_slice() {
            let i = *i;
            self.doc.enter(i);
        }
    }

    pub fn zoom_extents(&mut self) {
        let (mut min, mut max) = scene::model_bounds(&self.doc.model);
        // What the Ruby tool and the overlays draw counts too (their getExtents).
        for &(a, b) in &self.ui_state.extra.ruby_tools.extents {
            (min, max) = if min.is_finite() { (min.min(a), max.max(b)) } else { (a, b) };
        }
        self.cam.zoom_extents(min, max);
    }

    /// Sun and shadows for the renderer, from Window › Shadows.
    pub fn lighting(&self) -> crate::render::Lighting {
        let s = &self.doc.model.shadow;
        // Model Info › Geo-location › Use solar orientation off: the sun ignores the north.
        let solar = crate::dialogs::mi_file::solar_orientation(&self.doc.model);
        let sun = if solar {
            s.sun_direction()
        } else {
            esq_core::sun::ShadowInfo { north_angle: 0.0, ..s.clone() }.sun_direction()
        };
        crate::render::Lighting {
            // A sun linked to the environment (Set Sun Location) replaces the one of the date.
            sun: (s.display || s.use_sun_for_shading)
                .then(|| crate::edit_ops::linked_sun_direction(&self.doc.model).unwrap_or(sun)),
            light: s.light as f32 / 100.0,
            dark: s.dark as f32 / 100.0,
            cast: s.display,
            on_faces: s.on_faces,
            on_ground: s.on_ground,
        }
    }

    /// Right-click menu of a section plane.
    pub fn section_cmd(&mut self, cmd: crate::commands::Cmd) {
        use crate::commands::Cmd;
        let Some(s) = self.sel_section() else { return };
        match cmd {
            Cmd::ReverseSection => self.doc.commit("Inverser le plan de section", |m, d| {
                let sp = &mut m.entities_mut(d).sections[s];
                sp.normal = -sp.normal;
            }),
            Cmd::ActiveCut => self.doc.commit("Activer la coupe", |m, d| {
                let ents = m.entities_mut(d);
                ents.active_section = if ents.active_section == Some(s) { None } else { Some(s) };
            }),
            Cmd::AlignView => {
                // Look straight at the cut, in the direction of the arrows.
                let x = self.doc.ctx_xform();
                let n = self.doc.ents().sections[s].transformed(&x).normal;
                self.remember_view();
                self.cam.pitch = (-n.z).clamp(-1.0, 1.0).asin().clamp(-1.55, 1.55);
                if n.x.abs() + n.y.abs() > 1e-9 {
                    self.cam.yaw = (-n.y).atan2(-n.x);
                }
                self.zoom_extents();
            }
            Cmd::GroupFromSlice => {
                let mut made = None;
                self.doc.commit("Créer un groupe à partir de la tranche", |m, d| made = m.group_from_slice(d, s));
                if let Some(i) = made {
                    self.doc.select(vec![EntityRef::Instance(i)]);
                }
            }
            _ => {}
        }
    }

    /// Zoom on the selection's bounding box.
    pub fn zoom_selection(&mut self) {
        let ents = self.doc.ents();
        let x = self.doc.ctx_xform();
        let mut pts: Vec<DVec3> =
            ents.vertices_of(&self.doc.sel).iter().map(|v| x.transform_point3(ents.pos(*v))).collect();
        for e in &self.doc.sel {
            if let EntityRef::Instance(i) = e
                && let Some(inst) = ents.instances.get(*i)
            {
                let mut sub = Model { root: self.doc.model.defs[inst.def].entities.clone(), ..Model::new() };
                sub.defs = self.doc.model.defs.clone();
                let (a, b) = scene::model_bounds(&sub);
                if a.is_finite() && b.is_finite() {
                    let m = x * inst.xform;
                    for c in 0..8 {
                        let p = esq_core::glam::DVec3::new(
                            if c & 1 == 0 { a.x } else { b.x },
                            if c & 2 == 0 { a.y } else { b.y },
                            if c & 4 == 0 { a.z } else { b.z },
                        );
                        pts.push(m.transform_point3(p));
                    }
                }
            }
            if let EntityRef::Guide(g) = e
                && let Some(guide) = ents.guides.get(*g)
            {
                pts.push(x.transform_point3(guide.pos));
            }
        }
        if pts.is_empty() {
            return;
        }
        self.remember_view();
        let min = pts.iter().fold(DVec3::splat(f64::INFINITY), |a, p| a.min(*p));
        let max = pts.iter().fold(DVec3::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
        self.cam.zoom_extents(min, max);
    }

    /// Keeps the current view for Camera › Previous.
    pub fn remember_view(&mut self) {
        if self.cam_back.last().is_none_or(|c| !same_view(c, &self.cam)) {
            self.cam_back.push(self.cam.clone());
            if self.cam_back.len() > CAMERA_HISTORY {
                self.cam_back.remove(0);
            }
        }
        self.cam_fwd.clear();
    }

    pub fn camera_back(&mut self) {
        if let Some(c) = self.cam_back.pop() {
            let size = self.cam.size;
            self.cam_fwd.push(std::mem::replace(&mut self.cam, c));
            self.cam.size = size;
        }
    }

    pub fn camera_forward(&mut self) {
        if let Some(c) = self.cam_fwd.pop() {
            let size = self.cam.size;
            self.cam_back.push(std::mem::replace(&mut self.cam, c));
            self.cam.size = size;
        }
    }

    // ---------------------------------------------------------------------------------
    // Frame

    fn refresh_caches(&mut self) {
        if self.pick_stale() {
            let pick = PickCache::build(&self.doc.model, &self.style);
            self.set_pick(pick);
        }
    }

    fn pick_stale(&self) -> bool {
        self.pick_rev != self.doc.model_rev || self.pick_style != self.style
    }

    /// `pick` made from the model and style as they are.
    fn set_pick(&mut self, pick: PickCache) {
        self.pick = pick;
        self.pick_rev = self.doc.model_rev;
        self.pick_style = self.style.clone();
        self.pick_eye_stale = false;
    }

    /// `pick` with the components that turned to the camera at the start of this image
    /// (`follow_eye` leaves that rebuild to the first use: an orbit does not pay it at each
    /// image when nothing looks under the cursor). Called before anything of the image reads
    /// `pick` and before anything changes the model after `follow_eye`, it builds what
    /// `follow_eye` would have built.
    pub(crate) fn ensure_pick(&mut self) {
        if std::mem::take(&mut self.pick_eye_stale) {
            self.pick = PickCache::build(&self.doc.model, &self.style);
        }
    }

    pub fn current_lock(&self) -> Option<Lock> {
        self.shift_lock.or(self.arrow_lock)
    }

    /// Drawing axes: those of the open group, or the model's (Axes tool) at the top level.
    fn axes(&self) -> DMat4 {
        if self.doc.ctx.is_empty() { self.doc.model.axes } else { self.doc.ctx_xform() }
    }

    pub(crate) fn with_cx<R>(&mut self, mods: Modifiers, f: impl FnOnce(&mut dyn Tool, &mut ToolCx) -> R) -> R {
        self.ensure_pick();
        let axes = self.axes();
        let lock = self.current_lock();
        let units = self.doc.model.units;
        let mut msg = None;
        let mut mat = self.material;
        let mut tag = self.tag;
        let mut new_cam = None;
        let mut edit_text = None;
        let mut show_sections = false;
        let r = {
            let mut cx = ToolCx {
                doc: &mut self.doc,
                infer: InferCx { cam: &self.cam, pick: &self.pick, axes, recent: self.recent },
                lock,
                mods,
                units,
                material: &mut mat,
                tag: &mut tag,
                message: &mut msg,
                new_cam: &mut new_cam,
                edit_text: &mut edit_text,
                show_sections: &mut show_sections,
            };
            f(self.tool.as_mut(), &mut cx)
        };
        // Position Texture done, a material sampled: the tool hands over.
        if let Some(t) = self.tool.switch_to() {
            self.tool = t.make();
            self.vcb.clear();
        }
        if show_sections && !self.style.section_planes {
            self.style.section_planes = true;
            self.doc.rev += 1;
        }
        if let Some(a) = edit_text {
            let text = self.doc.ents().annots.get(a).map(|x| x.text.clone()).unwrap_or_default();
            self.ui_state.edit_text = Some((a, text));
        }
        self.material = mat;
        self.tag = tag;
        if let Some(c) = new_cam {
            let size = self.cam.size;
            self.cam = c;
            self.cam.size = size;
        }
        if let Some(m) = msg {
            self.message = Some((m, 0.0));
        }
        r
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (events, mods) = ctx.input(|i| (i.events.clone(), i.modifiers));

        // Ctrl tapped alone toggles copy / keep-base in the active tool.
        if mods.ctrl && !self.prev_mods.ctrl {
            self.ctrl_alone = true;
        }
        if !mods.ctrl && self.prev_mods.ctrl && self.ctrl_alone {
            self.ctrl_alone = false;
            self.with_cx(mods, |t, cx| t.ctrl_tap(cx));
        }
        // Alt tapped alone: auto-fold (Move), grip types, etc.
        if mods.alt && !self.prev_mods.alt {
            self.alt_alone = true;
        }
        if !mods.alt && self.prev_mods.alt && self.alt_alone {
            self.alt_alone = false;
            self.with_cx(mods, |t, cx| t.alt_tap(cx));
        }
        // Shift held locks the current inference direction.
        if mods.shift
            && !self.prev_mods.shift
            && let Some(s) = self.tool.snap()
        {
            self.shift_lock = match s.kind {
                SnapKind::Axis(_) => {
                    s.guide.map(|(a, b, c)| Lock { dir: (b - a).normalize_or(DVec3::X), color: c, kind: s.kind })
                }
                SnapKind::OnEdge | SnapKind::OnGuide => s.edge.as_ref().map(|(_, (a, b))| Lock {
                    dir: (*b - *a).normalize(),
                    color: crate::settings::parallel_color(),
                    kind: SnapKind::Parallel,
                }),
                SnapKind::Parallel | SnapKind::Perpendicular => {
                    s.guide.map(|(a, b, c)| Lock { dir: (b - a).normalize_or(DVec3::X), color: c, kind: s.kind })
                }
                _ => None,
            };
        }
        if !mods.shift && self.prev_mods.shift {
            self.shift_lock = None;
        }
        // Shift tapped alone (Position Texture toggles Fixed Pins).
        if mods.shift && !self.prev_mods.shift {
            self.shift_alone = true;
        }
        if ctx.input(|i| i.pointer.any_down()) {
            self.shift_alone = false;
        }
        if !mods.shift && self.prev_mods.shift && std::mem::take(&mut self.shift_alone) {
            self.with_cx(mods, |t, cx| t.shift_tap(cx));
        }
        // A Ruby tool hears every key (onKeyDown / onKeyUp), modifiers included.
        if self.tool.ruby().is_some() || self.tool.wants_keys() {
            let prev = self.prev_mods;
            for (now, before, vk) in
                [(mods.shift, prev.shift, 16), (mods.ctrl, prev.ctrl, 17), (mods.alt, prev.alt, 18)]
            {
                if now != before {
                    self.with_cx(mods, |t, cx| t.key(cx, vk, now, 1));
                }
            }
            for ev in &events {
                if let egui::Event::Key { key, pressed, repeat, .. } = ev
                    && let Some(vk) = crate::tools::ruby::vk_of(*key)
                {
                    let (down, n) = (*pressed, if *repeat { 2 } else { 1 });
                    self.with_cx(mods, |t, cx| t.key(cx, vk, down, n));
                }
            }
        }
        self.prev_mods = mods;

        for ev in events {
            match ev {
                egui::Event::Text(t) => {
                    self.ctrl_alone = false;
                    self.alt_alone = false;
                    self.shift_alone = false;
                    // Typing a value goes to the Measurements box, without clicking it.
                    let starts_vcb = t.chars().next().is_some_and(|c| c.is_ascii_digit() || "-.,[<*/'\"".contains(c));
                    if !self.vcb.is_empty() || starts_vcb {
                        self.vcb.push_str(&t);
                    }
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    self.ctrl_alone = false;
                    self.alt_alone = false;
                    self.shift_alone = false;
                    // Keys the tool uses itself (Position Texture: arrows, Escape).
                    if self.vcb.is_empty()
                        && (self.with_cx(mods, |t, cx| t.own_key(cx, key, modifiers))
                            || (key == Key::Escape && self.with_cx(mods, |t, cx| t.escape(cx))))
                    {
                        continue;
                    }
                    match key {
                        Key::Enter if !self.vcb.is_empty() => {
                            let text = std::mem::take(&mut self.vcb);
                            let res = self.with_cx(mods, |t, cx| t.vcb(cx, &text));
                            if let Err(e) = res {
                                self.say(ctx, e);
                            }
                        }
                        Key::Enter if self.tool.ruby().is_some() || self.tool.wants_keys() => {
                            self.with_cx(mods, |t, cx| t.enter(cx))
                        }
                        Key::Backspace if !self.vcb.is_empty() => {
                            self.vcb.pop();
                        }
                        Key::Escape => {
                            if !self.vcb.is_empty() {
                                self.vcb.clear();
                            } else if self.tool.busy() {
                                self.with_cx(mods, |t, cx| t.cancel(cx));
                            } else if !self.doc.ctx.is_empty() {
                                self.doc.exit();
                            } else if self.tool.id() != ToolId::Select {
                                self.set_tool(ToolId::Select);
                            } else {
                                self.doc.select(Vec::new());
                            }
                        }
                        Key::ArrowRight | Key::ArrowLeft | Key::ArrowUp
                            if !modifiers.ctrl && !self.tool.wants_keys() =>
                        {
                            let (i, color) = match key {
                                Key::ArrowRight => (0, crate::settings::axis_color(0)),
                                Key::ArrowLeft => (1, crate::settings::axis_color(1)),
                                _ => (2, crate::settings::axis_color(2)),
                            };
                            let dir = self.axes().col(i).truncate().normalize();
                            let same = self.arrow_lock.is_some_and(|l| l.kind == SnapKind::Axis(i));
                            self.arrow_lock =
                                if same { None } else { Some(Lock { dir, color, kind: SnapKind::Axis(i) }) };
                        }
                        Key::ArrowDown => self.arrow_lock = None,
                        Key::Plus | Key::Equals | Key::Minus if modifiers.ctrl || modifiers.command => {
                            let delta = if key == Key::Minus { -1 } else { 1 };
                            self.with_cx(mods, |t, cx| t.step(cx, delta));
                        }
                        _ if self.vcb.is_empty() => {
                            if let Some(c) = Self::cmd_for_key(key, modifiers) {
                                self.run(ctx, c);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    /// Start of a frame of the 3D view: components that always face the camera turn with it
    /// (`dialogs::comp_edit`), and the picking cache follows the model.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn follow_eye(&mut self) {
        self.turn_with_eye();
        self.refresh_caches();
    }

    /// `follow_eye` but the picking cache, which `prepare_scene` makes beside the mesh.
    pub(crate) fn turn_with_eye(&mut self) {
        if crate::dialogs::comp_edit::follow_camera(&self.doc.model, self.cam.to_view().eye) {
            self.pick_eye_stale = true;
            self.doc.rev += 1;
            self.eye_rev += 1;
        }
    }

    /// The scene's mesh on the GPU, rebuilt when what the view shows changed.
    pub(crate) fn prepare_scene(&mut self, rs: &eframe::egui_wgpu::RenderState) {
        // Preferences › Graphics changed the anti-aliasing or the shadows: a new renderer.
        if self.view.as_ref().is_some_and(|v| crate::render::config_changed(v.config)) {
            self.view = Some(Viewport3d::new(rs));
            self.built_rev = 0;
        }
        if self.built_rev != self.doc.rev || self.view.is_none() {
            let axes = self.axes();
            // When only the selection or the eye changed since the mesh was built, only the
            // parts that show them are made again (`scene::update_parts`).
            let key = ViewKey {
                doc: self.doc.id,
                rest: self.doc.rev.wrapping_sub(self.doc.sel_rev).wrapping_sub(self.eye_rev),
                model_rev: self.doc.model_rev,
                style: self.style.clone(),
                ctx: self.doc.ctx.clone(),
                ctx_def: self.doc.ctx_def(),
                axes: axes.to_cols_array().map(f64::to_bits),
                upload: self.view.as_ref().map_or(0, |v| v.uploads()),
            };
            let display = self.doc.display_model();
            let opts = SceneOptions {
                ctx: &self.doc.ctx,
                ctx_def: self.doc.ctx_def(),
                sel: &self.doc.sel,
                axes_xform: axes,
                style: &self.style,
            };
            let cached = self.doc.preview.is_none() && self.scene_parts.as_ref().is_some_and(|(k, _)| *k == key);
            let patched = cached
                && match (&mut self.scene_parts, self.view.as_mut()) {
                    (Some((_, parts)), Some(v)) => {
                        scene::update_parts(display, &opts, parts).map(|p| v.patch(rs, &p)).is_some()
                    }
                    _ => false,
                };
            if !patched {
                let pick_stale = self.pick_stale();
                let view = &mut self.view;
                let mut mesh = || {
                    let (mesh, parts) = scene::build_parts(display, &opts);
                    if let Some(v) = view.as_mut() {
                        v.upload(rs, &mesh);
                    }
                    parts
                };
                // After a change of the model, the picking cache is made on another thread
                // while the mesh is made and sent.
                let (pick, parts) = if pick_stale {
                    let (pick, parts) = PickCache::build_beside(&self.doc.model, &self.style, mesh);
                    (Some(pick), parts)
                } else {
                    (None, mesh())
                };
                let upload = self.view.as_ref().map_or(0, |v| v.uploads());
                self.scene_parts = self.doc.preview.is_none().then_some((ViewKey { upload, ..key }, parts));
                if let Some(pick) = pick {
                    self.set_pick(pick);
                }
            }
            let display = self.doc.display_model();
            let opts = SceneOptions {
                ctx: &self.doc.ctx,
                ctx_def: self.doc.ctx_def(),
                sel: &self.doc.sel,
                axes_xform: axes,
                style: &self.style,
            };
            self.guide_pts = scene::guide_points(display, &opts);
            self.annots = crate::annot::styled_annotations(display, opts.ctx, opts.sel);
            self.built_rev = self.doc.rev;
        }
        self.refresh_caches();
    }

    /// Before `paint_model_overlay`: guide points hidden by a face are not drawn, which looks
    /// under them.
    pub(crate) fn ensure_overlay_pick(&mut self) {
        // While the camera turns components that face it, the picking cache waits; what
        // turns is placed again for this image alone (`pick::Turning`).
        if !self.guide_pts.is_empty()
            && self.pick_eye_stale
            && self.turning.as_ref().is_none_or(|(rev, _)| *rev != self.eye_rev)
        {
            self.turning = Some((self.eye_rev, crate::pick::Turning::build(&self.doc.model, &self.style)));
        }
    }

    /// What the view draws over the 3D image from the model: dimensions and texts, guide
    /// points (unless a face hides them), what Ruby's `View` drew, snaps, spaces and lights.
    pub(crate) fn paint_model_overlay(&self, o: &Overlay) {
        for (a, selected, st) in &self.annots {
            let text = crate::annot::label_styled(a, st, self.doc.model.units);
            crate::annot::draw_styled(&o.painter, o.origin, o.cam, a, &text, *selected, st);
        }
        let turning = self.turning.as_ref().filter(|(rev, _)| self.pick_eye_stale && *rev == self.eye_rev);
        for &(p, selected) in &self.guide_pts {
            let (ro, rd, dist) = self.cam.ray_to(p);
            let hidden = self.pick.hides(ro, rd, dist - (dist * 1e-4).max(0.5), turning.map(|(_, t)| t));
            if let (false, Some(s)) = (hidden, o.screen(p)) {
                let c = crate::tools::c32(if selected { self.style.highlight_color } else { self.style.guide_color });
                let k = 4.0;
                let stroke = egui::Stroke::new(1.5, c);
                o.painter.line_segment([s + egui::vec2(-k, -k), s + egui::vec2(k, k)], stroke);
                o.painter.line_segment([s + egui::vec2(-k, k), s + egui::vec2(k, -k)], stroke);
            }
        }
        crate::ruby_host::view::paint(&self.ui_state.extra.view, &o.painter, o.origin, &self.cam);
        crate::tools::snap::paint_marks(o, self.doc.display_model(), crate::tools::c32(self.style.highlight_color));
    }

    fn viewport(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        self.ppp = ppp;
        self.cam.size = DVec2::new(rect.width() as f64, rect.height() as f64);
        self.turn_with_eye();

        // Scene.
        let Some(rs) = frame.wgpu_render_state() else {
            self.refresh_caches();
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Rendu wgpu indisponible",
                egui::FontId::proportional(18.0),
                egui::Color32::RED,
            );
            return;
        };
        self.prepare_scene(rs);
        // An animation export shows and hides tags: the pending picking first.
        if self.ui_state.extra.jobs.anim.is_some() {
            self.ensure_pick();
        }
        self.step_offscreen(ui.ctx(), rs);
        let size = [(rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32];
        let lighting = self.lighting();
        let photo_bg = self.photo_pre(ui, rect);
        // Window › Environments: its panorama is decoded in the background the first time.
        let (env, loading) = crate::environments::active(&self.doc.model, self.snapshot.is_some());
        if loading {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(v) = self.view.as_mut() {
            v.photo_bg = photo_bg;
            v.photo_overlay = crate::photo_fg::overlay(&self.style, self.photo_shown.as_ref(), ppp);
            v.set_environment(rs, env);
            v.set_grid(crate::model_grid::view_of(&self.doc.model, self.style.modeling_grid));
            v.mipmapping = self.doc.model.meta.mipmapping;
            // Preferences › Graphics › Use fast feedback: no ambient occlusion while the
            // camera moves.
            let moving = self.orbit_drag.is_some() || self.transition.is_some();
            let style = crate::dialogs::pref_pages::render_style(&self.style, moving);
            let tex = v.render(rs, &self.cam, size, ppp, &style, &lighting);
            ui.painter().image(
                tex,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }

        // Input.
        let (pointer, mods, scroll, pressed, released, double, triple, mpress, mrelease) = ui.input(|i| {
            (
                i.pointer.hover_pos(),
                i.modifiers,
                i.smooth_scroll_delta.y,
                i.pointer.button_pressed(PointerButton::Primary),
                i.pointer.button_released(PointerButton::Primary),
                i.pointer.button_double_clicked(PointerButton::Primary),
                i.pointer.button_triple_clicked(PointerButton::Primary),
                i.pointer.button_pressed(PointerButton::Middle),
                i.pointer.button_released(PointerButton::Middle),
            )
        });
        // ESQ_FAKE_POINTER="x,y" (view pixels): a pointer resting there when the real one is
        // elsewhere, to look at hover feedback on screen without moving the user's mouse.
        static FAKE: std::sync::OnceLock<Option<DVec2>> = std::sync::OnceLock::new();
        let fake = *FAKE.get_or_init(|| {
            let v = std::env::var("ESQ_FAKE_POINTER").ok()?;
            let (x, y) = v.split_once(',')?;
            Some(DVec2::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
        });
        // (Nudged by a hair every other frame, so that a tool chosen afterwards hears of it.)
        let nudge = DVec2::new((ui.ctx().cumulative_pass_nr() % 2) as f64 * 1e-3, 0.0);
        let fake = fake.filter(|_| pointer.is_none()).map(|f| f + nudge);
        let local = pointer
            .filter(|p| rect.contains(*p) || response.dragged())
            .map(|p| DVec2::new((p.x - rect.min.x) as f64, (p.y - rect.min.y) as f64))
            .or(fake);
        let hovered = response.hovered() || response.dragged() || fake.is_some();
        // A Ruby tool with onMButtonDown/Up takes the middle button (no orbit then).
        let tool_middle = self.tool.wants_middle();
        // What follows looks under the cursor at a click or a turn of the wheel.
        if hovered && (mpress || pressed || scroll.abs() > 0.0) {
            self.ensure_pick();
        }
        // Camera: middle drag orbits (Shift: pan), wheel zooms toward the cursor.
        if mpress
            && hovered
            && !tool_middle
            && let Some(m) = local
        {
            self.remember_view();
            let (o, d) = self.cam.ray(m);
            let pivot = self.pick.ray_face(o, d).map(|(t, _, _)| o + d * t);
            self.orbit_drag = Some((m, pivot));
        }
        if mrelease {
            self.orbit_drag = None;
        }
        let camera_tool = matches!(self.tool.id(), ToolId::Orbit | ToolId::Pan | ToolId::Zoom);
        if camera_tool
            && pressed
            && hovered
            && let Some(m) = local
        {
            self.remember_view();
            let (o, d) = self.cam.ray(m);
            let pivot = self.pick.ray_face(o, d).map(|(t, _, _)| o + d * t);
            self.orbit_drag = Some((m, pivot));
        }
        if camera_tool && released {
            self.orbit_drag = None;
        }
        if let (Some((prev, pivot)), Some(m)) = (self.orbit_drag, local) {
            let d = m - prev;
            let pan = mods.shift || self.tool.id() == ToolId::Pan;
            if self.tool.id() == ToolId::Zoom && mods.shift {
                // Shift: dragging up narrows the field of view, down widens it (1 to 120 degrees).
                let fov = (self.cam.fov_y.to_degrees() * (1.0 + d.y * 0.01)).clamp(1.0, 120.0);
                self.cam.fov_y = fov.to_radians();
            } else if self.tool.id() == ToolId::Zoom && !ui.input(|i| i.pointer.middle_down()) {
                self.cam.zoom_at((1.0 + d.y * 0.01).max(0.1), None);
            } else if pan {
                self.cam.pan(d.x, d.y);
            } else {
                // Ctrl suspends gravity (CCameraOrbitTool).
                let gravity = !(mods.ctrl || mods.command);
                match pivot {
                    Some(p) => self.cam.orbit_around_with(p, d.x, d.y, gravity),
                    None => self.cam.orbit_with(d.x, d.y, gravity),
                }
            }
            self.orbit_drag = Some((m, pivot));
        }
        if hovered
            && scroll.abs() > 0.0
            && let Some(m) = local
            && !(if self.tool.ruby().is_some() {
                crate::ruby_tools::wheel(self, m, scroll as f64, mods, ppp)
            } else {
                self.with_cx(mods, |t, cx| t.scroll(cx, m, scroll as f64))
            })
        {
            let (o, d) = self.cam.ray(m);
            let anchor = self.pick.ray_face(o, d).map(|(t, _, _)| o + d * t).or_else(|| {
                let g = esq_core::math::Plane::from_point_normal(DVec3::ZERO, DVec3::Z);
                g.intersect_ray(o, d).filter(|t| *t > 0.0).map(|t| o + d * t)
            });
            // Preferences › Compatibility › Mouse wheel: Invert.
            let factor = crate::settings::wheel_zoom_factor(scroll as f64);
            let now = ui.input(|i| i.time);
            if now - self.ui_state.last_wheel > 1.0 {
                self.remember_view();
            }
            self.ui_state.last_wheel = now;
            self.cam.zoom_at(factor, anchor);
        }

        // Tool events.
        if !camera_tool && self.orbit_drag.is_none() {
            if let Some(m) = local.filter(|_| hovered) {
                if self.last_mouse != Some(m) {
                    self.with_cx(mods, |t, cx| t.mouse_move(cx, m));
                    self.last_mouse = Some(m);
                    if let Some(r) = self.tool.snap().and_then(crate::pick::recent_from) {
                        self.recent = Some(r);
                    }
                }
                // A click on a comment's pin opens it (`comments`), the tool does not get it.
                if pressed && !crate::comments::click_pin(self, ui.painter().clone(), rect.min, m) {
                    self.with_cx(mods, |t, cx| t.press(cx, m));
                }
                if released {
                    self.with_cx(mods, |t, cx| t.release(cx, m));
                }
                if triple {
                    self.with_cx(mods, |t, cx| t.triple_click(cx, m));
                } else if double {
                    self.with_cx(mods, |t, cx| t.double_click(cx, m));
                }
            } else if released && let Some(m) = self.last_mouse {
                self.with_cx(mods, |t, cx| t.release(cx, m));
            }
            // Right and middle buttons (Ruby tools: onRButtonDown, onMButtonUp…).
            if let Some(m) = local.filter(|_| hovered) {
                use crate::tools::ButtonEvent as B;
                let (rp, rr, rd) = ui.input(|i| {
                    (
                        i.pointer.button_pressed(PointerButton::Secondary),
                        i.pointer.button_released(PointerButton::Secondary),
                        i.pointer.button_double_clicked(PointerButton::Secondary),
                    )
                });
                for (on, b, what) in [
                    (rp, 2, B::Down),
                    (rr, 2, B::Up),
                    (rd, 2, B::DoubleClick),
                    (mpress && tool_middle, 3, B::Down),
                    (mrelease && tool_middle, 3, B::Up),
                ] {
                    if on {
                        self.with_cx(mods, |t, cx| t.button(cx, m, b, what));
                    }
                }
            }
        }
        crate::ruby_tools::frame(self, ui.ctx(), rect.min, local.filter(|_| hovered), mods);

        // Overlay.
        let painter = ui.painter_at(rect);
        self.ensure_overlay_pick();
        let mut o = Overlay { painter, origin: rect.min, cam: &self.cam, doc: &self.doc, units: self.doc.model.units };
        self.paint_model_overlay(&o);
        crate::comments::paint(&o, &self.ui_state.extra.comments, local.filter(|_| hovered));
        self.tool.overlay(&mut o);
        // Preferences › Drawing › Display crosshairs.
        let axes = self.axes();
        crate::dialogs::pref_pages::crosshairs(&o, self.tool.snap(), self.tool.id(), &axes);
        if let Some(s) = self.tool.snap() {
            o.marker(s);
        }
        if let Some(l) = self.current_lock() {
            let txt = match l.kind {
                SnapKind::Axis(0) => "Verrouillé sur l'axe rouge",
                SnapKind::Axis(1) => "Verrouillé sur l'axe vert",
                SnapKind::Axis(_) => "Verrouillé sur l'axe bleu",
                _ => "Direction verrouillée",
            };
            o.painter.text(
                rect.left_top() + egui::vec2(10.0, 10.0),
                egui::Align2::LEFT_TOP,
                txt,
                egui::FontId::proportional(13.0),
                crate::tools::c32(l.color),
            );
        }
        if !self.doc.ctx.is_empty() {
            let names: Vec<String> = {
                let mut ents = &self.doc.model.root;
                let mut out = Vec::new();
                for &i in &self.doc.ctx {
                    if let Some(inst) = ents.instances.get(i) {
                        out.push(self.doc.model.defs[inst.def].name.clone());
                        ents = &self.doc.model.defs[inst.def].entities;
                    }
                }
                out
            };
            o.painter.text(
                rect.right_top() + egui::vec2(-10.0, 10.0),
                egui::Align2::RIGHT_TOP,
                format!("Édition : {}  (Échap pour sortir)", names.join(" › ")),
                egui::FontId::proportional(13.0),
                egui::Color32::from_rgb(60, 60, 120),
            );
        }

        let ctx = ui.ctx().clone();
        self.with_cx(mods, |t, cx| t.panel(&ctx, rect, cx));
        self.photo_tool_done();

        // Right click, Shift+F10, Menu key (`context_menu.rs`).
        crate::context_menu::viewport(self, ui, &response, local);

        if self.doc.preview.is_some() || self.orbit_drag.is_some() {
            ui.ctx().request_repaint();
        }
    }

    pub(crate) fn context_menu(&mut self, ui: &mut egui::Ui) {
        // The active Ruby tool's getMenu replaces the usual entries, as in SketchUp.
        if crate::ruby_tools::tool_menu_shown(self) {
            crate::ruby_ui::context_items(self, ui);
            return;
        }
        // Position Texture has its own menu (Done, Reset, Flip, Rotate, Fixed Pins…).
        if self.with_cx(Modifiers::NONE, |t, cx| t.menu(ui, cx)) {
            return;
        }
        crate::context_menu::show(self, ui);
    }

    fn take_snapshot(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(path) = self.snapshot.clone() else { return };
        self.frames += 1;
        if self.frames < 4 {
            ctx.request_repaint();
            return;
        }
        if let (Some(rs), Some(v)) = (frame.wgpu_render_state(), self.view.as_ref())
            && let Some((w, h, px)) = v.read_pixels(rs)
        {
            let px = self.photo_composite(w, h, px);
            let _ = crate::png::write_rgba(&path, w, h, &px);
        }
        std::process::exit(0);
    }
}

impl eframe::App for EsquisseApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if std::mem::take(&mut self.quit_requested) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.doc.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        } else if ctx.input(|i| i.viewport().close_requested()) {
            // Ruby's AppObserver#onQuit.
            crate::ruby_tools::quit(self);
        }
        crate::dialogs::frame_start(self, &ctx);
        self.run_pending(&ctx);
        self.handle_keys(&ctx);
        if self.tool.done() {
            self.set_tool(ToolId::Select);
        }
        if self.animate(ctx.input(|i| i.time)) {
            ctx.request_repaint();
        }
        crate::menus::menu_bar(self, ui);
        crate::ui::toolbar(self, ui);
        crate::ui::status_bar(self, ui);
        crate::ui::tray(self, ui);
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            self.scene_tabs(ui);
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.viewport(ui, frame));
        });
        crate::ui::dialogs(self, &ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.doc.title()));
        if self.confirm_close && self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.take_snapshot(&ctx, frame);
    }
}

impl EsquisseApp {
    pub fn force_close(&mut self, ctx: &egui::Context) {
        self.allow_close = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// `Sketchup.quit` from Ruby: closes the window at the next frame (asking to save first
    /// when the model has changes, like File › Exit).
    pub fn request_quit(&mut self) {
        self.quit_requested = true;
    }

    /// `Model#export` of an image from Ruby, at once: `model` (the script's, lent to the
    /// document meanwhile, edit path `ctx`, selection `sel`) seen by the current camera.
    pub fn export_script_image(
        &mut self,
        model: &mut Model,
        ctx: &[esq_core::model::InstanceId],
        sel: &[EntityRef],
        path: &std::path::Path,
        kind: crate::export2d::Kind,
        o: &crate::export2d::RasterOptions,
    ) -> anyhow::Result<()> {
        self.with_script_scene(model, ctx, sel, |app, rs| {
            let ppp = app.ppp as f64;
            let view = [(app.cam.size.x * ppp).round() as u32, (app.cam.size.y * ppp).round() as u32];
            app.export_raster(rs, path.to_path_buf(), kind, o, view, ppp).map(|_| ())
        })
    }

    /// Runs `f` with a script's `model` (lent to the document meanwhile, edit path `ctx`,
    /// selection `sel`) uploaded to the 3D view, for a picture drawn at once.
    pub fn with_script_scene<R>(
        &mut self,
        model: &mut Model,
        ctx: &[esq_core::model::InstanceId],
        sel: &[EntityRef],
        f: impl FnOnce(&mut Self, &eframe::egui_wgpu::RenderState) -> anyhow::Result<R>,
    ) -> anyhow::Result<R> {
        let Some(rs) = self.gpu.clone() else { anyhow::bail!("pas de rendu graphique") };
        std::mem::swap(&mut self.doc.model, model);
        let r = (|| {
            let m = &self.doc.model;
            let axes = if ctx.is_empty() { m.axes } else { esq_core::walk::path_xform(m, ctx) };
            let opts = SceneOptions {
                ctx,
                ctx_def: esq_core::walk::path_def(m, ctx),
                sel,
                axes_xform: axes,
                style: &self.style,
            };
            let mesh = scene::build(m, &opts);
            let Some(v) = self.view.as_mut() else { anyhow::bail!("pas de vue 3D") };
            v.upload(&rs, &mesh);
            f(self, &rs)
        })();
        std::mem::swap(&mut self.doc.model, model);
        // The view shows the document again from the next frame.
        self.built_rev = 0;
        r
    }

    /// The preview a `.skp` keeps for the Explorer and the open dialogs
    /// (`meta/model_thumbnail.png`, 256 × 256 as SketchUp 2026 writes it): the model seen by the
    /// current camera, rendered offscreen. `model`: a script's model (lent to the document
    /// meanwhile), else the document as the view last showed it. `None` without a GPU.
    pub fn skp_thumbnail(&mut self, model: Option<&mut Model>) -> Option<Vec<u8>> {
        const SIZE: u32 = 256;
        let rs = self.gpu.clone()?;
        let mut cam = self.cam.clone();
        cam.size = esq_core::glam::DVec2::splat(SIZE as f64);
        let px = match model {
            Some(model) => {
                std::mem::swap(&mut self.doc.model, model);
                let opts = SceneOptions {
                    ctx: &[],
                    ctx_def: None,
                    sel: &[],
                    axes_xform: self.doc.model.axes,
                    style: &self.style,
                };
                let mesh = scene::build(&self.doc.model, &opts);
                if let Some(v) = self.view.as_mut() {
                    v.upload(&rs, &mesh);
                }
                let px = self.render_rgba(&rs, &cam, [SIZE, SIZE], 1.0);
                std::mem::swap(&mut self.doc.model, model);
                // The view shows the document again from the next frame.
                self.built_rev = 0;
                px
            }
            None => self.render_rgba(&rs, &cam, [SIZE, SIZE], 1.0),
        };
        let (w, h, px) = px?;
        let img = image::RgbaImage::from_raw(w, h, px)?;
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .ok()?;
        Some(out)
    }

    /// The commands scripts sent (`Sketchup.send_action`), as SketchUp runs them: after the
    /// script, from the application's loop.
    pub fn run_pending(&mut self, ctx: &egui::Context) {
        for c in std::mem::take(&mut self.pending_cmds) {
            self.run(ctx, c);
        }
    }
}

/// Two cameras showing the same view (for the camera history).
fn same_view(a: &Camera, b: &Camera) -> bool {
    a.target.distance(b.target) < 1e-6
        && (a.distance - b.distance).abs() < 1e-6
        && (a.yaw - b.yaw).abs() < 1e-9
        && (a.pitch - b.pitch).abs() < 1e-9
        && a.perspective == b.perspective
        && (a.fov_y - b.fov_y).abs() < 1e-9
}
