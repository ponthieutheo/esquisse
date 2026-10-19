//! Lists the entities of a DWG/DXF as cadkit reads them (debugging aid):
//! `cargo run -p esq-formats --example caddump -- <file>`.

fn main() {
    for arg in std::env::args().skip(1) {
        let Ok(bytes) = std::fs::read(&arg) else { continue };
        let doc = match esq_formats::cad::read(&bytes) {
            Ok(d) => d,
            Err(e) => {
                println!("{arg}: {e:#}");
                continue;
            }
        };
        println!(
            "{arg}: unités {:?}, {} calques, {} blocs, {} espaces",
            doc.units,
            doc.layers.len(),
            doc.blocks.len(),
            doc.models.len()
        );
        for m in &doc.models {
            println!(" espace {} {:?} : {} entités", m.name, m.kind, m.entities.len());
            for e in &m.entities {
                let s = format!("{:?}", e.kind);
                let n = match &e.kind {
                    cadkit_core::EntityKind::Polyline { vertices, .. } => vertices.len(),
                    cadkit_core::EntityKind::Spline { control_points, fit_points, .. } => {
                        control_points.len() + fit_points.len()
                    }
                    cadkit_core::EntityKind::Hatch { loops, .. } => loops.iter().map(|l| l.edges.len()).sum(),
                    cadkit_core::EntityKind::Mesh { faces, .. } => faces.len(),
                    _ => 0,
                };
                println!("   {} [{n}] {}", e.layer.as_deref().unwrap_or("-"), &s[..s.len().min(120)]);
            }
        }
    }
}
