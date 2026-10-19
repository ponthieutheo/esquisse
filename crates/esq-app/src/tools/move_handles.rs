//! What the Move tool takes a selected group or component by, and how a component connects
//! to another one by its snaps, read from CMoveTool (SketchUp 2026).
//!
//! Before the first click, over a selected group or component (its bounding box under the
//! cursor), the tool offers within 12 pixels (twice the 6-pixel aperture): its snaps, its
//! origin, and the grips of its bounding box of the current type — corners, midpoints of the
//! box's edges, centres of its sides, its centre; Alt cycles the type.
//! The grips show only when the box is more than 24 pixels wide on screen.
//!
//! A move started at a snap connects: over another component's snap (12 px),
//! the moved object turns so that its snap faces that one (opposite directions, same up) and
//! the two snaps meet; over a face, its snap points into the face, its up toward the blue axis.
//! The "tractor beam" (Alt while moving) is the nearest snap on screen of the
//! other components: when it is on, the object already turns toward it.

use eframe::egui::{Color32, Stroke, vec2};
use esq_core::glam::{DMat3, DMat4, DVec2, DVec3};
use esq_core::model::{EntityRef, InstanceId, SnapPoint};

use super::move_grips::def_bounds;
use super::{Overlay, ToolCx};
use crate::pick::{Handle, Snap, SnapKind};

/// Pick distance of snaps, origins and grips (CMoveTool: 2 × the 6-pixel aperture).
pub const HANDLE_PX: f64 = 12.0;
/// The grips show when one side of the box is longer than this on screen (4 × 6 pixels).
const MIN_BOX_PX: f64 = 24.0;

/// The grips of the bounding box offered at a time (Alt cycles them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GripKind {
    #[default]
    Corners,
    Midpoints,
    SideCenters,
    Center,
}

impl GripKind {
    pub fn next(self) -> Self {
        match self {
            GripKind::Corners => GripKind::Midpoints,
            GripKind::Midpoints => GripKind::SideCenters,
            GripKind::SideCenters => GripKind::Center,
            GripKind::Center => GripKind::Corners,
        }
    }

    fn handle(self) -> Handle {
        match self {
            GripKind::Corners => Handle::Corner,
            GripKind::Midpoints => Handle::Midpoint,
            GripKind::SideCenters => Handle::SideCenter,
            GripKind::Center => Handle::Center,
        }
    }
}

/// The world transform of an instance of the active context.
fn world(cx: &ToolCx, inst: InstanceId) -> Option<DMat4> {
    cx.doc.ents().instances.get(inst).map(|i| cx.doc.ctx_xform() * i.xform)
}

fn corners(lo: DVec3, hi: DVec3) -> [DVec3; 8] {
    std::array::from_fn(|c| {
        DVec3::new(
            if c & 1 == 0 { lo.x } else { hi.x },
            if c & 2 == 0 { lo.y } else { hi.y },
            if c & 4 == 0 { lo.z } else { hi.z },
        )
    })
}

/// The grips of `kind` of an instance of the active context (world coordinates).
pub fn grip_points(cx: &ToolCx, inst: InstanceId, kind: GripKind) -> Vec<DVec3> {
    let Some(x) = world(cx, inst) else { return Vec::new() };
    let Some((lo, hi)) = cx.doc.ents().instances.get(inst).and_then(|i| def_bounds(&cx.doc.model, i.def)) else {
        return Vec::new();
    };
    let c = corners(lo, hi);
    let local: Vec<DVec3> = match kind {
        GripKind::Corners => c.to_vec(),
        // Corners differing by one bit share an edge of the box.
        GripKind::Midpoints => (0..8)
            .flat_map(|a| [1, 2, 4].into_iter().filter(move |b| a & b == 0).map(move |b| (a, a | b)))
            .map(|(a, b)| (c[a] + c[b]) * 0.5)
            .collect(),
        GripKind::SideCenters => (0..3)
            .flat_map(|axis| [lo[axis], hi[axis]].map(|v| (axis, v)))
            .map(|(axis, v)| {
                let mut p = (lo + hi) * 0.5;
                p[axis] = v;
                p
            })
            .collect(),
        GripKind::Center => vec![(lo + hi) * 0.5],
    };
    local.into_iter().map(|p| x.transform_point3(p)).collect()
}

/// The grips drawn on an object: none while its box is too small on screen.
pub fn shown_grips(cx: &ToolCx, inst: InstanceId, kind: GripKind) -> Vec<DVec3> {
    if box_big_enough(cx, inst) { grip_points(cx, inst, kind) } else { Vec::new() }
}

