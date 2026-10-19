//! Native side of the entity classes (Edge, Face, Vertex, Curve, construction geometry, texts,
//! dimensions, section planes, Entities…). Errors that Ruby must raise as a given exception
//! class start with `"ClassName: "` (see `Esq.call`).
//!
//! Handles are `kind:ctx:id` with kind `e` edge, `f` face, `i` instance, `v` vertex,
//! `c` curve (id = the curve number carried by its edges), `g` guide (construction point or
//! line), `a` annotation (text or dimension), `s` section plane.

use std::collections::{HashMap, HashSet};

use glam::{DMat4, DVec2, DVec3};
use serde_json::{Value as J, json};
use slotmap::{Key, KeyData};

use esq_core::attach::Attach;
use esq_core::math::{Plane, TOL, basis_for, point_in_polygon_2d, project_on_segment};
use esq_core::model::{
    AnnotKind, Annotation, AttrValue, DefId, EdgeId, Entities, EntityRef, FaceId, Guide, InstanceId, Model,
    SectionPlane, VertexId,
};

use super::{Ctx, H, INCH, R, ctx_of, handle, inches, mat_from, mat_to, out_pt, parse_h, pt, pts, str_of};

fn ffi<K: Key>(k: K) -> u64 {
    k.data().as_ffi()
}

fn key<K: From<KeyData>>(id: u64) -> K {
    K::from(KeyData::from_ffi(id))
}

fn kind_name(k: char) -> &'static str {
    match k {
        'e' => "Edge",
        'f' => "Face",
        'i' => "ComponentInstance",
        'v' => "Vertex",
        'c' => "Curve",
        'g' => "ConstructionLine",
        'a' => "Text",
        's' => "SectionPlane",
        'n' => "Snap",
        'p' => "Space",
        _ => "Entity",
    }
}

/// Index of a snap (`n:ctx:id`) in its collection.
fn snap_ix(e: &Entities, id: u64) -> Option<usize> {
    e.snap_index(u32::try_from(id).ok()?)
}

/// Index of a space (`p:ctx:id`) in its collection.
fn space_ix(e: &Entities, id: u64) -> Option<usize> {
    e.space_index(u32::try_from(id).ok()?)
}

