//! Display Crosshairs (`display crosshairs`, Preferences › Drawing in SketchUp): lines along
//! the drawing axes, in the axes' colours, through the point the tool infers under the cursor.
//!
//! As SketchUp draws them: each line crosses the whole view; when the tool measures from a
//! start point along one of the axes, that axis shows as two rays going out from the start
//! and from the cursor, leaving the measured stretch in view.

use esq_core::glam::{DMat4, DVec2, DVec3, DVec4};

use crate::camera::Camera;
use crate::pick::Snap;

/// Whether the crosshairs show (Preferences › Drawing, off by default as in SketchUp).
pub fn shown() -> bool {
    crate::settings::with(|s| s.crosshairs)
}

/// Turns them on or off (a preference, remembered between sessions).
pub fn set_shown(v: bool) {
    crate::settings::update(|s| s.crosshairs = v);
}
/// A line or ray in screen pixels and its colour.
pub type Stroke2 = (DVec2, DVec2, [u8; 3]);

/// The visible part, in pixels, of the line `p + t·dir` for `t` in `[t0, t1]` (infinite
/// bounds allowed): clipped in front of the eye, then to the view.
pub fn screen_span(cam: &Camera, p: DVec3, dir: DVec3, t0: f64, t1: f64) -> Option<(DVec2, DVec2)> {
    let vp = cam.view_proj();
    let a = vp * DVec4::new(p.x, p.y, p.z, 1.0);
    let d = vp * dir.extend(0.0);
    // Far enough to reach a vanishing point (perspective) or the view's edge (parallel).
    let far = 1e7 * (cam.distance.max(1.0) + (p - cam.target).length());
    let (mut lo, mut hi) = (t0.max(-far), t1.min(far));
    // In front of the eye: w > ε.
    let eps = 1e-6 * a.w.abs().max(1e-3);
    if d.w.abs() < 1e-15 {
        if a.w <= eps {
            return None;
        }
    } else {
        let t = (eps - a.w) / d.w;
        if d.w > 0.0 { lo = lo.max(t) } else { hi = hi.min(t) }
    }
    if lo >= hi {
        return None;
    }
    let px = |t: f64| {
        let c = a + d * t;
        let ndc = c.truncate() / c.w;
        DVec2::new((ndc.x + 1.0) * 0.5 * cam.size.x, (1.0 - ndc.y) * 0.5 * cam.size.y)
    };
    clip_to_view(px(lo), px(hi), cam.size)
}

/// Liang–Barsky clipping of a segment to the view rectangle (with a small margin).
fn clip_to_view(a: DVec2, b: DVec2, size: DVec2) -> Option<(DVec2, DVec2)> {
    let (min, max) = (DVec2::splat(-2.0), size + DVec2::splat(2.0));
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-d.x, a.x - min.x), (d.x, max.x - a.x), (-d.y, a.y - min.y), (d.y, max.y - a.y)] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 { t0 = t0.max(r) } else { t1 = t1.min(r) }
        }
    }
    (t0 < t1).then(|| (a + d * t0, a + d * t1))
}

/// The crosshairs for the tool's inference `s`, along the drawing `axes`.
pub fn lines(cam: &Camera, s: &Snap, axes: &DMat4) -> Vec<Stroke2> {
    let mut out = Vec::new();
    for i in 0..3 {
        // Preferences › Accessibility: the axis colours.
        let color = &crate::settings::axis_color(i);
        let axis = axes.col(i).truncate().normalize_or_zero();
        if axis == DVec3::ZERO {
            continue;
        }
        // Measuring from a start point along this axis: a ray out of each end.
        let along = s.guide.and_then(|(start, end, _)| {
            let v = end - start;
            let len = v.length();
            let k = v.dot(axis);
            (len > 1e-9 && (k.abs() - len).abs() <= 1e-6 * len).then_some((start, end, k.signum()))
        });
        let spans = match along {
            Some((start, end, sign)) => vec![
                screen_span(cam, start, axis * -sign, 0.0, f64::INFINITY),
                screen_span(cam, end, axis * sign, 0.0, f64::INFINITY),
            ],
            None => vec![screen_span(cam, s.pos, axis, f64::NEG_INFINITY, f64::INFINITY)],
        };
        out.extend(spans.into_iter().flatten().map(|(a, b)| (a, b, *color)));
    }
    out
}

