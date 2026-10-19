//! File › Export › 2D Graphic as a vector drawing: PDF and EPS (SketchUp's `CHLRExportPDF`
//! and `CHLRExportEPS`, options of `CPdfOptionsDlg`, preferences `PDFOptions`/`EPSOptions`)
//! and AutoCAD DWG / DXF (the `skp2acad` exporter, options `COptionsDlgHLR`; both written by
//! `esq_core::dwg` in the version chosen, Version 12 to AutoCAD 2018). The drawing is the
//! hidden-line view of `esq_core::hlr`; the options set its size and its lines:
//!
//! - "Taille et échelle de dessin": full scale (parallel projection only), with the ratio
//!   "Dans l'exportation en mode ligne cachée" / "Dans SketchUp"; else a width (the height
//!   follows the view). As SketchUp, full scale is the default when the view is in parallel
//!   projection and fits in 100 inches;
//! - "Lignes profilées", "Lignes de section", "Arêtes prolongées": shown or not, as wide as
//!   on screen ("Adapter à l'affichage écran") or as given;
//! - for AutoCAD: the AutoCAD version, and profiles / section lines as plain lines, wide
//!   polylines or thick lines, on their own layer if asked.

use std::path::PathBuf;

use eframe::egui;
use esq_core::dwg::DwgVersion;
use esq_core::glam::DVec2;
use esq_core::hlr::{
    Fill, HlrDrawing, HlrLine, HlrPolygon, HlrSettings, HlrText, HlrView, LineKind, Shading, hidden_lines,
};
use esq_core::model::{AnnotKind, Units};
use esq_core::vector2d::{
    CadLines, CadOptions, CadWide, Layout, Strokes, cad_drawing, dwg_doc, to_dxf, to_eps, to_pdf,
};
use serde_json::{Value, json};

use crate::app::EsquisseApp;
use crate::camera::Camera;
use crate::export2d::parse_num;
use crate::filedlg::FileType;
use crate::style::{FaceStyle, ViewStyle};
use crate::ui::centered;

/// Widths on screen, pixels (render.rs, scene.rs, shader.wgsl).
const PROFILE_PX: f64 = 2.0;
const SECTION_PX: f64 = 3.0;
const EXTENSION_PX: f64 = 3.0;
/// SketchUp leaves full scale when the page would be larger than this (inches).
const MAX_PAGE_IN: f64 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecKind {
    Pdf,
    Eps,
    Dwg,
    Dxf,
}

impl VecKind {
    pub const ALL: [VecKind; 4] = [VecKind::Pdf, VecKind::Eps, VecKind::Dwg, VecKind::Dxf];

    pub fn file_type(self) -> FileType {
        match self {
            VecKind::Pdf => FileType::new("Fichier PDF", &["pdf"]),
            VecKind::Eps => FileType::new("Fichier EPS", &["eps"]),
            VecKind::Dwg => FileType::new("AutoCAD DWG", &["dwg"]),
            VecKind::Dxf => FileType::new("AutoCAD DXF", &["dxf"]),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            VecKind::Pdf => "pdf",
            VecKind::Eps => "eps",
            VecKind::Dwg => "dwg",
            VecKind::Dxf => "dxf",
        }
    }

    fn title(self) -> &'static str {
        match self {
            VecKind::Pdf => "Options d'exportation de PDF",
            VecKind::Eps => "Options d'exportation EPS",
            VecKind::Dwg | VecKind::Dxf => "Options d'exportation DWG/DXF",
        }
    }

    fn cad(self) -> bool {
        matches!(self, VecKind::Dwg | VecKind::Dxf)
    }

    /// Key of the remembered options: DWG and DXF share theirs, as they share the dialog.
    pub(crate) fn options_key(self) -> &'static str {
        if self.cad() { "dxf" } else { self.key() }
    }
}

/// Units of the dialog's fields (`CPdfOptionsDlg`: inches, feet, millimetres, centimetres,
/// metres, points).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawUnit {
    Inches,
    Feet,
    Millimeters,
    Centimeters,
    Meters,
    Points,
}

impl DrawUnit {
    pub const ALL: [DrawUnit; 6] = [
        DrawUnit::Inches,
        DrawUnit::Feet,
        DrawUnit::Millimeters,
        DrawUnit::Centimeters,
        DrawUnit::Meters,
        DrawUnit::Points,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DrawUnit::Inches => "Pouces",
            DrawUnit::Feet => "Pieds",
            DrawUnit::Millimeters => "Millimètres",
            DrawUnit::Centimeters => "Centimètres",
            DrawUnit::Meters => "Mètres",
            DrawUnit::Points => "Points",
        }
    }

    pub fn mm(self) -> f64 {
        match self {
            DrawUnit::Inches => 25.4,
            DrawUnit::Feet => 304.8,
            DrawUnit::Millimeters => 1.0,
            DrawUnit::Centimeters => 10.0,
            DrawUnit::Meters => 1000.0,
            DrawUnit::Points => 25.4 / 72.0,
        }
    }

    fn code(self) -> u64 {
        self as u64
    }

    fn of(c: u64) -> DrawUnit {
        DrawUnit::ALL.get(c as usize).copied().unwrap_or(DrawUnit::Millimeters)
    }

    fn of_units(u: Units) -> DrawUnit {
        match u {
            Units::Inches => DrawUnit::Inches,
            // The dialog has no yards: feet are the closest of its units.
            Units::Feet | Units::Yards => DrawUnit::Feet,
            Units::Millimeters => DrawUnit::Millimeters,
            Units::Centimeters => DrawUnit::Centimeters,
            Units::Meters => DrawUnit::Meters,
        }
    }

    /// The CAD unit of the drawing's coordinates (points become inches).
    fn units(self) -> Units {
        match self {
            DrawUnit::Inches | DrawUnit::Points => Units::Inches,
            DrawUnit::Feet => Units::Feet,
            DrawUnit::Millimeters => Units::Millimeters,
            DrawUnit::Centimeters => Units::Centimeters,
            DrawUnit::Meters => Units::Meters,
        }
    }
}

