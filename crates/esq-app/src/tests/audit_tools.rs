//! Tests for the audit of the basic tools against SketchUp's behaviour (one module per audit,
//! so that parallel branches do not collide in `tests.rs`).

use esq_core::glam::DVec3;
use esq_core::math::newell_normal;
use esq_core::model::{EdgeId, EntityRef};
use esq_core::tess::face_area;

use super::Harness;
use crate::tools::ToolId;

fn edge_between(h: &Harness, a: DVec3, b: DVec3) -> EdgeId {
    let ents = h.doc.ents();
    ents.edges
        .keys()
        .find(|&e| {
            let (p, q) = ents.edge_points(e);
            (p.distance(a) < 1e-6 && q.distance(b) < 1e-6) || (p.distance(b) < 1e-6 && q.distance(a) < 1e-6)
        })
        .expect("edge")
}

fn max_off_plane(h: &Harness) -> f64 {
    let ents = h.doc.ents();
    ents.faces
        .values()
        .map(|f| {
            let pts = ents.loop_points(&f.outer);
            let n = newell_normal(&pts);
            let c = pts.iter().copied().sum::<DVec3>() / pts.len() as f64;
            pts.iter().map(|p| (*p - c).dot(n).abs()).fold(0.0, f64::max)
        })
        .fold(0.0, f64::max)
}

fn extent(h: &Harness) -> (DVec3, DVec3) {
    let ents = h.doc.ents();
    let lo = ents.verts.values().map(|v| v.pos).fold(DVec3::splat(f64::MAX), DVec3::min);
    let hi = ents.verts.values().map(|v| v.pos).fold(DVec3::splat(f64::MIN), DVec3::max);
    (lo, hi)
}

/// Box 1000 x 600 x 400 with its right top edge selected, Move tool active, base clicked.
fn move_top_edge_setup() -> Harness {
    let mut h = Harness::new();
    h.rect_and_box();
    let e = edge_between(&h, DVec3::new(1000.0, 0.0, 400.0), DVec3::new(1000.0, 600.0, 400.0));
    h.doc.select(vec![EntityRef::Edge(e)]);
    h.tool(ToolId::Move);
    h.click(DVec3::new(1000.0, 300.0, 400.0));
    h
}

#[test]
fn move_keeps_neighbouring_faces_flat_by_default() {
    // The end faces (front and back) keep the edge from sliding along itself...
    let mut h = move_top_edge_setup();
    h.hover(DVec3::new(1000.0, 800.0, 400.0));
    h.click(DVec3::new(1000.0, 800.0, 400.0));
    assert_eq!(extent(&h).1.y, 600.0, "no move along y");
    assert_eq!(h.faces(), 6);
    // ...while moving it across is free and leaves every face flat.
    let mut h = move_top_edge_setup();
    h.hover(DVec3::new(1400.0, 300.0, 400.0));
    h.click(DVec3::new(1400.0, 300.0, 400.0));
    assert_eq!(h.faces(), 6, "no fold needed");
    assert!(max_off_plane(&h) < 1e-6);
    assert!((extent(&h).1.x - 1400.0).abs() < 1e-6);
}

#[test]
fn alt_toggles_auto_fold_which_moves_freely_and_folds_the_faces() {
    let mut h = move_top_edge_setup();
    h.with(|t, cx| t.alt_tap(cx));
    h.hover(DVec3::new(1000.0, 800.0, 400.0));
    h.click(DVec3::new(1000.0, 800.0, 400.0));
    // The edge slid along itself: the front and back faces are bent, each is cut in two.
    assert!((extent(&h).1.y - 1100.0).abs() < 1e-6, "{:?}", extent(&h));
    assert_eq!(h.faces(), 8);
    assert!(max_off_plane(&h) < 1e-6);
}

// ---- Rotate (CRotateTool) ----

fn rotate_setup() -> Harness {
    let mut h = Harness::new();
    h.rect_and_box();
    let all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    h.doc.select(all);
    h.tool(ToolId::Rotate);
    h
}

fn angle_shown(h: &Harness) -> f64 {
    let s = h.tool.vcb_value(esq_core::model::Units::Millimeters);
    s.trim_end_matches('°').replace(',', ".").parse().unwrap_or_else(|_| panic!("angle {s:?}"))
}

fn has_vertex(h: &Harness, p: DVec3) -> bool {
    h.doc.ents().verts.values().any(|v| v.pos.distance(p) < 1e-3)
}

/// A point of the ground plane at `deg` degrees around the origin and `px` pixels away.
fn ground_point(h: &Harness, deg: f64, px: f64) -> DVec3 {
    let r = px * h.cam.pixel_size_at(DVec3::ZERO);
    DVec3::new(r * deg.to_radians().cos(), r * deg.to_radians().sin(), 0.0)
}

