//! The SketchUp 2021+ file (the format of File › Save, unchanged since SketchUp 2021): the
//! header, the « VFF » marker, then a ZIP archive holding `meta/meta.dat`, the materials
//! (`materials/<folder>/material.xml` and their images), the styles and `model.dat`. The
//! records follow the writers of SketchUp.exe (for the model,
//! for a collection…), in their order, with the values SketchUp gives what
//! Esquisse does not have (`docs/skp-format.md`, §2 and §8).

use super::plan::*;
use super::rec::Rec;
use super::xml;
use super::zip::ZipWriter;

/// The version written in the header and in `meta/meta.dat`: the SketchUp whose format is
/// written (the SDK of SketchUp 2026 writes the same).
pub const VERSION: (u32, u32, u32) = (26, 2, 243);

/// An MFC `CString` of the header (`FF FE FF`, length, UTF-16).
pub fn cstring(out: &mut Vec<u8>, s: &str) {
    let units: Vec<u16> = s.encode_utf16().collect();
    out.extend([0xFF, 0xFE, 0xFF]);
    let n = units.len();
    if n < 0xFF {
        out.push(n as u8);
    } else if n < 0xFFFF {
        out.push(0xFF);
        out.extend((n as u16).to_le_bytes());
    } else {
        out.push(0xFF);
        out.extend(0xFFFFu16.to_le_bytes());
        out.extend((n as u32).to_le_bytes());
    }
    for u in units {
        out.extend(u.to_le_bytes());
    }
}

/// Where the styles of the plan go in the archive: their folders (`styles/<folder>/`, the
/// list's then the active one's), their `style.xml` and files, the watermark images
/// (`watermarks/…`, the model's first) and the path of each of the model's watermarks.
pub struct StyleLayout {
    pub folders: Vec<String>,
    pub active_folder: String,
    pub docs: Vec<(String, String, crate::native::style::Files)>,
    pub images: crate::native::style::Files,
    pub watermark_paths: Vec<Option<String>>,
}

/// The active style written: as read, else a copy of the selected one (a 2021+ file always
/// has one).
fn active_of(st: &crate::native::style::Styles) -> Option<crate::native::style::StyleRec> {
    st.active.clone().or_else(|| {
        let s = st.selected.and_then(|i| st.list.get(i)).or(st.list.first())?;
        Some(crate::native::style::StyleRec { folder: String::new(), watermark_ids: Vec::new(), ..s.clone() })
    })
}

pub fn style_layout(p: &Plan) -> StyleLayout {
    use crate::native::style::{sanitized, to_xml, watermark_path};
    let st = &p.styles;
    let mut taken = std::collections::HashSet::new();
    let mut folder = |want: &str, base: &str| {
        let mut f = if want.is_empty() { sanitized(base) } else { want.to_string() };
        let stem = f.clone();
        let mut k = 1;
        while !taken.insert(f.to_lowercase()) {
            f = format!("{stem}_{k}");
            k += 1;
        }
        f
    };
    let folders: Vec<String> = st.list.iter().map(|s| folder(&s.folder, &s.name)).collect();
    let sel_folder = st.selected.and_then(|i| folders.get(i)).cloned().unwrap_or_else(|| "Style".into());
    let active = active_of(st);
    let active_folder = match &active {
        Some(a) if !a.folder.is_empty() => folder(&a.folder, &a.name),
        _ => folder("", &format!("{sel_folder}_1")),
    };
    let mut images = Vec::new();
    let mut wm_taken = std::collections::HashSet::new();
    let watermark_paths =
        st.watermarks.iter().map(|w| w.as_ref().and_then(|w| watermark_path(w, &mut images, &mut wm_taken))).collect();
    let mut docs = Vec::new();
    for (s, f) in st.list.iter().zip(&folders).chain(active.iter().map(|a| (a, &active_folder))) {
        let (x, files) = to_xml(s, &mut images, &mut wm_taken);
        docs.push((f.clone(), x, files));
    }
    StyleLayout { folders, active_folder, docs, images, watermark_paths }
}

/// An arc as 16 doubles (0x4c2d, 0x5dc2): centre, plane (normal, d), x and y
/// axes, radius, angles.
fn arc_values(a: &super::plan::Arc3) -> Vec<f64> {
    let mut v = Vec::with_capacity(16);
    v.extend(a.center);
    v.extend(a.normal);
    v.push(-(a.normal[0] * a.center[0] + a.normal[1] * a.center[1] + a.normal[2] * a.center[2]));
    v.extend(a.xaxis);
    v.extend(a.yaxis);
    v.extend([a.radius, a.start, a.end]);
    v
}

/// The whole file.
pub fn file(p: &Plan) -> Vec<u8> {
    let mut z = ZipWriter::new();
    if let Some(png) = &p.thumbnail {
        z.add("meta/model_thumbnail.png", png);
        z.add("meta/preview_thumbnail.png", png);
    }
    z.add("meta/meta.dat", &meta_dat(p));
    // The materials: the model's, then the tags'.
    for m in p.materials.iter().chain(p.layers.iter().map(|l| &l.material)) {
        material_files(&mut z, m);
    }
    if p.styles.list.is_empty() {
        let style = xml::style_xml("Style");
        z.add("styles/Style/style.xml", style.as_bytes());
        z.add("styles/Style_1/style.xml", xml::style_xml("Style_1").as_bytes());
    } else {
        let l = style_layout(p);
        for (path, b) in &l.images {
            z.add(path, b);
        }
        for (folder, x, files) in &l.docs {
            for (name, b) in files {
                z.add(&format!("styles/{folder}/{name}"), b);
            }
            z.add(&format!("styles/{folder}/style.xml"), x.as_bytes());
        }
    }
    for (s, stored) in p.schemas.iter().zip(schema_paths(p)) {
        z.add(&stored, &s.data);
    }
    for (d, path) in p.defs.iter().zip(thumbnail_paths(p)) {
        if let (Some(img), Some(path)) = (&d.thumb, path) {
            z.add(&path, img);
        }
    }
    for (s, path) in p.scenes.iter().zip(scene_thumbnail_paths(p)) {
        if let (Some(img), Some(path)) = (&s.thumbnail, path) {
            z.add(&path, img);
        }
    }
    for (m, path) in p.photos.iter().zip(photo_paths(p)) {
        if let (Some(img), Some(path)) = (&m.image, path) {
            z.add(&path, img);
        }
    }
    for h in &p.hatch_patterns {
        let dir = format!("hatchpatterns/{}", h.folder);
        if let Some(img) = &h.settings.image {
            z.add(&format!("{dir}/{}", h.settings.texture_name), img);
        }
        z.add(&format!("{dir}/hatchpattern.xml"), hatch_xml(h).as_bytes());
        if let Some(t) = &h.thumbnail {
            z.add(&format!("{dir}/thumbnail.{}", thumbnail_ext(t)), t);
        }
    }
    for e in &p.environments {
        let dir = format!("environments/{}", e.folder);
        if let Some(img) = &e.image {
            z.add(&format!("{dir}/{}", e.image_name), img);
        }
        z.add(&format!("{dir}/environment.xml"), environment_xml(e).as_bytes());
        if let Some(t) = &e.thumbnail {
            z.add(&format!("{dir}/thumbnail.jpg"), t);
        }
    }
    z.add("model.dat", &model_dat(p));
    let (archive, check) = z.finish();
    let mut out = Vec::with_capacity(archive.len() + 64);
    cstring(&mut out, "SketchUp Model");
    cstring(&mut out, &format!("{{{}.{}.{}}}", VERSION.0, VERSION.1, VERSION.2));
    out.extend(b"VFF");
    out.extend(8u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(0x11u16.to_le_bytes());
    out.extend(check.to_le_bytes());
    out.extend(archive);
    out
}

/// Where each definition's thumbnail goes in the archive (`thumbnails/<name>.png`, as SketchUp
/// names them; a second one of the same file name gets a number).
fn thumbnail_paths(p: &Plan) -> Vec<Option<String>> {
    let mut taken = std::collections::HashSet::new();
    p.defs
        .iter()
        .map(|d| {
            let img = d.thumb.as_ref()?;
            let ext = if img.starts_with(&[0xFF, 0xD8]) { "jpg" } else { "png" };
            let base: String = d.name.chars().map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
            let mut path = format!("thumbnails/{base}.{ext}");
            let mut k = 1;
            while !taken.insert(path.to_lowercase()) {
                path = format!("thumbnails/{base}_{k}.{ext}");
                k += 1;
            }
            Some(path)
        })
        .collect()
}

/// Where each schema's `.skc` goes in the archive (`classifications/<name>.skc`, as SketchUp
/// names it; a second schema of the same file name gets a number).
fn schema_paths(p: &Plan) -> Vec<String> {
    let mut taken = std::collections::HashSet::new();
    p.schemas
        .iter()
        .map(|s| {
            let base = format!("classifications/{}", s.name);
            let mut path = format!("{base}.skc");
            let mut k = 1;
            while !taken.insert(path.to_lowercase()) {
                path = format!("{base}_{k}.skc");
                k += 1;
            }
            path
        })
        .collect()
}

/// The extension of a thumbnail's file (SketchUp's are JPEG).
fn thumbnail_ext(t: &[u8]) -> &'static str {
    if t.starts_with(&[0xFF, 0xD8]) { "jpg" } else { "png" }
}

