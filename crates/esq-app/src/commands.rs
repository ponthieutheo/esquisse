//! Every menu command, with its label, default shortcut (SketchUp's), and whether it is
//! available or ticked. Menus, keyboard shortcuts and the command search all go through
//! here.

use eframe::egui::{self, Key, Modifiers};
use esq_core::intersect::IntersectWith;
use esq_core::model::EntityRef;

use crate::app::EsquisseApp;
use crate::style::FaceStyle;
use crate::tools::ToolId;

/// Panels of the default tray (Window › Default Tray).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Panel {
    EntityInfo,
    Materials,
    Components,
    Styles,
    Tags,
    Shadows,
    Scenes,
    MatchPhoto,
    Soften,
    Outliner,
    Fog,
    /// Window › Hatch Patterns (`hatch_panel.rs`).
    HatchPatterns,
    /// Window › Environments (`env_panel.rs`).
    Environments,
    /// Window › Overlays: the Ruby overlays (`Sketchup::Overlay`, `overlays_panel.rs`).
    Overlays,
    /// Window › Modeling Grid (`model_grid.rs`).
    ModelGrid,
    /// Window › Instructor (`dialogs/instructor.rs`).
    Instructor,
}

impl Panel {
    pub const ALL: [Panel; 16] = [
        Panel::EntityInfo,
        Panel::Materials,
        Panel::Components,
        Panel::Styles,
        Panel::Tags,
        Panel::Shadows,
        Panel::Scenes,
        Panel::MatchPhoto,
        Panel::Soften,
        Panel::Outliner,
        Panel::Fog,
        Panel::HatchPatterns,
        Panel::Environments,
        Panel::Overlays,
        Panel::ModelGrid,
        Panel::Instructor,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Panel::EntityInfo => "Infos sur l'entité",
            Panel::Materials => "Matières",
            Panel::Components => "Composants",
            Panel::Styles => "Styles",
            Panel::Tags => "Étiquettes",
            Panel::Shadows => "Ombres",
            Panel::Scenes => "Scènes",
            Panel::MatchPhoto => "Adapter une photo",
            Panel::Soften => "Adoucir les arêtes",
            Panel::Outliner => "Structure",
            Panel::Fog => "Brouillard",
            Panel::HatchPatterns => "Modèles de hachures",
            Panel::Environments => "Environnements",
            Panel::Overlays => "Incrustations",
            Panel::ModelGrid => "Grille de modélisation",
            Panel::Instructor => "Instructeur",
        }
    }
}

/// Camera › Standard Views.
pub const STD_VIEWS: [(&str, f64, f64); 7] = [
    ("Dessus", -90.0, 89.9),
    ("Dessous", -90.0, -89.9),
    ("Face", -90.0, 0.0),
    ("Derrière", 90.0, 0.0),
    ("Gauche", 180.0, 0.0),
    ("Droite", 0.0, 0.0),
    ("Iso", -45.0, 35.26),
];

