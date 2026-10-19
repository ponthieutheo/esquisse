//! Material textures for the 3D view: each image file is decoded once (and colorized the way
//! its material says), then shared by every rebuild of the scene. The renderer packs them in
//! one texture array, one layer per texture (see `render.rs`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use esq_core::meta::MaterialExt;
use esq_core::photo::Blob;

/// The pixels of a texture, RGBA, sRGB-encoded, unpremultiplied.
pub struct Texels {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    /// Some pixel is not fully opaque (the face is then drawn with blending).
    pub alpha: bool,
}

type Key = (usize, u8, [u64; 3]);

struct Entry {
    /// The file's bytes: the key is their address, valid while they live.
    blob: Weak<Vec<u8>>,
    texels: Option<Arc<Texels>>,
}

static CACHE: Mutex<Option<HashMap<Key, Entry>>> = Mutex::new(None);

/// The texture of a material as the view shows it: its image, colorized when the material
/// says so. `None` when the material has no texture or the image cannot be decoded.
pub fn of_material(ext: &MaterialExt) -> Option<Arc<Texels>> {
    let t = ext.texture.as_ref()?;
    let deltas = if ext.colorize_deltas == [0.0; 3] { None } else { Some(ext.colorize_deltas) };
    get(&t.image, deltas.map(|d| (ext.colorize_type, d)))
}

/// Decoded (and colorized) pixels of an image file, from the cache.
pub fn get(blob: &Blob, colorize: Option<(u8, [f64; 3])>) -> Option<Arc<Texels>> {
    let key: Key = (
        Arc::as_ptr(&blob.0) as usize,
        colorize.map_or(0, |c| c.0 + 1),
        colorize.map_or([0; 3], |c| c.1.map(f64::to_bits)),
    );
    let mut guard = CACHE.lock().ok()?;
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(e) = cache.get(&key)
        && e.blob.upgrade().is_some_and(|b| Arc::ptr_eq(&b, &blob.0))
    {
        return e.texels.clone();
    }
    // Forget the textures whose files are gone (their address may be reused).
    cache.retain(|_, e| e.blob.strong_count() > 0);
    let texels = decode(&blob.0).map(|t| match colorize {
        Some((kind, d)) => colorized(&t, kind, d),
        None => t,
    });
    let texels = texels.map(Arc::new);
    cache.insert(key, Entry { blob: Arc::downgrade(&blob.0), texels: texels.clone() });
    texels
}

