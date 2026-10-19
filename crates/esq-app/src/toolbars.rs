//! View › Toolbars: the tool bars (which ones are shown, the icon size, text or icons only,
//! tooltips), remembered between sessions in `%APPDATA%\Esquisse\toolbars.txt`. The settings
//! follow SketchUp's "Toolbar Settings" (show tooltips, use large icons, display mode).

use std::collections::BTreeSet;
use std::path::PathBuf;

use eframe::egui::{self, Color32, Sense, Stroke};

use crate::app::EsquisseApp;
use crate::commands::Cmd;
use crate::icons;
use crate::tools::ToolId;
use crate::ui::centered;

/// What a toolbar button does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Item {
    Tool(ToolId),
    ZoomExtents,
}

/// The toolbars, grouped like SketchUp's "Large Tool Set".
pub const TOOLBARS: &[(&str, &[Item])] = &[
    (
        "Principale",
        &[
            Item::Tool(ToolId::Select),
            Item::Tool(ToolId::Lasso),
            Item::Tool(ToolId::Eraser),
            Item::Tool(ToolId::Paint),
            Item::Tool(ToolId::TagTool),
        ],
    ),
    (
        "Dessin",
        &[
            Item::Tool(ToolId::Line),
            Item::Tool(ToolId::Freehand),
            Item::Tool(ToolId::ArcCenter),
            Item::Tool(ToolId::Arc),
            Item::Tool(ToolId::Arc3),
            Item::Tool(ToolId::Pie),
            Item::Tool(ToolId::Rect),
            Item::Tool(ToolId::RotRect),
            Item::Tool(ToolId::Circle),
            Item::Tool(ToolId::Polygon),
        ],
    ),
    (
        "Modification",
        &[
            Item::Tool(ToolId::PushPull),
            Item::Tool(ToolId::FollowMe),
            Item::Tool(ToolId::Offset),
            Item::Tool(ToolId::Move),
            Item::Tool(ToolId::Rotate),
            Item::Tool(ToolId::Scale),
            Item::Tool(ToolId::Flip),
        ],
    ),
    (
        "Construction",
        &[
            Item::Tool(ToolId::Tape),
            Item::Tool(ToolId::Dimension),
            Item::Tool(ToolId::Protractor),
            Item::Tool(ToolId::Text),
            Item::Tool(ToolId::AxesTool),
            Item::Tool(ToolId::SectionPlane),
        ],
    ),
    (
        "Caméra",
        &[
            Item::Tool(ToolId::Orbit),
            Item::Tool(ToolId::Pan),
            Item::Tool(ToolId::Zoom),
            Item::Tool(ToolId::ZoomWindow),
            Item::ZoomExtents,
        ],
    ),
    ("Visite", &[Item::Tool(ToolId::PositionCamera), Item::Tool(ToolId::Walk), Item::Tool(ToolId::LookAround)]),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayMode {
    IconOnly,
    TextAndIcon,
    TextOnly,
}

impl DisplayMode {
    pub const ALL: [DisplayMode; 3] = [DisplayMode::IconOnly, DisplayMode::TextAndIcon, DisplayMode::TextOnly];

    pub fn label(self) -> &'static str {
        match self {
            DisplayMode::IconOnly => "Icônes seules",
            DisplayMode::TextAndIcon => "Texte et icône",
            DisplayMode::TextOnly => "Texte seul",
        }
    }

    fn key(self) -> &'static str {
        match self {
            DisplayMode::IconOnly => "icons",
            DisplayMode::TextAndIcon => "both",
            DisplayMode::TextOnly => "text",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolbarConfig {
    /// Names (from `TOOLBARS`) of the toolbars that are hidden.
    pub hidden: BTreeSet<String>,
    pub large_icons: bool,
    pub tooltips: bool,
    pub mode: DisplayMode,
    /// The settings window is open.
    pub open: bool,
    /// Read from disk (done on first use, not at construction).
    loaded: bool,
}

impl Default for ToolbarConfig {
    fn default() -> Self {
        ToolbarConfig {
            hidden: BTreeSet::new(),
            large_icons: true,
            tooltips: true,
            mode: DisplayMode::IconOnly,
            open: false,
            loaded: false,
        }
    }
}

impl ToolbarConfig {
    pub fn icon_size(&self) -> f32 {
        if self.large_icons { 32.0 } else { 24.0 }
    }

    pub fn shown(&self, name: &str) -> bool {
        !self.hidden.contains(name)
    }

    pub fn set_shown(&mut self, name: &str, shown: bool) {
        if shown {
            self.hidden.remove(name);
        } else {
            self.hidden.insert(name.to_string());
        }
    }

    pub fn to_text(&self) -> String {
        let mut s = format!(
            "large_icons={}\ntooltips={}\nmode={}\n",
            self.large_icons as u8,
            self.tooltips as u8,
            self.mode.key()
        );
        for h in &self.hidden {
            s += &format!("hidden={h}\n");
        }
        s
    }

    pub fn from_text(text: &str) -> ToolbarConfig {
        let mut c = ToolbarConfig { loaded: true, ..Default::default() };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            match k.trim() {
                "large_icons" => c.large_icons = v.trim() != "0",
                "tooltips" => c.tooltips = v.trim() != "0",
                "mode" => {
                    if let Some(m) = DisplayMode::ALL.into_iter().find(|m| m.key() == v.trim()) {
                        c.mode = m;
                    }
                }
                "hidden" if TOOLBARS.iter().any(|(n, _)| *n == v.trim()) => {
                    c.hidden.insert(v.trim().to_string());
                }
                _ => {}
            }
        }
        c
    }
}

fn file() -> Option<PathBuf> {
    Some(crate::settings::user_dir()?.join("toolbars.txt"))
}

fn load() -> ToolbarConfig {
    file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|t| ToolbarConfig::from_text(&t))
        .unwrap_or(ToolbarConfig { loaded: true, ..Default::default() })
}

