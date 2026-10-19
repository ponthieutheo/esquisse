//! What the 3D view shows: rendered on the GPU and read back (see `gpu.rs`).

use std::io::Cursor;
use std::sync::Arc;

use esq_core::glam::{DVec2, DVec3};
use esq_core::meta::TextureData;
use esq_core::model::{MaterialId, Model};
use esq_core::photo::Blob;

use super::gpu::{Gpu, top_camera};
use crate::render::Lighting;
use crate::style::{FaceStyle, ViewStyle};

/// A PNG made of 2×2 blocks: red, green on top; blue, white below.
pub fn quad_png() -> Vec<u8> {
    let mut img = image::RgbaImage::new(2, 2);
    img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
    img.put_pixel(1, 0, image::Rgba([0, 255, 0, 255]));
    img.put_pixel(0, 1, image::Rgba([0, 0, 255, 255]));
    img.put_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

pub fn texture(bytes: Vec<u8>, size_mm: f64) -> TextureData {
    TextureData {
        filename: "quad.png".into(),
        image: Blob(Arc::new(bytes)),
        px: [2, 2],
        size: [size_mm, size_mm],
        avg: [128, 128, 128],
    }
}

/// A 1000 mm square on the ground, its top painted with `mat`.
fn square(m: &mut Model, mat: Option<MaterialId>) {
    let v = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)]
        .map(|(x, y)| m.root.add_vertex_raw(DVec3::new(x, y, 0.0)));
    let f = m.root.add_face_raw(v.to_vec(), Vec::new()).unwrap();
    m.root.faces[f].front = mat;
}

fn dominant(c: [u8; 3]) -> usize {
    (0..3).max_by_key(|&i| c[i]).unwrap()
}

fn unlit() -> Lighting {
    Lighting::default()
}

#[test]
fn textured_faces_show_their_texture_and_shaded_faces_its_colour() {
    let Some(mut gpu) = Gpu::new() else { return eprintln!("pas de GPU : test sauté") };
    let mut m = Model::new();
    let mat = m.add_material("Damier", [128, 128, 128]);
    m.materials[mat].ext.texture = Some(texture(quad_png(), 1000.0));
    square(&mut m, Some(mat));
    let cam = top_camera(1000.0, 200.0);
    let style = ViewStyle { faces: FaceStyle::ShadedWithTextures, axes: false, edges: false, ..Default::default() };
    let shot = gpu.render(&m, &style, &cam, &unlit());
    // One tile covers the square: the picture's top left (red) is at the square's far left.
    let at = |x: f64, y: f64| shot.at_point(&cam, DVec3::new(x, y, 0.0));
    assert_eq!(dominant(at(250.0, 750.0)), 0, "rouge en haut à gauche : {:?}", at(250.0, 750.0));
    assert_eq!(dominant(at(750.0, 750.0)), 1, "vert en haut à droite : {:?}", at(750.0, 750.0));
    assert_eq!(dominant(at(250.0, 250.0)), 2, "bleu en bas à gauche : {:?}", at(250.0, 250.0));
    let w = at(750.0, 250.0);
    assert!(w.iter().all(|c| *c > 150), "blanc en bas à droite : {w:?}");
    // Tiles repeat: a texture half the size shows the picture four times.
    m.materials[mat].ext.texture = Some(texture(quad_png(), 500.0));
    let shot = gpu.render(&m, &style, &cam, &unlit());
    let at = |x: f64, y: f64| shot.at_point(&cam, DVec3::new(x, y, 0.0));
    assert_eq!(dominant(at(125.0, 375.0)), 0);
    assert_eq!(dominant(at(625.0, 875.0)), 0);
    // Shaded (no textures): the material's colour, one colour over the whole face.
    let shaded = ViewStyle { faces: FaceStyle::Shaded, ..style.clone() };
    let shot = gpu.render(&m, &shaded, &cam, &unlit());
    let a = shot.at_point(&cam, DVec3::new(125.0, 375.0, 0.0));
    let b = shot.at_point(&cam, DVec3::new(375.0, 375.0, 0.0));
    assert_eq!(a, b, "une seule couleur");
    assert!((a[0] as i32 - a[2] as i32).abs() < 4, "gris : {a:?}");
}

#[test]
fn images_show_their_picture_in_every_shaded_style() {
    let Some(mut gpu) = Gpu::new() else { return eprintln!("pas de GPU : test sauté") };
    let mut m = Model::new();
    let t = texture(quad_png(), 0.0);
    m.add_image(None, "plan", "plan.png", t, DVec3::ZERO, DVec3::X, DVec3::Z, 1000.0, None).unwrap();
    let cam = top_camera(1000.0, 200.0);
    for faces in [FaceStyle::ShadedWithTextures, FaceStyle::Shaded, FaceStyle::Monochrome, FaceStyle::HiddenLine] {
        let style = ViewStyle { faces, axes: false, ..Default::default() };
        let shot = gpu.render(&m, &style, &cam, &unlit());
        let p = shot.at_point(&cam, DVec3::new(250.0, 750.0, 0.0));
        assert_eq!(dominant(p), 0, "{faces:?} : {p:?}");
        let p = shot.at_point(&cam, DVec3::new(250.0, 250.0, 0.0));
        assert_eq!(dominant(p), 2, "{faces:?} : {p:?}");
    }
    // An image has no outline: the middle of its left side is the picture's colour.
    let style = ViewStyle { faces: FaceStyle::ShadedWithTextures, axes: false, ..Default::default() };
    let shot = gpu.render(&m, &style, &cam, &unlit());
    let p = shot.at_point(&cam, DVec3::new(1.0, 750.0, 0.0));
    assert!(p[0] > 150, "pas d'arête noire : {p:?}");
}

