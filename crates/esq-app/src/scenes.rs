//! Scenes: saving points of view, going from one to the next with a smooth camera move,
//! playing them as a slideshow (View › Animation), and the scene tabs.

use eframe::egui::{self, RichText};
use esq_core::model::Scene;

use crate::app::EsquisseApp;
use crate::camera::Camera;

/// A camera move in progress.
pub struct Transition {
    from: Camera,
    to: Camera,
    start: f64,
    duration: f64,
}

impl Transition {
    /// How far the move is at `now`, 0 to 1 (`frameChange` of Ruby's frame observers).
    pub fn progress(&self, now: f64) -> f64 {
        if self.duration <= 0.0 { 1.0 } else { ((now - self.start) / self.duration).clamp(0.0, 1.0) }
    }
}

/// View › Animation › Play: the scene shown and when it was reached.
pub struct Playing {
    pub index: usize,
    pub reached: Option<f64>,
}

/// View › Animation › Settings.
pub struct AnimSettings {
    pub transition: f64,
    pub delay: f64,
}

impl Default for AnimSettings {
    fn default() -> Self {
        AnimSettings { transition: 2.0, delay: 1.0 }
    }
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(crate) fn lerp_cam(a: &Camera, b: &Camera, t: f64) -> Camera {
    let s = smoothstep(t);
    let mut c = b.clone();
    c.target = a.target.lerp(b.target, s);
    c.distance = (a.distance.ln() * (1.0 - s) + b.distance.ln() * s).exp();
    let tau = std::f64::consts::TAU;
    let dy = (b.yaw - a.yaw + std::f64::consts::PI).rem_euclid(tau) - std::f64::consts::PI;
    c.yaw = a.yaw + dy * s;
    c.pitch = a.pitch + (b.pitch - a.pitch) * s;
    c.fov_y = a.fov_y + (b.fov_y - a.fov_y) * s;
    c.roll = a.roll + (b.roll - a.roll) * s;
    c.shift = a.shift.lerp(b.shift, s);
    c.perspective = if s < 1.0 { a.perspective || b.perspective } else { b.perspective };
    c.size = a.size;
    c
}

impl EsquisseApp {
    /// Before a scene is added or updated with style changes not saved, SketchUp asks what to
    /// do with them (`StylesWarningDlg`, preference `ShowStylesWarning`): save them as a new
    /// style, update the selected style, or do nothing; then `then` runs (not on Annuler).
    fn ask_style_changes(&mut self, update: bool, then: fn(&mut EsquisseApp)) {
        let (list, sel) = crate::styles::model_styles(&mut self.doc.model, &self.style);
        if !list.iter().find(|s| s.id == sel).is_some_and(|s| s.differs(&self.style)) {
            return then(self);
        }
        let button = if update { "Actualiser la scène" } else { "Créer une scène" };
        let text = "Attention! Vous êtes en train de créer ou d'actualiser une scène, mais vous n'avez pas \
                    enregistré les modifications apportées à votre style.\n\nQue voulez-vous faire de vos \
                    modifications de style?";
        let m = crate::warnings::Message::question(
            "ShowStylesWarning",
            "Attention - Scènes et styles",
            text,
            &[button, "Annuler"],
            "Ne plus afficher ce message.  Je vais gérer les modifications de styles moi-même.",
        )
        .choices(
            &[
                "Enregistrer comme nouveau style.",
                "Actualiser le style sélectionné.",
                "Ne rien faire pour enregistrer les modifications.",
            ],
            0,
        )
        // Stopped: « I will manage style changes on my own ».
        .usual(0, 2)
        .then(move |app, a| {
            if a.button != 0 {
                return;
            }
            match a.choice {
                0 => app.create_style(),
                1 => app.update_style(),
                _ => {}
            }
            then(app);
        });
        crate::warnings::show(self, m);
    }

    /// View › Animation › Add Scene.
    pub fn add_scene(&mut self) {
        self.ask_style_changes(false, Self::add_scene_now);
    }

    fn add_scene_now(&mut self) {
        let mut scene = Scene {
            name: self.doc.model.next_scene_name(),
            description: String::new(),
            camera: self.cam.to_view(),
            hidden_layers: None,
            photo: None,
            ext: Default::default(),
        };
        // Everything a scene can save (`dialogs::scene_props`).
        crate::dialogs::scene_props::capture(self, &mut scene, crate::dialogs::scene_props::ALL);
        let at = self.active_scene.map(|i| i + 1).unwrap_or(self.doc.model.scenes.len());
        self.doc.commit("Ajouter une scène", |m, _| m.scenes.insert(at, scene));
        self.active_scene = Some(at);
        self.style.scene_tabs = true;
    }

    /// View › Animation › Update Scene: the present state replaces what the scene saves.
    pub fn update_scene(&mut self) {
        self.ask_style_changes(true, Self::update_scene_now);
    }

    fn update_scene_now(&mut self) {
        let Some(i) = self.active_scene.filter(|i| *i < self.doc.model.scenes.len()) else { return };
        let f = crate::dialogs::scene_props::flags(&self.doc.model.scenes[i]);
        crate::dialogs::scene_props::update(self, i, f);
    }

    pub fn delete_scene(&mut self) {
        let Some(i) = self.active_scene.filter(|i| *i < self.doc.model.scenes.len()) else { return };
        self.doc.commit("Supprimer la scène", |m, _| {
            m.scenes.remove(i);
        });
        let n = self.doc.model.scenes.len();
        self.active_scene = if n == 0 { None } else { Some(i.min(n - 1)) };
    }

    pub fn move_scene(&mut self, i: usize, delta: isize) {
        let n = self.doc.model.scenes.len() as isize;
        let j = i as isize + delta;
        if j < 0 || j >= n {
            return;
        }
        self.doc.commit("Déplacer la scène", |m, _| m.scenes.swap(i, j as usize));
        self.active_scene = Some(j as usize);
    }

