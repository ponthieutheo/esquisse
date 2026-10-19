//! Positioning a texture with pins (SketchUp's `CPositionTextureTool`, `CFaceTextureCoords`).
//!
//! A pin ties a point of the texture to a point of the face's plane. Like SketchUp, the map
//! from the texture to the plane is made from the first pins: one pin moves the texture (its
//! size and orientation are kept), two also rotate and scale it, three give any affine map,
//! four a perspective (the "distorted" texture). When the last pins are degenerate the map
//! is made from fewer of them.
//!
//! Units: plane points are millimetres along the face's texture axes ([`face_axes`]); texture
//! points are "texture millimetres", u × tile width and v × tile height (SketchUp keeps them
//! in model length too: one pin alone is then a pure translation).

use glam::{DMat3, DVec2, DVec3};

use crate::meta::{UvMap, face_axes};

/// One pin: `tex` (texture mm) is shown at `at` (plane mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pin {
    pub tex: DVec2,
    pub at: DVec2,
}

/// The plane of a face side, with the texture axes of that side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub ex: DVec3,
    pub ey: DVec3,
    pub n: DVec3,
    /// Offset of the plane along `n` (n·p for its points).
    pub c: f64,
}

impl Frame {
    /// The frame of the plane through `origin` facing `normal` (the side being textured).
    pub fn new(origin: DVec3, normal: DVec3) -> Frame {
        let n = normal.normalize_or_zero();
        let (ex, ey) = face_axes(n);
        Frame { ex, ey, n, c: n.dot(origin) }
    }

    /// The frame SketchUp lays the texture of a face side in (UV helper,
    /// `Face#position_material`, `uv_tile_at` of SketchUpAPI.dll): the face's
    /// plane, both sides alike; for a projected texture, the plane through the origin facing the
    /// projection direction (when it is a vector: SketchUp keeps it from a length of 1e-10).
    pub fn of_side(face_point: DVec3, face_normal: DVec3, projection: Option<DVec3>) -> Frame {
        match projection.filter(|d| d.length() >= 1e-10) {
            Some(d) => Frame::new(DVec3::ZERO, d),
            None => Frame::new(face_point, face_normal),
        }
    }

    pub fn to2(&self, p: DVec3) -> DVec2 {
        DVec2::new(p.dot(self.ex), p.dot(self.ey))
    }

    pub fn to3(&self, q: DVec2) -> DVec3 {
        self.ex * q.x + self.ey * q.y + self.n * self.c
    }
}

/// `m` applied to a point (with the perspective division).
pub fn apply(m: &DMat3, p: DVec2) -> DVec2 {
    let h = *m * p.extend(1.0);
    let w = if h.z.abs() < 1e-300 { 1e-300f64.copysign(h.z) } else { h.z };
    DVec2::new(h.x / w, h.y / w)
}

fn from_rows(r: [[f64; 3]; 3]) -> DMat3 {
    DMat3::from_cols(
        DVec3::new(r[0][0], r[1][0], r[2][0]),
        DVec3::new(r[0][1], r[1][1], r[2][1]),
        DVec3::new(r[0][2], r[1][2], r[2][2]),
    )
}

/// The matrix (texture mm → plane) made from the pins, trying all of them, then fewer.
pub fn homography(pins: &[Pin]) -> Option<DMat3> {
    (1..=pins.len().min(4)).rev().find_map(|k| from_pins(&pins[..k]))
}

