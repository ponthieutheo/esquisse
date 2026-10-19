//! File › Export › 2D Graphic, as SketchUp's `CModelExportRaster` does it: the type is
//! chosen in the save dialog (JPEG, PNG, TIFF, Targa, Windows bitmap), then the export
//! options (`CExportImageOptionsDlg`, its "extended" variant for PNG/TIFF and the JPEG one):
//!
//! - image size: the view's size in pixels ("Utiliser la taille de la vue", read-only then),
//!   or a width and a height linked by the view's proportions (`ImageExport::HeightForWidth`);
//! - line scale ("Échelle de ligne", 0.25 to 100, the display's scale with the view size);
//! - anti-aliasing: like SketchUp's `RenderToBitmap`, the picture is rendered twice as large
//!   (lines twice as thick) and reduced;
//! - transparent background (PNG and TIFF);
//! - JPEG compression (50 to 100, "Petit fichier" … "Meilleure qualité").
//!
//! The picture is rendered offscreen in tiles of 2048 pixels (SketchUp's tile size), a few per
//! displayed frame, so any size can be exported. The options are kept between sessions
//! (SketchUp's `ImageExport*` preferences).

use std::io::Cursor;
use std::path::PathBuf;

use eframe::egui;
use eframe::egui_wgpu;
use esq_core::glam::DVec2;
use serde_json::{Value, json};

use crate::app::EsquisseApp;
use crate::camera::Camera;
use crate::filedlg::FileType;
use crate::ui::centered;

/// Side of the tiles rendered offscreen, in rendered pixels (`CRenderBitmapView`: 0x800).
pub const TILE: u32 = 2048;
/// `ImageExport::MinimumLineScale` / `MaximumLineScale`.
pub const MIN_LINE_SCALE: f64 = 0.25;
pub const MAX_LINE_SCALE: f64 = 100.0;
/// The JPEG slider of `CExportJpgOptionsDlg`.
pub const JPEG_MIN: u8 = 50;
pub const JPEG_MAX: u8 = 100;

/// The types of File › Export › 2D Graphic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Jpeg,
    Png,
    Tiff,
    Tga,
    Bmp,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Jpeg, Kind::Png, Kind::Tiff, Kind::Tga, Kind::Bmp];

    pub fn file_type(self) -> FileType {
        match self {
            Kind::Jpeg => FileType::new("Image JPEG", &["jpg", "jpeg"]),
            Kind::Png => FileType::new("Image PNG", &["png"]),
            Kind::Tiff => FileType::new("Image TIFF", &["tif", "tiff"]),
            Kind::Tga => FileType::new("Fichier Targa", &["tga"]),
            Kind::Bmp => FileType::new("Bitmap Windows", &["bmp"]),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Kind::Jpeg => "jpg",
            Kind::Png => "png",
            Kind::Tiff => "tif",
            Kind::Tga => "tga",
            Kind::Bmp => "bmp",
        }
    }

    /// The "extended" options (transparent background) are offered for PNG and TIFF.
    pub fn has_alpha_option(self) -> bool {
        matches!(self, Kind::Png | Kind::Tiff)
    }
}

/// The options of the raster export (SketchUp's preferences of the same names).
#[derive(Clone, Debug, PartialEq)]
pub struct RasterOptions {
    /// `ImageExportUseVPSize`.
    pub use_view_size: bool,
    /// `ImageExportWidth` / `ImageExportHeight`, pixels.
    pub width: u32,
    pub height: u32,
    /// `ImageExportLineScale`.
    pub line_scale: f64,
    /// `ImageExportUseAA`.
    pub antialias: bool,
    /// `ImageExportUseTransparentBG`.
    pub transparent: bool,
    /// `JpgExportQuality`.
    pub jpeg_quality: u8,
}

impl Default for RasterOptions {
    /// SketchUp's initial values.
    fn default() -> Self {
        RasterOptions {
            use_view_size: true,
            width: 100,
            height: 100,
            line_scale: 1.0,
            antialias: true,
            transparent: false,
            jpeg_quality: 100,
        }
    }
}

impl RasterOptions {
    fn to_json(&self) -> Value {
        json!({
            "use_view_size": self.use_view_size, "width": self.width, "height": self.height,
            "line_scale": self.line_scale, "antialias": self.antialias,
            "transparent": self.transparent, "jpeg_quality": self.jpeg_quality,
        })
    }

