//! Comments, as in SketchUp 2026 (Tools › Comment, View › Comments, the Comments panel; read
//! in SketchUp.exe), kept in the model instead of Trimble Connect (no
//! online service):
//!
//! - the tool ([`tool::CommentTool`]) places the pin of a new comment; the panel asks for its
//!   text and keeps the view it was placed from;
//! - the panel lists the threads (unread ones marked, the resolved ones folded under
//!   « Résolu »), opens a thread to read and answer it, filters read and unread threads;
//! - the pins (« points actifs ») are drawn over the view; clicking one opens its thread and
//!   flies to its view; View › Comments hides or shows them;
//! - every change is an undoable step of the model, saved with it (.esq, .skp).

pub mod tool;

pub use tool::CommentTool;

use std::cell::RefCell;
use std::collections::BTreeMap;

use eframe::egui::{self, Color32, Pos2, RichText, Stroke};
use esq_core::comments::{Comment, Comments, Millis, format_date, now_ms};
use esq_core::glam::{DVec2, DVec3};
use esq_core::model::{InstanceId, SceneCamera};

use crate::app::EsquisseApp;
use crate::doc::Document;
use crate::tools::{Overlay, ToolId};

// SketchUp's French strings (Localizable.strings); the ones it has no French for are
// translated the same way as their neighbours.
const PANEL: &str = "Commentaires";
const ADD: &str = "Ajouter un commentaire";
const HOTSPOTS: &str = "Masquer/Afficher les points actifs";
const FILTER: &str = "Filtre";
const EMPTY_TITLE: &str = "Pour commencer un nouveau commentaire";
const EMPTY_TEXT: &str = "Cliquez sur l'icône + et sélectionnez un objet dans votre modèle. Partagez ce modèle avec \
                          d'autres personnes pour leur permettre de le visualiser et d'ajouter des commentaires.";
const CLICK_AN_OBJECT: &str = "Cliquez sur un objet pour insérer un commentaire.";
const PLACEHOLDER: &str = "Utilisez Ctrl + Entrée pour envoyer votre commentaire.";
const SEND: &str = "Envoyer";
const CLEAR: &str = "Effacer";
const ATTACH: &str = "Ajouter";
const ADD_VIEW: &str = "Ajouter une vue de caméra";
const UPDATE_VIEW: &str = "Mettre à jour la vue de la caméra";
const DELETE_VIEW: &str = "Supprimer la vue de la caméra";
const VIEW_CAPTURED: &str = "Vue de la caméra capturée";
const VIEW_DELETED: &str = "Vue de la caméra supprimée";
const OPEN_COMMENT: &str = "Ouvrir un commentaire";
const FLY: &str = "Voler vers la destination";
const OPTIONS: &str = "Options";
const MARK_READ: &str = "Marquer comme lu";
const MARK_UNREAD: &str = "Marquer comme non lu";
const MARK_RESOLVED: &str = "Marquer comme résolu";
const MARK_UNRESOLVED: &str = "Marquer comme non résolu";
const DELETE_THREAD: &str = "Supprimer la conversation";
const DELETE_REPLY: &str = "Supprimer le commentaire";
const RESOLVED: &str = "Résolu";
const BACK: &str = "< Retour";
const UNREAD: &str = "Non lu";
const READ: &str = "Lu";
const ON_COMMENT: &str = "Sur le commentaire";
const BAR_TOGGLE: &str = "Activer/Désactiver le panneau des commentaires";
const BAR_HIDE: &str = "Masquer la barre de collaboration";

/// Colours of the pins, as SketchUp's `comment_128` / `comment_hover_128` /
/// `comment_inactive_128`.
const PIN_BLUE: Color32 = Color32::from_rgb(0, 154, 217);
const PIN_HOVER_RING: Color32 = Color32::from_rgb(220, 237, 249);
const PIN_INACTIVE: Color32 = Color32::from_rgba_premultiplied(72, 74, 80, 128);
/// Size of a pin on screen (points).
pub const PIN: f32 = 26.0;

/// A comment being written: placed with the tool, its text typed in the panel.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub anchor: DVec3,
    pub target: String,
    pub text: String,
    pub camera: Option<SceneCamera>,
}

/// Which threads the user has read (SketchUp keeps it per user too): thread id → the latest
/// activity seen, -1 for "Mark Unread". In `%APPDATA%\Esquisse\commentaires-lus.json`.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct ReadState(BTreeMap<String, Millis>);

impl ReadState {
    fn file() -> Option<std::path::PathBuf> {
        Some(crate::settings::user_dir()?.join("commentaires-lus.json"))
    }

    fn load() -> ReadState {
        if cfg!(test) {
            return ReadState::default();
        }
        Self::file()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        if cfg!(test) {
            return;
        }
        if let (Some(f), Ok(b)) = (Self::file(), serde_json::to_vec(self)) {
            let _ = std::fs::write(f, b);
        }
    }

