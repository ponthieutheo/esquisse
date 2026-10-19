//! The XML files of the 2021+ archive, as SketchUp writes them: a material
//! (`materials/<folder>/material.xml`, for its dictionaries) and a style
//! (`styles/<name>/style.xml`). Numbers as C's `%.17g` (doubles) and `%.8g` (the single
//! precision factors).

use std::fmt::Write;

use super::plan::{Dict, Mat, Tex, Tv};

/// C's `%.<p>g`.
pub fn g(x: f64, p: usize) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    if !x.is_finite() {
        return if x.is_nan() {
            "nan".into()
        } else if x > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    let p = p.max(1);
    let e = format!("{:.*e}", p - 1, x);
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let strip = |s: String| -> String {
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
    };
    if exp < -4 || exp >= p as i32 {
        let m = strip(mant.to_string());
        format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        strip(format!("{:.*}", decimals, x))
    }
}

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => o.push(c),
        }
    }
    o
}

fn cdata(s: &str) -> String {
    format!("<![CDATA[{}]]>", s.replace("]]>", "]]]]><![CDATA[>"))
}

/// A colour as SketchUp writes it in XML: 0xAABBGGRR, in decimal.
fn abgr(c: [u8; 4]) -> u32 {
    u32::from_le_bytes(c)
}

fn value(out: &mut String, v: &Tv, ind: &str, depth: usize) {
    match v {
        Tv::Byte(x) => {
            let _ = write!(out, "{x}");
        }
        Tv::Short(x) => {
            let _ = write!(out, "{x}");
        }
        Tv::Int(x) => {
            let _ = write!(out, "{x}");
        }
        Tv::Float(x) => out.push_str(&g(*x as f64, 9)),
        Tv::Double(x) => out.push_str(&g(*x, 17)),
        Tv::Bool(b) => out.push(if *b { '1' } else { '0' }),
        Tv::Color(c) => {
            let _ = write!(out, "{}", abgr(*c));
        }
        Tv::Time(t) => {
            let _ = write!(out, "{t}");
        }
        Tv::Str(s) => out.push_str(&cdata(s)),
        Tv::Array(items) => {
            out.push('\n');
            if depth < 16 {
                for it in items {
                    let _ = write!(out, "{ind}  <value type=\"{}\">", it.variant());
                    value(out, it, &format!("{ind}  "), depth + 1);
                    out.push_str("</value>\n");
                }
            }
            out.push_str(ind);
        }
        Tv::Point(p) | Tv::Vector(p) => {
            let name = if matches!(v, Tv::Point(_)) { "point" } else { "vector" };
            let _ = write!(
                out,
                "\n{ind}  <{name} x=\"{}\" y=\"{}\" z=\"{}\"/>\n{ind}",
                g(p[0], 17),
                g(p[1], 17),
                g(p[2], 17)
            );
        }
    }
}

/// The dictionaries of a material (`<n0:AttributeDictionaries>`).
fn dictionaries(out: &mut String, dicts: &[Dict]) {
    let _ = writeln!(out, "    <n0:AttributeDictionaries count=\"{}\">", dicts.len());
    for d in dicts {
        let mut kv: Vec<&(String, Tv)> = d.values.iter().collect();
        kv.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        let _ = writeln!(out, "      <n0:AttributeDictionary name=\"{}\" count=\"{}\">", esc(&d.name), kv.len());
        for (k, v) in kv {
            let _ = write!(out, "        <n0:Attribute key=\"{}\" type=\"{}\"", esc(k), v.variant());
            if let Tv::Array(a) = v {
                let _ = write!(out, " count=\"{}\"", a.len());
            }
            out.push('>');
            value(out, v, "        ", 0);
            out.push_str("</n0:Attribute>\n");
        }
        out.push_str("      </n0:AttributeDictionary>\n");
    }
    out.push_str("    </n0:AttributeDictionaries>\n");
}

fn image_ref(out: &mut String, tag: &str, id: usize, t: &Tex) {
    let _ =
        writeln!(out, "      <mat:{tag} id=\"{id}\" path=\"./{}\" file_name=\"{}\"/>", esc(&t.stored), esc(&t.stored));
}