fn material_files(z: &mut ZipWriter, m: &Mat) {
    let dir = format!("materials/{}", m.folder);
    z.add(&format!("{dir}/material.xml"), xml::material_xml(m).as_bytes());
    // Its thumbnail as read, beside (record 0x32ce refers to it).
    if let Some(t) = &m.thumbnail {
        z.add(&format!("{dir}/thumbnail.{}", thumbnail_ext(t)), t);
    }
    let mut written = std::collections::HashSet::new();
    let p = &m.pbr;
    for t in
        [&m.texture, &p.metallic_texture, &p.roughness_texture, &p.normal_texture, &p.ao_texture].into_iter().flatten()
    {
        if written.insert(t.stored.clone()) {
            z.add(&format!("{dir}/{}", t.stored), &t.image);
        }
    }
}

/// `meta/meta.dat`: what the file is.
pub fn meta_dat(p: &Plan) -> Vec<u8> {
    let mut r = Rec::new();
    r.nest(0x64, |r| {
        r.str(0x75, &format!("{}.{}.{}", VERSION.0, VERSION.1, VERSION.2));
        r.u16(0x76, VERSION.0 as u16);
        r.u16(0x77, 1);
        r.u16(0x73, 1);
        r.u16(0x74, 0x11);
        r.guid(0x66, &p.guid);
        if p.thumbnail.is_some() {
            for (tag, name) in [(0x67, "meta/model_thumbnail.png"), (0x68, "meta/preview_thumbnail.png")] {
                r.nest(tag, |r| {
                    r.nest(0x2328, |r| {
                        r.u32(0x2329, 4);
                        r.str(0x232a, name);
                    })
                });
            }
        } else {
            r.empty(0x67);
            r.empty(0x68);
        }
        r.bool(0x69, true);
        r.str(0x6a, &p.name);
        r.str(0x6b, &p.description);
        r.empty(0x6c);
        r.empty(0x6e);
        r.bool(0x71, false);
        r.bool(0x79, false);
        r.bool(0x72, false);
        r.str(0x6d, unit_name(p.unit));
        r.bool(0x70, true);
        r.empty(0x6f);
        r.nest(0x78, |r| {
            r.nest(0xc8, |r| {
                r.nest(0xc9, |r| {
                    r.nest(0xca, |r| {
                        r.str(0xcb, "SketchUp");
                        r.u32(0xcc, p.time);
                        r.bytes(0xcd, &p.guid[..8]);
                    })
                })
            })
        });
    });
    r.buf
}

/// The name of the length unit in `meta/meta.dat`.
fn unit_name(u: i32) -> &'static str {
    match u {
        0 => "Inches",
        1 => "Feet",
        3 => "Centimeter",
        4 => "Meter",
        5 => "Yard",
        _ => "Millimeter",
    }
}

/// A typed value (record 0x38a4 and its one child, 0x38a3 + the variant type).
pub fn typed(r: &mut Rec, v: &Tv, depth: usize) {
    r.nest(0x38a4, |r| {
        let tag = 0x38a3 + v.variant() as u16;
        match v {
            Tv::Byte(x) => r.u8(tag, *x),
            Tv::Short(x) => r.bytes(tag, &x.to_le_bytes()),
            Tv::Int(x) => r.i32(tag, *x),
            Tv::Float(x) => r.f32(tag, *x),
            Tv::Double(x) => r.f64(tag, *x),
            Tv::Bool(x) => r.bool(tag, *x),
            Tv::Color(c) => r.color(tag, *c),
            Tv::Time(t) => r.u32(tag, (*t).clamp(i32::MIN as i64, u32::MAX as i64) as u32),
            Tv::Str(s) => r.str(tag, s),
            Tv::Array(items) => r.nest(tag, |r| {
                if depth < 16 {
                    for it in items {
                        typed(r, it, depth + 1);
                    }
                }
            }),
            Tv::Point(p) | Tv::Vector(p) => r.f64s(tag, p),
        }
    });
}

/// A dictionary (`CAttributeNamed`, 0x36b3): its own entity (0x36b0) holds the dictionaries
/// nested in it.
fn dict(r: &mut Rec, d: &Dict) {
    r.nest(0x36b3, |r| {
        if d.children.is_empty() {
            r.empty(0x36b0);
        } else {
            r.nest(0x36b0, |r| entity(r, 0, &d.children, None));
        }
        r.str(0x36b4, &d.name);
        r.nest(0x36b5, |r| {
            let mut kv: Vec<&(String, Tv)> = d.values.iter().collect();
            kv.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            for (k, v) in kv {
                r.str(0x36b6, k);
                typed(r, v, 0);
            }
        });
        r.u32(0x36b7, 0);
    });
}

