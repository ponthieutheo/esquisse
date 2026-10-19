//! Builds a small model with the Esquisse engine and saves it as .skp:
//! `cargo run -p esq-skp --example demo_skp -- out.skp`

use esq_core::glam::{DMat4, DVec3};
use esq_core::model::{EntityRef, Model};

fn main() -> anyhow::Result<()> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "demo.skp".into());
    let mut m = Model::new();
    let wood = m.add_material("Chêne", [176, 128, 74]);
    let e = &mut m.root;
    // A table: top + 4 legs, as one component per leg.
    e.add_polyline(
        &[
            DVec3::new(0.0, 0.0, 720.0),
            DVec3::new(1600.0, 0.0, 720.0),
            DVec3::new(1600.0, 800.0, 720.0),
            DVec3::new(0.0, 800.0, 720.0),
        ],
        true,
        false,
    );
    let top = e.faces.keys().next().unwrap();
    let up = if e.faces[top].normal.z > 0.0 { 30.0 } else { -30.0 };
    e.push_pull(top, up, false);
    for f in e.faces.values_mut() {
        f.front = Some(wood);
    }
    let top_items: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
    m.make_group(None, &top_items, true, Some("Plateau".into()));

    let mut leg = Model::new();
    let l = &mut leg.root;
    l.add_polyline(
        &[DVec3::ZERO, DVec3::new(60.0, 0.0, 0.0), DVec3::new(60.0, 60.0, 0.0), DVec3::new(0.0, 60.0, 0.0)],
        true,
        false,
    );
    let f = l.faces.keys().next().unwrap();
    l.push_pull(f, -720.0, false);
    let def = m.import_as_component(&leg, "Pied");
    for (x, y) in [(40.0, 40.0), (1500.0, 40.0), (1500.0, 700.0), (40.0, 700.0)] {
        m.root.instances.insert(esq_core::model::Instance {
            def,
            xform: DMat4::from_translation(DVec3::new(x, y, 0.0)),
            name: String::new(),
            material: Some(wood),
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
    }
    let view = esq_skp::View {
        eye: DVec3::new(4200.0, -3300.0, 2600.0),
        target: DVec3::new(800.0, 400.0, 380.0),
        up: DVec3::Z,
        fov_deg: 35.0,
        perspective: true,
    };
    // Dimensions, a text and two scenes.
    use esq_core::model::{AnnotKind, Annotation, Scene};
    let dim = |a: DVec3, b: DVec3, offset: DVec3| Annotation {
        kind: AnnotKind::Linear { a, b, offset },
        text: String::new(),
        layer: 0,
        hidden: false,
        attach: Default::default(),
    };
    m.root.annots.insert(dim(
        DVec3::new(0.0, 0.0, 750.0),
        DVec3::new(1600.0, 0.0, 750.0),
        DVec3::new(0.0, -250.0, 0.0),
    ));
    m.root.annots.insert(dim(
        DVec3::new(1600.0, 0.0, 0.0),
        DVec3::new(1600.0, 0.0, 750.0),
        DVec3::new(250.0, 0.0, 0.0),
    ));
    m.root.annots.insert(Annotation {
        kind: AnnotKind::Text { point: DVec3::new(800.0, 400.0, 750.0), leader: DVec3::new(300.0, -200.0, 500.0) },
        text: "Plateau en chêne".into(),
        layer: 0,
        hidden: false,
        attach: Default::default(),
    });
    m.scenes.push(Scene {
        name: "Perspective".into(),
        description: String::new(),
        camera: view,
        hidden_layers: None,
        photo: None,
        ext: Default::default(),
    });
    m.scenes.push(Scene {
        name: "Face".into(),
        description: String::new(),
        camera: esq_skp::View {
            eye: DVec3::new(800.0, -5000.0, 400.0),
            target: DVec3::new(800.0, 400.0, 400.0),
            ..view
        },
        hidden_layers: None,
        photo: None,
        ext: Default::default(),
    });
    esq_skp::export_skp_view(&m, std::path::Path::new(&out), Some(&view))?;
    println!("écrit : {out}");
    Ok(())
}
