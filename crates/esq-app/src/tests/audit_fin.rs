//! The findings the audit had left open, closed one by one: each test pins what
//! SketchUp does, as read in SketchUp.exe.

use esq_core::glam::DVec3;
use esq_core::model::{EntityRef, FaceId, Model};

/// A horizontal square [x0, x1]² at height z, facing up.
fn square(m: &mut Model, x0: f64, x1: f64, z: f64) -> FaceId {
    let v: Vec<_> = [(x0, x0), (x1, x0), (x1, x1), (x0, x1)]
        .iter()
        .map(|(x, y)| m.root.add_vertex_raw(DVec3::new(*x, *y, z)))
        .collect();
    m.root.add_face_raw(v, vec![]).expect("face")
}

/// `n` segments of a circle (or of an arc of `sweep` radians) of radius `r` around the origin,
/// one curve; with `face`, the disc it bounds.
pub(super) fn arc(m: &mut Model, r: f64, n: usize, sweep: f64, face: bool) -> u32 {
    let closed = (sweep - std::f64::consts::TAU).abs() < 1e-9;
    let pts = if closed { n } else { n + 1 };
    let e = &mut m.root;
    let v: Vec<_> = (0..pts)
        .map(|i| {
            let t = i as f64 * sweep / n as f64;
            e.add_vertex_raw(DVec3::new(r * t.cos(), r * t.sin(), 0.0))
        })
        .collect();
    let c = e.new_curve_id();
    for i in 0..n {
        let x = e.add_edge_raw(v[i], v[(i + 1) % pts]).unwrap();
        e.edges[x].curve = Some(c);
    }
    if face {
        e.add_face_raw(v, vec![]).expect("disc");
    }
    c
}

fn info_driver() -> super::ui_driver::Driver {
    let mut ui = super::ui_driver::Driver::new();
    ui.side = Some(|app, ui| {
        crate::entity_info::show(app, ui);
    });
    ui
}

fn edges_of(a: &crate::app::EsquisseApp, c: u32) -> Vec<EntityRef> {
    esq_core::curves::curve_edges(a.doc.ents(), c).into_iter().map(EntityRef::Edge).collect()
}

/// Entity Info's header is SketchUp's: the name of one entity ("Face", not
/// "1 face"), of a whole curve ("Cercle", "Arc"), "%d faces" for several of one kind, "%d
/// Entités" for a mix, then how many copies of the definition the model holds, or how many of
/// them are locked.
#[test]
fn entity_info_header_names_the_selection_as_sketchup() {
    let mut a = super::ui_driver::app();
    let (mut f, mut g, mut circle, mut open) = (None, None, 0, 0);
    a.doc.commit("Faces", |m, _| {
        f = Some(square(m, 0.0, 1000.0, 0.0));
        g = Some(square(m, 2000.0, 3000.0, 0.0));
        circle = arc(m, 500.0, 24, std::f64::consts::TAU, true);
        open = arc(m, 300.0, 12, std::f64::consts::PI, false);
    });
    let (f, g) = (f.unwrap(), g.unwrap());
    let title = |a: &crate::app::EsquisseApp, sel: &[EntityRef]| crate::entity_info::title(&a.doc, sel, &|_| false);
    assert_eq!(title(&a, &[]), "Aucune sélection");
    assert_eq!(title(&a, &[EntityRef::Face(f)]), "Face");
    assert_eq!(title(&a, &[EntityRef::Face(f), EntityRef::Face(g)]), "2 faces");
    let e = a.doc.ents().face_edges(f)[0];
    assert_eq!(title(&a, &[EntityRef::Edge(e)]), "Arête");
    assert_eq!(title(&a, &[EntityRef::Face(f), EntityRef::Edge(e)]), "2 Entités");
    assert_eq!(title(&a, &edges_of(&a, circle)), "Cercle");
    assert_eq!(title(&a, &edges_of(&a, open)), "Arc");
    let some: Vec<EntityRef> = edges_of(&a, circle).into_iter().take(3).collect();
    assert_eq!(title(&a, &some), "3 arêtes", "part of a curve is only edges");
    // Copies of a component, one of them locked.
    a.doc.select(vec![EntityRef::Face(f)]);
    a.make_group(false);
    let [EntityRef::Instance(i)] = a.doc.sel.as_slice() else { panic!("un composant") };
    let i = *i;
    assert_eq!(title(&a, &[EntityRef::Instance(i)]), "Composant (1 dans modèle)");
    let d = a.doc.ents().instances[i].def;
    let mut j = None;
    a.doc.commit("Copie", |m, ctx| {
        let mut copy = m.entities(ctx).instances[i].clone();
        copy.xform = esq_core::glam::DMat4::from_translation(DVec3::new(0.0, 5000.0, 0.0));
        j = Some(m.entities_mut(ctx).instances.insert(copy));
    });
    let j = j.unwrap();
    let both = [EntityRef::Instance(i), EntityRef::Instance(j)];
    assert_eq!(a.doc.model.instance_count(d), 2);
    assert_eq!(title(&a, &both), "2 composants (2 dans modèle)");
    a.doc.select(vec![EntityRef::Instance(j)]);
    a.set_locked(true, false);
    assert_eq!(title(&a, &both), "2 composants (1 verrouillé)");
    assert_eq!(title(&a, &[EntityRef::Instance(j)]), "Composant (2 dans modèle)", "one copy, locked");
    a.doc.select(vec![EntityRef::Instance(i)]);
    a.set_locked(true, false);
    assert_eq!(title(&a, &both), "2 composants (Tous verrouillés)");
    // A solid group, in the panel itself.
    let mut h = super::Harness::new();
    h.rect_and_box();
    let mut b = super::ui_driver::app();
    b.doc = h.doc;
    let all: Vec<EntityRef> = b.doc.ents().faces.keys().map(EntityRef::Face).collect();
    b.doc.select(all);
    assert!(title(&b, &b.doc.sel.clone()).ends_with(" faces"));
    b.make_group(true);
    let mut info = info_driver();
    info.frames(&mut b, 3);
    assert!(info.has("Groupe solide (1 dans modèle)"), "{:?}", info.nodes.iter().map(|n| &n.value).collect::<Vec<_>>());
}

