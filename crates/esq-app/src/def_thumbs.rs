//! Thumbnails of the model's components (Window › Components › In Model), as SketchUp makes
//! one for each definition (`CComponentUtils::CreateDefinitionThumbnailImage`,
//! `EnsureDefinitionHasThumbnailImageAsync`, `RenderingUtils::RenderComponent`): the
//! definition alone, seen in isometric view, rendered offscreen (`offscreen.rs`) with the
//! view's style, and kept until the definition changes.
//!
//! The panel asks ([`thumbnail`]): it gets the last picture made (possibly of an older state
//! of the definition, until the new one is ready) and the definition is queued when it has none
//! or changed. A few are rendered per frame ([`EsquisseApp::step_def_thumbnails`]). Whether a
//! definition changed is worked out once per version of the model, from what it holds and what
//! the definitions nested in it hold (each nested definition looked at once, iteratively).

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use esq_core::glam::{DMat4, DVec2};
use esq_core::model::{DefId, Model};

use crate::app::EsquisseApp;

/// Side of a thumbnail, in pixels (the tiles show 64 points).
pub const SIZE: u32 = 128;
/// Thumbnails rendered per displayed frame.
const PER_FRAME: usize = 2;

struct Entry {
    /// The fingerprint of the definition the picture shows.
    shown: u64,
    png: Arc<Vec<u8>>,
}

#[derive(Default)]
pub struct Cache {
    entries: HashMap<DefId, Entry>,
    /// Fingerprints of this version of the model (`rev`), worked out on demand.
    prints: HashMap<DefId, u64>,
    rev: u64,
    /// Definitions to render, in the order asked.
    queue: Vec<DefId>,
    /// A 3D view of its own (the window's keeps its mesh).
    view: Option<crate::render::Viewport3d>,
    /// Tests: how many thumbnails were rendered.
    pub rendered: usize,
}

/// Writes JSON into a hash (no buffer).
struct HashWriter<'a>(&'a mut std::collections::hash_map::DefaultHasher);

impl std::io::Write for HashWriter<'_> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.write(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// What a definition's picture depends on: its own content, the materials and tags of the
/// model (colours, visibility), and the fingerprints of the definitions nested in it (each
/// one worked out once, without recursion).
fn fingerprint(m: &Model, d: DefId, memo: &mut HashMap<DefId, u64>) -> u64 {
    if let Some(p) = memo.get(&d) {
        return *p;
    }
    // Children before parents: an explicit stack, a set against cycles.
    let mut stack = vec![(d, false)];
    let mut busy = HashSet::new();
    // How a material and a tag look (what the picture shows of them).
    let material = |h: &mut std::collections::hash_map::DefaultHasher, id: Option<esq_core::model::MaterialId>| {
        let Some(mat) = id.and_then(|id| m.materials.get(id)) else { return 0u8.hash(h) };
        (mat.color, mat.opacity.to_bits()).hash(h);
        mat.ext.texture.as_ref().map(|t| Arc::as_ptr(&t.image.0) as usize).hash(h);
    };
    let tag = |h: &mut std::collections::hash_map::DefaultHasher, l: usize| m.layers.get(l).map(|l| l.visible).hash(h);
    while let Some((x, expanded)) = stack.pop() {
        if memo.contains_key(&x) {
            continue;
        }
        let Some(def) = m.defs.get(x) else {
            memo.insert(x, 0);
            continue;
        };
        if expanded {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            let e = &def.entities;
            let _ = serde_json::to_writer(HashWriter(&mut h), &e.faces);
            let _ = serde_json::to_writer(HashWriter(&mut h), &e.edges);
            let _ = serde_json::to_writer(HashWriter(&mut h), &e.verts);
            for f in e.faces.values() {
                material(&mut h, f.front);
                material(&mut h, f.back);
                tag(&mut h, f.layer);
            }
            for ed in e.edges.values() {
                tag(&mut h, ed.layer);
            }
            for i in e.instances.values() {
                memo.get(&i.def).copied().unwrap_or(0).hash(&mut h);
                i.xform.to_cols_array().map(f64::to_bits).hash(&mut h);
                i.hidden.hash(&mut h);
                material(&mut h, i.material);
                tag(&mut h, i.layer);
            }
            busy.remove(&x);
            memo.insert(x, h.finish());
        } else if busy.insert(x) {
            stack.push((x, true));
            for i in def.entities.instances.values() {
                if !memo.contains_key(&i.def) && !busy.contains(&i.def) {
                    stack.push((i.def, false));
                }
            }
        }
    }
    memo.get(&d).copied().unwrap_or(0)
}

