//! Tests of the tools: commands on the selection and the
//! tools read from SketchUp.exe, driven like a user (select with the mouse, then the command).

use eframe::egui::Modifiers;
use esq_core::glam::{DVec2, DVec3};
use esq_core::model::{EdgeId, EntityRef, FaceId};

use super::{CTRL, Harness};
use crate::edit_ops;
use crate::tools::ToolId;

fn double_click(h: &mut Harness, p: DVec3) {
    let m = h.px(p);
    h.with(|t, cx| {
        t.mouse_move(cx, m);
        t.press(cx, m);
        t.release(cx, m);
        t.double_click(cx, m);
    });
}

fn count(h: &Harness, edges: bool) -> usize {
    h.doc
        .sel
        .iter()
        .filter(|e| if edges { matches!(e, EntityRef::Edge(_)) } else { matches!(e, EntityRef::Face(_)) })
        .count()
}

#[test]
fn deselect_edges_and_faces_keep_the_rest_of_the_selection() {
    let mut h = Harness::new();
    h.rect_and_box();
    // Double-click on the top face: the face and its four edges.
    h.tool(ToolId::Select);
    double_click(&mut h, DVec3::new(500.0, 300.0, 400.0));
    assert_eq!((count(&h, false), count(&h, true)), (1, 4));
    assert_eq!(edit_ops::deselect(&mut h.doc, true), 4);
    assert_eq!(h.doc.sel.len(), 1);
    assert!(matches!(h.doc.sel[0], EntityRef::Face(_)));
    // Nothing to do the second time.
    assert_eq!(edit_ops::deselect(&mut h.doc, true), 0);

    double_click(&mut h, DVec3::new(500.0, 300.0, 400.0));
    assert_eq!(edit_ops::deselect(&mut h.doc, false), 1);
    assert_eq!((count(&h, false), count(&h, true)), (0, 4));
}

/// The edge of the active context between two points (either way).
fn edge_at(h: &Harness, a: DVec3, b: DVec3) -> EdgeId {
    let ents = h.doc.ents();
    ents.edges
        .keys()
        .find(|&e| {
            let (p, q) = ents.edge_points(e);
            (p.distance(a) < 1e-6 && q.distance(b) < 1e-6) || (p.distance(b) < 1e-6 && q.distance(a) < 1e-6)
        })
        .expect("edge")
}

/// Three connected lines drawn with the Line tool (not closed).
fn zigzag(h: &mut Harness) -> [DVec3; 4] {
    h.tool(ToolId::Line);
    let pts =
        [DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1500.0, 800.0, 0.0), DVec3::new(2500.0, 800.0, 0.0)];
    for p in pts {
        h.click(p);
    }
    h.with(|t, cx| t.cancel(cx));
    assert_eq!(h.doc.ents().edges.len(), 3);
    pts
}

#[test]
fn weld_edges_makes_the_selected_chain_one_curve() {
    let mut h = Harness::new();
    let pts = zigzag(&mut h);
    // Click the first edge, Ctrl+click the two others.
    h.tool(ToolId::Select);
    for (i, w) in pts.windows(2).enumerate() {
        h.mods = if i == 0 { Modifiers::NONE } else { CTRL };
        h.click((w[0] + w[1]) * 0.5);
    }
    h.mods = Modifiers::NONE;
    assert_eq!(h.doc.sel.len(), 3);
    assert_eq!(edit_ops::weld_edges(&mut h.doc), 1);
    let c = h.doc.ents().edges[edge_at(&h, pts[0], pts[1])].curve;
    assert!(c.is_some());
    assert!(h.doc.ents().edges.values().all(|e| e.curve == c));
    assert_eq!(h.doc.undo_label(), Some("Souder les arêtes"));
}

#[test]
fn area_of_the_selection_sums_the_selected_faces() {
    let mut h = Harness::new();
    h.rect_and_box();
    // Top face (1000 x 600) and, with Ctrl, the front face (1000 x 400).
    h.tool(ToolId::Select);
    h.click(DVec3::new(500.0, 300.0, 400.0));
    h.mods = CTRL;
    h.click(DVec3::new(500.0, 0.0, 200.0));
    h.mods = Modifiers::NONE;
    assert_eq!(count(&h, false), 2);
    let a = edit_ops::selection_area(&h.doc);
    assert!((a - 1.0e6).abs() < 1e-6, "{a}");
    assert_eq!(esq_core::units::format_area(a, esq_core::model::Units::Meters), "1 m²");
}

#[test]
fn divide_tool_counts_with_the_cursor_and_cuts_on_click() {
    let mut h = Harness::new();
    h.rect_and_box();
    let (a, b) = (DVec3::new(0.0, 0.0, 400.0), DVec3::new(1000.0, 0.0, 400.0));
    h.tool(ToolId::Select);
    h.click((a + b) * 0.5);
    assert!(edit_ops::can_divide(&h.doc));
    h.tool(ToolId::Divide);
    assert_eq!(h.tool.status(), "Sélectionnez ou entrez le nombre de segments.");
    // A quarter of the way: 4 segments; near the middle: 2.
    h.hover(a + (b - a) * 0.25);
    assert_eq!(h.tool.vcb_value(esq_core::model::Units::Millimeters), "4");
    h.hover(a + (b - a) * 0.52);
    assert_eq!(h.tool.vcb_value(esq_core::model::Units::Millimeters), "2");
    h.click(a + (b - a) * 0.2);
    assert!(h.tool.done());
    let ents = h.doc.ents();
    assert_eq!((ents.faces.len(), ents.edges.len()), (6, 16));
    for x in [200.0, 400.0, 600.0, 800.0] {
        assert!(ents.find_vertex(DVec3::new(x, 0.0, 400.0)).is_some(), "{x}");
    }
    assert_eq!(h.doc.undo_label(), Some("Diviser des arêtes"));
}

#[test]
fn divide_typed_redraws_a_circle() {
    let mut h = Harness::new();
    h.tool(ToolId::Circle);
    h.click(DVec3::ZERO);
    h.hover(DVec3::new(500.0, 0.0, 0.0));
    h.type_vcb("500");
    let n0 = h.doc.ents().edges.len();
    assert!(n0 >= 12);
    let e = h.doc.ents().edges.keys().next().unwrap();
    h.doc.select(vec![EntityRef::Edge(e)]);
    assert!(edit_ops::can_divide(&h.doc));
    h.tool(ToolId::Divide);
    h.type_vcb("6");
    assert!(h.tool.done());
    let ents = h.doc.ents();
    assert_eq!((ents.edges.len(), ents.faces.len()), (6, 1));
    assert!(ents.verts.values().all(|v| (v.pos.length() - 500.0).abs() < 1e-6));
}