/// An attribute container (0x36b1 → 0x36b2): the dictionaries, then a face's texture
/// placement (the SDK reader lists only the dictionaries before it).
fn container(r: &mut Rec, dicts: &[Dict], tex: Option<&[Tc; 2]>) {
    r.nest(0x36b1, |r| {
        r.nest(0x36b2, |r| {
            for d in dicts {
                dict(r, d);
            }
            if let Some(t) = tex {
                r.nest(0x2710, |r| {
                    r.empty(0x36b0);
                    for (tag, side) in [(0x2711, &t[0]), (0x2712, &t[1])] {
                        r.nest(tag, |r| {
                            r.nest(0x2713, |r| {
                                r.u32(0x2714, side.flags);
                                r.f64s(0x2715, &side.matrix);
                                r.f64s(0x2716, &side.projection);
                                r.empty(0x2717);
                            })
                        });
                    }
                });
            }
        })
    });
}

/// The entity part (0x5dc): id, dictionaries; written only when there is one of them.
fn entity(r: &mut Rec, id: Id, dicts: &[Dict], tex: Option<&[Tc; 2]>) {
    if id == 0 && dicts.is_empty() && tex.is_none() {
        return;
    }
    r.nest(0x5dc, |r| {
        if id != 0 {
            r.id(0x5de, id);
        }
        if !dicts.is_empty() || tex.is_some() {
            r.nest(0x5dd, |r| container(r, dicts, tex));
        }
    });
}

/// The drawing element part (0x7d0): entity, material, tag, flags.
fn element(r: &mut Rec, el: &El) {
    r.nest(0x7d0, |r| {
        entity(r, el.id, &el.dicts, el.tex.as_deref());
        if let Some(m) = el.material {
            r.id(0x7d1, m);
        }
        if let Some(l) = el.layer {
            r.id(0x7d2, l);
        }
        r.u8(0x7d3, el.flags);
    });
}

pub fn camera(r: &mut Rec, c: &Camera) {
    r.nest(0x34bc, |r| {
        r.f64s(0x34bd, &c.eye);
        r.f64s(0x34be, &c.target);
        r.f64s(0x34bf, &c.up);
        r.f64(0x34c0, c.near);
        r.f64(0x34c1, c.far);
        r.bool(0x34c2, c.perspective);
        r.f64(0x34c4, c.fov);
        r.f64(0x34c3, c.height);
        r.f64(0x34c5, 0.0);
        r.bool(0x34c6, true);
        r.bool(0x34c7, false);
        r.empty(0x34c8);
        r.f64(0x34c9, 0.0);
        r.bool(0x34ca, false);
        r.f64(0x34cb, 1.0);
        r.f64(0x34cc, 0.0);
        r.f64(0x34cd, 0.0);
        r.bool(0x34ce, false);
    });
}

/// The camera of a definition's thumbnail (the SDK's, never set).
fn thumbnail_camera() -> Camera {
    Camera {
        eye: [0.0, 0.0, 1.0],
        target: [0.0, 0.0, 2.0],
        up: [0.0, 1.0, 0.0],
        perspective: true,
        fov: 30.0,
        near: 0.0,
        far: 0.0,
        height: 1.0,
    }
}

pub fn rendering(r: &mut Rec, ro: &[(u16, Ro)], hatch: &crate::native::doc::HatchSettings) {
    r.nest(0x733c, |r| {
        for (tag, v) in ro {
            // The hatch pattern of the section fills, before its switch.
            if *tag == 0x738c {
                r.nest(0x7388, |r| r.nest(0x7d0a, |r| hatch_settings(r, hatch)));
            }
            match v {
                Ro::Int(x) => r.i32(*tag, *x),
                Ro::Bool(x) => r.bool(*tag, *x),
                Ro::Double(x) => r.f64(*tag, *x),
                Ro::Float(x) => r.f32(*tag, *x),
                Ro::Color(c) => r.color(*tag, *c),
            }
        }
    });
}

/// A hatch pattern's settings inline: colour, the image (its bytes: no file
/// name is given), its file name, scales, the .pat floats when it has them, rotation, the .pat
/// file's name.
fn hatch_settings(r: &mut Rec, h: &crate::native::doc::HatchSettings) {
    r.color(0x7d0c, h.color);
    r.nest(0x7d0d, |r| {
        if let Some(img) = &h.image {
            r.nest(0x2328, |r| {
                r.u32(0x2329, 4);
                r.bytes(0x232b, img);
            });
        }
    });
    r.str(0x7d12, &h.texture_name);
    r.f32(0x7d0e, h.scale[0]);
    r.f32(0x7d0f, h.scale[1]);
    r.f32(0x7d14, h.rotation);
    if let Some(f) = &h.pat {
        let b: Vec<u8> = f.iter().flat_map(|x| x.to_le_bytes()).collect();
        r.bytes(0x7d11, &b);
    }
    r.str(0x7d13, &h.pat_name);
}

/// The hatch patterns (0x7d00 → 0x7d01 → 0x7d0a): entity, its
/// folder, its name, its thumbnail.
fn hatch_patterns(r: &mut Rec, p: &Plan) {
    r.nest(0x7d00, |r| {
        r.nest(0x7d01, |r| {
            for h in &p.hatch_patterns {
                r.nest(0x7d0a, |r| {
                    entity(r, h.id, &[], None);
                    r.str(0x7d15, &h.folder);
                    r.str(0x7d0b, &h.name);
                    if let Some(t) = &h.thumbnail {
                        let path = format!("hatchpatterns/{}/thumbnail.{}", h.folder, thumbnail_ext(t));
                        r.nest(0x7d10, |r| r.str(0x2134, &path));
                    }
                });
            }
        });
    });
}

/// A hatch pattern's `hatchpattern.xml` (the root `<element>Document`, the
/// pattern's name then its settings; the image of an image pattern a file of
/// its folder, `<textureName>`).
fn hatch_xml(h: &crate::native::doc::HatchPattern) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let s = &h.settings;
    let mut a = format!(
        "name=\"{}\" colorRed=\"{}\" colorGreen=\"{}\" colorBlue=\"{}\" hasTexture=\"{}\" xScale=\"{}\" yScale=\"{}\" rotationAngle=\"{}\" hasPat=\"{}\"",
        esc(&h.name),
        s.color[0],
        s.color[1],
        s.color[2],
        s.image.is_some() as u8,
        s.scale[0],
        s.scale[1],
        s.rotation,
        s.pat.is_some() as u8,
    );
    if let Some(f) = &s.pat {
        let b: Vec<u8> = f.iter().flat_map(|x| x.to_le_bytes()).collect();
        a.push_str(&format!(" patName=\"{}\" patData=\"{}\"", esc(&s.pat_name), esq_core::photo::base64_encode(&b)));
    }
    let body = if s.image.is_some() {
        format!(
            ">\n    <textureName id=\"1\" path=\"./{0}\" file_name=\"{0}\"/>\n  </hp:hatchpattern>",
            esc(&s.texture_name)
        )
    } else {
        "/>".to_string()
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<hatchpatternDocument xmlns=\"http://sketchup.com/schemas/sketchup/1.0/hatchpattern\" xmlns:hp=\"http://sketchup.com/schemas/sketchup/1.0/hatchpattern\">\n  <hp:hatchpattern {a}{body}\n</hatchpatternDocument>\n"
    )
}

