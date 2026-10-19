//! Tools › Comment, View › Comments and the Comments panel (services-commentaires): the tool
//! driven like the mouse, the panel through its accessibility tree.

use eframe::egui;
use esq_core::comments::Comment;
use esq_core::glam::DVec3;
use esq_core::model::EntityRef;

use super::Harness;
use super::ui_driver::{Driver, app};
use crate::app::EsquisseApp;
use crate::commands::Cmd;
use crate::comments::{self as cm, tool};
use crate::tools::ToolId;

fn placed() -> Option<tool::Placed> {
    cm::drain_tool_events().into_iter().last().flatten()
}

#[test]
fn the_comment_tool_pins_the_object_clicked_or_the_preselection() {
    cm::drain_tool_events();
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Comment);
    assert_eq!(h.tool.status(), tool::STATUS);
    // On the top face: the face is what the comment is about, the pin stands on it.
    h.click(DVec3::new(500.0, 300.0, 400.0));
    let p = placed().expect("a pin placed");
    assert_eq!(p.target, "Face");
    assert!((p.anchor - DVec3::new(500.0, 300.0, 400.0)).length() < 1.0, "{:?}", p.anchor);
    // A preselection is what the comment is about; the click only places the pin.
    let all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    h.doc.select(all);
    h.click(DVec3::new(500.0, 300.0, 400.0));
    assert_eq!(placed().unwrap().target, "6 entités");
    // Nothing under the cursor: « à la position 3D », on the ground.
    h.doc.select(Vec::new());
    h.click(DVec3::new(4000.0, 3000.0, 0.0));
    let p = placed().unwrap();
    assert_eq!(p.target, "");
    assert!(p.anchor.z.abs() < 1e-6);
    // Escape (or another tool) drops the comment being placed.
    h.with(|t, cx| t.cancel(cx));
    assert_eq!(cm::drain_tool_events(), vec![None]);
}

#[test]
fn the_comment_tool_refuses_too_large_a_selection() {
    cm::drain_tool_events();
    let mut h = Harness::new();
    // 26 groups (one edge each): more than 25 objects.
    let mut groups = Vec::new();
    h.doc.commit("Groupes", |m, d| {
        for i in 0..26 {
            let x = i as f64 * 100.0;
            let e = m.entities_mut(d).add_line(DVec3::new(x, 0.0, 0.0), DVec3::new(x, 50.0, 0.0));
            if let Some(g) = m.make_group(d, &[EntityRef::Edge(e[0])], true, Some(format!("g{i}"))) {
                groups.push(EntityRef::Instance(g));
            }
        }
    });
    assert_eq!(groups.len(), 26);
    h.doc.select(groups);
    assert_eq!(tool::selection_problem(&h.doc), Some(tool::TOO_MANY_OBJECTS));
    h.tool(ToolId::Comment);
    h.click(DVec3::new(500.0, 300.0, 0.0));
    assert!(placed().is_none(), "no comment on 26 objects");
    // 101 elements past the 25 counted: too many as well; 100 are fine.
    h.doc.commit("Arêtes", |m, d| {
        for i in 0..101 {
            let x = i as f64 * 10.0;
            m.entities_mut(d).add_line(DVec3::new(x, 1000.0, 0.0), DVec3::new(x, 1005.0, 0.0));
        }
    });
    let edges: Vec<EntityRef> = h.doc.ents().edges.keys().map(EntityRef::Edge).collect();
    assert_eq!(edges.len(), 101);
    h.doc.select(edges);
    assert_eq!(tool::selection_problem(&h.doc), Some(tool::TOO_MANY_ELEMENTS));
    h.doc.sel.truncate(100);
    assert_eq!(tool::selection_problem(&h.doc), None);
    h.click(DVec3::new(500.0, 300.0, 0.0));
    assert_eq!(placed().unwrap().target, "100 entités");
}

fn panel_driver() -> Driver {
    let mut d = Driver::new();
    d.side = Some(|app: &mut EsquisseApp, ui: &mut egui::Ui| crate::comments::show(app, ui.ctx()));
    d
}

