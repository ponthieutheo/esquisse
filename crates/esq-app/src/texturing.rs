//! The texture commands of a face (right-click › Texture, Edit › Face, Search): Position,
//! Reset Position, Projected and Make Unique Texture. Read from SketchUp.exe
//! (`GetEditorCommands` 21515, 21516, 30012, 30011 and the face's Texture submenu):
//!
//! - they act on the side of the face the camera sees, and need the view to show textures;
//! - Position, Reset and Projected need exactly one face selected whose seen side has a
//!   textured material; Reset needs a positioned texture; Position is refused on a projected
//!   texture whose direction is not the face's normal;
//! - Reset Position ("Reset Texture Position") forgets the side's positioning (both sides
//!   when they share the material); from Position Texture, positioning starts over;
//! - Projected ("Project Texture") toggles the projection of the side's texture along the
//!   face's normal: a sample of it (Sample Material, Alt+Paint Bucket) then projects the
//!   texture onto the faces painted with it;
//! - Make Unique Texture makes, for each selected face, a new material whose image is the
//!   texture as the face shows it (distortion included), laid flat on the face.

use esq_core::glam::{DVec2, DVec3};
use esq_core::meta::{FaceTex, TextureData, UvMap};
use esq_core::model::{EntityRef, FaceId, MaterialId, Model};
use esq_core::photo::Blob;
use esq_core::texpos::{self, Frame};

use crate::app::EsquisseApp;
use crate::camera::Camera;
use crate::doc::Document;
use crate::style::ViewStyle;
use crate::tools::ToolId;
use crate::tools::postex::{PositionTextureTool, back_seen, side_material};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TexCmd {
    Position,
    Reset,
    Project,
    MakeUnique,
}

impl TexCmd {
    /// Command name (commands.json).
    pub fn label(self) -> &'static str {
        match self {
            TexCmd::Position => "Positionner la texture",
            TexCmd::Reset => "Réinitialiser la texture",
            TexCmd::Project => "Projeter la texture",
            TexCmd::MakeUnique => "Rendre la texture unique",
        }
    }
}

/// The selected face, alone, with the textured side the camera sees: (face, back side,
/// both sides share the material).
pub(crate) fn one_face(doc: &Document, cam: &Camera, style: &ViewStyle) -> Option<(FaceId, bool, bool)> {
    if !crate::scene::textures_shown(style) {
        return None;
    }
    let [EntityRef::Face(f)] = doc.sel.as_slice() else { return None };
    let (_, back, both) = side_material(&doc.model, doc.ents(), *f, back_seen(doc, cam, *f))?;
    Some((*f, back, both))
}

fn side_tex(doc: &Document, f: FaceId) -> Option<&FaceTex> {
    doc.ents().faces.get(f)?.tex.as_deref()
}

pub fn enabled(app: &EsquisseApp, c: TexCmd) -> bool {
    enabled_in(&app.doc, &app.cam, &app.style, c)
}

pub fn enabled_in(doc: &Document, cam: &Camera, style: &ViewStyle, c: TexCmd) -> bool {
    match c {
        TexCmd::MakeUnique => !unique_targets(doc, cam, style).is_empty(),
        _ => {
            let Some((f, back, _)) = one_face(doc, cam, style) else { return false };
            let k = back as usize;
            match c {
                TexCmd::Position => {
                    let n = doc.ents().faces[f].normal;
                    side_tex(doc, f)
                        .and_then(|t| t.projection[k])
                        .is_none_or(|d| d.normalize_or_zero().dot(n).abs() > 1.0 - 1e-6)
                }
                TexCmd::Reset => {
                    side_tex(doc, f).is_some_and(|t| if back { t.back.is_some() } else { t.front.is_some() })
                }
                _ => true,
            }
        }
    }
}

/// Projected is ticked when the side's texture is projected.
pub fn checked(app: &EsquisseApp, c: TexCmd) -> Option<bool> {
    checked_in(&app.doc, &app.cam, &app.style, c)
}

pub fn checked_in(doc: &Document, cam: &Camera, style: &ViewStyle, c: TexCmd) -> Option<bool> {
    (c == TexCmd::Project).then(|| {
        one_face(doc, cam, style)
            .is_some_and(|(f, back, _)| side_tex(doc, f).is_some_and(|t| t.projection[back as usize].is_some()))
    })
}

pub fn run(app: &mut EsquisseApp, c: TexCmd) {
    match c {
        TexCmd::Position => {
            if let Some((f, _, _)) = one_face(&app.doc, &app.cam, &app.style) {
                position(app, f);
            }
        }
        TexCmd::Reset => {
            let restart = app.tool.id() == ToolId::PositionTexture;
            if restart {
                app.cancel_tool();
            }
            let f = one_face(&app.doc, &app.cam, &app.style).map(|x| x.0);
            apply(&mut app.doc, &app.cam, &app.style, c);
            if let (true, Some(f)) = (restart, f) {
                position(app, f);
            }
        }
        _ => apply(&mut app.doc, &app.cam, &app.style, c),
    }
}

