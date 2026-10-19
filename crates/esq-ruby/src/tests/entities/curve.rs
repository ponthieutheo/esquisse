use super::*;

#[test]
fn circle_edges_belong_to_an_arc_curve() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         edges = ents.add_circle([0,0,0], [0,0,1], 5, 12)
         c = edges[0].curve
         [edges.size, c.class.name, c.typename, c.count_edges, c.edges.size, c.vertices.size, c.first_edge == edges[0],
          c.length.round(4), c.is_polygon?, c.circular?, c.center.to_a, c.radius, c.normal.to_a, c.xaxis.length.round(6),
          c.yaxis.to_a.map { |v| v.round(6) }.size, c.start_angle, c.end_angle.round(6), c.plane,
          edges.all? { |e| e.curve == c }, c.edges.map(&:handle).sort == edges.map(&:handle).sort]"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[12, \"Sketchup::ArcCurve\", \"ArcCurve\", 12, 12, 12, true, 31.0583, false, true, [0.0, 0.0, 0.0], 5.0, [0.0, 0.0, 1.0], 1.0, 3, 0.0, 6.283185, [0.0, 0.0, 1.0, 0.0], true, true]"
    );
}

#[test]
fn arc_curve_angles_and_axes() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         edges = ents.add_arc([1,1,0], [1,0,0], [0,0,1], 4, 0.5, 2.0, 8)
         c = edges[0].curve
         [edges.size, c.class.name, c.circular?, c.start_angle, c.end_angle, c.radius.round(6), c.center.to_a.map { |v| v.round(6) },
          c.xaxis.to_a, c.yaxis.to_a, c.normal.to_a, c.vertices.size, c.is_polygon?]"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[8, \"Sketchup::ArcCurve\", false, 0.5, 2.0, 4.0, [1.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 9, false]"
    );
}

#[test]
fn polyline_curve_and_ngon() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         es = ents.add_curve([0,0,0], [1,1,0], [2,0,0], [3,1,0])
         c = es[0].curve
         ng = ents.add_ngon([10,0,0], [0,0,1], 3, 6)
         p = ng[0].curve
         [es.size, c.class.name, c.vertices.size, c.length.round(4), c.is_polygon?, p.class.name, p.is_polygon?, p.count_edges,
          c.vertices.map { |v| v.position.to_a }.first, c.edges.last == es.last]"#)
    else {
        return;
    };
    assert_eq!(r, "[3, \"Sketchup::Curve\", 4, 4.2426, false, \"Sketchup::Curve\", true, 6, [0.0, 0.0, 0.0], true]");
}

#[test]
fn curve_each_edge_and_interior_vertices() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         es = ents.add_curve([0,0,0], [1,1,0], [2,0,0], [3,1,0])
         c = es[0].curve
         seen = []
         ret = c.each_edge { |e| seen << e }
         vs = c.vertices
         [seen.size, ret, vs.map { |v| v.curve_interior? == c }, es[0].start.curve_interior?, c.last_edge == es[2]]"#)
    else {
        return;
    };
    assert_eq!(r, "[3, nil, [false, true, true, false], nil, true]");
}

#[test]
fn curve_move_vertices_and_errors() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         es = ents.add_curve([0,0,0], [1,1,0], [2,0,0])
         c = es[0].curve
         c.move_vertices([[0,0,0], [1,2,0], [2,0,0]])
         [c.vertices.map { |v| v.position.to_a }, c.length.round(4)]"#)
    else {
        return;
    };
    assert_eq!(r, "[[[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [2.0, 0.0, 0.0]], 4.4721]");
    assert_eq!(
        raised(
            "c = Sketchup.active_model.entities.add_curve([0,0,0],[1,1,0],[2,0,0])[0].curve; c.move_vertices([[0,0,0]])"
        )
        .unwrap(),
        "ArgumentError: Number of points must be the same as the number of vertices"
    );
}

#[test]
fn explode_curve_frees_the_edges() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         es = ents.add_curve([0,0,0], [1,1,0], [2,0,0])
         c = es[0].curve
         es[0].explode_curve
         [es.map(&:curve), c.valid?]"#)
    else {
        return;
    };
    assert_eq!(r, "[[nil, nil], false]");
}

