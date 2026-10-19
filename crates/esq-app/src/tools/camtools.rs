//! Camera tools of the Camera menu: Field of View, Zoom Window, Position Camera, Look
//! Around, Walk.

use eframe::egui::{self, Color32, Stroke};
use esq_core::glam::{DVec2, DVec3};
use esq_core::math::Plane;
use esq_core::model::Units;
use esq_core::units::format_length;

use super::{Overlay, Tool, ToolCx, ToolId};
use crate::camera::Camera;

/// SketchUp's default eye height: 5'6".
pub const EYE_HEIGHT: f64 = 1676.4;

/// Moves the eye to `eye`, looking along yaw/pitch, keeping a short target distance so
/// that orbiting afterwards turns around a nearby point.
fn camera_at(cam: &Camera, eye: DVec3, yaw: f64, pitch: f64) -> Camera {
    let mut c = cam.clone();
    c.yaw = yaw;
    c.pitch = pitch.clamp(-1.55, 1.55);
    c.distance = 1000.0;
    c.target = eye + c.forward() * c.distance;
    c
}

/// Height of the eye above the ground plane of the drawing axes.
fn eye_height(cx: &ToolCx) -> f64 {
    let g = cx.ground();
    g.distance(cx.cam().eye())
}

fn set_eye_height(cx: &mut ToolCx, h: f64) {
    let g = cx.ground();
    let cam = cx.cam().clone();
    let eye = cam.eye();
    let eye = eye + g.n * (h - g.distance(eye));
    *cx.new_cam = Some(camera_at(&cam, eye, cam.yaw, cam.pitch));
}

// ---------------------------------------------------------------------------------------

/// Field of View: drag up or down, or type an angle (degrees) or a focal length ("50mm").
#[derive(Default)]
pub struct FovTool {
    last: Option<DVec2>,
}

