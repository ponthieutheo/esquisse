//! `host.model.*`: what the document is (file, edit path, undo) as seen by Ruby scripts.
//!
//! While a script runs, the document sits outside the app (`app.doc` is a placeholder), so the
//! facts about it that the model alone does not hold (file path, unsaved changes, the open
//! group/component) live in a `ScriptState` copied from the document before the script and
//! written back afterwards.

use std::path::{Path, PathBuf};

use esq_core::model::{DefId, EntityRef, InstanceId, Model, SceneCamera};
use esq_core::walk::path_def;
use esq_formats::{AllOptions, ExportFormat, ImportFormat};
use serde_json::{Value as J, json};
use slotmap::{Key, KeyData};

use crate::app::EsquisseApp;
use crate::camera::Camera;
use crate::export_vec::{VecKind, VecOptions, ViewInfo};
use crate::export2d::{Kind, RasterOptions};
use crate::style::ViewStyle;

/// Document facts a script can read and change.
#[derive(Default)]
pub struct ScriptState {
    pub path: Option<PathBuf>,
    pub dirty: bool,
    /// Instance path of the group/component being edited.
    pub ctx: Vec<InstanceId>,
    pub ctx_changed: bool,
    /// The script replaced the document (`file_new`, `open_file`, `close`).
    pub replace: Option<Replace>,
    pub saved: bool,
    pub undo: u32,
    pub redo: u32,
    pub quit: bool,
    /// `Model#import` of a file placed with the cursor, or `Model#place_component`: the
    /// component to place once the script is over.
    pub place: Option<DefId>,
    /// `Model#place_component(definition, true)`: every click places one more copy.
    pub place_repeat: bool,
    /// `Sketchup.send_action`: the commands to run once the script is over.
    pub actions: Vec<crate::commands::Cmd>,
}

