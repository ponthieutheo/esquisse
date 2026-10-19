//! Images (File › Import as an image, the Image tool, `Sketchup::Image`), modelled as SketchUp
//! does: an instance of a definition flagged as an image. The definition holds one rectangle
//! face, one inch per pixel (as `SUImageCreateFromFile` makes it), painted on both sides with
//! the image's own material (`MaterialExt::owner_image`, hidden from Window › Materials) whose
//! texture tile is exactly the rectangle; the instance's transformation places, turns and
//! sizes the picture (width and height = rectangle × scale of the transformation).
//!
//! Exploding an image leaves a textured face, its material becoming an ordinary one.

use glam::{DMat4, DVec2, DVec3};
use serde::{Deserialize, Serialize};

use crate::meta::{FaceTex, TextureData, UvMap, default_uv};
use crate::model::{DefId, Definition, Entities, Instance, InstanceId, Material, MaterialId, Model};

/// One pixel of an image's definition, in millimetres (one inch, as in SketchUp).
pub const PX_MM: f64 = 25.4;

/// What makes a definition an image (`ComponentDefinition#image?`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageDef {
    /// The file the image was made from, as `Image#path` reports it. Informative only: a name
    /// read from a model is never opened as a path.
    pub path: String,
    /// Size of the picture in pixels.
    pub px: [u32; 2],
}

/// An image instance, as seen from its collection.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageView {
    pub def: DefId,
    pub path: String,
    pub px: [u32; 2],
    /// Size in the model, millimetres.
    pub width: f64,
    pub height: f64,
    /// Corner of the picture that was its first point (bottom-left of the picture).
    pub origin: DVec3,
    /// Unit vectors along the bottom and the left side of the picture, and its normal.
    pub xaxis: DVec3,
    pub yaxis: DVec3,
    pub normal: DVec3,
    pub xform: DMat4,
    /// The image's own material (its texture is the picture).
    pub material: Option<MaterialId>,
}

impl ImageView {
    /// The four corners in the collection's coordinates, counter-clockwise from the origin.
    pub fn corners(&self) -> [DVec3; 4] {
        let (x, y) = (self.xaxis * self.width, self.yaxis * self.height);
        [self.origin, self.origin + x, self.origin + x + y, self.origin + y]
    }
}

/// A rectangle face from the origin to `(w, h, 0)` painted with `mat` on both sides.
fn rectangle(w: f64, h: f64, mat: Option<MaterialId>) -> Entities {
    let mut e = Entities::default();
    let v = [DVec3::ZERO, DVec3::new(w, 0.0, 0.0), DVec3::new(w, h, 0.0), DVec3::new(0.0, h, 0.0)]
        .map(|p| e.add_vertex_raw(p));
    if let Some(f) = e.add_face_raw(v.to_vec(), Vec::new()) {
        e.faces[f].front = mat;
        e.faces[f].back = mat;
    }
    e
}

/// The placement of a picture of natural size `nat` (mm) shown `width` × `height` at `origin`,
/// its bottom along `xaxis`, facing `normal`.
fn placement(origin: DVec3, xaxis: DVec3, normal: DVec3, nat: DVec2, width: f64, height: f64) -> Option<DMat4> {
    let n = normal.normalize_or_zero();
    let x = (xaxis - n * xaxis.dot(n)).normalize_or_zero();
    if n == DVec3::ZERO || x == DVec3::ZERO || nat.x <= 0.0 || nat.y <= 0.0 {
        return None;
    }
    let y = n.cross(x);
    Some(DMat4::from_cols(
        (x * (width / nat.x)).extend(0.0),
        (y * (height / nat.y)).extend(0.0),
        n.extend(0.0),
        origin.extend(1.0),
    ))
}