/// A texture made from an image file, as an import would make it.
fn texture_of_file(path: &str, tile_mm: f64) -> Option<TextureData> {
    let bytes = std::fs::read(path).ok()?;
    let t = crate::textures::get(&Blob(Arc::new(bytes.clone())), None)?;
    let n = (t.rgba.len() / 4) as u64;
    let avg: [u8; 3] =
        std::array::from_fn(|c| (t.rgba.as_chunks::<4>().0.iter().map(|p| p[c] as u64).sum::<u64>() / n) as u8);
    let filename = std::path::Path::new(path).file_name()?.to_string_lossy().into_owned();
    Some(TextureData { filename, image: Blob(Arc::new(bytes)), px: [t.w, t.h], size: [tile_mm, tile_mm], avg })
}

/// Writes `textures.esq` in `ESQ_DEMO_DIR` for a look on screen: a photograph standing as an
/// image, and a box painted with a textured material.
#[test]
#[ignore]
fn write_demo_files() {
    let Ok(dir) = std::env::var("ESQ_DEMO_DIR") else { return };
    let mut m = Model::new();
    let photo = r"C:\Windows\Web\Wallpaper\ThemeA\img20.jpg";
    let tex = texture_of_file(photo, 0.0).expect("photo");
    m.add_image(None, "img20", photo, tex, DVec3::new(-3000.0, 1500.0, 0.0), DVec3::X, -DVec3::Y, 2800.0, None);
    let mat = m.add_material("Photo", [128, 128, 128]);
    let tex = texture_of_file(r"C:\Windows\Web\Wallpaper\Spotlight\img14.jpg", 600.0).expect("texture");
    m.materials[mat].color = tex.avg;
    m.materials[mat].ext.texture = Some(tex);
    let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
    let v: Vec<_> = [
        p(0.0, 0.0, 0.0),
        p(1200.0, 0.0, 0.0),
        p(1200.0, 900.0, 0.0),
        p(0.0, 900.0, 0.0),
        p(0.0, 0.0, 1000.0),
        p(1200.0, 0.0, 1000.0),
        p(1200.0, 900.0, 1000.0),
        p(0.0, 900.0, 1000.0),
    ]
    .into_iter()
    .map(|q| m.root.add_vertex_raw(q))
    .collect();
    for l in [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]] {
        let f = m.root.add_face_raw(l.iter().map(|&i| v[i]).collect(), Vec::new()).unwrap();
        m.root.faces[f].front = Some(mat);
    }
    esq_core::io::save_esq(&m, &std::path::Path::new(&dir).join("textures.esq")).unwrap();
}

/// Where `one_edge` lies: on a pixel row of `top_camera(1000.0, 400.0)` (2.75 mm a pixel).
const EDGE_Y: f64 = 500.0 + 2.75 / 2.0;

/// A model with one edge along red, from x = 100 to x = 900 at y = `EDGE_Y`.
fn one_edge() -> Model {
    let mut m = Model::new();
    let a = m.root.add_vertex_raw(DVec3::new(100.0, EDGE_Y, 0.0));
    let b = m.root.add_vertex_raw(DVec3::new(900.0, EDGE_Y, 0.0));
    m.root.add_edge_raw(a, b);
    m
}

fn lines_only() -> ViewStyle {
    ViewStyle { axes: false, sky: false, ..Default::default() }
}

fn is_dark(c: [u8; 3]) -> bool {
    (c[0] as u32 + c[1] as u32 + c[2] as u32) < 300
}

/// Dark pixels in column `x`, rows `y0..y1`.
fn column_ink(s: &super::gpu::Shot, x: u32, y0: u32, y1: u32) -> usize {
    (y0..y1).filter(|&y| is_dark(s.at(x, y))).count()
}

fn ink(s: &super::gpu::Shot) -> usize {
    s.px.as_chunks::<4>().0.iter().filter(|p| is_dark([p[0], p[1], p[2]])).count()
}

