//! The model of SketchUp 3 to 2020 files, read from their MFC archive (`mfc`) into a [`Doc`]
//! (`docs/skp-format.md`, §3).
//!
//! An MFC archive has no lengths: each class's `Serialize` reads its fields in a fixed order
//! that depends on the class's schema (from the file's `CVersionMap`). Every class that may
//! appear must therefore be read field by field, even the ones Esquisse does not use (styles,
//! fonts, watermarks…), or the rest of the file is lost. The readers below follow the loading
//! branch of each `Serialize` of SketchUp.exe; a class this reader does not know stops the
//! reading with an error.

use std::collections::{BTreeMap, HashMap, HashSet};

use esq_core::model::{AttrValue, Attributes};
use serde_json::{Value, json};

use super::Result;
use super::attach::{Leaf, cross, dot, sub, unit};
use super::doc::*;
use super::mfc::{Ar, MAX_DEPTH, Next};

type Attach = super::attach::Attach<usize>;

/// The version map (`CVersionMap`): class name → schema, in file order.
pub fn version_map(ar: &mut Ar) -> Result<Vec<(String, u32)>> {
    let mut out = Vec::new();
    match ar.read_ref()? {
        Next::New(name, idx) if name == "CVersionMap" => {
            ar.set_object(idx, 0);
            loop {
                let class = ar.string()?;
                let v = ar.u32()?;
                if class == "End-Of-Version-Map" {
                    break;
                }
                // Names of SketchUp 3 renamed later.
                let class = match class.as_str() {
                    "CESketchPage" => "CSketchUpPage".to_string(),
                    "CESketchDoc" => "CSketchUpModel".to_string(),
                    "CAnnotation" => "CText".to_string(),
                    _ => class,
                };
                ar.versions.insert(class.clone(), v);
                out.push((class, v));
                if out.len() > 10_000 {
                    return Err(ar.err("table des versions démesurée"));
                }
            }
        }
        _ => return Err(ar.err("table des versions absente")),
    }
    Ok(out)
}

/// The start of the archive after the GUID: a string, the save time, the version map.
pub fn header(rest: &[u8]) -> Result<(Ar<'_>, Vec<(String, u32)>)> {
    let mut ar = Ar::new(rest);
    let _ = ar.string()?;
    let _time = ar.u32()?;
    let map = version_map(&mut ar)?;
    Ok((ar, map))
}

/// The classes of the MFC archives, as their version map names them, that this reader reads
/// field by field, and whether it keeps what they hold (`false`: parsed to stay in step, of no
/// use to Esquisse). A class it does not know stops the reading (`docs/skp-format.md`, §3).
pub const CLASSES: &[(&str, bool)] = &[
    ("CVersionMap", true),
    ("CSketchUpModel", true),
    ("CEntity", true),
    ("CDrawingElement", true),
    ("CAttribute", true),
    ("CAttributeContainer", true),
    ("CAttributeNamed", true),
    ("CFaceTextureCoords", true),
    ("CVertex", true),
    ("CEdge", true),
    ("CEdgeUse", true),
    ("CLoop", true),
    ("CFace", true),
    ("CCurve", true),
    ("CArcCurve", true),
    ("CComponent", true),
    ("CComponentDefinition", true),
    ("CComponentInstance", true),
    ("CGroup", true),
    ("CImage", true),
    ("CComponentBehavior", true),
    ("CDefinitionList", true),
    ("CMaterial", true),
    ("CMaterialManager", true),
    ("CTexture", true),
    ("CDib", true),
    ("CLayer", true),
    ("CLayerManager", true),
    ("CLayerGroup", true),
    ("CConstructionGeometry", true),
    ("CConstructionLine", true),
    ("CConstructionPoint", true),
    ("CSectionPlane", true),
    ("CText", true),
    ("CDimension", true),
    ("CDimensionLinear", true),
    ("CDimensionStyle", true),
    ("CCamera", true),
    ("CPageList", true),
    ("CSketchUpPage", true),
    ("CViewPage", true),
    ("CRenderingOptions", true),
    ("CShadowInfo", true),
    ("CSketchCS", true),
    ("CLineStyleManager", true),
    ("CCustomLineStyle", true),
    ("CDimensionRadial", false),
    ("CPolyline3d", false),
    ("CThumbnail", false),
    ("CFontManager", false),
    ("CSkFont", false),
    ("CTextStyle", false),
    ("CSkpStyle", false),
    ("CSkpStyleManager", false),
    ("CWatermark", false),
    ("CWatermarkManager", false),
    ("CBackgroundImage", false),
    ("CRelationship", false),
    ("CRelationshipMap", false),
    ("CSchemaFile", false),
    ("CSchemaZipFile", false),
    ("CSchemaFilterFile", false),
];

/// Most objects of one file (an object costs at least 2 bytes, so this is no real limit).
const MAX_OBJECTS: usize = 50_000_000;

/// Schemas of the classes read, from the version map (0 for a class it does not list).
#[derive(Default)]
struct V {
    entity: u32,
    de: u32,
    edge: u32,
    edge_use: u32,
    loop_: u32,
    face: u32,
    curve: u32,
    arc: u32,
    component: u32,
    definition: u32,
    instance: u32,
    behavior: u32,
    material: u32,
    material_mgr: u32,
    texture: u32,
    dib: u32,
    layer: u32,
    layer_mgr: u32,
    layer_group: u32,
    attr_named: u32,
    tex_coords: u32,
    cline: u32,
    section: u32,
    text: u32,
    dimension: u32,
    dim_linear: u32,
    dim_radial: u32,
    dim_style: u32,
    text_style: u32,
    camera: u32,
    view_page: u32,
    rendering: u32,
    shadow: u32,
    thumbnail: u32,
    skfont: u32,
    line_style: u32,
    skp_style_mgr: u32,
    skp_style: u32,
    watermark_mgr: u32,
    background: u32,
    schema_file: u32,
    model: u32,
}

impl V {
    fn from(ar: &Ar) -> V {
        let v = |c: &str| ar.version(c);
        V {
            entity: v("CEntity"),
            de: v("CDrawingElement"),
            edge: v("CEdge"),
            edge_use: v("CEdgeUse"),
            loop_: v("CLoop"),
            face: v("CFace"),
            curve: v("CCurve"),
            arc: v("CArcCurve"),
            component: v("CComponent"),
            definition: v("CComponentDefinition"),
            instance: v("CComponentInstance"),
            behavior: v("CComponentBehavior"),
            material: v("CMaterial"),
            material_mgr: v("CMaterialManager"),
            texture: v("CTexture"),
            dib: v("CDib"),
            layer: v("CLayer"),
            layer_mgr: v("CLayerManager"),
            layer_group: v("CLayerGroup"),
            attr_named: v("CAttributeNamed"),
            tex_coords: v("CFaceTextureCoords"),
            cline: v("CConstructionLine"),
            section: v("CSectionPlane"),
            text: v("CText"),
            dimension: v("CDimension"),
            dim_linear: v("CDimensionLinear"),
            dim_radial: v("CDimensionRadial"),
            dim_style: v("CDimensionStyle"),
            text_style: v("CTextStyle"),
            camera: v("CCamera"),
            view_page: v("CViewPage"),
            rendering: v("CRenderingOptions"),
            shadow: v("CShadowInfo"),
            thumbnail: v("CThumbnail"),
            skfont: v("CSkFont"),
            line_style: v("CCustomLineStyle"),
            skp_style_mgr: v("CSkpStyleManager"),
            skp_style: v("CSkpStyle"),
            watermark_mgr: v("CWatermarkManager"),
            background: v("CBackgroundImage"),
            schema_file: v("CSchemaFile"),
            model: v("CSketchUpModel"),
        }
    }
}

/// The entity part of an object (`CEntity`): its dictionaries (an attribute container).
#[derive(Clone, Copy, Default)]
struct Ent {
    attrs: Option<usize>,
}

/// What an instance places.
#[derive(Clone, Copy, PartialEq)]
enum InstKind {
    Group,
    Component,
    Image,
}

/// A tag folder as read: its sub-folders and tags are objects of the archive.
#[derive(Clone, Default)]
struct RawFolder {
    name: String,
    hidden: bool,
    hidden_on_new_pages: bool,
    folders: Vec<usize>,
    layers: Vec<usize>,
}

/// A styles manager as read: its styles, the selected one, the active one, whether it was
/// changed (objects of the archive).
type StyleMgr = (Vec<usize>, Option<usize>, Option<usize>, bool);

/// Texture placements of both sides of a face.
type Placements = [Option<Box<TexCoords>>; 2];

/// An attribute container as read: its dictionaries, a face's texture placement, and what a
/// copy of them costs (bytes, for the budget).
type RawContainer = (Attributes, Placements, u64);

/// A dictionary as read: name, values, cost of a copy.
/// A dictionary as read: name, values, cost of a copy, the dictionaries nested in it (keys
/// `child\0grandchild…`, `doc::NEST`).
type RawDict = (String, BTreeMap<String, AttrValue>, u64, Attributes);

/// An object of the archive, as read.
#[derive(Clone)]
enum Obj {
    /// Being read (its fields are not known yet).
    Pending,
    /// Read but of no use to Esquisse.
    Other,
    Container(Box<RawContainer>),
    Dict(Box<RawDict>),
    TexCoords(Box<Placements>),
    Vertex([f64; 3]),
    Edge(Box<Edge>),
    EdgeUse(Option<usize>, bool),
    Loop(Vec<usize>),
    Face(Box<RawFace>),
    Curve(Box<Curve>),
    Material(Box<Material>),
    Image(std::sync::Arc<Vec<u8>>),
    Texture(Box<Texture>),
    Layer(Box<Layer>),
    Folder(Box<RawFolder>),
    Def(Box<RawDef>),
    Inst(Box<Inst>, InstKind),
    GuideLine(Box<GuideLine>),
    GuidePoint(Box<GuidePoint>),
    Section(Box<Section>),
    Text(Box<Text>),
    Dim(Box<DimLinear>),
    Radial(Box<DimRadial>),
    Polyline(Box<Polyline>),
    Thumb(Box<Thumbnail>),
    Photo(Box<MatchPhoto>),
    /// A relationship (`CRelationship`): what a component is glued to, the component.
    Relation(Option<usize>, Option<usize>),
    Camera(Box<Camera>),
    Page(Box<Scene>),
    LineStyle(String),
    Style(Box<super::style::StyleRec>),
    Watermark(Box<super::style::Wm>),
    Font(Box<Font>),
    Schema(Box<super::doc::SchemaFile>),
}

/// A face as read: drawing element, plane, loops (objects), back material.
type RawFace = (El, [f64; 4], Vec<usize>, Option<usize>);

// An object of the archive takes a few bytes of the file (a 2-byte tag and its fields): what
// it costs in memory stays small, the large ones are boxed.
const _: () = assert!(std::mem::size_of::<Obj>() <= 32);

/// Approximate bytes of a node of JSON: a tagged value is a small map (a node of a B-tree).
const VALUE_NODE_COST: u64 = 640;

/// Approximate bytes a value of a dictionary takes.
fn value_cost(v: &AttrValue) -> u64 {
    fn json(v: &Value) -> u64 {
        VALUE_NODE_COST
            + match v {
                Value::String(s) => s.len() as u64,
                Value::Array(a) => a.iter().map(json).sum(),
                Value::Object(o) => o.iter().map(|(k, x)| k.len() as u64 + json(x)).sum(),
                _ => 0,
            }
    }
    32 + match v {
        AttrValue::Text(s) => s.len() as u64,
        AttrValue::Json(j) => json(j),
        _ => 0,
    }
}

pub(crate) fn attrs_cost(a: &Attributes) -> u64 {
    a.iter()
        .map(|(k, d)| 48 + k.len() as u64 + d.iter().map(|(k, v)| 32 + k.len() as u64 + value_cost(v)).sum::<u64>())
        .sum()
}

/// Approximate bytes an object takes beyond its slot of the arena (counted once when read,
/// and again for each copy made of it).
fn obj_cost(o: &Obj) -> u64 {
    use std::mem::size_of;
    let s = |x: &String| x.len() as u64;
    let v = |n: usize| 8 * n as u64;
    match o {
        Obj::Pending | Obj::Other | Obj::Vertex(_) | Obj::EdgeUse(..) => 0,
        Obj::Relation(..) => 16,
        Obj::Container(c) => size_of::<RawContainer>() as u64 + c.2,
        // Its values were counted while read (`variant`); `d.2` is the cost of a copy.
        Obj::Dict(d) => size_of::<RawDict>() as u64 + s(&d.0),
        Obj::TexCoords(_) => 2 * size_of::<TexCoords>() as u64,
        Obj::Edge(_) => size_of::<Edge>() as u64,
        Obj::Loop(u) => v(u.len()),
        Obj::Face(f) => size_of::<RawFace>() as u64 + v(f.2.len()),
        Obj::Curve(_) => size_of::<Curve>() as u64 + size_of::<Arc>() as u64,
        Obj::Material(m) => {
            size_of::<Material>() as u64 + s(&m.name) + m.texture.as_ref().map_or(0, |t| s(&t.file_name))
        }
        Obj::Image(_) => 32,
        Obj::Texture(t) => size_of::<Texture>() as u64 + s(&t.file_name),
        Obj::Layer(l) => size_of::<Layer>() as u64 + s(&l.name),
        Obj::Folder(f) => size_of::<RawFolder>() as u64 + s(&f.name) + v(f.folders.len() + f.layers.len()),
        Obj::Def(d) => {
            size_of::<RawDef>() as u64 + s(&d.def.name) + s(&d.def.description) + s(&d.def.guid) + v(d.comp.items.len())
        }
        Obj::Inst(i, _) => size_of::<Inst>() as u64 + s(&i.name),
        Obj::GuideLine(_) => size_of::<GuideLine>() as u64,
        Obj::GuidePoint(_) => size_of::<GuidePoint>() as u64,
        Obj::Section(x) => size_of::<Section>() as u64 + s(&x.name) + s(&x.symbol),
        Obj::Text(t) => size_of::<Text>() as u64 + s(&t.text),
        Obj::Dim(d) => size_of::<DimLinear>() as u64 + s(&d.text),
        Obj::Radial(d) => size_of::<DimRadial>() as u64 + s(&d.text),
        // Its points were counted when read.
        Obj::Polyline(_) => size_of::<Polyline>() as u64,
        Obj::Thumb(_) => size_of::<Thumbnail>() as u64,
        // Its points were counted when read.
        Obj::Photo(m) => size_of::<MatchPhoto>() as u64 + s(&m.path),
        Obj::Camera(_) => size_of::<Camera>() as u64,
        Obj::Page(p) => {
            size_of::<Scene>() as u64
                + s(&p.name)
                + s(&p.description)
                + v(p.hidden_layers.as_ref().map_or(0, Vec::len))
                + p.rendering.as_ref().map_or(0, |r| 64 * r.len() as u64)
        }
        Obj::LineStyle(n) => s(n),
        Obj::Style(x) => {
            size_of::<super::style::StyleRec>() as u64
                + s(&x.name)
                + s(&x.description)
                + s(&x.path)
                + 64 * x.items.len() as u64
        }
        Obj::Watermark(w) => size_of::<super::style::Wm>() as u64 + s(&w.name) + s(&w.file),
        Obj::Font(f) => size_of::<Font>() as u64 + s(&f.face),
        // Its bytes were counted when read (shared by the copies).
        Obj::Schema(x) => size_of::<super::doc::SchemaFile>() as u64 + s(&x.path) + s(&x.name),
    }
}

/// A component definition as read: its entities are objects of the archive.
#[derive(Clone, Default)]
struct RawDef {
    def: Def,
    comp: RawComp,
}

/// The entities of a component (`CComponent`): objects of the archive.
#[derive(Clone, Default)]
struct RawComp {
    el: El,
    items: Vec<usize>,
    active_section: Option<usize>,
    /// Its relationships (`CRelationshipMap`).
    relations: Vec<usize>,
}

