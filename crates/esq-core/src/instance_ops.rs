//! Commands of the right-click menu on a group, a component or an image (SketchUp.exe):
//! Reset Scale, Reset Skew, Scale Definition, Change
//! Axes, Unglue, and Make Component on a group.
//!
//! - an instance is "scaled" when an axis of its transformation is not of unit length or
//!   when it is mirrored, "skewed" when its axes are not perpendicular
//! - Reset Scale keeps the position, the orientation and the skew; a mirror goes too;
//! - Scale Definition puts the instance's scale into the definition (a mirror along red):
//!   the instance keeps its size and loses its scale, the other copies take the new size
//! - Change Axes (`CComponentCSTool`) moves the definition's axes: the geometry does not move
//!   on screen, for any copy.

use glam::{DMat4, DVec3};

use crate::model::{DefId, Entities, FaceId, InstanceId, Model};

const EPS: f64 = 1e-9;

/// The three axes of a transformation (its first three columns).
fn axes(m: &DMat4) -> [DVec3; 3] {
    [m.x_axis.truncate(), m.y_axis.truncate(), m.z_axis.truncate()]
}

/// Scale along each axis; the first one is negative for a mirror.
pub fn scale_of(m: &DMat4) -> DVec3 {
    let a = axes(m);
    let s = DVec3::new(a[0].length(), a[1].length(), a[2].length());
    if m.determinant() < 0.0 { DVec3::new(-s.x, s.y, s.z) } else { s }
}

/// Whether Reset Scale and Scale Definition have something to do.
pub fn is_scaled(m: &DMat4) -> bool {
    let s = scale_of(m);
    (s - DVec3::ONE).abs().max_element() > 1e-6
}

/// Whether Reset Skew has something to do.
pub fn is_skewed(m: &DMat4) -> bool {
    let a = axes(m);
    let cos = |i: usize, j: usize| {
        let l = a[i].length() * a[j].length();
        if l < EPS { 0.0 } else { (a[i].dot(a[j]) / l).abs() }
    };
    cos(0, 1).max(cos(0, 2)).max(cos(1, 2)) > 1e-6
}

/// `m` without its scale (nor mirror): unit axes, same skew, same origin.
pub fn without_scale(m: &DMat4) -> DMat4 {
    let s = scale_of(m);
    *m * DMat4::from_scale(DVec3::ONE / s)
}

/// `m` with perpendicular axes of the same lengths: the first axis stays, the second is made
/// perpendicular to it in their plane, the third completes them (same handedness).
pub fn without_skew(m: &DMat4) -> DMat4 {
    let a = axes(m);
    let u0 = a[0].normalize_or(DVec3::X);
    let u1 = (a[1] - u0 * a[1].dot(u0)).normalize_or(u0.any_orthonormal_vector());
    let sign = if m.determinant() < 0.0 { -1.0 } else { 1.0 };
    let u2 = u0.cross(u1) * sign;
    DMat4::from_cols(
        (u0 * a[0].length()).extend(0.0),
        (u1 * a[1].length()).extend(0.0),
        (u2 * a[2].length()).extend(0.0),
        m.w_axis,
    )
}

/// Applies `t` to everything a collection holds: vertices, faces (and their textures),
/// instances, guides, dimensions and texts, section planes, snaps and lights.
pub fn transform_entities(e: &mut Entities, t: &DMat4) {
    for v in e.verts.values_mut() {
        v.pos = t.transform_point3(v.pos);
    }
    let mirror = t.determinant() < 0.0;
    let faces: Vec<FaceId> = e.faces.keys().collect();
    for f in faces {
        let face = &mut e.faces[f];
        if mirror {
            face.outer.reverse();
            face.holes.iter_mut().for_each(|h| h.reverse());
        }
        if let Some(tex) = face.tex.as_mut() {
            tex.front = tex.front.map(|m| crate::image::transform_uv(&m, t));
            tex.back = tex.back.map(|m| crate::image::transform_uv(&m, t));
            for d in tex.projection.iter_mut().flatten() {
                *d = t.transform_vector3(*d);
            }
            for side in tex.pins.iter_mut() {
                for (p, _) in side.iter_mut() {
                    *p = t.transform_point3(*p);
                }
            }
        }
        e.recompute_normal(f);
    }
    for i in e.instances.values_mut() {
        i.xform = *t * i.xform;
    }
    for g in e.guides.values_mut() {
        *g = g.transformed(t);
    }
    for a in e.annots.values_mut() {
        *a = a.transformed(t);
    }
    for s in e.sections.values_mut() {
        *s = s.transformed(t);
    }
    for s in e.snaps.iter_mut() {
        *s = s.transformed(t);
    }
    for l in e.lights.iter_mut() {
        l.pos = t.transform_point3(l.pos);
    }
}

