//! Tools › Sample Material (SketchUp's `CPaintMatchTool`): a click on a face loads its
//! material into the Paint Bucket, which then takes over. The Paint Bucket's Alt-click samples
//! the same way.
//!
//! Like SketchUp (`CFaceSideTextureInfo`), the sample also remembers how the texture lies on
//! the sampled face: painting that material afterwards on a face of the same plane continues
//! the texture there, and a projected texture (Texture › Projected) is projected onto any
//! face painted with it.

use std::cell::RefCell;

use eframe::egui::Color32;
use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::image::transform_uv;
use esq_core::meta::{FaceTex, UvMap, side_uv};
use esq_core::model::{Entities, EntityRef, FaceId, InstanceId, MaterialId, Model};
use esq_core::walk::{path_def, path_xform};

use super::{Overlay, Tool, ToolCx, ToolId};

/// How the texture lay on the sampled face, in world coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct SampledTex {
    pub map: UvMap,
    /// A point and the normal of the sampled side's plane.
    pub origin: DVec3,
    pub normal: DVec3,
    /// Projection direction of a projected texture.
    pub projection: Option<DVec3>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub material: Option<MaterialId>,
    pub tex: Option<SampledTex>,
}

thread_local! {
    static SAMPLE: RefCell<Option<Sample>> = const { RefCell::new(None) };
}

/// The last sample taken (this thread: the window, or one test).
pub fn last() -> Option<Sample> {
    SAMPLE.with(|s| s.borrow().clone())
}

pub fn forget() {
    SAMPLE.with(|s| *s.borrow_mut() = None);
}

/// The texture map a face side shows, in its own collection: the positioned one, else the
/// default one (as the renderer draws it).
pub fn side_map(model: &Model, ents: &Entities, f: FaceId, back: bool) -> Option<UvMap> {
    let face = ents.faces.get(f)?;
    let mat = if back { face.back } else { face.front }?;
    let tile = model.materials.get(mat)?.ext.texture.as_ref()?.size;
    Some(side_uv(face.tex.as_deref(), !back, face.normal, Some(tile)))
}

/// Samples what is under the cursor (a face of any depth, or a group's material) into the
/// Paint Bucket's material. Returns false if there is nothing there.
pub fn sample_at(cx: &mut ToolCx, m: DVec2) -> bool {
    let (o, d) = cx.cam().ray(m);
    let Some((_, hit, _)) = cx.infer.pick.ray_face(o, d) else { return false };
    let (path, item) = (hit.path.clone(), hit.item);
    let EntityRef::Face(f) = item else { return false };
    let model = &cx.doc.model;
    let Some(ents) = path_def(model, &path).map(|d| &model.defs[d].entities).or(path.is_empty().then_some(&model.root))
    else {
        return false;
    };
    let Some(face) = ents.faces.get(f) else { return false };
    let x = path_xform(model, &path);
    let n_world = x.transform_vector3(face.normal).normalize_or_zero();
    // The side the camera sees.
    let back = d.dot(n_world) > 0.0;
    let own = if back { face.back } else { face.front };
    let material = own.or_else(|| inherited(model, &path));
    let tex = own.and_then(|_| {
        let map = side_map(model, ents, f, back)?;
        let projection = face.tex.as_deref().and_then(|t| t.projection[back as usize]);
        Some(SampledTex {
            map: transform_uv(&map, &x),
            origin: x.transform_point3(ents.pos(face.outer[0])),
            normal: if back { -n_world } else { n_world },
            projection: projection.map(|p| x.transform_vector3(p).normalize_or_zero()),
        })
    });
    *cx.material = material;
    SAMPLE.with(|s| *s.borrow_mut() = Some(Sample { material, tex }));
    true
}

/// The material a face without one shows: that of the innermost painted group around it.
fn inherited(model: &Model, path: &[InstanceId]) -> Option<MaterialId> {
    let mut ents = &model.root;
    let mut found = None;
    for &i in path {
        let inst = ents.instances.get(i)?;
        found = inst.material.or(found);
        ents = &model.defs.get(inst.def)?.entities;
    }
    found
}

