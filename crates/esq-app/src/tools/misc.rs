//! Eraser, paint bucket, tape measure.

use std::collections::HashSet;

use eframe::egui::{self, Color32};
use esq_core::glam::{DVec2, DVec3};
use esq_core::math::TOL;

use esq_core::model::{Entities, EntityRef, FaceId, Guide, GuideId, MaterialId, Units};
use esq_core::units::format_length;

use super::{Overlay, Tool, ToolCx, ToolId};
use crate::pick::{EDGE_PX, Snap, SnapKind};

/// What the Eraser does with what it touches (CEraseTool): Ctrl softens and smooths, Shift
/// hides, Alt unsoftens and unhides; a tap toggles the mode, a held key applies it to the
/// click (Ctrl+Shift held on a click unsoftens too, as in older versions).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum EraseMode {
    #[default]
    Erase,
    Hide,
    Soften,
    Unsoften,
}

impl EraseMode {
    fn held(m: egui::Modifiers) -> Option<Self> {
        let ctrl = m.ctrl || m.command;
        match (ctrl, m.shift, m.alt) {
            (true, true, _) => Some(EraseMode::Unsoften),
            (true, false, _) => Some(EraseMode::Soften),
            (false, true, _) => Some(EraseMode::Hide),
            (false, false, true) => Some(EraseMode::Unsoften),
            (false, false, false) => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            EraseMode::Erase => "Effacer",
            EraseMode::Hide => "Masquer",
            EraseMode::Soften => "Adoucir/lisser",
            EraseMode::Unsoften => "Ne plus adoucir/démasquer",
        }
    }

    fn toggled(self, to: Self) -> Self {
        if self == to { EraseMode::Erase } else { to }
    }
}

/// Erases edges (and the faces they bound) and groups/components; drag to erase several.
#[derive(Default)]
pub struct EraserTool {
    marked: Vec<EntityRef>,
    hover: Option<EntityRef>,
    dragging: bool,
    /// Mode toggled by tapping Ctrl, Alt or Shift.
    mode: EraseMode,
    /// The mode the next click applies (held keys first), for the status bar.
    shown: EraseMode,
    /// Ctrl+Shift held during a drag: what the cursor passes over is unmarked.
    deselecting: bool,
    deselected: bool,
    press_at: Option<DVec2>,
    /// Keys held at the last click: releasing them is not a tap.
    held: (bool, bool),
}

impl EraserTool {
    fn under(cx: &ToolCx, m: DVec2) -> Option<EntityRef> {
        let ctx = &cx.doc.ctx;
        if let Some(hit) = cx.infer.pick.screen_annot(cx.cam(), m, EDGE_PX)
            && let Some(e) = hit.selectable(ctx)
        {
            return Some(e);
        }
        if let Some((_, _, hit, _)) = cx.infer.pick.screen_edge(cx.cam(), m, EDGE_PX)
            && let Some(e) = hit.selectable(ctx)
        {
            return Some(e);
        }
        if let Some((_, _, hit)) = cx.infer.pick.screen_guide(cx.cam(), m, EDGE_PX)
            && let Some(e) = hit.selectable(ctx)
        {
            return Some(e);
        }
        let (o, d) = cx.cam().ray(m);
        let (_, hit, _) = cx.infer.pick.ray_face(o, d)?;
        match hit.selectable(ctx)? {
            EntityRef::Instance(i) => Some(EntityRef::Instance(i)),
            _ => None,
        }
    }
}

