//! The component library: a collection named as SketchUp's sample one (`Components Sampler`,
//! "Échantillon de composants"), whose components are all made here from boxes and outlines:
//! furniture, a door and a window that glue to walls and cut their opening, a 2D figure and a
//! 2D tree that face the camera. Each one is a model of its own (`.esq`, its alignment in Model
//! Info › File as SketchUp keeps it in a component file) with its thumbnail beside it (`.png`).

use esq_core::glam::{DVec2, DVec3};
use esq_core::model::{Behavior, MaterialId, Model};

use super::noise::png_rgba;
use crate::camera::Camera;
use crate::render::Lighting;
use crate::style::ViewStyle;
use crate::tests::gpu::Gpu;

/// The collections and how many components each holds.
pub const COLLECTIONS: [(&str, usize); 1] = [("Components Sampler", 12)];

pub struct Entry {
    pub folder: &'static str,
    pub file: String,
    pub model: Model,
    pub thumbnail: Vec<u8>,
}

/// Builds a component's model.
struct Build {
    m: Model,
}

fn p(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

impl Build {
    fn new(name: &str, description: &str) -> Build {
        let mut m = Model::new();
        m.meta.name = name.into();
        m.meta.description = description.into();
        Build { m }
    }

    fn material(&mut self, name: &str, c: [u8; 3]) -> MaterialId {
        self.m.add_material(name, c)
    }

    /// A planar polygon (counter-clockwise seen from its front), both sides painted.
    fn poly(&mut self, pts: &[DVec3], mat: Option<MaterialId>) {
        let v: Vec<_> = pts.iter().map(|q| self.m.root.add_vertex_raw(*q)).collect();
        if let Some(f) = self.m.root.add_face_raw(v, Vec::new()) {
            self.m.root.faces[f].front = mat;
            self.m.root.faces[f].back = mat;
        }
    }

    /// A box between two corners, its faces turned outwards.
    fn cuboid(&mut self, lo: DVec3, hi: DVec3, mat: Option<MaterialId>) {
        let c = |i: usize| {
            p(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            )
        };
        for quad in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
            self.poly(&quad.map(c), mat);
        }
    }

    /// A closed loop of edges without a face (the opening a gluing component cuts).
    fn edge_loop(&mut self, pts: &[DVec3]) {
        let v: Vec<_> = pts.iter().map(|q| self.m.root.add_vertex_raw(*q)).collect();
        for i in 0..v.len() {
            self.m.root.add_edge_raw(v[i], v[(i + 1) % v.len()]);
        }
    }

    /// An upright outline in the XZ plane (x across, z up), facing -y.
    fn outline(&mut self, pts: &[(f64, f64)], mat: Option<MaterialId>) {
        let pts: Vec<DVec3> = pts.iter().map(|(x, z)| p(*x, 0.0, *z)).collect();
        self.poly(&pts, mat);
    }

    fn behave(mut self, f: impl FnOnce(&mut Behavior)) -> Build {
        let mut b = self.m.behavior();
        f(&mut b);
        self.m.set_behavior(b);
        self
    }

    fn done(mut self) -> Model {
        self.m.rebuild_topology();
        self.m
    }
}

fn table() -> Model {
    let mut b = Build::new("Table", "Table de 160 × 90 cm, plateau de bois sur quatre pieds.");
    let wood = b.material("Bois clair", [196, 160, 112]);
    b.cuboid(p(0.0, 0.0, 720.0), p(1600.0, 900.0, 760.0), Some(wood));
    for (x, y) in [(40.0, 40.0), (1500.0, 40.0), (40.0, 800.0), (1500.0, 800.0)] {
        b.cuboid(p(x, y, 0.0), p(x + 60.0, y + 60.0, 720.0), Some(wood));
    }
    b.done()
}

