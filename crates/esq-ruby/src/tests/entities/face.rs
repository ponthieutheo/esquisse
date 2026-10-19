use super::*;

#[test]
fn face_geometry_queries() {
    let Some(r) = ev(&sq(r#"
         [f.vertices.map { |v| v.position.to_a }, f.edges.size, f.normal.to_a, f.plane, f.area, f.area(Geom::Transformation.scaling(2)),
          f.outer_loop.class.name, f.loops.size, f.back_material, f.material, f.reverse!.equal?(f), f.normal.to_a, f.plane]"#))
    else {
        return;
    };
    assert_eq!(
        r,
        "[[[0.0, 0.0, 0.0], [0.0, 10.0, 0.0], [10.0, 10.0, 0.0], [10.0, 0.0, 0.0]], 4, [0.0, 0.0, -1.0], [0.0, 0.0, -1.0, 0.0], 100.0, 400.0, \"Sketchup::Loop\", 1, nil, nil, true, [0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 0.0]]"
    );
}

#[test]
fn ground_faces_face_down_others_follow_the_points() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         g1 = ents.add_face([0,0,0], [1,0,0], [1,1,0])
         g2 = ents.add_face([5,1,0], [6,1,0], [6,0,0])
         up = ents.add_face([0,0,5], [1,0,5], [1,1,5])
         down = ents.add_face([0,0,9], [1,1,9], [1,0,9])
         [g1.normal.z, g2.normal.z, up.normal.z, down.normal.z, g1.vertices[0].position.to_a]"#)
    else {
        return;
    };
    assert_eq!(r, "[-1.0, -1.0, 1.0, -1.0, [0.0, 0.0, 0.0]]");
}

#[test]
fn face_with_a_hole_has_two_loops() {
    let Some(r) = ev(&sq(r#"
         ents.add_edges([2,2,0], [4,2,0], [4,4,0], [2,4,0], [2,2,0])
         big = ents.grep(Sketchup::Face).max_by(&:area)
         [ents.grep(Sketchup::Face).size, big.loops.size, big.outer_loop.outer?, big.loops[1].outer?, big.area.round(6),
          big.vertices.size, big.edges.size, big.loops[1].vertices.size, big.loops.map { |l| l.face == big }.uniq]"#))
    else {
        return;
    };
    assert_eq!(r, "[2, 2, true, false, 96.0, 8, 8, 4, [true]]");
}

#[test]
fn face_pushpull_extrudes_and_returns_nil() {
    let Some((o, m)) = must(&sq(r#"
         a = f.pushpull(5)
         b = f.pushpull(0)
         [a, b, f.valid?, f.vertices.map { |v| v.position.z }.uniq, ents.grep(Sketchup::Face).size]"#))
    else {
        return;
    };
    assert_eq!(o.result, "[nil, nil, true, [-5.0], 6]");
    assert_eq!(m.root.faces.len(), 6);
    assert_eq!(raised(&sq("f.pushpull")).unwrap(), "ArgumentError: At least one argument (pushpull distance) required");
}

/// With a copy, the face stays the base, facing as it did (the API keeps it: TestUp
/// `test_pushpull_copy_face_direction`); the distance is a number (rb_num2dbl).
#[test]
fn face_pushpull_with_copy_keeps_the_base() {
    let Some(r) = ev(&sq(r#"
         z0 = f.vertices.map { |v| v.position.z }.uniq
         f.pushpull(5, true)
         [z0, f.vertices.map { |v| v.position.z }.uniq, ents.grep(Sketchup::Face).size, f.normal.to_a]"#))
    else {
        return;
    };
    assert_eq!(r, "[[0.0], [0.0], 6, [0.0, 0.0, -1.0]]");
    assert_eq!(raised(&sq("f.pushpull('st')")).unwrap(), "TypeError: no implicit conversion to float from string");
    assert_eq!(raised(&sq("f.pushpull(nil, nil)")).unwrap(), "TypeError: no implicit conversion to float from nil");
}

/// A face added inside another of its plane gives it a hole, as SketchUp's `add_face` does
/// (TestUp `create_face_with_hole`: two loops, a mesh of eight triangles).
#[test]
fn a_face_added_inside_another_cuts_a_hole() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         big = ents.add_face([-100, -100, 0], [100, -100, 0], [100, 100, 0], [-100, 100, 0])
         small = ents.add_face([0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0])
         [big.valid?, big.loops.size, small.loops.size, big.mesh(0).count_polygons, big.mesh(0).count_points,
          ents.grep(Sketchup::Face).size, big.area.round(6), small.normal.to_a]"#)
    else {
        return;
    };
    assert_eq!(r, "[true, 2, 1, 8, 8, 2, 39900.0, [0.0, 0.0, -1.0]]");
}

/// `followme` takes edges, or arrays of edges only.
#[test]
fn followme_refuses_arrays_of_other_things() {
    assert_eq!(
        raised(&sq("f.followme([\"Can't use array\"])")).unwrap(),
        "TypeError: wrong argument type (expected Sketchup::Edge)"
    );
}

#[test]
fn face_classify_point() {
    let Some(r) = ev(&sq(r#"
         [[5,5,0], [0,0,0], [5,0,0], [20,5,0], [5,5,1], [10,10,0], [0.0005,5,0]].map { |p| f.classify_point(p) }
         .push(Sketchup::Face::PointInside, Sketchup::Face::PointOnVertex, Sketchup::Face::PointOnEdge, Sketchup::Face::PointOutside,
               Sketchup::Face::PointNotOnPlane, Sketchup::Face::PointUnknown, Sketchup::Face::PointOnFace)"#))
    else {
        return;
    };
    assert_eq!(r, "[1, 2, 4, 16, 32, 2, 4, 1, 2, 4, 16, 32, 0, 8]");
}

#[test]
fn face_coplanar_with() {
    let Some(r) = ev(&sq(r#"
         g = ents.add_face([20,0,0], [30,0,0], [30,10,0])
         h = ents.add_face([0,0,5], [1,0,5], [1,1,5])
         w = ents.add_face([0,0,0], [0,0,5], [0,5,5])
         [f.coplanar_with?(g), f.coplanar_with?(h), f.coplanar_with?(w), f.coplanar_with?(5), f.coplanar_with?(nil)]"#))
    else {
        return;
    };
    assert_eq!(r, "[true, false, false, nil, nil]");
}

#[test]
fn face_mesh_is_a_triangulation_in_local_coordinates() {
    let Some(r) = ev(&sq(r#"
         pm = f.mesh
         tris = pm.polygons
         area = tris.sum do |t|
           a, b, c = t.map { |i| pm.points[i.abs - 1] }
           a.vector_to(b).cross(a.vector_to(c)).length / 2.0
         end
         [pm.points.size, tris.size, area.round(6), tris.flatten.any?(&:negative?)]"#))
    else {
        return;
    };
    assert_eq!(r, "[4, 2, 100.0, true]");
}

#[test]
fn face_followme_sweeps_the_profile() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         f = ents.add_face([0,0,0], [1,0,0], [1,1,0], [0,1,0])
         path = ents.add_edges([0.5,0.5,0], [0.5,0.5,10])
         r = f.followme(path)
         [r, ents.grep(Sketchup::Face).size]"#)
    else {
        return;
    };
    assert_eq!(r, "[true, 6]");
    assert_eq!(raised(&sq("f.followme")).unwrap(), "ArgumentError: Too few arguments - at least one Edge required");
}

#[test]
fn face_texture_methods() {
    let Some(r) = ev(&sq(r#"
         [f.texture_positioned?(true), f.texture_projected?(false), f.get_texture_projection(true), f.clear_texture_position(true),
          f.clear_texture_projection(false), f.get_glued_instances, f.uv_tile_at([1,1,0], true),
          %i[texture_positioned? texture_projected? get_texture_projection clear_texture_position clear_texture_projection
             uv_tile_at position_material get_UVHelper mesh].map { |s| Sketchup::Face.instance_method(s).arity }]"#))
    else {
        return;
    };
    // Arities of SketchUp.exe's `rb_define_method`.
    assert_eq!(r, "[false, false, nil, nil, nil, [], nil, [1, 1, 1, 1, 1, 2, -1, -1, -1]]");
    assert_eq!(
        raised(&sq("f.set_texture_projection([0,0,1], true)")).unwrap(),
        "NotImplementedError: This method never worked properly and was removed in SketchUp 2021.1. Use face.position_material with the projection vector instead."
    );
}

#[test]
fn face_back_material_and_materials() {
    let Some(r) = ev(&sq(r#"
         m.materials.add('Rouge'); m.materials.add('Bleu')
         f.material = 'Rouge'; f.back_material = 'Bleu'
         a = [f.material.name, f.back_material.name]
         f.reverse!
         a + [f.material.name, f.back_material.name]"#))
    else {
        return;
    };
    // Reversed, the face keeps its materials on their side (SUFaceReverse).
    assert_eq!(r, "[\"Rouge\", \"Bleu\", \"Rouge\", \"Bleu\"]");
}

#[test]
fn vertex_queries() {
    let Some(r) = ev(&sq(r#"
         v = f.vertices[0]
         w = f.vertices[2]
         e = v.edges.first
         far = ents.add_line([30,0,0], [31,0,0])
         [v.edges.size, v.faces.size, v.loops.size, v.position.to_a, v.common_edge(f.vertices[1]).class.name, v.common_edge(w),
          v.used_by?(f), v.used_by?(e), v.used_by?(far), v.curve_interior?, v == f.vertices[0], v.entityID == f.vertices[0].entityID]"#))
    else {
        return;
    };
    assert_eq!(r, "[2, 1, 1, [0.0, 0.0, 0.0], \"Sketchup::Edge\", nil, true, true, false, nil, true, true]");
    assert_eq!(raised(&sq("f.vertices[0].used_by?(5)")).unwrap(), "ArgumentError: Edge or Face required");
    assert_eq!(
        raised(&sq("f.vertices[0].common_edge(5)")).unwrap(),
        "TypeError: wrong argument type (expected Sketchup::Vertex)"
    );
}

#[test]
fn loops_and_edge_uses() {
    let Some(r) = ev(&sq(r#"
         l = f.outer_loop
         eus = l.edgeuses
         eu = eus[0]
         [l.vertices.size, l.edges.size, eus.size, l.face == f, l.outer?, l.convex?, eu.edge.class.name, eu.face == f, eu.loop == l,
          eu.next.edge == eus[1].edge, eu.previous.edge == eus[3].edge, eu.next.next.next.next.edge == eu.edge,
          eu.reversed?.class.name, eu.partners, eu.start_vertex_normal.to_a, eu.end_vertex_normal.to_a,
          eu.typename, l.typename, eu.valid?]"#))
    else {
        return;
    };
    assert_eq!(
        r,
        "[4, 4, 4, true, true, true, \"Sketchup::Edge\", true, true, true, true, true, \"TrueClass\", [], [0.0, 0.0, -1.0], [0.0, 0.0, -1.0], \"EdgeUse\", \"Loop\", true]"
    );
    let r = ev(&sq(r#"
         g = ents.add_face([0,0,0], [0,10,0], [-10,10,0], [-10, 0, 0])
         shared = f.edges.find { |e| e.faces.size == 2 }
         eu = f.outer_loop.edgeuses.find { |x| x.edge == shared }
         [eu.partners.size, eu.partners[0].face == g, eu.partners[0].edge == shared, eu.reversed? != eu.partners[0].reversed?]"#))
    .unwrap();
    assert_eq!(r, "[1, true, true, true]");
}

#[test]
fn concave_loop_is_not_convex() {
    let Some(r) = ev(r#"
         f = Sketchup.active_model.entities.add_face([0,0,0], [10,0,0], [10,10,0], [5,2,0], [0,10,0])
         f.outer_loop.convex?"#)
    else {
        return;
    };
    assert_eq!(r, "false");
}

#[test]
fn smooth_edges_average_vertex_normals() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         a = ents.add_face([0,0,0], [10,0,0], [10,10,0], [0,10,0])
         b = ents.add_face([10,0,0], [10,0,10], [10,10,10], [10,10,0])
         shared = a.edges.find { |e| e.faces.size == 2 }
         eu = a.outer_loop.edgeuses.find { |x| x.edge == shared }
         flat = eu.start_vertex_normal.to_a.map { |v| v.round(4) }
         shared.smooth = true
         sm = eu.start_vertex_normal.to_a.map { |v| v.round(4) }
         [flat, sm]"#)
    else {
        return;
    };
    assert_eq!(r, "[[0.0, 0.0, -1.0], [-0.7071, 0.0, -0.7071]]");
}
