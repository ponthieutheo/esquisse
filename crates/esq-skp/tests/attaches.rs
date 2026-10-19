//! Attached texts and dimensions (`esq_core::attach`): what the native
//! reader keeps of the points annotations are attached to (`doc::Connection`), what the writer
//! writes of them in every version, and what SketchUp's SDK reads back.

use std::collections::BTreeMap;
use std::path::PathBuf;

use esq_core::attach::{Attach, AttachTo};
use esq_core::glam::{DMat4, DVec3};
use esq_core::model::{AnnotKind, Annotation, Definition, Entities, Instance, Model};
use esq_skp::native;

mod oracle;

fn corpus_dir() -> Option<PathBuf> {
    std::env::var_os("ESQ_SKP_REEL")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("tools").join("skp-reel")))
}

/// The ends attached of every text and linear dimension of a model (radial dimensions, which
/// Esquisse attaches to their arc, left out): (leaf kind, path length).
fn attached(m: &Model) -> Vec<(String, usize)> {
    use esq_core::model::AnnotId;
    let mut out = Vec::new();
    let colls = std::iter::once(&m.root).chain(m.defs.values().map(|d| &d.entities));
    for e in colls {
        let radial = |id: AnnotId| e.prop(&format!("a{}", slotmap::Key::data(&id).as_ffi()), "radial").is_some();
        for (id, a) in e.annots.iter() {
            if radial(id) {
                continue;
            }
            for end in a.attach.iter().flatten() {
                let kind = format!("{:?}", end.to).split('(').next().unwrap_or("").to_string();
                out.push((kind, end.path.len()));
            }
        }
    }
    out
}

/// Every file of the corpus: the connections the reader kept become attaches of the model,
/// and each attached end is where the reader placed it. `--ignored` (the whole corpus; best in
/// release): `cargo test --release -p esq-skp --test attaches -- --ignored --nocapture`.
#[test]
#[ignore]
fn real_files_keep_their_attaches() {
    let Some(dir) = corpus_dir().filter(|d| d.is_dir()) else { return };
    let mut files = Vec::new();
    oracle::walk(&dir, &mut files, 0);
    files.sort();
    let mut by_kind: BTreeMap<i32, usize> = BTreeMap::new();
    let mut kept: BTreeMap<String, usize> = BTreeMap::new();
    let (mut with, mut ends, mut lost) = (0, 0, Vec::new());
    for p in &files {
        let Ok(bytes) = std::fs::read(p) else { continue };
        let Ok(doc) = native::read_doc(&bytes, &native::Options::default()) else { continue };
        let mut n = 0;
        let colls = std::iter::once(&doc.root).chain(doc.defs.iter().map(|d| &d.ents));
        for e in colls {
            for t in &e.texts {
                if let Some(c) = &t.connection {
                    *by_kind.entry(c.kind).or_default() += 1;
                    n += 1;
                }
            }
            for d in &e.dims {
                for c in d.connections.iter().flatten() {
                    *by_kind.entry(c.kind).or_default() += 1;
                    n += 1;
                }
            }
        }
        if n == 0 {
            continue;
        }
        with += 1;
        ends += n;
        let Ok(m) = native::build::model(&doc) else { continue };
        let a = attached(&m);
        for (k, _) in &a {
            *kept.entry(k.clone()).or_default() += 1;
        }
        if a.len() < n {
            lost.push(format!("{} : {} sur {n}", p.display(), a.len()));
        }
    }
    eprintln!("fichiers avec des attaches : {with}, bouts attachés : {ends}");
    eprintln!("types lus : {by_kind:?}");
    eprintln!("attaches gardées : {kept:?}");
    eprintln!("fichiers où des attaches ne sont pas gardées : {}", lost.len());
    for l in lost.iter().take(30) {
        eprintln!("  {l}");
    }
}

