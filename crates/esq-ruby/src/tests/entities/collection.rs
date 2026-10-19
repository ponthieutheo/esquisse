use super::*;

#[test]
fn entities_enumeration() {
    let Some(r) = ev(&sq(r#"
         c = ents.add_cpoint([1,1,1])
         types = ents.map { |e| e.class.name.split('::').last }
         [ents.length, ents.size, types.tally, ents[0].class.name, ents.at(0) == ents[0], ents[-1] == c, ents[99], ents[-99], ents['x'],
          ents.each { }.equal?(ents), ents.each.class.name, ents.model == m, ents.parent == m, ents.count(&:itself),
          ents.grep(Sketchup::Face).size, ents.include?(f)]"#))
    else {
        return;
    };
    assert_eq!(
        r,
        "[6, 6, {\"Edge\"=>4, \"Face\"=>1, \"ConstructionPoint\"=>1}, \"Sketchup::Edge\", true, true, nil, nil, nil, true, \"Enumerator\", true, true, 6, 1, true]"
    );
}

#[test]
fn add_face_from_points_edges_and_curves() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         a = ents.add_face([0,0,0], [1,0,0], [1,1,0])
         b = ents.add_face([[10,0,0], [11,0,0], [11,1,0], [10,1,0]])
         es = ents.add_edges([20,0,0], [21,0,0], [21,1,0], [20,1,0], [20,0,0])
         c = ents.add_face(es)
         circle = ents.add_circle([30,0,0], [0,0,1], 2, 8)
         d = ents.add_face(circle[0].curve)
         v = ents.add_face(Sketchup.active_model.entities.add_edges([40,0,0], [41,0,0], [41,1,0], [40,0,0]).map(&:start))
         [a.vertices.size, b.vertices.size, c.valid?, c.vertices.size, d.vertices.size, ents.grep(Sketchup::Face).size, a.class.name]"#)
    else {
        return;
    };
    assert_eq!(r, "[3, 4, true, 4, 8, 5, \"Sketchup::Face\"]");
}

#[test]
fn add_face_errors() {
    assert_eq!(
        raised("Sketchup.active_model.entities.add_face([0,0,0], [1,0,0])").unwrap(),
        "ArgumentError: Not enough edges - at least 3 required"
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.add_face").unwrap(),
        "ArgumentError: Not enough edges - at least 3 required"
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.add_face([0,0,0], [1,0,0], [1,0,0], [0,1,0])").unwrap(),
        "ArgumentError: Duplicate points in array"
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.add_face([0,0,0], [1,0,0], [1,1,0], [0,1,1])").unwrap(),
        "ArgumentError: Points are not planar"
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.add_face([0,0,0], [1,0,0], [2,0,0])").unwrap(),
        "ArgumentError: Points are not planar"
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.add_face([0,0,0], [1,0,0], 'x')").unwrap(),
        "ArgumentError: Cannot convert argument to Geom::Point3d"
    );
}

#[test]
fn add_group_and_instance() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model; ents = m.entities
         f = ents.add_face([0,0,0], [1,0,0], [1,1,0])
         g = ents.add_group
         g2 = ents.add_group(f)
         defn = m.definitions.add('Boîte')
         defn.entities.add_face([0,0,0], [1,0,0], [1,1,0])
         inst = ents.add_instance(defn, Geom::Transformation.translation([5,0,0]))
         [g.class.name, g2.class.name, g2.entities.grep(Sketchup::Face).size, f.valid?, inst.class.name, inst.definition == defn,
          inst.transformation.origin.to_a, ents.grep(Sketchup::Group).size]"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[\"Sketchup::Group\", \"Sketchup::Group\", 1, false, \"Sketchup::ComponentInstance\", true, [5.0, 0.0, 0.0], 2]"
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.add_instance(5, Geom::Transformation.new)").unwrap(),
        "ArgumentError: Invalid component definition"
    );
    assert_eq!(
        raised("m = Sketchup.active_model; d = m.definitions.add('x'); m.entities.add_instance(d, Geom::Transformation.new([0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,1]))").unwrap(),
        "ArgumentError: Unable to add instance: transformation matrix is not invertible"
    );
    assert_eq!(
        raised("m = Sketchup.active_model; d = m.definitions.add('x'); d.entities.add_instance(d, Geom::Transformation.new)").unwrap(),
        "ArgumentError: Insertion would result in recursive definition."
    );
}

