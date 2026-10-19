//! Audit of hollow implementations: methods that existed but did not read
//! or write the model as SketchUp does. Each test fetches objects twice and checks that a change
//! made through one handle shows on the other, and lands in the model.

use super::*;

/// Runs `code` and returns the `inspect` of its last expression.
fn ev(code: &str) -> Option<String> {
    must(code).map(|(o, _)| o.result)
}

/// The exception class and message raised by `code`, as "Class: message".
fn raised(code: &str) -> Option<String> {
    let wrapped = format!("begin\n{code}\n'rien'\nrescue Exception => e\n \"#{{e.class}}: #{{e.message}}\"\nend");
    ev(&wrapped).map(|s| s.trim_matches('"').replace("\\\"", "\"").replace("\\n", "\n"))
}

#[test]
fn snaps_are_entities_stored_in_their_collection() {
    let Some((o, m)) = must(
        r#"m = Sketchup.active_model; ents = m.entities
         g = ents.add_group; g.entities.add_face([0, 0, 0], [10, 0, 0], [10, 10, 0])
         d = g.definition
         s = d.entities.add_snap([10, 5, 0], [1, 0, 0])
         t = d.entities.add_snap([0, 5, 0], [-1, 0, 0], [0, 1, 1])
         r = [s.class.name, s.valid?, s.typename, s.parent == d, d.entities.grep(Sketchup::Snap).size,
              d.entities.length, d.entities.to_a.include?(t), ents.grep(Sketchup::Snap).size,
              t.up.to_a.map { |v| v.round(6) }, s.entityID != t.entityID]
         # Fetched again from the collection: the same snap, and a change shows on both handles.
         s2 = d.entities.grep(Sketchup::Snap).find { |x| x == s }
         s.set([10, 6, 1], [0, 1, 0])
         r << s2.position.to_a << s2.direction.to_a << s2.up.to_a
         s.set([10, 6, 2])
         r << s2.position.to_a << s2.direction.to_a
         s.hidden = true
         r << s2.hidden? << s2.bounds.center.to_a
         d.entities.transform_entities(Geom::Transformation.translation([1, 0, 0]), [s])
         r << s2.position.to_a
         s.set_attribute('a', 'k', 5)
         r << s2.get_attribute('a', 'k')
         # Erasing the first snap does not change the second one's handle.
         s.erase!
         r << s2.valid? << t.valid? << t.position.to_a << d.entities.grep(Sketchup::Snap).size
         u = d.entities.add_snap([1, 1, 1], [0, 0, 1])
         r << s.valid? << u.up.to_a << (u == s)
         r"#,
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[\"Sketchup::Snap\", true, \"Snap\", true, 2, 6, true, 0, [0.0, 0.707107, 0.707107], true, \
         [10.0, 6.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [10.0, 6.0, 2.0], [0.0, 1.0, 0.0], true, [10.0, 6.0, 2.0], \
         [11.0, 6.0, 2.0], 5, false, true, [0.0, 5.0, 0.0], 1, false, [0.0, 1.0, 0.0], false]"
    );
    // In the model: the definition's snaps (the Move tool reads them), with stable ids.
    let d = m.defs.values().find(|d| d.is_group).unwrap();
    assert_eq!(d.entities.snaps.len(), 2);
    let t = d.entities.snaps[0];
    assert!(t.id != 0 && t.id != d.entities.snaps[1].id);
    assert!((t.pos - glam::DVec3::new(0.0, 5.0 * 25.4, 0.0)).length() < 1e-9);
    assert!((t.up - glam::DVec3::new(0.0, 1.0, 1.0).normalize()).length() < 1e-9);
}

#[test]
fn snap_arguments_and_deleted_snaps() {
    for (code, msg) in [
        (
            "Sketchup.active_model.entities.add_snap([0,0,0], [1,0,0], [2,0,0])",
            "ArgumentError: direction and up can't be parallel",
        ),
        (
            "Sketchup.active_model.entities.add_snap([0,0,0], [0,0,1], [0,0,-1])",
            "ArgumentError: direction and up can't be parallel",
        ),
        (
            "s = Sketchup.active_model.entities.add_snap([0,0,0], [1,0,0]); s.set([0,0,0], [0,0,1], [0,0,3])",
            "ArgumentError: direction and up can't be parallel",
        ),
        (
            "s = Sketchup.active_model.entities.add_snap([0,0,0], [1,0,0]); s.erase!; s.position",
            "TypeError: reference to deleted Snap",
        ),
        (
            "s = Sketchup.active_model.entities.add_snap([0,0,0], [1,0,0]); s.erase!; s.set([1,1,1])",
            "TypeError: reference to deleted Snap",
        ),
        (
            "s = Sketchup.active_model.entities.add_snap([0,0,0], [1,0,0]); s.erase!; s.set_attribute('a', 'b', 1)",
            "TypeError: reference to deleted Snap",
        ),
        ("Sketchup::Snap.new", "TypeError: allocator undefined for Sketchup::Snap"),
    ] {
        assert_eq!(raised(code).unwrap(), msg, "{code}");
    }
    // Entities#clear! and erase_entities take snaps too; so does the selection.
    let Some(r) = ev("ents = Sketchup.active_model.entities
         s = ents.add_snap([0,0,0], [1,0,0]); ents.add_cpoint([1,1,1])
         n = Sketchup.active_model.selection.add(ents.to_a)
         a = [ents.length, n, Sketchup.active_model.selection.size]
         ents.erase_entities(s)
         a << ents.length << s.valid?
         ents.add_snap([0,0,0], [1,0,0]); ents.clear!
         a << ents.length")
    else {
        return;
    };
    assert_eq!(r, "[2, 2, 2, 1, false, 0]");
}

#[test]
fn intersect_with_places_both_sides_and_draws_into_the_destination() {
    // A square in group `a` (moved 100" along red) crossed by a vertical face of group `b`
    // (moved 105"): the cut is at x = 105" in the model, x = 5" inside `a`.
    let Some(r) = ev(r#"
         m = Sketchup.active_model; ents = m.entities
         a = ents.add_group; a.entities.add_face([0,0,0], [10,0,0], [10,10,0], [0,10,0])
         a.transformation = Geom::Transformation.translation([100, 0, 0])
         b = ents.add_group; b.entities.add_face([0,-5,-5], [0,15,-5], [0,15,5], [0,-5,5])
         b.transformation = Geom::Transformation.translation([105, 0, 0])
         r = a.entities.intersect_with(false, a.transformation, a.entities, a.transformation, false, [b])
         xs = ->(es) { es.map { |e| [e.start.position.x, e.end.position.x].map { |v| v.round(6) } }.uniq }
         out = [r.size, xs.call(r), r.all? { |e| e.parent == a.definition }]
         r2 = a.entities.intersect_with(false, a.transformation, ents, Geom::Transformation.new, false, b)
         out << xs.call(r2) << r2.all? { |e| e.parent == m }
         # The destination may be given as the group itself.
         c = ents.add_group; c.entities.add_cpoint([0, 0, 0])
         r3 = a.entities.intersect_with(false, a.transformation, c, Geom::Transformation.new, false, [b])
         out << xs.call(r3) << r3.all? { |e| e.parent == c.definition }
         out"#)
    else {
        return;
    };
    assert_eq!(r, "[1, [[5.0, 5.0]], true, [[105.0, 105.0]], true, [[105.0, 105.0]], true]");
}

#[test]
fn faces_list_the_instances_glued_to_them() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model; ents = m.entities
         f = ents.add_face([0,0,0], [10,0,0], [10,10,0], [0,10,0])
         d = m.definitions.add('Fenêtre'); d.entities.add_line([0,0,0], [1,0,0]); d.behavior.is2d = true
         i = ents.add_instance(d, Geom::Transformation.new)
         j = ents.add_instance(d, Geom::Transformation.translation([3, 0, 0]))
         out = [f.get_glued_instances]
         i.glued_to = f
         out << f.get_glued_instances.map { |x| x == i } << ents.grep(Sketchup::Face).first.get_glued_instances.size
         i.glued_to = nil
         out << f.get_glued_instances"#)
    else {
        return;
    };
    assert_eq!(r, "[[], [true], 1, []]");
}

#[test]
fn attribute_dictionaries_die_with_their_entity() {
    let Some(r) = ev(r#"
         ents = Sketchup.active_model.entities
         e = ents.add_line([0,0,0], [1,0,0]); e.set_attribute('d', 'k', 1)
         c = ents.add_cpoint([1,1,1]); c.set_attribute('d', 'k', 2)
         d = e.attribute_dictionary('d'); ds = e.attribute_dictionaries; dc = c.attribute_dictionary('d')
         out = [d.valid?, ds.valid?, dc.valid?, d['k']]
         e.erase!; c.erase!
         out + [d.valid?, ds.valid?, dc.valid?, (d['k'] rescue $!.message), (c.get_attribute('d', 'k') rescue $!.message),
                (begin; dc['k'] = 3; rescue => x; x.message; end)]"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[true, true, true, 1, false, false, false, \"reference to deleted Attribute\", \
         \"reference to deleted ConstructionPoint\", \"reference to deleted Attribute\"]"
    );
}

#[test]
fn entities_go_back_to_the_default_tag() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model; ents = m.entities
         l = m.layers.add('Plans')
         e = ents.add_line([0,0,0], [1,0,0]); g = ents.add_group; c = ents.add_cpoint([1,1,1])
         [e, g, c].each { |x| x.layer = l }
         out = [e.layer.name, g.layer.name, c.layer.name]
         e.layer = m.layers[0]; g.layer = 'Layer0'; c.layer = m.layers[0]
         out + [e.layer.name, g.layer.name, c.layer.name, ents.grep(Sketchup::Edge).first.layer == m.layers[0]]"#)
    else {
        return;
    };
    assert_eq!(r, "[\"Plans\", \"Plans\", \"Plans\", \"Layer0\", \"Layer0\", \"Layer0\", true]");
}

#[test]
fn what_a_script_adds_goes_on_the_active_tag() {
    let Some((o, m)) = must(
        r#"m = Sketchup.active_model; ents = m.entities
         l = m.layers.add('Plans')
         m.active_layer = l
         e = ents.add_line([0,0,0], [1,0,0])
         f = ents.add_face([0,0,1], [1,0,1], [1,1,1])
         c = ents.add_cpoint([2,2,2])
         g = ents.add_group; x = g.entities.add_line([0,0,0], [0,1,0])
         s = ents.add_snap([0,0,0], [1,0,0])
         t = ents.add_text('a', [0,0,0])
         k = ents.add_line([5,0,0], [6,0,0]); k.layer = m.layers[0]
         out = [e.layer.name, f.layer.name, f.edges.map { |y| y.layer.name }.uniq, c.layer.name, g.layer.name,
                x.layer.name, s.layer.name, t.layer.name, k.layer.name]
         m.active_layer = m.layers[0]
         out << ents.add_line([9,0,0], [9,1,0]).layer.name"#,
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[\"Plans\", \"Plans\", [\"Plans\"], \"Plans\", \"Plans\", \"Plans\", \"Plans\", \"Plans\", \"Layer0\", \"Layer0\"]"
    );
    assert_eq!(m.root.snaps[0].layer, 1);
}

#[test]
fn scenes_remember_hidden_tag_folders() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model
         f = m.layers.add_folder('F'); g = m.layers.add_folder('G')
         p = m.pages.add('A')
         p.set_visibility(f, false)
         out = [p.layer_folders.map(&:name), p.layers.map(&:name), m.pages['A'].layer_folders.map(&:name)]
         p.set_visibility(f, true)
         out << p.layer_folders.map(&:name)
         g.visible = false
         q = m.pages.add('B')
         g.visible = true
         out << q.layer_folders.map(&:name)
         m.pages.selected_page = q
         out << g.visible? << f.visible?
         p.use_hidden_layers = false
         out << p.layer_folders"#)
    else {
        return;
    };
    assert_eq!(r, "[[\"F\"], [], [\"F\"], [], [\"G\"], false, true, nil]");
}

#[test]
fn scenes_remember_their_active_section_planes() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model; ents = m.entities
         s = ents.add_section_plane([0,0,5], [0,0,1]); s.activate
         g = ents.add_group; g.entities.add_cpoint([0,0,0])
         t = g.entities.add_section_plane([0,0,1], [0,0,1]); t.activate
         p = m.pages.add('Coupe')
         ents.active_section_plane = nil; g.entities.active_section_plane = nil
         q = m.pages.add('Sans')
         out = [p.active_section_planes.size, p.active_section_planes.include?(s), p.active_section_planes.include?(t),
                q.active_section_planes]
         m.pages.selected_page = p
         out << (ents.active_section_plane == s) << (g.entities.active_section_plane == t)
         m.pages.selected_page = q
         out << ents.active_section_plane << g.entities.active_section_plane
         t.erase!
         out << p.active_section_planes.size"#)
    else {
        return;
    };
    assert_eq!(r, "[2, true, true, [], true, true, nil, nil, 1]");
}

