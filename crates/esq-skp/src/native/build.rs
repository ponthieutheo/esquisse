//! A [`Doc`] as an Esquisse model, made exactly as the SDK reader (`crate::Reader`) makes it:
//! same order of creation, same names for group definitions, same side tables. The
//! differential test (`tests/native_oracle.rs`) compares the two.

use std::collections::{HashMap, HashSet};

use esq_core::glam::{DMat4, DVec3};
use esq_core::meta::{MaterialExt, TextureData};
use esq_core::model::{
    AttrValue, Attributes, DefId, Definition, Entities, Instance, Layer, LayerIdx, MaterialId, Model, Units, VertexId,
};
use esq_core::photo::Blob;
use serde_json::Value;
use slotmap::Key;

use super::doc::{self, Doc, Ents, Id, flags};

const IN: f64 = 25.4;

const IDENTITY3: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn v3(p: [f64; 3]) -> DVec3 {
    DVec3::new(p[0], p[1], p[2])
}

fn xf_to_mm(x: &[f64; 16]) -> DMat4 {
    let mut m = DMat4::from_cols_array(x);
    m.w_axis.x *= IN;
    m.w_axis.y *= IN;
    m.w_axis.z *= IN;
    m
}

struct B<'d> {
    doc: &'d Doc,
    model: Model,
    materials: HashMap<Id, MaterialId>,
    layers: HashMap<Id, LayerIdx>,
    defs: HashMap<Id, DefId>,
    def_by_id: HashMap<Id, &'d doc::Def>,
    mat_by_id: HashMap<Id, &'d doc::Material>,
    /// The hatch patterns read: their uid and name in Esquisse.
    hatches: HashMap<Id, (u32, String)>,
    pending: Vec<(char, u64, Attributes)>,
    /// Definitions being read: a forged file whose definitions contain each other is cut there.
    open: HashSet<Id>,
    /// Names of the definitions and materials of the model, for the names the builder makes
    /// unique: a forged file with thousands of groups or images must not make each new name
    /// scan the model again (that was cubic in their number).
    def_names: Names,
    mat_names: Names,
    /// The texture of each image definition (the first textured face of its definition).
    image_tex: HashMap<Id, Option<&'d doc::Texture>>,
    /// Collections built (the measure of the work, for the tests).
    visited: usize,
    /// What each entity of the file (by id) became, for the annotations attached to it.
    keys: HashMap<Id, Made>,
    /// The attaches of the collection being built, and those of every collection once built.
    pending_attach: Vec<PendAttach>,
    attaches: Vec<(Option<DefId>, PendAttach)>,
}

/// What an entity of the file became in Esquisse (an edge of a curve also names its curve).
#[derive(Clone, Copy, Debug)]
enum Made {
    Vertex(esq_core::model::VertexId),
    Edge(esq_core::model::EdgeId),
    Face(esq_core::model::FaceId),
    Guide(esq_core::model::GuideId),
    Section(esq_core::model::SectionId),
    Instance(esq_core::model::InstanceId),
    /// A curve, by one of its edges.
    Curve(esq_core::model::EdgeId),
}

/// An annotation's end attached in the file: the annotation, which end, the connection, where
/// the reader placed it (mm, in the annotation's collection).
type PendAttach = (esq_core::model::AnnotId, usize, doc::Connection, DVec3);

/// The kind of `doc::Connection` Esquisse gives the end of a radial dimension on its arc (SketchUp
/// keeps the arc and the angle in `CDimensionRadial`, not as a connection).
const ON_ARC: i32 = -1;

/// What `B::entities` leaves to do once the collection has its place (`B::store_attrs`).
type Pending = (Vec<(char, u64, Attributes)>, Vec<PendAttach>);

/// Names taken, and the next index to try for each base: names are only ever added while the
/// model is built, so the smallest free index of a base never decreases.
#[derive(Default)]
struct Names {
    taken: HashSet<String>,
    next: HashMap<(bool, String), usize>,
    /// Candidates tried, all bases together (the measure of the work, for the tests).
    probes: usize,
}

impl Names {
    fn add(&mut self, name: &str) {
        if !self.taken.contains(name) {
            self.taken.insert(name.to_string());
        }
    }

    /// As `Model::unique_name`: `base` if it is free, otherwise `base1`, `base2`…
    fn plain(&mut self, base: &str) -> String {
        let name = if self.taken.contains(base) { self.indexed(base, false) } else { base.to_string() };
        self.add(&name);
        name
    }

    /// As `Model::unique_def_name`: `base#1`, `base#2`…
    fn hashed(&mut self, base: &str) -> String {
        let name = self.indexed(base, true);
        self.add(&name);
        name
    }

    fn indexed(&mut self, base: &str, hash: bool) -> String {
        let i = self.next.entry((hash, base.to_string())).or_insert(1);
        loop {
            self.probes += 1;
            let name = if hash { format!("{base}#{i}") } else { format!("{base}{i}") };
            *i += 1;
            if !self.taken.contains(&name) {
                return name;
            }
        }
    }
}

/// Deepest nesting of definitions followed: far beyond real models (a few dozen levels), within
/// the stack of `super::on_big_stack` (measured: 10 000 levels fit).
const MAX_NESTING: usize = 4096;

/// Point of view the file was saved with (millimetres).
pub fn view(doc: &Doc) -> Option<esq_core::model::SceneCamera> {
    doc.camera.map(camera)
}

/// The target SketchUp keeps for a camera read as an object (`CCamera::Serialize` →
/// `SetOrientation`): no nearer to the eye than a tenth of the larger of their
/// distances to the origin (pushed along the direction of view). A camera of the old format
/// (eye and direction, written in place) keeps eye + direction.
pub(crate) fn settled_target(eye: [f64; 3], target: [f64; 3]) -> [f64; 3] {
    let (eye, target) = (v3(eye), v3(target));
    let least = target.length().max(eye.length()) * 0.1;
    let d = target - eye;
    let l = d.length();
    let t = if l < least {
        let dir = if l >= 1e-10 { d / l } else { DVec3::ZERO };
        eye + dir * least
    } else {
        target
    };
    t.to_array()
}

fn camera(c: doc::Camera) -> esq_core::model::SceneCamera {
    esq_core::model::SceneCamera {
        eye: v3(c.eye) * IN,
        target: v3(c.target) * IN,
        up: v3(c.up),
        // `SUCameraGetPerspectiveFrustumFOV` fails on a parallel camera: the reader keeps 35°.
        fov_deg: if c.perspective { c.fov } else { 35.0 },
        perspective: c.perspective,
    }
}

/// The texture map of one side of a face (`CFaceTextureCoords`), as SketchUp's UV helper
/// computes it (`docs/skp-format.md` §2.3.3): the frame of the face's plane, or of the
/// projection for a projected side; the stored matrix (texture inches to the frame) inverted
/// and divided by the tile. The map is SketchUp's whole function of the point, off the face
/// too (a projected texture keeps its coordinates along the projection).
fn uv_map(plane: [f64; 4], tc: &doc::TexCoords, tile: [f64; 2]) -> Option<esq_core::meta::UvMap> {
    let n = DVec3::new(plane[0], plane[1], plane[2]);
    let len = n.length();
    if len < 1e-12 {
        return None;
    }
    let n = n / len;
    let origin = -n * (plane[3] / len * IN);
    let projection = tc.projection.map(v3).filter(|_| tc.projected);
    let frame = esq_core::texpos::Frame::of_side(origin, n, projection);
    esq_core::texpos::map_from_sketchup(&frame, &tc.matrix, tile)
}
/// Hue (degrees, −1 for a grey), lightness and saturation (0–1) of a colour, computed as
/// SketchUp computes them (components divided by 255 in single precision, comparisons with a
/// tolerance of 0.001; read in SketchUpAPI.dll).
fn hls(c: [u8; 3]) -> (f64, f64, f64) {
    const EPS: f64 = 0.001;
    let [r, g, b] = c.map(|x| (x as f32 / 255.0f32) as f64);
    let mut max = r;
    if EPS <= g - max {
        max = g;
    }
    if EPS <= b - max {
        max = b;
    }
    let mut min = r;
    if g - r <= -EPS {
        min = g;
    }
    if b - min <= -EPS {
        min = b;
    }
    let l = (min + max) * 0.5;
    if (min - max).abs() < EPS {
        return (-1.0, l, 0.0);
    }
    let l2 = l - 0.5;
    let s = if l2 <= -EPS || l2.abs() < EPS { (max - min) / (min + max) } else { (max - min) / ((2.0 - max) - min) };
    let d = max - min;
    let (rc, gc, bc) = ((max - r) / d, (max - g) / d, (max - b) / d);
    let mut h = 0.0;
    if (r - max).abs() >= EPS {
        if (g - max).abs() >= EPS {
            if (b - max).abs() < EPS {
                h = (gc + 4.0) - rc;
            }
        } else {
            h = (rc + 2.0) - bc;
        }
    } else {
        h = bc - gc;
    }
    h *= 60.0;
    if h - 0.0 <= -EPS {
        h += 360.0;
    }
    (h, l, s)
}

/// `SUMaterialGetColorizeDeltas`: hue, saturation and lightness offsets from the texture's
/// average colour to the material's colour (the hue itself when the material tints).
fn colorize_deltas(avg: [u8; 3], color: [u8; 3], shift: bool) -> [f64; 3] {
    let grey = |c: [u8; 3]| c[0] == c[1] && c[0] == c[2];
    let (h0, l0, s0) = if avg == [0, 0, 0] { (0.0, 0.0, 0.0) } else { hls(avg) };
    let (mut h, mut l, mut s) = hls(color);
    if grey(color) {
        h = 0.0;
        s = if grey(avg) { 0.0 } else { -1.0 };
        l -= l0;
    } else {
        l -= l0;
        if !grey(avg) {
            s -= s0;
            if shift {
                h -= h0;
            }
        }
    }
    [h, s, l]
}

