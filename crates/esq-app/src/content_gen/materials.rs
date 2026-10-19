//! The material library: as many materials, in the same collections, as SketchUp 2026's
//! (`Materials\<collection>\*.skm`), all made here: plain colours, procedural textures
//! (256 px, tileable, JPEG) and patterns (128 px, PNG).

use esq_core::model::Material;

use super::noise::{fbm, fbm2, hash, image, jpeg, mix, png, rgb, scale, smoothstep, value, worley};

/// SketchUp 2026's collections and how many materials each holds (counted in its library).
pub const COLLECTIONS: [(&str, usize); 14] = [
    ("Asphalt & Concrete", 6),
    ("Brick", 7),
    ("Fabric", 6),
    ("Glass", 4),
    ("Ground", 6),
    ("Metal", 6),
    ("Patterns", 122),
    ("Plaster", 4),
    ("Plastic", 4),
    ("Roofing", 4),
    ("Solid Colors", 106),
    ("Stone", 5),
    ("Tile", 4),
    ("Wood", 6),
];

/// One material of the library: its collection (folder), file name and settings.
pub struct Entry {
    pub folder: &'static str,
    pub file: String,
    pub material: Material,
}

const N: u32 = 256;
type Px = [f32; 3];

fn solid(name: &str, c: [u8; 3], opacity: f32, rough: f64, metal: f64) -> Material {
    let mut m = Material { name: name.into(), color: c, opacity, ext: Default::default() };
    pbr(&mut m, rough, metal);
    m
}

fn pbr(m: &mut Material, rough: f64, metal: f64) {
    m.ext.workflow = 1;
    let p = &mut m.ext.pbr;
    p.roughness_enabled = true;
    p.roughness_factor = rough;
    p.metalness_enabled = metal > 0.0;
    p.metallic_factor = metal;
}

/// A textured material: the image (its file name made from the material's), its tile in
/// millimetres; its colour is the image's average, as SketchUp gives it.
fn textured(name: &str, bytes: Vec<u8>, ext: &str, tile_mm: [f64; 2], rough: f64, metal: f64) -> Material {
    let file = format!("{}.{ext}", file_stem(name));
    let mut t = crate::mat_panel::texture_from_bytes(&file, bytes).expect("texture lisible");
    t.size = tile_mm;
    let mut m = Material { name: name.into(), color: t.avg, opacity: 1.0, ext: Default::default() };
    m.ext.texture = Some(t);
    pbr(&mut m, rough, metal);
    m
}

/// A file name for a material name (ASCII letters, digits, spaces and dashes).
pub fn file_stem(name: &str) -> String {
    let plain: String = name
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'É' => 'E',
            'À' => 'A',
            '×' => 'x',
            c if c.is_ascii_alphanumeric() || c == ' ' || c == '-' => c,
            _ => ' ',
        })
        .collect();
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn tex(name: &str, tile_mm: f64, rough: f64, metal: f64, f: impl Fn(f32, f32) -> Px) -> Material {
    textured(name, jpeg(N, N, &image(N, f)), "jpg", [tile_mm, tile_mm], rough, metal)
}

/// Varies a colour by a noise value around 1 (± `amount`).
fn vary(c: Px, n: f32, amount: f32) -> Px {
    scale(c, 1.0 + (n - 0.5) * 2.0 * amount)
}

// --------------------------------------------------------------------------- building blocks

/// Running-bond masonry: `rows` courses of `cols` units (rows even), joints `joint` (fraction
/// of a course's height). Returns (in joint, unit id, position in the unit 0–1).
fn bond(u: f32, v: f32, rows: i32, cols: i32, joint: f32, seed: u32) -> (bool, f32, f32, f32) {
    let y = v * rows as f32;
    let row = y.floor() as i32;
    let fy = y - row as f32;
    let shift = if row % 2 == 1 { 0.5 } else { 0.0 };
    let x = u * cols as f32 + shift;
    let col = x.floor() as i32;
    let fx = x - col as f32;
    let jx = joint * cols as f32 / rows as f32;
    let id = hash(col.rem_euclid(cols), row.rem_euclid(rows), seed);
    let in_joint = fy < joint || fx < jx;
    (in_joint, id, fx, fy)
}

/// Bricks of 200 × 75 mm with their joints (16 courses of 6 on a 1.2 m tile).
fn bricks(u: f32, v: f32, colors: &[Px], mortar: Px, seed: u32, wear: f32) -> Px {
    let (joint, id, fx, fy) = bond(u, v, 16, 6, 0.13, seed);
    let n = fbm(u, v, 16, 4, seed + 1);
    if joint {
        return vary(mortar, n, 0.12);
    }
    let c = colors[(id * colors.len() as f32) as usize % colors.len()];
    let fine = value(u * 128.0, v * 128.0, 128, seed + 2);
    let edge = smoothstep(0.0, 0.08, fx.min(1.0 - fx)) * smoothstep(0.0, 0.12, fy.min(1.0 - fy));
    let c = vary(c, fine, 0.08 + wear * 0.1);
    let c = vary(c, n, wear * 0.3);
    scale(c, 0.85 + 0.15 * edge)
}

