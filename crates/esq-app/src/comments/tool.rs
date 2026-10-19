//! Tools › Comment (SketchUp's `CommentTool`): a click on the model places the pin of a new
//! comment, whose text is then typed in the Comments panel.
//!
//! Read in SketchUp.exe: activating says « Sélectionner l'objet auquel
//! attacher le commentaire. »; moving the cursor highlights the object under it, or says « à
//! la position 3D » when there is no object; a preselection is what the comment is about (the
//! click only places the pin), but not more than 25 objects (groups, components) or 100
//! elements, which the cursor then says; Escape drops the comment being placed.

use eframe::egui::{self, Color32, Stroke};
use esq_core::glam::{DVec2, DVec3};
use esq_core::model::EntityRef;

use crate::doc::Document;
use crate::pick::{EDGE_PX, Hit};
use crate::tools::{Overlay, Tool, ToolCx, ToolId};

pub const STATUS: &str = "Sélectionner l'objet auquel attacher le commentaire.";
pub const AT_3D_POSITION: &str = "à la position 3D";
pub const TOO_MANY_OBJECTS: &str = "Sélectionnez moins de 25 objets pour créer un commentaire.";
pub const TOO_MANY_ELEMENTS: &str = "Sélectionnez moins de 100 éléments pour créer un commentaire.";

/// A comment being placed: where its pin stands and what it is about. The panel takes it
/// (`super::take_placed`) and asks for the text.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub anchor: DVec3,
    pub target: String,
}

/// Why the selection cannot be commented (SketchUp counts only past 25 selected items).
pub fn selection_problem(doc: &Document) -> Option<&'static str> {
    if doc.sel.len() <= 25 {
        return None;
    }
    let objects = doc.sel.iter().filter(|e| matches!(e, EntityRef::Instance(_))).count();
    if objects > 25 {
        Some(TOO_MANY_OBJECTS)
    } else if doc.sel.len() - objects > 100 {
        Some(TOO_MANY_ELEMENTS)
    } else {
        None
    }
}

/// How the interface names `items` ("Face", "Groupe Mur", "3 entités").
pub fn describe(doc: &Document, items: &[EntityRef]) -> String {
    let ents = doc.ents();
    match items {
        [] => String::new(),
        [EntityRef::Face(_)] => "Face".into(),
        [EntityRef::Edge(_)] => "Arête".into(),
        [EntityRef::Guide(_)] => "Guide".into(),
        [EntityRef::Section(_)] => "Plan de section".into(),
        [EntityRef::Annot(a)] => match ents.annots.get(*a).map(|x| &x.kind) {
            Some(esq_core::model::AnnotKind::Linear { .. }) => "Cotation".into(),
            _ => "Texte".into(),
        },
        [EntityRef::Instance(i)] => {
            let Some(inst) = ents.instances.get(*i) else { return String::new() };
            let def = &doc.model.defs[inst.def];
            let (kind, name) = if def.is_group { ("Groupe", &inst.name) } else { ("Composant", &def.name) };
            if name.is_empty() { kind.into() } else { format!("{kind} {name}") }
        }
        s => format!("{} entités", s.len()),
    }
}

/// What the cursor is on: the item clickable in the active context.
fn under(cx: &ToolCx, m: DVec2) -> Option<EntityRef> {
    let pick = cx.infer.pick;
    let hit: Option<Hit> = pick.screen_edge(cx.cam(), m, EDGE_PX).map(|e| e.2.clone()).or_else(|| {
        let (o, d) = cx.cam().ray(m);
        pick.ray_face(o, d).map(|f| f.1.clone())
    });
    hit.and_then(|h| h.selectable(&cx.doc.ctx))
}

#[derive(Default)]
pub struct CommentTool {
    /// The cursor, in view pixels.
    at: Option<DVec2>,
    /// Where a click would put the pin.
    pos: Option<DVec3>,
    hover: Option<EntityRef>,
    /// What the cursor says ("à la position 3D", too large a selection).
    tip: Option<&'static str>,
}

impl Tool for CommentTool {
    fn id(&self) -> ToolId {
        ToolId::Comment
    }

    fn status(&self) -> String {
        STATUS.into()
    }

    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.at = Some(m);
        let s = cx.snap(m, None, None);
        self.pos = Some(s.pos);
        self.hover = under(cx, m);
        self.tip = selection_problem(cx.doc).or(if self.hover.is_none() { Some(AT_3D_POSITION) } else { None });
    }

    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        if let Some(problem) = selection_problem(cx.doc) {
            *cx.message = Some(problem.into());
            return;
        }
        let Some(anchor) = self.pos else { return };
        // A preselection is what the comment is about; else the object clicked.
        let items: Vec<EntityRef> =
            if cx.doc.sel.is_empty() { self.hover.into_iter().collect() } else { cx.doc.sel.clone() };
        super::place(Placed { anchor, target: describe(cx.doc, &items) });
    }

    fn cancel(&mut self, cx: &mut ToolCx) {
        cx.doc.set_preview(None);
        super::drop_placed();
    }

    fn overlay(&self, o: &mut Overlay) {
        let blue = Color32::from_rgb(0, 154, 217);
        if self.tip.is_none() {
            match self.hover {
                Some(EntityRef::Face(f)) => o.fill_face(f, Color32::from_rgba_unmultiplied(0, 154, 217, 50)),
                Some(EntityRef::Edge(e)) if o.doc.ents().edges.contains_key(e) => {
                    let (a, b) = o.doc.ents().edge_points(e);
                    let x = o.doc.ctx_xform();
                    o.line(x.transform_point3(a), x.transform_point3(b), blue, 3.0);
                }
                Some(EntityRef::Instance(i)) => {
                    if let Some(corners) = super::instance_box(o.doc, i) {
                        for (a, b) in super::BOX_EDGES {
                            o.line(corners[a], corners[b], blue, 2.0);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(m) = self.at {
            let p = o.origin + egui::vec2(m.x as f32, m.y as f32);
            if let Some(tip) = self.tip {
                o.tooltip(p + egui::vec2(16.0, 18.0), tip);
            }
            // The cursor: SketchUp's comment cursor is a small bubble.
            super::paint_bubble(&o.painter, p + egui::vec2(10.0, -6.0), 9.0, blue, Stroke::new(1.5, Color32::WHITE));
        }
    }
}
