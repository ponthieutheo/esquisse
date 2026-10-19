//! What a .skp will hold, made from an Esquisse model: SketchUp's objects with their persistent
//! ids, in inches, as the SDK writer (`crate::write_model`) makes them through the C API —
//! same defaults (tag colours, fonts, line styles, options), same conversions (curves, texture
//! placements, images) — and what the C API cannot make (snaps, spaces, the selected scene).
//! Both file formats are written from this (`vff` for 2021+, `legacy` for the MFC archive).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use esq_core::glam::{DMat4, DVec3};
use esq_core::meta::TextureData;
use esq_core::model::{
    AnnotKind, AttrValue, Attributes, DefId, EdgeId, Entities, LayerIdx, MaterialId, Model, VertexId,
};
use serde_json::Value;
use slotmap::Key;

use super::super::{Error, Result};

pub type Id = u64;

const IN: f64 = 25.4;

/// A typed value of a dictionary or an option (`SUTypedValue`), by SketchUp's variant type.
#[derive(Clone, Debug, PartialEq)]
pub enum Tv {
    Byte(u8),
    Short(i16),
    Int(i32),
    Float(f32),
    Double(f64),
    Bool(bool),
    Color([u8; 4]),
    Time(i64),
    Str(String),
    Array(Vec<Tv>),
    Point([f64; 3]),
    Vector([f64; 3]),
}

impl Tv {
    /// SketchUp's variant type: record 0x38a3 + type in 2021+ files.
    pub fn variant(&self) -> u8 {
        match self {
            Tv::Byte(_) => 2,
            Tv::Short(_) => 3,
            Tv::Int(_) => 4,
            Tv::Float(_) => 5,
            Tv::Double(_) => 6,
            Tv::Bool(_) => 7,
            Tv::Color(_) => 8,
            Tv::Time(_) => 9,
            Tv::Str(_) => 10,
            Tv::Array(_) => 11,
            Tv::Point(_) => 17,
            Tv::Vector(_) => 18,
        }
    }
}

/// An attribute dictionary: its name, its keys (sorted) and the dictionaries nested in it.
#[derive(Clone, Debug, Default)]
pub struct Dict {
    pub name: String,
    pub values: Vec<(String, Tv)>,
    pub children: Vec<Dict>,
}

/// Deepest nesting of dictionaries written (SketchUp's own go three deep: a classification's
/// type, its property, the property's value).
const MAX_NESTED_DICTS: usize = 32;

/// One side of a face's texture placement (`CFaceTextureCoords`).
#[derive(Clone, Debug)]
pub struct Tc {
    /// Bit 0 positioned, bit 1 projected.
    pub flags: u32,
    /// Texture coordinates (inches, row vector [s t 1]) to the plane's 2D frame, row-major.
    pub matrix: [f64; 9],
    pub projection: [f64; 3],
}

impl Tc {
    pub fn none() -> Tc {
        Tc { flags: 0, matrix: IDENTITY3, projection: [0.0; 3] }
    }
}

const IDENTITY3: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// What every entity and drawing element has.
#[derive(Clone, Debug, Default)]
pub struct El {
    pub id: Id,
    pub material: Option<Id>,
    /// `None`: the default tag.
    pub layer: Option<Id>,
    /// `doc::flags`: hidden 1, casts 2, receives 4, soft 8, smooth 16, locked 32.
    pub flags: u8,
    pub dicts: Vec<Dict>,
    /// A face's texture placement, front and back.
    pub tex: Option<Box<[Tc; 2]>>,
}

pub mod flags {
    pub const HIDDEN: u8 = 1;
    pub const CASTS: u8 = 2;
    pub const RECEIVES: u8 = 4;
    pub const SOFT: u8 = 8;
    pub const SMOOTH: u8 = 16;
    pub const LOCKED: u8 = 32;
}

#[derive(Clone, Debug)]
pub struct Vertex {
    pub id: Id,
    pub pos: [f64; 3],
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub el: El,
    pub start: Id,
    pub end: Id,
    pub curve: Option<Id>,
}

/// An arc (`CArcCurve`, 16 doubles of record 0x4c2d).
#[derive(Clone, Debug)]
pub struct Arc3 {
    pub center: [f64; 3],
    pub normal: [f64; 3],
    pub xaxis: [f64; 3],
    pub yaxis: [f64; 3],
    pub radius: f64,
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Debug)]
pub struct Curve {
    pub id: Id,
    pub segments: u32,
    /// Drawn as a polygon (always for a plain curve).
    pub polygon: bool,
    /// The edges, in order.
    pub edges: Vec<Id>,
    pub arc: Option<Arc3>,
}

#[derive(Clone, Debug)]
pub struct Face {
    pub el: El,
    pub back: Option<Id>,
    pub plane: [f64; 4],
    /// Outer loop first; (edge, reversed) in loop order. Each loop has its id.
    pub loops: Vec<(Id, Vec<(Id, bool)>)>,
}

#[derive(Clone, Debug)]
pub struct Inst {
    pub el: El,
    pub name: String,
    /// The 3×3 part by rows, the translation (inches), the scale: as record 0x1966.
    pub xform: [f64; 13],
    pub def: Id,
    pub guid: [u8; 16],
    /// The hatch pattern of its section cuts (id of one of `Plan::hatch_patterns`).
    pub hatch: Option<Id>,
}

#[derive(Clone, Debug)]
pub struct GuideLine {
    pub el: El,
    pub point: [f64; 3],
    pub dir: [f64; 3],
    /// Parameters of the two ends along `dir` (±1e30 for an infinite line).
    pub t: [f64; 2],
}

#[derive(Clone, Debug)]
pub struct GuidePoint {
    pub el: El,
    pub pos: [f64; 3],
}

#[derive(Clone, Debug)]
pub struct Section {
    pub el: El,
    pub plane: [f64; 4],
    pub name: String,
    pub symbol: String,
    pub hatch: Option<Id>,
}

