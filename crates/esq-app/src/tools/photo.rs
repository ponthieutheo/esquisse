//! Camera › Match New Photo / Edit Matched Photo (SketchUp's `CMatchPhotoTool`).
//!
//! The photo is shown behind the model. Two sets of bars (two lines each) are dragged onto
//! edges of the photo that are parallel to two world axes; their vanishing points, the
//! origin and a distance give the camera (`esq_core::matchphoto`). The 3D view follows the
//! calibration while the bars move, so the model is drawn over the photo in perspective.
//!
//! Hit sizes and hints come from SketchUp: grips of 10 px, the origin 12 px, and messages
//! such as "Adjust horizon to move the red and green vanishing points".

use eframe::egui::{self, Color32, Pos2, Stroke};
use esq_core::glam::{DMat3, DVec2, DVec3};
use esq_core::matchphoto::{Line2, PhotoCamera, Scale, Setup, SolveError, solve, vanishing_pixel};
use esq_core::model::Scene;
use esq_core::photo::{Blob, GridStyle, MatchPhoto, PhotoGrid};

use super::{Overlay, Tool, ToolCx, ToolId};
use crate::camera::{Camera, PhotoMap};

/// Radius (in points) in which a bar grip or a vanishing point is grabbed.
const GRIP_R: f64 = 10.0;
const ORIGIN_R: f64 = 12.0;
/// How close the cursor must be to the horizon to grab it.
const HORIZON_R: f64 = 6.0;
/// The photo is shown a little smaller than the view while it is matched, so that there is
/// room around it to click ("Click outside the image when done").
const EDIT_ZOOM: f64 = 0.85;
const CLICK_SLOP: f64 = 4.0;
const AXIS_COLORS: [Color32; 3] =
    [Color32::from_rgb(220, 40, 40), Color32::from_rgb(30, 160, 50), Color32::from_rgb(40, 70, 230)];

/// Which two world axes the bars follow ("Red/Green", "Red/Blue", "Green/Blue").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pair {
    RedGreen,
    RedBlue,
    GreenBlue,
}

impl Pair {
    pub const ALL: [Pair; 3] = [Pair::RedGreen, Pair::RedBlue, Pair::GreenBlue];

    pub fn axes(self) -> [usize; 2] {
        match self {
            Pair::RedGreen => [0, 1],
            Pair::RedBlue => [0, 2],
            Pair::GreenBlue => [1, 2],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Pair::RedGreen => "Rouge/Vert",
            Pair::RedBlue => "Rouge/Bleu",
            Pair::GreenBlue => "Vert/Bleu",
        }
    }

    pub fn of(setup: &Setup) -> Pair {
        match (setup.lines[0].is_some(), setup.lines[1].is_some(), setup.lines[2].is_some()) {
            (true, false, true) => Pair::RedBlue,
            (false, true, true) => Pair::GreenBlue,
            _ => Pair::RedGreen,
        }
    }
}

/// Something under the cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    /// End `end` (0 or 1) of bar `line` (0 or 1) of an axis.
    Grip {
        axis: usize,
        line: usize,
        end: usize,
    },
    Origin,
    /// The vanishing point of an axis that has bars.
    Vp(usize),
    Horizon,
    /// A vanishing point that is computed, not set by hand (the third axis).
    DerivedVp(usize),
    /// The optical centre.
    Centre,
}

#[derive(Clone, Copy, Debug)]
enum Drag {
    Hit(Hit),
    Pan,
}

/// The state of one matching session. No egui in here: it is driven by points in the
/// viewport, so that tests can drive it like the mouse does.
#[derive(Clone)]
pub struct Editor {
    /// Scene being modified; `None` for a new photo.
    pub scene: Option<usize>,
    pub name: String,
    pub image: Blob,
    /// Photo size in pixels.
    pub size: DVec2,
    pub pair: Pair,
    /// Two bars per axis (only those of `pair` are used).
    pub bars: [[Line2; 2]; 3],
    pub origin: DVec2,
    /// Distance from the camera to the origin, mm.
    pub distance: f64,
    pub variant: Option<u8>,
    pub grid: PhotoGrid,
    pub zoom: f64,
    pub pan: DVec2,
    drag: Option<Drag>,
    press_at: Option<DVec2>,
    moved: bool,
    last: DVec2,
    pub hover: Option<Hit>,
    /// Last calibration that worked: the view follows it.
    pub cam: PhotoCamera,
    pub error: Option<SolveError>,
}

