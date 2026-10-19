//! More drawing tools: Freehand, Arc (centre), 3-Point Arc, Pie, Rotated Rectangle.

use eframe::egui::Color32;
use esq_core::arcs::{Sides, arc_points, check_arc, parse_sides};
use esq_core::glam::{DVec2, DVec3};
use esq_core::math::{Plane, TOL};
use esq_core::model::Units;
use esq_core::units::format_length;

use super::{ClickDrag, Overlay, Tool, ToolCx, ToolId, split_values};
use crate::pick::Snap;

const PREVIEW: Color32 = Color32::from_rgba_premultiplied(48, 56, 102, 50);
/// Distance to the start of a freehand stroke under which it closes.
const CLOSE_PX: f64 = 12.0;

/// Projection of a snapped point on a plane, or where the cursor ray meets it.
fn on_plane(cx: &ToolCx, m: DVec2, s: &Snap, pl: &Plane) -> DVec3 {
    use crate::pick::SnapKind;
    if matches!(s.kind, SnapKind::Ground | SnapKind::Free | SnapKind::OnFace) {
        let (ro, rd) = cx.cam().ray(m);
        if let Some(t) = pl.intersect_ray(ro, rd).filter(|t| *t > 0.0) {
            return ro + rd * t;
        }
    }
    s.pos - pl.n * pl.distance(s.pos)
}

fn parse_angle(t: &str) -> Result<f64, String> {
    t.trim()
        .trim_end_matches('°')
        .replace(',', ".")
        .parse::<f64>()
        .map(f64::to_radians)
        .map_err(|_| "Angle invalide".into())
}

// ---------------------------------------------------------------------------------------

/// Freehand: press and drag to draw a curve on the face under the cursor (or the ground).
#[derive(Default)]
pub struct FreehandTool {
    plane: Option<Plane>,
    pts: Vec<DVec3>,
    last_px: DVec2,
    snap: Option<Snap>,
    /// The cursor is back near the start: releasing closes the curve.
    closing: bool,
}

impl Tool for FreehandTool {
    fn id(&self) -> ToolId {
        ToolId::Freehand
    }
    fn status(&self) -> String {
        // CFreehandTool ("Curve Closed" is a tooltip in SketchUp, see `overlay`).
        format!(
            "Cliquez et faites glisser pour dessiner une courbe à main levée. | {}",
            super::hints::ARROWS_DRAWING_PLANE
        )
    }
    fn busy(&self) -> bool {
        self.plane.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let s = cx.snap(m, None, self.plane);
        if let Some(pl) = self.plane
            && m.distance(self.last_px) > 5.0
        {
            let p = on_plane(cx, m, &s, &pl);
            self.pts.push(p);
            self.last_px = m;
        }
        // Near the start of a long enough stroke the curve closes ("Curve Closed").
        self.closing = self.plane.is_some()
            && self.pts.len() > 3
            && cx.cam().project(self.pts[0]).is_some_and(|p| p.distance(m) < CLOSE_PX);
        self.snap = Some(s);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        let s = cx.snap(m, None, None);
        let mut pl = cx.plane_at(&s);
        // An arrow key locks the drawing plane perpendicular to that axis.
        if let Some(l) = cx.lock.filter(|l| matches!(l.kind, crate::pick::SnapKind::Axis(_))) {
            pl = Plane::from_point_normal(s.pos, l.dir.normalize());
        }
        self.plane = Some(pl);
        self.pts = vec![s.pos];
        self.last_px = m;
        self.snap = Some(s);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.closing = false;
        let Some(_) = self.plane.take() else { return };
        let mut pts = std::mem::take(&mut self.pts);
        pts.dedup_by(|a, b| a.distance(*b) < TOL);
        if pts.len() < 2 {
            return;
        }
        // Ends close to where it started: a closed loop (and a face).
        let start_px = cx.cam().project(pts[0]).unwrap_or_default();
        let closed = pts.len() > 3 && start_px.distance(m) < CLOSE_PX;
        if closed {
            pts.pop();
        }
        let local: Vec<DVec3> = pts.iter().map(|p| cx.to_local(*p)).collect();
        cx.doc.commit("Main levée", |md, d| {
            md.entities_mut(d).add_polyline(&local, closed, true);
        });
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.plane = None;
        self.pts.clear();
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        o.polyline(&self.pts, false, Color32::BLACK, 2.0);
        if self.closing
            && let Some(p) = self.pts.last()
        {
            o.text_at(*p, "Courbe fermée");
        }
    }
}