#[test]
fn scenes_remember_their_drawing_axes() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model
         m.axes.set([1,2,3], [1,0,0], [0,1,0], [0,0,1])
         p = m.pages.add('A')
         m.axes.set([0,0,0], [0,1,0], [-1,0,0], [0,0,1])
         out = [p.axes.origin.to_a, m.axes.origin.to_a, p.axes == m.axes, p.axes == m.pages['A'].axes]
         m.pages.selected_page = p
         out << m.axes.origin.to_a << m.axes.xaxis.to_a
         p.axes.set([5,5,5], [1,0,0], [0,1,0], [0,0,1])
         out << m.pages[0].axes.origin.to_a << m.axes.origin.to_a
         m.pages.erase(p)
         out << p.axes.valid? rescue out << $!.message"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[[1.0, 2.0, 3.0], [0.0, 0.0, 0.0], false, true, [1.0, 2.0, 3.0], [1.0, 0.0, 0.0], [5.0, 5.0, 5.0], \
         [1.0, 2.0, 3.0], \"reference to deleted Page\"]"
    );
}

#[test]
fn a_scene_camera_fetched_earlier_does_not_undo_later_changes() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model
         p = m.pages.add('A')
         c1 = p.camera; c2 = m.pages['A'].camera
         c2.set([1,1,1], [0,0,0], [0,0,1])
         c1.description = 'vue'
         [p.camera.eye.to_a, p.camera.description, c1.eye.to_a]"#)
    else {
        return;
    };
    assert_eq!(r, "[[1.0, 1.0, 1.0], \"vue\", [1.0, 1.0, 1.0]]");
}

