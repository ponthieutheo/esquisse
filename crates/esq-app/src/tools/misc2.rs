//! Tag, Flip, Protractor, Lasso and Axes tools.

use eframe::egui::{self, Color32, Stroke};
use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::math::{Plane, TOL, basis_for};
use esq_core::model::{EntityRef, Guide, Units};

use super::{ClickDrag, Overlay, Tool, ToolCx, ToolId, c32, circle_points};
use crate::pick::{EDGE_PX, Hit, Snap, SnapKind};

/// What a click designates: an edge or guide, else a face or a group/component.
fn entity_under(cx: &ToolCx, m: DVec2) -> Option<EntityRef> {
    let ctx = &cx.doc.ctx;
    let pick = cx.infer.pick;
    let hit: Option<Hit> = pick
        .screen_edge(cx.cam(), m, EDGE_PX)
        .map(|e| e.2.clone())
        .or_else(|| pick.screen_guide(cx.cam(), m, EDGE_PX).map(|g| g.2.clone()))
        .or_else(|| {
            let (o, d) = cx.cam().ray(m);
            pick.ray_face(o, d).map(|f| f.1.clone())
        });
    hit.and_then(|h| h.selectable(ctx))
}

// ---------------------------------------------------------------------------------------

/// CTagTool's modes, toggled by Alt (sample), Shift (replace matching) and Ctrl (all
/// instances); the key held during a click works too.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TagMode {
    #[default]
    Apply,
    Sample,
    ReplaceMatching,
    AllInstances,
}

impl TagMode {
    fn toggled(self, to: Self) -> Self {
        if self == to { TagMode::Apply } else { to }
    }
}

/// Tag tool: a click gives the current tag (Window › Tags) to what is under the cursor, or
/// samples its tag, replaces that tag everywhere, or tags every instance of a component.
#[derive(Default)]
pub struct TagTool {
    hover: Option<EntityRef>,
    mode: TagMode,
    /// Ctrl and Alt held while moving or clicking: releasing them is not a tap.
    held: (bool, bool),
}

impl TagTool {
    fn mode_now(&self, mods: egui::Modifiers) -> TagMode {
        if mods.alt {
            TagMode::Sample
        } else if mods.shift {
            TagMode::ReplaceMatching
        } else if mods.ctrl || mods.command {
            TagMode::AllInstances
        } else {
            self.mode
        }
    }
}

/// Every entity of the model (all contexts) for which `pick` holds: the definition (`None`
/// for the root) and the entity.
fn model_entities(
    m: &esq_core::model::Model,
    pick: impl Fn(&esq_core::model::Entities, EntityRef) -> bool,
) -> Vec<(Option<esq_core::model::DefId>, EntityRef)> {
    let mut out = Vec::new();
    let all = std::iter::once((None, &m.root)).chain(m.defs.iter().map(|(d, def)| (Some(d), &def.entities)));
    for (d, e) in all {
        let refs = e
            .edges
            .keys()
            .map(EntityRef::Edge)
            .chain(e.faces.keys().map(EntityRef::Face))
            .chain(e.instances.keys().map(EntityRef::Instance))
            .chain(e.guides.keys().map(EntityRef::Guide))
            .chain(e.annots.keys().map(EntityRef::Annot));
        out.extend(refs.filter(|r| pick(e, *r)).map(|r| (d, r)));
    }
    out
}

