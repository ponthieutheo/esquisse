//! Walking the instance tree with world transforms.

use glam::{DMat4, DVec3};

use crate::model::{DefId, Entities, InstanceId, MaterialId, Model, SectionPlane};

/// One entities collection as it appears in the world.
pub struct Placed<'a> {
    pub ents: &'a Entities,
    pub def: Option<DefId>,
    pub xform: DMat4,
    /// Material painted on the enclosing instance, used by faces with no material.
    pub inherited: Option<MaterialId>,
    pub path: Vec<InstanceId>,
    /// Reached through a hidden instance (only with `visit_all`).
    pub hidden: bool,
    /// Active section planes cutting this collection (its own and its parents'), in world
    /// coordinates.
    pub cuts: Vec<SectionPlane>,
}

thread_local! {
    /// The camera's eye, for components that always face the camera (`set_eye`).
    static EYE: std::cell::Cell<Option<DVec3>> = const { std::cell::Cell::new(None) };
}

/// Where the camera is: components whose behaviour is "always face camera" turn about their
/// blue axis towards it, wherever the instance tree is walked on this thread (display,
/// picking). `None`: they stay as placed.
pub fn set_eye(eye: Option<DVec3>) {
    EYE.with(|e| e.set(eye));
}

pub fn eye() -> Option<DVec3> {
    EYE.with(|e| e.get())
}

/// The turn about its blue axis that brings the front (-Y side) of a component placed at
/// `world` to face `eye`.
pub fn face_camera(world: &DMat4, eye: DVec3) -> DMat4 {
    let e = world.inverse().transform_point3(eye);
    if e.x.hypot(e.y) < 1e-9 {
        return DMat4::IDENTITY;
    }
    DMat4::from_rotation_z(e.y.atan2(e.x) + std::f64::consts::FRAC_PI_2)
}

fn with_cut(mut cuts: Vec<SectionPlane>, ents: &Entities, xform: &DMat4) -> Vec<SectionPlane> {
    if let Some(s) = ents.active_section.and_then(|s| ents.sections.get(s)) {
        cuts.push(s.transformed(xform));
    }
    cuts
}

/// Calls `f` for the root and for every visible instance, depth first.
pub fn visit<'a>(model: &'a Model, f: impl FnMut(&Placed<'a>)) {
    visit_all(model, false, f)
}

/// Like `visit`, and with `hidden_objects` also enters hidden instances (View › Hidden
/// Objects); those collections are flagged `hidden`.
pub fn visit_all<'a>(model: &'a Model, hidden_objects: bool, mut f: impl FnMut(&Placed<'a>)) {
    fn rec<'a>(model: &'a Model, p: Placed<'a>, all: bool, f: &mut impl FnMut(&Placed<'a>), depth: usize) {
        f(&p);
        if depth > 64 {
            return;
        }
        for (id, inst) in p.ents.instances.iter() {
            if (inst.hidden && !all) || !model.layers.get(inst.layer).is_none_or(|l| l.visible) {
                continue;
            }
            let Some(def) = model.defs.get(inst.def) else { continue };
            let mut path = p.path.clone();
            path.push(id);
            let mut xform = p.xform * inst.xform;
            if def.ext.behavior.always_face_camera
                && let Some(e) = eye()
            {
                xform *= face_camera(&xform, e);
            }
            rec(
                model,
                Placed {
                    ents: &def.entities,
                    def: Some(inst.def),
                    cuts: with_cut(p.cuts.clone(), &def.entities, &xform),
                    xform,
                    inherited: inst.material.or(p.inherited),
                    path,
                    hidden: p.hidden || inst.hidden,
                },
                all,
                f,
                depth + 1,
            );
        }
    }
    rec(
        model,
        Placed {
            ents: &model.root,
            def: None,
            xform: DMat4::IDENTITY,
            inherited: None,
            path: Vec::new(),
            hidden: false,
            cuts: with_cut(Vec::new(), &model.root, &DMat4::IDENTITY),
        },
        hidden_objects,
        &mut f,
        0,
    );
}

/// World transform of the collection reached through `path` (instance ids from the root).
pub fn path_xform(model: &Model, path: &[InstanceId]) -> DMat4 {
    let mut m = DMat4::IDENTITY;
    let mut ents = &model.root;
    for &i in path {
        let Some(inst) = ents.instances.get(i) else { break };
        m *= inst.xform;
        ents = &model.defs[inst.def].entities;
    }
    m
}

/// Definition reached through `path` (None = root).
pub fn path_def(model: &Model, path: &[InstanceId]) -> Option<DefId> {
    let mut ents = &model.root;
    let mut def = None;
    for &i in path {
        let inst = ents.instances.get(i)?;
        def = Some(inst.def);
        ents = &model.defs[inst.def].entities;
    }
    def
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_component_turns_its_front_to_the_eye() {
        let front = |world: DMat4, eye: DVec3| {
            let m = world * face_camera(&world, eye);
            m.transform_vector3(-DVec3::Y).normalize()
        };
        let at = DMat4::from_translation(DVec3::new(100.0, 0.0, 0.0));
        assert!(front(at, DVec3::new(100.0, -500.0, 50.0)).abs_diff_eq(-DVec3::Y, 1e-9));
        assert!(front(at, DVec3::new(900.0, 0.0, 300.0)).abs_diff_eq(DVec3::X, 1e-9));
        // Turned about its own blue axis only: Z stays.
        let m = at * face_camera(&at, DVec3::new(-50.0, 70.0, 900.0));
        assert!(m.transform_vector3(DVec3::Z).abs_diff_eq(DVec3::Z, 1e-9));
    }
}
