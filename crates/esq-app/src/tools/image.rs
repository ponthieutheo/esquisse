//! Placing an imported image, as SketchUp's `CImageTool` does (File › Import, "Use Image As"
//! Image or Texture):
//!
//! - first point: the picture follows the cursor, its first corner on it, lying on the face
//!   under the cursor (else on the ground), at a first size of half the smaller side of the
//!   view (or the size of an image of the same file already in the model). A double-click
//!   drops it there;
//! - second point: the picture grows from the first corner, keeping its proportions (Shift:
//!   any proportions), from its centre while Ctrl is held. A drag from the first point works too;
//! - Measurements box: the height (or the width, when it is the width that follows the
//!   cursor), or "width;height". Before the first click, both values are needed.
//!
//! In texture mode the two points are picked on a face: the image becomes a material whose
//! texture tile is the rectangle, painted on the side of the face that is seen.

use eframe::egui::{Color32, Modifiers};
use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::math::Plane;
use esq_core::meta::{FaceTex, UvMap, face_axes};
use esq_core::model::{EntityRef, FaceId, GlueRef, Material, Units};

use super::{ClickDrag, Overlay, Tool, ToolCx, ToolId, split_values};
use crate::imgfile::LoadedImage;
use crate::pick::Snap;

/// SketchUp's length tolerance (0.001 inch), in mm.
const TOL: f64 = 0.0254;
/// Undo step name (SketchUp: "Import 2D Graphic").
pub const OPERATION: &str = "Importer graphique 2D";

/// The plane the picture lies on: corner, axes along its bottom and its left side, normal
/// (world coordinates, facing the camera).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub o: DVec3,
    pub x: DVec3,
    pub y: DVec3,
    pub n: DVec3,
}

impl Frame {
    /// The frame of a picture put at `p` on a plane of normal `n`, seen along `view`
    /// (from the camera to `p`).
    pub fn at(p: DVec3, n: DVec3, view: DVec3) -> Frame {
        let n = if n.dot(view) > 0.0 { -n } else { n };
        let (x, y) = face_axes(n);
        Frame { o: p, x, y, n: n.normalize() }
    }

    pub fn plane(&self) -> Plane {
        Plane::from_point_normal(self.o, self.n)
    }

    /// Coordinates of `p` in the plane.
    pub fn coords(&self, p: DVec3) -> DVec2 {
        DVec2::new((p - self.o).dot(self.x), (p - self.o).dot(self.y))
    }
}

/// The rectangle a size gives: lower-left corner offset (in the plane) and absolute size.
/// From the corner, a negative size extends the other way; by centre, the first point is
/// the middle.
pub fn rectangle(size: DVec2, by_center: bool) -> (DVec2, DVec2) {
    let abs = size.abs();
    let offset = if by_center { -abs * 0.5 } else { DVec2::new(size.x.min(0.0), size.y.min(0.0)) };
    (offset, abs)
}

/// The size for a cursor `d` away from the first point (in the plane): free, or with the
/// picture's proportions (`aspect` = width / height). Returns the size and whether the
/// width follows the cursor (then the Measurements box asks for the width), or `None` when
/// the cursor gives no usable size yet.
pub fn size_for(d: DVec2, aspect: f64, uniform: bool, by_center: bool) -> Option<(DVec2, bool)> {
    let (mut w, mut h) = (d.x, d.y);
    let mut width_driven = false;
    if !uniform {
        if w.abs() <= TOL || h.abs() <= TOL {
            return None;
        }
    } else {
        if w.abs() <= TOL && h.abs() <= TOL {
            return None;
        }
        if w.abs() >= TOL {
            if (h / w).abs() > 1.0 / aspect {
                w = h.abs() * w.signum() * aspect;
            } else {
                h = h.signum_or_one() * w.abs() / aspect;
                width_driven = true;
            }
        } else {
            w = aspect * h;
        }
    }
    if by_center {
        w *= 2.0;
        h *= 2.0;
    }
    Some((DVec2::new(w, h), width_driven))
}

