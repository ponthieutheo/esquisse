//! Native side of the Ruby API: `Esq.__native(op, json)` lands here. The Ruby prelude builds
//! the SketchUp classes on top of these operations. Lengths cross the bridge in inches (the
//! unit of SketchUp's Ruby API) and are converted to the model's millimetres here.

use glam::{DMat4, DVec3, DVec4};
mod entities;
mod geom;
mod instances;
pub(crate) mod misc;
mod model;
mod tools;
mod ui;
mod view;

use serde_json::{Value as J, json};
use slotmap::{Key, KeyData};

pub use view::{shadow_restore, shadow_snapshot};

use esq_core::model::{DefId, EdgeId, Entities, EntityRef, FaceId, InstanceId, MaterialId, Model};

const INCH: f64 = 25.4;

/// What a running script can touch.
pub struct Ctx<'a> {
    pub model: &'a mut Model,
    pub sel: &'a mut Vec<EntityRef>,
}

pub(crate) type R = Result<J, String>;

pub(crate) fn def_str(c: Option<DefId>) -> String {
    c.map_or("0".into(), |d| d.data().as_ffi().to_string())
}

pub(crate) fn parse_ctx(s: &str) -> Result<Option<DefId>, String> {
    if s == "0" {
        return Ok(None);
    }
    s.parse::<u64>().map(|n| Some(DefId::from(KeyData::from_ffi(n)))).map_err(|_| format!("contexte invalide: {s}"))
}

pub(crate) fn handle(kind: char, ctx: Option<DefId>, id: u64) -> String {
    format!("{kind}:{}:{id}", def_str(ctx))
}

pub(crate) struct H {
    pub kind: char,
    pub ctx: Option<DefId>,
    pub id: u64,
}

pub(crate) fn parse_h(s: &str) -> Result<H, String> {
    let mut it = s.splitn(3, ':');
    let (k, c, i) = (it.next(), it.next(), it.next());
    let (Some(k), Some(c), Some(i)) = (k, c, i) else { return Err(format!("poignée invalide: {s}")) };
    Ok(H {
        kind: k.chars().next().ok_or("poignée vide")?,
        ctx: parse_ctx(c)?,
        id: i.parse().map_err(|_| format!("poignée invalide: {s}"))?,
    })
}

pub(crate) fn eref_handle(ctx: Option<DefId>, e: EntityRef) -> Option<String> {
    Some(match e {
        EntityRef::Edge(x) => handle('e', ctx, x.data().as_ffi()),
        EntityRef::Face(x) => handle('f', ctx, x.data().as_ffi()),
        EntityRef::Instance(x) => handle('i', ctx, x.data().as_ffi()),
        EntityRef::Guide(x) => handle('g', ctx, x.data().as_ffi()),
        EntityRef::Annot(x) => handle('a', ctx, x.data().as_ffi()),
        EntityRef::Section(x) => handle('s', ctx, x.data().as_ffi()),
        EntityRef::Snap(x) => handle('n', ctx, u64::from(x)),
        EntityRef::Space(x) => handle('p', ctx, u64::from(x)),
    })
}

pub(crate) fn to_eref(h: &H) -> Result<EntityRef, String> {
    let k = KeyData::from_ffi(h.id);
    Ok(match h.kind {
        'e' => EntityRef::Edge(EdgeId::from(k)),
        'f' => EntityRef::Face(FaceId::from(k)),
        'i' => EntityRef::Instance(InstanceId::from(k)),
        'g' => EntityRef::Guide(slotmap::KeyData::from_ffi(h.id).into()),
        'a' => EntityRef::Annot(slotmap::KeyData::from_ffi(h.id).into()),
        's' => EntityRef::Section(slotmap::KeyData::from_ffi(h.id).into()),
        'n' => EntityRef::Snap(u32::try_from(h.id).map_err(|_| "poignée invalide")?),
        'p' => EntityRef::Space(u32::try_from(h.id).map_err(|_| "poignée invalide")?),
        c => return Err(format!("type d'entité non géré: {c}")),
    })
}

