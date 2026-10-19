//! Extensions › Developer › Console, Extensions › Extension Manager and the menu entries
//! that Ruby plugins register (`UI.menu("Plugins").add_item(...)`).
//!
//! Plugins are the `*.rb` files of `%APPDATA%\Esquisse\Plugins`, loaded once at start-up. Ruby
//! itself only starts when there is something to run (a plugin, or the console being opened).

use std::rc::Rc;

use eframe::egui;
use esq_ruby::{Interpreter, MenuCommand, RunOutput, ToolbarSpec};

use crate::app::EsquisseApp;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Input,
    Result,
    Output,
    Error,
}

#[derive(Default)]
pub struct RubyState {
    interp: Option<Result<Rc<Interpreter>, String>>,
    plugins_checked: bool,
    pub console_open: bool,
    pub manager_open: bool,
    lines: Vec<(Kind, String)>,
    input: String,
    history: Vec<String>,
    hist_pos: Option<usize>,
    focus_input: bool,
    pub commands: Vec<MenuCommand>,
    /// A menu entry clicked this frame, run at the end of it.
    pending: Option<i64>,
    /// Plugin files seen at the last (re)load, with their outcome.
    loaded: Vec<String>,
    /// Toolbars built by plugins (`UI::Toolbar`).
    pub toolbars: Vec<ToolbarSpec>,
    /// Entries the plugins' context-menu handlers added for the last right click.
    context: Vec<MenuCommand>,
    /// What the model looked like when the validation procs last ran.
    validated_at: (usize, usize, usize),
    /// Extensions registered through `Sketchup.register_extension` (rows of `Esq.extensions_json`).
    extensions: Vec<serde_json::Value>,
    /// Outcome of each plugin file at the last load (rows of `Esq.plugin_report_json`).
    plugin_report: Vec<serde_json::Value>,
    /// Extension switch clicked in the manager, applied during the next frame.
    toggle: Option<(String, bool)>,
}

impl RubyState {
    pub fn focus_input(&mut self) {
        self.focus_input = true;
    }

    /// Runs a plugin command at the end of the frame (menu entry, toolbar button).
    pub fn click(&mut self, id: i64) {
        self.pending = Some(id);
    }

    /// `Sketchup::Console#write`.
    pub fn console_write(&mut self, text: &str) {
        let t = text.trim_end_matches('\n');
        for line in t.split('\n') {
            self.lines.push((Kind::Output, line.to_string()));
        }
    }

    pub fn console_clear(&mut self) {
        self.lines.clear();
    }

    fn interp(&mut self) -> Result<Rc<Interpreter>, String> {
        share_support_dir();
        self.interp.get_or_insert_with(|| Interpreter::start().map(Rc::new).map_err(|e| format!("{e:#}"))).clone()
    }

    fn log(&mut self, kind: Kind, text: impl Into<String>) {
        let t: String = text.into();
        if !t.is_empty() {
            self.lines.push((kind, t));
        }
    }

    fn report(&mut self, out: &RunOutput) {
        self.log(Kind::Output, out.out.trim_end().to_string());
        if out.ok {
            if !out.result.is_empty() {
                self.log(Kind::Result, format!("=> {}", out.result));
            }
        } else {
            self.log(Kind::Error, out.error.trim_end().to_string());
        }
    }

    /// The interpreter if Ruby already runs (never starts it): Ruby tools and observers.
    pub(crate) fn started(&self) -> Option<Rc<Interpreter>> {
        match &self.interp {
            Some(Ok(i)) => Some(i.clone()),
            _ => None,
        }
    }

    /// Output and errors of a script run by the app (Ruby tools, observers) go to the console.
    pub(crate) fn report_run(&mut self, out: Result<RunOutput, String>) {
        match out {
            Ok(o) => self.report(&RunOutput { result: String::new(), ..o }),
            Err(e) => self.log(Kind::Error, e),
        }
    }

    /// True if the last right click collected entries of the plugin menu `path` (« Context/Tool »:
    /// the active Ruby tool's `getMenu`).
    pub(crate) fn context_items_of(&self, path: &str) -> bool {
        self.context.iter().any(|c| c.menu.starts_with(path))
    }

