//! File › Export › Animation: the scenes played as a video (MP4 or GIF through ffmpeg) or as
//! a numbered sequence of PNG images. The timeline follows View › Animation › Settings: the
//! first scene is held for the delay, then each next scene is reached by a smooth camera
//! move of the transition time and held for the delay. Frames are rendered offscreen by
//! `offscreen.rs`, a few per displayed frame, so the window keeps answering.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use eframe::egui;

use crate::app::EsquisseApp;
use crate::filedlg::FileType;
use crate::ui::centered;

/// What one exported frame shows: the camera between scene `from` and scene `to`, `t` of the way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameSpec {
    pub from: usize,
    pub to: usize,
    pub t: f64,
}

/// The frames of the animation. `transition` and `delay` are in seconds. With `looped`, the
/// last scene moves back to the first one (no pause on arrival: the video loops there).
pub fn plan(scenes: usize, transition: f64, delay: f64, fps: f64, looped: bool) -> Vec<FrameSpec> {
    let mut out = Vec::new();
    if scenes == 0 {
        return out;
    }
    let frames = |secs: f64| (secs * fps).round().max(0.0) as usize;
    let hold = |out: &mut Vec<FrameSpec>, i: usize| {
        out.extend((0..frames(delay).max(1)).map(|_| FrameSpec { from: i, to: i, t: 1.0 }));
    };
    let mv = |out: &mut Vec<FrameSpec>, from: usize, to: usize| {
        let n = frames(transition);
        out.extend((1..=n).map(|k| FrameSpec { from, to, t: k as f64 / n as f64 }));
    };
    hold(&mut out, 0);
    for i in 1..scenes {
        mv(&mut out, i - 1, i);
        hold(&mut out, i);
    }
    if looped && scenes > 1 {
        mv(&mut out, scenes - 1, 0);
    }
    out
}

