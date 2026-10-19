//! Match Photo, driven through the tool trait like the application does.

use esq_core::glam::{DVec2, DVec3};
use esq_core::matchphoto::PhotoCamera;

use super::Harness;
use crate::camera::Camera;
use crate::photo_img::tests::png_bytes;
use crate::tools::photo_tests::{look_camera, truth_bars};
use crate::tools::{MatchPhotoTool, PhotoEditor};

const W: f64 = 1600.0;
const H: f64 = 1200.0;

/// A camera 8 m from the origin (the distance a new photo starts with).
fn truth() -> PhotoCamera {
    let dir = DVec3::new(-0.28, -0.93, 0.24).normalize();
    look_camera(dir * 8000.0, DVec3::ZERO, 1300.0, DVec2::new(W, H), DVec2::new(W, H) * 0.5)
}

fn fresh_editor() -> PhotoEditor {
    let [w, h] = crate::photo_img::image_size(&png_bytes(W as u32, H as u32)).unwrap();
    assert_eq!([w, h], [W as u32, H as u32]);
    PhotoEditor::new(
        "mur.png".into(),
        esq_core::photo::Blob(std::sync::Arc::new(png_bytes(W as u32, H as u32))),
        DVec2::new(W, H),
    )
}

impl Harness {
    fn press_release(&mut self, from: DVec2, to: DVec2) {
        self.with(|t, cx| {
            t.mouse_move(cx, from);
            t.press(cx, from);
            t.mouse_move(cx, (from + to) * 0.5);
            t.mouse_move(cx, to);
            t.release(cx, to);
        });
    }
}

/// Puts every bar and the origin of the tool onto the photo of `truth`.
fn match_to(h: &mut Harness, start: &PhotoEditor, truth: &PhotoCamera) {
    let map = start.map(h.cam.size);
    let goal = truth_bars(truth);
    for (a_axis, g_axis) in start.bars.iter().zip(&goal).take(2) {
        for (a, b) in a_axis.iter().zip(g_axis) {
            h.press_release(map.to_screen(a.a), map.to_screen(b.a));
            h.press_release(map.to_screen(a.b), map.to_screen(b.b));
        }
    }
    h.press_release(map.to_screen(start.origin), map.to_screen(truth.project(DVec3::ZERO).unwrap()));
}

/// A click on the grey around the photo.
fn click_outside(h: &mut Harness, ed: &PhotoEditor) {
    let (tl, _) = ed.map(h.cam.size).rect();
    let p = tl - DVec2::new(6.0, 6.0);
    h.press_release(p, p);
}

#[test]
fn matching_a_photo_makes_a_scene_whose_camera_fits_the_photo() {
    let mut h = Harness::new();
    let truth = truth();
    let start = fresh_editor();
    h.tool = Box::new(MatchPhotoTool::with_editor(start.clone()));
    match_to(&mut h, &start, &truth);

    // The 3D view now sees the world the way the photo does: a world point lands on the
    // pixel of the photo that shows it.
    let map = start.map(h.cam.size);
    for p in [DVec3::new(1500.0, -700.0, 0.0), DVec3::new(-900.0, 600.0, 1200.0), DVec3::ZERO] {
        let want = map.to_screen(truth.project(p).unwrap());
        let got = h.cam.project(p).unwrap();
        assert!(got.distance(want) < 0.5, "{p:?}: {got:?} vs {want:?}");
    }
    // A ray through the pixel of a point comes back to that point.
    let p = DVec3::new(800.0, 500.0, 900.0);
    let (o, d) = h.cam.ray(h.cam.project(p).unwrap());
    assert!((o + d * (p - o).length()).distance(p) < 1.0);

    // Click outside the photo: the match is done and the scene exists.
    click_outside(&mut h, &start);
    assert_eq!(h.doc.model.scenes.len(), 1);
    let scene = &h.doc.model.scenes[0];
    assert_eq!(scene.name, "mur");
    let photo = scene.photo.as_ref().expect("photo kept");
    assert_eq!(photo.size, [W as u32, H as u32]);
    assert!((photo.camera.focal - truth.focal).abs() < 0.002 * truth.focal, "{}", photo.camera.focal);
    assert!(photo.camera.eye().distance(truth.eye()) < 10.0);
    assert!((scene.camera.eye - truth.eye()).length() < 10.0);
    // The camera of the scene is the one the 3D view uses.
    assert!(h.cam.is_photo_view(&photo.camera));
    assert_eq!(h.tool.finished(), Some(Some(0)));
    // One undo step.
    h.doc.undo();
    assert!(h.doc.model.scenes.is_empty());
}

