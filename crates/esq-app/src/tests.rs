//! Drives the tools with synthetic mouse/keyboard events, the way a user would.

use eframe::egui::Modifiers;
use esq_core::glam::{DVec2, DVec3};
use esq_core::model::{EntityRef, Model};
use esq_core::tess::face_area;

use crate::camera::Camera;
use crate::doc::Document;
use crate::pick::{InferCx, PickCache, Recent, SnapKind};
use crate::tools::{Tool, ToolCx, ToolId};

mod app_audit;
mod associativite;
mod audit_fin;
mod bench;
mod comments;
mod context_menu;
mod dialogs;
mod dialogs_annot;
mod dialogs_classif;
mod dialogs_comp;
mod dialogs_file;
mod dialogs_misc;
mod dialogs_panels;
mod dialogs_prefs;
mod display;
mod formats3d;
pub(crate) mod gpu;
mod image_tool;
pub(crate) mod ruby;
mod ruby_audit;
mod ruby_audit2;
mod ruby_formats;
mod ruby_model;
mod ruby_reel;
mod ruby_tools;
mod ruby_view;
mod sculpt;
mod securite;
mod skp_complet;
mod textures;
pub(crate) mod ui_driver;

struct Harness {
    doc: Document,
    cam: Camera,
    pick: PickCache,
    pick_rev: u64,
    tool: Box<dyn Tool>,
    recent: Option<Recent>,
    style: crate::style::ViewStyle,
    mods: Modifiers,
    lock: Option<crate::pick::Lock>,
    material: Option<esq_core::model::MaterialId>,
}

impl Harness {
    fn new() -> Self {
        let cam = Camera {
            size: DVec2::new(1280.0, 800.0),
            target: DVec3::new(500.0, 300.0, 200.0),
            distance: 5000.0,
            yaw: (-57.0f64).to_radians(),
            pitch: 22.0f64.to_radians(),
            ..Default::default()
        };
        Harness {
            doc: Document::new(Model::new()),
            cam,
            pick: PickCache::default(),
            pick_rev: 0,
            tool: ToolId::Select.make(),
            recent: None,
            style: Default::default(),
            mods: Modifiers::NONE,
            material: None,
            lock: None,
        }
    }

    fn tool(&mut self, id: ToolId) {
        self.tool = id.make();
    }

    fn with<R>(&mut self, f: impl FnOnce(&mut dyn Tool, &mut ToolCx) -> R) -> R {
        if self.pick_rev != self.doc.model_rev {
            self.pick = PickCache::build(&self.doc.model, &self.style);
            self.pick_rev = self.doc.model_rev;
        }
        let axes = if self.doc.ctx.is_empty() { self.doc.model.axes } else { self.doc.ctx_xform() };
        let mut mat = self.material;
        let mut msg = None;
        let mut tag = 0;
        let mut new_cam = None;
        let mut edit_text = None;
        let mut show_sections = false;
        let mut cx = ToolCx {
            doc: &mut self.doc,
            infer: InferCx { cam: &self.cam, pick: &self.pick, axes, recent: self.recent },
            lock: self.lock,
            mods: self.mods,
            units: esq_core::model::Units::Millimeters,
            material: &mut mat,
            tag: &mut tag,
            message: &mut msg,
            new_cam: &mut new_cam,
            edit_text: &mut edit_text,
            show_sections: &mut show_sections,
        };
        let r = f(self.tool.as_mut(), &mut cx);
        self.material = mat;
        if show_sections && !self.style.section_planes {
            self.style.section_planes = true;
            self.pick_rev = u64::MAX;
        }
        if let Some(m) = msg {
            eprintln!("message : {m}");
        }
        if let Some(c) = new_cam {
            self.cam = c;
        }
        r
    }

    fn px(&self, p: DVec3) -> DVec2 {
        self.cam.project(p).expect("point in front of the camera")
    }

    fn hover(&mut self, p: DVec3) {
        let m = self.px(p);
        self.hover_px(m);
    }

    fn hover_px(&mut self, m: DVec2) {
        self.with(|t, cx| t.mouse_move(cx, m));
        if let Some(r) = self.tool.snap().and_then(crate::pick::recent_from) {
            self.recent = Some(r);
        }
    }

    fn click(&mut self, p: DVec3) {
        let m = self.px(p);
        self.with(|t, cx| {
            t.mouse_move(cx, m);
            t.press(cx, m);
            t.release(cx, m);
        });
    }

    fn type_vcb(&mut self, s: &str) {
        let r = self.with(|t, cx| t.vcb(cx, s));
        r.expect("VCB accepted");
    }

    fn faces(&self) -> usize {
        self.doc.ents().faces.len()
    }

    fn zmax(&self) -> f64 {
        self.doc.ents().verts.values().map(|v| v.pos.z).fold(f64::MIN, f64::max)
    }

    fn rect_and_box(&mut self) {
        self.tool(ToolId::Rect);
        self.click(DVec3::ZERO);
        self.hover(DVec3::new(900.0, 500.0, 0.0));
        self.type_vcb("1000;600");
        assert_eq!(self.faces(), 1);
        self.tool(ToolId::PushPull);
        self.click(DVec3::new(500.0, 300.0, 0.0));
        self.hover(DVec3::new(500.0, 300.0, 250.0));
        self.type_vcb("400");
    }
}

#[test]
fn rectangle_from_typed_dimensions() {
    let mut h = Harness::new();
    h.tool(ToolId::Rect);
    h.click(DVec3::ZERO);
    h.hover(DVec3::new(900.0, 500.0, 0.0));
    h.type_vcb("1000;600");
    let ents = h.doc.ents();
    assert_eq!(ents.faces.len(), 1);
    let f = ents.faces.keys().next().unwrap();
    assert!((face_area(ents, f) - 600_000.0).abs() < 1e-6);
}

#[test]
fn push_pull_follows_the_mouse_direction() {
    let mut h = Harness::new();
    h.rect_and_box();
    assert_eq!(h.faces(), 6);
    assert!((h.zmax() - 400.0).abs() < 1e-6, "zmax = {}", h.zmax());
    // Typing again changes the last push/pull, as in SketchUp.
    h.type_vcb("700");
    assert_eq!(h.faces(), 6);
    assert!((h.zmax() - 700.0).abs() < 1e-6, "zmax = {}", h.zmax());
}

#[test]
fn line_tool_closes_a_loop_and_stops() {
    let mut h = Harness::new();
    h.tool(ToolId::Line);
    let pts = [
        DVec3::ZERO,
        DVec3::new(1000.0, 0.0, 0.0),
        DVec3::new(1000.0, 800.0, 0.0),
        DVec3::new(0.0, 800.0, 0.0),
        DVec3::ZERO,
    ];
    for p in pts {
        h.click(p);
    }
    assert_eq!(h.faces(), 1);
    assert_eq!(h.doc.ents().edges.len(), 4);
    assert!(!h.tool.busy(), "chain ends when a face is created");
}

