//! Show Face Edges (SketchUp's developer menu, `CShowFaceEdgesTool`): a click on a face
//! selects it with all its edges and tells how many loops and edges it has.

use eframe::egui::Color32;
use esq_core::glam::DVec2;
use esq_core::model::{EntityRef, FaceId};
use slotmap::Key;

use super::{Overlay, Tool, ToolCx, ToolId};

#[derive(Default)]
pub struct FaceEdgesTool {
    hover: Option<FaceId>,
    /// "Face %d has %d loops and %d edges" for the last face clicked.
    report: Option<String>,
}

fn face_under(cx: &ToolCx, m: DVec2) -> Option<FaceId> {
    let (o, d) = cx.cam().ray(m);
    cx.infer.pick.ray_face(o, d).and_then(|(_, h, _)| h.face_in(&cx.doc.ctx))
}

impl Tool for FaceEdgesTool {
    fn id(&self) -> ToolId {
        ToolId::ShowFaceEdges
    }
    fn status(&self) -> String {
        self.report.clone().unwrap_or_else(|| "Sélectionnez une face et mettez ses bords en évidence".into())
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.hover = face_under(cx, m);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        // The selection is cleared first, as in SketchUp, even when no face is under the cursor.
        cx.doc.select(Vec::new());
        let Some(f) = face_under(cx, m) else { return };
        let ents = cx.doc.ents();
        let edges = ents.face_edges(f);
        let loops = ents.faces[f].loops().count();
        let id = f.data().as_ffi() & 0xffff_ffff;
        self.report = Some(format!("La face {id} a {loops} boucles et {} arêtes", edges.len()));
        let mut sel = vec![EntityRef::Face(f)];
        sel.extend(edges.into_iter().map(EntityRef::Edge));
        cx.doc.select(sel);
    }
    fn overlay(&self, o: &mut Overlay) {
        if let Some(f) = self.hover {
            o.fill_face(f, Color32::from_rgba_unmultiplied(40, 120, 255, 50));
        }
    }
}
