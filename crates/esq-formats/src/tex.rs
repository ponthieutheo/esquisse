//! Texture images going in and out of files. A texture name read from a file is only ever a
//! bare file name: it is looked up next to the imported file (or inside its package), never
//! used as a path.

use std::path::Path;
use std::sync::Arc;

use esq_core::meta::{TextureData, base_name};
use esq_core::photo::Blob;

/// A texture from an image file's bytes; `size_mm` is the tile size (default: one pixel per
/// inch, like SketchUp).
pub fn texture_from_bytes(filename: &str, bytes: Vec<u8>, size_mm: Option<[f64; 2]>) -> Option<TextureData> {
    let img = image::load_from_memory(&bytes).ok()?;
    let (w, h) = (img.width(), img.height());
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
    Some(TextureData {
        filename: base_name(filename),
        image: Blob(Arc::new(bytes)),
        px: [w, h],
        size: size_mm.unwrap_or([w as f64 * 25.4, h as f64 * 25.4]),
        avg,
    })
}

/// The bare file name of a reference found in a file ("../maps/wood.jpg" → "wood.jpg"),
/// percent-escapes decoded. `None` for a name that cannot be a file (empty, "..").
pub fn safe_name(reference: &str) -> Option<String> {
    let r = reference.trim();
    let r = r.strip_prefix("file:///").or_else(|| r.strip_prefix("file://")).unwrap_or(r);
    // Byte by byte: the old decoder sliced the str after a '%' and panicked on "%aé".
    let decoded = esq_core::untrusted::percent_decode(r);
    let name = decoded.rsplit(['/', '\\']).next()?.rsplit(':').next()?.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains('\0') {
        return None;
    }
    Some(base_name(name))
}

/// Finds the image a file refers to: by its bare name, in the imported file's folder or one
/// of its direct sub-folders (SketchUp writes textures in a folder named after the model).
pub fn find_beside(model_file: &Path, reference: &str) -> Option<(String, Vec<u8>)> {
    let name = safe_name(reference)?;
    let dir = model_file.parent()?;
    let lower = name.to_lowercase();
    let try_dir = |d: &Path| -> Option<Vec<u8>> {
        let rd = std::fs::read_dir(d).ok()?;
        for e in rd.flatten() {
            let fname = e.file_name().to_string_lossy().to_lowercase();
            if fname == lower && e.path().is_file() {
                return std::fs::read(e.path()).ok();
            }
        }
        None
    };
    if let Some(b) = try_dir(dir) {
        return Some((name, b));
    }
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        if e.path().is_dir()
            && let Some(b) = try_dir(&e.path())
        {
            return Some((name, b));
        }
    }
    None
}

/// The extension of an image file from its first bytes ("png", "jpg"…).
pub fn image_ext(bytes: &[u8]) -> &'static str {
    match image::guess_format(bytes) {
        Ok(image::ImageFormat::Jpeg) => "jpg",
        Ok(image::ImageFormat::Bmp) => "bmp",
        Ok(image::ImageFormat::Tga) => "tga",
        Ok(image::ImageFormat::Gif) => "gif",
        Ok(image::ImageFormat::Tiff) => "tif",
        _ => "png",
    }
}

/// Unique file names for the textures of an export ("bois.jpg", "bois1.jpg"…), by material.
pub struct TexNames {
    used: Vec<String>,
}

impl TexNames {
    pub fn new() -> Self {
        TexNames { used: Vec::new() }
    }

    /// A file name for a texture, with the extension its bytes call for.
    pub fn name_for(&mut self, t: &TextureData) -> String {
        let ext = image_ext(&t.image.0);
        let base = base_name(&t.filename);
        let stem = base.rsplit_once('.').map_or(base.as_str(), |x| x.0).to_string();
        let clean: String =
            stem.chars().map(|c| if c.is_alphanumeric() || "-_".contains(c) { c } else { '_' }).collect();
        let clean = if clean.is_empty() { "texture".to_string() } else { clean };
        let mut name = format!("{clean}.{ext}");
        let mut i = 1;
        while self.used.iter().any(|u| u.eq_ignore_ascii_case(&name)) {
            name = format!("{clean}{i}.{ext}");
            i += 1;
        }
        self.used.push(name.clone());
        name
    }
}

impl Default for TexNames {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_reduced_to_bare_names() {
        assert_eq!(safe_name("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(safe_name("C:\\Users\\x\\bois%20clair.jpg").as_deref(), Some("bois clair.jpg"));
        assert_eq!(safe_name("file:///tmp/a.png").as_deref(), Some("a.png"));
        assert_eq!(safe_name(".."), None);
        assert_eq!(safe_name("maps/"), None);
        // A texture name of a DAE / glTF / 3DS file with '%' before a multibyte character:
        // the old decoder cut the character in two and panicked during the import.
        assert_eq!(safe_name("bois%aé.png").as_deref(), Some("bois%aé.png"));
        assert_eq!(safe_name("%é").as_deref(), Some("%é"));
        assert_eq!(safe_name("a%2").as_deref(), Some("a%2"));
    }

    #[test]
    fn export_names_are_unique() {
        let t = TextureData {
            filename: "../Bois clair.png".into(),
            image: Blob(Arc::new(vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10])),
            px: [1, 1],
            size: [1.0, 1.0],
            avg: [0, 0, 0],
        };
        let mut n = TexNames::new();
        assert_eq!(n.name_for(&t), "Bois_clair.png");
        assert_eq!(n.name_for(&t), "Bois_clair1.png");
    }
}
