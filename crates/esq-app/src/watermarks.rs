//! Watermarks of the style (Styles › Edit › Watermark): images drawn behind the model
//! (background) or over it (overlay), stretched, tiled or placed at one of nine places, with
//! an opacity, and optionally as a mask (dark parts in the background colour, as SketchUp's
//! watermark stage draws them). Drawn on the GPU within the 3D view, so that exported
//! images have them too.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{self, RichText};
use eframe::egui_wgpu::{self, wgpu};

use crate::app::EsquisseApp;
use crate::style::{ViewStyle, Watermark, WmLayout};
use crate::textures::Texels;

/// Where a watermark goes on a view of `view` pixels: a rectangle (pixels from the top left:
/// x0, y0, x1, y1) and the texture coordinates of its corners (scale, offset).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub rect: [f32; 4],
    pub uv_scale: [f32; 2],
    pub uv_offset: [f32; 2],
}

/// How a watermark of `img` pixels covers a view of `view` pixels (`ppp`: pixels per point).
pub fn placement(w: &Watermark, img: [u32; 2], view: [f32; 2], ppp: f32) -> Placement {
    let (iw, ih) = (img[0].max(1) as f32, img[1].max(1) as f32);
    let [vw, vh] = view;
    let full = Placement { rect: [0.0, 0.0, vw, vh], uv_scale: [1.0, 1.0], uv_offset: [0.0, 0.0] };
    match w.layout {
        WmLayout::Stretched if w.keep_aspect => {
            let k = (vw / iw).min(vh / ih);
            let (sw, sh) = (iw * k, ih * k);
            let (x, y) = ((vw - sw) / 2.0, (vh - sh) / 2.0);
            Placement { rect: [x, y, x + sw, y + sh], ..full }
        }
        WmLayout::Stretched => full,
        WmLayout::Tiled => {
            let s = (w.scale as f32).max(0.01) * ppp;
            Placement { uv_scale: [vw / (iw * s), vh / (ih * s)], ..full }
        }
        WmLayout::Positioned => {
            let s = (w.scale as f32).max(0.01) * ppp;
            let (sw, sh) = (iw * s, ih * s);
            let along = |i: u8, room: f32, size: f32| match i {
                0 => 0.0,
                1 => (room - size) / 2.0,
                _ => room - size,
            };
            let (x, y) = (along(w.position % 3, vw, sw), along(w.position.min(8) / 3, vh, sh));
            Placement { rect: [x, y, x + sw, y + sh], ..full }
        }
    }
}

const SHADER: &str = r#"
struct Wm {
    // the rectangle in normalized device coordinates: x0, y0 (top left), x1, y1
    rect: vec4<f32>,
    // texture coordinates: xy scale, zw offset
    uv: vec4<f32>,
    // mask colour (linear), w: opacity
    color: vec4<f32>,
    // x: 1 for a mask
    flags: vec4<f32>,
};
@group(0) @binding(0) var<uniform> wm: Wm;
@group(0) @binding(1) var img: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_wm(@builtin(vertex_index) i: u32) -> VOut {
    var c = array<vec2<f32>, 6>(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0));
    let k = c[i];
    var o: VOut;
    o.pos = vec4(mix(wm.rect.x, wm.rect.z, k.x), mix(wm.rect.y, wm.rect.w, k.y), 0.0, 1.0);
    o.uv = k * wm.uv.xy + wm.uv.zw;
    return o;
}

