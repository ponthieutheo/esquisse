//! Étiquettes (`Sketchup::Layer`, `Layers`) et dossiers (`LayerFolder`).

use esq_core::meta::LayerFolder;
use esq_core::model::{EntityRef, Layer, LayerIdx, Model};
use serde_json::{Value as J, json};
use slotmap::KeyData;

use super::misc::active_path;
use super::{id_of, raise};
use crate::bridge::{Ctx, R, eref_handle, parse_h, to_eref};

pub fn run(c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    c.model.ensure_layer_uids();
    Some(match op {
        "layer.list" => {
            Ok(json!(c.model.layers.iter().enumerate().map(|(i, l)| layer_json(c.model, i, l)).collect::<Vec<_>>()))
        }
        "layer.add" => add(c, a),
        "layer.update" => update(c, a),
        "layer.remove" => remove(c, a),
        "layer.purge" => Ok(json!(purge(c.model))),
        "layer.active" => {
            let u = c.model.meta.active_layer;
            Ok(json!(if c.model.layer_index_of_uid(u).is_some() && u != 0 { u } else { c.model.layers[0].uid }))
        }
        "layer.set_active" => set_active(c, a),
        "layer.assign" => assign(c, a),
        "layer.unique_name" => Ok(json!(unique(c.model, a["name"].as_str().unwrap_or("Tag")))),
        "folder.list" => Ok(json!(c.model.meta.folders.iter().map(|f| folder_json(c.model, f)).collect::<Vec<_>>())),
        "folder.add" => folder_add(c, a),
        "folder.update" => folder_update(c, a),
        "folder.remove" => folder_remove(c, a),
        "folder.purge" => Ok(json!(folder_purge(c.model))),
        _ => return None,
    })
}

fn layer_json(m: &Model, i: usize, l: &Layer) -> J {
    json!({
        "uid": l.uid, "index": i, "name": if i == 0 { "Layer0" } else { l.name.as_str() }, "visible": l.visible, "color": l.color,
        "folder": l.folder, "page_behavior": l.page_behavior, "line_style": l.line_style,
        "active": m.meta.active_layer == l.uid || (m.meta.active_layer == 0 && i == 0),
    })
}

/// A folder's own switch (`LayerFolder#visible?`): its tags and sub-folders keep
/// theirs (`Model::layer_shown` puts them together).
fn folder_json(_m: &Model, f: &LayerFolder) -> J {
    json!({
        "uid": f.uid, "name": f.name, "parent": f.parent, "visible": f.visible,
        "visible_on_new_pages": f.visible_on_new_pages,
        "flag": f.visible,
    })
}

fn layer_idx(c: &Ctx, a: &J) -> Result<LayerIdx, String> {
    let uid = id_of(a, "uid")? as u32;
    c.model.layer_index_of_uid(uid).ok_or_else(|| raise("TypeError", "reference to deleted Layer"))
}

/// `Layers#unique_name`: the default tag answers to "Layer0" (whatever its
/// name in the interface's language), as `Layers#[]` finds it.
fn unique(m: &Model, base: &str) -> String {
    let taken: Vec<String> =
        m.layers.iter().enumerate().map(|(i, l)| if i == 0 { "Layer0".to_string() } else { l.name.clone() }).collect();
    Model::unique_name(base, &taken)
}

fn add(c: &mut Ctx, a: &J) -> R {
    let name = a["name"].as_str().unwrap_or("");
    if name.is_empty() {
        return Err(raise("ArgumentError", "tag name required"));
    }
    let (i, created) = match c.model.layers.iter().position(|l| l.name == name).or((name == "Layer0").then_some(0)) {
        Some(i) => (i, false),
        None => (c.model.add_layer(name), true),
    };
    let mut j = layer_json(c.model, i, &c.model.layers[i]);
    j["created"] = json!(created);
    Ok(j)
}

fn valid_behavior(v: u32) -> Result<(), String> {
    if v & !0x31 != 0 {
        return Err(raise("ArgumentError", "Invalid page behavior"));
    }
    if v & 0x30 == 0x30 {
        return Err(raise("ArgumentError", "Invalid flags for visibility on new pages"));
    }
    Ok(())
}

