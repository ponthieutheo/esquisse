//! Position des textures sur les faces : `Face#position_material`, `uv_tile_at`,
//! `get_texture_projection`, `Sketchup::UVHelper`, les UVQ de `Face#mesh`. Le modèle de placage
//! est celui d'esq-core (`meta::side_uv`, `texpos`), le même que le lecteur et l'écrivain .skp.

use esq_core::glam::{DVec2, DVec3};
use esq_core::meta::{FaceTex, UvMap, side_uv};
use esq_core::model::{DefId, Face, FaceId, MaterialId, Model};
use esq_core::texpos::{Frame, map_from_pairs};
use serde_json::{Value as J, json};
use slotmap::KeyData;

use super::raise;
use crate::bridge::{Ctx, INCH, R, parse_h, pt};

pub fn run(c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    Some(match op {
        "uv.at" => at(c, a),
        "uv.set" => set(c, a),
        "uv.clear" => clear(c, a),
        "uv.state" => state(c, a),
        "uv.tile" => tile(c, a),
        _ => return None,
    })
}

fn face_of(a: &J) -> Result<(Option<DefId>, FaceId), String> {
    let h = parse_h(a["h"].as_str().ok_or("face attendue")?)?;
    if h.kind != 'f' {
        return Err(raise("TypeError", "wrong argument type (expected Sketchup::Face)"));
    }
    Ok((h.ctx, FaceId::from(KeyData::from_ffi(h.id))))
}

fn front(a: &J) -> bool {
    a.get("front").and_then(J::as_bool).unwrap_or(true)
}

fn face(m: &Model, ctx: Option<DefId>, f: FaceId) -> Result<&Face, String> {
    m.entities(ctx).faces.get(f).ok_or_else(|| raise("TypeError", "reference to deleted Face"))
}

/// The tile (mm) of the texture painted on one side, `None` for a colour or no material.
fn tile_of(m: &Model, mat: Option<MaterialId>) -> Option<[f64; 2]> {
    mat.and_then(|id| m.materials.get(id)).and_then(|x| x.ext.texture.as_ref()).map(|t| t.size)
}

fn side_material(f: &Face, front: bool) -> Option<MaterialId> {
    if front { f.front } else { f.back }
}

/// The map SketchUp's UV helper uses on one side.
pub(crate) fn map_of(m: &Model, f: &Face, front: bool) -> UvMap {
    side_uv(f.tex.as_deref(), front, f.normal, tile_of(m, side_material(f, front)))
}

/// A point of the face's plane, for the plane's frame.
fn plane_point(m: &Model, ctx: Option<DefId>, f: &Face) -> DVec3 {
    f.outer.first().and_then(|v| m.entities(ctx).verts.get(*v)).map_or(DVec3::ZERO, |v| v.pos)
}

/// `UVHelper#get_front_UVQ` and the UVQ of `Face#mesh`: the homogeneous coordinates (u·q, v·q, q).
fn at(c: &mut Ctx, a: &J) -> R {
    let (ctx, f) = face_of(a)?;
    let face = face(c.model, ctx, f)?;
    let h = map_of(c.model, face, front(a)).uvq(pt(&a["pt"])?);
    Ok(json!([h.x, h.y, h.z]))
}

/// `Face#position_material`: the model points (on any plane: taken in the frame of the side,
/// along the projection for a projected texture) and their texture points, solved as SketchUp
/// does. False when the material has no texture; an error when the points are degenerate.
fn set(c: &mut Ctx, a: &J) -> R {
    let (ctx, f) = face_of(a)?;
    let side = front(a);
    let raw = a["pairs"].as_array().ok_or("paires attendues")?;
    let mut pairs = Vec::new();
    for p in raw.chunks(2) {
        if p.len() < 2 {
            break;
        }
        let uv = p[1].as_array().ok_or("point UV attendu")?;
        let g = |i: usize| uv.get(i).and_then(J::as_f64).unwrap_or(0.0);
        pairs.push((pt(&p[0])?, DVec2::new(g(0), g(1))));
    }
    let mat = a
        .get("material")
        .and_then(J::as_str)
        .and_then(|n| c.model.materials.iter().find(|(_, m)| m.name == n))
        .map(|(id, _)| id);
    let Some(mat) = mat else { return Ok(json!(false)) };
    let Some(tile) = tile_of(c.model, Some(mat)) else { return Ok(json!(false)) };
    let projection = a.get("projection").and_then(|v| pt(v).ok()).map(|v| v / INCH).filter(|d| d.length() >= 1e-10);
    let face = face(c.model, ctx, f)?;
    let frame = Frame::of_side(plane_point(c.model, ctx, face), face.normal, projection);
    let map = map_from_pairs(&frame, tile, &pairs)
        .ok_or_else(|| raise("ArgumentError", "Could not compute valid matrix from points"))?;
    let face = c.model.entities_mut(ctx).faces.get_mut(f).unwrap();
    if side {
        face.front = Some(mat);
    } else {
        face.back = Some(mat);
    }
    let k = if side { 0 } else { 1 };
    let t = face.tex.get_or_insert_with(|| Box::new(FaceTex::default()));
    if side {
        t.front = Some(map);
    } else {
        t.back = Some(map);
    }
    t.projection[k] = projection.map(DVec3::normalize);
    // Position Texture shows these pins again.
    t.pins[k] = pairs.into_iter().take(4).collect();
    Ok(json!(true))
}

