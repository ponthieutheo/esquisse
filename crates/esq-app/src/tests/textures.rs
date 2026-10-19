//! Texture positioning, sampling and the texture commands of a face, driven as a user would.

use eframe::egui::{self, Modifiers};
use esq_core::glam::{DVec2, DVec3};
use esq_core::model::{EntityRef, FaceId, MaterialId};

use super::Harness;
use super::display::{quad_png, texture};
use crate::pick::{InferCx, PickCache};
use crate::texturing::{TexCmd, apply, checked_in, enabled_in};
use crate::tools::postex::PositionTextureTool;
use crate::tools::{Tool, ToolCx, ToolId};

/// A 1000 mm square on the ground facing up, both sides painted with a texture whose tile is
/// 500 mm.
fn textured_square(h: &mut Harness) -> (FaceId, MaterialId) {
    let mut out = None;
    h.doc.commit("carré", |m, _| {
        let mat = m.add_material("Damier", [128, 128, 128]);
        m.materials[mat].ext.texture = Some(texture(quad_png(), 500.0));
        let v = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)]
            .map(|(x, y)| m.root.add_vertex_raw(DVec3::new(x, y, 0.0)));
        let f = m.root.add_face_raw(v.to_vec(), Vec::new()).unwrap();
        m.root.faces[f].front = Some(mat);
        m.root.faces[f].back = Some(mat);
        out = Some((f, mat));
    });
    out.unwrap()
}

/// Runs `f` on a concrete tool with the harness' context (like `Harness::with`).
fn with<T: Tool + ?Sized, R>(h: &mut Harness, t: &mut T, f: impl FnOnce(&mut T, &mut ToolCx) -> R) -> R {
    if h.pick_rev != h.doc.model_rev {
        h.pick = PickCache::build(&h.doc.model, &h.style);
        h.pick_rev = h.doc.model_rev;
    }
    let axes = h.doc.model.axes;
    let mut mat = h.material;
    let (mut msg, mut tag, mut new_cam, mut edit_text, mut show) = (None, 0, None, None, false);
    let mut cx = ToolCx {
        doc: &mut h.doc,
        infer: InferCx { cam: &h.cam, pick: &h.pick, axes, recent: None },
        lock: None,
        mods: h.mods,
        units: esq_core::model::Units::Millimeters,
        material: &mut mat,
        tag: &mut tag,
        message: &mut msg,
        new_cam: &mut new_cam,
        edit_text: &mut edit_text,
        show_sections: &mut show,
    };
    let r = f(t, &mut cx);
    h.material = mat;
    r
}

fn press(h: &mut Harness, t: &mut (impl Tool + ?Sized), p: DVec3) {
    let m = h.px(p);
    with(h, t, |t, cx| {
        t.mouse_move(cx, m);
        t.press(cx, m);
    });
}

fn move_to(h: &mut Harness, t: &mut (impl Tool + ?Sized), p: DVec3) {
    let m = h.px(p);
    with(h, t, |t, cx| t.mouse_move(cx, m));
}

fn release(h: &mut Harness, t: &mut (impl Tool + ?Sized), p: DVec3) {
    let m = h.px(p);
    with(h, t, |t, cx| t.release(cx, m));
}

fn drag(h: &mut Harness, t: &mut (impl Tool + ?Sized), from: DVec3, to: DVec3) {
    press(h, t, from);
    move_to(h, t, (from + to) / 2.0);
    move_to(h, t, to);
    release(h, t, to);
}

fn uv(h: &Harness, f: FaceId, p: DVec3) -> DVec2 {
    h.doc.ents().faces[f].tex.as_ref().and_then(|t| t.front).expect("texture positionnée").at(p)
}

fn close(a: DVec2, b: DVec2) -> bool {
    (a - b).length() < 1e-6
}

fn start(h: &Harness, f: FaceId) -> PositionTextureTool {
    PositionTextureTool::start(&h.doc, &h.cam, f, None, ToolId::Select).expect("outil lancé")
}