    fn unread(&self, c: &Comment, me: &str) -> bool {
        match self.0.get(&c.id) {
            Some(-1) => true,
            seen => c.activity_of_others(me) > seen.copied().unwrap_or(0),
        }
    }

    /// Answers of others since the thread was last read (none counted before a first read).
    fn new_replies(&self, c: &Comment, me: &str) -> usize {
        match self.0.get(&c.id) {
            Some(&seen) if seen > 0 => c.replies.iter().filter(|r| r.author != me && r.created > seen).count(),
            _ => 0,
        }
    }

    /// `activity`: the thread's latest activity of others, so that it reads as read whatever the
    /// clocks of those who wrote in it.
    fn mark(&mut self, id: &str, read: bool, activity: Millis) {
        self.0.insert(id.to_string(), if read { now_ms().max(activity) } else { -1 });
        self.save();
    }
}

/// Marks a thread read (all of it so far) or unread.
fn mark(app: &mut EsquisseApp, id: &str, read: bool) {
    let st = &mut app.ui_state.extra.comments;
    let me = st.me();
    let activity = app.doc.model.comments.get(id).map_or(0, |c| c.activity_of_others(&me));
    st.read_state().mark(id, read, activity);
}

pub struct CommentsState {
    /// The Comments panel is shown.
    pub open: bool,
    /// View › Comments, the panel's « Masquer/Afficher les points actifs »: pins shown.
    pub pins: bool,
    /// The thread open in the panel.
    pub thread: Option<String>,
    pub draft: Option<Draft>,
    /// The answer being written in the open thread, and its view.
    pub reply: String,
    pub reply_camera: Option<SceneCamera>,
    /// The panel's Filter menu.
    pub show_unread: bool,
    pub show_read: bool,
    /// The « Résolu » group unfolded.
    pub resolved_open: bool,
    /// The collaboration bar closed for this session.
    pub bar_hidden: bool,
    /// A message shown over the view for ten seconds (as SketchUp's notifications).
    toast: Option<(String, f64)>,
    read: Option<ReadState>,
    me: Option<String>,
    tool_was_on: bool,
}

impl Default for CommentsState {
    fn default() -> Self {
        CommentsState {
            open: false,
            pins: true,
            thread: None,
            draft: None,
            reply: String::new(),
            reply_camera: None,
            show_unread: true,
            show_read: true,
            resolved_open: false,
            bar_hidden: false,
            toast: None,
            read: None,
            me: None,
            tool_was_on: false,
        }
    }
}

impl CommentsState {
    fn read_state(&mut self) -> &mut ReadState {
        self.read.get_or_insert_with(ReadState::load)
    }

    /// The user's name, as their comments are signed.
    pub fn me(&mut self) -> String {
        self.me.get_or_insert_with(crate::settings::user_name).clone()
    }

    /// The comment threads the Filter lets through.
    fn shown(&mut self, c: &Comment) -> bool {
        let me = self.me();
        let unread = self.read_state().unread(c, &me);
        if unread { self.show_unread } else { self.show_read }
    }
}

// ---------------------------------------------------------------------------------------
// From the tool to the panel.

enum ToolEvent {
    Placed(tool::Placed),
    Cancelled,
}

thread_local! {
    static TOOL_EVENTS: RefCell<Vec<ToolEvent>> = const { RefCell::new(Vec::new()) };
}

/// The tool placed the pin of a new comment.
pub fn place(p: tool::Placed) {
    TOOL_EVENTS.with(|e| e.borrow_mut().push(ToolEvent::Placed(p)));
}

/// Escape, or another tool: the comment being placed is dropped.
pub fn drop_placed() {
    TOOL_EVENTS.with(|e| e.borrow_mut().push(ToolEvent::Cancelled));
}

fn take_tool_events(app: &mut EsquisseApp, now: f64) {
    let events = TOOL_EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()));
    for ev in events {
        match ev {
            ToolEvent::Placed(p) => {
                let st = &mut app.ui_state.extra.comments;
                let text = st.draft.take().map(|d| d.text).unwrap_or_default();
                // The view is captured with the pin, as SketchUp does.
                st.draft =
                    Some(Draft { anchor: p.anchor, target: p.target, text, camera: Some(scene_camera(&app.cam)) });
                st.thread = None;
                st.open = true;
                st.toast = Some((VIEW_CAPTURED.into(), now + 10.0));
            }
            ToolEvent::Cancelled => app.ui_state.extra.comments.draft = None,
        }
    }
}

// ---------------------------------------------------------------------------------------
// Pins.

/// SketchUp's comment bubble, its sharp corner (bottom left) on `corner`.
pub fn paint_bubble(p: &egui::Painter, corner: Pos2, size: f32, fill: Color32, stroke: Stroke) {
    p.add(egui::Shape::convex_polygon(bubble_points(corner, size), fill, stroke));
}

