//! SketchUp's style files (`.style`): a ZIP archive holding `document.xml`, where each
//! setting is an item numbered by category (1000 edges, 2000 faces, 4000 background,
//! 5000 watermarks, 7000 modeling, 8000 photo matching, 8100 ambient occlusion), the
//! watermark images under `ref/`, and a thumbnail. The numbers and their rendering options
//! were read in SketchUp.exe (the function that turns rendering options into style items).

use serde_json::{Map, Value as J, json};

use crate::style::{Watermark, WmLayout};

/// How an item's value is written.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Bool,
    Int,
    Color,
    Double,
    Float,
}

/// Style item number → rendering option.
const ITEMS: &[(u32, &str, Kind)] = &[
    (1000, "EdgeDisplayMode", Kind::Int),
    (1001, "EdgeType", Kind::Int),
    (1002, "EdgeColorMode", Kind::Int),
    (1004, "ExtendLines", Kind::Bool),
    (1005, "LineExtension", Kind::Int),
    (1006, "DrawSilhouettes", Kind::Bool),
    (1007, "SilhouetteWidth", Kind::Int),
    (1008, "DrawDepthQue", Kind::Bool),
    (1009, "DepthQueWidth", Kind::Int),
    (1010, "DrawLineEnds", Kind::Bool),
    (1011, "LineEndWidth", Kind::Int),
    (1012, "JitterEdges", Kind::Bool),
    (1014, "ForegroundColor", Kind::Color),
    (1015, "DrawBackEdges", Kind::Bool),
    (1016, "LineStyleEdges", Kind::Bool),
    (2001, "RenderMode", Kind::Int),
    (2002, "FaceFrontColor", Kind::Color),
    (2003, "FaceBackColor", Kind::Color),
    (2004, "ModelTransparency", Kind::Bool),
    (2005, "MaterialTransparency", Kind::Bool),
    (2006, "TransparencySort", Kind::Int),
    (2007, "Texture", Kind::Bool),
    (2008, "XRayOpacity", Kind::Double),
    (4000, "BackgroundColor", Kind::Color),
    (4001, "SkyColor", Kind::Color),
    (4002, "DrawHorizon", Kind::Bool),
    (4003, "GroundColor", Kind::Color),
    (4004, "DrawGround", Kind::Bool),
    (4005, "GroundTransparency", Kind::Int),
    (4006, "DrawUnderground", Kind::Bool),
    (5000, "DisplayWatermarks", Kind::Bool),
    (7000, "HighlightColor", Kind::Color),
    (7001, "LockedColor", Kind::Color),
    (7002, "ConstructionColor", Kind::Color),
    (7003, "SectionActiveColor", Kind::Color),
    (7004, "SectionInactiveColor", Kind::Color),
    (7005, "SectionDefaultCutColor", Kind::Color),
    (7008, "DisplaySketchAxes", Kind::Bool),
    (7011, "DisplayColorByLayer", Kind::Bool),
    (7012, "HideConstructionGeometry", Kind::Bool),
    (7014, "SectionCutWidth", Kind::Int),
    (7015, "SectionCutFilled", Kind::Bool),
    (7016, "SectionDefaultFillColor", Kind::Color),
    (7017, "DrawHiddenGeometry", Kind::Bool),
    (7018, "DrawHiddenObjects", Kind::Bool),
    (8000, "PhotomatchDrawBackground", Kind::Bool),
    (8001, "PhotomatchBackgroundOpacity", Kind::Double),
    (8002, "PhotomatchDrawOverlay", Kind::Bool),
    (8003, "PhotomatchOverlayOpacity", Kind::Double),
    (8100, "AmbientOcclusion", Kind::Bool),
    (8102, "AmbientOcclusionDistance", Kind::Float),
    (8103, "AmbientOcclusionIntensity", Kind::Float),
    (8105, "AmbientOcclusionColorEnabled", Kind::Bool),
    (8106, "AmbientOcclusionColor", Kind::Color),
    (8107, "AmbientOcclusionMultiplier", Kind::Float),
];
/// Section planes (bit 0) and cuts (bit 1) shown.
const SECTION_MODE: u32 = 7013;
const WATERMARKS: u32 = 5001;
/// Sketchy edges: strokes drawn from images (not drawn by Esquisse).
const SKETCHY: u32 = 1013;