/// Draws them over the view (`origin`: the view's top left on screen).
pub fn paint(painter: &eframe::egui::Painter, origin: eframe::egui::Pos2, cam: &Camera, s: &Snap, axes: &DMat4) {
    for (a, b, c) in lines(cam, s, axes) {
        let at = |p: DVec2| origin + eframe::egui::vec2(p.x as f32, p.y as f32);
        painter.line_segment([at(a), at(b)], eframe::egui::Stroke::new(1.0, crate::tools::c32(c)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pick::SnapKind;

    fn snap(pos: DVec3, guide: Option<(DVec3, DVec3, [u8; 3])>) -> Snap {
        Snap { pos, kind: SnapKind::OnFace, face: None, edge: None, guide }
    }

    fn cam() -> Camera {
        let mut c =
            Camera { size: DVec2::new(800.0, 600.0), target: DVec3::ZERO, distance: 5000.0, ..Default::default() };
        c.set_view(-60.0, 30.0);
        c
    }

    /// Distance in pixels from `p` to the line through `a` and `b`.
    fn off_line(p: DVec2, a: DVec2, b: DVec2) -> f64 {
        let d = (b - a).normalize();
        (p - a).perp_dot(d).abs()
    }

    #[test]
    fn three_axis_lines_cross_the_view_through_the_cursor_point() {
        let cam = cam();
        let p = DVec3::new(300.0, -200.0, 100.0);
        let ls = lines(&cam, &snap(p, None), &DMat4::IDENTITY);
        assert_eq!(ls.len(), 3);
        let at = cam.project(p).unwrap();
        for (i, (a, b, c)) in ls.iter().enumerate() {
            assert_eq!(*c, crate::settings::axis_color(i));
            assert!(off_line(at, *a, *b) < 0.01, "la ligne passe par le point");
            // The line is the axis seen on screen.
            let along = cam.project(p + DMat4::IDENTITY.col(i).truncate() * 10.0).unwrap();
            assert!(off_line(along, *a, *b) < 0.05);
            // It reaches the view's edge on at least one side (a vanishing point may end the
            // other one inside the view).
            let edge = |q: DVec2| q.x <= 0.0 || q.y <= 0.0 || q.x >= 800.0 || q.y >= 600.0;
            assert!(edge(*a) || edge(*b), "{a:?} {b:?}");
        }
        // The drawing axes of an open group turn them.
        let r = DMat4::from_rotation_z(0.5);
        let ls = lines(&cam, &snap(p, None), &r);
        let along = cam.project(p + r.col(0).truncate() * 10.0).unwrap();
        assert!(off_line(along, ls[0].0, ls[0].1) < 0.05);
    }

    #[test]
    fn measuring_along_an_axis_leaves_the_measured_stretch_free() {
        let cam = cam();
        let (start, end) = (DVec3::new(0.0, 0.0, 0.0), DVec3::new(1000.0, 0.0, 0.0));
        let ls = lines(&cam, &snap(end, Some((start, end, [0, 0, 0]))), &DMat4::IDENTITY);
        // Red: two rays; green and blue: one line each.
        assert_eq!(ls.iter().filter(|l| l.2 == crate::settings::axis_color(0)).count(), 2);
        assert_eq!(ls.len(), 4);
        let mid = cam.project(DVec3::new(500.0, 0.0, 0.0)).unwrap();
        let (s, e) = (cam.project(start).unwrap(), cam.project(end).unwrap());
        for (a, b, _) in ls.iter().filter(|l| l.2 == crate::settings::axis_color(0)) {
            // Each ray starts at one end and goes away from the other.
            let (from, other) = if a.distance(s) < 0.5 { (s, e) } else { (e, s) };
            assert!(a.distance(from) < 0.5, "{a:?}");
            assert!((b - from).dot(other - from) < 0.0);
            // The middle of the measured stretch is not on it.
            let t = (mid - *a).dot(*b - *a) / (*b - *a).length_squared();
            assert!(!(0.0..=1.0).contains(&t));
        }
        // Not along an axis: three whole lines through the cursor.
        let ls = lines(&cam, &snap(end, Some((DVec3::new(0.0, 500.0, 0.0), end, [0, 0, 0]))), &DMat4::IDENTITY);
        assert_eq!(ls.len(), 3);
    }

    #[test]
    fn a_line_behind_the_eye_is_cut_at_the_eye() {
        let mut cam = cam();
        cam.perspective = true;
        // A point right in front of the eye: its axis lines pass beside and behind the eye.
        let p = cam.eye() + cam.forward() * 200.0;
        let ls = lines(&cam, &snap(p, None), &DMat4::IDENTITY);
        for (a, b, _) in &ls {
            assert!(a.is_finite() && b.is_finite());
        }
        assert!(!ls.is_empty());
    }
}
