//! Web dialogs (`UI::HtmlDialog`, `UI::WebDialog`, and the forms behind `UI.inputbox` and
//! `UI::InputBox`) as real windows hosting WebView2 (the Chromium that ships with Windows 11).
//!
//! Why a web view and not an HTML renderer: plugins' dialogs are web pages with JavaScript that
//! talks back to Ruby (`sketchup.callback(...)`, `skp:callback@params`). Only a real browser engine
//! runs them; SketchUp itself embeds Chromium (CEF) for the same reason.
//!
//! The windows live on their own thread with their own event loop (the main thread belongs to
//! eframe). Ruby reaches them through `Cmd`s; what happens in them comes back as events in the
//! shared queue (`super::push_event`) that the app hands to Ruby between frames.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{Value as J, json};
use tao::dpi::{LogicalPosition, LogicalSize};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
use tao::platform::windows::{EventLoopBuilderExtWindows, WindowBuilderExtWindows, WindowExtWindows};
use tao::window::{Window, WindowBuilder, WindowId};
use wry::{PageLoadEvent, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows};

/// Window properties from Ruby (`HtmlDialog.new(...)` / `WebDialog.new(...)`).
#[derive(Clone, Debug)]
pub struct Opts {
    pub title: String,
    pub pref_key: String,
    pub scrollable: bool,
    pub resizable: bool,
    pub use_content_size: bool,
    pub width: i32,
    pub height: i32,
    pub left: Option<i32>,
    pub top: Option<i32>,
    pub min: (i32, i32),
    pub max: (i32, i32),
    /// 0 dialog, 1 window, 2 utility (`HtmlDialog::STYLE_*`).
    pub style: i32,
    pub bg: Option<String>,
}

impl Opts {
    pub fn from_json(j: &J, bg: Option<String>) -> Opts {
        let i = |k: &str, d: i32| j[k].as_i64().map_or(d, |v| v as i32);
        let o = |k: &str| j[k].as_i64().map(|v| v as i32);
        Opts {
            title: j["dialog_title"].as_str().unwrap_or_default().into(),
            pref_key: j["preferences_key"].as_str().unwrap_or_default().into(),
            scrollable: j["scrollable"].as_bool().unwrap_or(true),
            resizable: j["resizable"].as_bool().unwrap_or(true),
            use_content_size: j["use_content_size"].as_bool().unwrap_or(false),
            width: i("width", 250),
            height: i("height", 250),
            left: o("left"),
            top: o("top"),
            min: (i("min_width", -1), i("min_height", -1)),
            max: (i("max_width", -1), i("max_height", -1)),
            style: i("style", 0),
            bg,
        }
    }
}

/// What a window shows.
#[derive(Clone, Debug)]
pub enum Content {
    Html(String),
    Url(String),
}

impl Content {
    pub fn from_json(kind: &str, value: &str) -> Content {
        match kind {
            "html" => Content::Html(value.to_string()),
            "file" => Content::Url(file_url(value)),
            _ => Content::Url(value.to_string()),
        }
    }
}

/// `file:///C:/dir/a%20b.html`
pub fn file_url(path: &str) -> String {
    let p = path.replace('\\', "/");
    if p.contains("://") {
        return p;
    }
    let mut out = String::from("file:///");
    for b in p.trim_start_matches('/').bytes() {
        match b {
            b' ' => out.push_str("%20"),
            b'#' => out.push_str("%23"),
            b'%' => out.push_str("%25"),
            b'?' => out.push_str("%3F"),
            _ => out.push(b as char),
        }
    }
    out
}

pub enum Cmd {
    Open { id: u64, opts: Opts, content: Option<Content>, can_close_hook: bool },
    Load { id: u64, content: Content },
    Show(u64),
    Hide(u64),
    Close { id: u64 },
    Exec { id: u64, js: String },
    Eval { id: u64, js: String, token: u64 },
    Pos { id: u64, x: i32, y: i32 },
    Size { id: u64, w: i32, h: i32, content: bool },
    Front(u64),
    Center(u64),
    Hook { id: u64, can_close: bool },
    Bg { id: u64, color: String },
    Limits { id: u64, min: (i32, i32), max: (i32, i32) },
    Capture { id: u64, path: String, token: u64 },
    Loaded(u64),
}

