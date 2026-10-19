use super::*;

/// Ruby : renvoie « Classe: message » de l'exception levée par le bloc (ou « aucune »).
const ERR: &str = "def err; yield; 'aucune'; rescue Exception => e; \"#{e.class}: #{e.message}\"; end\n";

fn ok(code: &str) -> Option<String> {
    let (o, _) = must(&format!("{ERR}{code}"))?;
    Some(o.result)
}

#[test]
fn camera_defaults_axes_and_set() {
    let Some(r) = ok("c = Sketchup::Camera.new
         [c.eye.to_a, c.target.to_a, c.up.to_a, c.fov, c.fov_is_height?, c.aspect_ratio, c.perspective?,
          c.height.round(3), c.image_width, c.is_2d?, c.scale_2d, c.description]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[[0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 30.0, true, 0.0, true, 1.0, 0.0, false, 1.0, \"\"]"
    );

    let Some(r) = ok("c = Sketchup::Camera.new([10, 0, 0], [0, 0, 0], [0, 0, 1])
         [c.direction.to_a, c.xaxis.to_a.map { |v| v.round(6) }, c.yaxis.to_a.map { |v| v.round(6) }, c.zaxis.to_a,
          c.set([1, 2, 3], [1, 2, 0], [0, 1, 0]).equal?(c), c.eye.to_a]")
    else {
        return;
    };
    assert_eq!(r, "[[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], true, [1.0, 2.0, 3.0]]");
}

#[test]
fn camera_errors_and_warnings() {
    let Some((o, _)) = must(&format!(
        "{ERR}c = Sketchup::Camera.new
         r = [err {{ c.set([0,0,0], [0,0,0]) }},
              err {{ c.set([0,0,0], [1,1,1], [1,1,1]) }},
              err {{ c.set([0,0,0], [1,1,1], [0,0,0]) }},
              err {{ c.set([0,0,0], [0,0,0], [0,0,1]) }},
              err {{ Sketchup::Camera.deserialize('pas du json') }},
              err {{ Sketchup.active_model.active_view.camera = 3 }},
              err {{ Sketchup.active_model.active_view.camera = [c] }}]
         c.perspective = false
         [c.fov, c.focal_length, c.image_width]
         c.fov = 20; c.focal_length = 50; c.image_width = 10
         c.perspective = true
         c.fov = 500
         c.focal_length = 0.1
         c.image_width = -5
         c.height
         c.height = 1
         [r, c.fov, c.focal_length.round(3), c.image_width]"
    )) else {
        return;
    };
    assert!(o.result.contains("ArgumentError: Eye, Target and Up are required"), "{}", o.result);
    assert!(o.result.contains("ArgumentError: Eye and target points must be different"), "{}", o.result);
    assert!(o.result.contains("ArgumentError: Up vector cannot be parallel to view direction"), "{}", o.result);
    assert!(o.result.contains("RuntimeError: Failed to deserialize camera from string."), "{}", o.result);
    assert!(o.result.contains("ArgumentError: Invalid Camera"), "{}", o.result);
    assert!(o.result.contains("ArgumentError: Wrong number of arguments: 2 expected"), "{}", o.result);
    for w in [
        "fov is only valid for perspective cameras",
        "focal_length is only valid for perspective cameras",
        "image_width is only valid for perspective cameras",
        "fov= is only valid for perspective views",
        "focal_length= is only valid for perspective views",
        "image_width= is only valid for perspective views",
        "fov must be between 1 and 120 degrees",
        "focal_length must be between 1 and 3000 mm",
        "image_width must be greater than 0.0 mm",
        "height is not valid for perspective views",
        "height= is not valid for perspective views",
    ] {
        assert!(o.out.contains(&format!("warning: {w}")), "{w} manque dans {:?}", o.out);
    }
    // focal_length borné à 1 mm → fov = 2·atan(18 / 1) ; image_width négatif → 0 (36 mm par défaut pour le calcul).
    assert!(o.result.ends_with(", 173.64033976027153, 1.0, 0.0]"), "{}", o.result);
}

#[test]
fn camera_focal_length_and_aspect() {
    let Some(r) = ok("c = Sketchup::Camera.new
         c.image_width = 36
         c.focal_length = 18
         f1 = c.fov.round(4)
         c.fov = 90
         f2 = c.focal_length.round(4)
         c.aspect_ratio = 1.5
         a = [c.fov_is_height?, c.aspect_ratio]
         c.aspect_ratio = -2
         [f1, f2, a, c.fov_is_height?, c.aspect_ratio]")
    else {
        return;
    };
    assert_eq!(r, "[90.0, 18.0, [false, 1.5], true, 0.0]");
}

#[test]
fn camera_perspective_switch_keeps_the_image() {
    let Some(r) = ok("c = Sketchup::Camera.new([0, -100, 0], [0, 0, 0], [0, 0, 1])
         c.fov = 60
         c.perspective = false
         h = c.height.round(4)
         c.perspective = true
         [h, (2 * 100 * Math.tan(30.degrees)).round(4), c.eye.to_a.map { |v| v.round(4) }, c.perspective?]")
    else {
        return;
    };
    assert_eq!(r, "[115.4701, 115.4701, [0.0, -100.0, 0.0], true]");
}

#[test]
fn camera_serialization_round_trips() {
    let Some(r) = ok("c = Sketchup::Camera.new([1, 2, 3], [4, 5, 6], [0, 0, 1], false, 7)
         c.description = 'vue'
         d = Sketchup::Camera.deserialize(c.serialize)
         [d.eye.to_a, d.target.to_a, d.perspective?, d.height, d.description, d == c]")
    else {
        return;
    };
    assert_eq!(r, "[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], false, 7.0, \"vue\", true]");
}

#[test]
fn global_constants_match_sketchup() {
    let Some(r) = ok(
        "[GL_POINTS, GL_LINES, GL_LINE_LOOP, GL_LINE_STRIP, GL_TRIANGLES, GL_TRIANGLE_STRIP, GL_TRIANGLE_FAN, GL_QUADS,
          GL_QUAD_STRIP, GL_POLYGON, MK_LBUTTON, MK_RBUTTON, MK_SHIFT, MK_CONTROL, MK_MBUTTON, MK_ALT, MK_COMMAND,
          PAGE_USE_CAMERA, PAGE_USE_RENDERING_OPTIONS, PAGE_USE_SHADOWINFO, PAGE_USE_SKETCHCS, PAGE_USE_HIDDEN,
          PAGE_USE_HIDDEN_GEOMETRY, PAGE_USE_HIDDEN_OBJECTS, PAGE_USE_LAYER_VISIBILITY, PAGE_USE_SECTION_PLANES,
          PAGE_USE_ALL, PAGE_NO_CAMERA, PAGE_USE_ENVIRONMENT,
          Sketchup::View::CORNER_TOP_LEFT, Sketchup::View::CORNER_BOTTOM_RIGHT, Sketchup::PickHelper::PICK_INSIDE,
          Sketchup::PickHelper::PICK_CROSSING, Sketchup::Pages::UnitsPixels, Sketchup::Pages::UnitsNormalizedX,
          Sketchup::Pages::ImageLinked, Sketchup::RenderingOptions::ROPSetRenderMode,
          Sketchup::RenderingOptions::ROPSetBackgroundColor, Sketchup::RenderingOptions::ROPSetFaceColorMode]",
    ) else {
        return;
    };
    assert_eq!(
        r,
        "[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 1, 2, 4, 8, 16, 32, 0, 1, 2, 4, 8, 16, 128, 256, 32, 64, 4095, 4094, 512, 0, 3, 2, 3, 0, 2, 2, 1, 12, 9999]"
    );
}

#[test]
fn pages_are_scenes_with_unique_names_and_stable_identity() {
    let Some((o, m)) = must(&format!(
        "{ERR}pages = Sketchup.active_model.pages
         a = pages.add('Façade')
         b = pages.add('Façade')
         c = pages.add
         d = pages.add('Coupe', 0, 0)
         names = pages.map(&:name)
         pages.reorder(a, 3)
         after = pages.map(&:name)
         pages.reorder(d, -1)
         x = [pages.size, pages.length, names, after, pages.map(&:name),
              pages['Coupe'].equal?(d), pages['Coupe'] == d, pages[0] == pages['Façade'], pages[99], pages[-1].name]
         x << err {{ pages.reorder(a, 9) }}
         x << err {{ pages.reorder(a, -9) }}
         x << pages.erase(b)
         x << pages.erase(b)
         x << b.valid? << a.valid? << pages.size
         x << err {{ b.name }}
         x << a.parent.equal?(pages) << a.typename << a.entityID.class
         x"
    )) else {
        return;
    };
    assert!(
        o.result.starts_with("[4, 4, [\"Coupe\", \"Façade\", \"Façade#1\", \"Scène 1\"], [\"Coupe\", \"Façade#1\", \"Scène 1\", \"Façade\"]"),
        "{}",
        o.result
    );
    assert!(o.result.contains("IndexError: Invalid index for reordering scene."), "{}", o.result);
    assert!(o.result.contains("true, false, false, true, 3"), "{}", o.result);
    assert_eq!(m.scenes.len(), 3);
    assert!(m.scenes.iter().all(|s| s.ext.uid != 0));
}

#[test]
fn page_properties_and_flags() {
    let Some(r) = ok("pages = Sketchup.active_model.pages
         p = pages.add('P', PAGE_USE_CAMERA | PAGE_USE_SHADOWINFO)
         before = [p.use_camera?, p.use_rendering_options?, p.use_shadow_info?, p.use_axes?, p.use_hidden?,
                   p.use_hidden_layers?, p.use_section_planes?, p.use_style?, p.use_environment?]
         p.use_rendering_options = true
         p.use_hidden_layers = true
         p.use_camera = false
         p.description = 'une scène'
         p.transition_time = -4
         p.delay_time = 2.5
         p.include_in_animation = false
         [before, p.use_camera?, p.use_rendering_options?, p.use_hidden_layers?, p.layers, p.layer_folders, p.hidden_entities,
          p.description, p.transition_time, p.delay_time, p.include_in_animation?, p.label, p.name = 'Autre', p.name]") else {
        return;
    };
    assert_eq!(
        r,
        "[[true, false, true, false, false, false, false, false, false], false, true, true, [], [], nil, \"une scène\", -1.0, 2.5, false, \"P\", \"Autre\", \"Autre\"]"
    );
}

#[test]
fn page_remembers_layers_and_visibility() {
    let Some((o, _)) = must(&format!(
        "{ERR}m = Sketchup.active_model
         m.layers.add('Murs')
         m.layers.add('Toit')
         p = m.pages.add('A', PAGE_USE_ALL)
         murs = m.layers['Murs']
         p.set_visibility(murs, false)
         e = [p.layers.map(&:name), p.use_hidden_layers?]
         p.set_visibility(murs, true)
         e << p.layers.map(&:name)
         e << err {{ p.set_visibility('Murs', false) }}
         e << err {{ p.set_drawingelement_visibility(3, true) }}
         e << err {{ p.get_drawingelement_visibility(3) }}
         e"
    )) else {
        return;
    };
    assert!(o.result.starts_with("[[\"Murs\"], true, []"), "{}", o.result);
    assert!(o.result.contains("TypeError: expected Sketchup::Layer or Sketchup::LayerFolder"), "{}", o.result);
    assert!(o.result.contains("TypeError: Not a Sketchup::Drawingelement."), "{}", o.result);
}

#[test]
fn pages_slideshow_times() {
    let Some(r) = ok("pages = Sketchup.active_model.pages
         a = pages.add('A'); b = pages.add('B'); c = pages.add('C')
         a.delay_time = 2; a.transition_time = 5
         b.delay_time = 3; b.transition_time = 4
         c.delay_time = 1; c.transition_time = 2
         c.include_in_animation = false
         t = pages.slideshow_time
         [t, pages.show_frame_at(1.0).map { |x| x.is_a?(Float) ? x : x.name }, pages.show_frame_at(4.0).map { |x| x.is_a?(Float) ? x.round(3) : x.name },
          pages.show_frame_at(7.0).map { |x| x.is_a?(Float) ? x : x.name }, pages.show_frame_at(99)]") else {
        return;
    };
    assert_eq!(r, "[9.0, [\"A\", 0.0], [\"A\", 0.5], [\"B\", 0.0], nil]");
}

#[test]
fn pages_add_matchphoto_page_validates_arguments() {
    let Some(r) = ok("pages = Sketchup.active_model.pages
         [err { pages.add_matchphoto_page }, err { pages.add_matchphoto_page(nil) }, err { pages.add_matchphoto_page('') },
          err { pages.add_matchphoto_page(*(['x'] * 10)) }, pages.add_matchphoto_page('photo.jpg', nil, 'Photo').name]") else {
        return;
    };
    assert!(r.contains("ArgumentError: Invalid number of arguments: 1 to 9 expected"), "{r}");
    assert!(r.contains("ArgumentError: Null image name encountered"), "{r}");
    assert!(r.contains("ArgumentError: Image name is empty"), "{r}");
    assert!(r.ends_with("\"Photo\"]"), "{r}");
}

#[test]
fn page_frame_observers_get_ids() {
    let Some(r) = ok("o = Object.new
         i = Sketchup::Pages.add_frame_change_observer(o)
         [i.class, Sketchup::Pages.remove_frame_change_observer(i), Sketchup::Pages.remove_frame_change_observer(i),
          Sketchup::Pages.remove_frame_change_observer(-1)]")
    else {
        return;
    };
    assert_eq!(r, "[Integer, true, false, false]");
}

#[test]
fn shadow_info_keys_values_and_setters() {
    let Some(r) = ok("s = Sketchup.active_model.shadow_info
         t = s['ShadowTime']
         k = s.keys
         s['DisplayShadows'] = true
         s['Light'] = 70
         s['City'] = 'Lyon'
         s['Latitude'] = 45.75
         s['DayOfYear'] = 5
         s['Inconnue'] = 3
         s['TZOffset'] = 1
         [k.size, k == k.sort, s.size, t.class, t.utc?, t.year, s['DisplayShadows'], s['Light'], s['City'], s['Latitude'],
          s['DayOfYear'], s['Inconnue'], s['TZOffset'], s['SunDirection'].class, s['SunRise'] < s['SunSet'],
          s['ShadowTime_time_t'].class, Sketchup::ShadowInfo.keys == k, s.each_key.to_a == k, s.each_pair.to_a.size]") else {
        return;
    };
    assert_eq!(
        r,
        "[23, true, 23, Time, true, 2013, true, 70, \"Lyon\", 45.75, 312, nil, 1.0, Geom::Vector3d, true, Integer, true, true, 23]"
    );
}

#[test]
fn shadow_time_is_utc_and_follows_the_time_zone() {
    let Some((o, m)) = must(
        "s = Sketchup.active_model.shadow_info
         s['ShadowTime'] = Time.utc(2020, 6, 21, 10, 30)
         [s['ShadowTime'].to_a.first(6).reverse, s['TZOffset']]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[[2020, 6, 21, 10, 30, 0], -7.0]");
    // 10:30 UTC avec un fuseau de -7 h : 03:30 à l'heure locale du modèle.
    assert_eq!(m.shadow.local().3, 3.0 * 60.0 + 30.0);
}

#[test]
fn rendering_options_defaults_types_and_errors() {
    let Some((o, _)) = must(&format!(
        "{ERR}ro = Sketchup.active_model.rendering_options
         r = [ro.keys.size, ro.keys == ro.keys.sort, ro.size, ro['Nope'], ro['BackgroundColor'].class,
              ro['DepthQueWidth'].class, ro['FogEndDist'].class, ro['Texture']]
         ro['BackgroundColor'] = Sketchup::Color.new(10, 20, 30)
         ro['EdgeColorMode'] = 2
         ro['DisplayFog'] = true
         ro['FogStartDist'] = 12
         r << ro['BackgroundColor'].to_a << ro['EdgeColorMode'] << ro['DisplayFog'] << ro['FogStartDist']
         r << err {{ ro['Nope'] = 1 }}
         r << err {{ ro['DisplayFog'] = 'oui' }}
         r << err {{ ro['BackgroundColor'] = 12.5 }}
         r << (ro['BackgroundColor'] = 'red').class << ro['BackgroundColor'].to_a
         r << ro.each_pair.to_a.size << ro.each_key.first << Sketchup::RenderingOptions.keys.last
         r << ro.hatch_pattern
         r"
    )) else {
        return;
    };
    assert!(
        o.result
            .starts_with("[70, true, 70, nil, Sketchup::Color, Integer, Float, true, [10, 20, 30, 255], 2, true, 12.0"),
        "{}",
        o.result
    );
    assert!(
        o.result.matches("ArgumentError: Rendering option could not be set to the given value").count() == 3,
        "{}",
        o.result
    );
    assert!(o.result.ends_with("\"TransparencySort\", nil]"), "{}", o.result);
}

#[test]
fn rendering_options_notify_observers_with_rop_ids() {
    let Some(r) = ok("ro = Sketchup.active_model.rendering_options
         seen = []
         obs = Object.new
         obs.define_singleton_method(:onRenderingOptionsChanged) { |_o, t| seen << t }
         a = ro.add_observer(obs)
         b = ro.add_observer(obs)
         ro['BackgroundColor'] = 'blue'
         ro['RenderMode'] = 1
         ro['DrawGround'] = true
         r = [a, b, seen, seen == [Sketchup::RenderingOptions::ROPSetBackgroundColor, Sketchup::RenderingOptions::ROPSetRenderMode, Sketchup::RenderingOptions::ROPSetDrawGround]]
         [r, ro.remove_observer(obs), ro.remove_observer(obs)]") else {
        return;
    };
    assert_eq!(r, "[[true, false, [12, 1, 56], true], true, false]");
}

#[test]
fn styles_default_duplicate_select_and_remove() {
    let Some((o, _)) = must(&format!(
        "{ERR}m = Sketchup.active_model
         ro = m.rendering_options
         st = m.styles
         base = st.selected_style
         r = [st.size, st.length, base.name, base.path, st.active_style == base, st.active_style_changed, st[0] == base, st[base.name] == base, st[5], st['x']]
         copy = base.duplicate
         r << st.size << copy.name << (copy == base)
         ro['BackgroundColor'] = 'red'
         r << st.active_style_changed << (st.active_style == base) << st.active_style.name
         r << err {{ st.selected_style = st.active_style }}
         st.update_selected_style
         r << st.active_style_changed
         ro['BackgroundColor'] = 'blue'
         st.selected_style = copy
         r << (st.selected_style == copy) << ro['BackgroundColor'].to_a
         copy.name = 'Autre'; copy.description = 'd'
         r << st['Autre'].description
         r << err {{ st.remove_style(base); st.remove_style(base) }}
         r << st.size << err {{ st.remove_style(copy) }}
         r << st.map(&:name) << st.add_style('/n/existe/pas.style') << st.purge_unused
         r"
    )) else {
        return;
    };
    assert!(
        o.result.starts_with("[1, 1, \"Style par défaut\", \"\", true, false, true, true, nil, nil, 2"),
        "{}",
        o.result
    );
    assert!(o.result.contains("ArgumentError: Can not set the selected style to the active style."), "{}", o.result);
    assert!(o.result.contains("ArgumentError: Style not found in the Styles collection."), "{}", o.result);
    assert!(o.result.contains("ArgumentError: The Styles collection must contain at least one style."), "{}", o.result);
}

#[test]
fn environments_manage_hdr_images() {
    let dir = std::env::temp_dir().join(format!("esq-env-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let hdr = dir.join("ciel.hdr");
    std::fs::write(&hdr, b"#?RADIANCE\n").unwrap();
    let path = hdr.to_string_lossy().replace('\\', "/");
    let Some((o, _)) = must(&format!(
        "{ERR}es = Sketchup.active_model.environments
         r = [es.size, es.current]
         e = es.add({path:?})
         f = es.add('Soir', {path:?})
         r << e.name << f.name << es.size << es.length << es.map(&:name) << (es['Soir'] == f) << es['x']
         r << err {{ es.add }} << err {{ es.add('Soir', {path:?}) }} << err {{ es.add('', {path:?}) }}
         r << err {{ es.add('Z', '/n/existe/pas.hdr') }}
         r << err {{ f.name = '' }} << err {{ f.name = 'ciel' }}
         f.name = 'Nuit'
         r << f.name << f.use_as_skydome? << f.use_for_reflections? << f.rotation << f.skydome_exposure << f.reflection_exposure << f.linked_sun?
         f.rotation = 90; f.skydome_exposure = 2.5; f.reflection_exposure = 0.5; f.linked_sun = true
         f.use_as_skydome = false; f.use_for_reflections = false; f.description = 'x'
         r << [f.rotation, f.skydome_exposure, f.reflection_exposure, f.linked_sun?, f.use_as_skydome?, f.use_for_reflections?, f.description]
         r << err {{ f.rotation = 400 }} << err {{ f.skydome_exposure = 21 }} << err {{ f.reflection_exposure = 11 }}
         r << err {{ f.linked_sun_position = [400, 0] }}
         f.linked_sun_position = [10, 20]
         r << f.linked_sun_position.to_a
         r << err {{ es.current = 3 }} << (es.current = f).name << es.current.name
         r << err {{ f.write_hdr('') }}
         out = {out:?}
         r << (f.write_hdr(out) == out) << File.exist?(out)
         r << es.remove(e) << es.remove(e) << es.size << es.purge_unused.class << es.size << es.current.nil?
         r << err {{ es.remove(3) }} << es.add_observer(Object.new) << f.valid? << f.path.end_with?('ciel.hdr')
         r",
        out = dir.join("copie.hdr").to_string_lossy().replace('\\', "/"),
    )) else {
        return;
    };
    for e in [
        "ArgumentError: Wrong number of arguments: requires 1 or 2",
        "ArgumentError: Environment name must be unique.",
        "ArgumentError: Environment name cannot be empty.",
        "ArgumentError: Failed to load image from path.",
        "RuntimeError: Environment name must be unique.",
        "RangeError: Rotation is out of range.",
        "RangeError: Skydome exposure is out of range.",
        "RangeError: Reflection exposure is out of range.",
        "RangeError: Linked sun position values are out of range.",
        "TypeError: Expected an Environment object.",
        "ArgumentError: The file name is empty.",
    ] {
        assert!(o.result.contains(e), "{e} manque dans {}", o.result);
    }
    assert!(o.result.starts_with("[0, nil, \"ciel\", \"Soir\", 2, 2, [\"ciel\", \"Soir\"], true, nil"), "{}", o.result);
    assert!(o.result.contains("[90.0, 2.5, 0.5, true, false, false, \"x\"]"), "{}", o.result);
    assert!(o.result.contains("[10.0, 20.0]"), "{}", o.result);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn snap_space_hlr_and_undefined_allocators() {
    let Some(r) = ok("ents = Sketchup.active_model.entities
         sp = ents.add_space('Salon', [[0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0]], [0, 0, 0], 0)
         e = [sp.name, sp.perimeter_points.size, sp.orientation_point.to_a, sp.ceiling_plane.size, sp.valid?]
         sp.name = 'Cuisine'
         sp.orientation_point = [1, 1, 0]
         sp.ceiling_plane = [0, 0, 1, -96]
         e << sp.name << sp.orientation_point.to_a << sp.ceiling_plane << sp.add_hole([[1, 1, 0], [2, 1, 0], [2, 2, 0]])
         e << err { sp.add_hole([[1, 1, 0]]) } << err { sp.perimeter_points = [[0, 0, 0]] } << err { sp.ceiling_plane = [0, 0, 0, 1] }
         sp.perimeter_points = [[0, 0, 0], [5, 0, 0], [5, 5, 0]]
         e << sp.perimeter_points.size
         h = Sketchup::HLR.new
         e << h.num_faces << h.calculate << h.cleanup << h.num_faces
         e << err { Sketchup::View.new } << err { Sketchup::Page.new } << err { Sketchup::Snap.new }
         s = ents.add_snap([0, 0, 0], [1, 0, 0], [0, 0, 1])
         s.set([1, 2, 3], [0, 1, 0])
         e << s.position.to_a << s.direction.to_a << s.up.to_a << err { s.set([0, 0, 0], [0, 0, 1], [0, 0, 2]) }
         e") else {
        return;
    };
    assert!(
        r.starts_with("[\"Salon\", 4, [0.0, 0.0, 0.0], 4, true, \"Cuisine\", [1.0, 1.0, 0.0], [0.0, 0.0, 1.0, -96.0]"),
        "{r}"
    );
    assert!(r.contains("ArgumentError: Hole must have at least 3 points."), "{r}");
    assert!(r.contains("ArgumentError: Perimeter points must have at least 3 points."), "{r}");
    assert!(r.contains("ArgumentError: Could not set ceiling plane"), "{r}");
    assert!(r.contains("TypeError: allocator undefined for Sketchup::View"), "{r}");
    assert!(r.contains("ArgumentError: direction and up can't be parallel"), "{r}");
    assert!(r.contains("[1.0, 2.0, 3.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]"), "{r}");
}

#[test]
fn view_argument_validation_without_an_application() {
    let Some(r) = ok("v = Sketchup.active_model.active_view
         [err { v.corner(4) }, err { v.corner(-1) }, v.corner(3), v.center, v.vpwidth.class, v.device_width.class,
          err { v.pickray(1) }, err { v.inputpoint(1) }, err { v.draw_lines([1, 1, 1]) }, err { v.draw_polyline([0, 0, 0]) },
          err { v.line_width = -1 }, err { v.tooltip = 3 }, err { v.show_frame(1, 2) },
          err { v.write_image }, err { v.write_image(1, 2, 3, 4, 5, 6) }, err { v.write_image('a.png', 0, 5) },
          err { v.write_image('a.png', 17000, 5) }, err { v.write_image(filename: 'a.png', scale_factor: 100) },
          err { v.write_image(width: 10, height: 10) }, v.write_image('x.gif', 10, 10),
          v.zoom(0.0), v.zoom_extents.equal?(v), v.invalidate.equal?(v), v.refresh.equal?(v), v.model == Sketchup.active_model,
          v.graphics_engine, v.add_observer(3), v.add_observer(3), v.remove_observer(3), v.remove_observer(3),
          err { v.load_texture(3) }, v.release_texture(5), v.lock_inference, v.inference_locked?, v.tooltip = nil]") else {
        return;
    };
    for e in [
        "IndexError: index out of range [0,3]",
        "ArgumentError: Wrong number of arguments: x and y required",
        "ArgumentError: Wrong number of points: at least two required",
        "ArgumentError: Line width must be positive",
        "ArgumentError: String expected",
        "ArgumentError: Wrong number of arguments: 0 or 1",
        "ArgumentError: Too few arguments. Filename or option hash required",
        "ArgumentError: Too many arguments.",
        "ArgumentError: Invalid width or height value. Valid range is 1 to 16000",
        "ArgumentError: :scale_factor must be between",
        "ArgumentError: Filename not specified.",
        // TypeError for what is not an ImageRep.
        "TypeError: wrong argument type",
    ] {
        assert!(r.contains(e), "{e} manque dans {r}");
    }
    assert!(r.contains("[1280.0, 800.0]"), "{r}");
    assert!(r.contains(":graphics_engine_2024, true, false, true, false"), "{r}");
}

#[test]
fn input_point_without_a_view_is_invalid() {
    let Some(r) = ok("ip = Sketchup::InputPoint.new
         [ip.valid?, ip.position.to_a, ip.vertex, ip.edge, ip.face, ip.depth, ip.tooltip, ip.degrees_of_freedom, ip.display?,
          ip.draw(nil), ip.instance_path, ip.transformation, ip == Sketchup::InputPoint.new, ip == 3,
          err { ip.pick }, err { ip.pick(3, 1, 2) }, err { ip.copy!(3) }, ip.clear.equal?(ip),
          Sketchup::InputPoint.new(Geom::Point3d.new(1, 2, 3)).position.to_a, Sketchup::InputPoint.new(Geom::Point3d.new(1, 2, 3)).valid?]") else {
        return;
    };
    assert!(
        r.starts_with("[false, [0.0, 0.0, 0.0], nil, nil, nil, nil, \"\", 0, false, nil, nil, nil, true, false"),
        "{r}"
    );
    assert!(r.contains("ArgumentError: Too few arguments: view, x and y required"), "{r}");
    assert!(r.contains("ArgumentError: Invalid view in InputPoint::pick"), "{r}");
    assert!(r.contains("TypeError: wrong argument type (expected Sketchup::InputPoint)"), "{r}");
    assert!(r.ends_with("[1.0, 2.0, 3.0], true]"), "{r}");
}

#[test]
fn pick_helper_validation() {
    let Some(r) = ok("v = Sketchup.active_model.active_view
         ph = v.pick_helper
         [ph.view.equal?(v), ph.count, ph.best_picked, ph.all_picked, ph.element_at(0), ph.path_at(0), ph.depth_at(0),
          ph.picked_edge, ph.picked_face, err { ph.init(1) }, err { ph.do_pick(1) }, err { ph.test_point }, err { ph.test_point([0, 0, 0], 1) },
          err { ph.pick_segment }, err { ph.pick_segment([[0, 0, 0]], 1, 2) }, err { ph.pick_segment([[0, 0, 0], [1, 1, 1]], 1) },
          err { ph.window_pick([0, 0], [5, 5], 9) }, err { ph.boundingbox_pick(Geom::BoundingBox.new, 9) },
          err { Sketchup::PickHelper.new }]") else {
        return;
    };
    assert!(r.starts_with("[true, 0, nil, [], nil, nil, nil, nil, nil"), "{r}");
    for e in [
        "ArgumentError: Wrong number of arguments: x and y required",
        "ArgumentError: Wrong number of arguments: point required",
        "ArgumentError: Wrong number of arguments: point, x and y required",
        "ArgumentError: Wrong number of arguments: Array of Points required",
        "ArgumentError: At least two Points are needed.",
        "ArgumentError: Both x and y are needed.",
        "ArgumentError: Invalid pick option. Use Sketchup::PickHelper::PICK_INSIDE or Sketchup::PickHelper::PICK_CROSSING.",
    ] {
        assert!(r.contains(e), "{e} manque dans {r}");
    }
}