impl Model {
    fn inst_xform(&self, ctx: Option<DefId>, inst: InstanceId) -> Option<DMat4> {
        self.entities(ctx).instances.get(inst).map(|i| i.xform)
    }

    /// Reset Scale. Returns false when the instance is not scaled (or locked).
    pub fn reset_scale(&mut self, ctx: Option<DefId>, inst: InstanceId) -> bool {
        let Some(x) = self.inst_xform(ctx, inst).filter(is_scaled) else { return false };
        let i = &mut self.entities_mut(ctx).instances[inst];
        if i.locked {
            return false;
        }
        i.xform = without_scale(&x);
        true
    }

    /// Reset Skew. Returns false when the instance's axes are already perpendicular.
    pub fn reset_skew(&mut self, ctx: Option<DefId>, inst: InstanceId) -> bool {
        let Some(x) = self.inst_xform(ctx, inst).filter(is_skewed) else { return false };
        let i = &mut self.entities_mut(ctx).instances[inst];
        if i.locked {
            return false;
        }
        i.xform = without_skew(&x);
        true
    }

    /// Scale Definition: the instance's scale goes into its definition.
    pub fn scale_definition(&mut self, ctx: Option<DefId>, inst: InstanceId) -> bool {
        let Some(x) = self.inst_xform(ctx, inst).filter(is_scaled) else { return false };
        let i = &self.entities(ctx).instances[inst];
        if i.locked {
            return false;
        }
        let def = i.def;
        let s = DMat4::from_scale(scale_of(&x));
        transform_entities(&mut self.defs[def].entities, &s);
        self.entities_mut(ctx).instances[inst].xform = x * s.inverse();
        true
    }

    /// Change Axes: `frame` (in the definition's coordinates) becomes the definition's
    /// origin and axes. The geometry is moved by the inverse and every copy by `frame`, so
    /// that nothing moves on screen.
    pub fn change_definition_axes(&mut self, def: DefId, frame: DMat4) -> bool {
        if frame.determinant().abs() < EPS || !self.defs.contains_key(def) {
            return false;
        }
        transform_entities(&mut self.defs[def].entities, &frame.inverse());
        let fix = |e: &mut Entities| {
            for i in e.instances.values_mut().filter(|i| i.def == def) {
                i.xform *= frame;
            }
        };
        fix(&mut self.root);
        for d in self.defs.values_mut() {
            fix(&mut d.entities);
        }
        true
    }

    /// Unglue: the instance no longer follows the face it was glued to.
    pub fn unglue(&mut self, ctx: Option<DefId>, inst: InstanceId) -> bool {
        match self.entities_mut(ctx).instances.get_mut(inst) {
            Some(i) if i.ext.glued_to.is_some() && !i.locked => {
                i.ext.glued_to = None;
                true
            }
            _ => false,
        }
    }

