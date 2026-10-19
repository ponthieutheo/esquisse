//! Persistent ids: a SketchUp 2021+ file numbers its entities with them (`docs/skp-format.md`
//! §2.1); the native reader keeps them for `Entity#persistent_id` (`Entities::pids`). Older
//! files have none: SketchUp gives new ones when it opens them.

use esq_core::glam::DVec3;
use esq_core::model::Model;
use esq_skp::SkpVersion;
use slotmap::Key;

#[test]
fn persistent_ids_are_kept_from_2021_files() {
    let mut m = Model::new();
    m.root.add_polyline(
        &[DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, 600.0, 0.0), DVec3::new(0.0, 600.0, 0.0)],
        true,
        false,
    );
    let f = m.root.faces.keys().next().unwrap();
    m.root.push_pull(f, -400.0, false);
    let dir = std::env::temp_dir().join(format!("esq-skp-pids-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for v in SkpVersion::ALL {
        let p = dir.join(format!("pids-{}.skp", v.name()));
        esq_skp::export_skp_version(&m, &p, None, Some(v)).unwrap();
        let (back, _) = esq_skp::native::read_bytes(&std::fs::read(&p).unwrap()).unwrap();
        let e = &back.root;
        let faces: Vec<u64> =
            e.faces.keys().filter_map(|k| e.pids.get(&format!("f{}", k.data().as_ffi())).copied()).collect();
        let edges = e.edges.keys().filter(|k| e.pids.contains_key(&format!("e{}", k.data().as_ffi()))).count();
        if v.major() >= 21 {
            assert_eq!((faces.len(), edges), (e.faces.len(), e.edges.len()), "{v:?}");
            let mut distinct = faces.clone();
            distinct.sort();
            distinct.dedup();
            assert_eq!(distinct.len(), faces.len(), "{v:?} : {faces:?}");
        } else {
            assert!(e.pids.is_empty(), "{v:?}");
        }
        let _ = std::fs::remove_file(&p);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