/// The AutoCAD versions of `skp2acad`'s list, indexed by its `ExportAcadVersion`.
pub const ACAD_VERSIONS: [&str; 9] = DwgVersion::LABELS;

/// The saved version of an export: `ExportAcadVersion` (0 = Version 12 … 8 = AutoCAD 2018),
/// or the index in the list of the first versions of Esquisse, which started at AutoCAD 2000.
pub fn saved_acad_version(v: &Value) -> Option<usize> {
    match v.get("exportAcadVersion").and_then(Value::as_u64) {
        Some(code) => Some((code as usize).min(ACAD_VERSIONS.len() - 1)),
        None => v.get("acadVersion").and_then(Value::as_u64).map(|i| (i as usize + 3).min(ACAD_VERSIONS.len() - 1)),
    }
}

/// Every option of the hidden-line exports (lengths in millimetres, shown in their unit).
#[derive(Clone, Debug, PartialEq)]
pub struct VecOptions {
    pub full_scale: bool,
    /// Drawing ratio: `drawing_mm` in the drawing for `model_mm` in the model.
    pub drawing_mm: f64,
    pub model_mm: f64,
    pub drawing_unit: DrawUnit,
    pub model_unit: DrawUnit,
    /// Width of the drawing when not at full scale (the height follows the view).
    pub width_mm: f64,
    pub size_unit: DrawUnit,
    pub profiles: bool,
    pub profile_auto: bool,
    pub profile_mm: f64,
    pub sections: bool,
    pub section_auto: bool,
    pub section_mm: f64,
    pub extensions: bool,
    pub extension_auto: bool,
    pub extension_mm: f64,
    pub line_unit: DrawUnit,
    /// `ExportAcadVersion`: 0 (Version 12) to 8 (AutoCAD 2018), 3 (AutoCAD 2000) at first.
    pub acad_version: usize,
    pub profile_cad: CadWide,
    pub profile_layer: bool,
    pub section_cad: CadWide,
    pub section_layer: bool,
}

/// What the current view gives: its size in pixels, the model millimetres per pixel (parallel
/// projection), whether it is in perspective.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewInfo {
    pub px: DVec2,
    pub mm_per_px: f64,
    pub perspective: bool,
}

impl ViewInfo {
    pub fn of(cam: &Camera) -> ViewInfo {
        ViewInfo { px: cam.size, mm_per_px: cam.pixel_size(), perspective: cam.perspective }
    }
}

fn cad_code(c: CadWide) -> u64 {
    match c {
        CadWide::None => 0,
        CadWide::Polyline => 1,
        CadWide::WideLine => 2,
    }
}

fn cad_of(c: u64) -> CadWide {
    match c {
        1 => CadWide::Polyline,
        2 => CadWide::WideLine,
        _ => CadWide::None,
    }
}

impl VecOptions {
    /// SketchUp's first values for a view: full scale in parallel
    /// projection when the page stays within 100 inches, else the largest page side of
    /// 100 inches; in perspective, one point per pixel. Lines as on screen.
    pub fn for_view(v: &ViewInfo, style: &ViewStyle, units: Units) -> VecOptions {
        let max_mm = MAX_PAGE_IN * 25.4;
        let full = v.px * v.mm_per_px;
        let (full_scale, width_mm) = if v.perspective {
            (false, v.px.x * 25.4 / 72.0)
        } else if full.max_element() <= max_mm {
            (true, full.x)
        } else {
            (false, full.x * max_mm / full.max_element())
        };
        let unit = DrawUnit::of_units(units);
        VecOptions {
            full_scale,
            drawing_mm: 1.0,
            model_mm: 1.0,
            drawing_unit: unit,
            model_unit: unit,
            width_mm,
            size_unit: unit,
            profiles: style.profiles,
            profile_auto: true,
            profile_mm: 0.5,
            sections: style.section_cuts,
            section_auto: true,
            section_mm: 0.7,
            extensions: style.extension,
            extension_auto: true,
            extension_mm: 1.0,
            line_unit: DrawUnit::Millimeters,
            acad_version: DwgVersion::R2000.code(),
            profile_cad: CadWide::None,
            profile_layer: false,
            section_cad: CadWide::None,
            section_layer: false,
        }
    }

    /// The DWG / DXF version chosen.
    pub fn version(&self) -> DwgVersion {
        DwgVersion::from_code(self.acad_version).unwrap_or_default()
    }

    /// Paper millimetres per pixel of the drawing.
    pub fn k(&self, v: &ViewInfo) -> f64 {
        if self.full_scale && !v.perspective {
            v.mm_per_px * self.drawing_mm / self.model_mm.max(1e-9)
        } else {
            self.width_mm / v.px.x.max(1.0)
        }
    }

    /// Size of the drawing, millimetres.
    pub fn page(&self, v: &ViewInfo) -> DVec2 {
        v.px * self.k(v)
    }

    pub fn strokes(&self, v: &ViewInfo) -> Strokes {
        let k = self.k(v);
        let w = |on: bool, auto: bool, px: f64, mm: f64| on.then_some(if auto { px * k } else { mm });
        Strokes {
            edge: k,
            profile: w(self.profiles, self.profile_auto, PROFILE_PX, self.profile_mm),
            section: w(self.sections, self.section_auto, SECTION_PX, self.section_mm),
            extension: w(self.extensions, self.extension_auto, EXTENSION_PX, self.extension_mm),
        }
    }

