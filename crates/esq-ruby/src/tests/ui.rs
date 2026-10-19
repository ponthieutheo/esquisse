//! Zone « ui » : module UI, menus, commandes, barres d'outils, minuteries, boîtes de dialogue.
//! L'application n'est pas là : un hôte factice enregistre les appels `host.ui.*` et rejoue des
//! réponses (la vraie chose est testée de bout en bout dans `esq-app`, `ruby_host::ui::e2e`).

use std::collections::HashMap;
use std::sync::Mutex;

use super::*;
use crate::host::{Host, with_host};

/// L'hôte est global au processus : un seul test d'hôte à la fois.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct Mock {
    calls: Vec<(String, J)>,
    /// Réponses fixes par opération.
    replies: HashMap<String, J>,
    /// Lots d'événements rendus par les appels successifs de `ui.html_wait` (`"$ID"` = la boîte).
    waits: Vec<Vec<J>>,
    queue: Vec<J>,
    wait_calls: usize,
}

impl Host for Mock {
    fn call(&mut self, _m: &mut Model, _s: &mut Vec<EntityRef>, op: &str, args: &J) -> Result<J, String> {
        self.calls.push((op.to_string(), args.clone()));
        if let Some(r) = self.replies.get(op) {
            return Ok(r.clone());
        }
        Ok(match op {
            // Fermer une boîte la ferme vraiment : l'événement revient au prochain tour.
            "ui.html_close" => {
                self.queue.push(json!({"t": "closed", "d": args["id"]}));
                J::Null
            }
            "ui.html_wait" => {
                self.wait_calls += 1;
                let mut evs: Vec<J> = Vec::new();
                if !self.waits.is_empty() {
                    for e in self.waits.remove(0) {
                        let s = e.to_string().replace("\"$ID\"", &args["id"].to_string());
                        evs.push(serde_json::from_str(&s).unwrap());
                    }
                }
                evs.append(&mut self.queue);
                // Garde-fou : une boîte que personne ne ferme ne doit pas bloquer le test.
                if evs.is_empty() && self.wait_calls > 40 {
                    evs.push(json!({"t": "closed", "d": args["id"]}));
                }
                J::Array(evs)
            }
            _ => J::Null,
        })
    }
}

impl Mock {
    fn reply(mut self, op: &str, v: J) -> Mock {
        self.replies.insert(op.to_string(), v);
        self
    }
    fn wait(mut self, batch: Vec<J>) -> Mock {
        self.waits.push(batch);
        self
    }
    fn called(&self, op: &str) -> Vec<&J> {
        self.calls.iter().filter(|c| c.0 == op).map(|c| &c.1).collect()
    }
}

/// Exécute `code` avec l'hôte factice ; renvoie la sortie et l'hôte pour inspection.
fn with_mock(mock: Mock, code: &str) -> Option<(RunOutput, Mock)> {
    let i = interp()?;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut mock = mock;
    let mut m = Model::new();
    let mut sel = Vec::new();
    let out = with_host(&mut mock, || i.run(code, "ui.rb", &mut m, &mut sel)).unwrap();
    Some((out, mock))
}

/// Sans application (les appels hôte échouent et les méthodes retombent sur leurs valeurs par défaut).
fn headless(code: &str) -> Option<String> {
    let i = interp()?;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut m = Model::new();
    let mut sel = Vec::new();
    let out = i.run(code, "ui.rb", &mut m, &mut sel).unwrap();
    assert!(out.ok, "{}", out.error);
    Some(out.result)
}

/// Le message d'erreur de la première ligne d'une exception levée.
fn error_of(code: &str) -> Option<String> {
    let i = interp()?;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut m = Model::new();
    let mut sel = Vec::new();
    let out = i.run(code, "ui.rb", &mut m, &mut sel).unwrap();
    assert!(!out.ok, "aurait dû lever une exception: {}", out.result);
    Some(out.error.lines().next().unwrap_or_default().to_string())
}

fn ok(code: &str, mock: Mock) -> Option<(String, Mock)> {
    let (o, m) = with_mock(mock, code)?;
    assert!(o.ok, "{}", o.error);
    Some((o.result, m))
}

// ------------------------------------------------------------------------------------ constantes