/// True when the box of the instance is large enough on screen for its grips to show.
fn box_big_enough(cx: &ToolCx, inst: InstanceId) -> bool {
    let pts = grip_points(cx, inst, GripKind::Corners);
    let px: Vec<Option<DVec2>> = pts.iter().map(|p| cx.cam().project(*p)).collect();
    let Some(o) = px[0] else { return false };
    [1, 2, 4].iter().any(|&i| px[i].is_some_and(|q| q.distance(o) > MIN_BOX_PX))
}

/// The snaps of an instance of the active context (world coordinates).
pub fn instance_snaps(cx: &ToolCx, inst: InstanceId) -> Vec<SnapPoint> {
    let Some(x) = world(cx, inst) else { return Vec::new() };
    let def = cx.doc.ents().instances[inst].def;
    let Some(d) = cx.doc.model.defs.get(def) else { return Vec::new() };
    d.entities.snaps.iter().filter(|s| super::snap::shown(&cx.doc.model, s)).map(|s| s.transformed(&x)).collect()
}

/// Where the ray of the cursor enters the bounding box of an instance, if it does.
fn box_hit(cx: &ToolCx, inst: InstanceId, m: DVec2) -> Option<f64> {
    let x = world(cx, inst)?;
    let (lo, hi) = def_bounds(&cx.doc.model, cx.doc.ents().instances[inst].def)?;
    let inv = x.inverse();
    let (ro, rd) = cx.cam().ray(m);
    let (o, d) = (inv.transform_point3(ro), inv.transform_vector3(rd));
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    for a in 0..3 {
        if d[a].abs() < 1e-12 {
            if o[a] < lo[a] - 1e-9 || o[a] > hi[a] + 1e-9 {
                return None;
            }
            continue;
        }
        let (ta, tb) = ((lo[a] - o[a]) / d[a], (hi[a] - o[a]) / d[a]);
        t0 = t0.max(ta.min(tb));
        t1 = t1.min(ta.max(tb));
    }
    (t1 >= t0.max(0.0)).then_some(t0.max(0.0))
}

/// A handle of a selected group or component under the cursor.
#[derive(Clone, Copy, Debug)]
pub struct Picked {
    pub inst: InstanceId,
    pub pos: DVec3,
    pub handle: Handle,
    /// The snap (world coordinates) when the handle is one.
    pub snap: Option<SnapPoint>,
}

/// The selected groups and components of the active context.
fn selected_instances(cx: &ToolCx) -> Vec<InstanceId> {
    cx.doc.sel.iter().filter_map(|e| if let EntityRef::Instance(i) = e { Some(*i) } else { None }).collect()
}

/// Before the first click: the selected object whose grips show (its box under the cursor,
/// or the one owning the handle), and the snap, origin or grip within 12 pixels.
pub fn pick(cx: &ToolCx, m: DVec2, kind: GripKind) -> (Option<InstanceId>, Option<Picked>) {
    let cam = cx.cam();
    let near = |p: DVec3| cam.project(p).map(|s| s.distance(m)).filter(|d| *d <= HANDLE_PX);
    let mut best: Option<(f64, Picked)> = None;
    let mut hovered: Option<(f64, InstanceId)> = None;
    let nearest = |c: Vec<(f64, Picked)>| c.into_iter().min_by(|a, b| a.0.total_cmp(&b.0));
    for inst in selected_instances(cx) {
        let Some(x) = world(cx, inst) else { continue };
        // Its snaps first, then its origin, then the grips of the current type.
        let snaps = instance_snaps(cx, inst)
            .into_iter()
            .filter_map(|s| Some((near(s.pos)?, Picked { inst, pos: s.pos, handle: Handle::Snap, snap: Some(s) })))
            .collect();
        let origin = x.transform_point3(DVec3::ZERO);
        let own = nearest(snaps)
            .or_else(|| near(origin).map(|d| (d, Picked { inst, pos: origin, handle: Handle::Origin, snap: None })))
            .or_else(|| {
                if !box_big_enough(cx, inst) {
                    return None;
                }
                let grips = grip_points(cx, inst, kind)
                    .into_iter()
                    .filter_map(|p| Some((near(p)?, Picked { inst, pos: p, handle: kind.handle(), snap: None })))
                    .collect();
                nearest(grips)
            });
        if let Some((d, p)) = own
            && best.as_ref().is_none_or(|b| d < b.0)
        {
            best = Some((d, p));
        }
        if let Some(t) = box_hit(cx, inst, m)
            && hovered.is_none_or(|h| t < h.0)
        {
            hovered = Some((t, inst));
        }
    }
    let picked = best.map(|b| b.1);
    (picked.map(|p| p.inst).or(hovered.map(|h| h.1)), picked)
}