/// A space as Ruby sees it (inches): name, perimeter, holes, orientation point, ceiling.
fn space_json(s: &esq_core::model::Space) -> J {
    let [a, b, c, d] = s.ceiling;
    json!({
        "name": s.name,
        "perimeter": s.perimeter.iter().map(|p| out_pt(*p)).collect::<Vec<_>>(),
        "holes": s.holes.iter().map(|h| h.iter().map(|p| out_pt(*p)).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "orientation": out_pt(s.orientation),
        "ceiling": [a + 0.0, b + 0.0, c + 0.0, inches(d)],
    })
}

/// `space.add` / `space.set` (`Entities#add_space`, `Space#name=`, `#perimeter_points=`…).
fn space_op(c: &mut Ctx, op: &str, a: &J) -> R {
    let ceiling = |j: &J| -> Result<[f64; 4], String> {
        let v = j.as_array().filter(|v| v.len() == 4).ok_or("plan [a, b, c, d] attendu")?;
        let f = |i: usize| v[i].as_f64().ok_or("nombre attendu");
        Ok([f(0)?, f(1)?, f(2)?, f(3)? * INCH])
    };
    if op == "space.add" {
        let ctx = ctx_of(a)?;
        ents(c.model, ctx).ok_or("contexte supprimé")?;
        let mut s = esq_core::model::Space::new(str_of(a, "name")?, pts(&a["perimeter"])?, pt(&a["orientation"])?, 0.0);
        s.ceiling = ceiling(&a["ceiling"])?;
        let id = c.model.add_space(ctx, s);
        return Ok(json!(handle('p', ctx, u64::from(id))));
    }
    let h = h_of(a, "h")?;
    let en = ents_mut(c.model, h.ctx).ok_or_else(|| deleted('p'))?;
    let i = space_ix(en, h.id).ok_or_else(|| deleted('p'))?;
    let mut s = en.spaces[i].clone();
    if let Some(n) = a.get("name").and_then(J::as_str) {
        s.name = n.to_string();
    }
    if let Some(p) = a.get("perimeter") {
        s.perimeter = pts(p)?;
    }
    if let Some(p) = a.get("orientation") {
        s.orientation = pt(p)?;
    }
    if let Some(p) = a.get("ceiling") {
        s.ceiling = ceiling(p)?;
    }
    if let Some(p) = a.get("hole") {
        s.holes.push(pts(p)?);
    }
    en.spaces[i] = s;
    Ok(J::Null)
}

fn deleted(k: char) -> String {
    format!("TypeError: reference to deleted {}", kind_name(k))
}

fn arg_err(msg: &str) -> String {
    format!("ArgumentError: {msg}")
}

fn prop_key(k: char, id: u64) -> String {
    format!("{k}{id}")
}

fn av_json(v: &AttrValue) -> J {
    match v {
        AttrValue::Text(s) => json!(s),
        AttrValue::Number(n) => json!(n),
        AttrValue::Bool(b) => json!(b),
        AttrValue::Json(j) => j.clone(),
    }
}

fn json_av(j: &J) -> Option<AttrValue> {
    match j {
        J::String(s) => Some(AttrValue::Text(s.clone())),
        J::Number(n) => n.as_f64().map(AttrValue::Number),
        J::Bool(b) => Some(AttrValue::Bool(*b)),
        _ => None,
    }
}

fn props_json(e: &Entities, key: &str) -> J {
    let mut m = serde_json::Map::new();
    if let Some(p) = e.props.get(key) {
        for (k, v) in p {
            m.insert(k.clone(), av_json(v));
        }
    }
    J::Object(m)
}

/// The collection a handle lives in.
fn ents(m: &Model, ctx: Option<DefId>) -> Option<&Entities> {
    match ctx {
        Some(d) => m.defs.get(d).map(|d| &d.entities),
        None => Some(&m.root),
    }
}

fn ents_mut(m: &mut Model, ctx: Option<DefId>) -> Option<&mut Entities> {
    match ctx {
        Some(d) => m.defs.get_mut(d).map(|d| &mut d.entities),
        None => Some(&mut m.root),
    }
}

fn curve_edges(e: &Entities, id: u64) -> Vec<EdgeId> {
    let mut v: Vec<EdgeId> =
        e.edges.iter().filter(|(_, x)| x.curve.map(u64::from) == Some(id)).map(|(k, _)| k).collect();
    v.sort_by_key(|k| ffi(*k) & 0xffff_ffff);
    v
}

fn exists(m: &Model, h: &H) -> bool {
    let Some(e) = ents(m, h.ctx) else { return false };
    let k = KeyData::from_ffi(h.id);
    match h.kind {
        'e' => e.edges.contains_key(EdgeId::from(k)),
        'f' => e.faces.contains_key(FaceId::from(k)),
        'i' => e.instances.contains_key(InstanceId::from(k)),
        'v' => e.verts.contains_key(VertexId::from(k)),
        'g' => e.guides.contains_key(key(h.id)),
        'a' => e.annots.contains_key(key(h.id)),
        's' => e.sections.contains_key(key(h.id)),
        'c' => e.edges.values().any(|x| x.curve.map(u64::from) == Some(h.id)),
        'n' => snap_ix(e, h.id).is_some(),
        'p' => space_ix(e, h.id).is_some(),
        _ => false,
    }
}

/// Whether the entity a handle names still exists (for the other zones of the bridge).
pub(crate) fn entity_exists(m: &Model, h: &H) -> bool {
    exists(m, h)
}

fn need(m: &Model, h: &H) -> Result<(), String> {
    if exists(m, h) { Ok(()) } else { Err(deleted(h.kind)) }
}

fn eref_of(h: &H) -> Option<EntityRef> {
    let k = KeyData::from_ffi(h.id);
    Some(match h.kind {
        'e' => EntityRef::Edge(EdgeId::from(k)),
        'f' => EntityRef::Face(FaceId::from(k)),
        'i' => EntityRef::Instance(InstanceId::from(k)),
        'g' => EntityRef::Guide(key(h.id)),
        'a' => EntityRef::Annot(key(h.id)),
        's' => EntityRef::Section(key(h.id)),
        'n' => EntityRef::Snap(u32::try_from(h.id).ok()?),
        'p' => EntityRef::Space(u32::try_from(h.id).ok()?),
        _ => return None,
    })
}

fn hs(a: &J, k: &str) -> Result<Vec<H>, String> {
    a.get(k)
        .and_then(J::as_array)
        .ok_or_else(|| format!("argument « {k} » manquant"))?
        .iter()
        .map(|x| parse_h(x.as_str().ok_or("poignée attendue")?))
        .collect()
}

fn h_of(a: &J, k: &str) -> Result<H, String> {
    parse_h(str_of(a, k)?)
}

fn vec3(j: &J) -> Result<DVec3, String> {
    let a = j.as_array().filter(|a| a.len() >= 3).ok_or("vecteur attendu [x,y,z]")?;
    let f = |i: usize| a[i].as_f64().ok_or("nombre attendu");
    Ok(DVec3::new(f(0)?, f(1)?, f(2)?))
}

/// A vector that is a length (inches in Ruby, millimetres here).
fn len_vec(j: &J) -> Result<DVec3, String> {
    Ok(vec3(j)? * INCH)
}

fn out_vec(v: DVec3) -> J {
    // `+ 0.0` turns -0.0 into 0.0.
    json!([v.x + 0.0, v.y + 0.0, v.z + 0.0])
}

fn out_len_vec(v: DVec3) -> J {
    json!([inches(v.x), inches(v.y), inches(v.z)])
}

fn vh(ctx: Option<DefId>, v: VertexId) -> String {
    handle('v', ctx, ffi(v))
}

fn eh(ctx: Option<DefId>, e: EdgeId) -> String {
    handle('e', ctx, ffi(e))
}

fn fh(ctx: Option<DefId>, f: FaceId) -> String {
    handle('f', ctx, ffi(f))
}

fn curve_h(ctx: Option<DefId>, id: u32) -> String {
    handle('c', ctx, id as u64)
}

/// Name → index into `Model::layers`.
fn layer_index(m: &Model, name: &str) -> Option<usize> {
    m.layers.iter().position(|l| l.name == name)
}

fn find_material(m: &Model, name: &str) -> Result<Option<esq_core::model::MaterialId>, String> {
    m.find_material(name).map(Some).ok_or_else(|| format!("matière inconnue: {name}"))
}

// ---------------------------------------------------------------- curves

struct CurveShape {
    edges: Vec<EdgeId>,
    verts: Vec<VertexId>,
    closed: bool,
}

/// Orders the edges of a curve along its length.
fn curve_shape(e: &Entities, id: u64) -> Option<CurveShape> {
    let edges = curve_edges(e, id);
    if edges.is_empty() {
        return None;
    }
    let mut adj: HashMap<VertexId, Vec<EdgeId>> = HashMap::new();
    for &x in &edges {
        for v in e.edges[x].v {
            adj.entry(v).or_default().push(x);
        }
    }
    let ends: Vec<VertexId> = adj.iter().filter(|(_, l)| l.len() == 1).map(|(v, _)| *v).collect();
    let first = edges[0];
    let (start, closed) = if ends.is_empty() {
        (e.edges[first].v[0], true)
    } else {
        let s = ends.iter().copied().min_by_key(|v| ffi(adj[v][0]) & 0xffff_ffff).unwrap();
        (s, false)
    };
    let mut verts = vec![start];
    let mut used: Vec<EdgeId> = Vec::new();
    let mut cur = start;
    let mut prefer = if closed { Some(first) } else { None };
    loop {
        let next = prefer.take().or_else(|| adj.get(&cur).and_then(|l| l.iter().copied().find(|x| !used.contains(x))));
        let Some(x) = next else { break };
        if used.contains(&x) {
            break;
        }
        used.push(x);
        cur = e.other_vertex(x, cur);
        if closed && cur == start {
            break;
        }
        verts.push(cur);
    }
    Some(CurveShape { edges: used, verts, closed })
}

/// Circle through three points: (centre, normal).
fn circumcircle(a: DVec3, b: DVec3, c: DVec3) -> Option<(DVec3, DVec3)> {
    let ab = b - a;
    let ac = c - a;
    let n = ab.cross(ac);
    let d = 2.0 * n.length_squared();
    if d < 1e-18 {
        return None;
    }
    let centre = a + (n.cross(ab) * ac.length_squared() + ac.cross(n) * ab.length_squared()) / d;
    Some((centre, n.normalize()))
}

struct ArcInfo {
    center: DVec3,
    normal: DVec3,
    radius: f64,
    xaxis: DVec3,
    start: f64,
    end: f64,
    circular: bool,
}

/// Arc parameters of a curve when its points lie on a circle at regular angular steps.
fn arc_info(e: &Entities, shape: &CurveShape, key: &str) -> Option<ArcInfo> {
    let pts: Vec<DVec3> = shape.verts.iter().map(|&v| e.verts[v].pos).collect();
    let n = pts.len();
    if n < 3 {
        return None;
    }
    let (center, normal) =
        circumcircle(pts[0], pts[n / 2], pts[n - 1]).or_else(|| circumcircle(pts[0], pts[1], pts[2]))?;
    let radius = pts[0].distance(center);
    if radius < TOL
        || pts.iter().any(|p| {
            (p.distance(center) - radius).abs() > TOL.max(radius * 1e-6) || (*p - center).dot(normal).abs() > TOL
        })
    {
        return None;
    }
    let num = |name: &str| match e.prop(key, name) {
        Some(AttrValue::Number(x)) => Some(*x),
        _ => None,
    };
    let (u, v) = (pts[0] - center, normal.cross(pts[0] - center));
    let angle = |p: DVec3| {
        let d = p - center;
        d.dot(v).atan2(d.dot(u)).rem_euclid(std::f64::consts::TAU)
    };
    let steps: Vec<f64> =
        (0..n - 1).map(|i| (angle(pts[i + 1]) - angle(pts[i])).rem_euclid(std::f64::consts::TAU)).collect();
    let step = steps[0];
    if steps.iter().any(|s| (s - step).abs() > 1e-6) {
        return None;
    }
    let sweep = if shape.closed { std::f64::consts::TAU } else { step * (n - 1) as f64 };
    let (xaxis, start) = match (num("xx"), num("xy"), num("xz"), num("start")) {
        (Some(x), Some(y), Some(z), Some(s)) => (DVec3::new(x, y, z), s),
        _ => (u.normalize(), 0.0),
    };
    let end = if shape.closed { start + std::f64::consts::TAU } else { num("end").unwrap_or(start + sweep) };
    // both angles at 0 (an arc of 0 to 2π that Entities#add_arc closed), or a
    // whole turn between them, within 1e-6.
    let full = (start.abs() < 1e-6 && end.abs() < 1e-6) || (std::f64::consts::TAU - (end - start).abs()).abs() < 1e-6;
    Some(ArcInfo { center, normal, radius, xaxis, start, end, circular: shape.closed || full })
}

fn curve_json(m: &Model, ctx: Option<DefId>, id: u64) -> R {
    let e = ents(m, ctx).ok_or_else(|| deleted('c'))?;
    let shape = curve_shape(e, id).ok_or_else(|| deleted('c'))?;
    let key = prop_key('c', id);
    let kind = match e.prop(&key, "kind") {
        Some(AttrValue::Text(t)) => t.clone(),
        _ => String::new(),
    };
    let arc = arc_info(e, &shape, &key);
    let is_arc = match kind.as_str() {
        "curve" | "polygon" => false,
        _ => arc.is_some(),
    };
    let mut out = json!({
        "edges": shape.edges.iter().map(|&x| eh(ctx, x)).collect::<Vec<_>>(),
        "verts": shape.verts.iter().map(|&v| vh(ctx, v)).collect::<Vec<_>>(),
        "pts": shape.verts.iter().map(|&v| out_pt(e.verts[v].pos)).collect::<Vec<_>>(),
        "closed": shape.closed,
        "arc": is_arc,
        "polygon": kind == "polygon",
    });
    if let (true, Some(a)) = (is_arc, arc) {
        out["center"] = out_pt(a.center);
        out["normal"] = out_vec(a.normal);
        out["radius"] = json!(inches(a.radius));
        out["xaxis"] = out_vec(a.xaxis);
        out["start"] = json!(a.start);
        out["end"] = json!(a.end);
        out["circular"] = json!(a.circular);
    }
    Ok(out)
}

// ---------------------------------------------------------------- types and listing

fn instance_type(m: &Model, i: &esq_core::model::Instance) -> &'static str {
    if m.is_image_def(i.def) {
        "Image"
    } else if m.defs.get(i.def).is_some_and(|d| d.is_group) {
        "Group"
    } else {
        "ComponentInstance"
    }
}

fn annot_type(e: &Entities, key_s: &str, a: &Annotation) -> &'static str {
    match a.kind {
        AnnotKind::Linear { .. } if e.prop(key_s, "radial").is_some() => "DimensionRadial",
        AnnotKind::Linear { .. } => "DimensionLinear",
        _ => "Text",
    }
}

fn guide_type(g: &Guide) -> &'static str {
    if g.is_point() { "ConstructionPoint" } else { "ConstructionLine" }
}

fn type_of(m: &Model, h: &H) -> Result<&'static str, String> {
    let e = ents(m, h.ctx).ok_or_else(|| deleted(h.kind))?;
    let k = KeyData::from_ffi(h.id);
    let name = match h.kind {
        'e' => e.edges.contains_key(EdgeId::from(k)).then_some("Edge"),
        'f' => e.faces.contains_key(FaceId::from(k)).then_some("Face"),
        'v' => e.verts.contains_key(VertexId::from(k)).then_some("Vertex"),
        'c' => exists(m, h).then(|| {
            let shape = curve_shape(e, h.id);
            let key = prop_key('c', h.id);
            match (shape, e.prop(&key, "kind")) {
                (_, Some(AttrValue::Text(t))) if t == "curve" || t == "polygon" => "Curve",
                (Some(s), _) if arc_info(e, &s, &key).is_some() => "ArcCurve",
                _ => "Curve",
            }
        }),
        'i' => e.instances.get(InstanceId::from(k)).map(|i| instance_type(m, i)),
        'g' => e.guides.get(key(h.id)).map(guide_type),
        'a' => e.annots.get(key(h.id)).map(|a| annot_type(e, &prop_key('a', h.id), a)),
        's' => e.sections.contains_key(key(h.id)).then_some("SectionPlane"),
        'n' => snap_ix(e, h.id).map(|_| "Snap"),
        'p' => space_ix(e, h.id).map(|_| "Space"),
        _ => None,
    };
    name.ok_or_else(|| deleted(h.kind))
}