impl Tool for EraserTool {
    fn id(&self) -> ToolId {
        ToolId::Eraser
    }
    fn status(&self) -> String {
        const MODS: &str = "Ctrl = Activer/Désactiver adoucir/lisser. \
             | Alt = Activer/Désactiver annuler l'adoucissement/révéler. | Maj = Activer/Désactiver masquer.";
        let main = match self.shown {
            _ if self.deselecting => "Continuez à faire glisser pour désélectionner des éléments.",
            EraseMode::Erase => "Cliquez ou faites glisser pour effacer les éléments.",
            EraseMode::Soften => "Cliquez ou faites glisser pour adoucir/lisser les arêtes.",
            EraseMode::Hide => "Cliquez ou faites glisser pour masquer les éléments.",
            EraseMode::Unsoften => "Cliquez ou faites glisser annuler l'adoucissement/révéler les éléments.",
        };
        if self.dragging {
            format!("{main} | {MODS} | Ctrl + Maj = Activer/Désactiver désélectionner.")
        } else {
            format!("{main} | {MODS}")
        }
    }
    fn ctrl_tap(&mut self, _cx: &mut ToolCx) {
        if !std::mem::take(&mut self.held.0) {
            self.mode = self.mode.toggled(EraseMode::Soften);
        }
        self.shown = self.mode;
    }
    fn alt_tap(&mut self, _cx: &mut ToolCx) {
        if !std::mem::take(&mut self.held.1) {
            self.mode = self.mode.toggled(EraseMode::Unsoften);
        }
        self.shown = self.mode;
    }
    fn shift_tap(&mut self, _cx: &mut ToolCx) {
        self.mode = self.mode.toggled(EraseMode::Hide);
        self.shown = self.mode;
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.hover = Self::under(cx, m);
        let ctrl = cx.mods.ctrl || cx.mods.command;
        // Keys held while the cursor moves are held keys, not taps.
        self.held.0 |= ctrl;
        self.held.1 |= cx.mods.alt;
        let dragged = self.press_at.is_some_and(|p| p.distance(m) > 4.0);
        self.deselecting = self.dragging && dragged && ctrl && cx.mods.shift;
        self.shown = EraseMode::held(cx.mods).unwrap_or(self.mode);
        if self.deselecting {
            if let Some(e) = self.hover {
                self.marked.retain(|x| *x != e);
                self.deselected = true;
            }
        } else if self.dragging
            && let Some(e) = self.hover
            && !self.marked.contains(&e)
        {
            self.marked.push(e);
        }
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.held.0 |= cx.mods.ctrl || cx.mods.command;
        self.held.1 |= cx.mods.alt;
        self.dragging = true;
        self.deselected = false;
        self.press_at = Some(m);
        self.marked.clear();
        self.mouse_move(cx, m);
    }
    fn release(&mut self, cx: &mut ToolCx, _m: DVec2) {
        self.dragging = false;
        self.deselecting = false;
        self.press_at = None;
        if self.marked.is_empty() {
            return;
        }
        let items = std::mem::take(&mut self.marked);
        // Ctrl+Shift that deselected during the drag is not a mode.
        let mode = match EraseMode::held(cx.mods) {
            Some(EraseMode::Unsoften) if self.deselected && cx.mods.shift => self.mode,
            Some(m) => m,
            None => self.mode,
        };
        cx.doc.commit(mode.label(), |m, d| {
            let ents = m.entities_mut(d);
            if mode == EraseMode::Erase {
                ents.erase(&items);
                return;
            }
            for it in &items {
                match (*it, mode) {
                    (EntityRef::Edge(e), _) => {
                        let Some(edge) = ents.edges.get_mut(e) else { continue };
                        match mode {
                            EraseMode::Hide => edge.hidden = true,
                            EraseMode::Soften => (edge.soft, edge.smooth) = (true, true),
                            _ => (edge.soft, edge.smooth, edge.hidden) = (false, false, false),
                        }
                    }
                    (EntityRef::Instance(i), EraseMode::Hide) => {
                        if let Some(inst) = ents.instances.get_mut(i) {
                            inst.hidden = true;
                        }
                    }
                    (EntityRef::Instance(i), EraseMode::Unsoften) => {
                        if let Some(inst) = ents.instances.get_mut(i) {
                            inst.hidden = false;
                        }
                    }
                    _ => {}
                }
            }
        });
    }
    fn busy(&self) -> bool {
        self.dragging
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.dragging = false;
        self.marked.clear();
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        let ents = o.doc.ents();
        let x = o.doc.ctx_xform();
        let draw = |e: &EntityRef, c: Color32| match *e {
            EntityRef::Edge(id) if ents.edges.contains_key(id) => {
                let (a, b) = ents.edge_points(id);
                o.line(x.transform_point3(a), x.transform_point3(b), c, 3.0);
            }
            EntityRef::Guide(id) if ents.guides.contains_key(id) => {
                let g = ents.guides[id].transformed(&x);
                let k = if g.infinite { o.cam.distance * 4.0 } else { 1.0 };
                if g.is_point() {
                    o.line(g.pos, g.pos, c, 6.0);
                } else if g.infinite {
                    o.line(g.pos - g.dir * k, g.pos + g.dir * k, c, 3.0);
                } else {
                    o.line(g.pos, g.pos + g.dir, c, 3.0);
                }
            }
            _ => {}
        };
        for e in &self.marked {
            draw(e, Color32::from_rgb(40, 90, 255));
        }
        if let Some(h) = &self.hover {
            draw(h, Color32::from_rgb(255, 120, 0));
        }
    }
}

