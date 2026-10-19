//! Photo Point Tool (Window › Match Photo, `CAddRayTool`): in a matched-photo scene, a click
//! on the image draws a guide from the camera through that point (a ray: closed at the eye,
//! open beyond). Seen from another matched photo, the guide shows where the point lies in
//! 3D. The tool gives way after one guide, and when the view leaves the photo.

use esq_core::glam::DVec2;
use esq_core::model::{AttrValue, Guide};
use slotmap::Key;

use super::{Tool, ToolCx, ToolId};

#[derive(Default)]
pub struct PhotoPointTool {
    done: bool,
}

impl Tool for PhotoPointTool {
    fn id(&self) -> ToolId {
        ToolId::PhotoPoint
    }
    fn status(&self) -> String {
        "Sélectionnez un point sur l'image pour tracer le guide. Allez sur une autre scène photo adaptée pour utiliser le guide."
            .into()
    }
    fn mouse_move(&mut self, _cx: &mut ToolCx, _m: DVec2) {}
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        let (o, d) = cx.cam().ray(m);
        let eye = cx.to_local(o);
        let dir = (cx.to_local(o + d) - eye).normalize();
        cx.doc.commit("Guide point photo", |model, def| {
            let ents = model.entities_mut(def);
            let g = ents.guides.insert(Guide::line(eye, dir));
            // A ray, as the Ruby API keeps it: an infinite line closed at its start.
            ents.set_prop(&format!("g{}", g.data().as_ffi()), "ray", AttrValue::Text("start".into()));
        });
        *cx.message = Some(
            "Vous pouvez utiliser la ligne de guide que vous venez de créer dans une autre photo adaptée pour localiser le point en 3D."
                .into(),
        );
        self.done = true;
    }
    fn done(&self) -> bool {
        self.done
    }
}
