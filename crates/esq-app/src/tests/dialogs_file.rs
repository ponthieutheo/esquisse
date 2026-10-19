//! Model Info › File and Geo-location, driven like a user.

use super::dialogs::{open, ruby};
use crate::commands::Cmd;
use crate::dialogs::mi_file::{geolocated, redefine_thumbnail, solar_orientation};

#[test]
fn file_page_names_describes_and_aligns_the_model() {
    let (mut a, mut ui) = open(Cmd::ModelInfo);
    ui.click(&mut a, "Fichier");
    assert_eq!(ui.value("Emplacement"), "(Jamais enregistré)");
    ui.type_into(&mut a, "Nom du modèle", "Maison");
    assert_eq!(a.doc.model.meta.name, "Maison");
    // The description: typed, then the field is left.
    ui.focus(&mut a, "Description du modèle");
    ui.key(&mut a, eframe::egui::Key::A, eframe::egui::Modifiers::COMMAND);
    ui.text(&mut a, "Plan de la maison");
    ui.focus(&mut a, "Nom du modèle");
    assert_eq!(a.doc.model.meta.description, "Plan de la maison");

    // Glued to vertical faces, cutting an opening: a window.
    assert!(ui.node("Découper l’ouverture").disabled);
    ui.choose(&mut a, "Liaison", "Vertical");
    ui.click(&mut a, "Découper l’ouverture");
    let b = a.doc.model.behavior();
    assert!(b.is2d && b.snapto == 2 && b.cuts_opening);
    if let Some(r) = ruby(
        &mut a,
        "b = Sketchup.active_model.behavior; [b.is2d?, b.snapto, b.cuts_opening?, Sketchup.active_model.name]",
    ) {
        assert_eq!(r, r#"[true, 2, true, "Maison"]"#);
    }

    // Redefine the thumbnail on save: the view is kept when the file is written.
    ui.click(&mut a, "Redéfinir la vignette à l'enregistrement");
    assert!(redefine_thumbnail(&a.doc.model));
    let dir = std::env::temp_dir().join(format!("esq-file-page-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("maison.esq");
    a.save_to(&path).unwrap();
    assert!(a.doc.model.view_ext["file"].get("thumbnail_camera").is_some());
    ui.frames(&mut a, 2);
    assert_eq!(ui.value("Emplacement"), path.display().to_string());
    assert!(ui.value("Taille").ends_with("octets)"), "{}", ui.value("Taille"));
    // Inserted in another model, it is a component that glues and cuts.
    let src = esq_core::io::load_esq(&path).unwrap();
    let mut other = esq_core::model::Model::new();
    let d = other.import_as_component(&src, "maison");
    assert!(other.defs[d].ext.behavior.cuts_opening);
    assert_eq!(other.defs[d].description, "Plan de la maison");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn geolocation_page_shows_sets_and_clears_the_location() {
    let (mut a, mut ui) = open(Cmd::ModelInfo);
    ui.click(&mut a, "Géoposition");
    assert_eq!(ui.value("État de la géoposition"), "Ce modèle n'est pas géopositionné");
    assert!(!geolocated(&a.doc.model));
    // Set Manual Location shows the location fields.
    ui.click(&mut a, "Définir un emplacement manuel...");
    a.doc.commit("t", |m, _| {
        m.shadow.latitude = 48.8566;
        m.shadow.longitude = 2.3522;
        m.shadow.city = "Paris".into();
        m.shadow.country = "France".into();
    });
    ui.frames(&mut a, 2);
    assert_eq!(ui.value("État de la géoposition"), "Ce modèle est géopositionné manuellement");
    assert_eq!((ui.value("Latitude"), ui.value("Longitude")), ("48.8566N".into(), "2.3522E".into()));
    if let Some(r) = ruby(&mut a, "Sketchup.active_model.georeferenced?") {
        assert_eq!(r, "true");
    }
    // The sun follows the model's north unless "Use solar orientation" is off.
    a.doc.commit("t", |m, _| {
        m.shadow.north_angle = 90.0;
        m.shadow.display = true;
    });
    let with = a.lighting().sun.unwrap();
    ui.click(&mut a, "Utiliser l'orientation du soleil");
    assert!(!solar_orientation(&a.doc.model));
    let without = a.lighting().sun.unwrap();
    assert!(with.distance(without) > 0.1, "{with:?} / {without:?}");
    // Clear Location.
    ui.click(&mut a, "Effacer l'emplacement");
    assert!(!geolocated(&a.doc.model));
}
