//! Window › Preferences, driven like a user.

use std::path::PathBuf;

use super::dialogs::open;
use crate::commands::Cmd;
use crate::settings;

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("esq-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn general_page_backs_up_auto_saves_purges_and_fixes() {
    let (mut a, mut ui) = open(Cmd::Preferences);
    assert_eq!(ui.toggled("Créer une sauvegarde"), Some(true));
    let dir = temp_dir("general");
    let path = dir.join("maison.esq");

    // Create backup: the previous version is kept as "maison~.esq".
    a.save_to(&path).unwrap();
    a.doc.commit("t", |m, _| {
        m.add_material("Brique", [150, 60, 40]);
    });
    a.save_to(&path).unwrap();
    assert!(dir.join("maison~.esq").is_file());
    ui.click(&mut a, "Créer une sauvegarde");
    assert!(!settings::get().create_backup);
    std::fs::remove_file(dir.join("maison~.esq")).unwrap();
    a.save_to(&path).unwrap();
    assert!(!dir.join("maison~.esq").exists());

    // Auto-save every minute: a copy is written next to the model, removed on save.
    ui.type_into(&mut a, "Minutes entre les enregistrements", "1");
    assert_eq!(settings::get().autosave_minutes, 1);
    a.doc.commit("t", |m, _| {
        m.add_material("Bois", [150, 100, 40]);
    });
    crate::dialogs::pref_general::tick(&mut a, 100.0);
    crate::dialogs::pref_general::tick(&mut a, 130.0);
    let auto = dir.join("AutoSave_maison.esq");
    assert!(!auto.exists(), "pas avant une minute");
    crate::dialogs::pref_general::tick(&mut a, 161.0);
    assert!(auto.is_file());
    assert!(a.doc.dirty, "the model itself is not saved");
    a.save_to(&path).unwrap();
    assert!(!auto.exists());

    // Ask to purge at save.
    ui.click(&mut a, "Demander de purger lors de la sauvegarde");
    let ctx = ui.ctx.clone();
    a.run(&ctx, Cmd::Save);
    ui.frames(&mut a, 2);
    ui.click(&mut a, "Purger");
    assert!(a.doc.model.materials.is_empty());
    assert!(!a.doc.dirty, "saved after the purge");

    // A model with a problem, opened: repaired, and the user told.
    ui.click(&mut a, "M'avertir lorsque les problèmes sont résolus");
    let broken = dir.join("casse.esq");
    let mut m = esq_core::model::Model::new();
    let v: Vec<_> = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]
        .iter()
        .map(|(x, y)| m.root.add_vertex_raw(esq_core::glam::DVec3::new(*x, *y, 0.0)))
        .collect();
    let f = m.root.add_face_raw(v, vec![]).unwrap();
    m.root.faces[f].layer = 9;
    esq_core::io::save_esq(&m, &broken).unwrap();
    a.open_path(&broken);
    let f = a.doc.model.root.faces.keys().next().unwrap();
    assert_eq!(a.doc.model.root.faces[f].layer, 0);
    assert!(!a.doc.dirty);
    assert!(a.message.as_ref().is_some_and(|(m, _)| m.contains("corrigé")), "{:?}", a.message);

    // Welcome window and toolbar settings.
    ui.click(&mut a, "Afficher la fenêtre d'accueil");
    assert!(!settings::get().show_welcome);
    let large = a.ui_state.extra.toolbars.large_icons;
    ui.click(&mut a, "Utiliser de grandes icônes");
    assert_eq!(a.ui_state.extra.toolbars.large_icons, !large);
    // Cancel puts every setting back.
    ui.click(&mut a, "Annuler");
    assert!(settings::get().show_welcome && settings::get().create_backup);
    assert!(!a.ui_state.preferences);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn shortcuts_page_assigns_moves_removes_and_resets() {
    use crate::tools::ToolId;
    use eframe::egui::{Key, Modifiers};
    crate::shortcuts::reset_all();
    // "Open Shortcuts Settings" opens Preferences at the Shortcuts page.
    let (mut a, mut ui) = open(Cmd::PrefsPage(crate::dialogs::PrefPage::Shortcuts));
    assert_eq!(a.ui_state.extra.dialogs.pref_page, crate::dialogs::PrefPage::Shortcuts);
    ui.type_into(&mut a, "Filtre", "Rectangle");
    ui.click(&mut a, "Dessiner/Formes/Rectangle");
    assert!(ui.has("Raccourci R"));
    // Press K in "Add Shortcut", then "+": K was View › Back Edges', the question comes.
    ui.focus(&mut a, "Ajouter un raccourci");
    ui.key(&mut a, Key::K, Modifiers::NONE);
    assert_eq!(ui.value("Ajouter un raccourci"), "K");
    ui.click(&mut a, "Ajouter le raccourci");
    assert!(ui.nodes.iter().any(|n| n.value.contains("déjà affecté à « Arêtes arrières »")));
    ui.click(&mut a, "Oui");
    assert_eq!(crate::app::EsquisseApp::cmd_for_key(Key::K, Modifiers::NONE), Some(Cmd::Tool(ToolId::Rect)));
    assert_eq!(Cmd::BackEdges.shortcut(), None);
    // The menus show the first shortcut; the keyboard knows both.
    assert_eq!(Cmd::Tool(ToolId::Rect).shortcut_text(), "R");
    // "-" removes R.
    ui.click(&mut a, "Raccourci R");
    ui.click(&mut a, "Supprimer le raccourci");
    assert_eq!(crate::app::EsquisseApp::cmd_for_key(Key::R, Modifiers::NONE), None);
    assert_eq!(Cmd::Tool(ToolId::Rect).shortcut_text(), "K");
    // The Ruby API (its own thread) lists the shortcuts as they are now.
    settings::share_for_test();
    let r = super::dialogs::ruby(&mut a, "Sketchup.get_shortcuts.grep(/Rectangle/)");
    settings::set(settings::Settings::default());
    settings::share_for_test();
    crate::shortcuts::assign(Cmd::Tool(ToolId::Rect), (Modifiers::NONE, Key::K));
    crate::shortcuts::remove(Cmd::Tool(ToolId::Rect), (Modifiers::NONE, Key::R));
    if let Some(r) = r {
        assert!(r.contains("K\\tDessiner/Formes/Rectangle"), "{r}");
    }
    // Reset All, after a question.
    ui.click(&mut a, "Tout réinitialiser");
    ui.click(&mut a, "Réinitialiser tous les raccourcis");
    assert_eq!(crate::app::EsquisseApp::cmd_for_key(Key::R, Modifiers::NONE), Some(Cmd::Tool(ToolId::Rect)));
    assert_eq!(crate::app::EsquisseApp::cmd_for_key(Key::K, Modifiers::NONE), Some(Cmd::BackEdges));
    // The "Reset Shortcuts" command does the same.
    crate::shortcuts::assign(Cmd::Tool(ToolId::Rect), (Modifiers::NONE, Key::J));
    let ctx = ui.ctx.clone();
    a.run(&ctx, Cmd::ResetShortcuts);
    assert_eq!(crate::app::EsquisseApp::cmd_for_key(Key::J, Modifiers::NONE), None);
    // Reserved keys cannot be given.
    a.ui_state.extra.dialogs.prefs.shortcuts.captured = Some((Modifiers::NONE, Key::Escape));
    crate::dialogs::pref_shortcuts::add(&mut a);
    assert_eq!(a.message.as_ref().map(|m| m.0.as_str()), Some("Ce raccourci ne peut pas être réaffecté."));
    // The other "… preferences" commands, found by the command search.
    let paths = crate::menus::command_paths();
    for c in crate::dialogs::SEARCH_CMDS {
        assert!(paths.iter().any(|(_, x)| x == c), "{c:?}");
    }
    a.run(&ctx, Cmd::PrefsPage(crate::dialogs::PrefPage::Accessibility));
    assert_eq!(a.ui_state.extra.dialogs.pref_page, crate::dialogs::PrefPage::Accessibility);
}

