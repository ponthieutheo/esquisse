//! Banc de mesure sur de vrais gros modèles.
//!
//! Corpus : `ESQ_SKP_REEL`, sinon `%USERPROFILE%\tools\skp-reel` (`tools/skp-reel.ps1`).
//! Les 20 fichiers mesurés sont `FICHIERS` (choisis par `scan`), ou `ESQ_BENCH_FILES`
//! (chemins séparés par `;`). En version optimisée, sans autre compilation en cours :
//!
//! ```text
//! cargo test --release -p esq-app --bin esquisse tests::bench::corpus -- --ignored --nocapture
//! ```
//!
//! Chaque fichier est mesuré dans un processus à lui (la mémoire de pointe est celle de ce
//! processus seul), `ESQ_BENCH_REPEAT` fois (2 par défaut) : la meilleure des répétitions est
//! gardée pour chaque temps. `ESQ_BENCH_SHOTS=<dossier>` y écrit l'image de la vue à
//! l'ouverture et après « Tout sélectionner » (comparaison avant/après : `compare`), avec la
//! signature des inférences. `ESQ_BENCH_MAX_MB` (6 000 par défaut) arrête les 50 opérations
//! annulables quand le processus dépasse cette mémoire : la machine sert à d'autres.

use std::path::{Path, PathBuf};
use std::time::Instant;

use eframe::egui::{self, Modifiers};
use eframe::egui_wgpu::{self, wgpu};
use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::model::{EntityRef, Model};

use crate::app::EsquisseApp;
use crate::pick::InferCx;
use crate::render::Viewport3d;
use crate::tools::{Overlay, ToolId};

/// Taille de la vue mesurée (pixels, à l'échelle 1).
const W: u32 = 1280;
const H: u32 = 800;
/// Images d'orbite mesurées (après 3 de mise en route), pixels tournés par image.
const ORBIT_FRAMES: usize = 60;
const ORBIT_STEP: f64 = 6.0;

/// Les 20 fichiers mesurés (relatifs au corpus), choisis par `scan` : voir
/// Les fichiers du banc.
pub const FICHIERS: &[&str] = &[
    r"darshgarg7_Garg-Industries_2013-2017\7d880886_Garg Indistries (Home).skp",
    r"fact-liverpool_medialab-factlab\50d624c6_media-lab_FACTLAB_2.skp",
    r"impakho_3D_Printing\7fd3443d_Ultimaker 2 Aluminum Extrusion 3D Printer.skp",
    r"brunurb_Google-Sketchup\90c60702_AutoSave_Skyscrapper.skp",
    r"aderusha_DDD-Printable-Wall-Control-System\99ad0bc9_DDD Printable Wall Control Centerpieces - Locking Spacers.skp",
    r"baronk2_intro-to-sketchup\d8b0d8f8_AutoSave_lego.skp",
    r"Gheanzkie_SKETCHUP\f2caa73f_BOX.skp",
    r"aderusha_DDD-Printable-Wall-Control-System\5ad1210c_DDD Printable Wall Control Gridfinity.skp",
    r"loglow_TinyNES\e6a8767f_TinyNES_Model.skp",
    r"brunurb_Google-Sketchup\faa79e93_LegoRoomFurniture.skp",
    r"Priyanshu10045_Residential-Complex-3D-Model\2c7e9b85_Tower.skp",
    r"brunurb_Google-Sketchup\dd8fa84c_container_withFurniture.skp",
    r"brunurb_Google-Sketchup\50ba44e5_transparent table.skp",
    r"brunurb_Google-Sketchup\2db8aef7_Modern20BankBuilding.skp",
    r"impakho_3D_Printing\34a87083_main.skp",
    r"sgraber_Graber\9c33c596_graber-atx-3d.skp",
    r"bwack_C64-Saver-bwack\f6f4fb07_c64saver12.skp",
    r"jamiehoward_sketchup-hackerspace\384f529e_hackerspace.skp",
    r"brunurb_Google-Sketchup\7cb5129e_interior_houseComponent_1_1_.skp",
    r"BIM-Tools_SketchUp-IFC-Manager\952a47bb_ifc-hbim-convento-textures.skp",
];

fn corpus_dir() -> PathBuf {
    std::env::var_os("ESQ_SKP_REEL")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("tools").join("skp-reel")))
        .expect("dossier du corpus")
}

// ---------------------------------------------------------------------------------------
// Mémoire du processus

#[repr(C)]
#[derive(Default)]
struct ProcessMemoryCounters {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32GetProcessMemoryInfo(process: isize, counters: *mut ProcessMemoryCounters, cb: u32) -> i32;
}

/// (mémoire privée, pointe de la mémoire privée, pointe de l'ensemble de travail), en Mo.
fn memory() -> (f64, f64, f64) {
    let mut c = ProcessMemoryCounters { cb: std::mem::size_of::<ProcessMemoryCounters>() as u32, ..Default::default() };
    // SAFETY: the structure is the documented PROCESS_MEMORY_COUNTERS, with its size in `cb`.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) } != 0;
    let mb = |b: usize| b as f64 / (1024.0 * 1024.0);
    if ok { (mb(c.pagefile_usage), mb(c.peak_pagefile_usage), mb(c.peak_working_set_size)) } else { (0.0, 0.0, 0.0) }
}

