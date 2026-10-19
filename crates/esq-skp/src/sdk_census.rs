//! What the SDK (SketchUpAPI.dll) exposes of the classes a .skp keeps beyond the geometry:
//! styles and the style of each scene, classifications, radial dimensions, 3D polylines,
//! fonts, the dimension style, the fonts of texts, environments. One list of lines per class,
//! independent of ids and of the order of creation where SketchUp's order is not meaningful.
//! The tests read a file of SketchUp's and the same file written by Esquisse through it, and
//! compare (`tests/skp_complet.rs`).

use std::collections::BTreeMap;

use super::*;

/// The classes of [`sdk_census`], in the order of the report.
pub const CENSUS_CLASSES: &[&str] = &[
    "styles",
    "style actif",
    "styles des scènes",
    "classifications",
    "cotes radiales",
    "polylignes",
    "polices",
    "style de cote",
    "polices des textes",
    "environnements",
];

/// A scalar the SDK writes into an 8-byte slot (whatever its declared size).
pub(crate) fn get(f: impl FnOnce(*mut u64) -> R) -> Option<u64> {
    let mut v = 0u64;
    (f(&mut v) == 0).then_some(v)
}

fn b(v: Option<u64>) -> String {
    v.map_or("-".into(), |x| ((x & 0xff) != 0).to_string())
}

fn d(v: Option<u64>) -> String {
    v.map_or("-".into(), |x| {
        let f = f64::from_bits(x);
        format!("{}", (f * 1e6).round() / 1e6)
    })
}

fn c(v: Option<u64>) -> String {
    v.map_or("-".into(), |x| format!("{:?}", (x as u32).to_le_bytes()))
}

fn p(q: &P3) -> String {
    let r = |x: f64| {
        let v = (x * 1e4).round() / 1e4;
        if v == 0.0 { "0".to_string() } else { format!("{v}") }
    };
    format!("({} {} {})", r(q.x), r(q.y), r(q.z))
}

/// The style properties `SUStyleGetProperty` knows (the enum → the item of
/// the style: edge colour, extensions, profiles, depth cue, background colour).
const STYLE_PROPERTIES: [i32; 8] = [0, 1, 2, 3, 4, 5, 6, 0x40];

fn style_line(a: &Api, s: Ref) -> String {
    let name = read_string(a, |x| unsafe { (a.SUStyleGetName)(s, x) });
    let desc = read_string(a, |x| unsafe { (a.SUStyleGetDescription)(s, x) });
    let guid = read_string(a, |x| unsafe { (a.SUStyleGetGuid)(s, x) });
    let path = read_string(a, |x| unsafe { (a.SUStyleGetPath)(s, x) });
    let wm = b(get(|x| unsafe { (a.SUStyleGetDisplaysWatermark)(s, x) }));
    let mut props = Vec::new();
    for k in STYLE_PROPERTIES {
        let mut v = Ref::NULL;
        if unsafe { (a.SUTypedValueCreate)(&mut v) } != 0 {
            continue;
        }
        let r = unsafe { (a.SUStyleGetProperty)(s, k, &mut v) };
        props.push(if r == 0 {
            model_info::typed_plain(a, v).map_or("?".into(), |j| j.to_string())
        } else {
            "-".into()
        });
        unsafe { (a.SUTypedValueRelease)(&mut v) };
    }
    format!("{name:?} desc={desc:?} guid={guid} chemin={path:?} filigrane={wm} [{}]", props.join(" "))
}

fn style_name(a: &Api, s: Ref) -> String {
    if s.is_null() {
        return "-".into();
    }
    read_string(a, |x| unsafe { (a.SUStyleGetName)(s, x) })
}

fn font_line(a: &Api, f: Ref) -> String {
    if f.is_null() {
        return "-".into();
    }
    let face = read_string(a, |x| unsafe { (a.SUFontGetFaceName)(f, x) });
    format!(
        "{face:?} {} gras={} italique={} monde={} {}",
        get(|x| unsafe { (a.SUFontGetPointSize)(f, x) }).map_or("-".into(), |v| v.to_string()),
        b(get(|x| unsafe { (a.SUFontGetBold)(f, x) })),
        b(get(|x| unsafe { (a.SUFontGetItalic)(f, x) })),
        b(get(|x| unsafe { (a.SUFontGetUseWorldSize)(f, x) })),
        d(get(|x| unsafe { (a.SUFontGetWorldSize)(f, x) })),
    )
}