/// Reset Position, Projected and Make Unique Texture on the selection (Position starts a
/// tool instead: [`position`]).
pub fn apply(doc: &mut Document, cam: &Camera, style: &ViewStyle, c: TexCmd) {
    if !enabled_in(doc, cam, style, c) {
        return;
    }
    match c {
        TexCmd::Position => {}
        TexCmd::Reset => {
            if let Some((f, back, both)) = one_face(doc, cam, style) {
                reset_face(doc, f, back, both);
            }
        }
        TexCmd::Project => {
            if let Some((f, back, _)) = one_face(doc, cam, style) {
                toggle_projected(doc, f, back);
            }
        }
        TexCmd::MakeUnique => {
            let targets = unique_targets(doc, cam, style);
            doc.commit("Rendre la texture unique", |m, d| {
                make_unique(m, d, &targets);
            });
        }
    }
}

/// Starts Position Texture on face `f` of the active context.
pub fn position(app: &mut EsquisseApp, f: FaceId) {
    app.cancel_tool();
    let prev = match app.tool.id() {
        ToolId::PositionTexture => ToolId::Select,
        t => t,
    };
    if let Some(t) = PositionTextureTool::start(&app.doc, &app.cam, f, app.context_point.take(), prev) {
        app.tool = Box::new(t);
    }
}

/// Texture › Reset Position: the side (and the other one when they share the material)
/// goes back to the default position, as one operation.
pub fn reset_face(doc: &mut Document, f: FaceId, back: bool, both: bool) {
    doc.commit("Réinitialiser la position de la texture", |m, d| {
        let Some(face) = m.entities_mut(d).faces.get_mut(f) else { return };
        let Some(t) = face.tex.as_mut() else { return };
        for side in [back, !back] {
            if side != back && !both {
                continue;
            }
            let k = side as usize;
            if side {
                t.back = None;
            } else {
                t.front = None;
            }
            t.projection[k] = None;
            t.pins[k].clear();
        }
        if t.front.is_none() && t.back.is_none() && t.projection == [None, None] {
            face.tex = None;
        }
    });
}

/// Texture › Projected: on, the texture of the side is projected along the face's normal, as
/// it lies on the face (SketchUp marks the side positioned too: TestUp
/// `test_texture_projected_Query_mapped_by_ui`); off, it stays where it is.
pub fn toggle_projected(doc: &mut Document, f: FaceId, back: bool) {
    doc.commit("Projeter la texture", |m, d| {
        let shown = crate::tools::sample::side_map(m, m.entities(d), f, back);
        let Some(face) = m.entities_mut(d).faces.get_mut(f) else { return };
        let n = face.normal;
        let t = face.tex.get_or_insert_with(|| Box::new(FaceTex::default()));
        let k = back as usize;
        t.projection[k] = match t.projection[k] {
            Some(_) => None,
            None => Some(n),
        };
        // A map in the face's plane does not change along its normal: projected along the
        // normal, it shows the same.
        let map = if back { &mut t.back } else { &mut t.front };
        if t.projection[k].is_some() && map.is_none() {
            *map = shown;
        }
        if t.front.is_none() && t.back.is_none() && t.projection == [None, None] && t.pins.iter().all(Vec::is_empty) {
            face.tex = None;
        }
    });
}

/// The selected faces Make Unique Texture works on: those whose seen side is textured.
fn unique_targets(doc: &Document, cam: &Camera, style: &ViewStyle) -> Vec<(FaceId, bool, bool)> {
    if !crate::scene::textures_shown(style) {
        return Vec::new();
    }
    let faces: Vec<FaceId> =
        doc.sel.iter().filter_map(|e| if let EntityRef::Face(f) = e { Some(*f) } else { None }).collect();
    let mut out = Vec::new();
    for f in faces {
        match side_material(&doc.model, doc.ents(), f, back_seen(doc, cam, f)) {
            Some((_, back, both)) => out.push((f, back, both)),
            None => return Vec::new(),
        }
    }
    out
}

/// Biggest side of an image Make Unique Texture creates.
const UNIQUE_MAX_PX: u32 = 2048;

