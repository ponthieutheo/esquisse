//! Renders a model offscreen on the real GPU, the way the 3D view does, and reads the pixels
//! back: tests of what the view shows. Without a usable adapter the tests that need one
//! say so and pass.

use std::future::Future;
use std::sync::Arc;

use eframe::egui_wgpu::{self, wgpu};
use esq_core::glam::{DMat4, DVec2};
use esq_core::model::Model;

use crate::camera::Camera;
use crate::render::{Lighting, Viewport3d};
use crate::style::ViewStyle;

fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

/// An instance with Vulkan, DX12 or Metal only. The tests never use OpenGL, but just making
/// the instance would start the OpenGL driver: AMD's (atio6axx.dll) sometimes divides by zero
/// when tests running side by side create their contexts at the same moment, and that kills
/// the whole test process.
pub fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
}

/// A device and queue like the window's, or `None` when the machine has no GPU.
pub fn render_state() -> Option<egui_wgpu::RenderState> {
    let instance = instance();
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let renderer = egui_wgpu::Renderer::new(&device, format, Default::default());
    Some(egui_wgpu::RenderState {
        adapter: adapter.clone(),
        available_adapters: vec![adapter],
        instance,
        device,
        queue,
        target_format: format,
        renderer: Arc::new(eframe::egui::mutex::RwLock::new(renderer)),
    })
}

/// An offscreen 3D view.
pub struct Gpu {
    pub rs: egui_wgpu::RenderState,
    pub view: Viewport3d,
}

impl Gpu {
    pub fn new() -> Option<Gpu> {
        let rs = render_state()?;
        let view = Viewport3d::new(&rs);
        Some(Gpu { rs, view })
    }

    /// The model as the view shows it with `style`, seen by `cam`: RGBA pixels, row by row.
    pub fn render(&mut self, model: &Model, style: &ViewStyle, cam: &Camera, lighting: &Lighting) -> Shot {
        let opts = crate::scene::SceneOptions { ctx: &[], ctx_def: None, sel: &[], axes_xform: DMat4::IDENTITY, style };
        let mesh = crate::scene::build(model, &opts);
        self.view.upload(&self.rs, &mesh);
        let size = [cam.size.x as u32, cam.size.y as u32];
        self.view.render(&self.rs, cam, size, 1.0, style, lighting);
        let (w, h, px) = self.view.read_pixels(&self.rs).expect("lecture des pixels");
        Shot { w, h, px }
    }
}

pub struct Shot {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Shot {
    pub fn at(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.w + x) * 4) as usize;
        [self.px[i], self.px[i + 1], self.px[i + 2]]
    }

    /// The pixel where a model point appears.
    pub fn at_point(&self, cam: &Camera, p: esq_core::glam::DVec3) -> [u8; 3] {
        let s: DVec2 = cam.project(p).expect("point visible");
        self.at(s.x.round() as u32, s.y.round() as u32)
    }

    #[allow(dead_code)]
    pub fn save(&self, path: &str) {
        crate::png::write_rgba(std::path::Path::new(path), self.w, self.h, &self.px).unwrap();
    }
}

/// A camera looking straight down at the XY square [0, size]², filling a `px` × `px` view.
pub fn top_camera(size: f64, px: f64) -> Camera {
    let mut cam = Camera {
        size: DVec2::new(px, px),
        target: esq_core::glam::DVec3::new(size / 2.0, size / 2.0, 0.0),
        distance: size * 3.0,
        ..Default::default()
    };
    cam.set_view(-90.0, 89.9);
    cam.perspective = false;
    cam.fov_y = 2.0 * (size * 0.55 / cam.distance).atan();
    cam
}

