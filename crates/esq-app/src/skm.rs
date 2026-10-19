//! SketchUp's material files (`.skm`, File › Save As… in Window › Materials): a ZIP holding
//! `document.xml`, `documentProperties.xml`, `references.xml`, the images under `ref/` and
//! a thumbnail. `document.xml` has one `<mat:material>`: name, type (0 colour, 1 texture,
//! 2 colorized texture), colour, colorize mode, opacity (`trans`, used when `useTrans`), the
//! texture (its size in inches, its average colour) and the metallic-roughness settings
//! (`<mat:pbrMR>`: what is enabled, the factors, the maps).
//!
//! The libraries of Window › Materials are folders of these files: one sub-folder per
//! collection.

use std::path::{Path, PathBuf};

use esq_core::meta::TextureData;
use esq_core::model::Material;

use crate::style_file::{Node, parse_xml};

const INCH: f64 = 25.4;

/// A colour item (signed integer, red in the low byte, alpha in the high one).
fn color_text(c: [u8; 3]) -> String {
    (0xff00_0000u32 | (c[2] as u32) << 16 | (c[1] as u32) << 8 | c[0] as u32).to_string()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn num(n: &Node, k: &str) -> Option<f64> {
    n.attr(k).and_then(|v| v.trim().parse().ok())
}

fn child_num(n: &Node, k: &str) -> Option<f64> {
    n.child(k).and_then(|c| c.text.trim().parse().ok())
}

fn child_flag(n: &Node, k: &str) -> Option<bool> {
    n.child(k).map(|c| c.text.trim() == "1")
}

/// A texture of the file: `ref/<path>`, `size` its tile in millimetres.
fn texture(
    files: &std::collections::BTreeMap<String, Vec<u8>>,
    image: Option<&Node>,
    size: [f64; 2],
) -> Option<TextureData> {
    let image = image?;
    let path = image.attr("path")?;
    let bytes = files.get(&format!("ref/{path}")).or_else(|| files.get(path))?.clone();
    let name = image.attr("file_name").unwrap_or(path);
    let mut t = crate::mat_panel::texture_from_bytes(name, bytes).ok()?;
    if size[0] > 0.0 && size[1] > 0.0 {
        t.size = size;
    }
    Some(t)
}

/// Reads a `.skm` file.
pub fn read(bytes: &[u8]) -> Result<Material, String> {
    let files = crate::zip::read(bytes)?;
    let doc = files.get("document.xml").ok_or("pas de document.xml : ce n'est pas une matière")?;
    let root = parse_xml(&String::from_utf8_lossy(doc))?;
    let mut found = Vec::new();
    root.find_all("material", &mut found);
    let mat = found.first().ok_or("pas de matière dans le document")?;
    let rgb = |k: &str| num(mat, k).unwrap_or(128.0).clamp(0.0, 255.0) as u8;
    let color = [rgb("colorRed"), rgb("colorGreen"), rgb("colorBlue")];
    let use_trans = mat.attr("useTrans").is_some_and(|v| v.trim() == "1");
    let opacity = if use_trans { num(mat, "trans").unwrap_or(1.0).clamp(0.0, 1.0) as f32 } else { 1.0 };
    let mut m =
        Material { name: mat.attr("name").unwrap_or_default().to_string(), color, opacity, ext: Default::default() };
    m.ext.colorize_type = num(mat, "colorizeType").unwrap_or(0.0) as u8;
    m.ext.workflow = num(mat, "workflow").unwrap_or(0.0) as u8;
    let kind = num(mat, "type").unwrap_or(0.0) as u8;
    if let Some(tex) = mat.child("texture") {
        let size = [num(tex, "xScale").unwrap_or(0.0) * INCH, num(tex, "yScale").unwrap_or(0.0) * INCH];
        let mut imgs = Vec::new();
        tex.find_all("image", &mut imgs);
        if let Some(t) = texture(&files, imgs.first().copied(), size) {
            m.ext.texture = Some(t);
            // A colorized texture keeps its colour (the shifts from the image's average);
            // a plain one shows the image as it is.
            if kind == 2 {
                crate::mat_panel::set_color(&mut m, color);
            }
        }
    }
    if let Some(p) = mat.child("pbrMR") {
        let size = [num(p, "xScale").unwrap_or(0.0) * INCH, num(p, "yScale").unwrap_or(0.0) * INCH];
        let pbr = &mut m.ext.pbr;
        pbr.metalness_enabled = child_flag(p, "enable_metalness").unwrap_or(false);
        pbr.roughness_enabled = child_flag(p, "enable_roughness").unwrap_or(false);
        pbr.normal_enabled = child_flag(p, "enable_normal").unwrap_or(false);
        pbr.ao_enabled = child_flag(p, "enable_occlusion").unwrap_or(false);
        pbr.metallic_factor = child_num(p, "metallicFactor").unwrap_or(pbr.metallic_factor);
        pbr.roughness_factor = child_num(p, "roughnessFactor").unwrap_or(pbr.roughness_factor);
        pbr.normal_style = child_num(p, "normalMapStyle").unwrap_or(0.0) as u8;
        pbr.normal_scale = child_num(p, "normalScale").unwrap_or(pbr.normal_scale);
        pbr.ao_strength = child_num(p, "occlusionStrength").unwrap_or(pbr.ao_strength);
        pbr.metallic_texture = texture(&files, p.child("metallic_texture"), size);
        pbr.roughness_texture = texture(&files, p.child("roughness_texture"), size);
        pbr.normal_texture = texture(&files, p.child("normal_texture"), size);
        pbr.ao_texture = texture(&files, p.child("occlusion_texture"), size);
    }
    Ok(m)
}

/// Reads a `.skm` file from disk; a nameless material takes the file's name.
pub fn read_file(path: &Path) -> Result<Material, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut m = read(&bytes)?;
    if m.name.trim().is_empty() {
        m.name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    }
    Ok(m)
}