struct Reader<'a> {
    ar: Ar<'a>,
    v: V,
    objs: Vec<Obj>,
    /// Objects already given to an owner (see [`Reader::take`]).
    taken: Vec<bool>,
    /// What the reading of this file may make.
    budget: super::Budget,
    /// Images decoded (average colour, transparency), by their bytes' address.
    decoded: HashMap<usize, Option<([u8; 3], bool)>>,
    /// See [`super::Options::sdk_quirks`].
    sdk_quirks: bool,
    /// Address of the reading thread's stack where the reading started.
    stack_base: usize,
    /// Version of SketchUp that wrote the file.
    file: (u32, u32, u32),
    // What the root component holds (its managers are the model's).
    materials: Vec<usize>,
    layers: Vec<usize>,
    active_layer: Option<usize>,
    root_folder: Option<RawFolder>,
    defs: Vec<usize>,
    root: bool,
    /// The next attribute container read is a component's (`CComponent::Serialize` keeps one
    /// dictionary of some names).
    component_dicts: bool,
    line_styles: Vec<usize>,
    /// `ESQ_SKP_TRACE` set: each object and step of the model on stderr (format study).
    trace: bool,
    /// Dimensions read.
    dims: usize,
    /// Section planes read without a name (old files): SketchUp numbers them.
    unnamed_sections: usize,
    see_through_cache: HashMap<usize, bool>,
    /// The colour the model's dimension style gives the dimensions of SketchUp 3 files.
    dim_color: Option<[u8; 4]>,
    /// The arrows it gives them.
    dim_arrow: Option<i32>,
    /// The material of that colour.
    dim_material: Option<Id>,
    /// The model's classification schemas (objects).
    schema_list: Vec<usize>,
    /// The model's values whose use is not known (`Doc::model_flags`).
    model_flags: Option<ModelFlags>,
    /// `AxisOptions/DisplayAxes` of an options manager before version 2 (SketchUp 3 to 5),
    /// which SketchUp makes the model's `DisplaySketchAxes`.
    display_axes: Option<bool>,
    /// Guide lines as read (point, direction), by object.
    guide_raw: HashMap<Id, ([f64; 3], [f64; 3])>,
    /// The styles manager as read: styles, selected, active, changed.
    style_mgr: Option<StyleMgr>,
    /// The watermark manager's list (`None`: the model's place).
    watermark_list: Vec<Option<usize>>,
    /// The model's fonts (objects).
    font_list: Vec<usize>,
    /// The fonts of the texts and dimensions (objects), which SketchUp adds to the model's
    /// fonts when its font manager does not list them.
    annot_fonts: Vec<usize>,
}

/// Reads the model of a SketchUp 3–2020 file (`rest`: what follows the GUID).
pub fn read(rest: &[u8], guid: &[u8; 16], version: (u32, u32, u32), opts: &super::Options) -> Result<Doc> {
    Ok(read_counted(rest, guid, version, opts)?.0)
}

/// Stack the reading may use: objects are read where they are first referred to, so the
/// reading nests as deep as the file makes it. Within the thread of `super::on_big_stack`.
const STACK_LIMIT: usize = super::BIG_STACK - (32 << 20);

/// [`read`], and the number of bytes left after the model (format study). Runs on a thread of
/// its own with a large stack, whatever the caller's.
pub fn read_counted(
    rest: &[u8],
    guid: &[u8; 16],
    version: (u32, u32, u32),
    opts: &super::Options,
) -> Result<(Doc, usize)> {
    super::on_big_stack(|| read_here(rest, guid, version, opts))?
}

/// The offsets in `rest` of the counts and string lengths the reading of the archive reads, in
/// order (robustness tests: the fields a damaged or forged file can inflate).
pub fn count_offsets(rest: &[u8], guid: &[u8; 16], version: (u32, u32, u32)) -> Result<Vec<usize>> {
    super::on_big_stack(|| {
        super::mfc::record_counts(true);
        let _ = read_here(rest, guid, version, &super::Options::default());
        super::mfc::record_counts(false)
    })
}

thread_local! {
    /// Objects read, by class, while [`object_census`] runs.
    static CENSUS: std::cell::RefCell<Option<BTreeMap<String, usize>>> = const { std::cell::RefCell::new(None) };
}

/// The objects of the archive, by class (the managers SketchUp reads in place are not objects
/// and are not counted): what a file holds, for the tests' counters.
pub fn object_census(rest: &[u8], guid: &[u8; 16], version: (u32, u32, u32)) -> Result<BTreeMap<String, usize>> {
    super::on_big_stack(|| {
        CENSUS.set(Some(BTreeMap::new()));
        let r = read_here(rest, guid, version, &super::Options::default());
        let c = CENSUS.take().unwrap_or_default();
        r.map(|_| c)
    })?
}

fn read_here(rest: &[u8], guid: &[u8; 16], version: (u32, u32, u32), opts: &super::Options) -> Result<(Doc, usize)> {
    let (ar, _) = header(rest)?;
    let base = 0u8;
    let mut r = Reader {
        v: V::from(&ar),
        ar,
        objs: vec![Obj::Other],
        taken: vec![true],
        budget: super::Budget::for_input(rest.len()),
        decoded: HashMap::new(),
        sdk_quirks: opts.sdk_quirks,
        stack_base: std::hint::black_box(&base) as *const u8 as usize,
        file: version,
        materials: Vec::new(),
        layers: Vec::new(),
        active_layer: None,
        root_folder: None,
        defs: Vec::new(),
        root: false,
        component_dicts: false,
        line_styles: Vec::new(),
        trace: std::env::var_os("ESQ_SKP_TRACE").is_some(),
        dims: 0,
        dim_color: None,
        dim_arrow: None,
        dim_material: None,
        schema_list: Vec::new(),
        model_flags: None,
        display_axes: None,
        unnamed_sections: 0,
        see_through_cache: HashMap::new(),
        guide_raw: HashMap::new(),
        style_mgr: None,
        watermark_list: Vec::new(),
        font_list: Vec::new(),
        annot_fonts: Vec::new(),
    };
    let mut doc = Doc { version, sdk_quirks: opts.sdk_quirks, ..Default::default() };
    doc.guid = guid_text(guid);
    r.model(&mut doc)?;
    Ok((doc, r.ar.left()))
}

/// The objects of the archive, for the points annotations are attached to (they are read
/// before the annotation that names them: in place, at their first reference).
impl super::attach::Scene for Reader<'_> {
    type Key = usize;

    fn leaf(&self, k: &usize) -> Leaf {
        let vertex = |i: Id| match self.objs.get(i as usize) {
            Some(Obj::Vertex(p)) => Some(*p),
            _ => None,
        };
        // The start and end vertex of an edge use (reversed: the edge's end first).
        let ends = |u: usize| match self.objs.get(u) {
            Some(Obj::EdgeUse(Some(e), rev)) => match self.objs.get(*e) {
                Some(Obj::Edge(e)) => {
                    let (s, t) = if *rev { (e.end, e.start) } else { (e.start, e.end) };
                    vertex(s).zip(vertex(t))
                }
                _ => None,
            },
            _ => None,
        };
        match self.objs.get(*k) {
            Some(Obj::Vertex(p)) => Leaf::Vertex(*p),
            Some(Obj::Edge(e)) => match vertex(e.start).zip(vertex(e.end)) {
                Some((s, t)) => Leaf::Edge(s, t),
                None => Leaf::Other,
            },
            Some(Obj::GuidePoint(g)) => Leaf::GuidePoint(g.pos),
            Some(Obj::GuideLine(_)) => match self.guide_raw.get(&(*k as Id)) {
                Some(&(p, d)) => Leaf::GuideLine(p, d),
                None => Leaf::Other,
            },
            Some(Obj::Face(f)) => {
                let uses = match f.2.first().and_then(|&l| self.objs.get(l)) {
                    Some(Obj::Loop(uses)) => uses.iter().filter_map(|&u| ends(u)).collect(),
                    _ => Vec::new(),
                };
                Leaf::Face(f.1, uses)
            }
            Some(Obj::Curve(c)) => c.arc.as_ref().map_or(Leaf::Other, |a| Leaf::Arc(a.center)),
            Some(Obj::Section(s)) => Leaf::Section(s.plane),
            // An image: its size is one tile of the texture of its definition's face.
            Some(Obj::Inst(x, InstKind::Image)) => {
                let items = match self.objs.get(x.def as usize) {
                    Some(Obj::Def(d)) => &d.comp.items[..],
                    _ => &[][..],
                };
                let size = items.iter().find_map(|&i| match self.objs.get(i) {
                    Some(Obj::Face(f)) => match f.0.material.and_then(|m| self.objs.get(m as usize)) {
                        Some(Obj::Material(m)) => m.texture.as_ref().map(|t| t.size),
                        _ => None,
                    },
                    _ => None,
                });
                size.map_or(Leaf::Other, |s| Leaf::Image(x.xform, s))
            }
            _ => Leaf::Other,
        }
    }

    fn xform(&self, k: &usize) -> Option<[f64; 16]> {
        match self.objs.get(*k) {
            Some(Obj::Inst(x, _)) => Some(x.xform),
            _ => None,
        }
    }
}

use super::curves::arc_param;

/// The name a section plane without one (`CSectionPlane` schema < 3) holds until the model is
/// read: no name read from a file has a NUL.
const UNNAMED: &str = "\0";

/// The section planes without a name are numbered « Section n » (symbol « n ») once the model
/// is read: those of the model first, then those of each definition in the order of the list
/// of definitions — not in the order of the archive, where the definitions come first.
fn name_sections(doc: &mut Doc) {
    let mut n = 0;
    let all = std::iter::once(&mut doc.root).chain(doc.defs.iter_mut().map(|d| &mut d.ents));
    for ents in all {
        for s in ents.sections.iter_mut().filter(|s| s.name == UNNAMED) {
            n += 1;
            s.name = format!("Section {n}");
            s.symbol = n.to_string();
        }
    }
}

/// Definitions that share a name (names compare without case):
/// each one after the first, in the order of the list, gets the name
/// `CDefinitionList::GenerateUniqueName` makes: a final
/// « #digits » taken off, then the name itself if free, otherwise the name without its final
/// digits followed by « #1 », « #2 »… the first one free (« AI9-BLK2 » → « AI9-BLK#1 »).
fn unique_definition_names(doc: &mut Doc) {
    let mut taken: HashSet<String> = HashSet::new();
    // The next index to try for each stem: names are only added, so the first free index of a
    // stem never decreases (a forged file of thousands of namesakes stays linear).
    let mut next: HashMap<String, usize> = HashMap::new();
    let mut again = Vec::new();
    for (i, d) in doc.defs.iter().enumerate() {
        if !taken.insert(d.name.to_uppercase()) {
            again.push(i);
        }
    }
    for i in again {
        let name = &doc.defs[i].name;
        let mut base = name.as_str();
        if let Some(p) = base.rfind('#')
            && p > 0
            && base[p + 1..].bytes().all(|b| b.is_ascii_digit())
        {
            base = base.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('#');
        }
        let new = if taken.contains(&base.to_uppercase()) {
            let stem = base.trim_end_matches(|c: char| c.is_ascii_digit());
            let k = next.entry(stem.to_uppercase()).or_insert(1);
            loop {
                let n = format!("{stem}#{k}");
                *k += 1;
                if !taken.contains(&n.to_uppercase()) {
                    break n;
                }
            }
        } else {
            base.to_string()
        };
        taken.insert(new.to_uppercase());
        doc.defs[i].name = new;
    }
}

/// What SketchUp does with the curves of a collection once its edges are loaded (the curves
/// met while loading, after `CComponent::Serialize`):
/// - each curve is walked from its first edge read (the curve is read in place at the first
///   edge that refers to it), backwards then forwards along its edges (a vertex
///   where three edges of the curve meet stops the walk). A curve whose walk does not reach all
///   its edges is dissolved: its edges are left without a curve. The walk sets the curve's first
///   and last edges: the two ends of an open curve; for a closed one, the first edge met
///   backwards whose rear vertex has another edge, not soft, of no curve of its own… or of
///   another curve, else the first edge read.
/// - without first and last edges in the file (`recompute`: `CCurve` schema ≠ 3), an arc's
///   frame and angles are worked out again from its end vertices (`CArcCurve`, vtable slot 25):
///   the x axis turned to the first vertex (at the arc's radius), the normal
///   made square to it, the y axis = normal × x, start angle 0; the end angle is that of the last
///   vertex, or for a closed arc 0 when its first vertex has two edges (a lone circle), 2π
///   otherwise (the end of a cylinder) — whatever angles the file holds.
fn settle_curves(e: &mut Ents, recompute: bool) {
    if e.curves.is_empty() {
        return;
    }
    // Counted once (a forged file may put thousands of edges, of thousands of curves, at one
    // vertex: no step of a walk looks through all the edges of a vertex): the edges at each
    // vertex, those that are not soft, and the edges of each curve at each vertex (indices of
    // `e.edges`, in their order).
    let mut valences: HashMap<Id, usize> = HashMap::new();
    let mut hard: HashMap<Id, usize> = HashMap::new();
    let mut of_curve: HashMap<(Id, Id), Vec<usize>> = HashMap::new();
    let mut hard_of_curve: HashMap<(Id, Id), usize> = HashMap::new();
    let mut members: HashMap<Id, Vec<usize>> = HashMap::new();
    for (i, x) in e.edges.iter().enumerate() {
        let soft = x.el.flags & flags::SOFT != 0;
        let ends = if x.end != x.start { &[x.start, x.end][..] } else { &[x.start][..] };
        for &v in ends {
            *valences.entry(v).or_default() += 1;
            if !soft {
                *hard.entry(v).or_default() += 1;
            }
            if let Some(c) = x.curve {
                of_curve.entry((v, c)).or_default().push(i);
                if !soft {
                    *hard_of_curve.entry((v, c)).or_default() += 1;
                }
            }
        }
        if let Some(c) = x.curve {
            members.entry(c).or_default().push(i);
        }
    }
    let pos: HashMap<Id, [f64; 3]> = e.verts.iter().map(|v| (v.id, v.pos)).collect();
    let valence = |v: Id| valences.get(&v).copied().unwrap_or(0);
    let mut dissolved = HashSet::new();
    for c in e.curves.iter_mut() {
        let Some(list) = members.get_mut(&c.id) else { continue };
        // The first edge read: the lowest index of the archive.
        list.sort_by_key(|&i| e.edges[i].el.id);
        let n = list.len();
        let edges = &e.edges;
        let cid = c.id;
        let other = |i: usize, v: Id| if edges[i].start == v { edges[i].end } else { edges[i].start };
        // The next edge of the curve at `v`: none where three meet.
        let next = |i: usize, v: Id| -> Option<usize> {
            let mine = of_curve.get(&(v, cid)).map_or(&[][..], Vec::as_slice);
            if mine.len() < 3 { mine.iter().rev().copied().find(|&j| j != i) } else { None }
        };
        // The file's first and last edges (2021+), else the first edge read and none
        let find = |id: Id| (id != 0).then(|| list.iter().copied().find(|&i| edges[i].el.id == id)).flatten();
        let (file_first, file_last) = (find(c.first), find(c.last));
        // Forwards along the arc from its start vertex to its end vertex;
        // plain curves: always (the walk clears the curve's ends first, which the direction of
        // their edges depends on).
        let forward = |i: usize| -> bool {
            let Some(a) = &c.arc else { return true };
            let x = &edges[i];
            let (Some(&p), Some(&q)) = (pos.get(&x.start), pos.get(&x.end)) else { return true };
            super::curves::forward(a, p, q)
        };
        let rear = |i: usize| if forward(i) { edges[i].start } else { edges[i].end };
        // The walk from one edge: the first and last edges, and whether it
        // went through all the curve's edges.
        let walk = |start: usize| -> (usize, usize, bool) {
            let target = other(start, rear(start));
            let (mut cur, mut v, mut walked) = (start, rear(start), 1usize);
            let mut candidate = None;
            loop {
                if candidate.is_none() && valence(v) != 2 {
                    // Another edge at `v`, not soft, not of this curve.
                    let foreign =
                        hard.get(&v).copied().unwrap_or(0) > hard_of_curve.get(&(v, cid)).copied().unwrap_or(0);
                    if foreign {
                        candidate = Some(cur);
                    }
                }
                if v == target {
                    // Around a closed curve.
                    let first = candidate.unwrap_or(start);
                    let last = next(first, rear(first)).unwrap_or(first);
                    return (first, last, walked == n);
                }
                match next(cur, v) {
                    Some(nx) => {
                        v = other(nx, v);
                        cur = nx;
                        walked += 1;
                        if walked > n {
                            return (start, start, false);
                        }
                    }
                    None => {
                        // The rear end of an open curve; then forwards from the first edge.
                        let first = cur;
                        let (mut cur, mut v) = (start, other(start, rear(start)));
                        while let Some(nx) = next(cur, v) {
                            v = other(nx, v);
                            cur = nx;
                            walked += 1;
                            if walked > n {
                                return (first, cur, false);
                            }
                        }
                        return (first, cur, walked == n);
                    }
                }
            }
        };
        let start = file_first.unwrap_or(list[0]);
        let (mut first, mut last, mut ok) = walk(start);
        // A walk that ends elsewhere than the file's last edge is walked again from that one:
        // it must come back to the first edge.
        if let Some(fl) = file_last.filter(|&fl| ok && fl != last) {
            (first, last, ok) = walk(fl);
            ok &= last == start;
        }
        if !ok {
            dissolved.insert(cid);
            continue;
        }
        // The ends the walk gives (what Check Validity looks at, `curves::check_arcs`).
        let ends = (edges[first].el.id, edges[last].el.id);
        let (p0, p1) = (rear(first), other(last, rear(last)));
        (c.first, c.last) = ends;
        let (Some(&q0), Some(&q1)) = (pos.get(&p0), pos.get(&p1)) else { continue };
        let Some(a) = c.arc.as_mut() else { continue };
        // Worked out again only without a last edge in the file.
        if recompute && file_last.is_none() {
            recompute_arc(a, q0, q1, p0 == p1, valence(p0));
        }
    }
    if !dissolved.is_empty() {
        e.curves.retain(|c| !dissolved.contains(&c.id));
        for x in &mut e.edges {
            if x.curve.is_some_and(|c| dissolved.contains(&c)) {
                x.curve = None;
            }
        }
    }
}

