//! File › Import and File › Export › 3D Model, as SketchUp shows them: a file type list, an
//! "Options…" button opening the options of the chosen format (saved between sessions),
//! then the import or export itself and, for the formats that have one, the results text.

use std::path::{Path, PathBuf};

use eframe::egui;
use esq_core::glam::DMat4;
use esq_core::model::{DefId, EntityRef, Instance, InstanceId, Model, SceneCamera, Units};
use esq_formats::options::{Field, FormatOptions};
use esq_formats::{AllOptions, ExportFormat, ImportFormat};

use crate::app::EsquisseApp;
use crate::doc::Document;

/// Which dialog's options are being edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsFor {
    Import(ImportFormat),
    Export(ExportFormat),
}

pub struct OptionsDlg {
    pub which: OptionsFor,
    /// Edited copy: OK keeps it, Cancel drops it.
    pub draft: AllOptions,
}

/// An entry of the import file-type list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportKind {
    All,
    Skp,
    Esq,
    Format(ImportFormat),
    /// A picture type (`import2d::PICTURE_TYPES`).
    Picture(usize),
}

impl ImportKind {
    pub fn list() -> Vec<ImportKind> {
        let mut v = vec![ImportKind::All, ImportKind::Skp, ImportKind::Esq];
        v.extend(ImportFormat::ALL.into_iter().map(ImportKind::Format));
        v.extend((0..crate::import2d::PICTURE_TYPES.len()).map(ImportKind::Picture));
        v
    }

    pub fn label(self) -> &'static str {
        match self {
            ImportKind::All => "Tous les fichiers pris en charge",
            ImportKind::Skp => "Modèles SketchUp (*.skp)",
            ImportKind::Esq => "Modèles Esquisse (*.esq)",
            ImportKind::Format(f) => f.label(),
            ImportKind::Picture(i) => crate::import2d::PICTURE_TYPES[i].0,
        }
    }

    pub fn extensions(self) -> Vec<&'static str> {
        match self {
            ImportKind::All => {
                let mut v = vec!["skp", "esq"];
                for f in ImportFormat::ALL {
                    v.extend(f.extensions());
                }
                v.extend(crate::imgfile::IMAGE_EXTS);
                v.push("pdf");
                v
            }
            ImportKind::Skp => vec!["skp"],
            ImportKind::Esq => vec!["esq"],
            ImportKind::Format(f) => f.extensions().to_vec(),
            ImportKind::Picture(i) => crate::import2d::PICTURE_TYPES[i].1.to_vec(),
        }
    }
}

#[derive(Default)]
pub struct State {
    opts: Option<AllOptions>,
    /// File › Import: the chosen file type.
    pub import: Option<ImportKind>,
    /// File › Export › 3D Model: the chosen format.
    pub export: Option<ExportFormat>,
    pub options: Option<OptionsDlg>,
    /// Title and text of the last results window.
    pub results: Option<(String, String)>,
    /// The types last chosen in the import and export dialogs (selected when they open).
    pub last_import: Option<ImportKind>,
    pub last_export: Option<ExportFormat>,
}

/// `%APPDATA%\Esquisse\formats3d.json` (a temporary folder in tests).
fn options_file() -> Option<PathBuf> {
    Some(crate::settings::user_dir()?.join("formats3d.json"))
}

impl State {
    /// The options of every format (read from `%APPDATA%\Esquisse\formats3d.json` once).
    pub fn opts(&mut self) -> &mut AllOptions {
        self.opts.get_or_insert_with(|| {
            options_file()
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default()
        })
    }

    pub fn set_opts(&mut self, o: AllOptions) {
        if let Some(p) = options_file()
            && let Ok(json) = serde_json::to_vec_pretty(&o)
        {
            let _ = std::fs::write(p, json);
        }
        self.opts = Some(o);
    }
}

fn state(app: &mut EsquisseApp) -> &mut State {
    &mut app.ui_state.extra.formats
}

/// File › Import: SketchUp's dialog (Windows), else the type and options window.
pub fn open_import(app: &mut EsquisseApp, ctx: &egui::Context) {
    if crate::file_flow::open(app, ctx, crate::file_flow::Purpose::Import) {
        return;
    }
    state(app).import = Some(ImportKind::All);
}

/// File › Export › 3D Model (`fmt` preselected by the "Exporter sous …" commands).
pub fn open_export(app: &mut EsquisseApp, ctx: &egui::Context, fmt: Option<ExportFormat>) {
    if let Some(f) = fmt {
        state(app).last_export = Some(f);
    }
    if crate::file_flow::open(app, ctx, crate::file_flow::Purpose::Export3d) {
        return;
    }
    let st = state(app);
    st.export = Some(fmt.or(st.export).unwrap_or(ExportFormat::Dae));
}

