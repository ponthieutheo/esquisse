//! File › Import and the File › Export dialogs as SketchUp shows them (`CImportDialog`,
//! `CExportHLRDialog`, `CExportModelDialog`, `CExportMovieDialog`, `CExportSectionDialog`):
//! one file dialog with the list of types, an "Options..." button that opens the options of
//! the chosen type over the dialog while it stays open (OK keeps them), OK renamed "Exporter"
//! or "Importer", and for pictures the "Utiliser image comme :" group. The export or the
//! import runs when the dialog closes, with the options kept.
//!
//! The file dialog runs on its own thread (`filedlg::spawn`): the application keeps drawing,
//! and the options window is a second native window (an egui viewport) given to the dialog,
//! which is disabled meanwhile, as under SketchUp's modal options dialogs. Windows only:
//! elsewhere each module keeps its own windows around `rfd`.

use eframe::egui;

use crate::app::EsquisseApp;
use crate::export_anim::AnimOptions;
use crate::export_section::SliceDialog;
use crate::export_vec::VecDialog;
use crate::export2d::Export2dDialog;
use crate::filedlg::{Choice, Event, Pending};
use crate::formats3d::OptionsDlg;

/// What a file dialog is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Purpose {
    /// File › Export › 2D Graphic: the view's size in pixels and the display scale when the
    /// dialog opened.
    Export2d { view: [u32; 2], ppp: f32 },
    /// File › Export › 3D Model.
    Export3d,
    /// File › Import.
    Import,
    /// File › Export › Animation.
    Anim,
    /// File › Export › Section Slice.
    Slice,
}

/// The options window of a type, shown over the dialog.
pub enum OptionsWin {
    /// JPEG, PNG, TIFF, Targa, BMP.
    Raster(Export2dDialog),
    /// PDF, EPS, DWG, DXF.
    Vector(VecDialog),
    /// A 3D format (import or export).
    Format(OptionsDlg),
    Anim(AnimOptions),
    /// DWG, DXF, SVG section slice (`COptionsDlgSection`).
    Slice(SliceDialog),
}

impl OptionsWin {
    pub fn title(&mut self) -> String {
        match self {
            OptionsWin::Raster(_) => "Options d'exportation".into(),
            OptionsWin::Vector(d) => d.title().into(),
            OptionsWin::Format(d) => d.title(),
            OptionsWin::Anim(_) => "Options d'exportation d'animation".into(),
            OptionsWin::Slice(_) => crate::export_section::TITLE.into(),
        }
    }

    /// The window's size before its content is measured, points.
    fn first_size(&self) -> egui::Vec2 {
        match self {
            OptionsWin::Vector(_) => egui::vec2(560.0, 520.0),
            OptionsWin::Slice(_) => egui::vec2(460.0, 520.0),
            _ => egui::vec2(380.0, 260.0),
        }
    }

    /// Draws the window: its controls, then "Paramètres par défaut" (vector drawings and
    /// section slices), OK and Annuler. `Some(true)` for OK, `Some(false)` for Annuler.
    pub fn ui(&mut self, app: &EsquisseApp, ui: &mut egui::Ui) -> Option<bool> {
        match self {
            OptionsWin::Raster(d) => crate::export2d::options_ui(ui, d),
            OptionsWin::Vector(d) => crate::export_vec::options_ui(ui, d),
            OptionsWin::Format(d) => {
                d.body(ui);
                ui.separator();
            }
            OptionsWin::Anim(o) => crate::export_anim::options_ui(ui, app, o),
            OptionsWin::Slice(d) => crate::export_section::options_ui(ui, d),
        }
        if matches!(self, OptionsWin::Raster(_)) {
            ui.separator();
        }
        let mut out = None;
        ui.horizontal(|ui| {
            match self {
                OptionsWin::Vector(d) if ui.button("Paramètres par défaut").clicked() => d.reset(app),
                OptionsWin::Slice(d) if ui.button("Paramètres par défaut").clicked() => d.reset(),
                _ => {}
            }
            if ui.button("OK").clicked() {
                out = Some(true);
            }
            if ui.button("Annuler").clicked() {
                out = Some(false);
            }
        });
        out
    }

