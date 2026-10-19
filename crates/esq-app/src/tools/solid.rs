//! Outer Shell and the Solid Tools: click a first solid group/component, then a second one.

use esq_core::glam::DVec2;
use esq_core::model::{EntityRef, InstanceId};
use esq_core::solid::SolidOp;

use super::{Overlay, Tool, ToolCx, ToolId};

/// What the tool does: one of the solid operations, or Outer Shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolidKind {
    OuterShell,
    Op(SolidOp),
}

impl SolidKind {
    pub fn tool(self) -> ToolId {
        match self {
            SolidKind::OuterShell => ToolId::OuterShell,
            SolidKind::Op(SolidOp::Intersect) => ToolId::SolidIntersect,
            SolidKind::Op(SolidOp::Union) => ToolId::SolidUnion,
            SolidKind::Op(SolidOp::Subtract) => ToolId::SolidSubtract,
            SolidKind::Op(SolidOp::Trim) => ToolId::SolidTrim,
            SolidKind::Op(SolidOp::Split) => ToolId::SolidSplit,
        }
    }

    pub fn of(tool: ToolId) -> Option<SolidKind> {
        Some(match tool {
            ToolId::OuterShell => SolidKind::OuterShell,
            ToolId::SolidIntersect => SolidKind::Op(SolidOp::Intersect),
            ToolId::SolidUnion => SolidKind::Op(SolidOp::Union),
            ToolId::SolidSubtract => SolidKind::Op(SolidOp::Subtract),
            ToolId::SolidTrim => SolidKind::Op(SolidOp::Trim),
            ToolId::SolidSplit => SolidKind::Op(SolidOp::Split),
            _ => return None,
        })
    }

    /// Runs the operation on two solids; returns the new groups.
    pub fn run(
        self,
        doc: &mut crate::doc::Document,
        first: InstanceId,
        second: InstanceId,
    ) -> Result<Vec<InstanceId>, String> {
        let mut made = Vec::new();
        doc.try_commit(self.tool().label(), |m, d| {
            made = match self {
                SolidKind::OuterShell => vec![m.outer_shell(d, &[first, second])?],
                SolidKind::Op(op) => m.solid_op(d, first, second, op)?,
            };
            Ok::<(), String>(())
        })?;
        doc.select(made.iter().map(|i| EntityRef::Instance(*i)).collect());
        Ok(made)
    }
}

pub struct SolidTool {
    kind: SolidKind,
    first: Option<InstanceId>,
    /// The group/component under the cursor, and whether it is a solid.
    hover: Option<(InstanceId, bool)>,
    cursor: DVec2,
}

impl SolidTool {
    pub fn new(kind: SolidKind) -> Self {
        SolidTool { kind, first: None, hover: None, cursor: DVec2::ZERO }
    }
}

impl Tool for SolidTool {
    fn id(&self) -> ToolId {
        self.kind.tool()
    }
    fn status(&self) -> String {
        // CBoolean…Tool: the same two messages for every operation ("Pas un solide" is the
        // tooltip by the cursor).
        if self.first.is_some() { "Sélectionnez le deuxième solide." } else { "Sélectionnez le premier solide." }.into()
    }
    fn busy(&self) -> bool {
        self.first.is_some()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.cursor = m;
        let (o, d) = cx.cam().ray(m);
        self.hover = match cx.infer.pick.ray_face(o, d).and_then(|(_, h, _)| h.selectable(&cx.doc.ctx)) {
            Some(EntityRef::Instance(i)) => Some((i, cx.doc.model.is_solid_instance(cx.doc.ctx_def(), i))),
            _ => None,
        };
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        let Some((i, true)) = self.hover else { return };
        match self.first {
            None => {
                self.first = Some(i);
                cx.doc.select(vec![EntityRef::Instance(i)]);
            }
            Some(first) if first != i => {
                self.first = None;
                self.hover = None;
                if let Err(e) = self.kind.run(cx.doc, first, i) {
                    *cx.message = Some(e);
                }
            }
            Some(_) => {}
        }
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.first = None;
        cx.doc.select(Vec::new());
    }
    fn overlay(&self, o: &mut Overlay) {
        if let Some((_, solid)) = self.hover {
            let text = if solid { "Solide" } else { "Pas un solide" };
            let at = o.origin + eframe::egui::vec2(self.cursor.x as f32 + 14.0, self.cursor.y as f32 + 14.0);
            o.tooltip(at, text);
        }
    }
}