fn chair() -> Model {
    let mut b = Build::new("Chaise", "Chaise à dossier droit, assise à 45 cm.");
    let wood = b.material("Bois foncé", [120, 84, 52]);
    b.cuboid(p(0.0, 0.0, 430.0), p(450.0, 450.0, 460.0), Some(wood));
    for (x, y) in [(0.0, 0.0), (410.0, 0.0), (0.0, 410.0), (410.0, 410.0)] {
        b.cuboid(p(x, y, 0.0), p(x + 40.0, y + 40.0, 430.0), Some(wood));
    }
    b.cuboid(p(0.0, 410.0, 460.0), p(450.0, 450.0, 900.0), Some(wood));
    b.done()
}

fn bench() -> Model {
    let mut b = Build::new("Banc", "Banc de jardin de 180 cm, trois lattes et deux piètements.");
    let wood = b.material("Lattes", [168, 128, 84]);
    let metal = b.material("Acier noir", [40, 40, 44]);
    for i in 0..3 {
        let y = i as f64 * 140.0;
        b.cuboid(p(0.0, y, 420.0), p(1800.0, y + 110.0, 450.0), Some(wood));
    }
    for x in [150.0, 1600.0] {
        b.cuboid(p(x, 0.0, 0.0), p(x + 50.0, 390.0, 420.0), Some(metal));
    }
    b.done()
}

fn shelf() -> Model {
    let mut b = Build::new("Étagère", "Bibliothèque de 80 × 30 × 180 cm, quatre tablettes.");
    let white = b.material("Blanc mat", [236, 234, 228]);
    b.cuboid(p(0.0, 0.0, 0.0), p(20.0, 300.0, 1800.0), Some(white));
    b.cuboid(p(780.0, 0.0, 0.0), p(800.0, 300.0, 1800.0), Some(white));
    for i in 0..5 {
        let z = i as f64 * 440.0;
        b.cuboid(p(20.0, 0.0, z), p(780.0, 300.0, z + 20.0), Some(white));
    }
    b.done()
}

fn bed() -> Model {
    let mut b = Build::new("Lit", "Lit double de 140 × 190 cm avec tête de lit.");
    let wood = b.material("Chêne", [176, 136, 92]);
    let cloth = b.material("Drap", [214, 222, 232]);
    b.cuboid(p(0.0, 0.0, 0.0), p(1440.0, 1940.0, 300.0), Some(wood));
    b.cuboid(p(20.0, 20.0, 300.0), p(1420.0, 1920.0, 500.0), Some(cloth));
    b.cuboid(p(0.0, 1940.0, 0.0), p(1440.0, 2000.0, 1000.0), Some(wood));
    b.done()
}

fn door() -> Model {
    let mut b = Build::new("Porte intérieure", "Porte de 83 × 204 cm : collée aux murs, elle y découpe son ouverture.")
        .behave(|x| {
            x.is2d = true;
            x.snapto = 2;
            x.cuts_opening = true;
        });
    let leaf = b.material("Porte peinte", [228, 226, 218]);
    let frame = b.material("Huisserie", [150, 150, 150]);
    // The opening's outline on the wall (z = 0 in the component's axes, the wall's plane).
    b.edge_loop(&[p(0.0, 0.0, 0.0), p(830.0, 0.0, 0.0), p(830.0, 2040.0, 0.0), p(0.0, 2040.0, 0.0)]);
    b.cuboid(p(0.0, 0.0, 0.0), p(40.0, 2040.0, 60.0), Some(frame));
    b.cuboid(p(790.0, 0.0, 0.0), p(830.0, 2040.0, 60.0), Some(frame));
    b.cuboid(p(40.0, 2000.0, 0.0), p(790.0, 2040.0, 60.0), Some(frame));
    b.cuboid(p(40.0, 0.0, 10.0), p(790.0, 2000.0, 50.0), Some(leaf));
    b.done()
}

