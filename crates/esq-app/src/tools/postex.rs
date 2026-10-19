//! Texture › Position (SketchUp's `CPositionTextureTool`): pins tie points of the texture to
//! points of the face. Read from SketchUp.exe:
//!
//! - Four fixed pins (red, green, blue, yellow) sit at the corners of the texture tile under
//!   the point that was right-clicked (else the face's centre); when none of them is on
//!   screen, or the texture is distorted, at the corners of the face's box. Pins saved on the
//!   face by an earlier positioning come back.
//! - Dragging the red pin moves the texture, the green one scales and rotates it about the
//!   red one, the blue one scales and shears it (the red-green line stays), the yellow one
//!   distorts it (perspective). Free pins (Shift, or the menu's Fixed Pins unticked): only
//!   the dragged pin moves, the map is fitted to all pins. Dragging elsewhere on the texture
//!   moves it.
//! - A click on a pin (under 0.5 s and 5 pixels) lifts it: it then follows the cursor without
//!   changing the texture, until the next click puts it down.
//! - Pins snap to the model's points unless Ctrl is held. The arrows move the texture by one
//!   pixel (ten with Shift). A click outside the texture finishes.
//! - Right-click menu: Done, Reset, Flip (Left/Right, Up/Down), Rotate (90, 180, 270), Fixed
//!   Pins, Undo, Redo. Fixed Pins is remembered between sessions.
//! - The whole positioning is one operation, "Position Texture", committed when the tool
//!   ends; Escape cancels it.

use std::cell::RefCell;
use std::time::Instant;

use eframe::egui::{self, Color32, Pos2, Stroke};
use esq_core::glam::{DMat3, DMat4, DVec2, DVec3};
use esq_core::meta::{FaceTex, UvMap, side_uv};
use esq_core::model::{FaceId, InstanceId, MaterialId, Model};
use esq_core::texpos::{self, Frame, Pin};

use super::{Overlay, Tool, ToolCx, ToolId};
use crate::camera::Camera;
use crate::doc::Document;
use crate::pick::SnapKind;

/// Pin colours: red, green, blue, yellow; free pins are white.
const PIN_COLORS: [Color32; 4] = [
    Color32::from_rgb(220, 30, 30),
    Color32::from_rgb(30, 160, 50),
    Color32::from_rgb(40, 70, 230),
    Color32::from_rgb(235, 200, 20),
];

/// A press on a pin is a click (lifting it) when released within this time and distance.
const CLICK_SECONDS: f64 = 0.5;
const CLICK_PX: f64 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Idle,
    /// Pressed on a pin: a click lifts it, a drag moves it.
    Pending {
        pin: usize,
        at: DVec2,
        since: Instant,
    },
    DragPin(usize),
    /// Dragging the texture; last plane point.
    DragTexture(DVec2),
    /// A lifted pin follows the cursor.
    Lifted(usize),
}

pub struct PositionTextureTool {
    ctx: Vec<InstanceId>,
    face: FaceId,
    /// The side being positioned (the one the camera sees).
    back: bool,
    /// The other side shows the same material: it is positioned too.
    both: bool,
    tile: [f64; 2],
    material: MaterialId,
    frame: Frame,
    /// Context transform (local → world).
    x: DMat4,
    pins: Vec<Pin>,
    /// The face's texture positioning before the tool.
    original: Option<Box<FaceTex>>,
    changed: bool,
    pub fixed: bool,
    undo: Vec<Vec<Pin>>,
    redo: Vec<Vec<Pin>>,
    hover: Option<usize>,
    cursor: Option<DVec2>,
    mouse: DVec2,
    over: bool,
    state: State,
    prev: ToolId,
    switch: Option<ToolId>,
    /// Points of the face, on the plane.
    outline: Vec<DVec2>,
    image: RefCell<Option<egui::TextureHandle>>,
}

/// The material a face side shows for positioning, with whether the other side shares it.
pub fn side_material(
    model: &Model,
    ents: &esq_core::model::Entities,
    f: FaceId,
    back: bool,
) -> Option<(MaterialId, bool, bool)> {
    let face = ents.faces.get(f)?;
    let textured =
        |m: Option<MaterialId>| m.filter(|m| model.materials.get(*m).is_some_and(|x| x.ext.texture.is_some()));
    let (this, other) = if back { (face.back, face.front) } else { (face.front, face.back) };
    match (textured(this), textured(other)) {
        (Some(m), _) => Some((m, back, other == this || other.is_none())),
        // The seen side has no texture: the other one is positioned (SketchUp's choice too).
        (None, Some(m)) if this.is_none() => Some((m, !back, true)),
        _ => None,
    }
}