#[test]
fn weld_joins_edges_into_a_curve() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         es = ents.add_edges([0,0,0], [1,1,0], [2,0,0], [3,1,0])
         other = ents.add_edges([10,0,0], [11,1,0], [12,0,0])
         cs = ents.weld(es + other)
         c = cs[0]
         [cs.size, c.class.name, c.count_edges, cs[1].count_edges, es.all? { |e| e.curve == c }, es.map(&:curve).uniq.size,
          other[0].curve == cs[1]]"#)
    else {
        return;
    };
    assert_eq!(r, "[2, \"Sketchup::Curve\", 3, 2, true, 1, true]");
    assert_eq!(
        raised("Sketchup.active_model.entities.weld(5)").unwrap(),
        "TypeError: wrong argument type Integer (expected Array)"
    );
}

#[test]
fn curve_argument_errors() {
    assert_eq!(
        raised("Sketchup.active_model.entities.add_curve([0,0,0])").unwrap(),
        "ArgumentError: Not enough points - at least 2 required"
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.add_edges([0,0,0])").unwrap(),
        "ArgumentError: Not enough points - at least 2 required"
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.add_arc([0,0,0], [1,0,0], [0,0,1], 1, 0)").unwrap(),
        "ArgumentError: Too few arguments - at least 6 required"
    );
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         [ents.add_line([1,1,1], [1,1,1]), ents.add_edges([1,1,1], [1,1,1]), ents.add_line([0,0,0],[1,0,0]).class.name]"#)
    else {
        return;
    };
    assert_eq!(r, "[nil, nil, \"Sketchup::Edge\"]");
}

#[test]
fn construction_point_and_lines() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         cp = ents.add_cpoint([1,2,3])
         seg = ents.add_cline([0,0,0], [10,0,0])
         inf = ents.add_cline([0,0,5], Geom::Vector3d.new(0,0,2), '-')
         [cp.class.name, cp.position.to_a, cp.typename, seg.class.name, seg.start.to_a, seg.end.to_a, seg.direction.to_a, seg.position.to_a,
          seg.stipple, inf.start, inf.end, inf.direction.to_a, inf.position.to_a, inf.stipple,
          ents.length, ents.grep(Sketchup::ConstructionLine).size]"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[\"Sketchup::ConstructionPoint\", [1.0, 2.0, 3.0], \"ConstructionPoint\", \"Sketchup::ConstructionLine\", [0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 0.0], \"\", nil, nil, [0.0, 0.0, 1.0], [0.0, 0.0, 5.0], \"-\", 3, 2]"
    );
}

#[test]
fn construction_line_setters() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         seg = ents.add_cline([0,0,0], [10,0,0])
         seg.end = [0, 10, 0]
         a = [seg.end.to_a, seg.direction.to_a]
         seg.start = [0, 4, 0]
         a += [seg.start.to_a, seg.end.to_a]
         seg.reverse!
         a += [seg.start.to_a, seg.end.to_a]
         seg.position = [1, 1, 1]
         a += [seg.position.to_a]
         seg.stipple = '.'
         seg.end = nil
         a += [seg.stipple, seg.end, seg.start.to_a, seg.reverse!.equal?(seg), seg.start, seg.end.to_a, seg.direction.to_a]
         seg.end = nil
         a + [seg.start, seg.end, seg.position.to_a]"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[[0.0, 10.0, 0.0], [0.0, 1.0, 0.0], [0.0, 4.0, 0.0], [0.0, 10.0, 0.0], [0.0, 10.0, 0.0], [0.0, 4.0, 0.0], [1.0, 1.0, 1.0], \".\", nil, [1.0, 1.0, 1.0], true, nil, [1.0, 1.0, 1.0], [0.0, 1.0, 0.0], nil, nil, [1.0, 1.0, 1.0]]"
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.add_cline([0,0,0])").unwrap(),
        "ArgumentError: Too few arguments - 2 required"
    );
}
