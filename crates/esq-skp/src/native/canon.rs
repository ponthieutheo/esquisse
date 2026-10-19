//! A model as sorted lines of text, class by class, independent of the order of creation and of
//! the slot-map keys: what the differential test compares between the native reader and the
//! SDK reader. Lengths are rounded to 1e-6 mm, angles and factors to 1e-9.

use std::collections::{BTreeMap, HashMap};

use esq_core::glam::{DMat4, DVec3};
use esq_core::meta::TextureData;
use esq_core::model::{AttrValue, Attributes, DefId, Entities, Model};
use slotmap::Key;

/// The classes compared, in the order of the report.
pub const CLASSES: &[&str] = &[
    "unités",
    "étiquettes",
    "dossiers",
    "matières",
    "définitions",
    "arêtes",
    "faces",
    "courbes",
    "instances",
    "attributs",
    "repères",
    "plans de section",
    "annotations",
    "ancrages",
    "espaces",
    "textures de faces",
    "scènes",
    "styles",
    "styles nommés",
    "options",
    "ombres",
    "modèle",
];

fn n(x: f64) -> String {
    let r = (x * 1e6).round() / 1e6;
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

fn f(x: f64) -> String {
    let r = (x * 1e9).round() / 1e9;
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

fn p(v: DVec3) -> String {
    format!("({} {} {})", n(v.x), n(v.y), n(v.z))
}

fn d(v: DVec3) -> String {
    format!("({} {} {})", f(v.x), f(v.y), f(v.z))
}

fn mat4(m: &DMat4) -> String {
    let a = m.to_cols_array();
    let parts: Vec<String> =
        a.iter().enumerate().map(|(i, x)| if (12..15).contains(&i) { n(*x) } else { f(*x) }).collect();
    format!("[{}]", parts.join(" "))
}

fn attr(v: &AttrValue) -> String {
    match v {
        AttrValue::Text(s) => format!("s:{s:?}"),
        AttrValue::Number(x) => format!("f:{}", f(*x)),
        AttrValue::Bool(b) => format!("b:{b}"),
        AttrValue::Json(j) => format!("j:{j}"),
    }
}

fn attrs(a: &Attributes) -> String {
    let mut dicts: Vec<String> = a
        .iter()
        .map(|(name, kv)| {
            let items: Vec<String> = kv.iter().map(|(k, v)| format!("{k:?}={}", attr(v))).collect();
            format!("{name:?}{{{}}}", items.join(", "))
        })
        .collect();
    dicts.sort();
    dicts.join(" ")
}

fn tex(t: &Option<TextureData>) -> String {
    match t {
        None => "-".into(),
        Some(t) => format!(
            "{} {}x{} {}x{} avg{:?} {}o crc{:08x}",
            t.filename,
            t.px[0],
            t.px[1],
            n(t.size[0]),
            n(t.size[1]),
            t.avg,
            t.image.0.len(),
            super::zip::crc32(&t.image.0)
        ),
    }
}

/// Everything about a collection, by its owner name.
struct Ctx<'m> {
    model: &'m Model,
    name: String,
    ents: &'m Entities,
    def: Option<DefId>,
}

fn layer_name(m: &Model, l: usize) -> String {
    m.layers.get(l).map_or_else(|| format!("#{l}"), |x| x.name.clone())
}

fn mat_name(m: &Model, id: Option<esq_core::model::MaterialId>) -> String {
    id.and_then(|i| m.materials.get(i)).map_or("-".into(), |x| x.name.clone())
}

/// Loop points, starting from the smallest point (orientation kept).
fn loop_key(e: &Entities, l: &[esq_core::model::VertexId]) -> String {
    let pts: Vec<String> = l.iter().map(|v| p(e.verts[*v].pos)).collect();
    if pts.is_empty() {
        return "[]".into();
    }
    let k = (0..pts.len()).min_by(|&a, &b| pts[a].cmp(&pts[b])).unwrap_or(0);
    let mut r = pts.clone();
    r.rotate_left(k);
    format!("[{}]", r.join(" "))
}

fn edge_key(e: &Entities, id: esq_core::model::EdgeId) -> String {
    let x = &e.edges[id];
    format!("{}→{}", p(e.verts[x.v[0]].pos), p(e.verts[x.v[1]].pos))
}

fn face_key(e: &Entities, id: esq_core::model::FaceId) -> String {
    let x = &e.faces[id];
    let mut holes: Vec<String> = x.holes.iter().map(|h| loop_key(e, h)).collect();
    holes.sort();
    format!("{} trous{:?}", loop_key(e, &x.outer), holes)
}

/// The canonical lines of a model, by class.
pub fn canon(m: &Model) -> BTreeMap<&'static str, Vec<String>> {
    let mut out: BTreeMap<&'static str, Vec<String>> = CLASSES.iter().map(|c| (*c, Vec::new())).collect();
    let mut push = |c: &'static str, s: String| out.entry(c).or_default().push(s);

    push("unités", format!("{:?}", m.units));
    // Tags and folders.
    let folder_name: HashMap<u32, String> = m.meta.folders.iter().map(|f| (f.uid, f.name.clone())).collect();
    for (i, l) in m.layers.iter().enumerate() {
        push(
            "étiquettes",
            format!(
                "{}{} visible={} couleur={:?} dossier={:?} pages={} trait={:?}",
                if i == 0 { "[défaut] " } else { "" },
                l.name,
                l.visible,
                l.color,
                l.folder.and_then(|u| folder_name.get(&u)),
                l.page_behavior,
                l.line_style
            ),
        );
    }
    let active = m.layer_index_of_uid(m.meta.active_layer).map(|i| layer_name(m, i));
    push("étiquettes", format!("active={active:?}"));
    for fo in &m.meta.folders {
        push(
            "dossiers",
            format!(
                "{} parent={:?} visible={} nouvelles={}",
                fo.name,
                fo.parent.and_then(|u| folder_name.get(&u)),
                fo.visible,
                fo.visible_on_new_pages
            ),
        );
    }
    // Materials.
    for mt in m.materials.values() {
        let e = &mt.ext;
        let pb = &e.pbr;
        push(
            "matières",
            format!(
                "{} rgb{:?} opacité={} tex={} colorisée={} {:?} flux={} image={} pbr[{} {} {} {} {} {} {} {} {} {} {} {} {} {}]",
                mt.name,
                mt.color,
                f(mt.opacity as f64),
                tex(&e.texture),
                e.colorize_type,
                e.colorize_deltas.map(f),
                e.workflow,
                e.owner_image,
                pb.metalness_enabled,
                f(pb.metallic_factor),
                tex(&pb.metallic_texture),
                pb.roughness_enabled,
                f(pb.roughness_factor),
                tex(&pb.roughness_texture),
                pb.roughness_inverted,
                pb.normal_enabled,
                f(pb.normal_scale),
                pb.normal_style,
                tex(&pb.normal_texture),
                pb.ao_enabled,
                f(pb.ao_strength),
                tex(&pb.ao_texture),
            ),
        );
    }
    // Collections.
    let mut ctxs = vec![Ctx { model: m, name: "<racine>".into(), ents: &m.root, def: None }];
    for (id, def) in m.defs.iter() {
        ctxs.push(Ctx { model: m, name: def.name.clone(), ents: &def.entities, def: Some(id) });
        let b = &def.ext.behavior;
        push(
            "définitions",
            format!(
                "{} groupe={} desc={:?} attrs[{}] comportement[{} {} {} {} {} {}] image={:?}",
                def.name,
                def.is_group,
                def.description,
                attrs(&def.attributes),
                b.always_face_camera,
                b.cuts_opening,
                b.is2d,
                b.shadows_face_sun,
                b.no_scale_mask,
                b.snapto,
                def.ext.image.as_ref().map(|i| (i.path.clone(), i.px))
            ),
        );
    }
    let mut owners: HashMap<String, String> = HashMap::new();
    for c in &ctxs {
        let e = c.ents;
        let ctx_ffi = c.def.map_or(0, |d| d.data().as_ffi());
        let props = |key: &str| -> String {
            e.props.get(key).map_or(String::new(), |kv| {
                kv.iter().map(|(k, v)| format!(" {k}={}", attr(v))).collect::<Vec<_>>().join("")
            })
        };
        for (id, x) in e.edges.iter() {
            let key = edge_key(e, id);
            let ffi = id.data().as_ffi();
            owners.insert(format!("h:e:{ctx_ffi}:{ffi}"), format!("{} arête {key}", c.name));
            push(
                "arêtes",
                format!(
                    "{} {key} doux={} lisse={} cachée={} étiquette={} courbe={}{}",
                    c.name,
                    x.soft,
                    x.smooth,
                    x.hidden,
                    layer_name(c.model, x.layer),
                    x.curve.is_some(),
                    props(&format!("e{ffi}"))
                ),
            );
            if let Some(cv) = x.curve {
                push("courbes", format!("{} {key}{}", c.name, props(&format!("c{cv}"))));
            }
        }
        for (id, x) in e.faces.iter() {
            let key = face_key(e, id);
            let ffi = id.data().as_ffi();
            owners.insert(format!("h:f:{ctx_ffi}:{ffi}"), format!("{} face {key}", c.name));
            push(
                "faces",
                format!(
                    "{} {key} n={} avant={} arrière={} étiquette={} cachée={}{}",
                    c.name,
                    d(x.normal),
                    mat_name(m, x.front),
                    mat_name(m, x.back),
                    layer_name(m, x.layer),
                    x.hidden,
                    props(&format!("f{ffi}"))
                ),
            );
            if let Some(t) = &x.tex {
                // A map is known up to a factor (its rows are homogeneous): scaled so that q is 1
                // at the face's first point. Each row: its gradient, then its value at that
                // point (better conditioned than at the origin, far from a face).
                let p0 = x.outer.first().and_then(|v| e.verts.get(*v)).map_or(DVec3::ZERO, |v| v.pos);
                let map = |m: &Option<esq_core::meta::UvMap>| match m {
                    None => "-".to_string(),
                    Some(m) => {
                        let w = m.uvq(p0).z;
                        let k = if w.abs() > 1e-300 { 1.0 / w } else { 1.0 };
                        let r = |c: &[f64; 4]| {
                            let at = c[0] * p0.x + c[1] * p0.y + c[2] * p0.z + c[3];
                            [c[0], c[1], c[2], at].map(|x| f(x * k))
                        };
                        format!("u{:?} v{:?} q{:?}", r(&m.u), r(&m.v), r(&m.q))
                    }
                };
                push(
                    "textures de faces",
                    format!(
                        "{} {key} avant[{}] arrière[{}] projection[{:?} {:?}]",
                        c.name,
                        map(&t.front),
                        map(&t.back),
                        t.projection[0].map(d),
                        t.projection[1].map(d)
                    ),
                );
            }
        }
        for (id, i) in e.instances.iter() {
            let ffi = id.data().as_ffi();
            push(
                "instances",
                format!(
                    "{} → {} {} nom={:?} matière={} étiquette={} cachée={} verrou={} attrs[{}]{}",
                    c.name,
                    m.defs.get(i.def).map_or("?".into(), |x| x.name.clone()),
                    mat4(&i.xform),
                    i.name,
                    mat_name(m, i.material),
                    layer_name(m, i.layer),
                    i.hidden,
                    i.locked,
                    attrs(&i.attributes),
                    props(&format!("i{ffi}"))
                ),
            );
        }
        for g in e.guides.values() {
            push(
                "repères",
                format!(
                    "{} {} {} infini={} étiquette={} cachée={}",
                    c.name,
                    p(g.pos),
                    p(g.dir),
                    g.infinite,
                    layer_name(m, g.layer),
                    g.hidden
                ),
            );
        }
        for (id, s) in e.sections.iter() {
            push(
                "plans de section",
                format!(
                    "{} {} n={} {:?} {:?} active={} étiquette={} cachée={}",
                    c.name,
                    p(s.pos),
                    d(s.normal),
                    s.name,
                    s.symbol,
                    e.active_section == Some(id),
                    layer_name(m, s.layer),
                    s.hidden
                ),
            );
        }
        for s in &e.snaps {
            push(
                "ancrages",
                format!(
                    "{} {} dir={} haut={} étiquette={} cachée={}",
                    c.name,
                    p(s.pos),
                    d(s.dir),
                    d(s.up),
                    layer_name(m, s.layer),
                    s.hidden
                ),
            );
        }
        for s in &e.spaces {
            let pts: Vec<String> = s.perimeter.iter().map(|x| p(*x)).collect();
            let holes: Vec<String> =
                s.holes.iter().map(|h| h.iter().map(|x| p(*x)).collect::<Vec<_>>().join(" ")).collect();
            push(
                "espaces",
                format!(
                    "{} {:?} orientation={} plafond={:?} contour[{}] trous[{}] étiquette={} cachée={}",
                    c.name,
                    s.name,
                    p(s.orientation),
                    s.ceiling.map(n),
                    pts.join(" "),
                    holes.join(" | "),
                    layer_name(m, s.layer),
                    s.hidden
                ),
            );
        }
        for (id, a) in e.annots.iter() {
            let kind = match &a.kind {
                esq_core::model::AnnotKind::Linear { a, b, offset } => {
                    format!("cote {} {} {}", p(*a), p(*b), p(*offset))
                }
                esq_core::model::AnnotKind::Text { point, leader } => format!("texte {} {}", p(*point), p(*leader)),
                esq_core::model::AnnotKind::ScreenText { x, y } => format!("écran {} {}", f(*x), f(*y)),
            };
            push(
                "annotations",
                format!(
                    "{} {kind} {:?} étiquette={} cachée={}{}",
                    c.name,
                    a.text,
                    layer_name(m, a.layer),
                    a.hidden,
                    props(&format!("a{}", id.data().as_ffi()))
                ),
            );
        }
    }
    // Dictionaries kept in the side table.
    for (k, a) in &m.attr_store {
        let owner = owners.get(k).cloned().unwrap_or_else(|| {
            if k.starts_with("h:g:") {
                // A scene's: by its name.
                let uid = k.rsplit(':').next().and_then(|u| u.parse::<u64>().ok());
                let name = m.scenes.iter().find(|s| Some(s.ext.uid) == uid).map(|s| s.name.clone());
                format!("scène {name:?}")
            } else {
                k.clone()
            }
        });
        push("attributs", format!("{owner}: {}", attrs(a)));
    }
    // Scenes.
    for (i, s) in m.scenes.iter().enumerate() {
        let c = &s.camera;
        let mut props = s.ext.props.clone();
        let ro = props.remove("ro");
        // Its style goes with the styles of the model.
        if let Some(st) = props.remove("style") {
            push("styles nommés", format!("scène {:?}: {st}", s.name));
        }
        push(
            "scènes",
            format!(
                "{i} {:?} {:?} œil={} cible={} haut={} fov={} persp={} cachées={:?} {:?}",
                s.name,
                s.description,
                p(c.eye),
                p(c.target),
                d(c.up),
                f(c.fov_deg),
                c.perspective,
                s.hidden_layers.as_ref().map(|v| v.iter().map(|l| layer_name(m, *l)).collect::<Vec<_>>()),
                props
            ),
        );
        if let Some(ro) = ro {
            push("styles", format!("scène {:?}: {ro}", s.name));
        }
    }
    push("scènes", format!("active={:?}", m.view_ext.get(esq_core::model::ACTIVE_SCENE)));
    // The styles of the model: what both readers know of them (the SDK has no more), by name.
    if let Some(st) = m.view_ext.get("styles") {
        let list = st["list"].as_array().cloned().unwrap_or_default();
        for e in &list {
            push("styles nommés", format!("{} desc={} chemin={}", e["name"], e["description"], e["path"]));
        }
        let sel = list.iter().find(|e| e["id"] == st["selected"]).map(|e| e["name"].to_string());
        push("styles nommés", format!("sélectionné={sel:?}"));
    }
    if let Some(ro) = m.view_ext.get("ro").and_then(|v| v.as_object()) {
        for (k, v) in ro {
            push("styles", format!("{k}={v}"));
        }
    }
    for (prov, kv) in &m.meta.options {
        for (k, v) in kv {
            push("options", format!("{prov}/{k}={v}"));
        }
    }
    let s = &m.shadow;
    push(
        "ombres",
        format!(
            "{} {} {} {} {} {} {} {} {} lat={} lon={} nord={} {:?} {:?}",
            s.display,
            s.time,
            f(s.tz_offset),
            s.light,
            s.dark,
            s.use_sun_for_shading,
            s.on_faces,
            s.on_ground,
            s.from_edges,
            f(s.latitude),
            f(s.longitude),
            f(s.north_angle),
            s.city,
            s.country
        ),
    );
    push("modèle", format!("{:?} {:?} guid={:?} axes={}", m.meta.name, m.meta.description, m.meta.guid, mat4(&m.axes)));
    for v in out.values_mut() {
        v.sort();
    }
    out
}

/// The point of view, as a line (compared with the model).
pub fn view(v: &Option<crate::View>) -> String {
    match v {
        None => "-".into(),
        Some(c) => format!("{} {} {} {} {}", p(c.eye), p(c.target), d(c.up), f(c.fov_deg), c.perspective),
    }
}

/// The lines of `a` missing from `b` and those of `b` missing from `a`.
pub fn diff(a: &[String], b: &[String]) -> (Vec<String>, Vec<String>) {
    let mut ca: HashMap<&String, i64> = HashMap::new();
    for x in a {
        *ca.entry(x).or_default() += 1;
    }
    for x in b {
        *ca.entry(x).or_default() -= 1;
    }
    let only_a = a.iter().filter(|x| ca.get(x).is_some_and(|c| *c > 0)).cloned().collect();
    let only_b = b.iter().filter(|x| ca.get(x).is_some_and(|c| *c < 0)).cloned().collect();
    (only_a, only_b)
}
