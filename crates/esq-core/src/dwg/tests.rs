use super::objects::{Names, T_DIMSTYLE, T_LINE, T_MLINESTYLE, T_PLACEHOLDER, T_VPORT, fixed};
use super::*;
use cadkit_core::{Color, EntityKind, ReadOptions};

pub(super) fn sample() -> DwgDoc {
    let p = DVec3::new;
    let murs = DwgLayer::new("Murs", 1);
    let cachés = DwgLayer { name: "Cachés".into(), color: 5, visible: false };
    let red = DwgColor::Aci(1);
    let window = DwgBlock {
        name: "Fenêtre".into(),
        base: p(1.0, 2.0, 0.0),
        entities: vec![
            DwgEntity::new(DwgGeom::Line { a: p(1.0, 2.0, 0.0), b: p(2.0, 2.0, 0.0), thickness: 0.0 }, "0"),
            DwgEntity::new(DwgGeom::Line { a: p(2.0, 2.0, 0.0), b: p(2.0, 3.0, 0.5), thickness: 0.0 }, "Murs"),
            DwgEntity::new(
                DwgGeom::Face3d {
                    corners: [p(1.0, 2.0, 0.0), p(2.0, 2.0, 0.0), p(2.0, 3.0, 0.0), p(2.0, 3.0, 0.0)],
                    hidden: 0,
                },
                "0",
            ),
        ],
    };
    let frame_block = DwgBlock {
        name: "Cadre".into(),
        base: DVec3::ZERO,
        entities: vec![DwgEntity::new(
            DwgGeom::Insert {
                block: "Fenêtre".into(),
                pos: p(5.0, 0.0, 0.0),
                scale: DVec3::ONE,
                rotation: 0.0,
                normal: DVec3::Z,
            },
            "0",
        )],
    };
    let cube_v = vec![
        p(0.0, 0.0, 0.0),
        p(1.0, 0.0, 0.0),
        p(1.0, 1.0, 0.0),
        p(0.0, 1.0, 0.0),
        p(0.0, 0.0, 1.0),
        p(1.0, 0.0, 1.0),
        p(1.0, 1.0, 1.0),
        p(0.0, 1.0, 1.0),
    ];
    let cube_f = vec![[1, 2, 3, 4], [5, 6, 7, 8], [1, 2, 6, 5], [2, 3, 7, 6], [3, 4, 8, 7], [4, -1, 5, 0]];
    let mut ents = vec![
        DwgEntity {
            color: red,
            ..DwgEntity::new(DwgGeom::Line { a: p(0.0, 0.0, 0.0), b: p(100.0, 50.0, 0.0), thickness: 0.0 }, "Murs")
        },
        DwgEntity::new(DwgGeom::Line { a: p(1.5, -2.25, 3.0), b: p(1.5, 7.0, -3.0), thickness: 2.0 }, "0"),
        DwgEntity::new(
            DwgGeom::LwPolyline {
                pts: vec![DVec2::new(0.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(10.0, 5.5)],
                closed: true,
                width: 0.35,
                elevation: 0.0,
            },
            "Cachés",
        ),
        DwgEntity::new(
            DwgGeom::LwPolyline {
                pts: vec![DVec2::new(-1.0, 1.0), DVec2::new(-2.0, 3.0)],
                closed: false,
                width: 0.0,
                elevation: 4.0,
            },
            "0",
        ),
        DwgEntity::new(
            DwgGeom::Polyline3d { pts: vec![p(0.0, 0.0, 0.0), p(1.0, 2.0, 3.0), p(-4.0, 5.0, 6.0)], closed: true },
            "Murs",
        ),
        DwgEntity::new(
            DwgGeom::Face3d {
                corners: [p(0.0, 0.0, 1.0), p(3.0, 0.0, 1.0), p(3.0, 3.0, 2.0), p(0.0, 3.0, 2.0)],
                hidden: 0b0101,
            },
            "0",
        ),
        DwgEntity::new(DwgGeom::PolyFace { verts: cube_v.clone(), faces: cube_f.clone() }, "Murs"),
        DwgEntity {
            color: DwgColor::Rgb { rgb: [180, 90, 40], aci: 32 },
            ..DwgEntity::new(
                DwgGeom::Solid {
                    corners: [DVec2::new(0.0, 0.0), DVec2::new(1.0, 0.0), DVec2::new(0.5, 1.0), DVec2::new(0.5, 1.0)],
                    elevation: 0.0,
                },
                "0",
            )
        },
        DwgEntity::new(
            DwgGeom::Text {
                pos: p(10.0, 20.0, 0.0),
                align: p(12.0, 21.0, 0.0),
                height: 2.5,
                rotation: 0.25,
                halign: 1,
                valign: 2,
                text: "Façade € Ω".into(),
            },
            "0",
        ),
        DwgEntity::new(
            DwgGeom::Text {
                pos: p(1.0, 2.0, 3.0),
                align: DVec3::ZERO,
                height: 1.0,
                rotation: 0.0,
                halign: 0,
                valign: 0,
                text: "Simple".into(),
            },
            "0",
        ),
        DwgEntity::new(
            DwgGeom::Arc { center: p(5.0, 5.0, 0.0), normal: DVec3::Z, radius: 2.0, start: 0.0, end: 1.5 },
            "0",
        ),
        DwgEntity::new(DwgGeom::Circle { center: p(1.0, 2.0, 3.0), normal: DVec3::X, radius: 4.0 }, "0"),
        DwgEntity::new(DwgGeom::Point { p: p(7.0, 8.0, 9.0) }, "0"),
        DwgEntity::new(DwgGeom::XLine { p: p(0.0, 0.0, 0.0), dir: DVec3::X }, "0"),
        DwgEntity::new(
            DwgGeom::Insert {
                block: "Fenêtre".into(),
                pos: p(10.0, 0.0, 0.0),
                scale: DVec3::ONE,
                rotation: 0.0,
                normal: DVec3::Z,
            },
            "0",
        ),
        DwgEntity {
            color: DwgColor::ByBlock,
            ..DwgEntity::new(
                DwgGeom::Insert {
                    block: "fenêtre".into(),
                    pos: p(20.0, 0.0, 0.0),
                    scale: DVec3::splat(2.0),
                    rotation: 0.5,
                    normal: DVec3::Z,
                },
                "Murs",
            )
        },
        DwgEntity::new(
            DwgGeom::Insert {
                block: "Cadre".into(),
                pos: p(0.0, 30.0, 4.0),
                scale: p(1.0, 2.0, 3.0),
                rotation: -1.0,
                normal: DVec3::Y,
            },
            "Nouveau",
        ),
        DwgEntity::new(
            DwgGeom::Insert {
                block: "Cadre".into(),
                pos: p(3.0, 3.0, 3.0),
                scale: p(-1.5, 2.0, 3.0),
                rotation: 0.0,
                normal: DVec3::Z,
            },
            "0",
        ),
    ];
    // Enough lines for several object map chunks and several pages of objects.
    for i in 0..400 {
        let x = i as f64 * 0.37;
        ents.push(DwgEntity::new(DwgGeom::Line { a: p(x, 0.0, 0.0), b: p(x, 1.0e6 + x, 0.0), thickness: 0.0 }, "0"));
    }
    DwgDoc {
        layers: vec![murs, cachés],
        blocks: vec![window, frame_block],
        entities: ents,
        insunits: 4,
        font: "txt".into(),
    }
}

fn close(a: cadkit_core::Point3, b: DVec3) -> bool {
    (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9 && (a.z - b.z).abs() < 1e-9
}

fn cadkit_version(v: DwgVersion) -> cadkit_dwg::DwgVersion {
    cadkit_dwg::DwgVersion::from_magic(v.magic().as_bytes()).unwrap()
}

/// The versions cadkit reads: all but release 12.
const READ: [DwgVersion; 8] = [
    DwgVersion::R13,
    DwgVersion::R14,
    DwgVersion::R2000,
    DwgVersion::R2004,
    DwgVersion::R2010,
    DwgVersion::R2013,
    DwgVersion::R2018,
    DwgVersion::R2007,
];

#[test]
fn versions_follow_sketchup() {
    // `ExportAcadVersion` 0–8 of skp2acad.dll, and its labels in French.
    assert_eq!(DwgVersion::from_code(0), Some(DwgVersion::R12));
    assert_eq!(DwgVersion::from_code(3), Some(DwgVersion::R2000));
    assert_eq!(DwgVersion::from_code(8), Some(DwgVersion::R2018));
    assert_eq!(DwgVersion::from_code(9), None);
    assert_eq!(DwgVersion::default().code(), 3);
    let labels: Vec<_> = DwgVersion::ALL.iter().map(|v| v.label()).collect();
    assert_eq!(labels[..4], ["Version 12", "Version 13", "Version 14", "AutoCAD 2000"]);
    assert_eq!(labels[8], "AutoCAD 2018");
}

#[test]
fn cadkit_reads_every_object_back() {
    let doc = sample();
    for v in READ {
        let bytes = write(&doc, v).unwrap();
        if let Ok(dir) = std::env::var("ESQ_DWG_OUT") {
            std::fs::write(format!("{dir}/sample_{}.dwg", v.magic()), &bytes).unwrap();
        }
        assert_eq!(&bytes[..6], v.magic().as_bytes());
        let file = cadkit_dwg::native::read_native(&bytes, &ReadOptions::default()).unwrap();
        assert!(file.warnings.is_empty(), "{v:?}: {:#?}", file.warnings);
        assert_eq!(file.version, cadkit_version(v));
        assert!(file.header.complete, "{v:?}");
        assert_eq!(file.header.crc_ok, Some(true), "{v:?}");
        assert_eq!(file.classes_crc_ok, Some(true), "{v:?}");
        assert_eq!(file.classes.len(), if v >= DwgVersion::R2000 { 3 } else { 0 }, "{v:?}");
        assert_eq!(file.codepage, 30);
        assert_eq!(file.measurement, Some(1), "{v:?}");
        assert_eq!(file.header.int("INSUNITS"), if v >= DwgVersion::R2000 { Some(4) } else { None }, "{v:?}");
        let seed = file.header.int("HANDSEED").unwrap() as u64;
        // Types the reader decodes only partly (names of VPORT and DIMSTYLE, nothing of
        // MLINESTYLE and the placeholder).
        let partial = [T_VPORT, T_DIMSTYLE, T_MLINESTYLE, T_PLACEHOLDER];
        for (h, o) in &file.objects {
            assert!(*h < seed);
            assert_eq!(o.crc_ok, Some(true), "{v:?} {} {h:X}", o.type_name);
            assert!(o.error.is_none(), "{v:?} {} {h:X}: {:?}", o.type_name, o.error);
            if !partial.contains(&o.type_code) {
                // From 2007 the data ends with the bit that says there is no string stream.
                let flag = u64::from(v >= DwgVersion::R2007 && o.string_stream.is_none());
                assert_eq!(o.data_bits_left, Some(flag), "{v:?} {} {h:X}", o.type_name);
                assert!(o.handle_bits_left.unwrap() < 8, "{v:?} {} {h:X}", o.type_name);
            }
        }
        for name in
            ["BLOCK_CONTROL_OBJECT", "LAYER_CONTROL_OBJECT", "DICTIONARY_NAMED_OBJECTS", "BLOCK_RECORD_MODEL_SPACE"]
        {
            assert!(file.header.handle(name).is_some(), "{v:?} {name}");
        }
        // Every object the map lists was read; the map lists every handle below the seed
        // but 4 and those of the objects the release does not have.
        let missing = match v {
            DwgVersion::R13 | DwgVersion::R14 => 10,
            DwgVersion::R2000 => 4,
            DwgVersion::R2004 => 3,
            _ => 2,
        };
        assert_eq!(file.objects.len(), file.handle_map.len(), "{v:?}");
        assert_eq!(file.objects.len() as u64, seed - 1 - missing, "{v:?}");
    }
}

#[test]
fn cadkit_finds_the_drawing() {
    let doc = sample();
    for v in READ {
        // Names as the release writes them (in capitals, `_` for accents, up to release 14).
        let n = |s: &str| Names::new(v, &[]).add(s);
        let bytes = write(&doc, v).unwrap();
        let d = cadkit_dwg::read(&bytes, &ReadOptions::default()).unwrap();
        // The reader's model has no infinite lines: it only says so.
        assert!(
            d.warnings.iter().all(|w| w.code == "dwg.unsupported_entity" && w.message.contains("XLINE")),
            "{v:?}: {:#?}",
            d.warnings
        );
        assert_eq!(d.source.version, v.magic());
        // Layers.
        let layer = |n: &str| d.layers.iter().find(|l| l.name == n).unwrap_or_else(|| panic!("{v:?} {n}"));
        assert_eq!(d.layers.len(), 4);
        assert_eq!(layer(&n("0")).color, Color::Aci { index: 7 });
        assert_eq!(layer(&n("Murs")).color, Color::Aci { index: 1 });
        assert!(layer(&n("Murs")).visible);
        assert_eq!(layer(&n("Cachés")).color, Color::Aci { index: 5 });
        assert!(!layer(&n("Cachés")).visible, "{v:?}");
        assert!(layer(&n("Nouveau")).visible);
        // Model space, in order.
        let model = d.models.iter().find(|m| m.kind == cadkit_core::ModelKind::Model).unwrap();
        assert_eq!(model.entities.len(), doc.entities.len(), "{v:?}");
        let e = &model.entities;
        assert_eq!(e[0].layer.as_deref(), Some(n("Murs").as_str()));
        assert_eq!(e[0].color, Color::Aci { index: 1 });
        assert_eq!(e[1].color, Color::ByLayer);
        match &e[0].kind {
            EntityKind::Line { start, end } => {
                assert!(close(*start, DVec3::ZERO) && close(*end, DVec3::new(100.0, 50.0, 0.0)))
            }
            k => panic!("{k:?}"),
        }
        match &e[1].kind {
            EntityKind::Line { start, end } => {
                assert!(close(*start, DVec3::new(1.5, -2.25, 3.0)) && close(*end, DVec3::new(1.5, 7.0, -3.0)))
            }
            k => panic!("{k:?}"),
        }
        assert_eq!(e[1].props.get("dwg.thickness"), Some(&cadkit_core::Value::Float(2.0)), "{v:?}");
        match &e[2].kind {
            EntityKind::Polyline { vertices, closed, .. } => {
                assert!(*closed, "{v:?}");
                assert_eq!(vertices.len(), 3);
                assert!(close(vertices[2].position, DVec3::new(10.0, 5.5, 0.0)), "{v:?}");
                assert_eq!(vertices[1].start_width, 0.35, "{v:?}");
            }
            k => panic!("{v:?} {k:?}"),
        }
        assert_eq!(e[2].layer.as_deref(), Some(n("Cachés").as_str()));
        match &e[3].kind {
            EntityKind::Polyline { vertices, closed, .. } => {
                assert!(!*closed);
                assert!(close(vertices[1].position, DVec3::new(-2.0, 3.0, 4.0)), "{v:?} {:?}", vertices[1].position);
            }
            k => panic!("{k:?}"),
        }
        match &e[4].kind {
            EntityKind::Polyline { vertices, closed, .. } => {
                assert!(*closed);
                let got: Vec<_> = vertices.iter().map(|v| v.position).collect();
                assert_eq!(got.len(), 3);
                assert!(close(got[2], DVec3::new(-4.0, 5.0, 6.0)));
            }
            k => panic!("{k:?}"),
        }
        match &e[5].kind {
            EntityKind::Face { points, filled } => {
                assert!(!*filled);
                assert_eq!(points.len(), 4);
                assert!(close(points[2], DVec3::new(3.0, 3.0, 2.0)));
            }
            k => panic!("{k:?}"),
        }
        assert_eq!(e[5].props.get("dwg.invisible_edges"), Some(&cadkit_core::Value::Int(5)), "{v:?}");
        match &e[6].kind {
            EntityKind::Mesh { vertices, faces } => {
                assert_eq!(vertices.len(), 8);
                assert_eq!(faces.len(), 6);
                assert_eq!(faces[0], vec![0, 1, 2, 3]);
                assert_eq!(faces[5], vec![3, 0, 4]);
                assert!(close(vertices[6], DVec3::ONE));
            }
            k => panic!("{v:?} {k:?}"),
        }
        match &e[7].kind {
            EntityKind::Face { points, filled } => {
                assert!(*filled);
                assert_eq!(points.len(), 3);
                assert!(close(points[2], DVec3::new(0.5, 1.0, 0.0)));
            }
            k => panic!("{k:?}"),
        }
        // A true colour from 2004, its index before.
        let brick = if v >= DwgVersion::R2004 { Color::Rgb { r: 180, g: 90, b: 40 } } else { Color::Aci { index: 32 } };
        assert_eq!(e[7].color, brick, "{v:?}");
        match &e[8].kind {
            EntityKind::Text { position, height, rotation, value, halign, valign, style, .. } => {
                assert!(close(*position, DVec3::new(12.0, 21.0, 0.0)), "{v:?} {position:?}");
                assert_eq!((*height, *rotation), (2.5, 0.25));
                assert_eq!(value, "Façade € Ω", "{v:?}");
                assert_eq!((*halign, *valign), (cadkit_core::HAlign::Center, cadkit_core::VAlign::Middle));
                assert_eq!(style.as_deref(), Some(fixed(v, "Standard").as_str()), "{v:?}");
            }
            k => panic!("{k:?}"),
        }
        match &e[9].kind {
            EntityKind::Text { position, value, .. } => {
                assert!(close(*position, DVec3::new(1.0, 2.0, 3.0)), "{v:?} {position:?}");
                assert_eq!(value, "Simple");
            }
            k => panic!("{k:?}"),
        }
        match &e[10].kind {
            EntityKind::Arc { center, radius, start_angle, end_angle, .. } => {
                assert!(close(*center, DVec3::new(5.0, 5.0, 0.0)));
                assert_eq!((*radius, *start_angle, *end_angle), (2.0, 0.0, 1.5));
            }
            k => panic!("{k:?}"),
        }
        match &e[11].kind {
            EntityKind::Circle { center, radius, normal } => {
                assert!(close(*center, DVec3::new(1.0, 2.0, 3.0)), "{v:?} {center:?}");
                assert_eq!(*radius, 4.0);
                assert_eq!((normal.x, normal.y, normal.z), (1.0, 0.0, 0.0));
            }
            k => panic!("{k:?}"),
        }
        match &e[12].kind {
            EntityKind::Point { position } => assert!(close(*position, DVec3::new(7.0, 8.0, 9.0))),
            k => panic!("{k:?}"),
        }
        assert!(matches!(&e[13].kind, EntityKind::Unknown { type_name } if type_name == "dwg.XLINE"));
        match &e[14].kind {
            EntityKind::Insert { block, position, scale, rotation, .. } => {
                assert_eq!(*block, n("Fenêtre"));
                assert!(close(*position, DVec3::new(10.0, 0.0, 0.0)));
                assert_eq!((scale.x, scale.y, scale.z, *rotation), (1.0, 1.0, 1.0, 0.0));
            }
            k => panic!("{k:?}"),
        }
        assert_eq!(e[15].color, Color::ByBlock);
        match &e[15].kind {
            EntityKind::Insert { block, scale, rotation, .. } => {
                assert_eq!(*block, n("Fenêtre"));
                assert_eq!((scale.x, scale.y, scale.z, *rotation), (2.0, 2.0, 2.0, 0.5));
            }
            k => panic!("{k:?}"),
        }
        match &e[16].kind {
            EntityKind::Insert { block, position, scale, rotation, normal, .. } => {
                assert_eq!(*block, n("Cadre"));
                assert!(close(*position, DVec3::new(0.0, 30.0, 4.0)), "{position:?}");
                assert_eq!((scale.x, scale.y, scale.z, *rotation), (1.0, 2.0, 3.0, -1.0));
                assert_eq!((normal.x, normal.y, normal.z), (0.0, 1.0, 0.0));
            }
            k => panic!("{k:?}"),
        }
        assert_eq!(e[16].layer.as_deref(), Some(n("Nouveau").as_str()));
        match &e[17].kind {
            EntityKind::Insert { scale, .. } => assert_eq!((scale.x, scale.y, scale.z), (-1.5, 2.0, 3.0)),
            k => panic!("{k:?}"),
        }
        match &e[400 + 17].kind {
            EntityKind::Line { end, .. } => assert!(close(*end, DVec3::new(399.0 * 0.37, 1.0e6 + 399.0 * 0.37, 0.0))),
            k => panic!("{k:?}"),
        }
        // Blocks.
        let block = |n: &str| d.blocks.iter().find(|b| b.name == n).unwrap_or_else(|| panic!("{n}"));
        let w = block(&n("Fenêtre"));
        assert!(close(w.base_point, DVec3::new(1.0, 2.0, 0.0)));
        assert_eq!(w.entities.len(), 3);
        assert_eq!(w.entities[1].layer.as_deref(), Some(n("Murs").as_str()));
        match &w.entities[1].kind {
            EntityKind::Line { end, .. } => assert!(close(*end, DVec3::new(2.0, 3.0, 0.5))),
            k => panic!("{k:?}"),
        }
        assert!(matches!(w.entities[2].kind, EntityKind::Face { .. }));
        let c = block(&n("Cadre"));
        assert!(matches!(&c.entities[0].kind, EntityKind::Insert { block, .. } if *block == n("Fenêtre")));
        if v >= DwgVersion::R2000 {
            let paper = d.models.iter().find(|m| m.kind == cadkit_core::ModelKind::Layout).unwrap();
            assert_eq!(paper.name, "Layout1");
            assert!(paper.entities.is_empty());
        }
    }
}

#[test]
fn the_reader_notices_a_damaged_byte() {
    // The checks above are only worth something if the reader complains about a bad file.
    for v in [DwgVersion::R2000, DwgVersion::R2018] {
        let mut bytes = write(&sample(), v).unwrap();
        let file = cadkit_dwg::native::read_native(&bytes, &ReadOptions::default()).unwrap();
        let line = file.objects.values().find(|o| o.type_code == T_LINE).unwrap();
        if v == DwgVersion::R2000 {
            bytes[line.offset as usize + line.size as usize] ^= 0x10;
        } else {
            // The objects are compressed: damage the middle of the file.
            let n = bytes.len() / 2;
            bytes[n] ^= 0x10;
        }
        let file = cadkit_dwg::native::read_native(&bytes, &ReadOptions::default());
        assert!(file.is_err() || !file.unwrap().warnings.is_empty(), "{v:?}");
        let mut bytes = write(&sample(), v).unwrap();
        bytes[0x30] ^= 1;
        bytes[0x90] ^= 1;
        let file = cadkit_dwg::native::read_native(&bytes, &ReadOptions::default());
        assert!(file.is_err() || !file.unwrap().warnings.is_empty(), "{v:?}");
    }
}

#[test]
fn an_empty_drawing_is_valid() {
    for v in READ {
        let bytes = write(&DwgDoc { insunits: 1, ..DwgDoc::default() }, v).unwrap();
        let d = cadkit_dwg::read(&bytes, &ReadOptions::default()).unwrap();
        assert!(d.warnings.is_empty(), "{v:?}: {:#?}", d.warnings);
        assert_eq!(d.layers.len(), 1);
        assert!(d.blocks.is_empty());
        let file = cadkit_dwg::native::read_native(&bytes, &ReadOptions::default()).unwrap();
        assert_eq!(file.measurement, Some(0), "{v:?}");
    }
}

#[test]
fn bad_blocks_are_refused() {
    let ins = |b: &str| {
        DwgEntity::new(
            DwgGeom::Insert { block: b.into(), pos: DVec3::ZERO, scale: DVec3::ONE, rotation: 0.0, normal: DVec3::Z },
            "0",
        )
    };
    let doc = DwgDoc { entities: vec![ins("Absent")], ..DwgDoc::default() };
    assert!(write(&doc, DwgVersion::R2000).is_err());
    let b = |name: &str, entities| DwgBlock { name: name.into(), base: DVec3::ZERO, entities };
    let doc = DwgDoc { blocks: vec![b("A", vec![ins("B")]), b("B", vec![])], ..DwgDoc::default() };
    assert!(write(&doc, DwgVersion::R2018).is_err());
    let doc = DwgDoc { blocks: vec![b("A", vec![]), b("a", vec![])], ..DwgDoc::default() };
    assert!(write(&doc, DwgVersion::R13).is_err());
}

/// Layers, blocks and model space as cadkit reads them, whatever the format.
fn summary(d: &cadkit_core::Document) -> String {
    let ents = |es: &[cadkit_core::Entity]| -> Vec<String> {
        es.iter().map(|e| format!("{:?} {:?} {:?}", e.layer, e.color, e.kind)).collect()
    };
    let mut layers: Vec<_> = d.layers.iter().map(|l| (&l.name, l.color, l.visible)).collect();
    layers.sort_by_key(|l| l.0.clone());
    let mut blocks: Vec<_> = d.blocks.iter().map(|b| (&b.name, b.base_point, ents(&b.entities))).collect();
    blocks.sort_by_key(|b| b.0.clone());
    let models: Vec<_> = d.models.iter().filter(|m| !m.entities.is_empty()).map(|m| ents(&m.entities)).collect();
    format!("{layers:#?}\n{blocks:#?}\n{models:#?}")
}

/// cadkit reads back from the DXF of every release the drawing it reads from the DWG of the
/// same release (release 12: of release 13, less the infinite line it cannot have).
#[test]
fn cadkit_reads_the_dxf_of_every_release() {
    let doc = sample();
    for v in DwgVersion::ALL {
        let bytes = write_dxf(&doc, v).unwrap();
        if let Ok(dir) = std::env::var("ESQ_DWG_OUT") {
            std::fs::write(format!("{dir}/sample_{}.dxf", v.magic()), &bytes).unwrap();
        }
        let d = cadkit_dxf::read(&bytes, &ReadOptions::default()).unwrap();
        // The reader only says it has no model of infinite lines.
        assert!(d.warnings.iter().all(|w| w.message.contains("XLINE")), "{v:?}: {:#?}", d.warnings);
        assert_eq!(d.warnings.len(), usize::from(v > DwgVersion::R12), "{v:?}");
        assert_eq!(d.source.version, v.magic());
        let dwg = cadkit_dwg::read(&write(&doc, v.max(DwgVersion::R13)).unwrap(), &ReadOptions::default()).unwrap();
        let want = summary(&dwg).replace("dwg.XLINE", "dxf.XLINE");
        let want: String =
            want.lines().filter(|l| v > DwgVersion::R12 || !l.contains("XLINE")).map(|l| format!("{l}\n")).collect();
        let got: String = summary(&d).lines().map(|l| format!("{l}\n")).collect();
        assert_eq!(got, want, "{v:?}");
    }
}

/// Checks what SketchUp's importer made of the sample, and returns it with the names of
/// release 14 to compare releases.
#[cfg(windows)]
fn check_import(
    v: DwgVersion,
    got: Option<Result<teigha::Import, String>>,
    guide_lines: usize,
) -> Option<teigha::Import> {
    let Some(got) = got else {
        eprintln!("SketchUp 2026 absent : pas de second lecteur");
        return None;
    };
    let mut got = got.unwrap_or_else(|e| panic!("{v:?}: {e}"));
    let n = |s: &str| Names::new(v, &[]).add(s);
    // 400 lines + 2 lines + polylines and faces of the model, the window block with its
    // two lines and its triangle, the frame inserting it.
    assert!(got.model.edges > 400, "{v:?}: {got:?}");
    assert_eq!(got.model.guide_lines, guide_lines, "{v:?}: {got:?}");
    assert_eq!(got.model.guide_points, 1, "{v:?}: {got:?}");
    assert_eq!(got.model.instances, 4, "{v:?}: {got:?}");
    assert!(got.layers.contains(&(n("Murs"), true)), "{v:?}: {got:?}");
    assert!(got.layers.contains(&(n("Cachés"), false)), "{v:?}: {got:?}");
    let def = |s: &str| got.defs.iter().find(|d| d.0 == n(s)).map(|d| d.1);
    assert_eq!(def("Fenêtre").map(|c| (c.edges, c.faces)), Some((4, 1)), "{v:?}: {got:?}");
    assert_eq!(def("Cadre").map(|c| c.instances), Some(1), "{v:?}: {got:?}");
    let r14 = |s: &mut String| *s = Names::new(DwgVersion::R14, &[]).add(s);
    got.layers.iter_mut().for_each(|l| r14(&mut l.0));
    got.defs.iter_mut().for_each(|d| r14(&mut d.0));
    for g in &mut got.geometry {
        if let Some((name, rest)) = g.split_once(' ') {
            *g = format!("{} {rest}", Names::new(DwgVersion::R14, &[]).add(name));
        }
    }
    Some(got)
}

/// SketchUp's importer (the ODA's libraries) reads every release back, DWG and DXF: the
/// same edges, faces, components and tags whatever the version (names aside).
#[cfg(windows)]
#[test]
fn sketchup_imports_every_version() {
    // In inches: files before 2000 have no units, which the importer takes for inches.
    let doc = DwgDoc { insunits: 1, ..sample() };
    let mut first: Option<teigha::Import> = None;
    for (v, ext) in DwgVersion::ALL.iter().rev().flat_map(|v| [(*v, "dwg"), (*v, "dxf")]) {
        let bytes = if ext == "dwg" { write(&doc, v) } else { write_dxf(&doc, v) }.unwrap();
        if let Ok(dir) = std::env::var("ESQ_DWG_OUT") {
            std::fs::write(format!("{dir}/sample_{}.{ext}", v.magic()), &bytes).unwrap();
        }
        // Release 12 has no infinite lines.
        let xlines = usize::from(v > DwgVersion::R12);
        let file = format!("sample_{}.{ext}", v.magic());
        let Some(got) = check_import(v, teigha::import(&bytes, &file), xlines) else { return };
        match &first {
            None => first = Some(got),
            Some(f) => {
                let want = teigha::Import { model: teigha::Counts { guide_lines: xlines, ..f.model }, ..f.clone() };
                assert_eq!(got, want, "{file}");
            }
        }
    }
}

/// SketchUp's importer checks the CRCs of release 12: the checks above are worth something.
#[cfg(windows)]
#[test]
fn sketchup_refuses_a_damaged_release_12() {
    let good = write(&sample(), DwgVersion::R12).unwrap();
    if teigha::import(&good, "intact_r12.dwg").is_none() {
        return;
    }
    // A coordinate of the first line, then the CRC of the header.
    for (at, name) in [(0x6CF + 20, "entite_r12.dwg"), (0x6BD, "entete_r12.dwg")] {
        let mut bad = good.clone();
        bad[at] ^= 0x40;
        assert!(teigha::import(&bad, name).unwrap().is_err(), "{name}");
    }
}

/// The records of a release 12 file follow one another with their sizes and CRCs: the
/// entities of model space and of the blocks, the symbol tables, the auxiliary header.
#[test]
fn release_12_records_are_whole() {
    let b = write(&sample(), DwgVersion::R12).unwrap();
    let u16_at = |at: usize| u16::from_le_bytes([b[at], b[at + 1]]) as usize;
    let u32_at = |at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize;
    assert_eq!(&b[..6], b"AC1009");
    assert_eq!(crc16(0xC0C1, &b[..0x6BD]) as usize, u16_at(0x6BD));
    // Entities: type, flags, size; the size ends with the CRC.
    let walk = |from: usize, to: usize| -> Vec<u8> {
        let mut kinds = Vec::new();
        let mut p = from;
        while p < to {
            let size = u16_at(p + 2);
            assert_eq!(crc16(0xC0C1, &b[p..p + size - 2]) as usize, u16_at(p + size - 2), "{p:X}");
            kinds.push(b[p]);
            p += size;
        }
        assert_eq!(p, to);
        kinds
    };
    let (start, end) = (u32_at(0x14), u32_at(0x18));
    let model = walk(start, end);
    // LINE LINE POLYLINE+VERTEX×3+SEQEND (2D) POLYLINE+3+SEQEND (3D) 3DFACE POLYLINE+8+6+SEQEND
    // SOLID TEXT TEXT ARC CIRCLE POINT (no XLINE) INSERT×4, then the 400 lines.
    let mut want = vec![1, 1, 19, 20, 20, 20, 17, 19, 20, 20, 17, 19, 20, 20, 20, 17, 22, 19];
    want.extend([20; 14]);
    want.extend([17, 11, 7, 7, 8, 3, 2, 14, 14, 14, 14]);
    want.extend([1; 400]);
    assert_eq!(model, want);
    // The second text: "Simple" at (1, 2) and elevation 3.
    let mut p = start;
    for _ in 0..want.iter().position(|k| *k == 7).unwrap() + 1 {
        p += u16_at(p + 2);
    }
    assert_eq!(b[p + 1] & 4, 4, "elevation");
    let data = &b[p + 8..p + u16_at(p + 2) - 2];
    let text_at = data.windows(6).position(|w| w == b"Simple").unwrap();
    assert_eq!(u16::from_le_bytes([data[text_at - 2], data[text_at - 1]]), 6);
    // Blocks: *PAPER_SPACE, then the window and the frame, each BLOCK … ENDBLK.
    let blocks_start = u32_at(0x1C);
    let blocks_size = u32_at(0x20) & 0x3FFF_FFFF;
    let kinds = walk(blocks_start, blocks_start + blocks_size);
    assert_eq!(kinds, [12, 13, 12, 1, 1, 22, 13, 12, 14, 13]);
    // Tables: records of their size, each with its CRC.
    for at in [0x2C, 0x36, 0x40, 0x4A, 0x54, 0x3EF, 0x500, 0x512, 0x522, 0x69F] {
        let (size, count, addr) = (u16_at(at), u16_at(at + 2), u32_at(at + 6));
        for r in 0..count {
            let rec = &b[addr + r * size..addr + (r + 1) * size];
            assert_eq!(
                crc16(0xC0C1, &rec[..size - 2]) as usize,
                u16::from_le_bytes([rec[size - 2], rec[size - 1]]) as usize
            );
        }
        if count > 0 {
            // Between an opening sentinel and its complement.
            let (open, close) = (&b[addr - 16..addr], &b[addr + count * size..addr + count * size + 16]);
            assert!(open.iter().zip(close).all(|(x, y)| x ^ y == 0xFF), "{at:X}");
        }
    }
    // Layers: "0", Murs, Cachés (off: negative colour), Nouveau.
    let (size, addr) = (u16_at(0x36), u32_at(0x36 + 6));
    let layer = |i: usize| {
        let r = &b[addr + i * size..];
        let name = r[1..33].split(|c| *c == 0).next().unwrap().to_vec();
        (String::from_utf8(name).unwrap(), i16::from_le_bytes([r[35], r[36]]))
    };
    assert_eq!(layer(2), ("CACH_S".to_string(), -5));
    // The auxiliary header closes the file.
    let aux = b.len() - 16 - 2 - 136;
    assert_eq!(crc16(0xC0C1, &b[aux..aux + 136]) as usize, u16_at(aux + 136));
    assert_eq!(u32_at(aux + 4), start);
}