    /// True if the last right click collected entries of the extensions.
    pub(crate) fn has_context_items(&self) -> bool {
        !self.context.is_empty()
    }

    /// What the extensions' handlers would have added (tests without Ruby).
    #[cfg(test)]
    pub(crate) fn set_context_items(&mut self, items: Vec<MenuCommand>) {
        self.context = items;
    }
}

/// One level of a plugin menu: entries and submenus in the order they were added.
#[derive(Debug, PartialEq)]
pub enum MenuNode<'a> {
    Item(&'a MenuCommand),
    Sub(String),
}

/// What plugins put directly in the menu at `path` (`Tools`, `Extensions/My plugin`): their
/// entries and separators, and the names of the submenus below it, in order of appearance.
pub fn menu_nodes<'a>(cmds: &'a [MenuCommand], path: &str) -> Vec<MenuNode<'a>> {
    let mut out: Vec<MenuNode> = Vec::new();
    for c in cmds {
        if c.menu == path {
            out.push(MenuNode::Item(c));
        } else if let Some(rest) = c.menu.strip_prefix(path).and_then(|r| r.strip_prefix('/')) {
            let name = rest.split('/').next().unwrap_or(rest).to_string();
            if !out.iter().any(|n| matches!(n, MenuNode::Sub(s) if *s == name)) {
                out.push(MenuNode::Sub(name));
            }
        }
    }
    out
}

fn draw_menu_item(app: &mut EsquisseApp, ui: &mut egui::Ui, c: &MenuCommand) {
    if c.separator {
        ui.separator();
        return;
    }
    let label = if c.checked() { format!("✔  {}", c.title) } else { c.title.clone() };
    let mut r = ui.add_enabled(c.enabled(), egui::Button::new(label));
    let tip = if !c.status.is_empty() { &c.status } else { &c.tooltip };
    if !tip.is_empty() {
        r = r.on_hover_text(tip);
    }
    if r.clicked() {
        ui.close();
        app.ui_state.extra.ruby.click(c.id);
    }
}

/// The entries plugins added to the menu at `path`, and its submenus.
pub fn plugin_menu(app: &mut EsquisseApp, ui: &mut egui::Ui, path: &str) {
    let cmds = app.ui_state.extra.ruby.commands.clone();
    let nodes = menu_nodes(&cmds, path);
    if nodes.is_empty() {
        return;
    }
    ui.separator();
    for n in nodes {
        match n {
            MenuNode::Item(c) => draw_menu_item(app, ui, c),
            // The Developer submenu of Extensions is drawn by the app's own submenu.
            MenuNode::Sub(name) if path == "Extensions" && name == "Developer" => {}
            MenuNode::Sub(name) => {
                let sub = format!("{path}/{name}");
                ui.menu_button(name, |ui| plugin_menu_body(app, ui, &cmds, &sub));
            }
        }
    }
}

fn plugin_menu_body(app: &mut EsquisseApp, ui: &mut egui::Ui, cmds: &[MenuCommand], path: &str) {
    for n in menu_nodes(cmds, path) {
        match n {
            MenuNode::Item(c) => draw_menu_item(app, ui, c),
            MenuNode::Sub(name) => {
                let sub = format!("{path}/{name}");
                ui.menu_button(name, |ui| plugin_menu_body(app, ui, cmds, &sub));
            }
        }
    }
}

/// Entries added by plugins to the app's own menu `name` (the app's menus are in French).
pub fn plugin_menu_for(app: &mut EsquisseApp, ui: &mut egui::Ui, name: &str) {
    let keys: &[&str] = match name {
        "Fichier" => &["File"],
        "Édition" => &["Edit"],
        "Affichage" => &["View"],
        "Fenêtre" => &["Window"],
        "Caméra" => &["Camera"],
        "Dessiner" => &["Draw"],
        "Outils" => &["Tools"],
        "Aide" => &["Help"],
        _ => &[],
    };
    for k in keys {
        plugin_menu(app, ui, k);
    }
}

