//! Z-up orbit camera, SketchUp style.

use esq_core::glam::dcamera::rh::{proj::directx, view};
use esq_core::glam::{DMat4, DVec2, DVec3, DVec4};

/// `Camera::project` with the view's matrix computed once (`Camera::projector`).
#[derive(Clone, Copy, Debug)]
pub struct Projector {
    view_proj: DMat4,
    size: DVec2,
}

impl Projector {
    /// World point → pixel position (origin top-left).
    pub fn project(&self, p: DVec3) -> Option<DVec2> {
        let c = self.clip(p);
        if c.w <= 1e-9 {
            return None;
        }
        let ndc = c.truncate() / c.w;
        Some(DVec2::new((ndc.x + 1.0) * 0.5 * self.size.x, (1.0 - ndc.y) * 0.5 * self.size.y))
    }

    /// Clip coordinates of a world point.
    pub fn clip(&self, p: DVec3) -> DVec4 {
        self.view_proj * DVec4::new(p.x, p.y, p.z, 1.0)
    }

    /// Whether something inside `b` may show within `px` pixels of `m` (never false when it
    /// does; true when the box reaches behind the eye, where projecting tells nothing).
    pub fn box_near(&self, b: &crate::bvh::Aabb, m: DVec2, px: f64) -> bool {
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for c in b.corners() {
            let Some(s) = self.project(c).filter(|s| s.is_finite()) else { return true };
            lo = lo.min(s);
            hi = hi.max(s);
        }
        // A projective map keeps a box in front of the eye inside the hull of its corners.
        let margin = px + 1.0 + 1e-6 * (hi - lo).max_element().abs();
        m.x >= lo.x - margin && m.x <= hi.x + margin && m.y >= lo.y - margin && m.y <= hi.y + margin
    }
}

/// Where a matched photo sits in the viewport, and the camera zoom that goes with it.
#[derive(Clone, Copy, Debug)]
pub struct PhotoMap {
    /// Viewport points per photo pixel.
    pub scale: f64,
    /// Middle of the photo, in viewport points.
    pub centre: DVec2,
    /// Photo size in pixels.
    pub size: DVec2,
    /// Vertical field of view of the viewport that shows the photo like this.
    pub fov_y: f64,
    /// Optical centre of that view (normalized device coordinates).
    pub shift: DVec2,
}

impl PhotoMap {
    pub fn new(photo: &esq_core::matchphoto::PhotoCamera, viewport: DVec2, zoom: f64, pan: DVec2) -> Self {
        let size = photo.size;
        let scale = (viewport.x / size.x).min(viewport.y / size.y) * zoom;
        let centre = viewport * 0.5 + pan;
        let pp = centre + (photo.principal - size * 0.5) * scale;
        PhotoMap {
            scale,
            centre,
            size,
            fov_y: 2.0 * ((viewport.y * 0.5) / (photo.focal * scale)).atan(),
            shift: DVec2::new(
                (pp.x - viewport.x * 0.5) / (viewport.x * 0.5),
                -(pp.y - viewport.y * 0.5) / (viewport.y * 0.5),
            ),
        }
    }

    pub fn to_screen(self, px: DVec2) -> DVec2 {
        self.centre + (px - self.size * 0.5) * self.scale
    }

    pub fn to_image(self, p: DVec2) -> DVec2 {
        (p - self.centre) / self.scale + self.size * 0.5
    }

    /// Corners of the photo on screen: (top-left, bottom-right).
    pub fn rect(&self) -> (DVec2, DVec2) {
        (self.to_screen(DVec2::ZERO), self.to_screen(self.size))
    }
}