fn bubble_points(corner: Pos2, size: f32) -> Vec<Pos2> {
    // In SketchUp's 128-unit drawing: 112 wide, 96 high, corners of 48, 32, 48 and a small
    // one at the point.
    let w = size;
    let h = size * 96.0 / 112.0;
    let (r_tl, r_tr, r_br, r_bl) = (w * 48.0 / 112.0, w * 32.0 / 112.0, w * 48.0 / 112.0, w * 4.0 / 112.0);
    let at = |x: f32, y: f32| egui::pos2(corner.x + x, corner.y - h + y);
    let mut pts = Vec::new();
    let mut arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| {
        for i in 0..=6 {
            let a = a0 + (a1 - a0) * i as f32 / 6.0;
            pts.push(at(cx + r * a.cos(), cy + r * a.sin()));
        }
    };
    use std::f32::consts::{FRAC_PI_2, PI};
    arc(r_tl, r_tl, r_tl, PI, PI + FRAC_PI_2);
    arc(w - r_tr, r_tr, r_tr, -FRAC_PI_2, 0.0);
    arc(w - r_br, h - r_br, r_br, 0.0, FRAC_PI_2);
    arc(r_bl, h - r_bl, r_bl, FRAC_PI_2, PI);
    pts
}

/// The screen box of a pin standing on `corner`.
fn pin_rect(corner: Pos2) -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(corner.x, corner.y - PIN * 96.0 / 112.0), egui::pos2(corner.x + PIN, corner.y))
}

/// The pins on screen, in drawing order: (thread id, corner, resolved).
fn pins(o: &Overlay, st: &CommentsState) -> Vec<(String, Pos2, bool)> {
    if !st.pins {
        return Vec::new();
    }
    let mut v: Vec<(String, Pos2, bool)> = o
        .doc
        .model
        .comments
        .items
        .iter()
        .filter_map(|c| Some((c.id.clone(), o.screen(c.anchor)?, c.resolved)))
        .collect();
    // The resolved ones under the others.
    v.sort_by_key(|p| !p.2);
    v
}

/// The thread whose pin is under `m` (view pixels), the topmost one.
pub fn pin_under(o: &Overlay, st: &CommentsState, m: DVec2) -> Option<String> {
    let p = o.origin + egui::vec2(m.x as f32, m.y as f32);
    pins(o, st).into_iter().rev().find(|(_, c, _)| pin_rect(*c).contains(p)).map(|(id, ..)| id)
}

/// Draws the pins over the view (and the one being placed), the thread open in the panel and
/// the one under the cursor ringed.
pub fn paint(o: &Overlay, st: &CommentsState, pointer: Option<DVec2>) {
    let hover = pointer.and_then(|m| pin_under(o, st, m));
    for (id, corner, resolved) in pins(o, st) {
        let lit = hover.as_deref() == Some(id.as_str()) || st.thread.as_deref() == Some(id.as_str());
        let (fill, stroke) = match (lit, resolved) {
            (true, _) => (PIN_BLUE, Stroke::new(3.0, PIN_HOVER_RING)),
            (false, true) => (PIN_INACTIVE, Stroke::new(2.0, Color32::WHITE)),
            (false, false) => (PIN_BLUE, Stroke::new(2.0, Color32::WHITE)),
        };
        paint_bubble(&o.painter, corner, PIN, fill, stroke);
    }
    if let Some(d) = &st.draft
        && let Some(corner) = o.screen(d.anchor)
    {
        paint_bubble(&o.painter, corner, PIN, PIN_BLUE, Stroke::new(3.0, PIN_HOVER_RING));
    }
    if let (Some(_), Some(m)) = (hover, pointer) {
        o.tooltip(o.origin + egui::vec2(m.x as f32 + 16.0, m.y as f32 + 18.0), ON_COMMENT);
    }
}

/// A click on the view: on a pin, opens its thread and flies to its view (the tool does not
/// get the click). Returns whether it was on a pin.
pub fn click_pin(app: &mut EsquisseApp, painter: egui::Painter, origin: Pos2, m: DVec2) -> bool {
    let id = {
        let o = Overlay { painter, origin, cam: &app.cam, doc: &app.doc, units: app.doc.model.units };
        pin_under(&o, &app.ui_state.extra.comments, m)
    };
    let Some(id) = id else { return false };
    open_thread(app, &id, true);
    true
}

/// The corners of an instance of the active context's box, in world coordinates.
pub fn instance_box(doc: &Document, i: InstanceId) -> Option<[DVec3; 8]> {
    let inst = doc.ents().instances.get(i)?;
    let (lo, hi) = crate::tools::move_grips::def_bounds(&doc.model, inst.def)?;
    let x = doc.ctx_xform() * inst.xform;
    Some(std::array::from_fn(|c| {
        x.transform_point3(DVec3::new(
            if c & 1 == 0 { lo.x } else { hi.x },
            if c & 2 == 0 { lo.y } else { hi.y },
            if c & 4 == 0 { lo.z } else { hi.z },
        ))
    }))
}

