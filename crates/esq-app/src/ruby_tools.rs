//! What the app tells Ruby each frame:
//! - the events of the active Ruby tool (mouse, keys, Measurements box) and of the overlays,
//!   then their `draw(view)`;
//! - what observers want to hear: changes made in the interface (undo, redo, a tool's
//!   commit, opening a group…), tool changes, camera moves, scene transitions.
//!
//! The viewport queues events ([`frame`]); they reach Ruby in the same run as the ui zone's
//! events, once per frame (`ruby_ui::show` → [`take_code`]). Nothing runs while Ruby has not
//! started.

use std::collections::HashSet;

use eframe::egui;
use esq_core::glam::{DVec2, DVec3};
use esq_core::model::{DefId, EntityRef, InstanceId};
use serde_json::{Value as J, json};
use slotmap::Key;

use crate::app::EsquisseApp;
use crate::doc::DocEvent;
use crate::tools::ruby::flags;
use crate::tools::{Tool, ToolId};

/// State kept between frames.
#[derive(Default)]
pub struct State {
    /// Kinds of observers registered in Ruby (`Esq::Events.observed_kinds`).
    pub observing: HashSet<String>,
    /// Tools suspended by `Tools#push_tool`, the last one resumes on `pop_tool`.
    pub suspended: Vec<Box<dyn Tool>>,
    /// Active tool last told to Ruby (id, Ruby token).
    pub reported: Option<(ToolId, u64)>,
    /// `busy()` of the active tool at the last frame (ToolsObserver#onToolStateChanged).
    busy: bool,
    /// Overlays added to the model: id, name, enabled.
    pub overlays: Vec<(String, String, bool)>,
    /// Their description and source (the extension that adds them), by id.
    pub overlay_info: std::collections::HashMap<String, (String, String)>,
    overlay_queue: Vec<J>,
    inside: bool,
    hover: Option<DVec2>,
    /// Cursor set by the tool's `onSetCursor` (0 = the default one).
    pub cursor: i64,
    /// `view.invalidate`: draw again.
    pub redraw: bool,
    /// `getExtents` of the tool and of the overlays (mm), for Zoom Extents.
    pub extents: Vec<(DVec3, DVec3)>,
    /// A tool switch done while a script ran: the real document's preview must go.
    pub clear_preview: bool,
    /// File › New / Open replaced the model: "new" or "open".
    pub replaced: Option<&'static str>,
    /// Top-left corner of the viewport (points).
    pub origin: egui::Pos2,
    /// Window › Overlays: an overlay turned on or off.
    pub toggle: Option<(String, bool)>,
    drawn: Option<(u64, String)>,
    synced: Option<(u64, Vec<EntityRef>)>,
    cam_sig: String,
    style_sig: String,
    scene: Option<usize>,
    from_scene: u64,
    in_transition: bool,
}

