//! Écarts trouvés par la suite officielle de TestUp (github.com/SketchUp/testup-2), deuxième
//! passe (`docs/ruby-reel.md`, vague 11). Chaque test reprend un écart avec un extrait écrit pour
//! Esquisse ; le comportement attendu est celui que SketchUp.exe code (adresses).

use super::*;

/// `err { … }` : la classe et le message de ce que lève le bloc (nil s'il ne lève rien).
const HELPERS: &str = "def err; yield; nil; rescue Exception => e; \"#{e.class}: #{e.message}\"; end
def cls; yield; nil; rescue Exception => e; e.class; end
";

fn ev(code: &str) -> Option<String> {
    must(&format!("{HELPERS}{code}")).map(|(o, _)| o.result)
}

/// Les styles de ligne étaient les cinq de SketchUp 2018 (« Solid Line »…) : SketchUp 2026 en a
/// douze, rendus toujours par le même objet ; `[]` prend un nom ou un index négatif et lève
/// TypeError sur nil; une étiquette neuve n'a pas de style (nil), nil l'enlève,
/// un autre objet lève TypeError.
#[test]
fn line_styles_are_those_of_sketchup_2026() {
    let Some(r) = ev("ls = Sketchup.active_model.line_styles
         l = Sketchup.active_model.layers.add('Murs')
         r = [ls.size, ls.names.first, ls.names.last, ls['Dot'].name, ls[-1].name, ls['Rien'],
              ls[0].equal?(ls['Solid Basic']), ls.to_a.map(&:name) == ls.names, cls { ls[nil] }, cls { ls[0, 1] },
              err { ls[12] }, l.line_style]
         l.line_style = ls['Dash']
         r += [l.line_style.name, l.line_style.equal?(ls['Dash']), cls { l.line_style = 'Dash' }]
         l.line_style = nil
         r << l.line_style")
    else {
        return;
    };
    assert_eq!(
        r,
        "[12, \"Solid Basic\", \"Long-dash double-dash\", \"Dot\", \"Long-dash double-dash\", nil, true, true, \
         TypeError, ArgumentError, \"IndexError: index 12 out of range [0, 12)\", nil, \"Dash\", true, TypeError, nil]"
    );
}

/// `Layers` comme SketchUp :
/// `at` par nom (« Untagged » et « layer0 » sont l'étiquette par défaut) ou par index (TypeError
/// pour nil ou un point), `remove` par nom et par la conversion en étiquette (ArgumentError pour
/// une étiquette effacée ou un autre objet), `unique_name` à partir de « Tag », `purge_unused`
/// qui compte, `add_folder` qui déplace un dossier existant ou lève TypeError, `remove_folder`
/// qui rend nil.
#[test]
fn layers_are_reached_and_removed_like_sketchup() {
    let Some(r) = ev("m = Sketchup.active_model
         ls = m.layers
         r = [ls.unique_name, ls.unique_name('Layer0'), ls.at('Untagged') == ls[0], ls.at('layer0') == ls[0],
              cls { ls.at(nil) }, cls { ls.at(ORIGIN) }]
         a = ls.add('Murs')
         r += [ls.add('Murs').equal?(a), ls.unique_name('Murs'), ls[-1] == a]
         ls.add('Toit')
         r << ls.remove('Toit') << ls['Toit']
         r += [err { ls.remove(ls['Toit'] || a.tap { ls.remove(a) }) }, cls { ls.remove(ORIGIN) }, cls { ls.remove(nil) }]
         ls.add('A'); ls.add('B')
         r << ls.purge_unused << ls.size
         f1 = ls.add_folder('Un'); f2 = f1.add_folder('Deux')
         r += [ls.add_folder(f2).equal?(f2), f2.folder, cls { ls.add_folder(nil) }, err { ls.add_folder('') },
               cls { ls.remove_folder(nil) }, ls.remove_folder(f2), ls.count_folders, ls.purge_unused_folders]
         r")
    else {
        return;
    };
    assert_eq!(
        r,
        "[\"Tag\", \"Layer01\", true, true, TypeError, TypeError, true, \"Murs1\", true, true, nil, \
         \"ArgumentError: No implicit conversion to tag\", ArgumentError, ArgumentError, 2, 1, true, nil, TypeError, \
         \"ArgumentError: Folder name cannot be empty\", TypeError, nil, 1, 1]"
    );
}

/// `Layer` comme SketchUp : comparable par le nom affiché sans la casse, l'autre côté passé par
/// la conversion en étiquette ; `color=` par la conversion de Color (nil :
/// TypeError) ; `page_behavior=` en entier non signé ; cachée, l'étiquette active
/// laisse la place à l'étiquette par défaut et la sélection perd ce qui est dessus
#[test]
fn layers_compare_take_colours_and_hide_like_sketchup() {
    let Some(r) = ev("m = Sketchup.active_model
         ls = m.layers
         b = ls.add('bacon'); v = ls.add('Vinegar')
         r = [Sketchup::Layer.include?(Comparable), ls[0] <=> b, b <=> ls[0], ls[0] <=> v, ls[0] <=> 'Vinegar',
              cls { ls[0] <=> 1 }, err { ls[0] <=> 'steak' }, cls { b.color = nil }, cls { b.color = 'porkfat' },
              cls { b.page_behavior = 'x' }, cls { b.page_behavior = -1 }, cls { b.page_behavior = nil }]
         b.color = Sketchup::Color.new(1, 2, 3, 4)
         r << b.color.to_a
         m.active_layer = b
         f = m.entities.add_face([0, 0, 0], [9, 0, 0], [9, 9, 0])
         f.layer = b
         m.selection.add(m.entities.to_a)
         n = m.selection.size
         b.visible = false
         r += [m.active_layer == ls[0], n, m.selection.size, m.selection.include?(f)]
         ls[0].visible = false
         r << (m.active_layer == ls[0])")
    else {
        return;
    };
    assert_eq!(
        r,
        "[true, 1, -1, -1, -1, ArgumentError, \"ArgumentError: Cannot find tag named \\\"steak\\\"\", TypeError, \
         ArgumentError, TypeError, RangeError, TypeError, [1, 2, 3, 255], true, 4, 0, false, true]"
    );
}

/// `LayerFolder` comme SketchUp : les noms en double sont permis (chaîne exigée,
/// TypeError sinon), un dossier caché garde ses étiquettes et ses sous-dossiers visibles
///  tout en cachant ce qu'ils portent, `parent` est le dossier qui le contient,
/// `add_layer` et `remove_layer` rendent nil, `remove_folder` fait remonter le
/// contenu, `<=>` rend nil face à un dossier effacé, `each_folder` avertit quand la
/// liste rétrécit pendant le parcours.
#[test]
fn layer_folders_behave_like_sketchup() {
    let Some(r) = ev("m = Sketchup.active_model
         ls = m.layers
         a = ls.add_folder('Hello'); b = ls.add_folder('Hello'); w = a.add_folder('World')
         t = ls.add('T'); e = m.entities.add_line([0, 0, 0], [9, 9, 9]); e.layer = t
         r = [a.name, b.name, a == b, cls { a.name = 1 }, cls { a.name = nil }, a.parent == ls, w.parent == a,
              w.add_layer(t), t.folder == w, m.drawing_element_visible?([e])]
         a.visible = false
         r += [a.visible?, w.visible?, t.visible?, m.drawing_element_visible?([e])]
         a.visible = true
         r += [a.remove_folder(w), t.folder == a, a.remove_layer(t), t.folder, cls { a.remove_folder('x') }]
         ls.remove_folder(b)
         r += [a <=> b, a <=> a, cls { a.add_folder(3) }]
         3.times { |i| a.add_folder(\"F#{i}\") }
         require 'stringio'
         old = $stderr; $stderr = StringIO.new
         a.each_folder { |f| a.remove_folder(f) if f.name == 'F1' }
         r << $stderr.string.include?(\"don't modify the collection while iterating\")
         $stderr = old
         r")
    else {
        return;
    };
    assert_eq!(
        r,
        "[\"Hello\", \"Hello\", false, TypeError, TypeError, true, true, nil, true, true, false, true, true, false, \
         nil, true, nil, nil, TypeError, nil, 0, TypeError, true]"
    );
}

/// Un dossier temporaire à soi pour un test.
fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("esq_reel2_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn rb_path(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// `ImageRep` comme SUImageRep : vide, tout vaut 0 et `data`, `colors`, `color_at_uv` rendent nil ;
/// chargée, 24 bits par pixel (32 avec une transparence), lignes depuis le haut en BGR, alignées
/// sur 4 octets (`row_padding` compte les octets ajoutés, `size` les octets) ; GIF et TGA se
/// lisent ; `color_at_uv` répète u et v, v monte depuis le bas ; `set_data`
/// vérifie ses entiers non signés, sa chaîne et sa taille.
#[test]
fn image_reps_keep_pixels_like_sketchup() {
    let dir = scratch("imagerep");
    // 17 × 17 : bleu en haut à gauche, vert en haut à droite, violet en bas à gauche, rouge
    // en bas à droite.
    let quad = image::RgbImage::from_fn(17, 17, |x, y| {
        image::Rgb(match (x < 9, y < 9) {
            (true, true) => [0, 0, 255],
            (false, true) => [0, 255, 0],
            (true, false) => [185, 0, 255],
            (false, false) => [255, 0, 0],
        })
    });
    quad.save(dir.join("quad.png")).unwrap();
    quad.save(dir.join("quad.gif")).unwrap();
    quad.save(dir.join("quad.tga")).unwrap();
    image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 4])).save(dir.join("alpha.png")).unwrap();
    image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255])).save(dir.join("opaque.png")).unwrap();
    image::GrayImage::from_pixel(3, 2, image::Luma([7])).save(dir.join("gris.png")).unwrap();
    std::fs::write(dir.join("vide.png"), b"").unwrap();
    let d = rb_path(&dir);
    let Some(r) = ev(&format!(
        "e = Sketchup::ImageRep.new
         r = [[e.width, e.height, e.bits_per_pixel, e.row_padding, e.size, e.data, e.colors, e.color_at_uv(0.1, 0.1)],
              cls {{ Sketchup::ImageRep.new('a', 'b') }}, cls {{ Sketchup::ImageRep.new(1234) }}, cls {{ e.load_file(nil) }},
              err {{ e.load_file('{d}/vide.png') }}, cls {{ e.save_file('{d}/x.png') }}]
         q = Sketchup::ImageRep.new('{d}/quad.png')
         r << [q.width, q.bits_per_pixel, q.row_padding, q.size, q.data.bytes.first(3), q.colors[0].to_a, q.colors.size]
         r << [[0.25, 0.75], [0.75, 0.75], [0.25, 0.25], [0.75, 0.25], [1.75, -0.25]].map {{ |u, v| q.color_at_uv(u, v).to_a }}
         r << q.color_at_uv(9.0 / 17, 0.5, true).to_a
         r += [cls {{ q.color_at_uv(0.1) }}, cls {{ q.color_at_uv(nil, 0.2) }}, cls {{ q.color_at_uv('0.1', 0.2) }}]
         r << ['gif', 'tga'].map {{ |x| i = Sketchup::ImageRep.new(\"{d}/quad.#{{x}}\"); [i.size, i.bits_per_pixel] }}
         r << (Sketchup::ImageRep.new('{d}/quad.tga').data == q.data)
         a = Sketchup::ImageRep.new('{d}/alpha.png')
         r << [a.bits_per_pixel, a.row_padding, a.data.bytes.first(4)]
         o = Sketchup::ImageRep.new('{d}/opaque.png'); g = Sketchup::ImageRep.new('{d}/gris.png')
         r << [o.bits_per_pixel, o.row_padding, o.size, g.bits_per_pixel, g.row_padding, g.size, g.colors[5].to_a]
         q.save_file('{d}/copie.png')
         r << (Sketchup::ImageRep.new('{d}/copie.png').data == q.data)
         r += [cls {{ q.save_file(nil) }}, cls {{ q.save_file('{d}/rien/x.png') }}]
         s = Sketchup::ImageRep.new
         r << [s.set_data(1, 1, 8, 3, \"\\0\\0\\0\\0\").equal?(s), s.size, s.row_padding, s.bits_per_pixel]
         r += [cls {{ s.set_data(1, 1, 32, 1, \"\\0\\0\\0\\0\") }}, cls {{ s.set_data('1', 1, 8, 0, 'x') }},
               err {{ s.set_data(-1, 1, 8, 0, 'x') }}, cls {{ s.set_data(1, 1, 8, 0, 0) }},
               err {{ s.set_data(1, 1, 16, 0, \"\\0\\0\") }}, err {{ s.set_data(0, 0, 8, 0, '') }}]"
    )) else {
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        r,
        "[[0, 0, 0, 0, 0, nil, nil, nil], ArgumentError, TypeError, TypeError, \"ArgumentError: failed to load image\", \
         RuntimeError, [17, 24, 1, 884, [255, 0, 0], [0, 0, 255, 255], 289], \
         [[0, 0, 255, 255], [0, 255, 0, 255], [185, 0, 255, 255], [255, 0, 0, 255], [0, 255, 0, 255]], \
         [0, 127, 127, 255], ArgumentError, TypeError, TypeError, [[884, 24], [884, 24]], true, [32, 0, [3, 2, 1, 4]], [24, 3, 24, 8, 1, 8, [7, 7, 7, 255]], \
         true, TypeError, RuntimeError, [true, 4, 3, 8], ArgumentError, TypeError, \
         \"RangeError: value cannot be negative\", TypeError, \"ArgumentError: invalid width, height, or bits per pixel. \
         Width or height must be greater than 0. Bits per pixel must be 8/24/32.\", \"ArgumentError: invalid pixel data\"]"
    );
}

/// `Model#find_entity_by_id` comme : tout ce qui a un entityID
/// (sommets, courbes, définitions, étiquettes, matières et textures, scènes, styles) ; des GUID
/// de groupes et d'instances ; un tableau d'un élément rend un tableau ; des entiers ou des
/// chaînes, pas les deux (TypeError) ; un tableau vide lève ArgumentError.
/// `find_entity_by_persistent_id` prend ses genres en mots-clés.
#[test]
fn models_find_entities_like_sketchup() {
    let Some(r) = ev("m = Sketchup.active_model
         g = m.entities.add_group; f = g.entities.add_face([0, 0, 0], [9, 0, 0], [9, 9, 0])
         c = m.entities.add_circle(ORIGIN, Z_AXIS, 5)[0].curve
         mat = m.materials.add('M'); l = m.layers.add('L'); pg = m.pages.add('P')
         objs = [f.vertices[0], c, g.definition, l, m.layers[0], mat, pg, m.styles.selected_style]
         r = [objs.map { |o| m.find_entity_by_id(o.entityID) == o }.all?, m.find_entity_by_id(g.guid) == g,
              m.find_entity_by_id(['', g.guid]).map(&:class), m.find_entity_by_id([f.entityID]).class,
              cls { m.find_entity_by_id(nil) }, cls { m.find_entity_by_id(ORIGIN) }, cls { m.find_entity_by_id(1, nil) },
              cls { m.find_entity_by_id([1, ORIGIN]) }, cls { m.find_entity_by_id(1, 'x') }, cls { m.find_entity_by_id('x', 1) },
              cls { m.find_entity_by_id }, cls { m.find_entity_by_id([]) }, m.find_entity_by_id(-123)]
         ids = [f.persistent_id, mat.persistent_id]
         r += [m.find_entity_by_persistent_id(ids, materials: true).map(&:class),
               m.find_entity_by_persistent_id(ids).map(&:class), m.find_entity_by_persistent_id([f.persistent_id]).size]
         r << m.instance_path_from_pid_path(\"#{g.persistent_id}.#{f.persistent_id}\").to_a.map(&:class)
         r += [cls { m.instance_path_from_pid_path(nil) }, cls { m.instance_path_from_pid_path('hello') },
               cls { m.close(true, '') }]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[true, true, [NilClass, Sketchup::Group], Array, TypeError, TypeError, TypeError, TypeError, TypeError, \
         TypeError, ArgumentError, ArgumentError, nil, [NilClass, Sketchup::Material], [Sketchup::Face, Sketchup::Material], \
         1, [Sketchup::Group, Sketchup::Face], TypeError, ArgumentError, ArgumentError]"
    );
}

/// Un observateur de modèle ajouté après des changements n'entendait la transaction implicite de
/// ce qui précédait (Esq::Events vidait les changements en attente une fois l'observateur dans
/// la liste) : il n'entend que ce qui suit (TestUp `TC_Sketchup_Model#test_active_path_Set_*`).
#[test]
fn a_new_model_observer_hears_nothing_of_the_past() {
    let Some(r) = ev("Esq::Events.reset!
         m = Sketchup.active_model
         mk = ->(l = []) { o = Class.new(Sketchup::ModelObserver) { define_method(:onTransactionStart) { |_| l << :start }
                                                                   define_method(:onTransactionCommit) { |_| l << :commit } }.new; [o, l] }
         a, la = mk.()
         m.add_observer(a)
         m.entities.add_cpoint([1, 2, 3])
         la.clear
         Esq.__native('ents.add_edges', JSON.generate('ctx' => '0', 'pts' => [[0, 0, 0], [1, 0, 0]]))
         b, lb = mk.()
         m.add_observer(b)
         r = [la.dup, lb.dup]
         m.entities.add_cpoint([4, 5, 6])
         r += [lb.dup]
         m.remove_observer(a); m.remove_observer(b)
         Esq::Events.reset!
         r")
    else {
        return;
    };
    assert_eq!(r, "[[:start, :commit], [], [:start, :commit]]");
}

/// Un type de classement n'écrivait que `AppliedSchemaTypes` : SketchUp range dans le
/// dictionnaire du schéma un dictionnaire par attribut du type, lu dans le schéma
/// (esq_core::classify_tree) ; `get/set_classification_value` suivent un chemin
/// [schéma, type, attribut…] et `set` vérifie la sorte de la valeur;
///  la première copie d'un composant lui donne `SU_DefinitionSet`.
/// TestUp `TC_Sketchup_ComponentDefinition#test_*_classification_*`.
#[test]
fn classifications_write_sketchups_tree() {
    let dir = esq_core::content::folder("Classifications").expect("classements livrés avec Esquisse");
    let skc = rb_path(&dir.join("IFC 2x3.skc"));
    let Some(r) = ev(&format!(
        "m = Sketchup.active_model
         m.classifications.load_schema({skc:?})
         d = m.definitions.add('Door')
         d.entities.add_line(ORIGIN, [0, 0, 10])
         m.entities.add_instance(d, ORIGIN)
         r = [d.attribute_dictionaries.map(&:name), d.add_classification('FooBar', 'IfcDoor'), d.add_classification('IFC 2x3', 'IfcDoor')]
         s = d.attribute_dictionary('IFC 2x3', false)
         r << s['SchemaType'] << s.attribute_dictionaries.map(&:name).values_at(0, -2, -1)
         s.attribute_dictionary('ObjectType', false).set_attribute('IfcLabel', 'value', 'TestUp Door')
         path = ['IFC 2x3', 'IfcDoor', 'ObjectType', 'IfcLabel']
         r << d.get_classification_value(path) << d.set_classification_value(path, 'Room 101') << d.get_classification_value(path)
         r << d.get_classification_value(['ClassifierPathToHeaven']) << d.set_classification_value(['IFC 2x3', 'IfcWall', 'Name', 'IfcLabel'], 'x')
         r << cls {{ d.set_classification_value(path, 123) }} << cls {{ d.get_classification_value('IFC 2x3') }}
         r << err {{ d.set_classification_value(['IFC 2x3', 'IfcDoor', 'OverallHeight', 'instanceAttributes', 'pos'], 'integer') }}
         r << d.get_classification_value(['IFC 2x3', 'IfcDoor', 'OverallHeight', 'IfcPositiveLengthMeasure', 'IfcLengthMeasure'])
         s.attribute_dictionary('ObjectType', false).set_attribute('IfcLabel', 'attribute_type', 123)
         r << cls {{ d.set_classification_value(path, 'x') }}
         r << d.remove_classification('FooBar', 'IfcDoor') << d.attribute_dictionaries.size
         r << d.remove_classification('IFC 2x3', 'IfcWall') << d.remove_classification('IFC 2x3', 'IfcDoor')
         r << d.attribute_dictionary('IFC 2x3', false) << d.attribute_dictionaries.map(&:name)
         r"
    )) else {
        return;
    };
    assert_eq!(
        r,
        "[[\"SU_DefinitionSet\"], false, true, \"http://www.iai-tech.org/ifcXML/IFC2x3/FINAL:IfcDoor\", \
         [\"OverallHeight\", \"Classifications\", \"PropertySets\"], \"TestUp Door\", true, \"Room 101\", nil, false, \
         TypeError, TypeError, \"NotImplementedError: Setting the value of 'choice' type attributes isn't supported yet.\", \
         0.0, RuntimeError, false, 3, false, true, nil, [\"SU_DefinitionSet\"]]"
    );
}

/// `ClassificationSchema` est Comparable (== et < par <=>, les noms) : deux recherches du même
/// schéma sont égales ; `unload_schema` lit son nom par StringValue (TypeError sur un nombre).
#[test]
fn schemas_compare_and_unload_like_sketchup() {
    let dir = esq_core::content::folder("Classifications").expect("classements livrés avec Esquisse");
    let (a, b) = (rb_path(&dir.join("IFC 2x3.skc")), rb_path(&dir.join("IFC 4.skc")));
    let Some(r) = ev(&format!(
        "cl = Sketchup.active_model.classifications
         cl.load_schema({a:?}); cl.load_schema({b:?})
         x, y = cl['IFC 2x3'], cl['IFC 4']
         [x == cl['IFC 2x3'], x != y, x < y, x != 3.14, (x <=> ORIGIN), cls {{ cl.unload_schema(1) }}, cl.unload_schema('IFC 4'), cl.keys]"
    )) else {
        return;
    };
    assert_eq!(r, "[true, true, true, true, nil, TypeError, true, [\"IFC 2x3\"]]");
}

/// Une copie de composant n'avait pas d'attributs avancés : SketchUp donne à chaque copie
/// `SU_InstanceSet` (Owner, Status) et à sa définition `SU_DefinitionSet` (Price, Size, Url)
/// quand elle ne l'a pas, jamais à un groupe ni à une image ; une définition
/// créée sans copie n'en a pas.
#[test]
fn a_component_copy_gets_the_advanced_attributes() {
    let Some(r) = ev("m = Sketchup.active_model
         d = m.definitions.add('Chaise')
         d.entities.add_line(ORIGIN, [0, 0, 10])
         r = [d.attribute_dictionaries]
         i = m.entities.add_instance(d, ORIGIN)
         r << d.attribute_dictionaries.map(&:name) << d.attribute_dictionary('SU_DefinitionSet').to_a
         r << i.attribute_dictionaries.map(&:name) << i.attribute_dictionary('SU_InstanceSet').to_a
         d.set_attribute('SU_DefinitionSet', 'Price', 12.5)
         m.entities.add_instance(d, [1, 0, 0])
         r << d.get_attribute('SU_DefinitionSet', 'Price')
         g = m.entities.add_group
         g.entities.add_line(ORIGIN, [1, 0, 0])
         r << g.definition.attribute_dictionaries << g.attribute_dictionaries
         r")
    else {
        return;
    };
    assert_eq!(
        r,
        "[nil, [\"SU_DefinitionSet\"], [[\"Price\", \"\"], [\"Size\", \"\"], [\"Url\", \"\"]], [\"SU_InstanceSet\"], \
         [[\"Owner\", \"\"], [\"Status\", \"\"]], 12.5, nil, nil]"
    );
}

/// `thumbnail_camera` rendait l'œil et la cible divisés par 25,4 (la définition les gardait en
/// pouces mais le pont les convertissait au retour) : SketchUp les garde en pouces et les rend
/// tels quels, au bit près (TestUp `test_thumbnail_camera` compare des nombres au hasard).
#[test]
fn thumbnail_cameras_come_back_exactly() {
    let Some(r) = ev("d = Sketchup.active_model.definitions.add
         c = Sketchup::Camera.new([0.9061447491933552, 0.7080162369509975, 0.9259520933513576], [0, 0, 0.1], [0, 0, 1])
         d.thumbnail_camera = c
         t = d.thumbnail_camera
         [t.eye.to_a == c.eye.to_a, t.target.to_a == c.target.to_a, t.up.to_a == c.up.to_a, t.eye.to_a, cls { d.thumbnail_camera = nil }]")
    else {
        return;
    };
    assert_eq!(r, "[true, true, true, [0.9061447491933552, 0.7080162369509975, 0.9259520933513576], TypeError]");
}

/// Un nombre passé par le pont revenait parfois à un bit près (serde_json lisait les nombres
/// sans `float_roundtrip`) : il revient tel quel.
#[test]
fn numbers_cross_the_bridge_exactly() {
    let Some(r) = ev("vs = [0.9061447491933552, 0.9259520933513576, 0.1 + 0.2, 123456.78901234567, 5.0e-324, 1.7976931348623157e+308]
         e = Sketchup.active_model.entities.add_line([0, 0, 0], [1, 0, 0])
         vs.each_with_index { |v, k| e.set_attribute('n', k.to_s, v) }
         vs.each_with_index.map { |v, k| e.get_attribute('n', k.to_s) == v }")
    else {
        return;
    };
    assert_eq!(r, "[true, true, true, true, true, true]");
}

/// `save_as` et `save_copy` : un ou deux arguments, la version en entier non
/// signé, ArgumentError « Invalid save version. » hors des versions connues ;
/// RuntimeError « version inconnue » et un troisième argument accepté auparavant.
#[test]
fn definitions_save_like_sketchup() {
    let dir = scratch("save");
    let p = rb_path(&dir.join("comp.skp"));
    let Some(r) = ev(&format!(
        "d = Sketchup.active_model.definitions.add('C')
         d.entities.add_line(ORIGIN, [0, 0, 10])
         [err {{ d.save_as({p:?}, 1337) }}, cls {{ d.save_as({p:?}, 'A string') }}, err {{ d.save_as({p:?}, -1) }},
          err {{ d.save_as({p:?}, 15, true) }}, err {{ d.save_copy }}, d.save_copy({p:?}, Sketchup::Model::VERSION_2017),
          File.exist?({p:?}), d.path == {p:?}, d.save_as({p:?}), d.path == {p:?}]"
    )) else {
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        r,
        "[\"ArgumentError: Invalid save version.\", TypeError, \"RangeError: value cannot be negative\", \
         \"ArgumentError: wrong number of arguments (given 3, expected 1..2)\", \
         \"ArgumentError: wrong number of arguments (given 0, expected 1..2)\", true, true, false, true, true]"
    );
}

/// `add_instance(définition, ORIGIN)` levait « matrice de 16 nombres attendue » : la
/// transformation peut être ce dont Geom::Transformation.new fait une (un point : une
/// translation), comme dans SketchUp (TestUp `test_count_used_instances`).
#[test]
fn add_instance_takes_a_point() {
    let Some(r) = ev("m = Sketchup.active_model
         d = m.definitions.add('P')
         d.entities.add_cpoint(ORIGIN)
         a = m.entities.add_instance(d, ORIGIN)
         b = m.entities.add_instance(d, [1, 2, 3])
         [a.transformation.origin.to_a, b.transformation.origin.to_a, d.count_used_instances]")
    else {
        return;
    };
    assert_eq!(r, "[[0.0, 0.0, 0.0], [1.0, 2.0, 3.0], 2]");
}

/// `live_component?` d'un groupe rangé dans un composant dynamique (SU_LiveComponent) est vrai,
/// de proche en proche à travers des groupes ; un composant
/// ordinaire coupe la chaîne.
#[test]
fn groups_of_live_components_are_live() {
    let Some(r) = ev("m = Sketchup.active_model
         lc = m.definitions.add('Porte')
         Esq.__native('attr.create', JSON.generate('owner' => \"d:0:#{lc.ctx}\", 'dict' => 'SU_LiveComponent'))
         g = lc.entities.add_group
         g.entities.add_line(ORIGIN, [1, 0, 0])
         g2 = g.entities.add_group
         g2.entities.add_line(ORIGIN, [0, 1, 0])
         plain = m.definitions.add('Simple')
         g3 = plain.entities.add_group
         g3.entities.add_line(ORIGIN, [0, 0, 1])
         [lc.live_component?, g.definition.live_component?, g2.definition.live_component?, plain.live_component?,
          g3.definition.live_component?, cls { lc.live_component?(1) }]")
    else {
        return;
    };
    assert_eq!(r, "[true, true, true, false, false, ArgumentError]");
}

/// `DefinitionList#load` n'annonçait que l'ajout : SketchUp annonce aussi le chargement comme un
/// changement des propriétés de la définition (« Load Component »), et un observateur qui purge
/// alors les définitions inutilisées l'efface (TestUp
/// `test_load_evil_definitions_observer_without_operation`).
#[test]
fn a_loaded_definition_announces_its_properties() {
    let dir = scratch("load");
    let p = rb_path(&dir.join("piece.skp"));
    let Some(r) = ev(&format!(
        "m = Sketchup.active_model
         d = m.definitions.add('Pièce')
         d.entities.add_line(ORIGIN, [0, 0, 10])
         d.save_as({p:?})
         got = []
         o = Class.new(Sketchup::DefinitionsObserver) {{
           define_method(:onComponentPropertiesChanged) {{ |defs, x| got << x.name; defs.purge_unused }}
         }}.new
         m.definitions.add_observer(o)
         loaded = m.definitions.load({p:?})
         m.definitions.remove_observer(o)
         [got.size, loaded.deleted?, cls {{ loaded.name }}]"
    )) else {
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(r, "[1, true, TypeError]");
}

/// Une matière neuve était grise (204) et opaque, son alpha un nombre simple précision : dans
/// SketchUp sa couleur est (0, 0, 0, 0), une couleur donnée prend l'alpha 255
/// et une couleur vide le gris par défaut, l'alpha est un double rendu tel
/// quel.
#[test]
fn materials_keep_colours_and_alpha_like_sketchup() {
    let Some(r) = ev("a = Sketchup.active_model.materials.add('Joe')
         r = [a.color.to_a]
         a.color = Sketchup::Color.new(32, 64, 128, 7)
         r << a.color.to_a
         a.color = nil
         r << a.color.to_a
         a.alpha = 0.4
         r += [a.alpha == 0.4, a.use_alpha?, cls { a.alpha = nil }, cls { a.alpha = 'x' }]
         a.alpha = 2
         r << a.alpha")
    else {
        return;
    };
    assert_eq!(r, "[[0, 0, 0, 0], [32, 64, 128, 255], [64, 64, 64, 255], true, true, TypeError, TypeError, 1.0]");
}

fn uniform_png(dir: &std::path::Path, name: &str, rgb: [u8; 3]) -> String {
    let p = dir.join(name);
    image::RgbImage::from_pixel(4, 2, image::Rgb(rgb)).save(&p).unwrap();
    rb_path(&p)
}

/// `colorize_deltas` rendait teinte, saturation, clarté : Ruby rend teinte, clarté, saturation,
/// calculées comme SketchUp ; une couleur donnée
/// à une matière texturée la colorise en teinte, `colorize_type=` change le
/// calcul, (0, 0, 0, 0) retire la colorisation.
#[test]
fn colorize_deltas_are_sketchups() {
    let dir = scratch("deltas");
    let png = uniform_png(&dir, "gris.png", [143, 143, 146]);
    let Some(r) = ev(&format!(
        "t = Sketchup.active_model.materials.add('Tex')
         t.texture = {png:?}
         r = [t.materialType, t.colorize_deltas]
         t.color = [147, 175, 110]
         r += [t.materialType, t.colorize_type, t.colorize_deltas]
         t.colorize_type = 0
         r << t.colorize_deltas
         t.color = Sketchup::Color.new(0, 0, 0, 0)
         r += [t.materialType, t.color.to_a]"
    )) else {
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        r,
        "[1, [0.0, 0.0, 0.0], 2, 1, [85.84615082430435, -0.007843151688575745, 0.27531425227386963], \
         [-154.15384917569565, -0.007843151688575745, 0.27531425227386963], 1, [143, 143, 146, 255]]"
    );
}

/// `Material#name=` lit le nom par StringValue et ne fait rien du même nom ;
/// `colorize_type=` prend un entier non signé jusqu'à 2 ; `save_as` lit son
/// chemin par StringValue.
#[test]
fn material_arguments_are_read_like_sketchup() {
    let Some(r) = ev("a = Sketchup.active_model.materials.add('Joe')
         [cls { a.name = nil }, cls { a.name = a }, (a.name = 'Joe'), a.name,
          cls { a.colorize_type = nil }, cls { a.colorize_type = 'x' }, cls { a.colorize_type = -1 }, err { a.colorize_type = 3 },
          (a.colorize_type = 2), cls { a.save_as(nil) }, cls { a.save_as(123) }]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[TypeError, TypeError, \"Joe\", \"Joe\", TypeError, TypeError, RangeError, \
         \"ArgumentError: Invalid colorization type\", 2, TypeError, TypeError]"
    );
}

/// `Materials#[]` prend un nom ou un index par rb_num2long, négatif depuis la fin;
///  `add` sans nom nomme « Material », « Material1 »… ;
/// `remove` prend une matière, un nom ou une couleur et rend faux pour nil ou ce qu'il ne trouve
/// pas ; `load` et `unique_name` lisent leur argument par StringValue.
#[test]
fn materials_are_reached_and_removed_like_sketchup() {
    let Some(r) = ev("m = Sketchup.active_model.materials
         a = m.add('Joe')
         b = m.add
         c = m.add
         r = [b.name, c.name, m.at(-1) == c, m[0] == a, cls { m.at(nil) }, cls { m.at(['Joe']) }, err { m.at(-9) },
              cls { m.unique_name(nil) }, cls { m.load(nil) }, cls { m.load(3) }]
         r += [m.remove(nil), m.remove(123), m.remove('Joe'), err { m.remove('Personne') }, m.remove(b), m.map(&:name)]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[\"Material\", \"Material1\", true, true, TypeError, TypeError, \"IndexError: index -6 out of range [0, 3)\", \
         TypeError, TypeError, TypeError, false, false, true, \"ArgumentError: Cannot find material named Personne\", true, \
         [\"Material1\"]]"
    );
}

/// La couleur d'une texture était la moyenne d'un pixel sur quelques-uns, décodé par `image` :
/// c'est la moyenne de tous les pixels tels que SketchUp les décode, tronquée,
/// la même qu'elle vienne d'un fichier ou d'une ImageRep ; une ImageRep, qui
/// n'a pas de nom de fichier, donne « .jpg » (« .png » avec une transparence).
#[test]
fn textures_take_sketchups_average_and_name() {
    let dir = scratch("tex");
    let p = dir.join("deux.png");
    let mut img = image::RgbImage::new(3, 1);
    img.put_pixel(0, 0, image::Rgb([10, 20, 30]));
    img.put_pixel(1, 0, image::Rgb([11, 21, 31]));
    img.put_pixel(2, 0, image::Rgb([11, 21, 31]));
    img.save(&p).unwrap();
    let png = rb_path(&p);
    let Some(r) = ev(&format!(
        "m = Sketchup.active_model.materials
         a = m.add('A'); a.texture = {png:?}
         b = m.add('B'); b.texture = Sketchup::ImageRep.new({png:?})
         c = m.add('C'); rep = Sketchup::ImageRep.new; rep.set_data(1, 1, 32, 0, [1, 2, 3, 128].pack('C*')); c.texture = rep
         [a.color.to_a, b.color.to_a, a.texture.filename, b.texture.filename, c.texture.filename]"
    )) else {
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(r, "[[10, 20, 30, 255], [10, 20, 30, 255], \"deux.png\", \".jpg\", \".png\"]");
}

/// `Texture#parent` rendait une entité effacée : c'est sa matière ; `write` lit son chemin par
/// StringValue (TestUp test_write_invalid_argument_*).
#[test]
fn textures_know_their_material() {
    let dir = scratch("texp");
    let png = uniform_png(&dir, "p.png", [1, 2, 3]);
    let Some(r) = ev(&format!(
        "a = Sketchup.active_model.materials.add('A'); a.texture = {png:?}
         t = a.texture
         [t.parent == a, cls {{ t.write(nil) }}, cls {{ t.write(123) }}]"
    )) else {
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(r, "[true, TypeError, TypeError]");
}

/// `Color.names` en donnait 147 (les variantes « Grey ») : SketchUp en liste 140, LightGrey à
/// la place de LightGray ; `blend` tronque chaque composante et lit son poids par rb_num2dbl ;
/// `red=`… lisent un entier par rb_num2long (une couleur, qui a `to_i`, lève TypeError).
#[test]
fn colors_blend_and_name_like_sketchup() {
    let Some(r) = ev("n = Sketchup::Color.names
         c1 = Sketchup::Color.new(240, 248, 255)
         c2 = Sketchup::Color.new('CornflowerBlue')
         [n.size, n.include?('LightGrey'), n.include?('LightGray'), Sketchup::Color.new('Grey').to_a,
          c1.blend(c2, 0.5).to_a, c1.blend(c2, -1).to_a, c1.blend(c2, 1.1).to_a, cls { c1.blend(c2, c1) },
          cls { c1.blend(c2, nil) }, cls { c1.blend(nil, 0.5) }, cls { c1.red = c1 }, cls { c1.green = 'red' },
          cls { c1.blue = [1] }, cls { c1.red = 256 }]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[140, true, false, [128, 128, 128, 255], [170, 198, 246, 255], [0, 50, 219, 255], [254, 255, 255, 255], \
         TypeError, TypeError, ArgumentError, TypeError, TypeError, TypeError, ArgumentError]"
    );
}

/// `weld` donnait une ArcCurve pour le tour d'un carré (un polygone régulier) : SketchUp soude
/// toujours en courbe simple (CCurve) ; un bord d'une autre collection lève
/// ArgumentError « Edge has different parent », pas « reference to deleted ».
#[test]
fn welded_edges_make_plain_curves() {
    let Some(r) = ev("ents = Sketchup.active_model.entities
         f = ents.add_face([0, 0, 0], [9, 0, 0], [9, 9, 0], [0, 9, 0])
         cs = ents.weld(f.edges)
         g = ents.add_group
         h = g.entities.add_face([0, 0, 5], [9, 0, 5], [9, 9, 5], [0, 9, 5])
         [cs.map(&:class), cs.first.vertices.size, err { ents.weld(h.edges) }]")
    else {
        return;
    };
    assert_eq!(r, "[[Sketchup::Curve], 5, \"ArgumentError: Edge has different parent\"]");
}

/// `transform_by_vectors` ne bougeait pas les sommets et refusait une Transformation : chaque
/// entité prend la transformation que donne sa valeur (vecteur, point ou
/// Transformation), les sommets bougent tous ensemble ; sans deux tableaux
/// rien ne se passe ; ce qui n'est pas une entité lève TypeError.
#[test]
fn vertices_move_by_their_own_vectors() {
    let Some(r) = ev("ents = Sketchup.active_model.entities
         f = ents.add_face([0, 0, 0], [9, 0, 0], [9, 9, 0], [0, 9, 0])
         vs = f.vertices
         shifts = [[2, 0, 0], [-2, 0, 0], [0, 2, 0], [0, -2, 0]]
         want = vs.zip(shifts).map { |v, s| (v.position + s).to_a }
         vals = [Geom::Vector3d.new(2, 0, 0), Geom::Point3d.new(-2, 0, 0), [0, 2, 0], Geom::Transformation.new([0, -2, 0])]
         r = ents.transform_by_vectors(vs, vals)
         got = vs.map { |v| v.position.to_a }
         e = ents.grep(Sketchup::Edge).first
         p0 = e.start.position.to_a
         ents.transform_by_vectors(e, [[5, 5, 5]])
         [r.equal?(ents), got == want, f.valid?, f.vertices.size, e.start.position.to_a == p0,
          cls { ents.transform_by_vectors([1], [[1, 0, 0]]) }, cls { ents.transform_by_vectors([e], ['x']) }]")
    else {
        return;
    };
    assert_eq!(r, "[true, true, true, 4, true, TypeError, TypeError]");
}

/// `each` rendait aussi ce que le bloc venait d'effacer (TypeError à l'usage) : l'itérateur de
/// SketchUp passe par-dessus (TestUp test_erase_entities_each_loop_crash).
#[test]
fn each_steps_over_what_the_block_erased() {
    let Some(r) = ev("g = Sketchup.active_model.entities.add_group
         ge = g.entities
         ge.add_face([0, 0, 0], [9, 0, 0], [9, 9, 0], [0, 9, 0])
         seen = []
         ge.each { |e| seen << e.typename; ge.erase_entities(e) }
         [seen, ge.size]")
    else {
        return;
    };
    assert_eq!(r, "[[\"Edge\", \"Edge\", \"Edge\", \"Edge\"], 0]");
}

/// `erase_entities` prend aussi une sélection, et lève TypeError pour ce qui n'est pas une
/// entité, une entité effacée (« reference to deleted Entity ») ou un tableau dans le tableau
#[test]
fn erase_entities_reads_its_arguments_like_sketchup() {
    let Some(r) = ev("m = Sketchup.active_model
         g = m.entities.add_group
         g.entities.add_line([0, 0, 0], [9, 9, 9])
         l = m.entities.add_line([0, 0, 0], [1, 0, 0])
         m.selection.clear
         m.selection.add(l)
         m.entities.erase_entities(m.selection)
         [l.deleted?, err { m.entities.erase_entities(l) }, cls { m.entities.erase_entities(5) },
          cls { m.entities.erase_entities([[g]]) }, g.deleted?]")
    else {
        return;
    };
    assert_eq!(r, "[true, \"TypeError: reference to deleted Entity\", TypeError, TypeError, false]");
}

/// Un point se lit comme SketchUp le lit (pour add_cpoint) :
/// 2 ou 3 nombres (rb_num2dbl), un Point3d, un Vertex ou un InputPoint ; le reste lève
/// ArgumentError (TestUp test_add_dimension_linear_cpoint_cpoint_array3d).
#[test]
fn points_are_read_like_sketchup() {
    let Some(r) = ev("ents = Sketchup.active_model.entities
         e = ents.add_line([1, 2, 3], [4, 5, 6])
         [ents.add_cpoint(e.start).position.to_a, ents.add_cpoint([1, 2]).position.to_a,
          cls { ents.add_cpoint(['a', 1, 2]) }, err { ents.add_cpoint([1, 2, 3, 4]) },
          err { ents.add_cpoint(Geom::Vector3d.new(1, 0, 0)) }]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[[1.0, 2.0, 3.0], [1.0, 2.0, 0.0], TypeError, \"ArgumentError: wrong number of values in array\", \
         \"ArgumentError: Cannot convert argument to Geom::Point3d\"]"
    );
}

/// Deux triangles d'un maillage dont la diagonale porte un indice négatif : avec
/// HIDE_BASED_ON_INDEX elle est cachée et pas adoucie, même avec SOFTEN_BASED_ON_INDEX, mais
/// SMOOTH_SOFT_EDGES la lisse encore (TestUp *_hide_based_on_index_override_soften_based_on_index).
#[test]
fn hiding_by_index_wins_over_softening() {
    let Some(r) = ev("mesh = Geom::PolygonMesh.new
         [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]].each { |p| mesh.add_point(Geom::Point3d.new(p)) }
         mesh.add_polygon(1, 2, -3)
         mesh.add_polygon(-1, 3, 4)
         flags = Geom::PolygonMesh::SOFTEN_BASED_ON_INDEX | Geom::PolygonMesh::SMOOTH_SOFT_EDGES |
                 Geom::PolygonMesh::HIDE_BASED_ON_INDEX
         r = []
         [true, false].each do |fill|
           g = Sketchup.active_model.entities.add_group
           fill ? g.entities.fill_from_mesh(mesh, true, flags) : g.entities.add_faces_from_mesh(mesh, flags)
           e = g.entities.grep(Sketchup::Edge).select(&:hidden?)
           r << [e.size, e.first.soft?, e.first.smooth?]
         end
         r")
    else {
        return;
    };
    assert_eq!(r, "[[1, false, true], [1, false, true]]");
}

/// `fill_from_mesh` remplissait une collection déjà pleine : SketchUp ne remplit qu'une
/// collection vide et rend nil sinon, avant même de lire ses arguments.
#[test]
fn a_mesh_fills_only_an_empty_collection() {
    let Some(r) = ev("mesh = Geom::PolygonMesh.new
         mesh.add_polygon([0, 0, 0], [1, 0, 0], [1, 1, 0])
         g = Sketchup.active_model.entities.add_group
         g.entities.add_line([100, 100, 100], [100, 100, 200])
         h = Sketchup.active_model.entities.add_group
         [g.entities.fill_from_mesh(mesh), g.entities.size, g.entities.fill_from_mesh, h.entities.fill_from_mesh(mesh),
          h.entities.grep(Sketchup::Face).size]")
    else {
        return;
    };
    assert_eq!(r, "[nil, 1, nil, true, 1]");
}

/// La matière d'une image ne va ni sur les faces d'un maillage ni sur une face :
/// ArgumentError (SketchUp depuis, TestUp
/// test_*_from_mesh_image_material).
#[test]
fn an_image_material_stays_on_its_image() {
    let dir = scratch("imgmat");
    let png = uniform_png(&dir, "img.png", [10, 20, 30]);
    let Some(r) = ev(&format!(
        "m = Sketchup.active_model
         m.entities.add_image({png:?}, [0, 0, 0], 10)
         im = m.definitions.find(&:image?).entities.grep(Sketchup::Face).first.material
         mesh = Geom::PolygonMesh.new
         mesh.add_polygon([0, 0, 0], [1, 0, 0], [1, 1, 0])
         g = m.entities.add_group
         [im.owner_type == Sketchup::Material::OWNER_IMAGE, err {{ m.entities.add_faces_from_mesh(mesh, 0, im) }},
          cls {{ m.entities.add_faces_from_mesh(mesh, 0, nil, im) }}, cls {{ g.entities.fill_from_mesh(mesh, true, 0, im) }},
          cls {{ g.entities.fill_from_mesh(mesh, true, 0, nil, im) }}, g.entities.size]"
    )) else {
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        r,
        "[true, \"ArgumentError: Cannot assign image material to model entities\", ArgumentError, ArgumentError, \
         ArgumentError, 0]"
    );
}

/// `glued_to=` comme SketchUp : ce qui n'est pas une entité lève
/// TypeError ; une image colle toujours, un groupe seulement si sa définition colle (TestUp
/// test_glued_to_Set_ungluable, test_glued_to_Set_invalid_argument).
#[test]
fn what_glues_is_checked_like_sketchup() {
    let dir = scratch("colle");
    let png = uniform_png(&dir, "img.png", [10, 20, 30]);
    let Some(r) = ev(&format!(
        "ents = Sketchup.active_model.entities
         f = ents.add_face([0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0])
         a = ents.add_image({png:?}, [0, 0, 0], 10)
         b = ents.add_image({png:?}, [0, 0, 0], 10)
         g = ents.add_group
         g.entities.add_line([0, 0, 0], [1, 1, 1])
         a.glued_to = f
         b.glued_to = a
         [a.glued_to == f, b.glued_to == a, err {{ g.glued_to = f }}, cls {{ g.glued_to = 3048 }},
          cls {{ a.glued_to = 3048 }}, err {{ a.glued_to = b }}]"
    )) else {
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        r,
        "[true, true, \"ArgumentError: The definition does not have glueing behavior\", TypeError, TypeError, \
         \"ArgumentError: Can't create cyclic gluing\"]"
    );
}

/// `EntitiesBuilder` passait par Entities : une face au sol était retournée, les trous et un
/// tableau de points refusés, vertex_at créait un sommet. Comme SketchUp 
/// la face garde l'orientation de ses points,
/// `holes:` fait des trous, add_edge et add_edges prennent un tableau, add_edges rend nil pour
/// un bord trop court, vertex_at rend nil où il n'y a rien, `new` n'existe pas.
#[test]
fn the_builder_builds_like_sketchup() {
    let Some(r) = ev("ents = Sketchup.active_model.entities.add_group.entities
         r = nil
         ents.build do |b|
           up = b.add_face([[0, 0, 0], [9, 0, 0], [9, 8, 0], [0, 8, 0]],
                           holes: [[[1, 1, 0], [4, 1, 0], [4, 7, 0], [1, 7, 0]], [[5, 1, 0], [8, 1, 0], [8, 7, 0], [5, 7, 0]]])
           down = b.add_face([20, 0, 0], [20, 6, 0], [29, 6, 0], [29, 0, 0])
           e = b.add_edge([[0, 0, 5], [9, 0, 5]])
           es = b.add_edges([[0, 0, 9], [5, 0, 9], [5.0001, 0, 9], [9, 0, 9]])
           r = [up.normal.to_a, up.loops.size, up.vertices.size, down.normal.to_a, e.length.to_f, es.map(&:class),
                b.vertex_at([9, 0, 9]).class, b.vertex_at([50, 50, 50]), err { b.add_edge([0, 0, 0], [0.0004, 0, 0]) },
                err { b.add_edges([[1, 1, 1]]) }, err { b.add_face([0, 0, 0], [1, 0, 0], [0, 0, 1], [1, 1, 1]) },
                cls { b.add_face([[0, 0, 0], [1, 0, 0], [1, 1, 0]], holes: 5) }]
         end
         r + [cls { Sketchup::EntitiesBuilder.new }]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[[0.0, 0.0, 1.0], 3, 12, [0.0, 0.0, -1.0], 9.0, [Sketchup::Edge, NilClass, Sketchup::Edge], Sketchup::Vertex, \
         nil, \"ArgumentError: Duplicate points\", \"ArgumentError: At least 2 points required, got 1\", \
         \"ArgumentError: Points are not planar\", TypeError, NoMethodError]"
    );
}

/// `DefinitionList#load` comme SketchUp (TestUp test_load_*) : un fichier d'un
/// SketchUp plus récent qu'on peut lire veut `allow_newer: true` (marqueur VFF 1, > 0x11), un
/// autre (marqueur > 1, ou ancien format d'une version > 26) est refusé ; un fichier qui n'a que
/// du texte d'écran aussi ; le même fichier inchangé redonne la même définition.
#[test]
fn definitions_load_like_sketchup() {
    use esq_core::glam::DVec3;
    let dir = scratch("defload");
    let mut m = esq_core::model::Model::new();
    m.root.add_polyline(&[DVec3::ZERO, DVec3::X * 100.0, DVec3::new(100.0, 100.0, 0.0)], true, false);
    let opts = Default::default();
    let vff = esq_skp::native::write::write(&m, None, None, &opts).unwrap();
    let at = vff.windows(3).position(|w| w == b"VFF").unwrap();
    let file = |name: &str, bytes: &[u8]| {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        rb_path(&p)
    };
    let face = file("face.skp", &vff);
    let mut soft = vff.clone();
    soft[at + 7..at + 9].copy_from_slice(&1000u16.to_le_bytes());
    let soft = file("soft.skp", &soft);
    let mut hard = vff.clone();
    hard[at + 5..at + 7].copy_from_slice(&1000u16.to_le_bytes());
    let hard = file("hard.skp", &hard);
    let mut old = esq_skp::native::write::write(&m, None, Some(esq_skp::SkpVersion::Su2018), &opts).unwrap();
    let i = old.windows(4).position(|w| w == [b'{', 0, b'1', 0]).unwrap();
    old[i + 2] = b'9';
    let old = file("su98.skp", &old);
    let mut t = esq_core::model::Model::new();
    t.root.annots.insert(esq_core::model::Annotation {
        kind: esq_core::model::AnnotKind::ScreenText { x: 0.1, y: 0.1 },
        text: "Bonjour".into(),
        layer: 0,
        hidden: false,
        attach: Default::default(),
    });
    let screen = file("ecran.skp", &esq_skp::native::write::write(&t, None, None, &opts).unwrap());
    let Some(r) = ev(&format!(
        "ds = Sketchup.active_model.definitions
         n = ds.size
         a = ds.load({face:?})
         b = ds.load({face:?})
         a.entities.add_cpoint([0, 0, 0])
         c = ds.load({face:?})
         [a == b, c == a, err {{ ds.load({soft:?}) }}, ds.load({soft:?}, allow_newer: true).class,
          cls {{ ds.load({hard:?}, allow_newer: true) }}, err {{ ds.load({old:?}) }}, err {{ ds.load({screen:?}) }},
          ds.size - n]"
    )) else {
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    let newer = "RuntimeError: This operation could not be completed since the model was created in a newer version \
                 of SketchUp. To complete this operation you will need to upgrade to the latest SketchUp version.";
    assert_eq!(
        r,
        format!(
            "[true, false, \"RuntimeError: Model is of a newer model version that can be opened, but some information \
             might be lost. Use `allow_newer: true` option to load the file.\", Sketchup::ComponentDefinition, RuntimeError, \
             {newer:?}, \"RuntimeError: Can't insert a component with just screen text.\", 3]"
        )
    );
}

/// `Geom::Vector2d` comme SketchUp : `*` est le produit vectoriel et refuse un nombre;
///  `length=` refuse une longueur ou un vecteur nuls (ArgumentError,
/// SUVector2DSetLength) ; `angle_between` rend les deux vecteurs unitaires avant le produit
/// scalaire (SUVector2DAngleBetween : 0.21109333322274632 pour (1, 2) et (4, 5)) ; `set!` copie
/// un premier Vector2d, quoi qui suive.
#[test]
fn vector2d_computes_like_sketchup() {
    let Some(r) = ev("v = Geom::Vector2d.new(1, 2)
         w = Geom::Vector2d.new(101, 102)
         u = Geom::Vector2d.new(1, 0)
         [cls { v * 1234 }, v * [3, 4], v.angle_between(Geom::Vector2d.new(4, 5)) == 0.21109333322274632,
          err { u.length = 0 }, cls { Geom::Vector2d.new(0, 0).length = 1 }, cls { u.length = nil },
          v.set!(w, Geom::Vector2d.new(3, 1)).to_a]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[TypeError, -2.0, true, \"ArgumentError: Cannot set the length of a zero length vector\", ArgumentError, \
         TypeError, [101.0, 102.0]]"
    );
}

/// `Point3d.linear_combination` lit chaque poids (rb_num2dbl) avant son point :
/// des nil ou des chaînes lèvent TypeError, des nombres à la place des points ArgumentError.
#[test]
fn linear_combinations_read_weights_first() {
    let Some(r) = ev("pc = Geom::Point3d
         [cls { pc.linear_combination(nil, nil, nil, nil) }, cls { pc.linear_combination('sk', 'et', 'ch', 'up') },
          cls { pc.linear_combination(1, 2, 3, 4) }, pc.linear_combination(0.25, [0, 0, 0], 0.75, [4, 8, 12]).to_a]")
    else {
        return;
    };
    assert_eq!(r, "[TypeError, TypeError, ArgumentError, [3.0, 6.0, 9.0]]");
}

/// Un dictionnaire ajouté à une arête ou une face est un changement de l'entité :
/// onChangeEntity (TestUp test_attribute_dictionary_evil_observer_without_operation).
#[test]
fn a_new_dictionary_changes_its_entity() {
    let Some(r) = ev("seen = []
         ob = Class.new(Sketchup::EntityObserver) do
           define_method(:onChangeEntity) { |e| seen << e.class; e.attribute_dictionaries.delete('Mal') }
         end.new
         edge = Sketchup.active_model.entities.add_line([0, 0, 0], [5, 5, 5])
         edge.add_observer(ob)
         d = edge.attribute_dictionary('Mal', true)
         r = [seen.first, d.deleted?]
         edge.remove_observer(ob)
         r")
    else {
        return;
    };
    assert_eq!(r, "[Sketchup::Edge, true]");
}

/// Dans une opération, les observateurs n'entendent rien avant sa validation (SketchUp 2016 et
/// suivants : TestUp test_attribute_dictionary_evil_observer_with_operation) ; une opération
/// annulée ne leur dit rien.
#[test]
fn observers_hear_an_operation_when_it_is_committed() {
    let Some(r) = ev("seen = []
         ob = Class.new(Sketchup::EntityObserver) do
           define_method(:onChangeEntity) { |e| seen << :change; e.attribute_dictionaries.delete('Mal') }
         end.new
         m = Sketchup.active_model
         edge = m.entities.add_line([0, 0, 0], [5, 5, 5])
         edge.add_observer(ob)
         m.start_operation('Mal', true)
         d = edge.attribute_dictionary('Mal', true)
         r = [d.valid?, seen.size]
         m.commit_operation
         r += [d.deleted?, seen.size > 0]
         n = seen.size
         m.start_operation('Rien', true)
         edge.set_attribute('Autre', 'k', 1)
         m.abort_operation
         edge.remove_observer(ob)
         r + [seen.size == n]")
    else {
        return;
    };
    assert_eq!(r, "[true, 0, true, true, true]");
}

/// Un tableau de deux nombres n'est en 2D que si l'autre côté l'est aussi 
/// avec un Point3d ou un Vector3d, il est lu en 3D (z = 0).
#[test]
fn two_number_arrays_meet_3d_in_3d() {
    let Some(r) = ev("a = [10, 10]
         a.offset!(Geom::Vector3d.new(0, 0, 1))
         [[1, 0].distance(Geom::Point3d.new(0, 0, 0)).to_f, [1, 2].dot(Geom::Vector3d.new(10, 20, 30)), a,
          [3, 4].distance([0, 0]).to_f, cls { [1, 2, 3].dot(Geom::Vector2d.new(1, 2)) }]")
    else {
        return;
    };
    assert_eq!(r, "[1.0, 50.0, [10.0, 10.0, 1.0], 5.0, ArgumentError]");
}

/// `BoundingBox#contains?` lit son point avant de regarder si la boîte est vide (TestUp
/// test_contains_Query_invalid_argument_*).
#[test]
fn an_empty_box_still_reads_its_point() {
    let Some(r) = ev("b = Geom::BoundingBox.new
         [cls { b.contains?(Geom::Vector3d.new(1, 2, 3)) }, cls { b.contains?('x') }, cls { b.contains?(123) },
          b.contains?([1, 2, 3])]")
    else {
        return;
    };
    assert_eq!(r, "[ArgumentError, ArgumentError, ArgumentError, false]");
}

/// `Geom::UTM.new` comme SketchUp : une UTM ou un tableau, sinon TypeError ; la
/// zone par rb_num2long, la bande par StringValue, les coordonnées par rb_num2dbl.
#[test]
fn utm_arguments_are_read_like_sketchup() {
    let Some(r) = ev("u = Geom::UTM
         [cls { u.new(13) }, cls { u.new(nil) }, cls { u.new(13, 1, 4.0, 5.0) }, cls { u.new('13', 'T', 4.0, 5.0) },
          cls { u.new(13, 'T', '4', 5.0) }, cls { u.new(13, 'T', 4.0) }, u.new(-1, 'T', 4.0, 5.0).to_s]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[TypeError, TypeError, TypeError, TypeError, TypeError, ArgumentError, \"UTM(-1 T 4.00000 5.00000)\"]"
    );
}

/// `Point2d#offset(vector, distance)` refuse une distance nulle (SUVector2DSetLength sous 1e-10,
/// « Zero distance is not allowed »).
#[test]
fn a_2d_offset_needs_a_distance() {
    let Some(r) = ev("p = Geom::Point2d.new(11, 21)
         [err { p.offset(Geom::Vector2d.new(1, 1), 0) }, p.offset(Geom::Vector2d.new(3, 4), 10).to_a]")
    else {
        return;
    };
    assert_eq!(r, "[\"ArgumentError: Zero distance is not allowed\", [17.0, 29.0]]");
}

/// `Vector3d.linear_combination` lit chaque poids avant son vecteur ; `*` est le
/// produit vectoriel et refuse un nombre ; `length=` prend toute longueur mais pas
/// un vecteur nul (SUVector3DSetLength) ; `valid?` est faux quand chaque coordonnée est sous
/// 1e-10.
#[test]
fn vector3d_computes_like_sketchup() {
    let Some(r) = ev("v = Geom::Vector3d.new(1, 2, 3)
         w = v.clone
         w.length = 1.0e-10
         z = v.clone
         z.length = 0
         vc = Geom::Vector3d
         [cls { vc.linear_combination(nil, nil, nil, nil) }, cls { vc.linear_combination([1], [2], [3], [4]) },
          cls { vc.linear_combination(2, [2], 'x', nil) }, err { v * 1234 }, w.valid?, z.valid?, z.to_a,
          err { Geom::Vector3d.new.length = 2 }]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[TypeError, TypeError, ArgumentError, \"ArgumentError: Cannot convert argument to Geom::Vector3d\", false, false, \
         [0.0, 0.0, 0.0], \"TypeError: Cannot set the length of a zero length vector)\"]"
    );
}

/// `Transformation.axes` rend chaque axe unitaire et refuse un axe nul.
#[test]
fn axes_are_made_unit_vectors() {
    let Some(r) = ev("t = Geom::Transformation.axes([1, 2, 3], [2, 0, 0], [0, 3, 0], [0, 0, 4])
         [t.to_a, err { Geom::Transformation.axes(ORIGIN, [1, 2, 3], [1, 2, 3], [0, 0, 0]) }]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 3.0, 1.0], \
         \"ArgumentError: Cannot create unit vector from zero length vector\"]"
    );
}

/// `Geom::Transformation2d` garde [xx, xy, yx, yy, tx, ty] et calcule en simple précision comme
/// SketchUp (SUTransformation2DRotation, …Multiply, …GetInverse : TestUp test_rotation,
/// test_Operator_Multiply_order_of_operation, test_inverse).
#[test]
fn transformation2d_computes_in_single_precision() {
    let Some(r) = ev("t = Geom::Transformation2d
         r = t.rotation(Geom::Point2d.new(11, 22), 45.degrees).to_a
         m = (t.translation([2, 5]) * t.rotation(ORIGIN_2D, 30.degrees)).to_a
         i = t.rotation(ORIGIN_2D, 30.degrees).inverse.to_a
         [r, m, i]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[[0.7071067690849304, -0.7071067690849304, 0.7071067690849304, 0.7071067690849304, 18.778175354003906, \
         -1.3345222473144531], [0.8660253882408142, -0.5, 0.5, 0.8660253882408142, 2.0, 5.0], \
         [0.8660253882408142, 0.5, -0.5, 0.8660253882408142, -0.0, 0.0]]"
    );
}

/// La face que `find_faces` fait suit l'arête de son début à sa fin ; au sol, elle regarde en
/// bas (TestUp test_find_faces_face_orientation_*).
#[test]
fn found_faces_follow_their_edge() {
    let Some(r) = ev("def tri(a, b, c)
           g = Sketchup.active_model.entities.add_group
           e = g.entities
           first = e.add_line(a, b)
           e.add_line(b, c)
           e.add_line(c, a)
           first.find_faces
           e.grep(Sketchup::Face).first.normal.to_a
         end
         [tri([1, 1, 10], [2, 2, 10], [2, 1, 10]), tri([1, 1, 10], [2, 1, 10], [2, 2, 10]),
          tri([1, 0, 1], [2, 0, 2], [2, 0, 1]), tri([0, 1, 1], [0, 2, 2], [0, 2, 1]), tri([1, 1, 0], [2, 1, 0], [2, 2, 0])]")
    else {
        return;
    };
    assert_eq!(r, "[[0.0, 0.0, -1.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0]]");
}

/// Une matière effacée ne se pose pas : ArgumentError « Cannot assign deleted material »
/// (TestUp test_material_Set_deleted_material).
#[test]
fn a_deleted_material_is_not_assigned() {
    let Some(r) = ev("m = Sketchup.active_model
         f = m.entities.add_face([0, 0, 5], [9, 0, 5], [9, 9, 5])
         e = m.entities.add_line([20, 0, 0], [21, 0, 0])
         mat = m.materials.add('Partie')
         m.materials.remove(mat)
         [err { f.back_material = mat }, err { f.material = mat }, err { e.material = mat }]")
    else {
        return;
    };
    let gone = "\"ArgumentError: Cannot assign deleted material\"";
    assert_eq!(r, format!("[{gone}, {gone}, {gone}]"));
}

/// Un arc de 0 à moins de 0,001 de 2π reste un cercle (son angle de fin devient
/// celui du début) ; `circular?` suit (TestUp test_circular_Query_*).
#[test]
fn nearly_whole_arcs_are_circles() {
    let Some(r) = ev("m = Sketchup.active_model
         arc = ->(a, b) { m.entities.add_group.entities.add_arc(ORIGIN, X_AXIS, Z_AXIS, 500.mm, a, b, 16).first.curve }
         near = arc.(0, 2 * Math::PI - 1.0e-4)
         [near.circular?, near.end_angle, arc.(0, 2 * Math::PI - 0.01).circular?, arc.(15.degrees, 375.degrees).circular?,
          arc.(0, 15.degrees).circular?]")
    else {
        return;
    };
    assert_eq!(r, "[true, 0.0, false, true, false]");
}

/// Les options d'impression sont celles de SketchUp 2026 (CPrintOptions)
/// `OptionsManager#[]` et `OptionsProvider#[]` prennent un nom ou un index par
/// rb_num2long.
#[test]
fn options_are_sketchups() {
    let Some(r) = ev("o = Sketchup.active_model.options
         po = o['PrintOptions']
         [po.keys.sort, po['PrintWidth'], po['PixelsPerInch'], po['ModelExtents'], o[0].name == o.keys[0], o[-1].name == o.keys[-1],
          o[99], cls { o[nil] }, o['UnitsOptions'][1] == o['UnitsOptions'][o['UnitsOptions'].keys[1]], cls { o['UnitsOptions'][nil] }]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[[\"ComputeSizeFromScale\", \"FitToPage\", \"LineWeight\", \"ModelExtents\", \"NumberOfPages\", \"PixelsPerInch\", \
         \"PrintHeight\", \"PrintQuality\", \"PrintWidth\", \"QualityAdjustment\", \"ScaleAdjustment\", \"SectionSlice\", \
         \"SizeInModel\", \"SizeInPrint\", \"VectorMode\"], 8.5, 150.0, true, true, true, nil, TypeError, true, TypeError]"
    );
}

/// Une surcouche retirée de son modèle a fini sa vie : `valid?` est faux et l'ajouter de nouveau
/// lève RuntimeError « invalid overlay » (TestUp test_valid_Query, test_remove_lifetime).
#[test]
fn a_removed_overlay_is_over() {
    let Some(r) = ev("ov = Sketchup::Overlay.new('esq.reel2.vie', 'Vie')
         ovs = Sketchup.active_model.overlays
         r = [ov.valid?, ovs.add(ov), ov.valid?, ovs.remove(ov), ov.valid?]
         r << err { ovs.add(ov) }")
    else {
        return;
    };
    assert_eq!(r, "[true, true, true, true, false, \"RuntimeError: invalid overlay\"]");
}

/// La visibilité d'un élément dans une scène est la liste des éléments cachés de cette scène, celle
/// que `update` remplit et que la sélection de la scène rétablit (
/// éléments du premier niveau et instances seulement, sinon faux).
#[test]
fn a_scene_hides_what_it_was_told_to_hide() {
    let Some(r) = ev("m = Sketchup.active_model
         cp = m.entities.add_cpoint(Geom::Point3d.new(10, 0, 0))
         p1 = m.pages.add('vis1')
         p2 = m.pages.add('vis2')
         g = m.entities.add_group
         inner = g.entities.add_cpoint(ORIGIN)
         r = [p1.set_drawingelement_visibility(cp, true), p2.set_drawingelement_visibility(cp, false),
              p1.get_drawingelement_visibility(cp), p2.get_drawingelement_visibility(cp),
              p2.hidden_entities.include?(cp), p2.set_drawingelement_visibility(inner, false)]
         m.pages.selected_page = p1
         r << cp.hidden?
         m.pages.selected_page = p2
         r << cp.hidden?")
    else {
        return;
    };
    assert_eq!(r, "[true, true, true, false, true, false, false, true]");
}
/// Une scène garde ses dictionnaires d'attributs (clé `h:g:0:<uid>`, comme le lecteur .skp) même
/// sans repère de même numéro, et ils meurent avec elle (TestUp test_erase_ensure_no_zombie).
#[test]
fn a_scene_keeps_its_dictionaries_until_erased() {
    let Some(r) = ev("pages = Sketchup.active_model.pages
         pg = pages.add('attrs')
         pg.set_attribute('Test', 'foo', 'bar')
         ads = pg.attribute_dictionaries
         ad = ads['Test']
         ax = pg.axes
         r = [pg.get_attribute('Test', 'foo'), ad.deleted?]
         pages.erase(pg)
         r + [pg.deleted?, ads.deleted?, ad.deleted?, ax.deleted?]")
    else {
        return;
    };
    assert_eq!(r, "[\"bar\", false, true, true, true, true]");
}
/// Les axes d'une scène changent hors de toute opération : aucune transaction n'est annoncée, alors
/// que ceux du modèle en annoncent une (TestUp test_set_axes_parent_to_page / _to_model).
#[test]
fn a_scenes_axes_change_without_a_transaction() {
    let Some(r) = ev("heard = []
         ob = Class.new(Sketchup::ModelObserver) do
           define_method(:onTransactionStart) { |_m| heard << :start }
           define_method(:onTransactionCommit) { |_m| heard << :commit }
         end.new
         m = Sketchup.active_model
         pg = m.pages.add('repere')
         m.add_observer(ob)
         x = Geom::Vector3d.new(3, 5, 0)
         pg.axes.set([10, 0, 0], x, x * Z_AXIS, Z_AXIS)
         r = [heard.dup, pg.axes.origin.to_a]
         m.axes.set([5, 0, 0], X_AXIS, Y_AXIS, Z_AXIS)
         m.remove_observer(ob)
         r << heard")
    else {
        return;
    };
    assert_eq!(r, "[[], [10.0, 0.0, 0.0], [:start, :commit]]");
}

/// Un dictionnaire effacé (ici par un observateur, hors opération) n'a plus de parent : TypeError
/// comme toute méthode d'une entité effacée (TestUp test_attribute_dictionary_evil_observer_without_operation).
#[test]
fn a_deleted_dictionary_has_no_parent() {
    let Some(r) = ev("ob = Class.new(Sketchup::EntityObserver) do
           define_method(:onChangeEntity) { |e| e.attribute_dictionaries.delete('Mal2') }
         end.new
         edge = Sketchup.active_model.entities.add_line([0, 0, 0], [5, 5, 5])
         edge.add_observer(ob)
         d = edge.attribute_dictionary('Mal2', true)
         edge.remove_observer(ob)
         [d.deleted?, cls { d.parent }, edge.attribute_dictionary('Autre2', true).parent.class]")
    else {
        return;
    };
    assert_eq!(r, "[true, TypeError, Sketchup::AttributeDictionaries]");
}
/// La couleur d'une matière texturée est celle de la texture, avec l'octet alpha que garde la
/// matière (0 pour celles des fichiers de SketchUp 2015).
#[test]
fn a_textured_colour_keeps_the_stored_alpha() {
    let Some(i) = interp() else { return };
    let mut m = Model::new();
    let id = m.add_material("Brique", [1, 2, 3]);
    let tex = esq_core::meta::TextureData {
        filename: "brique.jpg".into(),
        image: esq_core::photo::Blob(std::sync::Arc::new(Vec::new())),
        px: [8, 4],
        size: [100.0, 100.0],
        avg: [144, 143, 146],
    };
    m.materials[id].ext.texture = Some(tex);
    m.materials[id].ext.color_alpha = Some(0);
    let o = i.run("Sketchup.active_model.materials['Brique'].color.to_a", "alpha.rb", &mut m, &mut Vec::new()).unwrap();
    assert!(o.ok, "{}", o.error);
    assert_eq!(o.result, "[144, 143, 146, 0]");
}

/// Le plan ajusté est celui des moindres carrés de SketchUp (
/// sommes centrées, la coordonnée du plus grand déterminant prise à 1, sens de la normale en
/// éventail) ; des points qui ne fixent pas de plan laissent [0, 0, 1, 0] ; un seul argument
/// doit être un tableau (TypeError sinon).
#[test]
fn planes_are_fitted_like_sketchup() {
    let Some(r) =
        ev("c = Geom.fit_plane_to_points([20, 10, 30], [60, 30, 20], [40, 60, 30], [35, 35, 60], [15, 20, 10])
         [c.map { |v| v.round(9) }, Geom.fit_plane_to_points([10, 0, 0], [10, 0, 0], [10, 0, 0]),
          Geom.fit_plane_to_points([10, 0, 0], [25, 0, 0], [60, 0, 0]),
          Geom.fit_plane_to_points([[0, 0, 0], [25, 0, 60], [60, 0, 30]]).map { |v| v + 0.0 },
          Geom.fit_plane_to_points([0, 0, 0], [60, 0, 30], [25, 0, 60]).map { |v| v + 0.0 },
          cls { Geom.fit_plane_to_points([Geom::Point3d.new(1, 1, 1)]) }, err { Geom.fit_plane_to_points([]) }]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[[0.907282271, -0.420447882, 0.007903156, -18.050807549], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 1.0, 0.0], \
         [0.0, 1.0, 0.0, 0.0], [0.0, -1.0, 0.0, 0.0], TypeError, \"ArgumentError: At least three points needed to fit a plane\"]"
    );
}

/// La droite commune à deux plans passe par le point où s'annule la coordonnée de la plus
/// grande composante de sa direction n1 × n2.
#[test]
fn planes_meet_like_sketchup() {
    let Some(r) = ev(
        "a = Geom.intersect_plane_plane([Geom::Point3d.new(10, 0, 0), X_AXIS], [Geom::Point3d.new(0, 20, 0), Y_AXIS])
         b = Geom.intersect_plane_plane([[0, 0, 5], [0, 1, 1]], [[3, 0, 0], [1, 0, 0]])
         [a[0].to_a, a[1].to_a, b[0].to_a.map { |v| v.round(9) + 0.0 }]",
    ) else {
        return;
    };
    assert_eq!(r, "[[10.0, 20.0, 0.0], [0.0, 0.0, 1.0], [3.0, 0.0, 5.0]]");
}

/// `Geom.tesselate` suit le tesselateur GLU de SketchUp (règle positive) : une
/// boucle tournée comme le contour est remplie, une boucle inverse est un trou ; les points sont
/// projetés le long du plus grand axe de la normale de libtess (deux points confondus n'en font
/// qu'un) ; un polygone dégénéré ne donne rien.
#[test]
fn tesselation_follows_the_positive_winding() {
    let Some(r) = ev("sq = [[0, 0, 0], [90, 0, 0], [90, 90, 0], [0, 90, 0]]
         h1 = [[10, 10, 0], [20, 10, 0], [15, 15, 0]]
         h2 = [[30, 20, 0], [50, 20, 0], [50, 30, 0], [30, 30, 0]]
         [Geom.tesselate(sq, h1, h2).size, Geom.tesselate(sq, h1.reverse, h2.reverse).size,
          Geom.tesselate([[0, 0, 0], [7, 0, 0], [3, 0, 0]]), Geom.tesselate([[0, 0, 0], [7, 0, 7], [7, 7, 0], [0, 7, 0]]).size,
          Geom.tesselate([[0, 0, 0], [7, 0, 3], [7, 7, 0], [0, 7, 0]]).size]")
    else {
        return;
    };
    assert_eq!(r, "[48, 39, [], 3, 6]");
}
/// `Transformation.interpolate` est SUTransformationInterpolate : origines mêlées,
/// rotations des parties 3 × 3 lues en quaternions (un miroir y devient une
/// rotation quelconque) et interpolées, sans l'échelle.
#[test]
fn transformations_interpolate_like_sketchup() {
    let Some(r) = ev("t1 = Geom::Transformation.new([10, 20, 30])
         t2 = Geom::Transformation.axes(ORIGIN, Y_AXIS, X_AXIS, Z_AXIS)
         a = Geom::Transformation.interpolate(t1, t2, 0.25).to_a
         s = Geom::Transformation.interpolate(Geom::Transformation.new, Geom::Transformation.scaling(2), 0.5).to_a
         [a, s == Geom::Transformation.new.to_a]")
    else {
        return;
    };
    assert_eq!(r, "[[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 7.5, 15.0, 22.5, 1.0], true]");
}
/// Les conversions UTM sont celles de SketchUp : séries de Snyder sur l'ellipsoïde WGS 84 de sa
/// table, calculées dans son ordre, bandes de C à
/// X sans I ni O ; au sud, 10 000 km moins la distance.
#[test]
fn utm_conversions_are_sketchups() {
    let Some(r) = ev("u = Geom::LatLong.new(40.017, 105.283).to_utm.to_a
         l = Geom::UTM.new(13, 'T', 475849.37521, 4429682.73749).to_latlong.to_a
         s = Geom::LatLong.new(-10.5, -40.25).to_utm
         [u, l, s.zone_letter, s.y > 10_000_000.0, s.to_latlong.to_a.map { |v| v.round(9) }]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[[48, \"T\", 524150.8205633243, 4429682.4074293105], [40.01700297974308, -105.2829977182344], \"L\", true, [-10.5, -40.25]]"
    );
}