#[test]
fn accessibility_page_colours_axes_and_inferences() {
    use crate::pick::SnapKind;
    let (mut a, mut ui) = open(Cmd::PrefsPage(crate::dialogs::PrefPage::Accessibility));
    assert_eq!(ui.value("Axe rouge"), "220 30 30");
    // A colour chosen in the colour picker (egui's own widget) lands in the settings…
    settings::update(|s| {
        s.axis_colors[0] = [255, 128, 0];
        s.parallel_color = [0, 0, 0];
    });
    ui.frames(&mut a, 2);
    assert_eq!(ui.value("Axe rouge"), "255 128 0");
    // …and the axes, the inferences and the arrow-key locks take it.
    assert_eq!(SnapKind::Axis(0).color(), [255, 128, 0]);
    assert_eq!(SnapKind::Parallel.color(), [0, 0, 0]);
    let opts = crate::scene::SceneOptions {
        ctx: &[],
        ctx_def: None,
        sel: &[],
        axes_xform: esq_core::glam::DMat4::IDENTITY,
        style: &a.style,
    };
    let mesh = crate::scene::build(&a.doc.model, &opts);
    let orange = [1.0, 128.0 / 255.0, 0.0];
    assert!(mesh.lines.iter().any(|l| (0..3).all(|i| (l.color[i] - orange[i]).abs() < 0.01)), "axe rouge orange");
    // Reset All.
    ui.click(&mut a, "Tout réinitialiser");
    assert_eq!(settings::get().axis_colors, settings::AXIS_COLORS);
    assert_eq!(SnapKind::Parallel.color(), settings::PARALLEL_COLOR);
}

