//! The styles of a model (`CSkpStyleManager`, `CSkpStyle`) and its watermarks
//! (`CWatermarkManager`, `CWatermark`), as both file formats keep them, and as Esquisse keeps
//! them (`Model::view_ext["styles"]`, the list the Styles panel and `Sketchup::Styles` share).
//!
//! A style is a set of numbered items (`<sty:item id>` of `style.xml` in 2021+, the property
//! set of `CSkpStyle` in an MFC archive): 1000 edges, 2000 faces, 4000
//! background, 5000 watermarks, 7000 modeling, 8000 photo matching, 8100 ambient occlusion.
//! Most are rendering options (`ITEMS`, read in SketchUp.exe: the handler that turns a change
//! of rendering option into a style item); 1013 is the sketchy edges,
//! 5001 the watermarks. Esquisse keeps the rendering options of a style in
//! `ro`, its watermarks in `wm` (the application's `Watermark`), and every other item as it
//! was read (`skp_items`), so that a style it did not change is written back as it was.
//! `docs/skp-format.md` §10.

use std::sync::Arc;

use serde_json::{Map, Value, json};

use super::doc::Id;
use super::xml::Element;

/// The sketchy edges of a style (item 1013, `<n0:nprEdge>` /).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Npr {
    pub dropout: f64,
    pub fade: f64,
    pub width: u32,
    pub halo: bool,
    pub halo_length: u32,
    pub protos: u32,
    pub lengths: Vec<u32>,
    /// The stroke images (PNG), in the file's order.
    pub images: Vec<Arc<Vec<u8>>>,
}

/// A watermark (`CWatermark`, `<n1:screenimage>`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wm {
    /// Entity id (2021+: the watermark manager's record the styles name).
    pub id: Id,
    pub name: String,
    /// The image was found where `file` says (`info_found`).
    pub found: bool,
    /// The file the image was loaded from (`info_filename`).
    pub file: String,
    /// When it was loaded, seconds since 1970 (`info_time`).
    pub time: i64,
    /// 0–8, row by row from the top left (`position`).
    pub position: u32,
    /// 0 tiled, 1 stretched, 2 positioned (`fittingType`).
    pub fitting: u32,
    /// Stretched: keeps the image's proportions (`stretchType`, `maintainAR`).
    pub keep_aspect: bool,
    pub background: bool,
    /// Brightness is transparency (`intensForAlpha`).
    pub mask: bool,
    /// Opacity, 0–1 (`alphaScale`).
    pub alpha: f64,
    pub scale: f64,
    /// The image file (PNG, JPEG…).
    pub image: Option<Arc<Vec<u8>>>,
    /// The image's name in the style (`file_name`).
    pub image_name: String,
}

/// Files of the archive: path (or name in a style's folder) and bytes.
pub type Files = Vec<(String, Arc<Vec<u8>>)>;

/// A style's rendering options, watermarks, and the items kept as read.
pub type Split = (Map<String, Value>, Option<Vec<Option<Wm>>>, Vec<Value>);

/// The value of an item: its variant type (1 bool, 2 byte, 3 short, 4 int, 5 unsigned, 6
/// float, 7 double, 9–11 vectors, 12 transformation, 13 object, 14 time, 15 64 bits) and value.
#[derive(Clone, Debug, PartialEq)]
pub enum ItemV {
    /// Types 1–7, 14, 15: the value's bits (a float's or a double's as `to_bits`).
    Num(u32, u64),
    /// Types 9–11.
    Vec3(u32, [f64; 3]),
    /// Type 12.
    Xform(Box<[f64; 13]>),
    /// 1013.
    Npr(Box<Npr>),
    /// 5001: the watermarks, `None` for the model's place between those behind it and those
    /// over it (`<MODEL SPACE>`).
    Watermarks(Vec<Option<Wm>>),
    /// Any other object of a 2021+ style: its XML, as read.
    Xml(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u32,
    pub v: ItemV,
}

/// A style of the model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleRec {
    /// Entity id (2021+), object of the archive (MFC).
    pub id: Id,
    pub guid: String,
    /// The `.style` file it came from (`SUStyleGetPath`).
    pub path: String,
    /// Its folder in the archive (`styles/<folder>/style.xml`, 2021+).
    pub folder: String,
    pub name: String,
    pub description: String,
    /// In the file's order (by id).
    pub items: Vec<Item>,
    /// The watermarks of the model it shows (2021+, record 0x6b70).
    pub watermark_ids: Vec<Id>,
}

/// The styles of a model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Styles {
    pub list: Vec<StyleRec>,
    /// Index in `list` of the selected style.
    pub selected: Option<usize>,
    /// The active style: the copy of the selected one that holds the changes not saved in it.
    pub active: Option<StyleRec>,
    /// The active style differs from the selected one (`SUStylesGetActiveStyleChanged`).
    pub changed: bool,
    /// The model's watermarks (the watermark manager), `None` for the model's place.
    pub watermarks: Vec<Option<Wm>>,
}

/// How an item that is a rendering option holds it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Bool,
    Int,
    Color,
    Double,
    Float,
}

