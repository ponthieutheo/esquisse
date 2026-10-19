//! The document model: entities collections, component definitions, materials and tags.
//!
//! Like SketchUp, geometry is a graph of vertices and edges, and faces are planar polygons
//! (outer loop + holes) bounded by those edges. Groups and components are instances of a
//! definition that owns its own entities collection.

use std::collections::BTreeMap;

use glam::{DMat4, DVec3};
use serde::{Deserialize, Serialize};
use slotmap::{SlotMap, new_key_type};

use crate::math::{Plane, newell_normal};

new_key_type! {
    pub struct VertexId;
    pub struct EdgeId;
    pub struct FaceId;
    pub struct InstanceId;
    pub struct DefId;
    pub struct MaterialId;
    pub struct GuideId;
    pub struct AnnotId;
    pub struct SectionId;
}

impl DefId {
    /// The definition key as a number (how scripts name a definition) and back.
    pub fn to_ffi(self) -> u64 {
        slotmap::Key::data(&self).as_ffi()
    }

    pub fn from_ffi(n: u64) -> Self {
        slotmap::KeyData::from_ffi(n).into()
    }
}

/// Index into `Model::layers`; 0 is "Untagged".
pub type LayerIdx = usize;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Vertex {
    pub pos: DVec3,
    #[serde(skip)]
    pub edges: Vec<EdgeId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub v: [VertexId; 2],
    #[serde(default)]
    pub soft: bool,
    #[serde(default)]
    pub smooth: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub layer: LayerIdx,
    /// Edges created together as one circle/arc/curve share an id and select together.
    #[serde(default)]
    pub curve: Option<u32>,
    #[serde(skip)]
    pub faces: Vec<FaceId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Face {
    /// Counter-clockwise around `normal`.
    pub outer: Vec<VertexId>,
    /// Clockwise around `normal`.
    #[serde(default)]
    pub holes: Vec<Vec<VertexId>>,
    pub normal: DVec3,
    #[serde(default)]
    pub front: Option<MaterialId>,
    #[serde(default)]
    pub back: Option<MaterialId>,
    #[serde(default)]
    pub layer: LayerIdx,
    #[serde(default)]
    pub hidden: bool,
    /// Texture positioning (Paint Bucket position / `Face#position_material`); `None` = the
    /// default projection along the face's plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tex: Option<Box<crate::meta::FaceTex>>,
}

impl Face {
    pub fn loops(&self) -> impl Iterator<Item = &Vec<VertexId>> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }

    pub fn all_vertices(&self) -> impl Iterator<Item = VertexId> + '_ {
        self.loops().flat_map(|l| l.iter().copied())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Instance {
    pub def: DefId,
    pub xform: DMat4,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub material: Option<MaterialId>,
    #[serde(default)]
    pub layer: LayerIdx,
    #[serde(default)]
    pub hidden: bool,
    /// A locked group/component cannot be moved, erased, exploded or opened.
    #[serde(default)]
    pub locked: bool,
    /// Attribute dictionaries of this instance (Tools › Component Attributes).
    #[serde(default, skip_serializing_if = "Attributes::is_empty")]
    pub attributes: Attributes,
    /// Properties only the Ruby API reads (glue, hatch pattern).
    #[serde(default, skip_serializing_if = "InstanceExt::is_empty")]
    pub ext: InstanceExt,
}

/// What an instance is glued to (`Group#glued_to`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum GlueRef {
    Face(FaceId),
    Instance(InstanceId),
}

/// Instance properties beyond the geometry: the Ruby API reads and writes them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InstanceExt {
    /// The face (or instance) of the same collection this instance is glued to.
    #[serde(default)]
    pub glued_to: Option<GlueRef>,
    /// Name of the hatch pattern used for the section cut.
    #[serde(default)]
    pub hatch: Option<String>,
    /// The GUID a .skp gave the instance (SketchUp keeps it when it saves the file again).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guid: String,
}

impl InstanceExt {
    pub fn is_empty(&self) -> bool {
        *self == InstanceExt::default()
    }
}

/// The behavior of a component definition (`Sketchup::Behavior`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Behavior {
    pub always_face_camera: bool,
    pub cuts_opening: bool,
    pub is2d: bool,
    pub shadows_face_sun: bool,
    pub no_scale_mask: i32,
    /// 0 arbitrary, 1 horizontal, 2 vertical, 3 sloped.
    pub snapto: i32,
}

/// Definition properties beyond the geometry: the Ruby API reads and writes them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DefExt {
    #[serde(default)]
    pub behavior: Behavior,
    /// Camera of the thumbnail: eye, target, up.
    #[serde(default)]
    pub thumbnail_camera: Option<[DVec3; 3]>,
    /// When the definition was loaded (seconds since 1970; 0 = created in this model).
    #[serde(default)]
    pub loaded_at: f64,
    /// File the definition was loaded from.
    #[serde(default)]
    pub path: String,
    /// Set when the definition is that of an Image (`ComponentDefinition#image?`): see
    /// `crate::image`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<crate::image::ImageDef>,
    /// The GUID a .skp gave the definition (`ComponentDefinition#guid`; SketchUp keeps it when
    /// it saves the file again).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guid: String,
}

impl DefExt {
    pub fn is_empty(&self) -> bool {
        *self == DefExt::default()
    }
}

/// A value of an attribute dictionary. Plain strings, floats and booleans have their own
/// variants; every other Ruby value (integers, colours, times, points, arrays, `nil`…) is
/// kept as tagged JSON by the Ruby bridge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AttrValue {
    Text(String),
    Number(f64),
    Bool(bool),
    Json(serde_json::Value),
}

/// Attribute dictionaries: dictionary name → key → value (SketchUp's `AttributeDictionary`).
/// Dictionaries keep their creation order, the keys of a dictionary are sorted.
pub type Attributes = indexmap::IndexMap<String, BTreeMap<String, AttrValue>>;

/// The kinds of value the attribute editor offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrKind {
    Text,
    Number,
    Bool,
}

impl AttrKind {
    pub const ALL: [AttrKind; 3] = [AttrKind::Text, AttrKind::Number, AttrKind::Bool];

    pub fn label(self) -> &'static str {
        match self {
            AttrKind::Text => "Texte",
            AttrKind::Number => "Nombre",
            AttrKind::Bool => "Booléen",
        }
    }
}

impl AttrValue {
    pub fn kind(&self) -> AttrKind {
        match self {
            AttrValue::Text(_) | AttrValue::Json(_) => AttrKind::Text,
            AttrValue::Number(_) => AttrKind::Number,
            AttrValue::Bool(_) => AttrKind::Bool,
        }
    }

    /// The text shown in an editor field.
    pub fn display(&self) -> String {
        match self {
            AttrValue::Text(s) => s.clone(),
            AttrValue::Number(n) => format!("{n}"),
            AttrValue::Bool(b) => if *b { "vrai" } else { "faux" }.to_string(),
            AttrValue::Json(v) => v.get("v").unwrap_or(v).to_string(),
        }
    }