/// Whether the camera sees the back of a face of the active context.
pub fn back_seen(doc: &Document, cam: &Camera, f: FaceId) -> bool {
    let ents = doc.ents();
    let Some(face) = ents.faces.get(f) else { return false };
    let x = doc.ctx_xform();
    let p = x.transform_point3(ents.pos(face.outer[0]));
    let n = x.transform_vector3(face.normal);
    let (_, d, _) = cam.ray_to(p);
    d.dot(n) > 0.0
}

impl PositionTextureTool {
    /// Starts positioning the texture of face `f` (active context) on the side the camera
    /// sees. `reference`: the screen point the command came from (right-click), if any.
    pub fn start(doc: &Document, cam: &Camera, f: FaceId, reference: Option<DVec2>, prev: ToolId) -> Option<Self> {
        let model = &doc.model;
        let ents = doc.ents();
        let face = ents.faces.get(f)?;
        let (material, back, both) = side_material(model, ents, f, back_seen(doc, cam, f))?;
        let tile = model.materials.get(material)?.ext.texture.as_ref()?.size;
        let origin = ents.pos(face.outer[0]);
        let frame = Frame::new(origin, if back { -face.normal } else { face.normal });
        let x = doc.ctx_xform();
        let outline: Vec<DVec2> = face.outer.iter().map(|v| frame.to2(ents.pos(*v))).collect();
        let k = back as usize;
        let map = current_map(face, back, tile);
        let g = texpos::plane_matrix(&map, &frame, tile);
        let stored = face.tex.as_deref().map(|t| &t.pins[k]).filter(|p| p.len() == 4);
        let pins = match stored {
            Some(p) => p.iter().map(|(q, uv)| Pin { tex: *uv * DVec2::from(tile), at: frame.to2(*q) }).collect(),
            None => default_pins(&map, &g, &frame, tile, &outline, reference, cam, &x),
        };
        let mut tool = PositionTextureTool {
            ctx: doc.ctx.clone(),
            face: f,
            back,
            both,
            tile,
            material,
            frame,
            x,
            pins,
            original: face.tex.clone(),
            changed: false,
            fixed: fixed_pins_pref(),
            undo: Vec::new(),
            redo: Vec::new(),
            hover: None,
            cursor: None,
            mouse: DVec2::ZERO,
            over: false,
            state: State::Idle,
            prev,
            switch: None,
            outline,
            image: RefCell::new(None),
        };
        if tool.map().is_none() {
            tool.pins = texpos::box_pins(&g, &tool.outline);
        }
        Some(tool)
    }

    /// The map the pins give.
    pub fn map(&self) -> Option<UvMap> {
        texpos::map_from_pins(&self.pins, &self.frame, self.tile)
    }

    fn plane_matrix(&self) -> Option<DMat3> {
        Some(texpos::homography(&self.pins)?.inverse())
    }

    /// Writes the pins' map into `m` (the face of the active context).
    fn apply(&self, m: &mut Model) {
        let Some(map) = self.map() else { return };
        let def = esq_core::walk::path_def(m, &self.ctx);
        let Some(face) = m.entities_mut(def).faces.get_mut(self.face) else { return };
        let tex = face.tex.get_or_insert_with(|| Box::new(FaceTex::default()));
        let pins: Vec<(DVec3, DVec2)> =
            self.pins.iter().map(|p| (self.frame.to3(p.at), p.tex / DVec2::from(self.tile))).collect();
        for side in [self.back, !self.back] {
            if side != self.back && !self.both {
                continue;
            }
            let k = side as usize;
            if side {
                tex.back = Some(map);
            } else {
                tex.front = Some(map);
            }
            tex.pins[k] = pins.clone();
        }
    }

    fn preview(&self, doc: &mut Document) {
        let mut m = doc.model.clone();
        self.apply(&mut m);
        doc.set_preview(Some(m));
    }

    /// Records the pins before a change (the menu's Undo).
    fn remember(&mut self) {
        self.undo.push(self.pins.clone());
        self.redo.clear();
    }