    pub(crate) fn from_json(v: &Value) -> RasterOptions {
        let d = RasterOptions::default();
        let b = |k: &str, x: bool| v.get(k).and_then(Value::as_bool).unwrap_or(x);
        let n = |k: &str, x: u64| v.get(k).and_then(Value::as_u64).unwrap_or(x);
        RasterOptions {
            use_view_size: b("use_view_size", d.use_view_size),
            width: n("width", d.width as u64).clamp(1, u32::MAX as u64) as u32,
            height: n("height", d.height as u64).clamp(1, u32::MAX as u64) as u32,
            line_scale: v.get("line_scale").and_then(Value::as_f64).unwrap_or(d.line_scale),
            antialias: b("antialias", d.antialias),
            transparent: b("transparent", d.transparent),
            jpeg_quality: n("jpeg_quality", d.jpeg_quality as u64).clamp(JPEG_MIN as u64, JPEG_MAX as u64) as u8,
        }
    }

    /// The size and line scale of the picture for a view of `view` pixels whose display
    /// scale is `display_scale`: the view's own size, or the chosen width with the view's
    /// proportions (SketchUp keeps the height in step with the width).
    pub fn resolve(&self, view: [u32; 2], display_scale: f64) -> ([u32; 2], f64) {
        if self.use_view_size {
            return ([view[0].max(1), view[1].max(1)], display_scale.clamp(MIN_LINE_SCALE, MAX_LINE_SCALE));
        }
        let w = self.width.max(1);
        ([w, height_for_width(view, w)], self.line_scale.clamp(MIN_LINE_SCALE, MAX_LINE_SCALE))
    }
}

/// `ImageExport::HeightForWidth`: the height that keeps the view's proportions.
pub fn height_for_width(view: [u32; 2], w: u32) -> u32 {
    let aspect = view[0].max(1) as f64 / view[1].max(1) as f64;
    ((w as f64 / aspect).round() as u32).max(1)
}

/// `ImageExport::WidthForHeight`.
pub fn width_for_height(view: [u32; 2], h: u32) -> u32 {
    let aspect = view[0].max(1) as f64 / view[1].max(1) as f64;
    ((h as f64 * aspect).round() as u32).max(1)
}

fn prefs_path() -> Option<PathBuf> {
    crate::prefs::dir().map(|d| d.join("export2d.json"))
}

