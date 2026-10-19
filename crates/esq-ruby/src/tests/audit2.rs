//! Second audit of the Ruby API: what the first one left
//! open, and what it had not looked at.

use super::*;
use esq_core::model::EntityRef;

/// Runs `code` on a fresh model and returns its result, the model and the selection.
fn run_sel(code: &str) -> Option<(String, Model, Vec<EntityRef>)> {
    let i = interp()?;
    let mut m = Model::new();
    let mut sel = Vec::new();
    let out = i.run(code, "test.rb", &mut m, &mut sel).unwrap();
    assert!(out.ok, "erreur Ruby: {}", out.error);
    Some((out.result, m, sel))
}

#[test]
fn snaps_and_spaces_are_selectable() {
    let Some((r, m, sel)) = run_sel(
        r#"m = Sketchup.active_model; ents = m.entities; s = m.selection
         log = []
         obs = Class.new(Sketchup::SelectionObserver) do
           define_method(:onSelectionAdded) { |_sel, e| log << e.typename }
         end.new
         s.add_observer(obs)
         n = ents.add_snap([1, 2, 3], [1, 0, 0])
         p = ents.add_space('Salon', [[0, 0, 0], [100, 0, 0], [100, 50, 0]], [10, 10, 0], 96)
         e = ents.add_line([0, 0, 0], [0, 0, 10])
         r = [s.add(n), s.add(p, e), s.size, s.include?(n), s.include?(p), s.to_a.map(&:typename).sort, log]
         r << s.remove(n) << s.include?(n) << s.size
         s.toggle(n, p)
         r << s.include?(n) << s.include?(p)
         s.clear; s.invert
         r << s.to_a.map(&:typename).sort
         s.clear; s.add(n, p)
         r"#,
    ) else {
        return;
    };
    assert_eq!(
        r,
        "[1, 2, 3, true, true, [\"Edge\", \"Snap\", \"Space\"], [\"Snap\"], 1, false, 2, true, false, \
         [\"Edge\", \"Snap\", \"Space\"]]"
    );
    // The document's selection holds them: the application sees them selected.
    let n = m.root.snaps[0].id;
    let p = m.root.spaces[0].id;
    assert_eq!(sel.len(), 2);
    assert!(sel.contains(&EntityRef::Snap(n)) && sel.contains(&EntityRef::Space(p)), "{sel:?}");
}

#[test]
fn spaces_live_in_their_collection_with_their_tag() {
    let Some((r, m, _)) = run_sel(
        r#"m = Sketchup.active_model; ents = m.entities
         t = m.layers.add('Pièces'); m.active_layer = t
         p = ents.add_space('Cuisine', [[0, 0, 0], [10, 0, 0], [10, 10, 0]], [1, 1, 0], 100)
         p.add_hole([[2, 2, 0], [3, 2, 0], [3, 3, 0]])
         p.name = 'Séjour'
         [p.layer.name, p.name, p.perimeter_points.size, p.ceiling_plane]"#,
    ) else {
        return;
    };
    assert_eq!(r, "[\"Pièces\", \"Séjour\", 3, [0.0, 0.0, 1.0, -100.0]]");
    // In the engine, in millimetres: deleting the tag moves the space to the default one.
    let s = &m.root.spaces[0];
    assert_eq!((s.name.as_str(), s.layer, s.holes.len()), ("Séjour", 1, 1));
    assert!((s.perimeter[1].x - 254.0).abs() < 1e-9 && (s.ceiling[3] + 2540.0).abs() < 1e-9);
    let mut m = m;
    m.remove_layer(1, false);
    assert_eq!(m.root.spaces[0].layer, 0);
    assert!(!m.view_ext.contains_key("spaces"));
}

/// A template shipped with SketchUp (`resources/<langue>/Templates/*.skp`).
fn shipped_template() -> Option<std::path::PathBuf> {
    let dll = esq_skp::find_sketchup_api()?;
    let mut found = None;
    for lang in std::fs::read_dir(dll.parent()?.join("resources")).ok()?.flatten() {
        for f in std::fs::read_dir(lang.path().join("Templates")).into_iter().flatten().flatten() {
            if f.path().extension().is_some_and(|x| x == "skp") {
                found = Some(f.path());
            }
        }
    }
    found
}

