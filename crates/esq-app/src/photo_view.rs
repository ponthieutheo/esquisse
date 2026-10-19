//! Match Photo in the application: the photo behind the 3D view, and the commands of the
//! Camera menu (Match New Photo, Edit Matched Photo, Zoom Matched Photo).
//!
//! A matched photo is a scene (`Scene::photo`). While the camera sits exactly on the one of
//! that scene, the photo is painted behind the (transparent) 3D view and the zoom of the
//! view follows the photo; as soon as the camera moves away the photo is hidden.

use eframe::egui;
use esq_core::glam::DVec2;
use esq_core::photo::MatchPhoto;

use crate::app::EsquisseApp;
use crate::camera::{Camera, PhotoMap};
use crate::tools::{MatchPhotoTool, Overlay, PhotoEditor, ToolId};

impl EsquisseApp {
    /// The photo of the active scene, if it has one.
    pub fn active_photo(&self) -> Option<&MatchPhoto> {
        self.active_scene.and_then(|i| self.doc.model.scenes.get(i)).and_then(|s| s.photo.as_ref())
    }

    /// Before the 3D view is painted: lets a tool impose its camera, and paints the photo
    /// behind. True if a photo shows, in which case the 3D view must be transparent.
    pub fn photo_pre(&mut self, ui: &egui::Ui, rect: egui::Rect) -> bool {
        let size = self.cam.size;
        self.photo_shown = None;
        // The Photo Point tool only works on a matched photo (SketchUp drops it on "Changed To
        // 3D" and on a scene change).
        if self.tool.id() == ToolId::PhotoPoint
            && !self.active_photo().is_some_and(|p| self.cam.is_photo_view(&p.camera))
        {
            self.set_tool(ToolId::Select);
        }
        if let Some(mut c) = self.tool.camera(size) {
            c.size = size;
            self.cam = c;
        }
        let o = Overlay {
            painter: ui.painter_at(rect),
            origin: rect.min,
            cam: &self.cam,
            doc: &self.doc,
            units: self.doc.model.units,
        };
        if self.tool.backdrop(&o) {
            return true;
        }
        let Some(photo) = self.active_photo().cloned() else { return false };
        if !self.cam.is_photo_view(&photo.camera) {
            return false;
        }
        let map = PhotoMap::new(&photo.camera, size, 1.0, DVec2::ZERO);
        self.cam.fov_y = map.fov_y;
        self.cam.shift = map.shift;
        let (tl, br) = map.rect();
        let r = egui::Rect::from_min_max(
            rect.min + egui::vec2(tl.x as f32, tl.y as f32),
            rect.min + egui::vec2(br.x as f32, br.y as f32),
        );
        // Styles › Modeling › Match Photo › Background: the photo shows (or not) at its opacity,
        // over the style's background colour.
        let c = self.style.bg_color;
        ui.painter_at(rect).rect_filled(rect, 0.0, egui::Color32::from_rgb(c[0], c[1], c[2]));
        if self.style.photo_bg {
            let opacity = self.style.photo_bg_opacity.clamp(0.0, 1.0) as f32;
            crate::photo_img::paint(ui.ctx(), &ui.painter_at(rect), &photo.image, r, opacity);
        }
        self.photo_shown = Some((photo.image.clone(), [tl.x as f32, tl.y as f32, br.x as f32, br.y as f32]));
        true
    }

    /// Camera › Match New Photo...
    pub fn photo_new(&mut self) {
        let Some(path) = crate::settings::dialog("textures")
            .set_title("Adapter une nouvelle photo")
            .add_filter("Images", &["png", "jpg", "jpeg"])
            .pick_file()
        else {
            return;
        };
        match MatchPhotoTool::from_file(&path) {
            Ok(t) => self.start_photo_tool(t),
            Err(e) => self.message = Some((e, 0.0)),
        }
    }

    /// Camera › Edit Matched Photo: the active scene's photo goes back under the bars.
    pub fn photo_edit(&mut self) {
        let Some(i) = self.active_scene else { return };
        let Some(photo) = self.doc.model.scenes.get(i).and_then(|s| s.photo.clone()) else { return };
        self.start_photo_tool(MatchPhotoTool::with_editor(PhotoEditor::from_photo(i, &photo)));
    }

    fn start_photo_tool(&mut self, t: MatchPhotoTool) {
        self.set_tool(ToolId::Select);
        self.remember_view();
        self.tool = Box::new(t);
    }

    /// Camera › Zoom Matched Photo: the view goes back to the photo, entirely shown.
    pub fn photo_zoom(&mut self) {
        let Some(photo) = self.active_photo().cloned() else { return };
        self.remember_view();
        let size = self.cam.size;
        self.cam = Camera::from_photo(&photo.camera, size, 1.0, DVec2::ZERO).0;
    }

    pub fn photo_cmd_enabled(&self) -> bool {
        self.active_photo().is_some()
    }