/// Item → rendering option (the SDK's names), as `style_file.rs` reads `.style` files.
pub const ITEMS: &[(u32, &str, Kind)] = &[
    (1000, "EdgeDisplayMode", Kind::Int),
    (1001, "EdgeType", Kind::Int),
    (1002, "EdgeColorMode", Kind::Int),
    (1004, "ExtendLines", Kind::Bool),
    (1005, "LineExtension", Kind::Int),
    (1006, "DrawSilhouettes", Kind::Bool),
    (1007, "SilhouetteWidth", Kind::Int),
    (1008, "DrawDepthQue", Kind::Bool),
    (1009, "DepthQueWidth", Kind::Int),
    (1010, "DrawLineEnds", Kind::Bool),
    (1011, "LineEndWidth", Kind::Int),
    (1012, "JitterEdges", Kind::Bool),
    (1014, "ForegroundColor", Kind::Color),
    (1015, "DrawBackEdges", Kind::Bool),
    (1016, "LineStyleEdges", Kind::Bool),
    (2001, "RenderMode", Kind::Int),
    (2002, "FaceFrontColor", Kind::Color),
    (2003, "FaceBackColor", Kind::Color),
    (2004, "ModelTransparency", Kind::Bool),
    (2005, "MaterialTransparency", Kind::Bool),
    (2006, "TransparencySort", Kind::Int),
    (2007, "Texture", Kind::Bool),
    (2008, "XRayOpacity", Kind::Double),
    (4000, "BackgroundColor", Kind::Color),
    (4001, "SkyColor", Kind::Color),
    (4002, "DrawHorizon", Kind::Bool),
    (4003, "GroundColor", Kind::Color),
    (4004, "DrawGround", Kind::Bool),
    (4005, "GroundTransparency", Kind::Int),
    (4006, "DrawUnderground", Kind::Bool),
    (4007, "HorizonColor", Kind::Color),
    (5000, "DisplayWatermarks", Kind::Bool),
    (7000, "HighlightColor", Kind::Color),
    (7001, "LockedColor", Kind::Color),
    (7002, "ConstructionColor", Kind::Color),
    (7003, "SectionActiveColor", Kind::Color),
    (7004, "SectionInactiveColor", Kind::Color),
    (7005, "SectionDefaultCutColor", Kind::Color),
    (7008, "DisplaySketchAxes", Kind::Bool),
    (7011, "DisplayColorByLayer", Kind::Bool),
    (7012, "HideConstructionGeometry", Kind::Bool),
    (7014, "SectionCutWidth", Kind::Int),
    (7015, "SectionCutFilled", Kind::Bool),
    (7016, "SectionDefaultFillColor", Kind::Color),
    (7017, "DrawHiddenGeometry", Kind::Bool),
    (7018, "DrawHiddenObjects", Kind::Bool),
    (8000, "PhotomatchDrawBackground", Kind::Bool),
    (8001, "PhotomatchBackgroundOpacity", Kind::Double),
    (8002, "PhotomatchDrawOverlay", Kind::Bool),
    (8003, "PhotomatchOverlayOpacity", Kind::Double),
    (8100, "AmbientOcclusion", Kind::Bool),
    (8102, "AmbientOcclusionDistance", Kind::Float),
    (8103, "AmbientOcclusionIntensity", Kind::Float),
    (8105, "AmbientOcclusionColorEnabled", Kind::Bool),
    (8106, "AmbientOcclusionColor", Kind::Color),
    (8107, "AmbientOcclusionMultiplier", Kind::Float),
];

/// Section planes (bit 0) and cuts (bit 1) shown.
pub const SECTION_MODE: u32 = 7013;
/// « Hidden geometry or hidden objects », written for the versions before the two (7017,
/// 7018) were apart.
pub const DRAW_HIDDEN: u32 = 7010;
pub const SKETCHY: u32 = 1013;
pub const WATERMARKS: u32 = 5001;

impl ItemV {
    pub fn ty(&self) -> u32 {
        match self {
            ItemV::Num(t, _) | ItemV::Vec3(t, _) => *t,
            ItemV::Xform(_) => 12,
            ItemV::Npr(_) | ItemV::Watermarks(_) | ItemV::Xml(_) => 13,
        }
    }

    /// A number as a double (bool 0/1, integers, floats, doubles).
    pub fn f64(&self) -> Option<f64> {
        let ItemV::Num(t, b) = self else { return None };
        Some(match t {
            1 | 2 => (*b & 0xff) as f64,
            3 => (*b as u16 as i16) as f64,
            4 | 5 => (*b as u32 as i32) as f64,
            6 => f32::from_bits(*b as u32) as f64,
            7 => f64::from_bits(*b),
            14 | 15 => *b as i64 as f64,
            _ => return None,
        })
    }

    pub fn int(&self) -> Option<i64> {
        let ItemV::Num(t, b) = self else { return None };
        Some(match t {
            1 | 2 => (*b & 0xff) as i64,
            3 => (*b as u16 as i16) as i64,
            4 | 5 => (*b as u32 as i32) as i64,
            6 => f32::from_bits(*b as u32) as i64,
            7 => f64::from_bits(*b) as i64,
            _ => *b as i64,
        })
    }

    /// The value of type `ty` that holds `x`.
    pub fn of_f64(ty: u32, x: f64) -> ItemV {
        let clamp = |x: f64| x.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
        let bits = match ty {
            1 => (x != 0.0) as u64,
            2 => x.round().clamp(0.0, 255.0) as u64,
            3 => x.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16 as u16 as u64,
            4 | 5 => clamp(x) as u32 as u64,
            6 => (x as f32).to_bits() as u64,
            7 => x.to_bits(),
            _ => x as i64 as u64,
        };
        ItemV::Num(ty, bits)
    }
}

/// A colour item (red in the low byte, alpha in the high one) as `[r, g, b, a]`.
fn color_json(v: i64) -> Value {
    let v = v as u32;
    json!([v & 255, v >> 8 & 255, v >> 16 & 255, v >> 24])
}

/// `[r, g, b(, a)]` as a colour item (alpha 255 when not given).
fn color_bits(j: &Value) -> Option<i32> {
    let a = j.as_array().filter(|a| a.len() >= 3)?;
    let c = |i: usize| a.get(i).and_then(Value::as_f64).map(|f| f.round().clamp(0.0, 255.0) as u32);
    Some((c(3).unwrap_or(255) << 24 | c(2)? << 16 | c(1)? << 8 | c(0)?) as i32)
}

// ------------------------------------------------------------------------------ Esquisse

fn b64(b: &[u8]) -> Value {
    Value::String(esq_core::photo::base64_encode(b))
}

fn unb64(v: &Value) -> Option<Arc<Vec<u8>>> {
    esq_core::photo::base64_decode(v.as_str()?).map(Arc::new)
}

/// A watermark as the application keeps it (its `style::Watermark`), with what SketchUp's
/// file has besides.
pub fn wm_json(w: &Wm) -> Value {
    let layout = match w.fitting {
        0 => "Tiled",
        1 => "Stretched",
        _ => "Positioned",
    };
    json!({
        "name": w.name,
        "image": b64(w.image.as_deref().map_or(&[][..], |v| v)),
        "background": w.background,
        "blend": w.alpha,
        "mask": w.mask,
        "layout": layout,
        "keep_aspect": w.keep_aspect,
        "scale": w.scale,
        "position": w.position.min(8),
        "skp": {"found": w.found, "file": w.file, "time": w.time, "image_name": w.image_name},
    })
}