@fragment
fn fs_wm(i: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(img, samp, i.uv);
    if (wm.flags.x > 0.5) {
        // A mask: the darker the image, the more of the background colour.
        let l = dot(t.rgb, vec3(0.2126, 0.7152, 0.0722));
        return vec4(wm.color.rgb, (1.0 - l) * t.a * wm.color.w);
    }
    return vec4(t.rgb, t.a * wm.color.w);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct WmUniform {
    rect: [f32; 4],
    uv: [f32; 4],
    color: [f32; 4],
    flags: [f32; 4],
}

/// The GPU side: one pipeline, the images as textures (kept while they are used).
pub struct WmRenderer {
    pipe: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    images: HashMap<usize, (Arc<Texels>, wgpu::TextureView)>,
    /// This frame's watermarks: behind the model or over it, and their bindings.
    frame: Vec<(bool, wgpu::BindGroup)>,
}

impl WmRenderer {
    pub fn new(
        rs: &egui_wgpu::RenderState,
        color: wgpu::TextureFormat,
        depth: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
        let device = &rs.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("watermarks"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("watermark"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("watermark"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("watermark"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_wm"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_wm"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("watermark"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        WmRenderer { pipe, bgl, sampler, images: HashMap::new(), frame: Vec::new() }
    }

    fn image(&mut self, rs: &egui_wgpu::RenderState, t: &Arc<Texels>) -> wgpu::TextureView {
        let key = Arc::as_ptr(t) as usize;
        if let Some((_, v)) = self.images.get(&key) {
            return v.clone();
        }
        // The picture comes from a style or a model: brought within what the card takes.
        let (w, h, rgba) = crate::gpu_img::fit(t.w, t.h, &t.rgba, crate::gpu_img::device_max_side(&rs.device));
        let tex = rs.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("watermark image"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        rs.queue.write_texture(
            tex.as_image_copy(),
            &rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let view = tex.create_view(&Default::default());
        self.images.insert(key, (t.clone(), view.clone()));
        view
    }

    /// Prepares this frame's watermarks for a view of `size` pixels.
    pub fn prepare(&mut self, rs: &egui_wgpu::RenderState, style: &ViewStyle, size: [u32; 2], ppp: f32) {
        self.frame.clear();
        if !style.watermarks {
            return;
        }
        let bg = style.bg_color.map(|c| {
            let c = c as f32 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        });
        let mut used = Vec::new();
        for w in &style.watermark_list {
            let Some(t) = crate::textures::get(&w.image, None) else { continue };
            used.push(Arc::as_ptr(&t) as usize);
            let view = self.image(rs, &t);
            let p = placement(w, [t.w, t.h], [size[0] as f32, size[1] as f32], ppp);
            let ndc = |x: f32, y: f32| [x / size[0] as f32 * 2.0 - 1.0, 1.0 - y / size[1] as f32 * 2.0];
            let (a, b) = (ndc(p.rect[0], p.rect[1]), ndc(p.rect[2], p.rect[3]));
            let u = WmUniform {
                rect: [a[0], a[1], b[0], b[1]],
                uv: [p.uv_scale[0], p.uv_scale[1], p.uv_offset[0], p.uv_offset[1]],
                color: [bg[0], bg[1], bg[2], w.blend.clamp(0.0, 1.0) as f32],
                flags: [w.mask as u8 as f32, 0.0, 0.0, 0.0],
            };
            let buf = wgpu::util::DeviceExt::create_buffer_init(
                &rs.device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("watermark"),
                    contents: bytemuck::bytes_of(&u),
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let bind = rs.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("watermark"),
                layout: &self.bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            self.frame.push((w.background, bind));
        }
        self.images.retain(|k, _| used.contains(k));
    }

    /// Draws the watermarks behind the model (`background`) or over it, in their order.
    pub fn draw(&self, pass: &mut wgpu::RenderPass, background: bool) {
        for (b, bind) in &self.frame {
            if *b == background {
                pass.set_pipeline(&self.pipe);
                pass.set_bind_group(0, bind, &[]);
                pass.draw(0..6, 0..1);
            }
        }
    }
}

// ------------------------------------------------------------------------------ editing

fn layout_label(l: WmLayout) -> &'static str {
    match l {
        WmLayout::Stretched => "Étiré pour couvrir l'écran",
        WmLayout::Tiled => "En mosaïque sur tout l'écran",
        WmLayout::Positioned => "Positionné sur l'écran",
    }
}

/// A watermark of an image file, as SketchUp's Create Watermark makes it.
pub fn new_watermark(name: &str, bytes: Vec<u8>) -> Watermark {
    Watermark {
        name: name.to_string(),
        image: esq_core::photo::Blob(Arc::new(bytes)),
        background: true,
        blend: 0.5,
        mask: false,
        layout: WmLayout::Stretched,
        keep_aspect: true,
        scale: 1.0,
        position: 4,
        skp: Default::default(),
    }
}

/// Styles › Edit › Watermark.
pub fn edit(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let id = egui::Id::unique("watermark_selected");
    let mut sel: Option<usize> = ui.data(|d| d.get_temp(id)).unwrap_or(None);
    let s = &mut app.style;
    ui.checkbox(&mut s.watermarks, "Afficher les filigranes");
    let n = s.watermark_list.len();
    if sel.is_some_and(|i| i >= n) {
        sel = None;
    }
    // The list as SketchUp shows it: the backgrounds, the model, the overlays.
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let mut row = |ui: &mut egui::Ui, i: usize, w: &Watermark| {
            if ui.selectable_label(sel == Some(i), &w.name).clicked() {
                sel = Some(i);
            }
        };
        ui.label(RichText::new("Arrière-plan").small().weak());
        for (i, w) in s.watermark_list.iter().enumerate().filter(|(_, w)| w.background) {
            row(ui, i, w);
        }
        ui.label(RichText::new("Espace du modèle").italics());
        ui.label(RichText::new("Incrustation").small().weak());
        for (i, w) in s.watermark_list.iter().enumerate().filter(|(_, w)| !w.background) {
            row(ui, i, w);
        }
    });
    ui.horizontal(|ui| {
        if ui.button("+").on_hover_text("Ajouter un filigrane").clicked()
            && let Some(p) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg"]).pick_file()
            && let Ok(bytes) = std::fs::read(&p)
        {
            let name = p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Filigrane".into());
            let taken: Vec<String> = s.watermark_list.iter().map(|w| w.name.clone()).collect();
            s.watermark_list.push(new_watermark(&esq_core::model::Model::unique_name(&name, &taken), bytes));
            sel = Some(s.watermark_list.len() - 1);
        }
        if ui.add_enabled(sel.is_some(), egui::Button::new("−")).on_hover_text("Supprimer le filigrane").clicked()
            && let Some(i) = sel
        {
            s.watermark_list.remove(i);
            sel = None;
        }
        if ui
            .add_enabled(sel.is_some_and(|i| i > 0), egui::Button::new("▲"))
            .on_hover_text("Déplacer le filigrane vers le haut")
            .clicked()
            && let Some(i) = sel
        {
            s.watermark_list.swap(i, i - 1);
            sel = Some(i - 1);
        }
        if ui
            .add_enabled(sel.is_some_and(|i| i + 1 < n), egui::Button::new("▼"))
            .on_hover_text("Déplacer le filigrane vers le bas")
            .clicked()
            && let Some(i) = sel
        {
            s.watermark_list.swap(i, i + 1);
            sel = Some(i + 1);
        }
    });
    if let Some(i) = sel
        && let Some(w) = s.watermark_list.get_mut(i)
    {
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Nom");
            ui.text_edit_singleline(&mut w.name);
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut w.background, true, "Arrière-plan");
            ui.radio_value(&mut w.background, false, "Incrustation");
        });
        ui.checkbox(&mut w.mask, "Créer un masque")
            .on_hover_text("Vous pouvez utiliser la luminosité de la couleur pour créer un filigrane de masquage.");
        let mut blend = w.blend * 100.0;
        if ui.add(egui::Slider::new(&mut blend, 0.0..=100.0).fixed_decimals(0).text("Fusionner")).changed() {
            w.blend = blend / 100.0;
        }
        egui::ComboBox::from_id_salt("wm_layout").selected_text(layout_label(w.layout)).width(220.0).show_ui(
            ui,
            |ui| {
                for l in [WmLayout::Stretched, WmLayout::Tiled, WmLayout::Positioned] {
                    ui.selectable_value(&mut w.layout, l, layout_label(l));
                }
            },
        );
        match w.layout {
            WmLayout::Stretched => {
                ui.checkbox(&mut w.keep_aspect, "Verrouiller les proportions");
            }
            WmLayout::Tiled | WmLayout::Positioned => {
                ui.add(egui::Slider::new(&mut w.scale, 0.05..=4.0).logarithmic(true).text("Échelle"));
            }
        }
        if w.layout == WmLayout::Positioned {
            egui::Grid::new("wm_position").show(ui, |ui| {
                for k in 0..9u8 {
                    if ui.radio(w.position == k, "").clicked() {
                        w.position = k;
                    }
                    if k % 3 == 2 {
                        ui.end_row();
                    }
                }
            });
        }
    }
    ui.data_mut(|d| d.insert_temp(id, sel));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wm(layout: WmLayout) -> Watermark {
        Watermark { layout, ..new_watermark("w", Vec::new()) }
    }

    #[test]
    fn watermarks_are_placed_like_sketchup_places_them() {
        let view = [800.0, 400.0];
        assert_eq!(placement(&wm(WmLayout::Stretched), [100, 100], view, 1.0).rect, [200.0, 0.0, 600.0, 400.0]);
        let free = Watermark { keep_aspect: false, ..wm(WmLayout::Stretched) };
        assert_eq!(placement(&free, [100, 100], view, 1.0).rect, [0.0, 0.0, 800.0, 400.0]);
        let tiled = Watermark { scale: 0.5, ..wm(WmLayout::Tiled) };
        assert_eq!(placement(&tiled, [100, 50], view, 1.0).uv_scale, [16.0, 16.0]);
        let corner = Watermark { position: 8, ..wm(WmLayout::Positioned) };
        assert_eq!(placement(&corner, [100, 50], view, 2.0).rect, [600.0, 300.0, 800.0, 400.0]);
        let top_mid = Watermark { position: 1, ..wm(WmLayout::Positioned) };
        assert_eq!(placement(&top_mid, [100, 50], view, 1.0).rect, [350.0, 0.0, 450.0, 50.0]);
    }

    #[test]
    fn shader_is_valid_wgsl() {
        let module = naga::front::wgsl::parse_str(SHADER).unwrap_or_else(|e| panic!("{e}"));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
}