/// The command of a `Sketchup.send_action` action: its name ("selectLineTool:"…) or its number
/// (`CMD_LINE`…, the ids of SketchUp's commands).
pub fn action_cmd(action: &str) -> Option<crate::commands::Cmd> {
    use crate::commands::Cmd;
    use crate::style::FaceStyle as F;
    use crate::tools::ToolId as T;
    // STD_VIEWS: top, bottom, front, back, left, right, iso.
    let by_id = |n: i64| -> Option<&'static str> {
        Some(match n {
            0x521e => "selectSelectionTool:",
            0x5252 => "selectPaintTool:",
            0x521b => "selectEraseTool:",
            0x5266 => "selectRectangleTool:",
            0x521c => "selectLineTool:",
            0x5268 => "selectCircleTool:",
            0x5249 => "selectArcTool:",
            0x5267 => "selectPolygonTool:",
            0x5227 => "selectFreehandTool:",
            0x5231 => "selectPushPullTool:",
            0x539d => "selectTextTool:",
            0x5238 => "selectMoveTool:",
            0x5289 => "selectRotateTool:",
            0x5415 => "selectExtrudeTool:",
            0x52f4 => "selectScaleTool:",
            0x526c => "selectOffsetTool:",
            0x5220 => "selectMeasureTool:",
            0x5241 => "selectProtractorTool:",
            0x5286 => "selectAxisTool:",
            0x5359 => "selectSectionPlaneTool:",
            0x290c => "selectOrbitTool:",
            0x291b | 0x291d => "selectDollyTool:",
            0x290d => "selectZoomTool:",
            0x291e => "selectZoomWindowTool:",
            0x291f => "viewZoomExtents:",
            0x290e => "renderWireframe:",
            0x290f => "renderHiddenLine:",
            0x2910 => "renderShaded:",
            0x292b => "renderTextures:",
            0x2911 => "viewXRay:",
            0x2918 => "selectWalkTool:",
            0x525b => "makeComponent:",
            0x53a2 => "selectDimensionTool:",
            0x290b => "viewIso:",
            0x2905 => "viewTop:",
            0x2906 => "viewFront:",
            0x2907 => "viewRight:",
            0x2909 => "viewBack:",
            0x2908 => "viewLeft:",
            0x290a => "viewBottom:",
            0x2917 => "viewPerspective:",
            0x52b1 => "selectPositionCameraTool:",
            0xe123 => "editCut:",
            0xe122 => "editCopy:",
            0xe125 => "editPaste:",
            0x521d => "editDelete:",
            0xe107 => "printDocument:",
            0x524b => "pageAdd:",
            0x5256 => "pageDelete:",
            0x524c => "pageUpdate:",
            0x2927 => "pageNext:",
            0x2928 => "pagePrevious:",
            0x53e6 => "showRubyPanel:",
            0x291a => "viewShowAxes:",
            0x52a2 | 0x52a3 => "viewShowHidden:",
            0x52a1 => "viewShowHiddenObjects:",
            0x55dc => "viewShowGuides:",
            0x53dd => "viewZoomToSelection:",
            0x53f6 => "selectFieldOfViewTool:",
            _ => return None,
        })
    };
    let name = match action.trim().parse::<i64>() {
        Ok(n) => by_id(n)?,
        Err(_) => action.trim(),
    };
    Some(match name {
        "selectSelectionTool:" => Cmd::Tool(T::Select),
        "selectEraseTool:" => Cmd::Tool(T::Eraser),
        "selectPaintTool:" => Cmd::Tool(T::Paint),
        "selectLineTool:" => Cmd::Tool(T::Line),
        "selectFreehandTool:" => Cmd::Tool(T::Freehand),
        "selectRectangleTool:" => Cmd::Tool(T::Rect),
        "selectCircleTool:" => Cmd::Tool(T::Circle),
        "selectArcTool:" => Cmd::Tool(T::Arc),
        "selectPolygonTool:" => Cmd::Tool(T::Polygon),
        "selectMoveTool:" => Cmd::Tool(T::Move),
        "selectRotateTool:" => Cmd::Tool(T::Rotate),
        "selectScaleTool:" => Cmd::Tool(T::Scale),
        "selectPushPullTool:" => Cmd::Tool(T::PushPull),
        "selectOffsetTool:" => Cmd::Tool(T::Offset),
        "selectExtrudeTool:" | "selectFollowMeTool:" => Cmd::Tool(T::FollowMe),
        "selectMeasureTool:" | "selectTapeMeasureTool:" => Cmd::Tool(T::Tape),
        "selectProtractorTool:" => Cmd::Tool(T::Protractor),
        "selectAxisTool:" => Cmd::Tool(T::AxesTool),
        "selectTextTool:" => Cmd::Tool(T::Text),
        "selectDimensionTool:" => Cmd::Tool(T::Dimension),
        "selectSectionPlaneTool:" => Cmd::Tool(T::SectionPlane),
        "selectNorthTool:" => Cmd::Tool(T::North),
        "selectOrbitTool:" => Cmd::Tool(T::Orbit),
        "selectDollyTool:" | "selectPanTool:" => Cmd::Tool(T::Pan),
        "selectZoomTool:" => Cmd::Tool(T::Zoom),
        "selectZoomWindowTool:" => Cmd::Tool(T::ZoomWindow),
        "selectFieldOfViewTool:" => Cmd::Tool(T::Fov),
        "selectPositionCameraTool:" => Cmd::Tool(T::PositionCamera),
        "selectTurnTool:" => Cmd::Tool(T::LookAround),
        "selectWalkTool:" => Cmd::Tool(T::Walk),
        "viewTop:" => Cmd::StdView(0),
        "viewBottom:" => Cmd::StdView(1),
        "viewFront:" => Cmd::StdView(2),
        "viewBack:" => Cmd::StdView(3),
        "viewLeft:" => Cmd::StdView(4),
        "viewRight:" => Cmd::StdView(5),
        "viewIso:" => Cmd::StdView(6),
        "viewPerspective:" => Cmd::Perspective,
        "viewZoomExtents:" => Cmd::ZoomExtents,
        "viewZoomToSelection:" | "selectionZoomExt:" => Cmd::ZoomSelection,
        "viewUndo:" => Cmd::CamPrevious,
        "viewShowAxes:" => Cmd::Axes,
        "viewShowHidden:" => Cmd::HiddenGeometry,
        "viewShowHiddenObjects:" => Cmd::HiddenObjects,
        "viewShowGuides:" => Cmd::Guides,
        "viewShadows:" => Cmd::Shadows,
        "viewXRay:" => Cmd::Xray,
        "renderWireframe:" => Cmd::Faces(F::Wireframe),
        "renderHiddenLine:" => Cmd::Faces(F::HiddenLine),
        "renderShaded:" => Cmd::Faces(F::Shaded),
        "renderTextures:" => Cmd::Faces(F::ShadedWithTextures),
        "renderMonochrome:" => Cmd::Faces(F::Monochrome),
        "pageAdd:" => Cmd::AddScene,
        "pageDelete:" => Cmd::DeleteScene,
        "pageUpdate:" => Cmd::UpdateScene,
        "pageNext:" => Cmd::NextScene,
        "pagePrevious:" => Cmd::PrevScene,
        "editCut:" => Cmd::Cut,
        "editCopy:" => Cmd::Copy,
        "editPaste:" => Cmd::Paste,
        "editDelete:" => Cmd::Delete,
        "editHide:" => Cmd::Hide,
        "editUnhide:" => Cmd::RevealSelection,
        "editInvertSelection:" => Cmd::InvertSelection,
        "makeComponent:" => Cmd::MakeComponent,
        "printDocument:" | "filePrint:" => Cmd::Print,
        "showRubyPanel:" => Cmd::RubyConsole,
        "fileOpen:" | "openDocument:" => Cmd::Open,
        "fileSaveAs:" => Cmd::SaveAs,
        "fileSaveCopy:" => Cmd::SaveCopyAs,
        "fileSaveAsTemplate:" => Cmd::SaveAsTemplate,
        "fileRevert:" => Cmd::Revert,
        "fileImport:" => Cmd::Import,
        _ => return None,
    })
}

