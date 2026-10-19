//! SketchUp's objects as a .skp stores them (`docs/skp-format.md`, §3), before they become an
//! Esquisse model. Both file formats (the tagged records of 2021+ and the MFC archive of the
//! earlier versions) are read into this, and `build` turns it into an `esq_core` model the way
//! the SDK reader does. Lengths are in inches, as in the file.

use std::collections::BTreeMap;

use esq_core::model::Attributes;
use serde_json::Value;

/// Persistent id of an object of the file (vertices, edges, faces, materials…).
pub type Id = u64;

/// Entity flags (`CEntity` / `CDrawingElement`, record 0x7d3 in 2021+).
/// Separates the names of a dictionary nested in another one in the keys of an
/// [`Attributes`] as read (`parent\0child`): the builder files it under its parent
/// (`build::split_nested`). SketchUp's names are C strings: none holds it.
pub const NEST: char = '\0';

pub mod flags {
    pub const HIDDEN: u32 = 1;
    pub const CASTS_SHADOWS: u32 = 2;
    pub const RECEIVES_SHADOWS: u32 = 4;
    pub const SOFT: u32 = 8;
    pub const SMOOTH: u32 = 16;
    pub const LOCKED: u32 = 32;
}

/// What every drawing element has: its id, material, tag, flags and dictionaries.
#[derive(Clone, Debug, Default)]
pub struct El {
    pub id: Id,
    pub material: Option<Id>,
    pub layer: Option<Id>,
    pub flags: u32,
    pub attrs: Attributes,
    /// A face's texture placement, front and back (kept with the dictionaries in the file).
    pub tex: [Option<Box<TexCoords>>; 2],
}

impl El {
    pub fn hidden(&self) -> bool {
        self.flags & flags::HIDDEN != 0
    }
}

#[derive(Clone, Debug, Default)]
pub struct Vertex {
    pub id: Id,
    pub pos: [f64; 3],
}

#[derive(Clone, Debug, Default)]
pub struct Edge {
    pub el: El,
    pub start: Id,
    pub end: Id,
    pub curve: Option<Id>,
}

/// One side of a face's texture placement (`CFaceTextureCoords`).
#[derive(Clone, Debug, Default)]
pub struct TexCoords {
    /// The texture was positioned (Position Texture, `position_material`).
    pub positioned: bool,
    /// The texture is projected along a direction.
    pub projected: bool,
    /// 3×3 map, row-major: texture coordinates (inches, row vector [s t 1]) to the plane's
    /// 2D frame (see `docs/skp-format.md`).
    pub matrix: [f64; 9],
    pub projection: Option<[f64; 3]>,
}

#[derive(Clone, Debug, Default)]
pub struct Face {
    pub el: El,
    pub back: Option<Id>,
    /// a·x + b·y + c·z + d = 0, (a, b, c) the normal.
    pub plane: [f64; 4],
    /// Outer loop first: (edge, reversed) in loop order.
    pub loops: Vec<Vec<(Id, bool)>>,
}

