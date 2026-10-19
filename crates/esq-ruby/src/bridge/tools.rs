//! Pont natif de la zone « outils » : instantanés du modèle pour détecter ce qui a changé
//! (c'est ce qui permet d'émettre les événements des observateurs, voir `rb/46_observers_tools.rb`).
//!
//! `tools.state` reçoit la liste de ce que Ruby surveille et renvoie, pour chaque morceau, des
//! empreintes (hachages) : Ruby compare deux instantanés et en déduit `onElementAdded`,
//! `onSelectionAdded`, `onLayerChanged`, etc.

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde_json::{Map, Value as J, json};
use slotmap::{Key, KeyData};

use esq_core::model::{DefId, Entities, EntityRef};

use super::{Ctx, R, def_str, eref_handle, handle, parse_ctx, parse_h};

pub fn dispatch(c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    Some(match op {
        "tools.state" => state(c, a),
        _ => return None,
    })
}

/// Empreinte stable (pour un même processus) de n'importe quelle valeur sérialisable.
fn fp<T: std::fmt::Debug + ?Sized>(v: &T) -> String {
    let s = format!("{v:?}");
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:x}", h.finish())
}

/// Poignée → empreinte pour chaque entité d'une collection. Une arête ou une face dépend de la
/// position de ses sommets (qui ne sont pas dans la structure de l'arête elle-même).
fn entity_prints(model: &esq_core::model::Model, ctx: Option<DefId>) -> BTreeMap<String, String> {
    let e: &Entities = model.entities(ctx);
    let mut out = BTreeMap::new();
    for (id, ed) in e.edges.iter() {
        let (p, q) = e.edge_points(id);
        out.insert(handle('e', ctx, id.data().as_ffi()), fp(&(ed, p, q)));
    }
    for (id, f) in e.faces.iter() {
        let rings: Vec<_> = f.loops().map(|l| e.loop_points(l)).collect();
        out.insert(handle('f', ctx, id.data().as_ffi()), fp(&(f, rings)));
    }
    for (id, i) in e.instances.iter() {
        out.insert(handle('i', ctx, id.data().as_ffi()), fp(i));
    }
    // Guides, annotations et plans de coupe : seulement si la zone « entités » leur a donné une
    // poignée (`eref_handle` renvoie `None` sinon).
    for (id, g) in e.guides.iter() {
        if let Some(h) = eref_handle(ctx, EntityRef::Guide(id)) {
            out.insert(h, fp(g));
        }
    }
    for (id, t) in e.annots.iter() {
        if let Some(h) = eref_handle(ctx, EntityRef::Annot(id)) {
            out.insert(h, fp(t));
        }
    }
    for (id, s) in e.sections.iter() {
        if let Some(h) = eref_handle(ctx, EntityRef::Section(id)) {
            out.insert(h, fp(s));
        }
    }
    // Snaps and spaces are entities of the collection too (`Entities#add_snap`, `#add_space`).
    for s in e.snaps.iter().filter(|s| s.id != 0) {
        out.insert(handle('n', ctx, u64::from(s.id)), fp(s));
    }
    for s in e.spaces.iter().filter(|s| s.id != 0) {
        out.insert(handle('p', ctx, u64::from(s.id)), fp(s));
    }
    // An entity's attribute dictionaries are part of it: adding or changing one is a change
    // of the entity (EntityObserver#onChangeEntity, TestUp test_attribute_dictionary_evil_observer_*).
    for (h, print) in out.iter_mut() {
        if let Some(attrs) = model.attr_store.get(&format!("h:{h}")) {
            *print = fp(&(print.as_str(), attrs));
        }
    }
    out
}

/// Chaque collection du modèle : la racine (`None`) puis les définitions.
fn all_collections(model: &esq_core::model::Model) -> Vec<(Option<DefId>, &Entities)> {
    let mut v = vec![(None, &model.root)];
    v.extend(model.defs.iter().map(|(k, d)| (Some(k), &d.entities)));
    v
}