/// The rendering options of each part of a style (Styles › Mix), by item number range.
pub fn category_of(key: &str) -> Option<usize> {
    if key == "DisplaySectionPlanes" || key == "DisplaySectionCuts" || key == "DrawHidden" {
        return Some(4);
    }
    let id = ITEMS.iter().find(|(_, k, _)| *k == key)?.0;
    Some(match id {
        1000..2000 => 0,
        2000..4000 | 8100.. => 1,
        4000..5000 => 2,
        5000..7000 => 3,
        _ => 4,
    })
}

/// A style read from, or written to, a `.style` file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleFile {
    pub name: String,
    pub description: String,
    pub ro: Map<String, J>,
    pub watermarks: Vec<Watermark>,
    /// The PNG thumbnail the file shows in the Styles panel.
    pub thumbnail: Option<Vec<u8>>,
    /// The style draws its edges with sketchy strokes (Esquisse draws plain edges).
    pub sketchy: bool,
}

// ------------------------------------------------------------------------------ XML

#[derive(Debug, Default)]
pub(crate) struct Node {
    pub(crate) name: String,
    pub(crate) attrs: Vec<(String, String)>,
    pub(crate) children: Vec<Node>,
    pub(crate) text: String,
}

impl Node {
    pub(crate) fn local(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }

    pub(crate) fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }

    pub(crate) fn child(&self, local: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.local() == local)
    }

    pub(crate) fn find_all<'a>(&'a self, local: &str, out: &mut Vec<&'a Node>) {
        for c in &self.children {
            if c.local() == local {
                out.push(c);
            }
            c.find_all(local, out);
        }
    }
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';') else { break };
        let ent = &rest[1..end];
        let c = match ent {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match c {
            Some(c) => out.push(c),
            None => out.push_str(&rest[..=end]),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\n', "&#x0A;")
}

/// The deepest nesting [`parse_xml`] accepts.
const MAX_DEPTH: usize = 256;

/// A small XML reader: elements, attributes and text (enough for style documents).
pub(crate) fn parse_xml(s: &str) -> Result<Node, String> {
    let mut stack = vec![Node { name: "#root".into(), ..Default::default() }];
    let mut rest = s;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            stack.last_mut().unwrap().text.push_str(&unescape(rest));
            break;
        };
        if lt > 0 {
            stack.last_mut().unwrap().text.push_str(&unescape(&rest[..lt]));
        }
        rest = &rest[lt..];
        if rest.starts_with("<?") || rest.starts_with("<!") {
            let end =
                if rest.starts_with("<!--") { rest.find("-->").map(|e| e + 3) } else { rest.find('>').map(|e| e + 1) };
            rest = &rest[end.ok_or("XML tronqué")?..];
            continue;
        }
        let end = rest.find('>').ok_or("XML tronqué")?;
        let tag = &rest[1..end];
        rest = &rest[end + 1..];
        if let Some(name) = tag.strip_prefix('/') {
            let node = stack.pop().ok_or("XML mal formé")?;
            if node.name != name.trim() || stack.is_empty() {
                return Err(format!("balise fermante inattendue : {name}"));
            }
            stack.last_mut().unwrap().children.push(node);
            continue;
        }
        let empty = tag.ends_with('/');
        let tag = tag.trim_end_matches('/');
        let (name, mut attrs_s) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        let mut node = Node { name: name.to_string(), ..Default::default() };
        loop {
            attrs_s = attrs_s.trim_start();
            let Some(eq) = attrs_s.find('=') else { break };
            let key = attrs_s[..eq].trim().to_string();
            let after = attrs_s[eq + 1..].trim_start();
            // The quote's own length: `b=é…` sliced inside "é" (a panic on opening the file).
            let q = after.chars().next().ok_or("XML tronqué")?;
            let ql = q.len_utf8();
            let close = after[ql..].find(q).ok_or("XML tronqué")? + ql;
            node.attrs.push((key, unescape(&after[ql..close])));
            attrs_s = &after[close + ql..];
        }
        if empty {
            stack.last_mut().unwrap().children.push(node);
        } else {
            // Nodes are searched and dropped recursively: a document nested 100 000 deep
            // overflowed the stack. Style documents are a few levels deep.
            if stack.len() > MAX_DEPTH {
                return Err("XML trop imbriqué".into());
            }
            stack.push(node);
        }
    }
    if stack.len() != 1 {
        return Err("XML incomplet".into());
    }
    Ok(stack.pop().unwrap())
}