pub fn wm_of(j: &Value) -> Option<Wm> {
    let s = &j["skp"];
    let fitting = match j["layout"].as_str()? {
        "Tiled" => 0,
        "Stretched" => 1,
        _ => 2,
    };
    Some(Wm {
        id: 0,
        name: j["name"].as_str().unwrap_or_default().to_string(),
        found: s["found"].as_bool().unwrap_or(false),
        file: s["file"].as_str().unwrap_or_default().to_string(),
        time: s["time"].as_i64().unwrap_or(0),
        position: j["position"].as_u64().unwrap_or(4).min(8) as u32,
        fitting,
        keep_aspect: j["keep_aspect"].as_bool().unwrap_or(false),
        background: j["background"].as_bool().unwrap_or(false),
        mask: j["mask"].as_bool().unwrap_or(false),
        alpha: j["blend"].as_f64().unwrap_or(1.0),
        scale: j["scale"].as_f64().unwrap_or(1.0),
        image: unb64(&j["image"]).filter(|b| !b.is_empty()),
        image_name: s["image_name"].as_str().unwrap_or_default().to_string(),
    })
}

/// The application's list (those behind the model, then those over it) from SketchUp's
/// (the model's place between them).
pub fn wm_list_json(list: &[Option<Wm>]) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let mut behind = true;
    for w in list {
        match w {
            None => behind = false,
            Some(w) => {
                let mut w = w.clone();
                // Before the model's place: behind it.
                w.background = w.background || behind && list.iter().any(Option::is_none);
                out.push(wm_json(&w));
            }
        }
    }
    Value::Array(out)
}

/// SketchUp's list from the application's: those behind, the model's place, those over it.
pub fn wm_list_of(j: &Value) -> Vec<Option<Wm>> {
    let all: Vec<Wm> = j.as_array().map(|a| a.iter().filter_map(wm_of).collect()).unwrap_or_default();
    let mut out: Vec<Option<Wm>> = all.iter().filter(|w| w.background).cloned().map(Some).collect();
    out.push(None);
    out.extend(all.into_iter().filter(|w| !w.background).map(Some));
    out
}

fn npr_json(n: &Npr) -> Value {
    json!({
        "dropout": n.dropout, "fade": n.fade, "width": n.width, "halo": n.halo,
        "halo_length": n.halo_length, "protos": n.protos, "lengths": n.lengths,
        "images": n.images.iter().map(|i| b64(i)).collect::<Vec<_>>(),
    })
}

fn npr_of(j: &Value) -> Npr {
    let u = |k: &str| j[k].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32;
    Npr {
        dropout: j["dropout"].as_f64().unwrap_or(0.0),
        fade: j["fade"].as_f64().unwrap_or(0.0),
        width: u("width"),
        halo: j["halo"].as_bool().unwrap_or(false),
        halo_length: u("halo_length"),
        protos: u("protos"),
        lengths: j["lengths"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_u64().map(|v| v.min(u32::MAX as u64) as u32)).collect())
            .unwrap_or_default(),
        images: j["images"].as_array().map(|a| a.iter().filter_map(unb64).collect()).unwrap_or_default(),
    }
}

/// An item kept as read (not a rendering option of `ro`, nor the watermarks of `wm`).
fn item_json(it: &Item) -> Value {
    let mut o = Map::new();
    o.insert("id".into(), json!(it.id));
    o.insert("t".into(), json!(it.v.ty()));
    match &it.v {
        ItemV::Num(_, b) => {
            o.insert("bits".into(), json!(b));
        }
        ItemV::Vec3(_, v) => {
            o.insert("v".into(), json!(v));
        }
        ItemV::Xform(x) => {
            o.insert("v".into(), json!(x.to_vec()));
        }
        ItemV::Npr(n) => {
            o.insert("npr".into(), npr_json(n));
        }
        ItemV::Watermarks(w) => {
            // As read: the model's place (null) where it was.
            let raw: Vec<Value> = w.iter().map(|x| x.as_ref().map_or(Value::Null, wm_json)).collect();
            o.insert("wm_raw".into(), Value::Array(raw));
        }
        ItemV::Xml(x) => {
            o.insert("xml".into(), json!(x));
        }
    }
    Value::Object(o)
}

fn item_of(j: &Value) -> Option<Item> {
    let id = j["id"].as_u64()? as u32;
    let ty = j["t"].as_u64()? as u32;
    let v = if let Some(b) = j["bits"].as_u64() {
        ItemV::Num(ty, b)
    } else if let Some(n) = j.get("npr") {
        ItemV::Npr(Box::new(npr_of(n)))
    } else if let Some(w) = j["wm_raw"].as_array() {
        ItemV::Watermarks(w.iter().map(|x| if x.is_null() { None } else { wm_of(x) }).collect())
    } else if let Some(x) = j["xml"].as_str() {
        ItemV::Xml(x.to_string())
    } else {
        let v: Vec<f64> = j["v"].as_array()?.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect();
        match ty {
            12 if v.len() == 13 => ItemV::Xform(Box::new(v.try_into().ok()?)),
            _ if v.len() == 3 => ItemV::Vec3(ty, [v[0], v[1], v[2]]),
            _ => return None,
        }
    };
    Some(Item { id, v })
}

/// The rendering options (SDK names) and watermarks of a style's items, and the items kept as
/// read.
pub fn split(items: &[Item]) -> Split {
    let mut ro = Map::new();
    let mut wm = None;
    let mut rest = Vec::new();
    for it in items {
        if let Some((_, key, kind)) = ITEMS.iter().find(|(i, _, _)| *i == it.id)
            && let Some(x) = it.v.f64()
        {
            let v = match kind {
                Kind::Bool => json!(x != 0.0),
                Kind::Int => json!(x as i64),
                Kind::Double | Kind::Float => json!(x),
                Kind::Color => color_json(it.v.int().unwrap_or(0)),
            };
            ro.insert(key.to_string(), v);
            continue;
        }
        match (&it.v, it.id) {
            (ItemV::Num(..), SECTION_MODE) => {
                let m = it.v.int().unwrap_or(2);
                ro.insert("DisplaySectionPlanes".into(), json!(m & 1 != 0));
                ro.insert("DisplaySectionCuts".into(), json!(m & 2 != 0));
                rest.push(item_json(it));
            }
            (ItemV::Watermarks(w), WATERMARKS) => {
                wm = Some(w.clone());
                // Kept as read too: written back as it was while the list does not change.
                rest.push(item_json(it));
            }
            _ => rest.push(item_json(it)),
        }
    }
    (ro, wm, rest)
}

