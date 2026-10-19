//! Selection: click, Ctrl (add), Shift (toggle), Ctrl+Shift (remove), window and crossing
//! selection, double-click to open groups or select a face with its edges, triple-click for
//! everything connected.

use eframe::egui::{self, Color32, Stroke};
use esq_core::glam::DVec2;
use esq_core::model::EntityRef;

use super::{Overlay, Tool, ToolCx, ToolId};
use crate::pick::{EDGE_PX, Hit};

#[derive(Default)]
pub struct SelectTool {
    down: Option<DVec2>,
    cur: DVec2,
}

fn under_cursor(cx: &ToolCx, m: DVec2) -> Option<Hit> {
    hit_under(cx.infer.pick, cx.cam(), m)
}

/// What a click at `m` picks (also the right-click menu's entity): an annotation, else an
/// edge, a guide or a section plane unless a face in front hides it, else the face.
pub fn hit_under(pick: &crate::pick::PickCache, cam: &crate::camera::Camera, m: DVec2) -> Option<Hit> {
    // Prefer an edge, then a guide, unless a face in front hides it.
    if let Some(hit) = pick.screen_annot(cam, m, EDGE_PX) {
        return Some(hit.clone());
    }
    // Snaps are drawn over everything too.
    if let Some(hit) = pick.screen_snap(cam, m, EDGE_PX) {
        return Some(hit.clone());
    }
    let (o, d) = cam.ray(m);
    let face = pick.ray_face(o, d);
    let face_t = face.as_ref().map(|f| f.0).unwrap_or(f64::INFINITY);
    let in_front = |p: esq_core::glam::DVec3| (p - o).dot(d) <= face_t * 1.001 + 1.0;
    if let Some((_, p, hit, _)) = pick.screen_edge(cam, m, EDGE_PX)
        && in_front(p)
    {
        return Some(hit.clone());
    }
    if let Some((_, p, hit)) = pick.screen_guide(cam, m, EDGE_PX)
        && in_front(p)
    {
        return Some(hit.clone());
    }
    // Section planes show through faces (dashed), so they can be picked there too.
    if let Some(hit) = pick.screen_section(cam, m, EDGE_PX) {
        return Some(hit.clone());
    }
    // So do spaces (see-through volumes); an edge on their outline wins (above).
    if let Some(hit) = pick.screen_space(cam, m, EDGE_PX) {
        return Some(hit.clone());
    }
    face.map(|(_, h, _)| h.clone())
}

pub(super) fn apply(cx: &mut ToolCx, items: Vec<EntityRef>) {
    let mods = cx.mods;
    let mut sel = cx.doc.sel.clone();
    if mods.ctrl && mods.shift {
        sel.retain(|e| !items.contains(e));
    } else if mods.shift {
        for it in items {
            if let Some(i) = sel.iter().position(|e| *e == it) {
                sel.remove(i);
            } else {
                sel.push(it);
            }
        }
    } else if mods.ctrl {
        sel.extend(items);
    } else {
        sel = items;
    }
    cx.doc.select(sel);
}

