//! Window › Model Info, Window › Preferences and the other settings dialogs, driven like a
//! user does: open the window, click a page, pick values; then check that the setting is
//! stored where SketchUp (and the Ruby API) keeps it, and that it acts.

use esq_core::model::Units;
use esq_core::units::format_length;

use super::ui_driver::{Driver, app};
use crate::commands::Cmd;

pub(super) fn open(cmd: Cmd) -> (crate::app::EsquisseApp, Driver) {
    let mut a = app();
    let mut ui = Driver::new();
    let ctx = ui.ctx.clone();
    a.run(&ctx, cmd);
    ui.frames(&mut a, 3);
    (a, ui)
}

#[test]
fn model_info_units_page_sets_how_lengths_show_and_snap() {
    let (mut a, mut ui) = open(Cmd::ModelInfo);
    ui.click(&mut a, "Unités");
    assert_eq!(ui.value("Unité de longueur"), "Millimètres");
    assert_eq!(ui.value("Précision de longueur"), "0");

    ui.choose(&mut a, "Unité de longueur", "Mètres");
    assert_eq!(a.doc.model.units, Units::Meters);
    // A new unit starts at its template's precision.
    assert_eq!(ui.value("Précision de longueur"), "0,000");
    ui.choose(&mut a, "Précision de longueur", "0,00");
    let o = &a.doc.model.meta.options["UnitsOptions"];
    assert_eq!((o["LengthUnit"].as_i64(), o["LengthPrecision"].as_i64()), (Some(4), Some(2)));
    // Every length shown from now on follows (status bar, Entity Info, dimensions…).
    assert_eq!(format_length(1234.5, Units::Meters), "~1,23 m");

    ui.click(&mut a, "Afficher le format des unités");
    ui.frame(&mut a);
    assert_eq!(format_length(1250.0, Units::Meters), "1,25");
    ui.click(&mut a, "Afficher le format des unités");

    // Length snapping: on, every 5 cm.
    assert_eq!(ui.toggled("Activer l'ancrage de longueur"), Some(false));
    assert!(ui.node("Longueur d'ancrage").disabled);
    ui.click(&mut a, "Activer l'ancrage de longueur");
    assert!(ui.has("Longueur d'ancrage") && !ui.node("Longueur d'ancrage").disabled);
    ui.type_into(&mut a, "Longueur d'ancrage", "5cm");
    let d = esq_core::units::UnitsDisplay::of(&a.doc.model);
    assert!(d.length_snap);
    assert!((d.snap_length - 50.0).abs() < 1e-9, "{}", d.snap_length);
    assert_eq!(esq_core::units::snap_length(1234.0), 1250.0);

    // Angles: precision and snapping step.
    ui.choose(&mut a, "Précision des angles", "0,00");
    assert_eq!(esq_core::units::format_angle(12.5), "12,50°");
    ui.choose(&mut a, "Angle d'ancrage", "7,5");
    assert!((esq_core::units::angle_step().unwrap() - 7.5f64.to_radians()).abs() < 1e-12);

    // Architectural lengths are in inches, as in SketchUp.
    ui.choose(&mut a, "Format", "Architecture");
    assert_eq!(a.doc.model.units, Units::Inches);
    assert!(ui.node("Unité de longueur").disabled);
    assert_eq!(ui.value("Précision de longueur"), "1/16\"");
    assert_eq!(format_length(25.4 * 30.5, Units::Inches), "2' 6 1/2\"");
    ui.choose(&mut a, "Précision de longueur", "1/4\"");
    assert_eq!(format_length(25.4 * 30.6, Units::Inches), "~2' 6 1/2\"");

    // One undo step per change.
    let ctx = ui.ctx.clone();
    a.run(&ctx, Cmd::Undo);
    ui.frame(&mut a);
    assert_eq!(a.doc.model.units, Units::Inches);
    assert_eq!(esq_core::units::UnitsDisplay::of(&a.doc.model).precision, 4);
}

#[test]
fn length_snapping_rounds_a_line_drawn_along_an_axis() {
    use esq_core::glam::DVec3;
    use esq_core::units::{UnitsDisplay, set_display};
    let mut h = super::Harness::new();
    let d = UnitsDisplay { length_snap: true, snap_length: 100.0, ..UnitsDisplay::of(&h.doc.model) };
    set_display(Some(d));
    h.tool(crate::tools::ToolId::Line);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1234.0, 0.0, 0.0));
    let ents = h.doc.ents();
    let xs: Vec<f64> = ents.verts.values().map(|v| v.pos.x).collect();
    set_display(None);
    assert!(xs.iter().any(|x| (x - 1200.0).abs() < 1e-6), "{xs:?}");
}

/// Runs Ruby on the application's document; `None` without a Ruby runtime.
pub(super) fn ruby(a: &mut crate::app::EsquisseApp, code: &str) -> Option<String> {
    let i = super::ruby::interp()?;
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let r = crate::ruby_host::run_script(a, "Script Ruby", |m, s| i.run(code, "t.rb", m, s)).unwrap();
    assert!(r.ok, "erreur Ruby : {}", r.error);
    Some(r.result)
}

#[test]
fn units_settings_are_the_ones_the_ruby_api_reads() {
    let (mut a, mut ui) = open(Cmd::ModelInfo);
    ui.click(&mut a, "Unités");
    ui.choose(&mut a, "Unité de longueur", "Centimètres");
    ui.choose(&mut a, "Précision de longueur", "0,00");
    // Length#to_s uses the Windows regional settings, as SketchUp does.
    let Some(sep) = ruby(&mut a, "Sketchup::RegionalSettings.decimal_separator") else { return };
    let sep = sep.trim_matches('"').to_string();
    let r = ruby(
        &mut a,
        "o = Sketchup.active_model.options['UnitsOptions']; [o['LengthUnit'], o['LengthPrecision'], 10.mm.to_s]",
    )
    .unwrap();
    assert_eq!(r, format!(r#"[3, 2, "1{sep}00 cm"]"#));
    // And the other way: a script changes the precision, the page shows it.
    ruby(&mut a, "Sketchup.active_model.options['UnitsOptions']['LengthPrecision'] = 3").unwrap();
    ui.frames(&mut a, 2);
    assert_eq!(ui.value("Précision de longueur"), "0,000");
}