/// One reader or writer of the preferences file at a time: two windows (or a script and a
/// window, or two tests) saving different keys at once would otherwise lose one of them.
static PREFS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn read_unlocked() -> Value {
    prefs_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

/// The preferences of the 2D exports: `type` (last type exported), `raster`, and one set of
/// options per vector type.
pub fn read_prefs() -> Value {
    let _g = PREFS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    read_unlocked()
}

/// Sets one key of the preferences (the others are kept).
pub fn write_pref(key: &str, value: Value) {
    let _g = PREFS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = read_unlocked();
    v[key] = value;
    if let Some(p) = prefs_path() {
        let _ = std::fs::write(p, v.to_string());
    }
}

/// The options of the raster export.
pub fn load_prefs() -> RasterOptions {
    RasterOptions::from_json(read_prefs().get("raster").unwrap_or(&Value::Null))
}

pub fn save_prefs(o: &RasterOptions, kind: Kind) {
    save_options(o);
    write_pref("type", json!(kind.key()));
}

/// The options of the raster export, kept (OK of the options window).
pub fn save_options(o: &RasterOptions) {
    write_pref("raster", o.to_json());
}

/// The camera that renders the pixels `x0..x0+tw`, `y0..y0+th` of the picture `full` that
/// `base` shows (`base.size` is ignored): same eye, narrower field and shifted image plane.
pub fn tile_camera(base: &Camera, full: [u32; 2], x0: u32, y0: u32, tw: u32, th: u32) -> Camera {
    let (w, h) = (full[0] as f64, full[1] as f64);
    let (tw, th) = (tw as f64, th as f64);
    let (x0, y0) = (x0 as f64, y0 as f64);
    let s = DVec2::new(w / tw, h / th);
    let o = DVec2::new((w - 2.0 * x0 - tw) / tw, (th - h + 2.0 * y0) / th);
    let mut c = base.clone();
    c.size = DVec2::new(tw, th);
    c.fov_y = 2.0 * ((base.fov_y * 0.5).tan() / s.y).atan();
    c.shift = base.shift * s;
    c.lens_shift = base.lens_shift * s + o;
    c
}

/// Tiles covering a picture of `size` rendered pixels: (x, y, w, h), row by row.
pub fn tiles(size: [u32; 2], tile: u32) -> Vec<[u32; 4]> {
    let mut out = Vec::new();
    let mut y = 0;
    while y < size[1] {
        let th = tile.min(size[1] - y);
        let mut x = 0;
        while x < size[0] {
            let tw = tile.min(size[0] - x);
            out.push([x, y, tw, th]);
            x += tw;
        }
        y += th;
    }
    out
}

/// Halves a picture (2 × 2 box filter); `w` and `h` are even.
pub fn downsample2(w: u32, h: u32, px: &[u8]) -> Vec<u8> {
    let (ow, oh) = (w / 2, h / 2);
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    for y in 0..oh as usize {
        for x in 0..ow as usize {
            for c in 0..4 {
                let at = |dx: usize, dy: usize| px[((2 * y + dy) * w as usize + 2 * x + dx) * 4 + c] as u32;
                out[(y * ow as usize + x) * 4 + c] = ((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8;
            }
        }
    }
    out
}

/// Turns colours blended over a transparent background (premultiplied) into straight alpha.
pub fn unpremultiply(px: &mut [u8]) {
    for p in px.as_chunks_mut::<4>().0 {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

/// The file's bytes. Without the transparent background, pixels are opaque and the file has
/// no alpha channel (JPEG and BMP never have one).
pub fn encode(
    kind: Kind,
    w: u32,
    h: u32,
    rgba: Vec<u8>,
    transparent: bool,
    jpeg_quality: u8,
) -> anyhow::Result<Vec<u8>> {
    use image::{DynamicImage, ImageFormat, RgbaImage};
    let img = DynamicImage::ImageRgba8(
        RgbaImage::from_raw(w, h, rgba).ok_or_else(|| anyhow::anyhow!("taille d'image incohérente"))?,
    );
    let alpha = transparent && kind.has_alpha_option();
    let img = if alpha { img } else { DynamicImage::ImageRgb8(img.to_rgb8()) };
    let mut out = Vec::new();
    match kind {
        Kind::Jpeg => {
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, jpeg_quality.clamp(1, 100));
            img.write_with_encoder(enc)?;
        }
        Kind::Png => img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)?,
        Kind::Tiff => img.write_to(&mut Cursor::new(&mut out), ImageFormat::Tiff)?,
        Kind::Tga => img.write_to(&mut Cursor::new(&mut out), ImageFormat::Tga)?,
        Kind::Bmp => img.write_to(&mut Cursor::new(&mut out), ImageFormat::Bmp)?,
    }
    Ok(out)
}

/// The options window, shown once the file is chosen.
#[derive(Clone, Debug)]
pub struct Export2dDialog {
    pub path: PathBuf,
    pub kind: Kind,
    pub options: RasterOptions,
    /// Size of the view, pixels, when the dialog opened.
    pub view: [u32; 2],
    pub display_scale: f64,
    /// Texts being typed in the size and line scale fields.
    width_text: String,
    height_text: String,
    scale_text: String,
}

impl Export2dDialog {
    pub fn new(path: PathBuf, kind: Kind, options: RasterOptions, view: [u32; 2], display_scale: f64) -> Self {
        let mut d = Export2dDialog {
            path,
            kind,
            options,
            view,
            display_scale,
            width_text: String::new(),
            height_text: String::new(),
            scale_text: String::new(),
        };
        if d.options.use_view_size {
            d.options.width = view[0].max(1);
        }
        d.options.height = height_for_width(view, d.options.width);
        d.sync_texts();
        d
    }

    fn sync_texts(&mut self) {
        let ([w, h], ls) = self.options.resolve(self.view, self.display_scale);
        self.width_text = w.to_string();
        self.height_text = h.to_string();
        self.scale_text = format!("{ls:.2}").replace('.', ",");
    }

    /// While a size is being typed, the other one follows (SketchUp's `textChanged`).
    fn typed_width(&mut self) {
        if let Some(v) = parse_num(&self.width_text).filter(|v| *v >= 1.0) {
            self.options.width = v.round() as u32;
            self.options.height = height_for_width(self.view, self.options.width);
            self.height_text = self.options.height.to_string();
        }
    }

    fn typed_height(&mut self) {
        if let Some(v) = parse_num(&self.height_text).filter(|v| *v >= 1.0) {
            self.options.height = v.round() as u32;
            self.options.width = width_for_height(self.view, self.options.height);
            self.width_text = self.options.width.to_string();
        }
    }

    fn typed_scale(&mut self) {
        if let Some(v) = parse_num(&self.scale_text) {
            self.options.line_scale = v.clamp(MIN_LINE_SCALE, MAX_LINE_SCALE);
        }
    }

    pub fn set_use_view_size(&mut self, on: bool) {
        self.options.use_view_size = on;
        if on {
            self.options.width = self.view[0].max(1);
            self.options.height = self.view[1].max(1);
            self.options.line_scale = self.display_scale.clamp(MIN_LINE_SCALE, MAX_LINE_SCALE);
        }
        self.sync_texts();
    }
}

pub(crate) fn parse_num(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse().ok()
}

/// The types of File › Export › 2D Graphic: the raster ones, then the vector ones
/// (hidden-line drawings), with the key each is remembered by.
pub fn types() -> (Vec<FileType>, Vec<&'static str>) {
    let vec_kinds = crate::export_vec::VecKind::ALL;
    let mut types: Vec<FileType> = Kind::ALL.iter().map(|k| k.file_type()).collect();
    types.extend(vec_kinds.iter().map(|k| k.file_type()));
    let mut keys: Vec<&str> = Kind::ALL.iter().map(|k| k.key()).collect();
    keys.extend(vec_kinds.iter().map(|k| k.key()));
    (types, keys)
}

/// The type of the list at `i`: a raster kind, or a vector one.
pub fn kind_at(i: usize) -> Result<Kind, crate::export_vec::VecKind> {
    match Kind::ALL.get(i) {
        Some(k) => Ok(*k),
        None => {
            Err(crate::export_vec::VecKind::ALL[(i - Kind::ALL.len()).min(crate::export_vec::VecKind::ALL.len() - 1)])
        }
    }
}

/// The dialog of File › Export › 2D Graphic (`CExportHLRDialog`): the type last exported
/// selected, "Options..." for every type, OK "Exporter".
pub fn request(app: &EsquisseApp) -> crate::filedlg::Request {
    let (types, keys) = types();
    let last = read_prefs().get("type").and_then(Value::as_str).map(str::to_string);
    let initial = last.and_then(|k| keys.iter().position(|x| *x == k)).unwrap_or(1);
    crate::filedlg::Request::save("Exporter graphique 2D", &file_name(app), types, initial, "export")
        .ok_label("Exporter")
        .options(|_, _| crate::filedlg::Controls { options: true, image: false })
}

fn file_name(app: &EsquisseApp) -> String {
    let stem = app.doc.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
    stem.unwrap_or_else(|| "vue".into())
}

/// The size of the view in pixels at `ppp` pixels per point.
pub fn view_pixels(app: &EsquisseApp, ppp: f32) -> [u32; 2] {
    [(app.cam.size.x * ppp as f64).round() as u32, (app.cam.size.y * ppp as f64).round() as u32]
}

/// File › Export › 2D Graphic: SketchUp's dialog with its Options button (Windows), else the
/// save dialog, then the options.
pub fn start(app: &mut EsquisseApp, ctx: &egui::Context) {
    let ppp = ctx.pixels_per_point();
    let view = view_pixels(app, ppp);
    if crate::file_flow::open(app, ctx, crate::file_flow::Purpose::Export2d { view, ppp }) {
        return;
    }
    let options = load_prefs();
    let req = request(app);
    let Some((path, i)) = crate::filedlg::save(&req.title, &req.file_name, &req.types, req.initial, "export") else {
        return;
    };
    match kind_at(i) {
        Ok(kind) => app.ui_state.extra.export2d = Some(Export2dDialog::new(path, kind, options, view, ppp as f64)),
        Err(vk) => crate::export_vec::open(app, path, vk),
    }
}

/// The file chosen in the dialog, exported with the options kept: a raster picture is
/// rendered tile by tile from the next frame, a vector drawing written at once.
pub fn export_chosen(app: &mut EsquisseApp, ctx: &egui::Context, path: PathBuf, i: usize, view: [u32; 2], ppp: f32) {
    match kind_at(i) {
        Ok(kind) => {
            let o = load_prefs();
            save_prefs(&o, kind);
            let (size, line_scale) = o.resolve(view, ppp as f64);
            app.ui_state.extra.jobs.raster = Some(RasterJob::new(path, kind, &o, size, line_scale));
            ctx.request_repaint();
        }
        Err(vk) => crate::export_vec::export_now(app, ctx, path, vk),
    }
}

/// The controls of the options window of a raster export (`CExportImageOptionsDlg` and its
/// PNG/TIFF and JPEG variants).
pub fn options_ui(ui: &mut egui::Ui, dlg: &mut Export2dDialog) {
    ui.label(egui::RichText::new("Taille de l'image").strong());
    let mut use_vp = dlg.options.use_view_size;
    if ui.checkbox(&mut use_vp, "Utiliser la taille de la vue").changed() {
        dlg.set_use_view_size(use_vp);
    }
    let editable = !dlg.options.use_view_size;
    egui::Grid::new("export2d_size").num_columns(3).show(ui, |ui| {
        let field = |ui: &mut egui::Ui, text: &mut String, label: &str| {
            let l = ui.label(label);
            ui.add_enabled(editable, egui::TextEdit::singleline(text).desired_width(80.0).id_salt(label))
                .labelled_by(l.id)
        };
        let r = field(ui, &mut dlg.width_text, "Largeur :");
        if r.changed() {
            dlg.typed_width();
        }
        if r.lost_focus() {
            dlg.sync_texts();
        }
        ui.label("pixels");
        ui.end_row();
        let r = field(ui, &mut dlg.height_text, "Hauteur :");
        if r.changed() {
            dlg.typed_height();
        }
        if r.lost_focus() {
            dlg.sync_texts();
        }
        ui.label("pixels");
        ui.end_row();
        let r = field(ui, &mut dlg.scale_text, "Échelle de ligne :")
            .on_hover_text(format!("Entrez un nombre entre {MIN_LINE_SCALE:.2} et {MAX_LINE_SCALE:.2}"));
        if r.changed() {
            dlg.typed_scale();
        }
        if r.lost_focus() {
            dlg.sync_texts();
        }
        ui.label("");
        ui.end_row();
    });
    ui.separator();
    ui.label(egui::RichText::new("Rendu").strong());
    ui.checkbox(&mut dlg.options.antialias, "Anticrénelage");
    if dlg.kind.has_alpha_option() {
        ui.checkbox(&mut dlg.options.transparent, "Effectue le rendu sur un arrière-plan transparent");
    }
    if dlg.kind == Kind::Jpeg {
        ui.separator();
        ui.label(egui::RichText::new("Compression JPEG").strong());
        ui.horizontal(|ui| {
            ui.label("Petit fichier");
            ui.add(egui::Slider::new(&mut dlg.options.jpeg_quality, JPEG_MIN..=JPEG_MAX).show_value(false));
            ui.label("Meilleure qualité");
        });
    }
}

/// The options window (OK starts the export).
pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(mut dlg) = app.ui_state.extra.export2d.take() else { return };
    let mut action = None;
    centered("Options d'exportation").show(ctx, |ui| {
        options_ui(ui, &mut dlg);
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Exporter").clicked() {
                action = Some(true);
            }
            if ui.button("Annuler").clicked() {
                action = Some(false);
            }
        });
    });
    match action {
        Some(false) => {}
        Some(true) => {
            save_prefs(&dlg.options, dlg.kind);
            let (size, line_scale) = dlg.options.resolve(dlg.view, dlg.display_scale);
            app.ui_state.extra.jobs.raster = Some(RasterJob::new(dlg.path, dlg.kind, &dlg.options, size, line_scale));
            ctx.request_repaint();
        }
        None => app.ui_state.extra.export2d = Some(dlg),
    }
}