impl Tool for SelectTool {
    fn id(&self) -> ToolId {
        ToolId::Select
    }
    fn status(&self) -> String {
        // CSelectionTool.
        "Cliquez ou faites glisser pour sélectionner des objets. Maj = Ajouter/Soustraire. Ctrl = Ajouter. \
         Maj + Ctrl = Soustraire."
            .into()
    }
    fn mouse_move(&mut self, _cx: &mut ToolCx, m: DVec2) {
        self.cur = m;
    }
    fn press(&mut self, _cx: &mut ToolCx, m: DVec2) {
        self.down = Some(m);
        self.cur = m;
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        let Some(d) = self.down.take() else { return };
        if d.distance(m) > 4.0 {
            let crossing = m.x < d.x;
            let (min, max) = (d.min(m), d.max(m));
            let ctx = cx.doc.ctx.clone();
            let mut items: Vec<EntityRef> = cx
                .infer
                .pick
                .hits_in_rect(cx.cam(), min, max, crossing)
                .into_iter()
                .filter_map(|h| h.selectable(&ctx))
                .collect();
            items.sort();
            items.dedup();
            apply(cx, items);
            return;
        }
        match under_cursor(cx, m) {
            Some(hit) => match hit.selectable(&cx.doc.ctx) {
                Some(e) => apply(cx, vec![e]),
                None => {
                    // Clicking outside the open group closes it.
                    cx.doc.exit_all();
                }
            },
            None => {
                if !cx.mods.ctrl && !cx.mods.shift {
                    if cx.doc.ctx.is_empty() {
                        cx.doc.select(Vec::new());
                    } else {
                        cx.doc.exit();
                        cx.doc.select(Vec::new());
                    }
                }
            }
        }
    }
    fn double_click(&mut self, cx: &mut ToolCx, m: DVec2) {
        let Some(hit) = under_cursor(cx, m) else { return };
        let Some(e) = hit.selectable(&cx.doc.ctx) else { return };
        match e {
            EntityRef::Instance(i) => cx.doc.enter(i),
            EntityRef::Face(f) => {
                let ents = cx.doc.ents();
                let mut items: Vec<EntityRef> = ents.face_edges(f).into_iter().map(EntityRef::Edge).collect();
                items.push(EntityRef::Face(f));
                apply(cx, items);
            }
            EntityRef::Edge(ed) => {
                let ents = cx.doc.ents();
                let mut items: Vec<EntityRef> = ents.edges[ed].faces.iter().map(|f| EntityRef::Face(*f)).collect();
                items.push(EntityRef::Edge(ed));
                apply(cx, items);
            }
            EntityRef::Guide(_) | EntityRef::Snap(_) | EntityRef::Space(_) => apply(cx, vec![e]),
            // Double-clicking a section plane makes it the active cut (or turns it off).
            EntityRef::Section(s) => {
                let def = cx.doc.ctx_def();
                cx.doc.commit("Activer la coupe", |m, _| {
                    let ents = m.entities_mut(def);
                    ents.active_section = if ents.active_section == Some(s) { None } else { Some(s) };
                });
                apply(cx, vec![e]);
            }
            EntityRef::Annot(a) => {
                apply(cx, vec![e]);
                *cx.edit_text = Some(a);
            }
        }
    }
    fn triple_click(&mut self, cx: &mut ToolCx, m: DVec2) {
        select_connected(cx, m);
    }
    fn busy(&self) -> bool {
        self.down.is_some()
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        self.down = None;
        cx.doc.set_preview(None);
    }
    fn overlay(&self, o: &mut Overlay) {
        if let Some(d) = self.down
            && d.distance(self.cur) > 4.0
        {
            let a = o.origin + egui::vec2(d.x as f32, d.y as f32);
            let b = o.origin + egui::vec2(self.cur.x as f32, self.cur.y as f32);
            let r = egui::Rect::from_two_pos(a, b);
            let crossing = self.cur.x < d.x;
            o.painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(80, 120, 255, 25));
            if crossing {
                let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                o.painter.extend(egui::Shape::dashed_line(&c, Stroke::new(1.0, Color32::from_gray(40)), 5.0, 3.0));
            } else {
                o.painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(40)), egui::StrokeKind::Middle);
            }
        }
    }
}

/// Triple-click: everything connected to what is under the cursor.
pub fn select_connected(cx: &mut ToolCx, m: DVec2) {
    let Some(hit) = under_cursor(cx, m) else { return };
    let Some(e) = hit.selectable(&cx.doc.ctx) else { return };
    if !matches!(e, EntityRef::Edge(_) | EntityRef::Face(_)) {
        return;
    }
    let items = cx.doc.ents().connected(e);
    apply(cx, items);
}