/// The items of a new style (`super::write::xml::STYLE_ITEMS`, as SketchUp makes them).
pub fn default_items() -> Vec<Item> {
    super::write::xml::STYLE_ITEMS
        .iter()
        .map(|&(id, ty, v)| match ty {
            13 => Item { id, v: ItemV::Watermarks(Vec::new()) },
            t => Item { id, v: ItemV::of_f64(t as u32, v.parse().unwrap_or(0.0)) },
        })
        .collect()
}

/// A style read: the items its file does not have keep the value of a new style (SketchUp
/// fills the set of a new style before it reads one), e.g. the ambient
/// occlusion of a style of an MFC archive.
pub fn fill_defaults(items: &mut Vec<Item>) {
    for d in default_items() {
        if !items.iter().any(|i| i.id == d.id) {
            items.push(d);
        }
    }
    items.sort_by_key(|i| i.id);
}

/// The style SketchUp makes from a model's rendering options when its file has none
/// (SketchUp 3 to 5): a new style with the options' values; the system
/// colours (all zero) resolved to black ink and white paper; 7010 « hidden geometry or
/// hidden objects ».
pub fn from_rendering(ro: &Map<String, Value>) -> Vec<Item> {
    let mut ro = style_part(ro);
    for (k, sys) in [("ForegroundColor", [0, 0, 0, 255]), ("BackgroundColor", [255, 255, 255, 255])] {
        if ro.get(k).is_some_and(|v| v == &json!([0, 0, 0, 0])) {
            ro.insert(k.into(), json!(sys));
        }
    }
    let d = default_items();
    let mut items = join(&ro, None, &d.iter().map(item_json).collect::<Vec<_>>(), &d);
    let on = |k: &str| ro.get(k).and_then(Value::as_bool).unwrap_or(false);
    if let Some(i) = items.iter_mut().find(|i| i.id == DRAW_HIDDEN) {
        i.v = ItemV::Num(1, (on("DrawHiddenGeometry") || on("DrawHiddenObjects")) as u64);
    }
    items
}

/// The styles of a document, with the style of each scene (by index of `Styles::list`): as
/// the file has them, or, for a file without any (SketchUp 3 to 5), as SketchUp makes them
/// when it opens it: the active style of the model's rendering options;
/// a style for each scene that keeps its rendering options, and for the selected scene (of
/// its options, else the model's), one style for equal settings; the selected style is the
/// selected scene's, else one of the active style's settings. Named « Style », « Style1 »…
pub fn of_doc(doc: &super::doc::Doc) -> (Styles, Vec<Option<usize>>) {
    let by_id = |id: Option<Id>| id.and_then(|i| doc.styles.list.iter().position(|s| s.id == i && i != 0));
    if !doc.styles.list.is_empty() {
        return (doc.styles.clone(), doc.scenes.iter().map(|s| by_id(s.style)).collect());
    }
    let map = |ro: &std::collections::BTreeMap<String, Value>| -> Map<String, Value> {
        ro.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    };
    let model_ro = map(&doc.rendering);
    let mut st = Styles { changed: true, ..Default::default() };
    let active_items = from_rendering(&model_ro);
    // A style of these settings: an equal one of the list, else a new one.
    let add = |st: &mut Styles, items: Vec<Item>| -> usize {
        if let Some(i) = st.list.iter().position(|s| s.items == items) {
            return i;
        }
        let k = st.list.len();
        st.list.push(StyleRec {
            guid: esq_core::meta::new_guid().to_lowercase(),
            name: if k == 0 { "Style".into() } else { format!("Style{k}") },
            items,
            ..Default::default()
        });
        k
    };
    let mut scenes = Vec::new();
    for (i, s) in doc.scenes.iter().enumerate() {
        let selected = doc.active_scene == Some(i);
        if s.flags & 2 == 0 && !selected {
            scenes.push(None);
            continue;
        }
        let ro = match &s.rendering {
            Some(r) if s.flags & 2 != 0 => map(r),
            _ => model_ro.clone(),
        };
        let k = add(&mut st, from_rendering(&ro));
        scenes.push(Some(k));
        if selected {
            st.selected = Some(k);
            st.changed = st.list[k].items != active_items;
        }
    }
    if st.selected.is_none() {
        let k = add(&mut st, active_items.clone());
        st.selected = Some(k);
        st.changed = false;
    }
    let name = st.selected.and_then(|k| st.list.get(k)).map(|s| s.name.clone()).unwrap_or_default();
    st.active = Some(StyleRec {
        guid: esq_core::meta::new_guid().to_lowercase(),
        name,
        items: active_items,
        ..Default::default()
    });
    (st, scenes)
}
/// The type an item of `ro` is written in when the style did not have it (the SDK's).
fn default_ty(kind: Kind) -> u32 {
    match kind {
        Kind::Bool => 1,
        Kind::Int | Kind::Color => 4,
        Kind::Double => 7,
        Kind::Float => 6,
    }
}

/// The items of a style from its rendering options, watermarks and the items kept as read;
/// `like`: the items the style had when read (their types are kept). Sorted by id.
pub fn join(ro: &Map<String, Value>, wm: Option<&Value>, rest: &[Value], like: &[Item]) -> Vec<Item> {
    let mut out: Vec<Item> = rest.iter().filter_map(item_of).collect();
    let ty_of = |id: u32, kind: Kind| like.iter().find(|i| i.id == id).map_or(default_ty(kind), |i| i.v.ty());
    for (id, key, kind) in ITEMS {
        let Some(v) = ro.get(*key) else { continue };
        let ty = ty_of(*id, *kind);
        let x = match kind {
            Kind::Color => color_bits(v).map(|c| c as f64),
            _ => v.as_f64().or_else(|| v.as_bool().map(|b| b as i32 as f64)),
        };
        let Some(x) = x else { continue };
        out.retain(|i| i.id != *id);
        out.push(Item { id: *id, v: ItemV::of_f64(ty, x) });
    }
    if ro.contains_key("DisplaySectionPlanes") || ro.contains_key("DisplaySectionCuts") {
        let old = out.iter().find(|i| i.id == SECTION_MODE).and_then(|i| i.v.int()).unwrap_or(2);
        let flag =
            |k: &str, bit: i64| ro.get(k).and_then(Value::as_bool).map_or(old & bit, |b| if b { bit } else { 0 });
        let m = flag("DisplaySectionPlanes", 1) | flag("DisplaySectionCuts", 2);
        let ty = ty_of(SECTION_MODE, Kind::Int);
        out.retain(|i| i.id != SECTION_MODE);
        out.push(Item { id: SECTION_MODE, v: ItemV::of_f64(ty, m as f64) });
    }
    if let Some(w) = wm {
        let read = out.iter().find(|i| i.id == WATERMARKS).and_then(|i| match &i.v {
            ItemV::Watermarks(l) => Some(wm_list_json(l)),
            _ => None,
        });
        let empty = w.as_array().is_none_or(Vec::is_empty);
        let same = match &read {
            Some(r) => same_json(r, w),
            None => empty,
        };
        if !same {
            out.retain(|i| i.id != WATERMARKS);
            out.push(Item { id: WATERMARKS, v: ItemV::Watermarks(wm_list_of(w)) });
        }
    }
    out.sort_by_key(|i| i.id);
    out
}