/// An export being rendered, tile by tile.
pub struct RasterJob {
    pub path: PathBuf,
    pub kind: Kind,
    /// Final size, pixels.
    pub size: [u32; 2],
    /// Rendered pixels per final pixel (2 with anti-aliasing).
    pub factor: u32,
    pub line_scale: f64,
    pub transparent: bool,
    pub jpeg_quality: u8,
    pub tiles: Vec<[u32; 4]>,
    pub next: usize,
    pub pixels: Vec<u8>,
}

impl RasterJob {
    pub fn new(path: PathBuf, kind: Kind, o: &RasterOptions, size: [u32; 2], line_scale: f64) -> RasterJob {
        let factor = if o.antialias { 2 } else { 1 };
        RasterJob {
            path,
            kind,
            size,
            factor,
            line_scale,
            transparent: o.transparent && kind.has_alpha_option(),
            jpeg_quality: o.jpeg_quality,
            tiles: tiles([size[0] * factor, size[1] * factor], TILE),
            next: 0,
            pixels: vec![0; size[0] as usize * size[1] as usize * 4],
        }
    }

    /// Copies a rendered tile (rendered pixels) into the picture.
    pub fn put(&mut self, t: [u32; 4], px: &[u8]) {
        let [x, y, w, h] = t;
        let (px, w, h, x, y) = if self.factor == 2 {
            (downsample2(w, h, px), w / 2, h / 2, x / 2, y / 2)
        } else {
            (px.to_vec(), w, h, x, y)
        };
        let fw = self.size[0] as usize;
        for row in 0..h as usize {
            let dst = ((y as usize + row) * fw + x as usize) * 4;
            let src = row * w as usize * 4;
            self.pixels[dst..dst + w as usize * 4].copy_from_slice(&px[src..src + w as usize * 4]);
        }
    }

