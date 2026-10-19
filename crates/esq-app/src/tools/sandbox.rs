//! Sandbox tools (terrain meshes): From Scratch, From Contours, Add Detail, Flip Edge.
//! The geometry is in `esq_core::sandbox`.

use std::collections::HashSet;
use std::sync::Mutex;

use eframe::egui::Color32;
use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::math::{TOL, line_closest_to_ray};
use esq_core::model::{EdgeId, EntityRef, FaceId, Units, VertexId};
use esq_core::sandbox::{
    DEFAULT_GRID_SPACING, DetailTarget, MSG_DETAIL_INVALID, grid_count, grid_entities, grid_frame,
};
use esq_core::units::format_length;

use super::{Overlay, Tool, ToolCx, ToolId};
use crate::doc::Document;
use crate::pick::{Snap, SnapKind};

// ---------------------------------------------------------------------------------------
// Remembered settings (SketchUp keeps them in sandboxdefaults.ini)

static GRID_SPACING: Mutex<Option<f64>> = Mutex::new(None);

fn defaults_file() -> Option<std::path::PathBuf> {
    if cfg!(test) {
        return None;
    }
    Some(crate::settings::user_dir()?.join("sandbox.ini"))
}

/// Grid spacing used by From Scratch, in millimetres.
pub fn grid_spacing() -> f64 {
    let mut g = GRID_SPACING.lock().unwrap_or_else(|e| e.into_inner());
    *g.get_or_insert_with(|| {
        defaults_file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| {
                s.lines().find_map(|l| l.strip_prefix("GridSpacing=")).and_then(|v| v.trim().parse::<f64>().ok())
            })
            .filter(|v| *v > 0.0)
            .unwrap_or(DEFAULT_GRID_SPACING)
    })
}

fn set_grid_spacing(v: f64) {
    *GRID_SPACING.lock().unwrap_or_else(|e| e.into_inner()) = Some(v);
    if let Some(p) = defaults_file() {
        let _ = std::fs::write(p, format!("GridSpacing={v}\n"));
    }
}

// ---------------------------------------------------------------------------------------
// Flip Edge

const FLIP_LABEL: &str = "Retourner l'arête";

/// The edge of the active context under the cursor.
fn edge_under(cx: &ToolCx, snap: &Snap) -> Option<EdgeId> {
    match snap.edge.as_ref()?.0.selectable(&cx.doc.ctx)? {
        EntityRef::Edge(e) if cx.doc.ents().edges.contains_key(e) => Some(e),
        _ => None,
    }
}

/// Click an edge between two triangles to swap the diagonal of their quad.
#[derive(Default)]
pub struct FlipEdgeTool {
    snap: Option<Snap>,
    /// The flippable edge under the cursor, which is also the selection.
    edge: Option<EdgeId>,
}

impl FlipEdgeTool {
    fn update(&mut self, cx: &mut ToolCx, m: DVec2) {
        let snap = cx.snap(m, None, None);
        self.edge = edge_under(cx, &snap).filter(|&e| cx.doc.ents().can_flip_edge(e).is_ok());
        let want: Vec<EntityRef> = self.edge.map(EntityRef::Edge).into_iter().collect();
        if cx.doc.sel != want {
            cx.doc.select(want);
        }
        self.snap = Some(snap);
    }
}

impl Tool for FlipEdgeTool {
    fn id(&self) -> ToolId {
        ToolId::FlipEdge
    }
    fn status(&self) -> String {
        "Sélectionnez une arête à retourner sur la maille.".into()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.update(cx, m);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.update(cx, m);
        let Some(e) = self.edge.take() else { return };
        let mut result = Ok(());
        cx.doc.commit(FLIP_LABEL, |model, d| result = model.entities_mut(d).flip_edge(e));
        if let Err(err) = result {
            *cx.message = Some(err.message().into());
        }
        cx.doc.select(Vec::new());
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.edge = None;
        if !cx.doc.sel.is_empty() {
            cx.doc.select(Vec::new());
        }
    }
}

// ---------------------------------------------------------------------------------------
// Add Detail

const DETAIL_LABEL: &str = "Ajouter des détails";
const OFFSET_LABEL: &str = "Décalage de détails";

