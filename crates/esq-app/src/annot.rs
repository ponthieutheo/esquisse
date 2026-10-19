//! Dimensions and texts: their label, how they are drawn over the 3D view, and hit testing.
//! Their look (font, colour, end points, leader, alignment) comes from Window › Model Info ›
//! Text and Dimensions, or from the annotation itself once "Update selected" gave it its own
//! (the same per-entity properties the Ruby API's `Text#font`, `#arrow_type`… use).

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Align2, Color32, FontFamily, FontId, Pos2, Stroke};
use esq_core::glam::{DVec2, DVec3};
use esq_core::model::{AnnotId, AnnotKind, Annotation, AttrValue, Entities, EntityRef, Model, Units};
use esq_core::units::format_length;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::camera::Camera;
use crate::pick::dist_point_segment_2d;

pub const INK: Color32 = Color32::from_rgb(20, 20, 20);
pub const FONT: f32 = 13.0;
/// SketchUp's font sizes are in points; the view's are in pixels at 96 dpi.
const PX_PER_PT: f64 = 96.0 / 72.0;

/// End points of leaders and dimension lines (`arrow_type` of the Ruby API).
pub const ARROWS: [&str; 5] = ["Aucune", "Barre", "Point", "Flèche fermée", "Flèche ouverte"];

/// What the annotation shows: its text, or for a dimension without one, the length.
pub fn label(a: &Annotation, units: Units) -> String {
    match &a.kind {
        AnnotKind::Linear { a: p, b: q, .. } if a.text.is_empty() => format_length(p.distance(*q), units),
        _ => a.text.clone(),
    }
}

/// A font as Model Info › Text and Dimensions choose it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextFont {
    pub name: String,
    /// Points.
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub color: [u8; 3],
}

impl Default for TextFont {
    fn default() -> Self {
        TextFont { name: "Tahoma".into(), size: 10.0, bold: false, italic: false, color: [0, 0, 0] }
    }
}

impl TextFont {
    /// "Tahoma : 10 points", as the pages show it.
    pub fn describe(&self) -> String {
        let mut s = self.name.clone();
        if self.bold {
            s += ", gras";
        }
        if self.italic {
            s += ", italique";
        }
        format!("{s} : {} points", format!("{}", self.size).replace('.', ","))
    }
}

/// Model Info › Text: the look of new texts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextDefaults {
    pub screen: TextFont,
    pub leader: TextFont,
    /// End point of the leader (index in `ARROWS`, never 1).
    pub arrow: u8,
    /// 1 view based, 2 pushpin.
    pub leader_type: u8,
}

impl Default for TextDefaults {
    fn default() -> Self {
        TextDefaults { screen: TextFont::default(), leader: TextFont::default(), arrow: 2, leader_type: 1 }
    }
}

/// Model Info › Dimensions: the look of new dimensions, and the expert settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DimDefaults {
    pub font: TextFont,
    pub arrow: u8,
    /// Align to screen (true) or to the dimension line.
    pub align_screen: bool,
    /// Along the dimension line: 0 above, 1 centred, 2 outside.
    pub position: u8,
    pub show_prefix: bool,
    pub hide_foreshortened: bool,
    pub hide_small: bool,
    pub highlight_nonassociated: bool,
    pub highlight_color: [u8; 3],
}

impl Default for DimDefaults {
    fn default() -> Self {
        DimDefaults {
            font: TextFont::default(),
            arrow: 3,
            align_screen: false,
            position: 0,
            show_prefix: true,
            hide_foreshortened: true,
            hide_small: true,
            highlight_nonassociated: true,
            highlight_color: [255, 0, 0],
        }
    }
}

const TEXT_KEY: &str = "TextStyle";
const DIM_KEY: &str = "DimensionStyle";

