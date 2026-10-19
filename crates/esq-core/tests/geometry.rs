use esq_core::glam::DVec3;
use esq_core::model::{Entities, EntityRef, Model};
use esq_core::tess::face_area;

fn p(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn rect(e: &mut Entities, x0: f64, y0: f64, x1: f64, y1: f64, z: f64) {
    e.add_polyline(&[p(x0, y0, z), p(x1, y0, z), p(x1, y1, z), p(x0, y1, z)], true, false);
}

fn box_1m() -> Entities {
    let mut e = Entities::default();
    rect(&mut e, 0.0, 0.0, 1000.0, 1000.0, 0.0);
    let f = e.faces.keys().next().unwrap();
    // The ground face looks down, so pulling up is a negative distance along its normal.
    e.push_pull(f, -1000.0, false);
    e
}

fn assert_outward(e: &Entities, center: DVec3) {
    for (_, face) in e.faces.iter() {
        let c = e.pos(face.outer[0]);
        assert!(face.normal.dot(c - center) > 0.0, "face normal points inwards: {:?}", face.normal);
    }
}

#[test]
fn rectangle_makes_a_face_looking_down() {
    let mut e = Entities::default();
    rect(&mut e, 0.0, 0.0, 1000.0, 500.0, 0.0);
    assert_eq!(e.edges.len(), 4);
    assert_eq!(e.faces.len(), 1);
    let f = e.faces.values().next().unwrap();
    assert!(f.normal.z < -0.99);
    assert!((face_area(&e, e.faces.keys().next().unwrap()) - 500_000.0).abs() < 1e-6);
}

#[test]
fn closing_a_loop_line_by_line_creates_the_face_at_the_end() {
    let mut e = Entities::default();
    e.add_line(p(0.0, 0.0, 0.0), p(1000.0, 0.0, 0.0));
    e.add_line(p(1000.0, 0.0, 0.0), p(1000.0, 1000.0, 0.0));
    e.add_line(p(1000.0, 1000.0, 0.0), p(0.0, 1000.0, 0.0));
    assert_eq!(e.faces.len(), 0);
    e.add_line(p(0.0, 1000.0, 0.0), p(0.0, 0.0, 0.0));
    assert_eq!(e.faces.len(), 1);
}

#[test]
fn line_across_face_splits_it() {
    let mut e = Entities::default();
    rect(&mut e, 0.0, 0.0, 1000.0, 1000.0, 0.0);
    e.add_line(p(500.0, 0.0, 0.0), p(500.0, 1000.0, 0.0));
    assert_eq!(e.faces.len(), 2);
    assert_eq!(e.edges.len(), 7);
    for (f, face) in e.faces.iter() {
        assert!(face.normal.z < -0.99);
        assert!((face_area(&e, f) - 500_000.0).abs() < 1e-6);
    }
}

#[test]
fn crossing_lines_split_each_other() {
    let mut e = Entities::default();
    e.add_line(p(0.0, 0.0, 0.0), p(1000.0, 1000.0, 0.0));
    e.add_line(p(0.0, 1000.0, 0.0), p(1000.0, 0.0, 0.0));
    assert_eq!(e.edges.len(), 4);
    assert!(e.find_vertex(p(500.0, 500.0, 0.0)).is_some());
}

#[test]
fn rectangle_inside_face_makes_a_hole() {
    let mut e = Entities::default();
    rect(&mut e, 0.0, 0.0, 1000.0, 1000.0, 0.0);
    rect(&mut e, 250.0, 250.0, 750.0, 750.0, 0.0);
    assert_eq!(e.faces.len(), 2);
    let holed = e.faces.values().filter(|f| f.holes.len() == 1).count();
    assert_eq!(holed, 1);
    let total: f64 = e.faces.keys().map(|f| face_area(&e, f)).sum();
    assert!((total - 1_000_000.0).abs() < 1e-6);
}

#[test]
fn push_pull_makes_a_closed_box() {
    let e = box_1m();
    assert_eq!(e.faces.len(), 6);
    assert_eq!(e.edges.len(), 12);
    assert_eq!(e.verts.len(), 8);
    assert_outward(&e, p(500.0, 500.0, 500.0));
}

#[test]
fn push_pull_top_stretches_the_box() {
    let mut e = box_1m();
    let top = e.faces.iter().find(|(_, f)| f.normal.z > 0.99).map(|(k, _)| k).unwrap();
    e.push_pull(top, 500.0, false);
    assert_eq!(e.faces.len(), 6);
    assert_eq!(e.edges.len(), 12);
    let zmax = e.verts.values().map(|v| v.pos.z).fold(0.0, f64::max);
    assert!((zmax - 1500.0).abs() < 1e-9);
    assert_outward(&e, p(500.0, 500.0, 750.0));
}

#[test]
fn push_pull_with_copy_keeps_the_base() {
    let mut e = box_1m();
    let top = e.faces.iter().find(|(_, f)| f.normal.z > 0.99).map(|(k, _)| k).unwrap();
    e.push_pull(top, 500.0, true);
    assert_eq!(e.faces.len(), 11);
}

#[test]
fn rectangle_on_top_of_box_splits_the_top() {
    let mut e = box_1m();
    rect(&mut e, 250.0, 250.0, 750.0, 750.0, 1000.0);
    assert_eq!(e.faces.len(), 7);
    let inner = e
        .faces
        .iter()
        .find(|(f, face)| face.normal.z > 0.99 && face.holes.is_empty() && face_area(&e, *f) < 300_000.0)
        .map(|(k, _)| k)
        .expect("inner face facing up");
    e.push_pull(inner, 200.0, false);
    assert_eq!(e.faces.len(), 11);
    assert_outward(&e, p(500.0, 500.0, 500.0));
}

#[test]
fn erasing_a_split_line_merges_faces_back() {
    let mut e = Entities::default();
    rect(&mut e, 0.0, 0.0, 1000.0, 1000.0, 0.0);
    let line = e.add_line(p(500.0, 0.0, 0.0), p(500.0, 1000.0, 0.0));
    e.erase_edges(&line);
    assert_eq!(e.faces.len(), 1);
    assert_eq!(e.edges.len(), 4, "collinear pieces should be healed");
    assert!((face_area(&e, e.faces.keys().next().unwrap()) - 1_000_000.0).abs() < 1e-6);
}

#[test]
fn erasing_a_box_edge_removes_its_two_faces() {
    let mut e = box_1m();
    let edge = e.edges.keys().next().unwrap();
    e.erase_edges(&[edge]);
    assert_eq!(e.faces.len(), 4);
}

#[test]
fn circle_pushed_up_has_soft_sides() {
    let mut e = Entities::default();
    let pts: Vec<DVec3> = (0..24)
        .map(|i| {
            let a = i as f64 / 24.0 * std::f64::consts::TAU;
            p(500.0 * a.cos(), 500.0 * a.sin(), 0.0)
        })
        .collect();
    e.add_polyline(&pts, true, true);
    assert_eq!(e.faces.len(), 1);
    let f = e.faces.keys().next().unwrap();
    e.push_pull(f, -1000.0, false);
    assert_eq!(e.faces.len(), 26);
    let soft = e.edges.values().filter(|x| x.soft).count();
    assert_eq!(soft, 24);
}

#[test]
fn group_and_explode_round_trip() {
    let mut m = Model::new();
    m.root = box_1m();
    let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    let inst = m.make_group(None, &items, true, None).unwrap();
    assert_eq!(m.root.faces.len(), 0);
    assert_eq!(m.root.edges.len(), 0);
    assert_eq!(m.root.instances.len(), 1);
    let def = m.root.instances[inst].def;
    assert_eq!(m.defs[def].entities.faces.len(), 6);
    m.explode(None, inst);
    assert_eq!(m.root.faces.len(), 6);
    assert_eq!(m.root.edges.len(), 12);
    assert!(m.defs.is_empty());
    assert_outward(&m.root, p(500.0, 500.0, 500.0));
}

#[test]
fn moving_a_box_copy_keeps_it_closed() {
    let mut e = box_1m();
    let items: Vec<EntityRef> = e.faces.keys().map(EntityRef::Face).collect();
    let all = e.with_bounding_edges(&items);
    let copy = e.duplicate(&all);
    e.transform_selection(&copy, &esq_core::glam::DMat4::from_translation(p(2000.0, 0.0, 0.0)));
    assert_eq!(e.faces.len(), 12);
    assert_eq!(e.edges.len(), 24);
}

#[test]
fn save_and_load_native_format() {
    let mut m = Model::new();
    m.root = box_1m();
    let dir = std::env::temp_dir().join("esq-core-test.esq");
    esq_core::io::save_esq(&m, &dir).unwrap();
    let back = esq_core::io::load_esq(&dir).unwrap();
    assert_eq!(back.root.faces.len(), 6);
    assert_eq!(back.root.edges.values().map(|e| e.faces.len()).sum::<usize>(), 24);
    std::fs::remove_file(dir).ok();
}

#[test]
fn offset_draws_an_inner_loop() {
    let mut e = Entities::default();
    rect(&mut e, 0.0, 0.0, 1000.0, 1000.0, 0.0);
    let f = e.faces.keys().next().unwrap();
    e.offset_face(f, 100.0);
    assert_eq!(e.faces.len(), 2);
    let areas: Vec<f64> = e.faces.keys().map(|f| face_area(&e, f)).collect();
    assert!(areas.iter().any(|a| (a - 640_000.0).abs() < 1e-6), "{areas:?}");
}

fn volume(e: &Entities) -> f64 {
    // Divergence theorem over the closed surface.
    e.faces.keys().flat_map(|f| esq_core::tess::face_triangles(e, f)).map(|[a, b, c]| a.dot(b.cross(c)) / 6.0).sum()
}

fn has_edge(e: &Entities, a: DVec3, b: DVec3) -> bool {
    e.edges.keys().any(|id| {
        let (x, y) = e.edge_points(id);
        (x.distance(a) < 1e-6 && y.distance(b) < 1e-6) || (x.distance(b) < 1e-6 && y.distance(a) < 1e-6)
    })
}

#[test]
fn intersect_faces_with_a_group() {
    use esq_core::intersect::IntersectWith;
    let mut m = Model::new();
    m.root = box_1m();
    // A second cube, in a group, overlapping the first one's top right back corner.
    let mut other = Model::new();
    other.root = box_1m();
    let def = m.import_as_component(&other, "B");
    m.root.instances.insert(esq_core::model::Instance {
        def,
        xform: esq_core::glam::DMat4::from_translation(p(500.0, 500.0, 500.0)),
        name: String::new(),
        material: None,
        layer: 0,
        hidden: false,
        locked: false,
        attributes: Default::default(),
        ext: Default::default(),
    });
    let faces: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    let n = m.intersect_faces(None, &faces, IntersectWith::Model);
    assert_eq!(n, 6, "two segments on each of the three faces inside the other cube");
    assert_eq!(m.root.faces.len(), 9, "each of those faces is split in two");
    assert!(has_edge(&m.root, p(1000.0, 500.0, 500.0), p(1000.0, 500.0, 1000.0)));
    assert!(has_edge(&m.root, p(1000.0, 500.0, 500.0), p(1000.0, 1000.0, 500.0)));
    assert_eq!(m.defs[def].entities.faces.len(), 6, "the group is left alone");
    // Nothing new the second time.
    let faces: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    m.intersect_faces(None, &faces, IntersectWith::Model);
    assert_eq!(m.root.faces.len(), 9);
}

#[test]
fn intersect_selection_cuts_a_wall_with_a_slab() {
    use esq_core::intersect::IntersectWith;
    let mut e = Entities::default();
    // A vertical wall (plane x = 500) and a horizontal slab crossing it.
    e.add_polyline(
        &[p(500.0, 0.0, 0.0), p(500.0, 1000.0, 0.0), p(500.0, 1000.0, 1000.0), p(500.0, 0.0, 1000.0)],
        true,
        false,
    );
    rect(&mut e, 0.0, 200.0, 1000.0, 800.0, 400.0);
    assert_eq!(e.faces.len(), 2);
    let mut m = Model::new();
    m.root = e;
    let all: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    m.intersect_faces(None, &all, IntersectWith::Selection);
    // The line crosses the slab from side to side (it splits), but stays inside the wall.
    assert_eq!(m.root.faces.len(), 3);
    assert!(has_edge(&m.root, p(500.0, 200.0, 400.0), p(500.0, 800.0, 400.0)));
}

#[test]
fn follow_me_along_an_open_path() {
    let mut e = Entities::default();
    // Square profile 100 × 100 standing at the origin, in the plane x = 0.
    e.add_polyline(&[p(0.0, 0.0, 0.0), p(0.0, 100.0, 0.0), p(0.0, 100.0, 100.0), p(0.0, 0.0, 100.0)], true, false);
    let profile = e.faces.keys().next().unwrap();
    // L-shaped path: 1000 along x, then 1000 along y (starting at the profile's corner).
    let path = esq_core::followme::Path {
        points: vec![p(0.0, 0.0, 0.0), p(1000.0, 0.0, 0.0), p(1000.0, 1000.0, 0.0)],
        closed: false,
        smooth: vec![false; 3],
    };
    e.follow_me(profile, &path).unwrap();
    assert_eq!(e.faces.len(), 10, "2 caps + 4 sides × 2 legs");
    // Each leg is 1000 long on its outer side and 900 on its inner side (the mitre).
    let v = volume(&e).abs();
    assert!((v - 100.0 * 100.0 * 1900.0).abs() < 1e-3, "volume {v}");
    assert!(volume(&e) > 0.0, "faces look outwards");
    let xmax = e.verts.values().map(|v| v.pos.x).fold(f64::MIN, f64::max);
    let ymax = e.verts.values().map(|v| v.pos.y).fold(f64::MIN, f64::max);
    assert!((xmax - 1000.0).abs() < 1e-6 && (ymax - 1000.0).abs() < 1e-6, "{xmax} {ymax}");
}

#[test]
fn follow_me_around_a_closed_path_makes_a_ring() {
    let mut e = Entities::default();
    // A frame moulding: 50 × 50 profile at the corner of a 1000 × 1000 square path.
    rect(&mut e, 0.0, 0.0, 1000.0, 1000.0, 0.0);
    let base = e.faces.keys().next().unwrap();
    let path_edges = e.face_edges(base);
    e.add_polyline(&[p(0.0, 0.0, 0.0), p(0.0, -50.0, 0.0), p(0.0, -50.0, 50.0), p(0.0, 0.0, 50.0)], true, false);
    let profile = e.faces.keys().find(|&f| f != base).unwrap();
    let path = e.edge_path(&path_edges).unwrap();
    assert!(path.closed);
    e.follow_me(profile, &path).unwrap();
    assert!(!e.faces.contains_key(profile), "the profile is used up");
    assert_eq!(e.faces.len(), 1 + 4 * 4, "base + 4 sides × 4 legs");
    // Outer square 1100, inner 1000, height 50.
    let ring = 50.0 * (1100.0 * 1100.0 - 1000.0 * 1000.0);
    let v = volume(&e).abs();
    assert!((v - ring).abs() < 1.0, "volume {v} vs {ring}");
}

#[test]
fn follow_me_with_a_circle_is_smooth() {
    let mut e = Entities::default();
    // A pipe: a circle profile swept along a straight then bent path.
    let pts: Vec<DVec3> = (0..12)
        .map(|i| {
            let a = i as f64 / 12.0 * std::f64::consts::TAU;
            p(0.0, 50.0 * a.cos(), 50.0 * a.sin())
        })
        .collect();
    e.add_polyline(&pts, true, true);
    let profile = e.faces.keys().next().unwrap();
    let path = esq_core::followme::Path {
        points: vec![p(0.0, 0.0, 0.0), p(800.0, 0.0, 0.0), p(800.0, 0.0, 600.0)],
        closed: false,
        smooth: vec![false; 3],
    };
    e.follow_me(profile, &path).unwrap();
    assert_eq!(e.faces.len(), 2 + 12 * 2);
    let soft = e.edges.values().filter(|x| x.soft).count();
    assert_eq!(soft, 12 * 2, "lengthwise edges of the pipe are soft");
}

#[test]
fn locked_instances_cannot_be_moved_erased_or_exploded() {
    let mut m = Model::new();
    m.root = box_1m();
    let all: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    let inst = m.make_group(None, &all, true, None).unwrap();
    m.root.instances[inst].locked = true;
    let before = m.root.instances[inst].xform;
    m.root.transform_selection(
        &[EntityRef::Instance(inst)],
        &esq_core::glam::DMat4::from_translation(p(500.0, 0.0, 0.0)),
    );
    assert_eq!(m.root.instances[inst].xform, before, "not moved");
    m.root.erase(&[EntityRef::Instance(inst)]);
    assert!(m.root.instances.contains_key(inst), "not erased");
    assert!(m.explode(None, inst).is_empty(), "not exploded");
    m.root.instances[inst].locked = false;
    m.root.erase(&[EntityRef::Instance(inst)]);
    assert!(!m.root.instances.contains_key(inst));
}

#[test]
fn purge_removes_unused_materials_tags_and_definitions() {
    let mut m = Model::new();
    m.root = box_1m();
    let used = m.add_material("Bois", [180, 120, 60]);
    m.add_material("Inutile", [1, 2, 3]);
    m.layers.push(esq_core::model::Layer::new("Vide"));
    m.layers.push(esq_core::model::Layer::new("Murs"));
    let f = m.root.faces.keys().next().unwrap();
    m.root.faces[f].front = Some(used);
    m.root.faces[f].layer = 2;
    let mut other = Model::new();
    other.root = box_1m();
    m.import_as_component(&other, "Jamais placé");
    let n = m.purge_unused();
    assert_eq!(n, 3, "one material, one tag, one definition");
    assert_eq!(m.materials.len(), 1);
    assert_eq!(m.layers.len(), 2);
    assert_eq!(m.layers[m.root.faces[f].layer].name, "Murs", "tag indices are renumbered");
    assert!(m.defs.is_empty());
}

#[test]
fn section_plane_cuts_a_box_into_one_filled_loop() {
    use esq_core::model::SectionPlane;
    use esq_core::section::{chain_loops, clip_segment, fill_triangles};
    let mut m = Model::new();
    m.root = box_1m();
    let sec = m.root.sections.insert(SectionPlane::new(p(0.0, 0.0, 500.0), DVec3::Z));
    m.root.active_section = Some(sec);
    let segs = m.slice(None, sec);
    assert_eq!(segs.len(), 4, "{segs:?}");
    let loops = chain_loops(&segs);
    assert_eq!(loops.len(), 1);
    let plane = m.root.sections[sec].plane();
    let tris = fill_triangles(&plane, &loops);
    let area: f64 = tris.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).dot(DVec3::Z) * 0.5).sum();
    assert!((area - 1.0e6).abs() < 1e-3, "area {area}");
    // Points under the plane are cut away; a segment crossing it keeps its upper part.
    assert!(m.root.sections[sec].cuts(p(10.0, 10.0, 100.0)));
    let (a, b) = clip_segment(p(0.0, 0.0, 0.0), p(0.0, 0.0, 1000.0), &[m.root.sections[sec].clone()]).unwrap();
    assert!((a.z - 500.0).abs() < 1e-9 && (b.z - 1000.0).abs() < 1e-9);
}

