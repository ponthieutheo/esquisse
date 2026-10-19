//! Sandbox tools, driven like a user would.

use esq_core::glam::DVec3;
use esq_core::model::{EntityRef, InstanceId};

use super::Harness;
use crate::tools::ToolId;

/// Grid spacing used by every test here (the setting is shared by the whole process).
const SPACING: &str = "200";

fn groups(h: &Harness) -> Vec<InstanceId> {
    h.doc.ents().instances.keys().collect()
}

fn faces_in(h: &Harness, g: InstanceId) -> usize {
    h.doc.model.defs[h.doc.ents().instances[g].def].entities.faces.len()
}

fn scratch(h: &mut Harness) {
    h.tool(ToolId::FromScratch);
    h.type_vcb(SPACING);
}

#[test]
fn from_scratch_makes_a_grid_group_with_three_clicks() {
    let mut h = Harness::new();
    scratch(&mut h);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1050.0, 0.0, 0.0));
    h.hover(DVec3::new(1050.0, 650.0, 0.0));
    assert!(groups(&h).is_empty(), "nothing before the third click");
    h.click(DVec3::new(1050.0, 650.0, 0.0));
    let g = groups(&h);
    assert_eq!(g.len(), 1);
    // 5 x 3 squares of 200 mm, two triangles each.
    assert_eq!(faces_in(&h, g[0]), 5 * 3 * 2);
    let inst = &h.doc.ents().instances[g[0]];
    assert!(inst.xform.transform_vector3(DVec3::Z).z.abs() > 0.99, "lies on the ground");
    h.doc.undo();
    assert!(groups(&h).is_empty(), "one undo step");
}

#[test]
fn from_scratch_ignores_a_corner_closer_than_one_square() {
    let mut h = Harness::new();
    scratch(&mut h);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(100.0, 0.0, 0.0));
    h.click(DVec3::new(1050.0, 0.0, 0.0));
    h.click(DVec3::new(1050.0, 100.0, 0.0));
    assert!(groups(&h).is_empty(), "still waiting for the depth");
    h.click(DVec3::new(1050.0, 650.0, 0.0));
    assert_eq!(groups(&h).len(), 1);
}

#[test]
fn from_scratch_typed_lengths() {
    let mut h = Harness::new();
    scratch(&mut h);
    h.click(DVec3::ZERO);
    h.hover(DVec3::new(900.0, 0.0, 0.0));
    h.type_vcb("1050");
    h.hover(DVec3::new(1050.0, 700.0, 0.0));
    h.type_vcb("650");
    let g = groups(&h);
    assert_eq!(g.len(), 1);
    assert_eq!(faces_in(&h, g[0]), 5 * 3 * 2);
}

#[test]
fn from_scratch_typed_depth_right_after_redoes_the_grid() {
    let mut h = Harness::new();
    scratch(&mut h);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1050.0, 0.0, 0.0));
    h.click(DVec3::new(1050.0, 650.0, 0.0));
    h.type_vcb("450");
    let g = groups(&h);
    assert_eq!(g.len(), 1, "the first grid was replaced");
    assert_eq!(faces_in(&h, g[0]), 5 * 2 * 2);
}

#[test]
fn from_scratch_squares_are_soft_on_the_diagonal_and_selection_is_untouched() {
    let mut h = Harness::new();
    scratch(&mut h);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1050.0, 0.0, 0.0));
    h.click(DVec3::new(1050.0, 650.0, 0.0));
    let g = groups(&h)[0];
    let def = &h.doc.model.defs[h.doc.ents().instances[g].def];
    assert_eq!(def.entities.edges.values().filter(|e| e.soft).count(), 15);
    assert!(h.doc.sel.iter().all(|e| !matches!(e, EntityRef::Instance(_))));
}

/// Two contour lines, at 0 and 500 mm of height, selected.
fn contours(h: &mut Harness) {
    h.doc.commit("Courbes", |m, d| {
        let e = m.entities_mut(d);
        let low = [DVec3::new(0.0, 0.0, 0.0), DVec3::new(1000.0, 0.0, 0.0), DVec3::new(2000.0, 300.0, 0.0)];
        let high = [DVec3::new(0.0, 800.0, 500.0), DVec3::new(1000.0, 900.0, 500.0), DVec3::new(2000.0, 1100.0, 500.0)];
        e.add_polyline(&low, false, false);
        e.add_polyline(&high, false, false);
    });
    let edges: Vec<EntityRef> = h.doc.ents().edges.keys().map(EntityRef::Edge).collect();
    h.doc.select(edges);
}

#[test]
fn from_contours_needs_selected_edges() {
    let mut h = Harness::new();
    let r = crate::tools::sandbox::on_command(ToolId::FromContours, &mut h.doc).expect("acts at once");
    assert!(r.unwrap_err().contains("sélectionner les contours"));
    assert!(groups(&h).is_empty());
}