/// The model's text defaults (kept with the model, in `view_ext`).
pub fn text_defaults(m: &Model) -> TextDefaults {
    m.view_ext.get(TEXT_KEY).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

pub fn set_text_defaults(m: &mut Model, d: &TextDefaults) {
    m.view_ext.insert(TEXT_KEY.into(), serde_json::to_value(d).unwrap_or_default());
}

pub fn dim_defaults(m: &Model) -> DimDefaults {
    m.view_ext.get(DIM_KEY).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

pub fn set_dim_defaults(m: &mut Model, d: &DimDefaults) {
    m.view_ext.insert(DIM_KEY.into(), serde_json::to_value(d).unwrap_or_default());
}

/// How one annotation is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct AnnotStyle {
    pub font: TextFont,
    pub arrow: u8,
    pub leader_type: u8,
    pub align_screen: bool,
    /// Text aligned to the dimension line: 0 above, 1 centred, 2 outside (Model Info;
    /// Ruby's `aligned_text_position`).
    pub position: u8,
    /// Along the dimension line: 0 outside the start, 1 centred, 2 outside the end
    /// (right-click › Text Position; Ruby's `text_position`).
    pub along: u8,
    /// A text whose leader is hidden (right-click › Leader › Hidden, `display_leader`).
    pub leader_hidden: bool,
    /// A radial dimension: `Some(true)` when it shows the diameter.
    pub radial: Option<bool>,
    /// "Show radius/diameter prefix" of Model Info › Dimensions.
    pub prefix: bool,
    /// Dimensions: hidden when seen too foreshortened, or too small for their text.
    pub hide_foreshortened: bool,
    pub hide_small: bool,
    /// A dimension no longer attached to the geometry, shown in this colour.
    pub highlight: Option<[u8; 3]>,
}

impl Default for AnnotStyle {
    fn default() -> Self {
        AnnotStyle {
            font: TextFont::default(),
            arrow: 3,
            leader_type: 2,
            align_screen: true,
            position: 0,
            along: 1,
            leader_hidden: false,
            radial: None,
            prefix: true,
            hide_foreshortened: false,
            hide_small: false,
            highlight: None,
        }
    }
}

/// The label as the view shows it: a radial dimension gets its prefix ("R", "DIA ") and,
/// for a diameter, twice the radius.
pub fn label_styled(a: &Annotation, st: &AnnotStyle, units: Units) -> String {
    match (&a.kind, st.radial) {
        (AnnotKind::Linear { a: p, b: q, .. }, Some(dia)) if a.text.is_empty() => {
            let r = p.distance(*q);
            let (pre, len) = if dia { ("DIA ", 2.0 * r) } else { ("R", r) };
            format!("{}{}", if st.prefix { pre } else { "" }, format_length(len, units))
        }
        _ => label(a, units),
    }
}

/// Key of an annotation in `Entities::props` (as the Ruby bridge writes it).
pub fn prop_key(id: AnnotId) -> String {
    format!("a{}", slotmap::Key::data(&id).as_ffi())
}

fn prop<'a>(ents: &'a Entities, id: AnnotId, name: &str) -> Option<&'a AttrValue> {
    ents.prop(&prop_key(id), name)
}

fn num(v: Option<&AttrValue>) -> Option<f64> {
    match v? {
        AttrValue::Number(n) => Some(*n),
        AttrValue::Json(j) => j.as_f64(),
        _ => None,
    }
}

fn flag(v: Option<&AttrValue>) -> Option<bool> {
    match v? {
        AttrValue::Bool(b) => Some(*b),
        AttrValue::Json(j) => j.as_bool(),
        _ => None,
    }
}

fn color(v: Option<&AttrValue>) -> Option<[u8; 3]> {
    let AttrValue::Json(Value::Array(a)) = v? else { return None };
    let c: Vec<u8> = a.iter().filter_map(|x| x.as_u64()).map(|x| x.min(255) as u8).collect();
    (c.len() >= 3).then(|| [c[0], c[1], c[2]])
}

/// The style a new annotation gets: the model's defaults (Model Info › Text, Dimensions).
pub fn default_style(m: &Model, a: &Annotation) -> AnnotStyle {
    let d = dim_defaults(m);
    let t = text_defaults(m);
    let (font, arrow, leader_type) = match a.kind {
        AnnotKind::Linear { .. } => (d.font.clone(), d.arrow, 0),
        AnnotKind::Text { .. } => (t.leader.clone(), t.arrow, t.leader_type),
        AnnotKind::ScreenText { .. } => (t.screen.clone(), 0, 0),
    };
    let is_dim = matches!(a.kind, AnnotKind::Linear { .. });
    AnnotStyle {
        font,
        arrow,
        leader_type,
        align_screen: d.align_screen,
        position: d.position,
        along: 1,
        leader_hidden: false,
        radial: None,
        prefix: d.show_prefix,
        hide_foreshortened: is_dim && d.hide_foreshortened,
        hide_small: is_dim && d.hide_small,
        highlight: None,
    }
}

