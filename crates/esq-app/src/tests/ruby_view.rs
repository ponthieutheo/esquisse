//! L'API Ruby de la vue (zone « view ») branchée sur la vraie caméra et les vrais réglages
//! d'affichage de l'application (`ruby_host::view::ViewCx`).

use esq_core::glam::{DVec2, DVec3};
use esq_core::model::{EntityRef, Model};
use esq_ruby::RunOutput;
use serde_json::Value as J;

use super::ruby::interp;
use crate::camera::Camera;
use crate::ruby_host::view::{DrawCmd, ViewCx, ViewState};
use crate::style::{FaceStyle, ViewStyle};

/// L'hôte est global : un seul test à la fois le prête à Ruby (verrou commun à toutes les zones).
use super::ruby::HOST_LOCK as LOCK;

struct TestHost {
    cam: Camera,
    style: ViewStyle,
    active: Option<usize>,
    vs: ViewState,
}

impl esq_ruby::Host for TestHost {
    fn call(&mut self, model: &mut Model, _sel: &mut Vec<EntityRef>, op: &str, args: &J) -> Result<J, String> {
        let mut cx = ViewCx {
            cam: &mut self.cam,
            style: &mut self.style,
            active_scene: &mut self.active,
            anim: (2.0, 1.0),
            vs: &mut self.vs,
        };
        cx.call(model, op, args)
    }
}

fn host() -> TestHost {
    TestHost {
        cam: Camera { size: DVec2::new(800.0, 600.0), ..Default::default() },
        style: ViewStyle::default(),
        active: None,
        vs: ViewState::default(),
    }
}

fn run(h: &mut TestHost, m: &mut Model, code: &str) -> Option<RunOutput> {
    let i = interp()?;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut sel = Vec::new();
    let o = esq_ruby::with_host(h, || i.run(code, "vue.rb", m, &mut sel)).unwrap();
    assert!(o.ok, "erreur Ruby: {}", o.error);
    Some(o)
}

#[test]
fn view_camera_is_the_application_camera() {
    let mut h = host();
    let mut m = Model::new();
    let eye0 = h.cam.eye() / 25.4;
    let Some(o) = run(
        &mut h,
        &mut m,
        "v = Sketchup.active_model.active_view
         c = v.camera
         before = [c.eye.to_a.map { |x| x.round(3) }, c.fov.round(3), c.perspective?, c.fov_is_height?, v.vpwidth, v.vpheight,
                   v.device_width, v.center, v.corner(3), v.field_of_view.round(3)]
         v.camera = Sketchup::Camera.new([100, 0, 0], [0, 0, 0], [0, 0, 1])
         c.set([0, -200, 50], [0, 0, 50], [0, 0, 1])
         [before, v.camera.eye.to_a.map { |x| x.round(6) }]",
    ) else {
        return;
    };
    let want = format!("[[{:.3}, {:.3}, {:.3}]", eye0.x, eye0.y, eye0.z);
    assert!(
        o.result.starts_with(&want.replace(".000", ".0")) || o.result.contains("35.0, true, true, 800.0, 600.0, 800"),
        "{}",
        o.result
    );
    assert!(
        o.result.contains("35.0, true, true, 800.0, 600.0, 800, [400.0, 300.0], [800.0, 600.0], 35.0]"),
        "{}",
        o.result
    );
    // La caméra liée écrit directement dans celle de l'application.
    assert!(o.result.ends_with("[0.0, -200.0, 50.0]]"), "{}", o.result);
    assert!((h.cam.eye() - DVec3::new(0.0, -200.0, 50.0) * 25.4).length() < 1e-6);
    assert!((h.cam.target - DVec3::new(0.0, 0.0, 50.0) * 25.4).length() < 1e-6);
}

#[test]
fn view_camera_fov_parallel_projection_and_zoom() {
    let mut h = host();
    let mut m = Model::new();
    let Some(o) = run(
        &mut h,
        &mut m,
        "v = Sketchup.active_model.active_view
         v.camera = Sketchup::Camera.new([0, -100, 0], [0, 0, 0], [0, 0, 1], true, 50)
         f = v.field_of_view
         v.field_of_view = 70
         v.zoom(2.0)
         d = v.camera.eye.distance(v.camera.target)
         c = v.camera
         c.perspective = false
         [f.round(6), v.field_of_view.round(6), d.round(6), c.perspective?, v.camera.height.round(3)]",
    ) else {
        return;
    };
    // Parallèle : la hauteur visible vaut 2·d·tan(fov/2) avec d = 50 pouces et fov = 70°.
    let hgt = 2.0 * 50.0 * 35f64.to_radians().tan();
    assert_eq!(o.result, format!("[50.0, 70.0, 50.0, false, {:.3}]", (hgt * 1000.0).round() / 1000.0));
    assert!(!h.cam.perspective);
}

