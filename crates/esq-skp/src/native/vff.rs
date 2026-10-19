//! The model of SketchUp 2021+ files: `model.dat`, a tree of tagged records
//! (`docs/skp-format.md`, §2), plus the material files of the archive. Read into a [`Doc`].
//!
//! Each class writes one record whose tag names the class (an edge is 3000 = 0x0bb8), holding
//! the records of its base classes then its own fields, each a record of its own: a value
//! (number, string, persistent id) or more records. Unknown records are skipped, so a newer
//! file still opens with what this reader knows.

use std::collections::{BTreeMap, HashMap};

use esq_core::model::{AttrValue, Attributes};
use serde_json::{Value, json};

use super::attach::{Attach, Leaf};
use super::doc::*;
use super::tlv::{Rec, Records};
use super::xml;
use super::zip::Archive;
use super::{Result, bad};

/// Deepest nesting of records followed (a model nests ~20 deep; arrays of attributes more).
const MAX_DEPTH: usize = 64;

/// The class records of `model.dat` (the first record each writer of SketchUp.exe opens), the
/// SketchUp class it holds, and whether this reader reads it (`docs/skp-format.md`, §2.3).
pub const CLASSES: &[(u16, &str, bool)] = &[
    (0x05dc, "CEntity", true),
    (0x07d0, "CDrawingElement", true),
    (0x09c4, "CVertex", true),
    (0x0bb8, "CEdge", true),
    (0x0dac, "CFace", true),
    (0x0fa0, "CEdgeUse", true),
    (0x1194, "CLoop", true),
    (0x1388, "CEntities", true),
    (0x157c, "CComponentDefinition", true),
    (0x1964, "CComponentInstance", true),
    (0x1b58, "CComponentBehavior", true),
    (0x1d4c, "CGroup", true),
    (0x1f40, "CImage", true),
    (0x2328, "CDib (fichier de l'archive)", true),
    (0x251c, "CThumbnail", true),
    (0x2710, "CFaceTextureCoords", true),
    (0x2904, "CMatchPhoto", true),
    (0x2cec, "CWatermarkManager", true),
    (0x2ee0, "CWatermark", true),
    (0x30d4, "CMaterialManager", true),
    (0x32c8, "CMaterial", true),
    (0x34bc, "CCamera", true),
    (0x36b0, "CAttribute", true),
    (0x36b1, "CAttributeContainer", true),
    (0x36b3, "CAttributeNamed", true),
    (0x38a4, "TypedValue", true),
    (0x3a98, "CLayerManager", true),
    (0x3c8c, "CLayer", true),
    (0x3e80, "CLayerGroup", true),
    (0x4074, "CLineStyleManager", true),
    (0x4076, "CCustomLineStyle", true),
    (0x4268, "CConstructionGeometry", true),
    (0x4269, "CConstructionLine", true),
    (0x426c, "CConstructionPoint", true),
    (0x445c, "CSectionPlane", true),
    (0x4650, "CAxes", true),
    (0x4844, "CPolyline3d", true),
    (0x4a38, "CCurve", true),
    (0x4c2c, "CArcCurve", true),
    (0x4e20, "CFontManager", true),
    (0x5014, "CSkFont", true),
    (0x5208, "ConnectionPoint", true),
    (0x53fc, "CInstancePath", true),
    (0x55f0, "CText", true),
    (0x57e4, "CTextStyle", true),
    (0x59d8, "CDimension", true),
    (0x5bcc, "CDimensionLinear", true),
    (0x5dc0, "CDimensionRadial", true),
    (0x5fb4, "CDimensionStyle", true),
    (0x61a8, "COptionsManager", true),
    (0x639c, "CRelationship", true),
    (0x639f, "CRelationshipMap", true),
    (0x6590, "CShadowInfo", true),
    (0x6784, "CSchemaZipFile", true),
    (0x6978, "CSkpStyleManager", true),
    (0x6b6c, "CSkpStyle", true),
    (0x6d60, "CPageList", true),
    (0x6f54, "CSketchUpPage", true),
    (0x7148, "CViewPage", true),
    (0x733c, "CRenderingOptions", true),
    (0x7530, "CSnap", true),
    (0x7918, "CEnvironmentManager", true),
    (0x7b0c, "CEnvironment", true),
    (0x7d00, "HatchPatternManager", true),
    (0x7d0a, "HatchPattern", true),
    (0x7d64, "CForegroundImageManager", false),
    (0x7d6e, "CForegroundImage", false),
    (0x7dc8, "(section 0x216, ModelingGrid)", false),
    (0x7dfa, "(section 0x217, ?)", false),
    (0x7e2c, "CSpace", true),
    (0x7e5e, "LightElement", false),
    (0x7dfc, "(section 0x217, ?)", false),
];

/// Counts the class records of a `model.dat`, by tag (records that hold records are walked;
/// values that happen to parse as records are a rare false positive).
pub fn class_census(model: &[u8], out: &mut BTreeMap<u16, usize>) {
    let mut stack: Vec<(&[u8], usize)> = vec![(model, 0)];
    while let Some((d, depth)) = stack.pop() {
        for r in Records::new(d, 0) {
            if CLASSES.iter().any(|c| c.0 == r.tag) {
                *out.entry(r.tag).or_default() += 1;
            }
            if depth < 256 && super::tlv::is_records(r.data) {
                stack.push((r.data, depth + 1));
            }
        }
    }
}

/// The children of a record.
fn kids<'a>(r: &Rec<'a>) -> Records<'a> {
    Records::new(r.data, r.at + 6)
}

fn child<'a>(r: &Rec<'a>, tag: u16) -> Option<Rec<'a>> {
    kids(r).find(|c| c.tag == tag)
}

fn f64_of(r: &Rec) -> Option<f64> {
    Some(f64::from_le_bytes(r.data.get(..8)?.try_into().ok()?))
}

fn f64s<const N: usize>(r: &Rec) -> Option<[f64; N]> {
    if r.data.len() < N * 8 {
        return None;
    }
    let mut out = [0.0; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = f64::from_le_bytes(r.data[i * 8..i * 8 + 8].try_into().ok()?);
    }
    Some(out)
}

fn u32_of(r: &Rec) -> Option<u32> {
    match r.data.len() {
        0 => None,
        1 => Some(r.data[0] as u32),
        2 => Some(u16::from_le_bytes([r.data[0], r.data[1]]) as u32),
        _ => Some(u32::from_le_bytes(r.data.get(..4)?.try_into().ok()?)),
    }
}

fn bool_of(r: &Rec) -> bool {
    r.data.first().is_some_and(|&b| b != 0)
}

fn str_of(r: &Rec) -> String {
    String::from_utf8_lossy(r.data).into_owned()
}

/// A persistent id: 1 to 8 bytes, little-endian.
fn id_of(r: &Rec) -> Option<Id> {
    if r.data.is_empty() || r.data.len() > 8 {
        return None;
    }
    let mut b = [0u8; 8];
    b[..r.data.len()].copy_from_slice(r.data);
    Some(u64::from_le_bytes(b))
}

fn guid_of(r: &Rec) -> String {
    let d = r.data;
    if d.len() != 16 {
        return String::new();
    }
    // Stored as the Windows GUID structure.
    let a = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
    let b = u16::from_le_bytes([d[4], d[5]]);
    let c = u16::from_le_bytes([d[6], d[7]]);
    format!(
        "{a:08x}-{b:04x}-{c:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        d[8], d[9], d[10], d[11], d[12], d[13], d[14], d[15]
    )
}

fn color_of(r: &Rec) -> Option<[u8; 4]> {
    let d = r.data.get(..4)?;
    Some([d[0], d[1], d[2], d[3]])
}

struct Reader<'a> {
    archive: Option<&'a Archive<'a>>,
    doc: Doc,
    /// `material.xml` files already read, by folder (`None`: absent or unreadable), and what a
    /// copy of one costs.
    parsed_materials: HashMap<String, (Option<Material>, u64)>,
    /// What the reading of this file may make: many material records may name one folder,
    /// and each gets a copy of its material.
    budget: super::Budget,
    /// Points of annotations attached to entities, placed once the model is read.
    attach: Vec<(Id, u8, Attach<Id>)>,
    /// Guide lines as SketchUp keeps them (point, direction), by id.
    guide_lines: HashMap<Id, ([f64; 3], [f64; 3])>,
}

/// Approximate bytes of a copy of a material (its images are shared).
fn material_cost(m: &Material) -> u64 {
    let t = |t: &Option<Texture>| {
        t.as_ref().map_or(0, |t| std::mem::size_of::<Texture>() as u64 + t.file_name.len() as u64)
    };
    let p = &m.pbr;
    std::mem::size_of::<Material>() as u64
        + m.name.len() as u64
        + super::legacy::attrs_cost(&m.attrs)
        + t(&m.texture)
        + t(&p.metallic_texture)
        + t(&p.roughness_texture)
        + t(&p.normal_texture)
        + t(&p.ao_texture)
}

/// Reads the model of a 2021+ file.
pub fn read(model: &[u8], archive: Option<&Archive>, version: (u32, u32, u32), opts: &super::Options) -> Result<Doc> {
    SDK_QUIRKS.set(opts.sdk_quirks);
    let res = read_inner(model, archive, version);
    SDK_QUIRKS.set(false);
    res.map(|mut d| {
        d.sdk_quirks = opts.sdk_quirks;
        d
    })
}

fn read_inner(model: &[u8], archive: Option<&Archive>, version: (u32, u32, u32)) -> Result<Doc> {
    let top = Records::new(model, 0).next().ok_or_else(|| bad("model.dat vide"))?;
    if top.tag != 0x1f4 {
        return Err(bad("model.dat : enregistrement racine inattendu"));
    }
    let budget = super::Budget::for_input(model.len() + archive.map_or(0, |a| a.len()));
    let mut r = Reader {
        archive,
        doc: Doc { version, ..Default::default() },
        parsed_materials: HashMap::new(),
        budget,
        attach: Vec::new(),
        guide_lines: HashMap::new(),
    };
    if let Some(a) = archive {
        r.doc.guid = guid(a);
    }
    let mut root = None;
    for c in kids(&top) {
        match c.tag {
            0x1f6 => root = Some(c),
            0x1f7 => r.materials(&c)?,
            0x1f8 => r.layers(&c),
            0x1f9 => r.definitions(&c, 0)?,
            0x1fa => r.doc.camera = child(&c, 0x34bc).and_then(|x| camera(&x)),
            0x1fc => r.axes(&c),
            0x1fb => {
                if let Some(ro) = child(&c, 0x733c) {
                    r.doc.rendering = rendering_options(&ro);
                    r.doc.ro_hatch = r.ro_hatch(&ro)?;
                }
            }
            0x211 => r.hatch_patterns(&c)?,
            0x20c => r.doc.model_flags.get_or_insert_with(Default::default).v20c = bool_of(&c),
            0x20d => r.doc.model_flags.get_or_insert_with(Default::default).v20d = u32_of(&c).unwrap_or(1),
            0x20f => r.doc.model_flags.get_or_insert_with(Default::default).v20f = u32_of(&c).unwrap_or(0),
            0x200 => r.options(&c, 0)?,
            0x204 => {
                if let Some(s) = child(&c, 0x6590) {
                    r.doc.shadow = shadow_info(&s);
                }
            }
            0x208 => r.line_styles(&c),
            0x207 => r.scenes(&c)?,
            0x206 => r.styles(&c)?,
            0x1fd => r.fonts(&c)?,
            0x1fe => r.doc.text_style = child(&c, 0x57e4).map(|s| text_style(&s)),
            0x1ff => r.doc.dim_style = child(&c, 0x5fb4).map(|s| dim_style(&s)),
            0x203 => r.watermark_manager(&c)?,
            0x205 => r.classifications(&c),
            0x201 => r.match_photos(&c)?,
            0x210 => r.environments(&c)?,
            0x202 => r.doc.active_photo = id_of(&c).filter(|&i| i != 0),
            0x209 => {
                // Model properties: a dictionary "ModelProperties" with the name and description.
                let (props, _) = attribute_container(&c, 1)?;
                if let Some(p) = props.get("ModelProperties") {
                    let text = |k: &str| match p.get(k) {
                        Some(AttrValue::Text(s)) => s.clone(),
                        _ => String::new(),
                    };
                    r.doc.name = text("Name");
                    r.doc.description = text("Description");
                }
            }
            _ => {}
        }
    }
    // The root collection last: its definitions are known by then.
    if let Some(c) = root
        && let Some(e) = child(&c, 0x1388)
    {
        let (ents, el) = r.entities(&e, 0)?;
        r.doc.root = ents;
        r.doc.attrs = el.attrs;
    }
    place_annotations(&mut r.doc, &r.attach, &r.guide_lines);
    Ok(r.doc)
}