pub(crate) fn pt(j: &J) -> Result<DVec3, String> {
    let a = j.as_array().filter(|a| a.len() >= 3).ok_or("point attendu [x,y,z]")?;
    let f = |i: usize| a[i].as_f64().ok_or("nombre attendu");
    Ok(DVec3::new(f(0)?, f(1)?, f(2)?) * INCH)
}

pub(crate) fn pts(j: &J) -> Result<Vec<DVec3>, String> {
    j.as_array().ok_or("liste de points attendue")?.iter().map(pt).collect()
}

/// Millimetres → inches, without the float noise of the round trip (3" comes back as 3.0).
pub(crate) fn inches(mm: f64) -> f64 {
    let v = mm / INCH;
    let r = (v * 1e10).round() / 1e10 + 0.0;
    if r.abs() < 1e15 { r } else { v }
}

pub(crate) fn out_pt(p: DVec3) -> J {
    json!([inches(p.x), inches(p.y), inches(p.z)])
}

pub(crate) fn mat_from(j: &J) -> Result<DMat4, String> {
    let a = j.as_array().filter(|a| a.len() == 16).ok_or("matrice de 16 nombres attendue")?;
    let mut m = [0.0; 16];
    for (i, v) in a.iter().enumerate() {
        m[i] = v.as_f64().ok_or("nombre attendu")?;
    }
    for t in &mut m[12..15] {
        *t *= INCH;
    }
    Ok(DMat4::from_cols(
        DVec4::new(m[0], m[1], m[2], m[3]),
        DVec4::new(m[4], m[5], m[6], m[7]),
        DVec4::new(m[8], m[9], m[10], m[11]),
        DVec4::new(m[12], m[13], m[14], m[15]),
    ))
}

pub(crate) fn mat_to(m: &DMat4) -> J {
    let c = m.to_cols_array();
    let mut v: Vec<f64> = c.to_vec();
    for t in &mut v[12..15] {
        *t = inches(*t);
    }
    json!(v)
}

pub(crate) fn str_of<'a>(a: &'a J, k: &str) -> Result<&'a str, String> {
    a.get(k).and_then(J::as_str).ok_or_else(|| format!("argument « {k} » manquant"))
}

pub(crate) fn ctx_of(a: &J) -> Result<Option<DefId>, String> {
    parse_ctx(a.get("ctx").and_then(J::as_str).unwrap_or("0"))
}

pub(crate) fn area(e: &Entities, f: FaceId) -> f64 {
    let face = &e.faces[f];
    let ring = |l: &[esq_core::model::VertexId]| {
        let p = e.loop_points(l);
        let mut s = DVec3::ZERO;
        for i in 0..p.len() {
            s += p[i].cross(p[(i + 1) % p.len()]);
        }
        (s.dot(face.normal) / 2.0).abs()
    };
    let holes: f64 = face.holes.iter().map(|h| ring(h)).sum();
    (ring(&face.outer) - holes) / (INCH * INCH)
}

pub(crate) fn mat_name(m: &Model, id: Option<MaterialId>) -> J {
    id.and_then(|i| m.materials.get(i)).map_or(J::Null, |x| json!(x.name))
}

pub fn dispatch(c: &mut Ctx, op: &str, a: &J) -> R {
    // What a script adds goes on the model's active tag, as in SketchUp (the drawing tools do
    // the same, `dialogs::tag_dialog::tag_new`).
    let r = if c.model.meta.active_layer != 0 && ADDING.contains(&op) {
        with_active_tag(c, op, a)
    } else {
        dispatch_raw(c, op, a)
    };
    // The dimensions and texts attached to what the operation changed follow it at once, as
    // SketchUp's observers move them (`esq_core::attach`: only the attached ones are looked at).
    if r.is_ok() {
        c.model.sync_annotations();
    }
    r
}

/// Operations that add entities to the collection named by their `ctx` (or `dest`) argument.
const ADDING: &[&str] = &[
    "ents.add_edges",
    "ents.add_curve",
    "ents.add_face",
    "ents.add_group",
    "ents.add_instance",
    "ents.make_group",
    "ent.add_face",
    "ent.add_curve",
    "ent.add_mesh",
    "ent.intersect",
    "guide.add",
    "annot.add",
    "section.add",
    "image.add",
    "snap.add",
    "space.add",
    "host.entities.text3d",
];

