//! Pont natif : opérations de la géométrie. Renvoie `None` si
//! l'opération n'est pas de ce module.
#![allow(unused_imports)]

use glam::{DVec2, DVec3};
use serde_json::{Value as J, json};

use esq_core::math::{basis_for, newell_normal};
use esq_core::tess::triangulate_2d;

use super::{Ctx, R, out_pt, pts};

pub fn dispatch(_c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    Some(match op {
        "geom.tesselate" => tesselate(a),
        _ => return None,
    })
}

/// `Geom.tesselate(outer, hole, ...)` : triangles d'un polygone plan avec trous. Les points arrivent
/// en pouces (convertis en mm par `pts`, la triangulation ne dépend pas de l'échelle) ; chaque
/// triangle repart orienté comme le contour extérieur.
fn tesselate(a: &J) -> R {
    let loops: Vec<Vec<DVec3>> =
        a["loops"].as_array().ok_or("boucles attendues")?.iter().map(pts).collect::<Result<_, _>>()?;
    let outer = loops.first().ok_or("aucune boucle")?;
    let normal = newell_normal(outer);
    if normal.length_squared() < 1e-18 {
        return Err("polygon is degenerate".into());
    }
    let normal = normal.normalize();
    let (u, v) = basis_for(normal);
    let origin = outer[0];
    let proj = |p: &DVec3| DVec2::new((*p - origin).dot(u), (*p - origin).dot(v));
    let flat: Vec<DVec2> = outer.iter().map(proj).collect();
    let holes: Vec<Vec<DVec2>> = loops[1..].iter().map(|l| l.iter().map(proj).collect()).collect();
    let all: Vec<DVec3> = loops.iter().flatten().copied().collect();
    let tris = triangulate_2d(&flat, &holes);
    if tris.is_empty() {
        return Err("triangulation failed".into());
    }
    let mut out = Vec::with_capacity(tris.len());
    for [i, j, k] in tris {
        let (p, mut q, mut r) = (all[i], all[j], all[k]);
        if (q - p).cross(r - p).dot(normal) < 0.0 {
            std::mem::swap(&mut q, &mut r);
        }
        out.push(json!([out_pt(p), out_pt(q), out_pt(r)]));
    }
    Ok(J::Array(out))
}