#[test]
fn eye_height_typed_in_look_around_and_walk() {
    // CCameraEyeHeightTool, base of Look Around (CCameraPanTool) and Walk: the Measurements
    // box shows and sets the height of the eye above the ground.
    for tool in [ToolId::LookAround, ToolId::Walk] {
        let mut h = Harness::new();
        h.tool(tool);
        assert_eq!(h.tool.vcb_label(), "Hauteur des yeux");
        h.type_vcb("1700");
        assert!((h.cam.eye().z - 1700.0).abs() < 1e-6, "{tool:?} {}", h.cam.eye().z);
        h.hover(DVec3::new(500.0, 300.0, 0.0));
        assert_eq!(h.tool.vcb_value(esq_core::model::Units::Millimeters), "1700 mm");
    }
}

#[test]
fn show_face_edges_selects_the_face_and_counts_loops_and_edges() {
    let mut h = Harness::new();
    h.tool(ToolId::Rect);
    h.click(DVec3::ZERO);
    h.hover(DVec3::new(900.0, 500.0, 0.0));
    h.type_vcb("1000;600");
    h.tool(ToolId::Rect);
    h.click(DVec3::new(200.0, 200.0, 0.0));
    h.hover(DVec3::new(380.0, 290.0, 0.0));
    h.type_vcb("200;100");
    assert_eq!(h.faces(), 2);
    h.tool(ToolId::ShowFaceEdges);
    assert_eq!(h.tool.status(), "Sélectionnez une face et mettez ses bords en évidence");
    h.click(DVec3::new(100.0, 100.0, 0.0));
    assert_eq!((count(&h, false), count(&h, true)), (1, 8));
    assert!(h.tool.status().ends_with(" a 2 boucles et 8 arêtes"), "{}", h.tool.status());
    // Clicking in the void clears the selection.
    h.click(DVec3::new(3000.0, -2000.0, 0.0));
    assert!(h.doc.sel.is_empty());
}