#[test]
fn global_constants_match_sketchup() {
    let Some(r) = headless(
        "[MB_OK, MB_OKCANCEL, MB_ABORTRETRYIGNORE, MB_YESNOCANCEL, MB_YESNO, MB_RETRYCANCEL, MB_MULTILINE,
          IDOK, IDCANCEL, IDABORT, IDRETRY, IDIGNORE, IDYES, IDNO,
          VK_SPACE, VK_PRIOR, VK_NEXT, VK_END, VK_HOME, VK_LEFT, VK_UP, VK_RIGHT, VK_DOWN, VK_INSERT, VK_DELETE,
          VK_SHIFT, VK_CONTROL, VK_ALT, VK_COMMAND, VK_MENU,
          ALT_MODIFIER_KEY, ALT_MODIFIER_MASK, COPY_MODIFIER_KEY, COPY_MODIFIER_MASK,
          CONSTRAIN_MODIFIER_KEY, CONSTRAIN_MODIFIER_MASK,
          MF_ENABLED, MF_GRAYED, MF_DISABLED, MF_CHECKED, MF_UNCHECKED, TB_HIDDEN, TB_VISIBLE, TB_NEVER_SHOWN]",
    ) else {
        return;
    };
    assert_eq!(
        r,
        "[0, 1, 2, 3, 4, 5, 16, 1, 2, 3, 4, 5, 6, 7, 32, 33, 34, 35, 36, 37, 38, 39, 40, 45, 46, 16, 17, 18, 18, 18, \
         18, 32, 17, 8, 16, 4, 0, 1, 2, 8, 0, 0, 1, -1]"
    );
    let Some(r) = headless(
        "[CMD_SELECT, CMD_PAINT, CMD_ERASE, CMD_RECTANGLE, CMD_LINE, CMD_CIRCLE, CMD_ARC, CMD_POLYGON, CMD_FREEHAND,
          CMD_PUSHPULL, CMD_TEXT, CMD_MOVE, CMD_ROTATE, CMD_EXTRUDE, CMD_SCALE, CMD_OFFSET, CMD_MEASURE,
          CMD_PROTRACTOR, CMD_SKETCHCS, CMD_SECTION, CMD_DRAWOUTLINES, CMD_DRAWCUTS, CMD_ORBIT, CMD_DOLLY,
          CMD_ZOOM, CMD_ZOOM_WINDOW, CMD_ZOOM_EXTENTS, CMD_CAMERA_UNDO, CMD_WIREFRAME, CMD_HIDDENLINE, CMD_SHADED,
          CMD_TEXTURED, CMD_TRANSPARENT, CMD_WALK, CMD_PAN, CMD_MAKE_COMPONENT, CMD_DIMENSION, CMD_VIEW_ISO,
          CMD_VIEW_TOP, CMD_VIEW_FRONT, CMD_VIEW_RIGHT, CMD_VIEW_BACK, CMD_VIEW_LEFT, CMD_VIEW_BOTTOM,
          CMD_VIEW_PERSPECTIVE, CMD_POSITION_CAMERA, CMD_NEW, CMD_OPEN, CMD_SAVE, CMD_CUT, CMD_COPY, CMD_PASTE,
          CMD_DELETE, CMD_UNDO, CMD_REDO, CMD_PRINT, CMD_PAGE_NEW, CMD_PAGE_DELETE, CMD_PAGE_UPDATE,
          CMD_PAGE_NEXT, CMD_PAGE_PREVIOUS, CMD_RUBY_CONSOLE, CMD_SKETCHAXES, CMD_SHOWHIDDEN,
          CMD_SHOWHIDDENGEOMETRY, CMD_SHOWHIDDENOBJECTS, CMD_SHOWGUIDES, CMD_SELECTION_ZOOM_EXT,
          CMD_DISPLAY_FOV].map { |v| v.to_s(16) }.join(' ')",
    ) else {
        return;
    };
    assert_eq!(
        r,
        "\"521e 5252 521b 5266 521c 5268 5249 5267 5227 5231 539d 5238 5289 5415 52f4 526c 5220 5241 5286 5359 5363 \
         5364 290c 291b 290d 291e 291f 2921 290e 290f 2910 292b 2911 2918 291d 525b 53a2 290b 2905 2906 2907 2909 \
         2908 290a 2917 52b1 e100 e101 e103 e123 e122 e125 521d e12b e12c e107 524b 5256 524c 2927 2928 53e6 291a \
         52a2 52a3 52a1 55dc 53dd 53f6\""
    );
    let Some(r) = headless(
        "[UI::HtmlDialog::STYLE_WINDOW, UI::HtmlDialog::STYLE_DIALOG, UI::HtmlDialog::STYLE_UTILITY,
          UI::HtmlDialog::CEF_VERSION.class, UI::HtmlDialog::CHROME_VERSION.class,
          UI::InputBox::ValueType::STRING, UI::InputBox::ValueType::INTEGER, UI::InputBox::ValueType::FLOAT,
          UI::InputBox::ValueType::LENGTH]",
    ) else {
        return;
    };
    assert_eq!(r, "[1, 0, 2, String, String, 0, 1, 2, 3]");
}

// ------------------------------------------------------------------------------------ messagebox

#[test]
fn messagebox_returns_the_button_id() {
    let m = Mock::default().reply("ui.messagebox", json!(6));
    let Some((r, m)) = ok("UI.messagebox('Salut', MB_YESNO)", m) else { return };
    assert_eq!(r, "6");
    let c = m.called("ui.messagebox");
    assert_eq!(c[0]["text"], "Salut");
    assert_eq!(c[0]["type"], 4);
    // Le titre de SketchUp (« SketchUp ») devient « Esquisse ».
    assert_eq!(c[0]["title"], "Esquisse");
}

#[test]
fn messagebox_text_is_converted_and_type_defaults_to_ok() {
    let m = Mock::default().reply("ui.messagebox", json!(1));
    let Some((_, m)) = ok("UI.messagebox(42); UI.messagebox(:sym); UI.messagebox(nil)", m) else { return };
    let c = m.called("ui.messagebox");
    assert_eq!(c.iter().map(|a| a["text"].as_str().unwrap()).collect::<Vec<_>>(), ["42", "sym", ""]);
    assert!(c.iter().all(|a| a["type"] == 0));
}

#[test]
fn messagebox_multiline_returns_idok_with_its_title() {
    let Some((r, m)) = ok("UI.messagebox('a\nb', MB_MULTILINE, 'Titre')", Mock::default()) else { return };
    assert_eq!(r, "1");
    let c = m.called("ui.messagebox");
    assert_eq!((c[0]["type"].as_i64(), c[0]["title"].as_str()), (Some(16), Some("Titre")));
}

#[test]
fn messagebox_argument_errors() {
    let Some(e) = error_of("UI.messagebox") else { return };
    assert_eq!(e, "ArgumentError: Too few arguments: text required");
    let Some(e) = error_of("UI.messagebox('x', 'y')") else { return };
    assert!(e.starts_with("TypeError:"), "{e}");
    // Sans application, la boîte « répond » OK.
    let Some(r) = headless("UI.messagebox('x', MB_YESNO)") else { return };
    assert_eq!(r, "1");
}

// ------------------------------------------------------------------------------------ menus

fn menu_entries() -> &'static str {
    "JSON.parse(Esq.commands_json).map { |c| c['sep'] ? [c['menu'], '-'] : [c['menu'], c['title']] }"
}

#[test]
fn menu_names_resolve_like_sketchup() {
    let Some(r) = headless(
        "n = ->(a) { m = UI.menu(*a); m && m.__path }
         [n.([]), n.(['Plugins']), n.(['extensions']), n.(['File']), n.(['edit']), n.(['View']), n.(['Camera']),
          n.(['Draw']), n.(['Tools']), n.(['Window']), n.(['Help']), n.(['Developer']),
          n.(['ed']), n.(['pl']), n.(['hel']), n.([6]), n.([10]), n.([0]), n.([11])]",
    ) else {
        return;
    };
    assert_eq!(
        r,
        "[\"Extensions\", \"Extensions\", \"Extensions\", \"File\", \"Edit\", \"View\", \"Camera\", \"Draw\", \"Tools\", \
         \"Window\", \"Help\", \"Extensions/Developer\", \"Edit\", \"Extensions\", \"Help\", \"Tools\", \
         \"Extensions/Developer\", nil, nil]"
    );
    let Some(e) = error_of("UI.menu('zzz')") else { return };
    assert_eq!(e, "ArgumentError: Unknown menu zzz");
}