// ------------------------------------------------------------------------------ names

/// The `"key" = "value";` pairs of a `.strings` file.
pub fn parse_strings(text: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let quoted = |s: &str| -> Option<(String, usize)> {
        let s2 = s.strip_prefix('"')?;
        let mut v = String::new();
        let mut it = s2.char_indices();
        while let Some((i, c)) = it.next() {
            match c {
                '\\' => match it.next()?.1 {
                    'n' => v.push('\n'),
                    't' => v.push('\t'),
                    o => v.push(o),
                },
                '"' => return Some((v, i + 2)),
                _ => v.push(c),
            }
        }
        None
    };
    for line in text.lines() {
        let l = line.trim();
        let Some((k, used)) = quoted(l) else { continue };
        let rest = l[used..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        if let Some((v, _)) = quoted(rest.trim_start()) {
            out.insert(k, v);
        }
    }
    out
}

/// The French name of a style collection (a folder of a style library, named in English as
/// SketchUp names its own), from Esquisse's translations.
pub fn collection_name(folder: &str) -> String {
    crate::i18n::fr_or_same(folder)
}

/// A name or description as SketchUp shows it: `[English]` texts are looked up in
/// Esquisse's translations (and lose their brackets otherwise).
pub fn localized(s: &str) -> String {
    let t = s.trim();
    match t.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
        Some(key) => crate::i18n::fr_or_same(key),
        None => t.to_string(),
    }
}

// ------------------------------------------------------------------------------ reading

/// A colour item (signed integer, red in the low byte) as `[r, g, b, 255]`.
fn color_value(text: &str) -> Option<J> {
    let v = text.trim().parse::<i64>().ok()? as u32;
    Some(json!([v & 255, v >> 8 & 255, v >> 16 & 255, 255]))
}

fn item_value(kind: Kind, text: &str) -> Option<J> {
    let t = text.trim();
    Some(match kind {
        Kind::Bool => json!(t != "0" && !t.eq_ignore_ascii_case("false")),
        Kind::Int => json!(t.parse::<f64>().ok()?.round() as i64),
        Kind::Double | Kind::Float => json!(t.parse::<f64>().ok()?),
        Kind::Color => color_value(t)?,
    })
}

/// Reads a `.style` file.
pub fn read(bytes: &[u8]) -> Result<StyleFile, String> {
    let files = crate::zip::read(bytes)?;
    let doc = files.get("document.xml").ok_or("pas de document.xml : ce n'est pas un style")?;
    let root = parse_xml(&String::from_utf8_lossy(doc))?;
    let mut styles = Vec::new();
    root.find_all("style", &mut styles);
    let style = styles.first().ok_or("pas de style dans le document")?;
    let mut out = StyleFile {
        name: localized(style.attr("name").unwrap_or("")),
        description: localized(style.attr("desc").unwrap_or("").trim_end()),
        thumbnail: files.get("doc_thumbnail.png").cloned(),
        ..Default::default()
    };
    for item in style.children.iter().filter(|c| c.local() == "item") {
        let Some(id) = item.attr("id").and_then(|i| i.parse::<u32>().ok()) else { continue };
        let Some(var) = item.child("variant") else { continue };
        if let Some((_, key, kind)) = ITEMS.iter().find(|(i, _, _)| *i == id) {
            if let Some(v) = item_value(*kind, &var.text) {
                out.ro.insert(key.to_string(), v);
            }
            continue;
        }
        match id {
            SECTION_MODE => {
                let m = var.text.trim().parse::<i64>().unwrap_or(2);
                out.ro.insert("DisplaySectionPlanes".into(), json!(m & 1 != 0));
                out.ro.insert("DisplaySectionCuts".into(), json!(m & 2 != 0));
            }
            SKETCHY => out.sketchy = true,
            WATERMARKS => out.watermarks = read_watermarks(var, &files),
            _ => {}
        }
    }
    if out.name.is_empty()
        && let Some(p) = files.get("documentProperties.xml")
        && let Ok(props) = parse_xml(&String::from_utf8_lossy(p))
    {
        let mut t = Vec::new();
        props.find_all("title", &mut t);
        out.name = t.first().map(|n| n.text.trim().to_string()).unwrap_or_default();
    }
    Ok(out)
}

