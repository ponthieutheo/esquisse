//! `host.view.*` : ce que Ruby voit de la fenêtre 3D (caméra, projection, pointage, dessin,
//! options de rendu qui sont des réglages de l'application).
//!
//! Toutes les longueurs franchissent la frontière en millimètres (le pont Ruby les convertit
//! en pouces), sauf les champs marqués « pouces » ci-dessous.

use eframe::egui;
use esq_core::glam::{DVec2, DVec3};
use esq_core::model::{EntityRef, Model};
use serde_json::{Value as J, json};
use slotmap::Key;

use crate::app::EsquisseApp;
use crate::camera::Camera;
use crate::pick::{Hit, InferCx, PickCache, SnapKind};
use crate::style::ViewStyle;

const INCH: f64 = 25.4;

/// Un dessin demandé par un script Ruby (`View#draw`…), repeint à chaque image.
#[derive(Clone, Debug)]
pub enum DrawCmd {
    /// `uvs` and `texture`: a textured surface (`View#draw(…, texture:, uvs:)`).
    Poly {
        mode: i64,
        pts: Vec<DVec3>,
        color: [u8; 4],
        width: f32,
        two_d: bool,
        uvs: Option<Vec<[f32; 2]>>,
        texture: Option<i64>,
    },
    Points {
        pts: Vec<DVec3>,
        size: f32,
        kind: i64,
        color: [u8; 4],
    },
    Text {
        pt: DVec3,
        text: String,
        color: [u8; 4],
        size: f32,
    },
}

pub struct ImageJob {
    pub path: std::path::PathBuf,
    pub size: [u32; 2],
    pub transparent: bool,
    pub antialias: bool,
    /// `:compression` (0..1): the JPEG quality.
    pub compression: f64,
    /// `:scale_factor`: the width of the lines.
    pub scale: f64,
}

/// État de la vue que seuls les scripts Ruby alimentent.
#[derive(Default)]
pub struct ViewState {
    pub draw: Vec<DrawCmd>,
    pub tooltip: Option<String>,
    pub images: Vec<ImageJob>,
    /// `View#load_texture`: pixels by id, sent to egui at the first frame that draws them.
    pub textures: std::collections::BTreeMap<i64, ViewTexture>,
    pub next_texture: i64,
}

/// A texture of a script (RGBA rows, top first).
pub struct ViewTexture {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
    handle: std::cell::RefCell<Option<egui::TextureHandle>>,
}

impl ViewTexture {
    /// The pixels of an `ImageRep` (`w`, `h`, `bpp`, `pad`, `data` in base64; BGR(A) rows from
    /// the top, each followed by `pad` bytes, as `ImageRep#row_padding` counts them).
    fn from_rep(a: &J) -> Option<ViewTexture> {
        let (w, h) = (a["w"].as_u64()? as usize, a["h"].as_u64()? as usize);
        let px = (a["bpp"].as_u64().unwrap_or(32) / 8) as usize;
        let pad = a["pad"].as_u64().unwrap_or(0) as usize;
        let data = esq_core::photo::base64_decode(a["data"].as_str()?)?;
        let row = w * px + pad;
        if w == 0 || h == 0 || px == 0 || data.len() < row * h {
            return None;
        }
        let mut rgba = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let o = y * row + x * px;
                let (b, g, r) = if px >= 3 { (data[o], data[o + 1], data[o + 2]) } else { (data[o], data[o], data[o]) };
                rgba.extend([r, g, b, if px == 4 { data[o + 3] } else { 255 }]);
            }
        }
        Some(ViewTexture { size: [w, h], rgba, handle: Default::default() })
    }

    /// The egui texture (repeated beyond 0..1, like SketchUp's).
    fn id(&self, ctx: &egui::Context, key: i64) -> egui::TextureId {
        let mut h = self.handle.borrow_mut();
        h.get_or_insert_with(|| {
            let opts =
                egui::TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..egui::TextureOptions::LINEAR };
            crate::gpu_img::load_texture(ctx, format!("ruby-texture-{key}"), self.size, &self.rgba, opts)
        })
        .id()
    }
}