#[test]
fn inference_finds_endpoints_and_midpoints() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Line);
    let near = h.px(DVec3::new(1000.0, 0.0, 400.0)) + DVec2::new(4.0, -3.0);
    h.with(|t, cx| t.mouse_move(cx, near));
    let s = h.tool.snap().unwrap().clone();
    assert_eq!(s.kind, SnapKind::Endpoint);
    assert!(s.pos.distance(DVec3::new(1000.0, 0.0, 400.0)) < 1e-6);
    let mid = h.px(DVec3::new(500.0, 0.0, 400.0)) + DVec2::new(3.0, 2.0);
    h.with(|t, cx| t.mouse_move(cx, mid));
    assert_eq!(h.tool.snap().unwrap().kind, SnapKind::Midpoint);
}

#[test]
fn line_typed_length_along_red_axis() {
    let mut h = Harness::new();
    h.tool(ToolId::Line);
    h.click(DVec3::ZERO);
    h.hover(DVec3::new(800.0, 0.0, 0.0));
    let s = h.tool.snap().unwrap().clone();
    assert_eq!(s.kind, SnapKind::Axis(0), "on the red axis");
    h.type_vcb("1250");
    let ents = h.doc.ents();
    assert_eq!(ents.edges.len(), 1);
    let (a, b) = ents.edge_points(ents.edges.keys().next().unwrap());
    assert!((a.distance(b) - 1250.0).abs() < 1e-6);
    assert!(b.y.abs() < 1e-6 && b.z.abs() < 1e-6);
}

#[test]
fn move_with_copy_and_array() {
    let mut h = Harness::new();
    h.rect_and_box();
    let all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    h.doc.select(all);
    h.tool(ToolId::Move);
    h.with(|t, cx| t.ctrl_tap(cx));
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    h.hover(DVec3::new(1800.0, 0.0, 0.0));
    h.type_vcb("1500");
    assert_eq!(h.faces(), 12);
    h.type_vcb("3x");
    assert_eq!(h.faces(), 24);
    let xmax = h.doc.ents().verts.values().map(|v| v.pos.x).fold(f64::MIN, f64::max);
    assert!((xmax - 5500.0).abs() < 1e-6, "xmax = {xmax}");
}

#[test]
fn select_double_click_and_erase_with_undo() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Select);
    let top = DVec3::new(500.0, 300.0, 400.0);
    h.click(top);
    assert!(matches!(h.doc.sel.as_slice(), [EntityRef::Face(_)]));
    let m = h.px(top);
    h.with(|t, cx| t.double_click(cx, m));
    assert_eq!(h.doc.sel.len(), 5, "face + 4 edges");

    h.tool(ToolId::Eraser);
    h.click(DVec3::new(500.0, 0.0, 400.0));
    assert_eq!(h.faces(), 4, "erasing a box edge removes its two faces");
    h.doc.undo();
    assert_eq!(h.faces(), 6);
    h.doc.redo();
    assert_eq!(h.faces(), 4);
}

#[test]
fn group_then_edit_inside() {
    let mut h = Harness::new();
    h.rect_and_box();
    let all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    let mut inst = None;
    h.doc.commit("Grouper", |m, d| inst = m.make_group(d, &all, true, None));
    let inst = inst.unwrap();
    h.tool(ToolId::Select);
    let m = h.px(DVec3::new(500.0, 300.0, 400.0));
    h.with(|t, cx| t.double_click(cx, m));
    assert_eq!(h.doc.ctx, vec![inst]);
    // Draw on the top face, inside the group.
    h.tool(ToolId::Rect);
    h.click(DVec3::new(200.0, 100.0, 400.0));
    h.hover(DVec3::new(400.0, 300.0, 400.0));
    h.type_vcb("300;200");
    assert_eq!(h.doc.ents().faces.len(), 7);
    assert_eq!(h.doc.model.root.faces.len(), 0, "nothing leaked outside the group");
    h.doc.exit();
    assert!(h.doc.ctx.is_empty());
}

#[test]
fn rotate_by_typed_angle() {
    let mut h = Harness::new();
    h.rect_and_box();
    let all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    h.doc.select(all);
    h.tool(ToolId::Rotate);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    h.hover(DVec3::new(0.0, 1000.0, 0.0));
    h.type_vcb("90");
    let ents = h.doc.ents();
    let xmin = ents.verts.values().map(|v| v.pos.x).fold(f64::MAX, f64::min);
    let ymax = ents.verts.values().map(|v| v.pos.y).fold(f64::MIN, f64::max);
    assert!((xmin + 600.0).abs() < 1e-6, "xmin = {xmin}");
    assert!((ymax - 1000.0).abs() < 1e-6, "ymax = {ymax}");
}

fn guide_list(h: &Harness) -> Vec<esq_core::model::Guide> {
    h.doc.ents().guides.values().cloned().collect()
}

#[test]
fn tape_from_an_edge_makes_a_parallel_guide() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Tape);
    // Bottom front edge (y = 0), pulled towards -y, then a typed distance.
    h.click(DVec3::new(300.0, 0.0, 0.0));
    h.hover(DVec3::new(300.0, -300.0, 0.0));
    h.type_vcb("250");
    let g = guide_list(&h);
    assert_eq!(g.len(), 1);
    assert!(g[0].infinite && g[0].dir.cross(DVec3::X).length() < 1e-9, "parallel to the edge: {:?}", g[0].dir);
    assert!((g[0].pos.y + 250.0).abs() < 1e-6 && g[0].pos.z.abs() < 1e-6, "pos = {:?}", g[0].pos);
    // Typing again moves the guide just made.
    h.type_vcb("400");
    let g = guide_list(&h);
    assert_eq!(g.len(), 1);
    assert!((g[0].pos.y + 400.0).abs() < 1e-6, "pos = {:?}", g[0].pos);
}

#[test]
fn guides_cross_and_snap_as_intersection() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Tape);
    h.click(DVec3::new(300.0, 0.0, 0.0));
    h.hover(DVec3::new(300.0, -300.0, 0.0));
    h.type_vcb("250");
    h.click(DVec3::new(1000.0, 200.0, 0.0));
    h.hover(DVec3::new(1300.0, 200.0, 0.0));
    h.type_vcb("100");
    assert_eq!(guide_list(&h).len(), 2);
    h.tool(ToolId::Line);
    let x = DVec3::new(1100.0, -250.0, 0.0);
    let m = h.px(x) + DVec2::new(3.0, -2.0);
    h.hover_px(m);
    let s = h.tool.snap().unwrap().clone();
    assert_eq!(s.kind, SnapKind::Intersection);
    assert!(s.pos.distance(x) < 1e-3, "pos = {:?}", s.pos);
    // Along one guide, away from the other one: "on the guide".
    let m = h.px(DVec3::new(1100.0, -900.0, 0.0));
    h.hover_px(m);
    assert_eq!(h.tool.snap().unwrap().kind, SnapKind::OnGuide);
}

#[test]
fn tape_from_a_point_makes_a_guide_point() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Tape);
    h.click(DVec3::new(1000.0, 0.0, 400.0));
    h.hover(DVec3::new(1000.0, 0.0, 700.0));
    h.type_vcb("100");
    let g = guide_list(&h);
    assert_eq!(g.len(), 1);
    assert!(g[0].is_point());
    assert!(g[0].pos.distance(DVec3::new(1000.0, 0.0, 500.0)) < 1e-6, "pos = {:?}", g[0].pos);
    h.tool(ToolId::Line);
    let m = h.px(DVec3::new(1000.0, 0.0, 500.0)) + DVec2::new(4.0, 3.0);
    h.hover_px(m);
    assert_eq!(h.tool.snap().unwrap().kind, SnapKind::GuidePoint);
}

