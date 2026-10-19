//! What a .skp holds besides entities: tags (colour, line style, folders, scene behaviour),
//! the model's name, description, axes, Model Info options, rendering options and attribute
//! dictionaries, and what scenes remember beyond their camera.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::*;

/// A typed value as plain JSON, as Esquisse keeps options (`Model::view_ext["ro"]`,
/// `ModelMeta::options`): booleans, numbers, strings, colours as `[r, g, b, a]`.
pub(crate) fn typed_plain(a: &Api, v: Ref) -> Option<Value> {
    let t = typed_tagged(a, v, 0)?;
    fn plain(t: &Value) -> Value {
        let v = t.get("v").cloned().unwrap_or(Value::Null);
        match t.get("t").and_then(Value::as_str) {
            Some("a") => Value::Array(v.as_array().map(|x| x.iter().map(plain).collect()).unwrap_or_default()),
            Some("time") => v.get(0).cloned().unwrap_or(Value::Null),
            Some("n") => Value::Null,
            _ => v,
        }
    }
    Some(plain(&t))
}

/// Sets `v` from plain JSON, in the type `ty` the option already has.
pub(crate) fn set_plain(a: &Api, v: Ref, ty: i32, j: &Value) -> R {
    let tagged = match (ty, j) {
        (tv::BOOL, _) => json!({"t": "b", "v": j.as_bool().unwrap_or(j.as_f64().is_some_and(|x| x != 0.0))}),
        (tv::BYTE | tv::SHORT | tv::INT32, _) => json!({"t": "i", "v": j.as_f64().map(|x| x.round() as i64)}),
        (tv::FLOAT | tv::DOUBLE, _) => json!({"t": "f", "v": j.as_f64()}),
        (tv::COLOR, _) => json!({"t": "c", "v": j}),
        (tv::TIME, _) => json!({"t": "time", "v": [j.as_i64().unwrap_or_default(), 0]}),
        (tv::STRING, _) => json!({"t": "s", "v": j.as_str().unwrap_or_default()}),
        (tv::VECTOR3D, _) => json!({"t": "vec", "v": j}),
        // An option the file does not have yet: from the value's own type.
        (_, Value::Bool(b)) => json!({"t": "b", "v": b}),
        (_, Value::Number(n)) if n.is_i64() => json!({"t": "i", "v": n}),
        (_, Value::Number(n)) => json!({"t": "f", "v": n}),
        (_, Value::String(s)) => json!({"t": "s", "v": s}),
        _ => return 0,
    };
    set_tagged(a, v, &tagged, 0)
}

/// Every key and value of a list of options (`count`, `keys`, `get` of the SDK).
fn read_options(
    a: &Api,
    count: impl FnOnce(&mut usize) -> R,
    keys: impl FnOnce(usize, *mut Ref, &mut usize) -> R,
    get: impl Fn(&CString, *mut Ref) -> R,
) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let mut n = 0;
    if count(&mut n) != 0 || n == 0 {
        return out;
    }
    let mut ks = vec![Ref::NULL; n];
    for k in &mut ks {
        unsafe { (a.SUStringCreate)(k) };
    }
    let mut got = 0;
    if keys(n, ks.as_mut_ptr(), &mut got) == 0 {
        for k in &ks[..got.min(n)] {
            let key = string_of(a, *k);
            let ck = cstr(&key);
            let mut v = Ref::NULL;
            unsafe { (a.SUTypedValueCreate)(&mut v) };
            if get(&ck, &mut v) == 0
                && let Some(x) = typed_plain(a, v)
            {
                out.insert(key, x);
            }
            unsafe { (a.SUTypedValueRelease)(&mut v) };
        }
    }
    for k in &mut ks {
        unsafe { (a.SUStringRelease)(k) };
    }
    out
}

/// Writes options: each value takes the type the option has in the file.
fn write_options(
    a: &Api,
    values: &serde_json::Map<String, Value>,
    get: impl Fn(&CString, *mut Ref) -> R,
    set: impl Fn(&CString, Ref) -> R,
) -> Result<()> {
    for (key, j) in values {
        let ck = cstr(key);
        let mut cur = Ref::NULL;
        su!(a.SUTypedValueCreate(&mut cur))?;
        let mut ty = -1;
        if get(&ck, &mut cur) == 0 {
            unsafe { (a.SUTypedValueGetType)(cur, &mut ty) };
        }
        unsafe { (a.SUTypedValueRelease)(&mut cur) };
        let mut v = Ref::NULL;
        su!(a.SUTypedValueCreate(&mut v))?;
        // Options SketchUp refuses (read-only ones) are skipped, as SketchUp skips them.
        if set_plain(a, v, ty, j) == 0 {
            set(&ck, v);
        }
        unsafe { (a.SUTypedValueRelease)(&mut v) };
    }
    Ok(())
}