/// Last known geometry of a window, in logical pixels.
#[derive(Clone, Copy, Debug, Default)]
pub struct Geo {
    pub visible: bool,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub cw: i32,
    pub ch: i32,
}

static GEO: Mutex<Option<HashMap<u64, Geo>>> = Mutex::new(None);
static RESULTS: Mutex<Option<HashMap<u64, Option<String>>>> = Mutex::new(None);
static RESULT_CV: Condvar = Condvar::new();
static PROXY: OnceLock<Result<Mutex<EventLoopProxy<Cmd>>, String>> = OnceLock::new();

pub fn geo(id: u64) -> Option<Geo> {
    GEO.lock().unwrap().as_ref().and_then(|m| m.get(&id).copied())
}

fn set_geo(id: u64, f: impl FnOnce(&mut Geo)) {
    f(GEO.lock().unwrap().get_or_insert_with(HashMap::new).entry(id).or_default());
}

fn put_result(token: u64, v: Option<String>) {
    RESULTS.lock().unwrap().get_or_insert_with(HashMap::new).insert(token, v);
    RESULT_CV.notify_all();
}

/// Waits for the answer to an `Eval`/`Capture`.
pub fn wait_result(token: u64, timeout: Duration) -> Option<String> {
    let end = Instant::now() + timeout;
    let mut g = RESULTS.lock().unwrap();
    loop {
        if let Some(v) = g.as_mut().and_then(|m| m.remove(&token)) {
            return v;
        }
        let left = end.checked_duration_since(Instant::now())?;
        g = RESULT_CV.wait_timeout(g, left).unwrap().0;
    }
}

/// Sends a command, starting the window thread on first use.
pub fn send(cmd: Cmd) -> Result<(), String> {
    let p = PROXY.get_or_init(start_thread);
    match p {
        Ok(p) => p.lock().unwrap().send_event(cmd).map_err(|_| "le fil des fenêtres web s'est arrêté".to_string()),
        Err(e) => Err(e.clone()),
    }
}

fn start_thread() -> Result<Mutex<EventLoopProxy<Cmd>>, String> {
    let (tx, rx) = channel::<Result<EventLoopProxy<Cmd>, String>>();
    std::thread::Builder::new()
        .name("webdialogs".into())
        .spawn(move || run_loop(tx))
        .map_err(|e| format!("fil des fenêtres web: {e}"))?;
    rx.recv().map_err(|_| "fil des fenêtres web arrêté".to_string())?.map(Mutex::new)
}

struct Entry {
    window: Window,
    view: WebView,
    loaded: bool,
    pending: Vec<String>,
    can_close_hook: bool,
    pref_key: String,
}

fn data_dir() -> PathBuf {
    crate::settings::user_dir().unwrap_or_else(|| std::env::temp_dir().join("Esquisse")).join("WebView2")
}

fn prefs_file() -> PathBuf {
    data_dir().with_file_name("dialogs.json")
}

fn load_prefs(key: &str) -> Option<(i32, i32, i32, i32)> {
    let j: J = serde_json::from_str(&std::fs::read_to_string(prefs_file()).ok()?).ok()?;
    let e = &j[key];
    Some((e["x"].as_i64()? as i32, e["y"].as_i64()? as i32, e["w"].as_i64()? as i32, e["h"].as_i64()? as i32))
}

fn save_prefs(key: &str, g: Geo) {
    let mut j: J =
        std::fs::read_to_string(prefs_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
    j[key] = json!({"x": g.x, "y": g.y, "w": g.w, "h": g.h});
    let _ = std::fs::create_dir_all(prefs_file().parent().unwrap_or(&data_dir()));
    let _ = std::fs::write(prefs_file(), j.to_string());
}

/// JavaScript every page gets: the `sketchup` object (promise-returning callbacks to Ruby) and
/// the hooks the host uses to settle those promises.
const INIT_JS: &str = r#"(function(){
 if(window.__esq)return;
 window.__esq={n:0,p:{}};
 window.__esq_settle=function(i,ok,v){var p=window.__esq.p[i];if(!p)return;delete window.__esq.p[i];ok?p.res(v):p.rej(v);};
 window.sketchup=new Proxy({},{get:function(t,name){
   if(typeof name!=='string')return undefined;
   return function(){var a=Array.prototype.slice.call(arguments);
     return new Promise(function(res,rej){var i=++window.__esq.n;window.__esq.p[i]={res:res,rej:rej};
       window.ipc.postMessage(JSON.stringify({n:name,a:a,i:i}));});};}});
})();"#;

