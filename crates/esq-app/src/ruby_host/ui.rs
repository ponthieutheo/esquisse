//! `host.ui.*`: what the Ruby `UI` module asks of the application — message boxes and file
//! panels (real native windows), web dialogs (WebView2, see `web`), timers, toolbars' visibility,
//! notifications (toasts), the console, inspectors and so on.
//!
//! Ruby runs on its own thread while the UI thread waits inside `run_script`, so a call that
//! blocks (a message box, a modal dialog) simply blocks the script. Things that happen later (a
//! JavaScript callback, a timer, a notification button) are queued as events; `ruby_ui::show`
//! hands them to Ruby once per frame (`Esq.ui_pump`).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use esq_core::model::{EntityRef, Model};
use serde_json::{Value as J, json};

use crate::app::EsquisseApp;
use crate::commands::{Cmd, Panel};

mod web;
mod win;

// ------------------------------------------------------------------------------------ shared state

struct Timer {
    id: u64,
    due: Instant,
    every: Duration,
    repeat: bool,
}

struct Toast {
    id: u64,
    message: String,
    tooltip: String,
    accept: Option<String>,
    dismiss: Option<String>,
}

/// Something only the UI thread can do (it needs the egui context).
enum Req {
    Cmd(Cmd),
}

static EVENTS: Mutex<VecDeque<J>> = Mutex::new(VecDeque::new());
static EVENTS_CV: Condvar = Condvar::new();
static TIMERS: Mutex<Vec<Timer>> = Mutex::new(Vec::new());
static NEXT_TIMER: AtomicU64 = AtomicU64::new(1);
static TOASTS: Mutex<Vec<Toast>> = Mutex::new(Vec::new());
static REQS: Mutex<Vec<Req>> = Mutex::new(Vec::new());
static CURSORS: Mutex<Vec<(String, i32, i32)>> = Mutex::new(Vec::new());
static REPAINT: Mutex<Option<egui::Context>> = Mutex::new(None);
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
/// Plugin toolbars: shown or hidden (absent = never shown), kept between sessions.
static PLUGIN_TOOLBARS: Mutex<Option<BTreeMap<String, bool>>> = Mutex::new(None);

/// Queues an event for Ruby (called from any thread) and wakes the UI.
pub(super) fn push_event(ev: J) {
    EVENTS.lock().unwrap().push_back(ev);
    EVENTS_CV.notify_all();
    if let Some(c) = REPAINT.lock().unwrap().as_ref() {
        c.request_repaint();
    }
}

fn drain_events() -> Vec<J> {
    EVENTS.lock().unwrap().drain(..).collect()
}

pub fn has_events() -> bool {
    !EVENTS.lock().unwrap().is_empty()
}

fn fire_due_timers() {
    let now = Instant::now();
    let mut fired = Vec::new();
    {
        let mut ts = TIMERS.lock().unwrap();
        for t in ts.iter_mut() {
            if t.due <= now {
                fired.push(t.id);
                t.due = now + t.every;
            }
        }
        ts.retain(|t| t.repeat || !fired.contains(&t.id));
    }
    for id in fired {
        push_event(json!({"t": "timer", "id": id}));
    }
}

fn next_timer_due() -> Option<Duration> {
    let now = Instant::now();
    TIMERS.lock().unwrap().iter().map(|t| t.due.saturating_duration_since(now)).min()
}

// ------------------------------------------------------------------------------------ plugin toolbars

fn plugin_toolbar_file() -> Option<std::path::PathBuf> {
    Some(crate::settings::user_dir()?.join("plugin_toolbars.json"))
}

fn with_plugin_toolbars<R>(f: impl FnOnce(&mut BTreeMap<String, bool>) -> R) -> R {
    let mut g = PLUGIN_TOOLBARS.lock().unwrap();
    let m = g.get_or_insert_with(|| {
        plugin_toolbar_file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|s| serde_json::from_str::<BTreeMap<String, bool>>(&s).ok())
            .unwrap_or_default()
    });
    f(m)
}

fn save_plugin_toolbars() {
    let m = with_plugin_toolbars(|m| m.clone());
    if let Some(f) = plugin_toolbar_file() {
        let _ = std::fs::write(f, serde_json::to_string(&m).unwrap_or_default());
    }
}