/// The twelve edges of a box given as [`instance_box`] gives its corners.
pub const BOX_EDGES: [(usize, usize); 12] =
    [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];

// ---------------------------------------------------------------------------------------
// Commands.

/// View › Comments: hides or shows the pins.
pub fn toggle_pins(app: &mut EsquisseApp) {
    let st = &mut app.ui_state.extra.comments;
    st.pins = !st.pins;
    app.doc.rev += 1;
}

pub fn pins_shown(app: &EsquisseApp) -> bool {
    app.ui_state.extra.comments.pins
}

/// The panel's « + »: the Comment tool, and SketchUp's ten-second reminder.
pub fn start_tool(app: &mut EsquisseApp, now: f64) {
    app.set_tool(ToolId::Comment);
    let st = &mut app.ui_state.extra.comments;
    st.open = true;
    st.toast = Some((CLICK_AN_OBJECT.into(), now + 10.0));
}

pub fn scene_camera(cam: &crate::camera::Camera) -> SceneCamera {
    let v = cam.to_view();
    SceneCamera { eye: v.eye, target: v.target, up: v.up, fov_deg: v.fov_deg, perspective: v.perspective }
}

/// « Voler vers la destination »: the view saved with a comment (Camera › Previous goes back).
pub fn fly_to(app: &mut EsquisseApp, c: &SceneCamera) {
    app.remember_view();
    app.cam.set_from_view(&esq_skp::View {
        eye: c.eye,
        target: c.target,
        up: c.up,
        fov_deg: c.fov_deg,
        perspective: c.perspective,
    });
}

/// Opens a thread in the panel (marking it read); `fly` goes to its view as well.
pub fn open_thread(app: &mut EsquisseApp, id: &str, fly: bool) {
    let st = &mut app.ui_state.extra.comments;
    st.open = true;
    if st.thread.as_deref() != Some(id) {
        st.reply.clear();
        st.reply_camera = None;
    }
    st.thread = Some(id.to_string());
    mark(app, id, true);
    let cam = app.doc.model.comments.get(id).and_then(|c| c.camera);
    if fly && let Some(c) = cam {
        fly_to(app, &c);
    }
    app.doc.rev += 1;
}

// ---------------------------------------------------------------------------------------
// Changes: undoable steps of the model.

fn commit_comments(app: &mut EsquisseApp, label: &str, f: impl FnOnce(&mut Comments, Millis)) {
    let now = now_ms();
    app.doc.commit(label, |m, _| f(&mut m.comments, now));
}

/// Sends the comment being written: a new thread.
pub fn submit_draft(app: &mut EsquisseApp) -> Option<String> {
    let me = app.ui_state.extra.comments.me();
    let d = app.ui_state.extra.comments.draft.take()?;
    if d.text.trim().is_empty() {
        app.ui_state.extra.comments.draft = Some(d);
        return None;
    }
    let mut c = Comment::new(d.anchor, &d.target, &me, d.text.trim(), now_ms());
    c.camera = d.camera;
    let id = c.id.clone();
    commit_comments(app, "Créer un commentaire", |cs, now| {
        c.created = now;
        cs.add(c);
    });
    mark(app, &id, true);
    Some(id)
}

/// Sends the answer being written in the open thread.
pub fn submit_reply(app: &mut EsquisseApp) {
    let st = &mut app.ui_state.extra.comments;
    let (Some(id), text) = (st.thread.clone(), st.reply.trim().to_string()) else { return };
    if text.is_empty() {
        return;
    }
    let me = st.me();
    let camera = st.reply_camera.take();
    st.reply.clear();
    commit_comments(app, "Répondre au commentaire", |cs, now| {
        if let Some(c) = cs.get_mut(&id) {
            c.reply(&me, &text, camera, now);
        }
    });
    mark(app, &id, true);
}

pub fn set_resolved(app: &mut EsquisseApp, id: &str, resolved: bool) {
    let label = if resolved { MARK_RESOLVED } else { MARK_UNRESOLVED };
    commit_comments(app, label, |cs, _| {
        if let Some(c) = cs.get_mut(id) {
            c.resolved = resolved;
        }
    });
}

pub fn delete_thread(app: &mut EsquisseApp, id: &str) {
    commit_comments(app, DELETE_THREAD, |cs, _| cs.remove(id));
    let st = &mut app.ui_state.extra.comments;
    if st.thread.as_deref() == Some(id) {
        st.thread = None;
    }
}

pub fn delete_reply(app: &mut EsquisseApp, id: &str, reply: &str) {
    commit_comments(app, DELETE_REPLY, |cs, _| {
        if let Some(c) = cs.get_mut(id) {
            c.delete_reply(reply);
        }
    });
}

/// A model was opened: the panel forgets the other model's thread and draft.
pub fn after_open(app: &mut EsquisseApp) {
    let st = &mut app.ui_state.extra.comments;
    st.thread = None;
    st.draft = None;
    st.reply.clear();
    st.reply_camera = None;
}

