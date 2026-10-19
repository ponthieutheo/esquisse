//! File › Export › Section Slice, as SketchUp's "Acad Section Exporter" (`skp2acad`, options
//! window `COptionsDlgSection`, French texts of `acad_exporter.strings`): the cut of the active
//! section plane written as AutoCAD DWG or DXF (SVG is Esquisse's own: SketchUp 2026 offers
//! only DWG and DXF), with its options:
//!
//! - "Section réelle (orthographique)" or "Projection à l’écran" (`ExportSectionType`);
//! - "Taille et échelle de dessin": "Échelle réelle (1 : 1)" with the scale "Dans le dessin" /
//!   "Dans modèle", else a width (the height follows); the screen projection of a perspective
//!   view has no real scale;
//! - "Lignes de coupe": "Aucune" (plain lines), "Polylignes avec largeur", "Entités ligne
//!   épaisse", their width or "Automatique" (the style's section cut width as on screen:
//!   width in pixels / view width × drawing width);
//! - "Version AutoCAD";
//! - "Toujours proposer les options de tranches".
//!
//! As SketchUp (`CExportSectionDialog`), the file is chosen first, the options opened from the
//! dialog's "Options..." (`file_flow.rs`); they come back after the dialog only when "Toujours
//! proposer…" is ticked and they were not opened from it.
//!
//! The drawing itself is `esq_core::slice_export`; coordinates are in the model's unit.

use eframe::egui;
use esq_core::glam::DVec2;
use esq_core::model::{DefId, Model, SectionId, Units};
use esq_core::slice_export::{SliceDrawing, SliceOptions, SliceProjection, export_slice};
use esq_core::vector2d::CadWide;
use serde_json::{Value, json};

use crate::app::EsquisseApp;
use crate::camera::Camera;
use crate::export_vec::{ACAD_VERSIONS, saved_acad_version};
use crate::filedlg::FileType;
use crate::style::ViewStyle;
use crate::ui::centered;

/// The section plane to export: the active one of the current context, else the one selected.
pub fn section_to_export(app: &EsquisseApp) -> Option<SectionId> {
    let ents = app.doc.ents();
    ents.active_section.filter(|s| ents.sections.contains_key(*s)).or_else(|| app.sel_section())
}

const NOTHING: &str = "Le plan de section ne coupe aucune face : rien à exporter.";

/// The true section of a plane, with a message in French when there is nothing to draw.
pub fn drawing(model: &Model, ctx: Option<DefId>, sec: SectionId) -> Result<SliceDrawing, String> {
    SliceDrawing::of_section(model, ctx, sec).ok_or_else(|| NOTHING.to_string())
}

/// The drawing of the projection `p`: the true section, or the cut as `cam` shows it (model
/// millimetres in a parallel view, points per pixel in perspective), framed by the view.
pub fn slice_drawing(
    model: &Model,
    ctx: Option<DefId>,
    sec: SectionId,
    p: SliceProjection,
    cam: &Camera,
) -> Result<SliceDrawing, String> {
    if p == SliceProjection::TrueSection {
        return drawing(model, ctx, sec);
    }
    let s = if cam.perspective { 25.4 / 72.0 } else { cam.pixel_size() };
    let h = cam.size.y;
    let mut d = SliceDrawing::mapped(model, ctx, sec, |q| cam.project(q).map(|v| DVec2::new(v.x, h - v.y) * s))
        .ok_or_else(|| NOTHING.to_string())?;
    d.frame = Some([DVec2::ZERO, cam.size * s]);
    Ok(d)
}

/// The options as they apply to `cam`: no real scale for the screen projection of a
/// perspective view.
pub fn effective(o: &SliceOptions, cam: &Camera) -> SliceOptions {
    let mut o = o.clone();
    if o.projection == SliceProjection::Screen && cam.perspective {
        o.full_scale = false;
    }
    o
}

/// The automatic width of the section lines on paper (millimetres): the style's section cut
/// width as a share of the view's width, times the drawing's width.
pub fn auto_width(d: &SliceDrawing, o: &SliceOptions, cam: &Camera, style: &ViewStyle) -> f64 {
    style.section_cut_width as f64 / cam.size.x.max(1.0) * d.extent().x * o.k(d)
}

/// Writes the slice of plane `sec` to `path` (DWG, DXF or SVG after its extension).
pub fn export_file(
    model: &Model,
    ctx: Option<DefId>,
    sec: SectionId,
    o: &SliceOptions,
    cam: &Camera,
    style: &ViewStyle,
    path: &std::path::Path,
) -> anyhow::Result<()> {
    let o = effective(o, cam);
    let d = slice_drawing(model, ctx, sec, o.projection, cam).map_err(anyhow::Error::msg)?;
    export_slice(&d, &o, auto_width(&d, &o, cam, style), path)
}