/// `Model#place_component` (SketchUp): the Component tool takes the
/// definition once the script is over. `false` when there is no such definition (Ruby raises
/// "Invalid component - cannot place").
pub fn place_component(st: &mut ScriptState, model: &Model, a: &J) -> Result<J, String> {
    let id = match &a["definition"] {
        J::String(s) => s.parse::<u64>().ok(),
        v => v.as_u64(),
    };
    let def = id.map(|i| DefId::from(KeyData::from_ffi(i))).filter(|d| model.defs.contains_key(*d));
    let Some(d) = def else { return Ok(json!(false)) };
    st.place = Some(d);
    st.place_repeat = a["repeat"].as_bool().unwrap_or(false);
    Ok(json!(true))
}

pub enum Replace {
    New,
    Open(PathBuf),
}

fn handle(ctx: Option<DefId>, id: InstanceId) -> String {
    format!("i:{}:{}", ctx.map_or("0".into(), |d| d.data().as_ffi().to_string()), id.data().as_ffi())
}

fn parse_inst(model: &Model, h: &str) -> Result<(Option<DefId>, InstanceId), String> {
    let mut it = h.splitn(3, ':');
    let (k, c, i) = (it.next(), it.next(), it.next());
    let (Some("i"), Some(c), Some(i)) = (k, c, i) else { return Err(format!("poignée d'instance invalide: {h}")) };
    let ctx = if c == "0" {
        None
    } else {
        Some(DefId::from(KeyData::from_ffi(c.parse::<u64>().map_err(|_| "poignée invalide")?)))
    };
    let id = InstanceId::from(KeyData::from_ffi(i.parse::<u64>().map_err(|_| "poignée invalide")?));
    if !model.entities(ctx).instances.contains_key(id) {
        return Err("instance supprimée".into());
    }
    Ok((ctx, id))
}

fn ext_of(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase()
}