#[test]
fn typed_slope_and_the_side_of_the_cursor_set_the_rotation() {
    // "1:1" is 45 degrees, counter-clockwise when the cursor is on the positive side.
    let mut h = rotate_setup();
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    h.hover(DVec3::new(0.0, 1000.0, 0.0));
    h.type_vcb("1:1");
    let c = 1000.0 / 2f64.sqrt();
    assert!(has_vertex(&h, DVec3::new(c, c, 0.0)));
    // With the cursor on the negative side a typed angle turns the other way.
    let mut h = rotate_setup();
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    h.hover(DVec3::new(0.0, -1000.0, 0.0));
    h.type_vcb("45");
    assert!(has_vertex(&h, DVec3::new(c, -c, 0.0)));
}

#[test]
fn protractor_snaps_on_its_rim_and_on_its_rays_but_is_free_inside() {
    let mut h = rotate_setup();
    h.click(DVec3::ZERO);
    h.click(ground_point(&h, 0.0, 75.0));
    // On the rim (75 px): 37 degrees snaps to 30.
    h.hover(ground_point(&h, 37.0, 75.0));
    assert_eq!(angle_shown(&h), 30.0);
    // Inside the rim and off the rays: the angle is free.
    h.hover(ground_point(&h, 37.5, 60.0));
    let a = angle_shown(&h);
    assert!((a - 37.5).abs() < 1.5 && a % 15.0 != 0.0, "a = {a}");
}

#[test]
fn rotation_goes_past_half_a_turn() {
    let mut h = rotate_setup();
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    h.hover(DVec3::new(0.0, 1000.0, 0.0));
    assert_eq!(angle_shown(&h), 90.0);
    h.hover(DVec3::new(-1000.0, 0.0, 0.0));
    assert_eq!(angle_shown(&h), 180.0);
    h.hover(DVec3::new(0.0, -1000.0, 0.0));
    assert_eq!(angle_shown(&h), 270.0);
}

#[test]
fn arrow_keys_lock_the_rotation_plane_until_the_start_point() {
    use crate::pick::{Lock, SnapKind};
    let mut h = rotate_setup();
    h.hover(DVec3::new(10.0, 10.0, 0.0));
    assert_eq!(
        h.tool.status(),
        "Cliquez pour définir le point central de la rotation. | Ctrl = Activer/Désactiver Copier. \
         | Flèches = Activer/Désactiver verrouiller le plan de rotation."
    );
    h.click(DVec3::ZERO);
    // Red arrow: the plane perpendicular to the red axis.
    h.lock = Some(Lock { dir: DVec3::X, color: [220, 30, 30], kind: SnapKind::Axis(0) });
    h.hover(DVec3::new(0.0, 700.0, 0.0));
    assert!(h.tool.status().starts_with("Déplacez le curseur pour localiser le point de départ"));
    h.with(|t, cx| t.ctrl_tap(cx));
    assert!(h.tool.status().contains("faire pivoter une copie."));
    h.with(|t, cx| t.ctrl_tap(cx));
    h.click(DVec3::new(0.0, 1000.0, 0.0));
    assert_eq!(
        h.tool.status(),
        "Cliquez pour définir la rotation ou entrez l'angle. | Ctrl = Activer/Désactiver Copier."
    );
    h.lock = None;
    h.hover(DVec3::new(0.0, 0.0, 1000.0));
    h.type_vcb("90");
    // Turned 90 degrees about the red axis: the 600 mm deep box now rises 600 mm.
    assert!((h.zmax() - 600.0).abs() < 1e-6, "zmax = {}", h.zmax());
}

// ---- Arcs (CArcTool, CArc3PointTool, CFit3PointArcTool) ----

fn edges_count(h: &Harness) -> usize {
    h.doc.ents().edges.len()
}

#[test]
fn arc_sides_in_arc_or_in_circle_within_two_and_999() {
    // 3-point half circle: "24c" makes 12 segments, "30s" makes 30.
    for (typed, expected) in [("24c", 12), ("30s", 30)] {
        let mut h = Harness::new();
        h.tool(ToolId::Arc3);
        h.type_vcb(typed);
        h.click(DVec3::ZERO);
        h.click(DVec3::new(500.0, 500.0, 0.0));
        h.click(DVec3::new(1000.0, 0.0, 0.0));
        assert_eq!(edges_count(&h), expected, "{typed}");
    }
    let mut h = Harness::new();
    h.tool(ToolId::Arc3);
    assert!(h.with(|t, cx| t.vcb(cx, "1s")).is_err());
    assert!(h.with(|t, cx| t.vcb(cx, "1000c")).is_err());
    // Ctrl + / Ctrl - stop at 2.
    h.type_vcb("3s");
    h.with(|t, cx| t.step(cx, -1));
    h.with(|t, cx| t.step(cx, -1));
    h.type_vcb("2s");
    h.click(DVec3::ZERO);
    h.click(DVec3::new(500.0, 500.0, 0.0));
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    assert_eq!(edges_count(&h), 2);
}

