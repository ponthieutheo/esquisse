//! Offscreen wgpu renderer for the 3D view; the result is shown as an egui image.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use wgpu::util::DeviceExt;

use esq_core::glam::DVec3;
use esq_core::glam::dcamera::rh::{proj::directx, view};

use crate::camera::Camera;
use crate::scene::{FaceVertex, LineInst, MAX_CLIPS, SceneMesh};
use crate::style::{FaceStyle, ViewStyle};
use crate::textures::Texels;

const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// What Preferences › Graphics sets in the renderer (a change rebuilds it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuConfig {
    /// Multisample anti-aliasing: samples per pixel (1 = off).
    pub samples: u32,
    /// Size of the shadow map, in texels (smaller with "Use Faster Shadows").
    pub shadow_size: u32,
}

/// The graphics card, for Preferences › Graphics (known once a view was created).
#[derive(Clone, Debug, Default)]
pub struct GpuInfo {
    pub name: String,
    pub backend: String,
    pub driver: String,
    /// Sample counts the card can draw with.
    pub samples: Vec<u32>,
    pub max_texture: u32,
}

static GPU_INFO: std::sync::RwLock<Option<GpuInfo>> = std::sync::RwLock::new(None);

pub fn gpu_info() -> Option<GpuInfo> {
    GPU_INFO.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Sample counts both the colour and the depth formats support on this device.
fn supported_samples(rs: &egui_wgpu::RenderState) -> Vec<u32> {
    let specific = rs.device.features().contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
    let (c, d) = (rs.adapter.get_texture_format_features(COLOR), rs.adapter.get_texture_format_features(DEPTH));
    [1, 2, 4, 8, 16]
        .into_iter()
        .filter(|n| {
            matches!(n, 1 | 4) || (specific && c.flags.sample_count_supported(*n) && d.flags.sample_count_supported(*n))
        })
        .collect()
}

/// The configuration the preferences ask for, within the sample counts the card can do.
pub fn config_for(supported: &[u32]) -> GpuConfig {
    let (msaa, faster) = crate::settings::with(|s| (s.msaa, s.faster_shadows));
    let want = msaa.max(1);
    let samples = supported.iter().copied().filter(|n| *n <= want).max().unwrap_or(1);
    GpuConfig { samples, shadow_size: if faster { 2048 } else { 4096 } }
}

fn wanted_config(rs: &egui_wgpu::RenderState) -> GpuConfig {
    config_for(&supported_samples(rs))
}

/// True when the preferences changed what a view was built with: it must be rebuilt.
pub fn config_changed(current: GpuConfig) -> bool {
    let supported = gpu_info().map(|g| g.samples).unwrap_or_else(|| vec![1, 4]);
    config_for(&supported) != current
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    light_dir: [f32; 4],
    params: [f32; 4],
    viewport: [f32; 4],
    eye: [f32; 4],
    fwd: [f32; 4],
    fog: [f32; 4],
    bg: [f32; 4],
    /// Section planes: (normal, offset); what lies where `normal · p < offset` is cut away.
    clips: [[f32; 4]; MAX_CLIPS],
    /// World → shadow map (sun's orthographic view).
    light_vp: [[f32; 4]; 4],
    /// xyz: direction towards the sun; w: 1 when faces are lit by the sun.
    sun: [f32; 4],
    /// x: light, y: dark (0–1); z: 1 to shadow faces; w: 1 when shadows are cast.
    shadow: [f32; 4],
    /// Ground shadows: xy centre and z half-size of the ground square, w: 1 when shown.
    ground: [f32; 4],
    /// Clip space → world, to find each pixel's view direction for the sky.
    inv_view_proj: [[f32; 4]; 4],
    /// Sky colour (sRGB, 0–1); w: 1 when the sky is drawn.
    sky: [f32; 4],
    /// Ambient occlusion: radius (mm), intensity, multiplier.
    ao: [f32; 4],
    /// Edge style: extension length, endpoint length (px), endpoint width factor, jitter (px).
    edge: [f32; 4],
    /// Colour the fog fades to (linear); w: 1 when fog is shown.
    fog_color: [f32; 4],
    /// Ground colour (linear) and opacity.
    ground_color: [f32; 4],
    /// Environment: rotation (radians), skydome exposure, reflection exposure; w: 1 skydome,
    /// 2 reflections (bits).
    env: [f32; 4],
    /// Environment panorama: number of levels, level to sample for the sky.
    env_lod: [f32; 4],
    /// Modeling grid (`model_grid.rs`): origin; w bits: 1 shown, 2 major lines, 4 minor
    /// lines, 8 radial.
    grid_origin: [f32; 4],
    /// The plane's first direction, spacing along it (mm).
    grid_u: [f32; 4],
    /// The plane's second direction, spacing along it.
    grid_v: [f32; 4],
    /// Major lines: linear colour, thickness (px).
    grid_major: [f32; 4],
    /// Minor lines: linear colour, thickness (px).
    grid_minor: [f32; 4],
    /// Subdivisions along u and v, dash patterns of the major and minor lines.
    grid_misc: [f32; 4],
    /// The square drawn: centre (grid coordinates), half size; w: spokes of a radial grid.
    grid_quad: [f32; 4],
}

/// How the sun lights the view (Window › Shadows).
#[derive(Clone, Copy, Debug, Default)]
pub struct Lighting {
    /// Direction towards the sun when faces are lit by it (shadows on, or "use sun for
    /// shading"); otherwise the light follows the camera.
    pub sun: Option<DVec3>,
    /// 0–1, as in the Shadows panel.
    pub light: f32,
    pub dark: f32,
    /// View › Shadows.
    pub cast: bool,
    pub on_faces: bool,
    pub on_ground: bool,
}

/// Edge style › Endpoints: the ends of an edge are this many times as thick as the edge.
pub const ENDPOINT_WIDTH: f32 = 3.0;
/// Edge style › Jitter: how far, in points, the extra strokes of an edge stray.
pub const JITTER_PX: f32 = 1.5;

struct Targets {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    resolve: wgpu::TextureView,
    depth: wgpu::TextureView,
    texture: wgpu::Texture,
    /// Ambient occlusion: single-sample depth of the opaque faces, raw and blurred occlusion.
    ao_depth: wgpu::TextureView,
    ao_raw: wgpu::TextureView,
    ao_blur: wgpu::TextureView,
}

#[cfg(test)]
thread_local! {
    /// Tests: items per buffer, instead of what the card's largest buffer holds.
    pub static PART_ITEMS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Vertices (or line instances) on the GPU, in as many buffers as the card's largest buffer
/// size requires (a big model's faces outgrow wgpu's 256 Mo by default): drawn one after the
/// other, in order, they draw what one buffer would.
/// What a shadow map is drawn from (`Viewport3d::shadow_of`).
type ShadowKey = ([u32; 16], [[u32; 4]; MAX_CLIPS], u64);

struct GpuBuf {
    parts: Vec<(wgpu::Buffer, u32)>,
    count: u32,
    /// Bytes per item.
    stride: usize,
}

impl GpuBuf {
    /// `n` items of `data`, cut every multiple of `group` items (3: a triangle stays whole).
    fn new(rs: &egui_wgpu::RenderState, label: &str, data: &[u8], n: usize, group: usize) -> Option<GpuBuf> {
        if n == 0 {
            return None;
        }
        let stride = data.len() / n;
        let max = rs.device.limits().max_buffer_size as usize;
        let per = ((max / stride.max(1)) / group * group).max(group);
        #[cfg(test)]
        let per = PART_ITEMS.with(|p| p.get()).map_or(per, |p| (p / group * group).max(group));
        let parts = data
            .chunks(per * stride)
            .map(|c| {
                let buf = rs.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: c,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                });
                (buf, (c.len() / stride) as u32)
            })
            .collect();
        Some(GpuBuf { parts, count: n as u32, stride })
    }

    /// Replaces the items from `at` on with `data` (as many whole items as it holds).
    fn write(&self, rs: &egui_wgpu::RenderState, mut at: usize, mut data: &[u8]) {
        let mut first = 0;
        for (buf, n) in &self.parts {
            let n = *n as usize;
            if data.is_empty() {
                return;
            }
            if at < first + n {
                let k = (first + n - at).min(data.len() / self.stride);
                let (now, rest) = data.split_at(k * self.stride);
                rs.queue.write_buffer(buf, ((at - first) * self.stride) as u64, now);
                data = rest;
                at += k;
            }
            first += n;
        }
    }

    /// Draws the vertices (one per vertex of the list).
    fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        for (b, n) in &self.parts {
            pass.set_vertex_buffer(0, b.slice(..));
            pass.draw(0..*n, 0..1);
        }
    }

    /// Draws `verts` vertices for each item (instanced lines).
    fn draw_instances<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, verts: u32) {
        for (b, n) in &self.parts {
            pass.set_vertex_buffer(0, b.slice(..));
            pass.draw(0..verts, 0..*n);
        }
    }
}