/// The name SketchUp gives a texture: the file's, with the extension of the image's format
/// when it has none of an image (`photo` → `photo.jpg`, `S.` → `S..jpg`, `S. ` → `S. .jpg`:
/// the dots and spaces that end the name stay before the extension). Only then is it made a
/// bare name (`base_name`), which a name ending with an extension passes unchanged.
fn texture_name(t: &doc::Texture) -> String {
    const IMAGES: &[&str] = &["jpg", "jpeg", "png", "bmp", "gif", "tif", "tiff", "tga", "psd", "dib", "jpe"];
    let has = |n: &str| n.rsplit_once('.').is_some_and(|(_, e)| IMAGES.contains(&e.to_ascii_lowercase().as_str()));
    let name = esq_core::meta::base_name(&t.file_name);
    // The last part of the path, as written.
    let raw = t.file_name.rsplit(['/', '\\']).next().unwrap_or("");
    let raw = raw.rsplit(':').next().unwrap_or("");
    match super::xml::image_extension(&t.image) {
        Some(ext) if !has(raw) && !raw.trim_matches(|c: char| c == '.' || c.is_whitespace()).is_empty() => {
            esq_core::meta::base_name(&format!("{raw}.{ext}"))
        }
        Some(ext) if !has(&name) => format!("{name}.{ext}"),
        _ => name,
    }
}

fn texture(t: &doc::Texture) -> TextureData {
    TextureData {
        filename: texture_name(t),
        image: Blob(t.image.clone()),
        px: t.px,
        size: [t.size[0] * IN, t.size[1] * IN],
        avg: t.avg,
    }
}

/// Builds the model. Nested definitions are built recursively: this runs on a thread of its
/// own with a large stack (`super::on_big_stack`), whatever the caller's stack.
pub fn model(doc: &Doc) -> super::Result<Model> {
    super::on_big_stack(|| model_here(doc))
}

fn model_here(doc: &Doc) -> Model {
    built(doc).model
}

/// The builder once it has built the model (its counters tell the tests how much it did).
fn built(doc: &Doc) -> B<'_> {
    let mut b = B {
        doc,
        model: Model::new(),
        materials: HashMap::new(),
        layers: HashMap::new(),
        defs: HashMap::new(),
        def_by_id: doc.defs.iter().map(|d| (d.id, d)).collect(),
        mat_by_id: doc.materials.iter().map(|m| (m.id, m)).collect(),
        hatches: HashMap::new(),
        pending: Vec::new(),
        open: HashSet::new(),
        def_names: Names::default(),
        mat_names: Names::default(),
        image_tex: HashMap::new(),
        visited: 0,
        keys: HashMap::new(),
        pending_attach: Vec::new(),
        attaches: Vec::new(),
    };
    for d in b.model.defs.values() {
        b.def_names.add(&d.name);
    }
    // The names of the file's components are taken before any group is named: a component
    // may be called « Groupe#1 » (two definitions of one name shared a thumbnail).
    for d in doc.defs.iter().filter(|d| !d.is_group && !d.is_image) {
        b.def_names.add(&d.name);
    }
    for m in b.model.materials.values() {
        b.mat_names.add(&m.name);
    }
    let units = doc.options.get("UnitsOptions");
    let unit = units.and_then(|o| o.get("LengthUnit")).and_then(Value::as_i64);
    // The format decides for the formats of one unit (`SUModelGetUnits`): architectural and
    // fractional are in inches, engineering in feet, whatever unit the file names.
    let unit = match units.and_then(|o| o.get("LengthFormat")).and_then(Value::as_i64) {
        Some(1 | 3) => Some(0),
        Some(2) => Some(1),
        _ => unit,
    };
    b.model.units = match unit {
        Some(0) => Units::Inches,
        Some(1) => Units::Feet,
        Some(3) => Units::Centimeters,
        Some(4) => Units::Meters,
        Some(5) => Units::Yards,
        _ => Units::Millimeters,
    };
    b.shadow();
    b.layers();
    b.model_info();
    b.hatch_patterns();
    for m in &doc.materials {
        if !m.owned {
            b.material(m.id);
        }
    }
    let (root, attrs) = b.entities(&doc.root);
    b.model.root = root;
    b.store_attrs(None, attrs);
    b.unplaced();
    b.model.rebuild_topology();
    b.attach_annotations();
    b.scenes();
    split_nested(&mut b.model);
    radial_handles(&mut b.model.root, 0);
    for (k, d) in b.model.defs.iter_mut() {
        radial_handles(&mut d.entities, k.data().as_ffi());
    }
    b
}

/// A matched photo as Esquisse keeps it (`esq_core::photo::MatchPhoto`), from what SketchUp keeps
/// and the camera of its scene (SketchUp's calibrated one): the lines drawn on it (their ends in
/// units of the image's height, red, green, blue axes) become those of the setup, the scene's
/// camera that of the photo (vertical field of view). `None` without its image.
pub(crate) fn match_photo(
    m: &doc::MatchPhoto,
    cam: &esq_core::model::SceneCamera,
) -> Option<esq_core::photo::MatchPhoto> {
    use esq_core::glam::{DMat3, DVec2};
    use esq_core::matchphoto::{Line2, PhotoCamera, Scale, Setup};
    let image = m.image.clone()?;
    let (w, h) = (m.size[0].max(1) as f64, m.size[1].max(1) as f64);
    let px = |p: &[f64; 3]| DVec2::new(p[0] * h, p[1] * h);
    let mut lines = [None; 3];
    for (k, l) in lines.iter_mut().enumerate() {
        if let Some(q) = m.points.get(4 * k..4 * k + 4) {
            *l = Some([Line2 { a: px(&q[0]), b: px(&q[1]) }, Line2 { a: px(&q[2]), b: px(&q[3]) }]);
        }
    }
    let forward = (cam.target - cam.eye).normalize_or_zero();
    let right = forward.cross(cam.up).normalize_or_zero();
    let down = forward.cross(right);
    let rot = DMat3::from_cols(right, down, forward).transpose();
    let focal = h * 0.5 / (cam.fov_deg.to_radians() * 0.5).tan().max(1e-9);
    let name = m.path.rsplit(['/', '\\']).next().unwrap_or_default().to_string();
    Some(esq_core::photo::MatchPhoto {
        name,
        image: Blob(image),
        size: m.size,
        setup: Setup {
            size: DVec2::new(w, h),
            principal: None,
            lines,
            origin: DVec2::new(m.offset[0] * h, m.offset[1] * h),
            scale: Scale::Distance(cam.eye.length()),
            variant: None,
        },
        camera: PhotoCamera {
            rot,
            trans: -(rot * cam.eye),
            focal,
            principal: DVec2::new(w * 0.5, h * 0.5),
            size: DVec2::new(w, h),
        },
        grid: Default::default(),
    })
}

/// Where a definition's thumbnail as read is kept (`Model::files`): by the definition's name.
pub fn def_thumbnail_key(name: &str) -> String {
    format!("thumb:def:{name}")
}

/// Where `Model::files` keeps the `hatchpattern.xml` SketchUp wrote of a hatch pattern (by its
/// uid), and its thumbnail.
pub fn hatch_xml_key(uid: u32) -> String {
    format!("skp_hatch:{uid}")
}

pub fn hatch_thumbnail_key(uid: u32) -> String {
    format!("thumb:hatch:{uid}")
}

/// The hatch pattern settings of rendering options without a pattern, as SketchUp sets them
/// (`RenderingOptions#hatch_pattern = nil`): the section fill colour, scales of 1.
pub fn default_ro_hatch(fill: [u8; 4]) -> doc::HatchSettings {
    doc::HatchSettings { color: fill, scale: [1.0, 1.0], ..Default::default() }
}

/// A hatch pattern as Esquisse keeps it (`esq_core::meta::HatchPattern`) from SketchUp's
/// settings: an image pattern its image, a .pat pattern the text of its lines.
pub fn hatch_of(uid: u32, name: &str, s: &doc::HatchSettings) -> esq_core::meta::HatchPattern {
    let mut h = match (&s.image, &s.pat) {
        (Some(img), _) => esq_core::meta::HatchPattern::new(uid, name, &s.texture_name, img.to_vec()),
        (None, Some(f)) => esq_core::meta::HatchPattern::new(uid, name, &s.pat_name, pat_text_of(name, f).into_bytes()),
        (None, None) => esq_core::meta::HatchPattern::new(uid, name, "", Vec::new()),
    };
    h.image = s.image.is_some();
    h.color = [s.color[0], s.color[1], s.color[2]];
    h.size = [s.scale[0] as f64, s.scale[1] as f64];
    h.rotation = s.rotation as f64;
    h
}