/// A plausible camera for a fresh photo: a few metres from a box, a bit above the ground.
fn starting_camera(size: DVec2) -> PhotoCamera {
    let (yaw, pitch, d) = (35f64.to_radians(), 15f64.to_radians(), 8000.0);
    let eye = DVec3::new(yaw.sin() * pitch.cos(), -yaw.cos() * pitch.cos(), pitch.sin()) * d;
    let fwd = -eye.normalize();
    let right = fwd.cross(DVec3::Z).normalize();
    let up = right.cross(fwd);
    let rot = DMat3::from_cols(right, -up, fwd).transpose();
    PhotoCamera { rot, trans: -(rot * eye), focal: 0.95 * size.x, principal: size * 0.5, size }
}

/// Bars along the edges of a box seen by `cam`.
fn starting_bars(cam: &PhotoCamera) -> [[Line2; 2]; 3] {
    let (h, e) = (2000.0, 2500.0);
    let seg = |a: DVec3, b: DVec3| Line2 {
        a: cam.project(a).unwrap_or(cam.principal),
        b: cam.project(b).unwrap_or(cam.principal),
    };
    [
        [seg(DVec3::new(-h, -h, 0.0), DVec3::new(h, -h, 0.0)), seg(DVec3::new(-h, h, 0.0), DVec3::new(h, h, 0.0))],
        [seg(DVec3::new(-h, -h, 0.0), DVec3::new(-h, h, 0.0)), seg(DVec3::new(h, -h, 0.0), DVec3::new(h, h, 0.0))],
        [seg(DVec3::new(-h, -h, 0.0), DVec3::new(-h, -h, e)), seg(DVec3::new(h, -h, 0.0), DVec3::new(h, -h, e))],
    ]
}

fn clip_line(a: DVec2, b: DVec2, min: DVec2, max: DVec2) -> Option<(DVec2, DVec2)> {
    // Liang–Barsky.
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-d.x, a.x - min.x), (d.x, max.x - a.x), (-d.y, a.y - min.y), (d.y, max.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                if r > t1 {
                    return None;
                }
                t0 = t0.max(r);
            } else {
                if r < t0 {
                    return None;
                }
                t1 = t1.min(r);
            }
        }
    }
    Some((a + d * t0, a + d * t1))
}

impl Editor {
    pub fn new(name: String, image: Blob, size: DVec2) -> Editor {
        let cam = starting_camera(size);
        let mut e = Editor {
            scene: None,
            name,
            image,
            size,
            pair: Pair::RedGreen,
            bars: starting_bars(&cam),
            origin: cam.project(DVec3::ZERO).unwrap_or(size * 0.5),
            distance: cam.trans.length(),
            variant: None,
            grid: PhotoGrid::default(),
            zoom: EDIT_ZOOM,
            pan: DVec2::ZERO,
            drag: None,
            press_at: None,
            moved: false,
            last: DVec2::ZERO,
            hover: None,
            cam,
            error: None,
        };
        e.recalc();
        e
    }

    /// Edit Matched Photo: starts from what was saved with the scene.
    pub fn from_photo(scene: usize, p: &MatchPhoto) -> Editor {
        let mut e = Editor::new(p.name.clone(), p.image.clone(), DVec2::new(p.size[0] as f64, p.size[1] as f64));
        e.scene = Some(scene);
        e.pair = Pair::of(&p.setup);
        for a in 0..3 {
            if let Some(l) = p.setup.lines[a] {
                e.bars[a] = l;
            }
        }
        e.origin = p.setup.origin;
        e.distance = match p.setup.scale {
            Scale::Distance(d) => d,
            Scale::Reference { .. } => p.camera.trans.length(),
        };
        e.variant = p.setup.variant;
        e.grid = p.grid;
        e.recalc();
        e
    }

    /// What the calibration is given.
    pub fn setup(&self) -> Setup {
        let mut lines = [None; 3];
        for a in self.pair.axes() {
            lines[a] = Some(self.bars[a]);
        }
        Setup {
            size: self.size,
            principal: None,
            lines,
            origin: self.origin,
            scale: Scale::Distance(self.distance),
            variant: self.variant,
        }
    }