fn state(c: &mut Ctx, a: &J) -> R {
    let mut out = Map::new();
    let model: &esq_core::model::Model = c.model;

    if let Some(ctxs) = a.get("ents").and_then(J::as_array) {
        let mut m = Map::new();
        // The active section plane of each collection (EntitiesObserver#onActiveSectionPlaneChanged).
        let mut active = Map::new();
        for cx in ctxs.iter().filter_map(J::as_str) {
            let ctx = parse_ctx(cx)?;
            if ctx.is_some_and(|d| !model.defs.contains_key(d)) {
                m.insert(cx.into(), J::Null);
                continue;
            }
            let prints = entity_prints(model, ctx);
            m.insert(cx.into(), json!(prints));
            let sec = model.entities(ctx).active_section;
            active.insert(cx.into(), sec.map_or(J::Null, |s| json!(handle('s', ctx, s.data().as_ffi()))));
        }
        out.insert("ents".into(), J::Object(m));
        out.insert("active_sections".into(), J::Object(active));
    }

    if let Some(hs) = a.get("handles").and_then(J::as_array) {
        let mut m = Map::new();
        let mut cache: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        for h in hs.iter().filter_map(J::as_str) {
            let Ok(p) = parse_h(h) else {
                m.insert(h.into(), J::Null);
                continue;
            };
            if p.ctx.is_some_and(|d| !model.defs.contains_key(d)) {
                m.insert(h.into(), J::Null);
                continue;
            }
            // Une définition (« d:0:<ctx> ») : ses propriétés.
            if p.kind == 'd' {
                let def = DefId::from(KeyData::from_ffi(p.id));
                let v = model
                    .defs
                    .get(def)
                    .map_or(J::Null, |d| json!(fp(&(&d.name, &d.description, d.is_group, &d.attributes))));
                m.insert(h.into(), v);
                continue;
            }
            let key = def_str(p.ctx);
            let prints = cache.entry(key).or_insert_with(|| entity_prints(model, p.ctx));
            let Some(print) = prints.get(h) else {
                m.insert(h.into(), J::Null);
                continue;
            };
            // Cotes et textes : [empreinte, empreinte du texte] (DimensionObserver#onTextChanged).
            let text = (p.kind == 'a')
                .then(|| model.entities(p.ctx).annots.get(KeyData::from_ffi(p.id).into()))
                .flatten()
                .map(|t| fp(&t.text));
            m.insert(h.into(), text.map_or_else(|| json!(print), |t| json!([print, t])));
        }
        out.insert("handles".into(), J::Object(m));
    }

    if a.get("sel").and_then(J::as_bool).unwrap_or(false) {
        out.insert("sel".into(), json!(c.sel.iter().filter_map(|&e| eref_handle(None, e)).collect::<Vec<_>>()));
    }

    if a.get("layers").and_then(J::as_bool).unwrap_or(false) {
        out.insert("layers".into(), json!(model.layers.iter().map(|l| json!([l.name, fp(l)])).collect::<Vec<_>>()));
        // Tag folders (onLayerFolderAdded / Removed / Changed): [uid, empreinte].
        out.insert(
            "folders".into(),
            json!(model.meta.folders.iter().map(|f| json!([f.uid, fp(f)])).collect::<Vec<_>>()),
        );
    }

    if a.get("mats").and_then(J::as_bool).unwrap_or(false) {
        let mut refs: BTreeMap<u64, usize> = BTreeMap::new();
        for (_, e) in all_collections(model) {
            for f in e.faces.values() {
                for m in [f.front, f.back].into_iter().flatten() {
                    *refs.entry(m.data().as_ffi()).or_default() += 1;
                }
            }
            for i in e.instances.values() {
                if let Some(m) = i.material {
                    *refs.entry(m.data().as_ffi()).or_default() += 1;
                }
            }
        }
        let list: Vec<J> = model
            .materials
            .iter()
            .map(|(k, m)| json!([m.name, fp(m), refs.get(&k.data().as_ffi()).copied().unwrap_or(0)]))
            .collect();
        out.insert("mats".into(), json!(list));
    }

    if a.get("defs").and_then(J::as_bool).unwrap_or(false) {
        // [ctx, nom, empreinte des propriétés, type (groupe ou composant)]
        let list: Vec<J> = model
            .defs
            .iter()
            .map(|(k, d)| json!([def_str(Some(k)), d.name, fp(&(&d.name, &d.description)), d.is_group]))
            .collect();
        out.insert("defs".into(), json!(list));
    }

    if let Some(ds) = a.get("definstances").and_then(J::as_array) {
        // Pour chaque définition surveillée : les instances qui l'utilisent.
        let mut m = Map::new();
        for cx in ds.iter().filter_map(J::as_str) {
            let Some(def) = parse_ctx(cx)? else { continue };
            let mut v = Vec::new();
            for (ctx, e) in all_collections(model) {
                for (id, i) in e.instances.iter() {
                    if i.def == def {
                        v.push(handle('i', ctx, id.data().as_ffi()));
                    }
                }
            }
            v.sort();
            m.insert(cx.into(), json!(v));
        }
        out.insert("definstances".into(), J::Object(m));
    }

    if a.get("pages").and_then(J::as_bool).unwrap_or(false) {
        // [clé, empreinte] : la clé est l'identité de la scène (`Sketchup::Page`), sinon son nom.
        let list: Vec<J> = model
            .scenes
            .iter()
            .map(|s| {
                let key = if s.ext.uid != 0 { s.ext.uid.to_string() } else { format!("name:{}", s.name) };
                json!([key, fp(s)])
            })
            .collect();
        out.insert("pages".into(), json!(list));
    }

    if a.get("shadow").and_then(J::as_bool).unwrap_or(false) {
        out.insert("shadow".into(), json!(fp(&model.shadow)));
    }

    if a.get("envs").and_then(J::as_bool).unwrap_or(false) {
        // Environnements de la zone « vue » (`view_ext`) : [identifiant, empreinte] et le courant.
        let list: Vec<J> = model
            .view_ext
            .get("envs")
            .and_then(J::as_array)
            .into_iter()
            .flatten()
            .map(|e| json!([e["id"], fp(&e.to_string())]))
            .collect();
        let current = model.view_ext.get("env_current").cloned().unwrap_or(J::Null);
        out.insert("envs".into(), json!({ "list": list, "current": current }));
    }

    if a.get("rev").and_then(J::as_bool).unwrap_or(false) {
        // Empreinte de tout le modèle : sert à savoir si quelque chose a changé, même hors de ce qui est observé.
        out.insert("rev".into(), json!(fp(&serde_json::to_string(model).unwrap_or_default())));
    }
    Ok(J::Object(out))
}