/// Entity Info › Segments and Radius of a circle are typed as in SketchUp (`CArcPP`): the
/// circle is redrawn and its disc kept, one operation « Propriétés »; out of range, SketchUp's
/// warning; a circle pushed into a cylinder keeps its segments (the field is greyed).
#[test]
fn entity_info_retypes_the_segments_and_radius_of_a_circle() {
    crate::warnings::clear();
    let mut a = super::ui_driver::app();
    let mut c = 0;
    a.doc.commit("Cercle", |m, _| c = arc(m, 500.0, 24, std::f64::consts::TAU, true));
    a.doc.select(edges_of(&a, c));
    let mut ui = info_driver();
    ui.frames(&mut a, 2);
    assert!(ui.has("Circonférence :"));
    ui.type_into(&mut a, "Segments :", "48");
    assert_eq!(esq_core::curves::curve_edges(a.doc.ents(), c).len(), 48);
    assert_eq!(a.doc.ents().faces.len(), 1, "the disc is kept");
    assert_eq!(a.doc.ents().faces.values().next().unwrap().outer.len(), 48);
    assert_eq!(a.doc.undo_label(), Some("Propriétés"));
    assert_eq!(a.doc.sel.len(), 48, "the new edges are selected");
    ui.frames(&mut a, 2);
    let r = esq_core::units::format_length(800.0, a.doc.model.units);
    ui.type_into(&mut a, "Rayon:", &r);
    let arc0 = esq_core::arc_edit::ArcCurve::of(a.doc.ents(), c).unwrap();
    assert!((arc0.fit.radius - 800.0).abs() < 1e-6, "{}", arc0.fit.radius);
    assert_eq!(arc0.segments(), 48);
    // Out of range: SketchUp's message, nothing changes.
    ui.frames(&mut a, 2);
    ui.type_into(&mut a, "Segments :", "1000");
    assert_eq!(
        crate::warnings::front().as_deref(),
        Some("Les segments de la courbe doivent être compris entre 3 et 999")
    );
    assert_eq!(esq_core::curves::curve_edges(a.doc.ents(), c).len(), 48);
    ui.frames(&mut a, 2);
    ui.click(&mut a, "OK");
    assert!(crate::warnings::front().is_none());
    // Undo brings back the 24 segments.
    a.doc.undo();
    a.doc.undo();
    assert_eq!(esq_core::curves::curve_edges(a.doc.ents(), c).len(), 24);
    // A side face out of the circle's plane: Segments is greyed.
    let mut b = super::ui_driver::app();
    let mut c2 = 0;
    b.doc.commit("Cercle", |m, _| {
        c2 = arc(m, 500.0, 12, std::f64::consts::TAU, false);
        let e = esq_core::curves::curve_edges(&m.root, c2)[0];
        let [p, q] = m.root.edges[e].v;
        let (pp, pq) = (m.root.pos(p), m.root.pos(q));
        let tp = m.root.add_vertex_raw(pp + DVec3::Z * 300.0);
        let tq = m.root.add_vertex_raw(pq + DVec3::Z * 300.0);
        m.root.add_face_raw(vec![p, q, tq, tp], vec![]).unwrap();
        m.root.rebuild_topology();
    });
    b.doc.select(edges_of(&b, c2));
    ui.frames(&mut b, 2);
    assert!(ui.node("Segments :").disabled, "a pushed circle keeps its segments");
    assert!(!ui.node("Rayon:").disabled);
}

