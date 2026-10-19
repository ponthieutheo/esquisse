//! Commands that act on the selection or the model rather than through a tool, read from
//! SketchUp.exe: Select › Deselect Edges / Deselect Faces (right-click menu of an edge or a
//! face), Edit › Weld Edges, Orient Faces.
//!
//! Each command is a function of the document so that the tests drive it after selecting
//! with the Select tool, like a user; `EsquisseApp::run_op` only adds the messages.

use eframe::egui;
use esq_core::model::{EdgeId, EntityRef, FaceId};

use crate::app::EsquisseApp;
use crate::doc::Document;

/// The commands of this module (one `Cmd::Op` each).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    /// "deselect edges" (skoreId 30008).
    DeselectEdges,
    /// "deselect faces" (skoreId 30009).
    DeselectFaces,
    /// "edit/weld edges" (skoreId 141).
    WeldEdges,
    /// "orient faces" (skoreId 21168).
    OrientFaces,
    /// "area > selection" (skoreId 21233): right-click on a face › Area › Selection.
    AreaSelection,
    /// "divide" (skoreId 21287): right-click on an edge › Divide (the Divide tool).
    Divide,
    /// "move rotate grips" (skoreId 24199): shows or hides the Move tool's rotation grips.
    MoveRotateGrips,
    /// "set sun location" (skoreId 21402): the sun follows a point clicked on the current
    /// environment's image (Environments › edit › "Set Sun Location").
    SetSunLocation,
    /// "optimize memory" (skoreId 35031): frees what is kept besides the model.
    OptimizeMemory,
}

/// Undo steps kept by Optimize Memory.
pub const OPTIMIZE_UNDO_KEEP: usize = 10;

/// Optimize Memory: the undo history beyond the last steps and the redo steps go (each one
/// is a copy of the model). Returns how many copies were freed.
///
/// Not read from SketchUp: SketchUp 2026 for Windows has no code for this command. Its
/// skoreId 35031 is in no instruction, no 32-bit datum and no string of SketchUp.exe or its
/// DLLs (no more than the other 35xxx ids), the key "optimize memory" appears nowhere, and
/// the "Memory" page of the App Settings that its description names does not exist on the
/// desktop (`commands.json` is shared with the other platforms). The nearest things in the
/// exe are on the hidden Model Info page "Memory Statistics" (`ShowMemoryStats` flag): Purge
/// Data and Optimize Textures (`MaterialUtils::OptimizeCDibFileFormat`).
pub fn optimize_memory(doc: &mut Document) -> usize {
    doc.trim_history(OPTIMIZE_UNDO_KEEP)
}

// ---------------------------------------------------------------------------------------
// Environments: kept where the Ruby API keeps them (`view_ext` "envs", "env_current").

/// The current environment (index in the list, and its description).
pub fn current_env(m: &esq_core::model::Model) -> Option<(usize, serde_json::Value)> {
    let id = m.view_ext.get("env_current")?.as_i64()?;
    let list = m.view_ext.get("envs")?.as_array()?;
    list.iter().position(|e| e["id"].as_i64() == Some(id)).map(|i| (i, list[i].clone()))
}

/// "Set Sun Location": a click at (`u`, `v`) (0..1, left to right, top to bottom) on the
/// environment's equirectangular image puts the sun there and links it to the environment.
/// Returns the sun position kept by the environment: azimuth (0..360°) and elevation
/// (0..90°, like `Environment#linked_sun_position`).
pub fn set_sun_location(m: &mut esq_core::model::Model, u: f64, v: f64) -> Option<[f64; 2]> {
    let (i, _) = current_env(m)?;
    let az = (u.clamp(0.0, 1.0) * 360.0).clamp(0.0, 360.0);
    let el = ((0.5 - v.clamp(0.0, 1.0)) * 180.0).clamp(0.0, 90.0);
    let list = m.view_ext.get_mut("envs")?.as_array_mut()?;
    list[i]["sun_pos"] = serde_json::json!([az, el]);
    list[i]["linked_sun"] = serde_json::json!(true);
    Some([az, el])
}

/// Turns the link between the sun and the current environment on or off.
pub fn set_sun_linked(m: &mut esq_core::model::Model, on: bool) -> bool {
    let Some((i, _)) = current_env(m) else { return false };
    let Some(list) = m.view_ext.get_mut("envs").and_then(|l| l.as_array_mut()) else { return false };
    list[i]["linked_sun"] = serde_json::json!(on);
    true
}

/// The direction toward the sun when it is linked to the current environment: the point of
/// the panorama where the sun was put, turned with the environment (the same mapping as the
/// skydome, see `environments::direction`).
pub fn linked_sun_direction(m: &esq_core::model::Model) -> Option<esq_core::glam::DVec3> {
    crate::environments::linked_sun(m)
}

