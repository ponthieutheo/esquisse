//! The fonts of a model (`CFontManager`, `CSkFont`), its text style (`CTextStyle`) and its
//! dimension style (`CDimensionStyle`), and the font of each text and dimension: as both file
//! formats keep them (`docs/skp-format.md` §10.3) and as Esquisse keeps them.
//!
//! Esquisse: `view_ext["skp_fonts"]` (the model's list, as read), `view_ext["skp_text_style"]`,
//! `view_ext["skp_dim_style"]` (each font by its rank in the list), and what the application
//! shows of them in Model Info (`view_ext["TextStyle"]`, `view_ext["DimensionStyle"]`, the
//! application's `TextDefaults` and `DimDefaults`); an annotation has its font in its own
//! properties (`font_name`, `font_size`, `font_bold`, `font_italic`, and `font_height` for a
//! font of the model's size). Writing takes the list as read, adds the fonts the annotations
//! and Model Info use, and the styles as read with what Model Info changed.

use std::collections::HashMap;

use esq_core::model::{AttrValue, Model};
use serde_json::{Value, json};

use super::doc::{DimStyle, Doc, Font, Id, TextStyle};

const IN: f64 = 25.4;

fn rgb(c: [u8; 4]) -> Value {
    json!([c[0], c[1], c[2]])
}

fn font_json(f: Option<&Font>, color: [u8; 4]) -> Value {
    let f = f.cloned().unwrap_or(Font { face: "Tahoma".into(), size: 10, ..Default::default() });
    json!({"name": f.face, "size": f.size as f64, "bold": f.bold, "italic": f.italic, "color": rgb(color)})
}

/// The properties an annotation gets from its font.
pub fn props(f: &Font) -> Vec<(&'static str, AttrValue)> {
    let mut v = vec![
        ("font_name", AttrValue::Text(f.face.clone())),
        ("font_size", AttrValue::Number(f.size as f64)),
        ("font_bold", AttrValue::Bool(f.bold)),
        ("font_italic", AttrValue::Bool(f.italic)),
    ];
    if f.world {
        v.push(("font_height", AttrValue::Number(f.height * IN)));
    }
    v
}

/// The fonts, styles and Model Info of a document.
pub fn to_view_ext(doc: &Doc, m: &mut Model) {
    let rank: HashMap<Id, usize> = doc.fonts.iter().enumerate().map(|(i, f)| (f.id, i)).collect();
    let by = |id: Option<Id>| id.and_then(|i| rank.get(&i)).map(|&k| k as Id);
    let font = |id: Option<Id>| id.and_then(|i| rank.get(&i)).and_then(|&k| doc.fonts.get(k));
    if !doc.fonts.is_empty() {
        let list: Vec<Font> = doc.fonts.iter().enumerate().map(|(i, f)| Font { id: i as Id, ..f.clone() }).collect();
        m.view_ext.insert("skp_fonts".into(), serde_json::to_value(list).unwrap_or(Value::Null));
    }
    if let Some(t) = &doc.text_style {
        let s = TextStyle { font: by(t.font), font2: by(t.font2), ..t.clone() };
        m.view_ext.insert("skp_text_style".into(), serde_json::to_value(&s).unwrap_or(Value::Null));
        m.view_ext.insert(
            "TextStyle".into(),
            json!({
                "leader": font_json(font(t.font), t.color),
                "screen": font_json(font(t.font2).or(font(t.font)), t.color2),
                "arrow": t.v1.min(4), "leader_type": t.v2.clamp(1, 2),
            }),
        );
    }
    if let Some(d) = &doc.dim_style {
        let s = DimStyle { font: by(d.font), ..d.clone() };
        m.view_ext.insert("skp_dim_style".into(), serde_json::to_value(&s).unwrap_or(Value::Null));
        m.view_ext.insert(
            "DimensionStyle".into(),
            json!({
                "font": font_json(font(d.font), d.text_color),
                "arrow": d.arrow.clamp(0, 4), "show_prefix": d.radial_prefix,
                "hide_foreshortened": d.hide_out_of_plane, "hide_small": d.hide_small,
                "highlight_nonassociated": d.highlight, "highlight_color": rgb(d.highlight_color),
            }),
        );
    }
}

/// What the writer writes: the fonts (ids from 0, the caller numbers them), the text style
/// and the dimension style (fonts by their rank), and the rank of the font of a set of
/// annotation properties.
#[derive(Clone, Debug, Default)]
pub struct Fonts {
    pub list: Vec<Font>,
    pub text_style: TextStyle,
    pub dim_style: DimStyle,
}

impl Fonts {
    /// The rank of this font in the list, added if it is not there.
    pub fn rank(&mut self, f: &Font) -> usize {
        let same = |x: &Font| {
            x.face == f.face && x.size == f.size && x.bold == f.bold && x.italic == f.italic && x.world == f.world
        };
        if let Some(i) = self.list.iter().position(same) {
            return i;
        }
        self.list.push(Font { id: self.list.len() as Id, ..f.clone() });
        self.list.len() - 1
    }

