//! Scale tool. The selection gets a bounding box with 26 grips (8 corners, 12 edge middles,
//! 6 face centres; 8 on a flat selection), as in SketchUp:
//! - a corner scales the three axes together (uniformly), an edge grip the two axes it
//!   touches, a face grip its one axis;
//! - Shift swaps uniform and non-uniform (a corner then scales its three axes freely, an
//!   edge or face grip scales all three together);
//! - Ctrl swaps scaling about the opposite grip and about the centre;
//! - typing a factor (`1,5`), a factor per axis (`2;1;0,5`) or a length (`500mm`) scales
//!   with the same grip.

use eframe::egui::{self, Color32};
use esq_core::glam::{DMat3, DMat4, DVec2, DVec3};
use esq_core::math::{Plane, TOL, line_closest_to_ray};
use esq_core::model::{EntityRef, Model, Units};

use super::modify::targets;
use super::{ClickDrag, Overlay, Tool, ToolCx, ToolId, split_values};
use crate::pick::Snap;

/// Pixels within which the cursor picks a grip.
const GRIP_PX: f64 = 12.0;
/// A factor never gets closer to zero than this (the object would collapse).
const MIN_FACTOR: f64 = 1e-3;

/// One of the 26 grips: for each axis, 0 = minimum face, 1 = middle, 2 = maximum face.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Grip([u8; 3]);

impl Grip {
    fn active(self) -> [bool; 3] {
        self.0.map(|i| i != 1)
    }

    fn count(self) -> usize {
        self.active().iter().filter(|a| **a).count()
    }

    /// Corners scale uniformly unless Shift is held; edges and faces the opposite.
    fn uniform_by_default(self) -> bool {
        self.count() == 3
    }
}

/// The selection's bounding box, in the frame of the drawing axes.
#[derive(Clone, Copy, Debug)]
struct Frame {
    origin: DVec3,
    axis: [DVec3; 3],
    min: DVec3,
    max: DVec3,
}

impl Frame {
    fn local(&self, p: DVec3) -> DVec3 {
        let d = p - self.origin;
        DVec3::new(d.dot(self.axis[0]), d.dot(self.axis[1]), d.dot(self.axis[2]))
    }

    fn world(&self, c: DVec3) -> DVec3 {
        self.origin + self.axis[0] * c.x + self.axis[1] * c.y + self.axis[2] * c.z
    }

    fn mid(&self) -> DVec3 {
        (self.min + self.max) * 0.5
    }

    /// An axis along which the box has no thickness (a flat selection).
    fn flat(&self, i: usize) -> bool {
        (self.max[i] - self.min[i]).abs() < TOL
    }

    /// Local position of a grip.
    fn grip_local(&self, g: Grip) -> DVec3 {
        let pick = |i: usize| match g.0[i] {
            0 => self.min[i],
            1 => (self.min[i] + self.max[i]) * 0.5,
            _ => self.max[i],
        };
        DVec3::new(pick(0), pick(1), pick(2))
    }

    fn grips(&self) -> Vec<Grip> {
        let mut out = Vec::new();
        for x in 0..3u8 {
            for y in 0..3u8 {
                for z in 0..3u8 {
                    let g = Grip([x, y, z]);
                    if g.count() == 0 {
                        continue;
                    }
                    // A flat axis only has one position.
                    if (0..3).any(|i| self.flat(i) && g.0[i] != 0) {
                        continue;
                    }
                    out.push(g);
                }
            }
        }
        out
    }

    /// The fixed point for `g`: the opposite grip, or the centre.
    fn anchor(&self, g: Grip, about_center: bool) -> DVec3 {
        if about_center {
            return self.mid();
        }
        let opposite = Grip(g.0.map(|i| match i {
            0 => 2,
            2 => 0,
            _ => 1,
        }));
        self.grip_local(opposite)
    }
}

#[derive(Default)]
pub struct ScaleTool {
    items: Vec<EntityRef>,
    frame: Option<Frame>,
    hover: Option<Grip>,
    grip: Option<Grip>,
    /// Ctrl: scale about the centre.
    center: bool,
    /// Shift tapped: uniform scaling toggled (a held Shift toggles it too).
    uniform_toggle: bool,
    shift_held: bool,
    shift_used: bool,
    factors: DVec3,
    snap: Option<Snap>,
    cd: ClickDrag,
}