#[test]
fn editing_a_matched_photo_replaces_its_scene() {
    let mut h = Harness::new();
    let truth = truth();
    let start = fresh_editor();
    h.tool = Box::new(MatchPhotoTool::with_editor(start.clone()));
    match_to(&mut h, &start, &truth);
    click_outside(&mut h, &start);
    let first = h.doc.model.scenes[0].photo.clone().unwrap();

    // Edit Matched Photo: the bars come back where they were left; moving the origin
    // and validating again changes the same scene.
    let ed = PhotoEditor::from_photo(0, &first);
    assert_eq!(ed.bars, start_bars_after(&first));
    h.tool = Box::new(MatchPhotoTool::with_editor(ed.clone()));
    let map = ed.map(h.cam.size);
    let o = truth.project(DVec3::ZERO).unwrap();
    h.press_release(map.to_screen(o), map.to_screen(o + DVec2::new(120.0, 60.0)));
    click_outside(&mut h, &ed);
    assert_eq!(h.doc.model.scenes.len(), 1, "same scene");
    let second = h.doc.model.scenes[0].photo.clone().unwrap();
    assert_ne!(second.setup.origin, first.setup.origin);
    assert!(second.camera.eye().distance(first.camera.eye()) > 1.0, "the camera moved with the origin");
    assert_eq!(h.tool.finished(), Some(Some(0)));
}

fn start_bars_after(p: &esq_core::photo::MatchPhoto) -> [[esq_core::matchphoto::Line2; 2]; 3] {
    let ed = PhotoEditor::from_photo(0, p);
    ed.bars
}

/// Paints the tool's backdrop, bars and panel in a headless egui frame: nothing panics and
/// something is drawn.
#[test]
fn the_tool_paints_its_backdrop_overlay_and_panel() {
    use crate::tools::{Overlay, Tool};
    let mut h = Harness::new();
    let mut tool = MatchPhotoTool::with_editor(fresh_editor());
    // Bring the camera of the tool to the harness camera like the application does.
    let viewport = h.cam.size;
    h.cam = tool.camera(viewport).unwrap();
    let ctx = eframe::egui::Context::default();
    let mut shapes = 0;
    for frame in 0..2 {
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let rect =
                eframe::egui::Rect::from_min_size(eframe::egui::pos2(0.0, 0.0), eframe::egui::vec2(1280.0, 800.0));
            let mut o = Overlay {
                painter: ui.painter_at(rect),
                origin: rect.min,
                cam: &h.cam,
                doc: &h.doc,
                units: h.doc.model.units,
            };
            assert!(tool.backdrop(&o), "frame {frame}");
            tool.overlay(&mut o);
        });
        shapes += out.shapes.len();
        let uploaded = !out.textures_delta.set.is_empty();
        out.textures_delta.clear();
        // The photo texture is uploaded once.
        assert!(uploaded || frame > 0);
    }
    assert!(shapes > 20, "{shapes} shapes");
    // The panel.
    let mut out = ctx.run_ui(Default::default(), |ui| {
        let rect = eframe::egui::Rect::from_min_size(eframe::egui::pos2(0.0, 0.0), eframe::egui::vec2(1280.0, 800.0));
        let _ = ui;
        h.with(|_, cx| tool.panel(ui.ctx(), rect, cx));
    });
    out.textures_delta.clear();
    assert!(!out.shapes.is_empty());
    // With no photo yet, the panel offers to pick one and nothing is drawn behind the view.
    let mut empty = MatchPhotoTool::default();
    let o = Overlay {
        painter: ctx.layer_painter(eframe::egui::LayerId::background()),
        origin: eframe::egui::pos2(0.0, 0.0),
        cam: &h.cam,
        doc: &h.doc,
        units: h.doc.model.units,
    };
    assert!(!empty.backdrop(&o));
}