    pub fn finish(mut self) -> anyhow::Result<PathBuf> {
        if self.transparent {
            unpremultiply(&mut self.pixels);
        }
        let bytes = encode(self.kind, self.size[0], self.size[1], self.pixels, self.transparent, self.jpeg_quality)?;
        std::fs::write(&self.path, bytes)?;
        Ok(self.path)
    }
}

/// Tiles rendered per displayed frame.
const TILES_PER_TICK: usize = 2;

impl EsquisseApp {
    /// Renders the next tile of a raster export; false when the renderer failed.
    fn render_tile(&mut self, rs: &egui_wgpu::RenderState, job: &mut RasterJob) -> bool {
        let Some(&t) = job.tiles.get(job.next) else { return true };
        let full = [job.size[0] * job.factor, job.size[1] * job.factor];
        let ppp = (job.line_scale * job.factor as f64) as f32;
        let cam = tile_camera(&self.cam, full, t[0], t[1], t[2], t[3]);
        let keep = self.view.as_ref().map(|v| v.photo_bg);
        if let (Some(v), true) = (self.view.as_mut(), job.transparent) {
            v.photo_bg = true;
        }
        let r = self.render_rgba(rs, &cam, [t[2], t[3]], ppp);
        if let (Some(v), Some(k)) = (self.view.as_mut(), keep) {
            v.photo_bg = k;
        }
        match r {
            Some((w, h, px)) if [w, h] == [t[2], t[3]] => job.put(t, &px),
            _ => return false,
        }
        job.next += 1;
        true
    }