fn wide_code(w: CadWide) -> u64 {
    match w {
        CadWide::None => 0,
        CadWide::Polyline => 1,
        CadWide::WideLine => 2,
    }
}

fn wide_of(c: u64) -> CadWide {
    match c {
        1 => CadWide::Polyline,
        2 => CadWide::WideLine,
        _ => CadWide::None,
    }
}

/// The options kept between exports (`export2d.json`, key "slice"; SketchUp's preferences of
/// the "Acad Section Exporter").
pub fn to_json(o: &SliceOptions) -> Value {
    json!({
        "exportSectionType": if o.projection == SliceProjection::Screen { 1 } else { 0 },
        "fullScale": o.full_scale, "inDrawing": o.drawing_mm, "inModel": o.model_mm, "width": o.width_mm,
        "exportAcadVersion": o.acad_version, "sectionLineType": wide_code(o.line_type),
        "sectionAutomatic": o.line_auto, "sectionWidth": o.line_width,
        "color": o.color, "fill": o.fill, "fillColor": o.fill_color,
    })
}

/// Saved options (SketchUp's first values for what is missing), in the model's `units`.
pub fn from_json(v: &Value, units: Units) -> SliceOptions {
    let d = SliceOptions::default();
    let b = |k: &str, x: bool| v.get(k).and_then(Value::as_bool).unwrap_or(x);
    let f = |k: &str, x: f64| v.get(k).and_then(Value::as_f64).filter(|y| *y > 0.0).unwrap_or(x);
    let n = |k: &str| v.get(k).and_then(Value::as_u64);
    let rgb =
        |k: &str, x: [u8; 3]| v.get(k).and_then(|c| serde_json::from_value::<[u8; 3]>(c.clone()).ok()).unwrap_or(x);
    SliceOptions {
        projection: if n("exportSectionType") == Some(1) {
            SliceProjection::Screen
        } else {
            SliceProjection::TrueSection
        },
        full_scale: b("fullScale", d.full_scale),
        drawing_mm: f("inDrawing", d.drawing_mm),
        model_mm: f("inModel", d.model_mm),
        width_mm: f("width", d.width_mm),
        unit: units,
        acad_version: saved_acad_version(v).unwrap_or(d.acad_version),
        line_type: n("sectionLineType").map_or(d.line_type, wide_of),
        line_auto: b("sectionAutomatic", d.line_auto),
        line_width: f("sectionWidth", d.line_width),
        color: rgb("color", d.color),
        fill: b("fill", d.fill),
        fill_color: rgb("fillColor", d.fill_color),
    }
}

/// The saved options of the slice export for a model in `units`.
pub fn saved(units: Units) -> SliceOptions {
    from_json(crate::export2d::read_prefs().get("slice").unwrap_or(&Value::Null), units)
}

/// Key of "Toujours proposer les options de tranches" in `export2d.json` (SketchUp's
/// preference `HLROptions` › `alwaysPrompt`, off at first).
const ALWAYS_PROMPT: &str = "slice_always_prompt";

/// "Toujours proposer les options de tranches": the options window shows again after the
/// file dialog when it was not opened from there.
pub fn always_prompt() -> bool {
    crate::export2d::read_prefs().get(ALWAYS_PROMPT).and_then(Value::as_bool).unwrap_or(false)
}

/// Keeps the options of the window (its OK).
pub fn keep(d: &SliceDialog) {
    crate::export2d::write_pref("slice", to_json(&d.options));
    crate::export2d::write_pref(ALWAYS_PROMPT, json!(d.always_prompt));
}

const F_DRAWING: usize = 0;
const F_MODEL: usize = 1;
const F_WIDTH: usize = 2;
const F_HEIGHT: usize = 3;
const F_LINE: usize = 4;

/// The options window.
#[derive(Clone, Debug)]
pub struct SliceDialog {
    pub options: SliceOptions,
    /// "Toujours proposer les options de tranches".
    pub always_prompt: bool,
    /// The file already chosen in the file dialog ("Toujours proposer…": OK exports there);
    /// `None` when the file is asked after the window.
    pub path: Option<std::path::PathBuf>,
    /// Width and height of what the drawing spans, model millimetres, for each projection
    /// (true section, screen).
    extents: [DVec2; 2],
    /// The view is in perspective (no real scale for the screen projection).
    perspective: bool,
    texts: [String; 5],
}