/// Entity Info › image: Width and Height typed, the proportions kept while « Verrouiller les
/// proportions » is ticked, the first corner kept; a null width refused with SketchUp's words.
#[test]
fn entity_info_resizes_an_image() {
    crate::warnings::clear();
    let mut a = super::ui_driver::app();
    let tex = esq_core::meta::TextureData {
        filename: "plan.png".into(),
        image: esq_core::photo::Blob(std::sync::Arc::new(vec![1, 2, 3])),
        px: [200, 100],
        size: [0.0, 0.0],
        avg: [10, 20, 30],
    };
    let mut i = None;
    a.doc.commit("Image", |m, _| {
        let o = DVec3::new(100.0, 50.0, 0.0);
        i = m.add_image(None, "Plan", "C:/x/plan.png", tex, o, DVec3::X, DVec3::Z, 1000.0, None);
    });
    let i = i.unwrap();
    a.doc.select(vec![EntityRef::Instance(i)]);
    let mut ui = info_driver();
    ui.frames(&mut a, 2);
    assert!(ui.has("200 x 100 px") && ui.has("plan.png"));
    assert_eq!(ui.toggled("Verrouiller les proportions"), Some(true), "the picture's proportions");
    let units = a.doc.model.units;
    ui.type_into(&mut a, "Largeur :", &esq_core::units::format_length(2000.0, units));
    let v = a.doc.model.image_of(None, i).unwrap();
    assert!((v.width - 2000.0).abs() < 1e-6 && (v.height - 1000.0).abs() < 1e-6, "{v:?}");
    assert!(v.origin.distance(DVec3::new(100.0, 50.0, 0.0)) < 1e-9, "the first corner stays");
    assert_eq!(a.doc.undo_label(), Some("Propriétés"));
    // Unlocked: the height alone.
    ui.frames(&mut a, 2);
    ui.click(&mut a, "Verrouiller les proportions");
    ui.type_into(&mut a, "Hauteur :", &esq_core::units::format_length(300.0, units));
    let v = a.doc.model.image_of(None, i).unwrap();
    assert!((v.width - 2000.0).abs() < 1e-6 && (v.height - 300.0).abs() < 1e-6, "{v:?}");
    ui.frames(&mut a, 2);
    assert_eq!(ui.toggled("Verrouiller les proportions"), Some(false));
    // Ticked again: the proportions come back from the side typed last (the height).
    ui.click(&mut a, "Verrouiller les proportions");
    let v = a.doc.model.image_of(None, i).unwrap();
    assert!((v.width - 600.0).abs() < 1e-6 && (v.height - 300.0).abs() < 1e-6, "{v:?}");
    // A null width: SketchUp's message, nothing changes.
    ui.frames(&mut a, 2);
    ui.type_into(&mut a, "Largeur :", "0");
    assert_eq!(crate::warnings::front().as_deref(), Some("La largeur de l’image doit être supérieure à 0."));
    assert!((a.doc.model.image_of(None, i).unwrap().width - 600.0).abs() < 1e-6);
}