#[test]
fn ctrl_turns_guide_creation_off() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Tape);
    // Ctrl cycles guide lines, guide points, measure (SketchUp 2026).
    h.with(|t, cx| t.ctrl_tap(cx));
    h.with(|t, cx| t.ctrl_tap(cx));
    h.click(DVec3::new(300.0, 0.0, 0.0));
    h.hover(DVec3::new(300.0, -300.0, 0.0));
    h.type_vcb("250");
    assert!(guide_list(&h).is_empty());
}

#[test]
fn guides_are_selected_moved_and_erased() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Tape);
    h.click(DVec3::new(300.0, 0.0, 0.0));
    h.hover(DVec3::new(300.0, -300.0, 0.0));
    h.type_vcb("250");
    h.tool(ToolId::Select);
    h.click(DVec3::new(2000.0, -250.0, 0.0));
    assert!(matches!(h.doc.sel.as_slice(), [EntityRef::Guide(_)]), "sel = {:?}", h.doc.sel);
    // Moved with the box.
    let mut all = h.doc.sel.clone();
    all.extend(h.doc.ents().faces.keys().map(EntityRef::Face));
    h.doc.select(all);
    h.tool(ToolId::Move);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    h.hover(DVec3::new(1000.0, 0.0, 300.0));
    h.type_vcb("100");
    assert!((guide_list(&h)[0].pos.z - 100.0).abs() < 1e-6);
    // Erased with the eraser.
    h.tool(ToolId::Eraser);
    h.click(DVec3::new(2500.0, -250.0, 100.0));
    assert!(guide_list(&h).is_empty());
    assert_eq!(h.faces(), 6, "the box is untouched");
}

#[test]
fn guides_follow_groups() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Tape);
    h.click(DVec3::new(300.0, 0.0, 0.0));
    h.hover(DVec3::new(300.0, -300.0, 0.0));
    h.type_vcb("250");
    let mut all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    all.extend(h.doc.ents().guides.keys().map(EntityRef::Guide));
    let mut inst = None;
    h.doc.commit("Grouper", |m, d| inst = m.make_group(d, &all, true, None));
    assert!(h.doc.model.root.guides.is_empty());
    let def = h.doc.model.root.instances[inst.unwrap()].def;
    assert_eq!(h.doc.model.defs[def].entities.guides.len(), 1);
    let mut out = Vec::new();
    h.doc.commit("Éclater", |m, d| out = m.explode(d, inst.unwrap()));
    let g = guide_list(&h);
    assert_eq!(g.len(), 1);
    assert!((g[0].pos.y + 250.0).abs() < 1e-6, "back in place: {:?}", g[0].pos);
}

#[test]
fn parallel_and_perpendicular_inferences() {
    let mut h = Harness::new();
    // A slanted edge teaches its direction when the cursor rests on it.
    let (a, b) = (DVec3::new(0.0, 0.0, 0.0), DVec3::new(1000.0, 500.0, 0.0));
    h.doc.commit("Ligne", |m, d| {
        m.entities_mut(d).add_line(a, b);
    });
    h.tool(ToolId::Line);
    h.hover(a.lerp(b, 0.3));
    assert_eq!(h.tool.snap().unwrap().kind, SnapKind::OnEdge);
    let start = DVec3::new(0.0, 1500.0, 0.0);
    h.click(start);
    let dir = (b - a).normalize();
    h.hover_px(h.px(start + dir * 800.0) + DVec2::new(2.0, 3.0));
    let s = h.tool.snap().unwrap().clone();
    assert_eq!(s.kind, SnapKind::Parallel);
    assert!((s.pos - start).normalize().cross(dir).length() < 1e-9);
    let perp = DVec3::new(-dir.y, dir.x, 0.0);
    h.hover_px(h.px(start + perp * 600.0) + DVec2::new(-2.0, 2.0));
    let s = h.tool.snap().unwrap().clone();
    assert_eq!(s.kind, SnapKind::Perpendicular);
    assert!((s.pos - start).normalize().dot(dir).abs() < 1e-9);
}

#[test]
fn follow_me_tool_with_a_preselected_path() {
    let mut h = Harness::new();
    h.tool(ToolId::Rect);
    h.click(DVec3::ZERO);
    h.hover(DVec3::new(900.0, 500.0, 0.0));
    h.type_vcb("1000;600");
    let base = h.doc.ents().faces.keys().next().unwrap();
    // A small upright profile at the front left corner.
    h.doc.commit("Profil", |m, d| {
        let pts = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(0.0, -50.0, 0.0),
            DVec3::new(0.0, -50.0, 80.0),
            DVec3::new(0.0, 0.0, 80.0),
        ];
        m.entities_mut(d).add_polyline(&pts, true, false);
    });
    assert_eq!(h.faces(), 2);
    h.doc.select(vec![EntityRef::Face(base)]);
    h.tool(ToolId::FollowMe);
    h.click(DVec3::new(0.0, -25.0, 40.0));
    assert_eq!(h.faces(), 1 + 16, "base + 4 sides × 4 legs, the profile is used up");
    let zmax = h.zmax();
    assert!((zmax - 80.0).abs() < 1e-6);
    h.doc.undo();
    assert_eq!(h.faces(), 2);
}

#[test]
fn follow_me_tool_profile_then_edge() {
    let mut h = Harness::new();
    h.doc.commit("Chemin et profil", |m, d| {
        let e = m.entities_mut(d);
        e.add_polyline(
            &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, 800.0, 0.0)],
            false,
            false,
        );
        let prof = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(0.0, -60.0, 0.0),
            DVec3::new(0.0, -60.0, 60.0),
            DVec3::new(0.0, 0.0, 60.0),
        ];
        e.add_polyline(&prof, true, false);
    });
    assert_eq!(h.faces(), 1);
    h.tool(ToolId::FollowMe);
    h.click(DVec3::new(0.0, -30.0, 30.0));
    assert!(h.tool.busy(), "waiting for the path");
    h.click(DVec3::new(600.0, 0.0, 0.0));
    assert_eq!(h.faces(), 2 + 4 * 2);
}

fn edge_count(h: &Harness) -> usize {
    h.doc.ents().edges.len()
}

#[test]
fn arc_from_centre_and_pie() {
    let mut h = Harness::new();
    h.tool(ToolId::ArcCenter);
    h.click(DVec3::ZERO);
    h.hover(DVec3::new(800.0, 0.0, 0.0));
    h.type_vcb("500");
    h.hover(DVec3::new(0.0, 700.0, 0.0));
    h.type_vcb("90");
    assert_eq!(edge_count(&h), 12, "12 segments by default");
    let ents = h.doc.ents();
    let r: Vec<f64> = ents.verts.values().map(|v| v.pos.length()).collect();
    assert!(r.iter().all(|x| (x - 500.0).abs() < 1e-6), "{r:?}");
    let ymax = ents.verts.values().map(|v| v.pos.y).fold(f64::MIN, f64::max);
    assert!((ymax - 500.0).abs() < 1e-6, "quarter turn towards +y: {ymax}");

    let mut h = Harness::new();
    h.tool(ToolId::Pie);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(600.0, 0.0, 0.0));
    h.hover(DVec3::new(0.0, 600.0, 0.0));
    h.type_vcb("90");
    assert_eq!(h.faces(), 1, "a pie is a closed face");
    let f = h.doc.ents().faces.keys().next().unwrap();
    let area = face_area(h.doc.ents(), f);
    let quarter = std::f64::consts::PI * 600.0 * 600.0 / 4.0;
    assert!((area - quarter).abs() / quarter < 0.02, "area {area} vs {quarter}");
}