/// Places a pin as the tool does and writes the comment in the panel.
fn write_comment(d: &mut Driver, a: &mut EsquisseApp, text: &str) {
    cm::place(tool::Placed { anchor: DVec3::new(100.0, 200.0, 300.0), target: "Face".into() });
    d.frames(a, 3);
    assert!(a.ui_state.extra.comments.open, "the panel opens with the comment to write");
    assert!(d.node("Envoyer").disabled, "nothing to send yet");
    d.focus(a, "Utilisez Ctrl + Entrée pour envoyer votre commentaire.");
    d.text(a, text);
    d.frames(a, 2);
    d.click(a, "Envoyer");
}

#[test]
fn a_comment_is_written_answered_resolved_and_undone_like_a_model_change() {
    let mut a = app();
    let mut d = panel_driver();
    a.run(&egui::Context::default(), Cmd::Tool(ToolId::Comment));
    d.frames(&mut a, 2);
    assert!(a.ui_state.extra.comments.open, "choosing the tool shows the panel");
    assert!(d.has("Pour commencer un nouveau commentaire"));
    write_comment(&mut d, &mut a, "Linteau à reprendre");
    let cs = &a.doc.model.comments;
    assert_eq!(cs.items.len(), 1);
    let c = cs.items[0].clone();
    assert_eq!((c.text.as_str(), c.target.as_str()), ("Linteau à reprendre", "Face"));
    assert_eq!(c.author, a.ui_state.extra.comments.me());
    assert_eq!(c.author, crate::settings::user_name(), "signed with the Windows user name, like Claim Credit");
    assert!(c.camera.is_some(), "the view is saved with the comment");
    assert!(a.doc.dirty);
    // The thread opens after sending; answer it with Ctrl + Entrée.
    assert_eq!(a.ui_state.extra.comments.thread.as_deref(), Some(c.id.as_str()));
    d.focus(&mut a, "Utilisez Ctrl + Entrée pour envoyer votre commentaire.");
    d.text(&mut a, "Vu, je m'en occupe");
    d.key(&mut a, egui::Key::Enter, egui::Modifiers::COMMAND);
    assert_eq!(a.doc.model.comments.items[0].reply_count(), 1);
    // Resolve, then back to the list: it is folded under « Résolu ».
    d.click(&mut a, "Marquer comme résolu");
    assert!(a.doc.model.comments.items[0].resolved);
    d.click(&mut a, "< Retour");
    assert!(d.has("Résolu (1) ▾"));
    assert!(!d.has("Linteau à reprendre"), "folded");
    d.click(&mut a, "Résolu (1) ▾");
    assert!(d.has("Linteau à reprendre"));
    // Each change is an undo step.
    a.doc.undo();
    assert!(!a.doc.model.comments.items[0].resolved);
    a.doc.undo();
    assert_eq!(a.doc.model.comments.items[0].reply_count(), 0);
    a.doc.undo();
    assert!(a.doc.model.comments.items.is_empty());
    a.doc.redo();
    assert_eq!(a.doc.model.comments.items.len(), 1);
    assert_eq!(a.doc.undo_label(), Some("Créer un commentaire"));
    // Delete conversation (the card's Options), and undo it.
    d.frames(&mut a, 2);
    assert!(d.has("Linteau à reprendre") && !d.has("1 réponse"));
    d.click(&mut a, "Options");
    d.click(&mut a, "Supprimer la conversation");
    assert!(a.doc.model.comments.items.is_empty());
    a.doc.undo();
    assert_eq!(a.doc.model.comments.items.len(), 1);
}

#[test]
fn the_filter_shows_read_and_unread_threads() {
    let mut a = app();
    let mut d = panel_driver();
    let mut c = Comment::new(DVec3::X, "", "Quelqu'un d'autre", "Non lu encore", 1_000);
    c.camera = None;
    a.doc.model.comments.add(c.clone());
    a.ui_state.extra.comments.open = true;
    d.frames(&mut a, 3);
    assert!(d.has("Non lu encore"));
    d.click(&mut a, "Filtre");
    d.click(&mut a, "Non lu");
    assert!(!d.has("Non lu encore"), "unread threads hidden");
    a.ui_state.extra.comments.show_unread = true;
    a.ui_state.extra.comments.show_read = false;
    d.frames(&mut a, 2);
    assert!(d.has("Non lu encore"));
    // Opened, it is read.
    d.click(&mut a, "Non lu encore");
    d.click(&mut a, "< Retour");
    assert!(!d.has("Non lu encore"), "read threads hidden");
}

