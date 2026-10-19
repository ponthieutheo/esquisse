//! Sandbox tools (Smoove, Stamp, Drape), driven like the mouse does.

use eframe::egui::Modifiers;
use esq_core::glam::DVec3;
use esq_core::model::{EntityRef, VertexId};
use esq_core::sculpt::{smoove_falloff, triangle_grid};

use super::Harness;
use crate::tools::ToolId;

/// A mesh of `n` × `n` cells of `s` mm centred on the origin; returns the vertex ids.
fn mesh(h: &mut Harness, n: usize, s: f64, z: impl Fn(usize, usize) -> f64) -> Vec<VertexId> {
    let half = n as f64 * s / 2.0;
    let ids = triangle_grid(&mut h.doc.model.root, DVec3::new(-half, -half, 0.0), n, n, s, z);
    h.doc.model_rev += 1;
    ids
}

fn z_of(h: &Harness, v: VertexId) -> f64 {
    h.doc.ents().pos(v).z
}

#[test]
fn smoove_lifts_the_mesh_under_the_radius() {
    let mut h = Harness::new();
    let ids = mesh(&mut h, 8, 125.0, |_, _| 0.0);
    let at = |i: usize, j: usize| ids[j * 9 + i];
    h.tool(ToolId::Smoove);
    h.type_vcb("400");
    // Base on the vertex at the centre, then 300 mm up and click.
    h.click(DVec3::ZERO);
    assert!(h.tool.busy());
    h.hover(DVec3::new(0.0, 0.0, 300.0));
    assert!((z_of(&h, at(4, 4)) - 0.0).abs() < 1e-9, "only a preview so far");
    h.click(DVec3::new(0.0, 0.0, 300.0));
    assert!(!h.tool.busy());
    assert!((z_of(&h, at(4, 4)) - 300.0).abs() < 2.0, "{}", z_of(&h, at(4, 4)));
    let near = z_of(&h, at(5, 4));
    assert!((near - 300.0 * smoove_falloff(125.0, 400.0)).abs() < 2.0, "{near}");
    // Corners are far: they stay where they were.
    assert_eq!(z_of(&h, at(0, 0)), 0.0);
    assert_eq!(z_of(&h, at(8, 8)), 0.0);
    // One undo step.
    h.doc.undo();
    assert_eq!(z_of(&h, at(4, 4)), 0.0);
}

#[test]
fn smoove_offset_and_radius_can_be_typed() {
    let mut h = Harness::new();
    let ids = mesh(&mut h, 8, 125.0, |_, _| 0.0);
    let at = |i: usize, j: usize| ids[j * 9 + i];
    h.tool(ToolId::Smoove);
    h.type_vcb("200");
    h.click(DVec3::ZERO);
    // A radius of 130 mm only takes the four neighbours at 125 mm.
    h.type_vcb("130r");
    h.type_vcb("-250");
    assert!(!h.tool.busy());
    assert!((z_of(&h, at(4, 4)) + 250.0).abs() < 1e-9);
    assert!(z_of(&h, at(5, 4)) < 0.0, "inside the radius");
    assert_eq!(z_of(&h, at(6, 4)), 0.0, "outside the radius");
    // The diameter works too: 260 mm is the same radius of 130 mm.
    h.tool(ToolId::Smoove);
    h.type_vcb("260d");
    h.click(DVec3::new(500.0, 500.0, 0.0));
    h.type_vcb("50");
    assert_eq!(z_of(&h, at(6, 6)), 0.0);
    assert!(z_of(&h, at(8, 8)) > 0.0);
    // Zero or negative radii are refused.
    h.tool(ToolId::Smoove);
    let r = h.with(|t, cx| t.vcb(cx, "0"));
    assert!(r.is_err());
}

#[test]
fn smoove_shift_follows_the_normal() {
    let mut h = Harness::new();
    // A slope of 1 for 1 along X: normals lean by 45°.
    let ids = mesh(&mut h, 8, 125.0, |i, _| i as f64 * 125.0);
    let at = |i: usize, j: usize| ids[j * 9 + i];
    h.tool(ToolId::Smoove);
    h.type_vcb("300");
    let centre = h.doc.ents().pos(at(4, 4));
    let n = DVec3::new(-1.0, 0.0, 1.0).normalize();
    h.click(centre);
    h.mods = Modifiers::SHIFT;
    h.hover(centre + n * 200.0);
    h.click(centre + n * 200.0);
    h.mods = Modifiers::NONE;
    let moved = h.doc.ents().pos(at(4, 4)) - centre;
    assert!(moved.length() > 150.0, "{moved:?}");
    assert!(moved.normalize().dot(n) > 0.99, "{moved:?}");
}