fn update(c: &mut Ctx, a: &J) -> R {
    let i = layer_idx(c, a)?;
    if let Some(n) = a.get("name").and_then(J::as_str) {
        if n.is_empty() {
            return Err(raise("ArgumentError", "tag name required"));
        }
        if i != 0 && c.model.layers[i].name != n {
            let taken: Vec<String> =
                c.model.layers.iter().enumerate().filter(|(k, _)| *k != i).map(|(_, l)| l.name.clone()).collect();
            let new = Model::unique_name(n, &taken);
            let old = std::mem::replace(&mut c.model.layers[i].name, new.clone());
            c.model.rename_attr_owner(&format!("layer:{old}"), &format!("layer:{new}"));
        }
    }
    if let Some(v) = a.get("visible").and_then(J::as_bool) {
        if !v && active_path(c).iter().any(|(_, l)| *l == i) {
            return Err(raise("ArgumentError", "cannot hide layer that is a part of the active path"));
        }
        c.model.layers[i].visible = v;
        if !v {
            // a hidden active tag gives way to the default one.
            if i != 0 && c.model.meta.active_layer == c.model.layers[i].uid {
                c.model.meta.active_layer = 0;
            }
            deselect_hidden(c);
        }
    }
    if let Some(col) = a.get("color") {
        c.model.layers[i].color = col
            .as_array()
            .filter(|x| x.len() >= 3)
            .map(|x| std::array::from_fn(|k| x[k].as_u64().unwrap_or(0).min(255) as u8));
    }
    if let Some(v) = a.get("page_behavior").and_then(J::as_u64) {
        valid_behavior(v as u32)?;
        c.model.layers[i].page_behavior = v as u32;
    }
    if let Some(v) = a.get("line_style").and_then(J::as_str) {
        c.model.layers[i].line_style = v.to_string();
    }
    if let Some(f) = a.get("folder") {
        if i == 0 && !f.is_null() {
            return Err(raise("ArgumentError", "Layer0 (Untagged) cannot appear in a folder"));
        }
        let f = f.as_u64().map(|u| u as u32);
        if let Some(u) = f
            && !c.model.meta.folders.iter().any(|x| x.uid == u)
        {
            return Err(raise("TypeError", "parent must be a Sketchup::LayerFolder or nil"));
        }
        c.model.layers[i].folder = f;
    }
    Ok(layer_json(c.model, i, &c.model.layers[i]))
}

fn remove(c: &mut Ctx, a: &J) -> R {
    let i = layer_idx(c, a)?;
    if i == 0 {
        return Err(raise("ArgumentError", "cannot remove default tag"));
    }
    if active_path(c).iter().any(|(_, l)| *l == i) {
        return Err(raise("RuntimeError", "tag is used by the active path"));
    }
    let del = a.get("delete_geometry").and_then(J::as_bool).unwrap_or(false);
    c.model.remove_layer(i, del);
    Ok(json!(true))
}

fn purge(m: &mut Model) -> usize {
    let unused = m.unused_layers();
    for i in &unused {
        m.remove_layer(*i, false);
    }
    unused.len()
}

fn set_active(c: &mut Ctx, a: &J) -> R {
    let i = layer_idx(c, a)?;
    c.model.meta.active_layer = if i == 0 { 0 } else { c.model.layers[i].uid };
    Ok(J::Null)
}

fn assign(c: &mut Ctx, a: &J) -> R {
    let i = layer_idx(c, a)?;
    let mut n = 0;
    for h in a["hs"].as_array().ok_or("hs attendu")? {
        let h = parse_h(h.as_str().ok_or("poignée attendue")?)?;
        let e = to_eref(&h)?;
        c.model.entities_mut(h.ctx).set_layer(e, i);
        n += 1;
    }
    Ok(json!(n))
}

fn folder_of(c: &Ctx, a: &J) -> Result<usize, String> {
    let uid = id_of(a, "uid")? as u32;
    c.model
        .meta
        .folders
        .iter()
        .position(|f| f.uid == uid)
        .ok_or_else(|| raise("TypeError", "reference to deleted LayerFolder"))
}