fn init_script(scrollable: bool) -> String {
    let mut s = INIT_JS.to_string();
    if !scrollable {
        s += "document.addEventListener('DOMContentLoaded',function(){document.documentElement.style.overflow='hidden'});";
    }
    s
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `skp:name@params` (the WebDialog way of calling Ruby by navigating).
fn skp_event(id: u64, url: &str) -> Option<J> {
    let rest = url.strip_prefix("skp:")?;
    let (name, params) = rest.split_once('@').unwrap_or((rest, ""));
    Some(json!({"t": "cb", "d": id, "n": percent_decode(name), "p": percent_decode(params)}))
}

fn run_loop(ready: Sender<Result<EventLoopProxy<Cmd>, String>>) {
    let mut builder = EventLoopBuilder::<Cmd>::with_user_event();
    builder.with_any_thread(true);
    let event_loop = builder.build();
    let _ = ready.send(Ok(event_loop.create_proxy()));
    let proxy = event_loop.create_proxy();
    let mut ctx = WebContext::new(Some(data_dir()));
    let mut wins: HashMap<u64, Entry> = HashMap::new();
    event_loop.run(move |event, target, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(cmd) => handle(cmd, target, &proxy, &mut ctx, &mut wins),
            Event::WindowEvent { window_id, event, .. } => {
                let Some(id) = find(&wins, window_id) else { return };
                match event {
                    WindowEvent::CloseRequested => {
                        if wins[&id].can_close_hook {
                            super::push_event(json!({"t": "close_request", "d": id}));
                        } else {
                            close(&mut wins, id);
                        }
                    }
                    WindowEvent::Moved(_) | WindowEvent::Resized(_) => refresh_geo(id, &wins[&id]),
                    _ => {}
                }
            }
            _ => {}
        }
    });
}

fn find(wins: &HashMap<u64, Entry>, wid: WindowId) -> Option<u64> {
    wins.iter().find(|(_, e)| e.window.id() == wid).map(|(id, _)| *id)
}

fn refresh_geo(id: u64, e: &Entry) {
    let s = e.window.scale_factor();
    let pos = e.window.outer_position().map(|p| p.to_logical::<f64>(s)).unwrap_or(LogicalPosition::new(0.0, 0.0));
    let (o, i) = (e.window.outer_size().to_logical::<f64>(s), e.window.inner_size().to_logical::<f64>(s));
    set_geo(id, |g| {
        g.visible = e.window.is_visible();
        g.x = pos.x.round() as i32;
        g.y = pos.y.round() as i32;
        g.w = o.width.round() as i32;
        g.h = o.height.round() as i32;
        g.cw = i.width.round() as i32;
        g.ch = i.height.round() as i32;
    });
}

fn close(wins: &mut HashMap<u64, Entry>, id: u64) {
    if let Some(e) = wins.remove(&id) {
        refresh_geo(id, &e);
        if !e.pref_key.is_empty()
            && let Some(g) = geo(id)
        {
            save_prefs(&e.pref_key, g);
        }
        drop(e);
        set_geo(id, |g| g.visible = false);
        super::push_event(json!({"t": "closed", "d": id}));
    }
}

fn set_content(e: &mut Entry, c: &Content) {
    e.loaded = false;
    let _ = match c {
        Content::Html(h) => e.view.load_html(h),
        Content::Url(u) => e.view.load_url(u),
    };
}

