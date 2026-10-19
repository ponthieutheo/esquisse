//! Tests of the remaining tool details: what the first tool tests and
//! the texture tests left out, read from SketchUp.exe and driven like a user.

use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::model::{EntityRef, InstanceId, SnapPoint};

use super::Harness;
use crate::pick::{Handle, SnapKind};
use crate::tools::ToolId;

/// The ends of the open chain of edges of the model (the vertices used by one edge).
fn chain_ends(h: &Harness) -> Vec<DVec3> {
    let ents = h.doc.ents();
    let mut uses = std::collections::HashMap::new();
    for e in ents.edges.values() {
        for v in e.v {
            *uses.entry(v).or_insert(0) += 1;
        }
    }
    uses.into_iter().filter(|(_, n)| *n == 1).map(|(v, _)| ents.pos(v)).collect()
}

// ---- Line (CSketchTool): Alt cycles the linear inferences while drawing ----

#[test]
fn line_alt_cycles_linear_inferences_all_off_then_parallel_perpendicular_only() {
    let mut h = Harness::new();
    // An edge along (1, 1): hovering it teaches the parallel inference.
    h.doc.commit("ligne", |m, d| {
        m.entities_mut(d).add_line(DVec3::new(0.0, 2000.0, 0.0), DVec3::new(1000.0, 3000.0, 0.0));
    });
    h.tool(ToolId::Line);
    // Before the first click Alt does nothing, and the status does not offer it.
    h.with(|t, cx| t.alt_tap(cx));
    assert!(!h.tool.status().contains("Alt"));
    h.hover(DVec3::new(500.0, 2500.0, 0.0));
    h.click(DVec3::ZERO);
    assert!(h.tool.status().contains("| Alt = Activer/désactiver les inférences linéaires (Tous activés). |"));
    let on_red = DVec3::new(800.0, 0.0, 0.0);
    let parallel = DVec3::new(700.0, 700.0, 0.0);
    h.hover(on_red);
    assert_eq!(h.tool.snap().unwrap().kind, SnapKind::Axis(0));
    h.hover(parallel);
    assert_eq!(h.tool.snap().unwrap().kind, SnapKind::Parallel);
    // Alt: all off — neither the axis nor the parallel.
    h.with(|t, cx| t.alt_tap(cx));
    assert!(h.tool.status().contains("(Tous désactivés)."));
    for p in [on_red, parallel] {
        h.hover(p);
        assert_eq!(h.tool.snap().unwrap().kind, SnapKind::Ground, "{p:?}");
    }
    // Alt: parallel/perpendicular only — the parallel, not the axis.
    h.with(|t, cx| t.alt_tap(cx));
    assert!(h.tool.status().contains("(Parallèle/perpendiculaire uniquement)."));
    h.hover(on_red);
    assert_eq!(h.tool.snap().unwrap().kind, SnapKind::Ground);
    h.hover(parallel);
    assert_eq!(h.tool.snap().unwrap().kind, SnapKind::Parallel);
    // Alt: all on again.
    h.with(|t, cx| t.alt_tap(cx));
    h.hover(on_red);
    assert_eq!(h.tool.snap().unwrap().kind, SnapKind::Axis(0));
    // Points are still found with everything off.
    h.with(|t, cx| t.alt_tap(cx));
    h.hover(DVec3::new(1000.0, 3000.0, 0.0));
    assert_eq!(h.tool.snap().unwrap().kind, SnapKind::Endpoint);
}

// ---- 3-Point Arc (CFit3PointArcTool): the angle typed after the second point ----

#[test]
fn three_point_arc_takes_an_angle_typed_after_the_second_point() {
    let mut h = Harness::new();
    h.tool(ToolId::Arc3);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(500.0, 500.0, 0.0));
    // The cursor at (1000, 0): the circle through the three points is centred on (500, 0),
    // the arc from the start to the cursor is a half turn.
    h.hover(DVec3::new(1000.0, 0.0, 0.0));
    assert_eq!(h.tool.vcb_label(), "Angle");
    assert_eq!(h.tool.vcb_value(esq_core::model::Units::Millimeters), "180,0°");
    assert!(h.tool.status().starts_with("Cliquez pour définir le point d'arrivée ou entrez l'angle."));
    // 90°: the same circle, the arc stops a quarter turn from the start (on the second point).
    h.type_vcb("90");
    assert!(!h.tool.busy(), "the arc is made");
    let ents = h.doc.ents();
    assert!(!ents.edges.is_empty());
    let centre = DVec3::new(500.0, 0.0, 0.0);
    for v in ents.verts.values() {
        assert!((v.pos.distance(centre) - 500.0).abs() < 1e-6, "{:?} is off the circle", v.pos);
    }
    let ends = chain_ends(&h);
    assert_eq!(ends.len(), 2);
    assert!(ends.iter().any(|p| p.distance(DVec3::ZERO) < 1e-6));
    assert!(ends.iter().any(|p| p.distance(DVec3::new(500.0, 500.0, 0.0)) < 1e-6), "{ends:?}");
}

