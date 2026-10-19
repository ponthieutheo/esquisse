//! Texture placements written by the native writer read back as they were placed, natively
//! and by SketchUp's own reader (SketchUpAPI.dll, the oracle: skipped without it), in every
//! version that has placements: positioned, projected along a direction that is not the
//! face's, distorted (four pairs), projected and distorted, on the back, on a face facing down.
//! The map is compared as SketchUp's UV helper gives it: at the face's points, and off the face
//! (a projected texture keeps its coordinates along the projection, another along the normal).

use std::path::PathBuf;
use std::sync::Arc;

use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::meta::{FaceTex, TextureData, UvMap};
use esq_core::model::{FaceId, MaterialId, Model};
use esq_core::photo::Blob;
use esq_core::texpos::{Frame, map_from_pairs};
use esq_skp::SkpVersion;

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 9) as u8, (y * 5) as u8, 70]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

const TILE: [f64; 2] = [600.0, 300.0];

fn quad(m: &mut Model, pts: [DVec3; 4]) -> FaceId {
    let vs = pts.iter().map(|p| m.root.vertex_at(*p)).collect();
    m.root.add_face_raw(vs, vec![]).unwrap()
}

/// What each face's side was placed with: (face centre, side, map, projection).
type Placed = Vec<(DVec3, usize, UvMap, Option<DVec3>)>;

fn place(
    m: &mut Model,
    f: FaceId,
    mat: MaterialId,
    side: usize,
    pairs: &[(DVec3, DVec2)],
    projection: Option<DVec3>,
) -> (usize, UvMap) {
    let face = &m.root.faces[f];
    let p0 = m.root.verts[face.outer[0]].pos;
    let frame = Frame::of_side(p0, face.normal, projection);
    let map = map_from_pairs(&frame, TILE, pairs).expect("placement");
    let face = &mut m.root.faces[f];
    let t = face.tex.get_or_insert_with(|| Box::new(FaceTex::default()));
    if side == 0 {
        face.front = Some(mat);
        t.front = Some(map);
    } else {
        face.back = Some(mat);
        t.back = Some(map);
    }
    t.projection[side] = projection.map(DVec3::normalize);
    (side, map)
}

fn centre(m: &Model, f: FaceId) -> DVec3 {
    let face = &m.root.faces[f];
    face.outer.iter().map(|v| m.root.verts[*v].pos).sum::<DVec3>() / face.outer.len() as f64
}

