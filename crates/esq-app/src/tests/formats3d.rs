//! File › Import and File › Export › 3D Model through the application (formats3d.rs).

use esq_core::glam::DVec3;
use esq_core::model::{EntityRef, Model};
use esq_formats::options::{StlFormat, StlUnit};
use esq_formats::{AllOptions, ExportFormat};

use crate::commands::Cmd;
use crate::doc::Document;
use crate::formats3d::{Placement, export_doc, import_into};

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("esq_app_formats_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

/// Two separate squares (1 m and 0.5 m) in the root.
fn doc_with_squares() -> Document {
    let mut m = Model::new();
    for (x0, s) in [(0.0, 1000.0), (3000.0, 500.0)] {
        let v: Vec<_> = [(0.0, 0.0), (s, 0.0), (s, s), (0.0, s)]
            .iter()
            .map(|(x, y)| m.root.add_vertex_raw(DVec3::new(x0 + x, *y, 0.0)))
            .collect();
        m.root.add_face_raw(v, vec![]);
    }
    Document::new(m)
}

#[test]
fn export_selection_only_then_import_with_the_cursor() {
    let mut doc = doc_with_squares();
    let mut o = AllOptions::default();
    o.stl.format = StlFormat::Ascii;
    o.stl.units = StlUnit::Millimeters;
    let all = tmp("tout.stl");
    export_doc(&doc, None, &o, &all, ExportFormat::Stl).unwrap();
    let text = std::fs::read_to_string(&all).unwrap();
    assert_eq!(text.matches("facet normal").count(), 4);
    // "Exporter uniquement la sélection actuelle": only the selected square.
    let small = doc
        .model
        .root
        .faces
        .iter()
        .find(|(_, f)| f.outer.len() == 4 && doc.model.root.verts[f.outer[0]].pos.x >= 3000.0);
    let small = small.map(|x| x.0).unwrap();
    doc.sel = vec![EntityRef::Face(small)];
    o.stl.selection_only = true;
    let sel = tmp("sel.stl");
    export_doc(&doc, None, &o, &sel, ExportFormat::Stl).unwrap();
    assert_eq!(std::fs::read_to_string(&sel).unwrap().matches("facet normal").count(), 2);

    // Import: by default the STL is placed with the cursor, moved to its own origin.
    let mut target = Document::new(Model::new());
    o.stl_in.merge_coplanar = true;
    o.stl_in.preserve_origin = false;
    let (place, report) = import_into(&mut target, &o, &sel).unwrap();
    assert!(report.is_none());
    let Placement::WithCursor(def) = place else { panic!("{place:?}") };
    let d = &target.model.defs[def];
    assert_eq!(d.name, "sel");
    assert_eq!(d.entities.faces.len(), 1);
    let minx = d.entities.verts.values().map(|v| v.pos.x).fold(f64::MAX, f64::min);
    assert!(minx.abs() < 1e-6, "{minx}");
    assert!(target.model.root.instances.is_empty());
    // With "Préserver l'origine du dessin": an instance at the origin, geometry in place.
    o.stl_in.preserve_origin = true;
    let (place, _) = import_into(&mut target, &o, &sel).unwrap();
    assert_eq!(place, Placement::AtOrigin);
    assert_eq!(target.model.root.instances.len(), 1);
    let inst = target.model.root.instances.values().next().unwrap();
    let minx = target.model.defs[inst.def].entities.verts.values().map(|v| v.pos.x).fold(f64::MAX, f64::min);
    assert!((minx - 3000.0).abs() < 1e-3);
    // One undo step per import.
    assert_eq!(target.undo_label(), Some("Importer"));
}

#[test]
fn export_commands_are_searchable() {
    let paths = crate::menus::command_paths();
    for (key, f) in [("Exporter sous 3DS", ExportFormat::ThreeDs), ("Exporter sous Collada", ExportFormat::Dae)] {
        assert!(paths.iter().any(|(p, c)| p.ends_with(key) && *c == Cmd::ExportAs(f)), "{key}");
        assert_eq!(Cmd::ExportAs(f).label(), key);
    }
}

#[test]
fn every_format_with_options_has_a_dialog() {
    let mut o = AllOptions::default();
    for f in esq_formats::ImportFormat::ALL {
        assert_eq!(o.import_options(f).is_some(), f.has_options(), "{f:?}");
    }
    for f in ExportFormat::ALL {
        let has = o.export_options(f).map(|x| !x.fields().is_empty());
        assert_eq!(has.is_some(), f.has_options(), "{f:?}");
    }
}

/// A textured box (component) placed away from the origin.
fn textured_doc() -> Document {
    let mut m = Model::new();
    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 8, image::Rgb([180, 90, 40])))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let brick = m.add_material("Brique", [180, 90, 40]);
    m.materials[brick].ext.texture = esq_formats::tex::texture_from_bytes("brique.png", png, Some([400.0, 400.0]));
    let mut e = esq_core::model::Entities::default();
    let s = 1000.0;
    let p = [DVec3::ZERO, DVec3::new(s, 0.0, 0.0), DVec3::new(s, s, 0.0), DVec3::new(0.0, s, 0.0)];
    let v: Vec<_> = p.iter().map(|x| e.add_vertex_raw(*x)).collect();
    let top: Vec<_> = p.iter().map(|x| e.add_vertex_raw(*x + DVec3::Z * s)).collect();
    e.add_face_raw(vec![v[0], v[3], v[2], v[1]], vec![]);
    let f = e.add_face_raw(top.clone(), vec![]).unwrap();
    e.faces[f].front = Some(brick);
    for i in 0..4 {
        let j = (i + 1) % 4;
        e.add_face_raw(vec![v[i], v[j], top[j], top[i]], vec![]);
    }
    let d = m.defs.insert(esq_core::model::Definition {
        name: "Boîte".into(),
        is_group: false,
        description: String::new(),
        entities: e,
        attributes: Default::default(),
        ext: Default::default(),
    });
    m.root.instances.insert(esq_core::model::Instance {
        def: d,
        xform: esq_core::glam::DMat4::from_translation(DVec3::new(2000.0, 0.0, 0.0)),
        name: String::new(),
        material: None,
        layer: 0,
        hidden: false,
        locked: false,
        attributes: Default::default(),
        ext: Default::default(),
    });
    Document::new(m)
}