pub fn shadow(r: &mut Rec, s: &Shadow) {
    r.nest(0x6590, |r| {
        r.u32(0x6591, s.time);
        r.bool(0x6592, s.daylight);
        r.str(0x6593, &s.city);
        r.str(0x6594, &s.country);
        r.f64(0x6595, s.longitude);
        r.f64(0x6596, s.latitude);
        r.f64(0x6597, s.tz);
        r.f64s(0x6598, &s.north);
        r.bool(0x6599, s.display);
        r.bool(0x659a, s.display_north);
        r.bool(0x659b, s.on_faces);
        r.bool(0x659c, s.on_ground);
        r.bool(0x659d, s.from_edges);
        r.i32(0x659e, s.light);
        r.i32(0x659f, s.dark);
        r.bool(0x65a0, s.sun_shading);
    });
}

fn axes(r: &mut Rec, a: &[f64; 12]) {
    r.nest(0x4650, |r| {
        r.nest(0x7d0, |r| r.u8(0x7d3, 6));
        r.f64s(0x4651, &a[0..3]);
        r.f64s(0x4652, &a[3..6]);
        r.f64s(0x4653, &a[6..9]);
        r.f64s(0x4654, &a[9..12]);
    });
}

fn material(r: &mut Rec, m: &Mat) {
    r.nest(0x32c8, |r| {
        entity(r, m.id, &[], None);
        r.str(0x32cc, &m.folder);
        r.bool(0x32ca, m.owned);
        r.empty(0x32cb);
        if m.texture.is_some() {
            r.u32(0x32cd, 90);
        }
        // Its thumbnail beside its file, its hatch pattern.
        if let Some(t) = &m.thumbnail {
            r.nest(0x32ce, |r| r.str(0x2134, &format!("materials/{}/thumbnail.{}", m.folder, thumbnail_ext(t))));
        }
        if let Some(h) = m.hatch {
            r.id(0x32cf, h);
        }
    });
}

fn behavior(r: &mut Rec, b: &Behavior) {
    r.nest(0x1b58, |r| {
        r.i32(0x1b59, b.snap_to);
        r.i32(0x1b5a, b.no_scale_mask);
        r.bool(0x1b5b, b.glues);
        r.bool(0x1b5c, b.cuts_opening);
        r.bool(0x1b5d, b.always_face_camera);
        r.bool(0x1b5e, b.shadows_face_sun);
    });
}

fn instance(r: &mut Rec, i: &Inst) {
    r.nest(0x1964, |r| {
        element(r, &i.el);
        r.str(0x1965, &i.name);
        r.f64s(0x1966, &i.xform);
        r.id(0x1967, i.def);
        r.guid(0x1968, &i.guid);
        if let Some(h) = i.hatch {
            r.id(0x1969, h);
        }
    });
}

fn curve(r: &mut Rec, c: &Curve) {
    r.nest(0x4a38, |r| {
        entity(r, c.id, &[], None);
        r.u32(0x4a39, c.segments);
        r.bool(0x4a3a, c.polygon);
        r.id(0x4a3b, c.edges.first().copied().unwrap_or(0));
        r.id(0x4a3c, c.edges.last().copied().unwrap_or(0));
    });
}

/// A point an annotation is attached to (0x5208): free at `p` (type 1), or attached as `c`
/// says (its type, its parameter, 0x520b the entity 0x53fd and the instances 0x53fe that lead
/// to it, outermost first).
fn connection(r: &mut Rec, p: Option<[f64; 3]>, c: Option<&super::plan::Conn>) {
    r.nest(0x5208, |r| {
        match c {
            Some(c) => {
                r.u32(0x5209, c.kind as u32);
                r.f64s(0x520a, &c.point);
                r.nest(0x520b, |r| {
                    r.nest(0x53fc, |r| {
                        if let Some((leaf, _)) = c.leaf {
                            r.id(0x53fd, leaf);
                        }
                        let path: Vec<u64> = c.path.iter().map(|x| x.0).collect();
                        r.ids(0x53fe, &path);
                    })
                });
            }
            None => {
                r.u32(0x5209, p.is_some() as u32);
                r.f64s(0x520a, &p.unwrap_or_default());
                r.nest(0x520b, |r| r.nest(0x53fc, |r| r.empty(0x53fe)));
            }
        }
        r.nest(0x520c, |r| r.nest(0x53fc, |r| r.empty(0x53fe)));
    });
}

fn construction(r: &mut Rec, el: &El) {
    r.nest(0x4268, |r| element(r, el));
}

