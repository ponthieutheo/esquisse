//! Window › Preferences: what Esquisse remembers for the user rather than for the model
//! (SketchUp keeps the same things in its `PrivatePreferences.json`), saved in
//! `%APPDATA%\Esquisse\preferences.json`. The toolbar settings stay in `toolbars.rs` and the
//! default template in `prefs.rs`: one place for each setting.
//!
//! Tools, the renderer and the viewport read the current values with [`get`]; the
//! Preferences window changes them with [`set`], which saves them.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Preferences › Drawing › Click Style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClickStyle {
    ClickDragRelease,
    AutoDetect,
    ClickMoveClick,
}

impl ClickStyle {
    pub const ALL: [ClickStyle; 3] = [ClickStyle::ClickDragRelease, ClickStyle::AutoDetect, ClickStyle::ClickMoveClick];

    pub fn label(self) -> &'static str {
        match self {
            ClickStyle::ClickDragRelease => "Cliquer-glisser-relâcher",
            ClickStyle::AutoDetect => "Détection automatique",
            ClickStyle::ClickMoveClick => "Cliquer-déplacer-cliquer",
        }
    }
}

/// Where a tray sits (SketchUp docks its trays on a side of the window, or lets them float).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dock {
    #[default]
    Right,
    Left,
    Floating,
}

/// A tray of panels (Window › New Tray, Manage Trays): its name, its panels (by
/// `commands::Panel` name, in order), whether it shows, where it sits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TraySpec {
    pub name: String,
    pub panels: Vec<String>,
    pub visible: bool,
    pub dock: Dock,
}

/// Preferences › Files: the folders SketchUp proposes for each kind of file.
pub const LOCATIONS: [(&str, &str); 11] = [
    ("models", "Modèles :"),
    ("components", "Composants :"),
    ("materials", "Matières :"),
    ("styles", "Styles :"),
    ("textures", "Images de texture :"),
    ("watermarks", "Images de filigrane :"),
    ("export", "Exporter :"),
    ("classifications", "Classements :"),
    ("templates", "Modèles types :"),
    ("environments", "Environnements :"),
    ("workspaces", "Espaces de travail :"),
];