trait SignumOrOne {
    fn signum_or_one(self) -> f64;
}

impl SignumOrOne for f64 {
    /// SketchUp's `(0 <= x) ? 1 : -1`.
    fn signum_or_one(self) -> f64 {
        if self >= 0.0 { 1.0 } else { -1.0 }
    }
}

/// The first size of a picture: its larger side is half the smaller side of the view, at the
/// distance of `p`, rounded to the millimetre (SketchUp rounds to the display precision).
pub fn first_size(px: [u32; 2], half_view_px: f64, pixel_size: f64) -> DVec2 {
    let aspect = px[0] as f64 / px[1].max(1) as f64;
    let mut d = (half_view_px * pixel_size).round();
    if d == 0.0 {
        d = 1.0;
    }
    if px[0] <= px[1] { DVec2::new(aspect * d, d) } else { DVec2::new(d, d / aspect) }
}

pub struct ImageTool {
    image: LoadedImage,
    /// Paint a face with the image (Use Image As: Texture) instead of adding an Image.
    texture: bool,
    aspect: f64,
    /// 0: first point, 1: second point.
    state: u8,
    frame: Option<Frame>,
    /// Size (signed, mm) shown now; the first one until the first click.
    size: DVec2,
    first: Option<DVec2>,
    width_driven: bool,
    /// Face of the active context under the first point (glue or texture target).
    face: Option<FaceId>,
    snap: Option<Snap>,
    mods: Modifiers,
    click: ClickDrag,
    pub done: bool,
}

impl ImageTool {
    pub fn new(image: LoadedImage, texture: bool) -> Self {
        let aspect = image.px[0] as f64 / image.px[1].max(1) as f64;
        ImageTool {
            image,
            texture,
            aspect,
            state: 0,
            frame: None,
            size: DVec2::ZERO,
            first: None,
            width_driven: false,
            face: None,
            snap: None,
            mods: Modifiers::NONE,
            click: ClickDrag::default(),
            done: false,
        }
    }

    /// Proportional unless Shift is held (`CImageTool` +0x15c set, +0x15d = Shift).
    fn uniform(&self) -> bool {
        !self.mods.shift
    }

    fn by_center(&self) -> bool {
        self.mods.ctrl || self.mods.command
    }

    /// The size of an image of the same file already in the model, if any.
    fn existing_size(&self, cx: &ToolCx) -> Option<DVec2> {
        let m = &cx.doc.model;
        let def = m
            .defs
            .iter()
            .find(|(_, d)| d.ext.image.as_ref().is_some_and(|i| i.path == self.image.path && !i.path.is_empty()))?
            .0;
        for ents in std::iter::once(&m.root).chain(m.defs.values().map(|d| &d.entities)) {
            if let Some(i) = ents.instances.values().find(|i| i.def == def) {
                let nat = m.image_natural_size(def)?;
                return Some(DVec2::new(
                    i.xform.x_axis.truncate().length() * nat.x,
                    i.xform.y_axis.truncate().length() * nat.y,
                ));
            }
        }
        None
    }

    fn update_first(&mut self, cx: &mut ToolCx, m: DVec2) {
        let s = cx.snap(m, None, None);
        let n = cx.plane_at(&s).n;
        let view = (s.pos - cx.cam().eye()).normalize_or(cx.cam().forward());
        let view = if cx.cam().perspective { view } else { cx.cam().forward() };
        self.frame = Some(Frame::at(s.pos, n, view));
        self.face = s.face.as_ref().and_then(|(h, _)| h.face_in(&cx.doc.ctx));
        if self.size == DVec2::ZERO {
            self.size = self.existing_size(cx).unwrap_or_else(|| {
                let cam = cx.cam();
                first_size(self.image.px, cam.size.x.min(cam.size.y) / 2.0, cam.pixel_size_at(s.pos))
            });
        }
        if self.uniform() {
            self.size.y = self.size.x / self.aspect;
        }
        self.snap = Some(s);
    }