/// A collection of entities (0x1388).
pub fn entities(r: &mut Rec, e: &Ents) {
    r.nest(0x1388, |r| {
        element(r, &e.el);
        if !e.verts.is_empty() {
            r.nest(0x1389, |r| {
                for v in &e.verts {
                    r.nest(0x9c4, |r| {
                        entity(r, v.id, &[], None);
                        r.f64s(0x9c5, &v.pos);
                    });
                }
            });
        }
        if !e.curves.is_empty() {
            r.nest(0x1396, |r| e.curves.iter().for_each(|c| curve(r, c)));
        }
        if !e.arcs.is_empty() {
            r.nest(0x1397, |r| {
                for c in &e.arcs {
                    r.nest(0x4c2c, |r| {
                        curve(r, c);
                        if let Some(a) = &c.arc {
                            r.f64s(0x4c2d, &arc_values(a));
                        }
                    });
                }
            });
        }
        if !e.edges.is_empty() {
            r.nest(0x138a, |r| {
                for x in &e.edges {
                    r.nest(0xbb8, |r| {
                        element(r, &x.el);
                        r.id(0xbb9, x.start);
                        r.id(0xbba, x.end);
                        if let Some(c) = x.curve {
                            r.id(0xbbb, c);
                        }
                    });
                }
            });
        }
        if !e.faces.is_empty() {
            r.nest(0x138b, |r| {
                for f in &e.faces {
                    r.nest(0xdac, |r| {
                        element(r, &f.el);
                        if let Some(b) = f.back {
                            r.id(0xdaf, b);
                        }
                        r.f64s(0xdad, &f.plane);
                        r.nest(0xdae, |r| {
                            for (_, uses) in &f.loops {
                                r.nest(0x1194, |r| {
                                    r.nest(0x1195, |r| {
                                        for (edge, rev) in uses {
                                            r.nest(0xfa0, |r| {
                                                r.id(0xfa1, *edge);
                                                r.bool(0xfa2, *rev);
                                            });
                                        }
                                    })
                                });
                            }
                        });
                    });
                }
            });
        }
        if !e.instances.is_empty() {
            r.nest(0x138c, |r| e.instances.iter().for_each(|i| instance(r, i)));
        }
        if !e.groups.is_empty() {
            r.nest(0x138d, |r| {
                for g in &e.groups {
                    r.nest(0x1d4c, |r| instance(r, g));
                }
            });
        }
        if !e.images.is_empty() {
            r.nest(0x1390, |r| {
                for i in &e.images {
                    r.nest(0x1f40, |r| instance(r, &i.inst));
                }
            });
        }
        if !e.guide_lines.is_empty() {
            r.nest(0x1391, |r| {
                for g in &e.guide_lines {
                    r.nest(0x4269, |r| {
                        construction(r, &g.el);
                        let mut v = Vec::with_capacity(8);
                        v.extend(g.point);
                        v.extend(g.dir);
                        v.extend(g.t);
                        r.f64s(0x426a, &v);
                        r.u16(0x426b, 0xffff);
                    });
                }
            });
        }
        if !e.guide_points.is_empty() {
            r.nest(0x1392, |r| {
                for g in &e.guide_points {
                    r.nest(0x426c, |r| {
                        construction(r, &g.el);
                        r.f64s(0x426d, &g.pos);
                        r.f64s(0x426e, &[0.0; 3]);
                        r.bool(0x426f, false);
                    });
                }
            });
        }
        if !e.snaps.is_empty() {
            r.nest(0x139d, |r| {
                for s in &e.snaps {
                    r.nest(0x7530, |r| {
                        element(r, &s.el);
                        r.f64s(0x7531, &s.pos);
                        r.f64s(0x7532, &s.dir);
                        r.f64s(0x7533, &s.right);
                        r.f64s(0x7534, &s.up);
                        r.u32(0x7535, 0);
                    });
                }
            });
        }
        if !e.sections.is_empty() {
            r.nest(0x1393, |r| {
                for s in &e.sections {
                    r.nest(0x445c, |r| {
                        element(r, &s.el);
                        r.f64s(0x445d, &s.plane);
                        r.str(0x445e, &s.name);
                        r.str(0x445f, &s.symbol);
                        if let Some(h) = s.hatch {
                            r.id(0x4460, h);
                        }
                    });
                }
            });
        }
        if !e.texts.is_empty() {
            r.nest(0x1398, |r| {
                for t in &e.texts {
                    r.nest(0x55f0, |r| {
                        element(r, &t.el);
                        r.str(0x55f1, &t.text);
                        r.empty(0x55ff);
                        r.f64(0x55f2, t.screen[0]);
                        r.f64(0x55f3, t.screen[1]);
                        r.nest(0x55f4, |r| connection(r, t.point, t.conn.as_ref()));
                        r.f64s(0x55f5, &t.leader);
                        r.bool(0x55f6, true);
                        r.f64s(0x55f7, &[0.0, 0.0, 1.0]);
                        r.bool(0x55f8, false);
                        r.id(0x55f9, t.font);
                        r.i32(0x55fa, t.arrow_type);
                        r.i32(0x55fb, 1);
                        r.bool(0x55fc, true);
                        r.i32(0x55fd, t.leader_type);
                        r.i32(0x55fe, 0);
                    });
                }
            });
        }
        if !e.dims.is_empty() {
            r.nest(0x1399, |r| {
                for d in &e.dims {
                    r.nest(0x5bcc, |r| {
                        r.nest(0x59d8, |r| {
                            element(r, &d.el);
                            r.str(0x59d9, &d.text);
                            if d.font != 0 {
                                r.id(0x59da, d.font);
                            }
                            r.bool(0x59db, false);
                            r.i32(0x59dc, d.arrow_type);
                        });
                        r.nest(0x5bcd, |r| connection(r, Some(d.start), d.conns[0].as_ref()));
                        r.nest(0x5bce, |r| connection(r, Some(d.end), d.conns[1].as_ref()));
                        r.f64s(0x5bcf, &d.normal);
                        r.f64s(0x5bd0, &d.xaxis);
                        r.i32(0x5bd1, 0);
                        r.f64(0x5bd3, d.position[0]);
                        r.f64(0x5bd2, d.position[1]);
                        r.i32(0x5bd4, 1);
                    });
                }
            });
        }
        if !e.radials.is_empty() {
            // the dimension part, the edge of the arc (else 0 and the arc), the
            // angle, the leader's offset, diameter.
            r.nest(0x139a, |r| {
                for d in &e.radials {
                    r.nest(0x5dc0, |r| {
                        r.nest(0x59d8, |r| {
                            element(r, &d.el);
                            r.str(0x59d9, &d.text);
                            if d.font != 0 {
                                r.id(0x59da, d.font);
                            }
                            r.bool(0x59db, false);
                            r.i32(0x59dc, d.arrow_type);
                        });
                        match (d.edge, &d.arc) {
                            (Some(x), _) => r.id(0x5dc1, x),
                            // No edge: no record (SketchUp writes nothing for a null
                            // entity; an id 0 makes SketchUp refuse the file), the arc itself.
                            (None, a) => {
                                if let Some(a) = a {
                                    r.f64s(0x5dc2, &arc_values(a));
                                }
                            }
                        }
                        r.f64(0x5dc3, d.angle);
                        r.f64(0x5dc4, d.leader);
                        r.bool(0x5dc5, d.diameter);
                    });
                }
            });
        }
        if !e.polylines.is_empty() {
            // 3D polylines (0x4844): the drawing element, the points.
            r.nest(0x1395, |r| {
                for pl in &e.polylines {
                    r.nest(0x4844, |r| {
                        element(r, &pl.el);
                        let flat: Vec<f64> = pl.points.iter().flatten().copied().collect();
                        r.f64s(0x4845, &flat);
                    });
                }
            });
        }
        if !e.spaces.is_empty() {
            r.nest(0x13a2, |r| {
                for s in &e.spaces {
                    r.nest(0x7e2c, |r| {
                        element(r, &s.el);
                        r.str(0x7e2d, &s.name);
                        r.u32(0x7e2e, 0);
                        r.u32(0x7e2f, 0);
                        r.f64s(0x7e30, &s.orientation);
                        r.f64s(0x7e31, &s.ceiling);
                        let flat: Vec<f64> = s.perimeter.iter().flatten().copied().collect();
                        r.f64s(0x7e32, &flat);
                        r.nest(0x7e33, |r| {
                            for h in &s.holes {
                                let flat: Vec<f64> = h.iter().flatten().copied().collect();
                                r.f64s(0x7e34, &flat);
                            }
                        });
                    });
                }
            });
        }
        r.empty(0x139e);
        let order = e.order();
        if !order.is_empty() {
            r.ids(0x138e, &order);
        }
        if let Some(a) = e.active_section {
            r.id(0x1394, a);
        }
        // Relationships: what each glued component is glued to.
        r.nest(0x139b, |r| {
            r.nest(0x639f, |r| {
                if e.glues.is_empty() {
                    r.empty(0x63a0);
                } else {
                    r.nest(0x63a0, |r| {
                        for [to, what] in &e.glues {
                            r.nest(0x639c, |r| {
                                r.id(0x639d, *to);
                                r.id(0x639e, *what);
                            });
                        }
                    });
                }
            })
        });
        r.bool(0x139f, e.bbox.is_some());
        if let Some(b) = &e.bbox {
            r.f64s(0x13a0, b);
        }
    });
}

fn folder(r: &mut Rec, f: &Folder, depth: usize) {
    r.nest(0x3e80, |r| {
        if f.id != 0 {
            entity(r, f.id, &[], None);
        }
        r.str(0x3e81, &f.name);
        r.nest(0x3e83, |r| {
            if depth < 64 {
                for s in &f.folders {
                    folder(r, s, depth + 1);
                }
            }
        });
        r.ids(0x3e84, &f.layers);
        r.bool(0x3e82, f.hidden);
        r.bool(0x3e85, f.hidden_on_new_pages);
    });
}