// ---------------------------------------------------------------------------------------
// The panel.

enum Action {
    StartTool,
    TogglePins,
    Open(String),
    Back,
    Fly(SceneCamera),
    SubmitDraft,
    CancelDraft,
    DraftView,
    SubmitReply,
    ReplyView(bool),
    Resolve(String, bool),
    DeleteThread(String),
    DeleteReply(String, String),
    Mark(String, bool),
}

fn icon_button(
    ui: &mut egui::Ui,
    label: &str,
    enabled: bool,
    draw: impl FnOnce(&egui::Painter, egui::Rect, Color32),
) -> bool {
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), sense);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if resp.hovered() && enabled {
        ui.painter().rect_filled(rect, 3.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let color = if enabled { Color32::from_rgb(40, 44, 52) } else { Color32::from_gray(160) };
    draw(ui.painter(), rect.shrink(4.0), color);
    resp.on_hover_text(label).clicked()
}

fn icon_add(p: &egui::Painter, r: egui::Rect, c: Color32) {
    paint_bubble(p, r.left_bottom(), r.width(), Color32::TRANSPARENT, Stroke::new(1.4, c));
    let m = r.center() + egui::vec2(0.5, -0.5);
    p.line_segment([m - egui::vec2(3.5, 0.0), m + egui::vec2(3.5, 0.0)], Stroke::new(1.4, c));
    p.line_segment([m - egui::vec2(0.0, 3.5), m + egui::vec2(0.0, 3.5)], Stroke::new(1.4, c));
}

fn icon_hotspots(shown: bool) -> impl FnOnce(&egui::Painter, egui::Rect, Color32) {
    move |p, r, c| {
        paint_bubble(
            p,
            r.left_bottom(),
            r.width(),
            if shown { PIN_BLUE } else { Color32::TRANSPARENT },
            Stroke::new(1.4, c),
        );
        if !shown {
            p.line_segment([r.left_bottom(), r.right_top()], Stroke::new(1.4, c));
        }
    }
}

fn icon_filter(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let s = Stroke::new(1.4, c);
    let funnel = vec![
        r.left_top(),
        r.right_top(),
        egui::pos2(r.center().x + 2.0, r.center().y + 1.0),
        egui::pos2(r.center().x + 2.0, r.bottom()),
        egui::pos2(r.center().x - 2.0, r.bottom() - 2.0),
        egui::pos2(r.center().x - 2.0, r.center().y + 1.0),
    ];
    p.add(egui::Shape::closed_line(funnel, s));
}

fn icon_location(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let s = Stroke::new(1.4, c);
    let ctr = r.center() - egui::vec2(0.0, 1.5);
    p.circle_stroke(ctr, r.width() * 0.3, s);
    p.line_segment([ctr + egui::vec2(-3.0, 3.0), egui::pos2(r.center().x, r.bottom())], s);
    p.line_segment([ctr + egui::vec2(3.0, 3.0), egui::pos2(r.center().x, r.bottom())], s);
}

fn icon_options(p: &egui::Painter, r: egui::Rect, c: Color32) {
    for dx in [-5.0, 0.0, 5.0] {
        p.circle_filled(r.center() + egui::vec2(dx, 0.0), 1.6, c);
    }
}

fn icon_check(p: &egui::Painter, r: egui::Rect, _c: Color32) {
    let g = Color32::from_rgb(40, 160, 70);
    p.line_segment(
        [r.left_center() + egui::vec2(1.0, 0.0), r.center_bottom() - egui::vec2(1.0, 2.0)],
        Stroke::new(2.0, g),
    );
    p.line_segment(
        [r.center_bottom() - egui::vec2(1.0, 2.0), r.right_top() + egui::vec2(-1.0, 2.0)],
        Stroke::new(2.0, g),
    );
}

/// What the comment editor was asked.
#[derive(Default)]
struct EditorOut {
    send: bool,
    /// The « + » menu's camera view entry.
    view: bool,
}

/// The comment editor (SketchUp's AddCommentWidget): the text (Ctrl + Enter sends it), a
/// button that clears it, « + » with the camera view entry `view_label`, and « Envoyer »,
/// greyed while there is no text.
fn editor(ui: &mut egui::Ui, id: &str, text: &mut String, view_label: &str) -> EditorOut {
    let edit_id = ui.make_persistent_id(("comment-editor", id));
    let focused = ui.memory(|m| m.has_focus(edit_id));
    let ctrl_enter = focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
    let mut out = EditorOut { send: ctrl_enter, view: false };
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::multiline(text)
                .id(edit_id)
                .hint_text(PLACEHOLDER)
                .desired_rows(2)
                .desired_width(ui.available_width() - 28.0),
        );
        // Screen readers (and UI Automation) name the field by its placeholder.
        ui.ctx().accesskit_node_builder(r.id, |n| n.set_label(PLACEHOLDER));
        if icon_button(ui, CLEAR, !text.is_empty(), |p, r, c| {
            p.line_segment([r.left_top(), r.right_bottom()], Stroke::new(1.4, c));
            p.line_segment([r.right_top(), r.left_bottom()], Stroke::new(1.4, c));
        }) {
            text.clear();
        }
    });
    ui.horizontal(|ui| {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, ATTACH));
        let c = Color32::from_rgb(40, 44, 52);
        let m = rect.center();
        ui.painter().circle_stroke(m, 8.0, Stroke::new(1.4, c));
        ui.painter().line_segment([m - egui::vec2(4.0, 0.0), m + egui::vec2(4.0, 0.0)], Stroke::new(1.4, c));
        ui.painter().line_segment([m - egui::vec2(0.0, 4.0), m + egui::vec2(0.0, 4.0)], Stroke::new(1.4, c));
        let resp = resp.on_hover_text(ATTACH);
        egui::Popup::menu(&resp).show(|ui| {
            if ui.button(view_label).clicked() {
                out.view = true;
                ui.close();
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            out.send |= ui.add_enabled(!text.trim().is_empty(), egui::Button::new(SEND)).clicked();
        });
    });
    out.send &= !text.trim().is_empty();
    out
}

