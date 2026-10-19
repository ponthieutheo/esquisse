//! Native side of groups, components, definitions and attribute dictionaries.
//!
//! Errors that Ruby must raise as a particular exception travel as `!<Kind>|<message>`
//! (`Arg`, `Type`, `Runtime`, `Index`, `IO`, `Gone`); `Esq.icall` in `42_instances.rb`
//! turns them into the right exception class.

use std::collections::{BTreeMap, HashSet};

use esq_core::model::{
    AttrValue, Attributes, Behavior, DefExt, DefId, Entities, EntityRef, FaceId, GlueRef, Instance, InstanceId, Model,
    set_attr,
};
use esq_core::solid::SolidOp;
use glam::{DMat4, DVec3};
use serde_json::{Value as J, json};
use slotmap::{Key, KeyData};

use super::{Ctx, INCH, R, def_str, eref_handle, handle, mat_from, mat_name, parse_h, pt, str_of};

fn err(kind: &str, msg: &str) -> String {
    format!("!{kind}|{msg}")
}

fn gone() -> String {
    err("Gone", "")
}

fn def_key(id: u64) -> DefId {
    DefId::from(KeyData::from_ffi(id))
}

/// The collection `ctx`, or "gone" when its definition no longer exists.
fn ents(m: &Model, ctx: Option<DefId>) -> Result<&Entities, String> {
    match ctx {
        Some(d) => m.defs.get(d).map(|d| &d.entities).ok_or_else(gone),
        None => Ok(&m.root),
    }
}

fn ents_mut(m: &mut Model, ctx: Option<DefId>) -> Result<&mut Entities, String> {
    match ctx {
        Some(d) => m.defs.get_mut(d).map(|d| &mut d.entities).ok_or_else(gone),
        None => Ok(&mut m.root),
    }
}

/// `(collection, instance)` of an instance handle.
fn inst_ref(a: &J, key: &str) -> Result<(Option<DefId>, InstanceId), String> {
    let h = parse_h(str_of(a, key)?)?;
    if h.kind != 'i' {
        return Err(err("Type", "wrong argument type (expected instance)"));
    }
    Ok((h.ctx, InstanceId::from(KeyData::from_ffi(h.id))))
}

fn get_inst(m: &Model, ctx: Option<DefId>, id: InstanceId) -> Result<&Instance, String> {
    ents(m, ctx)?.instances.get(id).ok_or_else(gone)
}

fn get_inst_mut(m: &mut Model, ctx: Option<DefId>, id: InstanceId) -> Result<&mut Instance, String> {
    ents_mut(m, ctx)?.instances.get_mut(id).ok_or_else(gone)
}

/// The definition named by the `ctx` argument (its id as text).
fn def_ref(m: &Model, a: &J, key: &str) -> Result<DefId, String> {
    let s = str_of(a, key)?;
    let id = s.parse::<u64>().map_err(|_| format!("contexte invalide: {s}"))?;
    let d = def_key(id);
    if m.defs.contains_key(d) { Ok(d) } else { Err(gone()) }
}

fn set_behavior(bh: &mut Behavior, b: &J) {
    for (k, f) in [
        ("always_face_camera", &mut bh.always_face_camera),
        ("cuts_opening", &mut bh.cuts_opening),
        ("is2d", &mut bh.is2d),
        ("shadows_face_sun", &mut bh.shadows_face_sun),
    ] {
        if let Some(v) = b.get(k).and_then(J::as_bool) {
            *f = v;
        }
    }
    if let Some(v) = b.get("no_scale_mask").and_then(J::as_i64) {
        bh.no_scale_mask = v as i32;
    }
    if let Some(v) = b.get("snapto").and_then(J::as_i64) {
        bh.snapto = v as i32;
    }
}

fn behavior_json(b: &Behavior) -> J {
    json!({
        "always_face_camera": b.always_face_camera, "cuts_opening": b.cuts_opening, "is2d": b.is2d,
        "shadows_face_sun": b.shadows_face_sun, "no_scale_mask": b.no_scale_mask, "snapto": b.snapto,
    })
}