/// The text of a .pat pattern from SketchUp's floats (per line: angle, origin, offset, the
/// number of dash/gap pairs, the pairs; a full line is one pair 1, 0), each number the shortest
/// that reads back as the same float.
pub fn pat_text_of(name: &str, f: &[f32]) -> String {
    let mut out = format!("*{name}\n");
    let mut i = 0;
    while i + 6 <= f.len() {
        let n = f[i + 5];
        let n = if n >= 0.0 && n.fract() == 0.0 && n <= 1000.0 { n as usize } else { 0 };
        let pairs = f.get(i + 6..(i + 6 + 2 * n).min(f.len())).unwrap_or_default();
        let mut parts: Vec<String> = f[i..i + 5].iter().map(|x| format!("{x}")).collect();
        if !(n == 1 && pairs.get(1) == Some(&0.0)) {
            for p in pairs.chunks(2) {
                parts.push(format!("{}", p[0]));
                if let Some(g) = p.get(1) {
                    parts.push(format!("{}", -g));
                }
            }
        }
        out.push_str(&parts.join(", "));
        out.push('\n');
        i += 6 + 2 * n;
    }
    out
}

/// Where a scene's thumbnail as read is kept (`Model::files`): by the scene's uid.
pub fn scene_thumbnail_key(uid: u64) -> String {
    format!("thumb:scene:{uid}")
}

/// Where a material's thumbnail as read is kept (`Model::files`).
pub fn mat_thumbnail_key(name: &str) -> String {
    format!("thumb:mat:{name}")
}

/// The arc a radial dimension measures, as the Ruby bridge names it (`c:<collection>:<curve>`,
/// `DimensionRadial#arc_curve`), once the collection has its id.
fn radial_handles(e: &mut Entities, ctx: u64) {
    for props in e.props.values_mut() {
        if let Some(AttrValue::Number(c)) = props.get("radial_curve") {
            let h = format!("c:{ctx}:{}", *c as u64);
            props.insert("radial".into(), AttrValue::Text(h));
        }
    }
}

/// The dictionaries nested in a dictionary (read as `parent\0child…`, `doc::NEST`) go where the
/// Ruby API finds them (`AttributeDictionary#attribute_dictionaries`): in the side table, under
/// the owner of their parent (`ad:<owner>|<parent>`), whoever owns the parent (the model, a
/// definition `d:0:<id>`, an instance `i:<ctx>:<id>`, an edge `h:e:…`, a material `mat:…`…).
fn split_nested(m: &mut Model) {
    type Out = Vec<(String, String, std::collections::BTreeMap<String, AttrValue>)>;
    fn take(owner: &str, attrs: &mut Attributes, out: &mut Out) {
        if !attrs.keys().any(|k| k.contains(doc::NEST)) {
            return;
        }
        let keys: Vec<String> = attrs.keys().filter(|k| k.contains(doc::NEST)).cloned().collect();
        for k in keys {
            let Some(v) = attrs.shift_remove(&k) else { continue };
            let mut parts: Vec<&str> = k.split(doc::NEST).collect();
            let Some(name) = parts.pop() else { continue };
            let mut o = owner.to_string();
            for p in parts {
                o = format!("ad:{o}|{p}");
            }
            out.push((o, name.to_string(), v));
        }
    }
    let mut out = Out::new();
    for (owner, attrs) in m.attr_store.iter_mut() {
        take(owner, attrs, &mut out);
    }
    for (k, i) in m.root.instances.iter_mut() {
        take(&format!("i:0:{}", k.data().as_ffi()), &mut i.attributes, &mut out);
    }
    for (id, d) in m.defs.iter_mut() {
        let ctx = id.data().as_ffi();
        take(&format!("d:0:{ctx}"), &mut d.attributes, &mut out);
        for (k, i) in d.entities.instances.iter_mut() {
            take(&format!("i:{ctx}:{}", k.data().as_ffi()), &mut i.attributes, &mut out);
        }
    }
    for (owner, name, v) in out {
        m.attr_store.entry(owner).or_default().entry(name).or_default().extend(v);
    }
}