/// Title of SketchUp's "export …" commands (commands.json).
pub fn export_label(f: esq_formats::ExportFormat) -> &'static str {
    use esq_formats::ExportFormat as E;
    match f {
        E::ThreeDs => "Exporter sous 3DS",
        E::Dae => "Exporter sous Collada",
        E::Dwg => "Exporter sous DWG (3D)",
        E::Dxf => "Exporter sous DXF (3D)",
        E::Fbx => "Exporter sous FBX",
        E::Kmz => "Exporter sous KMZ",
        E::Obj => "Exporter sous OBJ",
        E::Stl => "Exporter sous STL",
        E::Vrml => "Exporter sous VRML",
        E::Xsi => "Exporter sous XSI",
        E::Ifc => "Exporter sous IFC",
        E::Glb => "Exporter sous glTF",
        E::Usdz => "Exporter sous USDZ",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    // Fichier
    New,
    NewFromTemplate,
    Open,
    Save,
    SaveAs,
    SaveCopyAs,
    SaveAsTemplate,
    Revert,
    Import,
    ExportModel,
    /// "Exporter sous 3DS", "Exporter sous Collada"… (the export dialog with that format).
    ExportAs(esq_formats::ExportFormat),
    ExportImage,
    ExportSlice,
    ExportAnimation,
    Print,
    Toolbars,
    ComponentOptions,
    ComponentAttributes,
    RubyConsole,
    ExtensionManager,
    Quit,
    // Édition
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PasteInPlace,
    Delete,
    DeleteGuides,
    PurgeUnused,
    SelectAll,
    SelectNone,
    InvertSelection,
    Hide,
    RevealSelection,
    RevealLast,
    RevealAll,
    Lock,
    UnlockSelection,
    UnlockAll,
    MakeComponent,
    MakeGroup,
    CloseGroup,
    Intersect(IntersectWith),
    // Entity submenu (also the right-click menu)
    EditGroup,
    Explode,
    MakeUnique,
    ReverseFaces,
    ZoomSelection,
    // Section plane (right-click menu)
    ReverseSection,
    ActiveCut,
    AlignView,
    GroupFromSlice,
    // Affichage
    HiddenGeometry,
    HiddenObjects,
    Axes,
    Guides,
    Shadows,
    Fog,
    Edges,
    BackEdges,
    Profiles,
    Extension,
    Xray,
    Faces(FaceStyle),
    HideRest,
    HideSimilar,
    SceneTabs,
    SectionPlanes,
    SectionCuts,
    SectionFill,
    AddScene,
    UpdateScene,
    DeleteScene,
    PrevScene,
    NextScene,
    PlayScenes,
    AnimSettings,
    // Caméra
    CamPrevious,
    CamNext,
    StdView(usize),
    Parallel,
    Perspective,
    TwoPoint,
    ZoomExtents,
    // Adapter une photo (Match Photo)
    PhotoNew,
    PhotoEdit,
    PhotoZoom,
    // Display effects (render.rs)
    DepthCue,
    AmbientOcclusion,
    Photoreal,
    /// Display commands without a menu entry (see `view_cmds.rs`).
    View(crate::view_cmds::ViewCmd),
    /// View › Comments: shows or hides the comments' pins (`comments`).
    Comments,
    // Outils (and Dessiner, and the camera tools)
    Tool(ToolId),
    Text3d,
    /// Texture of a face: Position, Reset, Projected, Make Unique (`texturing.rs`).
    Tex(crate::texturing::TexCmd),
    // Fenêtre
    Panel(Panel),
    ToggleTray,
    ModelInfo,
    Preferences,
    /// Opens Window › Preferences at one page (SketchUp's "… preferences" commands).
    PrefsPage(crate::dialogs::PrefPage),
    /// Back to SketchUp's shortcuts.
    ResetShortcuts,
    /// New Tray, Manage Trays… (`dialogs::Dlg`).
    Dialog(crate::dialogs::Dlg),
    // Aide
    Welcome,
    About,
    Search,
    /// Commands on the selection read from SketchUp (`edit_ops.rs`).
    Op(crate::edit_ops::Op),
    /// Commands of the right-click menu only (`context_menu.rs`).
    Ctx(crate::context_menu::CtxCmd),
}

