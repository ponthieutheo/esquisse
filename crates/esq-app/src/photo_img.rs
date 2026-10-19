//! Photographs for Match Photo: decoding (PNG, JPEG with its EXIF orientation) and the
//! egui textures they are painted with.

use std::io::Cursor;
use std::sync::{Arc, Mutex};

use eframe::egui;
use esq_core::photo::Blob;
use image::{DynamicImage, ImageDecoder, ImageReader};

/// Longest side of the texture a photo is shown with; bigger photos are reduced for display
/// (the calibration keeps working in the pixels of the original).
pub const MAX_TEXTURE_SIDE: u32 = 4096;
/// Photos kept as textures at the same time.
const CACHE_SIZE: usize = 6;
/// What an image of a model may take: its longest side, and its pixels once decoded. The
/// image comes from a file (a model, a material, a photo): its header is read first and an
/// image beyond these is refused before anything is decoded (`docs/securite.md`, rule 6).
pub const MAX_IMAGE_SIDE: u32 = 16384;
pub const MAX_DECODED_BYTES: u64 = 512 << 20;

/// Decodes an image file, turned the way its EXIF orientation says; refused when its header
/// announces more than [`MAX_IMAGE_SIDE`] pixels on a side or [`MAX_DECODED_BYTES`] decoded.
pub fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    let mut reader =
        ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| format!("Image illisible : {e}"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| format!("Format d'image non pris en charge ({e})"))?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| format!("Image illisible : {e}"))?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img)
}

/// Size in pixels of an image file, as `decode` would give it.
pub fn image_size(bytes: &[u8]) -> Result<[u32; 2], String> {
    let img = decode(bytes)?;
    Ok([img.width(), img.height()])
}

struct Entry {
    /// Keeps the bytes alive so that the key (their address) cannot be reused.
    blob: Arc<Vec<u8>>,
    handle: Option<egui::TextureHandle>,
}

#[derive(Clone, Default)]
struct Cache(Arc<Mutex<Vec<Entry>>>);

/// The texture of a photo (loaded on first use), `None` if it cannot be decoded.
pub fn texture(ctx: &egui::Context, image: &Blob) -> Option<egui::TextureId> {
    let cache: Cache =
        ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(egui::Id::unique("match_photo_textures")).clone());
    let mut entries = cache.0.lock().ok()?;
    if let Some(e) = entries.iter().find(|e| Arc::ptr_eq(&e.blob, &image.0)) {
        return e.handle.as_ref().map(|h| h.id());
    }
    let handle = decode(&image.0).ok().map(|img| {
        let rgba = img.to_rgba8();
        let (w, h, px) = crate::gpu_img::fit(rgba.width(), rgba.height(), rgba.as_raw(), MAX_TEXTURE_SIDE);
        crate::gpu_img::load_texture(ctx, "match_photo", [w as usize, h as usize], &px, egui::TextureOptions::LINEAR)
    });
    let id = handle.as_ref().map(|h| h.id());
    if entries.len() >= CACHE_SIZE {
        entries.remove(0);
    }
    entries.push(Entry { blob: image.0.clone(), handle });
    id
}

/// Paints a photo in `rect`, then dims it a little when `opacity` < 1.
pub fn paint(ctx: &egui::Context, painter: &egui::Painter, image: &Blob, rect: egui::Rect, opacity: f32) {
    let Some(id) = texture(ctx, image) else {
        painter.rect_filled(rect, 0.0, egui::Color32::from_gray(90));
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Photo illisible",
            egui::FontId::proportional(16.0),
            egui::Color32::WHITE,
        );
        return;
    };
    let tint = egui::Color32::from_white_alpha((opacity.clamp(0.0, 1.0) * 255.0) as u8);
    painter.image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A PNG file of the given size, with a smooth gradient.
    pub fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let mut img = image::RgbaImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgba([(x * 255 / w.max(1)) as u8, (y * 255 / h.max(1)) as u8, 128, 255]);
        }
        let mut out = Vec::new();
        DynamicImage::ImageRgba8(img).write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn decodes_png_size() {
        assert_eq!(image_size(&png_bytes(64, 48)).unwrap(), [64, 48]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(image_size(b"pas une image").is_err());
        assert!(decode(&[]).is_err());
    }
}