/// A style as an entry of `view_ext["styles"].list` (`id`: its rank, from 1).
pub fn entry(s: &StyleRec, id: i64) -> Value {
    let (ro, wm, rest) = split(&s.items);
    let mut o = Map::new();
    o.insert("id".into(), json!(id));
    o.insert("name".into(), json!(s.name));
    o.insert("description".into(), json!(s.description));
    o.insert("path".into(), json!(s.path));
    o.insert("ro".into(), Value::Object(ro));
    o.insert("wm".into(), wm.as_deref().map_or(json!([]), wm_list_json));
    o.insert("guid".into(), json!(s.guid));
    o.insert("skp_items".into(), Value::Array(rest));
    o.insert("skp_folder".into(), json!(s.folder));
    o.insert("skp_like".into(), Value::Array(s.items.iter().map(|i| json!([i.id, i.v.ty()])).collect()));
    Value::Object(o)
}

/// A style of `view_ext["styles"].list` as SketchUp keeps it.
pub fn rec_of(e: &Value) -> StyleRec {
    let like: Vec<Item> = e["skp_like"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let id = p.get(0)?.as_u64()? as u32;
                    let ty = p.get(1)?.as_u64()? as u32;
                    Some(Item { id, v: ItemV::Num(ty, 0) })
                })
                .collect()
        })
        .unwrap_or_default();
    let ro = e["ro"].as_object().cloned().unwrap_or_default();
    let rest = e["skp_items"].as_array().cloned().unwrap_or_default();
    let s = |k: &str| e[k].as_str().unwrap_or_default().to_string();
    StyleRec {
        id: 0,
        guid: s("guid"),
        path: s("path"),
        folder: s("skp_folder"),
        name: s("name"),
        description: s("description"),
        items: join(&ro, e.get("wm"), &rest, &like),
        watermark_ids: Vec::new(),
    }
}

// ------------------------------------------------------------------------------ style.xml

fn esc(s: &str) -> String {
    super::write::xml::esc(s)
}

fn element_xml(e: &Element, out: &mut String, depth: usize) {
    if depth > 32 {
        return;
    }
    out.push('<');
    out.push_str(&e.name);
    for (k, v) in &e.attrs {
        out.push_str(&format!(" {k}=\"{}\"", esc(v)));
    }
    if e.children.is_empty() && e.text.trim().is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    out.push_str(&esc(e.text.trim()));
    for c in &e.children {
        element_xml(c, out, depth + 1);
    }
    out.push_str(&format!("</{}>", e.name));
}

/// Reads `style.xml` of the folder `styles/<folder>/`; `file(path)` gives a file of the
/// archive (the path as the XML names it, relative to the folder when it starts with `./`).
pub fn from_xml(bytes: &[u8], file: &dyn Fn(&str) -> Option<Arc<Vec<u8>>>) -> super::Result<StyleRec> {
    let root = super::xml::parse(bytes)?;
    let style = if root.name == "style" { Some(&root) } else { root.child("style") };
    let Some(style) = style else { return Err(super::bad("style.xml sans style")) };
    let mut s = StyleRec {
        name: style.attr("name").unwrap_or_default().to_string(),
        description: style.attr("desc").unwrap_or_default().to_string(),
        ..Default::default()
    };
    for item in style.children.iter().filter(|c| c.name == "item") {
        let Some(id) = item.attr("id").and_then(|i| i.trim().parse::<u32>().ok()) else { continue };
        let Some(var) = item.child("variant") else { continue };
        let ty = var.attr("type").and_then(|t| t.trim().parse::<u32>().ok()).unwrap_or(0);
        let text = var.text.trim();
        let v = match ty {
            13 => match (id, var.children.first()) {
                (SKETCHY, Some(n)) if n.name == "nprEdge" => ItemV::Npr(Box::new(npr_xml(n, file))),
                (WATERMARKS, Some(n)) if n.name == "wmlist" => ItemV::Watermarks(wm_xml(n, file)),
                _ => {
                    let mut x = String::new();
                    for c in &var.children {
                        element_xml(c, &mut x, 0);
                    }
                    ItemV::Xml(x)
                }
            },
            1..=5 | 14 | 15 => {
                let n: f64 = text.parse().unwrap_or(0.0);
                ItemV::of_f64(ty, n)
            }
            6 | 7 => ItemV::of_f64(ty, text.parse().unwrap_or(0.0)),
            _ => continue,
        };
        s.items.push(Item { id, v });
    }
    Ok(s)
}

fn npr_xml(n: &Element, file: &dyn Fn(&str) -> Option<Arc<Vec<u8>>>) -> Npr {
    let num = |k: &str| n.attr(k).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0);
    let lengths = n.child("lengths").map_or(Vec::new(), |l| {
        l.children.iter().filter_map(|c| c.text.trim().parse::<f64>().ok()).map(|x| x.max(0.0) as u32).collect()
    });
    let images = n.child("images").map_or(Vec::new(), |l| {
        l.children
            .iter()
            .filter_map(|c| c.attr("path").or(c.attr("file_name")))
            .map(|p| file(p).unwrap_or_default())
            .collect()
    });
    Npr {
        dropout: num("dropoutLength"),
        fade: num("fadeFactor"),
        width: num("width").max(0.0) as u32,
        halo: num("useHalo") != 0.0,
        halo_length: num("haloLength").max(0.0) as u32,
        protos: num("numProtos").max(0.0) as u32,
        lengths,
        images,
    }
}