#[test]
fn arc_through_three_points() {
    let mut h = Harness::new();
    h.tool(ToolId::Arc3);
    h.click(DVec3::new(0.0, 0.0, 0.0));
    h.click(DVec3::new(500.0, 500.0, 0.0));
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    let ents = h.doc.ents();
    assert_eq!(ents.edges.len(), 12);
    // Half circle of radius 500 centred on (500, 0).
    for v in ents.verts.values() {
        assert!((v.pos.distance(DVec3::new(500.0, 0.0, 0.0)) - 500.0).abs() < 1e-6, "{:?}", v.pos);
        assert!(v.pos.y > -1e-6);
    }
}

#[test]
fn rotated_rectangle() {
    let mut h = Harness::new();
    h.tool(ToolId::RotRect);
    h.click(DVec3::ZERO);
    h.hover(DVec3::new(600.0, 600.0, 0.0));
    h.type_vcb("1000");
    h.hover(DVec3::new(0.0, 900.0, 0.0));
    h.type_vcb("400");
    assert_eq!(h.faces(), 1);
    let f = h.doc.ents().faces.keys().next().unwrap();
    assert!((face_area(h.doc.ents(), f) - 400_000.0).abs() < 1e-3);
}

#[test]
fn freehand_loop_makes_a_face() {
    let mut h = Harness::new();
    h.tool(ToolId::Freehand);
    let pts: Vec<DVec3> = (0..=40)
        .map(|i| {
            let a = i as f64 / 40.0 * std::f64::consts::TAU;
            DVec3::new(500.0 + 400.0 * a.cos(), 300.0 + 300.0 * a.sin(), 0.0)
        })
        .collect();
    let first = h.px(pts[0]);
    h.with(|t, cx| t.press(cx, first));
    for p in &pts[1..] {
        let m = h.px(*p);
        h.with(|t, cx| t.mouse_move(cx, m));
    }
    let last = h.px(pts[40]);
    h.with(|t, cx| t.release(cx, last));
    assert_eq!(h.faces(), 1, "closing the loop makes a face");
    assert!(h.doc.ents().edges.values().all(|e| e.curve.is_some()), "one curve");
}

#[test]
fn flip_mirrors_and_keeps_faces_outward() {
    let mut h = Harness::new();
    h.rect_and_box();
    let all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    h.doc.select(all);
    h.tool(ToolId::Flip);
    // The red plane (x = 500) stands across the box: click it in front of the box.
    h.click(DVec3::new(500.0, -350.0, 150.0));
    let ents = h.doc.ents();
    let c = DVec3::new(500.0, 300.0, 200.0);
    for f in ents.faces.values() {
        let p = ents.pos(f.outer[0]);
        assert!(f.normal.dot(p - c) > 0.0, "normal points inwards after flipping");
    }
}

#[test]
fn protractor_makes_an_angled_guide() {
    let mut h = Harness::new();
    h.tool(ToolId::Protractor);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    h.hover(DVec3::new(700.0, 700.0, 0.0));
    h.type_vcb("30");
    let g = h.doc.ents().guides.values().next().expect("a guide").clone();
    let a = g.dir.y.atan2(g.dir.x).to_degrees();
    assert!((a - 30.0).abs() < 1e-6, "angle {a}");
}

const ALT: Modifiers = Modifiers { alt: true, ..Modifiers::NONE };
const SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::NONE };
const CTRL: Modifiers = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ctrl: true, command: true, ..Modifiers::NONE };

fn material(h: &mut Harness, name: &str) -> esq_core::model::MaterialId {
    h.doc.model.materials.insert(esq_core::model::Material {
        name: name.into(),
        color: [200, 30, 30],
        opacity: 1.0,
        ext: Default::default(),
    })
}

fn painted(h: &Harness, m: esq_core::model::MaterialId) -> usize {
    h.doc.ents().faces.values().filter(|f| f.front == Some(m)).count()
}

#[test]
fn paint_bucket_modifiers_fill_matching_faces() {
    let mut h = Harness::new();
    h.rect_and_box();
    assert_eq!(h.faces(), 6);
    h.tool(ToolId::Paint);
    let (a, b, c) = (material(&mut h, "A"), material(&mut h, "B"), material(&mut h, "C"));
    // Plain click: one face.
    h.material = Some(a);
    h.click(DVec3::new(500.0, 300.0, 400.0));
    assert_eq!(painted(&h, a), 1);
    // Ctrl: the connected faces that still have the default material, i.e. all but the top.
    h.mods = CTRL;
    h.material = Some(b);
    h.click(DVec3::new(0.0, 300.0, 200.0));
    assert_eq!(painted(&h, b), 5);
    assert_eq!(painted(&h, a), 1, "the top keeps its own material");
    // Shift: every face with the clicked face's material, wherever it is.
    h.mods = SHIFT;
    h.material = Some(c);
    h.click(DVec3::new(500.0, 300.0, 400.0));
    assert_eq!((painted(&h, a), painted(&h, c)), (0, 1));
    // Ctrl+Shift: those of the same object.
    h.mods = CTRL_SHIFT;
    h.material = Some(a);
    h.click(DVec3::new(0.0, 300.0, 200.0));
    assert_eq!((painted(&h, a), painted(&h, b)), (5, 0));
}

#[test]
fn eraser_modifiers_hide_soften_and_restore() {
    let mut h = Harness::new();
    h.tool(ToolId::Line);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    assert_eq!(h.doc.ents().edges.len(), 1);
    let edge = |h: &Harness| h.doc.ents().edges.values().next().cloned().unwrap();
    let mid = DVec3::new(500.0, 0.0, 0.0);
    h.tool(ToolId::Eraser);

    h.mods = SHIFT;
    h.click(mid);
    assert!(edge(&h).hidden, "Shift hides");

    // Hidden geometry shown, so the edge can be picked again.
    h.style.hidden_geometry = true;
    h.pick_rev = u64::MAX;
    h.mods = CTRL_SHIFT;
    h.click(mid);
    assert!(!edge(&h).hidden && !edge(&h).soft, "Ctrl+Shift restores");

    h.mods = CTRL;
    h.click(mid);
    let e = edge(&h);
    assert!(e.soft && e.smooth, "Ctrl softens and smooths");

    h.mods = Modifiers::NONE;
    h.click(mid);
    assert!(h.doc.ents().edges.is_empty(), "no modifier erases");
}

