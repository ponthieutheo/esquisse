//! Pont natif de la zone « model » : sélection, matières, étiquettes, motifs de hachure,
//! options du modèle, images, opérations annulables.
#![allow(unused_imports)]

use serde_json::{Value as J, json};

use super::{Ctx, R};

pub(crate) mod images;
mod layers;
mod materials;
mod misc;
mod ops;
mod selection;
pub(crate) mod uv;

pub use ops::{begin_run, finish_run, take_ops};

pub fn dispatch(c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    let (domain, _) = op.split_once('.')?;
    match domain {
        "sel" => selection::run(c, op, a),
        "mat" => materials::run(c, op, a),
        "layer" | "folder" => layers::run(c, op, a),
        "img" => images::run(c, op, a),
        "op" => ops::run(c, op, a),
        "uv" => uv::run(c, op, a),
        "meta" | "opt" | "hatch" => misc::run(c, op, a),
        _ => None,
    }
}

/// Reads an unsigned id given as a JSON number or string.
pub(super) fn id_of(a: &J, key: &str) -> Result<u64, String> {
    match a.get(key) {
        Some(J::String(s)) => s.parse().map_err(|_| format!("identifiant invalide: {s}")),
        Some(J::Number(n)) => n.as_u64().ok_or_else(|| "identifiant invalide".to_string()),
        _ => Err(format!("argument « {key} » manquant")),
    }
}

/// Error that Ruby turns into an exception of class `class` (`Esq.raise_native`).
pub(super) fn raise(class: &str, msg: &str) -> String {
    format!("{class}: {msg}")
}

pub(super) fn b64(data: &[u8]) -> String {
    esq_core::photo::base64_encode(data)
}

pub(super) fn unb64(s: &str) -> Result<Vec<u8>, String> {
    esq_core::photo::base64_decode(s).ok_or_else(|| "base64 invalide".to_string())
}