fn decode(bytes: &[u8]) -> Option<Texels> {
    let img = crate::photo_img::decode(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let rgba = img.into_raw();
    let alpha = rgba.as_chunks::<4>().0.iter().any(|p| p[3] < 255);
    Some(Texels { w, h, rgba, alpha })
}

/// RGB (0–1) → hue (degrees), saturation, lightness.
pub fn hls(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = rgb;
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let l = (mx + mn) / 2.0;
    if mx - mn < 1e-12 {
        return [0.0, 0.0, l];
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if mx == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [h * 60.0, s, l]
}

/// Hue (degrees), saturation, lightness → RGB (0–1).
pub fn rgb(hls: [f64; 3]) -> [f64; 3] {
    let [h, s, l] = hls;
    if s <= 0.0 {
        return [l; 3];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let h = h.rem_euclid(360.0) / 360.0;
    let f = |t: f64| {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0)]
}

/// A colorized texture (Materials › Edit › Colorize): `deltas` are the hue, saturation and
/// lightness offsets from the texture's average colour to the material's colour. Kind 0
/// shifts every pixel by them; kind 1 (tint) gives every pixel the material's hue and
/// saturation and keeps its shifted lightness.
pub fn colorized(t: &Texels, kind: u8, deltas: [f64; 3]) -> Texels {
    let avg = average(t);
    let target = [avg[0] + deltas[0], (avg[1] + deltas[1]).clamp(0.0, 1.0), avg[2] + deltas[2]];
    let mut rgba = t.rgba.clone();
    for p in rgba.as_chunks_mut::<4>().0 {
        let c = hls([p[0], p[1], p[2]].map(|v| v as f64 / 255.0));
        let out = if kind == 1 {
            [target[0], target[1], (c[2] + deltas[2]).clamp(0.0, 1.0)]
        } else {
            [c[0] + deltas[0], (c[1] + deltas[1]).clamp(0.0, 1.0), (c[2] + deltas[2]).clamp(0.0, 1.0)]
        };
        let o = rgb(out);
        for i in 0..3 {
            p[i] = (o[i] * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    Texels { w: t.w, h: t.h, rgba, alpha: t.alpha }
}

/// Average colour of a texture, as hue, saturation and lightness.
fn average(t: &Texels) -> [f64; 3] {
    let n = (t.rgba.len() / 4).max(1) as f64;
    let mut sum = [0.0; 3];
    for p in t.rgba.as_chunks::<4>().0 {
        for i in 0..3 {
            sum[i] += p[i] as f64;
        }
    }
    hls(sum.map(|s| s / n / 255.0))
}

/// Shrinks (or enlarges) a texture to `side` × `side` pixels, bilinear.
pub fn resized(t: &Texels, side: u32) -> Vec<u8> {
    if t.w == side && t.h == side {
        return t.rgba.clone();
    }
    let img = image::RgbaImage::from_raw(t.w, t.h, t.rgba.clone()).expect("taille cohérente");
    image::imageops::resize(&img, side, side, image::imageops::FilterType::Triangle).into_raw()
}

/// The next mipmap level of a square RGBA image (half the side, 2×2 average).
pub fn half(rgba: &[u8], side: u32) -> Vec<u8> {
    let n = (side / 2).max(1);
    let s = side as usize;
    let mut out = vec![0u8; (n * n * 4) as usize];
    for y in 0..n as usize {
        for x in 0..n as usize {
            for c in 0..4 {
                let at = |xx: usize, yy: usize| rgba[((yy.min(s - 1)) * s + xx.min(s - 1)) * 4 + c] as u32;
                let v = at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1);
                out[(y * n as usize + x) * 4 + c] = ((v + 2) / 4) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_once_and_shared() {
        let blob = Blob(Arc::new(crate::photo_img::tests::png_bytes(8, 4)));
        let a = get(&blob, None).unwrap();
        let b = get(&blob, None).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!((a.w, a.h, a.rgba.len()), (8, 4, 128));
        assert!(!a.alpha);
        assert!(get(&Blob(Arc::new(b"rien".to_vec())), None).is_none());
    }

    #[test]
    fn hls_round_trips() {
        for c in [[1.0, 0.0, 0.0], [0.2, 0.5, 0.9], [0.5, 0.5, 0.5], [0.9, 0.8, 0.1]] {
            let back = rgb(hls(c));
            for i in 0..3 {
                assert!((back[i] - c[i]).abs() < 1e-9, "{c:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn colorizing_shifts_the_hue() {
        // A red texture shifted by 120°: green.
        let t = Texels { w: 1, h: 1, rgba: vec![200, 0, 0, 255], alpha: false };
        let g = colorized(&t, 0, [120.0, 0.0, 0.0]);
        assert_eq!(&g.rgba[..3], &[0, 200, 0]);
        // Tint: every pixel takes the target hue, keeps its lightness.
        let two = Texels { w: 2, h: 1, rgba: vec![200, 0, 0, 255, 100, 0, 0, 255], alpha: false };
        let tint = colorized(&two, 1, [240.0, 0.0, 0.0]);
        assert!(tint.rgba[2] > tint.rgba[0] && tint.rgba[6] > tint.rgba[4]);
        assert!(tint.rgba[2] > tint.rgba[6], "lightness kept");
    }

    #[test]
    fn mipmaps_average() {
        let px = [10u8, 20, 30, 255, 30, 40, 50, 255, 50, 60, 70, 255, 70, 80, 90, 255];
        assert_eq!(half(&px, 2), vec![40, 50, 60, 255]);
    }
}