#[derive(Clone, Debug)]
pub struct Camera {
    pub target: DVec3,
    pub distance: f64,
    /// Around Z, radians.
    pub yaw: f64,
    /// Above the horizon, radians.
    pub pitch: f64,
    pub fov_y: f64,
    pub perspective: bool,
    /// Rotation about the view direction, radians (0 = the horizon is level). Only a matched
    /// photo gives a non-zero value; orbiting levels the view again.
    pub roll: f64,
    /// Where the optical centre is, in normalized device coordinates (0 = the middle of the
    /// viewport). Non-zero for a matched photo that is cropped or zoomed.
    pub shift: DVec2,
    /// Camera › Two-Point Perspective (SketchUp's "2D" camera flag, `CCamera` +0xd0): the
    /// view stays horizontal and vertical lines stay parallel.
    pub two_point: bool,
    /// Offset of the image plane, in half viewport sizes (+y moves the picture up). SketchUp
    /// keeps it in the camera's "centre 2D" (reset to 0 when two-point is switched on).
    pub lens_shift: DVec2,
    /// Viewport size in physical pixels.
    pub size: DVec2,
}

impl Default for Camera {
    fn default() -> Self {
        // The view saved in SketchUp 2026's "Simple" templates: eye 1.88 m above the
        // ground, in front and to the right of the origin, looking slightly down.
        let mut c = Camera {
            target: DVec3::ZERO,
            distance: 1.0,
            yaw: 0.0,
            pitch: 0.0,
            fov_y: 35.0f64.to_radians(),
            perspective: true,
            roll: 0.0,
            shift: DVec2::ZERO,
            two_point: false,
            lens_shift: DVec2::ZERO,
            size: DVec2::new(1280.0, 800.0),
        };
        c.set_from_view(&esq_skp::View {
            eye: DVec3::new(4805.213, -6098.650, 1880.048),
            target: DVec3::new(-8452.837, 14351.420, -983.443),
            up: DVec3::Z,
            fov_deg: 35.0,
            perspective: true,
        });
        c
    }
}

impl Camera {
    pub fn forward(&self) -> DVec3 {
        -DVec3::new(self.pitch.cos() * self.yaw.cos(), self.pitch.cos() * self.yaw.sin(), self.pitch.sin())
    }

    pub fn eye(&self) -> DVec3 {
        self.target - self.forward() * self.distance
    }

    /// Right and up of a level view (no roll). Past the vertical (an orbit with gravity
    /// suspended), the view is upside down rather than turned half a turn.
    fn level_axes(&self) -> (DVec3, DVec3) {
        let mut right = self.forward().cross(DVec3::Z).normalize_or(DVec3::X);
        if self.pitch.cos() < 0.0 {
            right = -right;
        }
        (right, right.cross(self.forward()))
    }

    pub fn right(&self) -> DVec3 {
        let (r, u) = self.level_axes();
        if self.roll == 0.0 { r } else { r * self.roll.cos() + u * self.roll.sin() }
    }

    pub fn up(&self) -> DVec3 {
        let (r, u) = self.level_axes();
        if self.roll == 0.0 { u } else { u * self.roll.cos() - r * self.roll.sin() }
    }

    pub fn view(&self) -> DMat4 {
        view::look_at_mat4(self.eye(), self.target, self.up())
    }

    pub fn aspect(&self) -> f64 {
        (self.size.x / self.size.y.max(1.0)).max(1e-3)
    }

    /// Reversed-Z projection (near = 1, far = 0) with an infinite far plane.
    pub fn proj(&self) -> DMat4 {
        let mut p = if self.perspective {
            let near = (self.distance * 1e-3).clamp(0.5, 100.0);
            let p = directx::perspective_infinite_reverse(self.fov_y, self.aspect(), near);
            if self.shift == DVec2::ZERO { p } else { DMat4::from_translation(self.shift.extend(0.0)) * p }
        } else {
            let h = self.distance * (self.fov_y * 0.5).tan();
            let w = h * self.aspect();
            let depth = self.distance * 100.0 + 1e6;
            directx::orthographic(-w, w, -h, h, depth, -depth)
        };
        // Lens shift: translate the image in clip space (x += sx·w, y += sy·w).
        let s = self.lens_shift;
        if s != DVec2::ZERO {
            for c in [&mut p.x_axis, &mut p.y_axis, &mut p.z_axis, &mut p.w_axis] {
                c.x += s.x * c.w;
                c.y += s.y * c.w;
            }
        }
        p
    }