#[test]
fn from_contours_triangulates_the_selected_edges_into_a_group() {
    let mut h = Harness::new();
    contours(&mut h);
    let before = h.doc.ents().edges.len();
    let r = crate::tools::sandbox::on_command(ToolId::FromContours, &mut h.doc).expect("acts at once");
    assert_eq!(r, Ok(()));
    let g = groups(&h);
    assert_eq!(g.len(), 1);
    assert!(h.doc.sel.is_empty(), "the selection is cleared");
    assert_eq!(h.doc.ents().edges.len(), before, "the contours stay");
    let def = &h.doc.model.defs[h.doc.ents().instances[g[0]].def];
    assert!(def.entities.faces.values().all(|f| f.normal.z > 0.0 && f.outer.len() == 3));
    assert!(def.entities.verts.values().any(|v| v.pos.z == 500.0));
    assert!(def.entities.verts.values().any(|v| v.pos.z == 0.0));
    h.doc.undo();
    assert!(groups(&h).is_empty(), "one undo step");
}

/// A 1000 x 600 rectangle of two triangles, cut from (1000,0) to (0,600), in the root.
fn two_triangles(h: &mut Harness) {
    h.doc.commit("Maillage", |m, d| {
        let pts = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(1000.0, 0.0, 0.0),
            DVec3::new(0.0, 600.0, 0.0),
            DVec3::new(1000.0, 600.0, 0.0),
        ];
        m.entities_mut(d).add_mesh(&pts, &[[0, 1, 2], [2, 1, 3]], &[]);
    });
}

fn has_edge(h: &Harness, a: DVec3, b: DVec3) -> bool {
    let e = h.doc.ents();
    match (e.find_vertex(a), e.find_vertex(b)) {
        (Some(x), Some(y)) => e.find_edge(x, y).is_some(),
        _ => false,
    }
}

#[test]
fn flip_edge_turns_the_diagonal_with_one_click() {
    let mut h = Harness::new();
    two_triangles(&mut h);
    h.tool(ToolId::FlipEdge);
    let (c0, c1) = (DVec3::new(0.0, 0.0, 0.0), DVec3::new(1000.0, 600.0, 0.0));
    h.hover(DVec3::new(500.0, 300.0, 0.0));
    assert_eq!(h.doc.sel.len(), 1, "the flippable edge is highlighted");
    assert!(!has_edge(&h, c0, c1));
    h.click(DVec3::new(500.0, 300.0, 0.0));
    assert!(has_edge(&h, c0, c1));
    assert!(!has_edge(&h, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(0.0, 600.0, 0.0)));
    assert_eq!(h.faces(), 2);
    assert!(h.doc.sel.is_empty());
    assert!(h.doc.ents().faces.values().all(|f| f.normal.z.abs() > 0.99));
    // And back, in one undo step each.
    h.click(DVec3::new(500.0, 300.0, 0.0));
    assert!(!has_edge(&h, c0, c1));
    h.doc.undo();
    assert!(has_edge(&h, c0, c1));
}

fn vertex_xy(h: &Harness, x: f64, y: f64) -> Option<DVec3> {
    h.doc.ents().verts.values().map(|v| v.pos).find(|p| (p.x - x).abs() < 0.5 && (p.y - y).abs() < 0.5)
}

#[test]
fn add_detail_on_a_face_then_offset_by_the_mouse() {
    let mut h = Harness::new();
    two_triangles(&mut h);
    h.tool(ToolId::AddDetail);
    h.click(DVec3::new(200.0, 100.0, 0.0));
    assert_eq!(h.faces(), 4, "the triangle is cut in three around the new vertex");
    assert_eq!(h.doc.undo_label(), Some("Ajouter des détails"));
    h.hover(DVec3::new(200.0, 100.0, 300.0));
    assert!(h.doc.preview.is_some(), "the offset is shown before the click");
    h.click(DVec3::new(200.0, 100.0, 300.0));
    let v = vertex_xy(&h, 200.0, 100.0).expect("the new vertex");
    assert!((v.z - 300.0).abs() < 1.0, "{v:?}");
    assert_eq!(h.faces(), 4);
    assert_eq!(h.doc.undo_label(), Some("Décalage de détails"));
    h.doc.undo();
    assert!((vertex_xy(&h, 200.0, 100.0).unwrap().z).abs() < 1e-9, "the offset is its own undo step");
    assert_eq!(h.faces(), 4);
    h.doc.undo();
    assert_eq!(h.faces(), 2);
}

#[test]
fn add_detail_on_an_edge_with_a_typed_offset() {
    let mut h = Harness::new();
    two_triangles(&mut h);
    h.tool(ToolId::AddDetail);
    h.click(DVec3::new(500.0, 300.0, 0.0));
    assert_eq!(h.faces(), 4, "both faces on the edge are cut in two");
    h.type_vcb("250");
    let v = vertex_xy(&h, 500.0, 300.0).expect("the new vertex");
    assert!((v.z - 250.0).abs() < 1e-6, "{v:?}");
}

