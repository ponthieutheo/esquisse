use super::*;

/// One interpreter per process (Ruby cannot be started twice); tests share it and are
/// skipped on machines without a Ruby 3.2 runtime.
fn interp() -> Option<&'static Interpreter> {
    static I: OnceLock<Option<Interpreter>> = OnceLock::new();
    I.get_or_init(|| match Interpreter::start() {
        Ok(i) => {
            // Tests expect "." whatever the machine's regional settings say.
            let pin = "Sketchup::RegionalSettings.instance_variable_set(:@seps, { 'decimal' => '.', 'list' => ',' })";
            let out = i.run(pin, "pin.rb", &mut Model::new(), &mut Vec::new()).unwrap();
            assert!(out.ok, "{}", out.error);
            Some(i)
        }
        Err(e) => {
            assert!(!candidate_dlls().iter().any(|p| p.is_file()), "Ruby présent mais inutilisable: {e:#}");
            // A check that must cover Ruby (merges) sets ESQ_REQUIRE_RUBY: no silent skip then.
            assert!(std::env::var_os("ESQ_REQUIRE_RUBY").is_none(), "Ruby exigé (ESQ_REQUIRE_RUBY) : {e:#}");
            eprintln!("Ruby indisponible, tests ignorés: {e:#}");
            None
        }
    })
    .as_ref()
}

fn run(code: &str) -> Option<(RunOutput, Model)> {
    let i = interp()?;
    let mut m = Model::new();
    let mut sel = Vec::new();
    let out = i.run(code, "test.rb", &mut m, &mut sel).unwrap();
    Some((out, m))
}

fn must(code: &str) -> Option<(RunOutput, Model)> {
    let (o, m) = run(code)?;
    assert!(o.ok, "erreur Ruby: {}", o.error);
    Some((o, m))
}

#[test]
fn evaluates_and_prints() {
    let Some((o, _)) = must("puts 'bonjour'; 6 * 7") else { return };
    assert_eq!(o.result, "42");
    assert_eq!(o.out, "bonjour\n");
}

