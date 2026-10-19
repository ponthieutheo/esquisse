//! Lossless round trip of SketchUp's own files: what a .skp holds is counted through the SDK
//! (independently of the reader), before and after a pass through Esquisse.

use std::collections::BTreeMap;

use super::*;

/// What a .skp holds, as lines "what = value" (sorted).
fn census(path: &Path) -> BTreeMap<String, String> {
    let a = api().unwrap();
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let p = cstr(&path.to_string_lossy());
    let mut m = Ref::NULL;
    let mut status = 0;
    su!(a.SUModelCreateFromFileWithStatus(&mut m, p.as_ptr(), &mut status)).unwrap();
    let mut add = |k: String, v: String| {
        out.insert(k, v);
    };
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();

    // Materials.
    for mat in
        list(|n| unsafe { (a.SUModelGetNumMaterials)(m, n) }, |n, p, g| unsafe { (a.SUModelGetMaterials)(m, n, p, g) })
    {
        let name = read_string(a, |s| unsafe { (a.SUMaterialGetName)(mat, s) });
        let mut c = Color::default();
        unsafe { (a.SUMaterialGetColor)(mat, &mut c) };
        let mut ty = 0;
        unsafe { (a.SUMaterialGetType)(mat, &mut ty) };
        let mut op = 0.0;
        unsafe { (a.SUMaterialGetOpacity)(mat, &mut op) };
        let mut tex = Ref::NULL;
        let mut t = String::new();
        if unsafe { (a.SUMaterialGetTexture)(mat, &mut tex) } == 0 {
            let (mut w, mut h, mut s, mut tt) = (0usize, 0usize, 0.0, 0.0);
            unsafe { (a.SUTextureGetDimensions)(tex, &mut w, &mut h, &mut s, &mut tt) };
            let f = read_string(a, |x| unsafe { (a.SUTextureGetFileName)(tex, x) });
            t = format!(" texture {} {w}x{h} {:.4}x{:.4}", esq_core::meta::base_name(&f), 1.0 / s, 1.0 / tt);
        }
        let attrs = read_attrs(a, unsafe { (a.SUMaterialToEntity)(mat) });
        add(
            format!("material {name}"),
            format!("rgb {} {} {} type {ty} opacity {op:.3}{t} dicts {}", c.r, c.g, c.b, attrs.len()),
        );
    }
    // Tags.
    for l in list(|n| unsafe { (a.SUModelGetNumLayers)(m, n) }, |n, p, g| unsafe { (a.SUModelGetLayers)(m, n, p, g) }) {
        let name = read_string(a, |s| unsafe { (a.SULayerGetName)(l, s) });
        let mut vis = true;
        unsafe { (a.SULayerGetVisibility)(l, &mut vis) };
        let mut mat = Ref::NULL;
        let mut c = Color::default();
        if unsafe { (a.SULayerGetMaterial)(l, &mut mat) } == 0 {
            unsafe { (a.SUMaterialGetColor)(mat, &mut c) };
        }
        let mut ls = Ref::NULL;
        let style = if unsafe { (a.SULayerGetLineStyle)(l, &mut ls) } == 0 && !ls.is_null() {
            read_string(a, |s| unsafe { (a.SULineStyleGetName)(ls, s) })
        } else {
            String::new()
        };
        let mut f = Ref::NULL;
        let folder = if unsafe { (a.SULayerGetParentLayerFolder)(l, &mut f) } == 0 && !f.is_null() {
            read_string(a, |s| unsafe { (a.SULayerFolderGetName)(f, s) })
        } else {
            String::new()
        };
        let (mut d, mut ns) = (0, 0);
        unsafe { (a.SULayerGetSceneBehavior)(l, &mut d, &mut ns) };
        add(
            format!("tag {name}"),
            format!("visible {vis} rgb {} {} {} style {style} folder {folder} behaviour {d}/{ns}", c.r, c.g, c.b),
        );
    }
    *count.entry("folders").or_default() += list(
        |n| unsafe { (a.SUModelGetNumLayerFolders)(m, n) },
        |n, p, g| unsafe { (a.SUModelGetLayerFolders)(m, n, p, g) },
    )
    .len();
    // Scenes.
    for s in list(|n| unsafe { (a.SUModelGetNumScenes)(m, n) }, |n, p, g| unsafe { (a.SUModelGetScenes)(m, n, p, g) }) {
        let name = read_string(a, |x| unsafe { (a.SUSceneGetName)(s, x) });
        let mut flags = 0u32;
        unsafe { (a.SUSceneGetFlags)(s, &mut flags) };
        let mut ro = Ref::NULL;
        let mut use_ro = false;
        unsafe { (a.SUSceneGetUseRenderingOptions)(s, &mut use_ro) };
        let n_ro = if use_ro && unsafe { (a.SUSceneGetRenderingOptions)(s, &mut ro) } == 0 && !ro.is_null() {
            format!("{:?}", model_info::rendering_options(a, ro))
        } else {
            String::new()
        };
        add(format!("scene {name}"), format!("flags {flags} ro {n_ro}"));
    }
    // Model.
    add("model name".into(), read_string(a, |s| unsafe { (a.SUModelGetName)(m, s) }));
    add("model description".into(), read_string(a, |s| unsafe { (a.SUModelGetDescription)(m, s) }));
    add("model dicts".into(), format!("{:?}", read_model_attrs(a, m)));
    let mut ro = Ref::NULL;
    unsafe { (a.SUModelGetRenderingOptions)(m, &mut ro) };
    for (k, v) in model_info::rendering_options(a, ro) {
        add(format!("ro {k}"), v.to_string());
    }
    let mut axes = Ref::NULL;
    let mut x = Xf { v: [0.0; 16] };
    unsafe {
        (a.SUModelGetAxes)(m, &mut axes);
        (a.SUAxesGetTransform)(axes, &mut x);
    }
    add("model axes".into(), format!("{:?}", x.v.map(|v| (v * 1e6).round() / 1e6)));

    // Entities, recursively.
    let mut ents = Ref::NULL;
    su!(a.SUModelGetEntities(m, &mut ents)).unwrap();
    let mut seen: HashSet<usize> = HashSet::new();
    let mut stack = vec![ents];
    let mut curves: HashSet<usize> = HashSet::new();
    let mut arcs = 0;
    while let Some(e) = stack.pop() {
        let faces = list(
            |n| unsafe { (a.SUEntitiesGetNumFaces)(e, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetFaces)(e, n, p, g) },
        );
        *count.entry("faces").or_default() += faces.len();
        for f in faces {
            for front in [true, false] {
                // SketchUp keeps the position of a texture painted over with a colour; it is
                // only seen (and only written back) with a texture.
                let mut mat = Ref::NULL;
                let mut tex = Ref::NULL;
                let r = unsafe {
                    if front { (a.SUFaceGetFrontMaterial)(f, &mut mat) } else { (a.SUFaceGetBackMaterial)(f, &mut mat) }
                };
                if r != 0 || unsafe { (a.SUMaterialGetTexture)(mat, &mut tex) } != 0 {
                    continue;
                }
                let mut pos = false;
                unsafe { (a.SUFaceIsTexturePositioned)(f, front, &mut pos) };
                if pos {
                    *count.entry("positioned sides").or_default() += 1;
                    // Where the texture is, at the face's first point.
                    let mut lp = Ref::NULL;
                    unsafe { (a.SUFaceGetOuterLoop)(f, &mut lp) };
                    let vs = list(
                        |n| unsafe { (a.SULoopGetNumVertices)(lp, n) },
                        |n, p, g| unsafe { (a.SULoopGetVertices)(lp, n, p, g) },
                    );
                    let mut uvh = Ref::NULL;
                    unsafe { (a.SUFaceGetUVHelper)(f, true, true, Ref::NULL, &mut uvh) };
                    let mut sig = String::new();
                    for v in vs.iter().take(3) {
                        let mut p = P3::default();
                        let mut q = Uvq::default();
                        unsafe {
                            (a.SUVertexGetPosition)(*v, &mut p);
                            if front {
                                (a.SUUVHelperGetFrontUVQ)(uvh, &p, &mut q);
                            } else {
                                (a.SUUVHelperGetBackUVQ)(uvh, &p, &mut q);
                            }
                        }
                        let r = |x: f64| format!("{x:.4}").replace("-0.0000", "0.0000");
                        sig += &format!("({},{}) ", r(q.u / q.q), r(q.v / q.q));
                    }
                    unsafe { (a.SUUVHelperRelease)(&mut uvh) };
                    let mut p0 = P3::default();
                    if let Some(v) = vs.first() {
                        unsafe { (a.SUVertexGetPosition)(*v, &mut p0) };
                    }
                    add(format!("uv at ({:.3},{:.3},{:.3}) {front}", p0.x, p0.y, p0.z), sig);
                }
            }
            // Counted, not read: `SUEntityGetAttributeDictionaries` loses the dictionaries that
            // follow a face's texture placement (docs/skp-format.md, §4), the count is right.
            let mut n = 0;
            unsafe { (a.SUEntityGetNumAttributeDictionaries)((a.SUFaceToEntity)(f), &mut n) };
            *count.entry("face dicts").or_default() += n;
        }
        let edges = list(
            |n| unsafe { (a.SUEntitiesGetNumEdges)(e, false, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetEdges)(e, false, n, p, g) },
        );
        *count.entry("edges").or_default() += edges.len();
        for ed in edges {
            let mut c = Ref::NULL;
            if unsafe { (a.SUEdgeGetCurve)(ed, &mut c) } == 0 && !c.is_null() && curves.insert(c.0 as usize) {
                let mut ty = 0;
                unsafe { (a.SUCurveGetType)(c, &mut ty) };
                arcs += (ty == 1) as usize;
            }
            let mut soft = false;
            unsafe { (a.SUEdgeGetSoft)(ed, &mut soft) };
            *count.entry("soft edges").or_default() += soft as usize;
            *count.entry("edge dicts").or_default() += read_attrs(a, unsafe { (a.SUEdgeToEntity)(ed) }).len();
        }
        let n = |c: &dyn Fn(&mut usize) -> R| {
            let mut x = 0;
            c(&mut x);
            x
        };
        *count.entry("texts").or_default() += n(&|x| unsafe { (a.SUEntitiesGetNumTexts)(e, x) });
        *count.entry("dimensions").or_default() += n(&|x| unsafe { (a.SUEntitiesGetNumDimensions)(e, x) });
        *count.entry("guide lines").or_default() += n(&|x| unsafe { (a.SUEntitiesGetNumGuideLines)(e, x) });
        *count.entry("guide points").or_default() += n(&|x| unsafe { (a.SUEntitiesGetNumGuidePoints)(e, x) });
        *count.entry("section planes").or_default() += n(&|x| unsafe { (a.SUEntitiesGetNumSectionPlanes)(e, x) });
        *count.entry("images").or_default() += n(&|x| unsafe { (a.SUEntitiesGetNumImages)(e, x) });
        let groups = list(
            |n| unsafe { (a.SUEntitiesGetNumGroups)(e, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetGroups)(e, n, p, g) },
        );
        *count.entry("groups").or_default() += groups.len();
        for g in groups {
            *count.entry("instance dicts").or_default() += read_attrs(a, unsafe { (a.SUGroupToEntity)(g) }).len();
            let mut sub = Ref::NULL;
            unsafe { (a.SUGroupGetEntities)(g, &mut sub) };
            stack.push(sub);
        }
        let insts = list(
            |n| unsafe { (a.SUEntitiesGetNumInstances)(e, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetInstances)(e, n, p, g) },
        );
        *count.entry("instances").or_default() += insts.len();
        for i in insts {
            *count.entry("instance dicts").or_default() +=
                read_attrs(a, unsafe { (a.SUComponentInstanceToEntity)(i) }).len();
            let mut d = Ref::NULL;
            unsafe { (a.SUComponentInstanceGetDefinition)(i, &mut d) };
            if seen.insert(d.0 as usize) {
                let name = read_string(a, |s| unsafe { (a.SUComponentDefinitionGetName)(d, s) });
                let desc = read_string(a, |s| unsafe { (a.SUComponentDefinitionGetDescription)(d, s) });
                let mut b = Behavior::default();
                unsafe { (a.SUComponentDefinitionGetBehavior)(d, &mut b) };
                let dicts = read_attrs(a, unsafe { (a.SUComponentDefinitionToEntity)(d) }).len();
                add(
                    format!("definition {name}"),
                    format!(
                        "{desc:?} snap {} cuts {} face {} sun {} mask {} dicts {dicts}",
                        b.snap, b.cuts_opening, b.always_face_camera, b.shadows_face_sun, b.no_scale_mask
                    ),
                );
                let mut sub = Ref::NULL;
                unsafe { (a.SUComponentDefinitionGetEntities)(d, &mut sub) };
                stack.push(sub);
            }
        }
    }
    *count.entry("curves").or_default() += curves.len();
    *count.entry("arcs").or_default() += arcs;
    unsafe { (a.SUModelRelease)(&mut m) };
    for (k, v) in count {
        out.insert(format!("count {k}"), v.to_string());
    }
    out
}