#[test]
fn menu_items_commands_and_submenus() {
    let Some(r) = headless(&format!(
        "Esq.menu_items.clear
         m = UI.menu('Tools')
         id1 = m.add_item('Un') {{ 1 }}
         cmd = UI::Command.new('Deux') {{ 2 }}
         id2 = m.add_item(cmd)
         m.add_separator
         sub = m.add_submenu('Sous')
         id3 = sub.add_item('Trois') {{ 3 }}
         [id1.class, id2 == cmd.__id, id3 > id2, sub.class, {}, Esq.invoke_command(id3), Esq.invoke_command(-1)]",
        menu_entries()
    )) else {
        return;
    };
    assert_eq!(
        r,
        "[Integer, true, true, Sketchup::Menu, [[\"Tools\", \"Un\"], [\"Tools\", \"Deux\"], [\"Tools\", \"-\"], \
         [\"Tools/Sous\", \"Trois\"]], true, false]"
    );
}

#[test]
fn menu_add_item_edge_cases() {
    // Pas de bloc : la création du Proc échoue comme dans SketchUp.
    let Some(e) = error_of("UI.menu('Tools').add_item('Sans bloc')") else { return };
    assert_eq!(e, "ArgumentError: tried to create Proc object without a block");
    // Arité hors de 1..2 : nil, rien n'est ajouté.
    let Some(r) = headless("Esq.menu_items.clear; r = UI.menu.add_item('a', 1, 2) { }; [r, Esq.menu_items.size]")
    else {
        return;
    };
    assert_eq!(r, "[nil, 0]");
    let Some(r) = headless("UI.menu.add_item('Avec position', 3) { }.class") else { return };
    assert_eq!(r, "Integer");
    let Some(e) = error_of("UI.menu.add_submenu") else { return };
    assert!(e.starts_with("ArgumentError:"), "{e}");
    let Some(e) = error_of("Sketchup::Menu.new") else { return };
    assert!(e.starts_with("NoMethodError"), "{e}");
}

#[test]
fn validation_procs_set_flags() {
    let Some(r) = headless(
        "Esq.menu_items.clear
         m = UI.menu('Edit')
         a = m.add_item('A') { }
         b = m.add_item('B') { }
         c = m.add_item('C') { }
         r1 = m.set_validation_proc(a) { MF_GRAYED }
         r2 = m.set_validation_proc(b) { MF_CHECKED | MF_ENABLED }
         r3 = m.set_validation_proc(9_999_999) { MF_GRAYED }
         cmd = Esq.cmds[c]
         cmd.set_validation_proc { raise 'boum' }
         flags = JSON.parse(Esq.commands_json).map { |x| x['flags'] }
         [r1, r2, r3, flags, cmd.set_validation_proc { MF_DISABLED }.equal?(cmd), cmd.get_validation_proc.class]",
    ) else {
        return;
    };
    // L'exception d'une validation ne casse pas le menu : l'entrée reste active.
    assert_eq!(r, "[true, true, false, [1, 8, 0], true, Proc]");
}

#[test]
fn command_attributes_and_errors() {
    let Some(r) = headless(
        "c = UI::Command.new('Titre') { :ok }
         before = [c.menu_text, c.tooltip, c.status_bar_text, c.small_icon, c.large_icon, c.extension, c.get_validation_proc]
         c.tooltip = 'info'; c.status_bar_text = 'barre'; c.menu_text = 'Autre'
         c.small_icon = 'a.png'; c.large_icon = 'b.png'
         [before, c.tooltip, c.status_bar_text, c.menu_text, c.small_icon, c.large_icon, c.proc.call,
          (c.extension = nil), c.extension]",
    ) else {
        return;
    };
    assert_eq!(
        r,
        "[[\"Titre\", \"\", \"\", \"\", \"\", nil, nil], \"info\", \"barre\", \"Autre\", \"a.png\", \"b.png\", :ok, nil, nil]"
    );
    let Some(e) = error_of("UI::Command.new('x')") else { return };
    assert_eq!(e, "ArgumentError: tried to create Proc object without a block");
    // Une commande « interne » (argument entier) n'a ni bloc ni titre.
    let Some(r) = headless("c = UI::Command.new(7); [c.menu_text, c.proc]") else { return };
    assert_eq!(r, "[\"\", nil]");
}

#[test]
fn context_menu_handlers_add_entries() {
    let Some(r) = headless(
        "Esq.context_handlers.clear
         a = UI.add_context_menu_handler { |menu| menu.add_item('Mon action') { }; menu.add_separator }
         b = UI.add_context_menu_handler { |menu| raise 'oups' }
         j = JSON.parse(Esq.context_menu_json).map { |c| c['sep'] ? '-' : c['title'] }
         [a, b, j, Esq.menu_items.none? { |i| i[:menu].start_with?('Context') }]",
    ) else {
        return;
    };
    assert_eq!(r, "[1, 2, [\"Mon action\", \"-\"], true]");
    let Some(e) = error_of("UI.add_context_menu_handler") else { return };
    assert!(e.starts_with("ArgumentError"), "{e}");
}

// ------------------------------------------------------------------------------------ barres d'outils

#[test]
fn toolbars_collect_commands_and_separators() {
    let m = Mock::default().reply("ui.toolbar_visible", json!(true)).reply("ui.toolbar_state", json!(1));
    let Some((r, m)) = ok(
        "Esq.toolbar_defs.clear
         t = UI::Toolbar.new('Ma barre')
         a = UI::Command.new('A') { }
         b = UI::Command.new('B') { }
         r = [t.add_item(a).equal?(t), t.add_separator.equal?(t), t.add_item(b).equal?(t)]
         same = UI::Toolbar.new('Ma barre').size
         items = t.map { |x| x.respond_to?(:menu_text) ? x.menu_text : x }
         r + [t.name, t.length, t.size, same, items, t.show, t.hide, t.restore, t.visible?, t.get_last_state,
              JSON.parse(Esq.toolbars_json).map { |x| [x['name'], x['items'].map { |i| i && i['title'] }] }]",
        m,
    ) else {
        return;
    };
    assert_eq!(
        r,
        "[true, true, true, \"Ma barre\", 3, 3, 3, [\"A\", \"|\", \"B\"], nil, nil, nil, true, 1, \
         [[\"Ma barre\", [\"A\", nil, \"B\"]]]]"
    );
    let c = m.called("ui.toolbar_set_visible");
    // show, hide, puis restore (montre sauf si masquée la dernière fois : état 1 → montre).
    assert_eq!(c.iter().map(|a| a["visible"].as_bool().unwrap()).collect::<Vec<_>>(), [true, false, true]);
    assert!(c.iter().all(|a| a["plugin"] == true && a["name"] == "Ma barre"));
}

