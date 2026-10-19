//! The environments: as many as SketchUp 2026 ships (18), all computed here: an
//! equirectangular high dynamic range panorama (1024 × 512, Radiance HDR) of a sky (gradient,
//! sun, clouds, stars), a ground (grass, snow, sand, sea…) and a skyline, written as `.ske`
//! files with their settings, the sun position and a thumbnail.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use super::noise::{fbm, hash, hdr, mix, png_rgba, scale, smoothstep, value};

pub const COUNT: usize = 18;
const W: u32 = 1024;
const H: u32 = 512;

#[derive(Clone, Copy, PartialEq)]
enum Ground {
    Grass,
    DryGrass,
    Snow,
    Sand,
    Sea,
    Asphalt,
    Dark,
    Studio,
}

#[derive(Clone, Copy, PartialEq)]
enum Skyline {
    None,
    Mountains,
    SnowyMountains,
    Trees,
    Hills,
    City,
}

struct Sky {
    name: &'static str,
    desc: &'static str,
    zenith: [f32; 3],
    horizon: [f32; 3],
    /// Column (0–1), elevation (degrees), radiance of the disc, colour.
    sun: Option<(f32, f32, f32, [f32; 3])>,
    /// Glow around the sun (also when it is below the horizon).
    glow: f32,
    /// Cloud cover 0–1, cloud colour.
    clouds: f32,
    cloud: [f32; 3],
    ground: Ground,
    skyline: Skyline,
    stars: bool,
}

const fn sky(name: &'static str, desc: &'static str, zenith: [f32; 3], horizon: [f32; 3]) -> Sky {
    Sky {
        name,
        desc,
        zenith,
        horizon,
        sun: None,
        glow: 0.0,
        clouds: 0.0,
        cloud: [1.0, 1.0, 1.0],
        ground: Ground::Grass,
        skyline: Skyline::None,
        stars: false,
    }
}