#[test]
fn offset_of_selected_edges_makes_a_parallel_chain_on_the_cursor_side() {
    let mut h = Harness::new();
    h.tool(ToolId::Line);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    h.click(DVec3::new(1000.0, 600.0, 0.0));
    h.with(|t, cx| t.cancel(cx));
    let edges: Vec<EntityRef> = h.doc.ents().edges.keys().map(EntityRef::Edge).collect();
    assert_eq!(edges.len(), 2);
    h.doc.select(edges);

    h.tool(ToolId::Offset);
    h.click(DVec3::new(500.0, 0.0, 0.0)); // the point the offset is measured from
    h.click(DVec3::new(500.0, -100.0, 0.0)); // 100 to the outside
    let has = |h: &Harness, p: DVec3| h.doc.ents().verts.values().any(|v| v.pos.distance(p) < 1e-6);
    assert!(has(&h, DVec3::new(0.0, -100.0, 0.0)), "start of the copy");
    assert!(has(&h, DVec3::new(1100.0, -100.0, 0.0)), "mitred corner");
    assert!(has(&h, DVec3::new(1100.0, 600.0, 0.0)), "end of the copy");
    assert_eq!(h.faces(), 1, "the band between the chain and its copy");

    // Typing a distance keeps the side the mouse chose.
    let mut h = Harness::new();
    h.tool(ToolId::Line);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    h.click(DVec3::new(1000.0, 600.0, 0.0));
    h.with(|t, cx| t.cancel(cx));
    let edges: Vec<EntityRef> = h.doc.ents().edges.keys().map(EntityRef::Edge).collect();
    h.doc.select(edges);
    h.tool(ToolId::Offset);
    h.click(DVec3::new(500.0, 0.0, 0.0));
    h.hover(DVec3::new(500.0, -30.0, 0.0));
    h.type_vcb("250");
    assert!(has(&h, DVec3::new(0.0, -250.0, 0.0)), "typed 250 on the outside");
}

#[test]
fn glued_text_lies_on_the_face_under_the_cursor() {
    let mut h = Harness::new();
    h.rect_and_box();
    // A one-edge definition: enough to place.
    let mut e = esq_core::model::Entities::default();
    let (a, b) = (e.add_vertex_raw(DVec3::ZERO), e.add_vertex_raw(DVec3::new(100.0, 0.0, 0.0)));
    e.add_edge_raw(a, b);
    let def = h.doc.model.defs.insert(esq_core::model::Definition {
        name: "t".into(),
        is_group: false,
        description: String::new(),
        attributes: Default::default(),
        ext: Default::default(),
        entities: e,
    });
    // On a vertical wall: Z out of the wall, X horizontal along it, Y up.
    h.tool = Box::new(crate::tools::PlaceTool::glued(def));
    h.click(DVec3::new(0.0, 300.0, 200.0));
    let inst = h.doc.ents().instances.values().next().expect("placed").xform;
    let (x, y, z) = (inst.x_axis.truncate(), inst.y_axis.truncate(), inst.z_axis.truncate());
    assert!(z.z.abs() < 1e-9 && (z.length() - 1.0).abs() < 1e-9, "Z along a wall normal, {z:?}");
    assert!((x - DVec3::Z.cross(z)).length() < 1e-9, "X is horizontal along the wall, {x:?}");
    assert!((y - DVec3::Z).length() < 1e-9, "Y points up, {y:?}");

    // The plain placement tool does not rotate.
    let mut h2 = Harness::new();
    h2.rect_and_box();
    let def2 = h2.doc.model.defs.insert(h.doc.model.defs[def].clone());
    h2.tool = Box::new(crate::tools::PlaceTool::new(def2));
    h2.click(DVec3::new(0.0, 300.0, 200.0));
    let m = h2.doc.ents().instances.values().next().unwrap().xform;
    assert!((m.z_axis.truncate() - DVec3::Z).length() < 1e-9);
}

#[test]
fn push_pull_with_alt_stretches_instead_of_extruding() {
    // An isolated face: normally Push/Pull extrudes a solid; with Alt it just moves.
    let mut h = Harness::new();
    h.tool(ToolId::Rect);
    h.click(DVec3::ZERO);
    h.hover(DVec3::new(900.0, 500.0, 0.0));
    h.type_vcb("1000;600");
    h.tool(ToolId::PushPull);
    h.mods = ALT;
    h.click(DVec3::new(500.0, 300.0, 0.0));
    h.hover(DVec3::new(500.0, 300.0, 250.0));
    h.type_vcb("400");
    assert_eq!(h.faces(), 1, "no side faces: nothing was extruded");
    assert!((h.zmax() - 400.0).abs() < 1e-6, "the face moved up by 400, z = {}", h.zmax());
}

/// A 1000 x 600 x 400 box selected, with the Scale tool ready.
fn scale_setup() -> Harness {
    let mut h = Harness::new();
    h.rect_and_box();
    let all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    h.doc.select(all);
    h.tool(ToolId::Scale);
    h
}

/// Grip at `from`, dragged to `to`.
fn scale_drag(h: &mut Harness, from: DVec3, to: DVec3) {
    h.click(from);
    h.click(to);
}

fn size(h: &Harness) -> DVec3 {
    let (lo, hi) = extent_xy(h);
    hi - lo
}

#[test]
fn scale_corner_grip_is_uniform() {
    let mut h = scale_setup();
    scale_drag(&mut h, DVec3::new(1000.0, 600.0, 400.0), DVec3::new(2000.0, 1200.0, 800.0));
    let s = size(&h);
    assert!((s - DVec3::new(2000.0, 1200.0, 800.0)).length() < 1e-6, "{s:?}");
}

#[test]
fn scale_face_grip_scales_one_axis_and_shift_makes_it_uniform() {
    let mut h = scale_setup();
    scale_drag(&mut h, DVec3::new(1000.0, 300.0, 200.0), DVec3::new(1500.0, 300.0, 200.0));
    let s = size(&h);
    assert!((s - DVec3::new(1500.0, 600.0, 400.0)).length() < 1e-6, "face grip: x only, {s:?}");

    let mut h = scale_setup();
    h.click(DVec3::new(1000.0, 300.0, 200.0));
    h.mods = SHIFT;
    h.click(DVec3::new(1500.0, 300.0, 200.0));
    let s = size(&h);
    assert!((s - DVec3::new(1500.0, 900.0, 600.0)).length() < 1e-6, "Shift: uniform, {s:?}");
}

#[test]
fn scale_edge_grip_scales_two_axes_freely() {
    let mut h = scale_setup();
    scale_drag(&mut h, DVec3::new(1000.0, 600.0, 200.0), DVec3::new(1500.0, 1200.0, 200.0));
    let s = size(&h);
    assert!((s - DVec3::new(1500.0, 1200.0, 400.0)).length() < 1e-6, "{s:?}");
}

#[test]
fn scale_about_the_centre_with_ctrl() {
    let mut h = scale_setup();
    h.with(|t, cx| t.ctrl_tap(cx));
    scale_drag(&mut h, DVec3::new(1000.0, 300.0, 200.0), DVec3::new(1500.0, 300.0, 200.0));
    let (lo, hi) = extent_xy(&h);
    assert!((lo.x + 500.0).abs() < 1e-6 && (hi.x - 1500.0).abs() < 1e-6, "{lo:?} {hi:?}");
}