/// What a point of an annotation is attached to (2021+ 0x5208, MFC): SketchUp's
/// type of connection, the point of the file (the parameter of types 5 to 7 and 11), the entity
/// and the instances that lead to it from the annotation's collection, each by id and class.
#[derive(Clone, Debug, Default)]
pub struct Conn {
    pub kind: i32,
    pub point: [f64; 3],
    pub leaf: Option<(Id, &'static str)>,
    pub path: Vec<(Id, &'static str)>,
}

#[derive(Clone, Debug)]
pub struct Text {
    pub el: El,
    pub text: String,
    /// The point the leader points to.
    pub point: Option<[f64; 3]>,
    /// What it is attached to.
    pub conn: Option<Conn>,
    pub leader: [f64; 3],
    pub screen: [f64; 2],
    /// 0 none, 1 view based, 2 pushpin.
    pub leader_type: i32,
    pub arrow_type: i32,
    pub font: Id,
}

#[derive(Clone, Debug)]
pub struct Dim {
    pub el: El,
    pub text: String,
    pub arrow_type: i32,
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub normal: [f64; 3],
    pub xaxis: [f64; 3],
    /// Along the dimension, and the offset of the dimension line.
    pub position: [f64; 2],
    pub font: Id,
    /// What its start and its end are attached to.
    pub conns: [Option<Conn>; 2],
}

/// A radial dimension (`CDimensionRadial`): the edge of the arc it measures, else the arc
/// itself; the angle of its point on the arc and its leader's offset (inches); diameter.
#[derive(Clone, Debug)]
pub struct Radial {
    pub el: El,
    pub text: String,
    pub arrow_type: i32,
    pub font: Id,
    pub edge: Option<Id>,
    pub arc: Option<Arc3>,
    pub angle: f64,
    pub leader: f64,
    pub diameter: bool,
}

/// A 3D polyline (`CPolyline3d`): its points (inches).
#[derive(Clone, Debug)]
pub struct Polyline {
    pub el: El,
    pub points: Vec<[f64; 3]>,
}

#[derive(Clone, Debug)]
pub struct Snap {
    pub el: El,
    pub pos: [f64; 3],
    pub dir: [f64; 3],
    pub right: [f64; 3],
    pub up: [f64; 3],
}

#[derive(Clone, Debug)]
pub struct Space {
    pub el: El,
    pub name: String,
    pub orientation: [f64; 3],
    pub ceiling: [f64; 4],
    pub perimeter: Vec<[f64; 3]>,
    pub holes: Vec<Vec<[f64; 3]>>,
}

/// An image placement: an instance of an image definition.
#[derive(Clone, Debug)]
pub struct Image {
    pub inst: Inst,
}

#[derive(Clone, Debug, Default)]
pub struct Ents {
    /// The collection's own entity part: the definition's id and dictionaries, or the model's
    /// dictionaries for the root.
    pub el: El,
    pub verts: Vec<Vertex>,
    pub curves: Vec<Curve>,
    pub arcs: Vec<Curve>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
    pub instances: Vec<Inst>,
    pub groups: Vec<Inst>,
    pub images: Vec<Image>,
    pub guide_lines: Vec<GuideLine>,
    pub guide_points: Vec<GuidePoint>,
    pub snaps: Vec<Snap>,
    pub sections: Vec<Section>,
    pub texts: Vec<Text>,
    pub dims: Vec<Dim>,
    pub radials: Vec<Radial>,
    pub polylines: Vec<Polyline>,
    pub spaces: Vec<Space>,
    pub active_section: Option<Id>,
    /// Bounding box of a definition (inches), when it has something.
    pub bbox: Option<[f64; 6]>,
    /// What components are glued to (`CRelationship`): the face or instance, the component.
    pub glues: Vec<[Id; 2]>,
}

impl Ents {
    /// Ids of the drawing elements of the collection, sorted (record 0x138e).
    pub fn order(&self) -> Vec<Id> {
        let mut v: Vec<Id> = Vec::new();
        v.extend(self.edges.iter().map(|x| x.el.id));
        v.extend(self.faces.iter().map(|x| x.el.id));
        v.extend(self.instances.iter().map(|x| x.el.id));
        v.extend(self.groups.iter().map(|x| x.el.id));
        v.extend(self.images.iter().map(|x| x.inst.el.id));
        v.extend(self.guide_lines.iter().map(|x| x.el.id));
        v.extend(self.guide_points.iter().map(|x| x.el.id));
        v.extend(self.snaps.iter().map(|x| x.el.id));
        v.extend(self.sections.iter().map(|x| x.el.id));
        v.extend(self.texts.iter().map(|x| x.el.id));
        v.extend(self.dims.iter().map(|x| x.el.id));
        v.extend(self.radials.iter().map(|x| x.el.id));
        v.extend(self.polylines.iter().map(|x| x.el.id));
        v.extend(self.spaces.iter().map(|x| x.el.id));
        v.sort_unstable();
        v
    }
}

#[derive(Clone, Debug, Default)]
pub struct Behavior {
    /// 0 any, 1 horizontal, 2 vertical, 3 sloped.
    pub snap_to: i32,
    pub glues: bool,
    pub cuts_opening: bool,
    pub always_face_camera: bool,
    pub shadows_face_sun: bool,
    pub no_scale_mask: i32,
}

#[derive(Clone, Debug)]
pub struct Def {
    pub id: Id,
    pub guid: [u8; 16],
    pub name: String,
    pub description: String,
    /// 0 component, 1 group, 2 image.
    pub kind: u32,
    pub behavior: Behavior,
    pub ents: Ents,
    /// Its thumbnail: the camera (eye, target, up; inches) and the image as read
    /// (`build::def_thumbnail_key`); none: the SDK's camera, no image.
    pub thumb_camera: Option<[[f64; 3]; 3]>,
    pub thumb: Option<Arc<Vec<u8>>>,
    /// The file it was loaded from and when (seconds since 1970; none: the plan's time).
    pub path: String,
    pub time: Option<u32>,
}

/// A texture: the image file as SketchUp keeps it.
#[derive(Clone, Debug)]
pub struct Tex {
    /// The name the texture gives (`textureFilename`).
    pub file_name: String,
    /// The file of the material's folder.
    pub stored: String,
    pub image: Arc<Vec<u8>>,
    pub px: [u32; 2],
    /// Size of one tile, inches.
    pub size: [f64; 2],
    /// Average colour of the image.
    pub avg: [u8; 3],
}

#[derive(Clone, Debug, Default)]
pub struct Pbr {
    pub metalness: bool,
    pub metallic: f32,
    pub roughness: bool,
    pub roughness_factor: f32,
    pub roughness_inverted: bool,
    pub normal: bool,
    pub normal_scale: f32,
    pub normal_style: i32,
    pub ao: bool,
    pub ao_strength: f32,
    pub metallic_texture: Option<Tex>,
    pub roughness_texture: Option<Tex>,
    pub normal_texture: Option<Tex>,
    pub ao_texture: Option<Tex>,
}

#[derive(Clone, Debug)]
pub struct Mat {
    pub id: Id,
    pub name: String,
    /// The folder of `materials/` its files go in.
    pub folder: String,
    /// Belongs to a tag (its colour) or an image: not listed with the others.
    pub owned: bool,
    /// A tag's colour (`pbrPromoState` 2).
    pub of_layer: bool,
    pub color: [u8; 3],
    /// 0 colour, 1 texture, 2 colorized texture.
    pub kind: i32,
    pub colorize_type: i32,
    /// 1 − opacity.
    pub trans: f64,
    pub use_trans: bool,
    pub workflow: i32,
    pub texture: Option<Tex>,
    pub pbr: Pbr,
    pub dicts: Vec<Dict>,
    /// The thumbnail of its folder as read (`build::mat_thumbnail_key`).
    pub thumbnail: Option<Arc<Vec<u8>>>,
    /// The hatch pattern of its section cuts.
    pub hatch: Option<Id>,
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub id: Id,
    pub name: String,
    pub hidden: bool,
    pub material: Mat,
    pub page_behavior: u32,
    pub line_style: Option<Id>,
    pub dicts: Vec<Dict>,
    /// A tag whose colour was never set: black in the current format, the grey of SketchUp's
    /// new tags in an MFC archive (as SketchUp 2026 writes them).
    pub colorless: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Folder {
    /// 0 for the manager's root folder.
    pub id: Id,
    pub name: String,
    pub hidden: bool,
    pub hidden_on_new_pages: bool,
    pub folders: Vec<Folder>,
    pub layers: Vec<Id>,
}

#[derive(Clone, Debug)]
pub struct LineStyle {
    pub id: Id,
    pub name: &'static str,
    pub pattern: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    pub perspective: bool,
    pub fov: f64,
    /// Records 0x34c0, 0x34c1 (near and far ?), 0x34c3 (height of the parallel view).
    pub near: f64,
    pub far: f64,
    pub height: f64,
}

/// The shadow settings (`CShadowInfo`), by record.
#[derive(Clone, Debug)]
pub struct Shadow {
    pub time: u32,
    pub daylight: bool,
    pub city: String,
    pub country: String,
    pub longitude: f64,
    pub latitude: f64,
    pub tz: f64,
    pub north: [f64; 3],
    pub display: bool,
    pub display_north: bool,
    pub on_faces: bool,
    pub on_ground: bool,
    pub from_edges: bool,
    pub light: i32,
    pub dark: i32,
    pub sun_shading: bool,
}

/// A value of the rendering options, by record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ro {
    Int(i32),
    Bool(bool),
    Double(f64),
    Float(f32),
    Color([u8; 4]),
}

#[derive(Clone, Debug)]
pub struct Page {
    pub id: Id,
    pub name: String,
    pub description: String,
    pub flags: u32,
    pub camera: Camera,
    pub rendering: Option<Vec<(u16, Ro)>>,
    pub shadow: Option<Shadow>,
    pub axes: Option<[f64; 12]>,
    pub hidden_layers: Vec<Id>,
    pub animate: bool,
    pub dicts: Vec<Dict>,
    /// The style the scene keeps (id of one of `Plan::styles`; none: the selected one).
    pub style: Option<Id>,
    /// Its matched photo (id of one of `Plan::photos`).
    pub photo: Option<Id>,
    /// Its transition and delay times (−1: the model's).
    pub transition: f64,
    pub delay: f64,
    /// 0x7153 and 0x7157 as read (empty and true for a scene of Esquisse).
    pub v7153: String,
    pub v7157: bool,
    /// Its thumbnail as read (`build::scene_thumbnail_key`).
    pub thumbnail: Option<Arc<Vec<u8>>>,
    /// The environment it keeps (id of one of `Plan::environments`) and its settings (with flag
    /// 0x200).
    pub env_id: Option<Id>,
    pub env: Option<crate::native::doc::SceneEnv>,
    /// The hatch pattern settings of its rendering options (none: the model's).
    pub ro_hatch: Option<crate::native::doc::HatchSettings>,
}

/// Everything a file holds.
#[derive(Clone)]
pub struct Plan {
    pub guid: [u8; 16],
    pub name: String,
    pub description: String,
    /// `UnitsOptions/LengthUnit`.
    pub unit: i32,
    pub options: Vec<(String, Vec<(String, Tv)>)>,
    pub camera: Camera,
    pub rendering: Vec<(u16, Ro)>,
    pub shadow: Shadow,
    /// Origin, x, y, z axes (inches).
    pub axes: [f64; 12],
    pub line_styles: Vec<LineStyle>,
    pub materials: Vec<Mat>,
    pub layers: Vec<Layer>,
    pub active_layer: Id,
    pub folders: Folder,
    pub fonts: [Id; 2],
    /// The model's fonts (with their ids), its text and dimension styles (fonts by id).
    pub font_list: Vec<crate::native::doc::Font>,
    pub text_style: crate::native::doc::TextStyle,
    pub dim_style: crate::native::doc::DimStyle,
    pub style: Id,
    /// The model's styles and watermarks, with their ids (none: the default style).
    pub styles: crate::native::style::Styles,
    /// The classification schemas loaded in the model that have their `.skc` file.
    pub schemas: Vec<crate::native::doc::SchemaFile>,
    /// The matched photos of the scenes (`doc::MatchPhoto`, ids of the plan), the active one.
    pub photos: Vec<crate::native::doc::MatchPhoto>,
    pub active_photo: Option<Id>,
    /// The environments (`doc::Environment`, ids of the plan, their folders unique), the
    /// current one.
    pub environments: Vec<crate::native::doc::Environment>,
    pub current_env: Option<Id>,
    /// The hatch patterns (ids of the plan, folders unique) and the settings of the model's
    /// rendering options.
    pub hatch_patterns: Vec<crate::native::doc::HatchPattern>,
    pub ro_hatch: crate::native::doc::HatchSettings,
    /// The model's values whose use is not known (`doc::Doc::model_flags`).
    pub model_flags: crate::native::doc::ModelFlags,
    /// Definitions, each after the ones it places (the MFC archive writes them in place).
    pub defs: Vec<Def>,
    pub root: Ents,
    pub model_dicts: Vec<Dict>,
    pub scenes: Vec<Page>,
    pub selected_scene: Option<Id>,
    pub next_id: Id,
    /// Seconds since 1970 (definitions, client history).
    pub time: u32,
    /// The model's preview (PNG), when the caller made one.
    pub thumbnail: Option<Vec<u8>>,
}

/// A fresh random GUID (version 4), as the SDK gives a new model.
pub fn new_guid() -> [u8; 16] {
    use std::hash::{BuildHasher, Hasher};
    let mut out = [0u8; 16];
    for (i, half) in out.chunks_mut(8).enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_usize(i);
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        h.write_u128(t);
        half.copy_from_slice(&h.finish().to_le_bytes());
    }
    out[7] = (out[7] & 0x0f) | 0x40;
    out[8] = (out[8] & 0x3f) | 0x80;
    out
}

/// The twelve line styles every model has (ids 6 to 17, after the style, the two fonts, the
/// default tag and its colour), with their dash patterns.
pub const LINE_STYLES: [(&str, &str); 12] = [
    ("Solid Basic", "16.0"),
    ("Short dash", "6.0, -6.0"),
    ("Dash", "12.0, -6.0"),
    ("Dot", "1.0, -6.0"),
    ("Dash dot", "12.0, -6.0, 1.0, -6.0"),
    ("Dash double-dot", "12.0, -6.0, 1.0, -6.0, 1.0, -6.0"),
    ("Dash triple-dot", "12.0, -6.0, 1.0, -6.0, 1.0, -6.0, 1.0, -6.0"),
    ("Double-dash dot", "12.0, -6.0, 12.0, -6.0, 1.0, -6.0"),
    ("Double-dash double-dot", "12.0, -6.0, 12.0, -6.0, 1.0, -6.0, 1.0, -6.0"),
    ("Double-dash triple-dot", "12.0, -6.0, 12.0, -6.0, 1.0, -6.0, 1.0, -6.0, 1.0, -6.0"),
    ("Long-dash dash", "36.0, -10.0, 12.0, -10.0"),
    ("Long-dash double-dash", "36.0, -10.0, 12.0, -10.0, 12.0, -10.0"),
];

/// Colour of the default tag of a new model (`Layer0`).
pub const LAYER0_COLOR: [u8; 3] = [255, 84, 84];

/// An arc as the reader kept it (millimetres): centre, unit normal, axes with their lengths,
/// radius, angles.
struct ExactArc {
    c: DVec3,
    n: DVec3,
    x: DVec3,
    y: DVec3,
    r: f64,
    a0: f64,
    a1: f64,
}

/// The arc as read, its vertices in its order: from the one at its start angle, the way the
/// angle grows.
fn exact_arc(e: &Entities, vs: &mut [VertexId], closed: bool, a: &ExactArc) -> Result<Arc3> {
    let at = |t: f64| a.c + a.x * t.cos() + a.y * t.sin();
    let pos = |v: VertexId| e.verts[v].pos;
    let w = at(a.a0);
    let n = vs.len();
    if n >= 2 {
        if closed {
            let k = (0..n).min_by(|&i, &j| pos(vs[i]).distance(w).total_cmp(&pos(vs[j]).distance(w))).unwrap_or(0);
            vs.rotate_left(k);
            let sweep = if (a.a1 - a.a0).abs() > 1e-9 { a.a1 - a.a0 } else { std::f64::consts::TAU };
            let next = at(a.a0 + sweep / n as f64);
            if pos(vs[1]).distance(next) > pos(vs[n - 1]).distance(next) {
                vs[1..].reverse();
            }
        } else if pos(vs[n - 1]).distance(w) < pos(vs[0]).distance(w) {
            vs.reverse();
        }
    }
    exact_arc3(a)
}

/// The arc as read, in the file's units.
fn exact_arc3(a: &ExactArc) -> Result<Arc3> {
    let c = a.c / IN;
    Ok(Arc3 {
        center: [fin(c.x, "arc")?, fin(c.y, "arc")?, fin(c.z, "arc")?],
        normal: vec3(a.n, "arc")?,
        xaxis: vec3(a.x / IN, "arc")?,
        yaxis: vec3(a.y / IN, "arc")?,
        radius: fin(a.r / IN, "arc")?,
        start: fin(a.a0, "arc")?,
        end: fin(a.a1, "arc")?,
    })
}

fn fin(x: f64, what: &str) -> Result<f64> {
    if x.is_finite() { Ok(x) } else { Err(Error(format!("valeur non finie ({what})"))) }
}

fn pt(p: DVec3, what: &str) -> Result<[f64; 3]> {
    Ok([fin(p.x / IN, what)?, fin(p.y / IN, what)?, fin(p.z / IN, what)?])
}

fn vec3(v: DVec3, what: &str) -> Result<[f64; 3]> {
    Ok([fin(v.x, what)?, fin(v.y, what)?, fin(v.z, what)?])
}

/// A transformation in millimetres as record 0x1966: rows of the 3×3 part, translation in
/// inches, scale.
pub fn xform13(m: &DMat4) -> Result<[f64; 13]> {
    let c = m.to_cols_array();
    let t = [c[0], c[4], c[8], c[1], c[5], c[9], c[2], c[6], c[10], c[12] / IN, c[13] / IN, c[14] / IN, c[15]];
    for x in t {
        fin(x, "transformation")?;
    }
    Ok(t)
}

/// A tagged JSON value of the Ruby bridge as the SDK stores it (`set_tagged`): `None` for
/// `nil` (the SDK leaves the key out).
fn tagged(j: &Value, depth: usize) -> Option<Tv> {
    let v = j.get("v").unwrap_or(&Value::Null);
    let num = |k: usize| v.get(k).and_then(Value::as_f64).unwrap_or_default();
    Some(match j.get("t").and_then(Value::as_str).unwrap_or("n") {
        "s" => Tv::Str(v.as_str().unwrap_or_default().to_string()),
        "b" => Tv::Bool(v.as_bool().unwrap_or_default()),
        "i" => match v.as_i64() {
            Some(n) if i32::try_from(n).is_ok() => Tv::Int(n as i32),
            _ => Tv::Double(v.as_f64().unwrap_or_default()),
        },
        "f" | "l" => Tv::Double(v.as_f64().unwrap_or_default()),
        "nf" => Tv::Double(match v.as_str() {
            Some("inf") => f64::INFINITY,
            Some("-inf") => f64::NEG_INFINITY,
            _ => f64::NAN,
        }),
        "c" => {
            let c = |k: usize| v.get(k).and_then(Value::as_u64).unwrap_or(255).min(255) as u8;
            Tv::Color([c(0), c(1), c(2), c(3)])
        }
        "time" => Tv::Time(v.get(0).and_then(Value::as_i64).unwrap_or_default()),
        "p" | "pt" | "vec" => Tv::Vector([num(0), num(1), num(2)]),
        "a" if depth < 16 => {
            Tv::Array(v.as_array().map(|a| a.iter().filter_map(|x| tagged(x, depth + 1)).collect()).unwrap_or_default())
        }
        _ => return None,
    })
}

/// An attribute value as the SDK writer stores it (`write_dicts`).
pub fn attr_tv(v: &AttrValue) -> Option<Tv> {
    match v {
        AttrValue::Text(s) => Some(Tv::Str(s.clone())),
        AttrValue::Number(n) => Some(Tv::Double(*n)),
        AttrValue::Bool(b) => Some(Tv::Bool(*b)),
        AttrValue::Json(j) => tagged(j, 0),
    }
}

/// Attribute dictionaries as SketchUp keeps them: keys sorted, `nil` left out, an empty
/// dictionary kept (SketchUp writes it).
pub fn dicts(a: &Attributes) -> Vec<Dict> {
    a.iter()
        .map(|(name, kv)| Dict {
            name: name.clone(),
            values: kv.iter().filter_map(|(k, v)| Some((k.clone(), attr_tv(v)?))).collect(),
            children: Vec::new(),
        })
        .collect()
}

/// The dictionaries of `owner` (its key in the Ruby bridge: `model`, `d:0:<id>`, `h:e:…`) with
/// those nested in them, kept in the side table under `ad:<owner>|<name>`
/// (`AttributeDictionary#attribute_dictionaries`, `build::split_nested`).
pub fn dicts_with(model: &Model, owner: &str, a: &Attributes) -> Vec<Dict> {
    fn go(model: &Model, owner: &str, a: &Attributes, depth: usize) -> Vec<Dict> {
        let mut out = dicts(a);
        if depth < MAX_NESTED_DICTS {
            for d in &mut out {
                let key = format!("ad:{owner}|{}", d.name);
                if let Some(sub) = model.attr_store.get(&key) {
                    d.children = go(model, &key, sub, depth + 1);
                }
            }
        }
        out
    }
    go(model, owner, a, 0)
}

/// A JSON value of the options as the SDK writes it, in the type `like` has (or its own).
fn option_tv(j: &Value, like: Option<&Tv>) -> Option<Tv> {
    let f = j.as_f64().or_else(|| j.as_bool().map(|b| b as i32 as f64));
    Some(match like {
        Some(Tv::Bool(_)) => Tv::Bool(j.as_bool().unwrap_or(f.is_some_and(|x| x != 0.0))),
        Some(Tv::Int(_)) => Tv::Int(f.map(|x| x.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32)?),
        Some(Tv::Double(_)) => Tv::Double(f?),
        Some(Tv::Str(_)) => Tv::Str(j.as_str()?.to_string()),
        _ => match j {
            Value::Bool(b) => Tv::Bool(*b),
            Value::Number(n) if n.is_i64() => Tv::Int(i32::try_from(n.as_i64()?).ok()?),
            Value::Number(n) => Tv::Double(n.as_f64()?),
            Value::String(s) => Tv::Str(s.clone()),
            _ => return None,
        },
    })
}

/// The options of a new model (Model Info), in the order SketchUp writes them.
pub fn default_options() -> Vec<(String, Vec<(String, Tv)>)> {
    let s = |x: &str| x.to_string();
    vec![
        (s("NamedOptions"), vec![]),
        (s("PageOptions"), vec![(s("ShowTransition"), Tv::Bool(true)), (s("TransitionTime"), Tv::Double(2.0))]),
        (s("SlideshowOptions"), vec![(s("LoopSlideshow"), Tv::Bool(false)), (s("SlideTime"), Tv::Double(1.0))]),
        (
            s("UnitsOptions"),
            vec![
                (s("LengthPrecision"), Tv::Int(0)),
                (s("LengthFormat"), Tv::Int(0)),
                (s("LengthUnit"), Tv::Int(2)),
                (s("LengthSnapEnabled"), Tv::Bool(false)),
                (s("LengthSnapLength"), Tv::Double(1.0)),
                (s("AnglePrecision"), Tv::Int(1)),
                (s("AngleSnapEnabled"), Tv::Bool(true)),
                (s("SnapAngle"), Tv::Double(15.0)),
                (s("SuppressUnitsDisplay"), Tv::Bool(false)),
                (s("ForceInchDisplay"), Tv::Bool(false)),
                (s("AreaUnit"), Tv::Int(4)),
                (s("VolumeUnit"), Tv::Int(4)),
                (s("AreaPrecision"), Tv::Int(2)),
                (s("VolumePrecision"), Tv::Int(2)),
            ],
        ),
    ]
}

/// The rendering options of a new model, in the order of their records.
pub fn default_rendering() -> Vec<(u16, Ro)> {
    use Ro::*;
    let c = |r, g, b, a| Color([r, g, b, a]);
    vec![
        (0x733d, Int(2)),
        (0x733e, Bool(false)),
        (0x733f, Bool(true)),
        (0x7340, Bool(true)),
        (0x7341, Int(1)),
        (0x7342, Int(0)),
        (0x7343, Bool(true)),
        (0x7344, Bool(true)),
        (0x7345, Bool(true)),
        (0x7346, Bool(false)),
        (0x7347, Bool(false)),
        (0x7348, Int(1)),
        (0x7349, Bool(false)),
        (0x7380, Bool(false)),
        (0x7381, Bool(false)),
        (0x734b, Bool(false)),
        (0x734c, Bool(true)),
        (0x734d, Bool(false)),
        (0x734e, Int(8)),
        (0x734f, Bool(true)),
        (0x7350, Int(3)),
        (0x7351, Bool(false)),
        (0x7352, Int(4)),
        (0x7353, Bool(false)),
        (0x7354, Int(5)),
        (0x7355, Bool(false)),
        (0x7356, Bool(false)),
        (0x7357, c(0, 0, 0, 0)),
        (0x7358, c(0, 0, 0, 0)),
        (0x7359, c(255, 255, 0, 255)),
        (0x735a, c(255, 0, 0, 255)),
        (0x735b, c(128, 128, 128, 255)),
        (0x735c, c(225, 225, 200, 255)),
        (0x735d, c(128, 128, 200, 255)),
        (0x735e, Bool(true)),
        (0x735f, Bool(false)),
        (0x7360, c(0, 0, 0, 0)),
        (0x7361, Bool(true)),
        (0x7362, Double(-1.0)),
        (0x7363, Double(-1.0)),
        (0x7364, Int(1)),
        (0x7365, c(140, 165, 255, 255)),
        (0x7366, c(0, 0, 0, 0)),
        (0x7367, c(63, 147, 63, 255)),
        (0x7368, Bool(false)),
        (0x7369, Bool(false)),
        (0x736a, Bool(true)),
        (0x736b, Int(50)),
        (0x736c, Double(0.75)),
        (0x736d, Double(0.5)),
        (0x736e, Bool(false)),
        (0x736f, Bool(false)),
        (0x7370, c(102, 110, 141, 255)),
        (0x7371, c(192, 192, 192, 255)),
        (0x7372, c(255, 0, 0, 255)),
        (0x7373, c(0, 0, 0, 255)),
        // 0x7388, the hatch pattern settings, goes before it (`vff::rendering`).
        (0x738c, Bool(false)),
        (0x7374, Int(4)),
        (0x7375, Int(3)),
        (0x7376, Bool(false)),
        (0x7377, Int(0)),
        (0x7378, Double(0.65)),
        (0x7379, Bool(true)),
        (0x737a, Double(0.349_065_850_398_865_95)),
        (0x737b, Bool(true)),
        (0x737c, Bool(true)),
        (0x737d, Double(1.0)),
        (0x737e, Bool(true)),
        (0x737f, Double(0.8)),
        (0x7382, Bool(true)),
        (0x7383, Bool(false)),
        (0x7385, Float(10.0)),
        (0x7386, Float(0.0)),
        (0x738b, Bool(false)),
        (0x7389, Float(1.0)),
        (0x738a, c(0, 0, 0, 255)),
    ]
}

/// Sets rendering options from Esquisse's (`view_ext["ro"]`, the SDK's names), as
/// `SURenderingOptionsSetValue` does: each name its record, in the record's type.
pub fn apply_rendering(ro: &mut [(u16, Ro)], values: &Value) {
    let Some(map) = values.as_object() else { return };
    let table = crate::native::vff::RENDERING;
    fn set(ro: &mut [(u16, Ro)], tag: u16, j: &Value) {
        let Some(slot) = ro.iter_mut().find(|(t, _)| *t == tag) else { return };
        let f = j.as_f64().or_else(|| j.as_bool().map(|b| b as i32 as f64));
        slot.1 = match slot.1 {
            Ro::Int(_) => match f {
                Some(x) => Ro::Int(x.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32),
                None => return,
            },
            Ro::Bool(_) => Ro::Bool(j.as_bool().unwrap_or(f.is_some_and(|x| x != 0.0))),
            Ro::Double(_) => match f {
                Some(x) => Ro::Double(x),
                None => return,
            },
            Ro::Float(_) => match f {
                Some(x) => Ro::Float(x as f32),
                None => return,
            },
            Ro::Color(_) => match j.as_array() {
                Some(a) if a.len() >= 3 => {
                    let c = |k: usize| a.get(k).and_then(Value::as_u64).unwrap_or(255).min(255) as u8;
                    Ro::Color([c(0), c(1), c(2), c(3)])
                }
                _ => return,
            },
        };
    }
    for (name, j) in map {
        match name.as_str() {
            "DrawHidden" => {
                set(ro, 0x7380, j);
                set(ro, 0x7381, j);
            }
            "DisplaySectionPlanes" | "DisplaySectionCuts" => {
                let bit = if name == "DisplaySectionPlanes" { 1 } else { 2 };
                if let Some((_, Ro::Int(v))) = ro.iter_mut().find(|(t, _)| *t == 0x7375) {
                    if j.as_bool().unwrap_or(false) {
                        *v |= bit;
                    } else {
                        *v &= !bit;
                    }
                }
            }
            _ => {
                if let Some(&(_, tag, _)) = table.iter().find(|r| r.0 == name) {
                    set(ro, tag, j);
                }
            }
        }
    }
}

/// The shadow settings of the model, as `write_shadow` sets them.
fn shadow_of(s: &esq_core::sun::ShadowInfo) -> Shadow {
    let a = s.north_angle.to_radians();
    let clean = |x: f64| if x.abs() < 1e-15 { 0.0 } else { x };
    Shadow {
        time: s.time.clamp(0, u32::MAX as i64) as u32,
        daylight: false,
        city: s.city.clone(),
        country: s.country.clone(),
        longitude: s.longitude,
        latitude: s.latitude,
        tz: s.tz_offset,
        north: [clean(a.sin()), clean(a.cos()), 0.0],
        display: s.display,
        display_north: false,
        on_faces: s.on_faces,
        on_ground: s.on_ground,
        from_edges: s.from_edges,
        light: s.light,
        dark: s.dark,
        sun_shading: s.use_sun_for_shading,
    }
}

/// The name of a material's folder in `materials/`: the characters a file
/// name cannot have become `_`; a name already taken gets `_1`, `_2`…
fn folder_name(name: &str, taken: &mut HashSet<String>) -> String {
    let mut base: String =
        name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    if base.trim().is_empty() {
        base = "_".into();
    }
    let mut out = base.clone();
    let mut k = 1;
    while !taken.insert(out.to_lowercase()) {
        out = format!("{base}_{k}");
        k += 1;
    }
    out
}

/// A file name of a material folder: the texture's own name, without its folders.
fn stored_name(t: &TextureData, taken: &mut HashSet<String>) -> String {
    let mut name = esq_core::meta::base_name(&t.filename);
    let ext = crate::textures::image_ext(&t.image.0, &name);
    let stem = std::path::Path::new(&name).file_stem().and_then(|s| s.to_str()).unwrap_or("texture").to_string();
    let has_ext = std::path::Path::new(&name).extension().is_some();
    if name.trim().is_empty() || !has_ext {
        name = format!("{}.{ext}", if stem.trim().is_empty() { "texture" } else { stem.as_str() });
    }
    let name: String =
        name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let mut out = name.clone();
    let mut k = 1;
    while !taken.insert(out.to_lowercase()) {
        let p = std::path::Path::new(&name);
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("texture");
        let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("png");
        out = format!("{stem}_{k}.{ext}");
        k += 1;
    }
    out
}

/// What SketchUp works out from an image: the average of each channel over all pixels,
/// truncated; grey when the image cannot be decoded.
pub fn average_color(bytes: &[u8]) -> Option<[u8; 3]> {
    // JPEG: the pixels of SketchUp's own decoder (§9 of docs/skp-format.md).
    if crate::native::jpeg::is_jpeg(bytes) {
        return crate::native::jpeg::average(bytes, 512 << 20);
    }
    let r = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let (w, h) = r.into_dimensions().ok()?;
    if w == 0 || h == 0 || w as u64 * h as u64 > 1 << 26 {
        return None;
    }
    let mut r = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(512 << 20);
    r.limits(limits);
    let img = r.decode().ok()?.into_rgba8();
    let n = img.width() as u64 * img.height() as u64;
    let mut sum = [0u64; 3];
    for p in img.pixels() {
        for (s, &c) in sum.iter_mut().zip(p.0.iter()) {
            *s += c as u64;
        }
    }
    Some(sum.map(|s| (s as f64 / n as f64) as u8))
}

fn tex(t: &TextureData, taken: &mut HashSet<String>) -> Tex {
    let px = if t.px == [0, 0] { crate::native::xml::image_size(&t.image.0).unwrap_or([0, 0]) } else { t.px };
    Tex {
        file_name: esq_core::meta::base_name(&t.filename),
        stored: stored_name(t, taken),
        image: t.image.0.clone(),
        px,
        size: [t.size[0].max(1e-6) / IN, t.size[1].max(1e-6) / IN],
        avg: average_color(&t.image.0).unwrap_or(t.avg),
    }
}

/// Persistent ids, given in the order SketchUp gives them.
struct Ids(Id);

impl Ids {
    fn next(&mut self) -> Id {
        self.0 += 1;
        self.0
    }
}

struct P<'m> {
    model: &'m Model,
    ids: Ids,
    materials: HashMap<MaterialId, Id>,
    layers: Vec<Id>,
    defs: HashMap<DefId, Id>,
    /// Definitions made, by Esquisse id, and the order they were finished in.
    made: Vec<Def>,
    /// Definitions being made (a definition that contains itself is cut there).
    open: Vec<DefId>,
    folders_taken: HashSet<String>,
    /// Image materials made for image definitions (owned).
    owned: Vec<Mat>,
    fonts: [Id; 2],
    /// The fonts to write and the id each rank has (ranks 0 and 1: ids 2 and 3).
    font_set: crate::native::fonts::Fonts,
    font_ids: Vec<Id>,
    /// The hatch patterns' ids, by uid and by name.
    hatches: HashMap<u32, Id>,
    hatch_names: HashMap<String, Id>,
    /// The id and class each entity of each collection got (collection, kind of handle, key),
    /// for the points annotations are attached to (`Conn`).
    ids_of: HashMap<(Option<DefId>, char, u64), (Id, &'static str)>,
}

/// Builds the plan of a model: `view` is the camera it opens with.
pub fn plan(model: &Model, view: Option<&crate::View>) -> Result<Plan> {
    super::on_big_stack(|| plan_here(model, view))?
}

pub(super) fn plan_here(model: &Model, view: Option<&crate::View>) -> Result<Plan> {
    // Ids 1 to 17: the style, the fonts, the default tag and its colour, the line styles.
    let mut p = P {
        model,
        ids: Ids(17),
        materials: HashMap::new(),
        layers: Vec::new(),
        defs: HashMap::new(),
        made: Vec::new(),
        open: Vec::new(),
        folders_taken: HashSet::new(),
        owned: Vec::new(),
        fonts: [2, 3],
        font_set: crate::native::fonts::of_model(model),
        font_ids: vec![2, 3],
        hatches: HashMap::new(),
        hatch_names: HashMap::new(),
        ids_of: HashMap::new(),
    };
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs().min(u32::MAX as u64) as u32);