#[test]
fn position_texture_with_fixed_pins() {
    let mut h = Harness::new();
    let (f, _) = textured_square(&mut h);
    h.doc.select(vec![EntityRef::Face(f)]);
    assert!(enabled_in(&h.doc, &h.cam, &h.style, TexCmd::Position));
    assert!(!enabled_in(&h.doc, &h.cam, &h.style, TexCmd::Reset), "rien à réinitialiser");
    let mut t = start(&h, f);
    assert!(!t.back(), "la caméra voit le dessus");
    // The pins sit on the corners of the tile under the face's centre.
    let corners = [(500.0, 500.0), (1000.0, 500.0), (500.0, 1000.0), (1000.0, 1000.0)];
    for (i, (x, y)) in corners.iter().enumerate() {
        assert!(t.pin_world(i).distance(DVec3::new(*x, *y, 0.0)) < 1e-6, "punaise {i} : {:?}", t.pin_world(i));
    }
    // Red pin: moves the texture.
    let from = t.pin_world(0);
    drag(&mut h, &mut t, from, DVec3::new(600.0, 550.0, 0.0));
    assert!(h.doc.preview.is_some(), "aperçu pendant le positionnement");
    with(&mut h, &mut t, |t, cx| t.cancel(cx));
    assert_eq!(t.switch_to(), Some(ToolId::Select));
    assert_eq!(h.doc.undo_label(), Some("Positionner la texture"));
    assert!(close(uv(&h, f, DVec3::new(600.0, 550.0, 0.0)), DVec2::new(1.0, 1.0)));
    assert!(close(uv(&h, f, DVec3::new(1100.0, 550.0, 0.0)), DVec2::new(2.0, 1.0)), "la taille reste");

    // The pins come back where they were left; green pin: scale and rotate about the red one.
    let mut t = start(&h, f);
    assert!(t.pin_world(0).distance(DVec3::new(600.0, 550.0, 0.0)) < 1e-6);
    let from = t.pin_world(1);
    drag(&mut h, &mut t, from, DVec3::new(600.0, 1550.0, 0.0));
    with(&mut h, &mut t, |t, cx| t.cancel(cx));
    assert!(close(uv(&h, f, DVec3::new(600.0, 550.0, 0.0)), DVec2::new(1.0, 1.0)));
    assert!(close(uv(&h, f, DVec3::new(600.0, 1550.0, 0.0)), DVec2::new(2.0, 1.0)), "quart de tour, double");

    // Blue pin: scale and shear, the red-green line stays.
    let mut t = start(&h, f);
    let (p0, p1) = (t.pin_world(0), t.pin_world(1));
    let blue = t.pin_world(2);
    drag(&mut h, &mut t, blue, blue + DVec3::new(-300.0, 200.0, 0.0));
    with(&mut h, &mut t, |t, cx| t.cancel(cx));
    let m = h.doc.ents().faces[f].tex.as_ref().unwrap().front.unwrap();
    assert!(close(m.at(p0), DVec2::new(1.0, 1.0)) && close(m.at(p1), DVec2::new(2.0, 1.0)));
    assert!(close(m.at(blue + DVec3::new(-300.0, 200.0, 0.0)), DVec2::new(1.0, 2.0)));
    assert!(m.is_affine());

    // Yellow pin: distorts (a perspective of the image).
    let mut t = start(&h, f);
    let y = t.pin_world(3);
    drag(&mut h, &mut t, y, y + DVec3::new(150.0, -100.0, 0.0));
    with(&mut h, &mut t, |t, cx| t.cancel(cx));
    let m = h.doc.ents().faces[f].tex.as_ref().unwrap().front.unwrap();
    assert!(!m.is_affine(), "déformée");
    assert!(close(m.at(y + DVec3::new(150.0, -100.0, 0.0)), DVec2::new(2.0, 2.0)));
    // Make Unique Texture is offered for a distorted texture.
    assert!(enabled_in(&h.doc, &h.cam, &h.style, TexCmd::MakeUnique));
}

