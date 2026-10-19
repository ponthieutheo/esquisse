//! The native .skp writer (`native::write`) against SketchUp's own, the SDK's writer
//! (`SketchUpAPI.dll`, the oracle: the tests that need it are skipped without it, those that
//! need the corpus without the corpus).
//!
//! - Corpus: each file, read natively, is written in a version by the native writer and by
//!   the SDK. Both files are read back by the SDK and by the native reader, class by class:
//!   ours must read as the SDK's, or else as the file it came from (where the SDK's writer
//!   loses what ours keeps: edge directions, arc angles, polygons, the colours placed on
//!   faces…). Every version, on every file with `ESQ_SKP_WRITE_ALL=1`; by default every file
//!   in the current version and a share of them in each older one.
//! - A model made in Esquisse is written in every version and opens in the SDK.
//! - What the SDK cannot write (snaps, spaces, the selected scene) reads back.
//! - Strange models give an error, never a panic.
//! - Coverage: the classes written, out of those SketchUp writes, per version.
//!
//! `cargo test -p esq-skp --test native_writer -- --nocapture` prints the counts;
//! `ESQ_WRITE_DIFF=<classe>` prints the differences of one class.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::meta::{LayerFolder, TextureData};
use esq_core::model::{AnnotKind, Annotation, AttrValue, EntityRef, Model, Scene, SceneCamera, set_attr};
use esq_core::photo::Blob;
use esq_skp::SkpVersion;
use esq_skp::native::canon::{self, CLASSES};

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() && depth < 8 {
            walk(&p, out, depth + 1);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("skp")) {
            out.push(p);
        }
    }
}

/// The corpus of `native_oracle`: `ESQ_SKP_CORPUS` (or `%USERPROFILE%\tools\skp-corpus`), the
/// .skp files shipped with SketchUp, `tests/data`.
fn corpus() -> Vec<PathBuf> {
    let mut files = Vec::new();
    let dir = std::env::var_os("ESQ_SKP_CORPUS")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("tools").join("skp-corpus")));
    if let Some(d) = dir {
        walk(&d, &mut files, 0);
    }
    if let Some(dll) = esq_skp::find_sketchup_api()
        && let Some(su) = dll.parent()
    {
        walk(su, &mut files, 0);
    }
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data"), &mut files, 0);
    files.sort();
    files.dedup();
    files
}

type Canon = BTreeMap<&'static str, Vec<String>>;

/// The canonical form of a model, with the camera and without the model's GUID (each writer
/// makes its own for a model that has none).
fn canon_of(m: &Model, view: &Option<esq_skp::View>) -> Canon {
    let mut c = canon::canon(m);
    if let Some(v) = c.get_mut("modèle") {
        for l in v.iter_mut() {
            if let Some(i) = l.find("guid=") {
                let end = l[i..].find(' ').map_or(l.len(), |e| i + e);
                l.replace_range(i..end, "guid=…");
            }
        }
    }
    c.insert("caméra", vec![canon::view(view)]);
    c
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("esq-native-writer-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir.join(name)
}

/// Lines of edges (`… (x y z)→(x y z) …`) with their two ends in one order, sorted.
fn undirected(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = lines
        .iter()
        .map(|l| {
            let Some(arrow) = l.find(")→(") else { return l.clone() };
            let (Some(open), Some(close)) = (l[..arrow].rfind('('), l[arrow + 4..].find(')')) else {
                return l.clone();
            };
            // (`→` is 3 bytes long.)
            let (a, b) = (&l[open..=arrow], &l[arrow + 4..arrow + 4 + close + 1]);
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            format!("{}{a}→{b}{}", &l[..open], &l[arrow + 4 + close + 1..])
        })
        .collect();
    out.sort();
    out
}

/// The classes compared, the camera last.
fn classes() -> impl Iterator<Item = &'static str> {
    CLASSES.iter().copied().chain(["caméra"])
}

#[derive(Default)]
struct Tally {
    /// Files written, opened by the SDK (ours / its own), read back natively.
    written: usize,
    sdk_opens: usize,
    sdk_own: usize,
    native_opens: usize,
    /// Per class: files where ours reads as the SDK's (or as the source) / compared, by the
    /// SDK and by the native reader.
    sdk: BTreeMap<&'static str, (usize, usize)>,
    native: BTreeMap<&'static str, (usize, usize)>,
    /// Files whose every class reads the same, by each reader; of them, those that read
    /// exactly as the SDK's file (no class where ours keeps what the SDK's writer loses).
    same_sdk: usize,
    same_native: usize,
    exact_sdk: usize,
    exact_native: usize,
}