/// Whether a plugin toolbar is shown.
pub fn plugin_toolbar_shown(name: &str) -> bool {
    with_plugin_toolbars(|m| m.get(name).copied().unwrap_or(false))
}

pub fn set_plugin_toolbar_shown(name: &str, shown: bool) {
    with_plugin_toolbars(|m| {
        m.insert(name.to_string(), shown);
    });
    save_plugin_toolbars();
}

// ------------------------------------------------------------------------------------ frame

/// Once per frame, before Ruby's turn: timers, requests, the notification toasts.
pub fn frame(app: &mut EsquisseApp, ctx: &egui::Context) {
    win::note_main_window();
    {
        let mut r = REPAINT.lock().unwrap();
        if r.is_none() {
            *r = Some(ctx.clone());
        }
    }
    fire_due_timers();
    for r in std::mem::take(&mut *REQS.lock().unwrap()) {
        match r {
            Req::Cmd(c) => app.run(ctx, c),
        }
    }
    toasts(ctx);
    if let Some(d) = next_timer_due() {
        ctx.request_repaint_after(d.max(Duration::from_millis(5)));
    }
}

fn toasts(ctx: &egui::Context) {
    let mut ts = TOASTS.lock().unwrap();
    if ts.is_empty() {
        return;
    }
    let mut done: Vec<(u64, &'static str)> = Vec::new();
    egui::Area::new(egui::Id::unique("esq_toasts"))
        .anchor(egui::Align2::RIGHT_BOTTOM, [-14.0, -34.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            for t in ts.iter() {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(340.0);
                    ui.horizontal(|ui| {
                        let l = ui.label(&t.message);
                        if !t.tooltip.is_empty() {
                            l.on_hover_text(&t.tooltip);
                        }
                    });
                    ui.horizontal(|ui| {
                        if let Some(a) = &t.accept
                            && ui.button(if a.is_empty() { "OK" } else { a }).clicked()
                        {
                            done.push((t.id, "accept"));
                        }
                        if ui.button(t.dismiss.as_deref().filter(|d| !d.is_empty()).unwrap_or("×")).clicked() {
                            done.push((t.id, "dismiss"));
                        }
                    });
                });
                ui.add_space(6.0);
            }
        });
    for (id, what) in done {
        ts.retain(|t| t.id != id);
        push_event(json!({"t": "notif", "id": id, "action": what}));
    }
}

// ------------------------------------------------------------------------------------ plugin toolbars drawing

fn icon_texture(ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
    static CACHE: Mutex<Option<HashMap<String, Option<egui::TextureHandle>>>> = Mutex::new(None);
    let mut g = CACHE.lock().unwrap();
    let m = g.get_or_insert_with(HashMap::new);
    m.entry(path.to_string())
        .or_insert_with(|| {
            let img = image::open(path).ok()?.to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            Some(crate::gpu_img::load_texture(
                ctx,
                format!("plugin:{path}"),
                size,
                img.as_raw(),
                egui::TextureOptions::LINEAR,
            ))
        })
        .clone()
}

/// Draws the toolbars plugins have built and shown, after the built-in ones.
pub fn draw_toolbars(app: &mut EsquisseApp, ui: &mut egui::Ui, side: f32, tooltips: bool) {
    let bars = app.ui_state.extra.ruby.toolbars.clone();
    for bar in bars.iter().filter(|b| plugin_toolbar_shown(&b.name)) {
        ui.separator();
        for item in &bar.items {
            let Some(c) = item else {
                ui.separator();
                continue;
            };
            let icon = [&c.small_icon, &c.large_icon].into_iter().find(|p| !p.is_empty());
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
            let on = c.checked();
            let bg = if on {
                egui::Color32::from_rgb(204, 222, 252)
            } else if resp.hovered() && c.enabled() {
                egui::Color32::from_rgb(230, 234, 240)
            } else {
                egui::Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, 4.0, bg);
            let inner = rect.shrink(side / 8.0);
            let mut drawn = false;
            if let Some(p) = icon
                && let Some(tex) = icon_texture(ui.ctx(), p)
            {
                let tint = if c.enabled() { egui::Color32::WHITE } else { egui::Color32::from_white_alpha(90) };
                egui::Image::new(&tex).tint(tint).paint_at(ui, inner);
                drawn = true;
            }
            if !drawn {
                let letter = c.title.chars().next().unwrap_or('?').to_uppercase().to_string();
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    letter,
                    egui::FontId::proportional(side * 0.5),
                    if c.enabled() { egui::Color32::from_rgb(40, 44, 52) } else { egui::Color32::GRAY },
                );
            }
            // Named for accessibility (screen readers, UI Automation) like any other button.
            let (enabled, name) = (c.enabled(), if c.title.is_empty() { c.tooltip.clone() } else { c.title.clone() });
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &name));
            let tip = if !c.tooltip.is_empty() { c.tooltip.clone() } else { c.title.clone() };
            let resp = if tooltips && !tip.is_empty() { resp.on_hover_text(tip) } else { resp };
            if resp.clicked() && c.enabled() {
                app.ui_state.extra.ruby.click(c.id);
            }
        }
    }
}