#[test]
fn position_texture_lift_free_pins_keys_menu_and_escape() {
    let mut h = Harness::new();
    let (f, _) = textured_square(&mut h);
    h.doc.select(vec![EntityRef::Face(f)]);
    let mut t = start(&h, f);
    // A click on a pin lifts it; it follows the cursor without moving the texture.
    let p2 = t.pin_world(2);
    press(&mut h, &mut t, p2);
    release(&mut h, &mut t, p2);
    assert!(t.lifted());
    let before = t.map().unwrap();
    move_to(&mut h, &mut t, DVec3::new(300.0, 700.0, 0.0));
    press(&mut h, &mut t, DVec3::new(300.0, 700.0, 0.0));
    release(&mut h, &mut t, DVec3::new(300.0, 700.0, 0.0));
    assert!(!t.lifted());
    assert!(t.pin_world(2).distance(DVec3::new(300.0, 700.0, 0.0)) < 1e-6);
    let after = t.map().unwrap();
    for p in [DVec3::new(10.0, 20.0, 0.0), DVec3::new(900.0, 400.0, 0.0)] {
        assert!(close(before.at(p), after.at(p)), "la texture n'a pas bougé");
    }
    // Arrows: one pixel; Shift: ten.
    let red = t.pin_world(0);
    let px = h.cam.pixel_size_at(red);
    with(&mut h, &mut t, |t, cx| t.own_key(cx, egui::Key::ArrowRight, Modifiers::NONE));
    assert!((t.pin_world(0).x - red.x - px).abs() < 1e-6);
    let red = t.pin_world(0);
    let px = h.cam.pixel_size_at(red);
    with(&mut h, &mut t, |t, cx| t.own_key(cx, egui::Key::ArrowUp, Modifiers::SHIFT));
    assert!((t.pin_world(0).y - red.y - 10.0 * px).abs() < 1e-6);
    // Flip Left/Right about the red pin, then the menu's Undo puts it back.
    let p1 = t.pin_world(1);
    let red = t.pin_world(0);
    with(&mut h, &mut t, |t, cx| t.flip(cx.doc, true));
    assert!((t.pin_world(1).x - (2.0 * red.x - p1.x)).abs() < 1e-6);
    t.rotate(&mut h.doc, 90.0);
    // Shift alone: free pins; dragging one moves only it.
    with(&mut h, &mut t, |t, cx| t.shift_tap(cx));
    assert!(!t.fixed);
    let others: Vec<DVec3> = (0..3).map(|i| t.pin_world(i)).collect();
    let y = t.pin_world(3);
    drag(&mut h, &mut t, y, y + DVec3::new(40.0, 40.0, 0.0));
    for (i, p) in others.iter().enumerate() {
        assert!(t.pin_world(i).distance(*p) < 1e-6, "punaise {i} immobile");
    }
    // Escape: nothing is kept.
    let label = h.doc.undo_label().map(str::to_string);
    with(&mut h, &mut t, |t, cx| t.escape(cx));
    assert_eq!(t.switch_to(), Some(ToolId::Select));
    assert!(h.doc.preview.is_none());
    assert_eq!(h.doc.undo_label().map(str::to_string), label);
    assert!(h.doc.ents().faces[f].tex.is_none());
}

#[test]
fn a_click_outside_the_texture_finishes() {
    let mut h = Harness::new();
    let (f, _) = textured_square(&mut h);
    let mut t = start(&h, f);
    let from = t.pin_world(0);
    drag(&mut h, &mut t, from, DVec3::new(700.0, 380.0, 0.0));
    // Far from the face and its tiles.
    press(&mut h, &mut t, DVec3::new(2800.0, 300.0, 0.0));
    assert_eq!(t.switch_to(), Some(ToolId::Select));
    assert_eq!(h.doc.undo_label(), Some("Positionner la texture"));
}

#[test]
fn reset_and_projected_texture_commands() {
    let mut h = Harness::new();
    let (f, _) = textured_square(&mut h);
    h.doc.select(vec![EntityRef::Face(f)]);
    let mut t = start(&h, f);
    let from = t.pin_world(0);
    drag(&mut h, &mut t, from, DVec3::new(560.0, 510.0, 0.0));
    with(&mut h, &mut t, |t, cx| t.cancel(cx));
    assert!(enabled_in(&h.doc, &h.cam, &h.style, TexCmd::Reset));
    apply(&mut h.doc, &h.cam, &h.style, TexCmd::Reset);
    assert_eq!(h.doc.undo_label(), Some("Réinitialiser la position de la texture"));
    assert!(h.doc.ents().faces[f].tex.is_none(), "les deux faces partagent la matière");
    // Projected: ticked, then off again. The side becomes positioned too, its texture where it
    // was (TestUp `test_texture_projected_Query_mapped_by_ui`), and stays so.
    assert_eq!(checked_in(&h.doc, &h.cam, &h.style, TexCmd::Project), Some(false));
    let p = DVec3::new(130.0, 820.0, 0.0);
    let shown = esq_core::meta::default_uv(DVec3::Z, [500.0, 500.0]).at(p);
    apply(&mut h.doc, &h.cam, &h.style, TexCmd::Project);
    assert_eq!(h.doc.undo_label(), Some("Projeter la texture"));
    assert_eq!(checked_in(&h.doc, &h.cam, &h.style, TexCmd::Project), Some(true));
    assert!(close(uv(&h, f, p), shown), "{:?} {shown:?}", uv(&h, f, p));
    assert!(enabled_in(&h.doc, &h.cam, &h.style, TexCmd::Position), "projetée selon sa normale");
    apply(&mut h.doc, &h.cam, &h.style, TexCmd::Project);
    assert_eq!(checked_in(&h.doc, &h.cam, &h.style, TexCmd::Project), Some(false));
    assert!(close(uv(&h, f, p), shown));
    // Without textures on screen, nothing is offered.
    let shaded = crate::style::ViewStyle { faces: crate::style::FaceStyle::Shaded, ..h.style.clone() };
    assert!(!enabled_in(&h.doc, &h.cam, &shaded, TexCmd::Position));
}