fn handle(
    cmd: Cmd,
    target: &EventLoopWindowTarget<Cmd>,
    proxy: &EventLoopProxy<Cmd>,
    ctx: &mut WebContext,
    wins: &mut HashMap<u64, Entry>,
) {
    match cmd {
        Cmd::Open { id, opts, content, can_close_hook } => {
            if wins.contains_key(&id) {
                return;
            }
            match open(id, &opts, content.as_ref(), can_close_hook, target, proxy, ctx) {
                Ok(e) => {
                    refresh_geo(id, &e);
                    wins.insert(id, e);
                }
                Err(err) => {
                    super::push_event(json!({"t": "error", "d": id, "msg": err}));
                    super::push_event(json!({"t": "closed", "d": id}));
                }
            }
        }
        Cmd::Load { id, content } => {
            if let Some(e) = wins.get_mut(&id) {
                set_content(e, &content);
            }
        }
        Cmd::Show(id) => {
            if let Some(e) = wins.get(&id) {
                e.window.set_visible(true);
                refresh_geo(id, e);
            }
        }
        Cmd::Hide(id) => {
            if let Some(e) = wins.get(&id) {
                e.window.set_visible(false);
                refresh_geo(id, e);
            }
        }
        Cmd::Close { id } => close(wins, id),
        Cmd::Exec { id, js } => {
            if let Some(e) = wins.get_mut(&id) {
                if e.loaded {
                    let _ = e.view.evaluate_script(&js);
                } else {
                    e.pending.push(js);
                }
            }
        }
        Cmd::Eval { id, js, token } => match wins.get(&id) {
            Some(e) => {
                let _ = e.view.evaluate_script_with_callback(&js, move |s| put_result(token, Some(s)));
            }
            None => put_result(token, None),
        },
        Cmd::Pos { id, x, y } => {
            if let Some(e) = wins.get(&id) {
                e.window.set_outer_position(LogicalPosition::new(x as f64, y as f64));
                refresh_geo(id, e);
            }
        }
        Cmd::Size { id, w, h, content } => {
            if let Some(e) = wins.get(&id) {
                apply_size(&e.window, w, h, content);
                refresh_geo(id, e);
            }
        }
        Cmd::Front(id) => {
            if let Some(e) = wins.get(&id) {
                e.window.set_visible(true);
                e.window.set_focus();
            }
        }
        Cmd::Center(id) => {
            if let Some(e) = wins.get(&id) {
                center(&e.window);
                refresh_geo(id, e);
            }
        }
        Cmd::Hook { id, can_close } => {
            if let Some(e) = wins.get_mut(&id) {
                e.can_close_hook = can_close;
            }
        }
        Cmd::Bg { id, color } => {
            if let (Some(e), Some(c)) = (wins.get(&id), parse_color(&color)) {
                let _ = e.view.set_background_color(c);
            }
        }
        Cmd::Limits { id, min, max } => {
            if let Some(e) = wins.get(&id) {
                apply_limits(&e.window, min, max);
            }
        }
        Cmd::Capture { id, path, token } => match wins.get(&id) {
            Some(e) => put_result(token, capture(&e.window, &path).then(|| "ok".to_string())),
            None => put_result(token, None),
        },
        Cmd::Loaded(id) => {
            if let Some(e) = wins.get_mut(&id) {
                e.loaded = true;
                for js in std::mem::take(&mut e.pending) {
                    let _ = e.view.evaluate_script(&js);
                }
            }
        }
    }
}

fn parse_color(s: &str) -> Option<(u8, u8, u8, u8)> {
    let h = s.trim().trim_start_matches('#');
    // Byte slices: "#aé1234" cut a character in two (a panic that stopped every web dialog).
    let v = |i: usize| u8::from_str_radix(std::str::from_utf8(h.as_bytes().get(i..i + 2)?).ok()?, 16).ok();
    Some((v(0)?, v(2)?, v(4)?, 255))
}

/// Whether a dialog may go to `url` (a page's link, `location = …`, `window.open`): not to a
/// network share (`file://serveur/…`), where Windows would send the user's NTLM hash.
fn navigation_allowed(url: &str) -> bool {
    let remote = esq_core::untrusted::is_remote(url) || esq_core::untrusted::is_remote(&percent_decode(url));
    if remote {
        esq_core::untrusted::note_refused(url);
    }
    !remote
}

