//! Follow Me: select a path (edges, or a face for its outline), then click the profile; or
//! click the profile first, then an edge of the path (or a face to follow its outline).

use std::collections::HashSet;

use eframe::egui::Color32;
use esq_core::glam::DVec2;
use esq_core::model::{EdgeId, EntityRef, FaceId};

use super::{Overlay, Tool, ToolCx, ToolId};
use crate::pick::EDGE_PX;

#[derive(Default)]
pub struct FollowMeTool {
    profile: Option<FaceId>,
    hover: Option<EntityRef>,
    /// Alt (CExtrudeTool "Use Face Perimeter"): once the profile is chosen, a face under the
    /// cursor gives its outline as the path; otherwise the path is a chain of edges (or, as a
    /// fallback, the outline of a face clicked).
    perimeter: bool,
    /// A path was selected before the tool.
    preselected: bool,
}

/// Edges of the path chosen beforehand: the selected edges, or the outline of the selected
/// face.
fn preselected_path(cx: &ToolCx, profile: FaceId) -> Vec<EdgeId> {
    let ents = cx.doc.ents();
    let mut edges: Vec<EdgeId> =
        cx.doc.sel.iter().filter_map(|e| if let EntityRef::Edge(x) = e { Some(*x) } else { None }).collect();
    if edges.is_empty()
        && let Some(f) = cx
            .doc
            .sel
            .iter()
            .find_map(|e| if let EntityRef::Face(f) = e { Some(*f) } else { None })
            .filter(|f| *f != profile)
    {
        edges = outline(cx, f);
    }
    let own: HashSet<EdgeId> = ents.face_edges(profile).into_iter().collect();
    edges.retain(|e| !own.contains(e));
    edges
}

fn outline(cx: &ToolCx, f: FaceId) -> Vec<EdgeId> {
    let ents = cx.doc.ents();
    let face = &ents.faces[f];
    (0..face.outer.len())
        .filter_map(|i| ents.find_edge(face.outer[i], face.outer[(i + 1) % face.outer.len()]))
        .collect()
}

impl FollowMeTool {
    /// The face under the cursor, or once the profile is chosen, the edge of the path.
    fn under(cx: &ToolCx, m: DVec2, want_edge: bool) -> Option<EntityRef> {
        let ctx = &cx.doc.ctx;
        if want_edge
            && let Some((_, _, hit, _)) = cx.infer.pick.screen_edge(cx.cam(), m, EDGE_PX)
            && let Some(e @ EntityRef::Edge(_)) = hit.selectable(ctx)
        {
            return Some(e);
        }
        let (o, d) = cx.cam().ray(m);
        let (_, hit, _) = cx.infer.pick.ray_face(o, d)?;
        hit.face_in(ctx).map(EntityRef::Face)
    }

    fn run(&mut self, cx: &mut ToolCx, profile: FaceId, path: Vec<EdgeId>) {
        self.profile = None;
        let path = match cx.doc.ents().edge_path(&path) {
            Ok(p) => p,
            Err(e) => {
                *cx.message = Some(e.to_string());
                return;
            }
        };
        match cx.doc.try_commit("Suivez-moi", |m, d| m.entities_mut(d).follow_me(profile, &path).map(|_| ())) {
            Ok(()) => cx.doc.select(Vec::new()),
            Err(e) => *cx.message = Some(e.to_string()),
        }
    }
}

impl Tool for FollowMeTool {
    fn id(&self) -> ToolId {
        ToolId::FollowMe
    }
    fn status(&self) -> String {
        match self.profile {
            None if self.preselected => {
                "Cliquez sur un profil à extruder le long du chemin d'accès présélectionné.".into()
            }
            None => "Cliquez sur le profil que vous souhaitez extruder.".into(),
            Some(_) => {
                let main = if self.perimeter {
                    "Passez le curseur sur une face pour utiliser son périmètre comme chemin d'extrusion."
                } else {
                    "Déplacez le curseur le long du chemin d'extrusion souhaité."
                };
                format!("{main} | Alt = Activez/désactivez Utiliser le périmètre de la zone.")
            }
        }
    }
    fn alt_tap(&mut self, _cx: &mut ToolCx) {
        self.perimeter = !self.perimeter;
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.preselected = cx.doc.sel.iter().any(|e| matches!(e, EntityRef::Edge(_) | EntityRef::Face(_)));
        let want_edge = self.profile.is_some() && !self.perimeter;
        self.hover = Self::under(cx, m, want_edge);
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(target) = self.hover else { return };
        match (self.profile, target) {
            (None, EntityRef::Face(f)) => {
                let path = preselected_path(cx, f);
                if path.is_empty() {
                    self.profile = Some(f);
                } else {
                    self.run(cx, f, path);
                }
            }
            (Some(p), EntityRef::Edge(e)) => {
                let own: HashSet<EdgeId> = cx.doc.ents().face_edges(p).into_iter().collect();
                let path = cx.doc.ents().chain_through(e, &own);
                self.run(cx, p, path);
            }
            (Some(p), EntityRef::Face(f)) if f != p => {
                let path = outline(cx, f);
                self.run(cx, p, path);
            }
            _ => {}
        }
    }
    fn busy(&self) -> bool {
        self.profile.is_some()
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.profile = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        let ents = o.doc.ents();
        let x = o.doc.ctx_xform();
        if let Some(p) = self.profile.filter(|p| ents.faces.contains_key(*p)) {
            o.fill_face(p, Color32::from_rgba_unmultiplied(255, 160, 0, 90));
        }
        match self.hover {
            Some(EntityRef::Face(f)) if ents.faces.contains_key(f) && Some(f) != self.profile => {
                o.fill_face(f, Color32::from_rgba_unmultiplied(80, 120, 255, 50));
            }
            Some(EntityRef::Edge(e)) if self.profile.is_some() && ents.edges.contains_key(e) => {
                let own: HashSet<EdgeId> =
                    self.profile.map(|p| ents.face_edges(p).into_iter().collect()).unwrap_or_default();
                for c in ents.chain_through(e, &own) {
                    let (a, b) = ents.edge_points(c);
                    o.line(x.transform_point3(a), x.transform_point3(b), Color32::from_rgb(255, 120, 0), 3.0);
                }
            }
            _ => {}
        }
    }
}