/// Plugin entries of the Extensions menu.
pub fn plugin_items(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    plugin_menu(app, ui, "Extensions");
}

/// Plugin entries of the Extensions › Developer submenu.
pub fn plugin_developer_items(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let cmds = app.ui_state.extra.ruby.commands.clone();
    plugin_menu_body(app, ui, &cmds, "Extensions/Developer");
}

/// Draws the toolbars plugins built, after the app's own (`toolbars::draw`).
pub fn plugin_toolbars(app: &mut EsquisseApp, ui: &mut egui::Ui, side: f32, tooltips: bool) {
    crate::ruby_host::draw_toolbars(app, ui, side, tooltips);
}

/// Whether any plugin toolbar is currently shown.
pub fn any_plugin_toolbar_shown(app: &EsquisseApp) -> bool {
    app.ui_state.extra.ruby.toolbars.iter().any(|b| crate::ruby_host::plugin_toolbar_shown(&b.name))
}

/// View › Toolbars: a check box per plugin toolbar.
pub fn plugin_toolbar_toggles(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let names: Vec<(String, usize)> =
        app.ui_state.extra.ruby.toolbars.iter().map(|b| (b.name.clone(), b.items.iter().flatten().count())).collect();
    for (name, n) in names {
        let mut shown = crate::ruby_host::plugin_toolbar_shown(&name);
        if ui.checkbox(&mut shown, format!("{name} ({n} outils)")).changed() {
            crate::ruby_host::set_plugin_toolbar_shown(&name, shown);
        }
    }
}

/// Right click in the viewport: lets the plugins' handlers add their entries.
pub fn collect_context_menu(app: &mut EsquisseApp) {
    let st = &mut app.ui_state.extra.ruby;
    st.context.clear();
    let Some(Ok(interp)) = st.interp.clone() else { return };
    let mut sel = std::mem::take(&mut app.doc.sel);
    let r = interp.context_menu(&mut app.doc.model, &mut sel);
    app.doc.sel = sel;
    match r {
        Ok(c) => app.ui_state.extra.ruby.context = c,
        Err(e) => app.ui_state.extra.ruby.log(Kind::Error, format!("{e:#}")),
    }
}

/// The context-menu entries collected by `collect_context_menu`.
pub fn context_items(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let items = app.ui_state.extra.ruby.context.clone();
    if items.is_empty() {
        return;
    }
    ui.separator();
    for c in &items {
        draw_menu_item(app, ui, c);
    }
}

/// Runs a Ruby expression that returns a JSON array (as a string) and parses the rows.
fn json_rows(app: &mut EsquisseApp, expr: &str) -> Vec<serde_json::Value> {
    let Ok(interp) = app.ui_state.extra.ruby.interp() else { return Vec::new() };
    let mut sel = Vec::new();
    let Ok(out) = interp.run(expr, "extensions", &mut app.doc.model, &mut sel) else { return Vec::new() };
    if !out.ok {
        return Vec::new();
    }
    // `result` is the `inspect` of a Ruby string: a JSON string literal holding JSON.
    serde_json::from_str::<String>(&out.result)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok())
        .unwrap_or_default()
}

fn refresh_extensions(app: &mut EsquisseApp) {
    let ext = json_rows(app, "Esq.extensions_json");
    let rep = json_rows(app, "Esq.plugin_report_json");
    app.ui_state.extra.ruby.extensions = ext;
    app.ui_state.extra.ruby.plugin_report = rep;
}

fn refresh_commands(app: &mut EsquisseApp) {
    let Ok(interp) = app.ui_state.extra.ruby.interp() else { return };
    let mut sel = std::mem::take(&mut app.doc.sel);
    let cmds = interp.commands(&mut app.doc.model, &mut sel);
    let bars = interp.toolbars(&mut app.doc.model, &mut sel);
    app.doc.sel = sel;
    app.ui_state.extra.ruby.validated_at = model_key(app);
    match cmds {
        Ok(c) => app.ui_state.extra.ruby.commands = c,
        Err(e) => app.ui_state.extra.ruby.log(Kind::Error, format!("{e:#}")),
    }
    match bars {
        Ok(b) => app.ui_state.extra.ruby.toolbars = b,
        Err(e) => app.ui_state.extra.ruby.log(Kind::Error, format!("{e:#}")),
    }
}

