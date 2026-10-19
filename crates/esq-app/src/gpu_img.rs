//! Pictures handed to the GPU, never larger than what the card takes. A texture beyond the
//! card's side makes egui assert and wgpu refuse it, which ends the application; and the
//! pictures come from files (styles, plugins, materials, environments, scripts), so their size
//! is anyone's. Every egui texture made from a picture goes through [`load_texture`].

use std::borrow::Cow;

use eframe::egui;

/// The longest side kept for display, whatever the card takes (memory: 256 MB at 8192²).
pub const MAX_SIDE: u32 = 8192;

/// `rgba` (`w` × `h`, 4 bytes a pixel) reduced, proportions kept, until neither side is above
/// `max`; as it is when it already fits. A buffer too short for its size gives one clear pixel.
pub fn fit(w: u32, h: u32, rgba: &[u8], max: u32) -> (u32, u32, Cow<'_, [u8]>) {
    let max = max.max(1);
    let (w, h) = (w.max(1), h.max(1));
    let Some(img) = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(w, h, rgba) else {
        return (1, 1, Cow::Owned(vec![0; 4]));
    };
    if w <= max && h <= max {
        return (w, h, Cow::Borrowed(&rgba[..(w as usize * h as usize * 4)]));
    }
    let k = max as f64 / w.max(h) as f64;
    let side = |n: u32| ((n as f64 * k).round() as u32).clamp(1, max);
    let (nw, nh) = (side(w), side(h));
    let small = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle);
    (nw, nh, Cow::Owned(small.into_raw()))
}

/// The longest texture side for this context: the card's (egui's `max_texture_side`), within
/// [`MAX_SIDE`].
pub fn max_side(ctx: &egui::Context) -> u32 {
    (ctx.input(|i| i.max_texture_side).min(u32::MAX as usize) as u32).min(MAX_SIDE)
}

/// The longest texture side for a wgpu device, within [`MAX_SIDE`].
pub fn device_max_side(device: &eframe::egui_wgpu::wgpu::Device) -> u32 {
    device.limits().max_texture_dimension_2d.min(MAX_SIDE)
}

/// `ctx.load_texture` for a picture of any size (`size` = width, height; `rgba` unmultiplied):
/// reduced first to what the card takes.
pub fn load_texture(
    ctx: &egui::Context,
    name: impl Into<String>,
    size: [usize; 2],
    rgba: &[u8],
    options: egui::TextureOptions,
) -> egui::TextureHandle {
    let (w, h, px) = fit(size[0] as u32, size[1] as u32, rgba, max_side(ctx));
    ctx.load_texture(name, egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px), options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_brought_within_the_side_with_its_proportions() {
        let px = vec![200u8; 1000 * 10 * 4];
        let (w, h, out) = fit(1000, 10, &px, 256);
        assert_eq!((w, h, out.len()), (256, 3, 256 * 3 * 4));
        let (w, h, out) = fit(100, 50, &px[..100 * 50 * 4], 256);
        assert!(matches!(out, Cow::Borrowed(_)) && (w, h) == (100, 50));
        // A buffer shorter than its size, or an empty picture: one clear pixel, no panic.
        assert_eq!(fit(1000, 1000, &px, 256).0, 1);
        assert_eq!(fit(0, 0, &[], 256).2.len(), 4);
    }

    #[test]
    fn a_texture_never_exceeds_the_cards_side() {
        let ctx = egui::Context::default();
        let px = vec![255u8; 5000 * 20 * 4];
        let mut size = [0, 0];
        // The smallest side egui takes (its font atlas needs 1024 at least).
        let input = egui::RawInput { max_texture_side: Some(2048), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            // egui asserts (debug) when a texture is above `max_texture_side`.
            size = load_texture(ui.ctx(), "large", [5000, 20], &px, egui::TextureOptions::LINEAR).size();
        });
        out.textures_delta.clear();
        assert_eq!(size, [2048, 8]);
    }

    /// Every egui texture made in the application goes through [`load_texture`].
    #[test]
    fn no_texture_is_loaded_around_the_check() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![src];
        let mut found = Vec::new();
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("gpu_img.rs") {
                    let text = std::fs::read_to_string(&p).unwrap();
                    for (i, l) in text.lines().enumerate() {
                        if l.contains("ctx.load_texture(") || l.contains("ctx().load_texture(") {
                            found.push(format!("{}:{}", p.display(), i + 1));
                        }
                    }
                }
            }
        }
        assert!(found.is_empty(), "textures loaded without gpu_img::load_texture: {found:?}");
    }
}