// ---------------------------------------------------------------------------------------
// Mesures

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Valeur au rang `p` (0 à 1) d'une série.
fn percentile(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    s[((s.len() - 1) as f64 * p).round() as usize]
}

/// Ce que le modèle contient, placé dans le monde (chaque copie d'un composant compte).
#[derive(Default)]
struct Shape {
    faces: u64,
    edges: u64,
    instances: u64,
    depth: usize,
    defs: usize,
    textures: usize,
}

fn shape(m: &Model) -> Shape {
    let mut s = Shape {
        defs: m.defs.len(),
        textures: m.materials.values().filter(|x| x.ext.texture.is_some()).count(),
        ..Default::default()
    };
    esq_core::walk::visit(m, |p| {
        s.faces += p.ents.faces.len() as u64;
        s.edges += p.ents.edges.len() as u64;
        s.instances += (!p.path.is_empty()) as u64;
        s.depth = s.depth.max(p.path.len());
    });
    s
}

/// Une image de la vue 3D comme la fenêtre la dessine (`EsquisseApp::viewport`), hors écran,
/// attendue jusqu'au bout sur la carte graphique.
fn frame(app: &mut EsquisseApp, rs: &egui_wgpu::RenderState, painter: &egui::Painter, moving: bool) {
    app.turn_with_eye();
    app.prepare_scene(rs);
    let lighting = app.lighting();
    let (env, _) = crate::environments::active(&app.doc.model, true);
    let style = crate::dialogs::pref_pages::render_style(&app.style, moving);
    let grid = crate::model_grid::view_of(&app.doc.model, app.style.modeling_grid);
    let mip = app.doc.model.meta.mipmapping;
    let cam = app.cam.clone();
    let v = app.view.as_mut().expect("vue 3D");
    v.set_environment(rs, env);
    v.set_grid(grid);
    v.mipmapping = mip;
    v.render(rs, &cam, [W, H], 1.0, &style, &lighting);
    app.ensure_overlay_pick();
    let o = Overlay {
        painter: painter.clone(),
        origin: egui::Pos2::ZERO,
        cam: &app.cam,
        doc: &app.doc,
        units: app.doc.model.units,
    };
    app.paint_model_overlay(&o);
    let _ = rs.device.poll(wgpu::PollType::wait_indefinitely());
}

fn render_state() -> Option<egui_wgpu::RenderState> {
    let instance = crate::tests::gpu::instance();
    let opts =
        wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() };
    let adapter = pollster(instance.request_adapter(&opts)).ok()?;
    let (device, queue) = pollster(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let renderer = egui_wgpu::Renderer::new(&device, format, Default::default());
    Some(egui_wgpu::RenderState {
        adapter: adapter.clone(),
        available_adapters: vec![adapter],
        instance,
        device,
        queue,
        target_format: format,
        renderer: std::sync::Arc::new(egui::mutex::RwLock::new(renderer)),
    })
}

fn pollster<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

/// Signature des inférences d'une grille de points de la vue : la même avant et après une
/// optimisation.
fn snap_signature(snaps: &[crate::pick::Snap]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for s in snaps {
        format!("{:?}", s.kind).hash(&mut h);
        for c in s.pos.to_array() {
            ((c * 1e3).round() as i64).hash(&mut h);
        }
        format!("{:?}", s.face.as_ref().map(|f| &f.0.item)).hash(&mut h);
        format!("{:?}", s.edge.as_ref().map(|e| &e.0.item)).hash(&mut h);
    }
    h.finish()
}

fn save_png(dir: &Path, name: &str, app: &EsquisseApp, rs: &egui_wgpu::RenderState) {
    if let Some((w, h, px)) = app.view.as_ref().and_then(|v| v.read_pixels(rs)) {
        crate::png::write_rgba(&dir.join(name), w, h, &px).expect("image écrite");
    }
}

/// Temps donné à chaque mesure répétée (orbite, inférences, annulations) : au-delà, la série
/// s'arrête (au moins quelques essais) et le nombre fait est noté.
fn budget_s() -> f64 {
    std::env::var("ESQ_BENCH_BUDGET_S").ok().and_then(|v| v.parse().ok()).unwrap_or(20.0)
}

fn phase(name: &str, t: Instant) {
    eprintln!("[banc] {name} : {:.0} ms", ms(t));
}

