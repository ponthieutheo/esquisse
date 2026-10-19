//! A model of Esquisse goes through a .skp and comes back the same: textures and their
//! positions, PBR maps, curves and arcs, tags with colours and folders, attribute dictionaries
//! of every kind of value on every kind of entity, images, component behaviour, Model Info
//! options, rendering options, scenes.

use std::sync::Arc;

use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::meta::{FaceTex, LayerFolder, TextureData, uv_from_pairs};
use esq_core::model::{AttrValue, Attributes, Model, Scene, SceneCamera, set_attr};
use esq_core::photo::Blob;
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

/// `nil` values: the SDK has no way of storing one (`SUAttributeDictionarySetValue` with an
/// empty value leaves the key out).
#[test]
fn nil_values_are_left_out() {
    let mut m = Model::new();
    let mut a = all_kinds();
    set_attr(&mut a, "fiche", "rien", AttrValue::Json(json!({"t": "n"})));
    m.attr_store.insert("model".into(), a);
    let path = std::env::temp_dir().join(format!("esq-nil-{}.skp", std::process::id()));
    esq_skp::export_skp(&m, &path).unwrap();
    let back = esq_skp::import_skp(&path).unwrap();
    let sdk = esq_skp::available().then(|| esq_skp::sdk_import_skp_view(&path).unwrap().0);
    let _ = std::fs::remove_file(&path);
    for back in std::iter::once(back).chain(sdk) {
        assert_eq!(back.attr_store["model"], all_kinds());
    }
}

fn circle(c: DVec3, r: f64, n: usize, sweep: f64) -> Vec<DVec3> {
    let count = if sweep >= std::f64::consts::TAU { n } else { n + 1 };
    (0..count)
        .map(|i| c + DVec3::new((sweep * i as f64 / n as f64).cos(), (sweep * i as f64 / n as f64).sin(), 0.0) * r)
        .collect()
}

