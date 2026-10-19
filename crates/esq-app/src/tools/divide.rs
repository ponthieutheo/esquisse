//! Divide (right-click on an edge › Divide), after SketchUp's `CDivideLineTool`: the
//! selected edge — or the arc or circle it belongs to — shows division points; moving the
//! cursor toward an end adds segments (`1/t` rounded, at most 100), a click or a number typed
//! in the Measurements box cuts it, then the tool gives way.

use eframe::egui::{self, Color32, Stroke};
use esq_core::curves::{ArcFit, arc_fit, divide, divide_count};
use esq_core::glam::{DVec2, DVec3};
use esq_core::math::{Plane, line_closest_to_ray};
use esq_core::model::{EdgeId, EntityRef};
use esq_core::units::format_length;

use super::{Overlay, Tool, ToolCx, ToolId};

/// What is divided, in world coordinates.
enum Target {
    Line(DVec3, DVec3),
    Arc(ArcFit),
}

#[derive(Default)]
pub struct DivideTool {
    started: bool,
    edge: Option<EdgeId>,
    target: Option<Target>,
    /// Segments shown (1 = nothing to cut yet).
    n: usize,
    /// Where the label goes (screen point of the cursor).
    cursor: Option<DVec2>,
    units: esq_core::model::Units,
    done: bool,
}

impl DivideTool {
    /// On activation: the first selected edge, unless it belongs to a curve that is not an
    /// arc or a circle (then the tool does nothing, as in SketchUp).
    fn start(&mut self, cx: &ToolCx) {
        if self.started {
            return;
        }
        self.started = true;
        self.n = 1;
        self.units = cx.units;
        let ents = cx.doc.ents();
        let Some(e) = cx.doc.sel.iter().find_map(|x| if let EntityRef::Edge(e) = x { Some(*e) } else { None }) else {
            return;
        };
        let x = cx.doc.ctx_xform();
        self.target = match ents.edges[e].curve {
            None => {
                let (a, b) = ents.edge_points(e);
                Some(Target::Line(x.transform_point3(a), x.transform_point3(b)))
            }
            Some(c) => arc_fit(ents, c).map(|f| {
                let center = x.transform_point3(f.center);
                let normal = x.transform_vector3(f.normal).normalize();
                let r = x.transform_point3(f.center + f.xaxis * f.radius);
                let xaxis = (r - center).normalize();
                Target::Arc(ArcFit { center, normal, xaxis, radius: r.distance(center), ..f })
            }),
        };
        if self.target.is_some() {
            self.edge = Some(e);
        }
    }

    /// The division points (ends included).
    fn points(&self) -> Vec<DVec3> {
        let n = self.n.max(1);
        match &self.target {
            Some(Target::Line(a, b)) => (0..=n).map(|i| *a + (*b - *a) * (i as f64 / n as f64)).collect(),
            Some(Target::Arc(f)) => (0..=n).map(|i| f.point(f.sweep * i as f64 / n as f64)).collect(),
            None => Vec::new(),
        }
    }

    /// Length of one segment.
    fn seg_len(&self) -> f64 {
        let n = self.n.max(1) as f64;
        match &self.target {
            Some(Target::Line(a, b)) => a.distance(*b) / n,
            Some(Target::Arc(f)) => f.sweep * f.radius / n,
            None => 0.0,
        }
    }

    fn cut(&mut self, cx: &mut ToolCx) {
        if let (Some(e), true) = (self.edge, self.n > 1) {
            let n = self.n;
            let mut ok = false;
            cx.doc.commit("Diviser des arêtes", |m, d| ok = divide(m.entities_mut(d), e, n));
            if !ok {
                cx.doc.undo();
            }
        }
        self.done = true;
    }
}

impl Tool for DivideTool {
    fn id(&self) -> ToolId {
        ToolId::Divide
    }
    fn status(&self) -> String {
        "Sélectionnez ou entrez le nombre de segments.".into()
    }
    fn vcb_label(&self) -> &'static str {
        "Segments"
    }
    fn vcb_value(&self, _units: esq_core::model::Units) -> String {
        if self.target.is_some() { self.n.max(1).to_string() } else { String::new() }
    }
    fn busy(&self) -> bool {
        true
    }
    fn done(&self) -> bool {
        self.done
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.start(cx);
        self.cursor = Some(m);
        let (o, d) = cx.cam().ray(m);
        self.n = match &self.target {
            Some(Target::Line(a, b)) => line_closest_to_ray(*a, *b - *a, o, d).map_or(1, divide_count),
            Some(Target::Arc(f)) => Plane::from_point_normal(f.center, f.normal)
                .intersect_ray(o, d)
                .map_or(1, |t| divide_count(f.angle_of(o + d * t) / f.sweep)),
            None => 1,
        };
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        self.cut(cx);
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        self.start(cx);
        let n: usize = text.trim().parse().map_err(|_| format!("Nombre de segments invalide : « {text} »"))?;
        if n == 0 {
            return Ok(());
        }
        self.n = n;
        self.cut(cx);
        Ok(())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        cx.doc.set_preview(None);
        self.done = true;
    }
    fn overlay(&self, o: &mut Overlay) {
        if self.n < 2 {
            return;
        }
        for p in self.points() {
            if let Some(s) = o.screen(p) {
                let r = egui::Rect::from_center_size(s, egui::vec2(6.0, 6.0));
                o.painter.rect_filled(r, 0.0, Color32::from_rgb(220, 30, 30));
                o.painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Outside);
            }
        }
        if let Some(c) = self.cursor {
            let text = format!("{} segments\nLongueur: {}", self.n, format_length(self.seg_len(), self.units));
            o.tooltip(o.origin + egui::vec2(c.x as f32 + 14.0, c.y as f32 + 14.0), &text);
        }
    }
}
