//! Second audit of the Ruby API, application side.

use eframe::egui::Modifiers;
use esq_core::glam::{DVec2, DVec3};
use esq_core::model::{EntityRef, Model, SnapPoint, Space};

use super::Harness;
use crate::tools::ToolId;

/// A box, a snap in front of it and a room beside it (ceiling 300 mm up).
fn box_snap_space(h: &mut Harness) -> (u32, u32) {
    h.rect_and_box();
    let (mut n, mut p) = (0, 0);
    h.doc.commit("Repères", |m, d| {
        n = m.add_snap(d, SnapPoint::new(DVec3::new(500.0, -200.0, 200.0), DVec3::NEG_Y, DVec3::Z));
        let room = vec![
            DVec3::new(1500.0, 0.0, 0.0),
            DVec3::new(2500.0, 0.0, 0.0),
            DVec3::new(2500.0, 600.0, 0.0),
            DVec3::new(1500.0, 600.0, 0.0),
        ];
        p = m.add_space(d, Space::new("Salon", room, DVec3::new(1600.0, 100.0, 0.0), 300.0));
    });
    (n, p)
}

#[test]
fn the_select_tool_picks_snaps_and_spaces() {
    let mut h = Harness::new();
    let (n, p) = box_snap_space(&mut h);
    h.tool(ToolId::Select);
    // A click on the snap's point selects it.
    h.click(DVec3::new(500.0, -200.0, 200.0));
    assert_eq!(h.doc.sel, vec![EntityRef::Snap(n)]);
    // Ctrl + click on the room's ceiling outline adds the space.
    h.mods = Modifiers::CTRL;
    h.click(DVec3::new(2000.0, 0.0, 300.0));
    h.mods = Modifiers::NONE;
    assert_eq!(h.doc.sel, vec![EntityRef::Snap(n), EntityRef::Space(p)]);
    // Entity Info names a mix as SketchUp does (« %d Entités »).
    assert_eq!(crate::entity_info::title(&h.doc, &h.doc.sel, &|_| false), "2 Entités");
    // A window around the room selects the space alone.
    let corners = [DVec3::new(1450.0, -50.0, -10.0), DVec3::new(2550.0, 650.0, 310.0)];
    let mut min = DVec2::splat(f64::MAX);
    let mut max = DVec2::splat(f64::MIN);
    for x in [corners[0].x, corners[1].x] {
        for y in [corners[0].y, corners[1].y] {
            for z in [corners[0].z, corners[1].z] {
                let s = h.px(DVec3::new(x, y, z));
                (min, max) = (min.min(s), max.max(s));
            }
        }
    }
    h.with(|t, cx| {
        t.mouse_move(cx, min);
        t.press(cx, min);
        t.mouse_move(cx, max);
        t.release(cx, max);
    });
    assert_eq!(h.doc.sel, vec![EntityRef::Space(p)]);
    // A double-click keeps it selected (nothing to open).
    let m = h.px(DVec3::new(2500.0, 300.0, 300.0));
    h.with(|t, cx| t.double_click(cx, m));
    assert_eq!(h.doc.sel, vec![EntityRef::Space(p)]);
    assert_eq!(crate::entity_info::title(&h.doc, &[EntityRef::Snap(1), EntityRef::Snap(2)], &|_| false), "2 ancrages");
}