/// The style of an annotation: its own properties, else the model's defaults.
pub fn style_of(m: &Model, ents: &Entities, id: AnnotId) -> AnnotStyle {
    let Some(a) = ents.annots.get(id) else { return AnnotStyle::default() };
    let mut st = default_style(m, a);
    let f = &mut st.font;
    if let Some(AttrValue::Text(n)) = prop(ents, id, "font_name") {
        f.name = n.clone();
    }
    f.size = num(prop(ents, id, "font_size")).unwrap_or(f.size);
    f.bold = flag(prop(ents, id, "font_bold")).unwrap_or(f.bold);
    f.italic = flag(prop(ents, id, "font_italic")).unwrap_or(f.italic);
    f.color = color(prop(ents, id, "color")).unwrap_or(f.color);
    st.arrow = num(prop(ents, id, "arrow_type")).map_or(st.arrow, |v| v as u8);
    st.leader_type = num(prop(ents, id, "leader_type")).map_or(st.leader_type, |v| v as u8);
    st.align_screen = flag(prop(ents, id, "aligned")).map_or(st.align_screen, |aligned| !aligned);
    st.position = num(prop(ents, id, "aligned_text_position")).map_or(st.position, |v| v as u8);
    st.along = num(prop(ents, id, "text_position")).map_or(st.along, |v| v as u8);
    st.leader_hidden = flag(prop(ents, id, "display_leader")) == Some(false);
    if prop(ents, id, "radial").is_some() {
        st.radial = Some(flag(prop(ents, id, "diameter")).unwrap_or(false));
    }
    let d = dim_defaults(m);
    if matches!(a.kind, AnnotKind::Linear { .. }) && d.highlight_nonassociated && !associated(a) {
        st.highlight = Some(d.highlight_color);
    }
    st
}

/// Writes a style into an annotation's own properties ("Update selected text/dimensions").
pub fn apply_style(ents: &mut Entities, id: AnnotId, font: &TextFont, arrow: u8, extra: &[(&str, AttrValue)]) {
    let k = prop_key(id);
    ents.set_prop(&k, "font_name", AttrValue::Text(font.name.clone()));
    ents.set_prop(&k, "font_size", AttrValue::Number(font.size));
    ents.set_prop(&k, "font_bold", AttrValue::Bool(font.bold));
    ents.set_prop(&k, "font_italic", AttrValue::Bool(font.italic));
    ents.set_prop(&k, "color", AttrValue::Json(serde_json::json!(font.color)));
    ents.set_prop(&k, "arrow_type", AttrValue::Number(arrow as f64));
    for (name, v) in extra {
        ents.set_prop(&k, name, v.clone());
    }
}

/// A dimension is associated when both its ends are attached to the model (`attach.rs`).
fn associated(a: &Annotation) -> bool {
    !matches!(a.kind, AnnotKind::Linear { .. }) || (a.attach[0].is_some() && a.attach[1].is_some())
}

/// Like `scene::annotations`, with each one's style.
pub fn styled_annotations(
    model: &Model,
    ctx: &[esq_core::model::InstanceId],
    sel: &[EntityRef],
) -> Vec<(Annotation, bool, AnnotStyle)> {
    let sel: HashSet<EntityRef> = sel.iter().copied().collect();
    let mut out = Vec::new();
    esq_core::walk::visit(model, |p| {
        let direct = p.path.as_slice() == ctx;
        for (id, a) in p.ents.annots.iter() {
            if a.hidden || !model.layers.get(a.layer).is_none_or(|x| x.visible) {
                continue;
            }
            let st = style_of(model, p.ents, id);
            out.push((a.transformed(&p.xform), direct && sel.contains(&EntityRef::Annot(id)), st));
        }
    });
    out
}

fn screen(cam: &Camera, p: DVec3) -> Option<DVec2> {
    cam.project(p)
}