/// Inches back from millimetres, without the 1e-16 noise of `x * 25.4 / 25.4`.
fn clean(x: f64) -> f64 {
    (x * 1e12).round() / 1e12
}

fn pt_out(p: DVec3) -> J {
    json!([clean(p.x / INCH), clean(p.y / INCH), clean(p.z / INCH)])
}

fn mat_out(m: &DMat4) -> J {
    let mut v = m.to_cols_array();
    for (i, x) in v.iter_mut().enumerate() {
        *x = clean(if (12..15).contains(&i) { *x / INCH } else { *x });
    }
    json!(v)
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// Number of times instances of `def` appear in what the model actually shows.
/// An instance inside a definition counts once per placement of that definition (SketchUp's
/// `count_used_instances` walks the whole hierarchy).
fn used_instances(m: &Model, def: DefId, depth: usize) -> usize {
    if depth > 64 {
        return 0;
    }
    let mut n = m.root.instances.values().filter(|i| i.def == def).count();
    for (pid, p) in m.defs.iter() {
        let here = p.entities.instances.values().filter(|i| i.def == def).count();
        if here > 0 {
            n += here * used_instances(m, pid, depth + 1);
        }
    }
    n
}

/// Handles of every instance of `def`, wherever it sits.
fn instances_of(m: &Model, def: DefId) -> Vec<String> {
    let mut out: Vec<String> = m
        .root
        .instances
        .iter()
        .filter(|(_, i)| i.def == def)
        .map(|(id, _)| handle('i', None, id.data().as_ffi()))
        .collect();
    for (cid, d) in m.defs.iter() {
        out.extend(
            d.entities
                .instances
                .iter()
                .filter(|(_, i)| i.def == def)
                .map(|(id, _)| handle('i', Some(cid), id.data().as_ffi())),
        );
    }
    out
}

fn extend_bounds(m: &Model, e: &Entities, xf: &DMat4, lo: &mut DVec3, hi: &mut DVec3, depth: usize) {
    for v in e.verts.values() {
        let p = xf.transform_point3(v.pos);
        *lo = lo.min(p);
        *hi = hi.max(p);
    }
    if depth > 32 {
        return;
    }
    for i in e.instances.values() {
        if let Some(d) = m.defs.get(i.def) {
            extend_bounds(m, &d.entities, &(*xf * i.xform), lo, hi, depth + 1);
        }
    }
}

fn translate_entities(e: &mut Entities, by: DVec3) {
    let t = DMat4::from_translation(by);
    let all: HashSet<_> = e.verts.keys().collect();
    e.transform_vertices(&all, &t);
    for i in e.instances.values_mut() {
        i.xform = t * i.xform;
    }
    for g in e.guides.values_mut() {
        *g = g.transformed(&t);
    }
    for a in e.annots.values_mut() {
        *a = a.transformed(&t);
    }
    for s in e.sections.values_mut() {
        *s = s.transformed(&t);
    }
    for s in e.snaps.iter_mut() {
        *s = s.transformed(&t);
    }
}

// ---------------------------------------------------------------------------------------
// Attribute dictionaries

/// The dictionaries of an owner: `i:ctx:id` an instance, `d:0:id` a definition, anything
/// else a key of the model's side table.
fn attrs_ref<'m>(m: &'m Model, owner: &str) -> Result<Option<&'m Attributes>, String> {
    owner_alive(m, owner)?;
    if owner.starts_with("i:") || owner.starts_with("d:") {
        let h = parse_h(owner)?;
        return match h.kind {
            'i' => Ok(Some(&get_inst(m, h.ctx, InstanceId::from(KeyData::from_ffi(h.id)))?.attributes)),
            _ => Ok(Some(&m.defs.get(def_key(h.id)).ok_or_else(gone)?.attributes)),
        };
    }
    Ok(m.attr_store.get(owner))
}

/// The dictionaries of an erased entity are gone with it (`AttributeDictionary#valid?` is
/// false, reading them raises): the side table is keyed by the entity's handle (`h:<handle>`).
fn owner_alive(m: &Model, owner: &str) -> Result<(), String> {
    let Some(h) = owner.strip_prefix("h:").and_then(|h| parse_h(h).ok()) else { return Ok(()) };
    if "efgasncvp".contains(h.kind) && !super::entities::entity_exists(m, &h) { Err(gone()) } else { Ok(()) }
}