fn wm_xml(n: &Element, file: &dyn Fn(&str) -> Option<Arc<Vec<u8>>>) -> Vec<Option<Wm>> {
    let mut out = Vec::new();
    let Some(list) = n.child("screenimages") else { return out };
    for s in list.children.iter().filter(|c| c.name == "screenimage") {
        let name = s.attr("name").unwrap_or_default();
        if name == "<MODEL SPACE>" {
            out.push(None);
            continue;
        }
        // SketchUp makes a watermark only of an element that has all of these
        // (each read in turn, the next only if it was there).
        const REQUIRED: [&str; 11] = [
            "info_found",
            "info_filename",
            "info_time",
            "position",
            "tiled",
            "stretched",
            "maintainAR",
            "background",
            "intensForAlpha",
            "alphaScale",
            "scale",
        ];
        if REQUIRED.iter().any(|k| s.attr(k).is_none()) {
            continue;
        }
        let num = |k: &str, d: f64| s.attr(k).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(d);
        let img = s.child("images").and_then(|i| i.children.first());
        let (image, image_name) = match img {
            // From the root of the archive (`watermarks/…`), as SketchUp writes and reads it.
            Some(i) => (
                i.attr("path").filter(|p| !p.starts_with("./")).and_then(file),
                i.attr("file_name").unwrap_or_default().to_string(),
            ),
            None => (None, String::new()),
        };
        let fitting = match s.attr("fittingType") {
            Some(f) => f.trim().parse().unwrap_or(2),
            None if num("tiled", 0.0) != 0.0 => 0,
            None if num("stretched", 0.0) != 0.0 => 1,
            None => 2,
        };
        out.push(Some(Wm {
            id: 0,
            name: name.to_string(),
            found: num("info_found", 0.0) != 0.0,
            file: s.attr("info_filename").unwrap_or_default().to_string(),
            time: num("info_time", 0.0) as i64,
            position: num("position", 4.0).clamp(0.0, 8.0) as u32,
            fitting,
            keep_aspect: s.attr("stretchType").map_or(num("maintainAR", 0.0) != 0.0, |t| t.trim() == "1"),
            background: num("background", 0.0) != 0.0,
            mask: num("intensForAlpha", 0.0) != 0.0,
            alpha: num("alphaScale", 1.0),
            scale: num("scale", 1.0),
            image,
            image_name,
        }));
    }
    out
}

/// The text of a number item, as SketchUp writes it.
fn num_text(v: &ItemV) -> String {
    let g = super::write::xml::g;
    match v {
        ItemV::Num(6, b) => g(f32::from_bits(*b as u32) as f64, 17),
        ItemV::Num(7, b) => g(f64::from_bits(*b), 17),
        ItemV::Num(_, _) => v.int().unwrap_or(0).to_string(),
        _ => String::new(),
    }
}

/// `style.xml` of a style, and the files of its folder (name in the folder or path from the
/// archive's root, bytes). `wm_dir`: where the watermark images go (`watermarks/…`); `taken`:
/// names already used there.
pub fn to_xml(s: &StyleRec, wm_files: &mut Files, taken: &mut std::collections::HashSet<String>) -> (String, Files) {
    use std::fmt::Write;
    let mut files: Vec<(String, Arc<Vec<u8>>)> = Vec::new();
    let mut o = String::new();
    o.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n");
    o.push_str("<styleDocument xmlns=\"http://sketchup.google.com/schemas/sketchup/1.0/style\" xmlns:sty=\"http://sketchup.google.com/schemas/sketchup/1.0/style\">\n");
    let _ = writeln!(
        o,
        "  <sty:style xmlns:t=\"http://sketchup.google.com/schemas/1.0/types\" name=\"{}\" desc=\"{}\">",
        esc(&s.name),
        esc(&s.description)
    );
    let mut next_image = 1;
    for it in &s.items {
        let _ = writeln!(o, "    <sty:item id=\"{}\">", it.id);
        match &it.v {
            ItemV::Npr(n) => {
                o.push_str(
                    "      <t:variant type=\"13\" xmlns:n0=\"http://sketchup.google.com/schemas/sketchup/1.0/npr\">\n",
                );
                let _ = writeln!(
                    o,
                    "        <n0:nprEdge dropoutLength=\"{}\" fadeFactor=\"{}\" width=\"{}\" useHalo=\"{}\" haloLength=\"{}\" numProtos=\"{}\">",
                    super::write::xml::g(n.dropout, 17),
                    super::write::xml::g(n.fade, 17),
                    n.width,
                    n.halo as u8,
                    n.halo_length,
                    n.protos
                );
                o.push_str("          <n0:lengths>\n");
                for l in &n.lengths {
                    let _ = writeln!(o, "            <n0:length>{l}</n0:length>");
                }
                o.push_str("          </n0:lengths>\n          <n0:images>\n");
                for (k, img) in n.images.iter().enumerate() {
                    // Strokes are named by proto and length (`Stroke_<p>_<length>.png`).
                    let per = n.lengths.len().max(1);
                    let len = n.lengths.get(k % per).copied().unwrap_or(32);
                    let fname = format!("Stroke_{}_{len}.png", k / per);
                    let _ = writeln!(
                        o,
                        "            <n0:image id=\"{next_image}\" path=\"./{fname}\" file_name=\"{fname}\"/>"
                    );
                    next_image += 1;
                    files.push((fname, img.clone()));
                }
                o.push_str("          </n0:images>\n        </n0:nprEdge>\n      </t:variant>\n");
            }
            ItemV::Watermarks(list) => {
                o.push_str("      <t:variant type=\"13\" xmlns:n1=\"http://sketchup.google.com/schemas/sketchup/1.0/wmlist\">\n");
                let _ = writeln!(o, "        <n1:wmlist count=\"{}\">", list.len());
                o.push_str("          <n1:screenimages>\n");
                for w in list {
                    let Some(w) = w else {
                        o.push_str("            <n1:screenimage name=\"&lt;MODEL SPACE&gt;\"/>\n");
                        continue;
                    };
                    let _ = write!(
                        o,
                        "            <n1:screenimage name=\"{}\" info_found=\"{}\" info_filename=\"{}\" info_time=\"{}\" position=\"{}\" tiled=\"{}\" stretched=\"{}\" maintainAR=\"{}\" fittingType=\"{}\" stretchType=\"{}\" background=\"{}\" intensForAlpha=\"{}\" alphaScale=\"{}\" scale=\"{}\">",
                        esc(&w.name),
                        w.found as u8,
                        esc(&w.file),
                        w.time,
                        w.position,
                        (w.fitting == 0) as u8,
                        (w.fitting == 1) as u8,
                        w.keep_aspect as u8,
                        w.fitting,
                        w.keep_aspect as u8,
                        w.background as u8,
                        w.mask as u8,
                        super::write::xml::g(w.alpha, 17),
                        super::write::xml::g(w.scale, 17),
                    );
                    if let Some(path) = watermark_path(w, wm_files, taken) {
                        let ext = path.rsplit('.').next().unwrap_or("png");
                        let fname = if w.image_name.is_empty() {
                            format!("{}.{ext}", sanitized(&w.name))
                        } else {
                            w.image_name.clone()
                        };
                        let _ = write!(
                            o,
                            "<n1:images><n1:image id=\"{next_image}\" path=\"{}\" file_name=\"{}\"/></n1:images>",
                            esc(&path),
                            esc(&fname)
                        );
                        next_image += 1;
                    }
                    o.push_str("</n1:screenimage>\n");
                }
                o.push_str("          </n1:screenimages>\n        </n1:wmlist>\n      </t:variant>\n");
            }
            ItemV::Xml(x) => {
                let _ = writeln!(o, "      <t:variant type=\"13\">{x}</t:variant>");
            }
            ItemV::Vec3(t, v) => {
                let _ = writeln!(
                    o,
                    "      <t:variant type=\"{t}\">{} {} {}</t:variant>",
                    super::write::xml::g(v[0], 17),
                    super::write::xml::g(v[1], 17),
                    super::write::xml::g(v[2], 17)
                );
            }
            ItemV::Xform(x) => {
                let t: Vec<String> = x.iter().map(|v| super::write::xml::g(*v, 17)).collect();
                let _ = writeln!(o, "      <t:variant type=\"12\">{}</t:variant>", t.join(" "));
            }
            v @ ItemV::Num(t, _) => {
                let _ = writeln!(o, "      <t:variant type=\"{t}\">{}</t:variant>", num_text(v));
            }
        }
        o.push_str("    </sty:item>\n");
    }
    o.push_str("  </sty:style>\n</styleDocument>\n");
    (o, files)
}