/// The kind of file `name` is, for the type `kind` chosen in the import dialog: "All
/// supported files" takes the type of the file's extension (SketchUp asks the importer of
/// that extension).
pub fn import_kind_of(kind: ImportKind, name: &str) -> Option<ImportKind> {
    if kind != ImportKind::All {
        return Some(kind);
    }
    let ext = Path::new(name).extension()?.to_str()?.to_lowercase();
    ImportKind::list().into_iter().skip(1).find(|k| k.extensions().contains(&ext.as_str()))
}

/// What the import dialog shows for `kind` and the file `name` (`CImportDialog`): "Options..."
/// when the importer has options, "Utiliser image comme :" for a picture.
pub fn import_controls(kind: ImportKind, name: &str) -> crate::filedlg::Controls {
    let k = import_kind_of(kind, name);
    crate::filedlg::Controls {
        options: matches!(k, Some(ImportKind::Format(f)) if f.has_options()),
        image: matches!(k, Some(ImportKind::Picture(_))),
    }
}

/// What the export dialog shows for `fmt` (`CExportModelDialog::OnTypeChange`): "Options..."
/// when the exporter has options.
pub fn export_controls(fmt: ExportFormat) -> crate::filedlg::Controls {
    crate::filedlg::Controls { options: fmt.has_options(), image: false }
}

fn file_type(label: &str, exts: Vec<&'static str>) -> crate::filedlg::FileType {
    crate::filedlg::FileType::new(label, &exts)
}

/// File › Import (`CImportDialog`): every importable type, the last one chosen selected,
/// "Options...", "Utiliser image comme :" (the last choice), OK "Importer".
pub fn import_request(app: &mut EsquisseApp) -> crate::filedlg::Request {
    let kinds = ImportKind::list();
    let types = kinds.iter().map(|k| file_type(k.label(), k.extensions())).collect();
    let last = state(app).last_import.unwrap_or(ImportKind::All);
    let initial = kinds.iter().position(|k| *k == last).unwrap_or(0);
    let use_as = crate::import2d::load_prefs();
    crate::filedlg::Request::open("Importer", types, initial, "models")
        .ok_label("Importer")
        .options(move |i, name| import_controls(kinds.get(i).copied().unwrap_or(ImportKind::All), name))
        .image_group(use_as.index())
}

/// File › Export › 3D Model (`CExportModelDialog`): every format, the last one selected,
/// "Options..." for those that have some, OK "Exporter".
pub fn export_request(app: &mut EsquisseApp) -> crate::filedlg::Request {
    let formats = ExportFormat::ALL;
    let types = formats.iter().map(|f| file_type(f.label(), vec![f.extension()])).collect();
    let last = state(app).last_export.unwrap_or(ExportFormat::Dae);
    let initial = formats.iter().position(|f| *f == last).unwrap_or(0);
    let stem = app.doc.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
    crate::filedlg::Request::save(
        "Exporter le modèle",
        &stem.unwrap_or_else(|| "modele".into()),
        types,
        initial,
        "export",
    )
    .ok_label("Exporter")
    .options(move |i, _| export_controls(formats[i.min(formats.len() - 1)]))
}

/// The options window of type `i` of the import dialog for the file `name` (none when the
/// importer has no options).
pub fn import_options(app: &mut EsquisseApp, i: usize, name: &str) -> Option<OptionsDlg> {
    let kind = ImportKind::list().get(i).copied()?;
    match import_kind_of(kind, name)? {
        ImportKind::Format(f) if f.has_options() => {
            Some(OptionsDlg { which: OptionsFor::Import(f), draft: state(app).opts().clone() })
        }
        _ => None,
    }
}

/// The format at `i` in the export dialog's list.
pub fn format_at(i: usize) -> ExportFormat {
    ExportFormat::ALL[i.min(ExportFormat::ALL.len() - 1)]
}

/// The options window of format `i` of the export dialog.
pub fn export_options(app: &mut EsquisseApp, i: usize) -> Option<OptionsDlg> {
    let f = ExportFormat::ALL.get(i).copied().filter(|f| f.has_options())?;
    Some(OptionsDlg { which: OptionsFor::Export(f), draft: state(app).opts().clone() })
}