/// A scene (`CViewPage`): what it keeps follows its flags; then its times,
/// its matched photo and its thumbnail (`thumb`, its file in the archive).
fn page(r: &mut Rec, s: &Page, style: Id, thumb: Option<&str>, hatch: &crate::native::doc::HatchSettings) {
    r.nest(0x7148, |r| {
        r.nest(0x6f54, |r| {
            entity(r, s.id, &s.dicts, None);
            r.str(0x6f55, &s.name);
            r.str(0x6f56, &s.description);
        });
        r.u32(0x7149, s.flags);
        if s.flags & 1 != 0 {
            r.nest(0x714a, |r| camera(r, &s.camera));
        }
        if s.flags & 0x10 != 0 {
            r.empty(0x714b);
        }
        // The scene's style: SketchUp keeps it even when the scene does not save its style.
        if s.flags & 2 == 0
            && let Some(st) = s.style
        {
            r.id(0x714c, st);
        }
        if s.flags & 2 != 0 {
            r.id(0x714c, style);
            r.nest(0x714d, |r| {
                if let Some(ro) = &s.rendering {
                    rendering(r, ro, s.ro_hatch.as_ref().unwrap_or(hatch));
                }
            });
        }
        if s.flags & 4 != 0 {
            r.nest(0x714e, |r| {
                if let Some(sh) = &s.shadow {
                    shadow(r, sh);
                }
            });
        }
        if s.flags & 0x200 != 0 {
            if let Some(e) = s.env_id {
                r.id(0x715a, e);
            }
            // The environment the scene keeps: its settings (none: SketchUp's
            // defaults).
            let e = s.env.unwrap_or(crate::native::doc::SceneEnv {
                skydome_exposure: 1.0,
                reflection_exposure: 1.0,
                ..Default::default()
            });
            r.nest(0x715b, |r| {
                r.bool(0x7b0f, e.reflections);
                r.bool(0x7b10, e.skydome);
                r.f32(0x7b11, e.rotation);
                r.f32(0x7b12, e.skydome_exposure);
                r.f32(0x7b13, e.reflection_exposure);
                r.bool(0x7b14, e.linked_sun);
                r.f32(0x7b15, e.sun[0]);
                r.f32(0x7b16, e.sun[1]);
            });
        }
        if s.flags & 8 != 0 {
            r.nest(0x714f, |r| {
                if let Some(a) = &s.axes {
                    axes(r, a);
                }
            });
        }
        if s.flags & 0x20 != 0 {
            r.ids(0x7150, &s.hidden_layers);
            r.empty(0x7159);
        }
        if s.flags & 0x40 != 0 {
            r.empty(0x7151);
        }
        r.bool(0x7152, s.animate);
        if s.v7153.is_empty() {
            r.empty(0x7153);
        } else {
            r.str(0x7153, &s.v7153);
        }
        r.f64(0x7154, s.transition);
        r.f64(0x7155, s.delay);
        if let Some(ph) = s.photo {
            r.id(0x7156, ph);
        }
        r.empty(0x715c);
        r.bool(0x7157, s.v7157);
        if let Some(path) = thumb {
            r.nest(0x7158, |r| r.str(0x2134, path));
        }
    });
}

/// Where each scene's thumbnail goes in the archive (`scene_thumbnails/<name>.png`).
fn scene_thumbnail_paths(p: &Plan) -> Vec<Option<String>> {
    let mut taken = std::collections::HashSet::new();
    p.scenes.iter().map(|s| Some(unique_file(&mut taken, "scene_thumbnails", &s.name, s.thumbnail.as_ref()?))).collect()
}

/// Where each matched photo goes in the archive (`matched_photos/<file name>`).
fn photo_paths(p: &Plan) -> Vec<Option<String>> {
    let mut taken = std::collections::HashSet::new();
    p.photos
        .iter()
        .map(|m| {
            let name = m.path.rsplit(['/', '\\']).next().unwrap_or_default();
            let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
            Some(unique_file(
                &mut taken,
                "matched_photos",
                if stem.is_empty() { "photo" } else { stem },
                m.image.as_ref()?,
            ))
        })
        .collect()
}

/// `<dir>/<name>.<png|jpg>` (by the image's bytes), not taken yet (case aside).
fn unique_file(taken: &mut std::collections::HashSet<String>, dir: &str, name: &str, img: &[u8]) -> String {
    let ext = if img.starts_with(&[0xFF, 0xD8]) { "jpg" } else { "png" };
    let base: String = name.chars().map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let mut path = format!("{dir}/{base}.{ext}");
    let mut k = 1;
    while !taken.insert(path.to_lowercase()) {
        path = format!("{dir}/{base}_{k}.{ext}");
        k += 1;
    }
    path
}

/// The environments (0x210 → 0x7918 → 0x7919 list of 0x7b0c, 0x791a the current one).
fn environments(r: &mut Rec, p: &Plan) {
    r.nest(0x210, |r| {
        r.nest(0x7918, |r| {
            if p.environments.is_empty() {
                r.empty(0x7919);
                return;
            }
            r.nest(0x7919, |r| {
                for e in &p.environments {
                    r.nest(0x7b0c, |r| {
                        entity(r, e.id, &[], None);
                        r.str(0x7b0d, &e.folder);
                        if e.thumbnail.is_some() {
                            r.nest(0x7b0e, |r| r.str(0x2134, &format!("environments/{}/thumbnail.jpg", e.folder)));
                        }
                    });
                }
            });
            r.id(0x791a, p.current_env.unwrap_or(0));
        })
    });
}

/// An environment's `environment.xml`, as SketchUp writes it.
fn environment_xml(e: &crate::native::doc::Environment) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let b = |x: bool| if x { "1" } else { "0" };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<environmentDocument xmlns=\"http://sketchup.com/schemas/sketchup/1.0/environment\" xmlns:env=\"http://sketchup.com/schemas/sketchup/1.0/environment\">\n  <env:environment name=\"{}\" description=\"{}\" ibl_enabled=\"{}\" rotation_angle=\"{}\" skybox_enabled=\"{}\" skybox_gain=\"{}\" reflection_gain=\"{}\" shadow_light_enabled=\"{}\" shadow_light_positionX=\"{}\" shadow_light_positionY=\"{}\" ibl_filename=\"{}\">\n    <ibl_image id=\"1\" path=\"./{}\" file_name=\"{}\"/>\n  </env:environment>\n</environmentDocument>\n",
        esc(&e.name),
        esc(&e.description),
        b(e.reflections),
        e.rotation,
        b(e.skydome),
        e.skydome_exposure,
        e.reflection_exposure,
        b(e.linked_sun),
        e.sun[0],
        e.sun[1],
        esc(&e.image_name),
        esc(&e.image_name),
        esc(&e.image_name),
    )
}