    // Model Info options: each provider SketchUp has, each value in the type it has there.
    let mut options = default_options();
    for (prov, values) in &model.meta.options {
        let Some(slot) = options.iter_mut().find(|(n, _)| n == prov) else { continue };
        for (k, j) in values {
            let pos = slot.1.iter().position(|(n, _)| n == k);
            let Some(tv) = option_tv(j, pos.map(|i| &slot.1[i].1)) else { continue };
            match pos {
                Some(i) => slot.1[i].1 = tv,
                None => slot.1.push((k.clone(), tv)),
            }
        }
    }
    let unit = model.units.code() as i32;
    {
        let (_, units) = options.iter_mut().find(|(n, _)| n == "UnitsOptions").expect("UnitsOptions");
        let int = |units: &Vec<(String, Tv)>, k: &str| {
            units.iter().find(|(n, _)| n == k).and_then(|(_, v)| if let Tv::Int(i) = v { Some(*i) } else { None })
        };
        let mut precision = match model.units {
            esq_core::model::Units::Millimeters => 0,
            esq_core::model::Units::Centimeters => 1,
            esq_core::model::Units::Meters => 2,
            _ => 3,
        };
        let mut format = 0;
        let o = model.meta.options.get("UnitsOptions");
        let given = |k: &str| o.and_then(|o| o.get(k)).and_then(Value::as_i64).map(|v| v as i32);
        // The unit the options read give the model (the reader's rule, `SUModelGetUnits`: the
        // architectural and fractional formats are in inches, engineering in feet): while it is
        // the model's, the options are written as read.
        let read_unit = match given("LengthFormat") {
            Some(1 | 3) => Some(0),
            Some(2) => Some(1),
            _ => given("LengthUnit"),
        };
        let mut unit = unit;
        if read_unit == Some(unit) {
            format = given("LengthFormat").unwrap_or(format);
            precision = given("LengthPrecision").unwrap_or(precision);
            unit = given("LengthUnit").unwrap_or(unit);
        }
        let _ = int;
        for (k, v) in [("LengthFormat", format), ("LengthUnit", unit), ("LengthPrecision", precision)] {
            match units.iter_mut().find(|(n, _)| n == k) {
                Some(slot) => slot.1 = Tv::Int(v),
                None => units.push((k.to_string(), Tv::Int(v))),
            }
        }
    }