/// The inference shown for a handle.
pub fn handle_snap(cx: &ToolCx, p: &Picked) -> Snap {
    let group = cx.doc.model.defs.get(cx.doc.ents().instances[p.inst].def).is_some_and(|d| d.is_group);
    Snap { pos: p.pos, kind: SnapKind::Handle(p.handle, group), face: None, edge: None, guide: None }
}

/// The snaps of the other groups and components of the active context, which a moved snap can
/// meet: those of the instances that are not moved.
pub fn targets(cx: &ToolCx, moved: &[EntityRef]) -> Vec<(InstanceId, SnapPoint)> {
    let ids: Vec<InstanceId> = cx.doc.ents().instances.keys().collect();
    ids.into_iter()
        .filter(|i| !moved.contains(&EntityRef::Instance(*i)))
        .flat_map(|i| instance_snaps(cx, i).into_iter().map(move |s| (i, s)))
        .collect()
}

/// The target snap within 12 pixels of the cursor, nearest first.
pub fn target_under(cx: &ToolCx, m: DVec2, targets: &[(InstanceId, SnapPoint)]) -> Option<SnapPoint> {
    targets
        .iter()
        .filter_map(|(_, s)| Some((cx.cam().project(s.pos)?.distance(m), *s)))
        .filter(|(d, _)| *d <= HANDLE_PX)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|x| x.1)
}

/// The tractor beam's snap: the target nearest to the cursor on screen, other than one at
/// the point reached.
pub fn beam_target(cx: &ToolCx, m: DVec2, at: DVec3, targets: &[(InstanceId, SnapPoint)]) -> Option<SnapPoint> {
    targets
        .iter()
        .filter(|(_, s)| s.pos.distance(at) >= 1e-3 * 25.4)
        .filter_map(|(_, s)| Some((cx.cam().project(s.pos)?.distance(m), *s)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|x| x.1)
}

/// An orthonormal frame (direction, up, right) from a snap's direction and up.
fn frame(dir: DVec3, up: DVec3) -> Option<DMat3> {
    let d = dir.try_normalize()?;
    let u = (up - d * up.dot(d)).try_normalize()?;
    Some(DMat3::from_cols(d, u, d.cross(u)))
}

/// The turn that brings the moved snap `s` to face a frame (direction `dir`, up `up`): its
/// direction becomes the opposite of `dir`, its up `up`.
pub fn facing(s: &SnapPoint, dir: DVec3, up: DVec3) -> DMat3 {
    match (frame(s.dir, s.up), frame(-dir, up)) {
        (Some(a), Some(b)) => b * a.transpose(),
        _ => DMat3::IDENTITY,
    }
}

/// Over a face (world normal `n`, turned toward the eye): the snap points into it, its up
/// toward the blue axis (the red one on a horizontal face).
pub fn facing_face(cx: &ToolCx, s: &SnapPoint, n: DVec3, at: DVec3) -> DMat3 {
    let n = if n.dot(cx.cam().eye() - at) < 0.0 { -n } else { n };
    let z = cx.axis(2);
    let up = if n.cross(z).length() < 1e-6 { cx.axis(0) } else { z };
    facing(s, n, up)
}

/// The world transform of a move from `base` to `to` turning by `turn` about `base`.
pub fn moved(base: DVec3, to: DVec3, turn: DMat3) -> DMat4 {
    DMat4::from_translation(to) * DMat4::from_mat3(turn) * DMat4::from_translation(-base)
}

/// The grips of the object shown, the handle under the cursor highlighted.
pub fn paint(o: &Overlay, pts: &[DVec3], picked: Option<DVec3>) {
    let green = Color32::from_rgb(0, 150, 0);
    for p in pts {
        if let Some(s) = o.screen(*p) {
            let hot = picked.is_some_and(|q| q.distance(*p) < 1e-9);
            let r = if hot { 5.0 } else { 3.5 };
            let rect = eframe::egui::Rect::from_center_size(s, vec2(r * 2.0, r * 2.0));
            o.painter.rect_filled(rect, 0.0, if hot { green } else { Color32::WHITE });
            o.painter.rect_stroke(rect, 0.0, Stroke::new(1.5, green), eframe::egui::StrokeKind::Middle);
        }
    }
}