#[test]
fn erase_and_clear() {
    let Some(r) = ev(&sq(r#"
         e = ents.grep(Sketchup::Edge).first
         cp = ents.add_cpoint([1,1,1])
         t = ents.add_text('x', [0,0,0], [1,1,1])
         a = [ents.erase_entities(cp, t), cp.valid?, t.valid?, ents.length]
         a << ents.erase_entities([e])
         a += [e.valid?, f.valid?, ents.length]
         a << ents.clear!.equal?(ents)
         a + [ents.length, ents.clear!.equal?(ents)]"#))
    else {
        return;
    };
    assert_eq!(r, "[nil, false, false, 5, nil, false, false, 3, true, 0, true]");
    assert_eq!(
        raised("Sketchup.active_model.entities.erase_entities").unwrap(),
        "ArgumentError: Too few arguments - at least 1 required"
    );
    assert_eq!(
        raised(&sq("e = ents.grep(Sketchup::Edge).first; e.erase!; ents.erase_entities(e)")).unwrap(),
        "TypeError: reference to deleted Entity"
    );
}

#[test]
fn erasing_a_curve_removes_its_edges() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         es = ents.add_circle([0,0,0], [0,0,1], 3, 8)
         ents.erase_entities(es[0].curve)
         ents.grep(Sketchup::Edge).size"#)
    else {
        return;
    };
    assert_eq!(r, "0");
}

#[test]
fn transform_entities_moves_geometry_and_instances() {
    let Some(r) = ev(&sq(r#"
         g = ents.add_group
         ok = ents.transform_entities(Geom::Transformation.translation([5, 0, 0]), f)
         ok2 = ents.transform_entities(Geom::Transformation.translation([0, 0, 2]), [g])
         [ok, ok2, f.vertices.map { |v| v.position.x }.min, g.transformation.origin.z,
          ents.transform_entities(Geom::Transformation.new, ents.grep(Sketchup::Edge))]"#))
    else {
        return;
    };
    assert_eq!(r, "[true, true, 5.0, 2.0, true]");
    assert_eq!(
        raised(&sq("ents.transform_entities(Geom::Transformation.new)")).unwrap(),
        "ArgumentError: Too few arguments - at least 2 required"
    );
    assert_eq!(
        raised(&sq("ents.transform_entities(Geom::Transformation.new([0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,1]), f)"))
            .unwrap(),
        "ArgumentError: Unable to apply transformation: transformation matrix is not invertible"
    );
    assert_eq!(
        raised(&sq("ents.transform_entities(Geom::Transformation.new, 5)")).unwrap(),
        "TypeError: wrong argument type (expected Sketchup::Entity)"
    );
}

#[test]
fn transform_by_vectors_moves_each_entity() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         a = ents.add_group; b = ents.add_group
         r = ents.transform_by_vectors([a, b], [Geom::Vector3d.new(1, 0, 0), Geom::Vector3d.new(0, 2, 0)])
         [r.equal?(ents), a.transformation.origin.to_a, b.transformation.origin.to_a]"#)
    else {
        return;
    };
    assert_eq!(r, "[true, [1.0, 0.0, 0.0], [0.0, 2.0, 0.0]]");
    assert_eq!(
        raised("e = Sketchup.active_model.entities; a = e.add_group; b = e.add_group; e.transform_by_vectors([a, b], [Geom::Vector3d.new(1, 0, 0)])").unwrap(),
        "ArgumentError: second array must be at least as large as the first"
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.transform_by_vectors([])").unwrap(),
        "ArgumentError: Too few arguments - at least 2 required"
    );
}

#[test]
fn build_yields_a_builder_valid_only_inside_the_block() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         seen = nil
         ret = ents.build do |b|
           seen = b
           f = b.add_face([0,0,0], [4,0,0], [4,4,0], [0,4,0])
           e = b.add_edge([10,0,0], [11,0,0])
           es = b.add_edges([20,0,0], [21,0,0], [22,1,0])
           v = b.vertex_at([30,30,30])
           [b.valid?, b.entities == ents, f.class.name, e.class.name, es.size, v.class.name, b.respond_to?(:add_line)]
           seen = [seen, [b.valid?, b.entities == ents, f.class.name, e.class.name, es.size, v.class.name]]
         end
         [ret, seen[1], seen[0].valid?, ents.grep(Sketchup::Face).size, ents.grep(Sketchup::Edge).size]"#)
    else {
        return;
    };
    assert_eq!(r, "[nil, [true, true, \"Sketchup::Face\", \"Sketchup::Edge\", 2, \"NilClass\"], false, 1, 7]");
    assert_eq!(
        raised("ents = Sketchup.active_model.entities; b = nil; ents.build { |x| b = x }; b.add_face([0,0,0],[1,0,0],[1,1,0])").unwrap(),
        "RuntimeError: Invalid Sketchup::EntitiesBuilder object."
    );
    assert_eq!(
        raised("ents = Sketchup.active_model.entities; ents.build { |x| ents.build { } }").unwrap(),
        "RuntimeError: Cannot nest calls to entities.build"
    );
    assert_eq!(
        raised("ents = Sketchup.active_model.entities; ents.build { |x| x.clone }").unwrap(),
        "TypeError: Cannot duplicate or clone Sketchup::EntitiesBuilder."
    );
    assert_eq!(
        raised("ents = Sketchup.active_model.entities; ents.build { |x| x.add_face([0,0,0],[1,0,0]) }").unwrap(),
        "ArgumentError: Not enough points in loop require at least 3"
    );
}

#[test]
fn faces_from_a_polygon_mesh() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model; ents = m.entities
         mesh = Geom::PolygonMesh.new
         a = mesh.add_point([0,0,0]); b = mesh.add_point([10,0,0]); c = mesh.add_point([10,10,0]); d = mesh.add_point([0,10,0])
         mesh.add_polygon(a, b, c)
         mesh.add_polygon(a, c, d)
         n = ents.add_faces_from_mesh(mesh)
         [n, ents.grep(Sketchup::Face).size, ents.grep(Sketchup::Edge).size]"#)
    else {
        return;
    };
    assert_eq!(r, "[2, 2, 5]");
}