#[test]
fn section_fill_leaves_holes_open() {
    use esq_core::math::Plane;
    use esq_core::section::fill_triangles;
    let sq = |s: f64| vec![p(-s, -s, 0.0), p(s, -s, 0.0), p(s, s, 0.0), p(-s, s, 0.0)];
    let tris = fill_triangles(&Plane::from_point_normal(DVec3::ZERO, DVec3::Z), &[sq(100.0), sq(50.0), sq(20.0)]);
    let area: f64 = tris.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).z * 0.5).sum();
    // 200² − 100² + 40²
    assert!((area - (40000.0 - 10000.0 + 1600.0)).abs() < 1e-6, "area {area}");
}

#[test]
fn group_from_slice_and_erasing_the_active_plane() {
    use esq_core::model::SectionPlane;
    let mut m = Model::new();
    m.root = box_1m();
    let sec = m.root.sections.insert(SectionPlane::new(p(500.0, 0.0, 0.0), DVec3::X));
    m.root.active_section = Some(sec);
    let g = m.group_from_slice(None, sec).unwrap();
    let def = m.root.instances[g].def;
    assert_eq!(m.defs[def].entities.edges.len(), 4);
    assert!(m.defs[def].entities.faces.is_empty());
    m.root.erase(&[EntityRef::Section(sec)]);
    assert!(m.root.active_section.is_none() && m.root.sections.is_empty());
}

