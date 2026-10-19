//! Drawing tools: line, rectangle, circle/polygon, two-point arc.

use eframe::egui::Color32;
use esq_core::angle::SNAP_TOLERANCE_PX;
use esq_core::arcs::{
    Fillet, Sides, arc_points, check_arc, fillet, fillet_max_radius, fillet_radius_for_apex,
    fillet_radius_for_tangent_length, parse_sides, tangent_bulge,
};
use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::math::{Plane, TOL};
use esq_core::model::{EdgeId, EntityRef, Units};
use esq_core::units::format_length;

use super::{ClickDrag, Overlay, Tool, ToolCx, ToolId, c32, circle_points, split_values};
use crate::pick::{Linear, Snap, SnapKind};

fn parse_triplet(cx: &ToolCx, s: &str) -> Result<DVec3, String> {
    let inner = &s[1..s.len() - 1];
    let parts: Vec<&str> = if inner.contains(';') { inner.split(';').collect() } else { inner.split(',').collect() };
    if parts.len() != 3 {
        return Err("Format attendu : [x; y; z] ou <dx; dy; dz>".into());
    }
    let v: Result<Vec<f64>, String> = parts.iter().map(|p| cx.parse_len(p)).collect();
    let v = v?;
    Ok(DVec3::new(v[0], v[1], v[2]))
}

// ---------------------------------------------------------------------------------------

#[derive(Default)]
pub struct LineTool {
    start: Option<DVec3>,
    snap: Option<Snap>,
    cd: ClickDrag,
    /// Alt while drawing (CSketchTool): the linear inferences offered, and when they were last
    /// changed (a tooltip says so for 5 seconds).
    linear: Linear,
    linear_changed: Option<std::time::Instant>,
    last_m: DVec2,
}

impl LineTool {
    /// The French of CSketchTool's "All On", "Parallel/Perpendicular Only", "All Off".
    fn linear_label(&self) -> &'static str {
        match self.linear {
            Linear::All => "Tous activés",
            Linear::ParallelPerpendicular => "Parallèle/perpendiculaire uniquement",
            Linear::Off => "Tous désactivés",
        }
    }

    fn click(&mut self, cx: &mut ToolCx, end: DVec3) {
        let Some(start) = self.start else {
            self.start = Some(end);
            return;
        };
        if start.distance(end) < TOL {
            return;
        }
        let (a, b) = (cx.to_local(start), cx.to_local(end));
        let faces_before = cx.doc.ents().faces.len();
        cx.doc.commit("Ligne", |m, d| {
            m.entities_mut(d).add_line(a, b);
        });
        // Closing a loop creates a face and ends the chain, as in SketchUp; so does every
        // line without Preferences › Drawing › Continue line drawing.
        let closed = cx.doc.ents().faces.len() > faces_before;
        self.start = if closed || !crate::settings::with(|s| s.continue_lines) { None } else { Some(end) };
    }
}