#[test]
fn compatibility_page_box_highlight_and_wheel() {
    use esq_core::glam::{DMat4, DVec3};
    use esq_core::model::EntityRef;
    let (mut a, mut ui) = open(Cmd::PrefsPage(crate::dialogs::PrefPage::Compatibility));
    // A group, selected.
    let mut inst = None;
    a.doc.commit("t", |m, _| {
        let v: Vec<_> = [(0.0, 0.0), (100.0, 0.0), (100.0, 50.0)]
            .iter()
            .map(|(x, y)| m.root.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
            .collect();
        m.root.add_face_raw(v, vec![]);
        let all: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
        inst = m.make_group(None, &all, true, None);
    });
    a.doc.select(vec![EntityRef::Instance(inst.unwrap())]);
    let build = |a: &crate::app::EsquisseApp| {
        let opts = crate::scene::SceneOptions {
            ctx: &[],
            ctx_def: None,
            sel: &a.doc.sel,
            axes_xform: DMat4::IDENTITY,
            style: &a.style,
        };
        crate::scene::build(&a.doc.model, &opts)
    };
    let sel = crate::style::ViewStyle::default().highlight_color.map(|c| c as f32 / 255.0);
    let is_sel = |c: [f32; 4]| (0..3).all(|i| (c[i] - sel[i]).abs() < 0.01);
    let highlighted = build(&a);
    assert_eq!(highlighted.lines.iter().filter(|l| is_sel(l.color)).count(), 3, "the triangle's edges");
    ui.click(&mut a, "Boîte englobante uniquement");
    assert!(settings::get().bbox_highlight);
    let boxed = build(&a);
    assert_eq!(boxed.lines.iter().filter(|l| is_sel(l.color)).count(), 12, "the box only");
    // Mouse wheel: forward zooms in, unless inverted.
    assert!(settings::wheel_zoom_factor(120.0) < 1.0);
    ui.click(&mut a, "Inverser");
    assert!(settings::wheel_zoom_factor(120.0) > 1.0);
}

#[test]
fn drawing_page_click_style_lines_lasso_crosshairs_and_prepick() {
    use crate::settings::ClickStyle;
    use crate::tools::ToolId;
    use esq_core::glam::{DVec2, DVec3};
    let (mut a, mut ui) = open(Cmd::PrefsPage(crate::dialogs::PrefPage::Drawing));
    ui.click(&mut a, "Cliquer-déplacer-cliquer");
    assert_eq!(settings::click_style(), ClickStyle::ClickMoveClick);
    // A press-drag-release draws no line in click-move-click…
    let mut h = super::Harness::new();
    h.tool(ToolId::Line);
    let (p, q) = (h.px(DVec3::ZERO), h.px(DVec3::new(1000.0, 0.0, 0.0)));
    let drag = |h: &mut super::Harness| {
        h.with(|t, cx| {
            t.mouse_move(cx, p);
            t.press(cx, p);
            t.mouse_move(cx, q);
            t.release(cx, q);
        })
    };
    drag(&mut h);
    assert_eq!(h.doc.ents().edges.len(), 0);
    // …and one with automatic detection.
    ui.click(&mut a, "Détection automatique");
    h.with(|t, cx| t.cancel(cx));
    h.tool(ToolId::Line);
    drag(&mut h);
    assert_eq!(h.doc.ents().edges.len(), 1);

    // Continue line drawing off: each line stands alone.
    ui.click(&mut a, "Tracé de ligne continu");
    assert!(!settings::get().continue_lines);
    let mut h = super::Harness::new();
    h.tool(ToolId::Line);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    assert!(!h.tool.busy(), "the chain stops after one line");
    h.click(DVec3::new(0.0, 1000.0, 0.0));
    assert_eq!(h.doc.ents().edges.len(), 1);

    // Lasso direction: drawn clockwise, the lasso now takes what it crosses.
    ui.click(&mut a, "Sélection croisée dans le sens des aiguilles d'une montre");
    let mut h = super::Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Lasso);
    let c = h.px(DVec3::new(0.0, 0.0, 0.0));
    let pts: Vec<DVec2> = (0..=36)
        .map(|i| {
            let ang = i as f64 / 36.0 * std::f64::consts::TAU;
            c + DVec2::new(ang.cos(), ang.sin()) * 60.0
        })
        .collect();
    h.with(|t, cx| t.press(cx, pts[0]));
    for p in &pts[1..] {
        h.with(|t, cx| t.mouse_move(cx, *p));
    }
    h.with(|t, cx| t.release(cx, pts[36]));
    assert!(!h.doc.sel.is_empty(), "a corner's faces and edges are crossed");

    // Crosshairs follow the cursor along the axes.
    let snap = crate::pick::Snap {
        pos: DVec3::new(10.0, 20.0, 30.0),
        kind: crate::pick::SnapKind::Free,
        face: None,
        edge: None,
        guide: None,
    };
    let axes = esq_core::glam::DMat4::IDENTITY;
    assert!(crate::dialogs::pref_pages::crosshair_lines(Some(&snap), ToolId::Line, &axes, 5.0).is_empty());
    ui.click(&mut a, "Afficher les réticules");
    let lines = crate::dialogs::pref_pages::crosshair_lines(Some(&snap), ToolId::Line, &axes, 5.0);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].0, DVec3::new(5.0, 20.0, 30.0));

    // Pre-pick: a face selected first is the one Push/Pull takes, wherever the click.
    let mut h = super::Harness::new();
    h.rect_and_box();
    let top = h.doc.ents().faces.iter().find(|(_, f)| f.normal.z > 0.99).map(|(k, _)| k).unwrap();
    h.doc.select(vec![esq_core::model::EntityRef::Face(top)]);
    h.tool(ToolId::PushPull);
    h.with(|t, cx| t.mouse_move(cx, DVec2::new(5.0, 5.0)));
    h.with(|t, cx| t.press(cx, DVec2::new(5.0, 5.0)));
    assert!(h.tool.busy(), "the selected face is being pushed");
    ui.click(&mut a, "Désactiver la présélection dans l’outil Pousser/Tirer");
    let mut h2 = super::Harness::new();
    h2.rect_and_box();
    h2.doc.select(vec![esq_core::model::EntityRef::Face(h2.doc.ents().faces.keys().next().unwrap())]);
    h2.tool(ToolId::PushPull);
    h2.with(|t, cx| t.press(cx, DVec2::new(5.0, 5.0)));
    assert!(!h2.tool.busy(), "only the face under the cursor");

    // Show tool rotation grips: the Move tool's grips, the same setting as Edit's command.
    assert!(crate::tools::move_grips::grips_enabled());
    ui.click(&mut a, "Afficher les poignées de rotation des outils");
    assert!(!crate::tools::move_grips::grips_enabled());
}

