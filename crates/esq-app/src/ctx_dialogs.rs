//! What two entries of the right-click menu open (`context_menu.rs`):
//!
//! - the matched photo's **Project Photo** (also Window › Match Photo ›
//!   Project textures from photo): the photo becomes the material of the selected faces, each
//!   texture placed by the photo's camera (a perspective map: what the face shows of the
//!   photo); when some of those faces already have a material, SketchUp first asks
//!   « Écraser les matières existantes? » (Oui, Non: only the faces without one, Annuler);
//! - the model axes' **Move** (`CMoveSketchCSDlg`, « Déplacer le contexte d'esquisse »): moves
//!   the axes along their own red, green and blue directions and turns them about those
//!   directions, in one operation « Déplacer les axes ».

use std::cell::RefCell;

use eframe::egui;
use esq_core::glam::{DMat4, DVec3};
use esq_core::meta::{FaceTex, UvMap};
use esq_core::model::{EntityRef, FaceId, Material, MaterialId, Model};
use esq_core::photo::MatchPhoto;

use crate::app::EsquisseApp;

#[derive(Default)]
struct State {
    /// Project Photo waits for the answer to « Écraser les matières existantes? ».
    overwrite_question: bool,
    /// The Move Sketching Context dialog: the six fields as typed.
    move_axes: Option<[String; 6]>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

// ------------------------------------------------------------------------------------------
// Project Photo

/// The faces Project Photo works on (selected in the open context) and, for each, the side
/// that faces the photo's camera (true = front).
fn targets(app: &EsquisseApp, photo: &MatchPhoto) -> Vec<(FaceId, bool)> {
    let ents = app.doc.ents();
    let x = app.doc.ctx_xform();
    let eye = photo.camera.eye();
    app.doc
        .sel
        .iter()
        .filter_map(|e| if let EntityRef::Face(f) = e { Some(*f) } else { None })
        .filter_map(|f| {
            let face = ents.faces.get(f)?;
            let p = x.transform_point3(ents.pos(face.outer[0]));
            let n = x.transform_vector3(face.normal);
            Some((f, n.dot(eye - p) >= 0.0))
        })
        .collect()
}

/// Right-click › Project Photo: asks first when a face would lose its material.
pub fn project_photo(app: &mut EsquisseApp) {
    let Some(photo) = app.active_photo().cloned() else { return };
    let t = targets(app, &photo);
    let ents = app.doc.ents();
    let painted = t.iter().any(|(f, front)| {
        let face = &ents.faces[*f];
        if *front { face.front.is_some() } else { face.back.is_some() }
    });
    if painted {
        with(|s| s.overwrite_question = true);
    } else {
        project(app, true);
    }
}

/// The texture map that shows on a point (of the open context, placed by `x`) the pixel of the
/// photo seen there: u and v from 0 to 1 across the picture (v upwards), homogeneous since the
/// camera has perspective.
pub fn photo_map(photo: &MatchPhoto, x: &DMat4) -> UvMap {
    let c = &photo.camera;
    // Camera coordinates of a local point: rot · (x · p) + trans, as rows on (p, 1).
    let rows: [[f64; 4]; 3] = std::array::from_fn(|i| {
        let r = c.rot.row(i);
        let lin = |j: usize| r.dot(x.col(j).truncate());
        [lin(0), lin(1), lin(2), r.dot(x.w_axis.truncate()) + c.trans[i]]
    });
    let (w, h) = (c.size.x.max(1.0), c.size.y.max(1.0));
    let (px, py, f) = (c.principal.x, c.principal.y, c.focal);
    let q = rows[2];
    let u = std::array::from_fn(|k| (px * rows[2][k] + f * rows[0][k]) / w);
    let v = std::array::from_fn(|k| ((h - py) * rows[2][k] - f * rows[1][k]) / h);
    UvMap { u, v, q }
}

/// The material of the photo: the one an earlier projection made, else a new one.
fn photo_material(m: &mut Model, photo: &MatchPhoto) -> Option<MaterialId> {
    let base = std::path::Path::new(&photo.name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Photo".into());
    let same = m.materials.iter().find(|(_, x)| {
        x.name.starts_with(&base) && x.ext.texture.as_ref().is_some_and(|t| t.image.0[..] == photo.image.0[..])
    });
    if let Some((id, _)) = same {
        return Some(id);
    }
    let tex = crate::mat_panel::texture_from_bytes(&photo.name, photo.image.0.to_vec()).ok()?;
    let taken: Vec<String> = m.materials.values().map(|x| x.name.clone()).collect();
    Some(m.materials.insert(Material {
        name: Model::unique_name(&base, &taken),
        color: tex.avg,
        opacity: 1.0,
        ext: esq_core::meta::MaterialExt { texture: Some(tex), ..Default::default() },
    }))
}

/// Projects the photo on the selected faces; `overwrite` false leaves the faces that already
/// have a material on that side as they are. One operation, « Projeter la photo ».
pub fn project(app: &mut EsquisseApp, overwrite: bool) {
    let Some(photo) = app.active_photo().cloned() else { return };
    let t = targets(app, &photo);
    if t.is_empty() {
        return;
    }
    let x = app.doc.ctx_xform();
    let map = photo_map(&photo, &x);
    app.doc.commit("Projeter la photo", |m, d| {
        let Some(mat) = photo_material(m, &photo) else { return };
        let ents = m.entities_mut(d);
        for (f, front) in t {
            let Some(face) = ents.faces.get_mut(f) else { continue };
            let side = if front { &mut face.front } else { &mut face.back };
            if side.is_some() && !overwrite {
                continue;
            }
            *side = Some(mat);
            let tex = face.tex.get_or_insert_with(|| Box::new(FaceTex::default()));
            let k = usize::from(!front);
            if front {
                tex.front = Some(map);
            } else {
                tex.back = Some(map);
            }
            tex.projection[k] = None;
            tex.pins[k].clear();
        }
    });
}

// ------------------------------------------------------------------------------------------
// Move Sketching Context

/// Opens the dialog, its fields at zero.
pub fn open_move_axes() {
    with(|s| s.move_axes = Some(std::array::from_fn(|_| "0".to_string())));
}

/// The new axes: moved by `offset` along the current red, green and blue directions, then
/// turned by `angles` (degrees) about the moved red, green and blue directions, in that order.
pub fn moved_axes(axes: DMat4, offset: DVec3, angles: DVec3) -> DMat4 {
    let r = angles.to_array().map(f64::to_radians);
    axes * DMat4::from_translation(offset)
        * DMat4::from_rotation_x(r[0])
        * DMat4::from_rotation_y(r[1])
        * DMat4::from_rotation_z(r[2])
}

fn move_axes_window(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(mut fields) = with(|s| s.move_axes.clone()) else { return };
    let mut close = false;
    let mut ok = false;
    crate::ui::centered("Déplacer le contexte d'esquisse").show(ctx, |ui| {
        egui::Grid::new("move_axes").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            for (k, group) in ["Déplacer", "Faire pivoter"].iter().enumerate() {
                ui.label(egui::RichText::new(*group).strong());
                ui.end_row();
                for (j, axis) in ["X (rouge)", "Y (vert)", "Z (bleu)"].iter().enumerate() {
                    ui.label(*axis);
                    let r = ui.add(egui::TextEdit::singleline(&mut fields[k * 3 + j]).desired_width(110.0));
                    crate::dialogs::name_widget(ui, &r, &format!("{group} {axis}"), Some(&fields[k * 3 + j]));
                    ui.end_row();
                }
            }
        });
        ui.horizontal(|ui| {
            ok = ui.button("OK").clicked();
            close = ui.button("Annuler").clicked();
        });
    });
    if ok {
        let units = app.doc.model.units;
        let len = |s: &str| esq_core::units::parse_length(s, units).unwrap_or(0.0);
        let deg = |s: &str| s.trim().replace(',', ".").parse::<f64>().unwrap_or(0.0);
        let offset = DVec3::new(len(&fields[0]), len(&fields[1]), len(&fields[2]));
        let angles = DVec3::new(deg(&fields[3]), deg(&fields[4]), deg(&fields[5]));
        if offset != DVec3::ZERO || angles != DVec3::ZERO {
            app.doc.commit("Déplacer les axes", |m, _| m.axes = moved_axes(m.axes, offset, angles));
        }
        close = true;
    }
    with(|s| s.move_axes = if close { None } else { Some(fields) });
}

// ------------------------------------------------------------------------------------------

/// Every frame (with the other dialogs).
pub fn windows(app: &mut EsquisseApp, ctx: &egui::Context) {
    if with(|s| s.overwrite_question) {
        let mut answer = None;
        crate::ui::centered("Esquisse").show(ctx, |ui| {
            ui.label("Écraser les matières existantes?");
            ui.horizontal(|ui| {
                for (label, a) in [("Oui", Some(true)), ("Non", Some(false)), ("Annuler", None)] {
                    if ui.button(label).clicked() {
                        answer = Some(a);
                    }
                }
            });
        });
        if let Some(a) = answer {
            with(|s| s.overwrite_question = false);
            if let Some(overwrite) = a {
                project(app, overwrite);
            }
        }
    }
    move_axes_window(app, ctx);
}
