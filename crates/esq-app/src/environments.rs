//! Environments (Window › Environments): a high dynamic range panorama around the model,
//! shown as a skydome behind it and reflected by the photoreal materials.
//!
//! Kept where the Ruby API keeps them (`Sketchup::Environments`): the list in
//! `view_ext["envs"]`, the current one's id in `view_ext["env_current"]`; the image file
//! travels in the model (`Model::files["env:<id>"]`), else it is read from `path`.
//!
//! Files: SketchUp's `.ske` (a ZIP: `document.xml` with the settings, `ref/` the HDR
//! image, `doc_thumbnail.png`), or a bare `.hdr` / `.exr` panorama. The images are
//! equirectangular: `u` turns around the horizon, `v` goes from the zenith to the nadir.
//! As SketchUp places them (`EnvironmentManager`'s sun): the image point `u` lies in the
//! direction at angle `rotation − u·360°` from the red axis' negative side, measured towards
//! the green axis' negative side.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use esq_core::glam::DVec3;
use esq_core::model::Model;
use esq_core::photo::Blob;
use serde_json::{Map, Value as J, json};

use crate::style_file::{Node, parse_xml};

/// SketchUp's own environments, when it is installed: the tests compare with them (never
/// read by the application).
#[cfg(test)]
pub const SKETCHUP_LIBRARY: &str = r"C:\ProgramData\SketchUp\SketchUp 2026\SketchUp\Environments";
/// Largest panorama width sent to the GPU.
const MAX_WIDTH: u32 = 2048;

// ------------------------------------------------------------------------------ data

/// One environment of the model (`Sketchup::Environment`).
#[derive(Clone, Debug, PartialEq)]
pub struct Env {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub path: String,
    pub skydome: bool,
    pub reflections: bool,
    /// Degrees, 0–360.
    pub rotation: f64,
    /// 0–20.
    pub skydome_exposure: f64,
    /// 0–10.
    pub reflection_exposure: f64,
    pub linked_sun: bool,
    /// Azimuth (0–360°) and elevation (0–90°) of the sun in the panorama.
    pub sun_pos: [f64; 2],
}

impl Env {
    fn of(v: &J) -> Option<Env> {
        let f = |k: &str, d: f64| v[k].as_f64().unwrap_or(d);
        let b = |k: &str| v[k].as_bool().unwrap_or(false);
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        let sun = v["sun_pos"].as_array();
        let sp = |i: usize, d: f64| sun.and_then(|a| a.get(i)).and_then(J::as_f64).unwrap_or(d);
        Some(Env {
            id: v["id"].as_i64()?,
            name: s("name"),
            description: s("description"),
            path: s("path"),
            skydome: b("skydome"),
            reflections: b("reflections"),
            rotation: f("rotation", 0.0),
            skydome_exposure: f("skydome_exposure", 1.0),
            reflection_exposure: f("reflection_exposure", 1.0),
            linked_sun: b("linked_sun"),
            sun_pos: [sp(0, 0.0), sp(1, 45.0)],
        })
    }

    fn write(&self, d: &mut Map<String, J>) {
        d.insert("id".into(), json!(self.id));
        d.insert("name".into(), json!(self.name));
        d.insert("description".into(), json!(self.description));
        d.insert("path".into(), json!(self.path));
        d.insert("skydome".into(), json!(self.skydome));
        d.insert("reflections".into(), json!(self.reflections));
        d.insert("rotation".into(), json!(self.rotation));
        d.insert("skydome_exposure".into(), json!(self.skydome_exposure));
        d.insert("reflection_exposure".into(), json!(self.reflection_exposure));
        d.insert("linked_sun".into(), json!(self.linked_sun));
        d.insert("sun_pos".into(), json!(self.sun_pos));
    }

    /// The settings of a new environment (as `Environments#add` makes them).
    pub fn new(name: &str) -> Env {
        Env {
            id: 0,
            name: name.into(),
            description: String::new(),
            path: String::new(),
            skydome: true,
            reflections: true,
            rotation: 0.0,
            skydome_exposure: 1.0,
            reflection_exposure: 1.0,
            linked_sun: false,
            sun_pos: [0.0, 45.0],
        }
    }
}

pub fn list(m: &Model) -> Vec<Env> {
    m.view_ext.get("envs").and_then(J::as_array).map(|l| l.iter().filter_map(Env::of).collect()).unwrap_or_default()
}

