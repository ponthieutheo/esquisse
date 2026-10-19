//! Styles › Faces › « Qualité de transparence: » (`TransparencySort`), as SketchUp 2026 draws
//! its two choices (`CStylesEditFacePage`, `SlugTransparentDrawer`):
//!
//! - **Rapide** (« Faster », values 0 and 1): order-independent transparency in one pass, the
//!   transparent faces added into a colour accumulator and a coverage product
//!   (`instanced_face_transparency_capture_shader`, `face_transparency_resolve_shader`,
//!   « Transparency Color Accumulator Texture »), then resolved over the opaque image. The
//!   result does not depend on the order of the faces; overlapping colours are averaged, as
//!   SketchUp's « Rapide » shows them, the nearer face not winning. X-ray shows at 50 % then,
//!   whatever its slider says.
//! - **Meilleure** (« Nicer », value 2): the faces composited from back to front, each over
//!   what is behind it (SketchUp peels the depth layers, `instanced_face_transparency_peel_shader`,
//!   « Depth Peel Render Pass »; Esquisse sorts the transparent triangles from the eye, which
//!   gives the same image where faces do not cross). X-ray takes its slider's opacity, and the
//!   slider can be moved only then.
//!
//! The weights of SketchUp's capture shader are compiled code (not readable): every face
//! counts by its opacity alone.

use eframe::egui_wgpu::{self, wgpu};

use crate::scene::FaceVertex;

/// Whether the style draws transparency the « Meilleure » way.
pub fn nicer(sort: i64) -> bool {
    sort >= 2
}

/// The opacity X-ray gives the faces: the slider's with « Meilleure », 50 % with « Rapide ».
pub fn xray_opacity(st: &crate::style::ViewStyle) -> f64 {
    if nicer(st.transparency_sort) { st.xray_opacity } else { 0.5 }
}

/// The middle of a triangle (as `back_to_front` sorts by it).
fn centre(t: &[FaceVertex; 3]) -> [f32; 3] {
    [0, 1, 2].map(|k| (t[0].pos[k] + t[1].pos[k] + t[2].pos[k]) / 3.0)
}

fn sort_key(c: [f32; 3], eye: [f32; 3], fwd: [f32; 3], perspective: bool) -> f32 {
    let d = [c[0] - eye[0], c[1] - eye[1], c[2] - eye[2]];
    if perspective { d[0] * d[0] + d[1] * d[1] + d[2] * d[2] } else { d[0] * fwd[0] + d[1] * fwd[1] + d[2] * fwd[2] }
}

/// `back_to_front` made again at each image of an orbit: the middles of the triangles kept
/// from when they were set, the lists reused. Same order: the farthest first, triangles
/// as far as each other in the order they were built.
#[derive(Default)]
pub struct Sorter {
    centres: Vec<[f32; 3]>,
    keyed: Vec<(f32, u32)>,
    out: Vec<FaceVertex>,
}

impl Sorter {
    pub fn set(&mut self, verts: &[FaceVertex]) {
        self.centres = verts.as_chunks::<3>().0.iter().map(centre).collect();
    }

    pub fn sort(&mut self, verts: &[FaceVertex], eye: [f32; 3], fwd: [f32; 3], perspective: bool) -> &[FaceVertex] {
        self.keyed.clear();
        self.keyed
            .extend(self.centres.iter().enumerate().map(|(i, c)| (sort_key(*c, eye, fwd, perspective), i as u32)));
        self.keyed.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        self.out.clear();
        self.out
            .extend(self.keyed.iter().flat_map(|(_, i)| verts[*i as usize * 3..*i as usize * 3 + 3].iter().copied()));
        &self.out
    }
}

/// The transparent triangles from the farthest to the nearest (« Meilleure »): by the
/// distance of their middle from the eye (in perspective) or along the view (parallel).
#[cfg_attr(not(test), allow(dead_code))]
pub fn back_to_front(verts: &[FaceVertex], eye: [f32; 3], fwd: [f32; 3], perspective: bool) -> Vec<FaceVertex> {
    let (tris, _) = verts.as_chunks::<3>();
    let mut keyed: Vec<(f32, usize)> = tris
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let c = [0, 1, 2].map(|k| (t[0].pos[k] + t[1].pos[k] + t[2].pos[k]) / 3.0);
            let d = [c[0] - eye[0], c[1] - eye[1], c[2] - eye[2]];
            let key = if perspective {
                d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
            } else {
                d[0] * fwd[0] + d[1] * fwd[1] + d[2] * fwd[2]
            };
            (key, i)
        })
        .collect();
    keyed.sort_by(|a, b| b.0.total_cmp(&a.0));
    keyed.iter().flat_map(|(_, i)| verts[i * 3..i * 3 + 3].iter().copied()).collect()
}

/// The accumulation targets of « Rapide » and the pass that resolves them over the image.
pub struct Oit {
    size: [u32; 2],
    samples: u32,
    accum: wgpu::TextureView,
    reveal: wgpu::TextureView,
    /// Single-sample copies the resolve reads (the targets themselves when not multisampled).
    accum_read: wgpu::TextureView,
    reveal_read: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

pub const ACCUM: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const REVEAL: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

const RESOLVE_SHADER: &str = r#"
@group(0) @binding(0) var accum: texture_2d<f32>;
@group(0) @binding(1) var reveal: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let q = vec2<i32>(p.xy);
    let a = textureLoad(accum, q, 0);
    let r = textureLoad(reveal, q, 0).r;
    if (a.a <= 0.00001) {
        discard;
    }
    // The average colour of the faces over this pixel, covering 1 - product(1 - alpha).
    return vec4<f32>(a.rgb / a.a, 1.0 - r);
}
"#;