#[test]
fn collinear_three_point_arc_is_refused() {
    let mut h = Harness::new();
    h.tool(ToolId::Arc3);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(500.0, 0.0, 0.0));
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    assert_eq!(edges_count(&h), 0);
}

#[test]
fn alt_locks_the_two_point_arc_tangent_to_the_edge_it_starts_on() {
    let mut h = Harness::new();
    h.doc.commit("ligne", |m, d| {
        m.entities_mut(d).add_line(DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0));
    });
    h.tool(ToolId::Arc);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    h.click(DVec3::new(1500.0, 500.0, 0.0));
    h.with(|t, cx| t.alt_tap(cx));
    // Wherever the cursor goes, the arc is the quarter circle centred on (1000, 500).
    h.hover(DVec3::new(1400.0, 100.0, 0.0));
    h.click(DVec3::new(1400.0, 100.0, 0.0));
    let centre = DVec3::new(1000.0, 500.0, 0.0);
    let ents = h.doc.ents();
    let on_arc: Vec<_> = ents.verts.values().filter(|v| v.pos.x > 1000.0 - 1e-6).collect();
    assert!(on_arc.len() >= 12, "{}", on_arc.len());
    for v in on_arc {
        assert!((v.pos.distance(centre) - 500.0).abs() < 1e-3, "{:?}", v.pos);
    }
}

#[test]
fn two_edges_of_a_corner_at_equal_distance_make_a_rounded_corner() {
    let mut h = Harness::new();
    h.tool(ToolId::Rect);
    h.click(DVec3::ZERO);
    h.hover(DVec3::new(900.0, 500.0, 0.0));
    h.type_vcb("1000;600");
    assert_eq!(h.faces(), 1);
    h.tool(ToolId::Arc);
    h.click(DVec3::new(200.0, 0.0, 0.0));
    h.click(DVec3::new(0.0, 200.0, 0.0));
    assert!(h.tool.status().contains("rayon"), "{}", h.tool.status());
    h.type_vcb("150");
    assert_eq!(h.faces(), 1, "the sliver of the corner is gone");
    assert!(!has_vertex(&h, DVec3::ZERO), "the corner vertex is gone");
    assert!(has_vertex(&h, DVec3::new(150.0, 0.0, 0.0)) && has_vertex(&h, DVec3::new(0.0, 150.0, 0.0)));
    let f = h.doc.ents().faces.keys().next().unwrap();
    let area = face_area(h.doc.ents(), f);
    let cut = 150.0 * 150.0 * (1.0 - std::f64::consts::PI / 4.0);
    assert!((600_000.0 - cut - area).abs() < 60.0, "area {area}");
    // Points at unequal distances from the corner are a plain arc.
    let mut h2 = Harness::new();
    h2.tool(ToolId::Rect);
    h2.click(DVec3::ZERO);
    h2.hover(DVec3::new(900.0, 500.0, 0.0));
    h2.type_vcb("1000;600");
    h2.tool(ToolId::Arc);
    h2.click(DVec3::new(200.0, 0.0, 0.0));
    h2.click(DVec3::new(0.0, 400.0, 0.0));
    assert!(!h2.tool.status().contains("rayon"));
}

// ---- Freehand (CFreehandTool) ----

#[test]
fn freehand_announces_a_closed_curve_near_its_start() {
    let mut h = Harness::new();
    h.tool(ToolId::Freehand);
    let pts: Vec<DVec3> = (0..=20)
        .map(|i| {
            let a = i as f64 / 20.0 * std::f64::consts::TAU;
            DVec3::new(500.0 + 400.0 * a.cos(), 300.0 + 300.0 * a.sin(), 0.0)
        })
        .collect();
    // The status is CFreehandTool's all along; "Courbe fermée" is a tooltip, and a stroke
    // released near its start closes into a face.
    let status = h.tool.status();
    assert!(status.starts_with("Cliquez et faites glisser pour dessiner une courbe à main levée. | Flèches"));
    let first = h.px(pts[0]);
    h.with(|t, cx| t.press(cx, first));
    assert_eq!(h.tool.status(), status);
    for p in &pts[1..10] {
        let m = h.px(*p);
        h.with(|t, cx| t.mouse_move(cx, m));
    }
    let open = {
        let mut h2 = Harness::new();
        h2.tool(ToolId::Freehand);
        let first = h2.px(pts[0]);
        h2.with(|t, cx| t.press(cx, first));
        for p in &pts[1..10] {
            let m = h2.px(*p);
            h2.with(|t, cx| t.mouse_move(cx, m));
        }
        let last = h2.px(pts[9]);
        h2.with(|t, cx| t.release(cx, last));
        h2.faces()
    };
    assert_eq!(open, 0, "half a circle stays open");
    for p in &pts[10..] {
        let m = h.px(*p);
        h.with(|t, cx| t.mouse_move(cx, m));
    }
    let last = h.px(pts[20]);
    h.with(|t, cx| t.release(cx, last));
    assert_eq!(h.faces(), 1, "closed near its start");
}

