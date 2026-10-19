//! STL (stereolithography): SketchUp's STL importer and exporter. ASCII and binary files;
//! no units in the file, so the dialogs ask for them.

use std::fmt::Write as _;

use anyhow::{Result, bail};
use esq_core::glam::DVec3;
use esq_core::model::Model;

use crate::build::{BuildOpts, MeshIn, PolyIn, SceneIn};
use crate::options::{StlExport, StlFormat, StlImport};
use crate::scene::{Cache, Opts, for_each_world_face};

/// Reads an STL file (ASCII or binary) into one mesh per solid.
pub fn read(bytes: &[u8]) -> Result<SceneIn> {
    let mut scene = SceneIn::default();
    let binary_size = bytes.len() >= 84 && {
        let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        n.checked_mul(50).and_then(|x| x.checked_add(84)) == Some(bytes.len())
    };
    let ascii = !binary_size && bytes.trim_ascii_start().starts_with(b"solid");
    if ascii {
        let text = String::from_utf8_lossy(bytes);
        let mut mesh = MeshIn::default();
        let mut verts: Vec<DVec3> = Vec::new();
        let mut name = String::new();
        let flush = |scene: &mut SceneIn, mesh: &mut MeshIn, name: &str| {
            if !mesh.polys.is_empty() {
                let m = scene.add_mesh(std::mem::take(mesh));
                if name.is_empty() {
                    scene.root.meshes.push(m);
                } else {
                    scene.root.children.push(crate::build::NodeIn {
                        name: name.to_string(),
                        meshes: vec![m],
                        kind: crate::build::NodeKind::Flatten,
                        ..Default::default()
                    });
                }
            }
        };
        for line in text.lines() {
            let mut w = line.split_whitespace();
            match w.next() {
                Some("solid") => {
                    flush(&mut scene, &mut mesh, &name);
                    name = w.collect::<Vec<_>>().join(" ");
                }
                Some("vertex") => {
                    let c: Vec<f64> = w.filter_map(|x| x.parse().ok()).collect();
                    if c.len() < 3 {
                        bail!("sommet STL illisible : {line}");
                    }
                    verts.push(DVec3::new(c[0], c[1], c[2]));
                }
                Some("endfacet") => {
                    if verts.len() >= 3 {
                        let base = mesh.pos.len() as u32;
                        let n = verts.len() as u32;
                        mesh.pos.append(&mut verts);
                        mesh.polys.push(PolyIn { outer: (base..base + n).collect(), ..Default::default() });
                    }
                    verts.clear();
                }
                _ => {}
            }
        }
        flush(&mut scene, &mut mesh, "");
        if scene.meshes.is_empty() {
            bail!("aucune facette dans le fichier STL");
        }
    } else {
        if bytes.len() < 84 {
            bail!("fichier STL trop court");
        }
        let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        if bytes.len() < 84 + n * 50 {
            bail!("fichier STL binaire tronqué");
        }
        let mut mesh = MeshIn { pos: Vec::with_capacity(n * 3), ..Default::default() };
        for i in 0..n {
            let rec = &bytes[84 + i * 50..84 + i * 50 + 50];
            let f = |k: usize| f32::from_le_bytes(rec[k * 4..k * 4 + 4].try_into().unwrap()) as f64;
            let base = mesh.pos.len() as u32;
            for v in 0..3 {
                mesh.pos.push(DVec3::new(f(3 + v * 3), f(4 + v * 3), f(5 + v * 3)));
            }
            mesh.polys.push(PolyIn { outer: vec![base, base + 1, base + 2], ..Default::default() });
        }
        let m = scene.add_mesh(mesh);
        scene.root.meshes.push(m);
    }
    Ok(scene)
}

/// File › Import of an STL file.
pub fn import(bytes: &[u8], o: &StlImport) -> Result<Model> {
    let scene = read(bytes)?;
    Ok(crate::build::build(
        &scene,
        BuildOpts { scale: o.units.mm(), swap_yz: o.swap_yz, merge_coplanar: o.merge_coplanar, orient_faces: false },
    ))
}

/// The triangles written, in file units.
fn triangles(model: &Model, o: &StlExport) -> Vec<(DVec3, [DVec3; 3])> {
    let unit = o.units.len_unit().mm(model.units);
    let mut out = Vec::new();
    let mut cache = Cache::new(model, Opts::default());
    for_each_world_face(&mut cache, |_, f| {
        let pts: Vec<DVec3> = f.points().iter().map(|p| conv(*p, unit, o.swap_yz)).collect();
        let n = conv(f.normal(), 1.0, o.swap_yz).normalize_or_zero();
        for [a, b, c] in f.tris() {
            out.push((n, [pts[a], pts[b], pts[c]]));
        }
    });
    out
}

fn conv(p: DVec3, unit: f64, swap: bool) -> DVec3 {
    let p = p / unit;
    if swap { DVec3::new(p.x, p.z, -p.y) } else { p }
}