    fn update_second(&mut self, cx: &mut ToolCx, m: DVec2) {
        let Some(f) = self.frame else { return };
        let s = cx.snap(m, Some(f.o), Some(f.plane()));
        let q = s.pos - f.n * f.plane().distance(s.pos);
        if let Some((size, wd)) = size_for(f.coords(q), self.aspect, self.uniform(), self.by_center()) {
            self.size = size;
            self.width_driven = wd;
        }
        self.snap = Some(s);
    }

    /// The four corners (world) of the rectangle shown now.
    fn corners(&self) -> Option<[DVec3; 4]> {
        let f = self.frame?;
        let (off, abs) = rectangle(self.size, self.by_center() && self.state == 1);
        let p = |u: f64, v: f64| f.o + f.x * u + f.y * v;
        Some([p(off.x, off.y), p(off.x + abs.x, off.y), p(off.x + abs.x, off.y + abs.y), p(off.x, off.y + abs.y)])
    }

    fn preview(&self, cx: &mut ToolCx) {
        let mut m = cx.doc.model.clone();
        let (d, ctx) = (cx.doc.ctx_def(), cx.doc.ctx_xform());
        if self.texture || self.place(&mut m, d, ctx).is_none() {
            cx.doc.set_preview(None);
        } else {
            cx.doc.set_preview(Some(m));
        }
    }

    /// Adds the image to collection `d` of `m`, placed by `ctx`; returns its instance.
    fn place(
        &self,
        m: &mut esq_core::model::Model,
        d: Option<esq_core::model::DefId>,
        ctx: DMat4,
    ) -> Option<esq_core::model::InstanceId> {
        let f = self.frame?;
        let c = self.corners()?;
        let inv = ctx.inverse();
        let (o, x, n) = (inv.transform_point3(c[0]), inv.transform_vector3(f.x), inv.transform_vector3(f.n));
        let (_, abs) = rectangle(self.size, false);
        let inst = m.add_image(
            d,
            &self.image.stem(),
            &self.image.path,
            self.image.texture([0.0, 0.0]),
            o,
            x,
            n,
            abs.x,
            Some(abs.y),
        )?;
        if let Some(face) = self.face {
            m.entities_mut(d).instances[inst].ext.glued_to = Some(GlueRef::Face(face));
        }
        Some(inst)
    }

    /// Paints the face with the picture as a texture whose tile is the rectangle.
    fn paint(&self, cx: &mut ToolCx) -> bool {
        let (Some(face), Some(f), Some(c)) = (self.face, self.frame, self.corners()) else { return false };
        if !cx.doc.ents().faces.contains_key(face) {
            return false;
        }
        let inv = cx.doc.ctx_xform().inverse();
        let (o, x, y) = (inv.transform_point3(c[0]), inv.transform_vector3(f.x), inv.transform_vector3(f.y));
        let (_, abs) = rectangle(self.size, false);
        let row = |a: DVec3, len: f64| {
            let a = a / len;
            [a.x, a.y, a.z, -a.dot(o)]
        };
        let map = UvMap::affine(row(x.normalize(), abs.x), row(y.normalize(), abs.y));
        let front_seen = {
            let fnorm = cx.doc.ctx_xform().transform_vector3(cx.doc.ents().faces[face].normal);
            fnorm.dot(f.n) > 0.0
        };
        let tex = self.image.texture([abs.x, abs.y]);
        let base = self.image.stem();
        cx.doc.commit(OPERATION, |m, d| {
            let taken: Vec<String> = m.materials.values().map(|x| x.name.clone()).collect();
            let mat = m.materials.insert(Material {
                name: esq_core::model::Model::unique_name(&base, &taken),
                color: tex.avg,
                opacity: 1.0,
                ext: esq_core::meta::MaterialExt { texture: Some(tex), ..Default::default() },
            });
            let fc = &mut m.entities_mut(d).faces[face];
            let t = fc.tex.get_or_insert_with(|| Box::new(FaceTex::default()));
            if front_seen {
                fc.front = Some(mat);
                t.front = Some(map);
            } else {
                fc.back = Some(mat);
                t.back = Some(map);
            }
        });
        true
    }