fn list(m: &Model, ctx: Option<DefId>) -> R {
    let e = ents(m, ctx).ok_or("contexte supprimé")?;
    let mut v = Vec::new();
    for (id, _) in e.edges.iter() {
        v.push(json!([eh(ctx, id), "Edge"]));
    }
    for (id, _) in e.faces.iter() {
        v.push(json!([fh(ctx, id), "Face"]));
    }
    for (id, i) in e.instances.iter() {
        v.push(json!([handle('i', ctx, ffi(id)), instance_type(m, i)]));
    }
    for (id, g) in e.guides.iter() {
        v.push(json!([handle('g', ctx, ffi(id)), guide_type(g)]));
    }
    for (id, a) in e.annots.iter() {
        v.push(json!([handle('a', ctx, ffi(id)), annot_type(e, &prop_key('a', ffi(id)), a)]));
    }
    for (id, _) in e.sections.iter() {
        v.push(json!([handle('s', ctx, ffi(id)), "SectionPlane"]));
    }
    // Snaps are entities of the collection (`Entities#add_snap` adds them to it, as
    // `add_cpoint` does a guide): `each`, `length`, `grep(Sketchup::Snap)` see them.
    for s in e.snaps.iter().filter(|s| s.id != 0) {
        v.push(json!([handle('n', ctx, u64::from(s.id)), "Snap"]));
    }
    // So are spaces (`Entities#add_space`).
    for s in e.spaces.iter().filter(|s| s.id != 0) {
        v.push(json!([handle('p', ctx, u64::from(s.id)), "Space"]));
    }
    Ok(json!(v))
}

// ---------------------------------------------------------------- per-entity info

fn loop_edges(e: &Entities, l: &[VertexId]) -> Vec<Option<EdgeId>> {
    (0..l.len()).map(|i| e.find_edge(l[i], l[(i + 1) % l.len()])).collect()
}

fn get(m: &Model, h: &H) -> R {
    need(m, h)?;
    let ctx = h.ctx;
    let e = ents(m, ctx).unwrap();
    let pkey = prop_key(h.kind, h.id);
    let props = props_json(e, &pkey);
    let k = KeyData::from_ffi(h.id);
    Ok(match h.kind {
        'e' => {
            let id = EdgeId::from(k);
            let ed = &e.edges[id];
            let (p, q) = e.edge_points(id);
            json!({
                "a": out_pt(p), "b": out_pt(q),
                "va": vh(ctx, ed.v[0]), "vb": vh(ctx, ed.v[1]),
                "faces": ed.faces.iter().map(|&f| fh(ctx, f)).collect::<Vec<_>>(),
                "soft": ed.soft, "smooth": ed.smooth, "hidden": ed.hidden, "layer": ed.layer,
                "curve": ed.curve.map(|c| curve_h(ctx, c)),
                "props": props,
            })
        }
        'f' => {
            let id = FaceId::from(k);
            let f = &e.faces[id];
            let loops: Vec<J> = f
                .loops()
                .map(|l| {
                    json!({
                        "verts": l.iter().map(|&v| vh(ctx, v)).collect::<Vec<_>>(),
                        "pts": l.iter().map(|&v| out_pt(e.verts[v].pos)).collect::<Vec<_>>(),
                        "edges": loop_edges(e, l).into_iter().map(|x| x.map(|x| eh(ctx, x))).collect::<Vec<_>>(),
                    })
                })
                .collect();
            json!({
                "loops": loops, "normal": out_vec(f.normal), "area": super::area(e, id),
                "front": super::mat_name(m, f.front), "back": super::mat_name(m, f.back),
                "hidden": f.hidden, "layer": f.layer, "props": props,
            })
        }
        'v' => {
            let id = VertexId::from(k);
            let v = &e.verts[id];
            let faces: Vec<String> = e
                .faces
                .iter()
                .filter(|(_, f)| f.all_vertices().any(|x| x == id))
                .map(|(fid, _)| fh(ctx, fid))
                .collect();
            json!({
                "pos": out_pt(v.pos),
                "edges": v.edges.iter().map(|&x| eh(ctx, x)).collect::<Vec<_>>(),
                "faces": faces,
            })
        }
        'g' => {
            let g = &e.guides[key(h.id)];
            json!({
                "pos": out_pt(g.pos), "dir": if g.infinite { out_vec(g.dir) } else { out_len_vec(g.dir) },
                "point": g.is_point(), "infinite": g.infinite,
                "hidden": g.hidden, "layer": g.layer, "props": props,
            })
        }
        'a' => {
            let a = &e.annots[key(h.id)];
            let mut o = json!({"text": a.text, "hidden": a.hidden, "layer": a.layer, "props": props});
            match &a.kind {
                AnnotKind::Linear { a: p, b: q, offset } => {
                    o["kind"] = json!("linear");
                    o["start"] = out_pt(*p);
                    o["end"] = out_pt(*q);
                    o["offset"] = out_len_vec(*offset);
                    o["measure"] = json!(inches(p.distance(*q)));
                    // As the model shows it (Model Info › Units), whatever thread asks.
                    o["measure_text"] = json!(esq_core::units::UnitsDisplay::of(m).length(p.distance(*q)));
                }
                AnnotKind::Text { point, leader } => {
                    o["kind"] = json!("text");
                    o["point"] = out_pt(*point);
                    o["leader"] = out_len_vec(*leader);
                }
                AnnotKind::ScreenText { x, y } => {
                    o["kind"] = json!("screen");
                    o["x"] = json!(x);
                    o["y"] = json!(y);
                }
            }
            o
        }
        's' => {
            let s = &e.sections[key(h.id)];
            json!({
                "pos": out_pt(s.pos), "normal": out_vec(s.normal), "name": s.name, "symbol": s.symbol,
                "active": e.active_section.map(ffi) == Some(h.id), "hidden": s.hidden, "layer": s.layer,
                "props": props,
            })
        }
        'i' => {
            let i = &e.instances[InstanceId::from(k)];
            json!({"hidden": i.hidden, "layer": i.layer, "props": props})
        }
        'n' => {
            let s = &e.snaps[snap_ix(e, h.id).ok_or_else(|| deleted('n'))?];
            json!({
                "pos": out_pt(s.pos), "dir": out_vec(s.dir), "up": out_vec(s.up),
                "hidden": s.hidden, "layer": s.layer, "props": props,
            })
        }
        'p' => {
            let s = &e.spaces[space_ix(e, h.id).ok_or_else(|| deleted('p'))?];
            let mut o = space_json(s);
            o["hidden"] = json!(s.hidden);
            o["layer"] = json!(s.layer);
            o["props"] = props;
            o
        }
        'c' => curve_json(m, ctx, h.id)?,
        _ => return Err(format!("type d'entité non géré: {}", h.kind)),
    })
}

fn set_common(m: &mut Model, h: &H, a: &J) -> R {
    need(m, h)?;
    let layer = match a.get("layer") {
        Some(J::Number(n)) => n.as_u64().map(|x| x as usize),
        Some(J::String(s)) => layer_index(m, s),
        _ => None,
    };
    let e = ents_mut(m, h.ctx).unwrap();
    if let Some(er) = eref_of(h) {
        if let Some(v) = a.get("hidden").and_then(J::as_bool) {
            e.set_hidden(er, v);
        }
        if let Some(l) = layer {
            e.set_layer(er, l);
        }
    }
    if let Some(p) = a.get("props").and_then(J::as_object) {
        let pkey = prop_key(h.kind, h.id);
        for (k, v) in p {
            match json_av(v) {
                Some(av) => e.set_prop(&pkey, k, av),
                None => e.remove_prop(&pkey, k),
            }
        }
    }
    Ok(J::Null)
}

// ---------------------------------------------------------------- attaches of annotations

