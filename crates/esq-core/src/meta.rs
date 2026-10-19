//! What SketchUp's Ruby API keeps on the model besides geometry: tag folders and colours,
//! material textures and PBR settings, hatch patterns, the options providers of Model Info,
//! the model's name and description. Everything here is `#[serde(default)]` so older
//! documents still load.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::model::{Entities, LayerIdx, MaterialId, Model};
use crate::photo::Blob;

/// A folder of tags (`Sketchup::LayerFolder`). Folders nest; a tag lives in one folder at most.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerFolder {
    pub uid: u32,
    pub name: String,
    pub parent: Option<u32>,
    pub visible: bool,
    pub visible_on_new_pages: bool,
}

/// A hatch pattern of the model (`Sketchup::HatchPattern`, SketchUp's `HatchPatternSettings`:
/// a .pat pattern or an image, a colour, a rotation and two scales).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HatchPattern {
    pub uid: u32,
    pub name: String,
    /// Scales along x and y (`xScale`, `yScale`); `HatchPattern#size` in Ruby.
    pub size: [f64; 2],
    /// The pattern file as it was loaded: a .pat text, or an image when `image`.
    pub data: Blob,
    /// Colour of the pattern's background (`colorRed`…).
    #[serde(default = "white")]
    pub color: [u8; 3],
    /// Rotation of the pattern, degrees (`rotationAngle`).
    #[serde(default)]
    pub rotation: f64,
    /// Name of the file the pattern came from (`patName` / `textureName`).
    #[serde(default)]
    pub source: String,
    /// `data` is an image (`hasTexture`), not a .pat (`hasPat`).
    #[serde(default)]
    pub image: bool,
}

fn white() -> [u8; 3] {
    [255, 255, 255]
}

impl HatchPattern {
    /// A pattern made from a file's bytes: an image if it decodes as one (PNG, JPEG, BMP…
    /// signature), else a .pat text.
    pub fn new(uid: u32, name: &str, source: &str, data: Vec<u8>) -> HatchPattern {
        let image = crate::hatch::looks_like_image(&data);
        HatchPattern {
            uid,
            name: name.into(),
            size: [1.0, 1.0],
            data: Blob(std::sync::Arc::new(data)),
            color: white(),
            rotation: 0.0,
            source: base_name(source),
            image,
        }
    }
}

/// An image used as a texture: a material's, or one of its PBR maps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureData {
    /// File name the image came from (SketchUp keeps the bare name).
    pub filename: String,
    /// The image file as loaded (PNG, JPEG…).
    pub image: Blob,
    /// Size in pixels.
    pub px: [u32; 2],
    /// Size of one tile in the model, in millimetres.
    pub size: [f64; 2],
    /// Average colour of the image.
    pub avg: [u8; 3],
}

/// Physically based rendering settings of a material.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pbr {
    pub metalness_enabled: bool,
    pub metallic_factor: f64,
    pub metallic_texture: Option<TextureData>,
    pub roughness_enabled: bool,
    pub roughness_factor: f64,
    pub roughness_texture: Option<TextureData>,
    /// The roughness map is a glossiness map (Materials › Edit › Invert).
    pub roughness_inverted: bool,
    pub normal_enabled: bool,
    pub normal_scale: f64,
    /// 0 = OpenGL, 1 = DirectX.
    pub normal_style: u8,
    pub normal_texture: Option<TextureData>,
    pub ao_enabled: bool,
    pub ao_strength: f64,
    pub ao_texture: Option<TextureData>,
}

impl Default for Pbr {
    fn default() -> Self {
        Pbr {
            metalness_enabled: false,
            metallic_factor: 0.0,
            metallic_texture: None,
            roughness_enabled: false,
            roughness_factor: 0.5,
            roughness_texture: None,
            roughness_inverted: false,
            normal_enabled: false,
            normal_scale: 1.0,
            normal_style: 0,
            normal_texture: None,
            ao_enabled: false,
            ao_strength: 1.0,
            ao_texture: None,
        }
    }
}

/// What a material has beyond a colour and an opacity.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialExt {
    pub texture: Option<TextureData>,
    /// 0 = shift, 1 = tint.
    pub colorize_type: u8,
    /// Hue, saturation and lightness offsets of a colorized texture.
    pub colorize_deltas: [f64; 3],
    pub hatch: Option<u32>,
    /// 0 = classic, 1 = PBR metallic-roughness.
    pub workflow: u8,
    pub pbr: Pbr,
    /// The material of an Image (`MATERIAL_OWNER_IMAGE`): it belongs to the image and is not
    /// listed in Window › Materials. Exploding the image makes it an ordinary material.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub owner_image: bool,
    /// The opacity as a script gave it (`Material#alpha=`: SketchUp keeps a double), given
    /// back while `opacity` is still its rounding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha: Option<f64>,
    /// The alpha byte of the colour (SketchUp keeps the colour as RGBA: a material a script
    /// adds is (0, 0, 0, 0), a colour given takes 255); none: 255.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_alpha: Option<u8>,
}