/// `map` (texture coordinates of points of a collection) seen from the parent collection,
/// where that collection is placed by `m`: uv(p) = map(m⁻¹ p).
pub fn transform_uv(map: &UvMap, m: &DMat4) -> UvMap {
    let inv = m.inverse();
    let row = |c: &[f64; 4]| {
        let r = glam::DVec4::from_array(*c);
        [r.dot(inv.x_axis), r.dot(inv.y_axis), r.dot(inv.z_axis), r.dot(inv.w_axis)]
    };
    UvMap { u: row(&map.u), v: row(&map.v), q: row(&map.q) }
}

impl Model {
    /// Adds an image to collection `ctx`: a picture of `tex` (its pixel size must be set),
    /// `width` mm wide (`height` defaults to the picture's proportions), its first corner at
    /// `origin`, its bottom along `xaxis`, facing `normal`. `name` names the definition and
    /// the material; `path` is what `Image#path` will report.
    #[allow(clippy::too_many_arguments)]
    pub fn add_image(
        &mut self,
        ctx: Option<DefId>,
        name: &str,
        path: &str,
        mut tex: TextureData,
        origin: DVec3,
        xaxis: DVec3,
        normal: DVec3,
        width: f64,
        height: Option<f64>,
    ) -> Option<InstanceId> {
        let [pw, ph] = tex.px;
        if pw == 0 || ph == 0 || width.abs() < 1e-9 || height.is_some_and(|h| h.abs() < 1e-9) {
            return None;
        }
        let nat = DVec2::new(pw as f64, ph as f64) * PX_MM;
        let height = height.unwrap_or(width * ph as f64 / pw as f64);
        let xform = placement(origin, xaxis, normal, nat, width, height)?;
        if ctx.is_some_and(|d| !self.defs.contains_key(d)) {
            return None;
        }
        tex.size = [nat.x, nat.y];
        let def = self.add_image_def(name, path, tex)?;
        Some(self.entities_mut(ctx).instances.insert(Instance {
            def,
            xform,
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        }))
    }

    /// The definition of an image of `tex` (its pixel size must be set): the rectangle of one
    /// inch per pixel and the image's own material. The caller places instances of it.
    pub fn add_image_def(&mut self, name: &str, path: &str, mut tex: TextureData) -> Option<DefId> {
        let [pw, ph] = tex.px;
        if pw == 0 || ph == 0 {
            return None;
        }
        let nat = DVec2::new(pw as f64, ph as f64) * PX_MM;
        tex.size = [nat.x, nat.y];
        let base = if name.trim().is_empty() { "Image" } else { name.trim() };
        let taken: Vec<String> = self.materials.values().map(|m| m.name.clone()).collect();
        let mat = self.materials.insert(Material {
            name: Model::unique_name(base, &taken),
            color: tex.avg,
            opacity: 1.0,
            ext: crate::meta::MaterialExt { texture: Some(tex), owner_image: true, ..Default::default() },
        });
        let taken: Vec<String> = self.defs.values().map(|d| d.name.clone()).collect();
        Some(self.defs.insert(Definition {
            name: Model::unique_name(base, &taken),
            is_group: false,
            description: String::new(),
            entities: rectangle(nat.x, nat.y, Some(mat)),
            attributes: Default::default(),
            ext: crate::model::DefExt {
                image: Some(ImageDef { path: path.to_string(), px: [pw, ph] }),
                ..Default::default()
            },
        }))
    }

    /// Whether `def` is the definition of an image.
    pub fn is_image_def(&self, def: DefId) -> bool {
        self.defs.get(def).is_some_and(|d| d.ext.image.is_some())
    }

    /// Size (mm) of the rectangle an image definition holds, before its instance's scaling.
    pub fn image_natural_size(&self, def: DefId) -> Option<DVec2> {
        let d = self.defs.get(def)?;
        let img = d.ext.image.as_ref()?;
        let hi = d.entities.verts.values().fold(DVec2::ZERO, |a, v| a.max(v.pos.truncate()));
        if hi.x > 1e-9 && hi.y > 1e-9 {
            Some(hi)
        } else {
            Some(DVec2::new(img.px[0].max(1) as f64, img.px[1].max(1) as f64) * PX_MM)
        }
    }

