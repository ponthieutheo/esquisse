//! Saving for older SketchUp versions (`Model#save(path, version)`), natively: each file has
//! the header of its version and reads back, by Esquisse and (when installed) by SketchUp's SDK.

use esq_core::glam::DVec3;
use esq_core::model::Model;
use esq_skp::SkpVersion;

#[test]
fn every_version_is_written_and_read_back() {
    let mut m = Model::new();
    m.root.add_polyline(
        &[DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, 600.0, 0.0), DVec3::new(0.0, 600.0, 0.0)],
        true,
        false,
    );
    let f = m.root.faces.keys().next().unwrap();
    m.root.push_pull(f, -400.0, false);
    let sdk = esq_skp::available();
    for v in SkpVersion::ALL {
        let p = std::env::temp_dir().join(format!("esq-version-{}-{}.skp", v.name(), std::process::id()));
        esq_skp::export_skp_version(&m, &p, None, Some(v)).unwrap_or_else(|e| panic!("{v:?}: {e:#}"));
        let (major, _, _) = esq_skp::file_version(&p).expect("header");
        if v == SkpVersion::Su2021 {
            assert_eq!(major, 26, "{v:?}: written as SketchUp 2026 writes it");
        } else {
            assert_eq!(major, v.major(), "{v:?}");
        }
        let back = esq_skp::import_skp(&p).unwrap();
        assert_eq!(back.root.faces.len(), 6, "{v:?}");
        if sdk {
            let (theirs, _) = esq_skp::sdk_import_skp_view(&p).unwrap_or_else(|e| panic!("{v:?}: {e:#}"));
            assert_eq!(theirs.root.faces.len(), 6, "{v:?} (SDK)");
        }
        let _ = std::fs::remove_file(&p);
    }
    assert_eq!(SkpVersion::Su8.name(), "8");
    assert_eq!(SkpVersion::Su2017.name(), "2017");
    // `Sketchup::Model::VERSION_3` = 3 … `VERSION_2021` = 17.
    assert_eq!(SkpVersion::from_ruby(3), Some(SkpVersion::Su3));
    assert_eq!(SkpVersion::from_ruby(9), Some(SkpVersion::Su2013));
    assert_eq!(SkpVersion::from_ruby(17), Some(SkpVersion::Su2021));
    assert_eq!((SkpVersion::from_ruby(2), SkpVersion::from_ruby(18)), (None, None));
}