/// Screen-space pieces of an annotation: line segments and where the label is centred.
pub struct Layout {
    pub segs: Vec<(DVec2, DVec2)>,
    pub arrows: Vec<(DVec2, DVec2)>,
    pub label: DVec2,
    /// Half size of the label box, in pixels.
    pub half: DVec2,
    /// Angle of the label (radians, screen), for text aligned to a dimension line.
    pub angle: f64,
}

pub fn layout(cam: &Camera, a: &Annotation, text: &str) -> Option<Layout> {
    layout_styled(cam, a, text, &AnnotStyle::default())
}

pub fn layout_styled(cam: &Camera, a: &Annotation, text: &str, st: &AnnotStyle) -> Option<Layout> {
    let chars = text.lines().map(|l| l.chars().count()).max().unwrap_or(1).max(1) as f64;
    let lines = text.lines().count().max(1) as f64;
    let k = font_px(st) as f64 / FONT as f64;
    let half = DVec2::new(chars * 3.6 * k + 4.0, lines * 8.0 * k + 2.0);
    match &a.kind {
        AnnotKind::Linear { a: p, b: q, offset } => {
            let (pa, pb) = (screen(cam, *p)?, screen(cam, *q)?);
            let (da, db) = (screen(cam, *p + *offset)?, screen(cam, *q + *offset)?);
            // Extension lines go a little past the dimension line.
            let ext = |from: DVec2, to: DVec2| {
                let d = (to - from).normalize_or_zero();
                (from + d * 4.0, to + d * 6.0)
            };
            let dir = (db - da).normalize_or_zero();
            let mid = (da + db) * 0.5;
            let normal = DVec2::new(dir.y, -dir.x);
            let up = if normal.y > 0.0 { -normal } else { normal };
            // Text Position (along the line): outside the start or the end, else centred.
            let outside = match st.along {
                0 => Some(da - dir * (half.x + 8.0)),
                2 => Some(db + dir * (half.x + 8.0)),
                _ => None,
            };
            let (label, angle) = if st.align_screen {
                (outside.unwrap_or(mid) - DVec2::new(0.0, half.y + 2.0), 0.0)
            } else {
                let mut ang = dir.y.atan2(dir.x);
                if ang > std::f64::consts::FRAC_PI_2 {
                    ang -= std::f64::consts::PI;
                } else if ang < -std::f64::consts::FRAC_PI_2 {
                    ang += std::f64::consts::PI;
                }
                let along = DVec2::new(ang.cos(), ang.sin());
                let at = match (outside, st.position) {
                    (Some(o), 0) => o + up * (half.y + 2.0),
                    (Some(o), _) => o,
                    (None, 1) => mid,
                    (None, 2) => db + along * (half.x + 8.0) * if along.dot(dir) >= 0.0 { 1.0 } else { -1.0 },
                    _ => mid + up * (half.y + 2.0),
                };
                (at, ang)
            };
            let mut segs = vec![ext(pa, da), ext(pb, db)];
            if st.position == 1 && outside.is_none() && !st.align_screen && (db - da).length() > half.x * 2.0 {
                // Centred text breaks the dimension line.
                segs.push((da, mid - dir * half.x));
                segs.push((mid + dir * half.x, db));
            } else {
                segs.push((da, db));
            }
            Some(Layout { segs, arrows: vec![(da, -dir), (db, dir)], label, half, angle })
        }
        AnnotKind::Text { point, leader } => {
            let p = screen(cam, *point)?;
            let t = if st.leader_type == 1 {
                // View based: the leader keeps its length on the screen.
                let d = (screen(cam, *point + *leader)? - p).normalize_or(DVec2::new(1.0, -1.0).normalize());
                p + d * 60.0
            } else {
                screen(cam, *point + *leader)?
            };
            // A hidden leader leaves the text where it was, alone.
            let (segs, arrows) = if st.leader_hidden {
                (vec![], vec![])
            } else {
                (vec![(p, t)], vec![(p, (p - t).normalize_or_zero())])
            };
            Some(Layout { segs, arrows, label: t + (t - p).normalize_or_zero() * half.x.min(30.0), half, angle: 0.0 })
        }
        AnnotKind::ScreenText { x, y } => Some(Layout {
            segs: vec![],
            arrows: vec![],
            label: DVec2::new(x * cam.size.x, y * cam.size.y),
            half,
            angle: 0.0,
        }),
    }
}