/// An arc's frame and angles worked out again from its end vertices (see
/// [`settle_curves`]). An elliptic arc (x and y of different lengths, or not square: a circle
/// scaled) is worked out in the frame where it is a unit circle — the one of its axes x, y and
/// normal, from its centre — then brought back (its plane by the inverse transpose).
fn recompute_arc(a: &mut Arc, q0: [f64; 3], q1: [f64; 3], closed: bool, valence: usize) {
    let (lx, ly) = (dot(a.xaxis, a.xaxis).sqrt(), dot(a.yaxis, a.yaxis).sqrt());
    let circular =
        (lx - ly).abs() < 1e-3 && unit(a.xaxis).zip(unit(a.yaxis)).is_some_and(|(x, y)| dot(x, y).abs() < 1e-3);
    let (x, y, n, c) = (a.xaxis, a.yaxis, a.normal, a.center);
    let det = dot(n, cross(x, y));
    if circular || det == 0.0 || !det.is_finite() {
        recompute_circle(a, q0, q1, closed, valence);
        return;
    }
    // The rows of the inverse of [x y n].
    let rows = [cross(y, n), cross(n, x), cross(x, y)].map(|r| r.map(|v| v / det));
    let to = |p: [f64; 3]| {
        let d = sub(p, c);
        rows.map(|r| dot(r, d))
    };
    let Some(normal) = unit([dot(x, n), dot(y, n), dot(n, n)]) else { return };
    let mut u = Arc {
        center: [0.0; 3],
        normal,
        xaxis: [1.0, 0.0, 0.0],
        yaxis: [0.0, 1.0, 0.0],
        radius: 1.0,
        start: a.start,
        end: a.end,
    };
    recompute_circle(&mut u, to(q0), to(q1), closed, valence);
    let back = |v: [f64; 3]| [0, 1, 2].map(|k| x[k] * v[0] + y[k] * v[1] + n[k] * v[2]);
    let (bx, by) = (back(u.xaxis), back(u.yaxis));
    let nb = [0, 1, 2].map(|k| rows[0][k] * u.normal[0] + rows[1][k] * u.normal[1] + rows[2][k] * u.normal[2]);
    let Some(mut nb) = unit(nb) else { return };
    if dot(nb, cross(bx, by)) < 0.0 {
        nb = nb.map(|v| -v);
    }
    (a.xaxis, a.yaxis, a.normal) = (bx, by, nb);
    a.radius = dot(bx, bx).sqrt();
    (a.start, a.end) = (u.start, u.end);
}

/// [`recompute_arc`] for a circle: the x axis turned to the first vertex (at the radius), the
/// normal made square to it, y = normal × x, start 0, end at the last vertex.
fn recompute_circle(a: &mut Arc, q0: [f64; 3], q1: [f64; 3], closed: bool, valence: usize) {
    let Some(dir) = unit(sub(q0, a.center)) else { return };
    let Some(normal) = unit(cross(a.normal, dir)).and_then(|s| unit(cross(dir, s))) else { return };
    a.xaxis = dir.map(|x| x * a.radius);
    a.normal = normal;
    a.radius = dot(a.xaxis, a.xaxis).sqrt();
    a.yaxis = cross(normal, a.xaxis);
    a.start = 0.0;
    a.end = if closed { if valence == 2 { 0.0 } else { std::f64::consts::TAU } } else { arc_param(a, q1) };
}

/// The names SketchUp gives on loading (`CAttributeNamed`, `CLayer`)
/// to the tags and dictionary keys of the Google Earth imports of old files.
fn google_earth_renamed(s: String) -> String {
    match s.as_str() {
        "Google Earth Snapshot" => "Location Snapshot".into(),
        "Google Earth Terrain" => "Location Terrain".into(),
        _ => s,
    }
}

pub(crate) fn guid_text(d: &[u8; 16]) -> String {
    let a = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
    let b = u16::from_le_bytes([d[4], d[5]]);
    let c = u16::from_le_bytes([d[6], d[7]]);
    format!(
        "{a:08x}-{b:04x}-{c:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        d[8], d[9], d[10], d[11], d[12], d[13], d[14], d[15]
    )
}

fn rgb(c: [u8; 4]) -> [u8; 3] {
    [c[0], c[1], c[2]]
}

/// The rendering options of a new `CRenderingOptions`, by the tags of 2021+
/// files. Colours made from fractions are truncated (0.65 × 255 → 165, as files show).
fn rendering_defaults() -> Vec<(u16, Value)> {
    let (t, f) = (json!(true), json!(false));
    vec![
        (0x733d, json!(2)),
        (0x733e, f.clone()),
        (0x733f, t.clone()),
        (0x7340, t.clone()),
        (0x7341, json!(1)),
        (0x7342, json!(0)),
        (0x7343, t.clone()),
        (0x7344, t.clone()),
        (0x7345, t.clone()),
        (0x7346, f.clone()),
        (0x7347, f.clone()),
        (0x7348, json!(1)),
        (0x7349, f.clone()),
        (0x734b, f.clone()),
        (0x734c, t.clone()),
        (0x734d, f.clone()),
        (0x734e, json!(8)),
        (0x734f, t.clone()),
        (0x7350, json!(3)),
        (0x7351, f.clone()),
        (0x7352, json!(4)),
        (0x7353, f.clone()),
        (0x7354, json!(5)),
        (0x7355, f.clone()),
        (0x7356, f.clone()),
        (0x7357, json!([0, 0, 0, 0])),
        (0x7358, json!([0, 0, 0, 0])),
        (0x7359, json!([255, 255, 0, 255])),
        (0x735a, json!([255, 0, 0, 255])),
        (0x735b, json!([128, 128, 128, 255])),
        (0x735c, json!([225, 225, 200, 255])),
        (0x735d, json!([128, 128, 200, 255])),
        (0x735e, t.clone()),
        (0x735f, f.clone()),
        (0x7360, json!([0, 0, 0, 0])),
        (0x7361, t.clone()),
        (0x7362, json!(-1.0)),
        (0x7363, json!(-1.0)),
        (0x7364, json!(1)),
        (0x7365, json!([140, 165, 255, 255])),
        (0x7366, json!([0, 0, 0, 0])),
        (0x7367, json!([63, 147, 63, 255])),
        (0x7368, f.clone()),
        (0x7369, f.clone()),
        (0x736a, t.clone()),
        (0x736b, json!(50)),
        (0x736c, json!(0.75)),
        (0x736d, json!(0.5)),
        (0x736e, f.clone()),
        (0x736f, f.clone()),
        (0x7370, json!([102, 110, 141, 255])),
        (0x7371, json!([192, 192, 192, 255])),
        (0x7372, json!([255, 0, 0, 255])),
        (0x7373, json!([0, 0, 0, 255])),
        (0x7374, json!(4)),
        (0x7375, json!(3)),
        (0x7376, f.clone()),
        (0x7377, json!(0)),
        (0x7380, f.clone()),
        (0x7381, f),
        (0x7382, t),
    ]
}

/// Largest image decoded (pixels).
const MAX_DECODED_PIXELS: u64 = 1 << 26;
/// Largest side of an image decoded.
const MAX_DECODED_SIDE: u32 = 1 << 14;

/// What SketchUp works out from an image: its average colour (the sum of
/// each channel over all pixels, divided by their number, truncated) and whether it has pixels
/// that are not opaque. The size is read from the header first: the pixels
/// are taken from the file's budget before anything is decoded. `None` when the image cannot
/// be decoded, is too large, or the budget is spent.
fn analyze_image(bytes: &[u8], budget: &super::Budget) -> Option<([u8; 3], bool)> {
    // JPEG: the pixels of SketchUp's own decoder (another rounds differently, §9).
    if super::jpeg::is_jpeg(bytes) {
        let (w, h) = super::jpeg::dimensions(bytes)?;
        let n = w as u64 * h as u64;
        if n == 0 || n > MAX_DECODED_PIXELS || w > MAX_DECODED_SIDE as usize || h > MAX_DECODED_SIDE as usize {
            return None;
        }
        if !budget.take_pixels(n) {
            return None;
        }
        return super::jpeg::average(bytes, 256 << 20).map(|avg| (avg, false));
    }
    let open = || image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok();
    let (w, h) = open()?.into_dimensions().ok()?;
    let n = w as u64 * h as u64;
    if n == 0 || n > MAX_DECODED_PIXELS || w > MAX_DECODED_SIDE || h > MAX_DECODED_SIDE || !budget.take_pixels(n) {
        return None;
    }
    let mut r = open()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(256 << 20);
    limits.max_image_width = Some(MAX_DECODED_SIDE);
    limits.max_image_height = Some(MAX_DECODED_SIDE);
    r.limits(limits);
    let img = r.decode().ok()?;
    let n = img.width() as u64 * img.height() as u64;
    if n == 0 {
        return None;
    }
    let mut sum = [0u64; 3];
    let mut transparent = false;
    if img.color().has_alpha() {
        for p in img.into_rgba8().pixels() {
            for (s, &c) in sum.iter_mut().zip(p.0.iter()) {
                *s += c as u64;
            }
            transparent |= p.0[3] != 255;
        }
    } else {
        for p in img.into_rgb8().pixels() {
            for (s, &c) in sum.iter_mut().zip(p.0.iter()) {
                *s += c as u64;
            }
        }
    }
    Some((sum.map(|s| (s as f64 / n as f64) as u8), transparent))
}