impl OptionsDlg {
    /// "Options d'exportation de fichier 3DS"…
    pub fn title(&mut self) -> String {
        let o = match self.which {
            OptionsFor::Import(f) => self.draft.import_options(f),
            OptionsFor::Export(f) => self.draft.export_options(f),
        };
        o.map(|o| o.title().to_string()).unwrap_or_else(|| "Options".into())
    }

    /// The controls of the options, above OK / Annuler.
    pub fn body(&mut self, ui: &mut egui::Ui) {
        let o = match self.which {
            OptionsFor::Import(f) => self.draft.import_options(f),
            OptionsFor::Export(f) => self.draft.export_options(f),
        };
        if let Some(o) = o {
            fields_ui(ui, o);
        }
    }
}

/// The file chosen in the import dialog: a picture is used as "Utiliser image comme :" says,
/// any other file is imported as a component.
pub fn import_chosen(
    app: &mut EsquisseApp,
    ctx: &egui::Context,
    path: &Path,
    kind: usize,
    use_as: Option<crate::import2d::ImageUse>,
) {
    let k = ImportKind::list().get(kind).copied();
    state(app).last_import = k;
    if let Some(u) = use_as {
        crate::import2d::save_prefs(u);
    }
    if crate::imgfile::is_image(path) {
        let u = use_as.unwrap_or_else(crate::import2d::load_prefs);
        crate::import2d::import_as(app, ctx, path, u);
        return;
    }
    import_path(app, ctx, path);
}

/// The file chosen in the export dialog, written with the options kept.
pub fn export_chosen(app: &mut EsquisseApp, ctx: &egui::Context, path: &Path, fmt: ExportFormat) {
    state(app).last_export = Some(fmt);
    match export_path(app, path, fmt) {
        Ok(report) => {
            if let Some(r) = report {
                state(app).results = Some(("Résultats de l’exportation".into(), r));
            }
            app.say(ctx, format!("Exporté : {}", path.display()));
        }
        Err(e) => app.say(ctx, format!("Échec de l'export : {e:#}")),
    }
}

/// Where an imported component goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placement {
    /// At the origin of the active context ("Préserver l'origine du dessin", .skp files).
    AtOrigin,
    /// Held by the cursor until a click.
    WithCursor(DefId),
}

/// A file read for File › Import, not yet in the model.
pub struct Loaded {
    pub model: Model,
    /// Name of the component (the file name without extension).
    pub name: String,
    /// "Résultats de l'importation", for the formats that have one.
    pub report: Option<String>,
    pub at_origin: bool,
}

/// Reads a file of any importable format (`.skp` and `.esq` included).
pub fn load_import(path: &Path, opts: &AllOptions, units: Units) -> anyhow::Result<Loaded> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Import".into());
    let (mut src, report, at_origin) = if ext == "skp" || ext == "esq" {
        let m = if ext == "skp" { esq_skp::import_skp(path)? } else { esq_core::io::load_esq(path)? };
        (m, None, true)
    } else {
        let imp = esq_formats::import(path, opts, units)?;
        (imp.model, imp.report, imp.preserve_origin)
    };
    if src.root.is_empty() {
        anyhow::bail!("le fichier ne contient rien à importer");
    }
    if !at_origin {
        esq_formats::build::recenter(&mut src);
    }
    Ok(Loaded { model: src, name, report, at_origin })
}

/// Puts what was read into collection `ctx` of `m` as a component: an instance at the origin,
/// or only the definition when the cursor is to place it.
pub fn insert_import(m: &mut Model, ctx: Option<DefId>, l: &Loaded) -> Placement {
    let id = m.import_as_component(&l.model, &l.name);
    if !l.at_origin {
        return Placement::WithCursor(id);
    }
    m.entities_mut(ctx).instances.insert(Instance {
        def: id,
        xform: DMat4::IDENTITY,
        name: String::new(),
        material: None,
        layer: 0,
        hidden: false,
        locked: false,
        attributes: Default::default(),
        ext: Default::default(),
    });
    Placement::AtOrigin
}

/// Imports a file into the document as a component named after the file. Returns where it
/// goes and the "Résultats de l'importation" text of the formats that have one.
pub fn import_into(doc: &mut Document, opts: &AllOptions, path: &Path) -> anyhow::Result<(Placement, Option<String>)> {
    let mut l = load_import(path, opts, doc.model.units)?;
    let mut placement = Placement::AtOrigin;
    doc.commit("Importer", |m, d| placement = insert_import(m, d, &l));
    Ok((placement, l.report.take()))
}