    /// Goes to a scene: what it saves (`dialogs::scene_props`), and its point of view
    /// (smoothly unless `instant`) when it saves the camera.
    pub fn go_to_scene(&mut self, i: usize, now: f64, instant: bool) {
        let Some(s) = self.doc.model.scenes.get(i).cloned() else { return };
        self.active_scene = Some(i);
        crate::dialogs::scene_props::apply(self, &s);
        if crate::dialogs::scene_props::flags(&s) & crate::dialogs::scene_props::CAMERA == 0 {
            return;
        }
        let mut to = self.cam.clone();
        to.set_from_view(&s.camera);
        if instant || self.anim.transition <= 0.0 {
            self.cam = to;
        } else {
            self.remember_view();
            self.transition =
                Some(Transition { from: self.cam.clone(), to, start: now, duration: self.anim.transition });
        }
    }

    pub fn step_scene(&mut self, delta: isize, now: f64) {
        let n = self.doc.model.scenes.len();
        if n == 0 {
            return;
        }
        let i = match self.active_scene {
            Some(i) => (i as isize + delta).rem_euclid(n as isize) as usize,
            None => 0,
        };
        self.go_to_scene(i, now, false);
    }

    /// View › Animation › Play / Stop.
    pub fn toggle_play(&mut self, now: f64) {
        if self.playing.take().is_some() {
            return;
        }
        // Only the scenes included in the animation (Scenes › Include in animation).
        let Some(i) = crate::dialogs::scene_props::next_animated(&self.doc.model, self.active_scene) else { return };
        self.go_to_scene(i, now, false);
        // A scene that does not save the camera is reached at once.
        self.playing = Some(Playing { index: i, reached: self.transition.is_none().then_some(now) });
    }

    /// Advances the camera move and the slideshow; true while something moves.
    pub fn animate(&mut self, now: f64) -> bool {
        let mut moving = false;
        if let Some(t) = &self.transition {
            let k = (now - t.start) / t.duration;
            let size = self.cam.size;
            self.cam = lerp_cam(&t.from, &t.to, k);
            self.cam.size = size;
            if k >= 1.0 {
                self.transition = None;
                if let Some(p) = &mut self.playing {
                    p.reached = Some(now);
                }
            }
            moving = true;
        }
        if let Some(p) = &self.playing {
            moving = true;
            if let Some(r) = p.reached
                && now - r >= self.anim.delay
            {
                match crate::dialogs::scene_props::next_animated(&self.doc.model, Some(p.index)) {
                    Some(next) => {
                        self.go_to_scene(next, now, false);
                        let reached = self.transition.is_none().then_some(now);
                        self.playing = Some(Playing { index: next, reached });
                    }
                    None => self.playing = None,
                }
            }
        }
        moving
    }

    /// The scene tabs above the 3D view (View › Scene Tabs).
    pub fn scene_tabs(&mut self, ui: &mut egui::Ui) {
        if !self.style.scene_tabs || self.doc.model.scenes.is_empty() {
            return;
        }
        let now = ui.input(|i| i.time);
        egui::Panel::top("scene_tabs").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let names: Vec<String> = self.doc.model.scenes.iter().map(|s| s.name.clone()).collect();
                for (i, name) in names.iter().enumerate() {
                    let r = ui.selectable_label(self.active_scene == Some(i), RichText::new(name));
                    if r.clicked() {
                        self.playing = None;
                        self.go_to_scene(i, now, false);
                    }
                    r.context_menu(|ui| {
                        self.active_scene = Some(i);
                        if ui.button("Actualiser").clicked() {
                            self.update_scene();
                            ui.close();
                        }
                        if ui.button("Ajouter").clicked() {
                            self.add_scene();
                            ui.close();
                        }
                        if ui.button("Supprimer").clicked() {
                            self.delete_scene();
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Déplacer vers la gauche").clicked() {
                            self.move_scene(i, -1);
                            ui.close();
                        }
                        if ui.button("Déplacer vers la droite").clicked() {
                            self.move_scene(i, 1);
                            ui.close();
                        }
                    });
                }
            });
        });
    }

    /// Window › Scenes.
    pub fn scenes_panel(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        ui.horizontal(|ui| {
            if ui.button("+").on_hover_text("Ajouter une scène").clicked() {
                self.add_scene();
            }
            let has = self.active_scene.is_some_and(|i| i < self.doc.model.scenes.len());
            if ui.add_enabled(has, egui::Button::new("⟳")).on_hover_text("Actualiser la scène").clicked() {
                crate::dialogs::scene_props::ask_update(self);
            }
            if ui.add_enabled(has, egui::Button::new("−")).on_hover_text("Supprimer la scène").clicked() {
                self.delete_scene();
            }
            if let Some(i) = self.active_scene.filter(|_| has) {
                if ui.button("▲").on_hover_text("Monter").clicked() {
                    self.move_scene(i, -1);
                }
                if ui.button("▼").on_hover_text("Descendre").clicked() {
                    self.move_scene(i, 1);
                }
            }
        });
        if self.doc.model.scenes.is_empty() {
            ui.label("Aucune scène. « + » enregistre le point de vue actuel.");
            return;
        }
        let scenes = self.doc.model.scenes.clone();
        for (i, s) in scenes.iter().enumerate() {
            if ui.selectable_label(self.active_scene == Some(i), &s.name).clicked() {
                self.playing = None;
                self.go_to_scene(i, now, false);
            }
        }
        if let Some(i) = self.active_scene.filter(|i| *i < scenes.len()) {
            ui.separator();
            crate::dialogs::scene_props::properties_ui(self, ui, i);
        }
    }
}