/// The model's GUID: `meta/meta.dat` holds the same records as `model.dat`; 0x64 the file's
/// description, 0x66 the GUID.
pub(crate) fn guid(archive: &Archive) -> String {
    if let Ok(meta) = archive.read_name("meta/meta.dat")
        && let Some(top) = Records::new(&meta, 0).next()
        && let Some(g) = child(&top, 0x66)
    {
        return guid_of(&g);
    }
    String::new()
}

/// The entity part (record 0x5dc): id, dictionaries, texture placement of a face.
fn entity(r: &Rec, depth: usize) -> Result<El> {
    let mut el = El::default();
    for c in kids(r) {
        match c.tag {
            0x5de => el.id = id_of(&c).unwrap_or(0),
            0x5dd => (el.attrs, el.tex) = attribute_container(&c, depth + 1)?,
            _ => {}
        }
    }
    Ok(el)
}

/// The drawing element part (record 0x7d0): entity, material, tag, flags.
fn drawing_element(r: &Rec, depth: usize) -> Result<El> {
    let mut el = El::default();
    for c in kids(r) {
        match c.tag {
            0x5dc => {
                let e = entity(&c, depth + 1)?;
                (el.id, el.attrs, el.tex) = (e.id, e.attrs, e.tex);
            }
            0x7d1 => el.material = id_of(&c),
            0x7d2 => el.layer = id_of(&c),
            0x7d3 => el.flags = u32_of(&c).unwrap_or(0),
            _ => {}
        }
    }
    Ok(el)
}

