//! Window › Instructor, trays, workspace and the other panels of the dialogs domain, driven
//! like a user.

use eframe::egui;

use super::ui_driver::{Driver, app};
use crate::commands::{Cmd, Panel};
use crate::tools::ToolId;

#[test]
fn instructor_follows_the_active_tool() {
    let mut a = app();
    let mut ui = Driver::new();
    let ctx = ui.ctx.clone();
    // Window › Default Tray › Instructor adds it to the tray.
    assert!(!a.ui_state.panels.contains(&Panel::Instructor));
    a.run(&ctx, Cmd::Panel(Panel::Instructor));
    assert!(a.ui_state.panels.contains(&Panel::Instructor));

    ui.side = Some(|a, ui| {
        egui::Panel::right("side").exact_size(320.0).show(ui, |ui| crate::dialogs::instructor::show(a, ui));
    });
    a.set_tool(ToolId::Line);
    ui.frames(&mut a, 2);
    assert!(ui.has("Outil Ligne"));
    assert!(ui.has("Fonctionnement de l'outil") && ui.has("Touches de modification"));
    assert!(ui.has("1.  Cliquez pour placer le point de départ."));
    // The tool's present prompt.
    let status = a.tool.status();
    assert!(ui.has(&status), "{status}");

    a.set_tool(ToolId::Move);
    ui.frames(&mut a, 2);
    assert!(ui.has("Outil Déplacer") && !ui.has("Outil Ligne"));
    assert!(ui.has("= copier"));

    // Tools without modifier keys leave the section out.
    a.set_tool(ToolId::Pan);
    ui.frames(&mut a, 2);
    assert!(ui.has("Outil Panoramique") && !ui.has("Touches de modification"));
}

#[test]
fn trays_are_made_renamed_docked_and_deleted() {
    use crate::dialogs::Dlg;
    use crate::dialogs::trays;
    use crate::settings::{self, Dock};

    let mut a = app();
    let mut ui = Driver::new();
    let ctx = ui.ctx.clone();
    // The trays are drawn only to look at them: their panel titles would be found instead
    // of the check boxes of the windows.
    let trays_on = |ui: &mut Driver, a: &mut crate::app::EsquisseApp, on: bool| {
        ui.side = if on { Some(crate::ui::tray) } else { None };
        ui.frames(a, 3);
    };
    trays_on(&mut ui, &mut a, true);
    assert!(ui.has("Palette par défaut") && ui.has("Menu de Palette par défaut"));
    trays_on(&mut ui, &mut a, false);

    // Window › New Tray: a name, two panels.
    a.run(&ctx, Cmd::Dialog(Dlg::NewTray));
    ui.frames(&mut a, 2);
    ui.type_into(&mut a, "Nom :", "Rendu");
    ui.click(&mut a, "Ombres");
    ui.click(&mut a, "Scènes");
    ui.click(&mut a, "Ajouter");
    let t = settings::get().trays;
    assert_eq!(t.len(), 1);
    assert_eq!((t[0].name.as_str(), t[0].visible, t[0].dock), ("Rendu", true, Dock::Right));
    assert_eq!(t[0].panels, vec!["Shadows".to_string(), "Scenes".to_string()]);
    // A panel lives in one tray: they left the default one, which keeps the rest.
    assert!(!a.ui_state.panels.contains(&Panel::Shadows) && !a.ui_state.panels.contains(&Panel::Scenes));
    assert!(a.ui_state.panels.contains(&Panel::Tags));
    assert!(!ui.has("Nouvelle palette"), "la fenêtre se ferme");
    // The new tray shows, with its panels.
    trays_on(&mut ui, &mut a, true);
    assert!(ui.has("Menu de Rendu") && ui.has("Ombres") && ui.has("Scènes"));
    trays_on(&mut ui, &mut a, false);

    // Names SketchUp refuses.
    a.run(&ctx, Cmd::Dialog(Dlg::NewTray));
    ui.frames(&mut a, 2);
    ui.type_into(&mut a, "Nom :", "Palette par défaut");
    ui.click(&mut a, "Ajouter");
    assert!(ui.has("Nom de palette non valide"));
    assert!(ui.has("Nom de plateau invalide. Les noms des plateaux doivent être uniques."));
    ui.click(&mut a, "OK");
    ui.type_into(&mut a, "Nom :", "A & B");
    ui.click(&mut a, "Ajouter");
    assert!(ui.has("Nom de palette non valide"));
    ui.click(&mut a, "OK");
    ui.click(&mut a, "Annuler");
    assert_eq!(settings::get().trays.len(), 1);

    // Window › Manage Trays: rename it, give it a panel.
    a.run(&ctx, Cmd::Dialog(Dlg::ManageTrays));
    ui.frames(&mut a, 2);
    assert!(ui.node("Renommer").disabled, "la palette par défaut garde son nom");
    ui.click(&mut a, "Rendu");
    ui.click(&mut a, "Renommer");
    assert!(ui.has("Renommer le plateau"));
    ui.type_into(&mut a, "Nom :", "Atelier");
    ui.frames(&mut a, 2);
    assert_eq!(settings::get().trays[0].name, "Atelier");
    assert!(!ui.has("Renommer le plateau"));
    assert_eq!(ui.toggled("Ombres"), Some(true));
    ui.click(&mut a, "Instructeur");
    assert!(settings::get().trays[0].panels.contains(&"Instructor".to_string()));

    // Window › Default Tray › Shadows takes the panel back.
    a.run(&ctx, Cmd::Panel(Panel::Shadows));
    ui.frames(&mut a, 2);
    assert!(a.ui_state.panels.contains(&Panel::Shadows));
    assert!(!settings::get().trays[0].panels.contains(&"Shadows".to_string()));

    // Docking, hiding.
    trays::set_dock(&mut a, 1, Dock::Floating);
    trays_on(&mut ui, &mut a, true);
    assert!(ui.has("Menu de Atelier"));
    trays::set_visible(&mut a, 1, false);
    ui.frames(&mut a, 2);
    assert!(!ui.has("Menu de Atelier"));
    trays::set_visible(&mut a, 1, true);
    trays_on(&mut ui, &mut a, false);

    // Delete, after "Oui".
    ui.click(&mut a, "Atelier");
    ui.click(&mut a, "Supprimer");
    assert!(ui.has("Souhaitez-vous supprimer le plateau : Atelier ?"));
    ui.click(&mut a, "Non");
    assert_eq!(settings::get().trays.len(), 1);
    ui.click(&mut a, "Supprimer");
    ui.click(&mut a, "Oui");
    assert!(settings::get().trays.is_empty());
    ui.click(&mut a, "Fermer");
    assert!(!ui.has("Gérer les palettes"));

    // The default tray is saved for the next start (one panel taken out of it).
    a.run(&ctx, Cmd::Panel(Panel::Tags));
    ui.frames(&mut a, 2);
    let (panels, shown) = trays::initial_default();
    assert!(shown && !panels.contains(&Panel::Tags) && panels.contains(&Panel::Shadows));
}

