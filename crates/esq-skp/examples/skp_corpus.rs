//! Builds the .skp corpus of the native reader: one small model per kind of object (and pairs
//! that differ by one detail), saved by the SketchUp SDK in every version it can write.
//!
//! `cargo run -p esq-skp --example skp_corpus -- [out_dir]` (default
//! `%USERPROFILE%\tools\skp-corpus`). Files go to `<out>/gen/<version>/<name>.skp`. Needs SketchUp
//! installed (the SDK writes the files); the corpus stays out of the repository.

use std::path::PathBuf;
use std::sync::Arc;

use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::meta::{FaceTex, LayerFolder, TextureData, uv_from_pairs};
use esq_core::model::{
    AnnotKind, Annotation, AttrValue, Attributes, EntityRef, Guide, Instance, Model, Scene, SceneCamera, SectionPlane,
    Units, set_attr,
};
use esq_core::photo::Blob;
use esq_skp::SkpVersion;
use serde_json::json;

fn png(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 7) as u8 ^ seed, (y * 5) as u8, seed]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn texture(name: &str, w: u32, h: u32, seed: u8, size: [f64; 2]) -> TextureData {
    TextureData { filename: name.into(), image: Blob(Arc::new(png(w, h, seed))), px: [w, h], size, avg: [120, 90, 60] }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Vec<DVec3> {
    vec![DVec3::new(x0, y0, z), DVec3::new(x1, y0, z), DVec3::new(x1, y1, z), DVec3::new(x0, y1, z)]
}

fn circle(c: DVec3, r: f64, n: usize, sweep: f64) -> Vec<DVec3> {
    let count = if sweep >= std::f64::consts::TAU { n } else { n + 1 };
    (0..count)
        .map(|i| c + DVec3::new((sweep * i as f64 / n as f64).cos(), (sweep * i as f64 / n as f64).sin(), 0.0) * r)
        .collect()
}

fn boxed(m: &mut Model, x: f64, y: f64, w: f64, d: f64, h: f64) {
    let e = &mut m.root;
    e.add_polyline(&rect(x, y, x + w, y + d, 0.0), true, false);
    let f = e.faces.iter().find(|(_, f)| f.normal.z.abs() > 0.99 && e.verts[f.outer[0]].pos.z.abs() < 1e-9).unwrap().0;
    let dist = if e.faces[f].normal.z > 0.0 { h } else { -h };
    e.push_pull(f, dist, false);
}

fn all_kinds() -> Attributes {
    let mut a = Attributes::new();
    set_attr(&mut a, "fiche", "texte", AttrValue::Text("Chêne massif".into()));
    set_attr(&mut a, "fiche", "réel", AttrValue::Number(2.5));
    set_attr(&mut a, "fiche", "oui", AttrValue::Bool(true));
    set_attr(&mut a, "fiche", "entier", AttrValue::Json(json!({"t": "i", "v": 42})));
    set_attr(&mut a, "fiche", "couleur", AttrValue::Json(json!({"t": "c", "v": [10, 20, 30, 255]})));
    set_attr(&mut a, "fiche", "date", AttrValue::Json(json!({"t": "time", "v": [1760000000, 0]})));
    set_attr(&mut a, "fiche", "vecteur", AttrValue::Json(json!({"t": "vec", "v": [1.0, 0.0, 0.5]})));
    set_attr(
        &mut a,
        "fiche",
        "liste",
        AttrValue::Json(json!({"t": "a", "v": [{"t": "i", "v": 1}, {"t": "s", "v": "deux"}, {"t": "f", "v": 3.5}]})),
    );
    a
}

fn cam(eye: DVec3) -> SceneCamera {
    SceneCamera { eye, target: DVec3::new(500.0, 300.0, 200.0), up: DVec3::Z, fov_deg: 35.0, perspective: true }
}

fn instance(def: esq_core::model::DefId, at: DVec3) -> Instance {
    Instance {
        def,
        xform: DMat4::from_translation(at),
        name: String::new(),
        material: None,
        layer: 0,
        hidden: false,
        locked: false,
        attributes: Default::default(),
        ext: Default::default(),
    }
}

type Builder = fn() -> Model;

/// Everything at once (the samples kept in `tests/data`): textures and their placement, curves,
/// groups and components, tags and folders, scenes and styles, annotations, guides, sections,
/// an image, dictionaries, shadows and location.
fn kitchen() -> Model {
    let mut m = models().into_iter().find(|(n, _)| *n == "texture_cube").map(|(_, b)| b()).unwrap();
    m.meta.name = "Cuisine".into();
    m.meta.description = "Plan de la cuisine".into();
    m.attr_store.insert("model".into(), all_kinds());
    let walls = m.add_layer("Murs");
    m.layers[walls].color = Some([200, 10, 10]);
    m.layers[walls].line_style = "Dash".into();
    let hidden = m.add_layer("Cachée");
    m.layers[hidden].visible = false;
    let structure = m.new_uid();
    m.meta.folders.push(LayerFolder {
        uid: structure,
        name: "Structure".into(),
        parent: None,
        visible: true,
        visible_on_new_pages: true,
    });
    m.layers[walls].folder = Some(structure);
    let e = &mut m.root;
    e.add_polyline(&circle(DVec3::new(2000.0, 0.0, 0.0), 300.0, 24, std::f64::consts::TAU), true, true);
    e.add_polyline(&circle(DVec3::new(3000.0, 0.0, 0.0), 300.0, 8, 2.0), false, true);
    e.add_polyline(
        &[DVec3::new(4000.0, 0.0, 0.0), DVec3::new(4200.0, 300.0, 0.0), DVec3::new(4500.0, 250.0, 50.0)],
        false,
        true,
    );
    let soft = e.add_line(DVec3::new(0.0, -800.0, 0.0), DVec3::new(900.0, -800.0, 0.0))[0];
    (e.edges[soft].soft, e.edges[soft].smooth, e.edges[soft].layer) = (true, true, walls);
    // A component (two instances, one rotated and scaled) holding a group.
    let mut leg = Model::new();
    boxed(&mut leg, 0.0, 0.0, 60.0, 60.0, 700.0);
    let items: Vec<EntityRef> = leg.root.faces.keys().map(EntityRef::Face).collect();
    leg.make_group(None, &items, true, Some("Fût".into()));
    let def = m.import_as_component(&leg, "Pied");
    m.defs[def].attributes = all_kinds();
    m.defs[def].description = "Pied de table".into();
    let mut a = instance(def, DVec3::new(-500.0, 0.0, 0.0));
    a.attributes = all_kinds();
    a.layer = walls;
    m.root.instances.insert(a);
    let mut b = instance(def, DVec3::new(-500.0, 900.0, 0.0));
    b.xform = b.xform * DMat4::from_rotation_z(0.5) * DMat4::from_scale(DVec3::new(1.0, 2.0, 0.5));
    b.name = "Pied tourné".into();
    b.locked = true;
    m.root.instances.insert(b);
    m.root.guides.insert(Guide::line(DVec3::new(0.0, -250.0, 0.0), DVec3::X));
    m.root.guides.insert(Guide::point(DVec3::new(1000.0, 0.0, 500.0)));
    m.root.guides.insert(Guide::segment(DVec3::ZERO, DVec3::new(0.0, 0.0, 300.0)));
    let mut sec = SectionPlane::new(DVec3::new(0.0, 300.0, 0.0), DVec3::Y);
    (sec.name, sec.symbol) = ("Coupe".into(), "A".into());
    let sec = m.root.sections.insert(sec);
    m.root.active_section = Some(sec);
    m.root.annots.insert(Annotation {
        kind: AnnotKind::Linear {
            a: DVec3::ZERO,
            b: DVec3::new(1000.0, 0.0, 0.0),
            offset: DVec3::new(0.0, -300.0, 0.0),
        },
        text: String::new(),
        layer: walls,
        hidden: false,
        attach: Default::default(),
    });
    m.root.annots.insert(Annotation {
        kind: AnnotKind::Text { point: DVec3::new(500.0, 300.0, 400.0), leader: DVec3::new(200.0, 0.0, 300.0) },
        text: "Plateau chêne".into(),
        layer: 0,
        hidden: false,
        attach: Default::default(),
    });
    m.root.annots.insert(Annotation {
        kind: AnnotKind::ScreenText { x: 0.1, y: 0.05 },
        text: "Cuisine".into(),
        layer: 0,
        hidden: false,
        attach: Default::default(),
    });
    m.add_image(
        None,
        "plan",
        "plan.png",
        texture("plan.png", 40, 20, 11, [0.0, 0.0]),
        DVec3::new(0.0, -2000.0, 0.0),
        DVec3::new(1.0, 1.0, 0.0),
        DVec3::Z,
        1200.0,
        None,
    );
    m.shadow.display = true;
    m.shadow.set_local(2026, 6, 21, 9.0 * 60.0 + 15.0);
    (m.shadow.latitude, m.shadow.longitude, m.shadow.tz_offset) = (48.8566, 2.3522, 1.0);
    m.shadow.city = "Paris".into();
    m.view_ext.insert("ro".into(), json!({"DrawSilhouettes": false, "BackgroundColor": [10, 20, 30, 255]}));
    m.scenes.push(Scene {
        name: "Face".into(),
        description: "Vue de face".into(),
        camera: cam(DVec3::new(500.0, -6000.0, 1500.0)),
        hidden_layers: Some(vec![hidden]),
        photo: None,
        ext: Default::default(),
    });
    let mut s = Scene {
        name: "Stylée".into(),
        description: String::new(),
        camera: cam(DVec3::new(500.0, -5000.0, 1500.0)),
        hidden_layers: None,
        photo: None,
        ext: Default::default(),
    };
    s.ext.props.insert("flags".into(), json!(4095));
    s.ext.props.insert("ro".into(), json!({"DrawSilhouettes": true, "RenderMode": 1}));
    m.scenes.push(s);
    m
}

fn models() -> Vec<(&'static str, Builder)> {
    vec![
        ("empty", Model::new as Builder),
        ("kitchen", kitchen),
        ("edge", || {
            let mut m = Model::new();
            m.root.add_line(DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0));
            m
        }),
        ("edge_b", || {
            let mut m = Model::new();
            m.root.add_line(DVec3::new(10.0, 20.0, 30.0), DVec3::new(2000.0, 500.0, 300.0));
            m
        }),
        ("edges3", || {
            let mut m = Model::new();
            m.root.add_line(DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0));
            m.root.add_line(DVec3::new(0.0, 500.0, 0.0), DVec3::new(1000.0, 500.0, 0.0));
            m.root.add_line(DVec3::new(0.0, 1000.0, 0.0), DVec3::new(1000.0, 1000.0, 300.0));
            m
        }),
        ("edge_flags", || {
            let mut m = Model::new();
            for (i, (soft, smooth, hidden)) in
                [(true, false, false), (false, true, false), (false, false, true), (true, true, false)]
                    .into_iter()
                    .enumerate()
            {
                let y = i as f64 * 300.0;
                let e = m.root.add_line(DVec3::new(0.0, y, 0.0), DVec3::new(1000.0, y, 0.0))[0];
                let edge = &mut m.root.edges[e];
                (edge.soft, edge.smooth, edge.hidden) = (soft, smooth, hidden);
            }
            m
        }),
        ("triangle", || {
            let mut m = Model::new();
            m.root.add_polyline(
                &[DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(0.0, 1000.0, 0.0)],
                true,
                false,
            );
            m
        }),
        ("square", || {
            let mut m = Model::new();
            m.root.add_polyline(&rect(0.0, 0.0, 1000.0, 1000.0, 0.0), true, false);
            m
        }),
        ("square_up", || {
            let mut m = Model::new();
            m.root.add_polyline(&rect(0.0, 0.0, 1000.0, 1000.0, 500.0), true, false);
            m
        }),
        ("square_hole", || {
            let mut m = Model::new();
            m.root.add_polyline(&rect(0.0, 0.0, 1000.0, 1000.0, 0.0), true, false);
            m.root.add_polyline(&rect(300.0, 300.0, 700.0, 700.0, 0.0), true, false);
            m
        }),
        ("cube", || {
            let mut m = Model::new();
            boxed(&mut m, 0.0, 0.0, 1000.0, 600.0, 400.0);
            m
        }),
        ("curve", || {
            let mut m = Model::new();
            m.root.add_polyline(
                &[
                    DVec3::ZERO,
                    DVec3::new(200.0, 300.0, 0.0),
                    DVec3::new(500.0, 250.0, 0.0),
                    DVec3::new(700.0, 600.0, 0.0),
                ],
                false,
                true,
            );
            m
        }),
        ("arc", || {
            let mut m = Model::new();
            m.root.add_polyline(&circle(DVec3::ZERO, 300.0, 8, 2.0), false, true);
            m
        }),
        ("circle", || {
            let mut m = Model::new();
            m.root.add_polyline(&circle(DVec3::ZERO, 300.0, 24, std::f64::consts::TAU), true, true);
            m
        }),
        ("polygon", || {
            let mut m = Model::new();
            m.root.add_polyline(&circle(DVec3::ZERO, 300.0, 6, std::f64::consts::TAU), true, true);
            let c = m.root.edges.values().find_map(|e| e.curve).unwrap();
            m.root.set_prop(&format!("c{c}"), "kind", AttrValue::Text("polygon".into()));
            m
        }),
        ("group", || {
            let mut m = Model::new();
            boxed(&mut m, 0.0, 0.0, 1000.0, 600.0, 400.0);
            let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
            m.make_group(None, &items, true, Some("Caisson".into()));
            m
        }),
        ("component", || {
            let mut m = Model::new();
            boxed(&mut m, 0.0, 0.0, 100.0, 100.0, 700.0);
            let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
            let inst = m.make_group(None, &items, false, Some("Pied".into())).unwrap();
            let def = m.root.instances[inst].def;
            m.root.instances.insert(instance(def, DVec3::new(1500.0, 0.0, 0.0)));
            let mut rot = instance(def, DVec3::new(0.0, 900.0, 0.0));
            rot.xform = rot.xform * DMat4::from_rotation_z(0.5) * DMat4::from_scale(DVec3::new(1.0, 2.0, 0.5));
            rot.name = "Pied tourné".into();
            m.root.instances.insert(rot);
            m
        }),
        ("nested", || {
            // Component > group > component.
            let mut leaf = Model::new();
            boxed(&mut leaf, 0.0, 0.0, 50.0, 50.0, 50.0);
            let mut m = Model::new();
            let cube = m.import_as_component(&leaf, "Cube");
            let mut mid = Model::new();
            let mid_cube = mid.import_as_component(&leaf, "Cube");
            mid.root.instances.insert(instance(mid_cube, DVec3::ZERO));
            mid.root.instances.insert(instance(mid_cube, DVec3::new(100.0, 0.0, 0.0)));
            let items: Vec<EntityRef> = mid.root.instances.keys().map(EntityRef::Instance).collect();
            mid.make_group(None, &items, true, Some("Rangée".into()));
            let outer = m.import_as_component(&mid, "Bloc");
            m.root.instances.insert(instance(outer, DVec3::ZERO));
            m.root.instances.insert(instance(outer, DVec3::new(0.0, 500.0, 0.0)));
            m.root.instances.insert(instance(cube, DVec3::new(0.0, -500.0, 0.0)));
            m
        }),
        ("material_color", || {
            let mut m = Model::new();
            let red = m.add_material("Rouge", [200, 30, 40]);
            m.root.add_polyline(&rect(0.0, 0.0, 1000.0, 1000.0, 0.0), true, false);
            let f = m.root.faces.keys().next().unwrap();
            m.root.faces[f].front = Some(red);
            m
        }),
        ("material_back", || {
            let mut m = Model::new();
            let red = m.add_material("Rouge", [200, 30, 40]);
            let blue = m.add_material("Bleu", [20, 30, 240]);
            m.root.add_polyline(&rect(0.0, 0.0, 1000.0, 1000.0, 0.0), true, false);
            let f = m.root.faces.keys().next().unwrap();
            m.root.faces[f].front = Some(red);
            m.root.faces[f].back = Some(blue);
            m
        }),
        ("material_alpha", || {
            let mut m = Model::new();
            let glass = m.add_material("Verre", [200, 220, 255]);
            m.materials[glass].opacity = 0.4;
            m.root.add_polyline(&rect(0.0, 0.0, 1000.0, 1000.0, 0.0), true, false);
            let f = m.root.faces.keys().next().unwrap();
            m.root.faces[f].front = Some(glass);
            m
        }),
        ("material_unused", || {
            let mut m = Model::new();
            m.add_material("Seule", [1, 2, 3]);
            m
        }),
        ("material_texture", || {
            let mut m = Model::new();
            let brick = m.add_material("Brique", [150, 80, 60]);
            m.materials[brick].ext.texture = Some(texture("brique.png", 64, 32, 7, [500.0, 250.0]));
            m.root.add_polyline(&rect(0.0, 0.0, 1000.0, 1000.0, 0.0), true, false);
            let f = m.root.faces.keys().next().unwrap();
            m.root.faces[f].front = Some(brick);
            m
        }),
        ("material_texture_pos", || {
            let mut m = Model::new();
            let brick = m.add_material("Brique", [150, 80, 60]);
            m.materials[brick].ext.texture = Some(texture("brique.png", 64, 32, 7, [500.0, 250.0]));
            m.root.add_polyline(&rect(0.0, 0.0, 1000.0, 1000.0, 0.0), true, false);
            let f = m.root.faces.keys().next().unwrap();
            let n = m.root.faces[f].normal;
            let p0 = m.root.verts[m.root.faces[f].outer[0]].pos;
            let (ex, ey) = esq_core::meta::face_axes(n);
            let pairs = [
                (p0, DVec2::new(0.3, 0.1)),
                (p0 + ex * 400.0, DVec2::new(1.1, 0.9)),
                (p0 + ey * 300.0, DVec2::new(-0.2, 1.4)),
            ];
            let face = &mut m.root.faces[f];
            face.front = Some(brick);
            face.tex =
                Some(Box::new(FaceTex { front: uv_from_pairs(n, [500.0, 250.0], &pairs), ..Default::default() }));
            m
        }),
        ("texture_cube", || {
            // Positioned textures on every side of a box: three-pin, two-pin, distorted (four
            // pins), and on the back of a face.
            let mut m = Model::new();
            let brick = m.add_material("Brique", [150, 80, 60]);
            m.materials[brick].ext.texture = Some(texture("brique.png", 64, 32, 7, [500.0, 250.0]));
            let tile = m.add_material("Carreau", [100, 100, 100]);
            m.materials[tile].ext.texture = Some(texture("carreau.png", 32, 32, 9, [300.0, 300.0]));
            boxed(&mut m, 100.0, 200.0, 1000.0, 600.0, 400.0);
            let e = &mut m.root;
            let faces: Vec<_> = e.faces.keys().collect();
            for (i, f) in faces.iter().enumerate() {
                let n = e.faces[*f].normal;
                let p0 = e.verts[e.faces[*f].outer[0]].pos;
                let (ex, ey) = esq_core::meta::face_axes(n);
                let face = &mut e.faces[*f];
                let three = [
                    (p0, DVec2::new(0.3, 0.1)),
                    (p0 + ex * 400.0, DVec2::new(1.1, 0.9)),
                    (p0 + ey * 300.0, DVec2::new(-0.2, 1.4)),
                ];
                let four = [
                    (p0, DVec2::new(0.0, 0.0)),
                    (p0 + ex * 400.0, DVec2::new(1.0, 0.0)),
                    (p0 + ex * 350.0 + ey * 300.0, DVec2::new(1.0, 1.0)),
                    (p0 + ex * 50.0 + ey * 300.0, DVec2::new(0.0, 1.0)),
                ];
                let two = [(p0, DVec2::new(0.5, 0.5)), (p0 + ex * 250.0, DVec2::new(1.5, 0.5))];
                match i % 4 {
                    0 => {
                        face.front = Some(brick);
                        face.tex = Some(Box::new(FaceTex {
                            front: uv_from_pairs(n, [500.0, 250.0], &three),
                            ..Default::default()
                        }));
                    }
                    1 => {
                        face.front = Some(tile);
                        face.tex = Some(Box::new(FaceTex {
                            front: uv_from_pairs(n, [300.0, 300.0], &four),
                            ..Default::default()
                        }));
                    }
                    2 => {
                        face.back = Some(brick);
                        face.front = Some(tile);
                        face.tex = Some(Box::new(FaceTex {
                            back: uv_from_pairs(-n, [500.0, 250.0], &two),
                            ..Default::default()
                        }));
                    }
                    _ => face.front = Some(brick),
                }
            }
            m
        }),
        ("material_colorized", || {
            let mut m = Model::new();
            let tinted = m.add_material("Teintée", [40, 160, 200]);
            m.materials[tinted].ext.texture = Some(texture("brique2.png", 32, 32, 9, [300.0, 300.0]));
            m.materials[tinted].ext.colorize_deltas = [80.0, 0.3, 0.1];
            m.root.add_polyline(&rect(0.0, 0.0, 1000.0, 1000.0, 0.0), true, false);
            let f = m.root.faces.keys().next().unwrap();
            m.root.faces[f].front = Some(tinted);
            m
        }),
        ("material_pbr", || {
            let mut m = Model::new();
            let steel = m.add_material("Acier", [180, 180, 190]);
            let e = &mut m.materials[steel].ext;
            e.pbr.metalness_enabled = true;
            e.pbr.metallic_factor = 0.8;
            e.pbr.roughness_enabled = true;
            e.pbr.roughness_factor = 0.3;
            e.pbr.normal_enabled = true;
            e.pbr.normal_scale = 1.5;
            e.pbr.normal_texture = Some(texture("normale.png", 16, 16, 3, [254.0, 254.0]));
            e.texture = Some(texture("acier.png", 16, 16, 5, [254.0, 254.0]));
            m.root.add_polyline(&rect(0.0, 0.0, 1000.0, 1000.0, 0.0), true, false);
            let f = m.root.faces.keys().next().unwrap();
            m.root.faces[f].front = Some(steel);
            m
        }),
        ("instance_material", || {
            let mut m = Model::new();
            boxed(&mut m, 0.0, 0.0, 300.0, 300.0, 300.0);
            let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
            let inst = m.make_group(None, &items, false, Some("Boîte".into())).unwrap();
            let green = m.add_material("Vert", [10, 200, 30]);
            m.root.instances[inst].material = Some(green);
            m
        }),
        ("layers", || {
            let mut m = Model::new();
            let walls = m.add_layer("Murs");
            m.layers[walls].color = Some([200, 10, 10]);
            m.layers[walls].line_style = "Dash".into();
            let slabs = m.add_layer("Dalles");
            m.layers[slabs].page_behavior = 0x21;
            m.layers[slabs].visible = false;
            let e = m.root.add_line(DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0))[0];
            m.root.edges[e].layer = walls;
            m.root.add_polyline(&rect(0.0, 100.0, 1000.0, 1000.0, 0.0), true, false);
            let f = m.root.faces.keys().next().unwrap();
            m.root.faces[f].layer = slabs;
            m
        }),
        ("layer_folders", || {
            let mut m = Model::new();
            let walls = m.add_layer("Murs");
            let slabs = m.add_layer("Dalles");
            let structure = m.new_uid();
            let level = m.new_uid();
            m.meta.folders.push(LayerFolder {
                uid: structure,
                name: "Structure".into(),
                parent: None,
                visible: true,
                visible_on_new_pages: true,
            });
            m.meta.folders.push(LayerFolder {
                uid: level,
                name: "Niveau 1".into(),
                parent: Some(structure),
                visible: false,
                visible_on_new_pages: false,
            });
            m.layers[walls].folder = Some(structure);
            m.layers[slabs].folder = Some(level);
            m.meta.active_layer = m.layers[walls].uid;
            m
        }),
        ("hidden", || {
            let mut m = Model::new();
            m.root.add_polyline(&rect(0.0, 0.0, 1000.0, 1000.0, 0.0), true, false);
            let f = m.root.faces.keys().next().unwrap();
            m.root.faces[f].hidden = true;
            m
        }),
        ("scenes", || {
            let mut m = Model::new();
            boxed(&mut m, 0.0, 0.0, 1000.0, 600.0, 400.0);
            let l = m.add_layer("Cachée");
            m.scenes.push(Scene {
                name: "Face".into(),
                description: "Vue de face".into(),
                camera: cam(DVec3::new(500.0, -6000.0, 1500.0)),
                hidden_layers: Some(vec![l]),
                photo: None,
                ext: Default::default(),
            });
            let mut top = cam(DVec3::new(500.0, 300.0, 8000.0));
            top.perspective = false;
            top.up = DVec3::Y;
            m.scenes.push(Scene {
                name: "Dessus".into(),
                description: String::new(),
                camera: top,
                hidden_layers: None,
                photo: None,
                ext: Default::default(),
            });
            m
        }),
        ("scene_style", || {
            let mut m = Model::new();
            boxed(&mut m, 0.0, 0.0, 1000.0, 600.0, 400.0);
            let mut s = Scene {
                name: "Stylée".into(),
                description: String::new(),
                camera: cam(DVec3::new(500.0, -5000.0, 1500.0)),
                hidden_layers: None,
                photo: None,
                ext: Default::default(),
            };
            s.ext.props.insert("flags".into(), json!(4095));
            s.ext.props.insert("ro".into(), json!({"DrawSilhouettes": true, "RenderMode": 1}));
            s.ext.props.insert("animate".into(), json!(false));
            m.scenes.push(s);
            m
        }),
        ("style", || {
            let mut m = Model::new();
            m.view_ext.insert(
                "ro".into(),
                json!({"DrawSilhouettes": false, "BackgroundColor": [10, 20, 30, 255], "EdgeColorMode": 2}),
            );
            m
        }),
        ("fog", || {
            let mut m = Model::new();
            m.view_ext.insert(
                "ro".into(),
                json!({"DisplayFog": true, "FogColor": [200, 100, 50, 255], "FogStartDist": 12.5, "FogEndDist": 80.0, "FogUseBkColor": false}),
            );
            m
        }),
        ("shadows", || {
            let mut m = Model::new();
            m.shadow.display = true;
            m.shadow.set_local(2026, 6, 21, 9.0 * 60.0 + 15.0);
            (m.shadow.light, m.shadow.dark, m.shadow.on_ground) = (70, 30, false);
            m
        }),
        ("geolocation", || {
            let mut m = Model::new();
            (m.shadow.latitude, m.shadow.longitude, m.shadow.tz_offset) = (48.8566, 2.3522, 1.0);
            m.shadow.north_angle = 12.5;
            m.shadow.city = "Paris".into();
            m.shadow.country = "France".into();
            m
        }),
        ("dimension", || {
            let mut m = Model::new();
            m.root.add_line(DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0));
            m.root.annots.insert(Annotation {
                kind: AnnotKind::Linear {
                    a: DVec3::ZERO,
                    b: DVec3::new(1000.0, 0.0, 0.0),
                    offset: DVec3::new(0.0, -300.0, 0.0),
                },
                text: String::new(),
                layer: 0,
                hidden: false,
                attach: Default::default(),
            });
            m
        }),
        ("texts", || {
            let mut m = Model::new();
            m.root.annots.insert(Annotation {
                kind: AnnotKind::Text { point: DVec3::new(500.0, 300.0, 400.0), leader: DVec3::new(200.0, 0.0, 300.0) },
                text: "Plateau chêne".into(),
                layer: 0,
                hidden: false,
                attach: Default::default(),
            });
            m.root.annots.insert(Annotation {
                kind: AnnotKind::ScreenText { x: 0.1, y: 0.05 },
                text: "Cuisine".into(),
                layer: 0,
                hidden: false,
                attach: Default::default(),
            });
            m
        }),
        ("guides", || {
            let mut m = Model::new();
            m.root.guides.insert(Guide::line(DVec3::new(0.0, -250.0, 0.0), DVec3::X));
            m.root.guides.insert(Guide::point(DVec3::new(1000.0, 0.0, 500.0)));
            m.root.guides.insert(Guide::segment(DVec3::ZERO, DVec3::new(0.0, 0.0, 300.0)));
            m
        }),
        ("sections", || {
            let mut m = Model::new();
            boxed(&mut m, 0.0, 0.0, 1000.0, 600.0, 400.0);
            let mut sec = SectionPlane::new(DVec3::new(0.0, 300.0, 0.0), DVec3::Y);
            (sec.name, sec.symbol) = ("Coupe longitudinale".into(), "A".into());
            let sec = m.root.sections.insert(sec);
            m.root.active_section = Some(sec);
            let mut other = SectionPlane::new(DVec3::new(0.0, 0.0, 200.0), -DVec3::Z);
            other.symbol = "B".into();
            m.root.sections.insert(other);
            m
        }),
        ("image", || {
            let mut m = Model::new();
            m.add_image(
                None,
                "plan",
                "plan.png",
                texture("plan.png", 40, 20, 11, [0.0, 0.0]),
                DVec3::new(0.0, -2000.0, 0.0),
                DVec3::new(1.0, 1.0, 0.0),
                DVec3::Z,
                1200.0,
                None,
            );
            m
        }),
        ("attributes", || {
            let mut m = Model::new();
            m.attr_store.insert("model".into(), all_kinds());
            boxed(&mut m, 0.0, 0.0, 300.0, 300.0, 300.0);
            let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
            let inst = m.make_group(None, &items, false, Some("Boîte".into())).unwrap();
            let def = m.root.instances[inst].def;
            m.defs[def].attributes = all_kinds();
            m.root.instances[inst].attributes = all_kinds();
            m
        }),
        ("model_info", || {
            let mut m = Model::new();
            m.meta.name = "Cuisine".into();
            m.meta.description = "Plan de la cuisine".into();
            m.units = Units::Centimeters;
            m.meta.options.get_mut("UnitsOptions").unwrap().insert("AnglePrecision".into(), json!(3));
            m.meta.options.get_mut("PageOptions").unwrap().insert("TransitionTime".into(), json!(4.5));
            m
        }),
        ("units_inch", || {
            let mut m = Model::new();
            m.units = Units::Inches;
            m
        }),
        ("axes", || {
            let mut m = Model::new();
            m.axes = DMat4::from_translation(DVec3::new(100.0, 200.0, 0.0)) * DMat4::from_rotation_z(0.3);
            m
        }),
        ("locked", || {
            let mut m = Model::new();
            boxed(&mut m, 0.0, 0.0, 300.0, 300.0, 300.0);
            let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
            let inst = m.make_group(None, &items, true, Some("Verrou".into())).unwrap();
            m.root.instances[inst].locked = true;
            m.root.instances[inst].hidden = true;
            m
        }),
        ("environments", environments),
        ("behavior", || {
            let mut m = Model::new();
            m.root.add_polyline(&rect(0.0, 0.0, 300.0, 300.0, 0.0), true, false);
            let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
            let inst = m.make_group(None, &items, false, Some("Fenêtre".into())).unwrap();
            let def = m.root.instances[inst].def;
            m.defs[def].description = "Fenêtre à deux battants".into();
            let b = &mut m.defs[def].ext.behavior;
            (b.cuts_opening, b.is2d, b.snapto, b.no_scale_mask) = (true, true, 2, 5);
            m
        }),
    ]
}