/// `Esq.__native` called by an extension with something else than two strings: the
/// conversion (`rb_string_value_cstr`) raised from inside the Rust callback, and Ruby's
/// `longjmp` crossed Rust frames with destructors pending (undefined behaviour, a crash or
/// a leak). The native side now answers an error instead; same for an exception whose
/// `inspect` raises.
#[test]
fn the_native_bridge_refuses_what_is_not_a_string() {
    let Some((o, _)) = must(
        "o = Object.new
         def o.to_str; raise 'non'; end
         sub = Class.new(String).new('take')
         r = [Esq.__native(1, nil), Esq.__native('misc.paths', \"a\\0b\"), Esq.__native(o, ''), Esq.__native(\"take\\0\", '')]
         r = r.map { |x| JSON.parse(x)['err'].to_s.start_with?('ArgumentError') }
         r << JSON.parse(Esq.__native(sub, '')).key?('ok')
         r",
    ) else {
        return;
    };
    assert_eq!(o.result, "[true, true, true, true, true]");
    let Some((o, _)) = run("e = Class.new(StandardError) { def inspect; raise 'encore'; end }; raise e, 'x'") else {
        return;
    };
    assert!(!o.ok);
}

#[test]
fn reports_exceptions_with_backtrace() {
    let Some((o, _)) = run("raise ArgumentError, 'oups'") else { return };
    assert!(!o.ok);
    assert!(o.error.contains("ArgumentError: oups"), "{}", o.error);
}

#[test]
fn geom_math() {
    let Some((o, _)) = must(
        "t = Geom::Transformation.translation([1,2,3]) * Geom::Transformation.rotation(Geom::ORIGIN, Geom::Z_AXIS, 90.degrees)
         p = t * Geom::Point3d.new(1, 0, 0)
         [p.x.round(6), p.y.round(6), p.z.round(6), (t.inverse * p).to_a.map { |v| v.round(6) }]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[1.0, 3.0, 3.0, [1.0, 0.0, 0.0]]");
}

#[test]
fn units_are_inches() {
    let Some((o, _)) = must("[1.m.round(4), 25.4.mm.round(6), '10cm'.to_l.round(4)]") else { return };
    assert_eq!(o.result, "[39.3701, 1.0, 3.937]");
}

#[test]
fn draws_a_face_and_pushpulls_it() {
    let Some((o, m)) = must(
        "ents = Sketchup.active_model.entities
         f = ents.add_face([0,0,0], [100.mm,0,0], [100.mm,100.mm,0], [0,100.mm,0])
         f.pushpull(50.mm)
         [ents.grep(Sketchup::Face).size, ents.grep(Sketchup::Edge).size]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[6, 12]");
    assert_eq!(m.root.faces.len(), 6);
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for v in m.root.verts.values() {
        lo = lo.min(v.pos.z);
        hi = hi.max(v.pos.z);
    }
    assert!((hi - lo - 50.0).abs() < 1e-6, "hauteur {}", hi - lo);
}

#[test]
fn face_area_and_normal() {
    let Some((o, _)) = must(
        "f = Sketchup.active_model.entities.add_face([0,0,0], [10.mm,0,0], [10.mm,20.mm,0], [0,20.mm,0])
         [f.area.round(6), f.normal.to_a.map(&:round), f.vertices.size]",
    ) else {
        return;
    };
    // 10 mm x 20 mm = 200 mm² = 200 / 645.16 in²
    assert!(o.result.starts_with("[0.31"), "{}", o.result);
    assert!(o.result.contains("[0, 0, 1]") || o.result.contains("[0, 0, -1]"), "{}", o.result);
}

#[test]
fn groups_and_transformations() {
    let Some((o, m)) = must(
        "model = Sketchup.active_model
         g = model.entities.add_group
         g.entities.add_face([0,0,0], [1,0,0], [1,1,0], [0,1,0])
         g.transformation = Geom::Transformation.translation([5, 0, 0])
         g.name = 'caisse'
         [g.class.name, g.name, g.transformation.origin.x, g.entities.grep(Sketchup::Face).size]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[\"Sketchup::Group\", \"caisse\", 5.0, 1]");
    assert_eq!(m.root.instances.len(), 1);
    let inst = m.root.instances.values().next().unwrap();
    assert!((inst.xform.w_axis.x - 5.0 * 25.4).abs() < 1e-9);
}

#[test]
fn circles_materials_and_selection() {
    let Some((o, m)) = must(
        "model = Sketchup.active_model
         edges = model.entities.add_circle([0,0,0], [0,0,1], 1.m, 32)
         mat = model.materials.add('Brique')
         mat.color = 'red'
         face = model.entities.add_face(edges)
         face.material = mat
         model.selection.add(face)
         [edges.size, face.material.name, model.selection.size, mat.color.red]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[32, \"Brique\", 1, 255]");
    assert_eq!(m.materials.len(), 1);
}

#[test]
fn menu_items_are_registered_and_invocable() {
    let Some(i) = interp() else { return };
    let mut m = Model::new();
    let mut sel = Vec::new();
    let out = i
        .run("UI.menu('Plugins').add_item('Dessiner un carré') { Sketchup.active_model.entities.add_face([0,0,0],[1,0,0],[1,1,0],[0,1,0]) }", "plug.rb", &mut m, &mut sel)
        .unwrap();
    assert!(out.ok, "{}", out.error);
    let cmds = i.commands(&mut m, &mut sel).unwrap();
    let c = cmds.iter().find(|c| c.title == "Dessiner un carré").expect("commande enregistrée");
    assert_eq!(c.menu, "Extensions");
    let r = i.invoke(c.id, &mut m, &mut sel).unwrap();
    assert!(r.ok, "{}", r.error);
    assert_eq!(m.root.faces.len(), 1);
}

#[test]
fn erased_entities_are_invalid() {
    let Some((o, _)) = must(
        "f = Sketchup.active_model.entities.add_face([0,0,0],[1,0,0],[1,1,0])
         f.erase!
         f.valid?",
    ) else {
        return;
    };
    assert_eq!(o.result, "false");
}

/// Fraction of SketchUp's Ruby API (as read from SketchUp.exe, `docs/ruby-api.md`) that
/// Esquisse defines. Set ESQ_WRITE_COVERAGE=1 to write the per-class report to
/// `docs/ruby-coverage.md`.
#[test]
fn api_coverage() {
    let Some(i) = interp() else { return };
    let md = include_str!("../../../docs/ruby-api.md");
    let mut catalog = serde_json::Map::new();
    let mut cur: Option<String> = None;
    for line in md.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            let name = h.split(" (").next().unwrap_or(h);
            cur = Some(name.to_string());
        } else if let Some(c) = &cur
            && !line.is_empty()
            && !line.starts_with('#')
        {
            let ms: Vec<J> = line.split(", ").map(|s| json!(s.trim())).collect();
            catalog.insert(c.clone(), J::Array(ms));
        }
    }
    let code = format!(
        r#"
        cat = JSON.parse({:?})
        ns = {{ 'Sketchup::Color' => nil }}
        alias_of = Hash.new {{ |h, k| k }}
        %w[BoundingBox Bounds2d Point2d Point3d PolygonMesh Transformation Transformation2d Vector2d Vector3d LatLong UTM].each {{ |n| alias_of[n] = "Geom::#{{n}}" }}
        alias_of['Color'] = 'Sketchup::Color'
        alias_of['rb_cArray'] = 'Array'; alias_of['rb_cNumeric'] = 'Numeric'; alias_of['rb_cString'] = 'String'
        alias_of['Request'] = 'Sketchup::Http::Request'; alias_of['Response'] = 'Sketchup::Http::Response'
        alias_of['ExtensionLicense'] = 'Sketchup::Licensing::ExtensionLicense'
        res = {{}}
        cat.each do |cn, ms|
          klass = (Object.const_get(alias_of[cn]) rescue nil)
          if klass.nil?
            res[cn] = {{ 'total' => ms.size, 'found' => 0, 'missing' => ['(classe absente)'] }}
            next
          end
          miss = []
          ms.each do |m|
            ok = if m.start_with?('CONST ') then klass.const_defined?(m.sub('CONST ', ''))
                 elsif m.start_with?('self.') then klass.respond_to?(m[5..].split('/').first, true)
                 else n = m.split('/').first.to_sym
                      klass.method_defined?(n) || klass.private_method_defined?(n) end
            miss << m unless ok
          end
          res[cn] = {{ 'total' => ms.size, 'found' => ms.size - miss.size, 'missing' => miss }}
        end
        JSON.generate(res)
        "#,
        serde_json::to_string(&J::Object(catalog)).unwrap()
    );
    let mut m = Model::new();
    let mut sel = Vec::new();
    let out = i.run(&code, "coverage.rb", &mut m, &mut sel).unwrap();
    assert!(out.ok, "{}", out.error);
    let inner: String = serde_json::from_str(&out.result).unwrap();
    let res: serde_json::Map<String, J> = serde_json::from_str(&inner).unwrap();
    let (mut total, mut found) = (0u64, 0u64);
    let mut rows: Vec<(String, u64, u64, String)> = Vec::new();
    for (k, v) in &res {
        let (t, f) = (v["total"].as_u64().unwrap(), v["found"].as_u64().unwrap());
        total += t;
        found += f;
        let miss: Vec<&str> = v["missing"].as_array().unwrap().iter().filter_map(J::as_str).collect();
        rows.push((k.clone(), f, t, miss.join(", ")));
    }
    if let Ok(pat) = std::env::var("ESQ_COVERAGE_FILTER") {
        for (k, f, t, miss) in rows.iter().filter(|r| r.0.contains(&pat)) {
            eprintln!("{k} {f}/{t} manque : {miss}");
        }
    }
    let pct = 100.0 * found as f64 / total as f64;
    eprintln!("API Ruby couverte : {found}/{total} ({pct:.1} %)");
    if std::env::var("ESQ_WRITE_COVERAGE").is_ok() {
        let mut s = format!(
            "# Couverture de l'API Ruby de SketchUp\n\n{found}/{total} méthodes ({pct:.1} %). Généré par `ESQ_WRITE_COVERAGE=1 cargo test -p esq-ruby api_coverage`.\n\n| Classe | Couvert | Manquant |\n|---|---|---|\n"
        );
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, f, t, miss) in &rows {
            s.push_str(&format!("| {k} | {f}/{t} | {miss} |\n"));
        }
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/ruby-coverage.md"), s).unwrap();
    }
    // Floor: the coverage must not go down (raise it when the API grows).
    assert!(pct >= 100.0, "couverture {pct:.1} %");
}

#[allow(unused_imports)]
mod audit;
#[allow(unused_imports)]
mod audit2;
#[allow(unused_imports)]
mod entities;
#[allow(unused_imports)]
mod geom;
#[allow(unused_imports)]
mod instances;
#[allow(unused_imports)]
mod misc;
#[allow(unused_imports)]
mod model;
#[allow(unused_imports)]
mod reel;
#[allow(unused_imports)]
mod textures;
#[allow(unused_imports)]
mod tools;
#[allow(unused_imports)]
mod ui;
#[allow(unused_imports)]
mod view;