/// Edit › Copy in one window, Paste in another: the selection goes through the clipboard of
/// Windows under a private format (SketchUp's scheme, `QtClipboard`), with its definitions,
/// materials and tags; the same window still pastes what it copied without reading it back;
/// a hostile or truncated content is refused without harm; SketchUp's own `SketchUp_Data` (a
/// .skp) pastes too. The test's windows share a clipboard of their own, never the user's.
#[test]
fn copy_in_one_window_paste_in_another() {
    use crate::clipboard::testing::{Fake, set};
    use crate::commands::Cmd;
    use std::sync::atomic::Ordering;
    let ctx = eframe::egui::Context::default();
    let fake = std::sync::Arc::new(Fake::default());
    set(Some(fake.clone()));
    // Window A: a brick face on a tag, and a component holding another face.
    let mut a = super::ui_driver::app();
    let mut f = None;
    a.doc.commit("Faces", |m, _| {
        let brick = m.add_material("Brique", [170, 80, 60]);
        let tag = m.add_layer("Murs");
        let x = square(m, 0.0, 1000.0, 0.0);
        m.root.faces[x].front = Some(brick);
        m.root.faces[x].layer = tag;
        f = Some(x);
    });
    let mut g = None;
    a.doc.commit("Face", |m, _| g = Some(square(m, 3000.0, 3500.0, 0.0)));
    a.doc.select(vec![EntityRef::Face(g.unwrap())]);
    a.make_group(false);
    let mut sel = a.doc.sel.clone();
    sel.push(EntityRef::Face(f.unwrap()));
    a.doc.select(sel);
    a.run(&ctx, Cmd::Copy);
    assert!(fake.data.lock().unwrap().1.contains_key("Esquisse_Data"), "the private format");
    assert!(fake.data.lock().unwrap().2.contains("Esquisse"), "a text for other programs");
    // Window B pastes in place: the same geometry, material, tag and definition.
    let mut b = super::ui_driver::app();
    assert!(b.cmd_enabled(Cmd::PasteInPlace), "another window's copy can be pasted");
    b.run(&ctx, Cmd::PasteInPlace);
    let m = &b.doc.model;
    assert_eq!(b.doc.ents().faces.len(), 1, "the brick face");
    assert_eq!(b.doc.ents().instances.len(), 1, "the component");
    let face = b.doc.ents().faces.values().next().unwrap();
    assert_eq!(face.front.map(|x| m.materials[x].name.as_str()), Some("Brique"));
    assert_eq!(m.layers[face.layer].name, "Murs");
    let inst = b.doc.ents().instances.values().next().unwrap();
    assert_eq!(m.defs[inst.def].entities.faces.len(), 1);
    assert!(!m.defs[inst.def].is_group, "still a component");
    let xs: Vec<f64> = face.outer.iter().map(|v| b.doc.ents().pos(*v).x).collect();
    assert!(xs.iter().all(|x| x.abs() < 1e-9 || (x - 1000.0).abs() < 1e-9), "pasted in place: {xs:?}");
    // The window that copied pastes from memory, as before (the clipboard is not read).
    let before = fake.reads.load(Ordering::Relaxed);
    a.run(&ctx, Cmd::PasteInPlace);
    assert_eq!(fake.reads.load(Ordering::Relaxed), before, "same window: from memory");
    assert_eq!(a.doc.ents().instances.len(), 2);
    // Another program copied text: A still pastes its own copy; a fresh window has nothing.
    fake.other_program_copies("bonjour");
    assert!(a.cmd_enabled(Cmd::Paste));
    let fresh = super::ui_driver::app();
    assert!(!fresh.cmd_enabled(Cmd::Paste));
    // Hostile contents: refused with a message, nothing pasted, no panic.
    let mut c = super::ui_driver::app();
    let mut bomb = b"ESQCLIP1".to_vec();
    bomb.extend_from_slice(&10u64.to_le_bytes());
    bomb.extend(miniz_oxide::deflate::compress_to_vec(&[b' '; 100_000], 6));
    let mut huge = b"ESQCLIP1".to_vec();
    huge.extend_from_slice(&u64::MAX.to_le_bytes());
    huge.push(0);
    for hostile in [bomb, b"ESQCLIP1".to_vec(), huge, vec![0; 7]] {
        fake.put_raw("Esquisse_Data", hostile);
        c.run(&ctx, Cmd::PasteInPlace);
        assert!(c.doc.ents().is_empty(), "nothing pasted");
        let msg = c.message.take().map(|x| x.0).unwrap_or_default();
        assert!(msg.starts_with("Impossible de coller"), "{msg}");
    }
    // A component that contains itself, put by another program: cut as a file would be.
    let mut cyc = esq_core::model::Model::new();
    let d = cyc.defs.insert(esq_core::model::Definition {
        name: "A".into(),
        is_group: false,
        description: String::new(),
        entities: Default::default(),
        attributes: Default::default(),
        ext: Default::default(),
    });
    let own = esq_core::model::Instance {
        def: d,
        xform: esq_core::glam::DMat4::IDENTITY,
        name: String::new(),
        material: None,
        layer: 0,
        hidden: false,
        locked: false,
        attributes: Default::default(),
        ext: Default::default(),
    };
    cyc.defs[d].entities.instances.insert(own);
    fake.put_raw("Esquisse_Data", crate::clipboard::encode(&cyc, d).unwrap());
    c.run(&ctx, Cmd::PasteInPlace);
    let n: usize = c.doc.model.defs.values().map(|x| x.entities.instances.len()).sum();
    assert_eq!(n, 0, "the cycle is cut");
    // SketchUp's format: a .skp.
    let mut src = esq_core::model::Model::new();
    square(&mut src, 0.0, 250.0, 0.0);
    let skp = esq_skp::native::write::write(&src, None, None, &Default::default()).expect(".skp");
    fake.put_raw("SketchUp_Data", skp);
    let mut s = super::ui_driver::app();
    s.run(&ctx, Cmd::PasteInPlace);
    assert_eq!(s.doc.ents().faces.len(), 1, "SketchUp's copy pastes");
    set(None);
}