/// Files of a SketchUp folder with an extension, recursively.
fn files_in(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            files_in(&p, ext, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) {
            out.push(p);
        }
    }
}

/// The .skp files SketchUp installs (French templates, those of its extensions), read-only,
/// and one made by SketchUp's SDK with the materials it ships (.skm, textured), painted in
/// every way a face can be.
fn sketchup_files(scratch: &Path) -> Vec<PathBuf> {
    let Some(dll) = find_sketchup_api() else { return Vec::new() };
    let root = dll.parent().unwrap();
    let mut v = Vec::new();
    files_in(&root.join("resources").join("fr").join("Templates"), "skp", &mut v);
    files_in(&root.join("ShippedExtensions"), "skp", &mut v);
    let mut skm = Vec::new();
    files_in(&root.join("ShippedExtensions"), "skm", &mut skm);
    skm.sort();
    if !skm.is_empty() {
        let p = scratch.join("matieres-sketchup.skp");
        let _l = CALLS.lock().unwrap_or_else(|e| e.into_inner());
        textured_by_sketchup(&skm, &p);
        v.push(p);
    }
    v.sort();
    v
}

/// A model made through the SDK alone: one face per material of SketchUp, by turns plain,
/// positioned, distorted, on its back, on slopes and walls.
fn textured_by_sketchup(skm: &[PathBuf], out: &Path) {
    let a = api().unwrap();
    let mut m = Ref::NULL;
    su!(a.SUModelCreate(&mut m)).unwrap();
    let mut ents = Ref::NULL;
    su!(a.SUModelGetEntities(m, &mut ents)).unwrap();
    let mut gi = Ref::NULL;
    su!(a.SUGeometryInputCreate(&mut gi)).unwrap();
    let mut pts: Vec<P3> = Vec::new();
    for (i, f) in skm.iter().enumerate() {
        let p = cstr(&f.to_string_lossy());
        let mut mat = Ref::NULL;
        if unsafe { (a.SUModelLoadMaterial)(m, p.as_ptr(), &mut mat) } != 0 {
            continue;
        }
        // A 40 × 30 in rectangle: horizontal, vertical, or sloped.
        let o = DVec3::new(i as f64 * 60.0, 0.0, 0.0);
        let (u, v) = match i % 3 {
            0 => (DVec3::X, DVec3::Y),
            1 => (DVec3::X, DVec3::Z),
            _ => (DVec3::X, DVec3::new(0.0, 0.6, 0.8)),
        };
        let base = pts.len();
        for q in [o, o + u * 40.0, o + u * 40.0 + v * 30.0, o + v * 30.0] {
            pts.push(P3 { x: q.x, y: q.y, z: q.z });
        }
        su!(a.SUGeometryInputSetVertices(gi, pts.len(), pts.as_ptr())).unwrap();
        let mut li = Ref::NULL;
        su!(a.SULoopInputCreate(&mut li)).unwrap();
        for k in 0..4 {
            su!(a.SULoopInputAddVertexIndex(li, base + k)).unwrap();
        }
        let mut fi = 0;
        su!(a.SUGeometryInputAddFace(gi, &mut li, &mut fi)).unwrap();
        let corner = |k: usize| pts[base + k];
        let positioned = |n: usize, uv: [P2; 4]| MaterialPositionInput {
            num_uv_coords: n,
            uv_coords: uv,
            points: [corner(0), corner(1), corner(2), corner(3)],
            material: mat,
            projection: P3::default(),
        };
        let p2 = |x: f64, y: f64| P2 { x, y };
        match i % 4 {
            0 => {
                let mi = MaterialInput {
                    num_uv_coords: 0,
                    uv_coords: [P2::default(); 4],
                    vertex_indices: [0; 4],
                    material: mat,
                };
                su!(a.SUGeometryInputFaceSetFrontMaterial(gi, fi, &mi)).unwrap();
            }
            1 => {
                // Turned and scaled.
                let mp = positioned(3, [p2(0.2, 0.1), p2(1.4, 0.9), p2(0.6, 2.0), P2::default()]);
                su!(a.SUGeometryInputFaceSetFrontMaterialByPosition(gi, fi, &mp)).unwrap();
            }
            2 => {
                // Distorted (perspective).
                let mp = positioned(4, [p2(0.0, 0.0), p2(1.0, 0.0), p2(0.8, 0.7), p2(0.1, 1.0)]);
                su!(a.SUGeometryInputFaceSetFrontMaterialByPosition(gi, fi, &mp)).unwrap();
            }
            _ => {
                let mp = positioned(2, [p2(0.5, 0.5), p2(2.0, 0.5), P2::default(), P2::default()]);
                su!(a.SUGeometryInputFaceSetBackMaterialByPosition(gi, fi, &mp)).unwrap();
                let mi = MaterialInput {
                    num_uv_coords: 0,
                    uv_coords: [P2::default(); 4],
                    vertex_indices: [0; 4],
                    material: mat,
                };
                su!(a.SUGeometryInputFaceSetFrontMaterial(gi, fi, &mi)).unwrap();
            }
        }
    }
    su!(a.SUEntitiesFill(ents, gi, true)).unwrap();
    unsafe { (a.SUGeometryInputRelease)(&mut gi) };
    let p = cstr(&out.to_string_lossy());
    su!(a.SUModelSaveToFile(m, p.as_ptr())).unwrap();
    unsafe { (a.SUModelRelease)(&mut m) };
}