thread_local! {
    /// See [`super::Options::sdk_quirks`].
    static SDK_QUIRKS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// One side of a face's texture placement (0x2711 front, 0x2712 back → 0x2713): flags (bit 0
/// positioned, bit 1 projected), the 3×3 matrix, the projection direction.
fn tex_side(r: &Rec) -> Option<TexCoords> {
    let s = child(r, 0x2713)?;
    let mut t = TexCoords::default();
    let mut flags = 0;
    for c in kids(&s) {
        match c.tag {
            0x2714 => flags = u32_of(&c).unwrap_or(0),
            0x2715 => t.matrix = f64s::<9>(&c)?,
            0x2716 => t.projection = f64s::<3>(&c),
            _ => {}
        }
    }
    t.positioned = flags & 1 != 0;
    t.projected = flags & 2 != 0;
    Some(t)
}

/// Attribute dictionaries of an entity (0x5dd → 0x36b1 → 0x36b2 → items: dictionaries 0x36b3,
/// a face's texture placement 0x2710…).
fn attribute_container(r: &Rec, depth: usize) -> Result<(Attributes, [Option<Box<TexCoords>>; 2])> {
    let mut out = Attributes::new();
    let mut tex = [None, None];
    if depth > MAX_DEPTH {
        return Err(bad("dictionnaires trop imbriqués"));
    }
    let items: Vec<Rec> = kids(r)
        .filter(|c| c.tag == 0x36b1)
        .flat_map(|c| kids(&c).filter(|x| x.tag == 0x36b2).collect::<Vec<_>>())
        .flat_map(|l| kids(&l).collect::<Vec<_>>())
        .collect();
    let dicts = items.iter().filter(|x| x.tag == 0x36b3).count();
    let mut seen_tex = false;
    for (i, d) in items.iter().enumerate() {
        // A container may hold more than one placement: SketchUp uses the first.
        if d.tag == 0x2710 && !seen_tex {
            seen_tex = true;
            for c in kids(d) {
                match c.tag {
                    0x2711 => tex[0] = tex_side(&c).map(Box::new),
                    0x2712 => tex[1] = tex_side(&c).map(Box::new),
                    _ => {}
                }
            }
        }
        if d.tag != 0x36b3 {
            continue;
        }
        // `SUEntityGetAttributeDictionaries(n)` looks at the first n items of the container,
        // dictionaries or not, and stores each dictionary at its item's index: a dictionary
        // that follows a texture placement is lost (and one may leave a null slot).
        if SDK_QUIRKS.get() {
            let got = items.iter().take(dicts).filter(|x| x.tag == 0x36b3).count();
            if i >= got {
                continue;
            }
        }
        dictionary(d, depth + 1, &mut out)?;
    }
    Ok((out, tex))
}

fn dictionary(d: &Rec, depth: usize, out: &mut Attributes) -> Result<()> {
    let mut name = String::new();
    let mut values = BTreeMap::new();
    // The dictionaries nested in this one (0x36b0 → 0x5dc → 0x5dd, an attribute container):
    // the SDK does not list them (`sdk_quirks`).
    let mut nested = Attributes::new();
    for c in kids(d) {
        match c.tag {
            0x36b0 if !SDK_QUIRKS.get() => {
                if let Some(e) = child(&c, 0x5dc) {
                    nested = entity(&e, depth + 1)?.attrs;
                }
            }
            0x36b4 => name = str_of(&c).replace(super::doc::NEST, ""),
            0x36b5 => {
                let mut key = None;
                for kv in kids(&c) {
                    match kv.tag {
                        0x36b6 => key = Some(str_of(&kv)),
                        0x38a4 => {
                            // An empty value (nil) is not kept: the SDK does not list its key.
                            if let (Some(k), Some(v)) = (key.take(), typed_value(&kv, depth + 1)?)
                                && v != json!({"t": "n"})
                            {
                                values.insert(k, attr_value(v));
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    // An empty dictionary is not kept (the SDK reader skips dictionaries without keys), but
    // for the ones it holds.
    if !values.is_empty() || !nested.is_empty() {
        out.entry(name.clone()).or_default().extend(values);
    }
    for (k, v) in nested {
        out.entry(format!("{name}{}{k}", super::doc::NEST)).or_default().extend(v);
    }
    Ok(())
}

/// A typed value (record 0x38a4), in the Ruby bridge's tagged JSON (`{"t": "i", "v": 3}`…).
pub(crate) fn typed_value(r: &Rec, depth: usize) -> Result<Option<Value>> {
    if depth > MAX_DEPTH {
        return Err(bad("valeurs trop imbriquées"));
    }
    let Some(c) = kids(r).next() else { return Ok(Some(json!({"t": "n"}))) };
    let d = c.data;
    let int = |n: usize| -> Option<i64> {
        let b = d.get(..n)?;
        Some(match n {
            1 => b[0] as i64,
            2 => i16::from_le_bytes([b[0], b[1]]) as i64,
            _ => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64,
        })
    };
    Ok(Some(match c.tag {
        0x38a5 => json!({"t": "i", "v": int(1)}),
        0x38a6 => json!({"t": "i", "v": int(2)}),
        0x38a7 => json!({"t": "i", "v": int(4)}),
        0x38a8 => json!({"t": "f", "v": d.get(..4).map(|b| f32::from_le_bytes(b.try_into().unwrap()) as f64)}),
        0x38a9 => {
            let x = f64_of(&c).unwrap_or(0.0);
            if x.is_finite() {
                json!({"t": "f", "v": x})
            } else {
                json!({"t": "nf", "v": if x.is_nan() { "nan" } else if x > 0.0 { "inf" } else { "-inf" }})
            }
        }
        0x38aa => json!({"t": "b", "v": bool_of(&c)}),
        0x38ab => json!({"t": "c", "v": color_of(&c).unwrap_or([0, 0, 0, 255])}),
        0x38ac => json!({"t": "time", "v": [int(4).unwrap_or(0), 0]}),
        0x38ad => json!({"t": "s", "v": str_of(&c)}),
        0x38ae => {
            let mut items = Vec::new();
            for it in kids(&c).filter(|x| x.tag == 0x38a4) {
                if let Some(v) = typed_value(&it, depth + 1)? {
                    items.push(v);
                }
            }
            json!({"t": "a", "v": items})
        }
        // Lengths, angles… (types 12 to 16): doubles.
        0x38af..=0x38b3 => json!({"t": "f", "v": f64_of(&c).unwrap_or(0.0)}),
        // A point: a vector for the SDK, which has no type for it.
        0x38b4 => json!({"t": if SDK_QUIRKS.get() { "vec" } else { "pt" }, "v": f64s::<3>(&c).unwrap_or_default()}),
        0x38b5 | 0x38b6 => json!({"t": "vec", "v": f64s::<3>(&c).unwrap_or_default()}),
        _ => return Ok(None),
    }))
}

/// A tagged value as Esquisse keeps it in a dictionary.
pub(crate) fn attr_value(j: Value) -> AttrValue {
    match j.get("t").and_then(Value::as_str) {
        Some("s") => AttrValue::Text(j.get("v").and_then(Value::as_str).unwrap_or_default().to_string()),
        Some("f") => AttrValue::Number(j.get("v").and_then(Value::as_f64).unwrap_or_default()),
        Some("b") => AttrValue::Bool(j.get("v").and_then(Value::as_bool).unwrap_or_default()),
        _ => AttrValue::Json(j),
    }
}

/// A tagged value as plain JSON, as the SDK reader keeps options (`model_info::typed_plain`).
pub(crate) fn plain(t: &Value) -> Value {
    let v = t.get("v").cloned().unwrap_or(Value::Null);
    match t.get("t").and_then(Value::as_str) {
        Some("a") => Value::Array(v.as_array().map(|x| x.iter().map(plain).collect()).unwrap_or_default()),
        Some("time") => v.get(0).cloned().unwrap_or(Value::Null),
        Some("n") => Value::Null,
        _ => v,
    }
}

/// How a rendering option is stored.
#[derive(Clone, Copy)]
pub(crate) enum Ro {
    Int,
    Bool,
    Double,
    Float,
    Color,
}

/// Rendering options: the name the SDK gives each (`SURenderingOptionsGetKeys`), the record
/// that holds it in 0x733c, and its type. Read in SketchUp.exe: the accessor table of
/// `CRenderingOptions` (name → field) and its writer (field → record).
pub(crate) const RENDERING: &[(&str, u16, Ro)] = &[
    ("RenderMode", 0x733d, Ro::Int),
    ("EdgeDisplayMode", 0x7341, Ro::Int),
    ("EdgeColorMode", 0x7348, Ro::Int),
    ("EdgeType", 0x7342, Ro::Int),
    ("TransparencySort", 0x7377, Ro::Int),
    ("DrawBackEdges", 0x7356, Ro::Bool),
    ("LineExtension", 0x734e, Ro::Int),
    ("SilhouetteWidth", 0x7350, Ro::Int),
    ("DepthQueWidth", 0x7352, Ro::Int),
    ("LineEndWidth", 0x7354, Ro::Int),
    ("FogStartDist", 0x7362, Ro::Double),
    ("FogEndDist", 0x7363, Ro::Double),
    ("GroundTransparency", 0x736b, Ro::Int),
    ("SectionCutWidth", 0x7374, Ro::Int),
    ("SectionCutFilled", 0x7376, Ro::Bool),
    ("DefaultSectionFillStyleEnabled", 0x738c, Ro::Bool),
    ("ModelTransparency", 0x733e, Ro::Bool),
    ("MaterialTransparency", 0x733f, Ro::Bool),
    ("Texture", 0x7340, Ro::Bool),
    ("JitterEdges", 0x734b, Ro::Bool),
    ("DisplayInstanceAxes", 0x7349, Ro::Bool),
    ("DisplayColorByLayer", 0x7347, Ro::Bool),
    ("DrawHorizon", 0x7368, Ro::Bool),
    ("DrawGround", 0x7369, Ro::Bool),
    ("DrawUnderground", 0x736a, Ro::Bool),
    ("ExtendLines", 0x734d, Ro::Bool),
    ("DrawSilhouettes", 0x734f, Ro::Bool),
    ("DrawDepthQue", 0x7351, Ro::Bool),
    ("DrawLineEnds", 0x7353, Ro::Bool),
    ("DrawProfilesOnly", 0x7355, Ro::Bool),
    ("DrawHiddenGeometry", 0x7380, Ro::Bool),
    ("DrawHiddenObjects", 0x7381, Ro::Bool),
    ("InactiveHidden", 0x736e, Ro::Bool),
    ("InactiveFade", 0x736c, Ro::Double),
    ("InstanceHidden", 0x736f, Ro::Bool),
    ("InstanceFade", 0x736d, Ro::Double),
    ("DisplayWatermarks", 0x735e, Ro::Bool),
    ("DisplayFog", 0x735f, Ro::Bool),
    ("FogUseBkColor", 0x7361, Ro::Bool),
    ("HideConstructionGeometry", 0x7346, Ro::Bool),
    ("DisplayDims", 0x7345, Ro::Bool),
    ("DisplayText", 0x7344, Ro::Bool),
    ("DisplaySketchAxes", 0x7343, Ro::Bool),
    ("BackgroundColor", 0x7357, Ro::Color),
    ("ForegroundColor", 0x7358, Ro::Color),
    ("GroundColor", 0x7367, Ro::Color),
    ("SkyColor", 0x7365, Ro::Color),
    ("HorizonColor", 0x7366, Ro::Color),
    ("HighlightColor", 0x7359, Ro::Color),
    ("ConstructionColor", 0x735b, Ro::Color),
    ("FaceFrontColor", 0x735c, Ro::Color),
    ("FaceBackColor", 0x735d, Ro::Color),
    ("FogColor", 0x7360, Ro::Color),
    ("SectionActiveColor", 0x7370, Ro::Color),
    ("SectionInactiveColor", 0x7371, Ro::Color),
    ("SectionDefaultCutColor", 0x7372, Ro::Color),
    ("SectionDefaultFillColor", 0x7373, Ro::Color),
    ("LockedColor", 0x735a, Ro::Color),
    ("AmbientOcclusion", 0x7383, Ro::Bool),
    ("AmbientOcclusionColorEnabled", 0x738b, Ro::Bool),
    ("AmbientOcclusionDistance", 0x7385, Ro::Float),
    ("AmbientOcclusionIntensity", 0x7386, Ro::Float),
    ("AmbientOcclusionMultiplier", 0x7389, Ro::Float),
    ("AmbientOcclusionColor", 0x738a, Ro::Color),
];

/// The rendering options of a record 0x733c, by the SDK's names.
fn rendering_options(r: &Rec) -> BTreeMap<String, Value> {
    let recs: BTreeMap<u16, Rec> = kids(r).map(|c| (c.tag, c)).collect();
    let mut vals = BTreeMap::new();
    for &(_, tag, kind) in RENDERING {
        let Some(c) = recs.get(&tag) else { continue };
        let v = match kind {
            Ro::Int => json!(u32_of(c).unwrap_or(0) as i32),
            Ro::Bool => json!(bool_of(c)),
            Ro::Double => json!(f64_of(c).unwrap_or(0.0)),
            Ro::Float => json!(c.data.get(..4).map_or(0.0, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64)),
            Ro::Color => json!(color_of(c).unwrap_or([0, 0, 0, 255])),
        };
        vals.insert(tag, v);
    }
    if let Some(sp) = recs.get(&0x7375).and_then(u32_of) {
        vals.insert(0x7375, json!(sp));
    }
    rendering_from(&vals)
}

/// Rendering options by the SDK's names, from their values by record tag (the tags of 2021+
/// files; the reader of earlier files gives its fields the same tags).
pub(crate) fn rendering_from(vals: &BTreeMap<u16, Value>) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for &(name, tag, _) in RENDERING {
        if let Some(v) = vals.get(&tag) {
            out.insert(name.to_string(), v.clone());
        }
    }
    // Options the SDK derives from others: hidden geometry or objects; section planes and
    // cuts (two bits of one number); a colour left unset (all zero) takes its default.
    let b = |k: &str| out.get(k).and_then(Value::as_bool).unwrap_or(false);
    let hidden = b("DrawHiddenGeometry") || b("DrawHiddenObjects");
    out.insert("DrawHidden".into(), json!(hidden));
    if let Some(sp) = vals.get(&0x7375).and_then(Value::as_u64) {
        out.insert("DisplaySectionPlanes".into(), json!(sp & 1 != 0));
        out.insert("DisplaySectionCuts".into(), json!(sp & 2 != 0));
    }
    // Options of the object that the file does not hold: their values after loading.
    for (k, v) in [
        ("SectionCutDrawEdges", json!(true)),
        ("ShowViewName", json!(true)),
        ("BandColor", json!([0, 0, 0, 127])),
        ("DefaultSectionFillStyleEnabled", json!(false)),
        // Files older than the ambient occlusion (2025).
        ("AmbientOcclusion", json!(false)),
        ("AmbientOcclusionDistance", json!(10.0)),
        ("AmbientOcclusionIntensity", json!(0.0)),
        ("AmbientOcclusionMultiplier", json!(1.0)),
        ("AmbientOcclusionColorEnabled", json!(false)),
        ("AmbientOcclusionColor", json!([0, 0, 0, 255])),
    ] {
        out.entry(k.to_string()).or_insert(v);
    }
    let unset = |v: Option<&Value>| v.is_none_or(|x| x == &json!([0, 0, 0, 0]));
    if unset(out.get("BackgroundColor")) {
        out.insert("BackgroundColor".into(), json!([255, 255, 255, 255]));
    }
    if unset(out.get("ForegroundColor")) {
        out.insert("ForegroundColor".into(), json!([0, 0, 0, 255]));
    }
    if unset(out.get("FogColor")) {
        let bg = out.get("BackgroundColor").cloned().unwrap_or(json!([255, 255, 255, 255]));
        out.insert("FogColor".into(), bg);
    }
    out
}

/// Shadow settings (record 0x6590), by the SDK's key names.
fn shadow_info(r: &Rec) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for c in kids(r) {
        let (k, v) = match c.tag {
            0x6591 => ("ShadowTime", json!(u32_of(&c).unwrap_or(0) as i64)),
            0x6592 => ("DaylightSavings", json!(bool_of(&c))),
            0x6593 => ("City", json!(str_of(&c))),
            0x6594 => ("Country", json!(str_of(&c))),
            0x6595 => ("Longitude", json!(f64_of(&c).unwrap_or(0.0))),
            0x6596 => ("Latitude", json!(f64_of(&c).unwrap_or(0.0))),
            0x6597 => ("TZOffset", json!(f64_of(&c).unwrap_or(0.0))),
            0x6598 => {
                let n = f64s::<3>(&c).unwrap_or([0.0, 1.0, 0.0]);
                ("NorthAngle", json!(north_angle(n)))
            }
            0x6599 => ("DisplayShadows", json!(bool_of(&c))),
            0x659a => ("DisplayNorth", json!(bool_of(&c))),
            0x659b => ("DisplayOnAllFaces", json!(bool_of(&c))),
            0x659c => ("DisplayOnGroundPlane", json!(bool_of(&c))),
            0x659d => ("EdgesCastShadows", json!(bool_of(&c))),
            0x659e => ("Light", json!(u32_of(&c).unwrap_or(0) as i32)),
            0x659f => ("Dark", json!(u32_of(&c).unwrap_or(0) as i32)),
            0x65a0 => ("UseSunForAllShading", json!(bool_of(&c))),
            _ => continue,
        };
        out.insert(k.to_string(), v);
    }
    out
}

/// The angle of true north from the green axis, degrees clockwise, from the north vector.
pub(crate) fn north_angle(n: [f64; 3]) -> f64 {
    let a = n[0].atan2(n[1]).to_degrees();
    if a < 0.0 { a + 360.0 } else { a }
}

/// A camera (record 0x34bc).
fn camera(r: &Rec) -> Option<Camera> {
    let mut c = Camera { perspective: true, fov: 35.0, ..Default::default() };
    for x in kids(r) {
        match x.tag {
            0x34bd => c.eye = f64s::<3>(&x)?,
            0x34be => c.target = f64s::<3>(&x)?,
            0x34bf => c.up = f64s::<3>(&x)?,
            0x34c2 => c.perspective = bool_of(&x),
            0x34c4 => c.fov = f64_of(&x)?,
            _ => {}
        }
    }
    c.target = super::build::settled_target(c.eye, c.target);
    Some(c)
}

impl Reader<'_> {
    /// Options providers (0x200 → 0x61a8 → 0x61a9 → providers 0x61aa).
    fn options(&mut self, r: &Rec, depth: usize) -> Result<()> {
        if depth > 4 {
            return Ok(());
        }
        for c in kids(r) {
            match c.tag {
                0x61a8 | 0x61a9 => self.options(&c, depth + 1)?,
                0x61aa => {
                    let mut name = String::new();
                    let mut values = BTreeMap::new();
                    for x in kids(&c) {
                        match x.tag {
                            0x61ab => name = str_of(&x),
                            0x61ac => {
                                let mut key = None;
                                for kv in kids(&x) {
                                    match kv.tag {
                                        0x61ad => key = Some(str_of(&kv)),
                                        0x38a4 => {
                                            if let (Some(k), Some(v)) = (key.take(), typed_value(&kv, 1)?) {
                                                values.insert(k, plain(&v));
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    self.doc.options.entry(name).or_default().extend(values);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn axes(&mut self, r: &Rec) {
        let Some(a) = child(r, 0x4650) else { return };
        let (mut o, mut x, mut y, mut z) = ([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        for c in kids(&a) {
            match c.tag {
                0x4651 => o = f64s::<3>(&c).unwrap_or(o),
                0x4652 => x = f64s::<3>(&c).unwrap_or(x),
                0x4653 => y = f64s::<3>(&c).unwrap_or(y),
                0x4654 => z = f64s::<3>(&c).unwrap_or(z),
                _ => {}
            }
        }
        self.doc.axes =
            Some([x[0], x[1], x[2], 0.0, y[0], y[1], y[2], 0.0, z[0], z[1], z[2], 0.0, o[0], o[1], o[2], 1.0]);
    }

    fn line_styles(&mut self, r: &Rec) {
        for m in kids(r).filter(|x| x.tag == 0x4074) {
            for l in kids(&m).filter(|x| x.tag == 0x4075) {
                for s in kids(&l).filter(|x| x.tag == 0x4076) {
                    let mut id = 0;
                    let mut name = String::new();
                    for c in kids(&s) {
                        match c.tag {
                            0x5dc => id = child(&c, 0x5de).and_then(|x| id_of(&x)).unwrap_or(0),
                            0x4077 => name = str_of(&c),
                            _ => {}
                        }
                    }
                    self.doc.line_styles.insert(id, name);
                }
            }
        }
    }

    /// A material record (13000 = 0x32c8) and its file `material.xml`.
    fn material(&mut self, r: &Rec) -> Result<Material> {
        let mut m = Material { opacity: 1.0, ..Default::default() };
        let mut folder = String::new();
        for c in kids(r) {
            match c.tag {
                0x5dc => m.id = entity(&c, 1)?.id,
                0x32cc => folder = str_of(&c),
                0x32ca => m.owned = bool_of(&c),
                0x32cf => m.hatch = id_of(&c).filter(|&i| i != 0),
                _ => {}
            }
        }
        // The material's files are in `materials/<folder>/`, read once per folder: many records
        // may name the same one.
        let folder = format!("materials/{}", folder.trim_matches('/'));
        if !self.parsed_materials.contains_key(&folder) {
            let mut p = None;
            if let Some(a) = self.archive
                && let Ok(bytes) = a.read_name(&format!("{folder}/material.xml"))
            {
                let mut x = Material { opacity: 1.0, ..Default::default() };
                xml::material(&bytes, &folder, a, &mut x)?;
                // Its thumbnail, as SketchUp writes it beside (no reference to it).
                for ext in ["jpg", "png"] {
                    if let Ok(t) = a.read_name(&format!("{folder}/thumbnail.{ext}")) {
                        self.budget.charge(t.len() as u64)?;
                        x.thumbnail = Some(t);
                        break;
                    }
                }
                p = Some(x);
            }
            let cost = p.as_ref().map_or(0, material_cost);
            self.parsed_materials.insert(folder.clone(), (p, cost));
        }
        // Each record gets its copy: counted, so that a few bytes of records cannot copy a
        // large material many times over.
        if let Some((Some(p), cost)) = self.parsed_materials.get(&folder) {
            self.budget.charge(*cost)?;
            let (id, owned, hatch) = (m.id, m.owned, m.hatch);
            m = Material { id, owned, hatch, ..p.clone() };
        }
        Ok(m)
    }

    fn materials(&mut self, r: &Rec) -> Result<()> {
        for mgr in kids(r).filter(|x| x.tag == 0x30d4) {
            for list in kids(&mgr).filter(|x| x.tag == 0x30d5) {
                for m in kids(&list).filter(|x| x.tag == 0x32c8) {
                    let mat = self.material(&m)?;
                    self.doc.materials.push(mat);
                }
            }
        }
        Ok(())
    }

    fn layers(&mut self, r: &Rec) {
        let Some(mgr) = child(r, 0x3a98) else { return };
        for c in kids(&mgr) {
            match c.tag {
                0x3a99 => {
                    for l in kids(&c).filter(|x| x.tag == 0x3c8c) {
                        let mut layer = Layer { visible: true, ..Default::default() };
                        for f in kids(&l) {
                            match f.tag {
                                0x5dc => {
                                    if let Ok(e) = entity(&f, 1) {
                                        (layer.id, layer.attrs) = (e.id, e.attrs);
                                    }
                                }
                                0x3c8d => layer.name = str_of(&f),
                                0x3c8e => layer.visible = !bool_of(&f),
                                0x3c8f => {
                                    if let Some(m) = child(&f, 0x32c8)
                                        && let Ok(mat) = self.material(&m)
                                    {
                                        layer.color = Some(mat.color);
                                    }
                                }
                                0x3c90 => layer.page_behavior = u32_of(&f).unwrap_or(0),
                                0x3c91 => layer.line_style = id_of(&f),
                                _ => {}
                            }
                        }
                        self.doc.layers.push(layer);
                    }
                }
                0x3a9a => self.doc.active_layer = id_of(&c),
                0x3a9b => {
                    if let Some(g) = child(&c, 0x3e80) {
                        self.doc.folders = folders(&g, 0);
                    }
                }
                _ => {}
            }
        }
    }

    /// The definition list (0x1f9 → 0x1770 → 0x1771 → definitions 0x157c).
    fn definitions(&mut self, r: &Rec, depth: usize) -> Result<()> {
        if depth > 8 {
            return Ok(());
        }
        for c in kids(r) {
            match c.tag {
                0x1770 | 0x1771 => self.definitions(&c, depth + 1)?,
                0x157c => {
                    let d = self.definition(&c)?;
                    self.doc.defs.push(d);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn definition(&mut self, r: &Rec) -> Result<Def> {
        let mut d = Def::default();
        for c in kids(r) {
            match c.tag {
                0x1388 => {
                    let (ents, el) = self.entities(&c, 1)?;
                    (d.ents, d.id, d.attrs) = (ents, el.id, el.attrs);
                }
                0x157d => d.guid = guid_of(&c),
                0x1580 => d.path = str_of(&c),
                0x1581 => d.time = u32_of(&c).unwrap_or(0),
                0x157e => d.name = str_of(&c),
                0x157f => d.description = str_of(&c),
                // What the definition is: 0 component, 1 group, 2 image.
                0x1583 => {
                    let k = u32_of(&c).unwrap_or(0);
                    (d.is_group, d.is_image) = (k == 1, k == 2);
                }
                0x1b58 => d.behavior = behavior(&c),
                // Its thumbnail (0x251c): 0x251d the camera, 0x251e → 0x2328 the image's path.
                0x1585 => {
                    if let Some(t) = child(&c, 0x251c) {
                        let mut th = Thumbnail::default();
                        for x in kids(&t) {
                            match x.tag {
                                0x251d => th.camera = child(&x, 0x34bc).and_then(|k| camera(&k)),
                                0x251e => {
                                    th.image = child(&x, 0x2328)
                                        .and_then(|i| child(&i, 0x232a))
                                        .and_then(|p| self.archive_file(&str_of(&p)));
                                }
                                _ => {}
                            }
                        }
                        d.thumbnail = Some(th);
                    }
                }
                _ => {}
            }
        }
        Ok(d)
    }

    /// An entities collection (0x1388); returns it and its own entity part (id, dictionaries).
    fn entities(&mut self, r: &Rec, depth: usize) -> Result<(Ents, El)> {
        if depth > MAX_DEPTH {
            return Err(bad("collections trop imbriquées"));
        }
        let mut e = Ents::default();
        let mut el = El::default();
        for c in kids(r) {
            match c.tag {
                0x7d0 => el = drawing_element(&c, depth)?,
                0x1389 => {
                    for v in kids(&c).filter(|x| x.tag == 0x9c4) {
                        let mut vx = Vertex::default();
                        for f in kids(&v) {
                            match f.tag {
                                0x5dc => vx.id = entity(&f, depth)?.id,
                                0x9c5 => vx.pos = f64s::<3>(&f).unwrap_or_default(),
                                _ => {}
                            }
                        }
                        e.verts.push(vx);
                    }
                }
                0x138a => {
                    for x in kids(&c).filter(|x| x.tag == 0xbb8) {
                        let mut edge = Edge::default();
                        for f in kids(&x) {
                            match f.tag {
                                0x7d0 => edge.el = drawing_element(&f, depth)?,
                                0xbb9 => edge.start = id_of(&f).unwrap_or(0),
                                0xbba => edge.end = id_of(&f).unwrap_or(0),
                                0xbbb => edge.curve = id_of(&f),
                                _ => {}
                            }
                        }
                        e.edges.push(edge);
                    }
                }
                0x138b => {
                    for x in kids(&c).filter(|x| x.tag == 0xdac) {
                        e.faces.push(face(&x, depth)?);
                    }
                }
                0x138c => {
                    for x in kids(&c).filter(|x| x.tag == 0x1964) {
                        e.instances.push(instance(&x, depth)?);
                    }
                }
                0x138d => {
                    for g in kids(&c).filter(|x| x.tag == 0x1d4c) {
                        if let Some(x) = child(&g, 0x1964) {
                            e.groups.push(instance(&x, depth)?);
                        }
                    }
                }
                0x1396 => {
                    for x in kids(&c).filter(|x| x.tag == 0x4a38) {
                        e.curves.push(curve(&x, depth)?);
                    }
                }
                0x1397 => {
                    for a in kids(&c).filter(|x| x.tag == 0x4c2c) {
                        let mut cv = Curve::default();
                        for f in kids(&a) {
                            match f.tag {
                                0x4a38 => cv = curve(&f, depth)?,
                                0x4c2d => cv.arc = arc(&f),
                                _ => {}
                            }
                        }
                        e.curves.push(cv);
                    }
                }
                0x1390 => {
                    for x in kids(&c).filter(|x| x.tag == 0x1f40) {
                        if let Some(i) = child(&x, 0x1964) {
                            e.images.push(Image { inst: instance(&i, depth)?, ..Default::default() });
                        }
                    }
                }
                0x1391 => {
                    for x in kids(&c).filter(|x| x.tag == 0x4269) {
                        let g = guide_line(&x, depth)?;
                        // The line as SketchUp keeps it (an annotation can be attached to it).
                        if let Some(v) = child(&x, 0x426a).and_then(|f| f64s::<8>(&f)) {
                            self.guide_lines.insert(g.el.id, ([v[0], v[1], v[2]], [v[3], v[4], v[5]]));
                        }
                        e.guide_lines.push(g);
                    }
                }
                0x1392 => {
                    for x in kids(&c).filter(|x| x.tag == 0x426c) {
                        let mut g = GuidePoint::default();
                        for f in kids(&x) {
                            match f.tag {
                                0x4268 => g.el = construction(&f, depth)?,
                                0x426d => g.pos = f64s::<3>(&f).unwrap_or_default(),
                                _ => {}
                            }
                        }
                        e.guide_points.push(g);
                    }
                }
                0x1393 => {
                    for x in kids(&c).filter(|x| x.tag == 0x445c) {
                        let mut s = Section::default();
                        for f in kids(&x) {
                            match f.tag {
                                0x7d0 => s.el = drawing_element(&f, depth)?,
                                0x445d => s.plane = f64s::<4>(&f).unwrap_or_default(),
                                0x445e => s.name = str_of(&f),
                                0x445f => s.symbol = str_of(&f),
                                0x4460 => s.hatch = id_of(&f).filter(|&i| i != 0),
                                _ => {}
                            }
                        }
                        e.sections.push(s);
                    }
                }
                0x1394 => e.active_section = id_of(&c),
                // Relationships: what a component is glued
                // to, the component. The map under 0x139b, or its list alone.
                0x139b | 0x63a0 => {
                    let lists: Vec<Rec> = if c.tag == 0x63a0 {
                        vec![c]
                    } else {
                        kids(&c)
                            .filter(|m| m.tag == 0x639f)
                            .flat_map(|m| kids(&m).filter(|l| l.tag == 0x63a0).collect::<Vec<_>>())
                            .collect()
                    };
                    for l in &lists {
                        for x in kids(l).filter(|x| x.tag == 0x639c) {
                            let (to, what) =
                                (child(&x, 0x639d).and_then(|v| id_of(&v)), child(&x, 0x639e).and_then(|v| id_of(&v)));
                            if let (Some(to), Some(what)) = (to, what) {
                                e.glues.push([to, what]);
                            }
                        }
                    }
                }
                // Snaps (30000 = 0x7530): position, direction, (right), up. Read from the writer
                // of SketchUp.exe and the getters of the SDK; no file of the corpus has one.
                0x139d => {
                    for x in kids(&c).filter(|x| x.tag == 0x7530) {
                        let mut s = Snap::default();
                        for f in kids(&x) {
                            match f.tag {
                                0x7d0 => s.el = drawing_element(&f, depth)?,
                                0x7531 => s.pos = f64s::<3>(&f).unwrap_or_default(),
                                0x7532 => s.dir = f64s::<3>(&f).unwrap_or_default(),
                                0x7534 => s.up = f64s::<3>(&f).unwrap_or_default(),
                                _ => {}
                            }
                        }
                        e.snaps.push(s);
                    }
                }
                // Spaces (0x7e2c): name, orientation point, ceiling plane, floor perimeter (a
                // list of points, 0x7e32) and its holes (one list each, 0x7e33 › 0x7e34), as
                // SketchUp.exe reads them.
                0x13a2 => {
                    let points = |r: &Rec| -> Vec<[f64; 3]> {
                        r.data
                            .as_chunks::<24>()
                            .0
                            .iter()
                            .map(|c| {
                                std::array::from_fn(|i| {
                                    f64::from_le_bytes(c[i * 8..i * 8 + 8].try_into().unwrap_or_default())
                                })
                            })
                            .collect()
                    };
                    for x in kids(&c).filter(|x| x.tag == 0x7e2c) {
                        let mut s = SpaceRec::default();
                        for f in kids(&x) {
                            match f.tag {
                                0x7d0 => s.el = drawing_element(&f, depth)?,
                                0x7e2d => s.name = str_of(&f),
                                0x7e30 => s.orientation = f64s::<3>(&f).unwrap_or_default(),
                                0x7e31 => s.ceiling = f64s::<4>(&f).unwrap_or_default(),
                                0x7e32 => s.perimeter = points(&f),
                                0x7e33 => s.holes = kids(&f).filter(|p| p.tag == 0x7e34).map(|p| points(&p)).collect(),
                                _ => {}
                            }
                        }
                        e.spaces.push(s);
                    }
                }
                0x1398 => {
                    for x in kids(&c).filter(|x| x.tag == 0x55f0) {
                        let mut t = text(&x, depth)?;
                        if let Some(a) = child(&x, 0x55f4).and_then(|f| attach_of(&f)).filter(|a| a.kind != 0) {
                            t.connection = a.connection(|k| k);
                            self.attach.push((t.el.id, 0, a));
                        }
                        e.texts.push(t);
                    }
                }
                0x1399 => {
                    for x in kids(&c).filter(|x| x.tag == 0x5bcc) {
                        let mut d = dimension_linear(&x, depth)?;
                        for (which, tag) in [(1, 0x5bcd), (2, 0x5bce)] {
                            if let Some(a) = child(&x, tag).and_then(|f| attach_of(&f)) {
                                d.connections[which as usize - 1] = a.connection(|k| k);
                                self.attach.push((d.el.id, which, a));
                            }
                        }
                        e.dims.push(d);
                    }
                }
                0x139a => {
                    for x in kids(&c).filter(|x| x.tag == 0x5dc0) {
                        e.radials.push(dimension_radial(&x, depth)?);
                    }
                }
                // 3D polylines (0x4844): the drawing element, 0x4845 the points.
                0x1395 => {
                    for x in kids(&c).filter(|x| x.tag == 0x4844) {
                        let mut pl = Polyline::default();
                        for f in kids(&x) {
                            match f.tag {
                                0x7d0 => pl.el = drawing_element(&f, depth)?,
                                0x4845 => {
                                    self.budget.charge(f.data.len() as u64)?;
                                    pl.points = f
                                        .data
                                        .as_chunks::<24>()
                                        .0
                                        .iter()
                                        .map(|c| {
                                            let v =
                                                |i: usize| f64::from_le_bytes(c[i..i + 8].try_into().unwrap_or([0; 8]));
                                            [v(0), v(8), v(16)]
                                        })
                                        .filter(|p| p.iter().all(|x| x.is_finite()))
                                        .collect();
                                }
                                _ => {}
                            }
                        }
                        e.polylines.push(pl);
                    }
                }
                _ => {}
            }
        }
        // What SketchUp's Check Validity dissolves when it opens the model (§3.5). The walk along
        // the curves of the archives is not made: it would dissolve closed curves that SketchUp
        // keeps (their first edge is the one SketchUp chose when it saved).
        super::curves::check_arcs(&mut e);
        super::faces::check_faces(&mut e);
        Ok((e, el))
    }

    /// Scenes (0x207 → page list 0x6d60 → pages 0x7148) and the selected one.
    fn scenes(&mut self, r: &Rec) -> Result<()> {
        let Some(list) = child(r, 0x6d60) else { return Ok(()) };
        let mut selected = None;
        for c in kids(&list) {
            match c.tag {
                0x6d61 => {
                    for p in kids(&c).filter(|x| x.tag == 0x7148) {
                        let s = self.scene(&p)?;
                        self.doc.scenes.push(s);
                    }
                }
                0x6d62 => selected = id_of(&c),
                _ => {}
            }
        }
        if let Some(sel) = selected {
            self.doc.active_scene = self.doc.scenes.iter().position(|s| s.id == sel);
        }
        Ok(())
    }

    fn scene(&mut self, r: &Rec) -> Result<Scene> {
        let mut s = Scene { animate: true, transition: -1.0, delay: -1.0, v7157: true, ..Default::default() };
        let mut hidden = None;
        for c in kids(r) {
            match c.tag {
                0x6f54 => {
                    for x in kids(&c) {
                        match x.tag {
                            0x5dc => {
                                let e = entity(&x, 1)?;
                                (s.id, s.attrs) = (e.id, e.attrs);
                            }
                            0x6f55 => s.name = str_of(&x),
                            0x6f56 => s.description = str_of(&x),
                            _ => {}
                        }
                    }
                }
                0x7149 => s.flags = u32_of(&c).unwrap_or(0),
                0x714c => s.style = id_of(&c),
                0x714a => s.camera = child(&c, 0x34bc).and_then(|x| camera(&x)),
                0x714d => {
                    if let Some(x) = child(&c, 0x733c) {
                        s.rendering = Some(rendering_options(&x));
                        s.ro_hatch = self.ro_hatch(&x)?;
                    }
                }
                0x7150 => hidden = Some(id_list(c.data)),
                0x7152 => s.animate = bool_of(&c),
                0x7156 => s.photo = id_of(&c).filter(|&i| i != 0),
                0x7153 => s.v7153 = str_of(&c),
                0x7154 => s.transition = f64_of(&c).unwrap_or(-1.0),
                0x7155 => s.delay = f64_of(&c).unwrap_or(-1.0),
                0x7157 => s.v7157 = bool_of(&c),
                0x715a => s.env_id = id_of(&c).filter(|&i| i != 0),
                0x715b => {
                    let mut e = SceneEnv::default();
                    for x in kids(&c) {
                        let f =
                            || f32::from_le_bytes(x.data.get(..4).and_then(|b| b.try_into().ok()).unwrap_or([0; 4]));
                        match x.tag {
                            0x7b0f => e.reflections = bool_of(&x),
                            0x7b10 => e.skydome = bool_of(&x),
                            0x7b11 => e.rotation = f(),
                            0x7b12 => e.skydome_exposure = f(),
                            0x7b13 => e.reflection_exposure = f(),
                            0x7b14 => e.linked_sun = bool_of(&x),
                            0x7b15 => e.sun[0] = f(),
                            0x7b16 => e.sun[1] = f(),
                            _ => {}
                        }
                    }
                    s.env = Some(e);
                }
                // Its thumbnail: 0x2134 its file in the archive, or 0x2135 the
                // image itself.
                0x7158 => {
                    if let Some(p) = child(&c, 0x2134) {
                        s.thumbnail = self.archive_file(&str_of(&p));
                    } else if let Some(i) = child(&c, 0x2135) {
                        self.budget.charge(i.data.len() as u64)?;
                        s.thumbnail = Some(std::sync::Arc::new(i.data.to_vec()));
                    }
                }
                _ => {}
            }
        }
        if s.flags & 0x20 != 0 {
            s.hidden_layers = Some(hidden.unwrap_or_default());
        }
        // Scenes of files before SketchUp 2025 (no environments yet) keep their environment
        // (0x200) once loaded, as those of the MFC archives.
        if self.doc.version.0 < 25 {
            s.flags |= 0x200;
        }
        Ok(s)
    }

    /// The environments (0x210 → 0x7918 `CEnvironmentManager`: 0x7919 the list of 0x7b0c
    /// `CEnvironment` {0x5dc its id, 0x7b0d its folder of `environments/`, 0x7b0e → 0x2134 its
    /// thumbnail}, 0x791a the current one's id). Each folder's `environment.xml` gives the
    /// settings and the panorama's file.
    fn environments(&mut self, r: &Rec) -> Result<()> {
        for m in kids(r).filter(|x| x.tag == 0x7918) {
            for c in kids(&m) {
                match c.tag {
                    0x7919 => {
                        for x in kids(&c).filter(|x| x.tag == 0x7b0c) {
                            let mut e = Environment::default();
                            for f in kids(&x) {
                                match f.tag {
                                    0x5dc => e.id = entity(&f, 1)?.id,
                                    0x7b0d => e.folder = str_of(&f),
                                    0x7b0e => {
                                        e.thumbnail = child(&f, 0x2134).and_then(|p| self.archive_file(&str_of(&p)))
                                    }
                                    _ => {}
                                }
                            }
                            self.environment_xml(&mut e)?;
                            self.doc.environments.push(e);
                        }
                    }
                    0x791a => self.doc.current_env = id_of(&c).filter(|&i| i != 0),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// An environment's `environment.xml` (`<env:environment …>`, its panorama in
    /// `<ibl_image path>`, a file of its folder).
    fn environment_xml(&mut self, e: &mut Environment) -> Result<()> {
        let dir = format!("environments/{}", e.folder.trim_matches('/'));
        let Some(bytes) = self.archive_file(&format!("{dir}/environment.xml")) else { return Ok(()) };
        let root = super::xml::parse(&bytes)?;
        let mut stack = vec![(&root, 0usize)];
        let mut env = None;
        while let Some((x, depth)) = stack.pop() {
            if x.name == "environment" {
                env = Some(x);
                break;
            }
            if depth < 8 {
                stack.extend(x.children.iter().map(|c| (c, depth + 1)));
            }
        }
        let Some(x) = env else { return Ok(()) };
        let num = |k: &str, d: f64| {
            x.attr(k).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(d)
        };
        let flag = |k: &str| x.attr(k).is_some_and(|v| v.trim() == "1" || v.trim() == "true");
        e.name = x.attr("name").unwrap_or_default().to_string();
        e.description = x.attr("description").unwrap_or_default().to_string();
        e.reflections = flag("ibl_enabled");
        e.skydome = flag("skybox_enabled");
        e.linked_sun = flag("shadow_light_enabled");
        e.rotation = num("rotation_angle", 0.0);
        e.skydome_exposure = num("skybox_gain", 1.0);
        e.reflection_exposure = num("reflection_gain", 1.0);
        e.sun = [num("shadow_light_positionX", 0.0), num("shadow_light_positionY", 0.5)];
        e.image_name = x.attr("ibl_filename").unwrap_or_default().to_string();
        // The panorama: a file of the folder (`./name`), never a path out of it.
        if let Some(p) = x.child("ibl_image").and_then(|i| i.attr("path")) {
            let name = p.trim_start_matches("./");
            if !name.contains("..") && !name.starts_with('/') {
                e.image = self.archive_file(&format!("{dir}/{name}"));
            }
        }
        Ok(())
    }

    /// The matched photos (0x201 → 0x2904 `CMatchPhoto`): 0x5dc its id, 0x2905 → 0x2af8 the
    /// image (0x2af9 file it came from, 0x2afa, 0x2afb → 0x2328 its file in the archive, 0x2afc
    /// width, 0x2afd height, 0x2afe, 0x2aff time), 0x2906, 0x2907, 0x2908 the 12 ends of the
    /// lines drawn on it, 0x2909 the origin's offset, 0x290a, 0x290b.
    fn match_photos(&mut self, r: &Rec) -> Result<()> {
        for x in kids(r).filter(|x| x.tag == 0x2904) {
            let mut m = MatchPhoto::default();
            for f in kids(&x) {
                match f.tag {
                    0x5dc => m.id = entity(&f, 1)?.id,
                    0x2905 => {
                        for i in kids(&f).filter(|i| i.tag == 0x2af8).flat_map(|i| kids(&i).collect::<Vec<_>>()) {
                            match i.tag {
                                0x2af9 => m.path = str_of(&i),
                                0x2afa => m.kind = u32_of(&i).unwrap_or(0),
                                0x2afb => {
                                    m.image = child(&i, 0x2328)
                                        .and_then(|p| child(&p, 0x232a))
                                        .and_then(|p| self.archive_file(&str_of(&p)));
                                }
                                0x2afc => m.size[0] = u32_of(&i).unwrap_or(0),
                                0x2afd => m.size[1] = u32_of(&i).unwrap_or(0),
                                0x2afe => m.v2afe = u32_of(&i).unwrap_or(0),
                                0x2aff => m.time = u32_of(&i).unwrap_or(0),
                                _ => {}
                            }
                        }
                    }
                    0x2906 => m.flag = bool_of(&f),
                    0x2907 => m.value = f64_of(&f).unwrap_or(0.0),
                    0x2908 => {
                        self.budget.charge(f.data.len() as u64)?;
                        m.points = f
                            .data
                            .as_chunks::<24>()
                            .0
                            .iter()
                            .map(|c| {
                                let v = |i: usize| f64::from_le_bytes(c[i..i + 8].try_into().unwrap_or([0; 8]));
                                [v(0), v(8), v(16)]
                            })
                            .collect();
                    }
                    0x2909 => {
                        if let Some(c) = f.data.first_chunk::<24>() {
                            let v = |i: usize| f64::from_le_bytes(c[i..i + 8].try_into().unwrap_or([0; 8]));
                            m.offset = [v(0), v(8), v(16)];
                        }
                    }
                    0x290a => m.scale = f64_of(&f).unwrap_or(0.0),
                    0x290b => m.v290b = u32_of(&f).unwrap_or(0),
                    _ => {}
                }
            }
            self.doc.photos.push(m);
        }
        Ok(())
    }

    /// The classification schemas (0x205 → 0x6784 `CClassifications`: 0x6785 the file it was
    /// loaded from, 0x6786 its name, 0x6787 its `.skc` in the archive).
    fn classifications(&mut self, r: &Rec) {
        for s in kids(r).filter(|x| x.tag == 0x6784) {
            let (mut path, mut name, mut stored) = (String::new(), String::new(), String::new());
            for c in kids(&s) {
                match c.tag {
                    0x6785 => path = str_of(&c),
                    0x6786 => name = str_of(&c),
                    0x6787 => stored = str_of(&c),
                    _ => {}
                }
            }
            // Without its file SketchUp has no schema to load.
            if let Some(data) = self.archive_file(&stored) {
                self.doc.schemas.push(super::doc::SchemaFile { path, name, data });
            }
        }
    }

    /// The hatch patterns (0x211 → 0x7d00 `HatchPatternManager` → 0x7d01 the list of 0x7d0a
    /// `HatchPattern`): 0x5dc its id, 0x7d15 its folder of `hatchpatterns/`,
    /// 0x7d0b its name, 0x7d10 its thumbnail (0x2134 its file, 0x2135 the image). The settings
    /// are in the folder's `hatchpattern.xml`.
    fn hatch_patterns(&mut self, r: &Rec) -> Result<()> {
        for m in kids(r).filter(|x| x.tag == 0x7d00) {
            for l in kids(&m).filter(|x| x.tag == 0x7d01) {
                for x in kids(&l).filter(|x| x.tag == 0x7d0a) {
                    let mut h = HatchPattern::default();
                    for f in kids(&x) {
                        match f.tag {
                            0x5dc => h.id = entity(&f, 1)?.id,
                            0x7d15 => h.folder = str_of(&f),
                            0x7d0b => h.name = str_of(&f),
                            0x7d10 => h.thumbnail = self.image_ref(&f)?,
                            _ => {}
                        }
                    }
                    self.hatch_xml(&mut h)?;
                    self.doc.hatch_patterns.push(h);
                }
            }
        }
        Ok(())
    }

    /// An image kept by a record: 0x2134 its file in the archive, or 0x2135
    /// the image itself. Counted.
    fn image_ref(&self, r: &Rec) -> Result<Option<std::sync::Arc<Vec<u8>>>> {
        if let Some(p) = child(r, 0x2134) {
            return Ok(self.archive_file(&str_of(&p)));
        }
        let Some(i) = child(r, 0x2135) else { return Ok(None) };
        self.budget.charge(i.data.len() as u64)?;
        Ok(Some(std::sync::Arc::new(i.data.to_vec())))
    }

    /// A pattern's `hatchpattern.xml` (`<hp:hatchpattern name …>` with the attributes of its
    /// settings; an image pattern's image a file of its folder, `<textureName path>`).
    fn hatch_xml(&mut self, h: &mut HatchPattern) -> Result<()> {
        let dir = format!("hatchpatterns/{}", h.folder.trim_matches('/'));
        let Some(bytes) = self.archive_file(&format!("{dir}/hatchpattern.xml")) else { return Ok(()) };
        if let Some((s, image)) = hatch_xml_settings(&bytes)? {
            h.settings = s;
            // The image of an image pattern: a file of the folder, never a path out of it.
            if let Some(name) = image.filter(|n| !n.contains("..") && !n.starts_with('/')) {
                h.settings.image = self.archive_file(&format!("{dir}/{name}"));
            }
        }
        h.xml = Some(bytes);
        Ok(())
    }

    /// The hatch pattern settings of rendering options (0x7388 → 0x7d0a):
    /// 0x7d0c colour, 0x7d0d → 0x2328 the image (0x232a a file of the archive, 0x232b the
    /// bytes), 0x7d12 its file name, 0x7d0e and 0x7d0f the scales, 0x7d14 the rotation, 0x7d11
    /// the .pat floats, 0x7d13 the .pat file's name.
    fn ro_hatch(&self, ro: &Rec) -> Result<Option<HatchSettings>> {
        let Some(s) = child(ro, 0x7388).and_then(|w| child(&w, 0x7d0a)) else { return Ok(None) };
        let mut h = HatchSettings::default();
        let f32_of = |c: &Rec| c.data.get(..4).map_or(0.0, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        for c in kids(&s) {
            match c.tag {
                0x7d0c => h.color = color_of(&c).unwrap_or([0, 0, 0, 255]),
                0x7d0d => {
                    if let Some(f) = child(&c, 0x2328) {
                        if let Some(p) = child(&f, 0x232a) {
                            h.image = self.archive_file(&str_of(&p));
                        } else if let Some(i) = child(&f, 0x232b) {
                            self.budget.charge(i.data.len() as u64)?;
                            h.image = Some(std::sync::Arc::new(i.data.to_vec()));
                        }
                    }
                }
                0x7d12 => h.texture_name = str_of(&c),
                0x7d0e => h.scale[0] = f32_of(&c),
                0x7d0f => h.scale[1] = f32_of(&c),
                0x7d14 => h.rotation = f32_of(&c),
                0x7d11 => {
                    self.budget.charge(c.data.len() as u64)?;
                    h.pat = Some(c.data.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect());
                }
                0x7d13 => h.pat_name = str_of(&c),
                _ => {}
            }
        }
        Ok(Some(h))
    }

    /// A file of the archive kept by the model (an image of a style, a watermark): counted.
    fn archive_file(&self, path: &str) -> Option<std::sync::Arc<Vec<u8>>> {
        let b = self.archive?.read_name(path).ok()?;
        self.budget.charge(b.len() as u64).ok()?;
        Some(b)
    }

    /// The fonts (0x1fd → 0x4e20 `CFontManager` → 0x4e21 → 0x5014 `CSkFont`: 0x5015 face,
    /// 0x5016 bold, 0x5017 italic, 0x5018 points, 0x5019 world size, 0x501a height).
    fn fonts(&mut self, r: &Rec) -> Result<()> {
        for m in kids(r).filter(|x| x.tag == 0x4e20) {
            for l in kids(&m).filter(|x| x.tag == 0x4e21) {
                for f in kids(&l).filter(|x| x.tag == 0x5014) {
                    let mut font = Font { height: 10.0, ..Default::default() };
                    for c in kids(&f) {
                        match c.tag {
                            0x5dc => font.id = entity(&c, 1)?.id,
                            0x5015 => font.face = str_of(&c),
                            0x5016 => font.bold = bool_of(&c),
                            0x5017 => font.italic = bool_of(&c),
                            0x5018 => font.size = u32_of(&c).unwrap_or(0),
                            0x5019 => font.world = bool_of(&c),
                            0x501a => font.height = f64_of(&c).unwrap_or(0.0),
                            _ => {}
                        }
                    }
                    self.doc.fonts.push(font);
                }
            }
        }
        Ok(())
    }

    /// The styles (0x206 → 0x6978 `CSkpStyleManager`): the list (0x6979 → 0x6b6c
    /// `CSkpStyle`), the selected one (0x697a, id), the active one (0x697b → 0x6b6c, the copy
    /// that holds the changes not saved in the selected one), whether it was changed (0x697c).
    fn styles(&mut self, r: &Rec) -> Result<()> {
        let Some(m) = child(r, 0x6978) else { return Ok(()) };
        let mut selected = None;
        for c in kids(&m) {
            match c.tag {
                0x6979 => {
                    for s in kids(&c).filter(|x| x.tag == 0x6b6c) {
                        let st = self.style(&s)?;
                        self.doc.styles.list.push(st);
                    }
                }
                0x697a => selected = id_of(&c),
                0x697b => {
                    if let Some(s) = child(&c, 0x6b6c) {
                        self.doc.styles.active = Some(self.style(&s)?);
                    }
                }
                0x697c => self.doc.styles.changed = bool_of(&c),
                _ => {}
            }
        }
        let st = &mut self.doc.styles;
        st.selected = selected.and_then(|id| st.list.iter().position(|s| s.id == id && id != 0));
        Ok(())
    }

    /// A style (0x6b6c): entity, GUID (0x6b6d), the `.style` it came from (0x6b6e), its folder
    /// in the archive (0x6b6f: `styles/<folder>/style.xml`), the watermarks it shows (0x6b70,
    /// ids); its items, name and description are in its `style.xml`.
    fn style(&mut self, r: &Rec) -> Result<super::style::StyleRec> {
        let mut s = super::style::StyleRec::default();
        for f in kids(r) {
            match f.tag {
                0x5dc => s.id = entity(&f, 1)?.id,
                0x6b6d => s.guid = guid_of(&f),
                0x6b6e => s.path = str_of(&f),
                0x6b6f => s.folder = str_of(&f),
                0x6b70 => s.watermark_ids = id_list(f.data),
                _ => {}
            }
        }
        let dir = format!("styles/{}", s.folder);
        if let Some(xml) = self.archive_file(&format!("{dir}/style.xml")) {
            let file = |p: &str| match p.strip_prefix("./") {
                Some(name) => self.archive_file(&format!("{dir}/{name}")),
                None => self.archive_file(p).or_else(|| self.archive_file(&format!("{dir}/{p}"))),
            };
            if let Ok(x) = super::style::from_xml(&xml, &file) {
                (s.name, s.description, s.items) = (x.name, x.description, x.items);
                super::style::fill_defaults(&mut s.items);
            }
        }
        if s.name.is_empty() {
            s.name = s.folder.clone();
        }
        Ok(s)
    }

    /// The model's watermarks (0x203 → 0x2cec `CWatermarkManager`): the list (0x2ced → 0x2ee0
    /// `CWatermark`) and how many of them are behind the model (0x2cee).
    fn watermark_manager(&mut self, r: &Rec) -> Result<()> {
        let Some(m) = child(r, 0x2cec) else { return Ok(()) };
        let mut list = Vec::new();
        let mut behind = 0usize;
        for c in kids(&m) {
            match c.tag {
                0x2ced => {
                    for w in kids(&c).filter(|x| x.tag == 0x2ee0) {
                        list.push(Some(self.watermark(&w)?));
                    }
                }
                0x2cee => behind = u32_of(&c).unwrap_or(0) as usize,
                _ => {}
            }
        }
        if !list.is_empty() {
            list.insert(behind.min(list.len()), None);
        }
        self.doc.styles.watermarks = list;
        Ok(())
    }

    /// A watermark (0x2ee0): entity; 0x2ee1 the file it came from; 0x2ee2 found there; 0x2ee3
    /// when; 0x2ee4 name; 0x2ee5 position; 0x2ee6 its image (0x2328 → 0x232a, a file of the
    /// archive); 0x2ee7 tiled, 0x2ee8 stretched, 0x2ee9 keeps its proportions; 0x2eee fitting;
    /// 0x2eef stretch type; 0x2eea behind the model; 0x2eeb scale; 0x2eec brightness is
    /// transparency; 0x2eed opacity.
    fn watermark(&mut self, r: &Rec) -> Result<super::style::Wm> {
        let mut w = super::style::Wm { alpha: 1.0, scale: 1.0, fitting: 2, ..Default::default() };
        let (mut tiled, mut stretched, mut fitting) = (false, false, None);
        for f in kids(r) {
            match f.tag {
                0x5dc => w.id = entity(&f, 1)?.id,
                0x2ee1 => w.file = str_of(&f),
                0x2ee2 => w.found = bool_of(&f),
                0x2ee3 => w.time = u32_of(&f).unwrap_or(0) as i64,
                0x2ee4 => w.name = str_of(&f),
                0x2ee5 => w.position = u32_of(&f).unwrap_or(4).min(8),
                0x2ee6 => {
                    if let Some(p) = child(&f, 0x2328).and_then(|x| child(&x, 0x232a)) {
                        let path = str_of(&p);
                        w.image = self.archive_file(&path);
                        w.image_name = path.rsplit('/').next().unwrap_or_default().to_string();
                    }
                }
                0x2ee7 => tiled = bool_of(&f),
                0x2ee8 => stretched = bool_of(&f),
                0x2ee9 => w.keep_aspect = bool_of(&f),
                0x2eee => fitting = u32_of(&f),
                0x2eef => w.keep_aspect = u32_of(&f).unwrap_or(0) == 1,
                0x2eea => w.background = bool_of(&f),
                0x2eeb => w.scale = f64_of(&f).unwrap_or(1.0),
                0x2eec => w.mask = bool_of(&f),
                0x2eed => w.alpha = f64_of(&f).unwrap_or(1.0),
                _ => {}
            }
        }
        w.fitting = fitting.unwrap_or(if tiled {
            0
        } else if stretched {
            1
        } else {
            2
        });
        Ok(w)
    }
}

/// Construction geometry (0x4268): a drawing element.
fn construction(r: &Rec, depth: usize) -> Result<El> {
    match child(r, 0x7d0) {
        Some(d) => drawing_element(&d, depth),
        None => Ok(El::default()),
    }
}

/// A guide line (0x4269): point and direction, then the parameters of its two ends along the
/// direction (±1e30 for an infinite line).
fn guide_line(r: &Rec, depth: usize) -> Result<GuideLine> {
    let mut g = GuideLine::default();
    for f in kids(r) {
        match f.tag {
            0x4268 => g.el = construction(&f, depth)?,
            0x426a => {
                let v = f64s::<8>(&f).unwrap_or_default();
                let (p, d, t0, t1) = ([v[0], v[1], v[2]], [v[3], v[4], v[5]], v[6], v[7]);
                g.infinite = t0 <= -1e29 && t1 >= 1e29;
                if g.infinite {
                    (g.start, g.dir) = (p, d);
                } else {
                    let at = |t: f64| [p[0] + d[0] * t, p[1] + d[1] * t, p[2] + d[2] * t];
                    let (a, b) = (at(t0), at(t1));
                    (g.start, g.dir) = (a, [b[0] - a[0], b[1] - a[1], b[2] - a[2]]);
                }
            }
            _ => {}
        }
    }
    Ok(g)
}

/// A point an annotation is attached to (21000 = 0x5208): flag, point, instance paths.
fn connection_point(r: &Rec) -> Option<[f64; 3]> {
    let c = child(r, 0x5208)?;
    child(&c, 0x520a).and_then(|p| f64s::<3>(&p))
}

/// The attach point (0x5208) under `r`: 0x5209 its type, 0x520a the point, 0x520b and 0x520c
/// two instance paths (0x53fc: 0x53fd the entity, 0x53fe the instances, outermost first, as a
/// list of ids). Where SketchUp places it: `attach::position`.
fn attach_of(r: &Rec) -> Option<Attach<Id>> {
    let c = child(r, 0x5208)?;
    let path = |tag: u16| -> (Option<Id>, Vec<Id>) {
        match child(&c, tag).and_then(|p| child(&p, 0x53fc)) {
            Some(p) => {
                (child(&p, 0x53fd).and_then(|x| id_of(&x)), child(&p, 0x53fe).map_or(Vec::new(), |x| id_list(x.data)))
            }
            None => (None, Vec::new()),
        }
    };
    let (leaf, path1) = path(0x520b);
    let (leaf2, path2) = path(0x520c);
    Some(Attach {
        kind: child(&c, 0x5209).and_then(|x| u32_of(&x)).unwrap_or(0) as i32,
        point: child(&c, 0x520a).and_then(|p| f64s::<3>(&p)).unwrap_or_default(),
        leaf,
        path: path1,
        leaf2,
        path2,
    })
}

/// A face's plane and outer loop (edge, reversed).
type FacePlane = ([f64; 4], Vec<(Id, bool)>);

/// The entities of a 2021+ model by id, for the points annotations are attached to.
#[derive(Default)]
struct Geo {
    verts: HashMap<Id, [f64; 3]>,
    edges: HashMap<Id, (Id, Id)>,
    /// Plane and outer loop (edge, reversed).
    faces: HashMap<Id, FacePlane>,
    guide_points: HashMap<Id, [f64; 3]>,
    guide_lines: HashMap<Id, ([f64; 3], [f64; 3])>,
    arcs: HashMap<Id, [f64; 3]>,
    sections: HashMap<Id, [f64; 4]>,
    xforms: HashMap<Id, [f64; 16]>,
}

impl Geo {
    fn add(&mut self, e: &Ents) {
        self.verts.extend(e.verts.iter().map(|v| (v.id, v.pos)));
        self.edges.extend(e.edges.iter().map(|x| (x.el.id, (x.start, x.end))));
        for f in &e.faces {
            self.faces.insert(f.el.id, (f.plane, f.loops.first().cloned().unwrap_or_default()));
        }
        self.guide_points.extend(e.guide_points.iter().map(|g| (g.el.id, g.pos)));
        for c in &e.curves {
            if let Some(a) = &c.arc {
                self.arcs.insert(c.id, a.center);
            }
        }
        self.sections.extend(e.sections.iter().map(|s| (s.el.id, s.plane)));
        let insts = e.instances.iter().chain(&e.groups).chain(e.images.iter().map(|i| &i.inst));
        self.xforms.extend(insts.map(|i| (i.el.id, i.xform)));
    }
}

impl super::attach::Scene for Geo {
    type Key = Id;

    fn leaf(&self, k: &Id) -> Leaf {
        let vertex = |v: &Id| self.verts.get(v).copied();
        let edge = |e: &Id| self.edges.get(e).and_then(|(s, t)| vertex(s).zip(vertex(t)));
        if let Some(&p) = self.verts.get(k) {
            Leaf::Vertex(p)
        } else if let Some((s, t)) = edge(k) {
            Leaf::Edge(s, t)
        } else if let Some(&p) = self.guide_points.get(k) {
            Leaf::GuidePoint(p)
        } else if let Some(&(p, d)) = self.guide_lines.get(k) {
            Leaf::GuideLine(p, d)
        } else if let Some((plane, lp)) = self.faces.get(k) {
            let uses =
                lp.iter().filter_map(|(e, rev)| edge(e).map(|(s, t)| if *rev { (t, s) } else { (s, t) })).collect();
            Leaf::Face(*plane, uses)
        } else if let Some(&c) = self.arcs.get(k) {
            Leaf::Arc(c)
        } else if let Some(&p) = self.sections.get(k) {
            Leaf::Section(p)
        } else {
            Leaf::Other
        }
    }

    fn xform(&self, k: &Id) -> Option<[f64; 16]> {
        self.xforms.get(k).copied()
    }
}

/// Places the points of the annotations attached to entities (`attach::position`), once all
/// the model is read: `pending` holds (annotation id, 0 text / 1 start / 2 end of a dimension,
/// attach point).
fn place_annotations(doc: &mut Doc, pending: &[(Id, u8, Attach<Id>)], guide_lines: &HashMap<Id, ([f64; 3], [f64; 3])>) {
    if pending.is_empty() {
        return;
    }
    let mut geo = Geo { guide_lines: guide_lines.clone(), ..Default::default() };
    geo.add(&doc.root);
    for d in &doc.defs {
        geo.add(&d.ents);
    }
    let mut at: HashMap<(Id, u8), [f64; 3]> = HashMap::new();
    for (id, which, a) in pending {
        at.insert((*id, *which), super::attach::position(&geo, a));
    }
    let place = |e: &mut Ents| {
        for t in &mut e.texts {
            if let Some(p) = at.get(&(t.el.id, 0)) {
                t.point = Some(*p);
            }
        }
        for d in &mut e.dims {
            if let Some(p) = at.get(&(d.el.id, 1)) {
                d.start = *p;
            }
            if let Some(p) = at.get(&(d.el.id, 2)) {
                d.end = *p;
            }
        }
    };
    place(&mut doc.root);
    for d in &mut doc.defs {
        place(&mut d.ents);
    }
}

/// A text (22000 = 0x55f0).
fn text(r: &Rec, depth: usize) -> Result<Text> {
    let mut t = Text { leader_type: 0, ..Default::default() };
    for f in kids(r) {
        match f.tag {
            0x7d0 => t.el = drawing_element(&f, depth)?,
            0x55f1 => t.text = str_of(&f),
            0x55f2 => t.screen[0] = f64_of(&f).unwrap_or(0.0),
            0x55f3 => t.screen[1] = f64_of(&f).unwrap_or(0.0),
            0x55f4 => {
                if let Some(c) = child(&f, 0x5208) {
                    let has = child(&c, 0x5209).and_then(|x| u32_of(&x)).unwrap_or(0) != 0;
                    t.point = if has { connection_point(&f) } else { None };
                }
            }
            0x55f5 => t.leader = f64s::<3>(&f).unwrap_or_default(),
            0x55f9 => t.font = id_of(&f),
            0x55fa => t.arrow_type = u32_of(&f).unwrap_or(0) as i32,
            0x55fd => t.leader_type = u32_of(&f).unwrap_or(0) as i32,
            _ => {}
        }
    }
    Ok(t)
}

/// The text style (0x57e4): fonts (0x57e5, 0x57e6, ids), then its settings.
fn text_style(r: &Rec) -> TextStyle {
    let mut s = TextStyle::default();
    for c in kids(r) {
        match c.tag {
            0x57e5 => s.font = id_of(&c),
            0x57e6 => s.font2 = id_of(&c),
            0x57e7 => s.v1 = u32_of(&c).unwrap_or(0),
            0x57e8 => s.v2 = u32_of(&c).unwrap_or(0),
            0x57e9 => s.b1 = bool_of(&c),
            0x57ea => s.v3 = u32_of(&c).unwrap_or(0),
            0x57eb => s.b2 = bool_of(&c),
            0x57ec => s.color = color_of(&c).unwrap_or_default(),
            0x57ed => s.color2 = color_of(&c).unwrap_or_default(),
            _ => {}
        }
    }
    s
}

/// The dimension style (0x5fb4).
fn dim_style(r: &Rec) -> DimStyle {
    let mut s = DimStyle::default();
    for c in kids(r) {
        let u = || u32_of(&c).unwrap_or(0);
        match c.tag {
            0x5fb5 => s.font = id_of(&c),
            0x5fb6 => s.text3d = bool_of(&c),
            0x5fb7 => s.radial_prefix = bool_of(&c),
            0x5fb8 => s.ext_offset = u(),
            0x5fb9 => s.ext_overshoot = u(),
            0x5fba => s.line_weight = u(),
            0x5fbb => s.arrow = u() as i32,
            0x5fbc => s.arrow_size = u(),
            0x5fbd => s.highlight = bool_of(&c),
            0x5fbe => s.highlight_color = color_of(&c).unwrap_or_default(),
            0x5fbf => s.b5fbf = bool_of(&c),
            0x5fc0 => s.hide_out_of_plane = bool_of(&c),
            0x5fc1 => s.hide_out_of_plane_value = f64_of(&c).unwrap_or(0.0),
            0x5fc2 => s.hide_small = bool_of(&c),
            0x5fc3 => s.hide_small_value = f64_of(&c).unwrap_or(0.0),
            0x5fc4 => s.color = color_of(&c).unwrap_or_default(),
            0x5fc5 => s.text_color = color_of(&c).unwrap_or_default(),
            0x5fc6 => s.v5fc6 = u(),
            _ => {}
        }
    }
    s
}

/// The dimension part (23000 = 0x59d8): drawing element, text, font, arrows.
fn dimension_part(f: &Rec, depth: usize) -> Result<(El, String, Option<Id>, i32)> {
    let mut d = (El::default(), String::new(), None, 0);
    for x in kids(f) {
        match x.tag {
            0x7d0 => d.0 = drawing_element(&x, depth)?,
            0x59d9 => d.1 = str_of(&x),
            0x59da => d.2 = id_of(&x),
            0x59dc => d.3 = u32_of(&x).unwrap_or(0) as i32,
            _ => {}
        }
    }
    Ok(d)
}

/// A radial dimension (24000 = 0x5dc0): the dimension part, 0x5dc1 the edge of
/// its arc (absent, or 0 as Esquisse wrote it once: none), else 0x5dc2 the arc (as 0x4c2d), 0x5dc3 the angle of its point on the arc,
/// 0x5dc4 the leader's offset, 0x5dc5 diameter.
fn dimension_radial(r: &Rec, depth: usize) -> Result<DimRadial> {
    let mut d = DimRadial::default();
    for f in kids(r) {
        match f.tag {
            0x59d8 => (d.el, d.text, d.font, d.arrow_type) = dimension_part(&f, depth)?,
            0x5dc1 => d.edge = id_of(&f).filter(|&i| i != 0),
            0x5dc2 => d.arc = arc(&f),
            0x5dc3 => d.angle = f64_of(&f).unwrap_or(0.0),
            0x5dc4 => d.leader = f64_of(&f).unwrap_or(0.0),
            0x5dc5 => d.diameter = bool_of(&f),
            _ => {}
        }
    }
    Ok(d)
}

/// A linear dimension (0x5bcc): the dimension part (23000 = 0x59d8: text, arrows), its two
/// points, normal, x axis and the position of the dimension line.
fn dimension_linear(r: &Rec, depth: usize) -> Result<DimLinear> {
    let mut d = DimLinear::default();
    for f in kids(r) {
        match f.tag {
            0x59d8 => (d.el, d.text, d.font, d.arrow_type) = dimension_part(&f, depth)?,
            0x5bcd => d.start = connection_point(&f).unwrap_or_default(),
            0x5bce => d.end = connection_point(&f).unwrap_or_default(),
            0x5bcf => d.normal = f64s::<3>(&f).unwrap_or_default(),
            0x5bd0 => d.xaxis = f64s::<3>(&f).unwrap_or_default(),
            0x5bd3 => d.position[0] = f64_of(&f).unwrap_or(0.0),
            0x5bd2 => d.position[1] = f64_of(&f).unwrap_or(0.0),
            _ => {}
        }
    }
    Ok(d)
}

/// The sub-folders of a tag folder (16000 = 0x3e80); the manager's root folder has no name.
fn folders(r: &Rec, depth: usize) -> Vec<Folder> {
    folder(r, depth).folders
}

/// A tag folder: name (0x3e81), sub-folders (0x3e83), tags (0x3e84, packed ids), hidden
/// (0x3e82), hidden on new scenes (0x3e85).
fn folder(r: &Rec, depth: usize) -> Folder {
    let mut f = Folder { visible: true, visible_on_new_pages: true, ..Default::default() };
    for x in kids(r) {
        match x.tag {
            0x5dc => f.id = child(&x, 0x5de).and_then(|i| id_of(&i)).unwrap_or(0),
            0x3e81 => f.name = str_of(&x),
            0x3e83 if depth < 32 => {
                f.folders.extend(kids(&x).filter(|s| s.tag == 0x3e80).map(|s| folder(&s, depth + 1)))
            }
            0x3e84 => f.layers = id_list(x.data),
            0x3e82 => f.visible = !bool_of(&x),
            0x3e85 => f.visible_on_new_pages = !bool_of(&x),
            _ => {}
        }
    }
    f
}

/// A packed list of ids: each a length byte then that many bytes (little-endian).
pub(crate) fn id_list(d: &[u8]) -> Vec<Id> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < d.len() {
        let n = d[at] as usize;
        at += 1;
        if n == 0 || n > 8 || at + n > d.len() {
            break;
        }
        let mut b = [0u8; 8];
        b[..n].copy_from_slice(&d[at..at + n]);
        out.push(u64::from_le_bytes(b));
        at += n;
    }
    out
}

/// Component behaviour (7000 = 0x1b58): glues (0x1b5b) to which planes (0x1b59: any,
/// horizontal, vertical, sloped), cuts an opening (0x1b5c), scale mask (0x1b5a), always faces
/// the camera (0x1b5d), shadows face the sun (0x1b5e). As `SUComponentBehavior`: the snap is 0
/// when the component does not glue, else 1 + the plane.
fn behavior(r: &Rec) -> Behavior {
    let mut b = Behavior::default();
    let (mut glue, mut to) = (false, 0);
    for c in kids(r) {
        match c.tag {
            0x1b59 => to = u32_of(&c).unwrap_or(0) as i32,
            0x1b5a => b.no_scale_mask = u32_of(&c).unwrap_or(0) as i32,
            0x1b5b => glue = bool_of(&c),
            0x1b5c => b.cuts_opening = bool_of(&c),
            0x1b5d => b.always_face_camera = bool_of(&c),
            0x1b5e => b.shadows_face_sun = bool_of(&c),
            _ => {}
        }
    }
    b.snap = if glue { (to + 1).clamp(1, 4) } else { 0 };
    b
}

fn face(r: &Rec, depth: usize) -> Result<Face> {
    let mut f = Face::default();
    for c in kids(r) {
        match c.tag {
            0x7d0 => f.el = drawing_element(&c, depth)?,
            0xdaf => f.back = id_of(&c),
            0xdad => f.plane = f64s::<4>(&c).unwrap_or_default(),
            0xdae => {
                for lp in kids(&c).filter(|x| x.tag == 0x1194) {
                    let mut uses = Vec::new();
                    for list in kids(&lp).filter(|x| x.tag == 0x1195) {
                        for u in kids(&list).filter(|x| x.tag == 0xfa0) {
                            let (mut edge, mut rev) = (0, false);
                            for x in kids(&u) {
                                match x.tag {
                                    0xfa1 => edge = id_of(&x).unwrap_or(0),
                                    0xfa2 => rev = bool_of(&x),
                                    _ => {}
                                }
                            }
                            uses.push((edge, rev));
                        }
                    }
                    f.loops.push(uses);
                }
            }
            _ => {}
        }
    }
    Ok(f)
}

fn instance(r: &Rec, depth: usize) -> Result<Inst> {
    let mut i = Inst::default();
    for c in kids(r) {
        match c.tag {
            0x7d0 => i.el = drawing_element(&c, depth)?,
            0x1965 => i.name = str_of(&c),
            0x1966 => {
                if let Some(t) = f64s::<13>(&c) {
                    // Rows of the 3×3 part, then the translation and the scale.
                    i.xform = super::mfc::xform_of(&t);
                }
            }
            0x1967 => i.def = id_of(&c).unwrap_or(0),
            0x1969 => i.hatch = id_of(&c).filter(|&i| i != 0),
            0x1968 => i.guid = guid_of(&c),
            _ => {}
        }
    }
    i.locked = i.el.flags & flags::LOCKED != 0;
    Ok(i)
}

fn curve(r: &Rec, depth: usize) -> Result<Curve> {
    let mut cv = Curve::default();
    for c in kids(r) {
        match c.tag {
            0x5dc => cv.id = entity(&c, depth)?.id,
            0x4a39 => cv.segments = u32_of(&c).unwrap_or(0),
            0x4a3a => cv.kind = u32_of(&c).unwrap_or(0),
            0x4a3b => cv.first = id_of(&c).unwrap_or(0),
            0x4a3c => cv.last = id_of(&c).unwrap_or(0),
            _ => {}
        }
    }
    Ok(cv)
}

/// An arc (0x4c2d): centre, plane (normal, d), x and y axes (scaled by the radius), radius,
/// start and end angles.
fn arc(r: &Rec) -> Option<Arc> {
    let v = f64s::<16>(r)?;
    Some(Arc {
        center: [v[0], v[1], v[2]],
        normal: [v[3], v[4], v[5]],
        xaxis: [v[7], v[8], v[9]],
        yaxis: [v[10], v[11], v[12]],
        radius: v[13],
        start: v[14],
        end: v[15],
    })
}

/// A hatch pattern's settings from the attributes of its XML element:
/// `colorRed`, `colorGreen`, `colorBlue`, `hasTexture`, `xScale`, `yScale`, `rotationAngle`,
/// `hasPat`, `patName`, `patData` (base64 of the .pat floats).
pub(crate) fn hatch_attributes(x: &super::xml::Element) -> HatchSettings {
    let byte = |k: &str| x.attr(k).and_then(|v| v.trim().parse::<f64>().ok()).map_or(0, |v| v.clamp(0.0, 255.0) as u8);
    let num = |k: &str| x.attr(k).and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0);
    let on = |k: &str| x.attr(k).is_some_and(|v| v.trim() == "1" || v.trim() == "true");
    let mut h = HatchSettings {
        color: [byte("colorRed"), byte("colorGreen"), byte("colorBlue"), 255],
        scale: [num("xScale"), num("yScale")],
        rotation: num("rotationAngle"),
        ..Default::default()
    };
    if on("hasPat") {
        h.pat_name = x.attr("patName").unwrap_or_default().to_string();
        let floats = x.attr("patData").and_then(esq_core::photo::base64_decode).unwrap_or_default();
        h.pat = Some(floats.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect());
    }
    if on("hasTexture") {
        h.texture_name = x.attr("textureName").unwrap_or_default().to_string();
    }
    h
}

/// The settings a `hatchpatterns/<folder>/hatchpattern.xml` holds (`<hp:hatchpattern …>`), and
/// the file of its folder its image is (`<textureName path>`), without reading the image.
pub(crate) fn hatch_xml_settings(bytes: &[u8]) -> Result<Option<(HatchSettings, Option<String>)>> {
    let root = super::xml::parse(bytes)?;
    let mut stack = vec![(&root, 0usize)];
    while let Some((x, depth)) = stack.pop() {
        if x.name == "hatchpattern" {
            let mut s = hatch_attributes(x);
            let mut image = None;
            if let Some(t) = x.child("textureName") {
                image = t
                    .attr("path")
                    .map(|p| p.trim_start_matches("./"))
                    .or_else(|| t.attr("file_name"))
                    .map(str::to_string);
                s.texture_name = t.attr("file_name").unwrap_or_default().to_string();
            } else if !s.texture_name.is_empty() {
                image = Some(s.texture_name.clone());
            }
            return Ok(Some((s, image)));
        }
        if depth < 8 {
            stack.extend(x.children.iter().map(|c| (c, depth + 1)));
        }
    }
    Ok(None)
}
