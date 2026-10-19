//! Décodage et encodage d'images (ImageRep, textures).

use std::io::Cursor;
use std::path::Path;

use esq_core::meta::{TextureData, base_name};
use esq_core::photo::Blob;
use image::{DynamicImage, ImageFormat};
use serde_json::{Value as J, json};

use super::{b64, raise, unb64};
use crate::bridge::{Ctx, R};

pub fn run(_c: &mut Ctx, op: &str, a: &J) -> Option<R> {
    Some(match op {
        "img.load" => load(a),
        "img.save" => save(a),
        "img.convert" => convert(a),
        "img.decode" => decode(a),
        "img.encode" => encode(a),
        _ => return None,
    })
}

/// Pixels of an image file given in memory (base64): never goes through the disk.
fn decode(a: &J) -> R {
    let bytes = unb64(a["data"].as_str().unwrap_or(""))?;
    let img = image::load_from_memory(&bytes).map_err(|_| raise("RuntimeError", "Invalid image texture data"))?;
    let (w, h, bpp, data) = to_bgra(&img, 4);
    Ok(json!({"w": w, "h": h, "bpp": bpp, "pad": 4, "data": b64(&data)}))
}

/// PNG file (base64) of `ImageRep` pixels, built in memory.
fn encode(a: &J) -> R {
    let img = build(a)?;
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(json!(b64(&out)))
}

/// Pixel rows as SketchUp's `ImageRep#data` has them: BGR or BGRA, rows padded to `pad` bytes.
pub fn to_bgra(img: &DynamicImage, pad: usize) -> (u32, u32, u32, Vec<u8>) {
    let (w, h) = (img.width(), img.height());
    let alpha = img.color().has_alpha();
    let bpp = if alpha { 32 } else { 24 };
    let px = (bpp / 8) as usize;
    let row = (w as usize * px).div_ceil(pad) * pad;
    let mut out = vec![0u8; row * h as usize];
    let rgba = img.to_rgba8();
    for y in 0..h as usize {
        for x in 0..w as usize {
            let p = rgba.get_pixel(x as u32, y as u32).0;
            let o = y * row + x * px;
            out[o] = p[2];
            out[o + 1] = p[1];
            out[o + 2] = p[0];
            if alpha {
                out[o + 3] = p[3];
            }
        }
    }
    (w, h, bpp, out)
}

fn load(a: &J) -> R {
    let path = a["path"].as_str().ok_or("chemin manquant")?;
    let bytes = std::fs::read(path).map_err(|_| raise("ArgumentError", "failed to load image"))?;
    let img = image::load_from_memory(&bytes).map_err(|_| raise("ArgumentError", "failed to load image"))?;
    let (w, h, bpp, data) = to_bgra(&img, 4);
    Ok(json!({"w": w, "h": h, "bpp": bpp, "pad": 4, "data": b64(&data)}))
}

fn build(a: &J) -> Result<DynamicImage, String> {
    let (w, h) = (a["w"].as_u64().unwrap_or(0) as u32, a["h"].as_u64().unwrap_or(0) as u32);
    let bpp = a["bpp"].as_u64().unwrap_or(32) as usize;
    let pad = a["pad"].as_u64().unwrap_or(4).max(1) as usize;
    let data = unb64(a["data"].as_str().unwrap_or(""))?;
    let px = bpp / 8;
    let row = (w as usize * px).div_ceil(pad) * pad;
    if px == 0 || data.len() < row * h as usize {
        return Err(raise("RuntimeError", "failed to save image"));
    }
    let mut img = image::RgbaImage::new(w, h);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let o = y * row + x * px;
            let v = match px {
                1 => [data[o]; 3],
                _ => [data[o + 2], data[o + 1], data[o]],
            };
            let al = if px == 4 { data[o + 3] } else { 255 };
            img.put_pixel(x as u32, y as u32, image::Rgba([v[0], v[1], v[2], al]));
        }
    }
    Ok(DynamicImage::ImageRgba8(img))
}

fn format_of(path: &str) -> Option<ImageFormat> {
    ImageFormat::from_extension(Path::new(path).extension()?)
}

fn save(a: &J) -> R {
    let path = a["path"].as_str().ok_or("chemin manquant")?;
    let img = build(a)?;
    let fmt = format_of(path).ok_or_else(|| raise("RuntimeError", "failed to save image"))?;
    let img = if fmt == ImageFormat::Jpeg { DynamicImage::ImageRgb8(img.to_rgb8()) } else { img };
    img.save_with_format(path, fmt).map_err(|_| raise("RuntimeError", "failed to save image"))?;
    Ok(J::Null)
}

/// Re-encodes a stored image file (base64) in the format named by `path`'s extension.
fn convert(a: &J) -> R {
    let path = a["path"].as_str().ok_or("chemin manquant")?;
    let bytes = unb64(a["data"].as_str().unwrap_or(""))?;
    let fmt = format_of(path);
    let same = image::guess_format(&bytes).ok() == fmt;
    if same || fmt.is_none() {
        std::fs::write(path, &bytes).map_err(|e| e.to_string())?;
        return Ok(json!(true));
    }
    let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    let img = if fmt == Some(ImageFormat::Jpeg) { DynamicImage::ImageRgb8(img.to_rgb8()) } else { img };
    Ok(json!(img.save_with_format(path, fmt.unwrap()).is_ok()))
}

/// Builds a texture from an image file's bytes. `size_in` is the tile size in inches
/// (`None` = one pixel per inch, SketchUp's default).
pub fn texture_from_bytes(filename: &str, bytes: Vec<u8>, size_in: Option<[f64; 2]>) -> Result<TextureData, String> {
    let img = image::load_from_memory(&bytes)
        .map_err(|_| raise("ArgumentError", &format!("Could not load texture from {filename}")))?;
    let (w, h) = (img.width(), img.height());
    // Average over (at most about 65 000) evenly spread pixels.
    let rgb = img.to_rgb8();
    let step = ((w as u64 * h as u64) / 65_536).max(1) as usize;
    let (mut s, mut n) = ([0u64; 3], 0u64);
    for p in rgb.pixels().step_by(step) {
        for (acc, v) in s.iter_mut().zip(p.0) {
            *acc += v as u64;
        }
        n += 1;
    }
    let n = n.max(1);
    let avg = [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8];
    let size = size_in.unwrap_or([w as f64, h as f64]);
    Ok(TextureData {
        filename: base_name(filename),
        image: Blob(std::sync::Arc::new(bytes)),
        px: [w, h],
        size: [size[0] * 25.4, size[1] * 25.4],
        avg,
    })
}

pub fn texture_from_file(path: &str, size_in: Option<[f64; 2]>) -> Result<TextureData, String> {
    let bytes =
        std::fs::read(path).map_err(|_| raise("ArgumentError", &format!("Could not load texture from {path}")))?;
    let name = Path::new(path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    texture_from_bytes(&name, bytes, size_in)
}