#[test]
fn reading_the_styles_does_not_change_the_model() {
    let Some((o, m)) = must(
        "s = Sketchup.active_model.styles
         [s.size, s.selected_style.name, s.active_style.name, s[0] == s.selected_style]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[1, \"Style par défaut\", \"Style par défaut\", true]");
    assert!(!m.view_ext.contains_key("styles"), "lire les styles a modifié le modèle");
    let Some((o, m)) = must(
        "s = Sketchup.active_model.styles
         s.selected_style.description = 'Plan'
         [s.size, s.selected_style.description]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[1, \"Plan\"]");
    assert_eq!(m.view_ext["styles"]["list"][0]["description"], "Plan");
    // The default style keeps the settings it had before the first change.
    let Some((o, _)) = must(
        "m = Sketchup.active_model
         m.rendering_options['BackgroundColor'] = 'red'
         [m.styles.active_style_changed, m.styles.active_style == m.styles.selected_style]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[true, false]");
}

#[test]
fn spaces_are_entities_of_their_collection() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model; ents = m.entities
         g = ents.add_group; g.entities.add_cpoint([0,0,0])
         s = g.entities.add_space('Salon', [[0,0,0], [100,0,0], [100,50,0]], [10,10,0], 96)
         out = [s.class.name, s.valid?, s.parent == g.definition, g.entities.grep(Sketchup::Space).size,
                ents.grep(Sketchup::Space).size, s.ceiling_plane, g.entities.length]
         s2 = g.entities.grep(Sketchup::Space).first
         s.name = 'Cuisine'; out << s2.name << (s2 == s)
         s.hidden = true; out << s2.hidden?
         g.entities.transform_entities(Geom::Transformation.translation([0, 0, 10]), s)
         out << s2.perimeter_points[0].to_a << s2.orientation_point.to_a << s2.ceiling_plane
         s.erase!
         out << s.valid? << g.entities.grep(Sketchup::Space).size << (s.name rescue $!.message)
         t = ents.add_space('Autre', [[0,0,0], [1,0,0], [1,1,0]], [0,0,0], 50)
         out << (t == s) << t.valid? << ents.to_a.include?(t)
         out << (ents.add_space('x', 3, [0,0,0], 1) rescue $!.message)
         out << (ents.add_space('x', [[0,0,0], [1,0,0], [1,1,0]], [0,0,0]) rescue $!.message)"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[\"Sketchup::Space\", true, true, 1, 0, [0.0, 0.0, 1.0, -96.0], 2, \"Cuisine\", true, true, [0.0, 0.0, 10.0], \
         [10.0, 10.0, 10.0], [0.0, 0.0, 1.0, -106.0], false, 0, \"reference to deleted Space\", false, true, true, \
         \"wrong argument type Integer (expected Array)\", \"wrong number of arguments (given 3, expected 4)\"]"
    );
}

#[test]
fn grouping_everything_takes_the_snaps_and_spaces_along() {
    let Some((o, m)) = must(
        r#"m = Sketchup.active_model; ents = m.entities
         ents.add_face([10,10,0], [20,10,0], [20,20,0])
         ents.add_snap([15, 12, 0], [1, 0, 0])
         ents.add_space('Pièce', [[10,10,0], [20,10,0], [20,20,0]], [12,12,0], 96)
         g = ents.add_group(ents.to_a)
         s = g.entities.grep(Sketchup::Snap).first; p = g.entities.grep(Sketchup::Space).first
         t = g.transformation
         only = ents.add_group(ents.add_snap([0, 0, 0], [0, 1, 0]))
         [ents.to_a.map { |e| e.class.name }.sort, g.entities.grep(Sketchup::Face).size,
          (t * s.position).to_a, (t * p.orientation_point).to_a, only.entities.grep(Sketchup::Snap).size]"#,
    ) else {
        return;
    };
    assert_eq!(o.result, "[[\"Sketchup::Group\", \"Sketchup::Group\"], 1, [15.0, 12.0, 0.0], [12.0, 12.0, 0.0], 1]");
    assert!(m.root.snaps.is_empty());
}

#[test]
fn exploding_a_component_brings_its_snaps_out() {
    let Some((o, m)) = must(
        r#"m = Sketchup.active_model; ents = m.entities
         d = m.definitions.add('Prise'); d.entities.add_line([0,0,0], [1,0,0])
         d.entities.add_snap([1, 0, 0], [1, 0, 0])
         a = ents.add_instance(d, Geom::Transformation.translation([10, 0, 0]))
         b = ents.add_instance(d, Geom::Transformation.translation([20, 0, 0]))
         ra = a.explode; rb = b.explode
         snaps = ents.grep(Sketchup::Snap)
         [ra.grep(Sketchup::Snap).size, snaps.map { |s| s.position.x }.sort, snaps[0] != snaps[1], d.entities.grep(Sketchup::Snap).size]"#,
    ) else {
        return;
    };
    assert_eq!(o.result, "[1, [11.0, 21.0], true, 1]");
    assert_eq!(m.root.snaps.len(), 2);
}

#[test]
fn view_objects_answer_as_entities() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model
         p = m.pages.add('P')
         objs = [m.styles, m.styles[0], m.rendering_options, m.shadow_info, m.pages, m.environments, p.rendering_options, p.shadow_info]
         out = objs.map do |o|
           [o.valid?, o.typename, o.entityID.is_a?(Integer), o.persistent_id == o.entityID, o.parent.class.name.split('::').last,
            o.model == m]
         end
         out << [m.rendering_options.entityID == m.rendering_options.entityID, m.styles[0].parent == m.styles]
         ro = p.rendering_options
         m.pages.erase(p)
         out << ro.valid?"#)
    else {
        return;
    };
    assert_eq!(
        r,
        "[[true, \"Styles\", true, true, \"Model\", true], [true, \"Style\", true, true, \"Styles\", true], \
         [true, \"RenderingOptions\", true, true, \"Model\", true], [true, \"ShadowInfo\", true, true, \"Model\", true], \
         [true, \"Pages\", true, true, \"Model\", true], [true, \"Environments\", true, true, \"Model\", true], \
         [true, \"RenderingOptions\", true, true, \"Page\", true], [true, \"ShadowInfo\", true, true, \"Page\", true], \
         [true, true], false]"
    );
}