#[test]
fn scale_typed_factors_per_axis() {
    let mut h = scale_setup();
    h.click(DVec3::new(1000.0, 600.0, 400.0));
    h.type_vcb("2;1;0,5");
    let s = size(&h);
    assert!((s - DVec3::new(2000.0, 600.0, 200.0)).length() < 1e-6, "{s:?}");

    // A single value goes with the grip's mode; a length is turned into a factor.
    let mut h = scale_setup();
    h.click(DVec3::new(1000.0, 300.0, 200.0));
    h.type_vcb("2000mm");
    let s = size(&h);
    assert!((s - DVec3::new(2000.0, 600.0, 400.0)).length() < 1e-6, "length along x: {s:?}");
}

fn extent_xy(h: &Harness) -> (DVec3, DVec3) {
    let v = verts_around(h);
    (v.iter().fold(DVec3::splat(f64::MAX), |a, p| a.min(*p)), v.iter().fold(DVec3::splat(f64::MIN), |a, p| a.max(*p)))
}

#[test]
fn rectangle_snaps_to_a_square_within_six_pixels() {
    let mut h = Harness::new();
    h.tool(ToolId::Rect);
    h.click(DVec3::ZERO);
    let (near, square) = (DVec3::new(1000.0, 1004.0, 0.0), DVec3::new(1000.0, 1000.0, 0.0));
    assert!(h.px(near).distance(h.px(square)) < 6.0, "the test point is within 6 px");
    h.click(near);
    let (lo, hi) = extent_xy(&h);
    assert!((hi.x - lo.x - 1000.0).abs() < 1e-6 && (hi.y - lo.y - 1000.0).abs() < 1e-6, "{lo:?} {hi:?}");
}

#[test]
fn rectangle_snaps_to_the_golden_section() {
    let mut h = Harness::new();
    h.tool(ToolId::Rect);
    h.click(DVec3::ZERO);
    let phi = (5f64.sqrt() + 1.0) / 2.0;
    let near = DVec3::new(1000.0 * phi + 3.0, 1000.0, 0.0);
    h.click(near);
    let (lo, hi) = extent_xy(&h);
    assert!((hi.x - lo.x - 1000.0 * phi).abs() < 1e-6, "long side is phi x the short one: {}", hi.x - lo.x);
    assert!((hi.y - lo.y - 1000.0).abs() < 1e-6);
}

#[test]
fn rectangle_far_from_the_inferences_stays_free() {
    let mut h = Harness::new();
    h.tool(ToolId::Rect);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1300.0, 1000.0, 0.0));
    let (lo, hi) = extent_xy(&h);
    assert!((hi.x - lo.x - 1300.0).abs() < 1e-6, "{}", hi.x - lo.x);
}

#[test]
fn rectangle_from_the_centre() {
    let mut h = Harness::new();
    h.tool(ToolId::Rect);
    h.with(|t, cx| t.ctrl_tap(cx));
    h.click(DVec3::ZERO);
    h.click(DVec3::new(500.0, 200.0, 0.0));
    let (lo, hi) = extent_xy(&h);
    assert!((lo.x + 500.0).abs() < 1e-6 && (hi.x - 500.0).abs() < 1e-6, "{lo:?} {hi:?}");
    assert!((lo.y + 200.0).abs() < 1e-6 && (hi.y - 200.0).abs() < 1e-6);
}

fn verts_around(h: &Harness) -> Vec<DVec3> {
    h.doc.ents().verts.values().map(|v| v.pos).collect()
}

#[test]
fn polygon_is_inscribed_until_ctrl_makes_it_circumscribed() {
    // Inscribed: the vertices lie on the circle through the cursor.
    let mut h = Harness::new();
    h.tool(ToolId::Polygon);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    let v = verts_around(&h);
    assert_eq!(v.len(), 6);
    assert!(v.iter().all(|p| (p.length() - 1000.0).abs() < 1e-6), "all on the circle");
    assert!(v.iter().any(|p| p.distance(DVec3::new(1000.0, 0.0, 0.0)) < 1e-6), "first vertex at the cursor");

    // Circumscribed: the cursor marks the middle of an edge (r = apothem).
    let mut h = Harness::new();
    h.tool(ToolId::Polygon);
    h.click(DVec3::ZERO);
    h.with(|t, cx| t.ctrl_tap(cx));
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    let v = verts_around(&h);
    let r = 1000.0 / 30f64.to_radians().cos();
    assert_eq!(v.len(), 6);
    assert!(v.iter().all(|p| (p.length() - r).abs() < 1e-6), "vertices at r / cos(pi/n)");
    let right: Vec<&DVec3> = v.iter().filter(|p| p.x > 1.0).collect();
    assert_eq!(right.len(), 2, "one edge faces the cursor");
    assert!(right.iter().all(|p| (p.x - 1000.0).abs() < 1e-6), "that edge is tangent to the circle at the cursor");
}

#[test]
fn ctrl_plus_and_minus_change_the_number_of_sides() {
    let mut h = Harness::new();
    h.tool(ToolId::Circle);
    h.with(|t, cx| t.step(cx, 1));
    h.with(|t, cx| t.step(cx, 1));
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    assert_eq!(verts_around(&h).len(), 26);

    let mut h = Harness::new();
    h.tool(ToolId::Polygon);
    for _ in 0..10 {
        h.with(|t, cx| t.step(cx, -1));
    }
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    assert_eq!(verts_around(&h).len(), 3, "never under 3 sides");
}

#[test]
fn true_north_follows_the_second_click() {
    let mut h = Harness::new();
    h.tool(ToolId::North);
    h.click(DVec3::ZERO);
    // Within the 0.001" box nothing changes; the second click sets the angle.
    h.hover(DVec3::new(0.01, 0.01, 0.0));
    assert!(h.doc.preview.is_none());
    h.click(DVec3::new(1000.0, 1000.0, 0.0));
    assert!((h.doc.model.shadow.north_angle - 45.0).abs() < 1e-9, "{}", h.doc.model.shadow.north_angle);
    // Clockwise from the green axis: pointing along -red gives -90.
    h.click(DVec3::ZERO);
    h.click(DVec3::new(-1000.0, 0.0, 0.0));
    assert!((h.doc.model.shadow.north_angle + 90.0).abs() < 1e-9, "{}", h.doc.model.shadow.north_angle);
    h.doc.undo();
    assert!((h.doc.model.shadow.north_angle - 45.0).abs() < 1e-9);
}

#[test]
fn lasso_selects_what_it_surrounds() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Lasso);
    // A clockwise loop (on screen) around the whole box.
    let c = h.px(DVec3::new(500.0, 300.0, 200.0));
    let pts: Vec<DVec2> = (0..=36)
        .map(|i| {
            let a = i as f64 / 36.0 * std::f64::consts::TAU;
            c + DVec2::new(a.cos(), a.sin()) * 400.0
        })
        .collect();
    h.with(|t, cx| t.press(cx, pts[0]));
    for p in &pts[1..] {
        h.with(|t, cx| t.mouse_move(cx, *p));
    }
    h.with(|t, cx| t.release(cx, pts[36]));
    assert_eq!(h.doc.sel.len(), 6 + 12, "6 faces and 12 edges");
}