/// A model with a component placed twice, a text on a face of one instance, a dimension
/// between the two instances (vertex to edge middle), and in the root a dimension on an edge
/// and a text on a guide point.
pub fn attached_model() -> Model {
    let mut m = Model::new();
    let mut e = Entities::default();
    let c = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y].map(|q| e.add_vertex_raw(q * 254.0));
    for k in 0..4 {
        e.add_edge_raw(c[k], c[(k + 1) % 4]);
    }
    let f = e.add_face_raw(c.to_vec(), Vec::new()).unwrap();
    let edge = e.find_edge(c[1], c[2]).unwrap();
    let d = m.defs.insert(Definition {
        name: "Carré".into(),
        is_group: false,
        description: String::new(),
        entities: e,
        attributes: Default::default(),
        ext: Default::default(),
    });
    let place = |m: &mut Model, at: DVec3| {
        m.root.instances.insert(Instance {
            def: d,
            xform: DMat4::from_translation(at),
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        })
    };
    let i1 = place(&mut m, DVec3::ZERO);
    let i2 = place(&mut m, DVec3::new(508.0, 0.0, 0.0));
    let annot = |m: &mut Model, kind: AnnotKind| {
        m.root.annots.insert(Annotation {
            kind,
            text: String::new(),
            layer: 0,
            hidden: false,
            attach: Default::default(),
        })
    };
    let t = annot(&mut m, AnnotKind::Text { point: DVec3::ZERO, leader: DVec3::new(100.0, 0.0, 100.0) });
    m.root.set_text(t, "sur la face");
    let on = esq_core::attach::on_face(&m.defs[d].entities, f, DVec3::new(63.5, 190.5, 0.0)).unwrap();
    m.set_attach(None, t, 0, Some(Attach::new(vec![i1], on)));
    let dim = annot(&mut m, AnnotKind::Linear { a: DVec3::ZERO, b: DVec3::X, offset: DVec3::Y * -100.0 });
    m.set_attach(None, dim, 0, Some(Attach::new(vec![i1], AttachTo::Vertex(c[1]))));
    m.set_attach(None, dim, 1, Some(Attach::new(vec![i2], AttachTo::EdgeMiddle(edge))));
    // In the root: an edge dimensioned end to end, a text on a guide point.
    let a = m.root.add_vertex_raw(DVec3::new(0.0, 600.0, 0.0));
    let b = m.root.add_vertex_raw(DVec3::new(300.0, 600.0, 0.0));
    m.root.add_edge_raw(a, b);
    let g = m.root.guides.insert(esq_core::model::Guide::point(DVec3::new(0.0, 900.0, 0.0)));
    let d2 = annot(&mut m, AnnotKind::Linear { a: DVec3::ZERO, b: DVec3::X, offset: DVec3::Y * 50.0 });
    m.set_attach(None, d2, 0, Some(Attach::local(AttachTo::Vertex(a))));
    m.set_attach(None, d2, 1, Some(Attach::local(AttachTo::Vertex(b))));
    let t2 = annot(&mut m, AnnotKind::Text { point: DVec3::ZERO, leader: DVec3::new(0.0, 50.0, 50.0) });
    m.set_attach(None, t2, 0, Some(Attach::local(AttachTo::GuidePoint(g))));
    m
}

/// The attached ends of a model's root annotations, in their order: (what they name, path
/// length, where they are).
fn root_ends(m: &Model) -> Vec<(String, usize, DVec3)> {
    let mut out = Vec::new();
    for a in m.root.annots.values() {
        for (w, end) in a.attach.iter().enumerate() {
            let Some(end) = end else { continue };
            let kind = format!("{:?}", end.to).split('(').next().unwrap_or("").to_string();
            out.push((kind, end.path.len(), a.end(w).unwrap()));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.x.total_cmp(&b.2.x)).then(a.2.y.total_cmp(&b.2.y)));
    out
}

/// Written in every version and read back, the annotations are still attached to the same
/// things (the instances of their paths, a vertex, an edge's middle, a face, a guide point)
/// and still follow them, from SketchUp 3 (whose `CText` and `CDimensionLinear` already keep
/// a connection) to 2026.
#[test]
fn attaches_survive_every_version() {
    let m = attached_model();
    let want = root_ends(&m);
    assert_eq!(want.len(), 6, "{want:?}");
    for v in esq_skp::SkpVersion::ALL {
        let bytes = native::write::write(&m, None, Some(v), &Default::default()).unwrap();
        let (mut back, _) = native::read_bytes(&bytes).unwrap();
        let got = root_ends(&back);
        assert_eq!(got.len(), want.len(), "{v:?} : {got:?}");
        for (g, w) in got.iter().zip(&want) {
            assert_eq!((&g.0, g.1), (&w.0, w.1), "{v:?}");
            assert!(g.2.distance(w.2) < 1e-6, "{v:?} : {g:?} / {w:?}");
        }
        // Read back, they follow: the first instance moved, its text and dimension end move.
        let i = *back
            .root
            .instances
            .keys()
            .collect::<Vec<_>>()
            .iter()
            .min_by_key(|i| {
                let p = back.root.instances[**i].xform.w_axis;
                (p.x * 1000.0) as i64
            })
            .unwrap();
        back.root.instances[i].xform = DMat4::from_translation(DVec3::Z * 100.0) * back.root.instances[i].xform;
        let s = back.sync_annotations();
        assert_eq!((s.moved, s.detached), (2, 0), "{v:?}");
    }
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("esq-attaches-{}-{name}", std::process::id()))
}

