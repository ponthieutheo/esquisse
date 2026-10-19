use super::*;

#[test]
fn text_with_a_leader() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         t = ents.add_text('Bonjour', [1,2,3], [0,0,5])
         [t.class.name, t.typename, t.text, t.point.to_a, t.vector.to_a, t.leader_type, t.has_leader?, t.display_leader?,
          t.arrow_type, t.line_weight, t.font, t.attached_to, ents.length]"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[\"Sketchup::Text\", \"Text\", \"Bonjour\", [1.0, 2.0, 3.0], [0.0, 0.0, 5.0], 2, true, true, 0, 1, {:name=>\"Arial\", :size=>12, :bold=>false, :italic=>false}, nil, 1]"
    );
}

#[test]
fn text_without_a_leader_ignores_point_and_vector() {
    let Some(r) = ev(r#"
         t = Sketchup.active_model.entities.add_text('seul', [1,1,1])
         [t.leader_type, t.has_leader?, t.vector, t.point = [5,5,5], t.vector = [1,1,1], t.point.to_a]"#)
    else {
        return;
    };
    assert_eq!(r, "[0, false, nil, [5, 5, 5], [1, 1, 1], [1.0, 1.0, 1.0]]");
}

#[test]
fn text_setters() {
    let Some(r) = ev(r#"
         t = Sketchup.active_model.entities.add_text('a', [1,1,1], [1,0,0])
         a = [t.text = 'b', t.set_text('c').equal?(t), t.text]
         a += [t.send(:point=, [2, 2, 2]), t.send(:vector=, [0, 3, 0]), t.point.to_a, t.vector.to_a]
         t.leader_type = 1
         t.arrow_type = Sketchup::Dimension::ARROW_DOT
         t.line_weight = 3
         t.display_leader = false
         t.font = { name: 'Courier', size: 20, bold: true }
         a + [t.leader_type, t.arrow_type, t.line_weight, t.display_leader?, t.font]"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[\"b\", true, \"c\", true, true, [2.0, 2.0, 2.0], [0.0, 3.0, 0.0], 1, 2, 3, false, {:name=>\"Courier\", :size=>20, :bold=>true, :italic=>false}]"
    );
}

#[test]
fn text_range_checks() {
    let base = "t = Sketchup.active_model.entities.add_text('a', [1,1,1], [1,0,0])\n";
    assert_eq!(raised(&format!("{base} t.leader_type = 7")).unwrap(), "RangeError: Value out of range");
    assert_eq!(raised(&format!("{base} t.arrow_type = 5")).unwrap(), "RangeError: Value out of range");
    assert_eq!(raised(&format!("{base} t.arrow_type = -1")).unwrap(), "RangeError: Value out of range");
    assert_eq!(
        raised(&format!("{base} t.font = {{ size: 5000 }}")).unwrap(),
        "RangeError: Font size 5000 out of range (1..1000)"
    );
    assert_eq!(raised(&format!("{base} t.font = 'Arial'")).unwrap(), "TypeError: wrong argument type (expected Hash)");
    assert_eq!(
        raised("Sketchup.active_model.entities.add_text('a')").unwrap(),
        "ArgumentError: Wrong number of arguments: at least 2 required"
    );
    assert_eq!(raised("Sketchup.active_model.entities.add_text('a', 5)").unwrap(), "ArgumentError: Invalid arguments.");
    let Some(r) = ev(&format!("{base} t.leader_type = 0")) else { return };
    assert_eq!(r, "0");
    let Some(o) = run(&format!("{base} t.leader_type = 0; t.leader_type")) else { return };
    assert!(o.0.out.contains("ALeaderNone cannot be set"), "{}", o.0.out);
    assert_eq!(o.0.result, "2");
}

#[test]
fn text_attached_to_an_entity() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         e = ents.add_line([0,0,0], [10,0,0])
         t = ents.add_text('lié', [[e], [5,0,0]], [0,0,3])
         [t.attached_to[0].leaf == e, t.attached_to[0].class, t.attached_to[1].to_a, t.point.to_a]"#)
    else {
        return;
    };
    assert_eq!(r, "[true, Sketchup::InstancePath, [5.0, 0.0, 0.0], [5.0, 0.0, 0.0]]");
    // SketchUp veut un chemin en tête du tableau, pas une entité ; un chemin
    // sans feuille est refusé.
    assert_eq!(
        raised("e = Sketchup.active_model.entities.add_line([0,0,0],[1,0,0]); Sketchup.active_model.entities.add_text('x', [e, [0,0,0]])").unwrap(),
        "TypeError: Argument must be type of Sketchup::InstancePath or an Array representing an instance path"
    );
    assert_eq!(
        raised("g = Sketchup.active_model.entities.add_group; g.entities.add_cpoint([0,0,0]); Sketchup.active_model.entities.add_text('x', [[g], [0,0,0]])").unwrap(),
        "TypeError: Invalid element. Leaf has to be either Sketchup::Vertex, Sketchup::Edge, Sketchup::ConstructionLine, or Sketchup::ConstructionPoint"
    );
}