/// Make Component › « Définir les axes du composant » (`IDC_PICK_ORIGIN`): the dialog hides
/// while the Axes tool picks an origin and two directions, shows again, and Create gives the
/// new definition those axes, the geometry staying where it was.
#[test]
fn make_component_sets_the_component_axes() {
    use crate::tools::ToolId;
    let ctx = eframe::egui::Context::default();
    let mut a = super::ui_driver::app();
    let mut f = None;
    a.doc.commit("Face", |m, _| f = Some(square(m, 0.0, 1000.0, 0.0)));
    let corners: Vec<DVec3> = {
        let e = a.doc.ents();
        e.faces[f.unwrap()].outer.iter().map(|v| e.pos(*v)).collect()
    };
    a.doc.select(vec![EntityRef::Face(f.unwrap())]);
    a.run(&ctx, crate::commands::Cmd::MakeComponent);
    let mut ui = super::ui_driver::Driver::new();
    ui.frames(&mut a, 3);
    ui.click(&mut a, "Définir les axes du composant");
    assert!(!ui.has("Créer"), "the dialog hides while the axes are picked");
    assert_eq!(a.tool.id(), ToolId::AxesTool);
    // The Axes tool, driven like a user: origin, red along +Y, green along -X.
    let mut h = super::Harness::new();
    h.doc = std::mem::replace(&mut a.doc, crate::doc::Document::new(Model::new()));
    h.tool = std::mem::replace(&mut a.tool, ToolId::Select.make());
    h.click(DVec3::new(500.0, 500.0, 0.0));
    h.click(DVec3::new(500.0, 800.0, 0.0));
    h.click(DVec3::new(200.0, 500.0, 0.0));
    a.doc = h.doc;
    a.tool = h.tool;
    ui.frames(&mut a, 3);
    assert!(ui.has("Créer"), "the dialog is back");
    ui.click(&mut a, "Créer");
    let [EntityRef::Instance(i)] = a.doc.sel.as_slice() else { panic!("the new component is selected") };
    let inst = &a.doc.ents().instances[*i];
    let x = inst.xform;
    assert!(x.w_axis.truncate().distance(DVec3::new(500.0, 500.0, 0.0)) < 1e-6, "origin: {:?}", x.w_axis);
    assert!(x.x_axis.truncate().distance(DVec3::Y) < 1e-9, "red axis: {:?}", x.x_axis);
    assert!(x.y_axis.truncate().distance(-DVec3::X) < 1e-9, "green axis: {:?}", x.y_axis);
    // The geometry did not move.
    let def = &a.doc.model.defs[inst.def].entities;
    let face = def.faces.values().next().unwrap();
    for (v, c) in face.outer.iter().zip(&corners) {
        assert!(x.transform_point3(def.pos(*v)).distance(*c) < 1e-6);
    }
    assert_eq!(a.doc.undo_label(), Some("Créer un composant"));
}

/// The messages SketchUp lets the user stop (SketchUp lists their preferences): the
/// ones whose situation exists in Esquisse show with SketchUp's words and tick box; once
/// stopped they are answered as SketchUp does without asking; Preferences › General ›
/// « Réinitialiser tous les messages d'avertissement » shows them all again.
#[test]
fn warning_messages_can_be_stopped_and_reset_like_sketchups() {
    use crate::commands::Cmd;
    crate::warnings::clear();
    let ctx = eframe::egui::Context::default();
    let mut a = super::ui_driver::app();
    let mut ui = super::ui_driver::Driver::new();
    // Scenes and Styles: the style was changed and not saved.
    crate::styles::model_styles(&mut a.doc.model, &a.style);
    a.style.xray = !a.style.xray;
    a.add_scene();
    assert!(a.doc.model.scenes.is_empty(), "asked first");
    let front = crate::warnings::front().unwrap();
    assert!(front.starts_with("Attention! Vous êtes en train de créer ou d'actualiser une scène"), "{front}");
    ui.frames(&mut a, 2);
    assert!(ui.has("Attention - Scènes et styles") || ui.has("Créer une scène"));
    ui.click(&mut a, "Créer une scène");
    assert_eq!(a.doc.model.scenes.len(), 1);
    let (list, _) = crate::styles::model_styles(&mut a.doc.model, &a.style);
    assert_eq!(list.len(), 2, "« Enregistrer comme nouveau style. » (the first choice)");
    // Changed again, and stopped this time: then the scene comes without asking, the style
    // changes left alone (« Je vais gérer les modifications de styles moi-même. »).
    a.style.xray = !a.style.xray;
    a.add_scene();
    ui.frames(&mut a, 2);
    ui.click(&mut a, "Ne plus afficher ce message.  Je vais gérer les modifications de styles moi-même.");
    ui.click(&mut a, "Ne rien faire pour enregistrer les modifications.");
    ui.click(&mut a, "Créer une scène");
    assert_eq!(a.doc.model.scenes.len(), 2);
    assert!(crate::warnings::stopped("ShowStylesWarning"));
    a.style.xray = !a.style.xray;
    a.add_scene();
    assert!(crate::warnings::front().is_none(), "stopped: not asked");
    assert_eq!(a.doc.model.scenes.len(), 3);
    assert_eq!(crate::styles::model_styles(&mut a.doc.model, &a.style).0.len(), 2, "nothing saved");
    // Photoreal materials: what they need first; OK turns them on.
    a.run(&ctx, Cmd::Photoreal);
    assert!(!a.style.photoreal);
    assert_eq!(
        crate::warnings::front().as_deref(),
        Some("Les matériaux seront rendus en temps réel sur la base de textures photoréelles et d'un environnement.")
    );
    ui.frames(&mut a, 2);
    assert!(ui.has("Ne plus montrer"));
    ui.click(&mut a, "OK");
    assert!(a.style.photoreal);
    a.run(&ctx, Cmd::Photoreal);
    assert!(!a.style.photoreal, "turned off without a question");
    // Section planes: named after they are placed.
    let mut h = super::Harness::new();
    h.rect_and_box();
    h.tool(crate::tools::ToolId::SectionPlane);
    h.click(DVec3::new(500.0, 300.0, 400.0));
    let mut b = super::ui_driver::app();
    b.doc = h.doc;
    ui.frames(&mut b, 2);
    assert!(ui.has("Ne plus afficher ce message.  Je vais utiliser les noms et symboles par défaut."));
    ui.type_into(&mut b, "Nom :", "Coupe A");
    ui.type_into(&mut b, "Symbole :", "A");
    ui.click(&mut b, "OK");
    let sp = b.doc.ents().sections.values().next().expect("a section plane");
    assert_eq!((sp.name.as_str(), sp.symbol.as_str()), ("Coupe A", "A"));
    assert_eq!(b.doc.undo_label(), Some("Nomenclature de la coupe"));
    // Preferences › General › Reset All Warning Messages: asked again.
    let mut prefs = super::ui_driver::Driver::new();
    prefs.side = Some(crate::dialogs::pref_general::general_page);
    prefs.frames(&mut a, 2);
    prefs.click(&mut a, "Réinitialiser tous les messages d'avertissement");
    assert!(!crate::warnings::stopped("ShowStylesWarning"));
    a.style.xray = !a.style.xray;
    a.add_scene();
    assert!(crate::warnings::front().is_some(), "shown again after the reset");
    crate::warnings::clear();
}

