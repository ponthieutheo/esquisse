//! InstancePath, Sketchup::Set, classifications.
use super::*;

const ERR: &str = "def err; yield; 'ok'; rescue Exception => e; \"#{e.class}: #{e.message}\"; end";

/// Two nested groups and a face inside the inner one: `outer`, `inner`, `face`.
const NEST: &str = "model = Sketchup.active_model
outer = model.entities.add_group
inner = outer.entities.add_group
face = inner.entities.add_face([0,0,0],[1,0,0],[1,1,0])
outer.transformation = Geom::Transformation.translation([10, 0, 0])
inner.transformation = Geom::Transformation.translation([0, 5, 0])";

#[test]
fn instance_path_holds_instances_and_a_leaf() {
    let src = format!(
        "{NEST}
         p = Sketchup::InstancePath.new([outer, inner, face])
         q = Sketchup::InstancePath.new([outer, inner])
         [p.size, p.length, p.root == outer, p.leaf == face, p[2] == face, p.to_a.size, p.include?(inner), p.include?(3),
          q.size, q.leaf, q.root == outer, p.valid?, p.empty?, Sketchup::InstancePath.new([]).empty?]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(r, "[3, 3, true, true, true, 3, true, false, 2, nil, true, true, false, true]");
}

#[test]
fn instance_path_rejects_bad_paths() {
    let src = format!(
        "{ERR}
         {NEST}
         [err {{ Sketchup::InstancePath.new(outer) }}, err {{ Sketchup::InstancePath.new([inner]) }},
          err {{ Sketchup::InstancePath.new([outer, face]) }}, err {{ Sketchup::InstancePath.new([face, outer]) }},
          err {{ Sketchup::InstancePath.new([outer, 3]) }},
          Sketchup::InstancePath.new([inner]).valid?, Sketchup::InstancePath.new([outer, face]).valid?]"
    );
    let Some(r) = val(&src) else { return };
    // Comme SketchUp, un chemin mal emboîté se construit ; valid? refuse la face
    // hors de la définition d'outer mais accepte une instance seule, où qu'elle soit.
    assert_eq!(
        r,
        "[\"TypeError: wrong argument type Sketchup::Group (expected Array)\", \"ok\", \"ok\", \
         \"ArgumentError: invalid instance path\", \"ArgumentError: invalid instance path\", true, false]"
    );
}

#[test]
fn instance_path_transformation_multiplies_the_instances() {
    let src = format!(
        "{ERR}
         {NEST}
         p = Sketchup::InstancePath.new([outer, inner, face])
         [p.transformation.origin.to_a, p.transformation(0).origin.to_a, p.transformation(1).origin.to_a,
          p.transformation(2).origin.to_a, err {{ p.transformation(3) }}, err {{ p.transformation(-1) }}, err {{ p[5] }}, err {{ p[-1] }}]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[[10.0, 5.0, 0.0], [10.0, 0.0, 0.0], [10.0, 5.0, 0.0], [10.0, 5.0, 0.0], \"IndexError: index out of bounds\", \
         \"IndexError: index out of bounds\", \"IndexError: index out of bounds\", \"IndexError: index out of bounds\"]"
    );
}

#[test]
fn instance_path_push_pop_and_leaf() {
    let src = format!(
        "{ERR}
         {NEST}
         p = Sketchup::InstancePath.new([])
         r = []
         r << p.push(outer).equal?(p) << p.push(inner).size << p.leaf
         r << p.push(face).leaf.equal?(face)
         r << err {{ p.push(face) }} << err {{ p.push(3) }}
         r << (p.pop == face) << p.size << (p.pop == inner) << p.size
         p.leaf = face
         r << p.size << (p.leaf == face) << err {{ p.leaf = 3 }} << (p.leaf = nil) << p.size
         p.clear
         r << p.pop << p.empty?
         r"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[true, 2, nil, true, \"ArgumentError: cannot push to a path that already has a leaf entity\", \
         \"TypeError: wrong argument type (expected Sketchup::Entity)\", true, 2, true, 1, 2, true, \
         \"TypeError: wrong argument type (expected Sketchup::Entity)\", nil, 1, nil, true]"
    );
}

#[test]
fn instance_path_copies_compare_and_persistent_ids() {
    let src = format!(
        "{NEST}
         p = Sketchup::InstancePath.new([outer, inner, face])
         c = p.clone; d = p.dup; e = Sketchup::InstancePath.new([outer])
         e.copy(p)
         c.pop
         [p == d, p == c, p == e, p.size, c.size, p == 3, p.persistent_id_path == [outer, inner, face].map(&:persistent_id).join('.'),
          (r = []; p.each {{ |x| r << x }}).nil?, r.size]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(r, "[true, false, true, 3, 2, false, true, true, 3]");
}

#[test]
fn instance_path_becomes_invalid_when_an_entity_goes() {
    let src = format!(
        "{NEST}
         p = Sketchup::InstancePath.new([outer, inner, face])
         face.erase!
         p.valid?"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(r, "false");
}

#[test]
fn sketchup_set_is_an_identity_set() {
    let Some(r) = val("s = Sketchup::Set.new
         a = 'x'; b = 'x'
         r = [s.empty?, s.insert(3, 1, 2, 1).equal?(s), s.size, s.to_a]
         s.clear
         s.insert(a, a, b)
         r += [s.length, s.include?(a), s.contains?('x'), s.delete(a).equal?(a), s.delete(a), s.size]
         got = []; ret = s.each { |x| got << x }
         r += [ret, got.size, s.clear.equal?(s), s.empty?, Sketchup::Set.new.respond_to?(:map)]
         r")
    else {
        return;
    };
    assert_eq!(r, "[true, true, 3, [1, 2, 3], 2, true, false, true, nil, 1, nil, 1, true, true, true]");
}

#[test]
fn sketchup_set_takes_no_constructor_argument() {
    let src = format!("{ERR}\n err {{ Sketchup::Set.new([1]) }}");
    let Some(r) = val(&src) else { return };
    assert_eq!(r, "\"ArgumentError: wrong number of arguments (given 1, expected 0)\"");
}

/// A `.skc` archive (zip, stored entries) with a two-type schema, `Wall` having a `Name`.
fn write_skc(path: &std::path::Path) {
    write_skc_with(
        path,
        "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:t=\"http://example.org/t1\" \
         targetNamespace=\"http://example.org/t1\">\
         <xs:element substitutionGroup=\"t:Base\" name=\"Wall\" type=\"t:Wall\"></xs:element>\
         <xs:complexType name=\"Wall\"><xs:sequence><xs:element name=\"Name\" type=\"xs:string\"/></xs:sequence></xs:complexType>\
         <xs:element substitutionGroup=\"t:Root\" name=\"Base\" type=\"t:Base\"></xs:element></xs:schema>",
    );
}

/// A `.skc` whose two types are each other's parent: `load_schema` went round the cycle
/// forever (`while (t = parents[t])`).
#[test]
fn a_cycle_of_parent_types_ends() {
    let dir = std::env::temp_dir().join(format!("esq_skc_cycle_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let skc = dir.join("boucle.skc");
    write_skc_with(
        &skc,
        "<xs:schema targetNamespace=\"http://example.org/t1\">\
         <xs:element substitutionGroup=\"t:Base\" name=\"Wall\" type=\"t:Wall\"></xs:element>\
         <xs:element substitutionGroup=\"t:Wall\" name=\"Base\" type=\"t:Base\"></xs:element></xs:schema>",
    );
    let p = skc.to_string_lossy().replace('\\', "/");
    let Some(r) = val(&format!("Sketchup.active_model.classifications.load_schema({p:?})")) else { return };
    assert_eq!(r, "true");
    let _ = std::fs::remove_dir_all(&dir);
}

fn write_skc_with(path: &std::path::Path, xsd: &str) {
    let files: [(&str, &str); 3] = [
        (
            "documentProperties.xml",
            "<?xml version=\"1.0\"?><documentProperties><dp:title>Test 1</dp:title></documentProperties>",
        ),
        ("Schemas/T1.xsd", xsd),
        ("Schemas/T1.xsd.filter", "Wall\n{\n  Name\n}\n\n// Blacklist Attributes\n{\n  cType\n}\n"),
    ];
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    for (name, body) in files {
        let off = out.len() as u32;
        le32(&mut out, 0x0403_4b50);
        for x in [20u16, 0, 0, 0, 0] {
            le16(&mut out, x);
        }
        le32(&mut out, 0);
        le32(&mut out, body.len() as u32);
        le32(&mut out, body.len() as u32);
        le16(&mut out, name.len() as u16);
        le16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(body.as_bytes());
        le32(&mut central, 0x0201_4b50);
        for x in [20u16, 20, 0, 0, 0, 0] {
            le16(&mut central, x);
        }
        le32(&mut central, 0);
        le32(&mut central, body.len() as u32);
        le32(&mut central, body.len() as u32);
        le16(&mut central, name.len() as u16);
        for x in [0u16, 0, 0, 0] {
            le16(&mut central, x);
        }
        le32(&mut central, 0);
        le32(&mut central, off);
        central.extend_from_slice(name.as_bytes());
    }
    let cd_off = out.len() as u32;
    let cd_len = central.len() as u32;
    out.extend_from_slice(&central);
    le32(&mut out, 0x0605_4b50);
    for x in [0u16, 0, 3, 3] {
        le16(&mut out, x);
    }
    le32(&mut out, cd_len);
    le32(&mut out, cd_off);
    le16(&mut out, 0);
    std::fs::write(path, out).unwrap();
}

#[test]
fn classifications_load_schemas_and_classify_definitions() {
    let dir = std::env::temp_dir().join(format!("esq_skc_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let skc = dir.join("t1.skc");
    write_skc(&skc);
    let p = skc.to_string_lossy().replace('\\', "/");
    let src = format!(
        "{ERR}
         model = Sketchup.active_model
         cl = model.classifications
         r = [cl.load_schema({p:?}), cl.load_schema('/nope.skc'), cl.keys, cl.size, cl.length]
         s = cl['Test 1']
         r += [s.class.name, s.name, s.namespace, cl[0].name, cl[-1].name, cl[1], cl['nope'], (s <=> cl[0]), (s <=> 3)]
         got = []; r << cl.each {{ |x| got << x.name }} << got
         d = model.definitions.add('Mur')
         r << d.add_classification('Test 1', 'Wall') << d.add_classification('Test 1', 'Base') << d.add_classification('Test 1', 'Door')
         r << d.add_classification('Absent', 'Wall')
         d.add_classification('Test 1', 'Wall')
         r << d.set_classification_value(['Test 1', 'Wall', 'Name'], 'M1') << d.get_classification_value(['Test 1', 'Wall', 'Name'])
         r << d.get_classification_value(['Autre', 'X']) << err {{ d.get_classification_value('Test 1') }}
         r << err {{ d.remove_classification }} << d.remove_classification('Test 1', 'Door') << d.remove_classification('Test 1')
         r << d.get_classification_value(['Test 1', 'Wall', 'Name']) << d.remove_classification('Test 1')
         r << cl.unload_schema('Test 1') << cl.keys
         r"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(
        r,
        "[true, false, [\"Test 1\"], 1, 1, \"Sketchup::ClassificationSchema\", \"Test 1\", \"http://example.org/t1\", \"Test 1\", \"Test 1\", nil, nil, \
         0, nil, nil, [\"Test 1\"], true, true, false, false, true, \"M1\", nil, \"TypeError: wrong argument type String (expected Array)\", \
         \"ArgumentError: Invalid number of arguments.\", false, true, nil, false, true, []]"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn loaded_schemas_belong_to_the_model() {
    let dir = std::env::temp_dir().join(format!("esq_skc2_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let skc = dir.join("t1.skc");
    write_skc(&skc);
    let p = skc.to_string_lossy().replace('\\', "/");
    let Some((_, m)) = must(&format!("Sketchup.active_model.classifications.load_schema({p:?})")) else { return };
    // Another model does not see it, the model that loaded it keeps it (and saves it).
    let Some(r) = val("Sketchup.active_model.classifications.keys") else { return };
    assert_eq!(r, "[]");
    assert!(m.attr_store.get("classifications").is_some_and(|a| a["schemas"].contains_key("Test 1")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn value_objects_inspect_like_sketchup() {
    let Some(r) = val(
        "[Sketchup::Set.new.inspect, Sketchup::InstancePath.new([]).inspect, Sketchup.active_model.classifications.inspect]
          .map { |s| s.sub(/0x\\h+/, 'X') }",
    ) else {
        return;
    };
    assert_eq!(r, "[\"#<Sketchup::Set:X>\", \"#<Sketchup::InstancePath:X>\", \"#<Sketchup::Classifications:X>\"]");
}

#[test]
fn attributes_of_a_deleted_edge_raise() {
    let src = format!(
        "{ERR}
         e = Sketchup.active_model.entities.add_line([0,0,0],[1,0,0])
         e.set_attribute('d', 'k', 1)
         e.erase!
         [err {{ e.get_attribute('d', 'k') }}, err {{ e.attribute_dictionaries }}]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(r, "[\"TypeError: reference to deleted Edge\", \"TypeError: reference to deleted Edge\"]");
}

/// The IFC schemas that come with Esquisse (buildingSMART's, in `Classifications` of the
/// content) load like SketchUp's; the attributes listed after a type are not types.
#[test]
fn ifc_schemas_shipped_with_esquisse_load() {
    let dir = esq_core::content::folder("Classifications").expect("classements livrés avec Esquisse");
    for (file, name, ns) in [
        ("IFC 2x3.skc", "IFC 2x3", "http://www.iai-tech.org/ifcXML/IFC2x3/FINAL"),
        ("IFC 4.skc", "IFC 4", "https://standards.buildingsmart.org/IFC/RELEASE/IFC4/Add2TC1"),
    ] {
        let p = dir.join(file).to_string_lossy().replace('\\', "/");
        let src = format!(
            "cl = Sketchup.active_model.classifications
             ok = cl.load_schema({p:?})
             d = Sketchup.active_model.definitions.add('Mur')
             [ok, cl[{name:?}].namespace, d.add_classification({name:?}, 'IfcWall'), d.add_classification({name:?}, 'IfcDoor'), d.add_classification({name:?}, 'Name')]"
        );
        let Some(r) = val(&src) else { return };
        assert_eq!(r, format!("[true, {ns:?}, true, true, false]"), "{file}");
    }
}

#[test]
fn shipped_ifc_schema_loads_when_sketchup_is_installed() {
    let skc =
        std::path::Path::new(r"C:\Program Files\SketchUp\SketchUp 2026\SketchUp\ShippedClassifications\IFC 2x3.skc");
    if !skc.is_file() {
        return;
    }
    let p = skc.to_string_lossy().replace('\\', "/");
    let src = format!(
        "cl = Sketchup.active_model.classifications
         ok = cl.load_schema({p:?})
         s = cl['IFC 2x3']
         d = Sketchup.active_model.definitions.add('Mur')
         [ok, s.namespace, d.add_classification('IFC 2x3', 'IfcWall'), d.add_classification('IFC 2x3', 'IfcBuildingElement'), d.add_classification('IFC 2x3', 'IfcNothing')]"
    );
    let Some(r) = val(&src) else { return };
    assert_eq!(r, "[true, \"http://www.iai-tech.org/ifcXML/IFC2x3/FINAL\", true, true, false]");
}
