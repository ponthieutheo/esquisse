//! Modification tools: push/pull, move/copy, rotate, scale, offset.

use eframe::egui::Color32;
use esq_core::angle::{
    DEFAULT_SNAP_STEP, PROTRACTOR_RADIUS_PX, SNAP_TOLERANCE_PX, continuous_angle, parse_angle, protractor_angle,
};
use esq_core::fold::MoveLimit;
use esq_core::glam::{DMat3, DMat4, DVec2, DVec3};
use esq_core::math::{Plane, TOL, line_closest_to_ray, point_in_polygon_2d};
use esq_core::model::{EdgeId, EntityRef, FaceId, InstanceId, SnapPoint, Units};
use esq_core::ops::offset_polygon;
use esq_core::units::format_length;

use super::move_grips::{
    Grip, grip_angle, grip_object, grip_rotation, grip_under, grips_enabled, grips_of, paint_grips,
};
use super::move_handles::{self as handles, GripKind, Picked};
use super::{ClickDrag, Overlay, Tool, ToolCx, ToolId, circle_points};
use crate::pick::{Snap, SnapKind};

/// Face of the active context under the cursor.
fn hovered_face(cx: &ToolCx, s: &Snap) -> Option<FaceId> {
    s.face.as_ref().and_then(|(h, _)| h.face_in(&cx.doc.ctx))
}

