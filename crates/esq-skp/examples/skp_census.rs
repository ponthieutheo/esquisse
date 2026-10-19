//! What the SDK exposes of a .skp's styles, scenes' styles, classifications, radial
//! dimensions, polylines, fonts, dimension style, texts' fonts and environments
//! (`esq_skp::sdk_census`), and, with `--rewrite [version]`, the same file written natively.
//!
//! `cargo run -p esq-skp --example skp_census -- fichier.skp [--rewrite 2017]`

fn print(c: &std::collections::BTreeMap<&'static str, Vec<String>>) {
    for (k, v) in c {
        for l in v {
            println!("{k} | {l}");
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::path::PathBuf::from(&args[0]);
    let c = esq_skp::sdk_census(&path).ok_or_else(|| anyhow::anyhow!("le SDK n'ouvre pas le fichier"))?;
    if let Some(i) = args.iter().position(|a| a == "--rewrite") {
        let v = args.get(i + 1).and_then(|n| esq_skp::SkpVersion::ALL.into_iter().find(|v| v.name() == *n));
        let (m, view) = esq_skp::native::read_bytes(&std::fs::read(&path)?)?;
        let bytes = esq_skp::native::write::write(&m, view.as_ref(), v, &Default::default())?;
        let out = std::env::temp_dir().join(format!("esq-census-{}.skp", std::process::id()));
        std::fs::write(&out, bytes)?;
        let d = esq_skp::sdk_census(&out).ok_or_else(|| anyhow::anyhow!("le SDK n'ouvre pas la réécriture"))?;
        let _ = std::fs::remove_file(&out);
        // SketchUp's own conversion: a class written as it makes it is no difference.
        esq_skp::sdk_resave(&path, &out, v)?;
        let theirs = esq_skp::sdk_census(&out).unwrap_or_default();
        let _ = std::fs::remove_file(&out);
        for (k, a) in &c {
            if theirs.get(k) == d.get(k) && a != &d[k] {
                println!("= {k} : comme la conversion de SketchUp");
                continue;
            }
            let (only_a, only_b) = esq_skp::native::canon::diff(a, &d[k]);
            for l in only_a {
                println!("- {k} | {l}");
            }
            for l in only_b {
                println!("+ {k} | {l}");
            }
        }
        return Ok(());
    }
    print(&c);
    Ok(())
}
