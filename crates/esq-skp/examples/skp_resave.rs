//! SketchUp's own conversion of a file (the SDK opens it and saves it in a version): what the
//! application would write, to compare with Esquisse's writer.
//!
//! `cargo run -p esq-skp --example skp_resave -- fichier.skp sortie.skp [2017]`

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let v = args.get(2).and_then(|n| esq_skp::SkpVersion::ALL.into_iter().find(|v| v.name() == *n));
    esq_skp::sdk_resave(std::path::Path::new(&args[0]), std::path::Path::new(&args[1]), v)
}