/// A `.skm` file of this material (`thumbnail`: a PNG shown by material browsers): the library
/// generator (`content_gen`), Window › Materials › Save As and Ruby's `Material#save_as`.
pub fn write(m: &Material, thumbnail: Option<&[u8]>) -> Vec<u8> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut next = 1;
    let mut image_ref = |t: &TextureData, tag: &str, files: &mut Vec<(String, Vec<u8>)>| {
        let id = next;
        next += 1;
        let base = if t.filename.is_empty() { format!("image{id}.png") } else { t.filename.clone() };
        let (stem, ext) = base.rsplit_once('.').unwrap_or((&base, "png"));
        let path = format!("{stem}_{id}.{ext}");
        files.push((format!("ref/{path}"), t.image.0.to_vec()));
        format!("<mat:{tag} id=\"{id}\" path=\"{}\" file_name=\"{}\"/>", escape(&path), escape(&base))
    };
    let t = m.ext.texture.as_ref();
    let colorized = t.is_some() && (m.ext.colorize_type != 0 || m.ext.colorize_deltas.iter().any(|d| d.abs() > 1e-9));
    let kind = match (t, colorized) {
        (None, _) => 0,
        (Some(_), false) => 1,
        (Some(_), true) => 2,
    };
    let mut body = String::new();
    let scale = |s: [f64; 2]| (s[0] / INCH, s[1] / INCH);
    let mut tile = (1.0, 1.0);
    if let Some(t) = t {
        tile = scale(t.size);
        let img = image_ref(t, "image", &mut files);
        body.push_str(&format!(
            "    <mat:texture textureFilename=\"{}\" xScale=\"{}\" yScale=\"{}\" avgColor=\"{}\">\n      <mat:images>\n        {img}\n      </mat:images>\n    </mat:texture>\n",
            escape(&t.filename),
            tile.0,
            tile.1,
            color_text(t.avg)
        ));
    }
    let p = &m.ext.pbr;
    let flag = |b: bool| if b { 1 } else { 0 };
    let mut maps = String::new();
    for (tex, tag) in [
        (&p.metallic_texture, "metallic_texture"),
        (&p.roughness_texture, "roughness_texture"),
        (&p.normal_texture, "normal_texture"),
        (&p.ao_texture, "occlusion_texture"),
    ] {
        if let Some(x) = tex {
            maps.push_str(&format!("      {}\n", image_ref(x, tag, &mut files)));
        }
    }
    body.push_str(&format!(
        "    <mat:pbrMR xScale=\"{}\" yScale=\"{}\">\n      <mat:enable_metalness>{}</mat:enable_metalness>\n      <mat:enable_roughness>{}</mat:enable_roughness>\n      <mat:enable_normal>{}</mat:enable_normal>\n      <mat:enable_occlusion>{}</mat:enable_occlusion>\n{maps}      <mat:metallicFactor>{}</mat:metallicFactor>\n      <mat:roughnessFactor>{}</mat:roughnessFactor>\n      <mat:normalMapStyle>{}</mat:normalMapStyle>\n      <mat:normalScale>{}</mat:normalScale>\n      <mat:occlusionStrength>{}</mat:occlusionStrength>\n      <mat:baseColorFactor>4294967295</mat:baseColorFactor>\n    </mat:pbrMR>\n",
        tile.0,
        tile.1,
        flag(p.metalness_enabled),
        flag(p.roughness_enabled),
        flag(p.normal_enabled),
        flag(p.ao_enabled),
        p.metallic_factor,
        p.roughness_factor,
        p.normal_style,
        p.normal_scale,
        p.ao_strength
    ));
    let doc = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<materialDocument xmlns=\"http://sketchup.google.com/schemas/sketchup/1.0/material\" xmlns:mat=\"http://sketchup.google.com/schemas/sketchup/1.0/material\" xmlns:r=\"http://sketchup.google.com/schemas/1.0/references\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n  <mat:material name=\"{}\" type=\"{kind}\" workflow=\"{}\" colorRed=\"{}\" colorGreen=\"{}\" colorBlue=\"{}\" colorizeType=\"{}\" trans=\"{}\" useTrans=\"{}\" hasTexture=\"{}\">\n{body}  </mat:material>\n</materialDocument>\n",
        escape(&m.name),
        m.ext.workflow,
        m.color[0],
        m.color[1],
        m.color[2],
        m.ext.colorize_type,
        m.opacity,
        flag(m.opacity < 1.0),
        flag(t.is_some()),
    );
    let props = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<documentProperties xmlns=\"http://sketchup.google.com/schemas/1.0/documentproperties\" xmlns:dp=\"http://sketchup.google.com/schemas/1.0/documentproperties\">\n  <dp:title>{}</dp:title>\n  <dp:description></dp:description>\n  <dp:creator></dp:creator>\n  <dp:keywords></dp:keywords>\n  <dp:lastModifiedBy></dp:lastModifiedBy>\n  <dp:revision>0</dp:revision>\n  <dp:created>2026-01-01T00:00:00Z</dp:created>\n  <dp:modified>2026-01-01T00:00:00Z</dp:modified>\n  <dp:thumbnail>doc_thumbnail.png</dp:thumbnail>\n  <dp:generator dp:name=\"Material\" dp:version=\"1\" />\n</documentProperties>\n",
        escape(&m.name)
    );
    let refs = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<references xmlns=\"http://sketchup.google.com/schemas/1.0/references\" />\n";
    let mut all = vec![
        ("document.xml".to_string(), doc.into_bytes()),
        ("documentProperties.xml".to_string(), props.into_bytes()),
        ("references.xml".to_string(), refs.as_bytes().to_vec()),
    ];
    if let Some(png) = thumbnail {
        all.push(("doc_thumbnail.png".to_string(), png.to_vec()));
    }
    all.extend(files);
    crate::zip::write(&all)
}