    /// The options of the AutoCAD drawing; widths and lengths in drawing units.
    pub fn cad(&self, v: &ViewInfo) -> (CadOptions, [f64; 2]) {
        let unit = self.size_unit.units();
        let k = self.k(v) / unit.factor();
        let lines = |mode: CadWide, auto: bool, mm: f64, layer: bool| CadLines {
            mode,
            width: (!auto).then_some(mm / unit.factor()),
            own_layer: layer,
        };
        let o = CadOptions {
            unit,
            profiles: lines(
                if self.profiles { self.profile_cad } else { CadWide::None },
                self.profile_auto,
                self.profile_mm,
                self.profile_layer,
            ),
            sections: lines(self.section_cad, self.section_auto, self.section_mm, self.section_layer),
            extension: self.extensions.then_some(if self.extension_auto {
                EXTENSION_PX * k
            } else {
                self.extension_mm / unit.factor()
            }),
        };
        (o, [PROFILE_PX * k, SECTION_PX * k])
    }

    pub fn to_json(&self) -> Value {
        json!({
            "fullScale": self.full_scale, "drawingLength": self.drawing_mm, "modelLength": self.model_mm,
            "drawingUnits": self.drawing_unit.code(), "modelUnits": self.model_unit.code(),
            "width": self.width_mm, "widthUnits": self.size_unit.code(),
            "outputProfileLines": self.profiles, "matchScreenProfiles": self.profile_auto, "profileWidth": self.profile_mm,
            "outputSectionLines": self.sections, "matchScreenSection": self.section_auto, "sectionWidth": self.section_mm,
            "outputEdgeExtensions": self.extensions, "matchScreenExtensions": self.extension_auto,
            "extensionLength": self.extension_mm, "lineWidthUnits": self.line_unit.code(),
            "exportAcadVersion": self.acad_version,
            "profileCad": cad_code(self.profile_cad), "profileLayer": self.profile_layer,
            "sectionCad": cad_code(self.section_cad), "sectionLayer": self.section_layer,
        })
    }

    /// Options saved earlier, completed by `base` (the view's defaults). The size and scale
    /// come from the view each time, as in SketchUp.
    pub fn from_json(v: &Value, base: VecOptions) -> VecOptions {
        let b = |k: &str, d: bool| v.get(k).and_then(Value::as_bool).unwrap_or(d);
        let f = |k: &str, d: f64| v.get(k).and_then(Value::as_f64).filter(|x| *x > 0.0).unwrap_or(d);
        let u = |k: &str, d: DrawUnit| v.get(k).and_then(Value::as_u64).map(DrawUnit::of).unwrap_or(d);
        let n = |k: &str| v.get(k).and_then(Value::as_u64);
        VecOptions {
            drawing_unit: u("drawingUnits", base.drawing_unit),
            model_unit: u("modelUnits", base.model_unit),
            size_unit: u("widthUnits", base.size_unit),
            profiles: b("outputProfileLines", base.profiles),
            profile_auto: b("matchScreenProfiles", base.profile_auto),
            profile_mm: f("profileWidth", base.profile_mm),
            sections: b("outputSectionLines", base.sections),
            section_auto: b("matchScreenSection", base.section_auto),
            section_mm: f("sectionWidth", base.section_mm),
            extensions: b("outputEdgeExtensions", base.extensions),
            extension_auto: b("matchScreenExtensions", base.extension_auto),
            extension_mm: f("extensionLength", base.extension_mm),
            line_unit: u("lineWidthUnits", base.line_unit),
            acad_version: saved_acad_version(v).unwrap_or(base.acad_version),
            profile_cad: n("profileCad").map_or(base.profile_cad, cad_of),
            profile_layer: b("profileLayer", base.profile_layer),
            section_cad: n("sectionCad").map_or(base.section_cad, cad_of),
            section_layer: b("sectionLayer", base.section_layer),
            ..base
        }
    }
}

/// The drawing's settings from the view's style and the model's lighting.
pub fn hlr_settings(style: &ViewStyle, model: &esq_core::model::Model, cam: &Camera, o: &VecOptions) -> HlrSettings {
    let s = &model.shadow;
    let shading = if s.display || s.use_sun_for_shading {
        Shading::Sun { dir: s.sun_direction(), light: s.light as f64 / 100.0, dark: s.dark as f64 / 100.0 }
    } else {
        Shading::Camera((cam.right() * 0.35 + cam.up() * 0.55 - cam.forward() * 0.75).normalize())
    };
    let fill = match style.faces {
        FaceStyle::Wireframe => Fill::None,
        FaceStyle::HiddenLine => Fill::Color(style.bg_color),
        FaceStyle::Monochrome => {
            Fill::Materials { front: style.front_color, back: style.back_color, shading, materials: false }
        }
        _ => Fill::Materials { front: style.front_color, back: style.back_color, shading, materials: true },
    };
    HlrSettings {
        fill,
        edges: style.edges,
        profiles: o.profiles,
        section_cuts: style.section_cuts,
        section_fill: (style.section_fill && style.faces != FaceStyle::Wireframe).then_some(style.section_fill_color),
        edge_color: style.edge_color,
    }
}

pub fn hlr_view(cam: &Camera) -> HlrView {
    HlrView {
        view_proj: cam.view_proj(),
        eye: cam.eye(),
        forward: cam.forward(),
        perspective: cam.perspective,
        size: cam.size,
    }
}