/// What a hidden tag or folder no longer shows leaves the selection.
fn deselect_hidden(c: &mut Ctx) {
    let ctx = super::misc::active_def(c);
    let ents = c.model.entities(ctx);
    let shown = c.model.layers_shown();
    c.sel.retain(|e| ents.layer_of(*e).is_none_or(|l| shown.get(l).copied().unwrap_or(true)));
}

/// A new folder: SketchUp lets folders share a name.
fn folder_add(c: &mut Ctx, a: &J) -> R {
    let name = a["name"].as_str().unwrap_or("").to_string();
    if name.is_empty() {
        return Err(raise("ArgumentError", "Folder name cannot be empty"));
    }
    let parent = a.get("parent").and_then(J::as_u64).map(|u| u as u32);
    let uid = c.model.new_uid();
    c.model.meta.folders.push(LayerFolder { uid, name, parent, visible: true, visible_on_new_pages: true });
    let f = c.model.meta.folders.last().unwrap().clone();
    Ok(folder_json(c.model, &f))
}

fn folder_update(c: &mut Ctx, a: &J) -> R {
    let k = folder_of(c, a)?;
    let uid = c.model.meta.folders[k].uid;
    if let Some(n) = a.get("name").and_then(J::as_str) {
        if n.is_empty() {
            return Err(raise("ArgumentError", "Folder names cannot be empty."));
        }
        c.model.meta.folders[k].name = n.to_string();
    }
    if let Some(p) = a.get("parent") {
        let p = p.as_u64().map(|u| u as u32);
        if let Some(pu) = p
            && (!c.model.meta.folders.iter().any(|x| x.uid == pu) || c.model.folder_within(pu, uid))
        {
            return Err(raise("TypeError", "parent must be a Sketchup::LayerFolder or nil"));
        }
        c.model.meta.folders[k].parent = p;
    }
    // A scene puts back the folder's own switch; its tags get their own state from the scene.
    if let Some(v) = a.get("flag").and_then(J::as_bool) {
        c.model.meta.folders[k].visible = v;
    }
    if let Some(v) = a.get("visible").and_then(J::as_bool) {
        if !v {
            let mut stack = vec![uid];
            let mut inside = Vec::new();
            while let Some(u) = stack.pop() {
                inside.push(u);
                stack.extend(c.model.meta.folders.iter().filter(|x| x.parent == Some(u)).map(|x| x.uid));
            }
            let path = active_path(c);
            for (i, l) in c.model.layers.iter().enumerate() {
                if l.folder.is_some_and(|f| inside.contains(&f)) && path.iter().any(|(_, pl)| *pl == i) {
                    return Err(raise(
                        "ArgumentError",
                        "cannot hide layer/layer that is a folder part of the active path",
                    ));
                }
            }
        }
        // The folder's own switch only: what it holds keeps its own.
        c.model.meta.folders[k].visible = v;
        if !v {
            deselect_hidden(c);
        }
    }
    if let Some(v) = a.get("visible_on_new_pages").and_then(J::as_bool) {
        c.model.meta.folders[k].visible_on_new_pages = v;
    }
    let f = c.model.meta.folders[k].clone();
    Ok(folder_json(c.model, &f))
}

/// the folder goes, its tags and sub-folders move up into its parent.
fn folder_remove(c: &mut Ctx, a: &J) -> R {
    let k = folder_of(c, a)?;
    let uid = c.model.meta.folders[k].uid;
    c.model.remove_folder(uid);
    Ok(json!(true))
}

fn folder_purge(m: &mut Model) -> usize {
    let mut n = 0;
    loop {
        let empty: Vec<u32> = m
            .meta
            .folders
            .iter()
            .filter(|f| {
                !m.layers.iter().any(|l| l.folder == Some(f.uid))
                    && !m.meta.folders.iter().any(|x| x.parent == Some(f.uid))
            })
            .map(|f| f.uid)
            .collect();
        if empty.is_empty() {
            return n;
        }
        n += empty.len();
        m.meta.folders.retain(|f| !empty.contains(&f.uid));
    }
}