/// Ce dont les opérations ont besoin : la caméra et les réglages d'affichage de l'application.
pub struct ViewCx<'a> {
    pub cam: &'a mut Camera,
    pub style: &'a mut ViewStyle,
    pub active_scene: &'a mut Option<usize>,
    pub anim: (f64, f64),
    pub vs: &'a mut ViewState,
}

#[allow(clippy::ptr_arg)]
pub fn call(
    app: &mut EsquisseApp,
    model: &mut Model,
    _sel: &mut Vec<EntityRef>,
    op: &str,
    args: &J,
) -> Result<J, String> {
    let mut cx = ViewCx {
        cam: &mut app.cam,
        style: &mut app.style,
        active_scene: &mut app.active_scene,
        anim: (app.anim.transition, app.anim.delay),
        vs: &mut app.ui_state.extra.view,
    };
    cx.call(model, op, args)
}

fn v3(j: &J) -> Option<DVec3> {
    let a = j.as_array().filter(|a| a.len() >= 3)?;
    Some(DVec3::new(a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?))
}

fn out(p: DVec3) -> J {
    json!([p.x, p.y, p.z])
}

fn color(j: &J) -> [u8; 4] {
    let a = j.as_array();
    let g = |i: usize, d: u8| a.and_then(|a| a.get(i)).and_then(J::as_u64).map_or(d, |v| v.min(255) as u8);
    [g(0, 0), g(1, 0), g(2, 0), g(3, 255)]
}

fn kind_name(k: SnapKind) -> &'static str {
    match k {
        SnapKind::Endpoint => "endpoint",
        SnapKind::Midpoint => "midpoint",
        SnapKind::OnEdge => "on_edge",
        SnapKind::OnFace => "on_face",
        SnapKind::Origin => "origin",
        SnapKind::GuidePoint => "guide_point",
        SnapKind::Intersection => "intersection",
        SnapKind::OnGuide => "on_guide",
        SnapKind::Axis(_) => "axis",
        SnapKind::Parallel => "parallel",
        SnapKind::Perpendicular => "perpendicular",
        SnapKind::GridPoint => "grid_point",
        SnapKind::Handle(..) => "grip",
        SnapKind::Ground => "ground",
        SnapKind::Free => "free",
    }
}

/// Degrés de liberté de l'inférence : 0 point fixe, 1 sur une ligne, 2 sur une surface, 3 libre.
fn dof(k: SnapKind) -> i64 {
    match k {
        SnapKind::Endpoint
        | SnapKind::Midpoint
        | SnapKind::Origin
        | SnapKind::GuidePoint
        | SnapKind::Intersection
        | SnapKind::GridPoint
        | SnapKind::Handle(..) => 0,
        SnapKind::OnEdge | SnapKind::OnGuide | SnapKind::Axis(_) | SnapKind::Parallel | SnapKind::Perpendicular => 1,
        SnapKind::OnFace | SnapKind::Ground => 2,
        SnapKind::Free => 3,
    }
}

fn hit_json(h: &Hit) -> Option<J> {
    let (kind, id) = match h.item {
        EntityRef::Edge(e) => ('e', e.data().as_ffi()),
        EntityRef::Face(f) => ('f', f.data().as_ffi()),
        EntityRef::Instance(i) => ('i', i.data().as_ffi()),
        _ => return None,
    };
    Some(
        json!({"path": h.path.iter().map(|i| i.data().as_ffi()).collect::<Vec<_>>(), "kind": kind.to_string(), "id": id}),
    )
}