/// Adds the visible texts and dimensions, drawn over the model as the view draws them
/// (`annot.rs`): lines, arrowheads, leader dots and labels.
pub fn add_annotations(d: &mut HlrDrawing, cam: &Camera, model: &esq_core::model::Model) {
    let ink = [crate::annot::INK.r(), crate::annot::INK.g(), crate::annot::INK.b()];
    let mut list = Vec::new();
    esq_core::walk::visit(model, |p| {
        for (_, a) in p.ents.annots.iter() {
            if !a.hidden && model.layers.get(a.layer).is_none_or(|x| x.visible) {
                list.push(a.transformed(&p.xform));
            }
        }
    });
    for a in &list {
        let text = crate::annot::label(a, model.units);
        let Some(l) = crate::annot::layout(cam, a, &text) else { continue };
        for (s, e) in &l.segs {
            d.lines.push(HlrLine { a: *s, b: *e, kind: LineKind::Edge, color: ink, ends: [false, false] });
        }
        for (tip, dir) in &l.arrows {
            let side = DVec2::new(-dir.y, dir.x) * 3.0;
            let back = *tip - *dir * 9.0;
            d.marks.push(HlrPolygon { pts: vec![*tip, back + side, back - side], color: ink });
        }
        if let AnnotKind::Text { .. } = a.kind
            && let Some((s, _)) = l.segs.first()
        {
            let pts = (0..8).map(|i| *s + DVec2::from_angle(i as f64 * std::f64::consts::FRAC_PI_4) * 2.5).collect();
            d.marks.push(HlrPolygon { pts, color: ink });
        }
        if !text.trim().is_empty() {
            let lines = text.lines().map(str::to_owned).collect();
            d.texts.push(HlrText { pos: l.label, lines, size: crate::annot::FONT as f64, color: ink });
        }
    }
}

/// The drawing of the view: the model in hidden lines, its texts and dimensions.
pub fn drawing(model: &esq_core::model::Model, style: &ViewStyle, cam: &Camera, o: &VecOptions) -> HlrDrawing {
    let mut d = hidden_lines(model, &hlr_view(cam), &hlr_settings(style, model, cam, o));
    add_annotations(&mut d, cam, model);
    d
}

/// Writes the drawing of `kind` to `path`.
pub fn export(
    kind: VecKind,
    path: &std::path::Path,
    model: &esq_core::model::Model,
    style: &ViewStyle,
    cam: &Camera,
    o: &VecOptions,
    title: &str,
) -> anyhow::Result<()> {
    let info = ViewInfo::of(cam);
    let d = drawing(model, style, cam, o);
    let lay = Layout { k: o.k(&info) };
    match kind {
        VecKind::Pdf => std::fs::write(path, to_pdf(&d, &lay, &o.strokes(&info), title))?,
        VecKind::Eps => std::fs::write(path, to_eps(&d, &lay, &o.strokes(&info), title))?,
        VecKind::Dwg => {
            let (cad, auto) = o.cad(&info);
            let doc = dwg_doc(&cad_drawing(&d, &lay, &cad, auto), cad.unit);
            std::fs::write(path, esq_core::dwg::write(&doc, o.version())?)?
        }
        VecKind::Dxf => {
            let (cad, auto) = o.cad(&info);
            std::fs::write(path, to_dxf(&cad_drawing(&d, &lay, &cad, auto), cad.unit, o.version())?)?
        }
    }
    Ok(())
}

/// The options window of a vector export.
pub struct VecDialog {
    pub path: PathBuf,
    pub kind: VecKind,
    pub options: VecOptions,
    pub view: ViewInfo,
    texts: Vec<String>,
}

const F_WIDTH: usize = 0;
const F_HEIGHT: usize = 1;
const F_DRAWING: usize = 2;
const F_MODEL: usize = 3;
const F_PROFILE: usize = 4;
const F_SECTION: usize = 5;
const F_EXTENSION: usize = 6;

fn fmt(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.replace('.', ",")
}

impl VecDialog {
    pub fn new(path: PathBuf, kind: VecKind, options: VecOptions, view: ViewInfo) -> VecDialog {
        let mut d = VecDialog { path, kind, options, view, texts: vec![String::new(); 7] };
        d.sync();
        d
    }

    fn sync(&mut self) {
        let o = &self.options;
        let page = o.page(&self.view);
        self.texts[F_WIDTH] = fmt(page.x / o.size_unit.mm());
        self.texts[F_HEIGHT] = fmt(page.y / o.size_unit.mm());
        self.texts[F_DRAWING] = fmt(o.drawing_mm / o.drawing_unit.mm());
        self.texts[F_MODEL] = fmt(o.model_mm / o.model_unit.mm());
        self.texts[F_PROFILE] = fmt(o.profile_mm / o.line_unit.mm());
        self.texts[F_SECTION] = fmt(o.section_mm / o.line_unit.mm());
        self.texts[F_EXTENSION] = fmt(o.extension_mm / o.line_unit.mm());
    }

    /// A value typed in field `f`.
    fn typed(&mut self, f: usize) {
        let Some(v) = parse_num(&self.texts[f]).filter(|v| *v > 0.0) else { return };
        let o = &mut self.options;
        let aspect = self.view.px.x / self.view.px.y.max(1.0);
        match f {
            F_WIDTH => o.width_mm = v * o.size_unit.mm(),
            F_HEIGHT => o.width_mm = v * o.size_unit.mm() * aspect,
            F_DRAWING => o.drawing_mm = v * o.drawing_unit.mm(),
            F_MODEL => o.model_mm = v * o.model_unit.mm(),
            F_PROFILE => o.profile_mm = v * o.line_unit.mm(),
            F_SECTION => o.section_mm = v * o.line_unit.mm(),
            F_EXTENSION => o.extension_mm = v * o.line_unit.mm(),
            _ => {}
        }
        // The other size follows (the view's proportions).
        let page = self.options.page(&self.view);
        let u = self.options.size_unit.mm();
        if f != F_WIDTH {
            self.texts[F_WIDTH] = fmt(page.x / u);
        }
        if f != F_HEIGHT {
            self.texts[F_HEIGHT] = fmt(page.y / u);
        }
    }
}