    /// OK: the options are kept for the export or import, and the next time.
    pub fn keep(self, app: &mut EsquisseApp) {
        match self {
            OptionsWin::Raster(d) => crate::export2d::save_options(&d.options),
            OptionsWin::Vector(d) => crate::export_vec::save_options(d.kind, &d.options),
            OptionsWin::Format(d) => app.ui_state.extra.formats.set_opts(d.draft),
            OptionsWin::Anim(o) => app.ui_state.extra.files.anim = o,
            OptionsWin::Slice(d) => crate::export_section::keep(&d),
        }
    }
}

/// A file dialog open, and the options window it shows.
pub struct Flow {
    pub purpose: Purpose,
    pending: Pending,
    pub options: Option<OptionsWin>,
    /// Size of the options window measured at its last frame, points.
    size: Option<egui::Vec2>,
    /// The options window is given to the dialog, centred over it, and shown.
    shown: bool,
    /// Options were opened from this dialog.
    options_used: bool,
}

#[derive(Default)]
pub struct State {
    pub flow: Option<Flow>,
    /// The options of File › Export › Animation, kept for the session.
    pub anim: AnimOptions,
}

/// The dialog of `purpose`.
fn request(app: &mut EsquisseApp, purpose: Purpose) -> crate::filedlg::Request {
    match purpose {
        Purpose::Export2d { .. } => crate::export2d::request(app),
        Purpose::Export3d => crate::formats3d::export_request(app),
        Purpose::Import => crate::formats3d::import_request(app),
        Purpose::Anim => crate::export_anim::request(),
        Purpose::Slice => crate::export_section::request(app),
    }
}

/// Opens the dialog of `purpose` on its own thread. False where there is no such dialog
/// (the caller shows its own windows then); true too while another one is open.
pub fn open(app: &mut EsquisseApp, ctx: &egui::Context, purpose: Purpose) -> bool {
    if !cfg!(windows) {
        return false;
    }
    if app.ui_state.extra.files.flow.is_some() {
        return true;
    }
    let req = request(app, purpose);
    let Some(pending) = crate::filedlg::spawn(req, ctx) else { return false };
    start(app, purpose, pending);
    true
}

/// Follows a dialog already open (`filedlg::spawn`, or a test's).
pub fn start(app: &mut EsquisseApp, purpose: Purpose, pending: Pending) {
    app.ui_state.extra.files.flow =
        Some(Flow { purpose, pending, options: None, size: None, shown: false, options_used: false });
}

/// The options window of type `i` of the dialog, `name` being the file selected or typed
/// (none for a type without options).
pub fn options_window(app: &mut EsquisseApp, purpose: Purpose, i: usize, name: &str) -> Option<OptionsWin> {
    Some(match purpose {
        Purpose::Export2d { view, ppp } => match crate::export2d::kind_at(i) {
            Ok(kind) => OptionsWin::Raster(Export2dDialog::new(
                Default::default(),
                kind,
                crate::export2d::load_prefs(),
                view,
                ppp as f64,
            )),
            Err(vk) => OptionsWin::Vector(crate::export_vec::dialog(app, Default::default(), vk)),
        },
        Purpose::Export3d => OptionsWin::Format(crate::formats3d::export_options(app, i)?),
        Purpose::Import => OptionsWin::Format(crate::formats3d::import_options(app, i, name)?),
        Purpose::Anim => OptionsWin::Anim(app.ui_state.extra.files.anim.clone()),
        Purpose::Slice => OptionsWin::Slice(crate::export_section::dialog(app)?),
    })
}

/// The dialog closed on OK: the export or import of the file chosen. `options_used`: the
/// options were opened from the dialog.
pub fn finish(app: &mut EsquisseApp, ctx: &egui::Context, purpose: Purpose, c: Choice, options_used: bool) {
    match purpose {
        Purpose::Export2d { view, ppp } => crate::export2d::export_chosen(app, ctx, c.path, c.type_index, view, ppp),
        Purpose::Export3d => {
            let f = crate::formats3d::format_at(c.type_index);
            crate::formats3d::export_chosen(app, ctx, &c.path, f);
        }
        Purpose::Import => {
            let use_as = c.image_use.map(crate::import2d::ImageUse::of_index);
            crate::formats3d::import_chosen(app, ctx, &c.path, c.type_index, use_as);
        }
        Purpose::Anim => {
            let o = app.ui_state.extra.files.anim.clone();
            if let Some(msg) = crate::export_anim::start_job(app, &o, c.path) {
                app.say(ctx, msg);
            }
        }
        Purpose::Slice => crate::export_section::export_chosen(app, ctx, c.path, options_used),
    }
}