/// The faces reachable from `start` through shared edges whose front material is `mat`.
fn connected_with_material(ents: &Entities, start: FaceId, mat: Option<MaterialId>) -> Vec<FaceId> {
    let mut seen: HashSet<FaceId> = HashSet::from([start]);
    let mut todo = vec![start];
    while let Some(f) = todo.pop() {
        for e in ents.face_edges(f) {
            for &g in &ents.edges[e].faces {
                if ents.faces[g].front == mat && seen.insert(g) {
                    todo.push(g);
                }
            }
        }
    }
    seen.into_iter().collect()
}

/// Gives `new` to every face of `ents` whose front material is `old`.
fn recolor(ents: &mut Entities, old: Option<MaterialId>, new: Option<MaterialId>) {
    for face in ents.faces.values_mut() {
        if face.front == old {
            face.front = new;
        }
    }
}

/// Paints faces and groups/components with the current material. Alt+click picks a
/// material; Ctrl, Shift and Ctrl+Shift paint the faces that share the clicked face's one.
#[derive(Default)]
pub struct PaintTool {
    hover: Option<EntityRef>,
    /// Modifiers held at the last move, which pick CPaintTool's mode.
    mods: egui::Modifiers,
}

impl Tool for PaintTool {
    fn id(&self) -> ToolId {
        ToolId::Paint
    }
    fn status(&self) -> String {
        let (m, ctrl) = (self.mods, self.mods.ctrl || self.mods.command);
        if m.alt {
            return "Cliquez sur une face pour charger sa matière dans l'outil Colorier.".into();
        }
        let main = match (ctrl, m.shift) {
            (true, true) => "Cliquez pour colorier les faces correspondantes du même objet.",
            (false, true) => "Cliquez pour colorier les faces correspondantes.",
            (true, false) => "Cliquez pour colorier les faces connectées avec la matière correspondante.",
            (false, false) => "Cliquez pour colorier un élément ou objet.",
        };
        format!(
            "{main} | Alt = Prélevez un échantillon de la matière. | Maj = Coloriez toutes les faces correspondantes. \
             | Ctrl = Colorier tous les éléments connectés. | Maj + Ctrl = Tout colorier sur le même objet."
        )
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mods = cx.mods;
        let (o, d) = cx.cam().ray(m);
        self.hover = cx.infer.pick.ray_face(o, d).and_then(|(_, h, _)| h.selectable(&cx.doc.ctx));
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(target) = self.hover else { return };
        if cx.mods.alt {
            // Sample Material (`tools/sample.rs`), texture position included.
            super::sample::sample_at(cx, m);
            return;
        }
        let mat = *cx.material;
        let x = cx.doc.ctx_xform();
        // Ctrl: the connected faces with the same material. Shift: every face of the model
        // with it. Ctrl+Shift: those of the same object (the active group or component).
        if let EntityRef::Face(f) = target
            && (cx.mods.ctrl || cx.mods.command || cx.mods.shift)
        {
            let ents = cx.doc.ents();
            let Some(old) = ents.faces.get(f).map(|x| x.front) else { return };
            let ctrl = cx.mods.ctrl || cx.mods.command;
            if ctrl && !cx.mods.shift {
                let faces = connected_with_material(ents, f, old);
                cx.doc.commit("Colorier", |m, d| {
                    let ents = m.entities_mut(d);
                    for f in faces {
                        ents.faces[f].front = mat;
                        super::sample::align_painted(ents, f, mat, &x);
                    }
                });
            } else if ctrl {
                cx.doc.commit("Colorier", |m, d| recolor(m.entities_mut(d), old, mat));
            } else {
                cx.doc.commit("Colorier", |m, _| {
                    recolor(&mut m.root, old, mat);
                    for def in m.defs.values_mut() {
                        recolor(&mut def.entities, old, mat);
                    }
                });
            }
            return;
        }
        let items: Vec<EntityRef> = if cx.doc.is_selected(target) { cx.doc.sel.clone() } else { vec![target] };
        cx.doc.commit("Colorier", |m, d| {
            let ents = m.entities_mut(d);
            for it in &items {
                match *it {
                    EntityRef::Face(f) => {
                        if let Some(face) = ents.faces.get_mut(f) {
                            face.front = mat;
                            super::sample::align_painted(ents, f, mat, &x);
                        }
                    }
                    EntityRef::Instance(i) => {
                        if let Some(inst) = ents.instances.get_mut(i) {
                            inst.material = mat;
                        }
                    }
                    _ => {}
                }
            }
        });
    }
    fn overlay(&self, o: &mut Overlay) {
        if let Some(EntityRef::Face(f)) = self.hover {
            o.fill_face(f, Color32::from_rgba_unmultiplied(255, 160, 0, 60));
        }
    }
}