    /// A raster export in one call (for scripts): the current view, sized by `o` for a view of
    /// `view` pixels shown at `ppp` pixels per point, written to `path` as `kind`.
    pub fn export_raster(
        &mut self,
        rs: &egui_wgpu::RenderState,
        path: PathBuf,
        kind: Kind,
        o: &RasterOptions,
        view: [u32; 2],
        ppp: f64,
    ) -> anyhow::Result<PathBuf> {
        let (size, line_scale) = o.resolve(view, ppp);
        self.render_raster(rs, RasterJob::new(path, kind, o, size, line_scale))
    }

    /// Renders all the tiles of `job` at once and writes the file.
    pub fn render_raster(&mut self, rs: &egui_wgpu::RenderState, mut job: RasterJob) -> anyhow::Result<PathBuf> {
        let size = job.size;
        while job.next < job.tiles.len() {
            if !self.render_tile(rs, &mut job) {
                anyhow::bail!("le rendu de l'image a échoué ({} × {} pixels)", size[0], size[1]);
            }
        }
        job.finish()
    }

    /// Renders the next tiles of a raster export, and writes the file when all are done.
    pub fn step_export2d(&mut self, ctx: &egui::Context, rs: &egui_wgpu::RenderState) {
        let Some(mut job) = self.ui_state.extra.jobs.raster.take() else { return };
        let mut failed = false;
        for _ in 0..TILES_PER_TICK {
            if job.next >= job.tiles.len() {
                break;
            }
            if !self.render_tile(rs, &mut job) {
                failed = true;
                break;
            }
        }
        if failed {
            let msg = format!(
                "Un problème est survenu lors de l'exportation de {}.\n\nAvant de réexporter, essayez de réduire la hauteur et la largeur dans les options.",
                job.path.display()
            );
            self.say(ctx, msg);
            return;
        }
        if job.next < job.tiles.len() {
            self.ui_state.extra.jobs.raster = Some(job);
            ctx.request_repaint();
            return;
        }
        let msg = match job.finish() {
            Ok(p) => format!("Image exportée : {}", p.display()),
            Err(e) => format!("Échec de l'export de l'image : {e:#}"),
        };
        self.say(ctx, msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::glam::DVec3;

    #[test]
    fn size_follows_the_view_proportions() {
        let view = [1600, 900];
        let o = RasterOptions { use_view_size: false, width: 3200, ..Default::default() };
        assert_eq!(o.resolve(view, 1.25), ([3200, 1800], 1.0));
        let o = RasterOptions { use_view_size: true, width: 10, ..Default::default() };
        assert_eq!(o.resolve(view, 1.25), ([1600, 900], 1.25), "the view's size and display scale");
        let o = RasterOptions { use_view_size: false, width: 100, line_scale: 1000.0, ..Default::default() };
        assert_eq!(o.resolve(view, 1.0).1, MAX_LINE_SCALE);
        assert_eq!(width_for_height(view, 450), 800);
        let mut d = Export2dDialog::new(PathBuf::from("a.png"), Kind::Png, o, view, 1.0);
        d.height_text = "90".into();
        d.typed_height();
        assert_eq!((d.options.width, d.options.height), (160, 90));
        d.scale_text = "0.01".into();
        d.typed_scale();
        d.sync_texts();
        assert_eq!(d.options.line_scale, MIN_LINE_SCALE);
        assert_eq!(d.scale_text, "0,25");
        d.set_use_view_size(true);
        assert_eq!(d.width_text, "1600");
        // Typing a width moves the height at once.
        d.set_use_view_size(false);
        d.width_text = "3200".into();
        d.typed_width();
        assert_eq!((d.options.width, d.options.height, d.height_text.as_str()), (3200, 1800, "1800"));
        d.scale_text = "2,5".into();
        d.typed_scale();
        assert_eq!(d.options.line_scale, 2.5);
    }

    #[test]
    fn options_survive_the_preferences_file() {
        let o = RasterOptions {
            use_view_size: false,
            width: 4000,
            height: 2000,
            line_scale: 2.5,
            antialias: false,
            transparent: true,
            jpeg_quality: 70,
        };
        assert_eq!(RasterOptions::from_json(&o.to_json()), o);
        assert_eq!(RasterOptions::from_json(&Value::Null), RasterOptions::default());
        assert_eq!(RasterOptions::from_json(&json!({"jpeg_quality": 3})).jpeg_quality, JPEG_MIN);
    }

    #[test]
    fn tiles_cover_the_picture_once() {
        let t = tiles([5000, 3000], 2048);
        assert_eq!(t.len(), 6);
        let area: u64 = t.iter().map(|t| t[2] as u64 * t[3] as u64).sum();
        assert_eq!(area, 5000 * 3000);
        assert_eq!(t[2], [4096, 0, 904, 2048]);
    }

    #[test]
    fn a_tile_shows_its_part_of_the_picture() {
        for perspective in [true, false] {
            let mut cam = Camera {
                target: DVec3::new(500.0, 300.0, 200.0),
                distance: 5000.0,
                yaw: 0.7,
                pitch: 0.3,
                perspective,
                ..Default::default()
            };
            cam.lens_shift = DVec2::new(0.1, -0.05);
            let full = [3000u32, 2000u32];
            cam.size = DVec2::new(full[0] as f64, full[1] as f64);
            let tile = tile_camera(&cam, full, 2048, 1024, 952, 976);
            for p in [DVec3::new(0.0, 0.0, 0.0), DVec3::new(1000.0, 600.0, 400.0), DVec3::new(-300.0, 800.0, 50.0)] {
                let a = cam.project(p).unwrap();
                let b = tile.project(p).unwrap();
                assert!((a - DVec2::new(2048.0, 1024.0) - b).length() < 1e-6, "{perspective}: {a} {b}");
            }
        }
    }

    #[test]
    fn antialiasing_halves_and_transparency_unpremultiplies() {
        let px: Vec<u8> = [[255, 0, 0, 255], [0, 0, 0, 255], [255, 0, 0, 255], [0, 0, 0, 255]].concat();
        assert_eq!(downsample2(2, 2, &px), vec![128, 0, 0, 255]);
        let mut half = vec![100u8, 50, 0, 128];
        unpremultiply(&mut half);
        assert_eq!(half, vec![199, 100, 0, 128]);
        let mut job = RasterJob::new(PathBuf::new(), Kind::Png, &RasterOptions::default(), [3, 2], 1.0);
        assert_eq!(job.tiles, vec![[0, 0, 6, 4]]);
        let tile: Vec<u8> = (0..24).flat_map(|i| [i as u8 * 10, 0, 0, 255]).collect();
        job.put([0, 0, 6, 4], &tile);
        assert_eq!(job.pixels[..4], [35, 0, 0, 255]);
    }

    #[test]
    fn every_type_is_written() {
        let (w, h) = (4u32, 3u32);
        let px: Vec<u8> = (0..w * h).flat_map(|i| [i as u8 * 20, 100, 200, if i == 0 { 0 } else { 255 }]).collect();
        for k in Kind::ALL {
            let bytes = encode(k, w, h, px.clone(), true, 80).unwrap();
            // Targa files have no signature: say which format it is.
            let img = if k == Kind::Tga {
                image::load_from_memory_with_format(&bytes, image::ImageFormat::Tga)
            } else {
                image::load_from_memory(&bytes)
            }
            .unwrap_or_else(|e| panic!("{k:?}: {e}"));
            assert_eq!((img.width(), img.height()), (w, h), "{k:?}");
            assert_eq!(img.color().has_alpha(), k.has_alpha_option(), "{k:?}");
            if k != Kind::Jpeg {
                assert_eq!(img.to_rgba8().get_pixel(1, 0).0[..3], [20, 100, 200], "{k:?} is lossless");
            }
        }
        let opaque = encode(Kind::Png, w, h, px, false, 100).unwrap();
        assert!(!image::load_from_memory(&opaque).unwrap().color().has_alpha());
    }

    /// The one-call export renders the current view at the chosen size: 300 pixels wide for
    /// a 600 × 400 view gives 300 × 200, the face in the middle, the background around it.
    #[test]
    fn a_raster_export_runs_in_one_call() {
        let Some(rs) = crate::tests::gpu::render_state() else { return eprintln!("pas de GPU : test sauté") };
        let mut app = EsquisseApp::bare(Some(crate::render::Viewport3d::new(&rs)), None);
        let e = &mut app.doc.model.root;
        let v: Vec<_> = [(200.0, 200.0), (800.0, 200.0), (800.0, 800.0), (200.0, 800.0)]
            .iter()
            .map(|(x, y)| e.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
            .collect();
        e.add_face_raw(v, vec![]);
        let mut cam = crate::tests::gpu::top_camera(1000.0, 400.0);
        cam.size = esq_core::glam::DVec2::new(600.0, 400.0);
        app.cam = cam;
        let opts = crate::scene::SceneOptions {
            ctx: &[],
            ctx_def: None,
            sel: &[],
            axes_xform: esq_core::glam::DMat4::IDENTITY,
            style: &app.style,
        };
        let mesh = crate::scene::build(&app.doc.model, &opts);
        app.view.as_mut().unwrap().upload(&rs, &mesh);
        let path = std::env::temp_dir().join(format!("esq-raster-{}.png", std::process::id()));
        let o = RasterOptions { use_view_size: false, width: 300, antialias: true, ..Default::default() };
        let out = app.export_raster(&rs, path.clone(), Kind::Png, &o, [600, 400], 1.0).unwrap();
        let img = image::open(&out).unwrap().to_rgb8();
        let _ = std::fs::remove_file(&path);
        assert_eq!(img.dimensions(), (300, 200));
        let bg = app.style.bg_color;
        let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(x, y)| (*x as i32 - y as i32).abs() <= 3);
        assert!(near(img.get_pixel(5, 5).0, bg), "{:?}", img.get_pixel(5, 5));
        assert!(!near(img.get_pixel(150, 100).0, bg), "{:?}", img.get_pixel(150, 100));
    }
}