/// The materials of the `.skm` files of a folder, sorted by file name (unreadable files are
/// left out).
pub fn read_folder(dir: &Path) -> Vec<Material> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("skm")))
                .collect()
        })
        .unwrap_or_default();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    files.into_iter().filter_map(|p| read_file(&p).ok()).collect()
}

/// The library that comes with Esquisse (`Materials` of its content).
pub fn library() -> Option<PathBuf> {
    esq_core::content::folder("Materials")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materials_round_trip_through_skm_files() {
        let mut m =
            Material { name: "Brique & co".into(), color: [150, 80, 60], opacity: 0.75, ext: Default::default() };
        let mut t =
            crate::mat_panel::texture_from_bytes("brique.png", crate::photo_img::tests::png_bytes(8, 4)).unwrap();
        t.size = [500.0, 250.0];
        m.ext.texture = Some(t);
        m.ext.workflow = 1;
        m.ext.pbr.roughness_enabled = true;
        m.ext.pbr.roughness_factor = 0.8;
        m.ext.pbr.normal_enabled = true;
        m.ext.pbr.normal_texture =
            Some(crate::mat_panel::texture_from_bytes("n.png", crate::photo_img::tests::png_bytes(4, 4)).unwrap());
        let back = read(&write(&m, None)).unwrap();
        assert_eq!((back.name.as_str(), back.opacity), ("Brique & co", 0.75));
        let bt = back.ext.texture.as_ref().unwrap();
        assert!((bt.size[0] - 500.0).abs() < 1e-6 && (bt.size[1] - 250.0).abs() < 1e-6);
        assert_eq!((bt.px, bt.filename.as_str()), ([8, 4], "brique.png"));
        assert_eq!(back.ext.workflow, 1);
        assert!(back.ext.pbr.roughness_enabled && (back.ext.pbr.roughness_factor - 0.8).abs() < 1e-9);
        assert!(back.ext.pbr.normal_enabled && back.ext.pbr.normal_texture.is_some());
        // A plain colour.
        let c = Material { name: "Bleu".into(), color: [10, 20, 200], opacity: 1.0, ext: Default::default() };
        let back = read(&write(&c, None)).unwrap();
        assert_eq!((back.color, back.opacity, back.ext.texture.is_none()), ([10, 20, 200], 1.0, true));
    }

    /// SketchUp's own material files, when it is installed (read only, nothing is copied).
    #[test]
    fn sketchup_material_files_are_read() {
        let dir = Path::new(r"C:\ProgramData\SketchUp\SketchUp 2026\SketchUp\Materials");
        let Ok(m) = read_file(&dir.join("Brick").join("Brick_01_1K.skm")) else {
            return eprintln!("SketchUp absent : test sauté");
        };
        let t = m.ext.texture.as_ref().expect("texture");
        assert!((t.size[0] - 2500.0).abs() < 0.01 && (t.size[1] - 1250.0).abs() < 0.01, "{:?}", t.size);
        assert_eq!(m.ext.workflow, 1);
        let p = &m.ext.pbr;
        assert!(p.roughness_enabled && p.normal_enabled && p.ao_enabled && !p.metalness_enabled);
        assert!(p.roughness_texture.is_some() && p.normal_texture.is_some() && p.ao_texture.is_some());
        assert!((p.normal_scale - 0.26).abs() < 1e-6);
        let c = read_file(&dir.join("Solid Colors").join("A01_Blush_Petal.skm")).unwrap();
        assert_eq!((c.color, c.ext.texture.is_none()), ([255, 204, 204], true));
        assert!(c.ext.pbr.metalness_enabled && (c.ext.pbr.metallic_factor - 0.1).abs() < 1e-6);
    }
}