pub fn current_id(m: &Model) -> Option<i64> {
    let id = m.view_ext.get("env_current")?.as_i64()?;
    list(m).iter().any(|e| e.id == id).then_some(id)
}

pub fn current(m: &Model) -> Option<Env> {
    let id = current_id(m)?;
    list(m).into_iter().find(|e| e.id == id)
}

pub fn set_current(m: &mut Model, id: Option<i64>) {
    m.view_ext.insert("env_current".into(), id.map_or(J::Null, |i| json!(i)));
}

/// Saves the settings of `e` (found by id); unknown keys of the entry are kept.
pub fn update(m: &mut Model, e: &Env) -> bool {
    let Some(list) = m.view_ext.get_mut("envs").and_then(J::as_array_mut) else { return false };
    let Some(d) = list.iter_mut().find(|d| d["id"].as_i64() == Some(e.id)).and_then(J::as_object_mut) else {
        return false;
    };
    e.write(d);
    true
}

/// A name not yet taken: `name`, else `name 1`, `name 2`… (as `Environments#add`).
pub fn unique_name(m: &Model, name: &str) -> String {
    let taken: Vec<String> = list(m).into_iter().map(|e| e.name).collect();
    let mut n = name.to_string();
    let mut i = 1;
    while taken.contains(&n) {
        n = format!("{name} {i}");
        i += 1;
    }
    n
}

/// Adds an environment (its name made unique) with the file that holds its image; returns
/// its id.
pub fn add(m: &mut Model, mut e: Env, file: Option<Blob>) -> i64 {
    let id = list(m).iter().map(|e| e.id).max().unwrap_or(0) + 1;
    e.id = id;
    e.name = unique_name(m, &e.name);
    let mut d = Map::new();
    e.write(&mut d);
    let entry = m.view_ext.entry("envs".into()).or_insert_with(|| json!([]));
    if !entry.is_array() {
        *entry = json!([]);
    }
    entry.as_array_mut().unwrap().push(J::Object(d));
    if let Some(f) = file {
        m.files.insert(file_key(id), f);
    }
    id
}

pub fn remove(m: &mut Model, id: i64) {
    if let Some(l) = m.view_ext.get_mut("envs").and_then(J::as_array_mut) {
        l.retain(|d| d["id"].as_i64() != Some(id));
    }
    m.files.remove(&file_key(id));
    if m.view_ext.get("env_current").and_then(J::as_i64) == Some(id) {
        set_current(m, None);
    }
}

/// Names of the environments the scenes use.
fn used_by_scenes(m: &Model) -> Vec<String> {
    m.scenes.iter().filter_map(|s| s.ext.props.get("env").and_then(J::as_str).map(str::to_string)).collect()
}

/// Model Info › Statistics / the panel's "Purge Unused": keeps the current environment and
/// those of the scenes. Returns how many went.
pub fn purge_unused(m: &mut Model) -> usize {
    let (cur, used) = (current_id(m), used_by_scenes(m));
    let gone: Vec<i64> =
        list(m).into_iter().filter(|e| Some(e.id) != cur && !used.contains(&e.name)).map(|e| e.id).collect();
    for &id in &gone {
        remove(m, id);
    }
    gone.len()
}

pub fn file_key(id: i64) -> String {
    format!("env:{id}")
}

// ------------------------------------------------------------------------------ files

/// What a `.ske` file holds.
#[derive(Clone, Debug, Default)]
pub struct SkeFile {
    pub name: String,
    pub description: String,
    pub skydome: bool,
    pub reflections: bool,
    pub rotation: f64,
    pub skydome_exposure: f64,
    pub reflection_exposure: f64,
    pub linked_sun: bool,
    /// Degrees (the file keeps fractions of a turn and of a quarter turn).
    pub sun_pos: [f64; 2],
    /// The panorama file (`.hdr` or `.exr`).
    pub image: Vec<u8>,
    pub thumbnail: Option<Vec<u8>>,
}

impl SkeFile {
    pub fn env(&self) -> Env {
        Env {
            id: 0,
            name: self.name.clone(),
            description: self.description.clone(),
            path: String::new(),
            skydome: self.skydome,
            reflections: self.reflections,
            rotation: self.rotation,
            skydome_exposure: self.skydome_exposure,
            reflection_exposure: self.reflection_exposure,
            linked_sun: self.linked_sun,
            sun_pos: self.sun_pos,
        }
    }
}

pub fn is_zip(b: &[u8]) -> bool {
    b.starts_with(b"PK\x03\x04")
}