    /// Parses what was typed as a value of `kind` ("1,5" is accepted for numbers).
    pub fn parse(kind: AttrKind, s: &str) -> Option<AttrValue> {
        match kind {
            AttrKind::Text => Some(AttrValue::Text(s.to_string())),
            AttrKind::Number => s.trim().replace(',', ".").parse().ok().map(AttrValue::Number),
            AttrKind::Bool => match s.trim().to_lowercase().as_str() {
                "vrai" | "true" | "oui" | "1" => Some(AttrValue::Bool(true)),
                "faux" | "false" | "non" | "0" => Some(AttrValue::Bool(false)),
                _ => None,
            },
        }
    }
}

/// Sets a value, creating the dictionary when needed.
pub fn set_attr(attrs: &mut Attributes, dict: &str, key: &str, value: AttrValue) {
    attrs.entry(dict.to_string()).or_default().insert(key.to_string(), value);
}

/// Removes a key; an emptied dictionary goes away too.
pub fn remove_attr(attrs: &mut Attributes, dict: &str, key: &str) {
    if let Some(d) = attrs.get_mut(dict) {
        d.remove(key);
        if d.is_empty() {
            attrs.shift_remove(dict);
        }
    }
}

/// A construction guide made with the Tape Measure or the Protractor: an infinite line, a
/// finite line, or a point. Guides help drawing but are not part of the geometry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Guide {
    pub pos: DVec3,
    /// Direction of a line (unit length when infinite, start→end vector when finite);
    /// zero for a guide point.
    #[serde(default)]
    pub dir: DVec3,
    #[serde(default)]
    pub infinite: bool,
    #[serde(default)]
    pub layer: LayerIdx,
    #[serde(default)]
    pub hidden: bool,
}

impl Guide {
    pub fn point(pos: DVec3) -> Self {
        Guide { pos, dir: DVec3::ZERO, infinite: false, layer: 0, hidden: false }
    }

    pub fn line(pos: DVec3, dir: DVec3) -> Self {
        Guide { pos, dir: dir.normalize(), infinite: true, layer: 0, hidden: false }
    }

    pub fn segment(a: DVec3, b: DVec3) -> Self {
        Guide { pos: a, dir: b - a, infinite: false, layer: 0, hidden: false }
    }

    pub fn is_point(&self) -> bool {
        self.dir == DVec3::ZERO
    }

    /// Unit direction of a guide line.
    pub fn unit_dir(&self) -> Option<DVec3> {
        (!self.is_point()).then(|| self.dir.normalize())
    }

    pub fn transformed(&self, m: &DMat4) -> Guide {
        let pos = m.transform_point3(self.pos);
        let dir = m.transform_vector3(self.dir);
        let dir = if self.infinite { dir.normalize_or_zero() } else { dir };
        Guide { pos, dir, ..self.clone() }
    }
}

/// What an annotation is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AnnotKind {
    /// Linear dimension between two points; the dimension line is moved by `offset`.
    Linear { a: DVec3, b: DVec3, offset: DVec3 },
    /// Text pinned to a point, shown at `point + leader` with a leader line.
    Text { point: DVec3, leader: DVec3 },
    /// Text fixed on the screen, at a fraction of the view's width and height.
    ScreenText { x: f64, y: f64 },
}

/// A dimension or a text (Tools › Dimensions, Tools › Text).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Annotation {
    pub kind: AnnotKind,
    /// The text; for a dimension, empty means "show the measured length".
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub layer: LayerIdx,
    #[serde(default)]
    pub hidden: bool,
    /// What its ends are attached to (`attach.rs`): `[0]` a dimension's start or a text's
    /// point, `[1]` a dimension's end. The points of `kind` are where they were last worked out.
    #[serde(default, skip_serializing_if = "crate::attach::no_ends")]
    pub attach: crate::attach::Ends,
}

impl Annotation {
    pub fn transformed(&self, m: &DMat4) -> Annotation {
        let kind = match &self.kind {
            AnnotKind::Linear { a, b, offset } => AnnotKind::Linear {
                a: m.transform_point3(*a),
                b: m.transform_point3(*b),
                offset: m.transform_vector3(*offset),
            },
            AnnotKind::Text { point, leader } => {
                AnnotKind::Text { point: m.transform_point3(*point), leader: m.transform_vector3(*leader) }
            }
            k @ AnnotKind::ScreenText { .. } => k.clone(),
        };
        Annotation { kind, ..self.clone() }
    }
}

/// A section plane (Tools › Section Plane). The active one of a collection cuts away what
/// lies behind it, in that collection and in the groups/components it contains.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectionPlane {
    pub pos: DVec3,
    /// Unit normal, pointing like the arrows: towards the part that stays visible.
    pub normal: DVec3,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub layer: LayerIdx,
    #[serde(default)]
    pub hidden: bool,
    /// Right-click › Troubleshoot Section Fill: the cut shows what keeps it from being filled.
    /// A viewing aid of the session, as in SketchUp (no operation, not saved).
    #[serde(skip)]
    pub troubleshoot: bool,
}

impl SectionPlane {
    pub fn new(pos: DVec3, normal: DVec3) -> Self {
        SectionPlane {
            pos,
            normal: normal.normalize(),
            name: String::new(),
            symbol: String::new(),
            layer: 0,
            hidden: false,
            troubleshoot: false,
        }
    }

    pub fn plane(&self) -> Plane {
        Plane::from_point_normal(self.pos, self.normal)
    }

    /// Whether a point is cut away (it lies behind the plane).
    pub fn cuts(&self, p: DVec3) -> bool {
        (p - self.pos).dot(self.normal) < -crate::math::TOL
    }