#[test]
fn scenes_save_and_restore_the_properties_ticked() {
    use crate::dialogs::scene_props::{self, CAMERA, SHADOW};
    let mut a = app();
    let mut ui = Driver::new();
    a.doc.commit("t", |m, _| m.layers.push(esq_core::model::Layer::new("Meubles".to_string())));
    a.add_scene();
    let s = a.doc.model.scenes[0].clone();
    assert_eq!(scene_props::flags(&s), scene_props::ALL);
    for k in ["ro", "shadow", "axes", "hidden_ents", "sections"] {
        assert!(s.ext.props.contains_key(k), "{k}");
    }
    // What Ruby's Page reads.
    if let Some(r) = super::dialogs::ruby(
        &mut a,
        "pg = Sketchup.active_model.pages[0]; [pg.use_camera?, pg.use_shadow_info?, pg.use_rendering_options?, pg.include_in_animation?]",
    ) {
        assert_eq!(r, "[true, true, true, true]");
    }

    // Another state, then back to the scene: tags, style, shadows and camera come back.
    let cam = a.cam.clone();
    a.doc.model.layers[1].visible = false;
    a.style.fog = true;
    a.doc.model.shadow.display = true;
    a.cam.yaw += 1.0;
    a.go_to_scene(0, 0.0, true);
    assert!(a.doc.model.layers[1].visible && !a.style.fog && !a.doc.model.shadow.display);
    assert!((a.cam.yaw - cam.yaw).abs() < 1e-9);

    // Window › Scenes: the properties of the selected scene.
    ui.side = Some(|a, ui| {
        egui::Panel::right("side")
            .exact_size(330.0)
            .show(ui, |ui| crate::dialogs::scene_props::properties_ui(a, ui, 0));
    });
    ui.frames(&mut a, 2);
    assert_eq!(ui.toggled("Inclure dans l'animation"), Some(true));
    ui.type_into(&mut a, "Nom de la scène", "Façade");
    assert_eq!(a.doc.model.scenes[0].name, "Façade");
    // Camera location unticked: the scene leaves the camera where it is.
    ui.click(&mut a, "Emplacement de la caméra");
    assert_eq!(scene_props::flags(&a.doc.model.scenes[0]) & CAMERA, 0);
    a.cam.yaw += 0.5;
    let moved = a.cam.yaw;
    a.go_to_scene(0, 0.0, true);
    assert!((a.cam.yaw - moved).abs() < 1e-9);
    // Not in the animation: the slideshow skips it.
    a.add_scene();
    ui.click(&mut a, "Inclure dans l'animation");
    assert!(!scene_props::animated(&a.doc.model.scenes[0]));
    assert_eq!(scene_props::next_animated(&a.doc.model, Some(1)), Some(1));
    if let Some(r) = super::dialogs::ruby(&mut a, "Sketchup.active_model.pages[0].include_in_animation?") {
        assert_eq!(r, "false");
    }

    // The panel's Update asks which properties to update ("Actualisation").
    ui.side = None;
    a.active_scene = Some(1);
    a.doc.model.shadow.display = true;
    a.style.fog = true;
    scene_props::ask_update(&mut a);
    ui.frames(&mut a, 2);
    assert!(ui.has("Propriétés à actualiser:"));
    ui.click(&mut a, "Paramètres d'ombre");
    ui.click(&mut a, "Mettre à jour");
    let s1 = &a.doc.model.scenes[1];
    assert_eq!(s1.ext.props["ro"]["DisplayFog"], serde_json::json!(true));
    assert_eq!(s1.ext.props["shadow"]["DisplayShadows"], serde_json::json!(false), "ombres non actualisées");
    assert_ne!(scene_props::flags(s1) & SHADOW, 0, "toujours enregistrées");
}