impl B<'_> {
    /// Scenes, as `crate::read_model` reads them. A scene that does not keep its camera has
    /// the camera SketchUp makes by default (`CCamera`): 1" above the origin, looking down,
    /// y up, perspective, 30°.
    fn scenes(&mut self) {
        let doc = self.doc;
        let (styles, scene_styles) = match self.model.view_ext.get("styles") {
            // The names the model's list has (those made for a file without styles too).
            Some(v) => {
                let names: Vec<String> = v["list"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|e| e["name"].as_str().unwrap_or_default().to_string())
                    .collect();
                let (mut st, map) = super::style::of_doc(doc);
                for (s, n) in st.list.iter_mut().zip(names) {
                    s.name = n;
                }
                (st, map)
            }
            None => super::style::of_doc(doc),
        };
        let mut scene_attrs = Vec::new();
        for (i, s) in doc.scenes.iter().enumerate() {
            let cam = s.camera.unwrap_or(doc::Camera {
                eye: [0.0, 0.0, 1.0],
                target: [0.0; 3],
                up: [0.0, 1.0, 0.0],
                perspective: true,
                fov: 30.0,
            });
            let hidden_layers =
                s.hidden_layers.as_ref().map(|ids| ids.iter().filter_map(|l| self.layers.get(l).copied()).collect());
            let photo = s.photo.and_then(|id| doc.photos.iter().find(|m| m.id == id));
            let mut scene = esq_core::model::Scene {
                name: s.name.clone(),
                description: s.description.clone(),
                camera: camera(cam),
                hidden_layers,
                photo: photo.and_then(|m| match_photo(m, &camera(cam))),
                ext: Default::default(),
            };
            let p = &mut scene.ext.props;
            // What SketchUp keeps of the photo, to write it back as read (`write::plan`; the SDK
            // gives none of it: `sdk_quirks`).
            if let Some(m) = photo
                && scene.photo.is_some()
                && !doc.sdk_quirks
            {
                p.insert("skp_photo".into(), serde_json::to_value(m).unwrap_or_default());
            }
            // The environment it keeps (by name, `Page#environment`): the one of its settings,
            // else the current one; and its settings as read.
            if let Some(se) = s.env.filter(|_| !doc.sdk_quirks && s.flags & 0x200 != 0) {
                let same = |e: &&doc::Environment| {
                    e.skydome == se.skydome
                        && e.reflections == se.reflections
                        && (e.rotation - se.rotation as f64).abs() < 1e-3
                        && (e.skydome_exposure - se.skydome_exposure as f64).abs() < 1e-3
                        && (e.reflection_exposure - se.reflection_exposure as f64).abs() < 1e-3
                };
                let env = s
                    .env_id
                    .and_then(|id| doc.environments.iter().find(|e| e.id == id))
                    .or_else(|| doc.environments.iter().find(same))
                    .or_else(|| doc.environments.iter().find(|e| Some(e.id) == doc.current_env));
                if let Some(e) = env {
                    p.insert("env".into(), serde_json::json!(e.name));
                }
                p.insert("skp_env".into(), serde_json::to_value(se).unwrap_or_default());
            }
            p.insert("flags".into(), serde_json::json!(s.flags));
            if s.flags & 2 != 0
                && let Some(ro) = &s.rendering
            {
                p.insert("ro".into(), serde_json::json!(ro));
                // The hatch pattern settings of its rendering options, as read, when they are
                // not the model's (the writer gives it those).
                if let Some(h) = s.ro_hatch.as_ref().filter(|h| !doc.sdk_quirks && doc.ro_hatch.as_ref() != Some(*h)) {
                    p.insert("skp_ro_hatch".into(), serde_json::to_value(h).unwrap_or_default());
                }
            }
            if !s.animate {
                p.insert("animate".into(), serde_json::json!(false));
            }
            // Its own transition and delay times (`Page#transition_time`, `#delay_time`), and
            // what else SketchUp keeps of it (the SDK gives none of them: `sdk_quirks`).
            if !doc.sdk_quirks {
                if s.transition >= 0.0 {
                    p.insert("transition".into(), serde_json::json!(s.transition));
                }
                if s.delay >= 0.0 {
                    p.insert("delay".into(), serde_json::json!(s.delay));
                }
                if !s.v7153.is_empty() || !s.v7157 {
                    p.insert("skp_page".into(), serde_json::json!({"v7153": s.v7153, "v7157": s.v7157}));
                }
            }
            // The style the scene keeps, by name (as `Sketchup::Page#style` names it).
            if let Some(st) = scene_styles.get(i).copied().flatten().and_then(|k| styles.list.get(k)) {
                p.insert("style".into(), serde_json::json!(st.name));
            }
            if doc.active_scene == Some(i) {
                self.model.view_ext.insert(esq_core::model::ACTIVE_SCENE.into(), self.model.scenes.len().into());
            }
            self.model.scenes.push(scene);
            scene_attrs.push(s.attrs.clone());
        }
        self.model.ensure_scene_uids();
        // Their thumbnails as read, kept with the model (`scene_thumbnail_key`).
        for (s, d) in self.model.scenes.iter().zip(&doc.scenes) {
            if let Some(t) = &d.thumbnail {
                self.model.files.insert(scene_thumbnail_key(s.ext.uid), Blob(t.clone()));
            }
        }
        for (s, attrs) in self.model.scenes.iter().zip(scene_attrs) {
            if !attrs.is_empty() {
                self.model.attr_store.insert(format!("h:g:0:{}", s.ext.uid), attrs);
            }
        }
    }

    fn shadow(&mut self) {
        let sh = &self.doc.shadow;
        let s = &mut self.model.shadow;
        let flag = |k: &str, out: &mut bool| {
            if let Some(v) = sh.get(k).and_then(Value::as_bool) {
                *out = v;
            }
        };
        flag("DisplayShadows", &mut s.display);
        flag("UseSunForAllShading", &mut s.use_sun_for_shading);
        flag("DisplayOnAllFaces", &mut s.on_faces);
        flag("DisplayOnGroundPlane", &mut s.on_ground);
        flag("EdgesCastShadows", &mut s.from_edges);
        let real = |k: &str, out: &mut f64| {
            if let Some(v) = sh.get(k).and_then(Value::as_f64) {
                *out = v;
            }
        };
        real("TZOffset", &mut s.tz_offset);
        real("Latitude", &mut s.latitude);
        real("Longitude", &mut s.longitude);
        real("NorthAngle", &mut s.north_angle);
        if let Some(t) = sh.get("ShadowTime").and_then(Value::as_i64) {
            s.time = t;
        }
        if let Some(v) = sh.get("Light").and_then(Value::as_i64) {
            s.light = v as i32;
        }
        if let Some(v) = sh.get("Dark").and_then(Value::as_i64) {
            s.dark = v as i32;
        }
        if let Some(v) = sh.get("City").and_then(Value::as_str) {
            s.city = v.to_string();
        }
        if let Some(v) = sh.get("Country").and_then(Value::as_str) {
            s.country = v.to_string();
        }
    }

    /// Name, description, GUID, axes, Model Info options, rendering options, dictionaries.
    fn model_info(&mut self) {
        let doc = self.doc;
        let meta = &mut self.model.meta;
        meta.name = doc.name.clone();
        meta.description = doc.description.clone();
        meta.guid = doc.guid.clone();
        if let Some(a) = &doc.axes {
            self.model.axes = xf_to_mm(a);
        }
        for (name, values) in &doc.options {
            self.model.meta.options.entry(name.clone()).or_default().extend(values.clone());
        }
        if !doc.rendering.is_empty() {
            self.model.view_ext.insert("ro".into(), serde_json::json!(doc.rendering));
        }
        let ro: serde_json::Map<String, Value> = doc.rendering.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        super::fonts::to_view_ext(doc, &mut self.model);
        // The classification schemas, with their files (the SDK exposes their names only:
        // `sdk_quirks` leaves them out).
        if !doc.sdk_quirks {
            for s in &doc.schemas {
                let parsed = esq_core::classify::parse_skc(&s.data, &s.path);
                let schema = esq_core::classify::Schema {
                    name: s.name.clone(),
                    namespace: parsed.as_ref().map_or(String::new(), |p| p.namespace.clone()),
                    path: s.path.clone(),
                    types: parsed.map_or(Vec::new(), |p| p.types),
                };
                esq_core::classify::store_schema(&mut self.model, &schema);
                self.model.files.insert(esq_core::classify::file_key(&s.name), esq_core::photo::Blob(s.data.clone()));
            }
        }
        // The environments (`Sketchup::Environments`: `view_ext["envs"]`, `["env_current"]`,
        // the panorama in `Model::files["env:<id>"]`), what the SDK gives of them aside
        // (`sdk_quirks` leaves them out).
        if !doc.sdk_quirks && !doc.environments.is_empty() {
            let mut list = Vec::new();
            for (k, e) in doc.environments.iter().enumerate() {
                let id = k as i64 + 1;
                list.push(serde_json::json!({
                    "id": id, "name": e.name, "description": e.description, "path": "",
                    "skydome": e.skydome, "reflections": e.reflections, "rotation": e.rotation,
                    "skydome_exposure": e.skydome_exposure, "reflection_exposure": e.reflection_exposure,
                    "linked_sun": e.linked_sun, "sun_pos": [e.sun[0] * 360.0, e.sun[1] * 90.0],
                    "skp_folder": e.folder, "skp_file": e.image_name,
                }));
                if let Some(img) = &e.image {
                    self.model.files.insert(format!("env:{id}"), Blob(img.clone()));
                }
                if let Some(t) = &e.thumbnail {
                    self.model.files.insert(format!("thumb:env:{id}"), Blob(t.clone()));
                }
                if doc.current_env == Some(e.id) {
                    self.model.view_ext.insert("env_current".into(), serde_json::json!(id));
                }
            }
            self.model.view_ext.insert("envs".into(), Value::Array(list));
        }
        // A file without styles (SketchUp 3 to 5): those SketchUp makes when it opens it.
        let (st, _) = super::style::of_doc(doc);
        if let Some((styles, wm)) = super::style::to_view_ext(&st, &ro) {
            self.model.view_ext.insert("styles".into(), styles);
            if let Some(wm) = wm {
                self.model.view_ext.insert("watermarks".into(), wm);
            }
        }
        if !doc.attrs.is_empty() {
            self.model.attr_store.insert("model".into(), doc.attrs.clone());
        }
        // The model's values whose use is not known, when they are not a new model's.
        if let Some(f) = doc.model_flags.filter(|f| !doc.sdk_quirks && *f != doc::ModelFlags::default()) {
            self.model.view_ext.insert("skp_model".into(), serde_json::to_value(f).unwrap_or_default());
        }
        self.model.take_skp_comments();
    }

    fn layers(&mut self) {
        let doc = self.doc;
        for (i, l) in doc.layers.iter().enumerate() {
            let idx = if i == 0 {
                0
            } else {
                self.model.layers.push(Layer::new(l.name.clone()));
                self.model.layers.len() - 1
            };
            self.layers.insert(l.id, idx);
            let layer = &mut self.model.layers[idx];
            if idx != 0 {
                layer.visible = l.visible;
                layer.line_style = l.line_style.and_then(|s| doc.line_styles.get(&s).cloned()).unwrap_or_default();
            }
            layer.color = l.color;
            layer.page_behavior = l.page_behavior;
            if !l.attrs.is_empty() {
                let name = self.model.layers[idx].name.clone();
                self.model.attr_store.insert(format!("layer:{name}"), l.attrs.clone());
            }
        }
        self.model.ensure_layer_uids();
        // Folders, depth first.
        let mut stack: Vec<(&doc::Folder, Option<u32>)> = doc.folders.iter().rev().map(|f| (f, None)).collect();
        while let Some((f, parent)) = stack.pop() {
            let uid = self.model.new_uid();
            self.model.meta.folders.push(esq_core::meta::LayerFolder {
                uid,
                name: f.name.clone(),
                parent,
                visible: f.visible,
                visible_on_new_pages: f.visible_on_new_pages,
            });
            for l in &f.layers {
                if let Some(&i) = self.layers.get(l) {
                    self.model.layers[i].folder = Some(uid);
                }
            }
            stack.extend(f.folders.iter().rev().map(|s| (s, Some(uid))));
        }
        if let Some(&i) = doc.active_layer.and_then(|a| self.layers.get(&a)) {
            self.model.meta.active_layer = if i == 0 { 0 } else { self.model.layers[i].uid };
        }
    }

    /// The hatch patterns (`Sketchup::HatchPatterns`: `ModelMeta::hatch_patterns`), what SketchUp
    /// wrote of each in `Model::files` (`hatch_xml_key`: its `hatchpattern.xml`, to write its
    /// numbers back as read; `hatch_thumbnail_key`); the settings of the model's rendering
    /// options as read (`view_ext["skp_ro_hatch"]`) and the pattern they are, by name
    /// (`["ro_hatch"]`, `RenderingOptions#hatch_pattern`). The SDK gives none (`sdk_quirks`).
    fn hatch_patterns(&mut self) {
        let doc = self.doc;
        if doc.sdk_quirks {
            return;
        }
        for h in &doc.hatch_patterns {
            let uid = self.model.new_uid();
            self.model.meta.hatch_patterns.push(hatch_of(uid, &h.name, &h.settings));
            if let Some(x) = &h.xml {
                self.model.files.insert(hatch_xml_key(uid), Blob(x.clone()));
            }
            if let Some(t) = &h.thumbnail {
                self.model.files.insert(hatch_thumbnail_key(uid), Blob(t.clone()));
            }
            self.hatches.insert(h.id, (uid, h.name.clone()));
        }
        if let Some(s) = &doc.ro_hatch {
            let fill =
                doc.rendering.get("SectionDefaultFillColor").and_then(|c| serde_json::from_value(c.clone()).ok());
            if let Some(h) = doc.hatch_patterns.iter().find(|h| h.settings == *s) {
                self.model.view_ext.insert("ro_hatch".into(), serde_json::json!(h.name));
            } else if *s != default_ro_hatch(fill.unwrap_or([0, 0, 0, 255])) {
                self.model.view_ext.insert("skp_ro_hatch".into(), serde_json::to_value(s).unwrap_or_default());
            }
        }
    }

    fn material(&mut self, id: Id) -> Option<MaterialId> {
        if let Some(&m) = self.materials.get(&id) {
            return Some(m);
        }
        let m = *self.mat_by_id.get(&id)?;
        let mut ext = MaterialExt { texture: m.texture.as_ref().map(texture), ..Default::default() };
        ext.hatch = m.hatch.and_then(|h| self.hatches.get(&h)).map(|h| h.0);
        if m.kind == 2 {
            ext.colorize_type = m.colorize_type.clamp(0, 1) as u8;
            // SketchUp keeps the colour, and gives the offsets from the image's average colour
            // to it; the texture's average is then the colour.
            if let Some(t) = ext.texture.as_mut() {
                ext.colorize_deltas = colorize_deltas(t.avg, m.color, m.colorize_type == 0);
                t.avg = m.color;
            }
        }
        ext.workflow = m.workflow.clamp(0, 1) as u8;
        let p = &m.pbr;
        let e = &mut ext.pbr;
        e.metalness_enabled = p.metalness_enabled;
        e.metallic_factor = p.metallic_factor;
        e.roughness_enabled = p.roughness_enabled;
        e.roughness_factor = p.roughness_factor;
        e.roughness_inverted = p.roughness_inverted;
        e.normal_enabled = p.normal_enabled;
        e.normal_scale = p.normal_scale;
        e.normal_style = p.normal_style.clamp(0, 1) as u8;
        e.ao_enabled = p.ao_enabled;
        e.ao_strength = p.ao_strength;
        e.metallic_texture = p.metallic_texture.as_ref().map(texture);
        e.roughness_texture = p.roughness_texture.as_ref().map(texture);
        e.normal_texture = p.normal_texture.as_ref().map(texture);
        e.ao_texture = p.ao_texture.as_ref().map(texture);
        let opacity = if m.use_opacity { m.opacity as f32 } else { 1.0 };
        let mid = self.model.add_material(&m.name, m.color);
        self.mat_names.add(&m.name);
        let mm = &mut self.model.materials[mid];
        (mm.opacity, mm.ext) = (opacity, ext);
        if !m.attrs.is_empty() {
            self.model.attr_store.insert(format!("mat:{}", m.name), m.attrs.clone());
        }
        if let Some(t) = &m.thumbnail {
            let key = mat_thumbnail_key(&self.model.materials[mid].name);
            self.model.files.insert(key, esq_core::photo::Blob(t.clone()));
        }
        self.materials.insert(id, mid);
        Some(mid)
    }

    /// A component's definition, built where it is first placed (or, placed nowhere, after the
    /// model: `unplaced`).
    fn component_def(&mut self, sd: &doc::Def) -> DefId {
        self.open.insert(sd.id);
        let (sub, attrs) = self.entities(&sd.ents);
        self.open.remove(&sd.id);
        self.def_names.add(&sd.name);
        let b = &sd.behavior;
        let behavior = esq_core::model::Behavior {
            always_face_camera: b.always_face_camera,
            cuts_opening: b.cuts_opening,
            is2d: b.snap != 0,
            shadows_face_sun: b.shadows_face_sun,
            no_scale_mask: b.no_scale_mask,
            snapto: (b.snap - 1).clamp(0, 3),
        };
        let d = self.model.defs.insert(Definition {
            name: sd.name.clone(),
            is_group: false,
            description: sd.description.clone(),
            attributes: sd.attrs.clone(),
            ext: esq_core::model::DefExt { behavior, ..Default::default() },
            entities: sub,
        });
        self.store_attrs(Some(d), attrs);
        self.thumbnail(d, sd);
        self.provenance(d, sd);
        self.defs.insert(sd.id, d);
        d
    }

    /// The name and description of a group's definition as the file has them (« Group#3 »:
    /// SketchUp keeps them when it saves the file again; Esquisse names group definitions its
    /// own way): `view_ext["skp_group_names"]`, by GUID.
    fn group_name(&mut self, sd: &doc::Def) {
        if self.doc.sdk_quirks || sd.guid.is_empty() {
            return;
        }
        let map = self.model.view_ext.entry("skp_group_names".to_string()).or_insert_with(|| serde_json::json!({}));
        if let Some(o) = map.as_object_mut() {
            o.insert(sd.guid.clone(), serde_json::json!([sd.name, sd.description]));
        }
    }

    /// Where a definition comes from, as SketchUp keeps it when it saves the file again: its
    /// GUID, the file it was loaded from and when (the SDK reader gives none: `sdk_quirks`).
    fn provenance(&mut self, d: DefId, sd: &doc::Def) {
        if self.doc.sdk_quirks {
            return;
        }
        let e = &mut self.model.defs[d].ext;
        e.guid = sd.guid.clone();
        e.path = sd.path.clone();
        e.loaded_at = sd.time as f64;
    }

    /// The components the file has but places nowhere (Window › Components « In Model » lists
    /// them until they are purged): kept too. The SDK reader does not list them (`sdk_quirks`).
    fn unplaced(&mut self) {
        if self.doc.sdk_quirks {
            return;
        }
        let doc = self.doc;
        for sd in doc.defs.iter().filter(|d| !d.is_group && !d.is_image) {
            if !self.defs.contains_key(&sd.id) && !self.open.contains(&sd.id) {
                self.component_def(sd);
            }
        }
    }

    /// A definition's thumbnail: its camera (`ComponentDefinition#thumbnail_camera`) and its
    /// image, kept with the model (`def_thumbnail_key`).
    fn thumbnail(&mut self, d: DefId, sd: &doc::Def) {
        let Some(t) = &sd.thumbnail else { return };
        if let Some(c) = &t.camera {
            self.model.defs[d].ext.thumbnail_camera = Some([v3(c.eye) * IN, v3(c.target) * IN, v3(c.up)]);
        }
        if let Some(img) = &t.image {
            let key = def_thumbnail_key(&self.model.defs[d].name);
            self.model.files.insert(key, esq_core::photo::Blob(img.clone()));
        }
    }

    fn layer(&self, el: &doc::El) -> LayerIdx {
        el.layer.and_then(|l| self.layers.get(&l).copied()).unwrap_or(0)
    }

    fn el_material(&mut self, el: &doc::El) -> Option<MaterialId> {
        self.material(el.material?)
    }

    fn shadow_flags(&self, out: &mut Entities, kind: char, ffi: u64, el: &doc::El) {
        let key = format!("{kind}{ffi}");
        if el.flags & flags::CASTS_SHADOWS == 0 {
            out.set_prop(&key, "casts", AttrValue::Bool(false));
        }
        if el.flags & flags::RECEIVES_SHADOWS == 0 {
            out.set_prop(&key, "receives", AttrValue::Bool(false));
        }
    }

    fn entity_extras(&mut self, out: &mut Entities, kind: char, ffi: u64, el: &doc::El) {
        if !el.attrs.is_empty() {
            self.pending.push((kind, ffi, el.attrs.clone()));
        }
        // A SketchUp 2021+ file numbers its entities with their persistent ids (§2.1); older
        // files have none (SketchUp gives new ones when it opens them).
        if self.doc.version.0 >= 21 && el.id != 0 {
            out.pids.insert(format!("{kind}{ffi}"), el.id);
        }
        self.shadow_flags(out, kind, ffi, el);
        if kind == 'e'
            && let Some(m) = self.el_material(el)
        {
            out.set_prop(&format!("{kind}{ffi}"), "material", AttrValue::Text(self.model.materials[m].name.clone()));
        }
    }

    /// The font of a text or a dimension, and its colour (a material SketchUp names « * »).
    fn annot_extras(&mut self, out: &mut Entities, key: &str, el: &doc::El, font: Option<Id>) {
        if let Some(f) = font.and_then(|i| self.doc.fonts.iter().find(|f| f.id == i && i != 0)) {
            for (k, v) in super::fonts::props(f) {
                out.set_prop(key, k, v);
            }
        }
        if let Some(m) = self.el_material(el) {
            let mt = &self.model.materials[m];
            out.set_prop(key, "material", AttrValue::Text(mt.name.clone()));
            out.set_prop(key, "color", AttrValue::Json(serde_json::json!(mt.color)));
        }
    }

    fn curve_props(&self, out: &mut Entities, c: &doc::Curve, id: u32) {
        let key = format!("c{id}");
        let polygon = c.kind != 0;
        // What the SDK does not give (`sdk_quirks` leaves it out): the arc exactly as read, its
        // radius and its axes with their lengths (an elliptic arc, scaled), and, once its
        // edges are known (`curve_fingerprint`), the points it was read with. The writer
        // writes it as read while its points are those.
        let exact = |out: &mut Entities, a: &doc::Arc| {
            if self.doc.sdk_quirks {
                return;
            }
            let y = v3(a.yaxis).normalize_or_zero();
            for (k, v) in [
                ("r", a.radius * IN),
                ("rx", v3(a.xaxis).length() * IN),
                ("ry", v3(a.yaxis).length() * IN),
                ("yx", y.x),
                ("yy", y.y),
                ("yz", y.z),
            ] {
                out.set_prop(&key, k, AttrValue::Number(v));
            }
        };
        match &c.arc {
            Some(a) if polygon => {
                out.set_prop(&key, "kind", AttrValue::Text("polygon".into()));
                if !self.doc.sdk_quirks {
                    arc_props(out, &key, a);
                    exact(out, a);
                }
            }
            None => out.set_prop(&key, "kind", AttrValue::Text("curve".into())),
            Some(a) => {
                arc_props(out, &key, a);
                exact(out, a);
            }
        }
    }

    /// What a collection's entities left to do once the collection has its place (`ctx`): their
    /// attribute dictionaries, and the attaches of its annotations (`attach_annotations`).
    fn store_attrs(&mut self, ctx: Option<DefId>, (list, attaches): Pending) {
        for (kind, ffi, attrs) in list {
            self.model.attr_store.insert(crate::attr_owner(kind, ctx, ffi), attrs);
        }
        self.attaches.extend(attaches.into_iter().map(|a| (ctx, a)));
    }

    fn entities(&mut self, e: &Ents) -> (Entities, Pending) {
        self.visited += 1;
        let outer = std::mem::take(&mut self.pending);
        let outer_attach = std::mem::take(&mut self.pending_attach);
        let out = self.entities_inner(e);
        let mine = std::mem::replace(&mut self.pending, outer);
        let attach = std::mem::replace(&mut self.pending_attach, outer_attach);
        (out, (mine, attach))
    }

    /// The attaches of the annotations read (`doc::Connection`) as Esquisse keeps them
    /// (`esq_core::attach`), once every collection is built: the path's instances and the
    /// entity by the keys they got, the parameter of the point worked out from where the reader
    /// placed it (`attach::position`). What Esquisse does not keep (an intersection, an image,
    /// a point not attached) stays free.
    fn attach_annotations(&mut self) {
        use esq_core::attach::{self, Attach, AttachTo};
        for (ctx, (id, which, c, p)) in std::mem::take(&mut self.attaches) {
            let path: Option<Vec<esq_core::model::InstanceId>> = c
                .path
                .iter()
                .map(|i| match self.keys.get(i) {
                    Some(Made::Instance(x)) => Some(*x),
                    _ => None,
                })
                .collect();
            let Some(path) = path else { continue };
            let leaf = c.leaf.and_then(|l| self.keys.get(&l)).copied();
            let Some((_, ents, m)) = self.model.attach_leaf(ctx, &Attach::new(path.clone(), AttachTo::Origin)) else {
                continue;
            };
            let q = m.inverse().transform_point3(p);
            let to = match (c.kind, leaf) {
                (2, Some(Made::Vertex(v))) => Some(AttachTo::Vertex(v)),
                (3, Some(Made::Guide(g))) => Some(AttachTo::GuidePoint(g)),
                (4, Some(Made::Edge(e))) => Some(AttachTo::EdgeMiddle(e)),
                (5, Some(Made::Edge(e))) => attach::along_edge(ents, e, q),
                (6, Some(Made::Guide(g))) => attach::on_guide(ents, g, q),
                (7, Some(Made::Face(f))) => attach::on_face(ents, f, q),
                (10, Some(Made::Curve(e))) => Some(AttachTo::ArcCenter(e)),
                (ON_ARC, Some(Made::Curve(e))) => attach::on_arc(ents, e, q),
                (11, Some(Made::Section(s))) => Some(AttachTo::Section(s, q)),
                // The origin of the path's instances (without a path, the point of the file:
                // a free point).
                (12 | 13, _) if !path.is_empty() => Some(AttachTo::Origin),
                _ => None,
            };
            if let Some(to) = to {
                self.model.set_attach(ctx, id, which, Some(Attach::new(path, to)));
            }
        }
    }

    fn entities_inner(&mut self, e: &Ents) -> Entities {
        let mut out = Entities::default();
        let pos: HashMap<Id, [f64; 3]> = e.verts.iter().map(|v| (v.id, v.pos)).collect();
        let edges_by_id: HashMap<Id, &doc::Edge> = e.edges.iter().map(|x| (x.el.id, x)).collect();
        let mut verts: HashMap<Id, VertexId> = HashMap::new();
        let mut vertex = |out: &mut Entities, v: Id| -> VertexId {
            *verts.entry(v).or_insert_with(|| out.add_vertex_raw(v3(pos.get(&v).copied().unwrap_or_default()) * IN))
        };
        let curves_by_id: HashMap<Id, &doc::Curve> = e.curves.iter().map(|c| (c.id, c)).collect();
        let mut curves: HashMap<Id, u32> = HashMap::new();
        // What relationships refer to: faces and instances, by the file's ids.
        let mut face_ids: HashMap<Id, esq_core::model::FaceId> = HashMap::new();
        let mut inst_ids: HashMap<Id, esq_core::model::InstanceId> = HashMap::new();

        for edge in &e.edges {
            let (vs, vt) = (vertex(&mut out, edge.start), vertex(&mut out, edge.end));
            let Some(id) = out.add_edge_raw(vs, vt) else { continue };
            self.keys.insert(edge.el.id, Made::Edge(id));
            let layer = self.layer(&edge.el);
            let x = &mut out.edges[id];
            x.soft = edge.el.flags & flags::SOFT != 0;
            x.smooth = edge.el.flags & flags::SMOOTH != 0;
            x.hidden = edge.el.hidden();
            x.layer = layer;
            if let Some(c) = edge.curve.and_then(|c| curves_by_id.get(&c)) {
                let cid = match curves.get(&c.id) {
                    Some(&x) => x,
                    None => {
                        let x = out.new_curve_id();
                        curves.insert(c.id, x);
                        self.curve_props(&mut out, c, x);
                        x
                    }
                };
                out.edges[id].curve = Some(cid);
                self.keys.entry(c.id).or_insert(Made::Curve(id));
            }
            self.entity_extras(&mut out, 'e', id.data().as_ffi(), &edge.el);
        }
        // The points each arc was read with (one pass over the edges).
        if !self.doc.sdk_quirks && !curves.is_empty() {
            let mut by_curve: HashMap<u32, Vec<esq_core::model::EdgeId>> = HashMap::new();
            for (k, x) in out.edges.iter() {
                if let Some(c) = x.curve {
                    by_curve.entry(c).or_default().push(k);
                }
            }
            for (c, list) in by_curve {
                let key = format!("c{c}");
                if out.prop(&key, "r").is_some() {
                    let fp = curve_fingerprint(&out, &list);
                    out.set_prop(&key, "fp", AttrValue::Text(fp));
                }
            }
        }

        for f in &e.faces {
            let mut loops = Vec::new();
            for lp in &f.loops {
                let mut ids = Vec::new();
                for &(edge, rev) in lp {
                    let Some(x) = edges_by_id.get(&edge) else { continue };
                    let v = if rev { x.end } else { x.start };
                    ids.push(vertex(&mut out, v));
                }
                loops.push(ids);
            }
            if loops.is_empty() {
                continue;
            }
            let outer_ids = loops.remove(0);
            let Some(id) = out.add_face_raw(outer_ids, loops) else { continue };
            face_ids.insert(f.el.id, id);
            self.keys.insert(f.el.id, Made::Face(id));
            let front = f.el.material.and_then(|m| self.material(m));
            let back = f.back.and_then(|m| self.material(m));
            let layer = self.layer(&f.el);
            let face = &mut out.faces[id];
            let normal = DVec3::new(f.plane[0], f.plane[1], f.plane[2]);
            if normal.length() > 0.5 {
                face.normal = normal.normalize();
            }
            face.front = front;
            face.back = back;
            face.layer = layer;
            face.hidden = f.el.hidden();
            // Texture placement of the painted sides that were positioned or projected.
            let mut tex = esq_core::meta::FaceTex::default();
            let mut any = false;
            for (side, mat) in [f.el.material, f.back].into_iter().enumerate() {
                let (Some(tc), Some(m)) = (&f.el.tex[side], mat) else { continue };
                if !(tc.positioned || tc.projected) {
                    continue;
                }
                // A colour without a texture: the UV helper ignores the placement and maps the
                // plane's frame, one inch per tile.
                let texture = self.mat_by_id.get(&m).and_then(|m| m.texture.as_ref());
                let identity = doc::TexCoords { matrix: IDENTITY3, ..Default::default() };
                let (size, tc_used) = match texture {
                    Some(t) => (t.size, &**tc),
                    None => ([1.0, 1.0], &identity),
                };
                let Some(map) = uv_map(f.plane, tc_used, size.map(|x| x * IN)) else { continue };
                if side == 0 {
                    tex.front = Some(map);
                } else {
                    tex.back = Some(map);
                }
                if tc.projected {
                    tex.projection[side] = tc.projection.map(v3).filter(|d| d.length() > 1e-12).map(|d| d.normalize());
                }
                any = true;
            }
            if any {
                out.faces[id].tex = Some(Box::new(tex));
            }
            self.entity_extras(&mut out, 'f', id.data().as_ffi(), &f.el);
        }

        for (k, v) in &verts {
            self.keys.insert(*k, Made::Vertex(*v));
        }
        for gl in &e.guide_lines {
            let pos = v3(gl.start) * IN;
            let mut g = if gl.infinite {
                esq_core::model::Guide::line(pos, v3(gl.dir))
            } else {
                esq_core::model::Guide::segment(pos, pos + v3(gl.dir) * IN)
            };
            if g.dir.is_nan() || g.dir == DVec3::ZERO {
                continue;
            }
            (g.layer, g.hidden) = (self.layer(&gl.el), gl.el.hidden());
            let id = out.guides.insert(g);
            self.keys.insert(gl.el.id, Made::Guide(id));
        }
        for gp in &e.guide_points {
            let mut g = esq_core::model::Guide::point(v3(gp.pos) * IN);
            (g.layer, g.hidden) = (self.layer(&gp.el), gp.el.hidden());
            let id = out.guides.insert(g);
            self.keys.insert(gp.el.id, Made::Guide(id));
        }
        // The SDK lists no snap nor space of a file (`SUEntitiesEntityListFill` has no case for
        // their types, `docs/skp-format.md` §8.4): `sdk_quirks` leaves them out as well.
        let (snaps, spaces) = if self.doc.sdk_quirks { (&[][..], &[][..]) } else { (&e.snaps[..], &e.spaces[..]) };
        for s in snaps {
            out.snaps.push(esq_core::model::SnapPoint::new(v3(s.pos) * IN, v3(s.dir), v3(s.up)));
        }
        for s in spaces {
            let [a, b, c, d] = s.ceiling;
            let points = |l: &[[f64; 3]]| -> Vec<DVec3> { l.iter().map(|p| v3(*p) * IN).collect() };
            out.spaces.push(esq_core::model::Space {
                name: s.name.clone(),
                orientation: v3(s.orientation) * IN,
                ceiling: [a, b, c, d * IN],
                perimeter: points(&s.perimeter),
                holes: s.holes.iter().map(|h| points(h)).collect(),
                layer: self.layer(&s.el),
                hidden: s.el.hidden(),
                ..Default::default()
            });
        }

        for s in &e.sections {
            let n = DVec3::new(s.plane[0], s.plane[1], s.plane[2]);
            let len = n.length();
            if len < 1e-12 {
                continue;
            }
            let mut sec = esq_core::model::SectionPlane::new(-n * (s.plane[3] / (len * len)) * IN, n);
            sec.name = s.name.clone();
            sec.symbol = s.symbol.clone();
            (sec.layer, sec.hidden) = (self.layer(&s.el), s.el.hidden());
            let id = out.sections.insert(sec);
            self.keys.insert(s.el.id, Made::Section(id));
            if e.active_section == Some(s.el.id) {
                out.active_section = Some(id);
            }
            // Its hatch pattern, by name (`SectionPlane#hatch_pattern`).
            if let Some((_, n)) = s.hatch.and_then(|h| self.hatches.get(&h)) {
                out.set_prop(&format!("s{}", id.data().as_ffi()), "hatch", AttrValue::Text(n.clone()));
            }
        }

        for t in &e.texts {
            use esq_core::model::{AnnotKind, Annotation};
            let kind = match t.point {
                Some(p) if t.leader_type != 0 => AnnotKind::Text { point: v3(p) * IN, leader: v3(t.leader) * IN },
                _ => AnnotKind::ScreenText { x: t.screen[0], y: t.screen[1] },
            };
            let id = out.annots.insert(Annotation {
                kind,
                text: t.text.clone(),
                layer: self.layer(&t.el),
                hidden: t.el.hidden(),
                attach: Default::default(),
            });
            let key = format!("a{}", id.data().as_ffi());
            if t.leader_type == 1 {
                out.set_prop(&key, "leader_type", AttrValue::Number(1.0));
            }
            if t.arrow_type != 0 {
                out.set_prop(&key, "arrow_type", AttrValue::Number(t.arrow_type as f64));
            }
            self.annot_extras(&mut out, &key, &t.el, t.font);
            if let (Some(c), AnnotKind::Text { point, .. }) = (&t.connection, &out.annots[id].kind) {
                self.pending_attach.push((id, 0, c.clone(), *point));
            }
        }
        for d in &e.dims {
            use esq_core::model::{AnnotKind, Annotation};
            let side = v3(d.normal).cross(v3(d.xaxis)).normalize_or_zero();
            let mut text = d.text.clone();
            if text.starts_with('<') && text.ends_with('>') {
                text.clear();
            }
            let id = out.annots.insert(Annotation {
                kind: AnnotKind::Linear { a: v3(d.start) * IN, b: v3(d.end) * IN, offset: side * d.position[1] * IN },
                text,
                layer: self.layer(&d.el),
                hidden: d.el.hidden(),
                attach: Default::default(),
            });
            let key = format!("a{}", id.data().as_ffi());
            if d.arrow_type != 3 {
                out.set_prop(&key, "arrow_type", AttrValue::Number(d.arrow_type as f64));
            }
            // The SDK gives the style's font for a dimension without one of its own
            // (in SketchUpAPI.dll): it cannot tell an own font that is the style's.
            let style_font = self.doc.dim_style.as_ref().and_then(|s| s.font);
            let font = d.font.filter(|&f| !(self.doc.sdk_quirks && Some(f) == style_font));
            self.annot_extras(&mut out, &key, &d.el, font);
            for (w, (c, p)) in d.connections.iter().zip([d.start, d.end]).enumerate() {
                if let Some(c) = c {
                    self.pending_attach.push((id, w, c.clone(), v3(p) * IN));
                }
            }
        }
        // Radial dimensions (the SDK reader does not read them: `sdk_quirks` leaves them out),
        // as the Ruby bridge keeps them (`Entities#add_dimension_radial`): from the centre to
        // their point on the arc, `radial` the arc (`radial_handles`), and what SketchUp makes
        // its leader of (`skp_angle`, `skp_leader`).
        for d in e.radials.iter().filter(|_| !self.doc.sdk_quirks) {
            use esq_core::model::{AnnotKind, Annotation};
            let curve = d.edge.and_then(|x| edges_by_id.get(&x)).and_then(|x| x.curve);
            let arc = match curve.and_then(|c| curves_by_id.get(&c)).and_then(|c| c.arc.as_ref()) {
                Some(a) => a,
                None => match &d.arc {
                    Some(a) => a,
                    None => continue,
                },
            };
            let at = v3(arc.center) + v3(arc.xaxis) * d.angle.cos() + v3(arc.yaxis) * d.angle.sin();
            let mut text = d.text.clone();
            if text.starts_with('<') && text.ends_with('>') {
                text.clear();
            }
            let id = out.annots.insert(Annotation {
                kind: AnnotKind::Linear { a: v3(arc.center) * IN, b: at * IN, offset: DVec3::ZERO },
                text,
                layer: self.layer(&d.el),
                hidden: d.el.hidden(),
                attach: Default::default(),
            });
            let key = format!("a{}", id.data().as_ffi());
            match curve.and_then(|c| curves.get(&c)) {
                Some(&cid) => out.set_prop(&key, "radial_curve", AttrValue::Number(cid as f64)),
                // Its own arc (the one it measured is gone).
                None => {
                    let v: Vec<f64> = [arc.center, arc.normal, arc.xaxis, arc.yaxis]
                        .iter()
                        .flatten()
                        .copied()
                        .chain([arc.radius, arc.start, arc.end])
                        .collect();
                    out.set_prop(&key, "radial", AttrValue::Text(String::new()));
                    out.set_prop(&key, "skp_arc", AttrValue::Json(serde_json::json!(v)));
                }
            }
            if d.diameter {
                out.set_prop(&key, "diameter", AttrValue::Bool(true));
            }
            out.set_prop(&key, "arrow_type", AttrValue::Number(d.arrow_type as f64));
            out.set_prop(&key, "skp_angle", AttrValue::Number(d.angle));
            out.set_prop(&key, "skp_leader", AttrValue::Number(d.leader * IN));
            self.annot_extras(&mut out, &key, &d.el, d.font);
            // Attached to its arc: its centre, and its point at the angle it has (it follows the arc).
            if let Some(c) = curve.filter(|c| curves.contains_key(c)) {
                let at_arc = |kind: i32| doc::Connection { kind, point: [0.0; 3], leaf: Some(c), path: Vec::new() };
                self.pending_attach.push((id, 0, at_arc(10), v3(arc.center) * IN));
                self.pending_attach.push((id, 1, at_arc(ON_ARC), at * IN));
            }
        }
        // 3D polylines (the SDK reader does not read them; nothing compares them).
        for pl in &e.polylines {
            out.polylines.push(esq_core::model::Polyline3d {
                points: pl.points.iter().map(|p| v3(*p) * IN).collect(),
                layer: self.layer(&pl.el),
                hidden: pl.el.hidden(),
            });
        }

        for g in &e.groups {
            if self.open.contains(&g.def) || self.open.len() >= MAX_NESTING {
                continue;
            }
            let def = match self.defs.get(&g.def) {
                Some(&d) => d,
                None => {
                    let ents = self.def_by_id.get(&g.def).map(|d| &d.ents);
                    self.open.insert(g.def);
                    let (sub, attrs) = match ents {
                        Some(x) => self.entities(x),
                        None => (Entities::default(), Default::default()),
                    };
                    self.open.remove(&g.def);
                    let def_name = self.def_names.hashed("Groupe");
                    let attributes = self.def_by_id.get(&g.def).map_or_else(Attributes::new, |d| d.attrs.clone());
                    let d = self.model.defs.insert(Definition {
                        name: def_name,
                        is_group: true,
                        description: String::new(),
                        attributes,
                        ext: Default::default(),
                        entities: sub,
                    });
                    self.store_attrs(Some(d), attrs);
                    if let Some(sd) = self.def_by_id.get(&g.def).copied() {
                        self.thumbnail(d, sd);
                        self.provenance(d, sd);
                        self.group_name(sd);
                    }
                    self.defs.insert(g.def, d);
                    d
                }
            };
            inst_ids.insert(g.el.id, self.place(&mut out, g, def));
        }

        for i in &e.instances {
            if self.open.contains(&i.def) || self.open.len() >= MAX_NESTING {
                continue;
            }
            let def = match self.defs.get(&i.def) {
                Some(&d) => d,
                None => {
                    let Some(sd) = self.def_by_id.get(&i.def).copied() else { continue };
                    self.component_def(sd)
                }
            };
            inst_ids.insert(i.el.id, self.place(&mut out, i, def));
        }
        for img in &e.images {
            self.image(&mut out, img);
        }
        // What each component is glued to (`ComponentInstance#glued_to`: a face or an instance
        // of the same collection; the SDK gives none).
        if !self.doc.sdk_quirks {
            for [to, what] in &e.glues {
                let Some(&i) = inst_ids.get(what) else { continue };
                let target = match (face_ids.get(to), inst_ids.get(to)) {
                    (Some(&f), _) => esq_core::model::GlueRef::Face(f),
                    (None, Some(&t)) if t != i => esq_core::model::GlueRef::Instance(t),
                    _ => continue,
                };
                out.instances[i].ext.glued_to = Some(target);
            }
        }
        for (k, v) in &inst_ids {
            self.keys.insert(*k, Made::Instance(*v));
        }
        out.rebuild_topology();
        out
    }

    /// An image (`images::read_images`): the texture of its definition's face, as a new image
    /// definition of Esquisse.
    fn image(&mut self, out: &mut Entities, img: &doc::Image) {
        let tex = match self.image_tex.get(&img.inst.def) {
            Some(&t) => t,
            None => {
                let t = self.def_by_id.get(&img.inst.def).and_then(|def| {
                    def.ents
                        .faces
                        .iter()
                        .filter_map(|f| f.el.material)
                        .filter_map(|m| self.mat_by_id.get(&m))
                        .find_map(|m| m.texture.as_ref())
                });
                self.image_tex.insert(img.inst.def, t);
                t
            }
        };
        let Some(tex) = tex else { return };
        let file = esq_core::meta::base_name(&tex.file_name);
        let mut t = texture(tex);
        // SketchUp's size of an image in pixels is the size of one tile of its texture, in
        // inches, truncated (`SUImageGetPixelDimensions`), not the size of the picture: they
        // differ when the picture was replaced (a SketchUp 3 BMP of 384 pixels in a tile of
        // 386 inches, a PNG of 128 × 256 in 122 × 166).
        let [sw, sh] = tex.size;
        t.px = if sw.is_finite() && sh.is_finite() && (1.0..1e9).contains(&sw) && (1.0..1e9).contains(&sh) {
            [sw as u32, sh as u32]
        } else {
            tex.px
        };
        if !file.is_empty() {
            t.filename = file.clone();
        }
        let stem = std::path::Path::new(&file).file_stem().and_then(|s| s.to_str()).unwrap_or("Image").to_string();
        let Some(d) = self.image_def(&stem, &file, t) else { return };
        let i = &img.inst;
        let id = out.instances.insert(Instance {
            def: d,
            xform: xf_to_mm(&i.xform),
            name: i.name.clone(),
            material: None,
            layer: self.layer(&i.el),
            hidden: i.el.hidden(),
            locked: false,
            attributes: i.el.attrs.clone(),
            ext: esq_core::model::InstanceExt {
                hatch: i.hatch.and_then(|h| self.hatches.get(&h)).map(|h| h.1.clone()),
                guid: if self.doc.sdk_quirks { String::new() } else { i.guid.clone() },
                ..Default::default()
            },
        });
        self.shadow_flags(out, 'i', id.data().as_ffi(), &i.el);
    }

    /// `Model::add_image_def` (the same definition and material), its names made unique by
    /// `def_names` and `mat_names` instead of a scan of the model per image.
    fn image_def(&mut self, name: &str, path: &str, mut tex: TextureData) -> Option<DefId> {
        use esq_core::image::{ImageDef, PX_MM};
        let [pw, ph] = tex.px;
        if pw == 0 || ph == 0 {
            return None;
        }
        let (w, h) = (pw as f64 * PX_MM, ph as f64 * PX_MM);
        tex.size = [w, h];
        let base = if name.trim().is_empty() { "Image" } else { name.trim() };
        let mat = self.model.materials.insert(esq_core::model::Material {
            name: self.mat_names.plain(base),
            color: tex.avg,
            opacity: 1.0,
            ext: MaterialExt { texture: Some(tex), owner_image: true, ..Default::default() },
        });
        let mut ents = Entities::default();
        let v = [DVec3::ZERO, DVec3::new(w, 0.0, 0.0), DVec3::new(w, h, 0.0), DVec3::new(0.0, h, 0.0)]
            .map(|p| ents.add_vertex_raw(p));
        if let Some(f) = ents.add_face_raw(v.to_vec(), Vec::new()) {
            ents.faces[f].front = Some(mat);
            ents.faces[f].back = Some(mat);
        }
        Some(self.model.defs.insert(Definition {
            name: self.def_names.plain(base),
            is_group: false,
            description: String::new(),
            entities: ents,
            attributes: Default::default(),
            ext: esq_core::model::DefExt {
                image: Some(ImageDef { path: path.to_string(), px: [pw, ph] }),
                ..Default::default()
            },
        }))
    }

    fn place(&mut self, out: &mut Entities, i: &doc::Inst, def: DefId) -> esq_core::model::InstanceId {
        let material = self.el_material(&i.el);
        let id = out.instances.insert(Instance {
            def,
            xform: xf_to_mm(&i.xform),
            name: i.name.clone(),
            material,
            layer: self.layer(&i.el),
            hidden: i.el.hidden(),
            locked: i.locked,
            attributes: i.el.attrs.clone(),
            ext: esq_core::model::InstanceExt {
                hatch: i.hatch.and_then(|h| self.hatches.get(&h)).map(|h| h.1.clone()),
                guid: if self.doc.sdk_quirks { String::new() } else { i.guid.clone() },
                ..Default::default()
            },
        });
        self.shadow_flags(out, 'i', id.data().as_ffi(), &i.el);
        id
    }
}