/// Exactly `pins.len()` pins (1 to 4); `None` when they are degenerate.
pub fn from_pins(pins: &[Pin]) -> Option<DMat3> {
    let ok = |m: DMat3| (m.determinant().abs() > 1e-12 && m.is_finite()).then_some(m);
    match pins {
        [p] => {
            let d = p.at - p.tex;
            Some(from_rows([[1.0, 0.0, d.x], [0.0, 1.0, d.y], [0.0, 0.0, 1.0]]))
        }
        [a, b] => {
            let frame = |p: DVec2, q: DVec2| {
                let d = q - p;
                from_rows([[d.x, -d.y, p.x], [d.y, d.x, p.y], [0.0, 0.0, 1.0]])
            };
            let (ft, fa) = (frame(a.tex, b.tex), frame(a.at, b.at));
            let tol = |p: DVec2, q: DVec2| p.distance(q) > 1e-9 * (1.0 + p.length().max(q.length()));
            if !tol(a.tex, b.tex) || !tol(a.at, b.at) {
                return None;
            }
            ok(fa * ft.inverse())
        }
        [a, b, c] => {
            let frame = |p: DVec2, q: DVec2, r: DVec2| {
                let (u, v) = (q - p, r - p);
                from_rows([[u.x, v.x, p.x], [u.y, v.y, p.y], [0.0, 0.0, 1.0]])
            };
            let flat = |p: DVec2, q: DVec2, r: DVec2| {
                let (u, v) = (q - p, r - p);
                (u.x * v.y - u.y * v.x).abs() <= 1e-9 * u.length() * v.length() + 1e-15
            };
            if flat(a.tex, b.tex, c.tex) || flat(a.at, b.at, c.at) {
                return None;
            }
            ok(frame(a.at, b.at, c.at) * frame(a.tex, b.tex, c.tex).inverse())
        }
        [a, b, c, d] => {
            // Through the unit square: pins 0, 1, 3, 2 go round it.
            let qt = square_to_quad([a.tex, b.tex, d.tex, c.tex])?;
            let qa = square_to_quad([a.at, b.at, d.at, c.at])?;
            ok(qa * qt.inverse())
        }
        _ => None,
    }
}

/// The perspective that sends (0,0), (1,0), (1,1), (0,1) to `q` (Heckbert's formula).
fn square_to_quad(q: [DVec2; 4]) -> Option<DMat3> {
    let [p0, p1, p2, p3] = q;
    let s = p0 - p1 + p2 - p3;
    let size = (p1 - p0).length().max((p3 - p0).length()).max(1e-300);
    let (g, h) = if s.length() <= 1e-12 * size {
        (0.0, 0.0)
    } else {
        let (d1, d2) = (p1 - p2, p3 - p2);
        let den = d1.x * d2.y - d2.x * d1.y;
        if den.abs() <= 1e-12 * size * size {
            return None;
        }
        ((s.x * d2.y - d2.x * s.y) / den, (d1.x * s.y - s.x * d1.y) / den)
    };
    let m = from_rows([
        [p1.x - p0.x + g * p1.x, p3.x - p0.x + h * p3.x, p0.x],
        [p1.y - p0.y + g * p1.y, p3.y - p0.y + h * p3.y, p0.y],
        [g, h, 1.0],
    ]);
    (m.determinant().abs() > 1e-12 * size * size && m.is_finite()).then_some(m)
}

/// The texture map of a face side whose plane point (x, y) shows texture mm `g`·(x, y, 1);
/// `tile` is the texture's tile size (mm).
pub fn map_from_plane_matrix(ex: DVec3, ey: DVec3, g: DMat3, tile: [f64; 2]) -> Option<UvMap> {
    if !g.is_finite() {
        return None;
    }
    let (w, h) = (tile[0].max(1e-9), tile[1].max(1e-9));
    let row = |i: usize, k: f64| {
        let (a, b, c) = (g.x_axis[i], g.y_axis[i], g.z_axis[i]);
        let d = (ex * a + ey * b) / k;
        [d.x, d.y, d.z, c / k]
    };
    // Scaled so that q is (0, 0, 0, 1) for an affine map.
    let s = g.z_axis[2];
    let s = if s.abs() > 1e-300 { s } else { 1.0 };
    let div = |r: [f64; 4]| r.map(|x| x / s);
    Some(UvMap { u: div(row(0, w)), v: div(row(1, h)), q: div(row(2, 1.0)) })
}

/// The matrix (plane → texture mm) of a texture map on the plane of `frame`.
pub fn plane_matrix(map: &UvMap, frame: &Frame, tile: [f64; 2]) -> DMat3 {
    let row = |c: &[f64; 4], k: f64| {
        let d = DVec3::new(c[0], c[1], c[2]);
        [d.dot(frame.ex) * k, d.dot(frame.ey) * k, (d.dot(frame.n) * frame.c + c[3]) * k]
    };
    from_rows([row(&map.u, tile[0]), row(&map.v, tile[1]), row(&map.q, 1.0)])
}

/// The texture map made from the pins.
pub fn map_from_pins(pins: &[Pin], frame: &Frame, tile: [f64; 2]) -> Option<UvMap> {
    let h = homography(pins)?;
    map_from_plane_matrix(frame.ex, frame.ey, h.inverse(), tile)
}