fn author_line(ui: &mut egui::Ui, author: &str, when: Millis, unread: bool) {
    ui.horizontal(|ui| {
        if unread {
            ui.label(RichText::new("●").color(PIN_BLUE).small());
        }
        ui.label(RichText::new(author).strong().size(12.0));
        ui.label(RichText::new(format_date(when, now_ms())).weak().size(10.0));
    });
}

/// One thread in the list (SketchUp's TopicWidget).
fn card(ui: &mut egui::Ui, st: &mut CommentsState, c: &Comment, me: &str, actions: &mut Vec<Action>) {
    let unread = st.read_state().unread(c, me);
    let new_replies = st.read_state().new_replies(c, me);
    let frame = egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(6));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            author_line(ui, &c.author, c.created, unread);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, OPTIONS));
                icon_options(ui.painter(), r.shrink(4.0), Color32::from_rgb(40, 44, 52));
                let resp = resp.on_hover_text(OPTIONS);
                egui::Popup::menu(&resp).show(|ui| {
                    let read_label = if unread || new_replies > 0 { MARK_READ } else { MARK_UNREAD };
                    if ui.button(read_label).clicked() {
                        actions.push(Action::Mark(c.id.clone(), read_label == MARK_READ));
                        ui.close();
                    }
                    if c.author == me {
                        let l = if c.resolved { MARK_UNRESOLVED } else { MARK_RESOLVED };
                        if ui.button(l).clicked() {
                            actions.push(Action::Resolve(c.id.clone(), !c.resolved));
                            ui.close();
                        }
                        if ui.button(DELETE_THREAD).clicked() {
                            actions.push(Action::DeleteThread(c.id.clone()));
                            ui.close();
                        }
                    }
                });
                if let Some(cam) = c.camera
                    && icon_button(ui, FLY, true, icon_location)
                {
                    actions.push(Action::Fly(cam));
                }
                if c.resolved {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                    icon_check(ui.painter(), r, PIN_BLUE);
                }
            });
        });
        if !c.target.is_empty() {
            ui.label(RichText::new(&c.target).weak().size(10.0));
        }
        // The text opens the thread (SketchUp: the whole card, « Ouvrir un commentaire »).
        let text = ui.add(egui::Label::new(RichText::new(&c.text).size(12.0)).truncate().sense(egui::Sense::click()));
        if text.on_hover_text(OPEN_COMMENT).clicked() {
            actions.push(Action::Open(c.id.clone()));
        }
        let n = c.reply_count();
        let replies = match (new_replies, n) {
            (1, _) => "1 nouvelle réponse".to_string(),
            (k, _) if k > 1 => format!("{k} nouvelles réponses"),
            (_, 0) => String::new(),
            (_, 1) => "1 réponse".to_string(),
            (_, n) => format!("{n} réponses"),
        };
        if !replies.is_empty() && ui.link(RichText::new(replies).size(10.0).strong().color(PIN_BLUE)).clicked() {
            actions.push(Action::Open(c.id.clone()));
        }
    });
}

fn list_view(ui: &mut egui::Ui, st: &mut CommentsState, comments: &Comments, me: &str, actions: &mut Vec<Action>) {
    let all: Vec<&Comment> = comments.items.iter().collect();
    if all.is_empty() && st.draft.is_none() {
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(EMPTY_TITLE).strong().size(16.0));
            ui.add_space(4.0);
            ui.label(RichText::new(EMPTY_TEXT).size(13.0));
            ui.add_space(8.0);
            let (r, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
            icon_add(ui.painter(), r.shrink(4.0), PIN_BLUE);
        });
        return;
    }
    let open: Vec<&Comment> = all.iter().copied().filter(|c| !c.resolved && st.shown(c)).collect();
    let resolved: Vec<&Comment> = all.iter().copied().filter(|c| c.resolved && st.shown(c)).collect();
    egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
        // Newest first, as SketchUp lists them.
        for c in open.iter().rev() {
            card(ui, st, c, me, actions);
        }
        if !resolved.is_empty() {
            let label = format!("{RESOLVED} ({}) {}", resolved.len(), if st.resolved_open { "▴" } else { "▾" });
            if ui.add(egui::Button::new(RichText::new(label).strong()).frame(false)).clicked() {
                st.resolved_open = !st.resolved_open;
            }
            if st.resolved_open {
                for c in resolved.iter().rev() {
                    card(ui, st, c, me, actions);
                }
            }
        }
    });
}