impl Tool for TagTool {
    fn id(&self) -> ToolId {
        ToolId::TagTool
    }
    fn status(&self) -> String {
        let main = match self.mode {
            TagMode::Apply => "Cliquez sur un objet pour appliquer une étiquette.",
            TagMode::Sample => "Cliquez sur un objet pour utiliser son étiquette avec l'outil Étiquette.",
            TagMode::ReplaceMatching => {
                "Cliquez sur pour remplacer l'étiquette d'un élément par une étiquette correspondante."
            }
            TagMode::AllInstances => "Cliquez pour attribuer une étiquette à toutes les instances d'un composant.",
        };
        format!(
            "{main} | Alt = Activer/désactiver l'étiquette de l'échantillon. \
             | Maj = Activer/désactiver Remplacer les correspondances. \
             | Ctrl = Activer/désactiver l'application de l'étiquette à toutes les instances."
        )
    }
    fn alt_tap(&mut self, _cx: &mut ToolCx) {
        if !std::mem::take(&mut self.held.1) {
            self.mode = self.mode.toggled(TagMode::Sample);
        }
    }
    fn shift_tap(&mut self, _cx: &mut ToolCx) {
        self.mode = self.mode.toggled(TagMode::ReplaceMatching);
    }
    fn ctrl_tap(&mut self, _cx: &mut ToolCx) {
        if !std::mem::take(&mut self.held.0) {
            self.mode = self.mode.toggled(TagMode::AllInstances);
        }
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.held.0 |= cx.mods.ctrl || cx.mods.command;
        self.held.1 |= cx.mods.alt;
        self.hover = entity_under(cx, m);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(target) = self.hover else { return };
        let ents = cx.doc.ents();
        let tag = *cx.tag;
        match self.mode_now(cx.mods) {
            TagMode::Sample => {
                *cx.tag = ents.layer_of(target).unwrap_or(0);
                let name = cx.doc.model.layers.get(*cx.tag).map(|l| l.name.clone()).unwrap_or_default();
                *cx.message = Some(format!("Étiquette active : {name}"));
                // Sampling hands back to applying, as in SketchUp.
                self.mode = TagMode::Apply;
            }
            TagMode::ReplaceMatching => {
                let Some(old) = ents.layer_of(target) else { return };
                let found = model_entities(&cx.doc.model, |e, r| e.layer_of(r) == Some(old));
                cx.doc.commit("Étiqueter", |m, _| {
                    for (d, r) in &found {
                        m.entities_mut(*d).set_layer(*r, tag);
                    }
                });
            }
            TagMode::AllInstances => {
                let EntityRef::Instance(i) = target else {
                    *cx.message = Some("Cliquez sur un composant".into());
                    return;
                };
                let def = ents.instances[i].def;
                let found = model_entities(
                    &cx.doc.model,
                    |e, r| matches!(r, EntityRef::Instance(x) if e.instances[x].def == def),
                );
                cx.doc.commit("Étiqueter", |m, _| {
                    for (d, r) in &found {
                        m.entities_mut(*d).set_layer(*r, tag);
                    }
                });
            }
            TagMode::Apply => {
                let items: Vec<EntityRef> = if cx.doc.is_selected(target) { cx.doc.sel.clone() } else { vec![target] };
                cx.doc.commit("Étiqueter", |m, d| {
                    let e = m.entities_mut(d);
                    for it in &items {
                        e.set_layer(*it, tag);
                    }
                });
            }
        }
    }
    fn overlay(&self, o: &mut Overlay) {
        if let Some(EntityRef::Face(f)) = self.hover {
            o.fill_face(f, Color32::from_rgba_unmultiplied(255, 160, 0, 60));
        }
    }
}

// ---------------------------------------------------------------------------------------

/// The frame the Flip planes stand in: the model axes, or (Alt, one group or component
/// selected) the object's own axes, with the selection's extent along each.
#[derive(Clone, Copy)]
struct FlipFrame {
    axes: [DVec3; 3],
    min: DVec3,
    max: DVec3,
}

impl FlipFrame {
    fn of(doc: &crate::doc::Document, local: bool) -> Option<Self> {
        let ents = doc.ents();
        let x = doc.ctx_xform();
        let mut pts: Vec<DVec3> = ents.vertices_of(&doc.sel).iter().map(|v| ents.pos(*v)).collect();
        for e in &doc.sel {
            match *e {
                EntityRef::Instance(i) if ents.instances.contains_key(i) => {
                    let inst = &ents.instances[i];
                    let def = &doc.model.defs[inst.def].entities;
                    pts.extend(def.verts.values().map(|v| inst.xform.transform_point3(v.pos)));
                }
                EntityRef::Guide(g) if ents.guides.contains_key(g) => pts.push(ents.guides[g].pos),
                _ => {}
            }
        }
        if pts.is_empty() {
            return None;
        }
        let axes = match (local, doc.sel.as_slice()) {
            (true, [EntityRef::Instance(i)]) => {
                let w = x * ents.instances[*i].xform;
                [w.x_axis, w.y_axis, w.z_axis].map(|a| a.truncate().normalize_or(DVec3::X))
            }
            _ => [DVec3::X, DVec3::Y, DVec3::Z],
        };
        let mut f = FlipFrame { axes, min: DVec3::INFINITY, max: DVec3::NEG_INFINITY };
        for p in pts {
            let p = x.transform_point3(p);
            let c = DVec3::new(p.dot(axes[0]), p.dot(axes[1]), p.dot(axes[2]));
            f.min = f.min.min(c);
            f.max = f.max.max(c);
        }
        Some(f)
    }

    fn center(&self) -> DVec3 {
        let c = (self.min + self.max) * 0.5;
        self.axes[0] * c.x + self.axes[1] * c.y + self.axes[2] * c.z
    }