impl Cmd {
    pub fn label(self) -> String {
        let s = match self {
            Cmd::New => "Nouveau",
            Cmd::NewFromTemplate => "Nouveau à partir du modèle type...",
            Cmd::Open => "Ouvrir...",
            Cmd::Save => "Enregistrer",
            Cmd::SaveAs => "Enregistrer sous...",
            Cmd::SaveCopyAs => "Enregistrer une copie sous...",
            Cmd::SaveAsTemplate => "Enregistrer comme modèle type...",
            Cmd::Revert => "Revenir",
            Cmd::Import => "Importer...",
            Cmd::ExportModel => "Modèle 3D...",
            Cmd::ExportAs(f) => return export_label(f).to_string(),
            Cmd::ExportImage => "Graphique 2D...",
            Cmd::ExportSlice => "Tranche de section...",
            Cmd::ExportAnimation => "Animation...",
            Cmd::Print => "Imprimer...",
            Cmd::Toolbars => "Barres d'outils...",
            Cmd::ComponentOptions => "Options du composant",
            Cmd::ComponentAttributes => "Attributs du composant",
            Cmd::RubyConsole => "Console Ruby",
            Cmd::ExtensionManager => "Gestionnaire d'extensions",
            Cmd::Quit => "Quitter",
            Cmd::Undo => "Annuler",
            Cmd::Redo => "Rétablir",
            Cmd::Cut => "Couper",
            Cmd::Copy => "Copier",
            Cmd::Paste => "Coller",
            Cmd::PasteInPlace => "Coller sur place",
            Cmd::Delete => "Supprimer",
            Cmd::DeleteGuides => "Supprimer les guides",
            Cmd::PurgeUnused => "Supprimer les actifs inutilisés",
            Cmd::SelectAll => "Tout sélectionner",
            Cmd::SelectNone => "Sélectionner Aucun",
            Cmd::InvertSelection => "Inverser la sélection",
            Cmd::Hide => "Masquer",
            Cmd::RevealSelection => "Sélection",
            Cmd::RevealLast => "Dernière",
            Cmd::RevealAll => "Tout",
            Cmd::Lock => "Verrouiller",
            Cmd::UnlockSelection => "Sélection",
            Cmd::UnlockAll => "Tout",
            Cmd::MakeComponent => "Créer un composant...",
            Cmd::MakeGroup => "Créer un groupe",
            Cmd::CloseGroup => "Fermer le groupe/composant",
            Cmd::Intersect(IntersectWith::Model) => "Avec le modèle",
            Cmd::Intersect(IntersectWith::Selection) => "Avec la sélection",
            Cmd::Intersect(IntersectWith::Context) => "Avec le contexte",
            Cmd::EditGroup => "Modifier",
            Cmd::Explode => "Éclater",
            Cmd::MakeUnique => "Rendre unique",
            Cmd::ReverseFaces => "Inverser les faces",
            Cmd::ZoomSelection => "Zoom sur la sélection",
            Cmd::HiddenGeometry => "Géométrie cachée",
            Cmd::HiddenObjects => "Objets Masqués",
            Cmd::Axes => "Axes",
            Cmd::Guides => "Guides",
            Cmd::Shadows => "Ombres",
            Cmd::Fog => "Brouillard",
            Cmd::Edges => "Arêtes",
            Cmd::BackEdges => "Arêtes arrières",
            Cmd::Profiles => "Profils",
            Cmd::Extension => "Extension",
            Cmd::Xray => "Transparence",
            Cmd::Faces(f) => f.label(),
            Cmd::HideRest => "Masquer le reste du modèle",
            Cmd::HideSimilar => "Masquer les composants similaires",
            Cmd::SceneTabs => "Onglets de scène",
            Cmd::SectionPlanes => "Plans de section",
            Cmd::SectionCuts => "Coupes",
            Cmd::SectionFill => "Remplissage coupe",
            Cmd::ReverseSection => "Inverser",
            Cmd::ActiveCut => "Activer la coupe",
            Cmd::AlignView => "Aligner la vue",
            Cmd::GroupFromSlice => "Créer un groupe à partir de la tranche",
            Cmd::AddScene => "Ajouter une scène",
            Cmd::UpdateScene => "Actualiser la scène",
            Cmd::DeleteScene => "Supprimer la scène",
            Cmd::PrevScene => "Scène précédente",
            Cmd::NextScene => "Scène suivante",
            Cmd::PlayScenes => "Lire",
            Cmd::AnimSettings => "Paramètres",
            Cmd::CamPrevious => "Précédent",
            Cmd::CamNext => "Suivant",
            Cmd::StdView(i) => STD_VIEWS[i].0,
            Cmd::Parallel => "Projection parallèle",
            Cmd::Perspective => "Perspective",
            Cmd::TwoPoint => "Perspective à deux points",
            Cmd::DepthCue => "Effet de profondeur",
            Cmd::AmbientOcclusion => "Occlusion ambiante",
            Cmd::Photoreal => "Matériaux photoréalistes",
            Cmd::View(v) => v.label(),
            Cmd::Comments => "Commentaires",
            Cmd::ZoomExtents => "Zoom étendu",
            Cmd::PhotoNew => "Adapter nouvelle photo...",
            Cmd::PhotoEdit => "Modifier photo adaptée",
            Cmd::PhotoZoom => "Zoom sur la photo",
            Cmd::Tool(t) => t.label(),
            Cmd::Text3d => "Texte 3D...",
            Cmd::Tex(c) => c.label(),
            Cmd::Panel(p) => p.label(),
            Cmd::ToggleTray => "Masquer le plateau",
            Cmd::ModelInfo => "Infos sur le modèle",
            Cmd::Preferences => "Préférences",
            Cmd::PrefsPage(p) => p.command_label(),
            Cmd::ResetShortcuts => "Réinitialiser les raccourcis",
            Cmd::Dialog(d) => d.label(),
            Cmd::Welcome => "Bienvenue dans Esquisse...",
            Cmd::About => "À propos d'Esquisse...",
            Cmd::Search => "Rechercher dans Esquisse",
            Cmd::Op(o) => o.label(),
            Cmd::Ctx(c) => c.label(),
        };
        s.to_string()
    }

