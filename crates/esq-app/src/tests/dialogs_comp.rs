//! Window › Components › Edit: a definition's name, description and alignment, and what the
//! alignment does (glue, cut opening, face camera).

use eframe::egui;
use esq_core::glam::{DMat4, DVec3};
use esq_core::model::{DefId, Definition, Entities, EntityRef, GlueRef, Instance, Model};

use super::Harness;
use super::dialogs::ruby;
use super::ui_driver::{Driver, app};
use crate::dialogs::comp_edit;

/// A "window": a 200 × 200 square drawn in its gluing plane (z = 0).
fn window_def(m: &mut Model) -> DefId {
    let mut e = Entities::default();
    let v: Vec<_> = [(0.0, 0.0), (200.0, 0.0), (200.0, 200.0), (0.0, 200.0)]
        .iter()
        .map(|(x, y)| e.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
        .collect();
    for i in 0..4 {
        e.add_edge_raw(v[i], v[(i + 1) % 4]);
    }
    // The pane.
    e.add_face_raw(v, vec![]);
    m.defs.insert(Definition {
        name: "Fenêtre".into(),
        is_group: false,
        description: String::new(),
        entities: e,
        attributes: Default::default(),
        ext: Default::default(),
    })
}

fn instance(def: DefId, xform: DMat4) -> Instance {
    Instance {
        def,
        xform,
        name: String::new(),
        material: None,
        layer: 0,
        hidden: false,
        locked: false,
        attributes: Default::default(),
        ext: Default::default(),
    }
}

#[test]
fn components_edit_names_describes_and_aligns_a_definition() {
    let mut a = app();
    let mut d = None;
    let mut i = None;
    a.doc.commit("t", |m, _| {
        let def = window_def(m);
        i = Some(m.root.instances.insert(instance(def, DMat4::IDENTITY)));
        d = Some(def);
    });
    let (d, i) = (d.unwrap(), i.unwrap());
    a.doc.select(vec![EntityRef::Instance(i)]);
    let mut ui = Driver::new();
    ui.side = Some(|a, ui| {
        egui::Panel::right("side").exact_size(330.0).show(ui, |ui| comp_edit::edit_section(a, ui));
    });
    ui.frames(&mut a, 2);
    // The selected component is the one edited.
    assert_eq!(ui.value("Composant à modifier"), "Fenêtre");
    assert_eq!(ui.value("Chargé depuis"), "(ce modèle)");
    ui.type_into(&mut a, "Nom du composant", "Baie");
    assert_eq!(a.doc.model.defs[d].name, "Baie");
    ui.focus(&mut a, "Description du composant");
    ui.text(&mut a, "Baie vitrée");
    ui.focus(&mut a, "Nom du composant");
    ui.frames(&mut a, 2);
    assert_eq!(a.doc.model.defs[d].description, "Baie vitrée");

    // Alignment: glue to vertical faces, cut the opening, face the camera, shadows to the sun.
    assert!(ui.node("Découper l’ouverture").disabled, "sans liaison, pas de découpe");
    ui.choose(&mut a, "Liaison", "Vertical");
    ui.click(&mut a, "Découper l’ouverture");
    assert!(ui.node("Ombres face au soleil").disabled);
    ui.click(&mut a, "Toujours faire face à la caméra");
    ui.click(&mut a, "Ombres face au soleil");
    let b = a.doc.model.defs[d].ext.behavior;
    assert!(b.is2d && b.snapto == 2 && b.cuts_opening && b.always_face_camera && b.shadows_face_sun);
    // Ruby's Behavior reads the same settings.
    if let Some(r) = ruby(
        &mut a,
        "b = Sketchup.active_model.definitions['Baie'].behavior; [b.is2d?, b.snapto, b.cuts_opening?, b.always_face_camera?, b.shadows_face_sun?]",
    ) {
        assert_eq!(r, "[true, 2, true, true, true]");
    }
    // One undo step each.
    a.doc.undo();
    assert!(!a.doc.model.defs[d].ext.behavior.shadows_face_sun);

    // Save As: a file of its own that keeps the behaviour.
    let f = std::env::temp_dir().join(format!("esq-baie-{}.esq", std::process::id()));
    comp_edit::save_definition(&a.doc.model, d, &f).unwrap();
    let back = esq_core::io::load_esq(&f).unwrap();
    assert!(back.behavior().cuts_opening && back.meta.description == "Baie vitrée");
    let _ = std::fs::remove_file(&f);
}

#[test]
fn gluing_components_lie_on_faces_of_their_slope_and_cut_them() {
    let mut h = Harness::new();
    h.rect_and_box();
    let mut d = None;
    h.doc.commit("t", |m, _| {
        let def = window_def(m);
        let b = &mut m.defs[def].ext.behavior;
        b.is2d = true;
        b.snapto = 2;
        b.cuts_opening = true;
        d = Some(def);
    });
    let d = d.unwrap();
    let wall = h.doc.ents().faces.iter().find(|(_, f)| f.normal.y < -0.99).map(|(k, _)| k).unwrap();

    // On the wall (vertical): it lies on it, glued.
    h.tool = Box::new(crate::tools::PlaceTool::new(d));
    h.click(DVec3::new(400.0, 0.0, 100.0));
    let placed: Vec<_> = h.doc.ents().instances.values().cloned().collect();
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].ext.glued_to, Some(GlueRef::Face(wall)));
    assert!(placed[0].xform.transform_vector3(DVec3::Z).abs_diff_eq(-DVec3::Y, 1e-9));
    // Cut opening: the wall shows a 200 × 200 hole.
    let ents = h.doc.ents();
    let holes = comp_edit::openings(&h.doc.model, ents);
    let area: f64 = comp_edit::cut_triangles(ents, wall, &holes[&wall])
        .iter()
        .map(|[a, b, c]| (*b - *a).cross(*c - *a).length() * 0.5)
        .sum();
    assert!((area - (1000.0 * 400.0 - 200.0 * 200.0)).abs() < 1e-3, "{area}");

    // On the top (horizontal): a vertical-only component does not glue.
    h.tool = Box::new(crate::tools::PlaceTool::new(d));
    h.click(DVec3::new(500.0, 300.0, 400.0));
    let top: Vec<_> = h.doc.ents().instances.values().filter(|x| x.ext.glued_to.is_none()).cloned().collect();
    assert_eq!(top.len(), 1);
    assert!(top[0].xform.transform_vector3(DVec3::Z).abs_diff_eq(DVec3::Z, 1e-9));
}