/// The camera of a frame: the scenes' points of view blended like the on-screen transition.
pub fn camera_for(
    scenes: &[esq_core::model::Scene],
    base: &crate::camera::Camera,
    spec: &FrameSpec,
    width: u32,
    height: u32,
) -> crate::camera::Camera {
    let of = |i: usize| {
        let mut c = base.clone();
        if let Some(s) = scenes.get(i) {
            c.set_from_view(&s.camera);
        }
        c
    };
    let mut c = crate::scenes::lerp_cam(&of(spec.from), &of(spec.to), spec.t);
    c.size = esq_core::glam::DVec2::new(width as f64, height as f64);
    c
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimFormat {
    Mp4,
    Gif,
    Png,
}

impl AnimFormat {
    pub fn of_path(p: &Path) -> AnimFormat {
        match p.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() {
            Some("mp4") => AnimFormat::Mp4,
            Some("gif") => AnimFormat::Gif,
            _ => AnimFormat::Png,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimOptions {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub looped: bool,
}

impl Default for AnimOptions {
    fn default() -> Self {
        AnimOptions { width: 1280, height: 720, fps: 24, looped: false }
    }
}

/// Sizes offered in the dialog.
pub const SIZES: [(&str, u32, u32); 4] = [
    ("480p (854 × 480)", 854, 480),
    ("720p (1280 × 720)", 1280, 720),
    ("1080p (1920 × 1080)", 1920, 1080),
    ("Carré (1080 × 1080)", 1080, 1080),
];

/// ffmpeg: `ESQUISSE_FFMPEG`, the PATH, then the WinGet package folder.
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ESQUISSE_FFMPEG") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let works = |p: &Path| {
        Command::new(p).arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    };
    if works(Path::new("ffmpeg")) {
        return Some(PathBuf::from("ffmpeg"));
    }
    let base = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Microsoft").join("WinGet").join("Packages");
    for pkg in std::fs::read_dir(base).ok()?.flatten() {
        if !pkg.file_name().to_string_lossy().starts_with("Gyan.FFmpeg") {
            continue;
        }
        for build in std::fs::read_dir(pkg.path()).ok()?.flatten() {
            let exe = build.path().join("bin").join("ffmpeg.exe");
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

/// ffmpeg's arguments for raw RGBA frames on stdin.
pub fn ffmpeg_args(format: AnimFormat, w: u32, h: u32, fps: u32, out: &Path) -> Vec<String> {
    let mut a: Vec<String> = ["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba", "-s"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    a.push(format!("{w}x{h}"));
    a.extend(["-r".to_string(), fps.to_string(), "-i".to_string(), "-".to_string()]);
    match format {
        AnimFormat::Gif => {
            a.extend(["-vf", "split[a][b];[a]palettegen[p];[b][p]paletteuse", "-loop", "0"].map(String::from));
        }
        _ => {
            a.extend(
                ["-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "+faststart", "-crf", "18"].map(String::from),
            );
        }
    }
    a.push(out.to_string_lossy().into_owned());
    a
}

/// Where the frames go.
pub enum Sink {
    Ffmpeg {
        child: Child,
        stdin: Option<ChildStdin>,
    },
    /// `<stem>_0001.png`, `<stem>_0002.png`… in `dir`.
    Png {
        dir: PathBuf,
        stem: String,
        count: usize,
    },
}

impl Sink {
    pub fn open(format: AnimFormat, o: &AnimOptions, path: &Path, ffmpeg: Option<&Path>) -> Result<Sink, String> {
        let png = |path: &Path| Sink::Png {
            dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            stem: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "animation".into()),
            count: 0,
        };
        if format == AnimFormat::Png {
            return Ok(png(path));
        }
        let Some(ff) = ffmpeg else { return Ok(png(path)) };
        let mut child = Command::new(ff)
            .args(ffmpeg_args(format, o.width, o.height, o.fps, path))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("ffmpeg ne démarre pas : {e}"))?;
        let stdin = child.stdin.take();
        Ok(Sink::Ffmpeg { child, stdin })
    }

    pub fn is_sequence(&self) -> bool {
        matches!(self, Sink::Png { .. })
    }

    pub fn push(&mut self, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
        match self {
            Sink::Ffmpeg { stdin, .. } => stdin
                .as_mut()
                .ok_or("ffmpeg est fermé")?
                .write_all(rgba)
                .map_err(|e| format!("écriture vers ffmpeg impossible : {e}")),
            Sink::Png { dir, stem, count } => {
                *count += 1;
                crate::png::write_rgba(&dir.join(format!("{stem}_{count:04}.png")), w, h, rgba)
                    .map_err(|e| format!("écriture de l'image impossible : {e}"))
            }
        }
    }

    /// Closes the sink; for a video, waits for ffmpeg to finish writing the file.
    pub fn finish(self) -> Result<(), String> {
        match self {
            Sink::Png { .. } => Ok(()),
            Sink::Ffmpeg { child, stdin } => {
                drop(stdin);
                let out = child.wait_with_output().map_err(|e| format!("ffmpeg : {e}"))?;
                if out.status.success() {
                    Ok(())
                } else {
                    Err(format!("ffmpeg a échoué : {}", String::from_utf8_lossy(&out.stderr).trim()))
                }
            }
        }
    }

    /// Gives up: kills ffmpeg.
    pub fn abort(self) {
        if let Sink::Ffmpeg { mut child, stdin } = self {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// An export in progress.
pub struct AnimJob {
    pub plan: Vec<FrameSpec>,
    pub next: usize,
    pub options: AnimOptions,
    pub path: PathBuf,
    pub sink: Sink,
    /// Tag visibility before the export, put back at the end.
    pub saved_layers: Vec<bool>,
    /// Scene whose tags are applied.
    pub layers_scene: Option<usize>,
    pub cancel: bool,
}

#[derive(Clone, Debug, Default)]
pub struct AnimDialog {
    pub options: AnimOptions,
}

pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    if let Some(job) = &mut app.ui_state.extra.jobs.anim {
        let mut cancel = false;
        centered("Export de l'animation").show(ctx, |ui| {
            ui.add(egui::ProgressBar::new(job.next as f32 / job.plan.len().max(1) as f32).text(format!(
                "Image {} sur {}",
                job.next,
                job.plan.len()
            )));
            cancel = ui.button("Annuler").clicked();
        });
        job.cancel |= cancel;
        return;
    }
    let Some(mut dlg) = app.ui_state.extra.anim.take() else { return };
    let mut action = None;
    centered("Exporter l'animation").show(ctx, |ui| {
        options_ui(ui, app, &mut dlg.options);
        let scenes = app.doc.model.scenes.len();
        ui.horizontal(|ui| {
            if ui.add_enabled(scenes > 0, egui::Button::new("Exporter...")).clicked() {
                action = Some(true);
            }
            if ui.button("Fermer").clicked() {
                action = Some(false);
            }
        });
    });
    match action {
        Some(false) => return,
        Some(true) => {
            if let Some(msg) = start_export(app, &dlg.options) {
                app.say(ctx, msg);
            }
            if app.ui_state.extra.jobs.anim.is_some() {
                return;
            }
        }
        None => {}
    }
    app.ui_state.extra.anim = Some(dlg);
}

/// The options of the animation export ("Options d'exportation d'animation"): size, frame
/// rate, loop, and what the scenes give.
pub fn options_ui(ui: &mut egui::Ui, app: &EsquisseApp, o: &mut AnimOptions) {
    let scenes = app.doc.model.scenes.len();
    let (transition, delay) = (app.anim.transition, app.anim.delay);
    egui::Grid::new("anim_opts").num_columns(2).show(ui, |ui| {
        ui.label("Taille");
        let current = SIZES.iter().find(|(_, w, h)| (*w, *h) == (o.width, o.height)).map(|s| s.0);
        egui::ComboBox::from_id_salt("anim_size").selected_text(current.unwrap_or("Personnalisée")).show_ui(ui, |ui| {
            for (name, w, h) in SIZES {
                if ui.selectable_label(current == Some(name), name).clicked() {
                    (o.width, o.height) = (w, h);
                }
            }
        });
        ui.end_row();
        ui.label("Largeur × hauteur (px)");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut o.width).range(64..=4096));
            ui.label("×");
            ui.add(egui::DragValue::new(&mut o.height).range(64..=4096));
        });
        ui.end_row();
        ui.label("Images par seconde");
        ui.add(egui::DragValue::new(&mut o.fps).range(1..=60));
        ui.end_row();
        ui.label("Boucler");
        ui.checkbox(&mut o.looped, "Revenir à la première scène");
        ui.end_row();
    });
    let frames = plan(scenes, transition, delay, o.fps as f64, o.looped).len();
    ui.label(format!(
        "{scenes} scène(s), transition {transition:.1} s, pause {delay:.1} s : {frames} images ({:.1} s).",
        frames as f64 / o.fps as f64
    ));
    if find_ffmpeg().is_none() {
        ui.colored_label(
            egui::Color32::from_rgb(180, 90, 0),
            "ffmpeg est introuvable : l'animation sera écrite en images PNG.",
        );
    }
}

/// The types of File › Export › Animation.
pub fn types() -> Vec<FileType> {
    vec![
        FileType::new("Vidéo MP4 H.264", &["mp4"]),
        FileType::new("GIF animé", &["gif"]),
        FileType::new("Séquence d'images PNG", &["png"]),
    ]
}

/// File › Export › Animation (`CExportMovieDialog`): the types, "Options..." always, OK
/// "Exporter".
pub fn request() -> crate::filedlg::Request {
    crate::filedlg::Request::save("Exporter l’animation", "animation", types(), 0, "export")
        .ok_label("Exporter")
        .options(|_, _| crate::filedlg::Controls { options: true, image: false })
}

/// File › Export › Animation: SketchUp's dialog (Windows), else the options window first.
pub fn start(app: &mut EsquisseApp, ctx: &egui::Context) {
    if !crate::file_flow::open(app, ctx, crate::file_flow::Purpose::Anim) {
        app.ui_state.extra.anim = Some(AnimDialog { options: app.ui_state.extra.files.anim.clone() });
    }
}

/// Asks for a file and starts the job; returns a message only when it fails to start.
fn start_export(app: &mut EsquisseApp, o: &AnimOptions) -> Option<String> {
    let c = crate::filedlg::run(request())?;
    app.ui_state.extra.files.anim = o.clone();
    start_job(app, o, c.path)
}

/// Starts the export of the scenes to `path`; the format follows the extension. Returns a
/// message when it cannot start.
pub fn start_job(app: &mut EsquisseApp, o: &AnimOptions, path: PathBuf) -> Option<String> {
    let mut o = o.clone();
    // H.264 wants even sizes.
    o.width &= !1;
    o.height &= !1;
    let format = AnimFormat::of_path(&path);
    let ffmpeg = find_ffmpeg();
    let sink = match Sink::open(format, &o, &path, ffmpeg.as_deref()) {
        Ok(s) => s,
        Err(e) => return Some(e),
    };
    let plan = plan(app.doc.model.scenes.len(), app.anim.transition, app.anim.delay, o.fps as f64, o.looped);
    if plan.is_empty() {
        sink.abort();
        return Some("Aucune scène à exporter.".into());
    }
    app.playing = None;
    app.transition = None;
    app.ui_state.extra.jobs.anim = Some(AnimJob {
        plan,
        next: 0,
        options: o,
        path,
        sink,
        saved_layers: app.doc.model.layers.iter().map(|l| l.visible).collect(),
        layers_scene: None,
        cancel: false,
    });
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_holds_then_moves() {
        // 3 scenes, 2 s moves, 1 s pauses, 10 fps.
        let p = plan(3, 2.0, 1.0, 10.0, false);
        assert_eq!(p.len(), 10 + 2 * (20 + 10));
        assert_eq!(p[0], FrameSpec { from: 0, to: 0, t: 1.0 });
        assert_eq!(p[10], FrameSpec { from: 0, to: 1, t: 0.05 });
        assert_eq!(p[29], FrameSpec { from: 0, to: 1, t: 1.0 });
        assert_eq!(p.last().unwrap().to, 2);
        // Looping adds the way back, ending exactly on the first scene.
        let l = plan(3, 2.0, 1.0, 10.0, true);
        assert_eq!(l.len(), p.len() + 20);
        assert_eq!(*l.last().unwrap(), FrameSpec { from: 2, to: 0, t: 1.0 });
    }

    #[test]
    fn plan_edge_cases() {
        assert!(plan(0, 2.0, 1.0, 24.0, true).is_empty());
        // One scene: just a pause, even looped.
        assert_eq!(plan(1, 2.0, 1.0, 24.0, true).len(), 24);
        // No transition: cuts; no pause: one frame per scene.
        let p = plan(2, 0.0, 0.0, 24.0, false);
        assert_eq!(p, vec![FrameSpec { from: 0, to: 0, t: 1.0 }, FrameSpec { from: 1, to: 1, t: 1.0 }]);
    }

    #[test]
    fn frame_cameras_go_from_scene_to_scene() {
        use esq_core::glam::DVec3;
        let view = |x: f64| esq_core::model::SceneCamera {
            eye: DVec3::new(x, -5000.0, 1000.0),
            target: DVec3::new(x, 0.0, 0.0),
            up: DVec3::Z,
            fov_deg: 35.0,
            perspective: true,
        };
        let scene = |x| esq_core::model::Scene {
            name: "s".into(),
            description: String::new(),
            camera: view(x),
            hidden_layers: None,
            photo: None,
            ext: Default::default(),
        };
        let scenes = [scene(0.0), scene(1000.0)];
        let base = crate::camera::Camera::default();
        let at = |t| camera_for(&scenes, &base, &FrameSpec { from: 0, to: 1, t }, 640, 360);
        assert!(at(0.0).target.distance(DVec3::ZERO) < 1e-6);
        assert!(at(1.0).target.distance(DVec3::new(1000.0, 0.0, 0.0)) < 1e-6);
        let mid = at(0.5);
        assert!((mid.target.x - 500.0).abs() < 1e-6, "smoothstep is 0.5 halfway");
        assert_eq!((mid.size.x, mid.size.y), (640.0, 360.0));
    }

    #[test]
    fn format_follows_extension() {
        assert_eq!(AnimFormat::of_path(Path::new("a.MP4")), AnimFormat::Mp4);
        assert_eq!(AnimFormat::of_path(Path::new("a.gif")), AnimFormat::Gif);
        assert_eq!(AnimFormat::of_path(Path::new("a.png")), AnimFormat::Png);
        let a = ffmpeg_args(AnimFormat::Mp4, 64, 48, 12, Path::new("o.mp4"));
        assert!(a.windows(2).any(|w| w == ["-s", "64x48"]) && a.contains(&"libx264".to_string()));
        let a = ffmpeg_args(AnimFormat::Gif, 64, 48, 12, Path::new("o.gif"));
        assert!(a.iter().any(|s| s.contains("palettegen")));
    }

    fn frames(n: usize, w: u32, h: u32) -> Vec<Vec<u8>> {
        (0..n).map(|k| (0..w * h).flat_map(|i| [(i % w) as u8, (k * 20) as u8, (i / w) as u8, 255]).collect()).collect()
    }

    #[test]
    fn png_sequence_sink() {
        let dir = std::env::temp_dir().join("esq_anim_png");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let o = AnimOptions { width: 16, height: 8, ..Default::default() };
        let mut s = Sink::open(AnimFormat::Png, &o, &dir.join("seq.png"), None).unwrap();
        assert!(s.is_sequence());
        for f in frames(3, 16, 8) {
            s.push(16, 8, &f).unwrap();
        }
        s.finish().unwrap();
        assert!(dir.join("seq_0001.png").is_file() && dir.join("seq_0003.png").is_file());
    }

    #[test]
    fn ffmpeg_writes_mp4_and_gif() {
        let Some(ff) = find_ffmpeg() else {
            eprintln!("ffmpeg absent : test ignoré");
            return;
        };
        let dir = std::env::temp_dir().join("esq_anim_ff");
        std::fs::create_dir_all(&dir).unwrap();
        let o = AnimOptions { width: 64, height: 48, fps: 12, looped: false };
        for (name, magic_at, magic) in [("t.mp4", 4, &b"ftyp"[..]), ("t.gif", 0, &b"GIF89a"[..])] {
            let out = dir.join(name);
            let _ = std::fs::remove_file(&out);
            let mut s = Sink::open(AnimFormat::of_path(&out), &o, &out, Some(&ff)).unwrap();
            assert!(!s.is_sequence());
            for f in frames(12, 64, 48) {
                s.push(64, 48, &f).unwrap();
            }
            s.finish().unwrap();
            let bytes = std::fs::read(&out).unwrap();
            assert!(bytes.len() > 100 && &bytes[magic_at..magic_at + magic.len()] == magic, "{name}");
        }
    }
}