/// Faces of everything an import added, and whether a textured face is among them.
fn imported(doc: &Document) -> (usize, f64, bool) {
    let (mut faces, mut area, mut textured) = (0, 0.0, false);
    let mut look = |e: &esq_core::model::Entities, scale: f64| {
        for (f, face) in e.faces.iter() {
            faces += 1;
            area += esq_core::tess::face_area(e, f) * scale * scale;
            let mat = face.front.and_then(|m| doc.model.materials.get(m));
            textured |= mat.is_some_and(|m| m.ext.texture.is_some()) && face.tex.is_some();
        }
    };
    // The definitions themselves (an import held by the cursor has no instance yet).
    for d in doc.model.defs.values() {
        look(&d.entities, 1.0);
    }
    (faces, area, textured)
}

/// Writes `imports.esq` in `ESQ_DEMO_DIR` for a look on screen: a box with a checkered
/// texture exported then imported back through every format that has textures, side by
/// side, with a DEM terrain carrying its gradient texture.
#[test]
#[ignore]
fn write_import_demo() {
    let Ok(dir) = std::env::var("ESQ_DEMO_DIR") else { return };
    let dir = std::path::PathBuf::from(dir);
    let mut src = textured_doc();
    // A checkerboard instead of the plain colour, on every face.
    let mut png = Vec::new();
    let img = image::RgbImage::from_fn(64, 64, |x, y| {
        if (x / 16 + y / 16) % 2 == 0 { image::Rgb([200, 40, 30]) } else { image::Rgb([240, 230, 200]) }
    });
    image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let mat = src.model.materials.keys().next().unwrap();
    src.model.materials[mat].ext.texture =
        esq_formats::tex::texture_from_bytes("damier.png", png, Some([500.0, 500.0]));
    let def = src.model.defs.keys().next().unwrap();
    for f in src.model.defs[def].entities.faces.values_mut() {
        f.front = Some(mat);
    }
    let mut doc = Document::new(Model::new());
    let mut o = AllOptions::default();
    o.ifc_in.preserve_origin = false;
    let formats = [
        ExportFormat::Dae,
        ExportFormat::Glb,
        ExportFormat::ThreeDs,
        ExportFormat::Kmz,
        ExportFormat::Usdz,
        ExportFormat::Ifc,
    ];
    for (k, fmt) in formats.into_iter().enumerate() {
        let path = dir.join(format!("demo.{}", fmt.extension()));
        export_doc(&src, None, &o, &path, fmt).unwrap();
        // SketchUp imports neither FBX nor OBJ.
        if esq_formats::ImportFormat::from_path(&path).is_none() {
            continue;
        }
        let (place, _) = import_into(&mut doc, &o, &path).unwrap();
        let at = esq_core::glam::DMat4::from_translation(DVec3::new(k as f64 * 1500.0, 0.0, 0.0));
        match place {
            Placement::WithCursor(d) => {
                doc.model.root.instances.insert(esq_core::model::Instance {
                    def: d,
                    xform: at,
                    name: String::new(),
                    material: None,
                    layer: 0,
                    hidden: false,
                    locked: false,
                    attributes: Default::default(),
                    ext: Default::default(),
                });
            }
            Placement::AtOrigin => {
                let i = doc.model.root.instances.values_mut().last().unwrap();
                i.xform = at * i.xform;
            }
        }
    }
    // A cone-shaped hill, 33 × 33 points 100 mm apart, as DEM terrain with its gradient.
    let n = 33;
    let z = (0..n * n)
        .map(|i| {
            let (x, y) = ((i % n) as f64 - 16.0, (i / n) as f64 - 16.0);
            (40.0 - 2.5 * (x * x + y * y).sqrt()).max(0.0)
        })
        .collect();
    let grid = esq_formats::dem::Grid {
        title: "Colline".into(),
        width: n,
        height: n,
        z,
        ground_unit: 1,
        elev_unit: 1,
        res: [0.33, 0.33, 0.1],
        min: 0.0,
        max: 4.0,
    };
    let opts = esq_formats::options::DemImport { max_points: 200, gradient_texture: true };
    let (terrain, _) = esq_formats::dem::terrain(&grid, &opts).unwrap();
    esq_core::io::save_esq(&terrain, &dir.join("terrain.esq")).unwrap();
    let t = doc.model.import_as_component(&terrain, "Colline");
    doc.model.root.instances.insert(esq_core::model::Instance {
        def: t,
        xform: esq_core::glam::DMat4::from_translation(DVec3::new(0.0, 2000.0, 0.0)),
        name: String::new(),
        material: None,
        layer: 0,
        hidden: false,
        locked: false,
        attributes: Default::default(),
        ext: Default::default(),
    });
    esq_core::io::save_esq(&doc.model, &dir.join("imports.esq")).unwrap();
}