/// A big model's vertices go to the GPU in several buffers (wgpu refuses one above its
/// limit, 256 Mo by default, and the application stopped): drawn in order, they show the
/// same image as one buffer, faces, transparent faces, edges and shadows alike.
#[test]
fn vertices_cut_into_several_buffers_draw_the_same_image() {
    use esq_core::glam::DVec3;
    let Some(mut gpu) = Gpu::new() else { return eprintln!("pas de GPU : test sauté") };
    let mut m = Model::new();
    let glass = m.add_material("Verre", [90, 160, 220]);
    m.materials[glass].opacity = 0.4;
    for i in 0..12 {
        for j in 0..12 {
            let (x, y, z) = (i as f64 * 90.0, j as f64 * 90.0, ((i + 2 * j) % 4) as f64 * 30.0);
            let c = [(0.0, 0.0), (80.0, 0.0), (80.0, 80.0), (0.0, 80.0)]
                .map(|(dx, dy)| m.root.add_vertex_raw(DVec3::new(x + dx, y + dy, z)));
            for k in 0..4 {
                m.root.add_edge_raw(c[k], c[(k + 1) % 4]);
            }
            let f = m.root.add_face_raw(c.to_vec(), Vec::new()).unwrap();
            if (i + j) % 3 == 0 {
                m.root.faces[f].front = Some(glass);
            }
        }
    }
    let cam = Camera {
        size: DVec2::new(240.0, 180.0),
        target: DVec3::new(540.0, 540.0, 0.0),
        distance: 2200.0,
        yaw: (-57.0f64).to_radians(),
        pitch: 35.0f64.to_radians(),
        ..Default::default()
    };
    let lighting = Lighting {
        sun: Some(DVec3::new(0.3, -0.4, 0.8).normalize()),
        light: 0.8,
        dark: 0.45,
        cast: true,
        on_faces: true,
        on_ground: true,
    };
    for sort in [0, 2] {
        let style = ViewStyle { transparency_sort: sort, ..Default::default() };
        crate::render::PART_ITEMS.with(|p| p.set(None));
        let whole = gpu.render(&m, &style, &cam, &lighting);
        assert_eq!(gpu.view.gpu_parts(), (1, 1, 1));
        crate::render::PART_ITEMS.with(|p| p.set(Some(7)));
        let cut = gpu.render(&m, &style, &cam, &lighting);
        let parts = gpu.view.gpu_parts();
        crate::render::PART_ITEMS.with(|p| p.set(None));
        assert!(parts.0 > 10 && parts.1 > 2 && parts.2 > 10, "{parts:?}");
        assert!(whole.px == cut.px, "transparence {sort} : image différente");
    }
}

/// The shadow map is drawn again only when the sun, the cut planes or the faces change: an
/// orbit draws it once, and its images are those of a view that draws it at each one.
#[test]
fn an_orbit_draws_the_shadow_map_once() {
    use esq_core::glam::DVec3;
    let Some(mut gpu) = Gpu::new() else { return eprintln!("pas de GPU : test sauté") };
    let mut m = Model::new();
    for (x, z) in [(0.0, 0.0), (300.0, 400.0), (600.0, 150.0)] {
        let c = [(0.0, 0.0), (500.0, 0.0), (500.0, 500.0), (0.0, 500.0)]
            .map(|(dx, dy)| m.root.add_vertex_raw(DVec3::new(x + dx, dy, z)));
        m.root.add_face_raw(c.to_vec(), Vec::new()).unwrap();
    }
    let style = ViewStyle::default();
    let mut cam = Camera {
        size: DVec2::new(200.0, 150.0),
        target: DVec3::new(500.0, 250.0, 100.0),
        distance: 2500.0,
        yaw: (-57.0f64).to_radians(),
        pitch: 35.0f64.to_radians(),
        ..Default::default()
    };
    let sun = |s: DVec3| Lighting {
        sun: Some(s.normalize()),
        light: 0.8,
        dark: 0.45,
        cast: true,
        on_faces: true,
        on_ground: true,
    };
    let lighting = sun(DVec3::new(0.3, -0.4, 0.8));
    gpu.render(&m, &style, &cam, &lighting);
    let p0 = crate::perf::get().shadow_passes;
    for _ in 0..5 {
        cam.orbit_with(20.0, 0.0, true);
        gpu.view.render(&gpu.rs, &cam, [200, 150], 1.0, &style, &lighting);
    }
    assert_eq!(crate::perf::get().shadow_passes - p0, 0, "carte d'ombres redessinée");
    let (_, _, orbit) = gpu.view.read_pixels(&gpu.rs).unwrap();
    let mut other = Gpu::new().unwrap();
    assert!(other.render(&m, &style, &cam, &lighting).px == orbit, "image différente");
    // Another sun: drawn again.
    let p1 = crate::perf::get().shadow_passes;
    gpu.view.render(&gpu.rs, &cam, [200, 150], 1.0, &style, &sun(DVec3::new(-0.5, 0.2, 0.7)));
    assert_eq!(crate::perf::get().shadow_passes - p1, 1);
}