#[test]
fn edge_style_options_change_how_edges_are_drawn() {
    let Some(mut gpu) = Gpu::new() else { return eprintln!("pas de GPU : test sauté") };
    let m = one_edge();
    let cam = top_camera(1000.0, 400.0);
    let p = |x: f64| cam.project(DVec3::new(x, EDGE_Y, 0.0)).unwrap();
    let (ya, yb) = (p(100.0).y as u32 - 8, p(100.0).y as u32 + 8);
    let plain = gpu.render(&m, &lines_only(), &cam, &unlit());
    let mid = p(500.0).x as u32;
    let near_end = p(100.0).x as u32 + 3;
    assert!(column_ink(&plain, mid, ya, yb) <= 2, "un trait fin");
    assert_eq!(column_ink(&plain, near_end, ya, yb), column_ink(&plain, mid, ya, yb));
    // Endpoints: thicker over the last pixels of each end, not in the middle.
    let ends = gpu.render(&m, &ViewStyle { endpoints: true, ..lines_only() }, &cam, &unlit());
    assert!(column_ink(&ends, near_end, ya, yb) >= 3, "{}", column_ink(&ends, near_end, ya, yb));
    assert_eq!(column_ink(&ends, mid, ya, yb), column_ink(&plain, mid, ya, yb));
    // Extension: the edge runs past its end.
    let past = p(100.0).x as u32 - 2;
    assert_eq!(column_ink(&plain, past, ya, yb), 0);
    let ext = gpu.render(&m, &ViewStyle { extension: true, ..lines_only() }, &cam, &unlit());
    assert!(column_ink(&ext, past, ya, yb) > 0);
    // Jitter: more strokes, more ink.
    let jit = gpu.render(&m, &ViewStyle { jitter: true, ..lines_only() }, &cam, &unlit());
    assert!(ink(&jit) > ink(&plain) * 3 / 2, "{} {}", ink(&jit), ink(&plain));
    // Dashes: the tag's dash pattern leaves gaps along the edge, unless dashes are off.
    let mut dashed = one_edge();
    let tag = dashed.add_layer("Pointillé");
    dashed.layers[tag].line_style = "Dotted Basic".into();
    for e in dashed.root.edges.values_mut() {
        e.layer = tag;
    }
    let y = p(500.0).y as u32;
    let gaps = |s: &super::gpu::Shot| {
        (p(150.0).x as u32..p(850.0).x as u32).filter(|&x| column_ink(s, x, y - 2, y + 2) == 0).count()
    };
    assert_eq!(gaps(&plain), 0);
    let d = gpu.render(&dashed, &lines_only(), &cam, &unlit());
    assert!(gaps(&d) > 50, "{}", gaps(&d));
    let solid = gpu.render(&dashed, &ViewStyle { dashes: false, ..lines_only() }, &cam, &unlit());
    assert_eq!(gaps(&solid), 0);
}

#[test]
fn edges_take_one_colour_their_material_or_their_axis() {
    use crate::style::EdgeColor;
    let Some(mut gpu) = Gpu::new() else { return eprintln!("pas de GPU : test sauté") };
    let mut m = one_edge();
    let cam = top_camera(1000.0, 400.0);
    let at = |s: &super::gpu::Shot| {
        let q = cam.project(DVec3::new(500.0, EDGE_Y, 0.0)).unwrap();
        (q.y as u32 - 2..q.y as u32 + 2).map(|y| s.at(q.x as u32, y)).min_by_key(|c| c[1] as u32 + c[2] as u32).unwrap()
    };
    let style = |mode| ViewStyle { edge_color_mode: mode, edge_color: [0, 0, 0], ..lines_only() };
    let black = at(&gpu.render(&m, &style(EdgeColor::AllSame), &cam, &unlit()));
    assert!(black.iter().all(|c| *c < 90), "{black:?}");
    let red = at(&gpu.render(&m, &style(EdgeColor::ByAxis), &cam, &unlit()));
    assert!(red[0] > 150 && red[1] < 120, "{red:?}");
    let mat = m.add_material("Vert", [0, 200, 0]);
    let e = m.root.edges.keys().next().unwrap();
    let key = format!("e{}", slotmap::Key::data(&e).as_ffi());
    m.root.set_prop(&key, "material", esq_core::model::AttrValue::Text(m.materials[mat].name.clone()));
    let green = at(&gpu.render(&m, &style(EdgeColor::ByMaterial), &cam, &unlit()));
    assert!(green[1] > 120 && green[0] < 100, "{green:?}");
}

