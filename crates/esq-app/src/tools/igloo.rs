//! Camera › Image Igloo (`IglooTool`): the matched photos of the model shown where they were
//! taken (each camera as a pyramid with its photo at the base), to pick one and go there.
//! Dragging orbits like the camera tools; the scene that best matches the view is picked
//! after each move, the arrow keys pick another (the nearest one that way on the screen, or
//! in the order of the scenes with Shift), a double-click picks the camera under the cursor
//! and Enter makes the picked scene the active one, which ends the tool.

use eframe::egui::{self, Color32};
use esq_core::glam::{DVec2, DVec3};
use esq_core::model::Model;

use super::{Overlay, Tool, ToolCx, ToolId};
use crate::camera::Camera;

#[derive(Default)]
pub struct IglooTool {
    /// The scene picked (index in the model's scenes) and its name.
    picked: Option<usize>,
    name: Option<String>,
    /// Pick the scene that best matches the view when the drag ends ("Preview Best").
    preview_best: bool,
    drag: Option<DVec2>,
    finish: Option<Option<usize>>,
    started: bool,
}

/// The scenes with a matched photo.
fn photo_scenes(m: &Model) -> Vec<usize> {
    m.scenes.iter().enumerate().filter(|(_, s)| s.photo.is_some()).map(|(i, _)| i).collect()
}

/// The camera of a photo scene, fitted in `size`.
fn scene_view(m: &Model, i: usize, size: DVec2) -> Option<Camera> {
    let p = m.scenes.get(i)?.photo.as_ref()?;
    Some(Camera::from_photo(&p.camera, size, 1.0, DVec2::ZERO).0)
}

/// The photo scene whose camera looks most like `cam` (same direction first, then nearest).
pub fn best_scene(m: &Model, cam: &Camera) -> Option<usize> {
    let (eye, fwd) = (cam.eye(), cam.forward());
    let score = |i: usize| {
        let p = &m.scenes[i].photo.as_ref().unwrap().camera;
        fwd.dot(p.forward()) - eye.distance(p.eye()) * 1e-9
    };
    photo_scenes(m).into_iter().max_by(|&a, &b| score(a).total_cmp(&score(b)))
}

impl IglooTool {
    fn pick(&mut self, cx: &ToolCx, i: Option<usize>) {
        self.picked = i;
        self.name = i.and_then(|i| cx.doc.model.scenes.get(i)).map(|s| s.name.clone());
    }

    fn start(&mut self, cx: &ToolCx) {
        if !self.started {
            self.started = true;
            self.pick(cx, best_scene(&cx.doc.model, cx.cam()));
        }
    }

    /// The view moves to the picked camera (the tool stays).
    fn look_through(&self, cx: &mut ToolCx) {
        if let Some(c) = self.picked.and_then(|i| scene_view(&cx.doc.model, i, cx.cam().size)) {
            *cx.new_cam = Some(c);
        }
    }