/// The options of `kind` kept from the last export (else the defaults for the view).
pub fn saved_options(app: &EsquisseApp, kind: VecKind) -> VecOptions {
    let view = ViewInfo::of(&app.cam);
    let base = VecOptions::for_view(&view, &app.style, app.doc.model.units);
    let saved = crate::export2d::read_prefs();
    match saved.get(kind.options_key()) {
        Some(v) => VecOptions::from_json(v, base),
        None => base,
    }
}

/// Keeps the options of `kind` (OK of the options window).
pub fn save_options(kind: VecKind, o: &VecOptions) {
    crate::export2d::write_pref(kind.options_key(), o.to_json());
}

/// The options window of `kind`, for the current view.
pub fn dialog(app: &EsquisseApp, path: PathBuf, kind: VecKind) -> VecDialog {
    VecDialog::new(path, kind, saved_options(app, kind), ViewInfo::of(&app.cam))
}

/// Opens the options of the vector export of `path` (the type chosen in the save dialog).
pub fn open(app: &mut EsquisseApp, path: PathBuf, kind: VecKind) {
    app.ui_state.extra.export_vec = Some(dialog(app, path, kind));
}

/// Writes the drawing of `kind` to `path` with the options kept (the dialog's Export).
pub fn export_now(app: &mut EsquisseApp, ctx: &egui::Context, path: PathBuf, kind: VecKind) {
    let o = saved_options(app, kind);
    write_and_say(app, ctx, &path, kind, &o);
}

fn write_and_say(app: &mut EsquisseApp, ctx: &egui::Context, path: &std::path::Path, kind: VecKind, o: &VecOptions) {
    crate::export2d::write_pref(kind.options_key(), o.to_json());
    crate::export2d::write_pref("type", json!(kind.key()));
    let title = app.doc.title();
    let msg = match export(kind, path, &app.doc.model, &app.style, &app.cam, o, &title) {
        Ok(()) => format!("Graphique 2D exporté : {}", path.display()),
        Err(e) => format!("Échec de l'export : {e:#}"),
    };
    app.say(ctx, msg);
}

impl VecDialog {
    pub fn title(&self) -> &'static str {
        self.kind.title()
    }

    /// "Paramètres par défaut": the options SketchUp starts with for this view.
    pub fn reset(&mut self, app: &EsquisseApp) {
        self.options = VecOptions::for_view(&self.view, &app.style, app.doc.model.units);
        self.sync();
    }
}

fn unit_combo(ui: &mut egui::Ui, field: usize, u: &mut DrawUnit) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(("vec_unit", field)).selected_text(u.label()).show_ui(ui, |ui| {
        for x in DrawUnit::ALL {
            changed |= ui.selectable_value(u, x, x.label()).changed();
        }
    });
    changed
}

pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(mut dlg) = app.ui_state.extra.export_vec.take() else { return };
    let mut action = None;
    let kind = dlg.kind;
    centered(kind.title()).min_width(520.0).show(ctx, |ui| {
        options_ui(ui, &mut dlg);
        ui.horizontal(|ui| {
            if ui.button("Paramètres par défaut").clicked() {
                action = Some(2);
            }
            if ui.button("Exporter").clicked() {
                action = Some(1);
            }
            if ui.button("Annuler").clicked() {
                action = Some(0);
            }
        });
    });
    match action {
        Some(0) => {}
        Some(1) => write_and_say(app, ctx, &dlg.path.clone(), kind, &dlg.options.clone()),
        Some(2) => {
            dlg.reset(app);
            app.ui_state.extra.export_vec = Some(dlg);
        }
        _ => app.ui_state.extra.export_vec = Some(dlg),
    }
}

