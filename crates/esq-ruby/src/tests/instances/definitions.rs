//! Définitions, liste des définitions, comportement.
use super::*;

const ERR: &str = "def err; yield; 'ok'; rescue Exception => e; \"#{e.class}: #{e.message}\"; end";

#[test]
fn definitions_add_gives_unique_names() {
    let Some(r) = val("defs = Sketchup.active_model.definitions
         a = defs.add('Porte'); b = defs.add('Porte'); c = defs.add
         [a.name, b.name, c.name, defs.length, defs.size, a.class.name]")
    else {
        return;
    };
    assert_eq!(r, "[\"Porte\", \"Porte#1\", \"Composant\", 3, 3, \"Sketchup::ComponentDefinition\"]");
}

#[test]
fn definition_list_lookup_by_name_index_and_guid() {
    let src = format!(
        "{ERR}
         defs = Sketchup.active_model.definitions
         a = defs.add('A'); b = defs.add('B'); c = defs.add('C')
         [defs['B'] == b, defs['nope'], defs[0] == a, defs[-1] == c, defs.at(1) == b, defs[b.guid] == b,
          err {{ defs[3] }}, err {{ defs[-4] }}, err {{ defs[nil] }}, defs[1.9] == b]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[true, nil, true, true, true, true, \"IndexError: index 3 out of range [0, 3)\", \"IndexError: index -1 out of range [0, 3)\", \
         \"TypeError: no implicit conversion from nil to integer\", true]"
    );
}

#[test]
fn definition_list_includes_group_definitions_and_each_returns_self() {
    let Some(r) = val("model = Sketchup.active_model
         g = model.entities.add_group
         d = model.definitions.add('C')
         names = []; ret = model.definitions.each { |x| names << x.group? }
         [names.sort_by(&:to_s), ret.equal?(model.definitions), model.definitions.to_a.size, g.definition.group?]")
    else {
        return;
    };
    assert_eq!(r, "[[false, true], true, 2, true]");
}

#[test]
fn definition_list_remove_erases_the_instances() {
    let src = format!(
        "{ERR}
         model = Sketchup.active_model
         d = model.definitions.add('C'); d.entities.add_face([0,0,0],[1,0,0],[1,1,0])
         i = model.entities.add_instance(d, Geom::Transformation.new)
         r = model.definitions.remove(d)
         [r, d.valid?, i.valid?, model.definitions.length, err {{ model.definitions.remove(d) }}, err {{ model.definitions.remove(3) }}]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[true, false, false, 0, \"TypeError: reference to deleted ComponentDefinition\", \
         \"TypeError: wrong argument type (expected Sketchup::ComponentDefinition)\"]"
    );
}

#[test]
fn purge_unused_removes_definitions_nothing_uses() {
    let Some(r) = val("model = Sketchup.active_model
         used = model.definitions.add('Used'); model.entities.add_instance(used, Geom::Transformation.new)
         model.definitions.add('Free')
         r = model.definitions.purge_unused
         [r.equal?(model.definitions), model.definitions.map(&:name)]")
    else {
        return;
    };
    assert_eq!(r, "[true, [\"Used\"]]");
}

#[test]
fn unique_name_does_not_create_anything() {
    let Some(r) = val("defs = Sketchup.active_model.definitions
         defs.add('Chaise')
         [defs.unique_name('Chaise'), defs.unique_name('Table'), defs.length]")
    else {
        return;
    };
    assert_eq!(r, "[\"Chaise#1\", \"Table\", 1]");
}

#[test]
fn definition_identity_and_ordering() {
    let src = format!(
        "{ERR}
         defs = Sketchup.active_model.definitions
         a = defs.add('A'); b = defs.add('B')
         [a == defs['A'], a == b, a == 3, a.eql?(defs['A']), a.hash == defs['A'].hash, a <=> b, b <=> a, a <=> a, err {{ a <=> 3 }}, [b, a].sort.map(&:name)]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[true, false, false, true, true, -1, 1, 0, \"TypeError: wrong argument type (expected Sketchup::ComponentDefinition)\", [\"A\", \"B\"]]"
    );
}

#[test]
fn definition_name_clashes_get_a_suffix() {
    let Some(r) = val("defs = Sketchup.active_model.definitions
         a = defs.add('A'); b = defs.add('B')
         b.name = 'A'; a.name = 'A'
         [b.name, a.name]")
    else {
        return;
    };
    assert_eq!(r, "[\"A#1\", \"A\"]");
}

#[test]
fn definition_counts_and_instances() {
    let Some(r) = val(
        "model = Sketchup.active_model
         d = model.definitions.add('C'); d.entities.add_face([0,0,0],[1,0,0],[1,1,0])
         outer = model.definitions.add('Outer')
         top = model.entities.add_instance(d, Geom::Transformation.new)
         nested = outer.entities.add_instance(d, Geom::Transformation.new)
         [d.count_instances, d.count_used_instances, d.instances.size, d.instances.include?(top), d.instances.include?(nested),
          d.instances.first.class.name, outer.count_used_instances]",
    ) else {
        return;
    };
    // `outer` is never placed, so the instance inside it is not used.
    assert_eq!(r, "[2, 1, 2, true, true, \"Sketchup::ComponentInstance\", 0]");
}

#[test]
fn used_instances_count_every_placement_in_the_hierarchy() {
    let Some(r) = val("model = Sketchup.active_model
         leaf = model.definitions.add('Vis')
         box = model.definitions.add('Boîte')
         2.times { box.entities.add_instance(leaf, Geom::Transformation.new) }
         3.times { model.entities.add_instance(box, Geom::Transformation.new) }
         model.entities.add_instance(leaf, Geom::Transformation.new)
         [leaf.count_instances, leaf.count_used_instances, box.count_used_instances]")
    else {
        return;
    };
    // 2 in the box × 3 boxes + 1 at the top.
    assert_eq!(r, "[3, 7, 3]");
}

#[test]
fn definition_text_properties() {
    let Some(r) = val("d = Sketchup.active_model.definitions.add('Porte')
         d.description = 'Porte simple'
         [d.description, d.path, d.internal?, d.group?, d.image?, d.hidden?, d.live_component?, d.procedural?,
          d.guid == d.guid, d.guid =~ /\\A\\h{8}-\\h{4}-4\\h{3}-[89ab]\\h{3}-\\h{12}\\z/ && d.guid == d.guid.downcase && 36, d.invalidate_bounds, d.refresh_thumbnail, d.load_time.class.name]")
    else {
        return;
    };
    assert_eq!(r, "[\"Porte simple\", \"\", true, false, false, false, false, false, true, 36, true, nil, \"Time\"]");
}

#[test]
fn definition_is_a_drawingelement_and_an_entity() {
    let Some(r) = val("d = Sketchup.active_model.definitions.add('C')
         [d.is_a?(Sketchup::Drawingelement), d.is_a?(Sketchup::Entity), d.typename, d.valid?, d.model.class.name,
          d.parent.class.name, d.entityID.class.name]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[true, true, \"ComponentDefinition\", true, \"Sketchup::Model\", \"Sketchup::DefinitionList\", \"Integer\"]"
    );
}

#[test]
fn definition_entities_and_bounds() {
    let Some(r) = val(
        "d = Sketchup.active_model.definitions.add('C')
         d.entities.add_face([0,0,0],[2,0,0],[2,3,0],[0,3,0])
         b = d.bounds
         [d.entities.grep(Sketchup::Face).size, b.min.to_a, b.max.to_a.map { |v| v.round(9) }, d.entities.parent == d]",
    ) else {
        return;
    };
    assert_eq!(r, "[1, [0.0, 0.0, 0.0], [2.0, 3.0, 0.0], true]");
}

#[test]
fn manifold_depends_on_the_definitions_own_geometry() {
    let Some(r) = val("d = Sketchup.active_model.definitions.add('Cube')
         f = d.entities.add_face([0,0,0],[1,0,0],[1,1,0],[0,1,0])
         before = d.manifold?
         f.pushpull(-1)
         [before, d.manifold?]")
    else {
        return;
    };
    assert_eq!(r, "[false, true]");
}

#[test]
fn behavior_defaults_and_setters() {
    let src = format!(
        "{ERR}
         d = Sketchup.active_model.definitions.add('C')
         b = d.behavior
         before = [b.always_face_camera?, b.cuts_opening?, b.is2d?, b.no_scale_mask?, b.shadows_face_sun?, b.snapto]
         b.always_face_camera = true; b.cuts_opening = 1; b.is2d = true; b.no_scale_mask = 5; b.shadows_face_sun = true; b.snapto = 2
         after = [d.behavior.always_face_camera?, d.behavior.cuts_opening?, d.behavior.is2d?, d.behavior.no_scale_mask?, d.behavior.shadows_face_sun?, d.behavior.snapto]
         b.is2d = nil
         [before, after, b.is2d?, err {{ b.snapto = 4 }}, err {{ b.snapto = -1 }}, b.snapto, Sketchup::Behavior.instance_method(:snapto).arity]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[[false, false, false, 0, false, 0], [true, true, true, 5, true, 2], false, \"ArgumentError: Snapto value out of range 0..3\", \
         \"ArgumentError: Snapto value out of range 0..3\", 2, 0]"
    );
}

#[test]
fn snapto_constants_exist_at_top_level() {
    let Some(r) = val("[SnapTo_Arbitrary, SnapTo_Horizontal, SnapTo_Vertical, SnapTo_Sloped]") else { return };
    assert_eq!(r, "[0, 1, 2, 3]");
}

#[test]
fn insertion_point_moves_the_origin_without_moving_the_instances() {
    let src = format!(
        "{ERR}
         model = Sketchup.active_model
         d = model.definitions.add('C')
         d.entities.add_face([1,1,0],[3,1,0],[3,3,0],[1,3,0])
         i = model.entities.add_instance(d, Geom::Transformation.translation([10, 0, 0]))
         before = i.bounds.min.to_a
         d.insertion_point = Geom::Point3d.new(1, 1, 0)
         [before, i.bounds.min.to_a, d.entities.grep(Sketchup::Face).first.vertices.map {{ |v| v.position.to_a }}.min,
          d.insertion_point.to_a, err {{ d.insertion_point = 'x' }}, (d.insertion_point = [0, 0, 0]).class.name]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[[11.0, 1.0, 0.0], [11.0, 1.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], \
         \"ArgumentError: Wrong argument type - expected Point3d.\", \"Array\"]"
    );
}

#[test]
fn procedural_definitions_raise_the_documented_errors() {
    let src = format!(
        "{ERR}
         model = Sketchup.active_model
         d = model.definitions.add('C'); g = model.entities.add_group
         [err {{ d.control_entities }}, err {{ d.set_procedure_parameters({{}}) }}, err {{ d.attach_procedure('x', {{}}) }},
          err {{ g.definition.attach_procedure('x', {{}}) }}]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[\"TypeError: Control elements can only be accessed for procedural components.\", \
         \"TypeError: This definition is not procedural.\", \"ArgumentError: Failed to attach procedure.\", \
         \"TypeError: Cannot attach a procedure to this definition.\"]"
    );
}

#[test]
fn merge_dissolves_edges_between_coplanar_faces() {
    let Some(r) = val("d = Sketchup.active_model.definitions.add('C')
         d.entities.add_face([0,0,0],[1,0,0],[1,1,0],[0,1,0])
         d.entities.add_face([1,0,0],[2,0,0],[2,1,0],[1,1,0])
         before = d.entities.grep(Sketchup::Face).size
         r = d.merge
         [before, d.entities.grep(Sketchup::Face).size, r]")
    else {
        return;
    };
    assert_eq!(r, "[2, 1, true]");
}

#[test]
fn merge_rejects_a_bad_first_argument() {
    let src = format!(
        "{ERR}
         d = Sketchup.active_model.definitions.add('C')
         [err {{ d.merge(5) }}, err {{ d.merge([]) }}]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(r, "[\"RuntimeError: Error : merge - First argument must be an entity array or nil\", \"ok\"]");
}

#[test]
fn save_copy_and_load_round_trip_a_definition() {
    let dir = std::env::temp_dir().join(format!("esq_defs_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Chaise.esq").to_string_lossy().replace('\\', "/");
    let src = format!(
        "{ERR}
         model = Sketchup.active_model
         d = model.definitions.add('Chaise')
         d.entities.add_face([0,0,0],[2,0,0],[2,2,0],[0,2,0]).pushpull(-1)
         ok = d.save_copy({path:?})
         loaded = model.definitions.load({path:?})
         r = [ok, loaded.name, loaded.entities.grep(Sketchup::Face).size, loaded.path == {path:?}, loaded.internal?, loaded.manifold?]
         r << err {{ model.definitions.load('/nope/missing.skp') }}
         r << err {{ model.definitions.load('{dir}/readme.txt') }}
         r",
        dir = dir.to_string_lossy().replace('\\', "/")
    );
    std::fs::write(dir.join("readme.txt"), "x").unwrap();
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[true, \"Chaise#1\", 6, true, false, true, \"IOError: Invalid component file\", \"IOError: Invalid component file\"]"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_as_remembers_the_file() {
    let dir = std::env::temp_dir().join(format!("esq_defs_as_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Table.esq").to_string_lossy().replace('\\', "/");
    let src = format!(
        "d = Sketchup.active_model.definitions.add('Table')
         d.entities.add_face([0,0,0],[1,0,0],[1,1,0])
         [d.save_as({path:?}), d.path == {path:?}, d.internal?, (d.save_as rescue :argerr)]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(r, "[true, true, false, :argerr]");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_from_url_needs_an_url() {
    let src = format!(
        "{ERR}
         defs = Sketchup.active_model.definitions
         [err {{ defs.load_from_url }}, err {{ defs.load_from_url('ftp://example/x.skp') }}]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(r, "[\"RuntimeError: Usage: load_from_url url [callback]\", \"IOError: Invalid component URL\"]");
}

#[test]
fn import_reads_obj_and_stl_and_checks_the_file() {
    let dir = std::env::temp_dir().join(format!("esq_defs_imp_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let d = dir.to_string_lossy().replace('\\', "/");
    std::fs::write(dir.join("tri.obj"), "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
    std::fs::write(dir.join("a.skp"), "x").unwrap();
    std::fs::write(dir.join("a.zzz"), "x").unwrap();
    std::fs::write(
        dir.join("tri.stl"),
        "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid\n",
    )
    .unwrap();
    let src = format!(
        "{ERR}
         defs = Sketchup.active_model.definitions
         o = defs.import('{d}/tri.obj'); s = defs.import('{d}/tri.stl')
         [o.name, o.entities.grep(Sketchup::Face).size, s.name, s.entities.grep(Sketchup::Face).size,
          err {{ defs.import('{d}/none.obj') }}, err {{ defs.import('{d}/a.skp') }}, err {{ defs.import('{d}/a.zzz') }}]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[\"tri\", 1, \"tri#1\", 1, \"IOError: File not found\", \"ArgumentError: Skp files not supported. Use DefinitionList#load instead.\", \
         \"ArgumentError: No importer found for file extension\"]"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn group_definitions_report_their_kind() {
    let Some(r) = val(
        "g = Sketchup.active_model.entities.add_group
         [g.definition.group?, g.definition.name =~ /\\AGroupe#\\d+\\z/ ? :named : g.definition.name, g.definition.instances == [g]]",
    ) else {
        return;
    };
    assert_eq!(r, "[true, :named, true]");
}

/// `ComponentDefinition#guid` and `ComponentInstance#guid` (`Group#guid`) give the GUIDs the file
/// holds, as SketchUp does (the definition's, hyphenated; and
/// the instance's, its 128 bits in 22 base-64 digits), checked
/// against SketchUp's SDK (`SUComponentDefinitionGetGuid`, `SUComponentInstanceGetGuid`) on a
/// file of each format (2026 and an MFC archive of 2017) read natively; a copy gets a GUID of its
/// own.
#[test]
fn guids_are_those_the_file_holds() {
    let Some(i) = interp() else { return };
    let dir = std::env::temp_dir().join(format!("esq-rb-guid-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, version) in [("a2026.skp", None), ("a2017.skp", Some(esq_skp::SkpVersion::Su2017))] {
        let path = dir.join(name);
        let build = "m = Sketchup.active_model
             d = m.definitions.add('Caisse'); d.entities.add_face([0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0])
             d.entities.add_group.entities.add_line([0, 0, 0], [0, 0, 5])
             m.entities.add_instance(d, Geom::Transformation.new)
             m.entities.add_instance(d, Geom::Transformation.translation([20, 0, 0]))
             m.entities.add_group.entities.add_line([0, 0, 0], [1, 1, 1])";
        let mut built = Model::new();
        let out = i.run(build, "build.rb", &mut built, &mut Vec::new()).unwrap();
        assert!(out.ok, "{}", out.error);
        esq_skp::export_skp_version(&built, &path, None, version).unwrap();
        let mut m = esq_skp::native::read_file(&path).unwrap().0;
        assert!(m.defs.values().any(|d| !d.ext.guid.is_empty()));
        let code = "m = Sketchup.active_model
             insts = ([m.entities] + m.definitions.map(&:entities)).flat_map { |e|
               e.to_a.select { |x| x.is_a?(Sketchup::ComponentInstance) || x.is_a?(Sketchup::Group) } }
             defs = m.definitions.reject(&:image?).select { |d| d.instances.size > 0 }
             c = insts.first.copy
             forms = defs.all? { |d| d.guid =~ /\\A\\h{8}-\\h{4}-\\h{4}-\\h{4}-\\h{12}\\z/ && d.guid == d.guid.downcase }
             raise 'forme' unless forms
             # (The SDK gives a definition's GUID in the 22-character form.)
             [defs.map { |d| Esq.ifc_of_guid(d.guid) }.sort, insts.map(&:guid).sort, c.guid != insts.first.guid, c.guid.size]";
        let out = i.run(code, "guid.rb", &mut m, &mut Vec::new()).unwrap();
        assert!(out.ok, "{}", out.error);
        assert!(out.result.ends_with(", true, 22]"), "{}", out.result);
        if esq_skp::available() {
            let (defs, insts) = esq_skp::sdk_guids(&path).unwrap();
            assert_eq!((defs.len(), insts.len()), (3, 4), "{name}");
            let q = |v: &[String]| v.iter().map(|s| format!("{s:?}")).collect::<Vec<_>>().join(", ");
            assert_eq!(out.result, format!("[[{}], [{}], true, 22]", q(&defs), q(&insts)), "{name}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