#[test]
fn linear_dimension() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         d = ents.add_dimension_linear([0,0,0], [10,0,0], [0,5,0])
         [d.class.name, d.typename, d.start[0], d.start[1].to_a, d.end[1].to_a, d.offset_vector.to_a, d.text, d.arrow_type,
          d.has_aligned_text?, d.text_position, d.aligned_text_position, d.start_attached_to, d.end_attached_to, d.plane.size,
          d.is_a?(Sketchup::Dimension), d.is_a?(Sketchup::Drawingelement)]"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[\"Sketchup::DimensionLinear\", \"DimensionLinear\", nil, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 5.0, 0.0], \"254 mm\", 3, false, 1, 1, nil, nil, 4, true, true]"
    );
}

#[test]
fn linear_dimension_setters() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         e = ents.add_line([0,0,0], [10,0,0])
         d = ents.add_dimension_linear([e.start, [0,0,0]], [e.end, [10,0,0]], [0,5,0])
         a = [d.start_attached_to[0].leaf == e.start, d.end_attached_to[0].leaf == e.end]
         d.text = '10 pouces'
         d.arrow_type = Sketchup::Dimension::ARROW_SLASH
         d.has_aligned_text = true
         d.text_position = Sketchup::DimensionLinear::TEXT_OUTSIDE_END
         d.aligned_text_position = Sketchup::DimensionLinear::ALIGNED_TEXT_OUTSIDE
         d.offset_vector = [0, 0, 7]
         d.start = [[0, 1, 0]].flatten
         a + [d.text, d.arrow_type, d.has_aligned_text?, d.text_position, d.aligned_text_position, d.offset_vector.to_a, d.start[1].to_a]"#)
    else {
        return;
    };
    assert_eq!(r, "[true, true, \"10 pouces\", 1, true, 2, 2, [0.0, 0.0, 7.0], [0.0, 1.0, 0.0]]");
}

#[test]
fn dimension_errors_and_constants() {
    let base = "ents = Sketchup.active_model.entities\n d = ents.add_dimension_linear([0,0,0], [10,0,0], [0,5,0])\n";
    assert_eq!(raised(&format!("{base} d.offset_vector = [0,0,0]")).unwrap(), "ArgumentError: Invalid argument");
    assert_eq!(raised(&format!("{base} d.arrow_type = 9")).unwrap(), "RangeError: Value out of range");
    assert_eq!(raised(&format!("{base} d.text_position = 3")).unwrap(), "RangeError: Value out of range");
    assert_eq!(raised(&format!("{base} d.aligned_text_position = -1")).unwrap(), "RangeError: Value out of range");
    assert_eq!(
        raised("Sketchup.active_model.entities.add_dimension_linear([0,0,0], [0,0,0], [0,1,0])").unwrap(),
        "ArgumentError: The start point and end point cannot be the same."
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.add_dimension_linear([0,0,0], [1,0,0])").unwrap(),
        "ArgumentError: Wrong number of arguments: at least 3 required"
    );
    let Some(r) = ev(
        "[Sketchup::Dimension::ARROW_NONE, Sketchup::Dimension::ARROW_SLASH, Sketchup::Dimension::ARROW_DOT, Sketchup::Dimension::ARROW_CLOSED, Sketchup::Dimension::ARROW_OPEN,
          Sketchup::DimensionLinear::TEXT_OUTSIDE_START, Sketchup::DimensionLinear::TEXT_CENTERED, Sketchup::DimensionLinear::TEXT_OUTSIDE_END,
          Sketchup::DimensionLinear::ALIGNED_TEXT_ABOVE, Sketchup::DimensionLinear::ALIGNED_TEXT_CENTER, Sketchup::DimensionLinear::ALIGNED_TEXT_OUTSIDE]",
    ) else {
        return;
    };
    assert_eq!(r, "[0, 1, 2, 3, 4, 0, 1, 2, 0, 1, 2]");
}

