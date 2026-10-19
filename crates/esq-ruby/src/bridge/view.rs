//! Pont natif de la zone « view » : scènes (`Sketchup::Page`), ombres (`ShadowInfo`) et l'état
//! libre du modèle (`Model::view_ext`) où Ruby range les styles, environnements, options de rendu
//! et espaces. La caméra et la fenêtre 3D, elles, passent par l'hôte (`host.view.*`).
//!
//! Les longueurs franchissent le pont en pouces (voir `bridge/mod.rs`).

use esq_core::model::{Model, Scene, SceneCamera};
use serde_json::{Value as J, json};

use super::{Ctx, R, out_pt, pt, str_of};

pub fn dispatch(c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    Some(match op {
        "scene.list" => scene_list(c.model),
        "scene.add" => scene_add(c.model, a),
        "scene.set" => scene_set(c.model, a),
        "scene.remove" => scene_remove(c.model, a),
        "scene.move" => scene_move(c.model, a),
        "shadow.get" => Ok(shadow_get(c.model)),
        "shadow.set" => shadow_set(c.model, a),
        "vx.get" => str_of(a, "k").map(|k| c.model.view_ext.get(k).cloned().unwrap_or(J::Null)),
        "vx.set" => str_of(a, "k").map(|k| {
            c.model.view_ext.insert(k.to_string(), a.get("v").cloned().unwrap_or(J::Null));
            J::Null
        }),
        "vx.delete" => str_of(a, "k").map(|k| c.model.view_ext.remove(k).unwrap_or(J::Null)),
        "pick.infer" => pick_infer(c, a),
        "pick.list" => pick_list(c, "view.pick_raw", a),
        "pick.rect" => pick_list(c, "view.pick_rect_raw", a),
        "pick.bbox" => pick_bbox(c.model, a),
        "hlr.count" => {
            let mut n = 0usize;
            esq_core::walk::visit(c.model, |p| n += p.ents.faces.len());
            Ok(json!(n))
        }
        _ => return None,
    })
}

// ------------------------------------------------------------------------------ sélection par pointage

/// Poignées « i:ctx:id » du chemin d'instances (identifiants bruts) et définition atteinte.
fn path_handles(m: &Model, path: &[J]) -> Option<(Vec<String>, Option<esq_core::model::DefId>)> {
    use slotmap::{Key, KeyData};
    let mut ctx = None;
    let mut out = Vec::new();
    for p in path {
        let raw = p.as_u64()?;
        let id = esq_core::model::InstanceId::from(KeyData::from_ffi(raw));
        let inst = m.entities(ctx).instances.get(id)?;
        out.push(super::handle('i', ctx, id.data().as_ffi()));
        ctx = Some(inst.def);
    }
    Some((out, ctx))
}

/// `{"path":[…], "kind":"e", "id":n}` de l'hôte → (chemin, poignée de la feuille).
fn hit_handles(m: &Model, hit: &J) -> Option<(Vec<String>, String)> {
    let (path, ctx) = path_handles(m, hit["path"].as_array()?)?;
    let kind = hit["kind"].as_str()?.chars().next()?;
    Some((path, super::handle(kind, ctx, hit["id"].as_u64()?)))
}

fn pick_infer(c: &mut Ctx, a: &J) -> R {
    let ref_mm = a.get("ref").filter(|r| !r.is_null()).map(pt).transpose()?;
    let args = json!({"x": a["x"], "y": a["y"], "ref": ref_mm.map(|r| json!([r.x, r.y, r.z]))});
    let r = crate::host::call(c.model, c.sel, "view.infer_raw", &args)?;
    if r.is_null() {
        return Ok(J::Null);
    }
    let pos = r["pos"].as_array().filter(|p| p.len() == 3).ok_or("position attendue")?;
    let p = esq_core::glam::DVec3::new(
        pos[0].as_f64().unwrap_or(0.0),
        pos[1].as_f64().unwrap_or(0.0),
        pos[2].as_f64().unwrap_or(0.0),
    );
    let mut face = J::Null;
    let mut edge = J::Null;
    let mut path: Vec<String> = Vec::new();
    for (key, out) in [("face", &mut face), ("edge", &mut edge)] {
        if let Some((pa, leaf)) = r.get(key).filter(|h| !h.is_null()).and_then(|h| hit_handles(c.model, h)) {
            if path.is_empty() || key == "face" && out.is_null() {
                path = pa;
            }
            *out = json!(leaf);
        }
    }
    let vertex = if matches!(r["kind"].as_str(), Some("endpoint")) { edge.clone() } else { J::Null };
    Ok(json!({
        "pos": out_pt(p), "kind": r["kind"], "tip": r["tip"], "dof": r["dof"],
        "path": path, "face": face, "edge": edge, "vertex": vertex,
    }))
}