impl ViewCx<'_> {
    pub fn call(&mut self, model: &mut Model, op: &str, a: &J) -> Result<J, String> {
        let name = op.strip_prefix("view.").ok_or_else(|| format!("opération hôte inconnue: {op}"))?;
        Ok(match name {
            "size" => {
                let (w, h) = (self.cam.size.x, self.cam.size.y);
                json!({"w": w, "h": h, "dw": w, "dh": h})
            }
            "camera_get" => self.camera_get(),
            "camera_set" => {
                self.camera_set(a)?;
                J::Null
            }
            "zoom_extents" => {
                let (min, max) = crate::scene::model_bounds(model);
                self.cam.zoom_extents(min, max);
                J::Null
            }
            "zoom_box" => {
                let (lo, hi) = (v3(&a["min"]).ok_or("min attendu")? * INCH, v3(&a["max"]).ok_or("max attendu")? * INCH);
                self.cam.zoom_extents(lo, hi);
                J::Null
            }
            "zoom" => {
                let f = a["factor"].as_f64().filter(|f| *f > 0.0).ok_or("facteur attendu")?;
                self.cam.distance = (self.cam.distance / f).clamp(1.0, 1e9);
                J::Null
            }
            "screen_coords" => {
                let p = v3(&a["pt"]).ok_or("point attendu")? * INCH;
                match self.cam.project(p) {
                    None => J::Null,
                    Some(s) => {
                        let depth = (p - self.cam.eye()).dot(self.cam.forward()).max(1e-6);
                        let near = (self.cam.distance * 1e-3).clamp(0.5, 100.0);
                        json!([s.x, s.y, (1.0 - near / depth).clamp(0.0, 1.0)])
                    }
                }
            }
            "pickray" => {
                let (o, d) = self.cam.ray(DVec2::new(a["x"].as_f64().unwrap_or(0.0), a["y"].as_f64().unwrap_or(0.0)));
                json!({"origin": out(o / INCH), "dir": out(d)})
            }
            "pixel_size" => {
                let p = v3(&a["pt"]).ok_or("point attendu")? * INCH;
                json!(self.cam.pixel_size_at(p) * a["px"].as_f64().unwrap_or(1.0) / INCH)
            }
            "guess_target" => self.guess_target(model, a),
            "infer_raw" => self.infer_raw(model, a),
            "pick_raw" => self.pick_raw(model, a),
            "pick_rect_raw" => self.pick_rect_raw(model, a),
            "ro_get" => self.ro_get(a["key"].as_str().unwrap_or("")),
            "ro_set" => self.ro_set(a["key"].as_str().unwrap_or(""), &a["value"]),
            "anim_settings" => json!({"transition": self.anim.0, "delay": self.anim.1}),
            // A `.style` file (Styles#add_style): its name, description, settings, watermarks.
            "style_file" => {
                let path = a["path"].as_str().ok_or("chemin attendu")?;
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                match crate::style_file::read(&bytes) {
                    Ok(f) => json!({
                        "name": f.name, "description": f.description, "ro": f.ro,
                        "wm": serde_json::to_value(&f.watermarks).unwrap_or(J::Null),
                    }),
                    Err(_) => J::Null,
                }
            }
            // The watermarks of a style that is selected.
            "watermarks_set" => {
                self.style.watermark_list = serde_json::from_value(a["wm"].clone()).unwrap_or_default();
                J::Null
            }
            "active_scene" => json!(*self.active_scene),
            "go_to_scene" => {
                self.go_to_scene(
                    model,
                    a["index"].as_u64().ok_or("index attendu")? as usize,
                    a["camera"].as_bool().unwrap_or(true),
                    a["layers"].as_bool().unwrap_or(true),
                )?;
                J::Null
            }
            "scene_added" => {
                let i = a["index"].as_u64().unwrap_or(0) as usize;
                // Comme SketchUp (« ShowPageTabs ») : ajouter une scène montre les onglets.
                self.style.scene_tabs = true;
                if let Some(s) = self.active_scene.as_mut()
                    && *s >= i
                {
                    *s += 1;
                }
                J::Null
            }
            "scene_removed" => {
                let i = a["index"].as_u64().unwrap_or(0) as usize;
                *self.active_scene = match *self.active_scene {
                    Some(s) if s == i => None,
                    Some(s) if s > i => Some(s - 1),
                    other => other,
                };
                J::Null
            }
            "draw" => {
                // draw2d points are screen pixels, not model inches.
                let two_d = a["two_d"].as_bool().unwrap_or(false);
                let k = if two_d { 1.0 } else { INCH };
                let pts: Vec<DVec3> = a["pts"].as_array().into_iter().flatten().filter_map(v3).map(|p| p * k).collect();
                let st = &a["style"];
                let uvs = a["uvs"].as_array().map(|v| {
                    v.iter()
                        .map(|uv| {
                            let c = |i: usize| uv.get(i).and_then(J::as_f64).unwrap_or(0.0) as f32;
                            [c(0), c(1)]
                        })
                        .collect()
                });
                self.vs.draw.push(DrawCmd::Poly {
                    mode: a["mode"].as_i64().unwrap_or(1),
                    pts,
                    color: color(&st["color"]),
                    width: st["width"].as_f64().unwrap_or(1.0) as f32,
                    two_d,
                    uvs,
                    texture: a["texture"].as_i64().filter(|t| self.vs.textures.contains_key(t)),
                });
                J::Null
            }
            "load_texture" => {
                let t = ViewTexture::from_rep(a).ok_or("ArgumentError: invalid ImageRep")?;
                self.vs.next_texture += 1;
                self.vs.textures.insert(self.vs.next_texture, t);
                json!(self.vs.next_texture)
            }
            "release_texture" => json!(self.vs.textures.remove(&a["id"].as_i64().unwrap_or(0)).is_some()),
            "draw_points" => {
                let pts: Vec<DVec3> =
                    a["pts"].as_array().into_iter().flatten().filter_map(v3).map(|p| p * INCH).collect();
                self.vs.draw.push(DrawCmd::Points {
                    pts,
                    size: a["size"].as_f64().unwrap_or(6.0) as f32,
                    kind: a["kind"].as_i64().unwrap_or(3),
                    color: color(&a["style"]["color"]),
                });
                J::Null
            }
            "draw_text" => {
                let o = &a["options"];
                let c = if o["color"].is_null() { &a["style"]["color"] } else { &o["color"] };
                // Like SketchUp's View#draw_text, the point is in screen pixels (from screen_coords).
                self.vs.draw.push(DrawCmd::Text {
                    pt: v3(&a["pt"]).ok_or("point attendu")?,
                    text: a["text"].as_str().unwrap_or("").to_string(),
                    color: color(c),
                    size: o["size"].as_f64().unwrap_or(12.0) as f32,
                });
                J::Null
            }
            "draw_clear" => {
                self.vs.draw.clear();
                J::Null
            }
            "tooltip" => {
                self.vs.tooltip = a["text"].as_str().map(str::to_string);
                J::Null
            }
            "write_image" => match image_job(a)? {
                Some(job) => {
                    self.vs.images.push(job);
                    json!(true)
                }
                None => json!(false),
            },
            // Pas de mesure de temps de rafraîchissement ici : Ruby retombe sur ses valeurs par défaut.
            "refresh_times" | "graphics_engine" => J::Null,
            "invalidate" => J::Null,
            _ => return Err(format!("opération hôte non implémentée: {op}")),
        })
    }

    // ---------------------------------------------------------------------- caméra

    /// La caméra de l'application vue comme une `Sketchup::Camera` (pouces, degrés).
    fn camera_get(&self) -> J {
        let c = &*self.cam;
        let up = c.up();
        let height = if c.perspective { 1.0 } else { 2.0 * c.distance * (c.fov_y * 0.5).tan() / INCH };
        json!({
            "eye": out(c.eye() / INCH), "target": out(c.target / INCH), "up": out(up),
            "persp": c.perspective, "fov": c.fov_y.to_degrees(), "height": height,
            "aspect": 0.0, "image_width": 36.0, "desc": "",
            "is2d": c.two_point, "c2d": [c.lens_shift.x, c.lens_shift.y], "s2d": 1.0,
        })
    }

    fn camera_set(&mut self, a: &J) -> Result<(), String> {
        let eye = v3(&a["eye"]).ok_or("œil attendu")? * INCH;
        let target = v3(&a["target"]).ok_or("cible attendue")? * INCH;
        let up = v3(&a["up"]).ok_or("vecteur haut attendu")?;
        let persp = a["persp"].as_bool().unwrap_or(true);
        let dist = eye.distance(target);
        let fov_deg = if persp {
            let f = a["fov"].as_f64().unwrap_or(35.0).clamp(1.0, 120.0);
            let aspect = a["aspect"].as_f64().unwrap_or(0.0);
            if aspect > 0.001 {
                // Champ horizontal : on en tire le champ vertical de la fenêtre.
                (2.0 * ((f.to_radians() * 0.5).tan() / aspect).atan()).to_degrees()
            } else {
                f
            }
        } else {
            let h = a["height"].as_f64().unwrap_or(1.0).max(0.001) * INCH;
            (2.0 * (h / (2.0 * dist.max(1e-6))).atan()).to_degrees()
        };
        let size = self.cam.size;
        self.cam.set_from_view(&esq_skp::View { eye, target, up, fov_deg, perspective: persp });
        self.cam.fov_y = fov_deg.to_radians().clamp(0.0175, 2.1);
        self.cam.size = size;
        self.cam.two_point = a["is2d"].as_bool().unwrap_or(false);
        let c2d = a["c2d"].as_array();
        self.cam.lens_shift = match c2d {
            Some(c) if self.cam.two_point && c.len() >= 2 => {
                DVec2::new(c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(0.0))
            }
            _ => DVec2::ZERO,
        };
        Ok(())
    }

    fn go_to_scene(&mut self, model: &mut Model, i: usize, camera: bool, layers: bool) -> Result<(), String> {
        let s = model.scenes.get(i).cloned().ok_or("scène inconnue")?;
        *self.active_scene = Some(i);
        if layers && let Some(hidden) = &s.hidden_layers {
            for (k, l) in model.layers.iter_mut().enumerate() {
                l.visible = !hidden.contains(&k);
            }
        }
        if camera {
            let size = self.cam.size;
            self.cam.set_from_view(&s.camera);
            self.cam.size = size;
        }
        Ok(())
    }

    // ---------------------------------------------------------------------- pointage

    fn guess_target(&self, model: &Model, a: &J) -> J {
        let m = match (a["x"].as_f64(), a["y"].as_f64()) {
            (Some(x), Some(y)) => DVec2::new(x, y),
            _ => self.cam.size * 0.5,
        };
        let pick = PickCache::build(model, self.style);
        let (o, d) = self.cam.ray(m);
        let p = match pick.ray_face(o, d) {
            Some((t, _, _)) => o + d * t,
            None => {
                let f = self.cam.forward();
                let den = d.dot(f);
                let t = if den.abs() > 1e-9 { (self.cam.target - o).dot(f) / den } else { self.cam.distance };
                o + d * t
            }
        };
        out(p / INCH)
    }

    fn infer_raw(&self, model: &Model, a: &J) -> J {
        let m = DVec2::new(a["x"].as_f64().unwrap_or(0.0), a["y"].as_f64().unwrap_or(0.0));
        let reference = v3(&a["ref"]);
        let pick = PickCache::build(model, self.style);
        let icx = InferCx { cam: &*self.cam, pick: &pick, axes: model.axes, recent: None };
        let s = icx.infer(m, reference, None, None);
        let face =
            s.face.as_ref().and_then(|(h, _)| (matches!(h.item, EntityRef::Face(_))).then(|| hit_json(h)).flatten());
        let edge =
            s.edge.as_ref().and_then(|(h, _)| (matches!(h.item, EntityRef::Edge(_))).then(|| hit_json(h)).flatten());
        json!({
            "pos": out(s.pos), "kind": kind_name(s.kind), "tip": s.kind.label(), "dof": dof(s.kind),
            "face": face, "edge": edge,
        })
    }

    fn pick_raw(&self, model: &Model, a: &J) -> J {
        let m = DVec2::new(a["x"].as_f64().unwrap_or(0.0), a["y"].as_f64().unwrap_or(0.0));
        let ap = a["aperture"].as_f64().filter(|v| *v > 0.0).unwrap_or(crate::pick::EDGE_PX);
        let pick = PickCache::build(model, self.style);
        let mut out = Vec::new();
        if let Some(e) = pick.screen_edge(&*self.cam, m, ap)
            && let Some(j) = hit_json(e.2)
        {
            out.push(j);
        }
        let (o, d) = self.cam.ray(m);
        if let Some((_, h, _)) = pick.ray_face(o, d)
            && let Some(j) = hit_json(h)
        {
            out.push(j);
        }
        J::Array(out)
    }

    fn pick_rect_raw(&self, model: &Model, a: &J) -> J {
        let g = |k: &str| {
            let v = a[k].as_array();
            DVec2::new(
                v.and_then(|v| v.first()).and_then(J::as_f64).unwrap_or(0.0),
                v.and_then(|v| v.get(1)).and_then(J::as_f64).unwrap_or(0.0),
            )
        };
        // The entities of the script's context (`ctx`: the instances of its edit path), an
        // instance as a whole.
        let ctx: Vec<esq_core::model::InstanceId> = a["ctx"]
            .as_array()
            .map(|l| {
                l.iter()
                    .filter_map(J::as_str)
                    .filter_map(|h| h.rsplit(':').next()?.parse::<u64>().ok())
                    .map(|id| slotmap::KeyData::from_ffi(id).into())
                    .collect()
            })
            .unwrap_or_default();
        let pick = PickCache::build(model, self.style);
        let items = pick.items_in_rect(&*self.cam, g("min"), g("max"), a["crossing"].as_bool().unwrap_or(false), &ctx);
        J::Array(items.into_iter().filter_map(|item| hit_json(&Hit { path: ctx.clone(), item })).collect())
    }

    // ---------------------------------------------------------------------- options de rendu

    /// Les options de rendu sont celles du style de l'application (`ViewStyle::ro_get`) : Ruby
    /// et l'interface partagent les mêmes réglages. Les autres restent dans le modèle.
    pub fn ro_get(&self, key: &str) -> J {
        match self.style.ro_get(key) {
            Some(v) => json!({"mapped": true, "value": v}),
            None => json!({"mapped": false}),
        }
    }

    pub fn ro_set(&mut self, key: &str, v: &J) -> J {
        match self.style.ro_set(key, v) {
            Some(ok) => json!({"mapped": true, "ok": ok}),
            None => json!({"mapped": false}),
        }
    }
}