/// Default axis colours (red, green, blue) and inference colours (parallel/perpendicular,
/// tangent): Preferences › Accessibility › Reset All brings them back.
pub const AXIS_COLORS: [[u8; 3]; 3] = [[220, 30, 30], [20, 150, 40], [30, 50, 230]];
pub const PARALLEL_COLOR: [u8; 3] = [200, 0, 200];
pub const TANGENT_COLOR: [u8; 3] = [0, 190, 210];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // General › Saving
    pub create_backup: bool,
    pub autosave: bool,
    pub autosave_minutes: u32,
    pub purge_on_save: bool,
    // General › Check for problems
    pub auto_fix: bool,
    pub notify_fix: bool,
    // General › Messages: warnings the user asked not to see again.
    pub dismissed: BTreeSet<String>,
    pub urgent_only: bool,
    // General › Software updates (a Trimble service: kept, never used).
    pub check_updates: bool,
    // General › Welcome window
    pub show_welcome: bool,
    // Accessibility
    pub axis_colors: [[u8; 3]; 3],
    pub parallel_color: [u8; 3],
    pub tangent_color: [u8; 3],
    // Compatibility
    pub bbox_highlight: bool,
    pub invert_wheel: bool,
    // Drawing
    pub click_style: ClickStyle,
    pub continue_lines: bool,
    /// Lasso direction: true = drawn clockwise, the lasso takes what it crosses.
    pub lasso_cw_crossing: bool,
    pub crosshairs: bool,
    pub no_prepick_pushpull: bool,
    pub rotation_grips: bool,
    /// New edges split those they cross (`Preferences/IntersectEdges`, set by Ruby's
    /// `Sketchup.break_edges=`; no page of SketchUp's Preferences shows it).
    pub intersect_edges: bool,
    // Files: kind (see `LOCATIONS`) → folder.
    pub locations: BTreeMap<String, String>,
    // Graphics
    /// Multisample anti-aliasing: 0 (off), 2, 4, 8 or 16 samples.
    pub msaa: u32,
    pub fast_feedback: bool,
    pub max_texture: bool,
    pub faster_shadows: bool,
    /// Automatically Enhance Materials: metalness and roughness guessed for the photoreal style.
    pub enhance_metalness: bool,
    pub enhance_roughness: bool,
    // Applications
    pub image_editor: String,
    /// Shortcuts: command key (see `commands::Cmd::key`) → its shortcuts ("Ctrl+Maj+K"…).
    /// A command absent from the map has SketchUp's default.
    pub shortcuts: BTreeMap<String, Vec<String>>,
    /// Window › Default Tray: its panels and whether it shows (None: as installed).
    pub default_tray: Option<TraySpec>,
    /// The trays the user made (Window › New Tray).
    pub trays: Vec<TraySpec>,
    /// ID of the workspace last applied (Window › Workspace).
    pub workspace: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            create_backup: true,
            autosave: true,
            autosave_minutes: 5,
            purge_on_save: false,
            auto_fix: true,
            notify_fix: false,
            dismissed: BTreeSet::new(),
            urgent_only: false,
            check_updates: true,
            show_welcome: true,
            axis_colors: AXIS_COLORS,
            parallel_color: PARALLEL_COLOR,
            tangent_color: TANGENT_COLOR,
            bbox_highlight: false,
            invert_wheel: false,
            click_style: ClickStyle::AutoDetect,
            continue_lines: true,
            lasso_cw_crossing: false,
            crosshairs: false,
            no_prepick_pushpull: false,
            rotation_grips: true,
            intersect_edges: true,
            locations: BTreeMap::new(),
            msaa: 4,
            fast_feedback: true,
            max_texture: false,
            faster_shadows: true,
            enhance_metalness: true,
            enhance_roughness: true,
            image_editor: String::new(),
            shortcuts: BTreeMap::new(),
            default_tray: None,
            trays: Vec::new(),
            workspace: String::new(),
        }
    }
}

/// The groups of settings Preferences › Import/Export offers (SketchUp's list).
pub const EXPORT_GROUPS: [&str; 6] = ["Dessin", "Fichiers", "Général", "Graphiques", "Raccourcis", "Modèle type"];

impl Settings {
    /// The folder of a kind of file (Preferences › Files), if one was chosen and exists.
    pub fn location(&self, kind: &str) -> Option<PathBuf> {
        self.locations.get(kind).map(PathBuf::from).filter(|p| p.is_dir())
    }

    /// Settings of the chosen groups as JSON (Preferences › Export).
    pub fn export_json(&self, groups: &[&str]) -> serde_json::Value {
        let all = serde_json::to_value(self).unwrap_or_default();
        let mut out = serde_json::Map::new();
        for g in groups {
            for k in group_keys(g) {
                if let Some(v) = all.get(*k) {
                    out.insert(k.to_string(), v.clone());
                }
            }
            if *g == "Modèle type" {
                out.insert("default_template".into(), crate::prefs::default_template().name.into());
            }
        }
        serde_json::Value::Object(out)
    }

    /// Takes the settings of the chosen groups from an exported file (Preferences › Import).
    pub fn import_json(&mut self, v: &serde_json::Value, groups: &[&str]) {
        let mut all = serde_json::to_value(&*self).unwrap_or_default();
        for g in groups {
            for k in group_keys(g) {
                if let Some(x) = v.get(*k) {
                    all[*k] = x.clone();
                }
            }
            if *g == "Modèle type"
                && let Some(name) = v.get("default_template").and_then(|n| n.as_str())
                && let Some(t) = crate::prefs::all_templates().into_iter().find(|t| t.name == name)
            {
                crate::prefs::set_default_template(&t);
            }
        }
        if let Ok(s) = serde_json::from_value(all) {
            *self = s;
        }
    }
}

