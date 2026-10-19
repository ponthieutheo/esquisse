//! Pont natif : opérations de l'interface. Renvoie `None` si
//! l'opération n'est pas de ce module.
#![allow(unused_imports)]

use serde_json::{Value as J, json};

use super::{Ctx, R};

pub fn dispatch(_c: &mut Ctx, _op: &str, _a: &J) -> Option<R> {
    None
}