#[test]
fn radial_dimension() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         c = ents.add_circle([0,0,0], [0,0,1], 5, 24)[0].curve
         d = ents.add_dimension_radial(c, [1, 0, 0])
         pts = d.leader_points
         [d.class.name, d.typename, d.arc_curve == c, pts.size, pts[0].to_a, d.leader_break_point.to_a.map { |v| v.round(4) }, d.text, d.arrow_type,
          d.is_a?(Sketchup::Dimension)]"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[\"Sketchup::DimensionRadial\", \"DimensionRadial\", true, 3, [5.0, 0.0, 0.0], [6.25, 0.0, 0.0], \"127 mm\", 3, true]"
    );
    assert_eq!(
        raised("e = Sketchup.active_model.entities.add_line([0,0,0],[1,0,0]); Sketchup.active_model.entities.add_dimension_radial(e, [1,0,0])").unwrap(),
        "ArgumentError: Invalid argument"
    );
}

#[test]
fn section_planes() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         s = ents.add_section_plane([0,0,5], [0,0,1])
         a = [s.class.name, s.typename, s.get_plane, s.active?, ents.active_section_plane, s.name, s.symbol]
         s.activate
         s.name = 'coupe A'
         s.symbol = 'AB'
         a += [s.active?, ents.active_section_plane == s, s.name, s.symbol]
         s.set_plane([1, 0, 0, -2]).equal?(s)
         a += [s.get_plane]
         s.set_plane([0, 0, 0], [0, 1, 0])
         a += [s.get_plane]
         ents.active_section_plane = nil
         a + [s.active?, ents.active_section_plane, s.hatch_pattern]"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[\"Sketchup::SectionPlane\", \"SectionPlane\", [0.0, 0.0, 1.0, -5.0], false, nil, \"Section 1\", \"1\", true, true, \"coupe A\", \"AB\", [1.0, 0.0, 0.0, -2.0], [0.0, 1.0, 0.0, 0.0], false, nil, nil]"
    );
}

#[test]
fn section_plane_errors() {
    let base = "ents = Sketchup.active_model.entities\n s = ents.add_section_plane([0,0,0], [0,0,1])\n";
    assert_eq!(raised(&format!("{base} s.symbol = 'ABCDEF'")).unwrap(), "ArgumentError: invalid section plane symbol");
    assert_eq!(raised(&format!("{base} s.name = 5")).unwrap(), "TypeError: no implicit conversion into String");
    assert_eq!(
        raised(&format!("{base} s.set_plane([1,2,3])")).unwrap(),
        "ArgumentError: Incorrect number of elements."
    );
    assert_eq!(
        raised(&format!("{base} g = ents.add_group; g.entities.active_section_plane = s")).unwrap(),
        "ArgumentError: The given SectionPlane does not belong to this Entities."
    );
    let Some(r) = ev(&format!("{base} ents.add_section_plane([0,0,1,-3]).get_plane")) else { return };
    assert_eq!(r, "[0.0, 0.0, 1.0, -3.0]");
}

#[test]
fn model_axes() {
    let Some(r) = ev(r#"
         ax = Sketchup.active_model.axes
         a = [ax.class.name, ax.typename, ax.origin.to_a, ax.xaxis.to_a, ax.yaxis.to_a, ax.zaxis.to_a, ax.axes.size, ax.to_a.size, ax.sketch_plane]
         ax.set([1, 2, 3], [0, 1, 0], [-1, 0, 0], [0, 0, 1])
         a + [ax.origin.to_a, ax.xaxis.to_a, ax.yaxis.to_a, ax.zaxis.to_a, ax.transformation.origin.to_a, ax.sketch_plane,
              ax.valid?, ax.persistent_id]"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[\"Sketchup::Axes\", \"Axes\", [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 3, 4, [0.0, 0.0, 1.0, 0.0], [1.0, 2.0, 3.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 2.0, 3.0], [0.0, 0.0, 1.0, -3.0], true, 0]"
    );
    assert_eq!(
        raised("Sketchup.active_model.axes.set([0,0,0], [1,0,0], [1,1,0], [0,0,1])").unwrap(),
        "ArgumentError: Axes must be orthogonal"
    );
}