#[test]
fn sketchup_files_go_through_esquisse_without_loss() {
    if api().is_err() {
        return;
    }
    let out = std::env::temp_dir().join(format!("esq-census-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let made = out.join("made");
    std::fs::create_dir_all(&made).unwrap();
    let files = sketchup_files(&made);
    assert!(files.len() > 10, "{files:?}");
    let mut report = Vec::new();
    for f in &files {
        let before = {
            let _l = CALLS.lock().unwrap_or_else(|e| e.into_inner());
            census(f)
        };
        let (model, view) = import_skp_view(f).unwrap();
        let copy = out.join(f.file_name().unwrap());
        export_skp_view(&model, &copy, view.as_ref()).unwrap();
        let after = {
            let _l = CALLS.lock().unwrap_or_else(|e| e.into_inner());
            census(&copy)
        };
        for (k, v) in &before {
            let w = after.get(k).map(String::as_str).unwrap_or("<absent>");
            if v != w {
                report.push(format!("{}: {k}: {v} → {w}", f.file_name().unwrap().to_string_lossy()));
            }
        }
        for (k, w) in &after {
            if !before.contains_key(k) {
                report.push(format!("{}: {k}: <absent> → {w}", f.file_name().unwrap().to_string_lossy()));
            }
        }
    }
    let _ = std::fs::remove_dir_all(out);
    assert!(report.is_empty(), "{} différences :\n{}", report.len(), report.join("\n"));
}

/// Component snaps are read (the SDK can read them but has no function that adds one to an
/// entities collection, so a .skp written by Esquisse cannot carry them).
#[test]
fn component_snaps_are_read() {
    let Ok(a) = api() else { return };
    let t = {
        let _l = CALLS.lock().unwrap_or_else(|e| e.into_inner());
        snap_type(a)
    };
    // The API makes snaps: their type is known, after the older ones.
    assert!(t.is_some_and(|t| t > 50), "{t:?}");
    let Some(dll) = find_sketchup_api() else { return };
    let mut files = Vec::new();
    files_in(dll.parent().unwrap(), "skp", &mut files);
    if let Some(pd) = std::env::var_os("ProgramData") {
        files_in(&PathBuf::from(pd).join("SketchUp"), "skp", &mut files);
    }
    let mut found = 0;
    for f in &files {
        let Ok(m) = import_skp(f) else { continue };
        let snaps: Vec<_> = m.root.snaps.iter().chain(m.defs.values().flat_map(|d| d.entities.snaps.iter())).collect();
        for s in &snaps {
            assert!((s.dir.length() - 1.0).abs() < 1e-6 && (s.up.length() - 1.0).abs() < 1e-6, "{f:?} {s:?}");
            assert!(s.dir.dot(s.up).abs() < 1e-6, "{f:?} {s:?}");
        }
        found += snaps.len();
    }
    eprintln!("type {t:?}: {found} snaps in {} files", files.len());
}

/// The scene selected when SketchUp saved the file (`SUModelGetActiveScene`) is read.
#[test]
fn the_active_scene_is_read() {
    let Ok(a) = api() else { return };
    let dir = std::env::temp_dir().join(format!("esq_active_scene_skp_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let f = dir.join("scenes.skp");
    let mut m = Model::new();
    let cam = esq_core::model::SceneCamera {
        eye: DVec3::new(-1000.0, -1000.0, 1000.0),
        target: DVec3::ZERO,
        up: DVec3::Z,
        fov_deg: 35.0,
        perspective: true,
    };
    for n in ["Une", "Deux"] {
        m.scenes.push(esq_core::model::Scene {
            name: n.into(),
            description: String::new(),
            camera: cam,
            hidden_layers: None,
            photo: None,
            ext: Default::default(),
        });
    }
    export_skp(&m, &f).unwrap();
    assert!(!import_skp(&f).unwrap().view_ext.contains_key(esq_core::model::ACTIVE_SCENE));
    {
        // SketchUp selects the second scene and saves.
        let _l = CALLS.lock().unwrap_or_else(|e| e.into_inner());
        let p = cstr(&f.to_string_lossy());
        let (mut sm, mut status) = (Ref::NULL, 0);
        su!(a.SUModelCreateFromFileWithStatus(&mut sm, p.as_ptr(), &mut status)).unwrap();
        let scenes =
            list(|n| unsafe { (a.SUModelGetNumScenes)(sm, n) }, |n, q, g| unsafe { (a.SUModelGetScenes)(sm, n, q, g) });
        su!(a.SUModelSetActiveScene(sm, scenes[1])).unwrap();
        su!(a.SUModelSaveToFile(sm, p.as_ptr())).unwrap();
        unsafe { (a.SUModelRelease)(&mut sm) };
    }
    let back = import_skp(&f).unwrap();
    assert_eq!(back.view_ext.get(esq_core::model::ACTIVE_SCENE), Some(&serde_json::json!(1)));
    let _ = std::fs::remove_dir_all(&dir);
}