pub struct Viewport3d {
    face_opaque: wgpu::RenderPipeline,
    face_blend: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    back_lines: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    bgl: wgpu::BindGroupLayout,
    shadow_sampler: wgpu::Sampler,
    targets: Option<Targets>,
    tex_id: Option<egui::TextureId>,
    opaque: Option<GpuBuf>,
    transparent: Option<GpuBuf>,
    /// Styles › Faces › Transparency quality (`oit.rs`): the transparent triangles as built,
    /// the view they were last sorted for (« Meilleure »), and « Rapide »'s pipelines and
    /// targets.
    transparent_cpu: Vec<FaceVertex>,
    sorted_for: Option<[f32; 7]>,
    sorter: crate::oit::Sorter,
    /// Meshes uploaded so far (`uploads`).
    uploads: u64,
    /// Changes of the faces on the GPU (uploads and patches).
    mesh_gen: u64,
    /// What the shadow map was last drawn from: the sun's view, the cut planes, the faces.
    shadow_of: Option<ShadowKey>,
    face_oit: wgpu::RenderPipeline,
    oit_pipes: crate::oit::OitPipes,
    oit: Option<crate::oit::Oit>,
    line_buf: Option<GpuBuf>,
    clips: Vec<[f32; 4]>,
    bounds: Option<(DVec3, DVec3)>,
    shadow_pipe: wgpu::RenderPipeline,
    ground_pipe: wgpu::RenderPipeline,
    shadow_view: wgpu::TextureView,
    shadow_bind: wgpu::BindGroup,
    sky_pipe: wgpu::RenderPipeline,
    /// Match Photo: the background (sky and ground colour) is left transparent so that the
    /// photo painted behind the view shows through.
    pub photo_bg: bool,
    /// Styles › Modeling › Match Photo › « Photo de premier plan » (`photo_fg.rs`): the photo
    /// over the faces this frame.
    pub photo_overlay: Option<crate::photo_fg::Overlay>,
    pub(crate) photo_fg: crate::photo_fg::Pipe,
    /// Ambient occlusion passes (see `fs_ao`).
    ao_pre: wgpu::RenderPipeline,
    ao_gen: wgpu::RenderPipeline,
    ao_blur: wgpu::RenderPipeline,
    post_bgl: wgpu::BindGroupLayout,
    /// 1×1 white occlusion map, bound while ambient occlusion is off.
    white: wgpu::TextureView,
    /// Material textures (one layer each) and the textures they were made from.
    tex_view: wgpu::TextureView,
    tex_sampler: wgpu::Sampler,
    /// Model Info › Rendering › Use Anti-Aliased Textures off: the full-size image only.
    tex_sampler_plain: wgpu::Sampler,
    /// The model's "Use Anti-Aliased Textures" (`Model#mipmapping?`).
    pub mipmapping: bool,
    tex_layers: Vec<Arc<Texels>>,
    /// Anti-aliasing and shadow map size it was built with (Preferences › Graphics).
    pub config: GpuConfig,
    /// Watermarks of the style.
    wm: crate::watermarks::WmRenderer,
    /// The current environment's panorama (black when there is none) and its settings.
    env_view: wgpu::TextureView,
    env_sampler: wgpu::Sampler,
    env_image: Option<Arc<crate::environments::EnvImage>>,
    env: Option<crate::environments::EnvView>,
    /// View › Grid: the modeling grid to draw.
    grid_pipe: wgpu::RenderPipeline,
    grid: Option<crate::model_grid::GridView>,
}