fn fmt(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').replace('.', ",")
}

impl SliceDialog {
    pub fn new(options: SliceOptions, extents: [DVec2; 2], perspective: bool) -> SliceDialog {
        let mut d = SliceDialog {
            options,
            always_prompt: always_prompt(),
            path: None,
            extents,
            perspective,
            texts: Default::default(),
        };
        d.sync();
        d
    }

    /// "Paramètres par défaut": SketchUp's first options, the drawing at 1:1.
    pub fn reset(&mut self) {
        let units = self.options.unit;
        self.options = SliceOptions { unit: units, width_mm: self.extents[0].x, ..Default::default() };
        self.sync();
    }

    fn extent(&self) -> DVec2 {
        self.extents[usize::from(self.options.projection == SliceProjection::Screen)]
    }

    fn aspect(&self) -> f64 {
        let e = self.extent();
        e.x / e.y.max(1e-9)
    }

    /// The real scale can be chosen (not for the screen projection of a perspective view).
    fn scale_allowed(&self) -> bool {
        !(self.perspective && self.options.projection == SliceProjection::Screen)
    }

    /// Size of the drawing on paper, millimetres.
    fn page(&self) -> DVec2 {
        let o = &self.options;
        let k = if o.full_scale && self.scale_allowed() {
            o.drawing_mm / o.model_mm.max(1e-9)
        } else {
            o.width_mm / self.extent().x.max(1e-9)
        };
        self.extent() * k
    }

    fn sync(&mut self) {
        let u = self.options.unit.factor();
        let page = self.page();
        let o = &self.options;
        self.texts = [o.drawing_mm / u, o.model_mm / u, page.x / u, page.y / u, o.line_width].map(fmt);
    }

    /// A value typed in field `f` (lengths in the model's unit, the line width in mm).
    fn typed(&mut self, f: usize) {
        let Some(v) = crate::export2d::parse_num(&self.texts[f]).filter(|v| *v > 0.0) else { return };
        let u = self.options.unit.factor();
        let aspect = self.aspect();
        let o = &mut self.options;
        match f {
            F_DRAWING => o.drawing_mm = v * u,
            F_MODEL => o.model_mm = v * u,
            F_WIDTH => o.width_mm = v * u,
            F_HEIGHT => o.width_mm = v * u * aspect,
            F_LINE => o.line_width = v,
            _ => {}
        }
        let page = self.page();
        if f != F_WIDTH {
            self.texts[F_WIDTH] = fmt(page.x / u);
        }
        if f != F_HEIGHT {
            self.texts[F_HEIGHT] = fmt(page.y / u);
        }
    }

    /// "Échelle réelle" turned off: the width starts at the size the scale gave.
    fn set_full_scale(&mut self, on: bool) {
        if !on {
            self.options.width_mm = self.page().x;
        }
        self.options.full_scale = on;
        self.sync();
    }
}

/// The options window with the saved options, for the plane to export.
pub fn dialog(app: &EsquisseApp) -> Option<SliceDialog> {
    let sec = section_to_export(app)?;
    let (m, ctx) = (&app.doc.model, app.doc.ctx_def());
    let ext = |p| slice_drawing(m, ctx, sec, p, &app.cam).map(|d| d.extent()).unwrap_or(DVec2::ONE);
    let extents = [ext(SliceProjection::TrueSection), ext(SliceProjection::Screen)];
    let mut options = saved(m.units);
    if crate::export2d::read_prefs().get("slice").is_none() {
        // First time: the drawing at 1:1.
        options.width_mm = extents[0].x;
    }
    Some(SliceDialog::new(options, extents, app.cam.perspective))
}

/// The options window first, then the file (where SketchUp's file dialog is not available).
pub fn open(app: &mut EsquisseApp) {
    app.ui_state.extra.slice = dialog(app);
}

/// File › Export › Section Slice (`CExportSectionDialog`): the file dialog first, with
/// "Options..." (Windows); else the options window first.
pub fn start(app: &mut EsquisseApp, ctx: &egui::Context) {
    if section_to_export(app).is_none() {
        return;
    }
    if !crate::file_flow::open(app, ctx, crate::file_flow::Purpose::Slice) {
        open(app);
    }
}

/// The file dialog: DWG, DXF, SVG, "Options..." for each, OK "Exporter".
pub fn request(app: &EsquisseApp) -> crate::filedlg::Request {
    crate::filedlg::Request::save("Exporte les tranches 2D", &file_name(app), file_types().to_vec(), 0, "export")
        .ok_label("Exporter")
        .options(|_, _| crate::filedlg::Controls { options: true, image: false })
}

