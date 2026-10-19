//! Prints what the SDK reader and the native reader make of a .skp (format study):
//! `cargo run -p esq-skp --example skp_probe -- file.skp [classe]`.

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::path::PathBuf::from(args.first().ok_or_else(|| anyhow::anyhow!("chemin"))?);
    let class = args.get(1).cloned();
    let sdk = esq_skp::sdk_import_skp_view(&path)?.0;
    let opts = esq_skp::native::Options { sdk_quirks: true };
    let native = esq_skp::native::read_bytes_with(&std::fs::read(&path)?, &opts).map(|x| x.0);
    println!("SDK attr_store : {:?}", sdk.attr_store.keys().take(8).collect::<Vec<_>>());
    if let Ok(n) = &native {
        println!("natif attr_store : {:?}", n.attr_store.keys().take(8).collect::<Vec<_>>());
    }
    for m in sdk.materials.values() {
        println!("SDK matière {} {:?} {:?}", m.name, m.ext.colorize_deltas, m.ext.texture.as_ref().map(|t| t.avg));
    }
    let a = esq_skp::native::canon::canon(&sdk);
    let b = native.as_ref().map(esq_skp::native::canon::canon);
    for (c, lines) in &a {
        if class.as_deref().is_some_and(|x| x != *c) {
            continue;
        }
        for l in lines {
            let mark = match &b {
                Ok(b) if b[c].contains(l) => " ",
                _ => "≠",
            };
            println!("{mark} [{c}] {l}");
        }
        if let Ok(b) = &b {
            for l in &b[c] {
                if !lines.contains(l) {
                    println!("+ [{c}] {l}");
                }
            }
        }
    }
    if let Err(e) = native {
        println!("natif : {e}");
    }
    Ok(())
}
