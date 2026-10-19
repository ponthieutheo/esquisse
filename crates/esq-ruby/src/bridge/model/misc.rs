//! Réglages du modèle (nom, description, options), motifs de hachure, lancer de rayon,
//! chemin actif.

use esq_core::glam::{DMat4, DVec3};
use esq_core::meta::HatchPattern;
use esq_core::model::{AnnotKind, Annotation, DefId, EntityRef, InstanceId, Model, Units};
use serde_json::{Value as J, json};
use slotmap::{Key, KeyData};

use super::{b64, id_of, raise};
use crate::bridge::{Ctx, INCH, R, def_str, handle, parse_ctx, parse_h, pt};

pub fn run(c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    Some(match op {
        "meta.get" => Ok(meta(c)),
        "meta.set" => set_meta(c, a),
        "meta.raytest" => raytest(c, a),
        "meta.add_note" => add_note(c, a),
        "meta.regional" => Ok(json!({"decimal": locale_info(0x0E, "."), "list": locale_info(0x0C, ",")})),
        // Sketchup.break_edges? / break_edges= : the switch of the geometry engine.
        "meta.break_edges" => {
            if let Some(on) = a.get("on").and_then(J::as_bool) {
                esq_core::ops::set_break_edges(on);
            }
            Ok(json!(esq_core::ops::break_edges()))
        }
        // Sketchup::Skp.read_guid: the GUID saved in the file, or "invalid model".
        "meta.read_guid" => {
            let path = std::path::Path::new(a["path"].as_str().unwrap_or_default());
            match esq_skp::read_guid(path) {
                Ok(Some(g)) => Ok(json!(g)),
                Ok(None) => Err(raise("ArgumentError", "invalid model")),
                Err(_) => Err(raise("RuntimeError", "unknown error")),
            }
        }
        "opt.providers" => Ok(json!(c.model.meta.options.keys().collect::<Vec<_>>())),
        "opt.get" => option_get(c, a),
        "opt.set" => option_set(c, a),
        "hatch.list" => Ok(json!(c.model.meta.hatch_patterns.iter().map(hatch_json).collect::<Vec<_>>())),
        "hatch.add" => hatch_add(c, a),
        "hatch.update" => hatch_update(c, a),
        "hatch.remove" => hatch_remove(c, a),
        "hatch.purge" => Ok(json!(hatch_purge(c.model))),
        "hatch.write" => hatch_write(c, a),
        _ => return None,
    })
}

