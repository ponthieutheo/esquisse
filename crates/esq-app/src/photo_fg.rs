//! Styles › Modeling › Match Photo › « Photo de premier plan » (`PhotomatchDrawOverlay`,
//! `PhotomatchOverlayOpacity`; SketchUp's label in `StylesEditModelingPage`): in the view of a
//! matched photo, the photo is drawn again over the model's faces at that opacity, the edges
//! staying on top — modelling over a photo, the faces show the photo through them. Where no
//! face is, the background photo shows (« Photo d'arrière-plan »). SketchUp's default style
//! (`00Default Style.style`) has it ticked at 80 %; « Reversed Photo Modeling Style » shows
//! the photo opaque over the faces (100 %) and dims the background one (80 %).
//!
//! Drawn by the 3D view, after its opaque faces and before its edges, as one triangle over the
//! whole view: the depth test keeps the pixels where a face was drawn.

use eframe::egui_wgpu::{self, wgpu};
use wgpu::util::DeviceExt;

const SHADER: &str = r#"
struct Overlay {
    // The photo's rectangle in the view, in pixels (x0, y0, x1, y1).
    rect: vec4<f32>,
    // x: opacity.
    opacity: vec4<f32>,
};
@group(0) @binding(0) var<uniform> o: Overlay;
@group(0) @binding(1) var photo: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    // Depth 0: the far plane of the reversed depth; "less" passes where a face is nearer.
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = (p.xy - o.rect.xy) / (o.rect.zw - o.rect.xy);
    if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) {
        discard;
    }
    let c = textureSample(photo, samp, uv);
    return vec4<f32>(c.rgb, o.opacity.x);
}
"#;

/// A photo's texture and its size.
type Uploaded = (wgpu::TextureView, [u32; 2]);

/// The pipeline (one per 3D view: it must match its targets).
pub struct Pipe {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The photo uploaded (its bytes' address, `None` when it could not be decoded), its
    /// texture and the texture's size.
    photo: Option<(usize, Option<Uploaded>)>,
    bind: Option<wgpu::BindGroup>,
}

/// What is drawn this frame.
#[derive(Clone)]
pub struct Overlay {
    pub image: esq_core::photo::Blob,
    /// The photo's rectangle in the view, in physical pixels.
    pub rect: [f32; 4],
    pub opacity: f32,
}

/// The overlay of the photo shown this frame (`EsquisseApp::photo_shown`: the photo and its
/// rectangle in points), when the style asks for it.
pub fn overlay(
    st: &crate::style::ViewStyle,
    shown: Option<&(esq_core::photo::Blob, [f32; 4])>,
    ppp: f32,
) -> Option<Overlay> {
    let (image, rect) = shown.filter(|_| st.photo_fg)?;
    Some(Overlay {
        image: image.clone(),
        rect: rect.map(|x| x * ppp),
        opacity: st.photo_fg_opacity.clamp(0.0, 1.0) as f32,
    })
}

impl Pipe {
    pub fn new(
        rs: &egui_wgpu::RenderState,
        color: wgpu::TextureFormat,
        depth: wgpu::TextureFormat,
        samples: u32,
    ) -> Pipe {
        let device = &rs.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("photo de premier plan"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("photo de premier plan"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
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
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("photo de premier plan"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("photo de premier plan"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
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
            label: Some("photo de premier plan"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Pipe { pipeline, layout, sampler, photo: None, bind: None }
    }

    /// Tests: the size of the texture made of the photo, if one could be.
    #[cfg(test)]
    pub fn texture_size(&self) -> Option<[u32; 2]> {
        self.photo.as_ref().and_then(|(_, t)| t.as_ref()).map(|(_, s)| *s)
    }

    /// Prepares this frame's overlay (`None`: nothing drawn). The photo is decoded and
    /// uploaded once (again only when it is another photo). The photo comes from the model:
    /// its decoding is bounded (`photo_img::decode` refuses what its header makes too big),
    /// its texture reduced, proportions kept, to 4 096 pixels and to what the device takes; a
    /// photo that cannot be decoded is not drawn (and not decoded again at each frame), the
    /// view shows the rest.
    pub fn prepare(&mut self, rs: &egui_wgpu::RenderState, o: Option<&Overlay>) {
        let Some(o) = o.filter(|o| o.opacity > 0.0) else {
            self.bind = None;
            return;
        };
        let key = std::sync::Arc::as_ptr(&o.image.0) as usize;
        if self.photo.as_ref().is_none_or(|(k, _)| *k != key) {
            self.photo = Some((key, None));
            let Ok(img) = crate::photo_img::decode(&o.image.0) else {
                self.bind = None;
                return;
            };
            // Rule 15 of docs/securite.md: within the card's side (`gpu_img`), 4 096 for a photo.
            let rgba = img.to_rgba8();
            let side = crate::photo_img::MAX_TEXTURE_SIDE.min(crate::gpu_img::device_max_side(&rs.device));
            let (w, h, px) = crate::gpu_img::fit(rgba.width(), rgba.height(), rgba.as_raw(), side);
            let tex = rs.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("photo de premier plan"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            rs.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &px,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
            self.photo = Some((key, Some((tex.create_view(&Default::default()), [w, h]))));
        }
        let Some((_, Some((view, _)))) = &self.photo else {
            self.bind = None;
            return;
        };
        let u: [f32; 8] = [o.rect[0], o.rect[1], o.rect[2], o.rect[3], o.opacity.clamp(0.0, 1.0), 0.0, 0.0, 0.0];
        let buf = rs.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("photo de premier plan"),
            contents: bytemuck::cast_slice(&u),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        self.bind = Some(rs.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("photo de premier plan"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        }));
    }

    /// Draws the overlay prepared, inside the view's pass (after its opaque faces).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(b) = &self.bind {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, b, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