#[test]
fn background_ground_fog_colour_by_tag_and_transparency() {
    let Some(mut gpu) = Gpu::new() else { return eprintln!("pas de GPU : test sauté") };
    let cam = top_camera(1000.0, 200.0);
    let empty = Model::new();
    // The background colour of the style; the ground over it, half transparent.
    let bg = ViewStyle { bg_color: [200, 40, 40], ..lines_only() };
    assert_eq!(gpu.render(&empty, &bg, &cam, &unlit()).at(5, 5), [200, 40, 40]);
    let g = ViewStyle { ground: true, ground_color: [40, 40, 200], ground_transparency: 0, ..bg.clone() };
    assert_eq!(gpu.render(&empty, &g, &cam, &unlit()).at(5, 5), [40, 40, 200]);
    let half = gpu.render(&empty, &ViewStyle { ground_transparency: 50, ..g.clone() }, &cam, &unlit()).at(5, 5);
    assert!(half[0] > 60 && half[2] > 60, "{half:?}");
    // Seen from below, the ground shows only when asked to.
    let mut below = cam.clone();
    below.set_view(-90.0, -89.9);
    let mut under = |show| gpu_pixel(&mut gpu, &empty, &ViewStyle { underground: show, ..g.clone() }, &below);
    assert_eq!(under(true), [40, 40, 200]);
    assert_eq!(under(false), [200, 40, 40]);
    // The sky, looking at the horizon.
    let mut level = crate::camera::Camera { size: DVec2::new(200.0, 200.0), ..Default::default() };
    level.set_view(-90.0, 0.0);
    let sky = ViewStyle { sky: true, sky_color: [0, 154, 217], ..lines_only() };
    let top = gpu_pixel(&mut gpu, &empty, &sky, &level);
    assert!(top[2] > top[0] + 80, "{top:?}");
    // Fog: a face beyond the fog's end takes the fog colour.
    let mut m = Model::new();
    let red = m.add_material("Rouge", [255, 0, 0]);
    square(&mut m, Some(red));
    let fog = ViewStyle {
        fog: true,
        fog_start: 0.0,
        fog_end: 10.0,
        fog_use_bg: false,
        fog_color: [0, 255, 0],
        ..lines_only()
    };
    let p = gpu.render(&m, &fog, &cam, &unlit()).at_point(&cam, DVec3::new(500.0, 500.0, 0.0));
    assert!(p[1] > 230 && p[0] < 20, "{p:?}");
    // Unset distances: no fog.
    let unset = ViewStyle { fog_start: -1.0, fog_end: -1.0, ..fog.clone() };
    let p = gpu.render(&m, &unset, &cam, &unlit()).at_point(&cam, DVec3::new(500.0, 500.0, 0.0));
    assert!(p[0] > 150 && p[1] < 60, "{p:?}");
    // Colour by tag: the face shows its tag's colour, not its material's.
    let t = m.add_layer("Bleu");
    m.layers[t].color = Some([0, 0, 255]);
    for f in m.root.faces.values_mut() {
        f.layer = t;
    }
    let by_tag = ViewStyle { color_by_tag: true, ..lines_only() };
    let p = gpu.render(&m, &by_tag, &cam, &unlit()).at_point(&cam, DVec3::new(500.0, 500.0, 0.0));
    assert!(p[2] > 150 && p[0] < 60, "{p:?}");
    // Transparency off: a see-through material is drawn opaque.
    m.materials[red].opacity = 0.2;
    for f in m.root.faces.values_mut() {
        f.layer = 0;
    }
    let see = gpu.render(&m, &bg, &cam, &unlit()).at_point(&cam, DVec3::new(500.0, 500.0, 0.0));
    let opaque = gpu
        .render(&m, &ViewStyle { transparency: false, bg_color: [0, 0, 255], ..lines_only() }, &cam, &unlit())
        .at_point(&cam, DVec3::new(500.0, 500.0, 0.0));
    assert!(see[2] > 20, "transparent : {see:?}");
    assert!(opaque[0] > 150 && opaque[2] < 60, "opaque : {opaque:?}");
}

fn gpu_pixel(gpu: &mut Gpu, m: &Model, st: &ViewStyle, cam: &crate::camera::Camera) -> [u8; 3] {
    gpu.render(m, st, cam, &unlit()).at(5, 5)
}

#[test]
fn display_commands_toggle_the_style_and_fog_starts_on_the_model() {
    use crate::commands::Cmd;
    use crate::style::EdgeColor;
    use crate::view_cmds::ViewCmd;
    let mut app = crate::app::EsquisseApp::bare(None, None);
    let ctx = eframe::egui::Context::default();
    for (c, get) in [
        (ViewCmd::Dashes, (|s: &ViewStyle| s.dashes) as fn(&ViewStyle) -> bool),
        (ViewCmd::Endpoints, |s| s.endpoints),
        (ViewCmd::Jitter, |s| s.jitter),
        (ViewCmd::Ground, |s| s.ground),
        (ViewCmd::Sky, |s| s.sky),
        (ViewCmd::TransparentMaterials, |s| s.transparency),
        (ViewCmd::Watermarks, |s| s.watermarks),
        (ViewCmd::ColorByTag, |s| s.color_by_tag),
    ] {
        let before = get(&app.style);
        assert_eq!(app.cmd_checked(Cmd::View(c)), Some(before));
        app.run(&ctx, Cmd::View(c));
        assert_eq!(get(&app.style), !before, "{c:?}");
    }
    assert_eq!(app.cmd_checked(Cmd::View(ViewCmd::ShowNewTags)), Some(true));
    app.run(&ctx, Cmd::View(ViewCmd::ShowNewTags));
    assert_eq!(app.cmd_checked(Cmd::View(ViewCmd::ShowNewTags)), Some(false));
    // Display Crosshairs: off by default, a preference (not a style setting).
    assert_eq!(app.cmd_checked(Cmd::View(ViewCmd::Crosshairs)), Some(false));
    app.run(&ctx, Cmd::View(ViewCmd::Crosshairs));
    assert!(crate::crosshairs::shown());
    app.run(&ctx, Cmd::View(ViewCmd::Crosshairs));
    assert!(!crate::crosshairs::shown());
    // View › Grid is a rendering option; grid snapping a setting of the model's grids.
    assert_eq!(app.cmd_checked(Cmd::View(ViewCmd::Grid)), Some(false));
    app.run(&ctx, Cmd::View(ViewCmd::Grid));
    assert!(app.style.modeling_grid && app.style.ro_get("ModelingGrid") == Some(serde_json::json!(true)));
    app.run(&ctx, Cmd::View(ViewCmd::GridSnapping));
    assert!(crate::model_grid::settings(&app.doc.model).snap);
    assert_eq!(app.cmd_checked(Cmd::View(ViewCmd::GridSnapping)), Some(true));
    app.run(&ctx, Cmd::View(ViewCmd::EdgeColor(EdgeColor::ByAxis)));
    assert_eq!(app.style.edge_color_mode, EdgeColor::ByAxis);
    assert_eq!(app.cmd_checked(Cmd::View(ViewCmd::EdgeColor(EdgeColor::AllSame))), Some(false));
    let sun = app.doc.model.shadow.use_sun_for_shading;
    app.run(&ctx, Cmd::View(ViewCmd::UseSunForShading));
    assert_eq!(app.doc.model.shadow.use_sun_for_shading, !sun);
    app.doc.undo();
    assert_eq!(app.doc.model.shadow.use_sun_for_shading, sun, "undoable");
    // Fog shown for the first time spans the model as seen from the camera.
    square(&mut app.doc.model, None);
    app.run(&ctx, Cmd::Fog);
    let (near, far) = crate::fog::model_range(&app.doc.model, &app.cam);
    assert!(app.style.fog && (app.style.fog_start - near).abs() < 1e-6 && (app.style.fog_end - far).abs() < 1e-6);
    assert!(near < far);
    // The commands are found by the command search.
    let paths = crate::menus::command_paths();
    assert!(paths.iter().any(|(p, c)| *c == Cmd::View(ViewCmd::Jitter) && p.contains("lignes floues")));
}