fn two_boxes(offset: DVec3) -> (Model, esq_core::model::InstanceId, esq_core::model::InstanceId) {
    let mut m = Model::new();
    m.root = box_1m();
    let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    let a = m.make_group(None, &items, true, None).unwrap();
    let mut copy = m.root.instances[a].clone();
    copy.xform = esq_core::glam::DMat4::from_translation(offset) * copy.xform;
    let def = m.defs[copy.def].clone();
    copy.def = m.defs.insert(def);
    let b = m.root.instances.insert(copy);
    (m, a, b)
}

fn result_volume(m: &Model, i: esq_core::model::InstanceId) -> f64 {
    assert!(m.is_solid_instance(None, i), "result is a solid");
    m.solid_volume(None, i).unwrap()
}

#[test]
fn solid_tools_on_overlapping_cubes() {
    use esq_core::solid::SolidOp;
    let near = |a: f64, b: f64| (a - b).abs() < 1.0;
    let off = DVec3::new(500.0, 500.0, 500.0);
    let (m, a, _) = two_boxes(off);
    assert!(near(m.solid_volume(None, a).unwrap(), 1.0e9));
    for (op, want) in [
        (SolidOp::Union, vec![1.875e9]),
        (SolidOp::Intersect, vec![1.25e8]),
        (SolidOp::Subtract, vec![8.75e8]),
        (SolidOp::Split, vec![8.75e8, 8.75e8, 1.25e8]),
    ] {
        let (mut m, a, b) = two_boxes(off);
        let made = m.solid_op(None, a, b, op).unwrap();
        let got: Vec<f64> = made.iter().map(|i| result_volume(&m, *i)).collect();
        assert_eq!(got.len(), want.len(), "{op:?}");
        for (g, w) in got.iter().zip(&want) {
            assert!(near(*g, *w), "{op:?}: {got:?}");
        }
        assert_eq!(m.root.instances.len(), want.len(), "{op:?}: originals replaced");
    }
    let (mut m, a, b) = two_boxes(off);
    m.solid_op(None, a, b, SolidOp::Trim).unwrap();
    assert_eq!(m.root.instances.len(), 2, "trim keeps the first solid");
    assert!(m.root.instances.contains_key(a));
}