/// The pipelines of « Rapide »: the faces into the accumulators (made by the 3D view with
/// its shader), and the resolve.
pub struct OitPipes {
    pub resolve: wgpu::RenderPipeline,
    pub layout: wgpu::BindGroupLayout,
}

/// The faces into the accumulators: the view's face shader (`fs_face_oit`), its depth test
/// against the opaque faces, no depth written.
#[allow(clippy::too_many_arguments)]
pub fn face_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    attrs: &[wgpu::VertexAttribute],
    stride: usize,
    bias: wgpu::DepthBiasState,
    samples: u32,
    depth: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let targets = OitPipes::face_targets();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("transparence rapide : faces"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_face"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: stride as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: attrs,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
            stencil: Default::default(),
            bias,
        }),
        multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_face_oit"),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl OitPipes {
    pub fn new(
        rs: &egui_wgpu::RenderState,
        color: wgpu::TextureFormat,
        depth: wgpu::TextureFormat,
        samples: u32,
    ) -> OitPipes {
        let device = &rs.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("transparence rapide"),
            source: wgpu::ShaderSource::Wgsl(RESOLVE_SHADER.into()),
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("transparence rapide"),
            entries: &[tex(0), tex(1)],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("transparence rapide"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let resolve = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("transparence rapide"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            // In the pass that also draws the watermarks over the model (they test depth).
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
        OitPipes { resolve, layout }
    }

    /// The blends of the faces' two outputs: colour × alpha and alpha added up, the coverage
    /// multiplied by 1 - alpha.
    pub fn face_targets() -> [Option<wgpu::ColorTargetState>; 2] {
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let product = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::OneMinusSrc,
            operation: wgpu::BlendOperation::Add,
        };
        [
            Some(wgpu::ColorTargetState {
                format: ACCUM,
                blend: Some(wgpu::BlendState { color: add, alpha: add }),
                write_mask: wgpu::ColorWrites::ALL,
            }),
            Some(wgpu::ColorTargetState {
                format: REVEAL,
                blend: Some(wgpu::BlendState { color: product, alpha: product }),
                write_mask: wgpu::ColorWrites::ALL,
            }),
        ]
    }
}

impl Oit {
    /// The targets for a view of `size` with `samples` per pixel (made again when either changes).
    pub fn ensure(slot: &mut Option<Oit>, rs: &egui_wgpu::RenderState, pipes: &OitPipes, size: [u32; 2], samples: u32) {
        if slot.as_ref().is_some_and(|o| o.size == size && o.samples == samples) {
            return;
        }
        let device = &rs.device;
        let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let make = |label, format, n: u32, read: bool| {
            let usage = if read {
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING
            } else {
                wgpu::TextureUsages::RENDER_ATTACHMENT
            };
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: n,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let ms = samples > 1;
        let accum_read = make("accumulateur", ACCUM, 1, true);
        let reveal_read = make("couverture", REVEAL, 1, true);
        let (accum, reveal) = if ms {
            (make("accumulateur msaa", ACCUM, samples, false), make("couverture msaa", REVEAL, samples, false))
        } else {
            (accum_read.clone(), reveal_read.clone())
        };
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("transparence rapide"),
            layout: &pipes.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&accum_read) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&reveal_read) },
            ],
        });
        *slot = Some(Oit { size, samples, accum, reveal, accum_read, reveal_read, bind });
    }

    /// The two colour attachments the transparent faces are drawn into (cleared: nothing
    /// accumulated, everything uncovered).
    pub fn attachments(&self) -> [Option<wgpu::RenderPassColorAttachment<'_>>; 2] {
        let ms = self.samples > 1;
        [
            Some(wgpu::RenderPassColorAttachment {
                view: &self.accum,
                depth_slice: None,
                resolve_target: ms.then_some(&self.accum_read),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            }),
            Some(wgpu::RenderPassColorAttachment {
                view: &self.reveal,
                depth_slice: None,
                resolve_target: ms.then_some(&self.reveal_read),
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
            }),
        ]
    }

    pub fn bind(&self) -> &wgpu::BindGroup {
        &self.bind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sort made again at each image gives the order of the first sort, triangles as
    /// far as each other included, in perspective and in parallel projection.
    #[test]
    fn the_sorter_orders_as_back_to_front() {
        let mut verts = Vec::new();
        for i in 0..600u32 {
            // Many triangles at the same distance (same middle), others scattered.
            let c = if i % 3 == 0 {
                [10.0, 20.0, 30.0]
            } else {
                [(i * 37 % 101) as f32, (i * 11 % 67) as f32, (i % 13) as f32]
            };
            for k in 0..3 {
                let mut v =
                    FaceVertex::plain(esq_core::glam::DVec3::ZERO, esq_core::glam::DVec3::Z, [1.0; 4], [1.0; 4], i);
                v.pos = [c[0] + [1.0, -1.0, 0.0][k], c[1] + [0.0, 0.0, 0.0][k], c[2] + [0.0, 0.0, 0.0][k]];
                verts.push(v);
            }
        }
        let mut s = Sorter::default();
        s.set(&verts);
        for (eye, fwd, persp) in
            [([0.0, -50.0, 40.0], [0.0, 1.0, -0.5], true), ([5.0, 5.0, 5.0], [0.3, 0.4, -0.866], false)]
        {
            let old = back_to_front(&verts, eye, fwd, persp);
            let new = s.sort(&verts, eye, fwd, persp);
            assert!(bytemuck::cast_slice::<FaceVertex, u8>(&old) == bytemuck::cast_slice::<FaceVertex, u8>(new));
        }
    }
}
