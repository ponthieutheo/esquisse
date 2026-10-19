//! Tests of the Match Photo editor, driven with viewport points like the mouse does.

use super::*;

pub const VIEWPORT: DVec2 = DVec2::new(1280.0, 800.0);

/// Camera at `eye` looking at `target` (Z up), photo of `size` with focal `focal` pixels.
pub fn look_camera(eye: DVec3, target: DVec3, focal: f64, size: DVec2, principal: DVec2) -> PhotoCamera {
    let fwd = (target - eye).normalize();
    let right = fwd.cross(DVec3::Z).normalize();
    let up = right.cross(fwd);
    let rot = DMat3::from_cols(right, -up, fwd).transpose();
    PhotoCamera { rot, trans: -(rot * eye), focal, principal, size }
}

/// Bars along the edges of a box as `cam` sees them.
pub fn truth_bars(cam: &PhotoCamera) -> [[Line2; 2]; 3] {
    starting_bars(cam)
}

fn blob() -> Blob {
    Blob(std::sync::Arc::new(vec![1, 2, 3]))
}

fn editor() -> Editor {
    Editor::new("mur.png".into(), blob(), DVec2::new(1600.0, 1200.0))
}

fn drag(ed: &mut Editor, from: DVec2, to: DVec2) {
    ed.press(from, VIEWPORT);
    // In two steps, like a mouse that does not jump.
    ed.move_to(from + (to - from) * 0.5, VIEWPORT);
    ed.move_to(to, VIEWPORT);
    ed.release(to, VIEWPORT);
}

#[test]
fn a_new_photo_starts_from_a_plausible_camera() {
    let ed = editor();
    assert!(ed.error.is_none());
    assert!((ed.cam.focal - 0.95 * 1600.0).abs() < 1e-6, "{}", ed.cam.focal);
    // The origin is where the starting camera sees it, and the camera is `distance` away.
    assert!((ed.cam.trans.length() - ed.distance).abs() < 1e-6);
    assert!(ed.cam.project(DVec3::ZERO).unwrap().distance(ed.origin) < 1e-6);
}

#[test]
fn dragging_the_grips_onto_a_known_camera_finds_it() {
    let size = DVec2::new(1600.0, 1200.0);
    let truth =
        look_camera(DVec3::new(-2500.0, -9000.0, 2200.0), DVec3::new(200.0, 0.0, 500.0), 1250.0, size, size * 0.5);
    let goal = truth_bars(&truth);
    let mut ed = editor();
    let map = ed.map(VIEWPORT);
    for (axis, g_axis) in goal.iter().enumerate().take(2) {
        for (line, g) in g_axis.iter().enumerate() {
            for end in 0..2 {
                let l = ed.bars[axis][line];
                let from = map.to_screen(if end == 0 { l.a } else { l.b });
                drag(&mut ed, from, map.to_screen(if end == 0 { g.a } else { g.b }));
            }
        }
    }
    let origin_to = truth.project(DVec3::ZERO).unwrap();
    let origin_from = map.to_screen(ed.origin);
    drag(&mut ed, origin_from, map.to_screen(origin_to));
    ed.set_distance(truth.trans.length());
    assert!(ed.error.is_none(), "{:?}", ed.error);
    assert!((ed.cam.focal - truth.focal).abs() < 1e-3 * truth.focal, "{} vs {}", ed.cam.focal, truth.focal);
    assert!(ed.cam.eye().distance(truth.eye()) < 1.0, "{:?} vs {:?}", ed.cam.eye(), truth.eye());
    assert!(ed.cam.forward().distance(truth.forward()) < 1e-6);
}

#[test]
fn aiming_the_bars_moves_their_vanishing_point_and_keeps_their_length() {
    let mut ed = editor();
    let vp = ed.vp(0).expect("finite");
    let lens: Vec<f64> = ed.bars[0].iter().map(|l| l.a.distance(l.b)).collect();
    let to = vp + DVec2::new(0.0, -120.0);
    ed.aim(0, to);
    let now = ed.vp(0).unwrap();
    assert!(now.distance(to) < 1e-6 * to.length().max(1.0), "{now:?} vs {to:?}");
    for (l, len) in ed.bars[0].iter().zip(lens) {
        assert!((l.a.distance(l.b) - len).abs() < 1e-6);
    }
}

#[test]
fn the_horizon_moves_both_vanishing_points() {
    let mut ed = editor();
    let (r0, g0) = (ed.vp(0).unwrap(), ed.vp(1).unwrap());
    let map = ed.map(VIEWPORT);
    let (p, q) = ed.horizon().unwrap();
    // A point of the horizon that is on screen, away from any grip.
    let (a, b) = (map.to_screen(p), map.to_screen(q));
    let (a, b) = clip_line(a, b, DVec2::new(300.0, 100.0), DVec2::new(900.0, 700.0)).expect("horizon crosses the view");
    let mid = (a + b) * 0.5;
    assert_eq!(ed.hit(mid, &map), Some(Hit::Horizon), "{mid:?}");
    drag(&mut ed, mid, mid + DVec2::new(0.0, 30.0));
    let (r1, g1) = (ed.vp(0).unwrap(), ed.vp(1).unwrap());
    let dy = 30.0 / map.scale;
    assert!((r1.y - r0.y - dy).abs() < 1e-3 * dy.max(1.0), "{} {}", r1.y - r0.y, dy);
    assert!((g1.y - g0.y - dy).abs() < 1e-3 * dy.max(1.0));
}