    let mut rendering = default_rendering();
    if let Some(ro) = model.view_ext.get("ro") {
        apply_rendering(&mut rendering, ro);
    }

    let camera = match view {
        Some(v) => camera_of(v, 1.0, 1000.0)?,
        None => Camera {
            eye: [0.0, 0.0, 500.0],
            target: [0.0; 3],
            up: [0.0, 1.0, 0.0],
            perspective: true,
            fov: 30.0,
            near: 1.0,
            far: 1000.0,
            height: 133.0,
        },
    };

    let ax = &model.axes;
    let axes = [
        fin(ax.w_axis.x / IN, "axes")?,
        fin(ax.w_axis.y / IN, "axes")?,
        fin(ax.w_axis.z / IN, "axes")?,
        fin(ax.x_axis.x, "axes")?,
        fin(ax.x_axis.y, "axes")?,
        fin(ax.x_axis.z, "axes")?,
        fin(ax.y_axis.x, "axes")?,
        fin(ax.y_axis.y, "axes")?,
        fin(ax.y_axis.z, "axes")?,
        fin(ax.z_axis.x, "axes")?,
        fin(ax.z_axis.y, "axes")?,
        fin(ax.z_axis.z, "axes")?,
    ];

    let line_styles: Vec<LineStyle> = LINE_STYLES
        .iter()
        .enumerate()
        .map(|(i, (name, pattern))| LineStyle { id: 6 + i as Id, name, pattern })
        .collect();

    // Hatch patterns (2026), before what refers to them.
    let hatch_patterns = hatch_patterns_of(model, &mut p);
    let ro_hatch = model
        .view_ext
        .get("ro_hatch")
        .and_then(Value::as_str)
        .and_then(|n| hatch_patterns.iter().find(|h| h.name == n))
        .map(|h| h.settings.clone())
        .or_else(|| model.view_ext.get("skp_ro_hatch").and_then(|v| serde_json::from_value(v.clone()).ok()))
        .unwrap_or_else(|| default_ro_hatch(&rendering));

    // Materials, in the model's order (an image's material is made with its image).
    p.folders_taken.insert("layer_layer0".into());
    let mut materials = Vec::new();
    for (mid, m) in model.materials.iter() {
        if m.ext.owner_image {
            continue;
        }
        let id = p.ids.next();
        let mat = p.material(id, m, false)?;
        p.materials.insert(mid, id);
        materials.push(mat);
    }

    // Tags: the default one keeps its ids (4, its colour 5).
    let mut layers = Vec::new();
    for (i, l) in model.layers.iter().enumerate() {
        let (id, mid) = if i == 0 { (4, 5) } else { (p.ids.next(), p.ids.next()) };
        let name = if i == 0 { "Layer0".to_string() } else { l.name.clone() };
        let color = l.color.unwrap_or(if i == 0 { LAYER0_COLOR } else { [0, 0, 0] });
        let folder = if i == 0 {
            "Layer_Layer0".to_string()
        } else {
            folder_name(&format!("Layer_{name}"), &mut p.folders_taken)
        };
        let material = Mat {
            id: mid,
            name: format!("Layer_{name}"),
            folder,
            owned: true,
            of_layer: true,
            color,
            kind: 0,
            colorize_type: 0,
            trans: 0.5,
            use_trans: false,
            workflow: 0,
            texture: None,
            pbr: default_pbr(),
            dicts: Vec::new(),
            thumbnail: None,
            hatch: None,
        };
        let line_style = (i != 0 && !l.line_style.is_empty())
            .then(|| line_styles.iter().find(|s| s.name == l.line_style).map(|s| s.id))
            .flatten();
        layers.push(Layer {
            id,
            name,
            hidden: i != 0 && !l.visible,
            material,
            page_behavior: l.page_behavior,
            line_style,
            dicts: {
                let o = format!("layer:{}", l.name);
                model.attr_store.get(&o).map(|a| dicts_with(model, &o, a)).unwrap_or_default()
            },
            colorless: i != 0 && l.color.is_none(),
        });
        p.layers.push(id);
    }

    // Folders: a tree, parents first.
    let mut folder_ids: HashMap<u32, Id> = HashMap::new();
    for f in &model.meta.folders {
        folder_ids.insert(f.uid, p.ids.next());
    }
    let folders = folder_tree(model, &folder_ids, &p.layers);
    let active_layer = match model.layer_index_of_uid(model.meta.active_layer) {
        Some(i) if model.meta.active_layer != 0 => p.layers[i],
        _ => 4,
    };

    // Entities (definitions are made where they are first placed).
    let mut root = p.entities(&model.root, None, 0)?;
    // The components nothing places (Window › Components « In Model » keeps them until they are
    // purged): SketchUp keeps them in the file. Groups and images go with their last instance.
    let unplaced: Vec<DefId> =
        model.defs.iter().filter(|(_, d)| !d.is_group && d.ext.image.is_none()).map(|(k, _)| k).collect();
    for d in unplaced {
        p.definition(d, 0)?;
    }
    root.el.id = 0;
    root.el.dicts = model.skp_model_attrs().map(|a| dicts_with(model, "model", &a)).unwrap_or_default();
    root.bbox = None;

    // Environments (2021+ only).
    let (environments, current_env) = environments_of(model, &mut p.ids);
    // Scenes.
    let mut scenes = Vec::new();
    let mut photos = Vec::new();
    let shadow = shadow_of(&model.shadow);
    for sc in &model.scenes {
        let id = p.ids.next();
        let pr = &sc.ext.props;
        let rendering = pr.get("ro").map(|ro| {
            let mut r = default_rendering();
            apply_rendering(&mut r, ro);
            r
        });
        // As the SDK writer: the camera, the hidden tags and the style set what the scene
        // keeps, then the flags Esquisse has (`SUSceneSetFlags`) replace them.
        let mut flags = 0x01;
        if sc.hidden_layers.is_some() {
            flags |= 0x20;
        }
        if rendering.is_some() {
            flags |= 0x02;
        }
        if let Some(f) = pr.get("flags").and_then(Value::as_u64) {
            flags = f as u32;
        }
        let hidden_layers = sc
            .hidden_layers
            .as_ref()
            .map(|h| h.iter().filter(|&&l| l != 0 && l < p.layers.len()).map(|&l| p.layers[l]).collect())
            .unwrap_or_default();
        scenes.push(Page {
            id,
            name: sc.name.clone(),
            description: sc.description.clone(),
            flags,
            camera: camera_of(&sc.camera, 0.1, 2.0)?,
            rendering,
            shadow: (flags & 4 != 0).then(|| shadow.clone()),
            axes: (flags & 8 != 0).then_some(axes),
            hidden_layers,
            animate: pr.get("animate").and_then(Value::as_bool) != Some(false),
            dicts: {
                let o = crate::model_info::scene_owner(sc);
                model.attr_store.get(&o).map(|a| dicts_with(model, &o, a)).unwrap_or_default()
            },
            style: None,
            photo: sc.photo.as_ref().map(|m| {
                let mut ph = photo_of(m, pr.get("skp_photo"), time);
                ph.id = p.ids.next();
                let id = ph.id;
                photos.push(ph);
                id
            }),
            transition: pr
                .get("transition")
                .and_then(Value::as_f64)
                .filter(|t| t.is_finite() && *t >= 0.0)
                .unwrap_or(-1.0),
            delay: pr.get("delay").and_then(Value::as_f64).filter(|t| t.is_finite() && *t >= 0.0).unwrap_or(-1.0),
            v7153: pr.get("skp_page").and_then(|v| v["v7153"].as_str()).unwrap_or_default().to_string(),
            v7157: pr.get("skp_page").and_then(|v| v["v7157"].as_bool()).unwrap_or(true),
            thumbnail: model.files.get(&crate::native::build::scene_thumbnail_key(sc.ext.uid)).map(|b| b.0.clone()),
            env_id: pr
                .get("env")
                .and_then(Value::as_str)
                .filter(|_| flags & 0x200 != 0)
                .and_then(|n| environments.iter().find(|e| e.name == n))
                .map(|e| e.id),
            env: (flags & 0x200 != 0).then(|| scene_env(model, pr)).flatten(),
            ro_hatch: pr.get("skp_ro_hatch").and_then(|v| serde_json::from_value(v.clone()).ok()),
        });
    }

    // Styles: the model's list (the first takes the id of the default style), their
    // watermarks, the style each scene keeps.
    let mut styles = crate::native::style::from_view_ext(
        model.view_ext.get("styles"),
        model.view_ext.get("watermarks"),
        model.view_ext.get("ro"),
    );
    for (i, s) in styles.list.iter_mut().enumerate() {
        s.id = if i == 0 { 1 } else { p.ids.next() };
    }
    for w in styles.watermarks.iter_mut().flatten() {
        w.id = p.ids.next();
    }
    let manager: Vec<(String, Id)> = styles.watermarks.iter().flatten().map(|w| (w.name.clone(), w.id)).collect();
    // A style's watermark is the model's when they are the same (one object in an MFC archive).
    let models_wms: Vec<crate::native::style::Wm> = styles.watermarks.iter().flatten().cloned().collect();
    for s in styles.list.iter_mut().chain(styles.active.iter_mut()) {
        for it in &mut s.items {
            if let crate::native::style::ItemV::Watermarks(l) = &mut it.v {
                for w in l.iter_mut().flatten() {
                    let same = |m: &&crate::native::style::Wm| {
                        crate::native::style::Wm { id: 0, ..(*m).clone() }
                            == crate::native::style::Wm { id: 0, ..w.clone() }
                    };
                    w.id = models_wms.iter().find(same).map_or_else(|| p.ids.next(), |m| m.id);
                }
            }
        }
    }
    for s in &mut styles.list {
        s.watermark_ids = s
            .items
            .iter()
            .filter_map(|i| match &i.v {
                crate::native::style::ItemV::Watermarks(l) => Some(l),
                _ => None,
            })
            .flatten()
            .flatten()
            .filter_map(|w| manager.iter().find(|(n, _)| *n == w.name).map(|(_, id)| *id))
            .collect();
    }
    for (page, sc) in scenes.iter_mut().zip(&model.scenes) {
        let name = sc.ext.props.get("style").and_then(Value::as_str);
        page.style = name.and_then(|n| styles.list.iter().find(|s| s.name == n)).map(|s| s.id);
    }
    let selected_scene = model
        .view_ext
        .get(esq_core::model::ACTIVE_SCENE)
        .and_then(Value::as_u64)
        .and_then(|i| scenes.get(i as usize))
        .map(|s| s.id);
    // The active matched photo: that of the selected scene (SketchUp shows it), else none.
    let active_photo = selected_scene.and_then(|id| scenes.iter().find(|s| s.id == id)).and_then(|s| s.photo);

    materials.extend(std::mem::take(&mut p.owned));
    // The fonts (ids for the ranks the styles name too), the styles' fonts by id.
    let ts = p.font_set.text_style.clone();
    let ds = p.font_set.dim_style.clone();
    let text_style = crate::native::doc::TextStyle {
        font: ts.font.map(|r| p.font_id(r as usize)),
        font2: ts.font2.map(|r| p.font_id(r as usize)),
        ..ts
    };
    let dim_style = crate::native::doc::DimStyle { font: ds.font.map(|r| p.font_id(r as usize)), ..ds };
    let font_list: Vec<crate::native::doc::Font> = (0..p.font_set.list.len())
        .map(|r| crate::native::doc::Font { id: p.font_id(r), ..p.font_set.list[r].clone() })
        .collect();
    Ok(Plan {
        guid: new_guid(),
        name: model.meta.name.clone(),
        description: model.meta.description.clone(),
        unit,
        options,
        camera,
        rendering,
        shadow,
        axes,
        line_styles,
        materials,
        layers,
        active_layer,
        folders,
        fonts: p.fonts,
        font_list,
        text_style,
        dim_style,
        style: 1,
        styles,
        schemas: schemas_of(model),
        photos,
        active_photo,
        environments,
        current_env,
        hatch_patterns,
        ro_hatch,
        model_flags: model
            .view_ext
            .get("skp_model")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        defs: std::mem::take(&mut p.made),
        root,
        model_dicts: Vec::new(),
        scenes,
        selected_scene,
        next_id: p.ids.0 + 1,
        time,
        thumbnail: None,
    })
}

/// The hatch patterns as the file keeps them (`ModelMeta::hatch_patterns`), each with an id
/// and a folder of its own; their settings as read while the pattern is the one read
/// (`build::hatch_xml_key`), else made from Esquisse's.
fn hatch_patterns_of(model: &Model, p: &mut P) -> Vec<crate::native::doc::HatchPattern> {
    let mut taken = HashSet::new();
    let mut out = Vec::new();
    for h in &model.meta.hatch_patterns {
        if p.hatch_names.contains_key(&h.name) {
            continue;
        }
        let id = p.ids.next();
        p.hatches.insert(h.uid, id);
        p.hatch_names.insert(h.name.clone(), id);
        out.push(crate::native::doc::HatchPattern {
            id,
            folder: folder_name(&h.name, &mut taken),
            name: h.name.clone(),
            settings: hatch_settings(model, h),
            thumbnail: model.files.get(&crate::native::build::hatch_thumbnail_key(h.uid)).map(|b| b.0.clone()),
            xml: None,
        });
    }
    out
}