// ---------------------------------------------------------------------------------------

/// Arc (centre, start, angle) and Pie (the same, closed by two radii).
pub struct ArcCenterTool {
    pie: bool,
    sides: Sides,
    center: Option<(DVec3, Plane)>,
    start: Option<(DVec3, f64)>,
    angle: f64,
    snap: Option<Snap>,
    cd: ClickDrag,
}

impl ArcCenterTool {
    pub fn new(pie: bool) -> Self {
        ArcCenterTool {
            pie,
            sides: Sides::default(),
            center: None,
            start: None,
            angle: 0.0,
            snap: None,
            cd: ClickDrag::default(),
        }
    }

    fn points(&self) -> Option<Vec<DVec3>> {
        let (c, pl) = self.center?;
        let (dir, r) = self.start?;
        if self.angle.abs() < 1e-6 {
            return None;
        }
        let segs = self.sides.segments(self.angle);
        Some(arc_points(c, pl.n, dir, r, self.angle, segs))
    }

    fn finish(&mut self, cx: &mut ToolCx) {
        let Some(mut pts) = self.points() else { return };
        if let Some((_, r)) = self.start
            && let Err(msg) = check_arc(r, self.angle, pts.len() - 1)
        {
            *cx.message = Some(msg);
            return;
        }
        let c = self.center.unwrap().0;
        let full = self.angle.abs() >= std::f64::consts::TAU - 1e-9;
        if full {
            pts.pop();
        }
        let local: Vec<DVec3> = pts.iter().map(|p| cx.to_local(*p)).collect();
        let (pie, cl) = (self.pie, cx.to_local(c));
        cx.doc.commit(if pie { "Portion de cercle" } else { "Arc" }, |m, d| {
            let e = m.entities_mut(d);
            e.add_polyline(&local, full, true);
            if pie && !full {
                e.add_line(cl, local[0]);
                e.add_line(cl, *local.last().unwrap());
            }
        });
        self.center = None;
        self.start = None;
        self.angle = 0.0;
    }
}