// ------------------------------------------------------------------------------------ ops

fn s<'a>(a: &'a J, k: &str) -> &'a str {
    a[k].as_str().unwrap_or_default()
}

fn id_of(a: &J) -> u64 {
    a["id"].as_u64().unwrap_or(0)
}

/// `Name|*.ext;*.ext2|Name2|*.*||` → (name, extensions).
fn parse_filters(spec: &str) -> Vec<(String, Vec<String>)> {
    let parts: Vec<&str> = spec.split('|').filter(|p| !p.is_empty()).collect();
    parts
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| {
            let exts = c[1]
                .split(';')
                .map(|e| e.trim().trim_start_matches("*.").to_string())
                .filter(|e| !e.is_empty() && e != "*")
                .collect();
            (c[0].to_string(), exts)
        })
        .collect()
}

fn dialog_dir(dir: &str) -> Option<std::path::PathBuf> {
    let p = std::path::PathBuf::from(dir);
    (!dir.is_empty() && p.is_dir()).then_some(p)
}

/// File types Windows runs rather than shows: `UI.openURL` never hands one to the shell.
const RUNNABLE: &[&str] = &[
    "exe",
    "com",
    "bat",
    "cmd",
    "scr",
    "pif",
    "msi",
    "msp",
    "mst",
    "msc",
    "cpl",
    "hta",
    "js",
    "jse",
    "vb",
    "vbs",
    "vbe",
    "wsf",
    "wsh",
    "ws",
    "wsc",
    "ps1",
    "psm1",
    "psd1",
    "ps1xml",
    "lnk",
    "url",
    "reg",
    "jar",
    "inf",
    "scf",
    "chm",
    "dll",
    "sys",
    "application",
    "appref-ms",
    "gadget",
    "settingcontent-ms",
    "library-ms",
    "search-ms",
    "searchconnector-ms",
    "diagcab",
    "msix",
    "appx",
    "appinstaller",
    "rdp",
    "iqy",
    "slk",
    "xll",
];

/// Whether `UI.openURL` may hand `url` to Windows' URL handler (`url.dll`, which also runs
/// programs): web and mail links, and `file:` links to a local document. A `file:` link to a
/// network share would send the user's NTLM hash, one to a program (`.exe`, `.bat`, `.lnk`…)
/// would run it.
fn open_url_allowed(url: &str) -> bool {
    let u = url.trim();
    let low = u.to_ascii_lowercase();
    if low.starts_with("http://") || low.starts_with("https://") || low.starts_with("mailto:") {
        // One argument, no line break or quote that could change how it is read.
        return !u.chars().any(|c| c.is_control() || c == '"');
    }
    let Some(rest) = u.get(5..).filter(|_| low.starts_with("file:")) else { return false };
    let path = esq_core::untrusted::percent_decode(rest);
    // file://serveur/…, file:////serveur/… (the host is checked on the link as written).
    if esq_core::untrusted::is_remote(u) || path.contains(['"', '\0']) {
        return false;
    }
    // The file itself, without the link's query or anchor, nor the slashes before the drive.
    let local = path.split(['#', '?']).next().unwrap_or("").trim_start_matches(['/', '\\']);
    // A local, absolute path (C:/…) of plain names: no device, no stream (`x.exe::$DATA`); a
    // network path written with escapes (`%5C%5Cserveur`) is no longer absolute once the
    // slashes before it are gone.
    let Some(p) = esq_core::untrusted::local_path(local) else { return false };
    let plain = p.components().all(|c| match c {
        std::path::Component::Normal(n) => n.to_str().is_some_and(esq_core::untrusted::is_safe_component),
        _ => true,
    });
    let file = p.file_name().and_then(|f| f.to_str()).unwrap_or("");
    let ext = file.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    plain && !ext.is_some_and(|e| RUNNABLE.contains(&e.as_str()))
}