/// What a modification tool acts on: the selection, or the entity under the cursor.
pub(super) fn targets(cx: &ToolCx, s: Option<&Snap>) -> Vec<EntityRef> {
    if !cx.doc.sel.is_empty() {
        return cx.doc.sel.clone();
    }
    let Some(s) = s else { return Vec::new() };
    let ctx = &cx.doc.ctx;
    let hit = s.edge.as_ref().map(|e| &e.0).or(s.face.as_ref().map(|f| &f.0));
    hit.and_then(|h| h.selectable(ctx)).map(|e| vec![e]).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------

#[derive(Default)]
pub struct PushPullTool {
    hover: Option<FaceId>,
    active: Option<(FaceId, DVec3, DVec3)>,
    dist: f64,
    /// CPushPullTool's modes, exclusive: new starting face (Ctrl) or stretch (Alt).
    keep_base: bool,
    stretch: bool,
    /// The one face selected before the tool: a click anywhere pushes or pulls it.
    preselected: Option<FaceId>,
    last: Option<f64>,
    /// Last committed push/pull, so typing a value afterwards adjusts it.
    last_op: Option<(FaceId, bool)>,
    snap: Option<Snap>,
    cd: ClickDrag,
}

impl PushPullTool {
    fn local_dist(cx: &ToolCx, f: FaceId, world: f64) -> f64 {
        let n = cx.doc.ents().faces[f].normal;
        let scale = cx.doc.ctx_xform().transform_vector3(n).length();
        world / scale.max(1e-12)
    }

    fn apply(&mut self, cx: &mut ToolCx, f: FaceId, dist: f64, keep: bool) {
        let d = Self::local_dist(cx, f, dist);
        let stretch = self.stretch || cx.mods.alt;
        cx.doc.commit("Pousser/tirer", |m, def| {
            let ents = m.entities_mut(def);
            if stretch {
                ents.stretch_face(f, d);
            } else {
                ents.push_pull(f, d, keep);
            }
        });
        self.last = Some(dist);
        self.last_op = Some((f, keep));
        self.active = None;
        self.hover = None;
    }

    fn update_preview(&self, cx: &mut ToolCx) {
        let Some((f, _, _)) = self.active else { return };
        let mut m = cx.doc.model.clone();
        let def = cx.doc.ctx_def();
        let d = Self::local_dist(cx, f, self.dist);
        if self.stretch || cx.mods.alt {
            m.entities_mut(def).stretch_face(f, d);
        } else {
            m.entities_mut(def).push_pull(f, d, self.keep_base);
        }
        cx.doc.set_preview(Some(m));
    }
}

impl Tool for PushPullTool {
    fn id(&self) -> ToolId {
        ToolId::PushPull
    }
    fn status(&self) -> String {
        let main = match (self.active.is_some(), self.preselected.is_some(), self.keep_base, self.stretch) {
            (false, false, _, false) => "Cliquez pour sélectionner la face que vous souhaitez pousser/tirer.",
            (false, false, _, true) => "Cliquez pour sélectionner la face que vous souhaitez étirer.",
            (false, true, _, false) => "Cliquez pour pousser ou tirer la face sélectionnée.",
            (false, true, _, true) => "Cliquez pour étirer la face sélectionnée.",
            (true, _, true, _) => "Cliquez pour définir la nouvelle face ou entrez la distance.",
            (true, _, _, true) => "Cliquez pour définir les faces que vous étirez ou entrez la distance.",
            (true, _, _, _) => "Cliquez pour définir la face ou entrez la distance.",
        };
        format!(
            "{main} | Ctrl = Activer/désactiver créer une nouvelle face de départ. \
             | Alt = Activer/Désactiver le mode Étirer."
        )
    }
    fn vcb_label(&self) -> &'static str {
        "Distance"
    }
    fn vcb_value(&self, units: Units) -> String {
        if self.active.is_some() {
            format_length(self.dist.abs(), units)
        } else {
            self.last.map(|d| format_length(d.abs(), units)).unwrap_or_default()
        }
    }
    fn busy(&self) -> bool {
        self.active.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.active
            .and(self.snap.as_ref())
            .filter(|s| matches!(s.kind, SnapKind::Endpoint | SnapKind::Midpoint | SnapKind::OnEdge))
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        match self.active {
            None => {
                let s = cx.snap(m, None, None);
                // Pre-pick (Preferences › Drawing): a face selected beforehand is the one
                // pushed, wherever the click.
                let prepick = !crate::settings::with(|st| st.no_prepick_pushpull);
                self.preselected = match cx.doc.sel.as_slice() {
                    [EntityRef::Face(f)] if prepick && cx.doc.ents().faces.contains_key(*f) => Some(*f),
                    _ => None,
                };
                self.hover = self.preselected.or_else(|| hovered_face(cx, &s));
                self.snap = Some(s);
            }
            Some((_, start, n)) => {
                let s = cx.snap(m, Some(start), None);
                let (ro, rd) = cx.cam().ray(m);
                self.dist = match s.kind {
                    SnapKind::Endpoint | SnapKind::Midpoint | SnapKind::OnEdge => (s.pos - start).dot(n),
                    _ => line_closest_to_ray(start, n, ro, rd).unwrap_or(0.0),
                };
                self.snap = Some(s);
                self.update_preview(cx);
            }
        }
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let started = self.active.is_none();
        match self.active {
            None => {
                if let (Some(f), Some(s)) = (self.hover, &self.snap) {
                    let n = cx.doc.ctx_xform().transform_vector3(cx.doc.ents().faces[f].normal).normalize();
                    self.active = Some((f, s.pos, n));
                    self.dist = 0.0;
                }
            }
            Some((f, _, _)) => {
                if self.dist.abs() > TOL {
                    let (d, k) = (self.dist, self.keep_base);
                    self.apply(cx, f, d, k);
                }
            }
        }
        self.cd.press(m, started && self.active.is_some());
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.cd.release(m)
            && let Some((f, _, _)) = self.active
            && self.dist.abs() > TOL
        {
            let (d, k) = (self.dist, self.keep_base);
            self.apply(cx, f, d, k);
        }
    }
    fn double_click(&mut self, cx: &mut ToolCx, m: DVec2) {
        // The first click of the double-click started an operation on this face.
        let s = cx.snap(m, None, None);
        if let (Some(f), Some(d)) = (hovered_face(cx, &s), self.last) {
            cx.doc.set_preview(None);
            let k = self.keep_base;
            self.apply(cx, f, d, k);
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let d = cx.parse_len(text)?;
        if let Some((f, _, _)) = self.active {
            // The mouse gives the direction, the typed value the distance.
            let d = if self.dist.abs() > TOL { d * self.dist.signum() } else { d };
            let k = self.keep_base;
            self.apply(cx, f, d, k);
            return Ok(());
        }
        if let Some((f, k)) = self.last_op {
            // Re-do the last push/pull with the typed distance.
            cx.doc.undo();
            if !cx.doc.ents().faces.contains_key(f) {
                cx.doc.redo();
                return Err("Impossible de modifier la dernière opération".into());
            }
            // A positive value keeps the direction of the last push/pull.
            let sign = self.last.map(|l| l.signum()).unwrap_or(1.0);
            self.apply(cx, f, d * sign, k);
            return Ok(());
        }
        Err("Cliquez d'abord sur une face".into())
    }
    fn ctrl_tap(&mut self, cx: &mut ToolCx) {
        self.keep_base = !self.keep_base;
        self.stretch &= !self.keep_base;
        self.update_preview(cx);
    }
    fn alt_tap(&mut self, cx: &mut ToolCx) {
        self.stretch = !self.stretch;
        self.keep_base &= !self.stretch;
        self.update_preview(cx);
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.active = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        if self.active.is_none() {
            if let Some(f) = self.hover {
                o.fill_face(f, Color32::from_rgba_unmultiplied(60, 90, 255, 70));
            }
        } else if let Some((_, start, n)) = self.active {
            o.dashed(start, start + n * self.dist, Color32::from_gray(80), 1.5);
            o.text_at(start + n * self.dist, &format_length(self.dist.abs(), o.units));
        }
    }
}

// ---------------------------------------------------------------------------------------

#[derive(Default)]
pub struct MoveTool {
    items: Vec<EntityRef>,
    base: Option<DVec3>,
    vector: DVec3,
    copy: bool,
    /// Stamp (third Ctrl mode, with `copy`): every click places one more copy, from the same
    /// base point, until Esc.
    stamp: bool,
    /// Something is selected before the first click.
    preselected: bool,
    /// Auto-fold (Alt): free displacement, faces that stop being flat are folded. Without it the
    /// displacement is restricted so that neighbouring faces stay flat (after CMoveTool's
    /// motion constraints), and folded only when no such displacement exists.
    autofold: bool,
    /// Displacement before the flatness restriction.
    raw: DVec3,
    limit: MoveLimit,
    snap: Option<Snap>,
    /// Last copy, for "3x" / "/3" arrays typed afterwards.
    last_copy: Option<(Vec<EntityRef>, DVec3)>,
    cd: ClickDrag,
    /// Rotation grips of the object under the cursor (`move_grips.rs`), the one hovered, and
    /// the turn started from a grip (with its angle).
    grips: Vec<Grip>,
    grip_hover: Option<Grip>,
    turning: Option<(Grip, f64)>,
    /// Edit › Paste (CPasteTool): the pasted selection follows the cursor by the corner of
    /// its bounding box until a click places it.
    paste: bool,
    /// The handles of a selected group or component (`move_handles.rs`): the grips offered
    /// (Alt cycles them), the object showing them and the handle under the cursor.
    grip_kind: GripKind,
    hovered: Option<InstanceId>,
    picked: Option<Picked>,
    /// A move started at a snap: that snap (world), the snaps it can meet, the tractor beam's
    /// snap and whether the beam is on (Alt), and the turn of the moved object about the base.
    start_snap: Option<SnapPoint>,
    snap_targets: Vec<(InstanceId, SnapPoint)>,
    beam: Option<SnapPoint>,
    beam_on: bool,
    turn: DMat3,
    grip_pts: Vec<DVec3>,
    last_m: DVec2,
}

impl MoveTool {
    /// The tool after Edit › Paste.
    pub fn pasting() -> Self {
        MoveTool { paste: true, ..Default::default() }
    }

    /// Paste: takes the selection by the low corner of its bounding box.
    fn grab_pasted(&mut self, cx: &mut ToolCx) {
        let items = cx.doc.sel.clone();
        let ents = cx.doc.ents();
        let x = cx.doc.ctx_xform();
        let moved = ents.vertices_of(&ents.with_bounding_edges(&items));
        let mut pts: Vec<DVec3> = moved.iter().map(|v| x.transform_point3(ents.pos(*v))).collect();
        for it in &items {
            if let EntityRef::Instance(i) = it
                && let Some(inst) = ents.instances.get(*i)
            {
                pts.push(x.transform_point3(inst.xform.w_axis.truncate()));
            }
        }
        let min = pts.iter().copied().fold(DVec3::INFINITY, DVec3::min);
        if !min.is_finite() {
            return;
        }
        self.limit = ents.move_limit(&moved).transformed(&x);
        self.items = items;
        self.base = Some(min);
        self.vector = DVec3::ZERO;
        self.raw = DVec3::ZERO;
    }

    fn turned(cx: &ToolCx, g: &Grip, angle: f64) -> esq_core::model::Model {
        let mut m = cx.doc.model.clone();
        let r = grip_rotation(cx, g, angle);
        let def = cx.doc.ctx_def();
        if let Some(i) = m.entities_mut(def).instances.get_mut(g.inst) {
            i.xform = r * i.xform;
        }
        m
    }

    fn finish_turn(&mut self, cx: &mut ToolCx, angle: f64) {
        let Some((g, _)) = self.turning.take() else { return };
        if angle.abs() > 1e-9 {
            let m = Self::turned(cx, &g, angle);
            cx.doc.commit("Faire pivoter", |model, _| *model = m);
        } else {
            cx.doc.set_preview(None);
        }
    }
}

/// Array count typed in the Measurements box: "3x", "x3", "*3", "3*", "/3", "3/" (SketchUp
/// accepts `*`, `x`, `X` and `/` before or after the number). The flag is true for "/".
fn parse_array(t: &str) -> Option<(usize, bool)> {
    let t = t.trim();
    let is_op = |c: char| matches!(c, '*' | 'x' | 'X' | '/');
    let first = t.chars().next()?;
    let last = t.chars().last()?;
    let (op, digits) = if is_op(first) {
        (first, &t[first.len_utf8()..])
    } else if is_op(last) {
        (last, &t[..t.len() - last.len_utf8()])
    } else {
        return None;
    };
    let n: usize = digits.trim().parse().ok()?;
    Some((n, op == '/'))
}

impl MoveTool {
    fn transformed(
        cx: &ToolCx,
        items: &[EntityRef],
        v: DVec3,
        copy: bool,
        count: usize,
        divide: bool,
        turn: Option<(DVec3, DMat3)>,
    ) -> esq_core::model::Model {
        let mut m = cx.doc.model.clone();
        let def = cx.doc.ctx_def();
        let ents = m.entities_mut(def);
        let all = ents.with_bounding_edges(items);
        // A move that connects snaps also turns the objects about the base point.
        let xf = |t: DVec3| match turn {
            Some((b, r)) => handles::moved(b, b + t, r),
            None => DMat4::from_translation(t),
        };
        if copy {
            for i in 1..=count {
                let f = if divide { i as f64 / count as f64 } else { i as f64 };
                let dup = ents.duplicate(&all);
                ents.transform_selection(&dup, &cx.local_xform(xf(v * f)));
            }
        } else {
            let moved = ents.vertices_of(&all);
            ents.transform_selection(&all, &cx.local_xform(xf(v)));
            ents.fold_nonplanar(&moved);
        }
        m
    }

    /// The turn about the base point (a move started at a snap), if any.
    fn snap_turn(&self) -> Option<(DVec3, DMat3)> {
        let b = self.base?;
        (!self.turn.abs_diff_eq(DMat3::IDENTITY, 1e-12)).then_some((b, self.turn))
    }

    /// The displacement actually applied for a requested one.
    fn constrained(&self, raw: DVec3) -> DVec3 {
        if self.autofold || self.copy || self.limit == MoveLimit::Fixed { raw } else { self.limit.apply(raw) }
    }

    fn refresh_preview(&mut self, cx: &mut ToolCx) {
        if self.base.is_some() {
            self.vector = self.constrained(self.raw);
            let m = Self::transformed(cx, &self.items, self.vector, self.copy, 1, false, self.snap_turn());
            cx.doc.set_preview(Some(m));
        }
    }

    /// While a move started at a snap: the snap the moved one meets under the cursor, the
    /// tractor beam's snap, and the turn (`move_handles.rs`). Returns where the base goes.
    fn connect(&mut self, cx: &ToolCx, m: DVec2, s: &mut Snap) -> DVec3 {
        let Some(own) = self.start_snap else { return s.pos };
        let target = handles::target_under(cx, m, &self.snap_targets);
        let to = target.map_or(s.pos, |t| t.pos);
        self.beam = handles::beam_target(cx, m, to, &self.snap_targets);
        // A face of the moved objects themselves does not count.
        let face = s
            .face
            .as_ref()
            .filter(|(h, _)| !h.selectable(&cx.doc.ctx).is_some_and(|e| self.items.contains(&e)))
            .map(|f| f.1);
        self.turn = if let Some(t) = target {
            *s = Snap { pos: t.pos, kind: SnapKind::Handle(crate::pick::Handle::Snap, false), ..s.clone() };
            handles::facing(&own, t.dir, t.up)
        } else if let Some(b) = self.beam.filter(|_| self.beam_on) {
            handles::facing(&own, b.dir, b.up)
        } else if let Some(n) = face {
            handles::facing_face(cx, &own, n, s.pos)
        } else {
            DMat3::IDENTITY
        };
        to
    }

    fn finish(&mut self, cx: &mut ToolCx, v: DVec3) {
        self.paste = false;
        if v.length() < TOL && self.snap_turn().is_none() && (!self.copy || self.stamp) {
            if !self.stamp {
                self.base = None;
            }
            cx.doc.set_preview(None);
            return;
        }
        let m = Self::transformed(cx, &self.items, v, self.copy, 1, false, self.snap_turn());
        let label = if self.copy { "Copier" } else { "Déplacer" };
        cx.doc.commit(label, |model, _| *model = m);
        self.last_copy = self.copy.then(|| (self.items.clone(), v));
        if self.stamp {
            // Same items, same base point: the next click stamps another copy.
            self.vector = DVec3::ZERO;
            self.raw = DVec3::ZERO;
            return;
        }
        self.base = None;
        self.items.clear();
        self.end_snap_move();
    }

    fn end_snap_move(&mut self) {
        self.start_snap = None;
        self.snap_targets.clear();
        self.beam = None;
        self.beam_on = false;
        self.turn = DMat3::IDENTITY;
    }
}

impl Tool for MoveTool {
    fn ctrl_tap(&mut self, cx: &mut ToolCx) {
        // CMoveTool: before the first click Ctrl cycles copy, stamp, move (auto-fold goes to
        // copy); while placing it switches between moving and copying.
        if self.base.is_some() {
            self.copy = !self.copy;
            self.stamp = false;
        } else if self.stamp {
            (self.copy, self.stamp) = (false, false);
        } else if self.copy {
            self.stamp = true;
        } else {
            self.copy = true;
        }
        if self.copy {
            self.autofold = false;
        }
        self.refresh_preview(cx);
    }
    fn alt_tap(&mut self, cx: &mut ToolCx) {
        // CMoveTool (Alt released alone): the tractor beam during a move started at a snap;
        // before the first click, over a selected object showing its grips, the next type of
        // grips (nothing while a grip is under the cursor); otherwise auto-fold.
        let m = self.last_m;
        if self.base.is_some() && self.start_snap.is_some() {
            self.beam_on = !self.beam_on;
            self.mouse_move(cx, m);
            return;
        }
        if self.base.is_none() && self.hovered.is_some() {
            use crate::pick::Handle;
            let on_grip = self.picked.is_some_and(|p| !matches!(p.handle, Handle::Snap | Handle::Origin));
            if !on_grip {
                self.grip_kind = self.grip_kind.next();
                self.mouse_move(cx, m);
            }
            return;
        }
        self.autofold = !self.autofold;
        if self.autofold {
            (self.copy, self.stamp) = (false, false);
        }
        self.refresh_preview(cx);
    }
    fn id(&self) -> ToolId {
        ToolId::Move
    }
    fn status(&self) -> String {
        // CMoveTool's messages; the rotation grips have their own, without modifiers.
        if self.turning.is_some() {
            return "Cliquez pour définir la rotation ou entrez l'angle.".into();
        }
        if self.base.is_none() && self.grip_hover.is_some() {
            return "Cliquez pour commencer à faire pivoter cet objet.".into();
        }
        if self.paste {
            return "Placer le contenu du Presse-papiers.".into();
        }
        let started = self.base.is_some();
        let main = match (self.stamp, self.copy, self.autofold) {
            (true, _, _) => "Cliquez pour créer plusieurs copies.",
            (_, true, _) if started => "Cliquez pour placer les éléments que vous copiez ou entrez une distance.",
            (_, true, _) if self.preselected => "Cliquez pour commencer à copier les éléments présélectionnés.",
            (_, true, _) => "Cliquez sur quelque chose pour commencer à le copier.",
            (_, _, true) if started => {
                "Cliquez pour placer l'élément que vous pliez automatiquement ou entrez la distance."
            }
            (_, _, true) if self.preselected => {
                "Cliquez pour commencer le pliage automatique des éléments présélectionnés."
            }
            (_, _, true) => "Cliquez sur un élément pour le plier automatiquement.",
            _ if started => "Cliquez pour placer les éléments que vous déplacez ou entrez une distance.",
            _ if self.preselected => "Cliquez pour commencer à déplacer les éléments présélectionnés.",
            _ => "Cliquez sur quelque chose pour commencer à le déplacer.",
        };
        let ctrl = match (started, self.copy) {
            (false, _) => "Alterner entre copier/tamponner/déplacer.",
            (true, true) => "Déplacer.",
            (true, false) => "Copier.",
        };
        // grips shown, then a tractor beam's snap, then auto-fold.
        let alt = if !started && self.hovered.is_some() {
            "Parcourez les types de poignées."
        } else if started && self.beam.is_some() {
            "Activer/désactiver le faisceau tracteur."
        } else {
            "Activer/Désactiver le pliage automatique."
        };
        format!("{main} | Ctrl = {ctrl} | Alt = {alt} | {}", super::hints::ARROWS_INFERENCE)
    }
    fn vcb_label(&self) -> &'static str {
        if self.turning.is_some() { "Angle" } else { "Distance" }
    }
    fn vcb_value(&self, units: Units) -> String {
        if let Some((_, a)) = self.turning {
            return format!("{:.1}°", a.to_degrees()).replace('.', ",");
        }
        if self.base.is_some() { format_length(self.vector.length(), units) } else { String::new() }
    }
    fn busy(&self) -> bool {
        self.base.is_some() || self.turning.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        if let Some((g, _)) = self.turning {
            let a = grip_angle(cx, &g, m).unwrap_or(0.0);
            self.turning = Some((g, a));
            let model = Self::turned(cx, &g, a);
            cx.doc.set_preview(Some(model));
            return;
        }
        self.last_m = m;
        self.preselected = !cx.doc.sel.is_empty();
        if self.paste && self.base.is_none() && self.preselected {
            self.grab_pasted(cx);
        }
        let mut s = cx.snap(m, self.base, None);
        if let Some(b) = self.base {
            let to = self.connect(cx, m, &mut s);
            self.raw = to - b;
            self.snap = Some(s);
            self.refresh_preview(cx);
            return;
        }
        if grips_enabled() {
            self.grips = grip_object(cx, m).map(|i| grips_of(cx, i)).unwrap_or_default();
            self.grip_hover = grip_under(cx, m);
        } else {
            self.grips.clear();
            self.grip_hover = None;
        }
        // The snaps, origin and grips of a selected group or component (not over a
        // rotation grip, nor while pasting).
        (self.hovered, self.picked) =
            if self.grip_hover.is_none() && !self.paste { handles::pick(cx, m, self.grip_kind) } else { (None, None) };
        self.grip_pts = self.hovered.map(|i| handles::shown_grips(cx, i, self.grip_kind)).unwrap_or_default();
        if let Some(p) = &self.picked {
            s = handles::handle_snap(cx, p);
        }
        self.snap = Some(s);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        if let Some((_, a)) = self.turning {
            self.finish_turn(cx, a);
            return;
        }
        if self.base.is_none()
            && let Some(g) = self.grip_hover
        {
            // A rotation grip: turn the object about it.
            self.turning = Some((g, 0.0));
            self.cd.press(m, true);
            return;
        }
        let started = self.base.is_none();
        if started {
            let items = targets(cx, self.snap.as_ref());
            if items.is_empty() {
                *cx.message = Some("Sélectionnez d'abord ce qu'il faut déplacer".into());
                return;
            }
            if cx.doc.sel.is_empty() {
                cx.doc.select(items.clone());
            }
            let ents = cx.doc.ents();
            let moved = ents.vertices_of(&ents.with_bounding_edges(&items));
            self.limit = ents.move_limit(&moved).transformed(&cx.doc.ctx_xform());
            self.items = items;
            self.base = self.snap.as_ref().map(|s| s.pos);
            self.vector = DVec3::ZERO;
            self.raw = DVec3::ZERO;
            // Taken by a snap: it can meet the snaps of the other objects.
            self.end_snap_move();
            self.start_snap = self.picked.and_then(|p| p.snap);
            if self.start_snap.is_some() {
                self.snap_targets = handles::targets(cx, &self.items);
            }
            self.hovered = None;
            self.grip_pts.clear();
        } else {
            let v = self.vector;
            self.finish(cx, v);
        }
        self.cd.press(m, started);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.cd.release(m) {
            if let Some((_, a)) = self.turning {
                self.finish_turn(cx, a);
                return;
            }
            let v = self.vector;
            self.finish(cx, v);
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        if self.turning.is_some() {
            let a = parse_angle(text).ok_or_else(|| format!("Angle invalide : « {text} »"))?;
            self.finish_turn(cx, a);
            return Ok(());
        }
        let t = text.trim().to_lowercase();
        let looks_like_array = t.starts_with(['*', 'x', '/']) || t.ends_with(['*', 'x', '/']);
        if looks_like_array {
            let (n, divide) = parse_array(&t).ok_or_else(|| "Nombre de copies invalide".to_string())?;
            let Some((items, v)) = self.last_copy.clone() else { return Err("Faites d'abord une copie (Ctrl)".into()) };
            cx.doc.undo();
            let m = Self::transformed(cx, &items, v, true, n.max(1), divide, None);
            cx.doc.commit("Réseau de copies", |model, _| *model = m);
            return Ok(());
        }
        let len = cx.parse_len(&t)?;
        if self.base.is_some() {
            let dir = self.vector.normalize_or_zero();
            if dir == DVec3::ZERO {
                return Err("Indiquez une direction avec la souris".into());
            }
            self.finish(cx, dir * len);
            return Ok(());
        }
        Err("Cliquez d'abord le point de base".into())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.base = None;
        self.turning = None;
        self.items.clear();
        self.end_snap_move();
        cx.doc.set_preview(None);
        if std::mem::take(&mut self.paste) {
            // Escape while pasting: nothing is pasted.
            cx.doc.undo();
        }
    }
    fn overlay(&self, o: &mut Overlay) {
        if let Some((g, a)) = &self.turning {
            paint_grips(o, std::slice::from_ref(g), Some(g), Some(*a));
            return;
        }
        if let (Some(b), Some(s)) = (self.base, &self.snap)
            && s.guide.is_none()
        {
            o.dashed(b, s.pos, Color32::from_gray(70), 1.5);
        }
        if self.base.is_none() {
            paint_grips(o, &self.grips, self.grip_hover.as_ref(), None);
            handles::paint(o, &self.grip_pts, self.picked.map(|p| p.pos));
        }
        // The tractor beam: from the moved snap to the snap it turns toward.
        if let (Some(b), Some(t)) = (self.base, self.beam.filter(|_| self.beam_on)) {
            o.dashed(b + self.vector, t.pos, Color32::from_rgb(230, 120, 0), 2.0);
        }
    }
}

// ---------------------------------------------------------------------------------------

#[derive(Default)]
pub struct RotateTool {
    items: Vec<EntityRef>,
    center: Option<(DVec3, DVec3)>,
    ref_dir: Option<DVec3>,
    angle: f64,
    copy: bool,
    /// Axis found under the cursor when the centre was clicked (restored when the arrow-key
    /// lock is released).
    auto_axis: DVec3,
    /// An arrow key locked the rotation plane.
    plane_locked: bool,
    /// Nothing is selected: the first click picks what turns.
    nothing_selected: bool,
    snap: Option<Snap>,
    cd: ClickDrag,
}

impl RotateTool {
    fn rotation(&self, angle: f64) -> DMat4 {
        let (c, axis) = self.center.unwrap_or((DVec3::ZERO, DVec3::Z));
        DMat4::from_translation(c) * DMat4::from_axis_angle(axis, angle) * DMat4::from_translation(-c)
    }

    fn model_with(&self, cx: &ToolCx, angle: f64) -> esq_core::model::Model {
        let mut m = cx.doc.model.clone();
        let def = cx.doc.ctx_def();
        let ents = m.entities_mut(def);
        let all = ents.with_bounding_edges(&self.items);
        let xf = cx.local_xform(self.rotation(angle));
        if self.copy {
            let dup = ents.duplicate(&all);
            ents.transform_selection(&dup, &xf);
        } else {
            ents.transform_selection(&all, &xf);
        }
        m
    }

    fn finish(&mut self, cx: &mut ToolCx, angle: f64) {
        let m = self.model_with(cx, angle);
        cx.doc.commit(if self.copy { "Copie pivotée" } else { "Faire pivoter" }, |model, _| *model = m);
        self.center = None;
        self.ref_dir = None;
        self.items.clear();
    }

    fn plane_point(&self, cx: &ToolCx, m: DVec2, s: &Snap) -> DVec3 {
        let (c, axis) = self.center.unwrap();
        let pl = Plane::from_point_normal(c, axis);
        if matches!(s.kind, SnapKind::Endpoint | SnapKind::Midpoint | SnapKind::OnEdge) {
            return s.pos - axis * pl.distance(s.pos);
        }
        let (ro, rd) = cx.cam().ray(m);
        pl.intersect_ray(ro, rd).map(|t| ro + rd * t).unwrap_or(s.pos)
    }
}

impl Tool for RotateTool {
    fn ctrl_tap(&mut self, cx: &mut ToolCx) {
        self.copy = !self.copy;
        if self.ref_dir.is_some() {
            let pm = self.model_with(cx, self.angle);
            cx.doc.set_preview(Some(pm));
        }
    }
    fn id(&self) -> ToolId {
        ToolId::Rotate
    }
    fn status(&self) -> String {
        // CRotateTool: the arrow keys are listed until the centre is set (they still lock the
        // plane until the start point).
        const CTRL: &str = "Ctrl = Activer/Désactiver Copier.";
        let step = match (self.center.is_some(), self.ref_dir.is_some(), self.copy) {
            (false, _, _) => {
                let first = if self.nothing_selected {
                    "Cliquez sur une quelque chose pour sélectionner et pour définir le point central de rotation."
                } else {
                    "Cliquez pour définir le point central de la rotation."
                };
                return format!("{first} | {CTRL} | {}", super::hints::ARROWS_ROTATION_PLANE);
            }
            (true, false, false) => {
                "Déplacez le curseur pour localiser le point de départ de la rotation et cliquez pour démarrer la rotation."
            }
            (true, false, true) => {
                "Déplacez le curseur pour localiser le point de départ de la rotation et cliquez pour commencer à faire pivoter une copie."
            }
            (true, true, false) => "Cliquez pour définir la rotation ou entrez l'angle.",
            (true, true, true) => "Cliquez pour définir la copie pivotée ou entrez l'angle.",
        };
        format!("{step} | {CTRL}")
    }
    fn vcb_label(&self) -> &'static str {
        "Angle"
    }
    fn vcb_value(&self, _units: Units) -> String {
        if self.ref_dir.is_some() { esq_core::units::format_angle(self.angle.to_degrees()) } else { String::new() }
    }
    fn busy(&self) -> bool {
        self.center.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let reference = self.center.map(|c| c.0);
        // Arrow keys lock (and unlock) the rotation plane until the start point is set.
        if let (Some((c, _)), None) = (self.center, self.ref_dir) {
            match cx.lock.filter(|l| matches!(l.kind, SnapKind::Axis(_))) {
                Some(l) => {
                    self.center = Some((c, l.dir.normalize()));
                    self.plane_locked = true;
                }
                None if self.plane_locked => {
                    self.center = Some((c, self.auto_axis));
                    self.plane_locked = false;
                }
                None => {}
            }
        }
        self.nothing_selected = cx.doc.sel.is_empty();
        let s = cx.snap(m, reference, None);
        if let (Some((c, axis)), Some(r)) = (self.center, self.ref_dir) {
            let p = self.plane_point(cx, m, &s);
            let v = p - c;
            let d = v.normalize_or_zero();
            if d != DVec3::ZERO {
                let raw = r.cross(d).dot(axis).atan2(r.dot(d));
                let mut a = continuous_angle(raw, self.angle);
                // Pointing at a point of the model gives its exact direction; elsewhere the
                // protractor snaps (rim and rays, after CRotateTool).
                if !matches!(s.kind, SnapKind::Endpoint | SnapKind::Midpoint | SnapKind::OnEdge) {
                    let px = cx.cam().pixel_size_at(p);
                    // Model Info › Units › Enable angle snapping (off: 0.1° steps everywhere).
                    let step = esq_core::units::angle_step().unwrap_or(0.1f64.to_radians());
                    let snapped =
                        protractor_angle(a.abs(), v.length() / px, PROTRACTOR_RADIUS_PX, SNAP_TOLERANCE_PX, step);
                    a = snapped * a.signum();
                }
                self.angle = a;
                let pm = self.model_with(cx, a);
                cx.doc.set_preview(Some(pm));
            }
        }
        self.snap = Some(s);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(s) = self.snap.clone() else { return };
        let started = self.center.is_none();
        if self.center.is_none() {
            let items = targets(cx, Some(&s));
            if items.is_empty() {
                *cx.message = Some("Sélectionnez d'abord ce qu'il faut faire pivoter".into());
                return;
            }
            if cx.doc.sel.is_empty() {
                cx.doc.select(items.clone());
            }
            self.items = items;
            let axis = match (cx.lock, &s.face) {
                (Some(l), _) => l.dir,
                (None, Some((_, n))) if s.kind == SnapKind::OnFace => *n,
                _ => cx.axis(2),
            };
            self.center = Some((s.pos, axis.normalize()));
            self.plane_locked = cx.lock.is_some_and(|l| matches!(l.kind, SnapKind::Axis(_)));
            self.auto_axis = match (&s.face, s.kind) {
                (Some((_, n)), SnapKind::OnFace) => n.normalize(),
                _ => cx.axis(2),
            };
            self.angle = 0.0;
        } else if self.ref_dir.is_none() {
            let p = self.plane_point(cx, m, &s);
            let c = self.center.unwrap().0;
            if p.distance(c) > TOL {
                self.ref_dir = Some((p - c).normalize());
            }
        } else {
            let a = self.angle;
            self.finish(cx, a);
        }
        self.cd.press(m, started);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.cd.release(m)
            && self.ref_dir.is_none()
            && let Some(s) = self.snap.clone()
        {
            let p = self.plane_point(cx, m, &s);
            let c = self.center.unwrap().0;
            if p.distance(c) > TOL {
                self.ref_dir = Some((p - c).normalize());
            }
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let mut a = parse_angle(text).ok_or_else(|| "Angle invalide".to_string())?;
        if self.ref_dir.is_none() {
            return Err("Fixez d'abord le centre et la référence".into());
        }
        // A typed angle turns the way the cursor was turning (CRotateTool).
        if self.angle < 0.0 {
            a = -a;
        }
        self.finish(cx, a);
        Ok(())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.center = None;
        self.ref_dir = None;
        self.items.clear();
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        let Some((c, axis)) = self.center else { return };
        let r = o.cam.pixel_size_at(c) * PROTRACTOR_RADIUS_PX;
        let pts = circle_points(c, axis, esq_core::math::basis_for(axis).0, r, 48);
        // Ticks every 15 degrees from the reference direction (or the plane's own axis).
        let zero = self.ref_dir.unwrap_or_else(|| esq_core::math::basis_for(axis).0);
        for i in 0..24 {
            let d = DMat4::from_axis_angle(axis, i as f64 * DEFAULT_SNAP_STEP).transform_vector3(zero);
            let long = if i % 6 == 0 { 0.12 } else { 0.06 };
            o.line(c + d * r * (1.0 - long), c + d * r, Color32::from_gray(110), 1.0);
        }
        let col = if axis.dot(DVec3::Z).abs() > 0.99 {
            Color32::from_rgb(30, 50, 230)
        } else if axis.dot(DVec3::X).abs() > 0.99 {
            Color32::from_rgb(220, 30, 30)
        } else if axis.dot(DVec3::Y).abs() > 0.99 {
            Color32::from_rgb(20, 150, 40)
        } else {
            Color32::from_rgb(140, 0, 160)
        };
        o.polyline(&pts, true, col, 1.5);
        if let Some(rd) = self.ref_dir {
            o.line(c, c + rd * r * 1.3, Color32::from_gray(60), 1.5);
            let cur = DMat4::from_axis_angle(axis, self.angle).transform_vector3(rd);
            o.line(c, c + cur * r * 1.3, Color32::from_gray(20), 2.0);
        }
    }
}

// ---------------------------------------------------------------------------------------

/// A chain of selected edges being offset.
struct EdgeOffset {
    edges: Vec<EdgeId>,
    /// World points in travel order.
    pts: Vec<DVec3>,
    closed: bool,
    /// Normal of the chain's plane: positive distances go to the left of travel.
    n: DVec3,
    /// The point from which the offset is measured.
    reference: DVec3,
}

impl EdgeOffset {
    /// Signed distance from the chain to `q` (in its plane): positive on the left of travel.
    fn signed_distance(&self, q: DVec3) -> f64 {
        let m = self.pts.len();
        let segs = if self.closed { m } else { m - 1 };
        let mut best = (f64::INFINITY, 1.0);
        for i in 0..segs {
            let (a, b) = (self.pts[i], self.pts[(i + 1) % m]);
            let c = crate::pick::closest_on_segment(q, a, b);
            let d = q.distance(c);
            if d < best.0 {
                let left = self.n.cross((b - a).normalize_or_zero());
                best = (d, if (q - c).dot(left) < 0.0 { -1.0 } else { 1.0 });
            }
        }
        best.0 * best.1
    }

    fn plane_point(&self, cx: &ToolCx, m: DVec2, fallback: DVec3) -> DVec3 {
        let (ro, rd) = cx.cam().ray(m);
        Plane::from_point_normal(self.pts[0], self.n).intersect_ray(ro, rd).map(|t| ro + rd * t).unwrap_or(fallback)
    }
}

#[derive(Default)]
pub struct OffsetTool {
    face: Option<FaceId>,
    hover: Option<FaceId>,
    edge: Option<EdgeOffset>,
    dist: f64,
    last: Option<f64>,
    /// Alt: keep the parts of a face offset that fold back over themselves (COffsetTool trims
    /// them by default).
    allow_overlap: bool,
    /// Something to offset under the cursor or selected.
    target: bool,
    snap: Option<Snap>,
    cd: ClickDrag,
}

impl OffsetTool {
    /// World loops of the offset of face `f` at `dist`, trimmed unless the overlap is allowed.
    fn face_loops(doc: &crate::doc::Document, f: FaceId, dist: f64, allow_overlap: bool) -> Vec<Vec<DVec3>> {
        let (pts, n) = Self::loop_world(doc, f);
        let off = offset_polygon(&pts, n, dist);
        if allow_overlap {
            return vec![off];
        }
        esq_core::offset::trim_overlap(&off, n, esq_core::offset::loop_area_sign(&pts, n))
    }

    /// The edges selected beforehand, as a chain; `None` when the selection is not made of
    /// edges only, `Some(Err(..))` when they cannot be offset.
    fn selected_chain(cx: &ToolCx, s: &Snap) -> Option<Result<EdgeOffset, String>> {
        let sel = &cx.doc.sel;
        let edges: Vec<EdgeId> =
            sel.iter().filter_map(|e| if let EntityRef::Edge(e) = e { Some(*e) } else { None }).collect();
        if edges.is_empty() || edges.len() != sel.len() {
            return None;
        }
        let ents = cx.doc.ents();
        let x = cx.doc.ctx_xform();
        Some((|| {
            let chain = ents.edge_chain(&edges).map_err(str::to_string)?;
            let pts: Vec<DVec3> = chain.verts.iter().map(|v| x.transform_point3(ents.pos(*v))).collect();
            let n =
                esq_core::offset::chain_normal(&pts).ok_or("Les arêtes doivent être coplanaires et non alignées")?;
            Ok(EdgeOffset { edges, pts, closed: chain.closed, n, reference: s.pos })
        })())
    }

    fn finish_edges(&mut self, cx: &mut ToolCx, d: f64) {
        let Some(eo) = self.edge.take() else { return };
        let scale = cx.doc.ctx_xform().transform_vector3(DVec3::X).length().max(1e-12);
        let dl = d / scale;
        let res = cx.doc.try_commit("Décalage", |m, def| m.entities_mut(def).offset_edges(&eo.edges, dl).map(|_| ()));
        match res {
            Ok(()) => self.last = Some(d),
            Err(e) => *cx.message = Some(e),
        }
        cx.doc.set_preview(None);
    }

    fn loop_world(cx_doc: &crate::doc::Document, f: FaceId) -> (Vec<DVec3>, DVec3) {
        let ents = cx_doc.ents();
        let x = cx_doc.ctx_xform();
        let face = &ents.faces[f];
        let pts = face.outer.iter().map(|v| x.transform_point3(ents.pos(*v))).collect();
        (pts, x.transform_vector3(face.normal).normalize())
    }

    fn finish(&mut self, cx: &mut ToolCx, d: f64) {
        let Some(f) = self.face else { return };
        let scale = cx.doc.ctx_xform().transform_vector3(DVec3::X).length().max(1e-12);
        let dl = d / scale;
        let allow = self.allow_overlap;
        cx.doc.commit("Décalage", |m, def| {
            m.entities_mut(def).offset_face_with(f, dl, allow);
        });
        self.last = Some(d);
        self.face = None;
    }

    fn signed_distance(pts: &[DVec3], n: DVec3, p: DVec3) -> f64 {
        let pl = Plane::from_point_normal(pts[0], n);
        let q = p - n * pl.distance(p);
        let mut d = f64::INFINITY;
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            d = d.min(q.distance(crate::pick::closest_on_segment(q, a, b)));
        }
        let poly: Vec<DVec2> = pts.iter().map(|p| pl.to_2d(*p)).collect();
        if point_in_polygon_2d(pl.to_2d(q), &poly) { d } else { -d }
    }
}