/// An attached end from the handles of a path (`Esq.path_handles`: the instances from the
/// annotation's collection `ctx`, then the leaf) and the point `p` it names, in `ctx`'s
/// coordinates (`esq_core::attach`). On an edge, with `mode` "along" (a text, an end given
/// without a path) the point stays a point of the edge's line; otherwise
///  an end at its middle is its middle, at an end its vertex. "arc": the point
/// of an arc at `p`'s angle.
fn attach_from(m: &Model, ctx: Option<DefId>, s: &str, p: DVec3, mode: Option<&str>) -> Result<Attach, String> {
    use esq_core::attach::{self, AttachTo};
    let bad = || arg_err("Invalid instance path.");
    let hs: Vec<H> = s.split_whitespace().map(parse_h).collect::<Result<_, _>>()?;
    let (leaf, insts) = hs.split_last().ok_or_else(bad)?;
    let mut path = Vec::new();
    let mut at = ctx;
    for h in insts {
        if h.kind != 'i' || h.ctx != at {
            return Err(bad());
        }
        let i: InstanceId = key(h.id);
        path.push(i);
        at = Some(ents(m, at).and_then(|e| e.instances.get(i)).ok_or_else(bad)?.def);
    }
    if leaf.ctx != at {
        return Err(bad());
    }
    let e = ents(m, at).ok_or_else(bad)?;
    let q = m
        .attach_leaf(ctx, &Attach::new(path.clone(), AttachTo::Origin))
        .ok_or_else(bad)?
        .2
        .inverse()
        .transform_point3(p);
    let to = match leaf.kind {
        'v' => Some(AttachTo::Vertex(key(leaf.id))),
        'e' if mode == Some("along") => attach::along_edge(e, key(leaf.id), q),
        'e' => attach::on_edge(e, key(leaf.id), q),
        'g' => attach::on_guide(e, key(leaf.id), q),
        'f' => attach::on_face(e, key(leaf.id), q),
        's' => e.sections.contains_key(key(leaf.id)).then(|| AttachTo::Section(key(leaf.id), q)),
        // An arc: its centre, or with `mode` "arc" the point of it at `p`'s angle (the end of a
        // radial dimension).
        'c' => {
            let edges = curve_edges(e, leaf.id);
            match (edges.first(), mode) {
                (Some(&x), Some("arc")) => {
                    esq_core::curves::arc_fit_edges(e, &edges).map(|f| AttachTo::OnArc(x, f.angle_of(q)))
                }
                (Some(&x), _) => Some(AttachTo::ArcCenter(x)),
                _ => None,
            }
        }
        'i' => {
            path.push(key(leaf.id));
            Some(AttachTo::Origin)
        }
        _ => None,
    };
    Ok(Attach::new(path, to.ok_or_else(bad)?))
}

/// The handles of an attached end of an annotation of collection `ctx`: its path's instances,
/// then the entity it names (`Esq.attach_path` reads them back); none when it no longer leads
/// anywhere.
fn attach_handles(m: &Model, ctx: Option<DefId>, a: &Attach) -> Option<String> {
    use esq_core::attach::AttachTo;
    let (leaf_ctx, e, _) = m.attach_leaf(ctx, a)?;
    let mut out = Vec::new();
    let mut at = ctx;
    for &i in &a.path {
        out.push(handle('i', at, ffi(i)));
        at = Some(ents(m, at)?.instances.get(i)?.def);
    }
    let leaf = match a.to {
        AttachTo::Vertex(v) => e.verts.contains_key(v).then(|| handle('v', leaf_ctx, ffi(v))),
        AttachTo::EdgeMiddle(x) | AttachTo::OnEdge(x, _) => e.edges.contains_key(x).then(|| eh(leaf_ctx, x)),
        AttachTo::ArcCenter(x) | AttachTo::OnArc(x, _) => {
            e.edges.get(x).and_then(|x| x.curve).map(|c| handle('c', leaf_ctx, c as u64))
        }
        AttachTo::GuidePoint(g) | AttachTo::OnGuide(g, _) => {
            e.guides.contains_key(g).then(|| handle('g', leaf_ctx, ffi(g)))
        }
        AttachTo::OnFace(f, ..) => e.faces.contains_key(f).then(|| fh(leaf_ctx, f)),
        AttachTo::Section(s, _) => e.sections.contains_key(s).then(|| handle('s', leaf_ctx, ffi(s))),
        AttachTo::Origin => Some(out.pop()?),
    }?;
    out.push(leaf);
    Some(out.join(" "))
}

// ---------------------------------------------------------------- geometry helpers

/// Splits edge `e` at `p` keeping `e` as the first half; returns the new second half.
fn split_keep(en: &mut Entities, e: EdgeId, p: DVec3) -> Option<EdgeId> {
    let edge = en.edges.get(e)?.clone();
    let [a, b] = edge.v;
    let v = en.add_vertex_raw(p);
    en.edges[e].v = [a, v];
    if let Some(vb) = en.verts.get_mut(b) {
        vb.edges.retain(|&x| x != e);
    }
    en.verts[v].edges.push(e);
    let mut ne = edge.clone();
    ne.v = [v, b];
    ne.faces = edge.faces.clone();
    let ne = en.edges.insert(ne);
    en.verts[v].edges.push(ne);
    en.verts[b].edges.push(ne);
    for &f in &edge.faces {
        let Some(face) = en.faces.get_mut(f) else { continue };
        for l in std::iter::once(&mut face.outer).chain(face.holes.iter_mut()) {
            let n = l.len();
            if let Some(i) = (0..n).find(|&i| {
                let (x, y) = (l[i], l[(i + 1) % n]);
                (x == a && y == b) || (x == b && y == a)
            }) {
                // Between the two ends, whichever way the loop runs.
                l.insert(i + 1, v);
            }
        }
    }
    Some(ne)
}

fn classify(e: &Entities, f: FaceId, p: DVec3) -> i64 {
    let face = &e.faces[f];
    let pl = Plane::from_point_normal(e.verts[face.outer[0]].pos, face.normal);
    if pl.distance(p).abs() > TOL {
        return 32;
    }
    for l in face.loops() {
        let pts = e.loop_points(l);
        if pts.iter().any(|q| q.distance(p) <= TOL) {
            return 2;
        }
        for i in 0..pts.len() {
            let (_, d) = project_on_segment(p, pts[i], pts[(i + 1) % pts.len()]);
            if d <= TOL {
                return 4;
            }
        }
    }
    let (u, v) = basis_for(face.normal);
    let to2 = |q: &DVec3| DVec2::new(q.dot(u), q.dot(v));
    let outer: Vec<DVec2> = e.loop_points(&face.outer).iter().map(to2).collect();
    if !point_in_polygon_2d(to2(&p), &outer) {
        return 16;
    }
    for h in &face.holes {
        let hole: Vec<DVec2> = e.loop_points(h).iter().map(to2).collect();
        if point_in_polygon_2d(to2(&p), &hole) {
            return 16;
        }
    }
    1
}

/// Whether face `f` lies on, inside or around another face of its plane (a vertex of either
/// strictly inside the other's outer loop).
fn overlaps_a_coplanar_face(e: &Entities, f: FaceId) -> bool {
    let face = &e.faces[f];
    let (n, p0) = (face.normal, e.verts[face.outer[0]].pos);
    let (u, v) = basis_for(n);
    let to2 = |p: DVec3| DVec2::new(p.dot(u), p.dot(v));
    let mine: Vec<DVec2> = face.outer.iter().map(|&x| to2(e.verts[x].pos)).collect();
    e.faces.iter().any(|(g, other)| {
        if g == f || other.normal.dot(n).abs() < 1.0 - 1e-9 {
            return false;
        }
        let q0 = e.verts[other.outer[0]].pos;
        if (q0 - p0).dot(n).abs() > TOL {
            return false;
        }
        let theirs: Vec<DVec2> = other.outer.iter().map(|&x| to2(e.verts[x].pos)).collect();
        mine.iter().any(|p| point_in_polygon_2d(*p, &theirs)) || theirs.iter().any(|p| point_in_polygon_2d(*p, &mine))
    })
}

/// `Face#mesh`: the points of the loops, the triangles, and per point the homogeneous texture
/// coordinates of the sides `flags` asks for (`MESH_UVQ_FRONT` 1, `MESH_UVQ_BACK` 2: SketchUp's
/// UV helper, `CFaceToMesh`) and the normal (`MESH_NORMALS` 4).
fn mesh_of(m: &Model, e: &Entities, f: FaceId, flags: u64) -> J {
    let face = &e.faces[f];
    let uvq = |front: bool| -> J {
        let map = super::model::uv::map_of(m, face, front);
        let all: Vec<J> = face
            .loops()
            .flat_map(|l| l.iter().map(|&x| e.verts[x].pos))
            .map(|p| {
                let h = map.uvq(p);
                json!([h.x, h.y, h.z])
            })
            .collect();
        J::Array(all)
    };
    let extra = json!({
        "front": if flags & 1 != 0 { uvq(true) } else { J::Null },
        "back": if flags & 2 != 0 { uvq(false) } else { J::Null },
        // (+ 0.0: no −0 in what a script sees.)
        "normal": if flags & 4 != 0 { json!([face.normal.x + 0.0, face.normal.y + 0.0, face.normal.z + 0.0]) } else { J::Null },
    });
    let (u, v) = basis_for(face.normal);
    let pts3: Vec<DVec3> = face.loops().flat_map(|l| l.iter().map(|&x| e.verts[x].pos)).collect();
    let to2 = |p: &DVec3| DVec2::new(p.dot(u), p.dot(v));
    let outer: Vec<DVec2> = e.loop_points(&face.outer).iter().map(to2).collect();
    let holes: Vec<Vec<DVec2>> = face.holes.iter().map(|h| e.loop_points(h).iter().map(to2).collect()).collect();
    let tris = esq_core::tess::triangulate_2d(&outer, &holes);
    let sizes: Vec<usize> = face.loops().map(|l| l.len()).collect();
    let tris: Vec<[usize; 3]> = tris
        .into_iter()
        .map(|[a, b, c]| {
            let t = [pts3[a], pts3[b], pts3[c]];
            if (t[1] - t[0]).cross(t[2] - t[0]).dot(face.normal) < 0.0 { [a, c, b] } else { [a, b, c] }
        })
        .collect();
    json!({"pts": pts3.into_iter().map(out_pt).collect::<Vec<_>>(), "tris": tris, "loops": sizes, "uvq": extra})
}