#[test]
fn toolbar_errors_and_names() {
    let Some(e) = error_of("UI::Toolbar.new('x').add_item('pas une commande')") else { return };
    assert!(e.starts_with("TypeError:"), "{e}");
    let Some(e) = error_of("UI::Toolbar.allocate") else { return };
    assert!(e.contains("private") || e.starts_with("NoMethodError") || e.starts_with("TypeError"), "{e}");
    let m = Mock::default().reply("ui.toolbar_names", json!(["Principale", "Dessin"]));
    let Some((r, _)) = ok("Esq.toolbar_defs.clear; UI::Toolbar.new('Perso'); UI.toolbar_names", m) else { return };
    assert_eq!(r, "[\"Principale\", \"Dessin\", \"Perso\"]");
    // restore ne montre pas une barre masquée à la dernière session.
    let m = Mock::default().reply("ui.toolbar_state", json!(0));
    let Some((_, m)) = ok("UI::Toolbar.new('Cachée').restore", m) else { return };
    assert!(m.called("ui.toolbar_set_visible").is_empty());
    // Première fois (jamais montrée) : restore montre.
    let m = Mock::default().reply("ui.toolbar_state", json!(-1));
    let Some((_, m)) = ok("UI::Toolbar.new('Neuve').restore", m) else { return };
    assert_eq!(m.called("ui.toolbar_set_visible").len(), 1);
}

#[test]
fn ui_toolbar_visibility_functions() {
    let m = Mock::default().reply("ui.toolbar_visible", json!(true)).reply("ui.toolbar_set_visible", json!(true));
    let Some((r, m)) = ok(
        "Esq.toolbar_defs.clear; UI::Toolbar.new('Plug')
         [UI.toolbar_visible?('Dessin'), UI.set_toolbar_visible('Plug', false), UI.toolbar('Plug').name]",
        m,
    ) else {
        return;
    };
    assert_eq!(r, "[true, true, \"Plug\"]");
    assert_eq!(m.called("ui.toolbar_visible")[0]["plugin"], false);
    assert_eq!(m.called("ui.toolbar_set_visible")[0]["plugin"], true);
}

// ------------------------------------------------------------------------------------ minuteries

#[test]
fn timers_register_fire_and_stop() {
    let m = Mock::default().reply("ui.timer_add", json!(77));
    let Some((r, m)) = ok(
        "$n = 0
         id = UI.start_timer(0.5, true) { $n += 1 }
         Esq.fire_timer(77); Esq.fire_timer(77)
         still = Esq.timers.key?(77)
         s = UI.stop_timer(77)
         gone = !Esq.timers.key?(77)
         once = UI.start_timer(1) { $n += 10 }
         Esq.timers.delete(77)
         [id, once, $n, still, s, !gone]",
        m,
    ) else {
        return;
    };
    assert_eq!(r, "[77, 77, 2, true, nil, false]");
    let add = m.called("ui.timer_add");
    assert_eq!((add[0]["ms"].as_i64(), add[0]["repeat"].as_bool()), (Some(500), Some(true)));
    assert_eq!((add[1]["ms"].as_i64(), add[1]["repeat"].as_bool()), (Some(1000), Some(false)));
    assert_eq!(m.called("ui.timer_remove")[0]["id"], 77);
}

#[test]
fn one_shot_timers_disappear_after_firing() {
    let m = Mock::default().reply("ui.timer_add", json!(5));
    let Some((r, _)) = ok(
        "$k = 0
         UI.start_timer(0.01) { $k += 1 }
         Esq.dispatch_ui_event({ 't' => 'timer', 'id' => 5 })
         Esq.dispatch_ui_event({ 't' => 'timer', 'id' => 5 })
         [$k, Esq.timers.key?(5)]",
        m,
    ) else {
        return;
    };
    assert_eq!(r, "[1, false]");
}

#[test]
fn timer_errors() {
    let Some(e) = error_of("UI.start_timer(1)") else { return };
    assert_eq!(e, "ArgumentError: tried to create Proc object without a block");
    let Some(e) = error_of("UI.start_timer { }") else { return };
    assert_eq!(e, "ArgumentError: Too few arguments");
    let Some(e) = error_of("UI.start_timer('1') { }") else { return };
    assert!(e.starts_with("TypeError:"), "{e}");
    let Some(e) = error_of("UI.stop_timer(nil)") else { return };
    assert!(e.starts_with("TypeError:"), "{e}");
    // Une exception dans la minuterie est signalée mais n'interrompt pas la distribution.
    let m = Mock::default().reply("ui.timer_add", json!(3));
    let Some((r, _)) = ok(
        "UI.start_timer(1) { raise 'raté' }
         Esq.dispatch_ui_event({ 't' => 'timer', 'id' => 3 })
         :suite",
        m,
    ) else {
        return;
    };
    assert_eq!(r, ":suite");
}

// ------------------------------------------------------------------------------------ HtmlDialog

#[test]
fn html_dialog_properties_and_defaults() {
    let Some((r, m)) = ok(
        "d = UI::HtmlDialog.new(dialog_title: 'Titre', width: 640.7, height: 480, resizable: false,
                                style: UI::HtmlDialog::STYLE_UTILITY, left: 10, top: 20, min_width: 100,
                                unknown: 1)
         d.set_html('<p>x</p>')
         d.show
         UI::HtmlDialog.new.show
         d.class",
        Mock::default(),
    ) else {
        return;
    };
    assert_eq!(r, "UI::HtmlDialog");
    let o = m.called("ui.html_open");
    let a = &o[0]["opts"];
    assert_eq!(a["dialog_title"], "Titre");
    assert_eq!((a["width"].as_i64(), a["height"].as_i64()), (Some(640), Some(480)));
    assert_eq!(a["resizable"], false);
    assert_eq!(a["scrollable"], true);
    assert_eq!((a["left"].as_i64(), a["top"].as_i64(), a["style"].as_i64()), (Some(10), Some(20), Some(2)));
    assert_eq!(a["min_width"], 100);
    assert!(a.get("unknown").is_none());
    assert_eq!(o[0]["content"], json!(["html", "<p>x</p>"]));
    // Valeurs par défaut de SketchUp : 250x250, défilement et redimensionnement permis, style « dialogue ».
    let d = &o[1]["opts"];
    assert_eq!((d["width"].as_i64(), d["height"].as_i64(), d["style"].as_i64()), (Some(250), Some(250), Some(0)));
    assert_eq!(
        (d["scrollable"].as_bool(), d["resizable"].as_bool(), d["use_content_size"].as_bool()),
        (Some(true), Some(true), Some(false))
    );
}