impl Reader<'_> {
    fn err(&self, what: &str) -> super::Error {
        self.ar.err(what)
    }

    fn step(&self, what: &str) {
        if self.trace {
            eprintln!("{:>9} {}[{what}]", self.ar.pos, "  ".repeat(self.ar.depth));
        }
    }

    /// The next object reference: null, an object already read, or a new object read in place.
    fn obj(&mut self) -> Result<Option<usize>> {
        match self.ar.read_ref()? {
            Next::Null => Ok(None),
            Next::Old(idx) => self.ar.object_at(idx).map(Some).ok_or_else(|| self.err("référence à un objet inconnu")),
            Next::New(class, idx) => {
                let slot = self.objs.len();
                if slot >= MAX_OBJECTS {
                    return Err(self.err("trop d'objets"));
                }
                // The depth, and the stack it takes (which depends on the build).
                let here = 0u8;
                let used = self.stack_base.abs_diff(std::hint::black_box(&here) as *const u8 as usize);
                if self.ar.depth >= MAX_DEPTH || used > STACK_LIMIT {
                    return Err(self.err("objets trop imbriqués"));
                }
                self.budget.charge((std::mem::size_of::<Obj>() + 9) as u64)?;
                self.objs.push(Obj::Pending);
                self.taken.push(false);
                self.step(&class);
                CENSUS.with_borrow_mut(|c| {
                    if let Some(c) = c {
                        *c.entry(class.clone()).or_default() += 1;
                    }
                });
                // MFC maps the object before reading it: it may refer to itself.
                self.ar.set_object(idx, slot);
                self.ar.depth += 1;
                let o = self.class(&class, slot)?;
                self.ar.depth -= 1;
                self.budget.charge(obj_cost(&o))?;
                self.objs[slot] = o;
                Ok(Some(slot))
            }
        }
    }

    /// Gives an object to an owner, the first time only. SketchUp adds an entity to a
    /// collection only while it has no parent (`CComponent::Serialize`, `plVar4[3] == 0`): an
    /// object that a forged file refers to again elsewhere is not added (nor copied) twice.
    /// The same holds here for every object that belongs to one other (dictionaries of an
    /// entity, loops of a face, folders, the items of the model's lists).
    fn take(&mut self, i: usize) -> bool {
        match self.taken.get_mut(i) {
            Some(t) if !*t => {
                *t = true;
                true
            }
            _ => false,
        }
    }

    /// Average colour and transparency of an image of the file (decoded once, within the
    /// file's budget).
    fn analyze(&mut self, img: &std::sync::Arc<Vec<u8>>) -> Option<([u8; 3], bool)> {
        let key = std::sync::Arc::as_ptr(img) as usize;
        if let Some(r) = self.decoded.get(&key) {
            return *r;
        }
        let r = analyze_image(img, &self.budget);
        self.decoded.insert(key, r);
        r
    }

    /// A list of objects that belong to it (see [`Reader::take`]): those the list does not
    /// own are left out.
    fn owned_list(&mut self) -> Result<Vec<usize>> {
        let mut l = self.obj_list()?;
        l.retain(|&i| self.take(i));
        Ok(l)
    }

    /// A list of `n` object references (`u32` count first).
    fn obj_list(&mut self) -> Result<Vec<usize>> {
        let n = self.ar.count(2)?;
        let mut out = Vec::new();
        for _ in 0..n {
            if let Some(o) = self.obj()? {
                out.push(o);
            }
        }
        Ok(out)
    }

    fn class(&mut self, class: &str, slot: usize) -> Result<Obj> {
        let id = slot as Id;
        Ok(match class {
            "CAttributeContainer" => self.container()?,
            "CAttributeNamed" => self.dict()?,
            "CFaceTextureCoords" => Obj::TexCoords(Box::new(self.tex_coords()?)),
            "CVertex" => {
                self.entity()?;
                Obj::Vertex(self.ar.v3()?)
            }
            "CEdge" => {
                let mut el = self.drawing_element()?;
                el.id = id;
                let start = self.obj()?.unwrap_or(0) as Id;
                let end = self.obj()?.unwrap_or(0) as Id;
                let curve = if matches!(self.v.edge, 1 | 2) { self.obj()?.map(|c| c as Id) } else { None };
                Obj::Edge(Box::new(Edge { el, start, end, curve }))
            }
            "CEdgeUse" => {
                self.entity()?;
                let edge = self.obj()?;
                let rev = self.ar.bool()?;
                self.obj()?;
                if self.v.edge_use == 0 {
                    self.obj()?;
                    self.obj()?;
                }
                Obj::EdgeUse(edge, rev)
            }
            "CLoop" => {
                self.entity()?;
                self.ar.bool()?;
                self.ar.bool()?;
                let mut uses = Vec::new();
                let first = self.obj()?;
                if let Some(f) = first {
                    uses.push(f);
                    if self.v.loop_ != 0 {
                        while let Some(u) = self.obj()? {
                            uses.push(u);
                        }
                    }
                }
                Obj::Loop(uses)
            }
            "CFace" => self.face(id)?,
            "CCurve" => Obj::Curve(Box::new(self.curve(id)?)),
            "CArcCurve" => {
                let mut c = self.curve(id)?;
                c.arc = Some(self.arc(self.v.arc != 0)?);
                Obj::Curve(Box::new(c))
            }
            "CComponentDefinition" => self.definition(id)?,
            "CComponentInstance" => self.instance(id, InstKind::Component)?,
            "CGroup" => self.instance(id, InstKind::Group)?,
            "CImage" => self.instance(id, InstKind::Image)?,
            "CMaterial" => Obj::Material(Box::new(self.material(id)?)),
            "CTexture" => Obj::Texture(Box::new(self.texture()?)),
            "CDib" => Obj::Image(self.dib()?),
            "CLayer" => Obj::Layer(Box::new(self.layer(id)?)),
            "CLayerGroup" => Obj::Folder(Box::new(self.layer_group()?)),
            "CConstructionLine" => self.construction_line(id)?,
            "CConstructionPoint" => {
                let mut el = self.drawing_element()?;
                el.id = id;
                let pos = self.ar.v3()?;
                self.ar.v3()?;
                self.ar.bool()?;
                Obj::GuidePoint(Box::new(GuidePoint { el, pos }))
            }
            "CSectionPlane" => {
                let mut el = self.drawing_element()?;
                el.id = id;
                let plane = self.ar.v4()?;
                let (name, symbol) = if self.v.section >= 3 {
                    (self.ar.string()?, self.ar.string()?)
                } else {
                    // Numbered once the model is read (`name_sections`).
                    self.unnamed_sections += 1;
                    (UNNAMED.to_string(), String::new())
                };
                Obj::Section(Box::new(Section { el, plane, name, symbol, hatch: None }))
            }
            "CText" => self.text(id)?,
            "CDimensionLinear" => self.dim_linear(id)?,
            "CDimensionRadial" => {
                // the dimension, the edge of its arc, the angle of its point,
                // the leader's offset, diameter, and without an edge the arc itself.
                self.dims += 1;
                let (mut el, text, arrow_type, font) = self.dimension()?;
                el.id = id;
                let edge = self.obj()?;
                let mut d = DimRadial { el, text, arrow_type, font, edge: edge.map(|e| e as Id), ..Default::default() };
                d.angle = self.ar.f64()?;
                d.leader = self.ar.f64()?;
                d.diameter = self.ar.bool()?;
                if edge.is_none() {
                    d.arc = Some(self.arc(self.v.dim_radial > 1)?);
                }
                Obj::Radial(Box::new(d))
            }
            "CPolyline3d" => {
                let mut el = self.drawing_element()?;
                el.id = id;
                let n = self.ar.count(24)?;
                self.budget.charge(n as u64 * 24)?;
                let mut points = Vec::with_capacity(n);
                for _ in 0..n {
                    let p = self.ar.v3()?;
                    if p.iter().all(|x| x.is_finite()) {
                        points.push(p);
                    }
                }
                Obj::Polyline(Box::new(Polyline { el, points }))
            }
            "CCamera" => Obj::Camera(Box::new(self.camera()?)),
            "CViewPage" => self.view_page(id)?,
            "CThumbnail" => {
                // Its camera and its image (`CDib`).
                self.entity()?;
                let camera = self.camera_ref(self.v.thumbnail == 0)?;
                let image = self.obj()?.and_then(|i| match self.objs.get(i) {
                    Some(Obj::Image(b)) if !b.is_empty() => Some(b.clone()),
                    _ => None,
                });
                Obj::Thumb(Box::new(Thumbnail { camera, image }))
            }
            "CSkFont" => {
                self.entity()?;
                // Without a world size in the file: 10 (`CSkFont` constructor).
                let mut f = Font { id, height: 10.0, ..Default::default() };
                f.face = self.ar.string()?;
                // Italic first (CSkFont::Serialize,: +0x49 then +0x48, bold).
                f.italic = self.ar.bool()?;
                f.bold = self.ar.bool()?;
                f.size = self.ar.u32()?;
                if self.v.skfont != 0 {
                    f.world = self.ar.bool()?;
                    f.height = self.ar.f64()?;
                }
                Obj::Font(Box::new(f))
            }
            "CCustomLineStyle" => self.line_style()?,
            "CSkpStyle" => Obj::Style(Box::new(self.skp_style(id)?)),
            "CWatermark" => Obj::Watermark(Box::new(self.watermark(id)?)),
            "CBackgroundImage" => Obj::Photo(Box::new(self.background(id)?)),
            // CEntity, what a component is glued to, the component.
            "CRelationship" => {
                self.entity()?;
                let to = self.obj()?;
                let what = self.obj()?;
                Obj::Relation(to, what)
            }
            "CTextStyle" => {
                self.text_style()?;
                Obj::Other
            }
            "CDimensionStyle" => {
                self.dim_style()?;
                Obj::Other
            }
            "CSchemaFile" => {
                self.schema_file()?;
                Obj::Other
            }
            "CSchemaZipFile" => {
                // two strings, i32 n, the n bytes of the `.skc`.
                let a = self.ar.string()?;
                let b = self.ar.string()?;
                let n = self.ar.icount(1)?;
                self.budget.charge(n as u64)?;
                let data = self.ar.bytes(n)?.to_vec();
                Obj::Schema(Box::new(super::doc::SchemaFile { path: a, name: b, data: std::sync::Arc::new(data) }))
            }
            "CSchemaFilterFile" => {
                self.ar.string()?;
                let n = self.ar.icount(1)?;
                self.ar.skip(n)?;
                Obj::Other
            }
            _ => return Err(self.err(&format!("classe {class} non lue"))),
        })
    }

    // --- Entities -------------------------------------------------------------------------

    /// `CEntity`: dictionaries, persistent id.
    fn entity(&mut self) -> Result<Ent> {
        let mut e = Ent::default();
        match self.v.entity {
            1 => self.ar.skip(4)?,
            0 | 2 => {}
            s => {
                e.attrs = self.obj()?;
                if s == 4 {
                    self.ar.skip(8)?;
                } else if s >= 5 {
                    self.ar.compact_id()?;
                }
            }
        }
        Ok(e)
    }

    /// The dictionaries and texture placement of an entity.
    /// The dictionaries and texture placement of an entity, from its container (given to the
    /// first entity that refers to it).
    fn ent_attrs(&mut self, e: Ent) -> Result<(Attributes, Placements)> {
        let Some(i) = e.attrs else { return Ok((Attributes::new(), [None, None])) };
        if !self.take(i) {
            return Ok((Attributes::new(), [None, None]));
        }
        // Moved, not copied: the container is this entity's alone (`take`).
        match self.objs.get_mut(i) {
            Some(Obj::Container(c)) => Ok((std::mem::take(&mut c.0), std::mem::take(&mut c.1))),
            _ => Ok((Attributes::new(), [None, None])),
        }
    }

    /// `CDrawingElement`: entity, material, flags, tag.
    fn drawing_element(&mut self) -> Result<El> {
        let e = self.entity()?;
        let (attrs, tex) = self.ent_attrs(e)?;
        let mut el = El { flags: flags::CASTS_SHADOWS | flags::RECEIVES_SHADOWS, attrs, tex, ..Default::default() };
        let s = self.v.de;
        let flag = |el: &mut El, ar: &mut Ar, bit: u32| -> Result<()> {
            if ar.bool()? {
                el.flags |= bit;
            } else {
                el.flags &= !bit;
            }
            Ok(())
        };
        let mut read_flags = s != 0;
        if s != 2 {
            if s > 4 {
                el.material = self.obj()?.map(|m| m as Id);
            } else {
                // Old files give a colour; SketchUp makes a material of it.
                self.ar.color()?;
            }
        }
        if s == 0 {
            read_flags = false;
        }
        if read_flags {
            flag(&mut el, &mut self.ar, flags::HIDDEN)?;
            if s >= 6 {
                flag(&mut el, &mut self.ar, flags::CASTS_SHADOWS)?;
                flag(&mut el, &mut self.ar, flags::RECEIVES_SHADOWS)?;
                if s >= 7 {
                    flag(&mut el, &mut self.ar, flags::SOFT)?;
                    if s >= 8 {
                        flag(&mut el, &mut self.ar, flags::SMOOTH)?;
                        if s > 8 {
                            flag(&mut el, &mut self.ar, flags::LOCKED)?;
                        }
                        el.layer = self.obj()?.map(|l| l as Id);
                        return Ok(el);
                    }
                }
            }
        }
        // Before schema 8 the smooth flag is the soft one.
        if el.flags & flags::SOFT != 0 {
            el.flags |= flags::SMOOTH;
        } else {
            el.flags &= !flags::SMOOTH;
        }
        if s == 2 {
            if self.ar.bool()? {
                self.obj()?;
            }
            return Ok(el);
        }
        if s >= 4 {
            el.layer = self.obj()?.map(|l| l as Id);
        }
        Ok(el)
    }

    fn face(&mut self, id: Id) -> Result<Obj> {
        let mut el = self.drawing_element()?;
        el.id = id;
        let plane = self.ar.v4()?;
        let n = self.ar.u32()?;
        if n > 0xffff || n as usize * 2 > self.ar.left() {
            return Err(self.err("face : trop de boucles"));
        }
        let mut loops = Vec::new();
        for _ in 0..n {
            match self.obj()? {
                Some(l) => {
                    // A loop belongs to one face.
                    if self.take(l) {
                        loops.push(l);
                    }
                }
                None => return Err(self.err("face : boucle nulle")),
            }
        }
        let s = self.v.face;
        let mut back = el.material;
        if s == 1 {
            if self.ar.u32()? != 0 {
                el.material = self.obj()?.map(|m| m as Id);
                back = el.material;
            }
        } else if s > 2 {
            back = self.obj()?.map(|m| m as Id);
        }
        // Files before 7.1: a side painted with a texture or a transparent material paints the
        // unpainted other side.
        if self.file.0 < 7 || (self.file.0 == 7 && self.file.1 < 1) {
            if back.is_none() && self.see_through(el.material) {
                back = el.material;
            } else if el.material.is_none() && self.see_through(back) {
                el.material = back;
            }
        }
        Ok(Obj::Face(Box::new((el, plane, loops, back.map(|b| b as usize)))))
    }

    /// A material one sees through: a texture with transparent pixels,
    /// or a transparency.
    fn see_through(&mut self, m: Option<Id>) -> bool {
        let Some(m) = m.map(|m| m as usize) else { return false };
        if let Some(&b) = self.see_through_cache.get(&m) {
            return b;
        }
        let (transparency, image) = match self.objs.get(m) {
            Some(Obj::Material(x)) => {
                (x.use_opacity && 1.0 - x.opacity >= 1e-6, x.texture.as_ref().map(|t| t.image.clone()))
            }
            _ => (false, None),
        };
        let b = transparency || image.is_some_and(|i| self.analyze(&i).is_some_and(|(_, t)| t));
        self.see_through_cache.insert(m, b);
        b
    }

    fn curve(&mut self, id: Id) -> Result<Curve> {
        self.entity()?;
        let mut c = Curve { id, ..Default::default() };
        match self.v.curve {
            4 => {
                c.kind = self.ar.bool()? as u32;
                c.segments = self.ar.u32()?;
            }
            3 => {
                // The first and last edges (SketchUp 3): the walk starts there (`settle_curves`).
                c.first = self.obj()?.map_or(0, |i| i as Id);
                c.last = self.obj()?.map_or(0, |i| i as Id);
                c.kind = self.ar.bool()? as u32;
                c.segments = self.ar.u32()?;
            }
            2 => c.kind = self.ar.bool()? as u32,
            0 => {
                self.obj()?;
                self.obj()?;
            }
            _ => {}
        }
        Ok(c)
    }

    /// An arc: centre, normal, x axis (scaled by the radius), start and end angles, and the y
    /// axis since schema 1.
    fn arc(&mut self, with_y: bool) -> Result<Arc> {
        let center = self.ar.v3()?;
        let normal = self.ar.v3()?;
        let xaxis = self.ar.v3()?;
        let start = self.ar.f64()?;
        let end = self.ar.f64()?;
        let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let cross = |a: [f64; 3], b: [f64; 3]| {
            [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
        };
        let radius = len(xaxis);
        let n = len(normal);
        let mut normal = if n > 0.0 { normal.map(|x| x / n) } else { normal };
        let yaxis = if with_y {
            self.ar.v3()?
        } else {
            let c = cross(normal, xaxis);
            let l = len(c);
            if l > 0.0 { c.map(|x| x * radius / l) } else { c }
        };
        let c = cross(xaxis, yaxis);
        if c[0] * normal[0] + c[1] * normal[1] + c[2] * normal[2] < 0.0 {
            normal = normal.map(|x| -x);
        }
        // A full circle's end is decided once its edges are known (`ents`).
        Ok(Arc { center, normal, xaxis, yaxis, radius, start, end })
    }

    /// `CComponent`: the entities of the model or of a definition, with (for the model) its
    /// materials, tags and definitions.
    fn component(&mut self, root: bool) -> Result<RawComp> {
        self.component_dicts = true;
        let el = self.drawing_element();
        self.component_dicts = false;
        let el = el?;
        let mut c = RawComp { el, ..Default::default() };
        let s = self.v.component;
        let was_root = std::mem::replace(&mut self.root, root);
        if s >= 9 {
            self.material_manager()?;
            self.layer_manager()?;
            self.definition_list()?;
            c.items = self.owned_list()?;
            c.relations = self.relationship_map()?;
            c.active_section = self.obj()?;
            if s == 10 {
                self.font_manager(false)?;
            }
        } else {
            c.items = self.owned_list()?;
            if s < 7 {
                self.ar.f64s::<13>()?;
            }
            if s != 0 {
                c.relations = self.relationship_map()?;
            }
            if s > 1 {
                self.definition_list()?;
            }
            if (3..6).contains(&s) && self.ar.u32()? != 0 {
                self.obj()?;
            }
            if s > 3 {
                self.material_manager()?;
            }
            if s > 4 {
                self.layer_manager()?;
            }
            if s > 7 {
                c.active_section = self.obj()?;
            }
        }
        self.root = was_root;
        Ok(c)
    }

    fn material_manager(&mut self) -> Result<()> {
        let s = self.v.material_mgr;
        if s != 0 {
            self.entity()?;
        }
        let list = self.obj_list()?;
        if s > 1 {
            self.obj()?;
            if s == 3 {
                self.obj()?;
            }
        }
        if self.root {
            self.materials = list;
        }
        Ok(())
    }

    fn layer_manager(&mut self) -> Result<()> {
        let s = self.v.layer_mgr;
        if s != 0 {
            self.entity()?;
        }
        let n = self.ar.i32()?;
        if n < 0 || n as usize * 2 > self.ar.left() {
            return Err(self.err("nombre de balises démesuré"));
        }
        let mut list = Vec::new();
        for _ in 0..n {
            match self.obj()? {
                Some(l) => list.push(l),
                None => return Err(self.err("balise nulle")),
            }
        }
        let mut active = None;
        let mut folder = None;
        if s < 3 {
            let name = self.ar.string()?;
            active = list.iter().copied().find(|&l| matches!(&self.objs[l], Obj::Layer(x) if x.name == name));
        } else {
            if s == 3 || n > 0 {
                active = self.obj()?;
            }
            if s == 6 {
                self.obj_list()?;
            } else if s >= 7 {
                folder = Some(self.layer_group()?);
            }
        }
        if self.root {
            self.layers = list;
            self.active_layer = active;
            self.root_folder = folder;
        }
        Ok(())
    }

    fn definition_list(&mut self) -> Result<()> {
        let list = self.obj_list()?;
        if self.root {
            self.defs = list;
        }
        Ok(())
    }

    /// `CRelationshipMap`: u32 n + n obj `CRelationship`.
    fn relationship_map(&mut self) -> Result<Vec<usize>> {
        self.obj_list()
    }

    /// `CFontManager` (in place): CEntity, the fonts (the model's: `model`).
    fn font_manager(&mut self, model: bool) -> Result<()> {
        self.entity()?;
        let list = self.obj_list()?;
        if model {
            self.font_list = list.into_iter().filter(|&i| matches!(self.objs.get(i), Some(Obj::Font(_)))).collect();
        }
        Ok(())
    }

    fn behavior(&mut self) -> Result<Behavior> {
        self.entity()?;
        let s = self.v.behavior;
        let mut b = Behavior::default();
        let glue;
        if s < 3 {
            glue = true;
            b.cuts_opening = self.ar.bool()?;
            if s < 2 {
                return Ok(b);
            }
        } else {
            glue = self.ar.bool()?;
            b.cuts_opening = self.ar.bool()?;
        }
        let to = self.ar.i32()?;
        b.snap = if glue { (to + 1).clamp(1, 4) } else { 0 };
        if s > 3 {
            let f = self.ar.u8()?;
            b.always_face_camera = f & 1 != 0;
            if self.file.0 >= 5 && b.always_face_camera {
                b.shadows_face_sun =
                    if self.file.0 == 5 && self.file.1 == 0 && self.file.2 < 0x97 { true } else { f & 2 != 0 };
            }
            if s >= 5 {
                b.no_scale_mask = self.ar.i32()?;
            }
        }
        Ok(b)
    }

    fn definition(&mut self, id: Id) -> Result<Obj> {
        let comp = self.component(false)?;
        let mut d = Def { id, attrs: comp.el.attrs.clone(), ..Default::default() };
        let g = self.ar.bytes(16)?;
        d.guid = guid_text(g.try_into().map_err(|_| self.err("GUID"))?);
        let s = self.v.definition;
        if s > 2 {
            d.name = self.ar.string()?;
            d.description = self.ar.string()?;
        }
        // The file it was loaded from (before schema 4, a list of names: the first) and when.
        if s < 4 {
            let n = self.ar.count(1)?;
            for k in 0..n {
                let name = self.ar.string()?;
                if k == 0 {
                    d.path = name;
                }
            }
        } else {
            d.path = self.ar.string()?;
        }
        d.time = self.ar.u32()?;
        if s > 8 {
            self.ar.bool()?;
        }
        if s > 4 {
            self.ar.v3()?;
        }
        if s < 6 {
            self.obj_list()?;
        }
        if s > 1 {
            d.behavior = self.behavior()?;
        }
        if s > 7 {
            let kind = self.ar.u32()?;
            (d.is_group, d.is_image) = (kind == 1, kind == 2);
        }
        if s > 9
            && let Some(i) = self.obj()?
            && let Some(Obj::Thumb(t)) = self.objs.get(i)
        {
            d.thumbnail = Some((**t).clone());
        }
        // Files before 2018 (schema < 11): SketchUp gives components the empty dictionary of its
        // advanced attributes.
        if s < 11 && !d.is_group && !d.is_image {
            let set = d.attrs.entry("SU_DefinitionSet".into()).or_default();
            for k in ["Price", "Size", "Url"] {
                set.entry(k.into()).or_insert(AttrValue::Text(String::new()));
            }
        }
        Ok(Obj::Def(Box::new(RawDef { def: d, comp })))
    }

    fn instance(&mut self, id: Id, kind: InstKind) -> Result<Obj> {
        let mut el = self.drawing_element()?;
        el.id = id;
        let def = self.obj()?.unwrap_or(0) as Id;
        let xform = self.ar.xform()?;
        let s = self.v.instance;
        if s < 2 {
            self.ar.skip(4)?;
        }
        if (1..=2).contains(&s) && self.ar.u32()? != 0 {
            self.obj()?;
        }
        let mut name = String::new();
        if s > 3 {
            name = self.ar.string()?;
        }
        let mut guid = String::new();
        if s > 4 {
            let g = self.ar.bytes(16)?;
            guid = guid_text(g.try_into().map_err(|_| self.err("GUID"))?);
        }
        let locked = el.flags & flags::LOCKED != 0;
        // The instances of a component get the empty dictionary of the advanced attributes
        // when they have none (`CComponentInstance::Serialize`).
        if let Some(Obj::Def(d)) = self.objs.get(def as usize)
            && !d.def.is_group
            && !d.def.is_image
        {
            let set = el.attrs.entry("SU_InstanceSet".into()).or_default();
            for k in ["Owner", "Status"] {
                set.entry(k.into()).or_insert(AttrValue::Text(String::new()));
            }
        }
        Ok(Obj::Inst(Box::new(Inst { el, name, xform, def, locked, hatch: None, guid }), kind))
    }

    fn construction_line(&mut self, id: Id) -> Result<Obj> {
        let mut el = self.drawing_element()?;
        el.id = id;
        let p = self.ar.v3()?;
        let d = self.ar.v3()?;
        // The line as SketchUp keeps it (an annotation can be attached to a point of it).
        self.guide_raw.insert(id, (p, d));
        let mut g = GuideLine { el, ..Default::default() };
        if self.v.cline < 1 {
            (g.start, g.dir, g.infinite) = (p, d, true);
        } else {
            let (t0, t1) = (self.ar.f64()?, self.ar.f64()?);
            self.ar.u32()?;
            g.infinite = t0 <= -1e29 && t1 >= 1e29;
            if g.infinite {
                (g.start, g.dir) = (p, d);
            } else {
                let at = |t: f64| [p[0] + d[0] * t, p[1] + d[1] * t, p[2] + d[2] * t];
                let (a, b) = (at(t0), at(t1));
                (g.start, g.dir) = (a, [b[0] - a[0], b[1] - a[1], b[2] - a[2]]);
            }
        }
        Ok(Obj::GuideLine(Box::new(g)))
    }

    /// The point an annotation is attached to: its type, the point, the
    /// entity it is attached to and the path of instances to it (a second entity and path for
    /// an intersection, schema ≥ 4). Returns the type and the point where SketchUp places it
    /// (`attach::position`).
    fn connection(&mut self) -> Result<(i32, [f64; 3], Option<super::doc::Connection>)> {
        let kind = self.ar.i32()?;
        let ver = self.ar.i32()?;
        let point = self.ar.v3()?;
        let leaf = self.obj()?;
        let leaf2 = if ver > 0 { self.obj()? } else { None };
        let path = self.obj_list()?;
        let path2 = if ver >= 4 { self.obj_list()? } else { Vec::new() };
        let a = Attach { kind, point, leaf, path, leaf2, path2 };
        Ok((kind, super::attach::position(&*self, &a), a.connection(|k| k as Id)))
    }

    fn text(&mut self, id: Id) -> Result<Obj> {
        let mut el = self.drawing_element()?;
        el.id = id;
        let mut t = Text { el, ..Default::default() };
        let s = self.v.text;
        if s == 0 {
            return Ok(Obj::Text(Box::new(t)));
        }
        if s > 3 {
            t.font = self.font_obj()?;
            self.annot_fonts.extend(t.font.map(|f| f as usize));
        }
        if s < 5 {
            self.obj()?;
        }
        t.screen = [self.ar.f64()?, self.ar.f64()?];
        let mut point = None;
        if s < 5 {
            point = Some(self.ar.v3()?);
        } else {
            let (kind, p, c) = self.connection()?;
            t.connection = c;
            if kind != 0 {
                point = Some(p);
            }
        }
        if s < 6 {
            let v = self.ar.v3()?;
            if s > 4 {
                t.leader = self.ar.v3()?;
            } else {
                self.ar.v3()?;
                t.leader = v;
            }
        } else {
            t.leader = self.ar.v3()?;
        }
        self.ar.v3()?;
        if (3..=7).contains(&s) {
            self.ar.f64()?;
        }
        t.leader_type = self.ar.i32()?;
        self.ar.i32()?;
        self.ar.bool()?;
        self.ar.bool()?;
        t.arrow_type = self.ar.i32()?;
        if s > 1 {
            self.ar.bool()?;
        }
        t.text = self.ar.string()?;
        if s > 7 {
            self.ar.bool()?;
        }
        if s > 8 {
            self.ar.i32()?;
        }
        t.point = point;
        Ok(Obj::Text(Box::new(t)))
    }

    /// `CDimension`: drawing element, text, style, arrows.
    fn dimension(&mut self) -> Result<(El, String, i32, Option<Id>)> {
        let el = self.drawing_element()?;
        let text = self.ar.string()?;
        let (mut arrow, mut font) = (0, None);
        if self.v.dimension > 0 {
            font = self.font_obj()?;
            self.annot_fonts.extend(font.map(|f| f as usize));
            self.ar.bool()?;
            arrow = self.ar.i32()?;
        }
        Ok((el, text, arrow, font))
    }

    fn dim_linear(&mut self, id: Id) -> Result<Obj> {
        self.dims += 1;
        let s = self.v.dim_linear;
        let mut d = DimLinear::default();
        if s == 0 {
            self.obj()?;
            self.obj()?;
        } else {
            if s >= 2 {
                let (el, text, arrow, font) = self.dimension()?;
                (d.el, d.text, d.arrow_type, d.font) = (el, text, arrow, font);
            }
            let (_, p, c) = self.connection()?;
            (d.start, d.connections[0]) = (p, c);
            let (_, p, c) = self.connection()?;
            (d.end, d.connections[1]) = (p, c);
        }
        d.el.id = id;
        d.normal = self.ar.v3()?;
        if s < 4 {
            self.ar.f64()?;
        } else {
            d.xaxis = self.ar.v3()?;
        }
        self.ar.u32()?;
        d.position[1] = self.ar.f64()?;
        if s >= 3 {
            let x = self.ar.f64()?;
            if s >= 5 {
                d.position[0] = x;
            }
            if s > 5 {
                self.ar.u32()?;
            }
        }
        Ok(Obj::Dim(Box::new(d)))
    }

    // --- Dictionaries -----------------------------------------------------------------------

    fn container(&mut self) -> Result<Obj> {
        // A component's (its own, not those of the dictionaries in it).
        let component = std::mem::take(&mut self.component_dicts);
        self.entity()?;
        let mut attrs = Attributes::new();
        let mut tex = [None, None];
        let mut seen_tex = false;
        let mut items = Vec::new();
        while let Some(o) = self.obj()? {
            items.push(o);
            if items.len() > 1_000_000 {
                return Err(self.err("trop de dictionnaires"));
            }
        }
        // `CComponent::Serialize` keeps the last « 3DWarehouse » and the last
        // « TempShadowInfo » dictionary of a component and deletes the others.
        if component {
            let mut kept = HashSet::new();
            let objs = &self.objs;
            let mut drop = HashSet::new();
            for &it in items.iter().rev() {
                if let Some(Obj::Dict(d)) = objs.get(it)
                    && (d.0 == "3DWarehouse" || d.0 == "TempShadowInfo")
                    && !kept.insert(d.0.clone())
                {
                    drop.insert(it);
                }
            }
            items.retain(|it| !drop.contains(it));
        }
        let is_dict = |objs: &[Obj], i: usize| matches!(objs[i], Obj::Dict(_));
        let dicts = items.iter().filter(|&&i| is_dict(&self.objs, i)).count();
        // See `vff::attribute_container` (`SUEntityGetAttributeDictionaries`): only the
        // dictionaries among the first `dicts` items are listed.
        let listed = items.iter().take(dicts).filter(|&&i| is_dict(&self.objs, i)).count();
        let mut cost = 0;
        for (i, &it) in items.iter().enumerate() {
            // An item belongs to one container.
            if !self.take(it) {
                continue;
            }
            let quirks = self.sdk_quirks();
            match &mut self.objs[it] {
                Obj::TexCoords(t) if !seen_tex => {
                    seen_tex = true;
                    tex = (**t).clone();
                    cost += 2 * std::mem::size_of::<TexCoords>() as u64;
                }
                Obj::Dict(d) => {
                    if quirks && i >= listed {
                        continue;
                    }
                    // Moved, not copied: the item is this container's alone (`take`), its
                    // values were counted when read.
                    let values = std::mem::take(&mut d.1);
                    let nested = std::mem::take(&mut d.3);
                    if !values.is_empty() || !nested.is_empty() {
                        attrs.entry(d.0.clone()).or_default().extend(values);
                    }
                    for (k, v) in nested {
                        attrs.entry(format!("{}{}{k}", d.0, super::doc::NEST)).or_default().extend(v);
                    }
                }
                _ => {}
            }
        }
        // The copies of the texture placements.
        self.budget.charge(cost)?;
        Ok(Obj::Container(Box::new((attrs, tex, cost))))
    }

    fn sdk_quirks(&self) -> bool {
        self.sdk_quirks
    }

    /// `CAttributeNamed`: a dictionary.
    fn dict(&mut self) -> Result<Obj> {
        // Its own dictionaries (its `CEntity`'s container): nested ones, which the SDK does not
        // list (`sdk_quirks`).
        let e = self.entity()?;
        let nested = if self.sdk_quirks { Attributes::new() } else { self.ent_attrs(e)?.0 };
        self.ar.skip(4)?;
        let name = google_earth_renamed(self.ar.string()?).replace(super::doc::NEST, "");
        let mut values = BTreeMap::new();
        loop {
            let key = google_earth_renamed(self.ar.string()?);
            if key.is_empty() {
                if self.v.attr_named > 0 {
                    self.ar.skip(4)?;
                }
                break;
            }
            if let Some(v) = self.variant(0)? {
                // An empty value is not listed by the SDK, nor (`sdk_quirks`) an empty array.
                let empty_array = self.sdk_quirks && v == json!({"t": "a", "v": []});
                if v != json!({"t": "n"}) && !empty_array {
                    values.insert(key, super::vff::attr_value(v));
                }
            }
        }
        let cost =
            48 + name.len() as u64 + values.iter().map(|(k, v)| 32 + k.len() as u64 + value_cost(v)).sum::<u64>();
        Ok(Obj::Dict(Box::new((name, values, cost, nested))))
    }

    /// A typed value, in the Ruby bridge's tagged JSON (as
    /// `vff::typed_value`); `None` for a type Esquisse does not keep.
    fn variant(&mut self, depth: usize) -> Result<Option<Value>> {
        if depth > 64 {
            return Err(self.err("valeurs trop imbriquées"));
        }
        // A value of one byte makes a node of JSON of some hundred bytes.
        self.budget.charge(VALUE_NODE_COST)?;
        let t = self.ar.u8()?;
        Ok(Some(match t {
            0 | 1 => json!({"t": "n"}),
            2 => json!({"t": "i", "v": self.ar.u8()? as i64}),
            3 => json!({"t": "i", "v": self.ar.u16()? as i16 as i64}),
            4 => json!({"t": "i", "v": self.ar.i32()? as i64}),
            5 => json!({"t": "f", "v": self.ar.f32()? as f64}),
            6 | 0xc..=0x10 => {
                let x = self.ar.f64()?;
                if x.is_finite() {
                    json!({"t": "f", "v": x})
                } else {
                    json!({"t": "nf", "v": if x.is_nan() { "nan" } else if x > 0.0 { "inf" } else { "-inf" }})
                }
            }
            7 => json!({"t": "b", "v": self.ar.bool()?}),
            8 => {
                let c = self.ar.color()?;
                json!({"t": "c", "v": [c[0], c[1], c[2], 255]})
            }
            9 => json!({"t": "time", "v": [self.ar.i32()? as i64, 0]}),
            10 => json!({"t": "s", "v": self.ar.string()?}),
            11 => {
                let n = self.ar.icount(1)?;
                let mut items = Vec::new();
                for _ in 0..n {
                    if let Some(v) = self.variant(depth + 1)? {
                        items.push(v);
                    }
                }
                json!({"t": "a", "v": items})
            }
            // A point: a vector for the SDK, which has no type for it.
            0x11 => json!({"t": if self.sdk_quirks { "vec" } else { "pt" }, "v": self.ar.v3()?}),
            0x12 | 0x13 => json!({"t": "vec", "v": self.ar.v3()?}),
            0x14 => {
                self.ar.f64s::<13>()?;
                return Ok(None);
            }
            _ => return Err(self.err(&format!("valeur de type {t} inconnue"))),
        }))
    }

    /// `CFaceTextureCoords`: the texture placement of both sides of a face.
    fn tex_coords(&mut self) -> Result<Placements> {
        self.entity()?;
        self.ar.skip(4)?;
        let s = self.v.tex_coords;
        // Written by columns, as in 2021+ files (whose reader takes them in file order).
        let matrix = |ar: &mut Ar| ar.f64s::<9>();
        let mut sides = [TexCoords::default(), TexCoords::default()];
        sides[0].matrix = matrix(&mut self.ar)?;
        if s != 0 {
            sides[0].projection = Some(self.ar.v3()?);
        }
        if s > 1 {
            sides[1].matrix = matrix(&mut self.ar)?;
            sides[1].projection = Some(self.ar.v3()?);
        }
        if s > 2 {
            for _ in 0..2 {
                let n = self.ar.icount(32)?;
                self.ar.skip(n * 32)?;
            }
        }
        let identity = |m: &[f64; 9]| *m == [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mut fl = [0u32; 2];
        if s < 4 {
            for (f, side) in fl.iter_mut().zip(&sides) {
                *f = if identity(&side.matrix) { 0 } else { 1 };
            }
        } else {
            fl = [self.ar.u32()?, self.ar.u32()?];
        }
        let mut out = [None, None];
        for (i, side) in sides.into_iter().enumerate() {
            out[i] = Some(Box::new(TexCoords { positioned: fl[i] & 1 != 0, projected: fl[i] & 2 != 0, ..side }));
        }
        Ok(out)
    }

    // --- Materials and tags ---------------------------------------------------------------

    fn material(&mut self, id: Id) -> Result<Material> {
        let e = self.entity()?;
        let s = self.v.material;
        let mut m = Material { id, opacity: 1.0, ..Default::default() };
        m.attrs = self.ent_attrs(e)?.0;
        m.name = self.ar.string()?;
        if s < 3 && self.ar.bool()? {
            self.ar.skip(4)?;
        }
        if self.ar.bool()? {
            if s < 4 {
                if let Some(t) = self.obj()?
                    && let Obj::Texture(x) = &self.objs[t]
                {
                    self.budget.charge(obj_cost(&self.objs[t]))?;
                    m.texture = Some((**x).clone());
                }
            } else {
                m.texture = Some(self.texture()?);
            }
        }
        if (2..=4).contains(&s) {
            self.ar.skip(4)?;
        }
        if s > 5 {
            m.owned = self.ar.bool()?;
        }
        let mut color = [0u8; 4];
        if s > 6 {
            color = self.ar.color()?;
        }
        if s > 7 {
            self.ar.string()?;
        }
        m.kind = 1;
        // The file keeps the transparency (1 − opacity).
        let mut transparency = 0.0;
        if s >= 9 {
            m.kind = self.ar.i32()?;
            if s >= 10 {
                m.colorize_type = self.ar.i32()?;
                if s >= 11 {
                    transparency = self.ar.f64()?;
                    m.use_opacity = if s >= 12 { self.ar.bool()? } else { transparency.abs() >= 1e-6 };
                }
            }
        }
        m.opacity = 1.0 - transparency;
        if m.texture.is_none() {
            m.kind = 0;
        }
        if let Some(mut t) = m.texture.take() {
            // Before schema 13 the average of a colorized texture is recomputed from its image
            // (the file keeps the colour it is tinted with).
            if m.kind == 2
                && s < 13
                && let Some((avg, _)) = self.analyze(&t.image)
            {
                t.avg = avg;
            }
            if m.kind == 1 && (s < 12 || color == [0; 4]) {
                color = [t.avg[0], t.avg[1], t.avg[2], 255];
            }
            m.texture = Some(t);
        }
        // No colour: SketchUp's default.
        if color == [0; 4] {
            color = [64, 64, 64, 255];
        }
        m.color = rgb(color);
        // `Material#color` gives the alpha byte as stored (0 in files of older SketchUps).
        m.color_alpha = (color[3] != 255).then_some(color[3]);
        // The SDK's values for files without physically based settings.
        m.pbr = Pbr {
            metallic_factor: 1.0,
            roughness_factor: 1.0,
            normal_scale: 1.0,
            normal_style: 1,
            ao_strength: 1.0,
            ..Default::default()
        };
        Ok(m)
    }

    /// `CTexture`: image, size of one tile, file name, average colour.
    fn texture(&mut self) -> Result<Texture> {
        let s = self.v.texture;
        if s > 4 {
            self.entity()?;
        }
        let mut t = Texture::default();
        if s < 6 {
            if s < 2 {
                if let Some(i) = self.obj()?
                    && let Obj::Image(img) = &self.objs[i]
                {
                    t.image = img.clone();
                }
            } else if self.ar.bool()? {
                t.image = self.dib()?;
            }
        } else if let Some(i) = self.obj()?
            && let Obj::Image(img) = &self.objs[i]
        {
            t.image = img.clone();
        }
        t.size = [self.ar.f64()?, self.ar.f64()?];
        t.file_name = self.ar.string()?;
        if s != 0 {
            t.avg = rgb(self.ar.color()?);
        }
        if s == 3 {
            self.ar.skip(8)?;
        }
        if let Some(px) = super::xml::image_size(&t.image) {
            t.px = px;
        }
        Ok(t)
    }

    /// `CDib`: an image, kept as the file bytes SketchUp stored.
    fn dib(&mut self) -> Result<std::sync::Arc<Vec<u8>>> {
        let s = self.v.dib;
        if s == 0 {
            return Ok(Default::default());
        }
        let format = if s == 1 { 4 } else { self.ar.i32()? };
        let n = self.ar.icount(1)?;
        self.budget.charge(n as u64)?;
        let bytes = self.ar.bytes(n)?.to_vec();
        if s >= 3 && format == 1 {
            self.ar.skip(4)?;
        }
        Ok(std::sync::Arc::new(bytes))
    }

    fn layer(&mut self, id: Id) -> Result<Layer> {
        let e = self.entity()?;
        let s = self.v.layer;
        let mut l = Layer { id, attrs: self.ent_attrs(e)?.0, ..Default::default() };
        l.name = google_earth_renamed(self.ar.string()?);
        l.visible = !self.ar.bool()?;
        if s == 0 {
            l.color = Some(rgb(self.ar.color()?));
            self.obj()?;
        } else {
            let m = self.material(0)?;
            l.color = Some(m.color);
            if s >= 2 {
                l.page_behavior = self.ar.u32()?;
                if s > 2 {
                    l.line_style = self.obj()?.map(|x| x as Id);
                }
            }
        }
        Ok(l)
    }

    fn layer_group(&mut self) -> Result<RawFolder> {
        self.entity()?;
        let s = self.v.layer_group;
        if !(1..=3).contains(&s) {
            return Err(self.err("dossier de balises : schéma inconnu"));
        }
        let mut f = RawFolder { name: self.ar.string()?, ..Default::default() };
        let n = self.ar.icount(2)?;
        for _ in 0..n {
            if let Some(g) = self.obj()? {
                f.folders.push(g);
            }
        }
        let n = self.ar.icount(2)?;
        for _ in 0..n {
            match self.obj()? {
                Some(l) => f.layers.push(l),
                None => return Err(self.err("balise nulle")),
            }
        }
        if s > 1 {
            f.hidden = self.ar.bool()?;
        }
        if s >= 3 {
            f.hidden_on_new_pages = self.ar.bool()?;
        }
        Ok(f)
    }

    fn line_style(&mut self) -> Result<Obj> {
        self.entity()?;
        let s = self.v.line_style;
        if s < 3 {
            self.entity()?;
        }
        let name = self.ar.string()?;
        if s == 1 {
            self.ar.skip(2)?;
        } else {
            self.ar.string()?;
        }
        self.ar.f64()?;
        if s > 1 {
            self.ar.f64()?;
            if s < 4 {
                self.ar.skip(8)?;
            } else {
                self.ar.color()?;
                self.ar.u8()?;
            }
        }
        Ok(Obj::LineStyle(name))
    }

    // --- View, scenes, options ------------------------------------------------------------

    fn camera(&mut self) -> Result<Camera> {
        let s = self.v.camera;
        let mut c = Camera { eye: self.ar.v3()?, target: self.ar.v3()?, up: self.ar.v3()?, ..Default::default() };
        self.ar.f64()?;
        self.ar.f64()?;
        c.perspective = self.ar.bool()?;
        c.fov = self.ar.f64()?;
        self.ar.f64()?;
        self.ar.v3()?;
        self.ar.f64()?;
        self.ar.bool()?;
        if s >= 2 {
            self.ar.bool()?;
            if s > 2 {
                self.ar.string()?;
            }
            if s > 3 {
                self.ar.f64()?;
            }
            if s > 4 {
                self.ar.bool()?;
                self.ar.f64s::<3>()?;
            }
        }
        c.target = super::build::settled_target(c.eye, c.target);
        Ok(c)
    }

    /// A camera written in place (`old`, the format of the first versions) or as an object.
    fn camera_ref(&mut self, old: bool) -> Result<Option<Camera>> {
        if old {
            let eye = self.ar.v3()?;
            let dir = self.ar.v3()?;
            let up = self.ar.v3()?;
            self.ar.f64()?;
            self.ar.f64()?;
            let perspective = self.ar.bool()?;
            let fov = self.ar.f64()?;
            self.ar.f64()?;
            self.ar.v3()?;
            let target = [eye[0] + dir[0], eye[1] + dir[1], eye[2] + dir[2]];
            return Ok(Some(Camera { eye, target, up, perspective, fov }));
        }
        Ok(match self.obj()? {
            Some(i) => match &self.objs[i] {
                Obj::Camera(c) => Some(**c),
                _ => return Err(self.err("caméra attendue")),
            },
            None => None,
        })
    }

    fn view_page(&mut self, id: Id) -> Result<Obj> {
        let mut s = Scene { id, animate: true, transition: -1.0, delay: -1.0, v7157: true, ..Default::default() };
        if self.v.entity != 0 {
            let e = self.entity()?;
            s.attrs = self.ent_attrs(e)?.0;
        }
        s.name = self.ar.string()?;
        s.description = self.ar.string()?;
        let v = self.v.view_page;
        s.flags = self.ar.u32()?;
        let f = s.flags;
        if f & 1 != 0 {
            s.camera = self.camera_ref(v < 7)?;
        }
        if f & 2 != 0 {
            s.rendering = Some(self.rendering()?.0);
            if v > 9 {
                s.style = self.obj()?.filter(|&i| matches!(self.objs.get(i), Some(Obj::Style(_)))).map(|i| i as Id);
            }
        }
        if f & 4 != 0 {
            self.shadow_info()?;
            if v > 1 {
                self.ar.bool()?;
            }
        }
        if f & 8 != 0 {
            self.sketch_cs()?;
            // Whether the scene shows the axes: its options of rendering's (`CViewPage::Serialize`
            // writes `DisplaySketchAxes` there).
            let axes = self.ar.bool()?;
            if let Some(r) = s.rendering.as_mut() {
                r.insert("DisplaySketchAxes".into(), json!(axes));
            }
        }
        if f & 0x10 != 0 {
            self.obj_list()?;
        }
        if v != 0 && f & 0x20 != 0 {
            let layers = self.obj_list()?;
            s.hidden_layers = Some(layers.into_iter().map(|l| l as Id).collect());
        }
        if v > 0xe && f & 0x20 != 0 {
            self.obj_list()?;
        }
        if v == 4 {
            if f & 0x40 != 0 {
                self.obj()?;
            }
            s.animate = self.ar.bool()?;
        } else if v > 4 {
            if f & 0x40 != 0 {
                self.obj_list()?;
            }
            s.animate = self.ar.bool()?;
        } else if v > 2 {
            s.animate = self.ar.bool()?;
        }
        if (6..=10).contains(&v) {
            self.ar.bool()?;
        }
        if v >= 8 {
            // Its transition and delay times (−1: the model's).
            s.transition = self.ar.f64()?;
            if v > 8 {
                s.delay = self.ar.f64()?;
                if v > 9 {
                    // Its matched photo (`CBackgroundImage`).
                    s.photo = self.obj()?.filter(|&i| matches!(self.objs.get(i), Some(Obj::Photo(_)))).map(|i| i as Id);
                }
                if v > 0xb {
                    s.v7157 = self.ar.bool()?;
                    // Its thumbnail: present, then its bytes if any.
                    if self.ar.bool()? && self.ar.bool()? {
                        let n = self.ar.icount(1)?;
                        self.budget.charge(n as u64)?;
                        let bytes = self.ar.bytes(n)?.to_vec();
                        s.thumbnail = (!bytes.is_empty()).then(|| std::sync::Arc::new(bytes));
                    }
                }
            }
        }
        if s.flags & 0x20 != 0 && s.hidden_layers.is_none() {
            s.hidden_layers = Some(Vec::new());
        }
        Ok(Obj::Page(Box::new(s)))
    }

    /// `CRenderingOptions`: the options by the tags of 2021+ files, and
    /// whether shadows are displayed (kept there by these versions).
    fn rendering(&mut self) -> Result<(BTreeMap<String, Value>, bool)> {
        self.entity()?;
        let s = self.v.rendering;
        let ar = &mut self.ar;
        let mut vals: BTreeMap<u16, Value> = BTreeMap::new();
        let mut put = |tag: u16, v: Value| {
            vals.insert(tag, v);
        };
        let int = |ar: &mut Ar| -> Result<Value> { Ok(json!(ar.i32()?)) };
        let boolean = |ar: &mut Ar| -> Result<Value> { Ok(json!(ar.bool()?)) };
        let real = |ar: &mut Ar| -> Result<Value> { Ok(json!(ar.f64()?)) };
        let color = |ar: &mut Ar| -> Result<Value> { Ok(json!(ar.color()?)) };
        put(0x733d, int(ar)?);
        if (0x11..=0x15).contains(&s) {
            let x = ar.i32()?;
            put(0x733e, json!(x == 1));
            put(0x733f, json!(x == 2));
        } else {
            put(0x733e, boolean(ar)?);
        }
        if s > 0x15 {
            put(0x733f, boolean(ar)?);
        }
        put(0x734b, boolean(ar)?);
        if s < 0x26 {
            if s <= 8 {
                put(0x7341, json!(ar.bool()? as i32));
            } else {
                put(0x7341, int(ar)?);
            }
        } else {
            put(0x734c, boolean(ar)?);
            put(0x7341, int(ar)?);
        }
        put(0x7357, color(ar)?);
        put(0x7358, color(ar)?);
        put(0x7359, color(ar)?);
        let mut shadows = false;
        if s >= 0xb {
            put(0x735b, color(ar)?);
            shadows = ar.bool()?;
        } else if s != 0 {
            shadows = ar.bool()?;
        }
        if s == 2 {
            put(0x7340, boolean(ar)?);
        }
        if s > 3 {
            put(0x7349, boolean(ar)?);
        }
        if s > 4 {
            put(0x7347, boolean(ar)?);
        }
        if s >= 6 {
            put(0x7340, boolean(ar)?);
            put(0x7348, int(ar)?);
            if s >= 7 {
                put(0x734d, boolean(ar)?);
                put(0x734e, int(ar)?);
                put(0x734f, boolean(ar)?);
                put(0x7350, int(ar)?);
                if s > 0x19 {
                    put(0x7351, boolean(ar)?);
                    put(0x7352, int(ar)?);
                    put(0x7353, boolean(ar)?);
                    put(0x7354, int(ar)?);
                    if s > 0x1b {
                        put(0x7355, boolean(ar)?);
                    }
                }
                if s < 0x27 {
                    let h = ar.bool()?;
                    put(0x7380, json!(h));
                    put(0x7381, json!(h));
                } else {
                    put(0x7380, boolean(ar)?);
                    put(0x7381, boolean(ar)?);
                }
            }
            if s >= 8 {
                ar.skip(4)?;
            }
        }
        if s > 9 {
            put(0x735c, color(ar)?);
            put(0x735d, color(ar)?);
        }
        if s > 0xb {
            put(0x736c, real(ar)?);
            put(0x736d, real(ar)?);
            put(0x736e, boolean(ar)?);
            put(0x736f, boolean(ar)?);
        }
        if s > 0x1c {
            put(0x735f, boolean(ar)?);
            put(0x7360, color(ar)?);
            put(0x7361, boolean(ar)?);
            put(0x7362, real(ar)?);
            put(0x7363, real(ar)?);
        }
        if s > 0x1d {
            put(0x7364, int(ar)?);
        }
        if s > 0x1e {
            put(0x7342, int(ar)?);
            put(0x7343, boolean(ar)?);
            put(0x7344, boolean(ar)?);
            put(0x7345, boolean(ar)?);
            put(0x7346, boolean(ar)?);
        }
        // Sky, ground, horizon; then (from schema 15) section planes and cuts.
        let mut defaults_7379 = true;
        if s >= 0x28 || s > 0xc {
            let mut ground = true;
            if s >= 0x28 {
                put(0x7382, boolean(ar)?);
                put(0x7365, color(ar)?);
                put(0x7366, color(ar)?);
            } else if s != 0xd {
                put(0x7365, color(ar)?);
                if s >= 0x18 {
                    put(0x7366, color(ar)?);
                }
            } else {
                let sky = ar.color()?;
                if sky != [0; 4] {
                    put(0x7365, json!(sky));
                }
                let gr = ar.color()?;
                if gr != [0; 4] {
                    put(0x7367, json!(gr));
                }
                for tag in [0x7368, 0x7369, 0x736a] {
                    let x = ar.i32()?;
                    if x != -1 {
                        put(tag, json!(x != 0));
                    }
                }
                ground = false;
            }
            if ground {
                put(0x7367, color(ar)?);
                put(0x7368, boolean(ar)?);
                put(0x7369, boolean(ar)?);
                put(0x736a, boolean(ar)?);
            }
            put(0x736b, int(ar)?);
            if s >= 0xf {
                put(0x7370, color(ar)?);
                put(0x7371, color(ar)?);
                put(0x7372, color(ar)?);
                put(0x7374, json!(ar.i32()?.clamp(1, 20)));
                put(0x7375, json!(ar.u32()?));
                if s >= 0x25 {
                    put(0x7373, color(ar)?);
                    put(0x7376, boolean(ar)?);
                    put(0x7377, int(ar)?);
                } else if s == 0x10 {
                    put(0x733f, boolean(ar)?);
                } else if s == 0x12 {
                    put(0x7377, json!(ar.bool()? as i32));
                } else if s > 0x12 {
                    put(0x7377, int(ar)?);
                }
                if s > 0x13 {
                    put(0x7379, boolean(ar)?);
                    put(0x737a, real(ar)?);
                }
                if s > 0x14 {
                    put(0x737b, boolean(ar)?);
                }
                if s > 0x16 {
                    defaults_7379 = false;
                }
            }
        }
        if defaults_7379 {
            put(0x7379, json!(true));
            put(0x737b, json!(true));
        }
        if (0x17..=0x18).contains(&s) {
            ar.bool()?;
        }
        if s > 0x1a {
            put(0x735a, color(ar)?);
        }
        if s > 0x1f {
            put(0x735e, boolean(ar)?);
        }
        if s == 0x21 {
            ar.bool()?;
        } else {
            if s > 0x22 {
                put(0x7378, real(ar)?);
            }
            if s > 0x23 {
                put(0x7356, boolean(ar)?);
                put(0x737c, boolean(ar)?);
                put(0x737d, real(ar)?);
                put(0x737e, boolean(ar)?);
                put(0x737f, real(ar)?);
            }
        }
        // Fields older schemas do not hold keep the values of the constructor.
        for (tag, v) in rendering_defaults() {
            vals.entry(tag).or_insert(v);
        }
        Ok((super::vff::rendering_from(&vals), shadows))
    }

    /// `CShadowInfo`, by the SDK's key names.
    fn shadow_info(&mut self) -> Result<BTreeMap<String, Value>> {
        self.entity()?;
        let s = self.v.shadow;
        let mut out = BTreeMap::new();
        let mut put = |k: &str, v: Value| {
            out.insert(k.to_string(), v);
        };
        put("ShadowTime", json!(self.ar.i32()? as i64));
        put("DaylightSavings", json!(self.ar.bool()?));
        put("City", json!(self.ar.string()?));
        put("Country", json!(self.ar.string()?));
        if s != 0 {
            put("Longitude", json!(self.ar.f64()?));
            put("Latitude", json!(self.ar.f64()?));
            put("TZOffset", json!(self.ar.f64()?));
        }
        if s > 1 {
            let n = self.ar.v3()?;
            put("NorthAngle", json!(super::vff::north_angle(n)));
            put("DisplayNorth", json!(self.ar.bool()?));
        }
        if s == 3 {
            self.ar.bool()?;
        } else {
            if s > 4 {
                put("DisplayOnAllFaces", json!(self.ar.bool()?));
                put("DisplayOnGroundPlane", json!(self.ar.bool()?));
                put("EdgesCastShadows", json!(self.ar.bool()?));
                put("Light", json!(self.ar.i32()?));
                put("Dark", json!(self.ar.i32()?));
            }
            if s > 5 {
                put("UseSunForAllShading", json!(self.ar.bool()?));
            }
        }
        Ok(out)
    }

    /// `CSketchCS`: the drawing axes, column-major 4×4.
    fn sketch_cs(&mut self) -> Result<[f64; 16]> {
        self.drawing_element()?;
        let o = self.ar.v3()?;
        let x = self.ar.v3()?;
        let y = self.ar.v3()?;
        let z = self.ar.v3()?;
        Ok([x[0], x[1], x[2], 0.0, y[0], y[1], y[2], 0.0, z[0], z[1], z[2], 0.0, o[0], o[1], o[2], 1.0])
    }

    /// `COptionsManager`: providers → key → value. The values of a provider SketchUp no
    /// longer has (`AxisOptions` of SketchUp 3 to 6) are read and left.
    fn options(&mut self, doc: &mut Doc) -> Result<()> {
        const PROVIDERS: &[&str] = &["UnitsOptions", "PageOptions", "SlideshowOptions", "NamedOptions", "PrintOptions"];
        let ver = self.ar.u32()?;
        let n = self.ar.count(2)?;
        let mut gone = BTreeMap::new();
        for _ in 0..n {
            let name = self.ar.string()?;
            let values = if PROVIDERS.contains(&name.as_str()) {
                doc.options.entry(name.clone()).or_default()
            } else {
                gone.clear();
                &mut gone
            };
            loop {
                let key = self.ar.string()?;
                if key.is_empty() {
                    break;
                }
                if let Some(v) = self.variant(0)? {
                    let v = super::vff::plain(&v);
                    if ver < 2 && name == "AxisOptions" && key == "DisplayAxes" {
                        self.display_axes = v.as_bool().or_else(|| v.as_i64().map(|i| i != 0));
                    }
                    // Before version 3 the length unit was the area and volume unit too.
                    if ver < 3 && name == "UnitsOptions" && key == "LengthUnit" {
                        values.insert("AreaUnit".into(), v.clone());
                        values.insert("VolumeUnit".into(), v.clone());
                    }
                    values.insert(key, v);
                }
            }
        }
        Ok(())
    }

    fn page_list(&mut self, doc: &mut Doc) -> Result<()> {
        self.entity()?;
        let pages = self.obj_list()?;
        let selected = self.obj()?;
        self.add_pages(doc, &pages, selected)?;
        Ok(())
    }

    fn add_pages(&mut self, doc: &mut Doc, pages: &[usize], selected: Option<usize>) -> Result<()> {
        for &p in pages {
            if self.take(p)
                && let Obj::Page(s) = &self.objs[p]
            {
                self.budget.charge(obj_cost(&self.objs[p]))?;
                if selected == Some(p) {
                    doc.active_scene = Some(doc.scenes.len());
                }
                let mut s = (**s).clone();
                // A flag of the 2021+ scenes that SketchUp sets on the scenes of older files.
                s.flags |= 0x200;
                // Hidden geometry (0x10) goes with its two sub-flags (0x80 hidden objects,
                // 0x100 hidden geometry): both set when neither is, both cleared without it
                // (`CViewPage::Serialize`).
                if s.flags & 0x10 == 0 {
                    s.flags &= !0x180;
                } else if s.flags & 0x180 == 0 {
                    s.flags |= 0x180;
                }
                doc.scenes.push(s);
            }
        }
        Ok(())
    }

    /// A font object: its slot when it is one.
    fn font_obj(&mut self) -> Result<Option<Id>> {
        Ok(self.obj()?.filter(|&i| matches!(self.objs.get(i), Some(Obj::Font(_)))).map(|i| i as Id))
    }

    /// `CTextStyle` (in place or object): its font, its settings, its second font.
    fn text_style(&mut self) -> Result<TextStyle> {
        self.entity()?;
        let s = self.v.text_style;
        let mut t = TextStyle::default();
        if s == 3 {
            self.ar.string()?;
            self.ar.u32()?;
            self.ar.bool()?;
            self.ar.bool()?;
            self.ar.string()?;
        } else {
            t.font = self.font_obj()?;
        }
        t.v1 = self.ar.u32()?;
        t.v2 = self.ar.u32()?;
        t.b1 = self.ar.bool()?;
        t.v3 = self.ar.u32()?;
        t.b2 = self.ar.bool()?;
        if s >= 2 {
            t.color = self.ar.color()?;
            if s >= 5 {
                t.color2 = self.ar.color()?;
                t.font2 = self.font_obj()?;
            }
        }
        Ok(t)
    }

    /// `CDimensionStyle` (in place), in the order of the 2021+ record (`docs/skp-format.md`
    /// §10.3).
    fn dim_style(&mut self) -> Result<DimStyle> {
        self.entity()?;
        let s = self.v.dim_style;
        let mut d = DimStyle { lead: self.ar.u32()?, ..Default::default() };
        if s > 0 {
            d.font = self.font_obj()?;
        }
        d.text3d = self.ar.bool()?;
        d.radial_prefix = self.ar.bool()?;
        d.ext_offset = self.ar.u32()?;
        if s > 2 {
            d.ext_overshoot = self.ar.u32()?;
        }
        d.line_weight = self.ar.u32()?;
        d.arrow = self.ar.i32()?;
        if s > 2 {
            d.arrow_size = self.ar.u32()?;
        }
        d.highlight = self.ar.bool()?;
        d.highlight_color = self.ar.color()?;
        d.b5fbf = self.ar.bool()?;
        d.hide_out_of_plane = self.ar.bool()?;
        d.hide_out_of_plane_value = self.ar.f64()?;
        d.hide_small = self.ar.bool()?;
        d.hide_small_value = self.ar.f64()?;
        if s > 1 {
            d.color = self.ar.color()?;
            d.text_color = self.ar.color()?;
        }
        if s >= 4 {
            d.v5fc6 = self.ar.u32()?;
        }
        Ok(d)
    }
    /// `CSkpStyle`: CEntity, GUID, the `.style` it came from, its items.
    fn skp_style(&mut self, id: Id) -> Result<super::style::StyleRec> {
        self.entity()?;
        let guid = self.ar.guid()?;
        let path = self.ar.string()?;
        let (name, description, mut items) = self.style_properties()?;
        // Before schema 2, one item for hidden geometry and objects (7010): SketchUp gives it
        // to both.
        if self.v.skp_style < 2
            && let Some(h) = items.iter().find(|i| i.id == super::style::DRAW_HIDDEN).map(|i| i.v.clone())
        {
            for k in [7017, 7018] {
                items.retain(|i| i.id != k);
                items.push(super::style::Item { id: k, v: h.clone() });
            }
            items.sort_by_key(|i| i.id);
        }
        super::style::fill_defaults(&mut items);
        Ok(super::style::StyleRec { id, guid, path, name, description, items, ..Default::default() })
    }

    /// The properties of a style: version, name, description, (version 1: a
    /// thumbnail), then the items (key, value).
    fn style_properties(&mut self) -> Result<(String, String, Vec<super::style::Item>)> {
        let ver = self.ar.u32()?;
        let name = self.ar.string()?;
        let description = self.ar.string()?;
        if ver == 1 && self.ar.bool()? {
            self.dib()?;
        }
        let n = self.ar.icount(4)?;
        let mut items = Vec::new();
        for _ in 0..n {
            let key = self.ar.u32()?;
            let v = match key {
                0x3f5 => super::style::ItemV::Npr(Box::new(self.style_image()?)),
                0x1389 => {
                    if ver <= 2 {
                        continue;
                    }
                    let n = self.ar.count(2)?;
                    let mut list = Vec::new();
                    for _ in 0..n {
                        let w = self.obj()?;
                        list.push(match w.and_then(|i| self.objs.get(i)) {
                            Some(Obj::Watermark(w)) => Some((**w).clone()),
                            _ => None,
                        });
                    }
                    super::style::ItemV::Watermarks(list)
                }
                _ => match self.style_value()? {
                    Some(v) => v,
                    None => continue,
                },
            };
            items.push(super::style::Item { id: key, v });
        }
        items.sort_by_key(|i| i.id);
        Ok((name, description, items))
    }

    /// The sketchy edges of a style: version; dropout and fade (doubles from
    /// version 2, before as 4 bytes each); width, halo, halo length, protos; lengths; images.
    fn style_image(&mut self) -> Result<super::style::Npr> {
        let ver = self.ar.i32()?;
        let mut n = super::style::Npr::default();
        if ver < 2 {
            n.dropout = self.ar.f32()? as f64;
            n.fade = self.ar.f32()? as f64;
        } else {
            n.dropout = self.ar.f64()?;
            n.fade = self.ar.f64()?;
        }
        n.width = self.ar.u32()?;
        n.halo = self.ar.bool()?;
        n.halo_length = self.ar.u32()?;
        n.protos = self.ar.u32()?;
        let k = self.ar.icount(4)?;
        for _ in 0..k {
            n.lengths.push(self.ar.u32()?);
        }
        let k = self.ar.icount(2)?;
        for _ in 0..k {
            if let Some(i) = self.obj()?
                && let Some(Obj::Image(img)) = self.objs.get(i)
            {
                n.images.push(img.clone());
            }
        }
        Ok(n)
    }

    /// A value of a style: a version (1), the variant type, the value.
    fn style_value(&mut self) -> Result<Option<super::style::ItemV>> {
        use super::style::ItemV;
        self.ar.skip(4)?;
        let t = self.ar.u32()?;
        Ok(Some(match t {
            0 => return Ok(None),
            1 | 2 => ItemV::Num(t, self.ar.u8()? as u64),
            3 => ItemV::Num(t, self.ar.u16()? as u64),
            // An unsigned value is read as an integer (the same for both).
            5 => ItemV::Num(4, self.ar.u32()? as u64),
            4 | 6 | 0xe => ItemV::Num(t, self.ar.u32()? as u64),
            7 | 0xf => ItemV::Num(t, self.ar.u64()?),
            9..=0xb => ItemV::Vec3(t, self.ar.v3()?),
            0xc => {
                let mut x = [0.0; 13];
                for v in x.iter_mut() {
                    *v = self.ar.f64()?;
                }
                ItemV::Xform(Box::new(x))
            }
            _ => return Err(self.err(&format!("valeur de style de type {t} inconnue"))),
        }))
    }

    /// `CSkpStyleManager` (in place): CEntity, the styles, the selected one,
    /// the active one (a copy, never one of the list), whether it was changed (schema > 1).
    fn skp_style_manager(&mut self) -> Result<()> {
        self.entity()?;
        let n = self.ar.i32()?;
        if n < 1 {
            return Ok(());
        }
        if n as usize * 2 > self.ar.left() {
            return Err(self.err("nombre de styles démesuré"));
        }
        let mut list = Vec::new();
        for _ in 0..n {
            if let Some(i) = self.obj()?
                && matches!(self.objs.get(i), Some(Obj::Style(_)))
                && !list.contains(&i)
            {
                list.push(i);
            }
        }
        let selected = self.obj()?;
        let active = self.obj()?.filter(|a| !list.contains(a));
        let changed = self.v.skp_style_mgr > 1 && self.ar.bool()?;
        self.style_mgr = Some((list, selected, active, changed));
        Ok(())
    }

    /// `CWatermark`: CEntity; found; name; when (8 bytes); position; tiled,
    /// stretched, keeps its proportions; behind the model; brightness is transparency;
    /// opacity; scale; the file it came from; the image.
    fn watermark(&mut self, id: Id) -> Result<super::style::Wm> {
        self.entity()?;
        let mut w = super::style::Wm { id, ..Default::default() };
        w.found = self.ar.bool()?;
        w.file = self.ar.string()?;
        w.time = self.ar.u32()? as i64;
        w.position = self.ar.u32()?.min(8);
        let (tiled, stretched, keep) = (self.ar.bool()?, self.ar.bool()?, self.ar.bool()?);
        w.fitting = if tiled {
            0
        } else if stretched {
            1
        } else {
            2
        };
        w.keep_aspect = stretched && keep;
        w.background = self.ar.bool()?;
        w.mask = self.ar.bool()?;
        w.alpha = self.ar.f64()?;
        w.scale = self.ar.f64()?;
        w.name = self.ar.string()?;
        if let Some(i) = self.obj()?
            && let Some(Obj::Image(img)) = self.objs.get(i)
        {
            w.image = Some(img.clone());
        }
        Ok(w)
    }

    fn watermark_manager(&mut self) -> Result<()> {
        self.entity()?;
        if self.v.watermark_mgr < 2 {
            self.obj_list()?;
        }
        let n = self.ar.icount(2)?;
        for _ in 0..n {
            let w = self.obj()?;
            self.watermark_list.push(w.filter(|&i| matches!(self.objs.get(i), Some(Obj::Watermark(_)))));
        }
        Ok(())
    }

    /// `CBackgroundImage`: a matched photo. Its image (the file
    /// it came from, from schema 2 its kind, its image object, from schema 3 width, height and a
    /// u32, from schema 4 the time), a bool, a double, the ends of its lines, from schema 6 the
    /// origin's offset, from 7 a double (made negative when positive), and the oldest forms.
    fn background(&mut self, id: Id) -> Result<MatchPhoto> {
        self.entity()?;
        let s = self.v.background;
        let p = match s {
            3 => 1,
            4 => 2,
            x if x > 4 => 3,
            _ => 0,
        };
        let mut m = MatchPhoto { id, ..Default::default() };
        let image = |r: &Self, i: Option<usize>| match i.and_then(|i| r.objs.get(i)) {
            Some(Obj::Image(b)) if !b.is_empty() => Some(b.clone()),
            _ => None,
        };
        if s < 2 {
            m.path = self.ar.string()?;
        } else {
            m.path = self.ar.string()?;
            m.kind = if p < 3 { if self.ar.bool()? { 1 } else { 2 } } else { self.ar.i32()? as u32 };
            let i = self.obj()?;
            m.image = image(self, i);
            if p > 0 {
                m.size = [self.ar.u32()?, self.ar.u32()?];
                m.v2afe = self.ar.u32()?;
            }
            if p > 1 {
                m.time = self.ar.u32()?;
            }
        }
        m.flag = self.ar.bool()?;
        m.value = self.ar.f64()?;
        let n = self.ar.count(24)?;
        self.budget.charge(24 * n as u64)?;
        for _ in 0..n {
            m.points.push(self.ar.v3()?);
        }
        if s > 5 {
            m.offset = self.ar.v3()?;
        }
        if s > 6 {
            let x = self.ar.f64()?;
            m.scale = if x > 0.0 { -x } else { x };
        }
        if s == 8 {
            self.ar.skip(4)?;
            let n = self.ar.icount(48)?;
            if n > 0 {
                self.ar.skip(n * 48)?;
                return Ok(m);
            }
        }
        if s == 0 {
            if self.ar.bool()? {
                m.image = Some(self.dib()?).filter(|b| !b.is_empty());
            }
        } else if s == 1 {
            let i = self.obj()?;
            m.image = image(self, i);
        }
        if s >= 10 {
            m.v290b = self.ar.u32()?;
        }
        Ok(m)
    }
    fn schema_file(&mut self) -> Result<()> {
        self.ar.string()?;
        self.ar.string()?;
        let n = self.ar.icount(1)?;
        self.ar.skip(n)?;
        for _ in 0..2 {
            let n = self.ar.count(2)?;
            for _ in 0..n {
                self.obj()?;
            }
        }
        if self.v.schema_file != 0 && self.ar.bool()? {
            self.obj()?;
        }
        Ok(())
    }

    // --- The model --------------------------------------------------------------------------

    /// `CSketchUpModel::Serialize`, loading branch.
    fn model(&mut self, doc: &mut Doc) -> Result<()> {
        let s = self.v.model;
        if s > 6 {
            self.ar.skip(8)?;
            if s > 0x11 {
                self.ar.skip(4)?;
            }
        }
        if s >= 0x1a {
            self.ar.skip(8)?;
        }
        if s > 3 {
            self.obj()?;
            self.ar.bool()?;
        }
        self.step("comportement");
        self.behavior()?;
        doc.description = self.ar.string()?;
        let mut model_attrs = None;
        if s >= 0x15 {
            self.step("options");
            self.options(doc)?;
            model_attrs = self.obj()?;
            self.obj()?;
        }
        self.step("caméra");
        // In place up to schema 10 (`R8B = schema <= 10` at the call).
        doc.camera = self.camera_ref(s <= 10)?;
        self.step("rendu");
        let (ro, shadows) = self.rendering()?;
        doc.rendering = ro;
        self.step("liste");
        self.obj_list()?;
        if s >= 0x11 {
            self.ar.u32()?;
        }
        self.step("racine");
        let root = self.component(true)?;
        self.step("ombres");
        if s > 1 {
            doc.shadow = self.shadow_info()?;
        }
        doc.shadow.insert("DisplayShadows".into(), json!(shadows));
        self.step("scènes");
        if s < 0xc {
            let pages = self.obj_list()?;
            self.add_pages(doc, &pages, None)?;
        } else {
            self.page_list(doc)?;
        }
        self.step("fin");
        if s < 6 {
            let n = self.ar.count(1)?;
            for _ in 0..n {
                self.ar.string()?;
            }
        }
        if s < 3 {
            self.material_manager()?;
        }
        doc.axes = Some(self.sketch_cs()?);
        self.ar.skip(16 * 4)?;
        if (5..=20).contains(&s) {
            self.options(doc)?;
        }
        if let Some(b) = self.display_axes {
            doc.rendering.insert("DisplaySketchAxes".into(), json!(b));
        }
        if s > 7 {
            let d = self.dim_style()?;
            let (c, arrow) = (d.color, d.arrow);
            doc.dim_style = Some(d);
            // Dimensions without a style of their own (`CDimension` schema 0) take the model's:
            // its colour becomes their material, its arrows theirs.
            if self.v.dimension == 0 {
                if c != [0; 4] {
                    self.dim_color = Some(c);
                }
                self.dim_arrow = Some(arrow);
            }
        }
        if s >= 9 {
            if s >= 10 {
                doc.text_style = Some(self.text_style()?);
            }
            self.font_manager(true)?;
            if s >= 0x1d {
                self.entity()?;
                self.line_styles = self.obj_list()?;
            }
            if s >= 0x1d || s == 0xd || s >= 0xf {
                // The active matched photo.
                doc.active_photo =
                    self.obj()?.filter(|&i| matches!(self.objs.get(i), Some(Obj::Photo(_)))).map(|i| i as Id);
            }
        }
        // Three values of the model whose use is not known (`Doc::model_flags`): an archive
        // without them leaves the bool false and the counter 0.
        let mut flags = ModelFlags { v20c: false, v20d: 1, v20f: 0 };
        if s >= 0xe {
            self.skp_style_manager()?;
            if s > 0xf {
                self.watermark_manager()?;
            }
            if s >= 0x14 {
                flags.v20f = self.ar.u32()?;
                flags.v20c = s == 0x15;
                if s >= 0x16 {
                    flags.v20c = self.ar.bool()?;
                    // Classification schemas (from schema 0x17).
                    if s >= 0x17 {
                        self.schema_list = self.obj_list()?;
                    }
                    if s > 0x1b {
                        flags.v20d = self.ar.u32()?.max(1);
                    }
                }
            } else if s == 0x13 {
                flags.v20f = if self.ar.bool()? { 0 } else { 2 };
            }
        }
        self.model_flags = Some(flags);
        self.finish(doc, root, model_attrs)?;
        self.finish_styles(doc)?;
        if self.unnamed_sections > 0 {
            name_sections(doc);
        }
        unique_definition_names(doc);
        // A model geolocated by an older SketchUp (a `GeoReference` dictionary without them):
        // SketchUp completes it from the shadow settings (at the end of
        // `CSketchUpModel::Serialize`).
        let num = |k: &str| doc.shadow.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let (lat, lon) = (num("Latitude"), num("Longitude"));
        if let Some(g) = doc.attrs.get_mut("GeoReference") {
            g.entry("UsesGeoReferencing".into()).or_insert(AttrValue::Bool(true));
            g.entry("LocationSource".into()).or_insert(AttrValue::Text("Google Earth".into()));
            g.entry("Longitude".into()).or_insert(AttrValue::Number(lon));
            g.entry("Latitude".into()).or_insert(AttrValue::Number(lat));
        }
        // Without dictionaries of its own in the file (schema < 0x15), the model gets the one
        // SketchUp keeps its contributors in.
        if s < 0x15 {
            let d = doc.attrs.entry("GSU_ContributorsInfo".into()).or_default();
            d.entry("LastModifiedByKey".into()).or_insert(AttrValue::Text(String::new()));
            d.entry("VersionKey".into()).or_insert(AttrValue::Json(json!({"t": "i", "v": 1000})));
        }
        Ok(())
    }

    /// The styles and watermarks of the document, from the managers read (each object once:
    /// a style or a watermark referred to again is counted again).
    fn finish_styles(&mut self, doc: &mut Doc) -> Result<()> {
        let style = |r: &Self, i: usize| match r.objs.get(i) {
            Some(Obj::Style(s)) => Some((**s).clone()),
            _ => None,
        };
        if let Some((list, selected, active, changed)) = self.style_mgr.take() {
            for &i in &list {
                if let Some(s) = style(self, i) {
                    self.budget.charge(obj_cost(&Obj::Style(Box::new(s.clone()))))?;
                    doc.styles.list.push(s);
                }
            }
            doc.styles.selected = selected.and_then(|s| doc.styles.list.iter().position(|x| x.id == s as Id));
            doc.styles.active = active.and_then(|i| style(self, i));
            doc.styles.changed = changed;
        }
        // The matched photos (each object once, in the order of the archive).
        for o in self.objs.iter() {
            if let Obj::Photo(m) = o {
                self.budget.charge(obj_cost(o))?;
                doc.photos.push((**m).clone());
            }
        }
        for i in std::mem::take(&mut self.schema_list) {
            if let Some(Obj::Schema(x)) = self.objs.get(i) {
                doc.schemas.push((**x).clone());
            }
        }
        doc.model_flags = self.model_flags.take();
        // The model's fonts, then those of texts and dimensions it does not list (SketchUp adds
        // them to its font manager: `SUModelGetFonts` gives them after).
        let listed = std::mem::take(&mut self.font_list);
        let mut seen: HashSet<usize> = listed.iter().copied().collect();
        let extra: Vec<usize> = std::mem::take(&mut self.annot_fonts).into_iter().filter(|&i| seen.insert(i)).collect();
        for i in listed.into_iter().chain(extra) {
            if let Some(Obj::Font(f)) = self.objs.get(i) {
                doc.fonts.push((**f).clone());
            }
        }
        for w in std::mem::take(&mut self.watermark_list) {
            doc.styles.watermarks.push(match w.and_then(|i| self.objs.get(i)) {
                Some(Obj::Watermark(w)) => Some((**w).clone()),
                _ => None,
            });
        }
        Ok(())
    }

    /// The document from the objects read.
    fn finish(&mut self, doc: &mut Doc, root: RawComp, model_attrs: Option<usize>) -> Result<()> {
        // Every copy made below is counted: an object is copied once (lists own their items,
        // see `take`), but its size is the file's to choose.
        let materials = std::mem::take(&mut self.materials);
        let mut listed = std::collections::HashSet::new();
        for m in materials {
            if self.take(m)
                && let Obj::Material(x) = &self.objs[m]
            {
                self.budget.charge(obj_cost(&self.objs[m]))?;
                listed.insert(m);
                doc.materials.push((**x).clone());
            }
        }
        // Then the others (a tag's colour, an image's texture), each object once.
        for (i, o) in self.objs.iter().enumerate() {
            if let Obj::Material(x) = o
                && !listed.contains(&i)
            {
                self.budget.charge(obj_cost(o))?;
                let mut x = (**x).clone();
                x.owned = true;
                doc.materials.push(x);
            }
        }
        // A colour material for the dimensions: an existing plain material of
        // that colour, else a new one named « * ».
        let same = |m: &Material, c: [u8; 4]| !m.owned && m.texture.is_none() && m.color == rgb(c) && !m.use_opacity;
        if let Some(c) = self.dim_color
            && let Some(m) = doc.materials.iter().find(|m| same(m, c))
        {
            self.dim_material = Some(m.id);
        }
        if let Some(c) = self.dim_color
            && self.dims > 0
            && self.dim_material.is_none()
        {
            self.dim_material = Some(self.objs.len() as Id);
            let names: std::collections::HashSet<&str> = doc.materials.iter().map(|m| m.name.as_str()).collect();
            let mut name = "*".to_string();
            let mut k = 1;
            while names.contains(name.as_str()) {
                name = format!("*{k}");
                k += 1;
            }
            doc.materials.push(Material {
                id: self.objs.len() as Id,
                name,
                color: rgb(c),
                opacity: 1.0,
                pbr: Pbr {
                    metallic_factor: 1.0,
                    roughness_factor: 1.0,
                    normal_scale: 1.0,
                    normal_style: 1,
                    ao_strength: 1.0,
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        for l in std::mem::take(&mut self.layers) {
            if self.take(l)
                && let Obj::Layer(x) = &self.objs[l]
            {
                self.budget.charge(obj_cost(&self.objs[l]))?;
                doc.layers.push((**x).clone());
            }
        }
        doc.active_layer = self.active_layer.map(|a| a as Id);
        if let Some(f) = self.root_folder.take() {
            doc.folders = self.folders(&f, 0)?;
        }
        for l in std::mem::take(&mut self.line_styles) {
            if self.take(l)
                && let Obj::LineStyle(name) = &self.objs[l]
            {
                self.budget.charge(obj_cost(&self.objs[l]))?;
                doc.line_styles.insert(l as Id, name.clone());
            }
        }
        let mut seen = std::collections::HashSet::new();
        let mut order = self.defs.clone();
        order.extend((0..self.objs.len()).filter(|&i| matches!(self.objs[i], Obj::Def(_))));
        for d in order {
            if !seen.insert(d) {
                continue;
            }
            let Obj::Def(raw) = &self.objs[d] else { continue };
            self.budget.charge(obj_cost(&self.objs[d]))?;
            let mut def = raw.def.clone();
            def.ents = self.ents(&raw.comp)?;
            doc.defs.push(def);
        }
        doc.root = self.ents(&root)?;
        doc.attrs = root.el.attrs.clone();
        if let Some(Obj::Container(c)) = model_attrs.and_then(|i| self.objs.get(i)) {
            let a = &c.0;
            for (k, v) in a {
                // The model's name and description, not a dictionary for the SDK.
                if k == "ModelProperties" {
                    let text = |key: &str| match v.get(key) {
                        Some(AttrValue::Text(s)) => Some(s.clone()),
                        _ => None,
                    };
                    if let Some(n) = text("Name") {
                        doc.name = n;
                    }
                    if let Some(d) = text("Description") {
                        doc.description = d;
                    }
                    continue;
                }
                doc.attrs.entry(k.clone()).or_default().extend(v.clone());
            }
        }
        Ok(())
    }

    /// The sub-folders of a tag folder: each folder object belongs to one parent (a forged file
    /// that lists a folder many times, or in many folders, does not multiply it).
    fn folders(&mut self, f: &RawFolder, depth: usize) -> Result<Vec<Folder>> {
        let mut out = Vec::new();
        if depth > 32 {
            return Ok(out);
        }
        for &g in &f.folders {
            if !self.take(g) {
                continue;
            }
            let Obj::Folder(x) = &self.objs[g] else { continue };
            self.budget.charge(obj_cost(&self.objs[g]))?;
            let x = (**x).clone();
            out.push(Folder {
                id: g as Id,
                name: x.name.clone(),
                visible: !x.hidden,
                visible_on_new_pages: !x.hidden_on_new_pages,
                layers: x.layers.iter().map(|&l| l as Id).collect(),
                folders: self.folders(&x, depth + 1)?,
            });
        }
        Ok(out)
    }

    /// The entities of a component, as the 2021+ reader gives them. Each item belongs to this
    /// component alone (read with `owned_list`), so the copies made here add up to the size of
    /// the objects read; they are counted all the same.
    fn ents(&self, c: &RawComp) -> Result<Ents> {
        let mut e = Ents::default();
        let mut verts = HashMap::new();
        let mut curves = std::collections::BTreeSet::new();
        let mut add_vertex = |e: &mut Ents, v: Id| {
            if verts.insert(v, ()).is_none()
                && let Some(Obj::Vertex(p)) = self.objs.get(v as usize)
            {
                e.verts.push(Vertex { id: v, pos: *p });
            }
        };
        let charge = |o: &Obj, extra: u64| self.budget.charge(obj_cost(o) + extra);
        for &i in &c.items {
            let o = &self.objs[i];
            match o {
                Obj::Edge(x) => {
                    charge(o, 2 * std::mem::size_of::<Vertex>() as u64)?;
                    add_vertex(&mut e, x.start);
                    add_vertex(&mut e, x.end);
                    if let Some(cv) = x.curve {
                        curves.insert(cv);
                    }
                    e.edges.push((**x).clone());
                }
                Obj::Face(f) => {
                    charge(o, 0)?;
                    let (el, plane, loops, back) = &**f;
                    let mut f = Face { el: el.clone(), plane: *plane, back: back.map(|b| b as Id), loops: Vec::new() };
                    for &l in loops {
                        let Obj::Loop(uses) = &self.objs[l] else { continue };
                        self.budget.charge(16 * uses.len() as u64)?;
                        let mut lp = Vec::new();
                        for &u in uses {
                            if let Obj::EdgeUse(Some(edge), rev) = &self.objs[u] {
                                lp.push((*edge as Id, *rev));
                                if let Obj::Edge(x) = &self.objs[*edge] {
                                    add_vertex(&mut e, x.start);
                                    add_vertex(&mut e, x.end);
                                }
                            }
                        }
                        f.loops.push(lp);
                    }
                    e.faces.push(f);
                }
                Obj::Inst(x, kind) => {
                    charge(o, 0)?;
                    match kind {
                        InstKind::Group => e.groups.push((**x).clone()),
                        InstKind::Component => e.instances.push((**x).clone()),
                        InstKind::Image => e.images.push(Image { inst: (**x).clone(), ..Default::default() }),
                    }
                }
                Obj::GuideLine(g) => {
                    charge(o, 0)?;
                    e.guide_lines.push((**g).clone());
                }
                Obj::GuidePoint(g) => {
                    charge(o, 0)?;
                    e.guide_points.push((**g).clone());
                }
                Obj::Section(s) => {
                    charge(o, 0)?;
                    e.sections.push((**s).clone());
                }
                Obj::Text(t) => {
                    charge(o, 0)?;
                    e.texts.push((**t).clone());
                }
                Obj::Polyline(x) => {
                    charge(o, 24 * x.points.len() as u64)?;
                    e.polylines.push((**x).clone());
                }
                Obj::Radial(d) => {
                    charge(o, 0)?;
                    // As a linear one: without a style of its own, the model's.
                    let mut d = (**d).clone();
                    if let Some(a) = self.dim_arrow {
                        d.arrow_type = a;
                    }
                    if d.el.material.is_none() {
                        d.el.material = self.dim_material;
                    }
                    e.radials.push(d);
                }
                Obj::Dim(d) => {
                    charge(o, 0)?;
                    let mut d = (**d).clone();
                    if let Some(a) = self.dim_arrow {
                        d.arrow_type = a;
                    }
                    if d.el.material.is_none() {
                        d.el.material = self.dim_material;
                    }
                    e.dims.push(d);
                }
                _ => {}
            }
        }
        // A curve's edges are in one component: one copy per component that uses it.
        for cv in curves {
            if let Some(o @ Obj::Curve(x)) = self.objs.get(cv as usize) {
                charge(o, 0)?;
                e.curves.push((**x).clone());
            }
        }
        settle_curves(&mut e, self.v.curve != 3);
        super::curves::check_arcs(&mut e);
        e.active_section = c.active_section.map(|a| a as Id);
        super::faces::check_faces(&mut e);
        self.budget.charge(16 * c.relations.len() as u64)?;
        for &r in &c.relations {
            if let Some(Obj::Relation(Some(to), Some(what))) = self.objs.get(r) {
                e.glues.push([*to as Id, *what as Id]);
            }
        }
        Ok(e)
    }
}