#[test]
fn smoove_uses_the_selected_edges() {
    let mut h = Harness::new();
    let ids = mesh(&mut h, 8, 125.0, |_, _| 0.0);
    let at = |i: usize, j: usize| ids[j * 9 + i];
    // Select the edge between (0,4) and (1,4), then click anywhere: the distances are
    // measured to that edge.
    let e = h.doc.ents().find_edge(at(0, 4), at(1, 4)).unwrap();
    h.doc.select(vec![EntityRef::Edge(e)]);
    h.tool(ToolId::Smoove);
    // (A radius wider than the distance to the vertices the mouse could snap to, so that
    // no vertex inference interferes.)
    h.type_vcb("600");
    h.click(DVec3::new(300.0, 0.0, 0.0));
    assert!(h.tool.busy());
    h.hover(DVec3::new(300.0, 0.0, 200.0));
    h.click(DVec3::new(300.0, 0.0, 200.0));
    assert!((z_of(&h, at(0, 4)) - 200.0).abs() < 8.0, "{}", z_of(&h, at(0, 4)));
    assert!(z_of(&h, at(1, 4)) > 150.0);
    assert_eq!(z_of(&h, at(8, 8)), 0.0, "far from the edge");
    assert_eq!(z_of(&h, at(8, 0)), 0.0);
}

#[test]
fn smoove_double_click_repeats_the_last_move() {
    let mut h = Harness::new();
    let ids = mesh(&mut h, 8, 125.0, |_, _| 0.0);
    let at = |i: usize, j: usize| ids[j * 9 + i];
    h.tool(ToolId::Smoove);
    h.type_vcb("200");
    h.click(DVec3::new(-250.0, 0.0, 0.0));
    h.type_vcb("100");
    assert!((z_of(&h, at(2, 4)) - 100.0).abs() < 1e-9);
    // Elsewhere: a click starts a Smoove, the double click repeats the 100 mm.
    let m = h.px(DVec3::new(250.0, 0.0, 0.0));
    h.with(|t, cx| {
        t.mouse_move(cx, m);
        t.press(cx, m);
        t.release(cx, m);
        t.press(cx, m);
        t.release(cx, m);
        t.double_click(cx, m);
    });
    assert!((z_of(&h, at(6, 4)) - 100.0).abs() < 1e-9, "{}", z_of(&h, at(6, 4)));
}

fn total_area(h: &Harness) -> f64 {
    let e = h.doc.ents();
    e.faces.keys().map(|f| esq_core::tess::face_area(e, f)).sum()
}

#[test]
fn drape_projects_selected_edges_onto_the_mesh() {
    let mut h = Harness::new();
    // A hill sloping along X; the edges to drape float 800 mm above and are not connected.
    mesh(&mut h, 8, 125.0, |i, _| i as f64 * 20.0);
    let faces_before = h.faces();
    let area = total_area(&h);
    let (a, b) = (DVec3::new(-400.0, 130.0, 800.0), DVec3::new(300.0, 130.0, 800.0));
    let edges = h.doc.model.root.add_line(a, b);
    h.doc.model_rev += 1;
    h.doc.select(edges.iter().map(|e| EntityRef::Edge(*e)).collect());
    h.tool(ToolId::Drape);
    // The selection is taken at once; the click designates the mesh.
    h.click(DVec3::new(-300.0, -300.0, 0.0));
    assert!(!h.tool.busy(), "done");
    assert!(h.doc.sel.is_empty());
    assert!(h.faces() > faces_before, "faces were cut");
    assert!((total_area(&h) - area).abs() < 1e-3, "nothing lost");
    // The draped line is on the mesh: a vertex at both ends, at the height of the slope.
    let ents = h.doc.ents();
    let on_mesh = |x: f64| {
        let z = (x + 500.0) / 125.0 * 20.0;
        ents.find_vertex(DVec3::new(x, 130.0, z)).is_some()
    };
    assert!(on_mesh(-400.0) && on_mesh(300.0));
    // The original edges did not move.
    assert!(ents.find_vertex(a).is_some() && ents.find_vertex(b).is_some());
    // One undo step.
    h.doc.undo();
    assert_eq!(h.faces(), faces_before);
}