/// `Entities#add_snap` / `Snap#set`: the up made perpendicular to the direction, as
/// SketchUp does; `None` when they are parallel (0.001 per component).
fn snap_frame(dir: DVec3, up: DVec3) -> Option<(DVec3, DVec3)> {
    let d = dir.normalize_or_zero();
    let u = up.normalize_or_zero();
    let same = |a: DVec3, b: DVec3| (a - b).abs().max_element() <= 0.001;
    if same(d, u) || same(d, -u) {
        return None;
    }
    Some((d, d.cross(u).cross(d).normalize()))
}

fn new_guide(en: &mut Entities, g: Guide) -> u64 {
    ffi(en.guides.insert(g))
}

/// Adds the faces of a polygon mesh (points in inches, polygons as 1-based indexes, negative
/// when the edge starting at that point is hidden). Returns the number of faces made.
fn add_mesh(c: &mut Ctx, a: &J) -> R {
    let ctx = ctx_of(a)?;
    let points = pts(&a["pts"])?;
    let polys = a["polys"].as_array().ok_or("polys attendu")?;
    let flags = a.get("flags").and_then(J::as_i64).unwrap_or(12);
    let front = match a.get("front").and_then(J::as_str) {
        Some(n) => find_material(c.model, n)?,
        None => None,
    };
    let back = match a.get("back").and_then(J::as_str) {
        Some(n) => find_material(c.model, n)?,
        None => None,
    };
    let en = ents_mut(c.model, ctx).ok_or("contexte supprimé")?;
    let mut count = 0;
    let mut touched: Vec<EdgeId> = Vec::new();
    let mut made: Vec<FaceId> = Vec::new();
    for poly in polys {
        let idx: Vec<i64> = poly.as_array().ok_or("polygone attendu")?.iter().filter_map(J::as_i64).collect();
        if idx.len() < 3 {
            continue;
        }
        let ps: Vec<DVec3> =
            idx.iter().map(|i| points[(i.unsigned_abs() as usize).saturating_sub(1).min(points.len() - 1)]).collect();
        let vs: Vec<VertexId> = ps.iter().map(|&p| en.vertex_at(p)).collect();
        let mut dedup = vs.clone();
        dedup.dedup();
        if dedup.len() < 3 || dedup.first() == dedup.last() && dedup.len() < 4 {
            continue;
        }
        let existing = en.faces.iter().find(|(_, f)| {
            let mut o = f.outer.clone();
            o.sort();
            let mut s = vs.clone();
            s.sort();
            o == s
        });
        let f = match existing {
            Some((f, _)) => f,
            None => match en.add_face_raw(vs.clone(), vec![]) {
                Some(f) => f,
                None => continue,
            },
        };
        en.faces[f].front = front.or(en.faces[f].front);
        en.faces[f].back = back.or(en.faces[f].back);
        made.push(f);
        count += 1;
        for (i, &ix) in idx.iter().enumerate() {
            let (x, y) = (vs[i], vs[(i + 1) % vs.len()]);
            if let Some(ed) = en.find_edge(x, y) {
                touched.push(ed);
                if ix < 0 {
                    // HIDE_BASED_ON_INDEX wins over SOFTEN_BASED_ON_INDEX, SMOOTH_SOFT_EDGES
                    // still applies (TestUp *_hide_based_on_index_override_soften_based_on_index).
                    let edge = &mut en.edges[ed];
                    if flags & 1 != 0 {
                        edge.hidden = true;
                    } else if flags & 2 != 0 {
                        edge.soft = true;
                    }
                    if flags & 8 != 0 {
                        edge.smooth = true;
                    }
                }
            }
        }
    }
    if flags & 4 != 0 {
        for ed in touched {
            if en.edges.get(ed).is_some_and(|x| x.faces.len() > 1) {
                en.edges[ed].soft = true;
                if flags & 8 != 0 {
                    en.edges[ed].smooth = true;
                }
            }
        }
    }
    Ok(json!(count))
}

// ---------------------------------------------------------------- dispatch

pub fn dispatch(c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    if !matches!(
        op,
        "ent.valid"
            | "ent.list"
            | "ent.type"
            | "ent.get"
            | "ent.set"
            | "ent.erase"
            | "ent.connected"
            | "ent.transform"
            | "ent.transform_by_vectors"
            | "ent.intersect"
            | "ent.weld"
            | "ent.add_mesh"
            | "ent.add_face"
            | "ent.add_curve"
            | "edge.split"
            | "edge.find_faces"
            | "edge.explode_curve"
            | "curve.set_meta"
            | "curve.move_vertices"
            | "face.classify"
            | "face.glued"
            | "face.mesh"
            | "ent.pid"
            | "face.followme"
            | "guide.add"
            | "guide.set"
            | "annot.add"
            | "annot.set"
            | "annot.attach"
            | "annot.attached"
            | "section.add"
            | "section.set"
            | "section.activate"
            | "axes.get"
            | "axes.set"
            | "ents.info"
            | "ents.vertex_at"
            | "builder.face"
            | "builder.edges"
            | "builder.find"
            | "image.add"
            | "image.get"
            | "image.size"
            | "image.origin"
            | "image.xform"
            | "image.rep"
            | "ents.active_section"
            | "snap.add"
            | "snap.set"
            | "space.add"
            | "space.set"
    ) {
        return None;
    }
    Some(run(c, op, a))
}