/// What these operations need from the application (a test stands in for it).
pub trait ModelApp {
    /// The point of view saved with a `.skp`.
    fn view(&self) -> Option<esq_skp::View>;
    /// The camera and style the 2D graphics are drawn with (`None` without a view).
    fn view2d(&self) -> Option<(Camera, ViewStyle)>;
    /// The options of File › Export › 2D Graphic as last used (`export2d.json`).
    fn prefs2d(&mut self) -> J;
    /// Keeps one of them for the next time.
    fn set_pref2d(&mut self, key: &str, value: J);
    /// File › Export › 2D Graphic as an image: `model` seen by the current camera, written at
    /// once (`ctx` and `sel`: the script's edit path and selection).
    fn image(
        &mut self,
        model: &mut Model,
        ctx: &[InstanceId],
        sel: &[EntityRef],
        path: &Path,
        kind: Kind,
        o: &RasterOptions,
    ) -> Result<(), String>;
    /// A message in the status bar.
    fn say(&mut self, text: &str);
    /// The options of File › Import and Export as last used (`formats3d.json`).
    fn formats(&mut self) -> AllOptions;
    /// Keeps them for the next time.
    fn set_formats(&mut self, o: AllOptions);
    /// The "Résultats de l'importation / de l'exportation" window.
    fn results(&mut self, title: &str, text: String);
    /// The preview a `.skp` keeps (PNG): `model` seen by the current camera (`None` without a
    /// GPU).
    fn thumbnail(&mut self, _model: &mut Model) -> Option<Vec<u8>> {
        None
    }
}

impl ModelApp for EsquisseApp {
    fn view(&self) -> Option<esq_skp::View> {
        Some(self.cam.to_view())
    }

    fn view2d(&self) -> Option<(Camera, ViewStyle)> {
        Some((self.cam.clone(), self.style.clone()))
    }

    fn prefs2d(&mut self) -> J {
        crate::export2d::read_prefs()
    }

    fn set_pref2d(&mut self, key: &str, value: J) {
        crate::export2d::write_pref(key, value);
    }

    fn image(
        &mut self,
        model: &mut Model,
        ctx: &[InstanceId],
        sel: &[EntityRef],
        path: &Path,
        kind: Kind,
        o: &RasterOptions,
    ) -> Result<(), String> {
        self.export_script_image(model, ctx, sel, path, kind, o).map_err(|e| format!("{e:#}"))
    }

    fn say(&mut self, text: &str) {
        // With a Ruby tool active, Sketchup.set_status_text is its status bar prompt.
        if !crate::ruby_tools::status(self, 0, text) {
            self.message = Some((text.to_string(), 0.0));
        }
    }

    fn formats(&mut self) -> AllOptions {
        self.ui_state.extra.formats.opts().clone()
    }

    fn set_formats(&mut self, o: AllOptions) {
        self.ui_state.extra.formats.set_opts(o);
    }

    fn results(&mut self, title: &str, text: String) {
        self.ui_state.extra.formats.results = Some((title.to_string(), text));
    }

    fn thumbnail(&mut self, model: &mut Model) -> Option<Vec<u8>> {
        self.skp_thumbnail(Some(model))
    }
}

/// The SketchUp version of `Model#save(path, version)` (`None`: the current format).
fn version_arg(v: &J) -> Result<Option<esq_skp::SkpVersion>, String> {
    match v {
        J::Null => Ok(None),
        v => v
            .as_i64()
            .and_then(esq_skp::SkpVersion::from_ruby)
            .map(Some)
            .ok_or_else(|| format!("version inconnue : {v}")),
    }
}