#[test]
fn transparent_texels_let_the_background_through() {
    let Some(mut gpu) = Gpu::new() else { return eprintln!("pas de GPU : test sauté") };
    let mut img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
    img.put_pixel(0, 1, image::Rgba([255, 0, 0, 0]));
    img.put_pixel(1, 1, image::Rgba([255, 0, 0, 0]));
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let mut m = Model::new();
    let mat = m.add_material("Feuille", [255, 0, 0]);
    m.materials[mat].ext.texture = Some(texture(png, 1000.0));
    square(&mut m, Some(mat));
    let cam = top_camera(1000.0, 200.0);
    let style = ViewStyle { faces: FaceStyle::ShadedWithTextures, axes: false, edges: false, ..Default::default() };
    let shot = gpu.render(&m, &style, &cam, &unlit());
    let top = shot.at_point(&cam, DVec3::new(500.0, 900.0, 0.0));
    let bottom = shot.at_point(&cam, DVec3::new(500.0, 100.0, 0.0));
    assert_eq!(dominant(top), 0);
    let bg = shot.at(2, 2);
    assert!(bottom.iter().zip(bg).all(|(a, b)| (*a as i32 - b as i32).abs() < 6), "{bottom:?} vs fond {bg:?}");
}

fn solid_png(c: [u8; 4], w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba(c));
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

#[test]
fn watermarks_are_drawn_behind_or_over_the_model() {
    use crate::style::WmLayout;
    let Some(mut gpu) = Gpu::new() else { return eprintln!("pas de GPU : test sauté") };
    let cam = top_camera(1000.0, 200.0);
    let mut m = Model::new();
    let green = m.add_material("Vert", [0, 200, 0]);
    square(&mut m, Some(green));
    let centre = DVec3::new(500.0, 500.0, 0.0);
    let red = crate::watermarks::new_watermark("Rouge", solid_png([255, 0, 0, 255], 4, 4));
    let opaque = |w: crate::style::Watermark| ViewStyle {
        watermark_list: vec![crate::style::Watermark {
            blend: 1.0,
            layout: WmLayout::Stretched,
            keep_aspect: false,
            ..w
        }],
        ..lines_only()
    };
    // Behind the model: the background is red, the face stays green.
    let shot = gpu.render(&m, &opaque(red.clone()), &cam, &unlit());
    assert_eq!(shot.at(2, 2), [255, 0, 0]);
    assert!(shot.at_point(&cam, centre)[1] > 150);
    // Over the model: red everywhere.
    let over = crate::style::Watermark { background: false, ..red.clone() };
    let shot = gpu.render(&m, &opaque(over.clone()), &cam, &unlit());
    assert_eq!(shot.at_point(&cam, centre), [255, 0, 0]);
    // Half blended over the face: between red and green.
    let half = ViewStyle {
        watermark_list: vec![crate::style::Watermark { blend: 0.5, ..opaque(over.clone()).watermark_list[0].clone() }],
        ..lines_only()
    };
    let p = gpu.render(&m, &half, &cam, &unlit()).at_point(&cam, centre);
    assert!(p[0] > 100 && p[1] > 60, "{p:?}");
    // Display watermarks off: nothing.
    let off = ViewStyle { watermarks: false, ..opaque(over.clone()) };
    assert!(gpu.render(&m, &off, &cam, &unlit()).at_point(&cam, centre)[1] > 150);
    // A mask: black shows the background colour, white lets the model through.
    let black = crate::style::Watermark {
        mask: true,
        background: false,
        ..crate::watermarks::new_watermark("Noir", solid_png([0, 0, 0, 255], 4, 4))
    };
    let mut st = opaque(black);
    st.bg_color = [0, 0, 255];
    assert_eq!(gpu.render(&m, &st, &cam, &unlit()).at_point(&cam, centre), [0, 0, 255]);
    let white = crate::style::Watermark {
        mask: true,
        background: false,
        ..crate::watermarks::new_watermark("Blanc", solid_png([255, 255, 255, 255], 4, 4))
    };
    let p = gpu.render(&m, &opaque(white), &cam, &unlit()).at_point(&cam, centre);
    assert!(p[1] > 150 && p[2] < 60, "{p:?}");
    // Positioned at the top left, small: only that corner is covered.
    let corner = crate::style::Watermark { layout: WmLayout::Positioned, position: 0, scale: 5.0, ..over };
    let st = ViewStyle { watermark_list: vec![crate::style::Watermark { blend: 1.0, ..corner }], ..lines_only() };
    let shot = gpu.render(&m, &st, &cam, &unlit());
    assert_eq!(shot.at(3, 3), [255, 0, 0]);
    assert_ne!(shot.at(190, 190), [255, 0, 0]);
}