    /// Arrow keys: VK_LEFT 0x25, VK_UP 0x26, VK_RIGHT 0x27, VK_DOWN 0x28.
    fn arrow(&mut self, cx: &mut ToolCx, vk: u32) {
        let scenes = photo_scenes(&cx.doc.model);
        let Some(cur) = self.picked.or_else(|| scenes.first().copied()) else { return };
        let next = if cx.mods.shift {
            let k = scenes.iter().position(|&i| i == cur).unwrap_or(0) as isize;
            let d = if vk == 0x27 || vk == 0x28 { 1 } else { -1 };
            Some(scenes[(k + d).rem_euclid(scenes.len() as isize) as usize])
        } else {
            let cam = cx.cam().clone();
            let at = |i: usize| cam.project(cx.doc.model.scenes[i].photo.as_ref().unwrap().camera.eye());
            let dir = match vk {
                0x25 => DVec2::NEG_X,
                0x27 => DVec2::X,
                0x26 => DVec2::NEG_Y,
                _ => DVec2::Y,
            };
            let from = at(cur);
            scenes
                .iter()
                .copied()
                .filter(|&i| i != cur)
                .filter_map(|i| {
                    let d = at(i)? - from?;
                    let along = d.dot(dir);
                    (along > 0.0 && along >= d.length() * 0.5).then_some((d.length(), i))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|x| x.1)
        };
        if next.is_some() {
            self.pick(cx, next);
            self.preview_best = false;
            self.look_through(cx);
        }
    }

    /// The four corners of a photo's pyramid, `depth` in front of its eye.
    fn corners(p: &esq_core::matchphoto::PhotoCamera, depth: f64) -> [DVec3; 4] {
        let f = p.forward();
        let px = [DVec2::ZERO, DVec2::new(p.size.x, 0.0), p.size, DVec2::new(0.0, p.size.y)];
        px.map(|q| {
            let (o, d) = p.ray(q);
            o + d * (depth / d.dot(f).max(1e-9))
        })
    }
}

impl Tool for IglooTool {
    fn id(&self) -> ToolId {
        ToolId::Igloo
    }
    fn status(&self) -> String {
        let mut s = String::from("Utilisez les flèches pour sélectionner la scène.  Maj = dans l'ordre");
        if let Some(name) = &self.name {
            s.push_str(&format!(", Entrer pour activer {name}."));
        }
        s
    }
    fn wants_keys(&self) -> bool {
        true
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.start(cx);
        if let Some(l) = self.drag {
            let mut c = cx.cam().clone();
            c.orbit(m.x - l.x, m.y - l.y);
            *cx.new_cam = Some(c);
            self.drag = Some(m);
            self.preview_best = true;
        }
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.start(cx);
        self.drag = Some(m);
    }
    fn release(&mut self, cx: &mut ToolCx, _m: DVec2) {
        self.drag = None;
        if self.preview_best {
            let best = best_scene(&cx.doc.model, cx.new_cam.as_ref().unwrap_or(cx.cam()));
            self.pick(cx, best);
            self.preview_best = false;
        }
    }
    fn double_click(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.start(cx);
        let cam = cx.cam().clone();
        let hit = photo_scenes(&cx.doc.model)
            .into_iter()
            .filter_map(|i| {
                let e = cam.project(cx.doc.model.scenes[i].photo.as_ref()?.camera.eye())?;
                let d = e.distance(m);
                (d < 20.0).then_some((d, i))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, i)) = hit {
            self.pick(cx, Some(i));
            self.look_through(cx);
        }
    }
    fn key(&mut self, cx: &mut ToolCx, vk: u32, down: bool, _repeat: u32) {
        if down && (0x25..=0x28).contains(&vk) {
            self.start(cx);
            self.arrow(cx, vk);
        }
    }
    fn enter(&mut self, cx: &mut ToolCx) {
        self.start(cx);
        if let Some(i) = self.picked {
            self.look_through(cx);
            self.finish = Some(Some(i));
        }
    }
    fn finished(&mut self) -> Option<Option<usize>> {
        self.finish.take()
    }
    fn overlay(&self, o: &mut Overlay) {
        let m = &o.doc.model;
        for i in photo_scenes(m) {
            let photo = m.scenes[i].photo.as_ref().unwrap();
            let p = &photo.camera;
            let eye = p.eye();
            let depth = o.cam.pixel_size_at(eye + p.forward()) * 90.0;
            let c = Self::corners(p, depth);
            let color = if Some(i) == self.picked { Color32::from_rgb(240, 120, 0) } else { Color32::from_gray(90) };
            // The photo at the base of the pyramid.
            let pts: Option<Vec<egui::Pos2>> = c.iter().map(|q| o.screen(*q)).collect();
            if let (Some(pts), Some(tex)) = (pts, crate::photo_img::texture(o.painter.ctx(), &photo.image)) {
                let mut mesh = egui::Mesh::with_texture(tex);
                let uv = [egui::pos2(0.0, 0.0), egui::pos2(1.0, 0.0), egui::pos2(1.0, 1.0), egui::pos2(0.0, 1.0)];
                for (q, t) in pts.iter().zip(uv) {
                    mesh.vertices.push(egui::epaint::Vertex { pos: *q, uv: t, color: Color32::from_white_alpha(220) });
                }
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                o.painter.add(egui::Shape::mesh(mesh));
            }
            for q in c {
                o.line(eye, q, color, 1.0);
            }
            o.polyline(&c, true, color, 1.5);
            o.text_at(eye, &m.scenes[i].name);
        }
    }
}
