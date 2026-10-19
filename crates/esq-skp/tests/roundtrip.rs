use std::path::PathBuf;

use esq_core::glam::{DMat4, DVec3};
use esq_core::model::{EntityRef, Guide, Model};

fn sample_model() -> Model {
    let mut m = Model::new();
    let wood = m.add_material("Bois", [180, 120, 60]);
    let e = &mut m.root;
    e.add_polyline(
        &[DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, 600.0, 0.0), DVec3::new(0.0, 600.0, 0.0)],
        true,
        false,
    );
    let f = e.faces.keys().next().unwrap();
    e.push_pull(f, -400.0, false);
    let top = e.faces.iter().find(|(_, x)| x.normal.z > 0.99).map(|(k, _)| k).unwrap();
    e.faces[top].front = Some(wood);
    let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    let inst = m.make_group(None, &items, false, Some("Caisson".into())).unwrap();
    {
        use esq_core::model::{AttrValue, set_attr};
        let def = m.root.instances[inst].def;
        set_attr(&mut m.defs[def].attributes, "dynamic_attributes", "hauteur", AttrValue::Number(400.5));
        set_attr(&mut m.defs[def].attributes, "dynamic_attributes", "visible", AttrValue::Bool(true));
        set_attr(&mut m.root.instances[inst].attributes, "fiche", "essence", AttrValue::Text("Chêne".into()));
    }
    let mut copy = m.root.instances[inst].clone();
    copy.xform = DMat4::from_translation(DVec3::new(1500.0, 0.0, 0.0)) * copy.xform;
    m.root.instances.insert(copy);
    m.root.add_line(DVec3::new(0.0, -500.0, 0.0), DVec3::new(2500.0, -500.0, 0.0));
    m.root.guides.insert(Guide::line(DVec3::new(0.0, -250.0, 0.0), DVec3::X));
    m.root.guides.insert(Guide::point(DVec3::new(1000.0, 0.0, 500.0)));
    m.root.guides.insert(Guide::segment(DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 0.0, 300.0)));
    m.layers.push(esq_core::model::Layer::new("Pieds"));
    let mut sec = esq_core::model::SectionPlane::new(DVec3::new(0.0, 300.0, 0.0), DVec3::Y);
    (sec.name, sec.symbol) = ("Coupe longitudinale".into(), "A".into());
    let sec = m.root.sections.insert(sec);
    m.root.active_section = Some(sec);
    let mut other = esq_core::model::SectionPlane::new(DVec3::new(0.0, 0.0, 200.0), -DVec3::Z);
    other.symbol = "B".into();
    m.root.sections.insert(other);
    m.shadow.display = true;
    m.shadow.set_local(2026, 6, 21, 9.0 * 60.0 + 15.0);
    (m.shadow.light, m.shadow.dark, m.shadow.on_ground) = (70, 30, false);
    (m.shadow.latitude, m.shadow.longitude, m.shadow.tz_offset) = (48.8566, 2.3522, 1.0);
    m.shadow.city = "Paris".into();
    use esq_core::model::{AnnotKind, Annotation};
    m.root.annots.insert(Annotation {
        kind: AnnotKind::Linear {
            a: DVec3::new(0.0, 0.0, 0.0),
            b: DVec3::new(1000.0, 0.0, 0.0),
            offset: DVec3::new(0.0, -300.0, 0.0),
        },
        text: String::new(),
        layer: 1,
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
    let cam = |eye: DVec3| esq_core::model::SceneCamera {
        eye,
        target: DVec3::new(1000.0, 300.0, 200.0),
        up: DVec3::Z,
        fov_deg: 35.0,
        perspective: true,
    };
    m.scenes.push(esq_core::model::Scene {
        name: "Face".into(),
        description: "Vue de face".into(),
        camera: cam(DVec3::new(1000.0, -6000.0, 1500.0)),
        hidden_layers: Some(vec![1]),
        photo: None,
        ext: Default::default(),
    });
    m.scenes.push(esq_core::model::Scene {
        name: "Dessus".into(),
        description: String::new(),
        camera: cam(DVec3::new(1000.0, 300.0, 8000.0)),
        hidden_layers: None,
        photo: None,
        ext: Default::default(),
    });
    m
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

/// Written natively, read back by Esquisse and (when installed) by SketchUp's SDK.
#[test]
fn export_then_import_keeps_geometry() {
    let m = sample_model();
    let path = tmp(&format!("esq-roundtrip-{}.skp", std::process::id()));
    let view = esq_skp::View {
        eye: DVec3::new(4000.0, -5000.0, 3000.0),
        target: DVec3::new(1000.0, 300.0, 200.0),
        up: DVec3::Z,
        fov_deg: 35.0,
        perspective: true,
    };
    esq_skp::export_skp_view(&m, &path, Some(&view)).unwrap();
    let native = esq_skp::import_skp_view(&path).unwrap();
    let sdk = esq_skp::available().then(|| esq_skp::sdk_import_skp_view(&path).unwrap());
    let _ = std::fs::remove_file(&path);
    for (back, v) in std::iter::once(native).chain(sdk) {
        check(&m, &view, back, v);
    }
}

fn check(m: &Model, view: &esq_skp::View, back: Model, v: Option<esq_skp::View>) {
    let v = v.expect("camera saved");
    assert!(v.eye.distance(view.eye) < 1e-6 && v.target.distance(view.target) < 1e-6, "{v:?}");
    assert_eq!(back.root.instances.len(), 2);
    assert_eq!(back.defs.len(), 1, "both instances share one component definition");
    let def = back.defs.values().next().unwrap();
    assert_eq!(def.name, "Caisson");
    assert_eq!(def.entities.faces.len(), 6);
    {
        use esq_core::model::AttrValue;
        assert_eq!(def.attributes["dynamic_attributes"]["hauteur"], AttrValue::Number(400.5));
        assert_eq!(def.attributes["dynamic_attributes"]["visible"], AttrValue::Bool(true));
        for i in back.root.instances.values() {
            assert_eq!(i.attributes["fiche"]["essence"], AttrValue::Text("Chêne".into()));
        }
    }
    assert_eq!(def.entities.edges.len(), 12);
    assert_eq!(back.root.edges.len(), 1);
    let xs: Vec<f64> = back.root.instances.values().map(|i| i.xform.w_axis.x).collect();
    assert!(xs.iter().any(|x| (x - 1500.0).abs() < 1e-6), "{xs:?}");
    let wood = back.find_material("Bois").expect("material kept");
    assert_eq!(back.materials[wood].color, [180, 120, 60]);
    let top = def.entities.faces.values().find(|f| f.normal.z > 0.99).unwrap();
    assert_eq!(top.front, Some(wood));
    let bottom = def.entities.faces.values().find(|f| f.normal.z < -0.99).unwrap();
    assert!(bottom.normal.z < 0.0, "orientation kept");
    assert_eq!(back.units, esq_core::model::Units::Millimeters);
    let zmax = def.entities.verts.values().map(|v| v.pos.z).fold(0.0, f64::max);
    assert!((zmax - 400.0).abs() < 1e-6, "{zmax}");
    let guides: Vec<&Guide> = back.root.guides.values().collect();
    assert_eq!(guides.len(), 3, "{guides:?}");
    let line = guides.iter().find(|g| g.infinite).expect("infinite guide");
    assert!(line.dir.cross(DVec3::X).length() < 1e-9 && (line.pos.y + 250.0).abs() < 1e-6, "{line:?}");
    let pt = guides.iter().find(|g| g.is_point()).expect("guide point");
    assert!(pt.pos.distance(DVec3::new(1000.0, 0.0, 500.0)) < 1e-6);
    let seg = guides.iter().find(|g| !g.infinite && !g.is_point()).expect("finite guide");
    assert!((seg.dir.length() - 300.0).abs() < 1e-6, "{seg:?}");
    assert_eq!(back.scenes.len(), 2);
    let face = &back.scenes[0];
    assert_eq!((face.name.as_str(), face.description.as_str()), ("Face", "Vue de face"));
    assert!(face.camera.eye.distance(DVec3::new(1000.0, -6000.0, 1500.0)) < 1e-6, "{:?}", face.camera);
    let hidden: Vec<&str> =
        face.hidden_layers.as_ref().expect("tags saved").iter().map(|l| back.layers[*l].name.as_str()).collect();
    assert_eq!(hidden, ["Pieds"]);
    assert_eq!(back.scenes[1].name, "Dessus");
    use esq_core::model::AnnotKind;
    let ann: Vec<_> = back.root.annots.values().collect();
    assert_eq!(ann.len(), 3, "{ann:?}");
    let dim = ann.iter().find(|a| matches!(a.kind, AnnotKind::Linear { .. })).expect("dimension");
    let AnnotKind::Linear { a, b, offset } = dim.kind else { unreachable!() };
    assert!(a.distance(DVec3::ZERO) < 1e-6 && b.distance(DVec3::new(1000.0, 0.0, 0.0)) < 1e-6, "{dim:?}");
    assert!(offset.distance(DVec3::new(0.0, -300.0, 0.0)) < 1e-6, "offset {offset:?}");
    assert_eq!(back.layers[dim.layer].name, "Pieds");
    let txt = ann.iter().find(|a| matches!(a.kind, AnnotKind::Text { .. })).expect("text");
    assert_eq!(txt.text, "Plateau chêne");
    let AnnotKind::Text { point, leader } = txt.kind else { unreachable!() };
    assert!(
        point.distance(DVec3::new(500.0, 300.0, 400.0)) < 1e-6 && leader.distance(DVec3::new(200.0, 0.0, 300.0)) < 1e-6
    );
    let scr = ann.iter().find(|a| matches!(a.kind, AnnotKind::ScreenText { .. })).expect("screen text");
    assert_eq!(scr.text, "Cuisine");
    assert_eq!(back.root.sections.len(), 2);
    let active = &back.root.sections[back.root.active_section.expect("active section kept")];
    assert_eq!((active.name.as_str(), active.symbol.as_str()), ("Coupe longitudinale", "A"));
    assert!(active.normal.distance(DVec3::Y) < 1e-9 && (active.pos.y - 300.0).abs() < 1e-6, "{active:?}");
    assert_eq!(back.shadow, m.shadow);
    let b = back.root.sections.values().find(|s| s.symbol == "B").unwrap();
    assert!(b.normal.distance(-DVec3::Z) < 1e-9 && (b.pos.z - 200.0).abs() < 1e-6, "{b:?}");
}

#[test]
fn opens_a_template_shipped_with_sketchup() {
    let Some(dll) = esq_skp::find_sketchup_api() else { return };
    let templates = dll.parent().unwrap().join("resources");
    let mut found = None;
    for lang in std::fs::read_dir(&templates).unwrap().flatten() {
        let dir = lang.path().join("Templates");
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for f in rd.flatten() {
                if f.path().extension().is_some_and(|x| x == "skp") {
                    found = Some(f.path());
                }
            }
        }
    }
    let path = found.expect("a template");
    let m = esq_skp::import_skp(&path).unwrap();
    assert!(!m.layers.is_empty());
}