#[test]
fn sample_material_then_paint_continues_or_projects_the_texture() {
    let mut h = Harness::new();
    let (f, mat) = textured_square(&mut h);
    let mut t = start(&h, f);
    let from = t.pin_world(0);
    drag(&mut h, &mut t, from, DVec3::new(570.0, 530.0, 0.0));
    with(&mut h, &mut t, |t, cx| t.cancel(cx));
    // A second square beside it, same plane, and a slope; nothing painted.
    let mut others = (None, None);
    h.doc.commit("faces", |m, _| {
        let add = |m: &mut esq_core::model::Model, pts: [(f64, f64, f64); 4]| {
            let v = pts.map(|(x, y, z)| m.root.add_vertex_raw(DVec3::new(x, y, z)));
            m.root.add_face_raw(v.to_vec(), Vec::new()).unwrap()
        };
        others.0 = Some(add(m, [(1500.0, 0.0, 0.0), (2500.0, 0.0, 0.0), (2500.0, 1000.0, 0.0), (1500.0, 1000.0, 0.0)]));
        others.1 =
            Some(add(m, [(0.0, 1500.0, 0.0), (1000.0, 1500.0, 0.0), (1000.0, 2500.0, 600.0), (0.0, 2500.0, 600.0)]));
    });
    let (g, slope) = (others.0.unwrap(), others.1.unwrap());
    // Tools › Sample Material: a click loads the material, then the Paint Bucket takes over.
    h.material = None;
    let mut s = ToolId::SampleMaterial.make();
    let at = h.px(DVec3::new(300.0, 300.0, 0.0));
    with(&mut h, &mut *s, |s, cx| {
        s.mouse_move(cx, at);
        s.press(cx, at);
    });
    assert_eq!(h.material, Some(mat));
    assert_eq!(s.switch_to(), Some(ToolId::Paint));
    let mut paint = ToolId::Paint.make();
    for p in [DVec3::new(2000.0, 500.0, 0.0), DVec3::new(500.0, 2000.0, 300.0)] {
        let m = h.px(p);
        with(&mut h, &mut *paint, |t, cx| {
            t.mouse_move(cx, m);
            t.press(cx, m);
            t.release(cx, m);
        });
    }
    // Same plane: the texture goes on where the sampled face left it.
    let q = DVec3::new(1600.0, 530.0, 0.0);
    let src = h.doc.ents().faces[f].tex.as_ref().unwrap().front.unwrap();
    assert!(close(uv(&h, g, q), src.at(q)));
    // Not the same plane, not projected: left as it is.
    assert!(h.doc.ents().faces[slope].tex.is_none());
    assert_eq!(h.doc.ents().faces[slope].front, Some(mat));
    // Projected: the slope takes the texture of its shadow on the ground.
    h.doc.select(vec![EntityRef::Face(f)]);
    apply(&mut h.doc, &h.cam, &h.style, TexCmd::Project);
    let at = h.px(DVec3::new(300.0, 300.0, 0.0));
    with(&mut h, &mut *paint, |t, cx| {
        cx.mods = Modifiers::ALT;
        t.mouse_move(cx, at);
        t.press(cx, at);
    });
    let m = h.px(DVec3::new(500.0, 2000.0, 300.0));
    with(&mut h, &mut *paint, |t, cx| {
        t.mouse_move(cx, m);
        t.press(cx, m);
        t.release(cx, m);
    });
    let p = DVec3::new(200.0, 2000.0, 300.0);
    assert!(close(uv(&h, slope, p), src.at(DVec3::new(200.0, 2000.0, 0.0))), "{:?}", uv(&h, slope, p));
    assert!(h.doc.ents().faces[slope].tex.as_ref().unwrap().projection[0].is_some());
}