fn attrs_mut<'m>(m: &'m mut Model, owner: &str) -> Result<&'m mut Attributes, String> {
    owner_alive(m, owner)?;
    if owner.starts_with("i:") || owner.starts_with("d:") {
        let h = parse_h(owner)?;
        return match h.kind {
            'i' => Ok(&mut get_inst_mut(m, h.ctx, InstanceId::from(KeyData::from_ffi(h.id)))?.attributes),
            _ => Ok(&mut m.defs.get_mut(def_key(h.id)).ok_or_else(gone)?.attributes),
        };
    }
    Ok(m.attr_store.entry(owner.to_string()).or_default())
}

/// Drops an empty side-table entry.
fn tidy_store(m: &mut Model, owner: &str) {
    if m.attr_store.get(owner).is_some_and(Attributes::is_empty) {
        m.attr_store.remove(owner);
    }
}

fn val_in(v: &J) -> AttrValue {
    let inner = &v["v"];
    match v.get("t").and_then(J::as_str) {
        Some("s") if inner.is_string() => AttrValue::Text(inner.as_str().unwrap_or_default().to_string()),
        Some("f") if inner.is_number() => AttrValue::Number(inner.as_f64().unwrap_or_default()),
        Some("b") if inner.is_boolean() => AttrValue::Bool(inner.as_bool().unwrap_or_default()),
        _ => AttrValue::Json(v.clone()),
    }
}

fn val_out(v: &AttrValue) -> J {
    match v {
        AttrValue::Text(s) => json!({"t": "s", "v": s}),
        AttrValue::Number(n) => json!({"t": "f", "v": n}),
        AttrValue::Bool(b) => json!({"t": "b", "v": b}),
        AttrValue::Json(j) => j.clone(),
    }
}

fn attr_op(c: &mut Ctx, op: &str, a: &J) -> R {
    let owner = str_of(a, "owner")?;
    let dict = a.get("dict").and_then(J::as_str).unwrap_or_default();
    let key = a.get("key").and_then(J::as_str).unwrap_or_default();
    match op {
        "attr.names" => Ok(match attrs_ref(c.model, owner)? {
            Some(at) if !at.is_empty() => json!(at.keys().collect::<Vec<_>>()),
            _ => J::Null,
        }),
        "attr.dict" => Ok(match attrs_ref(c.model, owner)?.and_then(|at| at.get(dict)) {
            Some(d) => json!(d.iter().map(|(k, v)| json!([k, val_out(v)])).collect::<Vec<_>>()),
            None => J::Null,
        }),
        "attr.get" => Ok(match attrs_ref(c.model, owner)?.and_then(|at| at.get(dict)).and_then(|d| d.get(key)) {
            Some(v) => json!({"found": true, "v": val_out(v)}),
            None => json!({"found": false}),
        }),
        "attr.set" => {
            let v = val_in(&a["v"]);
            set_attr(attrs_mut(c.model, owner)?, dict, key, v);
            Ok(J::Null)
        }
        "attr.create" => {
            let at = attrs_mut(c.model, owner)?;
            let fresh = !at.contains_key(dict);
            at.entry(dict.to_string()).or_default();
            Ok(json!(fresh))
        }
        "attr.delete_key" => {
            let prune = a.get("prune").and_then(J::as_bool).unwrap_or(false);
            let at = attrs_mut(c.model, owner)?;
            let removed = at.get_mut(dict).and_then(|d| d.remove(key));
            if prune && at.get(dict).is_some_and(BTreeMap::is_empty) {
                at.shift_remove(dict);
            }
            tidy_store(c.model, owner);
            Ok(match removed {
                Some(v) => json!({"found": true, "v": val_out(&v)}),
                None => json!({"found": false}),
            })
        }
        "attr.delete_dict" => {
            let at = attrs_mut(c.model, owner)?;
            let had = at.shift_remove(dict).is_some();
            tidy_store(c.model, owner);
            Ok(json!(had))
        }
        _ => Err(format!("opération inconnue: {op}")),
    }
}