fn file_name(app: &EsquisseApp) -> String {
    let stem = app.doc.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
    stem.unwrap_or_else(|| "tranche".into())
}

/// The file chosen in the dialog, written with the saved options; with "Toujours proposer les
/// options de tranches" and the options not opened from the dialog, the options window
/// comes first (as SketchUp does after its `CExportSectionDialog`).
pub fn export_chosen(app: &mut EsquisseApp, ctx: &egui::Context, path: std::path::PathBuf, options_shown: bool) {
    let Some(mut d) = dialog(app) else {
        app.say(ctx, "Aucun plan de section actif.");
        return;
    };
    if d.always_prompt && !options_shown {
        d.path = Some(path);
        app.ui_state.extra.slice = Some(d);
        return;
    }
    let msg = write(app, &d.options, &path);
    app.say(ctx, msg);
}

/// Writes the slice of the plane to export to `path`; the message to show.
fn write(app: &EsquisseApp, o: &SliceOptions, p: &std::path::Path) -> String {
    let Some(sec) = section_to_export(app) else { return "Aucun plan de section actif.".into() };
    match export_file(&app.doc.model, app.doc.ctx_def(), sec, o, &app.cam, &app.style, p) {
        Ok(()) => format!("Tranche exportée : {}", p.display()),
        Err(e) => format!("Échec de l'export : {e:#}"),
    }
}

/// The file types of the export: SketchUp's two, then Esquisse's SVG.
pub fn file_types() -> [FileType; 3] {
    [
        FileType::new("Fichier AutoCAD DWG", &["dwg"]),
        FileType::new("Fichier AutoCAD DXF", &["dxf"]),
        FileType::new("Dessin vectoriel SVG", &["svg"]),
    ]
}

fn length_field(ui: &mut egui::Ui, texts: &mut [String; 5], f: usize, label: &str, on: bool, unit: &str) -> bool {
    let l = ui.label(label);
    let r = ui
        .add_enabled(on, egui::TextEdit::singleline(&mut texts[f]).desired_width(110.0).id_salt(("slice_field", f)))
        .labelled_by(l.id);
    ui.label(unit);
    ui.end_row();
    r.changed()
}

/// Title of the options window (`COptionsDlgSection`).
pub const TITLE: &str = "Options d'exportation DWG/DXF";

/// The options window, alone (the file is asked after it, or was chosen before it with
/// "Toujours proposer les options de tranches").
pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(mut dlg) = app.ui_state.extra.slice.take() else { return };
    let mut action = None;
    centered(TITLE).min_width(440.0).show(ctx, |ui| {
        options_ui(ui, &mut dlg);
        ui.horizontal(|ui| {
            if ui.button("Paramètres par défaut").clicked() {
                action = Some(2);
            }
            let ok = if dlg.path.is_some() { "OK" } else { "Exporter..." };
            if ui.button(ok).clicked() {
                action = Some(1);
            }
            if ui.button("Annuler").clicked() {
                action = Some(0);
            }
        });
    });
    match action {
        Some(0) => {}
        Some(1) => {
            keep(&dlg);
            let msg = match &dlg.path {
                Some(p) => Some(write(app, &dlg.options, p)),
                None => export_dialog(app, &dlg.options),
            };
            if let Some(m) = msg {
                app.say(ctx, m);
            }
        }
        Some(2) => {
            dlg.reset();
            app.ui_state.extra.slice = Some(dlg);
        }
        _ => app.ui_state.extra.slice = Some(dlg),
    }
}

