//! Procedural images: hashed lattices, periodic noises (so that textures tile without seams)
//! and image encoding. Everything is deterministic: the same code makes the same files.

use std::io::Cursor;

/// A hash of a lattice point, in [0, 1).
pub fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h =
        (x as u32).wrapping_mul(0x8DA6_B343) ^ (y as u32).wrapping_mul(0xD816_3841) ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

fn smooth(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Value noise in [0, 1) at (x, y), periodic with period `p` lattice cells in both directions.
pub fn value(x: f32, y: f32, p: i32, seed: u32) -> f32 {
    value2(x, y, p, p, seed)
}

/// Value noise periodic with `px` cells across and `py` cells down.
pub fn value2(x: f32, y: f32, px: i32, py: i32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (smooth(x - x0), smooth(y - y0));
    let (ix, iy) = (x0 as i32, y0 as i32);
    let h = |i: i32, j: i32| hash(i.rem_euclid(px.max(1)), j.rem_euclid(py.max(1)), seed);
    let a = h(ix, iy) + (h(ix + 1, iy) - h(ix, iy)) * fx;
    let b = h(ix, iy + 1) + (h(ix + 1, iy + 1) - h(ix, iy + 1)) * fx;
    a + (b - a) * fy
}

/// Fractal noise stretched along one axis: `px` × `py` cells for the first layer over the
/// unit square (periodic).
pub fn fbm2(u: f32, v: f32, px: i32, py: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut fx, mut fy) = (0.0, 1.0, 0.0, px, py);
    for o in 0..octaves {
        sum += amp * value2(u * fx as f32, v * fy as f32, fx, fy, seed.wrapping_add(o * 101));
        norm += amp;
        amp *= 0.5;
        fx *= 2;
        fy *= 2;
    }
    sum / norm
}

/// Fractal noise in about [0, 1): `octaves` layers of value noise, the first with `p` cells
/// across the unit square (u, v in [0, 1)); periodic over the unit square.
pub fn fbm(u: f32, v: f32, p: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut f) = (0.0, 1.0, 0.0, p);
    for o in 0..octaves {
        sum += amp * value(u * f as f32, v * f as f32, f, seed.wrapping_add(o * 101));
        norm += amp;
        amp *= 0.5;
        f *= 2;
    }
    sum / norm
}

/// Cellular noise over the unit square with `p` × `p` cells (periodic): the distances to the
/// nearest and second nearest feature points (in cell units) and the nearest cell's id.
pub fn worley(u: f32, v: f32, p: i32, seed: u32) -> (f32, f32, u32) {
    let (x, y) = (u * p as f32, v * p as f32);
    let (cx, cy) = (x.floor() as i32, y.floor() as i32);
    let (mut d1, mut d2, mut id) = (f32::MAX, f32::MAX, 0u32);
    for j in -1..=1 {
        for i in -1..=1 {
            let (gx, gy) = (cx + i, cy + j);
            let (wx, wy) = (gx.rem_euclid(p), gy.rem_euclid(p));
            let fx = gx as f32 + hash(wx, wy, seed);
            let fy = gy as f32 + hash(wx, wy, seed ^ 0x5555);
            let d = ((fx - x).powi(2) + (fy - y).powi(2)).sqrt();
            if d < d1 {
                d2 = d1;
                d1 = d;
                id = (wy * p + wx) as u32;
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2, id)
}

pub fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

pub fn scale(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

/// An sRGB colour 0–255 as 0–1.
pub fn rgb(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A square image made by `f(u, v)` over the unit square (pixel centres), 0–1 sRGB.
pub fn image(n: u32, f: impl Fn(f32, f32) -> [f32; 3]) -> Vec<[f32; 3]> {
    let mut px = Vec::with_capacity((n * n) as usize);
    for y in 0..n {
        for x in 0..n {
            px.push(f((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32));
        }
    }
    px
}

fn to_u8(px: &[[f32; 3]]) -> Vec<u8> {
    px.iter().flat_map(|c| c.map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)).collect()
}

/// JPEG of an RGB image (quality 88).
pub fn jpeg(w: u32, h: u32, px: &[[f32; 3]]) -> Vec<u8> {
    let mut out = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88);
    image::ImageEncoder::write_image(enc, &to_u8(px), w, h, image::ExtendedColorType::Rgb8).expect("jpeg");
    out
}

/// PNG of an RGB image.
pub fn png(w: u32, h: u32, px: &[[f32; 3]]) -> Vec<u8> {
    let mut out = Vec::new();
    let img = image::RgbImage::from_raw(w, h, to_u8(px)).expect("taille");
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).expect("png");
    out
}

/// PNG of RGBA pixels (bytes).
pub fn png_rgba(w: u32, h: u32, rgba: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    let img = image::RgbaImage::from_raw(w, h, rgba).expect("taille");
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).expect("png");
    out
}

/// Radiance HDR (RGBE) of linear colours.
pub fn hdr(w: u32, h: u32, px: &[[f32; 3]]) -> Vec<u8> {
    let mut out = Vec::new();
    let data: Vec<image::Rgb<f32>> = px.iter().map(|c| image::Rgb(c.map(|v| v.max(0.0)))).collect();
    image::codecs::hdr::HdrEncoder::new(&mut out).encode(&data, w as usize, h as usize).expect("hdr");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noises_tile_and_stay_in_range() {
        for (u, v) in [(0.0, 0.3), (0.999_99, 0.3), (0.2, 0.0), (0.2, 0.999_99)] {
            let a = fbm(u, v, 4, 5, 7);
            assert!((0.0..1.0).contains(&a));
        }
        // Periodic: the left and right edges meet.
        assert!((fbm(0.0, 0.37, 4, 5, 9) - fbm(1.0, 0.37, 4, 5, 9)).abs() < 1e-5);
        assert!((worley(0.0, 0.5, 6, 3).0 - worley(1.0, 0.5, 6, 3).0).abs() < 1e-4);
        assert_eq!(hash(3, 4, 5), hash(3, 4, 5));
    }
}