/// Mesure un fichier dans ce processus et rend la ligne de résultats (JSON).
fn measure(path: &Path, out_dir: &Path, shots: Option<&Path>) -> serde_json::Value {
    let max_mb: f64 = std::env::var("ESQ_BENCH_MAX_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(6000.0);
    let budget = budget_s() * 1000.0;
    let rs = render_state().expect("carte graphique");
    let info = rs.adapter.get_info();
    let ctx = egui::Context::default();
    let painter = egui::Painter::new(
        ctx.clone(),
        egui::LayerId::background(),
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(W as f32, H as f32)),
    );
    let mut app = EsquisseApp::bare(Some(Viewport3d::new(&rs)), None);
    app.gpu = Some(rs.clone());
    app.cam.size = DVec2::new(W as f64, H as f64);
    let base = memory().0;

    // Ouverture : lecture, construction du document (caches compris), premier affichage.
    let t = Instant::now();
    let (model, view) = EsquisseApp::read_model(path).expect("fichier lu");
    let read_ms = ms(t);
    let shape = shape(&model);
    let t = Instant::now();
    app.open_model(path, model, view);
    app.cam.size = DVec2::new(W as f64, H as f64);
    let build_ms = ms(t);
    let opened_mb = memory().0 - base;
    let t = Instant::now();
    frame(&mut app, &rs, &painter, false);
    let first_ms = ms(t);
    let (gpu_faces, gpu_lines, gpu_bytes) = app.view.as_ref().map_or((0, 0, 0), |v| v.gpu_size());
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if let Some(d) = shots {
        save_png(d, &format!("{stem}.ouverture.png"), &app, &rs);
    }

    phase("ouverture", t);

    // Orbite continue.
    let t0 = Instant::now();
    let c0 = crate::perf::get();
    let mut orbit = Vec::new();
    let cam0 = app.cam.clone();
    for i in 0..ORBIT_FRAMES + 3 {
        let t = Instant::now();
        app.cam.orbit_with(ORBIT_STEP, 0.0, true);
        frame(&mut app, &rs, &painter, true);
        if i >= 3 {
            orbit.push(ms(t));
        }
        if ms(t0) > budget && orbit.len() >= 5 {
            break;
        }
    }
    let c1 = crate::perf::get();
    let frames = (orbit.len() + 3) as u64;
    let orbit_meshes = c1.mesh_builds - c0.mesh_builds;
    let orbit_updates = c1.mesh_updates - c0.mesh_updates;
    let orbit_picks = c1.pick_builds - c0.pick_builds;
    // Back where it was, for the same images and inferences before and after: the camera
    // itself (turning back as many steps would leave rounding that depends on how many
    // images the time given allowed).
    app.cam = cam0;
    frame(&mut app, &rs, &painter, false);
    phase("orbite", t0);

    // Inférence sous la souris, sur une grille de 15 × 10 points (dans l'ordre d'une
    // diagonale, pour qu'une série coupée par le temps couvre toute la vue).
    let t0 = Instant::now();
    let mut pick = Vec::new();
    let mut visits = Vec::new();
    let mut snaps = Vec::new();
    app.ensure_pick();
    let axes = app.doc.model.axes;
    let grid: Vec<(usize, usize)> = (0..150).map(|k| ((k * 7) % 15, (k * 3 + k / 50) % 10)).collect();
    for (i, j) in grid {
        if ms(t0) > budget && pick.len() >= 10 {
            break;
        }
        {
            let m = DVec2::new((i as f64 + 0.5) * W as f64 / 15.0, (j as f64 + 0.5) * H as f64 / 10.0);
            let v0 = crate::perf::get().pick_visits;
            let t = Instant::now();
            let cx = InferCx { cam: &app.cam, pick: &app.pick, axes, recent: None };
            let s = cx.infer(m, None, None, None);
            pick.push(ms(t));
            visits.push((crate::perf::get().pick_visits - v0) as f64);
            snaps.push(s);
        }
    }
    let pick_sig = snap_signature(&snaps);
    // The signature of the first k inferences, for each k: a series cut short by the time
    // given compares with the same beginning of a longer one.
    let pick_prefix: Vec<String> = (1..=snaps.len()).map(|k| format!("{:016x}", snap_signature(&snaps[..k]))).collect();
    phase("inférences", t0);

    // Tout sélectionner (et l'image qui le montre).
    let t = Instant::now();
    let c0 = crate::perf::get();
    app.select_all();
    frame(&mut app, &rs, &painter, false);
    let select_ms = ms(t);
    let select_updates = crate::perf::get().mesh_updates - c0.mesh_updates;
    // A click selecting one copy when nothing is selected (only the parts that show it).
    app.doc.select(Vec::new());
    frame(&mut app, &rs, &painter, false);
    let one = app.doc.model.root.instances.keys().next().map(EntityRef::Instance);
    let t1 = Instant::now();
    app.doc.select(one.into_iter().collect());
    frame(&mut app, &rs, &painter, false);
    let click_ms = ms(t1);
    app.select_all();
    frame(&mut app, &rs, &painter, false);
    phase("tout sélectionner", t);
    let selected = app.doc.sel.len();
    if let Some(d) = shots {
        save_png(d, &format!("{stem}.selection.png"), &app, &rs);
    }

    // Déplacer la sélection : 10 mouvements de la souris (aperçu), puis le clic qui pose.
    let t0 = Instant::now();
    let mut moves = Vec::new();
    let mut move_commit_ms = 0.0;
    if selected > 0 {
        app.set_tool(ToolId::Move);
        let m0 = DVec2::new(W as f64 / 2.0, H as f64 / 2.0);
        app.with_cx(Modifiers::NONE, |t, cx| {
            t.mouse_move(cx, m0);
            t.press(cx, m0);
            t.release(cx, m0);
        });
        let mut m = m0;
        for k in 0..10 {
            m = m0 + DVec2::new(4.0 * (k + 1) as f64, 2.0 * (k + 1) as f64);
            let t = Instant::now();
            app.with_cx(Modifiers::NONE, |t, cx| t.mouse_move(cx, m));
            frame(&mut app, &rs, &painter, false);
            moves.push(ms(t));
            if ms(t0) > budget && moves.len() >= 3 {
                break;
            }
        }
        let t = Instant::now();
        app.with_cx(Modifiers::NONE, |t, cx| {
            t.press(cx, m);
            t.release(cx, m);
        });
        frame(&mut app, &rs, &painter, false);
        move_commit_ms = ms(t);
        app.set_tool(ToolId::Select);
    }
    app.doc.select(Vec::new());
    frame(&mut app, &rs, &painter, false);
    phase("déplacer", t0);

    // 50 opérations annulables (déplacer d'un millimètre la première entité du modèle), puis
    // leur annulation, chacune suivie de l'image qui la montre.
    let target = {
        let e = &app.doc.model.root;
        e.instances.keys().map(EntityRef::Instance).next().or_else(|| e.edges.keys().map(EntityRef::Edge).next())
    };
    let t0 = Instant::now();
    let mut commits = Vec::new();
    let mut undos = Vec::new();
    let mut undo_mb = 0.0;
    let mut ops = 0;
    let mut undo_bytes = 0;
    if let Some(target) = target {
        let before = memory().0;
        let b0 = crate::perf::get().undo_bytes;
        for _ in 0..50 {
            if memory().0 > max_mb || (ms(t0) > budget && ops >= 3) {
                break;
            }
            let t = Instant::now();
            app.doc.commit("Déplacer", |m, d| {
                let e = m.entities_mut(d);
                let all = e.with_bounding_edges(&[target]);
                e.transform_selection(&all, &DMat4::from_translation(DVec3::X));
            });
            commits.push(ms(t));
            ops += 1;
        }
        undo_bytes = (crate::perf::get().undo_bytes - b0) / ops.max(1) as u64;
        undo_mb = memory().0 - before;
        let t1 = Instant::now();
        for _ in 0..ops {
            let t = Instant::now();
            app.doc.undo();
            frame(&mut app, &rs, &painter, false);
            undos.push(ms(t));
            if ms(t1) > budget && undos.len() >= 3 {
                break;
            }
        }
        // The rest without the images (the state after the 50 undos is that of the start).
        for _ in undos.len()..ops {
            app.doc.undo();
        }
        frame(&mut app, &rs, &painter, false);
    }
    phase("annulations", t0);

    // Enregistrer en .skp (avec sa vignette) et en .esq.
    let skp = out_dir.join(format!("{stem}.skp"));
    let t = Instant::now();
    app.save_as_format(&skp, crate::skpsave::SaveFormat::Skp(None)).expect(".skp enregistré");
    let save_skp_ms = ms(t);
    phase("enregistrer .skp", t);
    let esq = out_dir.join(format!("{stem}.esq"));
    let t = Instant::now();
    app.save_as_format(&esq, crate::skpsave::SaveFormat::Esq).expect(".esq enregistré");
    let save_esq_ms = ms(t);
    phase("enregistrer .esq", t);
    let size_mb = |p: &Path| std::fs::metadata(p).map_or(0.0, |m| m.len() as f64 / 1e6);

    let (_, peak_mb, ws_peak_mb) = memory();
    let mut r = serde_json::Map::new();
    let mut put = |k: &str, v: serde_json::Value| {
        r.insert(k.to_string(), v);
    };
    put("file", path.file_name().map(|s| s.to_string_lossy().into_owned()).into());
    put("gpu", format!("{} ({:?})", info.name, info.backend).into());
    put("mb", size_mb(path).into());
    put("faces", shape.faces.into());
    put("edges", shape.edges.into());
    put("instances", shape.instances.into());
    put("depth", shape.depth.into());
    put("defs", shape.defs.into());
    put("textures", shape.textures.into());
    put("gpu_faces", gpu_faces.into());
    put("gpu_lines", gpu_lines.into());
    put("gpu_mb", (gpu_bytes as f64 / 1e6).into());
    put("read_ms", read_ms.into());
    put("build_ms", build_ms.into());
    put("first_ms", first_ms.into());
    put("opened_mb", opened_mb.into());
    put("orbit_med_ms", percentile(&orbit, 0.5).into());
    put("orbit_p99_ms", percentile(&orbit, 0.99).into());
    put("orbit_frames", frames.into());
    put("orbit_meshes", orbit_meshes.into());
    put("orbit_updates", orbit_updates.into());
    put("select_updates", select_updates.into());
    put("click_ms", click_ms.into());
    put("orbit_picks", orbit_picks.into());
    put("picks", pick.len().into());
    put("pick_med_ms", percentile(&pick, 0.5).into());
    put("pick_p99_ms", percentile(&pick, 0.99).into());
    put("pick_visits_med", percentile(&visits, 0.5).into());
    put("pick_visits_max", percentile(&visits, 1.0).into());
    put("pick_sig", format!("{pick_sig:016x}").into());
    put("pick_prefix", pick_prefix.into());
    put("selected", selected.into());
    put("select_ms", select_ms.into());
    put("move_med_ms", percentile(&moves, 0.5).into());
    put("move_commit_ms", move_commit_ms.into());
    put("ops", ops.into());
    put("commit_med_ms", percentile(&commits, 0.5).into());
    put("undos", undos.len().into());
    put("undo_med_ms", percentile(&undos, 0.5).into());
    put("undo50_ms", (percentile(&undos, 0.5) * ops as f64).into());
    put("undo_mb", undo_mb.into());
    put("undo_bytes", undo_bytes.into());
    put("save_skp_ms", save_skp_ms.into());
    put("save_esq_ms", save_esq_ms.into());
    put("skp_mb", size_mb(&skp).into());
    put("esq_mb", size_mb(&esq).into());
    put("peak_mb", peak_mb.into());
    put("ws_peak_mb", ws_peak_mb.into());
    serde_json::Value::Object(r)
}