#[test]
fn tags_have_an_active_one_for_new_objects_and_ask_before_deleting() {
    use crate::dialogs::tag_dialog;
    use esq_core::glam::DVec3;
    let mut a = app();
    let mut ui = Driver::new();
    ui.side = Some(|a, ui| {
        egui::Panel::right("side").exact_size(330.0).show(ui, |ui| crate::tags_panel::show(a, ui));
    });
    ui.frames(&mut a, 2);
    a.doc.commit("Étiquettes", |m, _| {
        for name in ["Murs", "Meubles", "Vide"] {
            m.add_layer(name);
        }
    });
    ui.frames(&mut a, 2);
    assert_eq!(a.doc.model.layers.len(), 4);
    let edge = |a: &mut crate::app::EsquisseApp| {
        let mut id = None;
        a.doc.commit("Ligne", |m, d| {
            let e = m.entities_mut(d);
            let (p, q) = (e.add_vertex_raw(DVec3::ZERO), e.add_vertex_raw(DVec3::X * 1000.0));
            id = e.add_edge_raw(p, q);
        });
        id.unwrap()
    };
    let before = edge(&mut a);

    // The pencil: new objects go to the active tag, which cannot be hidden.
    ui.click(&mut a, "Étiquette active : Murs");
    assert_eq!(tag_dialog::active(&a.doc.model), 1);
    if let Some(r) = super::dialogs::ruby(&mut a, "Sketchup.active_model.active_layer.name") {
        assert_eq!(r, "\"Murs\"");
    }
    let new = edge(&mut a);
    assert_eq!((a.doc.ents().edges[before].layer, a.doc.ents().edges[new].layer), (0, 1));
    ui.click(&mut a, "Visible : Murs");
    assert!(a.doc.model.layers[1].visible);
    assert_eq!(crate::warnings::front().as_deref(), Some("Vous ne pouvez pas masquer l'étiquette actuelle."));
    ui.click(&mut a, "OK");

    // A tag tagging a locked group: asked first, then where its entities go.
    let mut inst = None;
    a.doc.commit("Groupe", |m, d| {
        let items = vec![esq_core::model::EntityRef::Edge(before)];
        inst = m.make_group(d, &items, true, None);
    });
    let inst = inst.unwrap();
    a.doc.commit("t", |m, _| {
        let i = &mut m.root.instances[inst];
        i.layer = 2;
        i.locked = true;
    });
    ui.click(&mut a, "Meubles");
    ui.click(&mut a, "Supprimer l'étiquette");
    assert!(
        ui.has("Les étiquettes à supprimer sont utilisées par des groupes ou des composants verrouillés.  Continuer ?")
    );
    ui.click(&mut a, "Oui");
    assert!(ui.has("Supprimer les balises contenant des entités"));
    assert_eq!(ui.value("Autre étiquette"), "Sans étiquette");
    ui.choose(&mut a, "Autre étiquette", "Murs");
    ui.click(&mut a, "OK");
    assert_eq!(a.doc.model.layers.len(), 3);
    assert_eq!(a.doc.model.root.instances[inst].layer, 1, "le groupe passe à Murs");

    // A tag without entities goes at once; one with entities can take them along.
    ui.click(&mut a, "Vide");
    ui.click(&mut a, "Supprimer l'étiquette");
    assert_eq!(a.doc.model.layers.len(), 2);
    ui.click(&mut a, "Murs");
    ui.click(&mut a, "Supprimer l'étiquette");
    ui.click(&mut a, "Oui"); // the locked group is now tagged Murs
    ui.click(&mut a, "Supprimer les entités");
    ui.click(&mut a, "OK");
    assert_eq!(a.doc.model.layers.len(), 1);
    assert!(!a.doc.ents().edges.contains_key(new));
    // A locked group is never erased: it loses its tag.
    assert_eq!(a.doc.model.root.instances[inst].layer, 0);
    assert_eq!(tag_dialog::active(&a.doc.model), 0, "l'étiquette active redevient Sans étiquette");
    a.doc.undo();
    assert_eq!(a.doc.model.layers.len(), 2);
}