fn sample() -> Model {
    let mut m = Model::new();
    m.meta.name = "Cuisine".into();
    m.meta.description = "Plan de la cuisine".into();
    m.axes = DMat4::from_translation(DVec3::new(100.0, 200.0, 0.0)) * DMat4::from_rotation_z(0.3);
    // Materials.
    let brick = m.add_material("Brique", [150, 80, 60]);
    m.materials[brick].ext.texture = Some(texture("brique.png", 64, 32, 7, [500.0, 250.0]));
    m.materials[brick].color = [120, 90, 60];
    let tinted = m.add_material("Brique teintée", [40, 160, 200]);
    m.materials[tinted].ext.texture = Some(texture("brique2.png", 32, 32, 9, [300.0, 300.0]));
    // Colorized (SketchUp works the offsets out again from the colour and the image).
    m.materials[tinted].ext.colorize_deltas = [80.0, 0.3, 0.1];
    let steel = m.add_material("Acier", [180, 180, 190]);
    {
        let e = &mut m.materials[steel].ext;
        e.pbr.metalness_enabled = true;
        e.pbr.metallic_factor = 0.8;
        e.pbr.roughness_enabled = true;
        e.pbr.roughness_factor = 0.3;
        e.pbr.normal_enabled = true;
        e.pbr.normal_scale = 1.5;
        e.pbr.normal_texture = Some(texture("normale.png", 16, 16, 3, [254.0, 254.0]));
        e.texture = Some(texture("acier.png", 16, 16, 5, [254.0, 254.0]));
    }
    let glass = m.add_material("Verre", [200, 220, 255]);
    m.materials[glass].opacity = 0.4;
    m.attr_store.insert("mat:Brique".into(), all_kinds());
    // Tags, folders.
    let walls = m.add_layer("Murs");
    m.layers[walls].color = Some([200, 10, 10]);
    m.layers[walls].line_style = "Dash".into();
    let slabs = m.add_layer("Dalles");
    m.layers[slabs].page_behavior = 0x21;
    m.layers[slabs].visible = false;
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
    m.attr_store.insert(format!("layer:{}", "Murs"), all_kinds());
    // Faces: a box painted every way.
    let e = &mut m.root;
    e.add_polyline(
        &[DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, 600.0, 0.0), DVec3::new(0.0, 600.0, 0.0)],
        true,
        false,
    );
    let f = e.faces.keys().next().unwrap();
    e.push_pull(f, -400.0, false);
    let faces: Vec<_> = e.faces.keys().collect();
    for (i, f) in faces.iter().enumerate() {
        let n = e.faces[*f].normal;
        let p0 = e.verts[e.faces[*f].outer[0]].pos;
        let (ex, ey) = esq_core::meta::face_axes(n);
        let face = &mut e.faces[*f];
        face.layer = walls;
        match i {
            0 => face.front = Some(brick),
            1 => {
                face.front = Some(brick);
                let pairs = [
                    (p0, DVec2::new(0.3, 0.1)),
                    (p0 + ex * 400.0, DVec2::new(1.1, 0.9)),
                    (p0 + ey * 300.0, DVec2::new(-0.2, 1.4)),
                ];
                face.tex =
                    Some(Box::new(FaceTex { front: uv_from_pairs(n, [500.0, 250.0], &pairs), ..Default::default() }));
            }
            2 => {
                face.front = Some(tinted);
                let pairs = [
                    (p0, DVec2::new(0.0, 0.0)),
                    (p0 + ex * 400.0, DVec2::new(1.0, 0.0)),
                    (p0 + ex * 350.0 + ey * 300.0, DVec2::new(1.0, 1.0)),
                    (p0 + ex * 50.0 + ey * 300.0, DVec2::new(0.0, 1.0)),
                ];
                face.tex =
                    Some(Box::new(FaceTex { front: uv_from_pairs(n, [300.0, 300.0], &pairs), ..Default::default() }));
            }
            3 => {
                face.back = Some(brick);
                face.front = Some(steel);
                let pairs = [(p0, DVec2::new(0.5, 0.5)), (p0 + ex * 250.0, DVec2::new(1.5, 0.5))];
                face.tex =
                    Some(Box::new(FaceTex { back: uv_from_pairs(-n, [500.0, 250.0], &pairs), ..Default::default() }));
            }
            4 => face.front = Some(glass),
            _ => face.hidden = true,
        }
    }
    let (f0, f1) = (faces[0], faces[1]);
    use slotmap::Key;
    m.attr_store.insert(format!("h:f:0:{}", faces[1].data().as_ffi()), all_kinds());
    let edge = m.root.edges.keys().next().unwrap();
    m.attr_store.insert(format!("h:e:0:{}", edge.data().as_ffi()), all_kinds());
    let e = &mut m.root;
    e.set_prop(&format!("e{}", edge.data().as_ffi()), "material", AttrValue::Text("Verre".into()));
    e.set_prop(&format!("f{}", f1.data().as_ffi()), "casts", AttrValue::Bool(false));
    // Curves: a circle face, an arc, a curve, a polygon.
    e.add_polyline(&circle(DVec3::new(2000.0, 0.0, 0.0), 300.0, 24, std::f64::consts::TAU), true, true);
    e.add_polyline(&circle(DVec3::new(3000.0, 0.0, 0.0), 300.0, 8, 2.0), false, true);
    e.add_polyline(
        &[
            DVec3::new(4000.0, 0.0, 0.0),
            DVec3::new(4200.0, 300.0, 0.0),
            DVec3::new(4500.0, 250.0, 0.0),
            DVec3::new(4700.0, 600.0, 0.0),
        ],
        false,
        true,
    );
    // A component with its behaviour, an image.
    let items: Vec<_> = vec![esq_core::model::EntityRef::Face(f0)];
    let inst = m.make_group(None, &items, false, Some("Fenêtre".into())).unwrap();
    let def = m.root.instances[inst].def;
    m.defs[def].description = "Fenêtre à deux battants".into();
    m.defs[def].ext.behavior.cuts_opening = true;
    m.defs[def].ext.behavior.is2d = true;
    m.defs[def].ext.behavior.snapto = 2;
    m.defs[def].ext.behavior.no_scale_mask = 5;
    m.root.instances[inst].name = "Fenêtre cuisine".into();
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
    // A text with a dot arrow on a leader that follows the view; a dimension with open arrows.
    {
        use esq_core::model::{AnnotKind, Annotation};
        let t = m.root.annots.insert(Annotation {
            kind: AnnotKind::Text { point: DVec3::new(0.0, 0.0, 400.0), leader: DVec3::new(100.0, 0.0, 200.0) },
            text: "Plan de travail".into(),
            layer: 0,
            hidden: false,
            attach: Default::default(),
        });
        let d = m.root.annots.insert(Annotation {
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
        let e = &mut m.root;
        e.set_prop(&format!("a{}", t.data().as_ffi()), "arrow_type", AttrValue::Number(2.0));
        e.set_prop(&format!("a{}", t.data().as_ffi()), "leader_type", AttrValue::Number(1.0));
        e.set_prop(&format!("a{}", d.data().as_ffi()), "arrow_type", AttrValue::Number(4.0));
        e.set_prop(&format!("f{}", faces[4].data().as_ffi()), "receives", AttrValue::Bool(false));
    }
    // Model Info, rendering options, model dictionaries.
    m.meta.options.get_mut("UnitsOptions").unwrap().insert("AnglePrecision".into(), json!(3));
    m.meta.options.get_mut("PageOptions").unwrap().insert("TransitionTime".into(), json!(4.5));
    m.view_ext.insert(
        "ro".into(),
        json!({"DrawSilhouettes": false, "BackgroundColor": [10, 20, 30, 255], "EdgeColorMode": 2, "FogStartDist": 12.5}),
    );
    m.attr_store.insert("model".into(), all_kinds());
    // A scene that keeps its style and stays out of the animation.
    let mut s = Scene {
        name: "Face".into(),
        description: "Vue de face".into(),
        camera: SceneCamera {
            eye: DVec3::new(500.0, -5000.0, 1500.0),
            target: DVec3::new(500.0, 0.0, 200.0),
            up: DVec3::Z,
            fov_deg: 35.0,
            perspective: true,
        },
        hidden_layers: None,
        photo: None,
        ext: Default::default(),
    };
    s.ext.props.insert("flags".into(), json!(4095));
    s.ext.props.insert("ro".into(), json!({"DrawSilhouettes": true, "RenderMode": 1}));
    s.ext.props.insert("animate".into(), json!(false));
    m.scenes.push(s);
    m.ensure_scene_uids();
    let key = format!("h:g:0:{}", m.scenes[0].ext.uid);
    m.attr_store.insert(key, all_kinds());
    m
}

fn close(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

/// Written natively, read back by Esquisse and (when installed) by SketchUp's SDK.
#[test]
fn an_esquisse_model_goes_through_a_skp_unchanged() {
    let m = sample();
    let path = std::env::temp_dir().join(format!("esq-fidelite-{}.skp", std::process::id()));
    esq_skp::export_skp(&m, &path).unwrap();
    let back = esq_skp::import_skp(&path).unwrap();
    let sdk = esq_skp::available().then(|| esq_skp::sdk_import_skp_view(&path).unwrap().0);
    // ESQ_FIDELITE_SKP keeps a copy, to look at in SketchUp or in Esquisse.
    if let Ok(keep) = std::env::var("ESQ_FIDELITE_SKP") {
        let _ = std::fs::copy(&path, keep);
    }
    let _ = std::fs::remove_file(&path);
    for (who, back) in std::iter::once(("natif", back)).chain(sdk.map(|b| ("SDK", b))) {
        eprintln!("relecture : {who}");
        check(&m, &back);
    }
}

fn check(m: &Model, back: &Model) {
    // Model.
    assert_eq!((back.meta.name.as_str(), back.meta.description.as_str()), ("Cuisine", "Plan de la cuisine"));
    assert!(back.axes.abs_diff_eq(m.axes, 1e-6), "{:?}", back.axes);
    assert_eq!(back.attr_store["model"], all_kinds());
    assert_eq!(back.meta.options["UnitsOptions"]["AnglePrecision"], json!(3));
    assert_eq!(back.meta.options["PageOptions"]["TransitionTime"], json!(4.5));
    let ro = &back.view_ext["ro"];
    assert_eq!(ro["DrawSilhouettes"], json!(false));
    assert_eq!(ro["BackgroundColor"], json!([10, 20, 30, 255]));
    assert_eq!(ro["EdgeColorMode"], json!(2));
    assert_eq!(ro["FogStartDist"], json!(12.5));

    // Materials.
    let mat = |n: &str| &back.materials[back.find_material(n).unwrap_or_else(|| panic!("{n}"))];
    let brick = mat("Brique");
    let t = brick.ext.texture.as_ref().unwrap();
    let t0 = m.materials[m.find_material("Brique").unwrap()].ext.texture.as_ref().unwrap();
    assert_eq!((t.filename.as_str(), t.px, t.image.0.as_slice()), ("brique.png", [64, 32], t0.image.0.as_slice()));
    assert!((t.size[0] - 500.0).abs() < 1e-6 && (t.size[1] - 250.0).abs() < 1e-6, "{:?}", t.size);
    assert_eq!(brick.ext.colorize_deltas, [0.0; 3]);
    assert_eq!(back.attr_store["mat:Brique"], all_kinds());
    let tinted = mat("Brique teintée");
    assert_ne!(tinted.ext.colorize_deltas, [0.0; 3], "colorized");
    assert_eq!(tinted.color, [40, 160, 200]);
    let steel = mat("Acier");
    let p = &steel.ext.pbr;
    // SketchUp keeps the factors in single precision.
    assert!(p.metalness_enabled && (p.metallic_factor - 0.8).abs() < 1e-6, "{p:?}");
    assert!(p.roughness_enabled && (p.roughness_factor - 0.3).abs() < 1e-6, "{p:?}");
    assert!(p.normal_enabled && (p.normal_scale - 1.5).abs() < 1e-6, "{p:?}");
    assert_eq!(p.normal_texture.as_ref().unwrap().filename, "normale.png");
    assert_eq!(steel.ext.workflow, 1);
    assert!((mat("Verre").opacity - 0.4).abs() < 1e-6);

    // Tags and folders.
    let li = |n: &str| back.layers.iter().position(|l| l.name == n).unwrap();
    let walls = &back.layers[li("Murs")];
    assert_eq!((walls.color, walls.line_style.as_str()), (Some([200, 10, 10]), "Dash"));
    let slabs = &back.layers[li("Dalles")];
    assert_eq!((slabs.page_behavior, slabs.visible), (0x21, false));
    let folder = |uid: Option<u32>| back.meta.folders.iter().find(|f| Some(f.uid) == uid).unwrap();
    assert_eq!(folder(walls.folder).name, "Structure");
    let lvl = folder(slabs.folder);
    assert_eq!((lvl.name.as_str(), lvl.visible, lvl.visible_on_new_pages), ("Niveau 1", false, false));
    assert_eq!(folder(lvl.parent).name, "Structure");
    assert_eq!(back.meta.active_layer, walls.uid);
    assert_eq!(back.attr_store["layer:Murs"], all_kinds());

    // Faces and their textures, matched by their points.
    let sig = |ents: &esq_core::model::Entities, f: &esq_core::model::Face| {
        let mut k: Vec<[i64; 3]> = f
            .outer
            .iter()
            .map(|v| ents.verts[*v].pos)
            .map(|p| [p.x.round() as i64, p.y.round() as i64, p.z.round() as i64])
            .collect();
        k.sort();
        k
    };
    let mut checked = 0;
    for (_, f) in m.root.faces.iter() {
        let g = back.root.faces.values().find(|g| sig(&back.root, g) == sig(&m.root, f)).expect("face kept");
        assert_eq!(g.hidden, f.hidden);
        assert_eq!(back.layers[g.layer].name, m.layers[f.layer].name);
        let name = |mm: &Model, x: Option<_>| x.map(|x| mm.materials[x].name.clone());
        assert_eq!((name(back, g.front), name(back, g.back)), (name(m, f.front), name(m, f.back)));
        if let Some(t) = &f.tex {
            let u = g.tex.as_ref().expect("texture position kept");
            for (a, b) in [(&t.front, &u.front), (&t.back, &u.back)] {
                match (a, b) {
                    (Some(a), Some(b)) => {
                        // The same map, up to the homogeneous scale.
                        let pts: Vec<DVec3> = f.outer.iter().map(|v| m.root.verts[*v].pos).collect();
                        for p in pts {
                            assert!((a.at(p) - b.at(p)).length() < 1e-6, "{:?} {:?}", a.at(p), b.at(p));
                        }
                        assert_eq!(a.is_affine(), b.is_affine());
                        checked += 1;
                    }
                    (None, None) => {}
                    _ => panic!("{a:?} {b:?}"),
                }
            }
        }
    }
    assert_eq!(checked, 3);
    let _ = close;

    // Attributes of faces and edges, edge material, shadows.
    eprintln!("keys {:?}", back.attr_store.keys().collect::<Vec<_>>());
    let face_attrs = back.attr_store.iter().filter(|(k, _)| k.starts_with("h:f:0:")).count();
    let edge_attrs = back.attr_store.iter().filter(|(k, _)| k.starts_with("h:e:0:")).count();
    assert_eq!((face_attrs, edge_attrs), (1, 1));
    assert!(back.root.props.values().any(|p| p.get("material") == Some(&AttrValue::Text("Verre".into()))));
    assert!(back.root.props.values().any(|p| p.get("casts") == Some(&AttrValue::Bool(false))));
    assert!(back.root.props.values().any(|p| p.get("receives") == Some(&AttrValue::Bool(false))));
    let num = |k: &str, v: f64| back.root.props.values().any(|p| p.get(k) == Some(&AttrValue::Number(v)));
    assert!(num("arrow_type", 2.0) && num("leader_type", 1.0) && num("arrow_type", 4.0));

    // Curves.
    let mut ids: Vec<u32> = back.root.edges.values().filter_map(|e| e.curve).collect();
    ids.sort();
    ids.dedup();
    for c in &ids {
        let n = back.root.edges.values().filter(|e| e.curve == Some(*c)).count();
        eprintln!("curve {c}: {n} edges {:?}", back.root.props.get(&format!("c{c}")));
    }
    assert_eq!(ids.len(), 3, "circle, arc, curve");
    let arcs = ids.iter().filter(|c| esq_core::curves::arc_fit(&back.root, **c).is_some()).count();
    assert_eq!(arcs, 2);
    assert!(ids.iter().any(|c| back.root.prop(&format!("c{c}"), "kind") == Some(&AttrValue::Text("curve".into()))));

    // Component, image.
    let def = back.defs.values().find(|d| d.name == "Fenêtre").unwrap();
    assert_eq!(def.description, "Fenêtre à deux battants");
    let b = def.ext.behavior;
    assert!(b.cuts_opening && b.is2d && b.snapto == 2 && b.no_scale_mask == 5, "{b:?}");
    assert!(back.root.instances.values().any(|i| i.name == "Fenêtre cuisine"));
    let img = back.root.instances.iter().find_map(|(id, _)| back.image_of(None, id)).expect("image");
    assert_eq!(img.px, [40, 20]);
    assert!((img.width - 1200.0).abs() < 1e-6 && img.xaxis.distance(DVec3::new(1.0, 1.0, 0.0).normalize()) < 1e-9);
    assert_eq!(img.path, "plan.png");

    // Scene.
    let s = &back.scenes[0];
    assert_eq!(s.ext.props["flags"], json!(4095));
    assert_eq!(s.ext.props["ro"]["DrawSilhouettes"], json!(true));
    assert_eq!(s.ext.props["ro"]["RenderMode"], json!(1));
    assert_eq!(s.ext.props["animate"], json!(false));
    assert_eq!(back.attr_store[&format!("h:g:0:{}", s.ext.uid)], all_kinds());
}
