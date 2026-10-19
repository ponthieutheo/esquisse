//! Window › Model Info and Window › Preferences: SketchUp's two settings windows, the list of
//! pages on the left and the chosen page on the right. Model Info settings live in the model
//! (`Model::meta.options` and friends, where the Ruby API reads them); preferences live in
//! `settings.rs`.

use eframe::egui::{self, RichText};

use crate::app::EsquisseApp;

pub mod comp_edit;
pub mod instructor;
pub mod mi_anim;
pub mod mi_annot;
pub mod mi_classif;
pub mod mi_file;
pub mod mi_misc;
pub mod mi_stats;
pub mod model_info;
pub mod pref_general;
pub mod pref_pages;
pub mod pref_shortcuts;
pub mod preferences;
pub mod scene_props;
pub mod tag_dialog;
pub mod trays;
pub mod workspaces;

/// The other windows of this module, opened from the Window menu (`Cmd::Dialog`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dlg {
    NewTray,
    ManageTrays,
    NewWorkspace,
    ConfigureWorkspace,
    SaveWorkspace,
    LoadWorkspace,
}

impl Dlg {
    pub fn label(self) -> &'static str {
        match self {
            Dlg::NewTray => "Nouvelle palette...",
            Dlg::ManageTrays => "Gérer les palettes...",
            Dlg::NewWorkspace => "Nouvel espace de travail...",
            Dlg::ConfigureWorkspace => "Configurer l'espace de travail...",
            Dlg::SaveWorkspace => "Sauvegarder l'espace de travail...",
            Dlg::LoadWorkspace => "Charger l'espace de travail...",
        }
    }
}

/// Runs `Cmd::Dialog`.
pub fn open(app: &mut EsquisseApp, ctx: &egui::Context, d: Dlg) {
    match d {
        Dlg::NewTray => trays::open_new(app),
        Dlg::ManageTrays => trays::open_manage(app),
        Dlg::NewWorkspace => workspaces::open_new(app),
        Dlg::ConfigureWorkspace => workspaces::open_configure(app),
        Dlg::SaveWorkspace => workspaces::save_dialog(app, ctx),
        Dlg::LoadWorkspace => workspaces::load_dialog(app, ctx),
    }
}

/// Pages of Window › Model Info, in the order of the list (French alphabetical, as SketchUp).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InfoPage {
    Animation,
    Classifications,
    Components,
    Dimensions,
    Credits,
    File,
    Geolocation,
    Rendering,
    Statistics,
    Text,
    Units,
}

impl InfoPage {
    pub const ALL: [InfoPage; 11] = [
        InfoPage::Animation,
        InfoPage::Classifications,
        InfoPage::Components,
        InfoPage::Dimensions,
        InfoPage::Credits,
        InfoPage::File,
        InfoPage::Geolocation,
        InfoPage::Rendering,
        InfoPage::Statistics,
        InfoPage::Text,
        InfoPage::Units,
    ];

    pub fn label(self) -> &'static str {
        match self {
            InfoPage::Animation => "Animation",
            InfoPage::Classifications => "Classements",
            InfoPage::Components => "Composants",
            InfoPage::Dimensions => "Cotations",
            InfoPage::Credits => "Crédits",
            InfoPage::File => "Fichier",
            InfoPage::Geolocation => "Géoposition",
            InfoPage::Rendering => "Rendu",
            InfoPage::Statistics => "Statistiques",
            InfoPage::Text => "Texte",
            InfoPage::Units => "Unités",
        }
    }
}

/// Pages of Window › Preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefPage {
    Accessibility,
    Applications,
    Compatibility,
    Drawing,
    Workspace,
    Files,
    General,
    Graphics,
    Template,
    Shortcuts,
}

impl PrefPage {
    pub const ALL: [PrefPage; 10] = [
        PrefPage::Accessibility,
        PrefPage::Applications,
        PrefPage::Compatibility,
        PrefPage::Drawing,
        PrefPage::Workspace,
        PrefPage::Files,
        PrefPage::General,
        PrefPage::Graphics,
        PrefPage::Template,
        PrefPage::Shortcuts,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PrefPage::Accessibility => "Accessibilité",
            PrefPage::Applications => "Applications",
            PrefPage::Compatibility => "Compatibilité",
            PrefPage::Drawing => "Dessin",
            PrefPage::Workspace => "Espace de travail",
            PrefPage::Files => "Fichiers",
            PrefPage::General => "Général",
            PrefPage::Graphics => "Graphiques",
            PrefPage::Template => "Modèle type",
            PrefPage::Shortcuts => "Raccourcis",
        }
    }

    /// Title of the command that opens the page (SketchUp's `commands.json` titles).
    pub fn command_label(self) -> &'static str {
        match self {
            PrefPage::Accessibility => "Ouvrir les paramètres d'accessibilité",
            PrefPage::Shortcuts => "Ouvrir Raccourcis Paramètres",
            PrefPage::Workspace => "Ouvrir les paramètres de l'espace de travail",
            PrefPage::Applications => "Ouvrir les paramètres des applications",
            PrefPage::Compatibility => "Ouvrir les paramètres de compatibilité",
            PrefPage::Drawing => "Ouvrir les paramètres de dessin",
            PrefPage::Files => "Ouvrir les paramètres des fichiers",
            PrefPage::General => "Ouvrir les paramètres généraux",
            PrefPage::Graphics => "Ouvrir les paramètres graphiques",
            PrefPage::Template => "Ouvrir les paramètres du modèle type",
        }
    }
}