/// `material.xml`.
pub fn material_xml(m: &Mat) -> String {
    let mut o = String::new();
    o.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n");
    o.push_str("<materialDocument xmlns=\"http://sketchup.google.com/schemas/sketchup/1.0/material\" xmlns:mat=\"http://sketchup.google.com/schemas/sketchup/1.0/material\">\n");
    let _ = write!(
        o,
        "  <mat:material name=\"{}\" type=\"{}\" workflow=\"{}\" colorRed=\"{}\" colorGreen=\"{}\" colorBlue=\"{}\" colorizeType=\"{}\" trans=\"{}\" useTrans=\"{}\" pbrPromoState=\"{}\" hasTexture=\"{}\"",
        esc(&m.name),
        m.kind,
        m.workflow,
        m.color[0],
        m.color[1],
        m.color[2],
        m.colorize_type,
        g(m.trans, 17),
        m.use_trans as u8,
        if m.of_layer { 2 } else { 0 },
        m.texture.is_some() as u8
    );
    if !m.dicts.is_empty() {
        o.push_str(" xmlns:n0=\"http://sketchup.google.com/schemas/1.0/types\"");
    }
    o.push_str(">\n");
    let mut scale = [1.0, 1.0];
    if let Some(t) = &m.texture {
        scale = t.size;
        let avg = abgr([t.avg[0], t.avg[1], t.avg[2], 255]);
        let _ = writeln!(
            o,
            "    <mat:texture textureFilename=\"{}\" xScale=\"{}\" yScale=\"{}\" avgColor=\"{avg}\">",
            esc(&t.file_name),
            g(t.size[0], 17),
            g(t.size[1], 17)
        );
        o.push_str("      <mat:images>\n");
        let _ =
            writeln!(o, "        <mat:image id=\"1\" path=\"./{}\" file_name=\"{}\"/>", esc(&t.stored), esc(&t.stored));
        o.push_str("      </mat:images>\n");
        o.push_str("    </mat:texture>\n");
    }
    let p = &m.pbr;
    let _ = writeln!(o, "    <mat:pbrMR xScale=\"{}\" yScale=\"{}\">", g(scale[0], 17), g(scale[1], 17));
    let flag = |o: &mut String, tag: &str, b: bool| {
        let _ = writeln!(o, "      <mat:{tag}>{}</mat:{tag}>", b as u8);
    };
    flag(&mut o, "enable_metalness", p.metalness);
    flag(&mut o, "enable_roughness", p.roughness);
    flag(&mut o, "roughness_texture_invert", p.roughness_inverted);
    flag(&mut o, "enable_normal", p.normal);
    flag(&mut o, "enable_occlusion", p.ao);
    let mut id = 1;
    for (tag, t) in [
        ("metallic_texture", &p.metallic_texture),
        ("roughness_texture", &p.roughness_texture),
        ("normal_texture", &p.normal_texture),
        ("occlusion_texture", &p.ao_texture),
    ] {
        if let Some(t) = t {
            id += 1;
            image_ref(&mut o, tag, id, t);
        }
    }
    let num = |o: &mut String, tag: &str, x: f32| {
        let _ = writeln!(o, "      <mat:{tag}>{}</mat:{tag}>", g(x as f64, 8));
    };
    num(&mut o, "metallicFactor", p.metallic);
    num(&mut o, "roughnessFactor", p.roughness_factor);
    let _ = writeln!(o, "      <mat:normalMapStyle>{}</mat:normalMapStyle>", p.normal_style);
    num(&mut o, "normalScale", p.normal_scale);
    num(&mut o, "occlusionStrength", p.ao_strength);
    o.push_str("      <mat:baseColorFactor>4294967295</mat:baseColorFactor>\n");
    o.push_str("    </mat:pbrMR>\n");
    if !m.dicts.is_empty() {
        dictionaries(&mut o, &m.dicts);
    }
    o.push_str("  </mat:material>\n");
    o.push_str("</materialDocument>\n");
    o
}

/// The settings of a new style by item (`<sty:item id>`): variant type and value, as the SDK
/// writes the styles of every model (it does not carry the model's rendering options into
/// them; the model and its scenes keep their own).
pub(crate) const STYLE_ITEMS: &[(u32, u8, &str)] = &[
    (1000, 4, "1"),
    (1001, 4, "0"),
    (1002, 4, "1"),
    (1004, 1, "0"),
    (1005, 4, "8"),
    (1006, 1, "1"),
    (1007, 4, "3"),
    (1008, 1, "0"),
    (1009, 4, "4"),
    (1010, 1, "0"),
    (1011, 4, "5"),
    (1012, 1, "0"),
    (1014, 4, "-16777216"),
    (1015, 1, "0"),
    (1016, 1, "1"),
    (2001, 4, "2"),
    (2002, 4, "-3612191"),
    (2003, 4, "-3637120"),
    (2004, 1, "0"),
    (2005, 1, "1"),
    (2006, 4, "0"),
    (2007, 1, "1"),
    (2008, 7, "0.65000000000000002"),
    (4000, 4, "-1"),
    (4001, 4, "-23156"),
    (4002, 1, "0"),
    (4003, 4, "-12610753"),
    (4004, 1, "0"),
    (4005, 4, "50"),
    (4006, 1, "1"),
    (4007, 4, "0"),
    (5000, 1, "1"),
    (5001, 13, ""),
    (7000, 4, "-16711681"),
    (7001, 4, "-16776961"),
    (7002, 4, "-8355712"),
    (7003, 4, "-7508378"),
    (7004, 4, "-4144960"),
    (7005, 4, "-16776961"),
    (7008, 1, "1"),
    (7010, 1, "0"),
    (7011, 1, "0"),
    (7012, 1, "0"),
    (7013, 4, "3"),
    (7014, 4, "4"),
    (7015, 1, "0"),
    (7016, 4, "-16777216"),
    (7017, 1, "0"),
    (7018, 1, "0"),
    (8000, 1, "1"),
    (8001, 7, "1"),
    (8002, 1, "1"),
    (8003, 7, "0.80000000000000004"),
    (8100, 1, "0"),
    (8102, 6, "10"),
    (8103, 6, "0"),
    (8105, 1, "0"),
    (8106, 4, "-16777216"),
    (8107, 6, "1"),
];