/// The matched photos (0x201 → 0x2904; for what each field is) and the active
/// one (0x202).
fn match_photos(r: &mut Rec, p: &Plan) {
    r.nest(0x201, |r| {
        for (m, path) in p.photos.iter().zip(photo_paths(p)) {
            r.nest(0x2904, |r| {
                entity(r, m.id, &[], None);
                r.nest(0x2905, |r| {
                    r.nest(0x2af8, |r| {
                        r.str(0x2af9, &m.path);
                        r.u32(0x2afa, m.kind);
                        if let Some(path) = &path {
                            r.nest(0x2afb, |r| {
                                r.nest(0x2328, |r| {
                                    r.u32(0x2329, 4);
                                    r.str(0x232a, path);
                                })
                            });
                        }
                        r.u32(0x2afc, m.size[0]);
                        r.u32(0x2afd, m.size[1]);
                        r.u32(0x2afe, m.v2afe);
                        r.u32(0x2aff, m.time);
                    })
                });
                r.bool(0x2906, m.flag);
                r.f64(0x2907, m.value);
                let flat: Vec<f64> = m.points.iter().flatten().copied().collect();
                r.f64s(0x2908, &flat);
                r.f64s(0x2909, &m.offset);
                r.f64(0x290a, m.scale);
                r.u32(0x290b, m.v290b);
            });
        }
    });
    if let Some(a) = p.active_photo {
        r.id(0x202, a);
    }
}

/// `model.dat`.
pub fn model_dat(p: &Plan) -> Vec<u8> {
    let mut r = Rec::new();
    r.nest(0x1f4, |r| {
        r.u32(0x63, 0);
        r.str(0x20a, "meta/meta.dat");
        // The model's properties: name and description in the dictionary SketchUp keeps them.
        r.nest(0x209, |r| {
            let mut d = Dict { name: "ModelProperties".into(), ..Default::default() };
            if !p.description.is_empty() {
                d.values.push(("Description".into(), Tv::Str(p.description.clone())));
            }
            for k in ["IsClassified", "IsDynamic", "IsLive"] {
                d.values.push((k.into(), Tv::Bool(false)));
            }
            if !p.name.is_empty() {
                d.values.push(("Name".into(), Tv::Str(p.name.clone())));
            }
            container(r, &[d], None);
        });
        r.nest(0x200, |r| {
            r.nest(0x61a8, |r| {
                r.nest(0x61a9, |r| {
                    for (name, values) in &p.options {
                        r.nest(0x61aa, |r| {
                            r.str(0x61ab, name);
                            r.nest(0x61ac, |r| {
                                for (k, v) in values {
                                    r.str(0x61ad, k);
                                    typed(r, v, 0);
                                }
                            });
                        });
                    }
                })
            })
        });
        r.nest(0x1f5, |r| r.nest(0x3e8, |r| r.id(0x3e9, p.next_id)));
        r.nest(0x1fa, |r| camera(r, &p.camera));
        r.nest(0x1fb, |r| rendering(r, &p.rendering, &p.ro_hatch));
        r.nest(0x204, |r| shadow(r, &p.shadow));
        r.nest(0x1fc, |r| axes(r, &p.axes));
        r.nest(0x208, |r| {
            r.nest(0x4074, |r| {
                r.nest(0x4075, |r| {
                    for s in &p.line_styles {
                        r.nest(0x4076, |r| {
                            entity(r, s.id, &[], None);
                            r.str(0x4077, s.name);
                            r.str(0x4078, s.pattern);
                            r.f64(0x407a, 1.0);
                            r.f64(0x4079, 1.0);
                            r.color(0x407b, [0, 0, 0, 255]);
                            r.bool(0x407c, false);
                        });
                    }
                })
            })
        });
        r.nest(0x211, |r| hatch_patterns(r, p));
        r.nest(0x1f7, |r| {
            r.nest(0x30d4, |r| {
                r.nest(0x30d5, |r| p.materials.iter().for_each(|m| material(r, m)));
            })
        });
        r.nest(0x1f8, |r| {
            r.nest(0x3a98, |r| {
                r.nest(0x3a99, |r| {
                    for l in &p.layers {
                        r.nest(0x3c8c, |r| {
                            entity(r, l.id, &l.dicts, None);
                            r.str(0x3c8d, &l.name);
                            r.bool(0x3c8e, l.hidden);
                            r.nest(0x3c8f, |r| material(r, &l.material));
                            r.u32(0x3c90, l.page_behavior);
                            if let Some(s) = l.line_style {
                                r.id(0x3c91, s);
                            }
                        });
                    }
                });
                r.id(0x3a9a, p.active_layer);
                r.nest(0x3a9b, |r| folder(r, &p.folders, 0));
            })
        });
        r.nest(0x1fd, |r| {
            r.nest(0x4e20, |r| {
                r.nest(0x4e21, |r| {
                    for f in &p.font_list {
                        r.nest(0x5014, |r| {
                            entity(r, f.id, &[], None);
                            r.str(0x5015, &f.face);
                            r.bool(0x5016, f.bold);
                            r.bool(0x5017, f.italic);
                            r.u32(0x5018, f.size);
                            r.bool(0x5019, f.world);
                            r.f64(0x501a, f.height);
                        });
                    }
                })
            })
        });
        // The text style and the dimension style.
        let t = &p.text_style;
        r.nest(0x1fe, |r| {
            r.nest(0x57e4, |r| {
                r.id(0x57e5, t.font.unwrap_or(p.fonts[1]));
                r.id(0x57e6, t.font2.unwrap_or(p.fonts[1]));
                r.u32(0x57e7, t.v1);
                r.u32(0x57e8, t.v2);
                r.bool(0x57e9, t.b1);
                r.u32(0x57ea, t.v3);
                r.bool(0x57eb, t.b2);
                r.color(0x57ec, t.color);
                r.color(0x57ed, t.color2);
            })
        });
        let d = &p.dim_style;
        r.nest(0x1ff, |r| {
            r.nest(0x5fb4, |r| {
                r.id(0x5fb5, d.font.unwrap_or(p.fonts[1]));
                r.bool(0x5fb6, d.text3d);
                r.bool(0x5fb7, d.radial_prefix);
                r.u32(0x5fb8, d.ext_offset);
                r.u32(0x5fb9, d.ext_overshoot);
                r.u32(0x5fba, d.line_weight);
                r.u32(0x5fbb, d.arrow as u32);
                r.u32(0x5fbc, d.arrow_size);
                r.bool(0x5fbd, d.highlight);
                r.color(0x5fbe, d.highlight_color);
                r.bool(0x5fbf, d.b5fbf);
                r.bool(0x5fc0, d.hide_out_of_plane);
                r.f64(0x5fc1, d.hide_out_of_plane_value);
                r.bool(0x5fc2, d.hide_small);
                r.f64(0x5fc3, d.hide_small_value);
                r.color(0x5fc4, d.color);
                r.color(0x5fc5, d.text_color);
                r.u32(0x5fc6, d.v5fc6);
            })
        });
        match_photos(r, p);
        watermarks(r, p);
        r.nest(0x1f9, |r| {
            r.u32(0x63, 0);
            r.nest(0x1770, |r| {
                r.u32(0x63, 0);
                r.nest(0x1771, |r| {
                    r.u32(0x63, 0);
                    for (d, thumb) in p.defs.iter().zip(thumbnail_paths(p)) {
                        r.nest(0x157c, |r| {
                            entities(r, &d.ents);
                            r.guid(0x157d, &d.guid);
                            r.str(0x157e, &d.name);
                            r.str(0x157f, &d.description);
                            if d.path.is_empty() {
                                r.empty(0x1580);
                            } else {
                                r.str(0x1580, &d.path);
                            }
                            r.bool(0x1582, false);
                            r.u32(0x1583, d.kind);
                            r.u32(0x1581, d.time.unwrap_or(p.time));
                            behavior(r, &d.behavior);
                            // Its thumbnail (0x251c): the camera, and the image as read.
                            r.nest(0x1585, |r| {
                                r.nest(0x251c, |r| {
                                    let cam = match d.thumb_camera {
                                        Some([eye, target, up]) => Camera { eye, target, up, ..thumbnail_camera() },
                                        None => thumbnail_camera(),
                                    };
                                    r.nest(0x251d, |r| camera(r, &cam));
                                    if let Some(path) = &thumb {
                                        r.nest(0x251e, |r| {
                                            r.nest(0x2328, |r| {
                                                r.u32(0x2329, 4);
                                                r.str(0x232a, path);
                                            })
                                        });
                                    }
                                })
                            });
                        });
                    }
                })
            })
        });
        r.nest(0x1f6, |r| entities(r, &p.root));
        r.bool(0x20c, p.model_flags.v20c);
        r.bool(0x214, false);
        r.u32(0x20d, p.model_flags.v20d);
        if !p.styles.list.is_empty() {
            styles(r, p);
        } else {
            default_styles(r, p);
        }
        environments(r, p);
        r.nest(0x213, |r| r.nest(0x7d64, |r| r.empty(0x7d65)));
        r.nest(0x207, |r| {
            r.nest(0x6d60, |r| {
                if p.scenes.is_empty() {
                    r.empty(0x6d61);
                } else {
                    r.nest(0x6d61, |r| {
                        for (s, thumb) in p.scenes.iter().zip(scene_thumbnail_paths(p)) {
                            page(r, s, s.style.unwrap_or(p.style), thumb.as_deref(), &p.ro_hatch);
                        }
                    });
                }
                if let Some(s) = p.selected_scene {
                    r.id(0x6d62, s);
                }
            })
        });
        // The classification schemas (`CClassifications`, 0x6784): the file each came from,
        // its name, its `.skc` in the archive.
        r.nest(0x205, |r| {
            for (s, stored) in p.schemas.iter().zip(schema_paths(p)) {
                r.nest(0x6784, |r| {
                    r.str(0x6785, &s.path);
                    r.str(0x6786, &s.name);
                    r.str(0x6787, &stored);
                });
            }
        });
        r.nest(0x20e, |r| behavior(r, &Behavior::default()));
        r.u32(0x20f, p.model_flags.v20f);
    });
    r.buf
}