fn window() -> Model {
    let mut b = Build::new(
        "Fenêtre",
        "Fenêtre de 100 × 120 cm à deux vantaux : collée aux murs, elle y découpe son ouverture.",
    )
    .behave(|x| {
        x.is2d = true;
        x.snapto = 2;
        x.cuts_opening = true;
    });
    let frame = b.material("Menuiserie", [244, 244, 240]);
    let glass = b.material("Vitre", [170, 200, 220]);
    b.m.materials[glass].opacity = 0.35;
    b.edge_loop(&[p(0.0, 0.0, 0.0), p(1000.0, 0.0, 0.0), p(1000.0, 1200.0, 0.0), p(0.0, 1200.0, 0.0)]);
    for (x0, x1) in [(0.0, 60.0), (470.0, 530.0), (940.0, 1000.0)] {
        b.cuboid(p(x0, 0.0, 0.0), p(x1, 1200.0, 70.0), Some(frame));
    }
    b.cuboid(p(60.0, 0.0, 0.0), p(940.0, 60.0, 70.0), Some(frame));
    b.cuboid(p(60.0, 1140.0, 0.0), p(940.0, 1200.0, 70.0), Some(frame));
    for (x0, x1) in [(60.0, 470.0), (530.0, 940.0)] {
        b.poly(&[p(x0, 60.0, 35.0), p(x1, 60.0, 35.0), p(x1, 1140.0, 35.0), p(x0, 1140.0, 35.0)], Some(glass));
    }
    b.done()
}

fn stairs() -> Model {
    let mut b =
        Build::new("Escalier droit", "Escalier de 12 marches : 17,5 cm de hauteur, 26 cm de giron, 90 cm de large.");
    let wood = b.material("Marches", [160, 120, 80]);
    for i in 0..12 {
        let (y, z) = (i as f64 * 260.0, i as f64 * 175.0);
        b.cuboid(p(0.0, y, z), p(900.0, y + 300.0, z + 175.0), Some(wood));
    }
    b.done()
}

fn fence() -> Model {
    let mut b = Build::new("Clôture", "Travée de clôture en bois de 2 m, poteaux et lames verticales.");
    let wood = b.material("Bois traité", [132, 116, 88]);
    for x in [0.0, 1900.0] {
        b.cuboid(p(x, 0.0, 0.0), p(x + 100.0, 100.0, 1200.0), Some(wood));
    }
    for z in [250.0, 900.0] {
        b.cuboid(p(100.0, 20.0, z), p(1900.0, 60.0, z + 80.0), Some(wood));
    }
    for i in 0..12 {
        let x = 130.0 + i as f64 * 148.0;
        b.cuboid(p(x, 60.0, 50.0), p(x + 110.0, 80.0, 1150.0), Some(wood));
    }
    b.done()
}

fn street_lamp() -> Model {
    let mut b = Build::new("Lampadaire", "Lampadaire de rue de 4 m.");
    let metal = b.material("Fonte", [52, 58, 62]);
    let light = b.material("Verre dépoli", [252, 246, 220]);
    b.cuboid(p(-150.0, -150.0, 0.0), p(150.0, 150.0, 300.0), Some(metal));
    b.cuboid(p(-50.0, -50.0, 300.0), p(50.0, 50.0, 3700.0), Some(metal));
    b.cuboid(p(-200.0, -200.0, 3700.0), p(200.0, 200.0, 4000.0), Some(light));
    b.cuboid(p(-250.0, -250.0, 4000.0), p(250.0, 250.0, 4060.0), Some(metal));
    b.done()
}