#[test]
fn a_click_outside_the_photo_means_done_and_a_click_inside_does_not() {
    let mut ed = editor();
    let map = ed.map(VIEWPORT);
    let (tl, br) = map.rect();
    let inside = (tl + br) * 0.5 + DVec2::new(37.0, 0.0);
    if ed.hit(inside, &map).is_none() {
        ed.press(inside, VIEWPORT);
        assert!(!ed.release(inside, VIEWPORT));
    }
    let outside = DVec2::new(tl.x - 5.0, tl.y - 5.0);
    ed.press(outside, VIEWPORT);
    assert!(ed.release(outside, VIEWPORT));
    // A drag that ends outside is a pan, not "done".
    ed.press(outside, VIEWPORT);
    ed.move_to(outside + DVec2::new(40.0, 0.0), VIEWPORT);
    assert!(!ed.release(outside + DVec2::new(40.0, 0.0), VIEWPORT));
}

#[test]
fn dragging_empty_space_pans_and_the_wheel_zooms_about_the_cursor() {
    let mut ed = editor();
    let map = ed.map(VIEWPORT);
    let (tl, _) = map.rect();
    let empty = tl - DVec2::new(8.0, 8.0);
    drag(&mut ed, empty, empty + DVec2::new(50.0, 20.0));
    assert!((ed.pan - DVec2::new(50.0, 20.0)).length() < 1e-9);
    let m = DVec2::new(700.0, 300.0);
    let img = ed.map(VIEWPORT).to_image(m);
    ed.zoom_at(m, VIEWPORT, 2.0);
    assert!(ed.map(VIEWPORT).to_screen(img).distance(m) < 1e-9);
    assert!((ed.zoom - EDIT_ZOOM * 2.0).abs() < 1e-12);
}

#[test]
fn flipping_cycles_through_four_orientations() {
    let mut ed = editor();
    let start = ed.current_variant();
    let mut seen = vec![start];
    for _ in 0..3 {
        ed.cycle_variant();
        seen.push(ed.current_variant());
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 4);
    ed.cycle_variant();
    assert_eq!(ed.current_variant(), start);
}

#[test]
fn switching_the_axis_pair_keeps_a_valid_calibration() {
    for pair in Pair::ALL {
        let mut ed = editor();
        ed.set_pair(pair);
        assert!(ed.error.is_none(), "{pair:?}: {:?}", ed.error);
        assert!((ed.cam.focal - 0.95 * 1600.0).abs() < 1.0, "{pair:?}: {}", ed.cam.focal);
    }
}

#[test]
fn hover_hints_follow_the_sketchup_messages() {
    let mut ed = editor();
    let map = ed.map(VIEWPORT);
    let g = ed.bars[0][0].a;
    ed.move_to(map.to_screen(g), VIEWPORT);
    assert_eq!(ed.hover, Some(Hit::Grip { axis: 0, line: 0, end: 0 }));
    assert_eq!(ed.hint(), "Ajustez les barres d’axe rouge pour repérer le point de fuite rouge.");
    ed.move_to(map.to_screen(ed.origin), VIEWPORT);
    assert!(ed.hint().contains("origine"));
    // The origin starts on the optical centre; move it away to reach the centre.
    ed.origin += DVec2::new(200.0, 150.0);
    ed.move_to(map.to_screen(ed.cam.principal), VIEWPORT);
    assert!(ed.hint().contains("centre de projection"));
}

#[test]
fn grid_segments_stay_in_front_of_the_camera() {
    let mut ed = editor();
    ed.grid.show = [true, true, true];
    assert!(!ed.grid_lines().is_empty());
    for (a, b, _) in ed.grid_lines() {
        if let Some((p, q)) = ed.project_segment(a, b) {
            assert!(p.is_finite() && q.is_finite());
        }
    }
}

#[test]
fn clipping_keeps_the_visible_part_of_a_line() {
    let (min, max) = (DVec2::ZERO, DVec2::new(100.0, 100.0));
    let (a, b) = clip_line(DVec2::new(-50.0, 50.0), DVec2::new(150.0, 50.0), min, max).unwrap();
    assert!(a.x.abs() < 1e-9 && (b.x - 100.0).abs() < 1e-9);
    assert!(clip_line(DVec2::new(-50.0, 150.0), DVec2::new(-10.0, 120.0), min, max).is_none());
}
