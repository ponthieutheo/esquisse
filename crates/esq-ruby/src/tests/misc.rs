use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use super::*;

/// Petit serveur HTTP local : une connexion = une réponse selon le chemin.
fn http_server() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || serve(s));
        }
    });
    port
}

fn serve(mut s: TcpStream) {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        let n = s.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let len: usize = head
        .lines()
        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0)))
        .unwrap_or(0);
    while buf.len() < head_end + len {
        let n = s.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let body = String::from_utf8_lossy(&buf[head_end..]).to_string();
    let mut first = head.lines().next().unwrap_or("").split_whitespace();
    let (method, path) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
    let xtest = head.lines().find_map(|l| l.strip_prefix("X-Test: ")).unwrap_or("").to_string();
    let reply = |status: &str, headers: &str, body: &[u8]| {
        let mut r = format!("HTTP/1.1 {status}\r\n{headers}Connection: close\r\n\r\n").into_bytes();
        r.extend_from_slice(body);
        r
    };
    let out = match path {
        "/hello" => {
            reply("200 OK", "Content-Type: text/plain; charset=utf-8\r\nContent-Length: 6\r\n", "Hé!".as_bytes())
        }
        "/latin" => {
            reply("200 OK", "Content-Type: text/plain; charset=ISO-8859-1\r\nContent-Length: 4\r\n", b"caf\xe9")
        }
        "/echo" => {
            let b = format!("{method}|{body}|{xtest}");
            reply("201 Created", &format!("Content-Length: {}\r\nX-Reply: ok\r\n", b.len()), b.as_bytes())
        }
        "/chunked" => reply("200 OK", "Transfer-Encoding: chunked\r\n", b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n"),
        "/redir" => reply("302 Found", "Location: /hello\r\nContent-Length: 0\r\n", b""),
        "/slow" => {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            reply("200 OK", "Content-Length: 4\r\n", b"late")
        }
        "/head" => reply("200 OK", "Content-Length: 99\r\n", b""),
        _ => reply("404 Not Found", "Content-Length: 4\r\n", b"nope"),
    };
    let _ = s.write_all(&out);
}

/// Ruby : attend (en vidant la file réseau) que `cond` devienne vrai.
const TRY: &str = "def try_; yield; 'ok'; rescue Exception => ex; \"#{ex.class}: #{ex.message}\"; end";

fn musttry(code: &str) -> Option<(RunOutput, Model)> {
    must(&format!(
        "{TRY}
{code}"
    ))
}

const WAIT: &str =
    "def wait_for(t = 5); n = (t / 0.02).to_i; n.times { return true if yield; Esq.pump; sleep 0.02 }; false; end";

#[test]
fn http_constants_and_errors() {
    let Some((o, _)) = must(
        "h = Sketchup::Http
         [h::GET, h::POST, h::PUT, h::DELETE, h::HEAD, h::OPTIONS, h::PATCH,
          h::STATUS_UNKNOWN, h::STATUS_SUCCESS, h::STATUS_PENDING, h::STATUS_CANCELED, h::STATUS_FAILED]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[\"GET\", \"POST\", \"PUT\", \"DELETE\", \"HEAD\", \"OPTIONS\", \"PATCH\", 0, 1, 2, 3, 4]");
    let msg = |code: &str| {
        must(&format!("begin; {code}; rescue Exception => e; \"#{{e.class}}: #{{e.message}}\"; end")).unwrap().0.result
    };
    assert_eq!(msg("Sketchup::Http::Request.new(5)"), "\"TypeError: wrong argument type Integer (expected String)\"");
    assert_eq!(msg("Sketchup::Http::Request.new(nil)"), "\"TypeError: wrong argument type nil (expected String)\"");
    assert_eq!(msg("Sketchup::Http::Request.new('http://x', 3)"), "\"TypeError: invalid Http verb argument)\"");
    assert_eq!(
        msg("Sketchup::Http::Request.new"),
        "\"ArgumentError: wrong number of arguments (given 0, expected 1..2)\""
    );
    assert_eq!(
        msg("Sketchup::Http::Request.new('http://x').body = 3"),
        "\"TypeError: wrong argument type Integer (expected String)\""
    );
    assert_eq!(
        msg("Sketchup::Http::Request.new('http://x').headers = []"),
        "\"TypeError: wrong argument type Array (expected Hash)\""
    );
    assert_eq!(
        msg("Sketchup::Http::Request.new('http://x').method = :get"),
        "\"TypeError: wrong argument type Symbol (expected String)\""
    );
    assert_eq!(msg("Sketchup::Http::Response.new"), "\"TypeError: allocator undefined for Sketchup::Http::Response\"");
    assert_eq!(
        msg("Sketchup::Http::Request.new('http://x').set_download_progress_callback"),
        "\"LocalJumpError: no block given\""
    );
    assert_eq!(
        msg("r = Sketchup::Http::Request.new('http://x'); r.headers = {'A' => '1', 'a' => '2'}; r.start"),
        "\"ArgumentError: Invalid headers - duplicate entries\""
    );
    let o = must(
        "r = Sketchup::Http::Request.new('http://x/y')
         [r.url, r.method, r.headers, r.body, r.status, r.cancel, r.status]",
    )
    .unwrap()
    .0;
    assert_eq!(o.result, "[\"http://x/y\", \"GET\", {}, \"\", 0, true, 0]");
}

#[test]
fn http_get_post_and_redirect() {
    if interp().is_none() {
        return;
    }
    let port = http_server();
    let o = must(&format!(
        "{WAIT}
         h = Sketchup::Http
         res = {{}}
         get = h::Request.new('http://127.0.0.1:{port}/hello')
         seen = nil
         r1 = get.start {{ |req, resp| seen = req.equal?(get); res[:get] = [resp.status_code, resp.body, resp.body.encoding.to_s, resp.headers['Content-Type'], req.status] }}
         st_pending = get.status
         post = h::Request.new('http://127.0.0.1:{port}/echo', h::POST)
         post.body = 'données'
         post.headers = {{'X-Test' => 'oui'}}
         post.start {{ |req, resp| res[:post] = [resp.status_code, resp.body, resp.headers['X-Reply']] }}
         red = h::Request.new('http://127.0.0.1:{port}/redir')
         red.start {{ |req, resp| res[:redir] = [resp.status_code, resp.body] }}
         chunk = h::Request.new('http://127.0.0.1:{port}/chunked')
         chunk.start {{ |req, resp| res[:chunk] = resp.body }}
         lat = h::Request.new('http://127.0.0.1:{port}/latin')
         lat.start {{ |req, resp| res[:lat] = [resp.body.encoding.to_s, resp.body.encode('UTF-8')] }}
         miss = h::Request.new('http://127.0.0.1:{port}/absent')
         miss.start {{ |req, resp| res[:miss] = [resp.status_code, resp.body, req.status] }}
         ok = wait_for {{ res.size == 6 }}
         [r1, st_pending, ok, seen, res[:get], res[:post], res[:redir], res[:chunk], res[:lat], res[:miss], get.status]"
    ))
    .unwrap()
    .0;
    assert!(o.error.is_empty() && o.out.is_empty(), "{} {}", o.error, o.out);
    assert_eq!(
        o.result,
        "[true, 2, true, true, [200, \"Hé!\", \"UTF-8\", \"text/plain; charset=utf-8\", 1], \
         [201, \"POST|données|oui\", \"ok\"], [200, \"Hé!\"], \"Wikipedia\", [\"ISO-8859-1\", \"café\"], [404, \"nope\", 1], 1]"
    );
}

#[test]
fn http_progress_start_twice_failure_and_cancel() {
    if interp().is_none() {
        return;
    }
    let port = http_server();
    let o = must(&format!(
        "{WAIT}
         h = Sketchup::Http
         down = []; up = []; done = nil
         r = h::Request.new('http://127.0.0.1:{port}/echo', h::PUT)
         r.body = 'x' * 40_000
         p1 = r.set_download_progress_callback {{ |cur, total| down << [cur, total] }}
         p2 = r.set_upload_progress_callback {{ |cur, total| up << [cur, total] }}
         r.start {{ |req, resp| done = resp.status_code }}
         twice = begin; r.start; rescue => e; \"#{{e.class}}: #{{e.message}}\"; end
         wait_for {{ done }}
         # connexion refusée -> STATUS_FAILED, réponse vide
         dead = TCPServer.new('127.0.0.1', 0) rescue nil
         free = Sketchup::Http::Request.new('http://127.0.0.1:1/')
         fail_resp = nil
         free.start {{ |req, resp| fail_resp = [resp.status_code, resp.body, resp.headers, req.status] }}
         wait_for {{ fail_resp }}
         # annulation d'une requête lente : aucun rappel, statut CANCELED
         slow = h::Request.new('http://127.0.0.1:{port}/slow')
         called = false
         slow.start {{ |*| called = true }}
         c = slow.cancel
         wait_for(2.2) {{ called }}
         # schéma inconnu
         bad = h::Request.new('ftp://example.com/x')
         bad_st = nil
         bad.start {{ |req, resp| bad_st = [req.status, resp.status_code] }}
         wait_for {{ bad_st }}
         [p1, p2, twice, done, down.last, up.last, up.size >= 1, fail_resp, c, slow.status, called, bad_st]"
    ))
    .unwrap()
    .0;
    assert_eq!(
        o.result,
        "[true, true, \"RuntimeError: start can only be called once per request)\", 201, [40005, 40005], [40000, 40000], true, \
         [0, \"\", {}, 4], true, 3, false, [4, 0]]"
    );
}

#[test]
fn http_head_and_options_verbs() {
    if interp().is_none() {
        return;
    }
    let port = http_server();
    let o = must(&format!(
        "{WAIT}
         r = Sketchup::Http::Request.new('http://127.0.0.1:{port}/head', Sketchup::Http::HEAD)
         out = nil
         r.start {{ |req, resp| out = [req.method, resp.status_code, resp.body, resp.headers['Content-Length']] }}
         wait_for {{ out }}
         out"
    ))
    .unwrap()
    .0;
    assert_eq!(o.result, "[\"HEAD\", 200, \"\", \"99\"]");
}

#[test]
fn sksocket_roundtrip() {
    if interp().is_none() {
        return;
    }
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = l.accept() {
            let mut b = [0u8; 64];
            if let Ok(n) = s.read(&mut b) {
                let mut r = b"re:".to_vec();
                r.extend_from_slice(&b[..n]);
                let _ = s.write_all(&r);
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    });
    let o = must(&format!(
        "{WAIT}
         got = +''
         n = SKSocket.add_socket_listener {{ |d| got << d }}
         con = SKSocket.connect('127.0.0.1', {port})
         # connect vide les écouteurs : on réinscrit après
         n2 = SKSocket.add_socket_listener {{ |d| got << d }}
         w = SKSocket.write('salut')
         wait_for {{ got.size >= 8 }}
         dis = SKSocket.disconnect
         after = SKSocket.write('rien')
         bad = SKSocket.connect('127.0.0.1', 1)
         [n, con, n2, w, got, dis, after, bad]"
    ))
    .unwrap()
    .0;
    assert_eq!(
        o.result,
        format!(
            "[1, \"Connected to {port}\", 1, \"SKSocket: salut\", \"re:salut\", \"Disconnected from {port}\", \"SKSocket: rien\", \"NOT connected to 1\"]"
        )
    );
}

#[test]
fn licensing() {
    let Some((o, _)) = must(
        "L = Sketchup::Licensing
         l = L.get_extension_license('org.exemple.rien')
         Esq.set_license('org.exemple.essai', L::TRIAL, 12, 'bientôt fini')
         t = L.get_extension_license('org.exemple.essai')
         Esq.set_license('org.exemple.ok', L::LICENSED, 0)
         ok = L.get_extension_license('org.exemple.ok')
         err = (L.get_extension_license(5) rescue \"#{$!.class}\")
         nonew = (L::ExtensionLicense.new rescue $!.class.to_s)
         [[L::LICENSED, L::EXPIRED, L::TRIAL, L::TRIAL_EXPIRED, L::NOT_LICENSED],
          [l.licensed?, l.state, l.days_remaining, l.error_description],
          [t.licensed?, t.state, t.days_remaining, t.error_description],
          [ok.licensed?, ok.state], err, nonew, L.frozen?, l.class.to_s]",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[[0, 1, 2, 3, 4], [false, 4, 0, \"\"], [true, 2, 12, \"bientôt fini\"], [true, 0], \"TypeError\", \"NoMethodError\", true, \
         \"Sketchup::Licensing::ExtensionLicense\"]"
    );
}

#[test]
fn amplitude_returns_the_module() {
    let Some((o, _)) = must(
        "A = Sketchup::Amplitude
         [A.get_all_guides.equal?(A), A.open_preview.equal?(A), A.update_preview('x').equal?(A), A.update_preview(3).equal?(A), A.close_preview.equal?(A)]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[true, true, true, true, true]");
}

#[test]
fn test_module_state_and_errors() {
    let Some((o, _)) = musttry(
        "d0 = Test.AutoDragDistance; t0 = Test.AutoClickTime
         old_d = Test.send(:AutoDragDistance=, 9.7); old_t = Test.send(:AutoClickTime=, 1)
         now = [Test.AutoDragDistance, Test.AutoClickTime]
         Test.AutoDragDistance = d0; Test.AutoClickTime = t0
         s0 = Test.suppress_warnings?; s1 = Test.send(:suppress_warnings=, 1); s2 = Test.send(:suppress_warnings=, nil)
         f0 = Test.show_fps(1); f1 = Test.show_fps(0); f2 = Test.show_fps(true)
         m0 = Test.min_feature_culling_size?; mo = Test.send(:min_feature_culling_size=, 3); m1 = Test.min_feature_culling_size?
         Test.min_feature_culling_size = 0
         tt = [Test.send(:ToolTips=, true), Test.send(:ToolTips=, false)]
         bb = [Test.draw_horizontal_bubble_lines?, Test.send(:draw_horizontal_bubble_lines=, true), Test.draw_horizontal_bubble_lines?]
         Test.draw_horizontal_bubble_lines = false
         cv = [Test.cv_enabled?, Test.send(:cv_enabled=, false), Test.cv_enabled?]
         Test.cv_enabled = true
         errs = [(try_ { Test.AutoDragDistance = nil }), (try_ { Test.AutoClickTime = 'x' })]
         [[d0, t0], [old_d, old_t], now, [s0, s1, s2], [f0, f1, f2], [m0, mo, m1], tt, bb, cv, errs,
          Test.CorruptModel, Test.project_textures_from_photo, Test.unregister_procedure('x'), Test.show_error_dialog,
          Test.repair_model, Test.eat_mem(10), Test.FindFile('/nonexistent-dir', 'a.txt')]",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[[6, 0.35], [6, 0.35], [9, 1.0], [false, true, false], [true, false, false], [0, 0, 3], [false, true], \
         [false, false, true], [true, nil, false], [\"TypeError: no implicit conversion from nil to integer\", \
         \"TypeError: no implicit conversion to float from string\"], false, false, false, nil, true, nil, false]"
    );
}

#[test]
fn test_extension_notifications() {
    let Some((o, _)) = must(
        "N = Test::ExtensionNotificationType; R = Test::ExtensionResult
         ok = Test.send_extension_notification(N::EXTENSION_UPDATED, R::RESULT_SUCCEEDED, 'org.exemple.x', sync_list: true)
         bad1 = (Test.send_extension_notification(7, 0, 'x') rescue \"#{$!.class}: #{$!.message}\")
         bad2 = (Test.send_extension_notification(0, 12, 'x') rescue \"#{$!.class}: #{$!.message}\")
         [[N::EXTENSION_INSTALLED, N::EXTENSION_UNINSTALLED, N::EXTENSION_UPDATED, N::EXTENSION_ENABLED, N::EXTENSION_DISABLED],
          [R::RESULT_SUCCEEDED, R::RESULT_FAILED, R::RESULT_FAILED_REMOVE_LICENSE, R::RESULT_FAILED_REMOVE_FILES,
           R::RESULT_FAILED_NO_ARCHIVE_HANDLER, R::RESULT_FAILED_WRITE_ACCESS, R::RESULT_FAILED_DOWNLOAD,
           R::RESULT_FAILED_UPDATE_LICENSE, R::RESULT_FAILED_UPDATE_VERSION_ID, R::RESULT_FAILED_ARCHIVE_READ],
          ok, bad1, bad2, Test.notifications.last[:sync_list]]",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[[0, 1, 2, 3, 4], [0, 1, 2, 3, 4, 5, 6, 7, 8, 9], nil, \"ArgumentError: Invalid extension notification type: 7\", \
         \"ArgumentError: Invalid extension notification result: 12\", true]"
    );
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("esq-misc-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn extension_registration_and_loading() {
    if interp().is_none() {
        return;
    }
    let d = temp_dir("ext");
    std::fs::create_dir_all(d.join("mon_ext")).unwrap();
    std::fs::write(
        d.join("mon_ext.rb"),
        "ext = SketchupExtension.new('Mon extension', 'mon_ext/main')
         ext.version = '1.2'; ext.creator = 'Théo'; ext.description = 'essai'; ext.copyright = '2026'
         $esq_ext_registered = Sketchup.register_extension(ext, true)
         $esq_ext = ext\n",
    )
    .unwrap();
    std::fs::write(d.join("mon_ext").join("main.rb"), "$esq_main_loaded = ($esq_main_loaded || 0) + 1\n").unwrap();
    std::fs::write(d.join("casse.rb"), "raise 'plugin cassé'\n").unwrap();
    let dir = d.to_string_lossy().replace('\\', "/");
    let o = must(&format!(
        "n = Esq.load_plugins({dir:?})
         e = $esq_ext
         m = Sketchup.extensions
         ids = [e.registered?, e.loaded?, e.load_on_start?, e.name, e.version, e.creator, e.description, e.copyright, e.id, e.version_id]
         e.uncheck; off = e.loaded?
         e.check; on = e.loaded?
         [n, $esq_ext_registered, $esq_main_loaded, ids, off, on, File.basename(e.extension_path),
          m[e.name].equal?(e), m['absent'], m[''], m[-1].equal?(e), m[99], m.keys.include?('Mon extension'),
          m.size == m.length, m.map(&:name).include?('Mon extension'), m.class.to_s, Esq.plugin_report.map {{ |r| [r['file'], r['ok']] }}]"
    ))
    .unwrap()
    .0;
    assert!(o.error.is_empty(), "{}", o.error);
    assert!(o.out.contains("casse.rb: RuntimeError: plugin cassé"), "{}", o.out);
    assert_eq!(
        o.result,
        "[1, true, 1, [true, true, true, \"Mon extension\", \"1.2\", \"Théo\", \"essai\", \"2026\", \"\", \"\"], false, true, \"main.rb\", \
         true, nil, nil, true, nil, true, true, true, \"Sketchup::ExtensionsManager\", [[\"casse.rb\", false], [\"mon_ext.rb\", true]]]"
    );
}

#[test]
fn register_extension_errors_and_duplicates() {
    let Some((o, _)) = musttry(
        "msg = ->(&b) { b.call; 'ok' rescue \"#{$!.class}: #{$!.message}\" }
         ext = SketchupExtension.new('Doublon', 'inexistant/chemin')
         first = Sketchup.register_extension(ext)           # sans 2e argument : non chargée au démarrage
         ext2 = SketchupExtension.new('Doublon', 'inexistant/chemin')
         second = Sketchup.register_extension(ext2, false)
         [try_ { Sketchup.register_extension },
          try_ { Sketchup.register_extension(Object.new) },
          try_ { SketchupExtension.new(1, 'x') },
          first, second, ext.load_on_start?, ext.loaded?, ext.registered?, ext2.registered?,
          try_ { ext.check },
          Sketchup.extensions.each { }.nil?,
          try_ { Sketchup.extensions.each },
          Sketchup.extensions.is_a?(Enumerable)]",
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[\"ArgumentError: Too few arguments: extension required\", \
         \"ArgumentError: An extension must respond to \\\"name\\\" and \\\"load\\\"\", \
         \"TypeError: no implicit conversion of Integer into String\", true, false, false, false, true, true, \
         \"LoadError: cannot load such file -- inexistant/chemin\", true, \"LocalJumpError: no block given (yield)\", true]"
    );
}

#[test]
fn support_files_and_load_require() {
    if interp().is_none() {
        return;
    }
    let d = temp_dir("sup");
    std::fs::create_dir_all(d.join("Components").join("Sous")).unwrap();
    std::fs::write(d.join("Components").join("a.skp"), "x").unwrap();
    std::fs::write(d.join("Components").join("Sous").join("b.SKP"), "x").unwrap();
    std::fs::write(d.join("Components").join("c.txt"), "x").unwrap();
    std::fs::create_dir_all(d.join("Resources").join("en-US")).unwrap();
    std::fs::write(d.join("Resources").join("en-US").join("icone.png"), "x").unwrap();
    std::fs::write(d.join("charge.rb"), "$esq_charge = ($esq_charge || 0) + 1\n").unwrap();
    std::fs::write(d.join("requis.rb"), "$esq_requis = ($esq_requis || 0) + 1\n").unwrap();
    let dir = d.to_string_lossy().replace('\\', "/");
    let o = musttry(&format!(
        "ENV['ESQ_RESOURCES'] = {dir:?}
         base = {dir:?}
         $LOAD_PATH.unshift(base)
         rel = ->(p) {{ p && p.sub(base, '') }}
         a = rel.(Sketchup.find_support_file('a.skp', 'Components'))
         nope = Sketchup.find_support_file('zzz.skp', 'Components')
         all = Sketchup.find_support_files('skp', 'Components').map {{ |p| rel.(p) }}
         res = rel.(Sketchup.get_resource_path('icone.png'))
         res_none = Sketchup.get_resource_path('absent.png')
         miss = (Sketchup.find_support_file rescue \"#{{$!.class}}: #{{$!.message}}\")
         miss2 = (Sketchup.find_support_files rescue $!.message)
         l1 = Sketchup.load('charge'); l2 = Sketchup.load('charge.rb')
         r1 = Sketchup.require('requis'); r2 = Sketchup.require('requis')
         lerr = try_ {{ Sketchup.load('introuvable') }}
         [a, nope, all, res, res_none, miss, miss2, l1, l2, $esq_charge, r1, r2, $esq_requis, lerr]"
    ))
    .unwrap()
    .0;
    assert!(o.error.is_empty(), "{}", o.error);
    assert_eq!(
        o.result,
        "[\"/Components/a.skp\", nil, [\"/Components/a.skp\", \"/Components/Sous/b.SKP\"], \"/Resources/en-US/icone.png\", \"\", \
         \"ArgumentError: File name missing\", \"File name missing\", true, true, 2, true, false, 1, \
         \"LoadError: cannot load such file -- introuvable\"]"
    );
}

#[test]
fn install_from_archive_and_misc_sketchup_functions() {
    if interp().is_none() {
        return;
    }
    // plugins_disabled= writes the preferences: a file of the test's own.
    let prefs = std::env::temp_dir()
        .join(format!("esq_prefs_misc_{}.json", std::process::id()))
        .to_string_lossy()
        .replace('\\', "/");
    let o = musttry(&format!(
        "ENV['ESQ_DEFAULTS_FILE'] = '{prefs}'
         e = ->(&b) {{ b.call; 'ok' rescue Exception; \"#{{$!.class}}: #{{$!.message}}\" }}
         r = [try_ {{ Sketchup.install_from_archive }},
          try_ {{ Sketchup.install_from_archive('C:/nulle-part.rbz', false) }},
          Sketchup.plugins_disabled?, Sketchup.send(:plugins_disabled=, 1), Sketchup.send(:plugins_disabled=, nil), Sketchup.plugins_disabled?,
          [true, false].include?(Sketchup.is_online)]
         ENV.delete('ESQ_DEFAULTS_FILE')
         r"
    ))
    .unwrap()
    .0;
    assert_eq!(
        o.result,
        "[\"ArgumentError: Too few arguments: path required\", \"Exception: Could not find: C:/nulle-part.rbz\", false, true, false, false, true]"
    );
}

/// A ZIP archive of stored entries `(name, data)`, as an `.rbz` is (with a Unix symbolic link
/// for the names in `links`).
fn zip_of(files: &[(&str, &[u8])], links: &[&str]) -> Vec<u8> {
    let (mut out, mut central) = (Vec::new(), Vec::new());
    for (name, data) in files {
        let off = out.len() as u32;
        let link = links.contains(name);
        let fields = |v: &mut Vec<u8>| {
            v.extend(20u16.to_le_bytes());
            v.extend([0u8; 2 + 2 + 4 + 4]);
            v.extend((data.len() as u32).to_le_bytes());
            v.extend((data.len() as u32).to_le_bytes());
            v.extend((name.len() as u16).to_le_bytes());
            v.extend(0u16.to_le_bytes());
        };
        out.extend(0x0403_4b50u32.to_le_bytes());
        fields(&mut out);
        out.extend(name.as_bytes());
        out.extend(*data);
        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend(if link { 0x031Eu16 } else { 20u16 }.to_le_bytes());
        fields(&mut central);
        central.extend([0u8; 6]);
        central.extend(if link { 0o120_777u32 << 16 } else { 0 }.to_le_bytes());
        central.extend(off.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let cd = out.len() as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(cd.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

/// Archives that would write outside the Plugins folder (`..`, absolute paths, a drive, a
/// network path, a device name, a symbolic link): the old installation handed them to
/// Windows' `tar`, which reads any archive format; now nothing is written, not even the
/// entries before the bad one.
#[test]
fn rbz_archives_cannot_write_outside_plugins() {
    let d = temp_dir("rbz-piege");
    let dest = d.join("Plugins");
    std::fs::create_dir_all(&dest).unwrap();
    let outside = d.join("dehors.rb");
    let traps: [&[(&str, &[u8])]; 9] = [
        &[("ok.rb", b"1"), ("../dehors.rb", b"x")],
        &[("ok.rb", b"1"), ("a/../../dehors.rb", b"x")],
        &[("ok.rb", b"1"), (r"..\dehors.rb", b"x")],
        &[("ok.rb", b"1"), ("/dehors.rb", b"x")],
        &[("ok.rb", b"1"), (r"C:\dehors.rb", b"x")],
        &[("ok.rb", b"1"), (r"\\esq-test-rbz\partage\dehors.rb", b"x")],
        &[("ok.rb", b"1"), ("CON.rb", b"x")],
        &[("ok.rb", b"1"), ("x.rb:flux", b"x")],
        &[("ok.rb", b"1"), ("lien", b"../../dehors.rb")],
    ];
    for (i, files) in traps.iter().enumerate() {
        let links: &[&str] = if i == 8 { &["lien"] } else { &[] };
        let z = zip_of(files, links);
        let r = crate::bridge::misc::extract_archive(&z, &dest);
        assert!(r.is_err(), "piège {i} accepté");
        assert!(!outside.exists() && !dest.join("ok.rb").exists(), "piège {i} : un fichier écrit");
    }
    // A plain archive is extracted, folders included.
    let z = zip_of(&[("ok.rb", b"1"), ("paquet/", b""), ("paquet/x.txt", b"x")], &[]);
    let w = crate::bridge::misc::extract_archive(&z, &dest).unwrap();
    assert_eq!(w.len(), 2);
    assert_eq!(std::fs::read(dest.join("paquet").join("x.txt")).unwrap(), b"x");
    // Not a ZIP (a tar, a rar…): refused instead of being handed to another reader.
    assert!(crate::bridge::misc::extract_archive(b"Rar!\x1a\x07\x00 pas un zip", &dest).is_err());
}

#[test]
fn install_rbz_archive() {
    if interp().is_none() {
        return;
    }
    // Un .rbz (zip), installé dans un dossier Plugins de remplacement.
    let d = temp_dir("rbz");
    let rbz = d.join("paquet.rbz");
    std::fs::write(&rbz, zip_of(&[("installe.rb", b"$esq_installe = true\n"), ("paquet/x.txt", b"x")], &[])).unwrap();
    let dest = d.join("Plugins");
    let o = must(&format!(
        "Esq.prepare_paths
         Esq.paths[:plugins] = {:?}
         r = Sketchup.install_from_archive({:?}, false)
         [r, $esq_installe, File.exist?(File.join(Esq.paths[:plugins], 'paquet', 'x.txt'))]",
        dest.to_string_lossy().replace('\\', "/"),
        rbz.to_string_lossy().replace('\\', "/")
    ))
    .unwrap()
    .0;
    assert_eq!(o.result, "[true, true, true]");
}
