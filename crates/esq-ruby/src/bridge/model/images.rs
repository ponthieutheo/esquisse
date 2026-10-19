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
    let rep = rep_of(&bytes, "").ok_or_else(|| raise("RuntimeError", "Invalid image texture data"))?;
    Ok(rep.json())
}

/// PNG file (base64) of `ImageRep` pixels, built in memory.
fn encode(a: &J) -> R {
    let img = build(a)?;
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(json!(b64(&out)))
}

/// Pixels as SketchUp's `ImageRep` keeps them (`SUImageRepGetData`): rows from the top, one
/// grey byte (8 bits), BGR (24 bits) or BGRA (32 bits), each row followed by `pad` bytes so
/// that it fills a multiple of 4 bytes.
pub struct Rep {
    pub w: u32,
    pub h: u32,
    pub bpp: u32,
    pub pad: usize,
    pub data: Vec<u8>,
}

impl Rep {
    /// From RGBA pixels (rows from the top). SketchUp keeps the transparency channel only
    /// when a pixel is not opaque (after counting the pixels' alpha in
    /// an opaque RGBA or grey-and-alpha picture becomes 24 bits, as the SDK's
    /// `SUImageRepLoadFile` shows), and a grey picture without one stays grey (8 bits).
    fn from_rgba(w: u32, h: u32, rgba: &[u8], grey: bool) -> Rep {
        let alpha = rgba.as_chunks::<4>().0.iter().any(|p| p[3] < 255);
        let px = if alpha {
            4
        } else if grey {
            1
        } else {
            3
        };
        let row = w as usize * px;
        let pad = (4 - row % 4) % 4;
        let mut data = Vec::with_capacity((row + pad) * h as usize);
        for line in rgba.chunks_exact(w as usize * 4).take(h as usize) {
            for p in line.as_chunks::<4>().0 {
                match px {
                    1 => data.push(p[0]),
                    3 => data.extend_from_slice(&[p[2], p[1], p[0]]),
                    _ => data.extend_from_slice(&[p[2], p[1], p[0], p[3]]),
                }
            }
            data.extend(std::iter::repeat_n(0, pad));
        }
        Rep { w, h, bpp: px as u32 * 8, pad, data }
    }

    fn json(&self) -> J {
        json!({"w": self.w, "h": self.h, "bpp": self.bpp, "pad": self.pad, "data": b64(&self.data)})
    }
}

/// Decodes an image file as SketchUp's image reader does: JPEG through the decoder that gives
/// SketchUp's pixels (`esq_skp::native::jpeg`, libjpeg's arithmetic), Photoshop through
/// `esq_core::psd`, the others (PNG, BMP, GIF, TIFF, TGA) by their content, TGA (which has no
/// signature) by the name's extension. `None` for what is not a readable image.
pub fn rep_of(bytes: &[u8], name: &str) -> Option<Rep> {
    let (w, h, rgba, grey) = pixels_of(bytes, name)?;
    Some(Rep::from_rgba(w, h, &rgba, grey))
}

/// The pixels of an image file as SketchUp decodes them (its JPEG decoder, PSD, the other
/// formats by `image`): width, height, RGBA rows from the top, and whether it is grey.
fn pixels_of(bytes: &[u8], name: &str) -> Option<(u32, u32, Vec<u8>, bool)> {
    const MAX_ALLOC: usize = 1 << 30;
    if esq_skp::native::jpeg::is_jpeg(bytes)
        && let Some(img) = esq_skp::native::jpeg::decode(bytes, MAX_ALLOC).filter(|i| !i.partial)
    {
        let (w, h) = (img.width as u32, img.height as u32);
        let rgba: Vec<u8> = match img.channels {
            1 => img.data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            3 => img.data.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
            _ => return None,
        };
        return Some((w, h, rgba, img.channels == 1));
    }
    if esq_core::psd::is_psd(bytes) {
        let (w, h, rgba, _) = esq_core::psd::decode(bytes).ok()?;
        return Some((w, h, rgba, false));
    }
    let tga = Path::new(name).extension().is_some_and(|e| e.eq_ignore_ascii_case("tga"));
    let format = if tga { Some(ImageFormat::Tga) } else { image::guess_format(bytes).ok() };
    let img = image::load_from_memory_with_format(bytes, format?).ok()?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return None;
    }
    let grey = matches!(img.color(), image::ColorType::L8 | image::ColorType::L16);
    Some((w, h, img.to_rgba8().into_raw(), grey))
}

/// The average colour of a texture (the texture's `+0x70`): each channel
/// summed over all the pixels, divided by their number, truncated.
fn average(rgba: &[u8]) -> [u8; 3] {
    let px = rgba.as_chunks::<4>().0;
    let n = px.len().max(1) as u64;
    let mut s = [0u64; 3];
    for p in px {
        for (acc, &v) in s.iter_mut().zip(p) {
            *acc += u64::from(v);
        }
    }
    s.map(|v| (v / n) as u8)
}

/// `SUImageRepLoadFile`.
fn load(a: &J) -> R {
    let path = a["path"].as_str().ok_or("chemin manquant")?;
    let failed = || raise("ArgumentError", "failed to load image");
    let bytes = std::fs::read(path).map_err(|_| failed())?;
    Ok(rep_of(&bytes, path).ok_or_else(failed)?.json())
}

fn build(a: &J) -> Result<DynamicImage, String> {
    let (w, h) = (a["w"].as_u64().unwrap_or(0) as u32, a["h"].as_u64().unwrap_or(0) as u32);
    let bpp = a["bpp"].as_u64().unwrap_or(32) as usize;
    let pad = a["pad"].as_u64().unwrap_or(0) as usize;
    let data = unb64(a["data"].as_str().unwrap_or(""))?;
    let px = bpp / 8;
    let row = w as usize * px + pad;
    if px == 0 || w == 0 || h == 0 || data.len() < row * h as usize {
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
    // Without a transparency channel, the file has none either (it reads back as 24 bits).
    Ok(if px == 4 {
        DynamicImage::ImageRgba8(img)
    } else {
        DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(img).to_rgb8())
    })
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
    let (w, h, rgba, _) = pixels_of(&bytes, filename)
        .ok_or_else(|| raise("ArgumentError", &format!("Could not load texture from {filename}")))?;
    let avg = average(&rgba);
    let size = size_in.unwrap_or([w as f64, h as f64]);
    // An image without a file (an ImageRep): SketchUp names it by its format alone.
    let filename = if matches!(filename, ".jpg" | ".png") { filename.to_string() } else { base_name(filename) };
    Ok(TextureData {
        filename,
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