/// `Face#position_material` (`SUFacePositionMaterial`, of
/// SketchUpAPI.dll): each model point (mm) is taken in `frame` (dropping its height above the
/// plane: along the projection for a projected texture), each texture point (tiles) in texture
/// length, and the map is solved from exactly these pairs (1 to 4, the first four counting).
/// `None` when they are degenerate: SketchUp then refuses the positioning.
pub fn map_from_pairs(frame: &Frame, tile: [f64; 2], pairs: &[(DVec3, DVec2)]) -> Option<UvMap> {
    let k = pairs.len().min(4);
    if k == 0 {
        return None;
    }
    let pins: Vec<Pin> = pairs[..k]
        .iter()
        .map(|(p, uv)| Pin { tex: DVec2::new(uv.x * tile[0], uv.y * tile[1]), at: frame.to2(*p) })
        .collect();
    let h = from_pins(&pins)?;
    map_from_plane_matrix(frame.ex, frame.ey, h.inverse(), tile)
}

/// Millimetres per inch: SketchUp keeps texture placements in inches.
const IN: f64 = 25.4;

fn inch_scale() -> DMat3 {
    DMat3::from_diagonal(DVec3::new(IN, IN, 1.0))
}

/// The map of a side placed by SketchUp (`CFaceTextureCoords`): `matrix` as the file keeps it
/// (nine doubles, the columns of the map from texture inches to the frame's inches, a
/// perspective when its last row is not (0, 0, 1)), `frame` from [`Frame::of_side`], `tile`
/// the texture's tile (mm). SketchUp's UV helper inverts it
/// and divides by the tile. `None` when the matrix cannot be inverted.
pub fn map_from_sketchup(frame: &Frame, matrix: &[f64; 9], tile: [f64; 2]) -> Option<UvMap> {
    let m = DMat3::from_cols_array(matrix);
    if !m.is_finite() || m.determinant().abs() < 1e-300 {
        return None;
    }
    let h = inch_scale() * m * inch_scale().inverse();
    map_from_plane_matrix(frame.ex, frame.ey, h.inverse(), tile)
}

/// The matrix SketchUp keeps for a map ([`map_from_sketchup`] backwards), scaled so that its
/// last coefficient is 1 (as SketchUp writes an affine placement).
pub fn sketchup_matrix(map: &UvMap, frame: &Frame, tile: [f64; 2]) -> Option<[f64; 9]> {
    let g = plane_matrix(map, frame, tile);
    if !g.is_finite() || g.determinant().abs() < 1e-300 {
        return None;
    }
    let m = inch_scale().inverse() * g.inverse() * inch_scale();
    let k = m.z_axis.z;
    let m = if k.abs() > 1e-12 { m * (1.0 / k) } else { m };
    m.is_finite().then(|| m.to_cols_array())
}

/// The four fixed pins of SketchUp, at the corners of the texture tile under `reference`
/// (plane point): red, green, blue, yellow = bottom left, bottom right, top left, top right.
pub fn tile_pins(g: &DMat3, tile: [f64; 2], reference: DVec2) -> Option<Vec<Pin>> {
    let inv = g.inverse();
    if !inv.is_finite() {
        return None;
    }
    let r = apply(g, reference);
    let (w, h) = (tile[0].max(1e-9), tile[1].max(1e-9));
    let (i, j) = ((r.x / w).floor(), (r.y / h).floor());
    let pins = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
        .map(|(a, b)| {
            let tex = DVec2::new((i + a) * w, (j + b) * h);
            Pin { tex, at: apply(&inv, tex) }
        })
        .to_vec();
    pins.iter().all(|p| p.at.is_finite()).then_some(pins)
}