#[test]
fn three_point_arc_refuses_an_angle_out_of_a_turn() {
    let mut h = Harness::new();
    h.tool(ToolId::Arc3);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(500.0, 500.0, 0.0));
    h.hover(DVec3::new(1000.0, 0.0, 0.0));
    for bad in ["0", "-30", "400", "abc"] {
        assert!(h.with(|t, cx| t.vcb(cx, bad)).is_err(), "{bad}");
    }
    assert!(h.tool.busy());
    // 270°: three quarters of the circle, past the cursor.
    h.type_vcb("270");
    let ends = chain_ends(&h);
    assert!(ends.iter().any(|p| p.distance(DVec3::new(500.0, -500.0, 0.0)) < 1e-6), "{ends:?}");
}

// ---- Move (CMoveTool): grips of a selected component, its snaps, the tractor beam ----

/// A 1000 × 600 × 400 box made a component (`a`), with a snap in the middle of its +X side
/// facing +X (up +Z), and a second instance `b` of it 2500 mm along red. The view sees both.
fn two_snapped_boxes(h: &mut Harness) -> (InstanceId, InstanceId) {
    h.rect_and_box();
    let items: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    let mut a = None;
    h.doc.commit("Composant", |m, d| a = m.make_group(d, &items, false, None));
    let a = a.unwrap();
    let def = h.doc.ents().instances[a].def;
    let mut b = None;
    h.doc.commit("Ancrage", |m, d| {
        m.add_snap(Some(def), SnapPoint::new(DVec3::new(1000.0, 300.0, 100.0), DVec3::X, DVec3::Z));
        let mut i = m.entities(d).instances[a].clone();
        i.xform = DMat4::from_translation(DVec3::new(2500.0, 0.0, 0.0));
        b = Some(m.entities_mut(d).instances.insert(i));
    });
    h.cam.target = DVec3::new(1800.0, 300.0, 200.0);
    h.cam.distance = 9000.0;
    (a, b.unwrap())
}

fn kind_at(h: &mut Harness, p: DVec3) -> SnapKind {
    h.hover(p);
    let s = h.tool.snap().unwrap().clone();
    assert!(s.pos.distance(p) < 1e-6 || !matches!(s.kind, SnapKind::Handle(..)), "{:?} at {:?}", s.kind, s.pos);
    s.kind
}

fn preview_xform(h: &Harness, i: InstanceId) -> DMat4 {
    h.doc.preview.as_ref().expect("preview").entities(h.doc.ctx_def()).instances[i].xform
}

#[test]
fn move_offers_the_grips_of_a_selected_component_and_alt_cycles_them() {
    let mut h = Harness::new();
    let (a, _) = two_snapped_boxes(&mut h);
    h.doc.select(vec![EntityRef::Instance(a)]);
    h.tool(ToolId::Move);
    // Near a corner of the box (not its origin): the corner, named as SketchUp does.
    let corner = DVec3::new(1000.0, 600.0, 400.0);
    h.hover_px(h.px(corner) + DVec2::new(4.0, 3.0));
    let s = h.tool.snap().unwrap().clone();
    assert_eq!(s.kind, SnapKind::Handle(Handle::Corner, false));
    assert!(s.pos.distance(corner) < 1e-6);
    assert_eq!(s.kind.label(), "Coin du composant");
    assert!(h.tool.status().contains("| Alt = Parcourez les types de poignées. |"), "{}", h.tool.status());
    // Alt on a grip does nothing.
    h.with(|t, cx| t.alt_tap(cx));
    assert_eq!(kind_at(&mut h, corner), SnapKind::Handle(Handle::Corner, false));
    // Elsewhere over the box: the midpoints of its edges, the centres of its sides, its centre.
    let inside = DVec3::new(250.0, 120.0, 400.0);
    h.hover(inside);
    h.with(|t, cx| t.alt_tap(cx));
    assert_eq!(kind_at(&mut h, DVec3::new(500.0, 600.0, 400.0)), SnapKind::Handle(Handle::Midpoint, false));
    h.hover(inside);
    h.with(|t, cx| t.alt_tap(cx));
    // (The centre of a side facing the eye is a rotation grip: the one underneath.)
    assert_eq!(kind_at(&mut h, DVec3::new(500.0, 300.0, 0.0)), SnapKind::Handle(Handle::SideCenter, false));
    h.hover(inside);
    h.with(|t, cx| t.alt_tap(cx));
    let centre = DVec3::new(500.0, 300.0, 200.0);
    assert_eq!(kind_at(&mut h, centre), SnapKind::Handle(Handle::Center, false));
    // Taken by its centre, the box goes where the centre is put.
    h.click(centre);
    h.click(DVec3::new(500.0, 300.0, 1200.0));
    let x = h.doc.ents().instances[a].xform;
    assert!(x.transform_point3(centre).distance(DVec3::new(500.0, 300.0, 1200.0)) < 1e-6);
}