    pub fn recalc(&mut self) {
        match solve(&self.setup()) {
            Ok(c) => {
                self.cam = c;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    pub fn photo(&self) -> Option<MatchPhoto> {
        let camera = solve(&self.setup()).ok()?;
        Some(MatchPhoto {
            name: self.name.clone(),
            image: self.image.clone(),
            size: [self.size.x as u32, self.size.y as u32],
            setup: self.setup(),
            camera,
            grid: self.grid,
        })
    }

    pub fn map(&self, viewport: DVec2) -> PhotoMap {
        PhotoMap::new(&self.cam, viewport, self.zoom, self.pan)
    }

    pub fn camera(&self, viewport: DVec2) -> Camera {
        Camera::from_photo(&self.cam, viewport, self.zoom, self.pan).0
    }

    pub fn set_pair(&mut self, pair: Pair) {
        self.pair = pair;
        self.recalc();
    }

    /// Start Over: bars and origin go back to where a new photo has them.
    pub fn start_over(&mut self) {
        let cam = starting_camera(self.size);
        self.bars = starting_bars(&cam);
        self.origin = cam.project(DVec3::ZERO).unwrap_or(self.size * 0.5);
        self.variant = None;
        self.recalc();
    }

    /// Which of the four orientations the calibration uses now.
    pub fn current_variant(&self) -> u8 {
        if let Some(v) = self.variant {
            return v & 3;
        }
        let auto = solve(&self.setup()).ok();
        (0..4u8)
            .find(|&v| {
                let s = Setup { variant: Some(v), ..self.setup() };
                auto.is_some() && solve(&s).ok().map(|c| c.rot) == auto.map(|c| c.rot)
            })
            .unwrap_or(0)
    }

    /// Turns the axes the other way (the scene seen from the opposite side).
    pub fn cycle_variant(&mut self) {
        self.variant = Some((self.current_variant() + 1) & 3);
        self.recalc();
    }

    pub fn set_distance(&mut self, d: f64) {
        if d.is_finite() && d > 0.0 {
            self.distance = d;
            self.recalc();
        }
    }

    // ---- Geometry on the photo ------------------------------------------------------

    /// Vanishing point of the bars of `axis` (photo pixels), `None` when it is at infinity.
    pub fn vp(&self, axis: usize) -> Option<DVec2> {
        vanishing_pixel(&self.bars[axis])
    }

    /// Vanishing point of an axis computed from the calibration.
    fn derived_vp(&self, axis: usize) -> Option<DVec2> {
        let d = self.cam.rot.col(axis);
        (d.z.abs() > 1e-6).then(|| self.cam.principal + DVec2::new(d.x, d.y) * (self.cam.focal / d.z))
    }

    fn free_axis(&self) -> usize {
        3 - self.pair.axes()[0] - self.pair.axes()[1]
    }

    /// The horizon: the line through the two vanishing points.
    pub fn horizon(&self) -> Option<(DVec2, DVec2)> {
        let [a, b] = self.pair.axes();
        match (self.vp(a), self.vp(b)) {
            (Some(p), Some(q)) if p.distance(q) > 1e-6 => Some((p, q)),
            _ => None,
        }
    }

    /// Bars of `axis` made to point at `vp`: the end of each bar nearest to it turns about the
    /// other one.
    fn aim(&mut self, axis: usize, vp: DVec2) {
        for l in self.bars[axis].iter_mut() {
            let (far, near) = if l.a.distance(vp) >= l.b.distance(vp) { (l.a, l.b) } else { (l.b, l.a) };
            let len = near.distance(far);
            let dir = (vp - far).normalize_or_zero();
            if dir == DVec2::ZERO || len < 1e-9 {
                continue;
            }
            let moved = far + dir * len;
            if l.a.distance(vp) >= l.b.distance(vp) {
                l.b = moved;
            } else {
                l.a = moved;
            }
        }
    }

    /// What is under `m` (viewport points).
    pub fn hit(&self, m: DVec2, map: &PhotoMap) -> Option<Hit> {
        for axis in self.pair.axes() {
            for line in 0..2 {
                let l = &self.bars[axis][line];
                for (end, p) in [l.a, l.b].into_iter().enumerate() {
                    if map.to_screen(p).distance(m) <= GRIP_R {
                        return Some(Hit::Grip { axis, line, end });
                    }
                }
            }
        }
        if map.to_screen(self.origin).distance(m) <= ORIGIN_R {
            return Some(Hit::Origin);
        }
        for axis in self.pair.axes() {
            if self.vp(axis).is_some_and(|v| map.to_screen(v).distance(m) <= GRIP_R) {
                return Some(Hit::Vp(axis));
            }
        }
        let free = self.free_axis();
        if self.derived_vp(free).is_some_and(|v| map.to_screen(v).distance(m) <= GRIP_R) {
            return Some(Hit::DerivedVp(free));
        }
        if map.to_screen(self.cam.principal).distance(m) <= GRIP_R {
            return Some(Hit::Centre);
        }
        if let Some((p, q)) = self.horizon() {
            let (p, q) = (map.to_screen(p), map.to_screen(q));
            if (m - p).perp_dot((q - p).normalize_or_zero()).abs() <= HORIZON_R {
                return Some(Hit::Horizon);
            }
        }
        None
    }

    pub fn press(&mut self, m: DVec2, viewport: DVec2) {
        let map = self.map(viewport);
        self.drag = Some(match self.hit(m, &map) {
            Some(h @ (Hit::DerivedVp(_) | Hit::Centre)) => {
                let _ = h;
                Drag::Pan
            }
            Some(h) => Drag::Hit(h),
            None => Drag::Pan,
        });
        self.press_at = Some(m);
        self.moved = false;
        self.last = m;
    }

    pub fn move_to(&mut self, m: DVec2, viewport: DVec2) {
        let map = self.map(viewport);
        let Some(drag) = self.drag else {
            self.hover = self.hit(m, &map);
            return;
        };
        if self.press_at.is_some_and(|p| p.distance(m) > CLICK_SLOP) {
            self.moved = true;
        }
        if !self.moved {
            return;
        }
        let delta = m - self.last;
        self.last = m;
        let img = map.to_image(m);
        match drag {
            Drag::Pan => self.pan += delta,
            Drag::Hit(Hit::Grip { axis, line, end }) => {
                let l = &mut self.bars[axis][line];
                if end == 0 {
                    l.a = img;
                } else {
                    l.b = img;
                }
            }
            Drag::Hit(Hit::Origin) => self.origin = img,
            Drag::Hit(Hit::Vp(axis)) => self.aim(axis, img),
            Drag::Hit(Hit::Horizon) => {
                let dy = delta.y / map.scale;
                for axis in self.pair.axes() {
                    if let Some(v) = self.vp(axis) {
                        self.aim(axis, v + DVec2::new(0.0, dy));
                    }
                }
            }
            Drag::Hit(_) => {}
        }
        if !matches!(drag, Drag::Pan) {
            self.recalc();
        }
    }

    /// Ends a press. True for a click outside the photo, which means "done".
    pub fn release(&mut self, m: DVec2, viewport: DVec2) -> bool {
        let was = self.drag.take();
        let clicked = was.is_some() && !self.moved;
        self.press_at = None;
        self.moved = false;
        if !clicked {
            return false;
        }
        let img = self.map(viewport).to_image(m);
        let outside = img.x < 0.0 || img.y < 0.0 || img.x > self.size.x || img.y > self.size.y;
        outside && matches!(was, Some(Drag::Pan))
    }

    /// Zoom about the cursor.
    pub fn zoom_at(&mut self, m: DVec2, viewport: DVec2, factor: f64) {
        let before = self.map(viewport);
        let img = before.to_image(m);
        self.zoom = (self.zoom * factor).clamp(0.1, 40.0);
        let after = self.map(viewport);
        self.pan += m - after.to_screen(img);
    }

    pub fn hint(&self) -> String {
        let axis_name = |a: usize| ["rouge", "vert", "bleu"][a];
        // CMatchPhotoTool's tooltips, as SketchUp words them.
        match self.hover {
            Some(Hit::Grip { axis, .. }) => {
                format!(
                    "Ajustez les barres d’axe {} pour repérer le point de fuite {}.",
                    axis_name(axis),
                    axis_name(axis)
                )
            }
            Some(Hit::Vp(a)) => format!("Ajustez le point de fuite {}.", axis_name(a)),
            Some(Hit::Horizon) => {
                let [a, b] = self.pair.axes();
                format!("Ajustez l’horizon pour déplacer les points de fuite {} et {}.", axis_name(a), axis_name(b))
            }
            Some(Hit::Origin) => "Placez l’origine sur un point de départ utile pour le dessin.".into(),
            Some(Hit::DerivedVp(a)) => {
                format!("Impossible d’ajuster directement le point de fuite {}.", axis_name(a))
            }
            Some(Hit::Centre) => "Impossible d’ajuster directement le centre de projection.".into(),
            None => "Une fois terminé, cliquez en dehors de l’image.".into(),
        }
    }

    // ---- Drawing --------------------------------------------------------------------

    fn grid_lines(&self) -> Vec<(DVec3, DVec3, usize)> {
        let mut out = Vec::new();
        let n = ((self.distance * 1.6 / self.grid.spacing.iter().cloned().fold(f64::MAX, f64::min).max(1.0)).ceil()
            as i32)
            .clamp(4, 40);
        for (plane, &(u, v)) in [(0usize, 1usize), (0, 2), (1, 2)].iter().enumerate() {
            if !self.grid.show[plane] {
                continue;
            }
            let (su, sv) = (self.grid.spacing[u].max(1.0), self.grid.spacing[v].max(1.0));
            let axis = |a: usize| [DVec3::X, DVec3::Y, DVec3::Z][a];
            for j in -n..=n {
                // Lines along u at v = j·sv, and along v at u = j·su.
                out.push((
                    axis(v) * (j as f64 * sv) - axis(u) * (n as f64 * su),
                    axis(v) * (j as f64 * sv) + axis(u) * (n as f64 * su),
                    u,
                ));
                out.push((
                    axis(u) * (j as f64 * su) - axis(v) * (n as f64 * sv),
                    axis(u) * (j as f64 * su) + axis(v) * (n as f64 * sv),
                    v,
                ));
            }
        }
        out
    }

    /// Photo pixel of a world point, clipped to the part of the segment in front of the camera.
    fn project_segment(&self, a: DVec3, b: DVec3) -> Option<(DVec2, DVec2)> {
        let ca = self.cam.rot * a + self.cam.trans;
        let cb = self.cam.rot * b + self.cam.trans;
        let near = self.cam.trans.length() * 0.02;
        let (ca, cb) = match (ca.z > near, cb.z > near) {
            (true, true) => (ca, cb),
            (false, false) => return None,
            (true, false) => (ca, ca + (cb - ca) * ((near - ca.z) / (cb.z - ca.z))),
            (false, true) => (cb + (ca - cb) * ((near - cb.z) / (ca.z - cb.z)), cb),
        };
        let px = |c: DVec3| self.cam.principal + DVec2::new(c.x, c.y) * (self.cam.focal / c.z);
        Some((px(ca), px(cb)))
    }

    fn draw(&self, o: &Overlay) {
        let size = o.cam.size;
        let map = self.map(size);
        let at = |p: DVec2| -> Pos2 { o.origin + egui::vec2(p.x as f32, p.y as f32) };
        let (min, max) = (DVec2::splat(-50.0), size + DVec2::splat(50.0));
        let seg = |a: DVec2, b: DVec2, stroke: Stroke| {
            if let Some((a, b)) = clip_line(a, b, min, max) {
                o.painter.line_segment([at(a), at(b)], stroke);
            }
        };
        // Photo outline.
        let (tl, br) = map.rect();
        o.painter.rect_stroke(
            egui::Rect::from_min_max(at(tl), at(br)),
            0.0,
            Stroke::new(1.0, Color32::from_gray(60)),
            egui::StrokeKind::Outside,
        );
        // Perspective grid.
        for (a, b, axis) in self.grid_lines() {
            if let Some((p, q)) = self.project_segment(a, b) {
                let c = AXIS_COLORS[axis].gamma_multiply(0.55);
                match self.grid.style {
                    GridStyle::Lines => seg(map.to_screen(p), map.to_screen(q), Stroke::new(1.0, c)),
                    GridStyle::Crosses => {}
                }
            }
        }
        if self.grid.style == GridStyle::Crosses {
            let n = 10;
            for (plane, &(u, v)) in [(0usize, 1usize), (0, 2), (1, 2)].iter().enumerate() {
                if !self.grid.show[plane] {
                    continue;
                }
                let axis = |a: usize| [DVec3::X, DVec3::Y, DVec3::Z][a];
                for i in -n..=n {
                    for j in -n..=n {
                        let w =
                            axis(u) * (i as f64 * self.grid.spacing[u]) + axis(v) * (j as f64 * self.grid.spacing[v]);
                        if let Some(p) = self.cam.project(w) {
                            let s = at(map.to_screen(p));
                            let k = 3.0;
                            let st = Stroke::new(1.0, Color32::from_white_alpha(200));
                            o.painter.line_segment([s - egui::vec2(k, 0.0), s + egui::vec2(k, 0.0)], st);
                            o.painter.line_segment([s - egui::vec2(0.0, k), s + egui::vec2(0.0, k)], st);
                        }
                    }
                }
            }
        }
        // Horizon.
        if let Some((p, q)) = self.horizon() {
            let (p, q) = (map.to_screen(p), map.to_screen(q));
            let d = (q - p).normalize_or_zero() * 1e5;
            let hot = matches!(self.hover, Some(Hit::Horizon)) || matches!(self.drag, Some(Drag::Hit(Hit::Horizon)));
            seg(p - d, p + d, Stroke::new(if hot { 2.5 } else { 1.0 }, Color32::from_rgb(240, 190, 20)));
        }
        // Bars, their prolongation and their grips.
        for axis in self.pair.axes() {
            let color = AXIS_COLORS[axis];
            for l in &self.bars[axis] {
                let (a, b) = (map.to_screen(l.a), map.to_screen(l.b));
                let d = (b - a).normalize_or_zero() * 1e5;
                seg(a - d, b + d, Stroke::new(1.0, color.gamma_multiply(0.45)));
                seg(a, b, Stroke::new(2.5, color));
            }
            for line in 0..2 {
                for end in 0..2 {
                    let l = &self.bars[axis][line];
                    let p = at(map.to_screen(if end == 0 { l.a } else { l.b }));
                    let hot = self.hover == Some(Hit::Grip { axis, line, end });
                    let r = if hot { 6.0 } else { 4.5 };
                    let rect = egui::Rect::from_center_size(p, egui::vec2(2.0 * r, 2.0 * r));
                    o.painter.rect_filled(rect, 1.0, Color32::WHITE);
                    o.painter.rect_stroke(rect, 1.0, Stroke::new(1.5, color), egui::StrokeKind::Middle);
                }
            }
            if let Some(v) = self.vp(axis) {
                let s = map.to_screen(v);
                if s.x >= 0.0 && s.y >= 0.0 && s.x <= size.x && s.y <= size.y {
                    o.painter.circle(at(s), 6.0, color, Stroke::new(1.5, Color32::WHITE));
                }
            }
        }
        // The vanishing point that follows from the others, and the optical centre.
        let free = self.free_axis();
        if let Some(v) = self.derived_vp(free) {
            let s = map.to_screen(v);
            if s.x >= 0.0 && s.y >= 0.0 && s.x <= size.x && s.y <= size.y {
                o.painter.circle_stroke(at(s), 5.0, Stroke::new(1.5, AXIS_COLORS[free]));
            }
        }
        let c = at(map.to_screen(self.cam.principal));
        let st = Stroke::new(1.0, Color32::from_gray(230));
        o.painter.line_segment([c - egui::vec2(6.0, 0.0), c + egui::vec2(6.0, 0.0)], st);
        o.painter.line_segment([c - egui::vec2(0.0, 6.0), c + egui::vec2(0.0, 6.0)], st);
        // Axes at the origin.
        let o_px = at(map.to_screen(self.origin));
        let len = self.distance * 0.25;
        for a in 0..3 {
            let dir = [DVec3::X, DVec3::Y, DVec3::Z][a] * len;
            if let Some((p, q)) = self.project_segment(DVec3::ZERO, dir) {
                seg(map.to_screen(p), map.to_screen(q), Stroke::new(2.5, AXIS_COLORS[a]));
            }
        }
        let hot = self.hover == Some(Hit::Origin);
        o.painter.circle(
            o_px,
            if hot { 8.0 } else { 6.0 },
            Color32::from_rgb(255, 255, 255),
            Stroke::new(2.0, Color32::BLACK),
        );
    }
}

/// The tool: a photo being matched, or nothing yet (the panel proposes to pick a photo).
#[derive(Default)]
pub struct MatchPhotoTool {
    pub ed: Option<Editor>,
    /// Set when the session ends: `Some(scene)` after a validation, `None` after a cancel.
    exit: Option<Option<usize>>,
    dist_text: Option<String>,
    spacing_text: Option<String>,
    note: Option<String>,
}

impl MatchPhotoTool {
    pub fn with_editor(ed: Editor) -> Self {
        MatchPhotoTool { ed: Some(ed), ..Default::default() }
    }

    /// Camera › Match New Photo: loads the photo file.
    pub fn from_file(path: &std::path::Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("Lecture impossible : {e}"))?;
        Self::from_bytes(path.file_name().and_then(|n| n.to_str()).unwrap_or("photo"), bytes)
    }

    pub fn from_bytes(name: &str, bytes: Vec<u8>) -> Result<Self, String> {
        let [w, h] = crate::photo_img::image_size(&bytes)?;
        let image = Blob(std::sync::Arc::new(bytes));
        Ok(Self::with_editor(Editor::new(name.to_string(), image, DVec2::new(w as f64, h as f64))))
    }

    fn sync_camera(&self, cx: &mut ToolCx) {
        if let Some(ed) = &self.ed {
            *cx.new_cam = Some(ed.camera(cx.cam().size));
        }
    }

    /// Validates the match: the photo becomes a scene (or replaces the one being modified).
    pub fn finish(&mut self, cx: &mut ToolCx) {
        let Some(ed) = &self.ed else { return };
        let Some(photo) = ed.photo() else {
            self.note = Some(ed.error.map(|e| e.to_string()).unwrap_or_else(|| "Calage impossible.".into()));
            return;
        };
        let hidden: Vec<usize> =
            cx.doc.model.layers.iter().enumerate().filter(|(_, l)| !l.visible).map(|(i, _)| i).collect();
        let scene = ed.scene.filter(|i| *i < cx.doc.model.scenes.len());
        let mut index = 0;
        let label = if scene.is_some() { "Modifier la photo adaptée" } else { "Adapter une photo" };
        cx.doc.commit(label, |m, _| {
            let camera = photo.scene_camera();
            match scene {
                Some(i) => {
                    m.scenes[i].camera = camera;
                    m.scenes[i].photo = Some(photo);
                    index = i;
                }
                None => {
                    let stem = std::path::Path::new(&photo.name)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    let name =
                        stem.filter(|n| !m.scenes.iter().any(|s| &s.name == n)).unwrap_or_else(|| m.next_scene_name());
                    m.scenes.push(Scene {
                        name,
                        description: String::new(),
                        camera,
                        hidden_layers: Some(hidden),
                        photo: Some(photo),
                        ext: Default::default(),
                    });
                    index = m.scenes.len() - 1;
                }
            }
        });
        let photo = cx.doc.model.scenes[index].photo.as_ref().expect("photo just saved");
        *cx.new_cam = Some(Camera::from_photo(&photo.camera, cx.cam().size, 1.0, DVec2::ZERO).0);
        *cx.message = Some("Photo adaptée.".into());
        self.exit = Some(Some(index));
        self.ed = None;
    }
}

impl Tool for MatchPhotoTool {
    fn id(&self) -> ToolId {
        ToolId::MatchPhoto
    }

    fn status(&self) -> String {
        match &self.ed {
            Some(ed) => ed.hint(),
            None => "Choisissez une photo à adapter au modèle.".into(),
        }
    }

    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let size = cx.cam().size;
        if let Some(ed) = &mut self.ed {
            ed.move_to(m, size);
        }
        self.sync_camera(cx);
    }

    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        let size = cx.cam().size;
        if let Some(ed) = &mut self.ed {
            ed.press(m, size);
        }
    }

    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        let size = cx.cam().size;
        let done = self.ed.as_mut().is_some_and(|ed| ed.release(m, size));
        if done {
            self.finish(cx);
        }
        self.sync_camera(cx);
    }

    fn cancel(&mut self, cx: &mut ToolCx) {
        cx.doc.set_preview(None);
        if self.ed.take().is_some() || self.exit.is_none() {
            self.exit = Some(None);
        }
    }

    fn busy(&self) -> bool {
        self.ed.is_some()
    }

    fn scroll(&mut self, cx: &mut ToolCx, m: DVec2, delta: f64) -> bool {
        let size = cx.cam().size;
        let Some(ed) = &mut self.ed else { return false };
        ed.zoom_at(m, size, (delta * 0.0025).exp());
        self.sync_camera(cx);
        true
    }

    fn camera(&mut self, viewport: DVec2) -> Option<Camera> {
        self.ed.as_ref().map(|ed| ed.camera(viewport))
    }

    fn finished(&mut self) -> Option<Option<usize>> {
        self.exit.take()
    }

    fn backdrop(&mut self, o: &Overlay) -> bool {
        let Some(ed) = &self.ed else { return false };
        let map = ed.map(o.cam.size);
        let (tl, br) = map.rect();
        let at = |p: DVec2| o.origin + egui::vec2(p.x as f32, p.y as f32);
        // The area around the photo is a flat grey, so that the photo stands out.
        o.painter.rect_filled(o.painter.clip_rect(), 0.0, Color32::from_gray(150));
        crate::photo_img::paint(o.painter.ctx(), &o.painter, &ed.image, egui::Rect::from_min_max(at(tl), at(br)), 1.0);
        true
    }

    fn overlay(&self, o: &mut Overlay) {
        if let Some(ed) = &self.ed {
            ed.draw(o);
        }
    }

    fn panel(&mut self, ctx: &egui::Context, rect: egui::Rect, cx: &mut ToolCx) {
        let mut action = None;
        egui::Area::new(egui::Id::unique("match_photo_panel"))
            .fixed_pos(rect.right_top() + egui::vec2(-300.0, 12.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(276.0);
                    ui.strong("Adapter une photo");
                    let Some(ed) = &mut self.ed else {
                        ui.label("Aucune photo n'est chargée.");
                        if ui.button("Choisir une photo...").clicked() {
                            action = Some(Action::Pick);
                        }
                        return;
                    };
                    ui.label(format!("{} ({} × {} px)", ed.name, ed.size.x, ed.size.y));
                    ui.separator();
                    ui.label("Axes suivis par les barres :");
                    ui.horizontal(|ui| {
                        for p in Pair::ALL {
                            if ui.selectable_label(ed.pair == p, p.label()).clicked() {
                                action = Some(Action::Pair(p));
                            }
                        }
                    });
                    if ui.button("Changer le sens des axes").clicked() {
                        action = Some(Action::Flip);
                    }
                    ui.horizontal(|ui| {
                        ui.label("Distance à l'origine :");
                        let text = self.dist_text.get_or_insert_with(|| cx.len(ed.distance));
                        let r = ui.add(egui::TextEdit::singleline(text).desired_width(80.0));
                        if r.lost_focus() {
                            match cx.parse_len(text) {
                                Ok(d) => action = Some(Action::Distance(d)),
                                Err(e) => self.note = Some(e),
                            }
                            self.dist_text = None;
                        }
                    });
                    ui.separator();
                    ui.label("Grille :");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut ed.grid.show[0], "Rouge-vert");
                        ui.checkbox(&mut ed.grid.show[1], "Rouge-bleu");
                        ui.checkbox(&mut ed.grid.show[2], "Vert-bleu");
                    });
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut ed.grid.style, GridStyle::Lines, "Lignes");
                        ui.selectable_value(&mut ed.grid.style, GridStyle::Crosses, "Croix");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Pas :");
                        let text = self.spacing_text.get_or_insert_with(|| cx.len(ed.grid.spacing[0]));
                        let r = ui.add(egui::TextEdit::singleline(text).desired_width(80.0));
                        if r.lost_focus() {
                            match cx.parse_len(text) {
                                Ok(d) if d > 0.0 => ed.grid.spacing = [d; 3],
                                Ok(_) => {}
                                Err(e) => self.note = Some(e),
                            }
                            self.spacing_text = None;
                        }
                    });
                    ui.separator();
                    if let Some(e) = ed.error {
                        ui.colored_label(Color32::from_rgb(200, 40, 40), e.to_string());
                    }
                    if let Some(n) = &self.note {
                        ui.colored_label(Color32::from_rgb(200, 40, 40), n);
                    }
                    ui.label(egui::RichText::new(ed.hint()).small());
                    ui.horizontal(|ui| {
                        if ui.add_enabled(ed.error.is_none(), egui::Button::new("Terminé")).clicked() {
                            action = Some(Action::Finish);
                        }
                        if ui.button("Recommencer").clicked() {
                            action = Some(Action::StartOver);
                        }
                        if ui.button("Annuler").clicked() {
                            action = Some(Action::Cancel);
                        }
                    });
                    if ui.button("Photo entière").clicked() {
                        action = Some(Action::Fit);
                    }
                });
            });
        match action {
            Some(Action::Pick) => {
                if let Some(path) = crate::settings::dialog("textures")
                    .set_title("Choisir une photo")
                    .add_filter("Images", &["png", "jpg", "jpeg"])
                    .pick_file()
                {
                    match MatchPhotoTool::from_file(&path) {
                        Ok(t) => {
                            self.ed = t.ed;
                            self.note = None;
                        }
                        Err(e) => self.note = Some(e),
                    }
                }
            }
            Some(Action::Pair(p)) => {
                if let Some(ed) = &mut self.ed {
                    ed.set_pair(p);
                }
            }
            Some(Action::Flip) => {
                if let Some(ed) = &mut self.ed {
                    ed.cycle_variant();
                }
            }
            Some(Action::Distance(d)) => {
                if let Some(ed) = &mut self.ed {
                    ed.set_distance(d);
                }
                self.note = None;
            }
            Some(Action::StartOver) => {
                if let Some(ed) = &mut self.ed {
                    ed.start_over();
                }
            }
            Some(Action::Fit) => {
                if let Some(ed) = &mut self.ed {
                    ed.zoom = EDIT_ZOOM;
                    ed.pan = DVec2::ZERO;
                }
            }
            Some(Action::Cancel) => self.cancel(cx),
            Some(Action::Finish) => self.finish(cx),
            None => {}
        }
        if self.ed.is_some() {
            self.sync_camera(cx);
        }
    }
}

enum Action {
    Pick,
    Pair(Pair),
    Flip,
    Distance(f64),
    StartOver,
    Fit,
    Cancel,
    Finish,
}

#[cfg(test)]
#[path = "photo_tests.rs"]
pub mod tests;