fn run(c: &mut Ctx, op: &str, a: &J) -> R {
    match op {
        "ent.valid" => {
            let h = h_of(a, "h")?;
            Ok(json!(exists(c.model, &h)))
        }
        "ent.list" => {
            // Snaps placed by older documents have no id yet (a no-op once the document ran
            // `Model::ensure_ruby_ids`).
            c.model.ensure_snap_ids();
            list(c.model, ctx_of(a)?)
        }
        "ent.type" => {
            let h = h_of(a, "h")?;
            Ok(json!(type_of(c.model, &h)?))
        }
        // The persistent id the entity had in the .skp it was read from, if any.
        "ent.pid" => {
            let h = h_of(a, "h")?;
            Ok(json!(ents(c.model, h.ctx).and_then(|e| e.pids.get(&format!("{}{}", h.kind, h.id)))))
        }
        "ent.get" => get(c.model, &h_of(a, "h")?),
        "ent.set" => set_common(c.model, &h_of(a, "h")?, a),
        "ent.erase" => {
            let hl = hs(a, "hs")?;
            let mut by_ctx: Vec<(Option<DefId>, Vec<EntityRef>)> = Vec::new();
            for h in &hl {
                need(c.model, h)?;
                let Some(er) = eref_of(h) else { continue };
                match by_ctx.iter_mut().find(|(x, _)| *x == h.ctx) {
                    Some((_, v)) => v.push(er),
                    None => by_ctx.push((h.ctx, vec![er])),
                }
            }
            for (ctx, items) in by_ctx {
                if let Some(en) = ents_mut(c.model, ctx) {
                    en.erase(&items);
                }
            }
            Ok(J::Null)
        }
        "ent.connected" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let er = eref_of(&h).ok_or("entité sans connexions")?;
            let en = ents(c.model, h.ctx).unwrap();
            Ok(json!(en.connected(er).into_iter().filter_map(|x| super::eref_handle(h.ctx, x)).collect::<Vec<_>>()))
        }
        "ent.transform" | "ent.transform_by_vectors" => {
            let ctx = ctx_of(a)?;
            let hl = hs(a, "hs")?;
            let mut items = Vec::new();
            for h in &hl {
                need(c.model, h)?;
                items.push(h);
            }
            // One transformation per entity: the same for all, or each its own.
            let xforms: Vec<DMat4> = if op == "ent.transform" {
                let m = mat_from(&a["xform"])?;
                if m.determinant().abs() < 1e-12 {
                    return Err(arg_err("Unable to apply transformation: transformation matrix is not invertible"));
                }
                vec![m; items.len()]
            } else {
                let list = a["xforms"].as_array().ok_or("xforms attendu")?;
                list.iter().take(items.len()).map(mat_from).collect::<Result<_, _>>()?
            };
            let en = ents_mut(c.model, ctx).ok_or("contexte supprimé")?;
            if op == "ent.transform" {
                let geo: Vec<EntityRef> = items.iter().filter_map(|h| eref_of(h)).collect();
                if let Some(m) = xforms.first() {
                    en.transform_selection(&geo, m);
                }
            } else {
                // `transform_by_vectors`: the vertices, given or
                // those of the edges and faces, move each by its own transformation, all at once.
                let mut moves: Vec<(VertexId, DMat4)> = Vec::new();
                for (h, m) in items.iter().zip(&xforms) {
                    match (h.kind, eref_of(h)) {
                        ('v', _) => moves.push((VertexId::from(KeyData::from_ffi(h.id)), *m)),
                        (_, Some(er @ (EntityRef::Edge(_) | EntityRef::Face(_)))) => {
                            let mut vs: Vec<VertexId> = en.vertices_of(&[er]).into_iter().collect();
                            vs.sort();
                            moves.extend(vs.into_iter().map(|v| (v, *m)));
                        }
                        (_, Some(er)) => en.transform_selection(&[er], m),
                        (_, None) => {}
                    }
                }
                en.transform_vertices_each(&moves);
            }
            Ok(J::Null)
        }
        "ent.intersect" => {
            // Entities#intersect_with: {ctx, t1, recurse, dest, t2, hs}.
            let ctx = ctx_of(a)?;
            let dest = match a.get("dest").and_then(J::as_str) {
                Some(d) => super::parse_ctx(d)?,
                None => ctx,
            };
            if ents(c.model, ctx).is_none() || ents(c.model, dest).is_none() {
                return Err("contexte supprimé".into());
            }
            let mat = |k: &str| {
                a.get(k).filter(|m| !m.is_null()).map(mat_from).transpose().map(|m| m.unwrap_or(DMat4::IDENTITY))
            };
            let (t1, t2) = (mat("t1")?, mat("t2")?);
            if t2.determinant().abs() < 1e-12 {
                return Err(arg_err("Unable to apply transformation: transformation matrix is not invertible"));
            }
            let recurse = a.get("recurse").and_then(J::as_bool).unwrap_or(false);
            let hidden = a.get("hidden").and_then(J::as_bool).unwrap_or(false);
            let others: Vec<(Option<DefId>, EntityRef)> =
                hs(a, "hs")?.iter().filter(|h| exists(c.model, h)).filter_map(|h| Some((h.ctx, eref_of(h)?))).collect();
            let opts = esq_core::intersect::IntersectOptions { recurse, hidden };
            let made = c.model.intersect_with(ctx, &t1, opts, &others, dest, &t2);
            Ok(json!(made.into_iter().map(|k| eh(dest, k)).collect::<Vec<_>>()))
        }
        "ent.weld" => {
            let ctx = ctx_of(a)?;
            let hl = hs(a, "hs")?;
            // Each edge must live and belong to this collection.
            for h in hl.iter().filter(|h| h.kind == 'e') {
                need(c.model, h)?;
                if h.ctx != ctx {
                    return Err(arg_err("Edge has different parent"));
                }
            }
            let en = ents_mut(c.model, ctx).ok_or("contexte supprimé")?;
            let mut edges: Vec<EdgeId> = Vec::new();
            for h in &hl {
                if h.kind == 'e' {
                    let eid = EdgeId::from(KeyData::from_ffi(h.id));
                    if !edges.contains(&eid) {
                        edges.push(eid);
                    }
                }
            }
            // One curve per connected run of the given edges.
            let mut left: HashSet<EdgeId> = edges.iter().copied().collect();
            let mut curves = Vec::new();
            for &start in &edges {
                if !left.remove(&start) {
                    continue;
                }
                let id = en.new_curve_id();
                let mut stack = vec![start];
                while let Some(x) = stack.pop() {
                    en.edges[x].curve = Some(id);
                    for v in en.edges[x].v {
                        for &y in &en.verts[v].edges {
                            if left.remove(&y) {
                                stack.push(y);
                            }
                        }
                    }
                }
                // A welded run is a plain curve, never an arc (CCurve).
                en.set_prop(&prop_key('c', id as u64), "kind", AttrValue::Text("curve".into()));
                curves.push(curve_h(ctx, id));
            }
            Ok(json!(curves))
        }
        "ent.add_mesh" => add_mesh(c, a),
        "ent.add_face" => {
            let ctx = ctx_of(a)?;
            let p = pts(&a["pts"])?;
            if p.len() < 3 {
                return Err(arg_err("Not enough edges - at least 3 required"));
            }
            let en = ents_mut(c.model, ctx).ok_or("contexte supprimé")?;
            let vs: Vec<VertexId> = p.iter().map(|&q| en.vertex_at(q)).collect();
            let mut sorted = vs.clone();
            sorted.sort();
            if let Some((f, _)) = en.faces.iter().find(|(_, f)| {
                let mut o = f.outer.clone();
                o.sort();
                o == sorted
            }) {
                return Ok(json!(fh(ctx, f)));
            }
            let f = en.add_face_raw(vs, vec![]).ok_or_else(|| arg_err("Points are not planar"))?;
            // Like SketchUp, a face on the ground plane always faces down.
            let n = en.faces[f].normal;
            let on_ground = en.faces[f].outer.iter().all(|&v| en.verts[v].pos.z.abs() < TOL);
            if on_ground && n.z > 0.0 {
                en.reverse_face(f);
                // Keep the first point first.
                en.faces[f].outer.rotate_right(1);
            }
            // Like SketchUp, a face added inside another of its plane (or around it) cuts it:
            // the outer one gets a hole (TestUp `create_face_with_hole`).
            if overlaps_a_coplanar_face(en, f) {
                let edges = en.face_edges(f);
                en.split_faces_with(&edges);
            }
            Ok(json!(fh(ctx, f)))
        }
        "ent.add_curve" => {
            // Curve with an explicit kind: arc (circle, arc), polygon (ngon), curve (polyline).
            let ctx = ctx_of(a)?;
            let p = pts(&a["pts"])?;
            let closed = a.get("closed").and_then(J::as_bool).unwrap_or(false);
            let en = ents_mut(c.model, ctx).ok_or("contexte supprimé")?;
            let ids = en.add_polyline_edges(&p, closed, true);
            let cid = ids.first().and_then(|&x| en.edges.get(x)).and_then(|x| x.curve);
            if let Some(cid) = cid {
                let key = prop_key('c', cid as u64);
                if let Some(k) = a.get("kind").and_then(J::as_str) {
                    en.set_prop(&key, "kind", AttrValue::Text(k.into()));
                }
                for (name, k) in [("start", "start"), ("end", "end")] {
                    if let Some(v) = a.get(k).and_then(J::as_f64) {
                        en.set_prop(&key, name, AttrValue::Number(v));
                    }
                }
                if let Some(x) = a.get("xaxis").and_then(|x| vec3(x).ok()) {
                    en.set_prop(&key, "xx", AttrValue::Number(x.x));
                    en.set_prop(&key, "xy", AttrValue::Number(x.y));
                    en.set_prop(&key, "xz", AttrValue::Number(x.z));
                }
            }
            Ok(json!(ids.into_iter().map(|x| eh(ctx, x)).collect::<Vec<_>>()))
        }
        "edge.split" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let en = ents_mut(c.model, h.ctx).unwrap();
            let id = EdgeId::from(KeyData::from_ffi(h.id));
            let (p, q) = en.edge_points(id);
            let len = p.distance(q);
            let t = if let Some(t) = a.get("t").and_then(J::as_f64) {
                t
            } else {
                let x = pt(&a["pt"])?;
                project_on_segment(x, p, q).0
            };
            if !(t > 0.0 && t < 1.0) {
                return Ok(J::Null);
            }
            let x = p + (q - p) * t;
            if t * len < TOL || (1.0 - t) * len < TOL {
                return Ok(J::Null);
            }
            Ok(match split_keep(en, id, x) {
                Some(ne) => json!(eh(h.ctx, ne)),
                None => J::Null,
            })
        }
        "edge.find_faces" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let en = ents_mut(c.model, h.ctx).unwrap();
            let id = EdgeId::from(KeyData::from_ffi(h.id));
            let old: HashSet<FaceId> = en.faces.keys().collect();
            let before = en.faces.len();
            en.heal_faces_around(&[id]);
            // As SketchUp orients what find_faces makes (TestUp test_find_faces_face_orientation_*):
            // the first new face runs along the edge, from its start to its end; on the ground
            // plane a face looks down.
            let [s, e] = en.edges[id].v;
            let mut new: Vec<FaceId> = en.faces.keys().filter(|f| !old.contains(f)).collect();
            new.sort();
            let mut along = false;
            for &f in &new {
                let on_ground = en.faces[f].all_vertices().all(|v| en.verts[v].pos.z.abs() < TOL);
                let outer = &en.faces[f].outer;
                let n = outer.len();
                let backward = (0..n).any(|i| outer[i] == e && outer[(i + 1) % n] == s);
                let forward = (0..n).any(|i| outer[i] == s && outer[(i + 1) % n] == e);
                let flip = if on_ground {
                    en.faces[f].normal.z > 0.0
                } else {
                    let first = !along && (forward || backward);
                    along |= first;
                    first && backward
                };
                if flip {
                    en.reverse_face(f);
                }
            }
            Ok(json!(en.faces.len().saturating_sub(before)))
        }
        "edge.explode_curve" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let en = ents_mut(c.model, h.ctx).unwrap();
            let id = EdgeId::from(KeyData::from_ffi(h.id));
            if let Some(cid) = en.edges[id].curve {
                for x in curve_edges(en, cid as u64) {
                    en.edges[x].curve = None;
                }
                en.props.remove(&prop_key('c', cid as u64));
            }
            Ok(J::Null)
        }
        "curve.set_meta" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let en = ents_mut(c.model, h.ctx).unwrap();
            let key = prop_key('c', h.id);
            if let Some(k) = a.get("kind").and_then(J::as_str) {
                en.set_prop(&key, "kind", AttrValue::Text(k.into()));
            }
            Ok(J::Null)
        }
        "curve.move_vertices" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let p = pts(&a["pts"])?;
            let en = ents_mut(c.model, h.ctx).unwrap();
            let shape = curve_shape(en, h.id).ok_or_else(|| deleted('c'))?;
            if shape.verts.len() != p.len() {
                return Err(arg_err("Number of points must be the same as the number of vertices"));
            }
            let moved: HashSet<VertexId> = shape.verts.iter().copied().collect();
            for (v, q) in shape.verts.iter().zip(p) {
                en.verts[*v].pos = q;
            }
            let faces: Vec<FaceId> =
                en.faces.iter().filter(|(_, f)| f.all_vertices().any(|v| moved.contains(&v))).map(|(k, _)| k).collect();
            for f in faces {
                en.recompute_normal(f);
            }
            Ok(J::Null)
        }
        "face.classify" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let p = pt(&a["p"])?;
            let en = ents(c.model, h.ctx).unwrap();
            Ok(json!(classify(en, FaceId::from(KeyData::from_ffi(h.id)), p)))
        }
        "face.glued" => {
            // Face#get_glued_instances: the groups, components and images glued to the face.
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let f = FaceId::from(KeyData::from_ffi(h.id));
            let en = ents(c.model, h.ctx).unwrap();
            Ok(json!(
                en.instances
                    .iter()
                    .filter(|(_, i)| i.ext.glued_to == Some(esq_core::model::GlueRef::Face(f)))
                    .map(|(id, i)| json!([handle('i', h.ctx, ffi(id)), instance_type(c.model, i)]))
                    .collect::<Vec<_>>()
            ))
        }
        "face.mesh" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let flags = a.get("flags").and_then(J::as_u64).unwrap_or(0);
            Ok(mesh_of(c.model, ents(c.model, h.ctx).unwrap(), FaceId::from(KeyData::from_ffi(h.id)), flags))
        }
        "face.followme" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let edges: Vec<EdgeId> = hs(a, "edges")?
                .iter()
                .filter(|x| x.kind == 'e')
                .map(|x| EdgeId::from(KeyData::from_ffi(x.id)))
                .collect();
            let en = ents_mut(c.model, h.ctx).unwrap();
            let Ok(path) = en.edge_path(&edges) else { return Ok(json!(false)) };
            Ok(json!(en.follow_me(FaceId::from(KeyData::from_ffi(h.id)), &path).is_ok()))
        }
        "guide.add" => {
            let ctx = ctx_of(a)?;
            let pos = pt(&a["pos"])?;
            let g = match a.get("mode").and_then(J::as_str).unwrap_or("point") {
                "point" => Guide::point(pos),
                "segment" => Guide::segment(pos, pt(&a["end"])?),
                _ => {
                    let d = vec3(&a["dir"])?;
                    if d.length() < 1e-12 {
                        return Err(arg_err("Invalid vector"));
                    }
                    Guide::line(pos, d)
                }
            };
            let en = ents_mut(c.model, ctx).ok_or("contexte supprimé")?;
            let id = new_guide(en, g);
            if let Some(s) = a.get("stipple").and_then(J::as_str) {
                en.set_prop(&prop_key('g', id), "stipple", AttrValue::Text(s.into()));
            }
            Ok(json!(handle('g', ctx, id)))
        }
        "guide.set" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let en = ents_mut(c.model, h.ctx).unwrap();
            let g = en.guides.get_mut(key(h.id)).ok_or_else(|| deleted('g'))?;
            if let Some(p) = a.get("pos") {
                g.pos = pt(p)?;
            }
            if let Some(mode) = a.get("mode").and_then(J::as_str) {
                match mode {
                    "segment" => {
                        let end = pt(&a["end"])?;
                        g.dir = end - g.pos;
                        g.infinite = false;
                    }
                    _ => {
                        g.dir = vec3(&a["dir"])?.normalize_or_zero();
                        g.infinite = true;
                    }
                }
            }
            if let Some(d) = a.get("reverse").and_then(J::as_bool)
                && d
            {
                if g.infinite {
                    g.dir = -g.dir;
                } else {
                    g.pos += g.dir;
                    g.dir = -g.dir;
                }
            }
            Ok(J::Null)
        }
        "annot.add" => {
            let ctx = ctx_of(a)?;
            let text = a.get("text").and_then(J::as_str).unwrap_or("").to_string();
            let kind = match a.get("kind").and_then(J::as_str).unwrap_or("text") {
                "linear" => {
                    AnnotKind::Linear { a: pt(&a["start"])?, b: pt(&a["end"])?, offset: len_vec(&a["offset"])? }
                }
                "radial" => AnnotKind::Linear { a: pt(&a["start"])?, b: pt(&a["end"])?, offset: DVec3::ZERO },
                _ => AnnotKind::Text { point: pt(&a["point"])?, leader: len_vec(&a["leader"])? },
            };
            let en = ents_mut(c.model, ctx).ok_or("contexte supprimé")?;
            let id =
                ffi(en.annots.insert(Annotation { kind, text, layer: 0, hidden: false, attach: Default::default() }));
            if let Some(p) = a.get("props").and_then(J::as_object) {
                for (k, v) in p {
                    if let Some(av) = json_av(v) {
                        en.set_prop(&prop_key('a', id), k, av);
                    }
                }
            }
            Ok(json!(handle('a', ctx, id)))
        }
        "annot.set" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let en = ents_mut(c.model, h.ctx).unwrap();
            let an = en.annots.get_mut(key(h.id)).ok_or_else(|| deleted('a'))?;
            if let Some(t) = a.get("text").and_then(J::as_str) {
                an.text = t.into();
            }
            // A point given makes a free end of it (SketchUp's connection becomes a point).
            let mut freed = [false; 2];
            match &mut an.kind {
                AnnotKind::Text { point, leader } => {
                    if let Some(p) = a.get("point") {
                        *point = pt(p)?;
                        freed[0] = true;
                    }
                    if let Some(l) = a.get("leader") {
                        *leader = len_vec(l)?;
                    }
                }
                AnnotKind::Linear { a: s, b: e, offset } => {
                    if let Some(p) = a.get("start") {
                        *s = pt(p)?;
                        freed[0] = true;
                    }
                    if let Some(p) = a.get("end") {
                        *e = pt(p)?;
                        freed[1] = true;
                    }
                    if let Some(o) = a.get("offset") {
                        *offset = len_vec(o)?;
                    }
                }
                AnnotKind::ScreenText { .. } => {}
            }
            for (end, f) in an.attach.iter_mut().zip(freed) {
                if f {
                    *end = None;
                }
            }
            Ok(J::Null)
        }
        "annot.attach" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let which = a.get("which").and_then(J::as_u64).unwrap_or(0) as usize;
            let id = key(h.id);
            let end = match a.get("handles").and_then(J::as_str).filter(|s| !s.is_empty()) {
                Some(s) => Some(attach_from(c.model, h.ctx, s, pt(&a["pt"])?, a.get("mode").and_then(J::as_str))?),
                None => None,
            };
            c.model.set_attach(h.ctx, id, which, end);
            Ok(J::Null)
        }
        "annot.attached" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let which = a.get("which").and_then(J::as_u64).unwrap_or(0) as usize;
            let en = ents(c.model, h.ctx).ok_or_else(|| deleted('a'))?;
            let an = en.annots.get(key(h.id)).ok_or_else(|| deleted('a'))?;
            Ok(match an.attach.get(which).and_then(|x| x.as_ref()) {
                Some(at) => attach_handles(c.model, h.ctx, at).map_or(J::Null, |s| json!(s)),
                None => J::Null,
            })
        }
        "section.add" => {
            let ctx = ctx_of(a)?;
            let n = vec3(&a["normal"])?;
            if n.length() < 1e-12 {
                return Err(arg_err("Invalid plane"));
            }
            // SketchUp numbers new planes: name "Section N", symbol "N".
            let symbol = c.model.next_section_symbol();
            let mut sp = SectionPlane::new(pt(&a["pos"])?, n);
            sp.name = format!("Section {symbol}");
            sp.symbol = symbol;
            let en = ents_mut(c.model, ctx).ok_or("contexte supprimé")?;
            let id = ffi(en.sections.insert(sp));
            Ok(json!(handle('s', ctx, id)))
        }
        "section.set" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let en = ents_mut(c.model, h.ctx).unwrap();
            let s = en.sections.get_mut(key(h.id)).ok_or_else(|| deleted('s'))?;
            if let Some(p) = a.get("pos") {
                s.pos = pt(p)?;
            }
            if let Some(n) = a.get("normal") {
                let n = vec3(n)?;
                if n.length() < 1e-12 {
                    return Err(arg_err("Invalid plane"));
                }
                s.normal = n.normalize();
            }
            if let Some(t) = a.get("name").and_then(J::as_str) {
                s.name = t.into();
            }
            if let Some(t) = a.get("symbol").and_then(J::as_str) {
                s.symbol = t.into();
            }
            Ok(J::Null)
        }
        "section.activate" => {
            let h = h_of(a, "h")?;
            need(c.model, &h)?;
            let en = ents_mut(c.model, h.ctx).unwrap();
            en.active_section = Some(key(h.id));
            Ok(J::Null)
        }
        "ents.active_section" => {
            let ctx = ctx_of(a)?;
            let en = ents_mut(c.model, ctx).ok_or("contexte supprimé")?;
            match a.get("h").and_then(J::as_str) {
                Some(s) => {
                    let h = parse_h(s)?;
                    if h.ctx != ctx || !en.sections.contains_key(key(h.id)) {
                        return Err(arg_err("The given SectionPlane does not belong to this Entities."));
                    }
                    en.active_section = Some(key(h.id));
                }
                None if a.get("set").is_some() => en.active_section = None,
                None => {}
            }
            Ok(match en.active_section {
                Some(s) => json!(handle('s', ctx, ffi(s))),
                None => J::Null,
            })
        }
        "ents.info" => {
            let ctx = ctx_of(a)?;
            let en = ents(c.model, ctx).ok_or("contexte supprimé")?;
            Ok(
                json!({"count": en.edges.len() + en.faces.len() + en.instances.len() + en.guides.len() + en.annots.len() + en.sections.len()}),
            )
        }
        // `EntitiesBuilder`: the
        // vertices are those already at the points (the builder indexes the collection's own)
        // or new ones; a face keeps the orientation of its points and neither
        // merges with nor cuts what is there; an edge whose ends meet is skipped (null).
        "builder.face" => {
            let ctx = ctx_of(a)?;
            let outer = pts(&a["pts"])?;
            let holes: Vec<Vec<DVec3>> = match a.get("holes").and_then(J::as_array) {
                Some(l) => l.iter().map(pts).collect::<Result<_, _>>()?,
                None => Vec::new(),
            };
            if outer.len() < 3 || holes.iter().any(|h| h.len() < 3) {
                return Err(arg_err("Not enough points in loop require at least 3"));
            }
            let n = esq_core::math::newell_normal(&outer).normalize_or_zero();
            if n == DVec3::ZERO {
                return Err(arg_err("Points are not planar"));
            }
            let off = n.dot(outer[0]);
            if outer.iter().chain(holes.iter().flatten()).any(|p| (n.dot(*p) - off).abs() > TOL) {
                return Err(arg_err("Points are not planar"));
            }
            let en = ents_mut(c.model, ctx).ok_or("contexte supprimé")?;
            let o: Vec<VertexId> = outer.iter().map(|&p| en.vertex_at(p)).collect();
            let hs: Vec<Vec<VertexId>> = holes.iter().map(|h| h.iter().map(|&p| en.vertex_at(p)).collect()).collect();
            let f = en.add_face_raw(o, hs).ok_or_else(|| arg_err("Points are not planar"))?;
            Ok(json!(fh(ctx, f)))
        }
        "builder.edges" => {
            let ctx = ctx_of(a)?;
            let p = pts(&a["pts"])?;
            let en = ents_mut(c.model, ctx).ok_or("contexte supprimé")?;
            let vs: Vec<VertexId> = p.iter().map(|&q| en.vertex_at(q)).collect();
            let out: Vec<J> =
                vs.windows(2).map(|w| en.add_edge_raw(w[0], w[1]).map_or(J::Null, |e| json!(eh(ctx, e)))).collect();
            Ok(J::Array(out))
        }
        "builder.find" => {
            let ctx = ctx_of(a)?;
            let p = pt(&a["p"])?;
            let en = ents(c.model, ctx).ok_or("contexte supprimé")?;
            Ok(en.find_vertex(p).map_or(J::Null, |v| json!(vh(ctx, v))))
        }
        "ents.vertex_at" => {
            let ctx = ctx_of(a)?;
            let p = pt(&a["p"])?;
            let en = ents_mut(c.model, ctx).ok_or("contexte supprimé")?;
            let v = en.vertex_at(p);
            Ok(json!(vh(ctx, v)))
        }
        "snap.add" | "snap.set" => snap_op(c, op, a),
        "space.add" | "space.set" => space_op(c, op, a),
        op if op.starts_with("image.") => image_op(c, op, a),
        "axes.get" => Ok(mat_to(&c.model.axes)),
        "axes.set" => {
            let m = mat_from(&a["m"])?;
            c.model.axes = m;
            Ok(J::Null)
        }
        _ => Err(format!("opération inconnue: {op}")),
    }
}

