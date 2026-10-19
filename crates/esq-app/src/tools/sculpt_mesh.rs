//! Stamp and Drape: tools that take something (faces, edges, a group) and work it into a
//! mesh (a terrain), chosen with a second click.

use std::cell::Cell;

use eframe::egui::Color32;
use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::math::line_closest_to_ray;
use esq_core::model::{DefId, EntityRef, FaceId, InstanceId, Units};
use esq_core::sculpt::stamp::{Footprint, StampState, TerrainSel, offset_outline, prepare, shift_building};
use esq_core::sculpt::{SandboxDefaults, collect_edges, collect_faces, drape::drape_edges, mesh_items};
use esq_core::units::format_length;

use super::{Overlay, Tool, ToolCx, ToolId};
use crate::pick::Snap;

/// What a second click designates as the mesh to work on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Terrain {
    /// A face or an edge of the active context: the mesh is what is connected to it.
    Mesh(EntityRef),
    /// A group or a component: the mesh is what it holds.
    Instance(InstanceId),
}

/// The face, edge or group of the active context under the cursor (edges first).
pub(super) fn hovered_item(cx: &ToolCx, s: &Snap) -> Option<EntityRef> {
    let ctx = &cx.doc.ctx;
    s.edge
        .as_ref()
        .and_then(|(h, _)| h.selectable(ctx))
        .or_else(|| s.face.as_ref().and_then(|(h, _)| h.selectable(ctx)))
        .filter(|e| matches!(e, EntityRef::Edge(_) | EntityRef::Face(_) | EntityRef::Instance(_)))
}

/// An edge picked stands for its whole curve.
pub(super) fn with_curve(cx: &ToolCx, item: EntityRef) -> Vec<EntityRef> {
    if let EntityRef::Edge(e) = item {
        let ents = cx.doc.ents();
        if let Some(c) = ents.edges.get(e).and_then(|x| x.curve) {
            return ents.edges.iter().filter(|(_, x)| x.curve == Some(c)).map(|(k, _)| EntityRef::Edge(k)).collect();
        }
    }
    vec![item]
}

impl Terrain {
    /// The mesh designated by a picked item, unless it is (part of) what is being worked in.
    pub(super) fn from_item(cx: &ToolCx, item: EntityRef, source: &[EntityRef]) -> Option<(Terrain, Vec<FaceId>)> {
        let ents = cx.doc.ents();
        match item {
            EntityRef::Instance(i) => {
                if source.contains(&item) {
                    return None;
                }
                let inst = ents.instances.get(i)?;
                let def = cx.doc.model.defs.get(inst.def)?;
                if def.entities.faces.is_empty() {
                    return None;
                }
                Some((Terrain::Instance(i), Vec::new()))
            }
            EntityRef::Face(_) | EntityRef::Edge(_) => {
                let (faces, edges) = mesh_items(ents, Some(item));
                let touches = |e: &EntityRef| match e {
                    EntityRef::Face(f) => faces.contains(f),
                    EntityRef::Edge(x) => edges.contains(x),
                    _ => false,
                };
                if source.iter().any(touches) || faces.is_empty() {
                    return None;
                }
                Some((Terrain::Mesh(item), faces))
            }
            _ => None,
        }
    }

    /// The collection that holds the mesh, its world transform and a seed in it.
    /// An instance is made unique first (inside `m`).
    pub(super) fn open(
        self,
        m: &mut esq_core::model::Model,
        ctx_def: Option<DefId>,
        ctx_xform: &DMat4,
    ) -> (Option<DefId>, DMat4, Option<EntityRef>) {
        match self {
            Terrain::Mesh(seed) => (ctx_def, *ctx_xform, Some(seed)),
            Terrain::Instance(i) => {
                let def = m.make_unique(ctx_def, i);
                let x = *ctx_xform * m.entities(ctx_def).instances[i].xform;
                (Some(def), x, None)
            }
        }
    }
}