fn thread_view(ui: &mut egui::Ui, st: &mut CommentsState, c: &Comment, me: &str, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        if ui.add(egui::Button::new(RichText::new(BACK).strong()).frame(false)).clicked() {
            actions.push(Action::Back);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(cam) = c.camera
                && icon_button(ui, FLY, true, icon_location)
            {
                actions.push(Action::Fly(cam));
            }
            if c.author == me {
                let l = if c.resolved { MARK_UNRESOLVED } else { MARK_RESOLVED };
                if ui.button(l).clicked() {
                    actions.push(Action::Resolve(c.id.clone(), !c.resolved));
                }
            }
        });
    });
    if c.resolved {
        ui.separator();
        ui.label(RichText::new(RESOLVED).strong().size(13.0));
    }
    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            author_line(ui, &c.author, c.created, false);
            if !c.target.is_empty() {
                ui.label(RichText::new(&c.target).weak().size(10.0));
            }
            ui.label(RichText::new(&c.text).size(12.0));
        });
        for r in &c.replies {
            egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 4, top: 4, bottom: 4 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    author_line(ui, &r.author, r.created, false);
                    if r.author == me {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
                            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, OPTIONS));
                            icon_options(ui.painter(), rect.shrink(4.0), Color32::from_rgb(40, 44, 52));
                            egui::Popup::menu(&resp).show(|ui| {
                                if ui.button(DELETE_REPLY).clicked() {
                                    actions.push(Action::DeleteReply(c.id.clone(), r.id.clone()));
                                    ui.close();
                                }
                            });
                            if let Some(cam) = r.camera
                                && icon_button(ui, FLY, true, icon_location)
                            {
                                actions.push(Action::Fly(cam));
                            }
                        });
                    }
                });
                ui.label(RichText::new(&r.text).size(12.0));
            });
        }
    });
    ui.separator();
    let has_view = st.reply_camera.is_some();
    let out = editor(ui, &c.id, &mut st.reply, if has_view { DELETE_VIEW } else { ADD_VIEW });
    if out.view {
        actions.push(Action::ReplyView(!has_view));
    }
    if out.send {
        actions.push(Action::SubmitReply);
    }
}

fn toast(app: &mut EsquisseApp, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let st = &mut app.ui_state.extra.comments;
    let Some((text, until)) = &st.toast else { return };
    if now > *until {
        st.toast = None;
        return;
    }
    egui::Area::new(egui::Id::unique("comment-toast"))
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -60.0])
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(RichText::new(text.as_str()).size(13.0));
            });
        });
    ctx.request_repaint_after(std::time::Duration::from_millis(250));
}

/// SketchUp's collaboration bar, for a model that has comments: opens or closes the panel.
fn collab_bar(app: &mut EsquisseApp, ctx: &egui::Context) {
    let n = app.doc.model.comments.items.len();
    let st = &mut app.ui_state.extra.comments;
    if n == 0 || st.bar_hidden {
        return;
    }
    egui::Area::new(egui::Id::unique("collab-bar")).anchor(egui::Align2::CENTER_TOP, [0.0, 96.0]).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let open = st.open;
                if icon_button(ui, BAR_TOGGLE, true, |p, r, c| {
                    paint_bubble(
                        p,
                        r.left_bottom(),
                        r.width(),
                        if open { PIN_BLUE } else { Color32::TRANSPARENT },
                        Stroke::new(1.4, c),
                    )
                }) {
                    st.open = !st.open;
                }
                ui.label(RichText::new(n.to_string()).size(12.0));
                if icon_button(ui, BAR_HIDE, true, |p, r, c| {
                    p.line_segment([r.left_top(), r.right_bottom()], Stroke::new(1.4, c));
                    p.line_segment([r.right_top(), r.left_bottom()], Stroke::new(1.4, c));
                }) {
                    st.bar_hidden = true;
                }
            });
        });
    });
}

