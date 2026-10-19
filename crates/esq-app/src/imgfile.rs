//! Image files that File › Import takes (SketchUp's image importers: `CBmpImageImporter`,
//! `CJpgImageImporter`, `CPngImageImporter`, `CPsdImageImporter`, `CTgaImageImporter`,
//! `CTifImageImporter`, and `PdfImageImporter`, which renders the first page of a PDF at
//! 150 dpi, at most 10 000 pixels on a side, and imports that picture).
//!
//! The model keeps the image as PNG or JPEG bytes: other formats are converted when loaded,
//! so that every part of Esquisse (display, .esq, .skp) reads them the same way.

use std::io::Cursor;
use std::path::Path;

use esq_core::meta::TextureData;
use esq_core::photo::Blob;
use image::{DynamicImage, ImageFormat, RgbaImage};

/// A picture ready to go into the model.
#[derive(Clone, Debug)]
pub struct LoadedImage {
    /// File name to keep (its extension follows the stored format).
    pub name: String,
    /// The original file's path, as `Image#path` reports it.
    pub path: String,
    /// PNG or JPEG bytes.
    pub bytes: Vec<u8>,
    pub px: [u32; 2],
    pub avg: [u8; 3],
}

impl LoadedImage {
    /// The image as a texture whose tile is `size` (mm).
    pub fn texture(&self, size: [f64; 2]) -> TextureData {
        TextureData {
            filename: esq_core::meta::base_name(&self.name),
            image: Blob(std::sync::Arc::new(self.bytes.clone())),
            px: self.px,
            size,
            avg: self.avg,
        }
    }

    /// File name without extension (names the image's definition and material).
    pub fn stem(&self) -> String {
        Path::new(&self.name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Image".into())
    }
}

/// Extensions of the image types SketchUp imports, in the order of its file dialog.
pub const IMAGE_EXTS: [&str; 8] = ["bmp", "jpg", "jpeg", "png", "psd", "tif", "tiff", "tga"];

pub fn is_image(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    IMAGE_EXTS.contains(&ext.as_str()) || ext == "pdf"
}

/// `PdfImageImporter`: 150 dpi, never more than 10 000 pixels on a side.
pub const PDF_DPI: f64 = 150.0;
pub const PDF_MAX_PX: f64 = 10_000.0;

/// Reads an image file (any type of `IMAGE_EXTS`, or a PDF).
pub fn load(path: &Path) -> Result<LoadedImage, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Lecture impossible : {e}"))?;
    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "image".into());
    let mut img = from_bytes(&name, bytes)?;
    img.path = path.to_string_lossy().into_owned();
    Ok(img)
}

/// Like `load`, from the file's bytes; `name` gives the type by its extension.
pub fn from_bytes(name: &str, bytes: Vec<u8>) -> Result<LoadedImage, String> {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    let invalid = |e: String| format!("Fichier d'image non valide ! ({e})");
    let (decoded, keep) = match ext.as_str() {
        "psd" => (decode_psd(&bytes).map_err(invalid)?, false),
        "pdf" => (render_pdf(&bytes).map_err(invalid)?, false),
        "tga" => {
            (image::load_from_memory_with_format(&bytes, ImageFormat::Tga).map_err(|e| invalid(e.to_string()))?, false)
        }
        _ => {
            use image::{ImageDecoder, ImageReader, metadata::Orientation};
            let reader =
                ImageReader::new(Cursor::new(&bytes)).with_guessed_format().map_err(|e| invalid(e.to_string()))?;
            let fmt = reader.format();
            let mut dec = reader.into_decoder().map_err(|e| invalid(e.to_string()))?;
            let orientation = dec.orientation().ok().filter(|o| *o != Orientation::NoTransforms);
            let mut img = DynamicImage::from_decoder(dec).map_err(|e| invalid(e.to_string()))?;
            if let Some(o) = orientation {
                img.apply_orientation(o);
            }
            // PNG and JPEG are kept as they are, unless the photo has to be turned upright.
            (img, matches!(fmt, Some(ImageFormat::Png) | Some(ImageFormat::Jpeg)) && orientation.is_none())
        }
    };
    let (w, h) = (decoded.width(), decoded.height());
    if w == 0 || h == 0 {
        return Err(invalid("image vide".into()));
    }
    let avg = average(&decoded);
    let (bytes, name) = if keep {
        (bytes, name.to_string())
    } else {
        let mut out = Vec::new();
        decoded.write_to(&mut Cursor::new(&mut out), ImageFormat::Png).map_err(|e| invalid(e.to_string()))?;
        let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        (out, format!("{stem}.png"))
    };
    Ok(LoadedImage { name, path: String::new(), bytes, px: [w, h], avg })
}

