//! Drives the windows of the application like a user: frames of a headless egui context, the
//! widgets found through the accessibility tree (by their label, as a screen reader reads
//! them), clicks and typing sent as real pointer and keyboard events.

use eframe::egui::{self, accesskit, pos2, vec2};

use crate::app::EsquisseApp;

#[derive(Clone, Debug)]
pub struct Node {
    pub id: accesskit::NodeId,
    pub label: String,
    pub value: String,
    pub role: accesskit::Role,
    pub rect: egui::Rect,
    pub toggled: Option<bool>,
    pub disabled: bool,
    pub labelled_by: Vec<accesskit::NodeId>,
}

pub struct Driver {
    pub ctx: egui::Context,
    events: Vec<egui::Event>,
    pub nodes: Vec<Node>,
    time: f64,
    modifiers: egui::Modifiers,
    /// More of the main window to draw under the windows (a tray, a panel).
    pub side: Option<fn(&mut EsquisseApp, &mut egui::Ui)>,
}

impl Driver {
    pub fn new() -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        ctx.set_theme(egui::Theme::Light);
        Driver { ctx, events: Vec::new(), nodes: Vec::new(), time: 0.0, modifiers: egui::Modifiers::NONE, side: None }
    }

    /// One frame of the settings windows (`dialogs::show`; the plugin loading of the other
    /// dialogs would start a second Ruby).
    pub fn frame(&mut self, app: &mut EsquisseApp) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(1600.0, 1000.0))),
            events: std::mem::take(&mut self.events),
            time: Some(self.time),
            focused: true,
            ..Default::default()
        };
        self.time += 1.0 / 30.0;
        let side = self.side;
        let mut out = self.ctx.run_ui(input, |ui| {
            crate::dialogs::frame_start(app, ui.ctx());
            if let Some(draw) = side {
                draw(app, ui);
            }
            crate::dialogs::show(app, ui.ctx());
        });
        out.textures_delta.clear();
        if let Some(u) = out.platform_output.accesskit_update {
            self.nodes = u
                .nodes
                .iter()
                .map(|(id, n)| Node {
                    id: *id,
                    label: n.label().unwrap_or_default().to_string(),
                    value: n.value().unwrap_or_default().to_string(),
                    role: n.role(),
                    rect: n
                        .bounds()
                        .map(|b| {
                            egui::Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32))
                        })
                        .unwrap_or(egui::Rect::NOTHING),
                    toggled: n.toggled().map(|t| t == accesskit::Toggled::True),
                    disabled: n.is_disabled(),
                    labelled_by: n.labelled_by().to_vec(),
                })
                .collect();
        }
    }

    /// A few frames, for windows to size and settle.
    pub fn frames(&mut self, app: &mut EsquisseApp, n: usize) {
        for _ in 0..n {
            self.frame(app);
        }
    }

    /// The widget named `name` (its own label, the text of a plain label, or the label it is
    /// "labelled by").
    pub fn find(&self, name: &str) -> Option<&Node> {
        let named = |n: &&Node| {
            n.label == name
                || (n.role == accesskit::Role::Label && n.value == name)
                || n.labelled_by
                    .iter()
                    .any(|l| self.nodes.iter().any(|x| x.id == *l && (x.value == name || x.label == name)))
        };
        // Prefer interactive widgets over the text labelling them, and the window in front
        // (the others are greyed while it is open) over the ones behind.
        self.nodes
            .iter()
            .filter(named)
            .filter(|n| n.rect.is_finite() && n.rect.area() > 0.0)
            .min_by_key(|n| (n.role == accesskit::Role::Label, n.disabled))
    }

    pub fn has(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    #[track_caller]
    pub fn node(&self, name: &str) -> Node {
        match self.find(name) {
            Some(n) => n.clone(),
            None => {
                let mut names: Vec<String> =
                    self.nodes.iter().filter(|n| !n.label.is_empty()).map(|n| n.label.clone()).collect();
                names.sort();
                names.dedup();
                panic!("« {name} » introuvable ; widgets : {names:?}")
            }
        }
    }

    /// Activates a widget the way UI Automation's "Invoke" does in the running application
    /// (and a screen reader): an AccessKit click on its node, wherever it is scrolled.
    #[track_caller]
    pub fn click(&mut self, app: &mut EsquisseApp, name: &str) {
        let n = self.node(name);
        assert!(!n.disabled, "« {name} » est grisé");
        self.action(app, &n, accesskit::Action::Click);
    }

    /// Sends an AccessKit action to a widget, then lets the windows settle.
    pub fn action(&mut self, app: &mut EsquisseApp, n: &Node, action: accesskit::Action) {
        self.events.push(egui::Event::AccessKitActionRequest(accesskit::ActionRequest {
            action,
            target_tree: accesskit::TreeId::ROOT,
            target_node: n.id,
            data: None,
        }));
        self.frames(app, 3);
    }

    pub fn click_at(&mut self, app: &mut EsquisseApp, p: egui::Pos2) {
        self.events.push(egui::Event::PointerMoved(p));
        self.frame(app);
        self.events.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: self.modifiers,
        });
        self.frame(app);
        self.events.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: self.modifiers,
        });
        self.frames(app, 2);
    }

    /// Opens a drop-down list and picks an entry.
    #[track_caller]
    pub fn choose(&mut self, app: &mut EsquisseApp, combo: &str, entry: &str) {
        let list = self.node(combo).id;
        self.click(app, combo);
        let n = self
            .nodes
            .iter()
            .filter(|n| n.id != list && n.label == entry)
            .filter(|n| n.rect.is_finite() && n.rect.area() > 0.0)
            .max_by(|a, b| a.rect.min.y.total_cmp(&b.rect.min.y))
            .cloned()
            .unwrap_or_else(|| panic!("entrée « {entry} » absente de « {combo} »"));
        self.action(app, &n, accesskit::Action::Click);
        // The list closes.
        self.frames(app, 2);
    }

    /// Replaces the text of a field and presses Enter.
    #[track_caller]
    pub fn type_into(&mut self, app: &mut EsquisseApp, name: &str, text: &str) {
        let n = self.node(name);
        self.action(app, &n, accesskit::Action::Focus);
        self.key(app, egui::Key::A, egui::Modifiers::COMMAND);
        self.events.push(egui::Event::Text(text.to_string()));
        self.frame(app);
        self.key(app, egui::Key::Enter, egui::Modifiers::NONE);
    }

    /// Gives the keyboard focus to a widget (as Tab would).
    #[track_caller]
    pub fn focus(&mut self, app: &mut EsquisseApp, name: &str) {
        let n = self.node(name);
        self.action(app, &n, accesskit::Action::Focus);
    }

    /// Types text into the focused field.
    pub fn text(&mut self, app: &mut EsquisseApp, text: &str) {
        self.events.push(egui::Event::Text(text.to_string()));
        self.frame(app);
    }

    pub fn key(&mut self, app: &mut EsquisseApp, key: egui::Key, modifiers: egui::Modifiers) {
        for pressed in [true, false] {
            self.events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
        self.frame(app);
        // What the key changed shows at the next frame.
        self.frame(app);
    }

    pub fn toggled(&self, name: &str) -> Option<bool> {
        self.find(name).and_then(|n| n.toggled)
    }

    /// The value shown by a widget (selected entry of a list, text of a field).
    #[track_caller]
    pub fn value(&self, name: &str) -> String {
        self.node(name).value
    }
}

/// The application without a window, with an empty model.
pub fn app() -> EsquisseApp {
    EsquisseApp::bare(None, None)
}
