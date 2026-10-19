//! A small scene to look at display effects (ambient occlusion, depth cue, photoreal
//! materials): `cargo run -p esq-skp --example fx_scene -- scene.skp`

use esq_core::glam::{DMat4, DVec3};
use esq_core::model::{Instance, Model};

fn block(w: f64, d: f64, h: f64) -> Model {
    let mut m = Model::new();
    let e = &mut m.root;
    e.add_polyline(
        &[DVec3::ZERO, DVec3::new(w, 0.0, 0.0), DVec3::new(w, d, 0.0), DVec3::new(0.0, d, 0.0)],
        true,
        false,
    );
    let f = e.faces.keys().next().unwrap();
    let up = if e.faces[f].normal.z > 0.0 { h } else { -h };
    e.push_pull(f, up, false);
    m
}

fn main() -> anyhow::Result<()> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "fx_scene.skp".into());
    let mut m = Model::new();
    let concrete = m.add_material("Béton", [170, 170, 165]);
    let wood = m.add_material("Chêne", [176, 128, 74]);
    let steel = m.add_material("Acier brossé", [190, 195, 200]);
    let plastic = m.add_material("Plastique rouge", [200, 40, 40]);
    let paint = m.add_material("Peinture blanche", [235, 235, 230]);
    let place = |m: &mut Model, w: f64, d: f64, h: f64, x: f64, y: f64, mat| {
        let def = m.import_as_component(&block(w, d, h), "Bloc");
        m.root.instances.insert(Instance {
            def,
            xform: DMat4::from_translation(DVec3::new(x, y, 0.0)),
            name: String::new(),
            material: Some(mat),
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
    };
    // Floor slab, back wall and side wall forming a corner, and objects close to each other.
    place(&mut m, 6000.0, 5000.0, 100.0, -500.0, -500.0, concrete);
    place(&mut m, 6000.0, 150.0, 2600.0, -500.0, 4350.0, paint);
    place(&mut m, 150.0, 5000.0, 2600.0, -500.0, -500.0, paint);
    place(&mut m, 800.0, 800.0, 900.0, 200.0, 3400.0, wood);
    place(&mut m, 600.0, 600.0, 600.0, 1200.0, 3600.0, steel);
    place(&mut m, 500.0, 500.0, 400.0, 1900.0, 3700.0, plastic);
    place(&mut m, 700.0, 500.0, 1800.0, 2700.0, 3750.0, wood);
    place(&mut m, 1500.0, 700.0, 450.0, 600.0, 1500.0, steel);
    place(&mut m, 400.0, 400.0, 450.0, 2300.0, 1800.0, plastic);
    let view = esq_skp::View {
        eye: DVec3::new(5200.0, -2800.0, 2300.0),
        target: DVec3::new(1400.0, 2400.0, 500.0),
        up: DVec3::Z,
        fov_deg: 40.0,
        perspective: true,
    };
    esq_skp::export_skp_view(&m, std::path::Path::new(&out), Some(&view))?;
    println!("écrit : {out}");
    Ok(())
}