#[test]
fn freehand_plane_can_be_locked_with_an_arrow_key() {
    use crate::pick::{Lock, SnapKind};
    let mut h = Harness::new();
    h.tool(ToolId::Freehand);
    h.lock = Some(Lock { dir: DVec3::X, color: [220, 30, 30], kind: SnapKind::Axis(0) });
    // Drawn on the plane x = 0 although the cursor is over the ground.
    let first = h.px(DVec3::ZERO);
    h.with(|t, cx| t.press(cx, first));
    for p in [DVec3::new(0.0, 500.0, 0.0), DVec3::new(0.0, 500.0, 400.0), DVec3::new(0.0, 0.0, 400.0)] {
        let m = h.px(p);
        h.with(|t, cx| t.mouse_move(cx, m));
    }
    let last = h.px(DVec3::new(0.0, 0.0, 400.0));
    h.with(|t, cx| t.release(cx, last));
    let ents = h.doc.ents();
    assert!(ents.verts.len() >= 3);
    assert!(ents.verts.values().all(|v| v.pos.x.abs() < 1e-6));
    assert!(ents.verts.values().any(|v| v.pos.z > 100.0), "it rose out of the ground");
}

#[test]
fn alt_and_ctrl_modes_exclude_each_other() {
    let mut h = Harness::new();
    h.tool(ToolId::Move);
    assert_eq!(
        h.tool.status(),
        "Cliquez sur quelque chose pour commencer à le déplacer. | Ctrl = Alterner entre copier/tamponner/déplacer. \
         | Alt = Activer/Désactiver le pliage automatique. \
         | Flèches = Activer/Désactiver verrouiller la direction de l'inférence."
    );
    h.with(|t, cx| t.alt_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez sur un élément pour le plier automatiquement. |"));
    h.with(|t, cx| t.ctrl_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez sur quelque chose pour commencer à le copier. |"));
    h.with(|t, cx| t.alt_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez sur un élément pour le plier"));
    h.with(|t, cx| t.alt_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez sur quelque chose pour commencer à le déplacer. |"));
}

#[test]
fn ctrl_cycles_copy_stamp_move_and_stamp_places_copies_until_esc() {
    let mut h = Harness::new();
    h.rect_and_box();
    let all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    h.doc.select(all);
    h.tool(ToolId::Move);
    h.hover(DVec3::new(1000.0, 0.0, 0.0));
    assert!(h.tool.status().starts_with("Cliquez pour commencer à déplacer les éléments présélectionnés. |"));
    h.with(|t, cx| t.ctrl_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez pour commencer à copier les éléments présélectionnés. |"));
    h.with(|t, cx| t.ctrl_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez pour créer plusieurs copies. | Ctrl = Alterner"));
    // Stamp: one base point, a copy at each click.
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    assert!(h.tool.status().starts_with("Cliquez pour créer plusieurs copies. | Ctrl = Déplacer. |"));
    h.click(DVec3::new(2500.0, 0.0, 0.0));
    assert_eq!(h.faces(), 12);
    h.hover(DVec3::new(1000.0, 1500.0, 0.0));
    h.type_vcb("2000");
    assert_eq!(h.faces(), 18);
    assert!(h.tool.busy(), "still stamping");
    h.with(|t, cx| t.cancel(cx));
    assert!(!h.tool.busy());
    let ymax = h.doc.ents().verts.values().map(|v| v.pos.y).fold(f64::MIN, f64::max);
    assert!((ymax - 2600.0).abs() < 1e-6, "ymax = {ymax}");
    // Ctrl once more: back to moving.
    h.with(|t, cx| t.ctrl_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez pour commencer à déplacer"));
    // While placing, Ctrl switches between moving and copying.
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    assert!(h.tool.status().contains(" | Ctrl = Copier. |"));
    h.with(|t, cx| t.ctrl_tap(cx));
    assert!(
        h.tool
            .status()
            .starts_with("Cliquez pour placer les éléments que vous copiez ou entrez une distance. | Ctrl = Déplacer.")
    );
}