fn skies() -> Vec<Sky> {
    let sun = |u: f32, el: f32, k: f32, c: [f32; 3]| Some((u, el, k, c));
    let white_sun = [1.0, 0.97, 0.9];
    vec![
        Sky {
            sun: sun(0.30, 62.0, 900.0, white_sun),
            glow: 0.6,
            clouds: 0.08,
            ..sky("Ciel de midi", "Ciel dégagé, soleil haut, prairie.", [0.18, 0.36, 0.85], [0.75, 0.85, 1.0])
        },
        Sky {
            sun: sun(0.22, 48.0, 500.0, white_sun),
            glow: 1.2,
            clouds: 0.3,
            cloud: [0.95, 0.95, 0.97],
            ground: Ground::DryGrass,
            ..sky("Ciel voilé", "Voile clair devant le soleil, herbe sèche.", [0.45, 0.55, 0.8], [0.9, 0.92, 0.95])
        },
        Sky {
            sun: sun(0.62, 45.0, 800.0, white_sun),
            glow: 0.6,
            clouds: 0.38,
            ..sky("Quelques nuages", "Ciel bleu et cumulus épars, prairie.", [0.16, 0.34, 0.8], [0.72, 0.82, 0.98])
        },
        Sky {
            clouds: 1.0,
            cloud: [0.62, 0.63, 0.65],
            ..sky(
                "Ciel couvert",
                "Couverture nuageuse uniforme, lumière douce sans ombre portée.",
                [0.55, 0.56, 0.58],
                [0.8, 0.8, 0.82],
            )
        },
        Sky {
            sun: sun(0.25, 4.0, 300.0, [1.0, 0.62, 0.35]),
            glow: 3.0,
            clouds: 0.2,
            cloud: [1.0, 0.75, 0.6],
            skyline: Skyline::Hills,
            ..sky(
                "Lever du soleil",
                "Soleil bas à l'est, horizon orangé, collines.",
                [0.12, 0.2, 0.45],
                [1.0, 0.7, 0.5],
            )
        },
        Sky {
            sun: sun(0.75, 3.0, 300.0, [1.0, 0.5, 0.25]),
            glow: 3.5,
            clouds: 0.25,
            cloud: [1.0, 0.6, 0.55],
            ground: Ground::DryGrass,
            ..sky("Coucher du soleil", "Soleil bas à l'ouest, nuages roses.", [0.15, 0.18, 0.4], [1.0, 0.55, 0.3])
        },
        Sky {
            sun: sun(0.7, -4.0, 0.0, [1.0, 0.45, 0.2]),
            glow: 1.5,
            ground: Ground::Asphalt,
            skyline: Skyline::City,
            ..sky(
                "Crépuscule",
                "L'heure bleue au-dessus d'une ville, lueur orange à l'horizon.",
                [0.03, 0.06, 0.18],
                [0.35, 0.3, 0.4],
            )
        },
        Sky {
            stars: true,
            ground: Ground::Dark,
            skyline: Skyline::Trees,
            ..sky(
                "Nuit étoilée",
                "Nuit claire et étoilée au-dessus d'une lisière.",
                [0.004, 0.008, 0.025],
                [0.03, 0.04, 0.07],
            )
        },
        Sky {
            sun: sun(0.4, 35.0, 800.0, white_sun),
            glow: 0.8,
            clouds: 0.15,
            ground: Ground::Sea,
            ..sky("En mer", "Pleine mer sous un ciel clair, reflets du soleil.", [0.15, 0.33, 0.78], [0.75, 0.85, 0.98])
        },
        Sky {
            sun: sun(0.55, 20.0, 600.0, [1.0, 0.95, 0.88]),
            glow: 1.0,
            clouds: 0.1,
            ground: Ground::Snow,
            ..sky("Champ enneigé", "Plaine enneigée sous un ciel d'hiver pâle.", [0.3, 0.45, 0.8], [0.85, 0.9, 0.97])
        },
        Sky {
            sun: sun(0.15, 30.0, 800.0, white_sun),
            glow: 0.7,
            clouds: 0.12,
            ground: Ground::Snow,
            skyline: Skyline::SnowyMountains,
            ..sky(
                "Montagne enneigée",
                "Sommets enneigés tout autour, ciel d'altitude.",
                [0.1, 0.25, 0.75],
                [0.7, 0.82, 0.98],
            )
        },
        Sky {
            sun: sun(0.48, 55.0, 1000.0, white_sun),
            glow: 1.5,
            ground: Ground::Sand,
            skyline: Skyline::Hills,
            ..sky("Désert", "Dunes sous un ciel chaud et pâle.", [0.3, 0.45, 0.8], [0.95, 0.9, 0.8])
        },
        Sky {
            sun: sun(0.35, 40.0, 800.0, white_sun),
            glow: 0.6,
            clouds: 0.3,
            skyline: Skyline::Trees,
            ..sky("Prairie", "Prairie bordée d'arbres, quelques nuages.", [0.17, 0.35, 0.82], [0.72, 0.82, 0.98])
        },
        Sky {
            sun: sun(0.85, 35.0, 700.0, white_sun),
            glow: 0.7,
            clouds: 0.2,
            skyline: Skyline::Mountains,
            ..sky("Vallée", "Montagnes vertes et bleutées à l'horizon.", [0.17, 0.35, 0.82], [0.75, 0.84, 0.97])
        },
        Sky {
            sun: sun(0.05, 40.0, 800.0, white_sun),
            glow: 0.8,
            clouds: 0.18,
            ground: Ground::Asphalt,
            skyline: Skyline::City,
            ..sky("Ville", "Silhouettes d'immeubles autour d'une place.", [0.2, 0.38, 0.8], [0.78, 0.84, 0.92])
        },
        Sky {
            sun: sun(0.6, 25.0, 150.0, [1.0, 0.9, 0.8]),
            glow: 0.4,
            clouds: 0.85,
            cloud: [0.28, 0.29, 0.33],
            ground: Ground::Dark,
            skyline: Skyline::Hills,
            ..sky("Ciel d'orage", "Nuages sombres et bas, éclaircie au loin.", [0.2, 0.22, 0.26], [0.5, 0.52, 0.55])
        },
        Sky {
            ground: Ground::Studio,
            ..sky(
                "Studio blanc",
                "Fond blanc sans horizon et deux boîtes à lumière : pour présenter un objet.",
                [0.95, 0.95, 0.95],
                [1.0, 1.0, 1.0],
            )
        },
        Sky {
            ground: Ground::Studio,
            ..sky("Studio gris", "Fond gris foncé et trois boîtes à lumière.", [0.08, 0.08, 0.085], [0.12, 0.12, 0.125])
        },
    ]
}