// -------------------------------------------------------------------------- affichage

/// The triangles (point indices) of a surface mode of `View#draw`: triangles, strip, fan,
/// quads, quad strip, polygon.
fn surface_triangles(mode: i64, n: usize) -> Vec<[usize; 3]> {
    match mode {
        4 => (0..n / 3).map(|i| [3 * i, 3 * i + 1, 3 * i + 2]).collect(),
        5 => (0..n.saturating_sub(2)).map(|i| if i % 2 == 0 { [i, i + 1, i + 2] } else { [i + 1, i, i + 2] }).collect(),
        7 => (0..n / 4).flat_map(|i| [[4 * i, 4 * i + 1, 4 * i + 2], [4 * i, 4 * i + 2, 4 * i + 3]]).collect(),
        8 => (0..n.saturating_sub(2) / 2)
            .flat_map(|i| [[2 * i, 2 * i + 1, 2 * i + 3], [2 * i, 2 * i + 3, 2 * i + 2]])
            .collect(),
        _ => (1..n.saturating_sub(1)).map(|i| [0, i, i + 1]).collect(),
    }
}

/// Peint les dessins des scripts par-dessus la vue 3D (`origin` = coin haut-gauche de la vue).
pub fn paint(vs: &ViewState, painter: &egui::Painter, origin: egui::Pos2, cam: &Camera) {
    let col = |c: [u8; 4]| egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
    let screen = |p: DVec3, two_d: bool| -> Option<egui::Pos2> {
        let s = if two_d { DVec2::new(p.x, p.y) } else { cam.project(p)? };
        Some(origin + egui::vec2(s.x as f32, s.y as f32))
    };
    for cmd in &vs.draw {
        match cmd {
            DrawCmd::Poly { mode, pts, color, width, two_d, uvs, texture } => {
                let sp: Vec<Option<egui::Pos2>> = pts.iter().map(|p| screen(*p, *two_d)).collect();
                // A textured surface: triangles mapped by the uvs (SketchUp turns v upwards).
                if let (Some(uvs), Some(t)) = (uvs, texture.and_then(|t| vs.textures.get(&t).map(|x| (t, x))))
                    && *mode >= 4
                {
                    let mut mesh = egui::Mesh::with_texture(t.1.id(painter.ctx(), t.0));
                    for tri in surface_triangles(*mode, sp.len()) {
                        let v: Option<Vec<egui::Pos2>> = tri.iter().map(|&i| sp[i]).collect();
                        let Some(v) = v else { continue };
                        let base = mesh.vertices.len() as u32;
                        for (k, p) in v.into_iter().enumerate() {
                            let [u, w] = uvs[tri[k]];
                            mesh.vertices.push(egui::epaint::Vertex {
                                pos: p,
                                uv: egui::pos2(u, -w),
                                color: egui::Color32::WHITE,
                            });
                        }
                        mesh.add_triangle(base, base + 1, base + 2);
                    }
                    painter.add(egui::Shape::mesh(mesh));
                    continue;
                }
                let stroke = egui::Stroke::new(*width, col(*color));
                let line = |a: Option<egui::Pos2>, b: Option<egui::Pos2>| {
                    if let (Some(a), Some(b)) = (a, b) {
                        painter.line_segment([a, b], stroke);
                    }
                };
                match *mode {
                    0 => {
                        for p in sp.iter().flatten() {
                            painter.circle_filled(*p, width.max(2.0), col(*color));
                        }
                    }
                    1 => sp.as_chunks::<2>().0.iter().for_each(|c| line(c[0], c[1])),
                    2 | 3 => {
                        sp.windows(2).for_each(|w| line(w[0], w[1]));
                        if *mode == 2 && sp.len() > 2 {
                            line(sp[sp.len() - 1], sp[0]);
                        }
                    }
                    _ => {
                        // Surfaces : triangles, bandes, éventails, quadrilatères, polygone.
                        let poly = |ids: &[usize]| {
                            let v: Vec<egui::Pos2> = ids.iter().filter_map(|i| sp.get(*i).copied().flatten()).collect();
                            if v.len() == ids.len() && v.len() >= 3 {
                                painter.add(egui::Shape::convex_polygon(v, col(*color), egui::Stroke::NONE));
                            }
                        };
                        let n = sp.len();
                        match *mode {
                            4 => (0..n / 3).for_each(|i| poly(&[3 * i, 3 * i + 1, 3 * i + 2])),
                            5 => (0..n.saturating_sub(2)).for_each(|i| poly(&[i, i + 1, i + 2])),
                            6 => (1..n.saturating_sub(1)).for_each(|i| poly(&[0, i, i + 1])),
                            7 => (0..n / 4).for_each(|i| poly(&[4 * i, 4 * i + 1, 4 * i + 2, 4 * i + 3])),
                            8 => (0..n.saturating_sub(2) / 2)
                                .for_each(|i| poly(&[2 * i, 2 * i + 1, 2 * i + 3, 2 * i + 2])),
                            _ => poly(&(0..n).collect::<Vec<_>>()),
                        }
                    }
                }
            }
            DrawCmd::Points { pts, size, kind, color } => {
                for p in pts.iter().filter_map(|p| screen(*p, false)) {
                    let k = size / 2.0;
                    let c = col(*color);
                    let st = egui::Stroke::new(1.5, c);
                    match kind {
                        1 => {
                            let _ = painter.rect_stroke(
                                egui::Rect::from_center_size(p, egui::vec2(*size, *size)),
                                0.0,
                                st,
                                egui::StrokeKind::Middle,
                            );
                        }
                        2 => {
                            let _ =
                                painter.rect_filled(egui::Rect::from_center_size(p, egui::vec2(*size, *size)), 0.0, c);
                        }
                        4 => {
                            painter.line_segment([p + egui::vec2(-k, -k), p + egui::vec2(k, k)], st);
                            painter.line_segment([p + egui::vec2(-k, k), p + egui::vec2(k, -k)], st);
                        }
                        5 => {
                            painter.line_segment([p + egui::vec2(-k, 0.0), p + egui::vec2(k, 0.0)], st);
                            painter.line_segment([p + egui::vec2(0.0, -k), p + egui::vec2(0.0, k)], st);
                            painter.line_segment([p + egui::vec2(-k, -k), p + egui::vec2(k, k)], st);
                            painter.line_segment([p + egui::vec2(-k, k), p + egui::vec2(k, -k)], st);
                        }
                        6 | 7 => {
                            let tri = vec![p + egui::vec2(0.0, -k), p + egui::vec2(k, k), p + egui::vec2(-k, k)];
                            let fill = if *kind == 7 { c } else { egui::Color32::TRANSPARENT };
                            let _ = painter.add(egui::Shape::convex_polygon(tri, fill, st));
                        }
                        _ => {
                            painter.line_segment([p + egui::vec2(-k, 0.0), p + egui::vec2(k, 0.0)], st);
                            painter.line_segment([p + egui::vec2(0.0, -k), p + egui::vec2(0.0, k)], st);
                        }
                    }
                }
            }
            DrawCmd::Text { pt, text, color, size } => {
                if let Some(p) = screen(*pt, true) {
                    painter.text(p, egui::Align2::LEFT_TOP, text, egui::FontId::proportional(*size), col(*color));
                }
            }
        }
    }
    if let Some(t) = &vs.tooltip {
        painter.text(
            painter.clip_rect().left_bottom() + egui::vec2(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            t,
            egui::FontId::proportional(13.0),
            egui::Color32::BLACK,
        );
    }
}

/// `View#write_image`: SketchUp writes the picture before it returns (TestUp
/// test_write_image_*). With a GPU it is drawn at once from the script's `model` (edit path
/// `ctx`); without one it waits for the next frame that can draw.
pub fn write_image(
    app: &mut EsquisseApp,
    ctx: &[esq_core::model::InstanceId],
    model: &mut Model,
    sel: &mut Vec<EntityRef>,
    op: &str,
    args: &J,
) -> Result<J, String> {
    if app.gpu.is_none() {
        return call(app, model, sel, op, args);
    }
    let Some(job) = image_job(args)? else { return Ok(json!(false)) };
    let r = app.with_script_scene(model, ctx, sel, |app, rs| app.write_ruby_image(rs, &job));
    Ok(json!(r.is_ok()))
}

/// The picture `View#write_image` asks for; `None` for a format it does not write.
fn image_job(a: &J) -> Result<Option<ImageJob>, String> {
    let path = std::path::PathBuf::from(a["path"].as_str().ok_or("chemin attendu")?);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
        return Ok(None);
    }
    let w = a["w"].as_u64().unwrap_or(1) as u32;
    let h = a["h"].as_u64().unwrap_or(1) as u32;
    Ok(Some(ImageJob {
        path,
        size: [w, h],
        transparent: a["transparent"].as_bool().unwrap_or(false),
        antialias: a["antialias"].as_bool().unwrap_or(false),
        compression: a["compression"].as_f64().unwrap_or(1.0),
        scale: a["scale"].as_f64().unwrap_or(1.0),
    }))
}