/// The centre, normal, x axis and angles of an arc, as both readers give them.
fn arc_props(out: &mut Entities, key: &str, a: &doc::Arc) {
    let x = v3(a.xaxis).normalize_or_zero();
    let cc = v3(a.center) * IN;
    for (k, v) in [
        ("start", a.start),
        ("end", a.end),
        ("xx", x.x),
        ("xy", x.y),
        ("xz", x.z),
        ("cx", cc.x),
        ("cy", cc.y),
        ("cz", cc.z),
        ("nx", a.normal[0]),
        ("ny", a.normal[1]),
        ("nz", a.normal[2]),
    ] {
        out.set_prop(key, k, AttrValue::Number(v));
    }
}

/// The points of a curve (its edges' vertices, in no order), as a short text: the writer
/// writes an arc as read only while its points are still those (`write::plan`).
pub(crate) fn curve_fingerprint(e: &Entities, edges: &[esq_core::model::EdgeId]) -> String {
    let mut pts: Vec<[u64; 3]> = Vec::with_capacity(2 * edges.len());
    for &x in edges {
        let Some(edge) = e.edges.get(x) else { continue };
        for v in edge.v {
            let p = e.verts[v].pos;
            pts.push([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]);
        }
    }
    pts.sort_unstable();
    pts.dedup();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for w in pts.iter().flatten() {
        for b in w.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{h:016x}:{}", pts.len())
}
#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: [f64; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

    fn group_of(def: Id) -> doc::Inst {
        doc::Inst { xform: IDENTITY, def, ..Default::default() }
    }

    /// `n` group definitions, each holding a group of the next one (`nested`), or the root
    /// holding a group of each.
    fn groups(n: Id, nested: bool) -> Doc {
        let mut doc = Doc::default();
        for k in 1..=n {
            let mut d = doc::Def { id: k, is_group: true, ..Default::default() };
            if nested && k < n {
                d.ents.groups.push(group_of(k + 1));
            }
            doc.defs.push(d);
        }
        if nested {
            doc.root.groups.push(group_of(1));
        } else {
            doc.root.groups = (1..=n).map(group_of).collect();
        }
        doc
    }

    /// The work of naming and building groups is linear in their number, nested or side by
    /// side: one collection built per group (and the root), one name tried per group. Naming a
    /// group used to scan every name taken for every index tried: cubic in their number.
    #[test]
    fn groups_are_built_with_linear_work() {
        for nested in [true, false] {
            for n in [500, 1000, 2000] {
                let doc = groups(n, nested);
                let work = super::super::on_big_stack(|| {
                    let b = built(&doc);
                    (b.visited, b.def_names.probes, b.model.defs.len())
                })
                .unwrap();
                assert_eq!(work, (n as usize + 1, n as usize, n as usize), "imbriqués : {nested}, {n} groupes");
            }
        }
    }

    /// SketchUp's name of a texture: the name written, its final dots and spaces kept, then the
    /// extension of the image's format when it has none of an image.
    #[test]
    fn texture_names_keep_their_final_dots_and_spaces() {
        let jpg = std::sync::Arc::new(vec![0xFF, 0xD8, 0xFF, 0xE0]);
        let name =
            |f: &str| texture_name(&doc::Texture { file_name: f.into(), image: jpg.clone(), ..Default::default() });
        assert_eq!(name("photo"), "photo.jpg");
        assert_eq!(name("C:\\textures\\S."), "S..jpg");
        assert_eq!(name("textures/S. "), "S. .jpg");
        assert_eq!(name("../x/photo.png"), "photo.png");
        assert_eq!(name("..."), "texture.png");
    }

    /// Names taken beforehand are skipped once each, not once per new name.
    #[test]
    fn unique_names_are_found_in_linear_time() {
        const N: usize = 10_000;
        let mut names = Names::default();
        for i in 1..=N {
            names.add(&format!("Groupe#{i}"));
            names.add(&format!("Image{i}"));
        }
        names.add("Image");
        for i in 1..=N {
            assert_eq!(names.hashed("Groupe"), format!("Groupe#{}", N + i));
            assert_eq!(names.plain("Image"), format!("Image{}", N + i));
        }
        // N + 1 tries for the first name of each base, then one each.
        assert_eq!(names.probes, 2 * (2 * N));
    }

    /// A model's unit is the one of its length format (`SUModelGetUnits`): architectural and
    /// fractional in inches, engineering in feet, decimal in the unit the file names.
    #[test]
    fn units_follow_the_length_format() {
        let units = |format: i64, unit: i64| {
            let mut doc = Doc::default();
            let o = [("LengthFormat".to_string(), Value::from(format)), ("LengthUnit".to_string(), Value::from(unit))];
            doc.options.insert("UnitsOptions".into(), o.into_iter().collect());
            model(&doc).unwrap().units
        };
        assert_eq!(units(1, 4), Units::Inches);
        assert_eq!(units(3, 2), Units::Inches);
        assert_eq!(units(2, 3), Units::Feet);
        assert_eq!(units(0, 4), Units::Meters);
        assert_eq!(units(0, 2), Units::Millimeters);
    }
}