    /// The square standing for the plane across axis `i` (`px`: size of a pixel there).
    fn quad(&self, i: usize, px: f64) -> [DVec3; 4] {
        let c = self.center();
        let half = (self.max - self.min).max(DVec3::splat(px * 40.0)) * 0.6;
        let (j, k) = ((i + 1) % 3, (i + 2) % 3);
        let (u, v) = (self.axes[j] * half[j], self.axes[k] * half[k]);
        [c - u - v, c + u - v, c + u + v, c - u + v]
    }

    /// Mirror across the plane of axis `i`, moved by `offset` along its normal.
    fn mirror(&self, i: usize, offset: f64) -> DMat4 {
        let n = self.axes[i];
        let p = self.center() + n * offset;
        let r = esq_core::glam::DMat3::from_cols(
            DVec3::X - n * (2.0 * n.x),
            DVec3::Y - n * (2.0 * n.y),
            DVec3::Z - n * (2.0 * n.z),
        );
        DMat4::from_translation(p) * DMat4::from_mat3(r) * DMat4::from_translation(-p)
    }
}

/// The last flip, which a distance typed afterwards moves (CFlipTool: "Enter a distance to
/// adjust mirror plane offset").
#[derive(Clone)]
struct LastFlip {
    items: Vec<EntityRef>,
    frame: FlipFrame,
    axis: usize,
    copy: bool,
}

/// Flip: mirrors the selection across one of three planes (red, green, blue) through the
/// middle of its bounding box; Ctrl flips a copy, Alt uses the axes of a selected group or
/// component, the arrow keys flip without clicking (right: red, left: green, up/down: blue).
#[derive(Default)]
pub struct FlipTool {
    hover: Option<usize>,
    copy: bool,
    local: bool,
    /// One group or component selected (then Alt is offered).
    single_object: bool,
    has_selection: bool,
    last: Option<LastFlip>,
}

impl FlipTool {
    fn under(&self, cx: &ToolCx, m: DVec2) -> Option<usize> {
        let fr = FlipFrame::of(cx.doc, self.local)?;
        let (o, d) = cx.cam().ray(m);
        let px = cx.cam().pixel_size_at(fr.center());
        let mut best: Option<(f64, usize)> = None;
        for i in 0..3 {
            let q = fr.quad(i, px);
            let Some(t) = Plane::from_point_normal(q[0], fr.axes[i]).intersect_ray(o, d).filter(|t| *t > 0.0) else {
                continue;
            };
            let p = o + d * t - q[0];
            let (e1, e2) = (q[1] - q[0], q[3] - q[0]);
            let (a, b) = (p.dot(e1) / e1.length_squared(), p.dot(e2) / e2.length_squared());
            let inside = (-TOL..=1.0 + TOL).contains(&a) && (-TOL..=1.0 + TOL).contains(&b);
            if inside && best.is_none_or(|x| t < x.0) {
                best = Some((t, i));
            }
        }
        best.map(|b| b.1)
    }

    fn apply(cx: &mut ToolCx, f: &LastFlip, offset: f64) {
        let xf = cx.local_xform(f.frame.mirror(f.axis, offset));
        let (items, copy) = (f.items.clone(), f.copy);
        cx.doc.commit(if copy { "Retourner et copier" } else { "Retourner" }, |m, d| {
            let e = m.entities_mut(d);
            let all = e.with_bounding_edges(&items);
            let all = if copy { e.duplicate(&all) } else { all };
            e.transform_selection(&all, &xf);
        });
    }

    fn flip(&mut self, cx: &mut ToolCx, axis: usize) {
        let Some(frame) = FlipFrame::of(cx.doc, self.local) else { return };
        let f = LastFlip { items: cx.doc.sel.clone(), frame, axis, copy: self.copy };
        Self::apply(cx, &f, 0.0);
        self.last = Some(f);
    }
}