/// After a change of the model, the view makes the picking cache on another thread while it
/// makes and sends the mesh (`EsquisseApp::prepare_scene`): counted here, it is the cache made
/// on this thread, for the eye that components facing the camera turn to and for the units
/// the dimensions are written in.
#[test]
fn the_picking_cache_made_beside_the_mesh_is_the_one_made_here() {
    use esq_core::glam::DVec3;
    use esq_core::model::{AnnotKind, Annotation, Definition, Instance};
    use esq_core::units::{UnitsDisplay, set_display};
    let Some(gpu) = Gpu::new() else { return eprintln!("pas de GPU : test sauté") };
    let mut m = Model::new();
    let mut e = esq_core::model::Entities::default();
    let c =
        [(0.0, 0.0), (300.0, 0.0), (300.0, 500.0), (0.0, 500.0)].map(|(x, z)| e.add_vertex_raw(DVec3::new(x, 0.0, z)));
    e.add_face_raw(c.to_vec(), Vec::new()).unwrap();
    let d = m.defs.insert(Definition {
        name: "Personnage".into(),
        is_group: false,
        description: String::new(),
        entities: e,
        attributes: Default::default(),
        ext: Default::default(),
    });
    m.defs[d].ext.behavior.always_face_camera = true;
    for x in [0.0, 900.0, 1800.0] {
        m.root.instances.insert(Instance {
            def: d,
            xform: DMat4::from_translation(DVec3::new(x, 400.0, 0.0)),
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
    }
    let kind = AnnotKind::Linear { a: DVec3::ZERO, b: DVec3::new(1234.56789, 0.0, 0.0), offset: DVec3::Z * 100.0 };
    m.root.annots.insert(Annotation { kind, text: String::new(), layer: 0, hidden: false, attach: Default::default() });
    set_display(Some(UnitsDisplay { precision: 5, ..UnitsDisplay::of(&m) }));
    let mut app = crate::app::EsquisseApp::bare(Some(Viewport3d::new(&gpu.rs)), None);
    app.doc = crate::doc::Document::new(m);
    app.cam = Camera {
        size: DVec2::new(200.0, 150.0),
        target: DVec3::new(900.0, 400.0, 250.0),
        distance: 4000.0,
        yaw: (-57.0f64).to_radians(),
        pitch: 25.0f64.to_radians(),
        ..Default::default()
    };
    app.turn_with_eye();
    app.prepare_scene(&gpu.rs);
    app.doc.commit("Déplacer", |m, _| {
        let i = m.root.instances.values_mut().next().unwrap();
        i.xform = DMat4::from_translation(DVec3::Y * 250.0) * i.xform;
    });
    app.cam.orbit_with(25.0, 0.0, true);
    let before = crate::perf::get().pick_builds;
    app.turn_with_eye();
    app.prepare_scene(&gpu.rs);
    assert_eq!(crate::perf::get().pick_builds - before, 1);
    let here = crate::pick::PickCache::build(&app.doc.model, &app.style).contents();
    assert_eq!(app.pick.contents(), here);
    // What the thread brings matters: the units write the dimension otherwise, and without
    // the eye the components stay as placed.
    let shown = esq_core::units::format_length(1234.56789, app.doc.model.units);
    set_display(None);
    assert_ne!(esq_core::units::format_length(1234.56789, app.doc.model.units), shown);
    assert!(here.iter().any(|l| l.contains(&shown)), "{shown}");
    let eye = esq_core::walk::eye();
    esq_core::walk::set_eye(None);
    assert!(eye.is_some());
    set_display(Some(UnitsDisplay { precision: 5, ..UnitsDisplay::of(&app.doc.model) }));
    assert_ne!(crate::pick::PickCache::build(&app.doc.model, &app.style).contents(), here);
    set_display(None);
}

/// A texture distorted in perspective (4 pins: a q row) is divided per pixel: it follows the
/// exact projective map everywhere, without the seam that interpolating per-vertex
/// coordinates leaves on the diagonal between the face's two triangles.
#[test]
fn perspective_texture_has_no_seam_on_the_diagonal() {
    use esq_core::glam::DVec3;
    use esq_core::meta::{FaceTex, TextureData, UvMap};
    use esq_core::photo::Blob;

    let Some(mut gpu) = Gpu::new() else { return eprintln!("pas de GPU : test sauté") };
    // Two texels: red on the left half of the tile, blue on the right.
    let mut img = image::RgbaImage::new(2, 1);
    img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
    img.put_pixel(1, 0, image::Rgba([0, 0, 255, 255]));
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let mut m = Model::new();
    let mat = m.add_material("Rayures", [128, 128, 128]);
    m.materials[mat].ext.texture = Some(TextureData {
        filename: "rayures.png".into(),
        image: Blob(Arc::new(png)),
        px: [2, 1],
        size: [250.0, 250.0],
        avg: [128, 128, 128],
    });
    let v = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)]
        .map(|(x, y)| m.root.add_vertex_raw(DVec3::new(x, y, 0.0)));
    let f = m.root.add_face_raw(v.to_vec(), Vec::new()).unwrap();
    // q grows unevenly towards the far corner: the stripes converge like a pinned photograph.
    let map = UvMap { u: [0.004, 0.0, 0.0, 0.0], v: [0.0, 0.004, 0.0, 0.0], q: [0.0005, 0.0015, 0.0, 1.0] };
    let face = &mut m.root.faces[f];
    face.front = Some(mat);
    face.tex = Some(Box::new(FaceTex { front: Some(map), ..Default::default() }));

    let cam = top_camera(1000.0, 200.0);
    let style = ViewStyle {
        faces: crate::style::FaceStyle::ShadedWithTextures,
        axes: false,
        edges: false,
        ..Default::default()
    };
    let shot = gpu.render(&m, &style, &cam, &Lighting::default());
    let (mut checked, mut wrong) = (0, Vec::new());
    for i in 1..40 {
        for j in 1..40 {
            let p = DVec3::new(i as f64 * 25.0, j as f64 * 25.0, 0.0);
            let s = cam.project(p).unwrap();
            // Pixel centres only, away from the stripes' borders (filtering blends there).
            let (x, y) = (s.x.floor(), s.y.floor());
            let (o, d) = cam.ray(DVec2::new(x + 0.5, y + 0.5));
            let centre = o - d * (o.z / d.z);
            let u = map.at(centre).x.rem_euclid(1.0);
            if [0.0, 0.5, 1.0].iter().any(|b| (u - b).abs() < 0.12) {
                continue;
            }
            let c = shot.at(x as u32, y as u32);
            let red = c[0] > c[2];
            checked += 1;
            if red != (u < 0.5) {
                wrong.push((p.x, p.y, u, c));
            }
        }
    }
    assert!(checked > 400, "{checked} points vérifiés");
    assert!(wrong.is_empty(), "{} points faux, ex. {:?}", wrong.len(), &wrong[..wrong.len().min(5)]);
}

#[test]
fn every_test_instance_leaves_opengl_out() {
    let banned = ["Instance::", "default()"].concat();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src];
    let mut found = Vec::new();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).unwrap();
                for (i, l) in text.lines().enumerate() {
                    if l.contains(&banned) {
                        found.push(format!("{}:{}", p.display(), i + 1));
                    }
                }
            }
        }
    }
    assert!(found.is_empty(), "wgpu instances made without tests::gpu::instance: {found:?}");
}
