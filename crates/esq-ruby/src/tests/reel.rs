//! Écarts trouvés en lançant dans Esquisse du code Ruby écrit pour SketchUp : la suite officielle
//! de tests de l'API (TestUp, github.com/SketchUp/testup-2) et les extensions d'exemple de
//! l'organisation SketchUp. Chaque test reprend l'écart avec un extrait écrit
//! pour Esquisse.

use super::*;

/// `Sketchup.mdi?` répondait vrai : TestUp (`discard_all_models`) fermait alors les modèles
/// jusqu'à ce que `active_model` soit nul, ce qui n'arrive jamais sous Windows (fermer le modèle
/// en ouvre un neuf) : boucle sans fin. SketchUp.exe ne répond vrai que si
/// l'interrupteur de fonction « MDI » est actif.
#[test]
fn sketchup_is_not_mdi_on_windows() {
    let Some((o, _)) = must(
        "m = Sketchup.active_model
         m.close(true)
         [Sketchup.mdi?, Sketchup.active_model.nil?, Sketchup.method(:mdi?).arity]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[false, false, 0]");
}

/// Les constantes géométriques étaient modifiables : un test de TestUp qui vérifie que
/// `ORIGIN_2D[0] = 1` échoue la déplaçait pour tous les suivants. SketchUp les gèle toutes
/// (`rb_obj_freeze`) ; leurs copies restent modifiables.
#[test]
fn geometric_constants_are_frozen() {
    let Some((o, _)) = must(
        "cs = [ORIGIN, X_AXIS, Y_AXIS, Z_AXIS, IDENTITY, ORIGIN_2D, X_AXIS_2D, Y_AXIS_2D, IDENTITY_2D]
         r = [cs.all?(&:frozen?), Geom::ORIGIN.equal?(ORIGIN)]
         r << (begin; ORIGIN.x = 1; rescue FrozenError; :gel; end)
         r << (begin; ORIGIN_2D[0] = 1; rescue FrozenError; :gel; end)
         r << (begin; IDENTITY_2D.invert!; rescue FrozenError; :gel; end)
         c = ORIGIN.clone; d = X_AXIS.dup; t = IDENTITY.clone
         c.x = 2; d.x = 3; t.set!(Geom::Transformation.translation([1, 0, 0]))
         r + [c.frozen?, d.frozen?, t.frozen?, ORIGIN.to_a, X_AXIS.to_a, IDENTITY.identity?, ORIGIN_2D.to_a]",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[true, true, :gel, :gel, :gel, false, false, false, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], true, [0.0, 0.0]]"
    );
}

/// `group.transform!([10, 20, 30])` levait TypeError (TC_Sketchup_InputPoint, InstancePath… le
/// font dans leur `setup`). SketchUp convertit l'argument de `transformation=`, `transform!` et
/// `move!` comme partout ailleurs : point ou vecteur = translation, nombre =
/// échelle uniforme, 16 nombres = matrice.
#[test]
fn instance_transformations_accept_what_sketchup_converts() {
    let Some((o, _)) = must(
        "g = Sketchup.active_model.entities.add_group
         g.entities.add_line([0, 0, 0], [1, 0, 0])
         g.transform!([10, 20, 30])
         r = [g.transformation.origin.to_a]
         g.transform!(Geom::Vector3d.new(1, 1, 1))
         r << g.transformation.origin.to_a
         g.transformation = 2
         r << g.transformation.to_a.values_at(0, 5, 10, 12)
         g.move!(Geom::Point3d.new(4, 5, 6))
         r << g.transformation.origin.to_a
         r << (begin; g.transformation = 'a'; rescue TypeError => e; e.message; end)",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[[10.0, 20.0, 30.0], [11.0, 21.0, 31.0], [2.0, 2.0, 2.0, 0.0], [4.0, 5.0, 6.0], \"no implicit conversion to Transformation\"]"
    );
}

/// Grouper une face tournée vers le bas retournait ses arêtes : la face recréait les siennes dans
/// l'ordre de sa boucle. TestUp (TC_Sketchup_Text, TC_Sketchup_Entities) cherche ensuite l'arête
/// (9,0,0)→(9,9,0) de la face qu'il a dessinée et ne la trouvait plus. SketchUp garde les arêtes.
#[test]
fn grouping_keeps_edge_directions() {
    let Some((o, _)) = must(
        "m = Sketchup.active_model
         f = m.entities.add_face([0, 0, 0], [9, 0, 0], [9, 9, 0], [0, 9, 0])
         seg = ->(es) { es.map { |e| [e.start.position.to_a, e.end.position.to_a] }.sort }
         before = seg.(f.edges)
         g = m.entities.add_group(f)
         after = seg.(g.entities.grep(Sketchup::Edge))
         c = g.to_component
         [before == after, seg.(c.definition.entities.grep(Sketchup::Edge)) == before,
          before.include?([[9.0, 0.0, 0.0], [9.0, 9.0, 0.0]]), c.definition.entities.grep(Sketchup::Face)[0].normal.to_a]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[true, true, true, [0.0, 0.0, -1.0]]");
}

/// `model.attribute_dictionaries` d'un modèle neuf était nil : le `setup` de
/// TC_Sketchup_AttributeDictionary (`model.attribute_dictionaries.delete(nom)`) échouait. Les
/// modèles de SketchUp sortent d'un gabarit qui en porte déjà ; une entité, elle, reste à nil.
#[test]
fn a_model_always_has_attribute_dictionaries() {
    let Some((o, _)) = must(
        "m = Sketchup.active_model
         ds = m.attribute_dictionaries
         r = [ds.class.name, ds.size, ds.delete('Absent').equal?(ds), ds.valid?]
         m.set_attribute('Essai', 'k', 1)
         r << m.attribute_dictionaries.map(&:name)
         m.attribute_dictionaries.delete('Essai')
         r << m.attribute_dictionaries.size << m.entities.add_line([0, 0, 0], [1, 0, 0]).attribute_dictionaries",
    ) else {
        return;
    };
    assert_eq!(o.result, "[\"Sketchup::AttributeDictionaries\", 0, true, true, [\"Essai\"], 0, nil]");
}

/// `90.degrees` valait π/2 exact ; SketchUp multiplie par 0,0174532925199433 (SUDegreesToRadians),
/// TestUp (TC_Numeric#test_degrees) attend 1.570796326794897.
#[test]
fn degrees_use_sketchup_factors() {
    let Some((o, _)) = must("[90.degrees, 45.degrees, (Math::PI / 2).radians, 1.radians, 180.degrees.radians]") else {
        return;
    };
    // L'aller-retour n'est pas exact, comme dans SketchUp.
    assert_eq!(o.result, "[1.570796326794897, 0.7853981633974485, 90.0, 57.29577951308232, 180.00000000000006]");
}

/// `Length.constants` montrait `:OWN`, une liste interne d'Esquisse ; TestUp
/// (TC_Length#test_class_constants_count) compare la liste à celle de SketchUp.
#[test]
fn length_constants_are_sketchup_ones() {
    let Some((o, _)) =
        must("[Length.constants.include?(:OWN), Length.constants.size, Length::Decimal, Length::USGallon]")
    else {
        return;
    };
    assert_eq!(o.result, "[false, 24, 0, 7]");
}

/// `''.to_l` levait ArgumentError ; pour SketchUp une chaîne vide vaut 0 (TestUp,
/// TC_String#test_to_l_empty_string). Le reste de ce qui n'est pas une longueur lève toujours.
#[test]
fn an_empty_string_is_a_zero_length() {
    let Some((o, _)) = must("[''.to_l, ''.to_l.class, (begin; 'abc'.to_l; rescue ArgumentError => e; e.message; end)]")
    else {
        return;
    };
    assert_eq!(o.result, "[0.0, Length, \"Cannot convert \\\"abc\\\" to Length\"]");
}

/// `Sketchup.format_area` écrivait « ~ 156,29 ft² » : le formateur d'aires et de volumes de
/// SketchUp n'en met jamais, retire les zéros de fin et choisit l'unité selon le format des
/// longueurs (TestUp, TC_Sketchup#test_format_area_*).
#[test]
fn areas_and_volumes_are_formatted_as_sketchup_does() {
    let Some((o, _)) = must(
        "u = Sketchup.active_model.options['UnitsOptions']
         u['LengthUnit'] = Length::Inches
         u['LengthPrecision'] = 2
         u['AreaPrecision'] = 2 if u.key?('AreaPrecision')
         u['VolumePrecision'] = 2 if u.key?('VolumePrecision')
         a = 3300.mm * 4400.mm
         sep = Sketchup::RegionalSettings.decimal_separator
         r = []
         [Length::Decimal, Length::Architectural, Length::Engineering, Length::Fractional].each do |f|
           u['LengthFormat'] = f
           u['AreaUnit'] = Length::SquareFeet
           r << Sketchup.format_area(a).tr(sep, '.')
         end
         u['LengthFormat'] = Length::Decimal
         u['AreaUnit'] = Length::SquareMeter
         r << Sketchup.format_area(1550.0031).tr(sep, '.') << Sketchup.format_area(1000).tr(sep, '.')
         u['LengthFormat'] = Length::Architectural
         r << Sketchup.format_area(100).tr(sep, '.') << Sketchup.format_volume(1728 * 2).tr(sep, '.')
         u['LengthFormat'] = Length::Decimal
         u['VolumeUnit'] = Length::CubicMeter
         r << Sketchup.format_volume(3300.mm * 4400.mm * 5500.mm).tr(sep, '.')",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[\"156.29 ft²\", \"156.29 ft²\", \"156.29 ft²\", \"22506.05 in²\", \"1 m²\", \"0.65 m²\", \"100 in²\", \
         \"2 ft³\", \"79.86 m³\"]"
    );
}

/// Une longueur arrondie s'écrivait « ~1,33' » ; SketchUp écrit L"~ ", et TestUp
/// (TC_Sketchup#test_format_length_engineering_inches, _m) attend « ~ 1,33' » et « ~ 0,41 m ».
#[test]
fn a_rounded_length_starts_with_a_tilde_and_a_space() {
    let Some((o, _)) = must(
        "u = Sketchup.active_model.options['UnitsOptions']
         u['LengthFormat'] = Length::Engineering
         u['LengthUnit'] = Length::Inches
         u['LengthPrecision'] = 2
         sep = Sketchup::RegionalSettings.decimal_separator
         r = [Sketchup.format_length(16.inch).tr(sep, '.'), 16.inch.to_s.tr(sep, '.')]
         u['LengthFormat'] = Length::Decimal
         u['LengthUnit'] = Length::Meter
         r << Sketchup.format_length(16.inch).tr(sep, '.') << (-16.inch).to_s.tr(sep, '.') << 16.inch.to_s.to_l.round(2)",
    ) else {
        return;
    };
    assert_eq!(o.result, "[\"~ 1.33'\", \"~ 1.33'\", \"~ 0.41 m\", \"~ -0.41 m\", 16.14]");
}

/// `Sketchup.format_length(nil)` rendait « 0 mm » : SketchUp convertit la valeur avec
/// rb_num2dbl, nil et les chaînes lèvent TypeError (TestUp, TC_Sketchup#test_format_*_invalid_*).
#[test]
fn format_functions_convert_their_value_as_sketchup_does() {
    let Some((o, _)) = must(
        "%i[format_length format_area format_volume format_angle format_degrees].flat_map do |m|
           [nil, '123'].map { |v| begin; Sketchup.send(m, v); :accepte; rescue TypeError => e; e.class; end }
         end + [Sketchup.format_length(1).class, Sketchup.format_area(2.5).class]",
    ) else {
        return;
    };
    assert_eq!(o.result, format!("[{}String, String]", "TypeError, ".repeat(10)));
}

/// `Sketchup.format_length(16.inch, -4)` rendait « ~406 mm » : une précision négative veut dire
/// « sans zéros de fin » (TestUp, TC_Sketchup#test_format_length_custom_precision_*), une
/// positive ôte le « ~ », et la précision plafonne à 14.
#[test]
fn format_length_precision_follows_sketchup() {
    let Some((o, _)) = must(
        "u = Sketchup.active_model.options['UnitsOptions']
         u['LengthFormat'] = Length::Decimal
         u['LengthUnit'] = Length::Millimeter
         u['LengthPrecision'] = 2
         sep = Sketchup::RegionalSettings.decimal_separator
         [Sketchup.format_length(16.inch, 4), Sketchup.format_length(16.inch, -4), Sketchup.format_length(12.3456789.mm, 4),
          Sketchup.format_length(0.5.mm, 0), Sketchup.format_length(1.mm / 3, 20).size,
          (begin; Sketchup.format_length(1, nil); rescue TypeError => e; e.class; end)].map { |s| s.is_a?(String) ? s.tr(sep, '.') : s }",
    ) else {
        return;
    };
    assert_eq!(o.result, "[\"406.4000 mm\", \"406.4 mm\", \"12.3457 mm\", \" 1 mm\", 19, TypeError]");
}

/// `Sketchup.open_file` : `with_status: true` rendait `[true, 0]` au lieu du code
/// LOAD_STATUS_*, un deuxième argument positionnel était accepté, nil levait ArgumentError, et
/// l'ancienne forme n'avertissait pas (TestUp, TC_Sketchup#test_open_file_*).
#[test]
fn open_file_arguments_and_results_follow_sketchup() {
    let Some((o, _)) = must(
        "def err; yield; :ok; rescue Exception => e; e.class; end
         require 'stringio'
         old, $stderr = $stderr, StringIO.new
         r = [Sketchup.open_file('absent.skp'), $stderr.string.include?('Deprecated overload, use with_status: true')]
         $stderr = StringIO.new
         r << Sketchup.open_file('absent.skp', with_status: true) << $stderr.string.empty?
         $stderr = old
         r + [err { Sketchup.open_file }, err { Sketchup.open_file('a.skp', 123) }, err { Sketchup.open_file(nil) },
              err { Sketchup.open_file(123) }]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[false, true, false, true, ArgumentError, ArgumentError, TypeError, TypeError]");
}

/// `Sketchup.resize_viewport` appelait une opération d'hôte absente (RuntimeError) quels que
/// soient ses arguments ; SketchUp les vérifie d'abord (TestUp,
/// TC_Sketchup#test_resize_viewport_*). Esquisse répond false : il ne redimensionne pas.
#[test]
fn resize_viewport_checks_its_arguments() {
    let Some((o, _)) = must(
        "def err; yield; rescue Exception => e; e.class; end
         m = Sketchup.active_model
         [err { Sketchup.resize_viewport(nil, 3048, 2870) }, err { Sketchup.resize_viewport(3000, 3048, 2870) },
          err { Sketchup.resize_viewport(m, Sketchup::Face, 2870) }, err { Sketchup.resize_viewport(m, 3048, Sketchup::Face) },
          err { Sketchup.resize_viewport }, err { Sketchup.resize_viewport(m, 3084, 4502, 3056) },
          Sketchup.resize_viewport(m, 800, 600), Sketchup.resize_viewport(m, 800.5, 600, logical_pixels: true)]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[TypeError, TypeError, TypeError, TypeError, ArgumentError, ArgumentError, false, false]");
}

/// `UI.set_clipboard_data(nil)` vidait le presse-papiers (`nil.to_s`) ; SketchUp convertit avec
/// StringValue et lève TypeError (TestUp, TC_UI#test_set_clipboard_data_invalid_argument_*).
/// Le test ne touche pas au vrai presse-papiers : l'appel à l'hôte est remplacé.
#[test]
fn set_clipboard_data_wants_a_string() {
    let Some((o, _)) = must(
        "calls = []
         Esq.singleton_class.alias_method(:__ui_host_reel, :ui_host)
         Esq.define_singleton_method(:ui_host) { |op, args = {}, fb = nil| op == 'clip_set' ? (calls << args[:text]; true) : __ui_host_reel(op, args, fb) }
         s = Object.new
         def s.to_str; 'converti'; end
         begin
           r = [nil, 123456].map { |v| begin; UI.set_clipboard_data(v); :accepte; rescue TypeError => e; e.class; end }
           r + [UI.set_clipboard_data('Bonjour'), UI.set_clipboard_data(s), calls]
         ensure
           Esq.singleton_class.alias_method(:ui_host, :__ui_host_reel)
         end",
    ) else {
        return;
    };
    assert_eq!(o.result, "[TypeError, TypeError, true, true, [\"Bonjour\", \"converti\"]]");
}

/// `require 'sketchup.rb'`, par quoi commencent presque toutes les extensions (et les exemples
/// de l'organisation SketchUp), levait LoadError, comme `require 'langhandler.rb'`
/// (TC_LanguageHandler) ; LanguageHandler n'existait pas. Les fichiers Tools de SketchUp sont
/// déjà chargés : `require` rend false.
#[test]
fn sketchup_tools_files_are_already_loaded() {
    let Some((o, _)) = must(
        r#"r = %w[sketchup.rb extensions.rb langhandler.rb sketchup extensions].map { |f| require(f) }
         r << Kernel.require('sketchup.rb')
         r << (begin; require('absent_esq_reel'); rescue LoadError; :load_error; end)
         require 'tmpdir'
         Dir.mktmpdir do |d|
           res = File.join(d, 'mon_ext', 'Resources', 'en-US')
           FileUtils.mkdir_p(res)
           File.binwrite(File.join(res, 'mon_ext.strings'),
             "\xEF\xBB\xBF// essai\n\"Bonjour\"=\"Hello\";\n/* \"Cache\"=\"x\";\n*/\"Dire \"oui\"\"=\"Say \"yes\"\"; // fin\n\"URL http://a/\"=\"http://b/\";\n")
           File.write(File.join(d, 'mon_ext.rb'), "$esq_lh = LanguageHandler.new('mon_ext.strings')\n")
           load File.join(d, 'mon_ext.rb')
           h = $esq_lh
           r << h.GetStrings.size << h['Bonjour'] << h.GetString('Dire "oui"') << h.GetString('URL http://a/')
           r << h.GetString('Absent') << h.GetString(nil) << h.GetResourcePath('mon_ext.strings').end_with?('mon_ext/Resources/en-US/mon_ext.strings')
           r << h.resource_path('absent.strings') << (begin; h.GetResourcePath(nil); rescue ArgumentError; :arg; end)
           r << h.GetStrings['Bonjour'].encoding.name << LanguageHandler.new('absent.strings').strings.size
         end
         r"#,
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        r#"[false, false, false, false, false, false, :load_error, 3, "Hello", "Say \"yes\"", "http://b/", "Absent", nil, true, "", :arg, "UTF-8", 0]"#
    );
}

/// `add_separator_to_menu`, `inputbox`, `require_all` et `show_ruby_panel` (Tools/sketchup.rb)
/// manquaient : l'extension Shapes de l'organisation SketchUp s'arrêtait au chargement sur
/// `add_separator_to_menu("Draw")`.
#[test]
fn sketchup_rb_top_level_functions_exist() {
    let Some((o, _)) = must(
        r#"require 'tmpdir'
         before = Esq.menu_items.size
         add_separator_to_menu('Draw'); add_separator_to_menu('Draw'); add_separator_to_menu('Tools')
         seps = Esq.menu_items[before..].map { |i| i[:sep] }
         r = [seps, Object.private_method_defined?(:inputbox), Object.private_method_defined?(:show_ruby_panel)]
         Dir.mktmpdir do |d|
           File.write(File.join(d, 'a_esq_reel.rb'), "$esq_reel_all = ($esq_reel_all || 0) + 1\n")
           File.write(File.join(d, 'b_esq_reel.rb'), "$esq_reel_all += 10\n")
           require_all(d)
           require_all(d)
           r << $esq_reel_all << $LOAD_PATH.include?(File.expand_path(d))
         end
         r"#,
    ) else {
        return;
    };
    assert_eq!(o.result, "[[true, true], true, true, 11, true]");
}

/// `add_face` avec un dernier point égal au premier levait « Duplicate points in array » : la
/// boîte de l'extension Shapes (github.com/SketchUp/sketchup-shapes) ne se dessinait pas.
/// SketchUp ôte ce point de fermeture et ne refuse que deux points consécutifs
/// confondus.
#[test]
fn add_face_drops_a_closing_point() {
    let Some((o, _)) = must(
        "ents = Sketchup.active_model.entities
         f = ents.add_face([[0,0,0], [5,0,0], [5,4,0], [0,4,0], [0,0,0]])
         def err; yield; rescue ArgumentError => e; e.message; end
         [f.vertices.size, f.area.round(6),
          err { ents.add_face([0,0,0], [1,0,0], [0,0,0]) },
          err { ents.add_face([10,0,0], [11,0,0], [11,0,0], [10,1,0]) },
          err { ents.add_face([20,0,0], [21,0,0], [21,0.0001,0], [20,1,0]) },
          ents.add_face([30,0,0], [31,0,0], [31,1,0], [30,0.0001,0]).vertices.size]",
    ) else {
        return;
    };
    // À moins de 0,001" du premier, le dernier point ferme la boucle : un triangle.
    assert_eq!(
        o.result,
        "[4, 20.0, \"Not enough points - at least 3 required\", \"Duplicate points in array\", \"Duplicate points in array\", 3]"
    );
}

/// Pousser-tirer un polygone (`add_ngon`) adoucissait ses arêtes verticales comme celles d'un
/// cercle : le prisme de l'extension Shapes sortait lisse. SketchUp garde les côtés d'un polygone
/// vifs, et le dessus reste un polygone.
#[test]
fn a_pushed_polygon_keeps_hard_sides() {
    let Some((o, _)) = must(
        "ents = Sketchup.active_model.entities
         soft = ->(f) { f.all_connected.grep(Sketchup::Edge).count(&:soft?) }
         p = ents.add_face(ents.add_ngon([0, 0, 0], Z_AXIS, 10, 6))
         p.reverse! if p.normal.z < 0
         p.pushpull(5)
         c = ents.add_face(ents.add_circle([50, 0, 0], Z_AXIS, 10, 16))
         c.reverse! if c.normal.z < 0
         c.pushpull(5)
         top = ents.grep(Sketchup::Face).find { |f| f.normal.z > 0.99 && f.bounds.center.x < 25 }
         [soft.(p), soft.(c), top.edges.map(&:curve).uniq.size, top.edges.first.curve.is_polygon?]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[0, 16, 1, true]");
}

/// Un cercle dessiné dans une face supprimait la face pour en créer deux neuves : le tube de
/// l'extension Shapes levait « reference to deleted Face » sur la face qu'il tenait. Dans
/// SketchUp la face coupée reste la même (elle y gagne un trou) et seul le disque est neuf.
#[test]
fn a_face_cut_by_new_edges_stays_the_same_face() {
    let Some((o, _)) = must(
        "ents = Sketchup.active_model.entities
         ring = ents.add_face(ents.add_circle(ORIGIN, Z_AXIS, 10, 16))
         inner = ents.add_circle(ORIGIN, Z_AXIS, 9, 16)
         disc = inner.first.faces.find { |f| f != ring }
         r = [ring.valid?, ring.loops.size, inner.first.faces.size, disc.loops.size]
         disc.erase!
         ring.reverse! if ring.normal.samedirection?(Z_AXIS.reverse)
         ring.pushpull(5)
         sq = ents.add_face([30, 0, 0], [40, 0, 0], [40, 10, 0], [30, 10, 0])
         ents.add_line([35, 0, 0], [35, 10, 0])
         halves = ents.grep(Sketchup::Face).select { |f| f.bounds.min.x >= 29.9 }
         r + [ents.grep(Sketchup::Face).size - halves.size, ents.grep(Sketchup::Edge).count(&:soft?), sq.valid?, halves.size,
              halves.include?(sq)]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[true, 2, 2, 1, 34, 32, true, 2, true]");
}

/// `View#draw_text` et `#text_bounds` acceptaient n'importe quelles options. SketchUp
///  les vérifie : TestUp (TC_Sketchup_View#test_draw_text_option_*,
/// #test_text_bounds_option_*) attend TypeError ou ArgumentError selon le cas.
#[test]
fn view_text_options_are_checked() {
    let Some((o, _)) = must(
        "v = Sketchup.active_model.active_view
         def err; yield; :ok; rescue TypeError, ArgumentError => e; e.class.name; end
         cases = [ORIGIN, { font: ORIGIN }, { font: 'ThisFontNameIsTooLongForWindows01234' }, { font: 'Tæsting てすと' },
                  { size: 'FooBar' }, { size: 0 }, { size: 10, pixel_size: 10 }, { color: ORIGIN }, { color: 'red' },
                  { align: -1 }, { align: 3 }, { align: :foobar }, { align: TextAlignRight },
                  { vertical_align: 4 }, { vertical_align: :foobar }, { vertical_align: TextVerticalAlignCenter }, {}]
         [cases.map { |c| err { v.draw_text(ORIGIN, 'Test', c) } }, cases.map { |c| err { v.text_bounds(ORIGIN, 'Test', c) } }].uniq",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[[\"TypeError\", \"TypeError\", \"ArgumentError\", :ok, \"TypeError\", :ok, \"ArgumentError\", \"TypeError\", :ok, \
         \"ArgumentError\", \"ArgumentError\", \"TypeError\", :ok, \"ArgumentError\", \"TypeError\", :ok, :ok]]"
    );
}

/// Un InstancePath dont un élément a été effacé se lisait comme si de rien n'était ; SketchUp
///  lève TypeError « invalid instance path - deleted elements » (TestUp,
/// TC_Sketchup_InstancePath#test_*_deleted_entities). `transformation(nil)` lève aussi.
#[test]
fn an_instance_path_with_deleted_elements_raises() {
    let Some((o, _)) = must(
        "ents = Sketchup.active_model.entities
         g = ents.add_group
         e = g.entities.add_line([0, 0, 0], [1, 0, 0])
         ip = Sketchup::InstancePath.new([g, e])
         r = [ip.size, ip.valid?]
         e.erase!
         def err; yield; :ok; rescue TypeError, ArgumentError, IndexError => x; \"#{x.class}: #{x.message}\"; end
         r << ip.valid? << err { ip.pop }
         ip2 = Sketchup::InstancePath.new([g, g.entities.add_line([0, 0, 0], [0, 1, 0])])
         g.entities.to_a.each(&:erase!)
         r += [err { ip2.to_a }, err { ip2.size }, err { ip2.empty? }, err { ip2.leaf }, err { ip2[0] }, err { ip2.each {} },
               err { ip2.include?(nil) }, err { ip2.root }, err { ip2.transformation }]
         ok = Sketchup::InstancePath.new([g])
         r + [err { ok.transformation(nil) }, err { ok.transformation(0, 0) }, ok.transformation(0).identity?]",
    ) else {
        return;
    };
    let deleted = "\"TypeError: invalid instance path - deleted elements\"";
    assert_eq!(
        o.result,
        format!(
            "[2, true, false, {}, \"TypeError: no implicit conversion from nil to integer\", \
             \"ArgumentError: wrong number of arguments (given 2, expected 0..1)\", true]",
            [deleted; 10].join(", ")
        )
    );
}

/// `add_classification(nil, 'IfcDoor')` rendait false : SketchUp lit les deux noms avec
/// StringValue et lève TypeError pour nil, un nombre ou un point (TestUp,
/// TC_Sketchup_ComponentDefinition#test_*_classification_invalid_*).
#[test]
fn classification_names_must_be_strings() {
    let Some((o, _)) = must(
        "d = Sketchup.active_model.definitions.add('Porte')
         def err; yield; rescue TypeError => e; e.class.name; end
         bad = [nil, 1, ORIGIN]
         bad.map { |b| err { d.add_classification(b, 'IfcDoor') } } + bad.map { |b| err { d.add_classification('IFC 2x3', b) } } +
           bad.map { |b| err { d.remove_classification(b, 'IfcDoor') } } + bad.map { |b| err { d.remove_classification('IFC 2x3', b) } } +
           [d.add_classification('Absent', 'IfcDoor'), d.remove_classification('Absent')]",
    ) else {
        return;
    };
    assert_eq!(o.result, format!("[{}false, false]", "\"TypeError\", ".repeat(12)));
}

/// `Geom::Bounds2d.new(x, y, w, h)` prenait les quatre nombres pour deux coins ; SketchUp
///  et la documentation de l'API y lisent x, y, largeur et hauteur. Les boîtes de
/// `View#text_bounds` en étaient fausses dès que le texte n'était pas à l'origine.
#[test]
fn bounds2d_takes_x_y_width_height() {
    let Some((o, _)) = must(
        "b = Geom::Bounds2d.new(5.0, 5.0, 7.0, 1.0)
         c = Geom::Bounds2d.new([[1, 1], [3, 3]])
         def err; yield; rescue TypeError, ArgumentError => e; \"#{e.class}: #{e.message}\"; end
         [b.upper_left.to_a, b.lower_right.to_a, b.width, b.height, c.width, Geom::Bounds2d.new([0, 0, 2, 3]).height,
          Geom::Bounds2d.new(b) == b, err { Geom::Bounds2d.new(5) }, err { Geom::Bounds2d.new(1, 2, 3) }]",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[[5.0, 5.0], [12.0, 6.0], 7.0, 1.0, 2.0, 3.0, true, \"TypeError: wrong argument type\", \"ArgumentError: Wrong number of arguments\"]"
    );
}

/// `View#draw` refusait un élément qui n'est pas un point (TypeError) et `draw(GL_POINTS)` sans
/// points passait ; SketchUp veut au moins un argument de points, compte les
/// points tels qu'ils sont donnés, puis ignore ceux qui ne sont pas des points, et les normales
/// ou uvs qui ne se convertissent pas. `text_bounds` et `draw_text` convertissent leur position
/// comme les autres arguments de point (tableau de deux nombres admis, sinon ArgumentError), et
/// `text_bounds` place sa boîte selon l'alignement (TestUp, TC_Sketchup_View).
#[test]
fn view_drawing_follows_sketchups_rules() {
    let Some((o, _)) = must(
        "v = Sketchup.active_model.active_view
         def err; yield; :ok; rescue TypeError, ArgumentError => e; e.class.name; end
         pts = [[0, 0, 0], [9, 0, 0], [9, 9, 0]]
         r = [err { v.draw(GL_POINTS) }, err { v.draw(GL_POINTS, []) }, err { v.draw(GL_TRIANGLES, pts, normals: [Z_AXIS, nil, Z_AXIS]) },
              err { v.draw(GL_TRIANGLES, pts, normals: [Z_AXIS, '[0,0,1]', Z_AXIS]) }, err { v.draw(GL_TRIANGLES, pts, normals: 'n') },
              err { v.draw2d(GL_QUADS, pts + [[0, 9, 0]], uvs: [[0, 0, 0], [1, 0, 0], nil, [0, 1, 0]]) },
              err { v.draw(GL_LINES, [ORIGIN, 'x']) }, err { v.draw(GL_LINES, [ORIGIN]) },
              err { v.draw_text('FooBar', 'Test') }, err { v.text_bounds('FooBar', 'Test') }]
         at = ->(o) { b = v.text_bounds([200, 300], 'Test', o); [b.upper_left.x, b.upper_left.y, b.lower_right.x] }
         left = at.({}); center = at.(align: TextAlignCenter); right = at.(align: TextAlignRight)
         base = at.(vertical_align: TextVerticalAlignBaseline); cap = at.(vertical_align: TextVerticalAlignCapHeight)
         r + [left[0..1], center[0] < 200, right[2], base[1] < cap[1], cap[1] < 300, v.text_bounds(ORIGIN, 'Test').upper_left.to_a]",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[\"ArgumentError\", :ok, :ok, :ok, \"TypeError\", :ok, :ok, \"ArgumentError\", \"ArgumentError\", \"ArgumentError\", \
         [200.0, 300.0], true, 200.0, true, true, [0.0, 0.0]]"
    );
}

/// `mesh.add_polygon([1, 2, 3])` prenait le tableau pour un point et levait « Not enough
/// points » ; SketchUp prend les éléments d'un tableau seul : ici trois indices
/// (TestUp, TC_Geom_PolygonMesh#test_add_polygon_array, #test_count_polygons).
#[test]
fn a_single_array_gives_the_polygon_indices() {
    let Some((o, _)) = must(
        "m = Geom::PolygonMesh.new
         [[0, 0, 0], [1, 0, 0], [1, 1, 0]].each { |p| m.add_point(p) }
         def err; yield; rescue ArgumentError => e; e.message; end
         [m.add_polygon([1, 2, 3]), m.add_polygon([1, 2, 3]), m.polygon_at(1), m.add_polygon([[0, 0, 0], [1, 0, 0], [1, 1, 0]]),
          m.count_points, m.count_polygons, err { m.add_polygon([0]) }, err { m.add_polygon([nil]) }]",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[1, 2, [1, 2, 3], 3, 3, 3, \"Not enough points - at least 3 required\", \"Not enough points - at least 3 required\"]"
    );
}

/// `Model#active_layer = nil` levait TypeError ; TestUp (`start_with_empty_model`) le fait
/// avant presque chaque test. SketchUp convertit l'argument comme pour `Drawingelement#layer=`:
///  nil ou "Layer0" = l'étiquette par défaut, un nom = cette étiquette, un nom
/// inconnu ou un autre type = ArgumentError.
#[test]
fn a_tag_argument_is_converted_as_sketchup_does() {
    let Some((o, _)) = must(
        r#"m = Sketchup.active_model
         murs = m.layers.add('Murs')
         m.active_layer = murs
         r = [m.active_layer.name]
         r << m.send(:active_layer=, nil).name << m.active_layer.name
         m.active_layer = 'Murs'
         r << m.active_layer.name
         m.active_layer = 'Layer0'
         r << m.active_layer.name
         r << (begin; m.active_layer = 'Absente'; rescue ArgumentError => e; e.message; end)
         r << (begin; m.active_layer = 3; rescue ArgumentError => e; e.message; end)
         e = m.entities.add_line([0, 0, 0], [1, 0, 0])
         e.layer = murs
         e.layer = nil
         r << e.layer.name
         r"#,
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        r#"["Murs", "Layer0", "Layer0", "Murs", "Layer0", "Cannot find tag named \"Absente\"", "No implicit conversion to tag", "Layer0"]"#
    );
}

/// `Entities#add_line`, `#add_edges`, `#add_circle`… faisaient une face dès qu'une boucle de
/// bords libres se fermait ; SketchUp ne le fait pas (TestUp : `TC_Sketchup_Edge#test_find_faces*`
/// attend 1 face trouvée après trois `add_line`). Ces bords coupent seulement les faces sur
/// lesquelles ils passent ; `Edge#find_faces` et `Entities#add_face` font les faces.
#[test]
fn new_edges_split_faces_but_make_none() {
    let Some((o, _)) = must(
        "ents = Sketchup.active_model.entities
         faces = -> { ents.grep(Sketchup::Face).size }
         a = ents.add_line([0, 0, 0], [10, 0, 0])
         ents.add_line([10, 0, 0], [10, 10, 0])
         ents.add_line([10, 10, 0], [0, 0, 0])
         r = [faces.call, a.find_faces, faces.call, a.find_faces]
         ents.add_edges([20, 0, 0], [30, 0, 0], [30, 10, 0], [20, 10, 0], [20, 0, 0])
         ents.add_circle([50, 0, 0], Z_AXIS, 5)
         ents.add_ngon([70, 0, 0], Z_AXIS, 5, 6)
         r << faces.call
         ents.add_face([0, 20, 0], [40, 20, 0], [40, 60, 0], [0, 60, 0])
         ents.add_line([20, 20, 0], [20, 60, 0])
         r << faces.call
         ents.add_edges([5, 30, 0], [10, 30, 0], [10, 40, 0], [5, 40, 0], [5, 30, 0])
         r << faces.call
         ents.add_circle([30, 40, 0], Z_AXIS, 4)
         r << faces.call",
    ) else {
        return;
    };
    assert_eq!(o.result, "[0, 1, 1, 0, 1, 3, 4, 5]");
}

/// `Model#drawing_element_visible?` prenait une entité ; SketchUp prend un
/// chemin (InstancePath ou tableau) et regarde chaque élément : étiquette ou dossier caché,
/// élément masqué selon DrawHiddenGeometry (bords, faces) ou DrawHiddenObjects (le reste).
/// TestUp : TC_Sketchup_Model#test_drawing_element_visible_Query_*.
#[test]
fn drawing_element_visibility_follows_the_path() {
    let Some((o, _)) = must(
        "m = Sketchup.active_model
         g = m.entities.add_group
         f = g.entities.add_face([0, 0, 0], [5, 0, 0], [5, 5, 0])
         tag = m.layers.add('Plans')
         g.layer = tag
         path = Sketchup::InstancePath.new([g, f])
         ro = m.rendering_options
         r = [m.drawing_element_visible?(path), m.drawing_element_visible?([g, f])]
         f.visible = false
         r << m.drawing_element_visible?(path)
         ro['DrawHiddenGeometry'] = true
         r << m.drawing_element_visible?(path)
         g.visible = false
         r << m.drawing_element_visible?(path)
         ro['DrawHiddenObjects'] = true
         r << m.drawing_element_visible?(path)
         dossier = m.layers.add_folder('Etage')
         tag.folder = dossier.add_folder('Sous')
         dossier.visible = false
         r << m.drawing_element_visible?(path)
         dossier.visible = true
         tag.visible = false
         r << m.drawing_element_visible?(path)
         def err; yield; :ok; rescue TypeError, ArgumentError => x; \"#{x.class}: #{x.message}\"; end
         r << err { m.drawing_element_visible?(f) }
         r << err { m.drawing_element_visible?([m.layers[0], f]) }
         c = m.entities.add_cpoint([1, 1, 1])
         c.hidden = true
         ro['DrawHiddenGeometry'] = false
         r << m.drawing_element_visible?([c])
         ro['DrawHiddenGeometry'] = true
         r << m.drawing_element_visible?([c])
         r",
    ) else {
        return;
    };
    // Un repère masqué suit DrawHiddenGeometry, pas DrawHiddenObjects.
    assert_eq!(
        o.result,
        "[true, true, false, true, false, true, false, false, \
         \"TypeError: Argument must be type of Sketchup::InstancePath or an Array representing an instance path\", \
         \"ArgumentError: Provided instance path is not valid.\", false, true]"
    );
}

/// `InstancePath.new` refusait un chemin mal emboîté ; SketchUp le construit
/// et c'est `valid?` qui le refuse, comme le chemin vide ou une feuille seule
/// prise dans une définition. `==` sur un chemin à élément effacé : vrai seulement pour le même
/// objet. TestUp : TC_Sketchup_InstancePath, TC_Sketchup_Entities.
#[test]
fn instance_paths_are_checked_by_valid() {
    let Some((o, _)) = must(
        "ents = Sketchup.active_model.entities
         g = ents.add_group
         e = g.entities.add_line([0, 0, 0], [1, 0, 0])
         h = ents.add_group
         h.entities.add_line([0, 0, 0], [0, 1, 0])
         top = ents.add_line([5, 0, 0], [6, 0, 0])
         P = Sketchup::InstancePath
         r = [P.new([g, h]).valid?, P.new([g, e]).valid?, P.new([]).valid?, P.new([e]).valid?,
              P.new([top]).valid?, P.new([g, h, e]).valid?, P.new([h, e]).valid?]
         r << (begin; P.new([e, g]); rescue ArgumentError => x; x.message; end)
         a = P.new([g, e])
         b = P.new([g, e])
         r << (a == b)
         g.erase!
         r << (a == b) << (a == a) << a.valid?
         r",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[false, true, false, false, true, false, false, \"invalid instance path\", true, false, true, false]"
    );
}

/// Textes et cotes attachés par un chemin d'instance : `add_text(t, [chemin, point])`,
/// `add_dimension_linear([chemin, point], …)` dont le point, en coordonnées
/// du modèle, doit tomber sur la feuille ; sur un bord, à un bout (la feuille devient ce
/// sommet) ou au milieu. Sans chemin, un bord ou une ligne de construction
/// suivi d'un point le projette sur sa droite, un sommet ou un point de construction donne sa
/// position. TestUp : TC_Sketchup_Text, TC_Sketchup_DimensionLinear,
/// TC_Sketchup_Entities#test_add_dimension_linear_*.
#[test]
fn annotations_attach_through_instance_paths() {
    let Some((o, _)) = must(
        "ents = Sketchup.active_model.entities
         g = ents.add_group
         e = g.entities.add_line([0, 0, 0], [10, 0, 0])
         c = g.entities.add_cpoint([0, 4, 0])
         g.transformation = Geom::Transformation.new([5, 0, 0])
         def err; yield; :ok; rescue TypeError, ArgumentError => x; \"#{x.class}: #{x.message}\"; end
         d = ents.add_dimension_linear([[g, e], [5, 0, 0]], [Sketchup::InstancePath.new([g, e]), [10, 0, 0]], [0, 5, 0])
         r = [d.start[0].class, d.end[0].class, d.start[1].to_a, d.start_attached_to[0].class, d.start_attached_to[0].to_a.size]
         r << err { ents.add_dimension_linear([[g, e], [7, 0, 0]], [[g, e], [15, 0, 0]], [0, 5, 0]) }
         r << err { ents.add_dimension_linear([[g, c], [0, 4, 0]], [[g, e], [15, 0, 0]], [0, 5, 0]) }
         r << err { ents.add_dimension_linear([[g], [5, 0, 0]], [[g, e], [15, 0, 0]], [0, 5, 0]) }
         r << err { ents.add_dimension_linear([g, [5, 0, 0]], [[g, e], [15, 0, 0]], [0, 5, 0]) }
         r << err { d.start_attached_to = 'x' } << err { d.start_attached_to = [[g, e]] }
         d.start_attached_to = [[g, c], [5, 4, 0]]
         r << d.start[0].class << d.start[1].to_a
         t = ents.add_text('t', [[g, e], [7, 0, 0]])
         r << t.attached_to[0].to_a.map(&:class) << t.has_leader?
         t.attached_to = [[g, e], [9, 0, 0]]
         r << t.point.to_a << err { t.attached_to = [[g, e]] }
         f = ents.add_line([0, 20, 0], [9, 20, 0])
         d2 = ents.add_dimension_linear([f, [1, 22, 3]], [f, [10, 22, 3]], [0, 0, 5])
         p1 = ents.add_cpoint([0, 30, 0])
         p2 = ents.add_cpoint([4, 30, 0])
         d3 = ents.add_dimension_linear(p1, p2, [0, 0, 1])
         r << d2.start[0].class << d2.start[1].to_a << d2.end[1].to_a << (d3.start[0] == p1) << d3.end[1].to_a
         r << err { ents.add_dimension_linear([0, 0, 0], [1, 0, 0], [0, 0, 0]) }
         r",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[Sketchup::Vertex, Sketchup::Edge, [5.0, 0.0, 0.0], Sketchup::InstancePath, 2, \
         \"ArgumentError: Invalid point on edge. Point must be transformed and either be the start, mid or end point.\", \
         \"ArgumentError: Make sure the point is correctly transformed from local to global space.\", \
         \"TypeError: Invalid instance path leaf entity.\", \
         \"TypeError: First element in the array is invalid. Must be Sketchup::InstancePath or an array containing Sketchup::ComponentInstance's and a valid leaf entity.\", \
         \"TypeError: wrong argument type String (expected Array)\", \"ArgumentError: Incorrect number of elements.\", \
         Sketchup::ConstructionPoint, [5.0, 4.0, 0.0], [Sketchup::Group, Sketchup::Edge], true, [9.0, 0.0, 0.0], \
         \"ArgumentError: Incorrect number of elements.\", Sketchup::Edge, [1.0, 20.0, 0.0], [10.0, 20.0, 0.0], true, \
         [4.0, 30.0, 0.0], \"ArgumentError: Invalid elements in the array.\"]"
    );
}

/// Un texte ou une cote attaché par un chemin d'instance suit l'instance déplacée, son vecteur
/// et son décalage restent ; l'instance effacée, il se détache là où il était
/// (`esq_core::attach`). TestUp : TC_Sketchup_Entities
/// `test_add_text_instance_path_after_move`, `test_add_dimension_linear_instance_path_after_move`.
#[test]
fn annotations_follow_their_instance_path() {
    let Some((o, _)) = must(
        "ents = Sketchup.active_model.entities
         f = ents.add_face([0, 0, 0], [9, 0, 0], [9, 9, 0], [0, 9, 0])
         c = ents.add_group(f).to_component
         e = c.definition.entities.grep(Sketchup::Edge).find { |x| x.start.position == [9, 0, 0] && x.end.position == [9, 9, 0] }
         ip = Sketchup::InstancePath.new([c, e])
         t = ents.add_text('su', [ip, e.start.position], [30, 30, 0])
         d = ents.add_dimension_linear([ip, e.start.position], [ip, e.end.position], [10, 10, 0])
         r = [t.point.to_a, t.attached_to[0].leaf == e]
         c.transformation = Geom::Transformation.new([12, 0, 0], [0, 1, 0], [0, 0, 1])
         r << t.point.to_a << t.vector.to_a << d.start[1].to_a << d.end[1].to_a << d.offset_vector.to_a
         c.transformation = Geom::Transformation.new([12, 12, 12])
         r << d.start[1].to_a << d.end[1].to_a
         c.definition.entities.transform_entities(Geom::Transformation.new([0, 0, 5]), c.definition.entities.to_a)
         r << d.end[1].to_a
         c.erase!
         r << t.point.to_a << t.attached_to << d.start_attached_to << d.end[1].to_a
         r",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[[9.0, 0.0, 0.0], true, [12.0, 9.0, 0.0], [30.0, 30.0, 0.0], [12.0, 9.0, 0.0], [12.0, 9.0, 9.0], \
         [10.0, 10.0, 0.0], [21.0, 12.0, 12.0], [21.0, 21.0, 12.0], [21.0, 21.0, 17.0], [21.0, 12.0, 17.0], \
         nil, nil, [21.0, 21.0, 17.0]]"
    );
}

/// `material=` refusait un entier ou un tableau ; SketchUp convertit en
/// couleur tout ce qui n'est ni nil, ni une Material, ni un nom : 0 et une couleur aux quatre
/// octets nuls ne mettent aucune matière, un nom inconnu qui n'est pas une couleur lève
/// « Cannot find material named … ». TestUp : TC_Sketchup_Drawingelement, TC_Sketchup_Face.
#[test]
fn material_values_are_converted_as_sketchup_does() {
    let Some((o, _)) = must(
        "f = Sketchup.active_model.entities.add_face([0, 0, 0], [5, 0, 0], [5, 5, 0])
         r = []
         [255, [255, 0, 0], [1.0, 0.0, 0.0], Sketchup::Color.new(0, 0, 255), 0, [0, 0, 0, 0]].each do |v|
           f.material = v
           r << (f.material && f.material.color.to_a)
         end
         f.back_material = [0, 128, 0]
         r << f.back_material.color.to_a
         r << (begin; f.material = 'pas_une_couleur'; rescue ArgumentError => e; e.message; end)
         r",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[[255, 0, 0, 255], [255, 0, 0, 255], [255, 0, 0, 255], [0, 0, 255, 255], nil, nil, [0, 128, 0, 255], \
         \"Cannot find material named pas_une_couleur\"]"
    );
}