/// Wood grain along u: growth rings bent by noise.
fn grain(u: f32, v: f32, light: Px, dark: Px, rings: f32, seed: u32) -> Px {
    let bend = fbm2(u, v, 2, 4, 4, seed);
    let t = (v * rings + bend * 3.0).fract();
    let ring = smoothstep(0.0, 0.25, t) * (1.0 - smoothstep(0.55, 1.0, t));
    let streak = fbm2(u, v, 2, 64, 3, seed + 7);
    let c = mix(dark, light, 0.35 + 0.65 * ring);
    vary(c, streak, 0.12)
}

/// Planks along u: `rows` boards, each cut in `pieces` lengths, staggered. Returns the colour
/// given by `board(u, v, id)` with dark joints.
fn planks(u: f32, v: f32, rows: i32, pieces: i32, joint: f32, seed: u32, board: impl Fn(f32, f32, f32) -> Px) -> Px {
    let y = v * rows as f32;
    let row = y.floor() as i32;
    let fy = y - row as f32;
    let shift = hash(row.rem_euclid(rows), 0, seed);
    let x = u * pieces as f32 + shift * pieces as f32;
    let piece = x.floor() as i32;
    let fx = x - piece as f32;
    let id = hash(piece.rem_euclid(pieces), row.rem_euclid(rows), seed + 1);
    let c = board(u + id * 3.7, v + id * 1.3, id);
    let jx = joint * pieces as f32 / rows as f32 * 0.5;
    if fy < joint || fx < jx { scale(c, 0.45) } else { c }
}