#[test]
fn move_by_a_snap_turns_the_component_to_face_another_snap() {
    let mut h = Harness::new();
    let (a, _) = two_snapped_boxes(&mut h);
    h.doc.select(vec![EntityRef::Instance(a)]);
    h.tool(ToolId::Move);
    let own = DVec3::new(1000.0, 300.0, 100.0);
    let s = kind_at(&mut h, own);
    assert_eq!(s, SnapKind::Handle(Handle::Snap, false));
    assert_eq!(s.label(), "Ancrage");
    h.click(own);
    // Over the snap of the other box: facing it, both snaps meet.
    let target = DVec3::new(3500.0, 300.0, 100.0);
    assert_eq!(kind_at(&mut h, target), SnapKind::Handle(Handle::Snap, false));
    h.click(target);
    assert_eq!(h.doc.undo_label(), Some("Déplacer"));
    let x = h.doc.ents().instances[a].xform;
    assert!(x.transform_point3(own).distance(target) < 1e-6);
    assert!(x.transform_vector3(DVec3::X).abs_diff_eq(-DVec3::X, 1e-9), "{:?}", x.transform_vector3(DVec3::X));
    assert!(x.transform_vector3(DVec3::Z).abs_diff_eq(DVec3::Z, 1e-9));
}

#[test]
fn move_by_a_snap_tractor_beam_and_faces() {
    let mut h = Harness::new();
    let (a, _) = two_snapped_boxes(&mut h);
    h.doc.select(vec![EntityRef::Instance(a)]);
    h.tool(ToolId::Move);
    let own = DVec3::new(1000.0, 300.0, 100.0);
    h.hover(own);
    h.click(own);
    // On the ground, away from the other snap: it is the tractor beam's; off, nothing turns.
    let free = DVec3::new(1500.0, -1500.0, 0.0);
    h.hover(free);
    assert!(h.tool.status().contains("| Alt = Activer/désactiver le faisceau tracteur. |"), "{}", h.tool.status());
    let x = preview_xform(&h, a);
    assert!(x.transform_vector3(DVec3::X).abs_diff_eq(DVec3::X, 1e-9));
    // Alt: the beam is on, the box turns to face the other snap, where the cursor is.
    h.with(|t, cx| t.alt_tap(cx));
    let x = preview_xform(&h, a);
    assert!(x.transform_vector3(DVec3::X).abs_diff_eq(-DVec3::X, 1e-9));
    assert!(x.transform_point3(own).distance(free) < 1e-6);
    // Alt again: off. Over a face (the other box's top): the snap points into it, up red.
    h.with(|t, cx| t.alt_tap(cx));
    let top = DVec3::new(3000.0, 300.0, 400.0);
    h.hover(top);
    let x = preview_xform(&h, a);
    assert!(x.transform_vector3(DVec3::X).abs_diff_eq(-DVec3::Z, 1e-9), "{:?}", x.transform_vector3(DVec3::X));
    assert!(x.transform_vector3(DVec3::Z).abs_diff_eq(DVec3::X, 1e-9));
    assert!(x.transform_point3(own).distance(top) < 1e-6);
    // Esc: nothing changes.
    h.with(|t, cx| t.cancel(cx));
    assert!(h.doc.ents().instances[a].xform.abs_diff_eq(DMat4::IDENTITY, 1e-12));
}

// ---- Placing a component (CComponentTool) ----

#[test]
fn placing_a_component_selects_it_and_hands_over_to_move_unless_repeating() {
    let mut h = Harness::new();
    let (a, _) = two_snapped_boxes(&mut h);
    let def = h.doc.ents().instances[a].def;
    let count = |h: &Harness| h.doc.ents().instances.len();
    h.tool = Box::new(crate::tools::PlaceTool::new(def));
    h.click(DVec3::new(0.0, 3000.0, 0.0));
    assert_eq!(count(&h), 3);
    assert!(matches!(h.doc.sel.as_slice(), [EntityRef::Instance(i)] if *i != a));
    assert_eq!(h.tool.switch_to(), Some(ToolId::Move));
    assert_eq!(h.tool.switch_to(), None, "once");
    // Repeat (`Model#place_component(d, true)`): one more copy per click, the tool stays.
    h.doc.select(vec![]);
    h.tool = Box::new(crate::tools::PlaceTool::repeating(def));
    h.click(DVec3::new(0.0, 5000.0, 0.0));
    h.click(DVec3::new(2000.0, 5000.0, 0.0));
    assert_eq!(count(&h), 5);
    assert!(h.doc.sel.is_empty());
    assert_eq!(h.tool.switch_to(), None);
}