/// Four pins at the corners of the box of `pts` (plane points), where SketchUp puts them
/// when the tile corners are not usable (a distorted texture, or no corner on screen).
pub fn box_pins(g: &DMat3, pts: &[DVec2]) -> Vec<Pin> {
    let lo = pts.iter().fold(DVec2::splat(f64::INFINITY), |a, p| a.min(*p));
    let hi = pts.iter().fold(DVec2::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
    [(lo.x, lo.y), (hi.x, lo.y), (lo.x, hi.y), (hi.x, hi.y)]
        .map(|(x, y)| {
            let at = DVec2::new(x, y);
            Pin { tex: apply(g, at), at }
        })
        .to_vec()
}

/// What dragging pin `i` does with the fixed pins: 0 moves the texture, 1 scales and rotates
/// it about pin 0, 2 scales and shears it (the line from pin 0 to pin 1 stays), 3 distorts
/// it. The other pins follow so that only pin `i` changes the map. False when the drag would
/// make the map degenerate (nothing is changed then).
pub fn drag_fixed(pins: &mut [Pin], i: usize, to: DVec2) -> bool {
    if i >= pins.len() {
        return false;
    }
    let p0 = pins[0].at;
    let m: Option<DMat3> = match i {
        0 => Some(DMat3::from_translation(to - p0)),
        1 => {
            let (a, b) = (pins[1].at - p0, to - p0);
            if a.length() < 1e-9 || b.length() < 1e-9 {
                return false;
            }
            let angle = (a.x * b.y - a.y * b.x).atan2(a.dot(b));
            let s = b.length() / a.length();
            Some(
                DMat3::from_translation(p0)
                    * DMat3::from_scale(DVec2::splat(s))
                    * DMat3::from_angle(angle)
                    * DMat3::from_translation(-p0),
            )
        }
        2 => {
            let d = pins[1].at - p0;
            if d.length() < 1e-9 {
                return false;
            }
            let dn = d / d.length();
            let perp = DVec2::new(-dn.y, dn.x);
            let dist = (pins[2].at - p0).dot(perp);
            if dist.abs() < 1e-9 {
                return false;
            }
            let delta = to - pins[2].at;
            let (k, s) = (dn.dot(delta) / dist, 1.0 + perp.dot(delta) / dist);
            if s.abs() < 1e-9 {
                return false;
            }
            // In the frame (pin 0; d, perp): (a, b) -> (a + k b, s b).
            let basis = from_rows([[dn.x, perp.x, 0.0], [dn.y, perp.y, 0.0], [0.0, 0.0, 1.0]]);
            let shear = from_rows([[1.0, k, 0.0], [0.0, s, 0.0], [0.0, 0.0, 1.0]]);
            Some(DMat3::from_translation(p0) * basis * shear * basis.transpose() * DMat3::from_translation(-p0))
        }
        _ => None,
    };
    let old: Vec<Pin> = pins.to_vec();
    if let Some(m) = m {
        for p in pins.iter_mut().skip(i + 1) {
            p.at = apply(&m, p.at);
        }
    }
    pins[i].at = to;
    if homography(pins).is_none() {
        pins.copy_from_slice(&old);
        return false;
    }
    true
}

/// Free pins: only the dragged pin moves.
pub fn drag_free(pins: &mut [Pin], i: usize, to: DVec2) -> bool {
    let Some(p) = pins.get_mut(i) else { return false };
    let old = p.at;
    p.at = to;
    if homography(pins).is_none() {
        pins[i].at = old;
        return false;
    }
    true
}

/// Moves the whole texture (dragging it, the arrow keys).
pub fn translate(pins: &mut [Pin], d: DVec2) {
    for p in pins {
        p.at += d;
    }
}

/// Flip and Rotate of the Position Texture menu: `m` (2×2) applied about the first pin.
pub fn transform_about_first(pins: &mut [Pin], m: glam::DMat2) {
    let Some(p0) = pins.first().map(|p| p.at) else { return };
    for p in pins {
        p.at = p0 + m * (p.at - p0);
    }
}

/// Flip › Left/Right (`horizontal`) or Up/Down.
pub fn flip(pins: &mut [Pin], horizontal: bool) {
    let m = if horizontal {
        glam::DMat2::from_diagonal(DVec2::new(-1.0, 1.0))
    } else {
        glam::DMat2::from_diagonal(DVec2::new(1.0, -1.0))
    };
    transform_about_first(pins, m);
}

/// Rotate › 90, 180, 270: turns the texture clockwise as seen from the side being positioned.
pub fn rotate(pins: &mut [Pin], degrees: f64) {
    transform_about_first(pins, glam::DMat2::from_angle(-degrees.to_radians()));
}

/// Whether a texture map has a perspective (a distorted texture).
pub fn is_distorted(map: &UvMap) -> bool {
    !map.is_affine()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(s: f64, t: f64, x: f64, y: f64) -> Pin {
        Pin { tex: DVec2::new(s, t), at: DVec2::new(x, y) }
    }

    fn honoured(pins: &[Pin]) {
        let h = from_pins(pins).unwrap();
        for p in pins {
            assert!((apply(&h, p.tex) - p.at).length() < 1e-7, "{p:?} -> {:?}", apply(&h, p.tex));
        }
    }

    #[test]
    fn one_to_four_pins() {
        let all = [
            pin(0.0, 0.0, 10.0, 20.0),
            pin(100.0, 0.0, 10.0, 220.0),
            pin(0.0, 50.0, -40.0, 20.0),
            pin(100.0, 50.0, -70.0, 260.0),
        ];
        for k in 1..=4 {
            honoured(&all[..k]);
        }
        // One pin keeps the size: a pure translation.
        let h = from_pins(&all[..1]).unwrap();
        assert!((apply(&h, DVec2::new(30.0, 0.0)) - DVec2::new(40.0, 20.0)).length() < 1e-12);
        // Two pins: a similarity (here a quarter turn, scale 2).
        let h = from_pins(&all[..2]).unwrap();
        assert!((apply(&h, DVec2::new(0.0, 50.0)) - DVec2::new(-90.0, 20.0)).length() < 1e-9);
        // Four pins out of a parallelogram: a perspective.
        let h = from_pins(&all).unwrap();
        assert!(h.x_axis.z.abs() + h.y_axis.z.abs() > 1e-9);
    }

    #[test]
    fn degenerate_pins_fall_back_to_fewer() {
        let pins = [pin(0.0, 0.0, 0.0, 0.0), pin(10.0, 0.0, 10.0, 0.0), pin(20.0, 0.0, 20.0, 0.0)];
        assert!(from_pins(&pins).is_none(), "three pins on a line");
        let h = homography(&pins).unwrap();
        assert!((apply(&h, DVec2::new(5.0, 7.0)) - DVec2::new(5.0, 7.0)).length() < 1e-9);
    }

    fn square() -> Vec<Pin> {
        vec![
            pin(0.0, 0.0, 0.0, 0.0),
            pin(100.0, 0.0, 100.0, 0.0),
            pin(0.0, 100.0, 0.0, 100.0),
            pin(100.0, 100.0, 100.0, 100.0),
        ]
    }

    #[test]
    fn red_pin_moves_everything() {
        let mut p = square();
        assert!(drag_fixed(&mut p, 0, DVec2::new(30.0, -10.0)));
        assert_eq!(p[3].at, DVec2::new(130.0, 90.0));
        let h = homography(&p).unwrap();
        assert!((apply(&h, DVec2::new(50.0, 50.0)) - DVec2::new(80.0, 40.0)).length() < 1e-9);
    }

    #[test]
    fn green_pin_scales_and_rotates_about_red() {
        let mut p = square();
        assert!(drag_fixed(&mut p, 1, DVec2::new(0.0, 200.0)));
        assert_eq!(p[0].at, DVec2::ZERO);
        assert!((p[2].at - DVec2::new(-200.0, 0.0)).length() < 1e-9, "{:?}", p[2].at);
        assert!((p[3].at - DVec2::new(-200.0, 200.0)).length() < 1e-9);
    }

    #[test]
    fn blue_pin_shears_and_scales_keeping_the_bottom() {
        let mut p = square();
        assert!(drag_fixed(&mut p, 2, DVec2::new(40.0, 50.0)));
        assert_eq!(p[0].at, DVec2::ZERO);
        assert_eq!(p[1].at, DVec2::new(100.0, 0.0));
        assert!((p[3].at - DVec2::new(140.0, 50.0)).length() < 1e-9, "{:?}", p[3].at);
    }

    #[test]
    fn yellow_pin_distorts() {
        let mut p = square();
        assert!(drag_fixed(&mut p, 3, DVec2::new(60.0, 60.0)));
        assert_eq!(&p[..3], &square()[..3]);
        let h = homography(&p).unwrap();
        assert!(h.x_axis.z.abs() + h.y_axis.z.abs() > 1e-9, "perspective");
        assert!((apply(&h, DVec2::new(100.0, 100.0)) - DVec2::new(60.0, 60.0)).length() < 1e-9);
    }

    #[test]
    fn flip_and_rotate_about_red() {
        let mut p = square();
        flip(&mut p, true);
        assert!((p[1].at - DVec2::new(-100.0, 0.0)).length() < 1e-12);
        let mut p = square();
        rotate(&mut p, 90.0);
        assert!((p[1].at - DVec2::new(0.0, -100.0)).length() < 1e-9, "clockwise: {:?}", p[1].at);
    }

    #[test]
    fn maps_round_trip_through_the_plane() {
        let f = Frame::new(DVec3::new(0.0, 0.0, 300.0), DVec3::Z);
        let mut p = square();
        drag_fixed(&mut p, 3, DVec2::new(70.0, 90.0));
        let map = map_from_pins(&p, &f, [100.0, 100.0]).unwrap();
        assert!(!map.is_affine());
        for q in &p {
            let uv = map.at(f.to3(q.at));
            assert!((uv * 100.0 - q.tex).length() < 1e-7, "{q:?} -> {uv:?}");
        }
        let g = plane_matrix(&map, &f, [100.0, 100.0]);
        assert!((apply(&g, DVec2::new(70.0, 90.0)) - DVec2::new(100.0, 100.0)).length() < 1e-7);
        let back = map.point_at(DVec2::new(0.5, 0.5), f.to3(DVec2::ZERO), DVec3::Z).unwrap();
        assert!((map.at(back) - DVec2::new(0.5, 0.5)).length() < 1e-9);
        assert!((back.z - 300.0).abs() < 1e-9);
    }

    /// A projected texture is laid in the plane through the origin facing the projection: its
    /// coordinates do not change along the projection, wherever the face is.
    #[test]
    fn a_projected_map_is_constant_along_the_projection() {
        let dir = DVec3::new(0.0, -1.0, 1.0);
        let face = Frame::of_side(DVec3::new(0.0, 0.0, 250.0), DVec3::Z, Some(dir));
        assert!(face.c.abs() < 1e-12 && (face.n - dir.normalize()).length() < 1e-12);
        let pairs = [
            (DVec3::new(0.0, 0.0, 0.0), DVec2::new(0.0, 0.0)),
            (DVec3::new(300.0, 0.0, 0.0), DVec2::new(1.0, 0.0)),
            (DVec3::new(400.0, 500.0, 0.0), DVec2::new(1.0, 1.0)),
            (DVec3::new(-200.0, 500.0, 0.0), DVec2::new(0.0, 1.0)),
        ];
        let map = map_from_pairs(&face, [100.0, 100.0], &pairs).unwrap();
        assert!(!map.is_affine(), "four pairs: distorted");
        for (p, uv) in &pairs {
            for k in [-300.0, 0.0, 700.0] {
                let q = *p + dir * k;
                assert!((map.at(q) - *uv).length() < 1e-9, "{q:?} -> {:?}", map.at(q));
            }
        }
        // Without the projection, the face's own plane: constant along its normal.
        let flat = map_from_pairs(&Frame::of_side(DVec3::ZERO, DVec3::Z, None), [100.0, 100.0], &pairs).unwrap();
        let p = DVec3::new(120.0, 80.0, 0.0);
        assert!((flat.at(p + DVec3::Z * 900.0) - flat.at(p)).length() < 1e-9);
        assert!((map.at(p + DVec3::Z * 900.0) - map.at(p)).length() > 1e-3);
    }

    /// The matrix SketchUp keeps (texture inches → frame inches) and back.
    #[test]
    fn sketchup_matrices_round_trip() {
        let frame = Frame::of_side(DVec3::new(10.0, 20.0, 30.0), DVec3::new(1.0, 2.0, 2.0), None);
        // Texture inches (s, t) → frame inches: a shear, a translation and a perspective.
        let m = [2.0, 0.5, 0.001, 0.3, 1.5, -0.002, 4.0, -3.0, 1.0];
        let map = map_from_sketchup(&frame, &m, [254.0, 127.0]).unwrap();
        let back = sketchup_matrix(&map, &frame, [254.0, 127.0]).unwrap();
        for (a, b) in m.iter().zip(&back) {
            assert!((a - b).abs() < 1e-9, "{m:?} {back:?}");
        }
        // Texture point (s, t) inches is at frame point M (s, t, 1): uv = (s / 10, t / 5).
        let (s, t) = (7.0, -2.0);
        let h = DMat3::from_cols_array(&m) * DVec3::new(s, t, 1.0);
        let p = frame.to3(DVec2::new(h.x / h.z, h.y / h.z) * IN);
        assert!((map.at(p) - DVec2::new(s / 10.0, t / 5.0)).length() < 1e-9, "{:?}", map.at(p));
    }

    #[test]
    fn fixed_pins_sit_on_the_tile_under_the_point() {
        let g = DMat3::IDENTITY;
        let p = tile_pins(&g, [254.0, 508.0], DVec2::new(300.0, -10.0)).unwrap();
        assert_eq!(p[0].tex, DVec2::new(254.0, -508.0));
        assert_eq!(p[3].tex, DVec2::new(508.0, 0.0));
        let b = box_pins(&g, &[DVec2::new(5.0, 6.0), DVec2::new(-3.0, 9.0)]);
        assert_eq!(b[0].at, DVec2::new(-3.0, 6.0));
        assert_eq!(b[3].at, DVec2::new(5.0, 9.0));
    }
}