#[test]
fn every_exporter_writes_and_every_importer_reads_it_back() {
    let doc = textured_doc();
    let mut o = AllOptions::default();
    o.stl_in.merge_coplanar = true;
    for fmt in ExportFormat::ALL.into_iter().filter(|f| f.implemented()) {
        let path = tmp(&format!("aller-retour.{}", fmt.extension()));
        export_doc(&doc, None, &o, &path, fmt).unwrap_or_else(|e| panic!("{fmt:?} : {e:#}"));
        let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        assert!(len > 100, "{fmt:?} : {len} octets");
        let Some(imp) = esq_formats::ImportFormat::from_path(&path) else { continue };
        if !imp.implemented() {
            continue;
        }
        let mut target = Document::new(Model::new());
        import_into(&mut target, &o, &path).unwrap_or_else(|e| panic!("{fmt:?} → {imp:?} : {e:#}"));
        let (faces, area, textured) = imported(&target);
        assert!(faces >= 6, "{fmt:?} : {faces} faces");
        // Six square metres in whatever units the file used.
        assert!((area - 6.0e6).abs() < 1.0, "{fmt:?} : aire {area}");
        // Formats without textures: STL, AutoCAD, IFC (colours only).
        if !matches!(fmt, ExportFormat::Stl | ExportFormat::Dxf | ExportFormat::Dwg | ExportFormat::Ifc) {
            assert!(textured, "{fmt:?} : texture perdue");
        }
    }
}