/// Every frame: what the tool placed, the reminders, the collaboration bar and the panel.
pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    // The Comment tool chosen (search, shortcut, panel): the panel shows, to type the text.
    let tool_on = app.tool.id() == ToolId::Comment;
    let st = &mut app.ui_state.extra.comments;
    if tool_on && !st.tool_was_on {
        st.open = true;
    }
    st.tool_was_on = tool_on;
    take_tool_events(app, now);
    toast(app, ctx);
    collab_bar(app, ctx);
    if !app.ui_state.extra.comments.open {
        return;
    }
    let comments = app.doc.model.comments.clone();
    let st = &mut app.ui_state.extra.comments;
    let me = st.me();
    let mut actions = Vec::new();
    let mut open = true;
    let right = ctx.content_rect().right();
    egui::Window::new(PANEL)
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .default_pos([right - 640.0, 110.0])
        .show(ctx, |ui| {
            ui.set_width(286.0);
            ui.horizontal(|ui| {
                if icon_button(ui, ADD, true, icon_add) {
                    actions.push(Action::StartTool);
                }
                if icon_button(ui, HOTSPOTS, true, icon_hotspots(st.pins)) {
                    actions.push(Action::TogglePins);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, FILTER));
                    icon_filter(ui.painter(), rect.shrink(5.0), Color32::from_rgb(40, 44, 52));
                    let resp = resp.on_hover_text(FILTER);
                    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                        ui.checkbox(&mut st.show_unread, UNREAD);
                        ui.checkbox(&mut st.show_read, READ);
                    });
                });
            });
            ui.separator();
            if let Some(d) = &mut st.draft {
                ui.label(
                    RichText::new(if d.target.is_empty() { "Nouveau commentaire" } else { &d.target })
                        .weak()
                        .size(11.0),
                );
                let out = editor(ui, "draft", &mut d.text, if d.camera.is_some() { UPDATE_VIEW } else { ADD_VIEW });
                if out.view {
                    actions.push(Action::DraftView);
                }
                if out.send {
                    actions.push(Action::SubmitDraft);
                }
                if ui.small_button("Annuler").clicked() {
                    actions.push(Action::CancelDraft);
                }
                ui.separator();
            }
            match st.thread.clone().and_then(|id| comments.get(&id)) {
                Some(c) => thread_view(ui, st, c, &me, &mut actions),
                None => list_view(ui, st, &comments, &me, &mut actions),
            }
        });
    if !open {
        app.ui_state.extra.comments.open = false;
    }
    for a in actions {
        apply(app, ctx, a);
    }
}

fn apply(app: &mut EsquisseApp, ctx: &egui::Context, a: Action) {
    let now = ctx.input(|i| i.time);
    match a {
        Action::StartTool => start_tool(app, now),
        Action::TogglePins => toggle_pins(app),
        Action::Open(id) => open_thread(app, &id, false),
        Action::Back => app.ui_state.extra.comments.thread = None,
        Action::Fly(c) => fly_to(app, &c),
        Action::SubmitDraft => {
            if let Some(id) = submit_draft(app) {
                app.ui_state.extra.comments.thread = Some(id);
            }
        }
        Action::CancelDraft => app.ui_state.extra.comments.draft = None,
        Action::DraftView => {
            let cam = scene_camera(&app.cam);
            if let Some(d) = &mut app.ui_state.extra.comments.draft {
                d.camera = Some(cam);
                app.ui_state.extra.comments.toast = Some((VIEW_CAPTURED.into(), now + 10.0));
            }
        }
        Action::SubmitReply => submit_reply(app),
        Action::ReplyView(add) => {
            let cam = scene_camera(&app.cam);
            let st = &mut app.ui_state.extra.comments;
            st.reply_camera = add.then_some(cam);
            st.toast = Some((if add { VIEW_CAPTURED } else { VIEW_DELETED }.into(), now + 10.0));
        }
        Action::Resolve(id, r) => set_resolved(app, &id, r),
        Action::DeleteThread(id) => delete_thread(app, &id),
        Action::DeleteReply(id, r) => delete_reply(app, &id, &r),
        Action::Mark(id, read) => mark(app, &id, read),
    }
}

/// The tool's icon (SketchUp's `tb_comment`: two speech bubbles), drawn like the others.
pub fn tool_icon(p: &egui::Painter, r: egui::Rect) {
    let s = r.width() / 24.0;
    let at = |x: f32, y: f32| r.min + egui::vec2(x * s, y * s);
    paint_bubble(p, at(4.0, 15.0), 12.0 * s, PIN_BLUE, Stroke::NONE);
    let back = bubble_points(at(8.0, 20.0), 12.0 * s).into_iter().map(|q| egui::pos2(2.0 * at(14.0, 0.0).x - q.x, q.y));
    p.add(egui::Shape::convex_polygon(back.rev().collect(), Color32::from_rgb(14, 65, 108), Stroke::NONE));
}

/// What the tool handed over since the last call (tests): `Some` placed, `None` dropped.
#[cfg(test)]
pub fn drain_tool_events() -> Vec<Option<tool::Placed>> {
    TOOL_EVENTS
        .with(|e| std::mem::take(&mut *e.borrow_mut()))
        .into_iter()
        .map(|ev| match ev {
            ToolEvent::Placed(p) => Some(p),
            ToolEvent::Cancelled => None,
        })
        .collect()
}
