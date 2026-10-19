//! Matières : `Sketchup::Material`, `Materials`, textures et réglages PBR.

use esq_core::meta::{MaterialExt, TextureData};
use esq_core::model::{Entities, Material, MaterialId, Model};
use serde_json::{Value as J, json};
use slotmap::{Key, KeyData};

use super::images::{texture_from_bytes, texture_from_file};
use super::{b64, id_of, raise, unb64};
use crate::bridge::{Ctx, R};

pub fn run(c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    Some(match op {
        "mat.list" => Ok(json!(c.model.materials.iter().map(|(id, _)| summary(c.model, id)).collect::<Vec<_>>())),
        "mat.get" => get(c, a).map(|id| summary(c.model, id)),
        "mat.add" => add(c, a),
        "mat.update" => update(c, a),
        "mat.remove" => remove(c, a),
        "mat.purge" => Ok(json!(purge(c.model))),
        "mat.duplicate" => duplicate(c, a),
        "mat.tex_set" => tex_set(c, a),
        "mat.tex_size" => tex_size(c, a),
        "mat.tex_data" => tex_data(c, a),
        "mat.current" => Ok(match c.model.meta.current_material.filter(|m| c.model.materials.contains_key(*m)) {
            Some(id) => summary(c.model, id),
            None => J::Null,
        }),
        "mat.set_current" => set_current(c, a),
        "mat.export" => export(c, a),
        // The material as data, for the application to write a .skm of it.
        "mat.dump" => get(c, a).and_then(|id| serde_json::to_value(&c.model.materials[id]).map_err(|e| e.to_string())),
        "mat.import" => import(c, a),
        "mat.unique_name" => Ok(json!(unique(c.model, a["name"].as_str().unwrap_or("Material"), None))),
        "mat.use_count" => Ok(json!(use_count(c.model, get(c, a).ok()))),
        _ => return None,
    })
}

pub fn mid(id: MaterialId) -> String {
    id.data().as_ffi().to_string()
}

fn get(c: &Ctx, a: &J) -> Result<MaterialId, String> {
    let id = MaterialId::from(KeyData::from_ffi(id_of(a, "id")?));
    if c.model.materials.contains_key(id) { Ok(id) } else { Err(raise("TypeError", "reference to deleted Material")) }
}

fn tex_json(t: &TextureData) -> J {
    json!({
        "filename": t.filename, "w": t.px[0], "h": t.px[1],
        "width": t.size[0] / 25.4, "height": t.size[1] / 25.4, "avg": t.avg,
    })
}

fn opt_tex(t: &Option<TextureData>) -> J {
    t.as_ref().map_or(J::Null, tex_json)
}

pub fn summary(m: &Model, id: MaterialId) -> J {
    let x = &m.materials[id];
    let e = &x.ext;
    let ty = match (&e.texture, e.colorize_deltas != [0.0; 3]) {
        (None, _) => 0,
        (Some(_), false) => 1,
        (Some(_), true) => 2,
    };
    let color = match &e.texture {
        Some(t) if ty == 1 => t.avg,
        _ => x.color,
    };
    json!({
        "id": mid(id), "name": x.name, "color": color, "alpha": x.opacity as f64, "type": ty,
        "colorize_type": e.colorize_type, "colorize_deltas": e.colorize_deltas,
        "hatch": e.hatch, "workflow": e.workflow, "owner_image": e.owner_image,
        "texture": opt_tex(&e.texture),
        "pbr": {
            "metalness_enabled": e.pbr.metalness_enabled, "metallic_factor": e.pbr.metallic_factor,
            "metallic_texture": opt_tex(&e.pbr.metallic_texture),
            "roughness_enabled": e.pbr.roughness_enabled, "roughness_factor": e.pbr.roughness_factor,
            "roughness_texture": opt_tex(&e.pbr.roughness_texture),
            "normal_enabled": e.pbr.normal_enabled, "normal_scale": e.pbr.normal_scale,
            "normal_style": e.pbr.normal_style, "normal_texture": opt_tex(&e.pbr.normal_texture),
            "ao_enabled": e.pbr.ao_enabled, "ao_strength": e.pbr.ao_strength,
            "ao_texture": opt_tex(&e.pbr.ao_texture),
        },
    })
}