/// File › Export › 3D Model › STL.
pub fn export(model: &Model, name: &str, o: &StlExport) -> Vec<u8> {
    let tris = triangles(model, o);
    match o.format {
        StlFormat::Binary => {
            let mut buf = Vec::with_capacity(84 + tris.len() * 50);
            let mut header = [b' '; 80];
            let h = format!("SketchUp STL {name}");
            let hb = h.as_bytes();
            header[..hb.len().min(80)].copy_from_slice(&hb[..hb.len().min(80)]);
            buf.extend_from_slice(&header);
            buf.extend_from_slice(&(tris.len() as u32).to_le_bytes());
            for (n, t) in &tris {
                for v in std::iter::once(n).chain(t.iter()) {
                    for x in [v.x, v.y, v.z] {
                        buf.extend_from_slice(&(x as f32).to_le_bytes());
                    }
                }
                buf.extend_from_slice(&[0, 0]);
            }
            buf
        }
        StlFormat::Ascii => {
            let mut s = String::new();
            let _ = writeln!(s, "solid {name}");
            for (n, t) in &tris {
                let _ = writeln!(s, "facet normal {} {} {}", g(n.x), g(n.y), g(n.z));
                let _ = writeln!(s, "  outer loop");
                for v in t {
                    let _ = writeln!(s, "    vertex {} {} {}", g(v.x), g(v.y), g(v.z));
                }
                let _ = writeln!(s, "  endloop");
                let _ = writeln!(s, "endfacet");
            }
            let _ = writeln!(s, "endsolid {name}");
            s.into_bytes()
        }
    }
}

/// A number the way C++ streams print it by default (6 significant digits).
pub fn g(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return "0".into();
    }
    let e = x.abs().log10().floor() as i32;
    if !(-4..6).contains(&e) {
        let s = format!("{:.5e}", x);
        let (m, ex) = s.split_once('e').unwrap();
        let m = m.trim_end_matches('0').trim_end_matches('.');
        let ex: i32 = ex.parse().unwrap();
        return format!("{m}e{}{:02}", if ex < 0 { '-' } else { '+' }, ex.abs());
    }
    let decimals = (5 - e).max(0) as usize;
    let s = format!("{:.*}", decimals, x);
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::{StlImportUnit, StlUnit};
    use esq_core::model::Units;

    fn cube() -> Model {
        let mut m = Model::new();
        let e = &mut m.root;
        let p = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 10.0, 0.0),
            DVec3::new(0.0, 10.0, 0.0),
        ];
        let v: Vec<_> = p.iter().map(|x| e.add_vertex_raw(*x)).collect();
        let top: Vec<_> = p.iter().map(|x| e.add_vertex_raw(*x + DVec3::Z * 10.0)).collect();
        e.add_face_raw(vec![v[0], v[3], v[2], v[1]], vec![]);
        e.add_face_raw(top.clone(), vec![]);
        for i in 0..4 {
            let j = (i + 1) % 4;
            e.add_face_raw(vec![v[i], v[j], top[j], top[i]], vec![]);
        }
        m
    }

    #[test]
    fn binary_and_ascii_round_trip() {
        let m = cube();
        for format in [StlFormat::Binary, StlFormat::Ascii] {
            let o = StlExport { format, units: StlUnit::Millimeters, ..Default::default() };
            let bytes = export(&m, "cube", &o);
            if format == StlFormat::Binary {
                assert_eq!(bytes.len(), 84 + 12 * 50);
                assert!(bytes.starts_with(b"SketchUp STL cube"));
            } else {
                assert!(String::from_utf8_lossy(&bytes).starts_with("solid cube\nfacet normal"));
            }
            let back = import(
                &bytes,
                &StlImport { merge_coplanar: true, units: StlImportUnit::Millimeters, ..Default::default() },
            )
            .unwrap();
            // Coplanar triangles merge back into the six squares.
            assert_eq!(back.root.faces.len(), 6, "{format:?}");
            assert_eq!(back.root.edges.len(), 12);
            let no_merge = import(
                &bytes,
                &StlImport { merge_coplanar: false, units: StlImportUnit::Millimeters, ..Default::default() },
            )
            .unwrap();
            assert_eq!(no_merge.root.faces.len(), 12);
            // Outward normals are kept.
            let up = back.root.faces.values().filter(|f| f.normal.z > 0.99).count();
            assert_eq!(up, 1);
        }
    }

    #[test]
    fn units_and_swap() {
        let mut m = cube();
        m.units = Units::Centimeters;
        let o = StlExport { format: StlFormat::Ascii, units: StlUnit::Model, swap_yz: true, ..Default::default() };
        let text = String::from_utf8(export(&m, "c", &o)).unwrap();
        // 10 mm = 1 cm; z becomes y.
        assert!(text.contains("vertex 1 1 -1") || text.contains("vertex 1 1 -0"), "{text}");
        let back =
            import(text.as_bytes(), &StlImport { units: StlImportUnit::Meters, swap_yz: true, ..Default::default() })
                .unwrap();
        let max = back.root.verts.values().map(|v| v.pos.z).fold(f64::MIN, f64::max);
        assert!((max - 1000.0).abs() < 1e-6, "{max}");
    }

    #[test]
    fn numbers_print_like_cpp_streams() {
        assert_eq!(g(1.0), "1");
        assert_eq!(g(-0.5), "-0.5");
        assert_eq!(g(1234.5678), "1234.57");
        assert_eq!(g(1e7), "1e+07");
        assert_eq!(g(0.000_012_5), "1.25e-05");
    }
}