    /// The font of an annotation's properties (`None`: it has none of its own).
    pub fn of_props(&mut self, get: &dyn Fn(&str) -> Option<AttrValue>) -> Option<usize> {
        let AttrValue::Text(face) = get("font_name")? else { return None };
        let num = |k: &str| match get(k) {
            Some(AttrValue::Number(x)) if x.is_finite() => Some(x),
            _ => None,
        };
        let flag = |k: &str| matches!(get(k), Some(AttrValue::Bool(true)));
        let height = num("font_height");
        let f = Font {
            id: 0,
            face,
            bold: flag("font_bold"),
            italic: flag("font_italic"),
            size: num("font_size").map_or(10, |s| s.round().clamp(1.0, 10_000.0) as u32),
            world: height.is_some(),
            height: height.map_or(1.0, |h| h / IN),
        };
        Some(self.rank(&f))
    }
}

fn app_font(v: &Value) -> Option<(Font, [u8; 3])> {
    let c = v["color"].as_array();
    let ch = |i: usize| c.and_then(|a| a.get(i)).and_then(Value::as_u64).unwrap_or(0).min(255) as u8;
    Some((
        Font {
            id: 0,
            face: v["name"].as_str()?.to_string(),
            bold: v["bold"].as_bool().unwrap_or(false),
            italic: v["italic"].as_bool().unwrap_or(false),
            size: v["size"].as_f64().map_or(10, |s| s.round().clamp(1.0, 10_000.0) as u32),
            world: false,
            height: 1.0,
        },
        [ch(0), ch(1), ch(2)],
    ))
}

/// A colour with the alpha of `like` when the colours are the same.
fn with_rgb(like: [u8; 4], c: [u8; 3]) -> [u8; 4] {
    if like[..3] == c { like } else { [c[0], c[1], c[2], 255] }
}

/// The fonts and styles to write: as read, else those of a new model (Arial and Tahoma, as
/// the SDK writes them), with what Model Info sets.
pub fn of_model(m: &Model) -> Fonts {
    let list: Vec<Font> =
        m.view_ext.get("skp_fonts").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let read = !list.is_empty();
    let mut f = Fonts { list, ..Default::default() };
    if !read {
        for face in ["Arial", "Tahoma"] {
            f.rank(&Font { face: face.into(), size: 12, height: 1.0, ..Default::default() });
        }
    }
    f.text_style =
        m.view_ext.get("skp_text_style").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(TextStyle {
            font: Some(1),
            font2: Some(1),
            v1: 3,
            v2: 1,
            b1: false,
            v3: 2,
            b2: true,
            color: [0, 0, 0, 255],
            color2: [0, 0, 0, 255],
        });
    f.dim_style =
        m.view_ext.get("skp_dim_style").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(DimStyle {
            lead: 0,
            font: Some(0),
            text3d: false,
            radial_prefix: true,
            ext_offset: 5,
            ext_overshoot: 10,
            line_weight: 1,
            arrow: 3,
            arrow_size: 10,
            highlight: false,
            highlight_color: [255, 0, 0, 255],
            b5fbf: true,
            hide_out_of_plane: false,
            hide_out_of_plane_value: 0.6,
            hide_small: false,
            hide_small_value: 10.0,
            color: [64, 64, 64, 255],
            text_color: [0, 0, 0, 0],
            v5fc6: 1,
        });
    // Model Info › Text and › Dimensions, when they differ from what was read.
    if let Some(t) = m.view_ext.get("TextStyle") {
        let ts = f.text_style.clone();
        if let Some((font, c)) = app_font(&t["leader"]) {
            let same = ts.font.and_then(|i| f.list.get(i as usize)).is_some_and(|x| same_font(x, &font));
            if !same {
                f.text_style.font = Some(f.rank(&font) as Id);
            }
            f.text_style.color = with_rgb(ts.color, c);
        }
        if let Some((font, c)) = app_font(&t["screen"]) {
            let same = ts.font2.and_then(|i| f.list.get(i as usize)).is_some_and(|x| same_font(x, &font));
            if !same {
                f.text_style.font2 = Some(f.rank(&font) as Id);
            }
            f.text_style.color2 = with_rgb(ts.color2, c);
        }
        if let Some(a) = t["arrow"].as_u64() {
            f.text_style.v1 = a as u32;
        }
        if let Some(l) = t["leader_type"].as_u64() {
            f.text_style.v2 = l as u32;
        }
    }
    if let Some(d) = m.view_ext.get("DimensionStyle") {
        let ds = f.dim_style.clone();
        if let Some((font, c)) = app_font(&d["font"]) {
            let same = ds.font.and_then(|i| f.list.get(i as usize)).is_some_and(|x| same_font(x, &font));
            if !same {
                f.dim_style.font = Some(f.rank(&font) as Id);
            }
            f.dim_style.text_color = with_rgb(ds.text_color, c);
        }
        let b = |k: &str, v: &mut bool| {
            if let Some(x) = d[k].as_bool() {
                *v = x;
            }
        };
        b("show_prefix", &mut f.dim_style.radial_prefix);
        b("hide_foreshortened", &mut f.dim_style.hide_out_of_plane);
        b("hide_small", &mut f.dim_style.hide_small);
        b("highlight_nonassociated", &mut f.dim_style.highlight);
        if let Some(a) = d["arrow"].as_i64() {
            f.dim_style.arrow = a as i32;
        }
        if let Some(c) = d["highlight_color"].as_array() {
            let ch = |i: usize| c.get(i).and_then(Value::as_u64).unwrap_or(0).min(255) as u8;
            f.dim_style.highlight_color = with_rgb(ds.highlight_color, [ch(0), ch(1), ch(2)]);
        }
    }
    f
}

fn same_font(a: &Font, b: &Font) -> bool {
    a.face == b.face && a.size == b.size && a.bold == b.bold && a.italic == b.italic
}