// ---------------------------------------------------------------- snaps

/// `snap.add` {ctx, pos, dir, up?} and `snap.set` {h, pos, dir?, up?}: without an up, the
/// blue axis (the green one for a snap facing up or down), made perpendicular.
fn snap_op(c: &mut Ctx, op: &str, a: &J) -> R {
    let pos = pt(&a["pos"])?;
    let frame = match a.get("dir").filter(|d| !d.is_null()) {
        None => None,
        Some(d) => {
            let d = vec3(d)?.normalize_or_zero();
            let up = match a.get("up").filter(|u| !u.is_null()) {
                Some(u) => vec3(u)?,
                None if (d - DVec3::Z).abs().max_element() <= 0.001 || (d + DVec3::Z).abs().max_element() <= 0.001 => {
                    DVec3::Y
                }
                None => DVec3::Z,
            };
            Some(snap_frame(d, up).ok_or_else(|| arg_err("direction and up can't be parallel"))?)
        }
    };
    if op == "snap.add" {
        let ctx = ctx_of(a)?;
        let (dir, up) = frame.ok_or("direction attendue")?;
        if ents(c.model, ctx).is_none() {
            return Err("contexte supprimé".into());
        }
        let id = c.model.add_snap(ctx, esq_core::model::SnapPoint::new(pos, dir, up));
        return Ok(json!(handle('n', ctx, u64::from(id))));
    }
    let h = h_of(a, "h")?;
    let en = ents_mut(c.model, h.ctx).ok_or_else(|| deleted('n'))?;
    let i = snap_ix(en, h.id).ok_or_else(|| deleted('n'))?;
    let s = &mut en.snaps[i];
    s.pos = pos;
    if let Some((dir, up)) = frame {
        s.dir = dir;
        s.up = up;
    }
    Ok(J::Null)
}