/// The new vertex while it is being offset from the mesh.
struct Offset {
    vert: VertexId,
    /// Where the vertex was, and where it is now (context coordinates).
    org: DVec3,
    cur: DVec3,
    /// Unit normal of the faces concerned (offset direction with Shift).
    normal: DVec3,
    shift: bool,
}

impl Offset {
    /// Direction of the offset in context coordinates: vertical, or along the normal.
    fn dir(&self) -> DVec3 {
        if self.shift { self.normal } else { DVec3::Z }
    }
}

/// Click a face, an edge or a vertex of a triangulated mesh to add a vertex there, then
/// offset it up (or along the normal with Shift). With triangles selected beforehand, the
/// tool cuts each of them into four at once.
#[derive(Default)]
pub struct AddDetailTool {
    snap: Option<Snap>,
    hover: Option<DetailTarget>,
    off: Option<Offset>,
}

impl AddDetailTool {
    /// What the cursor designates in the active context: a vertex, an edge or a face.
    fn target_at(cx: &ToolCx, snap: &Snap) -> Option<DetailTarget> {
        let ents = cx.doc.ents();
        match snap.kind {
            SnapKind::Endpoint | SnapKind::Origin => ents.find_vertex(cx.to_local(snap.pos)).map(DetailTarget::Vertex),
            SnapKind::Midpoint | SnapKind::OnEdge => edge_under(cx, snap).map(DetailTarget::Edge),
            SnapKind::OnFace => snap.face.as_ref()?.0.face_in(&cx.doc.ctx).map(DetailTarget::Face),
            _ => None,
        }
    }

    /// Moves the new vertex to `cur` on screen only.
    fn preview(&mut self, cx: &mut ToolCx) {
        let Some(off) = &self.off else { return };
        let mut pm = cx.doc.model.clone();
        let v: HashSet<VertexId> = [off.vert].into();
        pm.entities_mut(cx.doc.ctx_def()).transform_vertices(&v, &DMat4::from_translation(off.cur - off.org));
        cx.doc.set_preview(Some(pm));
    }

    fn commit_offset(&mut self, cx: &mut ToolCx) {
        let Some(off) = self.off.take() else { return };
        let delta = off.cur - off.org;
        if delta.length() > TOL {
            let v: HashSet<VertexId> = [off.vert].into();
            cx.doc
                .commit(OFFSET_LABEL, |m, d| m.entities_mut(d).transform_vertices(&v, &DMat4::from_translation(delta)));
        } else {
            cx.doc.set_preview(None);
        }
    }
}