/// A stored (uncompressed) zip archive.
fn zip(files: &[(&str, &str)]) -> Vec<u8> {
    let (mut out, mut central) = (Vec::new(), Vec::new());
    for (name, body) in files {
        let off = out.len() as u32;
        let (n, b) = (name.as_bytes(), body.as_bytes());
        out.extend(b"PK\x03\x04");
        out.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend((b.len() as u32).to_le_bytes());
        out.extend((b.len() as u32).to_le_bytes());
        out.extend((n.len() as u16).to_le_bytes());
        out.extend([0, 0]);
        out.extend(n);
        out.extend(b);
        central.extend(b"PK\x01\x02");
        central.extend([20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        central.extend((b.len() as u32).to_le_bytes());
        central.extend((b.len() as u32).to_le_bytes());
        central.extend((n.len() as u16).to_le_bytes());
        central.extend([0; 12]);
        central.extend(off.to_le_bytes());
        central.extend(n);
    }
    let cd_off = out.len() as u32;
    let cd_len = central.len() as u32;
    out.extend(central);
    out.extend(b"PK\x05\x06\0\0\0\0");
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend(cd_len.to_le_bytes());
    out.extend(cd_off.to_le_bytes());
    out.extend([0, 0]);
    out
}

#[test]
fn classification_files_are_read_like_sketchup() {
    let dir = std::env::temp_dir().join(format!("esq_outils_skc_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("Test.skc");
    let data = zip(&[
        ("documentProperties.xml", "<dp:documentProperties><dp:title>IFC Test</dp:title></dp:documentProperties>"),
        (
            "Schemas/T.xsd",
            "<xs:schema targetNamespace=\"http://example.org/t\">\
             <xs:element substitutionGroup=\"t:IfcBuildingElement\" name=\"IfcWall\"/>\
             <xs:element substitutionGroup=\"t:IfcElement\" name=\"IfcBuildingElement\"/></xs:schema>",
        ),
        ("Schemas/T.xsd.filter", "IfcWall\nIfcDoor\n// Blacklist\nIfcNothing\n"),
    ]);
    std::fs::write(&p, data).unwrap();
    let s = crate::tools::classifier::load_skc(&p).unwrap();
    assert_eq!((s.name.as_str(), s.namespace.as_str()), ("IFC Test", "http://example.org/t"));
    assert_eq!(s.types, ["IfcWall", "IfcDoor", "IfcBuildingElement", "IfcElement"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn classifier_applies_samples_and_removes_types() {
    use crate::tools::classifier::{current_type, set_current_type};
    use esq_core::classify;
    let mut h = Harness::new();
    h.rect_and_box();
    let items: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    let mut first = None;
    h.doc.commit("Composant", |m, d| first = m.make_group(d, &items, false, None));
    let first = first.unwrap();
    // A second copy of the component, 3 m to the right.
    let second = {
        let mut copy = h.doc.ents().instances[first].clone();
        copy.xform = esq_core::glam::DMat4::from_translation(DVec3::new(3000.0, 0.0, 0.0)) * copy.xform;
        let mut id = None;
        h.doc.commit("Copie", |m, d| id = Some(m.entities_mut(d).instances.insert(copy)));
        id.unwrap()
    };
    h.doc.commit("Classements", |m, _| {
        classify::store_schema(
            m,
            &classify::Schema {
                name: "IFC Test".into(),
                namespace: String::new(),
                path: String::new(),
                types: vec!["IfcWall".into(), "IfcDoor".into()],
            },
        )
    });
    set_current_type(Some(("IFC Test".into(), "IfcWall".into())));
    h.tool(ToolId::Classifier);
    assert_eq!(
        h.tool.status(),
        "Cliquez sur un composant ou groupe pour lui attribuer un type. | Alt = Échantillonner un type. | \
         Maj = Rendez-le unique et attribuez-lui un type. | Ctrl = Supprimer le type."
    );
    let on_first = DVec3::new(500.0, 300.0, 400.0);
    let on_second = DVec3::new(3500.0, 300.0, 400.0);
    h.click(on_first);
    let def_of = |h: &Harness, i| h.doc.ents().instances[i].def;
    let types = |h: &Harness, i| classify::applied(&h.doc.model, def_of(h, i));
    let wall = vec![("IFC Test".to_string(), "IfcWall".to_string())];
    let door = vec![("IFC Test".to_string(), "IfcDoor".to_string())];
    assert_eq!(types(&h, first), wall);
    assert_eq!(types(&h, second), wall, "the copies share the definition");
    // Shift tapped: the second copy is made unique and gets the door type.
    set_current_type(Some(("IFC Test".into(), "IfcDoor".into())));
    h.with(|t, cx| t.shift_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez sur une instance de composant pour la rendre unique"));
    h.click(on_second);
    assert_ne!(def_of(&h, first), def_of(&h, second));
    assert_eq!((types(&h, first), types(&h, second)), (wall.clone(), door.clone()));
    // Ctrl tapped: remove the type of the first one.
    h.with(|t, cx| t.ctrl_tap(cx));
    h.click(on_first);
    assert!(types(&h, first).is_empty());
    assert_eq!(h.doc.undo_label(), Some("Supprimer le type"));
    // Alt tapped: sample the second one's type, then back to assigning.
    set_current_type(None);
    h.with(|t, cx| t.alt_tap(cx));
    h.hover(on_second);
    h.click(on_second);
    assert_eq!(current_type(), Some(("IFC Test".into(), "IfcDoor".into())));
    assert!(h.tool.status().starts_with("Cliquez sur un composant ou groupe pour lui attribuer"));
    h.click(on_first);
    assert_eq!(types(&h, first), door);
    set_current_type(None);
}

#[test]
fn photo_point_draws_a_guide_ray_from_the_camera_through_the_click() {
    let mut h = Harness::new();
    h.tool(ToolId::PhotoPoint);
    assert!(h.tool.status().starts_with("Sélectionnez un point sur l'image pour tracer le guide."));
    let p = DVec3::new(800.0, 200.0, 0.0);
    h.click(p);
    assert!(h.tool.done());
    let ents = h.doc.ents();
    assert_eq!(ents.guides.len(), 1);
    let (k, g) = ents.guides.iter().next().unwrap();
    assert!(g.infinite && g.pos.distance(h.cam.eye()) < 1e-6);
    let t = (p - g.pos).dot(g.dir);
    assert!(t > 0.0 && (g.pos + g.dir * t).distance(p) < 1e-3, "the ray goes through the clicked point");
    use slotmap::Key;
    let key = format!("g{}", k.data().as_ffi());
    assert_eq!(ents.prop(&key, "ray"), Some(&esq_core::model::AttrValue::Text("start".into())));
    assert_eq!(h.doc.undo_label(), Some("Guide point photo"));
}

#[test]
fn add_snap_places_snaps_as_oriented_or_by_three_clicks() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::AddSnap);
    assert!(h.tool.status().starts_with("Cliquez pour définir la position de l'ancrage"));
    // Double-click on the front face: the snap faces like the face, its up is the blue axis.
    double_click(&mut h, DVec3::new(500.0, 0.0, 200.0));
    let s = h.doc.ents().snaps[0];
    assert!(s.pos.distance(DVec3::new(500.0, 0.0, 200.0)) < 1e-3, "{:?}", s.pos);
    assert!(s.dir.distance(DVec3::NEG_Y) < 1e-9 && s.up.distance(DVec3::Z) < 1e-9, "{s:?}");
    // Position, forward, up.
    h.click(DVec3::new(500.0, 300.0, 400.0));
    assert!(h.tool.status().starts_with("Cliquer pour définir la direction vers l'avant"));
    h.click(DVec3::new(1000.0, 300.0, 400.0));
    assert_eq!(h.tool.status(), "Cliquer pour définir la direction « vers le haut ».");
    h.click(DVec3::new(500.0, 0.0, 400.0));
    let s = h.doc.ents().snaps[1];
    assert!(s.dir.distance(DVec3::X) < 1e-9 && s.up.distance(DVec3::NEG_Y) < 1e-9, "{s:?}");
    // The angle typed turns the up around the forward direction.
    h.click(DVec3::new(500.0, 300.0, 400.0));
    h.click(DVec3::new(1000.0, 300.0, 400.0));
    h.type_vcb("90");
    let s = h.doc.ents().snaps[2];
    assert!(s.up.distance(DVec3::NEG_Y) < 1e-9, "{s:?}");
    assert_eq!(h.doc.undo_label(), Some("Ajouter un ancrage"));
}

#[test]
fn light_tool_places_a_point_light() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Light);
    assert_eq!(h.tool.status(), "Cliquez pour placer une lumière dans le modèle.");
    h.click(DVec3::new(500.0, 300.0, 400.0));
    let l = h.doc.ents().lights[0];
    assert!(l.pos.distance(DVec3::new(500.0, 300.0, 400.0)) < 1e-3);
    assert_eq!((l.color, l.intensity, l.size), ([1.0; 3], 1.0, 12.0));
    assert_eq!(h.doc.undo_label(), Some("Placer la lumière"));
}

/// A scene with a matched photo taken from `eye` toward the origin.
pub(super) fn photo_scene(name: &str, eye: DVec3) -> esq_core::model::Scene {
    use esq_core::glam::DVec2;
    let size = DVec2::new(1600.0, 1200.0);
    let camera = crate::tools::photo_tests::look_camera(eye, DVec3::ZERO, 1400.0, size, size * 0.5);
    let photo = esq_core::photo::MatchPhoto {
        name: format!("{name}.png"),
        image: esq_core::photo::Blob(std::sync::Arc::new(vec![1, 2, 3])),
        size: [1600, 1200],
        setup: esq_core::matchphoto::Setup {
            size,
            principal: None,
            lines: [None, None, None],
            origin: DVec2::ZERO,
            scale: esq_core::matchphoto::Scale::Distance(eye.length()),
            variant: None,
        },
        camera,
        grid: Default::default(),
    };
    esq_core::model::Scene {
        name: name.into(),
        description: String::new(),
        camera: photo.scene_camera(),
        hidden_layers: None,
        photo: Some(photo),
        ext: Default::default(),
    }
}

#[test]
fn image_igloo_picks_photos_with_the_arrows_and_activates_with_enter() {
    let mut h = Harness::new();
    h.rect_and_box();
    let eyes =
        [DVec3::new(6000.0, 300.0, 1500.0), DVec3::new(500.0, 6000.0, 1500.0), DVec3::new(-5000.0, 300.0, 1500.0)];
    h.doc.commit("Photos", |m, _| {
        for (n, e) in ["A", "B", "C"].iter().zip(eyes) {
            m.scenes.push(photo_scene(n, e));
        }
    });
    // Look from about where B was taken: the igloo picks B.
    let size = h.cam.size;
    h.cam = crate::camera::Camera::from_photo(
        &h.doc.model.scenes[1].photo.as_ref().unwrap().camera,
        size,
        1.0,
        Default::default(),
    )
    .0;
    h.cam.distance *= 2.0;
    h.tool(ToolId::Igloo);
    h.hover_px(esq_core::glam::DVec2::new(640.0, 400.0));
    assert_eq!(
        h.tool.status(),
        "Utilisez les flèches pour sélectionner la scène.  Maj = dans l'ordre, Entrer pour activer B."
    );
    // Shift + right arrow: the next photo scene in order, and the view goes there.
    h.mods = super::SHIFT;
    h.with(|t, cx| t.key(cx, 0x27, true, 1));
    h.mods = Modifiers::NONE;
    assert!(h.tool.status().ends_with("activer C."));
    assert!(h.cam.eye().distance(eyes[2]) < 1e-3, "{:?}", h.cam.eye());
    // Enter: C becomes the active scene and the tool ends.
    h.with(|t, cx| t.enter(cx));
    assert_eq!(h.tool.finished(), Some(Some(2)));
}

#[test]
fn move_rotation_grips_turn_a_component_about_a_face_of_its_box() {
    use crate::tools::move_grips::grips_enabled;
    let mut h = Harness::new();
    h.rect_and_box();
    let items: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    let mut inst = None;
    h.doc.commit("Composant", |m, d| inst = m.make_group(d, &items, false, None));
    let inst = inst.unwrap();
    h.doc.select(Vec::new());
    assert!(grips_enabled(), "on by default, as in SketchUp");
    h.tool(ToolId::Move);
    // The grip in the middle of the top face of the component's box.
    let top = DVec3::new(500.0, 300.0, 400.0);
    h.hover(top);
    h.click(top);
    assert!(h.tool.busy());
    assert_eq!(h.tool.vcb_label(), "Angle");
    h.type_vcb("90");
    let x = h.doc.ents().instances[inst].xform;
    assert!(x.transform_point3(DVec3::ZERO).distance(DVec3::new(800.0, -200.0, 0.0)) < 1e-6, "{x:?}");
    assert_eq!(h.doc.undo_label(), Some("Faire pivoter"));
    assert!(!h.tool.busy());
}

#[test]
fn set_sun_location_links_the_sun_to_the_point_clicked_on_the_environment() {
    let mut m = esq_core::model::Model::new();
    assert!(edit_ops::set_sun_location(&mut m, 0.5, 0.5).is_none(), "no environment");
    // An environment as the Ruby API adds it (Environments#add), made current.
    m.view_ext.insert(
        "envs".into(),
        serde_json::json!([{"id": 1, "name": "Ciel", "path": "", "rotation": 0.0, "linked_sun": false, "sun_pos": [0.0, 45.0]}]),
    );
    m.view_ext.insert("env_current".into(), serde_json::json!(1));
    assert!(edit_ops::linked_sun_direction(&m).is_none(), "not linked yet");
    // A quarter of the way across, a quarter down: azimuth 90°, elevation 45°.
    assert_eq!(edit_ops::set_sun_location(&mut m, 0.25, 0.25), Some([90.0, 45.0]));
    // Where the skydome shows that point of the panorama (SketchUp's mapping: a quarter turn
    // from the left edge, which faces the red axis' negative side, is the green axis).
    let d = edit_ops::linked_sun_direction(&m).unwrap();
    let h = 0.5f64.sqrt();
    assert!(d.distance(DVec3::new(0.0, h, h)) < 1e-9, "{d:?}");
    // The environment's rotation turns the sun with it; below the horizon is clamped.
    m.view_ext.get_mut("envs").unwrap()[0]["rotation"] = serde_json::json!(90.0);
    assert_eq!(edit_ops::set_sun_location(&mut m, 0.0, 0.9), Some([0.0, 0.0]));
    assert!(edit_ops::linked_sun_direction(&m).unwrap().distance(-DVec3::Y) < 1e-9);
    assert!(edit_ops::set_sun_linked(&mut m, false));
    assert!(edit_ops::linked_sun_direction(&m).is_none());
}

#[test]
fn optimize_memory_keeps_the_last_undo_steps_only() {
    let mut h = Harness::new();
    for i in 0..25 {
        h.doc.commit(&format!("pas {i}"), |m, d| {
            m.entities_mut(d).guides.insert(esq_core::model::Guide::point(DVec3::splat(i as f64)));
        });
    }
    h.doc.undo();
    assert_eq!(h.doc.history_len(), (24, 1));
    assert_eq!(edit_ops::optimize_memory(&mut h.doc), 14 + 1);
    assert_eq!(h.doc.history_len(), (edit_ops::OPTIMIZE_UNDO_KEEP, 0));
    // The model is untouched and the most recent steps still undo.
    assert_eq!(h.doc.ents().guides.len(), 24);
    assert_eq!(h.doc.undo_label(), Some("pas 23"));
}

#[test]
fn orient_faces_turns_the_box_like_the_clicked_face() {
    let mut h = Harness::new();
    h.rect_and_box();
    // Two side faces reversed by hand, then Orient Faces from the top face.
    let mut flip: Vec<FaceId> =
        h.doc.ents().faces.iter().filter(|(_, f)| f.normal.z.abs() < 0.5).map(|(k, _)| k).collect();
    flip.truncate(2);
    h.doc.commit("test", |m, d| {
        for f in &flip {
            m.entities_mut(d).reverse_face(*f);
        }
    });
    h.tool(ToolId::Select);
    h.click(DVec3::new(500.0, 300.0, 400.0));
    assert!(matches!(h.doc.sel.as_slice(), [EntityRef::Face(_)]));
    assert_eq!(edit_ops::orient_faces(&mut h.doc), 2);
    // Every face of the box faces outwards again, like the top one.
    let ents = h.doc.ents();
    let c = DVec3::new(500.0, 300.0, 200.0);
    for f in ents.faces.keys() {
        let pts = ents.loop_points(&ents.faces[f].outer);
        let mid = pts.iter().copied().sum::<DVec3>() / pts.len() as f64;
        assert!(ents.faces[f].normal.dot(mid - c) > 0.0);
    }
}

// ---- Status messages of the tools already ticked (CPushPullTool, …) ----

/// A 1000 x 600 rectangle on the ground.
fn rect() -> Harness {
    let mut h = Harness::new();
    h.tool(ToolId::Rect);
    h.click(DVec3::ZERO);
    h.hover(DVec3::new(900.0, 500.0, 0.0));
    h.type_vcb("1000;600");
    h
}

#[test]
fn push_pull_modes_alt_stretch_and_a_preselected_face() {
    const MODS: &str =
        " | Ctrl = Activer/désactiver créer une nouvelle face de départ. | Alt = Activer/Désactiver le mode Étirer.";
    let mut h = rect();
    h.tool(ToolId::PushPull);
    assert_eq!(h.tool.status(), format!("Cliquez pour sélectionner la face que vous souhaitez pousser/tirer.{MODS}"));
    // Alt toggles the stretch mode (exclusive with Ctrl's new starting face).
    h.with(|t, cx| t.ctrl_tap(cx));
    h.with(|t, cx| t.alt_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez pour sélectionner la face que vous souhaitez étirer. |"));
    h.click(DVec3::new(500.0, 300.0, 0.0));
    assert!(h.tool.status().starts_with("Cliquez pour définir les faces que vous étirez ou entrez la distance."));
    h.hover(DVec3::new(500.0, 300.0, 250.0));
    h.type_vcb("400");
    assert_eq!(h.faces(), 1, "stretched, not extruded");
    // A preselected face is pushed whatever is clicked.
    let mut h = rect();
    let f = h.doc.ents().faces.keys().next().unwrap();
    h.doc.select(vec![EntityRef::Face(f)]);
    h.tool(ToolId::PushPull);
    h.hover(DVec3::new(3000.0, 3000.0, 0.0));
    assert_eq!(h.tool.status(), format!("Cliquez pour pousser ou tirer la face sélectionnée.{MODS}"));
    h.click(DVec3::new(3000.0, 3000.0, 0.0));
    assert_eq!(h.tool.status(), format!("Cliquez pour définir la face ou entrez la distance.{MODS}"));
    h.hover(DVec3::new(3000.0, 3000.0, 500.0));
    h.type_vcb("300");
    assert_eq!(h.faces(), 6);
    assert!((h.zmax() - 300.0).abs() < 1e-6, "z = {}", h.zmax());
}

#[test]
fn offset_trims_its_overlap_unless_alt_allows_it() {
    let mut h = Harness::new();
    h.tool(ToolId::Rect);
    h.click(DVec3::ZERO);
    h.hover(DVec3::new(900.0, 150.0, 0.0));
    h.type_vcb("1000;200");
    h.tool(ToolId::Offset);
    h.hover(DVec3::new(3000.0, 3000.0, 0.0));
    assert_eq!(h.tool.status(), "Sélectionnez la face ou les arêtes à décaler. Alt = autoriser le chevauchement.");
    h.hover(DVec3::new(500.0, 100.0, 0.0));
    assert_eq!(
        h.tool.status(),
        "Sélectionnez le point à partir duquel le décalage sera mesuré. Alt = autoriser le chevauchement."
    );
    h.click(DVec3::new(500.0, 100.0, 0.0));
    assert!(h.tool.status().starts_with("Sélectionnez un point pour définir le décalage ou tapez une valeur."));
    let before = h.doc.ents().edges.len();
    // 150 mm inwards in a 200 mm wide face folds the offset over itself: trimmed away.
    h.type_vcb("150");
    assert_eq!(h.doc.ents().edges.len(), before);
    h.with(|t, cx| t.alt_tap(cx));
    assert!(h.tool.status().ends_with("Alt = découper le chevauchement."));
    h.click(DVec3::new(500.0, 100.0, 0.0));
    h.type_vcb("150");
    assert_eq!(h.doc.ents().edges.len(), before + 4, "the crossed loop is drawn");
}

#[test]
fn eraser_modes_toggle_with_taps_and_ctrl_shift_deselects_while_dragging() {
    const MODS: &str = " | Ctrl = Activer/Désactiver adoucir/lisser. \
         | Alt = Activer/Désactiver annuler l'adoucissement/révéler. | Maj = Activer/Désactiver masquer.";
    let mut h = Harness::new();
    h.doc.commit("lignes", |m, d| {
        for x in [0.0, 500.0, 1000.0] {
            m.entities_mut(d).add_line(DVec3::new(x, 0.0, 0.0), DVec3::new(x, 1000.0, 0.0));
        }
    });
    let edge_at = |h: &Harness, x: f64| {
        let ents = h.doc.ents();
        ents.edges.keys().find(|e| (ents.edge_points(*e).0.x - x).abs() < 1e-6).map(|e| ents.edges[e].clone())
    };
    h.tool(ToolId::Eraser);
    assert_eq!(h.tool.status(), format!("Cliquez ou faites glisser pour effacer les éléments.{MODS}"));
    h.with(|t, cx| t.alt_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez ou faites glisser annuler l'adoucissement/révéler"));
    h.with(|t, cx| t.shift_tap(cx));
    assert_eq!(h.tool.status(), format!("Cliquez ou faites glisser pour masquer les éléments.{MODS}"));
    h.click(DVec3::new(0.0, 500.0, 0.0));
    assert!(edge_at(&h, 0.0).unwrap().hidden, "the Shift tap hides");
    h.with(|t, cx| t.shift_tap(cx));
    h.with(|t, cx| t.ctrl_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez ou faites glisser pour adoucir/lisser les arêtes. |"));
    h.with(|t, cx| t.ctrl_tap(cx));
    // Ctrl held on a click is not a tap when it is released.
    h.mods = CTRL;
    h.click(DVec3::new(500.0, 500.0, 0.0));
    assert!(edge_at(&h, 500.0).unwrap().soft);
    h.mods = Modifiers::NONE;
    h.with(|t, cx| t.ctrl_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez ou faites glisser pour effacer les éléments."));
    h.doc.undo();
    // A drag over the three lines, back over the middle one with Ctrl+Shift: it is spared.
    h.pick_rev = u64::MAX;
    let (a, b, c) =
        (h.px(DVec3::new(0.0, 500.0, 0.0)), h.px(DVec3::new(500.0, 500.0, 0.0)), h.px(DVec3::new(1000.0, 500.0, 0.0)));
    h.with(|t, cx| {
        t.mouse_move(cx, a);
        t.press(cx, a);
        t.mouse_move(cx, b);
        t.mouse_move(cx, c);
    });
    assert!(h.tool.status().ends_with(" | Ctrl + Maj = Activer/Désactiver désélectionner."));
    h.mods = super::CTRL_SHIFT;
    h.hover_px(b);
    assert!(h.tool.status().starts_with("Continuez à faire glisser pour désélectionner des éléments. |"));
    h.with(|t, cx| t.release(cx, b));
    h.mods = Modifiers::NONE;
    assert!(edge_at(&h, 500.0).is_some(), "deselected");
    assert!(edge_at(&h, 1000.0).is_none());
    assert!(edge_at(&h, 0.0).is_some(), "hidden: not picked");
}

#[test]
fn paint_bucket_status_follows_the_held_modifiers() {
    const LIST: &str = " | Alt = Prélevez un échantillon de la matière. | Maj = Coloriez toutes les faces correspondantes. \
         | Ctrl = Colorier tous les éléments connectés. | Maj + Ctrl = Tout colorier sur le même objet.";
    let mut h = rect();
    h.tool(ToolId::Paint);
    let p = DVec3::new(500.0, 300.0, 0.0);
    for (mods, main) in [
        (Modifiers::NONE, "Cliquez pour colorier un élément ou objet."),
        (CTRL, "Cliquez pour colorier les faces connectées avec la matière correspondante."),
        (Modifiers::SHIFT, "Cliquez pour colorier les faces correspondantes."),
        (super::CTRL_SHIFT, "Cliquez pour colorier les faces correspondantes du même objet."),
    ] {
        h.mods = mods;
        h.hover(p);
        assert_eq!(h.tool.status(), format!("{main}{LIST}"));
    }
    h.mods = Modifiers::ALT;
    h.hover(p);
    assert_eq!(h.tool.status(), "Cliquez sur une face pour charger sa matière dans l'outil Colorier.");
}

#[test]
fn protractor_status_and_ctrl_measure_only() {
    const CTRL_TEXT: &str = " | Ctrl = Activer/désactiver la création de guides.";
    let mut h = Harness::new();
    h.tool(ToolId::Protractor);
    assert_eq!(
        h.tool.status(),
        format!(
            "Cliquez pour définir le centre du rapporteur.{CTRL_TEXT} \
             | Flèches = Activer/Désactiver verrouiller le plan de rotation."
        )
    );
    h.click(DVec3::ZERO);
    assert_eq!(h.tool.status(), format!("Cliquez pour créer un guide.{CTRL_TEXT}"));
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    assert_eq!(h.tool.status(), format!("Cliquez pour placer le guide ou entrez l'angle.{CTRL_TEXT}"));
    h.with(|t, cx| t.ctrl_tap(cx));
    assert_eq!(h.tool.status(), format!("Cliquez pour mesurer l'angle.{CTRL_TEXT}"));
    h.type_vcb("30");
    assert!(h.doc.ents().guides.is_empty(), "measured only");
}

#[test]
fn tape_cycles_guide_lines_points_and_measure_then_resizes_the_model() {
    let mut h = rect();
    h.tool(ToolId::Tape);
    let status = |h: &Harness| h.tool.status().split(" | ").next().unwrap().to_string();
    assert!(h.tool.status().ends_with(
        " | Ctrl = Alterner entre lignes de guide/points de guide/mesure. \
         | Flèches = Activer/Désactiver verrouiller la direction de l'inférence."
    ));
    assert_eq!(status(&h), "Cliquez pour créer un guide.");
    // Guide points: even from an edge, and a double-click drops one.
    h.with(|t, cx| t.ctrl_tap(cx));
    assert_eq!(status(&h), "Double-cliquez, ou cliquez et mesurez, pour créer un point de guide");
    h.click(DVec3::new(300.0, 0.0, 0.0));
    h.hover(DVec3::new(300.0, -300.0, 0.0));
    h.type_vcb("250");
    double_click(&mut h, DVec3::new(1000.0, 600.0, 0.0));
    let guides: Vec<_> = h.doc.ents().guides.values().cloned().collect();
    assert_eq!(guides.len(), 2);
    assert!(guides.iter().all(|g| g.is_point()));
    // Measure: the long side is 1000 mm; typing 2000 afterwards doubles the model.
    h.with(|t, cx| t.ctrl_tap(cx));
    assert_eq!(status(&h), "Cliquez sur un élément à partir duquel mesurer.");
    h.click(DVec3::new(0.0, 0.0, 0.0));
    assert_eq!(status(&h), "Cliquez sur un élément à mesurer ou entrez la distance.");
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    assert_eq!(h.doc.ents().guides.len(), 2);
    h.with(|t, cx| t.ctrl_tap(cx));
    assert_eq!(status(&h), "Cliquez pour créer un guide ou entrez la distance pour redimensionner le modèle.");
    h.type_vcb("2000");
    let xmax = h.doc.ents().verts.values().map(|v| v.pos.x).fold(f64::MIN, f64::max);
    assert!((xmax - 2000.0).abs() < 1e-6, "xmax = {xmax}");
    assert!(h.doc.ents().guides.values().any(|g| g.pos.distance(DVec3::new(2000.0, 1200.0, 0.0)) < 1e-6));
}

#[test]
fn scale_status_by_step_and_toggles() {
    const MODS: &str = " | Ctrl = Activer/Désactiver redimensionner autour du centre. \
         | Maj = Activer/désactiver mise à l’échelle uniforme.";
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Scale);
    let far = DVec3::new(5000.0, 5000.0, 0.0);
    h.hover(far);
    assert_eq!(h.tool.status(), format!("Cliquez sur la face ou l'objet que vous souhaitez redimensionner.{MODS}"));
    let all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    h.doc.select(all);
    h.hover(far);
    assert!(
        h.tool.status().starts_with("Cliquez sur une poignée de mise à l'échelle pour commencer à redimensionner. |")
    );
    h.with(|t, cx| t.shift_tap(cx));
    assert!(
        h.tool
            .status()
            .starts_with("Cliquez sur une poignée de mise à l'échelle pour commencer à redimensionner uniformément. |")
    );
    h.with(|t, cx| t.ctrl_tap(cx));
    assert!(h.tool.status().starts_with(
        "Cliquez sur une poignée de mise à l'échelle pour commencer à redimensionner uniformément autour du centre. |"
    ));
    // Shift held while moving is not a tap when it is released.
    h.mods = Modifiers::SHIFT;
    h.hover(far);
    assert!(h.tool.status().contains("redimensionner autour du centre. |"), "held Shift toggles back");
    h.mods = Modifiers::NONE;
    h.with(|t, cx| t.shift_tap(cx));
    h.hover(far);
    assert!(h.tool.status().contains("uniformément autour du centre. |"));
}

#[test]
fn follow_me_alt_uses_the_perimeter_of_the_face_under_the_cursor() {
    let mut h = rect();
    h.doc.commit("Profil", |m, d| {
        let pts = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(0.0, -50.0, 0.0),
            DVec3::new(0.0, -50.0, 80.0),
            DVec3::new(0.0, 0.0, 80.0),
        ];
        m.entities_mut(d).add_polyline(&pts, true, false);
    });
    h.tool(ToolId::FollowMe);
    h.hover(DVec3::new(3000.0, 3000.0, 0.0));
    assert_eq!(h.tool.status(), "Cliquez sur le profil que vous souhaitez extruder.");
    h.click(DVec3::new(0.0, -25.0, 40.0));
    const ALT_TEXT: &str = " | Alt = Activez/désactivez Utiliser le périmètre de la zone.";
    assert_eq!(h.tool.status(), format!("Déplacez le curseur le long du chemin d'extrusion souhaité.{ALT_TEXT}"));
    h.with(|t, cx| t.alt_tap(cx));
    assert_eq!(
        h.tool.status(),
        format!("Passez le curseur sur une face pour utiliser son périmètre comme chemin d'extrusion.{ALT_TEXT}")
    );
    // The middle of the base face: its outline is the path.
    h.click(DVec3::new(500.0, 300.0, 0.0));
    assert_eq!(h.faces(), 1 + 16);
    // A path selected first changes the first message.
    h.doc.undo();
    let base = h.doc.ents().faces.keys().find(|f| h.doc.ents().faces[*f].normal.z.abs() > 0.5).unwrap();
    h.doc.select(vec![EntityRef::Face(base)]);
    h.tool(ToolId::FollowMe);
    h.hover(DVec3::new(3000.0, 3000.0, 0.0));
    assert_eq!(h.tool.status(), "Cliquez sur un profil à extruder le long du chemin d'accès présélectionné.");
}

#[test]
fn tag_tool_modes_sample_replace_matching_and_all_instances() {
    const MODS: &str = " | Alt = Activer/désactiver l'étiquette de l'échantillon. \
         | Maj = Activer/désactiver Remplacer les correspondances. \
         | Ctrl = Activer/désactiver l'application de l'étiquette à toutes les instances.";
    // A box made a component, a second instance of it 2 m away, and a loose line; all on tag 1.
    let mut h = Harness::new();
    h.rect_and_box();
    let faces: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    let mut inst = None;
    h.doc.commit("Composant", |m, d| {
        inst = m.make_group(d, &faces, false, Some("Boîte".into()));
        let tag = m.add_layer("Meubles");
        let e = m.entities_mut(d);
        let mut copy = e.instances[inst.unwrap()].clone();
        copy.xform = esq_core::glam::DMat4::from_translation(DVec3::new(0.0, 2000.0, 0.0)) * copy.xform;
        let second = e.instances.insert(copy);
        let line = e.add_line(DVec3::new(-1000.0, 0.0, 0.0), DVec3::new(-1000.0, 1000.0, 0.0))[0];
        for r in [EntityRef::Instance(inst.unwrap()), EntityRef::Instance(second), EntityRef::Edge(line)] {
            e.set_layer(r, tag);
        }
    });
    let tagged = |h: &Harness| {
        let e = h.doc.ents();
        e.instances.values().filter(|i| i.layer == 1).count() + e.edges.values().filter(|x| x.layer == 1).count()
    };
    assert_eq!(tagged(&h), 3);
    h.tool(ToolId::TagTool);
    assert_eq!(h.tool.status(), format!("Cliquez sur un objet pour appliquer une étiquette.{MODS}"));
    h.with(|t, cx| t.alt_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez sur un objet pour utiliser son étiquette avec l'outil Étiquette."));
    h.with(|t, cx| t.alt_tap(cx));
    // Ctrl: every instance of the component clicked gets the current tag (Untagged here).
    h.with(|t, cx| t.ctrl_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez pour attribuer une étiquette à toutes les instances d'un composant."));
    h.click(DVec3::new(500.0, 300.0, 400.0));
    assert_eq!(tagged(&h), 1, "both instances, not the line");
    h.doc.undo();
    // Shift: everything that has the clicked entity's tag.
    h.with(|t, cx| t.ctrl_tap(cx));
    h.with(|t, cx| t.shift_tap(cx));
    assert!(h.tool.status().starts_with("Cliquez sur pour remplacer l'étiquette"));
    h.click(DVec3::new(500.0, 300.0, 400.0));
    assert_eq!(tagged(&h), 0);
}

#[test]
fn flip_with_arrow_keys_copy_and_a_typed_plane_offset() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Flip);
    let far = DVec3::new(5000.0, 5000.0, 0.0);
    h.hover(far);
    assert_eq!(
        h.tool.status(),
        "Cliquez sur quelque chose que vous souhaitez retourner. | Ctrl = Alterner entre Retournement/Copie."
    );
    let all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    h.doc.select(all);
    h.hover(far);
    assert_eq!(
        h.tool.status(),
        "Cliquez ou faites glisser un plan pour retourner la sélection. | Ctrl = Alterner entre Retournement/Copie. \
         | Touches fléchées = Retourner sur un plan."
    );
    // Ctrl, then the right arrow: a mirrored copy across the red plane (x = 500).
    h.with(|t, cx| t.ctrl_tap(cx));
    let took = h.with(|t, cx| t.own_key(cx, eframe::egui::Key::ArrowRight, Modifiers::NONE));
    assert!(took);
    assert_eq!(h.faces(), 12);
    h.hover(far);
    assert!(h.tool.status().starts_with(
        "Cliquez ou faites glisser un plan pour retourner et copier la sélection. \
         Entrer une distance pour ajuster le décalage du plan du miroir. |"
    ));
    // 500 typed: the plane moves to x = 1000, the copy lands on x = 1000..2000.
    h.type_vcb("500");
    assert_eq!(h.faces(), 12);
    let xmax = h.doc.ents().verts.values().map(|v| v.pos.x).fold(f64::MIN, f64::max);
    assert!((xmax - 2000.0).abs() < 1e-6, "xmax = {xmax}");
}

#[test]
fn axes_alt_sets_green_first_and_a_double_click_keeps_the_orientation() {
    let mut h = Harness::new();
    h.tool(ToolId::AxesTool);
    assert_eq!(
        h.tool.status(),
        "Cliquez pour définir la nouvelle origine ou double-cliquez pour définir les axes selon leur orientation actuelle."
    );
    // A double-click on a point: only the origin moves.
    double_click(&mut h, DVec3::new(300.0, 200.0, 0.0));
    let a = h.doc.model.axes;
    assert!(a.w_axis.truncate().distance(DVec3::new(300.0, 200.0, 0.0)) < 1e-6);
    assert!(a.x_axis.truncate().distance(DVec3::X) < 1e-9);
    // Alt: the green axis is set first, then the blue one.
    h.click(DVec3::ZERO);
    h.with(|t, cx| t.alt_tap(cx));
    assert_eq!(
        h.tool.status(),
        "Cliquez pour définir l'axe vert ou double-cliquez pour définir les axes selon leur orientation. \
         Alt = alterner l'orientation de l'axe (l'axe vert est verrouillé)."
    );
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    assert!(h.tool.status().starts_with("Cliquez pour définir l'axe bleu. Alt ="));
    h.click(DVec3::new(0.0, 1000.0, 0.0));
    let a = h.doc.model.axes;
    assert!(a.y_axis.truncate().distance(DVec3::X) < 1e-9, "green along world x");
    assert!(a.z_axis.truncate().distance(DVec3::Y) < 1e-9, "blue along world y");
    assert!(a.x_axis.truncate().distance(DVec3::Z) < 1e-9, "red = green × blue");
}

#[test]
fn solid_tools_ask_for_the_first_then_the_second_solid() {
    let mut h = Harness::new();
    h.rect_and_box();
    let items: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    h.doc.commit("Groupe", |m, d| {
        m.make_group(d, &items, true, None);
    });
    for tool in [ToolId::SolidSubtract, ToolId::SolidUnion] {
        h.tool(tool);
        assert_eq!(h.tool.status(), "Sélectionnez le premier solide.");
        h.click(DVec3::new(100.0, 100.0, 400.0));
        assert_eq!(h.tool.status(), "Sélectionnez le deuxième solide.");
    }
}

#[test]
fn walk_stops_before_a_wall_unless_alt_is_held() {
    let walk_to_wall = |alt: bool| {
        let mut h = Harness::new();
        // A wall across the y axis, 10 m wide and 5 m high.
        h.doc.commit("Mur", |m, d| {
            let pts = [
                DVec3::new(-5000.0, 0.0, 0.0),
                DVec3::new(5000.0, 0.0, 0.0),
                DVec3::new(5000.0, 0.0, 5000.0),
                DVec3::new(-5000.0, 0.0, 5000.0),
            ];
            m.entities_mut(d).add_polyline(&pts, true, false);
        });
        h.tool(ToolId::PositionCamera);
        assert_eq!(h.tool.status(), "Sélectionnez la position de la caméra.");
        // Stand 3 m in front of the wall, looking at it.
        let (a, b) = (h.px(DVec3::new(0.0, -3000.0, 0.0)), h.px(DVec3::new(0.0, 0.0, 1680.0)));
        h.with(|t, cx| t.press(cx, a));
        assert_eq!(h.tool.status(), "Sélectionnez un point qui sera visé par la caméra.");
        h.with(|t, cx| t.release(cx, b));
        assert!((h.cam.eye().y + 3000.0).abs() < 1e-6);
        h.tool(ToolId::Walk);
        assert!(h.tool.status().ends_with("Alt = désactiver la détection de collision"));
        h.mods = if alt { Modifiers::ALT } else { Modifiers::NONE };
        let m = DVec2::new(400.0, 300.0);
        h.with(|t, cx| {
            t.press(cx, m);
            t.mouse_move(cx, m - DVec2::new(0.0, 400.0));
        });
        h.cam.eye().y
    };
    let y = walk_to_wall(false);
    assert!((y + 300.0).abs() < 1e-3, "stopped 300 mm before the wall: y = {y}");
    let y = walk_to_wall(true);
    assert!(y > 500.0, "Alt walks through: y = {y}");
}

#[test]
fn orbit_with_ctrl_goes_over_the_top_and_gravity_rights_the_view() {
    use crate::camera::Camera;
    let mut c = Camera { pitch: 1.4, ..Camera::default() };
    let (f0, u0) = (c.forward(), c.up());
    // With gravity, the pitch stops short of the vertical.
    let mut g = c.clone();
    g.orbit_with(0.0, 100.0, true);
    assert!(g.pitch <= 1.55 + 1e-12 && g.up().z > 0.0);
    // Gravity suspended: past the vertical, the view is upside down and turns smoothly.
    let steps = 60;
    let mut prev = (f0, u0);
    for _ in 0..steps {
        c.orbit_with(0.0, 1.0, false);
        let (f, u) = (c.forward(), c.up());
        assert!(f.distance(prev.0) < 0.02 && u.distance(prev.1) < 0.02, "continuous");
        prev = (f, u);
    }
    assert!(c.pitch > std::f64::consts::FRAC_PI_2 && c.up().z < 0.0, "upside down");
    assert_eq!(
        crate::tools::ToolId::Orbit.make().status(),
        "Faites glisser pour naviguer. Maj = panoramique, Ctrl =  suspend la gravité."
    );
    // Gravity again: the same direction of view, upright.
    let f = c.forward();
    c.orbit_with(0.0, 0.0, true);
    assert!(c.forward().distance(f) < 1e-9 && c.up().z > 0.0);
}

#[test]
fn pasted_geometry_follows_the_cursor_until_a_click_and_escape_drops_it() {
    let mut h = rect();
    let all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    h.doc.select(all);
    // What Edit › Paste leaves: the pasted selection, then the paste tool.
    h.tool = Box::new(crate::tools::MoveTool::pasting());
    assert_eq!(h.tool.status(), "Placer le contenu du Presse-papiers.");
    h.hover(DVec3::new(3000.0, 2000.0, 0.0));
    assert!(h.tool.busy(), "attached to the cursor");
    h.click(DVec3::new(3000.0, 2000.0, 0.0));
    let xmin = h.doc.ents().verts.values().map(|v| v.pos.x).fold(f64::MAX, f64::min);
    assert!((xmin - 3000.0).abs() < 1e-6, "the low corner is where the click was: {xmin}");
    assert!(h.tool.status().starts_with("Cliquez pour commencer à déplacer"), "a plain Move afterwards");
    // Escape before placing: the paste is undone.
    let before = h.doc.history_len();
    h.doc.commit("Coller", |_, _| {});
    h.tool = Box::new(crate::tools::MoveTool::pasting());
    h.hover(DVec3::new(0.0, 0.0, 0.0));
    h.with(|t, cx| t.cancel(cx));
    assert_eq!(h.doc.history_len().0, before.0);
}

#[test]
fn statuses_of_section_select_dimension_text_and_component_tools() {
    for (tool, status) in [
        (ToolId::SectionPlane, "Positionnez le plan de section sur la face.  Maj = verrouiller sur le plan."),
        (
            ToolId::Select,
            "Cliquez ou faites glisser pour sélectionner des objets. Maj = Ajouter/Soustraire. Ctrl = Ajouter. \
             Maj + Ctrl = Soustraire.",
        ),
        (
            ToolId::Dimension,
            "Sélectionnez une arête, une courbe ou deux points pour coter, ou faites glisser un élément pour le déplacer.",
        ),
        (ToolId::Text, "Sélectionnez l’objet pour y attacher le texte ou le positionner à l’écran."),
        (ToolId::Fov, "Faites glisser la souris ou saisissez un champ angulaire."),
        (ToolId::ZoomWindow, "Définissez une zone à l’aide du pointeur pour effectuer un zoom"),
    ] {
        assert_eq!(tool.make().status(), status, "{tool:?}");
    }
    let mut h = Harness::new();
    h.tool(ToolId::Dimension);
    h.click(DVec3::ZERO);
    assert_eq!(h.tool.status(), "Sélectionnez le second point de la cotation linéaire.");
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    assert_eq!(h.tool.status(), "Placez la cotation.");
}

#[test]
fn lasso_made_of_clicked_segments_closes_on_a_double_click() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Lasso);
    assert!(
        h.tool.status().starts_with("Cliquez ou cliquez et faites glisser pour tracer les limites de la sélection. |")
    );
    // Four clicks around the box (clockwise on screen), the last one a double-click.
    let c = h.px(DVec3::new(500.0, 300.0, 200.0));
    let corners = [DVec2::new(-400.0, -400.0), DVec2::new(400.0, -400.0), DVec2::new(400.0, 400.0)];
    for d in corners {
        let m = c + d;
        h.with(|t, cx| {
            t.mouse_move(cx, m);
            t.press(cx, m);
            t.release(cx, m);
        });
        assert_eq!(h.tool.status(), "Double-cliquez pour fermer les limites de la sélection.");
    }
    let m = c + DVec2::new(-400.0, 400.0);
    h.with(|t, cx| {
        t.mouse_move(cx, m);
        t.press(cx, m);
        t.release(cx, m);
        t.press(cx, m);
        t.release(cx, m);
        t.double_click(cx, m);
    });
    assert_eq!(h.doc.sel.len(), 6 + 12, "6 faces and 12 edges");
    assert!(!h.tool.busy());
    // North: the two steps.
    h.tool(ToolId::North);
    assert_eq!(h.tool.status(), "Sélectionnez le point de référence du nord");
    h.click(DVec3::ZERO);
    assert_eq!(h.tool.status(), "Sélectionnez la direction du nord.");
}

#[test]
fn match_photo_hint_is_sketchup_s() {
    let png = crate::photo_img::tests::png_bytes(800, 600);
    let ed = crate::tools::PhotoEditor::new(
        "mur.png".into(),
        esq_core::photo::Blob(std::sync::Arc::new(png)),
        DVec2::new(800.0, 600.0),
    );
    assert_eq!(ed.hint(), "Une fois terminé, cliquez en dehors de l’image.");
}
