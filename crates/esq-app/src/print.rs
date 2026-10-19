//! File › Print: the view rendered offscreen at the resolution of the page, then written as a
//! PDF or sent to a Windows printer. Like SketchUp, the view is printed either "fit to page"
//! (the window's framing, as large as the page allows) or at a scale 1:N (parallel projection
//! only: one millimetre on paper is N millimetres of the model).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use eframe::egui;

use crate::app::EsquisseApp;
use crate::camera::Camera;
use crate::ui::centered;

/// Paper sizes, in millimetres (portrait).
pub const PAPERS: [(&str, f64, f64); 9] = [
    ("A5", 148.0, 210.0),
    ("A4", 210.0, 297.0),
    ("A3", 297.0, 420.0),
    ("A2", 420.0, 594.0),
    ("A1", 594.0, 841.0),
    ("A0", 841.0, 1189.0),
    ("Lettre (Letter)", 215.9, 279.4),
    ("Légal (Legal)", 215.9, 355.6),
    ("Tabloïd (Tabloid)", 279.4, 431.8),
];

/// Scales offered in the dialog (1:N).
pub const SCALES: [f64; 9] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0];

/// Largest side of the rendered image, in pixels.
const MAX_PX: u32 = 8192;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PrintScale {
    /// The window's framing, as large as the page allows.
    Fit,
    /// 1:N (parallel projection).
    Ratio(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrintOptions {
    /// Index in `PAPERS`.
    pub paper: usize,
    pub landscape: bool,
    pub margin_mm: f64,
    pub dpi: u32,
    pub scale: PrintScale,
}

impl Default for PrintOptions {
    fn default() -> Self {
        PrintOptions { paper: 1, landscape: true, margin_mm: 10.0, dpi: 200, scale: PrintScale::Fit }
    }
}

/// Where the picture goes on the page and how big it is rendered.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// Page size, millimetres.
    pub page: (f64, f64),
    /// Picture rectangle on the page, millimetres, from the bottom-left corner: x, y, w, h.
    pub rect: (f64, f64, f64, f64),
    /// Picture size, pixels.
    pub px: (u32, u32),
}

impl PrintOptions {
    pub fn page_mm(&self) -> (f64, f64) {
        let (_, w, h) = PAPERS[self.paper.min(PAPERS.len() - 1)];
        if self.landscape { (h, w) } else { (w, h) }
    }

    /// `aspect` is the width / height of the window's 3D view.
    pub fn layout(&self, aspect: f64) -> Layout {
        let (pw, ph) = self.page_mm();
        let m = self.margin_mm.clamp(0.0, pw.min(ph) / 3.0);
        let (aw, ah) = (pw - 2.0 * m, ph - 2.0 * m);
        let (w, h) = match self.scale {
            PrintScale::Ratio(_) => (aw, ah),
            PrintScale::Fit => {
                if aw / ah > aspect {
                    (ah * aspect, ah)
                } else {
                    (aw, aw / aspect)
                }
            }
        };
        let rect = ((pw - w) / 2.0, (ph - h) / 2.0, w, h);
        let k = self.dpi.max(30) as f64 / 25.4;
        let k = k.min(MAX_PX as f64 / w.max(h));
        Layout { page: (pw, ph), rect, px: (((w * k).round() as u32).max(1), ((h * k).round() as u32).max(1)) }
    }

    /// The camera for the picture: the window's, resized, or set so that 1 mm of paper is N
    /// mm of the model.
    pub fn camera(&self, base: &Camera, layout: &Layout) -> Camera {
        let mut c = base.clone();
        c.size = esq_core::glam::DVec2::new(layout.px.0 as f64, layout.px.1 as f64);
        if let PrintScale::Ratio(n) = self.scale {
            c.perspective = false;
            c.distance = layout.rect.3 * n / (2.0 * (c.fov_y * 0.5).tan());
        }
        c
    }
}

/// A one-page PDF with the picture (RGBA, 8 bits) at `layout.rect`.
pub fn pdf_bytes(layout: &Layout, w: u32, h: u32, rgba: &[u8], title: &str) -> Vec<u8> {
    let pt = |mm: f64| mm * 72.0 / 25.4;
    let rgb: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let data = miniz_oxide::deflate::compress_to_vec_zlib(&rgb, 6);
    let (x, y, rw, rh) = layout.rect;
    let content = format!("q {:.3} 0 0 {:.3} {:.3} {:.3} cm /Im0 Do Q\n", pt(rw), pt(rh), pt(x), pt(y));
    let title: String = title.chars().filter(|c| c.is_ascii() && !"()\\".contains(*c)).collect();
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    let mut obj = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    obj(
        &mut out,
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.3} {:.3}] /Contents 4 0 R /Resources << /XObject << /Im0 5 0 R >> >> >>",
            pt(layout.page.0),
            pt(layout.page.1)
        )
        .as_bytes(),
    );
    let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    stream.extend_from_slice(content.as_bytes());
    stream.extend_from_slice(b"endstream");
    obj(&mut out, &stream);
    let mut img = format!(
        "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
        data.len()
    )
    .into_bytes();
    img.extend_from_slice(&data);
    img.extend_from_slice(b"\nendstream");
    obj(&mut out, &img);
    obj(&mut out, format!("<< /Title ({title}) /Producer (Esquisse) >>").as_bytes());
    let xref = out.len();
    let _ = write!(out, "xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
    for o in &offsets {
        let _ = writeln!(out, "{o:010} 00000 n ");
    }
    let _ =
        write!(out, "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1);
    out
}

/// PowerShell that prints a PNG on a Windows printer, picture placed as in `layout`.
/// `printer` empty = the default printer. `to_file` prints to a file (printers such as
/// "Microsoft Print to PDF").
pub fn print_script(png: &Path, layout: &Layout, printer: &str, to_file: Option<&Path>) -> String {
    let hi = |mm: f64| (mm / 25.4 * 100.0).round() as i64; // hundredths of an inch
    let (pw, ph) = (hi(layout.page.0), hi(layout.page.1));
    let (x, y, w, h) = layout.rect;
    // The rectangle is measured from the bottom edge; GDI+ counts from the top.
    let (rx, ry, rw, rh) = (hi(x), hi(layout.page.1 - y - h), hi(w), hi(h));
    let q = |s: &str| s.replace('\'', "''");
    let mut s = String::from("$ErrorActionPreference = 'Stop'\nAdd-Type -AssemblyName System.Drawing\n");
    s += &format!("$img = [System.Drawing.Image]::FromFile('{}')\n", q(&png.to_string_lossy()));
    s += "$pd = New-Object System.Drawing.Printing.PrintDocument\n";
    if !printer.is_empty() {
        s += &format!("$pd.PrinterSettings.PrinterName = '{}'\n", q(printer));
    }
    if let Some(f) = to_file {
        s += &format!(
            "$pd.PrinterSettings.PrintToFile = $true\n$pd.PrinterSettings.PrintFileName = '{}'\n",
            q(&f.to_string_lossy())
        );
    }
    s += "$pd.DocumentName = 'Esquisse'\n$pd.OriginAtMargins = $false\n";
    s += &format!(
        "$pd.DefaultPageSettings.PaperSize = New-Object System.Drawing.Printing.PaperSize('Esquisse', {pw}, {ph})\n"
    );
    s += "$pd.DefaultPageSettings.Margins = New-Object System.Drawing.Printing.Margins(0, 0, 0, 0)\n";
    s += &format!(
        "$pd.add_PrintPage({{ param($sender, $e) $e.Graphics.DrawImage($img, {rx}, {ry}, {rw}, {rh}); $e.HasMorePages = $false }})\n"
    );
    s += "$pd.Print()\n$img.Dispose()\n";
    s
}

/// Names of the installed printers (empty when PowerShell is unavailable).
pub fn printers() -> Vec<String> {
    Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Add-Type -AssemblyName System.Drawing; [System.Drawing.Printing.PrinterSettings]::InstalledPrinters",
        ])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
        })
        .unwrap_or_default()
}