#[test]
fn rendering_options_drive_the_view_style() {
    let mut h = host();
    let mut m = Model::new();
    let Some(o) = run(
        &mut h,
        &mut m,
        "ro = Sketchup.active_model.rendering_options
         a = [ro['DrawHiddenGeometry'], ro['RenderMode'], ro['Texture'], ro['DisplaySectionPlanes'], ro['EdgeDisplayMode']]
         ro['DrawHiddenGeometry'] = true
         ro['RenderMode'] = 0
         ro['EdgeDisplayMode'] = 0
         ro['AmbientOcclusionDistance'] = 10
         ro['BackgroundColor'] = [1, 2, 3]
         ro['BandColor'] = [4, 5, 6]
         bad = begin; ro['RenderMode'] = 9; 'aucune'; rescue ArgumentError => e; e.message; end
         [a, ro['DrawHiddenGeometry'], ro['RenderMode'], ro['BackgroundColor'].to_a, bad]",
    ) else {
        return;
    };
    // « Ombré avec textures » : RenderMode 2 avec Texture, comme dans SketchUp.
    assert_eq!(
        o.result,
        "[[false, 2, true, false, 1], true, 0, [1, 2, 3, 255], \"Rendering option could not be set to the given value\"]"
    );
    assert!(h.style.hidden_geometry);
    assert_eq!(h.style.faces, FaceStyle::Wireframe);
    assert!(!h.style.edges);
    assert!((h.style.ao_distance - 254.0).abs() < 1e-3);
    // Le style de l'application et l'API Ruby partagent les réglages.
    assert_eq!(h.style.bg_color, [1, 2, 3]);
    // Ce que l'application n'affiche pas reste dans le modèle (enregistré avec lui).
    assert_eq!(m.view_ext["ro"]["BandColor"], serde_json::json!([4, 5, 6, 255]));
}

#[test]
fn pages_capture_and_restore_the_view() {
    let mut h = host();
    let mut m = Model::new();
    let Some(o) = run(
        &mut h,
        &mut m,
        "m = Sketchup.active_model
         v = m.active_view
         m.layers.add('Toit')
         v.camera = Sketchup::Camera.new([0, -100, 10], [0, 0, 10], [0, 0, 1])
         m.layers['Toit'].visible = false if m.layers['Toit'].respond_to?(:visible=)
         a = m.pages.add('Avant')
         v.camera = Sketchup::Camera.new([100, 0, 10], [0, 0, 10], [0, 0, 1])
         b = m.pages.add('Côté')
         r = [m.pages.selected_page, a.camera.eye.to_a.map { |x| x.round(6) }, b.camera.eye.to_a.map { |x| x.round(6) }]
         m.pages.selected_page = a
         r << v.camera.eye.to_a.map { |x| x.round(6) } << m.pages.selected_page.name
         a.use_camera = false
         m.pages.selected_page = b
         m.pages.selected_page = a
         r << v.camera.eye.to_a.map { |x| x.round(6) } << m.pages.selected_page.name
         b.camera.set([0, 0, 300], [0, 0, 0], [0, 1, 0])
         r << b.camera.eye.to_a
         r",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[nil, [0.0, -100.0, 10.0], [100.0, 0.0, 10.0], [0.0, -100.0, 10.0], \"Avant\", [100.0, 0.0, 10.0], \"Avant\", [0.0, 0.0, 300.0]]"
    );
    assert_eq!(h.active, Some(0));
    assert_eq!(m.scenes.len(), 2);
    assert!((m.scenes[1].camera.eye - DVec3::new(0.0, 0.0, 300.0) * 25.4).length() < 1e-6);
}

#[test]
fn picking_inferences_and_projection() {
    let mut h = host();
    let mut m = Model::new();
    let Some(o) = run(
        &mut h,
        &mut m,
        "m = Sketchup.active_model
         v = m.active_view
         m.entities.add_face([10, 10, 0], [110, 10, 0], [110, 110, 0], [10, 110, 0])
         v.camera = Sketchup::Camera.new([60, -140, 200], [60, 60, 0], [0, 0, 1])
         sc = v.screen_coords([60, 60, 0]).to_a.first(2).map(&:round)
         ray = v.pickray(400, 300)
         ip = v.inputpoint(400, 300)
         ph = v.pick_helper
         n = ph.do_pick(400, 300)
         [sc, ray[0].to_a.map { |x| x.round(3) }, ray[1].to_a.map { |x| x.round(6) }, ip.valid?, ip.face.class, ip.position.z.round(6),
          ip.degrees_of_freedom, ip.depth, n, ph.picked_face.class, ph.best_picked.class, ph.depth_at(0), ph.path_at(0).map(&:class),
          ph.test_point([60, 60, 0], 400, 300), ph.pick_segment([[50, 60, 0], [70, 60, 0]], 400, 300),
          v.pixels_to_model(1, [60, 60, 0]) > 0, v.guess_target.to_a.map { |x| x.round(3) },
          ph.window_pick([0, 0], [800, 600], Sketchup::PickHelper::PICK_INSIDE) >= 5,
          ph.boundingbox_pick(m.bounds, Sketchup::PickHelper::PICK_INSIDE)]",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[[400, 300], [60.0, -140.0, 200.0], [0.0, 0.707107, -0.707107], true, Sketchup::Face, 0.0, 2, 1, 1, Sketchup::Face, Sketchup::Face, 1, [Sketchup::Face], true, -1, true, [60.0, 60.0, 0.0], true, 5]"
    );
}