#[test]
fn workspaces_keep_a_layout_and_bring_it_back() {
    use crate::dialogs::workspaces::{self, Geometry};
    use crate::dialogs::{Dlg, trays};
    use crate::settings;

    let mut a = app();
    let mut ui = Driver::new();
    let ctx = ui.ctx.clone();
    if let Some(d) = workspaces::folder() {
        let _ = std::fs::remove_dir_all(&d);
    }
    // A layout of one's own: a tray, a toolbar hidden.
    trays::create(&mut a, "Rendu", &[Panel::Shadows, Panel::Scenes]).unwrap();
    // The toolbars as installed (tests running beside this one save theirs in the same file).
    a.ui_state.extra.toolbars = crate::toolbars::ToolbarConfig::from_text("");
    a.ui_state.extra.toolbars.set_shown("Visite", false);

    // Window › Workspace › New Workspace: SketchUp's configuration window.
    a.run(&ctx, Cmd::Dialog(Dlg::NewWorkspace));
    ui.frames(&mut a, 2);
    assert!(ui.has("Configuration de l'espace de travail"));
    assert_eq!(ui.value("Géométrie"), "Inchangé");
    assert_eq!(ui.value("Suivre"), "Tous");
    assert_eq!(ui.value("Non suivies"), "Inchangé");
    // The ID: SketchUp's mask, made by "Générer"; a wrong one blocks OK.
    ui.type_into(&mut a, "ID", "1234");
    assert!(ui.node("OK").disabled && ui.has("ID non valide"));
    ui.click(&mut a, "Générer");
    assert!(!ui.node("OK").disabled);
    ui.type_into(&mut a, "Nom de l'espace de travail", "Rendu photo");
    ui.click(&mut a, "Non");
    ui.choose(&mut a, "Géométrie", "Maximisé");
    ui.click(&mut a, "OK");
    assert!(!ui.has("Configuration de l'espace de travail"));
    let saved = workspaces::list();
    assert_eq!(saved.len(), 1);
    let ws = &saved[0];
    assert_eq!((ws.name.as_str(), ws.user_editable, ws.geometry), ("Rendu photo", false, Geometry::Maximized));
    assert!(workspaces::valid_id(&ws.id));
    assert_eq!(workspaces::current_id(), ws.id);
    assert_eq!(ws.trays.len(), 2);
    assert!(ws.toolbars.contains("hidden=Visite"));

    // Not editable: Configure refuses.
    a.run(&ctx, Cmd::Dialog(Dlg::ConfigureWorkspace));
    ui.frames(&mut a, 2);
    assert!(ui.has("Cet espace de travail n'est pas modifiable."));
    ui.click(&mut a, "OK");

    // Everything back as installed, then the workspace brings the layout back.
    crate::dialogs::pref_pages::reset_workspace(&mut a);
    assert!(settings::get().trays.is_empty() && a.ui_state.extra.toolbars.shown("Visite"));
    workspaces::apply(&mut a, &ctx, ws);
    assert_eq!(settings::get().trays.len(), 1);
    assert!(!a.ui_state.panels.contains(&Panel::Shadows));
    assert!(!a.ui_state.extra.toolbars.shown("Visite"));

    // Save to a file, load it back: the ID is already there, "Non" keeps a copy.
    let dir = std::env::temp_dir().join(format!("esq-ws-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("rendu.json");
    workspaces::save_file(&mut a, &ctx, &f).unwrap();
    workspaces::load_file(&mut a, &ctx, &f);
    ui.frames(&mut a, 2);
    assert!(ui.has("L'espace de travail existe"));
    ui.click(&mut a, "Non");
    let names: Vec<String> = workspaces::list().into_iter().map(|w| w.name).collect();
    assert_eq!(names, vec!["Rendu photo".to_string(), "Rendu photo Copie".to_string()]);
    // A file that is no workspace.
    let bad = dir.join("x.json");
    std::fs::write(&bad, "{}").unwrap();
    workspaces::load_file(&mut a, &ctx, &bad);
    ui.frames(&mut a, 2);
    assert!(a.ui_state.extra.dialogs.workspaces.error.as_deref().unwrap().starts_with("Échec de l'importation"));
    let _ = std::fs::remove_dir_all(&dir);
}