/// Average colour (over at most about 65 000 evenly spread pixels), the colour SketchUp shows
/// for a textured material when textures are off.
pub fn average(img: &DynamicImage) -> [u8; 3] {
    let rgb = img.to_rgb8();
    let step = ((rgb.width() as u64 * rgb.height() as u64) / 65_536).max(1) as usize;
    let (mut s, mut n) = ([0u64; 3], 0u64);
    for p in rgb.pixels().step_by(step) {
        for (acc, v) in s.iter_mut().zip(p.0) {
            *acc += v as u64;
        }
        n += 1;
    }
    let n = n.max(1);
    [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]
}

// ------------------------------------------------------------------------------------------
// Photoshop (.psd / .psb): the merged picture that ends the file ("image data" section),
// written from Adobe's published file format specification.

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.b.len()).ok_or("fichier tronqué")?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn skip_block(&mut self, wide: bool) -> Result<(), String> {
        let n = if wide { self.u64()? as usize } else { self.u32()? as usize };
        self.take(n).map(|_| ())
    }
}

/// PackBits run-length decoding of one row into `out` (exactly `len` bytes).
fn unpack_bits(src: &[u8], len: usize, out: &mut Vec<u8>) -> Result<(), String> {
    let start = out.len();
    let mut i = 0;
    while out.len() - start < len {
        let n = *src.get(i).ok_or("ligne RLE tronquée")? as i8;
        i += 1;
        if n >= 0 {
            let k = n as usize + 1;
            out.extend_from_slice(src.get(i..i + k).ok_or("ligne RLE tronquée")?);
            i += k;
        } else if n != -128 {
            let v = *src.get(i).ok_or("ligne RLE tronquée")?;
            i += 1;
            out.extend(std::iter::repeat_n(v, (1 - n as isize) as usize));
        }
    }
    out.truncate(start + len);
    Ok(())
}

