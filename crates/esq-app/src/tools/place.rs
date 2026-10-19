//! Placing a component chosen in Window › Components: it follows the cursor (inferences
//! included) until a click drops it.

use esq_core::glam::{DMat4, DVec2, DVec3, DVec4};
use esq_core::model::{DefId, EntityRef, Instance};

use super::{Tool, ToolCx, ToolId};
use crate::pick::{Snap, SnapKind};

pub struct PlaceTool {
    def: DefId,
    snap: Option<Snap>,
    /// Over a face, the component lies on it (its Z axis along the face's normal, its X axis
    /// horizontal), as SketchUp does with 3D text.
    glued: bool,
    /// CComponentTool's repeat flag (`Model#place_component(definition, true)`): every click
    /// places one more copy. Without it (Window › Components, File › Import), one click
    /// places the component, selects it and hands over to the Move tool.
    repeat: bool,
    placed: bool,
}

/// Rotation that puts a component's Z axis along `n`, keeping its X axis horizontal and its
/// Y axis pointing up on a wall.
fn lying_on(n: DVec3) -> DMat4 {
    let n = n.normalize_or(DVec3::Z);
    let (x, y) = if n.dot(DVec3::Z).abs() > 0.9999 {
        (DVec3::X, n.cross(DVec3::X))
    } else {
        let x = DVec3::Z.cross(n).normalize();
        (x, n.cross(x))
    };
    DMat4::from_cols(x.extend(0.0), y.extend(0.0), n.extend(0.0), DVec4::W)
}

impl PlaceTool {
    pub fn new(def: DefId) -> Self {
        PlaceTool { def, snap: None, glued: false, repeat: false, placed: false }
    }

    /// Like [`PlaceTool::new`], but the component lies on the face under the cursor.
    pub fn glued(def: DefId) -> Self {
        PlaceTool { glued: true, ..Self::new(def) }
    }

    /// Like [`PlaceTool::new`], placing copies until another tool is chosen.
    pub fn repeating(def: DefId) -> Self {
        PlaceTool { repeat: true, ..Self::new(def) }
    }

    fn instance(&self, cx: &ToolCx) -> Option<Instance> {
        let s = self.snap.as_ref()?;
        // Components › Edit › Glue to: a gluing component lies on the faces of its slope and
        // stays glued to the face (`Group#glued_to`).
        let behavior = cx.doc.model.defs.get(self.def).map(|d| d.ext.behavior).unwrap_or_default();
        let mut ext = esq_core::model::InstanceExt::default();
        let rotation = match &s.face {
            Some((hit, n))
                if matches!(s.kind, SnapKind::OnFace)
                    && (self.glued || crate::dialogs::comp_edit::glue_allows(&behavior, *n)) =>
            {
                if behavior.is2d {
                    ext.glued_to = crate::dialogs::comp_edit::glue_target(&hit.path, &cx.doc.ctx, hit.item);
                }
                lying_on(cx.doc.ctx_xform().inverse().transform_vector3(*n))
            }
            _ => DMat4::IDENTITY,
        };
        Some(Instance {
            def: self.def,
            xform: DMat4::from_translation(cx.to_local(s.pos)) * rotation,
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext,
        })
    }
}

impl Tool for PlaceTool {
    fn id(&self) -> ToolId {
        ToolId::Component
    }
    fn status(&self) -> String {
        // CComponentTool.
        "Placer le composant.".into()
    }
    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }
    fn busy(&self) -> bool {
        true
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.snap = Some(cx.snap(m, None, None));
        if let Some(inst) = self.instance(cx) {
            let mut preview = cx.doc.model.clone();
            let d = cx.doc.ctx_def();
            preview.entities_mut(d).instances.insert(inst);
            cx.doc.set_preview(Some(preview));
        }
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some(inst) = self.instance(cx) else { return };
        let mut id = None;
        cx.doc.commit("Placer le composant", |m, d| id = Some(m.entities_mut(d).instances.insert(inst)));
        // CComponentTool: in repeat mode the tool stays as it is; otherwise
        // the new instance is selected and the Move tool takes over.
        if self.repeat {
            return;
        }
        if let Some(i) = id {
            cx.doc.select(vec![EntityRef::Instance(i)]);
            self.placed = true;
        }
    }
    fn switch_to(&mut self) -> Option<ToolId> {
        std::mem::take(&mut self.placed).then_some(ToolId::Move)
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.snap = None;
        cx.doc.set_preview(None);
    }
}