#[derive(Clone, Debug, Default)]
pub struct Arc {
    pub center: [f64; 3],
    pub normal: [f64; 3],
    pub xaxis: [f64; 3],
    pub yaxis: [f64; 3],
    pub radius: f64,
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Curve {
    pub id: Id,
    pub segments: u32,
    /// The record after the count (0 for arcs, 1 for plain curves, measured).
    pub kind: u32,
    pub first: Id,
    pub last: Id,
    pub arc: Option<Arc>,
}

/// A group, a component instance or an image placement.
#[derive(Clone, Debug, Default)]
pub struct Inst {
    pub el: El,
    pub name: String,
    /// Column-major 4×4 (as `SUTransformation`), translation in inches.
    pub xform: [f64; 16],
    pub def: Id,
    pub locked: bool,
    /// The hatch pattern of its section cuts (2026: 0x1969, id of one of `Doc::hatch_patterns`).
    pub hatch: Option<Id>,
    /// Its GUID (2021+ 0x1968; MFC from schema 5).
    pub guid: String,
}

#[derive(Clone, Debug, Default)]
pub struct GuideLine {
    pub el: El,
    pub start: [f64; 3],
    /// Direction (unit) when infinite, else end − start.
    pub dir: [f64; 3],
    pub infinite: bool,
}

#[derive(Clone, Debug, Default)]
pub struct GuidePoint {
    pub el: El,
    pub pos: [f64; 3],
}

#[derive(Clone, Debug, Default)]
pub struct Section {
    pub el: El,
    pub plane: [f64; 4],
    pub name: String,
    pub symbol: String,
    /// The hatch pattern of its cuts (2026: 0x4460).
    pub hatch: Option<Id>,
}

/// What a point of an annotation is attached to, as the file keeps it (2021+ 0x5208, MFC,
/// `docs/skp-format.md` §2.3.7): SketchUp's type of connection, the point of
/// the file, the entity (by id) and the instances that lead to it, outermost first.
/// Intersections (type 9, two entities) are not kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Connection {
    pub kind: i32,
    pub point: [f64; 3],
    pub leaf: Option<Id>,
    pub path: Vec<Id>,
}

#[derive(Clone, Debug, Default)]
pub struct Text {
    pub el: El,
    pub text: String,
    /// The point the leader points to, when it has one.
    pub point: Option<[f64; 3]>,
    /// What that point is attached to.
    pub connection: Option<Connection>,
    pub leader: [f64; 3],
    /// 0 none, 1 view based, 2 pushpin.
    pub leader_type: i32,
    pub arrow_type: i32,
    /// Screen position (fraction of the view) of a screen text.
    pub screen: [f64; 2],
    /// Its font (one of `Doc::fonts`).
    pub font: Option<Id>,
}

#[derive(Clone, Debug, Default)]
pub struct DimLinear {
    pub el: El,
    pub start: [f64; 3],
    pub end: [f64; 3],
    /// What the start and the end are attached to.
    pub connections: [Option<Connection>; 2],
    pub xaxis: [f64; 3],
    pub normal: [f64; 3],
    pub position: [f64; 2],
    pub text: String,
    pub arrow_type: i32,
    /// Its font (one of `Doc::fonts`).
    pub font: Option<Id>,
}

/// A radial dimension (2021+ 0x5dc0, `CDimensionRadial`, and):
/// the dimension's part, the edge of the arc it measures (or, without one,
/// the arc itself), the angle of its point on the arc, the offset of its leader, whether it
/// gives the diameter. SketchUp makes its leader from them (in SketchUpAPI.dll).
#[derive(Clone, Debug, Default)]
pub struct DimRadial {
    pub el: El,
    pub text: String,
    pub arrow_type: i32,
    pub font: Option<Id>,
    pub edge: Option<Id>,
    pub arc: Option<Arc>,
    pub angle: f64,
    pub leader: f64,
    pub diameter: bool,
}

/// A 3D polyline (2021+ 0x4844, `CPolyline3d`): its drawing element and its points (inches).
#[derive(Clone, Debug, Default)]
pub struct Polyline {
    pub el: El,
    pub points: Vec<[f64; 3]>,
}

#[derive(Clone, Debug, Default)]
pub struct Image {
    pub inst: Inst,
    pub file_name: String,
    pub px: [u32; 2],
}

/// A component snap (`Sketchup::Snap`).
#[derive(Clone, Debug, Default)]
pub struct Snap {
    pub el: El,
    pub pos: [f64; 3],
    pub dir: [f64; 3],
    pub up: [f64; 3],
}

/// A space (`Sketchup::Space`, SketchUp 2026).
#[derive(Clone, Debug, Default)]
pub struct SpaceRec {
    pub el: El,
    pub name: String,
    pub orientation: [f64; 3],
    pub ceiling: [f64; 4],
    pub perimeter: Vec<[f64; 3]>,
    pub holes: Vec<Vec<[f64; 3]>>,
}