/// The model holding only definition `d`, once, at the origin (what the thumbnail shows).
pub fn alone(m: &Model, d: DefId) -> Model {
    let mut out = Model::new();
    let def = out.import_def(m, d, &mut esq_core::groups::ImportMap::default());
    out.root.instances.insert(esq_core::model::Instance {
        def,
        xform: DMat4::IDENTITY,
        name: String::new(),
        material: None,
        layer: 0,
        hidden: false,
        locked: false,
        attributes: Default::default(),
        ext: Default::default(),
    });
    out.layers = m.layers.clone();
    out
}

/// The camera of a thumbnail: SketchUp's isometric view, the definition filling the square.
pub fn camera(m: &Model) -> crate::camera::Camera {
    let mut cam = crate::camera::Camera { size: DVec2::splat(SIZE as f64), ..Default::default() };
    let (_, yaw, pitch) = crate::commands::STD_VIEWS[6];
    cam.set_view(yaw, pitch);
    let (min, max) = crate::scene::model_bounds(m);
    if min.is_finite() && max.is_finite() {
        cam.zoom_extents(min, max);
    }
    cam
}

impl Cache {
    /// The thumbnail of `d` for the panel (PNG), and its key (it changes with the picture).
    /// Queues a new rendering when there is none or the definition changed.
    pub fn get(&mut self, m: &Model, rev: u64, d: DefId) -> Option<(String, Arc<Vec<u8>>)> {
        if self.rev != rev {
            self.rev = rev;
            self.prints.clear();
            self.entries.retain(|k, _| m.defs.contains_key(*k));
        }
        let now = fingerprint(m, d, &mut self.prints);
        let stale = self.entries.get(&d).is_none_or(|e| e.shown != now);
        if stale && !self.queue.contains(&d) {
            self.queue.push(d);
        }
        self.entries.get(&d).map(|e| (format!("def{d:?}-{}", e.shown), e.png.clone()))
    }
}

/// Window › Components: the thumbnail of a definition of the model.
pub fn thumbnail(app: &mut EsquisseApp, d: DefId) -> Option<(String, Arc<Vec<u8>>)> {
    let rev = app.doc.model_rev;
    let m = &app.doc.model;
    app.ui_state.extra.def_thumbs.get(m, rev, d)
}

impl EsquisseApp {
    /// Renders the thumbnails the panel asked for, a few per frame.
    pub fn step_def_thumbnails(&mut self, rs: &eframe::egui_wgpu::RenderState) {
        let mut done = 0;
        while done < PER_FRAME {
            let Some(d) = (!self.ui_state.extra.def_thumbs.queue.is_empty())
                .then(|| self.ui_state.extra.def_thumbs.queue.remove(0))
            else {
                break;
            };
            if !self.doc.model.defs.contains_key(d) {
                continue;
            }
            let print = fingerprint(&self.doc.model, d, &mut self.ui_state.extra.def_thumbs.prints);
            let probe = alone(&self.doc.model, d);
            let cam = camera(&probe);
            // The view's style, without what is not the component (axes, ground, sky).
            let st = crate::style::ViewStyle { axes: false, ground: false, sky: false, ..self.style.clone() };
            let opts = crate::scene::SceneOptions {
                ctx: &[],
                ctx_def: None,
                sel: &[],
                axes_xform: DMat4::IDENTITY,
                style: &st,
            };
            let mesh = crate::scene::build(&probe, &opts);
            let v = self.ui_state.extra.def_thumbs.view.get_or_insert_with(|| crate::render::Viewport3d::new(rs));
            v.upload(rs, &mesh);
            v.render(rs, &cam, [SIZE, SIZE], 1.0, &st, &crate::render::Lighting::default());
            let Some((w, h, px)) = v.read_pixels(rs) else { break };
            let Some(img) = image::RgbaImage::from_raw(w, h, px) else { break };
            let mut png = Vec::new();
            if image::DynamicImage::ImageRgba8(img)
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .is_err()
            {
                break;
            }
            let c = &mut self.ui_state.extra.def_thumbs;
            c.entries.insert(d, Entry { shown: print, png: Arc::new(png) });
            c.rendered += 1;
            done += 1;
        }
    }
}