fn builtin_toolbar(name: &str) -> bool {
    crate::toolbars::TOOLBARS.iter().any(|(n, _)| *n == name)
}

#[allow(unused_variables, clippy::ptr_arg)]
pub fn call(
    app: &mut EsquisseApp,
    model: &mut Model,
    sel: &mut Vec<EntityRef>,
    op: &str,
    args: &J,
) -> Result<J, String> {
    let Some(op) = op.strip_prefix("ui.") else { return Err(format!("opération hôte inconnue: {op}")) };
    // The few operations that need the application's state; the rest only needs the OS.
    match op {
        // Sketchup.vcb_label= / vcb_value= : the Measurements box of the active Ruby tool.
        "vcb" => {
            for (pos, key) in [(1, "label"), (2, "value")] {
                if let Some(t) = args[key].as_str() {
                    crate::ruby_tools::status(app, pos, t);
                }
            }
            return Ok(J::Null);
        }
        "toolbar_visible" if !args["plugin"].as_bool().unwrap_or(false) => {
            let name = s(args, "name");
            crate::toolbars::ensure_loaded(&mut app.ui_state.extra.toolbars);
            return Ok(json!(builtin_toolbar(name) && app.ui_state.extra.toolbars.shown(name)));
        }
        "toolbar_set_visible" if !args["plugin"].as_bool().unwrap_or(false) => {
            let (name, v) = (s(args, "name"), args["visible"].as_bool().unwrap_or(false));
            if !builtin_toolbar(name) {
                return Ok(json!(false));
            }
            crate::toolbars::ensure_loaded(&mut app.ui_state.extra.toolbars);
            app.ui_state.extra.toolbars.set_shown(name, v);
            crate::toolbars::save(&app.ui_state.extra.toolbars);
            return Ok(json!(true));
        }
        "console" => {
            let r = &mut app.ui_state.extra.ruby;
            return Ok(match s(args, "action") {
                "write" => {
                    r.console_write(s(args, "text"));
                    json!(true)
                }
                "clear" => {
                    r.console_clear();
                    json!(true)
                }
                "show" => {
                    r.console_open = true;
                    json!(true)
                }
                "hide" => {
                    r.console_open = false;
                    json!(true)
                }
                _ => json!(r.console_open),
            });
        }
        _ => {}
    }
    pure(op, args)
}