/// Reads a SketchUp environment file.
pub fn read_ske(bytes: &[u8]) -> Result<SkeFile, String> {
    let files = crate::zip::read(bytes)?;
    let doc = files.get("document.xml").ok_or("document.xml manquant")?;
    let root = parse_xml(&String::from_utf8_lossy(doc))?;
    let mut all = Vec::new();
    root.find_all("environment", &mut all);
    let env: &Node = all.first().ok_or("environnement introuvable dans document.xml")?;
    let num = |k: &str, d: f64| env.attr(k).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(d);
    let flag = |k: &str| env.attr(k).is_some_and(|v| v.trim() == "1" || v.trim() == "true");
    let image_path = env.child("ibl_image").and_then(|n| n.attr("path")).map(str::to_string);
    let image = image_path
        .and_then(|p| files.get(&format!("ref/{p}")).cloned())
        .or_else(|| {
            files
                .iter()
                .find(|(k, _)| k.starts_with("ref/") && (k.ends_with(".hdr") || k.ends_with(".exr")))
                .map(|(_, v)| v.clone())
        })
        .ok_or("image HDR manquante")?;
    Ok(SkeFile {
        name: env.attr("name").unwrap_or_default().to_string(),
        description: env.attr("description").unwrap_or_default().to_string(),
        skydome: flag("skybox_enabled"),
        reflections: flag("ibl_enabled"),
        rotation: num("rotation_angle", 0.0),
        skydome_exposure: num("skybox_gain", 1.0),
        reflection_exposure: num("reflection_gain", 1.0),
        linked_sun: flag("shadow_light_enabled"),
        sun_pos: [num("shadow_light_positionX", 0.0) * 360.0, num("shadow_light_positionY", 0.5) * 90.0],
        image,
        thumbnail: files.get("doc_thumbnail.png").cloned(),
    })
}

/// The French name of an environment SketchUp names in English (Esquisse's translations).
pub fn localized(name: &str) -> String {
    crate::i18n::fr_or_same(name)
}

/// The environments that come with Esquisse (`Environments` of its content).
pub fn shipped_dir() -> Option<std::path::PathBuf> {
    esq_core::content::folder("Environments")
}

/// An environment that can be added: a `.ske` of a library, or a panorama.
#[derive(Clone, Debug)]
pub struct LibEnv {
    pub name: String,
    pub path: std::path::PathBuf,
}

/// The name a `.ske` file gives its environment (only its `document.xml` is read).
fn ske_name(path: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let files = crate::zip::read_only(&bytes, |n| n == "document.xml").ok()?;
    let root = parse_xml(&String::from_utf8_lossy(files.get("document.xml")?)).ok()?;
    let mut all = Vec::new();
    root.find_all("environment", &mut all);
    all.first()?.attr("name").map(str::to_string).filter(|n| !n.trim().is_empty())
}

/// The environments of a folder (`.ske`, `.hdr`, `.exr`), by name: a `.ske`'s own name, else
/// the file's (SketchUp's English names in French).
pub fn read_folder(dir: &std::path::Path) -> Vec<LibEnv> {
    let mut out: Vec<LibEnv> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension().and_then(|x| x.to_str()).is_some_and(|x| {
                        let x = x.to_ascii_lowercase();
                        x == "ske" || x == "hdr" || x == "exr"
                    })
                })
                .map(|p| {
                    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    let name = ske_name(&p).unwrap_or(stem);
                    LibEnv { name: localized(&name), path: p }
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// %APPDATA%\Esquisse\Environments, where Esquisse keeps the user's environments.
pub fn user_dir() -> Option<std::path::PathBuf> {
    Some(crate::settings::user_dir()?.join("Environments"))
}

/// The environment a file makes: a `.ske` brings its settings, a panorama the defaults
/// (named after the file). The file itself goes into the model.
pub fn from_file(path: &std::path::Path) -> Result<(Env, Blob), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut env = if is_zip(&bytes) {
        let ske = read_ske(&bytes)?;
        let mut e = ske.env();
        if e.name.is_empty() {
            e.name = stem.clone();
        }
        e.name = localized(&e.name);
        e
    } else {
        // Checks that it reads.
        decode(&bytes)?;
        Env::new(if stem.is_empty() { "Environnement" } else { &stem })
    };
    env.path = path.to_string_lossy().into_owned();
    Ok((env, Blob(Arc::new(bytes))))
}

// ------------------------------------------------------------------------------ images

/// A panorama ready for the GPU: RGB9E5 texels (linear light), level 0 first.
pub struct EnvImage {
    pub levels: Vec<(u32, u32, Vec<u32>)>,
    /// A small tone-mapped picture (sRGB RGBA) for the panel.
    pub thumb: (u32, u32, Vec<u8>),
}

impl std::fmt::Debug for EnvImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EnvImage({}×{})", self.levels[0].0, self.levels[0].1)
    }
}