const OPTIONS_VIEWPORT: &str = "esquisse-file-options";

/// Called every frame: the dialog's messages, and its options window.
pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(mut flow) = app.ui_state.extra.files.flow.take() else { return };
    while let Some(e) = flow.pending.try_recv() {
        match e {
            Event::Options { type_index, name } => {
                flow.options = options_window(app, flow.purpose, type_index, &name);
                flow.size = None;
                flow.shown = false;
                flow.options_used |= flow.options.is_some();
                if flow.options.is_none() {
                    flow.pending.options_closed();
                }
            }
            Event::Done(choice) => {
                if let Some(c) = choice {
                    finish(app, ctx, flow.purpose, c, flow.options_used);
                }
                return;
            }
        }
    }
    if let Some(mut win) = flow.options.take() {
        match options_viewport(app, ctx, &mut flow, &mut win) {
            Some(ok) => {
                if ok {
                    win.keep(app);
                }
                flow.pending.options_closed();
            }
            None => flow.options = Some(win),
        }
    }
    app.ui_state.extra.files.flow = Some(flow);
}

/// The options window, a native window of its own over the file dialog (inside the main
/// window where egui cannot open another one, as in tests).
fn options_viewport(app: &EsquisseApp, ctx: &egui::Context, flow: &mut Flow, win: &mut OptionsWin) -> Option<bool> {
    let title = win.title();
    let builder = egui::ViewportBuilder::default()
        .with_title(title.clone())
        .with_inner_size(flow.size.unwrap_or_else(|| win.first_size()))
        .with_resizable(false)
        .with_minimize_button(false)
        .with_maximize_button(false)
        .with_taskbar(false)
        .with_visible(flow.shown);
    let (out, measured, native) =
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(OPTIONS_VIEWPORT), builder, |ui, class| {
            let native = class != egui::ViewportClass::EmbeddedWindow;
            let mut out = None;
            let margin = 12.0;
            let frame = egui::Frame::central_panel(ui.style()).inner_margin(margin);
            // What the controls use (the panel itself fills the window, however large).
            let used = egui::CentralPanel::default()
                .frame(frame)
                .show(ui, |ui| ui.vertical(|ui| out = win.ui(app, ui)).response.rect)
                .inner;
            if native && ui.ctx().input(|i| i.viewport().close_requested()) {
                out = Some(false);
            }
            (out, used.max.to_vec2() + egui::vec2(margin, margin), native)
        });
    if out.is_some() || !native {
        return out;
    }
    // The window takes the size of what it shows, then is given to the dialog (it stays in
    // front of it), centred over it and shown.
    let size = measured.ceil();
    if flow.size.is_none_or(|s| (s - size).abs().max_elem() > 1.0) {
        flow.size = Some(size);
        ctx.request_repaint();
    } else if !flow.shown {
        flow.shown = crate::filedlg::adopt(&title, flow.pending.window());
        ctx.request_repaint();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filedlg::Choice;
    use crate::tests::ui_driver::{Driver, app};

    fn drive(a: &mut EsquisseApp, ui: &mut egui::Ui) {
        show(a, ui.ctx());
        crate::export_section::show(a, ui.ctx());
    }

    /// A 1000 × 600 × 400 box cut by an active section plane across it.
    fn boxed_app() -> EsquisseApp {
        use esq_core::glam::DVec3;
        let mut a = app();
        let e = &mut a.doc.model.root;
        let c = [(0., 0., 0.), (1000., 0., 0.), (1000., 600., 0.), (0., 600., 0.)];
        let v: Vec<_> = [0.0, 400.0]
            .iter()
            .flat_map(|z| c.iter().map(move |(x, y, _)| DVec3::new(*x, *y, *z)))
            .map(|p| e.add_vertex_raw(p))
            .collect();
        for f in [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]] {
            e.add_face_raw(f.iter().map(|i| v[*i]).collect(), vec![]);
        }
        let s = e.sections.insert(esq_core::model::SectionPlane::new(DVec3::new(500.0, 300.0, 200.0), DVec3::Y));
        e.active_section = Some(s);
        a
    }

    /// File › Export › Section Slice: the file first, written at once with the saved options;
    /// "Toujours proposer les options de tranches" (ticked in the options opened from the
    /// dialog) brings the options window back after the next dialog that did not open them,
    /// and its OK writes the file chosen.
    #[test]
    fn section_slice_options_come_after_only_when_asked() {
        let dir = std::env::temp_dir().join(format!("esq-flow-slice-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut a = boxed_app();
        let mut ui = Driver::new();
        ui.side = Some(drive);
        let done =
            |name: &str, i: usize| Event::Done(Some(Choice { path: dir.join(name), type_index: i, image_use: None }));
        assert!(!crate::export_section::always_prompt(), "off at first");
        let (tx, pending) = crate::filedlg::Pending::fake();
        start(&mut a, Purpose::Slice, pending);
        tx.send(done("a.dxf", 1)).unwrap();
        ui.frames(&mut a, 2);
        assert!(a.ui_state.extra.slice.is_none() && dir.join("a.dxf").is_file());

        let (tx, pending) = crate::filedlg::Pending::fake();
        start(&mut a, Purpose::Slice, pending);
        tx.send(Event::Options { type_index: 0, name: String::new() }).unwrap();
        ui.frames(&mut a, 2);
        assert_eq!(ui.toggled("Toujours proposer les options de tranches"), Some(false));
        ui.click(&mut a, "Toujours proposer les options de tranches");
        ui.click(&mut a, "OK");
        assert!(crate::export_section::always_prompt());
        tx.send(done("b.svg", 2)).unwrap();
        ui.frames(&mut a, 2);
        assert!(a.ui_state.extra.slice.is_none(), "the options were opened from the dialog");
        assert!(dir.join("b.svg").is_file());

        let (tx, pending) = crate::filedlg::Pending::fake();
        start(&mut a, Purpose::Slice, pending);
        tx.send(done("c.dwg", 0)).unwrap();
        ui.frames(&mut a, 2);
        assert!(!dir.join("c.dwg").exists(), "the options come first");
        assert_eq!(a.ui_state.extra.slice.as_ref().and_then(|d| d.path.clone()), Some(dir.join("c.dwg")));
        ui.click(&mut a, "Toujours proposer les options de tranches");
        ui.click(&mut a, "OK");
        assert!(dir.join("c.dwg").is_file() && !crate::export_section::always_prompt());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// File › Export › 2D Graphic: "Options..." with PNG chosen opens the raster options over
    /// the dialog; OK keeps them; the dialog's Export then renders with them.
    #[test]
    fn export_2d_options_open_over_the_dialog_and_are_kept() {
        let mut a = app();
        let mut ui = Driver::new();
        ui.side = Some(drive);
        let (tx, pending) = crate::filedlg::Pending::fake();
        start(&mut a, Purpose::Export2d { view: [800, 600], ppp: 1.0 }, pending);
        let png = crate::export2d::types().1.iter().position(|k| *k == "png").unwrap();
        tx.send(Event::Options { type_index: png, name: "vue.png".into() }).unwrap();
        ui.frames(&mut a, 2);
        assert!(ui.has("Effectue le rendu sur un arrière-plan transparent"), "the PNG options");
        ui.click(&mut a, "Utiliser la taille de la vue");
        ui.type_into(&mut a, "Largeur :", "400");
        ui.click(&mut a, "OK");
        assert!(a.ui_state.extra.files.flow.as_ref().unwrap().options.is_none(), "closed, the dialog stays");
        let kept = crate::export2d::load_prefs();
        assert_eq!((kept.use_view_size, kept.width), (false, 400));
        // Annuler drops what was changed.
        tx.send(Event::Options { type_index: png, name: String::new() }).unwrap();
        ui.frames(&mut a, 2);
        ui.type_into(&mut a, "Largeur :", "900");
        ui.click(&mut a, "Annuler");
        assert_eq!(crate::export2d::load_prefs().width, 400);
        // The dialog's Export: a raster job of 400 × 300 pixels.
        let path = std::env::temp_dir().join("esq-flow-vue.png");
        tx.send(Event::Done(Some(Choice { path: path.clone(), type_index: png, image_use: None }))).unwrap();
        ui.frames(&mut a, 1);
        assert!(a.ui_state.extra.files.flow.is_none());
        let job = a.ui_state.extra.jobs.raster.as_ref().expect("the export starts");
        assert_eq!((job.path.clone(), job.size), (path, [400, 300]));
    }

    /// The options window of each type: raster or vector for 2D, the format's own for 3D, none
    /// for a format without options.
    #[test]
    fn each_type_opens_its_own_options() {
        let mut a = app();
        let p = Purpose::Export2d { view: [800, 600], ppp: 1.0 };
        let (types, keys) = crate::export2d::types();
        assert_eq!(types.len(), keys.len());
        let jpg = keys.iter().position(|k| *k == "jpg").unwrap();
        let pdf = keys.iter().position(|k| *k == "pdf").unwrap();
        let dxf = keys.iter().position(|k| *k == "dxf").unwrap();
        assert!(
            matches!(options_window(&mut a, p, jpg, ""), Some(OptionsWin::Raster(d)) if d.kind == crate::export2d::Kind::Jpeg)
        );
        let mut w = options_window(&mut a, p, pdf, "").unwrap();
        assert_eq!(w.title(), "Options d'exportation de PDF");
        assert_eq!(options_window(&mut a, p, dxf, "").unwrap().title(), "Options d'exportation DWG/DXF");
        for (i, f) in esq_formats::ExportFormat::ALL.iter().enumerate() {
            let w = options_window(&mut a, Purpose::Export3d, i, "");
            assert_eq!(w.is_some(), f.has_options(), "{f:?}");
        }
        assert!(matches!(options_window(&mut a, Purpose::Anim, 0, ""), Some(OptionsWin::Anim(_))));
        assert!(options_window(&mut a, Purpose::Slice, 1, "").is_none(), "no section plane to export");
        let mut b = boxed_app();
        let mut w = options_window(&mut b, Purpose::Slice, 1, "").unwrap();
        assert_eq!(w.title(), "Options d'exportation DWG/DXF");
    }

    /// File › Import: "All supported files" takes the importer of the file's extension; a
    /// picture shows "Utiliser image comme :", a 3D format with options shows "Options...".
    #[test]
    fn the_import_dialog_follows_the_type_and_the_file() {
        use crate::filedlg::Controls;
        use crate::formats3d::{ImportKind, import_controls, import_kind_of};
        let none = Controls::default();
        assert_eq!(import_controls(ImportKind::All, ""), none, "no file yet: nothing to show");
        assert_eq!(import_controls(ImportKind::All, "C:/a/plan.PNG"), Controls { options: false, image: true });
        assert_eq!(import_controls(ImportKind::All, "C:/a/maison.skp"), none);
        assert_eq!(import_controls(ImportKind::Skp, "x.png"), none, "the type chosen wins over the name");
        let stl = esq_formats::ImportFormat::ALL.iter().copied().find(|f| f.has_options()).unwrap();
        let ext = stl.extensions()[0];
        assert_eq!(import_kind_of(ImportKind::All, &format!("a.{ext}")), Some(ImportKind::Format(stl)));
        assert_eq!(import_controls(ImportKind::All, &format!("a.{ext}")), Controls { options: true, image: false });
        assert_eq!(import_controls(ImportKind::Format(stl), ""), Controls { options: true, image: false });
        for (i, k) in ImportKind::list().iter().enumerate() {
            if let ImportKind::Picture(_) = k {
                assert!(import_controls(*k, "").image, "{}", k.label());
                assert!(!import_controls(*k, "").options);
            }
            let mut a = app();
            let w = options_window(&mut a, Purpose::Import, i, "");
            assert_eq!(w.is_some(), matches!(k, ImportKind::Format(f) if f.has_options()), "{}", k.label());
        }
        // 3D export: "Options..." only for the formats that have some.
        for f in esq_formats::ExportFormat::ALL {
            assert_eq!(crate::formats3d::export_controls(f).options, f.has_options(), "{f:?}");
        }
    }

    /// The dialog's Import with a picture and "Texture" chosen: the Image tool paints it, and
    /// the choice is the next dialog's.
    #[test]
    fn import_uses_the_picture_as_the_dialog_says() {
        let mut a = app();
        let ctx = egui::Context::default();
        let dir = std::env::temp_dir().join(format!("esq-flow-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("brique.png");
        image::RgbImage::from_pixel(4, 2, image::Rgb([200, 80, 40])).save(&path).unwrap();
        let kinds = crate::formats3d::ImportKind::list();
        let png = kinds.iter().position(|k| k.label() == "Image PNG (*.png)").unwrap();
        let (tx, pending) = crate::filedlg::Pending::fake();
        start(&mut a, Purpose::Import, pending);
        tx.send(Event::Done(Some(Choice { path, type_index: png, image_use: Some(1) }))).unwrap();
        show(&mut a, &ctx);
        assert_eq!(a.tool.id(), crate::tools::ToolId::Image);
        assert_eq!(crate::import2d::load_prefs(), crate::import2d::ImageUse::Texture);
        let req = crate::formats3d::import_request(&mut a);
        assert_eq!((req.title.as_str(), req.ok_label.as_deref()), ("Importer", Some("Importer")));
        assert_eq!((req.initial, req.image_use), (png, Some(1)), "the last type and choice");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The other dialogs of the same base: animation and section slice always have Options;
    /// the preferences ones (`CExportPrefsDialog` / `CImportPrefsDialog`) have none, as with
    /// workspaces, and export says "Exporter".
    #[test]
    fn the_other_dialogs_have_sketchups_buttons() {
        let r = crate::export_anim::request();
        assert_eq!((r.title.as_str(), r.ok_label.as_deref()), ("Exporter l’animation", Some("Exporter")));
        assert!((0..r.types.len()).all(|i| r.controls_of(i, "").options));
        let r = crate::export_section::request(&app());
        assert_eq!((r.title.as_str(), r.ok_label.as_deref()), ("Exporte les tranches 2D", Some("Exporter")));
        assert_eq!(r.types[0].exts, ["dwg"], "DWG first, as SketchUp");
        assert!(r.controls_of(1, "").options);
        let r = crate::dialogs::preferences::prefs_file_request(true);
        assert_eq!(
            (r.title.as_str(), r.ok_label.as_deref(), r.options),
            ("Exporter les préférences", Some("Exporter"), false)
        );
        let r = crate::dialogs::preferences::prefs_file_request(false);
        assert_eq!((r.title.as_str(), r.ok_label.as_deref(), r.options), ("Importer les préférences", None, false));
        // The animation's options are kept for the next export.
        let mut a = app();
        let mut o = AnimOptions { fps: 12, ..Default::default() };
        o.looped = true;
        OptionsWin::Anim(o.clone()).keep(&mut a);
        assert!(matches!(options_window(&mut a, Purpose::Anim, 0, ""), Some(OptionsWin::Anim(x)) if x == o));
    }

    /// Cancelling the dialog does nothing; another dialog cannot open while one is.
    #[test]
    fn a_cancelled_dialog_does_nothing() {
        let mut a = app();
        let ctx = egui::Context::default();
        let (tx, pending) = crate::filedlg::Pending::fake();
        start(&mut a, Purpose::Export3d, pending);
        assert!(open(&mut a, &ctx, Purpose::Import), "the open one is kept");
        tx.send(Event::Done(None)).unwrap();
        show(&mut a, &ctx);
        assert!(a.ui_state.extra.files.flow.is_none() && a.message.is_none());
        let r = crate::formats3d::export_request(&mut a);
        assert_eq!(
            (r.title.as_str(), r.ok_label.as_deref(), r.options),
            ("Exporter le modèle", Some("Exporter"), true)
        );
        let r = crate::export2d::request(&a);
        assert_eq!((r.title.as_str(), r.ok_label.as_deref()), ("Exporter graphique 2D", Some("Exporter")));
        assert!(r.controls_of(0, "").options && r.controls_of(r.types.len() - 1, "").options);
    }
}
