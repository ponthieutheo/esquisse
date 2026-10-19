//! Models made to break the walks of the tree (`docs/securite.md`): a chain of 100 000
//! nested definitions and a cycle A → B → A, through the main paths of the application —
//! opening, the offscreen 3D view, every 3D export, Ruby. The nesting is cut at 256 levels
//! and the cycle broken when the file is read (`esq_core::repair`): no recursive walk
//! overflows the stack (which kills the process) nor runs forever.

use std::path::PathBuf;
use std::time::Duration;

use esq_core::model::Model;
use esq_core::repair::{MAX_DEPTH, samples};

use super::ui_driver::{Driver, app};
use crate::commands::Cmd;

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("esq-securite-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The two models, saved as `.esq` files in `d`.
fn files(d: &std::path::Path) -> Vec<(&'static str, PathBuf)> {
    [("chaine", samples::chain(100_000)), ("cycle", samples::cycle())]
        .into_iter()
        .map(|(name, m)| {
            let p = d.join(format!("{name}.esq"));
            std::fs::write(&p, serde_json::to_vec(&m).unwrap()).unwrap();
            (name, p)
        })
        .collect()
}

/// The deepest nesting of a model, walked iteratively.
fn depth(m: &Model) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(&m.root, 0usize)];
    while let Some((e, d)) = stack.pop() {
        deepest = deepest.max(d);
        for i in e.instances.values() {
            if let Some(def) = m.defs.get(i.def) {
                stack.push((&def.entities, d + 1));
            }
        }
    }
    deepest
}

/// Runs `f` on a thread of its own (an 8 MB stack, like the interpreter's) and waits for it.
fn ends<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(move || {
            let _ = tx.send(f());
        })
        .unwrap();
    rx.recv_timeout(Duration::from_secs(300)).unwrap_or_else(|_| panic!("{what} : ne finit pas"))
}

/// File › Open, then a few frames of the windows, Model Info (statistics walk the tree)
/// and the 3D view's mesh (and the real GPU when there is one).
#[test]
fn deep_and_cyclic_models_open_and_show() {
    let d = dir("ouvrir");
    for (name, p) in files(&d) {
        let (deepest, tris) = ends(name, move || {
            let mut a = app();
            a.open_path(&p);
            assert!(a.message.as_ref().is_some_and(|(m, _)| m.starts_with("Ouvert")), "{:?}", a.message);
            let mut ui = Driver::new();
            let ctx = ui.ctx.clone();
            a.run(&ctx, Cmd::ModelInfo);
            ui.frames(&mut a, 3);
            let style = crate::style::ViewStyle::default();
            let opts = crate::scene::SceneOptions {
                ctx: &[],
                ctx_def: None,
                sel: &[],
                axes_xform: esq_core::glam::DMat4::IDENTITY,
                style: &style,
            };
            let mesh = crate::scene::build(&a.doc.model, &opts);
            if let Some(mut gpu) = super::gpu::Gpu::new() {
                let cam = super::gpu::top_camera(1000.0, 64.0);
                let _ = gpu.render(&a.doc.model, &style, &cam, &crate::render::Lighting::default());
            }
            (depth(&a.doc.model), mesh.opaque.len())
        });
        assert!(deepest <= MAX_DEPTH, "{name} : {deepest}");
        assert!(tris > 0, "{name}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// A `.skp` gets the same check when it is opened (`esq_skp::import_skp_view`): 300 nested
/// definitions, written then read back, keep 256 levels.
#[test]
fn deep_skp_models_are_cut_when_opened() {
    let d = dir("skp");
    let p = d.join("chaine.skp");
    let q = p.clone();
    ends("écriture", move || esq_skp::export_skp(&samples::chain(300), &q).unwrap());
    let (m, _) = ends("skp", move || esq_skp::import_skp_view(&p).unwrap());
    assert!(depth(&m) <= MAX_DEPTH, "{}", depth(&m));
    assert!(depth(&m) >= 200);
    let _ = std::fs::remove_dir_all(&d);
}

/// What the deepest model kept can still be saved on the main thread's stack (1 MB on
/// Windows), in a debug build too: the native writer works on a thread of its own with a
/// large stack (`esq_skp::native::write::WRITE_STACK`, `docs/securite.md`, point 25).
#[test]
fn the_deepest_model_kept_saves_on_a_small_stack() {
    let d = dir("skp-pile");
    let p = d.join("chaine.skp");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            let m = esq_core::io::load_esq_bytes(&serde_json::to_vec(&samples::chain(1000)).unwrap()).unwrap();
            let esq = esq_core::io::save_esq(&m, &p.with_extension("esq")).is_ok();
            let skp = esq_skp::export_skp(&m, &p).is_ok();
            let _ = tx.send((depth(&m), skp, esq));
        })
        .unwrap();
    let r = rx.recv_timeout(Duration::from_secs(120)).unwrap();
    assert_eq!(r, (MAX_DEPTH, true, true));
    let _ = std::fs::remove_dir_all(&d);
}

/// File › Export › 3D Model, every format.
#[test]
fn deep_and_cyclic_models_export() {
    let d = dir("exporter");
    for (name, p) in files(&d) {
        let out = d.clone();
        ends(name, move || {
            let m = esq_core::io::load_esq(&p).unwrap();
            let o = esq_formats::AllOptions::default();
            let cx = esq_formats::ExportCx { view: None, name: name.into() };
            for fmt in esq_formats::ExportFormat::ALL {
                let target = out.join(format!("{name}.{}", fmt.extension()));
                esq_formats::export(&m, &target, fmt, &o, &cx).unwrap_or_else(|e| panic!("{fmt:?} : {e:#}"));
            }
        });
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// A Ruby script walking the definitions recursively, counting instances and bounds.
#[test]
fn deep_and_cyclic_models_go_to_ruby() {
    let Some(interp) = super::ruby::interp() else { return };
    let d = dir("ruby");
    for (name, p) in files(&d) {
        let mut m = esq_core::io::load_esq(&p).unwrap();
        let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let code = "def prof(ents, d)
                      m = d
                      ents.each { |e| m = [m, prof(e.definition.entities, d + 1)].max if e.respond_to?(:definition) }
                      m
                    end
                    model = Sketchup.active_model
                    used = model.definitions.map(&:count_used_instances)
                    [prof(model.entities, 0), used.size > 0, model.bounds.valid?]";
        let r = interp.run(code, "profond.rb", &mut m, &mut Vec::new()).unwrap();
        assert!(r.ok, "{name} : {}", r.error);
        let deepest: usize = r.result.trim_start_matches('[').split(',').next().unwrap().trim().parse().unwrap();
        assert!(deepest <= MAX_DEPTH, "{name} : {}", r.result);
    }
    let _ = std::fs::remove_dir_all(&d);
}