    pub fn transformed(&self, m: &DMat4) -> SectionPlane {
        let normal = m.inverse().transpose().transform_vector3(self.normal).normalize_or_zero();
        SectionPlane { pos: m.transform_point3(self.pos), normal, ..self.clone() }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entities {
    pub verts: SlotMap<VertexId, Vertex>,
    pub edges: SlotMap<EdgeId, Edge>,
    pub faces: SlotMap<FaceId, Face>,
    pub instances: SlotMap<InstanceId, Instance>,
    #[serde(default)]
    pub guides: SlotMap<GuideId, Guide>,
    #[serde(default)]
    pub annots: SlotMap<AnnotId, Annotation>,
    #[serde(default)]
    pub sections: SlotMap<SectionId, SectionPlane>,
    /// The section plane that cuts this collection, if any.
    #[serde(default)]
    pub active_section: Option<SectionId>,
    #[serde(default)]
    pub next_curve: u32,
    /// Properties the Ruby API keeps per entity (material of an edge, arrow type of a text,
    /// kind of a curve…): entity key (`e12`, `g3`, `c4`…) → property → value.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub props: BTreeMap<String, BTreeMap<String, AttrValue>>,
    /// Persistent ids (`Entity#persistent_id`) the entities had in the .skp they were read from
    /// (SketchUp 2021+ keeps them in the file): entity key (`f12`, `e3`) → id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pids: BTreeMap<String, u64>,
    /// Snaps (Tools › Add Snap): where and how other components connect to this one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snaps: Vec<SnapPoint>,
    /// Point lights placed with the Light tool.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lights: Vec<PointLight>,
    /// Spaces (`Entities#add_space`): rooms outlined on the floor up to a ceiling plane.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spaces: Vec<Space>,
    /// 3D polylines of a .skp (`CPolyline3d`): drawn as lines, not geometry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub polylines: Vec<Polyline3d>,
    /// Last id given to a snap or a space of this collection (ids are never reused, so that
    /// a Ruby handle to an erased one stays invalid).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub last_mark_id: u32,
    /// What the last operations replaced, for the annotations attached to it (`attach.rs`):
    /// emptied by the next update, never saved.
    #[serde(skip)]
    pub replaced: Vec<crate::attach::Replaced>,
}

/// A 3D polyline (`CPolyline3d` of SketchUp, `SUPolyline3dRef`): points joined by lines, drawn
/// like edges but with no vertex, face or curve of the collection's geometry; kept as a .skp has
/// it (SketchUp makes them in some imports; it has no tool for them).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Polyline3d {
    pub points: Vec<DVec3>,
    #[serde(default)]
    pub layer: LayerIdx,
    #[serde(default)]
    pub hidden: bool,
}

impl Polyline3d {
    pub fn transformed(&self, m: &glam::DMat4) -> Polyline3d {
        Polyline3d { points: self.points.iter().map(|p| m.transform_point3(*p)).collect(), ..self.clone() }
    }
}

/// A space (`Sketchup::Space`, `CSpace` in SketchUp.exe): a named room, outlined by its floor
/// perimeter (with holes), up to a ceiling plane; the orientation point tells its front. Like
/// a snap, it is an entity of its collection: it can be selected, hidden, tagged, erased.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Space {
    /// Stable identity in its collection (see [`Entities::new_mark_id`]); 0 until numbered.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub id: u32,
    #[serde(default)]
    pub name: String,
    pub perimeter: Vec<DVec3>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<DVec3>>,
    pub orientation: DVec3,
    /// Ceiling plane `[a, b, c, d]`: the points where `a x + b y + c z + d = 0` (d in
    /// millimetres).
    pub ceiling: [f64; 4],
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub layer: LayerIdx,
}

impl Space {
    /// A space whose ceiling is the horizontal plane at `height` above the origin.
    pub fn new(name: impl Into<String>, perimeter: Vec<DVec3>, orientation: DVec3, height: f64) -> Self {
        Space { name: name.into(), perimeter, orientation, ceiling: [0.0, 0.0, 1.0, -height], ..Default::default() }
    }

    /// Where the vertical through `p` meets the ceiling (`p` itself when the ceiling is
    /// vertical).
    pub fn ceiling_point(&self, p: DVec3) -> DVec3 {
        let [a, b, c, d] = self.ceiling;
        if c.abs() < 1e-12 {
            return p;
        }
        DVec3::new(p.x, p.y, -(a * p.x + b * p.y + d) / c)
    }

    /// The outline drawn for the space: floor and ceiling loops and the vertical edges
    /// joining them, as segments.
    pub fn outline(&self) -> Vec<(DVec3, DVec3)> {
        let mut out = Vec::new();
        for ring in std::iter::once(&self.perimeter).chain(&self.holes) {
            let n = ring.len();
            for i in 0..n {
                let (p, q) = (ring[i], ring[(i + 1) % n]);
                let (pc, qc) = (self.ceiling_point(p), self.ceiling_point(q));
                out.push((p, q));
                out.push((pc, qc));
                out.push((p, pc));
            }
        }
        out
    }

    /// The same space moved by `m`: its points, and its ceiling plane.
    pub fn transformed(&self, m: &DMat4) -> Space {
        let tp = |p: &DVec3| m.transform_point3(*p);
        let [a, b, c, d] = self.ceiling;
        let n = DVec3::new(a, b, c);
        let ceiling = if n.length_squared() > 1e-24 {
            let p0 = n * (-d / n.length_squared());
            let n2 = m.inverse().transpose().transform_vector3(n).normalize();
            let p1 = m.transform_point3(p0);
            [n2.x, n2.y, n2.z, -n2.dot(p1)]
        } else {
            self.ceiling
        };
        Space {
            perimeter: self.perimeter.iter().map(tp).collect(),
            holes: self.holes.iter().map(|h| h.iter().map(tp).collect()).collect(),
            orientation: tp(&self.orientation),
            ceiling,
            ..self.clone()
        }
    }
}

/// A snap of a component (`Sketchup::Snap`): a position, the direction it faces and its up.
/// Like SketchUp's, it is an entity of its collection: it can be hidden, tagged, erased.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SnapPoint {
    pub pos: DVec3,
    pub dir: DVec3,
    pub up: DVec3,
    /// Stable identity (Ruby handles `n:<ctx>:<id>`, unaffected by erasing other snaps); 0
    /// until [`Model::add_snap`] or [`Model::ensure_snap_ids`] numbers it.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub id: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub layer: LayerIdx,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn is_zero_usize(v: &usize) -> bool {
    *v == 0
}

impl SnapPoint {
    pub fn new(pos: DVec3, dir: DVec3, up: DVec3) -> Self {
        SnapPoint { pos, dir, up, ..Default::default() }
    }

    /// The same snap moved by `m` (its directions stay unit vectors).
    pub fn transformed(&self, m: &DMat4) -> SnapPoint {
        SnapPoint {
            pos: m.transform_point3(self.pos),
            dir: m.transform_vector3(self.dir).normalize_or_zero(),
            up: m.transform_vector3(self.up).normalize_or_zero(),
            ..*self
        }
    }
}

/// A point light (SketchUp's `LightElement`): white, intensity 1, size 12 by default.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointLight {
    pub pos: DVec3,
    /// Linear RGB, 0..1.
    pub color: [f32; 3],
    pub intensity: f32,
    /// Size of the light's marker (inches in SketchUp's file, 12).
    pub size: f32,
}

impl PointLight {
    pub fn at(pos: DVec3) -> Self {
        PointLight { pos, color: [1.0; 3], intensity: 1.0, size: 12.0 }
    }
}

impl Entities {
    /// A fresh id for a snap or a space of this collection: above every id given so far.
    pub fn new_mark_id(&mut self) -> u32 {
        let top = self.snaps.iter().map(|s| s.id).chain(self.spaces.iter().map(|s| s.id)).max().unwrap_or(0);
        self.last_mark_id = self.last_mark_id.max(top) + 1;
        self.last_mark_id
    }

    /// Index of the snap with this id.
    pub fn snap_index(&self, id: u32) -> Option<usize> {
        (id != 0).then(|| self.snaps.iter().position(|s| s.id == id)).flatten()
    }

    /// Index of the space with this id.
    pub fn space_index(&self, id: u32) -> Option<usize> {
        (id != 0).then(|| self.spaces.iter().position(|s| s.id == id)).flatten()
    }

    pub fn snap(&self, id: u32) -> Option<&SnapPoint> {
        self.snap_index(id).map(|i| &self.snaps[i])
    }

    pub fn space(&self, id: u32) -> Option<&Space> {
        self.space_index(id).map(|i| &self.spaces[i])
    }

