//! Code Ruby écrit pour SketchUp, venu des dépôts publics de l'organisation SketchUp sur GitHub
//! (licence MIT), lancé dans Esquisse : la suite officielle de tests de l'API (TestUp) et les
//! extensions d'exemple. Voir `crates/esq-ruby/tests/data/ruby-reel.json`.
//!
//! Les dépôts restent hors du nôtre : `ESQ_RUBY_REEL` = le dossier qui les contient (un
//! sous-dossier par dépôt, ex. `%USERPROFILE%\tools\ruby-reel`). Sans lui, ces tests sont sautés ;
//! les écarts trouvés ont chacun leur test minimal, toujours lancé, dans les tests de leur zone.

use std::path::{Path, PathBuf};

use esq_ruby::{Interpreter, RunOutput};

use crate::app::EsquisseApp;

const RUNNER: &str = include_str!("ruby_reel/testup.rb");
const SAMPLES: &str = include_str!("ruby_reel/samples.rb");

/// Ce que chaque commande du menu « 3D Shapes » (github.com/SketchUp/sketchup-shapes) doit
/// laisser dans la définition qu'elle crée, d'après le code de l'extension et ce que SketchUp en
/// fait : (commande, faces, arêtes, arêtes adoucies, courbes). Les cotes valent l'unité de
/// l'extension (10 mm dans un modèle en millimètres).
const SHAPES: &[(&str, usize, usize, usize, usize)] = &[
    ("Box", 6, 12, 0, 0),
    ("Cylinder", 18, 48, 16, 2),
    ("Cone", 17, 32, 16, 1),
    ("Torus", 256, 512, 512, 0),
    ("Tube", 34, 96, 32, 4),
    ("Prism", 8, 18, 0, 2),
    ("Pyramid", 5, 8, 0, 1),
    ("Dome", 64, 128, 112, 0),
    ("Sphere", 128, 240, 240, 0),
    ("Helix", 0, 16, 0, 1),
];

/// Commandes de « 3D Shapes » qui lèvent aussi dans SketchUp 2026 : elles écrivent `TRUE`,
/// constante retirée de Ruby 3.2 (NameError).
const SHAPES_RUBY32: &[&str] = &["Helical Ramp", "Helical Ramp with Sides"];

/// Tests de la suite qui ne sont pas lancés, et pourquoi (`Classe#méthode` ou `Classe#*`).
const EXCLUDED: &[(&str, &str)] =
    &[("TC_Sketchup_Material#test_save_as_api_example", "écrit sur le Bureau de l'utilisateur (ENV['HOME']/Desktop)")];

fn reel_dir() -> Option<PathBuf> {
    let d = PathBuf::from(std::env::var_os("ESQ_RUBY_REEL")?);
    d.is_dir().then_some(d)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&p, &t);
        } else {
            copy_file(&p, &t);
        }
    }
}

/// Les fichiers du dépôt sont en lecture seule : la copie, elle, est à nous (et s'efface).
fn copy_file(from: &Path, to: &Path) {
    std::fs::copy(from, to).unwrap();
    let mut perm = std::fs::metadata(to).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    let _ = std::fs::set_permissions(to, perm);
}

fn ruby_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn script(app: &mut EsquisseApp, interp: &Interpreter, code: &str) -> RunOutput {
    crate::ruby_host::run_script(app, "TestUp", |m, s| interp.run(code, "testup", m, s)).unwrap()
}