fn model() -> (Model, Placed) {
    let mut m = Model::new();
    let wood = m.add_material("Bois", [180, 120, 60]);
    m.materials[wood].ext.texture = Some(TextureData {
        filename: "bois.png".into(),
        image: Blob(Arc::new(png(24, 12))),
        px: [24, 12],
        size: TILE,
        avg: [120, 90, 60],
    });
    let p = DVec3::new;
    let mut placed = Placed::new();
    let mut add = |m: &mut Model, f: FaceId, side: usize, pairs: &[(DVec3, DVec2)], projection: Option<DVec3>| {
        let (side, map) = place(m, f, wood, side, pairs, projection);
        let c = centre(m, f);
        placed.push((c, side, map, projection.map(DVec3::normalize)));
    };
    // A wall at an angle, projected from a direction that is neither its normal nor vertical.
    let wall = quad(&mut m, [p(0.0, 1000.0, 0.0), p(1000.0, 0.0, 0.0), p(1000.0, 0.0, 800.0), p(0.0, 1000.0, 800.0)]);
    let pairs = [
        (p(0.0, 1000.0, 0.0), DVec2::new(0.0, 0.0)),
        (p(1000.0, 0.0, 0.0), DVec2::new(2.0, 0.5)),
        (p(0.0, 1000.0, 800.0), DVec2::new(-0.3, 2.0)),
    ];
    add(&mut m, wall, 0, &pairs, Some(p(0.2, -1.0, 0.4)));
    // A table top, distorted.
    let top =
        quad(&mut m, [p(2000.0, 0.0, 500.0), p(3000.0, 0.0, 500.0), p(3000.0, 800.0, 500.0), p(2000.0, 800.0, 500.0)]);
    let pairs = [
        (p(2000.0, 0.0, 500.0), DVec2::new(0.0, 0.0)),
        (p(3000.0, 0.0, 500.0), DVec2::new(1.0, 0.0)),
        (p(2900.0, 700.0, 500.0), DVec2::new(1.0, 1.0)),
        (p(2100.0, 800.0, 500.0), DVec2::new(0.0, 1.0)),
    ];
    add(&mut m, top, 0, &pairs, None);
    // A slope, projected from above and distorted.
    let slope =
        quad(&mut m, [p(4000.0, 0.0, 0.0), p(5000.0, 0.0, 0.0), p(5000.0, 1000.0, 600.0), p(4000.0, 1000.0, 600.0)]);
    let pairs = [
        (p(4000.0, 0.0, 0.0), DVec2::new(0.0, 0.0)),
        (p(5000.0, 0.0, 0.0), DVec2::new(1.5, 0.0)),
        (p(5000.0, 1000.0, 0.0), DVec2::new(1.0, 2.0)),
        (p(4000.0, 1000.0, 0.0), DVec2::new(0.0, 1.5)),
    ];
    add(&mut m, slope, 0, &pairs, Some(DVec3::Z));
    // A wall painted on its back.
    let back =
        quad(&mut m, [p(0.0, 3000.0, 0.0), p(1000.0, 3000.0, 0.0), p(1000.0, 3000.0, 700.0), p(0.0, 3000.0, 700.0)]);
    let pairs = [
        (p(0.0, 3000.0, 0.0), DVec2::new(0.25, 0.0)),
        (p(1000.0, 3000.0, 0.0), DVec2::new(1.0, 0.5)),
        (p(0.0, 3000.0, 700.0), DVec2::new(0.0, 3.0)),
    ];
    add(&mut m, back, 1, &pairs, None);
    // A floor facing down, turned and scaled by two pairs.
    let floor = quad(&mut m, [p(6000.0, 0.0, 0.0), p(6000.0, 900.0, 0.0), p(7000.0, 900.0, 0.0), p(7000.0, 0.0, 0.0)]);
    assert!(m.root.faces[floor].normal.z < 0.0);
    let pairs = [(p(6000.0, 0.0, 0.0), DVec2::new(0.0, 0.0)), (p(6500.0, 300.0, 0.0), DVec2::new(1.0, 0.0))];
    add(&mut m, floor, 0, &pairs, None);
    (m, placed)
}

/// Points where the maps are compared: the face's corners and centre, and off the face.
fn points(m: &Model, f: FaceId, projection: Option<DVec3>) -> Vec<DVec3> {
    let face = &m.root.faces[f];
    let mut out: Vec<DVec3> = face.outer.iter().map(|v| m.root.verts[*v].pos).collect();
    let c = centre(m, f);
    out.push(c);
    out.push(c + face.normal * 250.0);
    if let Some(d) = projection {
        out.push(c + d * 400.0);
        out.push(c - d * 900.0);
    }
    out
}