// ---------------------------------------------------------------------------------------
// Groups and component instances

fn inst_info(c: &Ctx, a: &J) -> R {
    let (ctx, id) = inst_ref(a, "h")?;
    let i = get_inst(c.model, ctx, id)?;
    let d = c.model.defs.get(i.def).ok_or_else(gone)?;
    let glued = i.ext.glued_to.and_then(|g| match g {
        GlueRef::Face(f) => ents(c.model, ctx).ok()?.faces.contains_key(f).then(|| handle('f', ctx, f.data().as_ffi())),
        GlueRef::Instance(x) => {
            ents(c.model, ctx).ok()?.instances.contains_key(x).then(|| handle('i', ctx, x.data().as_ffi()))
        }
    });
    Ok(json!({
        "def": handle('d', None, i.def.data().as_ffi()),
        "defctx": def_str(Some(i.def)),
        "name": i.name, "defname": d.name, "is_group": d.is_group,
        "xform": mat_out(&i.xform), "layer": i.layer, "hidden": i.hidden, "locked": i.locked,
        "material": mat_name(c.model, i.material),
        "glued": glued, "hatch": i.ext.hatch,
        "count": c.model.instance_count(i.def),
        "guid": i.ext.guid,
    }))
}

fn inst_set(c: &mut Ctx, a: &J) -> R {
    let (ctx, id) = inst_ref(a, "h")?;
    let material = match a.get("material") {
        None => None,
        Some(J::Null) => Some(None),
        Some(v) => {
            let n = v.as_str().ok_or("nom de matière attendu")?;
            Some(Some(c.model.find_material(n).ok_or_else(|| format!("matière inconnue: {n}"))?))
        }
    };
    let xform = a.get("xform").filter(|m| !m.is_null()).map(mat_from).transpose()?;
    let new_def = match a.get("defctx").and_then(J::as_str) {
        Some(s) => {
            let d = def_key(s.parse::<u64>().map_err(|_| format!("contexte invalide: {s}"))?);
            if !c.model.defs.contains_key(d) {
                return Err(gone());
            }
            Some(d)
        }
        None => None,
    };
    let i = get_inst_mut(c.model, ctx, id)?;
    if let Some(n) = a.get("name").and_then(J::as_str) {
        i.name = n.into();
    }
    if let Some(m) = xform {
        i.xform = m;
    }
    if let Some(v) = a.get("hidden").and_then(J::as_bool) {
        i.hidden = v;
    }
    if let Some(v) = a.get("locked").and_then(J::as_bool) {
        i.locked = v;
    }
    if let Some(m) = material {
        i.material = m;
    }
    if let Some(d) = new_def {
        i.def = d;
    }
    match a.get("hatch") {
        None => {}
        Some(J::Null) => i.ext.hatch = None,
        Some(v) => i.ext.hatch = v.as_str().map(str::to_string),
    }
    Ok(J::Null)
}