/// A panorama red on its first half (u < 0.5: from the red axis' negative side through the
/// green axis to its positive side), blue on the other.
fn red_blue_env(skydome: bool, reflections: bool, rotation: f64) -> crate::environments::EnvView {
    let (w, h) = (64u32, 32u32);
    let px: Vec<[f32; 3]> =
        (0..w * h).map(|i| if (i % w) < w / 2 { [0.9, 0.05, 0.05] } else { [0.05, 0.05, 0.9] }).collect();
    crate::environments::EnvView {
        image: Arc::new(crate::environments::prepare(w, h, &px)),
        skydome,
        reflections,
        rotation,
        skydome_exposure: 1.0,
        reflection_exposure: 1.0,
    }
}

/// A camera at the origin looking level along `dir` (yaw in degrees, see `Camera::set_view`).
fn level_camera(yaw: f64) -> crate::camera::Camera {
    let mut cam = crate::camera::Camera {
        size: DVec2::new(120.0, 90.0),
        target: DVec3::new(0.0, 0.0, 1500.0),
        distance: 3000.0,
        ..Default::default()
    };
    cam.set_view(yaw, 0.0);
    cam
}

#[test]
fn the_environment_shows_as_a_skydome_and_in_photoreal_reflections() {
    let Some(mut gpu) = Gpu::new() else { return eprintln!("pas de GPU : test sauté") };
    let m = Model::new();
    let style = ViewStyle { axes: false, sky: false, ground: false, ..Default::default() };
    // Looking towards green: the red half of the panorama; towards −green: the blue half.
    let (to_green, to_minus_green) = (level_camera(-90.0), level_camera(90.0));
    assert!(to_green.forward().distance(DVec3::Y) < 1e-9);
    gpu.view.set_environment(&gpu.rs, Some(red_blue_env(true, true, 0.0)));
    let c = gpu.render(&m, &style, &to_green, &unlit()).at(60, 20);
    assert_eq!(dominant(c), 0, "rouge vers le vert : {c:?}");
    let c = gpu.render(&m, &style, &to_minus_green, &unlit()).at(60, 20);
    assert_eq!(dominant(c), 2, "bleu à l'opposé : {c:?}");
    // Turned half a turn, the halves swap.
    gpu.view.set_environment(&gpu.rs, Some(red_blue_env(true, true, 180.0)));
    let c = gpu.render(&m, &style, &to_green, &unlit()).at(60, 20);
    assert_eq!(dominant(c), 2, "tourné : {c:?}");
    // Not as a skydome: the style's background again.
    gpu.view.set_environment(&gpu.rs, Some(red_blue_env(false, true, 0.0)));
    let c = gpu.render(&m, &style, &to_green, &unlit()).at(60, 20);
    assert!(c[0].abs_diff(c[2]) < 60 && c[1] > 100, "arrière-plan du style : {c:?}");

    // A polished metal wall facing the camera mirrors what lies behind the camera.
    let mut m = Model::new();
    let mat = m.add_material("Chrome", [200, 200, 200]);
    let pbr = &mut m.materials[mat].ext.pbr;
    (pbr.metalness_enabled, pbr.metallic_factor, pbr.roughness_enabled, pbr.roughness_factor) = (true, 1.0, true, 0.05);
    let v = [(-5000.0, 0.0), (5000.0, 0.0), (5000.0, 4000.0), (-5000.0, 4000.0)]
        .map(|(x, z)| m.root.add_vertex_raw(DVec3::new(x, 2000.0, z)));
    let f = m.root.add_face_raw(v.to_vec(), Vec::new()).unwrap();
    let face = &mut m.root.faces[f];
    face.front = Some(mat);
    face.back = Some(mat);
    let photoreal = ViewStyle { axes: false, edges: false, photoreal: true, ..Default::default() };
    gpu.view.set_environment(&gpu.rs, Some(red_blue_env(true, true, 0.0)));
    let c = gpu.render(&m, &photoreal, &to_green, &unlit()).at(60, 45);
    assert_eq!(dominant(c), 2, "le mur reflète le bleu de derrière : {c:?}");
    // Not for reflections: the wall no longer shows the panorama.
    gpu.view.set_environment(&gpu.rs, Some(red_blue_env(true, false, 0.0)));
    let c = gpu.render(&m, &photoreal, &to_green, &unlit()).at(60, 45);
    assert!(c[2] < c[0] + 40, "sans reflet de l'environnement : {c:?}");
    gpu.view.set_environment(&gpu.rs, None);
}