impl ScaleTool {
    /// Bounding box of `items` in the frame of the drawing axes.
    fn frame_of(cx: &ToolCx, items: &[EntityRef]) -> Option<Frame> {
        let ents = cx.doc.ents();
        let x = cx.doc.ctx_xform();
        let a = cx.infer.axes;
        let axis = [0, 1, 2].map(|i| a.col(i).truncate().normalize_or(DVec3::ZERO));
        let mut f = Frame { origin: a.col(3).truncate(), axis, min: DVec3::INFINITY, max: DVec3::NEG_INFINITY };
        let grow = |p: DVec3, f: &mut Frame| {
            let c = f.local(p);
            f.min = f.min.min(c);
            f.max = f.max.max(c);
        };
        for v in ents.vertices_of(&ents.with_bounding_edges(items)) {
            grow(x.transform_point3(ents.pos(v)), &mut f);
        }
        for it in items {
            if let EntityRef::Instance(i) = *it {
                let inst = &ents.instances[i];
                grow(x.transform_point3(inst.xform.w_axis.truncate()), &mut f);
                let ix = x * inst.xform;
                for (_, v) in cx.doc.model.defs[inst.def].entities.verts.iter() {
                    grow(ix.transform_point3(v.pos), &mut f);
                }
            }
        }
        (f.min.is_finite() && f.max.is_finite()).then_some(f)
    }

    fn effective_uniform(&self, g: Grip, shift: bool) -> bool {
        g.uniform_by_default() != (shift != self.uniform_toggle)
    }

    /// Scale factors along the three axes when the cursor ray is `(ro, rd)`.
    fn factors_for(&self, cx: &ToolCx, m: DVec2, g: Grip, uniform: bool) -> Option<DVec3> {
        let fr = self.frame?;
        let a = fr.anchor(g, self.center);
        let gl = fr.grip_local(g);
        let (ro, rd) = cx.cam().ray(m);
        let clamp = |f: f64| if f.abs() < MIN_FACTOR { MIN_FACTOR * if f < 0.0 { -1.0 } else { 1.0 } } else { f };
        if uniform || g.count() == 1 {
            // Along the line from the fixed point through the grip.
            let (aw, gw) = (fr.world(a), fr.world(gl));
            let t = clamp(line_closest_to_ray(aw, gw - aw, ro, rd)?);
            return Some(if uniform {
                DVec3::splat(t)
            } else {
                let act = g.active();
                DVec3::new(if act[0] { t } else { 1.0 }, if act[1] { t } else { 1.0 }, if act[2] { t } else { 1.0 })
            });
        }
        // Free scaling: the cursor ray meets a plane through the grip, the one the two
        // scaled axes span, or (all three axes) the plane facing the camera.
        let normal = if g.count() == 2 {
            let k = g.0.iter().position(|i| *i == 1).expect("an edge grip has a middle axis");
            fr.axis[k]
        } else {
            cx.cam().forward()
        };
        let t = Plane::from_point_normal(fr.world(gl), normal).intersect_ray(ro, rd)?;
        let p = fr.local(ro + rd * t);
        let mut f = DVec3::ONE;
        for i in 0..3 {
            let span = gl[i] - a[i];
            if g.active()[i] && span.abs() > TOL {
                f[i] = clamp((p[i] - a[i]) / span);
            }
        }
        Some(f)
    }

    /// The model with the selection scaled by `f` about the fixed point of the grip.
    fn model_with(&self, cx: &ToolCx, g: Grip, f: DVec3) -> Option<Model> {
        let fr = self.frame?;
        let anchor = fr.world(fr.anchor(g, self.center));
        let basis = DMat4::from_mat3(DMat3::from_cols(fr.axis[0], fr.axis[1], fr.axis[2]));
        let xf = DMat4::from_translation(anchor)
            * basis
            * DMat4::from_scale(f)
            * basis.transpose()
            * DMat4::from_translation(-anchor);
        let mut m = cx.doc.model.clone();
        let ents = m.entities_mut(cx.doc.ctx_def());
        let all = ents.with_bounding_edges(&self.items);
        ents.transform_selection(&all, &cx.local_xform(xf));
        Some(m)
    }

    fn finish(&mut self, cx: &mut ToolCx, f: DVec3) {
        let Some(g) = self.grip else { return };
        if let Some(m) = self.model_with(cx, g, f) {
            cx.doc.commit("Échelle", |model, _| *model = m);
        }
        self.grip = None;
        self.items.clear();
        self.frame = None;
    }