    /// Whether the view is a two-point perspective: SketchUp tests for a perspective camera
    /// with the 2D flag whose viewing direction is horizontal.
    pub fn is_two_point(&self) -> bool {
        self.perspective && self.two_point && self.pitch.sin().abs() < 1e-6
    }

    /// Camera › Two-Point Perspective, on: perspective, looking horizontally from the same
    /// eye at the same distance, image plane centred (as `CSketchView`'s setter does).
    pub fn enable_two_point(&mut self) {
        let eye = self.eye();
        self.perspective = true;
        self.pitch = 0.0;
        self.target = eye + self.forward() * self.distance;
        self.two_point = true;
        self.lens_shift = DVec2::ZERO;
    }

    pub fn disable_two_point(&mut self) {
        self.two_point = false;
        self.lens_shift = DVec2::ZERO;
    }

    pub fn view_proj(&self) -> DMat4 {
        self.proj() * self.view()
    }

    /// World point → pixel position (origin top-left) and depth along the view direction.
    pub fn project(&self, p: DVec3) -> Option<DVec2> {
        self.projector().project(p)
    }

    /// `project` for many points: the view's matrix computed once.
    pub fn projector(&self) -> Projector {
        Projector { view_proj: self.view_proj(), size: self.size }
    }

    /// Ray through a pixel: (origin, unit direction).
    pub fn ray(&self, px: DVec2) -> (DVec3, DVec3) {
        let mut ndc = DVec2::new(px.x / self.size.x * 2.0 - 1.0, 1.0 - px.y / self.size.y * 2.0) - self.lens_shift;
        if self.perspective {
            ndc -= self.shift;
            let h = (self.fov_y * 0.5).tan();
            let dir = (self.forward() + self.right() * ndc.x * h * self.aspect() + self.up() * ndc.y * h).normalize();
            (self.eye(), dir)
        } else {
            let h = self.distance * (self.fov_y * 0.5).tan();
            let o = self.target + self.right() * ndc.x * h * self.aspect() + self.up() * ndc.y * h
                - self.forward() * self.distance * 50.0;
            (o, self.forward())
        }
    }

    /// Ray from the camera towards a world point: (origin, unit direction, distance to it).
    pub fn ray_to(&self, p: DVec3) -> (DVec3, DVec3, f64) {
        if self.perspective {
            let d = p - self.eye();
            let len = d.length().max(1e-9);
            (self.eye(), d / len, len)
        } else {
            let back = self.distance * 50.0;
            (p - self.forward() * back, self.forward(), back)
        }
    }

    /// World size of one pixel at the target distance.
    pub fn pixel_size(&self) -> f64 {
        2.0 * self.distance * (self.fov_y * 0.5).tan() / self.size.y.max(1.0)
    }

    /// World size of one pixel at point `p` (depth-dependent in perspective).
    pub fn pixel_size_at(&self, p: DVec3) -> f64 {
        if !self.perspective {
            return self.pixel_size();
        }
        let depth = (p - self.eye()).dot(self.forward()).max(1e-6);
        2.0 * depth * (self.fov_y * 0.5).tan() / self.size.y.max(1.0)
    }

    pub fn orbit(&mut self, dx: f64, dy: f64) {
        self.orbit_with(dx, dy, true);
    }

