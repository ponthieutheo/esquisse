//! The native JPEG decoder (`native::jpeg`) against SketchUp's own (`SUImageRepLoadFile`:
//! libjpeg-turbo through OpenImageIO): the same pixels, to the bit, so that the average colour of
//! a texture, and the colorization offsets SketchUp derives from it, are SketchUp's
//! (`docs/skp-format.md` §9).
//!
//! `jpegs_decode_as_sketchup` (always; skipped without SketchUp): JPEGs made by the `image`
//! crate's encoder (colour and grey, sizes that are not multiples of a block, several
//! qualities) and the first JPEG textures of SketchUp's material library.
//! `real_jpeg_textures_decode_as_sketchup` (`--ignored`, best in release): every JPEG texture
//! of the real corpus (`native_reel.rs`), baseline, progressive and with restart markers.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use esq_skp::native::jpeg;

mod oracle;

/// A folder of this test's own, removed at the end.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let d = std::env::temp_dir().join(format!("esq-jpeg-sdk-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Compares the two decoders on one JPEG; the first difference otherwise.
fn same_as_sdk(bytes: &[u8], dir: &Path, name: &str) -> Result<(), String> {
    let path = dir.join("image.jpg");
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let (w, h, ch, sdk) = esq_skp::sdk_decode_image(&path).map_err(|e| format!("{name} : SDK : {e}"))?;
    let ours = jpeg::decode(bytes, 1 << 30).ok_or_else(|| format!("{name} : refusé par le décodeur natif"))?;
    if (ours.width, ours.height, ours.channels) != (w, h, ch) {
        return Err(format!("{name} : {}×{}×{} au lieu de {w}×{h}×{ch}", ours.width, ours.height, ours.channels));
    }
    if let Some(i) = (0..sdk.len()).find(|&i| sdk[i] != ours.data[i]) {
        let px = i / ch;
        return Err(format!(
            "{name} : pixel ({}, {}) canal {} : {} au lieu de {}",
            px % w,
            px / w,
            i % ch,
            ours.data[i],
            sdk[i]
        ));
    }
    Ok(())
}

/// A picture with detail in every channel, so that every coefficient of the blocks matters.
fn picture(w: u32, h: u32) -> image::RgbImage {
    image::RgbImage::from_fn(w, h, |x, y| {
        let r = (x * 255 / w.max(1)) as u8;
        let g = ((x * 7 + y * 13) % 256) as u8;
        let b = (((x ^ y) * 37) % 256) as u8;
        image::Rgb([r, g, b])
    })
}

fn encode(img: &image::DynamicImage, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    enc.encode_image(img).unwrap();
    out
}

/// The JPEG textures of SketchUp's material library (`.skm` archives), `n` at most.
fn library_jpegs(n: usize) -> Vec<(String, Vec<u8>)> {
    let root = PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into()))
        .join("SketchUp")
        .join("SketchUp 2026")
        .join("SketchUp")
        .join("Materials");
    let mut skm = Vec::new();
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("skm")) {
                skm.push(p);
            }
        }
    }
    skm.sort();
    let mut out = Vec::new();
    for p in skm {
        let Ok(data) = std::fs::read(&p) else { continue };
        let Ok(entries) = esq_core::zip::read_matching(&data, |n| {
            let n = n.to_ascii_lowercase();
            n.ends_with(".jpg") || n.ends_with(".jpeg")
        }) else {
            continue;
        };
        for e in entries.into_iter().filter(|e| jpeg::is_jpeg(&e.data)) {
            out.push((format!("{} › {}", p.display(), e.name), e.data));
            if out.len() >= n {
                return out;
            }
        }
    }
    out
}

#[test]
fn jpegs_decode_as_sketchup() {
    if !esq_skp::available() {
        eprintln!("SketchUp absent : test ignoré");
        return;
    }
    let dir = Scratch::new("echantillon");
    let mut cases: Vec<(String, Vec<u8>)> = Vec::new();
    for (w, h) in [(1, 1), (8, 8), (17, 9), (64, 48), (99, 37)] {
        let img = image::DynamicImage::ImageRgb8(picture(w, h));
        for q in [30, 75, 95] {
            cases.push((format!("couleur {w}×{h} q{q}"), encode(&img, q)));
        }
        cases.push((format!("gris {w}×{h}"), encode(&image::DynamicImage::ImageLuma8(img.to_luma8()), 80)));
    }
    let library = library_jpegs(6);
    cases.extend(library.iter().cloned());
    let bad: Vec<String> = cases.iter().filter_map(|(n, b)| same_as_sdk(b, &dir.0, n).err()).collect();
    assert!(bad.is_empty(), "pixels différents de ceux de SketchUp :\n{}", bad.join("\n"));
    println!("{} JPEG identiques à SketchUp (dont {} de sa bibliothèque)", cases.len(), library.len());
}

#[test]
#[ignore]
fn real_jpeg_textures_decode_as_sketchup() {
    if !esq_skp::available() {
        eprintln!("SketchUp absent : test ignoré");
        return;
    }
    let root = std::env::var_os("ESQ_SKP_REEL")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("tools").join("skp-reel")));
    let mut files = Vec::new();
    if let Some(r) = &root {
        oracle::walk(r, &mut files, 0);
    }
    files.sort();
    if files.is_empty() {
        eprintln!("corpus réel absent : test ignoré");
        return;
    }
    let dir = Scratch::new("reel");
    let mut seen = HashSet::new();
    let (mut n, mut bad) = (0usize, Vec::new());
    for f in &files {
        let Ok(b) = std::fs::read(f) else { continue };
        let Ok((m, _)) = esq_skp::native::read_bytes(&b) else { continue };
        for mt in m.materials.values() {
            let Some(t) = &mt.ext.texture else { continue };
            let bytes = &t.image.0;
            if !jpeg::is_jpeg(bytes) || !seen.insert(bytes.clone()) {
                continue;
            }
            n += 1;
            if let Err(e) = same_as_sdk(bytes, &dir.0, &format!("{} › {}", f.display(), t.filename)) {
                bad.push(e);
            }
        }
    }
    println!("{n} textures JPEG distinctes, {} différentes de SketchUp", bad.len());
    assert!(bad.is_empty(), "pixels différents de ceux de SketchUp :\n{}", bad.join("\n"));
}