/// Un fichier (`ESQ_BENCH_FILE`), lancé par `corpus` dans un processus à lui.
#[test]
#[ignore]
fn one() {
    let Some(path) = std::env::var_os("ESQ_BENCH_FILE").map(PathBuf::from) else { return };
    // The files saved go to ESQ_BENCH_TMP (a folder of its own: something else on this PC
    // empties the temporary folder now and then).
    let tmp = std::env::var_os("ESQ_BENCH_TMP").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let out = tmp.join(format!("esq-bench-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let shots = std::env::var_os("ESQ_BENCH_SHOTS").map(PathBuf::from);
    if let Some(d) = &shots {
        std::fs::create_dir_all(d).unwrap();
    }
    let r = measure(&path, &out, shots.as_deref());
    let _ = std::fs::remove_dir_all(&out);
    println!("BENCH {r}");
}

fn files() -> Vec<PathBuf> {
    match std::env::var("ESQ_BENCH_FILES") {
        Ok(v) => v.split(';').filter(|s| !s.trim().is_empty()).map(PathBuf::from).collect(),
        Err(_) => FICHIERS.iter().map(|f| corpus_dir().join(f)).collect(),
    }
}

/// Le banc complet : chaque fichier dans un processus, `ESQ_BENCH_REPEAT` fois ; une ligne
/// JSON par fichier (la meilleure valeur de chaque temps), puis le tableau de
/// `ESQ_BENCH_OUT=<fichier>` y écrit aussi les lignes JSON.
#[test]
#[ignore]
fn corpus() {
    let repeat: usize = std::env::var("ESQ_BENCH_REPEAT").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    let exe = std::env::current_exe().unwrap();
    let mut rows = Vec::new();
    for f in files() {
        let mut best: Option<serde_json::Value> = None;
        for _ in 0..repeat {
            let out = std::process::Command::new(&exe)
                .args(["tests::bench::one", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
                .env("ESQ_BENCH_FILE", &f)
                .output()
                .expect("banc lancé");
            let text = String::from_utf8_lossy(&out.stdout);
            let Some(line) = text.lines().find_map(|l| l.strip_prefix("BENCH ")) else {
                eprintln!("{} : échec\n{}", f.display(), String::from_utf8_lossy(&out.stderr));
                break;
            };
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            best = Some(match best {
                None => v,
                Some(mut b) => {
                    for (k, x) in v.as_object().unwrap() {
                        if k.ends_with("_ms")
                            && let (Some(a), Some(n)) = (b[k].as_f64(), x.as_f64())
                        {
                            b[k] = a.min(n).into();
                        }
                    }
                    b
                }
            });
        }
        if let Some(b) = best {
            eprintln!("{b}");
            rows.push(b);
        }
    }
    if let Ok(p) = std::env::var("ESQ_BENCH_OUT") {
        let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
        std::fs::write(p, text).unwrap();
    }
    println!("{}", table(&rows));
}

/// Le tableau Markdown des résultats.
fn table(rows: &[serde_json::Value]) -> String {
    let f = |v: &serde_json::Value, k: &str, d: usize| match v[k].as_f64() {
        Some(x) => format!("{x:.d$}"),
        None => v[k].to_string(),
    };
    let mut s = String::from(
        "| Fichier | Mo | faces | arêtes | copies | lecture ms | construction ms | 1re image ms | orbite méd./99e ms | inférence méd./99e ms | tout sél. ms | déplacer (aperçu / pose) ms | 50 annul. : Mo / ms | .skp ms | .esq ms | pointe Mo |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for r in rows {
        s += &format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} / {} | {} / {} | {} | {} / {} | {} / {} | {} | {} | {} |\n",
            r["file"].as_str().unwrap_or("?"),
            f(r, "mb", 1),
            r["faces"],
            r["edges"],
            r["instances"],
            f(r, "read_ms", 0),
            f(r, "build_ms", 0),
            f(r, "first_ms", 0),
            f(r, "orbit_med_ms", 1),
            f(r, "orbit_p99_ms", 1),
            f(r, "pick_med_ms", 2),
            f(r, "pick_p99_ms", 2),
            f(r, "select_ms", 0),
            f(r, "move_med_ms", 0),
            f(r, "move_commit_ms", 0),
            f(r, "undo_mb", 0),
            f(r, "undo50_ms", 0),
            f(r, "save_skp_ms", 0),
            f(r, "save_esq_ms", 0),
            f(r, "peak_mb", 0),
        );
    }
    s
}

/// Parcourt tout le corpus : taille, lecture, contenu placé de chaque fichier (TSV dans
/// `ESQ_BENCH_SCAN`), pour choisir les fichiers les plus lourds.
#[test]
#[ignore]
fn scan() {
    let Some(out) = std::env::var_os("ESQ_BENCH_SCAN") else { return };
    let root = corpus_dir();
    let mut all = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("skp")) {
                let len = e.metadata().map_or(0, |m| m.len());
                all.push((len, p));
            }
        }
    }
    all.sort();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let rows = std::sync::Mutex::new(Vec::new());
    let work = |limit: u64| {
        loop {
            let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let Some((len, p)) = all.get(i) else { break };
            if *len > limit {
                next.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                break;
            }
            let t = Instant::now();
            let row = match esq_skp::import_skp(p) {
                Ok(m) => {
                    let read = ms(t);
                    let s = shape(&m);
                    format!(
                        "{}\t{len}\t{read:.0}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t",
                        p.strip_prefix(&root).unwrap_or(p).display(),
                        s.faces,
                        s.edges,
                        s.instances,
                        s.depth,
                        s.defs,
                        s.textures,
                        m.root.faces.len() + m.defs.values().map(|d| d.entities.faces.len()).sum::<usize>(),
                    )
                }
                Err(e) => format!("{}\t{len}\t\t\t\t\t\t\t\t\t{e}", p.strip_prefix(&root).unwrap_or(p).display()),
            };
            rows.lock().unwrap().push(row);
        }
    };
    // Small files side by side, the big ones one at a time (memory).
    std::thread::scope(|s| {
        for _ in 0..6 {
            s.spawn(|| work(8_000_000));
        }
    });
    work(u64::MAX);
    let mut text = String::from(
        "fichier\toctets\tlecture_ms\tfaces\taretes\tcopies\tprofondeur\tdefinitions\ttextures\tfaces_propres\terreur\n",
    );
    for r in rows.into_inner().unwrap() {
        text += &r;
        text += "\n";
    }
    std::fs::write(out, text).unwrap();
}