    /// The grip nearest to the cursor, within [`GRIP_PX`].
    fn grip_at(&self, cx: &ToolCx, m: DVec2) -> Option<Grip> {
        let fr = self.frame?;
        fr.grips()
            .into_iter()
            .filter_map(|g| cx.cam().project(fr.world(fr.grip_local(g))).map(|px| (g, px.distance(m))))
            .filter(|(_, d)| *d <= GRIP_PX)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(g, _)| g)
    }

    fn refresh_frame(&mut self, cx: &ToolCx) {
        if self.grip.is_some() {
            return;
        }
        self.frame = if cx.doc.sel.is_empty() { None } else { Self::frame_of(cx, &cx.doc.sel) };
    }
}

impl Tool for ScaleTool {
    fn id(&self) -> ToolId {
        ToolId::Scale
    }
    fn status(&self) -> String {
        // CScaleTool: twelve messages, by step (object, grip, scaling) and mode (uniform,
        // about the centre), then the two toggles.
        const MESSAGES: [[&str; 3]; 4] = [
            [
                "Cliquez sur la face ou l'objet que vous souhaitez redimensionner.",
                "Cliquez sur une poignée de mise à l'échelle pour commencer à redimensionner.",
                "Cliquez pour finaliser la mise à l'échelle ou entrez un facteur d'échelle ou une cotation.",
            ],
            [
                "Cliquez sur la face ou l'objet que vous souhaitez redimensionner uniformément.",
                "Cliquez sur une poignée de mise à l'échelle pour commencer à redimensionner uniformément.",
                "Cliquez pour terminer la mise à l'échelle de manière uniforme ou entrez un facteur d'échelle ou une cotation.",
            ],
            [
                "Cliquez sur la face ou l'objet que vous souhaitez redimensionner autour du centre.",
                "Cliquez sur une poignée de mise à l'échelle pour commencer à redimensionner autour du centre.",
                "Cliquez pour terminer la mise à l'échelle autour du centre ou entrez un facteur d'échelle ou une cotation.",
            ],
            [
                "Cliquez sur la face ou l'objet que vous souhaitez redimensionner uniformément autour du centre.",
                "Cliquez sur une poignée de mise à l'échelle pour commencer à redimensionner uniformément autour du centre.",
                "Cliquez pour terminer la mise à l'échelle de manière uniforme autour du centre ou entrez un facteur d'échelle ou une cotation.",
            ],
        ];
        let step = if self.grip.is_some() {
            2
        } else if self.frame.is_some() {
            1
        } else {
            0
        };
        let uniform = match self.grip.or(self.hover) {
            Some(g) => self.effective_uniform(g, self.shift_held),
            None => self.shift_held != self.uniform_toggle,
        };
        let mode = usize::from(uniform) + if self.center { 2 } else { 0 };
        format!(
            "{} | Ctrl = Activer/Désactiver redimensionner autour du centre. \
             | Maj = Activer/désactiver mise à l’échelle uniforme.",
            MESSAGES[mode][step]
        )
    }
    fn shift_tap(&mut self, _cx: &mut ToolCx) {
        // Shift held while the cursor moved was a held key, not a tap.
        if !std::mem::take(&mut self.shift_used) {
            self.uniform_toggle = !self.uniform_toggle;
        }
    }
    fn vcb_label(&self) -> &'static str {
        "Échelle"
    }
    fn vcb_value(&self, _units: Units) -> String {
        let f = self.factors;
        if self.grip.is_none() {
            String::new()
        } else if (f.x - f.y).abs() < 1e-9 && (f.y - f.z).abs() < 1e-9 {
            format!("{:.3}", f.x).replace('.', ",")
        } else {
            format!("{:.3}; {:.3}; {:.3}", f.x, f.y, f.z).replace('.', ",")
        }
    }
    fn busy(&self) -> bool {
        self.grip.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn ctrl_tap(&mut self, _cx: &mut ToolCx) {
        self.center = !self.center;
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.shift_held = cx.mods.shift;
        self.shift_used |= cx.mods.shift;
        self.snap = Some(cx.snap(m, None, None));
        self.refresh_frame(cx);
        match self.grip {
            Some(g) => {
                let uniform = self.effective_uniform(g, cx.mods.shift);
                if let Some(f) = self.factors_for(cx, m, g, uniform) {
                    self.factors = f;
                    if let Some(pm) = self.model_with(cx, g, f) {
                        cx.doc.set_preview(Some(pm));
                    }
                }
            }
            None => self.hover = self.grip_at(cx, m),
        }
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        if let Some(g) = self.grip {
            let _ = g;
            let f = self.factors;
            self.finish(cx, f);
            self.cd.press(m, false);
            return;
        }
        if cx.doc.sel.is_empty() {
            let items = targets(cx, self.snap.as_ref());
            if items.is_empty() {
                *cx.message = Some("Cliquez sur l'objet à mettre à l'échelle".into());
            } else {
                cx.doc.select(items);
                self.refresh_frame(cx);
            }
            return;
        }
        let Some(g) = self.hover else {
            *cx.message = Some("Cliquez sur une poignée de la boîte englobante".into());
            return;
        };
        if cx.mods.ctrl || cx.mods.command {
            self.center = true;
        }
        self.items = cx.doc.sel.clone();
        self.grip = Some(g);
        self.factors = DVec3::ONE;
        self.cd.press(m, true);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.cd.release(m) {
            let f = self.factors;
            self.finish(cx, f);
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let (Some(g), Some(fr)) = (self.grip, self.frame) else { return Err("Cliquez d'abord une poignée".into()) };
        let uniform = self.effective_uniform(g, cx.mods.shift);
        let act = g.active();
        let parts = split_values(text);
        let factor = |s: &str| -> Result<f64, String> {
            let f = s.trim().replace(',', ".").parse::<f64>().map_err(|_| "Facteur invalide".to_string())?;
            if f.abs() < 1e-6 { Err("Facteur nul impossible".into()) } else { Ok(f) }
        };
        let is_length = |s: &str| s.chars().any(|c| c.is_alphabetic() || c == '\'' || c == '"');
        let f = match parts.as_slice() {
            [one] if is_length(one) => {
                let (a, gl) = (fr.anchor(g, self.center), fr.grip_local(g));
                let span = if uniform || g.count() > 1 {
                    fr.world(gl).distance(fr.world(a))
                } else {
                    (0..3).find(|i| act[*i]).map(|i| (gl[i] - a[i]).abs()).unwrap_or(0.0)
                };
                if span < TOL {
                    return Err("Dimension nulle : impossible de la fixer".into());
                }
                let f = cx.parse_len(one)? / span;
                if uniform {
                    DVec3::splat(f)
                } else {
                    DVec3::new(if act[0] { f } else { 1.0 }, if act[1] { f } else { 1.0 }, if act[2] { f } else { 1.0 })
                }
            }
            [one] => {
                let f = factor(one)?;
                if uniform {
                    DVec3::splat(f)
                } else {
                    DVec3::new(if act[0] { f } else { 1.0 }, if act[1] { f } else { 1.0 }, if act[2] { f } else { 1.0 })
                }
            }
            [x, y, z] => DVec3::new(factor(x)?, factor(y)?, factor(z)?),
            _ => return Err("Tapez un facteur, ou trois facteurs séparés par des points-virgules".into()),
        };
        self.finish(cx, f);
        Ok(())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.grip = None;
        self.items.clear();
        self.frame = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        let Some(fr) = self.frame else { return };
        let (lo, hi) = (fr.min, fr.max);
        let corner = |x: bool, y: bool, z: bool| {
            fr.world(DVec3::new(if x { hi.x } else { lo.x }, if y { hi.y } else { lo.y }, if z { hi.z } else { lo.z }))
        };
        let ink = Color32::from_gray(90);
        for (a, b) in [
            ((false, false, false), (true, false, false)),
            ((false, true, false), (true, true, false)),
            ((false, false, true), (true, false, true)),
            ((false, true, true), (true, true, true)),
            ((false, false, false), (false, true, false)),
            ((true, false, false), (true, true, false)),
            ((false, false, true), (false, true, true)),
            ((true, false, true), (true, true, true)),
            ((false, false, false), (false, false, true)),
            ((true, false, false), (true, false, true)),
            ((false, true, false), (false, true, true)),
            ((true, true, false), (true, true, true)),
        ] {
            o.dashed(corner(a.0, a.1, a.2), corner(b.0, b.1, b.2), ink, 1.0);
        }
        for g in fr.grips() {
            let Some(p) = o.screen(fr.world(fr.grip_local(g))) else { continue };
            let size = if g.count() == 3 { 7.0 } else { 5.5 };
            let colour = if Some(g) == self.grip {
                Color32::from_rgb(220, 30, 30)
            } else if Some(g) == self.hover {
                Color32::from_rgb(255, 160, 0)
            } else {
                Color32::from_rgb(70, 110, 220)
            };
            let r = egui::Rect::from_center_size(p, egui::vec2(size * 2.0, size * 2.0).min(egui::vec2(11.0, 11.0)));
            o.painter.rect_filled(r, 1.0, colour);
            o.painter.rect_stroke(r, 1.0, egui::Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
        }
    }
}