/// Commands SketchUp has in its command search but in no menu.
pub const SEARCH_CMDS: &[crate::commands::Cmd] = &[
    crate::commands::Cmd::PrefsPage(PrefPage::Accessibility),
    crate::commands::Cmd::PrefsPage(PrefPage::Shortcuts),
    crate::commands::Cmd::PrefsPage(PrefPage::Workspace),
    crate::commands::Cmd::ResetShortcuts,
];

/// State of the two windows between frames.
pub struct DialogState {
    pub info_page: InfoPage,
    pub pref_page: PrefPage,
    pub info: model_info::InfoState,
    pub prefs: preferences::PrefState,
    pub trays: trays::TrayState,
    pub workspaces: workspaces::WorkspaceState,
    /// Window › Components › Edit.
    pub comp_edit: comp_edit::CompEditState,
    /// Window › Scenes: the selected scene's properties.
    pub scene_edit: scene_props::SceneEditState,
    /// Window › Tags: deleting a tag.
    pub tags: tag_dialog::TagState,
}

impl Default for DialogState {
    fn default() -> Self {
        DialogState {
            info_page: InfoPage::Units,
            pref_page: PrefPage::General,
            info: Default::default(),
            prefs: Default::default(),
            trays: Default::default(),
            workspaces: Default::default(),
            comp_edit: Default::default(),
            scene_edit: Default::default(),
            tags: Default::default(),
        }
    }
}

/// Called at the start of every frame: the open model's unit settings become the ones every
/// length, area and angle shown uses.
pub fn frame_start(app: &mut EsquisseApp, ctx: &egui::Context) {
    esq_core::units::set_display(Some(esq_core::units::UnitsDisplay::of(&app.doc.model)));
    mi_anim::sync(app);
    pref_general::tick(app, ctx.input(|i| i.time));
    trays::sync(app);
}

/// Called once per frame with the other dialogs.
pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    if app.ui_state.model_info {
        let mut open = true;
        egui::Window::new("Infos sur le modèle")
            .collapsible(false)
            .resizable(true)
            .default_size([620.0, 460.0])
            .default_pos([180.0, 110.0])
            .open(&mut open)
            .show(ctx, |ui| model_info::window(app, ui));
        app.ui_state.model_info = open;
    }
    if app.ui_state.preferences {
        preferences::window(app, ctx);
    }
    pref_general::purge_dialog(app, ctx);
    trays::windows(app, ctx);
    workspaces::windows(app, ctx);
    scene_props::update_window(app, ctx);
    tag_dialog::windows(app, ctx);
    crate::ctx_dialogs::windows(app, ctx);
    crate::make_component::window(app, ctx);
    crate::warnings::window(app, ctx);
}

/// The list of pages on the left of a settings window.
pub(crate) fn page_list<P: Copy + PartialEq>(ui: &mut egui::Ui, pages: &[(P, &str)], current: &mut P) {
    ui.vertical(|ui| {
        ui.set_width(150.0);
        for (p, label) in pages {
            if ui.selectable_label(*current == *p, *label).clicked() {
                *current = *p;
            }
        }
    });
}

/// A group title inside a page ("Unités de mesure", "Enregistrement"…).
pub(crate) fn group(ui: &mut egui::Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).strong());
    ui.separator();
}

/// Gives a widget its accessible name (and value): what a screen reader says, and what the
/// tests use to find it.
pub(crate) fn name_widget(ui: &egui::Ui, r: &egui::Response, name: &str, value: Option<&str>) {
    ui.ctx().accesskit_node_builder(r.id, |b| {
        b.set_label(name);
        if let Some(v) = value {
            b.set_value(v);
        }
    });
}

/// A drop-down list: `name` is its accessible name, `choices` the values and their labels.
/// True when the user picked another value.
pub(crate) fn combo<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    name: &str,
    current: &mut T,
    choices: &[(T, String)],
) -> bool {
    let before = *current;
    let shown = choices.iter().find(|(v, _)| *v == *current).map(|(_, l)| l.clone()).unwrap_or_default();
    let r = egui::ComboBox::from_id_salt(name).selected_text(shown.clone()).show_ui(ui, |ui| {
        for (v, l) in choices {
            ui.selectable_value(current, *v, l);
        }
    });
    name_widget(ui, &r.response, name, Some(&shown));
    *current != before
}

/// A one-line text field named for accessibility: (the user is done with it — Enter or focus
/// lost —, it has the focus).
pub(crate) fn text_field(ui: &mut egui::Ui, name: &str, text: &mut String, width: f32) -> (bool, bool) {
    let r = ui.add(egui::TextEdit::singleline(text).desired_width(width));
    name_widget(ui, &r, name, None);
    (r.lost_focus(), r.has_focus())
}