#[test]
fn solid_tools_with_coplanar_and_touching_faces() {
    use esq_core::solid::SolidOp;
    let near = |a: f64, b: f64| (a - b).abs() < 1.0;
    let (mut m, a, b) = two_boxes(DVec3::new(500.0, 0.0, 0.0));
    let u = m.solid_op(None, a, b, SolidOp::Union).unwrap()[0];
    assert!(near(result_volume(&m, u), 1.5e9));
    assert_eq!(m.defs[m.root.instances[u].def].entities.faces.len(), 6, "coplanar pieces merged");
    let (mut m, a, b) = two_boxes(DVec3::new(500.0, 0.0, 0.0));
    let i = m.solid_op(None, a, b, SolidOp::Intersect).unwrap()[0];
    assert!(near(result_volume(&m, i), 5.0e8));
    // Two cubes side by side: the wall between them goes away.
    let (mut m, a, b) = two_boxes(DVec3::new(1000.0, 0.0, 0.0));
    let s = m.outer_shell(None, &[a, b]).unwrap();
    assert!(near(result_volume(&m, s), 2.0e9));
    assert_eq!(m.defs[m.root.instances[s].def].entities.faces.len(), 6);
    assert_eq!(m.root.instances.len(), 1);
}

#[test]
fn subtracting_a_box_from_a_cylinder_keeps_it_smooth() {
    use esq_core::solid::SolidOp;
    let mut m = Model::new();
    let pts: Vec<DVec3> = (0..24)
        .map(|i| {
            let a = i as f64 / 24.0 * std::f64::consts::TAU;
            p(500.0 * a.cos(), 500.0 * a.sin(), 0.0)
        })
        .collect();
    m.root.add_polyline(&pts, true, true);
    let f = m.root.faces.keys().next().unwrap();
    m.root.push_pull(f, -1000.0, false);
    let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    let cyl = m.make_group(None, &items, true, None).unwrap();
    let full = m.solid_volume(None, cyl).unwrap();
    // A box taking the cylinder's top half.
    let mut b = Entities::default();
    rect(&mut b, -600.0, -600.0, 600.0, 600.0, 500.0);
    let bf = b.faces.keys().next().unwrap();
    b.push_pull(bf, -800.0, false);
    let items: Vec<EntityRef> = b.faces.keys().map(EntityRef::Face).collect();
    let mut tmp = Model::new();
    tmp.root = b;
    let g = tmp.make_group(None, &items, true, None).unwrap();
    let def = tmp.defs[tmp.root.instances[g].def].clone();
    let d = m.defs.insert(def);
    let mut inst = tmp.root.instances[g].clone();
    inst.def = d;
    let cutter = m.root.instances.insert(inst);
    let r = m.solid_op(None, cutter, cyl, SolidOp::Subtract).unwrap()[0];
    let v = result_volume(&m, r);
    assert!((v - full / 2.0).abs() < 1.0, "{v} vs {}", full / 2.0);
    let ents = &m.defs[m.root.instances[r].def].entities;
    assert_eq!(ents.edges.values().filter(|e| e.soft).count(), 24, "sides stay smooth");
}

