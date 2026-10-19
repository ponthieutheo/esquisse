//! SketchUp hatch pattern files (`.skh`): a zip holding `document.xml` (one
//! `hash:hatchpattern` element with the `HatchPatternSettings` fields as attributes:
//! name, colorRed/Green/Blue, hasTexture, xScale, yScale, rotationAngle, hasPat, patName,
//! patData), `documentProperties.xml`, `doc_thumbnail.png` and `references.xml` — as the
//! shipped `Default Hatching` files are made. An image pattern keeps its picture in the zip.

use std::io::Cursor;

use esq_core::hatch::{parse_pat, parse_pat_data, pat_data, pat_text};
use esq_core::meta::HatchPattern;

// ------------------------------------------------------------------------------- zip

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut c = !0u32;
    for b in data {
        c = table[((c ^ *b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// The files of a zip archive (stored or deflated; other entries are skipped), by name: the
/// shared reader of `esq_core::zip`, within one decompression budget.
pub fn unzip(b: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    let a = esq_core::zip::Archive::open(b).ok()?;
    let budget = esq_core::untrusted::Budget::for_input(b.len());
    let mut out = Vec::new();
    for e in a.entries.iter().filter(|e| matches!(e.method, 0 | 8)) {
        out.push((e.name.clone(), a.read(e, &budget).ok()?));
    }
    Some(out)
}

/// A zip archive of these files (deflated).
pub fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    let w16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let w32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    for (name, data) in files {
        let packed = miniz_oxide::deflate::compress_to_vec(data, 6);
        let crc = crc32(data);
        let off = out.len() as u32;
        w32(&mut out, 0x0403_4b50);
        for x in [20u16, 0, 8, 0, 0] {
            w16(&mut out, x);
        }
        w32(&mut out, crc);
        w32(&mut out, packed.len() as u32);
        w32(&mut out, data.len() as u32);
        w16(&mut out, name.len() as u16);
        w16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&packed);
        w32(&mut central, 0x0201_4b50);
        for x in [20u16, 20, 0, 8, 0, 0] {
            w16(&mut central, x);
        }
        w32(&mut central, crc);
        w32(&mut central, packed.len() as u32);
        w32(&mut central, data.len() as u32);
        w16(&mut central, name.len() as u16);
        for x in [0u16, 0, 0, 0] {
            w16(&mut central, x);
        }
        w32(&mut central, 0);
        w32(&mut central, off);
        central.extend_from_slice(name.as_bytes());
    }
    let cd_off = out.len() as u32;
    let cd_len = central.len() as u32;
    out.extend_from_slice(&central);
    w32(&mut out, 0x0605_4b50);
    for x in [0u16, 0, files.len() as u16, files.len() as u16] {
        w16(&mut out, x);
    }
    w32(&mut out, cd_len);
    w32(&mut out, cd_off);
    w16(&mut out, 0);
    out
}

// ------------------------------------------------------------------------------- xml

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The attributes of the first element named `tag` (`prefix:tag` accepted).
fn attributes(xml: &str, tag: &str) -> Option<Vec<(String, String)>> {
    let start = xml.find(&format!(":{tag} ")).or_else(|| xml.find(&format!("<{tag} ")))?;
    let rest = &xml[start..];
    let end = rest.find('>')?;
    let mut body = &rest[rest.find(' ')?..end];
    let mut out = Vec::new();
    while let Some(eq) = body.find('=') {
        let key = body[..eq].trim().trim_start_matches('/').to_string();
        let after = body[eq + 1..].trim_start();
        // The quote's own length: `name=é…` sliced inside "é" (a panic on opening the file).
        let q = after.chars().next()?;
        let ql = q.len_utf8();
        let close = after[ql..].find(q)? + ql;
        out.push((key, unescape(&after[ql..close])));
        body = &after[close + ql..];
    }
    Some(out)
}

// ------------------------------------------------------------------------------- .skh

/// Reads a .skh file into a pattern (`uid` given by the caller).
pub fn read(bytes: &[u8], uid: u32) -> Result<HatchPattern, String> {
    let files = unzip(bytes).ok_or("Ce fichier n'est pas un modèle de hachures SketchUp.")?;
    let doc = files.iter().find(|(n, _)| n == "document.xml").ok_or("document.xml absent")?;
    let xml = String::from_utf8_lossy(&doc.1);
    let attrs = attributes(&xml, "hatchpattern").ok_or("Ce fichier n'est pas un modèle de hachures SketchUp.")?;
    let get = |k: &str| attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    let num = |k: &str, d: f64| get(k).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(d);
    let name = get("name").unwrap_or("Hachures").to_string();
    let image = num("hasTexture", 0.0) != 0.0;
    let (data, source) = if image {
        let pic = files
            .iter()
            .find(|(n, b)| n != "doc_thumbnail.png" && esq_core::hatch::looks_like_image(b))
            .ok_or("Image du motif absente.")?;
        (pic.1.clone(), get("textureName").unwrap_or(&pic.0).to_string())
    } else {
        let lines = parse_pat_data(get("patData").unwrap_or(""))?;
        (pat_text(&name, &lines).into_bytes(), get("patName").unwrap_or("").to_string())
    };
    let mut h = HatchPattern::new(uid, &name, &source, data);
    h.image = image;
    h.color =
        [num("colorRed", 255.0), num("colorGreen", 255.0), num("colorBlue", 255.0)].map(|c| c.clamp(0.0, 255.0) as u8);
    h.size = [num("xScale", 1.0), num("yScale", 1.0)];
    h.rotation = num("rotationAngle", 0.0);
    Ok(h)
}

/// Writes a pattern as a .skh file.
pub fn write(h: &HatchPattern, thumbnail_png: &[u8]) -> Result<Vec<u8>, String> {
    let (pat_name, data_attr, texture) = if h.image {
        (String::new(), String::new(), Some(esq_core::meta::base_name(&h.source)))
    } else {
        let (_, lines) = parse_pat(&String::from_utf8_lossy(&h.data.0))?;
        (h.source.clone(), pat_data(&lines), None)
    };
    let f = |x: f64| {
        let s = format!("{x}");
        s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
    };
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<hatch_patternDocument xmlns=\"http://sketchup.com/schemas/sketchup/1.0/hatchpattern\" xmlns:hash=\"http://sketchup.com/schemas/sketchup/1.0/hatchpattern\" xmlns:r=\"http://sketchup.google.com/schemas/1.0/references\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"http://sketchup.com/schemas/sketchup/1.0/hatchpattern http://sketchup.com/schemas/sketchup/1.0/hatchpattern.xsd\">\n  <hash:hatchpattern name=\"{}\" colorRed=\"{}\" colorGreen=\"{}\" colorBlue=\"{}\" hasTexture=\"{}\"{} xScale=\"{}\" yScale=\"{}\" rotationAngle=\"{}\" hasPat=\"{}\" patName=\"{}\" patData=\"{}\"/>\n</hatch_patternDocument>\n",
        escape(&h.name),
        h.color[0],
        h.color[1],
        h.color[2],
        h.image as u8,
        texture.as_ref().map_or(String::new(), |t| format!(" textureName=\"{}\"", escape(t))),
        f(h.size[0]),
        f(h.size[1]),
        f(h.rotation),
        (!h.image) as u8,
        escape(&pat_name),
        data_attr,
    );
    let props = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<documentProperties xmlns=\"http://sketchup.google.com/schemas/1.0/documentproperties\" xmlns:dp=\"http://sketchup.google.com/schemas/1.0/documentproperties\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"http://sketchup.google.com/schemas/1.0/documentproperties http://sketchup.google.com/schemas/1.0/documentproperties.xsd\">\n  <dp:title>{}</dp:title>\n  <dp:description></dp:description>\n  <dp:creator></dp:creator>\n  <dp:keywords></dp:keywords>\n  <dp:lastModifiedBy></dp:lastModifiedBy>\n  <dp:revision>0</dp:revision>\n  <dp:created>2026-01-01T00:00:00Z</dp:created>\n  <dp:modified>2026-01-01T00:00:00Z</dp:modified>\n  <dp:thumbnail>doc_thumbnail.png</dp:thumbnail>\n  <dp:generator dp:name=\"HatchPattern\" dp:version=\"1\"/>\n</documentProperties>\n",
        escape(&h.name)
    );
    let refs = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<references xmlns=\"http://sketchup.google.com/schemas/1.0/references\" xmlns:r=\"http://sketchup.google.com/schemas/1.0/references\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"http://sketchup.google.com/schemas/1.0/references http://sketchup.google.com/schemas/1.0/references.xsd\"/>\n";
    let tex_name = texture.unwrap_or_default();
    let mut files: Vec<(&str, &[u8])> = vec![
        ("document.xml", document.as_bytes()),
        ("documentProperties.xml", props.as_bytes()),
        ("doc_thumbnail.png", thumbnail_png),
        ("references.xml", refs.as_bytes()),
    ];
    if h.image {
        files.push((tex_name.as_str(), &h.data.0[..]));
    }
    Ok(zip(&files))
}