fn rb_path(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[test]
fn read_guid_gives_the_guid_saved_in_the_skp() {
    let dir = std::env::temp_dir().join(format!("esq_read_guid_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let text = dir.join("pas_un_modele.skp");
    std::fs::write(&text, "bonjour").unwrap();
    let err = |call: String| format!("({call} rescue \"#{{$!.class}}: #{{$!.message}}\")");
    let code = format!(
        "[{}, {}, {}]",
        err(format!("Sketchup::Skp.read_guid('{}')", rb_path(&dir.join("absent.skp")))),
        err("Sketchup::Skp.read_guid(42)".into()),
        err(format!("Sketchup::Skp.read_guid('{}')", rb_path(&text))),
    );
    let Some((r, _, _)) = run_sel(&code) else { return };
    let invalid = "ArgumentError: invalid model";
    assert_eq!(
        r,
        format!(
            "[\"ArgumentError: file not found\", \"TypeError: no implicit conversion of Integer into String\", \
             \"{invalid}\"]"
        )
    );
    // The GUID of the model, as SketchUp saved it (the model opened from the file has it too),
    // not a fingerprint of the bytes.
    if let Some(t) = shipped_template() {
        let code = format!("g = Sketchup::Skp.read_guid('{0}'); [g, g == Sketchup::Skp.read_guid('{0}')]", rb_path(&t));
        let Some((r, _, _)) = run_sel(&code) else { return };
        let opened = esq_skp::import_skp(&t).unwrap().meta.guid;
        assert!(opened.len() == 36 && opened.matches('-').count() == 4, "{opened}");
        assert_eq!(r, format!("[\"{opened}\", true]"));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn break_edges_decides_whether_new_edges_split_those_they_cross() {
    // One script (the interpreter runs one at a time): the switch is back on at the end.
    let Some((r, m, _)) = run_sel(
        "ents = Sketchup.active_model.entities
         r = [Sketchup.break_edges?]
         begin
           r << Sketchup.send(:break_edges=, nil) << Sketchup.break_edges?
           ents.add_line([0, 0, 0], [10, 0, 0]); ents.add_line([5, -5, 0], [5, 5, 0])
           r << ents.grep(Sketchup::Edge).size
         ensure
           Sketchup.break_edges = true
         end
         ents.add_line([0, 2, 0], [10, 2, 0])
         r << Sketchup.break_edges? << ents.grep(Sketchup::Edge).size",
    ) else {
        return;
    };
    // Off: the cross stays two edges; on: the new line splits the vertical one and itself
    // (1 + 2 + 2).
    assert_eq!(r, "[true, false, false, 2, true, 5]");
    assert_eq!(m.root.edges.len(), 5);
    assert!(esq_core::ops::break_edges());
}

#[test]
fn level_of_detail_settings_answer_the_value_they_replace() {
    let dir = std::env::temp_dir().join(format!("esq_lod_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let f = rb_path(&dir.join("defaults.json"));
    let Some((r, _, _)) = run_sel(&format!(
        "ENV['ESQ_DEFAULTS_FILE'] = '{f}'
         s = Sketchup
         r = [s.use_lod?, s.max_draw_time, s.required_tool_lod, s.required_camera_lod,
              s.full_detail_render_delay_min, s.full_detail_render_delay_max]
         r << s.send(:use_lod=, false) << s.use_lod? << s.send(:max_draw_time=, 0.5) << s.max_draw_time
         r << s.send(:required_tool_lod=, 2) << s.required_tool_lod << (s.send(:required_tool_lod=, 'x') rescue $!.class)
         r << s.read_default('LODPreferences', 'MaxDrawTime')
         s.use_lod = true; s.max_draw_time = 0.1667; s.required_tool_lod = 3
         ENV.delete('ESQ_DEFAULTS_FILE')
         r"
    )) else {
        return;
    };
    assert_eq!(r, "[true, 0.1667, 3, 1, 1.0, 5.0, true, false, 0.1667, 0.5, 3, 2, TypeError, 0.5]");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn observers_that_never_fired_now_do() {
    let dir = std::env::temp_dir().join(format!("esq_obs_plugins_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let Some((r, _, _)) = run_sel(&format!(
        "m = Sketchup.active_model; ents = m.entities
         seen = []
         rec = Class.new do
           define_method(:respond_to_missing?) {{ |n, _ = false| n.to_s.start_with?('on') }}
           define_method(:method_missing) {{ |n, *a| n.to_s.start_with?('on') ? seen << n.to_s : super(n, *a) }}
         end
         ents.add_observer(eo = rec.new); m.layers.add_observer(lo = rec.new); Sketchup.add_observer(app = rec.new)
         # The active section plane of a collection.
         sp = ents.add_section_plane([0, 0, 5], [0, 0, 1])
         sp.activate
         ents.active_section_plane = nil
         # A space is an element of its collection.
         ents.add_space('Salon', [[0, 0, 0], [10, 0, 0], [10, 10, 0]], [1, 1, 0], 96)
         # Tag folders made and removed by Ruby…
         f = m.layers.add_folder('Mobilier'); m.layers.remove_folder(f)
         # Once the plugins of a folder are loaded.
         Esq.load_plugins('{}')
         Sketchup.remove_observer(app); ents.remove_observer(eo); m.layers.remove_observer(lo)
         seen.select {{ |n| n =~ /Section|Folder|Extensions|ElementAdded/ }}",
        dir.to_string_lossy().replace('\\', "/")
    )) else {
        return;
    };
    assert_eq!(
        r,
        "[\"onElementAdded\", \"onActiveSectionPlaneChanged\", \"onActiveSectionPlaneChanged\", \"onElementAdded\", \
         \"onLayerFolderAdded\", \"onLayerFolderRemoved\", \"onExtensionsLoaded\"]"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_tag_folder_made_in_the_interface_reaches_layers_observers() {
    // One script (other tests' scripts share the interpreter): the interface's change is made
    // by the bare native call, which Ruby does not watch, then the application syncs.
    let Some((r, _, _)) = run_sel(
        "out = []
         rec = Class.new do
           define_method(:onLayerFolderAdded) { |_l, f| out << ['+', f.name] }
           define_method(:onLayerFolderRemoved) { |_l, f| out << ['-', f.uid] }
         end
         layers = Sketchup.active_model.layers
         layers.add_observer(o = rec.new)
         Esq::Events.app_sync([])
         raw = ->(op, a) { JSON.parse(Esq.__native(op, JSON.generate(a))) }
         f = raw.('folder.add', 'name' => 'Cuisine')['ok']
         Esq::Events.app_sync(['commit'])
         raw.('folder.remove', 'uid' => f['uid'])
         Esq::Events.app_sync(['commit'])
         layers.remove_observer(o)
         out == [['+', 'Cuisine'], ['-', f['uid']]] ? true : out",
    ) else {
        return;
    };
    assert_eq!(r, "true");
}

#[test]
fn intersect_with_hidden_and_recurse_as_sketchup() {
    let Some((r, _, _)) = run_sel(
        "m = Sketchup.active_model; ents = m.entities
         # The other side: a vertical face across x = 5, in its own group.
         wall = ents.add_group
         wall.entities.add_face([5, -5, -5], [5, 15, -5], [5, 15, 5], [5, -5, 5])
         # This side: a group holding a hidden face, a face on a hidden tag, and a sub-group.
         g = ents.add_group; ge = g.entities
         h = ge.add_face([0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0]); h.hidden = true
         t = m.layers.add('Masqué'); t.visible = false
         l = ge.add_face([0, 0, 2], [10, 0, 2], [10, 10, 2], [0, 10, 2]); l.layer = t
         sub = ge.add_group; sub.entities.add_face([0, 0, 4], [10, 0, 4], [10, 10, 4], [0, 10, 4])
         dest = ents.add_group
         cuts = ->(rec, hid) { ge.intersect_with(rec, Geom::Transformation.new, dest.entities, Geom::Transformation.new, hid, [wall]).size }
         r = [cuts.(false, false), cuts.(false, true), dest.entities.grep(Sketchup::Edge).size]
         dest.entities.clear!
         # Recursing: the sub-group's cut goes into the destination too, not into the sub-group.
         r << cuts.(true, false) << dest.entities.grep(Sketchup::Edge).size << sub.entities.grep(Sketchup::Edge).size
         r",
    ) else {
        return;
    };
    // Not hidden: nothing to cut; hidden: the hidden face and the one on the hidden tag;
    // recurse: the sub-group's face (its 4 own edges untouched).
    assert_eq!(r, "[0, 2, 2, 1, 1, 4]");
}