/// A square face floating `z` mm above the ground, `half` mm from its centre to its sides.
fn floating_square(h: &mut Harness, cx: f64, cy: f64, half: f64, z: f64) -> esq_core::model::FaceId {
    let pts = [
        DVec3::new(cx - half, cy - half, z),
        DVec3::new(cx + half, cy - half, z),
        DVec3::new(cx + half, cy + half, z),
        DVec3::new(cx - half, cy + half, z),
    ];
    h.doc.model.root.add_polyline(&pts, true, false);
    h.doc.model_rev += 1;
    let (f, _) = h
        .doc
        .model
        .root
        .faces
        .iter()
        .find(|(_, f)| f.outer.len() == 4 && h.doc.model.root.pos(f.outer[0]).z == z)
        .expect("the square became a face");
    f
}

fn faces_at(h: &Harness, z: f64) -> usize {
    let e = h.doc.ents();
    e.faces.values().filter(|f| e.loop_points(&f.outer).iter().all(|p| (p.z - z).abs() < 8.0)).count()
}

#[test]
fn stamp_presses_a_footprint_into_the_mesh() {
    let mut h = Harness::new();
    mesh(&mut h, 12, 250.0, |_, _| 0.0);
    floating_square(&mut h, 0.0, 0.0, 300.0, 600.0);
    let faces_before = h.faces();
    h.tool(ToolId::Stamp);
    // The offset is typed first: 250 mm around the footprint.
    h.type_vcb("250");
    h.click(DVec3::new(0.0, 0.0, 600.0));
    assert!(h.tool.busy());
    // The mesh, away from the square.
    h.click(DVec3::new(-900.0, -900.0, 0.0));
    assert!(h.tool.busy(), "waiting for the elevation");
    assert_eq!(h.faces(), faces_before, "nothing is committed yet");
    // The footprint starts one inch under the highest point of the mesh; the mouse moves it.
    h.hover(DVec3::new(-900.0, -900.0, 400.0));
    h.click(DVec3::new(-900.0, -900.0, 400.0));
    assert!(!h.tool.busy());
    assert!(h.faces() > faces_before);
    // 0 - 25.4 + 400, give or take the pixel.
    assert!(faces_at(&h, 374.6) >= 1, "the footprint is at the chosen elevation");
    // The square that was stamped did not move.
    assert!(faces_at(&h, 600.0) >= 1);
    // The terrain around is intact, and the cut is a ring around the footprint.
    assert!(h.doc.ents().find_vertex(DVec3::new(-1000.0, -1000.0, 0.0)).is_some());
    assert!(h.doc.ents().find_vertex(DVec3::new(0.0, 0.0, 0.0)).is_none());
    // One undo step.
    h.doc.undo();
    assert_eq!(h.faces(), faces_before);
}

#[test]
fn stamp_a_selected_group_into_a_group() {
    let mut h = Harness::new();
    mesh(&mut h, 12, 250.0, |i, j| ((i + j) % 3) as f64 * 10.0);
    let all: Vec<EntityRef> = h.doc.model.root.faces.keys().map(EntityRef::Face).collect();
    let terrain = h.doc.model.make_group(None, &all, true, Some("Terrain".into())).expect("terrain group");
    // A box on the ground, in a group of its own.
    let sq = floating_square(&mut h, 0.0, 0.0, 300.0, 300.0);
    let mut items = vec![EntityRef::Face(sq)];
    items.extend(h.doc.model.root.edges.keys().map(EntityRef::Edge));
    let building = h.doc.model.make_group(None, &items, true, Some("Maison".into())).expect("building group");
    h.doc.model_rev += 1;
    let def = h.doc.model.root.instances[terrain].def;
    let terrain_faces = h.doc.model.defs[def].entities.faces.len();
    h.doc.select(vec![EntityRef::Instance(building)]);
    h.tool(ToolId::Stamp);
    h.type_vcb("200");
    // The group is taken from the selection; the click designates the terrain group.
    h.click(DVec3::new(-900.0, 900.0, 0.0));
    assert!(h.tool.busy(), "waiting for the elevation");
    h.click(DVec3::new(-900.0, 900.0, 0.0));
    assert!(!h.tool.busy());
    assert!(h.doc.model.defs[def].entities.faces.len() != terrain_faces, "the terrain group was edited");
    // The footprint is in the terrain group now, at the height of its highest point minus
    // an inch (the group's own frame).
    let xf = h.doc.model.root.instances[terrain].xform;
    let ents = &h.doc.model.defs[def].entities;
    let top = ents.verts.values().map(|v| v.pos.z).fold(f64::MIN, f64::max);
    assert!(top <= 20.0 + 1e-6, "no vertex above the terrain's own relief: {top}");
    let foot_world: Vec<DVec3> = ents
        .faces
        .values()
        .filter(|f| ents.loop_points(&f.outer).iter().all(|p| (p.z - ents.loop_points(&f.outer)[0].z).abs() < 1e-6))
        .filter(|f| ents.loop_points(&f.outer).iter().all(|p| xf.transform_point3(*p).x.abs() < 301.0))
        .flat_map(|f| ents.loop_points(&f.outer))
        .map(|p| xf.transform_point3(p))
        .collect();
    assert!(foot_world.len() >= 4, "{foot_world:?}");
    assert!(foot_world.iter().all(|p| p.x.abs() <= 300.1 && p.y.abs() <= 300.1));
}