#[test]
fn add_detail_with_ctrl_leaves_the_vertex_in_place() {
    let mut h = Harness::new();
    two_triangles(&mut h);
    h.tool(ToolId::AddDetail);
    h.mods.ctrl = true;
    h.click(DVec3::new(200.0, 100.0, 0.0));
    h.mods.ctrl = false;
    assert_eq!(h.faces(), 4);
    assert!(!h.tool.busy(), "no offset step");
    assert!(h.with(|t, cx| t.vcb(cx, "100")).is_err());
}

#[test]
fn add_detail_on_a_vertex_just_offsets_it() {
    let mut h = Harness::new();
    two_triangles(&mut h);
    h.tool(ToolId::AddDetail);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    assert_eq!(h.faces(), 2);
    assert_eq!(h.doc.ents().verts.len(), 4);
    h.type_vcb("100");
    assert!((vertex_xy(&h, 1000.0, 0.0).unwrap().z - 100.0).abs() < 1e-6);
}

#[test]
fn add_detail_with_shift_offsets_along_the_normal() {
    let mut h = Harness::new();
    h.doc.commit("Mur", |m, d| {
        let pts = [DVec3::new(0.0, 0.0, 0.0), DVec3::new(1000.0, 0.0, 0.0), DVec3::new(0.0, 0.0, 600.0)];
        m.entities_mut(d).add_mesh(&pts, &[[0, 1, 2]], &[]);
    });
    h.tool(ToolId::AddDetail);
    h.mods.shift = true;
    h.click(DVec3::new(200.0, 0.0, 100.0));
    h.type_vcb("100");
    h.mods.shift = false;
    let v = h.doc.ents().verts.values().map(|v| v.pos).find(|p| (p.x - 200.0).abs() < 0.5).expect("new vertex");
    assert!((v.z - 100.0).abs() < 1e-6, "stays at its height: {v:?}");
    assert!((v.y.abs() - 100.0).abs() < 1e-6, "moved along the wall's normal: {v:?}");
}

#[test]
fn add_detail_escape_keeps_the_detail_but_not_the_offset() {
    let mut h = Harness::new();
    two_triangles(&mut h);
    h.tool(ToolId::AddDetail);
    h.click(DVec3::new(200.0, 100.0, 0.0));
    h.hover(DVec3::new(200.0, 100.0, 300.0));
    h.with(|t, cx| t.cancel(cx));
    assert!(h.doc.preview.is_none());
    assert_eq!(h.faces(), 4);
    assert!((vertex_xy(&h, 200.0, 100.0).unwrap().z).abs() < 1e-9);
}

#[test]
fn add_detail_refuses_faces_that_are_not_triangles() {
    let mut h = Harness::new();
    h.doc.commit("Carré", |m, d| {
        m.entities_mut(d).add_polyline(
            &[
                DVec3::new(0.0, 0.0, 0.0),
                DVec3::new(1000.0, 0.0, 0.0),
                DVec3::new(1000.0, 600.0, 0.0),
                DVec3::new(0.0, 600.0, 0.0),
            ],
            true,
            false,
        );
    });
    h.tool(ToolId::AddDetail);
    h.click(DVec3::new(300.0, 300.0, 0.0));
    assert_eq!(h.faces(), 1);
    assert!(!h.tool.busy());
}

#[test]
fn add_detail_with_a_selection_subdivides_the_triangles() {
    let mut h = Harness::new();
    two_triangles(&mut h);
    let all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    h.doc.select(all);
    let r = crate::tools::sandbox::on_command(ToolId::AddDetail, &mut h.doc).expect("acts at once");
    assert_eq!(r, Ok(()));
    assert_eq!(h.faces(), 8);
    assert!(h.doc.sel.is_empty());
    h.doc.undo();
    assert_eq!(h.faces(), 2, "one undo step");
    // Without a selection it is an ordinary tool.
    assert!(crate::tools::sandbox::on_command(ToolId::AddDetail, &mut h.doc).is_none());
    // A selection without triangles is refused.
    let e = h.doc.ents().edges.keys().next().map(EntityRef::Edge).unwrap();
    h.doc.select(vec![e]);
    assert!(crate::tools::sandbox::on_command(ToolId::AddDetail, &mut h.doc).unwrap().is_err());
}

#[test]
fn flip_edge_ignores_border_edges() {
    let mut h = Harness::new();
    two_triangles(&mut h);
    h.tool(ToolId::FlipEdge);
    h.hover(DVec3::new(500.0, 0.0, 0.0));
    assert!(h.doc.sel.is_empty());
    h.click(DVec3::new(500.0, 0.0, 0.0));
    assert!(has_edge(&h, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(0.0, 600.0, 0.0)));
}