/// Cheap fingerprint of what validation procs look at (selection, amount of geometry).
fn model_key(app: &EsquisseApp) -> (usize, usize, usize) {
    let r = &app.doc.model.root;
    (app.doc.sel.len(), r.edges.len() + r.faces.len(), r.instances.len())
}

/// Hands the events waiting since the last frame (JavaScript callbacks, timers, closed dialogs,
/// notification buttons) to Ruby, in one run with the code of the tools zone (`tools`: the
/// Ruby tool's events and drawing, the observers; `light` when it is only mouse moves).
fn pump(app: &mut EsquisseApp, ui_events: bool, tools: Option<(String, bool)>) {
    let Some(Ok(interp)) = app.ui_state.extra.ruby.interp.clone() else { return };
    let (mut code, light) = tools.unwrap_or_default();
    if ui_events {
        code.push_str("Esq.ui_pump");
    }
    let r = if light && !ui_events {
        crate::ruby_host::run_light(app, "Outil Ruby", |m, s| interp.run(&code, "outil", m, s))
    } else {
        crate::ruby_host::run_script(app, "Rappels Ruby", |m, s| interp.run(&code, "ui", m, s))
    };
    match r {
        Ok(out) => {
            let failed = !out.ok;
            app.ui_state.extra.ruby.report(&RunOutput { result: String::new(), ..out });
            if failed && ui_events {
                app.ui_state.extra.ruby.console_open = true;
            }
        }
        Err(e) => app.ui_state.extra.ruby.log(Kind::Error, format!("{e:#}")),
    }
    if ui_events {
        refresh_commands(app);
    }
}

/// Gives Ruby the user folder of [`crate::settings::user_dir`] (a temporary one in tests): its
/// `Plugins` and saved defaults live there.
pub(crate) fn share_support_dir() {
    if let Some(d) = crate::settings::user_dir() {
        esq_ruby::set_support_dir(d);
    }
}

fn plugins_dir() -> std::path::PathBuf {
    share_support_dir();
    esq_ruby::plugins_dir()
}

fn load_plugins(app: &mut EsquisseApp) {
    let dir = plugins_dir();
    let files: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            let mut v: Vec<String> = rd
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".rb"))
                .collect();
            v.sort();
            v
        })
        .unwrap_or_default();
    app.ui_state.extra.ruby.loaded = files.clone();
    if files.is_empty() {
        return;
    }
    match app.ui_state.extra.ruby.interp() {
        Err(e) => app.ui_state.extra.ruby.log(Kind::Error, format!("Ruby indisponible : {e}")),
        Ok(interp) => {
            let r = crate::ruby_host::run_script(app, "Plugins Ruby", |m, s| interp.load_plugins(&dir, m, s));
            match r {
                Ok(out) => app.ui_state.extra.ruby.report(&RunOutput { result: String::new(), ..out }),
                Err(e) => app.ui_state.extra.ruby.log(Kind::Error, format!("{e:#}")),
            }
            refresh_commands(app);
        }
    }
}

fn run_input(app: &mut EsquisseApp) {
    let code = std::mem::take(&mut app.ui_state.extra.ruby.input);
    if code.trim().is_empty() {
        return;
    }
    {
        let st = &mut app.ui_state.extra.ruby;
        st.history.push(code.clone());
        st.hist_pos = None;
        st.log(Kind::Input, format!(">> {}", code.trim_end().replace('\n', "\n   ")));
    }
    match app.ui_state.extra.ruby.interp() {
        Err(e) => app.ui_state.extra.ruby.log(Kind::Error, format!("Ruby indisponible : {e}")),
        Ok(interp) => {
            let r = crate::ruby_host::run_script(app, "Script Ruby", |m, s| interp.run(&code, "console", m, s));
            match r {
                Ok(out) => app.ui_state.extra.ruby.report(&out),
                Err(e) => app.ui_state.extra.ruby.log(Kind::Error, format!("{e:#}")),
            }
            refresh_commands(app);
        }
    }
}