/// Ce que rend une expression Ruby qui finit par `.unpack1('H*')` (du JSON en hexadécimal :
/// l'`inspect` d'une chaîne n'est pas toujours du JSON valide).
fn hex_json(o: &RunOutput) -> serde_json::Value {
    assert!(o.ok, "{}\n{}", o.error, o.out);
    let h = o.result.trim_matches('"');
    let bytes: Vec<u8> = (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect();
    serde_json::from_slice(&bytes).unwrap()
}

/// Sonde de mise au point : `ESQ_REEL_PROBE=<fichier.rb>` lance ce script dans l'application
/// (hôte complet) et affiche ce qu'il rend. Sauté sans la variable.
#[test]
fn reel_probe() {
    let Some(path) = std::env::var_os("ESQ_REEL_PROBE") else { return };
    let Some(interp) = super::ruby::interp() else { return };
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let code = std::fs::read_to_string(path).unwrap();
    let mut app = EsquisseApp::bare(None, None);
    for part in code.split("\n#---\n") {
        let o = script(&mut app, interp, part);
        eprintln!("ok={} result={}\nout={}\nerr={}", o.ok, o.result, o.out, o.error);
    }
}

/// Les extensions d'exemple de l'organisation SketchUp, chargées depuis un dossier Plugins
/// temporaire puis utilisées par leurs menus (formulaires validés tels qu'ils s'ouvrent, bureau
/// simulé). Sauté sans `ESQ_RUBY_REEL`.
#[test]
fn sample_extensions() {
    let Some(reel) = reel_dir() else {
        eprintln!("ESQ_RUBY_REEL absent : extensions d'exemple sautées");
        return;
    };
    let shapes = reel.join("sketchup-shapes").join("src");
    assert!(shapes.is_dir(), "{} introuvable", shapes.display());
    let Some(interp) = super::ruby::interp() else { return };
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let root = std::env::temp_dir().join(format!("esq-reel-ext-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for d in ["tmp", "home"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    // Un dossier Plugins avec plusieurs extensions, comme celui d'un utilisateur.
    let plugins = root.join("Plugins");
    copy_dir(&shapes, &plugins);
    copy_dir(&reel.join("sketchup-weld").join("src"), &plugins);
    copy_dir(&reel.join("sketchup-draw-mesh").join("src"), &plugins);

    let mut app = EsquisseApp::bare(None, None);
    for code in [RUNNER, SAMPLES] {
        let o = script(&mut app, interp, code);
        assert!(o.ok, "{}", o.error);
    }
    let o = script(&mut app, interp, &format!("EsqReel.start({:?})", ruby_path(&root)));
    assert!(o.ok, "{}", o.error);
    let o = script(
        &mut app,
        interp,
        &format!(
            "n = Esq.load_plugins({:?})
             l = ['Shapes Tool', 'SketchUp Weld', 'Draw Mesh Example'].map {{ |x| e = Sketchup.extensions[x]; e && e.loaded? }}
             JSON.generate([n, l, EsqReel.menu_commands('3D Shapes').map(&:first), Esq.plugin_report]).unpack1('H*')",
            ruby_path(&plugins)
        ),
    );
    let loaded = hex_json(&o);
    eprintln!("extensions chargées : {loaded}");
    assert_eq!(loaded[1], serde_json::json!([true, true, true]), "{loaded}");

    let mut gaps = Vec::new();
    for &(name, faces, edges, soft, curves) in SHAPES {
        let o = script(&mut app, interp, &format!("EsqReel.click('3D Shapes', {name:?}).unpack1('H*')"));
        let r = hex_json(&o);
        eprintln!("{name:<24} {r}");
        let d = &r["defs"][0];
        let got = (d["faces"].as_u64(), d["edges"].as_u64(), d["soft"].as_u64(), d["curves"].as_u64());
        let want = (Some(faces as u64), Some(edges as u64), Some(soft as u64), Some(curves as u64));
        if got != want || !r["error"].is_null() || r["forms"].as_array().map(Vec::len) != Some(1) {
            gaps.push(format!("{name} : attendu {want:?}, obtenu {got:?} ({r})"));
        }
    }
    for &name in SHAPES_RUBY32 {
        let o = script(&mut app, interp, &format!("EsqReel.click('3D Shapes', {name:?}).unpack1('H*')"));
        let r = hex_json(&o);
        eprintln!("{name:<24} {r}");
        if !r["error"].as_str().is_some_and(|e| e.starts_with("NameError")) {
            gaps.push(format!("{name} : NameError (TRUE) attendu, obtenu {r}"));
        }
    }
    // Weld (github.com/SketchUp/sketchup-weld) : ce que ses commandes doivent laisser.
    let o = script(&mut app, interp, "EsqReel.weld_session.unpack1('H*')");
    let weld = hex_json(&o);
    eprintln!("Weld : {weld}");
    let want = serde_json::json!({
        "weld": true,
        "curves": [1, [["Sketchup::Curve", 4]]],
        "Count Curves": [null, ["There are 1 curves in the current context."]],
        "Mark Curve Endpoints": [null, []],
        "Mark Edge PIDS": [null, []],
        "Colors Curves": [null, []],
        "Validate Model": [null, []],
        "Dump PIDs": [null, []],
        "cpoints": 2,
        "clines": [".", "_"],
        "texts": 4,
        "coloured": 4,
        "edge colour mode": 0,
    });
    if weld != want {
        gaps.push(format!("Weld : attendu {want}, obtenu {weld}"));
    }
    // Draw Mesh (github.com/SketchUp/sketchup-draw-mesh) : un outil qui dessine dans la vue le
    // maillage d'un carré de 10", décalé de 1,5 fois la largeur du modèle.
    let o = script(&mut app, interp, "EsqReel.drawmesh_session.unpack1('H*')");
    let started = hex_json(&o);
    let o = script(&mut app, interp, "EsqReel.drawmesh_draw.unpack1('H*')");
    let mesh = hex_json(&o);
    eprintln!("Draw Mesh : {started} {mesh}");
    let want = serde_json::json!({
        "tool": "Examples::DrawMesh::DrawMeshTool",
        "plain": [[4, 6, []]],
        "toggle": [null, null],
        "flags": [[0, 0], [8, 8]],
        "lit": [[4, 6, ["normals"]], [1, 12, []]],
        "extents": [[15.0, 0.0, 0.0], [25.0, 10.0, 0.0]],
    });
    if started["tool"] != serde_json::Value::Null || mesh != want {
        gaps.push(format!("Draw Mesh : attendu {want}, obtenu {started} {mesh}"));
    }
    let _ = script(&mut app, interp, "EsqReel.stop");
    let _ = std::fs::remove_dir_all(&root);
    assert!(gaps.is_empty(), "écarts :\n{}", gaps.join("\n"));
}

/// Remplace SKUI dans la copie de l'extension STL : son pont évalue des chaînes reçues de la
/// boîte web (`eval` de données), il n'est pas chargé. Seule la fenêtre d'options d'export s'en
/// sert ; l'export lui-même et l'import n'en ont pas besoin.
const SKUI_STUB: &str = "# Écrit pour Esquisse : SKUI n'est pas chargé.\n\
                         module SKUI\n  def self.embed_in(_mod); end\nend\n";

/// L'extension STL de l'organisation SketchUp (github.com/SketchUp/sketchup-stl), chargée comme
/// une extension, puis son export (ASCII, binaire) et son import d'une boîte de 10 × 20 × 30 mm.
/// Sauté sans `ESQ_RUBY_REEL`.
#[test]
fn sample_stl_extension() {
    let Some(reel) = reel_dir() else {
        eprintln!("ESQ_RUBY_REEL absent : extension STL sautée");
        return;
    };
    let src = reel.join("sketchup-stl").join("src");
    assert!(src.is_dir(), "{} introuvable", src.display());
    let Some(interp) = super::ruby::interp() else { return };
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let root = std::env::temp_dir().join(format!("esq-reel-stl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for d in ["tmp", "home"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    let plugins = root.join("Plugins");
    copy_dir(&src, &plugins);
    let skui = plugins.join("sketchup-stl").join("SKUI");
    std::fs::remove_dir_all(&skui).unwrap();
    std::fs::create_dir_all(&skui).unwrap();
    std::fs::write(skui.join("embed_skui.rb"), SKUI_STUB).unwrap();

    let mut app = EsquisseApp::bare(None, None);
    for code in [RUNNER, SAMPLES] {
        let o = script(&mut app, interp, code);
        assert!(o.ok, "{}", o.error);
    }
    let o = script(&mut app, interp, &format!("EsqReel.start({:?})", ruby_path(&root)));
    assert!(o.ok, "{}", o.error);
    let o = script(
        &mut app,
        interp,
        &format!(
            "n = Esq.load_plugins({:?})
             JSON.generate([n, Esq.plugin_report, EsqReel.menu_commands('File').map(&:first)]).unpack1('H*')",
            ruby_path(&plugins)
        ),
    );
    let loaded = hex_json(&o);
    eprintln!("STL chargé : {loaded}");
    let o =
        script(&mut app, interp, &format!("EsqReel.stl_round_trip({:?}).unpack1('H*')", ruby_path(&root.join("tmp"))));
    let r = hex_json(&o);
    eprintln!("STL : {r}");
    let _ = script(&mut app, interp, "EsqReel.stop");
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(loaded[0], 1, "{loaded}");
    let want = serde_json::json!({
        "box": [6, 12, [0.3937, 0.7874, 1.1811]],
        "ASCII": {"facets": 12, "max": [10.0, 20.0, 30.0]},
        "Binary": {"size": 84 + 50 * 12, "count": 12},
        "import ASCII": [true, 12, 18, [0.3937, 0.7874, 1.1811]],
        "import Binary": [true, 12, 18, [0.3937, 0.7874, 1.1811]],
        "messages": [],
    });
    assert_eq!(r, want);
}

/// La suite « SketchUp Ruby API » de TestUp 2 (github.com/SketchUp/testup-2, MIT), test par
/// test dans l'application. `ESQ_REEL_FILTER=TC_Geom,TC_Length` limite aux fichiers dont le
/// nom contient l'un des motifs ; `ESQ_REEL_OUT=<fichier.tsv>` y écrit le détail.
#[test]
fn testup_api_suite() {
    let Some(reel) = reel_dir() else {
        eprintln!("ESQ_RUBY_REEL absent : suite TestUp sautée");
        return;
    };
    let testup = reel.join("testup-2");
    let suite_src = testup.join("tests").join("SketchUp Ruby API");
    assert!(suite_src.is_dir(), "{} introuvable", suite_src.display());
    let Some(interp) = super::ruby::interp() else { return };
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let root = std::env::temp_dir().join(format!("esq-reel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    // La disposition du dépôt : `skippy.json` à la racine dit aux tests qu'ils ne tournent pas
    // dans les sources de SketchUp (TC_Sketchup_RenderingOptions#test_constants se saute).
    let suite = root.join("tests").join("SketchUp Ruby API");
    copy_dir(&suite_src, &suite);
    copy_file(&testup.join("skippy.json"), &root.join("skippy.json"));
    for d in ["tmp", "home", "lib/testup"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    std::fs::write(root.join("lib/testup/testcase.rb"), "EsqReel.define_testcase\n").unwrap();

    let filter: Vec<String> = std::env::var("ESQ_REEL_FILTER")
        .map(|f| f.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let mut files: Vec<String> = std::fs::read_dir(&suite)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("TC_") && n.ends_with(".rb"))
        .filter(|n| filter.is_empty() || filter.iter().any(|f| n.contains(f.as_str())))
        .collect();
    files.sort();
    let files: Vec<String> = files.iter().map(|n| ruby_path(&suite.join(n))).collect();
    let skip: serde_json::Map<String, serde_json::Value> =
        EXCLUDED.iter().map(|(k, v)| (k.to_string(), serde_json::Value::from(*v))).collect();

    let trace = std::env::var_os("ESQ_REEL_TRACE").is_some();
    // L'application dessine, comme SketchUp : View#write_image et les vignettes s'écrivent.
    let mut app = match crate::tests::gpu::render_state() {
        Some(rs) => {
            let mut a = EsquisseApp::bare(Some(crate::render::Viewport3d::new(&rs)), None);
            a.gpu = Some(rs);
            a
        }
        None => EsquisseApp::bare(None, None),
    };
    if trace {
        eprintln!("suite recopiée dans {}", root.display());
    }
    let o = script(&mut app, interp, RUNNER);
    assert!(o.ok, "{}", o.error);
    if trace {
        eprintln!("lanceur chargé");
    }
    let o = script(
        &mut app,
        interp,
        &format!(
            "JSON.generate(EsqReel.prepare({:?}, {:?}, JSON.parse({:?}), JSON.parse({:?}))).unpack1('H*')",
            ruby_path(&root),
            ruby_path(&testup.join("src")),
            serde_json::to_string(&files).unwrap(),
            serde_json::Value::Object(skip).to_string(),
        ),
    );
    let n = hex_json(&o).as_u64().unwrap() as usize;
    if trace {
        eprintln!("{n} tests");
    }
    let mut rows = Vec::with_capacity(n);
    let mut per_file: std::collections::BTreeMap<String, [usize; 5]> = Default::default();
    for i in 0..n {
        let label = hex_json(&script(&mut app, interp, &format!("EsqReel.label({i}).unpack1('H*')")));
        if trace {
            eprintln!("{i} {label}");
        }
        let o = script(&mut app, interp, &format!("EsqReel.run_one({i}).unpack1('H*')"));
        let (status, msg) = if o.ok {
            let r = hex_json(&o);
            (r[0].as_str().unwrap_or("").to_string(), r[1].as_str().unwrap_or("").to_string())
        } else {
            ("error".to_string(), format!("lanceur : {}", o.error.lines().next().unwrap_or("")))
        };
        let file = label[0].as_str().unwrap_or("").to_string();
        let slot = match status.as_str() {
            "pass" => 0,
            "fail" => 1,
            "error" => 2,
            "skip" => 3,
            _ => 4,
        };
        per_file.entry(file.clone()).or_default()[slot] += 1;
        rows.push(format!(
            "{file}\t{}\t{}\t{status}\t{}",
            label[1].as_str().unwrap_or(""),
            label[2].as_str().unwrap_or(""),
            msg.replace(['\t', '\n'], " ")
        ));
    }
    let end = hex_json(&script(&mut app, interp, "EsqReel.finish.unpack1('H*')"));
    let (load_errors, ui_log) = (&end[0], &end[1]);
    let mut total = [0usize; 5];
    for (f, c) in &per_file {
        eprintln!(
            "{f:<45} réussis {:>4}  échecs {:>4}  erreurs {:>4}  sautés {:>3}  exclus {:>3}",
            c[0], c[1], c[2], c[3], c[4]
        );
        for (t, x) in total.iter_mut().zip(c) {
            *t += x;
        }
    }
    eprintln!(
        "TestUp « SketchUp Ruby API » : {} réussis / {n} ({} échecs, {} erreurs, {} sautés, {} exclus)",
        total[0], total[1], total[2], total[3], total[4]
    );
    if let Some(a) = load_errors.as_array().filter(|a| !a.is_empty()) {
        eprintln!("fichiers non chargés : {a:?}");
    }
    eprintln!("appels simulés au bureau (boîtes, presse-papiers…) : {ui_log}");
    if let Some(out) = std::env::var_os("ESQ_REEL_OUT") {
        std::fs::write(out, rows.join("\n") + "\n").unwrap();
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// `Model#active_path=` comme, dans l'application (le chemin est celui du
/// document) : la feuille d'un chemin qui n'est pas une instance est laissée de côté ; instance
/// effacée : TypeError ; chemin invalide, instance verrouillée (ou une autre instance de sa
/// définition) : ArgumentError ; hors opération le changement en est une (onTransactionStart,
/// onTransactionCommit), dans une opération celle-ci est validée et rouverte autour ; une
/// opération qui n'a rien changé s'annonce vide ; les instances ouvertes et fermées entendent
/// onOpen et onClose (TestUp `TC_Sketchup_Model#test_active_path_Set_*`).
#[test]
fn active_paths_open_like_sketchup() {
    let Some(interp) = super::ruby::interp() else { return };
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = EsquisseApp::bare(None, None);
    let o = script(
        &mut app,
        interp,
        "def cls; yield; nil; rescue Exception => e; e.class; end
         def err; yield; nil; rescue Exception => e; \"#{e.class}: #{e.message}\"; end
         m = Sketchup.active_model
         log = []
         obs = Class.new(Sketchup::ModelObserver) {
           define_method(:initialize) { |l| @l = l }
           %w[onTransactionStart onTransactionCommit onTransactionEmpty onActivePathChanged].each { |n| define_method(n) { |_m| @l << n } }
         }.new(log)
         g1 = m.entities.add_group; g2 = g1.entities.add_group
         f = g2.entities.add_face([0, 0, 0], [9, 0, 0], [9, 9, 0])
         io = []
         iobs = Class.new(Sketchup::InstanceObserver) {
           define_method(:initialize) { |l| @l = l }
           define_method(:onOpen) { |i| @l << [:open, i] }
           define_method(:onClose) { |i| @l << [:close, i] }
         }.new(io)
         g2.add_observer(iobs)
         m.add_observer(obs)
         m.active_path = Sketchup::InstancePath.new([g1, g2, f])
         r = [m.active_path == [g1, g2], log.dup, io.map { |e, i| [e, i == g2] }]
         log.clear; io.clear
         m.start_operation('Ouvrir', true)
         m.active_path = [g1]
         m.commit_operation
         r += [log.dup, io.map { |e, i| [e, i == g2] }]
         log.clear
         m.start_operation('Cube', true)
         m.entities.add_cpoint([1, 2, 3])
         m.active_path = nil
         m.commit_operation
         r << log.dup << m.active_path
         m.remove_observer(obs)
         g2.remove_observer(iobs)
         g3 = m.entities.add_group; g3.entities.add_cpoint(ORIGIN); g3.locked = true
         r += [cls { m.active_path = 123 }, cls { m.active_path = [g1] * 3 }, cls { m.active_path = [g3] }]
         s = m.entities.add_instance(g2.definition, IDENTITY); s.locked = true
         r << err { m.active_path = [g1, g2] }
         g4 = m.entities.add_group; g4.entities.add_cpoint(ORIGIN); p4 = Sketchup::InstancePath.new([g4]); g4.erase!
         r + [cls { m.active_path = p4 }, m.active_path]",
    );
    assert!(o.ok, "{}", o.error);
    assert_eq!(
        o.result,
        "[true, [\"onTransactionStart\", \"onTransactionCommit\", \"onActivePathChanged\"], [[:open, true]], \
         [\"onTransactionStart\", \"onTransactionCommit\", \"onTransactionStart\", \"onActivePathChanged\", \"onTransactionEmpty\"], \
         [[:close, true]], [\"onTransactionStart\", \"onTransactionCommit\", \"onTransactionStart\", \"onTransactionCommit\", \
         \"onTransactionStart\", \"onActivePathChanged\", \"onTransactionEmpty\"], nil, TypeError, ArgumentError, ArgumentError, \
         \"ArgumentError: Cannot open instance path with locked sibling instances\", TypeError, nil]"
    );
}
