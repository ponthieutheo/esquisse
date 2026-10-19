//! Host side of the entity classes: what needs the application (system fonts for
//! `Entities#add_3d_text`). Ruby reaches it as `host.entities.<op>`.

use esq_core::model::{DefId, EntityRef, Model};
use esq_core::text3d::{self, Align, TextSpec};
use serde_json::{Value as J, json};

use crate::app::EsquisseApp;
use crate::fonts::{font_list, with_source};

const INCH: f64 = 25.4;

#[allow(clippy::ptr_arg)]
pub fn call(
    _app: &mut EsquisseApp,
    model: &mut Model,
    _sel: &mut Vec<EntityRef>,
    op: &str,
    args: &J,
) -> Result<J, String> {
    match op {
        "entities.text3d" => text3d(model, args),
        _ => Err(format!("opération hôte non implémentée: {op}")),
    }
}

/// Builds 3D text into the collection `ctx` ("0" = the model root). Lengths come in inches.
/// Answers whether any geometry was made.
fn text3d(model: &mut Model, a: &J) -> Result<J, String> {
    let ctx = match a.get("ctx").and_then(J::as_str).unwrap_or("0") {
        "0" => None,
        s => Some(DefId::from_ffi(s.parse().map_err(|_| format!("contexte invalide: {s}"))?)),
    };
    if ctx.is_some_and(|d| !model.defs.contains_key(d)) {
        return Err("contexte supprimé".into());
    }
    let num = |k: &str, d: f64| a.get(k).and_then(J::as_f64).unwrap_or(d);
    let flag = |k: &str| a.get(k).and_then(J::as_bool).unwrap_or(false);
    let family = a.get("font").and_then(J::as_str).unwrap_or("Arial");
    let file = font_list()
        .file(family, flag("bold"), flag("italic"))
        .ok_or_else(|| format!("Police introuvable : {family}"))?;
    let spec = TextSpec {
        text: a.get("text").and_then(J::as_str).unwrap_or("").to_string(),
        align: match a.get("align").and_then(J::as_i64).unwrap_or(0) {
            1 => Align::Center,
            2 => Align::Right,
            _ => Align::Left,
        },
        height: num("height", 1.0) * INCH,
        tolerance: num("tolerance", 0.0) * INCH,
        z: num("z", 0.0) * INCH,
        filled: a.get("filled").and_then(J::as_bool).unwrap_or(true),
        extrusion: num("extrusion", 0.0) * INCH,
    };
    let ents = model.entities_mut(ctx);
    let before = ents.edges.len();
    with_source(file, |src| {
        text3d::build(src, &spec, ents);
    })
    .ok_or("Impossible de lire la police")?;
    Ok(json!(ents.edges.len() > before))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_text_into_the_root() {
        if font_list().families.is_empty() {
            return; // no fonts on this machine
        }
        let family = font_list().families.keys().next().unwrap().clone();
        let mut m = Model::new();
        let r = text3d(
            &mut m,
            &json!({"ctx": "0", "text": "Hi", "align": 0, "font": family, "height": 1.0, "filled": true, "extrusion": 0.5}),
        )
        .unwrap();
        assert_eq!(r, json!(true));
        assert!(!m.root.faces.is_empty());
        let top = m.root.verts.values().map(|v| v.pos.z).fold(f64::MIN, f64::max);
        assert!((top - 0.5 * INCH).abs() < 1e-6, "extrusion {top}");
        assert!(text3d(&mut m, &json!({"ctx": "99999999", "text": "x", "font": family})).is_err());
    }
}