fn check(read: &Model, placed: &Placed, what: &str) {
    for (c, side, want, projection) in placed {
        let (f, face) = read
            .root
            .faces
            .iter()
            .find(|(k, _)| centre(read, *k).distance(*c) < 1e-6)
            .unwrap_or_else(|| panic!("{what} : face {c:?} absente"));
        let t = face.tex.as_deref().unwrap_or_else(|| panic!("{what} : face {c:?} sans placement"));
        let got = if *side == 0 { t.front } else { t.back };
        let got = got.unwrap_or_else(|| panic!("{what} : face {c:?}, côté {side} sans placement"));
        let proj = t.projection[*side];
        assert_eq!(proj.is_some(), projection.is_some(), "{what} : face {c:?} projetée ?");
        if let (Some(a), Some(b)) = (proj, projection) {
            assert!(a.distance(*b) < 1e-9, "{what} : projection {a:?} au lieu de {b:?}");
        }
        for q in points(read, f, *projection) {
            let (a, b) = (got.at(q), want.at(q));
            assert!((a - b).length() < 1e-7, "{what} : face {c:?}, côté {side}, en {q:?} : {a:?} au lieu de {b:?}");
        }
    }
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("esq-skp-uv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn placements_read_back_as_placed_in_every_version() {
    let (m, placed) = model();
    let sdk = esq_skp::available();
    if !sdk {
        eprintln!("SketchUp absent : seule la relecture native est vérifiée");
    }
    let mut sdk_reads = 0;
    // SketchUp 3 keeps no placement (`docs/skp-format.md` §8.3).
    for v in SkpVersion::ALL.into_iter().filter(|v| v.major() > 3) {
        let p = tmp(&format!("uv-{}.skp", v.name()));
        esq_skp::export_skp_version(&m, &p, None, Some(v)).unwrap_or_else(|e| panic!("{v:?} : {e:#}"));
        let bytes = std::fs::read(&p).unwrap();
        let (native, _) = esq_skp::native::read_bytes(&bytes).unwrap_or_else(|e| panic!("{v:?} : {e:#}"));
        check(&native, &placed, &format!("{} relu nativement", v.name()));
        if sdk {
            let (theirs, _) = esq_skp::sdk_import_skp_view(&p).unwrap_or_else(|e| panic!("{v:?} : SDK : {e:#}"));
            check(&theirs, &placed, &format!("{} relu par le SDK", v.name()));
            sdk_reads += 1;
        }
        let _ = std::fs::remove_file(&p);
    }
    eprintln!("fichiers relus par le SDK : {sdk_reads}");
}

/// What a placement becomes when its face is moved or turned (`SUEntitiesTransform`, SketchUp's
/// own transformation of entities) or reversed (`SUFaceReverse`, the core of `Face#reverse!`):
/// read back from the SDK, the same as Esquisse's for the same edit
/// (`instance_ops::transform_entities`, `Entities::flip_face`), on and off the face. A
/// placement moves with its face; reversed, materials and placements stay on their side and
/// the turned frame mirrors the unprojected ones.
#[test]
fn placements_follow_their_faces_as_in_sketchup() {
    if !esq_skp::available() {
        eprintln!("SketchUp absent : test ignoré");
        return;
    }
    let (m, _) = model();
    let (a, b) = (tmp("edit-a.skp"), tmp("edit-b.skp"));
    esq_skp::export_skp_version(&m, &a, None, None).unwrap();
    let (base, _) = esq_skp::native::read_bytes(&std::fs::read(&a).unwrap()).unwrap();
    let turn = DMat4::from_translation(DVec3::new(100.0, 50.0, -30.0))
        * DMat4::from_rotation_z(0.6)
        * DMat4::from_rotation_x(0.3);
    let edits = [
        ("déplacement", Some(DMat4::from_translation(DVec3::new(300.0, -200.0, 150.0)))),
        ("rotation", Some(turn)),
        ("retournement", None),
    ];
    let mut compared = 0;
    for (what, t) in edits {
        esq_skp::sdk_edit_faces(&a, &b, t.as_ref()).unwrap();
        let (theirs, _) = esq_skp::sdk_import_skp_view(&b).unwrap();
        let mut ours = base.clone();
        match t {
            Some(t) => esq_core::instance_ops::transform_entities(&mut ours.root, &t),
            None => {
                let faces: Vec<FaceId> = ours.root.faces.keys().collect();
                for f in faces {
                    ours.root.flip_face(f);
                }
            }
        }
        for (f, face) in ours.root.faces.iter() {
            let c = centre(&ours, f);
            let (_, other) = theirs
                .root
                .faces
                .iter()
                .find(|(k, _)| centre(&theirs, *k).distance(c) < 1e-6)
                .unwrap_or_else(|| panic!("{what} : face {c:?} absente"));
            assert!(face.normal.distance(other.normal) < 1e-9, "{what} : face {c:?} : normale");
            for side in 0..2 {
                let get = |t: Option<&FaceTex>, s: usize| t.and_then(|t| if s == 0 { t.front } else { t.back });
                let (mine, sk) = (get(face.tex.as_deref(), side), get(other.tex.as_deref(), side));
                let mat = |x: &esq_core::model::Face, s: usize| if s == 0 { x.front } else { x.back }.is_some();
                assert_eq!(mat(face, side), mat(other, side), "{what} : face {c:?}, côté {side} : matière");
                let projection = face.tex.as_deref().and_then(|t| t.projection[side]);
                match (mine, sk) {
                    (None, None) => {}
                    (Some(x), Some(y)) => {
                        for q in points(&ours, f, projection) {
                            let (u, v) = (x.at(q), y.at(q));
                            assert!(
                                (u - v).length() < 1e-7,
                                "{what} : face {c:?}, côté {side}, en {q:?} : {u:?} (Esquisse) au lieu de {v:?} (SketchUp)"
                            );
                        }
                        compared += 1;
                    }
                    _ => panic!(
                        "{what} : face {c:?}, côté {side} : placement {} chez Esquisse, {} chez SketchUp",
                        mine.is_some(),
                        sk.is_some()
                    ),
                }
            }
        }
    }
    assert_eq!(compared, 15, "cinq placages, trois retouches");
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
}