fn pick_list(c: &mut Ctx, op: &str, a: &J) -> R {
    let r = crate::host::call(c.model, c.sel, op, a)?;
    let mut out = Vec::new();
    for h in r.as_array().cloned().unwrap_or_default() {
        if let Some((path, leaf)) = hit_handles(c.model, &h) {
            out.push(json!({"path": path, "leaf": leaf}));
        }
    }
    Ok(J::Array(out))
}

/// Arêtes et faces dont la géométrie (monde) est dans la boîte, ou la touche si `crossing`.
fn pick_bbox(m: &Model, a: &J) -> R {
    use slotmap::Key;
    let lo = pt(&a["min"])?;
    let hi = pt(&a["max"])?;
    let crossing = a.get("crossing").and_then(J::as_bool).unwrap_or(false);
    let inside = |q: esq_core::glam::DVec3| q.cmpge(lo).all() && q.cmple(hi).all();
    let mut out = Vec::new();
    esq_core::walk::visit(m, |p| {
        let ctx = p.def;
        let test = |pts: &[esq_core::glam::DVec3]| {
            let w: Vec<_> = pts.iter().map(|q| p.xform.transform_point3(*q)).collect();
            if crossing { w.iter().any(|q| inside(*q)) } else { w.iter().all(|q| inside(*q)) }
        };
        let path: Vec<String> = {
            let mut ctx2 = None;
            let mut v = Vec::new();
            for &i in &p.path {
                v.push(super::handle('i', ctx2, i.data().as_ffi()));
                ctx2 = m.entities(ctx2).instances.get(i).map(|x| x.def);
            }
            v
        };
        for (id, _) in p.ents.edges.iter() {
            let (s, e) = p.ents.edge_points(id);
            if test(&[s, e]) {
                out.push(json!({"path": path, "leaf": super::handle('e', ctx, id.data().as_ffi())}));
            }
        }
        for (id, f) in p.ents.faces.iter() {
            if test(&p.ents.loop_points(&f.outer)) {
                out.push(json!({"path": path, "leaf": super::handle('f', ctx, id.data().as_ffi())}));
            }
        }
    });
    Ok(J::Array(out))
}

// ------------------------------------------------------------------------------ scenes

fn camera_json(c: &SceneCamera) -> J {
    json!({
        "eye": out_pt(c.eye), "target": out_pt(c.target),
        "up": [c.up.x, c.up.y, c.up.z], "fov": c.fov_deg, "persp": c.perspective,
    })
}

fn camera_from(j: &J) -> Result<SceneCamera, String> {
    let up = j["up"].as_array().filter(|u| u.len() == 3).ok_or("caméra : vecteur « up » attendu")?;
    let f = |i: usize| up[i].as_f64().unwrap_or(0.0);
    Ok(SceneCamera {
        eye: pt(&j["eye"])?,
        target: pt(&j["target"])?,
        up: esq_core::glam::DVec3::new(f(0), f(1), f(2)),
        fov_deg: j["fov"].as_f64().unwrap_or(35.0),
        perspective: j["persp"].as_bool().unwrap_or(true),
    })
}

fn scene_json(m: &Model, s: &Scene) -> J {
    let hidden = s
        .hidden_layers
        .as_ref()
        .map(|h| h.iter().filter_map(|&i| m.layers.get(i)).map(|l| json!(l.name)).collect::<Vec<_>>());
    json!({
        "uid": s.ext.uid, "name": s.name, "description": s.description,
        "camera": camera_json(&s.camera), "hidden_layers": hidden,
        "props": s.ext.props,
    })
}