#[test]
fn axes_tool_moves_the_ground() {
    let mut h = Harness::new();
    h.tool(ToolId::AxesTool);
    h.click(DVec3::new(0.0, 0.0, 0.0));
    h.click(DVec3::new(1000.0, 1000.0, 0.0));
    h.click(DVec3::new(-1000.0, 1000.0, 0.0));
    let x = h.doc.model.axes.col(0).truncate();
    assert!((x - DVec3::new(1.0, 1.0, 0.0).normalize()).length() < 1e-9, "{x:?}");
    // A rectangle now lines up with the new red axis.
    h.tool(ToolId::Rect);
    h.click(DVec3::ZERO);
    h.hover(DVec3::new(0.0, 1500.0, 0.0));
    h.type_vcb("1000;500");
    let ents = h.doc.ents();
    let has = |p: DVec3| ents.verts.values().any(|v| v.pos.distance(p) < 1e-6);
    let d = DVec3::new(1.0, 1.0, 0.0).normalize() * 1000.0;
    assert!(has(d), "corner along the new red axis");
}

#[test]
fn camera_tools() {
    let mut h = Harness::new();
    h.tool(ToolId::Fov);
    h.type_vcb("50mm");
    let fov = h.cam.fov_y.to_degrees();
    assert!((fov - 27.0).abs() < 0.1, "{fov}");
    h.type_vcb("60");
    assert!((h.cam.fov_y.to_degrees() - 60.0).abs() < 1e-9);

    h.tool(ToolId::PositionCamera);
    h.click(DVec3::new(2000.0, -3000.0, 0.0));
    let eye = h.cam.eye();
    assert!((eye.z - 1676.4).abs() < 1e-6 && (eye.x - 2000.0).abs() < 1e-6, "{eye:?}");
    h.tool(ToolId::LookAround);
    let m = DVec2::new(640.0, 400.0);
    h.with(|t, cx| t.press(cx, m));
    h.with(|t, cx| t.mouse_move(cx, m + DVec2::new(80.0, 30.0)));
    h.with(|t, cx| t.release(cx, m));
    assert!(h.cam.eye().distance(eye) < 1e-6, "the eye stays put");
    h.type_vcb("1200");
    assert!((h.cam.eye().z - 1200.0).abs() < 1e-6);
}

fn annots(h: &Harness) -> Vec<esq_core::model::Annotation> {
    h.doc.ents().annots.values().cloned().collect()
}

#[test]
fn dimension_between_two_points_and_on_an_edge() {
    use esq_core::model::AnnotKind;
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Dimension);
    h.click(DVec3::new(0.0, 0.0, 400.0));
    h.click(DVec3::new(1000.0, 0.0, 400.0));
    h.click(DVec3::new(500.0, -300.0, 700.0));
    let a = annots(&h);
    assert_eq!(a.len(), 1);
    let AnnotKind::Linear { a: p, b: q, offset } = a[0].kind else { panic!("{:?}", a[0].kind) };
    assert!((p.distance(q) - 1000.0).abs() < 1e-6);
    assert!(offset.length() > 100.0 && offset.dot(q - p).abs() < 1e-6, "offset {offset:?}");
    assert_eq!(crate::annot::label(&a[0], esq_core::model::Units::Millimeters), "1000 mm");
    // Clicking on its label selects it.
    h.tool(ToolId::Select);
    let label = h.px((p + q) * 0.5 + offset) - DVec2::new(0.0, 10.0);
    h.with(|t, cx| {
        t.mouse_move(cx, label);
        t.press(cx, label);
        t.release(cx, label);
    });
    assert!(matches!(h.doc.sel.as_slice(), [EntityRef::Annot(_)]), "{:?}", h.doc.sel);

    // Clicking an edge dimensions the whole edge.
    h.tool(ToolId::Dimension);
    h.click(DVec3::new(1000.0, 200.0, 400.0));
    h.click(DVec3::new(1300.0, 200.0, 600.0));
    let a = annots(&h);
    assert_eq!(a.len(), 2);
    assert!(a.iter().any(|x| crate::annot::label(x, esq_core::model::Units::Millimeters) == "600 mm"));
}

#[test]
fn text_on_a_face_shows_its_area() {
    use esq_core::model::AnnotKind;
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Text);
    h.click(DVec3::new(400.0, 300.0, 400.0));
    h.click(DVec3::new(400.0, 300.0, 900.0));
    let a = annots(&h);
    assert_eq!(a.len(), 1);
    assert!(matches!(a[0].kind, AnnotKind::Text { .. }));
    // In the area unit and precision of Model Info › Units (m², 0,00 in the template).
    assert_eq!(a[0].text, "0,60 m²");
    // Erased with the eraser.
    let AnnotKind::Text { point, leader } = a[0].kind else { unreachable!() };
    h.tool(ToolId::Eraser);
    let m = h.px(point + leader);
    h.with(|t, cx| {
        t.mouse_move(cx, m);
        t.press(cx, m);
        t.release(cx, m);
    });
    assert!(annots(&h).is_empty());
}

#[test]
fn section_plane_placed_on_a_face_cuts_the_box() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::SectionPlane);
    // The front face looks at -Y: the plane takes its place, arrows pointing into the box.
    h.click(DVec3::new(500.0, 0.0, 200.0));
    let ents = h.doc.ents();
    let s = ents.active_section.expect("new plane is active");
    let sp = &ents.sections[s];
    assert!(sp.normal.distance(DVec3::Y) < 1e-9 && sp.pos.y.abs() < 1e-6, "{sp:?}");
    assert_eq!(sp.symbol, "1");
    assert_eq!(h.doc.sel, vec![EntityRef::Section(s)]);
    // Move it 300 mm into the box: the front half is cut away.
    h.doc.commit("Déplacer", |m, d| {
        m.entities_mut(d).transform_selection(
            &[EntityRef::Section(s)],
            &esq_core::glam::DMat4::from_translation(DVec3::new(0.0, 300.0, 0.0)),
        )
    });
    let style = crate::style::ViewStyle::default();
    let opts = crate::scene::SceneOptions {
        ctx: &[],
        ctx_def: None,
        sel: &[],
        axes_xform: esq_core::glam::DMat4::IDENTITY,
        style: &style,
    };
    let mesh = crate::scene::build(&h.doc.model, &opts);
    assert_eq!(mesh.clips.len(), 1);
    let cuts = mesh.lines.iter().filter(|l| l.width == style.section_cut_width as f32).count();
    assert_eq!(cuts, 4, "the cut is a rectangle");
    // Clicking where the front face was now goes through it, into the box.
    h.tool(ToolId::Select);
    h.click(DVec3::new(500.0, 0.0, 200.0));
    let [EntityRef::Face(f)] = h.doc.sel.as_slice() else { panic!("{:?}", h.doc.sel) };
    assert!(h.doc.ents().faces[*f].normal.y > -0.99, "the front face is cut away");
    // Double-clicking the plane's outline turns the cut off.
    let sp = h.doc.ents().sections[s].clone();
    let r = crate::scene::section_rect(&h.doc.model, &h.doc.model.root, &esq_core::glam::DMat4::IDENTITY, &sp);
    let m = h.px((r[0] + r[1]) * 0.5);
    h.with(|t, cx| t.double_click(cx, m));
    assert!(h.doc.ents().active_section.is_none());
    assert_eq!(h.doc.sel, vec![EntityRef::Section(s)]);
}

#[test]
fn shader_is_valid_wgsl() {
    let module = naga::front::wgsl::parse_str(include_str!("shader.wgsl")).unwrap_or_else(|e| panic!("{e}"));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}"));
}