#[test]
fn html_dialog_argument_errors() {
    let Some(e) = error_of("UI::HtmlDialog.new('titre')") else { return };
    assert_eq!(e, "TypeError: wrong argument type String (expected Hash)");
    let Some(e) = error_of("UI::HtmlDialog.new.add_action_callback('x')") else { return };
    assert_eq!(e, "ArgumentError: Method requires block argument for callback");
    let Some(e) = error_of("UI::HtmlDialog.new.set_size('a', 1)") else { return };
    assert!(e.starts_with("TypeError:"), "{e}");
}

#[test]
fn html_dialog_return_values() {
    let Some((r, _)) = ok(
        "d = UI::HtmlDialog.new
         [d.add_action_callback('a') { }, d.execute_script('1'), d.set_can_close { true }, d.set_on_closed { },
          d.set_file('x.html'), d.set_html('x'), d.set_url('http://a'), d.show, d.bring_to_front, d.center,
          d.set_position(1, 2), d.set_size(3, 4), d.set_content_size(5, 6), d.hide, d.close, d.visible?]",
        Mock::default(),
    ) else {
        return;
    };
    assert_eq!(r, "[true, nil, nil, nil, nil, nil, nil, nil, nil, nil, nil, nil, nil, nil, nil, false]");
}

#[test]
fn html_dialog_callbacks_and_closing() {
    let Some((r, m)) = ok(
        "$log = []
         d = UI::HtmlDialog.new
         d.add_action_callback('plus') { |ctx, a, b| $log << [ctx.class.name, a, b]; 99 }
         d.add_action_callback('skp') { |ctx, param| $log << param }
         d.set_on_closed { $log << :closed }
         d.show
         id = d.__id
         Esq.dispatch_ui_event({ 't' => 'cb', 'd' => id, 'n' => 'plus', 'a' => [1, 'deux'], 'i' => 4 })
         Esq.dispatch_ui_event({ 't' => 'cb', 'd' => id, 'n' => 'skp', 'p' => 'a b' })
         Esq.dispatch_ui_event({ 't' => 'cb', 'd' => id, 'n' => 'absent', 'a' => [] })
         Esq.dispatch_ui_event({ 't' => 'closed', 'd' => id })
         Esq.dispatch_ui_event({ 't' => 'closed', 'd' => id })
         [$log, d.visible?, Esq.dialogs.key?(id)]",
        Mock::default(),
    ) else {
        return;
    };
    assert_eq!(r, "[[[\"UI::ActionCallbackContext\", 1, \"deux\"], \"a b\", :closed], false, false]");
    // La promesse de `sketchup.plus(...)` est résolue à la fin du rappel.
    let exec = m.called("ui.html_exec");
    assert!(exec.iter().any(|a| a["js"].as_str().unwrap().starts_with("window.__esq_settle(4,true,null)")), "{exec:?}");
}

#[test]
fn html_dialog_can_close_decides() {
    let Some((r, m)) = ok(
        "$allow = false
         d = UI::HtmlDialog.new
         d.set_can_close { $allow }
         d.show
         Esq.dispatch_ui_event({ 't' => 'close_request', 'd' => d.__id })
         $allow = true
         Esq.dispatch_ui_event({ 't' => 'close_request', 'd' => d.__id })
         :fait",
        Mock::default(),
    ) else {
        return;
    };
    assert_eq!(r, ":fait");
    assert_eq!(m.called("ui.html_close").len(), 1, "fermée une seule fois, quand le bloc a dit oui");
    assert_eq!(m.called("ui.html_open")[0]["can_close_hook"], true);
}

#[test]
fn html_dialog_geometry_comes_from_the_window() {
    let geo = json!({"visible": true, "x": 11, "y": 22, "w": 333, "h": 444, "cw": 300, "ch": 400});
    let m = Mock::default().reply("ui.html_geo", geo);
    let Some((r, _)) = ok(
        "d = UI::HtmlDialog.new(width: 300, height: 400)
         before = [d.get_size, d.get_position]
         d.show
         [before, d.get_size, d.get_position, d.get_content_size, d.visible?]",
        m,
    ) else {
        return;
    };
    // Avant l'ouverture, ce sont les propriétés demandées ; ensuite, ce que la fenêtre dit.
    assert_eq!(r, "[[[300, 400], [0, 0]], [333, 444], [11, 22], [300, 400], true]");
}

#[test]
fn html_dialog_modal_waits_until_closed() {
    let Some((r, m)) = ok(
        "$got = nil
         d = UI::HtmlDialog.new
         d.add_action_callback('fini') { |_c, v| $got = v; d.close }
         d.show_modal
         [$got, d.visible?]",
        Mock::default().wait(vec![]).wait(vec![json!({"t": "cb", "d": "$ID", "n": "fini", "a": ["ok"]})]),
    ) else {
        return;
    };
    assert_eq!(r, "[\"ok\", false]");
    assert_eq!(m.called("ui.html_open")[0]["modal"], true);
    assert!(m.wait_calls >= 3, "{}", m.wait_calls);
}

#[test]
fn html_dialog_is_not_modal_by_show() {
    let Some((_, m)) = ok("d = UI::HtmlDialog.new; d.show; d.show", Mock::default()) else { return };
    assert_eq!(m.called("ui.html_open").len(), 1, "la deuxième ouverture ne fait que ramener devant");
    assert_eq!(m.called("ui.html_front").len(), 1);
    assert_eq!(m.called("ui.html_open")[0]["modal"], false);
}

// ------------------------------------------------------------------------------------ WebDialog

#[test]
fn web_dialog_positional_arguments() {
    let Some((_, m)) = ok(
        "d = UI::WebDialog.new('Ancien', true, 'cle', 500, 300, 40, 50, false)
         d.show
         UI::WebDialog.new.show",
        Mock::default(),
    ) else {
        return;
    };
    let o = m.called("ui.html_open");
    let a = &o[0]["opts"];
    assert_eq!(a["dialog_title"], "Ancien");
    assert_eq!((a["scrollable"].as_bool(), a["preferences_key"].as_str()), (Some(true), Some("cle")));
    assert_eq!(
        (a["width"].as_i64(), a["height"].as_i64(), a["left"].as_i64(), a["top"].as_i64()),
        (Some(500), Some(300), Some(40), Some(50))
    );
    assert_eq!(a["resizable"], false);
    assert_eq!(o[0]["kind"], "web");
    // Par défaut : 250x250, sans défilement, redimensionnable ; (0, 0) = placement automatique.
    let d = &o[1]["opts"];
    assert_eq!(
        (d["width"].as_i64(), d["scrollable"].as_bool(), d["resizable"].as_bool(), d["left"].is_null()),
        (Some(250), Some(false), Some(true), true)
    );
}

