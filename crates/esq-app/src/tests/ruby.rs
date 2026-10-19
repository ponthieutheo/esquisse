//! Ruby scripts run through the document: one undo step per run, none when nothing changed.

use esq_core::model::Model;
use esq_ruby::Interpreter;

use crate::doc::Document;

/// The Ruby host (`esq_ruby::with_host`) is global to the process: every test that lends one to
/// Ruby holds this lock, whatever its zone.
pub(crate) static HOST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One interpreter per test process (Ruby cannot be started twice): every esq-app test that
/// runs Ruby goes through this. `None` on machines without a Ruby 3.2 runtime.
pub(crate) fn interp() -> Option<&'static Interpreter> {
    static I: std::sync::OnceLock<Option<Interpreter>> = std::sync::OnceLock::new();
    crate::ruby_ui::share_support_dir();
    I.get_or_init(|| match Interpreter::start() {
        Ok(i) => Some(i),
        Err(e) => {
            assert!(!esq_ruby::candidate_dlls().iter().any(|p| p.is_file()), "Ruby présent mais inutilisable: {e:#}");
            // A check that must cover Ruby (merges) sets ESQ_REQUIRE_RUBY: no silent skip then.
            assert!(std::env::var_os("ESQ_REQUIRE_RUBY").is_none(), "Ruby exigé (ESQ_REQUIRE_RUBY) : {e:#}");
            None
        }
    })
    .as_ref()
}

#[test]
fn script_is_one_undo_step() {
    let Some(interp) = interp() else { return };
    // Every script starts by forgetting the previous one's operations: no other test's
    // script may run between another's end and its `take_ops`.
    let _g = HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut doc = Document::new(Model::new());

    // A script that changes nothing leaves no undo step.
    let r = doc.run_script("Script Ruby", |m, s| interp.run("1 + 1", "t.rb", m, s)).unwrap();
    assert!(r.ok && r.result == "2");
    assert!(doc.undo_label().is_none());

    // A drawing script is a single step, selection included.
    let code = "ents = Sketchup.active_model.entities
                f = ents.add_face([0,0,0],[100.mm,0,0],[100.mm,100.mm,0],[0,100.mm,0])
                f.pushpull(100.mm)
                Sketchup.active_model.selection.add(ents.grep(Sketchup::Face))
                ents.grep(Sketchup::Face).size";
    let r = doc.run_script("Script Ruby", |m, s| interp.run(code, "t.rb", m, s)).unwrap();
    assert!(r.ok, "{}", r.error);
    assert_eq!(r.result, "6");
    assert_eq!(doc.model.root.faces.len(), 6);
    assert_eq!(doc.sel.len(), 6);
    assert_eq!(doc.undo_label(), Some("Script Ruby"));

    doc.undo();
    assert!(doc.model.root.faces.is_empty());
    assert!(doc.sel.is_empty());
}