/// The controls of the options window of a vector export (`CPdfOptionsDlg`,
/// `COptionsDlgHLR`), above its buttons.
pub fn options_ui(ui: &mut egui::Ui, dlg: &mut VecDialog) {
    let kind = dlg.kind;
    {
        let parallel = !dlg.view.perspective;
        if kind.cad() {
            ui.horizontal(|ui| {
                ui.label("Version AutoCAD");
                let o = &mut dlg.options;
                egui::ComboBox::from_id_salt("acad_version").selected_text(ACAD_VERSIONS[o.acad_version]).show_ui(
                    ui,
                    |ui| {
                        for (i, v) in ACAD_VERSIONS.iter().enumerate() {
                            ui.selectable_value(&mut o.acad_version, i, *v);
                        }
                    },
                );
            });
            ui.separator();
        }
        ui.label(egui::RichText::new("Taille et échelle de dessin").strong());
        let mut changed = false;
        ui.add_enabled_ui(parallel, |ui| {
            changed |= ui.checkbox(&mut dlg.options.full_scale, "Échelle réelle (1 : 1)").changed();
        });
        let full = dlg.options.full_scale && parallel;
        egui::Grid::new("vec_size").num_columns(3).show(ui, |ui| {
            for (f, label) in [(F_WIDTH, "Largeur"), (F_HEIGHT, "Hauteur")] {
                let l = ui.label(label);
                let r = ui
                    .add_enabled(
                        !full,
                        egui::TextEdit::singleline(&mut dlg.texts[f])
                            .desired_width(90.0)
                            .min_size(egui::vec2(90.0, 0.0))
                            .id_salt(("vec_field", f)),
                    )
                    .labelled_by(l.id);
                if r.changed() {
                    dlg.typed(f);
                }
                if f == F_WIDTH {
                    changed |= unit_combo(ui, F_WIDTH, &mut dlg.options.size_unit);
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
            let (a, b) = if kind.cad() {
                ("Dans le dessin", "Dans modèle")
            } else {
                ("Dans l'exportation en mode ligne cachée", "Dans SketchUp")
            };
            for (f, label, unit) in [(F_DRAWING, a, 0), (F_MODEL, b, 1)] {
                let l = ui.label(label);
                let r = ui
                    .add_enabled(
                        full,
                        egui::TextEdit::singleline(&mut dlg.texts[f])
                            .desired_width(90.0)
                            .min_size(egui::vec2(90.0, 0.0))
                            .id_salt(("vec_field", f)),
                    )
                    .labelled_by(l.id);
                if r.changed() {
                    dlg.typed(f);
                }
                let u = if unit == 0 { &mut dlg.options.drawing_unit } else { &mut dlg.options.model_unit };
                changed |= unit_combo(ui, f, u);
                ui.end_row();
            }
        });
        ui.separator();
        let groups: [(&str, &str, usize); 3] = [
            ("Lignes profilées", "Afficher les profils", F_PROFILE),
            (if kind.cad() { "Lignes de coupe" } else { "Lignes de section" }, "Afficher lignes de section", F_SECTION),
            (
                "Arêtes prolongées",
                if kind.cad() { "Afficher les extensions" } else { "Prolonger les arêtes" },
                F_EXTENSION,
            ),
        ];
        for (title, show, f) in groups {
            ui.label(egui::RichText::new(title).strong());
            let o = &mut dlg.options;
            let (on, auto, cad, layer) = match f {
                F_PROFILE => {
                    (&mut o.profiles, &mut o.profile_auto, Some(&mut o.profile_cad), Some(&mut o.profile_layer))
                }
                F_SECTION => {
                    (&mut o.sections, &mut o.section_auto, Some(&mut o.section_cad), Some(&mut o.section_layer))
                }
                _ => (&mut o.extensions, &mut o.extension_auto, None, None),
            };
            ui.checkbox(on, show);
            if kind.cad()
                && let (Some(cad), Some(layer)) = (cad, layer)
            {
                ui.horizontal(|ui| {
                    ui.radio_value(cad, CadWide::None, "Aucune");
                    ui.radio_value(cad, CadWide::Polyline, "Polylignes avec largeur");
                    ui.radio_value(cad, CadWide::WideLine, "Entités ligne épaisse");
                });
                ui.checkbox(layer, "Placer sur un calque séparé");
            }
            let auto_label = if kind.cad() { "Automatique" } else { "Adapter à l'affichage écran (largeur auto)" };
            ui.checkbox(auto, auto_label);
            let editable = !*auto;
            ui.horizontal(|ui| {
                let l = ui.label(if f == F_EXTENSION { "Longueur" } else { "Largeur" });
                let r = ui
                    .add_enabled(
                        editable,
                        egui::TextEdit::singleline(&mut dlg.texts[f])
                            .desired_width(90.0)
                            .min_size(egui::vec2(90.0, 0.0))
                            .id_salt(("vec_field", f)),
                    )
                    .labelled_by(l.id);
                if r.changed() {
                    dlg.typed(f);
                }
                changed |= unit_combo(ui, f, &mut dlg.options.line_unit);
            });
        }
        ui.separator();
        if changed {
            dlg.sync();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::glam::DVec3;

    fn top_view(px: f64, mm_per_px: f64) -> ViewInfo {
        ViewInfo { px: DVec2::new(px, px / 2.0), mm_per_px, perspective: false }
    }

    #[test]
    fn full_scale_by_default_when_the_page_fits() {
        let style = ViewStyle::default();
        // 1000 px at 2 mm per pixel: a 2 m wide drawing at 1:1, within 100 inches.
        let v = top_view(1000.0, 2.0);
        let o = VecOptions::for_view(&v, &style, Units::Millimeters);
        assert!(o.full_scale);
        assert_eq!(o.page(&v), DVec2::new(2000.0, 1000.0));
        // 10 mm per pixel: 10 m, too big: the larger side is 100 inches.
        let v = top_view(1000.0, 10.0);
        let o = VecOptions::for_view(&v, &style, Units::Millimeters);
        assert!(!o.full_scale);
        assert!((o.page(&v).x - 2540.0).abs() < 1e-9);
        // Perspective: one point per pixel.
        let v = ViewInfo { perspective: true, ..top_view(720.0, 1.0) };
        let o = VecOptions::for_view(&v, &style, Units::Millimeters);
        assert!(!o.full_scale && (o.page(&v).x - 254.0).abs() < 1e-9);
        // A scale of 1:50 at full scale.
        let v = top_view(1000.0, 10.0);
        let o = VecOptions { full_scale: true, drawing_mm: 1.0, model_mm: 50.0, ..o };
        assert!((o.page(&v).x - 200.0).abs() < 1e-9);
    }

    #[test]
    fn line_widths_follow_the_screen_or_the_options() {
        let v = top_view(1000.0, 2.0);
        let mut o = VecOptions::for_view(&v, &ViewStyle::default(), Units::Millimeters);
        o.full_scale = false;
        o.width_mm = 500.0;
        o.profiles = true;
        o.extensions = true;
        let st = o.strokes(&v);
        // 0.5 mm per pixel: profiles 2 px = 1 mm, sections 3 px, extensions 3 px.
        assert_eq!((st.profile, st.section, st.extension), (Some(1.0), Some(1.5), Some(1.5)));
        o.profile_auto = false;
        o.profile_mm = 0.35;
        o.sections = false;
        let st = o.strokes(&v);
        assert_eq!((st.profile, st.section), (Some(0.35), None));
    }

    #[test]
    fn options_are_remembered() {
        let v = top_view(1000.0, 2.0);
        let base = VecOptions::for_view(&v, &ViewStyle::default(), Units::Meters);
        let mut o = base.clone();
        o.profile_auto = false;
        o.profile_mm = 0.25;
        o.profile_cad = CadWide::Polyline;
        o.acad_version = 8;
        o.line_unit = DrawUnit::Points;
        let back = VecOptions::from_json(&o.to_json(), base.clone());
        assert_eq!(back, o);
        assert_eq!(VecOptions::from_json(&Value::Null, base.clone()), base);
        // AutoCAD 2000 at first, as in SketchUp; the list of the first versions of Esquisse
        // started there.
        assert_eq!(ACAD_VERSIONS[base.acad_version], "AutoCAD 2000");
        assert_eq!(VecOptions::from_json(&json!({"acadVersion": 1}), base.clone()).acad_version, 4);
    }

    #[test]
    fn the_dialog_keeps_the_proportions() {
        let v = top_view(1000.0, 2.0);
        let mut o = VecOptions::for_view(&v, &ViewStyle::default(), Units::Centimeters);
        o.full_scale = false;
        let mut d = VecDialog::new(PathBuf::from("a.pdf"), VecKind::Pdf, o, v);
        d.texts[F_HEIGHT] = "25".into();
        d.typed(F_HEIGHT);
        assert!((d.options.width_mm - 500.0).abs() < 1e-9, "{}", d.options.width_mm);
        assert_eq!(d.texts[F_WIDTH], "50");
    }

    fn boite() -> (esq_core::model::Model, Camera) {
        let mut m = esq_core::model::Model::new();
        let e = &mut m.root;
        e.add_polyline(
            &[DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, 600.0, 0.0), DVec3::new(0.0, 600.0, 0.0)],
            true,
            false,
        );
        let f = e.faces.keys().next().unwrap();
        e.push_pull(f, -400.0, false);
        let mut cam = Camera { size: DVec2::new(800.0, 500.0), ..Default::default() };
        cam.zoom_extents(DVec3::ZERO, DVec3::new(1000.0, 600.0, 400.0));
        (m, cam)
    }

    /// The PDF, read back by a PDF reader, shows the box as on screen: shaded faces drawn
    /// in black, on a white page of the chosen size.
    #[test]
    fn the_pdf_shows_the_view() {
        let (m, cam) = boite();
        let style = ViewStyle::default();
        let info = ViewInfo::of(&cam);
        let mut o = VecOptions::for_view(&info, &style, Units::Millimeters);
        o.full_scale = false;
        o.width_mm = 800.0 * 25.4 / 72.0;
        let dir = std::env::temp_dir().join(format!("esq-vecpdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("boite.pdf");
        export(VecKind::Pdf, &p, &m, &style, &cam, &o, "Boîte").unwrap();
        let img = crate::imgfile::render_pdf(&std::fs::read(&p).unwrap()).unwrap().to_rgb8();
        if let Ok(out) = std::env::var("ESQ_VEC_PNG") {
            img.save(out).unwrap();
        }
        let _ = std::fs::remove_dir_all(dir);
        // 800 x 500 points at 150 dpi.
        assert_eq!((img.width(), img.height()), (1666, 1041));
        let at = |p: DVec3| {
            let s = hlr_view(&cam).screen(p).unwrap() * (img.width() as f64 / 800.0);
            img.get_pixel(s.x as u32, s.y as u32).0
        };
        // Faces: the shaded front colour, never white; around: the white page.
        let top = at(DVec3::new(500.0, 300.0, 400.0));
        assert!(top != [255, 255, 255] && top.iter().all(|c| *c > 120), "{top:?}");
        assert_eq!(img.get_pixel(3, 3).0, [255, 255, 255]);
        // An edge is black.
        let mid = |a: DVec3, b: DVec3| hlr_view(&cam).screen((a + b) / 2.0).unwrap() * (img.width() as f64 / 800.0);
        let e = mid(DVec3::new(0.0, 0.0, 400.0), DVec3::new(1000.0, 0.0, 400.0));
        let dark = (-2..=2)
            .flat_map(|dx| (-2..=2).map(move |dy| (dx, dy)))
            .map(|(dx, dy)| img.get_pixel((e.x as i32 + dx) as u32, (e.y as i32 + dy) as u32).0)
            .any(|c| c.iter().all(|v| *v < 60));
        assert!(dark);
    }

    /// Texts and dimensions are in the drawing, over the model, as on screen.
    #[test]
    fn texts_and_dimensions_are_drawn() {
        use esq_core::model::Annotation;
        let (mut m, cam) = boite();
        let (a, b) = (DVec3::new(0.0, 0.0, 400.0), DVec3::new(1000.0, 0.0, 400.0));
        m.root.annots.insert(Annotation {
            kind: AnnotKind::Linear { a, b, offset: DVec3::new(0.0, -200.0, 0.0) },
            text: String::new(),
            layer: 0,
            hidden: false,
            attach: Default::default(),
        });
        m.root.annots.insert(Annotation {
            kind: AnnotKind::Text { point: DVec3::new(1000.0, 600.0, 400.0), leader: DVec3::new(300.0, 0.0, 200.0) },
            text: "Plan de\ntravail".into(),
            layer: 0,
            hidden: true,
            attach: Default::default(),
        });
        m.root.annots.insert(Annotation {
            kind: AnnotKind::ScreenText { x: 0.5, y: 0.1 },
            text: "Cuisine".into(),
            layer: 0,
            hidden: false,
            attach: Default::default(),
        });
        let style = ViewStyle::default();
        let o = VecOptions::for_view(&ViewInfo::of(&cam), &style, Units::Millimeters);
        let d = drawing(&m, &style, &cam, &o);
        // The hidden text is left out; the dimension shows the length, with two arrowheads.
        let texts: Vec<_> = d.texts.iter().map(|t| t.lines.join("|")).collect();
        assert_eq!(texts.len(), 2, "{texts:?}");
        assert!(texts.contains(&"1000 mm".to_string()) && texts.contains(&"Cuisine".to_string()));
        assert_eq!(d.marks.len(), 2);
        let screen = d.texts.iter().find(|t| t.lines[0] == "Cuisine").unwrap();
        assert_eq!(screen.pos, DVec2::new(400.0, 50.0));
        // In the PDF the label is ink on the page; in the DXF a centred text.
        let dir = std::env::temp_dir().join(format!("esq-vectxt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.pdf");
        export(VecKind::Pdf, &p, &m, &style, &cam, &o, "t").unwrap();
        let img = crate::imgfile::render_pdf(&std::fs::read(&p).unwrap()).unwrap().to_rgb8();
        let s = img.width() as f64 / 800.0;
        let ink = (380..420)
            .flat_map(|x| (40..60).map(move |y| (x, y)))
            .filter(|(x, y)| img.get_pixel((*x as f64 * s) as u32, (*y as f64 * s) as u32).0[0] < 128)
            .count();
        assert!(ink > 10, "{ink}");
        let p = dir.join("t.dxf");
        export(VecKind::Dxf, &p, &m, &style, &cam, &o, "t").unwrap();
        let dxf = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(dxf.matches("\r\nTEXT\r\n").count(), 2);
        assert!(dxf.contains("\r\n  1\r\n1000 mm\r\n"));
        assert_eq!(dxf.matches("\r\nSOLID\r\n").count(), 2);
    }

    #[test]
    fn a_box_is_exported_to_every_type() {
        let (m, cam) = boite();
        let style = ViewStyle::default();
        let o = VecOptions::for_view(&ViewInfo::of(&cam), &style, Units::Millimeters);
        let dir = std::env::temp_dir().join(format!("esq-vec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for k in VecKind::ALL {
            let p = dir.join(format!("boite.{}", k.key()));
            export(k, &p, &m, &style, &cam, &o, "Boîte").unwrap();
            let bytes = std::fs::read(&p).unwrap();
            match k {
                VecKind::Pdf => assert!(bytes.starts_with(b"%PDF-1.4")),
                VecKind::Eps => assert!(bytes.starts_with(b"%!PS-Adobe-3.0 EPSF-3.0")),
                VecKind::Dwg => {
                    let d = cadkit_dwg::read(&bytes, &cadkit_core::ReadOptions::default()).unwrap();
                    assert!(d.warnings.is_empty(), "{:#?}", d.warnings);
                    let lines =
                        d.models[0].entities.iter().filter(|e| matches!(e.kind, cadkit_core::EntityKind::Line { .. }));
                    assert_eq!(lines.count(), 9);
                    assert_eq!(d.units.unit, cadkit_core::LengthUnit::Millimeter);
                }
                VecKind::Dxf => {
                    let t = String::from_utf8(bytes).unwrap();
                    // The 9 visible edges of a box seen from a corner.
                    assert_eq!(t.matches("\r\nLINE\r\n").count(), 9, "{t}");
                }
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The DWG and the DXF are of the version chosen, Version 12 to AutoCAD 2018, with the
    /// same layers and entities in every one (release 12 DWG, which cadkit does not read, is
    /// read back by the tests of `esq_core::dwg`).
    #[test]
    fn dwg_and_dxf_of_every_version() {
        let (m, cam) = boite();
        let style = ViewStyle::default();
        let mut o = VecOptions::for_view(&ViewInfo::of(&cam), &style, Units::Millimeters);
        o.profiles = true;
        o.profile_cad = CadWide::Polyline;
        o.profile_layer = true;
        let dir = std::env::temp_dir().join(format!("esq-vec-versions-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for k in [VecKind::Dwg, VecKind::Dxf] {
            let mut first: Option<String> = None;
            for v in DwgVersion::ALL.iter().rev() {
                o.acad_version = v.code();
                let p = dir.join(format!("boite_{}.{}", v.magic(), k.key()));
                export(k, &p, &m, &style, &cam, &o, "Boîte").unwrap();
                let bytes = std::fs::read(&p).unwrap();
                if k == VecKind::Dwg {
                    assert!(bytes.starts_with(v.magic().as_bytes()), "{v:?}");
                    if *v == DwgVersion::R12 {
                        continue;
                    }
                } else {
                    assert!(String::from_utf8_lossy(&bytes).contains(&format!("$ACADVER\r\n  1\r\n{}\r\n", v.magic())));
                }
                let d = esq_formats::cad::read(&bytes).unwrap();
                assert!(d.warnings.is_empty(), "{v:?}: {:#?}", d.warnings);
                let layers: Vec<_> = d.layers.iter().map(|l| (&l.name, l.color)).collect();
                let ents: Vec<_> = d.models[0].entities.iter().map(|e| format!("{:?} {:?}", e.layer, e.kind)).collect();
                let s = format!("{layers:?} {ents:?}");
                assert!(s.contains("PROFILES") && s.contains("Polyline"), "{s}");
                match &first {
                    None => first = Some(s),
                    Some(f) => assert_eq!(&s, f, "{k:?} {v:?}"),
                }
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