fn inst_glue(c: &mut Ctx, a: &J) -> R {
    let (ctx, id) = inst_ref(a, "h")?;
    let me = get_inst(c.model, ctx, id)?.clone();
    let target = a.get("to").and_then(J::as_str);
    let Some(t) = target else {
        get_inst_mut(c.model, ctx, id)?.ext.glued_to = None;
        return Ok(J::Null);
    };
    let h = parse_h(t)?;
    let key = KeyData::from_ffi(h.id);
    let glue = match h.kind {
        'f' => GlueRef::Face(FaceId::from(key)),
        'i' => GlueRef::Instance(InstanceId::from(key)),
        _ => {
            return Err(err(
                "Arg",
                "wrong type - expected Sketchup::Face, Sketchup::Group, SketchUp::Image or Sketchup::ComponentInstance",
            ));
        }
    };
    if h.ctx != ctx {
        return Err(err("Arg", "Can only glue to something in the same component"));
    }
    let here = ents(c.model, ctx)?;
    match glue {
        GlueRef::Face(f) if !here.faces.contains_key(f) => return Err(err("Arg", "The instance parent is not valid")),
        GlueRef::Instance(x) => {
            if !here.instances.contains_key(x) {
                return Err(err("Arg", "The instance parent is not valid"));
            }
            // Gluing to itself, or to something that is glued (however indirectly) to it.
            let mut cur = Some(x);
            let mut guard = 0;
            while let Some(k) = cur {
                if k == id {
                    return Err(err("Arg", "Can't create cyclic gluing"));
                }
                cur = match here.instances.get(k).and_then(|i| i.ext.glued_to) {
                    Some(GlueRef::Instance(n)) => Some(n),
                    _ => None,
                };
                guard += 1;
                if guard > 1000 {
                    break;
                }
            }
        }
        GlueRef::Face(_) => {}
    }
    let d = c.model.defs.get(me.def).ok_or_else(gone)?;
    if !d.is_group && !d.ext.behavior.is2d {
        return Err(err("Arg", "The definition does not have glueing behavior"));
    }
    if let (GlueRef::Face(f), true) = (glue, !d.is_group) {
        let n = here.faces[f].normal;
        let ok = match d.ext.behavior.snapto {
            1 => n.z.abs() > 1.0 - 1e-6,
            2 => n.z.abs() < 1e-6,
            3 => n.z.abs() <= 1.0 - 1e-6 && n.z.abs() >= 1e-6,
            _ => true,
        };
        if !ok {
            return Err(err("Arg", "Alignment is wrong"));
        }
    }
    get_inst_mut(c.model, ctx, id)?.ext.glued_to = Some(glue);
    Ok(J::Null)
}

fn solid_op(c: &mut Ctx, a: &J) -> R {
    let (c1, i1) = inst_ref(a, "a")?;
    let (c2, i2) = inst_ref(a, "b")?;
    get_inst(c.model, c1, i1)?;
    get_inst(c.model, c2, i2)?;
    if c1 != c2 {
        return Ok(J::Null);
    }
    let op = match str_of(a, "op")? {
        "union" => SolidOp::Union,
        "intersect" => SolidOp::Intersect,
        "subtract" => SolidOp::Subtract,
        "trim" => SolidOp::Trim,
        "split" => SolidOp::Split,
        "outer_shell" => {
            return Ok(match c.model.outer_shell(c1, &[i1, i2]) {
                Ok(n) => json!([handle('i', c1, n.data().as_ffi())]),
                Err(_) => J::Null,
            });
        }
        o => return Err(format!("opération solide inconnue: {o}")),
    };
    Ok(match c.model.solid_op(c1, i1, i2, op) {
        Ok(made) => json!(made.into_iter().map(|n| handle('i', c1, n.data().as_ffi())).collect::<Vec<_>>()),
        Err(_) => J::Null,
    })
}

/// Same shape in both: same vertices (to a micron), same number of edges and faces.
fn same_geometry(m: &Model, a: DefId, b: DefId) -> bool {
    if a == b {
        return true;
    }
    let (ea, eb) = (&m.defs[a].entities, &m.defs[b].entities);
    if ea.edges.len() != eb.edges.len() || ea.faces.len() != eb.faces.len() || ea.instances.len() != eb.instances.len()
    {
        return false;
    }
    let key = |e: &Entities| {
        let mut v: Vec<[i64; 3]> = e
            .verts
            .values()
            .map(|v| [(v.pos.x * 1e3).round() as i64, (v.pos.y * 1e3).round() as i64, (v.pos.z * 1e3).round() as i64])
            .collect();
        v.sort();
        v
    };
    key(ea) == key(eb)
}

