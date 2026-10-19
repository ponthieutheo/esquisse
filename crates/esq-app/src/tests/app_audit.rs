//! Audit of the application's checked entries: what was hollow, pinned
//! once fixed.

use esq_core::glam::DVec3;
use esq_core::model::{AttrValue, EntityRef, FaceId, Model};

/// A horizontal square [x0, x1]² at height z, facing up.
fn square(m: &mut Model, x0: f64, x1: f64, z: f64) -> FaceId {
    let v: Vec<_> = [(x0, x0), (x1, x0), (x1, x1), (x0, x1)]
        .iter()
        .map(|(x, y)| m.root.add_vertex_raw(DVec3::new(*x, *y, z)))
        .collect();
    m.root.add_face_raw(v, vec![]).expect("face")
}

fn face_key(f: FaceId) -> String {
    format!("f{}", slotmap::Key::data(&f).as_ffi())
}

/// Entity Info › Receive Shadows (`Drawingelement#receives_shadows=`) off: the shadow of the
/// roof no longer darkens the ground, which stays as lit as where no shadow falls.
#[test]
fn receive_shadows_off_keeps_cast_shadows_off_the_face() {
    let mut m = Model::new();
    let ground = square(&mut m, 0.0, 2000.0, 0.0);
    square(&mut m, 700.0, 1300.0, 500.0);
    let style = crate::style::ViewStyle::default();
    // Scene bits: the face that does not receive is marked, the others are not.
    let opts = crate::scene::SceneOptions {
        ctx: &[],
        ctx_def: None,
        sel: &[],
        axes_xform: esq_core::glam::DMat4::IDENTITY,
        style: &style,
    };
    let marked = |m: &Model| {
        let mesh = crate::scene::build(m, &opts);
        mesh.opaque.iter().filter(|v| v.tex & crate::scene::NO_RECEIVE != 0).count()
    };
    assert_eq!(marked(&m), 0);
    let mut off = m.clone();
    off.root.set_prop(&face_key(ground), "receives", AttrValue::Bool(false));
    assert!(marked(&off) > 0);
    // On the GPU: the sun at 45° casts the roof's shadow 500 mm towards -x.
    let Some(mut g) = super::gpu::Gpu::new() else { return };
    let cam = super::gpu::top_camera(2000.0, 96.0);
    let lighting = crate::render::Lighting {
        sun: Some(DVec3::new(1.0, 0.0, 1.0).normalize()),
        light: 0.8,
        dark: 0.45,
        cast: true,
        on_faces: true,
        on_ground: false,
    };
    let lum = |c: [u8; 3]| c.iter().map(|v| *v as u32).sum::<u32>();
    let shot = g.render(&m, &style, &cam, &lighting);
    let (shade, lit) =
        (shot.at_point(&cam, DVec3::new(400.0, 1000.0, 0.0)), shot.at_point(&cam, DVec3::new(1700.0, 1000.0, 0.0)));
    assert!(lum(shade) + 60 < lum(lit), "the roof's shadow on the ground: {shade:?} / {lit:?}");
    let shot = g.render(&off, &style, &cam, &lighting);
    let (shade, lit) =
        (shot.at_point(&cam, DVec3::new(400.0, 1000.0, 0.0)), shot.at_point(&cam, DVec3::new(1700.0, 1000.0, 0.0)));
    assert!(lum(shade).abs_diff(lum(lit)) <= 6, "no shadow on a face that does not receive: {shade:?} / {lit:?}");
}