fn apply_limits(w: &Window, min: (i32, i32), max: (i32, i32)) {
    w.set_min_inner_size((min.0 > 0 || min.1 > 0).then(|| LogicalSize::new(min.0.max(0) as f64, min.1.max(0) as f64)));
    w.set_max_inner_size((max.0 > 0 || max.1 > 0).then(|| {
        LogicalSize::new(if max.0 > 0 { max.0 as f64 } else { 1.0e5 }, if max.1 > 0 { max.1 as f64 } else { 1.0e5 })
    }));
}

/// Sets the outer size, or the content size when `content`.
fn apply_size(w: &Window, width: i32, height: i32, content: bool) {
    let s = w.scale_factor();
    if content {
        w.set_inner_size(LogicalSize::new(width as f64, height as f64));
    } else {
        let frame = (
            w.outer_size().to_logical::<f64>(s).width - w.inner_size().to_logical::<f64>(s).width,
            w.outer_size().to_logical::<f64>(s).height - w.inner_size().to_logical::<f64>(s).height,
        );
        w.set_inner_size(LogicalSize::new((width as f64 - frame.0).max(1.0), (height as f64 - frame.1).max(1.0)));
    }
}

fn center(w: &Window) {
    if let Some(m) = w.current_monitor().or_else(|| w.primary_monitor()) {
        let s = m.scale_factor();
        let (ms, mp) = (m.size().to_logical::<f64>(s), m.position().to_logical::<f64>(s));
        let ws = w.outer_size().to_logical::<f64>(w.scale_factor());
        w.set_outer_position(LogicalPosition::new(
            mp.x + (ms.width - ws.width) / 2.0,
            mp.y + (ms.height - ws.height) / 3.0,
        ));
    }
}

fn capture(w: &Window, path: &str) -> bool {
    let Some((pw, ph, px)) = super::win::capture_window(w.hwnd()) else { return false };
    let Some(img) = image::RgbaImage::from_raw(pw, ph, px) else { return false };
    let ext = path.rsplit('.').next().unwrap_or("png").to_ascii_lowercase();
    if ext == "jpg" || ext == "jpeg" {
        image::DynamicImage::ImageRgba8(img).to_rgb8().save(path).is_ok()
    } else {
        img.save_with_format(path, image::ImageFormat::Png).is_ok()
    }
}