#[test]
fn ruby_place_component_gives_the_component_tool() {
    let mut app = crate::app::EsquisseApp::bare(None, None);
    let code = "m = Sketchup.active_model
        d = m.definitions.add('Bloc')
        d.entities.add_face([0,0,0], [100,0,0], [100,100,0], [0,100,0])
        r = m.place_component(d, true)
        t = begin; m.place_component(m); rescue TypeError => e; e.message; end
        [r == m, t].inspect";
    let Some(out) = super::dialogs::ruby(&mut app, code) else { return };
    assert!(out.contains("true"), "{out}");
    assert!(out.contains("wrong argument type (expected Sketchup::ComponentDefinition)"), "{out}");
    assert_eq!(app.tool.id(), ToolId::Component);
    // Repeating: every click places one more.
    let mut h = Harness::new();
    h.doc = std::mem::replace(&mut app.doc, crate::doc::Document::new(esq_core::model::Model::new()));
    h.tool = std::mem::replace(&mut app.tool, ToolId::Select.make());
    h.click(DVec3::new(0.0, 0.0, 0.0));
    h.click(DVec3::new(500.0, 0.0, 0.0));
    assert_eq!(h.doc.ents().instances.len(), 2);
    assert_eq!(h.tool.switch_to(), None);
    // Without repeat: one placement, then Move.
    let mut app = crate::app::EsquisseApp::bare(None, None);
    let code = "m = Sketchup.active_model
        d = m.definitions.add('Bloc')
        d.entities.add_face([0,0,0], [100,0,0], [100,100,0], [0,100,0])
        m.place_component(d).class.name";
    let Some(out) = super::dialogs::ruby(&mut app, code) else { return };
    assert!(out.contains("Sketchup::Model"), "{out}");
    h.doc = std::mem::replace(&mut app.doc, crate::doc::Document::new(esq_core::model::Model::new()));
    h.tool = std::mem::replace(&mut app.tool, ToolId::Select.make());
    h.click(DVec3::new(0.0, 0.0, 0.0));
    assert_eq!(h.tool.switch_to(), Some(ToolId::Move));
}

// ---- Materials › Edit: "Match color on screen" (CMaterialEditPage) ----

#[test]
fn match_color_on_screen_takes_the_pixel_under_the_next_press() {
    use eframe::egui::{self, Color32, ColorImage, pos2};

    use crate::mat_panel::{ScreenPick, give_screenshot, pixel_at, screen_pick_state};
    use crate::tests::ui_driver::{Driver, app};

    // A 4 × 2 window image at 2 pixels per point: the point (1.2, 0.6) is the pixel (2, 1).
    let mut px = vec![Color32::BLACK; 8];
    px[4 + 2] = Color32::from_rgb(10, 120, 200);
    let img = ColorImage::new([4, 2], px);
    assert_eq!(pixel_at(&img, pos2(1.2, 0.6), 2.0), Some([10, 120, 200]));
    assert_eq!(pixel_at(&img, pos2(2.0, 0.0), 2.0), None);
    assert_eq!(pixel_at(&img, pos2(-1.0, 0.0), 2.0), None);

    // In the panel: the button arms the pick, the next press anywhere asks for the image of
    // the window, whose pixel there becomes the colour (one undo step); the button goes up.
    let mut a = app();
    let id = a.doc.model.add_material("Brique", [180, 90, 40]);
    a.material = Some(id);
    let mut ui = Driver::new();
    ui.side = Some(|a, ui| {
        egui::Panel::right("side").exact_size(330.0).show(ui, |ui| crate::mat_panel::show(a, ui));
    });
    ui.frames(&mut a, 2);
    ui.click(&mut a, "Éditer");
    ui.click(&mut a, "Faire correspondre à la couleur à l’écran");
    assert_eq!(screen_pick_state(), Some(ScreenPick::Armed(id)));
    let at = pos2(200.0, 300.0);
    ui.click_at(&mut a, at);
    assert_eq!(screen_pick_state(), Some(ScreenPick::Waiting(id, at)));
    assert_eq!(a.doc.model.materials[id].color, [180, 90, 40], "nothing else took the press");
    let mut px = vec![Color32::WHITE; 1600 * 1000];
    px[300 * 1600 + 200] = Color32::from_rgb(12, 34, 56);
    give_screenshot(ColorImage::new([1600, 1000], px));
    ui.frames(&mut a, 2);
    assert_eq!(a.doc.model.materials[id].color, [12, 34, 56]);
    assert_eq!(a.doc.undo_label(), Some("Modifier la matière"));
    assert_eq!(screen_pick_state(), None);
}
