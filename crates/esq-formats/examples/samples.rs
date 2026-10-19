//! Writes a small textured model in every export format, to check the files in other
//! programs: `cargo run -p esq-formats --example samples -- <folder>`.

use esq_core::glam::{DMat4, DVec3};
use esq_core::model::{Definition, Entities, Instance, Model, SceneCamera};
use esq_formats::{AllOptions, ExportCx, ExportFormat};

fn main() -> anyhow::Result<()> {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "samples".into()));
    std::fs::create_dir_all(&dir)?;
    let mut m = Model::new();
    let mut png = Vec::new();
    let img = image::RgbImage::from_fn(16, 16, |x, y| {
        if (x / 4 + y / 4) % 2 == 0 { image::Rgb([200, 60, 40]) } else { image::Rgb([240, 230, 210]) }
    });
    image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
    let brick = m.add_material("Brique", [200, 60, 40]);
    m.materials[brick].ext.texture = esq_formats::tex::texture_from_bytes("brique.png", png, Some([500.0, 500.0]));
    let blue = m.add_material("Bleu", [40, 80, 220]);
    let mut e = Entities::default();
    let s = 1000.0;
    let p = [DVec3::ZERO, DVec3::new(s, 0.0, 0.0), DVec3::new(s, s, 0.0), DVec3::new(0.0, s, 0.0)];
    let v: Vec<_> = p.iter().map(|x| e.add_vertex_raw(*x)).collect();
    let top: Vec<_> = p.iter().map(|x| e.add_vertex_raw(*x + DVec3::Z * s)).collect();
    e.add_face_raw(vec![v[0], v[3], v[2], v[1]], vec![]);
    let f = e.add_face_raw(top.clone(), vec![]).unwrap();
    e.faces[f].front = Some(brick);
    for i in 0..4 {
        let j = (i + 1) % 4;
        let f = e.add_face_raw(vec![v[i], v[j], top[j], top[i]], vec![]).unwrap();
        if i == 0 {
            e.faces[f].front = Some(brick);
        }
    }
    let d = m.defs.insert(Definition {
        name: "Boite".into(),
        is_group: false,
        description: String::new(),
        entities: e,
        attributes: Default::default(),
        ext: Default::default(),
    });
    for (x, mat) in [(0.0, None), (2000.0, Some(blue))] {
        m.root.instances.insert(Instance {
            def: d,
            xform: DMat4::from_translation(DVec3::new(x, 0.0, 0.0)) * DMat4::from_rotation_z(0.3),
            name: String::new(),
            material: mat,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
    }
    let o = AllOptions::default();
    let view = SceneCamera {
        eye: DVec3::new(-3000.0, -6000.0, 3000.0),
        target: DVec3::new(1500.0, 500.0, 500.0),
        up: DVec3::Z,
        fov_deg: 35.0,
        perspective: true,
    };
    // Every AutoCAD version.
    for v in esq_formats::options::AcadVersion::ALL {
        let mut a = o.clone();
        a.acad.version = *v;
        let cx = ExportCx { view: Some(view), name: "acad".into() };
        let _ = esq_formats::export(&m, &dir.join(format!("acad_{v:?}.dxf")), ExportFormat::Dxf, &a, &cx);
    }
    // Variants of some options.
    let mut flat = o.clone();
    flat.dae.preserve_hierarchies = false;
    flat.dae.two_sided = true;
    let cx = ExportCx { view: Some(view), name: "variante".into() };
    esq_formats::export(&m, &dir.join("variante.dae"), ExportFormat::Dae, &flat, &cx)?;
    for fmt in ExportFormat::ALL.into_iter().filter(|f| f.implemented()) {
        let path = dir.join(format!("exemple.{}", fmt.extension()));
        match esq_formats::export(&m, &path, fmt, &o, &ExportCx { view: Some(view), name: "exemple".into() }) {
            Ok(_) => println!("{}", path.display()),
            Err(e) => println!("{fmt:?} : {e:#}"),
        }
    }
    Ok(())
}