    fn finish(&mut self, cx: &mut ToolCx) {
        if self.texture {
            if !self.paint(cx) {
                return;
            }
        } else {
            let ctx = cx.doc.ctx_xform();
            let me = &*self;
            let mut made = None;
            let r = cx.doc.try_commit(OPERATION, |m, d| {
                made = me.place(m, d, ctx);
                made.map(|_| ()).ok_or(())
            });
            match (r, made) {
                // As SketchUp, the new image is selected.
                (Ok(()), Some(i)) => cx.doc.select(vec![EntityRef::Instance(i)]),
                _ => return,
            }
        }
        cx.doc.set_preview(None);
        self.state = 0;
        self.frame = None;
        self.done = true;
    }

    /// The image of the second click, the size given in the Measurements box.
    fn typed(&mut self, cx: &mut ToolCx, w: f64, h: f64) {
        self.size = DVec2::new(w, h);
        self.finish(cx);
    }
}

impl Tool for ImageTool {
    fn id(&self) -> ToolId {
        ToolId::Image
    }

    fn status(&self) -> String {
        let first = match (self.texture, self.state) {
            (false, 0) => "Choisissez le premier point de l'image. Double-clic : déposer.",
            (true, 0) => "Choisissez le premier point sur la face à peindre avec l'image. Double-clic : déposer.",
            _ => "Choisissez le deuxième point de l'image.",
        };
        format!("{first} Ctrl : par le centre. Maj : non uniforme.")
    }