/// Whether a dimension is hidden by the expert settings in this view.
pub fn hidden_in_view(cam: &Camera, a: &Annotation, text: &str, st: &AnnotStyle) -> bool {
    let AnnotKind::Linear { a: p, b: q, offset } = a.kind else { return false };
    // Seen almost end-on: the dimension line points at the eye.
    if st.hide_foreshortened && (q - p).normalize_or_zero().dot(cam.forward()).abs() > 0.97 {
        return true;
    }
    if st.hide_small
        && let (Some(da), Some(db), Some(l)) =
            (screen(cam, p + offset), screen(cam, q + offset), layout_styled(cam, a, text, st))
        && da.distance(db) < l.half.x * 2.0 * 0.5
    {
        return true;
    }
    false
}

/// Distance in pixels from the cursor to an annotation (0 inside its label).
pub fn screen_distance(cam: &Camera, a: &Annotation, text: &str, m: DVec2) -> Option<f64> {
    let l = layout(cam, a, text)?;
    let d = m - l.label;
    let mut best = if d.x.abs() <= l.half.x && d.y.abs() <= l.half.y { 0.0 } else { f64::INFINITY };
    for (s, e) in &l.segs {
        best = best.min(dist_point_segment_2d(m, *s, *e));
    }
    Some(best)
}

fn font_px(st: &AnnotStyle) -> f32 {
    if st.font.size > 0.0 { (st.font.size * PX_PER_PT) as f32 } else { FONT }
}

/// A system font that is not there (or not readable): never looked for again.
const MISSING: u64 = u64::MAX;

/// The egui family of a system font (loaded the first time; the default one if missing).
/// Fonts are kept per context, with the pass that added them: egui takes a new font in at the
/// start of the next pass, so until then the default family stands in.
fn family(ctx: &egui::Context, f: &TextFont) -> FontFamily {
    let key = format!("{}|{}|{}", f.name, f.bold, f.italic);
    let id = egui::Id::unique("annot.fonts");
    let pass = ctx.cumulative_pass_nr();
    match ctx.data(|d| d.get_temp::<HashMap<String, u64>>(id)).and_then(|m| m.get(&key).copied()) {
        Some(added) if added < pass => return FontFamily::Name(key.into()),
        Some(_) => return FontFamily::Proportional,
        None => {}
    }
    let file = crate::fonts::font_list().file(&f.name, f.bold, f.italic);
    let bytes = file.as_ref().and_then(|file| std::fs::read(&file.path).ok());
    let added = match (file, bytes) {
        (Some(file), Some(bytes)) => {
            let mut data = egui::FontData::from_owned(bytes);
            data.index = file.index;
            ctx.add_font(egui::epaint::text::FontInsert::new(
                &key,
                data,
                vec![egui::epaint::text::InsertFontFamily {
                    family: FontFamily::Name(key.clone().into()),
                    priority: egui::epaint::text::FontPriority::Highest,
                }],
            ));
            pass
        }
        _ => MISSING,
    };
    ctx.data_mut(|d| d.get_temp_mut_or_default::<HashMap<String, u64>>(id).insert(key, added));
    FontFamily::Proportional
}

#[allow(clippy::too_many_arguments)]
pub fn draw_styled(
    painter: &egui::Painter,
    origin: Pos2,
    cam: &Camera,
    a: &Annotation,
    text: &str,
    selected: bool,
    st: &AnnotStyle,
) {
    if !selected && hidden_in_view(cam, a, text, st) {
        return;
    }
    let Some(l) = layout_styled(cam, a, text, st) else { return };
    let ink = st.highlight.unwrap_or(st.font.color);
    let col = if selected { Color32::from_rgb(40, 90, 255) } else { Color32::from_rgb(ink[0], ink[1], ink[2]) };
    let p = |v: DVec2| origin + egui::vec2(v.x as f32, v.y as f32);
    for (s, e) in &l.segs {
        painter.line_segment([p(*s), p(*e)], Stroke::new(1.0, col));
    }
    for (tip, dir) in &l.arrows {
        end_point(painter, p(*tip), *dir, st.arrow, col);
    }
    let r = egui::Rect::from_center_size(p(l.label), egui::vec2(l.half.x as f32 * 2.0, l.half.y as f32 * 2.0));
    let font = FontId::new(font_px(st), family(painter.ctx(), &st.font));
    if l.angle == 0.0 {
        painter.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 170));
        if selected {
            painter.rect_stroke(r, 2.0, Stroke::new(1.0, col), egui::StrokeKind::Inside);
        }
        painter.text(p(l.label), Align2::CENTER_CENTER, text, font, col);
    } else {
        let galley = painter.layout_no_wrap(text.to_string(), font, col);
        let top_left = p(l.label) - galley.size() / 2.0;
        let shape = egui::epaint::TextShape::new(top_left, galley, col)
            .with_angle_and_anchor(l.angle as f32, Align2::CENTER_CENTER);
        painter.add(shape);
    }
}