#[test]
fn environment_commands_act_on_the_current_environment() {
    use crate::commands::Cmd;
    use crate::environments as envs;
    use crate::view_cmds::ViewCmd;
    let mut app = crate::app::EsquisseApp::bare(None, None);
    let ctx = eframe::egui::Context::default();
    let (sky, refl) = (Cmd::View(ViewCmd::EnvSkydome), Cmd::View(ViewCmd::EnvReflections));
    assert!(!app.cmd_enabled(sky) && !app.cmd_enabled(refl), "no environment");
    // A small HDR panorama, added as Window › Environments › Add does.
    let dir = std::env::temp_dir().join(format!("esq-env-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Ciel du test.hdr");
    let img =
        image::Rgb32FImage::from_fn(16, 8, |x, _| image::Rgb(if x < 8 { [3.0, 0.2, 0.1] } else { [0.1, 0.2, 0.9] }));
    image::DynamicImage::ImageRgb32F(img).save(&path).unwrap();
    let id = app.use_environment_file(&path).unwrap();
    let e = envs::current(&app.doc.model).unwrap();
    assert_eq!((e.id, e.name.as_str(), e.skydome, e.reflections), (id, "Ciel du test", true, true));
    assert!(app.doc.model.files.contains_key(&envs::file_key(id)), "the panorama travels with the model");
    let (view, _) = envs::active(&app.doc.model, true);
    let view = view.expect("decoded");
    assert_eq!(view.image.levels[0].0, 16);
    // The commands toggle the current environment's settings.
    assert!(app.cmd_enabled(sky));
    assert_eq!(app.cmd_checked(sky), Some(true));
    app.run(&ctx, sky);
    assert!(!envs::current(&app.doc.model).unwrap().skydome);
    assert_eq!(app.cmd_checked(sky), Some(false));
    app.run(&ctx, refl);
    assert!(!envs::current(&app.doc.model).unwrap().reflections);
    // Neither as a skydome nor for reflections: nothing to draw.
    assert!(envs::active(&app.doc.model, true).0.is_none());
    // Changing the active environment is undoable.
    app.doc.undo();
    assert!(envs::current(&app.doc.model).is_none());
    std::fs::remove_dir_all(&dir).ok();
    // One of SketchUp's: added with its own settings and French name.
    if let Some(first) = envs::read_folder(std::path::Path::new(envs::SKETCHUP_LIBRARY)).first() {
        app.use_environment_file(&first.path).unwrap();
        let e = envs::current(&app.doc.model).unwrap();
        assert_eq!(e.name, first.name);
        assert!(e.skydome && e.reflections);
    }
}

/// Writes `environment.esq` in `ESQ_DEMO_DIR` for a look on screen: a chrome box and a red
/// one on a grey floor, with one of SketchUp's environments (when installed) current.
#[test]
#[ignore]
fn write_environment_demo() {
    use crate::environments as envs;
    let Ok(dir) = std::env::var("ESQ_DEMO_DIR") else { return };
    let mut m = Model::new();
    let chrome = m.add_material("Chrome", [220, 220, 225]);
    let p = &mut m.materials[chrome].ext.pbr;
    (p.metalness_enabled, p.metallic_factor, p.roughness_enabled, p.roughness_factor) = (true, 1.0, true, 0.08);
    let red = m.add_material("Rouge", [200, 30, 30]);
    let floor = m.add_material("Sol", [120, 120, 120]);
    let boxes = [(-2500.0, chrome), (1000.0, red)];
    for (x0, mat) in boxes {
        let v = [(x0, -1500.0), (x0 + 2000.0, -1500.0), (x0 + 2000.0, 500.0), (x0, 500.0)]
            .map(|(x, y)| m.root.add_vertex_raw(DVec3::new(x, y, 0.0)));
        let f = m.root.add_face_raw(v.to_vec(), Vec::new()).unwrap();
        m.root.push_pull(f, 2000.0, false);
        for face in m.root.faces.values_mut() {
            if face.front.is_none() {
                face.front = Some(mat);
                face.back = Some(mat);
            }
        }
    }
    let v = [(-8000.0, -8000.0), (8000.0, -8000.0), (8000.0, 8000.0), (-8000.0, 8000.0)]
        .map(|(x, y)| m.root.add_vertex_raw(DVec3::new(x, y, -1.0)));
    let f = m.root.add_face_raw(v.to_vec(), Vec::new()).unwrap();
    m.root.faces[f].front = Some(floor);
    m.root.faces[f].back = Some(floor);
    let ske = std::path::Path::new(envs::SKETCHUP_LIBRARY).join("Sky - Partial Clouds 2K IBL.ske");
    if let Ok((e, file)) = envs::from_file(&ske) {
        let id = envs::add(&mut m, e, Some(file));
        envs::set_current(&mut m, Some(id));
    }
    esq_core::io::save_esq(&m, &std::path::Path::new(&dir).join("environment.esq")).unwrap();
}

#[test]
fn the_modeling_grid_is_drawn_on_its_plane_and_the_cursor_snaps_to_it() {
    use crate::model_grid as mg;
    // Snapping: View › Grid on and grid snapping on, a free point on the ground goes to the
    // nearest intersection ("Point de grille").
    let mut h = super::Harness::new();
    let mut s = mg::settings(&h.doc.model);
    s.snap = true;
    mg::save(&mut h.doc.model, &s);
    h.doc.model_rev += 1;
    h.tool(crate::tools::ToolId::Line);
    let near = DVec3::new(930.0, 1840.0, 0.0);
    h.hover(near);
    assert_ne!(h.tool.snap().unwrap().kind, crate::pick::SnapKind::GridPoint, "grid hidden");
    h.style.modeling_grid = true;
    h.doc.model_rev += 1;
    h.hover(near);
    let snap = h.tool.snap().unwrap();
    assert_eq!(snap.kind, crate::pick::SnapKind::GridPoint);
    assert!(snap.pos.distance(DVec3::new(914.4, 1828.8, 0.0)) < 1e-6, "{:?}", snap.pos);

    // Drawing: major lines every 36" (914.4 mm), 2 px, black.
    let Some(mut gpu) = Gpu::new() else { return eprintln!("pas de GPU : test sauté") };
    let m = Model::new();
    let style = ViewStyle { axes: false, sky: false, ground: false, ..Default::default() };
    let cam = top_camera(2000.0, 200.0);
    gpu.view.set_grid(mg::view_of(&m, true));
    let shot = gpu.render(&m, &style, &cam, &unlit());
    let at = |x: f64, y: f64| shot.at_point(&cam, DVec3::new(x, y, 0.0));
    let line = at(914.4, 500.0);
    let cell = at(450.0, 450.0);
    assert!(line.iter().all(|c| *c < 90), "ligne principale : {line:?}");
    assert!(cell.iter().all(|c| *c > 150), "entre les lignes : {cell:?}");
    // Minor lines with two subdivisions; major lines hidden.
    let mut m = Model::new();
    let mut s = mg::settings(&m);
    s.current_mut().unwrap().subdivisions = [2, 2];
    s.show_major = false;
    mg::save(&mut m, &s);
    gpu.view.set_grid(mg::view_of(&m, true));
    let shot = gpu.render(&m, &style, &cam, &unlit());
    // The pixel the point falls in.
    let at = |x: f64, y: f64| {
        let s = cam.project(DVec3::new(x, y, 0.0)).unwrap();
        shot.at(s.x.floor() as u32, s.y.floor() as u32)
    };
    assert!(at(457.2, 300.0).iter().all(|c| *c < 150), "ligne secondaire : {:?}", at(457.2, 300.0));
    assert!(at(685.8, 300.0).iter().all(|c| *c > 150), "entre deux lignes : {:?}", at(685.8, 300.0));
    gpu.view.set_grid(None);
}

/// Writes `grid.esq` in `ESQ_DEMO_DIR`: a box on the modeling grid (minor lines, snapping on),
/// View › Grid on.
#[test]
#[ignore]
fn write_grid_demo() {
    use crate::model_grid as mg;
    let Ok(dir) = std::env::var("ESQ_DEMO_DIR") else { return };
    let mut m = Model::new();
    let v = [(0.0, 0.0), (1828.8, 0.0), (1828.8, 914.4), (0.0, 914.4)]
        .map(|(x, y)| m.root.add_vertex_raw(DVec3::new(x, y, 0.0)));
    let f = m.root.add_face_raw(v.to_vec(), Vec::new()).unwrap();
    m.root.push_pull(f, 914.4, false);
    let mut s = mg::settings(&m);
    s.snap = true;
    let g = s.current_mut().unwrap();
    g.subdivisions = [4, 4];
    g.minor.color = [120, 120, 120];
    mg::save(&mut m, &s);
    m.view_ext.insert("ro".into(), serde_json::json!({"ModelingGrid": true}));
    esq_core::io::save_esq(&m, &std::path::Path::new(&dir).join("grid.esq")).unwrap();
}

/// Imports the files of `ESQ_IMPORT_DIR` (DAE, GLB, 3DS, KMZ, USDZ…) and writes each as
/// `imp_<file>.esq` in `ESQ_DEMO_DIR`, to look at their textures on screen.
#[test]
#[ignore]
fn write_imported_demos() {
    let (Ok(src), Ok(dir)) = (std::env::var("ESQ_IMPORT_DIR"), std::env::var("ESQ_DEMO_DIR")) else { return };
    for e in std::fs::read_dir(&src).unwrap().flatten() {
        let p = e.path();
        let Ok(imp) = esq_formats::import(&p, &Default::default(), esq_core::model::Units::Millimeters) else {
            continue;
        };
        let name = p.file_name().unwrap().to_string_lossy().replace('.', "_");
        esq_core::io::save_esq(&imp.model, &std::path::Path::new(&dir).join(format!("imp_{name}.esq"))).unwrap();
    }
}
