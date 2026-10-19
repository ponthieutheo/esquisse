//! Groupes et instances de composant.
use super::*;

const BOX: &str = "def box(ents, w = 1.0)
  f = ents.add_face([0,0,0],[w,0,0],[w,w,0],[0,w,0])
  f.pushpull(-w)
  f
end";

const ERR: &str = "def err; yield; 'ok'; rescue Exception => e; \"#{e.class}: #{e.message}\"; end";

#[test]
fn group_basics_name_guid_locked() {
    let Some(r) = val("model = Sketchup.active_model
         g = model.entities.add_group
         a = [g.name, g.guid =~ /\\A[0-9A-Za-z_$]{22}\\z/ && g.guid == g.guid ? :guid : g.guid, g.locked?]
         g.name = 'Caisse'; g.locked = true
         a + [g.name, g.locked?, g.class.name, g.is_a?(Sketchup::Drawingelement), g.typename]")
    else {
        return;
    };
    assert_eq!(r, "[\"\", :guid, false, \"Caisse\", true, \"Sketchup::Group\", true, \"Group\"]");
}

#[test]
fn transformation_setters_reject_singular_matrices() {
    let src = format!(
        "{ERR}
         g = Sketchup.active_model.entities.add_group
         flat = Geom::Transformation.new([1,0,0,0, 0,1,0,0, 0,0,0,0, 0,0,0,1])
         [err {{ g.transformation = flat }}, err {{ g.transform!(flat) }}, err {{ g.move!(flat) }}, err {{ g.transformation = 'a' }}]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[\"ArgumentError: Unable to set transformation: transformation matrix is not invertible\", \
         \"ArgumentError: Unable to apply transformation: transformation matrix is not invertible\", \"ok\", \
         \"TypeError: no implicit conversion to Transformation\"]"
    );
}

#[test]
fn transform_composes_and_move_replaces() {
    let Some(r) = val("g = Sketchup.active_model.entities.add_group
         g.transformation = Geom::Transformation.translation([1, 0, 0])
         a = g.transform!(Geom::Transformation.translation([0, 2, 0])).equal?(g)
         o1 = g.transformation.origin.to_a
         b = g.move!(Geom::Transformation.translation([5, 5, 5])).equal?(g)
         [a, o1, b, g.transformation.origin.to_a]")
    else {
        return;
    };
    assert_eq!(r, "[true, [1.0, 2.0, 0.0], true, [5.0, 5.0, 5.0]]");
}

#[test]
fn transformations_are_in_inches_and_millimetres_in_the_model() {
    let Some((_, m)) = must(
        "g = Sketchup.active_model.entities.add_group
         g.transformation = Geom::Transformation.translation([1, 0, 0])",
    ) else {
        return;
    };
    let i = m.root.instances.values().next().unwrap();
    assert!((i.xform.w_axis.x - 25.4).abs() < 1e-9);
}

#[test]
fn group_entities_make_a_shared_group_unique() {
    let Some(r) = val(
        "model = Sketchup.active_model
         g = model.entities.add_group
         g.entities.add_face([0,0,0],[1,0,0],[1,1,0])
         c = g.copy
         same = c.definition == g.definition
         c.entities.add_line([5,5,5],[6,6,6])
         [same, c.definition == g.definition, g.entities.grep(Sketchup::Edge).size, c.entities.grep(Sketchup::Edge).size]",
    ) else {
        return;
    };
    assert_eq!(r, "[true, false, 3, 4]");
}

#[test]
fn group_copy_keeps_name_position_and_attributes() {
    let Some(r) = val(
        "model = Sketchup.active_model
         g = model.entities.add_group
         g.name = 'Mur'; g.transformation = Geom::Transformation.translation([3, 0, 0]); g.set_attribute('a', 'b', 1)
         c = g.copy
         [c.equal?(g), c == g, c.name, c.transformation.origin.to_a, c.get_attribute('a', 'b'), model.entities.grep(Sketchup::Group).size]",
    ) else {
        return;
    };
    assert_eq!(r, "[false, false, \"Mur\", [3.0, 0.0, 0.0], 1, 2]");
}

#[test]
fn group_to_component_replaces_the_group() {
    let Some(r) = val(
        "model = Sketchup.active_model
         g = model.entities.add_group
         g.entities.add_face([0,0,0],[1,0,0],[1,1,0])
         c = g.to_component
         [c.class.name, g.valid?, c.definition.group?, c.definition.name =~ /\\AComposant#\\d+\\z/ ? :named : c.definition.name, model.entities.grep(Sketchup::ComponentInstance).size]",
    ) else {
        return;
    };
    assert_eq!(r, "[\"Sketchup::ComponentInstance\", false, false, :named, 1]");
}

#[test]
fn explode_returns_the_released_entities_or_false() {
    let Some(r) = val("model = Sketchup.active_model
         g = model.entities.add_group
         g.entities.add_face([0,0,0],[1,0,0],[1,1,0])
         out = g.explode
         locked = model.entities.add_group
         locked.entities.add_line([0,0,0],[1,1,1])
         locked.locked = true
         [out.map { |e| e.class.name }.sort, g.valid?, locked.explode, locked.valid?]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[[\"Sketchup::Edge\", \"Sketchup::Edge\", \"Sketchup::Edge\", \"Sketchup::Face\"], false, false, true]"
    );
}

#[test]
fn make_unique_returns_self_and_splits_the_definition() {
    let Some(r) = val(
        "model = Sketchup.active_model
         d = model.definitions.add('C'); d.entities.add_face([0,0,0],[1,0,0],[1,1,0])
         a = model.entities.add_instance(d, Geom::Transformation.new)
         b = model.entities.add_instance(d, Geom::Transformation.new)
         [d.count_instances, b.make_unique.equal?(b), d.count_instances, b.definition == d, b.definition.name =~ /\\AC#\\d+\\z/ ? :renamed : b.definition.name]",
    ) else {
        return;
    };
    assert_eq!(r, "[2, true, 1, false, :renamed]");
}

#[test]
fn definition_assignment_changes_the_definition() {
    let src = format!(
        "{ERR}
         model = Sketchup.active_model
         d1 = model.definitions.add('A'); d2 = model.definitions.add('B')
         i = model.entities.add_instance(d1, Geom::Transformation.new)
         [i.definition == d1, (i.definition = d2) == d2, i.definition == d2, d1.count_instances, d2.count_instances, err {{ i.definition = 5 }}]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[true, true, true, 0, 1, \"TypeError: wrong argument type (expected Sketchup::ComponentDefinition)\"]"
    );
}

#[test]
fn deleted_groups_raise_type_error() {
    let src = format!(
        "{ERR}
         g = Sketchup.active_model.entities.add_group
         g.erase!
         [g.valid?, g.deleted?, err {{ g.name }}, err {{ g.transformation }}, err {{ g.entities }}]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[false, true, \"TypeError: reference to deleted Group\", \"TypeError: reference to deleted Group\", \"TypeError: reference to deleted Group\"]"
    );
}

#[test]
fn local_bounds_are_in_the_groups_own_coordinates() {
    let Some(r) = val("g = Sketchup.active_model.entities.add_group
         g.entities.add_face([1,1,0],[3,1,0],[3,2,0],[1,2,0])
         g.transformation = Geom::Transformation.translation([10, 0, 0])
         b = g.local_bounds
         [b.min.to_a, b.max.to_a]")
    else {
        return;
    };
    assert_eq!(r, "[[1.0, 1.0, 0.0], [3.0, 2.0, 0.0]]");
}

#[test]
fn volume_is_minus_one_for_non_solids_and_cubic_inches_for_solids() {
    let src = format!(
        "{BOX}
         model = Sketchup.active_model
         solid = model.entities.add_group; box(solid.entities, 2.0)
         open = model.entities.add_group; open.entities.add_face([0,0,0],[1,0,0],[1,1,0])
         [solid.volume, solid.manifold?, open.volume, open.manifold?]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(r, "[8.0, true, -1.0, false]");
}

#[test]
fn solid_operations_return_nil_for_non_solids_and_groups_for_solids() {
    let src = format!(
        "{BOX}
         model = Sketchup.active_model
         a = model.entities.add_group; box(a.entities, 2.0)
         b = model.entities.add_group; box(b.entities, 2.0)
         b.transformation = Geom::Transformation.translation([1, 1, 0])
         open = model.entities.add_group; open.entities.add_face([0,0,0],[1,0,0],[1,1,0])
         bad = a.union(open)
         u = a.union(b)
         [bad, u.class.name, u.volume.round(6), a.valid?, b.valid?, model.entities.grep(Sketchup::Group).size]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(r, "[nil, \"Sketchup::Group\", 14.0, false, false, 2]");
}

#[test]
fn subtract_trim_intersect_and_split() {
    let src = format!(
        "{BOX}
         def pair
           model = Sketchup.active_model
           a = model.entities.add_group; box(a.entities, 2.0)
           b = model.entities.add_group; box(b.entities, 2.0)
           b.transformation = Geom::Transformation.translation([1, 1, 0])
           [a, b]
         end
         a, b = pair; sub = a.subtract(b)
         a2, b2 = pair; tr = a2.trim(b2)
         a3, b3 = pair; inter = a3.intersect(b3)
         a4, b4 = pair; parts = a4.split(b4)
         [sub.volume.round(6), tr.volume.round(6), a2.valid?, inter.volume.round(6), parts.size, parts.map {{ |p| p.volume.round(6) }}.sort]"
    );
    let Some(r) = val(&src) else { return };
    // The boxes overlap on 2 in³: subtract and trim give 8 - 2, split gives 6, 6 and 2.
    assert_eq!(r, "[6.0, 6.0, true, 2.0, 3, [2.0, 6.0, 6.0]]");
}

#[test]
fn outer_shell_merges_two_solids() {
    let src = format!(
        "{BOX}
         model = Sketchup.active_model
         a = model.entities.add_group; box(a.entities, 2.0)
         b = model.entities.add_group; box(b.entities, 2.0)
         b.transformation = Geom::Transformation.translation([1, 1, 0])
         s = a.outer_shell(b)
         [s.class.name, s.volume.round(6)]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(r, "[\"Sketchup::Group\", 14.0]");
}

#[test]
fn solid_operations_check_their_argument() {
    let src = format!(
        "{ERR}
         g = Sketchup.active_model.entities.add_group
         [err {{ g.union(3) }}, err {{ g.subtract(nil) }}, err {{ g.show_differences }}, err {{ g.equals?(:x) }}]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[\"TypeError: wrong argument type (expected Sketchup::Group)\", \"TypeError: wrong argument type (expected Sketchup::Group)\", \
         \"ArgumentError: 'show_differences' requires at least 1 argument.\", \"TypeError: wrong argument type (expected Sketchup::Group)\"]"
    );
}

#[test]
fn equals_compares_the_geometry_of_the_definitions() {
    let src = format!(
        "{BOX}
         model = Sketchup.active_model
         a = model.entities.add_group; box(a.entities, 2.0)
         b = model.entities.add_group; box(b.entities, 2.0)
         c = model.entities.add_group; box(c.entities, 3.0)
         [a.equals?(b), a.equals?(c), a.show_differences(b)]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(r, "[true, false, true]");
}

#[test]
fn glue_to_a_face_checks_alignment_parent_and_behavior() {
    let src = format!(
        "{ERR}
         model = Sketchup.active_model
         face = model.entities.add_face([0,0,0],[5,0,0],[5,5,0],[0,5,0])
         wall = model.entities.add_face([0,0,0],[5,0,0],[5,0,5],[0,0,5])
         d = model.definitions.add('Porte')
         comp = model.entities.add_instance(d, Geom::Transformation.new)
         grp = model.entities.add_group
         inner = model.definitions.add('Autre').entities.add_group
         r = []
         r << err {{ comp.glued_to = face }}
         d.behavior.is2d = true; d.behavior.snapto = 1
         r << err {{ comp.glued_to = wall }}
         r << err {{ comp.glued_to = face }}
         r << (comp.glued_to == face)
         r << err {{ comp.glued_to = 3 }}
         r << err {{ comp.glued_to = inner }}
         comp.glued_to = nil
         r << comp.glued_to
         grp.glued_to = face
         r << (grp.glued_to == face)
         r << err {{ grp.glued_to = grp }}
         r"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[\"ArgumentError: The definition does not have glueing behavior\", \"ArgumentError: Alignment is wrong\", \"ok\", true, \
         \"ArgumentError: wrong type - expected Sketchup::Face, Sketchup::Group, SketchUp::Image or Sketchup::ComponentInstance\", \
         \"ArgumentError: Can only glue to something in the same component\", nil, true, \"ArgumentError: Can't create cyclic gluing\"]"
    );
}

#[test]
fn hatch_pattern_defaults_to_nil_and_accepts_nil() {
    let Some(r) = val("g = Sketchup.active_model.entities.add_group
         [g.hatch_pattern, (g.hatch_pattern = nil)]")
    else {
        return;
    };
    assert_eq!(r, "[nil, nil]");
}

#[test]
fn setters_return_what_sketchup_returns() {
    let Some(r) = val("model = Sketchup.active_model
         g = model.entities.add_group
         d = model.definitions.add('C'); c = model.entities.add_instance(d, Geom::Transformation.new)
         t = Geom::Transformation.translation([1, 0, 0])
         [g.send(:name=, 'G'), c.send(:name=, 'C').equal?(c), g.send(:locked=, 1), c.send(:locked=, nil),
          g.send(:transformation=, t).equal?(g), g.send(:description=, 'x').equal?(g), g.send(:glued_to=, nil),
          d.send(:name=, 'D'), d.send(:description=, 'y').equal?(d), d.send(:insertion_point=, [0, 0, 0]).equal?(d),
          d.behavior.send(:is2d=, true).class.name, d.behavior.send(:snapto=, 1).class.name]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[\"G\", true, true, false, true, true, nil, nil, true, true, \"Sketchup::Behavior\", \"Sketchup::Behavior\"]"
    );
}

#[test]
fn group_description_is_the_definitions() {
    let Some(r) = val("g = Sketchup.active_model.entities.add_group
         g.description = 'Mur porteur'
         [g.description, g.definition.description]")
    else {
        return;
    };
    assert_eq!(r, "[\"Mur porteur\", \"Mur porteur\"]");
}