/// A GUID's text (`8-4-4-4-12` hexadecimal digits) as the 16 bytes of the Windows structure
/// (Data1 u32, Data2 u16, Data3 u16, 8 bytes); `None` when it is not one.
pub fn guid_bytes(s: &str) -> Option<[u8; 16]> {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 32 || s.chars().any(|c| !(c.is_ascii_hexdigit() || "-{}".contains(c))) {
        return None;
    }
    let b: Vec<u8> = (0..16).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap_or(0)).collect();
    let mut out = [0u8; 16];
    out[..4].copy_from_slice(&[b[3], b[2], b[1], b[0]]);
    out[4..6].copy_from_slice(&[b[5], b[4]]);
    out[6..8].copy_from_slice(&[b[7], b[6]]);
    out[8..].copy_from_slice(&b[8..]);
    Some(out)
}

/// Where a watermark's image is written in the archive (`watermarks/<name>.<ext>`): once per
/// image, whoever shows it (the model, its styles).
pub fn watermark_path(w: &Wm, files: &mut Files, taken: &mut std::collections::HashSet<String>) -> Option<String> {
    let img = w.image.as_ref().filter(|i| !i.is_empty())?;
    if let Some((p, _)) = files.iter().find(|(_, b)| b == img) {
        return Some(p.clone());
    }
    let ext = super::xml::image_extension(img).unwrap_or("png");
    let stem = sanitized(&w.name);
    let mut path = format!("watermarks/{stem}.{ext}");
    let mut k = 1;
    while !taken.insert(path.to_lowercase()) {
        path = format!("watermarks/{stem}_{k}.{ext}");
        k += 1;
    }
    files.push((path.clone(), img.clone()));
    Some(path)
}

/// A name usable as a file or folder name of the archive (the characters a
/// file name cannot have become `_`).
pub fn sanitized(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    if s.trim().is_empty() { "_".into() } else { s }
}

/// The rendering options of `ro` that styles hold (the model's own, such as fog, left out).
pub fn style_part(ro: &Map<String, Value>) -> Map<String, Value> {
    ro.iter()
        .filter(|(k, _)| ITEMS.iter().any(|(_, n, _)| n == k) || k.starts_with("DisplaySection"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// JSON values equal, numbers by value (`1` and `1.0` are one value).
pub fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_json(p, q)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same_json(v, w)))
        }
        _ => a == b,
    }
}

/// The styles as Esquisse keeps them (`view_ext["styles"]`), with the model's watermarks
/// (`view_ext["watermarks"]`) and what SketchUp's file has besides (`skp`): the active style
/// and whether it was changed, kept as long as the view keeps the settings it was read with
/// (`ro`: the model's rendering options).
pub fn to_view_ext(st: &Styles, ro: &Map<String, Value>) -> Option<(Value, Option<Value>)> {
    if st.list.is_empty() {
        return None;
    }
    let list: Vec<Value> = st.list.iter().enumerate().map(|(i, s)| entry(s, i as i64 + 1)).collect();
    let selected = st.selected.unwrap_or(0) as i64 + 1;
    let mut v = json!({"list": list, "selected": selected});
    let wm = st.watermarks.iter().any(Option::is_some).then(|| wm_list_json(&st.watermarks));
    let mut skp = Map::new();
    skp.insert("changed".into(), json!(st.changed));
    if let Some(a) = &st.active {
        skp.insert("active".into(), entry(a, 0));
    }
    skp.insert("ro0".into(), Value::Object(style_part(ro)));
    skp.insert("wm0".into(), wm.clone().unwrap_or(json!([])));
    v["skp"] = Value::Object(skp);
    Some((v, wm))
}