#[test]
fn stamp_refuses_vertical_faces_and_escape_forgets() {
    let mut h = Harness::new();
    mesh(&mut h, 12, 250.0, |_, _| 0.0);
    // A vertical wall is not a footprint.
    h.doc.model.root.add_polyline(
        &[
            DVec3::new(-200.0, 0.0, 100.0),
            DVec3::new(200.0, 0.0, 100.0),
            DVec3::new(200.0, 0.0, 500.0),
            DVec3::new(-200.0, 0.0, 500.0),
        ],
        true,
        false,
    );
    h.doc.model_rev += 1;
    h.tool(ToolId::Stamp);
    h.click(DVec3::new(0.0, 0.0, 300.0));
    assert!(!h.tool.busy(), "a vertical face gives no footprint");
    // A flat one does; Escape drops it.
    floating_square(&mut h, 800.0, 800.0, 100.0, 400.0);
    h.click(DVec3::new(800.0, 800.0, 400.0));
    assert!(h.tool.busy());
    h.with(|t, cx| t.cancel(cx));
    assert!(!h.tool.busy());
    // The offset cannot be nought.
    h.tool(ToolId::Stamp);
    h.type_vcb("0");
    assert_eq!(h.tool.vcb_value(esq_core::model::Units::Millimeters), "25,4 mm");
}

#[test]
fn drape_picks_an_edge_then_the_mesh() {
    let mut h = Harness::new();
    mesh(&mut h, 8, 125.0, |_, _| 0.0);
    let faces_before = h.faces();
    let (a, b) = (DVec3::new(-400.0, 130.0, 800.0), DVec3::new(300.0, 130.0, 800.0));
    h.doc.model.root.add_line(a, b);
    h.doc.model_rev += 1;
    h.tool(ToolId::Drape);
    // First click on the floating edge, second on the mesh.
    h.click(DVec3::new(-50.0, 130.0, 800.0));
    assert!(h.tool.busy(), "the edge is picked");
    // A click in the void does nothing.
    h.click(DVec3::new(900.0, 900.0, 900.0));
    assert!(h.tool.busy());
    h.click(DVec3::new(-300.0, -300.0, 0.0));
    assert!(!h.tool.busy());
    assert!(h.faces() > faces_before);
    // Escape forgets what was picked.
    h.click(DVec3::new(-50.0, 130.0, 800.0));
    assert!(h.tool.busy());
    h.with(|t, cx| t.cancel(cx));
    assert!(!h.tool.busy());
}

#[test]
fn drape_a_group_into_another_group() {
    let mut h = Harness::new();
    mesh(&mut h, 8, 125.0, |_, _| 0.0);
    // A group holding a rectangle high above one corner of the mesh.
    let rect = h.doc.model.root.add_polyline(
        &[
            DVec3::new(-300.0, -300.0, 900.0),
            DVec3::new(0.0, -300.0, 900.0),
            DVec3::new(0.0, 0.0, 900.0),
            DVec3::new(-300.0, 0.0, 900.0),
        ],
        true,
        false,
    );
    let mut items: Vec<EntityRef> = rect.iter().map(|e| EntityRef::Edge(*e)).collect();
    items.extend(h.doc.model.root.faces.iter().filter(|(_, f)| f.outer.len() == 4).map(|(k, _)| EntityRef::Face(k)));
    let g = h.doc.model.make_group(None, &items, true, None).expect("group");
    h.doc.model_rev += 1;
    h.doc.select(vec![EntityRef::Instance(g)]);
    let faces_before = h.faces();
    h.tool(ToolId::Drape);
    h.click(DVec3::new(300.0, 300.0, 0.0));
    assert!(h.faces() > faces_before);
    // The rectangle is drawn on the mesh.
    for p in [(-300.0, -300.0), (0.0, -300.0), (0.0, 0.0), (-300.0, 0.0)] {
        assert!(h.doc.ents().find_vertex(DVec3::new(p.0, p.1, 0.0)).is_some(), "{p:?}");
    }
}