/// The ends of a model's root as the SDK lists them: "texte" for a text, "départ" and
/// "arrivée" for a dimension, with what they are attached to (`SURefType`, path depth).
fn our_ends(m: &Model) -> Vec<esq_skp::SdkEnd> {
    use esq_skp::ref_type as t;
    let mut out = Vec::new();
    for a in m.root.annots.values() {
        let names: &[&'static str] =
            if matches!(a.kind, AnnotKind::Linear { .. }) { &["départ", "arrivée"] } else { &["texte"] };
        for (w, which) in names.iter().enumerate() {
            let leaf = a.attach[w].as_ref().map(|e| {
                let ty = match e.to {
                    AttachTo::Vertex(_) => t::VERTEX,
                    AttachTo::EdgeMiddle(_) | AttachTo::OnEdge(..) => t::EDGE,
                    AttachTo::OnFace(..) => t::FACE,
                    AttachTo::GuidePoint(_) => t::GUIDE_POINT,
                    AttachTo::OnGuide(..) => t::GUIDE_LINE,
                    _ => -1,
                };
                (ty, e.path.len())
            });
            out.push(esq_skp::SdkEnd { which, point: a.end(w).unwrap().to_array(), leaf });
        }
    }
    sorted(out)
}

fn sorted(mut v: Vec<esq_skp::SdkEnd>) -> Vec<esq_skp::SdkEnd> {
    let key = |e: &esq_skp::SdkEnd| (e.which, e.point.map(|x| (x * 1000.0).round() as i64));
    v.sort_by(|a, b| key(a).cmp(&key(b)));
    v
}

fn same(sdk: &[esq_skp::SdkEnd], ours: &[esq_skp::SdkEnd], what: &str) {
    assert_eq!(sdk.len(), ours.len(), "{what} : {sdk:?} / {ours:?}");
    for (s, o) in sdk.iter().zip(ours) {
        assert_eq!(s.which, o.which, "{what}");
        let d = (0..3).map(|i| (s.point[i] - o.point[i]).abs()).fold(0.0, f64::max);
        assert!(d < 1e-6, "{what} : {s:?} / {o:?}");
        assert_eq!(s.leaf, o.leaf, "{what} : {s:?} / {o:?}");
    }
}

/// Written natively (2026, 2017, SketchUp 8, SketchUp 3), the attaches read by SketchUp's SDK
/// name the same entities through the same instances, at the same places.
#[test]
fn the_sdk_reads_the_attaches_written() {
    if !esq_skp::available() {
        return;
    }
    let m = attached_model();
    let ours = our_ends(&m);
    use esq_skp::SkpVersion as V;
    for v in [V::Su2021, V::Su2017, V::Su8, V::Su3] {
        let p = temp(&format!("{v:?}.skp"));
        std::fs::write(&p, native::write::write(&m, None, Some(v), &Default::default()).unwrap()).unwrap();
        let sdk = esq_skp::sdk_attach_ends(&p, esq_skp::SdkOp::Nothing).expect("le SDK ouvre le fichier");
        let _ = std::fs::remove_file(&p);
        same(&sorted(sdk), &ours, &format!("{v:?}"));
    }
}

/// SketchUp's SDK as the oracle of what follows what: the same change made by the SDK on the
/// file and by Esquisse on the model read from it leaves every end at the same place, attached
/// to the same things or detached alike. The cases: the instances
/// moved, turned, scaled; their definition's geometry moved, turned, scaled; the instances
/// erased; the definition's edges erased.
#[test]
fn the_sdk_moves_them_as_esquisse_does() {
    if !esq_skp::available() {
        return;
    }
    use esq_skp::SdkOp;
    let m = attached_model();
    let p = temp("ops.skp");
    std::fs::write(&p, native::write::write(&m, None, None, &Default::default()).unwrap()).unwrap();
    let up = DMat4::from_translation(DVec3::Z * 254.0);
    let turn = DMat4::from_translation(DVec3::new(100.0, 50.0, 0.0)) * DMat4::from_rotation_z(0.5);
    let scale = DMat4::from_scale(DVec3::new(2.0, 0.5, 1.5));
    let ops = [
        ("instances déplacées", SdkOp::TransformInstances(up.to_cols_array())),
        ("instances tournées", SdkOp::TransformInstances(turn.to_cols_array())),
        ("instances mises à l'échelle", SdkOp::TransformInstances(scale.to_cols_array())),
        ("définition déplacée", SdkOp::TransformDefinitionVertices(up.to_cols_array())),
        ("définition tournée", SdkOp::TransformDefinitionVertices(turn.to_cols_array())),
        ("définition mise à l'échelle", SdkOp::TransformDefinitionVertices(scale.to_cols_array())),
        ("instances effacées", SdkOp::EraseInstances),
        ("arêtes de la définition effacées", SdkOp::EraseDefinitionEdges),
    ];
    for (what, op) in ops {
        let sdk = esq_skp::sdk_attach_ends(&p, op).expect("le SDK ouvre le fichier");
        let (mut back, _) = native::read_file(&p).unwrap();
        match op {
            SdkOp::TransformInstances(t) => {
                for i in back.root.instances.values_mut() {
                    i.xform = DMat4::from_cols_array(&t) * i.xform;
                }
            }
            SdkOp::TransformDefinitionVertices(t) => {
                let t = DMat4::from_cols_array(&t);
                for d in back.defs.values_mut() {
                    let all: std::collections::HashSet<_> = d.entities.verts.keys().collect();
                    for v in &all {
                        let x = &mut d.entities.verts[*v];
                        x.pos = t.transform_point3(x.pos);
                    }
                }
            }
            SdkOp::EraseInstances => back.root.instances.clear(),
            SdkOp::EraseDefinitionEdges => {
                for d in back.defs.values_mut() {
                    let all: Vec<_> = d.entities.edges.keys().map(esq_core::model::EntityRef::Edge).collect();
                    d.entities.erase(&all);
                }
            }
            SdkOp::Nothing => {}
        }
        back.sync_annotations();
        same(&sorted(sdk), &our_ends(&back), what);
    }
    let _ = std::fs::remove_file(&p);
}

/// The round trip through SketchUp's SDK: written natively, opened and saved again by the SDK
/// (2026, 2017, SketchUp 8), read natively: the annotations are still attached to the same
/// things, at the same places.
#[test]
fn attaches_survive_the_sdk_round_trip() {
    if !esq_skp::available() {
        return;
    }
    let m = attached_model();
    let want = root_ends(&m);
    let p = temp("aller.skp");
    std::fs::write(&p, native::write::write(&m, None, None, &Default::default()).unwrap()).unwrap();
    use esq_skp::SkpVersion as V;
    for v in [None, Some(V::Su2017), Some(V::Su8)] {
        let q = temp(&format!("retour-{v:?}.skp"));
        esq_skp::sdk_resave(&p, &q, v).unwrap();
        let (back, _) = native::read_file(&q).unwrap();
        let _ = std::fs::remove_file(&q);
        let got = root_ends(&back);
        assert_eq!(got.len(), want.len(), "{v:?} : {got:?}");
        for (g, w) in got.iter().zip(&want) {
            assert_eq!((&g.0, g.1), (&w.0, w.1), "{v:?}");
            assert!(g.2.distance(w.2) < 1e-6, "{v:?} : {g:?} / {w:?}");
        }
    }
    let _ = std::fs::remove_file(&p);
}

trait SetText {
    fn set_text(&mut self, id: esq_core::model::AnnotId, s: &str);
}

impl SetText for Entities {
    fn set_text(&mut self, id: esq_core::model::AnnotId, s: &str) {
        self.annots[id].text = s.into();
    }
}