fn inst_op(c: &mut Ctx, op: &str, a: &J) -> R {
    match op {
        "inst.info" => inst_info(c, a),
        "inst.set" => inst_set(c, a),
        "inst.glue" => inst_glue(c, a),
        "inst.copy" => {
            let (ctx, id) = inst_ref(a, "h")?;
            let mut copy = get_inst(c.model, ctx, id)?.clone();
            // A copy is a new instance: SketchUp gives it a GUID of its own.
            copy.ext.guid.clear();
            let n = ents_mut(c.model, ctx)?.instances.insert(copy);
            Ok(json!(handle('i', ctx, n.data().as_ffi())))
        }
        "inst.make_unique" => {
            let (ctx, id) = inst_ref(a, "h")?;
            get_inst(c.model, ctx, id)?;
            let d = c.model.make_unique(ctx, id);
            Ok(json!(def_str(Some(d))))
        }
        "inst.explode" => {
            let (ctx, id) = inst_ref(a, "h")?;
            if get_inst(c.model, ctx, id)?.locked {
                return Ok(J::Null);
            }
            // What the explosion brought out, snaps and spaces included.
            let out = c.model.explode(ctx, id);
            Ok(json!(out.into_iter().filter_map(|e| eref_handle(ctx, e)).collect::<Vec<_>>()))
        }
        "inst.to_component" => {
            let (ctx, id) = inst_ref(a, "h")?;
            let mut inst = get_inst(c.model, ctx, id)?.clone();
            let mut def = inst.def;
            if c.model.instance_count(def) > 1 {
                let copy = c.model.defs[def].clone();
                def = c.model.defs.insert(copy);
            }
            let name = c.model.unique_def_name("Composant");
            let d = &mut c.model.defs[def];
            d.is_group = false;
            d.name = name;
            inst.def = def;
            inst.ext.glued_to = None;
            let e = ents_mut(c.model, ctx)?;
            e.instances.remove(id);
            let n = e.instances.insert(inst);
            Ok(json!(handle('i', ctx, n.data().as_ffi())))
        }
        "inst.volume" => {
            let (ctx, id) = inst_ref(a, "h")?;
            get_inst(c.model, ctx, id)?;
            Ok(json!(c.model.solid_volume(ctx, id).map_or(-1.0, |v| v / (INCH * INCH * INCH))))
        }
        "inst.manifold" => {
            let (ctx, id) = inst_ref(a, "h")?;
            get_inst(c.model, ctx, id)?;
            Ok(json!(c.model.is_solid_instance(ctx, id)))
        }
        "inst.local_bounds" => {
            let (ctx, id) = inst_ref(a, "h")?;
            let i = get_inst(c.model, ctx, id)?;
            let d = c.model.defs.get(i.def).ok_or_else(gone)?;
            let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
            extend_bounds(c.model, &d.entities, &DMat4::IDENTITY, &mut lo, &mut hi, 0);
            Ok(if lo.is_finite() { json!([pt_out(lo), pt_out(hi)]) } else { J::Null })
        }
        "inst.same_geometry" => {
            let (c1, i1) = inst_ref(a, "a")?;
            let (c2, i2) = inst_ref(a, "b")?;
            let (d1, d2) = (get_inst(c.model, c1, i1)?.def, get_inst(c.model, c2, i2)?.def);
            Ok(json!(same_geometry(c.model, d1, d2)))
        }
        "inst.solid" => solid_op(c, a),
        _ => Err(format!("opération inconnue: {op}")),
    }
}

// ---------------------------------------------------------------------------------------
// Definitions

/// `Model#behavior`: the behaviour of the model itself (context "0", Model Info › File).
fn is_model_ctx(a: &J) -> bool {
    a.get("ctx").and_then(J::as_str) == Some("0")
}

fn def_info(c: &Ctx, a: &J) -> R {
    if is_model_ctx(a) {
        return Ok(json!({"behavior": behavior_json(&c.model.behavior())}));
    }
    let d = def_ref(c.model, a, "ctx")?;
    let def = &c.model.defs[d];
    Ok(json!({
        "name": def.name, "description": def.description, "is_group": def.is_group,
        "is_image": c.model.is_image_def(d),
        "count": c.model.instance_count(d), "used": used_instances(c.model, d, 0),
        "behavior": behavior_json(&def.ext.behavior),
        "thumb": def.ext.thumbnail_camera.map(|t| json!([pt_out(t[0]), pt_out(t[1]), pt_out(t[2])])),
        "path": def.ext.path, "loaded_at": def.ext.loaded_at, "guid": def.ext.guid,
    }))
}

fn new_def(m: &mut Model, name: &str, is_group: bool) -> DefId {
    let name = if m.defs.values().any(|d| d.name == name) { m.unique_def_name(name) } else { name.to_string() };
    m.defs.insert(esq_core::model::Definition {
        name,
        is_group,
        description: String::new(),
        entities: Entities::default(),
        attributes: Default::default(),
        ext: DefExt { loaded_at: now(), ..Default::default() },
    })
}