impl Tool for FlipTool {
    fn id(&self) -> ToolId {
        ToolId::Flip
    }
    fn status(&self) -> String {
        // CFlipTool.
        const CTRL: &str = "Ctrl = Alterner entre Retournement/Copie.";
        const ALT: &str = "Alt = Alterner entre Axes globaux/locaux.";
        const ARROWS: &str = "Touches fléchées = Retourner sur un plan.";
        if !self.has_selection {
            let main = if self.copy {
                "Cliquez sur quelque chose que vous souhaitez retourner et copier."
            } else {
                "Cliquez sur quelque chose que vous souhaitez retourner."
            };
            return format!("{main} | {CTRL}");
        }
        let mut main = String::from(if self.copy {
            "Cliquez ou faites glisser un plan pour retourner et copier la sélection."
        } else {
            "Cliquez ou faites glisser un plan pour retourner la sélection."
        });
        if self.last.is_some() {
            main.push_str(" Entrer une distance pour ajuster le décalage du plan du miroir.");
        }
        if self.single_object {
            format!("{main} | {CTRL} | {ALT} | {ARROWS}")
        } else {
            format!("{main} | {CTRL} | {ARROWS}")
        }
    }
    fn vcb_label(&self) -> &'static str {
        "Distance"
    }
    fn ctrl_tap(&mut self, _cx: &mut ToolCx) {
        self.copy = !self.copy;
    }
    fn alt_tap(&mut self, _cx: &mut ToolCx) {
        self.local = !self.local;
    }
    fn own_key(&mut self, cx: &mut ToolCx, key: egui::Key, mods: egui::Modifiers) -> bool {
        let axis = match key {
            egui::Key::ArrowRight => 0,
            egui::Key::ArrowLeft => 1,
            egui::Key::ArrowUp | egui::Key::ArrowDown => 2,
            _ => return false,
        };
        if cx.doc.sel.is_empty() || mods.any() {
            return false;
        }
        self.flip(cx, axis);
        true
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.has_selection = !cx.doc.sel.is_empty();
        self.single_object = matches!(cx.doc.sel.as_slice(), [EntityRef::Instance(_)]);
        self.local &= self.single_object;
        self.hover = self.under(cx, m);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        if cx.doc.sel.is_empty() {
            if let Some(e) = entity_under(cx, m) {
                cx.doc.select(vec![e]);
                self.last = None;
            } else {
                *cx.message = Some("Sélectionnez d'abord ce qu'il faut retourner".into());
            }
            return;
        }
        self.mouse_move(cx, m);
        if let Some(i) = self.hover {
            self.flip(cx, i);
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let d = cx.parse_len(text)?;
        let Some(last) = self.last.clone() else { return Err("Retournez d'abord la sélection".into()) };
        // The typed distance moves the mirror plane of the flip just made.
        cx.doc.undo();
        Self::apply(cx, &last, d);
        Ok(())
    }
    fn overlay(&self, o: &mut Overlay) {
        let colors = [0, 1, 2].map(crate::settings::axis_color);
        if o.doc.sel.is_empty() || o.doc.ents().is_empty() {
            return;
        }
        let Some(fr) = FlipFrame::of(o.doc, self.local) else { return };
        let px = o.cam.pixel_size_at(fr.center());
        for (i, col) in colors.iter().enumerate() {
            let q = fr.quad(i, px);
            let a = if self.hover == Some(i) { 110 } else { 40 };
            o.fill_polygon(&q, Color32::from_rgba_unmultiplied(col[0], col[1], col[2], a));
            o.polyline(&q, true, c32(*col), 1.5);
        }
    }
}

// ---------------------------------------------------------------------------------------

/// Protractor: centre, base direction, then the angle; makes a guide line through the
/// centre.
#[derive(Default)]
pub struct ProtractorTool {
    center: Option<(DVec3, DVec3)>,
    base: Option<DVec3>,
    angle: f64,
    snap: Option<Snap>,
    no_guide: bool,
    cd: ClickDrag,
}

impl ProtractorTool {
    fn plane_point(&self, cx: &ToolCx, m: DVec2, s: &Snap) -> DVec3 {
        let (c, axis) = self.center.unwrap();
        let pl = Plane::from_point_normal(c, axis);
        if matches!(s.kind, SnapKind::Endpoint | SnapKind::Midpoint | SnapKind::OnEdge | SnapKind::Intersection) {
            return s.pos - axis * pl.distance(s.pos);
        }
        let (ro, rd) = cx.cam().ray(m);
        pl.intersect_ray(ro, rd).map(|t| ro + rd * t).unwrap_or(s.pos)
    }

    fn finish(&mut self, cx: &mut ToolCx, angle: f64) {
        let (Some((c, axis)), Some(base)) = (self.center, self.base) else { return };
        let dir = DMat4::from_axis_angle(axis, angle).transform_vector3(base);
        if !self.no_guide {
            let x = cx.doc.ctx_xform().inverse();
            let g = Guide::line(x.transform_point3(c), x.transform_vector3(dir));
            cx.doc.commit("Rapporteur", |m, d| {
                m.entities_mut(d).guides.insert(g);
            });
        }
        *cx.message = Some(format!("Angle : {}", esq_core::units::format_angle(angle.to_degrees())));
        self.center = None;
        self.base = None;
    }
}