#[test]
fn view_comments_hides_and_shows_the_pins() {
    let mut a = app();
    let ctx = egui::Context::default();
    assert_eq!(a.cmd_checked(Cmd::Comments), Some(true));
    a.run(&ctx, Cmd::Comments);
    assert_eq!(a.cmd_checked(Cmd::Comments), Some(false));
    a.run(&ctx, Cmd::Comments);
    assert!(a.ui_state.extra.comments.pins);
    assert!(crate::menus::command_paths().iter().any(|(p, c)| p == "Affichage › Commentaires" && *c == Cmd::Comments));
    assert!(crate::menus::command_paths().iter().any(|(_, c)| *c == Cmd::Tool(ToolId::Comment)));
}

#[test]
fn a_click_on_a_pin_opens_its_thread_and_flies_to_its_view() {
    let mut a = app();
    a.cam.size = esq_core::glam::DVec2::new(1200.0, 800.0);
    let mut c = Comment::new(DVec3::ZERO, "", "Théo", "Ici", 1_000);
    let view = esq_core::model::SceneCamera {
        eye: DVec3::new(0.0, -8000.0, 1500.0),
        target: DVec3::new(0.0, 0.0, 1500.0),
        up: DVec3::Z,
        fov_deg: 35.0,
        perspective: true,
    };
    c.camera = Some(view);
    let id = c.id.clone();
    a.doc.model.comments.add(c);
    let ctx = egui::Context::default();
    let painter = egui::Painter::new(ctx, egui::LayerId::background(), egui::Rect::EVERYTHING);
    let corner = a.cam.project(DVec3::ZERO).unwrap();
    // Beside the pin: not for the comments.
    assert!(!cm::click_pin(
        &mut a,
        painter.clone(),
        egui::Pos2::ZERO,
        corner + esq_core::glam::DVec2::new(-20.0, 10.0)
    ));
    // On the bubble (it stands on the point, up and to the right).
    assert!(cm::click_pin(&mut a, painter.clone(), egui::Pos2::ZERO, corner + esq_core::glam::DVec2::new(10.0, -10.0)));
    assert_eq!(a.ui_state.extra.comments.thread.as_deref(), Some(id.as_str()));
    assert!((a.cam.to_view().eye - view.eye).length() < 1e-6, "the saved view");
    // Camera › Previous goes back.
    assert!(!a.cam_back.is_empty());
    // Hidden pins are not clicked.
    cm::toggle_pins(&mut a);
    let corner = a.cam.project(DVec3::ZERO).unwrap();
    assert!(!cm::click_pin(&mut a, painter, egui::Pos2::ZERO, corner + esq_core::glam::DVec2::new(10.0, -10.0)));
}

#[test]
fn comments_go_through_the_esq_file() {
    let mut a = app();
    let mut d = panel_driver();
    write_comment(&mut d, &mut a, "Dans le fichier");
    let path = std::env::temp_dir().join(format!("esq-commentaires-{}.esq", std::process::id()));
    a.save_to(&path).unwrap();
    let mut b = app();
    b.open_path(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(b.doc.model.comments, a.doc.model.comments);
}

#[test]
fn answers_of_others_are_new_until_the_thread_is_read() {
    let mut a = app();
    let mut d = panel_driver();
    write_comment(&mut d, &mut a, "Mon commentaire");
    let id = a.doc.model.comments.items[0].id.clone();
    d.click(&mut a, "< Retour");
    // Someone else answered in this copy of the model (a file passed on).
    a.doc.model.comments.get_mut(&id).unwrap().reply("Alice", "Réponse", None, esq_core::comments::now_ms() + 1_000);
    d.frames(&mut a, 2);
    assert!(d.has("1 nouvelle réponse"));
    d.click(&mut a, "Mon commentaire");
    d.click(&mut a, "< Retour");
    assert!(!d.has("1 nouvelle réponse"));
    assert!(d.has("1 réponse"));
}