    /// The image's own material (the one its face is painted with).
    pub fn image_material(&self, def: DefId) -> Option<MaterialId> {
        let d = self.defs.get(def)?;
        d.ext.image.as_ref()?;
        d.entities.faces.values().find_map(|f| f.front)
    }

    /// The image that instance `inst` of collection `ctx` is, if it is one.
    pub fn image_of(&self, ctx: Option<DefId>, inst: InstanceId) -> Option<ImageView> {
        let ents = match ctx {
            Some(d) => &self.defs.get(d)?.entities,
            None => &self.root,
        };
        self.image_view(ents.instances.get(inst)?)
    }

    fn image_view(&self, i: &Instance) -> Option<ImageView> {
        let img = self.defs.get(i.def)?.ext.image.clone()?;
        let nat = self.image_natural_size(i.def)?;
        let m = i.xform;
        let (xs, ys) = (m.x_axis.truncate(), m.y_axis.truncate());
        Some(ImageView {
            def: i.def,
            path: img.path,
            px: img.px,
            width: xs.length() * nat.x,
            height: ys.length() * nat.y,
            origin: m.w_axis.truncate(),
            xaxis: xs.normalize_or_zero(),
            yaxis: ys.normalize_or_zero(),
            normal: xs.cross(ys).normalize_or_zero(),
            xform: m,
            material: self.image_material(i.def),
        })
    }

    /// Resizes an image, keeping its origin and orientation. Returns false if it is not one
    /// or the size is null.
    pub fn set_image_size(&mut self, ctx: Option<DefId>, inst: InstanceId, width: f64, height: f64) -> bool {
        let Some(v) = self.image_of(ctx, inst) else { return false };
        let Some(nat) = self.image_natural_size(v.def) else { return false };
        let Some(x) = placement(v.origin, v.xaxis, v.normal, nat, width, height) else { return false };
        if width.abs() < 1e-9 || height.abs() < 1e-9 {
            return false;
        }
        self.entities_mut(ctx).instances[inst].xform = x;
        true
    }

    /// Moves an image's first corner to `origin`.
    pub fn set_image_origin(&mut self, ctx: Option<DefId>, inst: InstanceId, origin: DVec3) -> bool {
        if self.image_of(ctx, inst).is_none() {
            return false;
        }
        self.entities_mut(ctx).instances[inst].xform.w_axis = origin.extend(1.0);
        true
    }

    /// Replaces an image's transformation (`Image#transformation=`); a degenerate one is
    /// refused.
    pub fn set_image_transform(&mut self, ctx: Option<DefId>, inst: InstanceId, xform: DMat4) -> bool {
        if self.image_of(ctx, inst).is_none() || xform.determinant().abs() < 1e-18 {
            return false;
        }
        self.entities_mut(ctx).instances[inst].xform = xform;
        true
    }

    /// Images created before the definition flag existed kept their file in instance
    /// properties (`image`, `width`, `height`, `pixelwidth`, `pixelheight`, see `Entities::props`)
    /// and had no pixels. They become images without a picture (a blank rectangle of the
    /// right size), so that scripts still see an `Image`.
    pub fn migrate_legacy_images(&mut self) {
        let mut ctxs: Vec<Option<DefId>> = vec![None];
        ctxs.extend(self.defs.keys().map(Some));
        for ctx in ctxs {
            let found: Vec<(InstanceId, String, String, [u32; 2])> = {
                let e = self.entities(ctx);
                e.instances
                    .iter()
                    .filter_map(|(id, inst)| {
                        let key = format!("i{}", slotmap::Key::data(&id).as_ffi());
                        let p = e.props.get(&key)?;
                        let path = match p.get("image")? {
                            crate::model::AttrValue::Text(s) => s.clone(),
                            _ => return None,
                        };
                        let num = |k: &str| match p.get(k) {
                            Some(crate::model::AttrValue::Number(n)) => *n,
                            _ => 0.0,
                        };
                        let px = [num("pixelwidth").max(1.0) as u32, num("pixelheight").max(1.0) as u32];
                        (!self.is_image_def(inst.def)).then_some((id, key, path, px))
                    })
                    .collect()
            };
            for (id, key, path, px) in found {
                let def = self.entities(ctx).instances[id].def;
                // The old rectangle was drawn at its final size, without scaling.
                let old = self.defs[def].entities.verts.values().fold(DVec2::ZERO, |a, v| a.max(v.pos.truncate()));
                let nat = DVec2::new(px[0] as f64, px[1] as f64) * PX_MM;
                if old.x <= 0.0 || old.y <= 0.0 {
                    continue;
                }
                let d = &mut self.defs[def];
                d.entities = rectangle(nat.x, nat.y, None);
                d.ext.image = Some(ImageDef { path, px });
                let e = self.entities_mut(ctx);
                let inst = &mut e.instances[id];
                inst.xform *= DMat4::from_scale(DVec3::new(old.x / nat.x, old.y / nat.y, 1.0));
                e.props.remove(&key);
            }
        }
    }
}