#[test]
fn attribute_dictionaries_know_their_entity() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model; ents = m.entities
         e = ents.add_line([0,0,0], [1,0,0]); g = ents.add_group; g.entities.add_cpoint([0,0,0])
         d = m.definitions.add('D'); mat = m.materials.add('Bois'); l = m.layers.add('T')
         [e, g, d, mat, l, m].each { |x| x.set_attribute('a', 'k', 1) }
         [e.attribute_dictionaries.parent == e, g.attribute_dictionaries.parent == g, d.attribute_dictionaries.parent == d,
          mat.attribute_dictionaries.parent == mat, l.attribute_dictionaries.parent == l, m.attribute_dictionaries.parent == m,
          e.attribute_dictionary('a').parent.parent == e]"#)
    else {
        return;
    };
    assert_eq!(r, "[true, true, true, true, true, true, true]");
}

#[test]
fn erased_entities_leave_the_selection() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model; ents = m.entities; s = m.selection
         f = ents.add_face([0,0,0], [1,0,0], [1,1,0]); g = ents.add_group; g.entities.add_cpoint([0,0,0])
         c = ents.add_cpoint([5,5,5])
         s.add(f, g, c)
         out = [s.size]
         f.erase!; out << s.size << s.include?(f)
         g.explode; out << s.size
         ents.clear!; out << s.size << s.empty?"#)
    else {
        return;
    };
    assert_eq!(r, "[3, 2, false, 1, 0, true]");
}