#[test]
fn make_unique_texture_flattens_a_distorted_texture() {
    let mut h = Harness::new();
    let (f, mat) = textured_square(&mut h);
    // A 64-pixel picture: red, green on top; blue, white below.
    let n = 64u32;
    let mut img = image::RgbaImage::new(n, n);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgba(match (x < n / 2, y < n / 2) {
            (true, true) => [255, 0, 0, 255],
            (false, true) => [0, 255, 0, 255],
            (true, false) => [0, 0, 255, 255],
            (false, false) => [255, 255, 255, 255],
        });
    }
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    h.doc.model.materials[mat].ext.texture = Some(esq_core::meta::TextureData { px: [n, n], ..texture(png, 500.0) });
    let mut t = start(&h, f);
    let y = t.pin_world(3);
    drag(&mut h, &mut t, y, y + DVec3::new(200.0, 150.0, 0.0));
    with(&mut h, &mut t, |t, cx| t.cancel(cx));
    let before = h.doc.ents().faces[f].tex.as_ref().unwrap().front.unwrap();
    h.doc.select(vec![EntityRef::Face(f)]);
    apply(&mut h.doc, &h.cam, &h.style, TexCmd::MakeUnique);
    assert_eq!(h.doc.undo_label(), Some("Rendre la texture unique"));
    let face = &h.doc.ents().faces[f];
    let new = face.front.unwrap();
    assert_ne!(new, mat);
    assert_eq!(face.back, Some(new), "les deux faces avaient la matière");
    let m = &h.doc.model.materials[new];
    assert_eq!(m.name, "Damier1");
    let tex = m.ext.texture.as_ref().unwrap();
    assert_eq!(tex.size, [1000.0, 1000.0]);
    let map = face.tex.as_ref().unwrap().front.unwrap();
    assert!(map.is_affine());
    assert!(close(map.at(DVec3::ZERO), DVec2::ZERO) && close(map.at(DVec3::new(1000.0, 1000.0, 0.0)), DVec2::ONE));
    // The new image shows what the face showed through the distorted map.
    let px = crate::textures::get(&tex.image, None).unwrap();
    assert!(px.w > 100 && px.h > 100, "{}×{}", px.w, px.h);
    let mut checked = 0;
    for i in 1..20 {
        for j in 1..20 {
            let p = DVec3::new(i as f64 * 50.0, j as f64 * 50.0, 0.0);
            let a = before.at(p);
            let (u, v) = (a.x.rem_euclid(1.0), a.y.rem_euclid(1.0));
            // Away from the quarters' borders.
            if [u, v].iter().any(|c| (c - 0.5).abs() < 0.08 || *c < 0.08 || *c > 0.92) {
                continue;
            }
            let want: [u8; 3] = match (u < 0.5, v > 0.5) {
                (true, true) => [255, 0, 0],
                (false, true) => [0, 255, 0],
                (true, false) => [0, 0, 255],
                (false, false) => [255, 255, 255],
            };
            let x = ((p.x / 1000.0) * px.w as f64) as usize;
            let y = ((1.0 - p.y / 1000.0) * px.h as f64) as usize;
            let o = (y.min(px.h as usize - 1) * px.w as usize + x.min(px.w as usize - 1)) * 4;
            let got = &px.rgba[o..o + 3];
            assert!(got.iter().zip(want).all(|(g, w)| (*g as i32 - w as i32).abs() < 40), "{p:?} : {got:?} / {want:?}");
            checked += 1;
        }
    }
    assert!(checked > 100, "{checked}");
}