    /// Window › Match Photo: the photo of the active scene, its grid, and the commands.
    pub fn photo_panel(&mut self, ui: &mut egui::Ui) {
        let Some(i) = self.active_scene.filter(|_| self.active_photo().is_some()) else {
            ui.label("La scène active n'est pas une photo adaptée.");
            if ui.button("Adapter nouvelle photo...").clicked() {
                self.photo_new();
            }
            return;
        };
        let mut photo = self.active_photo().cloned().expect("checked above");
        let units = self.doc.model.units;
        ui.label(egui::RichText::new(&photo.name).strong());
        ui.label(format!("{} × {} pixels, intégrée au modèle", photo.size[0], photo.size[1]));
        let cam = &photo.camera;
        let fov_x = 2.0 * (cam.size.x * 0.5 / cam.focal).atan().to_degrees();
        ui.label(format!("Champ angulaire : {:.1}° (horizontal), {:.1}° (vertical)", fov_x, cam.fov_y_deg()));
        let off = cam.centre_offset();
        ui.label(format!("Centre optique : {:+.1} %, {:+.1} %", off.x * 100.0, off.y * 100.0));
        ui.label(format!("Distance de l'origine : {}", esq_core::units::format_length(cam.trans.length(), units)));
        ui.separator();
        ui.label("Grille de l'outil :");
        let before = photo.grid;
        ui.horizontal(|ui| {
            ui.checkbox(&mut photo.grid.show[0], "Rouge-vert");
            ui.checkbox(&mut photo.grid.show[1], "Rouge-bleu");
            ui.checkbox(&mut photo.grid.show[2], "Vert-bleu");
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut photo.grid.style, esq_core::photo::GridStyle::Lines, "Lignes");
            ui.selectable_value(&mut photo.grid.style, esq_core::photo::GridStyle::Crosses, "Croix");
        });
        if photo.grid != before {
            self.doc.commit("Grille de la photo", |m, _| m.scenes[i].photo = Some(photo));
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Modifier...").clicked() {
                self.photo_edit();
            }
            if ui.button("Zoom sur la photo").clicked() {
                self.photo_zoom();
            }
            // CAddRayTool, the "add camera guide" button of SketchUp's Match Photo panel.
            let on = self.tool.id() == ToolId::PhotoPoint;
            if ui.selectable_label(on, ToolId::PhotoPoint.label()).on_hover_text("Guide point photo").clicked() {
                self.set_tool(if on { ToolId::Select } else { ToolId::PhotoPoint });
            }
        });
        // SketchUp's « Project textures from photo » (as the right-click menu).
        let faces = self.doc.sel.iter().any(|e| matches!(e, esq_core::model::EntityRef::Face(_)));
        if ui.add_enabled(faces, egui::Button::new("Projeter les textures de la photo")).clicked() {
            crate::ctx_dialogs::project_photo(self);
        }
    }

    /// After a tool is done: select the scene it made, or give the old view back.
    pub fn photo_tool_done(&mut self) {
        let Some(done) = self.tool.finished() else { return };
        match done {
            Some(i) => {
                self.active_scene = Some(i);
                self.style.scene_tabs = true;
                self.tool = ToolId::Select.make();
            }
            None => {
                self.tool = ToolId::Select.make();
                self.camera_back();
            }
        }
    }

    /// On opening a file whose first scene is a photo: show it.
    pub fn open_photo_scene(&mut self) {
        if let Some(i) = self.doc.model.scenes.iter().position(|s| s.photo.is_some()) {
            self.go_to_scene(i, 0.0, true);
            self.style.scene_tabs = true;
        }
    }

    /// `--snapshot`: the photo under the rendered view (the render is transparent where
    /// there is no model). `px` is RGBA, premultiplied.
    pub fn photo_composite(&self, w: u32, h: u32, mut px: Vec<u8>) -> Vec<u8> {
        let Some((blob, [x0, y0, x1, y1])) = &self.photo_shown else { return px };
        let Ok(img) = crate::photo_img::decode(&blob.0) else { return px };
        let ppp = w as f32 / self.cam.size.x as f32;
        let (x0, y0, x1, y1) = (x0 * ppp, y0 * ppp, x1 * ppp, y1 * ppp);
        let rgba = img.to_rgba8();
        let (iw, ih) = (rgba.width() as f32, rgba.height() as f32);
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = ((x as f32 + 0.5 - x0) / (x1 - x0), (y as f32 + 0.5 - y0) / (y1 - y0));
                if !(0.0..1.0).contains(&fx) || !(0.0..1.0).contains(&fy) {
                    continue;
                }
                let p = rgba.get_pixel((fx * iw) as u32, (fy * ih) as u32);
                let i = ((y * w + x) * 4) as usize;
                let a = px[i + 3] as f32 / 255.0;
                // The photo at the style's background opacity over its background colour.
                let o = if self.style.photo_bg { self.style.photo_bg_opacity.clamp(0.0, 1.0) as f32 } else { 0.0 };
                for k in 0..3 {
                    let under = self.style.bg_color[k] as f32 * (1.0 - o) + p[k] as f32 * o;
                    px[i + k] = (px[i + k] as f32 + under * (1.0 - a)).min(255.0) as u8;
                }
                px[i + 3] = 255;
            }
        }
        px
    }
}