/// A hatch pattern's settings: those read (exact floats) while Esquisse's pattern is still
/// what they make, else Esquisse's (a .pat as SketchUp's floats, `esq_core::hatch::pat_data`).
fn hatch_settings(model: &Model, h: &esq_core::meta::HatchPattern) -> crate::native::doc::HatchSettings {
    use crate::native::doc::HatchSettings;
    if let Some(x) = model.files.get(&crate::native::build::hatch_xml_key(h.uid))
        && let Ok(Some((mut s, _))) = crate::native::vff::hatch_xml_settings(&x.0)
    {
        if h.image {
            s.image = Some(h.data.0.clone());
        }
        if crate::native::build::hatch_of(h.uid, &h.name, &s) == *h {
            return s;
        }
    }
    let mut s = HatchSettings {
        color: [h.color[0], h.color[1], h.color[2], 255],
        scale: [h.size[0] as f32, h.size[1] as f32],
        rotation: h.rotation as f32,
        ..Default::default()
    };
    if h.image {
        s.image = Some(h.data.0.clone());
        let ext = crate::native::xml::image_extension(&h.data.0).unwrap_or("png");
        let base = esq_core::meta::base_name(&h.source);
        s.texture_name =
            if base.trim().is_empty() || base.contains(['/', '\\']) { format!("hatch.{ext}") } else { base };
    } else if let Ok((_, lines)) = esq_core::hatch::parse_pat(&String::from_utf8_lossy(&h.data.0)) {
        let bytes = esq_core::photo::base64_decode(&esq_core::hatch::pat_data(&lines)).unwrap_or_default();
        s.pat = Some(bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect());
        s.pat_name = h.source.clone();
    }
    s
}

/// The settings of rendering options without a hatch pattern (`build::default_ro_hatch`).
fn default_ro_hatch(ro: &[(u16, Ro)]) -> crate::native::doc::HatchSettings {
    let color = ro.iter().find_map(|(t, v)| match v {
        Ro::Color(c) if *t == 0x7373 => Some(*c),
        _ => None,
    });
    crate::native::build::default_ro_hatch(color.unwrap_or([0, 0, 0, 255]))
}

/// The model's environments as a 2021+ file keeps them (`view_ext["envs"]`, the panorama in
/// `Model::files["env:<id>"]` — a `.ske` gives the one of its `ref/`), each in a folder of its
/// own, and the current one.
fn environments_of(model: &Model, ids: &mut Ids) -> (Vec<crate::native::doc::Environment>, Option<Id>) {
    let Some(list) = model.view_ext.get("envs").and_then(Value::as_array) else { return (Vec::new(), None) };
    let current = model.view_ext.get("env_current").and_then(Value::as_i64);
    let mut taken = HashSet::new();
    let (mut out, mut cur) = (Vec::new(), None);
    for e in list {
        let Some(id) = e["id"].as_i64() else { continue };
        let text = |k: &str| e[k].as_str().unwrap_or_default().to_string();
        let num = |k: &str, d: f64| e[k].as_f64().filter(|x| x.is_finite()).unwrap_or(d);
        let flag = |k: &str| e[k].as_bool().unwrap_or(false);
        let name = text("name");
        // The panorama: the file kept with the model, the one inside a `.ske`.
        let mut image_name = text("skp_file");
        let mut image = model.files.get(&format!("env:{id}")).map(|b| b.0.clone());
        if let Some(b) = image.clone().filter(|b| b.starts_with(b"PK\x03\x04"))
            && let Ok(a) = esq_core::zip::Archive::open(&b)
        {
            let budget = esq_core::untrusted::Budget::for_input(b.len());
            image = None;
            if let Some(entry) = a
                .entries
                .iter()
                .find(|x| x.name.starts_with("ref/") && (x.name.ends_with(".hdr") || x.name.ends_with(".exr")))
                && let Ok(data) = a.read(entry, &budget)
            {
                image_name = entry.name.trim_start_matches("ref/").to_string();
                image = Some(Arc::new(data));
            }
        }
        if image_name.is_empty() || image_name.contains(['/', '\\']) {
            let exr = image.as_ref().is_some_and(|b| b.starts_with(&[0x76, 0x2f, 0x31, 0x01]));
            image_name = format!("environment.{}", if exr { "exr" } else { "hdr" });
        }
        let wanted = {
            let f = text("skp_folder");
            if f.is_empty() { name.clone() } else { f }
        };
        let folder = folder_name(&wanted, &mut taken);
        let pid = ids.next();
        if current == Some(id) {
            cur = Some(pid);
        }
        let sun = e["sun_pos"].as_array();
        let sp = |i: usize, d: f64| {
            sun.and_then(|a| a.get(i)).and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(d)
        };
        out.push(crate::native::doc::Environment {
            id: pid,
            folder,
            name,
            description: text("description"),
            reflections: flag("reflections"),
            skydome: flag("skydome"),
            linked_sun: flag("linked_sun"),
            rotation: num("rotation", 0.0),
            skydome_exposure: num("skydome_exposure", 1.0),
            reflection_exposure: num("reflection_exposure", 1.0),
            sun: [sp(0, 0.0) / 360.0, sp(1, 45.0) / 90.0],
            image_name,
            image,
            thumbnail: model.files.get(&format!("thumb:env:{id}")).map(|b| b.0.clone()),
        });
    }
    (out, cur)
}

/// The environment settings a scene keeps: as read (`skp_env`), else those of the environment
/// it names (`env`), else none.
fn scene_env(model: &Model, pr: &BTreeMap<String, Value>) -> Option<crate::native::doc::SceneEnv> {
    if let Some(e) = pr.get("skp_env").and_then(|v| serde_json::from_value(v.clone()).ok()) {
        return Some(e);
    }
    let name = pr.get("env")?.as_str()?;
    let e = model.view_ext.get("envs")?.as_array()?.iter().find(|e| e["name"].as_str() == Some(name))?;
    let num = |k: &str, d: f64| e[k].as_f64().filter(|x| x.is_finite()).unwrap_or(d) as f32;
    let sun = e["sun_pos"].as_array();
    let sp = |i: usize, d: f64| sun.and_then(|a| a.get(i)).and_then(Value::as_f64).unwrap_or(d);
    Some(crate::native::doc::SceneEnv {
        reflections: e["reflections"].as_bool().unwrap_or(false),
        skydome: e["skydome"].as_bool().unwrap_or(false),
        rotation: num("rotation", 0.0),
        skydome_exposure: num("skydome_exposure", 1.0),
        reflection_exposure: num("reflection_exposure", 1.0),
        linked_sun: e["linked_sun"].as_bool().unwrap_or(false),
        sun: [(sp(0, 0.0) / 360.0) as f32, (sp(1, 45.0) / 90.0) as f32],
    })
}

/// A scene's matched photo as SketchUp keeps it: as read (`skp_photo`, `build`) while its image
/// and the lines drawn on it are those read; else from Esquisse's setup (the ends of the lines
/// in units of the image's height; an axis without lines: none).
fn photo_of(m: &esq_core::photo::MatchPhoto, read: Option<&Value>, time: u32) -> crate::native::doc::MatchPhoto {
    let h = (m.size[1].max(1)) as f64;
    let mut points = Vec::new();
    for l in m.setup.lines.iter().flatten() {
        for line in l {
            points.push([line.a.x / h, line.a.y / h, 0.0]);
            points.push([line.b.x / h, line.b.y / h, 0.0]);
        }
    }
    let raw: Option<crate::native::doc::MatchPhoto> = read.and_then(|v| serde_json::from_value(v.clone()).ok());
    let same = |r: &crate::native::doc::MatchPhoto| {
        r.size == m.size
            && r.points.len() == points.len()
            && r.points.iter().zip(&points).all(|(a, b)| (0..2).all(|k| (a[k] - b[k]).abs() < 1e-9))
    };
    let mut out = match raw {
        Some(r) if same(&r) => r,
        _ => crate::native::doc::MatchPhoto {
            path: m.name.clone(),
            size: m.size,
            time,
            flag: true,
            value: 1.0,
            points,
            offset: [m.setup.origin.x / h, m.setup.origin.y / h, 0.0],
            ..Default::default()
        },
    };
    out.image = Some(m.image.0.clone());
    out
}

/// The schemas of the model, with their files (`esq_core::classify::file_key`): one without
/// its file cannot be written (SketchUp loads a schema from its file).
fn schemas_of(model: &Model) -> Vec<crate::native::doc::SchemaFile> {
    esq_core::classify::schemas(model)
        .into_iter()
        .filter_map(|s| {
            let data = model.files.get(&esq_core::classify::file_key(&s.name))?.0.clone();
            Some(crate::native::doc::SchemaFile { path: s.path, name: s.name, data })
        })
        .collect()
}

fn default_pbr() -> Pbr {
    Pbr {
        metallic: 1.0,
        roughness_factor: 1.0,
        normal_scale: 1.0,
        normal_style: 1,
        ao_strength: 1.0,
        ..Default::default()
    }
}

fn camera_of(v: &crate::View, near: f64, far: f64) -> Result<Camera> {
    let eye = pt(v.eye, "caméra")?;
    let target = pt(v.target, "caméra")?;
    // An up that leans along the line of sight is set square to it, as SketchUp sets a
    // camera's orientation (`SUCameraSetOrientation`); one already square is kept as it is.
    let d = (v.target - v.eye).normalize_or_zero();
    let lean = v.up.dot(d);
    let square = v.up - d * lean;
    let up = if lean.abs() > 1e-9 && square.length() > 1e-9 { square.normalize() } else { v.up };
    let up = vec3(up, "caméra")?;
    let fov = if v.perspective { fin(v.fov_deg, "caméra")? } else { 30.0 };
    let dist = (v.eye - v.target).length() / IN;
    let height = if v.perspective { 2.0 * dist * (fov.to_radians() / 2.0).tan() } else { dist };
    Ok(Camera { eye, target, up, perspective: v.perspective, fov, near, far, height: fin(height, "caméra")? })
}

fn folder_tree(model: &Model, ids: &HashMap<u32, Id>, layers: &[Id]) -> Folder {
    fn fill(model: &Model, ids: &HashMap<u32, Id>, layers: &[Id], parent: Option<u32>, depth: usize) -> Vec<Folder> {
        if depth > 64 {
            return Vec::new();
        }
        let known = |u: u32| model.meta.folders.iter().any(|f| f.uid == u);
        model
            .meta
            .folders
            .iter()
            .filter(|f| match (f.parent, parent) {
                (None, None) => true,
                // A folder whose parent is gone goes to the top.
                (Some(p), None) => !known(p),
                (Some(p), Some(q)) => p == q && p != f.uid,
                _ => false,
            })
            .map(|f| Folder {
                id: ids[&f.uid],
                name: f.name.clone(),
                hidden: !f.visible,
                hidden_on_new_pages: !f.visible_on_new_pages,
                folders: fill(model, ids, layers, Some(f.uid), depth + 1),
                layers: model
                    .layers
                    .iter()
                    .enumerate()
                    .skip(1)
                    .filter(|(_, l)| l.folder == Some(f.uid))
                    .map(|(i, _)| layers[i])
                    .collect(),
            })
            .collect()
    }
    Folder { id: 0, folders: fill(model, ids, layers, None, 0), ..Default::default() }
}