/// A standing figure's outline (1.75 m), drawn here: head, shoulders, arms and legs.
fn figure() -> Model {
    let mut b =
        Build::new("Personnage 2D", "Silhouette de 1,75 m : elle fait toujours face à la caméra.").behave(|x| {
            x.always_face_camera = true;
            x.shadows_face_sun = true;
        });
    let tone = b.material("Silhouette", [70, 92, 120]);
    // The right half, from the crotch down the leg and up to the top of the head; the left half
    // mirrors it (counter-clockwise seen from the front).
    let right = [
        (0.0, 820.0),
        (30.0, 0.0),
        (180.0, 0.0),
        (175.0, 850.0),
        (245.0, 800.0),
        (260.0, 1380.0),
        (200.0, 1460.0),
        (90.0, 1490.0),
        (75.0, 1540.0),
        (100.0, 1620.0),
        (85.0, 1710.0),
        (40.0, 1750.0),
        (0.0, 1755.0),
    ];
    let mut outline: Vec<(f64, f64)> = right.to_vec();
    outline.extend(right[1..right.len() - 1].iter().rev().map(|(x, z)| (-*x, *z)));
    b.outline(&outline, Some(tone));
    b.done()
}

fn tree() -> Model {
    let mut b =
        Build::new("Arbre 2D", "Silhouette d'arbre de 5 m : elle fait toujours face à la caméra.").behave(|x| {
            x.always_face_camera = true;
            x.shadows_face_sun = true;
        });
    let leaf = b.material("Feuillage", [86, 128, 70]);
    // The trunk, then the crown all around from just right of the trunk to just left of it
    // (counter-clockwise seen from the front), its edge waving like foliage.
    let (centre, r) = (3300.0, 1600.0);
    let gap = (130.0f64 / r).asin();
    let (a0, a1) = (-std::f64::consts::FRAC_PI_2 + gap, 3.0 * std::f64::consts::FRAC_PI_2 - gap);
    let mut outline = vec![(-120.0, 0.0), (120.0, 0.0)];
    let n = 48;
    for i in 0..=n {
        let a = a0 + (a1 - a0) * i as f64 / n as f64;
        let wave = 1.0 + 0.07 * (7.0 * a).sin() + 0.04 * (11.0 * a + 1.0).sin();
        let k = if i == 0 || i == n { 1.0 } else { wave };
        outline.push((r * k * a.cos(), centre + r * k * a.sin()));
    }
    b.outline(&outline, Some(leaf));
    b.done()
}

/// The thumbnail: the component seen from the front left and above, on white.
fn thumbnail(gpu: &mut Gpu, m: &Model) -> Vec<u8> {
    let (big, small) = (192u32, 96u32);
    let (mut lo, mut hi) = (DVec3::INFINITY, DVec3::NEG_INFINITY);
    for v in m.root.verts.values() {
        lo = lo.min(v.pos);
        hi = hi.max(v.pos);
    }
    let mut cam = Camera { size: DVec2::new(big as f64, big as f64), ..Default::default() };
    // A gluing component lies in its red-green plane (the wall's): seen from above, it shows as
    // it stands on the wall; a face-me one is seen from the front.
    let b = m.behavior();
    match (b.is2d, b.always_face_camera) {
        (true, _) => cam.set_view(-90.0, 89.9),
        (_, true) => cam.set_view(-90.0, 0.0),
        _ => cam.set_view(-60.0, 20.0),
    }
    cam.zoom_extents(lo, hi);
    let s = ViewStyle { axes: false, sky: false, ground: false, bg_color: [255, 255, 255], ..ViewStyle::default() };
    let shot = gpu.render(m, &s, &cam, &Lighting::default());
    let img = image::RgbaImage::from_raw(shot.w, shot.h, shot.px).expect("pixels");
    let img = image::imageops::resize(&img, small, small, image::imageops::FilterType::Triangle);
    png_rgba(small, small, img.into_raw())
}

/// Every component of the library, with its thumbnail.
pub fn all(gpu: &mut Gpu) -> Vec<Entry> {
    let list = [
        table(),
        chair(),
        bench(),
        shelf(),
        bed(),
        door(),
        window(),
        stairs(),
        fence(),
        street_lamp(),
        figure(),
        tree(),
    ];
    list.into_iter()
        .map(|model| Entry {
            folder: COLLECTIONS[0].0,
            file: super::materials::file_stem(&model.meta.name),
            thumbnail: thumbnail(gpu, &model),
            model,
        })
        .collect()
}