impl Tool for FovTool {
    fn id(&self) -> ToolId {
        ToolId::Fov
    }
    fn status(&self) -> String {
        // CCameraFOVTool (the box takes an angle, 35, or a focal length, 50mm).
        "Faites glisser la souris ou saisissez un champ angulaire.".into()
    }
    fn vcb_label(&self) -> &'static str {
        "Champ angulaire"
    }
    fn vcb_value(&self, _units: Units) -> String {
        String::new()
    }
    fn busy(&self) -> bool {
        self.last.is_some()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        if let Some(l) = self.last {
            let mut c = cx.cam().clone();
            c.fov_y = (c.fov_y * (1.0 + (m.y - l.y) * 0.004)).clamp(1f64.to_radians(), 120f64.to_radians());
            *cx.message = Some(format!("Champ angulaire : {:.1}°", c.fov_y.to_degrees()).replace('.', ","));
            *cx.new_cam = Some(c);
            self.last = Some(m);
        }
    }
    fn press(&mut self, _cx: &mut ToolCx, m: DVec2) {
        self.last = Some(m);
    }
    fn release(&mut self, _cx: &mut ToolCx, _m: DVec2) {
        self.last = None;
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let t = text.trim().to_lowercase().replace(',', ".");
        let fov = if let Some(f) = t.strip_suffix("mm") {
            let f: f64 = f.trim().parse().map_err(|_| "Focale invalide".to_string())?;
            // 35 mm film: the picture is 24 mm high.
            2.0 * (12.0 / f.max(1.0)).atan()
        } else {
            t.trim_end_matches("deg")
                .trim_end_matches('°')
                .trim()
                .parse::<f64>()
                .map_err(|_| "Angle invalide".to_string())?
                .to_radians()
        };
        let mut c = cx.cam().clone();
        c.fov_y = fov.clamp(1f64.to_radians(), 120f64.to_radians());
        *cx.new_cam = Some(c);
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------

/// Zoom Window: drag a rectangle to fill the view with it.
#[derive(Default)]
pub struct ZoomWindowTool {
    down: Option<DVec2>,
    cur: DVec2,
}

impl Tool for ZoomWindowTool {
    fn id(&self) -> ToolId {
        ToolId::ZoomWindow
    }
    fn status(&self) -> String {
        "Définissez une zone à l’aide du pointeur pour effectuer un zoom".into()
    }
    fn busy(&self) -> bool {
        self.down.is_some()
    }
    fn mouse_move(&mut self, _cx: &mut ToolCx, m: DVec2) {
        self.cur = m;
    }
    fn press(&mut self, _cx: &mut ToolCx, m: DVec2) {
        self.down = Some(m);
        self.cur = m;
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        let Some(d) = self.down.take() else { return };
        let (min, max) = (d.min(m), d.max(m));
        let size = max - min;
        if size.x < 4.0 || size.y < 4.0 {
            return;
        }
        let cam = cx.cam().clone();
        let centre = (min + max) * 0.5;
        // The new target: where the rectangle's centre meets the plane of the old target.
        let pl = Plane::from_point_normal(cam.target, cam.forward());
        let (ro, rd) = cam.ray(centre);
        let Some(t) = pl.intersect_ray(ro, rd) else { return };
        let mut c = cam.clone();
        let k = (size.x / cam.size.x).max(size.y / cam.size.y);
        if cam.perspective {
            let target = ro + rd * t;
            c.target = target;
            c.distance = (cam.distance * k).max(1.0);
            // Keep the eye on the line through the new target.
            let eye = cam.eye() + (target - cam.eye()) * (1.0 - k);
            c.target = eye + c.forward() * c.distance;
        } else {
            c.target = ro + rd * t;
            c.distance = (cam.distance * k).max(1.0);
        }
        *cx.new_cam = Some(c);
    }
    fn cancel(&mut self, _cx: &mut ToolCx) {
        self.down = None;
    }
    fn overlay(&self, o: &mut Overlay) {
        if let Some(d) = self.down {
            let a = o.origin + egui::vec2(d.x as f32, d.y as f32);
            let b = o.origin + egui::vec2(self.cur.x as f32, self.cur.y as f32);
            o.painter.rect_stroke(
                egui::Rect::from_two_pos(a, b),
                0.0,
                Stroke::new(1.0, Color32::from_gray(40)),
                egui::StrokeKind::Middle,
            );
        }
    }
}

// ---------------------------------------------------------------------------------------

/// Position Camera: click where to stand (eyes at 1.68 m), or press where to stand and
/// drag to what to look at. Then the tool looks around, like SketchUp's.
#[derive(Default)]
pub struct PositionCameraTool {
    down: Option<(DVec2, DVec3)>,
    placed: bool,
    look: LookAroundTool,
}

impl Tool for PositionCameraTool {
    fn id(&self) -> ToolId {
        if self.placed { ToolId::LookAround } else { ToolId::PositionCamera }
    }
    fn status(&self) -> String {
        // CPositionCameraTool: a click stands there (eyes at 1.68 m), a drag also aims.
        match (self.placed, self.down.is_some()) {
            (true, _) => self.look.status(),
            (false, true) => "Sélectionnez un point qui sera visé par la caméra.".into(),
            (false, false) => "Sélectionnez la position de la caméra.".into(),
        }
    }
    fn vcb_label(&self) -> &'static str {
        "Hauteur des yeux"
    }
    fn vcb_value(&self, units: Units) -> String {
        self.look.vcb_value(units)
    }
    fn busy(&self) -> bool {
        self.down.is_some()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.placed {
            self.look.mouse_move(cx, m);
        }
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.placed {
            self.look.press(cx, m);
            return;
        }
        let s = cx.snap(m, None, None);
        self.down = Some((m, s.pos));
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.placed {
            self.look.release(cx, m);
            return;
        }
        let Some((d, p)) = self.down.take() else { return };
        let up = cx.axis(2);
        let eye = p + up * EYE_HEIGHT;
        let cam = cx.cam().clone();
        let new = if d.distance(m) > 6.0 {
            let target = cx.snap(m, None, None).pos;
            let dir = (target - eye).normalize_or_zero();
            camera_at(&cam, eye, dir.y.atan2(dir.x) + std::f64::consts::PI, -dir.z.clamp(-1.0, 1.0).asin())
        } else {
            camera_at(&cam, eye, cam.yaw, 0.0)
        };
        let mut new = new;
        new.perspective = true;
        *cx.new_cam = Some(new);
        self.placed = true;
        self.look.last = None;
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let h = cx.parse_len(text)?;
        set_eye_height(cx, h);
        Ok(())
    }
    fn cancel(&mut self, _cx: &mut ToolCx) {
        self.down = None;
    }
}

// ---------------------------------------------------------------------------------------

