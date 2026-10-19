//! What a .skp of SketchUp's holds beyond the geometry, once opened in Esquisse: its styles in
//! Window › Styles « In Model », the style each scene switches to, the watermarks; and a
//! model made in Esquisse keeps them through a .skp of every version.

use std::path::{Path, PathBuf};

use esq_core::model::{Scene, SceneCamera};
use serde_json::json;

use crate::app::EsquisseApp;
use crate::styles::model_styles;

fn reel(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("esq-skp").join("tests").join("reel").join(name)
}

fn camera() -> SceneCamera {
    SceneCamera {
        eye: esq_core::glam::DVec3::new(-1000.0, -1000.0, 1000.0),
        target: esq_core::glam::DVec3::ZERO,
        up: esq_core::glam::DVec3::Z,
        fov_deg: 35.0,
        perspective: true,
    }
}

#[test]
fn the_styles_of_a_skp_are_in_the_model_and_scenes_switch_them() {
    let mut app = EsquisseApp::bare(None, None);
    // SketchUp 8, two styles, the second selected (and changed since).
    app.open_path(&reel("stool-depth-8.skp"));
    let (list, sel) = model_styles(&mut app.doc.model, &app.style);
    let names: Vec<&str> = list.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["[Shaded with textures]", "[Shaded with textures]1"]);
    assert_eq!(list.iter().find(|s| s.id == sel).unwrap().name, "[Shaded with textures]1");
    // Shown translated in the panel (SketchUp's own styles are named by a key).
    assert_eq!(crate::style_file::localized("[Shaded with textures]"), crate::i18n::fr_or_same("Shaded with textures"));
    // A scene that keeps its style and fog selects its style when it is shown.
    let mut scene = Scene {
        name: "Avec style".into(),
        description: String::new(),
        camera: camera(),
        hidden_layers: None,
        photo: None,
        ext: Default::default(),
    };
    crate::dialogs::scene_props::capture(&mut app, &mut scene, crate::dialogs::scene_props::RENDERING);
    assert_eq!(scene.ext.props["style"], json!("[Shaded with textures]1"));
    scene.ext.props.insert("style".into(), json!("[Shaded with textures]"));
    crate::dialogs::scene_props::apply(&mut app, &scene);
    let (list, sel) = model_styles(&mut app.doc.model, &app.style);
    assert_eq!(list.iter().find(|s| s.id == sel).unwrap().name, "[Shaded with textures]");
    // Saved and opened again: the same styles, the scene's style.
    app.doc.model.scenes.push(scene);
    app.style_to_model();
    for v in [None, Some(esq_skp::SkpVersion::Su2017), Some(esq_skp::SkpVersion::Su6)] {
        let bytes = esq_skp::native::write::write(&app.doc.model, None, v, &Default::default()).unwrap();
        let (mut m, _) = esq_skp::native::read_bytes(&bytes).unwrap();
        let (back, bsel) = model_styles(&mut m, &app.style);
        let n: Vec<&str> = back.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(n, ["[Shaded with textures]", "[Shaded with textures]1"], "{v:?}");
        assert_eq!(back.iter().find(|s| s.id == bsel).unwrap().name, "[Shaded with textures]", "{v:?}");
        assert_eq!(m.scenes[0].ext.props["style"], json!("[Shaded with textures]"), "{v:?}");
        assert_eq!(back[0].ro, list[0].ro, "{v:?}");
    }
}

#[test]
fn watermarks_made_in_esquisse_go_through_a_skp_of_every_version() {
    let mut app = EsquisseApp::bare(None, None);
    let png = crate::photo_img::tests::png_bytes(4, 4);
    let mut w = crate::watermarks::new_watermark("Papier", png);
    w.blend = 0.25;
    w.layout = crate::style::WmLayout::Tiled;
    w.scale = 0.4;
    let mut over = w.clone();
    over.name = "Cadre".into();
    over.background = false;
    over.layout = crate::style::WmLayout::Positioned;
    over.position = 8;
    app.style.watermark_list = vec![w, over];
    app.style_to_model();
    for v in esq_skp::SkpVersion::ALL.into_iter().filter(|v| v.major() >= 6) {
        let bytes = esq_skp::native::write::write(&app.doc.model, None, Some(v), &Default::default()).unwrap();
        let (m, _) = esq_skp::native::read_bytes(&bytes).unwrap();
        let mut back = EsquisseApp::bare(None, None);
        back.doc = crate::doc::Document::new(m);
        back.style_from_model();
        let strip = |l: &[crate::style::Watermark]| -> Vec<crate::style::Watermark> {
            // An MFC archive keeps « keep the proportions » for a stretched watermark only (where
            // it means something; `CWatermark`), as SketchUp does.
            l.iter()
                .cloned()
                .map(|w| crate::style::Watermark {
                    skp: Default::default(),
                    keep_aspect: w.keep_aspect && w.layout == crate::style::WmLayout::Stretched,
                    ..w
                })
                .collect()
        };
        assert_eq!(strip(&back.style.watermark_list), strip(&app.style.watermark_list), "{v:?}");
    }
}

#[test]
fn a_classification_imported_in_esquisse_goes_through_a_skp_of_every_version() {
    let mut app = EsquisseApp::bare(None, None);
    let dir = esq_core::content::folder("Classifications").expect("les classements d'Esquisse");
    let skc = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "skc"))
        .expect("un .skc");
    let name = crate::dialogs::mi_classif::import(&mut app, &skc).unwrap();
    let types = crate::dialogs::mi_classif::schemas(&app.doc.model)[0].types.clone();
    assert!(!types.is_empty());
    assert!(esq_core::classify::file_of(&app.doc.model, &name).is_some(), "le fichier part avec le modèle");
    for v in esq_skp::SkpVersion::ALL.into_iter().filter(|v| v.major() >= 14) {
        let bytes = esq_skp::native::write::write(&app.doc.model, None, Some(v), &Default::default()).unwrap();
        let (m, _) = esq_skp::native::read_bytes(&bytes).unwrap();
        let back = crate::dialogs::mi_classif::schemas(&m);
        assert_eq!(back.len(), 1, "{v:?}");
        assert_eq!((back[0].name.as_str(), &back[0].types), (name.as_str(), &types), "{v:?}");
        assert!(esq_core::classify::file_of(&m, &name).is_some(), "{v:?}");
        // What SketchUp reads of it.
        if esq_skp::available() {
            let p = std::env::temp_dir().join(format!("esq-classif-{}-{}.skp", std::process::id(), v.name()));
            std::fs::write(&p, &bytes).unwrap();
            let census = esq_skp::sdk_census(&p).expect("le SDK ouvre le fichier");
            let _ = std::fs::remove_file(&p);
            assert_eq!(census["classifications"], [format!("schéma {name:?}")], "{v:?}");
        }
    }
}