/// Delivers to Ruby what its network threads received (`Sketchup::Http` responses, `SKSocket`
/// data) and keeps the frames coming while a request or socket is open. Never starts Ruby.
fn pump_network(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(Ok(interp)) = app.ui_state.extra.ruby.interp.clone() else { return };
    if esq_ruby::pending_events() {
        let r = crate::ruby_host::run_script(app, "Rappel réseau Ruby", |m, s| interp.pump(m, s));
        match r {
            Ok(out) => app.ui_state.extra.ruby.report(&RunOutput { result: String::new(), ..out }),
            Err(e) => app.ui_state.extra.ruby.log(Kind::Error, format!("{e:#}")),
        }
        refresh_commands(app);
        if !app.ui_state.extra.ruby.lines.is_empty() {
            app.ui_state.extra.ruby.console_open = true;
        }
    }
    if esq_ruby::network_active() || esq_ruby::pending_events() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// Called once per frame (from `extra::show`).
pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    if !app.ui_state.extra.ruby.plugins_checked {
        app.ui_state.extra.ruby.plugins_checked = true;
        load_plugins(app);
    }
    pump_network(app, ctx);
    if let Some((name, on)) = app.ui_state.extra.ruby.toggle.take()
        && let Some(Ok(interp)) = app.ui_state.extra.ruby.interp.clone()
    {
        let code = format!("Esq.set_extension_enabled({name:?}, {on})");
        let r = crate::ruby_host::run_script(app, "Extension Ruby", |m, s| interp.run(&code, "extensions", m, s));
        if let Ok(out) = r {
            app.ui_state.extra.ruby.report(&RunOutput { result: String::new(), ..out });
        }
        refresh_commands(app);
        refresh_extensions(app);
    }
    if let Some(id) = app.ui_state.extra.ruby.pending.take() {
        match app.ui_state.extra.ruby.interp() {
            Ok(interp) => {
                let r = crate::ruby_host::run_script(app, "Commande Ruby", |m, s| interp.invoke(id, m, s));
                match r {
                    Ok(out) => app.ui_state.extra.ruby.report(&RunOutput { result: String::new(), ..out }),
                    Err(e) => app.ui_state.extra.ruby.log(Kind::Error, format!("{e:#}")),
                }
            }
            Err(e) => app.ui_state.extra.ruby.log(Kind::Error, e),
        }
        if !app.ui_state.extra.ruby.lines.is_empty() {
            app.ui_state.extra.ruby.console_open = true;
        }
    }
    // Timers, toasts, requests from Ruby; then what happened in dialogs since the last frame,
    // and what the Ruby tool, the overlays and the observers must hear (ruby_tools.rs).
    crate::ruby_host::ui_frame(app, ctx);
    let tools = crate::ruby_tools::take_code(app, ctx.input(|i| i.time));
    let ui_events = matches!(app.ui_state.extra.ruby.interp, Some(Ok(_))) && crate::ruby_host::ui_has_events();
    if ui_events || tools.is_some() {
        pump(app, ui_events, tools);
        crate::ruby_tools::after_run(app);
    }
    // Validation procs (greyed/ticked entries) follow the selection and the model.
    let r = &app.ui_state.extra.ruby;
    if (!r.commands.is_empty() || !r.toolbars.is_empty()) && r.validated_at != model_key(app) {
        refresh_commands(app);
    }
    if app.ui_state.extra.ruby.console_open {
        console(app, ctx);
    }
    if app.ui_state.extra.ruby.manager_open {
        manager(app, ctx);
    }
}