#[test]
fn applications_page_picks_the_image_editor_textures_open_with() {
    let (mut a, mut ui) = open(Cmd::PrefsPage(crate::dialogs::PrefPage::Applications));
    assert_eq!(ui.value("Éditeur d'images"), "(aucun)");
    let img = std::path::Path::new(r"C:\tmp\brique.png");
    // None chosen: Windows' program for the file type.
    let c = settings::image_editor_command(img);
    assert_eq!(c.get_program(), "cmd");
    // "Choose..." (the file dialog is the system's): an editor that exists.
    let editor = std::env::current_exe().unwrap();
    settings::update(|s| s.image_editor = editor.display().to_string());
    ui.frames(&mut a, 2);
    assert_eq!(ui.value("Éditeur d'images"), editor.display().to_string());
    let c = settings::image_editor_command(img);
    assert_eq!((c.get_program(), c.get_args().next()), (editor.as_os_str(), Some(img.as_os_str())));
    ui.click(&mut a, "Effacer");
    assert!(settings::get().image_editor.is_empty());
}

#[test]
fn files_page_sets_where_models_and_templates_go() {
    let (mut a, mut ui) = open(Cmd::PrefsPage(crate::dialogs::PrefPage::Files));
    let dir = temp_dir("files");
    let models = dir.join("Projets");
    std::fs::create_dir_all(&models).unwrap();
    ui.type_into(&mut a, "Dossier Modèles :", &models.display().to_string());
    assert_eq!(settings::with(|s| s.location("models")), Some(models.clone()));
    // An unsaved model auto-saves there.
    assert_eq!(crate::dialogs::pref_general::autosave_path(&a), Some(models.join("AutoSave_Sans titre.esq")));
    // A folder that does not exist is shown as invalid and not used.
    ui.type_into(&mut a, "Dossier Exporter :", r"Z:\nulle\part");
    assert!(ui.has(
        "Un ou plusieurs chemins d'accès ne sont pas valides (en rouge) : les dossiers par défaut servent à leur place."
    ));
    assert_eq!(settings::with(|s| s.location("export")), None);
    // Templates: listed from their folder.
    let templates = dir.join("Gabarits");
    std::fs::create_dir_all(&templates).unwrap();
    esq_core::io::save_esq(&esq_core::model::Model::new(), &templates.join("Plan masse.esq")).unwrap();
    ui.type_into(&mut a, "Dossier Modèles types :", &templates.display().to_string());
    assert!(crate::prefs::all_templates().iter().any(|t| t.name == "Plan masse"));
    ui.click(&mut a, "Réinitialiser");
    assert!(settings::get().locations.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn graphics_page_sets_antialiasing_shadows_textures_and_materials() {
    use crate::render::{GpuConfig, config_changed, config_for};
    let (mut a, mut ui) = open(Cmd::PrefsPage(crate::dialogs::PrefPage::Graphics));
    assert_eq!(config_for(&[1, 4]), GpuConfig { samples: 4, shadow_size: 2048 });
    ui.choose(&mut a, "Anti-crénelage", "0x");
    ui.click(&mut a, "Utiliser les ombres plus rapides");
    assert_eq!(config_for(&[1, 4]), GpuConfig { samples: 1, shadow_size: 4096 });
    assert!(config_changed(GpuConfig { samples: 4, shadow_size: 2048 }), "the view is rebuilt");
    // The view draws without multisampling (on a real GPU, when there is one).
    if let Some(mut g) = super::gpu::Gpu::new() {
        assert_eq!(g.view.config.samples, 1);
        let mut m = esq_core::model::Model::new();
        let v: Vec<_> = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0)]
            .iter()
            .map(|(x, y)| m.root.add_vertex_raw(esq_core::glam::DVec3::new(*x, *y, 0.0)))
            .collect();
        m.root.add_face_raw(v, vec![]);
        let mut cam = crate::camera::Camera { size: esq_core::glam::DVec2::new(64.0, 48.0), ..Default::default() };
        cam.zoom_extents(esq_core::glam::DVec3::ZERO, esq_core::glam::DVec3::new(1000.0, 1000.0, 0.0));
        let shot = g.render(&m, &crate::style::ViewStyle::default(), &cam, &Default::default());
        assert_eq!((shot.w, shot.h), (64, 48));
    }
    // Maximum texture size: off, textures stay within 1024 texels.
    let big = std::sync::Arc::new(crate::textures::Texels { w: 4096, h: 4096, rgba: Vec::new(), alpha: false });
    assert_eq!(crate::render::layer_side(std::slice::from_ref(&big)), 1024);
    ui.click(&mut a, "Utiliser la taille maximale de texture");
    assert!(crate::render::layer_side(std::slice::from_ref(&big)) > 1024);
    // Automatically Enhance Materials: steel is metallic, unless metalness is off.
    let mut m = esq_core::model::Model::new();
    let steel = m.add_material("Acier brossé", [180, 180, 190]);
    assert_eq!(crate::style::pbr_of(&m.materials[steel]) >> 8 & 255, 255);
    ui.click(&mut a, "Aspect métallique");
    assert_eq!(crate::style::pbr_of(&m.materials[steel]) >> 8 & 255, 0);
    // Fast feedback: no ambient occlusion while orbiting.
    let style = crate::style::ViewStyle { ambient_occlusion: true, ..Default::default() };
    assert!(!crate::dialogs::pref_pages::render_style(&style, true).ambient_occlusion);
    ui.click(&mut a, "Utiliser la réponse rapide");
    assert!(crate::dialogs::pref_pages::render_style(&style, true).ambient_occlusion);
    // Details of the graphics engine.
    ui.click(&mut a, "Moteur graphique : détails");
    assert!(a.ui_state.extra.dialogs.prefs.gpu_details);
}

