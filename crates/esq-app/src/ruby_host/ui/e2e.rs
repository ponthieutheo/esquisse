//! End to end: real Ruby (`UI::HtmlDialog`, `UI::WebDialog`, `UI::InputBox`, `UI.inputbox`)
//! driving real WebView2 windows. Skipped when there is no Ruby 3.2 or no WebView2 runtime.

use esq_core::model::{EntityRef, Model};
use esq_ruby::{Host, RunOutput};
use serde_json::Value as J;

/// `HOST` is global to the process: one host test at a time, all zones together.
use crate::tests::ruby::HOST_LOCK as LOCK;

struct TestHost;

impl Host for TestHost {
    fn call(&mut self, _m: &mut Model, _s: &mut Vec<EntityRef>, op: &str, args: &J) -> Result<J, String> {
        match op.strip_prefix("ui.") {
            Some(o) => super::pure(o, args),
            None => Err(format!("opération hôte inattendue: {op}")),
        }
    }
}

/// Runs Ruby with the web dialogs wired in; `None` when this machine cannot.
fn run(code: &str) -> Option<RunOutput> {
    let i = crate::tests::ruby::interp()?;
    if wry::webview_version().is_err() {
        eprintln!("WebView2 absent, test ignoré");
        return None;
    }
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut m = Model::new();
    let mut s = Vec::new();
    let mut host = TestHost;
    Some(esq_ruby::with_host(&mut host, || i.run(code, "e2e.rb", &mut m, &mut s)).unwrap())
}

/// Waits (up to ~10 s) until the block is true, pumping events like the app does each frame.
const WAIT: &str = "def wait_for(n = 100); n.times { return true if yield; Esq.ui_pump; sleep 0.1 }; false; end\n";

