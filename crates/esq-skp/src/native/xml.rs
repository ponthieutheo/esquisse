//! A small XML reader for the `material.xml` and `style.xml` files of the archive
//! (`docs/skp-format.md`, §2.4): elements, attributes and text. Tolerant (unknown syntax is
//! skipped) and bounded (depth and element count).

use super::doc::{Material, Texture};
use super::zip::Archive;
use super::{Result, bad};

#[derive(Clone, Debug, Default)]
pub struct Element {
    /// Name without its namespace prefix (`mat:material` → `material`).
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Element>,
    pub text: String,
    /// The content of its CDATA sections, when it has some (the text then holds the
    /// indentation around them too).
    pub cdata: Option<String>,
}

impl Element {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    fn num(&self, k: &str) -> Option<f64> {
        self.attr(k)?.trim().parse().ok()
    }
}

const MAX_ELEMENTS: usize = 1 << 20;
const MAX_DEPTH: usize = 64;

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e <= 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32)
            }
            _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_string()
}

/// Parses a document; returns its root element. Line ends are read as `\n` (XML 1.0, §2.11).
pub fn parse(bytes: &[u8]) -> Result<Element> {
    let text = String::from_utf8_lossy(bytes);
    let text = if text.contains('\r') { text.replace("\r\n", "\n").replace('\r', "\n").into() } else { text };
    let s = text.as_ref();
    let mut stack: Vec<Element> = vec![Element::default()];
    let mut count = 0usize;
    let mut at = 0usize;
    while at < s.len() {
        let Some(lt) = s[at..].find('<').map(|i| at + i) else {
            if let Some(top) = stack.last_mut() {
                top.text.push_str(&unescape(&s[at..]));
            }
            break;
        };
        if lt > at
            && let Some(top) = stack.last_mut()
        {
            top.text.push_str(&unescape(&s[at..lt]));
        }
        let rest = &s[lt..];
        if rest.starts_with("<?") {
            at = lt + rest.find("?>").map_or(rest.len(), |i| i + 2);
        } else if rest.starts_with("<!--") {
            at = lt + rest.find("-->").map_or(rest.len(), |i| i + 3);
        } else if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").unwrap_or(rest.len());
            if let Some(top) = stack.last_mut() {
                let c = &rest[9.min(end)..end];
                top.text.push_str(c);
                top.cdata.get_or_insert_default().push_str(c);
            }
            at = lt + (end + 3).min(rest.len());
        } else if rest.starts_with("<!") {
            at = lt + rest.find('>').map_or(rest.len(), |i| i + 1);
        } else if rest.starts_with("</") {
            at = lt + rest.find('>').map_or(rest.len(), |i| i + 1);
            if stack.len() > 1 {
                let done = stack.pop().unwrap_or_default();
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(done);
                }
            }
        } else {
            let end = rest.find('>').ok_or_else(|| bad("xml: balise non fermée"))?;
            let inner = &rest[1..end];
            let self_closing = inner.ends_with('/');
            let inner = inner.trim_end_matches('/');
            let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
            let mut el = Element { name: local(&inner[..name_end]), ..Default::default() };
            let mut a = &inner[name_end..];
            while let Some(eq) = a.find('=') {
                let key = a[..eq].trim().to_string();
                let after = a[eq + 1..].trim_start();
                let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
                let Some(close) = after[1..].find(q) else { break };
                el.attrs.push((local(&key), unescape(&after[1..1 + close])));
                a = &after[1 + close + 1..];
            }
            count += 1;
            if count > MAX_ELEMENTS || stack.len() > MAX_DEPTH {
                return Err(bad("xml: document trop grand"));
            }
            if self_closing {
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(el);
                }
            } else {
                stack.push(el);
            }
            at = lt + end + 1;
        }
    }
    while stack.len() > 1 {
        let done = stack.pop().unwrap_or_default();
        if let Some(parent) = stack.last_mut() {
            parent.children.push(done);
        }
    }
    let mut root = stack.pop().unwrap_or_default();
    Ok(if root.children.len() == 1 { root.children.remove(0) } else { root })
}

/// An image of the material: `./name` in the material's folder; a path without `./` from the
/// root of the archive (a copy of a material shares the image of the original:
/// `materials/[Wood]/wood.jpg`), else in the folder.
fn image(archive: &Archive, folder: &str, file: &str) -> Option<std::sync::Arc<Vec<u8>>> {
    if !file.starts_with("./")
        && file.contains('/')
        && let Ok(b) = archive.read_name(file)
    {
        return Some(b);
    }
    let name = file.trim_start_matches("./");
    archive.read_name(&format!("{}/{name}", folder.trim_end_matches('/'))).ok()
}

fn texture(archive: &Archive, folder: &str, file: &str, scale: [f64; 2], avg: Option<u32>) -> Option<Texture> {
    let bytes = image(archive, folder, file)?;
    let px = image_size(&bytes).unwrap_or([0, 0]);
    let avg = avg.map(|c| [(c & 0xff) as u8, (c >> 8 & 0xff) as u8, (c >> 16 & 0xff) as u8]).unwrap_or([0; 3]);
    Some(Texture { file_name: file.trim_start_matches("./").to_string(), image: bytes, px, size: scale, avg })
}

