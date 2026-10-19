//! The menu bar, laid out like SketchUp's. Entries that Esquisse cannot do yet are shown
//! greyed out; Trimble's online services (Trimble Connect, 3D Warehouse, LayOut…) are left
//! out.

use eframe::egui;
use esq_core::intersect::IntersectWith;

use crate::app::EsquisseApp;
use crate::commands::{Cmd, Panel};
use crate::edit_ops::Op;
use crate::style::FaceStyle;
use crate::tools::ToolId;

pub enum M {
    C(Cmd),
    Sep,
    Sub(&'static str, &'static [M]),
    /// In SketchUp, not in Esquisse yet.
    Todo(&'static str),
    /// Recently opened files.
    Recent,
    /// Menu entries registered by Ruby plugins.
    Plugins,
    /// Entries plugins put in Extensions › Developer.
    PluginDeveloper,
    /// The submenu named after the selection ("Face", "3 entités"…).
    Entity,
    /// The trays made with Window › New Tray.
    Trays,
    /// The saved workspaces (Window › Workspace).
    Workspaces,
}

use M::*;

pub const MENUS: &[(&str, &[M])] = &[
    (
        "Fichier",
        &[
            C(Cmd::New),
            C(Cmd::NewFromTemplate),
            C(Cmd::Open),
            Sep,
            C(Cmd::Save),
            C(Cmd::SaveAs),
            C(Cmd::SaveCopyAs),
            C(Cmd::SaveAsTemplate),
            C(Cmd::Revert),
            Sep,
            C(Cmd::Import),
            Sub("Exporter", &[C(Cmd::ExportModel), C(Cmd::ExportImage), C(Cmd::ExportSlice), C(Cmd::ExportAnimation)]),
            Sep,
            C(Cmd::Print),
            Sep,
            Recent,
            Sep,
            C(Cmd::Quit),
        ],
    ),
    (
        "Édition",
        &[
            C(Cmd::Undo),
            C(Cmd::Redo),
            Sep,
            C(Cmd::Cut),
            C(Cmd::Copy),
            C(Cmd::Paste),
            C(Cmd::PasteInPlace),
            C(Cmd::Delete),
            C(Cmd::DeleteGuides),
            Sep,
            C(Cmd::PurgeUnused),
            Sep,
            C(Cmd::SelectAll),
            C(Cmd::SelectNone),
            C(Cmd::InvertSelection),
            Sep,
            C(Cmd::Hide),
            Sub("Révéler", &[C(Cmd::RevealSelection), C(Cmd::RevealLast), C(Cmd::RevealAll)]),
            Sep,
            C(Cmd::Lock),
            Sub("Déverrouiller", &[C(Cmd::UnlockSelection), C(Cmd::UnlockAll)]),
            Sep,
            C(Cmd::MakeComponent),
            C(Cmd::MakeGroup),
            C(Cmd::CloseGroup),
            Sep,
            Sub(
                "Intersection des faces",
                &[
                    C(Cmd::Intersect(IntersectWith::Model)),
                    C(Cmd::Intersect(IntersectWith::Selection)),
                    C(Cmd::Intersect(IntersectWith::Context)),
                ],
            ),
            C(Cmd::Op(Op::WeldEdges)),
            Sep,
            Entity,
        ],
    ),
    (
        "Affichage",
        &[
            C(Cmd::Toolbars),
            C(Cmd::SceneTabs),
            Sep,
            C(Cmd::HiddenGeometry),
            C(Cmd::HiddenObjects),
            C(Cmd::SectionPlanes),
            C(Cmd::SectionCuts),
            C(Cmd::SectionFill),
            C(Cmd::Axes),
            C(Cmd::Guides),
            C(Cmd::View(crate::view_cmds::ViewCmd::Grid)),
            C(Cmd::Comments),
            Sep,
            C(Cmd::Shadows),
            C(Cmd::Fog),
            Sep,
            Sub(
                "Style d'arêtes",
                &[C(Cmd::Edges), C(Cmd::BackEdges), Sep, C(Cmd::Profiles), C(Cmd::DepthCue), C(Cmd::Extension)],
            ),
            Sub(
                "Style de faces",
                &[
                    C(Cmd::Xray),
                    C(Cmd::AmbientOcclusion),
                    Sep,
                    C(Cmd::Faces(FaceStyle::Wireframe)),
                    C(Cmd::Faces(FaceStyle::HiddenLine)),
                    C(Cmd::Faces(FaceStyle::Shaded)),
                    C(Cmd::Faces(FaceStyle::ShadedWithTextures)),
                    C(Cmd::Faces(FaceStyle::Monochrome)),
                    C(Cmd::Photoreal),
                ],
            ),
            Sub("Modification du composant", &[C(Cmd::HideRest), C(Cmd::HideSimilar)]),
            Sep,
            Sub(
                "Animation",
                &[
                    C(Cmd::AddScene),
                    C(Cmd::UpdateScene),
                    C(Cmd::DeleteScene),
                    Sep,
                    C(Cmd::PrevScene),
                    C(Cmd::NextScene),
                    Sep,
                    C(Cmd::PlayScenes),
                    C(Cmd::AnimSettings),
                ],
            ),
            Sep,
            C(Cmd::Tool(ToolId::North)),
            Todo("Épingle de localisation"),
        ],
    ),
    (
        "Caméra",
        &[
            C(Cmd::CamPrevious),
            C(Cmd::CamNext),
            Sub(
                "Vues standard",
                &[
                    C(Cmd::StdView(0)),
                    C(Cmd::StdView(1)),
                    C(Cmd::StdView(2)),
                    C(Cmd::StdView(3)),
                    C(Cmd::StdView(4)),
                    C(Cmd::StdView(5)),
                    C(Cmd::StdView(6)),
                ],
            ),
            Sep,
            C(Cmd::Parallel),
            C(Cmd::Perspective),
            C(Cmd::TwoPoint),
            Sep,
            C(Cmd::PhotoNew),
            C(Cmd::PhotoEdit),
            Sep,
            C(Cmd::Tool(ToolId::Orbit)),
            C(Cmd::Tool(ToolId::Pan)),
            C(Cmd::Tool(ToolId::Zoom)),
            C(Cmd::Tool(ToolId::Fov)),
            C(Cmd::Tool(ToolId::ZoomWindow)),
            C(Cmd::ZoomExtents),
            C(Cmd::PhotoZoom),
            Sep,
            C(Cmd::Tool(ToolId::PositionCamera)),
            C(Cmd::Tool(ToolId::Walk)),
            C(Cmd::Tool(ToolId::LookAround)),
            C(Cmd::Tool(ToolId::Igloo)),
        ],
    ),
    (
        "Dessiner",
        &[
            Sub("Lignes", &[C(Cmd::Tool(ToolId::Line)), C(Cmd::Tool(ToolId::Freehand))]),
            Sub(
                "Arcs",
                &[
                    C(Cmd::Tool(ToolId::ArcCenter)),
                    C(Cmd::Tool(ToolId::Arc)),
                    C(Cmd::Tool(ToolId::Arc3)),
                    C(Cmd::Tool(ToolId::Pie)),
                ],
            ),
            Sub(
                "Formes",
                &[
                    C(Cmd::Tool(ToolId::Rect)),
                    C(Cmd::Tool(ToolId::RotRect)),
                    C(Cmd::Tool(ToolId::Circle)),
                    C(Cmd::Tool(ToolId::Polygon)),
                ],
            ),
            Sep,
            Sub("Bac à sable", &[C(Cmd::Tool(ToolId::FromContours)), C(Cmd::Tool(ToolId::FromScratch))]),
        ],
    ),
    (
        "Outils",
        &[
            C(Cmd::Tool(ToolId::Select)),
            C(Cmd::Tool(ToolId::Lasso)),
            C(Cmd::Tool(ToolId::Eraser)),
            C(Cmd::Tool(ToolId::Paint)),
            C(Cmd::Tool(ToolId::TagTool)),
            C(Cmd::Tool(ToolId::Classifier)),
            Sep,
            C(Cmd::Tool(ToolId::Move)),
            C(Cmd::Op(Op::MoveRotateGrips)),
            C(Cmd::Tool(ToolId::Rotate)),
            C(Cmd::Tool(ToolId::Scale)),
            C(Cmd::Tool(ToolId::Flip)),
            Sep,
            C(Cmd::Tool(ToolId::PushPull)),
            C(Cmd::Tool(ToolId::FollowMe)),
            C(Cmd::Tool(ToolId::Offset)),
            Sep,
            C(Cmd::Tool(ToolId::OuterShell)),
            Sub(
                "Solides",
                &[
                    C(Cmd::Tool(ToolId::SolidIntersect)),
                    C(Cmd::Tool(ToolId::SolidUnion)),
                    C(Cmd::Tool(ToolId::SolidSubtract)),
                    C(Cmd::Tool(ToolId::SolidTrim)),
                    C(Cmd::Tool(ToolId::SolidSplit)),
                ],
            ),
            Sep,
            C(Cmd::Tool(ToolId::Tape)),
            C(Cmd::Tool(ToolId::Protractor)),
            C(Cmd::Tool(ToolId::AxesTool)),
            Sep,
            C(Cmd::Tool(ToolId::Dimension)),
            C(Cmd::Tool(ToolId::Text)),
            C(Cmd::Text3d),
            Sep,
            C(Cmd::Tool(ToolId::SectionPlane)),
            C(Cmd::Tool(ToolId::AddSnap)),
            C(Cmd::Tool(ToolId::Light)),
            Sep,
            Todo("Interagir"),
            Sep,
            Sub(
                "Bac à sable",
                &[
                    C(Cmd::Tool(ToolId::Smoove)),
                    C(Cmd::Tool(ToolId::Stamp)),
                    C(Cmd::Tool(ToolId::Drape)),
                    Sep,
                    C(Cmd::Tool(ToolId::AddDetail)),
                    C(Cmd::Tool(ToolId::FlipEdge)),
                ],
            ),
        ],
    ),
    (
        "Fenêtre",
        &[
            Sub(
                "Palette par défaut",
                &[
                    C(Cmd::ToggleTray),
                    Sep,
                    C(Cmd::Panel(Panel::EntityInfo)),
                    Sep,
                    C(Cmd::Panel(Panel::Materials)),
                    C(Cmd::Panel(Panel::Components)),
                    C(Cmd::Panel(Panel::Styles)),
                    C(Cmd::Panel(Panel::Environments)),
                    Sep,
                    C(Cmd::Panel(Panel::Tags)),
                    C(Cmd::Panel(Panel::Scenes)),
                    Sep,
                    C(Cmd::Panel(Panel::Shadows)),
                    C(Cmd::Op(Op::SetSunLocation)),
                    C(Cmd::Panel(Panel::Fog)),
                    C(Cmd::Panel(Panel::MatchPhoto)),
                    C(Cmd::Panel(Panel::HatchPatterns)),
                    C(Cmd::Panel(Panel::Soften)),
                    C(Cmd::Panel(Panel::Overlays)),
                    C(Cmd::Panel(Panel::ModelGrid)),
                    C(Cmd::Panel(Panel::Instructor)),
                    C(Cmd::Panel(Panel::Outliner)),
                ],
            ),
            Trays,
            C(Cmd::Dialog(crate::dialogs::Dlg::ManageTrays)),
            C(Cmd::Dialog(crate::dialogs::Dlg::NewTray)),
            Sub(
                "Espace de travail",
                &[
                    Workspaces,
                    Sep,
                    C(Cmd::Dialog(crate::dialogs::Dlg::NewWorkspace)),
                    C(Cmd::Dialog(crate::dialogs::Dlg::ConfigureWorkspace)),
                    C(Cmd::Dialog(crate::dialogs::Dlg::SaveWorkspace)),
                    C(Cmd::Dialog(crate::dialogs::Dlg::LoadWorkspace)),
                ],
            ),
            Sep,
            C(Cmd::ModelInfo),
            C(Cmd::Preferences),
            C(Cmd::Op(Op::OptimizeMemory)),
            Sep,
            C(Cmd::ComponentOptions),
            C(Cmd::ComponentAttributes),
        ],
    ),
    (
        "Extensions",
        &[
            C(Cmd::ExtensionManager),
            Sub("Développeur", &[C(Cmd::RubyConsole), C(Cmd::Tool(ToolId::ShowFaceEdges)), PluginDeveloper]),
            Plugins,
        ],
    ),
    ("Aide", &[C(Cmd::Welcome), Sep, C(Cmd::About), C(Cmd::Search)]),
];

/// Every command reachable from the menus, with its path ("Édition › Masquer").
pub fn command_paths() -> Vec<(String, Cmd)> {
    fn rec(items: &[M], path: &str, out: &mut Vec<(String, Cmd)>) {
        for it in items {
            match it {
                C(c) => out.push((format!("{path} › {}", c.label()), *c)),
                Sub(name, sub) => rec(sub, &format!("{path} › {name}"), out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for (name, items) in MENUS {
        rec(items, name, &mut out);
    }
    // Display commands without a menu entry (toolbars, search).
    for v in crate::view_cmds::ViewCmd::ALL {
        out.push((format!("{} › {}", v.path(), v.label()), Cmd::View(v)));
    }
    // Commands of the entity submenu.
    for c in ENTITY_CMDS {
        out.push((format!("Édition › Entité › {}", c.label()), *c));
    }
    // SketchUp's "Exporter sous …" commands (found by the command search).
    for f in esq_formats::ExportFormat::ALL {
        out.push((format!("Fichier › Exporter › Modèle 3D › {}", Cmd::ExportAs(f).label()), Cmd::ExportAs(f)));
    }
    // In SketchUp's command list but in no menu (Search, the Materials panel's eyedropper).
    out.push((format!("Outils › {}", ToolId::SampleMaterial.label()), Cmd::Tool(ToolId::SampleMaterial)));
    // Tools › Comment: the Comments panel's « + » and the command search (no menu entry).
    out.push((format!("Outils › {}", ToolId::Comment.label()), Cmd::Tool(ToolId::Comment)));
    // Found by the command search only (SketchUp's "… preferences", "reset shortcuts").
    for c in crate::dialogs::SEARCH_CMDS {
        out.push((format!("Fenêtre › Préférences › {}", c.label()), *c));
    }
    out
}

pub fn all_commands() -> Vec<Cmd> {
    command_paths().into_iter().map(|(_, c)| c).collect()
}

pub const ENTITY_CMDS: &[Cmd] = &[
    Cmd::EditGroup,
    Cmd::Explode,
    Cmd::MakeUnique,
    Cmd::ReverseFaces,
    Cmd::Tex(crate::texturing::TexCmd::Position),
    Cmd::Tex(crate::texturing::TexCmd::Reset),
    Cmd::Tex(crate::texturing::TexCmd::Project),
    Cmd::Tex(crate::texturing::TexCmd::MakeUnique),
    Cmd::ReverseSection,
    Cmd::ActiveCut,
    Cmd::AlignView,
    Cmd::GroupFromSlice,
    Cmd::Hide,
    Cmd::Delete,
    Cmd::ZoomSelection,
    Cmd::Op(Op::DeselectEdges),
    Cmd::Op(Op::DeselectFaces),
    Cmd::Op(Op::OrientFaces),
    Cmd::Op(Op::AreaSelection),
    Cmd::Op(Op::Divide),
];

/// One command as a menu entry: label, shortcut, tick, greyed when unavailable.
pub fn command_button(app: &mut EsquisseApp, ui: &mut egui::Ui, cmd: Cmd) {
    let label = match cmd {
        Cmd::Undo => app.doc.undo_label().map(|l| format!("Annuler {l}")).unwrap_or_else(|| cmd.label()),
        Cmd::Redo => app.doc.redo_label().map(|l| format!("Rétablir {l}")).unwrap_or_else(|| cmd.label()),
        _ => cmd.label(),
    };
    let enabled = app.cmd_enabled(cmd);
    let text = if let Some(on) = app.cmd_checked(cmd) {
        // The tick takes the same room whether it shows or not, so labels line up.
        let style = ui.style();
        let font = egui::TextStyle::Button.resolve(style);
        let color = if enabled { style.visuals.text_color() } else { style.visuals.weak_text_color() };
        let mut job = egui::text::LayoutJob::default();
        let tick = if on { color } else { egui::Color32::TRANSPARENT };
        job.append("✔  ", 0.0, egui::TextFormat::simple(font.clone(), tick));
        job.append(&label, 0.0, egui::TextFormat::simple(font, color));
        egui::WidgetText::from(job)
    } else {
        label.into()
    };
    let button = egui::Button::new(text).shortcut_text(cmd.shortcut_text());
    if ui.add_enabled(enabled, button).clicked() {
        ui.close();
        let ctx = ui.ctx().clone();
        app.run(&ctx, cmd);
    }
}

fn items(app: &mut EsquisseApp, ui: &mut egui::Ui, list: &[M]) {
    for it in list {
        match it {
            C(c) => command_button(app, ui, *c),
            Sep => {
                ui.separator();
            }
            Sub(name, sub) => {
                ui.menu_button(*name, |ui| items(app, ui, sub));
            }
            Todo(name) => {
                ui.add_enabled(false, egui::Button::new(*name))
                    .on_disabled_hover_text("Pas encore disponible dans Esquisse");
            }
            Recent => {
                let files = app.recent_files.clone();
                for (i, f) in files.iter().enumerate() {
                    if ui.button(format!("{} {}", i + 1, f.display())).clicked() {
                        ui.close();
                        app.open_path(f);
                    }
                }
                if files.is_empty() {
                    ui.add_enabled(false, egui::Button::new("Aucun fichier récent"));
                }
            }
            Plugins => crate::ruby_ui::plugin_items(app, ui),
            PluginDeveloper => crate::ruby_ui::plugin_developer_items(app, ui),
            Trays => crate::dialogs::trays::menu_items(app, ui),
            Workspaces => crate::dialogs::workspaces::menu_items(app, ui),
            Entity => {
                let title = selection_title(app);
                if app.doc.sel.is_empty() {
                    ui.add_enabled(false, egui::Button::new(title));
                } else {
                    ui.menu_button(title, |ui| {
                        for c in ENTITY_CMDS {
                            if app.cmd_enabled(*c) {
                                command_button(app, ui, *c);
                            }
                        }
                    });
                }
            }
        }
    }
}

/// "Aucune sélection", "Face", "Groupe", "5 entités"…
pub fn selection_title(app: &EsquisseApp) -> String {
    use esq_core::model::EntityRef;
    match app.doc.sel.as_slice() {
        [] => "Aucune sélection".into(),
        [EntityRef::Face(_)] => "Face".into(),
        [EntityRef::Edge(_)] => "Arête".into(),
        [EntityRef::Guide(_)] => "Guide".into(),
        [EntityRef::Section(_)] => "Plan de section".into(),
        [EntityRef::Annot(a)] => match app.doc.ents().annots.get(*a).map(|x| &x.kind) {
            Some(esq_core::model::AnnotKind::Linear { .. }) => "Cotation".into(),
            _ => "Texte".into(),
        },
        [EntityRef::Instance(i)] => {
            let ents = app.doc.ents();
            let is_group = ents.instances.get(*i).is_some_and(|x| app.doc.model.defs[x.def].is_group);
            if is_group { "Groupe".into() } else { "Composant".into() }
        }
        s => format!("{} entités", s.len()),
    }
}

pub fn menu_bar(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    egui::Panel::top("menu").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            for (name, list) in MENUS {
                ui.menu_button(*name, |ui| {
                    ui.set_min_width(260.0);
                    items(app, ui, list);
                    crate::ruby_ui::plugin_menu_for(app, ui, name);
                });
            }
        });
    });
}
