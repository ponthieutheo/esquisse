//! Imports files with the default options and prints what came in:
//! `cargo run -p esq-formats --example import -- <file>...`.

fn main() {
    let o = esq_formats::AllOptions::default();
    for arg in std::env::args().skip(1) {
        let path = std::path::Path::new(&arg);
        let t = std::time::Instant::now();
        match esq_formats::import(path, &o, esq_core::model::Units::Millimeters) {
            Ok(imp) => {
                let m = &imp.model;
                let (mut faces, mut edges, mut insts, mut textured) = (0, 0, 0, 0);
                esq_core::walk::visit(m, |pl| {
                    faces += pl.ents.faces.len();
                    edges += pl.ents.edges.len();
                    insts += pl.ents.instances.len();
                    textured += pl.ents.faces.values().filter(|f| f.tex.is_some()).count();
                });
                let b = esq_formats::build::bounds(m);
                println!(
                    "{arg}: {faces} faces ({textured} texturées), {edges} arêtes, {insts} instances, {} définitions, {} matières, {} étiquettes, boîte {:?} ({:.0?})",
                    m.defs.len(),
                    m.materials.len(),
                    m.layers.len(),
                    b.map(|(lo, hi)| (hi - lo).to_array().map(|x| x.round())),
                    t.elapsed()
                );
                if std::env::var("ESQ_DEFS").is_ok() {
                    println!("  racine : {} arêtes, {} faces", m.root.edges.len(), m.root.faces.len());
                    for d in m.defs.values() {
                        println!(
                            "  {} : {} arêtes, {} faces, {} instances",
                            d.name,
                            d.entities.edges.len(),
                            d.entities.faces.len(),
                            d.entities.instances.len()
                        );
                    }
                }
                if let Some(r) = imp.report {
                    println!("{r}");
                }
            }
            Err(e) => println!("{arg}: ERREUR {e:#}"),
        }
    }
}