    fn vcb_label(&self) -> &'static str {
        if self.state == 1 && self.uniform() {
            if self.width_driven { "Largeur" } else { "Hauteur" }
        } else {
            "Dimensions"
        }
    }

    fn vcb_value(&self, units: Units) -> String {
        let (_, abs) = rectangle(self.size, false);
        if self.state == 1 && self.uniform() {
            esq_core::units::format_length(if self.width_driven { abs.x } else { abs.y }, units)
        } else {
            format!(
                "{}; {}",
                esq_core::units::format_length(abs.x, units),
                esq_core::units::format_length(abs.y, units)
            )
        }
    }

    fn snap(&self) -> Option<&Snap> {
        self.snap.as_ref()
    }

    fn busy(&self) -> bool {
        !self.done
    }

    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mods = cx.mods;
        if self.state == 0 {
            self.update_first(cx, m);
        } else {
            self.update_second(cx, m);
        }
        self.preview(cx);
    }

    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mods = cx.mods;
        if self.state == 0 {
            self.update_first(cx, m);
            // Texture: the first point must be on a face.
            if self.texture && self.face.is_none() {
                *cx.message = Some("Choisissez un point sur une face.".into());
                return;
            }
            self.first = self.frame.map(|f| cx.cam().project(f.o).unwrap_or(m));
            self.state = 1;
            self.click.press(m, true);
        } else {
            self.update_second(cx, m);
            self.finish(cx);
        }
    }

    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        // A drag from the first point sets the second one where the button is released.
        if self.click.release(m) && self.state == 1 {
            self.update_second(cx, m);
            self.finish(cx);
        }
    }

    fn double_click(&mut self, cx: &mut ToolCx, m: DVec2) {
        // Dbl Click = Drop: the picture at its first size, where the cursor is.
        if self.done {
            return;
        }
        self.state = 0;
        self.update_first(cx, m);
        if self.texture && self.face.is_none() {
            return;
        }
        self.finish(cx);
    }

    fn vcb(&mut self, cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let err = || "Dimensions d'image incorrectes.".to_string();
        let vals: Vec<f64> = split_values(text)
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| cx.parse_len(s))
            .collect::<Result<_, _>>()
            .map_err(|_| err())?;
        let (w, h) = match vals.as_slice() {
            [v] if self.state == 1 => {
                if self.width_driven {
                    (*v, v / self.aspect)
                } else {
                    (v * self.aspect, *v)
                }
            }
            [w, h] => (*w, *h),
            _ => return Err(err()),
        };
        if w.abs() < TOL || h.abs() < TOL || self.frame.is_none() {
            return Err(err());
        }
        self.typed(cx, w, h);
        Ok(())
    }

    fn cancel(&mut self, cx: &mut ToolCx) {
        if self.state == 1 {
            self.state = 0;
            self.size = DVec2::ZERO;
        }
        cx.doc.set_preview(None);
    }

    fn overlay(&self, o: &mut Overlay) {
        if let Some(c) = self.corners() {
            let col = Color32::from_rgb(0, 0, 255);
            o.polyline(&c, true, col, 1.5);
            if self.texture {
                o.fill_polygon(&c, Color32::from_rgba_unmultiplied(0, 0, 255, 30));
            }
        }
    }

    /// Once the image is placed, the tool gives way to Select.
    fn switch_to(&mut self) -> Option<ToolId> {
        std::mem::take(&mut self.done).then_some(ToolId::Select)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_follow_the_cursor_like_sketchup() {
        // A 2:1 picture, cursor 300 right and 100 up: the width follows (|dy/dx| < 1/2).
        let (s, wd) = size_for(DVec2::new(300.0, 100.0), 2.0, true, false).unwrap();
        assert_eq!((s, wd), (DVec2::new(300.0, 150.0), true));
        // Cursor 100 right and 300 up: the height follows.
        let (s, wd) = size_for(DVec2::new(100.0, 300.0), 2.0, true, false).unwrap();
        assert_eq!((s, wd), (DVec2::new(600.0, 300.0), false));
        // Down and to the left: the rectangle extends that way, never mirrored.
        let (s, _) = size_for(DVec2::new(-300.0, -100.0), 2.0, true, false).unwrap();
        assert_eq!(s, DVec2::new(-300.0, -150.0));
        assert_eq!(rectangle(s, false), (DVec2::new(-300.0, -150.0), DVec2::new(300.0, 150.0)));
        // Shift: any proportions; nothing until both sides have a length.
        assert_eq!(size_for(DVec2::new(300.0, 100.0), 2.0, false, false).unwrap().0, DVec2::new(300.0, 100.0));
        assert!(size_for(DVec2::new(300.0, 0.0), 2.0, false, false).is_none());
        // Ctrl: from the centre, twice the distance.
        let (s, _) = size_for(DVec2::new(300.0, 100.0), 2.0, true, true).unwrap();
        assert_eq!(rectangle(s, true), (DVec2::new(-300.0, -150.0), DVec2::new(600.0, 300.0)));
        // Straight up: the height gives the width.
        assert_eq!(size_for(DVec2::new(0.0, 50.0), 2.0, true, false).unwrap().0, DVec2::new(100.0, 50.0));
    }

    #[test]
    fn first_size_is_half_the_view() {
        // 400 px at 2.5 mm per pixel = 1000 mm for the larger side.
        assert_eq!(first_size([200, 100], 400.0, 2.5), DVec2::new(1000.0, 500.0));
        assert_eq!(first_size([100, 200], 400.0, 2.5), DVec2::new(500.0, 1000.0));
        // Rounded to the millimetre.
        assert_eq!(first_size([100, 100], 400.0, 0.00301).x, 1.0);
    }

    #[test]
    fn frames_face_the_camera() {
        // Ground seen from above: along red and green.
        let f = Frame::at(DVec3::ZERO, DVec3::Z, DVec3::new(0.0, 1.0, -1.0));
        assert_eq!((f.x, f.y, f.n), (DVec3::X, DVec3::Y, DVec3::Z));
        // A wall whose normal points away from the camera: turned to face it, upright.
        let f = Frame::at(DVec3::ZERO, DVec3::Y, DVec3::Y);
        assert!(f.n.distance(-DVec3::Y) < 1e-12 && f.y.distance(DVec3::Z) < 1e-12, "{f:?}");
    }
}