#[test]
fn attributes_survive_esq_round_trip_and_old_files_still_load() {
    use esq_core::model::{AttrValue, EntityRef, remove_attr, set_attr};
    let mut m = Model::new();
    m.root.add_polyline(
        &[DVec3::ZERO, DVec3::new(100.0, 0.0, 0.0), DVec3::new(100.0, 100.0, 0.0), DVec3::new(0.0, 100.0, 0.0)],
        true,
        false,
    );
    let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    let inst = m.make_group(None, &items, false, Some("Dalle".into())).unwrap();
    let def = m.root.instances[inst].def;
    set_attr(&mut m.defs[def].attributes, "dynamic_attributes", "prix", AttrValue::Number(12.5));
    set_attr(&mut m.root.instances[inst].attributes, "fiche", "ref", AttrValue::Text("A-1".into()));
    set_attr(&mut m.root.instances[inst].attributes, "fiche", "ok", AttrValue::Bool(true));
    let path = std::env::temp_dir().join("esq_attrs_test.esq");
    esq_core::io::save_esq(&m, &path).unwrap();
    let back = esq_core::io::load_esq(&path).unwrap();
    let bi = back.root.instances.values().next().unwrap();
    assert_eq!(bi.attributes["fiche"]["ref"], AttrValue::Text("A-1".into()));
    assert_eq!(bi.attributes["fiche"]["ok"], AttrValue::Bool(true));
    assert_eq!(back.defs[bi.def].attributes["dynamic_attributes"]["prix"], AttrValue::Number(12.5));
    // A model without attributes does not write the field, and a file without it loads.
    let plain = Model::new();
    esq_core::io::save_esq(&plain, &path).unwrap();
    assert!(!std::fs::read_to_string(&path).unwrap().contains("attributes"));
    // Removing the last key removes the dictionary.
    let mut a = m.root.instances[inst].attributes.clone();
    remove_attr(&mut a, "fiche", "ref");
    remove_attr(&mut a, "fiche", "ok");
    assert!(a.is_empty());
    // Typing values.
    assert_eq!(AttrValue::parse(esq_core::model::AttrKind::Number, " 1,5 "), Some(AttrValue::Number(1.5)));
    assert_eq!(AttrValue::parse(esq_core::model::AttrKind::Number, "abc"), None);
    assert_eq!(AttrValue::parse(esq_core::model::AttrKind::Bool, "Oui"), Some(AttrValue::Bool(true)));
}