/// Look Around: drag to turn the head; the eye stays where it is.
#[derive(Default)]
pub struct LookAroundTool {
    last: Option<DVec2>,
    height: Option<f64>,
}

impl Tool for LookAroundTool {
    fn id(&self) -> ToolId {
        ToolId::LookAround
    }
    fn status(&self) -> String {
        // CCameraPanTool ("pan" turns the camera; the Eye Height box is CCameraEyeHeightTool's).
        "Déplacez le curseur dans la direction voulue pour faire pivoter la caméra.".into()
    }
    fn vcb_label(&self) -> &'static str {
        "Hauteur des yeux"
    }
    fn vcb_value(&self, units: Units) -> String {
        self.height.map(|h| format_length(h, units)).unwrap_or_default()
    }
    fn busy(&self) -> bool {
        self.last.is_some()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.height = Some(eye_height(cx));
        let Some(l) = self.last else { return };
        let cam = cx.cam().clone();
        let d = m - l;
        let eye = cam.eye();
        *cx.new_cam = Some(camera_at(&cam, eye, cam.yaw - d.x * 0.004, cam.pitch + d.y * 0.004));
        self.last = Some(m);
    }
    fn press(&mut self, _cx: &mut ToolCx, m: DVec2) {
        self.last = Some(m);
    }
    fn release(&mut self, _cx: &mut ToolCx, _m: DVec2) {
        self.last = None;
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let h = cx.parse_len(text)?;
        set_eye_height(cx, h);
        self.height = Some(h);
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------

/// How close the walker may come to a face (a body's half width).
const WALK_CLEARANCE: f64 = 300.0;

/// The part of the step from `eye` that does not bring the walker within
/// [`WALK_CLEARANCE`] of a face ahead.
fn walk_step(cx: &ToolCx, eye: DVec3, step: DVec3) -> DVec3 {
    let len = step.length();
    if len < 1e-9 {
        return step;
    }
    let dir = step / len;
    match cx.infer.pick.ray_face(eye, dir) {
        Some((t, _, _)) if t < len + WALK_CLEARANCE => dir * (t - WALK_CLEARANCE).clamp(0.0, len),
        _ => step,
    }
}

/// Walk: drag up/down to walk forward/back, left/right to turn; Shift moves sideways and
/// up/down, Ctrl runs, Alt walks through walls.
#[derive(Default)]
pub struct WalkTool {
    last: Option<DVec2>,
    height: Option<f64>,
}

impl Tool for WalkTool {
    fn id(&self) -> ToolId {
        ToolId::Walk
    }
    fn status(&self) -> String {
        "Cliquez et faites glisser pour marcher.  Ctrl = courir, Maj = se déplacer verticalement ou latéralement, \
         Alt = désactiver la détection de collision"
            .into()
    }
    fn vcb_label(&self) -> &'static str {
        "Hauteur des yeux"
    }
    fn vcb_value(&self, units: Units) -> String {
        self.height.map(|h| format_length(h, units)).unwrap_or_default()
    }
    fn busy(&self) -> bool {
        self.last.is_some()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.height = Some(eye_height(cx));
        let Some(l) = self.last else { return };
        let cam = cx.cam().clone();
        let d = m - l;
        let speed = if cx.mods.ctrl { 30.0 } else { 10.0 };
        let eye = cam.eye();
        let up = cx.axis(2);
        let fwd = (cam.forward() - up * cam.forward().dot(up)).normalize_or(cam.forward());
        let (step, yaw) = if cx.mods.shift {
            let right = fwd.cross(up);
            (right * d.x * speed - up * d.y * speed, cam.yaw)
        } else {
            (-fwd * d.y * speed, cam.yaw - d.x * 0.004)
        };
        // Collision detection (CCameraWalkTool; Alt turns it off): no walking through a face.
        let step = if cx.mods.alt { step } else { walk_step(cx, eye, step) };
        *cx.new_cam = Some(camera_at(&cam, eye + step, yaw, cam.pitch));
        self.last = Some(m);
    }
    fn press(&mut self, _cx: &mut ToolCx, m: DVec2) {
        self.last = Some(m);
    }
    fn release(&mut self, _cx: &mut ToolCx, _m: DVec2) {
        self.last = None;
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let h = cx.parse_len(text)?;
        set_eye_height(cx, h);
        self.height = Some(h);
        Ok(())
    }
}