/// What the Tape Measure started from: a point (measures point to point, makes a guide
/// point) or a line (measures the offset from it, makes a parallel guide line).
#[derive(Clone, Copy)]
enum TapeFrom {
    Point,
    Line { dir: DVec3 },
}

/// The guide made by the last measurement, which a typed value can still move.
struct LastGuide {
    id: GuideId,
    origin: DVec3,
    dir: DVec3,
    line: Option<DVec3>,
}

/// CMeasureTool's modes, cycled by Ctrl: guide lines (from an edge; a guide point from a
/// point), guide points only, or measuring only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TapeMode {
    #[default]
    Lines,
    Points,
    Measure,
}

/// Measures distances and makes construction guides, as SketchUp's Tape Measure.
#[derive(Default)]
pub struct TapeTool {
    start: Option<(DVec3, TapeFrom)>,
    snap: Option<Snap>,
    measured: Option<f64>,
    mode: TapeMode,
    last: Option<LastGuide>,
}

impl TapeTool {
    /// Offset from the start: the whole vector for a point, the perpendicular part for a line.
    fn offset(&self, q: DVec3) -> Option<DVec3> {
        let (s, from) = self.start?;
        Some(match from {
            TapeFrom::Point => q - s,
            TapeFrom::Line { dir } => {
                let v = q - s;
                v - dir * v.dot(dir)
            }
        })
    }

    /// Ends the measurement at offset `off` from the start.
    fn finish(&mut self, cx: &mut ToolCx, off: DVec3) {
        let Some((s, from)) = self.start.take() else { return };
        let dist = off.length();
        self.measured = Some(dist);
        *cx.message = Some(format!("Distance : {}", cx.len(dist)));
        self.last = None;
        if self.mode == TapeMode::Measure || dist < TOL {
            return;
        }
        let line = match from {
            TapeFrom::Line { dir } if self.mode == TapeMode::Lines => Some(dir),
            _ => None,
        };
        let id = make_guide(cx, s + off, line, None);
        self.last = id.map(|id| LastGuide { id, origin: s, dir: off / dist, line });
    }
}

/// Adds (or, with `replace`, moves) a guide through the world point `p`: a line along
/// `line`, or a point.
fn make_guide(cx: &mut ToolCx, p: DVec3, line: Option<DVec3>, replace: Option<GuideId>) -> Option<GuideId> {
    let x = cx.doc.ctx_xform().inverse();
    let pos = x.transform_point3(p);
    let guide = match line {
        Some(d) => Guide::line(pos, x.transform_vector3(d)),
        None => Guide::point(pos),
    };
    let mut id = None;
    let label = if replace.is_some() { "Modifier le guide" } else { "Guide" };
    cx.doc.commit(label, |m, d| {
        let ents = m.entities_mut(d);
        id = match replace.and_then(|r| ents.guides.get_mut(r).map(|g| (r, g))) {
            Some((r, g)) => {
                *g = Guide { layer: g.layer, ..guide };
                Some(r)
            }
            None => Some(ents.guides.insert(guide)),
        };
    });
    id
}

