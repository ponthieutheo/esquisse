//! Renders the model offscreen, outside the window's own size, for the exports that need a
//! picture (animation frames, printing). `step` is called once per displayed frame, after the
//! scene mesh is uploaded and before the window's own render.

use eframe::egui;
use eframe::egui_wgpu;

use crate::app::EsquisseApp;
use crate::camera::Camera;

#[derive(Default)]
pub struct Jobs {
    pub anim: Option<crate::export_anim::AnimJob>,
    pub print: Option<crate::print::PrintJob>,
    pub raster: Option<crate::export2d::RasterJob>,
}

/// Frames rendered per displayed frame during an animation export.
const FRAMES_PER_TICK: usize = 3;

impl EsquisseApp {
    /// Renders the model seen by `cam` at `size` pixels; `ppp` scales line widths.
    pub fn render_rgba(
        &mut self,
        rs: &egui_wgpu::RenderState,
        cam: &Camera,
        size: [u32; 2],
        ppp: f32,
    ) -> Option<(u32, u32, Vec<u8>)> {
        let lighting = self.lighting();
        let (env, _) = crate::environments::active(&self.doc.model, true);
        let v = self.view.as_mut()?;
        v.set_environment(rs, env);
        v.set_grid(crate::model_grid::view_of(&self.doc.model, self.style.modeling_grid));
        v.render(rs, cam, size, ppp, &self.style, &lighting);
        v.read_pixels(rs)
    }

    /// Advances the exports in progress.
    pub fn step_offscreen(&mut self, ctx: &egui::Context, rs: &egui_wgpu::RenderState) {
        self.step_animation(ctx, rs);
        self.step_print(ctx, rs);
        self.step_export2d(ctx, rs);
        self.step_ruby_images(rs);
        self.step_def_thumbnails(rs);
    }

    fn step_print(&mut self, ctx: &egui::Context, rs: &egui_wgpu::RenderState) {
        let Some(job) = self.ui_state.extra.jobs.print.take() else { return };
        let layout = job.options.layout(self.cam.aspect());
        let cam = job.options.camera(&self.cam, &layout);
        let ppp = job.options.dpi as f32 / 96.0;
        let msg = match self.render_rgba(rs, &cam, [layout.px.0, layout.px.1], ppp) {
            Some((w, h, px)) => crate::print::deliver(&job, &layout, w, h, &px, &self.doc.title()),
            None => "Le rendu de la page a échoué".to_string(),
        };
        self.say(ctx, msg);
    }

    fn set_layer_visibility(&mut self, visible: impl Fn(usize) -> Option<bool>) {
        let mut changed = false;
        for (k, l) in self.doc.model.layers.iter_mut().enumerate() {
            if let Some(v) = visible(k) {
                changed |= l.visible != v;
                l.visible = v;
            }
        }
        if changed {
            self.doc.rev += 1;
            self.doc.model_rev += 1;
        }
    }

    fn step_animation(&mut self, ctx: &egui::Context, rs: &egui_wgpu::RenderState) {
        let Some(mut job) = self.ui_state.extra.jobs.anim.take() else { return };
        let mut error = None;
        let mut budget = FRAMES_PER_TICK;
        while budget > 0 && job.next < job.plan.len() && !job.cancel && error.is_none() {
            let spec = job.plan[job.next];
            // The destination scene's tags first (the mesh is rebuilt before the next tick).
            if job.layers_scene != Some(spec.to) {
                job.layers_scene = Some(spec.to);
                if let Some(h) = self.doc.model.scenes.get(spec.to).and_then(|s| s.hidden_layers.clone()) {
                    self.set_layer_visibility(|k| Some(!h.contains(&k)));
                    break;
                }
            }
            let cam = crate::export_anim::camera_for(
                &self.doc.model.scenes,
                &self.cam,
                &spec,
                job.options.width,
                job.options.height,
            );
            match self.render_rgba(rs, &cam, [job.options.width, job.options.height], 1.0) {
                Some((w, h, px)) => {
                    if let Err(e) = job.sink.push(w, h, &px) {
                        error = Some(e);
                    }
                }
                None => error = Some("le rendu hors écran a échoué".into()),
            }
            job.next += 1;
            budget -= 1;
        }
        if error.is_none() && !job.cancel && job.next < job.plan.len() {
            self.ui_state.extra.jobs.anim = Some(job);
            ctx.request_repaint();
            return;
        }
        // Finished, cancelled or failed.
        let saved = std::mem::take(&mut job.saved_layers);
        self.set_layer_visibility(|k| saved.get(k).copied());
        let path = job.path.clone();
        let sequence = job.sink.is_sequence();
        let msg = if job.cancel {
            job.sink.abort();
            "Export de l'animation annulé".to_string()
        } else if let Some(e) = error {
            job.sink.abort();
            format!("Échec de l'export de l'animation : {e}")
        } else {
            match job.sink.finish() {
                Ok(()) if sequence => {
                    format!("Animation exportée en images PNG : {}_0001.png…", path.with_extension("").display())
                }
                Ok(()) => format!("Animation exportée : {}", path.display()),
                Err(e) => format!("Échec de l'export de l'animation : {e}"),
            }
        };
        self.say(ctx, msg);
    }
}