fn group_keys(group: &str) -> &'static [&'static str] {
    match group {
        "Dessin" => &[
            "click_style",
            "continue_lines",
            "lasso_cw_crossing",
            "crosshairs",
            "no_prepick_pushpull",
            "rotation_grips",
        ],
        "Fichiers" => &["locations"],
        "Général" => &[
            "create_backup",
            "autosave",
            "autosave_minutes",
            "purge_on_save",
            "auto_fix",
            "notify_fix",
            "urgent_only",
            "check_updates",
            "show_welcome",
            "image_editor",
            "axis_colors",
            "parallel_color",
            "tangent_color",
            "bbox_highlight",
            "invert_wheel",
        ],
        "Graphiques" => {
            &["msaa", "fast_feedback", "max_texture", "faster_shadows", "enhance_metalness", "enhance_roughness"]
        }
        "Raccourcis" => &["shortcuts"],
        _ => &[],
    }
}

/// `%APPDATA%\Esquisse`, where the user's files live (a folder of their own in tests, so
/// that tests never touch the user's settings).
pub fn user_dir() -> Option<PathBuf> {
    let base = if cfg!(test) {
        // The folders of earlier test runs (a WebView2 profile each) go once a day has passed.
        static SWEEP: std::sync::Once = std::sync::Once::new();
        SWEEP.call_once(|| {
            let day = std::time::Duration::from_secs(24 * 3600);
            for e in std::fs::read_dir(std::env::temp_dir()).into_iter().flatten().flatten() {
                let old = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|a| a > day);
                if old && e.file_name().to_string_lossy().starts_with("esq-tests-") {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        });
        std::env::temp_dir().join(format!("esq-tests-{}", std::process::id()))
    } else {
        std::env::var_os("APPDATA").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)?
    };
    let d = base.join("Esquisse");
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

fn file() -> Option<PathBuf> {
    Some(user_dir()?.join("preferences.json"))
}

fn load() -> Settings {
    if cfg!(test) {
        return Settings::default();
    }
    file().and_then(|f| std::fs::read(f).ok()).and_then(|b| parse(&b)).unwrap_or_default()
}

/// Reads a preferences file (written by hand, it may start with a UTF-8 byte order mark).
pub fn parse(bytes: &[u8]) -> Option<Settings> {
    serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)).ok()
}

/// The preferences of the running application: one copy for every thread (the Ruby
/// interpreter has its own thread and reads them too).
static GLOBAL: std::sync::RwLock<Option<Settings>> = std::sync::RwLock::new(None);

thread_local! {
    /// Tests run side by side in one process: each test thread changes its own copy.
    static LOCAL: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

/// Reads one value without cloning everything.
pub fn with<R>(f: impl FnOnce(&Settings) -> R) -> R {
    let mut f = Some(f);
    if cfg!(test)
        && let Some(r) = LOCAL.with(|c| c.borrow().as_ref().map(|s| f.take().expect("once")(s)))
    {
        return r;
    }
    let f = f.expect("once");
    if let Some(s) = GLOBAL.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return f(s);
    }
    let mut g = GLOBAL.write().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(load))
}

/// The current preferences (read from disk the first time; defaults in tests).
pub fn get() -> Settings {
    with(Settings::clone)
}

/// Changes the preferences and saves them (not in tests).
pub fn set(s: Settings) {
    if cfg!(test) {
        LOCAL.with(|c| *c.borrow_mut() = Some(s));
        return;
    }
    if let Some(f) = file()
        && let Ok(b) = serde_json::to_vec_pretty(&s)
    {
        let _ = std::fs::write(f, b);
    }
    *GLOBAL.write().unwrap_or_else(|e| e.into_inner()) = Some(s);
}

