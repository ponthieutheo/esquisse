//! The modeling grid (Window › Modeling Grid, View › Grid, grid snapping), as SketchUp 2026
//! has it behind its `ModelingGrid` feature switch (`ModelingGridManager`, `ModelingGridInfo`,
//! `ModelGridEditPage`, `ModelGridSelectPage`, `SlugModelingGridDrawer`,
//! `CInputPointGridPoint`).
//!
//! The model keeps named grids: linear or radial, on one of the planes of the drawing axes
//! (red/green, red/blue, blue/green), major lines every `spacing` (36" by default, both
//! ways), minor lines `subdivisions` times between them, each set with its colour, dash
//! pattern and thickness. Which grid is used, whether major and minor lines show and whether
//! the cursor snaps to the grid's intersections are settings of the model; showing the grid
//! is a rendering option (`ModelingGrid`, `ROPSetModelingGrid`), so styles and scenes keep it.
//! Stored in `Model::view_ext["modeling_grid"]`.

use esq_core::glam::{DMat4, DVec3};
use esq_core::model::Model;
use serde::{Deserialize, Serialize};

const KEY: &str = "modeling_grid";
/// 36 inches, SketchUp's default spacing.
pub const DEFAULT_SPACING: f64 = 36.0 * 25.4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridLines {
    /// sRGB.
    pub color: [u8; 3],
    /// A dash pattern of `scene::DASHES` (empty: solid).
    #[serde(default)]
    pub dash: String,
    /// Pixels.
    pub thickness: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelGrid {
    pub id: i64,
    pub name: String,
    /// Grid Type: linear (false) or radial (rings and spokes).
    #[serde(default)]
    pub radial: bool,
    /// Plane Location: 0 red/green, 1 red/blue, 2 blue/green.
    #[serde(default)]
    pub plane: u8,
    /// Major line spacing along the plane's two directions (mm); radial: between rings.
    pub spacing: [f64; 2],
    /// Minor lines between two major ones (0: none); radial: rings and spokes.
    #[serde(default)]
    pub subdivisions: [u32; 2],
    pub major: GridLines,
    pub minor: GridLines,
}

impl ModelGrid {
    /// A grid as `ModelingGridInfo` starts: 36" both ways, black, major lines 2 px, minor 1 px.
    pub fn new(id: i64, name: &str) -> Self {
        ModelGrid {
            id,
            name: name.into(),
            radial: false,
            plane: 0,
            spacing: [DEFAULT_SPACING; 2],
            subdivisions: [0, 0],
            major: GridLines { color: [0, 0, 0], dash: String::new(), thickness: 2 },
            minor: GridLines { color: [0, 0, 0], dash: String::new(), thickness: 1 },
        }
    }

    /// Origin and the plane's two directions, from the drawing axes.
    pub fn frame(&self, axes: &DMat4) -> (DVec3, DVec3, DVec3) {
        let o = axes.transform_point3(DVec3::ZERO);
        let ax = |i: usize| axes.col(i).truncate().normalize_or_zero();
        let (u, v) = match self.plane {
            1 => (ax(0), ax(2)),
            2 => (ax(1), ax(2)),
            _ => (ax(0), ax(1)),
        };
        (o, u, v)
    }

    /// Number of spokes of a radial grid (major ones every 30°).
    pub fn spokes(&self) -> u32 {
        12
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridSettings {
    pub grids: Vec<ModelGrid>,
    pub current: i64,
    pub show_major: bool,
    pub show_minor: bool,
    /// Snap to Grid Lines (`enable grid snapping`).
    pub snap: bool,
}

impl Default for GridSettings {
    fn default() -> Self {
        GridSettings {
            grids: vec![ModelGrid::new(1, "Grille")],
            current: 1,
            show_major: true,
            show_minor: true,
            snap: false,
        }
    }
}

impl GridSettings {
    pub fn current(&self) -> Option<&ModelGrid> {
        self.grids.iter().find(|g| g.id == self.current).or(self.grids.first())
    }

    pub fn current_mut(&mut self) -> Option<&mut ModelGrid> {
        let id = self.current().map(|g| g.id)?;
        self.grids.iter_mut().find(|g| g.id == id)
    }

    /// A name not yet taken (`ModelingGridManager::GenerateUniqueName`).
    pub fn unique_name(&self, base: &str) -> String {
        let mut n = base.to_string();
        let mut i = 1;
        while self.grids.iter().any(|g| g.name == n) {
            n = format!("{base} {i}");
            i += 1;
        }
        n
    }

    /// Add Grid: a new grid with the defaults, made current.
    pub fn add(&mut self) -> i64 {
        let id = self.grids.iter().map(|g| g.id).max().unwrap_or(0) + 1;
        let name = self.unique_name("Grille");
        self.grids.push(ModelGrid::new(id, &name));
        self.current = id;
        id
    }

    /// Delete Grid: the last one cannot go.
    pub fn delete(&mut self, id: i64) -> bool {
        if self.grids.len() <= 1 || !self.grids.iter().any(|g| g.id == id) {
            return false;
        }
        self.grids.retain(|g| g.id != id);
        if self.current == id {
            self.current = self.grids[0].id;
        }
        true
    }
}

pub fn settings(m: &Model) -> GridSettings {
    m.view_ext
        .get(KEY)
        .and_then(|v| serde_json::from_value::<GridSettings>(v.clone()).ok())
        .filter(|s| !s.grids.is_empty())
        .unwrap_or_default()
}

pub fn save(m: &mut Model, s: &GridSettings) {
    if let Ok(v) = serde_json::to_value(s) {
        m.view_ext.insert(KEY.into(), v);
    }
}

// ------------------------------------------------------------------------------ snapping

/// What the inference needs of the grid when snapping is on.
#[derive(Clone, Debug)]
pub struct GridSnap {
    pub origin: DVec3,
    pub u: DVec3,
    pub v: DVec3,
    /// Distance between snapping lines along u and v (mm); radial: between rings.
    pub step: [f64; 2],
    pub radial: bool,
    /// Radial: angle between spokes (radians).
    pub angle: f64,
}

impl GridSnap {
    pub fn normal(&self) -> DVec3 {
        self.u.cross(self.v).normalize_or_zero()
    }

    /// The grid intersection nearest to point `p` of the grid's plane.
    pub fn nearest(&self, p: DVec3) -> DVec3 {
        let rel = p - self.origin;
        let (a, b) = (rel.dot(self.u), rel.dot(self.v));
        let (a, b) = if self.radial {
            let r = (a * a + b * b).sqrt();
            let r = (r / self.step[0]).round() * self.step[0];
            let t = (b.atan2(a) / self.angle).round() * self.angle;
            (r * t.cos(), r * t.sin())
        } else {
            ((a / self.step[0]).round() * self.step[0], (b / self.step[1]).round() * self.step[1])
        };
        self.origin + self.u * a + self.v * b
    }

    /// The grid point the cursor ray (`o`, `d`) is near, on the grid's plane.
    pub fn at_ray(&self, o: DVec3, d: DVec3) -> Option<DVec3> {
        let n = self.normal();
        let den = d.dot(n);
        if den.abs() < 1e-9 {
            return None;
        }
        let t = (self.origin - o).dot(n) / den;
        (t > 0.0).then(|| self.nearest(o + d * t))
    }
}

/// The grid to snap to: shown, with snapping on.
pub fn snap_of(m: &Model, shown: bool) -> Option<GridSnap> {
    let s = settings(m);
    if !shown || !s.snap {
        return None;
    }
    let g = s.current()?;
    let (origin, u, v) = g.frame(&m.axes);
    let minor = |i: usize| s.show_minor && g.subdivisions[i] > 0;
    let step = |i: usize| {
        let sp = g.spacing[i].max(1e-3);
        if minor(i) { sp / g.subdivisions[i] as f64 } else { sp }
    };
    let spokes = g.spokes() * if minor(1) { g.subdivisions[1].max(1) } else { 1 };
    Some(GridSnap {
        origin,
        u,
        v,
        step: [step(0), step(1)],
        radial: g.radial,
        angle: std::f64::consts::TAU / spokes as f64,
    })
}

// ------------------------------------------------------------------------------ drawing

/// What the renderer needs to draw the grid.
#[derive(Clone, Debug, PartialEq)]
pub struct GridView {
    pub origin: DVec3,
    pub u: DVec3,
    pub v: DVec3,
    pub spacing: [f64; 2],
    pub subdivisions: [u32; 2],
    pub radial: bool,
    pub spokes: u32,
    pub major: Option<GridLines>,
    pub minor: Option<GridLines>,
}

/// The grid as the view shows it, when View › Grid is on.
pub fn view_of(m: &Model, shown: bool) -> Option<GridView> {
    if !shown {
        return None;
    }
    let s = settings(m);
    let g = s.current()?;
    let (origin, u, v) = g.frame(&m.axes);
    let has_minor = g.subdivisions.iter().any(|&n| n > 1);
    Some(GridView {
        origin,
        u,
        v,
        spacing: g.spacing,
        subdivisions: g.subdivisions,
        radial: g.radial,
        spokes: g.spokes(),
        major: s.show_major.then(|| g.major.clone()),
        minor: (s.show_minor && has_minor).then(|| g.minor.clone()),
    })
}

// ------------------------------------------------------------------------------ panel

use eframe::egui::{self, Color32, RichText};

use crate::app::EsquisseApp;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Select,
    Edit,
}

const PLANES: [(&str, &str); 3] =
    [("Rouge/vert", "Plan rouge-vert (au sol)"), ("Rouge/bleu", "Plan rouge-bleu"), ("Bleu/vert", "Plan vert-bleu")];

impl EsquisseApp {
    /// Changes the grids (`label`: an undoable step, else at once).
    pub fn set_grids(&mut self, s: &GridSettings, label: Option<&str>) {
        match label {
            Some(l) => self.doc.commit(l, |m, _| save(m, s)),
            None => save(&mut self.doc.model, s),
        }
        self.doc.rev += 1;
    }
}

fn color_button(ui: &mut egui::Ui, c: &mut [u8; 3]) -> bool {
    let mut col = Color32::from_rgb(c[0], c[1], c[2]);
    let changed =
        egui::color_picker::color_edit_button_srgba(ui, &mut col, egui::color_picker::Alpha::Opaque).changed();
    if changed {
        *c = [col.r(), col.g(), col.b()];
    }
    changed
}

fn dash_combo(ui: &mut egui::Ui, id: &str, dash: &mut String) -> bool {
    let current = crate::scene::DASHES
        .iter()
        .find(|(n, _)| *n == dash.as_str() || (dash.is_empty() && *n == "Solid Basic"))
        .map_or("Continu", |d| d.1);
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).selected_text(current).show_ui(ui, |ui| {
        for (name, label) in crate::scene::DASHES {
            let v = if name == "Solid Basic" { String::new() } else { name.to_string() };
            if ui.selectable_label(*dash == v, label).clicked() {
                *dash = v;
                changed = true;
            }
        }
    });
    changed
}

fn length_field(ui: &mut egui::Ui, value: &mut f64, units: esq_core::model::Units) -> bool {
    let id = ui.next_auto_id();
    let mut text: String =
        ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| esq_core::units::format_length(*value, units));
    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(80.0));
    let mut changed = false;
    if r.lost_focus() {
        if let Some(v) = esq_core::units::parse_length(&text, units).filter(|v| *v > 0.0) {
            changed = (*value - v).abs() > 1e-9;
            *value = v;
        }
        ui.data_mut(|d| d.remove::<String>(id));
    } else if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text));
    }
    changed
}