/// The styles of `view_ext["styles"]` (none when the model has no list: the writer then
/// writes the default style). `ro`, `watermarks`: the view's settings (`view_ext`).
pub fn from_view_ext(styles: Option<&Value>, watermarks: Option<&Value>, ro: Option<&Value>) -> Styles {
    let list: Vec<&Value> = styles.and_then(|s| s["list"].as_array()).map(|a| a.iter().collect()).unwrap_or_default();
    let (Some(st), false) = (styles, list.is_empty()) else {
        // No list (a model made in Esquisse): the default style, unless the view shows
        // watermarks, which a style holds: then a style of the view's settings.
        let wm = watermarks.cloned().unwrap_or(json!([]));
        if wm.as_array().is_none_or(Vec::is_empty) {
            return Styles::default();
        }
        let view_ro = style_part(&ro.and_then(Value::as_object).cloned().unwrap_or_default());
        let d = default_items();
        let items = join(&view_ro, Some(&wm), &d.iter().map(item_json).collect::<Vec<_>>(), &d);
        let s = StyleRec { name: "Style".into(), items, ..Default::default() };
        return Styles {
            list: vec![s.clone()],
            selected: Some(0),
            active: Some(s),
            changed: false,
            watermarks: wm_list_of(&wm),
        };
    };
    let recs: Vec<StyleRec> = list.iter().map(|e| rec_of(e)).collect();
    let sel_id = st["selected"].as_i64();
    let selected = list.iter().position(|e| e["id"].as_i64() == sel_id).or(Some(0));
    let skp = &st["skp"];
    let wm_json = watermarks.cloned().unwrap_or(json!([]));
    let wms: Vec<Option<Wm>> = wm_list_of(&wm_json);
    let sel = selected.and_then(|i| recs.get(i)).cloned().unwrap_or_default();
    let view_ro = style_part(&ro.and_then(Value::as_object).cloned().unwrap_or_default());
    let read_active = skp.get("active").filter(|a| !a.is_null()).map(rec_of);
    // The view keeps the settings it was read with: the active style as SketchUp saved it.
    let untouched = skp.get("ro0").is_some_and(|r| same_json(r, &Value::Object(view_ro.clone())))
        && skp.get("wm0").is_some_and(|w| same_json(w, &wm_json));
    if untouched {
        let changed = skp["changed"].as_bool().unwrap_or(false);
        return Styles { list: recs, selected, active: read_active, changed, watermarks: wms };
    }
    // Changed since: the active style is the selected one with the view's settings.
    let base = read_active.unwrap_or_else(|| sel.clone());
    let (_, _, rest) = split(&base.items);
    let current = StyleRec { items: join(&view_ro, Some(&wm_json), &rest, &base.items), ..base };
    let changed = !same_settings(&sel, &current);
    // The copy keeps the selected style's name (its folder gets « _1 »).
    let active = StyleRec { name: sel.name.clone(), folder: String::new(), guid: sel.guid.clone(), ..current };
    Styles { list: recs, selected, active: Some(active), changed, watermarks: wms }
}

/// Whether two styles have the same settings (rendering options and watermarks).
pub fn same_settings(a: &StyleRec, b: &StyleRec) -> bool {
    let (ra, wa, _) = split(&a.items);
    let (rb, wb, _) = split(&b.items);
    let wj = |w: Option<Vec<Option<Wm>>>| wm_list_json(&w.unwrap_or_default());
    ra.iter().all(|(k, v)| rb.get(k).is_none_or(|w| same_json(v, w))) && same_json(&wj(wa), &wj(wb))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Arc<Vec<u8>> {
        Arc::new(vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10, 1, 2, 3])
    }

    #[test]
    fn styles_go_through_style_xml_and_esquisse_unchanged() {
        let w = Wm {
            name: "Papier".into(),
            found: true,
            file: "/x/papier.png".into(),
            time: 1790406769,
            position: 7,
            fitting: 2,
            keep_aspect: true,
            background: true,
            alpha: 0.5,
            scale: 0.1845,
            image: Some(png()),
            image_name: "papier.png".into(),
            ..Default::default()
        };
        let s = StyleRec {
            guid: "636c5b3b-d783-4510-a79b-6d6169a6da6e".into(),
            name: "[Airbrush]".into(),
            description: "Lignes & traits".into(),
            items: vec![
                Item { id: 1000, v: ItemV::of_f64(4, 1.0) },
                Item {
                    id: 1013,
                    v: ItemV::Npr(Box::new(Npr {
                        lengths: vec![32, 64],
                        images: vec![png(), png()],
                        width: 8,
                        ..Default::default()
                    })),
                },
                Item { id: 1014, v: ItemV::of_f64(5, -16777216.0) },
                Item { id: 2008, v: ItemV::of_f64(7, 0.65) },
                Item { id: 5001, v: ItemV::Watermarks(vec![Some(w.clone()), None]) },
                Item { id: 7013, v: ItemV::of_f64(4, 3.0) },
                Item { id: 8102, v: ItemV::of_f64(6, 10.0) },
            ],
            ..Default::default()
        };
        let mut wm_files = Vec::new();
        let (xml, files) = to_xml(&s, &mut wm_files, &mut Default::default());
        assert_eq!(files.len(), 2);
        assert_eq!(wm_files[0].0, "watermarks/Papier.png");
        let lookup = |p: &str| {
            let p = p.trim_start_matches("./");
            files.iter().chain(wm_files.iter()).find(|(n, _)| n == p).map(|(_, b)| b.clone())
        };
        let back = from_xml(xml.as_bytes(), &lookup).unwrap();
        assert_eq!((back.name.as_str(), back.description.as_str()), ("[Airbrush]", "Lignes & traits"));
        assert_eq!(back.items, s.items, "{xml}");
        // Through the entry of the application's list: the same items.
        let e = entry(&s, 1);
        assert_eq!(e["ro"]["ForegroundColor"], json!([0, 0, 0, 255]));
        assert_eq!(e["ro"]["DisplaySectionCuts"], json!(true));
        assert_eq!(e["wm"][0]["layout"], json!("Positioned"));
        let r = rec_of(&e);
        assert_eq!(r.items, s.items);
        // A setting changed in Esquisse: that item only.
        let mut e2 = e.clone();
        e2["ro"]["EdgeDisplayMode"] = json!(0);
        let r2 = rec_of(&e2);
        assert_eq!(r2.items.iter().find(|i| i.id == 1000).unwrap().v, ItemV::of_f64(4, 0.0));
        assert_eq!(r2.items.len(), s.items.len());
    }
}