#[test]
fn html_dialog_calls_back_into_ruby_and_closes() {
    let Some(o) = run(r#"
        $r = nil
        d = UI::HtmlDialog.new(dialog_title: 'Test', width: 320, height: 200)
        d.add_action_callback('hello') { |ctx, a, b| $r = [a, b, ctx.class.name]; d.close }
        d.set_html("<html><body><script>sketchup.hello('x', 42)</script></body></html>")
        d.show_modal
        [$r, d.visible?]
    "#) else {
        return;
    };
    assert!(o.ok, "{}", o.error);
    assert_eq!(o.result, r#"[["x", 42, "UI::ActionCallbackContext"], false]"#);
}

#[test]
fn web_dialog_skp_urls_and_element_values() {
    let Some(o) = run(&format!(
        "{WAIT}{}",
        r#"
        $p = nil; $v = nil
        d = UI::WebDialog.new('Ancien', false, 'esq-test', 300, 200, 50, 60, true)
        d.add_action_callback('go') { |dlg, params| $p = params; $v = dlg.get_element_value('i'); dlg.close }
        d.set_html("<input id='i' value='abc'><script>window.location='skp:go@hello%20w'</script>")
        d.show
        wait_for { $p }
        [$p, $v]
    "#
    )) else {
        return;
    };
    assert!(o.ok, "{}", o.error);
    assert_eq!(o.result, r#"["hello w", "abc"]"#);
}

#[test]
fn scripts_wait_for_the_page_and_geometry_is_reported() {
    let Some(o) = run(&format!(
        "{WAIT}{}",
        r#"
        $ready = false; $title = nil; $closed = false
        d = UI::HtmlDialog.new(dialog_title: 'Géo', width: 400, height: 300, left: 120, top: 140)
        d.add_action_callback('ready') { |_c| $ready = true }
        d.set_on_closed { $closed = true }
        d.set_html("<title>t</title><body onload='sketchup.ready()'><p id='p'>a</p>")
        d.show
        d.execute_script("document.getElementById('p').textContent='changed'")
        wait_for { $ready }
        sleep 0.3
        d.set_size(500, 350)
        d.set_position(200, 210)
        sleep 0.5; Esq.ui_pump
        out = [d.visible?, d.get_size, d.get_position, d.get_content_size.all? { |v| v > 0 }]
        out << d.__send__(:class).name
        d.close
        wait_for { $closed }
        out + [$closed, d.visible?]
    "#
    )) else {
        return;
    };
    assert!(o.ok, "{}", o.error);
    assert!(
        o.result.starts_with("[true, [500, 350], [200, 210], true, \"UI::HtmlDialog\", true, false]"),
        "{}",
        o.result
    );
}

#[test]
fn input_box_collects_typed_values() {
    let Some(o) = run(r#"
        Esq::Forms.on_ready = lambda do |dlg|
          dlg.execute_script("document.getElementsByName('f0')[0].value='7';" \
                             "document.getElementsByName('f1')[0].checked=true;" \
                             "document.getElementsByName('f2')[0].value='Deux';" \
                             "document.getElementById('ok').click()")
        end
        box = UI::InputBox.new(title: 'Réglages', accept_label: 'Valider')
        box.add_textbox(label: 'Nombre', value: 3)
        box.add_checkbox(label: 'Actif', value: false)
        box.add_dropdown(label: 'Choix', options: %w[Un Deux Trois], value: 'Un')
        box.add_separator
        box.add_description('Un texte')
        r = box.prompt
        Esq::Forms.on_ready = nil
        r
    "#) else {
        return;
    };
    assert!(o.ok, "{}", o.error);
    assert_eq!(o.result, r#"{:Nombre=>7, :Actif=>true, :Choix=>"Deux"}"#);
}

#[test]
fn legacy_inputbox_keeps_default_types_and_cancels_with_false() {
    let Some(o) = run(r#"
        Esq::Forms.on_ready = lambda do |dlg|
          dlg.execute_script("document.getElementsByName('f1')[0].value='2,5';document.getElementById('ok').click()")
        end
        ok = UI.inputbox(['Nom', 'Ratio', 'Entier'], ['abc', 1.5, 4], 'Titre')
        Esq::Forms.on_ready = lambda { |dlg| dlg.execute_script("document.getElementById('no').click()") }
        cancelled = UI.inputbox(['Nom'], ['x'])
        Esq::Forms.on_ready = nil
        [ok, cancelled]
    "#) else {
        return;
    };
    assert!(o.ok, "{}", o.error);
    assert_eq!(o.result, r#"[["abc", 2.5, 4], false]"#);
}

#[test]
fn legacy_inputbox_offers_lists_as_dropdowns() {
    let Some(o) = run(r#"
        Esq::Forms.on_ready = lambda do |dlg|
          dlg.execute_script("document.getElementsByName('f0')[0].value='Rouge';document.getElementById('ok').click()")
        end
        r = UI.inputbox(['Couleur'], ['Bleu'], ['Rouge|Vert|Bleu'], 'Choix')
        Esq::Forms.on_ready = nil
        r
    "#) else {
        return;
    };
    assert!(o.ok, "{}", o.error);
    assert_eq!(o.result, r#"["Rouge"]"#);
}

#[test]
fn web_dialog_writes_an_image_of_itself() {
    let dir = std::env::temp_dir().join("esq-e2e-shot");
    let _ = std::fs::create_dir_all(&dir);
    let png = dir.join("dialog.png");
    let _ = std::fs::remove_file(&png);
    let path = png.to_string_lossy().replace('\\', "/");
    let code = r#"
            $ready = false
            d = UI::WebDialog.new('Photo', false, '', 300, 200, 30, 30, true)
            d.add_action_callback('ready') { |_dlg, _p| $ready = true }
            d.set_html("<body style='background:#c00' onload='window.location=\"skp:ready@\"'>rouge")
            d.show
            wait_for { $ready }
            sleep 0.5
            r = d.write_image("PATH", 'png')
            d.close
            r
        "#
    .replace("PATH", &path);
    let Some(o) = run(&format!("{WAIT}{code}")) else { return };
    assert!(o.ok, "{}", o.error);
    // The capture can fail on a locked desktop; when it works the file is a real PNG.
    eprintln!("write_image -> {}", o.result);
    if o.result == "true" {
        let bytes = std::fs::read(&png).expect("fichier écrit");
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    }
}
