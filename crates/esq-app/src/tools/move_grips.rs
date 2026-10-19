//! The rotation grips of the Move tool (SketchUp's "Show Tool Rotation Grips", command
//! "move rotate grips"): over a group or a component, the Move tool shows a grip at the
//! middle of each face of its bounding box; clicking one turns the object about the axis of
//! that face (through the grip), like the Rotate tool, angle snapped by 15°, or typed.

use eframe::egui::{Color32, Stroke};
use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::math::Plane;
use esq_core::model::{DefId, EntityRef, InstanceId, Model};

use super::{Overlay, ToolCx, circle_points};

/// Preferences › Drawing › Show Tool Rotation Grips (on by default, as in SketchUp), kept
/// with the other preferences (`settings.rs`).
pub fn grips_enabled() -> bool {
    crate::settings::with(|s| s.rotation_grips)
}

pub fn set_grips_enabled(on: bool) {
    crate::settings::update(|s| s.rotation_grips = on);
}

/// Takes the setting a previous version saved on its own (`rotation_grips.txt`), once.
pub fn load_grips_pref() {
    let Some(f) = crate::settings::user_dir().map(|d| d.join("rotation_grips.txt")) else { return };
    if let Ok(s) = std::fs::read_to_string(&f) {
        set_grips_enabled(s.trim() != "0");
        let _ = std::fs::remove_file(f);
    }
}

/// A rotation grip: the middle of a face of an object's bounding box (world coordinates).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grip {
    pub inst: InstanceId,
    pub center: DVec3,
    /// The axis of the rotation (the face's normal).
    pub normal: DVec3,
    /// Angle 0 of the protractor, in the face.
    pub u: DVec3,
    /// Radius of the protractor (half the face's smaller side).
    pub radius: f64,
}

/// Bounding box of a definition in its own coordinates (nested objects included).
pub fn def_bounds(m: &Model, def: DefId) -> Option<(DVec3, DVec3)> {
    let e = &m.defs.get(def)?.entities;
    let mut pts: Vec<DVec3> = e.verts.values().map(|v| v.pos).collect();
    for i in e.instances.values() {
        if let Some((a, b)) = def_bounds(m, i.def) {
            for c in 0..8 {
                let p = DVec3::new(
                    if c & 1 == 0 { a.x } else { b.x },
                    if c & 2 == 0 { a.y } else { b.y },
                    if c & 4 == 0 { a.z } else { b.z },
                );
                pts.push(i.xform.transform_point3(p));
            }
        }
    }
    let lo = pts.iter().copied().reduce(DVec3::min)?;
    let hi = pts.iter().copied().reduce(DVec3::max)?;
    Some((lo, hi))
}

/// The six grips of an instance of the active context.
pub fn grips_of(cx: &ToolCx, inst: InstanceId) -> Vec<Grip> {
    let Some(i) = cx.doc.ents().instances.get(inst) else { return Vec::new() };
    let Some((lo, hi)) = def_bounds(&cx.doc.model, i.def) else { return Vec::new() };
    let x = cx.doc.ctx_xform() * i.xform;
    let c = (lo + hi) * 0.5;
    let h = (hi - lo) * 0.5;
    let mut out = Vec::new();
    for axis in 0..3 {
        for s in [-1.0, 1.0] {
            let mut n = DVec3::ZERO;
            n[axis] = s;
            let (ua, va) = ((axis + 1) % 3, (axis + 2) % 3);
            let mut u = DVec3::ZERO;
            u[ua] = 1.0;
            let local = c + n * h[axis];
            let center = x.transform_point3(local);
            let normal = x.transform_vector3(n).normalize_or_zero();
            let u = x.transform_vector3(u).normalize_or_zero();
            let side = (x.transform_vector3(DVec3::AXES[ua] * h[ua]).length())
                .min(x.transform_vector3(DVec3::AXES[va] * h[va]).length());
            if normal != DVec3::ZERO {
                out.push(Grip { inst, center, normal, u, radius: side.max(1.0) });
            }
        }
    }
    out
}

/// The object whose grips show: the group or component under the cursor, else the one
/// selected alone.
pub fn grip_object(cx: &ToolCx, m: DVec2) -> Option<InstanceId> {
    let (o, d) = cx.cam().ray(m);
    let under = cx.infer.pick.ray_face(o, d).and_then(|(_, h, _)| h.selectable(&cx.doc.ctx));
    match under {
        Some(EntityRef::Instance(i)) => Some(i),
        _ => match cx.doc.sel.as_slice() {
            [EntityRef::Instance(i)] => Some(*i),
            _ => None,
        },
    }
}

/// The grip under the cursor (within 10 pixels of its middle, on a face turned to the view).
pub fn grip_under(cx: &ToolCx, m: DVec2) -> Option<Grip> {
    let inst = grip_object(cx, m)?;
    let cam = cx.cam();
    grips_of(cx, inst)
        .into_iter()
        .filter(|g| g.normal.dot(cam.eye() - g.center) > 0.0)
        .filter_map(|g| Some((cam.project(g.center)?.distance(m), g)))
        .filter(|(d, _)| *d < 10.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|x| x.1)
}

/// The angle of the cursor around a grip, snapped to 15° when within 2°.
pub fn grip_angle(cx: &ToolCx, g: &Grip, m: DVec2) -> Option<f64> {
    let (o, d) = cx.cam().ray(m);
    let t = Plane::from_point_normal(g.center, g.normal).intersect_ray(o, d)?;
    let p = o + d * t - g.center;
    let v = g.normal.cross(g.u);
    let a = p.dot(v).atan2(p.dot(g.u));
    let step = 15f64.to_radians();
    let snapped = (a / step).round() * step;
    Some(if (a - snapped).abs() < 2f64.to_radians() { snapped } else { a })
}

/// The turn about a grip, in the active context's coordinates.
pub fn grip_rotation(cx: &ToolCx, g: &Grip, angle: f64) -> DMat4 {
    let w = DMat4::from_translation(g.center)
        * DMat4::from_axis_angle(g.normal, angle)
        * DMat4::from_translation(-g.center);
    cx.local_xform(w)
}

/// The grips of the object and, on the one hovered (or turning), the protractor.
pub fn paint_grips(o: &Overlay, grips: &[Grip], active: Option<&Grip>, angle: Option<f64>) {
    let red = Color32::from_rgb(220, 30, 30);
    for g in grips {
        if let Some(p) = o.screen(g.center) {
            let k = 5.0;
            let st = Stroke::new(2.0, red);
            o.painter.line_segment([p + eframe::egui::vec2(-k, 0.0), p + eframe::egui::vec2(k, 0.0)], st);
            o.painter.line_segment([p + eframe::egui::vec2(0.0, -k), p + eframe::egui::vec2(0.0, k)], st);
        }
    }
    if let Some(g) = active {
        let pts = circle_points(g.center, g.normal, g.u, g.radius, 48);
        o.polyline(&pts, true, Color32::from_rgb(30, 50, 230), 1.5);
        o.line(g.center, g.center + g.u * g.radius, Color32::from_gray(70), 1.0);
        if let Some(a) = angle {
            let v = g.normal.cross(g.u);
            let dir = g.u * a.cos() + v * a.sin();
            o.line(g.center, g.center + dir * g.radius, Color32::from_gray(30), 1.5);
            o.text_at(g.center + dir * g.radius, &format!("{:.1}°", a.to_degrees()));
        }
    }
}