pub fn decode_psd(bytes: &[u8]) -> Result<DynamicImage, String> {
    let mut r = Reader { b: bytes, at: 0 };
    if r.take(4)? != b"8BPS" {
        return Err("signature Photoshop absente".into());
    }
    let version = r.u16()?;
    let wide = match version {
        1 => false,
        2 => true,
        v => return Err(format!("version Photoshop {v} inconnue")),
    };
    r.take(6)?;
    let channels = r.u16()? as usize;
    let h = r.u32()? as usize;
    let w = r.u32()? as usize;
    let depth = r.u16()?;
    let mode = r.u16()?;
    // The header's sizes are checked before anything is made: a 30-byte file declaring
    // 30000 × 30000 pixels of 65535 channels made the reader reserve 59 TB (the application
    // died). Photoshop has at most 56 channels; the picture made is at most 512 MB, the
    // limit of the other image readers.
    if w == 0 || h == 0 || channels == 0 || channels > 56 || w * h > (512 << 20) / 4 {
        return Err("taille d'image incohérente".into());
    }
    let palette = {
        let n = r.u32()? as usize;
        r.take(n)?.to_vec()
    };
    r.skip_block(false)?; // image resources
    r.skip_block(wide)?; // layers and masks
    let compression = r.u16()?;
    let bpc = match depth {
        1 => 0,
        8 => 1,
        16 => 2,
        32 => 4,
        d => return Err(format!("profondeur {d} bits non prise en charge")),
    };
    let row = if depth == 1 { w.div_ceil(8) } else { w * bpc };
    let size = row * h * channels;
    // What the file can hold: as is, or run-length encoded (128 bytes from 2 at best).
    let left = bytes.len().saturating_sub(r.at);
    if size > left.saturating_mul(64).saturating_add(1 << 16) {
        return Err("données d'image tronquées".into());
    }
    let mut planes: Vec<u8> = Vec::with_capacity(size);
    match compression {
        0 => planes.extend_from_slice(r.take(size)?),
        1 => {
            let counts: Vec<usize> = (0..h * channels)
                .map(|_| if wide { r.u32().map(|x| x as usize) } else { r.u16().map(|x| x as usize) })
                .collect::<Result<_, _>>()?;
            for c in counts {
                unpack_bits(r.take(c)?, row, &mut planes)?;
            }
        }
        c => return Err(format!("compression {c} non prise en charge")),
    }
    // One 8-bit sample of channel `c` at pixel `i` (16 and 32 bits keep the high byte).
    let sample = |c: usize, i: usize| -> u8 {
        let (y, x) = (i / w, i % w);
        let base = c * row * h + y * row;
        match depth {
            1 => {
                // Bitmap: 1 = black.
                if planes[base + x / 8] & (0x80 >> (x % 8)) != 0 { 0 } else { 255 }
            }
            32 => {
                let o = base + x * 4;
                let f = f32::from_be_bytes([planes[o], planes[o + 1], planes[o + 2], planes[o + 3]]);
                (f.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8
            }
            _ => planes[base + x * bpc],
        }
    };
    let mut out = RgbaImage::new(w as u32, h as u32);
    for (i, p) in out.pixels_mut().enumerate() {
        let a = |k: usize| if channels > k { sample(k, i) } else { 255 };
        p.0 = match mode {
            // RGB (an extra channel is the merged transparency).
            3 if channels >= 3 => [sample(0, i), sample(1, i), sample(2, i), a(3)],
            // Grayscale, duotone (shown as its gray), bitmap.
            0 | 1 | 8 => {
                let g = sample(0, i);
                [g, g, g, a(1)]
            }
            // Indexed: 256-entry palette stored as all reds, all greens, all blues.
            2 => {
                let k = sample(0, i) as usize;
                let pal = |c: usize| palette.get(c * 256 + k).copied().unwrap_or(0);
                [pal(0), pal(1), pal(2), 255]
            }
            // CMYK: Photoshop stores 255 - ink.
            4 if channels >= 4 => {
                let (c, m, y, k) = (sample(0, i) as u32, sample(1, i) as u32, sample(2, i) as u32, sample(3, i) as u32);
                [(c * k / 255) as u8, (m * k / 255) as u8, (y * k / 255) as u8, a(4)]
            }
            mode => return Err(format!("mode de couleur Photoshop {mode} non pris en charge")),
        };
    }
    Ok(DynamicImage::ImageRgba8(out))
}

// ------------------------------------------------------------------------------------------
// PDF: the first page, rasterized like `PdfImageImporter` does.

/// Pixel size of a page of `w` × `h` points at 150 dpi, reduced to 10 000 pixels at most.
pub fn pdf_pixels(w: f64, h: f64) -> (u32, u32, f64) {
    let k = PDF_DPI / 72.0;
    let (mut pw, mut ph) = ((w * k) as i64, (h * k) as i64);
    let mut scale = k;
    if pw as f64 > PDF_MAX_PX || ph as f64 > PDF_MAX_PX {
        let m = pw.max(ph) as f64;
        pw = (pw as f64 * (PDF_MAX_PX / m)) as i64;
        ph = (ph as f64 * (PDF_MAX_PX / m)) as i64;
        scale *= PDF_MAX_PX / m;
    }
    (pw.max(1) as u32, ph.max(1) as u32, scale)
}

pub fn render_pdf(bytes: &[u8]) -> Result<DynamicImage, String> {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
    let pdf = Pdf::new(bytes.to_vec()).map_err(|e| format!("PDF illisible ({e:?})"))?;
    let pages = pdf.pages();
    let page = pages.first().ok_or("le PDF n'a aucune page")?;
    let (w, h) = page.render_dimensions();
    let (_, _, scale) = pdf_pixels(w as f64, h as f64);
    let settings = PixmapSettings { x_scale: scale as f32, y_scale: scale as f32, bg_color: WHITE };
    let pix = render(page, &RenderCache::new(), &InterpreterSettings::default(), &RenderSettings::default(), &settings);
    let png = pix.into_png().map_err(|e| format!("rendu PDF ({e:?})"))?;
    image::load_from_memory_with_format(&png, ImageFormat::Png).map_err(|e| e.to_string())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A small RGB Photoshop file (raw or RLE), 3 × 2 with an alpha channel.
    pub fn psd_bytes(rle: bool) -> Vec<u8> {
        let (w, h) = (3u32, 2u32);
        let mut b = b"8BPS".to_vec();
        b.extend_from_slice(&1u16.to_be_bytes());
        b.extend_from_slice(&[0; 6]);
        b.extend_from_slice(&4u16.to_be_bytes()); // R, G, B, A
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&8u16.to_be_bytes());
        b.extend_from_slice(&3u16.to_be_bytes()); // RGB
        b.extend_from_slice(&0u32.to_be_bytes()); // colour mode data
        b.extend_from_slice(&0u32.to_be_bytes()); // resources
        b.extend_from_slice(&0u32.to_be_bytes()); // layers
        let planes: [[u8; 6]; 4] =
            [[255, 0, 0, 10, 20, 30], [0, 255, 0, 10, 20, 30], [0, 0, 255, 10, 20, 30], [255; 6]];
        if rle {
            b.extend_from_slice(&1u16.to_be_bytes());
            let mut rows = Vec::new();
            for p in &planes {
                for y in 0..2 {
                    let row = &p[y * 3..y * 3 + 3];
                    // A literal run of 3 bytes.
                    let mut enc = vec![2u8];
                    enc.extend_from_slice(row);
                    rows.push(enc);
                }
            }
            for r in &rows {
                b.extend_from_slice(&(r.len() as u16).to_be_bytes());
            }
            for r in rows {
                b.extend(r);
            }
        } else {
            b.extend_from_slice(&0u16.to_be_bytes());
            for p in &planes {
                b.extend_from_slice(p);
            }
        }
        b
    }

    #[test]
    fn photoshop_files_are_read() {
        for rle in [false, true] {
            let img = decode_psd(&psd_bytes(rle)).unwrap().to_rgba8();
            assert_eq!(img.dimensions(), (3, 2));
            assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
            assert_eq!(img.get_pixel(2, 0).0, [0, 0, 255, 255]);
            assert_eq!(img.get_pixel(1, 1).0, [20, 20, 20, 255]);
        }
        assert!(decode_psd(b"8BPS").is_err());
        assert!(decode_psd(b"GIF89a").is_err());
        let mut runs = Vec::new();
        unpack_bits(&[0xFE, 7, 1, 9, 8], 5, &mut runs).unwrap();
        assert_eq!(runs, [7, 7, 7, 9, 8]);
    }

    /// A Photoshop header of a few bytes declaring 30000 × 30000 pixels of 65535 channels
    /// (or 16000 × 16000 of 4 channels, with no data): the reader reserved the planes'
    /// size at once (59 TB, the process died on the allocation). Refused from the header.
    #[test]
    fn photoshop_headers_cannot_ask_for_terabytes() {
        let header = |w: u32, h: u32, channels: u16, compression: u16| {
            let mut b = b"8BPS".to_vec();
            b.extend_from_slice(&1u16.to_be_bytes());
            b.extend_from_slice(&[0; 6]);
            b.extend_from_slice(&channels.to_be_bytes());
            b.extend_from_slice(&h.to_be_bytes());
            b.extend_from_slice(&w.to_be_bytes());
            b.extend_from_slice(&8u16.to_be_bytes());
            b.extend_from_slice(&3u16.to_be_bytes());
            b.extend_from_slice(&[0; 12]);
            b.extend_from_slice(&compression.to_be_bytes());
            b
        };
        assert!(decode_psd(&header(30_000, 30_000, 65_535, 1)).is_err());
        assert!(decode_psd(&header(16_000, 16_000, 4, 1)).is_err());
        // Within the picture limit, but far more than the file could hold.
        assert!(decode_psd(&header(8_000, 8_000, 4, 1)).is_err());
        assert!(decode_psd(&header(100_000, 100_000, 3, 0)).is_err());
    }

    #[test]
    fn every_type_is_loaded_and_kept_as_png_or_jpeg() {
        let base = image::RgbaImage::from_fn(8, 4, |x, y| image::Rgba([x as u8 * 30, y as u8 * 60, 100, 255]));
        let img = DynamicImage::ImageRgba8(base);
        for (ext, fmt) in [
            ("png", ImageFormat::Png),
            ("jpg", ImageFormat::Jpeg),
            ("bmp", ImageFormat::Bmp),
            ("tif", ImageFormat::Tiff),
            ("tga", ImageFormat::Tga),
        ] {
            let mut bytes = Vec::new();
            let src = if fmt == ImageFormat::Jpeg { DynamicImage::ImageRgb8(img.to_rgb8()) } else { img.clone() };
            src.write_to(&mut Cursor::new(&mut bytes), fmt).unwrap();
            let l = from_bytes(&format!("plan.{ext}"), bytes.clone()).unwrap();
            assert_eq!(l.px, [8, 4], "{ext}");
            let stored = image::guess_format(&l.bytes).unwrap();
            if matches!(ext, "png" | "jpg") {
                assert_eq!(l.bytes, bytes, "{ext} kept as it is");
                assert_eq!(l.name, format!("plan.{ext}"));
            } else {
                assert_eq!(stored, ImageFormat::Png, "{ext} converted");
                assert_eq!(l.name, "plan.png");
            }
        }
        let psd = from_bytes("calque.psd", psd_bytes(true)).unwrap();
        assert_eq!((psd.px, psd.name.as_str()), ([3, 2], "calque.png"));
        assert!(from_bytes("x.png", b"pas une image".to_vec()).unwrap_err().starts_with("Fichier d'image non valide"));
        assert_eq!(psd.stem(), "calque");
        let t = psd.texture([10.0, 20.0]);
        assert_eq!((t.px, t.size), ([3, 2], [10.0, 20.0]));
    }

    #[test]
    fn pdf_pages_are_rendered_at_150_dpi() {
        // An A4 page: 595 × 842 points.
        assert_eq!(pdf_pixels(595.0, 842.0).0, 1239);
        // A sheet of 5000 × 7000 points at 150 dpi would exceed 10 000 pixels: reduced.
        let (w, h, k) = pdf_pixels(5000.0, 7000.0);
        assert_eq!(h, 10_000);
        assert!(w.abs_diff(7142) <= 1, "{w}");
        assert!((k * 7000.0 - 10_000.0).abs() < 2.0);
        let pdf = crate::print::pdf_bytes(
            &crate::print::PrintOptions::default().layout(1.0),
            2,
            2,
            &[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 0, 0, 255],
            "t",
        );
        let img = from_bytes("plan.pdf", pdf).unwrap();
        // A4 landscape (841.9 × 595.3 points) at 150 dpi.
        assert!(img.px[0].abs_diff(1754) <= 1 && img.px[1].abs_diff(1240) <= 1, "{:?}", img.px);
        assert_eq!(img.name, "plan.png");
    }
}