/// Styles › Modeling › Match Photo › « Photo de premier plan »: in a matched photo's view the
/// photo is drawn over the faces at its opacity (80 % in SketchUp's default style), not where
/// no face is (the background shows there); unticked, the faces show their own colour.
#[test]
fn match_photo_foreground_covers_the_faces_at_its_opacity() {
    let mut m = Model::new();
    let f = square(&mut m, 500.0, 1500.0, 0.0);
    let red = m.add_material("Rouge", [220, 20, 20]);
    m.root.faces[f].front = Some(red);
    m.root.faces[f].back = Some(red);
    let Some(mut g) = super::gpu::Gpu::new() else {
        eprintln!("pas de GPU : premier plan non rendu");
        return;
    };
    let cam = super::gpu::top_camera(2000.0, 96.0);
    let style = crate::style::ViewStyle::default();
    assert!(style.photo_fg && (style.photo_fg_opacity - 0.8).abs() < 1e-9, "SketchUp's default: ticked, 80 %");
    // A green photo filling the view.
    let mut img = image::RgbaImage::new(4, 4);
    img.pixels_mut().for_each(|p| *p = image::Rgba([20, 220, 20, 255]));
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let blob = esq_core::photo::Blob(std::sync::Arc::new(png));
    let shown = (blob, [0.0, 0.0, 96.0, 96.0]);
    let face = DVec3::new(1000.0, 1000.0, 0.0);
    let empty = DVec3::new(150.0, 150.0, 0.0);
    g.view.photo_overlay = crate::photo_fg::overlay(&style, Some(&shown), 1.0);
    let over = g.render(&m, &style, &cam, &crate::render::Lighting::default());
    let (on_face, off_face) = (over.at_point(&cam, face), over.at_point(&cam, empty));
    assert!(on_face[1] > on_face[0] + 60, "the photo over the face: {on_face:?}");
    let off = crate::style::ViewStyle { photo_fg: false, ..style.clone() };
    g.view.photo_overlay = crate::photo_fg::overlay(&off, Some(&shown), 1.0);
    assert!(g.view.photo_overlay.is_none());
    let plain = g.render(&m, &off, &cam, &crate::render::Lighting::default());
    let p = plain.at_point(&cam, face);
    assert!(p[0] > p[1] + 60, "unticked: the face's own colour: {p:?}");
    assert_eq!(off_face, plain.at_point(&cam, empty), "no face, no photo over it");
    // Opacity 100 %: the photo alone over the face (SketchUp's « Reversed Photo Modeling »).
    let full = crate::style::ViewStyle { photo_fg_opacity: 1.0, ..style.clone() };
    g.view.photo_overlay = crate::photo_fg::overlay(&full, Some(&shown), 1.0);
    let c = g.render(&m, &full, &cam, &crate::render::Lighting::default()).at_point(&cam, face);
    assert!(c[0] < 40 && c[1] > 200, "opaque photo: {c:?}");
}

/// A PNG file whose header announces `w` × `h` pixels and holds none (a forged photo).
fn forged_png(w: u32, h: u32) -> Vec<u8> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for b in data {
            c ^= *b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    let chunk = |kind: &[u8], body: &[u8]| {
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        let mut typed = kind.to_vec();
        typed.extend_from_slice(body);
        out.extend_from_slice(&typed);
        out.extend_from_slice(&crc(&typed).to_be_bytes());
        out
    };
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(chunk(b"IHDR", &ihdr));
    png.extend(chunk(b"IEND", &[]));
    png
}