/// File › Import of a file chosen in the dialog.
pub fn import_path(app: &mut EsquisseApp, ctx: &egui::Context, path: &Path) {
    // A picture: "Utiliser image comme" (image, texture, Match Photo).
    if crate::import2d::start(app, ctx, path) {
        return;
    }
    let opts = state(app).opts().clone();
    match import_into(&mut app.doc, &opts, path) {
        Ok((placement, report)) => {
            if let Placement::WithCursor(d) = placement {
                app.place_component(d);
            }
            if let Some(r) = report {
                state(app).results = Some(("Résultats de l'importation".into(), r));
            }
            app.say(ctx, format!("Importé : {}", path.display()));
        }
        Err(e) => app.say(ctx, format!("Import impossible : {e:#}")),
    }
}

/// Writes the model (or the selection, when the format's option says so) in `fmt`; returns
/// the results text, if the format shows one.
pub fn export_doc(
    doc: &Document,
    view: Option<SceneCamera>,
    opts: &AllOptions,
    path: &Path,
    fmt: ExportFormat,
) -> anyhow::Result<Option<String>> {
    let sel = if fmt.selection_only(opts) { doc.sel.as_slice() } else { &[] };
    export_model(&doc.model, &doc.ctx, sel, view, opts, path, fmt)
}

/// Writes `model` in `fmt`: only the entities `sel` of the active context `ctx` when there
/// are some, else the whole model.
pub fn export_model(
    model: &Model,
    ctx: &[InstanceId],
    sel: &[EntityRef],
    view: Option<SceneCamera>,
    opts: &AllOptions,
    path: &Path,
    fmt: ExportFormat,
) -> anyhow::Result<Option<String>> {
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let cx = esq_formats::ExportCx { view, name };
    let part;
    let model = if sel.is_empty() {
        model
    } else {
        part = esq_formats::scene::selection_model(model, ctx, sel);
        &part
    };
    esq_formats::export(model, path, fmt, opts, &cx)
}

pub fn export_path(app: &mut EsquisseApp, path: &Path, fmt: ExportFormat) -> anyhow::Result<Option<String>> {
    let opts = state(app).opts().clone();
    let v = app.cam.to_view();
    let view = SceneCamera { eye: v.eye, target: v.target, up: v.up, fov_deg: v.fov_deg, perspective: v.perspective };
    export_doc(&app.doc, Some(view), &opts, path, fmt)
}

/// Draws the controls of an options dialog.
pub fn fields_ui(ui: &mut egui::Ui, o: &mut dyn FormatOptions) {
    let mut groups: Vec<(Option<&'static str>, Vec<Field>)> = vec![(None, Vec::new())];
    for f in o.fields() {
        match f {
            Field::Group(t) => groups.push((Some(t), Vec::new())),
            f => groups.last_mut().unwrap().1.push(f),
        }
    }
    for (title, fields) in groups {
        if title.is_none() && fields.is_empty() {
            continue;
        }
        let body = |ui: &mut egui::Ui| {
            for f in fields {
                match f {
                    Field::Check(label, v) => {
                        ui.checkbox(v, label);
                    }
                    Field::Combo(label, p) => {
                        let labels = p.labels();
                        let mut i = p.index();
                        ui.horizontal(|ui| {
                            ui.label(label);
                            egui::ComboBox::from_id_salt(label)
                                .selected_text(labels.get(i).copied().unwrap_or(""))
                                .show_ui(ui, |ui| {
                                    for (k, l) in labels.iter().enumerate() {
                                        ui.selectable_value(&mut i, k, *l);
                                    }
                                });
                        });
                        p.set_index(i);
                    }
                    Field::Radio(label, p) => {
                        let labels = p.labels();
                        let mut i = p.index();
                        ui.horizontal(|ui| {
                            ui.label(label);
                            for (k, l) in labels.iter().enumerate() {
                                ui.radio_value(&mut i, k, *l);
                            }
                        });
                        p.set_index(i);
                    }
                    Field::Int(label, v, lo, hi) => {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            ui.add(egui::DragValue::new(v).range(lo..=hi));
                        });
                    }
                    Field::Note(t) => {
                        ui.label(egui::RichText::new(t).small().weak());
                    }
                    Field::Value(label, v) => {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            let mut text = v;
                            ui.add_enabled(false, egui::TextEdit::singleline(&mut text).desired_width(80.0));
                        });
                    }
                    Field::Group(_) => {}
                }
            }
        };
        match title {
            Some(t) => {
                ui.label(egui::RichText::new(t).strong());
                ui.group(body);
            }
            None => {
                body(ui);
            }
        }
    }
}