/// The picture of a panorama file (`.ske`: its image), as it is stored.
pub fn decode_picture(bytes: &[u8]) -> Option<image::DynamicImage> {
    if is_zip(bytes) {
        return crate::photo_img::decode(&read_ske(bytes).ok()?.image).ok();
    }
    crate::photo_img::decode(bytes).ok()
}

/// Linear RGB pixels of a panorama file (`.ske`, `.hdr`, `.exr`, or an ordinary picture).
pub fn decode(bytes: &[u8]) -> Result<(u32, u32, Vec<[f32; 3]>), String> {
    let owned;
    let bytes = if is_zip(bytes) {
        owned = read_ske(bytes)?.image;
        &owned[..]
    } else {
        bytes
    };
    let img = crate::photo_img::decode(bytes)?;
    let hdr = matches!(img, image::DynamicImage::ImageRgb32F(_) | image::DynamicImage::ImageRgba32F(_));
    let rgb = img.to_rgb32f();
    let (w, h) = rgb.dimensions();
    let lin = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    let px = rgb
        .pixels()
        .map(|p| {
            let c = p.0.map(|c| if c.is_finite() { c.max(0.0) } else { 0.0 });
            if hdr { c } else { c.map(lin) }
        })
        .collect();
    Ok((w, h, px))
}

/// Packs a linear colour in the shared-exponent format of `Rgb9e5Ufloat`.
pub fn rgb9e5(c: [f32; 3]) -> u32 {
    const MAX: f32 = 65408.0;
    let c = c.map(|x| if x.is_finite() { x.clamp(0.0, MAX) } else { 0.0 });
    let maxc = c[0].max(c[1]).max(c[2]);
    if maxc < 1.0 / (1u32 << 24) as f32 {
        return 0;
    }
    let mut e = (maxc.log2().floor() as i32).max(-16) + 1 + 15;
    let mut denom = 2f32.powi(e - 15 - 9);
    if (maxc / denom + 0.5).floor() as i32 == 512 {
        denom *= 2.0;
        e += 1;
    }
    let m = c.map(|x| ((x / denom + 0.5).floor() as u32).min(511));
    m[0] | m[1] << 9 | m[2] << 18 | (e as u32) << 27
}

#[cfg(test)]
pub fn from_rgb9e5(v: u32) -> [f32; 3] {
    let e = (v >> 27) as i32 - 15 - 9;
    let s = 2f32.powi(e);
    [(v & 511) as f32 * s, ((v >> 9) & 511) as f32 * s, ((v >> 18) & 511) as f32 * s]
}

/// Halves a level (2×2 average; an odd or single row or column is kept whole).
fn half(w: u32, h: u32, px: &[[f32; 3]]) -> (u32, u32, Vec<[f32; 3]>) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = Vec::with_capacity((nw * nh) as usize);
    for y in 0..nh {
        for x in 0..nw {
            let mut s = [0.0f32; 3];
            let mut n = 0.0;
            for dy in 0..2u32.min(h) {
                for dx in 0..2u32.min(w) {
                    let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
                    let p = px[(sy * w + sx) as usize];
                    for i in 0..3 {
                        s[i] += p[i];
                    }
                    n += 1.0;
                }
            }
            out.push(s.map(|v| v / n));
        }
    }
    (nw, nh, out)
}

/// Resamples a panorama to `nw` × `nh` (area average when shrinking).
fn resample(w: u32, h: u32, px: &[[f32; 3]], nw: u32, nh: u32) -> Vec<[f32; 3]> {
    if (w, h) == (nw, nh) {
        return px.to_vec();
    }
    let (sx, sy) = (w as f64 / nw as f64, h as f64 / nh as f64);
    let mut out = Vec::with_capacity((nw * nh) as usize);
    for y in 0..nh {
        let (y0, y1) = ((y as f64 * sy) as u32, (((y + 1) as f64 * sy).ceil() as u32).clamp(1, h));
        for x in 0..nw {
            let (x0, x1) = ((x as f64 * sx) as u32, (((x + 1) as f64 * sx).ceil() as u32).clamp(1, w));
            let mut s = [0.0f32; 3];
            let mut n = 0.0f32;
            for yy in y0.min(h - 1)..y1.max(y0 + 1).min(h) {
                for xx in x0.min(w - 1)..x1.max(x0 + 1).min(w) {
                    let p = px[(yy * w + xx) as usize];
                    for i in 0..3 {
                        s[i] += p[i];
                    }
                    n += 1.0;
                }
            }
            out.push(s.map(|v| v / n.max(1.0)));
        }
    }
    out
}