pub(crate) fn rendering_options(a: &Api, ro: Ref) -> BTreeMap<String, Value> {
    read_options(
        a,
        |n| unsafe { (a.SURenderingOptionsGetNumKeys)(ro, n) },
        |n, p, g| unsafe { (a.SURenderingOptionsGetKeys)(ro, n, p, g) },
        |k, v| unsafe { (a.SURenderingOptionsGetValue)(ro, k.as_ptr(), v) },
    )
}

fn write_rendering_options(a: &Api, ro: Ref, values: &Value) -> Result<()> {
    let Some(map) = values.as_object() else { return Ok(()) };
    write_options(
        a,
        map,
        |k, v| unsafe { (a.SURenderingOptionsGetValue)(ro, k.as_ptr(), v) },
        |k, v| unsafe { (a.SURenderingOptionsSetValue)(ro, k.as_ptr(), v) },
    )
}

/// `SULayerVisibilityDefaultType` / `SULayerVisibilityNewSceneType` as Ruby's
/// `Layer#page_behavior` flags (measured: the SDK takes the same values, hidden by default = 1,
/// visible on new scenes = 0x10, hidden on new scenes = 0x20).
fn page_behavior(default: i32, new_scene: i32) -> u32 {
    (default & 1) as u32 | (new_scene & 0x30) as u32
}

fn scene_behavior(b: u32) -> (i32, i32) {
    ((b & 1) as i32, (b & 0x30) as i32)
}