/// A matched photo is data from the model: one whose header announces 30 000 × 30 000 pixels
/// is refused before it is decoded, the view drawn without it; a wide one is reduced to a
/// texture the device takes (4 096 at most), its proportions kept. Proved by the sizes asked,
/// never by a stopwatch.
#[test]
fn a_huge_photo_does_not_bring_the_view_down() {
    let forged = esq_core::photo::Blob(std::sync::Arc::new(forged_png(30_000, 30_000)));
    let err = crate::photo_img::decode(&forged.0).unwrap_err();
    assert!(err.contains("Image illisible") || err.contains("Format"), "{err}");
    let mut m = Model::new();
    square(&mut m, 500.0, 1500.0, 0.0);
    let Some(mut g) = super::gpu::Gpu::new() else {
        eprintln!("pas de GPU : texture non créée");
        return;
    };
    let cam = super::gpu::top_camera(2000.0, 96.0);
    let style = crate::style::ViewStyle::default();
    g.view.photo_overlay = crate::photo_fg::overlay(&style, Some(&(forged, [0.0, 0.0, 96.0, 96.0])), 1.0);
    let shot = g.render(&m, &style, &cam, &crate::render::Lighting::default());
    assert_eq!(g.view.photo_fg.texture_size(), None, "no texture for the forged photo");
    assert!(shot.at_point(&cam, DVec3::new(1000.0, 1000.0, 0.0)).iter().any(|c| *c > 100), "the face is drawn");
    // 6 000 × 60: reduced to the device's limit (4 096 at most), proportions kept.
    let wide = esq_core::photo::Blob(std::sync::Arc::new(crate::photo_img::tests::png_bytes(6000, 60)));
    g.view.photo_overlay = crate::photo_fg::overlay(&style, Some(&(wide, [0.0, 0.0, 96.0, 96.0])), 1.0);
    g.render(&m, &style, &cam, &crate::render::Lighting::default());
    let max = g.rs.device.limits().max_texture_dimension_2d.min(4096);
    let [w, h] = g.view.photo_fg.texture_size().expect("a texture");
    assert!(w <= max && h <= max, "{w} × {h} within {max}");
    assert_eq!(w, max);
    assert_eq!(h, (60.0 * max as f64 / 6000.0).round() as u32, "proportions kept");
}

/// The dominant colour of the middle of a PNG thumbnail.
fn middle_colour(png: &[u8]) -> [u8; 3] {
    let img = image::load_from_memory(png).expect("png").to_rgba8();
    let p = img.get_pixel(img.width() / 2, img.height() / 2);
    [p[0], p[1], p[2]]
}

/// Window › Components › In Model: each component of the model gets its own thumbnail,
/// rendered offscreen in isometric view (`CComponentUtils::CreateDefinitionThumbnailImage`),
/// kept while the definition stays the same, made again when it changes (the old picture
/// shows meanwhile) and for it alone.
#[test]
fn components_panel_renders_and_keeps_the_thumbnails_of_the_model() {
    let Some(rs) = super::gpu::render_state() else {
        eprintln!("pas de GPU : vignettes non rendues");
        return;
    };
    let mut a = super::ui_driver::app();
    let mut defs = Vec::new();
    for (x, colour) in [(0.0, [220, 30, 30]), (5000.0, [30, 30, 220])] {
        let mut f = None;
        a.doc.commit("Face", |m, _| {
            let mat = m.add_material("Couleur", colour);
            let face = square(m, x, x + 1000.0, 0.0);
            m.root.faces[face].front = Some(mat);
            m.root.faces[face].back = Some(mat);
            f = Some(face);
        });
        a.doc.select(vec![EntityRef::Face(f.unwrap())]);
        a.make_group(false);
        let [EntityRef::Instance(i)] = a.doc.sel.as_slice() else { panic!("un composant") };
        defs.push(a.doc.ents().instances[*i].def);
    }
    let (red, blue) = (defs[0], defs[1]);
    // Asked by the panel: none yet, both queued, rendered by the offscreen step.
    assert!(crate::def_thumbs::thumbnail(&mut a, red).is_none());
    assert!(crate::def_thumbs::thumbnail(&mut a, blue).is_none());
    a.step_def_thumbnails(&rs);
    assert_eq!(a.ui_state.extra.def_thumbs.rendered, 2);
    let (k_red, png) = crate::def_thumbs::thumbnail(&mut a, red).expect("rendered");
    let c = middle_colour(&png);
    assert!(c[0] > c[2] + 40, "the red component: {c:?}");
    let (_, png) = crate::def_thumbs::thumbnail(&mut a, blue).expect("rendered");
    let c = middle_colour(&png);
    assert!(c[2] > c[0] + 40, "the blue component: {c:?}");
    // Asked again, unchanged: kept.
    a.step_def_thumbnails(&rs);
    assert_eq!(a.ui_state.extra.def_thumbs.rendered, 2, "kept while unchanged");
    // The red component's face turns green: its thumbnail alone is made again.
    a.doc.commit("Peindre", |m, _| {
        let g = m.add_material("Vert", [30, 200, 30]);
        for f in m.defs[red].entities.faces.values_mut() {
            f.front = Some(g);
            f.back = Some(g);
        }
    });
    let (k_old, _) = crate::def_thumbs::thumbnail(&mut a, red).expect("the old picture meanwhile");
    assert_eq!(k_old, k_red);
    crate::def_thumbs::thumbnail(&mut a, blue);
    a.step_def_thumbnails(&rs);
    assert_eq!(a.ui_state.extra.def_thumbs.rendered, 3, "only the changed one");
    let (k_new, png) = crate::def_thumbs::thumbnail(&mut a, red).expect("rendered again");
    assert_ne!(k_new, k_red, "a new key: the panel shows the new picture");
    let c = middle_colour(&png);
    assert!(c[1] > c[0] + 40 && c[1] > c[2] + 40, "green now: {c:?}");
}