fn scene_list(m: &mut Model) -> R {
    m.ensure_scene_uids();
    Ok(J::Array(m.scenes.iter().map(|s| scene_json(m, s)).collect()))
}

fn layer_indices(m: &Model, names: &J) -> Result<Option<Vec<usize>>, String> {
    let Some(arr) = names.as_array() else { return Ok(None) };
    let mut out = Vec::new();
    for n in arr {
        let n = n.as_str().ok_or("nom d'étiquette attendu")?;
        out.push(m.layers.iter().position(|l| l.name == n).ok_or_else(|| format!("étiquette inconnue: {n}"))?);
    }
    Ok(Some(out))
}

fn find(m: &mut Model, a: &J) -> Result<usize, String> {
    m.ensure_scene_uids();
    let uid = a["uid"].as_u64().ok_or("uid de scène attendu")?;
    m.scenes.iter().position(|s| s.ext.uid == uid).ok_or_else(|| "scène supprimée".to_string())
}

fn props_merge(s: &mut Scene, patch: &J) {
    if let Some(p) = patch.as_object() {
        for (k, v) in p {
            if v.is_null() {
                s.ext.props.remove(k);
            } else {
                s.ext.props.insert(k.clone(), v.clone());
            }
        }
    }
}

fn scene_add(m: &mut Model, a: &J) -> R {
    m.ensure_scene_uids();
    let name = match a.get("name").and_then(J::as_str) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => m.next_scene_name(),
    };
    let camera = camera_from(a.get("camera").ok_or("caméra attendue")?)?;
    let hidden_layers = match a.get("hidden_layers") {
        Some(h) if !h.is_null() => layer_indices(m, h)?,
        _ => None,
    };
    let uid = m.scenes.iter().map(|s| s.ext.uid).max().unwrap_or(0) + 1;
    let mut s = Scene {
        name,
        description: a.get("description").and_then(J::as_str).unwrap_or("").to_string(),
        camera,
        hidden_layers,
        photo: None,
        ext: Default::default(),
    };
    s.ext.uid = uid;
    props_merge(&mut s, &a["props"]);
    let at = a.get("index").and_then(J::as_u64).map_or(m.scenes.len(), |i| (i as usize).min(m.scenes.len()));
    m.scenes.insert(at, s);
    Ok(json!(uid))
}

fn scene_set(m: &mut Model, a: &J) -> R {
    let i = find(m, a)?;
    let hidden = match a.get("hidden_layers") {
        None => None,
        Some(h) if h.is_null() => Some(None),
        Some(h) => Some(layer_indices(m, h)?),
    };
    let cam = a.get("camera").filter(|c| !c.is_null()).map(camera_from).transpose()?;
    let s = &mut m.scenes[i];
    if let Some(n) = a.get("name").and_then(J::as_str) {
        s.name = n.to_string();
    }
    if let Some(d) = a.get("description").and_then(J::as_str) {
        s.description = d.to_string();
    }
    if let Some(c) = cam {
        s.camera = c;
    }
    if let Some(h) = hidden {
        s.hidden_layers = h;
    }
    props_merge(s, &a["props"]);
    Ok(J::Null)
}

fn scene_remove(m: &mut Model, a: &J) -> R {
    let i = find(m, a)?;
    m.scenes.remove(i);
    Ok(json!(i))
}

fn scene_move(m: &mut Model, a: &J) -> R {
    let i = find(m, a)?;
    let to = a["index"].as_u64().ok_or("index attendu")? as usize;
    if to >= m.scenes.len() {
        return Err("index invalide".into());
    }
    let s = m.scenes.remove(i);
    m.scenes.insert(to, s);
    Ok(J::Null)
}

// ------------------------------------------------------------------------------ ombres

/// Heure UTC réelle (secondes depuis 1970) de l'heure locale stockée par le modèle.
fn utc_of(m: &Model) -> i64 {
    m.shadow.time - (m.shadow.tz_offset * 3600.0).round() as i64
}