fn read_watermarks(var: &Node, files: &std::collections::BTreeMap<String, Vec<u8>>) -> Vec<Watermark> {
    let mut list = Vec::new();
    var.find_all("screenimage", &mut list);
    let flag = |n: &Node, k: &str| n.attr(k).is_some_and(|v| v != "0");
    let num = |n: &Node, k: &str, d: f64| n.attr(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(d);
    list.into_iter()
        .filter_map(|n| {
            let mut imgs = Vec::new();
            n.find_all("image", &mut imgs);
            let path = imgs.first()?.attr("path")?;
            let data = files.get(&format!("ref/{path}")).or_else(|| files.get(path))?;
            let layout = if flag(n, "tiled") {
                WmLayout::Tiled
            } else if flag(n, "stretched") {
                WmLayout::Stretched
            } else {
                WmLayout::Positioned
            };
            Some(Watermark {
                name: n.attr("name").unwrap_or("").to_string(),
                image: esq_core::photo::Blob(std::sync::Arc::new(data.clone())),
                background: flag(n, "background"),
                blend: num(n, "alphaScale", 1.0).clamp(0.0, 1.0),
                mask: flag(n, "intensForAlpha"),
                layout,
                keep_aspect: flag(n, "maintainAR"),
                scale: num(n, "scale", 1.0),
                position: num(n, "position", 4.0).clamp(0.0, 8.0) as u8,
                skp: Default::default(),
            })
        })
        .collect()
}

// ------------------------------------------------------------------------------ writing

fn color_text(v: &J) -> Option<String> {
    let c = crate::style::color_of(v)?;
    let n = (0xff00_0000u32 | (c[2] as u32) << 16 | (c[1] as u32) << 8 | c[0] as u32) as i32;
    Some(n.to_string())
}

/// A `.style` file of these settings.
pub fn write(style: &StyleFile) -> Vec<u8> {
    let mut items = String::new();
    let mut push = |id: u32, ty: u32, text: &str| {
        items.push_str(&format!(
            "    <sty:item id=\"{id}\">\n      <t:variant type=\"{ty}\">{}</t:variant>\n    </sty:item>\n",
            escape(text)
        ));
    };
    for (id, key, kind) in ITEMS {
        let Some(v) = style.ro.get(*key) else { continue };
        match kind {
            Kind::Bool => push(*id, 1, if v.as_bool().unwrap_or(false) { "1" } else { "0" }),
            Kind::Int => push(*id, 4, &v.as_f64().unwrap_or(0.0).round().to_string()),
            Kind::Double => push(*id, 7, &v.as_f64().unwrap_or(0.0).to_string()),
            Kind::Float => push(*id, 6, &v.as_f64().unwrap_or(0.0).to_string()),
            Kind::Color => {
                if let Some(t) = color_text(v) {
                    push(*id, 4, &t)
                }
            }
        }
    }
    let planes = style.ro.get("DisplaySectionPlanes").and_then(J::as_bool).unwrap_or(false);
    let cuts = style.ro.get("DisplaySectionCuts").and_then(J::as_bool).unwrap_or(true);
    push(SECTION_MODE, 4, &((planes as u32) | (cuts as u32) << 1).to_string());
    let mut files = Vec::new();
    // Watermarks: those behind the model, the model's place, those over it.
    let mut shots = String::new();
    let mut next = 1;
    let mut wm_xml = |w: &Watermark, files: &mut Vec<(String, Vec<u8>)>| {
        let path = format!("watermark_{next}.png");
        next += 1;
        files.push((format!("ref/{path}"), w.image.0.to_vec()));
        format!(
            "<n1:screenimage name=\"{}\" info_found=\"1\" info_filename=\"{}\" info_time=\"0\" position=\"{}\" tiled=\"{}\" stretched=\"{}\" maintainAR=\"{}\" background=\"{}\" intensForAlpha=\"{}\" alphaScale=\"{}\" scale=\"{}\"><n1:images><n1:image id=\"{}\" path=\"{}\" /></n1:images></n1:screenimage>",
            escape(&w.name),
            escape(&path),
            w.position,
            (w.layout == WmLayout::Tiled) as u8,
            (w.layout == WmLayout::Stretched) as u8,
            w.keep_aspect as u8,
            w.background as u8,
            w.mask as u8,
            w.blend,
            w.scale,
            next - 1,
            escape(&path)
        )
    };
    for w in style.watermarks.iter().filter(|w| w.background) {
        shots.push_str(&wm_xml(w, &mut files));
    }
    shots.push_str("<n1:screenimage name=\"&lt;MODEL SPACE&gt;\" />");
    for w in style.watermarks.iter().filter(|w| !w.background) {
        shots.push_str(&wm_xml(w, &mut files));
    }
    items.push_str(&format!(
        "    <sty:item id=\"{WATERMARKS}\">\n      <t:variant type=\"13\" xmlns:n1=\"http://sketchup.google.com/schemas/sketchup/1.0/wmlist\"><n1:wmlist count=\"{}\"><n1:screenimages>{shots}</n1:screenimages></n1:wmlist></t:variant>\n    </sty:item>\n",
        style.watermarks.len() + 1
    ));
    let doc = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<styleDocument xmlns=\"http://sketchup.google.com/schemas/sketchup/1.0/style\" xmlns:sty=\"http://sketchup.google.com/schemas/sketchup/1.0/style\" xmlns:r=\"http://sketchup.google.com/schemas/1.0/references\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n  <sty:style xmlns:t=\"http://sketchup.google.com/schemas/1.0/types\" name=\"{}\" desc=\"{}\">\n{items}  </sty:style>\n</styleDocument>\n",
        escape(&style.name),
        escape(&style.description)
    );
    let props = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<documentProperties xmlns=\"http://sketchup.google.com/schemas/1.0/documentproperties\" xmlns:dp=\"http://sketchup.google.com/schemas/1.0/documentproperties\">\n  <dp:title>{}</dp:title>\n  <dp:description>{}</dp:description>\n  <dp:creator></dp:creator>\n  <dp:keywords></dp:keywords>\n  <dp:lastModifiedBy></dp:lastModifiedBy>\n  <dp:revision>0</dp:revision>\n  <dp:created>2026-01-01T00:00:00Z</dp:created>\n  <dp:modified>2026-01-01T00:00:00Z</dp:modified>\n  <dp:thumbnail>doc_thumbnail.png</dp:thumbnail>\n  <dp:generator dp:name=\"Style\" dp:version=\"1\" />\n</documentProperties>\n",
        escape(&style.name),
        escape(&style.description)
    );
    let refs = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<references xmlns=\"http://sketchup.google.com/schemas/1.0/references\" />\n";
    let mut all = vec![
        ("document.xml".to_string(), doc.into_bytes()),
        ("documentProperties.xml".to_string(), props.into_bytes()),
        ("references.xml".to_string(), refs.as_bytes().to_vec()),
    ];
    if let Some(t) = &style.thumbnail {
        all.push(("doc_thumbnail.png".to_string(), t.clone()));
    }
    all.extend(files);
    crate::zip::write(&all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::ViewStyle;

    #[test]
    fn xml_is_read_with_attributes_entities_and_namespaces() {
        let n = parse_xml("<?xml version=\"1.0\"?><a:b x='1 &amp; 2'><!-- c --><c>t&#x0A;u</c><d/></a:b>").unwrap();
        let b = &n.children[0];
        assert_eq!((b.local(), b.attr("x")), ("b", Some("1 & 2")));
        assert_eq!(b.child("c").unwrap().text, "t\nu");
        assert!(b.child("d").is_some());
        assert!(parse_xml("<a><b></a>").is_err());
    }

    /// A `.style` / `.skm` / `.ske` document with an attribute quoted by a non-ASCII
    /// character (the reader sliced inside it: a panic on opening the file), and one nested
    /// 200 000 deep (searching then dropping the tree recursed until the stack overflowed).
    #[test]
    fn hostile_xml_is_refused_without_panic() {
        assert_eq!(parse_xml("<a b=éxé/>").unwrap().children[0].attr("b"), Some("x"));
        assert!(parse_xml("<a b=é").is_err());
        assert!(parse_xml("<a b=").is_err());
        let deep = "<a>".repeat(200_000) + &"</a>".repeat(200_000);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(move || {
                let _ = tx.send(parse_xml(&deep).is_err());
            })
            .unwrap();
        assert!(rx.recv_timeout(std::time::Duration::from_secs(20)).expect("no stack overflow"));
        let ok = "<a>".repeat(200) + &"</a>".repeat(200);
        assert!(parse_xml(&ok).is_ok());
    }

    #[test]
    fn strings_files_are_read() {
        let m = parse_strings("/* c */\n\"A b\"=\"C \\\"d\\\"\";\n\"x\" = \"y\";\n");
        assert_eq!(m["A b"], "C \"d\"");
        assert_eq!(m["x"], "y");
    }

    #[test]
    fn styles_round_trip_through_style_files() {
        let mut view = ViewStyle {
            bg_color: [12, 34, 56],
            endpoints: true,
            endpoint_length: 7,
            section_planes: true,
            xray: true,
            ..Default::default()
        };
        view.watermark_list.push(Watermark {
            name: "Papier".into(),
            image: esq_core::photo::Blob(std::sync::Arc::new(crate::photo_img::tests::png_bytes(4, 4))),
            background: true,
            blend: 0.5,
            mask: false,
            layout: WmLayout::Tiled,
            keep_aspect: true,
            scale: 0.6,
            position: 4,
            skp: Default::default(),
        });
        let f = StyleFile {
            name: "Mon style".into(),
            description: "Bleu & gris\nsur deux lignes".into(),
            ro: view.snapshot(),
            watermarks: view.watermark_list.clone(),
            thumbnail: None,
            sketchy: false,
        };
        let back = read(&write(&f)).unwrap();
        assert_eq!((back.name.as_str(), back.description.as_str()), ("Mon style", "Bleu & gris\nsur deux lignes"));
        let mut v2 = ViewStyle::default();
        v2.restore(&back.ro);
        v2.watermark_list = back.watermarks.clone();
        assert_eq!(v2.bg_color, [12, 34, 56]);
        assert!(v2.endpoints && v2.endpoint_length == 7 && v2.section_planes && v2.xray);
        assert_eq!(back.watermarks, view.watermark_list);
    }

    #[test]
    fn sketchup_library_styles_are_read() {
        // SketchUp's own styles, when it is installed (read only, nothing is copied).
        let dir = std::path::Path::new(r"C:\ProgramData\SketchUp\SketchUp 2026\SketchUp\Styles");
        let Ok(bytes) = std::fs::read(dir.join("Default Styles").join("06Simple Style.style")) else { return };
        let s = read(&bytes).unwrap();
        let mut v = ViewStyle::default();
        v.restore(&s.ro);
        // The Simple style is the one of the Simple template.
        let d = ViewStyle::default();
        assert_eq!(
            (v.bg_color, v.sky_color, v.back_color, v.profile_width),
            (d.bg_color, d.sky_color, d.back_color, 2)
        );
        assert!(!s.sketchy);
        // Its name comes from SketchUp's French translations.
        assert_eq!(s.name, "Style simple");
        let Ok(bytes) = std::fs::read(dir.join("Assorted Styles").join("01PSO Vignette.style")) else { return };
        let s = read(&bytes).unwrap();
        assert_eq!(s.watermarks.len(), 3);
        assert!(s.watermarks[0].background && s.watermarks[0].layout == WmLayout::Tiled);
        assert!(s.watermarks[2].mask && !s.watermarks[2].background && s.watermarks[2].layout == WmLayout::Stretched);
        assert!(s.sketchy);
    }
}