/// Le tri des faces transparentes (« Meilleure ») d'une image d'orbite : 300 000 triangles,
/// tri d'origine contre `oit::Sorter`.
#[test]
#[ignore]
fn probe_sort() {
    use crate::scene::FaceVertex;
    let verts: Vec<FaceVertex> = (0..900_000u32)
        .map(|i| {
            let t = i / 3;
            let p = DVec3::new((t * 37 % 1001) as f64, (t * 11 % 677) as f64, (t % 113) as f64 + (i % 3) as f64);
            FaceVertex::plain(p, DVec3::Z, [1.0; 4], [1.0; 4], 0)
        })
        .collect();
    let mut sorter = crate::oit::Sorter::default();
    sorter.set(&verts);
    for k in 0..5 {
        let eye = [k as f32 * 100.0, -500.0, 300.0];
        let t = Instant::now();
        let a = crate::oit::back_to_front(&verts, eye, [0.0, 1.0, -0.5], true);
        phase("back_to_front", t);
        let t = Instant::now();
        let b = sorter.sort(&verts, eye, [0.0, 1.0, -0.5], true);
        phase("Sorter::sort", t);
        assert!(bytemuck::cast_slice::<FaceVertex, u8>(&a) == bytemuck::cast_slice::<FaceVertex, u8>(b));
    }
}