impl EsquisseApp {
    /// Écrit les images demandées par `View#write_image` (rendu hors écran de l'état actuel).
    pub fn step_ruby_images(&mut self, rs: &eframe::egui_wgpu::RenderState) {
        let jobs = std::mem::take(&mut self.ui_state.extra.view.images);
        for job in jobs {
            let _ = self.write_ruby_image(rs, &job);
        }
    }

    /// One picture of `View#write_image`: what the view shows, at the asked size, rendered as
    /// File › Export › 2D Graphic renders (anti-aliasing, transparent background for a PNG,
    /// JPEG quality, line scale).
    fn write_ruby_image(&mut self, rs: &eframe::egui_wgpu::RenderState, job: &ImageJob) -> anyhow::Result<()> {
        use crate::export2d::{Kind, RasterJob, RasterOptions};
        let ext = job.path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let kind = if ext == "png" { Kind::Png } else { Kind::Jpeg };
        let o = RasterOptions {
            use_view_size: false,
            width: job.size[0],
            height: job.size[1],
            line_scale: job.scale,
            antialias: job.antialias,
            transparent: job.transparent,
            jpeg_quality: (job.compression * 100.0).round().clamp(1.0, 100.0) as u8,
        };
        self.render_raster(rs, RasterJob::new(job.path.clone(), kind, &o, job.size, job.scale))?;
        Ok(())
    }
}