#[test]
fn sketchup_template_is_what_file_new_starts_from() {
    use super::ruby_model::{host, run};
    let _l = crate::prefs::TEMPLATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("esq_template_rb_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let src = dir.join("Atelier.esq");
    let mut m = Model { units: esq_core::model::Units::Feet, ..Default::default() };
    m.meta.name = "Atelier".into();
    esq_core::io::save_esq(&m, &src).unwrap();
    let path = src.to_string_lossy().replace('\\', "/");
    let mut h = host();
    let mut doc = crate::doc::Document::new(Model::new());
    let Some(o) = run(
        &mut h,
        &mut doc,
        &format!(
            "r = [Sketchup.send(:template=, '{path}'), File.basename(Sketchup.template),
                  Sketchup.send(:template=, '{path}.absent'), File.basename(Sketchup.template)]
             Sketchup.file_new
             r"
        ),
    ) else {
        return;
    };
    assert_eq!(o.result, "[true, \"Atelier.esq\", false, \"Atelier.esq\"]");
    // File › New, by Ruby or from the menu, starts from it.
    assert_eq!((doc.model.units, doc.model.meta.name.as_str()), (esq_core::model::Units::Feet, "Atelier"));
    let mut app = crate::app::EsquisseApp::bare(None, None);
    app.new_document();
    assert_eq!(app.doc.model.meta.name, "Atelier");
    // An empty path brings the default template back: a built-in one, whose file sits in
    // template_dir; giving that file back selects the built-in.
    let Some(o) = run(
        &mut h,
        &mut doc,
        "r = [Sketchup.send(:template=, ''), Sketchup.template.start_with?(Sketchup.template_dir),
              File.exist?(Sketchup.template)]
         r << Sketchup.send(:template=, Sketchup.template)
         r",
    ) else {
        return;
    };
    assert_eq!(o.result, "[true, true, true, true]");
    assert_eq!(crate::prefs::default_template(), crate::prefs::builtin_templates()[0]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn break_edges_is_saved_with_the_preferences() {
    use super::ruby_model::host;
    let mut h = host();
    let mut m = Model::new();
    // The host keeps it (Preferences/IntersectEdges); on is SketchUp's default.
    let r = crate::ruby_host::model::call(
        &mut h.app,
        &mut h.st,
        &mut m,
        &mut Vec::new(),
        "model.intersect_edges",
        &serde_json::json!({"on": true}),
    );
    assert!(r.is_ok());
    assert!(esq_core::ops::break_edges() && crate::settings::with(|s| s.intersect_edges));
    assert!(crate::settings::Settings::default().intersect_edges);
}

/// The clip planes and clipped faces of the 3D view.
fn view_clips(a: &crate::app::EsquisseApp) -> (usize, usize) {
    let opts = crate::scene::SceneOptions {
        ctx: &a.doc.ctx,
        ctx_def: a.doc.ctx_def(),
        sel: &a.doc.sel,
        axes_xform: esq_core::glam::DMat4::IDENTITY,
        style: &a.style,
    };
    let mesh = crate::scene::build(&a.doc.model, &opts);
    (mesh.clips.len(), mesh.opaque.iter().filter(|v| v.clip != 0).count())
}

#[test]
fn a_section_plane_activated_by_ruby_cuts_the_view() {
    let Some(interp) = super::ruby::interp() else { return };
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut a = crate::app::EsquisseApp::bare(None, None);
    let run = |a: &mut crate::app::EsquisseApp, code: &str| {
        let o = crate::ruby_host::run_script(a, "Script Ruby", |m, s| interp.run(code, "t.rb", m, s)).unwrap();
        assert!(o.ok, "{}", o.error);
        o.result
    };
    // SectionPlane#activate answers the plane.
    let r = run(
        &mut a,
        "ents = Sketchup.active_model.entities
         f = ents.add_face([0,0,0], [100,0,0], [100,100,0], [0,100,0]); f.pushpull(-100)
         sp = ents.add_section_plane([[50, 0, 0], [1, 0, 0]])
         sp.activate == sp",
    );
    assert_eq!(r, "true");
    let (clips, cut_faces) = view_clips(&a);
    assert!(clips == 1 && cut_faces > 0, "{clips} {cut_faces}");
    // Through Entities#active_section_plane= too, and for the inside of a group (the plane of
    // the model cuts what the groups hold).
    run(
        &mut a,
        "m = Sketchup.active_model; ents = m.entities
         ents.active_section_plane = nil
         g = ents.add_group(ents.grep(Sketchup::Face))
         ents.active_section_plane = ents.grep(Sketchup::SectionPlane).first",
    );
    assert_eq!(a.doc.model.root.instances.len(), 1);
    let (clips, cut_faces) = view_clips(&a);
    assert!(clips == 1 && cut_faces > 0, "{clips} {cut_faces}");
    let Some(mut gpu) = super::gpu::Gpu::new() else { return };
    let cam = crate::camera::Camera {
        size: DVec2::new(400.0, 300.0),
        target: DVec3::new(1270.0, 1270.0, 1270.0),
        distance: 8000.0,
        yaw: -90f64.to_radians(),
        pitch: 0.0,
        ..Default::default()
    };
    // On the GPU, seen from the front: the half behind the plane's normal is gone.
    let lighting = crate::render::Lighting::default();
    let cut = gpu.render(&a.doc.model, &a.style, &cam, &lighting);
    let mut whole = a.doc.model.clone();
    whole.root.active_section = None;
    let full = gpu.render(&whole, &a.style, &cam, &lighting);
    let px = |p: DVec3| cam.project(p).map(|s| (s.x as u32, s.y as u32)).unwrap();
    let (gx, gy) = px(DVec3::new(600.0, 0.0, 1270.0));
    let (kx, ky) = px(DVec3::new(1900.0, 0.0, 1270.0));
    assert_eq!(cut.at(kx, ky), full.at(kx, ky), "the kept half is unchanged");
    assert_ne!(cut.at(gx, gy), full.at(gx, gy), "the other half is cut away");
}

#[test]
fn a_section_plane_activated_by_a_ruby_tool_refreshes_the_view() {
    // A Ruby tool's events run light (no serialization): the plane and the cut must still count
    // as a change, or the view is not rebuilt.
    let Some(interp) = super::ruby::interp() else { return };
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut doc = crate::doc::Document::new(Model::new());
    let code = "ents = Sketchup.active_model.entities
                f = ents.add_face([0,0,0], [100,0,0], [100,100,0], [0,100,0]); f.pushpull(-100)";
    doc.run_script("Script Ruby", |m, s| interp.run(code, "t.rb", m, s)).unwrap();
    let rev = doc.model_rev;
    let code = "Sketchup.active_model.entities.add_section_plane([50, 0, 0], [1, 0, 0])";
    doc.run_script_light("Outil Ruby", |m, s| interp.run(code, "outil", m, s)).unwrap();
    assert!(doc.model_rev > rev);
    let rev = doc.model_rev;
    let code = "Sketchup.active_model.entities.grep(Sketchup::SectionPlane).first.activate";
    doc.run_script_light("Outil Ruby", |m, s| interp.run(code, "outil", m, s)).unwrap();
    assert!(doc.model.root.active_section.is_some() && doc.model_rev > rev);
    assert_eq!(doc.undo_label(), Some("Outil Ruby"));
}

/// A host serving `view.*` only, on the application's view state.
struct ViewHost {
    cam: crate::camera::Camera,
    style: crate::style::ViewStyle,
    active: Option<usize>,
    vs: crate::ruby_host::view::ViewState,
}

impl esq_ruby::Host for ViewHost {
    fn call(
        &mut self,
        model: &mut Model,
        _sel: &mut Vec<EntityRef>,
        op: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let mut cx = crate::ruby_host::view::ViewCx {
            cam: &mut self.cam,
            style: &mut self.style,
            active_scene: &mut self.active,
            anim: (2.0, 1.0),
            vs: &mut self.vs,
        };
        cx.call(model, op, args)
    }
}

#[test]
fn view_draw_takes_a_texture_and_uvs_and_paints_them() {
    use crate::ruby_host::view::DrawCmd;
    let Some(interp) = super::ruby::interp() else { return };
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = ViewHost {
        cam: crate::camera::Camera { size: DVec2::new(800.0, 600.0), ..Default::default() },
        style: Default::default(),
        active: None,
        vs: Default::default(),
    };
    let mut m = Model::new();
    let code = "v = Sketchup.active_model.active_view
        rep = Sketchup::ImageRep.new
        rep.set_data(2, 2, 32, 0, [0,0,255,255, 0,255,0,255, 255,0,0,255, 255,255,255,255].pack('C*'))
        t = $esq_tex = v.load_texture(rep)
        pts = [[0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0]]
        v.draw(GL_QUADS, pts, texture: t, uvs: [[0, 0], [1, 0], [1, 1], [0, 1]])
        e = ->(x) { x.is_a?(Exception) ? (x.is_a?(ArgumentError) ? x.message : x.class) : x }
        [t.class, e.((v.draw(GL_QUADS, pts, texture: t + 99, uvs: [[0, 0]] * 4) rescue $!)),
         e.((v.draw(GL_QUADS, pts, uvs: [[0, 0]]) rescue $!)), e.((v.draw(GL_QUADS, pts, normals: 3) rescue $!)),
         e.((v.draw(GL_TRIANGLES, pts) rescue $!)), e.((v.draw(42, pts) rescue $!)),
         e.((v.load_texture(3) rescue $!)), e.((v.load_texture(Sketchup::ImageRep.new) rescue $!))]";
    let o = esq_ruby::with_host(&mut h, || interp.run(code, "vue.rb", &mut m, &mut Vec::new())).unwrap();
    assert!(o.ok, "{}", o.error);
    assert_eq!(
        o.result,
        "[Integer, \"invalid texture ID\", \"uvs must match number of points. Expected 4, got 1\", TypeError, \
         \"Incorrect number of points: must be multiple of 3\", \"Invalid DrawingHelper drawing mode\", TypeError, \
         \"invalid ImageRep\"]"
    );
    assert_eq!(h.vs.draw.len(), 1);
    let DrawCmd::Poly { mode, uvs, texture, .. } = &h.vs.draw[0] else { panic!() };
    assert_eq!((*mode, uvs.as_ref().map(Vec::len), texture.is_some()), (7, Some(4), true));
    // Its pixels: the first row of the picture is blue then green.
    let t = &h.vs.textures[&texture.unwrap()];
    assert_eq!((t.size, &t.rgba[..8]), ([2, 2], &[255u8, 0, 0, 255, 0, 255, 0, 255][..]));
    // Painted as a textured mesh (two triangles).
    let ctx = eframe::egui::Context::default();
    let cam = h.cam.clone();
    let mut out = ctx.run_ui(Default::default(), |ui| {
        crate::ruby_host::view::paint(&h.vs, ui.painter(), eframe::egui::Pos2::ZERO, &cam);
    });
    out.textures_delta.clear();
    let meshes: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            eframe::egui::Shape::Mesh(m) if m.texture_id != eframe::egui::TextureId::default() => Some(m.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(meshes.len(), 1);
    assert_eq!(meshes[0].indices.len(), 6);
    assert!(meshes[0].vertices.iter().any(|v| v.uv == eframe::egui::pos2(1.0, -1.0)));
    // Released once: true, then false.
    let code = "v = Sketchup.active_model.active_view; t = $esq_tex
                [v.release_texture(t), v.release_texture(t)]";
    let o = esq_ruby::with_host(&mut h, || interp.run(code, "vue.rb", &mut m, &mut Vec::new())).unwrap();
    assert_eq!(o.result, "[true, false]");
    assert!(h.vs.textures.is_empty());
}

#[test]
fn a_scene_chosen_in_the_interface_restores_axes_section_and_folders() {
    let Some(interp) = super::ruby::interp() else { return };
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut a = crate::app::EsquisseApp::bare(None, None);
    let run = |a: &mut crate::app::EsquisseApp, code: &str| {
        let o = crate::ruby_host::run_script(a, "Script Ruby", |m, s| interp.run(code, "t.rb", m, s)).unwrap();
        assert!(o.ok, "{}", o.error);
        o.result
    };
    // A scene saved by Ruby with moved axes, an active section plane and a hidden tag folder.
    run(
        &mut a,
        "m = Sketchup.active_model; ents = m.entities
         f = m.layers.add_folder('Mobilier'); t = m.layers.add('Chaises'); f.add_layer(t)
         sp = ents.add_section_plane([0, 0, 50], [0, 0, -1])
         m.axes.set([10, 20, 0], [0, 1, 0], [-1, 0, 0], [0, 0, 1])
         ents.active_section_plane = sp
         f.visible = false
         m.pages.add('Coupe')
         m.axes.set([0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1])
         ents.active_section_plane = nil
         f.visible = true",
    );
    assert!(a.doc.model.root.active_section.is_none() && a.doc.model.meta.folders[0].visible);
    // Chosen from the scene tabs / Window › Scenes.
    a.go_to_scene(0, 0.0, true);
    let m = &a.doc.model;
    assert!((m.axes.w_axis.truncate() - DVec3::new(254.0, 508.0, 0.0)).length() < 1e-9, "{:?}", m.axes);
    assert!((m.axes.x_axis.truncate() - DVec3::Y).length() < 1e-9);
    assert!(m.root.active_section.is_some());
    assert!(!m.meta.folders[0].visible);
    // The other way round: a scene added in the interface reads the same from Ruby.
    a.add_scene();
    let r = run(
        &mut a,
        "m = Sketchup.active_model; p = m.pages[1]
         o = p.axes.origin.to_a
         m.axes.set([0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1])
         m.pages.selected_page = p
         [o, m.axes.origin.to_a]",
    );
    assert_eq!(r, "[[10.0, 20.0, 0.0], [10.0, 20.0, 0.0]]");
}

#[test]
fn casts_shadows_off_keeps_faces_out_of_the_shadow_map() {
    let Some(interp) = super::ruby::interp() else { return };
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut a = crate::app::EsquisseApp::bare(None, None);
    let o = crate::ruby_host::run_script(&mut a, "Script Ruby", |m, s| {
        interp.run(
            "ents = Sketchup.active_model.entities
             f = ents.add_face([0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0]); f.casts_shadows = false
             ents.add_face([20, 0, 0], [30, 0, 0], [30, 10, 0], [20, 10, 0])
             g = ents.add_group; g.entities.add_face([40, 0, 0], [50, 0, 0], [50, 10, 0], [40, 10, 0])
             g.casts_shadows = false",
            "t.rb",
            m,
            s,
        )
    })
    .unwrap();
    assert!(o.ok, "{}", o.error);
    let opts = crate::scene::SceneOptions {
        ctx: &[],
        ctx_def: None,
        sel: &[],
        axes_xform: esq_core::glam::DMat4::IDENTITY,
        style: &a.style,
    };
    let mesh = crate::scene::build(&a.doc.model, &opts);
    // Which faces stay out of the shadow pass, by their x (mm).
    let off = |x0: f32, x1: f32| {
        let v: Vec<_> = mesh.opaque.iter().filter(|v| v.pos[0] >= x0 && v.pos[0] <= x1).collect();
        assert!(!v.is_empty());
        v.iter().all(|v| v.tex & crate::scene::NO_SHADOW != 0)
    };
    assert!(off(0.0, 254.0), "the face that does not cast");
    assert!(!off(508.0, 762.0), "an ordinary face casts");
    assert!(off(1016.0, 1270.0), "the faces of a group that does not cast");
}

#[test]
fn the_selected_scene_is_saved_with_the_document() {
    let mut a = crate::app::EsquisseApp::bare(None, None);
    a.add_scene();
    a.add_scene();
    a.active_scene = Some(0);
    let dir = std::env::temp_dir().join(format!("esq_active_scene_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let f = dir.join("scenes.esq");
    a.save_to(&f).unwrap();
    let mut b = crate::app::EsquisseApp::bare(None, None);
    b.open_path(&f);
    assert_eq!((b.doc.model.scenes.len(), b.active_scene), (2, Some(0)));
    // No selected scene: none after opening either.
    a.active_scene = None;
    a.save_to(&f).unwrap();
    b.open_path(&f);
    assert_eq!(b.active_scene, None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn erase_removes_selected_snaps_and_spaces() {
    let mut h = Harness::new();
    let (n, p) = box_snap_space(&mut h);
    let mut app = crate::app::EsquisseApp::bare(None, None);
    app.doc = std::mem::replace(&mut h.doc, crate::doc::Document::new(Default::default()));
    app.doc.select(vec![EntityRef::Snap(n), EntityRef::Space(p)]);
    app.erase_selection();
    assert!(app.doc.model.root.snaps.is_empty() && app.doc.model.root.spaces.is_empty());
    assert!(app.doc.sel.is_empty());
    assert_eq!(app.doc.model.root.faces.len(), 6, "the box stays");
    app.doc.undo();
    assert_eq!((app.doc.model.root.snaps[0].id, app.doc.model.root.spaces[0].id), (n, p));
    // Edit › Select All takes them too; Invert Selection gives them back.
    app.select_all();
    assert!(app.doc.is_selected(EntityRef::Snap(n)) && app.doc.is_selected(EntityRef::Space(p)));
    app.doc.select(vec![]);
    app.invert_selection();
    assert!(app.doc.is_selected(EntityRef::Snap(n)) && app.doc.is_selected(EntityRef::Space(p)));
    app.doc.select(vec![]);
    // Hidden ones are not drawn, so not picked.
    app.doc.commit("Masquer", |m, d| {
        m.entities_mut(d).set_hidden(EntityRef::Space(p), true);
        m.entities_mut(d).set_hidden(EntityRef::Snap(n), true);
    });
    h.doc = app.doc;
    h.tool(ToolId::Select);
    h.click(DVec3::new(500.0, -200.0, 200.0));
    assert!(matches!(h.doc.sel.as_slice(), [EntityRef::Face(_)]), "the face behind: {:?}", h.doc.sel);
}