#[test]
fn web_dialog_methods_and_returns() {
    let m = Mock::default().reply("ui.html_eval", json!("valeur")).reply("ui.html_capture", json!(true));
    let Some((r, m)) = ok(
        "d = UI::WebDialog.new
         pre = d.get_element_value('x')
         d.show
         [pre, d.add_action_callback('a') { }, d.set_on_close { }, d.set_url('u'), d.set_html('h'), d.set_file('f.html', nil),
          d.navigation_buttons_enabled?, (d.navigation_buttons_enabled = false), d.navigation_buttons_enabled?,
          d.get_default_dialog_color, d.set_background_color('#ff0000'), d.allow_actions_from_host('x.com'),
          d.set_full_security, d.bring_to_front, d.execute_script('1'), d.get_element_value('x'),
          [d.min_width, d.min_height, d.max_width, d.max_height], (d.min_width = 120), d.min_width,
          d.set_size(10, 20), d.set_position(3, 4), d.write_image('a.png'), d.post_url('http://x', 'a=1&b=2'),
          d.screen_scale_factor.class]",
        m,
    ) else {
        return;
    };
    assert_eq!(
        r,
        "[\"\", nil, nil, nil, nil, nil, true, false, false, \"#F0F0F0\", nil, nil, nil, nil, nil, \"valeur\", \
         [0, 0, 0, 0], 120, 120, nil, nil, true, true, Float]"
    );
    assert!(m.called("ui.html_capture")[0]["path"] == "a.png");
}

#[test]
fn web_dialog_callbacks_receive_the_dialog_and_params() {
    let Some((r, _)) = ok(
        "$log = []
         d = UI::WebDialog.new('t')
         d.add_action_callback('cb') { |dlg, params| $log << [dlg.equal?(d), params] }
         d.set_on_close { $log << :close }
         d.show
         Esq.dispatch_ui_event({ 't' => 'cb', 'd' => d.__id, 'n' => 'cb', 'p' => 'x=1&y=2' })
         Esq.dispatch_ui_event({ 't' => 'closed', 'd' => d.__id })
         $log",
        Mock::default(),
    ) else {
        return;
    };
    assert_eq!(r, "[[true, \"x=1&y=2\"], :close]");
    let Some(e) = error_of("UI::WebDialog.new.add_action_callback('x')") else { return };
    assert_eq!(e, "ArgumentError: Required proc not found");
    let Some(e) = error_of("UI::WebDialog.new.write_image") else { return };
    assert_eq!(e, "ArgumentError: File name required");
}

// ------------------------------------------------------------------------------------ InputBox

fn submit(values: &str) -> J {
    json!({"t": "cb", "d": "$ID", "n": "submit", "a": [values]})
}