impl Op {
    pub fn label(self) -> &'static str {
        match self {
            Op::DeselectEdges => "Désélectionner les arêtes",
            Op::DeselectFaces => "Désélectionner les faces",
            Op::WeldEdges => "Souder les arêtes",
            Op::OrientFaces => "Orienter les faces",
            Op::AreaSelection => "Surface",
            Op::Divide => "Diviser",
            Op::MoveRotateGrips => "Afficher les poignées de rotation des outils",
            Op::SetSunLocation => "Emplacement du soleil couchant",
            Op::OptimizeMemory => "Optimiser l'utilisation de la mémoire",
        }
    }
}

/// Divide is offered for the first selected edge when it is a single edge or part of an arc
/// or a circle (not of another curve), as in SketchUp's edge menu.
pub fn can_divide(doc: &Document) -> bool {
    let ents = doc.ents();
    sel_edges(doc).first().is_some_and(|&e| match ents.edges[e].curve {
        None => true,
        Some(c) => esq_core::curves::arc_fit(ents, c).is_some(),
    })
}

/// Dialogs opened by these commands (kept in `ui_state.extra.ops`).
#[derive(Default)]
pub struct OpsState {
    /// Area › Selection (`CAreaDlg`): the formatted area shown in a read-only field.
    pub area: Option<String>,
    /// Set Sun Location: the environment's image is shown, a click there places the sun.
    pub sun: bool,
    /// The texture of the environment image shown, by file.
    sun_tex: Option<(String, egui::TextureHandle)>,
}

/// Area › Selection: the sum of the areas of the selected faces (mm², in the coordinates of
/// the active context, like SketchUp's face area without transformation).
pub fn selection_area(doc: &Document) -> f64 {
    let ents = doc.ents();
    sel_faces(doc).into_iter().map(|f| esq_core::tess::face_area(ents, f)).sum()
}

fn sel_edges(doc: &Document) -> Vec<EdgeId> {
    doc.sel.iter().filter_map(|e| if let EntityRef::Edge(x) = e { Some(*x) } else { None }).collect()
}

fn sel_faces(doc: &Document) -> Vec<FaceId> {
    doc.sel.iter().filter_map(|e| if let EntityRef::Face(x) = e { Some(*x) } else { None }).collect()
}

/// Edit › Weld Edges: the selected edges become curves (one per chain), as one undo step
/// named like SketchUp's. Returns the number of curves made.
pub fn weld_edges(doc: &mut Document) -> usize {
    let edges = sel_edges(doc);
    if edges.len() < 2 {
        return 0;
    }
    let mut n = 0;
    doc.commit("Souder les arêtes", |m, d| n = esq_core::curves::weld(m.entities_mut(d), &edges).len());
    n
}

/// Orient Faces (right-click on a face): the faces connected to the selected ones are turned
/// to face the same way. Returns how many were reversed.
pub fn orient_faces(doc: &mut Document) -> usize {
    let faces = sel_faces(doc);
    if faces.is_empty() {
        return 0;
    }
    let mut n = 0;
    doc.commit("Orienter les faces", |m, d| n = esq_core::curves::orient_faces(m.entities_mut(d), &faces));
    n
}

/// Select › Deselect Edges / Deselect Faces: removes every edge (or face) from the selection
/// and keeps the rest. Returns how many were removed.
pub fn deselect(doc: &mut Document, edges: bool) -> usize {
    let before = doc.sel.len();
    let keep: Vec<EntityRef> = doc
        .sel
        .iter()
        .copied()
        .filter(|e| if edges { !matches!(e, EntityRef::Edge(_)) } else { !matches!(e, EntityRef::Face(_)) })
        .collect();
    let n = before - keep.len();
    if n > 0 {
        doc.select(keep);
    }
    n
}

impl EsquisseApp {
    pub fn op_enabled(&self, op: Op) -> bool {
        let sel = &self.doc.sel;
        match op {
            Op::DeselectEdges => sel.iter().any(|e| matches!(e, EntityRef::Edge(_))),
            Op::DeselectFaces | Op::OrientFaces | Op::AreaSelection => {
                sel.iter().any(|e| matches!(e, EntityRef::Face(_)))
            }
            Op::WeldEdges => sel_edges(&self.doc).len() >= 2,
            Op::Divide => can_divide(&self.doc),
            Op::MoveRotateGrips => true,
            Op::SetSunLocation => current_env(&self.doc.model).is_some(),
            Op::OptimizeMemory => true,
        }
    }

    /// `Some(ticked)` for the commands shown with a check mark.
    pub fn op_checked(&self, op: Op) -> Option<bool> {
        match op {
            Op::MoveRotateGrips => Some(crate::tools::move_grips::grips_enabled()),
            Op::SetSunLocation => {
                Some(current_env(&self.doc.model).is_some_and(|(_, e)| e["linked_sun"].as_bool() == Some(true)))
            }
            _ => None,
        }
    }