/// Writes the model to `path` in the format its extension names (a `.skp` of `version`).
fn write_model(
    app: &mut dyn ModelApp,
    model: &mut Model,
    path: &Path,
    version: Option<esq_skp::SkpVersion>,
) -> Result<(), String> {
    let thumbnail = if ext_of(path) == "skp" { app.thumbnail(model) } else { None };
    // The view's rendering options go with the document, as File › Save writes them.
    let styled;
    let model: &Model = match app.view2d() {
        Some((_, style)) => {
            let mut m = model.clone();
            crate::styles::write_style(&mut m, &style);
            styled = m;
            &styled
        }
        None => model,
    };
    match ext_of(path).as_str() {
        "skp" => esq_skp::export_skp_with(model, path, app.view().as_ref(), version, thumbnail.as_deref())
            .map_err(|e| format!("{e:#}")),
        "obj" => esq_core::io::export_obj(model, path).map_err(|e| format!("{e:#}")),
        "stl" => esq_core::io::export_stl(model, path).map_err(|e| format!("{e:#}")),
        "esq" => esq_core::io::save_esq(model, path).map_err(|e| format!("{e:#}")),
        e => Err(format!("format non pris en charge: .{e}")),
    }
}

/// The hash of options a script gave (Symbol keys only, see `formats.rs`), if any.
fn options_of(a: &J) -> serde_json::Map<String, J> {
    a["options"].as_object().cloned().unwrap_or_default()
}

/// `Model#export` (SketchUp): the exporter of the file's
/// extension writes the model with its saved options, the script's hash laid over a copy of
/// them. Returns whether the file was written.
fn export(app: &mut dyn ModelApp, st: &ScriptState, model: &mut Model, sel: &[EntityRef], a: &J) -> Result<J, String> {
    let p = PathBuf::from(a["path"].as_str().ok_or("chemin manquant")?);
    let show = a["show"].as_bool().unwrap_or(false);
    let h = options_of(a);
    let ext = ext_of(&p);
    if let Some(kind) = raster_kind(&ext) {
        return Ok(json!({"ok": export_image(app, st, model, sel, &p, kind, &h).is_ok()}));
    }
    let ok = match ext.as_str() {
        "skp" | "esq" => write_model(app, model, &p, None).is_ok(),
        "pdf" => export_vector(app, st, model, &p, VecKind::Pdf, &h).is_ok(),
        "eps" => export_vector(app, st, model, &p, VecKind::Eps, &h).is_ok(),
        _ => match ExportFormat::from_path(&p) {
            Some(fmt) => {
                let mut o = app.formats();
                let req = super::formats::apply_export(&mut o, fmt, &h);
                // SketchUp writes what a script gives the STL exporter to its preferences
                // ("STL Exporter"): the next export starts from it.
                if fmt == ExportFormat::Stl && !h.is_empty() {
                    app.set_formats(o.clone());
                }
                let view = app.view().map(|v| SceneCamera {
                    eye: v.eye,
                    target: v.target,
                    up: v.up,
                    fov_deg: v.fov_deg,
                    perspective: v.perspective,
                });
                let part = if req.selection_only { sel } else { &[] };
                match req.acad_type {
                    0 => match crate::formats3d::export_model(model, &st.ctx, part, view, &o, &p, fmt) {
                        Ok(report) => {
                            if show && let Some(t) = report {
                                app.results("Résultats de l’exportation", t);
                            }
                            true
                        }
                        Err(_) => false,
                    },
                    // `:type => "2d"`: the hidden-line drawing of the view.
                    1 => {
                        let kind = if fmt == ExportFormat::Dwg { VecKind::Dwg } else { VecKind::Dxf };
                        export_vector(app, st, model, &p, kind, &h).is_ok()
                    }
                    // `"section"`: the slice of the active section plane.
                    _ => export_slice(app, st, model, &p, &h).is_ok(),
                }
            }
            None => false,
        },
    };
    Ok(json!({"ok": ok}))
}

/// The section slice of the active plane of the edited collection (File › Export › Section
/// Slice): its saved options, the script's keys laid over a copy of them.
fn export_slice(
    app: &mut dyn ModelApp,
    st: &ScriptState,
    model: &Model,
    p: &Path,
    h: &serde_json::Map<String, J>,
) -> anyhow::Result<()> {
    use crate::export_section as xs;
    let Some((cam, style)) = app.view2d() else { anyhow::bail!("pas de vue") };
    let ctx = path_def(model, &st.ctx);
    let ents = model.entities(ctx);
    let Some(sec) = ents.active_section.filter(|s| ents.sections.contains_key(*s)) else {
        anyhow::bail!("aucun plan de section actif")
    };
    let mut o = xs::from_json(app.prefs2d().get("slice").unwrap_or(&J::Null), model.units);
    let e = xs::slice_drawing(model, ctx, sec, o.projection, &cam).map_err(anyhow::Error::msg)?.extent();
    super::formats::apply_slice(&mut o, h, e.x / e.y.max(1e-9));
    xs::export_file(model, ctx, sec, &o, &cam, &style, p)
}