#[test]
fn the_corpus_is_written_in_every_version_as_the_sdk_writes_it() {
    if !esq_skp::available() {
        eprintln!("SketchUp absent : test ignoré");
        return;
    }
    let files = corpus();
    if files.is_empty() {
        eprintln!("corpus absent : test ignoré");
        return;
    }
    let all = std::env::var("ESQ_SKP_WRITE_ALL").is_ok_and(|v| v == "1");
    let show = std::env::var("ESQ_WRITE_DIFF").ok();
    let shown = |class: &str| show.as_deref().is_some_and(|s| s.split(',').any(|x| x == class || x == "all"));
    let mut tally: BTreeMap<String, Tally> = BTreeMap::new();
    let mut failures = Vec::new();
    let (a, b) = (tmp("sdk.skp"), tmp("natif.skp"));
    for (i, path) in files.iter().enumerate() {
        let Ok(data) = std::fs::read(path) else { continue };
        let Ok((m, view)) = esq_skp::native::read_bytes(&data) else { continue };
        let quirks = esq_skp::native::Options { sdk_quirks: true };
        let Ok((mq, view_q)) = esq_skp::native::read_bytes_with(&data, &quirks) else { continue };
        let (src, src_q) = (canon_of(&m, &view), canon_of(&mq, &view_q));
        for (k, v) in SkpVersion::ALL.into_iter().enumerate() {
            // By default: every file in the current version, one in 7 in each older one (a
            // different share for each version).
            if !all && v != SkpVersion::Su2021 && (i + k) % 7 != 0 {
                continue;
            }
            let t = tally.entry(format!("{:>4}", v.name())).or_default();
            let ours = match esq_skp::native::write::write(&m, view.as_ref(), Some(v), &Default::default()) {
                Ok(x) => x,
                Err(e) => {
                    failures.push(format!("{} en {} : écriture native : {e}", path.display(), v.name()));
                    continue;
                }
            };
            t.written += 1;
            std::fs::write(&b, &ours).unwrap();
            let theirs = esq_skp::sdk_export_skp_version(&m, &a, view.as_ref(), Some(v)).is_ok();
            // Read back by the SDK.
            let s_nat = esq_skp::sdk_import_skp_view(&b).map(|(x, w)| canon_of(&x, &w));
            let s_sdk =
                if theirs { esq_skp::sdk_import_skp_view(&a).ok().map(|(x, w)| canon_of(&x, &w)) } else { None };
            match &s_nat {
                Ok(_) => t.sdk_opens += 1,
                Err(e) => failures.push(format!("{} en {} : le SDK n'ouvre pas : {e:#}", path.display(), v.name())),
            }
            if s_sdk.is_some() {
                t.sdk_own += 1;
            }
            // Read back natively.
            let n_nat = esq_skp::native::read_bytes(&ours).map(|(x, w)| canon_of(&x, &w));
            let n_sdk = if theirs {
                std::fs::read(&a).ok().and_then(|d| esq_skp::native::read_bytes(&d).ok()).map(|(x, w)| canon_of(&x, &w))
            } else {
                None
            };
            match &n_nat {
                Ok(_) => t.native_opens += 1,
                Err(e) => failures.push(format!("{} en {} : relecture native : {e}", path.display(), v.name())),
            }
            for (who, got, oracle, source, per) in [
                ("SDK", s_nat.as_ref().ok(), s_sdk.as_ref(), &src_q, &mut t.sdk),
                ("natif", n_nat.as_ref().ok(), n_sdk.as_ref(), &src, &mut t.native),
            ] {
                let (Some(got), Some(oracle)) = (got, oracle) else { continue };
                let mut same = true;
                let mut exact = true;
                for class in classes() {
                    let e = per.entry(class).or_default();
                    e.1 += 1;
                    let (x, y, z) = (&got[class], &oracle[class], &source[class]);
                    // SketchUp 3: a curve's first edge must lead to the next one (else the curve
                    // is lost, by the SDK's writer too); ours turns it where needed, the SDK's
                    // takes the directions of the faces' loops: edges compared without their
                    // direction.
                    let loose = v == SkpVersion::Su3 && ["arêtes", "courbes"].contains(&class);
                    let same_as = |a: &[String], b: &[String]| a == b || (loose && undirected(a) == undirected(b));
                    if same_as(x, y) || same_as(x, z) {
                        exact &= same_as(x, y);
                        e.0 += 1;
                        continue;
                    }
                    same = false;
                    failures.push(format!("{} en {} : {class} relu par le {who}", path.display(), v.name()));
                    if shown(class) {
                        let (only_ours, only_theirs) = canon::diff(x, y);
                        eprintln!("== {} en {} ({class}, relu {who})", path.display(), v.name());
                        for l in only_ours.iter().take(6) {
                            eprintln!("  natif  {}", l.chars().take(300).collect::<String>());
                        }
                        for l in only_theirs.iter().take(6) {
                            eprintln!("  SDK    {}", l.chars().take(300).collect::<String>());
                        }
                    }
                }
                if same {
                    let (s, x) = if who == "SDK" {
                        (&mut t.same_sdk, &mut t.exact_sdk)
                    } else {
                        (&mut t.same_native, &mut t.exact_native)
                    };
                    *s += 1;
                    *x += exact as usize;
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(a.parent().unwrap());
    println!("Écrivain natif — corpus : {} fichiers{}", files.len(), if all { ", toutes les versions" } else { "" });
    println!(
        "  version : écrits / ouverts par le SDK (le sien) / relus nativement / identiques relus par le SDK \
         (dont tout à fait comme le fichier du SDK) / identiques relus nativement (dont…)"
    );
    for (v, t) in &tally {
        println!(
            "    {v} : {} / {} ({}) / {} / {} ({}) / {} ({})",
            t.written, t.sdk_opens, t.sdk_own, t.native_opens, t.same_sdk, t.exact_sdk, t.same_native, t.exact_native
        );
    }
    let mut per: BTreeMap<&str, [usize; 4]> = BTreeMap::new();
    for t in tally.values() {
        for (c, (ok, n)) in &t.sdk {
            let e = per.entry(c).or_default();
            (e[0], e[1]) = (e[0] + ok, e[1] + n);
        }
        for (c, (ok, n)) in &t.native {
            let e = per.entry(c).or_default();
            (e[2], e[3]) = (e[2] + ok, e[3] + n);
        }
    }
    println!("  par classe (relu par le SDK, relu nativement) :");
    for (c, [a, b, x, y]) in &per {
        println!("    {c:<20} {a}/{b}  {x}/{y}");
    }
    assert!(failures.is_empty(), "{} écarts :\n{}", failures.len(), failures.join("\n"));
}

// ---------------------------------------------------------------------------------------------
// A model made in Esquisse

fn png(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 7) as u8 ^ seed, (y * 5) as u8, seed]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn texture(name: &str, w: u32, h: u32, seed: u8, size: [f64; 2]) -> TextureData {
    TextureData { filename: name.into(), image: Blob(Arc::new(png(w, h, seed))), px: [w, h], size, avg: [120, 90, 60] }
}

fn rect(e: &mut esq_core::model::Entities, x: f64, y: f64, w: f64, h: f64) {
    e.add_polyline(
        &[DVec3::new(x, y, 0.0), DVec3::new(x + w, y, 0.0), DVec3::new(x + w, y + h, 0.0), DVec3::new(x, y + h, 0.0)],
        true,
        false,
    );
}

/// Geometry, components three deep, textured materials, tags in folders, scenes,
/// attributes, a text, a dimension, a section, guides, snaps, spaces.
fn esquisse_model() -> Model {
    let mut m = Model::new();
    m.meta.name = "Atelier".into();
    let wood = m.add_material("Bois", [180, 120, 60]);
    m.materials[wood].ext.texture = Some(texture("bois.png", 32, 16, 3, [600.0, 300.0]));
    let tinted = m.add_material("Bois teinté", [40, 160, 200]);
    m.materials[tinted].ext.texture = Some(texture("bois2.png", 16, 16, 5, [300.0, 300.0]));
    m.materials[tinted].ext.colorize_deltas = [80.0, 0.3, 0.1];
    let glass = m.add_material("Verre", [200, 220, 255]);
    m.materials[glass].opacity = 0.35;
    let walls = m.add_layer("Murs");
    m.layers[walls].color = Some([200, 10, 10]);
    let furniture = m.add_layer("Meubles");
    let folder = m.new_uid();
    m.meta.folders.push(LayerFolder {
        uid: folder,
        name: "Intérieur".into(),
        parent: None,
        visible: true,
        visible_on_new_pages: true,
    });
    m.layers[furniture].folder = Some(folder);
    set_attr(m.attr_store.entry("model".into()).or_default(), "fiche", "auteur", AttrValue::Text("Théo".into()));

    // A box, painted.
    rect(&mut m.root, 0.0, 0.0, 1000.0, 600.0);
    let f = m.root.faces.keys().next().unwrap();
    m.root.push_pull(f, -400.0, false);
    for (i, f) in m.root.faces.values_mut().enumerate() {
        f.front = Some([wood, tinted, glass][i % 3]);
        f.layer = walls;
    }
    // One face with its texture placed by hand.
    let (fid, n, p0) = {
        let (k, f) = m.root.faces.iter().find(|(_, f)| f.front == Some(wood)).unwrap();
        (k, f.normal, m.root.verts[f.outer[0]].pos)
    };
    let (ex, ey) = esq_core::meta::face_axes(n);
    let pairs =
        [(p0, DVec2::new(0.3, 0.1)), (p0 + ex * 400.0, DVec2::new(1.1, 0.9)), (p0 + ey * 300.0, DVec2::new(-0.2, 1.4))];
    m.root.faces[fid].tex = Some(Box::new(esq_core::meta::FaceTex {
        front: esq_core::meta::uv_from_pairs(n, [600.0, 300.0], &pairs),
        ..Default::default()
    }));
    // A circle, an arc, a curve.
    let ring = |c: DVec3, r: f64, n: usize, sweep: f64| -> Vec<DVec3> {
        let count = if sweep >= std::f64::consts::TAU { n } else { n + 1 };
        (0..count)
            .map(|i| c + DVec3::new((sweep * i as f64 / n as f64).cos(), (sweep * i as f64 / n as f64).sin(), 0.0) * r)
            .collect()
    };
    m.root.add_polyline(&ring(DVec3::new(3000.0, 0.0, 0.0), 300.0, 24, std::f64::consts::TAU), true, true);
    m.root.add_polyline(&ring(DVec3::new(4000.0, 0.0, 0.0), 300.0, 8, 2.0), false, true);
    m.root.add_polyline(
        &[DVec3::new(5000.0, 0.0, 0.0), DVec3::new(5200.0, 300.0, 0.0), DVec3::new(5500.0, 250.0, 0.0)],
        false,
        true,
    );
    // An image.
    m.add_image(
        None,
        "plan",
        "plan.png",
        texture("plan.png", 40, 20, 11, [0.0, 0.0]),
        DVec3::new(0.0, -2000.0, 0.0),
        DVec3::X,
        DVec3::Z,
        1200.0,
        None,
    );
    // A component three deep: a drawer in a cabinet in a kitchen.
    rect(&mut m.root, 2000.0, 0.0, 400.0, 400.0);
    let face = m.root.faces.iter().find(|(_, x)| x.outer.iter().any(|v| m.root.verts[*v].pos.x > 1500.0)).unwrap().0;
    m.root.push_pull(face, 300.0, false);
    let items: Vec<EntityRef> = m
        .root
        .faces
        .iter()
        .filter(|(_, x)| x.outer.iter().any(|v| m.root.verts[*v].pos.x > 1500.0))
        .map(|(k, _)| EntityRef::Face(k))
        .collect();
    let drawer = m.make_group(None, &items, false, Some("Tiroir".into())).unwrap();
    m.root.instances[drawer].layer = furniture;
    let cabinet = m.make_group(None, &[EntityRef::Instance(drawer)], false, Some("Caisson".into())).unwrap();
    let kitchen = m.make_group(None, &[EntityRef::Instance(cabinet)], true, Some("Cuisine".into())).unwrap();
    let mut copy = m.root.instances[kitchen].clone();
    copy.xform = DMat4::from_translation(DVec3::new(0.0, 2000.0, 0.0)) * copy.xform;
    m.root.instances.insert(copy);
    let def = m.root.instances[kitchen].def;
    set_attr(&mut m.defs[def].attributes, "dynamic_attributes", "hauteur", AttrValue::Number(300.0));
    // Annotations, a section, guides.
    m.root.annots.insert(Annotation {
        kind: AnnotKind::Text { point: DVec3::new(0.0, 0.0, 400.0), leader: DVec3::new(100.0, 0.0, 200.0) },
        text: "Établi « chêne »".into(),
        layer: 0,
        hidden: false,
        attach: Default::default(),
    });
    m.root.annots.insert(Annotation {
        kind: AnnotKind::Linear {
            a: DVec3::ZERO,
            b: DVec3::new(1000.0, 0.0, 0.0),
            offset: DVec3::new(0.0, -300.0, 0.0),
        },
        text: String::new(),
        layer: walls,
        hidden: false,
        attach: Default::default(),
    });
    m.root.sections.insert(esq_core::model::SectionPlane::new(DVec3::new(0.0, 300.0, 0.0), DVec3::Y));
    m.root.guides.insert(esq_core::model::Guide::line(DVec3::new(0.0, -250.0, 0.0), DVec3::X));
    m.root.guides.insert(esq_core::model::Guide::point(DVec3::new(1000.0, 0.0, 500.0)));
    // What the SDK cannot write: a snap, a space with a hole.
    m.root.snaps.push(esq_core::model::SnapPoint::new(DVec3::new(500.0, 0.0, 0.0), DVec3::Y, DVec3::Z));
    let mut room = esq_core::model::Space::new(
        "Séjour",
        vec![DVec3::ZERO, DVec3::new(5000.0, 0.0, 0.0), DVec3::new(5000.0, 4000.0, 0.0), DVec3::new(0.0, 4000.0, 0.0)],
        DVec3::new(2500.0, 2000.0, 0.0),
        2500.0,
    );
    room.holes =
        vec![vec![DVec3::new(1000.0, 1000.0, 0.0), DVec3::new(1500.0, 1000.0, 0.0), DVec3::new(1500.0, 1500.0, 0.0)]];
    m.root.spaces.push(room);
    // Two scenes, the second one selected.
    for (name, eye) in [("Face", DVec3::new(500.0, -6000.0, 1500.0)), ("Dessus", DVec3::new(500.0, 300.0, 9000.0))] {
        m.scenes.push(Scene {
            name: name.into(),
            description: String::new(),
            camera: SceneCamera {
                eye,
                target: DVec3::new(500.0, 300.0, 0.0),
                up: DVec3::Y,
                fov_deg: 35.0,
                perspective: true,
            },
            hidden_layers: Some(vec![furniture]),
            photo: None,
            ext: Default::default(),
        });
    }
    m.ensure_scene_uids();
    m.view_ext.insert(esq_core::model::ACTIVE_SCENE.into(), 1.into());
    m
}

fn view() -> esq_skp::View {
    esq_skp::View {
        eye: DVec3::new(4000.0, -5000.0, 3000.0),
        target: DVec3::new(1000.0, 300.0, 200.0),
        up: DVec3::Z,
        fov_deg: 35.0,
        perspective: true,
    }
}

/// The model's faces, components and annotations, whoever read it.
fn counts(m: &Model) -> (usize, usize, usize, usize) {
    let faces = m.root.faces.len() + m.defs.values().map(|d| d.entities.faces.len()).sum::<usize>();
    let defs = m.defs.values().filter(|d| !d.entities.faces.is_empty() || !d.entities.instances.is_empty()).count();
    (faces, defs, m.root.instances.len(), m.root.annots.len())
}

#[test]
fn a_model_made_in_esquisse_opens_in_the_sdk_in_every_version() {
    let m = esquisse_model();
    let want = counts(&m);
    let sdk = esq_skp::available();
    for v in SkpVersion::ALL {
        let p = tmp(&format!("esquisse-{}.skp", v.name()));
        esq_skp::export_skp_version(&m, &p, Some(&view()), Some(v)).unwrap_or_else(|e| panic!("{v:?} : {e:#}"));
        let (back, _) = esq_skp::import_skp_view(&p).unwrap_or_else(|e| panic!("{v:?} : relecture native : {e:#}"));
        assert_eq!(counts(&back), want, "{v:?} relu nativement");
        if sdk {
            let (theirs, w) =
                esq_skp::sdk_import_skp_view(&p).unwrap_or_else(|e| panic!("{v:?} : le SDK n'ouvre pas : {e:#}"));
            assert_eq!(counts(&theirs), want, "{v:?} relu par le SDK");
            assert!(w.is_some_and(|w| w.eye.distance(view().eye) < 1e-6), "{v:?} : caméra");
            // The SDK reads it as the file its own writer makes of the model (but where ours
            // keeps what its writer loses: then ours reads as the model itself).
            let q = tmp(&format!("esquisse-sdk-{}.skp", v.name()));
            esq_skp::sdk_export_skp_version(&m, &q, Some(&view()), Some(v)).unwrap();
            let (own, _) = esq_skp::sdk_import_skp_view(&q).unwrap();
            let _ = std::fs::remove_file(&q);
            let (mut x, mut y, mut z) = (canon_of(&theirs, &None), canon_of(&own, &None), canon_of(&m, &None));
            // A group's definition is renamed when read (as the SDK reader numbers them): the
            // model's by that name too.
            for l in z.get_mut("définitions").unwrap().iter_mut() {
                if let Some(i) = l.find(" groupe=true") {
                    l.replace_range(..i, "Groupe#");
                }
            }
            z.get_mut("définitions").unwrap().sort();
            // What the SDK's writer adds to a model made elsewhere: the empty advanced
            // attributes of a new component and of its instances (`SUComponentDefinitionCreate`,
            // `SUComponentDefinitionCreateInstance`), a definition per group; the SDK reader
            // numbers group definitions. The selected scene, which the SDK writer of Esquisse
            // does not set, is checked below.
            for c in [&mut x, &mut y] {
                c.get_mut("scènes").unwrap().retain(|l| !l.starts_with("active="));
                // Before SketchUp 6 a file has no styles: SketchUp makes them when it opens it,
                // one for the selected scene among them.
                if v.major() < 6 {
                    c.get_mut("styles nommés").unwrap().retain(|l| !l.starts_with("scène "));
                }
                for lines in c.values_mut() {
                    for l in lines.iter_mut() {
                        *l = l
                            .replace(r#""SU_DefinitionSet"{"Price"=s:"", "Size"=s:"", "Url"=s:""}"#, "")
                            .replace(r#""SU_InstanceSet"{"Owner"=s:"", "Status"=s:""}"#, "")
                            .replace("[, ", "[");
                        if let Some(i) = l.find("Groupe#") {
                            let end = l[i + 7..].find(|ch: char| !ch.is_ascii_digit()).map_or(l.len(), |e| i + 7 + e);
                            l.replace_range(i..end, "Groupe#");
                        }
                    }
                    lines.sort();
                    lines.dedup();
                }
            }
            // The SDK lists no snap nor space (`SUEntitiesEntityListFill` has no case for
            // their types: it gives an empty list).
            // A text made without a font has the one SketchUp gives it in the file (Arial 12, as
            // the SDK's writer gives it too).
            let default_font = r#" font_bold=b:false font_italic=b:false font_name=s:"Arial" font_size=f:12"#;
            for c in CLASSES.iter().filter(|c| !["ancrages", "espaces"].contains(c)) {
                let (only_ours, only_theirs) = canon::diff(&x[c], &y[c]);
                let (only_file, only_model) = canon::diff(&x[c], &z[c]);
                let as_model: Vec<String> = x[c]
                    .iter()
                    .map(|l| {
                        let bare = l.replace(default_font, "");
                        if z[c].contains(&bare) { bare } else { l.clone() }
                    })
                    .collect();
                assert!(
                    x[c] == y[c] || x[c] == z[c] || as_model == z[c],
                    "{v:?} : {c} :\nnotre fichier {only_ours:?}\nle sien {only_theirs:?}\nnotre fichier {only_file:?}\nle modèle {only_model:?}"
                );
            }
        }
        let _ = std::fs::remove_file(&p);
    }
}

/// Snaps, spaces (with their perimeter and holes) and the selected scene: the SDK has no way
/// of writing them; the native writer writes them where SketchUp.exe's writers do and its
/// readers read them (`docs/skp-format.md`, §8.4). They read back natively; the SDK reads the
/// selected scene (`SUModelGetActiveScene`) but lists no snap nor space of a file
/// (`SUEntitiesEntityListFill` has no case for their types), whoever wrote it.
#[test]
fn what_the_sdk_cannot_write_reads_back() {
    let m = esquisse_model();
    let p = tmp("ancrages.skp");
    esq_skp::export_skp(&m, &p).unwrap();
    let back = esq_skp::import_skp(&p).unwrap();
    let sdk = esq_skp::available().then(|| esq_skp::sdk_import_skp_view(&p).unwrap().0);
    let _ = std::fs::remove_file(&p);
    let s = &back.root.snaps;
    assert_eq!(s.len(), 1);
    assert!(s[0].pos.distance(DVec3::new(500.0, 0.0, 0.0)) < 1e-6 && s[0].dir.distance(DVec3::Y) < 1e-9, "{s:?}");
    assert!(s[0].up.distance(DVec3::Z) < 1e-9, "{s:?}");
    let sp = &back.root.spaces;
    assert_eq!(sp.len(), 1);
    assert_eq!(sp[0].name, "Séjour");
    assert!(sp[0].orientation.distance(DVec3::new(2500.0, 2000.0, 0.0)) < 1e-6, "{sp:?}");
    let [a, b, c, d] = sp[0].ceiling;
    assert!(a.abs() < 1e-9 && b.abs() < 1e-9 && (c - 1.0).abs() < 1e-9 && (d + 2500.0).abs() < 1e-6, "{sp:?}");
    let near = |a: &[DVec3], b: &[DVec3]| a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.distance(*q) < 1e-6);
    assert!(near(&sp[0].perimeter, &m.root.spaces[0].perimeter), "{sp:?}");
    assert_eq!(sp[0].holes.len(), 1, "{sp:?}");
    assert!(near(&sp[0].holes[0], &m.root.spaces[0].holes[0]), "{sp:?}");
    for back in std::iter::once(&back).chain(sdk.as_ref()) {
        assert_eq!(back.view_ext.get(esq_core::model::ACTIVE_SCENE), Some(&serde_json::json!(1)));
    }
    // Older versions have no snap nor space (SketchUp 2026 leaves them out too); the
    // selected scene is kept.
    let p = tmp("ancrages-2020.skp");
    esq_skp::export_skp_version(&m, &p, None, Some(SkpVersion::Su2020)).unwrap();
    let back = esq_skp::import_skp(&p).unwrap();
    let _ = std::fs::remove_file(&p);
    assert!(back.root.snaps.is_empty() && back.root.spaces.is_empty());
    assert_eq!(back.view_ext.get(esq_core::model::ACTIVE_SCENE), Some(&serde_json::json!(1)));
}

/// Models no file holds as such: the writer answers with an error, or writes a file the
/// native reader opens; it never panics.
#[test]
fn strange_models_give_an_error_not_a_panic() {
    let mut cases: Vec<(&str, Model)> = Vec::new();
    // A coordinate that is not a number.
    let mut m = Model::new();
    m.root.add_line(DVec3::ZERO, DVec3::new(f64::NAN, 0.0, 0.0));
    cases.push(("NaN", m));
    let mut m = Model::new();
    m.root.add_line(DVec3::ZERO, DVec3::new(f64::INFINITY, 0.0, 0.0));
    cases.push(("infini", m));
    // Empty names, unicode everywhere.
    let mut m = Model::new();
    let mat = m.add_material("", [1, 2, 3]);
    let _ = m.add_material("Matière 🎨 \u{0}\u{7f} <&>\"'", [4, 5, 6]);
    m.add_layer("");
    m.add_layer("Étiquette 漢字 \u{202e}");
    rect(&mut m.root, 0.0, 0.0, 10.0, 10.0);
    for f in m.root.faces.values_mut() {
        f.front = Some(mat);
    }
    let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    let g = m.make_group(None, &items, true, Some(String::new())).unwrap();
    m.root.instances[g].name = "\u{fffd}".repeat(3000);
    m.scenes.push(Scene {
        name: String::new(),
        description: "\n\t".into(),
        camera: SceneCamera { eye: DVec3::ONE, target: DVec3::ZERO, up: DVec3::Z, fov_deg: 35.0, perspective: false },
        hidden_layers: None,
        photo: None,
        ext: Default::default(),
    });
    set_attr(m.attr_store.entry("model".into()).or_default(), "", "", AttrValue::Text(String::new()));
    cases.push(("noms", m));
    // A material whose image is missing or is not an image.
    let mut m = Model::new();
    let a = m.add_material("Sans image", [9, 9, 9]);
    m.materials[a].ext.texture = Some(TextureData {
        filename: String::new(),
        image: Blob(Arc::new(Vec::new())),
        px: [0, 0],
        size: [0.0, 0.0],
        avg: [0, 0, 0],
    });
    let b = m.add_material("Image fausse", [9, 9, 9]);
    m.materials[b].ext.texture = Some(TextureData {
        filename: "x.png".into(),
        image: Blob(Arc::new(b"pas une image".to_vec())),
        px: [4, 4],
        size: [100.0, 100.0],
        avg: [1, 2, 3],
    });
    rect(&mut m.root, 0.0, 0.0, 10.0, 10.0);
    for (i, f) in m.root.faces.values_mut().enumerate() {
        (f.front, f.back) = (Some(a), Some(b));
        let _ = i;
    }
    cases.push(("matière sans image", m));
    // Components nested very deep.
    let mut m = Model::new();
    rect(&mut m.root, 0.0, 0.0, 10.0, 10.0);
    let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    let mut inst = m.make_group(None, &items, true, Some("0".into())).unwrap();
    // (The native reader takes a time that grows as the cube of the depth: 300 levels.)
    for i in 1..300 {
        inst = m.make_group(None, &[EntityRef::Instance(inst)], true, Some(i.to_string())).unwrap();
    }
    cases.push(("profondeur 300", m));
    // A definition that contains itself.
    let mut m = Model::new();
    rect(&mut m.root, 0.0, 0.0, 10.0, 10.0);
    let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    let g = m.make_group(None, &items, true, Some("Boucle".into())).unwrap();
    let d = m.root.instances[g].def;
    let mut me = m.root.instances[g].clone();
    me.xform = DMat4::IDENTITY;
    m.defs[d].entities.instances.insert(me);
    cases.push(("définition qui se contient", m));
    // A texture position that is degenerate.
    let mut m = Model::new();
    let t = m.add_material("Plaquée", [1, 1, 1]);
    m.materials[t].ext.texture = Some(texture("t.png", 4, 4, 1, [100.0, 100.0]));
    rect(&mut m.root, 0.0, 0.0, 10.0, 10.0);
    let n = DVec3::Z;
    for f in m.root.faces.values_mut() {
        f.front = Some(t);
        let pairs = [(DVec3::ZERO, DVec2::ZERO), (DVec3::ZERO, DVec2::ONE)];
        f.tex = Some(Box::new(esq_core::meta::FaceTex {
            front: esq_core::meta::uv_from_pairs(n, [100.0, 100.0], &pairs),
            ..Default::default()
        }));
    }
    cases.push(("placage dégénéré", m));

    for (name, m) in &cases {
        for v in SkpVersion::ALL {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                esq_skp::native::write::write(m, None, Some(v), &Default::default())
            }));
            match r {
                Err(_) => panic!("{name} en {} : panique", v.name()),
                Ok(Err(e)) => eprintln!("{name} en {} : {e}", v.name()),
                Ok(Ok(bytes)) => {
                    let back = std::panic::catch_unwind(|| esq_skp::native::read_bytes(&bytes));
                    assert!(matches!(back, Ok(Ok(_))), "{name} en {} : fichier écrit illisible", v.name());
                }
            }
        }
    }
    // What a file cannot hold is an error.
    for name in ["NaN", "infini", "définition qui se contient"] {
        let m = &cases.iter().find(|c| c.0 == name).unwrap().1;
        assert!(esq_skp::native::write::write(m, None, None, &Default::default()).is_err(), "{name}");
    }
}

/// Components nested `n` deep (a square at the bottom).
fn chain(n: usize) -> Model {
    let mut m = Model::new();
    rect(&mut m.root, 0.0, 0.0, 10.0, 10.0);
    let items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    let mut inst = m.make_group(None, &items, false, Some("0".into())).unwrap();
    for i in 1..n {
        inst = m.make_group(None, &[EntityRef::Instance(inst)], false, Some(i.to_string())).unwrap();
    }
    m
}

/// A component of the file named « Groupe#1 », as Esquisse names its groups, next to a group:
/// both readers keep the component's name and give the group's definition another. The native
/// reader reserves the names of the file's components (f59f125, `Foldarap.skp`); the SDK reader
/// named the group it reads first « Groupe#1 » as well, until it reserved them too.
#[test]
fn no_group_takes_the_name_of_a_component_of_the_file() {
    let mut m = Model::new();
    rect(&mut m.root, 0.0, 0.0, 100.0, 100.0);
    rect(&mut m.root, 500.0, 0.0, 100.0, 100.0);
    let faces: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    assert_eq!(faces.len(), 2);
    m.make_group(None, &faces[..1], false, Some("Groupe#1".into())).unwrap();
    m.make_group(None, &faces[1..], true, None).unwrap();
    let p = tmp("groupe-1.skp");
    esq_skp::export_skp_version(&m, &p, None, Some(SkpVersion::Su2021)).unwrap();
    let names = |m: &Model| {
        let mut v: Vec<(bool, String)> = m.defs.values().map(|d| (d.is_group, d.name.clone())).collect();
        v.sort();
        v
    };
    let want = vec![(false, "Groupe#1".to_string()), (true, "Groupe#2".to_string())];
    let (native, _) = esq_skp::import_skp_view(&p).unwrap();
    assert_eq!(names(&native), want, "lecteur natif");
    if esq_skp::available() {
        let (sdk, _) = esq_skp::sdk_import_skp_view(&p).unwrap();
        assert_eq!(names(&sdk), want, "lecteur du SDK");
    }
    let _ = std::fs::remove_file(&p);
}

/// The writer is not recursive on its caller's stack: from a thread of 1 MB (the main
/// thread's on Windows), in a debug build, the deepest model Esquisse keeps when it reads one
/// (256 levels, `Model::check_after_reading`) is written in every version and reads back;
/// so is the deepest the writer takes (`MAX_NESTING`), and one level more is an error.
#[test]
fn deep_nesting_is_written_from_a_small_stack() {
    use esq_skp::native::write::{MAX_NESTING, write};
    let (kept, deepest, beyond) = (chain(256), chain(MAX_NESTING), chain(MAX_NESTING + 1));
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn_scoped(s, || {
                for v in SkpVersion::ALL {
                    let bytes =
                        write(&kept, None, Some(v), &Default::default()).unwrap_or_else(|e| panic!("{v:?} : {e}"));
                    let (back, _) = esq_skp::native::read_bytes(&bytes).unwrap_or_else(|e| panic!("{v:?} : {e}"));
                    assert_eq!(back.defs.len(), 256, "{v:?}");
                    assert!(write(&deepest, None, Some(v), &Default::default()).is_ok(), "{v:?}");
                    let e = write(&beyond, None, Some(v), &Default::default()).err().map(|e| e.0);
                    assert_eq!(e, Some(format!("plus de {MAX_NESTING} définitions imbriquées")), "{v:?}");
                }
            })
            .unwrap()
            .join()
            .unwrap();
    });
}