/// A group that does not receive shadows: nothing inside it does.
#[test]
fn receive_shadows_off_on_a_group_covers_its_faces() {
    let Some(interp) = super::ruby::interp() else { return };
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut a = crate::app::EsquisseApp::bare(None, None);
    let o = crate::ruby_host::run_script(&mut a, "Script Ruby", |m, s| {
        interp.run(
            "ents = Sketchup.active_model.entities
             f = ents.add_face([0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0]); f.receives_shadows = false
             ents.add_face([20, 0, 0], [30, 0, 0], [30, 10, 0], [20, 10, 0])
             g = ents.add_group; g.entities.add_face([40, 0, 0], [50, 0, 0], [50, 10, 0], [40, 10, 0])
             g.receives_shadows = false",
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
    let off = |x0: f32, x1: f32| {
        let v: Vec<_> = mesh.opaque.iter().filter(|v| v.pos[0] >= x0 && v.pos[0] <= x1).collect();
        assert!(!v.is_empty());
        v.iter().all(|v| v.tex & crate::scene::NO_RECEIVE != 0)
    };
    assert!(off(0.0, 254.0), "the face that does not receive");
    assert!(!off(508.0, 762.0), "an ordinary face receives");
    assert!(off(1016.0, 1270.0), "the faces of a group that does not receive");
}

/// Window › Entity Info drawn in the test window.
fn info_driver() -> super::ui_driver::Driver {
    let mut ui = super::ui_driver::Driver::new();
    ui.side = Some(|app, ui| {
        crate::entity_info::show(app, ui);
    });
    ui
}

/// Entity Info had only the tag and a name (`ui::entity_info`): the toggles of SketchUp's base
/// page now hide, lock and switch the shadows, each an operation named as in SketchUp.
#[test]
fn entity_info_toggles_hide_and_switch_the_shadows_with_undo() {
    let mut a = super::ui_driver::app();
    let mut f = None;
    a.doc.commit("Face", |m, _| f = Some(square(m, 0.0, 1000.0, 0.0)));
    let f = f.unwrap();
    a.doc.select(vec![EntityRef::Face(f)]);
    let mut ui = info_driver();
    ui.frames(&mut a, 2);
    assert!(ui.has("Surface :"), "the area of a face");
    assert!(!ui.has("Verrouillé"), "a face has no lock");
    assert_eq!(ui.toggled("Recevoir des ombres"), Some(true));
    ui.click(&mut a, "Recevoir des ombres");
    assert_eq!(a.doc.ents().prop(&face_key(f), "receives"), Some(&AttrValue::Bool(false)));
    assert_eq!(a.doc.undo_label(), Some("Ne pas recevoir d'ombres"));
    ui.click(&mut a, "Projeter des ombres");
    assert_eq!(a.doc.ents().prop(&face_key(f), "casts"), Some(&AttrValue::Bool(false)));
    assert_eq!(a.doc.undo_label(), Some("Ne pas projeter d'ombres"));
    a.doc.undo();
    a.doc.undo();
    assert!(a.doc.ents().props.is_empty(), "Ctrl+Z brings both back");
    a.doc.select(vec![EntityRef::Face(f)]);
    ui.frames(&mut a, 2);
    ui.click(&mut a, "Caché");
    assert!(a.doc.ents().faces[f].hidden);
    assert_eq!(a.doc.undo_label(), Some("Masquer"));
    ui.click(&mut a, "Caché");
    assert!(!a.doc.ents().faces[f].hidden);
    assert_eq!(a.doc.undo_label(), Some("Révéler"));
}

/// Edges: their length, and Soft / Smooth; a circle also shows its radius and segments.
#[test]
fn entity_info_softens_smooths_edges_and_reads_a_circle() {
    let mut a = super::ui_driver::app();
    let mut f = None;
    a.doc.commit("Face", |m, _| f = Some(square(m, 0.0, 1000.0, 0.0)));
    let e = a.doc.ents().face_edges(f.unwrap())[0];
    a.doc.select(vec![EntityRef::Edge(e)]);
    let mut ui = info_driver();
    ui.frames(&mut a, 2);
    assert!(ui.has("Longueur :") && !ui.has("Rayon:"));
    ui.click(&mut a, "Adouci :");
    assert!(a.doc.ents().edges[e].soft && !a.doc.ents().edges[e].smooth);
    assert_eq!(a.doc.undo_label(), Some("Adoucir"));
    ui.click(&mut a, "Lisser :");
    assert!(a.doc.ents().edges[e].smooth);
    a.doc.undo();
    a.doc.undo();
    assert!(!a.doc.ents().edges[e].soft);
    // A circle of 24 segments, radius 500 mm.
    let mut b = super::ui_driver::app();
    b.doc.commit("Cercle", |m, d| {
        let e = m.entities_mut(d);
        let v: Vec<_> = (0..24)
            .map(|i| {
                let t = i as f64 * std::f64::consts::TAU / 24.0;
                e.add_vertex_raw(DVec3::new(500.0 * t.cos(), 500.0 * t.sin(), 0.0))
            })
            .collect();
        for i in 0..24 {
            let x = e.add_edge_raw(v[i], v[(i + 1) % 24]).unwrap();
            e.edges[x].curve = Some(7);
        }
    });
    let edges: Vec<EntityRef> = b.doc.ents().edges.keys().map(EntityRef::Edge).collect();
    b.doc.select(edges);
    ui.frames(&mut b, 2);
    assert!(ui.has("Rayon:") && ui.has("Segments :"));
    assert_eq!(ui.value("Segments :"), "24", "the number of segments");
}

/// A component: instance and definition names, and the advanced attributes, kept by the .esq
/// file (and by the .skp when SketchUp's API is there to write it).
#[test]
fn entity_info_names_a_component_and_sets_its_advanced_attributes() {
    let mut a = super::ui_driver::app();
    let mut f = None;
    a.doc.commit("Face", |m, _| f = Some(square(m, 0.0, 1000.0, 0.0)));
    a.doc.select(vec![EntityRef::Face(f.unwrap())]);
    a.make_group(false);
    let [EntityRef::Instance(i)] = a.doc.sel.as_slice() else { panic!("un composant") };
    let i = *i;
    let mut ui = info_driver();
    ui.frames(&mut a, 2);
    ui.type_into(&mut a, "Définition:", "Chaise");
    ui.type_into(&mut a, "Instance :", "Chaise 1");
    ui.click(&mut a, "Attributs avancés :");
    ui.type_into(&mut a, "Prix :", "120,5");
    ui.type_into(&mut a, "Propriétaire :", "Atelier");
    ui.frames(&mut a, 2);
    let inst = a.doc.ents().instances[i].clone();
    let def = &a.doc.model.defs[inst.def];
    assert_eq!((def.name.as_str(), inst.name.as_str()), ("Chaise", "Chaise 1"));
    assert_eq!(def.attributes["SU_DefinitionSet"]["Price"], AttrValue::Number(120.5));
    assert_eq!(inst.attributes["SU_InstanceSet"]["Owner"], AttrValue::Text("Atelier".into()));
    // Locked: everything greys out but the lock.
    ui.click(&mut a, "Verrouillé");
    assert!(a.doc.ents().instances[i].locked);
    assert_eq!(a.doc.undo_label(), Some("Verrouiller"));
    assert!(ui.node("Caché").disabled && !ui.node("Verrouillé").disabled);
    // Saved and read back.
    let dir = std::env::temp_dir().join(format!("esq_ei_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let check = |m: &Model, what: &str| {
        let (_, inst) = m.root.instances.iter().next().expect(what);
        let def = &m.defs[inst.def];
        assert_eq!((def.name.as_str(), inst.name.as_str(), inst.locked), ("Chaise", "Chaise 1", true), "{what}");
        assert_eq!(def.attributes["SU_DefinitionSet"]["Price"], AttrValue::Number(120.5), "{what}");
        assert_eq!(inst.attributes["SU_InstanceSet"]["Owner"], AttrValue::Text("Atelier".into()), "{what}");
    };
    let p = dir.join("chaise.esq");
    esq_core::io::save_esq(&a.doc.model, &p).unwrap();
    check(&esq_core::io::load_esq(&p).unwrap(), ".esq");
    // The .skp is written and read by Esquisse's own writer and reader (no SketchUp needed).
    let p = dir.join("chaise.skp");
    esq_skp::export_skp(&a.doc.model, &p).unwrap();
    check(&esq_skp::import_skp(&p).unwrap(), ".skp");
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the audit's fixes set survives a .skp written and read back by Esquisse's native
/// writer and reader: the shadow flags of a face and of a group, soft / smooth / hidden edges,
/// a hidden instance, the alignment and description a component gets from « Créer un
/// composant », and the perspective map of a face painted by « Projeter la photo ». What is
/// lost is known (open, .skp domain).
#[test]
fn what_the_audit_sets_survives_a_native_skp() {
    let mut a = super::ui_driver::app();
    let (mut f1, mut f2) = (None, None);
    a.doc.commit("Faces", |m, _| {
        f1 = Some(square(m, 0.0, 1000.0, 0.0));
        f2 = Some(square(m, 2000.0, 2500.0, 0.0));
    });
    let (f1, f2) = (f1.unwrap(), f2.unwrap());
    // Face: does not receive; one edge soft and smooth, another hidden.
    let e = a.doc.ents().face_edges(f1);
    a.doc.commit("réglages", |m, _| {
        m.root.set_prop(&face_key(f1), "receives", AttrValue::Bool(false));
        m.root.edges[e[0]].soft = true;
        m.root.edges[e[0]].smooth = true;
        m.root.edges[e[1]].hidden = true;
    });
    // A photo projected on the face.
    let mut scene = super::outils::photo_scene("Façade", DVec3::new(500.0, -3000.0, 2500.0));
    let mut png = Vec::new();
    let img = image::RgbaImage::from_pixel(8, 6, image::Rgba([200, 120, 40, 255]));
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    scene.photo.as_mut().unwrap().image = esq_core::photo::Blob(std::sync::Arc::new(png));
    a.doc.model.scenes.push(scene);
    a.active_scene = Some(0);
    a.doc.select(vec![EntityRef::Face(f1)]);
    crate::ctx_dialogs::project(&mut a, true);
    let probe = DVec3::new(300.0, 700.0, 0.0);
    let side_map = |m: &Model, f: FaceId| {
        let face = &m.root.faces[f];
        let t = face.tex.as_ref().expect("a texture map");
        t.front.or(t.back).expect("a side mapped")
    };
    let uv = side_map(&a.doc.model, f1).at(probe);
    // A component made by the dialog's settings, hidden, its group not casting.
    a.doc.select(vec![EntityRef::Face(f2)]);
    a.run(&eframe::egui::Context::default(), crate::commands::Cmd::MakeComponent);
    let mut ui = super::ui_driver::Driver::new();
    ui.frames(&mut a, 3);
    ui.type_into(&mut a, "Description:", "Porte");
    ui.choose(&mut a, "Liaison", "Vertical");
    ui.click(&mut a, "Découper l’ouverture");
    ui.click(&mut a, "Créer");
    let [EntityRef::Instance(i)] = a.doc.sel.as_slice() else { panic!("composant") };
    let i = *i;
    a.doc.commit("réglages", |m, _| {
        m.root.instances[i].hidden = true;
        let k = format!("i{}", slotmap::Key::data(&i).as_ffi());
        m.root.set_prop(&k, "casts", AttrValue::Bool(false));
    });
    let dir = std::env::temp_dir().join(format!("esq_audit_skp_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("audit.skp");
    esq_skp::export_skp(&a.doc.model, &p).unwrap();
    let b = esq_skp::import_skp(&p).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    // The face that does not receive: the only one left at the root.
    assert_eq!(b.root.faces.len(), 1);
    let g1 = b.root.faces.keys().next().expect("the face");
    assert_eq!(b.root.prop(&face_key(g1), "receives"), Some(&AttrValue::Bool(false)), "receives shadows");
    assert!(b.root.edges.values().any(|e| e.soft && e.smooth), "soft and smooth edge");
    assert!(b.root.edges.values().any(|e| e.hidden), "hidden edge");
    let (bi, inst) = b.root.instances.iter().next().expect("the component");
    assert!(inst.hidden, "hidden instance");
    let k = format!("i{}", slotmap::Key::data(&bi).as_ffi());
    assert_eq!(b.root.prop(&k, "casts"), Some(&AttrValue::Bool(false)), "casts shadows");
    let d = &b.defs[inst.def];
    assert_eq!(d.description, "Porte");
    assert!(d.ext.behavior.is2d && d.ext.behavior.snapto == 2 && d.ext.behavior.cuts_opening, "{:?}", d.ext.behavior);
    // The projected photo: the same pixel at the same point.
    let back = side_map(&b, g1).at(probe);
    assert!(back.distance(uv) < 1e-6, "projected map: {back:?} / {uv:?}");
}

/// A model whose component says it was loaded from `\\127.0.0.1\x\y` (anyone can write that in
/// a file): drawing the panel reads nothing — no thumbnail looked for beside that path, which
/// would make Windows connect to the server and hand it the user's NTLM hash — and loading
/// takes only the files the collections listed.
#[test]
fn components_panel_never_reads_a_path_written_in_the_model() {
    let mut a = super::ui_driver::app();
    let mut f = None;
    a.doc.commit("Face", |m, _| f = Some(square(m, 0.0, 1000.0, 0.0)));
    a.doc.select(vec![EntityRef::Face(f.unwrap())]);
    a.make_group(false);
    let [EntityRef::Instance(i)] = a.doc.sel.as_slice() else { panic!("un composant") };
    let d = a.doc.ents().instances[*i].def;
    a.doc.model.defs[d].ext.path = r"\\127.0.0.1\x\y".into();
    let mut ui = super::ui_driver::Driver::new();
    ui.side = Some(|app, ui| {
        crate::comp_library::show(app, ui);
    });
    let before = crate::comp_library::disk_reads();
    let _ = crate::safe_path::asked();
    ui.frames(&mut a, 30);
    let name = a.doc.model.defs[d].name.clone();
    assert!(ui.has(&name), "the model's component {name} is shown");
    assert_eq!(crate::comp_library::disk_reads(), before, "no file read while the panel draws");
    assert!(crate::safe_path::asked().is_empty(), "no path of the model asked for");
    // A path that no collection listed is not loaded, whatever the model says.
    let err = crate::comp_library::load_into(&mut a, std::path::Path::new(r"\\127.0.0.1\x\y")).unwrap_err();
    assert!(err.contains("hors des collections"), "{err}");
    assert_eq!(crate::comp_library::disk_reads(), before);
    // The same rule for the other paths a model holds: an environment's picture.
    assert!(crate::safe_path::read(r"\\127.0.0.1\x\ciel.hdr").is_none());
    // And the counter sees every read of the panel: it reads files at one place only.
    let src = include_str!("../comp_library.rs");
    assert_eq!(src.matches("fs::read(").count(), 1, "comp_library reads files only through read_file");
}

/// Window › Components browsed only the model's components: it now browses the collections of
/// component files as SketchUp does, starting with the library that comes with Esquisse; a
/// click loads the file once and places the component.
#[test]
fn components_panel_browses_the_library_loads_once_and_places() {
    let mut a = super::ui_driver::app();
    let mut ui = super::ui_driver::Driver::new();
    ui.side = Some(|app, ui| {
        crate::comp_library::show(app, ui);
    });
    ui.frames(&mut a, 2);
    assert!(ui.has("Aucun composant dans le modèle."));
    ui.choose(&mut a, "Collection de composants", "Échantillon de composants");
    for name in ["Chaise", "Table", "Porte intérieure", "Fenêtre", "Personnage 2D"] {
        assert!(ui.has(name), "{name} dans l'échantillon");
    }
    ui.click(&mut a, "Chaise");
    let defs: Vec<_> = a.doc.model.defs.iter().filter(|(_, d)| d.name == "Chaise").map(|(k, _)| k).collect();
    assert_eq!(defs.len(), 1, "the file is loaded as a definition");
    let d = defs[0];
    assert!(a.doc.model.defs[d].ext.path.ends_with("Chaise.esq"));
    assert!(!a.doc.model.defs[d].description.is_empty(), "with its description");
    assert_eq!(a.tool.id(), crate::tools::ToolId::Component, "and it is being placed");
    assert_eq!(a.doc.undo_label(), Some("Charger le composant"));
    // A second click takes the same definition again.
    a.set_tool(crate::tools::ToolId::Select);
    ui.click(&mut a, "Chaise");
    assert_eq!(a.doc.model.defs.values().filter(|d| d.name == "Chaise").count(), 1);
    // The door keeps the alignment saved in its file: glued to walls, cutting its opening.
    ui.click(&mut a, "Porte intérieure");
    let door = a.doc.model.defs.values().find(|d| d.name == "Porte intérieure").expect("porte");
    assert!(door.ext.behavior.is2d && door.ext.behavior.cuts_opening && door.ext.behavior.snapto == 2);
    // In the model, then the statistics of the chair: 6 boxes.
    ui.click(&mut a, "Dans le modèle");
    assert_eq!(ui.value("Collection de composants"), "Dans le modèle");
    assert!(ui.has("Chaise") && ui.has("Porte intérieure"));
    a.ui_state.extra.dialogs.comp_edit.def = Some(d);
    ui.click(&mut a, "Statistiques");
    let rows = crate::comp_library::component_statistics(&a.doc.model, d, false);
    assert_eq!(rows[1], ("Faces", 36));
    assert!(ui.has("36"));
}

/// Ruby's `Materials#load` read only Esquisse's own JSON file and `Material#save_as` wrote it:
/// both now go through SketchUp's .skm format (`skm.rs`), the JSON still read.
#[test]
fn ruby_materials_load_and_save_as_use_real_skm_files() {
    let Some(interp) = super::ruby::interp() else { return };
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("esq_skm_rb_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A .skm written as SketchUp writes it: a brick texture of 4 × 3 pixels, 2 m wide.
    let mut png = Vec::new();
    let img = image::RgbaImage::from_pixel(4, 3, image::Rgba([170, 80, 60, 255]));
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let mut brick = esq_core::model::Material {
        name: "Brique".into(),
        color: [170, 80, 60],
        opacity: 1.0,
        ext: Default::default(),
    };
    brick.ext.texture = Some(crate::mat_panel::texture_from_bytes("brique.png", png).unwrap());
    crate::mat_panel::set_texture_size(&mut brick, Some(2000.0), None, true);
    let skm = dir.join("Brique.skm");
    std::fs::write(&skm, crate::skm::write(&brick, None)).unwrap();
    let out = dir.join("copie.skm");
    let rb = |p: &std::path::Path| p.to_string_lossy().replace('\\', "/");
    let mut a = crate::app::EsquisseApp::bare(None, None);
    let mut result = String::new();
    let o = crate::ruby_host::run_script(&mut a, "Script Ruby", |m, s| {
        let r = interp.run(
            &format!(
                "ms = Sketchup.active_model.materials
                 b = ms.load('{}')
                 ok = b.save_as('{}')
                 [b.name, b.texture.filename, b.texture.width.round, ok]",
                rb(&skm),
                rb(&out)
            ),
            "t.rb",
            m,
            s,
        );
        if let Ok(o) = &r {
            result = o.result.clone();
        }
        r
    })
    .unwrap();
    assert!(o.ok, "{}", o.error);
    // Width in inches: 2000 mm.
    assert_eq!(result, format!("[\"Brique\", \"brique.png\", {}, true]", (2000.0f64 / 25.4).round() as i64));
    let saved = std::fs::read(&out).unwrap();
    assert!(saved.starts_with(b"PK"), "a .skm is a ZIP");
    let back = crate::skm::read_file(&out).unwrap();
    assert_eq!(back.name, "Brique");
    assert!(back.ext.texture.as_ref().is_some_and(|t| (t.size[0] - 2000.0).abs() < 0.5));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Window › Materials › details menu: Save collection as writes the model's materials as .skm
/// files that the panel reads back as a collection; Purge Unused removes those that paint
/// nothing (one operation).
#[test]
fn materials_collection_is_saved_and_purged() {
    let mut a = super::ui_driver::app();
    let mut f = None;
    a.doc.commit("Faces", |m, _| {
        let used = m.add_material("Brique", [170, 80, 60]);
        m.add_material("Inutile", [10, 200, 10]);
        f = Some(square(m, 0.0, 1000.0, 0.0));
        m.root.faces[f.unwrap()].front = Some(used);
    });
    let dir = std::env::temp_dir().join(format!("esq_matcoll_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(crate::mat_panel::save_collection(&a.doc.model, &dir), 2);
    let back: Vec<String> = crate::skm::read_folder(&dir).into_iter().map(|m| m.name).collect();
    assert_eq!(back, vec!["Brique".to_string(), "Inutile".to_string()]);
    assert_eq!(crate::mat_panel::purge_materials(&mut a), 1);
    let names: Vec<&str> = a.doc.model.materials.values().map(|m| m.name.as_str()).collect();
    assert_eq!(names, vec!["Brique"]);
    assert_eq!(a.doc.undo_label(), Some("Purger les matières"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Window › Materials had no right-click menu on the model's materials:
/// Delete gives the faces back the default material (undoable), Area adds up the faces it
/// paints (in groups too), Select picks what it paints in the open context.
#[test]
fn materials_menu_deletes_measures_and_selects() {
    let mut a = super::ui_driver::app();
    let (mut f1, mut f2) = (None, None);
    let mut mat = None;
    a.doc.commit("Faces", |m, _| {
        let id = m.add_material("Brique", [170, 80, 60]);
        f1 = Some(square(m, 0.0, 1000.0, 0.0));
        f2 = Some(square(m, 2000.0, 2500.0, 0.0));
        m.root.faces[f1.unwrap()].front = Some(id);
        mat = Some(id);
    });
    let (f1, mat) = (f1.unwrap(), mat.unwrap());
    // A group holding a painted face of 500 × 500.
    a.doc.select(vec![EntityRef::Face(f2.unwrap())]);
    a.make_group(true);
    let [EntityRef::Instance(g)] = a.doc.sel.as_slice() else { panic!("groupe") };
    let gd = a.doc.ents().instances[*g].def;
    a.doc.commit("peindre", |m, _| {
        for f in m.defs[gd].entities.faces.values_mut() {
            f.back = Some(mat);
        }
    });
    let area = crate::mat_panel::material_area(&a.doc.model, mat);
    assert!((area - (1000.0 * 1000.0 + 500.0 * 500.0)).abs() < 1e-6, "{area}");
    crate::mat_panel::select_painted(&mut a, mat);
    assert_eq!(a.doc.sel, vec![EntityRef::Face(f1)], "the open context only");
    crate::mat_panel::delete_material(&mut a, mat);
    assert!(a.doc.model.materials.get(mat).is_none() && a.doc.ents().faces[f1].front.is_none());
    assert!(a.doc.model.defs[gd].entities.faces.values().all(|f| f.back.is_none()));
    assert_eq!(a.doc.undo_label(), Some("Supprimer la matière"));
    a.doc.undo();
    assert_eq!(a.doc.ents().faces[f1].front, Some(mat));
}

/// Window › Shadows › Display › From edges had no effect: an edge that bounds no face now
/// casts its thin shadow on the ground face, only while the option is on.
#[test]
fn shadows_from_edges_cast_the_shadow_of_a_lone_edge() {
    let mut m = Model::new();
    let v: Vec<_> = [(-3000.0, -3000.0), (3000.0, -3000.0), (3000.0, 3000.0), (-3000.0, 3000.0)]
        .iter()
        .map(|(x, y)| m.root.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
        .collect();
    m.root.add_face_raw(v, vec![]).unwrap();
    // A wire 500 mm up, along red.
    let (a, b) =
        (m.root.add_vertex_raw(DVec3::new(-300.0, 20.0, 500.0)), m.root.add_vertex_raw(DVec3::new(900.0, 20.0, 500.0)));
    m.root.add_edge_raw(a, b).unwrap();
    m.shadow.display = true;
    let style = crate::style::ViewStyle::default();
    let opts = crate::scene::SceneOptions {
        ctx: &[],
        ctx_def: None,
        sel: &[],
        axes_xform: esq_core::glam::DMat4::IDENTITY,
        style: &style,
    };
    let ribbons =
        |m: &Model| crate::scene::build(m, &opts).opaque.iter().filter(|v| v.tex == crate::scene::SHADOW_ONLY).count();
    assert_eq!(ribbons(&m), 0, "off: no shadow from edges");
    let mut on = m.clone();
    on.shadow.from_edges = true;
    assert_eq!(ribbons(&on), 6, "on: one ribbon (two triangles) for the wire");
    // On the GPU, lit by the model's own sun: the ribbon darkens the ground under the wire.
    let Some(mut g) = super::gpu::Gpu::new() else { return };
    let lighting = crate::render::Lighting {
        sun: Some(on.shadow.sun_direction()),
        light: 0.8,
        dark: 0.45,
        cast: true,
        on_faces: true,
        on_ground: false,
    };
    let cam = super::gpu::top_camera(40.0, 64.0);
    let lum = |c: [u8; 3]| c.iter().map(|v| *v as u32).sum::<u32>();
    let dir = on.shadow.sun_direction();
    // Where the wire's shadow falls with the model's own sun (through the wire's middle).
    let mid = DVec3::new(300.0, 20.0, 500.0);
    let ground = mid - dir * (mid.z / dir.z);
    let cam = crate::camera::Camera { target: DVec3::new(ground.x, ground.y, 0.0), ..cam };
    let lit = lum(g.render(&m, &style, &cam, &lighting).at(32, 32));
    let shaded = lum(g.render(&on, &style, &cam, &lighting).at(32, 32));
    assert!(shaded + 40 < lit, "the wire's shadow: {shaded} / {lit}");
}

/// Styles › Edit › Modeling: « Verrouillé » (the colour of a selected locked group) and the
/// matched photo's background (shown or not, its opacity) were kept without effect.
#[test]
fn style_locked_colour_and_photo_background_take_effect() {
    let mut a = super::ui_driver::app();
    let mut f = None;
    a.doc.commit("Face", |m, _| f = Some(square(m, 0.0, 1000.0, 0.0)));
    a.doc.select(vec![EntityRef::Face(f.unwrap())]);
    a.make_group(true);
    let [EntityRef::Instance(g)] = a.doc.sel.as_slice() else { panic!("groupe") };
    let g = *g;
    let colours = |a: &crate::app::EsquisseApp| {
        let sel = a.doc.sel.clone();
        let opts = crate::scene::SceneOptions {
            ctx: &[],
            ctx_def: None,
            sel: &sel,
            axes_xform: esq_core::glam::DMat4::IDENTITY,
            style: &a.style,
        };
        let mesh = crate::scene::build(&a.doc.model, &opts);
        mesh.lines.iter().map(|l| [l.color[0], l.color[1], l.color[2]]).collect::<Vec<_>>()
    };
    let rgb = |c: [u8; 3]| c.map(|v| v as f32 / 255.0);
    assert!(colours(&a).contains(&rgb(a.style.highlight_color)));
    a.set_locked(true, false);
    assert!(a.doc.ents().instances[g].locked);
    let locked = colours(&a);
    assert!(locked.contains(&rgb(a.style.locked_color)) && !locked.contains(&rgb(a.style.highlight_color)));
    // The photo under a transparent render: as is, at half opacity over the background, or not.
    let png = crate::photo_img::tests::png_bytes(4, 4);
    let photo = image::load_from_memory(&png).unwrap().to_rgba8();
    a.cam.size = esq_core::glam::DVec2::new(4.0, 4.0);
    a.photo_shown = Some((esq_core::photo::Blob(std::sync::Arc::new(png)), [0.0, 0.0, 4.0, 4.0]));
    let px = |a: &crate::app::EsquisseApp| a.photo_composite(4, 4, vec![0; 64])[0..3].to_vec();
    let p = photo.get_pixel(0, 0);
    assert_eq!(px(&a), p.0[..3].to_vec());
    a.style.photo_bg_opacity = 0.5;
    let bg = a.style.bg_color;
    let half: Vec<u8> = (0..3).map(|k| (bg[k] as f32 * 0.5 + p[k] as f32 * 0.5) as u8).collect();
    assert_eq!(px(&a), half);
    a.style.photo_bg = false;
    assert_eq!(px(&a), bg.to_vec());
}

/// Window › Outliner was a bare tree: SketchUp's names (« Groupe », « <Définition> »), the
/// « Filtre : » field, the eye that hides an entry, and a click that opens the group holding
/// the entry to select it.
#[test]
fn outliner_names_filters_hides_and_selects_through_groups() {
    let mut a = super::ui_driver::app();
    let (mut f1, mut f2) = (None, None);
    a.doc.commit("Faces", |m, _| {
        f1 = Some(square(m, 0.0, 1000.0, 0.0));
        f2 = Some(square(m, 2000.0, 2500.0, 0.0));
    });
    // A component « Chaise » inside a group.
    a.doc.select(vec![EntityRef::Face(f1.unwrap())]);
    a.make_group(false);
    let [EntityRef::Instance(c)] = a.doc.sel.as_slice() else { panic!("composant") };
    let c = *c;
    let d = a.doc.ents().instances[c].def;
    a.doc.commit("nom", |m, _| m.defs[d].name = "Chaise".into());
    a.doc.select(vec![EntityRef::Instance(c), EntityRef::Face(f2.unwrap())]);
    a.make_group(true);
    let [EntityRef::Instance(g)] = a.doc.sel.as_slice() else { panic!("groupe") };
    let g = *g;
    a.doc.select(Vec::new());
    let mut ui = super::ui_driver::Driver::new();
    ui.side = Some(crate::outliner_panel::show);
    ui.frames(&mut a, 2);
    assert!(ui.has("Groupe"), "an unnamed group shows as « Groupe »");
    // The filter keeps the chair and the group that leads to it.
    ui.type_into(&mut a, "Filtre :", "chaise");
    ui.frames(&mut a, 2);
    assert!(ui.has("<Chaise>") && ui.has("Groupe"));
    ui.type_into(&mut a, "Filtre :", "rien");
    ui.frames(&mut a, 2);
    assert!(!ui.has("<Chaise>") && !ui.has("Groupe"));
    ui.focus(&mut a, "Filtre :");
    ui.key(&mut a, eframe::egui::Key::A, eframe::egui::Modifiers::COMMAND);
    ui.key(&mut a, eframe::egui::Key::Backspace, eframe::egui::Modifiers::NONE);
    ui.frames(&mut a, 2);
    // The eye hides the group, one operation.
    ui.click(&mut a, "Masquer Groupe");
    assert!(a.doc.ents().instances[g].hidden);
    assert_eq!(a.doc.undo_label(), Some("Masquer"));
    ui.click(&mut a, "Révéler Groupe");
    assert!(!a.doc.ents().instances[g].hidden);
    // A click on the chair opens the group and selects it.
    ui.click(&mut a, "☰");
    ui.click(&mut a, "Tout développer");
    ui.frames(&mut a, 2);
    ui.click(&mut a, "<Chaise>");
    let inner = a.doc.model.root.instances[g].def;
    assert_eq!(a.doc.ctx, vec![g]);
    assert!(a.doc.model.defs[inner].entities.instances.values().any(|i| i.def == d));
    assert!(matches!(a.doc.sel.as_slice(), [EntityRef::Instance(_)]));
}

/// Edit commands no test named: Copy keeps only what was copied, Paste
/// In Place puts it back where it was, Cut removes it, Select None, Hide / Reveal Last /
/// Reveal All, Lock / Unlock All act as SketchUp's, each one an operation undone by Ctrl+Z.
#[test]
fn edit_commands_copy_cut_paste_hide_and_lock() {
    use crate::commands::Cmd;
    let ctx = eframe::egui::Context::default();
    let mut a = super::ui_driver::app();
    let mut f = None;
    a.doc.commit("Faces", |m, _| {
        let big = m.add_material("Photo lourde", [1, 2, 3]);
        let t = esq_core::meta::TextureData {
            filename: "lourde.png".into(),
            image: esq_core::photo::Blob(std::sync::Arc::new(vec![0; 1 << 20])),
            px: [512, 512],
            size: [1000.0, 1000.0],
            avg: [1, 2, 3],
        };
        m.materials[big].ext.texture = Some(t);
        let used = m.add_material("Brique", [170, 80, 60]);
        f = Some(square(m, 0.0, 1000.0, 0.0));
        square(m, 2000.0, 2500.0, 0.0);
        m.root.faces[f.unwrap()].front = Some(used);
    });
    let f = f.unwrap();
    a.doc.select(vec![EntityRef::Face(f)]);
    a.run(&ctx, Cmd::Copy);
    let (clip, _) = a.clipboard.as_ref().expect("copied");
    let names: Vec<&str> = clip.materials.values().map(|m| m.name.as_str()).collect();
    assert_eq!(names, vec!["Brique"], "the clipboard holds what was copied, not the whole model");
    // Cut, then Paste In Place: the face comes back where it was.
    a.run(&ctx, Cmd::Cut);
    assert_eq!(a.doc.ents().faces.len(), 1);
    a.run(&ctx, Cmd::PasteInPlace);
    assert_eq!(a.doc.ents().faces.len(), 2);
    assert_eq!(a.doc.undo_label(), Some("Coller"));
    let back = a.doc.ents().faces.values().find(|x| x.front.is_some()).expect("pasted face");
    let lo = back.outer.iter().map(|v| a.doc.ents().pos(*v)).fold(DVec3::INFINITY, |p, q| p.min(q));
    assert!(lo.distance(DVec3::ZERO) < 1e-6, "in place: {lo:?}");
    a.doc.undo();
    assert_eq!(a.doc.ents().faces.len(), 1);
    // Select None, Hide, Reveal Last, Reveal All.
    let g = a.doc.ents().faces.keys().next().unwrap();
    a.doc.select(vec![EntityRef::Face(g)]);
    a.run(&ctx, Cmd::SelectNone);
    assert!(a.doc.sel.is_empty());
    a.doc.select(vec![EntityRef::Face(g)]);
    a.run(&ctx, Cmd::Hide);
    assert!(a.doc.ents().faces[g].hidden && a.doc.sel.is_empty());
    a.run(&ctx, Cmd::RevealLast);
    assert!(!a.doc.ents().faces[g].hidden);
    a.doc.select(vec![EntityRef::Face(g)]);
    a.run(&ctx, Cmd::Hide);
    a.run(&ctx, Cmd::RevealAll);
    assert!(!a.doc.ents().faces[g].hidden);
    a.doc.undo();
    assert!(a.doc.ents().faces[g].hidden, "Ctrl+Z hides it again");
    // Lock, then Unlock All.
    a.doc.select(vec![EntityRef::Face(g)]);
    a.make_group(true);
    let [EntityRef::Instance(i)] = a.doc.sel.as_slice() else { panic!("groupe") };
    let i = *i;
    a.run(&ctx, Cmd::Lock);
    assert!(a.doc.ents().instances[i].locked);
    a.run(&ctx, Cmd::UnlockAll);
    assert!(!a.doc.ents().instances[i].locked);
    assert_eq!(a.doc.undo_label(), Some("Déverrouiller"));
}

/// Camera and scene commands no test named: the standard views, Previous / Next (camera
/// history), Next / Previous Scene.
#[test]
fn camera_views_history_and_scenes() {
    use crate::commands::{Cmd, STD_VIEWS};
    let ctx = eframe::egui::Context::default();
    let mut a = super::ui_driver::app();
    a.doc.commit("Face", |m, _| {
        square(m, 0.0, 1000.0, 0.0);
    });
    a.cam.size = esq_core::glam::DVec2::new(800.0, 600.0);
    for (k, (name, _, pitch)) in STD_VIEWS.iter().enumerate() {
        a.run(&ctx, Cmd::StdView(k));
        assert!((a.cam.pitch.to_degrees() - pitch).abs() < 1e-6, "{name}: {}", a.cam.pitch.to_degrees());
    }
    // Top, then Previous goes back to Iso, Next forward again.
    a.run(&ctx, Cmd::StdView(0));
    let top = a.cam.pitch;
    a.run(&ctx, Cmd::CamPrevious);
    assert!((a.cam.pitch.to_degrees() - STD_VIEWS[6].2).abs() < 1e-6);
    a.run(&ctx, Cmd::CamNext);
    assert!((a.cam.pitch - top).abs() < 1e-9);
    // Two scenes: Next and Previous Scene step through them.
    a.run(&ctx, Cmd::AddScene);
    a.run(&ctx, Cmd::StdView(2));
    a.run(&ctx, Cmd::AddScene);
    assert_eq!(a.doc.model.scenes.len(), 2);
    a.active_scene = Some(1);
    a.run(&ctx, Cmd::NextScene);
    assert_eq!(a.active_scene, Some(0), "after the last scene, the first");
    a.run(&ctx, Cmd::PrevScene);
    assert_eq!(a.active_scene, Some(1));
}

/// A model built to be heavy: each definition holds two copies of the next, 32 levels deep
/// (2^32 entries in the tree). Filtering the Outliner, expanding it all and measuring a
/// material's area look into each definition once and draw a bounded number of rows.
#[test]
fn deep_nesting_keeps_the_outliner_and_area_quick() {
    use esq_core::glam::DMat4;
    use esq_core::model::{Definition, Instance};
    let mut m = Model::new();
    let mat = m.add_material("Brique", [170, 80, 60]);
    let mut leaf = esq_core::model::Entities::default();
    let v: Vec<_> = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
        .iter()
        .map(|(x, y)| leaf.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
        .collect();
    let f = leaf.add_face_raw(v, vec![]).unwrap();
    leaf.faces[f].front = Some(mat);
    let inst = |def| Instance {
        def,
        xform: DMat4::IDENTITY,
        name: String::new(),
        material: None,
        layer: 0,
        hidden: false,
        locked: false,
        attributes: Default::default(),
        ext: Default::default(),
    };
    let def = |name: String, entities| Definition {
        name,
        is_group: false,
        description: String::new(),
        entities,
        attributes: Default::default(),
        ext: Default::default(),
    };
    let mut next = m.defs.insert(def("Niveau 32".into(), leaf));
    for k in (0..32).rev() {
        let mut e = esq_core::model::Entities::default();
        e.instances.insert(inst(next));
        e.instances.insert(Instance { xform: DMat4::from_translation(DVec3::X * 20.0), ..inst(next) });
        next = m.defs.insert(def(format!("Niveau {k}"), e));
    }
    m.root.instances.insert(inst(next));
    m.root.instances.insert(Instance { xform: DMat4::from_translation(DVec3::Y * 20.0), ..inst(next) });
    // The counters above and below are the real guard; the clock only catches a hang (a 2^33
    // walk takes hours), with room for a machine busy compiling.
    const HANG: std::time::Duration = std::time::Duration::from_secs(20);
    let start = std::time::Instant::now();
    let area = crate::mat_panel::material_area(&m, mat);
    assert_eq!(area, 100.0 * 2f64.powi(33), "every copy counted, each definition measured once");
    let mut a = super::ui_driver::app();
    a.doc = crate::doc::Document::new(m);
    let mut ui = super::ui_driver::Driver::new();
    ui.side = Some(crate::outliner_panel::show);
    ui.frames(&mut a, 2);
    let before = crate::outliner_panel::visits();
    ui.type_into(&mut a, "Filtre :", "rien de tel");
    ui.frames(&mut a, 5);
    assert!(crate::outliner_panel::visits() - before <= 2 * 34, "each definition looked into once");
    assert!(!ui.has("<Niveau 0>"));
    assert!(start.elapsed() < HANG, "filter: {:?}", start.elapsed());
    ui.focus(&mut a, "Filtre :");
    ui.key(&mut a, eframe::egui::Key::A, eframe::egui::Modifiers::COMMAND);
    ui.key(&mut a, eframe::egui::Key::Backspace, eframe::egui::Modifiers::NONE);
    ui.click(&mut a, "☰");
    let start = std::time::Instant::now();
    ui.click(&mut a, "Tout développer");
    assert!(ui.has("…"), "at most {} rows, then « … »", crate::outliner_panel::MAX_ROWS);
    // One click and the frames that follow it.
    assert!(start.elapsed() < HANG, "expand all: {:?}", start.elapsed());
    let start = std::time::Instant::now();
    // Entity Info of a top copy: no volume worked out from 2^33 flattened faces.
    let top = a.doc.model.root.instances.keys().next().unwrap();
    let d0 = a.doc.model.root.instances[top].def;
    assert!(
        crate::entity_info::flattened_faces(&a.doc.model, d0, &mut Default::default())
            > crate::entity_info::MAX_VOLUME_FACES
    );
    a.doc.select(vec![EntityRef::Instance(top)]);
    let mut info = info_driver();
    info.frames(&mut a, 3);
    assert!(info.has("Instance :") && !info.has("Volume"));
    assert!(start.elapsed() < HANG, "{:?}", start.elapsed());
}

/// A chain of 100 000 nested definitions (nothing bounds nesting when an .esq is read):
/// selecting its top copy and drawing Entity Info and the Outliner, on a thread with Windows'
/// 1 MB main stack, ends without overflowing it.
#[test]
fn a_very_deep_chain_of_definitions_does_not_overflow_the_stack() {
    let worker = std::thread::Builder::new().stack_size(1 << 20).spawn(|| {
        use esq_core::glam::DMat4;
        use esq_core::model::{Definition, Instance};
        let mut m = Model::new();
        let mut leaf = esq_core::model::Entities::default();
        let v: Vec<_> = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]
            .iter()
            .map(|(x, y)| leaf.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
            .collect();
        leaf.add_face_raw(v, vec![]).unwrap();
        let inst = |def| Instance {
            def,
            xform: DMat4::IDENTITY,
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        };
        let def = |entities| Definition {
            name: "Maillon".into(),
            is_group: false,
            description: String::new(),
            entities,
            attributes: Default::default(),
            ext: Default::default(),
        };
        let mut next = m.defs.insert(def(leaf));
        for _ in 0..100_000 {
            let mut e = esq_core::model::Entities::default();
            e.instances.insert(inst(next));
            next = m.defs.insert(def(e));
        }
        let top = m.root.instances.insert(inst(next));
        assert_eq!(crate::entity_info::flattened_faces(&m, next, &mut Default::default()), 1);
        assert_eq!(crate::entity_info::nesting_depth(&m, next, 17), 17);
        let mut a = super::ui_driver::app();
        a.doc.model = m;
        a.doc.select(vec![EntityRef::Instance(top)]);
        let mut info = info_driver();
        info.frames(&mut a, 2);
        let name = info.has("Instance :");
        let mut outl = super::ui_driver::Driver::new();
        outl.side = Some(crate::outliner_panel::show);
        outl.frames(&mut a, 2);
        outl.click(&mut a, "☰");
        outl.click(&mut a, "Tout développer");
        outl.frames(&mut a, 2);
        outl.type_into(&mut a, "Filtre :", "rien");
        outl.frames(&mut a, 2);
        name
    });
    assert!(worker.unwrap().join().expect("no stack overflow"), "Entity Info shown");
}

/// Entity Info still gives the volume of an ordinary solid group.
#[test]
fn entity_info_shows_the_volume_of_a_solid() {
    let mut h = super::Harness::new();
    h.rect_and_box();
    let mut a = super::ui_driver::app();
    a.doc = h.doc;
    let all: Vec<EntityRef> = a.doc.ents().faces.keys().map(EntityRef::Face).collect();
    a.doc.select(all);
    a.make_group(true);
    let mut info = info_driver();
    info.frames(&mut a, 3);
    assert!(info.nodes.iter().any(|n| n.value.starts_with("Volume : ") || n.label.starts_with("Volume : ")));
}

/// Window › Soften Edges: SketchUp's labels, and the edges inside a selected group (the
/// right-click menu offers the panel for a group, the panel then did nothing).
#[test]
fn soften_panel_softens_inside_a_selected_group() {
    let mut h = super::Harness::new();
    h.rect_and_box();
    let mut a = super::ui_driver::app();
    a.doc = h.doc;
    let all: Vec<EntityRef> = a.doc.ents().faces.keys().map(EntityRef::Face).collect();
    a.doc.select(all);
    a.make_group(true);
    let [EntityRef::Instance(g)] = a.doc.sel.as_slice() else { panic!("groupe") };
    let d = a.doc.ents().instances[*g].def;
    let mut ui = super::ui_driver::Driver::new();
    ui.side = Some(crate::soften_panel::show);
    ui.frames(&mut a, 2);
    assert!(ui.has("Angle entre les normales:") && ui.has("Adoucir les faces coplanaires"));
    assert!(!ui.node("Lisser les normales").disabled, "a group selected: the panel works");
    a.ui_state.soften = (95.0, true, true);
    ui.click(&mut a, "Lisser les normales");
    let e = &a.doc.model.defs[d].entities;
    let shared: Vec<_> = e.edges.values().filter(|x| x.faces.len() == 2).collect();
    assert!(!shared.is_empty() && shared.iter().all(|x| x.soft && !x.smooth), "90° ≤ 95°: soft, smoothing off");
    assert_eq!(a.doc.undo_label(), Some("Adoucir les arêtes"));
}

/// Edit › Make Component made the component at once: SketchUp first shows « Créer un
/// composant » (name, description, alignment, advanced attributes, replace the selection).
#[test]
fn make_component_asks_its_settings_first() {
    let mut a = super::ui_driver::app();
    let mut f = None;
    a.doc.commit("Face", |m, _| f = Some(square(m, 0.0, 1000.0, 0.0)));
    let f = f.unwrap();
    a.doc.select(vec![EntityRef::Face(f)]);
    let ctx = eframe::egui::Context::default();
    a.run(&ctx, crate::commands::Cmd::MakeComponent);
    assert!(a.doc.model.defs.is_empty(), "nothing made before Create");
    let mut ui = super::ui_driver::Driver::new();
    ui.frames(&mut a, 3);
    assert_eq!(ui.value("Définition:"), "Composant#1");
    assert_eq!(ui.toggled("Remplacer la sélection par le composant"), Some(true));
    ui.type_into(&mut a, "Définition:", "Fenêtre");
    ui.type_into(&mut a, "Description:", "Fenêtre à deux vantaux");
    ui.choose(&mut a, "Liaison", "Vertical");
    ui.click(&mut a, "Découper l’ouverture");
    ui.type_into(&mut a, "Prix :", "249");
    ui.click(&mut a, "Créer");
    let [EntityRef::Instance(i)] = a.doc.sel.as_slice() else { panic!("the new component is selected") };
    let d = &a.doc.model.defs[a.doc.ents().instances[*i].def];
    assert_eq!((d.name.as_str(), d.description.as_str()), ("Fenêtre", "Fenêtre à deux vantaux"));
    assert!(d.ext.behavior.is2d && d.ext.behavior.snapto == 2 && d.ext.behavior.cuts_opening);
    assert_eq!(d.attributes["SU_DefinitionSet"]["Price"], AttrValue::Number(249.0));
    assert_eq!(a.doc.undo_label(), Some("Créer un composant"));
    assert!(crate::make_component::state().is_none(), "Create closes it");
    // Unticked: only the definition, the face stays.
    let mut b = super::ui_driver::app();
    let mut g = None;
    b.doc.commit("Face", |m, _| g = Some(square(m, 0.0, 1000.0, 0.0)));
    b.doc.select(vec![EntityRef::Face(g.unwrap())]);
    b.run(&ctx, crate::commands::Cmd::MakeComponent);
    let mut ui = super::ui_driver::Driver::new();
    ui.frames(&mut b, 3);
    ui.click(&mut b, "Remplacer la sélection par le composant");
    ui.click(&mut b, "Créer");
    assert_eq!(b.doc.model.defs.len(), 1);
    assert!(b.doc.ents().instances.is_empty() && b.doc.ents().faces.contains_key(g.unwrap()));
    // Cancel: nothing.
    let mut c = super::ui_driver::app();
    c.doc.commit("Face", |m, _| g = Some(square(m, 0.0, 1000.0, 0.0)));
    c.doc.select(vec![EntityRef::Face(g.unwrap())]);
    c.run(&ctx, crate::commands::Cmd::MakeComponent);
    ui.frames(&mut c, 3);
    ui.click(&mut c, "Annuler");
    assert!(c.doc.model.defs.is_empty() && crate::make_component::state().is_none());
}