    /// The command's shortcut (the first one, as menus show it): Preferences › Shortcuts.
    pub fn shortcut(self) -> Option<(Modifiers, Key)> {
        crate::shortcuts::bindings(self).first().copied()
    }

    /// SketchUp's default shortcut.
    pub fn default_shortcut(self) -> Option<(Modifiers, Key)> {
        let none = Modifiers::NONE;
        let ctrl = Modifiers::COMMAND;
        let shift = Modifiers::SHIFT;
        Some(match self {
            Cmd::New => (ctrl, Key::N),
            Cmd::Open => (ctrl, Key::O),
            Cmd::Save => (ctrl, Key::S),
            Cmd::Print => (ctrl, Key::P),
            Cmd::Undo => (ctrl, Key::Z),
            Cmd::Redo => (ctrl, Key::Y),
            Cmd::Cut => (ctrl, Key::X),
            Cmd::Copy => (ctrl, Key::C),
            Cmd::Paste => (ctrl, Key::V),
            Cmd::Delete => (none, Key::Delete),
            Cmd::SelectAll => (ctrl, Key::A),
            Cmd::SelectNone => (ctrl, Key::T),
            Cmd::InvertSelection => (ctrl | shift, Key::I),
            Cmd::MakeComponent => (none, Key::G),
            Cmd::MakeGroup => (ctrl, Key::G),
            Cmd::BackEdges => (none, Key::K),
            Cmd::ZoomExtents => (shift, Key::Z),
            Cmd::Search => (shift, Key::S),
            Cmd::Tool(t) => (
                none,
                match t {
                    ToolId::Select => Key::Space,
                    ToolId::Line => Key::L,
                    ToolId::Arc => Key::A,
                    ToolId::Rect => Key::R,
                    ToolId::Circle => Key::C,
                    ToolId::PushPull => Key::P,
                    ToolId::Move => Key::M,
                    ToolId::Rotate => Key::Q,
                    ToolId::Scale => Key::S,
                    ToolId::Offset => Key::F,
                    ToolId::Eraser => Key::E,
                    ToolId::Paint => Key::B,
                    ToolId::Tape => Key::T,
                    ToolId::Orbit => Key::O,
                    ToolId::Pan => Key::H,
                    ToolId::Zoom => Key::Z,
                    _ => return None,
                },
            ),
            _ => return None,
        })
    }

    pub fn shortcut_text(self) -> String {
        self.shortcut().map(crate::shortcuts::text).unwrap_or_default()
    }
}

impl EsquisseApp {
    fn has_sel(&self) -> bool {
        !self.doc.sel.is_empty()
    }

    fn sel_instances(&self) -> Vec<esq_core::model::InstanceId> {
        self.doc.sel.iter().filter_map(|e| if let EntityRef::Instance(i) = e { Some(*i) } else { None }).collect()
    }

    /// The section plane selected on its own, if that is the selection.
    pub fn sel_section(&self) -> Option<esq_core::model::SectionId> {
        match self.doc.sel.as_slice() {
            [EntityRef::Section(s)] => Some(*s),
            _ => None,
        }
    }