/// `CMaterialManager::GenerateUniqueName`, behind `Materials#unique_name` and `#add`.
fn unique(m: &Model, base: &str, except: Option<MaterialId>) -> String {
    let taken: Vec<String> =
        m.materials.iter().filter(|(k, _)| Some(*k) != except).map(|(_, x)| x.name.clone()).collect();
    Model::unique_material_name(base, &taken)
}

fn add(c: &mut Ctx, a: &J) -> R {
    let name = match a.get("name").and_then(J::as_str) {
        Some(n) => unique(c.model, n, None),
        None => default_name(c.model),
    };
    let id = c.model.add_material(&name, [204, 204, 204]);
    Ok(summary(c.model, id))
}

/// "Material1", "Material2"…: the name SketchUp gives a material created without one.
fn default_name(m: &Model) -> String {
    (1..).map(|i| format!("Material{i}")).find(|n| m.find_material(n).is_none()).unwrap()
}

fn hls(rgb: [u8; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(|v| v as f64 / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let l = (mx + mn) / 2.0;
    if mx == mn {
        return [0.0, 0.0, l];
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if mx == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [h * 60.0, s, l]
}

fn set_tex_slot(e: &mut MaterialExt, slot: &str, t: Option<TextureData>) -> Result<(), String> {
    match slot {
        "base" => e.texture = t,
        "metallic" => e.pbr.metallic_texture = t,
        "roughness" => e.pbr.roughness_texture = t,
        "normal" => e.pbr.normal_texture = t,
        "ao" => e.pbr.ao_texture = t,
        s => return Err(format!("emplacement de texture inconnu: {s}")),
    }
    Ok(())
}

fn tex_slot<'a>(e: &'a MaterialExt, slot: &str) -> Option<&'a TextureData> {
    match slot {
        "base" => e.texture.as_ref(),
        "metallic" => e.pbr.metallic_texture.as_ref(),
        "roughness" => e.pbr.roughness_texture.as_ref(),
        "normal" => e.pbr.normal_texture.as_ref(),
        "ao" => e.pbr.ao_texture.as_ref(),
        _ => None,
    }
}

fn update(c: &mut Ctx, a: &J) -> R {
    let id = get(c, a)?;
    if let Some(n) = a.get("name").and_then(J::as_str) {
        if c.model.materials.iter().any(|(k, m)| k != id && m.name == n) {
            return Err(raise("ArgumentError", "The provided name was not unique to the model."));
        }
        let old = std::mem::replace(&mut c.model.materials[id].name, n.to_string());
        c.model.rename_attr_owner(&format!("mat:{old}"), &format!("mat:{n}"));
    }
    let m = &mut c.model.materials[id];
    if let Some(col) = a.get("color").and_then(J::as_array)
        && col.len() >= 3
    {
        let rgb: [u8; 3] = std::array::from_fn(|i| col[i].as_u64().unwrap_or(0).min(255) as u8);
        if let Some(t) = &m.ext.texture {
            // A colour set on a textured material colorizes the texture.
            let (a, b) = (hls(t.avg), hls(rgb));
            m.ext.colorize_deltas = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            if m.ext.colorize_deltas == [0.0; 3] {
                m.ext.colorize_deltas = [0.0, 0.0, f64::MIN_POSITIVE];
            }
        }
        m.color = rgb;
    }
    if let Some(al) = a.get("alpha").and_then(J::as_f64) {
        m.opacity = al.clamp(0.0, 1.0) as f32;
    }
    if let Some(v) = a.get("colorize_type").and_then(J::as_u64) {
        m.ext.colorize_type = v as u8;
    }
    if let Some(v) = a.get("hatch") {
        m.ext.hatch = v.as_u64().map(|u| u as u32);
    }
    if let Some(p) = a.get("pbr").and_then(J::as_object) {
        let e = &mut m.ext.pbr;
        let b = |k: &str| p.get(k).and_then(J::as_bool);
        let f = |k: &str| p.get(k).and_then(J::as_f64);
        if let Some(v) = b("metalness_enabled") {
            e.metalness_enabled = v;
        }
        if let Some(v) = f("metallic_factor") {
            e.metallic_factor = v;
        }
        if let Some(v) = b("roughness_enabled") {
            e.roughness_enabled = v;
        }
        if let Some(v) = f("roughness_factor") {
            e.roughness_factor = v;
        }
        if let Some(v) = b("normal_enabled") {
            e.normal_enabled = v;
        }
        if let Some(v) = f("normal_scale") {
            e.normal_scale = v;
        }
        if let Some(v) = f("normal_style") {
            e.normal_style = v as u8;
        }
        if let Some(v) = b("ao_enabled") {
            e.ao_enabled = v;
        }
        if let Some(v) = f("ao_strength") {
            e.ao_strength = v;
        }
        m.ext.workflow = 1;
    }
    Ok(summary(c.model, id))
}

/// Entities that use material `id` revert to the default material.
fn forget(e: &mut Entities, id: MaterialId) {
    for f in e.faces.values_mut() {
        if f.front == Some(id) {
            f.front = None;
        }
        if f.back == Some(id) {
            f.back = None;
        }
    }
    for i in e.instances.values_mut() {
        if i.material == Some(id) {
            i.material = None;
        }
    }
}

fn remove(c: &mut Ctx, a: &J) -> R {
    let id = get(c, a)?;
    forget(&mut c.model.root, id);
    for d in c.model.defs.values_mut() {
        forget(&mut d.entities, id);
    }
    if let Some(m) = c.model.materials.remove(id) {
        c.model.attr_store.remove(&format!("mat:{}", m.name));
    }
    if c.model.meta.current_material == Some(id) {
        c.model.meta.current_material = None;
    }
    Ok(json!(true))
}

fn use_count(m: &Model, id: Option<MaterialId>) -> usize {
    let Some(id) = id else { return 0 };
    let count = |e: &Entities| {
        e.faces.values().filter(|f| f.front == Some(id) || f.back == Some(id)).count()
            + e.instances.values().filter(|i| i.material == Some(id)).count()
    };
    count(&m.root) + m.defs.values().map(|d| count(&d.entities)).sum::<usize>()
}

fn purge(m: &mut Model) -> usize {
    let unused: Vec<MaterialId> = m.materials.keys().filter(|&k| use_count(m, Some(k)) == 0).collect();
    for k in &unused {
        if let Some(x) = m.materials.remove(*k) {
            m.attr_store.remove(&format!("mat:{}", x.name));
        }
        if m.meta.current_material == Some(*k) {
            m.meta.current_material = None;
        }
    }
    unused.len()
}

fn duplicate(c: &mut Ctx, a: &J) -> R {
    let id = get(c, a)?;
    let src = c.model.materials[id].clone();
    let name = unique(c.model, &src.name, None);
    if let Some(attrs) = c.model.attr_store.get(&format!("mat:{}", src.name)).cloned() {
        c.model.attr_store.insert(format!("mat:{name}"), attrs);
    }
    let new = c.model.materials.insert(Material { name, ..src });
    Ok(summary(c.model, new))
}

fn tex_set(c: &mut Ctx, a: &J) -> R {
    let id = get(c, a)?;
    let slot = a["slot"].as_str().unwrap_or("base");
    let w = a.get("w").and_then(J::as_f64).filter(|v| *v > 0.0);
    let h = a.get("h").and_then(J::as_f64).filter(|v| *v > 0.0);
    let both = w.zip(h).map(|(w, h)| [w, h]);
    let tex = if let Some(p) = a.get("path").and_then(J::as_str) {
        Some(texture_from_file(p, both)?)
    } else if let Some(d) = a.get("data").and_then(J::as_str) {
        let name = a.get("name").and_then(J::as_str).unwrap_or("image.png");
        Some(texture_from_bytes(name, unb64(d)?, both)?)
    } else {
        None
    };
    // `Material#texture=`: a file without size is 10" wide, and
    // with a width alone the image keeps its proportions.
    let from_file = a.get("path").is_some();
    let tex = tex.map(|mut t| {
        let width = match (w, h) {
            (Some(w), None) => Some(w),
            (None, None) if from_file => Some(10.0),
            _ => None,
        };
        if let Some(w) = width {
            t.size = [w * 25.4, w * 25.4 * t.px[1] as f64 / t.px[0].max(1) as f64];
        }
        t
    });
    let m = &mut c.model.materials[id];
    if slot == "base" {
        match &tex {
            Some(t) => m.color = t.avg,
            None => m.ext.colorize_deltas = [0.0; 3],
        }
    }
    set_tex_slot(&mut m.ext, slot, tex)?;
    Ok(summary(c.model, id))
}

fn tex_size(c: &mut Ctx, a: &J) -> R {
    let id = get(c, a)?;
    let t = c.model.materials[id].ext.texture.as_mut().ok_or("pas de texture")?;
    let (w, h) = (a["w"].as_f64().unwrap_or(0.0), a["h"].as_f64().unwrap_or(0.0));
    t.size = [w * 25.4, h * 25.4];
    Ok(summary(c.model, id))
}

fn tex_data(c: &mut Ctx, a: &J) -> R {
    let id = get(c, a)?;
    let slot = a["slot"].as_str().unwrap_or("base");
    match tex_slot(&c.model.materials[id].ext, slot) {
        Some(t) => Ok(json!({"filename": t.filename, "data": b64(&t.image.0)})),
        None => Ok(J::Null),
    }
}

fn set_current(c: &mut Ctx, a: &J) -> R {
    match a.get("id") {
        None | Some(J::Null) => c.model.meta.current_material = None,
        _ => {
            let id = get(c, a)?;
            c.model.meta.current_material = Some(id);
        }
    }
    Ok(J::Null)
}

/// `Material#save_as`: a self-contained `.skm`-like file (JSON, images in base64).
fn export(c: &mut Ctx, a: &J) -> R {
    let id = get(c, a)?;
    let path = a["path"].as_str().ok_or("chemin manquant")?;
    let m = &c.model.materials[id];
    let doc = json!({"esquisse_material": 1, "material": serde_json::to_value(m).map_err(|e| e.to_string())?});
    Ok(json!(std::fs::write(path, doc.to_string()).is_ok()))
}

/// `Materials#load`: the material the application read from a .skm (`material`), else a file
/// of Esquisse's earlier format (JSON, `path`).
fn import(c: &mut Ctx, a: &J) -> R {
    let fail = || raise("RuntimeError", "failed to import material to model");
    let value = match a.get("material").filter(|v| !v.is_null()) {
        Some(v) => v.clone(),
        None => {
            let path = a["path"].as_str().ok_or("chemin manquant")?;
            let text = std::fs::read_to_string(path).map_err(|_| fail())?;
            let doc: J = serde_json::from_str(&text).map_err(|_| fail())?;
            doc["material"].clone()
        }
    };
    let mut m: Material = serde_json::from_value(value).map_err(|_| fail())?;
    m.name = unique(c.model, &m.name, None);
    m.ext.sanitize_texture_names();
    let id = c.model.materials.insert(m);
    Ok(summary(c.model, id))
}