    pub fn space_mut(&mut self, id: u32) -> Option<&mut Space> {
        self.space_index(id).map(|i| &mut self.spaces[i])
    }

    /// Gives the snaps and spaces that have no id (or the id of another one) a fresh id.
    pub fn ensure_mark_ids(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let redo_snaps: Vec<usize> =
            (0..self.snaps.len()).filter(|&i| self.snaps[i].id == 0 || !seen.insert(self.snaps[i].id)).collect();
        let redo_spaces: Vec<usize> =
            (0..self.spaces.len()).filter(|&i| self.spaces[i].id == 0 || !seen.insert(self.spaces[i].id)).collect();
        for i in redo_snaps {
            self.snaps[i].id = 0;
        }
        for i in &redo_spaces {
            self.spaces[*i].id = 0;
        }
        for i in 0..self.snaps.len() {
            if self.snaps[i].id == 0 {
                self.snaps[i].id = self.new_mark_id();
            }
        }
        for i in redo_spaces {
            self.spaces[i].id = self.new_mark_id();
        }
    }

    /// Every snap and space of the collection, as selectable entities.
    pub fn mark_refs(&self) -> impl Iterator<Item = EntityRef> + '_ {
        self.snaps
            .iter()
            .filter(|s| s.id != 0)
            .map(|s| EntityRef::Snap(s.id))
            .chain(self.spaces.iter().filter(|s| s.id != 0).map(|s| EntityRef::Space(s.id)))
    }

    /// A property stored for an entity key (see `props`).
    pub fn prop(&self, key: &str, name: &str) -> Option<&AttrValue> {
        self.props.get(key)?.get(name)
    }

    pub fn set_prop(&mut self, key: &str, name: &str, value: AttrValue) {
        self.props.entry(key.to_string()).or_default().insert(name.to_string(), value);
    }

    pub fn remove_prop(&mut self, key: &str, name: &str) {
        if let Some(d) = self.props.get_mut(key) {
            d.remove(name);
            if d.is_empty() {
                self.props.remove(key);
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Definition {
    pub name: String,
    pub is_group: bool,
    #[serde(default)]
    pub description: String,
    pub entities: Entities,
    /// Attribute dictionaries shared by every instance (Tools › Component Attributes).
    #[serde(default, skip_serializing_if = "Attributes::is_empty")]
    pub attributes: Attributes,
    /// Behavior, thumbnail camera… (Ruby API).
    #[serde(default, skip_serializing_if = "DefExt::is_empty")]
    pub ext: DefExt,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// sRGB, 0-255.
    pub color: [u8; 3],
    /// 0 transparent .. 1 opaque.
    pub opacity: f32,
    /// Texture, colorization, hatch pattern and PBR settings (Ruby API).
    #[serde(default, skip_serializing_if = "is_default_ext")]
    pub ext: crate::meta::MaterialExt,
}

fn is_default_ext(e: &crate::meta::MaterialExt) -> bool {
    *e == crate::meta::MaterialExt::default()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    /// Stable identity (the index changes when tags are removed); 0 = not assigned yet.
    #[serde(default)]
    pub uid: u32,
    #[serde(default)]
    pub color: Option<[u8; 3]>,
    /// uid of the folder holding this tag.
    #[serde(default)]
    pub folder: Option<u32>,
    /// `Layer#page_behavior` flags (LAYER_HIDDEN_BY_DEFAULT, LAYER_IS_VISIBLE_ON_NEW_PAGES…).
    #[serde(default)]
    pub page_behavior: u32,
    #[serde(default)]
    pub line_style: String,
}

impl Layer {
    pub fn new(name: impl Into<String>) -> Layer {
        Layer {
            name: name.into(),
            visible: true,
            uid: 0,
            color: None,
            folder: None,
            page_behavior: 0,
            line_style: String::new(),
        }
    }
}

/// Point of view saved in a scene (also the one a .skp file opens with).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneCamera {
    pub eye: DVec3,
    pub target: DVec3,
    pub up: DVec3,
    pub fov_deg: f64,
    pub perspective: bool,
}

/// A scene (View › Animation, Window › Scenes): a named point of view, and optionally
/// which tags it hides.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub camera: SceneCamera,
    /// Tags hidden in this scene; `None` when the scene does not change tag visibility.
    #[serde(default)]
    pub hidden_layers: Option<Vec<LayerIdx>>,
    /// Match Photo: the scene shows this photograph behind the model, and `camera` is
    /// calibrated on it.
    #[serde(default)]
    pub photo: Option<crate::photo::MatchPhoto>,
    /// What the Ruby API (`Sketchup::Page`) keeps on a scene beyond the point of view.
    #[serde(default, skip_serializing_if = "SceneExt::is_empty")]
    pub ext: SceneExt,
}

/// Extra, free-form data of a scene: a stable identity (`Sketchup::Page` objects survive
/// reordering) and the properties that scenes have in SketchUp but the data model does
/// not (which features the scene saves, delay, transition, style…).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SceneExt {
    /// Never 0 once [`Model::ensure_scene_uids`] ran.
    #[serde(default)]
    pub uid: u64,
    #[serde(default)]
    pub props: BTreeMap<String, serde_json::Value>,
}

impl SceneExt {
    pub fn is_empty(&self) -> bool {
        self.uid == 0 && self.props.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Units {
    #[default]
    Millimeters,
    Centimeters,
    Meters,
    Inches,
    Feet,
    /// Model Info › Units offers yards too (`LengthUnit` 5).
    Yards,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub format_version: u32,
    pub root: Entities,
    pub defs: SlotMap<DefId, Definition>,
    pub materials: SlotMap<MaterialId, Material>,
    pub layers: Vec<Layer>,
    pub units: Units,
    /// Drawing axes placed with the Axes tool (identity = the world axes).
    #[serde(default = "identity")]
    pub axes: DMat4,
    #[serde(default)]
    pub scenes: Vec<Scene>,
    #[serde(default)]
    pub shadow: crate::sun::ShadowInfo,
    /// Name, description, tag folders, hatch patterns, options providers (Ruby API).
    #[serde(default)]
    pub meta: crate::meta::ModelMeta,
    /// Free-form state of the view that the Ruby API keeps with the model (rendering
    /// options not shown by the interface, styles, environments…), by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub view_ext: BTreeMap<String, serde_json::Value>,
    /// Attribute dictionaries of everything that has no field of its own (edges, faces,
    /// materials, tags, the model…), by owner key: `e:<ctx>:<id>`, `mat:<name>`, `model`…
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attr_store: BTreeMap<String, Attributes>,
    /// Files the model carries that have no other home (the HDR image of each environment…),
    /// by key (`env:<id>`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, crate::photo::Blob>,
    /// Tools › Comment: the comment threads pinned on the model (`comments.rs`).
    #[serde(default, skip_serializing_if = "crate::comments::Comments::is_empty")]
    pub comments: crate::comments::Comments,
    /// Which collections hold attached annotations (`attach.rs`), worked out when needed.
    #[serde(skip)]
    pub attached: crate::attach::Index,
}

fn identity() -> DMat4 {
    DMat4::IDENTITY
}

/// Key of `Model::view_ext`: index of the scene selected when the document was saved
/// (`Pages#selected_page`, SketchUp's `SUModelGetActiveScene`).
pub const ACTIVE_SCENE: &str = "active_scene";

impl Default for Model {
    fn default() -> Self {
        Model {
            format_version: 1,
            root: Entities::default(),
            defs: SlotMap::with_key(),
            materials: SlotMap::with_key(),
            layers: vec![Layer::new("Sans étiquette")],
            units: Units::Millimeters,
            axes: DMat4::IDENTITY,
            scenes: Vec::new(),
            shadow: Default::default(),
            meta: Default::default(),
            view_ext: BTreeMap::new(),
            attr_store: BTreeMap::new(),
            files: BTreeMap::new(),
            comments: Default::default(),
            attached: Default::default(),
        }
    }
}

/// A reference to something selectable inside one entities collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntityRef {
    Edge(EdgeId),
    Face(FaceId),
    Instance(InstanceId),
    Guide(GuideId),
    Annot(AnnotId),
    Section(SectionId),
    /// A snap, by its id ([`SnapPoint::id`]).
    Snap(u32),
    /// A space, by its id ([`Space::id`]).
    Space(u32),
}

impl Entities {
    pub fn contains(&self, e: EntityRef) -> bool {
        match e {
            EntityRef::Edge(x) => self.edges.contains_key(x),
            EntityRef::Face(x) => self.faces.contains_key(x),
            EntityRef::Instance(x) => self.instances.contains_key(x),
            EntityRef::Guide(x) => self.guides.contains_key(x),
            EntityRef::Annot(x) => self.annots.contains_key(x),
            EntityRef::Section(x) => self.sections.contains_key(x),
            EntityRef::Snap(x) => self.snap_index(x).is_some(),
            EntityRef::Space(x) => self.space_index(x).is_some(),
        }
    }