    pub fn cmd_enabled(&self, cmd: Cmd) -> bool {
        let ents = self.doc.ents();
        match cmd {
            Cmd::ReverseSection | Cmd::ActiveCut | Cmd::AlignView | Cmd::GroupFromSlice => self.sel_section().is_some(),
            Cmd::Revert => self.doc.path.is_some() && self.doc.dirty,
            Cmd::ExportSlice => crate::export_section::section_to_export(self).is_some(),
            Cmd::ComponentOptions | Cmd::ComponentAttributes => crate::attrs::selected_instance(self).is_some(),
            Cmd::ExportAnimation => !self.doc.model.scenes.is_empty() && self.ui_state.extra.jobs.anim.is_none(),
            Cmd::Undo => self.doc.undo_label().is_some(),
            Cmd::Redo => self.doc.redo_label().is_some(),
            Cmd::Cut | Cmd::Copy | Cmd::Delete | Cmd::Hide | Cmd::SelectNone | Cmd::ZoomSelection => self.has_sel(),
            Cmd::Paste | Cmd::PasteInPlace => self.can_paste(),
            Cmd::DeleteGuides => !ents.guides.is_empty(),
            Cmd::MakeComponent | Cmd::MakeGroup | Cmd::Intersect(_) => self.has_sel(),
            Cmd::RevealSelection => self.doc.sel.iter().any(|e| self.is_hidden(*e)),
            Cmd::RevealLast => !self.last_hidden.is_empty(),
            Cmd::Lock => self.sel_instances().iter().any(|i| !ents.instances[*i].locked),
            Cmd::UnlockSelection => self.sel_instances().iter().any(|i| ents.instances[*i].locked),
            Cmd::UnlockAll => ents.instances.values().any(|i| i.locked),
            Cmd::CloseGroup => !self.doc.ctx.is_empty(),
            Cmd::EditGroup | Cmd::MakeUnique => matches!(self.doc.sel.as_slice(), [EntityRef::Instance(_)]),
            Cmd::Explode => !self.sel_instances().is_empty(),
            Cmd::ReverseFaces => self.doc.sel.iter().any(|e| matches!(e, EntityRef::Face(_))),
            Cmd::CamPrevious => !self.cam_back.is_empty(),
            Cmd::UpdateScene | Cmd::DeleteScene => self.active_scene.is_some_and(|i| i < self.doc.model.scenes.len()),
            Cmd::PrevScene | Cmd::NextScene | Cmd::PlayScenes => !self.doc.model.scenes.is_empty(),
            Cmd::CamNext => !self.cam_fwd.is_empty(),
            Cmd::PhotoEdit | Cmd::PhotoZoom => self.photo_cmd_enabled(),
            Cmd::Op(o) => self.op_enabled(o),
            Cmd::Ctx(c) => crate::context_menu::enabled(self, c),
            // SketchUp disables it when looking straight up or down.
            Cmd::TwoPoint => self.cam.pitch.cos().abs() > 1e-3,
            Cmd::Tex(c) => crate::texturing::enabled(self, c),
            Cmd::View(v) => self.view_enabled(v),
            _ => true,
        }
    }