// ---------------------------------------------------------------- images

/// The instance an image handle names, checked to be an image.
fn image_ref(m: &Model, h: &H) -> Result<(Option<DefId>, InstanceId), String> {
    let id = InstanceId::from(KeyData::from_ffi(h.id));
    if h.kind != 'i' || m.image_of(h.ctx, id).is_none() {
        return Err("TypeError: reference to deleted Image".into());
    }
    Ok((h.ctx, id))
}

fn image_op(c: &mut Ctx, op: &str, a: &J) -> R {
    if op == "image.add" {
        // Entities#add_image: the file's picture, one inch per pixel, scaled to `width`.
        let ctx = ctx_of(a)?;
        let path = str_of(a, "path")?;
        let Ok(bytes) = std::fs::read(path) else { return Ok(J::Null) };
        let file = std::path::Path::new(path);
        let name = file.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let Ok(tex) = super::model::images::texture_from_bytes(&name, bytes, None) else { return Ok(J::Null) };
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let width = a["width"].as_f64().ok_or("largeur attendue")? * INCH;
        let height = a.get("height").and_then(J::as_f64).map(|h| h * INCH).filter(|h| h.abs() > 1e-9);
        // A null side follows the picture's proportions.
        let (width, height) = match height {
            Some(h) if width.abs() < 1e-9 => (h * tex.px[0] as f64 / tex.px[1].max(1) as f64, Some(h)),
            _ => (width, height),
        };
        let origin = pt(&a["origin"])?;
        if ents(c.model, ctx).is_none() {
            return Err("contexte supprimé".into());
        }
        return Ok(match c.model.add_image(ctx, &stem, path, tex, origin, DVec3::X, DVec3::Z, width, height) {
            Some(i) => json!(handle('i', ctx, ffi(i))),
            None => J::Null,
        });
    }
    let h = h_of(a, "h")?;
    let (ctx, id) = image_ref(c.model, &h)?;
    match op {
        "image.get" => {
            let v = c.model.image_of(ctx, id).unwrap();
            Ok(json!({
                "path": v.path, "px": v.px, "width": inches(v.width), "height": inches(v.height),
                "origin": out_pt(v.origin), "xaxis": out_vec(v.xaxis), "yaxis": out_vec(v.yaxis),
                "normal": out_vec(v.normal), "xform": mat_to(&v.xform),
                "zrotation": v.xaxis.y.atan2(v.xaxis.x) + 0.0,
                "material": super::mat_name(c.model, v.material),
            }))
        }
        "image.size" => {
            let w = a["width"].as_f64().ok_or("largeur attendue")? * INCH;
            let hh = a["height"].as_f64().ok_or("hauteur attendue")? * INCH;
            Ok(json!(c.model.set_image_size(ctx, id, w, hh)))
        }
        "image.origin" => Ok(json!(c.model.set_image_origin(ctx, id, pt(&a["p"])?))),
        "image.xform" => {
            let m = mat_from(&a["m"])?;
            Ok(json!(c.model.set_image_transform(ctx, id, m)))
        }
        "image.rep" => {
            let v = c.model.image_of(ctx, id).unwrap();
            let tex = v.material.and_then(|m| c.model.materials.get(m)).and_then(|m| m.ext.texture.as_ref());
            Ok(match tex {
                Some(t) if !t.image.0.is_empty() => json!(esq_core::photo::base64_encode(&t.image.0)),
                _ => J::Null,
            })
        }
        _ => Err(format!("opération inconnue: {op}")),
    }
}