    fn changed(&mut self, doc: &mut Document) {
        self.changed = true;
        self.preview(doc);
    }

    /// Ends the positioning, keeping it (Done, another tool, a click outside).
    pub fn finish(&mut self, doc: &mut Document) {
        if self.switch.is_some() {
            return;
        }
        if self.changed && self.map().is_some() && doc.ctx == self.ctx {
            doc.commit("Positionner la texture", |m, _| self.apply(m));
        } else {
            doc.set_preview(None);
        }
        self.switch = Some(self.prev);
    }

    /// Escape: the face keeps its texture as it was.
    pub fn abort(&mut self, doc: &mut Document) {
        doc.set_preview(None);
        self.switch = Some(self.prev);
    }

    /// Texture › Reset Position, from the tool's menu: one operation of its own, then the
    /// positioning goes on from the default position.
    fn reset(&mut self, cx: &mut ToolCx) {
        cx.doc.set_preview(None);
        let (f, both, back) = (self.face, self.both, self.back);
        crate::texturing::reset_face(cx.doc, f, back, both);
        let cam = cx.cam().clone();
        if let Some(t) = Self::start(cx.doc, &cam, f, None, self.prev) {
            let fixed = self.fixed;
            *self = t;
            self.fixed = fixed;
        }
    }

    fn set_fixed(&mut self, fixed: bool) {
        self.fixed = fixed;
        save_fixed_pins_pref(fixed);
    }

    /// The plane point under the mouse, snapped to the model's points unless Ctrl is held.
    fn plane_point(&self, cx: &ToolCx, m: DVec2) -> Option<DVec2> {
        let inv = self.x.inverse();
        if !(cx.mods.ctrl || cx.mods.command) {
            let s = cx.snap(m, None, None);
            if matches!(s.kind, SnapKind::Endpoint | SnapKind::Midpoint | SnapKind::OnEdge | SnapKind::Intersection) {
                let p = inv.transform_point3(s.pos);
                if (p.dot(self.frame.n) - self.frame.c).abs() < 1e-3 {
                    return Some(self.frame.to2(p));
                }
            }
        }
        let (o, d) = cx.cam().ray(m);
        let (o, d) = (inv.transform_point3(o), inv.transform_vector3(d));
        let dn = d.dot(self.frame.n);
        if dn.abs() < 1e-12 {
            return None;
        }
        let t = (self.frame.c - o.dot(self.frame.n)) / dn;
        (t > 0.0).then(|| self.frame.to2(o + d * t))
    }

    fn screen(&self, cam: &Camera, at: DVec2) -> Option<DVec2> {
        cam.project(self.x.transform_point3(self.frame.to3(at)))
    }

    /// The pin under the mouse: on its point or on its head.
    fn pin_at(&self, cam: &Camera, m: DVec2) -> Option<usize> {
        let mut best: Option<(f64, usize)> = None;
        for (i, p) in self.pins.iter().enumerate() {
            let Some(s) = self.screen(cam, p.at) else { continue };
            let d = s.distance(m).min((s + HEAD).distance(m));
            if d < 9.0 && best.is_none_or(|b| d < b.0) {
                best = Some((d, i));
            }
        }
        best.map(|b| b.1)
    }

    /// Tile range shown around the face and the pins (texture tiles, inclusive-exclusive).
    fn region(&self) -> Option<[i64; 4]> {
        let g = self.plane_matrix()?;
        let (w, h) = (self.tile[0].max(1e-9), self.tile[1].max(1e-9));
        let mut lo = DVec2::splat(f64::INFINITY);
        let mut hi = DVec2::splat(f64::NEG_INFINITY);
        for p in self.outline.iter().chain(self.pins.iter().map(|p| &p.at)) {
            let t = texpos::apply(&g, *p);
            let t = DVec2::new(t.x / w, t.y / h);
            lo = lo.min(t);
            hi = hi.max(t);
        }
        if !lo.is_finite() || !hi.is_finite() {
            return None;
        }
        let c = |v: f64| v.clamp(-1e6, 1e6) as i64;
        // The tiles that cover the face and the pins (SketchUp).
        let (mut i0, mut i1, mut j0, mut j1) = (c(lo.x.floor()), c(hi.x.ceil()), c(lo.y.floor()), c(hi.y.ceil()));
        i1 = i1.max(i0 + 1);
        j1 = j1.max(j0 + 1);
        // At most 64 tiles each way, around the middle.
        let cap = |a: &mut i64, b: &mut i64| {
            if *b - *a > 64 {
                let m = (*a + *b) / 2;
                (*a, *b) = (m - 32, m + 32);
            }
        };
        cap(&mut i0, &mut i1);
        cap(&mut j0, &mut j1);
        Some([i0, i1, j0, j1])
    }