impl Tool for ProtractorTool {
    fn id(&self) -> ToolId {
        ToolId::Protractor
    }
    fn status(&self) -> String {
        // CProtractorTool: the step, then "Ctrl = Toggle Create Guides." (and the arrows at
        // the first step).
        const CTRL: &str = "Ctrl = Activer/désactiver la création de guides.";
        match (self.center.is_some(), self.base.is_some(), self.no_guide) {
            (false, _, _) => format!(
                "Cliquez pour définir le centre du rapporteur. | {CTRL} | {}",
                super::hints::ARROWS_ROTATION_PLANE
            ),
            (true, false, false) => format!("Cliquez pour créer un guide. | {CTRL}"),
            (true, true, false) => format!("Cliquez pour placer le guide ou entrez l'angle. | {CTRL}"),
            (true, _, true) => format!("Cliquez pour mesurer l'angle. | {CTRL}"),
        }
    }
    fn vcb_label(&self) -> &'static str {
        "Angle"
    }
    fn vcb_value(&self, _units: Units) -> String {
        if self.base.is_some() { esq_core::units::format_angle(self.angle.to_degrees()) } else { String::new() }
    }
    fn busy(&self) -> bool {
        self.center.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn ctrl_tap(&mut self, _cx: &mut ToolCx) {
        self.no_guide = !self.no_guide;
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let s = cx.snap(m, self.center.map(|c| c.0), None);
        if let (Some((c, axis)), Some(base)) = (self.center, self.base) {
            let d = (self.plane_point(cx, m, &s) - c).normalize_or_zero();
            if d != DVec3::ZERO {
                let mut a = base.cross(d).dot(axis).atan2(base.dot(d));
                // Model Info › Units › Enable angle snapping.
                if let Some(step) = esq_core::units::angle_step() {
                    let snapped = (a / step).round() * step;
                    if (a - snapped).abs() < 1.5f64.to_radians().min(step * 0.5) {
                        a = snapped;
                    }
                }
                self.angle = a;
            }
        }
        self.snap = Some(s);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(s) = self.snap.clone() else { return };
        let started = self.center.is_none();
        if self.center.is_none() {
            let axis = match (cx.lock, &s.face) {
                (Some(l), _) => l.dir,
                (None, Some((_, n))) if s.kind == SnapKind::OnFace => *n,
                _ => cx.axis(2),
            };
            self.center = Some((s.pos, axis.normalize()));
        } else if self.base.is_none() {
            let p = self.plane_point(cx, m, &s);
            let c = self.center.unwrap().0;
            if p.distance(c) > TOL {
                self.base = Some((p - c).normalize());
                self.angle = 0.0;
            }
        } else {
            let a = self.angle;
            self.finish(cx, a);
        }
        self.cd.press(m, started);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.cd.release(m)
            && self.base.is_none()
            && let (Some((c, _)), Some(s)) = (self.center, self.snap.clone())
        {
            let p = self.plane_point(cx, m, &s);
            if p.distance(c) > TOL {
                self.base = Some((p - c).normalize());
            }
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        if self.base.is_none() {
            return Err("Fixez d'abord le sommet et la ligne de base".into());
        }
        let t = text.trim();
        let a = if let Some((rise, run)) = t.split_once(':') {
            let p = |s: &str| s.trim().replace(',', ".").parse::<f64>().map_err(|_| "Pente invalide".to_string());
            p(rise)?.atan2(p(run)?)
        } else {
            t.trim_end_matches('°')
                .replace(',', ".")
                .parse::<f64>()
                .map_err(|_| "Angle invalide".to_string())?
                .to_radians()
        };
        let a = if self.angle < 0.0 { -a.abs() } else { a };
        self.finish(cx, a);
        Ok(())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.center = None;
        self.base = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        let Some((c, axis)) = self.center else { return };
        let r = o.cam.pixel_size() * 60.0;
        let start = self.base.unwrap_or_else(|| basis_for(axis).0);
        let pts = circle_points(c, axis, start, r, 48);
        o.polyline(&pts, true, Color32::from_rgb(140, 0, 160), 1.5);
        if let Some(b) = self.base {
            o.line(c, c + b * r * 1.3, Color32::from_gray(60), 1.5);
            let d = DMat4::from_axis_angle(axis, self.angle).transform_vector3(b);
            let k = o.cam.distance * 4.0;
            o.dashed(c - d * k, c + d * k, Color32::from_gray(50), 1.0);
        } else if let Some(s) = &self.snap {
            o.dashed(c, s.pos, Color32::from_gray(60), 1.5);
        }
    }
}

// ---------------------------------------------------------------------------------------

/// True North: a first click for the origin, a second one for the direction of north.
/// The direction is taken in the ground plane and ignored while the cursor stays within
/// `TOL` of the origin (SketchUp's tool uses the same 0.001" threshold); the north angle
/// is the clockwise angle of that direction from the green axis, and follows the cursor
/// until the second click.
#[derive(Default)]
pub struct NorthTool {
    origin: Option<DVec3>,
    angle: Option<f64>,
    snap: Option<Snap>,
    cd: ClickDrag,
}

impl NorthTool {
    fn aim(&mut self, cx: &mut ToolCx) {
        let (Some(o), Some(s)) = (self.origin, self.snap.as_ref()) else { return };
        let d = s.pos - o;
        // Same test as SketchUp: nothing changes until the cursor leaves a TOL-sized box.
        if d.x.abs() < TOL && d.y.abs() < TOL {
            return;
        }
        let a = d.x.atan2(d.y).to_degrees();
        self.angle = Some(a);
        let mut m = cx.doc.model.clone();
        m.shadow.north_angle = a;
        cx.doc.set_preview(Some(m));
    }
}

impl Tool for NorthTool {
    fn id(&self) -> ToolId {
        ToolId::North
    }
    fn status(&self) -> String {
        // CNorthTool.
        if self.origin.is_none() {
            "Sélectionnez le point de référence du nord".into()
        } else {
            "Sélectionnez la direction du nord.".into()
        }
    }
    fn busy(&self) -> bool {
        self.origin.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn vcb_label(&self) -> &'static str {
        "Angle"
    }
    fn vcb_value(&self, _units: Units) -> String {
        self.angle.map(|a| format!("{a:.1}°").replace('.', ",")).unwrap_or_default()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.snap = Some(cx.snap(m, self.origin, None));
        self.aim(cx);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let started = self.origin.is_none();
        match (self.origin, self.angle) {
            (None, _) => {
                self.origin = self.snap.as_ref().map(|s| s.pos);
                self.angle = None;
            }
            (Some(_), Some(a)) => self.commit(cx, a),
            (Some(_), None) => {}
        }
        self.cd.press(m, started);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.cd.release(m) {
            self.mouse_move(cx, m);
            if let Some(a) = self.angle {
                self.commit(cx, a);
            }
        }
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.origin = None;
        self.angle = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        let (Some(c), Some(s)) = (self.origin, &self.snap) else { return };
        o.dashed(c, s.pos, Color32::from_rgb(200, 120, 0), 1.5);
        if let Some(a) = self.angle {
            let (sa, ca) = a.to_radians().sin_cos();
            let tip = c + DVec3::new(sa, ca, 0.0) * o.cam.pixel_size() * 70.0;
            o.line(c, tip, Color32::from_rgb(200, 120, 0), 2.0);
            o.text_at(tip, "N");
        }
    }
}

impl NorthTool {
    fn commit(&mut self, cx: &mut ToolCx, angle: f64) {
        cx.doc.set_preview(None);
        cx.doc.commit("Vrai Nord", |m, _| m.shadow.north_angle = angle);
        *cx.message = Some(format!("Angle du nord : {angle:.1}°").replace('.', ","));
        self.origin = None;
        self.angle = None;
    }
}

// ---------------------------------------------------------------------------------------

/// Lasso: draw a loop around entities to select them (Ctrl adds, Shift toggles,
/// Ctrl+Shift removes). Drawn counter-clockwise, it also takes what the loop crosses. A
/// click without dragging starts a lasso made of segments, one more at each click, closed by
/// a double-click (LassoSelectionTool's "segmented" kind).
#[derive(Default)]
pub struct LassoTool {
    pts: Vec<DVec2>,
    dragging: bool,
    segmented: bool,
    cur: DVec2,
}

impl LassoTool {
    fn select(&mut self, cx: &mut ToolCx) {
        let pts = std::mem::take(&mut self.pts);
        self.segmented = false;
        self.dragging = false;
        if pts.len() < 3 {
            return;
        }
        // Screen y points down: a positive area is clockwise on screen.
        let area: f64 = (0..pts.len()).map(|i| pts[i].perp_dot(pts[(i + 1) % pts.len()])).sum();
        // Preferences › Drawing › Lasso direction: which way round takes what is crossed.
        let crossing = if crate::settings::with(|s| s.lasso_cw_crossing) { area > 0.0 } else { area < 0.0 };
        let ctx = cx.doc.ctx.clone();
        let mut items: Vec<EntityRef> = cx
            .infer
            .pick
            .hits_in_poly(cx.cam(), &pts, crossing)
            .into_iter()
            .filter_map(|h| h.selectable(&ctx))
            .collect();
        items.sort();
        items.dedup();
        super::select::apply(cx, items);
    }
}

impl Tool for LassoTool {
    fn id(&self) -> ToolId {
        ToolId::Lasso
    }
    fn status(&self) -> String {
        // LassoSelectionTool.
        if self.segmented {
            "Double-cliquez pour fermer les limites de la sélection.".into()
        } else if self.dragging {
            "Cliquez et faites glisser pour tracer les limites de la sélection.".into()
        } else {
            "Cliquez ou cliquez et faites glisser pour tracer les limites de la sélection. | Maj = Ajouter/Soustraire. \
             | Ctrl = Ajouter. | Maj + Ctrl = Soustraire."
                .into()
        }
    }
    fn busy(&self) -> bool {
        !self.pts.is_empty()
    }
    fn mouse_move(&mut self, _cx: &mut ToolCx, m: DVec2) {
        self.cur = m;
        if self.dragging && self.pts.last().is_some_and(|p| p.distance(m) > 3.0) {
            self.pts.push(m);
        }
    }
    fn press(&mut self, _cx: &mut ToolCx, m: DVec2) {
        self.cur = m;
        if self.segmented {
            if self.pts.last().is_none_or(|p| p.distance(m) > 3.0) {
                self.pts.push(m);
            }
            return;
        }
        self.pts = vec![m];
        self.dragging = true;
    }
    fn release(&mut self, cx: &mut ToolCx, _m: DVec2) {
        if !self.dragging {
            return;
        }
        self.dragging = false;
        if self.pts.len() >= 3 {
            self.select(cx);
        } else {
            // A click: the lasso goes on segment by segment.
            self.pts.truncate(1);
            self.segmented = true;
        }
    }
    fn double_click(&mut self, cx: &mut ToolCx, _m: DVec2) {
        if self.segmented {
            self.select(cx);
        }
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.pts.clear();
        self.segmented = false;
        self.dragging = false;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        let mut pts = self.pts.clone();
        if self.segmented {
            pts.push(self.cur);
        }
        if pts.len() > 1 {
            let s: Vec<egui::Pos2> = pts.iter().map(|p| o.origin + egui::vec2(p.x as f32, p.y as f32)).collect();
            o.painter.add(egui::Shape::line(s, Stroke::new(1.0, Color32::from_gray(40))));
        }
    }
}

// ---------------------------------------------------------------------------------------

/// Axes: places the drawing axes (origin, then the direction of a first axis, then of a
/// second); Alt cycles which axis comes first (red, green, blue), a double-click keeps the
/// current orientation for what is not set yet.
#[derive(Default)]
pub struct AxesTool {
    origin: Option<DVec3>,
    /// Direction given to the first axis.
    first_dir: Option<DVec3>,
    /// Index of the axis set first (0 red, 1 green, 2 blue).
    first: usize,
    snap: Option<Snap>,
    /// Change Axes of a component (`CComponentCSTool`, right-click menu): the path of the
    /// collection that holds it and the instance; the tool is done once the axes are set.
    component: Option<(Vec<esq_core::model::InstanceId>, esq_core::model::InstanceId)>,
    /// Make Component › Set Component Axes (`IDC_PICK_ORIGIN`): the axes go to the dialog.
    for_new_component: bool,
    finished: bool,
}

impl AxesTool {
    /// Set Component Axes of the « Créer un composant » dialog: the axes picked are given to
    /// the dialog (`make_component::axes_picked`), which shows again.
    pub fn for_new_component() -> Self {
        AxesTool { for_new_component: true, ..Default::default() }
    }

    /// Change Axes of the component `inst` of the collection at `path`.
    pub fn for_component(path: Vec<esq_core::model::InstanceId>, inst: esq_core::model::InstanceId) -> Self {
        AxesTool { component: Some((path, inst)), ..Default::default() }
    }

    /// The component's placement in the world, and its definition.
    fn component_xform(&self, m: &esq_core::model::Model) -> Option<(DMat4, esq_core::model::DefId)> {
        let (path, inst) = self.component.as_ref()?;
        let x = crate::context_menu::path_xform(m, path);
        let mut parent = &m.root;
        for i in path {
            parent = &m.defs[parent.instances.get(*i)?.def].entities;
        }
        let i = parent.instances.get(*inst)?;
        Some((x * i.xform, i.def))
    }

    /// The axes with `a` along axis `first` and `b` (made perpendicular) along the next one.
    fn axes_from(first: usize, o: DVec3, a: DVec3, b: DVec3) -> Option<DMat4> {
        let b = (b - a * b.dot(a)).normalize_or_zero();
        if b == DVec3::ZERO {
            return None;
        }
        let mut cols = [DVec3::ZERO; 3];
        cols[first] = a;
        cols[(first + 1) % 3] = b;
        cols[(first + 2) % 3] = a.cross(b);
        Some(DMat4::from_cols(cols[0].extend(0.0), cols[1].extend(0.0), cols[2].extend(0.0), o.extend(1.0)))
    }

    fn set(&mut self, cx: &mut ToolCx, axes: DMat4) {
        if self.for_new_component {
            crate::make_component::axes_picked(axes);
            self.finished = true;
        } else if self.component.is_some() {
            if let Some((w, def)) = self.component_xform(&cx.doc.model) {
                // The new axes in the definition's coordinates.
                let frame = w.inverse() * axes;
                cx.doc.commit("Modifier les axes", |m, _| {
                    m.change_definition_axes(def, frame);
                });
            }
            self.finished = true;
        } else if cx.doc.ctx.is_empty() {
            cx.doc.commit("Axes", |m, _| m.axes = axes);
        } else {
            *cx.message = Some("Fermez le groupe pour déplacer les axes du modèle".into());
        }
        self.origin = None;
        self.first_dir = None;
    }
}

impl Tool for AxesTool {
    fn id(&self) -> ToolId {
        ToolId::AxesTool
    }
    fn status(&self) -> String {
        // CCoordinateTool: the step, then "Alt = Alternate axis orientation (… is locked)."
        const NAMES: [&str; 3] = ["rouge", "vert", "bleu"];
        let alt = format!("Alt = alterner l'orientation de l'axe (l'axe {} est verrouillé).", NAMES[self.first]);
        match (self.origin.is_some(), self.first_dir.is_some()) {
            (false, _) => "Cliquez pour définir la nouvelle origine ou double-cliquez pour définir les axes selon leur orientation actuelle.".into(),
            (true, false) => format!(
                "Cliquez pour définir l'axe {} ou double-cliquez pour définir les axes selon leur orientation. {alt}",
                NAMES[self.first]
            ),
            (true, true) => format!("Cliquez pour définir l'axe {}. {alt}", NAMES[(self.first + 1) % 3]),
        }
    }
    fn busy(&self) -> bool {
        self.origin.is_some()
    }
    fn done(&self) -> bool {
        self.finished
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn alt_tap(&mut self, _cx: &mut ToolCx) {
        if self.first_dir.is_none() {
            self.first = (self.first + 1) % 3;
        }
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.snap = Some(cx.snap(m, self.origin, None));
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(p) = self.snap.as_ref().map(|s| s.pos) else { return };
        match (self.origin, self.first_dir) {
            (None, _) => self.origin = Some(p),
            (Some(o), None) => {
                if p.distance(o) > TOL {
                    self.first_dir = Some((p - o).normalize());
                }
            }
            (Some(o), Some(a)) => {
                if let Some(axes) = Self::axes_from(self.first, o, a, p - o) {
                    self.set(cx, axes);
                }
            }
        }
    }
    fn double_click(&mut self, cx: &mut ToolCx, _m: DVec2) {
        // The first click of the double-click set the origin (or the first axis): the rest
        // keeps the current orientation.
        let Some(o) = self.origin else { return };
        let cur = match self.component_xform(&cx.doc.model) {
            Some((w, _)) => w,
            None => cx.doc.model.axes,
        };
        let col = |i: usize| cur.col(i).truncate().normalize_or(DVec3::AXES[i]);
        let axes = match self.first_dir {
            None => Some(DMat4::from_cols(cur.x_axis, cur.y_axis, cur.z_axis, o.extend(1.0))),
            Some(a) => Self::axes_from(self.first, o, a, col((self.first + 1) % 3))
                .or_else(|| Self::axes_from(self.first, o, a, col((self.first + 2) % 3).cross(a))),
        };
        if let Some(axes) = axes {
            self.set(cx, axes);
        }
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.origin = None;
        self.first_dir = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        let (Some(org), Some(s)) = (self.origin, &self.snap) else { return };
        let len = o.cam.pixel_size() * 80.0;
        // Preferences › Accessibility colours the axes.
        let colors = [0, 1, 2].map(crate::settings::axis_color);
        match self.first_dir {
            None => o.line(org, org + (s.pos - org).normalize_or_zero() * len, c32(colors[self.first]), 2.5),
            Some(a) => {
                if let Some(axes) = Self::axes_from(self.first, org, a, s.pos - org) {
                    for (i, c) in colors.iter().enumerate() {
                        o.line(org, org + axes.col(i).truncate() * len, c32(*c), 2.5);
                    }
                }
            }
        }
    }
}