    /// Tag of an entity (None if it is gone).
    pub fn layer_of(&self, e: EntityRef) -> Option<LayerIdx> {
        Some(match e {
            EntityRef::Edge(x) => self.edges.get(x)?.layer,
            EntityRef::Face(x) => self.faces.get(x)?.layer,
            EntityRef::Instance(x) => self.instances.get(x)?.layer,
            EntityRef::Guide(x) => self.guides.get(x)?.layer,
            EntityRef::Annot(x) => self.annots.get(x)?.layer,
            EntityRef::Section(x) => self.sections.get(x)?.layer,
            EntityRef::Snap(x) => self.snap(x)?.layer,
            EntityRef::Space(x) => self.space(x)?.layer,
        })
    }

    pub fn set_layer(&mut self, e: EntityRef, layer: LayerIdx) {
        let slot = match e {
            EntityRef::Edge(x) => self.edges.get_mut(x).map(|v| &mut v.layer),
            EntityRef::Face(x) => self.faces.get_mut(x).map(|v| &mut v.layer),
            EntityRef::Instance(x) => self.instances.get_mut(x).map(|v| &mut v.layer),
            EntityRef::Guide(x) => self.guides.get_mut(x).map(|v| &mut v.layer),
            EntityRef::Annot(x) => self.annots.get_mut(x).map(|v| &mut v.layer),
            EntityRef::Section(x) => self.sections.get_mut(x).map(|v| &mut v.layer),
            EntityRef::Snap(x) => self.snap_index(x).map(|i| &mut self.snaps[i].layer),
            EntityRef::Space(x) => self.space_mut(x).map(|v| &mut v.layer),
        };
        if let Some(l) = slot {
            *l = layer;
        }
    }

    pub fn is_hidden(&self, e: EntityRef) -> bool {
        match e {
            EntityRef::Edge(x) => self.edges.get(x).is_some_and(|v| v.hidden),
            EntityRef::Face(x) => self.faces.get(x).is_some_and(|v| v.hidden),
            EntityRef::Instance(x) => self.instances.get(x).is_some_and(|v| v.hidden),
            EntityRef::Guide(x) => self.guides.get(x).is_some_and(|v| v.hidden),
            EntityRef::Annot(x) => self.annots.get(x).is_some_and(|v| v.hidden),
            EntityRef::Section(x) => self.sections.get(x).is_some_and(|v| v.hidden),
            EntityRef::Snap(x) => self.snap(x).is_some_and(|v| v.hidden),
            EntityRef::Space(x) => self.space(x).is_some_and(|v| v.hidden),
        }
    }

    pub fn set_hidden(&mut self, e: EntityRef, hidden: bool) {
        let slot = match e {
            EntityRef::Edge(x) => self.edges.get_mut(x).map(|v| &mut v.hidden),
            EntityRef::Face(x) => self.faces.get_mut(x).map(|v| &mut v.hidden),
            EntityRef::Instance(x) => self.instances.get_mut(x).map(|v| &mut v.hidden),
            EntityRef::Guide(x) => self.guides.get_mut(x).map(|v| &mut v.hidden),
            EntityRef::Annot(x) => self.annots.get_mut(x).map(|v| &mut v.hidden),
            EntityRef::Section(x) => self.sections.get_mut(x).map(|v| &mut v.hidden),
            EntityRef::Snap(x) => self.snap_index(x).map(|i| &mut self.snaps[i].hidden),
            EntityRef::Space(x) => self.space_mut(x).map(|v| &mut v.hidden),
        };
        if let Some(h) = slot {
            *h = hidden;
        }
    }

    pub fn pos(&self, v: VertexId) -> DVec3 {
        self.verts[v].pos
    }

    pub fn loop_points(&self, l: &[VertexId]) -> Vec<DVec3> {
        l.iter().map(|&v| self.verts[v].pos).collect()
    }

    pub fn face_plane(&self, f: FaceId) -> Plane {
        let face = &self.faces[f];
        Plane::from_point_normal(self.verts[face.outer[0]].pos, face.normal)
    }

    pub fn find_edge(&self, a: VertexId, b: VertexId) -> Option<EdgeId> {
        self.verts.get(a)?.edges.iter().copied().find(|&e| {
            let ev = self.edges[e].v;
            (ev[0] == a && ev[1] == b) || (ev[0] == b && ev[1] == a)
        })
    }

    pub fn other_vertex(&self, e: EdgeId, v: VertexId) -> VertexId {
        let ev = self.edges[e].v;
        if ev[0] == v { ev[1] } else { ev[0] }
    }

    pub fn edge_points(&self, e: EdgeId) -> (DVec3, DVec3) {
        let ev = self.edges[e].v;
        (self.verts[ev[0]].pos, self.verts[ev[1]].pos)
    }

    /// Edges bounding a face, in loop order.
    pub fn face_edges(&self, f: FaceId) -> Vec<EdgeId> {
        let face = &self.faces[f];
        let mut out = Vec::new();
        for l in face.loops() {
            for i in 0..l.len() {
                if let Some(e) = self.find_edge(l[i], l[(i + 1) % l.len()]) {
                    out.push(e);
                }
            }
        }
        out
    }

    pub fn new_curve_id(&mut self) -> u32 {
        self.next_curve += 1;
        self.next_curve
    }