/// The styles of the model (0x206 → `CSkpStyleManager`), as read or made in Esquisse.
fn styles(r: &mut Rec, p: &Plan) {
    use crate::native::style::guid_bytes;
    let st = &p.styles;
    let l = style_layout(p);
    let guid = |g: &str| guid_bytes(g).unwrap_or_else(new_guid);
    r.nest(0x206, |r| {
        r.nest(0x6978, |r| {
            r.nest(0x6979, |r| {
                for (s, f) in st.list.iter().zip(&l.folders) {
                    r.nest(0x6b6c, |r| {
                        entity(r, s.id, &[], None);
                        r.guid(0x6b6d, &guid(&s.guid));
                        r.str(0x6b6e, &s.path);
                        r.str(0x6b6f, f);
                        if !s.watermark_ids.is_empty() {
                            r.ids(0x6b70, &s.watermark_ids);
                        }
                    });
                }
            });
            r.id(0x697a, st.selected.and_then(|i| st.list.get(i)).map_or(p.style, |s| s.id));
            r.nest(0x697b, |r| {
                r.nest(0x6b6c, |r| {
                    let act = active_of(st);
                    let a = act.as_ref();
                    r.guid(0x6b6d, &guid(a.map_or("", |a| a.guid.as_str())));
                    r.str(0x6b6e, a.map_or("", |a| a.path.as_str()));
                    r.str(0x6b6f, &l.active_folder);
                })
            });
            r.u8(0x697c, st.changed as u8);
        })
    });
}

/// The model's watermarks (0x203 → `CWatermarkManager` → `CWatermark`): those behind the
/// model, then those over it, and how many are behind.
fn watermarks(r: &mut Rec, p: &Plan) {
    let l = style_layout(p);
    let list: Vec<(&crate::native::style::Wm, Option<&String>)> = p
        .styles
        .watermarks
        .iter()
        .zip(&l.watermark_paths)
        .filter_map(|(w, path)| w.as_ref().map(|w| (w, path.as_ref())))
        .collect();
    let behind = p.styles.watermarks.iter().position(Option::is_none).unwrap_or(0);
    r.nest(0x203, |r| {
        r.nest(0x2cec, |r| {
            if list.is_empty() {
                r.empty(0x2ced);
            } else {
                r.nest(0x2ced, |r| {
                    for (w, path) in &list {
                        r.nest(0x2ee0, |r| {
                            entity(r, w.id, &[], None);
                            r.str(0x2ee1, &w.file);
                            r.bool(0x2ee2, w.found);
                            r.u32(0x2ee3, w.time.clamp(0, u32::MAX as i64) as u32);
                            r.str(0x2ee4, &w.name);
                            r.u32(0x2ee5, w.position);
                            r.nest(0x2ee6, |r| {
                                if let Some(path) = path {
                                    r.nest(0x2328, |r| {
                                        r.u32(0x2329, 4);
                                        r.str(0x232a, path);
                                    });
                                }
                            });
                            r.bool(0x2ee7, w.fitting == 0);
                            r.bool(0x2ee8, w.fitting == 1);
                            r.bool(0x2ee9, w.keep_aspect);
                            r.u32(0x2eee, w.fitting);
                            r.u32(0x2eef, w.keep_aspect as u32);
                            r.bool(0x2eea, w.background);
                            r.f64(0x2eeb, w.scale);
                            r.bool(0x2eec, w.mask);
                            r.f64(0x2eed, w.alpha);
                        });
                    }
                });
            }
            r.u32(0x2cee, behind as u32);
        })
    });
}

/// The style of a model made in Esquisse: SketchUp's default, as the SDK writes it.
fn default_styles(r: &mut Rec, p: &Plan) {
    {
        r.nest(0x206, |r| {
            r.nest(0x6978, |r| {
                r.nest(0x6979, |r| {
                    r.nest(0x6b6c, |r| {
                        entity(r, p.style, &[], None);
                        r.guid(0x6b6d, &new_guid());
                        r.empty(0x6b6e);
                        r.str(0x6b6f, "Style");
                        r.empty(0x6b70);
                    })
                });
                r.u8(0x697a, 1);
                r.nest(0x697b, |r| {
                    r.nest(0x6b6c, |r| {
                        r.guid(0x6b6d, &new_guid());
                        r.empty(0x6b6e);
                        r.str(0x6b6f, "Style_1");
                    })
                });
                r.u8(0x697c, 0);
            })
        });
    }
}