#[derive(Clone, Debug, Default)]
pub struct Ents {
    pub snaps: Vec<Snap>,
    pub spaces: Vec<SpaceRec>,
    pub verts: Vec<Vertex>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
    pub groups: Vec<Inst>,
    pub instances: Vec<Inst>,
    pub images: Vec<Image>,
    pub guide_lines: Vec<GuideLine>,
    pub guide_points: Vec<GuidePoint>,
    pub sections: Vec<Section>,
    pub active_section: Option<Id>,
    pub texts: Vec<Text>,
    pub dims: Vec<DimLinear>,
    pub radials: Vec<DimRadial>,
    pub polylines: Vec<Polyline>,
    pub curves: Vec<Curve>,
    /// The relationships of the collection (2021+ 0x139b → 0x639f `CRelationshipMap` → 0x63a0
    /// → 0x639c `CRelationship`; MFC: `CComponent`'s map): what a component is glued to (0x639d)
    /// and the component (0x639e), by id.
    pub glues: Vec<[Id; 2]>,
}

#[derive(Clone, Debug, Default)]
pub struct Behavior {
    pub snap: i32,
    pub cuts_opening: bool,
    pub always_face_camera: bool,
    pub shadows_face_sun: bool,
    pub no_scale_mask: i32,
}

#[derive(Clone, Debug, Default)]
pub struct Def {
    pub id: Id,
    pub name: String,
    pub description: String,
    pub guid: String,
    /// The file it was loaded from (0x1580; MFC from schema 4, before a list of names) and when
    /// (0x1581, seconds since 1970).
    pub path: String,
    pub time: u32,
    pub is_group: bool,
    pub is_image: bool,
    pub behavior: Behavior,
    pub ents: Ents,
    pub attrs: Attributes,
    /// Its thumbnail (`CThumbnail`): the camera it was taken from, the image.
    pub thumbnail: Option<Thumbnail>,
}

/// A definition's thumbnail (2021+ 0x1585 → 0x251c; `CThumbnail`): its camera and its image
/// (2021+: `thumbnails/<name>.png` of the archive; MFC: a `CDib`).
#[derive(Clone, Debug, Default)]
pub struct Thumbnail {
    pub camera: Option<Camera>,
    pub image: Option<std::sync::Arc<Vec<u8>>>,
}

#[derive(Clone, Debug, Default)]
pub struct Texture {
    pub file_name: String,
    pub image: std::sync::Arc<Vec<u8>>,
    pub px: [u32; 2],
    /// Size of one tile, in inches.
    pub size: [f64; 2],
    pub avg: [u8; 3],
}

#[derive(Clone, Debug, Default)]
pub struct Pbr {
    pub metalness_enabled: bool,
    pub metallic_factor: f64,
    pub metallic_texture: Option<Texture>,
    pub roughness_enabled: bool,
    pub roughness_factor: f64,
    pub roughness_texture: Option<Texture>,
    pub roughness_inverted: bool,
    pub normal_enabled: bool,
    pub normal_scale: f64,
    pub normal_style: i32,
    pub normal_texture: Option<Texture>,
    pub ao_enabled: bool,
    pub ao_strength: f64,
    pub ao_texture: Option<Texture>,
}

#[derive(Clone, Debug, Default)]
pub struct Material {
    pub id: Id,
    pub name: String,
    pub color: [u8; 3],
    /// The alpha byte of the stored colour when it is not 255.
    pub color_alpha: Option<u8>,
    /// 0 colored, 1 textured, 2 colorized.
    pub kind: i32,
    pub opacity: f64,
    pub use_opacity: bool,
    pub colorize_type: i32,
    pub colorize_deltas: [f64; 3],
    pub texture: Option<Texture>,
    pub workflow: i32,
    pub pbr: Pbr,
    /// The material belongs to a tag (its colour) or an image: not listed with the others.
    pub owned: bool,
    pub attrs: Attributes,
    /// The thumbnail SketchUp keeps beside its file (2021+: `materials/<folder>/thumbnail.*`).
    pub thumbnail: Option<std::sync::Arc<Vec<u8>>>,
    /// The hatch pattern of its section cuts (2026: 0x32cf).
    pub hatch: Option<Id>,
}