/// The GPU levels and the thumbnail of a panorama.
pub fn prepare(w: u32, h: u32, px: &[[f32; 3]]) -> EnvImage {
    // The longer side within MAX_WIDTH (a panorama is 2:1, but a file may say anything).
    let ratio = |a: u32, b: u32, n: u32| ((a as u64 * n as u64) / b.max(1) as u64).max(1) as u32;
    let (nw, nh) = if w >= h {
        let nw = w.clamp(1, MAX_WIDTH);
        (nw, ratio(h, w, nw))
    } else {
        let nh = h.clamp(1, MAX_WIDTH);
        (ratio(w, h, nh), nh)
    };
    let mut cur = (nw, nh, resample(w, h, px, nw, nh));
    let mut levels = Vec::new();
    let mut thumb_src = None;
    loop {
        let (lw, lh, ref p) = cur;
        if thumb_src.is_none() && lw <= 256 {
            thumb_src = Some(cur.clone());
        }
        levels.push((lw, lh, p.iter().map(|c| rgb9e5(*c)).collect()));
        if lw == 1 && lh == 1 {
            break;
        }
        cur = half(lw, lh, p);
    }
    let (tw, th, tp) = thumb_src.unwrap_or_else(|| (nw, nh, resample(w, h, px, nw, nh)));
    let enc = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let s = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0 + 0.5) as u8
    };
    let thumb = tp.iter().flat_map(|c| [enc(c[0]), enc(c[1]), enc(c[2]), 255]).collect();
    EnvImage { levels, thumb: (tw, th, thumb) }
}

/// Where an environment's image comes from.
#[derive(Clone, Debug)]
pub enum Source {
    File(Blob),
    /// A path the model names (where the panorama was on its author's computer): read only
    /// when it is a local path (`esq_core::untrusted::read_referenced`), never a network
    /// one — Windows would send the user's NTLM hash to the server it names.
    Path(String),
    /// A file of a library folder of Preferences or of Esquisse's content, which the user
    /// chose.
    Library(String),
}

impl Source {
    pub(crate) fn key(&self) -> String {
        match self {
            Source::File(b) => format!("blob:{:p}:{}", Arc::as_ptr(&b.0), b.0.len()),
            // A path written in the model: read only when local (`safe_path`, the rule of
            // `esq_core::untrusted`).
            Source::Path(p) => format!("path:{p}:{:?}", crate::safe_path::modified(p)),
            Source::Library(p) => {
                let t = std::fs::metadata(p).and_then(|m| m.modified()).ok();
                format!("path:{p}:{t:?}")
            }
        }
    }

    pub(crate) fn bytes(&self) -> Result<Vec<u8>, String> {
        match self {
            Source::File(b) => Ok(b.0.to_vec()),
            Source::Path(p) => crate::safe_path::read(p).ok_or_else(|| format!("{p} : fichier illisible")),
            Source::Library(p) => std::fs::read(p).map_err(|e| format!("{p}: {e}")),
        }
    }
}

/// The image of environment `e`: the file in the model, else its path.
pub fn source(m: &Model, e: &Env) -> Option<Source> {
    if let Some(b) = m.files.get(&file_key(e.id)) {
        return Some(Source::File(b.clone()));
    }
    (!e.path.is_empty()).then(|| Source::Path(e.path.clone()))
}

enum Slot {
    Loading,
    Ready(Arc<EnvImage>),
    Failed,
}

fn cache() -> &'static Mutex<HashMap<String, Slot>> {
    static C: OnceLock<Mutex<HashMap<String, Slot>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

fn load(src: &Source) -> Option<Arc<EnvImage>> {
    let bytes = src.bytes().ok()?;
    let (w, h, px) = decode(&bytes).ok()?;
    Some(Arc::new(prepare(w, h, &px)))
}