/// A text of the user's regional settings (`LOCALE_SDECIMAL` = 0x0E, `LOCALE_SLIST` = 0x0C).
#[cfg(windows)]
fn locale_info(kind: u32, fallback: &str) -> String {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocaleInfoEx(locale: *const u16, lctype: u32, data: *mut u16, len: i32) -> i32;
    }
    let mut buf = [0u16; 16];
    // SAFETY: a null locale name means the user's default locale; the buffer and its length
    // (in UTF-16 units) are passed together.
    let n = unsafe { GetLocaleInfoEx(std::ptr::null(), kind, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 1 {
        return fallback.into();
    }
    String::from_utf16_lossy(&buf[..n as usize - 1])
}

#[cfg(not(windows))]
fn locale_info(_kind: u32, fallback: &str) -> String {
    fallback.into()
}

/// The instance path that is open for editing, as `(handle, tag index)` pairs.
pub fn active_path(c: &mut Ctx) -> Vec<(String, usize)> {
    let Ok(r) = crate::host::call(c.model, c.sel, "model.path", &json!({})) else { return Vec::new() };
    r["path"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|h| {
                    let h = h.as_str()?;
                    let p = parse_h(h).ok()?;
                    let id = InstanceId::from(KeyData::from_ffi(p.id));
                    let layer = c.model.entities(p.ctx).instances.get(id)?.layer;
                    Some((h.to_string(), layer))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Definition of the active editing context (None = the model root).
pub fn active_def(c: &mut Ctx) -> Option<DefId> {
    let r = crate::host::call(c.model, c.sel, "model.path", &json!({})).ok()?;
    parse_ctx(r["ctx"].as_str().unwrap_or("0")).ok().flatten()
}

fn unit_code(u: Units) -> i64 {
    u.code()
}

fn unit_from(code: i64) -> Option<Units> {
    Units::from_code(code)
}

fn faces_of(m: &Model) -> usize {
    m.root.faces.len() + m.defs.values().map(|d| d.entities.faces.len()).sum::<usize>()
}

fn meta(c: &mut Ctx) -> J {
    c.model.ensure_ruby_ids();
    let m = &c.model.meta;
    let s = &c.model.shadow;
    // The templates' location (Boulder) is not a geo-location the user chose.
    let template = esq_core::sun::ShadowInfo::default();
    let placed = s.latitude != template.latitude || s.longitude != template.longitude;
    json!({
        "name": m.name, "description": m.description, "guid": m.guid, "mipmapping": m.mipmapping,
        "diffusion": m.diffusion_image, "diffusion_opacity": m.diffusion_opacity, "datum": m.datum,
        "number_faces": faces_of(c.model),
        "units": unit_code(c.model.units),
        "georeferenced": placed,
        "latitude": s.latitude, "longitude": s.longitude, "north_angle": s.north_angle,
    })
}

fn set_meta(c: &mut Ctx, a: &J) -> R {
    let m = &mut c.model.meta;
    if let Some(v) = a.get("name").and_then(J::as_str) {
        m.name = v.into();
    }
    if let Some(v) = a.get("description").and_then(J::as_str) {
        m.description = v.into();
    }
    if let Some(v) = a.get("mipmapping").and_then(J::as_bool) {
        m.mipmapping = v;
    }
    if let Some(v) = a.get("diffusion_opacity").and_then(J::as_f64) {
        m.diffusion_opacity = v;
    }
    if let Some(v) = a.get("datum").and_then(J::as_str) {
        m.datum = v.into();
    }
    if let Some(d) = a.get("diffusion") {
        m.diffusion_image =
            d.as_str().map(|p| (p.to_string(), a["diffusion_opacity"].as_f64().unwrap_or(m.diffusion_opacity)));
    }
    Ok(J::Null)
}

fn option_get(c: &mut Ctx, a: &J) -> R {
    let p = a["provider"].as_str().unwrap_or("");
    let mut d = c.model.meta.options.get(p).cloned().ok_or_else(|| format!("fournisseur d'options inconnu: {p}"))?;
    if p == "UnitsOptions" {
        let unit = unit_code(c.model.units);
        d.insert("LengthUnit".into(), json!(unit));
        // Until set, the precision is the one of the unit's template (Esq::Units, zone geom).
        d.entry("LengthPrecision".to_string())
            .or_insert_with(|| json!(esq_core::units::default_precision(c.model.units)));
    }
    Ok(json!(d))
}

fn option_set(c: &mut Ctx, a: &J) -> R {
    let p = a["provider"].as_str().unwrap_or("").to_string();
    let k = a["key"].as_str().unwrap_or("").to_string();
    let v = a["value"].clone();
    if p == "UnitsOptions" && k == "LengthUnit" {
        let u = v.as_i64().and_then(unit_from).ok_or_else(|| raise("ArgumentError", "invalid length unit"))?;
        c.model.units = u;
    }
    c.model.meta.options.get_mut(&p).ok_or_else(|| format!("fournisseur d'options inconnu: {p}"))?.insert(k, v);
    Ok(J::Null)
}

// ---------------------------------------------------------------------------- hatch patterns

fn hatch_json(h: &HatchPattern) -> J {
    json!({"uid": h.uid, "name": h.name, "width": h.size[0], "height": h.size[1]})
}

fn hatch_add(c: &mut Ctx, a: &J) -> R {
    let name = a["name"].as_str().unwrap_or("");
    if name.is_empty() {
        return Err(raise("ArgumentError", "Hatch pattern name cannot be empty."));
    }
    if c.model.meta.hatch_patterns.iter().any(|h| h.name == name) {
        return Err(raise("ArgumentError", "Hatch pattern name must be unique."));
    }
    let path = a["path"].as_str().unwrap_or("");
    let data = std::fs::read(path).map_err(|_| raise("ArgumentError", "Failed to load pattern from path."))?;
    let uid = c.model.new_uid();
    let h = HatchPattern::new(uid, name, path, data);
    let j = hatch_json(&h);
    c.model.meta.hatch_patterns.push(h);
    Ok(j)
}

fn hatch_idx(c: &Ctx, a: &J) -> Result<usize, String> {
    let uid = id_of(a, "uid")? as u32;
    c.model
        .meta
        .hatch_patterns
        .iter()
        .position(|h| h.uid == uid)
        .ok_or_else(|| raise("TypeError", "reference to deleted HatchPattern"))
}

fn hatch_update(c: &mut Ctx, a: &J) -> R {
    let i = hatch_idx(c, a)?;
    if let Some(n) = a.get("name").and_then(J::as_str) {
        if n.is_empty() {
            return Err(raise("ArgumentError", "Hatch pattern name cannot be empty."));
        }
        if c.model.meta.hatch_patterns.iter().enumerate().any(|(k, h)| k != i && h.name == n) {
            return Err(raise("RuntimeError", "Hatch pattern name must be unique."));
        }
        c.model.meta.hatch_patterns[i].name = n.into();
    }
    if let Some(s) = a.get("size").and_then(J::as_array)
        && s.len() >= 2
    {
        c.model.meta.hatch_patterns[i].size = [s[0].as_f64().unwrap_or(1.0), s[1].as_f64().unwrap_or(1.0)];
    }
    Ok(hatch_json(&c.model.meta.hatch_patterns[i]))
}

fn hatch_remove(c: &mut Ctx, a: &J) -> R {
    let i = hatch_idx(c, a)?;
    let uid = c.model.meta.hatch_patterns[i].uid;
    for m in c.model.materials.values_mut() {
        if m.ext.hatch == Some(uid) {
            m.ext.hatch = None;
        }
    }
    c.model.meta.hatch_patterns.remove(i);
    Ok(json!(true))
}

fn hatch_purge(m: &mut Model) -> usize {
    let used: Vec<u32> = m.materials.values().filter_map(|x| x.ext.hatch).collect();
    let before = m.meta.hatch_patterns.len();
    m.meta.hatch_patterns.retain(|h| used.contains(&h.uid));
    before - m.meta.hatch_patterns.len()
}

fn hatch_write(c: &mut Ctx, a: &J) -> R {
    let i = hatch_idx(c, a)?;
    let path = a["path"].as_str().unwrap_or("");
    std::fs::write(path, &c.model.meta.hatch_patterns[i].data.0[..])
        .map_err(|_| raise("RuntimeError", "Failed to write hatch pattern to file."))?;
    Ok(json!(true))
}

// ---------------------------------------------------------------------------- notes, rays

fn add_note(c: &mut Ctx, a: &J) -> R {
    let (x, y) = (a["x"].as_f64().unwrap_or(0.0), a["y"].as_f64().unwrap_or(0.0));
    let id = c.model.root.annots.insert(Annotation {
        kind: AnnotKind::ScreenText { x, y },
        text: a["text"].as_str().unwrap_or("").to_string(),
        layer: 0,
        hidden: false,
        attach: Default::default(),
    });
    Ok(json!(handle('t', None, id.data().as_ffi())))
}

fn ray_tri(o: DVec3, d: DVec3, t: [DVec3; 3]) -> Option<f64> {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let dist = e2.dot(q) * inv;
    (dist > 1e-9).then_some(dist)
}

/// First visible face a ray meets, with the instance path leading to it.
fn raytest(c: &mut Ctx, a: &J) -> R {
    let o = pt(&a["origin"])?;
    let d = pt(&a["dir"])? / INCH;
    if d.length() < 1e-12 {
        return Ok(J::Null);
    }
    let d = d.normalize();
    let mut best: Option<(f64, Vec<String>)> = None;
    esq_core::walk::visit(c.model, |p| {
        let inv = p.xform.inverse();
        let (lo, ld) = (inv.transform_point3(o), inv.transform_vector3(d));
        for (fid, face) in p.ents.faces.iter() {
            if face.hidden || !c.model.layers.get(face.layer).is_none_or(|l| l.visible) {
                continue;
            }
            for t in esq_core::tess::face_triangles(p.ents, fid) {
                if let Some(dist) = ray_tri(lo, ld, t) {
                    // Distance along the local ray; convert to world.
                    let world = p.xform.transform_point3(lo + ld * dist);
                    let wd = (world - o).dot(d);
                    if best.as_ref().is_none_or(|(b, _)| wd < *b) {
                        let mut path = Vec::new();
                        let mut ctx: Option<DefId> = None;
                        for &i in &p.path {
                            path.push(handle('i', ctx, i.data().as_ffi()));
                            ctx = c.model.entities(ctx).instances.get(i).map(|x| x.def);
                        }
                        path.push(handle('f', p.def, fid.data().as_ffi()));
                        best = Some((wd, path));
                    }
                }
            }
        }
    });
    Ok(match best {
        Some((dist, path)) => {
            let w = o + d * dist;
            json!({"point": [w.x / INCH, w.y / INCH, w.z / INCH], "path": path})
        }
        None => J::Null,
    })
}

#[cfg(test)]
mod tests {
    /// The separators come from Windows: one or a few characters, never empty.
    #[test]
    fn regional_separators_are_read() {
        for (kind, fallback) in [(0x0E, "."), (0x0C, ",")] {
            let s = super::locale_info(kind, fallback);
            assert!(!s.is_empty() && s.chars().count() <= 3, "{s:?}");
        }
    }
}