#[test]
fn disabled_plugins_are_not_loaded_and_the_choice_is_kept() {
    let dir = std::env::temp_dir().join(format!("esq_plugins_off_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join("compte.rb"), "$esq_plugin_runs = ($esq_plugin_runs || 0) + 1\n").unwrap();
    let d = dir.to_string_lossy().replace('\\', "/");
    let Some(r) = ev(&format!(
        "ENV['ESQ_DEFAULTS_FILE'] = '{d}/prefs.json'
         Sketchup.plugins_disabled = true
         out = [Esq.load_plugins('{d}'), $esq_plugin_runs, Sketchup.read_default('PREFS', 'RubyManager_DisablePlugins')]
         Sketchup.plugins_disabled = false
         out += [Esq.load_plugins('{d}'), $esq_plugin_runs, Sketchup.plugins_disabled?]
         ENV.delete('ESQ_DEFAULTS_FILE')
         out"
    )) else {
        return;
    };
    assert_eq!(r, "[0, nil, true, 1, 1, false]");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_scenes_follow_the_tags_page_behavior() {
    let Some(r) = ev(r#"
         m = Sketchup.active_model
         a = m.layers.add('A'); b = m.layers.add('B'); c = m.layers.add('C'); b.visible = false; c.visible = false
         a.page_behavior = LAYER_IS_HIDDEN_ON_NEW_PAGES
         b.page_behavior = LAYER_IS_VISIBLE_ON_NEW_PAGES
         f = m.layers.add_folder('F'); f.visible_on_new_pages = false
         p = m.pages.add('P')
         q = m.pages.add('Q', PAGE_NO_CAMERA & ~PAGE_USE_LAYER_VISIBILITY)
         [p.layers.map(&:name).sort, p.layer_folders.map(&:name), a.visible?, b.visible?, f.visible?, q.layers]"#)
    else {
        return;
    };
    assert_eq!(r, "[[\"A\", \"C\"], [\"F\"], true, false, true, nil]");
}

#[test]
fn entities_observers_see_snaps_come_and_go() {
    let Some(r) = ev(r#"
         Esq::Events.reset!
         log = []
         spy = Object.new
         spy.define_singleton_method(:onElementAdded) { |_e, x| log << [:added, x.class.name] }
         spy.define_singleton_method(:onElementModified) { |_e, x| log << [:modified, x.class.name] }
         spy.define_singleton_method(:onElementRemoved) { |_e, _id| log << [:removed] }
         ents = Sketchup.active_model.entities
         ents.add_observer(spy)
         s = ents.add_snap([0, 0, 0], [1, 0, 0])
         s.set([1, 0, 0])
         s.erase!
         Esq::Events.reset!
         log"#)
    else {
        return;
    };
    assert_eq!(r, "[[:added, \"Sketchup::Snap\"], [:modified, \"Sketchup::Snap\"], [:removed]]");
}
