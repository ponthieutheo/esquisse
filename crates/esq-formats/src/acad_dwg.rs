//! File › Export › 3D Model › DWG (and the entities of the DXF): the drawing of
//! [`acad::document`] written by `esq_core::dwg` in the version the options name (Version
//! 12 to AutoCAD 2018, the list of SketchUp's exporter).
//!
//! The entities map one to one, except what the entity set of `esq_core::dwg` lacks:
//! multiline texts become one TEXT per line, leaders 3D polylines and dimensions their
//! lines and text; polyface meshes of more than 32 767 vertices or faces are split.
//! Material colours are true colours from 2004, their nearest colour index before.

use std::collections::HashMap;

use anyhow::Result;
use esq_core::dwg::{DwgBlock, DwgColor, DwgDoc, DwgEntity, DwgGeom, DwgLayer};
use esq_core::glam::DVec3;
use esq_core::model::{Model, Units};

use crate::acad::{CadDoc, CadEnt, CadItem, document, insert_params, nearest_aci};
use crate::options::AcadExport;

/// Most vertices or faces of one polyface mesh.
const MESH_MAX: usize = 32_767;

fn insunits(u: Units) -> i16 {
    match u {
        Units::Inches => 1,
        Units::Feet => 2,
        Units::Millimeters => 4,
        Units::Centimeters => 5,
        Units::Meters => 6,
        Units::Yards => 10,
    }
}

/// Faces of a mesh in groups that fit one polyface each, with their own vertices.
fn split_mesh(verts: &[DVec3], faces: &[[i32; 4]]) -> Vec<(Vec<DVec3>, Vec<[i32; 4]>)> {
    if verts.len() <= MESH_MAX && faces.len() <= MESH_MAX {
        return vec![(verts.to_vec(), faces.to_vec())];
    }
    let mut out = Vec::new();
    let (mut vs, mut fs, mut map) = (Vec::new(), Vec::new(), HashMap::new());
    for f in faces {
        let new = f.iter().filter(|i| **i != 0 && !map.contains_key(&i.unsigned_abs())).count();
        if vs.len() + new > MESH_MAX || fs.len() == MESH_MAX {
            out.push((std::mem::take(&mut vs), std::mem::take(&mut fs)));
            map.clear();
        }
        let mut g = [0i32; 4];
        for (k, &i) in f.iter().enumerate() {
            if i == 0 {
                continue;
            }
            let Some(&p) = verts.get(i.unsigned_abs() as usize - 1) else { continue };
            let n = *map.entry(i.unsigned_abs()).or_insert_with(|| {
                vs.push(p);
                vs.len() as i32
            });
            g[k] = if i < 0 { -n } else { n };
        }
        fs.push(g);
    }
    if !fs.is_empty() {
        out.push((vs, fs));
    }
    out
}