    /// Rebuilds the vertex→edge and edge→face back-references (they are not serialized).
    pub fn rebuild_topology(&mut self) {
        for (_, v) in self.verts.iter_mut() {
            v.edges.clear();
        }
        for (_, e) in self.edges.iter_mut() {
            e.faces.clear();
        }
        let ids: Vec<EdgeId> = self.edges.keys().collect();
        for e in ids {
            let [a, b] = self.edges[e].v;
            self.verts[a].edges.push(e);
            self.verts[b].edges.push(e);
        }
        let fids: Vec<FaceId> = self.faces.keys().collect();
        for f in fids {
            for e in self.face_edges(f) {
                if !self.edges[e].faces.contains(&f) {
                    self.edges[e].faces.push(f);
                }
            }
        }
    }

    /// Adds an edge between two existing vertices, or returns the one already there.
    pub fn add_edge_raw(&mut self, a: VertexId, b: VertexId) -> Option<EdgeId> {
        if a == b {
            return None;
        }
        if let Some(e) = self.find_edge(a, b) {
            return Some(e);
        }
        let e = self.edges.insert(Edge {
            v: [a, b],
            soft: false,
            smooth: false,
            hidden: false,
            layer: 0,
            curve: None,
            faces: Vec::new(),
        });
        self.verts[a].edges.push(e);
        self.verts[b].edges.push(e);
        Some(e)
    }

    pub fn add_vertex_raw(&mut self, p: DVec3) -> VertexId {
        self.verts.insert(Vertex { pos: p, edges: Vec::new() })
    }

    /// Adds a face from vertex loops, creating missing edges. The normal is computed from
    /// the outer loop order (counter-clockwise = front).
    pub fn add_face_raw(&mut self, outer: Vec<VertexId>, holes: Vec<Vec<VertexId>>) -> Option<FaceId> {
        if outer.len() < 3 {
            return None;
        }
        let normal = newell_normal(&self.loop_points(&outer));
        if normal == DVec3::ZERO {
            return None;
        }
        let f = self.faces.insert(Face {
            outer,
            holes,
            normal,
            front: None,
            back: None,
            layer: 0,
            hidden: false,
            tex: None,
        });
        self.link_face(f);
        Some(f)
    }

    pub(crate) fn link_face(&mut self, f: FaceId) {
        let loops: Vec<Vec<VertexId>> = self.faces[f].loops().cloned().collect();
        for l in loops {
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                if let Some(e) = self.add_edge_raw(a, b)
                    && !self.edges[e].faces.contains(&f)
                {
                    self.edges[e].faces.push(f);
                }
            }
        }
    }

    pub fn remove_face(&mut self, f: FaceId) {
        let edges = self.face_edges(f);
        for e in edges {
            if let Some(edge) = self.edges.get_mut(e) {
                edge.faces.retain(|&x| x != f);
            }
        }
        self.faces.remove(f);
    }

    /// Removes an edge only (faces using it must already be gone or will be left dangling).
    pub fn remove_edge_raw(&mut self, e: EdgeId) {
        if let Some(edge) = self.edges.remove(e) {
            for v in edge.v {
                if let Some(vx) = self.verts.get_mut(v) {
                    vx.edges.retain(|&x| x != e);
                }
            }
        }
    }

    pub fn remove_orphan_vertices(&mut self) {
        let orphans: Vec<VertexId> = self.verts.iter().filter(|(_, v)| v.edges.is_empty()).map(|(k, _)| k).collect();
        for v in orphans {
            self.verts.remove(v);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
            && self.faces.is_empty()
            && self.instances.is_empty()
            && self.guides.is_empty()
            && self.annots.is_empty()
            && self.sections.is_empty()
    }

    pub fn recompute_normal(&mut self, f: FaceId) {
        let pts = self.loop_points(&self.faces[f].outer);
        let n = newell_normal(&pts);
        if n != DVec3::ZERO {
            self.faces[f].normal = n;
        }
    }

    /// Turns a face over for a modelling operation (Push/Pull, Follow Me, importers…): the loops are
    /// reversed and each side's material goes with its placement to the other side, so that the
    /// paint stays where it was seen. SketchUp's Reverse Face is [`Self::flip_face`].
    pub fn reverse_face(&mut self, f: FaceId) {
        let face = &mut self.faces[f];
        face.outer.reverse();
        for h in face.holes.iter_mut() {
            h.reverse();
        }
        face.normal = -face.normal;
        std::mem::swap(&mut face.front, &mut face.back);
        // Each side's texture placement goes with its material: the texture stays where it is.
        if let Some(t) = face.tex.as_mut() {
            std::mem::swap(&mut t.front, &mut t.back);
            t.projection.swap(0, 1);
            t.pins.swap(0, 1);
        }
    }

    /// SketchUp's Reverse Face (`Face#reverse!`, the Reverse Faces command, `SUFaceReverse`:
    /// negates the plane and reverses the loops): the face turns over, its
    /// materials and texture placements stay on their side. SketchUp keeps each placement's
    /// matrix, which the turned plane's frame mirrors (its x axis turns, its origin and y axis
    /// stay: `docs/skp-format.md` §2.3.3); a projected placement keeps its own frame.
    pub fn flip_face(&mut self, f: FaceId) {
        let Some(face) = self.faces.get(f) else { return };
        let n = face.normal;
        let o = face.outer.first().and_then(|v| self.verts.get(*v)).map_or(DVec3::ZERO, |v| n * n.dot(v.pos));
        let (ax, _) = crate::meta::face_axes(n);
        // The reflection across the plane through the frame's origin normal to its x axis.
        let refl = |v: DVec3| v - ax * (2.0 * v.dot(ax));
        let r = DMat4::from_cols(
            refl(DVec3::X).extend(0.0),
            refl(DVec3::Y).extend(0.0),
            refl(DVec3::Z).extend(0.0),
            (ax * (2.0 * o.dot(ax))).extend(1.0),
        );
        let face = &mut self.faces[f];
        face.outer.reverse();
        for h in face.holes.iter_mut() {
            h.reverse();
        }
        face.normal = -face.normal;
        if let Some(t) = face.tex.as_mut() {
            for k in 0..2 {
                if t.projection[k].is_some() {
                    continue;
                }
                let side = if k == 0 { &mut t.front } else { &mut t.back };
                if let Some(m) = side.as_mut() {
                    *m = crate::image::transform_uv(m, &r);
                }
                for (p, _) in t.pins[k].iter_mut() {
                    *p = r.transform_point3(*p);
                }
            }
        }
    }
}

impl Model {
    pub fn new() -> Self {
        Self::default()
    }

    /// Entities collection for a definition, or the root when `None`.
    pub fn entities(&self, def: Option<DefId>) -> &Entities {
        match def {
            Some(d) => &self.defs[d].entities,
            None => &self.root,
        }
    }

    pub fn entities_mut(&mut self, def: Option<DefId>) -> &mut Entities {
        match def {
            Some(d) => &mut self.defs[d].entities,
            None => &mut self.root,
        }
    }