/// Width and height of a PNG, JPEG, BMP, GIF or TIFF file, from its header.
pub fn image_size(b: &[u8]) -> Option<[u32; 2]> {
    let be32 = |i: usize| b.get(i..i + 4).map(|x| u32::from_be_bytes([x[0], x[1], x[2], x[3]]));
    let le32 = |i: usize| b.get(i..i + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]));
    let be16 = |i: usize| b.get(i..i + 2).map(|x| u16::from_be_bytes([x[0], x[1]]) as u32);
    let le16 = |i: usize| b.get(i..i + 2).map(|x| u16::from_le_bytes([x[0], x[1]]) as u32);
    if b.starts_with(b"\x89PNG") {
        return Some([be32(16)?, be32(20)?]);
    }
    if b.starts_with(b"GIF8") {
        return Some([le16(6)?, le16(8)?]);
    }
    if b.starts_with(b"BM") {
        return Some([le32(18)?, (le32(22)? as i32).unsigned_abs()]);
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2usize;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let m = b[i + 1];
            if (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
                return Some([be16(i + 7)?, be16(i + 5)?]);
            }
            if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
                i += 2;
                continue;
            }
            i += 2 + be16(i + 2)? as usize;
        }
        return None;
    }
    // TIFF: the width (256) and height (257) tags of the first directory.
    if b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") {
        let le = b[0] == b'I';
        let u16_at = |i: usize| if le { le16(i) } else { be16(i) };
        let u32_at = |i: usize| if le { le32(i) } else { be32(i) };
        let ifd = u32_at(4)? as usize;
        let n = u16_at(ifd)? as usize;
        let (mut w, mut h) = (None, None);
        for k in 0..n.min(4096) {
            let e = ifd + 2 + 12 * k;
            let value = match u16_at(e + 2)? {
                3 => u16_at(e + 8)?,
                4 => u32_at(e + 8)?,
                _ => continue,
            };
            match u16_at(e)? {
                256 => w = Some(value),
                257 => h = Some(value),
                _ => {}
            }
        }
        return Some([w?, h?]);
    }
    // TGA (no signature): a plausible header — colour map 0 or 1, an image type of the
    // format, a pixel depth of the format.
    if b.len() > 18 && b[1] <= 1 && matches!(b[2], 1 | 2 | 3 | 9 | 10 | 11) && matches!(b[16], 8 | 15 | 16 | 24 | 32) {
        return Some([le16(12)?, le16(14)?]);
    }
    None
}

/// The extension of an image's format, from its first bytes (what SketchUp gives a texture
/// whose name has none).
pub fn image_extension(b: &[u8]) -> Option<&'static str> {
    Some(if b.starts_with(b"\x89PNG") {
        "png"
    } else if b.starts_with(&[0xFF, 0xD8]) {
        "jpg"
    } else if b.starts_with(b"BM") {
        "bmp"
    } else if b.starts_with(b"GIF8") {
        "gif"
    } else if b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") {
        "tif"
    } else {
        return None;
    })
}

/// A value of an attribute dictionary written in XML (`<Attribute key="…" type="…">`), as the
/// Ruby bridge's tagged JSON. `type` is SketchUp's variant type: 2 byte, 3
/// short, 4 int, 5 float, 6 and 12–16 double, 7 bool, 8 colour (0xAABBGGRR), 9 time, 10 string,
/// 11 array (`<value>` children), 17 point, 18 vector, 19 unit vector, 20 transformation.
fn xml_value(e: &Element, depth: usize) -> Option<serde_json::Value> {
    use serde_json::json;
    if depth > 32 {
        return None;
    }
    let ty: i64 = e.attr("type")?.trim().parse().ok()?;
    let text = e.text.trim();
    let num = || text.parse::<f64>().ok();
    let xyz = |name: &str| {
        let v = e.child(name)?;
        Some([v.num("x")?, v.num("y")?, v.num("z")?])
    };
    Some(match ty {
        2..=4 => json!({"t": "i", "v": text.parse::<i64>().ok()?}),
        5 => json!({"t": "f", "v": text.parse::<f32>().ok()? as f64}),
        6 | 12..=16 => json!({"t": "f", "v": num()?}),
        7 => json!({"t": "b", "v": text == "1" || text.eq_ignore_ascii_case("true")}),
        8 => {
            let c = text.parse::<u64>().ok()? as u32;
            json!({"t": "c", "v": [c & 0xff, c >> 8 & 0xff, c >> 16 & 0xff, c >> 24]})
        }
        9 => json!({"t": "time", "v": [text.parse::<i64>().ok()?, 0]}),
        // A string in CDATA: its content alone, not the indentation around it.
        10 => json!({"t": "s", "v": e.cdata.as_deref().unwrap_or(&e.text)}),
        11 => {
            json!({"t": "a", "v": e.children.iter().filter(|c| c.name == "value").filter_map(|c| xml_value(c, depth + 1)).collect::<Vec<_>>()})
        }
        17 => json!({"t": "vec", "v": xyz("point")?}),
        18 => json!({"t": "vec", "v": xyz("vector")?}),
        19 => json!({"t": "vec", "v": xyz("unitVector")?}),
        _ => return None,
    })
}