impl Reader<'_> {
    /// Tags and their folders.
    pub(crate) fn read_layers(&mut self, m: Ref) {
        let a = self.a;
        let mut default_layer = Ref::NULL;
        unsafe { (a.SUModelGetDefaultLayer)(m, &mut default_layer) };
        for l in
            list(|n| unsafe { (a.SUModelGetNumLayers)(m, n) }, |n, p, g| unsafe { (a.SUModelGetLayers)(m, n, p, g) })
        {
            let idx = if l.0 == default_layer.0 {
                0
            } else {
                let name = read_string(a, |s| unsafe { (a.SULayerGetName)(l, s) });
                self.model.layers.push(Layer::new(name));
                self.model.layers.len() - 1
            };
            self.layers.insert(l.0 as usize, idx);
            let mut visible = true;
            unsafe { (a.SULayerGetVisibility)(l, &mut visible) };
            let mut mat = Ref::NULL;
            let color = (unsafe { (a.SULayerGetMaterial)(l, &mut mat) } == 0 && !mat.is_null()).then(|| {
                let mut c = Color::default();
                unsafe { (a.SUMaterialGetColor)(mat, &mut c) };
                [c.r, c.g, c.b]
            });
            let mut ls = Ref::NULL;
            let line_style = if unsafe { (a.SULayerGetLineStyle)(l, &mut ls) } == 0 && !ls.is_null() {
                read_string(a, |s| unsafe { (a.SULineStyleGetName)(ls, s) })
            } else {
                String::new()
            };
            let (mut d, mut ns) = (0, 0);
            unsafe { (a.SULayerGetSceneBehavior)(l, &mut d, &mut ns) };
            let layer = &mut self.model.layers[idx];
            if idx != 0 {
                layer.visible = visible;
                layer.line_style = line_style;
            }
            layer.color = color;
            layer.page_behavior = page_behavior(d, ns);
            let attrs = read_attrs(a, unsafe { (a.SULayerToEntity)(l) });
            if !attrs.is_empty() {
                let name = self.model.layers[idx].name.clone();
                self.model.attr_store.insert(format!("layer:{name}"), attrs);
            }
        }
        self.model.ensure_layer_uids();
        // Folders, depth first.
        let mut stack: Vec<(Ref, Option<u32>)> = list(
            |n| unsafe { (a.SUModelGetNumLayerFolders)(m, n) },
            |n, p, g| unsafe { (a.SUModelGetLayerFolders)(m, n, p, g) },
        )
        .into_iter()
        .rev()
        .map(|f| (f, None))
        .collect();
        while let Some((f, parent)) = stack.pop() {
            let uid = self.model.new_uid();
            let name = read_string(a, |s| unsafe { (a.SULayerFolderGetName)(f, s) });
            let (mut visible, mut on_new) = (true, true);
            unsafe {
                (a.SULayerFolderGetVisibility)(f, &mut visible);
                (a.SULayerGroupGetVisibleOnNewScenes)(f, &mut on_new);
            }
            self.model.meta.folders.push(esq_core::meta::LayerFolder {
                uid,
                name,
                parent,
                visible,
                visible_on_new_pages: on_new,
            });
            for l in list(
                |n| unsafe { (a.SULayerFolderGetNumLayers)(f, n) },
                |n, p, g| unsafe { (a.SULayerFolderGetLayers)(f, n, p, g) },
            ) {
                if let Some(&i) = self.layers.get(&(l.0 as usize)) {
                    self.model.layers[i].folder = Some(uid);
                }
            }
            let subs = list(
                |n| unsafe { (a.SULayerFolderGetNumLayerFolders)(f, n) },
                |n, p, g| unsafe { (a.SULayerFolderGetLayerFolders)(f, n, p, g) },
            );
            stack.extend(subs.into_iter().rev().map(|s| (s, Some(uid))));
        }
        let mut active = Ref::NULL;
        if unsafe { (a.SUModelGetActiveLayer)(m, &mut active) } == 0
            && let Some(&i) = self.layers.get(&(active.0 as usize))
        {
            self.model.meta.active_layer = if i == 0 { 0 } else { self.model.layers[i].uid };
        }
    }

    /// Name, description, axes, options, rendering options and dictionaries of the model.
    pub(crate) fn read_model_info(&mut self, m: Ref) {
        let a = self.a;
        let meta = &mut self.model.meta;
        meta.name = read_string(a, |s| unsafe { (a.SUModelGetName)(m, s) });
        meta.description = read_string(a, |s| unsafe { (a.SUModelGetDescription)(m, s) });
        // `Model#guid` of the opened file is the one it was saved with.
        meta.guid = read_string(a, |s| unsafe { (a.SUModelGetGuid)(m, s) });
        let mut axes = Ref::NULL;
        if unsafe { (a.SUModelGetAxes)(m, &mut axes) } == 0 {
            let mut x = Xf { v: [0.0; 16] };
            if unsafe { (a.SUAxesGetTransform)(axes, &mut x) } == 0 {
                self.model.axes = xf_to_mm(&x);
            }
        }
        let mut om = Ref::NULL;
        if unsafe { (a.SUModelGetOptionsManager)(m, &mut om) } == 0 {
            let mut n = 0;
            unsafe { (a.SUOptionsManagerGetNumOptionsProviders)(om, &mut n) };
            let mut names = vec![Ref::NULL; n];
            for s in &mut names {
                unsafe { (a.SUStringCreate)(s) };
            }
            let mut got = 0;
            if n > 0 && unsafe { (a.SUOptionsManagerGetOptionsProviderNames)(om, n, names.as_mut_ptr(), &mut got) } == 0
            {
                for s in &names[..got.min(n)] {
                    let name = string_of(a, *s);
                    let cn = cstr(&name);
                    let mut p = Ref::NULL;
                    if unsafe { (a.SUOptionsManagerGetOptionsProviderByName)(om, cn.as_ptr(), &mut p) } != 0 {
                        continue;
                    }
                    let values = read_options(
                        a,
                        |n| unsafe { (a.SUOptionsProviderGetNumKeys)(p, n) },
                        |n, k, g| unsafe { (a.SUOptionsProviderGetKeys)(p, n, k, g) },
                        |k, v| unsafe { (a.SUOptionsProviderGetValue)(p, k.as_ptr(), v) },
                    );
                    self.model.meta.options.entry(name).or_default().extend(values);
                }
            }
            for s in &mut names {
                unsafe { (a.SUStringRelease)(s) };
            }
        }
        let mut ro = Ref::NULL;
        if unsafe { (a.SUModelGetRenderingOptions)(m, &mut ro) } == 0 {
            let values = rendering_options(a, ro);
            if !values.is_empty() {
                self.model.view_ext.insert("ro".into(), json!(values));
            }
        }
        let attrs = read_model_attrs(a, m);
        if !attrs.is_empty() {
            self.model.attr_store.insert("model".into(), attrs);
        }
        // Tools › Comment: the comments travel in a dictionary of their own.
        self.model.take_skp_comments();
    }

    /// What a scene remembers beyond its camera: which properties it saves (`flags`), its
    /// rendering options, whether it plays in the animation. Returns its dictionaries.
    pub(crate) fn read_scene_ext(&self, s: Ref, scene: &mut esq_core::model::Scene) -> Attributes {
        let a = self.a;
        let p = &mut scene.ext.props;
        let mut flags = 0u32;
        if unsafe { (a.SUSceneGetFlags)(s, &mut flags) } == 0 {
            p.insert("flags".into(), json!(flags));
        }
        let mut use_ro = false;
        unsafe { (a.SUSceneGetUseRenderingOptions)(s, &mut use_ro) };
        let mut ro = Ref::NULL;
        if use_ro && unsafe { (a.SUSceneGetRenderingOptions)(s, &mut ro) } == 0 && !ro.is_null() {
            p.insert("ro".into(), json!(rendering_options(a, ro)));
        }
        let mut anim = true;
        if unsafe { (a.SUSceneGetIncludeInAnimation)(s, &mut anim) } == 0 && !anim {
            p.insert("animate".into(), json!(false));
        }
        read_attrs(a, unsafe { (a.SUSceneToEntity)(s) })
    }
}

