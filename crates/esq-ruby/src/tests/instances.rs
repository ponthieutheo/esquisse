use super::*;

mod definitions;
mod groups;
mod paths;

/// Runs `code`, panics with the Ruby error when it fails, and returns the `inspect` of its value.
fn val(code: &str) -> Option<String> {
    must(code).map(|(o, _)| o.result)
}

#[test]
fn attributes_round_trip_every_kind_of_value() {
    let Some(r) = val(
        "g = Sketchup.active_model.entities.add_group
         t = Time.at(1_700_000_000, 5, :nsec)
         vals = [1, 2.5, 'texte', true, false, [1, 'a', [2.0]], Sketchup::Color.new(1, 2, 3), Geom::Point3d.new(1, 2, 3), Geom::Vector3d.new(4, 5, 6), t]
         vals.each_with_index { |v, i| g.set_attribute('d', \"k#{i}\", v) }
         out = vals.each_index.map { |i| g.get_attribute('d', \"k#{i}\") }
         [out[0], out[0].class, out[1], out[2], out[3], out[4], out[5], out[6].to_a, out[7].to_a, out[8].to_a, out[9] == t]",
    ) else {
        return;
    };
    assert_eq!(
        r,
        "[1, Integer, 2.5, \"texte\", true, false, [1, \"a\", [2.0]], [1, 2, 3, 255], [1.0, 2.0, 3.0], [4.0, 5.0, 6.0], true]"
    );
}

#[test]
fn set_attribute_returns_the_value_and_get_attribute_falls_back_to_the_default() {
    let Some(r) = val("g = Sketchup.active_model.entities.add_group
         [g.set_attribute('a', 'b', 42), g.get_attribute('a', 'b'), g.get_attribute('a', 'zzz', 'def'),
          g.get_attribute('nope', 'b', :dflt), g.get_attribute('a', 'zzz')]")
    else {
        return;
    };
    assert_eq!(r, "[42, 42, \"def\", :dflt, nil]");
}

#[test]
fn nil_and_hash_values_are_stored_as_empty() {
    let Some(r) = val("g = Sketchup.active_model.entities.add_group
         g.set_attribute('a', 'n', nil); g.set_attribute('a', 'h', { x: 1 })
         [g.get_attribute('a', 'n', 7), g.get_attribute('a', 'h', 8), g.attribute_dictionary('a').keys]")
    else {
        return;
    };
    assert_eq!(r, "[7, 8, [\"h\", \"n\"]]");
}

#[test]
fn attribute_argument_errors_use_sketchups_messages() {
    let Some(r) = val(
        "g = Sketchup.active_model.entities.add_group
         def err; yield; 'ok'; rescue Exception => e; \"#{e.class}: #{e.message}\"; end
         [err { g.get_attribute('a') }, err { g.set_attribute('a', 'b') }, err { g.set_attribute('a', '', 1) },
          err { g.delete_attribute }, err { g.attribute_dictionary }, err { g.set_attribute('SU_LiveComponent', 'x', 1) },
          err { g.attribute_dictionary('SU_PC', true) }, err { g.delete_attribute('SU_InstanceSet') }]",
    ) else {
        return;
    };
    assert_eq!(
        r,
        "[\"ArgumentError: Dictionary name and key required\", \"ArgumentError: Dictionary name, key and value required\", \
         \"ArgumentError: Key cannot be empty\", \"ArgumentError: Dictionary name required\", \"ArgumentError: Dictionary name required\", \
         \"ArgumentError: Cannot modify internal attribute dictionaries.\", \"ArgumentError: Cannot create a dictionary with a reserved name.\", \
         \"ok\"]"
    );
}

#[test]
fn names_and_keys_are_converted_with_to_s() {
    let Some(r) = val("g = Sketchup.active_model.entities.add_group
         g.set_attribute(:sym, 12, 'v')
         [g.get_attribute('sym', '12'), g.attribute_dictionaries.map(&:name)]")
    else {
        return;
    };
    assert_eq!(r, "[\"v\", [\"sym\"]]");
}

#[test]
fn attribute_dictionaries_are_nil_until_something_is_set_and_keep_creation_order() {
    let Some(r) = val("g = Sketchup.active_model.entities.add_group
         before = g.attribute_dictionaries
         g.set_attribute('zeta', 'a', 1); g.set_attribute('alpha', 'a', 1); g.set_attribute('mid', 'b', 2)
         ds = g.attribute_dictionaries
         [before, ds.size, ds.length, ds.map(&:name), ds['alpha'].class, ds['none'], ds.each { }]")
    else {
        return;
    };
    assert_eq!(r, "[nil, 3, 3, [\"zeta\", \"alpha\", \"mid\"], Sketchup::AttributeDictionary, nil, nil]");
}

#[test]
fn dictionary_keys_are_sorted_and_each_pair_yields_two_values() {
    let Some(r) = val(
        "g = Sketchup.active_model.entities.add_group
         g.set_attribute('d', 'b', 2); g.set_attribute('d', 'a', 1); g.set_attribute('d', 'c', 'x')
         d = g.attribute_dictionary('d')
         pairs = []; d.each_pair { |k, v| pairs << [k, v] }
         ks = []; d.each_key { |k| ks << k }
         [d.name, d.size, d.length, d.keys, d.values, pairs, ks, d.empty?, d['a'], d['zz'], d.each { }, d.each_key { }]",
    ) else {
        return;
    };
    assert_eq!(
        r,
        "[\"d\", 3, 3, [\"a\", \"b\", \"c\"], [1, 2, \"x\"], [[\"a\", 1], [\"b\", 2], [\"c\", \"x\"]], [\"a\", \"b\", \"c\"], false, 1, nil, nil, nil]"
    );
}

#[test]
fn dictionary_writes_and_deletions() {
    let Some(r) = val("g = Sketchup.active_model.entities.add_group
         d = g.attribute_dictionary('d', true)
         empty = d.empty?
         d['x'] = 1; d['y'] = [1, 2]
         first = d.delete_key('x'); again = d.delete_key('x')
         [empty, d['x'], first, again, d.keys, d.send(:[]=, 'z', 3).equal?(d)]")
    else {
        return;
    };
    assert_eq!(r, "[true, nil, 1, nil, [\"y\"], true]");
}

#[test]
fn dictionary_key_must_be_a_non_empty_string() {
    let Some(r) = val("g = Sketchup.active_model.entities.add_group
         d = g.attribute_dictionary('d', true)
         def err; yield; 'ok'; rescue Exception => e; \"#{e.class}: #{e.message}\"; end
         [err { d[''] }, err { d['' ] = 1 }, err { d.delete_key('') }, err { d[nil] }]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[\"ArgumentError: String expected\", \"ArgumentError: String expected\", \"ArgumentError: String expected\", \"ArgumentError: String expected\"]"
    );
}

#[test]
fn delete_attribute_follows_sketchups_return_values() {
    let Some(r) = val("g = Sketchup.active_model.entities.add_group
         g.set_attribute('d', 'a', 1); g.set_attribute('d', 'b', 2)
         r = []
         r << g.delete_attribute('absent') << g.delete_attribute('d', 'zzz') << g.delete_attribute('d', 'a')
         r << g.attribute_dictionary('d').keys
         r << g.delete_attribute('d', 'b')
         r << g.attribute_dictionary('d')
         g.set_attribute('e', 'k', 1)
         r << g.delete_attribute('e') << g.attribute_dictionaries
         r")
    else {
        return;
    };
    assert_eq!(r, "[false, false, true, [\"b\"], true, nil, true, nil]");
}

#[test]
fn a_created_empty_dictionary_survives_until_a_key_is_deleted() {
    let Some(r) = val("g = Sketchup.active_model.entities.add_group
         d = g.attribute_dictionary('keep', true)
         [g.attribute_dictionary('keep').nil?, g.attribute_dictionaries.size, d.valid?]")
    else {
        return;
    };
    assert_eq!(r, "[false, 1, true]");
}

#[test]
fn deleted_dictionaries_raise_on_use() {
    let Some(r) = val("g = Sketchup.active_model.entities.add_group
         g.set_attribute('d', 'a', 1)
         d = g.attribute_dictionary('d')
         g.attribute_dictionaries.delete('d')
         ok = d.valid?
         begin; d.keys; rescue TypeError => e; [ok, e.message]; end")
    else {
        return;
    };
    assert_eq!(r, "[false, \"reference to deleted Attribute\"]");
}

#[test]
fn attribute_dictionaries_delete_validates_its_argument() {
    let Some(r) = val(
        "g = Sketchup.active_model.entities.add_group
         g.set_attribute('d', 'a', 1)
         ds = g.attribute_dictionaries
         def err; yield; 'ok'; rescue Exception => e; \"#{e.class}: #{e.message}\"; end
         [err { ds.delete(5) }, err { ds.delete('SU_PC') }, ds.delete('absent').equal?(ds), ds.delete(ds['d']).equal?(ds), g.attribute_dictionaries]",
    ) else {
        return;
    };
    assert_eq!(
        r,
        "[\"ArgumentError: wrong argument type.  Expected String or AttributeDictionary\", \"ok\", true, true, nil]"
    );
}

#[test]
fn attributes_work_on_edges_faces_materials_tags_the_model_and_definitions() {
    let Some(r) = val("m = Sketchup.active_model
         e = m.entities.add_line([0,0,0],[1,0,0])
         f = m.entities.add_face([0,0,0],[1,0,0],[1,1,0],[0,1,0])
         mat = m.materials.add('Brique'); tag = m.layers.add('Murs')
         d = m.definitions.add('Porte')
         [e, f, mat, tag, m, d].each_with_index { |o, i| o.set_attribute('t', 'k', i) }
         [e, f, mat, tag, m, d].map { |o| o.get_attribute('t', 'k') }")
    else {
        return;
    };
    assert_eq!(r, "[0, 1, 2, 3, 4, 5]");
}

#[test]
fn instance_and_definition_attributes_are_separate() {
    let Some((_, m)) = must(
        "model = Sketchup.active_model
         d = model.definitions.add('C')
         d.entities.add_face([0,0,0],[1,0,0],[1,1,0])
         i = model.entities.add_instance(d, Geom::Transformation.new)
         i.set_attribute('x', 'k', 'inst'); d.set_attribute('x', 'k', 'def')
         [i.get_attribute('x', 'k'), d.get_attribute('x', 'k')]",
    ) else {
        return;
    };
    let inst = m.root.instances.values().next().unwrap();
    assert!(inst.attributes.contains_key("x"));
    assert!(m.defs.values().any(|d| d.attributes.contains_key("x")));
}

#[test]
fn model_attribute_dictionaries_follow_the_model() {
    let Some(r) = val(
        "m = Sketchup.active_model
         m.set_attribute('Geo', 'Lat', 48.8)
         [m.get_attribute('Geo', 'Lat'), m.attribute_dictionary('Geo').keys, m.attribute_dictionaries.map(&:name), m.model_attribute_dictionaries.size]",
    ) else {
        return;
    };
    assert_eq!(r, "[48.8, [\"Lat\"], [\"Geo\"], 1]");
}

#[test]
fn integer_values_survive_a_save_of_the_model() {
    let Some((_, m)) = must("Sketchup.active_model.entities.add_group.set_attribute('d', 'n', 7)") else { return };
    let json = serde_json::to_string(&m).unwrap();
    let back: Model = serde_json::from_str(&json).unwrap();
    let inst = back.root.instances.values().next().unwrap();
    assert_eq!(inst.attributes["d"]["n"], esq_core::model::AttrValue::Json(serde_json::json!({"t": "i", "v": 7})));
}
