//! `Model#export` / `Model#import` from Ruby, for every format of esq-formats, with the option
//! hashes read as SketchUp.exe reads them (`ruby_host/formats.rs`).

use std::path::{Path, PathBuf};

use esq_core::glam::DVec3;
use esq_core::model::Model;
use esq_formats::ExportFormat;
use esq_formats::options::{StlFormat, StlUnit};

use super::ruby_model::{host, run};
use crate::doc::Document;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("esq_ruby_formats_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn rb(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// A 1 m box with a textured top, and a 0.5 m square away from it, in the root.
fn doc() -> Document {
    let mut m = Model::new();
    let brick = m.add_material("Brique", [180, 90, 40]);
    let s = 1000.0;
    let p = [DVec3::ZERO, DVec3::new(s, 0.0, 0.0), DVec3::new(s, s, 0.0), DVec3::new(0.0, s, 0.0)];
    let e = &mut m.root;
    let v: Vec<_> = p.iter().map(|x| e.add_vertex_raw(*x)).collect();
    let top: Vec<_> = p.iter().map(|x| e.add_vertex_raw(*x + DVec3::Z * s)).collect();
    e.add_face_raw(vec![v[0], v[3], v[2], v[1]], vec![]);
    let f = e.add_face_raw(top.clone(), vec![]).unwrap();
    e.faces[f].front = Some(brick);
    for i in 0..4 {
        let j = (i + 1) % 4;
        e.add_face_raw(vec![v[i], v[j], top[j], top[i]], vec![]);
    }
    let q: Vec<_> = [(3000.0, 0.0), (3500.0, 0.0), (3500.0, 500.0), (3000.0, 500.0)]
        .iter()
        .map(|(x, y)| e.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
        .collect();
    e.add_face_raw(q, vec![]);
    Document::new(m)
}

fn read(p: &Path) -> String {
    String::from_utf8_lossy(&std::fs::read(p).unwrap_or_default()).into_owned()
}

#[test]
fn export_every_3d_format_with_its_keys() {
    let d = dir("keys");
    let mut h = host();
    let mut doc = doc();
    let Some(o) = run(
        &mut h,
        &mut doc,
        &format!(
            "m = Sketchup.active_model
             d = '{d}'
             [m.export(\"#{{d}}/a.obj\", triangulated_faces: true, edges: true, swap_yz: false, units: 'm'),
              m.export(\"#{{d}}/a.stl\", units: 'mm', format: 'ascii'),
              m.export(\"#{{d}}/a.3ds\", geometry: 'single_object', units: 'cm'),
              m.export(\"#{{d}}/a.dae\", triangulated_faces: false, edges: true),
              m.export(\"#{{d}}/a.kmz\", hidden_geometry: true),
              m.export(\"#{{d}}/a.fbx\", units: 'cm'),
              m.export(\"#{{d}}/a.xsi\", triangulated_faces: true),
              m.export(\"#{{d}}/a.wrl\", edges: false),
              m.export(\"#{{d}}/a.dxf\", acad_version: 'acad_12', faces_flag: false),
              m.export(\"#{{d}}/a.ifc\", ifc_version: 'IFC 2x3'),
              m.export(\"#{{d}}/a.glb\"), m.export(\"#{{d}}/a.usdz\"),
              m.export(\"#{{d}}/a.dwg\"), m.export(\"#{{d}}/b.dxf\", type: '2d')]",
            d = rb(&d)
        ),
    ) else {
        return;
    };
    // DXF 2D draws the view: without one (no window), false, as SketchUp answers a failed export.
    assert_eq!(o.result, "[true, true, true, true, true, true, true, true, true, true, true, true, true, false]");
    assert!(std::fs::read(d.join("a.dwg")).unwrap_or_default().starts_with(b"AC1015"));
    let obj = read(&d.join("a.obj"));
    assert!(obj.contains("# File units = meters") && obj.contains("\nl "), "{obj}");
    // Triangulated: 6 squares → 12 triangles, + 2 for the separate square; Z stays up.
    assert_eq!(obj.lines().filter(|l| l.starts_with("f ")).count(), 14);
    assert!(obj.contains("v 1 1 1\n"), "le coin (1 m, 1 m, 1 m) sans permutation YZ");
    let stl = read(&d.join("a.stl"));
    assert!(stl.starts_with("solid") && stl.contains("1000"), "STL ASCII en millimètres");
    let dae = read(&d.join("a.dae"));
    assert!(dae.contains("<polylist") && dae.contains("<lines"), "faces non triangulées, arêtes");
    let dxf = read(&d.join("a.dxf"));
    assert!(dxf.contains("AC1009") && !dxf.contains("3DFACE"), "R12, sans faces");
    assert!(read(&d.join("a.ifc")).contains("FILE_SCHEMA(('IFC2X3'))"));
    for f in ["a.3ds", "a.kmz", "a.fbx", "a.xsi", "a.wrl", "a.glb", "a.usdz"] {
        assert!(std::fs::metadata(d.join(f)).map(|m| m.len()).unwrap_or(0) > 100, "{f}");
    }
    // The script's hash is laid over a copy: the saved options did not change…
    assert_eq!(h.app.formats.obj, Default::default());
    // …except the STL exporter's, which SketchUp keeps (its "STL Exporter" preferences).
    assert_eq!((h.app.formats.stl.units, h.app.formats.stl.format), (StlUnit::Millimeters, StlFormat::Ascii));
    // Exporting writes nothing in the model.
    assert!(doc.undo_label().is_none());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn saved_options_symbol_keys_and_errors() {
    let d = dir("saved");
    let mut h = host();
    // Last used: triangulated, millimetres.
    h.app.formats.obj.triangulate = true;
    h.app.formats.obj.units = esq_formats::units::LenUnit::Millimeters;
    let mut doc = doc();
    let Some(o) = run(
        &mut h,
        &mut doc,
        &format!(
            "m = Sketchup.active_model
             d = '{d}'
             [m.export(\"#{{d}}/saved.obj\"),
              m.export(\"#{{d}}/strings.obj\", {{ 'triangulated_faces' => false, 'units' => 'm' }}),
              m.export(\"#{{d}}/types.obj\", triangulated_faces: 0, units: 7.0, couleur: 'rouge', edges: nil),
              (m.export(\"#{{d}}/a.xyz\") rescue $!.message), (m.export(\"#{{d}}/sans_extension\") rescue $!.message),
              (m.export rescue $!.message), (m.export(3) rescue $!.class),
              m.export(\"#{{d}}/report.obj\", show_summary: true), m.export(\"#{{d}}/quiet.obj\", show_summary: 1),
              m.export(\"#{{d}}/flag.obj\", true)]",
            d = rb(&d)
        ),
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[true, true, true, \"Unsupported file extension\", \"Unsupported file extension\", \
         \"Not enough arguments given\", TypeError, true, true, true]"
    );
    let tris = |f: &str| read(&d.join(f)).lines().filter(|l| l.starts_with("f ")).count();
    // No hash: the saved options (triangles, millimetres).
    assert_eq!(tris("saved.obj"), 14);
    assert!(read(&d.join("saved.obj")).contains("File units = millimeters"));
    // String keys are never read; nor are values of the wrong type or unknown keys.
    for f in ["strings.obj", "types.obj"] {
        assert_eq!(tris(f), 14, "{f}");
        assert!(read(&d.join(f)).contains("File units = millimeters"), "{f}");
    }
    // The results window: only when :show_summary is true, or for a true second argument.
    assert_eq!(h.app.results.len(), 2);
    assert!(h.app.results.iter().all(|(t, r)| t == "Résultats de l’exportation" && !r.is_empty()));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn selection_only_when_the_script_asks() {
    let d = dir("sel");
    let mut h = host();
    // Saved "Exporter uniquement la sélection" is not what a script gets (SketchUp decides
    // from the hash alone).
    h.app.formats.stl.selection_only = true;
    h.app.formats.stl.format = StlFormat::Ascii;
    let mut doc = doc();
    let Some(o) = run(
        &mut h,
        &mut doc,
        &format!(
            "m = Sketchup.active_model
             d = '{d}'
             small = m.entities.grep(Sketchup::Face).find {{ |f| f.bounds.min.x > 100 }}
             m.selection.add(small)
             [m.export(\"#{{d}}/all.stl\"), m.export(\"#{{d}}/sel.stl\", selectionset_only: true),
              m.export(\"#{{d}}/sel.dae\", selectionset_only: true), m.selection.size]",
            d = rb(&d)
        ),
    ) else {
        return;
    };
    assert_eq!(o.result, "[true, true, true, 1]");
    let facets = |f: &str| read(&d.join(f)).matches("facet normal").count();
    assert_eq!(facets("all.stl"), 14);
    assert_eq!(facets("sel.stl"), 2);
    // Every format, even one whose dialog has no such option.
    assert!(!read(&d.join("sel.dae")).contains("Brique"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn import_places_like_file_import() {
    let d = dir("import");
    // Files to import, written by the exporters.
    let src = doc();
    let o = esq_formats::AllOptions::default();
    for fmt in [ExportFormat::Dae, ExportFormat::Stl, ExportFormat::ThreeDs] {
        crate::formats3d::export_doc(&src, None, &o, &d.join(format!("boite.{}", fmt.extension())), fmt).unwrap();
    }
    std::fs::write(d.join("vide.yyy"), "x").unwrap();
    let mut h = host();
    let mut doc = Document::new(Model::new());
    let Some(r) = run(
        &mut h,
        &mut doc,
        &format!(
            "m = Sketchup.active_model
             d = '{d}'
             [m.import(\"#{{d}}/boite.dae\"), m.definitions.size, m.entities.grep(Sketchup::ComponentInstance).size]",
            d = rb(&d)
        ),
    ) else {
        return;
    };
    // COLLADA: a component held by the cursor (no instance yet).
    assert_eq!(r.result, "[true, 1, 0]");
    let def = h.st.place.expect("composant à placer");
    assert_eq!(doc.model.defs[def].name, "boite");
    assert_eq!(doc.undo_label(), Some("Script Ruby"));

    // STL: "Préserver l'origine du dessin" by default (an instance at the origin), unless the
    // hash says otherwise; units from the hash; a 3DS with its results when asked.
    let mut h = host();
    let mut doc = Document::new(Model::new());
    let Some(r) = run(
        &mut h,
        &mut doc,
        &format!(
            "m = Sketchup.active_model
             d = '{d}'
             r = [m.import(\"#{{d}}/boite.stl\"), m.entities.grep(Sketchup::ComponentInstance).size]
             r << m.entities.grep(Sketchup::ComponentInstance)[0].bounds.max.x.to_mm.round
             r += [m.import(\"#{{d}}/boite.stl\", preserve_origin: false, units: 'cm', merge_coplanar_faces: true),
                   m.entities.grep(Sketchup::ComponentInstance).size,
                   m.import(\"#{{d}}/boite.3ds\", show_summary: true), m.import(\"#{{d}}/rien.dae\"),
                   m.import(\"#{{d}}/vide.yyy\"), (m.import rescue $!.message), (m.import(nil) rescue $!.class)]",
            d = rb(&d)
        ),
    ) else {
        return;
    };
    assert_eq!(r.result, "[true, 1, 3500, true, 1, true, false, false, \"Not enough arguments given\", TypeError]");
    // The second STL, read in centimetres: ten times larger, coplanar faces merged.
    let stl = doc.model.defs.values().find(|x| x.name == "boite#1").expect("deuxième STL");
    let maxx = stl.entities.verts.values().map(|v| v.pos.x).fold(f64::MIN, f64::max);
    let minx = stl.entities.verts.values().map(|v| v.pos.x).fold(f64::MAX, f64::min);
    assert!((maxx - minx - 35_000.0).abs() < 1e-3, "{}", maxx - minx);
    assert_eq!(stl.entities.faces.len(), 7);
    // The STL importer keeps the script's options (SketchUp's "STL Importer" preferences).
    assert!(!h.app.formats.stl_in.preserve_origin && h.app.formats.stl_in.merge_coplanar);
    // The last import held by the cursor is the 3DS, with its results window.
    assert_eq!(h.st.place.map(|p| doc.model.defs[p].name.clone()).as_deref(), Some("boite#2"));
    assert_eq!(h.app.results.len(), 1);
    assert_eq!(h.app.results[0].0, "Résultats de l'importation");
    let _ = std::fs::remove_dir_all(&d);
}

/// In the application: the component of `Model#import` follows the cursor once the script is
/// over (the Place tool, as File › Import), and `show_summary` opens the results window.
#[test]
fn import_in_the_application_starts_the_placement() {
    let d = dir("app");
    let src = doc();
    let o = esq_formats::AllOptions::default();
    crate::formats3d::export_doc(&src, None, &o, &d.join("boite.3ds"), ExportFormat::ThreeDs).unwrap();
    let mut a = super::ui_driver::app();
    let code = format!("Sketchup.active_model.import('{}', show_summary: true)", rb(&d.join("boite.3ds")));
    let Some(r) = super::dialogs::ruby(&mut a, &code) else { return };
    assert_eq!(r, "true");
    assert_eq!(a.tool.id(), crate::tools::ToolId::Component);
    assert_eq!(a.doc.model.defs.values().filter(|x| x.name == "boite").count(), 1);
    let results = a.ui_state.extra.formats.results.clone().expect("fenêtre de résultats");
    assert_eq!(results.0, "Résultats de l'importation");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn ruby_importers_and_material_names() {
    let d = dir("rbimp");
    // A file whose "Brique" is grey: the model's own "Brique" is textured.
    let mut src = Model::new();
    let grey = src.add_material("Brique", [128, 128, 128]);
    let v: Vec<_> = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0)]
        .iter()
        .map(|(x, y)| src.root.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
        .collect();
    let f = src.root.add_face_raw(v, vec![]).unwrap();
    src.root.faces[f].front = Some(grey);
    esq_core::io::save_esq(&src, &d.join("gris.esq")).unwrap();
    let mut h = host();
    let mut doc = doc();
    let Some(r) = run(
        &mut h,
        &mut doc,
        &format!(
            "m = Sketchup.active_model
             d = '{d}'
             class Lecteur < Sketchup::Importer
               attr_reader :calls
               def initialize(answer); @answer = answer; @calls = []; end
               def description; 'Lecteur (*.zzz)'; end
               def file_extension; 'zzz'; end
               def load_file(path, show); @calls << [File.basename(path), show]; @answer; end
             end
             ok = Lecteur.new(0)
             Sketchup.register_importer(ok)
             r = [m.import(\"#{{d}}/absent.zzz\"), m.import(\"#{{d}}/x.ZZZ\", true), ok.calls]
             Esq.importers.delete(ok)
             nope = Lecteur.new(nil)
             Sketchup.register_importer(nope)
             r += [m.import(\"#{{d}}/x.zzz\", show_summary: true), nope.calls]
             Esq.importers.delete(nope)
             r << m.import(\"#{{d}}/gris.esq\")
             r << m.materials.map(&:name).sort
             r",
            d = rb(&d)
        ),
    ) else {
        return;
    };
    // A Ruby importer is called even for a missing file (it answers for it), with the
    // "show summary" flag; nil is a failure.
    assert_eq!(
        r.result,
        "[true, true, [[\"absent.zzz\", false], [\"x.ZZZ\", true]], false, [[\"x.zzz\", true]], true, \
         [\"Brique\", \"Brique1\"]]"
    );
    let b = doc.model.materials.values().find(|m| m.name == "Brique1").unwrap();
    assert_eq!(b.color, [128, 128, 128]);
    let _ = std::fs::remove_dir_all(&d);
}

/// The box and the square seen from a corner, in an 800 × 500 view (aspect 1.6).
fn view_of_doc() -> (crate::camera::Camera, crate::style::ViewStyle) {
    let mut cam = crate::camera::Camera { size: esq_core::glam::DVec2::new(800.0, 500.0), ..Default::default() };
    cam.zoom_extents(DVec3::ZERO, DVec3::new(3500.0, 1000.0, 1000.0));
    (cam, crate::style::ViewStyle::default())
}

/// A number after `key` in a text file ("/MediaBox [0 0 " → the width…), the `n`-th one.
fn number_after(text: &str, key: &str, n: usize) -> f64 {
    let at = text.find(key).unwrap_or_else(|| panic!("{key} absent")) + key.len();
    text[at..].split_whitespace().nth(n).and_then(|s| s.trim_end_matches(']').parse().ok()).unwrap()
}

#[test]
fn export_2d_vectors_with_their_keys() {
    let d = dir("vec");
    let mut h = host();
    h.app.view2d = Some(view_of_doc());
    let mut doc = doc();
    let Some(o) = run(
        &mut h,
        &mut doc,
        &format!(
            "m = Sketchup.active_model
             d = '{d}'
             [m.export(\"#{{d}}/a.pdf\"),
              m.export(\"#{{d}}/b.pdf\", window_height: 2.0, match_screen_profiles: false, line_width: 0.02,
                       line_width_section: 1, full_scale: 'oui'),
              m.export(\"#{{d}}/a.eps\"), m.export(\"#{{d}}/b.eps\", window_height: 2.0),
              m.export(\"#{{d}}/a.dxf\", type: '2d'),
              m.export(\"#{{d}}/b.dxf\", type: '2d', profile_type: 'polyline_with_width', width: 10.0),
              m.export(\"#{{d}}/a.dwg\", type: '2d'), m.export(\"#{{d}}/b.dwg\", type: '2d', profile_type: 1),
              m.export(\"#{{d}}/s.dxf\", type: 'section')]",
            d = rb(&d)
        ),
    ) else {
        return;
    };
    // The section slice is not written from Ruby.
    assert_eq!(o.result, "[true, true, true, true, true, true, true, true, false]");
    // PDF: `:window_height` (inches) sets the page: 2 inches high = 144 points, the width
    // following the view (1.6 × 144).
    let (a, b) = (read(&d.join("a.pdf")), read(&d.join("b.pdf")));
    assert!((number_after(&b, "/MediaBox [0 0", 1) - 144.0).abs() < 0.01, "{}", &b[..400.min(b.len())]);
    assert!((number_after(&b, "/MediaBox [0 0", 0) - 230.4).abs() < 0.01);
    assert!((number_after(&a, "/MediaBox [0 0", 1) - 144.0).abs() > 1.0);
    // SketchUp keeps them as the PDF exporter's settings; an Integer for a length or a String
    // for a boolean changes nothing.
    let saved = &h.app.prefs2d["pdf"];
    assert_eq!(saved["matchScreenProfiles"], false);
    assert!((saved["profileWidth"].as_f64().unwrap() - 0.508).abs() < 1e-9);
    assert_eq!(saved["sectionWidth"].as_f64(), Some(0.7));
    // EPS: SketchUp reads no key for it (the generic table, that its exporter ignores).
    let bbox = |f: &str| number_after(&read(&d.join(f)), "%%BoundingBox: 0 0", 1);
    assert_eq!(bbox("a.eps"), bbox("b.eps"));
    // AutoCAD 2D: profiles as wide polylines, and a drawing 10 inches wide (in millimetres,
    // the model's unit).
    let (a, b) = (read(&d.join("a.dxf")), read(&d.join("b.dxf")));
    // (AutoCAD 2000 by default: lightweight polylines.)
    assert!(a.contains("AC1015") && !a.contains("\r\nLWPOLYLINE\r\n") && b.contains("\r\nLWPOLYLINE\r\n"));
    let max_x = |t: &str| {
        // Pairs of lines (group code, value) of the ENTITIES section.
        let ents = &t[t.find("ENTITIES\r\n").unwrap() + 10..];
        let mut it = ents[..ents.find("ENDSEC\r\n").unwrap()].split("\r\n").map(str::trim);
        let mut m = f64::MIN;
        while let Some(code) = it.next() {
            if let ("10", Some(v)) = (code, it.next()) {
                m = m.max(v.parse().unwrap_or(f64::MIN));
            }
        }
        m
    };
    // The view's default for a perspective: one point per pixel, 800 pt = 282.2 mm wide.
    let ratio = max_x(&b) / max_x(&a);
    assert!((ratio - 254.0 / (800.0 * 25.4 / 72.0)).abs() < 1e-3, "{ratio}");
    let polylines = |f: &str| {
        let doc = cadkit_dwg::read(&std::fs::read(d.join(f)).unwrap(), &cadkit_core::ReadOptions::default()).unwrap();
        doc.models[0].entities.iter().filter(|e| matches!(e.kind, cadkit_core::EntityKind::Polyline { .. })).count()
    };
    assert_eq!(polylines("a.dwg"), 0);
    assert!(polylines("b.dwg") > 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn export_images_with_their_keys() {
    use crate::export2d::Kind;
    let mut h = host();
    h.app.view2d = Some(view_of_doc());
    let mut doc = doc();
    let Some(o) = run(
        &mut h,
        &mut doc,
        "m = Sketchup.active_model
         [m.export('C:/x/a.png', width: 320), m.export('C:/x/b.jpg', height: 100, antialias: false, compression: 0.5),
          m.export('C:/x/c.bmp'), m.export('C:/x/d.tif', width: 64.0), m.export('C:/x/e.tga', width: 50, height: 400),
          m.export('C:/x/f.jpeg', transparent: true)]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[true, true, true, true, true, true]");
    let got: Vec<_> = h.app.images.iter().map(|(_, k, o)| (*k, o.use_view_size, o.width, o.height)).collect();
    assert_eq!(
        got,
        [
            // Width given: the height follows the view (800 × 500).
            (Kind::Png, false, 320, 200),
            (Kind::Jpeg, false, 160, 100),
            // No key, or a Float: the saved options (the view's size).
            (Kind::Bmp, true, 100, 100),
            (Kind::Tiff, true, 100, 100),
            // Both: the width wins, the view's proportions are kept.
            (Kind::Tga, false, 50, 31),
            (Kind::Jpeg, true, 100, 100),
        ]
    );
    // :antialias, :compression, :transparent are not keys of Model#export.
    assert!(h.app.images.iter().all(|(_, _, o)| o.antialias && !o.transparent && o.jpeg_quality == 100));
}

/// In the application: the image is rendered at once at the asked size (not the window's).
#[test]
fn export_images_in_the_application() {
    let Some(rs) = crate::tests::gpu::render_state() else { return eprintln!("pas de GPU : test sauté") };
    let d = dir("img");
    let mut a = crate::app::EsquisseApp::bare(Some(crate::render::Viewport3d::new(&rs)), None);
    a.gpu = Some(rs);
    a.doc = doc();
    a.cam = view_of_doc().0;
    let code = format!(
        "m = Sketchup.active_model
         d = '{d}'
         [m.export(\"#{{d}}/a.png\", width: 320), m.export(\"#{{d}}/b.jpg\", height: 100), m.export(\"#{{d}}/c.bmp\", width: 64),
          m.export(\"#{{d}}/d.tif\", width: 48), m.export(\"#{{d}}/e.tga\", width: 40),
          File.exist?(\"#{{d}}/a.png\")]",
        d = rb(&d)
    );
    let Some(r) = super::dialogs::ruby(&mut a, &code) else { return };
    assert_eq!(r, "[true, true, true, true, true, true]");
    for (f, w, h) in [("a.png", 320, 200), ("b.jpg", 160, 100), ("c.bmp", 64, 40), ("d.tif", 48, 30)] {
        let img = image::open(d.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert_eq!((img.width(), img.height()), (w, h), "{f}");
    }
    let tga = image::load_from_memory_with_format(&std::fs::read(d.join("e.tga")).unwrap(), image::ImageFormat::Tga);
    assert_eq!(tga.map(|i| (i.width(), i.height())).unwrap(), (40, 25));
    // The script's model is drawn: the box, not the background, in the middle of the picture.
    let img = image::open(d.join("a.png")).unwrap().to_rgb8();
    let bg = a.style.bg_color;
    assert_ne!(img.get_pixel(5, 5).0, img.get_pixel(160, 100).0, "{bg:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// `View#write_image` wrote its picture at the next frame: SketchUp writes it before it
/// returns, at the size asked, whatever the view's proportions (TestUp test_write_image_legacy_*,
/// and the textures TC_Sketchup_Face makes with it).
#[test]
fn view_write_image_writes_at_once() {
    let Some(rs) = crate::tests::gpu::render_state() else { return eprintln!("pas de GPU : test sauté") };
    let d = dir("vue");
    let mut a = crate::app::EsquisseApp::bare(Some(crate::render::Viewport3d::new(&rs)), None);
    a.gpu = Some(rs);
    a.doc = doc();
    a.cam = view_of_doc().0;
    let code = format!(
        "v = Sketchup.active_model.active_view
         d = '{d}'
         [v.write_image(\"#{{d}}/a.png\", 300, 200), File.exist?(\"#{{d}}/a.png\"),
          v.write_image(filename: \"#{{d}}/b.jpg\", width: 64, height: 128), File.exist?(\"#{{d}}/b.jpg\"),
          v.write_image(\"#{{d}}/c.gif\"), File.exist?(\"#{{d}}/c.gif\"),
          v.write_image(filename: \"#{{d}}/t.png\", width: 40, height: 20, transparent: true, antialias: true)]",
        d = rb(&d)
    );
    let Some(r) = super::dialogs::ruby(&mut a, &code) else { return };
    assert_eq!(r, "[true, true, true, true, false, false, true]");
    for (f, w, h) in [("a.png", 300, 200), ("b.jpg", 64, 128), ("t.png", 40, 20)] {
        let img = image::open(d.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert_eq!((img.width(), img.height()), (w, h), "{f}");
    }
    // `:transparent`: the background is see-through (TestUp test_write_image_options_array).
    let t = image::open(d.join("t.png")).unwrap().to_rgba8();
    assert_eq!(t.get_pixel(0, 0).0, [0, 0, 0, 0]);
    assert_eq!(image::open(d.join("a.png")).unwrap().to_rgba8().get_pixel(0, 0).0[3], 255);
    let _ = std::fs::remove_dir_all(&d);
}

/// A .skp saved from the application keeps its preview (256 × 256, the model drawn), as
/// SketchUp does: from File › Save and from `Model#save`, whose version is the file's.
#[test]
fn a_skp_saved_in_the_application_has_its_thumbnail() {
    let Some(rs) = crate::tests::gpu::render_state() else { return eprintln!("pas de GPU : test sauté") };
    let d = dir("vignette");
    let mut a = crate::app::EsquisseApp::bare(Some(crate::render::Viewport3d::new(&rs)), None);
    a.gpu = Some(rs);
    a.doc = doc();
    a.cam = view_of_doc().0;
    let thumb = |p: &std::path::Path| -> image::RgbImage {
        let data = std::fs::read(p).unwrap();
        let f = esq_skp::native::container::open(&data).unwrap();
        let esq_skp::native::container::Body::Zip { archive, .. } = &f.body else { panic!("{p:?} : pas une archive") };
        image::load_from_memory(&archive.read_name("meta/model_thumbnail.png").unwrap()).unwrap().to_rgb8()
    };
    // Ruby: the script's model, in the current format and in SketchUp 8.
    let code = format!(
        "m = Sketchup.active_model
         [m.save('{d}/rb.skp'), m.save_copy('{d}/rb8.skp', Sketchup::Model::VERSION_8)]",
        d = rb(&d)
    );
    let Some(r) = super::dialogs::ruby(&mut a, &code) else { return };
    assert_eq!(r, "[true, true]");
    assert_eq!(esq_skp::file_version(&d.join("rb8.skp")).map(|v| v.0), Some(8));
    // File › Save.
    let p = d.join("app.skp");
    a.save_as_format(&p, crate::skpsave::SaveFormat::Skp(None)).unwrap();
    for f in [d.join("rb.skp"), p] {
        let img = thumb(&f);
        assert_eq!(img.dimensions(), (256, 256), "{f:?}");
        assert_ne!(img.get_pixel(2, 2).0, img.get_pixel(128, 128).0, "{f:?} : le modèle au milieu");
    }
    // The file opens again, natively.
    assert!(esq_skp::import_skp(&d.join("rb8.skp")).is_ok());
    let _ = std::fs::remove_dir_all(&d);
}

/// Writes `tranche.esq` in `ESQ_DEMO_DIR` for a look on screen: the box and the square with
/// an active section plane across them.
#[test]
#[ignore]
fn write_slice_demo() {
    let Ok(dir) = std::env::var("ESQ_DEMO_DIR") else { return };
    let mut h = host();
    let mut doc = doc();
    let Some(_) = run(
        &mut h,
        &mut doc,
        "sp = Sketchup.active_model.entities.add_section_plane([0, 300.mm, 0], [0, 1, 0]); sp.activate",
    ) else {
        return;
    };
    esq_core::io::save_esq(&doc.model, &std::path::Path::new(&dir).join("tranche.esq")).unwrap();
}

/// Smallest and largest x of the entities of a DXF (group codes 10 and 11).
fn dxf_x_range(t: &str) -> (f64, f64) {
    let ents = &t[t.find("ENTITIES\r\n").unwrap() + 10..];
    let mut it = ents[..ents.find("ENDSEC\r\n").unwrap()].split("\r\n").map(str::trim);
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    while let Some(code) = it.next() {
        if let ("10" | "11", Some(Ok(v))) = (code, it.next().map(str::parse::<f64>)) {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    (lo, hi)
}

/// Lines and polylines (with the width of the first) of a DWG read back by cadkit.
fn dwg_entities(p: &Path) -> (usize, usize, f64) {
    let d = cadkit_dwg::read(&std::fs::read(p).unwrap(), &cadkit_core::ReadOptions::default()).unwrap();
    let (mut lines, mut polys, mut width) = (0, 0, 0.0);
    for e in &d.models[0].entities {
        match &e.kind {
            cadkit_core::EntityKind::Line { .. } => lines += 1,
            cadkit_core::EntityKind::Polyline { vertices, .. } => {
                polys += 1;
                width = vertices[0].start_width;
            }
            _ => {}
        }
    }
    (lines, polys, width)
}

#[test]
fn export_section_slices_with_their_keys() {
    let d = dir("slice");
    let mut h = host();
    h.app.view2d = Some(view_of_doc());
    let mut doc = doc();
    let Some(o) = run(
        &mut h,
        &mut doc,
        &format!(
            "m = Sketchup.active_model
             d = '{d}'
             r = [m.export(\"#{{d}}/none.dwg\", type: 'section')]
             sp = m.entities.add_section_plane([0, 300.mm, 0], [0, 1, 0])
             sp.activate
             r + [m.export(\"#{{d}}/a.dwg\", type: 'section'),
                  m.export(\"#{{d}}/b.dwg\", type: 'section', section_line_type: 'polyline_with_width',
                           section_width: 0.02, section_automatic: false),
                  m.export(\"#{{d}}/a.dxf\", type: 'section'),
                  m.export(\"#{{d}}/c.dxf\", type: 'section', full_scale: true, in_drawing: 1.0, in_model: 10.0),
                  m.export(\"#{{d}}/w.dxf\", type: 'section', full_scale: false, width: 4.0),
                  m.export(\"#{{d}}/h.dxf\", type: 'section', full_scale: false, height: 2.0),
                  m.export(\"#{{d}}/i.dxf\", type: 'section', full_scale: false, width: 4, section_line_type: 2,
                           section_width: 1, acad_version: 'acad_2013'),
                  (m.export(\"#{{d}}/s.svg\", type: 'section') rescue $!.message)]",
            d = rb(&d)
        ),
    ) else {
        return;
    };
    // Without an active section plane, nothing is written; SVG is not one of SketchUp's.
    assert_eq!(o.result, "[false, true, true, true, true, true, true, true, \"Unsupported file extension\"]");
    // The cut of the box and of the square beside it: 5 lines at 1:1 ...
    assert_eq!(dwg_entities(&d.join("a.dwg")), (5, 0, 0.0));
    // ... polylines 0.02 inch wide when asked.
    let (lines, polys, w) = dwg_entities(&d.join("b.dwg"));
    assert_eq!((lines, polys), (0, 5));
    assert!((w - 0.508).abs() < 1e-9, "{w}");
    let width = |f: &str| {
        let (lo, hi) = dxf_x_range(&read(&d.join(f)));
        hi - lo
    };
    assert!((width("a.dxf") - 3500.0).abs() < 1e-6);
    // 1:10, then 4 inches wide, then 2 inches high (3.5 times as wide).
    assert!((width("c.dxf") - 350.0).abs() < 1e-6);
    assert!((width("w.dxf") - 101.6).abs() < 1e-6);
    assert!((width("h.dxf") - 177.8).abs() < 1e-6);
    // An Integer for a width in inches is not read: thick lines of the automatic width.
    let i = read(&d.join("i.dxf"));
    assert!((width("i.dxf") - 101.6).abs() > 1.0, "width: 4 (Integer) ignored");
    assert!(i.contains("\r\n 39\r\n"), "thick lines");
    // The saved projection: the cut as the view shows it, within the view's frame.
    h.app
        .prefs2d
        .insert("slice".into(), serde_json::json!({"exportSectionType": 1, "fullScale": false, "width": 200.0}));
    let Some(o) = run(&mut h, &mut doc, &format!("Sketchup.active_model.export('{}/p.dxf', type: 'section')", rb(&d)))
    else {
        return;
    };
    assert_eq!(o.result, "true");
    let (lo, hi) = dxf_x_range(&read(&d.join("p.dxf")));
    assert!(lo >= 0.0 && hi <= 200.0 && hi - lo > 10.0 && (hi - lo - 3500.0).abs() > 1.0, "{lo} {hi}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Layers, blocks and entities of an AutoCAD file as cadkit reads them, names as release 14
/// writes them (in capitals) to compare releases.
fn acad_summary(bytes: &[u8]) -> String {
    let d = esq_formats::cad::read(bytes).unwrap();
    let name = |n: &str| esq_core::dwg::DwgVersion::R14.table_name(n);
    let ents = |es: &[cadkit_core::Entity]| -> Vec<String> {
        es.iter()
            .map(|e| {
                let mut kind = format!("{:?}", e.kind);
                if let cadkit_core::EntityKind::Insert { block, .. } = &e.kind {
                    kind = kind.replace(&format!("{block:?}"), &format!("{:?}", name(block)));
                }
                format!("{:?} {:?} {kind}", e.layer.as_deref().map(name), e.color)
            })
            .collect()
    };
    let mut layers: Vec<_> = d.layers.iter().map(|l| (name(&l.name), l.color, l.visible)).collect();
    layers.sort_by(|a, b| a.0.cmp(&b.0));
    let mut blocks: Vec<_> = d.blocks.iter().map(|b| (name(&b.name), b.base_point, ents(&b.entities))).collect();
    blocks.sort_by(|a, b| a.0.cmp(&b.0));
    let models: Vec<_> = d.models.iter().filter(|m| !m.entities.is_empty()).map(|m| ents(&m.entities)).collect();
    format!("{layers:?}\n{blocks:?}\n{models:?}")
}

/// `Model#export` writes the AutoCAD version asked (`acad_version`: 0, Version 12, to 8,
/// AutoCAD 2018) on every path of the DWG writer — 3D model, 2D drawing, section slice —
/// in DWG and in DXF, with the same drawing in every version (release 12 DWG, which cadkit
/// does not read, is read back by the tests of `esq_core::dwg`).
#[test]
fn export_every_acad_version() {
    let d = dir("versions");
    let mut h = host();
    h.app.view2d = Some(view_of_doc());
    let mut doc = doc();
    let Some(o) = run(
        &mut h,
        &mut doc,
        &format!(
            "m = Sketchup.active_model
             d = '{d}'
             sp = m.entities.add_section_plane([0, 300.mm, 0], [0, 1, 0])
             sp.activate
             (0..8).map {{ |v|
               ['dwg', 'dxf'].map {{ |x|
                 [m.export(\"#{{d}}/m#{{v}}.#{{x}}\", acad_version: v),
                  m.export(\"#{{d}}/p#{{v}}.#{{x}}\", type: '2d', acad_version: v),
                  m.export(\"#{{d}}/s#{{v}}.#{{x}}\", type: 'section', acad_version: v)]
               }}
             }}.flatten.all?",
            d = rb(&d)
        ),
    ) else {
        return;
    };
    assert_eq!(o.result, "true");
    for path in ["m", "p", "s"] {
        for ext in ["dwg", "dxf"] {
            let summary_of = |v: usize| acad_summary(&std::fs::read(d.join(format!("{path}{v}.{ext}"))).unwrap());
            let (want, true_colour) = (summary_of(3), summary_of(4));
            for v in 0..=8 {
                // The materials of the 3D model are true colours from 2004.
                let want = if path == "m" && v >= 4 { &true_colour } else { &want };
                let version = esq_core::dwg::DwgVersion::from_code(v).unwrap();
                let file = format!("{path}{v}.{ext}");
                let bytes = std::fs::read(d.join(&file)).unwrap();
                if ext == "dwg" {
                    assert!(bytes.starts_with(version.magic().as_bytes()), "{file}");
                } else {
                    let text = String::from_utf8_lossy(&bytes);
                    assert!(text.contains(&format!("$ACADVER\r\n  1\r\n{}\r\n", version.magic())), "{file}");
                }
                if ext == "dwg" && v == 0 {
                    continue;
                }
                assert_eq!(&acad_summary(&bytes), want, "{file}");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}