/// Reads the saved settings the first time they are needed (keeps the window's open state).
pub(crate) fn ensure_loaded(c: &mut ToolbarConfig) {
    if !c.loaded {
        let open = c.open;
        *c = load();
        c.open = open;
    }
}

pub(crate) fn save(c: &ToolbarConfig) {
    if let Some(f) = file() {
        let _ = std::fs::write(f, c.to_text());
    }
}

fn label(item: Item) -> &'static str {
    match item {
        Item::Tool(t) => t.label(),
        Item::ZoomExtents => "Zoom étendu",
    }
}

/// The tool bars, at the top of the window.
pub fn draw(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    ensure_loaded(&mut app.ui_state.extra.toolbars);
    let cfg = app.ui_state.extra.toolbars.clone();
    if TOOLBARS.iter().all(|(n, _)| !cfg.shown(n)) && !crate::ruby_ui::any_plugin_toolbar_shown(app) {
        return;
    }
    let side = cfg.icon_size();
    egui::Panel::top("toolbar").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let mut first = true;
            for (name, items) in TOOLBARS {
                if !cfg.shown(name) {
                    continue;
                }
                if !first {
                    ui.separator();
                }
                first = false;
                for &item in *items {
                    button(app, ui, item, &cfg, side);
                }
            }
            crate::ruby_ui::plugin_toolbars(app, ui, side, cfg.tooltips);
        });
    });
}

fn button(app: &mut EsquisseApp, ui: &mut egui::Ui, item: Item, cfg: &ToolbarConfig, side: f32) {
    let active = matches!(item, Item::Tool(t) if app.tool.id() == t);
    let text = label(item);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, Color32::from_rgb(40, 44, 52));
    let with_icon = cfg.mode != DisplayMode::TextOnly;
    let with_text = cfg.mode != DisplayMode::IconOnly;
    let w = (if with_icon { side } else { 0.0 })
        + (if with_text { galley.size().x + if with_icon { 6.0 } else { 0.0 } } else { 0.0 })
        + if with_text { 8.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w.max(side), side), Sense::click());
    let bg = if active {
        Color32::from_rgb(204, 222, 252)
    } else if resp.hovered() {
        Color32::from_rgb(230, 234, 240)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4.0, bg);
    if active {
        ui.painter().rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0, Color32::from_rgb(52, 110, 220)),
            egui::StrokeKind::Inside,
        );
    }
    let mut x = rect.min.x + if with_text { 4.0 } else { 0.0 };
    if with_icon {
        let r = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(side, side)).shrink(side / 8.0);
        match item {
            Item::Tool(t) => icons::draw(ui.painter(), r, t),
            Item::ZoomExtents => icons::zoom_extents(ui.painter(), r),
        }
        x += side + 6.0;
    }
    if with_text {
        let pos = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, Color32::from_rgb(40, 44, 52));
    }
    let resp = if cfg.tooltips {
        let tip = match item {
            Item::Tool(t) => {
                let sc = Cmd::Tool(t).shortcut_text();
                if sc.is_empty() { text.to_string() } else { format!("{text} ({sc})") }
            }
            Item::ZoomExtents => "Zoom étendu (Maj+Z)".to_string(),
        };
        resp.on_hover_text(tip)
    } else {
        resp
    };
    if resp.clicked() {
        match item {
            Item::Tool(t) => app.set_tool(t),
            Item::ZoomExtents => {
                let ctx = ui.ctx().clone();
                app.run(&ctx, Cmd::ZoomExtents);
            }
        }
    }
}