/// Texture coordinates of the faces of a collection placed by `m` once exploded into the parent:
/// the textured sides keep their look. `inherited` is the instance's material.
pub fn exploded_tex(
    model: &Model,
    face: &crate::model::Face,
    inherited: Option<MaterialId>,
    m: &DMat4,
) -> Option<Box<FaceTex>> {
    let tile = |mat: Option<MaterialId>| {
        mat.and_then(|x| model.materials.get(x)).and_then(|x| x.ext.texture.as_ref()).map(|t| t.size)
    };
    let old = face.tex.as_deref().cloned().unwrap_or_default();
    let side = |map: Option<UvMap>, mat: Option<MaterialId>| {
        let size = tile(mat)?;
        let map = map.unwrap_or_else(|| default_uv(face.normal, size));
        Some(transform_uv(&map, m))
    };
    let front = side(old.front, face.front.or(inherited));
    let back = side(old.back, face.back);
    let projection = old.projection.map(|d| d.map(|d| m.transform_vector3(d).normalize_or_zero()));
    let pins = old.pins.map(|side| side.into_iter().map(|(p, uv)| (m.transform_point3(p), uv)).collect());
    (front.is_some() || back.is_some() || projection.iter().any(Option::is_some))
        .then(|| Box::new(FaceTex { front, back, projection, pins }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntityRef;
    use crate::photo::Blob;

    fn tex(w: u32, h: u32) -> TextureData {
        TextureData {
            filename: "plan.png".into(),
            image: Blob(std::sync::Arc::new(vec![1, 2, 3])),
            px: [w, h],
            size: [0.0, 0.0],
            avg: [10, 20, 30],
        }
    }

    fn sample() -> (Model, InstanceId) {
        let mut m = Model::new();
        let i = m
            .add_image(
                None,
                "plan",
                "C:\\images\\plan.png",
                tex(200, 100),
                DVec3::new(100.0, 50.0, 0.0),
                DVec3::X,
                DVec3::Z,
                1000.0,
                None,
            )
            .unwrap();
        (m, i)
    }

    #[test]
    fn an_image_is_a_flagged_definition_with_a_textured_rectangle() {
        let (m, i) = sample();
        let inst = &m.root.instances[i];
        let d = &m.defs[inst.def];
        assert_eq!(d.ext.image, Some(ImageDef { path: "C:\\images\\plan.png".into(), px: [200, 100] }));
        assert_eq!((d.entities.faces.len(), d.entities.edges.len()), (1, 4));
        let f = d.entities.faces.values().next().unwrap();
        assert!(f.normal.distance(DVec3::Z) < 1e-12);
        let mat = &m.materials[f.front.unwrap()];
        assert_eq!(f.back, f.front);
        assert!(mat.ext.owner_image);
        let t = mat.ext.texture.as_ref().unwrap();
        assert_eq!(t.size, [200.0 * 25.4, 100.0 * 25.4], "one inch per pixel");
        // The default projection stretches the picture exactly on the rectangle.
        let uv = default_uv(f.normal, t.size);
        assert!((uv.at(DVec3::new(200.0 * 25.4, 100.0 * 25.4, 0.0)) - DVec2::ONE).length() < 1e-12);
        let v = m.image_of(None, i).unwrap();
        assert!((v.width - 1000.0).abs() < 1e-9 && (v.height - 500.0).abs() < 1e-9, "{v:?}");
        assert!(v.origin.distance(DVec3::new(100.0, 50.0, 0.0)) < 1e-12);
        assert!(v.normal.distance(DVec3::Z) < 1e-12 && v.xaxis.distance(DVec3::X) < 1e-12);
        assert_eq!(v.corners()[2], DVec3::new(1100.0, 550.0, 0.0));
        assert_eq!(v.material, f.front);
        assert!(m.image_of(None, InstanceId::default()).is_none());
    }

    #[test]
    fn images_stand_on_any_plane_and_refuse_null_sizes() {
        let mut m = Model::new();
        // On a wall facing -Y, the bottom along X: the picture goes up along Z.
        let i =
            m.add_image(None, "", "x.jpg", tex(10, 20), DVec3::ZERO, DVec3::X, -DVec3::Y, 300.0, Some(900.0)).unwrap();
        let v = m.image_of(None, i).unwrap();
        assert!(v.yaxis.distance(DVec3::Z) < 1e-12, "{v:?}");
        assert!((v.height - 900.0).abs() < 1e-9);
        assert_eq!(m.defs[v.def].name, "Image");
        assert!(m.add_image(None, "a", "a", tex(10, 10), DVec3::ZERO, DVec3::X, DVec3::Z, 0.0, None).is_none());
        assert!(m.add_image(None, "a", "a", tex(0, 10), DVec3::ZERO, DVec3::X, DVec3::Z, 5.0, None).is_none());
        assert!(m.add_image(None, "a", "a", tex(10, 10), DVec3::ZERO, DVec3::Z, DVec3::Z, 5.0, None).is_none());
        // Names stay unique.
        let j = m.add_image(None, "", "y.jpg", tex(10, 20), DVec3::ZERO, DVec3::X, DVec3::Z, 1.0, None).unwrap();
        assert_eq!(m.defs[m.root.instances[j].def].name, "Image1");
    }

    #[test]
    fn resize_move_and_transform() {
        let (mut m, i) = sample();
        assert!(m.set_image_size(None, i, 400.0, 100.0));
        let v = m.image_of(None, i).unwrap();
        assert!((v.width - 400.0).abs() < 1e-9 && (v.height - 100.0).abs() < 1e-9);
        assert!(v.origin.distance(DVec3::new(100.0, 50.0, 0.0)) < 1e-12);
        assert!(!m.set_image_size(None, i, 0.0, 100.0));
        assert!(m.set_image_origin(None, i, DVec3::new(0.0, 0.0, 30.0)));
        assert_eq!(m.image_of(None, i).unwrap().origin, DVec3::new(0.0, 0.0, 30.0));
        let rot = DMat4::from_rotation_z(std::f64::consts::FRAC_PI_2) * DMat4::from_scale(DVec3::splat(0.1));
        assert!(m.set_image_transform(None, i, rot));
        let v = m.image_of(None, i).unwrap();
        assert!(v.xaxis.distance(DVec3::Y) < 1e-12 && (v.width - 200.0 * 2.54).abs() < 1e-9, "{v:?}");
        assert!(!m.set_image_transform(None, i, DMat4::ZERO));
        // Not an image.
        let copy = m.root.instances[i].clone();
        let g = m.root.instances.insert(copy);
        m.defs[v.def].ext.image = None;
        assert!(!m.set_image_size(None, g, 1.0, 1.0) && m.image_of(None, g).is_none());
    }

    #[test]
    fn exploding_an_image_leaves_the_textured_face() {
        let (mut m, i) = sample();
        let def = m.root.instances[i].def;
        let out = m.explode(None, i);
        assert!(!m.defs.contains_key(def), "an image's definition goes with it");
        let f = out.iter().find_map(|e| if let EntityRef::Face(f) = e { Some(*f) } else { None }).unwrap();
        let face = &m.root.faces[f];
        let mat = face.front.unwrap();
        assert!(!m.materials[mat].ext.owner_image, "the material becomes an ordinary one");
        let map = face.tex.as_ref().and_then(|t| t.front).expect("positioned texture");
        let c = [
            (DVec3::new(100.0, 50.0, 0.0), DVec2::ZERO),
            (DVec3::new(1100.0, 50.0, 0.0), DVec2::X),
            (DVec3::new(1100.0, 550.0, 0.0), DVec2::ONE),
        ];
        for (p, uv) in c {
            assert!((map.at(p) - uv).length() < 1e-9, "{p} -> {}", map.at(p));
        }
        assert_eq!(out.iter().filter(|e| matches!(e, EntityRef::Edge(_))).count(), 4);
    }

    #[test]
    fn exploding_a_component_keeps_its_textures_in_place() {
        let mut m = Model::new();
        let mat = m.add_material("Brique", [200, 0, 0]);
        m.materials[mat].ext.texture = Some(TextureData { size: [100.0, 50.0], ..tex(8, 8) });
        let mut e = rectangle(300.0, 200.0, Some(mat));
        e.faces.values_mut().for_each(|f| f.back = None);
        let def = m.defs.insert(Definition {
            name: "C".into(),
            is_group: false,
            description: String::new(),
            entities: e,
            attributes: Default::default(),
            ext: Default::default(),
        });
        let xf = DMat4::from_translation(DVec3::new(30.0, 0.0, 0.0)) * DMat4::from_rotation_z(0.3);
        let i = m.root.instances.insert(Instance {
            def,
            xform: xf,
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
        let before = default_uv(DVec3::Z, [100.0, 50.0]).at(DVec3::new(120.0, 70.0, 0.0));
        let out = m.explode(None, i);
        let f = out.iter().find_map(|e| if let EntityRef::Face(f) = e { Some(*f) } else { None }).unwrap();
        let map = m.root.faces[f].tex.as_ref().and_then(|t| t.front).unwrap();
        let after = map.at(xf.transform_point3(DVec3::new(120.0, 70.0, 0.0)));
        assert!((after - before).length() < 1e-9, "{before} {after}");
        assert!(m.defs.contains_key(def), "a component definition stays");
    }

    #[test]
    fn legacy_images_in_properties_become_images() {
        let mut m = Model::new();
        let def = m.defs.insert(Definition {
            name: "vieux".into(),
            is_group: false,
            description: String::new(),
            entities: rectangle(500.0, 250.0, None),
            attributes: Default::default(),
            ext: Default::default(),
        });
        let i = m.root.instances.insert(Instance {
            def,
            xform: DMat4::from_translation(DVec3::new(1.0, 2.0, 3.0)),
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
        let key = format!("i{}", slotmap::Key::data(&i).as_ffi());
        use crate::model::AttrValue::*;
        for (k, v) in [
            ("image", Text("C:/p.png".into())),
            ("width", Number(500.0)),
            ("height", Number(250.0)),
            ("pixelwidth", Number(40.0)),
            ("pixelheight", Number(20.0)),
        ] {
            m.root.set_prop(&key, k, v);
        }
        m.migrate_legacy_images();
        let v = m.image_of(None, i).expect("an image now");
        assert_eq!((v.path.as_str(), v.px), ("C:/p.png", [40, 20]));
        assert!((v.width - 500.0).abs() < 1e-9 && (v.height - 250.0).abs() < 1e-9, "{v:?}");
        assert_eq!(v.origin, DVec3::new(1.0, 2.0, 3.0));
        assert!(m.root.props.is_empty());
        // Saved and loaded again, nothing changes.
        let json = serde_json::to_string(&m).unwrap();
        let back: Model = serde_json::from_str(&json).unwrap();
        assert_eq!(back.image_of(None, i).unwrap().px, [40, 20]);
    }
}