#[test]
fn template_page_chooses_what_file_new_starts_from() {
    let _l = crate::prefs::TEMPLATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (mut a, mut ui) = open(Cmd::Preferences);
    ui.click(&mut a, "Modèle type");
    ui.click(&mut a, "Simple — Centimètres");
    assert_eq!(crate::prefs::default_template().units, esq_core::model::Units::Centimeters);
    // File › New: a centimetre model, with that template's precision.
    a.new_document();
    assert_eq!(a.doc.model.units, esq_core::model::Units::Centimeters);
    assert_eq!(esq_core::units::UnitsDisplay::of(&a.doc.model).precision, 1);
    // Browse…: any model file becomes the default template.
    let dir = temp_dir("template");
    let src = dir.join("Atelier.esq");
    let mut m = esq_core::model::Model { units: esq_core::model::Units::Feet, ..Default::default() };
    m.meta.name = "Atelier".into();
    esq_core::io::save_esq(&m, &src).unwrap();
    let t = crate::prefs::add_template_file(&src).unwrap();
    crate::prefs::set_default_template(&t);
    ui.frames(&mut a, 2);
    assert!(ui.has("Atelier"));
    a.new_document();
    assert_eq!((a.doc.model.units, a.doc.model.meta.name.as_str()), (esq_core::model::Units::Feet, "Atelier"));
    crate::prefs::set_default_template(&crate::prefs::builtin_templates()[0]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn workspace_page_sizes_tool_buttons_and_resets_trays_and_toolbars_on_ok() {
    use crate::commands::Panel;
    use crate::dialogs::{PrefPage, trays};
    let mut a = super::ui_driver::app();
    // The toolbars as installed (tests running beside this one save theirs in the same file).
    a.ui_state.extra.toolbars = crate::toolbars::ToolbarConfig::from_text("");
    let mut ui = super::ui_driver::Driver::new();
    a.run(&ui.ctx.clone(), Cmd::PrefsPage(PrefPage::Workspace));
    ui.frames(&mut a, 3);
    assert!(ui.has("Palette d'outils") && ui.has("Espace de travail"));

    // Use large tool buttons: the toolbars' icon size.
    assert_eq!(ui.toggled("Utiliser les grands boutons d'outils"), Some(true));
    ui.click(&mut a, "Utiliser les grands boutons d'outils");
    assert!(!a.ui_state.extra.toolbars.large_icons);
    assert_eq!(a.ui_state.extra.toolbars.icon_size(), 24.0);

    // A workspace of one's own: a tray, a hidden toolbar.
    trays::create(&mut a, "Rendu", &[Panel::Shadows]).unwrap();
    a.ui_state.extra.toolbars.set_shown("Visite", false);
    let mine = |a: &crate::app::EsquisseApp| {
        !settings::get().trays.is_empty()
            && !a.ui_state.panels.contains(&Panel::Shadows)
            && !a.ui_state.extra.toolbars.shown("Visite")
    };

    // Reset to Default asks first, then waits for OK: Cancel forgets it.
    ui.click(&mut a, "Rétablir les valeurs par défaut");
    assert!(ui.has("Réinitialisation de l'espace de travail"));
    ui.click(&mut a, "Non");
    assert!(!ui.node("Rétablir les valeurs par défaut").disabled);
    ui.click(&mut a, "Rétablir les valeurs par défaut");
    ui.click(&mut a, "Oui");
    assert!(ui.node("Rétablir les valeurs par défaut").disabled);
    assert!(mine(&a));
    ui.click(&mut a, "Annuler");
    assert!(!a.ui_state.preferences && mine(&a));

    a.run(&ui.ctx.clone(), Cmd::PrefsPage(PrefPage::Workspace));
    ui.frames(&mut a, 2);
    assert!(!ui.node("Rétablir les valeurs par défaut").disabled);
    settings::update(|s| s.crosshairs = true);
    ui.click(&mut a, "Rétablir les valeurs par défaut");
    ui.click(&mut a, "Oui");
    ui.click(&mut a, "OK");
    // Trays, toolbars and preferences as installed.
    assert!(!settings::get().crosshairs);
    assert!(settings::get().trays.is_empty());
    assert!(a.ui_state.tray && a.ui_state.panels.contains(&Panel::Shadows));
    let tb = &a.ui_state.extra.toolbars;
    assert!(tb.shown("Visite") && tb.large_icons && tb.tooltips);
}
