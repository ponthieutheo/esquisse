//! Esquisse — a free 3D modeller that works like SketchUp.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod annot;
mod app;
mod attrs;
mod bvh;
mod camera;
mod clipboard;
mod commands;
mod comments;
mod comp_library;
#[cfg(test)]
mod content_gen;
mod context_menu;
mod crosshairs;
mod ctx_dialogs;
mod def_thumbs;
mod dialogs;
mod doc;
mod edit_ops;
mod entity_info;
mod env_panel;
mod environments;
mod export2d;
mod export_anim;
mod export_section;
mod export_vec;
mod extra;
mod file_flow;
mod filedlg;
mod fog;
mod fonts;
mod formats3d;
mod gpu_img;
mod hatch_panel;
mod history;
mod i18n;
mod icons;
mod imgfile;
mod import2d;
mod logo;
mod make_component;
mod mat_panel;
mod menus;
mod model_grid;
mod offscreen;
mod oit;
mod outliner_panel;
mod overlays_panel;
#[allow(dead_code)] // only the parity test reads the tables
mod parity;
mod perf;
mod photo_fg;
mod photo_img;
mod photo_view;
mod pick;
mod png;
mod prefs;
mod print;
mod render;
mod ruby_host;
mod ruby_tools;
mod ruby_ui;
mod safe_path;
mod scene;
mod scenes;
mod settings;
mod shadows;
mod shortcuts;
mod skh;
mod skm;
mod skpsave;
mod soften_panel;
mod style;
mod style_file;
mod styles;
mod styles_panel;
mod tags_panel;
mod text3d;
mod textures;
mod texturing;
mod toolbars;
mod tools;
mod ui;
mod view_cmds;
mod warnings;
mod watermarks;
#[cfg(test)]
mod winres;
mod zip;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result {
    env_logger::init();
    let mut file = None;
    let mut snapshot = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--snapshot" => snapshot = args.next().map(PathBuf::from),
            _ => file = Some(PathBuf::from(a)),
        }
    }
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Esquisse")
        .with_inner_size([1400.0, 880.0])
        .with_min_inner_size([800.0, 500.0]);
    if let Some(icon) = logo::icon() {
        viewport = viewport.with_icon(icon);
    }
    // On-screen checks driven by UI Automation and captured with PrintWindow: the window opens
    // off the screen, without taking the focus or a taskbar button, so it never gets in the
    // way of whoever uses the PC meanwhile.
    if std::env::var_os("ESQ_OFFSCREEN").is_some() {
        viewport = viewport.with_position([-20000.0, -20000.0]).with_active(false).with_taskbar(false);
    }
    let options = eframe::NativeOptions { viewport, renderer: eframe::Renderer::Wgpu, ..Default::default() };
    eframe::run_native(
        "Esquisse",
        options,
        Box::new(move |cc| {
            #[cfg(windows)]
            logo::use_exe_icon(cc);
            Ok(Box::new(app::EsquisseApp::new(cc, file, snapshot)))
        }),
    )
}