impl Tool for OffsetTool {
    fn id(&self) -> ToolId {
        ToolId::Offset
    }
    fn status(&self) -> String {
        // COffsetTool: "%s %s", the step then the Alt toggle.
        let main = if self.busy() {
            "Sélectionnez un point pour définir le décalage ou tapez une valeur."
        } else if self.target {
            "Sélectionnez le point à partir duquel le décalage sera mesuré."
        } else {
            "Sélectionnez la face ou les arêtes à décaler."
        };
        let alt =
            if self.allow_overlap { "Alt = découper le chevauchement." } else { "Alt = autoriser le chevauchement." };
        format!("{main} {alt}")
    }
    fn alt_tap(&mut self, _cx: &mut ToolCx) {
        self.allow_overlap = !self.allow_overlap;
    }
    fn vcb_label(&self) -> &'static str {
        "Distance"
    }
    fn vcb_value(&self, units: Units) -> String {
        if self.face.is_some() || self.edge.is_some() { format_length(self.dist.abs(), units) } else { String::new() }
    }
    fn busy(&self) -> bool {
        self.face.is_some() || self.edge.is_some()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let s = cx.snap(m, None, None);
        if let Some(eo) = &self.edge {
            let q = eo.plane_point(cx, m, s.pos);
            self.dist = eo.signed_distance(q) - eo.signed_distance(eo.reference);
            self.snap = Some(s);
            return;
        }
        match self.face {
            None => {
                self.hover = hovered_face(cx, &s);
                self.target = self.hover.is_some() || !cx.doc.sel.is_empty();
            }
            Some(f) => {
                let (pts, n) = Self::loop_world(cx.doc, f);
                let (ro, rd) = cx.cam().ray(m);
                let pl = Plane::from_point_normal(pts[0], n);
                let p = pl.intersect_ray(ro, rd).map(|t| ro + rd * t).unwrap_or(s.pos);
                self.dist = Self::signed_distance(&pts, n, p);
            }
        }
        self.snap = Some(s);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        if self.edge.is_some() {
            if self.dist.abs() > TOL {
                let d = self.dist;
                self.finish_edges(cx, d);
            }
            self.cd.press(m, false);
            return;
        }
        let started = self.face.is_none();
        if started {
            // Edges selected beforehand take over: this first click is the point the
            // offset is measured from.
            if let Some(s) = self.snap.clone()
                && let Some(chain) = Self::selected_chain(cx, &s)
            {
                match chain {
                    Ok(mut eo) => {
                        eo.reference = eo.plane_point(cx, m, s.pos);
                        self.edge = Some(eo);
                        self.dist = 0.0;
                        self.cd.press(m, true);
                    }
                    Err(e) => *cx.message = Some(e),
                }
                return;
            }
            self.face = self.hover;
        } else if self.dist.abs() > TOL {
            let d = self.dist;
            self.finish(cx, d);
        }
        self.cd.press(m, started && self.face.is_some());
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.cd.release(m) && self.dist.abs() > TOL {
            let d = self.dist;
            if self.edge.is_some() {
                self.finish_edges(cx, d);
            } else {
                self.finish(cx, d);
            }
        }
    }
    fn double_click(&mut self, cx: &mut ToolCx, m: DVec2) {
        let s = cx.snap(m, None, None);
        if let (Some(f), Some(d)) = (hovered_face(cx, &s), self.last) {
            self.face = Some(f);
            self.finish(cx, d);
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let d = cx.parse_len(text)?;
        if self.edge.is_some() {
            // The mouse gives the side, the typed value the distance.
            let side = if self.dist < 0.0 { -1.0 } else { 1.0 };
            self.finish_edges(cx, d.abs() * side);
            return Ok(());
        }
        if self.face.is_none() {
            return Err("Cliquez d'abord sur une face".into());
        }
        self.finish(cx, d);
        Ok(())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.face = None;
        self.edge = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        if let Some(eo) = &self.edge {
            let off = esq_core::offset::offset_chain(&eo.pts, eo.closed, eo.n, self.dist);
            o.polyline(&off, eo.closed, Color32::BLACK, 2.0);
            return;
        }
        match self.face {
            None => {
                if let Some(f) = self.hover {
                    o.fill_face(f, Color32::from_rgba_unmultiplied(60, 90, 255, 60));
                }
            }
            Some(f) => {
                if !o.doc.ents().faces.contains_key(f) {
                    return;
                }
                for l in Self::face_loops(o.doc, f, self.dist, self.allow_overlap) {
                    o.polyline(&l, true, Color32::BLACK, 2.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_array;

    #[test]
    fn array_counts_accept_the_four_operators_on_either_side() {
        for t in ["3x", "x3", "3X", "X3", "3*", "*3"] {
            assert_eq!(parse_array(t), Some((3, false)), "{t}");
        }
        for t in ["/4", "4/"] {
            assert_eq!(parse_array(t), Some((4, true)), "{t}");
        }
        assert_eq!(parse_array("1500"), None);
        assert_eq!(parse_array("x"), None);
    }
}