    fn inside_region(&self, p: DVec2) -> bool {
        let (Some(r), Some(g)) = (self.region(), self.plane_matrix()) else { return false };
        let t = texpos::apply(&g, p);
        let (u, v) = (t.x / self.tile[0].max(1e-9), t.y / self.tile[1].max(1e-9));
        u >= r[0] as f64 && u <= r[1] as f64 && v >= r[2] as f64 && v <= r[3] as f64
    }

    /// What dragging a pin does now: 0 move, 1 scale/rotate, 2 scale/shear, 3 distort.
    fn action_of(&self, pin: usize) -> usize {
        if self.fixed { pin } else { self.pins.len().saturating_sub(1) }
    }

    fn drag_pin(&mut self, i: usize, to: DVec2) -> bool {
        if self.fixed { texpos::drag_fixed(&mut self.pins, i, to) } else { texpos::drag_free(&mut self.pins, i, to) }
    }

    /// Flip › Left/Right (`horizontal`) or Up/Down.
    pub fn flip(&mut self, doc: &mut Document, horizontal: bool) {
        self.remember();
        texpos::flip(&mut self.pins, horizontal);
        self.changed(doc);
    }

    /// Rotate › 90, 180, 270.
    pub fn rotate(&mut self, doc: &mut Document, degrees: f64) {
        self.remember();
        texpos::rotate(&mut self.pins, degrees);
        self.changed(doc);
    }

    fn undo_pins(&mut self, doc: &mut Document) {
        if let Some(p) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.pins, p));
            self.changed(doc);
        }
    }

    fn redo_pins(&mut self, doc: &mut Document) {
        if let Some(p) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.pins, p));
            self.changed(doc);
        }
    }

    /// What the cursor tooltip says.
    fn hint(&self) -> String {
        if let State::Lifted(_) = self.state {
            return String::new();
        }
        if let Some(i) = self.hover {
            let what = match self.action_of(i) {
                0 => "Faites glisser la punaise pour déplacer la texture.",
                1 => "Faites glisser la punaise pour mettre à l’échelle/faire pivoter la texture.",
                2 => "Faites glisser la punaise pour mettre à l’échelle/scinder la texture.",
                _ => "Faites glisser la punaise pour déformer la texture.",
            };
            return format!("{what}\nCliquez pour ôter la punaise.");
        }
        if !self.over {
            return "Cliquez pour terminer.".into();
        }
        match (self.fixed, self.pins.len()) {
            (true, _) | (false, 4) => "Faites glisser pour déplacer la texture.".into(),
            (false, 0) => "Placez une punaise et faites glisser pour déplacer la texture.".into(),
            (false, 1) => {
                "Placez une punaise et faites glisser pour mettre à l’échelle/faire pivoter la texture.".into()
            }
            (false, 2) => "Placez une punaise et faites glisser pour mettre à l’échelle/scinder la texture.".into(),
            _ => "Placez une punaise et faites glisser pour déformer la texture.".into(),
        }
    }

    fn texture_handle(&self, ctx: &egui::Context, model: &Model) -> Option<egui::TextureHandle> {
        if let Some(h) = self.image.borrow().as_ref() {
            return Some(h.clone());
        }
        let mat = model.materials.get(self.material)?;
        let t = crate::textures::of_material(&mat.ext)?;
        let opts = egui::TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..egui::TextureOptions::LINEAR };
        let h = crate::gpu_img::load_texture(ctx, "position-texture", [t.w as usize, t.h as usize], &t.rgba, opts);
        *self.image.borrow_mut() = Some(h.clone());
        Some(h)
    }
}

/// Offset of a pin's head from its point, on screen.
const HEAD: DVec2 = DVec2::new(9.0, -17.0);

/// The map a face side shows now: its own, or the default one (as the renderer draws it).
fn current_map(face: &esq_core::model::Face, back: bool, tile: [f64; 2]) -> UvMap {
    side_uv(face.tex.as_deref(), !back, face.normal, Some(tile))
}