    /// Orbit; without `gravity` (CCameraOrbitTool with Ctrl held) the camera may go past the
    /// vertical and turn upside down. With gravity, a view upside down is righted first.
    pub fn orbit_with(&mut self, dx: f64, dy: f64, gravity: bool) {
        self.roll = 0.0;
        self.shift = DVec2::ZERO;
        if gravity && self.pitch.cos() < 0.0 {
            // Same viewing direction, upright.
            self.yaw += std::f64::consts::PI;
            self.pitch = std::f64::consts::PI.copysign(self.pitch) - self.pitch;
        }
        self.yaw -= dx * 0.008;
        // Two-point perspective keeps the view horizontal.
        if !(self.two_point && self.perspective) {
            let p = self.pitch + dy * 0.008;
            self.pitch = if gravity {
                p.clamp(-1.55, 1.55)
            } else {
                // Kept within (-π, π].
                (p + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
            };
        }
    }

    /// Orbits around a pivot (the point under the cursor) instead of the target, gravity
    /// suspended or not (see [`Camera::orbit_with`]).
    pub fn orbit_around_with(&mut self, pivot: DVec3, dx: f64, dy: f64, gravity: bool) {
        if gravity && self.pitch.cos() < 0.0 {
            self.orbit_with(0.0, 0.0, true);
        }
        let old_right = self.right();
        let old = (self.yaw, self.pitch);
        self.orbit_with(dx, dy, gravity);
        // Rotate the whole camera frame about the pivot: yaw about Z, then pitch about the
        // (rotated) right axis.
        let rz = DMat4::from_rotation_z(self.yaw - old.0);
        let rp = DMat4::from_axis_angle(rz.transform_vector3(old_right), -(self.pitch - old.1));
        self.target = pivot + (rp * rz).transform_vector3(self.target - pivot);
    }

    pub fn pan(&mut self, dx: f64, dy: f64) {
        let s = self.pixel_size();
        if self.is_two_point() {
            // Panning vertically slides the image plane instead of tilting the camera.
            self.target += -self.right() * dx * s;
            self.lens_shift.y -= dy * 2.0 / self.size.y.max(1.0);
            return;
        }
        self.target += (-self.right() * dx + self.up() * dy) * s;
    }

    /// Zooms by `factor` (<1 = closer) keeping the point under the cursor fixed.
    pub fn zoom_at(&mut self, factor: f64, anchor: Option<DVec3>) {
        let factor = factor.clamp(0.2, 5.0);
        if let Some(a) = anchor {
            self.target = a + (self.target - a) * factor;
        }
        self.distance = (self.distance * factor).clamp(1.0, 1e9);
    }

    pub fn zoom_extents(&mut self, min: DVec3, max: DVec3) {
        if !min.is_finite() || !max.is_finite() {
            self.target = DVec3::ZERO;
            self.distance = 9000.0;
            return;
        }
        let center = (min + max) * 0.5;
        let radius = ((max - min).length() * 0.5).max(100.0);
        self.target = center;
        let half = (self.fov_y * 0.5).min((self.fov_y * 0.5).atan2(1.0 / self.aspect()));
        self.distance = radius / half.sin() * 1.05;
    }

    pub fn set_view(&mut self, yaw_deg: f64, pitch_deg: f64) {
        self.roll = 0.0;
        self.shift = DVec2::ZERO;
        self.yaw = yaw_deg.to_radians();
        self.pitch = pitch_deg.to_radians();
    }

    /// Point of view for a .skp file.
    pub fn to_view(&self) -> esq_skp::View {
        esq_skp::View {
            eye: self.eye(),
            target: self.target,
            up: self.up(),
            fov_deg: self.fov_y.to_degrees(),
            perspective: self.perspective,
        }
    }

    pub fn set_from_view(&mut self, v: &esq_skp::View) {
        let d = v.eye - v.target;
        let len = d.length();
        if len < 1e-6 {
            return;
        }
        self.target = v.target;
        self.distance = len;
        self.yaw = d.y.atan2(d.x);
        self.pitch = (d.z / len).clamp(-1.0, 1.0).asin().clamp(-1.55, 1.55);
        self.fov_y = v.fov_deg.clamp(5.0, 120.0).to_radians();
        self.perspective = v.perspective;
        self.shift = DVec2::ZERO;
        // Roll: the angle between the level "up" and the saved one.
        self.roll = 0.0;
        let (right, up) = self.level_axes();
        let want = v.up.normalize_or(DVec3::Z);
        if (want - up).length() > 1e-9 {
            self.roll = (-want.dot(right)).atan2(want.dot(up));
        }
    }

    /// The view of a matched photo, fitted in a viewport of `viewport` points: the photo is
    /// `zoom` times as large as the biggest size that fits, `pan` moves its centre. Also
    /// returns the map from photo pixels to viewport points.
    pub fn from_photo(
        photo: &esq_core::matchphoto::PhotoCamera,
        viewport: DVec2,
        zoom: f64,
        pan: DVec2,
    ) -> (Camera, PhotoMap) {
        let mut c = Camera { size: viewport, ..Default::default() };
        c.set_from_view(&photo.scene_camera(photo.trans.length().max(1.0)));
        let map = PhotoMap::new(photo, viewport, zoom, pan);
        c.fov_y = map.fov_y;
        c.shift = map.shift;
        (c, map)
    }

    /// True if this camera looks from the same place in the same direction as the one of a
    /// matched photo (the zoom and the optical centre are not compared).
    pub fn is_photo_view(&self, photo: &esq_core::matchphoto::PhotoCamera) -> bool {
        let scale = photo.trans.length().max(1.0);
        self.perspective
            && self.eye().distance(photo.eye()) < 1e-6 * scale
            && self.forward().distance(photo.forward()) < 1e-7
            && self.up().distance(photo.up()) < 1e-7
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_point_keeps_eye_and_distance_and_levels_the_view() {
        let mut c = Camera::default();
        let (eye, dist) = (c.eye(), c.distance);
        assert!(!c.is_two_point());
        c.enable_two_point();
        assert!(c.is_two_point());
        assert!((c.eye() - eye).length() < 1e-6);
        assert!((c.distance - dist).abs() < 1e-9);
        assert!(c.forward().z.abs() < 1e-12);
    }

    #[test]
    fn two_point_verticals_stay_vertical_on_screen() {
        let mut c = Camera::default();
        c.enable_two_point();
        // Vertical lines project to vertical lines, however far up or down the picture.
        let base = c.target + c.right() * 1500.0 + c.forward() * 2000.0;
        let low = c.project(base).unwrap();
        let high = c.project(base + DVec3::Z * 3000.0).unwrap();
        assert!((low.x - high.x).abs() < 1e-6, "{low} {high}");
        // A tilted camera makes them converge.
        let t = Camera { pitch: 0.4, ..Default::default() };
        let base = t.target + t.right() * 1500.0;
        let a = t.project(base).unwrap();
        let b = t.project(base + DVec3::Z * 3000.0).unwrap();
        assert!((a.x - b.x).abs() > 1.0);
    }

    #[test]
    fn two_point_pan_slides_the_image_instead_of_tilting() {
        let mut c = Camera::default();
        c.enable_two_point();
        let target = c.target;
        c.pan(0.0, 100.0);
        assert!(c.is_two_point());
        assert_eq!(c.target, target);
        assert!(c.lens_shift.y < 0.0);
        // The point that was at the centre has moved down by 100 px.
        let p = c.project(target).unwrap();
        assert!((p.y - (c.size.y * 0.5 + 100.0)).abs() < 1e-6, "{p}");
        // Orbit keeps the view horizontal.
        c.orbit(30.0, 50.0);
        assert!(c.is_two_point());
        // Rays go through the shifted image.
        let p = c.project(target).unwrap();
        let (o, d) = c.ray(p);
        let to = (target - o).normalize();
        assert!(d.dot(to) > 0.999999);
    }

    #[test]
    fn two_point_ends_when_leaving_perspective_or_tilting() {
        let mut c = Camera::default();
        c.enable_two_point();
        c.perspective = false;
        assert!(!c.is_two_point());
        c.perspective = true;
        c.pitch = 0.2;
        assert!(!c.is_two_point());
    }
}