/// Window › Modeling Grid.
pub fn show(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let tab_id = egui::Id::unique("model_grid_tab");
    let mut tab: Tab = ui.data_mut(|d| d.get_temp(tab_id)).unwrap_or_default();
    let mut s = settings(&app.doc.model);
    let before = s.clone();
    let mut label: Option<&str> = None;
    let mut shown = app.style.modeling_grid;
    if ui.checkbox(&mut shown, "Afficher la grille").changed() {
        app.style.modeling_grid = shown;
        app.doc.rev += 1;
    }
    ui.horizontal(|ui| {
        ui.selectable_value(&mut tab, Tab::Select, "Sélectionner");
        ui.selectable_value(&mut tab, Tab::Edit, "Modifier");
    });
    ui.separator();
    match tab {
        Tab::Select => {
            let mut delete = None;
            for g in &s.grids {
                ui.horizontal(|ui| {
                    let tag = PLANES[g.plane.min(2) as usize].0;
                    if ui.selectable_label(s.current == g.id, &g.name).clicked() {
                        label = Some("Grille de modélisation");
                        delete = None;
                        ui.data_mut(|d| d.insert_temp(egui::Id::unique("model_grid_pick"), g.id));
                    }
                    ui.label(RichText::new(tag).small().color(Color32::from_gray(110)));
                    if s.grids.len() > 1 && ui.small_button("✕").on_hover_text("Supprimer la grille").clicked() {
                        delete = Some(g.id);
                    }
                });
            }
            if let Some(id) = ui.data_mut(|d| d.remove_temp::<i64>(egui::Id::unique("model_grid_pick"))) {
                s.current = id;
            }
            if let Some(id) = delete
                && s.delete(id)
            {
                label = Some("Supprimer la grille");
            }
            if ui.button("Ajouter une grille").clicked() {
                s.add();
                label = Some("Ajouter une grille");
            }
            ui.add_space(6.0);
            ui.label(RichText::new("Paramètres de la grille").strong());
            ui.checkbox(&mut s.show_major, "Afficher la grille principale");
            ui.checkbox(&mut s.show_minor, "Afficher la grille secondaire");
            ui.checkbox(&mut s.snap, "Accrocher aux lignes de la grille");
            ui.add_space(6.0);
            ui.label(RichText::new("Emplacement du plan").strong());
            ui.horizontal(|ui| {
                if let Some(g) = s.current_mut() {
                    for (i, (name, tip)) in PLANES.iter().enumerate() {
                        if ui.selectable_label(g.plane as usize == i, *name).on_hover_text(*tip).clicked() {
                            g.plane = i as u8;
                        }
                    }
                }
            });
        }
        Tab::Edit => {
            let units = app.doc.model.units;
            let names: Vec<(i64, String)> = s.grids.iter().map(|g| (g.id, g.name.clone())).collect();
            if let Some(g) = s.current_mut() {
                egui::Grid::new("model_grid_edit").num_columns(3).show(ui, |ui| {
                    ui.label("Nom");
                    let mut n = g.name.clone();
                    if ui.text_edit_singleline(&mut n).changed()
                        && !n.trim().is_empty()
                        && !names.iter().any(|(id, x)| *x == n && *id != g.id)
                    {
                        g.name = n;
                    }
                    ui.end_row();
                    ui.label("Type de grille");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut g.radial, false, "Linéaire");
                        ui.radio_value(&mut g.radial, true, "Radiale");
                    });
                    ui.end_row();
                    for (title, major) in [("Lignes principales", true), ("Lignes secondaires", false)] {
                        ui.label(RichText::new(title).strong());
                        ui.end_row();
                        let lines = if major { &mut g.major } else { &mut g.minor };
                        ui.label("Couleur des lignes");
                        color_button(ui, &mut lines.color);
                        ui.end_row();
                        ui.label("Type de tirets");
                        dash_combo(ui, if major { "grid_dash_major" } else { "grid_dash_minor" }, &mut lines.dash);
                        ui.end_row();
                        ui.label("Épaisseur des lignes");
                        ui.add(egui::DragValue::new(&mut lines.thickness).range(1..=10).suffix(" px"));
                        ui.end_row();
                        if major {
                            ui.label(if g.radial { "Espacement des cercles" } else { "Espacement de grille" });
                            ui.horizontal(|ui| {
                                ui.label("Vertical");
                                length_field(ui, &mut g.spacing[0], units);
                                if !g.radial {
                                    ui.label("Horizontal");
                                    length_field(ui, &mut g.spacing[1], units);
                                }
                            });
                        } else {
                            ui.label("Subdivisions");
                            ui.horizontal(|ui| {
                                ui.label(if g.radial { "Cercles" } else { "Vertical" });
                                ui.add(egui::DragValue::new(&mut g.subdivisions[0]).range(0..=100));
                                ui.label(if g.radial { "Rayons" } else { "Horizontal" });
                                ui.add(egui::DragValue::new(&mut g.subdivisions[1]).range(0..=100));
                            });
                        }
                        ui.end_row();
                    }
                });
            }
        }
    }
    if s != before {
        app.set_grids(&s, label);
    }
    ui.data_mut(|d| d.insert_temp(tab_id, tab));
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grids_start_like_sketchups_and_live_in_the_model() {
        let mut m = Model::new();
        let mut s = settings(&m);
        let g = s.current().unwrap().clone();
        assert_eq!((g.spacing, g.subdivisions, g.major.thickness, g.minor.thickness), ([914.4; 2], [0, 0], 2, 1));
        assert!(s.show_major && s.show_minor && !s.snap);
        let id = s.add();
        assert_eq!(s.current().unwrap().name, "Grille 1");
        assert!(s.delete(1) && !s.delete(id), "the last grid stays");
        save(&mut m, &s);
        assert_eq!(settings(&m), s);
    }

    #[test]
    fn planes_and_snapping_points() {
        let mut m = Model::new();
        let mut s = settings(&m);
        s.snap = true;
        s.current_mut().unwrap().subdivisions = [4, 2];
        save(&mut m, &s);
        assert!(snap_of(&m, false).is_none(), "grid hidden");
        let gs = snap_of(&m, true).unwrap();
        // Minor lines every 914.4 / 4 along red, / 2 along green.
        assert_eq!(gs.step, [228.6, 457.2]);
        let p = gs.at_ray(DVec3::new(240.0, 440.0, 1000.0), -DVec3::Z).unwrap();
        assert!(p.distance(DVec3::new(228.6, 457.2, 0.0)) < 1e-9);
        // Red/blue plane: the grid stands up.
        s.current_mut().unwrap().plane = 1;
        save(&mut m, &s);
        let gs = snap_of(&m, true).unwrap();
        assert_eq!((gs.u, gs.v), (DVec3::X, DVec3::Z));
        let p = gs.at_ray(DVec3::new(700.0, -1000.0, 470.0), DVec3::Y).unwrap();
        assert!(p.distance(DVec3::new(685.8, 0.0, 457.2)) < 1e-9);
        // Radial: rings and spokes.
        s.current_mut().unwrap().plane = 0;
        s.current_mut().unwrap().radial = true;
        s.show_minor = false;
        save(&mut m, &s);
        let gs = snap_of(&m, true).unwrap();
        let p = gs.nearest(DVec3::new(1000.0, 80.0, 0.0));
        assert!(p.distance(DVec3::new(914.4, 0.0, 0.0)) < 1e-9);
        let a = 30f64.to_radians();
        let p = gs.nearest(DVec3::new(1700.0 * a.cos(), 1700.0 * a.sin(), 0.0));
        assert!(p.distance(DVec3::new(1828.8 * a.cos(), 1828.8 * a.sin(), 0.0)) < 1e-9);
    }
}