/// View › Toolbars...
pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    if !app.ui_state.extra.toolbars.open {
        return;
    }
    ensure_loaded(&mut app.ui_state.extra.toolbars);
    let before = app.ui_state.extra.toolbars.clone();
    let mut cfg = before.clone();
    let mut open = true;
    centered("Barres d'outils").open(&mut open).show(ctx, |ui| {
        ui.strong("Barres d'outils");
        for (name, items) in TOOLBARS {
            let mut shown = cfg.shown(name);
            if ui.checkbox(&mut shown, format!("{name} ({} outils)", items.len())).changed() {
                cfg.set_shown(name, shown);
            }
        }
        crate::ruby_ui::plugin_toolbar_toggles(app, ui);
        ui.horizontal(|ui| {
            if ui.button("Tout afficher").clicked() {
                cfg.hidden.clear();
            }
            if ui.button("Tout masquer").clicked() {
                cfg.hidden = TOOLBARS.iter().map(|(n, _)| n.to_string()).collect();
            }
        });
        ui.separator();
        ui.strong("Paramètres des barres d'outils");
        ui.checkbox(&mut cfg.tooltips, "Afficher les info-bulles sur les barres d'outils");
        ui.checkbox(&mut cfg.large_icons, "Utiliser de grandes icônes");
        egui::ComboBox::from_label("Mode d'affichage").selected_text(cfg.mode.label()).show_ui(ui, |ui| {
            for m in DisplayMode::ALL {
                ui.selectable_value(&mut cfg.mode, m, m.label());
            }
        });
        if ui.button("Rétablir les valeurs par défaut").clicked() {
            cfg = ToolbarConfig { loaded: true, ..Default::default() };
        }
    });
    cfg.open = open;
    if cfg != before {
        if cfg.hidden != before.hidden
            || cfg.large_icons != before.large_icons
            || cfg.tooltips != before.tooltips
            || cfg.mode != before.mode
        {
            save(&cfg);
        }
        app.ui_state.extra.toolbars = cfg;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_stays_on_a_toolbar() {
        let all: Vec<Item> = TOOLBARS.iter().flat_map(|(_, i)| i.iter().copied()).collect();
        for t in [
            ToolId::Select,
            ToolId::Lasso,
            ToolId::Eraser,
            ToolId::Paint,
            ToolId::TagTool,
            ToolId::Line,
            ToolId::Freehand,
            ToolId::ArcCenter,
            ToolId::Arc,
            ToolId::Arc3,
            ToolId::Pie,
            ToolId::Rect,
            ToolId::RotRect,
            ToolId::Circle,
            ToolId::Polygon,
            ToolId::PushPull,
            ToolId::FollowMe,
            ToolId::Offset,
            ToolId::Move,
            ToolId::Rotate,
            ToolId::Scale,
            ToolId::Flip,
            ToolId::Tape,
            ToolId::Dimension,
            ToolId::Protractor,
            ToolId::Text,
            ToolId::AxesTool,
            ToolId::SectionPlane,
            ToolId::Orbit,
            ToolId::Pan,
            ToolId::Zoom,
            ToolId::ZoomWindow,
            ToolId::PositionCamera,
            ToolId::Walk,
            ToolId::LookAround,
        ] {
            assert!(all.contains(&Item::Tool(t)), "{t:?} missing");
        }
        assert!(all.contains(&Item::ZoomExtents));
    }

    #[test]
    fn settings_round_trip() {
        let mut c = ToolbarConfig { large_icons: false, mode: DisplayMode::TextAndIcon, ..Default::default() };
        c.set_shown("Caméra", false);
        c.set_shown("Dessin", false);
        c.set_shown("Dessin", true);
        let back = ToolbarConfig::from_text(&c.to_text());
        assert!(!back.shown("Caméra") && back.shown("Dessin"));
        assert!(!back.large_icons && back.tooltips);
        assert_eq!(back.mode, DisplayMode::TextAndIcon);
        assert_eq!(back.icon_size(), 24.0);
        // Garbage and unknown toolbars are ignored.
        let c = ToolbarConfig::from_text("hidden=Inconnue\nmode=zzz\nn'importe quoi");
        assert!(c.hidden.is_empty() && c.mode == DisplayMode::IconOnly);
    }
}