/// The content Esquisse ships (styles, classifications, environments), in the repository.
fn content(rel: &str) -> PathBuf {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).expect("le dépôt").to_path_buf();
    repo.join("content").join(rel)
}

/// A model with an environment of Esquisse's (two, the second kept by a scene): the SDK writer
/// makes them (`write_environments`).
fn environments() -> Model {
    let mut m = Model::new();
    boxed(&mut m, 0.0, 0.0, 1000.0, 600.0, 400.0);
    let ske = std::fs::read(content("Environments/Ciel de midi.ske")).unwrap_or_default();
    m.view_ext.insert(
        "envs".into(),
        json!([
            {"id": 1, "name": "Midi", "description": "ciel de midi", "skydome": true, "reflections": true, "rotation": 30.0,
             "skydome_exposure": 1.5, "reflection_exposure": 0.75, "linked_sun": true, "sun_pos": [90.0, 45.0]},
            {"id": 2, "name": "Reflets", "description": "", "skydome": false, "reflections": true, "rotation": 90.0,
             "skydome_exposure": 2.0, "reflection_exposure": 3.0, "linked_sun": false, "sun_pos": [0.0, 45.0]}
        ]),
    );
    m.view_ext.insert("env_current".into(), json!(1));
    m.files.insert("env:1".into(), Blob(Arc::new(ske.clone())));
    m.files.insert("env:2".into(), Blob(Arc::new(ske)));
    let mut s = Scene {
        name: "Reflets".into(),
        description: String::new(),
        camera: cam(DVec3::new(500.0, -5000.0, 1500.0)),
        hidden_layers: None,
        photo: None,
        ext: Default::default(),
    };
    s.ext.props.insert("flags".into(), json!(0x201));
    s.ext.props.insert("env".into(), json!("Reflets"));
    m.scenes.push(s);
    m
}