/// Make Unique Texture on faces of collection `def`: returns the materials made.
pub fn make_unique(
    m: &mut Model,
    def: Option<esq_core::model::DefId>,
    faces: &[(FaceId, bool, bool)],
) -> Vec<MaterialId> {
    let mut made = Vec::new();
    for &(f, back, both) in faces {
        let Some((mat, frame, map, outline)) = (|| {
            let ents = m.entities(def);
            let face = ents.faces.get(f)?;
            let mat = if back { face.back } else { face.front }?;
            let map = crate::tools::sample::side_map(m, ents, f, back)?;
            let frame = Frame::new(ents.pos(face.outer[0]), if back { -face.normal } else { face.normal });
            let outline: Vec<DVec2> = face.outer.iter().map(|v| frame.to2(ents.pos(*v))).collect();
            Some((mat, frame, map, outline))
        })() else {
            continue;
        };
        let Some(src) = m.materials.get(mat).cloned() else { continue };
        let Some(tex) = src.ext.texture.as_ref() else { continue };
        let Some((data, lo, size)) = unique_image(tex, &map, &frame, &outline) else { continue };
        let taken: Vec<String> = m.materials.values().map(|x| x.name.clone()).collect();
        let name = Model::unique_name(&src.name, &taken);
        let mut ext = src.ext.clone();
        ext.owner_image = false;
        ext.texture = Some(TextureData { filename: format!("{name}.png"), ..data });
        let new = m.materials.insert(esq_core::model::Material { name, color: src.color, opacity: src.opacity, ext });
        made.push(new);
        // The new image lies flat on the face's box.
        let row = |e: DVec3, o: f64, s: f64| [e.x / s, e.y / s, e.z / s, -o / s];
        let flat = UvMap::affine(row(frame.ex, lo.x, size.x), row(frame.ey, lo.y, size.y));
        let Some(face) = m.entities_mut(def).faces.get_mut(f) else { continue };
        let t = face.tex.get_or_insert_with(|| Box::new(FaceTex::default()));
        for side in [back, !back] {
            if side != back && !both {
                continue;
            }
            let k = side as usize;
            if side {
                face.back = Some(new);
                t.back = Some(flat);
            } else {
                face.front = Some(new);
                t.front = Some(flat);
            }
            t.projection[k] = None;
            t.pins[k].clear();
        }
    }
    made
}

/// The texture as the face shows it, over the face's box (plane coordinates `lo` .. `lo +
/// size`): a new image file (PNG) with its size.
fn unique_image(
    tex: &TextureData,
    map: &UvMap,
    frame: &Frame,
    outline: &[DVec2],
) -> Option<(TextureData, DVec2, DVec2)> {
    let texels = crate::textures::get(&tex.image, None)?;
    let lo = outline.iter().fold(DVec2::splat(f64::INFINITY), |a, p| a.min(*p));
    let hi = outline.iter().fold(DVec2::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
    let size = hi - lo;
    if !(size.x > 1e-9 && size.y > 1e-9) {
        return None;
    }
    let (w, h) = (tex.size[0].max(1e-9), tex.size[1].max(1e-9));
    let g = texpos::plane_matrix(map, frame, tex.size);
    // As many pixels as the source shows over the face (its density at the face's centre).
    let c = (lo + hi) / 2.0;
    let k = 1e-3 * size.x.max(size.y);
    let d = |dir: DVec2| {
        let a = texpos::apply(&g, c);
        let b = texpos::apply(&g, c + dir * k);
        let t = (b - a) / k;
        (t.x / w * texels.w as f64).hypot(t.y / h * texels.h as f64)
    };
    let px = |len: f64, dens: f64| ((len * dens).round() as u32).clamp(1, UNIQUE_MAX_PX);
    let (pw, ph) = (px(size.x, d(DVec2::X)), px(size.y, d(DVec2::Y)));
    let (tw, th) = (texels.w as usize, texels.h as usize);
    let at = |x: i64, y: i64, ch: usize| {
        let x = x.rem_euclid(tw as i64) as usize;
        let y = y.rem_euclid(th as i64) as usize;
        texels.rgba[(y * tw + x) * 4 + ch] as f64
    };
    let mut out = vec![0u8; pw as usize * ph as usize * 4];
    let mut sum = [0.0f64; 3];
    for j in 0..ph {
        for i in 0..pw {
            let p =
                DVec2::new(lo.x + (i as f64 + 0.5) / pw as f64 * size.x, hi.y - (j as f64 + 0.5) / ph as f64 * size.y);
            let t = texpos::apply(&g, p);
            // Image pixel coordinates (row 0 at the top: v grows upward).
            let fx = t.x / w * tw as f64 - 0.5;
            let fy = (1.0 - t.y / h) * th as f64 - 0.5;
            let (x0, y0) = (fx.floor(), fy.floor());
            let (ax, ay) = (fx - x0, fy - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let o = (j as usize * pw as usize + i as usize) * 4;
            for ch in 0..4 {
                let v = at(x0, y0, ch) * (1.0 - ax) * (1.0 - ay)
                    + at(x0 + 1, y0, ch) * ax * (1.0 - ay)
                    + at(x0, y0 + 1, ch) * (1.0 - ax) * ay
                    + at(x0 + 1, y0 + 1, ch) * ax * ay;
                out[o + ch] = v.round().clamp(0.0, 255.0) as u8;
                if ch < 3 {
                    sum[ch] += out[o + ch] as f64;
                }
            }
        }
    }
    let n = (pw as f64 * ph as f64).max(1.0);
    let avg = [0, 1, 2].map(|i| (sum[i] / n).round() as u8);
    let img = image::RgbaImage::from_raw(pw, ph, out)?;
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).ok()?;
    let data = TextureData {
        filename: String::new(),
        image: Blob(std::sync::Arc::new(png.into_inner())),
        px: [pw, ph],
        size: [size.x, size.y],
        avg,
    };
    Some((data, lo, size))
}