/// `Tools#active_tool_id` and `#active_tool_name` of the native tools (ids of SketchUp's
/// commands, names of its tool classes).
pub fn native_info(t: ToolId) -> (u32, &'static str) {
    use ToolId::*;
    match t {
        Select | Component => (21022, "SelectionTool"),
        Line => (21020, "SketchTool"),
        Rect => (21094, "RectangleTool"),
        RotRect => (24223, "RectangleTool"),
        Circle => (21096, "CircleTool"),
        Polygon => (21095, "PolyTool"),
        Arc => (21065, "ArcTool"),
        ArcCenter => (21069, "ArcTool"),
        Arc3 => (21071, "ArcTool"),
        Pie => (21070, "ArcTool"),
        Freehand => (21031, "FreehandTool"),
        PushPull => (21041, "PushPullTool"),
        FollowMe => (21525, "ExtrudeTool"),
        Move => (21048, "MoveTool"),
        Rotate => (21129, "RotateTool"),
        Scale => (21236, "ScaleTool"),
        Offset => (21100, "OffsetTool"),
        Eraser => (21019, "EraseTool"),
        Paint => (21074, "PaintTool"),
        Tape => (21024, "MeasureTool"),
        Protractor => (21057, "ProtractorTool"),
        AxesTool => (21126, "CoordinateTool"),
        Dimension => (21410, "DimensionTool"),
        Text => (21405, "TextTool"),
        SectionPlane => (21337, "SectionPlaneTool"),
        Lasso => (21027, "LassoSelectionTool"),
        TagTool => (21028, "TagTool"),
        Flip => (21029, "FlipTool"),
        North => (0, "NorthTool"),
        Orbit => (10508, "CameraOrbitTool"),
        // In SketchUp the hand is CCameraDollyTool ("Drag in direction to pan") and Look
        // Around is CCameraPanTool ("Drag in direction to turn camera", Eye Height).
        Pan => (10523, "CameraDollyTool"),
        Zoom => (10509, "CameraZoomTool"),
        ZoomWindow => (10526, "CameraZoomWindowTool"),
        Fov => (21494, "CameraFOVTool"),
        PositionCamera => (21169, "PositionCameraTool"),
        LookAround => (10525, "CameraPanTool"),
        Walk => (10520, "CameraWalkTool"),
        OuterShell => (24198, "OuterShellTool"),
        SolidIntersect => (24200, "BooleanIntersectTool"),
        SolidUnion => (24201, "BooleanUnionTool"),
        SolidSubtract => (24202, "BooleanDifferenceTool"),
        SolidTrim => (24203, "BooleanTrimTool"),
        SolidSplit => (24204, "BooleanSplitTool"),
        MatchPhoto => (0, "MatchPhotoTool"),
        Image => (0, "ImageTool"),
        Divide => (21287, "DivideLineTool"),
        ShowFaceEdges => (21138, "ShowFaceEdgesTool"),
        Classifier => (0, "ClassifierTool"),
        PhotoPoint => (0, "AddRayTool"),
        AddSnap => (0, "SnapTool"),
        Light => (0, "LightTool"),
        Igloo => (10631, "IglooTool"),
        SampleMaterial => (21255, "PaintMatchTool"),
        PositionTexture => (21515, "PositionTextureTool"),
        Comment => (21184, "CommentTool"),
        // The Sandbox tools are Ruby tools in SketchUp.
        FromScratch | FromContours | FlipEdge | AddDetail | Smoove | Drape | Stamp | Ruby => (0, "RubyTool"),
    }
}

/// The Ruby handle of an instance: « i:<collection>:<id> ».
pub fn instance_handle(ctx: Option<DefId>, inst: InstanceId) -> String {
    let c = ctx.map_or("0".to_string(), |d| d.data().as_ffi().to_string());
    format!("i:{c}:{}", inst.data().as_ffi())
}