/// Taille (JSON) de chaque partie du modèle hors géométrie (`ESQ_BENCH_FILE`).
#[test]
#[ignore]
fn probe_skeleton() {
    let Some(path) = std::env::var_os("ESQ_BENCH_FILE").map(PathBuf::from) else { return };
    let (m, _) = EsquisseApp::read_model(&path).unwrap();
    let n = |v: serde_json::Result<Vec<u8>>| v.map_or(0, |b| b.len());
    eprintln!("[banc] materials {}", n(serde_json::to_vec(&m.materials)));
    eprintln!("[banc] layers {}", n(serde_json::to_vec(&m.layers)));
    eprintln!("[banc] scenes {}", n(serde_json::to_vec(&m.scenes)));
    eprintln!("[banc] meta {}", n(serde_json::to_vec(&m.meta)));
    eprintln!("[banc] view_ext {}", n(serde_json::to_vec(&m.view_ext)));
    eprintln!("[banc] attr_store {}", n(serde_json::to_vec(&m.attr_store)));
    eprintln!("[banc] files {}", n(serde_json::to_vec(&m.files)));
    eprintln!("[banc] comments {}", n(serde_json::to_vec(&m.comments)));
    let attrs: usize =
        m.defs.values().map(|d| n(serde_json::to_vec(&d.attributes)) + n(serde_json::to_vec(&d.ext))).sum();
    eprintln!("[banc] defs attributes+ext {attrs}");
    for (k, e) in std::iter::once(&m.root).chain(m.defs.values().map(|d| &d.entities)).enumerate() {
        let props = n(serde_json::to_vec(&e.props));
        if props > 100_000 {
            eprintln!("[banc] collection {k} : props {props}");
        }
    }
    let mut m = m;
    let (m0, t) = (memory().0, Instant::now());
    let a = crate::history::freeze(&mut m, None);
    phase("1er gel", t);
    let (m1, t) = (memory().0, Instant::now());
    let b = crate::history::freeze(&mut m, Some(&a));
    phase("2e gel", t);
    let m2 = memory().0;
    eprintln!(
        "[banc] parties partagées {:?} ; mémoire 1er gel {:.0} Mo, 2e {:.0} Mo",
        b.shared_parts(&a),
        m1 - m0,
        m2 - m1
    );
}