    /// `Some(ticked)` for the commands shown with a check mark.
    pub fn cmd_checked(&self, cmd: Cmd) -> Option<bool> {
        let s = &self.style;
        Some(match cmd {
            Cmd::HiddenGeometry => s.hidden_geometry,
            Cmd::HiddenObjects => s.hidden_objects,
            Cmd::Axes => s.axes,
            Cmd::Guides => s.guides,
            Cmd::Shadows => self.doc.model.shadow.display,
            Cmd::Fog => s.fog,
            Cmd::Edges => s.edges,
            Cmd::BackEdges => s.back_edges,
            Cmd::Profiles => s.profiles,
            Cmd::Extension => s.extension,
            Cmd::Xray => s.xray,
            Cmd::Faces(f) => s.faces == f,
            Cmd::HideRest => s.hide_rest,
            Cmd::HideSimilar => s.hide_similar,
            Cmd::SceneTabs => s.scene_tabs,
            Cmd::SectionPlanes => s.section_planes,
            Cmd::SectionCuts => s.section_cuts,
            Cmd::SectionFill => s.section_fill,
            Cmd::ActiveCut => self.sel_section().is_some_and(|x| self.doc.ents().active_section == Some(x)),
            Cmd::Parallel => !self.cam.perspective,
            Cmd::Perspective => self.cam.perspective,
            Cmd::TwoPoint => self.cam.is_two_point(),
            Cmd::DepthCue => s.depth_cue,
            Cmd::AmbientOcclusion => s.ambient_occlusion,
            Cmd::Photoreal => s.photoreal,
            Cmd::View(v) => self.view_checked(v),
            Cmd::Comments => crate::comments::pins_shown(self),
            Cmd::Tool(t) => self.tool.id() == t,
            Cmd::Panel(p) => self.ui_state.panels.contains(&p),
            Cmd::ToggleTray => !self.ui_state.tray,
            Cmd::Op(o) => return self.op_checked(o),
            Cmd::Tex(c) => return crate::texturing::checked(self, c),
            Cmd::Ctx(c) => return crate::context_menu::checked(self, c),
            _ => return None,
        })
    }