#[test]
fn escape_during_a_match_changes_nothing() {
    let mut h = Harness::new();
    let start = fresh_editor();
    h.tool = Box::new(MatchPhotoTool::with_editor(start));
    assert!(h.tool.busy());
    h.with(|t, cx| t.cancel(cx));
    assert!(h.doc.model.scenes.is_empty());
    assert_eq!(h.tool.finished(), Some(None));
    assert!(!h.tool.busy());
}

#[test]
fn the_view_of_a_matched_photo_fits_any_viewport() {
    let truth = truth();
    for vp in [DVec2::new(1280.0, 800.0), DVec2::new(700.0, 900.0), DVec2::new(2000.0, 500.0)] {
        let (cam, map) = Camera::from_photo(&truth, vp, 1.0, DVec2::ZERO);
        assert!(cam.is_photo_view(&truth));
        for p in [DVec3::new(1500.0, -700.0, 0.0), DVec3::new(-900.0, 600.0, 1200.0)] {
            let want = map.to_screen(truth.project(p).unwrap());
            assert!(cam.project(p).unwrap().distance(want) < 1e-6);
        }
        // The whole photo is in the viewport and touches it on one side.
        let (tl, br) = map.rect();
        assert!(tl.x >= -1e-9 && tl.y >= -1e-9 && br.x <= vp.x + 1e-9 && br.y <= vp.y + 1e-9);
        assert!((tl.x.abs() < 1e-6 && (br.x - vp.x).abs() < 1e-6) || (tl.y.abs() < 1e-6 && (br.y - vp.y).abs() < 1e-6));
    }
}

/// Writes `box.esq` (a box and a photo scene whose picture shows the same box, drawn from a
/// known camera) next to the file named by `ESQ_DEMO_OUT`, to look at the result with
/// `esquisse --snapshot`. Run with `cargo test write_demo_photo_file -- --ignored`.
#[test]
#[ignore]
fn write_demo_photo_file() {
    let Ok(out) = std::env::var("ESQ_DEMO_OUT") else { return };
    let mut h = Harness::new();
    h.rect_and_box();
    let size = DVec2::new(W, H);
    let truth =
        look_camera(DVec3::new(-2800.0, -6200.0, 1700.0), DVec3::new(500.0, 300.0, 150.0), 1500.0, size, size * 0.5);
    // The "photograph": sky, ground, and the box drawn with black edges.
    let mut img = image::RgbaImage::new(W as u32, H as u32);
    let horizon = truth.project(DVec3::new(0.0, 1e7, 0.0)).map(|p| p.y).unwrap_or(H * 0.4).clamp(0.0, H);
    for (_, y, p) in img.enumerate_pixels_mut() {
        *p = if (y as f64) < horizon { image::Rgba([160, 200, 235, 255]) } else { image::Rgba([120, 150, 100, 255]) };
    }
    let mut line = |a: DVec3, b: DVec3, c: [u8; 4]| {
        let (pa, pb) = (truth.project(a).unwrap(), truth.project(b).unwrap());
        let n = (pa.distance(pb) * 2.0) as i32 + 1;
        for i in 0..=n {
            let p = pa.lerp(pb, i as f64 / n as f64);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let (x, y) = ((p.x as i32 + dx) as u32, (p.y as i32 + dy) as u32);
                    if x < W as u32 && y < H as u32 {
                        img.put_pixel(x, y, image::Rgba(c));
                    }
                }
            }
        }
    };
    let (x, y, z) = (1000.0, 600.0, 400.0);
    for (a, b) in [
        ((0.0, 0.0, 0.0), (x, 0.0, 0.0)),
        ((0.0, y, 0.0), (x, y, 0.0)),
        ((0.0, 0.0, 0.0), (0.0, y, 0.0)),
        ((x, 0.0, 0.0), (x, y, 0.0)),
        ((0.0, 0.0, z), (x, 0.0, z)),
        ((0.0, y, z), (x, y, z)),
        ((0.0, 0.0, z), (0.0, y, z)),
        ((x, 0.0, z), (x, y, z)),
        ((0.0, 0.0, 0.0), (0.0, 0.0, z)),
        ((x, 0.0, 0.0), (x, 0.0, z)),
        ((0.0, y, 0.0), (0.0, y, z)),
        ((x, y, 0.0), (x, y, z)),
    ] {
        line(DVec3::new(a.0, a.1, a.2), DVec3::new(b.0, b.1, b.2), [20, 20, 20, 255]);
    }
    // A long red line along the X axis, to see the world axes on the photo.
    line(DVec3::new(-1500.0, 0.0, 0.0), DVec3::new(3000.0, 0.0, 0.0), [200, 40, 40, 255]);
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();

    // The match, made from the box's own edges.
    let mut ed = PhotoEditor::new("maison.png".into(), esq_core::photo::Blob(std::sync::Arc::new(bytes)), size);
    let seg = |a: (f64, f64, f64), b: (f64, f64, f64)| esq_core::matchphoto::Line2 {
        a: truth.project(DVec3::new(a.0, a.1, a.2)).unwrap(),
        b: truth.project(DVec3::new(b.0, b.1, b.2)).unwrap(),
    };
    ed.bars[0] = [seg((0.0, 0.0, 0.0), (x, 0.0, 0.0)), seg((0.0, y, z), (x, y, z))];
    ed.bars[1] = [seg((0.0, 0.0, 0.0), (0.0, y, 0.0)), seg((x, 0.0, z), (x, y, z))];
    ed.origin = truth.project(DVec3::ZERO).unwrap();
    ed.set_distance(truth.trans.length());
    assert!(ed.error.is_none());
    let photo = ed.photo().unwrap();
    h.doc.commit("Photo", |m, _| {
        m.scenes.push(esq_core::model::Scene {
            name: "maison".into(),
            description: String::new(),
            camera: photo.scene_camera(),
            hidden_layers: None,
            photo: Some(photo),
            ext: Default::default(),
        });
    });
    esq_core::io::save_esq(&h.doc.model, std::path::Path::new(&out)).unwrap();
}