/// A dimension's length as its default text: up to two decimals.
fn length_text(l: f64) -> String {
    let s = format!("{l:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn text(pos: DVec3, height: f64, rotation: f64, halign: u8, valign: u8, s: &str) -> DwgGeom {
    DwgGeom::Text { pos, align: pos, height, rotation, halign, valign, text: s.to_string() }
}

/// The DWG entities of one drawing entity.
fn geoms(e: &CadEnt, text_height: f64) -> Vec<DwgGeom> {
    match e {
        CadEnt::Line { a, b } => vec![DwgGeom::Line { a: *a, b: *b, thickness: 0.0 }],
        CadEnt::Polyline3d { pts, closed } => vec![DwgGeom::Polyline3d { pts: pts.clone(), closed: *closed }],
        CadEnt::Arc { center, normal, radius, start, end } => {
            vec![DwgGeom::Arc { center: *center, normal: *normal, radius: *radius, start: *start, end: *end }]
        }
        CadEnt::Circle { center, normal, radius } => {
            vec![DwgGeom::Circle { center: *center, normal: *normal, radius: *radius }]
        }
        CadEnt::PolyFace { verts, faces } => {
            split_mesh(verts, faces).into_iter().map(|(verts, faces)| DwgGeom::PolyFace { verts, faces }).collect()
        }
        CadEnt::Point { p } => vec![DwgGeom::Point { p: *p }],
        CadEnt::XLine { p, dir } => vec![DwgGeom::XLine { p: *p, dir: *dir }],
        CadEnt::MText { p, text: s, height } => {
            // Top left, one line under the other at AutoCAD's spacing (5/3 of the height).
            s.replace("\\P", "\n")
                .lines()
                .enumerate()
                .map(|(i, line)| text(*p - DVec3::Y * (*height * 5.0 / 3.0 * i as f64), *height, 0.0, 0, 3, line))
                .collect()
        }
        CadEnt::Leader { pts } => vec![DwgGeom::Polyline3d { pts: pts.clone(), closed: false }],
        CadEnt::Dimension { a, b, line, text: s } => {
            // The dimension line through `line`, its extension lines and its text above.
            let a2 = *a + (*line - *b);
            let b2 = *line;
            let d = b2 - a2;
            let rotation = if d.truncate().length() > 1e-9 { d.y.atan2(d.x) } else { 0.0 };
            let rotation = if rotation.abs() > std::f64::consts::FRAC_PI_2 + 1e-9 {
                rotation - std::f64::consts::PI.copysign(rotation)
            } else {
                rotation
            };
            let label = if s.is_empty() { length_text(a.distance(*b)) } else { s.clone() };
            let mid = (a2 + b2) / 2.0;
            let mut out = vec![DwgGeom::Line { a: a2, b: b2, thickness: 0.0 }];
            for (p, q) in [(*a, a2), (*b, b2)] {
                if p.distance(q) > 1e-9 {
                    out.push(DwgGeom::Line { a: p, b: q, thickness: 0.0 });
                }
            }
            let up = DVec3::new(-rotation.sin(), rotation.cos(), 0.0) * (text_height * 0.5);
            out.push(text(mid + up, text_height, rotation, 1, 1, &label));
            out
        }
        CadEnt::Insert { block, xform } => match insert_params(xform) {
            Some((_, scale, rotation, normal)) => {
                vec![DwgGeom::Insert { block: block.clone(), pos: xform.w_axis.truncate(), scale, rotation, normal }]
            }
            None => Vec::new(),
        },
    }
}

fn entities(items: &[CadItem], text_height: f64) -> Vec<DwgEntity> {
    let mut out = Vec::new();
    for it in items {
        // The material's colour: true colour from 2004, its nearest index before.
        let color = it.color.map_or(DwgColor::ByLayer, |c| DwgColor::Rgb { rgb: c, aci: nearest_aci(c) });
        for geom in geoms(&it.ent, text_height) {
            out.push(DwgEntity { geom, layer: it.layer.clone(), color });
        }
    }
    out
}

/// The DWG of a drawing.
pub fn dwg_doc(doc: &CadDoc) -> DwgDoc {
    let text_height = 120.0 / doc.units.factor();
    DwgDoc {
        layers: doc
            .layers
            .iter()
            .map(|l| DwgLayer { name: l.name.clone(), color: l.color.map_or(7, nearest_aci), visible: l.visible })
            .collect(),
        blocks: doc
            .blocks
            .iter()
            .map(|(name, items)| DwgBlock {
                name: name.clone(),
                base: DVec3::ZERO,
                entities: entities(items, text_height),
            })
            .collect(),
        entities: entities(&doc.model, text_height),
        insunits: insunits(doc.units),
        font: "Arial.ttf".into(),
    }
}

/// File › Export › 3D Model › DWG.
pub fn export_dwg(model: &Model, path: &std::path::Path, o: &AcadExport) -> Result<()> {
    let bytes = esq_core::dwg::write(&dwg_doc(&document(model, o)), o.version.dwg())?;
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::glam::DMat4;
    use esq_core::model::{Definition, Entities, Instance};

    fn model() -> Model {
        let mut m = Model::new();
        m.units = Units::Millimeters;
        let mut e = Entities::default();
        let v: Vec<_> = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 500.0), (0.0, 500.0)]
            .iter()
            .map(|(x, y)| e.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
            .collect();
        e.add_face_raw(v, vec![]);
        let pts: Vec<DVec3> = (0..24)
            .map(|i| {
                let a = i as f64 * std::f64::consts::TAU / 24.0;
                DVec3::new(3000.0 + 400.0 * a.cos(), 400.0 * a.sin(), 0.0)
            })
            .collect();
        e.add_polyline(&pts, true, true);
        let red = m.add_material("Rouge", [255, 0, 0]);
        let tag = m.add_layer("Mobilier");
        let d = m.defs.insert(Definition {
            name: "Table".into(),
            is_group: false,
            description: String::new(),
            entities: e,
            attributes: Default::default(),
            ext: Default::default(),
        });
        m.root.instances.insert(Instance {
            def: d,
            xform: DMat4::from_translation(DVec3::new(0.0, 2000.0, 0.0)) * DMat4::from_rotation_z(0.5),
            name: String::new(),
            material: Some(red),
            layer: tag,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
        m.root.guides.insert(esq_core::model::Guide::line(DVec3::ZERO, DVec3::X));
        m
    }

    #[test]
    fn the_dwg_reads_back_through_the_importer() {
        let m = model();
        let o = AcadExport { construction: true, ..Default::default() };
        let bytes = esq_core::dwg::write(&dwg_doc(&document(&m, &o)), o.version.dwg()).unwrap();
        // The reader: no warning except that it has no model of infinite lines.
        let d = cadkit_dwg::read(&bytes, &cadkit_core::ReadOptions::default()).unwrap();
        assert!(d.warnings.iter().all(|w| w.message.contains("XLINE")), "{:#?}", d.warnings);
        assert!(d.layers.iter().any(|l| l.name == "Mobilier"));
        let table = d.blocks.iter().find(|b| b.name == "Table").unwrap();
        let mesh = table.entities.iter().find(|e| matches!(e.kind, cadkit_core::EntityKind::Mesh { .. })).unwrap();
        assert_eq!(mesh.color, cadkit_core::Color::Aci { index: 1 });
        assert!(table.entities.iter().any(
            |e| matches!(e.kind, cadkit_core::EntityKind::Circle { radius, .. } if (radius - 400.0).abs() < 1e-6)
        ));
        match &d.models[0]
            .entities
            .iter()
            .find(|e| matches!(e.kind, cadkit_core::EntityKind::Insert { .. }))
            .unwrap()
            .kind
        {
            cadkit_core::EntityKind::Insert { block, position, rotation, .. } => {
                assert_eq!(block, "Table");
                assert!((position.y - 2000.0).abs() < 1e-9 && (rotation - 0.5).abs() < 1e-12);
            }
            _ => unreachable!(),
        }
        // Through the importer: the table comes back as a component with a 1 m × 0.5 m face.
        let (imp, _) = crate::cad::import(
            &bytes,
            &crate::options::DwgImport { preserve_origin: true, ..Default::default() },
            Units::Millimeters,
        )
        .unwrap();
        let def = imp.defs.values().find(|d| d.name == "Table").unwrap();
        let f = def.entities.faces.keys().next().unwrap();
        let area = esq_core::tess::face_area(&def.entities, f);
        assert!((area - 500_000.0).abs() < 1.0, "{area}");
    }

    /// Layers, blocks and model space as the reader sees them.
    pub(crate) fn summary(d: &cadkit_core::Document) -> String {
        let ents = |es: &[cadkit_core::Entity]| -> Vec<String> {
            es.iter().map(|e| format!("{:?} {:?} {:?}", e.layer, e.color, e.kind)).collect()
        };
        let layers: Vec<_> = d.layers.iter().map(|l| (&l.name, l.color, l.visible)).collect();
        let blocks: Vec<_> = d.blocks.iter().map(|b| (&b.name, b.base_point, ents(&b.entities))).collect();
        let models: Vec<_> = d.models.iter().filter(|m| !m.entities.is_empty()).map(|m| ents(&m.entities)).collect();
        format!("{layers:?}\n{blocks:?}\n{models:?}")
    }

    /// File › Export › 3D Model › DWG writes the version of the options, with the same
    /// layers, blocks and entities in every one, names as the release writes them (release
    /// 12, which cadkit does not read, is read back by the tests of `esq_core::dwg`).
    #[test]
    fn every_version_writes_the_same_drawing() {
        let m = model();
        let dir = std::env::temp_dir().join(format!("esq_acad_dwg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // The drawing of the model, made once: the direction of the polygon a circle is
        // found in, hence the sign of its normal, can change from one drawing to the next.
        let doc = dwg_doc(&document(&m, &AcadExport { construction: true, ..Default::default() }));
        // The table's colour: true colour from 2004, its index before.
        let (mut true_colour, mut index): (Option<String>, Option<String>) = (None, None);
        for v in crate::options::AcadVersion::ALL.iter().rev() {
            let last = if *v >= crate::options::AcadVersion::V2004 { &mut true_colour } else { &mut index };
            let o = AcadExport { version: *v, construction: true, ..Default::default() };
            let path = dir.join(format!("{v:?}.dwg"));
            export_dwg(&m, &path, &o).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(&bytes[..6], v.dwg().magic().as_bytes(), "{v:?}");
            if *v == crate::options::AcadVersion::R12 {
                continue;
            }
            let bytes = esq_core::dwg::write(&doc, v.dwg()).unwrap();
            let d = cadkit_dwg::read(&bytes, &cadkit_core::ReadOptions::default()).unwrap();
            assert!(d.warnings.iter().all(|w| w.message.contains("XLINE")), "{v:?}: {:#?}", d.warnings);
            assert_eq!(d.source.version, v.dwg().magic());
            let s = summary(&d);
            let name = |n: &str| format!("\"{}\"", v.dwg().table_name(n));
            assert!(s.contains(&name("Mobilier")) && s.contains(&name("Table")), "{s}");
            match last.as_ref() {
                None => *last = Some(s),
                Some(f) => {
                    let want = f
                        .replace("Mobilier", &v.dwg().table_name("Mobilier"))
                        .replace("Table", &v.dwg().table_name("Table"));
                    assert_eq!(s, want, "{v:?}");
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn big_meshes_are_split_and_texts_kept() {
        let verts: Vec<DVec3> = (0..40_000).map(|i| DVec3::new(i as f64, 0.0, (i % 7) as f64)).collect();
        let faces: Vec<[i32; 4]> = (1..39_999).step_by(2).map(|i| [i, -(i + 1), i + 2, 0]).collect();
        let parts = split_mesh(&verts, &faces);
        assert!(parts.len() >= 2);
        assert_eq!(parts.iter().map(|p| p.1.len()).sum::<usize>(), faces.len());
        for (vs, fs) in &parts {
            assert!(vs.len() <= MESH_MAX && fs.len() <= MESH_MAX);
            assert!(fs.iter().flatten().all(|i| i.unsigned_abs() as usize <= vs.len()));
        }
        // The first face keeps its corners and its hidden edge.
        let (vs, fs) = &parts[0];
        assert_eq!(fs[0], [1, -2, 3, 0]);
        assert_eq!(vs[2], verts[2]);
        let t = geoms(&CadEnt::MText { p: DVec3::ZERO, text: "Un\\PDeux".into(), height: 3.0 }, 1.0);
        assert_eq!(t.len(), 2);
        assert!(matches!(&t[1], DwgGeom::Text { pos, text, .. } if text == "Deux" && pos.y == -5.0));
        let dim = CadEnt::Dimension {
            a: DVec3::ZERO,
            b: DVec3::new(1000.0, 0.0, 0.0),
            line: DVec3::new(1000.0, 300.0, 0.0),
            text: String::new(),
        };
        let g = geoms(&dim, 10.0);
        assert_eq!(g.len(), 4);
        assert!(matches!(&g[3], DwgGeom::Text { text, .. } if text == "1000"));
    }
}