/// After the Paint Bucket gave `mat` to the front of face `f` (collection placed by `x`):
/// with a sample of that material, continue its texture on a face of the same plane, or
/// project it if it was projected.
pub fn align_painted(ents: &mut Entities, f: FaceId, mat: Option<MaterialId>, x: &DMat4) {
    let Some(s) = last() else { return };
    let (Some(t), true) = (s.tex, s.material == mat && mat.is_some()) else { return };
    let Some(face) = ents.faces.get(f) else { return };
    let n = x.transform_vector3(face.normal).normalize_or_zero();
    let p = x.transform_point3(ents.pos(face.outer[0]));
    let inv = x.inverse();
    let (map, dir) = match t.projection {
        Some(d) => match t.map.projected(t.origin, t.normal, d) {
            Some(m) => (m, Some(inv.transform_vector3(d).normalize_or_zero())),
            None => return,
        },
        None if n.dot(t.normal).abs() > 1.0 - 1e-9 && (p - t.origin).dot(t.normal).abs() < 1e-3 => (t.map, None),
        None => return,
    };
    let local = transform_uv(&map, &inv);
    let face = ents.faces.get_mut(f).unwrap();
    let tex = face.tex.get_or_insert_with(|| Box::new(FaceTex::default()));
    tex.front = Some(local);
    tex.projection[0] = dir;
    tex.pins[0].clear();
}

/// The colour a face shows under the cursor (Materials › Edit › "Match Color of object in
/// model"): its material's colour, or the default face colour.
pub fn color_at(cx: &ToolCx, m: DVec2) -> Option<[u8; 3]> {
    let (o, d) = cx.cam().ray(m);
    let (_, hit, _) = cx.infer.pick.ray_face(o, d)?;
    let EntityRef::Face(f) = hit.item else { return None };
    let model = &cx.doc.model;
    let ents = match path_def(model, &hit.path) {
        Some(def) => &model.defs[def].entities,
        None => &model.root,
    };
    let face = ents.faces.get(f)?;
    let x = path_xform(model, &hit.path);
    let back = d.dot(x.transform_vector3(face.normal)) > 0.0;
    let mat = if back { face.back } else { face.front }.or_else(|| inherited(model, &hit.path));
    Some(match mat.and_then(|id| model.materials.get(id)) {
        Some(m) => m.color,
        None if back => crate::scene::BACK,
        None => crate::scene::FRONT,
    })
}

/// Tools › Sample Material (`CPaintMatchTool`); the same tool, made by the Materials panel,
/// samples a colour for the material being edited.
#[derive(Default)]
pub struct SampleTool {
    hover: Option<FaceId>,
    done: bool,
    /// Match Color of object in model: the material that takes the colour, and the tool to
    /// go back to.
    color_for: Option<(MaterialId, ToolId)>,
}

impl SampleTool {
    pub fn for_color(m: MaterialId, back_to: ToolId) -> Self {
        SampleTool { color_for: Some((m, back_to)), ..Default::default() }
    }
}

impl Tool for SampleTool {
    fn id(&self) -> ToolId {
        ToolId::SampleMaterial
    }
    fn status(&self) -> String {
        "Cliquez sur une face pour charger sa matière dans l'outil Colorier.".into()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        let (o, d) = cx.cam().ray(m);
        self.hover = cx.infer.pick.ray_face(o, d).and_then(|(_, h, _)| h.face_in(&cx.doc.ctx));
    }
    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        if let Some((id, _)) = self.color_for {
            if let Some(c) = color_at(cx, m)
                && cx.doc.model.materials.contains_key(id)
            {
                cx.doc.commit("Modifier la matière", |md, _| crate::mat_panel::set_color(&mut md.materials[id], c));
                self.done = true;
            }
            return;
        }
        if sample_at(cx, m) {
            self.done = true;
        }
    }
    fn switch_to(&mut self) -> Option<ToolId> {
        let back = self.color_for.map_or(ToolId::Paint, |c| c.1);
        std::mem::take(&mut self.done).then_some(back)
    }
    fn overlay(&self, o: &mut Overlay) {
        if let Some(f) = self.hover {
            o.fill_face(f, Color32::from_rgba_unmultiplied(255, 160, 0, 50));
        }
    }
}