#[derive(Clone, Debug, Default)]
pub struct Layer {
    pub id: Id,
    pub name: String,
    pub visible: bool,
    pub color: Option<[u8; 3]>,
    pub line_style: Option<Id>,
    pub page_behavior: u32,
    pub attrs: Attributes,
}

#[derive(Clone, Debug, Default)]
pub struct Folder {
    pub id: Id,
    pub name: String,
    pub visible: bool,
    pub visible_on_new_pages: bool,
    pub layers: Vec<Id>,
    pub folders: Vec<Folder>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Camera {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    pub perspective: bool,
    pub fov: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub id: Id,
    pub name: String,
    pub description: String,
    pub camera: Option<Camera>,
    pub flags: u32,
    pub hidden_layers: Option<Vec<Id>>,
    pub rendering: Option<BTreeMap<String, Value>>,
    pub animate: bool,
    pub attrs: Attributes,
    /// The style the scene keeps (id of a style of `Doc::styles`).
    pub style: Option<Id>,
    /// Its matched photo (id of one of `Doc::photos`).
    pub photo: Option<Id>,
    /// Its own transition and delay times (−1: the model's; 0x7154, 0x7155).
    pub transition: f64,
    pub delay: f64,
    /// 0x7153 (a name SketchUp keeps for a matched photo's scene) and 0x7157.
    pub v7153: String,
    pub v7157: bool,
    /// Its thumbnail (2021+: 0x7158 → `scene_thumbnails/…`; MFC: an image after 0x7157).
    pub thumbnail: Option<std::sync::Arc<Vec<u8>>>,
    /// The environment it keeps (0x715a, id of one of `Doc::environments`) and its settings
    /// (0x715b).
    pub env_id: Option<Id>,
    pub env: Option<SceneEnv>,
    /// The hatch pattern settings of its rendering options (0x7388).
    pub ro_hatch: Option<HatchSettings>,
}

/// A hatch pattern's settings (`HatchPatternSettings`): inline in rendering options (0x7388 →
/// 0x7d0a), as the attributes of a pattern's
/// `hatchpattern.xml` and of a style's item 7019.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct HatchSettings {
    /// RGBA (0x7d0c; `colorRed`, `colorGreen`, `colorBlue`).
    pub color: [u8; 4],
    /// An image pattern: the image's file name (0x7d12; `textureName file_name`) and the
    /// image (0x7d0d → 0x2328; a file of the pattern's folder).
    pub texture_name: String,
    #[serde(with = "b64")]
    pub image: Option<std::sync::Arc<Vec<u8>>>,
    /// `xScale`, `yScale` and `rotationAngle` (0x7d0e, 0x7d0f, 0x7d14: f32).
    pub scale: [f32; 2],
    pub rotation: f32,
    /// A .pat pattern: SketchUp's floats (0x7d11; `patData` is their base64: per line angle,
    /// origin, offset, the number of dash/gap pairs, the pairs) and the file's name (0x7d13;
    /// `patName`).
    pub pat: Option<Vec<f32>>,
    pub pat_name: String,
}

/// Bytes as base64 in JSON (the image of a hatch pattern kept in a scene's properties).
mod b64 {
    use std::sync::Arc;
    pub fn serialize<S: serde::Serializer>(v: &Option<Arc<Vec<u8>>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(b) => s.serialize_some(&esq_core::photo::base64_encode(b)),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Arc<Vec<u8>>>, D::Error> {
        let t: Option<String> = serde::Deserialize::deserialize(d)?;
        Ok(t.and_then(|t| esq_core::photo::base64_decode(&t)).map(Arc::new))
    }
}

/// A hatch pattern of the model (2026: 0x211 → 0x7d00 `HatchPatternManager` → 0x7d01 → 0x7d0a
/// `HatchPattern`): 0x5dc its id, 0x7d15 its folder of
/// `hatchpatterns/` (`hatchpattern.xml`, its image, `thumbnail.jpg`), 0x7d0b its name, 0x7d10
/// → 0x2134 its thumbnail.
#[derive(Clone, Debug, Default)]
pub struct HatchPattern {
    pub id: Id,
    pub folder: String,
    pub name: String,
    pub settings: HatchSettings,
    pub thumbnail: Option<std::sync::Arc<Vec<u8>>>,
    /// Its `hatchpattern.xml` as read.
    pub xml: Option<std::sync::Arc<Vec<u8>>>,
}

/// What a scene keeps of an environment (0x715b): reflections (0x7b0f), skydome (0x7b10),
/// rotation (0x7b11), skydome and reflection exposures (0x7b12, 0x7b13), sun linked (0x7b14)
/// and the sun's position (0x7b15, 0x7b16).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SceneEnv {
    pub reflections: bool,
    pub skydome: bool,
    pub rotation: f32,
    pub skydome_exposure: f32,
    pub reflection_exposure: f32,
    pub linked_sun: bool,
    pub sun: [f32; 2],
}