/// A PNG of RGBA pixels.
pub fn png(rgba: Vec<u8>, side: u32) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(img) = image::RgbaImage::from_raw(side, side, rgba) {
        let _ = img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_round_trip() {
        let files = [("a.txt", &b"bonjour bonjour bonjour"[..]), ("b/c.bin", &[0u8, 1, 2, 3][..])];
        let z = zip(&files);
        let back = unzip(&z).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0], ("a.txt".to_string(), b"bonjour bonjour bonjour".to_vec()));
        assert_eq!(back[1].1, vec![0, 1, 2, 3]);
    }

    /// A `.skh` whose attribute is quoted by "é": the reader sliced inside it and opening
    /// the pattern panicked.
    #[test]
    fn a_non_ascii_quote_does_not_panic() {
        assert_eq!(attributes("<hash:hatchpattern name=éCroixé/>", "hatchpattern").unwrap()[0].1, "Croix");
        assert!(attributes("<hash:hatchpattern name=é/>", "hatchpattern").is_none());
        let z = zip(&[("document.xml", "<hash:hatchpattern name=éxé hasTexture=é0é/>".as_bytes())]);
        let _ = read(&z, 1);
    }

    #[test]
    fn skh_round_trip_and_sketchup_files() {
        let mut h =
            HatchPattern::new(7, "Croix & co", "C:/x/CROSS.pat", b"*CROSS\n0, 0,0, .25,.25, .125,-.375\n".to_vec());
        h.color = [200, 10, 30];
        h.size = [14.0, 7.5];
        h.rotation = 30.0;
        let bytes = write(&h, &png(vec![255; 4 * 4], 2)).unwrap();
        let back = read(&bytes, 7).unwrap();
        assert_eq!(
            (back.name.as_str(), back.color, back.size, back.rotation),
            ("Croix & co", [200, 10, 30], [14.0, 7.5], 30.0)
        );
        assert_eq!(back.source, "CROSS.pat");
        assert!(!back.image);
        assert!(String::from_utf8_lossy(&back.data.0).contains("0, 0, 0, 0.25, 0.25, 0.125, -0.375"));
        // A file shipped with SketchUp, when it is installed (read only).
        let shipped = r"C:\ProgramData\SketchUp\SketchUp 2026\SketchUp\HatchPatterns\Default Hatching\ANSI31.skh";
        if let Ok(b) = std::fs::read(shipped) {
            let p = read(&b, 1).unwrap();
            assert_eq!((p.name.as_str(), p.size, p.color), ("ANSI31", [14.0, 14.0], [255, 255, 255]));
            assert!(String::from_utf8_lossy(&p.data.0).contains("45, 0, 0, 0, 0.125"));
        }
    }
}