impl P<'_> {
    /// The id of the font of rank `r` (ranks 0 and 1 have ids 2 and 3, the next ones the
    /// next ids free).
    fn font_id(&mut self, r: usize) -> Id {
        while self.font_ids.len() <= r {
            let id = self.ids.next();
            self.font_ids.push(id);
        }
        self.font_ids[r]
    }

    /// The font and the material (its colour) of an annotation.
    fn annot_font(&mut self, e: &Entities, key: &str) -> (Option<Id>, Option<Id>) {
        let font = self.font_set.of_props(&|k| e.prop(key, k).cloned()).map(|r| self.font_id(r));
        let mat = match e.prop(key, "material") {
            Some(AttrValue::Text(n)) => self.model.find_material(n).and_then(|m| self.materials.get(&m).copied()),
            _ => None,
        };
        (font, mat)
    }

    fn material(&mut self, id: Id, m: &esq_core::model::Material, owned: bool) -> Result<Mat> {
        let e = &m.ext;
        let texture = e.texture.as_ref().map(|t| {
            let mut taken = HashSet::new();
            tex(t, &mut taken)
        });
        let kind = match &texture {
            None => 0,
            Some(_) if e.colorize_deltas != [0.0; 3] => 2,
            Some(_) => 1,
        };
        let pb = &e.pbr;
        let pbr_on =
            e.workflow == 1 || pb.metalness_enabled || pb.roughness_enabled || pb.normal_enabled || pb.ao_enabled;
        let mut pbr = default_pbr();
        if pbr_on {
            let mut taken: HashSet<String> = texture.iter().map(|t| t.stored.to_lowercase()).collect();
            let mut t = |x: &Option<TextureData>| x.as_ref().map(|x| tex(x, &mut taken));
            pbr.metallic_texture = t(&pb.metallic_texture);
            pbr.roughness_texture = t(&pb.roughness_texture);
            pbr.normal_texture = t(&pb.normal_texture);
            pbr.ao_texture = t(&pb.ao_texture);
            pbr.metalness = pb.metalness_enabled;
            pbr.metallic = pb.metallic_factor as f32;
            pbr.roughness = pb.roughness_enabled;
            pbr.roughness_factor = pb.roughness_factor as f32;
            pbr.roughness_inverted = pb.roughness_inverted;
            // Normal and occlusion maps cannot be turned on without their image.
            if pbr.normal_texture.is_some() {
                pbr.normal = pb.normal_enabled;
                pbr.normal_scale = pb.normal_scale as f32;
                pbr.normal_style = pb.normal_style as i32;
            }
            if pbr.ao_texture.is_some() {
                pbr.ao = pb.ao_enabled;
                pbr.ao_strength = pb.ao_strength as f32;
            }
        }
        let opacity = m.opacity as f64;
        let workflow =
            if pbr_on && (pbr.metalness || pbr.roughness || pbr.normal || pbr.ao || e.workflow == 1) { 1 } else { 0 };
        Ok(Mat {
            id,
            name: m.name.clone(),
            folder: folder_name(&m.name, &mut self.folders_taken),
            owned,
            of_layer: false,
            color: m.color,
            kind,
            colorize_type: if kind == 2 { e.colorize_type as i32 } else { if texture.is_some() { 1 } else { 0 } },
            trans: 1.0 - opacity.clamp(0.0, 1.0),
            use_trans: opacity < 1.0,
            workflow,
            texture,
            pbr,
            dicts: {
                let o = format!("mat:{}", m.name);
                self.model.attr_store.get(&o).map(|a| dicts_with(self.model, &o, a)).unwrap_or_default()
            },
            thumbnail: self.model.files.get(&crate::native::build::mat_thumbnail_key(&m.name)).map(|b| b.0.clone()),
            hatch: e.hatch.and_then(|u| self.hatches.get(&u)).copied(),
        })
    }

    /// What SketchUp keeps of an attached end of an annotation of collection `ctx`, now at `at`
    /// (mm, in `ctx`): its type, the entity and the path by the ids they got in the plan (the
    /// collections a path leads to are planned before the annotations, `entities`), and the
    /// parameter of the types that have one (5 the fraction along the edge,
    /// from its start as written; 6 inches along the line; 7 the fractions of the face's extent
    /// in the frame of its first edge use, as written). None for what SketchUp has no type of
    /// (the point of an arc: a radial dimension keeps its arc).
    fn conn(&self, ctx: Option<DefId>, a: &esq_core::attach::Attach, at: DVec3) -> Option<Conn> {
        use esq_core::attach::AttachTo as T;
        let (leaf_ctx, ents, m) = self.model.attach_leaf(ctx, a)?;
        let mut path = Vec::new();
        let mut c = ctx;
        for &i in &a.path {
            path.push(*self.ids_of.get(&(c, 'i', i.data().as_ffi()))?);
            c = Some(self.model.entities_get(c)?.instances.get(i)?.def);
        }
        let get = |k: char, key: u64| self.ids_of.get(&(leaf_ctx, k, key)).copied();
        let inch = |p: DVec3| (p / IN).to_array();
        let local = m.inverse().transform_point3(at);
        let (kind, leaf, point) = match a.to {
            T::Vertex(v) => (2, get('v', v.data().as_ffi()), inch(at)),
            T::GuidePoint(g) => (3, get('g', g.data().as_ffi()), inch(at)),
            T::EdgeMiddle(e) => (4, get('e', e.data().as_ffi()), inch(at)),
            T::OnEdge(e, t) => (5, get('e', e.data().as_ffi()), [t, 0.0, 0.0]),
            T::OnGuide(g, d) => (6, get('g', g.data().as_ffi()), [d / IN, 0.0, 0.0]),
            T::OnFace(f, ..) => {
                let (u, v) = esq_core::attach::face_fractions(ents, f, local)?;
                (7, get('f', f.data().as_ffi()), [u, v, 0.0])
            }
            T::ArcCenter(e) => {
                let (eid, _) = get('e', e.data().as_ffi())?;
                (10, self.ids_of.get(&(leaf_ctx, 'c', eid)).copied(), inch(at))
            }
            T::Section(s, p) => (11, get('s', s.data().as_ffi()), inch(p)),
            T::Origin if !path.is_empty() => return Some(Conn { kind: 12, point: [0.0; 3], leaf: None, path }),
            T::Origin | T::OnArc(..) => return None,
        };
        point.iter().all(|x| x.is_finite()).then_some(())?;
        Some(Conn { kind, point, leaf: Some(leaf?), path })
    }

    fn layer_of(&self, l: LayerIdx) -> Option<Id> {
        (l != 0).then(|| self.layers.get(l).copied()).flatten()
    }

    fn el(&mut self, layer: LayerIdx, hidden: bool, props: Option<&BTreeMap<String, AttrValue>>) -> El {
        let mut f = flags::CASTS | flags::RECEIVES;
        if hidden {
            f |= flags::HIDDEN;
        }
        if let Some(p) = props {
            if matches!(p.get("casts"), Some(AttrValue::Bool(false))) {
                f &= !flags::CASTS;
            }
            if matches!(p.get("receives"), Some(AttrValue::Bool(false))) {
                f &= !flags::RECEIVES;
            }
        }
        El { id: self.ids.next(), material: None, layer: self.layer_of(layer), flags: f, dicts: Vec::new(), tex: None }
    }

    fn mat(&self, m: Option<MaterialId>) -> Option<Id> {
        m.and_then(|m| self.materials.get(&m).copied())
    }

    /// An entities collection (the root, or a definition's: `ctx`).
    fn entities(&mut self, e: &Entities, ctx: Option<DefId>, depth: usize) -> Result<Ents> {
        let mut out = Ents::default();
        out.el.flags = flags::CASTS | flags::RECEIVES;
        let ctx_ffi = ctx.map_or(0, |d| d.data().as_ffi());
        let store = |kind: char, ffi: u64| crate::attr_owner(kind, ctx, ffi);
        // The ids of faces and instances, for the relationships.
        let mut face_ids: HashMap<esq_core::model::FaceId, Id> = HashMap::new();
        let mut inst_ids: HashMap<esq_core::model::InstanceId, Id> = HashMap::new();

        // Edges, their vertices as they are first met (SketchUp's order of ids).
        let mut vids: HashMap<VertexId, Id> = HashMap::new();
        let mut edge_ids: HashMap<EdgeId, Id> = HashMap::new();
        for (id, edge) in e.edges.iter() {
            let ffi = id.data().as_ffi();
            let key = format!("e{ffi}");
            let mut el = self.el(edge.layer, edge.hidden, e.props.get(&key));
            if edge.soft {
                el.flags |= flags::SOFT;
            }
            if edge.smooth {
                el.flags |= flags::SMOOTH;
            }
            if let Some(AttrValue::Text(n)) = e.prop(&key, "material") {
                el.material = self.model.find_material(n).and_then(|m| self.mat(Some(m)));
            }
            let o = store('e', ffi);
            el.dicts = self.model.attr_store.get(&o).map(|a| dicts_with(self.model, &o, a)).unwrap_or_default();
            let mut ends = [0; 2];
            for (k, v) in edge.v.iter().enumerate() {
                let Some(vx) = e.verts.get(*v) else {
                    return Err(Error("arête dont un sommet manque".into()));
                };
                ends[k] = match vids.get(v) {
                    Some(&i) => i,
                    None => {
                        let i = self.ids.next();
                        vids.insert(*v, i);
                        self.ids_of.insert((ctx, 'v', v.data().as_ffi()), (i, "CVertex"));
                        out.verts.push(Vertex { id: i, pos: pt(vx.pos, "sommet")? });
                        i
                    }
                };
            }
            edge_ids.insert(id, el.id);
            self.ids_of.insert((ctx, 'e', ffi), (el.id, "CEdge"));
            out.edges.push(Edge { el, start: ends[0], end: ends[1], curve: None });
        }
        self.curves(e, &edge_ids, &mut out)?;
        // Each edge's curve (by the edge's id in the file), for a centre of arc attached to.
        for (c, class) in out.curves.iter().map(|c| (c, "CCurve")).chain(out.arcs.iter().map(|c| (c, "CArcCurve"))) {
            for &x in &c.edges {
                self.ids_of.insert((ctx, 'c', x), (c.id, class));
            }
        }

        // Faces. An edge of a loop the collection lacks (a file read so) is made, rather than
        // the whole file refused.
        let mut made_edges: HashMap<(VertexId, VertexId), Id> = HashMap::new();
        for (fid, face) in e.faces.iter() {
            let ffi = fid.data().as_ffi();
            let mut el = self.el(face.layer, face.hidden, e.props.get(&format!("f{ffi}")));
            el.material = self.mat(face.front);
            let o = store('f', ffi);
            el.dicts = self.model.attr_store.get(&o).map(|a| dicts_with(self.model, &o, a)).unwrap_or_default();
            let mut loops = Vec::new();
            for l in face.loops() {
                let lid = self.ids.next();
                let mut uses = Vec::new();
                for k in 0..l.len() {
                    let (a, b) = (l[k], l[(k + 1) % l.len()]);
                    let Some(edge) = e.find_edge(a, b) else {
                        let key = (a.min(b), a.max(b));
                        let id = match made_edges.get(&key) {
                            Some(&id) => id,
                            None => {
                                let mut ends = [0; 2];
                                for (n, v) in [key.0, key.1].into_iter().enumerate() {
                                    let Some(vx) = e.verts.get(v) else {
                                        return Err(Error("face dont un sommet manque".into()));
                                    };
                                    ends[n] = match vids.get(&v) {
                                        Some(&i) => i,
                                        None => {
                                            let i = self.ids.next();
                                            vids.insert(v, i);
                                            out.verts.push(Vertex { id: i, pos: pt(vx.pos, "sommet")? });
                                            i
                                        }
                                    };
                                }
                                let el = self.el(face.layer, false, None);
                                let id = el.id;
                                out.edges.push(Edge { el, start: ends[0], end: ends[1], curve: None });
                                made_edges.insert(key, id);
                                id
                            }
                        };
                        uses.push((id, a != key.0));
                        continue;
                    };
                    let rev = e.edges[edge].v[0] != a;
                    uses.push((edge_ids[&edge], rev));
                }
                loops.push((lid, uses));
            }
            if loops.first().is_none_or(|(_, u)| u.len() < 3) {
                continue;
            }
            let n = face.normal;
            let n = if n.length() > 1e-12 { n.normalize() } else { DVec3::Z };
            let p0 = face.outer.first().and_then(|v| e.verts.get(*v)).map_or(DVec3::ZERO, |v| v.pos);
            // (+ 0.0: a plane through the origin has d = 0, not −0, as SketchUp writes it.)
            let plane = [fin(n.x, "face")?, fin(n.y, "face")?, fin(n.z, "face")?, fin(-n.dot(p0) / IN + 0.0, "face")?];
            // Texture placement, of the painted sides positioned in Esquisse.
            if let Some(t) = &face.tex {
                let mut sides = [Tc::none(), Tc::none()];
                let mut any = false;
                for (side, (map, m)) in [(t.front, face.front), (t.back, face.back)].into_iter().enumerate() {
                    let (Some(map), Some(m)) = (map, m) else { continue };
                    let Some(mat) = self.model.materials.get(m) else { continue };
                    let tc = match mat.ext.texture.as_ref() {
                        Some(tx) => placement(&map, t.projection[side], plane, tx.size),
                        // A colour positioned: SketchUp keeps the flag, its map is the plane's
                        // own (one inch per tile, whatever the matrix).
                        None => {
                            let d = t.projection[side].filter(|d| d.length() > 1e-12).map(|d| d.normalize());
                            Some(Tc {
                                flags: 1 | if d.is_some() { 2 } else { 0 },
                                matrix: IDENTITY3,
                                projection: d.map_or([0.0; 3], |d| [d.x, d.y, d.z]),
                            })
                        }
                    };
                    if let Some(tc) = tc {
                        sides[side] = tc;
                        any = true;
                    }
                }
                if any {
                    el.tex = Some(Box::new(sides));
                }
            }
            face_ids.insert(fid, el.id);
            self.ids_of.insert((ctx, 'f', ffi), (el.id, "CFace"));
            out.faces.push(Face { el, back: self.mat(face.back), plane, loops });
        }

        // Instances, groups, images: a definition is made where it is first placed.
        for (iid, inst) in e.instances.iter() {
            let ffi = iid.data().as_ffi();
            let Some(def) = self.model.defs.get(inst.def) else { continue };
            let mut el = self.el(inst.layer, inst.hidden, e.props.get(&format!("i{ffi}")));
            let o = format!("i:{}:{ffi}", ctx.map_or(0, |d| d.data().as_ffi()));
            el.dicts = dicts_with(self.model, &o, &inst.attributes);
            if def.ext.image.is_some() {
                let Some(d) = self.image_definition(inst.def)? else { continue };
                // An image's drawing element: it receives shadows and casts none (SDK default).
                el.flags &= !flags::CASTS;
                out.images.push(Image {
                    inst: Inst {
                        el,
                        name: inst.name.clone(),
                        xform: xform13(&inst.xform)?,
                        def: d,
                        guid: new_guid(),
                        hatch: None,
                    },
                });
                continue;
            }
            if self.open.contains(&inst.def) {
                return Err(Error("définition qui se contient elle-même".into()));
            }
            if depth >= super::MAX_NESTING {
                return Err(Error(format!("plus de {} définitions imbriquées", super::MAX_NESTING)));
            }
            el.material = self.mat(inst.material);
            if inst.locked {
                el.flags |= flags::LOCKED;
            }
            let d = self.definition(inst.def, depth)?;
            let hatch = inst.ext.hatch.as_ref().and_then(|n| self.hatch_names.get(n)).copied();
            let guid = crate::native::style::guid_bytes(&inst.ext.guid).unwrap_or_else(new_guid);
            let i = Inst { el, name: inst.name.clone(), xform: xform13(&inst.xform)?, def: d, guid, hatch };
            inst_ids.insert(iid, i.el.id);
            let class = if def.is_group { "CGroup" } else { "CComponentInstance" };
            self.ids_of.insert((ctx, 'i', iid.data().as_ffi()), (i.el.id, class));
            if def.is_group {
                out.groups.push(i);
            } else {
                out.instances.push(i);
            }
        }

        // What each component is glued to (`InstanceExt::glued_to`), as relationships.
        for (iid, inst) in e.instances.iter() {
            let Some(&what) = inst_ids.get(&iid) else { continue };
            let to = match inst.ext.glued_to {
                Some(esq_core::model::GlueRef::Face(f)) => face_ids.get(&f).copied(),
                Some(esq_core::model::GlueRef::Instance(t)) => inst_ids.get(&t).copied(),
                None => None,
            };
            if let Some(to) = to.filter(|&t| t != what) {
                out.glues.push([to, what]);
            }
        }

        // Guides.
        for (gid, g) in e.guides.iter() {
            let el = self.el(g.layer, g.hidden, None);
            let class = if g.is_point() { "CConstructionPoint" } else { "CConstructionLine" };
            self.ids_of.insert((ctx, 'g', gid.data().as_ffi()), (el.id, class));
            if g.is_point() {
                out.guide_points.push(GuidePoint { el, pos: pt(g.pos, "repère")? });
            } else if g.infinite {
                let d = g.dir.normalize_or_zero();
                out.guide_lines.push(GuideLine {
                    el,
                    point: pt(g.pos, "repère")?,
                    dir: vec3(d, "repère")?,
                    t: [-1e30, 1e30],
                });
            } else {
                let len = g.dir.length() / IN;
                let d = g.dir.normalize_or_zero();
                out.guide_lines.push(GuideLine {
                    el,
                    point: pt(g.pos, "repère")?,
                    dir: vec3(d, "repère")?,
                    t: [0.0, fin(len, "repère")?],
                });
            }
        }

        // Snaps, spaces.
        for s in &e.snaps {
            let el = self.el(s.layer, s.hidden, None);
            let dir = s.dir.normalize_or_zero();
            let up = s.up.normalize_or_zero();
            out.snaps.push(Snap {
                el,
                pos: pt(s.pos, "ancrage")?,
                dir: vec3(dir, "ancrage")?,
                right: vec3(dir.cross(up).normalize_or_zero(), "ancrage")?,
                up: vec3(up, "ancrage")?,
            });
        }
        for s in &e.spaces {
            let el = self.el(s.layer, s.hidden, None);
            let [a, b, c, d] = s.ceiling;
            out.spaces.push(Space {
                el,
                name: s.name.clone(),
                orientation: pt(s.orientation, "espace")?,
                ceiling: [fin(a, "espace")?, fin(b, "espace")?, fin(c, "espace")?, fin(d / IN, "espace")?],
                perimeter: s.perimeter.iter().map(|p| pt(*p, "espace")).collect::<Result<_>>()?,
                holes: s
                    .holes
                    .iter()
                    .map(|h| h.iter().map(|p| pt(*p, "espace")).collect::<Result<Vec<_>>>())
                    .collect::<Result<_>>()?,
            });
        }

        for pl in &e.polylines {
            let el = self.el(pl.layer, pl.hidden, None);
            let points = pl.points.iter().map(|p| pt(*p, "polyligne")).collect::<Result<_>>()?;
            out.polylines.push(Polyline { el, points });
        }

        // Section planes.
        for (sid, s) in e.sections.iter() {
            let el = self.el(s.layer, s.hidden, None);
            let n = s.normal.normalize_or_zero();
            let plane =
                [fin(n.x, "section")?, fin(n.y, "section")?, fin(n.z, "section")?, fin(-n.dot(s.pos) / IN, "section")?];
            if e.active_section == Some(sid) {
                out.active_section = Some(el.id);
            }
            // Its hatch pattern (`SectionPlane#hatch_pattern`, by name).
            let hatch = match e.props.get(&format!("s{}", sid.data().as_ffi())).and_then(|p| p.get("hatch")) {
                Some(AttrValue::Text(n)) => self.hatch_names.get(n).copied(),
                _ => None,
            };
            self.ids_of.insert((ctx, 's', sid.data().as_ffi()), (el.id, "CSectionPlane"));
            out.sections.push(Section { el, plane, name: s.name.clone(), symbol: s.symbol.clone(), hatch });
        }

        // Texts and dimensions.
        for (aid, a) in e.annots.iter() {
            let key = format!("a{}", aid.data().as_ffi());
            let num = |k: &str| match e.prop(&key, k) {
                Some(AttrValue::Number(x)) if x.is_finite() => Some(*x as i32),
                _ => None,
            };
            let (font, mat) = self.annot_font(e, &key);
            let mut el = self.el(a.layer, a.hidden, None);
            el.material = mat;
            match &a.kind {
                AnnotKind::Linear { a: s, b: t, .. } if e.prop(&key, "radial").is_some() => {
                    out.radials.push(self.radial(e, ctx, &key, *s, *t, el, a.text.clone(), font, &edge_ids)?);
                }
                AnnotKind::Linear { a: s, b: t, offset } => {
                    let x = (*t - *s).normalize_or_zero();
                    let side = offset.normalize_or_zero();
                    let n = if side == DVec3::ZERO { esq_core::math::basis_for(x).0 } else { x.cross(side) };
                    out.dims.push(Dim {
                        el,
                        // Empty: the measured length (the SDK writes it so).
                        text: a.text.clone(),
                        arrow_type: num("arrow_type").unwrap_or(3),
                        start: pt(*s, "cote")?,
                        end: pt(*t, "cote")?,
                        normal: vec3(n.normalize_or_zero(), "cote")?,
                        xaxis: vec3(x, "cote")?,
                        position: [0.0, fin(offset.length() / IN, "cote")?],
                        font: font.unwrap_or(0),
                        conns: [(0, *s), (1, *t)].map(|(w, p)| a.attach[w].as_ref().and_then(|x| self.conn(ctx, x, p))),
                    });
                }
                AnnotKind::Text { point, leader } => {
                    out.texts.push(Text {
                        el,
                        text: a.text.clone(),
                        // As read, even not finite (SketchUp wrote and reads them so).
                        point: Some((*point / IN).to_array()),
                        conn: a.attach[0].as_ref().and_then(|x| self.conn(ctx, x, *point)),
                        leader: (*leader / IN).to_array(),
                        screen: [0.5, 0.5],
                        leader_type: if num("leader_type") == Some(1) { 1 } else { 2 },
                        arrow_type: num("arrow_type").unwrap_or(0),
                        font: font.unwrap_or(self.fonts[0]),
                    });
                }
                AnnotKind::ScreenText { x, y } => {
                    out.texts.push(Text {
                        el,
                        text: a.text.clone(),
                        point: None,
                        conn: None,
                        leader: [0.0; 3],
                        screen: [fin(x.clamp(0.0, 1.0), "texte")?, fin(y.clamp(0.0, 1.0), "texte")?],
                        leader_type: 0,
                        arrow_type: num("arrow_type").unwrap_or(0),
                        font: font.unwrap_or(self.fonts[0]),
                    });
                }
            }
        }
        let _ = ctx_ffi;
        Ok(out)
    }

    /// The curves of a collection: each chain of edges sharing a curve id of Esquisse, as an
    /// arc (when its props or its points make one) or a plain curve, as the SDK makes them
    /// (`SUGeometryInputAddArcCurve`, `SUGeometryInputAddCurve`).
    /// A radial dimension (`Entities#add_dimension_radial`, `build` of a .skp): from the centre
    /// `s` to its point `t` on the arc. The arc is the curve `radial` names in this collection
    /// (`c:<collection>:<curve>`), measured at the edge nearest to `t`; without it, the arc read
    /// with the dimension (`skp_arc`), else the circle around `s` through `t`. The angle and the
    /// leader's offset are those read (`skp_angle`, `skp_leader`) while `t` is still the point at
    /// that angle; else the angle of `t` and no offset (SketchUp's defaults).
    #[allow(clippy::too_many_arguments)]
    fn radial(
        &self,
        e: &Entities,
        ctx: Option<DefId>,
        key: &str,
        s: DVec3,
        t: DVec3,
        el: El,
        text: String,
        font: Option<Id>,
        edge_ids: &HashMap<EdgeId, Id>,
    ) -> Result<Radial> {
        let num = |k: &str| match e.prop(key, k) {
            Some(AttrValue::Number(x)) if x.is_finite() => Some(*x),
            _ => None,
        };
        let here = format!("c:{}:", ctx.map_or(0, |d| d.data().as_ffi()));
        let cid = match e.prop(key, "radial") {
            Some(AttrValue::Text(h)) => h.strip_prefix(&here).and_then(|c| c.parse::<u32>().ok()),
            _ => None,
        };
        let edges = cid.map(|c| esq_core::curves::curve_edges(e, c)).unwrap_or_default();
        let mid = |x: &EdgeId| {
            let [a, b] = e.edges[*x].v;
            ((e.verts[a].pos + e.verts[b].pos) * 0.5).distance(t)
        };
        let edge = edges.iter().filter(|x| edge_ids.contains_key(*x)).min_by(|a, b| mid(a).total_cmp(&mid(b)));
        // The arc's frame (mm): centre, x and y axes with their lengths.
        let frame = |c: u32| -> Option<(DVec3, DVec3, DVec3)> {
            let k = format!("c{c}");
            let n = |q: &str| match e.prop(&k, q) {
                Some(AttrValue::Number(x)) if x.is_finite() => Some(*x),
                _ => None,
            };
            let center = DVec3::new(n("cx")?, n("cy")?, n("cz")?);
            let normal = DVec3::new(n("nx")?, n("ny")?, n("nz")?).normalize_or_zero();
            let x = DVec3::new(n("xx")?, n("xy")?, n("xz")?).normalize_or_zero();
            let y = match (n("yx"), n("yy"), n("yz")) {
                (Some(a), Some(b), Some(c)) => DVec3::new(a, b, c).normalize_or_zero(),
                _ => normal.cross(x),
            };
            let r = (t - center).length();
            Some((center, x * n("rx").unwrap_or(r), y * n("ry").unwrap_or(r)))
        };
        let skp_arc: Option<[f64; 15]> = (|| match e.prop(key, "skp_arc") {
            Some(AttrValue::Json(serde_json::Value::Array(v))) if v.len() == 15 => {
                let mut a = [0.0; 15];
                for (i, x) in v.iter().enumerate() {
                    a[i] = x.as_f64().filter(|x| x.is_finite())?;
                }
                Some(a)
            }
            _ => None,
        })();
        let (arc, frame) = match (edge, skp_arc) {
            (Some(_), _) => (None, cid.and_then(frame)),
            (None, Some(v)) => {
                let a = Arc3 {
                    center: [v[0], v[1], v[2]],
                    normal: [v[3], v[4], v[5]],
                    xaxis: [v[6], v[7], v[8]],
                    yaxis: [v[9], v[10], v[11]],
                    radius: v[12],
                    start: v[13],
                    end: v[14],
                };
                let f = (
                    DVec3::from_array(a.center) * IN,
                    DVec3::from_array(a.xaxis) * IN,
                    DVec3::from_array(a.yaxis) * IN,
                );
                (Some(a), Some(f))
            }
            (None, None) => {
                let x = t - s;
                let n = esq_core::math::basis_for(x.normalize_or_zero()).0;
                let y = n.cross(x);
                let r = x.length() / IN;
                let a = Arc3 {
                    center: pt(s, "cote radiale")?,
                    normal: vec3(n, "cote radiale")?,
                    xaxis: vec3(x / IN, "cote radiale")?,
                    yaxis: vec3(y / IN, "cote radiale")?,
                    radius: fin(r, "cote radiale")?,
                    start: 0.0,
                    end: std::f64::consts::TAU,
                };
                (Some(a), Some((s, x, y)))
            }
        };
        let at = |f: &(DVec3, DVec3, DVec3), a: f64| f.0 + f.1 * a.cos() + f.2 * a.sin();
        let tol = 1e-3f64.max((t - s).length() * 1e-6);
        let angle = match (num("skp_angle"), &frame) {
            (Some(a), Some(f)) if at(f, a).distance(t) < tol => a,
            (Some(a), None) => a,
            (_, Some(f)) => {
                let d = t - f.0;
                let (lx, ly) = (f.1.length_squared(), f.2.length_squared());
                if lx > 0.0 && ly > 0.0 { (d.dot(f.2) / ly).atan2(d.dot(f.1) / lx) } else { 0.0 }
            }
            (None, None) => 0.0,
        };
        let leader = match num("skp_leader") {
            Some(l) if num("skp_angle") == Some(angle) => l / IN,
            _ => 0.0,
        };
        Ok(Radial {
            el,
            text,
            arrow_type: num("arrow_type").map_or(3, |x| x as i32),
            font: font.unwrap_or(0),
            edge: edge.and_then(|x| edge_ids.get(x)).copied(),
            arc,
            angle: fin(angle, "cote radiale")?,
            leader: fin(leader, "cote radiale")?,
            diameter: matches!(e.prop(key, "diameter"), Some(AttrValue::Bool(true))),
        })
    }

    fn curves(&mut self, e: &Entities, edge_ids: &HashMap<EdgeId, Id>, out: &mut Ents) -> Result<()> {
        use esq_core::curves::{arc_fit, chain_vertices, chains, curve_edges};
        let mut ids: Vec<u32> = e.edges.values().filter_map(|x| x.curve).collect();
        ids.sort_unstable();
        ids.dedup();
        let index: HashMap<Id, usize> = out.edges.iter().enumerate().map(|(i, x)| (x.el.id, i)).collect();
        for cid in ids {
            let edges = curve_edges(e, cid);
            let ch = chains(e, &edges);
            let key = format!("c{cid}");
            let num = |k: &str| match e.prop(&key, k) {
                Some(AttrValue::Number(x)) if x.is_finite() => Some(*x),
                _ => None,
            };
            let kind = match e.prop(&key, "kind") {
                Some(AttrValue::Text(k)) => k.clone(),
                _ => String::new(),
            };
            // The arc exactly as read, while its points are those it was read with
            // (`build::curve_fingerprint`): on it, around it or cut on a chord, circular or
            // elliptic, SketchUp wrote it so.
            let exact = match e.prop(&key, "fp") {
                Some(AttrValue::Text(fp))
                    if kind != "curve" && *fp == crate::native::build::curve_fingerprint(e, &edges) =>
                {
                    let v = |a: &str, b: &str, c: &str| Some(DVec3::new(num(a)?, num(b)?, num(c)?));
                    (|| {
                        Some(ExactArc {
                            c: v("cx", "cy", "cz")?,
                            n: v("nx", "ny", "nz")?.normalize_or_zero(),
                            x: v("xx", "xy", "xz")?.normalize_or_zero() * num("rx")?,
                            y: v("yx", "yy", "yz")?.normalize_or_zero() * num("ry")?,
                            r: num("r")?,
                            a0: num("start")?,
                            a1: num("end")?,
                        })
                    })()
                }
                _ => None,
            };
            if ch.len() != 1 || ch[0].len() < 2 {
                // A curve whose edges do not make one chain (SketchUp keeps such curves): all its
                // edges, as a plain curve.
                let list: Vec<Id> = ch.iter().flatten().filter_map(|x| edge_ids.get(x).copied()).collect();
                if list.is_empty() {
                    continue;
                }
                let id = self.ids.next();
                for eid in &list {
                    if let Some(&i) = index.get(eid) {
                        out.edges[i].curve = Some(id);
                    }
                }
                // An arc as read (one edge, or edges SketchUp keeps apart) stays one.
                match &exact {
                    Some(x) => out.arcs.push(Curve {
                        id,
                        segments: list.len() as u32,
                        polygon: kind == "polygon",
                        edges: list,
                        arc: Some(exact_arc3(x)?),
                    }),
                    None => out.curves.push(Curve {
                        id,
                        segments: list.len() as u32,
                        polygon: true,
                        edges: list,
                        arc: None,
                    }),
                }
                continue;
            }
            let (vs, closed) = chain_vertices(e, &ch[0]);
            let pts: Vec<DVec3> = vs.iter().map(|v| e.verts[*v].pos).collect();
            // The arc as the file had it (centre, normal, x axis, angles), if the points still
            // lie on it; else as the points make one, as the SDK makes it (from the start
            // point, angle 0 to the sweep).
            let stored = match (num("cx"), num("cy"), num("cz"), num("nx"), num("ny"), num("nz")) {
                (Some(cx), Some(cy), Some(cz), Some(nx), Some(ny), Some(nz)) => {
                    let (c, n) = (DVec3::new(cx, cy, cz), DVec3::new(nx, ny, nz).normalize_or_zero());
                    let r = pts[0].distance(c);
                    let x = DVec3::new(num("xx").unwrap_or(0.0), num("xy").unwrap_or(0.0), num("xz").unwrap_or(0.0))
                        .normalize_or_zero();
                    let tol = 1e-3f64.max(r * 1e-6);
                    let on = n != DVec3::ZERO
                        && x != DVec3::ZERO
                        && x.dot(n).abs() < 1e-6
                        && pts.iter().all(|p| (p.distance(c) - r).abs() < tol && (*p - c).dot(n).abs() < tol);
                    on.then_some((c, n, x, r, num("start"), num("end")))
                }
                _ => None,
            };
            let arc = if kind == "curve" {
                None
            } else {
                stored.or_else(|| arc_fit(e, cid).map(|f| (f.center, f.normal, DVec3::ZERO, f.radius, None, None)))
            };
            let mut order: Vec<EdgeId> = Vec::new();
            let mut vs = vs;
            let arc3 = if let Some(x) = &exact {
                Some(exact_arc(e, &mut vs, closed, x)?)
            } else {
                match arc {
                    None => None,
                    Some((center, normal, xaxis, radius, a0, a1)) => {
                        let nn = normal.normalize_or_zero();
                        // Counter-clockwise around the normal, from the start point.
                        let turn =
                            |vs: &[VertexId]| (e.verts[vs[0]].pos - center).cross(e.verts[vs[1]].pos - center).dot(nn);
                        let (x, start, end) = match (a0, a1) {
                            (Some(a0), Some(a1)) if xaxis != DVec3::ZERO => {
                                let y = nn.cross(xaxis);
                                let at = |a: f64| center + (xaxis * a.cos() + y * a.sin()) * radius;
                                if closed {
                                    if turn(&vs) < 0.0 {
                                        vs.reverse();
                                    }
                                    let w = at(a0);
                                    let k = (0..vs.len())
                                        .min_by(|&i, &j| {
                                            e.verts[vs[i]].pos.distance(w).total_cmp(&e.verts[vs[j]].pos.distance(w))
                                        })
                                        .unwrap_or(0);
                                    vs.rotate_left(k);
                                } else if e.verts[vs[vs.len() - 1]].pos.distance(at(a0))
                                    < e.verts[vs[0]].pos.distance(at(a0))
                                {
                                    vs.reverse();
                                }
                                // A full circle read from a file whose curves do not keep their ends
                                // ends where it starts (0): SketchUp's own conversion of such a file
                                // writes it so (`sdk_resave`, `tests/writer_reel.rs`).
                                (xaxis, a0, a1)
                            }
                            _ => {
                                if closed {
                                    if turn(&vs) < 0.0 {
                                        vs.reverse();
                                    }
                                } else if turn(&vs) < 0.0 {
                                    vs.reverse();
                                }
                                let p0 = e.verts[vs[0]].pos;
                                let x = (p0 - center).normalize_or_zero();
                                let y = nn.cross(x);
                                let last = e.verts[vs[vs.len() - 1]].pos;
                                let sweep = if closed {
                                    std::f64::consts::TAU
                                } else {
                                    let l = (last - center).normalize_or_zero();
                                    let a = l.dot(y).atan2(l.dot(x));
                                    if a <= 1e-12 { a + std::f64::consts::TAU } else { a }
                                };
                                (x, 0.0, if sweep.is_finite() { sweep } else { std::f64::consts::TAU })
                            }
                        };
                        let y = nn.cross(x);
                        let r = radius / IN;
                        let c = center / IN;
                        Some(Arc3 {
                            center: [fin(c.x, "arc")?, fin(c.y, "arc")?, fin(c.z, "arc")?],
                            normal: vec3(nn, "arc")?,
                            xaxis: vec3(x * r, "arc")?,
                            yaxis: vec3(y * r, "arc")?,
                            radius: fin(r, "arc")?,
                            start: fin(start, "arc")?,
                            end: fin(end, "arc")?,
                        })
                    }
                }
            };
            let n = vs.len() - if closed { 0 } else { 1 };
            for k in 0..n {
                if let Some(x) = e.find_edge(vs[k], vs[(k + 1) % vs.len()]) {
                    order.push(x);
                }
            }
            if order.len() != ch[0].len() {
                order = ch[0].clone();
            }
            let id = self.ids.next();
            let mut list = Vec::new();
            for x in &order {
                let Some(&eid) = edge_ids.get(x) else { continue };
                if let Some(&i) = index.get(&eid) {
                    out.edges[i].curve = Some(id);
                }
                list.push(eid);
            }
            let c = Curve {
                id,
                segments: list.len() as u32,
                polygon: arc3.is_none() || kind == "polygon",
                edges: list,
                arc: arc3,
            };
            if c.arc.is_some() {
                out.arcs.push(c);
            } else {
                out.curves.push(c);
            }
        }
        Ok(())
    }

    /// A component or group definition (made once, after the ones it places).
    fn definition(&mut self, d: DefId, depth: usize) -> Result<Id> {
        if let Some(&id) = self.defs.get(&d) {
            return Ok(id);
        }
        let def = &self.model.defs[d];
        let id = self.ids.next();
        self.defs.insert(d, id);
        self.open.push(d);
        let mut ents = self.entities(&def.entities, Some(d), depth + 1)?;
        self.open.pop();
        ents.el.id = id;
        ents.el.dicts = dicts_with(self.model, &format!("d:0:{}", d.data().as_ffi()), &def.attributes);
        ents.bbox = self.bbox(&ents);
        let b = &def.ext.behavior;
        // A group's definition is named as the SDK names it (`SUGroupCreate`): « Group », and
        // so described; Esquisse does not keep these names (it renames group definitions).
        // A group read from a .skp keeps the name and description it had (`build::group_name`).
        let read_name = self
            .model
            .view_ext
            .get("skp_group_names")
            .and_then(|m| m.get(&def.ext.guid))
            .and_then(|v| Some((v.get(0)?.as_str()?.to_string(), v.get(1)?.as_str()?.to_string())))
            .filter(|_| !def.ext.guid.is_empty());
        let (name, description) = if def.is_group {
            read_name.unwrap_or_else(|| {
                (
                    "Group".to_string(),
                    if def.description.is_empty() { "Group".to_string() } else { def.description.clone() },
                )
            })
        } else {
            (def.name.clone(), def.description.clone())
        };
        self.made.push(Def {
            id,
            // As SketchUp: a definition keeps its GUID when the file is saved again.
            guid: crate::native::style::guid_bytes(&def.ext.guid).unwrap_or_else(new_guid),
            name,
            description,
            kind: if def.is_group { 1 } else { 0 },
            behavior: Behavior {
                snap_to: if b.is2d { b.snapto.clamp(0, 3) } else { 0 },
                glues: b.is2d,
                cuts_opening: b.cuts_opening,
                always_face_camera: b.always_face_camera,
                shadows_face_sun: b.shadows_face_sun,
                no_scale_mask: b.no_scale_mask.max(0),
            },
            ents,
            thumb_camera: match def.ext.thumbnail_camera {
                Some([e, t, u]) if [e, t, u].iter().all(|v| v.is_finite()) => {
                    Some([(e / IN).to_array(), (t / IN).to_array(), u.to_array()])
                }
                _ => None,
            },
            thumb: self.model.files.get(&crate::native::build::def_thumbnail_key(&def.name)).map(|b| b.0.clone()),
            path: def.ext.path.clone(),
            // A definition read from a .skp (it has its GUID) keeps its time, even 0.
            time: (def.ext.loaded_at.is_finite()
                && (def.ext.loaded_at >= 1.0 || !def.ext.guid.is_empty())
                && (0.0..u32::MAX as f64).contains(&def.ext.loaded_at))
            .then_some(def.ext.loaded_at as u32),
        });
        Ok(id)
    }

    /// The bounding box of a definition's contents (its vertices, and the boxes of what it
    /// places), in its own coordinates.
    fn bbox(&self, e: &Ents) -> Option<[f64; 6]> {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut add = |p: [f64; 3]| {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        };
        for v in &e.verts {
            add(v.pos);
        }
        for i in e.instances.iter().chain(&e.groups).chain(e.images.iter().map(|x| &x.inst)) {
            let Some(b) = self.made.iter().find(|d| d.id == i.def).and_then(|d| d.ents.bbox) else { continue };
            let t = &i.xform;
            for corner in 0..8 {
                let p = [
                    if corner & 1 == 0 { b[0] } else { b[3] },
                    if corner & 2 == 0 { b[1] } else { b[4] },
                    if corner & 4 == 0 { b[2] } else { b[5] },
                ];
                let w = if t[12].abs() > 1e-300 { t[12] } else { 1.0 };
                add([
                    (p[0] * t[0] + p[1] * t[1] + p[2] * t[2] + t[9]) / w,
                    (p[0] * t[3] + p[1] * t[4] + p[2] * t[5] + t[10]) / w,
                    (p[0] * t[6] + p[1] * t[7] + p[2] * t[8] + t[11]) / w,
                ]);
            }
        }
        (lo[0] <= hi[0]).then_some([lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]])
    }

    /// The definition of an image (SketchUp's `CImage`): one face of a pixel per inch, painted
    /// on both sides with a material of its own (`<auto>`, owned), as `SUImageCreateFromFile`
    /// makes it.
    fn image_definition(&mut self, d: DefId) -> Result<Option<Id>> {
        if let Some(&id) = self.defs.get(&d) {
            return Ok(Some(id));
        }
        let Some(mid) = self.model.image_material(d) else { return Ok(None) };
        let Some(m) = self.model.materials.get(mid) else { return Ok(None) };
        let Some(t) = m.ext.texture.as_ref() else { return Ok(None) };
        let def = &self.model.defs[d];
        let px = match t.px {
            [0, _] | [_, 0] => crate::native::xml::image_size(&t.image.0).unwrap_or([1, 1]),
            p => p,
        };
        let (w, h) = (px[0].max(1) as f64, px[1].max(1) as f64);
        let id = self.ids.next();
        self.defs.insert(d, id);
        // Edges and vertices, then the face (SketchUp's order of ids).
        let mut ents = Ents::default();
        let corners = [[0.0, 0.0, 0.0], [w, 0.0, 0.0], [w, h, 0.0], [0.0, h, 0.0]];
        let mut vids = Vec::new();
        let mut eids = Vec::new();
        for k in 0..4 {
            let eid = self.ids.next();
            if k == 0 {
                let v = self.ids.next();
                vids.push(v);
                ents.verts.push(Vertex { id: v, pos: corners[0] });
            }
            if k < 3 {
                let v = self.ids.next();
                vids.push(v);
                ents.verts.push(Vertex { id: v, pos: corners[k + 1] });
            }
            eids.push(eid);
        }
        for k in 0..4 {
            ents.edges.push(Edge {
                el: El { id: eids[k], flags: flags::CASTS | flags::RECEIVES, ..Default::default() },
                start: vids[k],
                end: vids[(k + 1) % 4],
                curve: None,
            });
        }
        let mat_id = self.ids.next();
        let mut im = self.material(mat_id, m, true)?;
        im.name = "<auto>".into();
        im.folder = folder_name("<auto>", &mut self.folders_taken);
        im.kind = 1;
        im.colorize_type = 0;
        im.color = [0, 0, 0];
        im.trans = 0.5;
        im.use_trans = false;
        if let Some(t) = im.texture.as_mut() {
            t.size = [w, h];
        }
        let face_id = self.ids.next();
        ents.faces.push(Face {
            el: El { id: face_id, material: Some(mat_id), flags: flags::CASTS | flags::RECEIVES, ..Default::default() },
            back: Some(mat_id),
            plane: [0.0, 0.0, 1.0, -0.0],
            loops: vec![(self.ids.next(), eids.iter().map(|&x| (x, false)).collect())],
        });
        self.owned.push(im);
        ents.el.id = id;
        ents.el.flags = flags::CASTS | flags::RECEIVES;
        ents.bbox = Some([0.0, 0.0, 0.0, w, h, 0.0]);
        let stem = def.ext.image.as_ref().map(|i| i.path.clone()).unwrap_or_else(|| def.name.clone());
        let stem = std::path::Path::new(&esq_core::meta::base_name(&stem))
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("image")
            .to_string();
        self.made.push(Def {
            id,
            guid: new_guid(),
            name: format!("{stem}_image"),
            description: String::new(),
            kind: 2,
            behavior: Behavior::default(),
            ents,
            thumb_camera: None,
            thumb: None,
            path: String::new(),
            time: None,
        });
        Ok(Some(id))
    }
}

/// The texture placement of one side (`CFaceTextureCoords`) from Esquisse's map: the matrix
/// SketchUp keeps in the frame of the side (`build::uv_map` read backwards, with the same
/// [`esq_core::texpos`] model), and the projection of a projected texture.
fn placement(map: &esq_core::meta::UvMap, projection: Option<DVec3>, plane: [f64; 4], size: [f64; 2]) -> Option<Tc> {
    let n = DVec3::new(plane[0], plane[1], plane[2]);
    let projection = projection.filter(|d| d.length() > 1e-12).map(|d| d.normalize());
    let frame = esq_core::texpos::Frame::of_side(-n * plane[3] * IN, n, projection);
    let m = esq_core::texpos::sketchup_matrix(map, &frame, [size[0].max(1e-9), size[1].max(1e-9)])?;
    Some(Tc {
        flags: 1 | if projection.is_some() { 2 } else { 0 },
        matrix: m,
        projection: projection.map_or([0.0; 3], |d| [d.x, d.y, d.z]),
    })
}