/// The image types of File › Export › 2D Graphic, by extension.
fn raster_kind(ext: &str) -> Option<Kind> {
    Some(match ext {
        "jpg" | "jpeg" => Kind::Jpeg,
        "png" => Kind::Png,
        "tif" | "tiff" => Kind::Tiff,
        "tga" => Kind::Tga,
        "bmp" => Kind::Bmp,
        _ => return None,
    })
}

/// An image of the view: the saved options of the image export, `:width` / `:height` laid
/// over a copy of them.
fn export_image(
    app: &mut dyn ModelApp,
    st: &ScriptState,
    model: &mut Model,
    sel: &[EntityRef],
    p: &Path,
    kind: Kind,
    h: &serde_json::Map<String, J>,
) -> Result<(), String> {
    let (cam, _) = app.view2d().ok_or("pas de vue")?;
    let mut o = RasterOptions::from_json(app.prefs2d().get("raster").unwrap_or(&J::Null));
    let view = [cam.size.x.round().max(1.0) as u32, cam.size.y.round().max(1.0) as u32];
    super::formats::apply_raster(&mut o, h, view);
    app.image(model, &st.ctx, sel, p, kind, &o)
}

/// A hidden-line drawing of the view (PDF, EPS, AutoCAD 2D) with the saved options of its
/// type, completed by the view's defaults as the dialog does (`export_vec::open`), and the
/// script's keys: those for PDF (kept afterwards, as SketchUp keeps them in
/// the exporter's settings), the generic table for AutoCAD; SketchUp reads none for EPS.
fn export_vector(
    app: &mut dyn ModelApp,
    st: &ScriptState,
    model: &Model,
    p: &Path,
    kind: VecKind,
    h: &serde_json::Map<String, J>,
) -> anyhow::Result<()> {
    let Some((cam, style)) = app.view2d() else { anyhow::bail!("pas de vue") };
    let info = ViewInfo::of(&cam);
    let base = VecOptions::for_view(&info, &style, model.units);
    let mut o = match app.prefs2d().get(kind.options_key()) {
        Some(v) => VecOptions::from_json(v, base),
        None => base,
    };
    let aspect = info.px.x / info.px.y.max(1.0);
    match kind {
        VecKind::Pdf => {
            super::formats::apply_pdf(&mut o, h, aspect);
            if !h.is_empty() {
                app.set_pref2d(kind.options_key(), o.to_json());
            }
        }
        VecKind::Dwg | VecKind::Dxf => super::formats::apply_cad2d(&mut o, h, aspect),
        VecKind::Eps => {}
    }
    let name = st.path.as_ref().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned());
    let title = format!("{} — Esquisse", name.unwrap_or_else(|| "Sans titre".into()));
    crate::export_vec::export(kind, p, model, &style, &cam, &o, &title)
}

/// `Model#import` (SketchUp): the importer of the
/// file's extension reads it with its saved options and the script's hash; the result is a
/// component placed as File › Import places it (at the origin, or held by the cursor).
fn import(app: &mut dyn ModelApp, st: &mut ScriptState, model: &mut Model, a: &J) -> Result<J, String> {
    let p = PathBuf::from(a["path"].as_str().ok_or("chemin manquant")?);
    let show = a["show"].as_bool().unwrap_or(false);
    let h = options_of(a);
    let mut o = app.formats();
    if let Some(fmt) = ImportFormat::from_path(&p)
        && !h.is_empty()
    {
        super::formats::apply_import(&mut o, fmt, &h, model.units);
        // As for the STL exporter ("STL Importer" preferences).
        if fmt == ImportFormat::Stl {
            app.set_formats(o.clone());
        }
    }
    let Ok(l) = crate::formats3d::load_import(&p, &o, model.units) else { return Ok(json!({"ok": false})) };
    let ctx = path_def(model, &st.ctx);
    if let crate::formats3d::Placement::WithCursor(d) = crate::formats3d::insert_import(model, ctx, &l) {
        st.place = Some(d);
    }
    if show && let Some(t) = l.report {
        app.results("Résultats de l'importation", t);
    }
    Ok(json!({"ok": true}))
}