/// Writes `textures.esq` in `ESQ_DEMO_DIR` for a look on screen: a box whose top and front
/// are painted with a photograph as texture.
#[test]
#[ignore]
fn write_texture_demo() {
    let Ok(dir) = std::env::var("ESQ_DEMO_DIR") else { return };
    let bytes = std::fs::read(r"C:\Windows\Web\Wallpaper\Spotlight\img14.jpg").expect("photo");
    let px = crate::textures::get(&esq_core::photo::Blob(std::sync::Arc::new(bytes.clone())), None).unwrap();
    let mut m = esq_core::model::Model::new();
    let mat = m.add_material("Photo", [128, 128, 128]);
    m.materials[mat].ext.texture = Some(esq_core::meta::TextureData {
        filename: "img14.jpg".into(),
        image: esq_core::photo::Blob(std::sync::Arc::new(bytes)),
        px: [px.w, px.h],
        size: [800.0, 800.0 * px.h as f64 / px.w as f64],
        avg: [120, 120, 120],
    });
    let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
    let quads = [
        [p(0.0, 0.0, 1000.0), p(2000.0, 0.0, 1000.0), p(2000.0, 1500.0, 1000.0), p(0.0, 1500.0, 1000.0)],
        [p(0.0, 0.0, 0.0), p(2000.0, 0.0, 0.0), p(2000.0, 0.0, 1000.0), p(0.0, 0.0, 1000.0)],
        [p(2000.0, 0.0, 0.0), p(2000.0, 1500.0, 0.0), p(2000.0, 1500.0, 1000.0), p(2000.0, 0.0, 1000.0)],
    ];
    for q in quads {
        let v = q.map(|x| m.root.add_vertex_raw(x));
        let f = m.root.add_face_raw(v.to_vec(), Vec::new()).unwrap();
        m.root.faces[f].front = Some(mat);
    }
    esq_core::io::save_esq(&m, &std::path::Path::new(&dir).join("textures.esq")).unwrap();
    // The same, positioned: the front turned and enlarged by its green pin, the side
    // distorted by its yellow pin, as Position Texture leaves them.
    use esq_core::texpos::{Frame, drag_fixed, map_from_pins, plane_matrix, tile_pins};
    let tile = m.materials[mat].ext.texture.as_ref().unwrap().size;
    let faces: Vec<FaceId> = m.root.faces.keys().collect();
    for (f, (pin, to)) in faces.into_iter().zip([(1usize, (0.0, 0.0)), (1, (1100.0, 700.0)), (3, (1600.0, 900.0))]) {
        let face = &m.root.faces[f];
        let frame = Frame::new(m.root.pos(face.outer[0]), face.normal);
        let map = esq_core::meta::default_uv(face.normal, tile);
        let g = plane_matrix(&map, &frame, tile);
        let c = face.outer.iter().map(|v| frame.to2(m.root.pos(*v))).fold(DVec2::ZERO, |a, p| a + p) / 4.0;
        let mut pins = tile_pins(&g, tile, c).unwrap();
        if to != (0.0, 0.0) {
            let target = frame.to2(DVec3::new(to.0, if pin == 1 { 0.0 } else { to.0 - 1600.0 + 300.0 }, to.1));
            let target = if pin == 3 { DVec2::new(pins[3].at.x + 250.0, pins[3].at.y + 200.0) } else { target };
            drag_fixed(&mut pins, pin, target);
        }
        let map = map_from_pins(&pins, &frame, tile).unwrap();
        m.root.faces[f].tex = Some(Box::new(esq_core::meta::FaceTex { front: Some(map), ..Default::default() }));
    }
    esq_core::io::save_esq(&m, &std::path::Path::new(&dir).join("textures_positionnees.esq")).unwrap();
}

#[test]
fn pins_start_on_the_tile_under_the_right_click() {
    let mut h = Harness::new();
    let (f, _) = textured_square(&mut h);
    let at = h.px(DVec3::new(700.0, 200.0, 0.0));
    let t = PositionTextureTool::start(&h.doc, &h.cam, f, Some(at), ToolId::Select).unwrap();
    assert!(t.pin_world(0).distance(DVec3::new(500.0, 0.0, 0.0)) < 1e-6, "{:?}", t.pin_world(0));
    assert!(t.pin_world(3).distance(DVec3::new(1000.0, 500.0, 0.0)) < 1e-6, "{:?}", t.pin_world(3));
}

#[test]
fn match_color_of_object_in_model() {
    let mut h = Harness::new();
    let (f, mat) = textured_square(&mut h);
    h.doc.model.materials[mat].color = [10, 120, 200];
    let other = h.doc.model.add_material("Autre", [0, 0, 0]);
    h.doc.model.materials[other].ext.texture = Some(texture(quad_png(), 300.0));
    let mut t = crate::tools::sample::SampleTool::for_color(other, ToolId::Move);
    let at = DVec3::new(400.0, 300.0, 0.0);
    press(&mut h, &mut t, at);
    assert_eq!(t.switch_to(), Some(ToolId::Move));
    let m = &h.doc.model.materials[other];
    assert_eq!(m.color, [10, 120, 200]);
    assert!(m.ext.colorize_deltas != [0.0; 3], "la texture prend la couleur");
    assert_eq!(h.doc.undo_label(), Some("Modifier la matière"));
    let _ = f;
}