/// A positioned texture stays where it is: reversing the face takes each side's placement with
/// its material; the face pulled with Push/Pull keeps its placement (a placement does not change
/// along its frame's normal; a projected one stays where it is projected).
#[test]
fn reverse_and_push_pull_keep_the_texture_placement() {
    use esq_core::glam::DVec2;
    use esq_core::meta::{FaceTex, uv_from_pairs};
    let mut e = Entities::default();
    rect(&mut e, 0.0, 0.0, 1000.0, 600.0, 0.0);
    let f = e.faces.keys().next().unwrap();
    let n = e.faces[f].normal;
    let pairs = [(p(0.0, 0.0, 0.0), DVec2::new(0.2, 0.1)), (p(1000.0, 0.0, 0.0), DVec2::new(1.4, 0.6))];
    let map = uv_from_pairs(n, [300.0, 300.0], &pairs).unwrap();
    let projected = uv_from_pairs(p(0.3, -1.0, 0.5), [300.0, 300.0], &pairs[..1]).unwrap();
    e.faces[f].tex = Some(Box::new(FaceTex {
        front: Some(map),
        back: Some(projected),
        projection: [None, Some(p(0.3, -1.0, 0.5))],
        ..Default::default()
    }));
    e.reverse_face(f);
    let t = e.faces[f].tex.as_deref().unwrap();
    assert_eq!((t.front, t.back), (Some(projected), Some(map)), "chaque placement suit sa matière");
    assert!(t.projection[0].is_some() && t.projection[1].is_none());
    // Pulled up by 400 mm: the moved face shows the texture as the face did.
    let n = e.faces[f].normal;
    let cap = e.push_pull(f, 400.0 * n.z.signum(), false).unwrap();
    let t = e.faces[cap].tex.as_deref().expect("placement gardé");
    let q = p(250.0, 330.0, 0.0);
    assert!((t.back.unwrap().at(q + p(0.0, 0.0, 400.0)) - map.at(q)).length() < 1e-9);
    assert_eq!(t.front, Some(projected));
}