#[test]
fn drawing_and_images_are_handed_to_the_application() {
    let mut h = host();
    let mut m = Model::new();
    let Some(o) = run(
        &mut h,
        &mut m,
        "v = Sketchup.active_model.active_view
         v.drawing_color = 'red'
         v.line_width = 3
         v.draw_lines([0, 0, 0], [1, 0, 0])
         v.draw(GL_TRIANGLES, [[0, 0, 0], [1, 0, 0], [0, 1, 0]])
         v.draw_points([[0, 0, 0]], 8, 2, 'blue')
         v.draw_text([30, 40, 0], 'Bonjour', size: 20)
         v.draw2d(GL_LINES, [10, 20, 0], [110, 20, 0])
         v.tooltip = 'Info'
         [v.write_image('C:/temp/vue.png', 64, 32), v.write_image(filename: 'C:/temp/vue.jpg', width: 10, height: 10)]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[true, true]");
    assert_eq!(h.vs.draw.len(), 5);
    // draw_text and draw2d take screen pixels: no inch → mm conversion.
    assert!(matches!(&h.vs.draw[3], DrawCmd::Text { pt, .. } if pt.x == 30.0 && pt.y == 40.0));
    assert!(matches!(&h.vs.draw[4], DrawCmd::Poly { pts, two_d: true, .. } if pts[1].x == 110.0));
    match &h.vs.draw[0] {
        DrawCmd::Poly { mode, pts, color, width, .. } => {
            assert_eq!((*mode, pts.len(), *color, *width), (1, 2, [255, 0, 0, 255], 3.0));
            assert!((pts[1].x - 25.4).abs() < 1e-9);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(&h.vs.draw[3], DrawCmd::Text { text, size, .. } if text == "Bonjour" && *size == 20.0));
    assert_eq!(h.vs.tooltip.as_deref(), Some("Info"));
    assert_eq!(h.vs.images.len(), 2);
    assert_eq!(h.vs.images[0].size, [64, 32]);
}

#[test]
fn styles_added_from_style_files_bring_their_settings() {
    let mut h = host();
    let mut m = Model::new();
    let mut view = ViewStyle { bg_color: [9, 8, 7], jitter: true, ..Default::default() };
    view.watermark_list.push(crate::style::Watermark {
        name: "Papier".into(),
        image: esq_core::photo::Blob(std::sync::Arc::new(crate::photo_img::tests::png_bytes(2, 2))),
        background: true,
        blend: 0.5,
        mask: false,
        layout: crate::style::WmLayout::Tiled,
        keep_aspect: true,
        scale: 1.0,
        position: 4,
        skp: Default::default(),
    });
    let f = crate::style_file::StyleFile {
        name: "Crayon".into(),
        description: "Pour l'essai".into(),
        ro: view.style_snapshot(),
        watermarks: view.watermark_list.clone(),
        ..Default::default()
    };
    let path = std::env::temp_dir().join(format!("esq_style_{}.style", std::process::id()));
    std::fs::write(&path, crate::style_file::write(&f)).unwrap();
    let code = format!(
        "s = Sketchup.active_model.styles.add_style({:?}, true)
         [s.name, s.description, Sketchup.active_model.styles.selected_style.name]",
        path.to_string_lossy().replace('\\', "/")
    );
    let Some(o) = run(&mut h, &mut m, &code) else { return };
    let _ = std::fs::remove_file(&path);
    assert_eq!(o.result, "[\"Crayon\", \"Pour l'essai\", \"Crayon\"]");
    // Selecting it applied its settings and watermarks to the view.
    assert_eq!(h.style.bg_color, [9, 8, 7]);
    assert!(h.style.jitter);
    assert_eq!(h.style.watermark_list.len(), 1);
    // The Styles panel sees the same list.
    let (list, sel) = crate::styles::model_styles(&mut m, &h.style);
    assert_eq!(list.iter().find(|s| s.id == sel).unwrap().name, "Crayon");
}