/// Temps de la triangulation de chaque face du modèle, une fois par collection
/// (`ESQ_BENCH_FILE`) : les faces les plus lentes et ce qu'elles ont.
#[test]
#[ignore]
fn probe_tess() {
    let Some(path) = std::env::var_os("ESQ_BENCH_FILE").map(PathBuf::from) else { return };
    let (m, _) = EsquisseApp::read_model(&path).unwrap();
    let t0 = Instant::now();
    let mut slow = Vec::new();
    let mut faces = 0;
    for e in std::iter::once(&m.root).chain(m.defs.values().map(|d| &d.entities)) {
        for (f, face) in e.faces.iter() {
            let t = Instant::now();
            let n = esq_core::tess::face_triangles(e, f).len();
            slow.push((ms(t), face.outer.len(), face.holes.len(), face.holes.iter().map(Vec::len).sum::<usize>(), n));
            faces += 1;
        }
    }
    phase(&format!("triangulation de {faces} faces"), t0);
    slow.sort_by(|a, b| b.0.total_cmp(&a.0));
    let top: f64 = slow.iter().take(100).map(|s| s.0).sum();
    eprintln!("[banc] les 100 plus lentes : {top:.0} ms");
    for s in slow.iter().take(8) {
        eprintln!("[banc]   {:.1} ms : {} sommets, {} trous ({} sommets), {} triangles", s.0, s.1, s.2, s.3, s.4);
    }
}

/// Temps de chaque étape d'une annulation et de l'image qui la suit (`ESQ_BENCH_FILE`), après
/// les mêmes opérations que le banc.
#[test]
#[ignore]
fn probe_undo() {
    let Some(path) = std::env::var_os("ESQ_BENCH_FILE").map(PathBuf::from) else { return };
    let rs = render_state().expect("carte graphique");
    let ctx = egui::Context::default();
    let painter = egui::Painter::new(
        ctx.clone(),
        egui::LayerId::background(),
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(W as f32, H as f32)),
    );
    let mut app = EsquisseApp::bare(Some(Viewport3d::new(&rs)), None);
    app.gpu = Some(rs.clone());
    let (model, view) = EsquisseApp::read_model(&path).unwrap();
    app.open_model(&path, model, view);
    app.cam.size = DVec2::new(W as f64, H as f64);
    frame(&mut app, &rs, &painter, false);
    let e = &app.doc.model.root;
    let Some(target) =
        e.instances.keys().map(EntityRef::Instance).next().or_else(|| e.edges.keys().map(EntityRef::Edge).next())
    else {
        return;
    };
    for _ in 0..6 {
        let t = Instant::now();
        app.doc.commit("Déplacer", |m, d| {
            let e = m.entities_mut(d);
            let all = e.with_bounding_edges(&[target]);
            e.transform_selection(&all, &DMat4::from_translation(DVec3::X));
        });
        phase("commit", t);
    }
    frame(&mut app, &rs, &painter, false);
    for _ in 0..5 {
        let t = Instant::now();
        app.doc.undo();
        phase("undo", t);
        let t = Instant::now();
        app.turn_with_eye();
        app.prepare_scene(&rs);
        phase("  prepare_scene (maillage et cache de sélection)", t);
        let t = Instant::now();
        frame(&mut app, &rs, &painter, false);
        phase("  reste de l'image", t);
    }
}