/// The attribute dictionaries of an XML element (`<AttributeDictionaries>`).
fn xml_attributes(el: &Element) -> esq_core::model::Attributes {
    let mut out = esq_core::model::Attributes::new();
    let Some(list) = el.child("AttributeDictionaries") else { return out };
    for d in list.children.iter().filter(|c| c.name == "AttributeDictionary") {
        let name = d.attr("name").unwrap_or_default().to_string();
        let mut values = std::collections::BTreeMap::new();
        for a in d.children.iter().filter(|c| c.name == "Attribute") {
            if let (Some(k), Some(v)) = (a.attr("key"), xml_value(a, 0)) {
                values.insert(k.to_string(), super::vff::attr_value(v));
            }
        }
        if !values.is_empty() {
            out.entry(name).or_default().extend(values);
        }
    }
    out
}

/// Fills a material from its `material.xml`.
pub fn material(bytes: &[u8], folder: &str, archive: &Archive, m: &mut Material) -> Result<()> {
    let doc = parse(bytes)?;
    let Some(el) = (if doc.name == "material" { Some(&doc) } else { doc.child("material") }) else {
        return Ok(());
    };
    if let Some(n) = el.attr("name") {
        m.name = n.to_string();
    }
    m.attrs = xml_attributes(el);
    let byte = |k: &str| el.num(k).unwrap_or(0.0).clamp(0.0, 255.0) as u8;
    m.color = [byte("colorRed"), byte("colorGreen"), byte("colorBlue")];
    m.kind = el.num("type").unwrap_or(0.0) as i32;
    m.workflow = el.num("workflow").unwrap_or(0.0) as i32;
    m.colorize_type = el.num("colorizeType").unwrap_or(0.0) as i32;
    m.use_opacity = el.num("useTrans").unwrap_or(0.0) != 0.0;
    m.opacity = 1.0 - el.num("trans").unwrap_or(0.0);
    if let Some(t) = el.child("texture") {
        let scale = [t.num("xScale").unwrap_or(1.0), t.num("yScale").unwrap_or(1.0)];
        let file = t
            .child("images")
            .and_then(|i| i.child("image"))
            .and_then(|i| i.attr("path").or(i.attr("file_name")))
            .or(t.attr("textureFilename"))
            .unwrap_or_default();
        let avg = t.num("avgColor").map(|c| c as u32);
        m.texture = texture(archive, folder, file, scale, avg);
        if let (Some(tex), Some(name)) = (m.texture.as_mut(), t.attr("textureFilename")) {
            tex.file_name = name.to_string();
        }
    }
    // PBR settings: SketchUp's defaults when the file has none.
    let pbr = &mut m.pbr;
    (pbr.metallic_factor, pbr.roughness_factor, pbr.normal_scale, pbr.normal_style, pbr.ao_strength) =
        (1.0, 1.0, 1.0, 1, 1.0);
    if let Some(p) = el.child("pbrMR") {
        let scale = [p.num("xScale").unwrap_or(1.0), p.num("yScale").unwrap_or(1.0)];
        let flag = |k: &str| p.child(k).is_some_and(|c| c.text.trim() == "1");
        // Factors are single precision in SketchUp.
        let num = |k: &str, d: f64| p.child(k).and_then(|c| c.text.trim().parse::<f32>().ok()).map_or(d, |x| x as f64);
        let tex = |k: &str| {
            p.child(k).and_then(|c| {
                let f = c.attr("path").or(c.attr("file_name"))?;
                let mut t = texture(archive, folder, f, scale, None)?;
                if let Some(n) = c.attr("file_name") {
                    t.file_name = n.to_string();
                }
                Some(t)
            })
        };
        let pbr = &mut m.pbr;
        pbr.metalness_enabled = flag("enable_metalness");
        pbr.roughness_enabled = flag("enable_roughness");
        pbr.roughness_inverted = flag("roughness_texture_invert");
        pbr.normal_enabled = flag("enable_normal");
        pbr.ao_enabled = flag("enable_occlusion");
        pbr.metallic_factor = num("metallicFactor", 1.0);
        pbr.roughness_factor = num("roughnessFactor", 1.0);
        pbr.normal_style = num("normalMapStyle", 1.0) as i32;
        pbr.normal_scale = num("normalScale", 1.0);
        pbr.ao_strength = num("occlusionStrength", 1.0);
        pbr.metallic_texture = tex("metallic_texture");
        pbr.roughness_texture = tex("roughness_texture");
        pbr.normal_texture = tex("normal_texture");
        pbr.ao_texture = tex("occlusion_texture");
    }
    Ok(())
}