/// Faces of the mesh, in world coordinates, for highlighting.
pub(super) fn draw_mesh(o: &Overlay, terrain: Terrain, faces: &[FaceId]) {
    let blue = Color32::from_rgba_unmultiplied(60, 90, 255, 50);
    match terrain {
        Terrain::Mesh(_) => {
            for &f in faces.iter().take(1500) {
                o.fill_face(f, blue);
            }
        }
        Terrain::Instance(i) => {
            let ents = o.doc.ents();
            let Some(inst) = ents.instances.get(i) else { return };
            let Some(def) = o.doc.model.defs.get(inst.def) else { return };
            let x = o.doc.ctx_xform() * inst.xform;
            for e in def.entities.edges.keys().take(3000) {
                let (a, b) = def.entities.edge_points(e);
                o.line(x.transform_point3(a), x.transform_point3(b), Color32::from_rgb(60, 90, 255), 1.5);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// Drape (DrapeTool)

#[derive(Default)]
pub struct DrapeTool {
    /// 0: choosing what to drape; 1: choosing the mesh.
    state: u8,
    hover: Option<EntityRef>,
    /// What the cursor would pick in state 0, as world segments.
    candidate: Vec<(DVec3, DVec3)>,
    /// What is draped, as world segments, and the items of the active context it comes from.
    source: Vec<(DVec3, DVec3)>,
    source_items: Vec<EntityRef>,
    terrain: Option<(Terrain, Vec<FaceId>)>,
}

impl DrapeTool {
    fn gather(cx: &ToolCx, items: &[EntityRef]) -> Vec<(DVec3, DVec3)> {
        collect_edges(&cx.doc.model, cx.doc.ents(), &cx.doc.ctx_xform(), items)
    }

    /// Takes what was selected before the tool was chosen.
    fn adopt_selection(&mut self, cx: &mut ToolCx) {
        let items = std::mem::take(&mut cx.doc.sel);
        cx.doc.select(Vec::new());
        self.source = Self::gather(cx, &items);
        if self.source.is_empty() {
            *cx.message = Some("Aucune arête à projeter dans la sélection.".into());
            return;
        }
        self.source_items = items;
        self.state = 1;
    }

    fn reset(&mut self) {
        *self = DrapeTool::default();
    }

    fn drape(&mut self, cx: &mut ToolCx, terrain: Terrain) {
        let segments = self.source.clone();
        let ctx_def = cx.doc.ctx_def();
        let ctx_xform = cx.doc.ctx_xform();
        let res = cx.doc.try_commit("Projeter", |m, _| {
            let (def, x, seed) = terrain.open(m, ctx_def, &ctx_xform);
            let inv = x.inverse();
            let local: Vec<(DVec3, DVec3)> =
                segments.iter().map(|(a, b)| (inv.transform_point3(*a), inv.transform_point3(*b))).collect();
            let ents = m.entities_mut(def);
            let (faces, _) = mesh_items(ents, seed);
            if drape_edges(ents, &faces, &local) == 0 {
                return Err("Rien à projeter : les arêtes ne passent pas au-dessus de la maille.".to_string());
            }
            Ok(())
        });
        if let Err(e) = res {
            *cx.message = Some(e);
        }
        cx.doc.select(Vec::new());
        self.reset();
    }
}

impl Tool for DrapeTool {
    fn id(&self) -> ToolId {
        ToolId::Drape
    }
    fn status(&self) -> String {
        match self.state {
            0 => "Sélectionnez les entités que vous souhaitez projeter.".into(),
            _ => "Sélectionnez la maille sur laquelle vous souhaitez effectuer la projection.".into(),
        }
    }
    fn busy(&self) -> bool {
        self.state > 0
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.state == 0 && !cx.doc.sel.is_empty() {
            self.adopt_selection(cx);
        }
        let s = cx.snap(m, None, None);
        let item = hovered_item(cx, &s);
        if item == self.hover {
            return;
        }
        self.hover = item;
        if self.state == 0 {
            self.candidate = match item {
                Some(it) => Self::gather(cx, &with_curve(cx, it)),
                None => Vec::new(),
            };
        } else {
            self.terrain = item.and_then(|it| Terrain::from_item(cx, it, &self.source_items));
        }
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        match self.state {
            0 => {
                if let Some(it) = self.hover {
                    let items = with_curve(cx, it);
                    let source = Self::gather(cx, &items);
                    if source.is_empty() {
                        *cx.message = Some("Aucune arête à projeter dans cette entité.".into());
                        return;
                    }
                    self.source = source;
                    self.source_items = items;
                    self.state = 1;
                    self.hover = None;
                    self.candidate.clear();
                }
            }
            _ => {
                if let Some((t, _)) = self.terrain.clone() {
                    self.drape(cx, t);
                }
            }
        }
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        cx.doc.select(Vec::new());
        cx.doc.set_preview(None);
        self.reset();
    }
    fn overlay(&self, o: &mut Overlay) {
        let blue = Color32::from_rgb(0, 80, 255);
        for (a, b) in &self.candidate {
            o.line(*a, *b, blue, 3.0);
        }
        if self.state > 0 {
            for (a, b) in &self.source {
                o.line(*a, *b, Color32::from_rgb(255, 120, 0), 3.0);
            }
            if let Some((t, faces)) = &self.terrain {
                draw_mesh(o, *t, faces);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// Stamp (StampTool)

thread_local! {
    /// Offset typed by the user, kept between two uses of the tool (SandboxDefaults.ini).
    static STAMP_OFFSET: Cell<Option<f64>> = const { Cell::new(None) };
}

fn stamp_offset(units: Units) -> f64 {
    STAMP_OFFSET.with(|r| r.get()).unwrap_or_else(|| SandboxDefaults::for_units(units).stamp_offset)
}

/// The stamp once the mesh is cut: the model with the footprint at its first level, and
/// what moves with the mouse.
struct Stamped {
    base: esq_core::model::Model,
    def: Option<DefId>,
    state: StampState,
    /// Where the mouse picked the mesh; the footprint follows the height of the mouse from here.
    click: DVec3,
    /// Scale of the mesh's frame (heights typed or measured in the world are divided by it).
    scale: f64,
    dz: f64,
}

#[derive(Default)]
pub struct StampTool {
    /// 0: choosing what to stamp; 1: choosing the mesh; 2: choosing the elevation.
    state: u8,
    hover: Option<EntityRef>,
    footprint: Footprint,
    source_items: Vec<EntityRef>,
    terrain: Option<(Terrain, Vec<FaceId>)>,
    stamped: Option<Stamped>,
}

impl StampTool {
    fn reset(&mut self) {
        *self = StampTool::default();
    }

    /// Finds the footprint of some items of the active context.
    fn find_footprint(cx: &ToolCx, items: &[EntityRef]) -> Footprint {
        let faces = collect_faces(&cx.doc.model, cx.doc.ents(), &cx.doc.ctx_xform(), items);
        Footprint::find(&faces)
    }

    fn take(&mut self, cx: &mut ToolCx, items: Vec<EntityRef>) {
        let fp = Self::find_footprint(cx, &items);
        if fp.is_empty() {
            *cx.message = Some("Sélection incorrecte. Il n'y a aucune face non verticale à tamponner.".into());
            return;
        }
        self.footprint = fp;
        self.source_items = items;
        self.state = 1;
        self.hover = None;
    }

    /// Cuts the mesh and fills the gap (in a copy of the model), then waits for the elevation.
    fn stamp_into(&mut self, cx: &mut ToolCx, terrain: Terrain, click: DVec3) {
        let ctx_def = cx.doc.ctx_def();
        let ctx_xform = cx.doc.ctx_xform();
        let mut base = cx.doc.model.clone();
        let (def, x, seed) = terrain.open(&mut base, ctx_def, &ctx_xform);
        let scale = x.transform_vector3(DVec3::X).length().max(1e-12);
        let local = self.footprint.transformed(&x.inverse());
        let sel = match seed {
            Some(s) => TerrainSel::Connected(s),
            None => TerrainSel::All,
        };
        let offset = stamp_offset(cx.doc.model.units) / scale;
        match prepare(base.entities_mut(def), sel, &local, offset) {
            Ok(state) => {
                self.stamped = Some(Stamped { base, def, state, click, scale, dz: 0.0 });
                self.state = 2;
                self.terrain = None;
                self.hover = None;
                self.show(cx);
            }
            Err(e) => *cx.message = Some(e),
        }
    }

    /// Shows the stamp at the current elevation.
    fn show(&self, cx: &mut ToolCx) {
        let Some(s) = &self.stamped else { return };
        let mut m = s.base.clone();
        shift_building(m.entities_mut(s.def), &s.state, s.dz / s.scale);
        cx.doc.set_preview(Some(m));
    }

    fn finish(&mut self, cx: &mut ToolCx) {
        let Some(s) = self.stamped.take() else { return };
        let mut m = s.base;
        shift_building(m.entities_mut(s.def), &s.state, s.dz / s.scale);
        cx.doc.commit("Tamponner", |model, _| *model = m);
        cx.doc.select(Vec::new());
        self.reset();
    }
}

impl Tool for StampTool {
    fn id(&self) -> ToolId {
        ToolId::Stamp
    }
    fn status(&self) -> String {
        match self.state {
            0 => "Sélectionnez la ou les faces à tamponner sur la maille ou tapez un nouveau décalage pour le tampon."
                .into(),
            1 => "Sélectionnez la maille sur laquelle appliquer le tampon ou tapez un nouveau décalage pour le tampon."
                .into(),
            _ => "Sélectionnez l'élévation des faces tamponnées.".into(),
        }
    }
    fn vcb_label(&self) -> &'static str {
        if self.state < 2 { "Décalage" } else { "" }
    }
    fn vcb_value(&self, units: Units) -> String {
        if self.state < 2 { format_length(stamp_offset(units), units) } else { String::new() }
    }
    fn busy(&self) -> bool {
        self.state > 0
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        if self.state == 0 && !cx.doc.sel.is_empty() {
            let items = std::mem::take(&mut cx.doc.sel);
            cx.doc.select(Vec::new());
            self.take(cx, items);
        }
        if self.state == 2 {
            let Some(s) = &mut self.stamped else { return };
            let (ro, rd) = cx.cam().ray(m);
            if let Some(t) = line_closest_to_ray(s.click, DVec3::Z, ro, rd) {
                s.dz = t;
                self.show(cx);
            }
            return;
        }
        let s = cx.snap(m, None, None);
        let item = hovered_item(cx, &s);
        if item == self.hover {
            return;
        }
        self.hover = item;
        if self.state == 0 {
            self.hover = item.filter(|e| matches!(e, EntityRef::Face(_) | EntityRef::Instance(_)));
        } else {
            self.terrain = item.and_then(|it| Terrain::from_item(cx, it, &self.source_items));
        }
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        match self.state {
            0 => {
                if let Some(it) = self.hover {
                    self.take(cx, vec![it]);
                }
            }
            1 => {
                let click = self.snap_pos(cx, m);
                if let Some((t, _)) = self.terrain.clone() {
                    self.stamp_into(cx, t, click);
                }
            }
            _ => self.finish(cx),
        }
    }
    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        if self.state >= 2 {
            return Err("Cliquez pour fixer l'élévation.".into());
        }
        let v = cx.parse_len(text)?;
        // The offset cannot be nought: one inch then.
        let v = if v <= 0.0 { 25.4 } else { v };
        STAMP_OFFSET.with(|c| c.set(Some(v)));
        Ok(())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        cx.doc.select(Vec::new());
        cx.doc.set_preview(None);
        self.reset();
    }
    fn overlay(&self, o: &mut Overlay) {
        match self.state {
            0 => match self.hover {
                Some(EntityRef::Face(f)) => o.fill_face(f, Color32::from_rgba_unmultiplied(60, 90, 255, 70)),
                Some(EntityRef::Instance(i)) => draw_mesh(o, Terrain::Instance(i), &[]),
                _ => {}
            },
            1 => {
                // The outline of the cut, around each part of the footprint.
                let offset = stamp_offset(o.units);
                for (flat, n) in self.footprint.flat_profile.iter().zip(&self.footprint.normals) {
                    let z = flat[0].z;
                    let ring: Vec<DVec3> =
                        offset_outline(flat, *n, offset).iter().map(|p| DVec3::new(p.x, p.y, z)).collect();
                    o.polyline(&ring, true, Color32::RED, 2.0);
                }
                if let Some((t, faces)) = &self.terrain {
                    draw_mesh(o, *t, faces);
                }
            }
            _ => {}
        }
    }
}

impl StampTool {
    /// Where the cursor is on the mesh (world), for the vertical line of the elevation.
    fn snap_pos(&self, cx: &ToolCx, m: DVec2) -> DVec3 {
        cx.snap(m, None, None).pos
    }
}
