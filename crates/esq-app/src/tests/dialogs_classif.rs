//! Model Info › Classifications: a schema loaded from a `.skc` file, shared with the Ruby API.

use std::path::{Path, PathBuf};

use super::dialogs::{open, ruby};
use crate::commands::Cmd;
use crate::dialogs::mi_classif::{import, read_schema, schemas};

/// A zip archive with stored (uncompressed) files.
fn zip(files: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, body) in files {
        let off = out.len() as u32;
        let (n, b) = (name.as_bytes(), body.as_bytes());
        let mut local = b"PK\x03\x04".to_vec();
        local.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        local.extend(0u32.to_le_bytes()); // crc (not checked)
        local.extend((b.len() as u32).to_le_bytes());
        local.extend((b.len() as u32).to_le_bytes());
        local.extend((n.len() as u16).to_le_bytes());
        local.extend(0u16.to_le_bytes());
        out.extend(&local);
        out.extend(n);
        out.extend(b);
        let mut c = b"PK\x01\x02".to_vec();
        c.extend([20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        c.extend(0u32.to_le_bytes());
        c.extend((b.len() as u32).to_le_bytes());
        c.extend((b.len() as u32).to_le_bytes());
        c.extend((n.len() as u16).to_le_bytes());
        c.extend([0u8; 12]);
        c.extend(off.to_le_bytes());
        c.extend(n);
        central.extend(c);
    }
    let cd_off = out.len() as u32;
    out.extend(&central);
    out.extend(b"PK\x05\x06");
    out.extend([0u8; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(cd_off.to_le_bytes());
    out.extend([0u8; 2]);
    out
}

fn sample_skc(dir: &Path) -> PathBuf {
    let p = dir.join("Essai.skc");
    let data = zip(&[
        ("documentProperties.xml", "<dp:documentProperties><dp:title>Essai 1.0</dp:title></dp:documentProperties>"),
        (
            "Schemas/essai.xsd",
            "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:e=\"http://exemple.org/essai\" \
             targetNamespace=\"http://exemple.org/essai\">\
             <xs:element substitutionGroup=\"e:Ouvrage\" name=\"Mur\"/>\
             <xs:element substitutionGroup=\"e:Element\" name=\"Ouvrage\"/></xs:schema>",
        ),
        ("Schemas/essai.xsd.filter", "Mur\nPorte\n// Blacklist\nInterdit\n"),
    ]);
    std::fs::write(&p, data).unwrap();
    p
}

#[test]
fn classifications_are_read_from_skc_files_and_shared_with_ruby() {
    let dir = std::env::temp_dir().join(format!("esq-classif-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let skc = sample_skc(&dir);
    let s = read_schema(&skc).unwrap();
    assert_eq!((s.name.as_str(), s.namespace.as_str()), ("Essai 1.0", "http://exemple.org/essai"));
    assert_eq!(s.types, vec!["Mur", "Porte", "Ouvrage", "Element"]);

    let (mut a, mut ui) = open(Cmd::ModelInfo);
    ui.click(&mut a, "Classements");
    assert!(ui.has("Aucun système de classement chargé."));
    // Import… (the file dialog is the system's; the import itself is what the button does).
    import(&mut a, &skc).unwrap();
    ui.frames(&mut a, 2);
    assert_eq!(ui.value("Classement Essai 1.0"), "4 types");
    if let Some(r) = ruby(
        &mut a,
        "m = Sketchup.active_model; d = m.definitions.add('Paroi'); \
         [m.classifications.keys, d.add_classification('Essai 1.0', 'Mur'), d.add_classification('Essai 1.0', 'Interdit')]",
    ) {
        assert_eq!(r, r#"[["Essai 1.0"], true, false]"#);
    }
    // Delete.
    assert!(ui.node("Supprimer").disabled);
    ui.click(&mut a, "Classement Essai 1.0");
    ui.click(&mut a, "Supprimer");
    assert!(schemas(&a.doc.model).is_empty());
    let _ = std::fs::remove_dir_all(&dir);

    // SketchUp's own IFC schema, when it is installed.
    let ifc = Path::new(r"C:\Program Files\SketchUp\SketchUp 2026\SketchUp\ShippedClassifications\IFC 2x3.skc");
    if ifc.is_file() {
        let s = read_schema(ifc).expect("IFC 2x3 lisible");
        assert!(
            s.name.contains("IFC") && s.types.iter().any(|t| t == "IfcWall"),
            "{} {:?}",
            s.name,
            &s.types[..5.min(s.types.len())]
        );
    }
}