    /// Adds a snap to a collection (Tools › Add Snap, `Entities#add_snap`) and returns the
    /// stable id it gets.
    pub fn add_snap(&mut self, def: Option<DefId>, mut s: SnapPoint) -> u32 {
        let e = self.entities_mut(def);
        s.id = e.new_mark_id();
        e.snaps.push(s);
        s.id
    }

    /// Adds a space to a collection (`Entities#add_space`) and returns its id.
    pub fn add_space(&mut self, def: Option<DefId>, mut s: Space) -> u32 {
        let e = self.entities_mut(def);
        let id = e.new_mark_id();
        s.id = id;
        e.spaces.push(s);
        id
    }

    /// Numbers the snaps and spaces that have no id yet, or the same id as another one of
    /// their collection (older documents, copies, files read from a .skp).
    pub fn ensure_snap_ids(&mut self) {
        self.migrate_spaces();
        self.root.ensure_mark_ids();
        for d in self.defs.values_mut() {
            d.entities.ensure_mark_ids();
        }
    }

    /// Spaces of documents saved before they were entities of their collection: records of
    /// `view_ext["spaces"]` in inches, each naming its collection (`ctx`, "0" = the model).
    pub fn migrate_spaces(&mut self) {
        let Some(serde_json::Value::Array(list)) = self.view_ext.remove("spaces") else { return };
        self.view_ext.remove("spaces_next");
        let inch = |j: &serde_json::Value| -> Option<DVec3> {
            let a = j.as_array()?;
            Some(DVec3::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?) * 25.4)
        };
        for s in list {
            let ctx = match s.get("ctx").and_then(|c| c.as_str()).unwrap_or("0") {
                "0" => None,
                c => match c.parse::<u64>().map(DefId::from_ffi) {
                    Ok(d) if self.defs.contains_key(d) => Some(d),
                    _ => continue,
                },
            };
            let pts = |k: &str| -> Vec<DVec3> {
                s.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(inch).collect()).unwrap_or_default()
            };
            let holes = s
                .get("holes")
                .and_then(|v| v.as_array())
                .map(|hs| hs.iter().filter_map(|h| h.as_array()).map(|h| h.iter().filter_map(inch).collect()).collect())
                .unwrap_or_default();
            let c: Vec<f64> = s
                .get("ceiling")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect())
                .unwrap_or_default();
            let ceiling = if c.len() == 4 { [c[0], c[1], c[2], c[3] * 25.4] } else { [0.0, 0.0, 1.0, 0.0] };
            let space = Space {
                id: s.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                name: s.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                perimeter: pts("perimeter"),
                holes,
                orientation: s.get("orientation").and_then(inch).unwrap_or_default(),
                ceiling,
                hidden: s.get("hidden").and_then(|v| v.as_bool()).unwrap_or(false),
                layer: s.get("layer").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
            };
            let e = self.entities_mut(ctx);
            e.last_mark_id = e.last_mark_id.max(space.id);
            e.spaces.push(space);
        }
    }

    pub fn rebuild_topology(&mut self) {
        self.root.rebuild_topology();
        for (_, d) in self.defs.iter_mut() {
            d.entities.rebuild_topology();
        }
    }

    pub fn add_material(&mut self, name: &str, color: [u8; 3]) -> MaterialId {
        self.materials.insert(Material { name: name.into(), color, opacity: 1.0, ext: Default::default() })
    }

    pub fn find_material(&self, name: &str) -> Option<MaterialId> {
        self.materials.iter().find(|(_, m)| m.name == name).map(|(k, _)| k)
    }

    pub fn unique_def_name(&self, base: &str) -> String {
        let mut i = 1;
        loop {
            let name = format!("{base}#{i}");
            if !self.defs.values().any(|d| d.name == name) {
                return name;
            }
            i += 1;
        }
    }

    pub fn instance_count(&self, def: DefId) -> usize {
        let mut n = self.root.instances.values().filter(|i| i.def == def).count();
        for d in self.defs.values() {
            n += d.entities.instances.values().filter(|i| i.def == def).count();
        }
        n
    }

    /// Edit › Purge unused assets: definitions, materials and tags that nothing uses.
    /// Returns how many items were removed.
    pub fn purge_unused(&mut self) -> usize {
        let defs_before = self.defs.len();
        self.purge_unused_defs();
        let mut n = defs_before - self.defs.len();

        let mut used_mats = std::collections::HashSet::new();
        let mut used_layers = std::collections::HashSet::from([0usize]);
        let mut note = |e: &Entities| {
            for f in e.faces.values() {
                used_mats.extend(f.front);
                used_mats.extend(f.back);
                used_layers.insert(f.layer);
            }
            for x in e.edges.values() {
                used_layers.insert(x.layer);
            }
            for i in e.instances.values() {
                used_mats.extend(i.material);
                used_layers.insert(i.layer);
            }
            for g in e.guides.values() {
                used_layers.insert(g.layer);
            }
            for a in e.annots.values() {
                used_layers.insert(a.layer);
            }
            for x in e.sections.values() {
                used_layers.insert(x.layer);
            }
            for l in e.snaps.iter().map(|s| s.layer).chain(e.spaces.iter().map(|s| s.layer)) {
                used_layers.insert(l);
            }
            for l in e.polylines.iter().map(|s| s.layer) {
                used_layers.insert(l);
            }
        };
        note(&self.root);
        for d in self.defs.values() {
            note(&d.entities);
        }
        let unused: Vec<MaterialId> = self.materials.keys().filter(|m| !used_mats.contains(m)).collect();
        n += unused.len();
        for m in unused {
            self.materials.remove(m);
        }
        // Tags are indices: renumber the ones that stay.
        let keep: Vec<usize> = (0..self.layers.len()).filter(|l| used_layers.contains(l)).collect();
        if keep.len() != self.layers.len() {
            n += self.layers.len() - keep.len();
            let map: std::collections::HashMap<usize, usize> =
                keep.iter().enumerate().map(|(new, &old)| (old, new)).collect();
            self.layers = keep.iter().map(|&l| self.layers[l].clone()).collect();
            let remap = |e: &mut Entities| {
                e.faces.values_mut().for_each(|f| f.layer = map[&f.layer]);
                e.edges.values_mut().for_each(|x| x.layer = map[&x.layer]);
                e.instances.values_mut().for_each(|i| i.layer = map[&i.layer]);
                e.guides.values_mut().for_each(|g| g.layer = map[&g.layer]);
                e.annots.values_mut().for_each(|a| a.layer = map[&a.layer]);
                e.sections.values_mut().for_each(|x| x.layer = map[&x.layer]);
                e.snaps.iter_mut().for_each(|x| x.layer = map[&x.layer]);
                e.spaces.iter_mut().for_each(|x| x.layer = map[&x.layer]);
                e.polylines.iter_mut().for_each(|x| x.layer = map[&x.layer]);
            };
            remap(&mut self.root);
            for d in self.defs.values_mut() {
                remap(&mut d.entities);
            }
            for s in self.scenes.iter_mut() {
                if let Some(h) = &mut s.hidden_layers {
                    *h = h.iter().filter_map(|l| map.get(l).copied()).collect();
                }
            }
        }
        n
    }

    /// Gives every scene an identity (see [`SceneExt::uid`]).
    pub fn ensure_scene_uids(&mut self) {
        let mut next = self.scenes.iter().map(|s| s.ext.uid).max().unwrap_or(0) + 1;
        let mut seen = std::collections::HashSet::new();
        for s in &mut self.scenes {
            if s.ext.uid == 0 || !seen.insert(s.ext.uid) {
                s.ext.uid = next;
                seen.insert(next);
                next += 1;
            }
        }
    }

    /// Unique scene name: "Scène 1", "Scène 2"…
    pub fn next_scene_name(&self) -> String {
        (1..).map(|i| format!("Scène {i}")).find(|n| !self.scenes.iter().any(|s| &s.name == n)).unwrap()
    }

    /// Removes definitions that no instance refers to any more.
    pub fn purge_unused_defs(&mut self) {
        loop {
            let unused: Vec<DefId> = self.defs.keys().filter(|&d| self.instance_count(d) == 0).collect();
            if unused.is_empty() {
                break;
            }
            for d in unused {
                self.defs.remove(d);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_props_are_saved_and_emptied_keys_go_away() {
        let mut e = Entities::default();
        e.set_prop("e1", "material", AttrValue::Text("Brique".into()));
        e.set_prop("e1", "casts", AttrValue::Bool(false));
        assert_eq!(e.prop("e1", "casts"), Some(&AttrValue::Bool(false)));
        let json = serde_json::to_string(&e).unwrap();
        let back: Entities = serde_json::from_str(&json).unwrap();
        assert_eq!(back.prop("e1", "material"), Some(&AttrValue::Text("Brique".into())));
        e.remove_prop("e1", "material");
        e.remove_prop("e1", "casts");
        assert!(e.props.is_empty());
        // Old files without the field still load.
        let old: Entities = serde_json::from_str(&json.replace(",\"props\"", ",\"unused\"")).unwrap();
        assert!(old.props.is_empty());
    }

    #[test]
    fn definition_keys_round_trip_as_numbers() {
        let mut m = Model::new();
        let d = m.defs.insert(Definition {
            name: "x".into(),
            is_group: false,
            description: String::new(),
            entities: Entities::default(),
            attributes: Default::default(),
            ext: Default::default(),
        });
        assert_eq!(DefId::from_ffi(d.to_ffi()), d);
    }

    #[test]
    fn snaps_get_stable_ids_and_follow_their_tag() {
        let mut m = Model::new();
        let a = m.add_snap(None, SnapPoint::new(DVec3::X, DVec3::X, DVec3::Z));
        let b = m.add_snap(None, SnapPoint::new(DVec3::Y, DVec3::Y, DVec3::Z));
        assert!(a != 0 && b != 0 && a != b);
        // Snaps of older documents (no id) and copies (same id) are numbered anew.
        let json = serde_json::to_string(&SnapPoint::new(DVec3::Z, DVec3::X, DVec3::Z)).unwrap();
        assert!(!json.contains("\"id\""), "{json}");
        let old: SnapPoint = serde_json::from_str(&json).unwrap();
        m.root.snaps.push(old);
        m.root.snaps.push(m.root.snaps[0]);
        m.ensure_snap_ids();
        let ids: std::collections::HashSet<u32> = m.root.snaps.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), 4);
        assert_eq!((m.root.snaps[0].id, m.root.snaps[1].id), (a, b));
        // Deleting a tag gives its snaps to the default one, or deletes them.
        m.layers.push(Layer::new("Ancrages"));
        m.layers.push(Layer::new("Autre"));
        m.root.snaps[0].layer = 1;
        m.root.snaps[1].layer = 2;
        m.remove_layer(1, true);
        assert_eq!(m.root.snaps.len(), 3);
        assert_eq!(m.root.snaps[0].layer, 1);
    }

    #[test]
    fn spaces_are_entities_that_group_explode_copy_and_migrate() {
        let mut m = Model::new();
        let room = vec![DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, 500.0, 0.0)];
        let p = m.add_space(None, Space::new("Salon", room, DVec3::new(10.0, 10.0, 0.0), 2500.0));
        let n = m.add_snap(None, SnapPoint::new(DVec3::new(5.0, 0.0, 0.0), DVec3::X, DVec3::Z));
        assert!(p != n && m.root.contains(EntityRef::Space(p)) && m.root.contains(EntityRef::Snap(n)));
        // Moved: points and ceiling plane follow.
        let up = DMat4::from_translation(DVec3::new(0.0, 0.0, 100.0));
        m.root.transform_selection(&[EntityRef::Space(p)], &up);
        let s = m.root.space(p).unwrap();
        assert!((s.perimeter[1].z - 100.0).abs() < 1e-9 && (s.ceiling[3] + 2600.0).abs() < 1e-9, "{s:?}");
        assert!((s.ceiling_point(DVec3::new(3.0, 4.0, 0.0)).z - 2600.0).abs() < 1e-9);
        // Copied with a fresh id that is never given again, even once erased.
        let c = m.root.duplicate(&[EntityRef::Space(p), EntityRef::Snap(n)]);
        assert_eq!(c.len(), 2);
        let EntityRef::Space(q) = c[0] else { panic!("{c:?}") };
        assert!(q > p.max(n));
        m.root.erase(&[EntityRef::Space(q)]);
        assert!(m.root.new_mark_id() > q);
        // Grouped and exploded with the rest.
        let g = m.make_group(None, &[EntityRef::Space(p), EntityRef::Snap(n)], true, None).unwrap();
        assert!(m.root.spaces.is_empty() && m.root.snaps.len() == 1, "the copied snap stays");
        let d = m.root.instances[g].def;
        assert_eq!((m.defs[d].entities.spaces.len(), m.defs[d].entities.snaps.len()), (1, 1));
        let out = m.explode(None, g);
        assert_eq!(out.iter().filter(|e| matches!(e, EntityRef::Space(_) | EntityRef::Snap(_))).count(), 2);
        let s = &m.root.spaces[0];
        assert!((s.perimeter[1] - DVec3::new(1000.0, 0.0, 100.0)).length() < 1e-9, "{s:?}");
        // Spaces of older documents (Ruby records in view_ext, inches) become entities.
        let mut old = Model::new();
        old.view_ext.insert(
            "spaces".into(),
            serde_json::json!([{"id": 7, "ctx": "0", "name": "Cuisine", "orientation": [1, 1, 0],
                "perimeter": [[0, 0, 0], [10, 0, 0], [10, 10, 0]], "holes": [], "ceiling": [0, 0, 1, -96],
                "hidden": true, "layer": 0}]),
        );
        old.ensure_snap_ids();
        let s = &old.root.spaces[0];
        assert_eq!((s.id, s.name.as_str(), s.hidden), (7, "Cuisine", true));
        assert!((s.perimeter[1].x - 254.0).abs() < 1e-9 && (s.ceiling[3] + 96.0 * 25.4).abs() < 1e-9);
        assert!(!old.view_ext.contains_key("spaces") && old.root.new_mark_id() == 8);
    }
}
