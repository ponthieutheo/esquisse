//! Zone « model » : Model, Selection, Material(s), Texture, ImageRep, Layer(s), LayerFolder,
//! HatchPattern(s), LineStyle(s), ShadowInfo, OptionsManager/Provider, Color, Sketchup.

use super::*;

/// `err { … }` / `cls { … }`: message / class of the exception the block raises (nil if none).
/// (`x.y = v rescue …` would rescue the value, not the assignment.)
const HELPERS: &str = "def err; yield; nil; rescue Exception => e; e.message; end
def cls; yield; nil; rescue Exception => e; e.class; end
";

/// Runs a script (with the helpers) on a fresh model.
fn mm(code: &str) -> Option<(RunOutput, Model)> {
    must(&format!("{HELPERS}{code}"))
}

/// Result (`inspect`) of a script run on a fresh model.
fn ev(code: &str) -> Option<String> {
    mm(code).map(|(o, _)| o.result)
}

/// A scratch folder of its own for a test.
fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("esq_model_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn rb_path(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// A 4×2 PNG: red on the left half, blue on the right.
fn png_bytes() -> Vec<u8> {
    let mut img = image::RgbaImage::new(4, 2);
    for (x, _, p) in img.enumerate_pixels_mut() {
        *p = if x < 2 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) };
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

#[test]
fn color_follows_sketchup() {
    let Some(r) = ev("c = Sketchup::Color.new
         d = Sketchup::Color.new(255, 0, 0)
         e = Sketchup::Color.new('Red')
         f = Sketchup::Color.new([0.5, 1.0, 2.0, 0.0])
         g = Sketchup::Color.new(0x0000ff)
         [c.to_a, d == e, f.to_a, g.to_a, d.to_i, d.to_s,
          (Sketchup::Color.new('pas une couleur') rescue $!.class), (Sketchup::Color.new(:x) rescue $!.class),
          err { d.red = 300 }, (d.alpha = 0.5; d.alpha), err { d.alpha = 2.0 },
          Sketchup::Color.names.size, Sketchup::Color.names.first,
          Sketchup::Color.new(0, 0, 0).blend(Sketchup::Color.new(200, 100, 50), 0.5).to_a, d == 1]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[[0, 0, 0, 255], true, [127, 255, 255, 0], [255, 0, 0, 255], 255, \"Color(255, 0, 0, 255)\", ArgumentError, \
         TypeError, \"Value must be between 0..255\", 127, \"Value must be between 0.0 and 1.0, or 0 and 255\", 140, \
         \"AliceBlue\", [100, 50, 25, 255], false]"
    );
}

#[test]
fn materials_add_rename_remove_purge() {
    let Some((o, m)) = mm("mats = Sketchup.active_model.materials
         a = mats.add('Brique')
         b = mats.add('Brique')
         c = mats.add
         a.color = [10, 20, 30]
         a.alpha = 0.25
         r = [a.name, b.name, c.name, mats.length, a.color.to_a, a.alpha, a.use_alpha?, a.materialType,
              err { b.name = 'Brique' }, mats['Brique'] == a, mats[1] == b, (mats[9] rescue $!.class),
              (mats.add(3) rescue $!.message), (mats.add('a', 'b') rescue $!.message)]
         f = Sketchup.active_model.entities.add_face([0,0,0], [1,0,0], [1,1,0])
         f.material = a
         mats.purge_unused
         r << mats.map(&:name)
         mats.remove(a)
         r + [mats.length, f.material]")
    else {
        return;
    };
    assert_eq!(
        o.result,
        "[\"Brique\", \"Brique1\", \"Material\", 3, [10, 20, 30, 255], 0.25, true, 0, \
         \"The provided name was not unique to the model.\", true, true, IndexError, \"Material name required\", \
         \"wrong # of arguments(2 for 0 or 1)\", [\"Brique\"], 0, nil]"
    );
    assert!(m.materials.is_empty());
}

#[test]
fn material_pbr_and_validation() {
    let Some(r) = ev("m = Sketchup.active_model.materials.add('Métal')
         m.metallic_factor = 0.75
         m.roughness_enabled = true
         [m.workflow, m.metallic_factor, m.roughness_enabled?, m.normal_scale,
          err { m.metallic_factor = 1.5 }, err { m.normal_scale = -1 },
          err { m.normal_scale = Float::INFINITY }, err { m.normal_enabled = true },
          cls { m.ao_enabled = true }, err { m.normal_style = 7 }, err { m.colorize_type = 3 },
          Sketchup::Material::MATERIAL_COLORIZED_TEXTURED, Sketchup::Material::OWNER_LAYER, m.owner_type, m.display_name]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[1, 0.75, true, 1.0, \"expected value between 0 and 1, got 1.5\", \"normal scale cannot be negative\", \
         \"normal scale cannot be Float::Infinity\", \"Failed to enable normal mapping: normal texture not set.\", \
         ArgumentError, \"expected NORMAL_STYLE_OPENGL or NORMAL_STYLE_DIRECTX, got 7\", \"Invalid colorization type\", \
         2, 2, 0, \"Métal\"]"
    );
}

#[test]
fn textures_and_image_reps() {
    let dir = scratch("tex");
    std::fs::write(dir.join("damier.png"), png_bytes()).unwrap();
    let d = rb_path(&dir);
    let Some(r) = ev(&format!(
        "m = Sketchup.active_model.materials.add('Tissu')
         m.texture = '{d}/damier.png'
         t = m.texture
         r = [m.materialType, t.filename, t.image_width, t.image_height, t.width, t.height, t.average_color.to_a, t.valid?]
         t.size = 8
         r += [t.width, t.height]
         m.texture = ['{d}/damier.png', 20, 10]
         r += [m.texture.width, m.texture.height]
         rep = m.texture.image_rep
         r += [rep.size, rep.bits_per_pixel, rep.colors.first.to_a, rep.color_at_uv(0.99, 0.0).to_a]
         r << m.texture.write('{d}/copie.png')
         m.color = 'Green'
         r << m.materialType
         m.texture = nil
         r += [m.texture, m.materialType, err {{ m.texture = 5 }}]
         img = Sketchup::ImageRep.new
         img.set_data(2, 1, 24, 2, [0, 0, 255, 255, 0, 0, 0, 0].pack('C*'))
         img.save_file('{d}/rep.png')
         r += [Sketchup::ImageRep.new('{d}/rep.png').colors.map(&:to_a),
               (img.set_data(2, 1, 24, 4, 'x') rescue $!.message), (img.load_file('{d}/rien.png') rescue $!.message)]
         m2 = Sketchup.active_model.materials.add('Image')
         m2.texture = img
         r + [m2.texture.image_width, m2.texture.filename]"
    )) else {
        return;
    };
    // A file without size: 10" wide, in the image's proportions.
    assert_eq!(
        r,
        "[1, \"damier.png\", 4, 2, 10.0, 5.0, [127, 0, 127, 255], true, 8.0, 4.0, 20.0, 10.0, 24, 24, \
         [255, 0, 0, 255], [0, 0, 255, 255], true, 2, nil, 0, \"Texture filename or nil required\", \
         [[255, 0, 0, 255], [0, 0, 255, 255]], \
         \"width, height, bits per pixel and row padding must equate to the size of the binary string\", \
         \"failed to load image\", 2, \".jpg\"]"
    );
    assert!(dir.join("copie.png").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A texture name comes from the model file: it must never be used as a path (a model could
/// otherwise make Ruby write or delete files anywhere).
#[test]
fn texture_names_never_escape_the_temp_folder() {
    let Some(i) = interp() else { return };
    for evil in ["../../x.png", "..\\..\\x.png"] {
        let tmp = std::env::temp_dir();
        let outside = [tmp.join(evil), tmp.join("..").join("..").join("x.png"), tmp.join("x.png")];
        let existed: Vec<bool> = outside.iter().map(|p| p.exists()).collect();
        // A model whose texture name is a path, as a crafted file could carry it.
        let mut m = Model::new();
        let id = m.add_material("Piège", [1, 2, 3]);
        m.materials[id].ext.texture = Some(esq_core::meta::TextureData {
            filename: evil.into(),
            image: esq_core::photo::Blob(std::sync::Arc::new(png_bytes())),
            px: [4, 2],
            size: [100.0, 50.0],
            avg: [127, 0, 127],
        });
        let mut sel = Vec::new();
        let out = i
            .run(
                "t = Sketchup.active_model.materials[0].texture; r = t.image_rep; [r.width, r.height]",
                "t.rb",
                &mut m,
                &mut sel,
            )
            .unwrap();
        assert!(out.ok, "{}", out.error);
        assert_eq!(out.result, "[4, 2]");
        for (p, e) in outside.iter().zip(&existed) {
            assert_eq!(p.exists(), *e, "fichier créé ou supprimé hors du dossier temporaire : {}", p.display());
        }
        let left: Vec<_> = std::fs::read_dir(&tmp)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("esq_tex_"))
            .collect();
        assert!(left.is_empty(), "fichiers temporaires laissés");

        // Loading a document or a material file keeps only the bare file name.
        let dir = scratch("evil");
        let esq = dir.join("piege.esq");
        esq_core::io::save_esq(&m, &esq).unwrap();
        let back = esq_core::io::load_esq(&esq).unwrap();
        assert_eq!(back.materials.values().next().unwrap().ext.texture.as_ref().unwrap().filename, "x.png");
        let skm = dir.join("piege.skm");
        let doc =
            serde_json::json!({"esquisse_material": 1, "material": serde_json::to_value(&m.materials[id]).unwrap()});
        std::fs::write(&skm, doc.to_string()).unwrap();
        let out = i
            .run(
                &format!("Sketchup.active_model.materials.load('{}').texture.filename", rb_path(&skm)),
                "t.rb",
                &mut Model::new(),
                &mut sel,
            )
            .unwrap();
        assert!(out.ok, "{}", out.error);
        assert_eq!(out.result, "\"x.png\"");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn layers_folders_and_errors() {
    let Some((o, m)) = mm("ls = Sketchup.active_model.layers
         a = ls.add('Murs')
         b = ls.add('Murs')
         c = ls.add('Toit')
         f = ls.add_folder('Bâtiment')
         f.add_layer(a)
         sub = f.add_folder('Étage')
         c.folder = sub
         a.color = 'Red'
         a.page_behavior = LAYER_HIDDEN_BY_DEFAULT | LAYER_IS_HIDDEN_ON_NEW_PAGES
         r = [ls.length, a == b, ls[0].name, ls[0].display_name, ls['Toit'] == c, a.folder == f, c.folder.folder == f,
              ls.count_layers, ls.count_folders, f.count_layers, f.count_folders, a.color.to_a, a.page_behavior,
              a.line_style, ls.unique_name('Murs'), (ls.add('') rescue $!.message), (ls[7] rescue $!.message),
              (ls.remove(ls[0]) rescue $!.message), (ls.remove rescue $!.message),
              err { ls[0].folder = f }, err { a.page_behavior = 0x30 },
              (f.remove_folder(f) rescue $!.message), (f.add_layer(3) rescue $!.message),
              cls { a.folder = 3 }]
         f.visible = false
         r += [a.visible?, c.visible?, f.visible?]
         r << cls { ls.active_layer = nil }
         Sketchup.active_model.active_layer = c
         r << Sketchup.active_model.active_layer.name
         e = Sketchup.active_model.entities.add_face([0,0,0],[1,0,0],[1,1,0])
         ls.remove(c)
         r += [ls.length, c.valid?, Sketchup.active_model.active_layer.name, ls.purge_unused, ls.length]
         r")
    else {
        return;
    };
    assert_eq!(
        o.result,
        "[3, true, \"Layer0\", \"Untagged\", true, true, true, 1, 1, 1, 1, [255, 0, 0, 255], 33, nil, \
         \"Murs1\", \"tag name required\", \"index 7 out of range [0, 3)\", \"cannot remove default tag\", \
         \"must have 1 or 2 arguments\", \"Layer0 (Untagged) cannot appear in a folder\", \
         \"Invalid flags for visibility on new pages\", \"folder is not a child\", \"argument must be a Sketchup::Layer\", \
         TypeError, true, true, false, NoMethodError, \"Toit\", 2, false, \"Layer0\", 1, 1]"
    );
    assert_eq!(m.layers.len(), 1);
}

#[test]
fn selection_counts_and_inverts() {
    let Some(r) = ev("ents = Sketchup.active_model.entities
         f = ents.add_face([0,0,0], [1,0,0], [1,1,0], [0,1,0])
         s = Sketchup.active_model.selection
         n = s.add(f, f.edges)
         r = [n, s.length, s.nitems, s.first == f || s.include?(f), s.add(f), s.remove(f.edges[0]), s.length]
         s.toggle(f)
         r += [s.include?(f), s.single_object?]
         s.clear
         r << s.empty?
         s.add(f)
         r << s.is_surface?
         s.invert
         r += [s.length, s.include?(f), s.is_curve?]
         x = s.shift
         r += [x.is_a?(Sketchup::Edge), s.length, s.model == Sketchup.active_model, s[0].class, s.at(99),
               (s.add(3) rescue $!.class), s.clear]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[5, 5, 5, true, 0, 1, 4, false, false, true, true, 4, false, false, true, 3, true, Sketchup::Edge, nil, \
         ArgumentError, nil]"
    );
}

#[test]
fn operations_abort_and_commit() {
    let Some((o, m)) = mm("mod = Sketchup.active_model
         mod.start_operation('Carré', true)
         mod.entities.add_face([0,0,0], [1,0,0], [1,1,0], [0,1,0])
         mod.abort_operation
         n0 = mod.entities.grep(Sketchup::Face).size
         mod.start_operation('Triangle', true)
         mod.entities.add_face([5,0,0], [6,0,0], [6,1,0])
         mod.commit_operation
         [n0, mod.entities.grep(Sketchup::Face).size, (mod.start_operation rescue $!.message),
          (mod.start_operation(3) rescue $!.message)]")
    else {
        return;
    };
    assert_eq!(o.result, "[0, 1, \"Description required\", \"Description required\"]");
    assert_eq!(m.root.faces.len(), 1);
    // One committed operation = one undo step, named after it.
    let ops = crate::take_ops();
    assert!(
        ops.is_empty() || ops.iter().any(|(n, _)| n == "Triangle"),
        "{:?}",
        ops.iter().map(|o| &o.0).collect::<Vec<_>>()
    );
}

#[test]
fn nested_operation_warns() {
    let Some((o, _)) = mm("mod = Sketchup.active_model
         mod.start_operation('Un')
         mod.start_operation('Deux')
         mod.commit_operation")
    else {
        return;
    };
    assert!(
        o.out.contains("New operation (\"Deux\") started while an existing operation (\"Un\") was still open"),
        "{}",
        o.out
    );
}

#[test]
fn model_properties_and_options() {
    let Some((o, m)) = mm(
        "mod = Sketchup.active_model
         mod.name = 'Maison'
         mod.description = 'Plan'
         u = mod.options['UnitsOptions']
         r = [mod.name, mod.description, mod.guid.size, mod.guid == mod.guid, mod.title, mod.path, mod.modified?,
              err { mod.name = 3 }, mod.options.keys.sort, u['LengthUnit'], u.has_key?('SnapAngle'),
              u.key?('Rien'), u.name, mod.options[0].class, mod.number_faces, mod.valid?, mod.get_product_family,
              Sketchup::Model::VERSION_2021, Sketchup::Model::Make, mod.mipmapping?, mod.georeferenced?]
         u['LengthUnit'] = 4
         u['LengthPrecision'] = 2
         sep = Sketchup::RegionalSettings.decimal_separator
         r += [[Sketchup.format_length(39.37007874015748), Sketchup.format_area(1550.0031), Sketchup.format_angle(Math::PI / 2),
                Sketchup.format_degrees(45), Sketchup.format_area(1000)].map { |s| s.tr(sep, '.') },
               Sketchup.parse_length('2,5m').round(3), Sketchup.parse_length('3\\'').round(1),
               (Sketchup.format_length rescue $!.message), Sketchup.parse_length('abc'),
               [',', '.'].include?(sep), Sketchup::RegionalSettings.list_separator.size]
         r += [mod.entities.class, mod.active_entities.class, mod.active_path, mod.edit_transform.identity?,
               (mod.add_note('x', 2, 0) rescue $!.message), (mod.add_note('x') rescue $!.message),
               (mod.save rescue $!.message), (mod.save_copy('a.skp') rescue $!.message),
               (mod.export('a.xyz') rescue $!.message), mod.export('a.dae'), (mod.find_entity_by_id rescue $!.message),
               (mod.drawing_element_visible?(3) rescue $!.message), (mod.close rescue $!.class),
               (mod.add_diffusion_image('', 0.5) rescue $!.message), err { mod.diffusion_image_opacity = 2 }]
         f = mod.entities.add_face([0,0,0], [10,0,0], [10,10,0], [0,10,0])
         hit = mod.raytest([[2, 2, 10], [0, 0, -1]])
         r += [hit[0].to_a.map { |v| v.round(6) }, hit[1].last == f, mod.raytest([[2, 2, 10], [0, 0, 1]]),
               mod.find_entity_by_id(f.entityID).entityID == f.entityID]
         r",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[\"Maison\", \"Plan\", 36, true, \"\", \"\", false, \"String expected\", \
         [\"NamedOptions\", \"PageOptions\", \"PrintOptions\", \"SlideshowOptions\", \"UnitsOptions\"], 2, true, false, \
         \"UnitsOptions\", Sketchup::OptionsProvider, 0, true, 2, 17, 6, false, false, \
         [\"1.00 m\", \"1 m²\", \"90.0\", \"45.0\", \"0.65 m²\"], 98.425, 36.0, \
         \"Too few arguments: length required\", nil, true, 1, \
         Sketchup::Entities, Sketchup::Entities, nil, true, \"Coordinates must be between 0 and 1\", \
         \"Wrong number of arguments - 3 expected\", \"File name must be specified for unsaved models.\", \
         \"Model must be saved before copying.\", \"Unsupported file extension\", false, \"Must have at least one argument.\", \
         \"Argument must be type of Sketchup::InstancePath or an Array representing an instance path\", nil, \
         \"File path cannot be empty.\", \
         \"Opacity must be between 0.0 and 1.0\", [2.0, 2.0, 0.0], true, nil, true]"
    );
    assert_eq!(m.units, esq_core::model::Units::Meters);
    assert_eq!(m.meta.name, "Maison");
}

#[test]
fn hatch_patterns_and_line_styles() {
    let dir = scratch("hatch");
    std::fs::write(dir.join("brique.pat"), "*BRIQUE\n0, 0,0, 0,.25\n").unwrap();
    let d = rb_path(&dir);
    let Some(r) = ev(&format!(
        "hp = Sketchup.active_model.hatch_patterns
         h = hp.add('Brique', '{d}/brique.pat')
         r = [hp.size, h.name, (hp.add('Brique', '{d}/brique.pat') rescue $!.message), (hp.add('', 'x') rescue $!.message),
              (hp.add('Autre', '{d}/rien.pat') rescue $!.message)]
         h.size = [2, 3]
         r += [h.width, h.height, err {{ h.size = 3 }}, err {{ h.size = [0, 1] }}, h.write('{d}/out.pat')]
         m = Sketchup.active_model.materials.add('Coupe')
         m.hatch_pattern = h
         r += [m.hatch_pattern == h, hp.purge_unused, hp.size]
         ls = Sketchup.active_model.line_styles
         r + [ls.size, ls.names.first, ls['Dot'].name, ls['Rien'], (ls[99] rescue $!.class), ls.to_a.size]"
    )) else {
        return;
    };
    assert_eq!(
        r,
        "[1, \"Brique\", \"Hatch pattern name must be unique.\", \"Hatch pattern name cannot be empty.\", \
         \"Failed to load pattern from path.\", 2.0, 3.0, \"Parameter must be an array\", \
         \"Texture sizes must be positive values\", true, true, false, 1, 12, \"Solid Basic\", \"Dot\", nil, \
         IndexError, 12]"
    );
    assert!(dir.join("out.pat").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sketchup_module_functions() {
    let dir = scratch("defaults");
    let f = rb_path(&dir.join("defaults.json"));
    let Some(r) = ev(&format!(
        "ENV['ESQ_DEFAULTS_FILE'] = '{f}'
         r = [Sketchup.write_default('Esq', 'n', 5), Sketchup.read_default('Esq', 'n'), Sketchup.read_default('Esq', 'x', 7),
              (Sketchup.read_default('Esq') rescue $!.message), Sketchup.write_default('', 'n', 1),
              Sketchup.is_valid_filename?('a.skp'), Sketchup.is_valid_filename?('a:b'), Sketchup.platform,
              Sketchup.display_name_from_action('selectMoveTool:'), Sketchup.break_edges?, (Sketchup.debug_mode = true),
              Sketchup.debug_mode?, Sketchup.create_texture_writer.class, (Sketchup::TextureWriter.new rescue $!.class),
              (Sketchup.register_importer(3) rescue $!.message)]
         ENV.delete('ESQ_DEFAULTS_FILE')
         r"
    )) else {
        return;
    };
    assert_eq!(
        r,
        "[true, 5, 7, \"Too few arguments - at least 2 required\", false, true, false, :platform_win, \"Move\", true, true, \
         true, Sketchup::TextureWriter, NoMethodError, \
         \"An extension must respond to \\\"load_file\\\", \\\"description\\\", and \\\"file_extension\\\"\"]"
    );
    assert!(std::fs::read_to_string(dir.join("defaults.json")).unwrap().contains("\"Esq/n\":5"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn texture_writer_and_uv_helper() {
    let dir = scratch("tw");
    std::fs::write(dir.join("damier.png"), png_bytes()).unwrap();
    let d = rb_path(&dir);
    let Some(r) = ev(&format!(
        "m = Sketchup.active_model.materials.add('T')
         m.texture = ['{d}/damier.png', 10, 10]
         f = Sketchup.active_model.entities.add_face([0,0,0], [10,0,0], [10,10,0], [0,10,0])
         f.material = m
         tw = Sketchup.create_texture_writer
         h = tw.load(f, true)
         uv = Sketchup::UVHelper.new(f, true).get_front_UVQ([5, 5, 0])
         [h, tw.count, tw.handle(f, true), tw.filename(h), tw.write(f, true, '{d}/w.png'), (tw.load(3) rescue $!.message),
          (tw.write(f) rescue $!.message), uv.to_a.map {{ |v| v.round(6) }}, FILE_WRITE_OK]"
    )) else {
        return;
    };
    assert_eq!(
        r,
        "[1, 1, 1, \"damier.png\", 0, \"TextureWriter does not support the loading of the passed object type.\", \
         \"Wrong number of arguments: 2 or 3 required\", [-0.5, 0.5, 1.0], 0]"
    );
    assert!(dir.join("w.png").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn faces_position_textures() {
    let dir = scratch("uv");
    std::fs::write(dir.join("damier.png"), png_bytes()).unwrap();
    let d = rb_path(&dir);
    let Some(r) = ev(&format!(
        "m = Sketchup.active_model.materials.add('T')
         m.texture = ['{d}/damier.png', 10, 10]
         f = Sketchup.active_model.entities.add_face([0,0,0], [0,20,0], [0,20,20], [0,0,20])
         r = [f.texture_positioned?(true), f.position_material(Sketchup.active_model.materials.add('Uni'), [[0,0,0],[0,0,0]], true)]
         f.position_material(m, [[0,0,0], [0,0,0], [0,20,0], [2,0,0]], true)
         uvh = f.get_UVHelper(true, false)
         uv = uvh.get_front_UVQ([0, 10, 5])
         t = f.uv_tile_at([0, 5, 5], true)
         r += [f.material == m, f.texture_positioned?(true), uv.to_a.map {{ |v| v.round(6) }}, t.size,
               t.each_slice(2).map {{ |p, q| uvh.get_front_UVQ(p).to_a.first(2).map {{ |v| v.round(6) }} == q.to_a.first(2) }}.uniq,
               err {{ f.position_material(m, [[0,0,0]], true) }}, err {{ f.position_material(m, [1, 2, 3], true) }}]
         f.clear_texture_position(true)
         r + [f.texture_positioned?(true), f.get_texture_projection(true), err {{ Sketchup::UVHelper.new(3) }}]"
    )) else {
        return;
    };
    assert_eq!(
        r,
        "[false, false, true, true, [1.0, 0.5, 1.0], 8, [true], \"Wrong number of points: must be between 2 and 8\", \
         \"Wrong number of points: must be an even number\", false, nil, \"wrong argument type (expected Sketchup::Face)\"]"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