/// `Face#clear_texture_position` (both the position and the projection go) and
/// `clear_texture_projection` (the texture stays where it is on the face, no longer projected).
fn clear(c: &mut Ctx, a: &J) -> R {
    let (ctx, f) = face_of(a)?;
    let side = front(a);
    let k = if side { 0 } else { 1 };
    let origin = {
        let face = face(c.model, ctx, f)?;
        plane_point(c.model, ctx, face)
    };
    let face = c.model.entities_mut(ctx).faces.get_mut(f).unwrap();
    let normal = face.normal;
    if let Some(t) = face.tex.as_mut() {
        if a.get("projection_only").and_then(J::as_bool).unwrap_or(false) {
            if t.projection[k].take().is_some() {
                let map = if side { &mut t.front } else { &mut t.back };
                // Seen from the face's plane, as a texture positioned on it.
                *map = map.and_then(|m| m.projected(origin, normal, normal));
            }
        } else {
            if side {
                t.front = None
            } else {
                t.back = None
            }
            t.projection[k] = None;
            t.pins[k].clear();
        }
        if t.front.is_none() && t.back.is_none() && t.projection == [None, None] {
            face.tex = None;
        }
    }
    Ok(J::Null)
}

fn state(c: &mut Ctx, a: &J) -> R {
    let (ctx, f) = face_of(a)?;
    let k = if front(a) { 0 } else { 1 };
    let t = face(c.model, ctx, f)?.tex.as_deref();
    let positioned = t.is_some_and(|t| if k == 0 { t.front.is_some() } else { t.back.is_some() });
    let projection = t.and_then(|t| t.projection[k]);
    Ok(json!({"positioned": positioned, "projection": projection.map(|v| [v.x, v.y, v.z])}))
}

/// `Face#uv_tile_at` (in SketchUp.exe): the reference point taken on the face's
/// plane, the tile of the texture under it, and its four corners (0, 0), (1, 0), (1, 1),
/// (0, 1) as `position_material` takes them: model point, UV point… The model points are on
/// the plane the texture is laid in: the face's, or for a projected texture the plane through
/// the origin facing the projection. Nil when the side has no texture.
fn tile(c: &mut Ctx, a: &J) -> R {
    let (ctx, f) = face_of(a)?;
    let side = front(a);
    let face = face(c.model, ctx, f)?;
    if tile_of(c.model, side_material(face, side)).is_none() {
        return Ok(J::Null);
    }
    let map = map_of(c.model, face, side);
    let origin = plane_point(c.model, ctx, face);
    let n = face.normal;
    let r = pt(&a["pt"])?;
    let r = r - n * (r - origin).dot(n);
    let projection = face.tex.as_deref().and_then(|t| t.projection[if side { 0 } else { 1 }]);
    let frame = Frame::of_side(origin, n, projection);
    let uv = map.at(r);
    let (i, j) = (uv.x.floor(), uv.y.floor());
    let mut out = Vec::new();
    for (du, dv) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        let t = DVec2::new(i + du, j + dv);
        let Some(p) = map.point_at(t, frame.n * frame.c, frame.n) else { return Ok(J::Null) };
        out.push(json!([p.x / INCH, p.y / INCH, p.z / INCH]));
        out.push(json!([t.x, t.y, 0.0]));
    }
    Ok(json!(out))
}