fn console(app: &mut EsquisseApp, ctx: &egui::Context) {
    let mut open = true;
    let mut run = false;
    egui::Window::new("Console Ruby").default_size([560.0, 340.0]).open(&mut open).show(ctx, |ui| {
        let st = &mut app.ui_state.extra.ruby;
        let input_height = 64.0;
        egui::ScrollArea::vertical()
            .id_salt("ruby_console_log")
            .stick_to_bottom(true)
            .max_height((ui.available_height() - input_height - 36.0).max(80.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if st.lines.is_empty() {
                    ui.weak("Saisissez du Ruby (API SketchUp) puis Entrée. Maj+Entrée : nouvelle ligne.");
                }
                for (kind, text) in &st.lines {
                    let color = match kind {
                        Kind::Input => ui.visuals().weak_text_color(),
                        Kind::Result => egui::Color32::from_rgb(90, 170, 110),
                        Kind::Output => ui.visuals().text_color(),
                        Kind::Error => egui::Color32::from_rgb(220, 90, 80),
                    };
                    ui.label(egui::RichText::new(text).monospace().color(color));
                }
            });
        ui.separator();
        // Enter runs, Shift+Enter inserts a newline, Up/Down walk through the history.
        let enter = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let up =
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) && st.input.lines().count() <= 1;
        let down = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown))
            && st.input.lines().count() <= 1;
        let resp = ui.add_sized(
            [ui.available_width(), input_height],
            egui::TextEdit::multiline(&mut st.input)
                .font(egui::TextStyle::Monospace)
                .hint_text(">> Ruby")
                .lock_focus(true),
        );
        if st.focus_input {
            resp.request_focus();
            st.focus_input = false;
        }
        if up && !st.history.is_empty() {
            let p = st.hist_pos.map_or(st.history.len() - 1, |p| p.saturating_sub(1));
            st.hist_pos = Some(p);
            st.input = st.history[p].clone();
        }
        if down && let Some(p) = st.hist_pos {
            if p + 1 < st.history.len() {
                st.hist_pos = Some(p + 1);
                st.input = st.history[p + 1].clone();
            } else {
                st.hist_pos = None;
                st.input.clear();
            }
        }
        if enter && resp.has_focus() {
            run = true;
        }
    });
    if run {
        run_input(app);
        app.ui_state.extra.ruby.focus_input = true;
        ctx.request_repaint();
    }
    app.ui_state.extra.ruby.console_open = open;
}

fn manager(app: &mut EsquisseApp, ctx: &egui::Context) {
    let mut open = true;
    let mut reload = false;
    egui::Window::new("Gestionnaire d'extensions").default_width(460.0).open(&mut open).show(ctx, |ui| {
        let dir = plugins_dir();
        let st = &app.ui_state.extra.ruby;
        ui.label("Extensions Ruby (API SketchUp) chargées au démarrage depuis :");
        ui.monospace(dir.display().to_string());
        ui.horizontal(|ui| {
            if ui.button("Ouvrir le dossier").clicked() {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::process::Command::new("explorer").arg(&dir).spawn();
            }
            if ui.button("Recharger").on_hover_text("Recharge tous les fichiers .rb du dossier").clicked() {
                reload = true;
            }
        });
        ui.separator();
        match &st.interp {
            Some(Err(e)) => {
                ui.colored_label(egui::Color32::from_rgb(220, 90, 80), format!("Ruby indisponible : {e}"));
            }
            Some(Ok(_)) => {
                ui.colored_label(egui::Color32::from_rgb(90, 170, 110), "Ruby 3.2 démarré");
            }
            None => {
                let found = esq_ruby::candidate_dlls().into_iter().find(|p| p.is_file());
                match found {
                    Some(p) => ui.weak(format!("Ruby trouvé : {}", p.display())),
                    None => ui.colored_label(
                        egui::Color32::from_rgb(220, 140, 60),
                        "Ruby 3.2 introuvable : le dossier « ruby » livré à côté d'esquisse.exe manque (ou posez ESQ_RUBY_DLL).",
                    ),
                };
            }
        }
        ui.separator();
        if st.loaded.is_empty() {
            ui.weak("Aucun fichier .rb dans le dossier.");
        }
        for f in &st.loaded {
            ui.label(format!("• {f}"));
        }
        if !st.commands.is_empty() {
            ui.separator();
            ui.label(format!("{} commande(s) de menu enregistrée(s)", st.commands.len()));
        }
    });
    if reload {
        load_plugins(app);
    }
    app.ui_state.extra.ruby.manager_open = open;
}