#[test]
fn input_box_prompt_returns_a_hash_with_typed_values() {
    let mock = Mock::default().wait(vec![submit(r#"["12","1,5",true,"B"]"#)]);
    let Some((r, m)) = ok(
        "b = UI::InputBox.new(title: 'Réglages', accept_label: 'Appliquer')
         b.add_textbox(label: 'Entier', value: 3)
         b.add_textbox(label: 'Réel', value: 2.5)
         b.add_checkbox(label: 'Case', value: false)
         b.add_dropdown(label: 'Choix', options: %w[A B], value: 'A')
         b.add_separator; b.add_description('texte')
         b.prompt",
        mock,
    ) else {
        return;
    };
    assert_eq!(r, "{:Entier=>12, :Réel=>1.5, :Case=>true, :Choix=>\"B\"}");
    let o = m.called("ui.html_open");
    assert_eq!(o[0]["opts"]["dialog_title"], "Réglages");
    assert!(o[0]["content"][1].as_str().unwrap().contains("Appliquer"));
}

#[test]
fn input_box_returns_nil_when_cancelled() {
    let Some((r, _)) = ok("b = UI::InputBox.new('Titre'); b.add_textbox(label: 'x'); b.prompt", Mock::default()) else {
        return;
    };
    assert_eq!(r, "nil");
}

#[test]
fn input_box_field_errors_and_copies() {
    let Some(e) = error_of("UI::InputBox.new.add_textbox(label: '')") else { return };
    assert_eq!(e, "ArgumentError: Field label must be non-empty");
    let Some(e) = error_of("UI::InputBox.new.add_checkbox") else { return };
    assert_eq!(e, "ArgumentError: Field label must be non-empty");
    let Some(e) = error_of("UI::InputBox.new.add_dropdown(label: 'x', options: 3)") else { return };
    assert!(e.starts_with("TypeError: Invalid type for input field"), "{e}");
    let Some(r) = headless(
        "a = UI::InputBox.new('t'); a.add_textbox(label: 'x')
         b = a.dup; b.add_textbox(label: 'y'); c = a.clone
         [a.__fields.size, b.__fields.size, c.__fields.size, [a.add_slider(label: 's', range: 1..9), a.add_listbox(label: 'l', options: ['a']), a.add_image('i.png')]]",
    ) else {
        return;
    };
    assert_eq!(r, "[1, 2, 1, [nil, nil, nil]]");
}

#[test]
fn input_box_slider_and_listbox_results() {
    let mock = Mock::default().wait(vec![submit(r#"["7","b"]"#)]);
    let Some((r, _)) = ok(
        "b = UI::InputBox.new
         b.add_slider(label: 'Taux', range: 0..10, value: 3)
         b.add_listbox(label: 'Liste', options: %w[a b c], value: 'a')
         b.prompt",
        mock,
    ) else {
        return;
    };
    assert_eq!(r, "{:Taux=>7, :Liste=>\"b\"}");
}

#[test]
fn lengths_are_shown_in_model_units_and_parsed_back() {
    let mock = Mock::default().wait(vec![submit(r#"["25mm"]"#)]).wait(vec![]).wait(vec![submit(r#"["2m"]"#)]);
    let Some((r, m)) = ok(
        "a = UI.inputbox(['Longueur'], [10.mm])
         b = UI::InputBox.new
         b.add_textbox(label: 'L', value: 1.m)
         h = b.prompt
         [a.first.class, a.first.to_mm.round(6), h[:L].class, h[:L].to_m.round(6)]",
        mock,
    ) else {
        return;
    };
    assert_eq!(r, "[Length, 25.0, Length, 2.0]");
    // La valeur par défaut est affichée telle que `Length#to_s` la formate (unités du modèle).
    let html = m.called("ui.html_open")[0]["content"][1].as_str().unwrap().to_string();
    let shown = headless("10.mm.to_s").unwrap_or_default();
    assert!(html.contains(&format!("value='{}'", shown.trim_matches('"'))), "{shown} absent de la page");
}

#[test]
fn legacy_inputbox_types_follow_the_defaults() {
    let mock = Mock::default().wait(vec![submit(r#"["abc","2,5","9","Rouge"]"#)]);
    let Some((r, m)) =
        ok("UI.inputbox(['N', 'R', 'E', 'C'], ['x', 1.5, 3, 'Bleu'], ['', '', '', 'Rouge|Bleu'], 'Titre')", mock)
    else {
        return;
    };
    assert_eq!(r, "[\"abc\", 2.5, 9, \"Rouge\"]");
    assert_eq!(m.called("ui.html_open")[0]["opts"]["dialog_title"], "Titre");
}

#[test]
fn legacy_inputbox_title_in_third_position_and_missing_defaults() {
    let mock = Mock::default().wait(vec![submit(r#"["a","b"]"#)]);
    let Some((r, m)) = ok("UI.inputbox(['A', 'B'], ['seul'], 'Mon titre')", mock) else { return };
    assert_eq!(r, "[\"a\", \"b\"]");
    assert_eq!(m.called("ui.html_open")[0]["opts"]["dialog_title"], "Mon titre");
    let html = m.called("ui.html_open")[0]["content"][1].as_str().unwrap().to_string();
    assert!(html.contains("value='seul'") && html.contains("name='f1'"));
}

#[test]
fn form_html_escapes_every_interpolated_value() {
    // Une valeur hostile ne doit jamais sortir de son attribut ni ouvrir une balise.
    let Some(r) = headless(
        r#"bad = %q{x' onmouseover='alert(1)' q="<b>&}
           fields = [
             { kind: :text, label: bad, value: bad, type: 0 },
             { kind: :select, label: bad, options: [bad, 'ok'], value: bad, type: 0 },
             { kind: :list, label: bad, options: [bad], value: bad, type: 0 },
             { kind: :check, label: bad, value: true },
             { kind: :slider, label: bad, range: (0..10), value: bad, type: 1 },
             { kind: :desc, label: bad },
             { kind: :image, label: bad }
           ]
           html = Esq::Forms.page(bad, fields, bad, bad)
           [html.include?("onmouseover='"), html.include?('<b>'), html.include?(%q{q="}),
            html.scan('x&#39; onmouseover=&#39;alert(1)&#39; q=&quot;&lt;b&gt;&amp;').size >= 12,
            Esq.h(%q{'"<>&})]"#,
    ) else {
        return;
    };
    assert_eq!(r, "[false, false, false, true, \"&#39;&quot;&lt;&gt;&amp;\"]");
    // post_url : les champs cachés aussi.
    let Some((_, m)) =
        ok("d = UI::WebDialog.new; d.show; d.post_url(%q{http://x/'a}, %q{k%27=v%22%3C})", Mock::default())
    else {
        return;
    };
    let html = m.called("ui.html_load")[0]["value"].as_str().unwrap().to_string();
    assert!(html.contains("action='http://x/&#39;a'"), "{html}");
    assert!(html.contains("name='k&#39;' value='v&quot;&lt;'"), "{html}");
}

#[test]
fn legacy_inputbox_cancel_is_false_and_errors() {
    let Some((r, _)) = ok("UI.inputbox(['A'], ['x'])", Mock::default()) else { return };
    assert_eq!(r, "false");
    let Some(e) = error_of("UI.inputbox") else { return };
    assert_eq!(e, "ArgumentError: Too few arguments");
    let Some(e) = error_of("UI.inputbox('A')") else { return };
    assert_eq!(e, "TypeError: wrong argument type String (expected Array)");
    let Some(e) = error_of("UI.inputbox(['A'], 'x')") else { return };
    assert_eq!(e, "TypeError: wrong argument type String (expected Array)");
}

// ------------------------------------------------------------------------------------ Notification

#[test]
fn notification_buttons_run_their_blocks() {
    let Some((r, m)) = ok(
        "$log = []
         n = UI::Notification.new(SketchupExtension.new('Ext', 'ext.rb'), 'Mise à jour', 'icon.png', 'astuce')
         a = n.on_accept('Installer') { $log << :accept }
         d = n.on_dismiss('Plus tard') { $log << :dismiss }
         info = [n.message, n.icon_name, n.icon_tooltip, n.on_accept_title, n.on_dismiss_title]
         shown = n.show
         nid = n.instance_variable_get(:@id)
         Esq.dispatch_ui_event({ 't' => 'notif', 'id' => nid, 'action' => 'accept' })
         [a, d, info, shown, $log, Esq.notifs.key?(nid) ? 1 : 0]",
        Mock::default(),
    ) else {
        return;
    };
    assert_eq!(
        r,
        "[true, true, [\"Mise à jour\", \"icon.png\", \"astuce\", \"Installer\", \"Plus tard\"], true, [:accept], 0]"
    );
    let n = m.called("ui.notify");
    assert_eq!(
        (n[0]["message"].as_str(), n[0]["accept"].as_str(), n[0]["dismiss"].as_str()),
        (Some("Mise à jour"), Some("Installer"), Some("Plus tard"))
    );
}

#[test]
fn notification_errors_and_setters() {
    // Le premier argument doit être une extension (vérification de type de SketchUp).
    let Some(e) = error_of("UI::Notification.new(Object.new)") else { return };
    assert_eq!(e, "TypeError: wrong argument type Object (expected SketchupExtension)");
    let Some(e) = error_of("UI::Command.new('c') { }.extension = 'x'") else { return };
    assert_eq!(e, "TypeError: expected SketchupExtension");
    let Some(r) = headless(
        "c = UI::Command.new('c') { }; e = SketchupExtension.new('E', 'e.rb'); c.extension = e; c.extension.equal?(e)",
    ) else {
        return;
    };
    assert_eq!(r, "true");
    let Some(e) = error_of("UI::Notification.new(SketchupExtension.new('Ext', 'ext.rb')).on_accept('x')") else {
        return;
    };
    assert!(e.starts_with("LocalJumpError"), "{e}");
    let Some(e) =
        error_of("n = UI::Notification.new(SketchupExtension.new('Ext', 'ext.rb')); n.show; n.on_accept('x') { }")
    else {
        return;
    };
    assert_eq!(e, "RuntimeError: cannot change the on_accept block, the notification already has been shown.)");
    let Some(r) = headless(
        "n = UI::Notification.new(SketchupExtension.new('Ext', 'ext.rb'))
         n.message = 'm'; n.icon_name = 'i'; n.icon_tooltip = 't'
         [n.message, n.icon_name, n.icon_tooltip, n.on_accept_title]",
    ) else {
        return;
    };
    assert_eq!(r, "[\"m\", \"i\", \"t\", nil]");
}

// ------------------------------------------------------------------------------------ console

#[test]
fn console_writes_through_the_app() {
    let m = Mock::default().reply("ui.console", json!(true));
    let Some((r, m)) = ok(
        "c = SKETCHUP_CONSOLE
         [c.class, c.write('abc'), c << 'défg', c.write(12), c.write(''), c.clear, c.show, c.hide, c.flush.equal?(c), c.sync]",
        m,
    ) else {
        return;
    };
    assert_eq!(r, "[Sketchup::Console, 3, 4, 2, 0, nil, nil, nil, true, true]");
    let acts: Vec<&str> = m.called("ui.console").iter().map(|a| a["action"].as_str().unwrap()).collect();
    assert_eq!(acts, ["write", "write", "write", "clear", "show", "hide"]);
    let Some(e) = error_of("Sketchup::Console.new") else { return };
    assert!(e.starts_with("NoMethodError"), "{e}");
}

#[test]
fn console_falls_back_to_stdout_without_the_app() {
    let Some(i) = interp() else { return };
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut m = Model::new();
    let mut sel = Vec::new();
    let out = i.run("SKETCHUP_CONSOLE.write('bonjour'); SKETCHUP_CONSOLE.visible?", "c.rb", &mut m, &mut sel).unwrap();
    assert!(out.ok, "{}", out.error);
    assert_eq!(out.out, "bonjour");
    assert_eq!(out.result, "false");
}

// ------------------------------------------------------------------------------------ le reste de UI

#[test]
fn ui_misc_functions_forward_to_the_app() {
    let m = Mock::default()
        .reply("ui.openurl", json!(true))
        .reply("ui.clip_get", json!("texte"))
        .reply("ui.clip_set", json!(true))
        .reply("ui.scale_factor", json!(1.5))
        .reply("ui.inspector_show", json!(true))
        .reply("ui.timer_add", json!(1));
    let Some((r, m)) = ok(
        "[UI.openURL('https://sketchup.com'), UI.get_clipboard_data, UI.set_clipboard_data('x'), UI.scale_factor,
          UI.show_inspector('Materials'), UI.beep, UI.play_sound('a.wav'), UI.refresh_inspectors, UI.refresh_toolbars,
          UI.show_model_info('Units'), UI.show_preferences('General'), UI.show_extension_manager,
          UI.show_extension_warehouse, UI.scale_factor(1)]",
        m,
    ) else {
        return;
    };
    assert_eq!(r, "[true, \"texte\", true, 1.5, true, nil, nil, nil, nil, true, true, nil, nil, 1.5]");
    assert_eq!(m.called("ui.openurl")[0]["url"], "https://sketchup.com");
    assert_eq!(m.called("ui.prefs_show")[0]["page"], "General");
    assert_eq!(m.called("ui.inspector_show")[0]["name"], "Materials");
    let Some(e) = error_of("UI.scale_factor(1, 2)") else { return };
    assert!(e.starts_with("ArgumentError"), "{e}");
}

#[test]
fn ui_lists_match_sketchup() {
    let Some(r) = headless("[UI.inspector_names, UI.model_info_pages, UI.preferences_pages]") else { return };
    assert_eq!(
        r,
        "[[\"Materials\", \"Components\", \"Styles\", \"Scenes\", \"Shadows\", \"Layers\", \"SoftenEdges\", \"Outliner\", \
         \"Instructor\", \"Fog\", \"MatchPhoto\", \"EntityInfo\", \"Overlays\", \"Environments\"], \
         [\"Animation\", \"Classifications\", \"Components\", \"Credits\", \"Dimension\", \"File\", \"Location\", \
         \"Rendering\", \"Statistics\", \"Text\", \"Units\"], \
         [\"Applications\", \"Compatibility\", \"Drawing\", \"Files\", \"General\", \"OpenGL\", \"Shortcuts\", \
         \"Template\", \"Workspace\"]]"
    );
}

#[test]
fn file_panels_return_forward_slash_paths() {
    let m = Mock::default().reply("ui.openpanel", json!("C:\\Dossier\\a.rb")).reply("ui.savepanel", J::Null);
    let Some((r, m)) = ok("[UI.openpanel('Ouvrir', 'C:/', 'Ruby|*.rb||'), UI.savepanel('Sauver', 'C:/', 'x.skp')]", m)
    else {
        return;
    };
    assert_eq!(r, "[\"C:/Dossier/a.rb\", nil]");
    assert_eq!(m.called("ui.openpanel")[0]["filename"], "Ruby|*.rb||");
    assert_eq!(m.called("ui.savepanel")[0]["filename"], "x.skp");
    let m = Mock::default().reply("ui.select_directory", json!(["C:\\a", "C:\\b"]));
    let Some((r, m)) = ok("UI.select_directory(title: 'Dossiers', directory: 'C:/', select_multiple: true)", m) else {
        return;
    };
    assert_eq!(r, "[\"C:/a\", \"C:/b\"]");
    assert_eq!(m.called("ui.select_directory")[0]["multiple"], true);
    let Some((r, _)) = ok("UI.select_directory", Mock::default()) else { return };
    assert_eq!(r, "nil");
}

#[test]
fn cursors_are_registered() {
    let m = Mock::default().reply("ui.cursor_create", json!(3)).reply("ui.cursor_set", json!(true));
    let Some((r, m)) = ok("id = UI.create_cursor('c.png', 4, 5); [id, UI.set_cursor(id)]", m) else { return };
    assert_eq!(r, "[3, true]");
    let c = m.called("ui.cursor_create");
    assert_eq!((c[0]["x"].as_i64(), c[0]["y"].as_i64()), (Some(4), Some(5)));
    let Some(e) = error_of("UI.create_cursor('c.png', 1)") else { return };
    assert_eq!(e, "ArgumentError: Too few arguments");
}

#[test]
fn ui_pump_distributes_events_from_the_app() {
    let m = Mock::default().reply(
        "ui.poll",
        json!([{"t": "timer", "id": 8}, {"t": "timer", "id": 8}, {"t": "inconnu"}, {"t": "cb", "d": 424242, "n": "x", "a": []}]),
    );
    let Some((r, _)) = ok(
        "$t = 0
         Esq.timers[8] = { proc: proc { $t += 1 }, repeat: true }
         [Esq.ui_pump, $t]",
        m,
    ) else {
        return;
    };
    assert_eq!(r, "[4, 2]");
}