/// Runs an adding operation, then gives the active tag to the untagged entities it made.
fn with_active_tag(c: &mut Ctx, op: &str, a: &J) -> R {
    let tag = c.model.layer_index_of_uid(c.model.meta.active_layer).unwrap_or(0);
    let ctx = |k: &str| a.get(k).and_then(J::as_str).map(parse_ctx).transpose();
    let mut ctxs = vec![ctx("ctx")?.flatten()];
    if let Some(d) = ctx("dest")? {
        ctxs.push(d);
    }
    let keys = |m: &Model, x: Option<DefId>| -> Option<std::collections::HashSet<String>> {
        let e = match x {
            Some(d) => &m.defs.get(d)?.entities,
            None => &m.root,
        };
        Some(entity_keys(e).collect())
    };
    let before: Vec<_> = ctxs.iter().map(|&x| (x, keys(c.model, x))).collect();
    let r = dispatch_raw(c, op, a)?;
    for (x, old) in before {
        let Some(old) = old else { continue };
        let e = match x {
            Some(d) => match c.model.defs.get_mut(d) {
                Some(d) => &mut d.entities,
                None => continue,
            },
            None => &mut c.model.root,
        };
        let fresh: Vec<String> = entity_keys(e).filter(|k| !old.contains(k)).collect();
        for k in fresh {
            let (kind, id) = k.split_at(1);
            let id: u64 = id.parse().unwrap_or(0);
            let er = to_eref(&H { kind: kind.chars().next().unwrap_or('?'), ctx: x, id });
            if let Ok(er) = er
                && e.layer_of(er) == Some(0)
            {
                e.set_layer(er, tag);
            }
        }
    }
    Ok(r)
}

/// Keys (`<kind><id>`) of every entity of a collection.
fn entity_keys(e: &Entities) -> impl Iterator<Item = String> + '_ {
    let k = |c: char, id: u64| format!("{c}{id}");
    e.edges
        .keys()
        .map(move |x| k('e', x.data().as_ffi()))
        .chain(e.faces.keys().map(move |x| k('f', x.data().as_ffi())))
        .chain(e.instances.keys().map(move |x| k('i', x.data().as_ffi())))
        .chain(e.guides.keys().map(move |x| k('g', x.data().as_ffi())))
        .chain(e.annots.keys().map(move |x| k('a', x.data().as_ffi())))
        .chain(e.sections.keys().map(move |x| k('s', x.data().as_ffi())))
        .chain(e.snaps.iter().map(move |s| k('n', u64::from(s.id))))
        .chain(e.spaces.iter().map(move |s| k('p', u64::from(s.id))))
}

fn dispatch_raw(c: &mut Ctx, op: &str, a: &J) -> R {
    if let Some(rest) = op.strip_prefix("host.") {
        return crate::host::call(c.model, c.sel, rest, a);
    }
    for f in [
        geom::dispatch,
        entities::dispatch,
        instances::dispatch,
        model::dispatch,
        view::dispatch,
        ui::dispatch,
        tools::dispatch,
        misc::dispatch,
    ] {
        if let Some(r) = f(c, op, a) {
            return r;
        }
    }
    core(c, op, a)
}

/// True while a network request or socket is open (poll again later).
pub fn network_active() -> bool {
    misc::active()
}

/// True when network results wait to be delivered to Ruby (see `Interpreter::pump`).
pub fn pending_events() -> bool {
    misc::pending()
}