/// The `ui.*` operations that do not touch the application (also what the tests drive).
fn pure(op: &str, args: &J) -> Result<J, String> {
    Ok(match op {
        // ---- native windows
        "messagebox" => {
            let ty = args["type"].as_u64().unwrap_or(0) as u32;
            // MB_MULTILINE (16) has no Win32 twin: a plain OK box.
            let ty = if ty == 0x10 { 0 } else { ty };
            let cap = if s(args, "title").is_empty() { "Esquisse" } else { s(args, "title") };
            json!(win::message_box(s(args, "text"), cap, ty))
        }
        "openpanel" => {
            let mut d = rfd::FileDialog::new();
            let t = s(args, "title");
            if !t.is_empty() {
                d = d.set_title(t);
            }
            if let Some(p) = dialog_dir(s(args, "dir")) {
                d = d.set_directory(p);
            }
            let f = s(args, "filename");
            if f.contains('|') {
                for (name, exts) in parse_filters(f) {
                    if !exts.is_empty() {
                        d = d.add_filter(name, &exts);
                    }
                }
            } else if !f.is_empty() {
                d = d.set_file_name(f);
            }
            d.pick_file().map_or(J::Null, |p| json!(p.to_string_lossy()))
        }
        "savepanel" => {
            let mut d = rfd::FileDialog::new();
            let t = s(args, "title");
            if !t.is_empty() {
                d = d.set_title(t);
            }
            if let Some(p) = dialog_dir(s(args, "dir")) {
                d = d.set_directory(p);
            }
            let f = s(args, "filename");
            if !f.is_empty() {
                d = d.set_file_name(f);
            }
            d.save_file().map_or(J::Null, |p| json!(p.to_string_lossy()))
        }
        "select_directory" => {
            let mut d = rfd::FileDialog::new();
            let t = s(args, "title");
            if !t.is_empty() {
                d = d.set_title(t);
            }
            if let Some(p) = dialog_dir(s(args, "directory")) {
                d = d.set_directory(p);
            }
            if args["multiple"].as_bool().unwrap_or(false) {
                d.pick_folders().map_or(J::Null, |v| json!(v.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>()))
            } else {
                d.pick_folder().map_or(J::Null, |p| json!(p.to_string_lossy()))
            }
        }
        "openurl" => {
            let url = s(args, "url");
            let ok = open_url_allowed(url)
                && std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn().is_ok();
            json!(ok)
        }
        "beep" => {
            win::beep();
            J::Null
        }
        "play_sound" => {
            win::play_sound(s(args, "path"));
            J::Null
        }
        "clip_get" => win::clipboard_get().map_or(J::Null, |t| json!(t)),
        "clip_set" => json!(win::clipboard_set(s(args, "text"))),
        "scale_factor" => json!(win::scale_factor()),

        // ---- timers and events
        "timer_add" => {
            let ms = args["ms"].as_u64().unwrap_or(0).max(1);
            let id = NEXT_TIMER.fetch_add(1, Ordering::Relaxed);
            let every = Duration::from_millis(ms);
            TIMERS.lock().unwrap().push(Timer {
                id,
                due: Instant::now() + every,
                every,
                repeat: args["repeat"].as_bool().unwrap_or(false),
            });
            if let Some(c) = REPAINT.lock().unwrap().as_ref() {
                c.request_repaint_after(every);
            }
            json!(id)
        }
        "timer_remove" => {
            let id = id_of(args);
            TIMERS.lock().unwrap().retain(|t| t.id != id);
            J::Null
        }
        "poll" => {
            fire_due_timers();
            J::Array(drain_events())
        }

        // ---- toolbars
        "toolbar_names" => json!(crate::toolbars::TOOLBARS.iter().map(|(n, _)| *n).collect::<Vec<_>>()),
        "toolbar_visible" => json!(plugin_toolbar_shown(s(args, "name"))),
        "toolbar_set_visible" => {
            set_plugin_toolbar_shown(s(args, "name"), args["visible"].as_bool().unwrap_or(false));
            json!(true)
        }
        "toolbar_state" => {
            let name = s(args, "name");
            let known = with_plugin_toolbars(|m| m.get(name).copied());
            json!(match known {
                Some(true) => 1,
                Some(false) => 0,
                None => -1,
            })
        }

        // ---- cursors
        "cursor_create" => {
            let mut c = CURSORS.lock().unwrap();
            c.push((
                s(args, "path").to_string(),
                args["x"].as_i64().unwrap_or(0) as i32,
                args["y"].as_i64().unwrap_or(0) as i32,
            ));
            json!(c.len())
        }
        "cursor_set" => {
            json!(args["id"].as_u64().is_some_and(|i| i >= 1 && i as usize <= CURSORS.lock().unwrap().len()))
        }

        // ---- panels and dialogs of the app
        "inspector_show" => {
            let p = match s(args, "name") {
                "Materials" => Some(Panel::Materials),
                "Components" => Some(Panel::Components),
                "Styles" => Some(Panel::Styles),
                "Scenes" => Some(Panel::Scenes),
                "Shadows" => Some(Panel::Shadows),
                "Layers" => Some(Panel::Tags),
                "SoftenEdges" => Some(Panel::Soften),
                "Outliner" => Some(Panel::Outliner),
                "MatchPhoto" => Some(Panel::MatchPhoto),
                "EntityInfo" => Some(Panel::EntityInfo),
                "HatchPatterns" => Some(Panel::HatchPatterns),
                "Fog" => Some(Panel::Fog),
                "Environments" => Some(Panel::Environments),
                "Overlays" => Some(Panel::Overlays),
                "ModelingGrid" => Some(Panel::ModelGrid),
                "Instructor" => Some(Panel::Instructor),
                _ => None,
            };
            if let Some(p) = p {
                REQS.lock().unwrap().push(Req::Cmd(Cmd::Panel(p)));
            }
            json!(p.is_some())
        }
        "modelinfo_show" => {
            REQS.lock().unwrap().push(Req::Cmd(Cmd::ModelInfo));
            J::Null
        }
        "prefs_show" => {
            REQS.lock().unwrap().push(Req::Cmd(Cmd::Preferences));
            J::Null
        }
        "extmgr_show" => {
            REQS.lock().unwrap().push(Req::Cmd(Cmd::ExtensionManager));
            J::Null
        }
        "notify" => {
            let mut t = TOASTS.lock().unwrap();
            let id = id_of(args);
            t.retain(|x| x.id != id);
            t.push(Toast {
                id,
                message: s(args, "message").to_string(),
                tooltip: s(args, "tooltip").to_string(),
                accept: args["accept"].as_str().map(str::to_string),
                dismiss: args["dismiss"].as_str().map(str::to_string),
            });
            drop(t);
            if let Some(c) = REPAINT.lock().unwrap().as_ref() {
                c.request_repaint();
            }
            json!(true)
        }

        // ---- web dialogs
        "html_open" => {
            let id = id_of(args);
            let bg = args["bg"].as_str().map(str::to_string);
            let opts = web::Opts::from_json(&args["opts"], bg);
            let content = args["content"]
                .as_array()
                .and_then(|c| Some(web::Content::from_json(c.first()?.as_str()?, c.get(1)?.as_str()?)));
            web::send(web::Cmd::Open {
                id,
                opts,
                content,
                can_close_hook: args["can_close_hook"].as_bool().unwrap_or(false),
            })?;
            J::Null
        }
        "html_load" => {
            web::send(web::Cmd::Load {
                id: id_of(args),
                content: web::Content::from_json(s(args, "kind"), s(args, "value")),
            })?;
            J::Null
        }
        "html_show" => send0(web::Cmd::Show(id_of(args)))?,
        "html_hide" => send0(web::Cmd::Hide(id_of(args)))?,
        "html_close" => send0(web::Cmd::Close { id: id_of(args) })?,
        "html_front" => send0(web::Cmd::Front(id_of(args)))?,
        "html_center" => send0(web::Cmd::Center(id_of(args)))?,
        "html_hook" => {
            send0(web::Cmd::Hook { id: id_of(args), can_close: args["can_close"].as_bool().unwrap_or(false) })?
        }
        "html_exec" => send0(web::Cmd::Exec { id: id_of(args), js: s(args, "js").to_string() })?,
        "html_bg" => send0(web::Cmd::Bg { id: id_of(args), color: s(args, "color").to_string() })?,
        "html_pos" => send0(web::Cmd::Pos {
            id: id_of(args),
            x: args["x"].as_i64().unwrap_or(0) as i32,
            y: args["y"].as_i64().unwrap_or(0) as i32,
        })?,
        "html_size" => send0(web::Cmd::Size {
            id: id_of(args),
            w: args["w"].as_i64().unwrap_or(0) as i32,
            h: args["h"].as_i64().unwrap_or(0) as i32,
            content: args["content"].as_bool().unwrap_or(false),
        })?,
        "html_limits" => {
            let o = web::Opts::from_json(&args["opts"], None);
            send0(web::Cmd::Limits { id: id_of(args), min: o.min, max: o.max })?
        }
        "html_geo" => match web::geo(id_of(args)) {
            Some(g) => json!({"visible": g.visible, "x": g.x, "y": g.y, "w": g.w, "h": g.h, "cw": g.cw, "ch": g.ch}),
            None => J::Null,
        },
        "html_eval" => {
            let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
            web::send(web::Cmd::Eval { id: id_of(args), js: s(args, "js").to_string(), token })?;
            match web::wait_result(token, Duration::from_secs(5)) {
                Some(r) => serde_json::from_str::<J>(&r).unwrap_or(J::Null),
                None => J::Null,
            }
        }
        "html_capture" => {
            let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
            web::send(web::Cmd::Capture { id: id_of(args), path: s(args, "path").to_string(), token })?;
            json!(web::wait_result(token, Duration::from_secs(5)).is_some())
        }
        // Modal dialogs: the script waits here, in slices, and handles what comes in.
        "html_wait" => {
            let ms = args["ms"].as_u64().unwrap_or(100).min(1000);
            fire_due_timers();
            let mut g = EVENTS.lock().unwrap();
            if g.is_empty() {
                g = EVENTS_CV.wait_timeout(g, Duration::from_millis(ms)).unwrap().0;
            }
            J::Array(g.drain(..).collect())
        }
        _ => return Err(format!("opération hôte non implémentée: {op}")),
    })
}