/// SketchUp's Reverse Face (`flip_face`): materials and placements stay on their
/// side; the turned plane's frame mirrors an unprojected placement (an identity placement on a
/// face facing down becomes the default one of a face facing up), a projected one is kept.
#[test]
fn reverse_face_as_sketchup_keeps_sides_and_mirrors_the_frame() {
    use esq_core::glam::DVec2;
    use esq_core::meta::{FaceTex, default_uv, uv_from_pairs};
    let mut e = Entities::default();
    rect(&mut e, 0.0, 0.0, 1000.0, 600.0, 0.0);
    let f = e.faces.keys().next().unwrap();
    assert!(e.faces[f].normal.z < 0.0);
    let dir = p(0.3, -1.0, 0.5);
    let projected = uv_from_pairs(dir, [300.0, 300.0], &[(p(0.0, 0.0, 0.0), DVec2::new(0.2, 0.1))]).unwrap();
    e.faces[f].tex = Some(Box::new(FaceTex {
        front: Some(default_uv(-esq_core::glam::DVec3::Z, [300.0, 300.0])),
        back: Some(projected),
        projection: [None, Some(dir)],
        ..Default::default()
    }));
    e.flip_face(f);
    let face = &e.faces[f];
    assert!(face.normal.z > 0.0);
    let t = face.tex.as_deref().unwrap();
    let up = default_uv(esq_core::glam::DVec3::Z, [300.0, 300.0]);
    for q in [p(10.0, 20.0, 0.0), p(700.0, 450.0, 0.0), p(-300.0, 90.0, 0.0)] {
        assert!((t.front.unwrap().at(q) - up.at(q)).length() < 1e-12, "{q:?}");
    }
    assert_eq!(t.back, Some(projected));
    assert_eq!(t.projection, [None, Some(dir)]);
}