/// Files whose classes the SDK writer of Esquisse does not make: the SDK adds them itself to a
/// file it wrote (`esq_skp::sdk_corpus_extras`).
fn with_extras() -> Vec<(&'static str, Builder, esq_skp::CorpusExtras)> {
    use esq_skp::CorpusExtras;
    let one_scene = || {
        let mut m = Model::new();
        boxed(&mut m, 0.0, 0.0, 1000.0, 600.0, 400.0);
        m.scenes.push(Scene {
            name: "Vue".into(),
            description: String::new(),
            camera: cam(DVec3::new(500.0, -5000.0, 1500.0)),
            hidden_layers: None,
            photo: None,
            ext: Default::default(),
        });
        m
    };
    let photo = std::env::temp_dir().join(format!("esq-corpus-photo-{}.png", std::process::id()));
    let _ = std::fs::write(&photo, png(64, 48, 21));
    vec![
        (
            "styles_files",
            one_scene,
            CorpusExtras {
                styles: vec![
                    content("Styles/Default Styles/Architecture.style"),
                    content("Styles/Assorted Styles/Brouillard du matin.style"),
                ],
                ..Default::default()
            },
        ),
        (
            "classification",
            one_scene,
            CorpusExtras {
                schemas: vec![content("Classifications/IFC 2x3.skc"), content("Classifications/IFC 4.skc")],
                ..Default::default()
            },
        ),
        (
            "radial",
            || {
                let mut m = Model::new();
                m.root.add_polyline(&circle(DVec3::ZERO, 300.0, 24, std::f64::consts::TAU), true, true);
                m.root.add_polyline(&circle(DVec3::new(1000.0, 0.0, 0.0), 200.0, 8, 2.0), false, true);
                m
            },
            CorpusExtras { radials: true, ..Default::default() },
        ),
        ("match_photo", one_scene, CorpusExtras { photo: Some((photo, "Photo".into())), ..Default::default() }),
    ]
}