/// The panorama of an environment as a texture with its levels (shared-exponent HDR).
fn env_texture(rs: &egui_wgpu::RenderState, img: Option<&crate::environments::EnvImage>) -> wgpu::TextureView {
    let black = [(1u32, 1u32, vec![0u32])];
    let levels: &[(u32, u32, Vec<u32>)] = img.map_or(&black[..], |i| &i.levels[..]);
    let (w, h) = (levels[0].0, levels[0].1);
    let tex = rs.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("environment"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgb9e5Ufloat,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (mip, (lw, lh, px)) in levels.iter().enumerate() {
        rs.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: mip as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(px),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * lw), rows_per_image: Some(*lh) },
            wgpu::Extent3d { width: *lw, height: *lh, depth_or_array_layers: 1 },
        );
    }
    tex.create_view(&Default::default())
}

/// Side of the texture array's layers: the largest texture's, as a power of two, smaller when
/// there are many textures (memory).
pub fn layer_side(layers: &[Arc<Texels>]) -> u32 {
    let cap = match layers.len() {
        0..=4 => 2048,
        5..=24 => 1024,
        25..=96 => 512,
        _ => 256,
    };
    // Preferences › Graphics › Use maximum texture size: off, textures stay within 1024
    // texels (as in SketchUp); on, twice the usual size, within what the card takes.
    let cap = if crate::settings::with(|s| s.max_texture) {
        (cap * 2).min(gpu_info().map_or(4096, |g| g.max_texture).max(256))
    } else {
        cap.min(1024)
    };
    layers.iter().map(|t| t.w.max(t.h)).max().unwrap_or(1).next_power_of_two().clamp(16, cap)
}

/// The texture array holding `layers` (with their mipmaps); one white layer when empty.
fn texture_array(rs: &egui_wgpu::RenderState, layers: &[Arc<Texels>]) -> wgpu::TextureView {
    let white = Texels { w: 1, h: 1, rgba: vec![255; 4], alpha: false };
    let side = if layers.is_empty() { 1 } else { layer_side(layers) };
    let mips = side.ilog2() + 1;
    let n = layers.len().max(1) as u32;
    let tex = rs.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("material textures"),
        size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: n },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // The texels stay sRGB-encoded, like the colours of the faces.
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let all: Vec<&Texels> = if layers.is_empty() { vec![&white] } else { layers.iter().map(|t| &**t).collect() };
    for (z, t) in all.iter().enumerate() {
        let mut level = crate::textures::resized(t, side);
        let mut s = side;
        for mip in 0..mips {
            rs.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: mip,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: z as u32 },
                    aspect: wgpu::TextureAspect::All,
                },
                &level,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * s), rows_per_image: Some(s) },
                wgpu::Extent3d { width: s, height: s, depth_or_array_layers: 1 },
            );
            if s > 1 {
                level = crate::textures::half(&level, s);
                s /= 2;
            }
        }
    }
    tex.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

/// Nearest and farthest distance, along the view direction, of a bounding box (the range
/// over which the depth cue thins the edges). Without a model, a range around the target.
pub fn depth_range(bounds: Option<(DVec3, DVec3)>, cam: &Camera) -> (f64, f64) {
    let (eye, fwd) = (cam.eye(), cam.forward());
    let Some((a, b)) = bounds else { return (cam.distance * 0.5, cam.distance * 1.5) };
    let (mut near, mut far) = (f64::MAX, f64::MIN);
    for i in 0..8 {
        let p = DVec3::new(
            if i & 1 == 0 { a.x } else { b.x },
            if i & 2 == 0 { a.y } else { b.y },
            if i & 4 == 0 { a.z } else { b.z },
        );
        let d = (p - eye).dot(fwd);
        near = near.min(d);
        far = far.max(d);
    }
    (near.max(0.0), far.max(near + 1.0))
}