#[allow(clippy::too_many_arguments)]
fn default_pins(
    map: &UvMap,
    g: &DMat3,
    frame: &Frame,
    tile: [f64; 2],
    outline: &[DVec2],
    reference: Option<DVec2>,
    cam: &Camera,
    x: &DMat4,
) -> Vec<Pin> {
    if map.is_affine() {
        let inv = x.inverse();
        // Where the command was given, on the face's plane; else the face's centre.
        let at = reference.and_then(|m| {
            let (o, d) = cam.ray(m);
            let (o, d) = (inv.transform_point3(o), inv.transform_vector3(d));
            let dn = d.dot(frame.n);
            (dn.abs() > 1e-12).then(|| frame.to2(o + d * ((frame.c - o.dot(frame.n)) / dn)))
        });
        let centre = {
            let lo = outline.iter().fold(DVec2::splat(f64::INFINITY), |a, p| a.min(*p));
            let hi = outline.iter().fold(DVec2::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
            (lo + hi) / 2.0
        };
        if let Some(pins) = texpos::tile_pins(g, tile, at.unwrap_or(centre)) {
            let on_screen = |p: &Pin| {
                cam.project(x.transform_point3(frame.to3(p.at)))
                    .is_some_and(|s| s.x >= 0.0 && s.y >= 0.0 && s.x <= cam.size.x && s.y <= cam.size.y)
            };
            if cam.size.x <= 0.0 || pins.iter().any(on_screen) {
                return pins;
            }
        }
    }
    texpos::box_pins(g, outline)
}

/// Fixed Pins, remembered between sessions (on by default, as in SketchUp).
fn fixed_pins_pref() -> bool {
    if cfg!(test) {
        return true;
    }
    crate::prefs::flag("punaises_fixes", true)
}

fn save_fixed_pins_pref(on: bool) {
    if !cfg!(test) {
        crate::prefs::set_flag("punaises_fixes", on);
    }
}

impl Tool for PositionTextureTool {
    fn id(&self) -> ToolId {
        ToolId::PositionTexture
    }
    fn status(&self) -> String {
        "Faites glisser les punaises pour ajuster la position, l'échelle ou l'orientation de la texture. | Maj = Activer/désactiver les punaises verrouillées. | Ctrl = Aucun accrochage."
            .into()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse = m;
        self.cursor = self.plane_point(cx, m);
        let Some(p) = self.cursor else {
            self.over = false;
            return;
        };
        match self.state {
            State::Pending { pin, at, .. } if at.distance(m) > CLICK_PX => {
                self.remember();
                self.state = State::DragPin(pin);
                self.mouse_move(cx, m);
            }
            State::DragPin(i) => {
                if self.drag_pin(i, p) {
                    self.changed(cx.doc);
                }
            }
            State::DragTexture(last) => {
                texpos::translate(&mut self.pins, p - last);
                self.state = State::DragTexture(p);
                self.changed(cx.doc);
            }
            State::Lifted(i) => {
                // The pin moves over the texture, which stays.
                if let Some(g) = self.plane_matrix() {
                    self.pins[i] = Pin { tex: texpos::apply(&g, p), at: p };
                }
            }
            _ => {
                self.hover = self.pin_at(cx.cam(), m);
                self.over = self.hover.is_some() || self.inside_region(p);
            }
        }
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        if let State::Lifted(_) = self.state {
            // The lifted pin is put down.
            self.state = State::Idle;
            self.hover = self.pin_at(cx.cam(), m);
            return;
        }
        let Some(p) = self.cursor.filter(|_| self.over) else {
            // Outside the texture: done.
            self.finish(cx.doc);
            return;
        };
        if let Some(i) = self.hover {
            self.state = State::Pending { pin: i, at: m, since: Instant::now() };
        } else if !self.fixed && self.pins.len() < 4 {
            // A new free pin, dragged at once.
            if let Some(g) = self.plane_matrix().or(Some(DMat3::IDENTITY)) {
                self.remember();
                self.pins.push(Pin { tex: texpos::apply(&g, p), at: p });
                self.state = State::DragPin(self.pins.len() - 1);
            }
        } else {
            self.remember();
            self.state = State::DragTexture(p);
        }
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        match self.state {
            State::Pending { pin, at, since } => {
                let click = since.elapsed().as_secs_f64() < CLICK_SECONDS && at.distance(m) < CLICK_PX;
                self.state = if click { State::Lifted(pin) } else { State::Idle };
            }
            State::DragPin(_) | State::DragTexture(_) => {
                self.state = State::Idle;
                if self.undo.last() == Some(&self.pins) {
                    self.undo.pop();
                }
            }
            _ => {}
        }
        let _ = cx;
    }
    fn own_key(&mut self, cx: &mut ToolCx, key: egui::Key, mods: egui::Modifiers) -> bool {
        let dir = match key {
            egui::Key::ArrowLeft => DVec2::new(-1.0, 0.0),
            egui::Key::ArrowRight => DVec2::new(1.0, 0.0),
            egui::Key::ArrowUp => DVec2::new(0.0, 1.0),
            egui::Key::ArrowDown => DVec2::new(0.0, -1.0),
            _ => return false,
        };
        // One pixel at the red pin (ten with Shift).
        let at = self.pins.first().map_or(DVec3::ZERO, |p| self.x.transform_point3(self.frame.to3(p.at)));
        let px = cx.cam().pixel_size_at(at) * if mods.shift { 10.0 } else { 1.0 };
        self.remember();
        texpos::translate(&mut self.pins, dir * px);
        self.changed(cx.doc);
        true
    }
    fn shift_tap(&mut self, _cx: &mut ToolCx) {
        let f = !self.fixed;
        self.set_fixed(f);
    }
    fn escape(&mut self, cx: &mut ToolCx) -> bool {
        match self.state {
            State::Pending { .. } | State::Lifted(_) => self.state = State::Idle,
            State::DragPin(_) | State::DragTexture(_) => {
                // Escape during a drag undoes that drag only.
                if let Some(p) = self.undo.pop() {
                    self.pins = p;
                    self.preview(cx.doc);
                }
                self.state = State::Idle;
            }
            _ => self.abort(cx.doc),
        }
        true
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        // Another tool was chosen: like SketchUp, the positioning is kept.
        self.finish(cx.doc);
    }
    fn switch_to(&mut self) -> Option<ToolId> {
        self.switch.take()
    }
    fn menu(&mut self, ui: &mut egui::Ui, cx: &mut ToolCx) -> bool {
        if ui.button("Terminé").clicked() {
            self.finish(cx.doc);
            ui.close();
        }
        let positioned =
            self.original.as_deref().is_some_and(|t| if self.back { t.back.is_some() } else { t.front.is_some() });
        if ui.add_enabled(positioned || self.changed, egui::Button::new("Réinitialiser")).clicked() {
            self.reset(cx);
            ui.close();
        }
        ui.menu_button("Retourner", |ui| {
            if ui.button("Gauche/Droite").clicked() {
                self.flip(cx.doc, true);
                ui.close();
            }
            if ui.button("Haut/Bas").clicked() {
                self.flip(cx.doc, false);
                ui.close();
            }
        });
        ui.menu_button("Faire pivoter", |ui| {
            for a in [90.0, 180.0, 270.0] {
                if ui.button(format!("{a}")).clicked() {
                    self.rotate(cx.doc, a);
                    ui.close();
                }
            }
        });
        ui.separator();
        let mut fixed = self.fixed;
        if ui.checkbox(&mut fixed, "Punaises verrouillées").clicked() {
            self.set_fixed(fixed);
            ui.close();
        }
        ui.separator();
        if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Annuler")).clicked() {
            self.undo_pins(cx.doc);
            ui.close();
        }
        if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Rétablir")).clicked() {
            self.redo_pins(cx.doc);
            ui.close();
        }
        true
    }
    fn overlay(&self, o: &mut Overlay) {
        let to_screen = |p: DVec2| o.screen(self.x.transform_point3(self.frame.to3(p)));
        // The texture, half transparent, over the tiles around the face.
        if let (Some(r), Some(h), Some(tex)) =
            (self.region(), texpos::homography(&self.pins), self.texture_handle(o.painter.ctx(), &o.doc.model))
        {
            let (w, ht) = (self.tile[0], self.tile[1]);
            let n = 24usize;
            let mut mesh = egui::Mesh::with_texture(tex.id());
            let tint = Color32::from_white_alpha(128);
            let mut ok = vec![false; (n + 1) * (n + 1)];
            for j in 0..=n {
                for i in 0..=n {
                    let u = r[0] as f64 + (r[1] - r[0]) as f64 * i as f64 / n as f64;
                    let v = r[2] as f64 + (r[3] - r[2]) as f64 * j as f64 / n as f64;
                    let at = texpos::apply(&h, DVec2::new(u * w, v * ht));
                    let s = to_screen(at).filter(|s| s.x.is_finite() && s.y.is_finite());
                    ok[j * (n + 1) + i] = s.is_some();
                    let s = s.unwrap_or(Pos2::ZERO);
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos: s,
                        uv: egui::pos2(u as f32, -v as f32),
                        color: tint,
                    });
                }
            }
            for j in 0..n {
                for i in 0..n {
                    let a = (j * (n + 1) + i) as u32;
                    let (b, c, d) = (a + 1, a + n as u32 + 1, a + n as u32 + 2);
                    if [a, b, c, d].iter().all(|k| ok[*k as usize]) {
                        mesh.add_triangle(a, b, d);
                        mesh.add_triangle(a, d, c);
                    }
                }
            }
            o.painter.add(egui::Shape::mesh(mesh));
            // Tile edges.
            let grid = Stroke::new(1.0, Color32::from_rgba_unmultiplied(40, 40, 40, 90));
            let line = |a: DVec2, b: DVec2| {
                let pts: Vec<Pos2> =
                    (0..=8).filter_map(|k| to_screen(texpos::apply(&h, a + (b - a) * (k as f64 / 8.0)))).collect();
                if pts.len() > 1 {
                    o.painter.add(egui::Shape::line(pts, grid));
                }
            };
            if (r[1] - r[0]) * (r[3] - r[2]) <= 64 * 64 {
                for i in r[0]..=r[1] {
                    line(DVec2::new(i as f64 * w, r[2] as f64 * ht), DVec2::new(i as f64 * w, r[3] as f64 * ht));
                }
                for j in r[2]..=r[3] {
                    line(DVec2::new(r[0] as f64 * w, j as f64 * ht), DVec2::new(r[1] as f64 * w, j as f64 * ht));
                }
            }
        }
        // The face's outline.
        let pts: Vec<Pos2> = self.outline.iter().filter_map(|p| to_screen(*p)).collect();
        if pts.len() == self.outline.len() && pts.len() > 2 {
            o.painter.add(egui::Shape::closed_line(pts, Stroke::new(2.0, Color32::from_rgb(52, 110, 220))));
        }
        // Feedback of the drag: the red pin to the dragged one.
        if let State::DragPin(i) = self.state
            && i > 0
            && let (Some(a), Some(b)) = (to_screen(self.pins[0].at), to_screen(self.pins[i].at))
        {
            o.painter.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(1.5, Color32::from_gray(30)), 6.0, 4.0));
        }
        // The pins.
        for (i, p) in self.pins.iter().enumerate() {
            let Some(s) = to_screen(p.at) else { continue };
            let lifted = self.state == State::Lifted(i);
            let color = if self.fixed && i < 4 { PIN_COLORS[i] } else { Color32::from_rgb(245, 245, 245) };
            let head = s + egui::vec2(HEAD.x as f32, HEAD.y as f32) * if lifted { 1.4 } else { 1.0 };
            let hot = self.hover == Some(i) || lifted;
            o.painter.line_segment([s, head], Stroke::new(2.0, Color32::from_gray(90)));
            o.painter.circle(head, if hot { 7.5 } else { 6.0 }, color, Stroke::new(1.5, Color32::from_gray(20)));
            o.painter.circle_filled(s, 2.0, Color32::from_gray(20));
        }
        let hint = if self.cursor.is_some() { self.hint() } else { String::new() };
        if !hint.is_empty() {
            let at = o.origin + egui::vec2(self.mouse.x as f32 + 16.0, self.mouse.y as f32 + 18.0);
            o.tooltip(at, &hint);
        }
    }
}

#[cfg(test)]
impl PositionTextureTool {
    pub fn lifted(&self) -> bool {
        matches!(self.state, State::Lifted(_))
    }

    pub fn back(&self) -> bool {
        self.back
    }

    /// A pin's point, in world coordinates.
    pub fn pin_world(&self, i: usize) -> DVec3 {
        self.x.transform_point3(self.frame.to3(self.pins[i].at))
    }
}