fn main() -> anyhow::Result<()> {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| format!(r"{}\tools\skp-corpus", std::env::var("USERPROFILE").unwrap_or_default())),
    );
    let only = std::env::args().nth(2);
    anyhow::ensure!(esq_skp::available(), "SketchUp SDK absent");
    let mut n = 0;
    for (name, build, extras) in with_extras() {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let first = std::env::temp_dir().join(format!("esq-corpus-{}-{name}.skp", std::process::id()));
        esq_skp::sdk_export_skp_version(&build(), &first, None, None)?;
        for v in SkpVersion::ALL {
            let dir = out.join("gen").join(v.name());
            std::fs::create_dir_all(&dir)?;
            match esq_skp::sdk_corpus_extras(&first, &dir.join(format!("{name}.skp")), Some(v), &extras) {
                Ok(()) => n += 1,
                Err(e) => eprintln!("{name} {}: {e:#}", v.name()),
            }
        }
        let _ = std::fs::remove_file(&first);
    }
    for (name, build) in models() {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let m = build();
        for v in SkpVersion::ALL {
            let dir = out.join("gen").join(v.name());
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(format!("{name}.skp"));
            // The corpus is SketchUp's own writing: the SDK's (the oracle of the native reader).
            match esq_skp::sdk_export_skp_version(&m, &path, None, Some(v)) {
                Ok(()) => n += 1,
                Err(e) => eprintln!("{name} {}: {e:#}", v.name()),
            }
        }
    }
    println!("{n} fichiers dans {}", out.display());
    Ok(())
}