/// The classes the native writer writes, out of those SketchUp writes: for 2021+ the class
/// records of SketchUp.exe's writers (`vff::CLASSES`), for the MFC archives the classes of
/// the version's map (each one a class SketchUp 2026 declares when it saves for that version).
#[test]
fn coverage_of_sketchups_classes_per_version() {
    use esq_skp::native::write::{legacy, plan, versions};
    let m = esquisse_model();
    let p = plan::plan(&m, Some(&view())).unwrap();
    // 2021+: the classes met in our model.dat.
    let bytes = esq_skp::native::write::write(&m, Some(&view()), None, &Default::default()).unwrap();
    let f = esq_skp::native::container::open(&bytes).unwrap();
    let esq_skp::native::container::Body::Zip { archive, .. } = &f.body else { panic!("pas une archive") };
    let mut met = BTreeMap::new();
    esq_skp::native::vff::class_census(&archive.read_name("model.dat").unwrap(), &mut met);
    let all = esq_skp::native::vff::CLASSES;
    let missing: Vec<&str> = all.iter().filter(|c| !met.contains_key(&c.0)).map(|c| c.1).collect();
    println!("Couverture de l'écrivain natif (classes écrites / classes que SketchUp écrit) :");
    println!("  2021+ : {}/{} ; non écrites : {}", all.len() - missing.len(), all.len(), missing.join(", "));
    // Nothing the reader reads is left unwritten, but the classes this model has none of.
    const NOT_WRITTEN: &[&str] = &[
        "CPolyline3d",
        "CDimensionRadial",
        "CThumbnail",
        "CMatchPhoto",
        "CWatermark",
        "CSchemaZipFile",
        "CEnvironment",
        "HatchPattern",
        "CRelationship",
        "CDib (fichier de l'archive)",
    ];
    for c in all.iter().filter(|c| c.2) {
        assert!(met.contains_key(&c.0) || NOT_WRITTEN.contains(&c.1), "{} lue mais pas écrite", c.1);
    }
    for v in SkpVersion::ALL.into_iter().filter(|v| *v != SkpVersion::Su2021) {
        let t = legacy::traced(&p, v, false);
        let map = versions::map(v);
        let missing: Vec<&str> = map.iter().map(|c| c.0).filter(|c| !t.classes.contains(c)).collect();
        println!(
            "  {:>4} : {}/{} ; non écrites : {}",
            v.name(),
            map.len() - missing.len(),
            map.len(),
            missing.join(", ")
        );
        // Every class written is one of the version's.
        for c in &t.classes {
            assert!(map.iter().any(|x| x.0 == *c), "{v:?} : {c} hors de la table de version");
        }
    }
}