#[test]
fn going_to_a_photo_scene_and_leaving_it() {
    let truth = truth();
    let scene_camera = truth.scene_camera(truth.trans.length());
    // Go to Scene: the camera is rebuilt from the saved point of view.
    let mut cam = Camera { size: DVec2::new(1280.0, 800.0), ..Default::default() };
    cam.set_from_view(&scene_camera);
    assert!(cam.is_photo_view(&truth));
    // Zoom Matched Photo from anywhere brings the photo back.
    cam.orbit(30.0, 10.0);
    assert!(!cam.is_photo_view(&truth));
    assert_eq!(cam.roll, 0.0);
    assert_eq!(cam.shift, DVec2::ZERO);
    let (back, _) = Camera::from_photo(&truth, cam.size, 1.0, DVec2::ZERO);
    assert!(back.is_photo_view(&truth));
    // Zooming or panning the view takes it off the photo too.
    let mut c = back.clone();
    c.zoom_at(0.5, None);
    assert!(!c.is_photo_view(&truth));
    let mut c = back.clone();
    c.pan(40.0, 0.0);
    assert!(!c.is_photo_view(&truth));
}

#[test]
fn a_rolled_and_cropped_photo_view_is_exact() {
    // Camera with a tilted horizon and an optical centre away from the image centre.
    let mut cam = look_camera(
        DVec3::new(3000.0, -6000.0, 1500.0),
        DVec3::new(0.0, 0.0, 600.0),
        1100.0,
        DVec2::new(W, H),
        DVec2::new(W * 0.5 + 90.0, H * 0.5 - 40.0),
    );
    // Roll the camera by 4 degrees about its view axis.
    let r = esq_core::glam::DMat3::from_axis_angle(DVec3::Z, 4f64.to_radians());
    cam.rot = r * cam.rot;
    cam.trans = r * cam.trans;
    let (view, map) = Camera::from_photo(&cam, DVec2::new(1280.0, 800.0), 0.8, DVec2::new(20.0, -10.0));
    assert!(view.roll.abs() > 0.01);
    assert!(view.is_photo_view(&cam));
    for p in [DVec3::ZERO, DVec3::new(900.0, 400.0, 700.0), DVec3::new(-700.0, -200.0, 100.0)] {
        let want = map.to_screen(cam.project(p).unwrap());
        let got = view.project(p).unwrap();
        assert!(got.distance(want) < 1e-6, "{p:?}: {got:?} vs {want:?}");
    }
}