fn srgb_to_linear(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

impl Viewport3d {
    pub fn new(rs: &egui_wgpu::RenderState) -> Self {
        let config = wanted_config(rs);
        let a = rs.adapter.get_info();
        *GPU_INFO.write().unwrap_or_else(|e| e.into_inner()) = Some(GpuInfo {
            name: a.name,
            backend: format!("{:?}", a.backend),
            driver: format!("{} {}", a.driver, a.driver_info).trim().to_string(),
            samples: supported_samples(rs),
            max_texture: rs.device.limits().max_texture_dimension_2d,
        });
        let (samples, shadow_size) = (config.samples, config.shadow_size);
        let device = &rs.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("esquisse"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
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
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                // The blurred ambient occlusion map.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // The material textures, one per layer, and how they are sampled.
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // The environment's panorama and its sampler (around: repeat; up: clamp).
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        // Ambient occlusion passes read the depth of the opaque faces and an occlusion map.
        let post_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        // The shadow pass writes the shadow map, so it only sees the uniforms.
        let shadow_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow"),
            bind_group_layouts: &[Some(&shadow_bgl)],
            immediate_size: 0,
        });
        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow map"),
            size: wgpu::Extent3d { width: shadow_size, height: shadow_size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_tex.create_view(&Default::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("esquisse"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let white_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("white"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        rs.queue.write_texture(
            white_tex.as_image_copy(),
            &[255],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1), rows_per_image: Some(1) },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let white = white_tex.create_view(&Default::default());
        let post_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post"),
            bind_group_layouts: &[Some(&shadow_bgl), Some(&post_bgl)],
            immediate_size: 0,
        });
        let shadow_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow globals"),
            layout: &shadow_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });

        let face_attrs = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x4, 4 => Uint32, 5 => Float32x3,
            6 => Float32x3, 7 => Uint32
        ];
        let tex_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("textures"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let tex_sampler_plain = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("textures (sans anticrénelage)"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            lod_max_clamp: 0.0,
            ..Default::default()
        });
        let tex_view = texture_array(rs, &[]);
        let env_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("environment"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let env_view = env_texture(rs, None);
        let line_attrs = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x3, 4 => Float32x4, 5 => Float32, 6 => Uint32
        ];
        let blend = Some(wgpu::BlendState::ALPHA_BLENDING);
        // Faces are pushed back in depth (more when seen at a grazing angle) so that edges
        // lying on them always win. Reversed Z: "back" is a negative bias.
        let face_bias = wgpu::DepthBiasState { constant: -2, slope_scale: -2.0, clamp: 0.0 };
        let make = |entry_vs: &str,
                    entry_fs: &str,
                    attrs: &[wgpu::VertexAttribute],
                    stride: usize,
                    step_mode: wgpu::VertexStepMode,
                    write_depth: bool,
                    compare: wgpu::CompareFunction| {
            let is_face = step_mode == wgpu::VertexStepMode::Vertex;
            let bias = if is_face { face_bias } else { Default::default() };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry_vs),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(entry_vs),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: stride as u64,
                        step_mode,
                        attributes: attrs,
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(write_depth),
                    depth_compare: Some(compare),
                    stencil: Default::default(),
                    bias,
                }),
                multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry_fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let per_vertex = wgpu::VertexStepMode::Vertex;
        let face_size = std::mem::size_of::<FaceVertex>();
        let front = wgpu::CompareFunction::GreaterEqual;
        let face_opaque = make("vs_face", "fs_face", &face_attrs, face_size, per_vertex, true, front);
        let face_blend = make("vs_face", "fs_face", &face_attrs, face_size, per_vertex, false, front);
        let face_oit =
            crate::oit::face_pipeline(device, &layout, &shader, &face_attrs, face_size, face_bias, samples, DEPTH);
        let oit_pipes = crate::oit::OitPipes::new(rs, COLOR, DEPTH, samples);
        let line_size = std::mem::size_of::<LineInst>();
        let per_line = wgpu::VertexStepMode::Instance;
        let lines = make("vs_line", "fs_line", &line_attrs, line_size, per_line, true, front);
        // Reversed Z: what lies behind the faces has a smaller depth.
        let behind = wgpu::CompareFunction::Less;
        let back_lines = make("vs_line", "fs_line_back", &line_attrs, line_size, per_line, false, behind);
        // The sun's view: depth only, with a slope bias against shadow acne.
        let shadow_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow"),
            layout: Some(&shadow_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_shadow"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: face_size as u64,
                    step_mode: per_vertex,
                    attributes: &face_attrs,
                })],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState { constant: 4, slope_scale: 3.0, clamp: 0.0 },
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_shadow"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        });
        // Sky and ground behind everything.
        let sky_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_sky"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_sky"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // Ground shadows: a square on the ground, drawn behind faces lying on it.
        let ground_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ground"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_ground"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(false),
                depth_compare: Some(front),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState { constant: -8, slope_scale: -8.0, clamp: 0.0 },
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_ground"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // The modeling grid: a large square on its plane, blended over the faces, hidden by
        // what stands in front of it (and winning over faces lying on it).
        let grid_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("grid"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_grid"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(false),
                depth_compare: Some(front),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState { constant: 2, slope_scale: 1.0, clamp: 0.0 },
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_grid"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // Ambient occlusion: depth pre-pass of the opaque faces, occlusion, blur.
        let ao_pre = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ao pre-pass"),
            layout: Some(&shadow_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_pre"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: face_size as u64,
                    step_mode: per_vertex,
                    attributes: &face_attrs,
                })],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(front),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_pre"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        });
        let post = |label: &str, fs: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&post_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_sky"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R8Unorm,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let ao_gen = post("ao", "fs_ao");
        let ao_blur = post("ao blur", "fs_ao_blur");
        Viewport3d {
            face_opaque,
            face_blend,
            lines,
            back_lines,
            globals,
            bgl,
            shadow_sampler,
            targets: None,
            tex_id: None,
            opaque: None,
            transparent: None,
            transparent_cpu: Vec::new(),
            sorted_for: None,
            sorter: Default::default(),
            uploads: 0,
            mesh_gen: 0,
            shadow_of: None,
            face_oit,
            oit_pipes,
            oit: None,
            line_buf: None,
            clips: Vec::new(),
            bounds: None,
            shadow_pipe,
            ground_pipe,
            shadow_view,
            shadow_bind,
            sky_pipe,
            photo_bg: false,
            photo_overlay: None,
            photo_fg: crate::photo_fg::Pipe::new(rs, COLOR, DEPTH, samples),
            ao_pre,
            ao_gen,
            ao_blur,
            post_bgl,
            white,
            tex_view,
            tex_sampler,
            tex_sampler_plain,
            mipmapping: true,
            tex_layers: Vec::new(),
            wm: crate::watermarks::WmRenderer::new(rs, COLOR, DEPTH, config.samples),
            env_view,
            env_sampler,
            env_image: None,
            env: None,
            grid_pipe,
            grid: None,
            config,
        }
    }

    /// The modeling grid to draw (View › Grid), or none.
    pub fn set_grid(&mut self, grid: Option<crate::model_grid::GridView>) {
        self.grid = grid;
    }

    /// The grid's part of the globals (see `Globals::grid_origin`…).
    fn grid_globals(&self, cam: &Camera, ppp: f32) -> [[f32; 4]; 7] {
        let Some(g) = &self.grid else { return [[0.0; 4]; 7] };
        let v3 = |v: DVec3| [v.x as f32, v.y as f32, v.z as f32];
        let lin = |c: [u8; 3]| c.map(|c| srgb_to_linear(c) as f32);
        let lines = |l: &Option<crate::model_grid::GridLines>| match l {
            Some(l) => {
                let c = lin(l.color);
                [c[0], c[1], c[2], l.thickness.max(1) as f32 * ppp]
            }
            None => [0.0; 4],
        };
        let bits = 1 | (g.major.is_some() as u32) << 1 | (g.minor.is_some() as u32) << 2 | (g.radial as u32) << 3;
        let dash = |l: &Option<crate::model_grid::GridLines>| {
            l.as_ref().map_or(0, |l| crate::scene::dash_pattern(&l.dash)) as f32
        };
        // The square follows the point of the plane the camera looks at, wide enough to reach
        // the horizon (the lines fade out where they crowd).
        let n = g.u.cross(g.v).normalize_or_zero();
        let (eye, fwd) = (cam.eye(), cam.forward());
        let den = fwd.dot(n);
        let focus = if den.abs() > 1e-6 {
            let t = (g.origin - eye).dot(n) / den;
            if t > 0.0 { eye + fwd * t } else { cam.target }
        } else {
            cam.target
        };
        let rel = focus - g.origin;
        let reach = (eye - focus).length().max(cam.distance) * 60.0;
        let origin = v3(g.origin);
        [
            [origin[0], origin[1], origin[2], bits as f32],
            {
                let u = v3(g.u);
                [u[0], u[1], u[2], g.spacing[0].max(1e-3) as f32]
            },
            {
                let v = v3(g.v);
                [v[0], v[1], v[2], g.spacing[1].max(1e-3) as f32]
            },
            lines(&g.major),
            lines(&g.minor),
            [g.subdivisions[0] as f32, g.subdivisions[1] as f32, dash(&g.major), dash(&g.minor)],
            [rel.dot(g.u) as f32, rel.dot(g.v) as f32, reach as f32, g.spokes as f32],
        ]
    }

    /// The environment to show (Window › Environments), or none.
    pub fn set_environment(&mut self, rs: &egui_wgpu::RenderState, env: Option<crate::environments::EnvView>) {
        let img = env.as_ref().map(|e| e.image.clone());
        let same = match (&img, &self.env_image) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.env_view = env_texture(rs, img.as_deref());
            self.env_image = img;
        }
        self.env = env;
    }

    /// Vertices of the faces (opaque and transparent) and line segments on the GPU, and the
    /// bytes they take.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn gpu_size(&self) -> (u64, u64, u64) {
        let n = |b: &Option<GpuBuf>| b.as_ref().map_or(0, |b| b.count as u64);
        let faces = n(&self.opaque) + n(&self.transparent);
        let lines = n(&self.line_buf);
        let bytes = faces * std::mem::size_of::<FaceVertex>() as u64 + lines * std::mem::size_of::<LineInst>() as u64;
        (faces, lines, bytes)
    }

    /// How many buffers hold the opaque faces, the transparent ones and the lines.
    #[cfg(test)]
    pub fn gpu_parts(&self) -> (usize, usize, usize) {
        let n = |b: &Option<GpuBuf>| b.as_ref().map_or(0, |b| b.parts.len());
        (n(&self.opaque), n(&self.transparent), n(&self.line_buf))
    }

    /// Changes the vertices of some parts of the mesh uploaded last (`scene::update_parts`),
    /// in place: the same lists as `upload` of the whole new mesh would make.
    pub fn patch(&mut self, rs: &egui_wgpu::RenderState, p: &crate::scene::Patch) {
        if !p.opaque.is_empty() {
            self.mesh_gen += 1;
        }
        if let Some(b) = &self.opaque {
            for (at, v) in &p.opaque {
                b.write(rs, *at, bytemuck::cast_slice(v));
            }
        }
        if let Some(b) = &self.line_buf {
            for (at, v) in &p.lines {
                b.write(rs, *at, bytemuck::cast_slice(v));
            }
        }
        if !p.transparent.is_empty() {
            for (at, v) in &p.transparent {
                self.transparent_cpu[*at..*at + v.len()].copy_from_slice(v);
            }
            // Its buffer may hold the triangles sorted (« Meilleure »): made again.
            let t = &self.transparent_cpu;
            self.transparent = GpuBuf::new(rs, "transparent", bytemuck::cast_slice(t), t.len(), 3);
            self.sorter.set(t);
            self.sorted_for = None;
        }
        self.bounds = p.bounds;
    }

    /// How many meshes were uploaded: `patch` changes the last one.
    pub fn uploads(&self) -> u64 {
        self.uploads
    }

    pub fn upload(&mut self, rs: &egui_wgpu::RenderState, mesh: &SceneMesh) {
        self.uploads += 1;
        self.mesh_gen += 1;
        self.opaque = GpuBuf::new(rs, "faces", bytemuck::cast_slice(&mesh.opaque), mesh.opaque.len(), 3);
        self.transparent =
            GpuBuf::new(rs, "transparent", bytemuck::cast_slice(&mesh.transparent), mesh.transparent.len(), 3);
        self.transparent_cpu = mesh.transparent.clone();
        self.sorter.set(&self.transparent_cpu);
        self.sorted_for = None;
        self.line_buf = GpuBuf::new(rs, "lines", bytemuck::cast_slice(&mesh.lines), mesh.lines.len(), 1);
        self.clips = mesh.clips.clone();
        self.bounds = mesh.bounds;
        let same = self.tex_layers.len() == mesh.textures.len()
            && self.tex_layers.iter().zip(&mesh.textures).all(|(a, b)| Arc::ptr_eq(a, b));
        if !same {
            self.tex_view = texture_array(rs, &mesh.textures);
            self.tex_layers = mesh.textures.clone();
        }
    }

    fn ensure_targets(&mut self, rs: &egui_wgpu::RenderState, size: [u32; 2]) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let device = &rs.device;
        let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let tex = |label, format, samples, usage, view_formats: &[wgpu::TextureFormat]| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats,
            })
        };
        let msaa = tex("msaa", COLOR, self.config.samples, wgpu::TextureUsages::RENDER_ATTACHMENT, &[]);
        // The scene is written through an sRGB view (linear → sRGB encoding) and egui samples
        // the same bytes through a plain view, since it expects gamma-encoded textures.
        let resolve = tex(
            "resolve",
            wgpu::TextureFormat::Rgba8Unorm,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            &[COLOR],
        );
        let depth = tex("depth", DEPTH, self.config.samples, wgpu::TextureUsages::RENDER_ATTACHMENT, &[]);
        let sampled = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let ao_depth = tex("ao depth", DEPTH, 1, sampled, &[]);
        let ao_raw = tex("ao raw", wgpu::TextureFormat::R8Unorm, 1, sampled, &[]);
        let ao_blur = tex("ao blur", wgpu::TextureFormat::R8Unorm, 1, sampled, &[]);
        let resolve_view =
            resolve.create_view(&wgpu::TextureViewDescriptor { format: Some(COLOR), ..Default::default() });
        let egui_view = resolve.create_view(&Default::default());
        let mut renderer = rs.renderer.write();
        match self.tex_id {
            Some(id) => {
                renderer.update_egui_texture_from_wgpu_texture(device, &egui_view, wgpu::FilterMode::Linear, id)
            }
            None => self.tex_id = Some(renderer.register_native_texture(device, &egui_view, wgpu::FilterMode::Linear)),
        }
        self.targets = Some(Targets {
            size,
            msaa: msaa.create_view(&Default::default()),
            resolve: resolve_view,
            depth: depth.create_view(&Default::default()),
            texture: resolve,
            ao_depth: ao_depth.create_view(&Default::default()),
            ao_raw: ao_raw.create_view(&Default::default()),
            ao_blur: ao_blur.create_view(&Default::default()),
        });
    }

    /// Renders the scene and returns the texture to paint.
    pub fn render(
        &mut self,
        rs: &egui_wgpu::RenderState,
        cam: &Camera,
        size: [u32; 2],
        ppp: f32,
        style: &ViewStyle,
        lighting: &Lighting,
    ) -> egui::TextureId {
        let size = [size[0].max(1), size[1].max(1)];
        self.ensure_targets(rs, size);
        let t = self.targets.as_ref().expect("targets");
        let vp = cam.view_proj().as_mat4().to_cols_array_2d();
        let light = (cam.right() * 0.35 + cam.up() * 0.55 - cam.forward() * 0.75).normalize();
        let (eye, fwd) = (cam.eye(), cam.forward());
        let mut bits = style.line_flags();
        if style.faces == FaceStyle::HiddenLine {
            bits |= 8;
        }
        if style.fog {
            bits |= 16;
        }
        if style.back_edges {
            bits |= 32;
        }
        if style.depth_cue {
            bits |= 64;
        }
        // Ambient occlusion needs faces to occlude each other.
        let ao_on = style.ambient_occlusion
            && self.opaque.is_some()
            && !matches!(style.faces, FaceStyle::Wireframe | FaceStyle::HiddenLine);
        if ao_on {
            bits |= 128;
        }
        if style.photoreal {
            bits |= 256;
        }
        if style.endpoints {
            bits |= 512;
        }
        // Background › Ground, and whether it is seen from below.
        if style.ground {
            bits |= 1024;
        }
        if style.underground {
            bits |= 2048;
        }
        // Fog shows once both distances are set, the start before the end (as SketchUp's
        // fog stage requires).
        let fog_on = style.fog && style.fog_start >= 0.0 && style.fog_end > style.fog_start;
        if fog_on {
            bits |= 16;
        } else {
            bits &= !16;
        }
        let (dc_near, dc_far) = depth_range(self.bounds, cam);
        // The sun's view covers the model's bounding sphere.
        let sun = lighting.sun.unwrap_or(DVec3::Z);
        let (center, radius) = match self.bounds {
            Some((a, b)) => ((a + b) * 0.5, ((b - a).length() * 0.5).max(100.0)),
            None => (DVec3::ZERO, 1000.0),
        };
        let up = if sun.cross(DVec3::Z).length_squared() < 1e-6 { DVec3::Y } else { DVec3::Z };
        let light_vp = directx::orthographic(-radius, radius, -radius, radius, 0.0, radius * 4.0)
            * view::look_at_mat4(center + sun * radius * 2.0, center, up);
        let cast = lighting.cast && lighting.sun.is_some() && sun.z > 0.0 && self.opaque.is_some();
        let ground_half = radius * 12.0;
        let mut clips = [[0.0f32; 4]; MAX_CLIPS];
        for (c, p) in clips.iter_mut().zip(&self.clips) {
            *c = *p;
        }
        let lin = |c: [u8; 3]| c.map(|c| srgb_to_linear(c) as f32);
        // Environment: as a skydome (not behind a matched photo) and in the reflections.
        let (env, env_lod) = match (&self.env, &self.env_image) {
            (Some(e), Some(img)) => {
                let flags = (e.skydome && !self.photo_bg) as u8 | (e.reflections as u8) << 1;
                // The sky's level: one texel per pixel.
                let texel = std::f64::consts::TAU / img.levels[0].0 as f64;
                let pixel = if cam.perspective { cam.fov_y / size[1] as f64 } else { texel };
                let lod = (pixel / texel).log2().max(0.0);
                (
                    [e.rotation.to_radians() as f32, e.skydome_exposure, e.reflection_exposure, flags as f32],
                    [img.levels.len() as f32, lod as f32, 0.0, 0.0],
                )
            }
            _ => ([0.0; 4], [1.0, 0.0, 0.0, 0.0]),
        };
        let grid = self.grid_globals(cam, ppp);
        let bg = lin(style.bg_color);
        let fog_rgb = lin(style.fog_rgb());
        let ground_rgb = lin(style.ground_color);
        let g = Globals {
            view_proj: vp,
            light_dir: [light.x as f32, light.y as f32, light.z as f32, 0.0],
            params: [if cam.perspective { 2e-4 } else { 2e-5 }, ppp, style.profile_width as f32, bits as f32],
            viewport: [size[0] as f32, size[1] as f32, dc_near as f32, dc_far as f32],
            eye: [eye.x as f32, eye.y as f32, eye.z as f32, if cam.perspective { 1.0 } else { 0.0 }],
            fwd: [fwd.x as f32, fwd.y as f32, fwd.z as f32, 0.0],
            fog: [style.fog_start as f32, style.fog_end as f32, style.depth_cue_width.clamp(1, 4) as f32, 0.0],
            bg: [bg[0], bg[1], bg[2], 1.0],
            clips,
            light_vp: light_vp.as_mat4().to_cols_array_2d(),
            sun: [sun.x as f32, sun.y as f32, sun.z as f32, lighting.sun.is_some() as u8 as f32],
            shadow: [lighting.light, lighting.dark, lighting.on_faces as u8 as f32, cast as u8 as f32],
            ground: [center.x as f32, center.y as f32, ground_half as f32, (cast && lighting.on_ground) as u8 as f32],
            inv_view_proj: cam.view_proj().inverse().as_mat4().to_cols_array_2d(),
            sky: [
                style.sky_color[0] as f32 / 255.0,
                style.sky_color[1] as f32 / 255.0,
                style.sky_color[2] as f32 / 255.0,
                style.sky as u8 as f32,
            ],
            ao: [style.ao_distance, style.ao_intensity, style.ao_multiplier, 0.0],
            edge: [style.extension_length as f32, style.endpoint_length as f32, ENDPOINT_WIDTH, JITTER_PX],
            fog_color: [fog_rgb[0], fog_rgb[1], fog_rgb[2], fog_on as u8 as f32],
            ground_color: [
                ground_rgb[0],
                ground_rgb[1],
                ground_rgb[2],
                1.0 - style.ground_transparency.min(100) as f32 / 100.0,
            ],
            env,
            env_lod,
            grid_origin: grid[0],
            grid_u: grid[1],
            grid_v: grid[2],
            grid_major: grid[3],
            grid_minor: grid[4],
            grid_misc: grid[5],
            grid_quad: grid[6],
        };
        rs.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&g));
        self.wm.prepare(rs, style, size, ppp);
        let mut enc = rs.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("3d") });
        // The shadow map depends on the sun's view, the cut planes and the faces only: when
        // none of them changed since it was drawn, it is still there (an orbit redraws it
        // no more).
        let shadow_key =
            (light_vp.as_mat4().to_cols_array().map(f32::to_bits), clips.map(|c| c.map(f32::to_bits)), self.mesh_gen);
        let shadow_drawn = self.shadow_of == Some(shadow_key);
        if cast
            && !shadow_drawn
            && let Some(b) = &self.opaque
        {
            self.shadow_of = Some(shadow_key);
            crate::perf::count(|c| c.shadow_passes += 1);
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.shadow_bind, &[]);
            pass.set_pipeline(&self.shadow_pipe);
            b.draw(&mut pass);
        }
        if ao_on && let Some(b) = &self.opaque {
            // 1. Depth of the opaque faces, 2. occlusion around each pixel, 3. blur.
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ao pre-pass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &t.ao_depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(0.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_bind_group(0, &self.shadow_bind, &[]);
                pass.set_pipeline(&self.ao_pre);
                b.draw(&mut pass);
            }
            let post_bind = |input: &wgpu::TextureView| {
                rs.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("post"),
                    layout: &self.post_bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&t.ao_depth) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(input) },
                    ],
                })
            };
            for (pipe, input, target, label) in
                [(&self.ao_gen, &self.white, &t.ao_raw, "ao"), (&self.ao_blur, &t.ao_raw, &t.ao_blur, "ao blur")]
            {
                let bind = post_bind(input);
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipe);
                pass.set_bind_group(0, &self.shadow_bind, &[]);
                pass.set_bind_group(1, &bind, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        let main_bind = rs.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.shadow_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.shadow_sampler) },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(if ao_on { &t.ao_blur } else { &self.white }),
                },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.tex_view) },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(if self.mipmapping {
                        &self.tex_sampler
                    } else {
                        &self.tex_sampler_plain
                    }),
                },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&self.env_view) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&self.env_sampler) },
            ],
        });
        self.photo_fg.prepare(rs, self.photo_overlay.as_ref());
        // Styles › Faces › Transparency quality (`oit.rs`): « Meilleure » draws the
        // transparent triangles sorted from the eye, « Rapide » accumulates them in a pass of
        // their own, resolved over the image.
        let nicer = crate::oit::nicer(style.transparency_sort);
        if nicer && self.transparent.is_some() {
            let (eye, fwd) = (cam.eye().as_vec3().to_array(), cam.forward().as_vec3().to_array());
            let key = [eye[0], eye[1], eye[2], fwd[0], fwd[1], fwd[2], cam.perspective as u8 as f32];
            if self.sorted_for != Some(key) {
                let sorted = self.sorter.sort(&self.transparent_cpu, eye, fwd, cam.perspective);
                // The same triangles in another order: written over the last sorted ones.
                if self.sorted_for.is_some()
                    && let Some(b) = &self.transparent
                {
                    b.write(rs, 0, bytemuck::cast_slice(sorted));
                } else {
                    self.transparent =
                        GpuBuf::new(rs, "transparent trié", bytemuck::cast_slice(sorted), sorted.len(), 3);
                }
                self.sorted_for = Some(key);
            }
        }
        let oit_after = !nicer && self.transparent.is_some();
        if oit_after {
            crate::oit::Oit::ensure(&mut self.oit, rs, &self.oit_pipes, size, self.config.samples);
        }
        let bg = style.bg_color.map(srgb_to_linear);
        // Without multisampling the view is drawn straight into the resolved image.
        let (view, resolve, store) = if self.config.samples > 1 {
            (&t.msaa, Some(&t.resolve), wgpu::StoreOp::Discard)
        } else {
            (&t.resolve, None, wgpu::StoreOp::Store)
        };
        {
            // With « Rapide », the image and its depth are kept for the passes after this one,
            // which resolves the samples last.
            let (resolve_main, store_main, depth_store) = if oit_after {
                (None, wgpu::StoreOp::Store, wgpu::StoreOp::Store)
            } else {
                (resolve, store, wgpu::StoreOp::Discard)
            };
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3d"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: resolve_main,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(if self.photo_bg {
                            wgpu::Color::TRANSPARENT
                        } else {
                            wgpu::Color { r: bg[0], g: bg[1], b: bg[2], a: 1.0 }
                        }),
                        store: store_main,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: depth_store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &main_bind, &[]);
            if !self.photo_bg {
                pass.set_pipeline(&self.sky_pipe);
                pass.draw(0..3, 0..1);
            }
            // Watermarks behind the model.
            self.wm.draw(&mut pass, true);
            pass.set_bind_group(0, &main_bind, &[]);
            if let Some(b) = &self.opaque {
                pass.set_pipeline(&self.face_opaque);
                b.draw(&mut pass);
            }
            // Match Photo › Foreground Photo: over the faces, under the edges.
            if self.photo_overlay.is_some() {
                self.photo_fg.draw(&mut pass);
                pass.set_bind_group(0, &main_bind, &[]);
            }
            if cast && lighting.on_ground {
                pass.set_pipeline(&self.ground_pipe);
                pass.draw(0..6, 0..1);
            }
            if self.grid.is_some() {
                pass.set_pipeline(&self.grid_pipe);
                pass.draw(0..6, 0..1);
            }
            if let Some(b) = &self.line_buf {
                pass.set_pipeline(&self.lines);
                b.draw_instances(&mut pass, 6);
                // Back edges, and the parts of section planes hidden by faces.
                pass.set_pipeline(&self.back_lines);
                b.draw_instances(&mut pass, 6);
            }
            if nicer && let Some(b) = &self.transparent {
                pass.set_pipeline(&self.face_blend);
                b.draw(&mut pass);
            }
            // Watermarks over the model.
            if !oit_after {
                self.wm.draw(&mut pass, false);
            }
        }
        if let (true, Some(o), Some(b)) = (oit_after, &self.oit, &self.transparent) {
            // « Rapide »: the transparent faces added up, tested against the opaque depth.
            {
                let attachments = o.attachments();
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("transparence rapide"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &t.depth,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_bind_group(0, &main_bind, &[]);
                pass.set_pipeline(&self.face_oit);
                b.draw(&mut pass);
            }
            // Resolved over the image, then the watermarks over the model.
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("résolution de la transparence"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.oit_pipes.resolve);
            pass.set_bind_group(0, o.bind(), &[]);
            pass.draw(0..3, 0..1);
            self.wm.draw(&mut pass, false);
        }
        rs.queue.submit([enc.finish()]);
        self.tex_id.expect("texture registered")
    }

    /// Reads the last rendered frame back (used by `--snapshot`).
    pub fn read_pixels(&self, rs: &egui_wgpu::RenderState) -> Option<(u32, u32, Vec<u8>)> {
        let t = self.targets.as_ref()?;
        let [w, h] = t.size;
        let bpr = (w * 4).div_ceil(256) * 256;
        let buf = rs.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (bpr * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = rs.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            t.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        rs.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        rs.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;
        let data = slice.get_mapped_range().ok()?;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h {
            let start = (row * bpr) as usize;
            out.extend_from_slice(&data[start..start + (w * 4) as usize]);
        }
        Some((w, h, out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_range_spans_the_bounding_box_along_the_view() {
        let cam = Camera { target: DVec3::ZERO, distance: 1000.0, yaw: 0.0, pitch: 0.0, ..Default::default() };
        // Eye on +X looking at the origin: the near face is at x = 300 (700 from the eye),
        // the far face at x = -100 (1100).
        let (near, far) = depth_range(Some((DVec3::new(-100.0, -50.0, 0.0), DVec3::new(300.0, 50.0, 80.0))), &cam);
        assert!((near - 700.0).abs() < 1e-6 && (far - 1100.0).abs() < 1e-6, "{near} {far}");
        let (n, f) = depth_range(None, &cam);
        assert!(n < f);
    }
}