fn dialog(title: &str) -> egui::Window<'static> {
    egui::Window::new(title.to_string())
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
}

/// Called every frame (see `extra.rs`).
pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    show_options(app, ctx);
    if state(app).options.is_none() {
        show_import(app, ctx);
        show_export(app, ctx);
    }
    show_results(app, ctx);
}

fn show_options(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(mut dlg) = state(app).options.take() else { return };
    let mut close = None;
    let title = match dlg.which {
        OptionsFor::Import(f) => dlg.draft.import_options(f).map(|o| o.title()),
        OptionsFor::Export(f) => dlg.draft.export_options(f).map(|o| o.title()),
    };
    let Some(title) = title else { return };
    dialog(title).show(ctx, |ui| {
        let o = match dlg.which {
            OptionsFor::Import(f) => dlg.draft.import_options(f),
            OptionsFor::Export(f) => dlg.draft.export_options(f),
        };
        if let Some(o) = o {
            fields_ui(ui, o);
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                close = Some(true);
            }
            if ui.button("Annuler").clicked() {
                close = Some(false);
            }
        });
    });
    match close {
        Some(true) => state(app).set_opts(dlg.draft),
        Some(false) => {}
        None => state(app).options = Some(dlg),
    }
}

fn show_import(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(mut kind) = state(app).import else { return };
    let mut action = None;
    dialog("Importer").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Type de fichier :");
            egui::ComboBox::from_id_salt("import-kind").selected_text(kind.label()).width(300.0).show_ui(ui, |ui| {
                for k in ImportKind::list() {
                    ui.selectable_value(&mut kind, k, k.label());
                }
            });
        });
        let with_options = matches!(kind, ImportKind::Format(f) if f.has_options());
        ui.horizontal(|ui| {
            if ui.add_enabled(with_options, egui::Button::new("Options...")).clicked() {
                action = Some(1);
            }
            if ui.button("Importer...").clicked() {
                action = Some(2);
            }
            if ui.button("Annuler").clicked() {
                action = Some(0);
            }
        });
    });
    state(app).import = Some(kind);
    match action {
        Some(0) => state(app).import = None,
        Some(1) => {
            if let ImportKind::Format(f) = kind {
                let draft = state(app).opts().clone();
                state(app).options = Some(OptionsDlg { which: OptionsFor::Import(f), draft });
            }
        }
        Some(2) => {
            let exts = kind.extensions();
            // Preferences › Files › Models: where the dialog starts.
            if let Some(p) = crate::settings::dialog("models").add_filter(kind.label(), &exts).pick_file() {
                state(app).import = None;
                import_path(app, ctx, &p);
            }
        }
        _ => {}
    }
}

fn show_export(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(mut fmt) = state(app).export else { return };
    let mut action = None;
    dialog("Exporter le modèle").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Type de fichier :");
            egui::ComboBox::from_id_salt("export-kind").selected_text(fmt.label()).width(300.0).show_ui(ui, |ui| {
                for f in ExportFormat::ALL {
                    ui.selectable_value(&mut fmt, f, f.label());
                }
            });
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(fmt.has_options(), egui::Button::new("Options...")).clicked() {
                action = Some(1);
            }
            if ui.button("Exporter...").clicked() {
                action = Some(2);
            }
            if ui.button("Annuler").clicked() {
                action = Some(0);
            }
        });
    });
    state(app).export = Some(fmt);
    match action {
        Some(0) => state(app).export = None,
        Some(1) => {
            let draft = state(app).opts().clone();
            state(app).options = Some(OptionsDlg { which: OptionsFor::Export(fmt), draft });
        }
        Some(2) => {
            let stem = app.doc.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
            let file = format!("{}.{}", stem.unwrap_or_else(|| "modele".into()), fmt.extension());
            // Preferences › Files › Export: where the dialog starts.
            if let Some(p) = crate::settings::dialog("export")
                .add_filter(fmt.label(), &[fmt.extension()])
                .set_file_name(&file)
                .save_file()
            {
                state(app).export = None;
                export_chosen(app, ctx, &p, fmt);
            }
        }
        _ => {}
    }
}

fn show_results(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some((title, text)) = state(app).results.clone() else { return };
    let mut close = false;
    dialog(&title).show(ctx, |ui| {
        ui.label(text.replace('\t', "    "));
        if ui.button("Fermer").clicked() {
            close = true;
        }
    });
    if close {
        state(app).results = None;
    }
}