fn open(
    id: u64,
    o: &Opts,
    content: Option<&Content>,
    can_close_hook: bool,
    target: &EventLoopWindowTarget<Cmd>,
    proxy: &EventLoopProxy<Cmd>,
    ctx: &mut WebContext,
) -> Result<Entry, String> {
    let saved = (!o.pref_key.is_empty()).then(|| load_prefs(&o.pref_key)).flatten();
    let (w, h) = saved.map_or((o.width, o.height), |s| (s.2, s.3));
    let mut wb = WindowBuilder::new()
        // tao and winit (the app's window) both default to the class name "Window Class": the
        // second to register would get the first one's window procedure and crash on creation.
        .with_window_classname("EsquisseWebDialog")
        .with_theme(Some(tao::window::Theme::Light))
        .with_title(o.title.clone())
        .with_inner_size(LogicalSize::new(w.max(1) as f64, h.max(1) as f64))
        .with_resizable(o.resizable)
        .with_visible(false);
    // Not owned by the app's window: an owned window on another thread shares its input queue,
    // so a script blocked in `show_modal` (the app's thread waiting) would freeze the dialog too,
    // and Windows ends the app when the owner cannot answer. Dialog and utility styles float
    // above instead, like SketchUp's.
    if o.style == 0 {
        wb = wb.with_minimizable(false).with_maximizable(false).with_always_on_top(true);
    }
    if o.style == 2 {
        wb = wb.with_skip_taskbar(true).with_always_on_top(true);
    }
    let window = wb.build(target).map_err(|e| format!("fenêtre: {e}"))?;
    apply_limits(&window, o.min, o.max);
    if !o.use_content_size && saved.is_none() {
        apply_size(&window, o.width, o.height, false);
    }
    match (saved, o.left, o.top) {
        (Some((x, y, _, _)), _, _) => window.set_outer_position(LogicalPosition::new(x as f64, y as f64)),
        (None, Some(x), Some(y)) => window.set_outer_position(LogicalPosition::new(x as f64, y as f64)),
        _ => center(&window),
    }
    let p_load = proxy.clone();
    let mut wvb = WebViewBuilder::new_with_web_context(ctx)
        .with_theme(wry::Theme::Light)
        .with_initialization_script(init_script(o.scrollable))
        .with_devtools(true)
        .with_ipc_handler(move |req| {
            let Ok(m) = serde_json::from_str::<J>(req.body()) else { return };
            let Some(name) = m["n"].as_str() else { return };
            let mut ev = json!({"t": "cb", "d": id, "n": name, "a": m["a"]});
            if let Some(i) = m["i"].as_i64() {
                ev["i"] = json!(i);
            }
            super::push_event(ev);
        })
        .with_navigation_handler(move |url| match skp_event(id, &url) {
            Some(ev) => {
                super::push_event(ev);
                false
            }
            None => navigation_allowed(&url),
        })
        .with_new_window_req_handler(|url, _| {
            if navigation_allowed(&url) { wry::NewWindowResponse::Allow } else { wry::NewWindowResponse::Deny }
        })
        .with_on_page_load_handler(move |ev, _url| {
            if matches!(ev, PageLoadEvent::Finished) {
                let _ = p_load.send_event(Cmd::Loaded(id));
            }
        });
    // White by default (pages without a background are drawn on white, as in SketchUp's CEF).
    wvb = wvb.with_background_color(o.bg.as_deref().and_then(parse_color).unwrap_or((255, 255, 255, 255)));
    wvb = match content {
        Some(Content::Html(h)) => wvb.with_html(h.clone()),
        Some(Content::Url(u)) => wvb.with_url(u.clone()),
        None => wvb.with_html(""),
    };
    let view = wvb.build(&window).map_err(|e| format!("WebView2: {e}"))?;
    window.set_visible(true);
    Ok(Entry { window, view, loaded: false, pending: Vec::new(), can_close_hook, pref_key: o.pref_key.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_are_escaped() {
        assert_eq!(file_url("C:\\a b\\c#d.html"), "file:///C:/a%20b/c%23d.html");
        assert_eq!(file_url("https://x.y/z"), "https://x.y/z");
    }

    #[test]
    fn skp_urls_become_callbacks() {
        let e = skp_event(3, "skp:ping@a%20b%26c").unwrap();
        assert_eq!(e["n"], "ping");
        assert_eq!(e["p"], "a b&c");
        assert_eq!(e["d"], 3);
        assert!(skp_event(3, "https://example.com").is_none());
        assert_eq!(skp_event(3, "skp:bare").unwrap()["p"], "");
    }

    #[test]
    fn options_have_sketchups_defaults() {
        let o = Opts::from_json(&json!({}), None);
        assert!(o.scrollable && o.resizable && !o.use_content_size);
        assert_eq!((o.width, o.height, o.style), (250, 250, 0));
        let o = Opts::from_json(&json!({"width": 400, "left": 5, "style": 2, "scrollable": false}), None);
        assert_eq!((o.width, o.left, o.style, o.scrollable), (400, Some(5), 2, false));
    }

    #[test]
    fn colors_parse() {
        assert_eq!(parse_color("#102030"), Some((16, 32, 48, 255)));
        assert_eq!(parse_color("zz"), None);
        // A character cut in two by the old slicing: a panic of the dialogs' thread.
        assert_eq!(parse_color("#aé1234"), None);
        assert_eq!(parse_color("#12é456"), None);
        assert_eq!(parse_color("#1234é"), None);
    }

    /// A page of a dialog (a link, `location`, `window.open`) could go to a network share.
    #[test]
    fn dialogs_never_go_to_network_shares() {
        for ok in ["https://exemple.org", "file:///C:/ext/page.html", "about:blank", "data:text/html,x"] {
            assert!(navigation_allowed(ok), "{ok}");
        }
        for bad in [
            "file://esq-test-web/partage/page.html",
            "file:////esq-test-web/partage/page.html",
            "file:///%5C%5Cesq-test-web/partage/page.html",
            r"\\esq-test-web\partage\page.html",
        ] {
            assert!(!navigation_allowed(bad), "{bad}");
        }
    }
}