/// The controls of the options window, above its buttons.
pub fn options_ui(ui: &mut egui::Ui, dlg: &mut SliceDialog) {
    let unit = dlg.options.unit.suffix();
    {
        ui.horizontal(|ui| {
            ui.label("Version AutoCAD");
            let o = &mut dlg.options;
            egui::ComboBox::from_id_salt("slice_acad").selected_text(ACAD_VERSIONS[o.acad_version]).show_ui(ui, |ui| {
                for (i, v) in ACAD_VERSIONS.iter().enumerate() {
                    ui.selectable_value(&mut o.acad_version, i, *v);
                }
            });
        });
        ui.separator();
        let before = dlg.options.projection;
        ui.radio_value(&mut dlg.options.projection, SliceProjection::TrueSection, "Section réelle (orthographique)");
        ui.radio_value(&mut dlg.options.projection, SliceProjection::Screen, "Projection à l’écran");
        if dlg.options.projection != before {
            dlg.sync();
        }
        ui.separator();
        ui.label(egui::RichText::new("Taille et échelle de dessin").strong());
        let allowed = dlg.scale_allowed();
        let mut full = dlg.options.full_scale && allowed;
        if ui.add_enabled(allowed, egui::Checkbox::new(&mut full, "Échelle réelle (1 : 1)")).changed() {
            dlg.set_full_scale(full);
        }
        let mut typed = None;
        egui::Grid::new("slice_size").num_columns(3).show(ui, |ui| {
            for (f, label, on) in [
                (F_DRAWING, "Dans le dessin", full),
                (F_MODEL, "Dans modèle", full),
                (F_WIDTH, "Largeur", !full),
                (F_HEIGHT, "Hauteur", !full),
            ] {
                if length_field(ui, &mut dlg.texts, f, label, on, unit) {
                    typed = Some(f);
                }
            }
        });
        ui.separator();
        ui.label(egui::RichText::new("Lignes de coupe").strong());
        ui.horizontal(|ui| {
            let o = &mut dlg.options;
            ui.radio_value(&mut o.line_type, CadWide::None, "Aucune");
            ui.radio_value(&mut o.line_type, CadWide::Polyline, "Polylignes avec largeur");
            ui.radio_value(&mut o.line_type, CadWide::WideLine, "Entités ligne épaisse");
        });
        let auto = dlg.options.line_auto;
        egui::Grid::new("slice_line").num_columns(3).show(ui, |ui| {
            if length_field(ui, &mut dlg.texts, F_LINE, "Largeur", !auto, "mm") {
                typed = Some(F_LINE);
            }
        });
        ui.checkbox(&mut dlg.options.line_auto, "Automatique");
        if let Some(f) = typed {
            dlg.typed(f);
        }
        ui.separator();
        ui.checkbox(&mut dlg.always_prompt, "Toujours proposer les options de tranches");
        ui.separator();
        ui.label(egui::RichText::new("SVG").strong());
        ui.horizontal(|ui| {
            ui.label("Couleur des traits");
            ui.color_edit_button_srgb(&mut dlg.options.color);
            ui.checkbox(&mut dlg.options.fill, "Remplir la coupe");
            ui.add_enabled_ui(dlg.options.fill, |ui| ui.color_edit_button_srgb(&mut dlg.options.fill_color));
        });
        ui.separator();
    }
}

/// Asks for the file (DWG first, as SketchUp) and writes the slice; the message to show.
fn export_dialog(app: &EsquisseApp, o: &SliceOptions) -> Option<String> {
    if section_to_export(app).is_none() {
        return Some("Aucun plan de section actif.".into());
    }
    let c = crate::filedlg::run(request(app))?;
    Some(write(app, o, &c.path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_survive_the_preferences() {
        let o = SliceOptions {
            projection: SliceProjection::Screen,
            full_scale: false,
            drawing_mm: 1.0,
            model_mm: 50.0,
            width_mm: 300.0,
            unit: Units::Centimeters,
            acad_version: 4,
            line_type: CadWide::WideLine,
            line_auto: false,
            line_width: 0.5,
            color: [10, 20, 30],
            fill: true,
            fill_color: [1, 2, 3],
        };
        assert_eq!(from_json(&to_json(&o), Units::Centimeters), o);
        assert_eq!(from_json(&Value::Null, Units::Meters), SliceOptions { unit: Units::Meters, ..Default::default() });
    }

    #[test]
    fn the_window_keeps_the_proportions() {
        // A 1000 × 400 mm cut, in centimetres.
        let o = SliceOptions { unit: Units::Centimeters, ..Default::default() };
        let mut d = SliceDialog::new(o, [DVec2::new(1000.0, 400.0), DVec2::new(800.0, 500.0)], true);
        assert_eq!(d.texts[F_WIDTH], "100");
        d.texts[F_DRAWING] = "1".into();
        d.typed(F_DRAWING);
        d.texts[F_MODEL] = "5".into();
        d.typed(F_MODEL);
        // 1 cm for 5 cm: 20 × 8 cm.
        assert_eq!((d.texts[F_WIDTH].as_str(), d.texts[F_HEIGHT].as_str()), ("20", "8"));
        d.set_full_scale(false);
        assert!((d.options.width_mm - 200.0).abs() < 1e-9);
        d.texts[F_HEIGHT] = "10".into();
        d.typed(F_HEIGHT);
        assert!((d.options.width_mm - 250.0).abs() < 1e-9);
        // The screen projection of a perspective view has no real scale.
        d.options.projection = SliceProjection::Screen;
        assert!(!d.scale_allowed());
    }
}