/// Key of a scene's dictionaries in the side table (`Sketchup::Page`'s handle).
pub(crate) fn scene_owner(scene: &esq_core::model::Scene) -> String {
    format!("h:g:0:{}", scene.ext.uid)
}

impl Writer<'_> {
    /// Tags, their folders, the active tag.
    pub(crate) fn write_layers(&mut self, m: Ref) -> Result<()> {
        let a = self.a;
        let model = self.model;
        let mut default_layer = Ref::NULL;
        su!(a.SUModelGetDefaultLayer(m, &mut default_layer))?;
        self.layers.push(default_layer);
        let mut styles = Ref::NULL;
        unsafe { (a.SUModelGetLineStyles)(m, &mut styles) };
        for (i, l) in model.layers.iter().enumerate() {
            let r = if i == 0 {
                default_layer
            } else {
                let mut r = Ref::NULL;
                su!(a.SULayerCreate(&mut r))?;
                let name = cstr(&l.name);
                su!(a.SULayerSetName(r, name.as_ptr()))?;
                su!(a.SULayerSetVisibility(r, l.visible))?;
                su!(a.SUModelAddLayers(m, 1, &r))?;
                self.layers.push(r);
                r
            };
            if let Some(c) = l.color {
                let mut mat = Ref::NULL;
                if unsafe { (a.SULayerGetMaterial)(r, &mut mat) } == 0 && !mat.is_null() {
                    su!(a.SUMaterialSetColor(mat, &Color { r: c[0], g: c[1], b: c[2], a: 255 }))?;
                }
            }
            if !l.line_style.is_empty() && !styles.is_null() {
                let n = cstr(&l.line_style);
                let mut ls = Ref::NULL;
                if unsafe { (a.SULineStylesGetLineStyleByName)(styles, n.as_ptr(), &mut ls) } == 0 {
                    su!(a.SULayerSetLineStyle(r, ls))?;
                }
            }
            if l.page_behavior != 0 {
                let (d, ns) = scene_behavior(l.page_behavior);
                su!(a.SULayerSetSceneBehavior(r, d, ns))?;
            }
            if let Some(attrs) = model.attr_store.get(&format!("layer:{}", l.name)) {
                write_attrs(a, unsafe { (a.SULayerToEntity)(r) }, attrs)?;
            }
        }
        // Folders: parents first.
        let mut made: HashMap<u32, Ref> = HashMap::new();
        let mut left: Vec<&esq_core::meta::LayerFolder> = model.meta.folders.iter().collect();
        while !left.is_empty() {
            let before = left.len();
            let mut i = 0;
            while i < left.len() {
                let f = left[i];
                let parent = match f.parent {
                    None => None,
                    Some(p) => match made.get(&p) {
                        Some(&r) => Some(r),
                        None if model.meta.folders.iter().any(|x| x.uid == p) => {
                            i += 1;
                            continue;
                        }
                        None => None,
                    },
                };
                let mut r = Ref::NULL;
                let n = cstr(&f.name);
                su!(a.SULayerFolderCreate(&mut r, n.as_ptr()))?;
                match parent {
                    Some(p) => su!(a.SULayerFolderAddLayerFolder(p, r))?,
                    None => su!(a.SUModelAddLayerFolder(m, r))?,
                }
                su!(a.SULayerFolderSetVisibility(r, f.visible))?;
                su!(a.SULayerGroupSetVisibleOnNewScenes(r, f.visible_on_new_pages))?;
                made.insert(f.uid, r);
                left.remove(i);
            }
            if left.len() == before {
                break;
            }
        }
        for (i, l) in model.layers.iter().enumerate().skip(1) {
            if let Some(&f) = l.folder.and_then(|u| made.get(&u)) {
                su!(a.SULayerFolderAddLayer(f, self.layers[i]))?;
            }
        }
        if model.meta.active_layer != 0
            && let Some(i) = model.layer_index_of_uid(model.meta.active_layer)
        {
            su!(a.SUModelSetActiveLayer(m, self.layers[i]))?;
        }
        Ok(())
    }

    /// Name, description, axes, options, rendering options and dictionaries of the model.
    pub(crate) fn write_model_info(&self, m: Ref) -> Result<()> {
        let a = self.a;
        let model = self.model;
        if !model.meta.name.is_empty() {
            let n = cstr(&model.meta.name);
            su!(a.SUModelSetName(m, n.as_ptr()))?;
        }
        if !model.meta.description.is_empty() {
            let d = cstr(&model.meta.description);
            su!(a.SUModelSetDescription(m, d.as_ptr()))?;
        }
        if model.axes != DMat4::IDENTITY {
            let mut axes = Ref::NULL;
            su!(a.SUModelGetAxes(m, &mut axes))?;
            let v = |c: esq_core::glam::DVec4| P3 { x: c.x, y: c.y, z: c.z };
            let ax = &model.axes;
            let o = ax.w_axis / IN;
            su!(a.SUAxesSetOrigin(axes, &P3 { x: o.x, y: o.y, z: o.z }))?;
            su!(a.SUAxesSetAxesVecs(axes, &v(ax.x_axis), &v(ax.y_axis), &v(ax.z_axis)))?;
        }
        let mut om = Ref::NULL;
        su!(a.SUModelGetOptionsManager(m, &mut om))?;
        for (name, values) in &model.meta.options {
            let cn = cstr(name);
            let mut p = Ref::NULL;
            if unsafe { (a.SUOptionsManagerGetOptionsProviderByName)(om, cn.as_ptr(), &mut p) } != 0 {
                continue;
            }
            let map: serde_json::Map<String, Value> = values.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            write_options(
                a,
                &map,
                |k, v| unsafe { (a.SUOptionsProviderGetValue)(p, k.as_ptr(), v) },
                |k, v| unsafe { (a.SUOptionsProviderSetValue)(p, k.as_ptr(), v) },
            )?;
        }
        if let Some(ro) = model.view_ext.get("ro") {
            let mut r = Ref::NULL;
            su!(a.SUModelGetRenderingOptions(m, &mut r))?;
            write_rendering_options(a, r, ro)?;
        }
        if let Some(attrs) = model.skp_model_attrs() {
            write_model_attrs(a, m, &attrs)?;
        }
        Ok(())
    }

    /// What [`Reader::read_scene_ext`] reads.
    pub(crate) fn write_scene_ext(&self, s: Ref, scene: &esq_core::model::Scene) -> Result<()> {
        let a = self.a;
        let p = &scene.ext.props;
        // The scene's own style: it exists once the scene keeps one.
        if let Some(ro) = p.get("ro") {
            su!(a.SUSceneSetUseRenderingOptions(s, true))?;
            let mut r = Ref::NULL;
            if unsafe { (a.SUSceneGetRenderingOptions)(s, &mut r) } == 0 && !r.is_null() {
                write_rendering_options(a, r, ro)?;
            }
        }
        if let Some(f) = p.get("flags").and_then(Value::as_u64) {
            let f = f as u32;
            // A scene that keeps the shadows or the axes keeps those of the model (measured:
            // without them SketchUp drops the flags).
            const SHADOWS_AND_AXES: u32 = 4 | 8;
            su!(a.SUSceneSetFlags(s, f))?;
            if f & SHADOWS_AND_AXES != 0 {
                su!(a.SUSceneUpdate(s, f & SHADOWS_AND_AXES))?;
                su!(a.SUSceneSetFlags(s, f))?;
            }
        }
        if p.get("animate").and_then(Value::as_bool) == Some(false) {
            su!(a.SUSceneSetIncludeInAnimation(s, false))?;
        }
        if let Some(attrs) = self.model.attr_store.get(&scene_owner(scene)) {
            write_attrs(a, unsafe { (a.SUSceneToEntity)(s) }, attrs)?;
        }
        Ok(())
    }
}