/// Hue (degrees, −1 for a grey), lightness and saturation (0–1) of a colour, computed as
/// SketchUp computes them (components divided by 255 in single precision,
/// comparisons with a tolerance of 0.001).
pub fn hls(c: [u8; 3]) -> (f64, f64, f64) {
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

/// The colorization offsets of a texture whose average colour is `avg` to the material's
/// colour (`SUMaterialGetColorizeDeltas`): hue, saturation and lightness
/// ([`MaterialExt::colorize_deltas`]), the hue itself when the material tints (`shift`
/// false).
pub fn colorize_deltas(avg: [u8; 3], color: [u8; 3], shift: bool) -> [f64; 3] {
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

impl MaterialExt {
    pub fn sanitize_texture_names(&mut self) {
        let p = &mut self.pbr;
        for t in [
            &mut self.texture,
            &mut p.metallic_texture,
            &mut p.roughness_texture,
            &mut p.normal_texture,
            &mut p.ao_texture,
        ]
        .into_iter()
        .flatten()
        {
            t.filename = base_name(&t.filename);
        }
    }
}

/// The model-wide settings Ruby reads and writes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelMeta {
    pub name: String,
    pub description: String,
    pub guid: String,
    pub next_uid: u32,
    pub folders: Vec<LayerFolder>,
    pub hatch_patterns: Vec<HatchPattern>,
    /// Model Info providers: provider name → key → value (`Model#options`).
    pub options: BTreeMap<String, BTreeMap<String, Value>>,
    /// uid of the active tag (0 = the default tag).
    pub active_layer: u32,
    pub mipmapping: bool,
    /// Image shown behind the model in diffusion (path, opacity).
    pub diffusion_image: Option<(String, f64)>,
    pub diffusion_opacity: f64,
    pub datum: String,
    /// The material the Paint Bucket holds, as set by `Materials#current=`.
    pub current_material: Option<MaterialId>,
}

impl Default for ModelMeta {
    fn default() -> Self {
        ModelMeta {
            name: String::new(),
            description: String::new(),
            guid: String::new(),
            next_uid: 1,
            folders: Vec::new(),
            hatch_patterns: Vec::new(),
            options: default_options(),
            active_layer: 0,
            mipmapping: false,
            diffusion_image: None,
            diffusion_opacity: 0.5,
            datum: "WGS84".into(),
            current_material: None,
        }
    }
}

/// The providers of Model Info, with the values of a new document.
pub fn default_options() -> BTreeMap<String, BTreeMap<String, Value>> {
    let mut o: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    let mut put = |p: &str, kv: &[(&str, Value)]| {
        o.insert(p.into(), kv.iter().map(|(k, v)| (k.to_string(), v.clone())).collect());
    };
    put(
        "UnitsOptions",
        &[
            ("LengthFormat", json!(0)),
            ("LengthUnit", json!(2)),
            ("LengthSnapEnabled", json!(false)),
            ("LengthSnapLength", json!(1.0)),
            ("AreaUnit", json!(4)),
            ("AreaPrecision", json!(2)),
            ("VolumeUnit", json!(4)),
            ("VolumePrecision", json!(2)),
            ("AnglePrecision", json!(1)),
            ("AngleSnapEnabled", json!(true)),
            ("SnapAngle", json!(15.0)),
            ("SuppressUnitsDisplay", json!(false)),
            ("ForceInchDisplay", json!(false)),
        ],
    );
    put("PageOptions", &[("ShowTransition", json!(true)), ("TransitionTime", json!(2.0))]);
    put("SlideshowOptions", &[("LoopSlideshow", json!(false)), ("SlideTime", json!(1.0))]);
    // SketchUp 2026's CPrintOptions (keys, values of a new one).
    put(
        "PrintOptions",
        &[
            ("PrintWidth", json!(8.5)),
            ("PrintHeight", json!(11.0)),
            ("ComputeSizeFromScale", json!(false)),
            ("SizeInPrint", json!(1.0)),
            ("SizeInModel", json!(1.0)),
            ("VectorMode", json!(false)),
            ("FitToPage", json!(true)),
            ("NumberOfPages", json!(1)),
            ("LineWeight", json!(0.5)),
            ("PixelsPerInch", json!(150.0)),
            ("SectionSlice", json!(false)),
            ("ModelExtents", json!(true)),
            ("PrintQuality", json!(0)),
            ("ScaleAdjustment", json!(1.0)),
            ("QualityAdjustment", json!(1.0)),
        ],
    );
    put("NamedOptions", &[]);
    o
}

/// Keeps only the file name of a path ("../../x.png", "..\\..\\x.png", "C:x.png" → "x.png").
/// Texture names come from model files: they must never be usable as paths.
pub fn base_name(name: &str) -> String {
    let n = name.rsplit(['/', '\\']).next().unwrap_or("");
    let n = n.rsplit(':').next().unwrap_or("");
    let n = n.trim_matches(|c: char| c == '.' || c.is_whitespace() || c.is_control());
    if n.is_empty() { "texture.png".into() } else { n.to_string() }
}

/// A new random GUID, uppercase, as SketchUp shows them ("8C1F…-…").
pub fn new_guid() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    let a = h.finish();
    h.write_u64(a ^ 0x9E37_79B9_7F4A_7C15);
    let b = h.finish();
    let mut x = [0u8; 16];
    x[..8].copy_from_slice(&a.to_be_bytes());
    x[8..].copy_from_slice(&b.to_be_bytes());
    x[6] = (x[6] & 0x0f) | 0x40;
    x[8] = (x[8] & 0x3f) | 0x80;
    let hx: Vec<String> = x.iter().map(|v| format!("{v:02X}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        hx[0..4].concat(),
        hx[4..6].concat(),
        hx[6..8].concat(),
        hx[8..10].concat(),
        hx[10..].concat()
    )
}

impl Model {
    /// Attributes of a material or tag follow it when it is renamed (`attr_store` is keyed
    /// by name: `mat:<name>`, `layer:<name>`).
    pub fn rename_attr_owner(&mut self, old: &str, new: &str) {
        if old != new
            && let Some(a) = self.attr_store.remove(old)
        {
            self.attr_store.insert(new.to_string(), a);
        }
    }

    /// Gives tags their stable `uid` and the model its GUID, if they have none yet. Done
    /// before a script runs so that merely reading them is not a change of the model.
    pub fn ensure_ruby_ids(&mut self) {
        self.ensure_layer_uids();
        self.ensure_snap_ids();
        if self.meta.guid.is_empty() {
            self.meta.guid = new_guid();
        }
    }

    /// Reduces every texture file name to a bare file name (see `base_name`). Called on every
    /// document and material file that is loaded.
    pub fn sanitize_texture_names(&mut self) {
        for m in self.materials.values_mut() {
            m.ext.sanitize_texture_names();
        }
    }

    /// Gives every tag a unique, stable `uid` (tags loaded from older documents have none).
    pub fn ensure_layer_uids(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let mut dup = Vec::new();
        for (i, l) in self.layers.iter().enumerate() {
            if l.uid == 0 || !seen.insert(l.uid) {
                dup.push(i);
            }
        }
        for i in dup {
            let uid = self.meta.next_uid.max(self.layers.iter().map(|l| l.uid).max().unwrap_or(0) + 1);
            self.meta.next_uid = uid + 1;
            self.layers[i].uid = uid;
        }
        if self.layers.first().is_some_and(|l| l.uid == 0) {
            self.layers[0].uid = 1;
        }
        let top = self.layers.iter().map(|l| l.uid).chain(self.meta.folders.iter().map(|f| f.uid)).max().unwrap_or(0);
        self.meta.next_uid = self.meta.next_uid.max(top + 1);
    }

    pub fn new_uid(&mut self) -> u32 {
        self.ensure_layer_uids();
        let u = self.meta.next_uid;
        self.meta.next_uid += 1;
        u
    }

    pub fn layer_index_of_uid(&self, uid: u32) -> Option<LayerIdx> {
        self.layers.iter().position(|l| l.uid == uid)
    }

    /// Name for a new tag/material/folder that does not clash with `taken`: "Name", "Name1"…
    pub fn unique_name(base: &str, taken: &[String]) -> String {
        if !taken.iter().any(|t| t == base) {
            return base.to_string();
        }
        (1..).map(|i| format!("{base}{i}")).find(|n| !taken.iter().any(|t| t == n)).unwrap()
    }

    /// Name for a new material that does not clash with `taken`, as SketchUp makes it
    /// (`CMaterialManager::GenerateUniqueName`): names compare without case, and a taken name
    /// loses its trailing digits and gets the first free index from 1 ("Brique" → "Brique1",
    /// "Brique1" → "Brique2").
    pub fn unique_material_name(base: &str, taken: &[String]) -> String {
        let up: Vec<String> = taken.iter().map(|t| t.to_uppercase()).collect();
        let free = |n: &str| !up.contains(&n.to_uppercase());
        if free(base) {
            return base.to_string();
        }
        let stem = base.trim_end_matches(|c: char| c.is_ascii_digit());
        (1..).map(|i| format!("{stem}{i}")).find(|n| free(n)).unwrap()
    }

    /// Adds a tag (the caller checked the name is free) and returns its index.
    pub fn add_layer(&mut self, name: &str) -> LayerIdx {
        let uid = self.new_uid();
        let mut l = crate::model::Layer::new(name);
        l.uid = uid;
        self.layers.push(l);
        self.layers.len() - 1
    }

    /// Renumbers the tag of every entity (and scene) through `f`.
    fn remap_layers(&mut self, f: &dyn Fn(LayerIdx) -> LayerIdx) {
        let remap = |e: &mut Entities| {
            e.faces.values_mut().for_each(|x| x.layer = f(x.layer));
            e.edges.values_mut().for_each(|x| x.layer = f(x.layer));
            e.instances.values_mut().for_each(|x| x.layer = f(x.layer));
            e.guides.values_mut().for_each(|x| x.layer = f(x.layer));
            e.annots.values_mut().for_each(|x| x.layer = f(x.layer));
            e.sections.values_mut().for_each(|x| x.layer = f(x.layer));
            e.snaps.iter_mut().for_each(|x| x.layer = f(x.layer));
            e.spaces.iter_mut().for_each(|x| x.layer = f(x.layer));
            e.polylines.iter_mut().for_each(|x| x.layer = f(x.layer));
        };
        remap(&mut self.root);
        for d in self.defs.values_mut() {
            remap(&mut d.entities);
        }
        for s in self.scenes.iter_mut() {
            if let Some(h) = &mut s.hidden_layers {
                *h = h.iter().map(|&l| f(l)).collect();
            }
        }
    }

    /// Removes tag `idx` (never the default one). Its entities go to the default tag, or are
    /// erased when `delete_geometry`.
    pub fn remove_layer(&mut self, idx: LayerIdx, delete_geometry: bool) {
        if idx == 0 || idx >= self.layers.len() {
            return;
        }
        if delete_geometry {
            let kill = |e: &mut Entities| {
                let all: Vec<crate::model::EntityRef> = e
                    .instances
                    .iter()
                    .filter(|(_, x)| x.layer == idx)
                    .map(|(k, _)| crate::model::EntityRef::Instance(k))
                    .chain(
                        e.faces.iter().filter(|(_, x)| x.layer == idx).map(|(k, _)| crate::model::EntityRef::Face(k)),
                    )
                    .chain(
                        e.edges.iter().filter(|(_, x)| x.layer == idx).map(|(k, _)| crate::model::EntityRef::Edge(k)),
                    )
                    .chain(
                        e.guides.iter().filter(|(_, x)| x.layer == idx).map(|(k, _)| crate::model::EntityRef::Guide(k)),
                    )
                    .chain(
                        e.annots.iter().filter(|(_, x)| x.layer == idx).map(|(k, _)| crate::model::EntityRef::Annot(k)),
                    )
                    .chain(
                        e.sections
                            .iter()
                            .filter(|(_, x)| x.layer == idx)
                            .map(|(k, _)| crate::model::EntityRef::Section(k)),
                    )
                    .collect();
                e.erase(&all);
                e.snaps.retain(|s| s.layer != idx);
                e.spaces.retain(|s| s.layer != idx);
                e.polylines.retain(|s| s.layer != idx);
            };
            kill(&mut self.root);
            for d in self.defs.values_mut() {
                kill(&mut d.entities);
            }
        }
        for s in self.scenes.iter_mut() {
            if let Some(h) = &mut s.hidden_layers {
                h.retain(|&l| l != idx);
            }
        }
        self.remap_layers(&|l| {
            if l == idx {
                0
            } else if l > idx {
                l - 1
            } else {
                l
            }
        });
        let uid = self.layers[idx].uid;
        let key = format!("layer:{}", self.layers[idx].name);
        self.attr_store.remove(&key);
        self.layers.remove(idx);
        if self.meta.active_layer == uid {
            self.meta.active_layer = 0;
        }
    }

    /// Tags no entity uses (the default tag stays). Returns the indices, highest first.
    pub fn unused_layers(&self) -> Vec<LayerIdx> {
        let mut used = std::collections::HashSet::from([0usize]);
        let note = |e: &Entities, used: &mut std::collections::HashSet<usize>| {
            used.extend(e.faces.values().map(|x| x.layer));
            used.extend(e.edges.values().map(|x| x.layer));
            used.extend(e.instances.values().map(|x| x.layer));
            used.extend(e.guides.values().map(|x| x.layer));
            used.extend(e.annots.values().map(|x| x.layer));
            used.extend(e.sections.values().map(|x| x.layer));
            used.extend(e.snaps.iter().map(|x| x.layer));
            used.extend(e.spaces.iter().map(|x| x.layer));
            used.extend(e.polylines.iter().map(|x| x.layer));
        };
        note(&self.root, &mut used);
        for d in self.defs.values() {
            note(&d.entities, &mut used);
        }
        (1..self.layers.len()).rev().filter(|i| !used.contains(i)).collect()
    }

    /// Whether folder `a` is `b` or lies inside `b`.
    pub fn folder_within(&self, a: u32, b: u32) -> bool {
        let mut cur = Some(a);
        let mut guard = 0;
        while let Some(c) = cur {
            if c == b {
                return true;
            }
            guard += 1;
            if guard > 1000 {
                return false;
            }
            cur = self.meta.folders.iter().find(|f| f.uid == c).and_then(|f| f.parent);
        }
        false
    }

    /// Whether what is on tag `l` shows: the tag is visible and so is every folder holding it,
    /// at any depth (SketchUp's tag test). Tags and folders keep their own
    /// switch: hiding a folder changes neither its tags' nor its sub-folders'. A tag that does
    /// not exist shows.
    pub fn layer_shown(&self, l: LayerIdx) -> bool {
        let Some(layer) = self.layers.get(l) else { return true };
        if !layer.visible {
            return false;
        }
        let mut cur = layer.folder;
        // A damaged file can make folders hold each other: at most one visit per folder.
        for _ in 0..=self.meta.folders.len() {
            let Some(f) = cur.and_then(|u| self.meta.folders.iter().find(|f| f.uid == u)) else { break };
            if !f.visible {
                return false;
            }
            cur = f.parent;
        }
        true
    }

    /// `layer_shown` of every tag, by index (for the loops that ask it of each entity).
    pub fn layers_shown(&self) -> Vec<bool> {
        (0..self.layers.len()).map(|l| self.layer_shown(l)).collect()
    }

    /// Removes a folder with everything in it: its tags and sub-folders are kept in the
    /// model but moved to the root, as SketchUp does when a folder is removed.
    pub fn remove_folder(&mut self, uid: u32) {
        let parent = self.meta.folders.iter().find(|f| f.uid == uid).and_then(|f| f.parent);
        for l in self.layers.iter_mut().filter(|l| l.folder == Some(uid)) {
            l.folder = parent;
        }
        for f in self.meta.folders.iter_mut().filter(|f| f.parent == Some(uid)) {
            f.parent = parent;
        }
        self.meta.folders.retain(|f| f.uid != uid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_get_uids_and_can_be_removed() {
        let mut m = Model::new();
        m.ensure_layer_uids();
        let a = m.add_layer("A");
        let b = m.add_layer("B");
        assert_ne!(m.layers[a].uid, m.layers[b].uid);
        let uid_b = m.layers[b].uid;
        m.remove_layer(a, false);
        assert_eq!(m.layer_index_of_uid(uid_b), Some(1));
    }

    /// A tag shows when it and every folder above it are visible; hiding a folder changes no
    /// other switch (TestUp `TC_Sketchup_LayerFolder#test_visible_Query_nested`).
    #[test]
    fn tags_show_through_their_folders() {
        let mut m = Model::new();
        m.ensure_layer_uids();
        let t = m.add_layer("T");
        let (outer, inner) = (m.new_uid(), m.new_uid());
        m.meta.folders.push(LayerFolder {
            uid: outer,
            name: "A".into(),
            parent: None,
            visible: true,
            visible_on_new_pages: true,
        });
        m.meta.folders.push(LayerFolder {
            uid: inner,
            name: "B".into(),
            parent: Some(outer),
            visible: true,
            visible_on_new_pages: true,
        });
        m.layers[t].folder = Some(inner);
        assert!(m.layer_shown(t) && m.layer_shown(0) && m.layer_shown(99));
        m.meta.folders[0].visible = false;
        assert!(!m.layer_shown(t) && m.layer_shown(0));
        assert!(m.layers[t].visible && m.meta.folders[1].visible);
        assert_eq!(m.layers_shown(), vec![true, false]);
        m.meta.folders[0].visible = true;
        m.layers[t].visible = false;
        assert!(!m.layer_shown(t));
        // Folders that hold each other (a damaged file): the walk ends.
        m.layers[t].visible = true;
        m.meta.folders[0].parent = Some(inner);
        assert!(m.layer_shown(t));
    }

    #[test]
    fn unique_names() {
        let taken = vec!["Brique".to_string(), "Brique1".to_string()];
        assert_eq!(Model::unique_name("Brique", &taken), "Brique2");
        assert_eq!(Model::unique_name("Autre", &taken), "Autre");
        // Materials: without case, trailing digits replaced.
        assert_eq!(Model::unique_material_name("brique", &taken), "brique2");
        assert_eq!(Model::unique_material_name("Brique1", &taken), "Brique2");
        assert_eq!(Model::unique_material_name("Brique7", &taken), "Brique7");
        assert_eq!(Model::unique_material_name("Autre", &taken), "Autre");
    }
}

// ------------------------------------------------------------------------------------------
// Texture positioning on faces.

use glam::{DVec2, DVec3};

/// A map from a point of the model (mm, in the face's collection) to texture coordinates:
/// u = u·(x, y, z, 1) / q·(x, y, z, 1), v = v·(x, y, z, 1) / q·(x, y, z, 1). One unit of u or
/// v is one tile. `q` is (0, 0, 0, 1) except for a distorted texture (Position Texture's
/// yellow pin, four pins of `Face#position_material`), which is a perspective of the image:
/// a renderer must then interpolate (u·p, v·p, q·p) and divide per pixel ([`UvMap::uvq`]).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct UvMap {
    pub u: [f64; 4],
    pub v: [f64; 4],
    #[serde(default = "affine_q", skip_serializing_if = "is_affine_q")]
    pub q: [f64; 4],
}

fn affine_q() -> [f64; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

fn is_affine_q(q: &[f64; 4]) -> bool {
    *q == affine_q()
}

impl UvMap {
    /// An affine map (no perspective).
    pub fn affine(u: [f64; 4], v: [f64; 4]) -> UvMap {
        UvMap { u, v, q: affine_q() }
    }

    pub fn is_affine(&self) -> bool {
        self.q[0].abs() < 1e-15 && self.q[1].abs() < 1e-15 && self.q[2].abs() < 1e-15
    }

    /// The homogeneous texture coordinates (u·p, v·p, q·p) of a point.
    pub fn uvq(&self, p: DVec3) -> DVec3 {
        let f = |c: &[f64; 4]| c[0] * p.x + c[1] * p.y + c[2] * p.z + c[3];
        DVec3::new(f(&self.u), f(&self.v), f(&self.q))
    }

    pub fn at(&self, p: DVec3) -> DVec2 {
        let h = self.uvq(p);
        let w = if h.z.abs() < 1e-300 { 1e-300f64.copysign(h.z) } else { h.z };
        DVec2::new(h.x / w, h.y / w)
    }

    /// The point of the plane (through `origin`, with `normal`) whose texture coordinates are
    /// `uv`, if the map is not degenerate on that plane.
    pub fn point_at(&self, uv: DVec2, origin: DVec3, normal: DVec3) -> Option<DVec3> {
        let (ex, ey) = face_axes(normal);
        // u·p − U q·p = 0 and v·p − V q·p = 0, with p = origin + s ex + t ey.
        let row = |c: &[f64; 4], k: f64| {
            let r = |i: usize| c[i] - k * self.q[i];
            let d = DVec3::new(r(0), r(1), r(2));
            (d.dot(origin) + r(3), d.dot(ex), d.dot(ey))
        };
        let (a0, a1, a2) = row(&self.u, uv.x);
        let (b0, b1, b2) = row(&self.v, uv.y);
        let det = a1 * b2 - a2 * b1;
        let scale = (a1.abs() + a2.abs()) * (b1.abs() + b2.abs());
        if det.abs() <= 1e-12 * scale.max(1e-300) {
            return None;
        }
        let s = (-a0 * b2 + a2 * b0) / det;
        let t = (-a1 * b0 + a0 * b1) / det;
        Some(origin + ex * s + ey * t)
    }

    /// The same map seen through a projection along `dir` onto the plane (`origin`, `normal`)
    /// it was made for: a point anywhere takes the coordinates of its shadow on that plane
    /// (Texture › Projected, painting with a projected sample).
    pub fn projected(&self, origin: DVec3, normal: DVec3, dir: DVec3) -> Option<UvMap> {
        let dn = dir.dot(normal);
        if dn.abs() < 1e-9 {
            return None;
        }
        // p' = p − dir (n·p − n·o) / (d·n)
        let k = normal.dot(origin);
        let row = |c: &[f64; 4]| {
            let r = DVec3::new(c[0], c[1], c[2]);
            let rd = r.dot(dir) / dn;
            let x = r - normal * rd;
            [x.x, x.y, x.z, c[3] + rd * k]
        };
        Some(UvMap { u: row(&self.u), v: row(&self.v), q: row(&self.q) })
    }
}

/// How the front and the back of a face are textured.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FaceTex {
    pub front: Option<UvMap>,
    pub back: Option<UvMap>,
    /// Projection direction of a projected texture, per side.
    pub projection: [Option<DVec3>; 2],
    /// The pins the texture was last positioned with (Position Texture, `position_material`),
    /// per side: model point (mm) and texture coordinates. Position Texture shows them again.
    #[serde(skip_serializing_if = "no_pins")]
    pub pins: [Vec<(DVec3, DVec2)>; 2],
}

fn no_pins(p: &[Vec<(DVec3, DVec2)>; 2]) -> bool {
    p[0].is_empty() && p[1].is_empty()
}

/// SketchUp's tolerance (0.001 inch), with which it compares a plane's normal to the vertical
/// when it chooses the texture axes.
const AXES_TOLERANCE: f64 = 1e-3;

/// The 2D frame SketchUp lays textures in on a plane (the "arbitrary axis" rule of the UV
/// helper, of SketchUpAPI.dll): x = world-z × n, or world-y × n when n is the
/// vertical (up or down, each component within 0.001); y = n × x. For a face facing down, x is
/// therefore −X (and y is +Y). Both sides of a face use the frame of its (front) normal, and a
/// projected texture the frame of its projection direction.
pub fn face_axes(normal: DVec3) -> (DVec3, DVec3) {
    let n = normal.normalize_or_zero();
    let vertical =
        |z: f64| n.x.abs() <= AXES_TOLERANCE && n.y.abs() <= AXES_TOLERANCE && (n.z - z).abs() <= AXES_TOLERANCE;
    let up = if vertical(1.0) || vertical(-1.0) { DVec3::Y } else { DVec3::Z };
    let ex = up.cross(n).normalize_or_zero();
    let ey = n.cross(ex).normalize_or_zero();
    (ex, ey)
}

/// The default projection of a texture whose tile is `tile` (mm) on a face: SketchUp's
/// unpositioned texture (identity matrix in the frame of [`face_axes`]). Both sides of a face
/// take the frame of its normal (`Face#mesh` gives the same UVs front and back).
pub fn default_uv(normal: DVec3, tile: [f64; 2]) -> UvMap {
    let (ex, ey) = face_axes(normal);
    let (w, h) = (tile[0].max(1e-9), tile[1].max(1e-9));
    UvMap::affine([ex.x / w, ex.y / w, ex.z / w, 0.0], [ey.x / h, ey.y / h, ey.z / h, 0.0])
}

/// The texture coordinates SketchUp gives one side of a face (its UV helper, `Face#mesh`,
/// exporters, the renderer): the side's placement when it is painted with a texture and
/// positioned or projected, otherwise the default projection in the frame of the face's
/// normal (both sides alike). `tile` is the texture's tile (mm); `None` for a colour or no
/// material: one inch per tile, any placement ignored.
pub fn side_uv(tex: Option<&FaceTex>, front: bool, normal: DVec3, tile: Option<[f64; 2]>) -> UvMap {
    match tile {
        None => default_uv(normal, [25.4, 25.4]),
        Some(t) => tex.and_then(|x| if front { x.front } else { x.back }).unwrap_or_else(|| default_uv(normal, t)),
    }
}

/// The map that sends each model point of `pairs` (mm) to its texture coordinates, on the
/// plane facing `normal` (`Face#position_material`, `SUFacePositionMaterial`), as SketchUp
/// solves it (in SketchUpAPI.dll) in texture length (u × tile width): one pair
/// moves the default projection, two also rotate and scale it, three give any affine map, four
/// a perspective (a distorted texture). `None` when the pairs are degenerate (SketchUp refuses
/// them: three texture or model points on a line, a quadrilateral that folds…); only the first
/// four pairs count.
pub fn uv_from_pairs(normal: DVec3, tile: [f64; 2], pairs: &[(DVec3, DVec2)]) -> Option<UvMap> {
    let frame = crate::texpos::Frame::new(DVec3::ZERO, normal);
    crate::texpos::map_from_pairs(&frame, tile, pairs)
}

#[cfg(test)]
mod uv_tests {
    use super::*;

    #[test]
    fn pairs_are_honoured() {
        let n = DVec3::Z;
        let pairs = [
            (DVec3::new(0.0, 0.0, 0.0), DVec2::new(0.0, 0.0)),
            (DVec3::new(100.0, 0.0, 0.0), DVec2::new(0.0, 1.0)),
            (DVec3::new(0.0, 50.0, 0.0), DVec2::new(-2.0, 0.0)),
        ];
        for k in 1..=3 {
            let m = uv_from_pairs(n, [10.0, 10.0], &pairs[..k]).unwrap();
            for (p, uv) in &pairs[..k] {
                assert!((m.at(*p) - *uv).length() < 1e-9, "{k} paires : {:?} -> {:?}", p, m.at(*p));
            }
            let back = m.point_at(DVec2::new(0.25, 0.75), DVec3::ZERO, n).unwrap();
            assert!((m.at(back) - DVec2::new(0.25, 0.75)).length() < 1e-9);
        }
    }

    #[test]
    fn four_pairs_make_a_perspective() {
        let n = DVec3::Z;
        let pairs = [
            (DVec3::new(0.0, 0.0, 0.0), DVec2::new(0.0, 0.0)),
            (DVec3::new(100.0, 0.0, 0.0), DVec2::new(1.0, 0.0)),
            (DVec3::new(0.0, 100.0, 0.0), DVec2::new(0.0, 1.0)),
            (DVec3::new(60.0, 70.0, 0.0), DVec2::new(1.0, 1.0)),
        ];
        let m = uv_from_pairs(n, [10.0, 10.0], &pairs).unwrap();
        assert!(!m.is_affine());
        for (p, uv) in &pairs {
            assert!((m.at(*p) - *uv).length() < 1e-9, "{p:?} -> {:?}", m.at(*p));
        }
        let p = m.point_at(DVec2::new(0.5, 0.5), DVec3::ZERO, n).unwrap();
        assert!((m.at(p) - DVec2::new(0.5, 0.5)).length() < 1e-9);
    }

    #[test]
    fn projected_maps_follow_the_shadow() {
        // A map made on the ground, projected down onto a slope.
        let m = default_uv(DVec3::Z, [100.0, 100.0]);
        let p = m.projected(DVec3::ZERO, DVec3::Z, DVec3::Z).unwrap();
        let q = DVec3::new(250.0, 50.0, 400.0);
        assert!((p.at(q) - m.at(DVec3::new(250.0, 50.0, 0.0))).length() < 1e-12);
        assert!(m.projected(DVec3::ZERO, DVec3::Z, DVec3::X).is_none());
    }

    #[test]
    fn default_follows_the_axes() {
        let m = default_uv(DVec3::Z, [254.0, 508.0]);
        assert!((m.at(DVec3::new(254.0, 508.0, 0.0)) - DVec2::new(1.0, 1.0)).length() < 1e-12);
        let (ex, ey) = face_axes(DVec3::Y);
        assert!(ex.z.abs() < 1e-12 && ey.z > 0.99, "{ex:?} {ey:?}");
    }

    fn close(a: DVec3, b: DVec3) -> bool {
        a.distance(b) < 1e-12
    }

    /// SketchUp's arbitrary axis rule: world-y × n for the vertical within
    /// 0.001 per component, world-z × n otherwise.
    #[test]
    fn axes_are_sketchups() {
        let (ex, ey) = face_axes(-DVec3::Z);
        assert!(close(ex, -DVec3::X) && close(ey, DVec3::Y), "face down: {ex:?} {ey:?}");
        let (ex, ey) = face_axes(DVec3::Z);
        assert!(close(ex, DVec3::X) && close(ey, DVec3::Y));
        let (ex, ey) = face_axes(-DVec3::Y);
        assert!(close(ex, DVec3::X) && close(ey, DVec3::Z), "{ex:?} {ey:?}");
        // Within the tolerance: still world-y × n; beyond: world-z × n, horizontal.
        let n = DVec3::new(0.0009, 0.0, 1.0).normalize();
        assert!(face_axes(n).0.y.abs() < 1e-6);
        let n = DVec3::new(0.002, 0.0, 1.0).normalize();
        let (ex, _) = face_axes(n);
        assert!(ex.z.abs() < 1e-12 && (ex - DVec3::Y).length() < 1e-9, "{ex:?}");
    }

    /// `Face#mesh` of a face on the ground (facing down) without material: u = −x, v = y in
    /// inches, the same on both sides (TestUp `test_mesh_all_flags`).
    #[test]
    fn a_face_down_maps_minus_x() {
        let n = -DVec3::Z;
        let p = DVec3::new(100.0, -1000.0, 0.0) * 25.4;
        for front in [true, false] {
            let m = side_uv(None, front, n, None);
            assert!((m.at(p) - DVec2::new(-100.0, -1000.0)).length() < 1e-9, "{front}");
        }
    }

    /// One pair moves the default projection (TestUp `TC_Geom_PolygonMesh#test_uv_at_from_textured_face`:
    /// face (0,0,0) (1,2,3) (2,3,4) (3,2,1) in inches, (1,2,3) at (0, 0), a 10" tile).
    #[test]
    fn one_pair_on_a_slanted_face() {
        let i = |x: f64, y: f64, z: f64| DVec3::new(x, y, z) * 25.4;
        let n = (i(1.0, 2.0, 3.0)).cross(i(2.0, 3.0, 4.0)).normalize();
        let m = uv_from_pairs(n, [254.0, 254.0], &[(i(1.0, 2.0, 3.0), DVec2::ZERO)]).unwrap();
        let (s5, s30) = (5f64.sqrt(), 30f64.sqrt());
        for (p, uv) in [
            (i(3.0, 2.0, 1.0), DVec2::new(-0.4 / s5, -1.2 / s30)),
            (i(0.0, 0.0, 0.0), DVec2::new(0.4 / s5, -1.8 / s30)),
            (i(2.0, 3.0, 4.0), DVec2::new(-0.3 / s5, 0.6 / s30)),
        ] {
            assert!((m.at(p) - uv).length() < 1e-12, "{p:?} -> {:?}, {uv:?}", m.at(p));
        }
    }

    /// Two pairs: a similarity in texture length, not in tiles (the tile is not square).
    #[test]
    fn two_pairs_keep_the_texture_square() {
        let pairs = [(DVec3::ZERO, DVec2::ZERO), (DVec3::new(100.0, 0.0, 0.0), DVec2::new(1.0, 0.0))];
        let m = uv_from_pairs(DVec3::Z, [100.0, 50.0], &pairs).unwrap();
        assert!((m.at(DVec3::new(0.0, 50.0, 0.0)) - DVec2::new(0.0, 1.0)).length() < 1e-12);
    }

    /// Model points on a line make no placement (TestUp `test_position_material_linear_points`).
    #[test]
    fn pairs_on_a_line_are_refused() {
        let p = |x: f64| DVec3::new(x, 0.0, 0.0);
        let pairs = [
            (p(100.0), DVec2::new(0.0, 0.0)),
            (p(200.0), DVec2::new(1.0, 0.0)),
            (p(250.0), DVec2::new(1.0, 1.0)),
            (p(350.0), DVec2::new(0.0, 1.0)),
        ];
        assert!(uv_from_pairs(DVec3::Z, [10.0, 10.0], &pairs).is_none());
        assert!(uv_from_pairs(DVec3::Z, [10.0, 10.0], &pairs[..3]).is_none());
    }
}