/// The prepared image of a source: at once when `wait`, else decoded in the background
/// (`None` until it is ready; `pending` tells).
pub fn image(src: &Source, wait: bool) -> (Option<Arc<EnvImage>>, bool) {
    let key = src.key();
    {
        let c = cache().lock().unwrap();
        match c.get(&key) {
            Some(Slot::Ready(i)) => return (Some(i.clone()), false),
            Some(Slot::Failed) => return (None, false),
            Some(Slot::Loading) if !wait => return (None, true),
            _ => {}
        }
    }
    if wait {
        let img = load(src);
        cache().lock().unwrap().insert(key, img.clone().map_or(Slot::Failed, Slot::Ready));
        return (img, false);
    }
    cache().lock().unwrap().insert(key.clone(), Slot::Loading);
    let src = src.clone();
    std::thread::spawn(move || {
        let img = load(&src);
        cache().lock().unwrap().insert(key, img.map_or(Slot::Failed, Slot::Ready));
    });
    (None, true)
}

/// A small picture of an environment, RGBA.
pub type Thumb = (u32, u32, Vec<u8>);

fn thumbs() -> &'static Mutex<HashMap<String, Option<Arc<Thumb>>>> {
    static C: OnceLock<Mutex<HashMap<String, Option<Arc<Thumb>>>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

fn make_thumb(src: &Source) -> Option<Arc<Thumb>> {
    let bytes = src.bytes().ok()?;
    if is_zip(&bytes)
        && let Some(png) = read_ske(&bytes).ok().and_then(|f| f.thumbnail)
        && let Ok(img) = crate::photo_img::decode(&png)
    {
        let rgba = img.to_rgba8();
        return Some(Arc::new((rgba.width(), rgba.height(), rgba.into_raw())));
    }
    let (w, h, px) = decode(&bytes).ok()?;
    Some(Arc::new(prepare(w, h, &px).thumb))
}

/// The picture of an environment: a `.ske`'s own thumbnail, else its panorama made small.
/// Made in the background (`None` meanwhile, `pending` tells).
pub fn thumbnail(src: &Source) -> (Option<Arc<Thumb>>, bool) {
    let key = src.key();
    if let Some(t) = thumbs().lock().unwrap().get(&key) {
        return (t.clone(), false);
    }
    static LOADING: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    let loading = LOADING.get_or_init(Default::default);
    if loading.lock().unwrap().insert(key.clone()) {
        let src = src.clone();
        std::thread::spawn(move || {
            let t = make_thumb(&src);
            thumbs().lock().unwrap().insert(key, t);
        });
    }
    (None, true)
}

/// What the renderer needs of the current environment.
#[derive(Clone, Debug)]
pub struct EnvView {
    pub image: Arc<EnvImage>,
    pub skydome: bool,
    pub reflections: bool,
    pub rotation: f64,
    pub skydome_exposure: f32,
    pub reflection_exposure: f32,
}

/// The current environment as the view shows it, and whether its image is still loading.
pub fn active(m: &Model, wait: bool) -> (Option<EnvView>, bool) {
    let Some(e) = current(m) else { return (None, false) };
    if !e.skydome && !e.reflections {
        return (None, false);
    }
    let Some(src) = source(m, &e) else { return (None, false) };
    let (img, pending) = image(&src, wait);
    let view = img.map(|image| EnvView {
        image,
        skydome: e.skydome,
        reflections: e.reflections,
        rotation: e.rotation,
        skydome_exposure: e.skydome_exposure as f32,
        reflection_exposure: e.reflection_exposure as f32,
    });
    (view, pending)
}

// ------------------------------------------------------------------------------ directions

/// The direction of panorama point `u` (0–1 around), `elevation` (radians above the
/// horizon) for an environment turned by `rotation` degrees (`EnvironmentManager`'s sun).
pub fn direction(u: f64, elevation: f64, rotation: f64) -> DVec3 {
    let a = rotation.to_radians() - u * std::f64::consts::TAU;
    DVec3::new(-a.cos() * elevation.cos(), -a.sin() * elevation.cos(), elevation.sin())
}

/// The panorama point (u, v: 0 at the zenith, 1 at the nadir) seen in direction `d` (what
/// the shader's `env_at` samples).
#[cfg(test)]
pub fn image_uv(d: DVec3, rotation: f64) -> esq_core::glam::DVec2 {
    let d = d.normalize_or(DVec3::Z);
    let el = d.z.clamp(-1.0, 1.0).asin();
    let a = (-d.y).atan2(-d.x);
    let u = ((rotation.to_radians() - a) / std::f64::consts::TAU).rem_euclid(1.0);
    esq_core::glam::DVec2::new(u, 0.5 - el / std::f64::consts::PI)
}