impl Tool for ArcCenterTool {
    fn step(&mut self, cx: &mut ToolCx, delta: i32) {
        *cx.message = Some(match self.sides.stepped(delta) {
            Ok(s) => {
                self.sides = s;
                s.status()
            }
            Err(msg) => msg,
        });
    }
    fn id(&self) -> ToolId {
        if self.pie { ToolId::Pie } else { ToolId::ArcCenter }
    }
    fn status(&self) -> String {
        // CArc3PointTool / CArc3PointPieTool (centre, start, end).
        let seg = super::hints::SEGMENTS;
        match (self.center.is_some(), self.start.is_some()) {
            (false, _) => format!("Sélectionnez le point central. {seg}"),
            (true, false) => format!("Sélectionnez le premier point d'arc ou entrez un rayon. {seg}"),
            _ => format!("Sélectionnez le deuxième point d'arc ou entrez un angle. {seg}"),
        }
    }
    fn vcb_label(&self) -> &'static str {
        match (self.center.is_some(), self.start.is_some()) {
            (false, _) => "Segments",
            (true, false) => "Rayon",
            _ => "Angle",
        }
    }
    fn vcb_value(&self, units: Units) -> String {
        match (self.center, self.start, &self.snap) {
            (None, _, _) => self.sides.vcb(),
            (Some((c, _)), None, Some(s)) => format_length(c.distance(s.pos), units),
            (Some(_), Some(_), _) => esq_core::units::format_angle(self.angle.to_degrees().abs()),
            _ => String::new(),
        }
    }
    fn busy(&self) -> bool {
        self.center.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let s = cx.snap(m, self.center.map(|c| c.0), self.center.map(|c| c.1));
        if let (Some((c, pl)), Some((dir, _))) = (self.center, self.start) {
            let p = on_plane(cx, m, &s, &pl);
            let d = (p - c).normalize_or_zero();
            if d != DVec3::ZERO {
                // Follows the mouse continuously, so the arc can go past half a turn.
                let a = dir.cross(d).dot(pl.n).atan2(dir.dot(d));
                let mut best = a;
                for k in [-1.0, 1.0] {
                    let cand = a + k * std::f64::consts::TAU;
                    if (cand - self.angle).abs() < (best - self.angle).abs() {
                        best = cand;
                    }
                }
                let step = 15f64.to_radians();
                let snapped = (best / step).round() * step;
                if (best - snapped).abs() < 1.5f64.to_radians() {
                    best = snapped;
                }
                self.angle = best.clamp(-std::f64::consts::TAU, std::f64::consts::TAU);
            }
        }
        self.snap = Some(s);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(s) = self.snap.clone() else { return };
        let started = self.center.is_none();
        match (self.center, self.start) {
            (None, _) => self.center = Some((s.pos, cx.plane_at(&s))),
            (Some((c, pl)), None) => {
                let p = on_plane(cx, m, &s, &pl);
                if p.distance(c) > TOL {
                    self.start = Some(((p - c).normalize(), p.distance(c)));
                    self.angle = 0.0;
                }
            }
            _ => self.finish(cx),
        }
        self.cd.press(m, started);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.cd.release(m)
            && self.start.is_none()
            && let (Some((c, pl)), Some(s)) = (self.center, self.snap.clone())
        {
            let p = on_plane(cx, m, &s, &pl);
            if p.distance(c) > TOL {
                self.start = Some(((p - c).normalize(), p.distance(c)));
            }
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        if let Some(sides) = parse_sides(text) {
            self.sides = sides?;
            *cx.message = Some(self.sides.status());
            return Ok(());
        }
        match (self.center, self.start) {
            (Some((c, pl)), None) => {
                let r = cx.parse_len(text)?.abs();
                let dir = self
                    .snap
                    .as_ref()
                    .map(|s| s.pos - pl.n * pl.distance(s.pos) - c)
                    .and_then(|v| v.try_normalize())
                    .unwrap_or_else(|| pl.basis().0);
                self.start = Some((dir, r));
                Ok(())
            }
            (Some(_), Some(_)) => {
                let a = parse_angle(text)?;
                self.angle = if self.angle < 0.0 { -a.abs() } else { a.abs() };
                self.finish(cx);
                Ok(())
            }
            _ => Err("Cliquez d'abord le centre".into()),
        }
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.center = None;
        self.start = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        let Some((c, pl)) = self.center else { return };
        match (self.start, &self.snap) {
            (None, Some(s)) => o.dashed(c, s.pos - pl.n * pl.distance(s.pos), Color32::from_gray(60), 1.5),
            (Some((dir, r)), _) => {
                o.dashed(c, c + dir * r, Color32::from_gray(90), 1.0);
                if let Some(pts) = self.points() {
                    if self.pie {
                        let mut poly = vec![c];
                        poly.extend(pts.iter().copied());
                        o.fill_polygon(&poly, PREVIEW);
                        o.line(c, pts[0], Color32::BLACK, 2.0);
                        o.line(c, *pts.last().unwrap(), Color32::BLACK, 2.0);
                    }
                    o.polyline(&pts, false, Color32::BLACK, 2.0);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------

/// Circle through three points: centre and normal.
fn circumcircle(a: DVec3, b: DVec3, c: DVec3) -> Option<(DVec3, DVec3, f64)> {
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    let n2 = n.length_squared();
    if n2 < 1e-9 {
        return None;
    }
    let centre = a + (n.cross(ab) * ac.length_squared() + ac.cross(n) * ab.length_squared()) / (2.0 * n2);
    Some((centre, n / n2.sqrt(), centre.distance(a)))
}

/// 3-Point Arc: start, a point on the arc, end.
#[derive(Default)]
pub struct Arc3Tool {
    p1: Option<(DVec3, Plane)>,
    p2: Option<DVec3>,
    sides: Sides,
    snap: Option<Snap>,
    cd: ClickDrag,
}

/// The circle of a 3-Point Arc (CFit3PointArcTool): through the start `a`, the second point `b`
/// and `end`, with the sweep from `a` to `end`.
struct Arc3Circle {
    centre: DVec3,
    /// Normal: a → b → end turns counter-clockwise around it.
    n: DVec3,
    radius: f64,
    /// Unit vector from the centre to the start.
    start: DVec3,
    /// Angle from the start to `end`, counter-clockwise (0..2π): the arc passes through `b`.
    sweep: f64,
}

impl Arc3Circle {
    fn new(a: DVec3, b: DVec3, end: DVec3) -> Option<Self> {
        let (centre, n, radius) = circumcircle(a, b, end)?;
        let start = (a - centre) / radius;
        let d = (end - centre) / radius;
        let sweep = n.cross(start).dot(d).atan2(start.dot(d)).rem_euclid(std::f64::consts::TAU);
        Some(Arc3Circle { centre, n, radius, start, sweep })
    }
}

impl Arc3Tool {
    /// The arc toward `end`; `sweep` replaces the angle to `end` (an angle typed after the
    /// second point keeps the circle through the cursor and sets the arc's angle).
    fn points(&self, end: DVec3, sweep: Option<f64>) -> Option<Vec<DVec3>> {
        let (a, _) = self.p1?;
        let k = Arc3Circle::new(a, self.p2?, end)?;
        let sweep = sweep.unwrap_or(k.sweep);
        let seg = self.sides.segments(sweep);
        Some(arc_points(k.centre, k.n, k.start, k.radius, sweep, seg))
    }

    /// The circle while the end point is being chosen (the cursor is the end).
    fn circle(&self) -> Option<Arc3Circle> {
        Arc3Circle::new(self.p1?.0, self.p2?, self.snap.as_ref()?.pos)
    }

    fn finish(&mut self, cx: &mut ToolCx, end: DVec3, sweep: Option<f64>) {
        let (Some((a, _)), Some(b)) = (self.p1, self.p2) else { return };
        let Some(k) = Arc3Circle::new(a, b, end) else {
            *cx.message = Some("Rayon nul refusé.".into());
            return;
        };
        let Some(pts) = self.points(end, sweep) else { return };
        if let Err(msg) = check_arc(k.radius, sweep.unwrap_or(k.sweep), pts.len() - 1) {
            *cx.message = Some(msg);
            return;
        }
        let local: Vec<DVec3> = pts.iter().map(|p| cx.to_local(*p)).collect();
        cx.doc.commit("Arc par 3 points", |m, d| {
            m.entities_mut(d).add_polyline(&local, false, true);
        });
        self.p1 = None;
        self.p2 = None;
    }
}

impl Tool for Arc3Tool {
    fn id(&self) -> ToolId {
        ToolId::Arc3
    }
    fn status(&self) -> String {
        // CFit3PointArcTool: "%s | %s".
        use super::hints::{ARROWS_INFERENCE, SEGMENTS};
        let step = match (self.p1.is_some(), self.p2.is_some()) {
            (false, _) => "Cliquez pour définir le point de départ.",
            (true, false) => "Cliquez pour définir le second point ou entrez une longueur.",
            _ => "Cliquez pour définir le point d'arrivée ou entrez l'angle.",
        };
        format!("{step} | {SEGMENTS} | {ARROWS_INFERENCE}")
    }
    fn vcb_label(&self) -> &'static str {
        match (self.p1.is_some(), self.p2.is_some()) {
            (true, false) => "Longueur",
            (true, true) => "Angle",
            _ => "Segments",
        }
    }
    fn vcb_value(&self, units: Units) -> String {
        match (self.p1, self.p2, &self.snap) {
            (Some((a, _)), None, Some(s)) => esq_core::units::format_length(a.distance(s.pos), units),
            (Some(_), Some(_), _) => {
                self.circle().map(|k| esq_core::units::format_angle(k.sweep.to_degrees())).unwrap_or_default()
            }
            _ => self.sides.vcb(),
        }
    }
    fn busy(&self) -> bool {
        self.p1.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let reference = self.p2.or(self.p1.map(|p| p.0));
        self.snap = Some(cx.snap(m, reference, self.p1.map(|p| p.1)));
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(s) = self.snap.clone() else { return };
        let started = self.p1.is_none();
        match (self.p1, self.p2) {
            (None, _) => self.p1 = Some((s.pos, cx.plane_at(&s))),
            (Some((a, _)), None) => {
                if s.pos.distance(a) > TOL {
                    self.p2 = Some(s.pos);
                }
            }
            _ => self.finish(cx, s.pos, None),
        }
        self.cd.press(m, started);
    }
    fn step(&mut self, cx: &mut ToolCx, delta: i32) {
        *cx.message = Some(match self.sides.stepped(delta) {
            Ok(s) => {
                self.sides = s;
                s.status()
            }
            Err(msg) => msg,
        });
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        match parse_sides(text) {
            Some(sides) => {
                self.sides = sides?;
                *cx.message = Some(self.sides.status());
                Ok(())
            }
            None if self.p2.is_some() => {
                // After the second point, the arc's angle (CFit3PointArcTool: 0.001 rad to a
                // full turn): the arc keeps the circle through the cursor and ends at that angle.
                let sweep = parse_angle(text).map_err(|_| "L’angle saisi est incorrect.".to_string())?;
                if !(0.001..=std::f64::consts::TAU + 0.001).contains(&sweep) {
                    return Err("L’angle saisi est incorrect.".into());
                }
                let Some(end) = self.snap.as_ref().map(|s| s.pos) else {
                    return Err("Indiquez le cercle avec la souris".into());
                };
                if self.circle().is_none() {
                    return Err("Indiquez le cercle avec la souris".into());
                }
                self.finish(cx, end, Some(sweep.min(std::f64::consts::TAU)));
                Ok(())
            }
            None => {
                // The length from the start to the second point, toward the cursor.
                let (Some((a, _)), None, Some(s)) = (self.p1, self.p2, &self.snap) else {
                    return Err("Tapez le nombre de côtés, par exemple 12s ou 24c".into());
                };
                let len = cx.parse_len(text)?;
                let dir = (s.pos - a).normalize_or_zero();
                if dir == DVec3::ZERO || len <= TOL {
                    return Err("Longueur invalide.".into());
                }
                self.p2 = Some(a + dir * len);
                Ok(())
            }
        }
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.p1 = None;
        self.p2 = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        let Some(s) = &self.snap else { return };
        match (self.p1, self.p2) {
            (Some((a, _)), None) => o.dashed(a, s.pos, Color32::from_gray(60), 1.5),
            (Some(_), Some(_)) => {
                if let Some(pts) = self.points(s.pos, None) {
                    o.polyline(&pts, false, Color32::BLACK, 2.0);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------

/// Rotated Rectangle: first side (any direction), then the width (any angle around it).
#[derive(Default)]
pub struct RotRectTool {
    p1: Option<(DVec3, Plane)>,
    p2: Option<DVec3>,
    width: Option<DVec3>,
    snap: Option<Snap>,
    cd: ClickDrag,
}

impl RotRectTool {
    fn corners(&self) -> Option<[DVec3; 4]> {
        let (a, _) = self.p1?;
        let b = self.p2?;
        let w = self.width?;
        Some([a, b, b + w, a + w])
    }

    fn width_from(&self, q: DVec3) -> Option<DVec3> {
        let (a, _) = self.p1?;
        let b = self.p2?;
        let d = (b - a).normalize();
        let v = q - b;
        let w = v - d * v.dot(d);
        (w.length() > TOL).then_some(w)
    }

    fn finish(&mut self, cx: &mut ToolCx) {
        let Some(c) = self.corners() else { return };
        let local: Vec<DVec3> = c.iter().map(|p| cx.to_local(*p)).collect();
        cx.doc.commit("Rectangle orienté", |m, d| {
            m.entities_mut(d).add_polyline(&local, true, false);
        });
        self.p1 = None;
        self.p2 = None;
        self.width = None;
    }
}

impl Tool for RotRectTool {
    fn id(&self) -> ToolId {
        ToolId::RotRect
    }
    fn status(&self) -> String {
        match (self.p1.is_some(), self.p2.is_some()) {
            (false, _) => "Cliquez sur le premier coin.".into(),
            (true, false) => "Cliquez sur le deuxième coin (premier côté) ou tapez sa longueur.".into(),
            _ => "Cliquez pour fixer la largeur (et l'inclinaison) ou tapez la largeur.".into(),
        }
    }
    fn vcb_label(&self) -> &'static str {
        if self.p2.is_some() { "Largeur" } else { "Longueur" }
    }
    fn vcb_value(&self, units: Units) -> String {
        match (self.p1, self.p2, &self.snap) {
            (Some(_), Some(_), _) => self.width.map(|w| format_length(w.length(), units)).unwrap_or_default(),
            (Some((a, _)), None, Some(s)) => format_length(a.distance(s.pos), units),
            _ => String::new(),
        }
    }
    fn busy(&self) -> bool {
        self.p1.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let reference = self.p2.or(self.p1.map(|p| p.0));
        let plane = if self.p2.is_none() { self.p1.map(|p| p.1) } else { None };
        let s = cx.snap(m, reference, plane);
        if self.p2.is_some() {
            // Free tilt around the first side; on the plane by default.
            let q = if matches!(s.kind, crate::pick::SnapKind::Ground | crate::pick::SnapKind::Free) {
                let pl = self.p1.unwrap().1;
                on_plane(cx, m, &s, &pl)
            } else {
                s.pos
            };
            self.width = self.width_from(q);
        }
        self.snap = Some(s);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(s) = self.snap.clone() else { return };
        let started = self.p1.is_none();
        match (self.p1, self.p2) {
            (None, _) => self.p1 = Some((s.pos, cx.plane_at(&s))),
            (Some((a, _)), None) => {
                if s.pos.distance(a) > TOL {
                    self.p2 = Some(s.pos);
                }
            }
            _ => self.finish(cx),
        }
        self.cd.press(m, started);
    }
    fn release(&mut self, _cx: &mut ToolCx, m: DVec2) {
        if self.cd.release(m)
            && self.p2.is_none()
            && let (Some((a, _)), Some(s)) = (self.p1, self.snap.clone())
            && s.pos.distance(a) > TOL
        {
            self.p2 = Some(s.pos);
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let parts = split_values(text);
        match (self.p1, self.p2) {
            (Some((a, _)), None) => {
                let len = cx.parse_len(&parts[0])?;
                let dir = self.snap.as_ref().and_then(|s| (s.pos - a).try_normalize());
                let Some(dir) = dir else { return Err("Indiquez une direction avec la souris".into()) };
                self.p2 = Some(a + dir * len);
                Ok(())
            }
            (Some(_), Some(_)) => {
                let w = cx.parse_len(&parts[0])?;
                let Some(cur) = self.width else { return Err("Indiquez la direction de la largeur".into()) };
                self.width = Some(cur.normalize() * w);
                self.finish(cx);
                Ok(())
            }
            _ => Err("Cliquez d'abord le premier coin".into()),
        }
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.p1 = None;
        self.p2 = None;
        self.width = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        match (self.p1, self.p2, &self.snap) {
            (Some((a, _)), None, Some(s)) => o.line(a, s.pos, Color32::BLACK, 2.0),
            (Some(_), Some(_), _) => {
                if let Some(c) = self.corners() {
                    o.fill_polygon(&c, PREVIEW);
                    o.polyline(&c, true, Color32::BLACK, 2.0);
                } else if let (Some((a, _)), Some(b)) = (self.p1, self.p2) {
                    o.line(a, b, Color32::BLACK, 2.0);
                }
            }
            _ => {}
        }
    }
}
