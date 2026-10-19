use super::*;

mod annot;
mod collection;
mod curve;
mod face;
mod image;

/// Runs `code` and returns the `inspect` of its last expression.
fn ev(code: &str) -> Option<String> {
    must(code).map(|(o, _)| o.result)
}

/// The exception class and message raised by `code`, as "Class: message".
fn raised(code: &str) -> Option<String> {
    let wrapped = format!("begin\n{code}\n'rien'\nrescue Exception => e\n \"#{{e.class}}: #{{e.message}}\"\nend");
    ev(&wrapped).map(|s| s.trim_matches('"').replace("\\\"", "\"").replace("\\n", "\n"))
}

/// A 10 x 10 face in the root, then `code`.
fn sq(code: &str) -> String {
    format!(
        "m = Sketchup.active_model; ents = m.entities\n f = ents.add_face([0,0,0], [10,0,0], [10,10,0], [0,10,0])\n{code}"
    )
}

#[test]
fn entity_identity_and_deleted_references() {
    let Some(r) = ev(&sq(r#"
         e = ents.grep(Sketchup::Edge).first
         id = e.entityID
         a = [e.valid?, e.deleted?, e.typename, f.typename, e.model == m, e.parent == m, id.is_a?(Integer), id == e.persistent_id,
              e.inspect =~ /\A#<Sketchup::Edge:0x\h+>\z/ ? true : false, f.entityID != e.entityID]
         f.erase!
         e.erase!
         a + [e.valid?, e.deleted?, e.inspect =~ /\A#<Deleted Entity:0x\h+>\z/ ? true : false]"#))
    else {
        return;
    };
    assert_eq!(r, "[true, false, \"Edge\", \"Face\", true, true, true, true, true, true, false, true, true]");
    for call in ["e.start", "e.typename", "e.entityID", "e.length", "e.faces", "e.parent", "e.hidden?"] {
        let code = sq(&format!("e = ents.grep(Sketchup::Edge).first\n e.erase!\n {call}"));
        assert_eq!(raised(&code).unwrap(), "TypeError: reference to deleted Edge", "{call}");
    }
    assert_eq!(raised(&sq("f.erase!\n f.normal")).unwrap(), "TypeError: reference to deleted Face");
}

#[test]
fn entity_parent_is_the_container() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model
         g = m.entities.add_group
         f = g.entities.add_face([0,0,0], [1,0,0], [1,1,0])
         e = m.entities.add_line([5,5,5], [6,6,6])
         [f.parent == g.definition, f.parent.class.name, e.parent.class.name, g.entities.parent == g.definition]"#)
    else {
        return;
    };
    assert_eq!(r, "[true, \"Sketchup::ComponentDefinition\", \"Sketchup::Model\", true]");
}

#[test]
fn entity_observers_register_and_unregister() {
    let Some(r) = ev(r#"
         e = Sketchup.active_model.entities.add_line([0,0,0], [1,0,0])
         o = Object.new
         [e.add_observer(o), e.add_observer(o), e.remove_observer(o), e.remove_observer(o)]"#)
    else {
        return;
    };
    assert_eq!(r, "[true, true, true, false]");
}

#[test]
fn entity_attributes_roundtrip() {
    let Some(r) = ev(r#"
         e = Sketchup.active_model.entities.add_line([0,0,0], [1,0,0])
         a = [e.attribute_dictionaries, e.get_attribute('d', 'k'), e.get_attribute('d', 'k', 5)]
         e.set_attribute('d', 'k', 42)
         a + [e.get_attribute('d', 'k'), e.attribute_dictionary('d').name, e.attribute_dictionary('zz'), e.attribute_dictionaries.size,
              e.delete_attribute('d', 'k'), e.get_attribute('d', 'k', :gone)]"#)
    else {
        return;
    };
    assert_eq!(r, "[nil, nil, 5, 42, \"d\", nil, 1, true, :gone]");
    assert_eq!(
        raised("Sketchup.active_model.entities.add_line([0,0,0],[1,0,0]).get_attribute('')").unwrap(),
        "ArgumentError: Dictionary name and key required"
    );
}

#[test]
fn drawingelement_flags_layer_material() {
    let Some(r) = ev(&sq(r#"
         e = ents.grep(Sketchup::Edge).first
         m.layers.add('Murs'); m.materials.add('Brique')
         out = [e.hidden?, e.visible?, e.casts_shadows?, e.receives_shadows?, e.layer.name, e.material]
         e.hidden = true; e.casts_shadows = false; e.receives_shadows = false
         e.layer = 'Murs'; e.material = 'Brique'
         out += [e.hidden?, e.visible?, e.casts_shadows?, e.receives_shadows?, e.layer.name, e.material.name]
         out += [(begin; e.layer = 'inconnue'; rescue ArgumentError => x; x.message; end), e.layer.name]
         e.visible = true
         out + [e.hidden?, e.erase!, e.valid?]"#))
    else {
        return;
    };
    assert_eq!(
        r,
        "[false, true, true, true, \"Layer0\", nil, true, false, false, false, \"Murs\", \"Brique\", \"Cannot find tag named \\\"inconnue\\\"\", \"Murs\", false, nil, false]"
    );
}

#[test]
fn drawingelement_material_accepts_color_and_names() {
    let Some(r) = ev(&sq(r#"
         f.material = Sketchup::Color.new(255, 0, 0)
         a = f.material.name
         f.material = 'blue'
         b = [f.material.name, f.material.color.blue]
         f.material = nil
         [a, b, f.material, f.back_material]"#))
    else {
        return;
    };
    assert_eq!(r, "[\"Color_FF0000\", [\"blue\", 255], nil, nil]");
    // Le message de SketchUp.
    assert_eq!(
        raised(&sq("f.material = 'pas-une-matiere'")).unwrap(),
        "ArgumentError: Cannot find material named pas-une-matiere"
    );
}

#[test]
fn bounds_of_edges_faces_and_construction_geometry() {
    let Some(r) = ev(&sq(r#"
         e = ents.add_line([0,0,0], [0,0,7])
         cp = ents.add_cpoint([3,4,5])
         b = f.bounds
         [b.min.to_a, b.max.to_a, e.bounds.max.to_a, cp.bounds.min.to_a]"#))
    else {
        return;
    };
    assert_eq!(r, "[[0.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 0.0, 7.0], [3.0, 4.0, 5.0]]");
}

#[test]
fn edge_basics() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model; ents = m.entities
         e = ents.add_line([0,0,0], [3,4,0])
         s, t = e.start, e.end
         [e.length, e.length(Geom::Transformation.scaling(2)), e.line[0].to_a, e.line[1].to_a.map { |v| v.round(6) },
          s.position.to_a, t.position.to_a, e.vertices.size, e.other_vertex(s) == t, e.other_vertex(t) == s,
          e.smooth?, e.soft?, e.faces, e.curve, e.used_by?(s), e.used_by?(t), e.explode_curve.equal?(e)]"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[5.0, 10.0, [0.0, 0.0, 0.0], [0.6, 0.8, 0.0], [0.0, 0.0, 0.0], [3.0, 4.0, 0.0], 2, true, true, false, false, [], nil, true, true, true]"
    );
    let r = ev(r#"
         ents = Sketchup.active_model.entities
         e = ents.add_line([0,0,0], [3,4,0]); f = ents.add_line([0,0,0], [0,5,0])
         o = ents.add_line([8,8,8], [9,9,9])
         [e.other_vertex(o.start), e.used_by?(o.start), e.used_by?(5), e.common_face(f)]"#)
    .unwrap();
    assert_eq!(r, "[nil, false, false, nil]");
    assert_eq!(
        raised("e = Sketchup.active_model.entities.add_line([0,0,0],[1,0,0]); e.other_vertex(3)").unwrap(),
        "TypeError: wrong argument type (expected Sketchup::Vertex)"
    );
    assert_eq!(
        raised("e = Sketchup.active_model.entities.add_line([0,0,0],[1,0,0]); e.common_face(e.start)").unwrap(),
        "TypeError: wrong argument type (expected Sketchup::Edge)"
    );
}

#[test]
fn edge_smooth_soft_setters() {
    let Some(r) = ev(r#"
         e = Sketchup.active_model.entities.add_line([0,0,0], [3,4,0])
         e.smooth = true; e.soft = true
         a = [e.smooth?, e.soft?]
         e.smooth = nil; e.soft = false
         a + [e.smooth?, e.soft?]"#)
    else {
        return;
    };
    assert_eq!(r, "[true, true, false, false]");
}

#[test]
fn edge_split_by_fraction_and_point() {
    let Some(r) = ev(&sq(r#"
         e = ents.grep(Sketchup::Edge).find { |x| x.line[1].to_a == [1.0, 0.0, 0.0] && x.start.position.y == 0 }
         n = e.split(0.25)
         [e.valid?, n.class.name, e.length.round(6), n.length.round(6), e.end.position.to_a, n.start.position.to_a,
          e.end == n.start, f.valid?, f.edges.size, f.vertices.size, e.faces.size, n.faces.size,
          e.split(0), e.split(1), e.split(1.5), e.split(-1), e.split(Geom::Point3d.new(0, 0, 0)),
          (n.split(Geom::Point3d.new(5, 0, 0)) ? n.length.round(6) : nil)]"#))
    else {
        return;
    };
    assert_eq!(
        r,
        "[true, \"Sketchup::Edge\", 2.5, 7.5, [2.5, 0.0, 0.0], [2.5, 0.0, 0.0], true, true, 5, 5, 1, 1, nil, nil, nil, nil, nil, 2.5]"
    );
}

#[test]
fn edge_faces_reversed_in_and_common_face() {
    let Some(r) = ev(&sq(r#"
         o = ents.add_line([20,0,0], [30,0,0])
         shared = ents.add_face([0,0,0], [0,10,0], [-10,10,0], [-10, 0, 0])
         common = f.edges.find { |e| e.faces.size == 2 }
         [o.reversed_in?(f), common.common_face(common) == common.faces[0],
          common.faces.map(&:class).uniq.map(&:name), common.reversed_in?(f) != common.reversed_in?(shared),
          common.faces.size]"#))
    else {
        return;
    };
    assert_eq!(r, "[nil, true, [\"Sketchup::Face\"], true, 2]");
    assert_eq!(
        raised(&sq("f.edges[0].reversed_in?(3)")).unwrap(),
        "TypeError: wrong argument type (expected Sketchup::Face)"
    );
}

#[test]
fn edge_find_faces_creates_missing_faces() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         a = ents.add_line([0,0,0], [10,0,0]); b = ents.add_line([10,0,0], [10,10,0]); c = ents.add_line([10,10,0], [0,0,0])
         [ents.grep(Sketchup::Face).size, a.find_faces, ents.grep(Sketchup::Face).size, a.find_faces]"#)
    else {
        return;
    };
    // Like SketchUp, add_line makes no face out of loose edges: find_faces does, once.
    assert_eq!(r, "[0, 1, 1, 0]");
}

#[test]
fn edge_all_connected_follows_the_graph() {
    let Some(r) = ev(&sq(r#"
         far = ents.add_line([50,50,50], [60,60,60])
         e = f.edges.first
         c = e.all_connected
         [c.grep(Sketchup::Edge).size, c.grep(Sketchup::Face).size, c.include?(far), far.all_connected.size, f.all_connected.size]"#))
    else {
        return;
    };
    assert_eq!(r, "[4, 1, false, 1, 5]");
}