/// Tests: makes this thread's preferences those of every thread (the Ruby thread's).
#[cfg(test)]
pub fn share_for_test() {
    let s = get();
    *GLOBAL.write().unwrap_or_else(|e| e.into_inner()) = Some(s);
}

/// Changes some preferences.
pub fn update(f: impl FnOnce(&mut Settings)) {
    let mut s = get();
    f(&mut s);
    set(s);
}

// ------------------------------------------------------------------------------------------
// What the other modules ask.

pub fn axis_color(i: usize) -> [u8; 3] {
    with(|s| s.axis_colors[i.min(2)])
}

pub fn parallel_color() -> [u8; 3] {
    with(|s| s.parallel_color)
}

pub fn tangent_color() -> [u8; 3] {
    with(|s| s.tangent_color)
}

/// The zoom factor of a mouse wheel turn (< 1 brings the camera closer): forward zooms in,
/// or out with Preferences › Compatibility › Mouse wheel › Invert.
pub fn wheel_zoom_factor(scroll: f64) -> f64 {
    let sign = if with(|s| s.invert_wheel) { 1.0 } else { -1.0 };
    (sign * scroll * 0.0025).exp()
}

pub fn click_style() -> ClickStyle {
    with(|s| s.click_style)
}

/// General › Create backup: before a file is overwritten, the previous version is kept next
/// to it as `name~.ext` (SketchUp's `.skb` backup).
pub fn backup_before_save(path: &Path) {
    if with(|s| s.create_backup) && path.is_file() {
        let _ = std::fs::copy(path, backup_path(path));
    }
}

pub fn backup_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!("{stem}~.{ext}"))
}

/// The command that opens an image for editing: Preferences › Applications › Default Image
/// Editor, else the program Windows associates with the file.
pub fn image_editor_command(path: &Path) -> std::process::Command {
    let editor = with(|s| s.image_editor.clone());
    if !editor.is_empty() && Path::new(&editor).is_file() {
        let mut c = std::process::Command::new(editor);
        c.arg(path);
        c
    } else {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]).arg(path);
        c
    }
}

/// Opens an image in the image editor (Materials › Edit texture image in external editor).
pub fn open_image(path: &Path) {
    let _ = image_editor_command(path).spawn();
}

/// Starts a file dialog in the folder Preferences › Files gives for that kind of file.
pub fn dialog(kind: &str) -> rfd::FileDialog {
    let d = rfd::FileDialog::new();
    match with(|s| s.location(kind)) {
        Some(dir) => d.set_directory(dir),
        None => d,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_a_round_trip_and_unknown_fields() {
        let s = Settings { autosave_minutes: 12, click_style: ClickStyle::ClickMoveClick, ..Default::default() };
        let back: Settings = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        // Older or partial files keep the defaults for what they lack.
        let partial: Settings = serde_json::from_str(r#"{"autosave": false, "inconnu": 3}"#).unwrap();
        assert!(!partial.autosave && partial.create_backup && partial.msaa == 4);
    }

    #[test]
    fn export_import_only_moves_the_chosen_groups() {
        let mut a = Settings { crosshairs: true, msaa: 8, ..Default::default() };
        a.shortcuts.insert("Tool(Line)".into(), vec!["K".into()]);
        let j = a.export_json(&["Dessin", "Raccourcis"]);
        assert!(j.get("msaa").is_none() && j["crosshairs"] == true);
        let mut b = Settings::default();
        b.import_json(&j, &["Dessin"]);
        assert!(b.crosshairs && b.msaa == 4 && b.shortcuts.is_empty(), "raccourcis non demandés");
        b.import_json(&j, &["Raccourcis"]);
        assert_eq!(b.shortcuts["Tool(Line)"], vec!["K".to_string()]);
    }

    #[test]
    fn backup_names() {
        assert_eq!(backup_path(Path::new(r"C:\x\maison.esq")), PathBuf::from(r"C:\x\maison~.esq"));
    }
}