impl Tool for TapeTool {
    fn id(&self) -> ToolId {
        ToolId::Tape
    }
    fn status(&self) -> String {
        // CMeasureTool (2026). Texts SketchUp does not translate are translated here in the
        // words of its neighbours.
        let started = self.start.is_some();
        let main = match self.mode {
            TapeMode::Lines if started => "Cliquez pour placer le guide ou entrez la distance.",
            TapeMode::Lines if self.measured.is_some() && self.last.is_none() => {
                "Cliquez pour créer un guide ou entrez la distance pour redimensionner le modèle."
            }
            TapeMode::Lines => "Cliquez pour créer un guide.",
            TapeMode::Points if started => "Cliquez pour placer le point de guide ou entrez la distance",
            TapeMode::Points => "Double-cliquez, ou cliquez et mesurez, pour créer un point de guide",
            TapeMode::Measure if started => "Cliquez sur un élément à mesurer ou entrez la distance.",
            TapeMode::Measure => "Cliquez sur un élément à partir duquel mesurer.",
        };
        format!(
            "{main} | Ctrl = Alterner entre lignes de guide/points de guide/mesure. | {}",
            super::hints::ARROWS_INFERENCE
        )
    }
    fn vcb_label(&self) -> &'static str {
        "Distance"
    }
    fn vcb_value(&self, units: Units) -> String {
        match (self.start, &self.snap) {
            (Some(_), Some(s)) => self.offset(s.pos).map(|o| format_length(o.length(), units)).unwrap_or_default(),
            _ => self.measured.map(|d| format_length(d, units)).unwrap_or_default(),
        }
    }
    fn busy(&self) -> bool {
        self.start.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.snap = Some(cx.snap(m, self.start.map(|s| s.0), None));
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(s) = self.snap.clone() else { return };
        if self.start.is_some() {
            if let Some(off) = self.offset(s.pos) {
                self.finish(cx, off);
            }
            return;
        }
        let from = match (&s.kind, &s.edge) {
            (SnapKind::OnEdge | SnapKind::OnGuide, Some((_, (a, b)))) if a.distance(*b) > TOL => {
                TapeFrom::Line { dir: (*b - *a).normalize() }
            }
            _ => TapeFrom::Point,
        };
        self.start = Some((s.pos, from));
    }
    fn double_click(&mut self, cx: &mut ToolCx, _m: DVec2) {
        let Some(s) = self.snap.clone() else { return };
        if self.mode == TapeMode::Points {
            // A guide point right there.
            self.start = None;
            let id = make_guide(cx, s.pos, None, None);
            self.last = id.map(|id| LastGuide { id, origin: s.pos, dir: DVec3::X, line: None });
            return;
        }
        // A guide right on the edge.
        if self.mode == TapeMode::Measure || !matches!(s.kind, SnapKind::OnEdge | SnapKind::Midpoint) {
            return;
        }
        if let Some((_, (a, b))) = s.edge {
            self.start = None;
            self.last = None;
            make_guide(cx, a, Some((b - a).normalize()), None);
        }
    }
    fn ctrl_tap(&mut self, _cx: &mut ToolCx) {
        self.mode = match self.mode {
            TapeMode::Lines => TapeMode::Points,
            TapeMode::Points => TapeMode::Measure,
            TapeMode::Measure => TapeMode::Lines,
        };
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let len = cx.parse_len(text)?;
        if self.start.is_some() {
            let off = self.snap.as_ref().and_then(|s| self.offset(s.pos)).unwrap_or_default();
            if off.length() < TOL {
                return Err("Indiquez une direction avec la souris".into());
            }
            self.finish(cx, off.normalize() * len);
            return Ok(());
        }
        // After the fact: moves the guide just made, or resizes the model (the active group or
        // component) so that the distance measured becomes the one typed.
        if let Some(last) = &self.last {
            let (id, p, line) = (last.id, last.origin + last.dir * len, last.line);
            make_guide(cx, p, line, Some(id));
            self.measured = Some(len);
            return Ok(());
        }
        let Some(measured) = self.measured.filter(|d| *d > TOL) else {
            return Err("Mesurez d'abord une distance".into());
        };
        if len <= TOL {
            return Err("La longueur doit être positive".into());
        }
        let k = len / measured;
        let label = if cx.doc.ctx.is_empty() { "Redimensionner le modèle" } else { "Redimensionner" };
        cx.doc.commit(label, |m, d| {
            let e = m.entities_mut(d);
            let mut all: Vec<EntityRef> = e.edges.keys().map(EntityRef::Edge).collect();
            all.extend(e.faces.keys().map(EntityRef::Face));
            all.extend(e.instances.keys().map(EntityRef::Instance));
            all.extend(e.guides.keys().map(EntityRef::Guide));
            all.extend(e.annots.keys().map(EntityRef::Annot));
            e.transform_selection(&all, &esq_core::glam::DMat4::from_scale(DVec3::splat(k)));
        });
        self.measured = Some(len);
        Ok(())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.start = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        let (Some((s, from)), Some(snap)) = (self.start, &self.snap) else { return };
        let Some(off) = self.offset(snap.pos) else { return };
        let end = s + off;
        o.dashed(s, end, Color32::from_gray(30), 2.0);
        if let TapeFrom::Line { dir } = from
            && self.mode == TapeMode::Lines
            && off.length() > TOL
        {
            let k = o.cam.distance * 4.0;
            o.dashed(end - dir * k, end + dir * k, Color32::from_gray(60), 1.0);
        }
        o.text_at(end, &format_length(off.length(), o.units));
    }
}