/// An environment (2021+ 0x7b0c `CEnvironment`): its folder of the archive
/// (`environments/<folder>/`: `environment.xml`, the panorama, `thumbnail.jpg`) and what its
/// `environment.xml` holds.
#[derive(Clone, Debug, Default)]
pub struct Environment {
    pub id: Id,
    pub folder: String,
    pub name: String,
    pub description: String,
    /// `ibl_enabled`, `skybox_enabled`, `shadow_light_enabled`.
    pub reflections: bool,
    pub skydome: bool,
    pub linked_sun: bool,
    /// `rotation_angle` (degrees), `skybox_gain`, `reflection_gain`.
    pub rotation: f64,
    pub skydome_exposure: f64,
    pub reflection_exposure: f64,
    /// `shadow_light_positionX`, `…Y`: fractions of a turn and of a quarter turn.
    pub sun: [f64; 2],
    /// `ibl_filename`: the panorama's file name, and the panorama.
    pub image_name: String,
    pub image: Option<std::sync::Arc<Vec<u8>>>,
    pub thumbnail: Option<std::sync::Arc<Vec<u8>>>,
}

/// A matched photo (2021+ 0x2904 `CMatchPhoto`, `CBackgroundImage` before):
/// the image (where it was loaded from, its pixels), then what SketchUp calibrated on it.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MatchPhoto {
    pub id: Id,
    /// The file it was loaded from (0x2af9).
    pub path: String,
    /// 0x2afa (MFC: 1 or 2 before schema 5).
    pub kind: u32,
    #[serde(skip)]
    pub image: Option<std::sync::Arc<Vec<u8>>>,
    /// Width and height in pixels (0x2afc, 0x2afd).
    pub size: [u32; 2],
    /// 0x2afe.
    pub v2afe: u32,
    /// When it was loaded (0x2aff).
    pub time: u32,
    /// 0x2906 (+0x2c).
    pub flag: bool,
    /// 0x2907 (+0x30).
    pub value: f64,
    /// The ends of the lines drawn on it (0x2908, +0x48): two lines per axis, red, green, blue,
    /// in units of the image's height from its top left corner.
    pub points: Vec<[f64; 3]>,
    /// 0x2909 (+0xb8): the origin's offset.
    pub offset: [f64; 3],
    /// 0x290a (+0xd0).
    pub scale: f64,
    /// 0x290b (+0x1f0).
    pub v290b: u32,
}