/// Reads `path` through the SDK. `None` when the SDK does not open it.
pub(crate) fn census_of(a: &Api, path: &Path) -> Option<BTreeMap<&'static str, Vec<String>>> {
    let mut out: BTreeMap<&'static str, Vec<String>> = CENSUS_CLASSES.iter().map(|c| (*c, Vec::new())).collect();
    let cp = cstr(&path.to_string_lossy());
    let (mut m, mut status) = (Ref::NULL, 0);
    if unsafe { (a.SUModelCreateFromFileWithStatus)(&mut m, cp.as_ptr(), &mut status) } != 0 {
        return None;
    }
    let mut push = |k: &'static str, s: String| out.entry(k).or_default().push(s);

    // Styles, in the model's order.
    let mut styles = Ref::NULL;
    if unsafe { (a.SUModelGetStyles)(m, &mut styles) } == 0 && !styles.is_null() {
        let list = list(
            |n| unsafe { (a.SUStylesGetNumStyles)(styles, n) },
            |n, q, g| unsafe { (a.SUStylesGetStyles)(styles, n, q, g) },
        );
        // The SDK lists them in the order of a set of pointers (`CSkpStyleManager`): sorted.
        for s in &list {
            push("styles", style_line(a, *s));
        }
        let mut sel = Ref::NULL;
        unsafe { (a.SUStylesGetSelectedStyle)(styles, &mut sel) };
        let mut act = Ref::NULL;
        unsafe { (a.SUStylesGetActiveStyle)(styles, &mut act) };
        let changed = b(get(|x| unsafe { (a.SUStylesGetActiveStyleChanged)(styles, x) }));
        push("style actif", format!("sélectionné={:?} modifié={changed}", style_name(a, sel)));
        if !act.is_null() {
            // The active style: the selected one, or the copy that holds its changes.
            let line = style_line(a, act);
            let line = line
                .split_once(" guid=")
                .map_or(line.clone(), |(n, r)| format!("{n} {}", r.split_once(' ').map_or("", |x| x.1)));
            push("style actif", format!("actif={line}"));
        }
    }
    // Scenes: their style, environment, photo.
    let scenes =
        list(|n| unsafe { (a.SUModelGetNumScenes)(m, n) }, |n, q, g| unsafe { (a.SUModelGetScenes)(m, n, q, g) });
    for s in &scenes {
        let name = read_string(a, |x| unsafe { (a.SUSceneGetName)(*s, x) });
        let mut st = Ref::NULL;
        let style = if unsafe { (a.SUSceneGetStyle)(*s, &mut st) } == 0 { style_name(a, st) } else { "-".into() };
        let mut env = Ref::NULL;
        let env_name = if unsafe { (a.SUSceneGetEnvironment)(*s, &mut env) } == 0 && !env.is_null() {
            read_string(a, |x| unsafe { (a.SUEnvironmentGetName)(env, x) })
        } else {
            "-".into()
        };
        push(
            "styles des scènes",
            format!(
                "{name:?} style={style:?} env={env_name:?} utilise_env={} photo={}",
                b(get(|x| unsafe { (a.SUSceneGetUseEnvironment)(*s, x) })),
                b(get(|x| unsafe { (a.SUSceneGetPhotoMatchImageDisplayed)(*s, x) })),
            ),
        );
    }
    // Classification schemas.
    let mut cl = Ref::NULL;
    if unsafe { (a.SUModelGetClassifications)(m, &mut cl) } == 0 && !cl.is_null() {
        let schemas = list(
            |n| unsafe { (a.SUClassificationsGetNumSchemas)(cl, n) },
            |n, q, g| unsafe { (a.SUClassificationsGetSchemas)(cl, n, q, g) },
        );
        for s in schemas {
            push(
                "classifications",
                format!("schéma {:?}", read_string(a, |x| unsafe { (a.SUSchemaGetSchemaName)(s, x) })),
            );
        }
    }
    // Fonts.
    for f in list(|n| unsafe { (a.SUModelGetNumFonts)(m, n) }, |n, q, g| unsafe { (a.SUModelGetFonts)(m, n, q, g) }) {
        push("polices", font_line(a, f));
    }
    // The dimension style.
    let mut ds = Ref::NULL;
    if unsafe { (a.SUModelGetDimensionStyle)(m, &mut ds) } == 0 && !ds.is_null() {
        let mut f = Ref::NULL;
        unsafe { (a.SUDimensionStyleGetFont)(ds, &mut f) };
        let n = |v: Option<u64>| v.map_or("-".into(), |x| (x as u32 as i32).to_string());
        push("style de cote", format!("police {}", font_line(a, f)));
        push(
            "style de cote",
            format!(
                "3d={} flèche={} taille={} couleur={} texte={} trait={} décalage={} dépassement={} préfixe={} hors_plan={} {} petites={} {} non_associées={} {}",
                b(get(|x| unsafe { (a.SUDimensionStyleGet3D)(ds, x) })),
                n(get(|x| unsafe { (a.SUDimensionStyleGetArrowType)(ds, x) })),
                n(get(|x| unsafe { (a.SUDimensionStyleGetArrowSize)(ds, x) })),
                c(get(|x| unsafe { (a.SUDimensionStyleGetColor)(ds, x) })),
                c(get(|x| unsafe { (a.SUDimensionStyleGetTextColor)(ds, x) })),
                n(get(|x| unsafe { (a.SUDimensionStyleGetLineWeight)(ds, x) })),
                n(get(|x| unsafe { (a.SUDimensionStyleGetExtensionLineOffset)(ds, x) })),
                n(get(|x| unsafe { (a.SUDimensionStyleGetExtensionLineOvershoot)(ds, x) })),
                b(get(|x| unsafe { (a.SUDimensionStyleGetShowRadialPrefix)(ds, x) })),
                b(get(|x| unsafe { (a.SUDimensionStyleGetHideOutOfPlane)(ds, x) })),
                d(get(|x| unsafe { (a.SUDimensionStyleGetHideOutOfPlaneValue)(ds, x) })),
                b(get(|x| unsafe { (a.SUDimensionStyleGetHideSmall)(ds, x) })),
                d(get(|x| unsafe { (a.SUDimensionStyleGetHideSmallValue)(ds, x) })),
                b(get(|x| unsafe { (a.SUDimensionStyleGetHighlightNonAssociativeDimensions)(ds, x) })),
                c(get(|x| unsafe { (a.SUDimensionStyleGetHighlightNonAssociativeDimensionsColor)(ds, x) })),
            ),
        );
    }
    // Environments.
    let mut envs = Ref::NULL;
    if unsafe { (a.SUModelGetEnvironments)(m, &mut envs) } == 0 && !envs.is_null() {
        let all = list(
            |n| unsafe { (a.SUEnvironmentsGetCount)(envs, n) },
            |n, q, g| unsafe { (a.SUEnvironmentsGetAll)(envs, n, q, g) },
        );
        for e in all {
            push(
                "environnements",
                format!(
                    "{:?} desc={:?} rotation={} ciel={} {} reflets={} {} soleil={}",
                    read_string(a, |x| unsafe { (a.SUEnvironmentGetName)(e, x) }),
                    read_string(a, |x| unsafe { (a.SUEnvironmentGetDescription)(e, x) }),
                    d(get(|x| unsafe { (a.SUEnvironmentGetRotation)(e, x) })),
                    b(get(|x| unsafe { (a.SUEnvironmentGetUseAsSkydome)(e, x) })),
                    d(get(|x| unsafe { (a.SUEnvironmentGetSkydomeExposure)(e, x) })),
                    b(get(|x| unsafe { (a.SUEnvironmentGetUseForReflections)(e, x) })),
                    d(get(|x| unsafe { (a.SUEnvironmentGetReflectionExposure)(e, x) })),
                    b(get(|x| unsafe { (a.SUEnvironmentGetLinkedSun)(e, x) })),
                ),
            );
        }
        let mut sel = Ref::NULL;
        if unsafe { (a.SUEnvironmentsGetSelectedEnvironment)(envs, &mut sel) } == 0 && !sel.is_null() {
            push(
                "environnements",
                format!("courant={:?}", read_string(a, |x| unsafe { (a.SUEnvironmentGetName)(sel, x) })),
            );
        }
    }

    // Entities: each definition once, the model's collection first.
    let mut ents = Ref::NULL;
    unsafe { (a.SUModelGetEntities)(m, &mut ents) };
    let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut stack = vec![(ents, "<racine>".to_string())];
    while let Some((e, owner)) = stack.pop() {
        if e.is_null() {
            continue;
        }
        for dm in list(
            |n| unsafe { (a.SUEntitiesGetNumDimensions)(e, n) },
            |n, q, g| unsafe { (a.SUEntitiesGetDimensions)(e, n, q, g) },
        ) {
            let mut ty = 0;
            unsafe { (a.SUDimensionGetType)(dm, &mut ty) };
            if ty != 2 {
                continue;
            }
            let r = unsafe { (a.SUDimensionRadialFromDimension)(dm) };
            let text = read_string(a, |x| unsafe { (a.SUDimensionGetText)(dm, x) });
            let mut arrow = 0;
            unsafe { (a.SUDimensionGetArrowType)(dm, &mut arrow) };
            let mut pts = [P3::default(); 3];
            let lp = unsafe { (a.SUDimensionRadialGetLeaderPoints)(r, pts.as_mut_ptr()) } == 0;
            let mut f = Ref::NULL;
            unsafe { (a.SUDimensionGetFont)(dm, &mut f) };
            push(
                "cotes radiales",
                format!(
                    "{owner} {text:?} diamètre={} flèche={arrow} points={} police {}",
                    b(get(|x| unsafe { (a.SUDimensionRadialGetDiameter)(r, x) })),
                    if lp { pts.iter().map(p).collect::<Vec<_>>().join(" ") } else { "-".into() },
                    font_line(a, f)
                ),
            );
        }
        for pl in list(
            |n| unsafe { (a.SUEntitiesGetNumPolyline3ds)(e, n) },
            |n, q, g| unsafe { (a.SUEntitiesGetPolyline3ds)(e, n, q, g) },
        ) {
            let mut n = 0;
            unsafe { (a.SUPolyline3dGetNumPoints)(pl, &mut n) };
            let mut pts = vec![P3::default(); n.min(1 << 20)];
            let mut got = 0;
            unsafe { (a.SUPolyline3dGetPoints)(pl, pts.len(), pts.as_mut_ptr(), &mut got) };
            pts.truncate(got);
            push("polylignes", format!("{owner} {}", pts.iter().map(p).collect::<Vec<_>>().join(" ")));
        }
        for t in list(
            |n| unsafe { (a.SUEntitiesGetNumTexts)(e, n) },
            |n, q, g| unsafe { (a.SUEntitiesGetTexts)(e, n, q, g) },
        ) {
            let s = read_string(a, |x| unsafe { (a.SUTextGetString)(t, x) });
            let mut f = Ref::NULL;
            unsafe { (a.SUTextGetFont)(t, &mut f) };
            push(
                "polices des textes",
                format!(
                    "{owner} {s:?} {} couleur={}",
                    font_line(a, f),
                    c(get(|x| unsafe { (a.SUTextGetColor)(t, x) }))
                ),
            );
        }
        let groups = list(
            |n| unsafe { (a.SUEntitiesGetNumGroups)(e, n) },
            |n, q, g| unsafe { (a.SUEntitiesGetGroups)(e, n, q, g) },
        );
        let insts = list(
            |n| unsafe { (a.SUEntitiesGetNumInstances)(e, n) },
            |n, q, g| unsafe { (a.SUEntitiesGetInstances)(e, n, q, g) },
        );
        for g in groups {
            let mut def = Ref::NULL;
            unsafe { (a.SUGroupGetDefinition)(g, &mut def) };
            if seen.insert(def.0 as usize) {
                let mut sub = Ref::NULL;
                unsafe { (a.SUGroupGetEntities)(g, &mut sub) };
                let name = read_string(a, |x| unsafe { (a.SUComponentDefinitionGetName)(def, x) });
                stack.push((sub, name));
            }
        }
        for i in insts {
            let mut def = Ref::NULL;
            unsafe { (a.SUComponentInstanceGetDefinition)(i, &mut def) };
            // The types applied to each instance (the definition's, once per definition).
            if seen.insert(def.0 as usize) {
                let name = read_string(a, |x| unsafe { (a.SUComponentDefinitionGetName)(def, x) });
                let mut info = Ref::NULL;
                if unsafe { (a.SUComponentInstanceCreateClassificationInfo)(i, &mut info) } == 0 && !info.is_null() {
                    let mut n = 0;
                    unsafe { (a.SUClassificationInfoGetNumSchemas)(info, &mut n) };
                    for k in 0..n.min(1024) {
                        let sn = read_string(a, |x| unsafe { (a.SUClassificationInfoGetSchemaName)(info, k, x) });
                        let st = read_string(a, |x| unsafe { (a.SUClassificationInfoGetSchemaType)(info, k, x) });
                        push("classifications", format!("{name:?} {sn:?} {st:?}"));
                    }
                    unsafe { (a.SUClassificationInfoRelease)(&mut info) };
                }
                let mut sub = Ref::NULL;
                unsafe { (a.SUComponentDefinitionGetEntities)(def, &mut sub) };
                stack.push((sub, name));
            }
        }
    }
    unsafe { (a.SUModelRelease)(&mut m) };
    for v in out.values_mut() {
        v.sort();
    }
    Some(out)
}