/// The properties of a new style in an MFC archive (`CSkpStyle`): the same items, without the
/// ambient occlusion of SketchUp 2025 (key, variant type, value).
pub fn style_items_legacy() -> impl Iterator<Item = (u32, u8, &'static str)> {
    STYLE_ITEMS.iter().copied().filter(|(id, _, _)| *id < 8100)
}

/// The rendering option each item of a style takes its value from (which
/// fills a style from rendering options; the options by the records of 2021+ files). 1014 and
/// 4000 are the foreground and background colours with their system values resolved; 7010
/// is « hidden geometry or hidden objects ».
pub const STYLE_FROM_RENDERING: &[(u32, u16)] = &[
    (1000, 0x7341),
    (1001, 0x7342),
    (1002, 0x7348),
    (1004, 0x734d),
    (1005, 0x734e),
    (1006, 0x734f),
    (1007, 0x7350),
    (1008, 0x7351),
    (1009, 0x7352),
    (1010, 0x7353),
    (1011, 0x7354),
    (1012, 0x734b),
    (1014, 0x7358),
    (1015, 0x7356),
    (1016, 0x734c),
    (2001, 0x733d),
    (2002, 0x735c),
    (2003, 0x735d),
    (2004, 0x733e),
    (2005, 0x733f),
    (2006, 0x7377),
    (2007, 0x7340),
    (2008, 0x7378),
    (4000, 0x7357),
    (4001, 0x7365),
    (4002, 0x7368),
    (4003, 0x7367),
    (4004, 0x7369),
    (4005, 0x736b),
    (4006, 0x736a),
    (4007, 0x7366),
    (5000, 0x735e),
    (7000, 0x7359),
    (7001, 0x735a),
    (7002, 0x735b),
    (7003, 0x7370),
    (7004, 0x7371),
    (7005, 0x7372),
    (7008, 0x7343),
    (7011, 0x7347),
    (7012, 0x7346),
    (7013, 0x7375),
    (7014, 0x7374),
    (7015, 0x7376),
    (7016, 0x7373),
    (7017, 0x7380),
    (7018, 0x7381),
    (8000, 0x737c),
    (8001, 0x737d),
    (8002, 0x737e),
    (8003, 0x737f),
];

/// `style.xml` of the model's style.
pub fn style_xml(name: &str) -> String {
    let mut o = String::new();
    o.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n");
    o.push_str("<styleDocument xmlns=\"http://sketchup.google.com/schemas/sketchup/1.0/style\" xmlns:sty=\"http://sketchup.google.com/schemas/sketchup/1.0/style\">\n");
    let _ = writeln!(
        o,
        "  <sty:style xmlns:t=\"http://sketchup.google.com/schemas/1.0/types\" name=\"{}\" desc=\"\">",
        esc(name.trim_end_matches("_1"))
    );
    for &(id, ty, v) in STYLE_ITEMS {
        let _ = writeln!(o, "    <sty:item id=\"{id}\">");
        if ty == 13 {
            o.push_str(
                "      <t:variant type=\"13\" xmlns:n0=\"http://sketchup.google.com/schemas/sketchup/1.0/wmlist\">\n",
            );
            o.push_str("        <n0:wmlist count=\"0\"/>\n");
            o.push_str("      </t:variant>\n");
        } else {
            let _ = writeln!(o, "      <t:variant type=\"{ty}\">{v}</t:variant>");
        }
        o.push_str("    </sty:item>\n");
    }
    o.push_str("  </sty:style>\n");
    o.push_str("</styleDocument>\n");
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_as_c_writes_them() {
        // 500 mm in inches.
        assert_eq!(g(500.0 / 25.4, 17), "19.685039370078741");
        assert_eq!(g(0.6f32 as f64 * 0.0 + (1.0 - 0.4f32 as f64), 17), "0.59999999403953552");
        assert_eq!(g(10.0, 17), "10");
        assert_eq!(g(0.5, 17), "0.5");
        assert_eq!(g(0.8f32 as f64, 8), "0.80000001");
        assert_eq!(g(0.3f32 as f64, 8), "0.30000001");
        assert_eq!(g(1.5, 8), "1.5");
        assert_eq!(g(0.65, 17), "0.65000000000000002");
        assert_eq!(g(1e30, 17), "1e+30");
        assert_eq!(g(1e-7, 6), "1e-07");
    }
}