/// Temps de chaque étape d'un déplacement de tout le modèle (`ESQ_BENCH_FILE`).
#[test]
#[ignore]
fn probe_move() {
    let Some(path) = std::env::var_os("ESQ_BENCH_FILE").map(PathBuf::from) else { return };
    let mut app = EsquisseApp::bare(None, None);
    let (model, view) = EsquisseApp::read_model(&path).unwrap();
    app.open_model(&path, model, view);
    app.cam.size = DVec2::new(W as f64, H as f64);
    for _ in 0..2 {
        let t = Instant::now();
        let mut pc = crate::pick::PickCache::build(&app.doc.model, &app.style);
        phase("PickCache::build", t);
        let t = Instant::now();
        pc.build_trees();
        phase("PickCache::build_trees", t);
        let t = Instant::now();
        let st = app.style.clone();
        let opts =
            crate::scene::SceneOptions { ctx: &[], ctx_def: None, sel: &[], axes_xform: DMat4::IDENTITY, style: &st };
        let _ = crate::scene::build(&app.doc.model, &opts);
        phase("scene::build (rien de sélectionné)", t);
    }
    if std::env::var_os("ESQ_BENCH_PROBE_BUILD_ONLY").is_some() {
        return;
    }
    app.select_all();
    let sel = app.doc.sel.clone();
    let t = Instant::now();
    let ents = app.doc.ents();
    let all = ents.with_bounding_edges(&sel);
    phase("with_bounding_edges", t);
    let t = Instant::now();
    let moved = ents.vertices_of(&all);
    phase("vertices_of", t);
    let t = Instant::now();
    let _ = ents.move_limit(&moved);
    phase("move_limit", t);
    let t = Instant::now();
    let mut m = app.doc.model.clone();
    phase("clone", t);
    let t = Instant::now();
    m.root.transform_selection(&all, &DMat4::from_translation(DVec3::new(10.0, 5.0, 0.0)));
    phase("transform_selection", t);
    let t = Instant::now();
    m.root.fold_nonplanar(&moved);
    phase("fold_nonplanar", t);
    let t = Instant::now();
    let st = app.style.clone();
    let opts =
        crate::scene::SceneOptions { ctx: &[], ctx_def: None, sel: &sel, axes_xform: DMat4::IDENTITY, style: &st };
    let _ = crate::scene::build(&m, &opts);
    phase("scene::build", t);
    let t = Instant::now();
    let cx = InferCx { cam: &app.cam, pick: &app.pick, axes: DMat4::IDENTITY, recent: None };
    let _ = cx.infer(DVec2::new(640.0, 400.0), Some(DVec3::ZERO), None, None);
    phase("infer", t);
    let t = Instant::now();
    app.set_tool(ToolId::Move);
    let m0 = DVec2::new(640.0, 400.0);
    app.with_cx(Modifiers::NONE, |t, cx| t.mouse_move(cx, m0));
    phase("mouse_move (avant)", t);
    let t = Instant::now();
    app.with_cx(Modifiers::NONE, |t, cx| t.press(cx, m0));
    phase("press", t);
    let t = Instant::now();
    app.with_cx(Modifiers::NONE, |t, cx| t.mouse_move(cx, m0 + DVec2::new(5.0, 3.0)));
    phase("mouse_move (pendant)", t);
}

/// Compare deux dossiers d'images de `ESQ_BENCH_SHOTS` (`ESQ_BENCH_A`, `ESQ_BENCH_B`) :
/// chaque image doit être identique au pixel près.
#[test]
#[ignore]
fn compare() {
    let (Some(a), Some(b)) = (std::env::var_os("ESQ_BENCH_A"), std::env::var_os("ESQ_BENCH_B")) else { return };
    let (a, b) = (PathBuf::from(a), PathBuf::from(b));
    let mut differ = Vec::new();
    let mut n = 0;
    for e in std::fs::read_dir(&a).unwrap().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "png") {
            continue;
        }
        let q = b.join(p.file_name().unwrap());
        let (Ok(x), Ok(y)) = (image::open(&p), image::open(&q)) else {
            differ.push(format!("{} : absente", q.display()));
            continue;
        };
        n += 1;
        let (x, y) = (x.to_rgba8(), y.to_rgba8());
        if x.dimensions() != y.dimensions() {
            differ.push(format!("{} : tailles différentes", p.display()));
            continue;
        }
        let diff = x.pixels().zip(y.pixels()).filter(|(u, v)| u != v).count();
        if diff > 0 {
            differ.push(format!("{} : {diff} pixels différents", p.display()));
        }
    }
    println!("{n} images comparées");
    assert!(differ.is_empty(), "{differ:#?}");
}