    pub fn run_op(&mut self, ctx: &egui::Context, op: Op) {
        match op {
            Op::DeselectEdges => {
                deselect(&mut self.doc, true);
            }
            Op::DeselectFaces => {
                deselect(&mut self.doc, false);
            }
            Op::WeldEdges => {
                weld_edges(&mut self.doc);
            }
            Op::OrientFaces => {
                orient_faces(&mut self.doc);
            }
            Op::AreaSelection => {
                let a = selection_area(&self.doc);
                self.ui_state.extra.ops.area = Some(esq_core::units::format_area(a, self.doc.model.units));
            }
            Op::Divide => self.set_tool(crate::tools::ToolId::Divide),
            Op::MoveRotateGrips => {
                use crate::tools::move_grips::{grips_enabled, set_grips_enabled};
                set_grips_enabled(!grips_enabled());
                self.doc.rev += 1;
            }
            // The checkbox of SketchUp's environment dialog: turning it on shows the image to
            // click; turning it off unlinks the sun.
            Op::SetSunLocation => {
                let on = self.op_checked(op) == Some(true);
                if on {
                    self.doc.commit("Emplacement du soleil", |m, _| {
                        set_sun_linked(m, false);
                    });
                    self.ui_state.extra.ops.sun = false;
                } else {
                    self.ui_state.extra.ops.sun = true;
                }
            }
            Op::OptimizeMemory => {
                let n = optimize_memory(&mut self.doc);
                // The textures shown (environment image) are loaded again when needed.
                self.ui_state.extra.ops.sun_tex = None;
                let (left, _) = self.doc.history_len();
                self.say(
                    ctx,
                    format!("Mémoire libérée : {n} copie(s) du modèle ; {left} étape(s) restent annulables."),
                );
            }
        }
    }
}

/// The dialogs of these commands, once per frame (from `extra::show`).
pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    if let Some(text) = app.ui_state.extra.ops.area.clone() {
        let mut close = false;
        crate::ui::centered("Surface").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Surface :");
                let mut t = text.clone();
                ui.add(egui::TextEdit::singleline(&mut t).interactive(false));
            });
            ui.vertical_centered(|ui| close = ui.button("Fermer").clicked());
        });
        if close {
            app.ui_state.extra.ops.area = None;
        }
    }
    if app.ui_state.extra.ops.sun {
        sun_dialog(app, ctx);
    }
}

/// The current environment's image: a click puts the sun there.
fn sun_dialog(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some((_, env)) = current_env(&app.doc.model) else {
        app.ui_state.extra.ops.sun = false;
        return;
    };
    // The panorama the model holds, else the file it names (only a local one: a network path
    // read here would send the user's NTLM hash to its server).
    let src =
        crate::environments::current(&app.doc.model).and_then(|e| crate::environments::source(&app.doc.model, &e));
    let path = src.as_ref().map(|s| s.key()).unwrap_or_default();
    let ops = &mut app.ui_state.extra.ops;
    if ops.sun_tex.as_ref().is_none_or(|(p, _)| *p != path)
        && let Some(img) = src.and_then(|s| s.bytes().ok()).and_then(|b| crate::environments::decode_picture(&b))
    {
        let img = img.resize(1024, 512, image::imageops::FilterType::Triangle).to_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        let tex = crate::gpu_img::load_texture(ctx, "environment", size, img.as_raw(), Default::default());
        ops.sun_tex = Some((path.clone(), tex));
    }
    let tex = ops.sun_tex.as_ref().map(|(_, t)| t.clone());
    let pos = env["sun_pos"].as_array().map(|a| {
        let at = |i: usize| a.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0);
        (at(0), at(1))
    });
    let (mut close, mut click) = (false, None);
    crate::ui::centered("Emplacement du soleil couchant").show(ctx, |ui| {
        ui.label(env["name"].as_str().unwrap_or_default());
        let size = egui::vec2(480.0, 240.0);
        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
        match &tex {
            Some(t) => {
                ui.painter().image(
                    t.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None => {
                ui.painter().rect_filled(rect, 0.0, egui::Color32::from_gray(90));
            }
        }
        if let Some((az, el)) = pos.filter(|_| env["linked_sun"].as_bool() == Some(true)) {
            let p = rect.min + egui::vec2((az / 360.0) as f32 * size.x, (0.5 - el / 180.0) as f32 * size.y);
            ui.painter().circle(p, 6.0, egui::Color32::YELLOW, egui::Stroke::new(1.5, egui::Color32::BLACK));
        }
        if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
        {
            click = Some(((p.x - rect.min.x) / size.x, (p.y - rect.min.y) / size.y));
        }
        if let Some((az, el)) = pos {
            ui.label(format!("Azimut : {az:.0}°, élévation : {el:.0}°"));
        }
        ui.vertical_centered(|ui| close = ui.button("Fermer").clicked());
    });
    if let Some((u, v)) = click {
        app.doc.commit("Emplacement du soleil", |m, _| {
            set_sun_location(m, u as f64, v as f64);
        });
    }
    if close {
        app.ui_state.extra.ops.sun = false;
    }
}