fn load_file(path: &str) -> Result<Model, String> {
    let p = std::path::Path::new(path);
    if !p.is_file() {
        return Err(err("IO", "Invalid component file"));
    }
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    let loaded = match ext.as_str() {
        "skp" => esq_skp::import_skp(p),
        "esq" => esq_core::io::load_esq(p),
        _ => return Err(err("IO", "Invalid component file")),
    };
    loaded.map_err(|_| err("IO", "Invalid component file"))
}

fn def_op(c: &mut Ctx, op: &str, a: &J) -> R {
    match op {
        "defs.all" => Ok(json!(
            c.model
                .defs
                .iter()
                .map(|(id, d)| json!({"ctx": def_str(Some(id)), "name": d.name, "is_group": d.is_group}))
                .collect::<Vec<_>>()
        )),
        "def.info" => def_info(c, a),
        "def.set" if is_model_ctx(a) => {
            let mut bh = c.model.behavior();
            if let Some(b) = a.get("behavior") {
                set_behavior(&mut bh, b);
            }
            c.model.set_behavior(bh);
            Ok(J::Null)
        }
        "def.set" => {
            let d = def_ref(c.model, a, "ctx")?;
            if let Some(n) = a.get("name").and_then(J::as_str) {
                let name = if c.model.defs.iter().any(|(k, x)| k != d && x.name == n) {
                    c.model.unique_def_name(n)
                } else {
                    n.to_string()
                };
                c.model.defs[d].name = name;
            }
            let def = &mut c.model.defs[d];
            if let Some(n) = a.get("description").and_then(J::as_str) {
                def.description = n.into();
            }
            if let Some(b) = a.get("behavior") {
                set_behavior(&mut def.ext.behavior, b);
            }
            match a.get("thumb") {
                None => {}
                Some(J::Null) => def.ext.thumbnail_camera = None,
                Some(t) => {
                    let p = |i: usize| pt(&t[i]).map(|v| v / INCH);
                    def.ext.thumbnail_camera = Some([p(0)?, p(1)?, p(2)?]);
                }
            }
            if let Some(p) = a.get("path").and_then(J::as_str) {
                def.ext.path = p.into();
            }
            Ok(J::Null)
        }
        "defs.add" => {
            let name = a.get("name").and_then(J::as_str).unwrap_or("Composant").to_string();
            let d = new_def(c.model, &name, false);
            Ok(json!(def_str(Some(d))))
        }
        "defs.unique_name" => {
            let n = str_of(a, "name")?;
            Ok(json!(if c.model.defs.values().any(|d| d.name == n) {
                c.model.unique_def_name(n)
            } else {
                n.to_string()
            }))
        }
        "defs.purge" => {
            c.model.purge_unused_defs();
            Ok(J::Null)
        }
        "defs.remove" => {
            let d = def_ref(c.model, a, "ctx")?;
            // Its instances go with it.
            let mut homes: Vec<Option<DefId>> = vec![None];
            homes.extend(c.model.defs.keys().map(Some));
            for home in homes {
                let e = ents_mut(c.model, home)?;
                let doomed: Vec<EntityRef> =
                    e.instances.iter().filter(|(_, i)| i.def == d).map(|(k, _)| EntityRef::Instance(k)).collect();
                if !doomed.is_empty() {
                    e.erase(&doomed);
                }
            }
            c.model.defs.remove(d);
            Ok(J::Null)
        }
        "def.manifold" => {
            let d = def_ref(c.model, a, "ctx")?;
            Ok(json!(esq_core::solid::is_solid(&c.model.defs[d].entities)))
        }
        "def.instances" => {
            let d = def_ref(c.model, a, "ctx")?;
            Ok(json!(instances_of(c.model, d)))
        }
        "def.set_origin" => {
            let d = def_ref(c.model, a, "ctx")?;
            let p = pt(&a["p"])?;
            translate_entities(&mut c.model.defs[d].entities, -p);
            let t = DMat4::from_translation(p);
            let mut homes: Vec<Option<DefId>> = vec![None];
            homes.extend(c.model.defs.keys().map(Some));
            for home in homes {
                for i in ents_mut(c.model, home)?.instances.values_mut().filter(|i| i.def == d) {
                    i.xform *= t;
                }
            }
            Ok(J::Null)
        }
        "defs.load" => {
            let path = str_of(a, "path")?;
            let src = load_file(path)?;
            let stem = std::path::Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("Composant");
            let d = c.model.import_as_component(&src, stem);
            let def = &mut c.model.defs[d];
            def.ext.path = path.into();
            def.ext.loaded_at = now();
            Ok(json!(def_str(Some(d))))
        }
        "def.save" => {
            let d = def_ref(c.model, a, "ctx")?;
            let path = std::path::Path::new(str_of(a, "path")?);
            let mut out = Model::new();
            let mut map = esq_core::groups::ImportMap::default();
            let root = out.import_entities(c.model, &c.model.defs[d].entities, &mut map);
            out.root = root;
            out.rebuild_topology();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
            // `save_as(path, version)`: a `Sketchup::Model::VERSION_…` (the current format by default).
            let version = match a.get("version").filter(|v| !v.is_null()) {
                None => None,
                Some(v) => Some(v.as_i64().and_then(esq_skp::SkpVersion::from_ruby).ok_or("version inconnue")?),
            };
            let ok = match ext.as_str() {
                "skp" => esq_skp::export_skp_version(&out, path, None, version).is_ok(),
                _ => esq_core::io::save_esq(&out, path).is_ok(),
            };
            if ok && a.get("relink").and_then(J::as_bool).unwrap_or(false) {
                c.model.defs[d].ext.path = path.to_string_lossy().into_owned();
            }
            Ok(json!(ok))
        }
        "def.merge" => {
            // Dissolves the edges between coplanar faces among `hs` (everything when absent).
            let d = def_ref(c.model, a, "ctx")?;
            let e = &mut c.model.defs[d].entities;
            let chosen: Option<HashSet<_>> = match a.get("hs").and_then(J::as_array) {
                None => None,
                Some(list) => {
                    let mut s = HashSet::new();
                    for h in list {
                        let h = parse_h(h.as_str().ok_or("poignée attendue")?)?;
                        if h.kind == 'e' {
                            s.insert(esq_core::model::EdgeId::from(KeyData::from_ffi(h.id)));
                        }
                    }
                    Some(s)
                }
            };
            let candidates: Vec<_> = e
                .edges
                .iter()
                .filter(|(k, x)| {
                    x.faces.len() == 2 && chosen.as_ref().is_none_or(|s| s.contains(k)) && {
                        let (p, q) = (e.face_plane(x.faces[0]), e.face_plane(x.faces[1]));
                        p.same_as(&q)
                    }
                })
                .map(|(k, _)| k)
                .collect();
            let n = candidates.len();
            e.erase_edges(&candidates);
            Ok(json!(n > 0 || chosen.is_none()))
        }
        _ => Err(format!("opération inconnue: {op}")),
    }
}

pub fn dispatch(c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    Some(match op {
        // Only definitions are mine; edges, faces and instances stay with the core.
        "valid" => {
            let h = str_of(a, "h").ok().and_then(|s| parse_h(s).ok())?;
            if h.kind != 'd' {
                return None;
            }
            Ok(json!(c.model.defs.contains_key(def_key(h.id))))
        }
        o if o.starts_with("attr.") => attr_op(c, o, a),
        "inst.info" | "inst.set" | "inst.glue" | "inst.copy" | "inst.make_unique" | "inst.explode"
        | "inst.to_component" | "inst.volume" | "inst.manifold" | "inst.local_bounds" | "inst.same_geometry"
        | "inst.solid" => inst_op(c, op, a),
        "defs.all" | "def.info" | "def.set" | "defs.add" | "defs.unique_name" | "defs.purge" | "defs.remove"
        | "def.instances" | "def.manifold" | "def.set_origin" | "defs.load" | "def.save" | "def.merge" => {
            def_op(c, op, a)
        }
        _ => return None,
    })
}