impl Tool for LineTool {
    fn id(&self) -> ToolId {
        ToolId::Line
    }
    fn status(&self) -> String {
        // CSketchTool: "%s = %s (%s)." for Alt while a line is being drawn.
        let step = if self.start.is_none() {
            "Cliquez pour définir la première extrémité."
        } else {
            "Cliquez pour définir la deuxième extrémité ou entrez une longueur."
        };
        let alt = if self.start.is_some() {
            format!(" | Alt = Activer/désactiver les inférences linéaires ({}).", self.linear_label())
        } else {
            String::new()
        };
        format!("{step}{alt} | {}", super::hints::ARROWS_INFERENCE)
    }
    fn alt_tap(&mut self, cx: &mut ToolCx) {
        // CSketchTool: only while drawing and without a locked direction; All On → All Off →
        // Parallel/Perpendicular Only → All On.
        if self.start.is_none() || cx.lock.is_some() {
            return;
        }
        self.linear = match self.linear {
            Linear::All => Linear::Off,
            Linear::Off => Linear::ParallelPerpendicular,
            Linear::ParallelPerpendicular => Linear::All,
        };
        self.linear_changed = Some(std::time::Instant::now());
        let m = self.last_m;
        self.mouse_move(cx, m);
    }
    fn vcb_label(&self) -> &'static str {
        "Longueur"
    }
    fn vcb_value(&self, units: Units) -> String {
        match (self.start, &self.snap) {
            (Some(a), Some(s)) => format_length(a.distance(s.pos), units),
            _ => String::new(),
        }
    }
    fn busy(&self) -> bool {
        self.start.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.last_m = m;
        self.snap = Some(cx.snap_linear(m, self.start, None, self.linear));
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        let started = self.start.is_none();
        self.mouse_move(cx, m);
        if let Some(s) = self.snap.clone() {
            self.click(cx, s.pos);
        }
        self.cd.press(m, started);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.cd.release(m)
            && let Some(s) = self.snap.clone()
        {
            self.click(cx, s.pos);
            self.start = None;
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let text = text.trim();
        let start = self.start;
        if text.starts_with('[') && text.ends_with(']') {
            let p = cx.doc.ctx_xform().transform_point3(parse_triplet(cx, text)?);
            self.click(cx, p);
            return Ok(());
        }
        let Some(start) = start else { return Err("Cliquez d'abord le point de départ".into()) };
        if text.starts_with('<') && text.ends_with('>') {
            let d = cx.doc.ctx_xform().transform_vector3(parse_triplet(cx, text)?);
            self.click(cx, start + d);
            return Ok(());
        }
        let len = cx.parse_len(text)?;
        let dir = match &self.snap {
            Some(s) if s.pos.distance(start) > TOL => (s.pos - start).normalize(),
            _ => return Err("Indiquez une direction avec la souris".into()),
        };
        self.click(cx, start + dir * len);
        Ok(())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.start = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        if let (Some(a), Some(s)) = (self.start, &self.snap) {
            let col = s.guide.map(|g| c32(g.2)).unwrap_or(Color32::BLACK);
            o.line(a, s.pos, col, 2.0);
            // CSketchTool: for 5 seconds after Alt, what is left of the linear inferences.
            let text = match self.linear {
                Linear::All => None,
                Linear::ParallelPerpendicular => Some("Inférence parallèle/perpendiculaire uniquement"),
                Linear::Off => Some("Inférence linéaire (tous désactivés)"),
            };
            if let Some(t) = text.filter(|_| self.linear_changed.is_some_and(|i| i.elapsed().as_secs_f64() < 5.0)) {
                o.text_at(s.pos, t);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------

struct RectStart {
    p0: DVec3,
    plane: Plane,
    u: DVec3,
    v: DVec3,
}

/// The golden ratio, (sqrt(5) + 1) / 2: SketchUp's second rectangle inference.
const GOLDEN: f64 = std::f64::consts::GOLDEN_RATIO;
/// How near the cursor must be, on screen, to the square or golden corner to snap to it.
const RECT_INFER_PX: f64 = 6.0;

/// A proportion the rectangle snapped to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RectInfer {
    Square,
    Golden,
}

impl RectInfer {
    fn label(self) -> &'static str {
        match self {
            RectInfer::Square => "Carré",
            RectInfer::Golden => "Section d'or",
        }
    }
}

#[derive(Default)]
pub struct RectTool {
    start: Option<RectStart>,
    snap: Option<Snap>,
    /// Width and height of the rectangle (signed along u and v), as the Measurements box
    /// shows them: twice the distance to the cursor when drawing from the centre.
    dims: Option<(f64, f64)>,
    /// Ctrl: the first click is the centre.
    from_center: bool,
    infer: Option<RectInfer>,
    cd: ClickDrag,
}

impl RectTool {
    /// Corners for a rectangle `w` by `h`: from the first click, or around it.
    fn corners(&self, w: f64, h: f64) -> Option<Vec<DVec3>> {
        let s = self.start.as_ref()?;
        let o = if self.from_center { s.p0 - s.u * (w * 0.5) - s.v * (h * 0.5) } else { s.p0 };
        Some(vec![o, o + s.u * w, o + s.u * w + s.v * h, o + s.v * h])
    }

    /// SketchUp's square and golden-section inferences. `(w, h)` is the offset of the cursor
    /// from the first click. A rectangle that already has the proportion keeps it; otherwise
    /// the corner of the square (side = the smaller side) or of the golden rectangle (long
    /// side = golden ratio x the short one) within 6 pixels of the cursor wins. Points of
    /// the model under the cursor take precedence.
    fn infer_proportion(&self, cx: &ToolCx, m: DVec2, w: f64, h: f64, snap: &Snap) -> (f64, f64, Option<RectInfer>) {
        let Some(st) = &self.start else { return (w, h, None) };
        let (a, b) = (w.abs(), h.abs());
        if a < TOL || b < TOL {
            return (w, h, None);
        }
        let (sw, sh) = (w.signum(), h.signum());
        if (a - b).abs() < TOL {
            return (w, h, Some(RectInfer::Square));
        }
        let golden_gap = if b <= a { a - b * GOLDEN } else { a * GOLDEN - b };
        if golden_gap.abs() < TOL {
            return (w, h, Some(RectInfer::Golden));
        }
        if matches!(snap.kind, SnapKind::Endpoint | SnapKind::Midpoint | SnapKind::OnEdge | SnapKind::Intersection) {
            return (w, h, None);
        }
        let s = a.min(b);
        let golden = if b <= a { (sw * b * GOLDEN, sh * b) } else { (sw * a, sh * a * GOLDEN) };
        for (cand, kind) in [((sw * s, sh * s), RectInfer::Square), (golden, RectInfer::Golden)] {
            let p = st.p0 + st.u * cand.0 + st.v * cand.1;
            if cx.cam().project(p).is_some_and(|px| px.distance(m) <= RECT_INFER_PX) {
                return (cand.0, cand.1, Some(kind));
            }
        }
        (w, h, None)
    }

    fn finish(&mut self, cx: &mut ToolCx, w: f64, h: f64) {
        if w.abs() < TOL || h.abs() < TOL {
            return;
        }
        let Some(c) = self.corners(w, h) else { return };
        let local: Vec<DVec3> = c.iter().map(|p| cx.to_local(*p)).collect();
        cx.doc.commit("Rectangle", |m, d| {
            m.entities_mut(d).add_polyline(&local, true, false);
        });
        self.start = None;
        self.dims = None;
    }
}

impl Tool for RectTool {
    fn id(&self) -> ToolId {
        ToolId::Rect
    }
    fn status(&self) -> String {
        // CRectangleTool: "%s | %s | %s".
        let (step, ctrl) = match (self.start.is_none(), self.from_center) {
            (true, false) => ("Cliquez pour déterminer le premier coin.", "Activer/Désactiver sélectionner le centre."),
            (true, true) => ("Cliquez pour définir le centre.", "Activer/Désactiver sélectionner le centre."),
            (false, false) => (
                "Cliquez pour sélectionner l’angle opposé ou tapez la longueur, largeur.",
                "Activer/désactiver dessiner à partir du centre.",
            ),
            (false, true) => (
                "Cliquez pour définir l’angle ou tapez la longueur, largeur.",
                "Activer/désactiver dessiner à partir du centre.",
            ),
        };
        format!("{step} | Ctrl = {ctrl} | {}", super::hints::ARROWS_DRAWING_PLANE)
    }
    fn ctrl_tap(&mut self, cx: &mut ToolCx) {
        self.from_center = !self.from_center;
        // The cursor offset is now a half-extent or a full one.
        if let Some((w, h)) = self.dims {
            let k = if self.from_center { 2.0 } else { 0.5 };
            self.dims = Some((w * k, h * k));
        }
        *cx.message =
            Some(if self.from_center { "Rectangle depuis le centre" } else { "Rectangle depuis un coin" }.into());
    }
    fn vcb_label(&self) -> &'static str {
        "Dimensions"
    }
    fn vcb_value(&self, units: Units) -> String {
        self.dims
            .map(|(w, h)| format!("{}; {}", format_length(w.abs(), units), format_length(h.abs(), units)))
            .unwrap_or_default()
    }
    fn busy(&self) -> bool {
        self.start.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let plane = self.start.as_ref().map(|s| s.plane);
        // An arrow key locks the drawing plane, not a direction (CRectangleTool).
        let s = cx.infer.infer(m, self.start.as_ref().map(|s| s.p0), None, plane);
        self.infer = None;
        if let Some(st) = &self.start {
            let q = s.pos - st.plane.n * st.plane.distance(s.pos);
            let (w, h) = ((q - st.p0).dot(st.u), (q - st.p0).dot(st.v));
            let (w, h, infer) = self.infer_proportion(cx, m, w, h, &s);
            self.infer = infer;
            let k = if self.from_center { 2.0 } else { 1.0 };
            self.dims = Some((w * k, h * k));
        }
        self.snap = Some(s);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let started = self.start.is_none();
        if started {
            let Some(s) = self.snap.clone() else { return };
            let plane = match cx.lock {
                Some(l) if matches!(l.kind, SnapKind::Axis(_)) => Plane::from_point_normal(s.pos, l.dir),
                _ => cx.plane_at(&s),
            };
            let (u, v) = cx.plane_axes(plane.n);
            self.start = Some(RectStart { p0: s.pos, plane, u, v });
        } else if let Some((w, h)) = self.dims {
            self.finish(cx, w, h);
        }
        self.cd.press(m, started);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.cd.release(m)
            && let Some((w, h)) = self.dims
        {
            self.finish(cx, w, h);
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let parts = split_values(text);
        if parts.len() != 2 {
            return Err("Tapez « largeur;hauteur », par exemple 1200;600".into());
        }
        let (w, h) = (cx.parse_len(&parts[0])?, cx.parse_len(&parts[1])?);
        let (sw, sh) = self.dims.map(|(a, b)| (a.signum(), b.signum())).unwrap_or((1.0, 1.0));
        if self.start.is_none() {
            return Err("Cliquez d'abord le premier coin".into());
        }
        self.finish(cx, w * sw, h * sh);
        Ok(())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.start = None;
        self.dims = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        if let Some((w, h)) = self.dims
            && let Some(c) = self.corners(w, h)
        {
            o.fill_polygon(&c, Color32::from_rgba_unmultiplied(120, 140, 255, 50));
            o.polyline(&c, true, Color32::BLACK, 2.0);
            if let (Some(i), Some(s)) = (self.infer, &self.snap) {
                o.text_at(s.pos, i.label());
            }
        }
    }
}

// ---------------------------------------------------------------------------------------

pub struct CircleTool {
    polygon: bool,
    segments: usize,
    /// Polygon only: the radius reaches the middle of an edge instead of a vertex.
    circumscribed: bool,
    center: Option<(DVec3, Plane)>,
    snap: Option<Snap>,
    radius: f64,
    cd: ClickDrag,
}

impl CircleTool {
    pub fn new(polygon: bool) -> Self {
        CircleTool {
            polygon,
            segments: if polygon { 6 } else { 24 },
            circumscribed: false,
            center: None,
            snap: None,
            radius: 0.0,
            cd: ClickDrag::default(),
        }
    }

    fn points(&self) -> Option<Vec<DVec3>> {
        let (c, pl) = self.center?;
        let s = self.snap.as_ref()?;
        if self.radius < TOL {
            return None;
        }
        if self.polygon && self.circumscribed {
            // As SketchUp: the cursor marks the middle of an edge. The first vertex is half an
            // edge (tan(pi/n) * r) further along the circle's tangent, at r / cos(pi/n).
            let half = std::f64::consts::PI / self.segments as f64;
            let start = DMat4::from_axis_angle(pl.n, half).transform_vector3(s.pos - c);
            return Some(circle_points(c, pl.n, start, self.radius / half.cos(), self.segments));
        }
        Some(circle_points(c, pl.n, s.pos - c, self.radius, self.segments))
    }

    fn finish(&mut self, cx: &mut ToolCx) {
        let Some(pts) = self.points() else { return };
        let local: Vec<DVec3> = pts.iter().map(|p| cx.to_local(*p)).collect();
        let label = if self.polygon { "Polygone" } else { "Cercle" };
        cx.doc.commit(label, |m, d| {
            m.entities_mut(d).add_polyline(&local, true, true);
        });
        self.center = None;
        self.radius = 0.0;
    }
}

impl Tool for CircleTool {
    fn id(&self) -> ToolId {
        if self.polygon { ToolId::Polygon } else { ToolId::Circle }
    }
    fn status(&self) -> String {
        // CCircleTool / CPolyTool.
        let seg = super::hints::SEGMENTS;
        if self.center.is_none() {
            format!("Sélectionnez le point central. {seg}")
        } else if self.polygon {
            let ctrl = if self.circumscribed { "Ctrl = inscrit." } else { "Ctrl = circonscrit." };
            format!("Sélectionner un point sur le bord. {ctrl} {seg}")
        } else {
            format!("Sélectionner un point sur le bord. {seg}")
        }
    }
    fn vcb_label(&self) -> &'static str {
        match (self.center.is_none(), self.polygon, self.circumscribed) {
            (true, ..) => "Côtés",
            (false, true, false) => "Rayon inscrit",
            (false, true, true) => "Rayon circonscrit",
            (false, false, _) => "Rayon",
        }
    }
    fn ctrl_tap(&mut self, cx: &mut ToolCx) {
        if self.polygon {
            self.circumscribed = !self.circumscribed;
            *cx.message = Some(if self.circumscribed { "Polygone circonscrit" } else { "Polygone inscrit" }.into());
        }
    }
    fn step(&mut self, cx: &mut ToolCx, delta: i32) {
        self.segments = (self.segments as i32 + delta).clamp(3, 999) as usize;
        *cx.message = Some(format!("{} côtés", self.segments));
    }
    fn vcb_value(&self, units: Units) -> String {
        if self.center.is_none() { self.segments.to_string() } else { format_length(self.radius, units) }
    }
    fn busy(&self) -> bool {
        self.center.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let plane = self.center.map(|c| c.1);
        let s = cx.snap(m, self.center.map(|c| c.0), plane);
        if let Some((c, pl)) = self.center {
            let q = s.pos - pl.n * pl.distance(s.pos);
            self.radius = q.distance(c);
        }
        self.snap = Some(s);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let started = self.center.is_none();
        if started {
            if let Some(s) = self.snap.clone() {
                self.center = Some((s.pos, cx.plane_at(&s)));
            }
        } else {
            self.finish(cx);
        }
        self.cd.press(m, started);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.cd.release(m) {
            self.finish(cx);
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let t = text.trim().to_lowercase();
        if let Some(n) = t.strip_suffix('s') {
            let n: usize = n.trim().parse().map_err(|_| "Nombre de côtés invalide".to_string())?;
            if !(3..=999).contains(&n) {
                return Err("Entre 3 et 999 côtés".into());
            }
            self.segments = n;
            return Ok(());
        }
        if self.center.is_none() {
            return Err("Cliquez d'abord le centre".into());
        }
        self.radius = cx.parse_len(&t)?.abs();
        self.finish(cx);
        Ok(())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.center = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        if let Some(pts) = self.points() {
            o.fill_polygon(&pts, Color32::from_rgba_unmultiplied(120, 140, 255, 50));
            o.polyline(&pts, true, Color32::BLACK, 2.0);
            if let Some((c, _)) = self.center {
                o.dashed(c, pts[0], Color32::from_gray(90), 1.0);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------

/// A rounded corner being set up by the two-point arc: two straight edges that meet at `v`.
struct Corner {
    v: DVec3,
    /// Unit rays leaving the corner along each edge, and the edge lengths.
    u1: DVec3,
    u2: DVec3,
    len1: f64,
    len2: f64,
}

/// What the cursor is on at a click: an edge of the current context.
#[derive(Clone, Copy)]
struct EdgeHit {
    id: EdgeId,
    a: DVec3,
    b: DVec3,
}

/// Two-point arc (CArcTool): start, end, then the bulge. Alt locks the arc tangent to the edge it
/// starts on. When the start and the end are on two straight edges that meet in a corner, at the
/// same distance from it, the last step selects the radius of a rounded corner (fillet) instead
/// of the bulge.
#[derive(Default)]
pub struct ArcTool {
    p1: Option<(DVec3, Plane)>,
    edge1: Option<EdgeHit>,
    /// Unit direction leaving the start along the edge it is the end of.
    tangent: Option<DVec3>,
    tangent_locked: bool,
    on_tangent: bool,
    p2: Option<DVec3>,
    bulge: f64,
    corner: Option<Corner>,
    radius: f64,
    sides: Sides,
    snap: Option<Snap>,
    cd: ClickDrag,
}

/// Everything needed to draw and measure the arc being made.
struct ArcShape {
    pts: Vec<DVec3>,
    radius: f64,
    sweep: f64,
}

impl ArcTool {
    fn edge_under(cx: &ToolCx, s: &Snap) -> Option<EdgeHit> {
        if !matches!(s.kind, SnapKind::Endpoint | SnapKind::OnEdge | SnapKind::Midpoint) {
            return None;
        }
        let (h, (a, b)) = s.edge.as_ref()?;
        match h.selectable(&cx.doc.ctx)? {
            EntityRef::Edge(id) => Some(EdgeHit { id, a: *a, b: *b }),
            _ => None,
        }
    }

    /// The tangent direction, projected in the arc's plane.
    fn tangent_in_plane(&self) -> Option<DVec3> {
        let (_, pl) = self.p1?;
        let t = self.tangent?;
        let t = t - pl.n * t.dot(pl.n);
        (t.length() > 0.3).then(|| t.normalize())
    }

    fn bulge_perp(&self) -> Option<DVec3> {
        let (a, pl) = self.p1?;
        let b = self.p2?;
        Some(pl.n.cross((b - a).normalize_or_zero()))
    }

    fn bulge_from(&self, q: DVec3) -> f64 {
        let (Some((a, _)), Some(b), Some(perp)) = (self.p1, self.p2, self.bulge_perp()) else { return 0.0 };
        (q - (a + b) * 0.5).dot(perp)
    }

    /// The bulge of the arc tangent to the starting edge, if there is one.
    fn tangent_bulge(&self) -> Option<f64> {
        let (a, _) = self.p1?;
        tangent_bulge(a, self.p2?, self.tangent_in_plane()?, self.bulge_perp()?)
    }

    fn fillet(&self) -> Option<Fillet> {
        let c = self.corner.as_ref()?;
        fillet(c.v, c.u1, c.u2, self.radius)
    }

    fn shape(&self) -> Option<ArcShape> {
        if let Some(f) = self.fillet() {
            let segs = self.sides.segments(f.sweep);
            let pts = arc_points(f.center, f.normal, f.start, f.radius, f.sweep, segs);
            return Some(ArcShape { pts, radius: f.radius, sweep: f.sweep });
        }
        if self.corner.is_some() {
            return None;
        }
        let (a, pl) = self.p1?;
        let b = self.p2?;
        let s = self.bulge;
        let chord = b - a;
        let half = chord.length() * 0.5;
        if half < TOL {
            return None;
        }
        if s.abs() < TOL {
            return Some(ArcShape { pts: vec![a, b], radius: f64::INFINITY, sweep: 0.0 });
        }
        let perp = pl.n.cross(chord / (2.0 * half));
        let mid = (a + b) * 0.5;
        let r = (half * half + s * s) / (2.0 * s.abs());
        let c = mid + perp * (s - s.signum() * r);
        let bulge = mid + perp * s;
        // Sweep from a to b passing through the bulge point.
        let e1 = (a - c) / r;
        let e2 = pl.n.cross(e1);
        let angle = |p: DVec3| {
            let d = p - c;
            d.dot(e2).atan2(d.dot(e1)).rem_euclid(std::f64::consts::TAU)
        };
        let (tb, tm) = (angle(b), angle(bulge));
        let sweep = if tm < tb { tb } else { tb - std::f64::consts::TAU };
        let segs = self.sides.segments(sweep);
        Some(ArcShape { pts: arc_points(c, pl.n, e1, r, sweep, segs), radius: r, sweep })
    }

    fn finish(&mut self, cx: &mut ToolCx) {
        let Some(shape) = self.shape() else { return };
        if shape.radius.is_finite()
            && let Err(msg) = check_arc(shape.radius, shape.sweep, shape.pts.len() - 1)
        {
            *cx.message = Some(msg);
            return;
        }
        let local: Vec<DVec3> = shape.pts.iter().map(|p| cx.to_local(*p)).collect();
        if let Some(c) = &self.corner {
            // Rounded corner: the arc replaces the corner, whose sliver and edges go.
            let corner = cx.to_local(c.v);
            let normal = c.u1.cross(c.u2);
            let normal_local = cx.doc.ctx_xform().inverse().transpose().transform_vector3(normal).normalize_or_zero();
            cx.doc.commit("Coin arrondi", |m, d| {
                let e = m.entities_mut(d);
                e.add_polyline(&local, false, true);
                let Some(vv) = e.find_vertex(corner) else { return };
                let slivers: Vec<_> = e
                    .faces
                    .iter()
                    .filter(|(_, f)| {
                        f.all_vertices().any(|x| x == vv)
                            && f.normal.cross(normal_local).length() < 1e-6
                            && (e.pos(f.outer[0]) - corner).dot(normal_local).abs() < TOL
                    })
                    .map(|(k, _)| k)
                    .collect();
                for f in slivers {
                    e.remove_face(f);
                }
                let dead: Vec<EdgeId> = [local[0], *local.last().unwrap()]
                    .iter()
                    .filter_map(|t| e.find_vertex(*t).and_then(|tv| e.find_edge(vv, tv)))
                    .filter(|ed| e.edges[*ed].faces.is_empty())
                    .collect();
                e.erase_edges(&dead);
            });
        } else {
            cx.doc.commit("Arc", |m, d| {
                m.entities_mut(d).add_polyline(&local, false, true);
            });
        }
        self.reset();
    }

    fn reset(&mut self) {
        self.p1 = None;
        self.edge1 = None;
        self.tangent = None;
        self.on_tangent = false;
        self.p2 = None;
        self.bulge = 0.0;
        self.corner = None;
        self.radius = 0.0;
    }

    /// After the second click: are both points on two straight edges that meet in a corner, at
    /// the same distance from it (within 6 pixels)? Then the arc is a rounded corner.
    fn corner_from(&self, cx: &ToolCx, e2: EdgeHit, b: DVec3) -> Option<Corner> {
        let (a, _) = self.p1?;
        let e1 = self.edge1?;
        if e1.id == e2.id {
            return None;
        }
        let ents = cx.doc.ents();
        if ents.edges.get(e1.id)?.curve.is_some() || ents.edges.get(e2.id)?.curve.is_some() {
            return None;
        }
        // The shared end of the two edges is the corner.
        let ends1 = [e1.a, e1.b];
        let ends2 = [e2.a, e2.b];
        let (v, far1, far2) = ends1.iter().enumerate().find_map(|(i, p)| {
            ends2.iter().enumerate().find(|(_, q)| p.distance(**q) < TOL).map(|(j, _)| (*p, ends1[1 - i], ends2[1 - j]))
        })?;
        let (l1, l2) = (far1 - v, far2 - v);
        let (len1, len2) = (l1.length(), l2.length());
        if len1 < TOL || len2 < TOL {
            return None;
        }
        let (u1, u2) = (l1 / len1, l2 / len2);
        if u1.cross(u2).length() < 1e-3 {
            return None;
        }
        let (d1, d2) = ((a - v).dot(u1), (b - v).dot(u2));
        let tol = SNAP_TOLERANCE_PX * cx.cam().pixel_size_at(v);
        if d1 < TOL || d2 < TOL || (d1 - d2).abs() > tol {
            return None;
        }
        Some(Corner { v, u1, u2, len1, len2 })
    }
}

impl Tool for ArcTool {
    fn id(&self) -> ToolId {
        ToolId::Arc
    }
    fn status(&self) -> String {
        // CArcTool (the two-point arc): "%s | %s", modifiers from.
        use super::hints::{ARROWS_INFERENCE, LOCK_TANGENCY, SEGMENTS};
        let mods = if self.tangent.is_some() {
            format!("{SEGMENTS} | {LOCK_TANGENCY} | {ARROWS_INFERENCE}")
        } else {
            format!("{SEGMENTS} | {ARROWS_INFERENCE}")
        };
        match (self.p1.is_some(), self.p2.is_some()) {
            (false, _) => format!("Cliquez pour définir le point de départ. | {mods}"),
            (true, false) => format!("Cliquez pour définir le point d'arrivée ou entrez une longueur. | {mods}"),
            _ if self.corner.is_some() => format!("Sélectionnez le rayon d'arrondi ou entrez une valeur. {SEGMENTS}"),
            _ => format!("Cliquez pour définir la courbure ou entrez la distance. | {mods}"),
        }
    }
    fn vcb_label(&self) -> &'static str {
        match (self.p2.is_some(), self.corner.is_some()) {
            (true, true) => "Rayon",
            (true, false) => "Flèche",
            _ => "Longueur",
        }
    }
    fn alt_tap(&mut self, cx: &mut ToolCx) {
        self.tangent_locked = !self.tangent_locked;
        *cx.message =
            Some(if self.tangent_locked { "Verrouillé sur la tangente" } else { "Tangente déverrouillée" }.into());
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
    fn vcb_value(&self, units: Units) -> String {
        match (self.p1, self.p2, &self.snap) {
            (Some(_), Some(_), _) if self.corner.is_some() => format_length(self.radius, units),
            (Some(_), Some(_), _) => format_length(self.bulge.abs(), units),
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
        let plane = self.p1.map(|p| p.1);
        let reference = self.p2.or(self.p1.map(|p| p.0));
        let s = cx.snap(m, reference, plane);
        if let (Some(c), Some((_, pl))) = (&self.corner, self.p1) {
            // The cursor drags the middle of the rounded corner.
            let q = s.pos - pl.n * pl.distance(s.pos);
            let bis = (c.u1 + c.u2).normalize();
            let apex = (q - c.v).dot(bis).max(0.0);
            let r_max = fillet_max_radius(c.u1, c.u2, c.len1, c.len2);
            self.radius = fillet_radius_for_apex(c.u1, c.u2, apex).clamp(TOL, r_max);
        } else if let Some(p2) = self.p2 {
            let pl = self.p1.unwrap().1;
            let q = s.pos - pl.n * pl.distance(s.pos);
            self.bulge = self.bulge_from(q);
            self.on_tangent = false;
            if let Some(st) = self.tangent_bulge() {
                let (a, _) = self.p1.unwrap();
                let mid = (a + p2) * 0.5;
                let near = (self.bulge - st).abs() < SNAP_TOLERANCE_PX * cx.cam().pixel_size_at(mid);
                if self.tangent_locked || near {
                    self.bulge = st;
                    self.on_tangent = true;
                }
            }
        }
        self.snap = Some(s);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(s) = self.snap.clone() else { return };
        let started = self.p1.is_none();
        if self.p1.is_none() {
            self.p1 = Some((s.pos, cx.plane_at(&s)));
            self.edge1 = Self::edge_under(cx, &s);
            self.tangent = match (&self.edge1, s.kind) {
                (Some(e), SnapKind::Endpoint) => {
                    let other = if e.a.distance(s.pos) > e.b.distance(s.pos) { e.a } else { e.b };
                    (s.pos - other).try_normalize()
                }
                _ => None,
            };
        } else if self.p2.is_none() {
            let (a, pl) = self.p1.unwrap();
            let q = s.pos - pl.n * pl.distance(s.pos);
            if q.distance(a) > TOL {
                self.set_end(cx, &s, q);
            }
        } else {
            self.finish(cx);
        }
        self.cd.press(m, started);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.cd.release(m)
            && self.p2.is_none()
            && let (Some(s), Some((a, pl))) = (self.snap.clone(), self.p1)
        {
            let q = s.pos - pl.n * pl.distance(s.pos);
            if q.distance(a) > TOL {
                self.set_end(cx, &s, q);
            }
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        if let Some(sides) = parse_sides(text) {
            self.sides = sides?;
            *cx.message = Some(self.sides.status());
            return Ok(());
        }
        let v = cx.parse_len(text.trim())?;
        match (self.p1, self.p2) {
            (Some((a, _)), None) => {
                let dir = self.snap.as_ref().map(|s| (s.pos - a).normalize_or_zero()).unwrap_or_default();
                if dir == DVec3::ZERO {
                    return Err("Indiquez une direction avec la souris".into());
                }
                let b = a + dir * v;
                self.set_end_typed(cx, b);
                Ok(())
            }
            (Some(_), Some(_)) if self.corner.is_some() => {
                let c = self.corner.as_ref().unwrap();
                self.radius = v.abs().clamp(TOL, fillet_max_radius(c.u1, c.u2, c.len1, c.len2));
                self.finish(cx);
                Ok(())
            }
            (Some(_), Some(_)) => {
                self.bulge = v * if self.bulge < 0.0 { -1.0 } else { 1.0 };
                self.finish(cx);
                Ok(())
            }
            _ => Err("Cliquez d'abord le début de l'arc".into()),
        }
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.reset();
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        match (self.p1, self.p2, &self.snap) {
            (Some(_), Some(_), _) => {
                if let Some(shape) = self.shape() {
                    let col = if self.on_tangent { c32(crate::settings::tangent_color()) } else { Color32::BLACK };
                    o.polyline(&shape.pts, false, col, 2.0);
                }
                if let Some(f) = self.fillet() {
                    o.dashed(f.t1, f.center, Color32::from_gray(150), 1.0);
                    o.dashed(f.t2, f.center, Color32::from_gray(150), 1.0);
                }
            }
            (Some((a, _)), None, Some(s)) => o.dashed(a, s.pos, Color32::from_gray(60), 1.5),
            _ => {}
        }
    }
}

impl ArcTool {
    /// Second click on the screen: the end of the arc, or the second edge of a rounded corner.
    fn set_end(&mut self, cx: &mut ToolCx, s: &Snap, q: DVec3) {
        let hit = Self::edge_under(cx, s);
        if let Some(e2) = hit
            && let Some(c) = self.corner_from(cx, e2, q)
        {
            let (a, _) = self.p1.unwrap();
            let d = (a - c.v).dot(c.u1);
            self.radius = fillet_radius_for_tangent_length(c.u1, c.u2, d)
                .clamp(TOL, fillet_max_radius(c.u1, c.u2, c.len1, c.len2));
            self.p2 = Some(c.v + c.u2 * d);
            self.corner = Some(c);
            return;
        }
        self.p2 = Some(q);
    }

    fn set_end_typed(&mut self, _cx: &mut ToolCx, b: DVec3) {
        self.p2 = Some(b);
    }
}