/// A cut-out person: a 200 × 400 face standing in its red-blue plane, its front to -Y.
fn person_def(m: &mut Model) -> DefId {
    let mut e = Entities::default();
    let v: Vec<_> = [(0.0, 0.0), (200.0, 0.0), (200.0, 400.0), (0.0, 400.0)]
        .iter()
        .map(|(x, z)| e.add_vertex_raw(DVec3::new(*x, 0.0, *z)))
        .collect();
    e.add_face_raw(v, vec![]);
    m.defs.insert(Definition {
        name: "Personnage".into(),
        is_group: false,
        description: String::new(),
        entities: e,
        attributes: Default::default(),
        ext: Default::default(),
    })
}

#[test]
fn face_me_components_turn_to_the_camera() {
    let mut m = Model::new();
    let d = person_def(&mut m);
    m.defs[d].ext.behavior.always_face_camera = true;
    let at = DMat4::from_translation(DVec3::new(1000.0, 0.0, 0.0));
    m.root.instances.insert(instance(d, at));
    let placed = |m: &Model| {
        let mut x = None;
        esq_core::walk::visit(m, |p| {
            if p.def == Some(d) {
                x = Some(p.xform);
            }
        });
        x.unwrap()
    };
    assert!(!comp_edit::follow_camera(&Model::new(), DVec3::ZERO), "nothing to turn");
    assert_eq!(placed(&m), at, "no camera yet: as placed");
    let eye = DVec3::new(6000.0, 0.0, 1500.0);
    assert!(comp_edit::follow_camera(&m, eye), "the display is rebuilt");
    assert!(!comp_edit::follow_camera(&m, eye), "the eye did not move");
    // Its front (-Y) faces the eye, seen from above.
    let front = placed(&m).transform_vector3(-DVec3::Y);
    assert!(front.abs_diff_eq(DVec3::X, 1e-9), "{front}");
    assert!(comp_edit::follow_camera(&m, DVec3::new(1000.0, -5000.0, 0.0)));
    assert!(placed(&m).transform_vector3(-DVec3::Y).abs_diff_eq(-DVec3::Y, 1e-9));

    // Shadows face sun (with shadows on): the pane shown casts no shadow, a copy turned to
    // the sun does and is not shown.
    m.defs[d].ext.behavior.shadows_face_sun = true;
    m.shadow.display = true;
    let style = crate::style::ViewStyle::default();
    let opts =
        crate::scene::SceneOptions { ctx: &[], ctx_def: None, sel: &[], axes_xform: DMat4::IDENTITY, style: &style };
    let mesh = crate::scene::build(&m, &opts);
    let shown: Vec<_> = mesh.opaque.iter().filter(|v| v.tex & crate::scene::SHADOW_ONLY == 0).collect();
    let caster: Vec<_> = mesh.opaque.iter().filter(|v| v.tex & crate::scene::SHADOW_ONLY != 0).collect();
    assert_eq!((shown.len(), caster.len()), (6, 6));
    assert!(shown.iter().all(|v| v.tex & crate::scene::NO_SHADOW != 0));
    // The pane shown faces the camera, the caster faces the sun.
    let normal = |v: &crate::scene::FaceVertex| DVec3::from(v.normal.map(f64::from));
    assert!(normal(shown[0]).abs_diff_eq(-DVec3::Y, 1e-6), "{}", normal(shown[0]));
    let sun = comp_edit::sun_direction(&m);
    let flat = DVec3::new(sun.x, sun.y, 0.0).normalize();
    assert!(normal(caster[0]).dot(flat) > 0.99, "{} / {sun}", normal(caster[0]));
    // The behaviour off: it stays as placed.
    m.defs[d].ext.behavior.always_face_camera = false;
    assert!(comp_edit::follow_camera(&m, eye));
    assert_eq!(placed(&m), at);
}