/// The direction towards the sun linked to the current environment, if it is.
pub fn linked_sun(m: &Model) -> Option<DVec3> {
    let e = current(m).filter(|e| e.linked_sun)?;
    Some(direction(e.sun_pos[0] / 360.0, e.sun_pos[1].to_radians(), e.rotation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb9e5_keeps_high_dynamic_range_colours() {
        for c in [[0.0, 0.0, 0.0], [1.0, 0.5, 0.25], [120.0, 3.0, 0.01], [0.001, 0.002, 0.0005]] {
            let back = from_rgb9e5(rgb9e5(c));
            let m = c[0].max(c[1]).max(c[2]);
            for i in 0..3 {
                assert!((back[i] - c[i]).abs() <= m / 256.0 + 1e-9, "{c:?} → {back:?}");
            }
        }
    }

    #[test]
    fn directions_and_panorama_points_match() {
        for rot in [0.0, 37.0, 300.0] {
            for (u, el) in [(0.1, 0.3), (0.5, -0.2), (0.9, 1.2), (0.25, 0.0)] {
                let d = direction(u, el, rot);
                let uv = image_uv(d, rot);
                assert!((uv.x - u).abs() < 1e-9 && (uv.y - (0.5 - el / std::f64::consts::PI)).abs() < 1e-9);
            }
        }
        // The left edge of an unturned panorama faces the red axis' negative side, a quarter
        // turn further the green axis' positive side; turning the environment turns it.
        assert!(direction(0.0, 0.0, 0.0).distance(-DVec3::X) < 1e-12);
        assert!(direction(0.25, 0.0, 0.0).distance(DVec3::Y) < 1e-12);
        assert!(direction(0.0, 0.0, 90.0).distance(-DVec3::Y) < 1e-12);
        assert!(direction(0.3, std::f64::consts::FRAC_PI_2, 10.0).distance(DVec3::Z) < 1e-12);
    }

    #[test]
    fn the_list_is_the_ruby_one_and_files_go_with_the_model() {
        let mut m = Model::new();
        let a = add(&mut m, Env::new("Ciel"), Some(Blob(Arc::new(vec![1, 2, 3]))));
        let b = add(&mut m, Env::new("Ciel"), None);
        assert_eq!(list(&m).iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Ciel", "Ciel 1"]);
        assert!(m.files.contains_key(&file_key(a)));
        set_current(&mut m, Some(b));
        let mut e = current(&m).unwrap();
        assert_eq!(e.id, b);
        e.rotation = 45.0;
        e.skydome = false;
        assert!(update(&mut m, &e));
        assert_eq!(current(&m).unwrap(), e);
        // Purging keeps the current one only (no scene uses the other).
        assert_eq!(purge_unused(&mut m), 1);
        assert!(!m.files.contains_key(&file_key(a)));
        remove(&mut m, b);
        assert!(current(&m).is_none() && list(&m).is_empty());
    }

    /// A model whose environment names its panorama by a network path, without the file:
    /// showing it read `\\serveur\…` at once (metadata then read), and Windows sent the
    /// user's NTLM hash to that server. Now the path is refused without any access (the
    /// journal of `esq_core::untrusted` shows it refused and never opened).
    #[test]
    fn a_network_path_in_a_model_is_never_opened() {
        for unc in [
            format!(r"\\esq-test-env-{}\partage\ciel.hdr", std::process::id()),
            format!("//esq-test-env2-{}/partage/ciel.hdr", std::process::id()),
            format!(r"\\?\UNC\esq-test-env3-{}\partage\ciel.hdr", std::process::id()),
        ] {
            let mut m = Model::new();
            let mut e = Env::new("Ciel");
            e.skydome = true;
            e.reflections = true;
            e.path = unc.clone();
            let id = add(&mut m, e, None);
            set_current(&mut m, Some(id));
            let src = source(&m, &current(&m).unwrap()).unwrap();
            assert!(matches!(src, Source::Path(_)));
            assert!(src.bytes().is_err());
            let _ = src.key();
            assert!(active(&m, true).0.is_none());
            assert!(esq_core::untrusted::refused_paths().contains(&unc), "{unc}");
            assert!(!esq_core::untrusted::opened_paths().contains(&unc), "{unc}");
        }
    }

    #[test]
    fn panoramas_shrink_to_levels_and_a_thumbnail() {
        let (w, h) = (64, 32);
        let px: Vec<[f32; 3]> =
            (0..w * h).map(|i| if (i % w) < w / 2 { [4.0, 0.0, 0.0] } else { [0.0, 0.0, 0.5] }).collect();
        let img = prepare(w, h, &px);
        assert_eq!(img.levels[0].0, 64);
        assert_eq!(img.levels.last().map(|l| (l.0, l.1)), Some((1, 1)));
        // The last level is the average.
        let avg = from_rgb9e5(img.levels.last().unwrap().2[0]);
        assert!((avg[0] - 2.0).abs() < 0.02 && (avg[2] - 0.25).abs() < 0.01, "{avg:?}");
        // The thumbnail is clipped to white at most.
        assert_eq!(&img.thumb.2[0..4], &[255, 0, 0, 255]);
    }

    /// The sun position a SketchUp environment file gives is where its panorama is the
    /// brightest: `u` = the turn fraction, the elevation = the quarter-turn fraction.
    #[test]
    fn sketchup_sun_positions_point_at_the_sun_of_the_panorama() {
        for name in ["Sky - Mid-day 2K IBL", "Sky - At Sea 2K IBL", "Sky - Partial Clouds 2K IBL"] {
            let path = std::path::Path::new(SKETCHUP_LIBRARY).join(format!("{name}.ske"));
            let Ok(bytes) = std::fs::read(&path) else { return eprintln!("SketchUp absent : test sauté") };
            let ske = read_ske(&bytes).unwrap();
            let (w, h, px) = decode(&bytes).unwrap();
            let sum = |c: &[f32; 3]| c[0] + c[1] + c[2];
            let i = (0..px.len()).max_by(|&a, &b| sum(&px[a]).total_cmp(&sum(&px[b]))).unwrap();
            let (u, v) = ((i as u32 % w) as f64 / w as f64, (i as u32 / w) as f64 / h as f64);
            assert!((u - ske.sun_pos[0] / 360.0).abs() < 0.02, "{name}: u {u}");
            assert!(((0.5 - v) * 180.0 - ske.sun_pos[1]).abs() < 3.0, "{name}: v {v}");
        }
    }
    #[test]
    fn the_environments_that_come_with_esquisse_are_listed_by_their_own_names() {
        let list = read_folder(&shipped_dir().expect("environnements livrés avec Esquisse"));
        assert_eq!(list.len(), 18);
        // The name inside the file, not the file's (ASCII) name.
        assert!(
            list.iter().any(|e| e.name == "Ciel d'orage"),
            "{:?}",
            list.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
        let (env, _) = from_file(&list[0].path).unwrap();
        assert!(env.skydome && env.reflections && !env.name.is_empty());
    }

    #[test]
    fn reads_sketchups_environment_files() {
        let Some(first) = read_folder(std::path::Path::new(SKETCHUP_LIBRARY)).into_iter().next() else {
            return eprintln!("SketchUp absent : test sauté");
        };
        let bytes = std::fs::read(&first.path).unwrap();
        let ske = read_ske(&bytes).unwrap();
        assert!(!ske.name.is_empty());
        assert!(ske.skydome && ske.reflections);
        assert!(ske.thumbnail.is_some());
        let (w, h, px) = decode(&bytes).unwrap();
        assert_eq!(w, 2 * h);
        assert_eq!(px.len(), (w * h) as usize);
        // An HDR panorama: some light is brighter than white.
        assert!(px.iter().any(|c| c[0] > 1.5));
    }

    /// A file may give any shape: the longer side goes to the GPU within MAX_WIDTH, a tall
    /// picture included (its height used to stay whole).
    #[test]
    fn a_panorama_of_any_shape_fits_the_gpu() {
        for (w, h) in [(10u32, 5000u32), (5000, 10), (4096, 2048), (300, 150)] {
            let img = prepare(w, h, &vec![[0.5f32; 3]; (w * h) as usize]);
            let (lw, lh, _) = img.levels[0];
            assert!(lw <= MAX_WIDTH && lh <= MAX_WIDTH, "{w}×{h} → {lw}×{lh}");
            assert_eq!((lw >= lh), (w >= h), "{w}×{h} → {lw}×{lh}");
        }
        assert_eq!(prepare(4096, 2048, &vec![[0.0f32; 3]; 4096 * 2048]).levels[0].0, MAX_WIDTH);
        assert_eq!(prepare(300, 150, &vec![[0.0f32; 3]; 300 * 150]).levels[0], (300, 150, vec![0; 300 * 150]));
    }
}