/// What a file holds.
#[derive(Clone, Debug, Default)]
pub struct Doc {
    /// The styles of the model and its watermarks (`super::style`).
    pub styles: super::style::Styles,
    /// The fonts of the model (`CFontManager`), the text style, the dimension style.
    pub fonts: Vec<Font>,
    pub text_style: Option<TextStyle>,
    pub dim_style: Option<DimStyle>,
    /// The classification schemas loaded in the model, in the file's order.
    pub schemas: Vec<SchemaFile>,
    /// The matched photos, and the active one.
    pub photos: Vec<MatchPhoto>,
    pub active_photo: Option<Id>,
    /// The environments, and the current one (0x791a).
    pub environments: Vec<Environment>,
    pub current_env: Option<Id>,
    /// The hatch patterns, and those of the model's rendering options (0x7388).
    pub hatch_patterns: Vec<HatchPattern>,
    pub ro_hatch: Option<HatchSettings>,
    /// Three values of `CSketchUpModel` whose use is not known :
    /// a bool at 0x2ff (0x20c; MFC from schema 0x16, true at 0x15), a u32 at
    /// 0x328 (0x20d; MFC from schema 0x1c, the largest met) and one at 0x24c (0x20f; MFC from
    /// schema 0x14, at 0x13 a bool: 0 or 2). As read, to write them back.
    pub model_flags: Option<ModelFlags>,
    pub version: (u32, u32, u32),
    pub guid: String,
    pub name: String,
    pub description: String,
    pub materials: Vec<Material>,
    /// The default tag first.
    pub layers: Vec<Layer>,
    pub folders: Vec<Folder>,
    pub active_layer: Option<Id>,
    pub line_styles: BTreeMap<Id, String>,
    pub defs: Vec<Def>,
    pub root: Ents,
    pub scenes: Vec<Scene>,
    pub active_scene: Option<usize>,
    pub camera: Option<Camera>,
    /// Drawing axes, column-major 4×4 (inches).
    pub axes: Option<[f64; 16]>,
    /// Options providers (Model Info): provider → key → value.
    pub options: BTreeMap<String, BTreeMap<String, Value>>,
    pub rendering: BTreeMap<String, Value>,
    /// Shadow settings, by the SDK's key names.
    pub shadow: BTreeMap<String, Value>,
    /// The model's attribute dictionaries.
    pub attrs: Attributes,
    /// Build the model as the SDK reader would (`super::Options::sdk_quirks`).
    pub sdk_quirks: bool,
}

/// A classification schema the model carries (2021+ 0x6784, `CSchemaZipFile`): where it was
/// loaded from, its name, its `.skc` file (a zip).
#[derive(Clone, Debug, Default)]
pub struct SchemaFile {
    pub path: String,
    pub name: String,
    pub data: std::sync::Arc<Vec<u8>>,
}

/// A font of the model (`CSkFont`): face, bold, italic, size in points, or a height in the
/// model (inches) when it is « world size ».
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Font {
    /// Persistent id (2021+), object of the archive (MFC); in Esquisse, its rank in the list.
    pub id: Id,
    pub face: String,
    pub bold: bool,
    pub italic: bool,
    pub size: u32,
    pub world: bool,
    pub height: f64,
}

/// The text style of the model (2021+ 0x57e4, `CTextStyle`): two fonts, then its settings in
/// the order both formats write them.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TextStyle {
    pub font: Option<Id>,
    pub font2: Option<Id>,
    pub v1: u32,
    pub v2: u32,
    pub b1: bool,
    pub v3: u32,
    pub b2: bool,
    pub color: [u8; 4],
    pub color2: [u8; 4],
}

/// The dimension style of the model (2021+ 0x5fb4, `CDimensionStyle`), in the order both
/// formats write it (the names: `SUDimensionStyleGet…`).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DimStyle {
    /// 4 bytes an MFC archive writes first.
    pub lead: u32,
    pub font: Option<Id>,
    pub text3d: bool,
    pub radial_prefix: bool,
    pub ext_offset: u32,
    pub ext_overshoot: u32,
    pub line_weight: u32,
    pub arrow: i32,
    pub arrow_size: u32,
    pub highlight: bool,
    pub highlight_color: [u8; 4],
    pub b5fbf: bool,
    pub hide_out_of_plane: bool,
    pub hide_out_of_plane_value: f64,
    pub hide_small: bool,
    pub hide_small_value: f64,
    pub color: [u8; 4],
    pub text_color: [u8; 4],
    pub v5fc6: u32,
}

/// See `Doc::model_flags`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelFlags {
    pub v20c: bool,
    pub v20d: u32,
    pub v20f: u32,
}

impl Default for ModelFlags {
    /// What a new model has (SketchUp 2026, and both writers of Esquisse before).
    fn default() -> Self {
        ModelFlags { v20c: true, v20d: 1, v20f: 0 }
    }
}