#[test]
fn faces_from_a_mesh_with_flags_and_materials() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model; ents = m.entities
         m.materials.add('Rouge'); m.materials.add('Bleu')
         mesh = Geom::PolygonMesh.new
         a = mesh.add_point([0,0,0]); b = mesh.add_point([10,0,0]); c = mesh.add_point([10,10,0]); d = mesh.add_point([0,10,0])
         mesh.add_polygon(a, b, c)
         mesh.add_polygon(-a, c, d)
         ents.fill_from_mesh(mesh, true, 1 | 2, 'Rouge', 'Bleu')
         diag = ents.grep(Sketchup::Edge).find { |e| e.faces.size == 2 }
         fs = ents.grep(Sketchup::Face)
         [fs.size, diag.hidden?, diag.soft?, diag.smooth?, fs.map { |f| f.material.name }.uniq, fs.map { |f| f.back_material.name }.uniq]"#)
    else {
        return;
    };
    assert_eq!(r, "[2, true, false, false, [\"Rouge\"], [\"Bleu\"]]");
    assert_eq!(
        raised("Sketchup.active_model.entities.add_faces_from_mesh").unwrap(),
        "ArgumentError: Too few arguments - 1 to 4 required"
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.fill_from_mesh").unwrap(),
        "ArgumentError: Too few arguments - 1 to 5 required"
    );
}

#[test]
fn intersect_with_cuts_faces() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         a = ents.add_face([0,0,0], [10,0,0], [10,10,0], [0,10,0])
         b = ents.add_face([5,-5,-5], [5,15,-5], [5,15,5], [5,-5,5])
         r = ents.intersect_with(false, Geom::Transformation.new, ents, Geom::Transformation.new, true, [a, b])
         [r.class.name, r.all? { |x| x.is_a?(Sketchup::Edge) }, r.empty?, ents.grep(Sketchup::Edge).any? { |e| e.line[1].to_a.map(&:abs) == [0.0, 1.0, 0.0] && (e.start.position.x - 5).abs < 1e-6 && (e.end.position.x - 5).abs < 1e-6 }]"#)
    else {
        return;
    };
    assert_eq!(r, "[\"Array\", true, false, true]");
    assert_eq!(
        raised("Sketchup.active_model.entities.intersect_with(false)").unwrap(),
        "ArgumentError: Too few arguments - at least 6 required"
    );
}

#[test]
fn entities_observers_and_frozen_collections() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         o = Object.new
         [ents.add_observer(o), ents.remove_observer(o), ents.remove_observer(o)]"#)
    else {
        return;
    };
    assert_eq!(r, "[true, true, false]");
    let msg = raised("ents = Sketchup::Entities.new('0').freeze; ents.add_line([0,0,0], [1,0,0])").unwrap();
    assert!(msg.starts_with("FrozenError: can't modify frozen Sketchup::Entities"), "{msg}");
}

#[test]
fn snaps_spaces_and_3d_text_arguments() {
    assert_eq!(
        raised("Sketchup.active_model.entities.add_3d_text('a', 0)").unwrap(),
        "ArgumentError: Need at 3 arguments"
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.add_snap([0,0,0])").unwrap(),
        "ArgumentError: wrong number of arguments (given 1, expected 2..3)"
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.add_space('x', [[0,0,0],[1,0,0]], [0,0,0], 96)").unwrap(),
        "ArgumentError: Perimeter points must have at least 3 points."
    );
    let Some(r) = ev(r#"
         s = Sketchup.active_model.entities.add_snap([1,2,3], [2,0,0])
         [s.class.name, s.position.to_a, s.direction.to_a, s.up.to_a]"#)
    else {
        return;
    };
    assert_eq!(r, "[\"Sketchup::Snap\", [1.0, 2.0, 3.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]");
}