    pub fn run(&mut self, ctx: &egui::Context, cmd: Cmd) {
        if !self.cmd_enabled(cmd) {
            return;
        }
        let restyle = |app: &mut EsquisseApp, f: &dyn Fn(&mut crate::style::ViewStyle)| {
            f(&mut app.style);
            app.doc.rev += 1;
        };
        match cmd {
            Cmd::New => self.ui_state.ask_new = true,
            Cmd::NewFromTemplate => self.ui_state.templates = true,
            Cmd::Open => self.open_dialog(),
            Cmd::Save => self.save(ctx, false),
            Cmd::SaveAs => self.save(ctx, true),
            Cmd::SaveCopyAs => self.save_copy(ctx),
            Cmd::SaveAsTemplate => self.ui_state.save_template = Some(String::new()),
            Cmd::Revert => self.revert(),
            Cmd::Import => crate::formats3d::open_import(self, ctx),
            Cmd::ExportModel => crate::formats3d::open_export(self, ctx, None),
            Cmd::ExportAs(f) => crate::formats3d::open_export(self, ctx, Some(f)),
            Cmd::ExportImage => crate::export2d::start(self, ctx),
            Cmd::ExportSlice => crate::export_section::start(self, ctx),
            Cmd::ExportAnimation => crate::export_anim::start(self, ctx),
            Cmd::Print => self.ui_state.extra.print = Some(Default::default()),
            Cmd::Toolbars => self.ui_state.extra.toolbars.open = true,
            Cmd::ComponentOptions => self.ui_state.extra.attrs = Some(crate::attrs::AttrDialog::new(true)),
            Cmd::ComponentAttributes => self.ui_state.extra.attrs = Some(crate::attrs::AttrDialog::new(false)),
            Cmd::RubyConsole => {
                self.ui_state.extra.ruby.console_open = true;
                self.ui_state.extra.ruby.focus_input();
            }
            Cmd::ExtensionManager => self.ui_state.extra.ruby.manager_open = true,
            Cmd::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Cmd::Undo => {
                self.cancel_tool();
                self.doc.undo();
            }
            Cmd::Redo => self.doc.redo(),
            Cmd::Cut => self.cut(),
            Cmd::Copy => self.copy(),
            Cmd::Paste => self.paste(true),
            Cmd::PasteInPlace => self.paste(false),
            Cmd::Delete => self.erase_selection(),
            Cmd::DeleteGuides => self.delete_guides(),
            Cmd::PurgeUnused => {
                let mut n = 0;
                self.doc.commit("Supprimer les actifs inutilisés", |m, _| n = m.purge_unused());
                self.say(ctx, format!("{n} élément(s) inutilisé(s) supprimé(s)"));
            }
            Cmd::SelectAll => self.select_all(),
            Cmd::SelectNone => self.doc.select(Vec::new()),
            Cmd::InvertSelection => self.invert_selection(),
            Cmd::Hide => self.hide_selection(true),
            Cmd::RevealSelection => self.hide_selection(false),
            Cmd::RevealLast => self.reveal_last(),
            Cmd::RevealAll => self.unhide_all(),
            Cmd::Lock => self.set_locked(true, false),
            Cmd::UnlockSelection => self.set_locked(false, false),
            Cmd::UnlockAll => self.set_locked(false, true),
            Cmd::MakeComponent => crate::make_component::open(self),
            Cmd::MakeGroup => self.make_group(true),
            Cmd::CloseGroup => self.doc.exit(),
            Cmd::Intersect(with) => self.intersect(ctx, with),
            Cmd::EditGroup => self.enter_selected(),
            Cmd::Explode => self.explode_selection(),
            Cmd::MakeUnique => {
                if let [EntityRef::Instance(i)] = self.doc.sel.as_slice() {
                    let i = *i;
                    self.doc.commit("Rendre unique", |m, d| {
                        m.make_unique(d, i);
                    });
                }
            }
            Cmd::ReverseFaces => self.reverse_selected_faces(),
            Cmd::ZoomSelection => self.zoom_selection(),
            Cmd::HiddenGeometry => restyle(self, &|s| s.hidden_geometry = !s.hidden_geometry),
            Cmd::HiddenObjects => restyle(self, &|s| s.hidden_objects = !s.hidden_objects),
            Cmd::Axes => restyle(self, &|s| s.axes = !s.axes),
            Cmd::Guides => restyle(self, &|s| s.guides = !s.guides),
            Cmd::Shadows => self.toggle_shadows(),
            Cmd::Fog => self.toggle_fog(),
            Cmd::Edges => restyle(self, &|s| s.edges = !s.edges),
            Cmd::BackEdges => restyle(self, &|s| s.back_edges = !s.back_edges),
            Cmd::Profiles => restyle(self, &|s| s.profiles = !s.profiles),
            Cmd::Extension => restyle(self, &|s| s.extension = !s.extension),
            Cmd::Xray => restyle(self, &|s| s.xray = !s.xray),
            Cmd::Faces(f) => restyle(self, &|s| s.faces = f),
            Cmd::HideRest => restyle(self, &|s| s.hide_rest = !s.hide_rest),
            Cmd::HideSimilar => restyle(self, &|s| s.hide_similar = !s.hide_similar),
            Cmd::SceneTabs => restyle(self, &|s| s.scene_tabs = !s.scene_tabs),
            Cmd::SectionPlanes => restyle(self, &|s| s.section_planes = !s.section_planes),
            Cmd::SectionCuts => restyle(self, &|s| s.section_cuts = !s.section_cuts),
            Cmd::SectionFill => restyle(self, &|s| s.section_fill = !s.section_fill),
            Cmd::ReverseSection | Cmd::ActiveCut | Cmd::AlignView | Cmd::GroupFromSlice => self.section_cmd(cmd),
            Cmd::AddScene => self.add_scene(),
            Cmd::UpdateScene => self.update_scene(),
            Cmd::DeleteScene => self.delete_scene(),
            Cmd::PrevScene => self.step_scene(-1, ctx.input(|i| i.time)),
            Cmd::NextScene => self.step_scene(1, ctx.input(|i| i.time)),
            Cmd::PlayScenes => self.toggle_play(ctx.input(|i| i.time)),
            // View › Animation › Settings opens Model Info › Animation, as in SketchUp.
            Cmd::AnimSettings => {
                self.ui_state.model_info = true;
                self.ui_state.extra.dialogs.info_page = crate::dialogs::InfoPage::Animation;
            }
            Cmd::CamPrevious => self.camera_back(),
            Cmd::CamNext => self.camera_forward(),
            Cmd::StdView(i) => {
                self.remember_view();
                let (_, yaw, pitch) = STD_VIEWS[i];
                self.cam.set_view(yaw, pitch);
                self.zoom_extents();
            }
            Cmd::Parallel => {
                self.remember_view();
                self.cam.perspective = false;
            }
            Cmd::Perspective => {
                self.remember_view();
                self.cam.perspective = true;
                self.cam.disable_two_point();
            }
            Cmd::TwoPoint => {
                self.remember_view();
                if self.cam.is_two_point() {
                    self.cam.disable_two_point();
                } else {
                    self.cam.enable_two_point();
                }
            }
            Cmd::DepthCue => restyle(self, &|s| s.depth_cue = !s.depth_cue),
            Cmd::AmbientOcclusion => restyle(self, &|s| s.ambient_occlusion = !s.ambient_occlusion),
            // Turned on, SketchUp first says what photoreal materials need (`warnings.rs`).
            Cmd::Photoreal if !self.style.photoreal => crate::warnings::turn_photoreal_on(self),
            Cmd::Photoreal => restyle(self, &|s| s.photoreal = false),
            Cmd::View(v) => self.run_view(ctx, v),
            Cmd::Comments => crate::comments::toggle_pins(self),
            Cmd::ZoomExtents => {
                self.remember_view();
                self.zoom_extents();
            }
            Cmd::PhotoNew => self.photo_new(),
            Cmd::PhotoEdit => self.photo_edit(),
            Cmd::PhotoZoom => self.photo_zoom(),
            Cmd::Tool(t) => {
                // Sandbox tools that work on the selection act at once.
                if let Some(r) = crate::tools::sandbox::on_command(t, &mut self.doc) {
                    if let Err(m) = r {
                        self.say(ctx, m);
                    }
                    self.set_tool(ToolId::Select);
                    return;
                }
                // With two solids already selected, the solid tools act at once.
                let sel = self.sel_instances();
                match crate::tools::SolidKind::of(t) {
                    Some(kind) if sel.len() >= 2 => {
                        let msg = match kind {
                            crate::tools::SolidKind::OuterShell => {
                                let mut made = None;
                                let r = self.doc.try_commit(t.label(), |m, d| {
                                    made = Some(m.outer_shell(d, &sel)?);
                                    Ok::<(), String>(())
                                });
                                if let Some(i) = made {
                                    self.doc.select(vec![EntityRef::Instance(i)]);
                                }
                                r.err()
                            }
                            _ => kind.run(&mut self.doc, sel[0], sel[1]).err(),
                        };
                        if let Some(m) = msg {
                            self.say(ctx, m);
                        }
                    }
                    _ => self.set_tool(t),
                }
            }
            Cmd::Text3d => {
                let mut st = self
                    .ui_state
                    .text3d_last
                    .clone()
                    .unwrap_or_else(|| crate::text3d::Text3dState::new(self.doc.model.units));
                st.text = "Entrez le texte".into();
                st.error = None;
                self.ui_state.text3d = Some(st);
            }
            Cmd::Tex(c) => crate::texturing::run(self, c),
            Cmd::Panel(p) => {
                if !self.ui_state.panels.remove(&p) {
                    self.ui_state.panels.insert(p);
                    self.ui_state.tray = true;
                    self.ui_state.unfold = Some(p);
                }
            }
            Cmd::ToggleTray => self.ui_state.tray = !self.ui_state.tray,
            Cmd::ModelInfo => self.ui_state.model_info = true,
            Cmd::Preferences => self.ui_state.preferences = true,
            Cmd::PrefsPage(p) => {
                self.ui_state.preferences = true;
                self.ui_state.extra.dialogs.pref_page = p;
            }
            Cmd::ResetShortcuts => {
                crate::shortcuts::reset_all();
                self.say(ctx, "Raccourcis par défaut rétablis");
            }
            Cmd::Dialog(d) => crate::dialogs::open(self, ctx, d),
            Cmd::Welcome => self.ui_state.welcome = true,
            Cmd::About => self.ui_state.show_about = true,
            Cmd::Search => {
                self.ui_state.search = Some(String::new());
            }
            Cmd::Op(o) => self.run_op(ctx, o),
            Cmd::Ctx(c) => crate::context_menu::run(self, ctx, c),
        }
    }

    /// The command bound to a key press, if any.
    pub fn cmd_for_key(key: Key, mods: Modifiers) -> Option<Cmd> {
        crate::shortcuts::cmd_for(key, mods)
    }
}