/// A Ruby single-quoted string literal (no interpolation).
pub fn ruby_str(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn json_arg(v: &J) -> String {
    format!("JSON.parse({})", ruby_str(&v.to_string()))
}

/// Called once per frame by the viewport, after the tool events (`local`: the pointer over
/// the viewport, in points): pointer entering, leaving and moving over the viewport.
pub fn frame(
    app: &mut EsquisseApp,
    ctx: &egui::Context,
    origin: egui::Pos2,
    local: Option<DVec2>,
    mods: egui::Modifiers,
) {
    app.ui_state.extra.ruby_tools.origin = origin;
    if app.ui_state.extra.ruby.started().is_none() {
        return;
    }
    let ppp = ctx.pixels_per_point();
    let ruby = app.tool.ruby();
    if let Some(sh) = &ruby {
        sh.borrow_mut().ppp = ppp;
    }
    let st = &mut app.ui_state.extra.ruby_tools;
    let any_overlay = st.overlays.iter().any(|o| o.2);
    let inside = local.is_some();
    let px = |m: DVec2| (m.x * ppp as f64, m.y * ppp as f64);
    let f = flags(0, mods);
    if inside != st.inside {
        st.inside = inside;
        if let Some(sh) = &ruby {
            sh.borrow_mut().push(json!([if inside { "onMouseEnter" } else { "onMouseLeave" }]));
        }
        if any_overlay {
            match local {
                Some(m) => {
                    let (x, y) = px(m);
                    st.overlay_queue.push(json!(["onMouseEnter", f, x, y]));
                }
                None => st.overlay_queue.push(json!(["onMouseLeave"])),
            }
        }
    }
    if let Some(m) = local
        && st.hover != Some(m)
        && any_overlay
    {
        let (x, y) = px(m);
        if st.overlay_queue.last().is_some_and(|e| e[0] == "onMouseMove") {
            st.overlay_queue.pop();
        }
        st.overlay_queue.push(json!(["onMouseMove", f, x, y]));
    }
    st.hover = local;
}

/// The Ruby code this frame needs (observers, the Ruby tool's events and drawing), run by
/// `ruby_ui::show` with the ui zone's events. `Some((code, light))`: `light` when only mouse
/// moves and drawing are delivered (no undo step expected).
pub fn take_code(app: &mut EsquisseApp, now: f64) -> Option<(String, bool)> {
    if app.ui_state.extra.ruby.started().is_none() {
        app.doc.events.clear();
        app.ui_state.extra.ruby_tools.replaced = None;
        return None;
    }
    let mut code = observers(app, now);
    let light = code.is_empty();
    let (tool_code, tool_light) = tool_and_overlays(app);
    code.push_str(&tool_code);
    if code.is_empty() { None } else { Some((code, light && tool_light)) }
}

/// After the run of [`take_code`].
pub fn after_run(app: &mut EsquisseApp) {
    let st = &mut app.ui_state.extra.ruby_tools;
    if st.clear_preview {
        st.clear_preview = false;
        app.doc.set_preview(None);
    }
    st.synced = Some((app.doc.model_rev, app.doc.sel.clone()));
}

/// Changes made in the interface → `Esq::Events.app_sync`; tool changes → `Esq::Tools`.
fn observers(app: &mut EsquisseApp, now: f64) -> String {
    let mut code = String::new();
    let mut hints: Vec<String> = Vec::new();
    let ruby = app.tool.ruby();
    for e in std::mem::take(&mut app.doc.events) {
        match e {
            DocEvent::Commit => hints.push("commit".into()),
            DocEvent::Undo => {
                hints.push("undo".into());
                // SketchUp: onCancel(2, view) when the user undoes while a Ruby tool is active.
                if let Some(sh) = &ruby {
                    sh.borrow_mut().push(json!(["onCancel", 2]));
                }
            }
            DocEvent::Redo => hints.push("redo".into()),
            DocEvent::Open(c, i) => {
                hints.push("path".into());
                hints.push(format!("open:{}", instance_handle(c, i)));
            }
            DocEvent::Close(c, i) => {
                hints.push("path".into());
                hints.push(format!("close:{}", instance_handle(c, i)));
            }
        }
    }
    let st = &mut app.ui_state.extra.ruby_tools;
    // The document Ruby first sees is not a replacement (File › New at start-up, before Ruby).
    if st.synced.is_none() {
        st.replaced = None;
    }
    if let Some(how) = st.replaced.take() {
        code.push_str(&format!("Esq::Events.model_replaced('{how}'); Esq::Tools.reset!; Esq::Overlays.reset!\n"));
        st.observing.clear();
        st.overlays.clear();
        st.suspended.clear();
    }

    // The user picked another tool: the Ruby tool is deactivated, ToolsObserver hears it.
    let key = (app.tool.id(), ruby.as_ref().map_or(0, |s| s.borrow().token));
    match st.reported {
        None => st.reported = Some(key),
        Some(k) if k != key => {
            st.reported = Some(key);
            if key.0 != ToolId::Ruby {
                let (id, name) = native_info(key.0);
                code.push_str(&format!("Esq::Tools.app_changed('{name}', {id})\n"));
            }
        }
        _ => {}
    }
    let busy = ruby.is_none() && app.tool.busy();
    if busy != st.busy {
        st.busy = busy;
        if st.observing.contains("tools") {
            let (id, name) = native_info(app.tool.id());
            code.push_str(&format!("Esq::Tools.app_state_changed('{name}', {id}, {})\n", busy as i32));
        }
    }

    if st.observing.contains("view") {
        let sig = format!("{:?}", app.cam);
        if sig != st.cam_sig {
            st.cam_sig = sig;
            hints.push("view".into());
        }
    }
    if st.observing.contains("rendering_options") {
        let sig = format!("{:?}", app.style);
        if sig != st.style_sig {
            st.style_sig = sig;
            hints.push("ro".into());
        }
    }
    // Scene transitions: Pages.add_frame_change_observer → frameChange(from, to, percent).
    if st.observing.contains("frame") {
        app.doc.model.ensure_scene_uids();
        let uid = |i: Option<usize>| i.and_then(|i| app.doc.model.scenes.get(i)).map_or(0, |s| s.ext.uid);
        let st = &mut app.ui_state.extra.ruby_tools;
        if app.active_scene != st.scene {
            st.from_scene = uid(st.scene);
            st.scene = app.active_scene;
            if app.transition.is_none() && st.scene.is_some() {
                hints.push(format!("frame:{}:{}:1.0", fmt_uid(st.from_scene), uid(st.scene)));
            }
        }
        match &app.transition {
            Some(t) => {
                st.in_transition = true;
                hints.push(format!("frame:{}:{}:{}", fmt_uid(st.from_scene), uid(st.scene), t.progress(now)));
            }
            None if st.in_transition => {
                st.in_transition = false;
                hints.push(format!("frame:{}:{}:1.0", fmt_uid(st.from_scene), uid(st.scene)));
            }
            None => {}
        }
    }

    let st = &mut app.ui_state.extra.ruby_tools;
    let now_state = (app.doc.model_rev, app.doc.sel.clone());
    let doc_changed = st.synced.as_ref() != Some(&now_state);
    if !st.observing.is_empty() && (doc_changed || !hints.is_empty()) {
        let list: Vec<J> = hints.iter().map(|h| json!(h)).collect();
        code.push_str(&format!("Esq::Events.app_sync({})\n", json_arg(&J::Array(list))));
    } else if !code.is_empty() {
        code.push_str("Esq::Events.notify_host\n");
    }
    if code.is_empty() {
        st.synced = Some(now_state);
    }
    code
}

fn fmt_uid(u: u64) -> String {
    if u == 0 { String::new() } else { u.to_string() }
}

/// Events of the Ruby tool and of the overlays, then their `draw(view)`.
fn tool_and_overlays(app: &mut EsquisseApp) -> (String, bool) {
    let ruby = app.tool.ruby();
    let st = &mut app.ui_state.extra.ruby_tools;
    let any_overlay = st.overlays.iter().any(|o| o.2);
    let key = (app.doc.rev, format!("{:?}", app.cam));
    // A tool that left keeps its drawing until the next one: draw once more without it.
    let had_drawing = !app.ui_state.extra.view.draw.is_empty();
    let st = &mut app.ui_state.extra.ruby_tools;
    if ruby.is_none() && !any_overlay && !(st.redraw && had_drawing) {
        return (String::new(), true);
    }
    let need_draw = st.redraw || st.drawn.as_ref() != Some(&key);
    let ov_events = std::mem::take(&mut st.overlay_queue);
    let (tool_events, token) = match &ruby {
        Some(sh) => {
            let mut s = sh.borrow_mut();
            (std::mem::take(&mut s.queue), s.token)
        }
        None => (Vec::new(), 0),
    };
    if !need_draw && ov_events.is_empty() && tool_events.is_empty() {
        return (String::new(), true);
    }
    let mut code = String::new();
    if !ov_events.is_empty() {
        code.push_str(&format!("Esq::Overlays.deliver({}, false)\n", json_arg(&J::Array(ov_events))));
    }
    // Only mouse moves (and enter / leave): no change of the model is expected.
    let light =
        tool_events.iter().all(|e| matches!(e[0].as_str(), Some("onMouseMove" | "onMouseEnter" | "onMouseLeave")));
    if ruby.is_some() {
        code.push_str(&format!("Esq::Tools.deliver({token}, {}, true)\n", json_arg(&J::Array(tool_events))));
    } else {
        code.push_str("Esq::Tools.render_all\n");
    }
    st.redraw = false;
    st.drawn = Some(key);
    (code, light)
}

/// The mouse wheel over the viewport while a Ruby tool is active: true if the tool's
/// `onMouseWheel` returned true (then the view does not zoom). Run at once: the answer
/// decides whether the view zooms in this frame.
pub fn wheel(app: &mut EsquisseApp, m: DVec2, delta: f64, mods: egui::Modifiers, ppp: f32) -> bool {
    let Some(sh) = app.tool.ruby() else { return false };
    let Some(interp) = app.ui_state.extra.ruby.started() else { return false };
    let (token, buttons) = {
        let s = sh.borrow();
        (s.token, s.buttons)
    };
    let d = if delta > 0.0 { 1 } else { -1 };
    let ev = json!([["onMouseWheel", flags(buttons, mods), d, m.x * ppp as f64, m.y * ppp as f64]]);
    let code = format!("Esq::Tools.deliver({token}, {})", json_arg(&ev));
    let r = crate::ruby_host::run_light(app, "Outil Ruby", |m, s| interp.run(&code, "outil", m, s));
    let r = r.map_err(|e| format!("{e:#}"));
    let used = r.as_ref().is_ok_and(|o| o.ok && o.result == "true");
    app.ui_state.extra.ruby.report_run(r);
    used
}

/// Window › Overlays, and the cursor of the Ruby tool. Called once per frame.
pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    let st = &app.ui_state.extra.ruby_tools;
    if st.inside && st.cursor != 0 && app.tool.ruby().is_some() {
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    // Window › Overlays is a panel of the tray (`overlays_panel.rs`); it asks for toggles.
    if let Some((id, on)) = app.ui_state.extra.ruby_tools.toggle.take()
        && let Some(interp) = app.ui_state.extra.ruby.started()
    {
        let code = format!("Esq::Overlays.toggle({}, {on})", ruby_str(&id));
        let r = crate::ruby_host::run_script(app, "Surcouche", |m, s| interp.run(&code, "surcouches", m, s));
        app.ui_state.extra.ruby.report_run(r.map_err(|e| format!("{e:#}")));
    }
}

/// Status bar prompt (0), Measurements label (1) or value (2) of the active Ruby tool
/// (`Sketchup.set_status_text`, `vcb_label=`, `vcb_value=`). False when no Ruby tool is active.
pub fn status(app: &mut EsquisseApp, pos: i64, text: &str) -> bool {
    tool_status(app.tool.as_ref(), pos, text)
}

/// [`status`] for a given tool.
pub fn tool_status(tool: &dyn Tool, pos: i64, text: &str) -> bool {
    let Some(sh) = tool.ruby() else { return false };
    let mut s = sh.borrow_mut();
    match pos {
        1 => s.vcb_label = text.to_string(),
        2 => s.vcb_value = text.to_string(),
        _ => s.status = text.to_string(),
    }
    true
}

/// The app closes: `AppObserver#onQuit`.
pub fn quit(app: &mut EsquisseApp) {
    let Some(interp) = app.ui_state.extra.ruby.started() else { return };
    if !app.ui_state.extra.ruby_tools.observing.contains("app") {
        return;
    }
    let r = crate::ruby_host::run_script(app, "Observateurs Ruby", |m, s| {
        interp.run("Esq::Events.app_event(:onQuit)", "observateurs", m, s)
    });
    app.ui_state.extra.ruby.report_run(r.map_err(|e| format!("{e:#}")));
}

/// Context menu of the viewport: a Ruby tool's `getMenu` items (menu « Context/Tool », see
/// `Esq::Tools.fill_context_menu`) replace the app's own entries, as in SketchUp.
pub fn tool_menu_shown(app: &EsquisseApp) -> bool {
    app.tool.ruby().is_some() && app.ui_state.extra.ruby.context_items_of("Context/Tool")
}