fn dir(u: f32, el: f32) -> [f32; 3] {
    [el.cos() * (u * TAU).cos(), el.cos() * (u * TAU).sin(), el.sin()]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Height of the skyline above the horizon (radians) at column `u`.
fn skyline(kind: Skyline, u: f32) -> f32 {
    let deg = |d: f32| d.to_radians();
    match kind {
        Skyline::None => -1.0,
        Skyline::Mountains | Skyline::SnowyMountains => {
            let n = fbm(u, 0.5, 6, 5, 1101);
            deg(1.5 + 14.0 * smoothstep(0.25, 0.85, n))
        }
        Skyline::Hills => deg(0.8 + 3.0 * fbm(u, 0.3, 4, 4, 1102)),
        Skyline::Trees => {
            let crowns = fbm(u, 0.7, 64, 3, 1103);
            deg(2.5 + 2.5 * crowns + 1.5 * fbm(u, 0.2, 8, 2, 1104))
        }
        Skyline::City => {
            let b = (u * 90.0).floor() as i32;
            let tall = hash(b.rem_euclid(90), 0, 1105);
            let gap = hash(b.rem_euclid(90), 1, 1106) < 0.15;
            if gap { deg(1.0) } else { deg(2.0 + 10.0 * tall * tall) }
        }
    }
}

/// The radiance seen in direction (u, el) (linear light).
fn radiance(s: &Sky, u: f32, el: f32) -> [f32; 3] {
    let d = dir(u, el);
    if s.ground == Ground::Studio {
        return studio(s, u, el);
    }
    let sun_dir = s.sun.map(|(su, se, ..)| dir(su, se.to_radians()));
    let sky_at = |el: f32, d: [f32; 3]| {
        let t = 1.0 - (el.max(0.0) / FRAC_PI_2);
        let mut c = mix(s.zenith, s.horizon, t.powf(3.0));
        if let (Some(sd), Some((_, _, _, col))) = (sun_dir, s.sun) {
            let cosang = dot(d, sd).clamp(-1.0, 1.0);
            // A wide glow and a tight halo around the sun, stronger near the horizon.
            let g =
                s.glow * (0.12 * ((cosang - 1.0) * 8.0).exp() + 0.5 * ((cosang - 1.0) * 150.0).exp()) * (0.5 + 0.5 * t);
            c = [c[0] + col[0] * g, c[1] + col[1] * g, c[2] + col[2] * g];
        }
        c
    };
    // Skyline: mountains, trees, buildings.
    let line = skyline(s.skyline, u);
    if el > 0.0 && el < line {
        let haze = mix(s.horizon, s.zenith, 0.3);
        let (base, k) = match s.skyline {
            Skyline::SnowyMountains => {
                let snow = el > line * 0.55 + 0.01 * fbm(u, 0.1, 32, 2, 1107);
                (if snow { [0.9, 0.92, 0.95] } else { [0.18, 0.2, 0.24] }, 0.35)
            }
            Skyline::Mountains => ([0.12, 0.18, 0.15], 0.45),
            Skyline::Hills => (mix([0.18, 0.22, 0.12], [0.35, 0.3, 0.2], fbm(u, 0.4, 16, 3, 1108)), 0.3),
            Skyline::Trees => ([0.05, 0.09, 0.05], 0.15),
            Skyline::City => {
                let window = hash((u * 2000.0) as i32, (el * 600.0) as i32, 1109) > 0.9;
                let lit = if s.stars || s.zenith[2] < 0.2 { 1.0 } else { 0.0 };
                (if window { [0.9 * lit + 0.15, 0.7 * lit + 0.15, 0.4 * lit + 0.17] } else { [0.12, 0.13, 0.15] }, 0.2)
            }
            Skyline::None => (haze, 0.0),
        };
        let light = s.horizon.iter().sum::<f32>() / 3.0;
        return mix(scale(base, light.max(0.05)), haze, k);
    }
    if el >= 0.0 {
        let mut c = sky_at(el, d);
        if s.clouds > 0.0 && el > 0.01 {
            let k = 1.0 / (d[2] + 0.12);
            let (x, y) = (d[0] * k * 0.6, d[1] * k * 0.6);
            let n = fbm(x / 64.0 + 0.5, y / 64.0 + 0.5, 64, 6, 1110);
            let cover = if s.clouds >= 1.0 { 1.0 } else { smoothstep(1.0 - s.clouds, 1.0 - s.clouds + 0.18, n) };
            let fade = smoothstep(0.01, 0.12, el);
            let mut cloud = s.cloud;
            if let (Some(sd), Some((_, _, _, col))) = (sun_dir, s.sun) {
                let lit = 0.75 + 0.5 * dot(d, sd).max(0.0);
                cloud = [cloud[0] * lit * col[0], cloud[1] * lit * col[1], cloud[2] * lit * col[2]];
            }
            if s.clouds >= 1.0 {
                cloud = scale(cloud, 0.9 + 0.2 * n);
            }
            c = mix(c, cloud, cover * fade);
        }
        if s.stars {
            let (px, py) = ((u * 4096.0) as i32, (el * 1300.0) as i32);
            let h = hash(px, py, 1111);
            if h > 0.9985 {
                let k = 0.5 + 4.0 * (h - 0.9985) / 0.0015;
                c = [c[0] + k, c[1] + k, c[2] + k * 1.1];
            }
        }
        if let (Some(sd), Some((_, _, k, col))) = (sun_dir, s.sun)
            && k > 0.0
            && dot(d, sd) > 1.2f32.to_radians().cos()
        {
            c = scale(col, k);
        }
        return c;
    }
    // Ground: seen on a plane below the eye, hazier towards the horizon.
    let depth = -el;
    let k = 1.0 / (-d[2]).max(0.02);
    let (x, y) = (d[0] * k, d[1] * k);
    let tex = |p: i32, seed: u32| fbm((x / 40.0).rem_euclid(1.0), (y / 40.0).rem_euclid(1.0), p, 5, seed);
    let light = (s.zenith.iter().sum::<f32>() + s.horizon.iter().sum::<f32>()) / 6.0;
    let sun_light =
        s.sun.map_or(0.0, |(_, se, sk, _)| if sk > 0.0 { se.to_radians().sin().max(0.0) * 1.2 } else { 0.0 });
    let lit = |c: [f32; 3]| scale(c, light * 0.8 + sun_light);
    let base = match s.ground {
        Ground::Grass => lit(mix([0.06, 0.12, 0.03], [0.12, 0.2, 0.05], tex(64, 1112))),
        Ground::DryGrass => lit(mix([0.2, 0.17, 0.08], [0.32, 0.27, 0.13], tex(64, 1113))),
        Ground::Snow => lit(scale([0.85, 0.88, 0.92], 0.92 + 0.08 * tex(32, 1114))),
        Ground::Sand => lit(mix([0.5, 0.4, 0.27], [0.62, 0.5, 0.34], tex(16, 1115))),
        Ground::Asphalt => lit(mix([0.06, 0.06, 0.065], [0.1, 0.1, 0.1], tex(128, 1116))),
        Ground::Dark => lit(mix([0.05, 0.06, 0.04], [0.09, 0.1, 0.07], tex(32, 1118))),
        Ground::Studio => [0.0; 3],
        Ground::Sea => {
            let m = [d[0], d[1], -d[2]];
            let reflected = sky_at(-el, m);
            let cos = (-d[2]).clamp(0.0, 1.0);
            let fresnel = 0.02 + 0.98 * (1.0 - cos).powi(5);
            let wave = value(x * 3.0 % 1024.0, y * 3.0 % 1024.0, 1024, 1117);
            let deep = scale([0.01, 0.04, 0.07], 0.8 + 0.4 * wave);
            let mut c = mix(deep, reflected, fresnel);
            if let (Some(sd), Some((_, _, k, col))) = (sun_dir, s.sun) {
                let glint = dot(m, sd).max(0.0).powf(800.0) * (0.5 + wave);
                c = [
                    c[0] + col[0] * k * 0.05 * glint,
                    c[1] + col[1] * k * 0.05 * glint,
                    c[2] + col[2] * k * 0.05 * glint,
                ];
            }
            return c;
        }
    };
    mix(base, mix(s.horizon, base, 0.4), (-depth * 25.0).exp())
}

/// A studio: a seamless backdrop and soft boxes.
fn studio(s: &Sky, u: f32, el: f32) -> [f32; 3] {
    let t = (el / PI + 0.5).clamp(0.0, 1.0);
    let mut c = mix(scale(s.horizon, 0.9), s.zenith, t);
    let bright = s.zenith[0] > 0.5;
    let boxes: &[(f32, f32, f32, f32, f32)] = if bright {
        &[(0.15, 35.0, 0.06, 14.0, 6.0), (0.6, 50.0, 0.08, 16.0, 4.0)]
    } else {
        &[(0.1, 30.0, 0.05, 12.0, 12.0), (0.45, 45.0, 0.07, 15.0, 8.0), (0.8, 20.0, 0.04, 10.0, 5.0)]
    };
    for &(bu, be, half_u, half_e, k) in boxes {
        let du = ((u - bu + 0.5).rem_euclid(1.0) - 0.5).abs();
        let de = (el.to_degrees() - be).abs();
        let inside = smoothstep(half_u, half_u * 0.8, du) * smoothstep(half_e, half_e * 0.8, de);
        c = mix(c, [k, k, k * 0.98], inside);
    }
    c
}

fn panorama(s: &Sky) -> Vec<[f32; 3]> {
    let mut px = Vec::with_capacity((W * H) as usize);
    for y in 0..H {
        let el = (0.5 - (y as f32 + 0.5) / H as f32) * PI;
        for x in 0..W {
            px.push(radiance(s, (x as f32 + 0.5) / W as f32, el));
        }
    }
    px
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// One environment file.
pub struct Entry {
    pub file: String,
    pub bytes: Vec<u8>,
}

fn ske(s: &Sky) -> Entry {
    let px = panorama(s);
    let image = hdr(W, H, &px);
    let thumb = crate::environments::prepare(W, H, &px).thumb;
    let thumb_png = png_rgba(thumb.0, thumb.1, thumb.2);
    let stem = super::materials::file_stem(s.name);
    let image_name = format!("{stem}.hdr");
    let (sun_x, sun_y, linked) = match s.sun {
        Some((u, el, k, _)) if k > 0.0 => (u, el / 90.0, 1),
        _ => (0.0, 0.5, 0),
    };
    let doc = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<environmentDocument xmlns=\"http://sketchup.com/schemas/sketchup/1.0/environment\" xmlns:env=\"http://sketchup.com/schemas/sketchup/1.0/environment\" xmlns:r=\"http://sketchup.google.com/schemas/1.0/references\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n  <env:environment name=\"{}\" description=\"{}\" ibl_enabled=\"1\" rotation_angle=\"0\" skybox_enabled=\"1\" skybox_gain=\"1\" reflection_gain=\"1\" shadow_light_enabled=\"{linked}\" shadow_light_positionX=\"{sun_x}\" shadow_light_positionY=\"{sun_y}\" ibl_filename=\"{}\">\n    <ibl_image id=\"1\" path=\"{}\" file_name=\"{}\"/>\n  </env:environment>\n</environmentDocument>\n",
        escape(s.name),
        escape(s.desc),
        escape(&image_name),
        escape(&image_name),
        escape(&image_name)
    );
    let props = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<documentProperties xmlns=\"http://sketchup.google.com/schemas/1.0/documentproperties\" xmlns:dp=\"http://sketchup.google.com/schemas/1.0/documentproperties\">\n  <dp:title>{}</dp:title>\n  <dp:description>{}</dp:description>\n  <dp:creator></dp:creator>\n  <dp:keywords></dp:keywords>\n  <dp:lastModifiedBy></dp:lastModifiedBy>\n  <dp:revision>0</dp:revision>\n  <dp:created>2026-01-01T00:00:00Z</dp:created>\n  <dp:modified>2026-01-01T00:00:00Z</dp:modified>\n  <dp:thumbnail>doc_thumbnail.png</dp:thumbnail>\n  <dp:generator dp:name=\"Environment\" dp:version=\"1\"/>\n</documentProperties>\n",
        escape(s.name),
        escape(s.desc)
    );
    let refs = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<references xmlns=\"http://sketchup.google.com/schemas/1.0/references\" />\n";
    let bytes = crate::zip::write(&[
        ("document.xml".to_string(), doc.into_bytes()),
        ("documentProperties.xml".to_string(), props.into_bytes()),
        ("doc_thumbnail.png".to_string(), thumb_png),
        (format!("ref/{image_name}"), image),
        ("references.xml".to_string(), refs.as_bytes().to_vec()),
    ]);
    Entry { file: stem, bytes }
}

/// Every environment (computed on several threads: each panorama is half a million rays).
pub fn all() -> Vec<Entry> {
    let list = skies();
    std::thread::scope(|scope| {
        let jobs: Vec<_> = list.iter().map(|s| scope.spawn(move || ske(s))).collect();
        jobs.into_iter().map(|j| j.join().expect("panorama")).collect()
    })
}