fn shadow_get(m: &Model) -> J {
    let s = &m.shadow;
    let sun = s.sun_direction();
    let day = s.time.div_euclid(86_400) * 86_400;
    let (rise, set) = s.sunrise_sunset().unwrap_or((0.0, 0.0));
    let at = |minutes: f64| day + (minutes * 60.0).round() as i64 - (s.tz_offset * 3600.0).round() as i64;
    let ext = |k: &str| m.view_ext.get("shadow").and_then(|e| e.get(k)).and_then(J::as_bool).unwrap_or(false);
    json!({
        "City": s.city, "Country": s.country, "Dark": s.dark,
        "DayOfYear": s.day_of_year(),
        "DaylightSavings": ext("DaylightSavings"),
        "DisplayNorth": ext("DisplayNorth"),
        "DisplayOnAllFaces": s.on_faces, "DisplayOnGroundPlane": s.on_ground,
        "DisplayShadows": s.display, "EdgesCastShadows": s.from_edges,
        "Latitude": s.latitude, "Light": s.light, "Longitude": s.longitude,
        "NorthAngle": s.north_angle,
        "ShadowTime_time_t": utc_of(m),
        "SunDirection": [sun.x, sun.y, sun.z],
        "SunRise_time_t": at(rise), "SunSet_time_t": at(set),
        "TZOffset": s.tz_offset, "UseSunForAllShading": s.use_sun_for_shading,
    })
}

/// The shadow settings a scene saves (`Page#update`, `ShadowInfo#__snapshot`): those of
/// `ShadowInfo` without the computed ones.
pub fn shadow_snapshot(m: &Model) -> J {
    let mut j = shadow_get(m);
    if let Some(o) = j.as_object_mut() {
        for k in ["DayOfYear", "SunDirection", "SunRise_time_t", "SunSet_time_t"] {
            o.remove(k);
        }
    }
    j
}

/// Puts back shadow settings saved by [`shadow_snapshot`].
pub fn shadow_restore(m: &mut Model, h: &J) {
    for (k, v) in h.as_object().into_iter().flatten() {
        let _ = shadow_set(m, &json!({"k": k, "v": v}));
    }
}

fn shadow_set(m: &mut Model, a: &J) -> R {
    let k = str_of(a, "k")?;
    let v = &a["v"];
    let num = || v.as_f64().ok_or_else(|| format!("{k} : nombre attendu"));
    let flag = || v.as_bool().ok_or_else(|| format!("{k} : booléen attendu"));
    let s = &mut m.shadow;
    match k {
        "City" => s.city = v.as_str().ok_or("City : texte attendu")?.to_string(),
        "Country" => s.country = v.as_str().ok_or("Country : texte attendu")?.to_string(),
        "Dark" => s.dark = num()?.round().clamp(0.0, 100.0) as i32,
        "Light" => s.light = num()?.round().clamp(0.0, 100.0) as i32,
        "DisplayOnAllFaces" => s.on_faces = flag()?,
        "DisplayOnGroundPlane" => s.on_ground = flag()?,
        "DisplayShadows" => s.display = flag()?,
        "EdgesCastShadows" => s.from_edges = flag()?,
        "UseSunForAllShading" => s.use_sun_for_shading = flag()?,
        "Latitude" => s.latitude = num()?.clamp(-90.0, 90.0),
        "Longitude" => s.longitude = num()?.clamp(-180.0, 180.0),
        "NorthAngle" => s.north_angle = num()?,
        "TZOffset" => {
            // L'heure UTC reste la même : l'heure locale suit le fuseau.
            let utc = s.time - (s.tz_offset * 3600.0).round() as i64;
            s.tz_offset = num()?.clamp(-12.0, 14.0);
            s.time = utc + (s.tz_offset * 3600.0).round() as i64;
        }
        "ShadowTime_time_t" | "ShadowTime" => {
            let utc = v.as_f64().ok_or("ShadowTime : heure attendue")?.round() as i64;
            s.time = utc + (s.tz_offset * 3600.0).round() as i64;
        }
        "DisplayNorth" | "DaylightSavings" => {
            let on = flag()?;
            let e = m.view_ext.entry("shadow".to_string()).or_insert_with(|| json!({}));
            e[k] = json!(on);
        }
        // Valeurs calculées (DayOfYear, SunDirection, SunRise…) : lecture seule.
        _ => return Ok(json!(false)),
    }
    Ok(json!(true))
}