/// `DefinitionList#import`: the file through the importer of its extension, with the
/// script's options over the saved ones (as `Model#import`), as a new definition that nothing
/// places. `{"ctx": …}`, or `{"err": "none"}` (no importer) / `{"err": "fail"}`.
fn import_def(app: &mut dyn ModelApp, model: &mut Model, a: &J) -> Result<J, String> {
    let p = PathBuf::from(a["path"].as_str().ok_or("chemin manquant")?);
    let Some(fmt) = ImportFormat::from_path(&p) else { return Ok(json!({"err": "none"})) };
    let mut o = app.formats();
    let h = options_of(a);
    if !h.is_empty() {
        super::formats::apply_import(&mut o, fmt, &h, model.units);
    }
    match crate::formats3d::load_import(&p, &o, model.units) {
        Ok(l) => {
            let d = model.import_as_component(&l.model, &l.name);
            Ok(json!({"ctx": d.data().as_ffi().to_string()}))
        }
        Err(_) => Ok(json!({"err": "fail"})),
    }
}

pub fn call(
    app: &mut dyn ModelApp,
    st: &mut ScriptState,
    model: &mut Model,
    sel: &mut Vec<EntityRef>,
    op: &str,
    args: &J,
) -> Result<J, String> {
    match op.strip_prefix("model.").unwrap_or(op) {
        // `Materials#load` and `Material#save_as`: SketchUp's .skm files (`skm.rs`); null when
        // the file is not one (the script side then tries Esquisse's older format).
        "read_skm" => {
            let path = std::path::Path::new(args["path"].as_str().ok_or("chemin attendu")?);
            Ok(crate::skm::read_file(path).ok().and_then(|m| serde_json::to_value(m).ok()).unwrap_or(J::Null))
        }
        "write_skm" => {
            let path = args["path"].as_str().ok_or("chemin attendu")?;
            let m: esq_core::model::Material =
                serde_json::from_value(args["material"].clone()).map_err(|e| format!("matière : {e}"))?;
            Ok(json!(std::fs::write(path, crate::skm::write(&m, None)).is_ok()))
        }
        "path" => {
            let mut path = Vec::new();
            let mut ctx: Option<DefId> = None;
            for &i in &st.ctx {
                if model.entities(ctx).instances.contains_key(i) {
                    path.push(handle(ctx, i));
                    ctx = model.entities(ctx).instances.get(i).map(|x| x.def);
                }
            }
            let cur = path_def(model, &st.ctx);
            Ok(json!({"path": path, "ctx": cur.map_or("0".into(), |d| d.data().as_ffi().to_string())}))
        }
        "set_path" => {
            let mut v = Vec::new();
            for h in args["path"].as_array().ok_or("chemin attendu")? {
                let (_, id) = parse_inst(model, h.as_str().unwrap_or(""))?;
                v.push(id);
            }
            st.ctx = v;
            st.ctx_changed = true;
            Ok(J::Null)
        }
        "info" => Ok(json!({
            "path": st.path.as_ref().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default(),
            "dirty": st.dirty || (!st.saved && crate::doc::script_changed(model).unwrap_or(false)),
        })),
        "save" => {
            let mut p = PathBuf::from(args["path"].as_str().ok_or("chemin manquant")?);
            if p.extension().is_none() {
                p.set_extension("skp");
            }
            let r = version_arg(&args["version"]).and_then(|v| write_model(app, model, &p, v));
            if r.is_ok() && matches!(ext_of(&p).as_str(), "skp" | "esq") {
                st.path = Some(p);
                st.dirty = false;
                st.saved = true;
            }
            Ok(json!({"ok": r.is_ok(), "error": r.err()}))
        }
        "save_copy" => {
            let mut p = PathBuf::from(args["path"].as_str().ok_or("chemin manquant")?);
            if p.extension().is_none() {
                p.set_extension("skp");
            }
            let r = version_arg(&args["version"]).and_then(|v| write_model(app, model, &p, v));
            Ok(json!({"ok": r.is_ok(), "error": r.err()}))
        }
        "export" => export(app, st, model, sel, args),
        "import" => import(app, st, model, args),
        "import_def" => import_def(app, model, args),
        "open" => {
            use esq_skp::native::container::{Newer, newer};
            let p = PathBuf::from(args["path"].as_str().ok_or("chemin manquant")?);
            // A file of a newer SketchUp: opened after a warning when it can be read
            // (LOAD_STATUS_SUCCESS_MORE_RECENT), not at all otherwise.
            let level = if ext_of(&p) == "skp" { newer(&std::fs::read(&p).unwrap_or_default()) } else { Newer::No };
            if level == Newer::Hard {
                return Ok(json!({"status": -1}));
            }
            let loaded = match ext_of(&p).as_str() {
                "skp" => esq_skp::import_skp_view(&p).map(|(m, _)| m),
                _ => esq_core::io::load_esq(&p),
            };
            match loaded {
                Ok(m) => {
                    *model = m;
                    sel.clear();
                    st.path = Some(p.clone());
                    st.dirty = false;
                    st.ctx.clear();
                    st.replace = Some(Replace::Open(p));
                    Ok(json!({"status": if level == Newer::Soft { 1 } else { 0 }}))
                }
                Err(_) => Ok(json!({"status": -1})),
            }
        }
        "new" | "close" => {
            *model = crate::prefs::default_template().model();
            sel.clear();
            st.path = None;
            st.dirty = false;
            st.ctx.clear();
            st.replace = Some(Replace::New);
            Ok(J::Null)
        }
        "undo" => {
            st.undo += 1;
            Ok(json!(true))
        }
        "redo" => {
            st.redo += 1;
            Ok(json!(true))
        }
        "quit" => {
            st.quit = true;
            Ok(J::Null)
        }
        // "Ctrl+S", "Fichier/Enregistrer" for each command that has a shortcut.
        "shortcuts" => Ok(json!(
            crate::menus::command_paths()
                .into_iter()
                .filter(|(_, c)| c.shortcut().is_some())
                .map(|(p, c)| json!([c.shortcut_text(), p.replace(" › ", "/")]))
                .collect::<Vec<_>>()
        )),
        "status" => {
            app.say(args["text"].as_str().unwrap_or(""));
            Ok(J::Null)
        }
        // Sketchup.send_action: true for an action Esquisse has, run after the script.
        "action" => Ok(json!(match args["action"].as_str().and_then(action_cmd) {
            Some(c) => {
                st.actions.push(c);
                true
            }
            None => false,
        })),
        "thumbnail" => Ok(json!(false)),
        // Sketchup.template / template_dir / template= : File › New's template (prefs.rs).
        "template" => {
            let t = crate::prefs::default_template();
            let s = |p: Option<PathBuf>| p.map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            Ok(json!({"path": s(crate::prefs::template_file(&t)), "dir": s(crate::prefs::builtin_dir())}))
        }
        "set_template" => Ok(json!(crate::prefs::set_template_path(args["path"].as_str().unwrap_or("")))),
        // Sketchup.break_edges= : saved with the preferences (Preferences/IntersectEdges).
        "intersect_edges" => {
            let on = args["on"].as_bool().unwrap_or(true);
            esq_core::ops::set_break_edges(on);
            crate::settings::update(|s| s.intersect_edges = on);
            Ok(J::Null)
        }
        _ => Err(format!("opération hôte non implémentée: {op}")),
    }
}