/// An end point: 1 slash, 2 dot, 3 closed arrow, 4 open arrow (pointing along `dir`).
fn end_point(painter: &egui::Painter, tip: Pos2, dir: DVec2, kind: u8, col: Color32) {
    let d = egui::vec2(dir.x as f32, dir.y as f32);
    let side = egui::vec2(-d.y, d.x) * 3.0;
    let back = tip - d * 9.0;
    match kind {
        1 => {
            let s = (d + egui::vec2(-d.y, d.x)).normalized() * 5.0;
            painter.line_segment([tip - s, tip + s], Stroke::new(1.5, col));
        }
        2 => {
            painter.circle_filled(tip, 2.5, col);
        }
        3 => {
            painter.add(egui::Shape::convex_polygon(vec![tip, back + side, back - side], col, Stroke::NONE));
        }
        4 => {
            painter.line_segment([tip, back + side], Stroke::new(1.0, col));
            painter.line_segment([tip, back - side], Stroke::new(1.0, col));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotation_properties_override_the_model_defaults() {
        let mut m = Model::new();
        let id = m.root.annots.insert(Annotation {
            kind: AnnotKind::Text { point: DVec3::ZERO, leader: DVec3::X },
            text: "Note".into(),
            layer: 0,
            hidden: false,
            attach: Default::default(),
        });
        let st = style_of(&m, &m.root, id);
        assert_eq!((st.font.name.as_str(), st.arrow, st.leader_type), ("Tahoma", 2, 1));
        let d = TextDefaults { arrow: 4, ..Default::default() };
        set_text_defaults(&mut m, &d);
        assert_eq!(style_of(&m, &m.root, id).arrow, 4);
        let f = TextFont { name: "Arial".into(), size: 18.0, color: [200, 0, 0], ..Default::default() };
        let ents = &mut m.root;
        apply_style(ents, id, &f, 0, &[("leader_type", AttrValue::Number(2.0))]);
        let st = style_of(&m, &m.root, id);
        assert_eq!((st.font, st.arrow, st.leader_type), (f, 0, 2));
    }

    /// A system font asked for twice in one pass is not named before egui has taken it in
    /// (the second ask used to name a family egui did not have yet).
    #[test]
    fn a_system_font_is_named_from_the_pass_after_it_is_added() {
        let ctx = egui::Context::default();
        let tahoma = TextFont::default();
        let missing = TextFont { name: "Police qui n'existe pas".into(), ..Default::default() };
        let mut asked = Vec::new();
        for _ in 0..3 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let ctx = ui.ctx().clone();
                let a = family(&ctx, &tahoma);
                let b = family(&ctx, &tahoma);
                asked.push((a.clone(), b, family(&ctx, &missing)));
                // A named family must lay text out (egui panics or logs on an unknown one).
                ctx.fonts_mut(|f| f.layout_no_wrap("Cuisine".into(), FontId::new(12.0, a), Color32::BLACK));
            });
            out.textures_delta.clear();
        }
        assert!(asked.iter().all(|x| x.2 == FontFamily::Proportional));
        assert_eq!((&asked[0].0, &asked[0].1), (&FontFamily::Proportional, &FontFamily::Proportional));
        if crate::fonts::font_list().file("Tahoma", false, false).is_some() {
            assert!(matches!(asked[2].0, FontFamily::Name(_)), "{asked:?}");
        }
    }
}