fn send0(c: web::Cmd) -> Result<J, String> {
    web::send(c)?;
    Ok(J::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `UI.openURL` gave any `file:` link to `url.dll,FileProtocolHandler`, which opens a
    /// network share (NTLM hash sent) and runs programs (`file:///C:/…/x.exe`): an extension
    /// that opens a link read from a model or a web page could be made to run one.
    #[test]
    fn open_url_refuses_network_shares_and_programs() {
        for ok in [
            "https://sketchup.com",
            "http://exemple.org/a?b=c",
            "mailto:a@b.fr",
            "file:///C:/Docs/guide.pdf",
            "file:///C:/Docs/guide%20complet.html#partie",
            "file:C:/Docs/plan.png",
        ] {
            assert!(open_url_allowed(ok), "{ok}");
        }
        for bad in [
            "file://serveur/partage/doc.pdf",
            "file:////serveur/partage/doc.pdf",
            r"file:///\\serveur\partage\doc.pdf",
            r"file:\\serveur\partage\doc.pdf",
            "file://%5C%5Cserveur/partage/doc.pdf",
            "file:///C:/Users/x/Downloads/piege.exe",
            "file:///C:/x/piege.EXE",
            "file:///C:/x/piege.exe.",
            "file:///C:/x/piege.bat#a",
            "file:///C:/x/raccourci.lnk",
            "file:///C:/x/script.ps1",
            "file:///C:/x/a.exe::$DATA",
            "file:///C:/x/CON",
            "file:relatif.pdf",
            "ms-msdt:/id",
            "search-ms:query=x",
            "javascript:alert(1)",
            "https://a.fr/\"x",
        ] {
            assert!(!open_url_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn file_filters_follow_sketchups_syntax() {
        let f = parse_filters("Ruby Files|*.rb;*.rbs|All Files|*.*||");
        assert_eq!(f.len(), 2);
        assert_eq!(f[0], ("Ruby Files".to_string(), vec!["rb".to_string(), "rbs".to_string()]));
        assert!(f[1].1.is_empty(), "*.* : pas de filtre d'extension");
        assert!(parse_filters("").is_empty());
        assert!(parse_filters("seul").is_empty());
    }

    #[test]
    fn timers_fire_once_or_repeat() {
        TIMERS.lock().unwrap().clear();
        EVENTS.lock().unwrap().clear();
        let now = Instant::now();
        {
            let mut t = TIMERS.lock().unwrap();
            t.push(Timer { id: 901, due: now, every: Duration::from_secs(60), repeat: false });
            t.push(Timer { id: 902, due: now, every: Duration::from_secs(60), repeat: true });
            t.push(Timer {
                id: 903,
                due: now + Duration::from_secs(60),
                every: Duration::from_secs(60),
                repeat: false,
            });
        }
        fire_due_timers();
        let ids: Vec<u64> = drain_events().iter().filter_map(|e| e["id"].as_u64()).collect();
        assert_eq!(ids, vec![901, 902]);
        let left: Vec<u64> = TIMERS.lock().unwrap().iter().map(|t| t.id).collect();
        assert_eq!(left, vec![902, 903], "la minuterie à tir unique disparaît, la répétée reste");
        fire_due_timers();
        assert!(drain_events().is_empty(), "rien avant l'échéance suivante");
        TIMERS.lock().unwrap().clear();
    }

    #[test]
    fn plugin_toolbars_start_hidden() {
        assert!(!plugin_toolbar_shown("Barre inconnue"));
    }

    #[test]
    fn notifications_draw_as_toasts() {
        pure("notify", &json!({"id": 7001, "message": "Bonjour", "tooltip": "t", "accept": "Oui", "dismiss": null}))
            .unwrap();
        let ctx = egui::Context::default();
        for _ in 0..3 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| toasts(ui.ctx()));
            out.textures_delta.clear();
        }
        assert!(TOASTS.lock().unwrap().iter().any(|t| t.id == 7001));
        TOASTS.lock().unwrap().retain(|t| t.id != 7001);
    }
}

#[cfg(test)]
mod e2e;