/// Operations of the first iteration; their owners may move them into their own module.
fn core(c: &mut Ctx, op: &str, a: &J) -> R {
    match op {
        "ping" => Ok(json!("pong")),
        "model.info" => Ok(json!({
            "units": format!("{:?}", c.model.units),
            "scale_to_inch": 1.0,
        })),
        "ents.list" => {
            let ctx = ctx_of(a)?;
            let e = c.model.entities(ctx);
            let mut v = Vec::new();
            for (id, _) in e.edges.iter() {
                v.push(json!(handle('e', ctx, id.data().as_ffi())));
            }
            for (id, _) in e.faces.iter() {
                v.push(json!(handle('f', ctx, id.data().as_ffi())));
            }
            for (id, _) in e.instances.iter() {
                v.push(json!(handle('i', ctx, id.data().as_ffi())));
            }
            Ok(json!(v))
        }
        "ents.add_edges" | "ents.add_curve" => {
            let ctx = ctx_of(a)?;
            let p = pts(&a["pts"])?;
            let closed = a.get("closed").and_then(J::as_bool).unwrap_or(false);
            let e = c.model.entities_mut(ctx);
            let ids = e.add_polyline_edges(&p, closed, op == "ents.add_curve");
            Ok(json!(ids.into_iter().map(|x| handle('e', ctx, x.data().as_ffi())).collect::<Vec<_>>()))
        }
        "ents.add_face" => {
            let ctx = ctx_of(a)?;
            let p = pts(&a["pts"])?;
            if p.len() < 3 {
                return Err("add_face: au moins 3 points".into());
            }
            let e = c.model.entities_mut(ctx);
            let vs: Vec<_> = p.iter().map(|&q| e.vertex_at(q)).collect();
            let mut sorted = vs.clone();
            sorted.sort();
            if let Some((f, _)) = e.faces.iter().find(|(_, f)| {
                let mut o = f.outer.clone();
                o.sort();
                o == sorted
            }) {
                return Ok(json!(handle('f', ctx, f.data().as_ffi())));
            }
            let f = e.add_face_raw(vs, vec![]).ok_or("add_face: points alignés ou dupliqués")?;
            Ok(json!(handle('f', ctx, f.data().as_ffi())))
        }
        "ents.add_group" | "ents.add_instance" => {
            let ctx = ctx_of(a)?;
            let is_group = op == "ents.add_group";
            let def = if is_group {
                let name = c.model.unique_def_name("Groupe");
                c.model.defs.insert(esq_core::model::Definition {
                    name,
                    is_group: true,
                    description: String::new(),
                    entities: Entities::default(),
                    attributes: Default::default(),
                    ext: Default::default(),
                })
            } else {
                let h = parse_h(str_of(a, "def")?)?;
                DefId::from(KeyData::from_ffi(h.id))
            };
            if !c.model.defs.contains_key(def) {
                return Err("définition inconnue".into());
            }
            let xform = if let Some(m) = a.get("xform").filter(|m| !m.is_null()) { mat_from(m)? } else { DMat4::IDENTITY };
            let i = c.model.entities_mut(ctx).instances.insert(esq_core::model::Instance {
                def,
                xform,
                name: String::new(),
                material: None,
                layer: 0,
                hidden: false,
                locked: false,
                attributes: Default::default(),
                ext: Default::default(),
            });
            Ok(json!(handle('i', ctx, i.data().as_ffi())))
        }
        "ents.make_group" => {
            let ctx = ctx_of(a)?;
            let mut items = Vec::new();
            for h in a["hs"].as_array().ok_or("hs attendu")? {
                items.push(to_eref(&parse_h(h.as_str().ok_or("poignée attendue")?)?)?);
            }
            let id = c.model.make_group(ctx, &items, true, None).ok_or("make_group: rien à grouper")?;
            Ok(json!(handle('i', ctx, id.data().as_ffi())))
        }
        "ents.erase" => {
            let ctx = ctx_of(a)?;
            let mut items = Vec::new();
            for h in a["hs"].as_array().ok_or("hs attendu")? {
                items.push(to_eref(&parse_h(h.as_str().ok_or("poignée attendue")?)?)?);
            }
            c.model.entities_mut(ctx).erase(&items);
            Ok(J::Null)
        }
        "ents.clear" => {
            let ctx = ctx_of(a)?;
            let e = c.model.entities_mut(ctx);
            let all: Vec<EntityRef> = e
                .instances
                .keys()
                .map(EntityRef::Instance)
                .chain(e.faces.keys().map(EntityRef::Face))
                .chain(e.edges.keys().map(EntityRef::Edge))
                .collect();
            e.erase(&all);
            Ok(J::Null)
        }
        "valid" => {
            let h = parse_h(str_of(a, "h")?)?;
            let e = c.model.entities(h.ctx);
            let k = KeyData::from_ffi(h.id);
            Ok(json!(match h.kind {
                'e' => e.edges.contains_key(EdgeId::from(k)),
                'f' => e.faces.contains_key(FaceId::from(k)),
                'i' => e.instances.contains_key(InstanceId::from(k)),
                _ => false,
            }))
        }
        "edge.info" => {
            let h = parse_h(str_of(a, "h")?)?;
            let e = c.model.entities(h.ctx);
            let id = EdgeId::from(KeyData::from_ffi(h.id));
            let ed = e.edges.get(id).ok_or("arête supprimée")?;
            let (p, q) = e.edge_points(id);
            Ok(json!({
                "a": out_pt(p), "b": out_pt(q),
                "faces": ed.faces.iter().map(|f| handle('f', h.ctx, f.data().as_ffi())).collect::<Vec<_>>(),
                "soft": ed.soft, "smooth": ed.smooth, "hidden": ed.hidden, "layer": ed.layer,
                "curve": ed.curve,
            }))
        }
        "edge.set" => {
            let h = parse_h(str_of(a, "h")?)?;
            let id = EdgeId::from(KeyData::from_ffi(h.id));
            let ed = c.model.entities_mut(h.ctx).edges.get_mut(id).ok_or("arête supprimée")?;
            if let Some(v) = a.get("soft").and_then(J::as_bool) { ed.soft = v; }
            if let Some(v) = a.get("smooth").and_then(J::as_bool) { ed.smooth = v; }
            if let Some(v) = a.get("hidden").and_then(J::as_bool) { ed.hidden = v; }
            Ok(J::Null)
        }
        "face.info" => {
            let h = parse_h(str_of(a, "h")?)?;
            let id = FaceId::from(KeyData::from_ffi(h.id));
            let e = c.model.entities(h.ctx);
            let f = e.faces.get(id).ok_or("face supprimée")?;
            let ring = |l: &Vec<esq_core::model::VertexId>| json!(e.loop_points(l).into_iter().map(out_pt).collect::<Vec<_>>());
            Ok(json!({
                "outer": ring(&f.outer),
                "holes": f.holes.iter().map(ring).collect::<Vec<_>>(),
                "normal": [f.normal.x, f.normal.y, f.normal.z],
                "area": area(e, id),
                "front": mat_name(c.model, f.front), "back": mat_name(c.model, f.back),
                "hidden": f.hidden, "layer": f.layer,
                "edges": e.face_edges(id).into_iter().map(|x| handle('e', h.ctx, x.data().as_ffi())).collect::<Vec<_>>(),
            }))
        }
        "face.pushpull" => {
            let h = parse_h(str_of(a, "h")?)?;
            let id = FaceId::from(KeyData::from_ffi(h.id));
            let d = a["d"].as_f64().ok_or("distance attendue")? * INCH;
            let e = c.model.entities_mut(h.ctx);
            if !e.faces.contains_key(id) {
                return Err("face supprimée".into());
            }
            let keep = a.get("keep").and_then(J::as_bool).unwrap_or(false);
            let r = e.push_pull(id, d, keep);
            // The API keeps the copied face as it was, facing the pull (TestUp
            // `test_pushpull_copy_face_direction` : SketchUp 2021.1's tool turns it, not the API).
            if keep && d > 0.0 && r.is_some() && e.faces.contains_key(id) {
                e.reverse_face(id);
            }
            Ok(match r {
                Some(f) => json!(handle('f', h.ctx, f.data().as_ffi())),
                None => J::Null,
            })
        }
        "face.reverse" => {
            let h = parse_h(str_of(a, "h")?)?;
            let id = FaceId::from(KeyData::from_ffi(h.id));
            let e = c.model.entities_mut(h.ctx);
            if !e.faces.contains_key(id) {
                return Err("face supprimée".into());
            }
            e.flip_face(id);
            Ok(J::Null)
        }
        "face.set" => {
            let h = parse_h(str_of(a, "h")?)?;
            let id = FaceId::from(KeyData::from_ffi(h.id));
            let mat = |m: &mut Model, key: &str| -> Result<Option<Option<MaterialId>>, String> {
                match a.get(key) {
                    None => Ok(None),
                    Some(J::Null) => Ok(Some(None)),
                    Some(v) => {
                        let n = v.as_str().ok_or("nom de matière attendu")?;
                        Ok(Some(Some(m.find_material(n).ok_or_else(|| format!("matière inconnue: {n}"))?)))
                    }
                }
            };
            let front = mat(c.model, "front")?;
            let back = mat(c.model, "back")?;
            let f = c.model.entities_mut(h.ctx).faces.get_mut(id).ok_or("face supprimée")?;
            if let Some(m) = front { f.front = m; }
            if let Some(m) = back { f.back = m; }
            if let Some(v) = a.get("hidden").and_then(J::as_bool) { f.hidden = v; }
            Ok(J::Null)
        }
        "inst.info" => {
            let h = parse_h(str_of(a, "h")?)?;
            let id = InstanceId::from(KeyData::from_ffi(h.id));
            let i = c.model.entities(h.ctx).instances.get(id).ok_or("instance supprimée")?;
            let d = &c.model.defs[i.def];
            Ok(json!({
                "def": handle('d', None, i.def.data().as_ffi()),
                "defctx": def_str(Some(i.def)),
                "name": i.name, "defname": d.name, "is_group": d.is_group,
                "xform": mat_to(&i.xform), "layer": i.layer, "hidden": i.hidden, "locked": i.locked,
                "material": mat_name(c.model, i.material),
            }))
        }
        "inst.set" => {
            let h = parse_h(str_of(a, "h")?)?;
            let id = InstanceId::from(KeyData::from_ffi(h.id));
            let material = match a.get("material") {
                None => None,
                Some(J::Null) => Some(None),
                Some(v) => {
                    let n = v.as_str().ok_or("nom de matière attendu")?;
                    Some(Some(c.model.find_material(n).ok_or_else(|| format!("matière inconnue: {n}"))?))
                }
            };
            let xform = a.get("xform").filter(|m| !m.is_null()).map(mat_from).transpose()?;
            let i = c.model.entities_mut(h.ctx).instances.get_mut(id).ok_or("instance supprimée")?;
            if let Some(n) = a.get("name").and_then(J::as_str) { i.name = n.into(); }
            if let Some(m) = xform { i.xform = m; }
            if let Some(v) = a.get("hidden").and_then(J::as_bool) { i.hidden = v; }
            if let Some(v) = a.get("locked").and_then(J::as_bool) { i.locked = v; }
            if let Some(m) = material { i.material = m; }
            Ok(J::Null)
        }
        "inst.explode" => {
            let h = parse_h(str_of(a, "h")?)?;
            let id = InstanceId::from(KeyData::from_ffi(h.id));
            let out = c.model.explode(h.ctx, id);
            Ok(json!(out.into_iter().filter_map(|e| eref_handle(h.ctx, e)).collect::<Vec<_>>()))
        }
        "def.list" => Ok(json!(
            c.model
                .defs
                .iter()
                .filter(|(_, d)| !d.is_group)
                .map(|(id, d)| json!({"h": handle('d', None, id.data().as_ffi()), "ctx": def_str(Some(id)), "name": d.name}))
                .collect::<Vec<_>>()
        )),
        "def.add" => {
            let name = c.model.unique_def_name(str_of(a, "name")?);
            let id = c.model.defs.insert(esq_core::model::Definition {
                name: name.clone(),
                is_group: false,
                description: String::new(),
                entities: Entities::default(),
                attributes: Default::default(),
                ext: Default::default(),
            });
            Ok(json!({"h": handle('d', None, id.data().as_ffi()), "ctx": def_str(Some(id)), "name": name}))
        }
        _ => Err(format!("opération inconnue: {op}")),
    }
}

pub(crate) use model::{begin_run, finish_run, take_ops};