#[test]
fn union_tool_clicks_two_solids() {
    let mut h = Harness::new();
    h.rect_and_box();
    let items: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    h.doc.commit("Groupe", |m, d| {
        let g = m.make_group(d, &items, true, None).unwrap();
        let mut copy = m.root.instances[g].clone();
        copy.xform = esq_core::glam::DMat4::from_translation(DVec3::new(500.0, 300.0, 0.0)) * copy.xform;
        let def = m.defs[copy.def].clone();
        copy.def = m.defs.insert(def);
        m.root.instances.insert(copy);
    });
    h.tool(ToolId::SolidUnion);
    // Top of the first box where the second does not cover it, then top of the second.
    h.click(DVec3::new(100.0, 100.0, 400.0));
    assert_eq!(h.doc.sel.len(), 1, "first solid picked");
    h.click(DVec3::new(1400.0, 800.0, 400.0));
    assert_eq!(h.doc.ents().instances.len(), 1);
    let [EntityRef::Instance(u)] = h.doc.sel.as_slice() else { panic!("{:?}", h.doc.sel) };
    let v = h.doc.model.solid_volume(None, *u).unwrap();
    // 2 × 1000×600×400 minus the 500×300×400 overlap.
    assert!((v - (2.0 * 2.4e8 - 6.0e7)).abs() < 1.0, "{v}");
    h.doc.undo();
    assert_eq!(h.doc.ents().instances.len(), 2, "one undo step");
}

mod sandbox_tests;

#[test]
fn section_slice_of_the_box_exports_to_svg_and_dxf() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::SectionPlane);
    h.click(DVec3::new(500.0, 0.0, 200.0));
    let s = h.doc.ents().active_section.expect("active plane");
    h.doc.commit("Déplacer", |m, d| {
        m.entities_mut(d).transform_selection(
            &[EntityRef::Section(s)],
            &esq_core::glam::DMat4::from_translation(DVec3::new(0.0, 300.0, 0.0)),
        )
    });
    let d = crate::export_section::drawing(&h.doc.model, h.doc.ctx_def(), s).expect("the plane cuts the box");
    let (lo, hi) = d.bounds().unwrap();
    assert!(((hi.x - lo.x) - 1000.0).abs() < 1e-6 && ((hi.y - lo.y) - 400.0).abs() < 1e-6, "{lo} {hi}");
    assert_eq!(d.loops.len(), 1);
    let dir = std::env::temp_dir().join("esq_slice_test");
    std::fs::create_dir_all(&dir).unwrap();
    let o = esq_core::slice_export::SliceOptions { model_mm: 10.0, ..Default::default() };
    let svg = dir.join("t.svg");
    esq_core::slice_export::export_slice(&d, &o, 0.35, &svg).unwrap();
    assert!(std::fs::read_to_string(&svg).unwrap().contains("width=\"104mm\" height=\"44mm\""));
    let dxf = dir.join("t.dxf");
    esq_core::slice_export::export_slice(&d, &o, 0.35, &dxf).unwrap();
    assert!(std::fs::read_to_string(&dxf).unwrap().contains("LINE"));
    assert!(esq_core::slice_export::export_slice(&d, &o, 0.35, &dir.join("t.png")).is_err());
    // A plane that misses the model has nothing to export.
    h.doc.commit("Déplacer", |m, d| {
        m.entities_mut(d).transform_selection(
            &[EntityRef::Section(s)],
            &esq_core::glam::DMat4::from_translation(DVec3::new(0.0, 5000.0, 0.0)),
        )
    });
    assert!(crate::export_section::drawing(&h.doc.model, h.doc.ctx_def(), s).is_err());
}

#[test]
fn component_attributes_are_undoable_and_follow_the_selection() {
    use crate::attrs::{OPTIONS_DICT, Scope, options_of, read_scope, selected_instance, set_option, write_scope};
    use esq_core::model::{AttrValue, Attributes, set_attr};
    let mut h = Harness::new();
    h.rect_and_box();
    let items: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    let mut made = None;
    h.doc.commit("Composant", |m, d| made = m.make_group(d, &items, false, Some("Caisson".into())));
    let inst = made.expect("component made");
    // Nothing to edit until a component is selected on its own.
    h.doc.select(Vec::new());
    assert!(!h.doc.ents().instances.is_empty());
    h.tool(ToolId::Select);
    h.click(DVec3::new(500.0, 300.0, 400.0));
    assert_eq!(h.doc.sel, vec![EntityRef::Instance(inst)]);
    let mut a = Attributes::new();
    set_attr(&mut a, "fiche", "ref", AttrValue::Text("C-12".into()));
    h.doc.commit("Attributs du composant", |m, d| write_scope(m, d, inst, Scope::Instance, a.clone()));
    set_attr(&mut a, OPTIONS_DICT, "couleur", AttrValue::Text("rouge".into()));
    h.doc.commit("Attributs du composant", |m, d| write_scope(m, d, inst, Scope::Definition, a.clone()));
    h.doc.commit("Options du composant", |m, d| set_option(m, d, inst, "couleur", AttrValue::Text("bleu".into())));
    let ctx = h.doc.ctx_def();
    let opts = options_of(&h.doc.model, ctx, inst);
    assert_eq!(opts, vec![("couleur".to_string(), AttrValue::Text("bleu".into()))]);
    // Undo walks back through each edit, redo brings them back.
    h.doc.undo();
    assert_eq!(options_of(&h.doc.model, ctx, inst)[0].1, AttrValue::Text("rouge".into()));
    h.doc.undo();
    assert!(read_scope(&h.doc.model, ctx, inst, Scope::Definition).unwrap().is_empty());
    assert_eq!(
        read_scope(&h.doc.model, ctx, inst, Scope::Instance).unwrap()["fiche"]["ref"],
        AttrValue::Text("C-12".into())
    );
    h.doc.redo();
    h.doc.redo();
    assert_eq!(options_of(&h.doc.model, ctx, inst)[0].1, AttrValue::Text("bleu".into()));
    // Attributes follow a copy, and "make unique" gives the copy its own definition attributes.
    let mut copy = h.doc.ents().instances[inst].clone();
    copy.xform = esq_core::glam::DMat4::from_translation(DVec3::new(2000.0, 0.0, 0.0)) * copy.xform;
    let mut second = None;
    h.doc.commit("Copie", |m, d| {
        second = Some(m.entities_mut(d).instances.insert(copy));
        m.make_unique(d, second.unwrap());
    });
    let second = second.unwrap();
    assert_eq!(h.doc.ents().instances[second].attributes["fiche"]["ref"], AttrValue::Text("C-12".into()));
    let (d1, d2) = (h.doc.ents().instances[inst].def, h.doc.ents().instances[second].def);
    assert_ne!(d1, d2);
    assert_eq!(h.doc.model.defs[d1].attributes, h.doc.model.defs[d2].attributes);
    h.doc.select(vec![EntityRef::Instance(second)]);
    let sel = match h.doc.sel.as_slice() {
        [EntityRef::Instance(i)] => Some(*i),
        _ => None,
    };
    assert_eq!(sel, Some(second));
    let _ = selected_instance;
}
mod audit_tools;
#[path = "tests_photo.rs"]
mod photo;

mod outils;
mod outils_restes;