impl Tool for AddDetailTool {
    fn id(&self) -> ToolId {
        ToolId::AddDetail
    }
    fn status(&self) -> String {
        if self.off.is_some() {
            "Décalez le nouveau sommet dans la maille. Cliquez pour définir le décalage ou tapez une distance de décalage. Maintenez la touche Maj enfoncée pour un décalage perpendiculaire à l’entité sélectionnée."
                .into()
        } else {
            "Sélectionnez une face, une arête ou un sommet à détailler sur la maille. Maintenez la touche CTRL enfoncée pour ajouter des détails sans décaler le nouveau sommet."
                .into()
        }
    }
    fn vcb_label(&self) -> &'static str {
        if self.off.is_some() { "Décalage" } else { "" }
    }
    fn vcb_value(&self, units: Units) -> String {
        match &self.off {
            Some(o) => format_length(o.cur.distance(o.org), units),
            None => String::new(),
        }
    }
    fn busy(&self) -> bool {
        self.off.is_some()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref().filter(|_| self.hover.is_some() || self.off.is_some())
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let snap = cx.snap(m, None, None);
        if let Some(off) = &mut self.off {
            off.shift = cx.mods.shift;
            let x = cx.doc.ctx_xform();
            let org = x.transform_point3(off.org);
            let dir = x.transform_vector3(off.dir()).normalize_or(DVec3::Z);
            // On a vertex of the mesh the offset follows it; elsewhere the line is
            // projected to the cursor ray.
            let at = if snap.kind == SnapKind::Endpoint {
                org + dir * (snap.pos - org).dot(dir)
            } else {
                let (ro, rd) = cx.cam().ray(m);
                line_closest_to_ray(org, dir, ro, rd).map_or(org, |t| org + dir * t)
            };
            off.cur = x.inverse().transform_point3(at);
            self.snap = Some(snap);
            self.preview(cx);
        } else {
            self.hover = Self::target_at(cx, &snap);
            self.snap = Some(snap);
        }
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        if self.off.is_some() {
            self.commit_offset(cx);
            return;
        }
        let (Some(target), Some(snap)) = (self.hover, self.snap.as_ref()) else { return };
        if !cx.doc.ents().detail_possible(target) {
            *cx.message = Some(MSG_DETAIL_INVALID.into());
            return;
        }
        let normal = cx.doc.ents().detail_normal(target);
        let pos = cx.to_local(snap.pos);
        let mut vert = None;
        let r = cx.doc.try_commit(DETAIL_LABEL, |m, d| {
            vert = Some(m.entities_mut(d).add_detail_vertex(target, pos)?);
            Ok::<(), String>(())
        });
        if let Err(e) = r {
            *cx.message = Some(e);
            return;
        }
        self.hover = None;
        // With Ctrl the vertex stays where it was put.
        if cx.mods.ctrl || cx.mods.command {
            return;
        }
        if let Some(v) = vert {
            let org = cx.doc.ents().pos(v);
            self.off = Some(Offset { vert: v, org, cur: org, normal, shift: cx.mods.shift });
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let value = cx.parse_len(text.trim())?;
        let Some(off) = &mut self.off else { return Err("Cliquez d'abord sur la maille".into()) };
        off.cur = off.org + off.dir() * value;
        self.commit_offset(cx);
        Ok(())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.off = None;
        self.hover = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        let x = o.doc.ctx_xform();
        if let Some(off) = &self.off {
            let col = if off.shift { Color32::from_rgb(220, 0, 220) } else { Color32::from_rgb(0, 0, 230) };
            o.dashed(x.transform_point3(off.org), x.transform_point3(off.cur), col, 1.5);
            return;
        }
        let Some(t) = self.hover else { return };
        let hi = Color32::from_rgba_unmultiplied(255, 140, 0, 110);
        match t {
            DetailTarget::Face(f) => o.fill_face(f, hi),
            DetailTarget::Edge(e) if o.doc.ents().edges.contains_key(e) => {
                let (a, b) = o.doc.ents().edge_points(e);
                o.line(x.transform_point3(a), x.transform_point3(b), Color32::from_rgb(255, 140, 0), 3.0);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------
// Commands that act on the selection at once

/// Tools that work on what is already selected do their job when chosen in the menu, and
/// the Select tool comes back. Returns `None` when the command is an ordinary tool.
pub fn on_command(tool: ToolId, doc: &mut Document) -> Option<Result<(), String>> {
    match tool {
        ToolId::FromContours => {
            let edges: Vec<EdgeId> =
                doc.sel.iter().filter_map(|e| if let EntityRef::Edge(x) = e { Some(*x) } else { None }).collect();
            let r = doc.try_commit(CONTOURS_LABEL, |m, d| m.terrain_from_contours(d, &edges).map(|_| ()));
            if r.is_ok() {
                doc.select(Vec::new());
            }
            Some(r)
        }
        ToolId::AddDetail if !doc.sel.is_empty() => {
            // Triangles selected beforehand are subdivided at once.
            let faces: Vec<FaceId> =
                doc.sel.iter().filter_map(|e| if let EntityRef::Face(x) = e { Some(*x) } else { None }).collect();
            let r = doc.try_commit(DETAIL_LABEL, |m, d| match m.entities_mut(d).subdivide_triangles(&faces) {
                0 => Err(MSG_DETAIL_INVALID.to_string()),
                _ => Ok(()),
            });
            doc.select(Vec::new());
            Some(r)
        }
        _ => None,
    }
}

const CONTOURS_LABEL: &str = "Bac à sable à partir des contours";

/// Placeholder tool for the commands that act at once (`on_command`).
pub struct ActionTool(pub ToolId);

impl Tool for ActionTool {
    fn id(&self) -> ToolId {
        self.0
    }
    fn status(&self) -> String {
        match self.0 {
            ToolId::FromContours => "Créez un terrain à partir d'un ensemble d'arêtes sélectionnées.".into(),
            _ => String::new(),
        }
    }
    fn mouse_move(&mut self, _cx: &mut ToolCx, _m: DVec2) {}
}

// ---------------------------------------------------------------------------------------
// From Scratch

const SCRATCH_LABEL: &str = "Bac à sable à partir de zéro";

/// Draws a grid with three clicks: first corner, second corner (the width), then the depth.
pub struct FromScratchTool {
    /// 0: first corner, 1: second corner, 2: depth.
    state: u8,
    /// Corners in world coordinates: [0] first, [1] second, [2] and [3] the far side.
    pts: [DVec3; 4],
    snap: Option<Snap>,
    moved: bool,
    spacing: f64,
    /// Squares along the width of the grid just made, which a typed depth can redo.
    last_nx: Option<usize>,
}

impl Default for FromScratchTool {
    fn default() -> Self {
        FromScratchTool {
            state: 0,
            pts: [DVec3::ZERO; 4],
            snap: None,
            moved: false,
            spacing: grid_spacing(),
            last_nx: None,
        }
    }
}

impl FromScratchTool {
    fn reset(&mut self) {
        self.state = 0;
        self.moved = false;
        self.spacing = grid_spacing();
    }

    fn set_point(&mut self, cx: &mut ToolCx, m: DVec2) {
        let reference = match self.state {
            1 => Some(self.pts[0]),
            2 => Some(self.pts[1]),
            _ => None,
        };
        let s = cx.snap(m, reference, None);
        let pos = s.pos;
        self.snap = Some(s);
        match self.state {
            0 => {
                self.pts[0] = pos;
                self.pts[3] = pos;
            }
            1 => self.pts[1] = pos,
            _ => {
                let line = self.pts[1] - self.pts[0];
                let t = (pos - self.pts[0]).dot(line) / line.length_squared().max(1e-12);
                let vec = pos - (self.pts[0] + line * t);
                self.pts[2] = self.pts[1] + vec;
                self.pts[3] = self.pts[0] + vec;
            }
        }
    }

    /// Makes the grid from the current corners. Does nothing when a side is shorter than a
    /// square.
    fn create(&mut self, cx: &mut ToolCx) -> bool {
        let nx = grid_count(self.pts[1].distance(self.pts[0]), self.spacing);
        let ny = grid_count(self.pts[3].distance(self.pts[0]), self.spacing);
        if nx == 0 || ny == 0 {
            return false;
        }
        let origin = cx.to_local(self.pts[0]);
        let xdir = cx.to_local(self.pts[1]) - origin;
        let ydir = cx.to_local(self.pts[3]) - origin;
        let Some(frame) = grid_frame(origin, xdir, ydir) else { return false };
        let spacing = self.spacing;
        cx.doc.commit(SCRATCH_LABEL, |m, d| {
            m.add_group_with(d, grid_entities(nx, ny, spacing), frame);
        });
        self.last_nx = Some(nx);
        true
    }

    fn advance(&mut self, cx: &mut ToolCx) {
        match self.state {
            0 => {
                self.state = 1;
                self.moved = false;
            }
            1 => {
                self.state = 2;
                self.moved = false;
                self.pts[2] = self.pts[1];
                self.pts[3] = self.pts[0];
            }
            _ => {
                self.create(cx);
                self.reset();
            }
        }
    }

    fn length_of(&self, a: usize, b: usize) -> f64 {
        self.pts[a].distance(self.pts[b])
    }

    /// The grid being dragged out, as lines in world coordinates (the squares' sides), and
    /// the tick marks when it has no depth yet.
    fn grid_lines(&self) -> Vec<(DVec3, DVec3)> {
        let mut out = Vec::new();
        let xvec = self.pts[1] - self.pts[0];
        let nx = grid_count(xvec.length(), self.spacing);
        if nx == 0 || self.state < 2 {
            return out;
        }
        let yvec = self.pts[3] - self.pts[0];
        let ny = grid_count(yvec.length(), self.spacing);
        if ny == 0 {
            return out;
        }
        let (xs, ys) = (xvec.normalize() * self.spacing, yvec.normalize() * self.spacing);
        let (w, h) = (xs * nx as f64, ys * ny as f64);
        for i in 0..=nx {
            let a = self.pts[0] + xs * i as f64;
            out.push((a, a + h));
        }
        for j in 0..=ny {
            let a = self.pts[0] + ys * j as f64;
            out.push((a, a + w));
        }
        out
    }
}

impl Tool for FromScratchTool {
    fn id(&self) -> ToolId {
        ToolId::FromScratch
    }
    fn status(&self) -> String {
        match self.state {
            0 => "Sélectionnez le premier angle du bac à sable ou saisissez la valeur d’espacement de grille du bac à sable."
                .into(),
            1 => "Sélectionnez le deuxième angle du bac à sable. Maj. permet de verrouiller le sens de la ligne. Saisissez la longueur de la ligne si vous le souhaitez."
                .into(),
            _ => "Sélectionnez le troisième angle du bac à sable. Saisissez la longueur de la ligne si vous le souhaitez."
                .into(),
        }
    }
    fn vcb_label(&self) -> &'static str {
        if self.state == 0 { "Espacement de grille" } else { "Longueur" }
    }
    fn vcb_value(&self, units: Units) -> String {
        match self.state {
            0 => format_length(self.spacing, units),
            1 => format_length(self.length_of(1, 0), units),
            _ => format_length(self.length_of(2, 1), units),
        }
    }
    fn busy(&self) -> bool {
        self.state > 0
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.moved = true;
        self.set_point(cx, m);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.set_point(cx, m);
        // The user must have gone at least one square away in each direction.
        let ok = match self.state {
            0 => true,
            1 => self.length_of(1, 0) > self.spacing,
            _ => self.length_of(2, 1) > self.spacing,
        };
        if ok {
            self.advance(cx);
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let value = cx.parse_len(text.trim())?.abs();
        match self.state {
            0 => {
                if !self.moved
                    && let Some(nx) = self.last_nx
                    && cx.doc.undo_label() == Some(SCRATCH_LABEL)
                {
                    // Right after a grid: retype its depth.
                    let dir = (self.pts[2] - self.pts[1]).normalize_or_zero();
                    if dir == DVec3::ZERO || grid_count(value, self.spacing) == 0 || nx == 0 {
                        return Err("La profondeur doit couvrir au moins un carreau".into());
                    }
                    cx.doc.undo();
                    self.pts[2] = self.pts[1] + dir * value;
                    self.pts[3] = self.pts[0] + dir * value;
                    self.create(cx);
                } else if value > 0.0 {
                    self.spacing = value;
                    set_grid_spacing(value);
                }
            }
            1 => {
                let dir = (self.pts[1] - self.pts[0]).normalize_or_zero();
                if dir == DVec3::ZERO {
                    return Err("Indiquez une direction avec la souris".into());
                }
                self.pts[1] = self.pts[0] + dir * value;
                self.advance(cx);
            }
            _ => {
                if !self.moved {
                    let dir = (self.pts[1] - self.pts[0]).normalize_or_zero();
                    self.pts[1] = self.pts[0] + dir * value;
                } else {
                    let dir = (self.pts[2] - self.pts[1]).normalize_or_zero();
                    if dir == DVec3::ZERO {
                        return Err("Indiquez une direction avec la souris".into());
                    }
                    self.pts[2] = self.pts[1] + dir * value;
                    self.pts[3] = self.pts[0] + dir * value;
                    self.advance(cx);
                }
            }
        }
        Ok(())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.reset();
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        match self.state {
            1 => {
                o.dashed(self.pts[0], self.pts[1], Color32::BLACK, 1.5);
                // Tick marks at every square, 6 pixels long.
                let n = grid_count(self.length_of(1, 0), self.spacing);
                let dir = (self.pts[1] - self.pts[0]).normalize_or_zero();
                if n > 0 && dir != DVec3::ZERO {
                    for i in 0..=n {
                        let a = self.pts[0] + dir * (self.spacing * i as f64);
                        if let (Some(s), Some(t)) = (o.screen(a), o.screen(self.pts[1])) {
                            let along = (t - o.screen(self.pts[0]).unwrap_or(t)).normalized();
                            let perp = eframe::egui::vec2(-along.y, along.x) * 6.0;
                            o.painter.line_segment([s, s + perp], eframe::egui::Stroke::new(1.0, Color32::BLACK));
                        }
                    }
                }
            }
            2 => {
                let lines = self.grid_lines();
                if lines.is_empty() {
                    o.dashed(self.pts[0], self.pts[1], Color32::BLACK, 1.5);
                    o.dashed(self.pts[0], self.pts[3], Color32::BLACK, 1.5);
                }
                for (a, b) in lines {
                    o.dashed(a, b, Color32::BLACK, 1.0);
                }
            }
            _ => {}
        }
    }
}