    /// A group's Make Component: the group becomes a component (its own definition when the
    /// group's one is shared), in place; returns the new instance.
    pub fn group_to_component(&mut self, ctx: Option<DefId>, inst: InstanceId) -> Option<InstanceId> {
        let mut i = self.entities(ctx).instances.get(inst)?.clone();
        if !self.defs.get(i.def)?.is_group || i.locked {
            return None;
        }
        if self.instance_count(i.def) > 1 {
            let copy = self.defs[i.def].clone();
            i.def = self.defs.insert(copy);
        }
        let name = self.unique_def_name("Composant");
        let d = &mut self.defs[i.def];
        d.is_group = false;
        d.name = name;
        i.ext.glued_to = None;
        let e = self.entities_mut(ctx);
        e.instances.remove(inst);
        Some(e.instances.insert(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Entities, Instance};

    fn model_with_box() -> (Model, DefId, InstanceId) {
        let mut m = Model::new();
        let mut e = Entities::default();
        e.add_polyline(
            &[DVec3::ZERO, DVec3::new(100.0, 0.0, 0.0), DVec3::new(100.0, 50.0, 0.0), DVec3::new(0.0, 50.0, 0.0)],
            true,
            false,
        );
        let def = m.defs.insert(crate::model::Definition {
            name: "Boîte".into(),
            is_group: false,
            description: String::new(),
            entities: e,
            attributes: Default::default(),
            ext: Default::default(),
        });
        let inst = m.root.instances.insert(Instance {
            def,
            xform: DMat4::from_translation(DVec3::new(10.0, 20.0, 0.0)),
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
        (m, def, inst)
    }

    #[test]
    fn scale_skew_and_mirror_are_told_apart() {
        let t = DMat4::from_translation(DVec3::new(5.0, 0.0, 0.0));
        assert!(!is_scaled(&t) && !is_skewed(&t));
        assert!(is_scaled(&(t * DMat4::from_scale(DVec3::new(2.0, 1.0, 1.0)))));
        assert!(is_scaled(&DMat4::from_scale(DVec3::new(-1.0, 1.0, 1.0))), "a mirror counts as a scale");
        let skew =
            DMat4::from_cols_array(&[1.0, 0.0, 0.0, 0.0, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        assert!(is_skewed(&skew));
        let fixed = without_skew(&skew);
        assert!(!is_skewed(&fixed));
        assert!((fixed.y_axis.truncate().length() - skew.y_axis.truncate().length()).abs() < 1e-9);
    }

    #[test]
    fn reset_scale_keeps_place_and_orientation() {
        let (mut m, _, inst) = model_with_box();
        let rot = DMat4::from_rotation_z(0.5);
        m.root.instances[inst].xform =
            DMat4::from_translation(DVec3::new(10.0, 20.0, 0.0)) * rot * DMat4::from_scale(DVec3::new(2.0, 3.0, 1.0));
        assert!(m.reset_scale(None, inst));
        let x = m.root.instances[inst].xform;
        assert!(x.abs_diff_eq(DMat4::from_translation(DVec3::new(10.0, 20.0, 0.0)) * rot, 1e-9), "{x:?}");
        assert!(!m.reset_scale(None, inst), "nothing left to reset");
    }

    #[test]
    fn scale_definition_moves_the_scale_into_the_definition() {
        let (mut m, def, inst) = model_with_box();
        let other = m.root.instances.insert(m.root.instances[inst].clone());
        m.root.instances[inst].xform *= DMat4::from_scale(DVec3::new(2.0, 1.0, 1.0));
        assert!(m.scale_definition(None, inst));
        // The scaled copy keeps its size and has no scale left; the definition is 200 wide.
        assert!(!is_scaled(&m.root.instances[inst].xform));
        let max_x = m.defs[def].entities.verts.values().map(|v| v.pos.x).fold(0.0, f64::max);
        assert!((max_x - 200.0).abs() < 1e-9);
        // The other copy, not scaled, now shows the new size.
        assert!(!is_scaled(&m.root.instances[other].xform));
    }

    #[test]
    fn change_axes_keeps_the_geometry_in_place() {
        let (mut m, def, inst) = model_with_box();
        let world = |m: &Model| -> Vec<DVec3> {
            let x = m.root.instances[inst].xform;
            let mut p: Vec<DVec3> = m.defs[def].entities.verts.values().map(|v| x.transform_point3(v.pos)).collect();
            p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
            p
        };
        let before = world(&m);
        // New origin at the far corner, red along the old green.
        let frame = DMat4::from_cols(
            DVec3::Y.extend(0.0),
            (-DVec3::X).extend(0.0),
            DVec3::Z.extend(0.0),
            DVec3::new(100.0, 50.0, 0.0).extend(1.0),
        );
        assert!(m.change_definition_axes(def, frame));
        let after = world(&m);
        for (a, b) in before.iter().zip(&after) {
            assert!(a.distance(*b) < 1e-9, "{a} {b}");
        }
        assert!(m.defs[def].entities.verts.values().any(|v| v.pos.length() < 1e-9), "the corner is the new origin");
    }

    #[test]
    fn a_group_becomes_a_component_in_place() {
        let (mut m, def, inst) = model_with_box();
        m.defs[def].is_group = true;
        let x = m.root.instances[inst].xform;
        let n = m.group_to_component(None, inst).expect("component");
        let i = &m.root.instances[n];
        assert!(!m.defs[i.def].is_group && i.xform == x);
        assert!(m.root.instances.get(inst).is_none());
    }
}