fn run_script(script: &str) -> Result<(), String> {
    let path = std::env::temp_dir().join("esquisse_print.ps1");
    // A BOM makes Windows PowerShell 5.1 read the file as UTF-8.
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(script.as_bytes());
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let out = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&path)
        .output()
        .map_err(|e| format!("PowerShell ne démarre pas : {e}"))?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

/// Where the picture goes once rendered.
#[derive(Clone, Debug)]
pub enum Destination {
    Pdf(PathBuf),
    Printer(String),
}

/// A print waiting for the next frame to be rendered.
pub struct PrintJob {
    pub options: PrintOptions,
    pub dest: Destination,
}

/// Renders and delivers the page; returns the message to show.
pub fn deliver(job: &PrintJob, layout: &Layout, w: u32, h: u32, rgba: &[u8], title: &str) -> String {
    match &job.dest {
        Destination::Pdf(p) => match std::fs::write(p, pdf_bytes(layout, w, h, rgba, title)) {
            Ok(()) => format!("PDF enregistré : {}", p.display()),
            Err(e) => format!("Échec de l'enregistrement du PDF : {e}"),
        },
        Destination::Printer(name) => {
            let png = std::env::temp_dir().join("esquisse_print.png");
            if let Err(e) = crate::png::write_rgba(&png, w, h, rgba) {
                return format!("Échec de l'impression : {e}");
            }
            match run_script(&print_script(&png, layout, name, None)) {
                Ok(()) => "Page envoyée à l'imprimante".to_string(),
                Err(e) => format!("Échec de l'impression : {e}"),
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct PrintDialog {
    pub options: PrintOptions,
    pub printers: Vec<String>,
    pub printer: usize,
}

impl Default for PrintDialog {
    fn default() -> Self {
        PrintDialog { options: PrintOptions::default(), printers: printers(), printer: 0 }
    }
}

pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(mut dlg) = app.ui_state.extra.print.take() else { return };
    let mut action = None;
    let perspective = app.cam.perspective;
    let aspect = app.cam.aspect();
    centered("Imprimer").show(ctx, |ui| {
        let o = &mut dlg.options;
        egui::Grid::new("print_opts").num_columns(2).show(ui, |ui| {
            ui.label("Papier");
            egui::ComboBox::from_id_salt("print_paper").selected_text(PAPERS[o.paper].0).show_ui(ui, |ui| {
                for (i, p) in PAPERS.iter().enumerate() {
                    ui.selectable_value(&mut o.paper, i, p.0);
                }
            });
            ui.end_row();
            ui.label("Orientation");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut o.landscape, false, "Portrait");
                ui.selectable_value(&mut o.landscape, true, "Paysage");
            });
            ui.end_row();
            ui.label("Marges (mm)");
            ui.add(egui::DragValue::new(&mut o.margin_mm).range(0.0..=50.0));
            ui.end_row();
            ui.label("Résolution (ppp)");
            egui::ComboBox::from_id_salt("print_dpi").selected_text(o.dpi.to_string()).show_ui(ui, |ui| {
                for d in [100, 150, 200, 300, 600] {
                    ui.selectable_value(&mut o.dpi, d, d.to_string());
                }
            });
            ui.end_row();
            ui.label("Échelle");
            ui.horizontal(|ui| {
                let mut fit = o.scale == PrintScale::Fit;
                if ui.checkbox(&mut fit, "Ajuster à la page").changed() {
                    o.scale = if fit { PrintScale::Fit } else { PrintScale::Ratio(SCALES[3]) };
                }
                if let PrintScale::Ratio(n) = &mut o.scale {
                    egui::ComboBox::from_id_salt("print_scale").selected_text(format!("1:{n}")).show_ui(ui, |ui| {
                        for s in SCALES {
                            ui.selectable_value(n, s, format!("1:{s}"));
                        }
                    });
                }
            });
            ui.end_row();
        });
        if matches!(o.scale, PrintScale::Ratio(_)) && perspective {
            ui.colored_label(
                egui::Color32::from_rgb(180, 90, 0),
                "L'échelle impose la projection parallèle : elle sera appliquée à l'impression.",
            );
        }
        let l = o.layout(aspect);
        ui.label(format!("Image de {} × {} px sur {:.0} × {:.0} mm.", l.px.0, l.px.1, l.page.0, l.page.1));
        if !dlg.printers.is_empty() {
            let current = dlg.printers.get(dlg.printer).cloned().unwrap_or_default();
            egui::ComboBox::from_label("Imprimante").selected_text(current).show_ui(ui, |ui| {
                for (i, p) in dlg.printers.iter().enumerate() {
                    ui.selectable_value(&mut dlg.printer, i, p);
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Enregistrer en PDF...").clicked() {
                action = Some(1);
            }
            if ui.add_enabled(!dlg.printers.is_empty(), egui::Button::new("Imprimer")).clicked() {
                action = Some(2);
            }
            if ui.button("Fermer").clicked() {
                action = Some(0);
            }
        });
    });
    match action {
        Some(0) => return,
        Some(1) => {
            if let Some(p) = crate::settings::dialog("export")
                .add_filter("Document PDF", &["pdf"])
                .set_file_name("vue.pdf")
                .save_file()
            {
                app.ui_state.extra.jobs.print =
                    Some(PrintJob { options: dlg.options.clone(), dest: Destination::Pdf(p) });

                ctx.request_repaint();
            }
        }
        Some(2) => {
            let name = dlg.printers.get(dlg.printer).cloned().unwrap_or_default();
            app.ui_state.extra.jobs.print =
                Some(PrintJob { options: dlg.options.clone(), dest: Destination::Printer(name) });

            ctx.request_repaint();
        }
        _ => {}
    }
    app.ui_state.extra.print = Some(dlg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_the_window_framing_inside_the_margins() {
        let o = PrintOptions { paper: 1, landscape: true, margin_mm: 10.0, dpi: 254, scale: PrintScale::Fit };
        // A4 landscape 297 x 210 minus margins: 277 x 190. A 2:1 view is limited by the width.
        let l = o.layout(2.0);
        assert_eq!(l.page, (297.0, 210.0));
        assert!((l.rect.2 - 277.0).abs() < 1e-9 && (l.rect.3 - 138.5).abs() < 1e-9, "{l:?}");
        assert!((l.rect.0 - 10.0).abs() < 1e-9 && (l.rect.1 - 35.75).abs() < 1e-9, "centred");
        assert_eq!(l.px, (2770, 1385), "254 dpi = 10 px per mm");
        // A tall view is limited by the height.
        let l = o.layout(0.5);
        assert!((l.rect.3 - 190.0).abs() < 1e-9 && (l.rect.2 - 95.0).abs() < 1e-9);
        let p = PrintOptions { landscape: false, ..o };
        assert_eq!(p.page_mm(), (210.0, 297.0));
    }

    #[test]
    fn huge_pages_are_limited_in_pixels() {
        let o = PrintOptions { paper: 5, landscape: false, margin_mm: 0.0, dpi: 600, scale: PrintScale::Fit };
        let l = o.layout(841.0 / 1189.0);
        assert!(l.px.0 <= MAX_PX && l.px.1 <= MAX_PX && l.px.1 >= MAX_PX - 1, "{l:?}");
    }

    #[test]
    fn scale_sets_the_orthographic_camera() {
        let o = PrintOptions { scale: PrintScale::Ratio(50.0), margin_mm: 10.0, ..Default::default() };
        let l = o.layout(1.7);
        assert_eq!((l.rect.2, l.rect.3), (277.0, 190.0), "fills the area");
        let c = o.camera(&Camera::default(), &l);
        assert!(!c.perspective);
        // The view is 190 mm tall on paper = 9500 mm of the model.
        let (a, b) =
            (c.ray(esq_core::glam::DVec2::new(0.0, 0.0)).0, c.ray(esq_core::glam::DVec2::new(0.0, c.size.y)).0);
        assert!((a.distance(b) - 9500.0).abs() < 1e-6, "{}", a.distance(b));
        assert_eq!((c.size.x as u32, c.size.y as u32), l.px);
    }

    #[test]
    fn pdf_is_well_formed() {
        let o = PrintOptions::default();
        let l = o.layout(1.5);
        let (w, h) = (8u32, 6u32);
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [i as u8, 100, 200, 255]).collect();
        let pdf = pdf_bytes(&l, w, h, &rgba, "Vue (test)");
        assert!(pdf.starts_with(b"%PDF-1.4"));
        let text: String = pdf.iter().map(|&b| if b < 128 { b as char } else { '?' }).collect();
        assert!(text.contains("/MediaBox [0 0 841.890 595.276]"), "A4 landscape in points");
        assert!(text.trim_end().ends_with("%%EOF"));
        // Every xref offset points at its "N 0 obj".
        let xref = text.rfind("\nxref\n").unwrap() + 1;
        let entries: Vec<&str> = text[xref..].lines().skip(3).take(6).collect();
        assert_eq!(entries.len(), 6);
        for (i, e) in entries.iter().enumerate() {
            let off: usize = e[..10].parse().unwrap();
            assert!(pdf[off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()), "object {}", i + 1);
        }
        let start: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert_eq!(start, xref);
        // The image stream inflates back to the RGB pixels.
        let key = b"/Filter /FlateDecode /Length ";
        let at = pdf.windows(key.len()).position(|w| w == key).unwrap() + key.len();
        let len: usize = text[at..].split_whitespace().next().unwrap().parse().unwrap();
        let body = text[at..].find("stream\n").unwrap() + at + 7;
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&pdf[body..body + len]).unwrap();
        assert_eq!(raw.len(), (w * h * 3) as usize);
        assert_eq!(&raw[..3], &[0, 100, 200]);
    }

    #[test]
    fn print_script_places_the_picture_from_the_top() {
        let l = Layout { page: (297.0, 210.0), rect: (10.0, 35.75, 277.0, 138.5), px: (10, 10) };
        let s = print_script(Path::new("C:\\t\\it's.png"), &l, "Mon imprimante", None);
        assert!(s.contains("PaperSize('Esquisse', 1169, 827)"), "{s}");
        assert!(s.contains("DrawImage($img, 39, 141, 1091, 545)"), "{s}");
        assert!(s.contains("it''s.png") && s.contains("PrinterName = 'Mon imprimante'"));
    }

    #[test]
    fn printing_to_microsoft_pdf_writes_a_file() {
        if !printers().iter().any(|p| p == "Microsoft Print to PDF") {
            eprintln!("« Microsoft Print to PDF » absente : test ignoré");
            return;
        }
        // A folder of this process: other copies of the repository run this test at the same
        // time (a shared folder had one test write the picture another was printing).
        let dir = std::env::temp_dir().join(format!("esq_print_test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("p.png");
        let rgba: Vec<u8> = (0..64 * 48).flat_map(|i| [(i % 64) as u8 * 4, 50, 150, 255]).collect();
        crate::png::write_rgba(&png, 64, 48, &rgba).unwrap();
        let out = dir.join("out.pdf");
        let l = PrintOptions::default().layout(64.0 / 48.0);
        run_script(&print_script(&png, &l, "Microsoft Print to PDF", Some(&out))).unwrap();
        // The spooler may still hold the file a moment after the job is handed over.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let bytes = loop {
            match std::fs::read(&out) {
                Ok(b) if b.ends_with(b"%%EOF\r\n") || b.ends_with(b"%%EOF\n") || b.ends_with(b"%%EOF") => break b,
                r if std::time::Instant::now() > deadline => panic!("the printer wrote no whole file: {r:?}"),
                _ => std::thread::sleep(std::time::Duration::from_millis(200)),
            }
        };
        assert!(bytes.starts_with(b"%PDF"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