/// Cells around the points of a staggered lattice (`cols` × `rows`, rows even): hexagons.
fn hex_cells(u: f32, v: f32, cols: i32, rows: i32) -> (f32, u32) {
    let aspect = rows as f32 / cols as f32;
    let (mut d1, mut d2, mut best) = (f32::MAX, f32::MAX, 0u32);
    let (cx, cy) = ((u * cols as f32) as i32, (v * rows as f32) as i32);
    for j in cy - 2..=cy + 2 {
        for i in cx - 2..=cx + 2 {
            let px = (i as f32 + if j.rem_euclid(2) == 1 { 0.5 } else { 0.0 } + 0.5) / cols as f32;
            let py = (j as f32 + 0.5) / rows as f32;
            let (dx, dy) = ((u - px) * cols as f32, (v - py) * rows as f32 / aspect * 0.866);
            let d = (dx * dx + dy * dy).sqrt();
            if d < d1 {
                d2 = d1;
                d1 = d;
                best = (j.rem_euclid(rows) * cols + i.rem_euclid(cols)) as u32;
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d2 - d1, best)
}

// --------------------------------------------------------------------------- collections

fn asphalt_concrete() -> Vec<Material> {
    vec![
        tex("Asphalte neuf", 1000.0, 0.8, 0.0, |u, v| {
            let (d, _, id) = worley(u, v, 40, 11);
            let base = vary(rgb([50, 50, 53]), value(u * 256.0, v * 256.0, 256, 12), 0.25);
            if d < 0.18 && hash(id as i32, 0, 13) > 0.8 {
                vary(rgb([120, 118, 112]), hash(id as i32, 1, 13), 0.3)
            } else {
                base
            }
        }),
        tex("Asphalte usé", 1500.0, 0.85, 0.0, |u, v| {
            let patch = fbm(u, v, 3, 4, 21);
            let base = mix(rgb([70, 70, 70]), rgb([110, 108, 102]), smoothstep(0.35, 0.7, patch));
            let (d, _, id) = worley(u, v, 48, 22);
            let c = vary(base, value(u * 256.0, v * 256.0, 256, 23), 0.2);
            if d < 0.2 && hash(id as i32, 2, 24) > 0.7 { scale(c, 1.25) } else { c }
        }),
        tex("Béton brut", 1500.0, 0.9, 0.0, |u, v| {
            let blot = fbm(u, v, 4, 5, 31);
            let c = vary(rgb([165, 163, 157]), blot, 0.12);
            let pore = value(u * 128.0, v * 128.0, 128, 32);
            let c = if pore > 0.93 { scale(c, 0.7) } else { c };
            vary(c, value(u * 256.0, v * 256.0, 256, 33), 0.05)
        }),
        tex("Béton lissé", 2000.0, 0.35, 0.0, |u, v| {
            let blot = fbm(u, v, 3, 5, 41);
            vary(rgb([188, 186, 180]), blot, 0.08)
        }),
        tex("Béton désactivé", 600.0, 0.9, 0.0, |u, v| {
            let (d1, d2, id) = worley(u, v, 22, 51);
            let stones = [rgb([190, 175, 150]), rgb([140, 135, 128]), rgb([170, 150, 120]), rgb([110, 100, 92])];
            if d2 - d1 < 0.12 || d1 > 0.62 {
                vary(rgb([150, 146, 138]), value(u * 256.0, v * 256.0, 256, 52), 0.1)
            } else {
                let c = stones[(hash(id as i32, 3, 53) * 4.0) as usize % 4];
                scale(c, 1.08 - d1 * 0.4)
            }
        }),
        tex("Béton banché", 1800.0, 0.85, 0.0, |u, v| {
            planks(u, v, 6, 1, 0.015, 61, |u, v, id| {
                let g = fbm2(u, v, 2, 24, 4, 62);
                vary(rgb([158 + (id * 12.0) as u8, 156 + (id * 12.0) as u8, 150]), g, 0.1)
            })
        }),
    ]
}

fn brick() -> Vec<Material> {
    let reds = [rgb([150, 62, 45]), rgb([165, 75, 52]), rgb([135, 55, 42]), rgb([175, 88, 60])];
    let beige = [rgb([214, 190, 140]), rgb([200, 172, 122]), rgb([224, 204, 160]), rgb([190, 160, 112])];
    let old = [rgb([140, 70, 50]), rgb([120, 80, 62]), rgb([160, 96, 70]), rgb([105, 60, 48]), rgb([150, 110, 85])];
    let flamed = [rgb([90, 45, 38]), rgb([160, 70, 45]), rgb([190, 110, 70]), rgb([70, 50, 45])];
    let grey = [rgb([120, 118, 115]), rgb([135, 132, 128]), rgb([105, 104, 102])];
    let mortar = rgb([200, 195, 185]);
    vec![
        tex("Brique rouge", 1200.0, 0.85, 0.0, move |u, v| bricks(u, v, &reds, mortar, 101, 0.3)),
        tex("Brique beige", 1200.0, 0.85, 0.0, move |u, v| bricks(u, v, &beige, rgb([215, 210, 200]), 102, 0.25)),
        tex("Brique ancienne", 1200.0, 0.9, 0.0, move |u, v| bricks(u, v, &old, rgb([170, 162, 150]), 103, 0.8)),
        tex("Brique flammée", 1200.0, 0.85, 0.0, move |u, v| bricks(u, v, &flamed, mortar, 104, 0.5)),
        tex("Brique grise", 1200.0, 0.85, 0.0, move |u, v| bricks(u, v, &grey, rgb([175, 172, 168]), 105, 0.3)),
        tex("Parpaing", 1200.0, 0.9, 0.0, |u, v| {
            let (joint, id, _, _) = bond(u, v, 6, 3, 0.05, 106);
            let n = value(u * 256.0, v * 256.0, 256, 107);
            if joint {
                vary(rgb([150, 148, 142]), n, 0.1)
            } else {
                let pore = if n > 0.9 { 0.75 } else { 1.0 };
                scale(vary(rgb([128 + (id * 20.0) as u8, 127 + (id * 20.0) as u8, 124]), n, 0.12), pore)
            }
        }),
        tex("Bardage à clin gris", 1200.0, 0.6, 0.0, |u, v| {
            let y = v * 8.0;
            let f = y.fract();
            let shade = 0.7 + 0.3 * smoothstep(0.0, 0.25, f);
            let g = fbm2(u, v, 2, 32, 3, 108);
            scale(vary(rgb([150, 158, 162]), g, 0.05), shade)
        }),
    ]
}

fn fabric() -> Vec<Material> {
    let weave = |u: f32, v: f32, threads: f32, base: Px, seed: u32| {
        let a = (u * threads * std::f32::consts::TAU).sin();
        let b = (v * threads * std::f32::consts::TAU).sin();
        let over = if (((u * threads).floor() + (v * threads).floor()) as i32).rem_euclid(2) == 0 { a } else { b };
        let slub = fbm2(u, v, 4, 64, 3, seed);
        vary(scale(base, 0.85 + 0.15 * over.abs()), slub, 0.15)
    };
    vec![
        tex("Lin naturel", 200.0, 0.9, 0.0, move |u, v| weave(u, v, 64.0, rgb([205, 195, 170]), 201)),
        tex("Toile de jute", 200.0, 0.95, 0.0, move |u, v| weave(u, v, 24.0, rgb([165, 132, 88]), 202)),
        tex("Moquette bouclée", 400.0, 1.0, 0.0, |u, v| {
            let (d, _, id) = worley(u, v, 64, 203);
            let c = vary(rgb([150, 140, 125]), hash(id as i32, 4, 204), 0.15);
            scale(c, 1.1 - d * 0.5)
        }),
        tex("Velours bleu", 400.0, 0.8, 0.0, |u, v| {
            let sheen = fbm(u, v, 3, 5, 205);
            vary(rgb([45, 62, 120]), sheen, 0.25)
        }),
        tex("Tweed gris", 300.0, 0.95, 0.0, |u, v| {
            let n = value(u * 128.0, v * 128.0, 128, 206);
            let fleck = hash((u * 256.0) as i32, (v * 256.0) as i32, 207);
            let base = vary(rgb([120, 118, 112]), n, 0.2);
            if fleck > 0.96 {
                rgb([170, 90, 60])
            } else if fleck < 0.04 {
                rgb([60, 80, 110])
            } else {
                base
            }
        }),
        tex("Cuir brun", 400.0, 0.6, 0.0, |u, v| {
            let (d1, d2, _) = worley(u, v, 20, 208);
            let crease = smoothstep(0.0, 0.08, d2 - d1);
            let c = vary(rgb([112, 72, 42]), fbm(u, v, 6, 4, 209), 0.2);
            scale(c, 0.75 + 0.25 * crease)
        }),
    ]
}

fn glass() -> Vec<Material> {
    vec![
        solid("Verre clair", [200, 225, 230], 0.3, 0.05, 0.0),
        solid("Verre fumé", [90, 95, 100], 0.5, 0.05, 0.0),
        solid("Verre bleuté", [150, 190, 220], 0.35, 0.05, 0.0),
        solid("Verre dépoli", [230, 235, 235], 0.65, 0.6, 0.0),
    ]
}

fn ground() -> Vec<Material> {
    let blades = |u: f32, v: f32, a: Px, b: Px, seed: u32| {
        let streak = fbm2(u, v, 64, 8, 3, seed);
        let patch = fbm(u, v, 4, 4, seed + 1);
        let c = mix(a, b, smoothstep(0.3, 0.7, patch));
        vary(c, streak, 0.3)
    };
    vec![
        tex("Gazon", 1000.0, 0.95, 0.0, move |u, v| blades(u, v, rgb([70, 120, 45]), rgb([95, 145, 55]), 301)),
        tex("Herbe sèche", 1000.0, 0.95, 0.0, move |u, v| blades(u, v, rgb([165, 150, 90]), rgb([130, 120, 70]), 302)),
        tex("Gravier", 800.0, 0.95, 0.0, |u, v| {
            let (d1, d2, id) = worley(u, v, 40, 303);
            let shades = [rgb([150, 148, 140]), rgb([120, 115, 108]), rgb([175, 168, 155]), rgb([95, 92, 90])];
            let c = shades[(hash(id as i32, 5, 304) * 4.0) as usize % 4];
            if d2 - d1 < 0.06 { scale(c, 0.45) } else { scale(c, 1.1 - d1 * 0.5) }
        }),
        tex("Terre", 1500.0, 1.0, 0.0, |u, v| {
            let n = fbm(u, v, 6, 5, 305);
            let clod = worley(u, v, 24, 306).0;
            scale(vary(rgb([110, 80, 55]), n, 0.25), 1.05 - clod * 0.3)
        }),
        tex("Sable", 1500.0, 0.95, 0.0, |u, v| {
            let warp = fbm(u, v, 3, 3, 307);
            let ripple = ((v * 18.0 + warp * 2.0) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
            let grain = value(u * 256.0, v * 256.0, 256, 308);
            vary(scale(rgb([214, 192, 150]), 0.92 + 0.08 * ripple), grain, 0.08)
        }),
        tex("Paillis", 800.0, 1.0, 0.0, |u, v| {
            let chips = fbm2(u, v, 24, 6, 3, 309);
            let id = value(u * 24.0, v * 6.0, 24, 310);
            let c = mix(rgb([95, 60, 35]), rgb([150, 100, 60]), id);
            scale(c, 0.6 + 0.6 * chips)
        }),
    ]
}

fn metal() -> Vec<Material> {
    let brushed = |u: f32, v: f32, c: Px, seed: u32| vary(c, fbm2(u, v, 2, 128, 3, seed), 0.08);
    vec![
        tex("Acier brossé", 500.0, 0.35, 1.0, move |u, v| brushed(u, v, rgb([160, 162, 165]), 401)),
        tex("Aluminium", 500.0, 0.3, 1.0, move |u, v| brushed(u, v, rgb([196, 198, 200]), 402)),
        tex("Cuivre", 500.0, 0.3, 1.0, |u, v| {
            let c = vary(rgb([184, 115, 70]), fbm(u, v, 4, 4, 403), 0.12);
            let patina = fbm(u, v, 6, 4, 404);
            if patina > 0.72 { mix(c, rgb([90, 150, 125]), (patina - 0.72) * 3.0) } else { c }
        }),
        tex("Laiton", 500.0, 0.25, 1.0, move |u, v| brushed(u, v, rgb([200, 166, 82]), 405)),
        tex("Acier rouillé", 800.0, 0.85, 0.3, |u, v| {
            let n = fbm(u, v, 5, 5, 406);
            let c = mix(rgb([140, 65, 30]), rgb([95, 50, 30]), n);
            let pit = value(u * 128.0, v * 128.0, 128, 407);
            if pit > 0.9 { scale(c, 0.6) } else { vary(c, pit, 0.15) }
        }),
        tex("Acier galvanisé", 800.0, 0.4, 1.0, |u, v| {
            let (_, _, id) = worley(u, v, 10, 408);
            let k = 0.85 + 0.3 * hash(id as i32, 6, 409);
            scale(rgb([165, 170, 172]), k)
        }),
    ]
}

fn plaster() -> Vec<Material> {
    vec![
        tex("Enduit blanc", 1500.0, 0.9, 0.0, |u, v| vary(rgb([235, 233, 228]), fbm(u, v, 4, 5, 501), 0.05)),
        tex("Enduit taloché ocre", 1500.0, 0.9, 0.0, |u, v| {
            let t = fbm(u, v, 6, 4, 502);
            let marks = smoothstep(0.45, 0.55, t);
            scale(vary(rgb([212, 172, 112]), fbm(u, v, 3, 3, 503), 0.08), 0.92 + 0.08 * marks)
        }),
        tex("Crépi gris", 1000.0, 1.0, 0.0, |u, v| {
            let (d, _, _) = worley(u, v, 48, 504);
            scale(vary(rgb([182, 180, 174]), fbm(u, v, 4, 3, 505), 0.06), 1.08 - d * 0.35)
        }),
        tex("Stuc rose", 1500.0, 0.8, 0.0, |u, v| vary(rgb([222, 182, 170]), fbm(u, v, 5, 5, 506), 0.1)),
    ]
}

fn plastic() -> Vec<Material> {
    vec![
        solid("Plastique blanc brillant", [240, 240, 238], 1.0, 0.2, 0.0),
        solid("Plastique noir mat", [30, 30, 32], 1.0, 0.7, 0.0),
        solid("Plastique rouge", [200, 30, 35], 1.0, 0.3, 0.0),
        solid("Plastique bleu translucide", [60, 120, 220], 0.6, 0.2, 0.0),
    ]
}

fn roofing() -> Vec<Material> {
    vec![
        tex("Tuile canal", 1200.0, 0.8, 0.0, |u, v| {
            let row = (v * 6.0).floor() as i32;
            let fy = (v * 6.0).fract();
            let x = u * 8.0 + if row % 2 == 1 { 0.5 } else { 0.0 };
            let id = hash((x.floor() as i32).rem_euclid(8), row.rem_euclid(6), 601);
            let round = (x.fract() * std::f32::consts::PI).sin();
            let c = vary(mix(rgb([175, 80, 45]), rgb([150, 95, 60]), id), fbm(u, v, 8, 3, 602), 0.15);
            scale(c, (0.55 + 0.45 * round) * (0.8 + 0.2 * smoothstep(0.0, 0.3, fy)))
        }),
        tex("Ardoise", 1000.0, 0.6, 0.0, |u, v| {
            let (joint, id, _, fy) = bond(u, v, 10, 5, 0.04, 603);
            let c = vary(rgb([70, 75, 85]), id, 0.18);
            let c = vary(c, fbm2(u, v, 4, 16, 3, 604), 0.1);
            if joint { scale(c, 0.4) } else { scale(c, 0.85 + 0.15 * fy) }
        }),
        tex("Bac acier", 1000.0, 0.4, 0.8, |u, v| {
            let x = (u * 8.0).fract();
            let rib = if x < 0.12 { 0.75 + 0.5 * (x / 0.12 * std::f32::consts::PI).sin() } else { 1.0 };
            scale(vary(rgb([82, 88, 94]), fbm2(u, v, 2, 32, 2, 605), 0.05), rib)
        }),
        tex("Bardeau bitumé", 1000.0, 0.95, 0.0, |u, v| {
            let (joint, id, _, fy) = bond(u, v, 8, 4, 0.03, 606);
            let granule = value(u * 256.0, v * 256.0, 256, 607);
            let c = vary(mix(rgb([70, 62, 58]), rgb([95, 78, 66]), id), granule, 0.3);
            if joint { scale(c, 0.35) } else { scale(c, 0.8 + 0.2 * fy) }
        }),
    ]
}

fn stone() -> Vec<Material> {
    vec![
        tex("Granit gris", 600.0, 0.4, 0.0, |u, v| {
            let a = value(u * 96.0, v * 96.0, 96, 701);
            let b = value(u * 192.0, v * 192.0, 192, 702);
            let base = mix(rgb([150, 148, 146]), rgb([200, 198, 196]), b);
            if a > 0.72 {
                rgb([40, 40, 42])
            } else if a < 0.12 {
                rgb([190, 160, 155])
            } else {
                base
            }
        }),
        tex("Marbre blanc", 1000.0, 0.2, 0.0, |u, v| {
            // Whole turns along the diagonal: the veins meet across the tile's edges.
            let warp = fbm(u, v, 3, 6, 703);
            let t = (u + v) + warp * 1.5;
            let vein = (t * std::f32::consts::TAU).sin().abs();
            let fine = ((t * 3.0 + fbm(u, v, 6, 4, 705)) * std::f32::consts::TAU).sin().abs();
            let base = vary(rgb([236, 234, 230]), fbm(u, v, 5, 4, 704), 0.04);
            let c = mix(rgb([150, 152, 158]), base, 0.35 + 0.65 * smoothstep(0.0, 0.3, vein));
            mix(rgb([175, 176, 180]), c, 0.5 + 0.5 * smoothstep(0.0, 0.08, fine))
        }),
        tex("Grès", 1200.0, 0.85, 0.0, |u, v| {
            let warp = fbm(u, v, 3, 4, 705);
            let band = ((v * 5.0 + warp * 1.5) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
            let c = mix(rgb([200, 160, 110]), rgb([175, 130, 85]), band);
            vary(c, value(u * 256.0, v * 256.0, 256, 706), 0.08)
        }),
        tex("Pierre de taille", 2000.0, 0.8, 0.0, |u, v| {
            let (joint, id, _, _) = bond(u, v, 4, 2, 0.02, 707);
            let c = vary(rgb([205, 192, 165]), id, 0.08);
            let c = vary(c, fbm(u, v, 8, 4, 708), 0.08);
            if joint { rgb([170, 165, 155]) } else { c }
        }),
        tex("Moellons", 1500.0, 0.9, 0.0, |u, v| {
            let (d1, d2, id) = worley(u, v, 6, 709);
            let stones = [rgb([150, 140, 125]), rgb([120, 112, 100]), rgb([170, 160, 140]), rgb([135, 120, 105])];
            let c = stones[(hash(id as i32, 7, 710) * 4.0) as usize % 4];
            if d2 - d1 < 0.1 {
                vary(rgb([185, 180, 170]), value(u * 256.0, v * 256.0, 256, 711), 0.1)
            } else {
                vary(c, fbm(u, v, 12, 3, 712), 0.18)
            }
        }),
    ]
}

fn tile() -> Vec<Material> {
    vec![
        tex("Carrelage blanc 20×20", 1000.0, 0.15, 0.0, |u, v| {
            let (fx, fy) = ((u * 5.0).fract(), (v * 5.0).fract());
            if fx < 0.03 || fy < 0.03 {
                rgb([185, 183, 178])
            } else {
                vary(rgb([240, 240, 236]), fbm(u, v, 5, 3, 801), 0.02)
            }
        }),
        tex("Carreaux de ciment", 800.0, 0.5, 0.0, |u, v| {
            let (cx, cy) = ((u * 4.0).floor() as i32, (v * 4.0).floor() as i32);
            let (fx, fy) = ((u * 4.0).fract(), (v * 4.0).fract());
            let wear = fbm(u, v, 8, 4, 802);
            let c = if (cx + cy) % 2 == 0 { rgb([40, 40, 42]) } else { rgb([228, 224, 214]) };
            if fx < 0.02 || fy < 0.02 { rgb([150, 148, 140]) } else { vary(c, wear, 0.08) }
        }),
        tex("Mosaïque bleue", 400.0, 0.2, 0.0, |u, v| {
            let (cx, cy) = ((u * 16.0).floor() as i32, (v * 16.0).floor() as i32);
            let (fx, fy) = ((u * 16.0).fract(), (v * 16.0).fract());
            let id = hash(cx, cy, 803);
            let c = mix(rgb([30, 90, 160]), rgb([90, 160, 210]), id);
            if fx < 0.08 || fy < 0.08 { rgb([210, 210, 205]) } else { c }
        }),
        tex("Tomettes", 800.0, 0.7, 0.0, |u, v| {
            let (edge, id) = hex_cells(u, v, 6, 6);
            let c = vary(rgb([170, 85, 50]), hash(id as i32, 8, 804), 0.15);
            if edge < 0.08 { rgb([175, 165, 150]) } else { vary(c, fbm(u, v, 8, 3, 805), 0.1) }
        }),
    ]
}

fn wood() -> Vec<Material> {
    vec![
        tex("Chêne clair", 1000.0, 0.6, 0.0, |u, v| grain(u, v, rgb([196, 156, 106]), rgb([150, 108, 64]), 9.0, 901)),
        tex("Noyer", 1000.0, 0.55, 0.0, |u, v| grain(u, v, rgb([118, 76, 48]), rgb([70, 42, 26]), 11.0, 902)),
        tex("Pin", 1000.0, 0.65, 0.0, |u, v| grain(u, v, rgb([220, 185, 125]), rgb([185, 132, 72]), 7.0, 903)),
        tex("Parquet chêne", 1200.0, 0.45, 0.0, |u, v| {
            planks(u, v, 8, 2, 0.02, 904, |u, v, id| {
                let c = grain(u, v, rgb([200, 160, 108]), rgb([155, 112, 68]), 10.0, 905);
                scale(c, 0.9 + 0.2 * id)
            })
        }),
        tex("Planches grises vieillies", 1200.0, 0.9, 0.0, |u, v| {
            planks(u, v, 5, 1, 0.03, 906, |u, v, id| {
                let c = grain(u, v, rgb([165, 160, 150]), rgb([110, 105, 98]), 6.0, 907);
                scale(c, 0.85 + 0.25 * id)
            })
        }),
        tex("Contreplaqué", 1200.0, 0.6, 0.0, |u, v| {
            grain(u, v, rgb([220, 195, 152]), rgb([190, 158, 110]), 4.0, 908)
        }),
    ]
}

// --------------------------------------------------------------------------- plain colours

/// HSL (degrees, 0–1, 0–1) to sRGB 0–255.
fn hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h / 60.0).rem_euclid(6.0);
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r, g, b].map(|v| ((v + m).clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
}

/// 12 hues in 8 shades each, then 10 greys from white to black: 106 colours.
fn solid_colors() -> Vec<(String, Material)> {
    const HUES: [(&str, f32); 12] = [
        ("Rouge", 0.0),
        ("Orange", 28.0),
        ("Jaune", 52.0),
        ("Vert-jaune", 80.0),
        ("Vert", 120.0),
        ("Émeraude", 155.0),
        ("Cyan", 185.0),
        ("Azur", 210.0),
        ("Bleu", 235.0),
        ("Violet", 270.0),
        ("Magenta", 305.0),
        ("Rose", 335.0),
    ];
    let mut out = Vec::new();
    for (i, (name, h)) in HUES.iter().enumerate() {
        let letter = (b'A' + i as u8) as char;
        for s in 0..8 {
            let l = 0.88 - s as f32 * 0.09;
            let sat = 0.55 + 0.4 * (1.0 - ((s as f32 - 3.5) / 3.5).abs());
            let n = format!("{name} {}", s + 1);
            out.push((format!("{letter}{} {}", s + 1, file_stem(&n)), solid(&n, hsl(*h, sat, l), 1.0, 0.6, 0.0)));
        }
    }
    let greys = ["Blanc", "Gris 1", "Gris 2", "Gris 3", "Gris 4", "Gris 5", "Gris 6", "Gris 7", "Gris 8", "Noir"];
    for (i, n) in greys.iter().enumerate() {
        let g = (255.0 * (1.0 - i as f32 / 9.0)).round() as u8;
        out.push((format!("M{:02} {n}", i + 1), solid(n, [g, g, g], 1.0, 0.6, 0.0)));
    }
    out
}

// --------------------------------------------------------------------------- patterns

const PATTERN_KINDS: [&str; 14] = [
    "Damier",
    "Rayures",
    "Rayures diagonales",
    "Pois",
    "Quadrillage",
    "Losanges",
    "Chevrons",
    "Vagues",
    "Écailles",
    "Hexagones",
    "Triangles",
    "Vichy",
    "Appareil de briques",
    "Cercles",
];

const SCHEMES: [(&str, [u8; 3], [u8; 3]); 9] = [
    ("noir et blanc", [245, 245, 242], [25, 25, 28]),
    ("bleu marine et crème", [240, 232, 210], [30, 45, 90]),
    ("rouge et blanc", [248, 246, 242], [190, 35, 40]),
    ("vert sapin et sable", [225, 210, 175], [30, 85, 60]),
    ("ocre et brun", [215, 165, 80], [95, 60, 35]),
    ("gris et anthracite", [175, 177, 180], [55, 58, 62]),
    ("jaune et gris", [240, 200, 50], [90, 92, 95]),
    ("turquoise et blanc", [245, 248, 248], [20, 150, 150]),
    ("violet et lilas", [205, 185, 225], [85, 45, 120]),
];

/// How much of the "ink" colour covers (u, v) for a pattern kind (0 background, 1 ink).
fn pattern(kind: usize, u: f32, v: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let cell = |k: f32| ((u * k).fract(), (v * k).fract(), (u * k).floor() as i32, (v * k).floor() as i32);
    let on = |b: bool| if b { 1.0 } else { 0.0 };
    match kind {
        0 => {
            let (_, _, x, y) = cell(4.0);
            on((x + y) % 2 == 0)
        }
        1 => on((v * 8.0).fract() < 0.5),
        2 => on(((u + v) * 6.0).fract() < 0.5),
        3 => {
            let (fx, fy, x, y) = cell(4.0);
            let (fx, fy) = if y % 2 == 1 { ((fx + 0.5).fract(), fy) } else { (fx, fy) };
            let _ = x;
            on((fx - 0.5).powi(2) + (fy - 0.5).powi(2) < 0.06)
        }
        4 => {
            let (fx, fy, _, _) = cell(8.0);
            on(fx < 0.12 || fy < 0.12)
        }
        5 => {
            let (fx, fy, _, _) = cell(3.0);
            on((fx - 0.5).abs() + (fy - 0.5).abs() < 0.35)
        }
        6 => {
            let x = (u * 4.0).fract();
            let zig = (x - 0.5).abs() * 0.5;
            on(((v * 6.0) + zig).fract() < 0.4)
        }
        7 => on(((v * 6.0) + 0.12 * (u * TAU * 2.0).sin()).fract() < 0.45),
        8 => {
            let row = (v * 6.0).floor();
            let fy = (v * 6.0).fract();
            let x = u * 6.0 + if (row as i32) % 2 == 1 { 0.5 } else { 0.0 };
            let fx = x.fract();
            let d = ((fx - 0.5).powi(2) + fy.powi(2)).sqrt();
            on(d < 0.55 && d > 0.42)
        }
        9 => {
            let (edge, _) = hex_cells(u, v, 4, 4);
            on(edge < 0.09)
        }
        10 => {
            let (fx, fy, _, _) = cell(4.0);
            on(fy > (fx - 0.5).abs() * 2.0)
        }
        11 => {
            let a = (u * 6.0).fract() < 0.5;
            let b = (v * 6.0).fract() < 0.5;
            0.5 * (on(a) + on(b))
        }
        12 => {
            let (joint, ..) = bond(u, v, 8, 4, 0.12, 1);
            on(joint)
        }
        _ => {
            let (fx, fy, _, _) = cell(2.0);
            let r = ((fx - 0.5).powi(2) + (fy - 0.5).powi(2)).sqrt();
            on((r * 8.0 * PI).sin() > 0.0 && r < 0.5)
        }
    }
}

fn patterns() -> Vec<Material> {
    let n = 128u32;
    let mut out = Vec::new();
    'all: for (k, kind) in PATTERN_KINDS.iter().enumerate() {
        for (scheme, bg, ink) in SCHEMES {
            if out.len() == 122 {
                break 'all;
            }
            let name = format!("{kind} {scheme}");
            // Four samples per pixel: smooth edges.
            let px = image(n, |u, v| {
                let d = 0.25 / n as f32;
                let t = [(-d, -d), (d, -d), (-d, d), (d, d)]
                    .iter()
                    .map(|(a, b)| pattern(k, (u + a).rem_euclid(1.0), (v + b).rem_euclid(1.0)))
                    .sum::<f32>()
                    / 4.0;
                mix(rgb(bg), rgb(ink), t)
            });
            out.push(textured(&name, png(n, n, &px), "png", [300.0, 300.0], 0.6, 0.0));
        }
    }
    out
}

/// Every material of the library, by collection.
pub fn all() -> Vec<Entry> {
    let groups: Vec<(&'static str, Vec<Material>)> = vec![
        ("Asphalt & Concrete", asphalt_concrete()),
        ("Brick", brick()),
        ("Fabric", fabric()),
        ("Glass", glass()),
        ("Ground", ground()),
        ("Metal", metal()),
        ("Patterns", patterns()),
        ("Plaster", plaster()),
        ("Plastic", plastic()),
        ("Roofing", roofing()),
        ("Stone", stone()),
        ("Tile", tile()),
        ("Wood", wood()),
    ];
    let mut out = Vec::new();
    for (folder, list) in groups {
        for m in list {
            out.push(Entry { folder, file: file_stem(&m.name), material: m });
        }
    }
    for (file, m) in solid_colors() {
        out.push(Entry { folder: "Solid Colors", file, material: m });
    }
    out
}