/// Styles › Faces › « Qualité de transparence: » (`CStylesEditFacePage`, `SlugTransparentDrawer`):
/// « Meilleure » composites the transparent faces from back to front, the nearer one over the
/// farther one; « Rapide » adds them up in one order-independent pass, overlapping colours
/// averaged. Both give the same picture whichever face was drawn first (before, the order of
/// the mesh decided). X-ray shows at 50 % in « Rapide », at its slider's opacity in « Meilleure ».
#[test]
fn transparency_quality_draws_as_sketchups_two_modes() {
    // A half-transparent red square above a half-transparent blue one, built in either order.
    let make = |red_first: bool| {
        let mut m = Model::new();
        let red = m.add_material("Rouge", [230, 20, 20]);
        let blue = m.add_material("Bleu", [20, 20, 230]);
        for id in [red, blue] {
            m.materials[id].opacity = 0.5;
        }
        let layers = if red_first { [(red, 100.0), (blue, 0.0)] } else { [(blue, 0.0), (red, 100.0)] };
        for (mat, z) in layers {
            let f = square(&mut m, 500.0, 1500.0, z);
            m.root.faces[f].front = Some(mat);
            m.root.faces[f].back = Some(mat);
        }
        m
    };
    let Some(mut g) = super::gpu::Gpu::new() else {
        eprintln!("pas de GPU : transparence non rendue");
        return;
    };
    let cam = super::gpu::top_camera(2000.0, 96.0);
    let middle = DVec3::new(1000.0, 1000.0, 50.0);
    let mut px = |m: &Model, sort: i64| {
        let st = crate::style::ViewStyle { transparency_sort: sort, ..Default::default() };
        g.render(m, &st, &cam, &crate::render::Lighting::default()).at_point(&cam, middle)
    };
    let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(&b).all(|(x, y)| x.abs_diff(*y) <= 3);
    let (a, b) = (make(true), make(false));
    // Meilleure: the red, nearer the eye, over the blue, whatever the order.
    let (n1, n2) = (px(&a, 2), px(&b, 2));
    assert!(near(n1, n2), "Meilleure, same picture either way: {n1:?} / {n2:?}");
    assert!(n1[0] > n1[2] + 30, "the nearer red wins: {n1:?}");
    // Rapide: one pass, order-independent, red and blue averaged.
    let (f1, f2) = (px(&a, 1), px(&b, 1));
    assert!(near(f1, f2), "Rapide, same picture either way: {f1:?} / {f2:?}");
    assert!(f1[0].abs_diff(f1[2]) <= 12, "an average of red and blue: {f1:?}");
    assert!(near(px(&a, 0), f1), "value 0 (older styles) is « Rapide » too");
    // X-ray: 50 % with Rapide whatever its slider, the slider's with Meilleure.
    let xr = |sort: i64| {
        crate::oit::xray_opacity(&crate::style::ViewStyle {
            xray: true,
            xray_opacity: 0.8,
            transparency_sort: sort,
            ..Default::default()
        })
    };
    assert_eq!((xr(1), xr(2)), (0.5, 0.8));
}

/// Entity Info of a text or a dimension: « Police : » and « Changer de police... »
/// (`CTextPP`, `CDimensionPP`), the font of that annotation alone, one operation « Propriétés ».
#[test]
fn entity_info_changes_the_font_of_a_text_or_a_dimension() {
    use esq_core::model::{AnnotKind, Annotation};
    let mut a = super::ui_driver::app();
    let mut ids = Vec::new();
    a.doc.commit("Annotations", |m, _| {
        for kind in [
            AnnotKind::Text { point: DVec3::ZERO, leader: DVec3::new(100.0, 0.0, 100.0) },
            AnnotKind::Linear { a: DVec3::ZERO, b: DVec3::new(1000.0, 0.0, 0.0), offset: DVec3::new(0.0, 200.0, 0.0) },
        ] {
            ids.push(m.root.annots.insert(Annotation {
                kind,
                text: "Note".into(),
                layer: 0,
                hidden: false,
                attach: Default::default(),
            }));
        }
    });
    for id in ids {
        a.doc.select(vec![EntityRef::Annot(id)]);
        let mut ui = info_driver();
        ui.frames(&mut a, 2);
        assert!(ui.has("Police :"));
        if !ui.has("Gras") {
            ui.click(&mut a, "Polices de l'entité");
        }
        ui.click(&mut a, "Gras");
        let key = crate::annot::prop_key(id);
        assert_eq!(a.doc.ents().prop(&key, "font_bold"), Some(&esq_core::model::AttrValue::Bool(true)));
        assert!(crate::annot::style_of(&a.doc.model, a.doc.ents(), id).font.bold);
        assert_eq!(a.doc.undo_label(), Some("Propriétés"));
    }
}
